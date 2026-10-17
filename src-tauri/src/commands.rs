use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{command, AppHandle, Emitter, State};
use tracing::{info, warn};

use crate::error::AppError;
use crate::models::{
    Board, BoardItem, BoardNote, BookmarkRecord, CreateBoardNotePayload, CreateBoardPayload,
    CreateBookmarkPayload, GetImagesPayload, ImageDetail, ImageRecord, ThumbProgress,
    TimelineSummary, UpdateBoardItemPayload, UpdateBoardNotePayload, UpdateBoardPayload,
    WatchedFolder, WindowState, PlatformInfo, PresetFolder,
};
use crate::pipeline::JobPriority;
use crate::scanner::walker::{is_sub_directory, normalize_path};
use crate::state::AppState;

/// 監視対象フォルダを追加し走査を開始する
///
/// 変更理由: 仕様書§2.2(F-01)「既登録フォルダの子を追加しようとした場合は拒否。親を追加した場合は既存の子を削除して親で再走査」
#[command]
pub async fn add_watch_folder(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    path: String,
) -> Result<WatchedFolder, AppError> {
    let normalized = normalize_path(&path);
    let path_obj = Path::new(&normalized);

    if !path_obj.exists() || !path_obj.is_dir() {
        return Err(AppError::invalid_argument("指定されたディレクトリが存在しません"));
    }

    // 既存フォルダ一覧を取得
    let existing = {
        let conn = state.db.reader()?;
        crate::db::repo::get_watched_folders(&conn)?
    };

    // 1. 既登録フォルダの子を追加しようとした場合は拒否
    for folder in &existing {
        if is_sub_directory(&folder.path, &normalized) {
            return Err(AppError::invalid_argument(format!(
                "指定されたフォルダは既に登録済みの親フォルダ [{}] の配下です",
                folder.path
            )));
        }
    }

    // 2. 親を追加した場合は既存の子の登録を削除
    let mut children_to_remove = Vec::new();
    for folder in &existing {
        if is_sub_directory(&normalized, &folder.path) {
            children_to_remove.push(folder.id);
        }
    }

    let watched_folder = {
        let conn = state.db.writer();
        for child_id in children_to_remove {
            crate::db::repo::remove_watched_folder(&conn, child_id)?;
        }
        let now = chrono::Utc::now().timestamp();
        crate::db::repo::add_watched_folder(&conn, &normalized, now)?
    };

    // バックグラウンドで走査を実行
    let state_clone = Arc::clone(&state);
    let folder_id = watched_folder.id;
    let norm_clone = normalized.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(e) = crate::scanner::scan_folder(app, state_clone, folder_id, norm_clone) {
            warn!("Error during folder scanning: {:?}", e);
        }
    });

    Ok(watched_folder)
}

/// 監視フォルダを解除
///
/// 変更理由: 仕様書§2.2(F-01)解除時はDBレコードのみ削除
#[command]
pub async fn remove_watch_folder(
    state: State<'_, Arc<AppState>>,
    id: i64,
) -> Result<(), AppError> {
    let conn = state.db.writer();
    crate::db::repo::remove_watched_folder(&conn, id)?;
    state.bump_catalog_version();
    Ok(())
}

