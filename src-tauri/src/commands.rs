use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tauri::{command, AppHandle, Emitter, State};
use tracing::{info, warn};

use crate::error::AppError;
use crate::models::{
    Board, BoardItem, BoardNote, BookmarkRecord, CreateBoardNotePayload, CreateBoardPayload,
    CreateBookmarkPayload, GetImagesPayload, ImageDetail, ImageRecord, ImageTagAssignment,
    PlatformInfo, PresetFolder, SubfolderEntry, TagRecord, TagWithCount, ThumbProgress,
    TimelineFilter, TimelineSummary, UpdateBoardItemPayload, UpdateBoardNotePayload,
    UpdateBoardPayload, WatchedFolder, WindowState,
};
use crate::pipeline::JobPriority;
use crate::scanner::walker::{is_sub_directory, normalize_path};
use crate::state::AppState;

static CLIPBOARD_SEQ: AtomicU64 = AtomicU64::new(0);
static WEB_CLIP_SEQ: AtomicU64 = AtomicU64::new(0);

/// ドロップされたローカルファイル候補の情報
#[derive(Debug, serde::Deserialize)]
pub struct DroppedFileCandidate {
    /// ファイル名（例: "sample.png", "drawing.clip"）
    pub name: String,
    /// ファイルサイズ（バイト）
    pub size: u64,
    /// 最終更新日時（ミリ秒または秒単位のタイムスタンプ、オプション）
    pub last_modified: Option<i64>,
}

/// ドロップファイル解決結果
#[derive(Debug, serde::Serialize)]
pub struct ResolveDroppedFilesResult {
    /// 配置完了したボードアイテム一覧
    pub items: Vec<BoardItem>,
    /// 実パスや既存IDを特定できなかった candidate のインデックス一覧
    pub unresolved_indices: Vec<usize>,
}

/// Base64 Data URL またはプレーンBase64文字列、あるいはバイナリVecから画像バイト列と検出された拡張子を取得する
///
/// 変更理由: フロントエンドからの巨大バイナリ配列（number[]）転送によるIPC負荷・メモリ消費を軽減するため、Base64文字列転送をサポートする。
fn extract_image_bytes(
    image_data: Option<&str>,
    image_bytes: Option<Vec<u8>>,
) -> Result<(Vec<u8>, Option<String>), AppError> {
    if let Some(raw) = image_bytes {
        if !raw.is_empty() {
            return Ok((raw, None));
        }
    }
    if let Some(data_str) = image_data {
        let trimmed = data_str.trim();
        if trimmed.is_empty() {
            return Err(AppError::invalid_argument("画像データが空です"));
        }
        use base64::Engine;
        if trimmed.starts_with("data:image/") {
            let comma_idx = trimmed.find(',').ok_or_else(|| AppError::invalid_argument("無効なData URL形式です"))?;
            let header = &trimmed[..comma_idx];
            let b64_data = &trimmed[comma_idx + 1..];
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(b64_data)
                .map_err(|e| AppError::invalid_argument(format!("Base64デコードに失敗: {}", e)))?;
            let ext = if header.contains("image/png") {
                Some("png".to_string())
            } else if header.contains("image/jpeg") {
                Some("jpg".to_string())
            } else if header.contains("image/webp") {
                Some("webp".to_string())
            } else if header.contains("image/gif") {
                Some("gif".to_string())
            } else if header.contains("image/bmp") {
                Some("bmp".to_string())
            } else {
                None
            };
            return Ok((decoded, ext));
        } else {
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(trimmed)
                .map_err(|e| AppError::invalid_argument(format!("Base64デコードに失敗: {}", e)))?;
            return Ok((decoded, None));
        }
    }
    Err(AppError::invalid_argument("画像データが指定されていません"))
}


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
/// 変更理由: 仕様書§8.1全レコードを取得しなくてもスクロール全長を確定させるため。
/// 検索・拡張子・縦横比・サブフォルダ・タグフィルタが指定された場合もフィルタ適用後の件数とバケットをキャッシュ付きで返却する。
#[command]
pub async fn get_timeline_summary(
    state: State<'_, Arc<AppState>>,
    folder_id: Option<i64>,
    sort: Option<crate::models::TimelineSort>,
    filter: Option<TimelineFilter>,
) -> Result<TimelineSummary, AppError> {
    let current_version = state.current_catalog_version();
    let sort_val = sort.unwrap_or(crate::models::TimelineSort::TakenAtDesc);
    let filter_key = filter.as_ref().map(|f| f.cache_key()).unwrap_or_default();

    // インメモリキャッシュをまず確認（SQLiteクエリのバイパス）
    if let Some(cached) = state.get_cached_timeline_summary(folder_id, sort_val, &filter_key, current_version) {
        return Ok(cached);
    }

    let conn = state.db.reader()?;
    let summary = crate::db::repo::get_timeline_summary_filtered(
        &conn,
        folder_id,
        current_version,
        Some(sort_val),
        filter.as_ref(),
    )?;
    state.put_cached_timeline_summary(folder_id, sort_val, filter_key, current_version, summary.clone());
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
    let images = crate::db::repo::get_timeline_images_filtered(
        &conn,
        payload.offset,
        limit,
        payload.folder_id,
        payload.sort,
        payload.filter.as_ref(),
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

/// どのボードからも参照されなくなったクリップボード保存画像およびWebクリップ画像をDB・サムネイル・ディスクからクリーンアップする
///
/// 変更理由: クリップボード貼り付け画像やWebドロップ画像がボードから削除された際、孤立した一時ファイルやDBレコードを自動回収するため。
///
/// @param state アプリケーション共有状態
pub fn cleanup_orphan_clipboard_files(state: &AppState) {
    for folder_name in &["clipboard_images", "web_clips"] {
        let raw_dir = state.data_dir.join(folder_name);
        let norm_dir = PathBuf::from(normalize_path(&raw_dir.to_string_lossy()));
        let cleaned = {
            let writer = state.db.writer();
            crate::db::repo::cleanup_orphan_clipboard_images(&writer, &norm_dir).unwrap_or_default()
        };
        for (_id, file_path, quick_hash) in cleaned {
            let _ = std::fs::remove_file(&file_path);
            state.remove_thumb_from_memory(&quick_hash);
            let _ = state.thumb_store.delete(&quick_hash);
        }
    }
}

/// ドロップされた外部ファイル（またはフォルダ直下の画像）をインデックスし、ムードボードの指定座標へ配置する共通処理
///
/// 変更理由: 監視フォルダに含まれない画像ファイルでもタイムラインに混入させず（folder_id = 0）、
/// ムードボード上に直接ドロップして配置・表示できるようにするため。
///
/// @param state アプリケーション共有状態
/// @param board_id 追加先ボードID
/// @param paths ドロップされたファイルまたはフォルダのパス一覧
/// @param x 配置先のキャンバスX座標（オプション）
/// @param y 配置先のキャンバスY座標（オプション）
/// @return 更新後のボード内全アイテム一覧
pub fn register_dropped_paths_to_board_core(
    state: &AppState,
    board_id: i64,
    paths: Vec<String>,
    x: Option<f64>,
    y: Option<f64>,
) -> Result<Vec<BoardItem>, AppError> {
    let mut target_files: Vec<PathBuf> = Vec::new();

    for raw in paths {
        let p = PathBuf::from(raw.trim());
        if !p.exists() {
            continue;
        }
        if p.is_file() {
            if crate::scanner::walker::is_supported_image(&p, &state.format_plugins) {
                target_files.push(p);
            }
        } else if p.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&p) {
                let mut dir_files: Vec<PathBuf> = entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|ep| ep.is_file() && crate::scanner::walker::is_supported_image(ep, &state.format_plugins))
                    .collect();
                dir_files.sort();
                target_files.extend(dir_files);
            }
        }
    }

    if target_files.is_empty() {
        return Err(AppError::invalid_argument("対応する画像ファイルが見つかりませんでした"));
    }

    let now = chrono::Utc::now().timestamp();
    let mut prepared_records: Vec<crate::db::repo::NewImageRecord> = Vec::with_capacity(target_files.len());

    for file_path in &target_files {
        let norm_path = normalize_path(&file_path.to_string_lossy());
        let meta = std::fs::metadata(file_path)
            .map_err(|e| AppError::io(format!("ファイル情報の取得に失敗: {}", e)))?;
        let size = meta.len();
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(now);

        let mut file = std::fs::File::open(file_path)
            .map_err(|e| AppError::io(format!("ファイルのオープンに失敗: {}", e)))?;
        let quick_hash = crate::pipeline::hash::calculate_quick_hash_from_file(&mut file, size)
            .map_err(|e| AppError::io(format!("ハッシュ計算に失敗: {}", e)))?;
        let img_meta = crate::pipeline::exif::extract_metadata_from_file_with_plugins(
            file_path,
            &mut file,
            mtime,
            Some(&state.format_plugins),
        );

        prepared_records.push(crate::db::repo::NewImageRecord {
            folder_id: 0, // 後続のDB書き込み時に監視フォルダ該当判定を行う
            file_path: norm_path,
            file_size: size,
            file_mtime: mtime,
            taken_at: img_meta.taken_at,
            taken_at_source: img_meta.taken_at_source,
            width: img_meta.width,
            height: img_meta.height,
            orientation: img_meta.orientation,
            format: img_meta.format,
            quick_hash,
            indexed_at: now,
            source_url: None,
        });
    }

    let mut image_ids = Vec::with_capacity(prepared_records.len());
    let mut thumbs_to_enqueue = Vec::new();

    let items = {
        let writer = state.db.writer();
        for mut rec in prepared_records {
            rec.folder_id = crate::db::repo::find_matching_watched_folder_id(&writer, &rec.file_path)?;
            let img_id = crate::db::repo::upsert_single_image_and_get_id(&writer, &rec)?;
            image_ids.push(img_id);
            if let Ok(Some(src)) = crate::db::repo::get_thumb_source(&writer, img_id) {
                if src.thumb_status != 1 {
                    thumbs_to_enqueue.push(src);
                }
            }
        }
        crate::db::repo::add_board_items_at(&writer, board_id, &image_ids, x, y, now)?
    };

    for src in thumbs_to_enqueue {
        state.thumb_pipeline.enqueue_source(&src, JobPriority::High);
    }

    Ok(items)
}

