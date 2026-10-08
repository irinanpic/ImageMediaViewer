use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{command, AppHandle, State};
use tracing::{info, warn};

use crate::error::AppError;
use crate::models::{
    Board, BoardItem, BoardNote, BookmarkRecord, CreateBoardNotePayload, CreateBoardPayload,
    CreateBookmarkPayload, GetImagesPayload, ImageDetail, ImageRecord, ThumbProgress,
    TimelineSummary, UpdateBoardItemPayload, UpdateBoardNotePayload, UpdateBoardPayload,
    WatchedFolder, WindowState,
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
    let conn = state.db.reader()?;
    let summary = crate::db::repo::get_timeline_summary(&conn, folder_id, current_version, sort)?;
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
    use tauri_plugin_opener::OpenerExt;

    let path_str = {
        let conn = state.db.reader()?;
        crate::db::repo::get_image_file_info(&conn, id)?
            .map(|(p, _, _, _)| p)
            .ok_or_else(|| AppError::not_found("画像が見つかりません"))?
    };

    app.opener()
        .reveal_item_in_dir(path_str)
        .map_err(|e| AppError::internal(format!("エクスプローラ表示に失敗: {}", e)))?;

    Ok(())
}

/// 表示範囲（ビューポート）の画像ID群を登録し、優先度付きパイプラインの世代を進める
///
/// 変更理由: Picasa並みの表示速度のため、画面内（High）およびその周辺（Mid）を最優先で生成し、
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
    use tauri_plugin_opener::OpenerExt;
    if let Some(manager) = crate::logger::get_log_manager() {
        let dir = manager.log_dir();
        let dir_str = dir.to_string_lossy().to_string();
        let _ = app.opener().reveal_item_in_dir(&dir_str);
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
    info!("Reset thumbnail failure flags (Targets: {})", reset_count);
    Ok(serde_json::json!({
        "success": true,
        "resetCount": reset_count
    }))
}

/// サムネイル生成進捗状況を取得
#[command]
pub async fn get_thumb_progress(state: State<'_, Arc<AppState>>) -> Result<ThumbProgress, AppError> {
    let conn = state.db.reader()?;
    let (done, failed, total) = crate::db::repo::get_thumb_progress(&conn)?;
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

    info!("Language updated: locale = {}", locale);
    Ok(locale)
}