/// 登録済み監視フォルダ一覧を取得
#[command]
pub async fn get_watch_folders(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<WatchedFolder>, AppError> {
    let conn = state.db.reader()?;
    let folders = crate::db::repo::get_watched_folders(&conn)?;
    Ok(folders)
}

/// フォルダの再走査を実行
#[command]
pub async fn rescan(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    folder_id: Option<i64>,
) -> Result<(), AppError> {
    let folders = {
        let conn = state.db.reader()?;
        crate::db::repo::get_watched_folders(&conn)?
    };

    let target_folders: Vec<WatchedFolder> = match folder_id {
        Some(fid) => folders.into_iter().filter(|f| f.id == fid).collect(),
        None => folders,
    };

    // 再走査前にプラグインディレクトリの最新状態を再読込
    // 変更理由: アプリ起動中に plugins フォルダへ追加・更新された独自形式プラグイン（.clip等）を
    // 再走査時に即時認識し、該当ファイルが走査対象から脱落するのを防止するため。
    let settings = crate::settings::load_settings(&state.data_dir);
    state.format_plugins.reload(&settings.enabled_plugins, &state.data_dir);

    let state_clone = Arc::clone(&state);
    tauri::async_runtime::spawn_blocking(move || {
        for folder in target_folders {
            let _ = crate::scanner::scan_folder(
                app.clone(),
                Arc::clone(&state_clone),
                folder.id,
                folder.path,
            );
        }
    });

    Ok(())
}

/// タイムライン全体サマリを取得（日別件数サマリと総件数）
///
/// 変更理由: 仕様書§8.1全レコードを取得しなくてもスクロール全長を確定させるため
#[command]
pub async fn get_timeline_summary(
    state: State<'_, Arc<AppState>>,
    folder_id: Option<i64>,
    sort: Option<crate::models::TimelineSort>,
) -> Result<TimelineSummary, AppError> {
    let current_version = state.current_catalog_version();
    let sort_val = sort.unwrap_or(crate::models::TimelineSort::TakenAtDesc);

    // インメモリキャッシュをまず確認（SQLiteクエリのバイパス）
    if let Some(cached) = state.get_cached_timeline_summary(folder_id, sort_val, current_version) {
        return Ok(cached);
    }

    let conn = state.db.reader()?;
    let summary = crate::db::repo::get_timeline_summary(&conn, folder_id, current_version, Some(sort_val))?;
    state.put_cached_timeline_summary(folder_id, sort_val, current_version, summary.clone());
    Ok(summary)
}

/// タイムライン用の画像一覧をページング取得
#[command]
pub async fn get_timeline_images(
    state: State<'_, Arc<AppState>>,
    payload: GetImagesPayload,
) -> Result<Vec<ImageRecord>, AppError> {
    let limit = payload.limit.clamp(1, 500);
    let conn = state.db.reader()?;
    let images = crate::db::repo::get_timeline_images(
        &conn,
        payload.offset,
        limit,
        payload.folder_id,
        payload.sort,
    )?;
    Ok(images)
}


/// 単一画像の詳細情報を取得
#[command]
pub async fn get_image_detail(
    state: State<'_, Arc<AppState>>,
    id: i64,
) -> Result<ImageDetail, AppError> {
    let conn = state.db.reader()?;
    let detail = crate::db::repo::get_image_detail(&conn, id)?
        .ok_or_else(|| AppError::not_found("画像が見つかりません"))?;
    Ok(detail)
}

/// 画面外近傍画像のサムネイルを優先度 MID で事前生成キューに登録
///
/// 変更理由: 仕様書§7「② prefetch コマンド（画面外近傍）→ 優先度 MID」
#[command]
pub async fn prefetch_thumbnails(
    state: State<'_, Arc<AppState>>,
    ids: Vec<i64>,
) -> Result<(), AppError> {
    for id in ids {
        if let Ok(conn) = state.db.reader() {
            if let Ok(Some((path, hash, _, _))) = crate::db::repo::get_image_file_info(&conn, id) {
                state.thumb_pipeline.enqueue(
                    id,
                    PathBuf::from(path),
                    hash,
                    1,
                    None,
                    None,
                    JobPriority::Mid,
                );
            }
        }
    }
    Ok(())
}

/// OSのファイルマネージャ（エクスプローラ）で該当ファイルを表示
///
/// 変更理由: 仕様書§2.2(F-12)「OSのファイルマネージャで該当ファイルを表示」
#[command]
pub async fn reveal_in_file_manager(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    id: i64,
) -> Result<(), AppError> {
    let path_str = {
        let conn = state.db.reader()?;
        crate::db::repo::get_image_file_info(&conn, id)?
            .map(|(p, _, _, _)| p)
            .ok_or_else(|| AppError::not_found("画像が見つかりません"))?
    };

    #[cfg(target_os = "android")]
    {
        crate::android_native::open_folder(&app, &path_str)?;
        Ok(())
    }
    #[cfg(not(target_os = "android"))]
    {
        use tauri_plugin_opener::OpenerExt;
        app.opener()
            .reveal_item_in_dir(path_str)
            .map_err(|e| AppError::internal(format!("エクスプローラ表示に失敗: {}", e)))?;
        Ok(())
    }
}

/// 表示範囲（ビューポート）の画像ID群を登録し、優先度付きパイプラインの世代を進める
///
/// 変更理由: 高速な表示レスポンスを実現するため、画面内（High）およびその周辺（Mid）を最優先で生成し、
/// 画面外の古い世代の要求を自動破棄する
#[command]
pub async fn set_viewport(
    state: State<'_, Arc<AppState>>,
    visible_ids: Vec<i64>,
    nearby_ids: Vec<i64>,
) -> Result<serde_json::Value, AppError> {
    let conn = state.db.reader()?;
    let visible = crate::db::repo::get_thumb_sources(&conn, &visible_ids)?;
    let nearby = crate::db::repo::get_thumb_sources(&conn, &nearby_ids)?;
    let gen = state.thumb_pipeline.set_viewport(&visible, &nearby);
    Ok(serde_json::json!({
        "generation": gen,
        "visibleCount": visible.len(),
        "nearbyCount": nearby.len()
    }))
}

/// サムネイル生成に失敗した画像の一覧を取得
#[command]
pub async fn get_failed_thumbnails(
    state: State<'_, Arc<AppState>>,
    limit: Option<usize>,
) -> Result<Vec<crate::models::FailedImageRecord>, AppError> {
    let conn = state.db.reader()?;
    let failed = crate::db::repo::get_failed_thumb_images(&conn, limit.unwrap_or(100))?;
    Ok(failed)
}

/// 直近のログ一覧を取得
#[command]
pub async fn get_logs(
    limit: Option<usize>,
) -> Result<Vec<crate::models::LogEntry>, AppError> {
    if let Some(manager) = crate::logger::get_log_manager() {
        Ok(manager.get_recent_logs(limit.unwrap_or(200)))
    } else {
        Ok(Vec::new())
    }
}

/// ログ保存先ディレクトリをOSのファイルマネージャで開く
#[command]
pub async fn open_log_folder(
    app: AppHandle,
) -> Result<serde_json::Value, AppError> {
    if let Some(manager) = crate::logger::get_log_manager() {
        let dir = manager.log_dir();
        let dir_str = dir.to_string_lossy().to_string();

        #[cfg(target_os = "android")]
        {
            let _ = crate::android_native::open_folder(&app, &dir_str);
        }
        #[cfg(not(target_os = "android"))]
        {
            use tauri_plugin_opener::OpenerExt;
            let _ = app.opener().reveal_item_in_dir(&dir_str);
        }

        Ok(serde_json::json!({ "success": true, "path": dir_str }))
    } else {
        Err(AppError::internal("ロガーが初期化されていません"))
    }
}

/// サムネイル生成キューをクリアする
#[command]
pub async fn clear_queue(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value, AppError> {
    let gen = state.thumb_pipeline.clear_queue();
    Ok(serde_json::json!({
        "generation": gen,
        "status": "cleared"
    }))
}

/// 未生成サムネイルを再走査してキューに登録する
#[command]
pub async fn rescan_missing_thumbnails(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value, AppError> {
    let writer = state.db.writer();
    let reset_count = crate::db::repo::reset_failed_thumbnails(&writer).unwrap_or(0);
    state.thumb_pipeline.trigger_background_refill();
    state.init_thumb_progress();
    state.emit_thumb_progress_throttled();
    info!("Reset thumbnail failure flags (Targets: {})", reset_count);
    Ok(serde_json::json!({
        "success": true,
        "resetCount": reset_count
    }))
}

/// サムネイル生成進捗状況を取得（インメモリアトミックカウンタからO(1)即時取得）
///
/// 変更理由: 全画像テーブルに対するCOUNT(*)×3回のフルスキャンを廃止し、
/// メモリ上のAtomicU64カウンタから即座に返却してDBクエリ負荷を解消する。
#[command]
pub async fn get_thumb_progress(state: State<'_, Arc<AppState>>) -> Result<ThumbProgress, AppError> {
    let total = state.thumb_total.load(std::sync::atomic::Ordering::Relaxed);
    let raw_done = state.thumb_done.load(std::sync::atomic::Ordering::Relaxed);
    let raw_failed = state.thumb_failed.load(std::sync::atomic::Ordering::Relaxed);

    let done = raw_done.min(total);
    let failed = raw_failed.min(total.saturating_sub(done));

    Ok(ThumbProgress {
        done,
        failed,
        total,
    })
}

/// ウィンドウ状態設定ファイルのパスを取得
fn get_window_state_path() -> PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        return PathBuf::from(appdata)
            .join("com.imagemediaviewer.desktop")
            .join("window_state.json");
    }
    if let Some(data_dir) = dirs::data_dir() {
        return data_dir
            .join("com.imagemediaviewer.desktop")
            .join("window_state.json");
    }
    PathBuf::from("window_state.json")
}

/// ウィンドウ状態を取得
#[command]
pub async fn get_window_state() -> Result<WindowState, AppError> {
    let path = get_window_state_path();
    if let Ok(content) = std::fs::read_to_string(&path) {
        if let Ok(state) = serde_json::from_str::<WindowState>(&content) {
            return Ok(state);
        }
    }
    Ok(WindowState::default())
}

/// ウィンドウ状態を保存
#[command]
pub async fn save_window_state(state: WindowState) -> Result<(), AppError> {
    let path = get_window_state_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let json = serde_json::to_string_pretty(&state)
        .map_err(|e| AppError::internal(format!("ウィンドウ状態のシリアライズに失敗: {}", e)))?;
    std::fs::write(&path, json)
        .map_err(|e| AppError::internal(format!("ウィンドウ状態の保存に失敗: {}", e)))?;
    Ok(())
}

/// ムードボード一覧を取得
#[command]
pub async fn get_boards(state: State<'_, Arc<AppState>>) -> Result<Vec<Board>, AppError> {
    let conn = state.db.reader()?;
    let boards = crate::db::repo::get_boards(&conn)?;
    Ok(boards)
}

/// ムードボードを新規作成
#[command]
pub async fn create_board(
    state: State<'_, Arc<AppState>>,
    payload: CreateBoardPayload,
) -> Result<Board, AppError> {
    let writer = state.db.writer();
    let now = chrono::Utc::now().timestamp();
    let board = crate::db::repo::create_board(&writer, &payload, now)?;
    Ok(board)
}

/// ムードボードを更新
#[command]
pub async fn update_board(
    state: State<'_, Arc<AppState>>,
    id: i64,
    payload: UpdateBoardPayload,
) -> Result<serde_json::Value, AppError> {
    let writer = state.db.writer();
    let now = chrono::Utc::now().timestamp();
    crate::db::repo::update_board(&writer, id, &payload, now)?;
    Ok(serde_json::json!({ "success": true }))
}

/// ムードボードを削除
#[command]
pub async fn delete_board(
    state: State<'_, Arc<AppState>>,
    id: i64,
) -> Result<serde_json::Value, AppError> {
    let writer = state.db.writer();
    crate::db::repo::delete_board(&writer, id)?;
    Ok(serde_json::json!({ "success": true }))
}

/// ボード内の画像アイテム一覧を取得
#[command]
pub async fn get_board_items(
    state: State<'_, Arc<AppState>>,
    board_id: i64,
) -> Result<Vec<BoardItem>, AppError> {
    let conn = state.db.reader()?;
    let items = crate::db::repo::get_board_items(&conn, board_id)?;
    Ok(items)
}

/// ボードに画像アイテムを追加
#[command]
pub async fn add_board_items(
    state: State<'_, Arc<AppState>>,
    board_id: i64,
    image_ids: Vec<i64>,
) -> Result<Vec<BoardItem>, AppError> {
    let writer = state.db.writer();
    let now = chrono::Utc::now().timestamp();
    let items = crate::db::repo::add_board_items(&writer, board_id, &image_ids, now)?;
    Ok(items)
}

/// ボード内の画像アイテム（位置・サイズ等）を更新
#[command]
pub async fn update_board_item(
    state: State<'_, Arc<AppState>>,
    id: i64,
    mut payload: UpdateBoardItemPayload,
) -> Result<serde_json::Value, AppError> {
    payload.id = id;
    let writer = state.db.writer();
    let now = chrono::Utc::now().timestamp();
    crate::db::repo::update_board_item(&writer, &payload, now)?;
    Ok(serde_json::json!({ "success": true }))
}

/// ボード内の画像アイテムを削除
#[command]
pub async fn delete_board_item(
    state: State<'_, Arc<AppState>>,
    id: i64,
) -> Result<serde_json::Value, AppError> {
    let writer = state.db.writer();
    crate::db::repo::delete_board_item(&writer, id)?;
    Ok(serde_json::json!({ "success": true }))
}

/// ボード内のテキストメモ一覧を取得
#[command]
pub async fn get_board_notes(
    state: State<'_, Arc<AppState>>,
    board_id: i64,
) -> Result<Vec<BoardNote>, AppError> {
    let conn = state.db.reader()?;
    let notes = crate::db::repo::get_board_notes(&conn, board_id)?;
    Ok(notes)
}

/// ボードにテキストメモを新規作成
#[command]
pub async fn create_board_note(
    state: State<'_, Arc<AppState>>,
    board_id: i64,
    mut payload: CreateBoardNotePayload,
) -> Result<BoardNote, AppError> {
    payload.board_id = board_id;
    let writer = state.db.writer();
    let now = chrono::Utc::now().timestamp();
    let note = crate::db::repo::create_board_note(&writer, &payload, now)?;
    Ok(note)
}

/// ボード内のテキストメモを更新
#[command]
pub async fn update_board_note(
    state: State<'_, Arc<AppState>>,
    id: i64,
    mut payload: UpdateBoardNotePayload,
) -> Result<serde_json::Value, AppError> {
    payload.id = id;
    let writer = state.db.writer();
    let now = chrono::Utc::now().timestamp();
    crate::db::repo::update_board_note(&writer, &payload, now)?;
    Ok(serde_json::json!({ "success": true }))
}

/// ボード内のテキストメモを削除
#[command]
pub async fn delete_board_note(
    state: State<'_, Arc<AppState>>,
    id: i64,
) -> Result<serde_json::Value, AppError> {
    let writer = state.db.writer();
    crate::db::repo::delete_board_note(&writer, id)?;
    Ok(serde_json::json!({ "success": true }))
}

/// 栞（ブックマーク）一覧を取得
#[command]
pub async fn get_bookmarks(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<BookmarkRecord>, AppError> {
    let conn = state.db.reader()?;
    let bookmarks = crate::db::repo::get_bookmarks(&conn)?;
    Ok(bookmarks)
}

/// 栞を新規作成
#[command]
pub async fn create_bookmark(
    state: State<'_, Arc<AppState>>,
    payload: CreateBookmarkPayload,
) -> Result<BookmarkRecord, AppError> {
    let writer = state.db.writer();
    let now = chrono::Utc::now().timestamp();
    let bm = crate::db::repo::create_bookmark(&writer, &payload, now)?;
    Ok(bm)
}

/// 栞を削除
#[command]
pub async fn delete_bookmark(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<serde_json::Value, AppError> {
    let writer = state.db.writer();
    crate::db::repo::delete_bookmark(&writer, &id)?;
    Ok(serde_json::json!({ "success": true }))
}

/// 生存確認用ハートビートコマンド
#[command]
pub async fn heartbeat() -> Result<serde_json::Value, AppError> {
    Ok(serde_json::json!({ "alive": true }))
}

/// 現在の言語設定を取得
#[command]
pub async fn get_locale(state: State<'_, Arc<AppState>>) -> Result<String, AppError> {
    let settings = crate::settings::load_settings(&state.data_dir);
    Ok(settings.locale)
}

/// 言語設定を更新し、タスクトレイメニューも同期更新する
#[command]
pub async fn set_locale(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    locale: String,
) -> Result<String, AppError> {
    let mut settings = crate::settings::load_settings(&state.data_dir);
    settings.locale = locale.clone();
    crate::settings::save_settings(&state.data_dir, &settings)
        .map_err(|e| AppError::io(format!("設定の保存に失敗しました: {}", e)))?;

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    crate::tray::update_tray_locale(&app, &locale);
    #[cfg(any(target_os = "android", target_os = "ios"))]
    let _ = &app;

    info!("Language updated: locale = {}", locale);
    Ok(locale)
}

/// プラットフォーム情報および端末内の推奨画像フォルダ候補を取得する
///
/// 変更理由: モバイル（Android）環境におけるフォルダ選択ダイアログ非対応（Folder picker is not implemented on mobile）への対策。
/// OSに応じたプリセットフォルダ（DCIM/Camera, Pictures等）をフロントエンドへ提示し、ワンタップで監視対象に追加可能にする。
///
/// @return プラットフォーム情報とプリセットフォルダ候補のリスト
#[command]
pub async fn get_platform_info() -> Result<PlatformInfo, AppError> {
    #[cfg(target_os = "android")]
    let os = "android".to_string();
    #[cfg(target_os = "windows")]
    let os = "windows".to_string();
    #[cfg(target_os = "macos")]
    let os = "macos".to_string();
    #[cfg(target_os = "linux")]
    let os = "linux".to_string();
    #[cfg(not(any(target_os = "android", target_os = "windows", target_os = "macos", target_os = "linux")))]
    let os = "unknown".to_string();

    #[cfg(any(target_os = "android", target_os = "ios"))]
    let is_mobile = true;
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let is_mobile = false;

    let mut presets = Vec::new();

    #[cfg(target_os = "android")]
    {
        let candidates = [
            ("📷 カメラ (Camera)", "/storage/emulated/0/DCIM/Camera"),
            ("📁 DCIM 全体", "/storage/emulated/0/DCIM"),
            ("🖼️ ピクチャ (Pictures)", "/storage/emulated/0/Pictures"),
            ("📥 ダウンロード (Download)", "/storage/emulated/0/Download"),
            ("💾 内部ストレージ", "/storage/emulated/0"),
        ];
        for (name, path) in candidates {
            let exists = Path::new(path).exists();
            presets.push(PresetFolder {
                name: name.to_string(),
                path: path.to_string(),
                exists,
            });
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(pic_dir) = dirs::picture_dir() {
            presets.push(PresetFolder {
                name: "ピクチャ (Pictures)".to_string(),
                path: pic_dir.to_string_lossy().to_string(),
                exists: pic_dir.exists(),
            });
        }
        if let Some(down_dir) = dirs::download_dir() {
            presets.push(PresetFolder {
                name: "ダウンロード (Download)".to_string(),
                path: down_dir.to_string_lossy().to_string(),
                exists: down_dir.exists(),
            });
        }
    }

    Ok(PlatformInfo {
        os,
        is_mobile,
        presets,
    })
}

/// ストレージアクセス権限の要求および確認を行う
///
/// 変更理由: Android環境での実行時パーミッション要求をフロントエンドから明示的にキックできるようにするため。
///
/// @return 権限が付与されているかどうかの真偽値
#[command]
pub async fn request_storage_permissions(#[allow(unused_variables)] app: AppHandle) -> Result<bool, AppError> {
    #[cfg(target_os = "android")]
    {
        let check_path = Path::new("/storage/emulated/0/Pictures");
        let has_perm = check_path.read_dir().is_ok() || Path::new("/storage/emulated/0/DCIM").read_dir().is_ok();
        Ok(has_perm)
    }
    #[cfg(not(target_os = "android"))]
    {
        Ok(true)
    }
}

/// ファイルをOS既定の関連付けアプリケーションで開く
///
/// 変更理由: アクションプラグイン「既定のアプリで開く」のバックエンド処理。
/// モバイル環境（Android）およびデスクトップ環境でOS標準のファイルオープンAPIを抽象化して実行する。
///
/// @param app Tauri AppHandle
/// @param state アプリケーション状態
/// @param id 画像ID（省略可能）
/// @param path ファイルパス（省略時はIDから検索）
#[command]
pub async fn open_file_with_default_app(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    id: Option<i64>,
    path: Option<String>,
) -> Result<(), AppError> {
    let target_path = if let Some(p) = path {
        p
    } else if let Some(img_id) = id {
        let conn = state.db.reader()?;
        crate::db::repo::get_image_file_info(&conn, img_id)?
            .map(|(p, _, _, _)| p)
            .ok_or_else(|| AppError::not_found("画像が見つかりません"))?
    } else {
        return Err(AppError::invalid_argument("パスまたは画像IDを指定してください"));
    };

    #[cfg(target_os = "android")]
    {
        crate::android_native::open_file(&app, &target_path, None, None)?;
        Ok(())
    }
    #[cfg(not(target_os = "android"))]
    {
        use tauri_plugin_opener::OpenerExt;
        app.opener()
            .open_path(target_path, None::<&str>)
            .map_err(|e| AppError::internal(format!("既定アプリでの起動に失敗: {}", e)))?;
        Ok(())
    }
}

/// ファイルをOSのファイルマネージャー（エクスプローラー等）で表示しハイライトする
///
/// 変更理由: アクションプラグイン「フォルダで表示」のバックエンド処理。
/// デスクトップ環境では対象ファイルを選択状態でエクスプローラーを開き、
/// Android 環境では DocumentsContract またはファイルマネージャ用 Intent で該当フォルダを表示する。
///
/// @param app Tauri AppHandle
/// @param state アプリケーション状態
/// @param id 画像ID（省略可能）
/// @param path ファイルパス（省略時はIDから検索）
#[command]
pub async fn show_in_folder(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    id: Option<i64>,
    path: Option<String>,
) -> Result<(), AppError> {
    let target_path = if let Some(p) = path {
        p
    } else if let Some(img_id) = id {
        let conn = state.db.reader()?;
        crate::db::repo::get_image_file_info(&conn, img_id)?
            .map(|(p, _, _, _)| p)
            .ok_or_else(|| AppError::not_found("画像が見つかりません"))?
    } else {
        return Err(AppError::invalid_argument("パスまたは画像IDを指定してください"));
    };

    #[cfg(target_os = "android")]
    {
        crate::android_native::open_folder(&app, &target_path)?;
        Ok(())
    }
    #[cfg(target_os = "ios")]
    {
        use tauri_plugin_opener::OpenerExt;
        let res = if let Some(parent) = std::path::Path::new(&target_path).parent() {
            app.opener().open_path(parent.to_string_lossy().to_string(), None::<&str>)
        } else {
            app.opener().open_path(&target_path, None::<&str>)
        };
        let _ = res;
        Ok(())
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        use tauri_plugin_opener::OpenerExt;
        app.opener()
            .reveal_item_in_dir(&target_path)
            .map_err(|e| AppError::internal(format!("エクスプローラーでの表示に失敗: {}", e)))?;
        Ok(())
    }
}

/// プラグインが提供するコンテキストメニューアクション一覧を取得する
///
/// 変更理由: plugin.json で定義された右クリックメニューアクションを
/// フロントエンドUIへ動的に提供するため。
///
/// @param state アプリケーション状態
/// @return PluginActionDefinition の配列
#[command]
pub async fn get_plugin_actions(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<crate::plugins::PluginActionDefinition>, AppError> {
    Ok(state.format_plugins.get_actions())
}

/// プラグインマニフェストで定義されたアクションを実行する
///
/// 変更理由: 特定アプリ（CLIP STUDIO等）への依存を本体コアから完全に切り離し、
/// マニフェスト（plugin.json）に記載されたコマンド・引数定義に基づいて
/// 汎用的に外部ツールを起動できるようにするため。
///
/// @param app Tauri AppHandle
/// @param state アプリケーション状態
/// @param action_id アクション識別子 (例: "action.open_clip_studio")
/// @param id 画像ID（省略可能）
/// @param path ファイルパス（省略可能）
#[command]
pub async fn execute_plugin_action(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    action_id: String,
    id: Option<i64>,
    path: Option<String>,
) -> Result<(), AppError> {
    let target_path = if let Some(p) = path {
        p
    } else if let Some(img_id) = id {
        let conn = state.db.reader()?;
        crate::db::repo::get_image_file_info(&conn, img_id)?
            .map(|(p, _, _, _)| p)
            .ok_or_else(|| AppError::not_found("画像が見つかりません"))?
    } else {
        return Err(AppError::invalid_argument("パスまたは画像IDを指定してください"));
    };

    let action_def = state
        .format_plugins
        .find_action(&action_id)
        .ok_or_else(|| AppError::not_found(format!("プラグインアクションが見つかりません: {}", action_id)))?;

    // 1. コマンド定義が存在すれば外部プロセス起動を最優先で試行
    #[allow(unused_variables)]
    if let Some(cmd_def) = action_def.command {
        #[cfg(target_os = "windows")]
        {
            let mut candidates = cmd_def.windows.clone();
            if let Some(def_cmd) = &cmd_def.default {
                if !candidates.contains(def_cmd) {
                    candidates.push(def_cmd.clone());
                }
            }

            for exe_str in &candidates {
                let candidate_path = std::path::Path::new(exe_str);
                // 実行ファイルの存在確認、またはPATH上のコマンド名（パス区切り文字を含まない）の場合に実行
                if candidate_path.is_file() || !exe_str.contains(std::path::MAIN_SEPARATOR) && !exe_str.contains('/') {
                    let mut cmd = std::process::Command::new(exe_str);
                    let mut path_added = false;
                    for arg in &cmd_def.args {
                        if arg.contains("{path}") {
                            cmd.arg(arg.replace("{path}", &target_path));
                            path_added = true;
                        } else {
                            cmd.arg(arg);
                        }
                    }
                    if !path_added {
                        cmd.arg(&target_path);
                    }
                    if cmd.spawn().is_ok() {
                        return Ok(());
                    }
                }
            }
        }

        #[cfg(not(any(target_os = "windows", target_os = "android", target_os = "ios")))]
        {
            if let Some(default_cmd) = &cmd_def.default {
                let mut cmd = std::process::Command::new(default_cmd);
                let mut path_added = false;
                for arg in &cmd_def.args {
                    if arg.contains("{path}") {
                        cmd.arg(arg.replace("{path}", &target_path));
                        path_added = true;
                    } else {
                        cmd.arg(arg);
                    }
                }
                if !path_added {
                    cmd.arg(&target_path);
                }
                if cmd.spawn().is_ok() {
                    return Ok(());
                }
            }
        }
    }

    // 2. Android環境での起動:
    // CLIP STUDIO PAINT または .clip 拡張子の場合は専用パッケージとMIMEタイプを指定して起動
    #[cfg(target_os = "android")]
    {
        let is_clip = action_id == "action.open_clip_studio"
            || target_path.to_lowercase().ends_with(".clip");
        let mime = if is_clip { Some("application/x-clip") } else { None };
        let package = if is_clip {
            Some("jp.co.celsys.clipstudiopaint.googleplay")
        } else {
            None
        };

        crate::android_native::open_file(&app, &target_path, mime, package)?;
        Ok(())
    }

    // 3. デスクトップ環境: コマンド未指定、またはプロセス起動失敗時はOS標準のファイル関連付け（既定アプリ/共有）でオープン
    #[cfg(not(target_os = "android"))]
    {
        use tauri_plugin_opener::OpenerExt;
        if app.opener().open_path(&target_path, None::<&str>).is_ok() {
            return Ok(());
        }

        Err(AppError::internal(format!("アクション [{}] の実行に失敗しました（関連付けまたは実行ファイルが見つかりません）", action_def.name)))
    }
}

/// コマンドライン引数文字列を空白・引用符を考慮してトークン分割するヘルパー
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn parse_command_args(input: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut quote_char = ' ';

    for ch in input.chars() {
        match ch {
            '"' | '\'' if !in_quotes => {
                in_quotes = true;
                quote_char = ch;
            }
            c if in_quotes && c == quote_char => {
                in_quotes = false;
            }
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    args.push(current.clone());
                    current.clear();
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

/// ユーザー登録された外部アプリケーションでファイルを開く
///
/// 変更理由: カスタム右クリックメニュー項目から指定のペイントソフト等を起動するため。
/// 引数テンプレート {path} の正確な置換、二重クォートのエスケープ汚染防止、複数引数のトークン分解に対応する。
///
/// @param app Tauri AppHandle
/// @param state アプリケーション状態
/// @param app_path 実行ファイルのパスまたはコマンド
/// @param args コマンド引数リスト（オプション）
/// @param id 画像ID（省略可能）
/// @param file_path 対象ファイルパス（省略時はIDから検索）
#[command]
pub async fn open_file_with_app(
    #[allow(unused_variables)] app: AppHandle,
    state: State<'_, Arc<AppState>>,
    #[allow(unused_variables)] app_path: String,
    #[allow(unused_variables)] args: Option<Vec<String>>,
    id: Option<i64>,
    file_path: Option<String>,
) -> Result<(), AppError> {
    let target_path = if let Some(p) = file_path {
        p
    } else if let Some(img_id) = id {
        let conn = state.db.reader()?;
        crate::db::repo::get_image_file_info(&conn, img_id)?
            .map(|(p, _, _, _)| p)
            .ok_or_else(|| AppError::not_found("画像が見つかりません"))?
    } else {
        return Err(AppError::invalid_argument("ファイルパスまたは画像IDを指定してください"));
    };

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let mut cmd = std::process::Command::new(&app_path);
        let mut path_inserted = false;

        if let Some(extra_args) = args {
            for raw_arg in extra_args {
                let tokens = parse_command_args(&raw_arg);
                for token in tokens {
                    let mut cleaned = token;
                    if (cleaned.starts_with('"') && cleaned.ends_with('"') && cleaned.len() >= 2)
                        || (cleaned.starts_with('\'') && cleaned.ends_with('\'') && cleaned.len() >= 2)
                    {
                        cleaned = cleaned[1..cleaned.len() - 1].to_string();
                    }

                    if cleaned.contains("{path}") {
                        path_inserted = true;
                        cmd.arg(cleaned.replace("{path}", &target_path));
                    } else {
                        cmd.arg(cleaned);
                    }
                }
            }
        }

        if !path_inserted {
            cmd.arg(&target_path);
        }

        cmd.spawn()
            .map_err(|e| AppError::internal(format!("アプリケーションの起動に失敗: {}", e)))?;
        Ok(())
    }
    #[cfg(target_os = "android")]
    {
        crate::android_native::open_file(&app, &target_path, None, None)?;
        Ok(())
    }
    #[cfg(target_os = "ios")]
    {
        use tauri_plugin_opener::OpenerExt;
        app.opener()
            .open_path(target_path, None::<&str>)
            .map_err(|e| AppError::internal(format!("共有アプリでのオープンに失敗: {}", e)))?;
        Ok(())
    }
}

/// ファイルをゴミ箱（リサイクルビン）へ移動し、カタログDBおよびキャッシュから削除する
///
/// 変更理由: 仕様書および右クリックメニュー「ゴミ箱へ移動」アクションの実装。
/// ファイルをOSのゴミ箱へ安全に退避させ、UI表示を即時更新する。
/// Android環境ではMediaStoreまたはストレージAPI経由で安全に削除する。
///
/// @param app Tauri AppHandle
/// @param state アプリケーション状態
/// @param image_id 対象画像ID
#[command]
pub async fn trash_file(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    image_id: i64,
) -> Result<(), AppError> {
    // 1. 画像情報の取得
    let (file_path_str, quick_hash) = {
        let conn = state.db.reader()?;
        let (p, h, _, _) = crate::db::repo::get_image_file_info(&conn, image_id)?
            .ok_or_else(|| AppError::not_found("画像が見つかりません"))?;
        (p, h)
    };

    let target_path = std::path::PathBuf::from(&file_path_str);

    // 2. ゴミ箱へ移動（ファイルが存在する場合）
    if target_path.exists() {
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
            trash::delete(&target_path)
                .map_err(|e| AppError::internal(format!("ゴミ箱への移動に失敗: {}", e)))?;
        }
        #[cfg(target_os = "android")]
        {
            crate::android_native::trash_file(&app, &file_path_str)?;
        }
        #[cfg(target_os = "ios")]
        {
            std::fs::remove_file(&target_path)
                .map_err(|e| AppError::internal(format!("ファイルの削除に失敗: {}", e)))?;
        }
    }

    // 3. カタログDBから削除
    {
        let conn = state.db.writer();
        crate::db::repo::delete_image(&conn, image_id)?;
    }

    // 4. メモリキャッシュおよびストアからサムネイルをパージ
    state.remove_thumb_from_memory(&quick_hash);
    let _ = state.thumb_store.delete(&quick_hash);

    // 5. カタログバージョン更新およびUI変更通知
    let new_ver = state.bump_catalog_version();
    let _ = app.emit("catalog-changed", serde_json::json!({ "version": new_ver }));

    info!("File trashed and removed from catalog: id={}, path={}", image_id, file_path_str);
    Ok(())
}

/// アプリケーション全般の設定（常駐設定、言語、有効化プラグイン一覧、カスタムメニュー項目）を取得する
///
/// @param state アプリケーション状態
/// @return AppSettings
#[command]
pub async fn get_app_settings(
    state: State<'_, Arc<AppState>>,
) -> Result<crate::settings::AppSettings, AppError> {
    Ok(crate::settings::load_settings(&state.data_dir))
}

/// アプリケーション設定を保存する
///
/// @param app Tauri AppHandle
/// @param state アプリケーション状態
/// @param settings 保存する設定データ
#[command]
pub async fn save_app_settings(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    settings: crate::settings::AppSettings,
) -> Result<(), AppError> {
    crate::settings::save_settings(&state.data_dir, &settings)
        .map_err(|e| AppError::io(format!("設定の保存に失敗: {}", e)))?;

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    crate::tray::update_tray_locale(&app, &settings.locale);
    #[cfg(any(target_os = "android", target_os = "ios"))]
    let _ = &app;

    // プラグインの動的リロードおよび新規拡張子検出時の自動再走査
    let reload_res = state.format_plugins.reload(&settings.enabled_plugins, &state.data_dir);
    if !reload_res.added_extensions.is_empty() {
        tracing::info!(
            "New plugin extensions registered: {:?}. Triggering automatic background rescan.",
            reload_res.added_extensions
        );
        let app_clone = app.clone();
        let state_clone = Arc::clone(&state);
        tauri::async_runtime::spawn_blocking(move || {
            if let Ok(conn) = state_clone.db.reader() {
                if let Ok(folders) = crate::db::repo::get_watched_folders(&conn) {
                    for folder in folders {
                        let _ = crate::scanner::scan_folder(
                            app_clone.clone(),
                            Arc::clone(&state_clone),
                            folder.id,
                            folder.path,
                        );
                    }
                }
            }
        });
    }

    info!("AppSettings saved successfully");
    Ok(())
}

/// 導入済みプラグイン一覧を取得する
///
/// 変更理由: 設定モーダルの「プラグイン管理」タブで全プラグインの状態・対応拡張子を表示するため。
///
/// @param state アプリケーション状態
/// @return InstalledPluginInfo の配列
#[command]
pub async fn get_installed_plugins(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<crate::plugins::InstalledPluginInfo>, AppError> {
    let settings = crate::settings::load_settings(&state.data_dir);
    Ok(state
        .format_plugins
        .list_installed_plugins(&settings.enabled_plugins, &state.data_dir))
}

/// ユーザーのプラグインフォルダ（%APPDATA%/com.imagemediaviewer.desktop/plugins）をOSのファイルマネージャで開く
///
/// 変更理由: ユーザーが手動でプラグインフォルダを配置・確認したい場合にワンクリックでアクセス可能にするため。
///
/// @param app Tauri AppHandle
/// @param state アプリケーション状態
#[command]
pub async fn open_plugins_folder(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<String, AppError> {
    let plugins_dir = state.data_dir.join("plugins");
    std::fs::create_dir_all(&plugins_dir)
        .map_err(|e| AppError::io(format!("プラグインフォルダの作成に失敗: {}", e)))?;

    let dir_str = plugins_dir.to_string_lossy().to_string();

    #[cfg(target_os = "android")]
    {
        crate::android_native::open_folder(&app, &dir_str)?;
    }
    #[cfg(not(target_os = "android"))]
    {
        use tauri_plugin_opener::OpenerExt;
        let _ = app.opener().open_path(&dir_str, None::<&str>);
    }

    Ok(dir_str)
}

/// ディレクトリ配下から plugin.json が存在するプラグインルートディレクトリを再帰探索する（深さ上限3）
fn find_manifest_root_dir(dir: &Path, depth: usize) -> Option<PathBuf> {
    if depth > 3 || !dir.is_dir() {
        return None;
    }
    if dir.join("plugin.json").is_file() {
        return Some(dir.to_path_buf());
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                if let Some(found) = find_manifest_root_dir(&p, depth + 1) {
                    return Some(found);
                }
            }
        }
    }
    None
}

/// ディレクトリの中身を再帰的にコピーするヘルパー
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let dst_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&entry.path(), &dst_path)?;
        } else {
            std::fs::copy(entry.path(), dst_path)?;
        }
    }
    Ok(())
}

/// プラグインIDから安全なディレクトリ名を生成する
fn sanitize_plugin_folder_name(id: &str) -> String {
    let stripped = id.strip_prefix("format.").unwrap_or(id);
    let sanitized: String = stripped
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();
    if sanitized.is_empty() {
        "custom_plugin".to_string()
    } else {
        sanitized
    }
}

/// 指定されたパス（ZIPアーカイブ、プラグインフォルダ、または plugin.json）からプラグインをインストールする
///
/// 変更理由: 外部で個別開発・配布されたプラグイン（ClipStudioプラグイン等）を、
/// ユーザーが設定ファイル（app_settings.json）を手動編集したりパスを探すことなく、
/// GUIからファイル/フォルダ選択するだけで即座に導入・有効化できるようにするため。
/// 最小限の依存関係原則に従い、ZIP展開にはOS標準の `tar -xf`（WindowsフォールバックとしてPowerShell `Expand-Archive`）を使用する。
///
/// @param app_data_dir アプリケーションデータディレクトリ
/// @param source_path 選択されたZIPファイル、プラグインディレクトリ、または plugin.json のパス
/// @return インストールされた PluginManifest
pub fn install_plugin_from_path(
    app_data_dir: &Path,
    source_path: &Path,
) -> Result<crate::plugins::PluginManifest, AppError> {
    if !source_path.exists() {
        return Err(AppError::invalid_argument("指定されたパスが存在しません"));
    }

    // 一時作業ディレクトリの準備（ZIP展開用）
    let temp_extract_dir = std::env::temp_dir().join(format!(
        "imv_plugin_import_{}_{}",
        std::process::id(),
        chrono::Utc::now().timestamp_millis()
    ));

    let resolve_res = (|| -> Result<(PathBuf, crate::plugins::PluginManifest), AppError> {
        let plugin_src_dir = if source_path.is_dir() {
            find_manifest_root_dir(source_path, 0).ok_or_else(|| {
                AppError::invalid_argument("指定されたフォルダ内に plugin.json が見つかりません")
            })?
        } else if source_path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.eq_ignore_ascii_case("plugin.json"))
            .unwrap_or(false)
        {
            source_path
                .parent()
                .ok_or_else(|| AppError::invalid_argument("親ディレクトリを特定できません"))?
                .to_path_buf()
        } else if source_path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("zip"))
            .unwrap_or(false)
        {
            std::fs::create_dir_all(&temp_extract_dir).map_err(|e| {
                AppError::io(format!("一時展開ディレクトリの作成に失敗: {}", e))
            })?;

            // 1. OS標準の tar コマンドによる展開を試行 (Windows 10/11, macOS, Linux 標準搭載)
            let tar_ok = std::process::Command::new("tar")
                .arg("-xf")
                .arg(source_path)
                .arg("-C")
                .arg(&temp_extract_dir)
                .status()
                .map(|s| s.success())
                .unwrap_or(false);

            if !tar_ok {
                #[cfg(target_os = "windows")]
                {
                    let ps_ok = std::process::Command::new("powershell")
                        .args([
                            "-NoProfile",
                            "-NonInteractive",
                            "-Command",
                            &format!(
                                "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
                                source_path.to_string_lossy().replace('\'', "''"),
                                temp_extract_dir.to_string_lossy().replace('\'', "''")
                            ),
                        ])
                        .status()
                        .map(|s| s.success())
                        .unwrap_or(false);
                    if !ps_ok {
                        return Err(AppError::internal(
                            "ZIPアーカイブの展開に失敗しました（破損しているか未対応の形式です）",
                        ));
                    }
                }
                #[cfg(not(target_os = "windows"))]
                {
                    let unzip_ok = std::process::Command::new("unzip")
                        .arg("-o")
                        .arg(source_path)
                        .arg("-d")
                        .arg(&temp_extract_dir)
                        .status()
                        .map(|s| s.success())
                        .unwrap_or(false);
                    if !unzip_ok {
                        return Err(AppError::internal("ZIPアーカイブの展開に失敗しました"));
                    }
                }
            }

            find_manifest_root_dir(&temp_extract_dir, 0).ok_or_else(|| {
                AppError::invalid_argument("ZIPアーカイブ内に有効な plugin.json が含まれていません")
            })?
        } else {
            return Err(AppError::invalid_argument(
                "対応していないファイル形式です（.zip ファイル、plugin.json、またはプラグインフォルダを指定してください）",
            ));
        };

        let manifest_path = plugin_src_dir.join("plugin.json");
        let content = std::fs::read_to_string(&manifest_path)
            .map_err(|e| AppError::io(format!("plugin.json の読み込みに失敗: {}", e)))?;
        let manifest: crate::plugins::PluginManifest = serde_json::from_str(&content)
            .map_err(|e| AppError::invalid_argument(format!("plugin.json の構文が不正です: {}", e)))?;

        if manifest.id.trim().is_empty() || manifest.name.trim().is_empty() {
            return Err(AppError::invalid_argument(
                "plugin.json の id または name が空です",
            ));
        }

        Ok((plugin_src_dir, manifest))
    })();

    let result = match resolve_res {
        Ok((plugin_src_dir, manifest)) => {
            let folder_name = sanitize_plugin_folder_name(&manifest.id);
            let target_dir = app_data_dir.join("plugins").join(&folder_name);

            let src_canon = plugin_src_dir.canonicalize().unwrap_or_else(|_| plugin_src_dir.clone());
            let dst_canon = target_dir.canonicalize().unwrap_or_else(|_| target_dir.clone());

            if src_canon != dst_canon {
                copy_dir_recursive(&plugin_src_dir, &target_dir).map_err(|e| {
                    AppError::io(format!("プラグインファイルのコピーに失敗: {}", e))
                })?;
            }

            Ok(manifest)
        }
        Err(e) => Err(e),
    };

    if temp_extract_dir.exists() {
        let _ = std::fs::remove_dir_all(&temp_extract_dir);
    }

    result
}