/// クリップボード上の画像バイナリをアプリ管理領域へ保存し、ムードボードの指定座標へ配置する共通処理
///
/// 変更理由: どこにもファイルとして保存されていないクリップボード上の画像を、
/// ムードボード上に直接貼り付けて永続的に表示・管理できるようにするため。
///
/// @param state アプリケーション共有状態
/// @param board_id 追加先ボードID
/// @param image_bytes 画像バイナリデータ
/// @param extension 拡張子（"png", "jpg", "webp" 等、省略時は "png"）
/// @param x 配置先のキャンバスX座標（オプション）
/// @param y 配置先のキャンバスY座標（オプション）
/// @return 更新後のボード内全アイテム一覧
pub fn register_clipboard_image_to_board_core(
    state: &AppState,
    board_id: i64,
    image_bytes: &[u8],
    extension: Option<&str>,
    x: Option<f64>,
    y: Option<f64>,
) -> Result<Vec<BoardItem>, AppError> {
    if image_bytes.is_empty() {
        return Err(AppError::invalid_argument("クリップボードの画像データが空です"));
    }

    let ext = extension
        .map(|s| s.trim().trim_start_matches('.').to_ascii_lowercase())
        .filter(|s| matches!(s.as_str(), "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp"))
        .unwrap_or_else(|| "png".to_string());

    let hash_hex = blake3::hash(image_bytes).to_hex().to_string();
    let clip_dir = state.data_dir.join("clipboard_images");
    std::fs::create_dir_all(&clip_dir)
        .map_err(|e| AppError::io(format!("クリップボード保存用フォルダの作成に失敗: {}", e)))?;

    // 同一ハッシュの既存クリップファイルがあれば再利用し、なければ連番で作成
    let existing_path = std::fs::read_dir(&clip_dir).ok().and_then(|entries| {
        entries.flatten().find_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with(&format!("clip_{}", &hash_hex[..24])) {
                Some(e.path())
            } else {
                None
            }
        })
    });

    let file_path = match existing_path {
        Some(p) => p,
        None => {
            let seq = CLIPBOARD_SEQ.fetch_add(1, Ordering::Relaxed);
            let p = clip_dir.join(format!("clip_{}_{}.{}", &hash_hex[..24], seq, ext));
            std::fs::write(&p, image_bytes)
                .map_err(|e| AppError::io(format!("クリップボード画像の保存に失敗: {}", e)))?;
            p
        }
    };

    register_dropped_paths_to_board_core(
        state,
        board_id,
        vec![file_path.to_string_lossy().to_string()],
        x,
        y,
    )
}

/// ムードボードを削除
#[command]
pub async fn delete_board(
    state: State<'_, Arc<AppState>>,
    id: i64,
) -> Result<serde_json::Value, AppError> {
    {
        let writer = state.db.writer();
        crate::db::repo::delete_board(&writer, id)?;
    }
    cleanup_orphan_clipboard_files(&state);
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

/// ムードボードへ直接ドロップされた画像ファイル群を登録・配置する
///
/// @param state アプリケーション共有状態
/// @param board_id ボードID
/// @param paths ドロップされたファイルパス配列
/// @param x 配置先X座標（オプション）
/// @param y 配置先Y座標（オプション）
/// @return ボード内アイテム一覧
#[command]
pub async fn add_dropped_files_to_board(
    state: State<'_, Arc<AppState>>,
    board_id: i64,
    paths: Vec<String>,
    x: Option<f64>,
    y: Option<f64>,
) -> Result<Vec<BoardItem>, AppError> {
    let state_clone = Arc::clone(&state);
    tauri::async_runtime::spawn_blocking(move || {
        register_dropped_paths_to_board_core(&state_clone, board_id, paths, x, y)
    })
    .await
    .map_err(|e| AppError::internal(format!("ドロップ処理タスク失敗: {}", e)))?
}

/// クリップボード上の画像をムードボードへ直接貼り付け・配置する
///
/// 変更理由: 巨大画像によるIPC負荷を軽減するため、Base64文字列（image_data）による転送をサポートする。
///
/// @param state アプリケーション共有状態
/// @param board_id ボードID
/// @param image_bytes 画像バイナリ配列（後方互換用、オプション）
/// @param image_data Base64 Data URL またはプレーンBase64文字列（オプション）
/// @param extension 画像拡張子（省略時は "png"）
/// @param x 配置先X座標（オプション）
/// @param y 配置先Y座標（オプション）
/// @return ボード内アイテム一覧
#[command]
pub async fn add_clipboard_image_to_board(
    state: State<'_, Arc<AppState>>,
    board_id: i64,
    image_bytes: Option<Vec<u8>>,
    image_data: Option<String>,
    extension: Option<String>,
    x: Option<f64>,
    y: Option<f64>,
) -> Result<Vec<BoardItem>, AppError> {
    let (bytes, detected_ext) = extract_image_bytes(image_data.as_deref(), image_bytes)?;
    let final_ext = extension.or(detected_ext);
    let state_clone = Arc::clone(&state);
    tauri::async_runtime::spawn_blocking(move || {
        register_clipboard_image_to_board_core(
            &state_clone,
            board_id,
            &bytes,
            final_ext.as_deref(),
            x,
            y,
        )
    })
    .await
    .map_err(|e| AppError::internal(format!("クリップボード貼り付けタスク失敗: {}", e)))?
}

/// Webからドロップまたは取得された画像データをアプリ領域へ保存し、元のURLを保持してムードボードへ配置する共通処理
///
/// 変更理由: Webブラウザ（Chrome/Firefox）からの画像D&Dにおいて、画像実体を安全にローカル保存しつつ
/// 元のWeb画像URL（source_url）をDBに記録し、後からブラウザで開けるようにするため。
/// Base64（image_data）による高速転送、HTML混入ガード（Content-Type）、50MBサイズ上限、連番による一意化に対応。
///
/// @param state アプリケーション共有状態
/// @param board_id 追加先ボードID
/// @param image_bytes 画像バイナリデータ（オプション）
/// @param image_data Base64 Data URL またはプレーンBase64文字列（オプション）
/// @param image_url 画像自体のダウンロード用URL（オプション）
/// @param source_url 元記事・出典WebページのURL（オプション）
/// @param extension 拡張子（オプション）
/// @param x 配置先のキャンバスX座標（オプション）
/// @param y 配置先のキャンバスY座標（オプション）
/// @return 更新後のボード内全アイテム一覧
pub fn register_web_image_to_board_core(
    state: &AppState,
    board_id: i64,
    image_bytes: Option<Vec<u8>>,
    image_data: Option<&str>,
    image_url: Option<&str>,
    source_url: Option<&str>,
    extension: Option<&str>,
    x: Option<f64>,
    y: Option<f64>,
) -> Result<Vec<BoardItem>, AppError> {
    let has_bytes = image_bytes.as_ref().map_or(false, |b| !b.is_empty());
    let has_data = image_data.map_or(false, |s| !s.trim().is_empty());

    let (bytes, detected_ext) = if has_bytes || has_data {
        extract_image_bytes(image_data, image_bytes)?
    } else {
        let download_url = image_url
            .or(source_url)
            .ok_or_else(|| AppError::invalid_argument("画像データまたはURLが指定されていません"))?;
        let trimmed = download_url.trim();
        if trimmed.starts_with("data:image/") {
            extract_image_bytes(Some(trimmed), None)?
        } else if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            let agent = ureq::AgentBuilder::new()
                .timeout(std::time::Duration::from_secs(10))
                .build();
            let user_agent = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";
            let accept_hdr = "image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8";
            let referer_candidate = source_url
                .map(|s| s.trim())
                .filter(|s| s.starts_with("http://") || s.starts_with("https://"))
                .map(|s| s.to_string())
                .or_else(|| {
                    url::Url::parse(trimmed)
                        .ok()
                        .map(|u| format!("{}://{}/", u.scheme(), u.host_str().unwrap_or("")))
                });

            let mut req = agent
                .get(trimmed)
                .set("User-Agent", user_agent)
                .set("Accept", accept_hdr);
            if let Some(ref referer) = referer_candidate {
                req = req.set("Referer", referer);
            }

            let response = match req.call() {
                Ok(resp) => resp,
                Err(_) if referer_candidate.is_some() => {
                    // Referer付きで失敗した場合はRefererなしでフォールバック再試行
                    agent
                        .get(trimmed)
                        .set("User-Agent", user_agent)
                        .set("Accept", accept_hdr)
                        .call()
                        .map_err(|e| AppError::io(format!("Web画像のダウンロードに失敗: {}", e)))?
                }
                Err(e) => return Err(AppError::io(format!("Web画像のダウンロードに失敗: {}", e))),
            };

            let content_type = response.header("Content-Type").unwrap_or("").to_lowercase();
            if content_type.contains("text/html") || content_type.contains("application/xhtml") {
                return Err(AppError::invalid_argument("指定されたURLは画像ではなくWebページ(HTML)です"));
            }

            let ext = if content_type.contains("image/png") {
                Some("png".to_string())
            } else if content_type.contains("image/jpeg") {
                Some("jpg".to_string())
            } else if content_type.contains("image/webp") {
                Some("webp".to_string())
            } else if content_type.contains("image/gif") {
                Some("gif".to_string())
            } else if content_type.contains("image/bmp") {
                Some("bmp".to_string())
            } else {
                None
            };

            const MAX_WEB_IMAGE_SIZE: u64 = 50 * 1024 * 1024; // 50MB上限
            let mut reader = response.into_reader().take(MAX_WEB_IMAGE_SIZE + 1);
            let mut buffer = Vec::new();
            reader.read_to_end(&mut buffer)
                .map_err(|e| AppError::io(format!("画像レスポンスの読み取りに失敗: {}", e)))?;
            if buffer.len() as u64 > MAX_WEB_IMAGE_SIZE {
                return Err(AppError::invalid_argument("Web画像のサイズが上限(50MB)を超えています"));
            }
            (buffer, ext)
        } else {
            return Err(AppError::invalid_argument("未対応のURL形式です"));
        }
    };

    if bytes.is_empty() {
        return Err(AppError::invalid_argument("画像データが空です"));
    }

    let ext = extension
        .map(|s| s.trim().trim_start_matches('.').to_ascii_lowercase())
        .or(detected_ext)
        .or_else(|| {
            if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
                Some("png".to_string())
            } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
                Some("jpg".to_string())
            } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
                Some("webp".to_string())
            } else if bytes.starts_with(b"GIF8") {
                Some("gif".to_string())
            } else if bytes.starts_with(b"BM") {
                Some("bmp".to_string())
            } else {
                None
            }
        })
        .filter(|s| matches!(s.as_str(), "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp"))
        .unwrap_or_else(|| "png".to_string());

    let hash_hex = blake3::hash(&bytes).to_hex().to_string();
    let clips_dir = state.data_dir.join("web_clips");
    std::fs::create_dir_all(&clips_dir)
        .map_err(|e| AppError::io(format!("Web保存用フォルダの作成に失敗: {}", e)))?;

    // 同一ハッシュの既存クリップファイルがあれば再利用し、なければ連番で作成
    let existing_path = std::fs::read_dir(&clips_dir).ok().and_then(|entries| {
        entries.flatten().find_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with(&format!("web_{}", &hash_hex[..24])) {
                Some(e.path())
            } else {
                None
            }
        })
    });

    let file_path = match existing_path {
        Some(p) => p,
        None => {
            let seq = WEB_CLIP_SEQ.fetch_add(1, Ordering::Relaxed);
            let p = clips_dir.join(format!("web_{}_{}.{}", &hash_hex[..24], seq, ext));
            std::fs::write(&p, &bytes)
                .map_err(|e| AppError::io(format!("Web画像の保存に失敗: {}", e)))?;
            p
        }
    };

    let norm_path = normalize_path(&file_path.to_string_lossy());
    let meta = std::fs::metadata(&file_path)
        .map_err(|e| AppError::io(format!("ファイル情報の取得に失敗: {}", e)))?;
    let size = meta.len();
    let now = chrono::Utc::now().timestamp();
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(now);

    let mut file = std::fs::File::open(&file_path)
        .map_err(|e| AppError::io(format!("ファイルのオープンに失敗: {}", e)))?;
    let quick_hash = crate::pipeline::hash::calculate_quick_hash_from_file(&mut file, size)
        .map_err(|e| AppError::io(format!("ハッシュ計算に失敗: {}", e)))?;
    let img_meta = crate::pipeline::exif::extract_metadata_from_file_with_plugins(
        &file_path,
        &mut file,
        mtime,
        Some(&state.format_plugins),
    );

    // 出典元URL: 明示的な source_url（記事URL）があればそれを優先し、なければ image_url を記録
    let final_source_url = source_url
        .or(image_url)
        .map(|s| s.trim())
        .filter(|s| s.starts_with("http://") || s.starts_with("https://"))
        .map(|s| s.to_string());

    let record = crate::db::repo::NewImageRecord {
        folder_id: 0,
        file_path: norm_path,
        file_size: size,
        file_mtime: mtime,
        taken_at: img_meta.taken_at,
        taken_at_source: img_meta.taken_at_source,
        width: img_meta.width,
        height: img_meta.height,
        orientation: img_meta.orientation,
        format: img_meta.format,
        quick_hash,
        indexed_at: now,
        source_url: final_source_url,
    };

    let mut thumbs_to_enqueue = Vec::new();
    let items = {
        let writer = state.db.writer();
        let img_id = crate::db::repo::upsert_single_image_and_get_id(&writer, &record)?;
        if let Ok(Some(src)) = crate::db::repo::get_thumb_source(&writer, img_id) {
            if src.thumb_status != 1 {
                thumbs_to_enqueue.push(src);
            }
        }
        crate::db::repo::add_board_items_at(&writer, board_id, &[img_id], x, y, now)?
    };

    for src in thumbs_to_enqueue {
        state.thumb_pipeline.enqueue_source(&src, JobPriority::High);
    }

    Ok(items)
}