/// GUIからプラグイン（ZIPパッケージ・フォルダ・plugin.json）をインストールし即時有効化するコマンド
///
/// @param app Tauri AppHandle
/// @param state アプリケーション状態
/// @param source_path インポート元のパス
/// @return 更新後の導入済みプラグイン一覧
#[command]
pub async fn install_plugin(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    source_path: String,
) -> Result<Vec<crate::plugins::InstalledPluginInfo>, AppError> {
    let manifest = install_plugin_from_path(&state.data_dir, Path::new(&source_path))?;

    // 設定を更新し、対象プラグインが明示的に無効化（!id）されていた場合は解除して有効化リストに追加
    let mut settings = crate::settings::load_settings(&state.data_dir);
    let disable_entry = format!("!{}", manifest.id);
    settings.enabled_plugins.retain(|p| p != &disable_entry);
    if !crate::plugins::format::is_plugin_enabled_by_patterns(&manifest.id, &settings.enabled_plugins) {
        settings.enabled_plugins.push(manifest.id.clone());
    }
    for act in &manifest.actions {
        let disable_act = format!("!{}", act.id);
        settings.enabled_plugins.retain(|p| p != &disable_act);
    }
    let _ = crate::settings::save_settings(&state.data_dir, &settings);

    // プラグインレジストリを即時リロードし、新規拡張子があれば監視フォルダを再走査
    let reload_res = state
        .format_plugins
        .reload(&settings.enabled_plugins, &state.data_dir);

    if !reload_res.added_extensions.is_empty() {
        let app_clone = app.clone();
        let state_clone = Arc::clone(&state);
        tauri::async_runtime::spawn_blocking(move || {
            if let Ok(conn) = state_clone.db.reader() {
                if let Ok(folders) = crate::db::repo::get_watched_folders(&conn) {
                    for folder in folders {
                        let _ = crate::scanner::scan_folder(
                            app_clone.clone(),
                            Arc::clone(&state_clone),
                            folder.id,
                            folder.path,
                        );
                    }
                }
            }
        });
    }

    info!("Plugin installed and enabled: id={}, name={}", manifest.id, manifest.name);

    Ok(state
        .format_plugins
        .list_installed_plugins(&settings.enabled_plugins, &state.data_dir))
}