/// Webからドロップされた画像バイナリ、Base64データ、またはURLを登録し、ムードボードに配置
///
/// 変更理由: ブラウザ（Chrome/Firefox）からの画像D&Dを受け入れ、元URL（source_url）とともに
/// 安全にローカルへ保存・永続化してボード上に配置するため。
///
/// @param state アプリケーション共有状態
/// @param board_id 追加先ボードID
/// @param image_bytes 画像バイナリデータ（オプション）
/// @param image_data Base64 Data URL またはプレーンBase64文字列（オプション）
/// @param image_url 画像ダウンロード用URL（オプション）
/// @param source_url 元記事・出典WebページのURL（オプション）
/// @param extension 拡張子（オプション）
/// @param x 配置キャンバスX座標（オプション）
/// @param y 配置キャンバスY座標（オプション）
/// @return 更新後のボード内全アイテム一覧
#[command]
pub async fn add_web_image_to_board(
    state: State<'_, Arc<AppState>>,
    board_id: i64,
    image_bytes: Option<Vec<u8>>,
    image_data: Option<String>,
    image_url: Option<String>,
    source_url: Option<String>,
    extension: Option<String>,
    x: Option<f64>,
    y: Option<f64>,
) -> Result<Vec<BoardItem>, AppError> {
    let state_clone = Arc::clone(&state);
    tauri::async_runtime::spawn_blocking(move || {
        register_web_image_to_board_core(
            &state_clone,
            board_id,
            image_bytes,
            image_data.as_deref(),
            image_url.as_deref(),
            source_url.as_deref(),
            extension.as_deref(),
            x,
            y,
        )
    })
    .await
    .map_err(|e| AppError::internal(format!("Web画像追加タスク失敗: {}", e)))?
}

/// 画像の元URLをOSの既定ブラウザで開く
///
/// 変更理由: Webからドロップされた画像の出典元ページ・高解像度画像をOS既定のブラウザで開くため。
/// @param app Tauriアプリケーションハンドル
/// @param url 開く対象のWeb URL (http:// または https://)
#[command]
pub async fn open_source_url(
    app: AppHandle,
    url: String,
) -> Result<(), AppError> {
    let trimmed = url.trim();
    if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        return Err(AppError::invalid_argument("有効なWeb URL(http/https)ではありません"));
    }

    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(trimmed, None::<&str>)
        .map_err(|e| AppError::internal(format!("ブラウザでのURLオープンに失敗: {}", e)))?;

    Ok(())
}

/// エクスプローラ等からドロップされたファイル群の実体や既存登録を逆引きし、ムードボードに配置する共通処理
///
/// 変更理由: dragDropEnabled: false の環境下でエクスプローラからドロップされた際、
/// DOMのFileオブジェクトからはフルパスが得られないが、ファイル名・サイズ・更新時刻から
/// 既存カタログまたは監視フォルダ／デスクトップ配下の実パスを逆引きして、
/// 無駄なWebクリップ複製を防ぎ、.clip等のプラグイン独自形式も確実にインデックス配置できるようにするため。
///
/// @param state アプリケーション共有状態
/// @param board_id 追加先ボードID
/// @param candidates ドロップされたファイル候補一覧
/// @param base_x 配置開始X座標（オプション）
/// @param base_y 配置開始Y座標（オプション）
/// @return 配置結果と未解決候補のインデックス一覧
pub fn resolve_dropped_files_to_board_core(
    state: &AppState,
    board_id: i64,
    candidates: Vec<DroppedFileCandidate>,
    base_x: Option<f64>,
    base_y: Option<f64>,
) -> Result<ResolveDroppedFilesResult, AppError> {
    let mut direct_image_ids: Vec<(usize, i64)> = Vec::new();
    let mut resolved_paths: Vec<(usize, PathBuf)> = Vec::new();
    let mut unresolved_indices: Vec<usize> = Vec::new();

    {
        let conn = state.db.reader()?;
        let watched_folders = crate::db::repo::get_watched_folders(&conn)?;

        // 候補探索用ディレクトリ一覧（監視フォルダ + 一般的なフォルダ）
        let mut search_dirs: Vec<PathBuf> = watched_folders
            .iter()
            .map(|f| PathBuf::from(&f.path))
            .filter(|p| p.exists() && p.is_dir())
            .collect();

        if let Some(dl) = dirs::download_dir() {
            if dl.exists() && !search_dirs.contains(&dl) {
                search_dirs.push(dl);
            }
        }
        if let Some(desk) = dirs::desktop_dir() {
            if desk.exists() && !search_dirs.contains(&desk) {
                search_dirs.push(desk);
            }
        }
        if let Some(pic) = dirs::picture_dir() {
            if pic.exists() && !search_dirs.contains(&pic) {
                search_dirs.push(pic);
            }
        }
        if let Some(doc) = dirs::document_dir() {
            if doc.exists() && !search_dirs.contains(&doc) {
                search_dirs.push(doc);
            }
        }

        for (idx, cand) in candidates.into_iter().enumerate() {
            let mtime_secs = cand.last_modified.map(|m| {
                if m > 1_000_000_000_000 {
                    m / 1000
                } else {
                    m
                }
            });

            // 1. カタログDBから逆引き
            if let Ok(Some((img_id, _path))) = crate::db::repo::find_image_by_filename_and_size(
                &conn,
                &cand.name,
                cand.size,
                mtime_secs,
            ) {
                direct_image_ids.push((idx, img_id));
                continue;
            }

            // 2. 実ファイルシステムから検索（監視フォルダ & ユーザーフォルダ）
            let mut found_path: Option<PathBuf> = None;
            for dir in &search_dirs {
                // 直下を素早く確認
                let direct_child = dir.join(&cand.name);
                if direct_child.is_file() {
                    if let Ok(meta) = direct_child.metadata() {
                        if meta.len() == cand.size {
                            found_path = Some(direct_child);
                            break;
                        }
                    }
                }

                // 直下になければ深さ3階層まで探索
                for entry in walkdir::WalkDir::new(dir)
                    .max_depth(3)
                    .into_iter()
                    .filter_entry(|e| {
                        let name = e.file_name().to_string_lossy();
                        !name.starts_with('.')
                            && name != "node_modules"
                            && name != "target"
                            && name != "AppData"
                    })
                    .flatten()
                {
                    if entry.file_type().is_file()
                        && entry.file_name().to_string_lossy() == cand.name
                    {
                        if let Ok(meta) = entry.metadata() {
                            if meta.len() == cand.size {
                                found_path = Some(entry.into_path());
                                break;
                            }
                        }
                    }
                }
                if found_path.is_some() {
                    break;
                }
            }

            if let Some(p) = found_path {
                resolved_paths.push((idx, p));
            } else {
                unresolved_indices.push(idx);
            }
        }
    }

    // 解決したアイテムを配置
    let now = chrono::Utc::now().timestamp();

    // A. 既存の image_id で見つかったものを配置
    if !direct_image_ids.is_empty() {
        let writer = state.db.writer();
        for (idx, img_id) in direct_image_ids {
            let offset_x = base_x.map(|x| x + (idx as f64) * 36.0);
            let offset_y = base_y.map(|y| y + (idx as f64) * 36.0);
            let _ = crate::db::repo::add_board_items_at(&writer, board_id, &[img_id], offset_x, offset_y, now)?;
        }
    }

    // B. 実ファイルパスで見つかったものをインデックス＆配置
    if !resolved_paths.is_empty() {
        for (idx, path) in resolved_paths {
            let offset_x = base_x.map(|x| x + (idx as f64) * 36.0);
            let offset_y = base_y.map(|y| y + (idx as f64) * 36.0);
            let str_path = path.to_string_lossy().to_string();
            if register_dropped_paths_to_board_core(
                state,
                board_id,
                vec![str_path],
                offset_x,
                offset_y,
            ).is_err() {
                unresolved_indices.push(idx);
            }
        }
    }

    // 最新のボードアイテム一覧を返す
    let conn = state.db.reader()?;
    let latest_items = crate::db::repo::get_board_items(&conn, board_id)?;

    Ok(ResolveDroppedFilesResult {
        items: latest_items,
        unresolved_indices,
    })
}

/// エクスプローラ等からドロップされたファイル群の実体や既存登録を逆引きし、ムードボードに配置する
///
/// 変更理由: dragDropEnabled: false の環境下でエクスプローラからドロップされた際、
/// DOMのFileオブジェクトからはフルパスが得られないが、ファイル名・サイズ・更新時刻から
/// 既存カタログまたは監視フォルダ／デスクトップ配下の実パスを逆引きして、
/// 無駄なWebクリップ複製を防ぎ、.clip等のプラグイン独自形式も確実にインデックス配置できるようにするため。
///
/// @param state アプリケーション共有状態
/// @param board_id 追加先ボードID
/// @param candidates ドロップされたファイル候補一覧
/// @param x 配置開始X座標（オプション）
/// @param y 配置開始Y座標（オプション）
/// @return 配置結果と未解決候補のインデックス一覧
#[command]
pub async fn resolve_dropped_files_to_board(
    state: State<'_, Arc<AppState>>,
    board_id: i64,
    candidates: Vec<DroppedFileCandidate>,
    x: Option<f64>,
    y: Option<f64>,
) -> Result<ResolveDroppedFilesResult, AppError> {
    let state_clone = Arc::clone(&state);
    tauri::async_runtime::spawn_blocking(move || {
        resolve_dropped_files_to_board_core(&state_clone, board_id, candidates, x, y)
    })
    .await
    .map_err(|e| AppError::internal(format!("ドロップファイル解決タスク失敗: {}", e)))?
}

/// ドロップされたフォルダ名と直下エントリ情報からローカルPC上の該当絶対パスを逆引き探索する
///
/// @param state アプリケーション共有状態
/// @param folder_name フォルダ名
/// @param child_names 直下の子ファイル/フォルダ名一覧
/// @return 特定できたフォルダパス（見つからない場合はNone）
fn find_matching_folder_path(
    state: &AppState,
    folder_name: &str,
    child_names: &[String],
) -> Option<PathBuf> {
    let mut candidate_roots = Vec::new();

    // 1. 登録済み監視フォルダ、その親、その祖父
    if let Ok(conn) = state.db.reader() {
        if let Ok(watched) = crate::db::repo::get_watched_folders(&conn) {
            for wf in watched {
                let p = PathBuf::from(&wf.path);
                if let Some(parent) = p.parent() {
                    candidate_roots.push(parent.to_path_buf());
                    if let Some(gparent) = parent.parent() {
                        candidate_roots.push(gparent.to_path_buf());
                    }
                }
            }
        }
    }

    // 2. ユーザー標準フォルダ
    if let Some(p) = dirs::picture_dir() {
        candidate_roots.push(p);
    }
    if let Some(p) = dirs::download_dir() {
        candidate_roots.push(p);
    }
    if let Some(p) = dirs::desktop_dir() {
        candidate_roots.push(p);
    }
    if let Some(p) = dirs::document_dir() {
        candidate_roots.push(p);
    }
    if let Some(p) = dirs::home_dir() {
        candidate_roots.push(p);
    }

    // 3. ドライブ直下（Windows）
    #[cfg(target_os = "windows")]
    {
        for b in b'C'..=b'Z' {
            let drive = format!("{}:\\", b as char);
            let drive_path = PathBuf::from(&drive);
            if drive_path.exists() {
                candidate_roots.push(drive_path);
            }
        }
    }

    // 重複除去
    candidate_roots.sort();
    candidate_roots.dedup();

    let target_name_lower = folder_name.to_lowercase();
    let mut best_match: Option<(PathBuf, usize)> = None;

    for root in candidate_roots {
        if !root.is_dir() {
            continue;
        }

        // ドライブ直下は深さ2まで、ユーザーフォルダや監視フォルダ周辺は深さ3まで探索
        let max_depth = if root.parent().is_none() { 2 } else { 3 };

        for entry in walkdir::WalkDir::new(&root)
            .min_depth(1)
            .max_depth(max_depth)
            .into_iter()
            .filter_entry(|e| {
                if !e.file_type().is_dir() {
                    return false;
                }
                let n = e.file_name().to_string_lossy();
                !n.starts_with('.')
                    && !n.starts_with('$')
                    && n != "Windows"
                    && n != "Program Files"
                    && n != "Program Files (x86)"
                    && n != "ProgramData"
                    && n != "AppData"
                    && n != "node_modules"
                    && n != "target"
            })
            .flatten()
        {
            let path = entry.path();
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };

            if name.to_lowercase() == target_name_lower {
                let mut score = 1;
                if !child_names.is_empty() {
                    if let Ok(sub_entries) = std::fs::read_dir(path) {
                        let actual_children: std::collections::HashSet<String> = sub_entries
                            .flatten()
                            .filter_map(|e| e.file_name().into_string().ok())
                            .map(|s| s.to_lowercase())
                            .collect();

                        for c in child_names {
                            if actual_children.contains(&c.to_lowercase()) {
                                score += 10;
                            }
                        }
                    }
                }

                if let Some((_, best_score)) = &best_match {
                    if score > *best_score {
                        best_match = Some((path.to_path_buf(), score));
                    }
                } else {
                    best_match = Some((path.to_path_buf(), score));
                }
            }
        }
    }

    best_match.map(|(p, _)| p)
}

/// ドロップされたフォルダ名と直下エントリ情報からローカルパスを特定し、監視フォルダとして追加
///
/// 変更理由: HTML5 D&D API (dragDropEnabled: false) ではセキュリティ上ローカル絶対パスが取得できないため、
/// ユーザーが普段利用する既知のディレクトリや既登録フォルダ周辺から一致する実パスを逆引き特定し、
/// サイドバーへのスムーズなフォルダD&D体験を維持するため。
///
/// @param app Tauriアプリケーションハンドル
/// @param state アプリケーション共有状態
/// @param folder_name ドロップされたフォルダ名
/// @param child_names 直下の子ファイル/フォルダ名一覧
/// @return 登録された監視フォルダ情報
#[command]
pub async fn resolve_and_add_dropped_folder(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    folder_name: String,
    child_names: Vec<String>,
) -> Result<WatchedFolder, AppError> {
    let found_path = find_matching_folder_path(&state, &folder_name, &child_names)
        .ok_or_else(|| AppError::not_found(format!("フォルダ [{}] のローカルパスを特定できませんでした", folder_name)))?;

    add_watch_folder(app, state, found_path.to_string_lossy().to_string()).await
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
    {
        let writer = state.db.writer();
        crate::db::repo::delete_board_item(&writer, id)?;
    }
    cleanup_orphan_clipboard_files(&state);
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

/// Android 11+ (API 30+) の「すべてのファイルへのアクセス（MANAGE_EXTERNAL_STORAGE）」権限状態を確認する
///
/// 変更理由: Scoped Storage 環境下において .clip 等の独自形式ファイルの走査や、
/// 他アプリが作成したファイルの削除が可能かどうかをフロントエンドUIで判定するため。
///
/// @param app Tauri AppHandle
/// @return 全ファイルアクセス権限が付与されている場合は true
#[command]
pub async fn check_all_files_access(#[allow(unused_variables)] app: AppHandle) -> Result<bool, AppError> {
    #[cfg(target_os = "android")]
    {
        crate::android_native::check_all_files_access(&app)
    }
    #[cfg(not(target_os = "android"))]
    {
        Ok(true)
    }
}

/// Android 11+ (API 30+) の「すべてのファイルへのアクセス」権限設定画面を明示的に開く
///
/// 変更理由: 起動時の強制設定画面遷移を行わず、ユーザーがフォルダ追加モーダルまたは
/// プラグイン設定モーダルから任意に全ファイルアクセス権限を許可できるようにするため。
///
/// @param app Tauri AppHandle
/// @return 現在の全ファイルアクセス権限状態
#[command]
pub async fn request_all_files_access(#[allow(unused_variables)] app: AppHandle) -> Result<bool, AppError> {
    #[cfg(target_os = "android")]
    {
        crate::android_native::request_all_files_access(&app)
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
        // 変更理由: スタンドアロンまたは監視画像削除時にサムネイル進捗カウンタをDBの最新状態と即座に同期するため
        state.init_thumb_progress();
        if let Ok((done, failed, total)) = crate::db::repo::get_thumb_progress(&conn) {
            let _ = app.emit(
                "thumb-progress",
                crate::models::ThumbProgress {
                    done,
                    failed,
                    total,
                },
            );
        }
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

// -----------------------------------------------------------------------------
// 画像本体のクリップボードコピー & サブフォルダ階層 & タグ管理コマンド
// -----------------------------------------------------------------------------

/// 画像本体をOSのシステムクリップボードへコピーする内部コア処理
///
/// 変更理由: タイムライン・ビューア・ムードボードのすべてにおいて、右クリックメニューおよび
/// Ctrl+C ショートカットから原寸画像（.clip等のプラグイン形式はプレビューPNG）を
/// 外部アプリ（CLIP STUDIO PAINT, Photoshop, Webブラウザ等）へ直接貼り付け可能にするため。
pub fn copy_image_to_clipboard_core(
    state: &Arc<AppState>,
    image_id: Option<i64>,
    file_path: Option<String>,
) -> Result<(), AppError> {
    let (resolved_path, orientation) = if let Some(id) = image_id {
        let conn = state.db.reader()?;
        let src = crate::db::repo::get_thumb_source(&conn, id)?
            .ok_or_else(|| AppError::not_found("指定された画像が見つかりません"))?;
        (PathBuf::from(src.file_path), src.orientation)
    } else if let Some(p) = file_path {
        let trimmed = p.trim();
        if trimmed.is_empty() {
            return Err(AppError::invalid_argument("ファイルパスが空です"));
        }
        (PathBuf::from(trimmed), 1)
    } else {
        return Err(AppError::invalid_argument(
            "imageId または filePath のいずれかを指定してください",
        ));
    };

    if !resolved_path.exists() {
        return Err(AppError::not_found(format!(
            "画像ファイルが存在しません: {}",
            resolved_path.display()
        )));
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        use std::borrow::Cow;
        use std::fs::File;
        use std::io::BufReader;

        let ext = resolved_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        // 1. プラグイン対応フォーマット（.clip等）の場合はプラグインから内蔵プレビューバイト列を抽出してデコード
        let dyn_img = if let Some(plugin) = state.format_plugins.get_by_extension(ext) {
            let (preview_bytes, _) = plugin
                .extract_original_preview(&resolved_path)
                .map_err(|e| AppError::internal(format!("プラグインプレビュー抽出失敗: {}", e)))?;
            image::load_from_memory(&preview_bytes)
                .map_err(|e| AppError::internal(format!("プレビュー画像のデコードに失敗: {}", e)))?
        } else {
            let file = File::open(&resolved_path)
                .map_err(|e| AppError::io(format!("画像ファイルを開けません: {}", e)))?;
            let mut reader = image::ImageReader::new(BufReader::new(file))
                .with_guessed_format()
                .map_err(|e| AppError::io(format!("画像フォーマットの判定に失敗: {}", e)))?;
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(20_000);
            limits.max_image_height = Some(20_000);
            limits.max_alloc = Some(512 * 1024 * 1024);
            reader.limits(limits);
            reader
                .decode()
                .map_err(|e| AppError::internal(format!("画像のデコードに失敗: {}", e)))?
        };

        let oriented = crate::pipeline::thumbnail::apply_orientation(dyn_img, orientation);
        let rgba = oriented.to_rgba8();
        let (width, height) = rgba.dimensions();
        let raw_bytes = rgba.into_raw();

        let mut clipboard = arboard::Clipboard::new().map_err(|e| {
            AppError::internal(format!("システムクリップボードの初期化に失敗: {}", e))
        })?;

        let img_data = arboard::ImageData {
            width: width as usize,
            height: height as usize,
            bytes: Cow::Owned(raw_bytes),
        };

        clipboard.set_image(img_data).map_err(|e| {
            AppError::internal(format!("クリップボードへの画像書き込みに失敗: {}", e))
        })?;

        info!(
            "Copied image to clipboard: path={:?} ({}x{})",
            resolved_path, width, height
        );
        Ok(())
    }

    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        let _ = orientation;
        Err(AppError::invalid_argument(
            "モバイル環境ではシステムクリップボードへの直接画像コピーはサポートされていません",
        ))
    }
}

/// 指定した画像本体（RGBAピクセルデータ）をOSのシステムクリップボードへコピーするコマンド
///
/// @param state アプリケーション共有状態
/// @param image_id 対象画像ID（オプション）
/// @param file_path 対象画像ファイルパス（オプション）
#[command]
pub async fn copy_image_to_clipboard(
    state: State<'_, Arc<AppState>>,
    image_id: Option<i64>,
    file_path: Option<String>,
) -> Result<(), AppError> {
    let state_clone = Arc::clone(&state);
    tauri::async_runtime::spawn_blocking(move || {
        copy_image_to_clipboard_core(&state_clone, image_id, file_path)
    })
    .await
    .map_err(|e| AppError::internal(format!("スレッド実行エラー: {}", e)))?
}

/// 指定した監視フォルダ配下のサブフォルダ階層ツリーと各画像枚数を取得するコマンド
///
/// @param state アプリケーション共有状態
/// @param folder_id 監視フォルダID
/// @return SubfolderEntry の配列
#[command]
pub async fn get_subfolder_tree(
    state: State<'_, Arc<AppState>>,
    folder_id: i64,
) -> Result<Vec<SubfolderEntry>, AppError> {
    let conn = state.db.reader()?;
    let entries = crate::db::repo::get_subfolder_tree(&conn, folder_id)?;
    Ok(entries)
}

/// 登録されている全タグ一覧（付与画像数付き）を取得するコマンド
#[command]
pub async fn get_tags(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<TagWithCount>, AppError> {
    let conn = state.db.reader()?;
    let tags = crate::db::repo::get_tags(&conn)?;
    Ok(tags)
}

/// 新規タグを作成するコマンド（同名タグが存在する場合は既存タグを返却）
#[command]
pub async fn create_tag(
    state: State<'_, Arc<AppState>>,
    name: String,
    color: Option<String>,
) -> Result<TagRecord, AppError> {
    let clean = name.trim();
    if clean.is_empty() {
        return Err(AppError::invalid_argument("タグ名を入力してください"));
    }
    let conn = state.db.writer();
    let now = chrono::Utc::now().timestamp();
    let record = crate::db::repo::create_tag(&conn, clean, color.as_deref(), now)?;
    Ok(record)
}

/// タグの名前・カラー・表示順を更新するコマンド
#[command]
pub async fn update_tag(
    state: State<'_, Arc<AppState>>,
    id: i64,
    name: Option<String>,
    color: Option<String>,
    sort_order: Option<i32>,
) -> Result<TagRecord, AppError> {
    let conn = state.db.writer();
    let record = crate::db::repo::update_tag(
        &conn,
        id,
        name.as_deref(),
        color.as_deref(),
        sort_order,
    )?;
    state.bump_catalog_version();
    Ok(record)
}

/// タグを削除するコマンド
#[command]
pub async fn delete_tag(
    state: State<'_, Arc<AppState>>,
    id: i64,
) -> Result<(), AppError> {
    let conn = state.db.writer();
    crate::db::repo::delete_tag(&conn, id)?;
    state.bump_catalog_version();
    Ok(())
}

/// 複数画像に対する各タグの付与状態（all / some / none）を取得するコマンド
#[command]
pub async fn get_tags_for_images(
    state: State<'_, Arc<AppState>>,
    image_ids: Vec<i64>,
) -> Result<Vec<ImageTagAssignment>, AppError> {
    let conn = state.db.reader()?;
    let assignments = crate::db::repo::get_tags_for_images(&conn, &image_ids)?;
    Ok(assignments)
}

/// 複数画像に対してタグの一括追加・一括解除を実行するコマンド
#[command]
pub async fn update_images_tags(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    image_ids: Vec<i64>,
    add_tag_ids: Vec<i64>,
    remove_tag_ids: Vec<i64>,
) -> Result<(), AppError> {
    {
        let mut conn = state.db.writer();
        let now = chrono::Utc::now().timestamp();
        crate::db::repo::update_images_tags(
            &mut conn,
            &image_ids,
            &add_tag_ids,
            &remove_tag_ids,
            now,
        )?;
    }
    let version = state.bump_catalog_version();
    let _ = app.emit(
        "catalog-changed",
        crate::models::CatalogChangedPayload { version },
    );
    Ok(())
}






