use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, UNIX_EPOCH};
use tiny_http::{Header, Method, Response, Server, StatusCode};
use tracing::{error, info, warn};

use crate::error::AppError;
use crate::models::GetImagesPayload;
use crate::pipeline::exif::extract_metadata;
use crate::pipeline::hash::calculate_quick_hash;
use crate::pipeline::thumbnail::get_thumbnail_path;
use crate::pipeline::JobPriority;
use crate::scanner::walker::{is_sub_directory, normalize_path};
use crate::state::AppState;

// 最後にフロントエンドから通信があった時刻（UNIX秒）
static LAST_CLIENT_SEEN: AtomicI64 = AtomicI64::new(0);
// 少なくとも1回フロントエンドから通信があったフラグ
static CLIENT_CONNECTED: AtomicBool = AtomicBool::new(false);
// アイドル自動終了（--auto-exit）モードフラグ（常駐モード時はfalse）
static AUTO_EXIT_ON_IDLE: AtomicBool = AtomicBool::new(false);

/// 許可されたオリジンの一覧（ホワイトリスト）
///
/// 変更理由: Localhost HTTP サーバーにおける CORS 全許可 (*) による写真・メタデータ漏洩の防止
const ALLOWED_ORIGINS: &[&str] = &[
    "http://127.0.0.1:14201",
    "http://localhost:14201",
    "http://127.0.0.1:5173",
    "http://localhost:5173",
    "tauri://localhost",
    "http://tauri.localhost",
    "https://tauri.localhost",
];

/// リクエストの Origin ヘッダーを取得
fn get_request_origin(request: &tiny_http::Request) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Origin"))
        .map(|h| h.value.as_str().to_string())
}

/// レスポンスにCORSヘッダーを付与する（ホワイトリストに含まれるOriginのみ許可）
fn apply_cors_headers<R: std::io::Read>(resp: &mut Response<R>, origin: Option<&str>) {
    if let Some(orig) = origin {
        if ALLOWED_ORIGINS.contains(&orig) {
            if let Ok(hdr) = Header::from_bytes(&b"Access-Control-Allow-Origin"[..], orig.as_bytes()) {
                resp.add_header(hdr);
            }
            resp.add_header(Header::from_bytes(&b"Vary"[..], &b"Origin"[..]).unwrap());
        }
    }
    resp.add_header(Header::from_bytes(&b"Access-Control-Allow-Methods"[..], &b"GET, POST, PUT, DELETE, OPTIONS"[..]).unwrap());
    resp.add_header(Header::from_bytes(&b"Access-Control-Allow-Headers"[..], &b"Content-Type, Range"[..]).unwrap());
}

/// CORSヘッダーを付与してリクエストに応答する
fn respond_cors<R: std::io::Read>(request: tiny_http::Request, mut resp: Response<R>, origin: Option<&str>) {
    apply_cors_headers(&mut resp, origin);
    let _ = request.respond(resp);
}

/// フロントエンドのアクティビティを記録
fn record_client_activity() {
    let now = chrono::Utc::now().timestamp();
    LAST_CLIENT_SEEN.store(now, Ordering::Relaxed);
    CLIENT_CONNECTED.store(true, Ordering::Relaxed);
}

/// JSON レスポンスを生成するヘルパー関数
fn json_response<T: serde::Serialize>(data: &T, status: u16) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = serde_json::to_vec(data).unwrap_or_else(|_| b"{}".to_vec());
    let mut resp = Response::from_data(body).with_status_code(StatusCode(status));
    resp.add_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
    resp
}

/// エラーレスポンスを生成するヘルパー関数
fn error_response(err: AppError, status: u16) -> Response<std::io::Cursor<Vec<u8>>> {
    json_response(&err, status)
}

/// OPTIONS (CORS preflight) レスポンス
fn options_response() -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_data(Vec::new()).with_status_code(StatusCode(204))
}

/// 拡張子に応じたMIMEタイプを取得
fn get_mime_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("bmp") => "image/bmp",
        _ => "application/octet-stream",
    }
}

/// ウィンドウ状態設定ファイルの保存先パスを解決
fn get_window_state_path() -> PathBuf {
    // 1. プロジェクトルート探索（exeのパスから辿る）
    if let Ok(exe_path) = std::env::current_exe() {
        let mut cur = exe_path.clone();
        for _ in 0..5 {
            if let Some(parent) = cur.parent() {
                cur = parent.to_path_buf();
                if cur.join("package.json").exists() || cur.join("run.bat").exists() {
                    return cur.join("window_state.json");
                }
            }
        }
    }

    // 2. カレントディレクトリ
    if let Ok(cwd) = std::env::current_dir() {
        if cwd.join("run.bat").exists() || cwd.join("package.json").exists() {
            return cwd.join("window_state.json");
        }
    }

    // 3. フォールバック（インストール版）: 書き込み可能なユーザーデータ領域に保存
    // 変更理由: Program Files 配下などカレントディレクトリが書き込み不可でも状態を保存できるようにする
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

/// ウィンドウ状態の読み込み
fn load_window_state() -> crate::models::WindowState {
    let path = get_window_state_path();
    if let Ok(content) = std::fs::read_to_string(&path) {
        if let Ok(state) = serde_json::from_str::<crate::models::WindowState>(&content) {
            return state;
        }
    }
    crate::models::WindowState::default()
}

/// ウィンドウ状態の保存
fn save_window_state(state: &crate::models::WindowState) -> std::io::Result<()> {
    let path = get_window_state_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let json = serde_json::to_string_pretty(state).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(&path, json)
}

/// サムネイル画像バイナリレスポンスを生成（永続不変キャッシュ）
fn image_response(data: Vec<u8>, mime: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let mut resp = Response::from_data(data).with_status_code(StatusCode(200));
    resp.add_header(Header::from_bytes(&b"Content-Type"[..], mime.as_bytes()).unwrap());
    resp.add_header(Header::from_bytes(&b"Cache-Control"[..], &b"public, max-age=31536000, immutable"[..]).unwrap());
    resp.add_header(Header::from_bytes(&b"Connection"[..], &b"keep-alive"[..]).unwrap());
    resp
}

/// 原寸画像バイナリレスポンスを生成（外部編集の反映のため must-revalidate を適用）
fn raw_image_response(data: Vec<u8>, mime: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let mut resp = Response::from_data(data).with_status_code(StatusCode(200));
    resp.add_header(Header::from_bytes(&b"Content-Type"[..], mime.as_bytes()).unwrap());
    resp.add_header(Header::from_bytes(&b"Cache-Control"[..], &b"public, max-age=3600, must-revalidate"[..]).unwrap());
    resp.add_header(Header::from_bytes(&b"Connection"[..], &b"keep-alive"[..]).unwrap());
    resp
}


/// フロントエンド(dist/)の探索候補ディレクトリを優先順に返す
///
/// 変更理由: インストール版ではカレントディレクトリが不定のため、実行ファイル基準の
/// `dist`（Tauri の bundle.resources で同梱）を最優先で探索する必要がある。
/// 開発時（target/debug|release 配下からの実行）は従来の相対パスにフォールバックする。
///
/// @return 探索候補ディレクトリの配列（優先順）
fn dist_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            // インストール版: <インストール先>/dist
            dirs.push(exe_dir.join("dist"));
            // 開発時: src-tauri/target/{debug,release} -> プロジェクトルート/dist
            if let Some(root) = exe_dir.ancestors().nth(3) {
                dirs.push(root.join("dist"));
            }
        }
    }
    dirs.push(PathBuf::from("dist"));
    dirs.push(PathBuf::from("../dist"));
    dirs
}

/// dist/ ディレクトリ内の静的フロントエンドアセットを配信する
///
/// 変更理由: `..` を含むパスで dist 外のファイルを読み出せないよう拒否する（パストラバーサル対策）。
///
/// @param path リクエストパス（例: "/assets/index.js"）
/// @return 見つかった場合はレスポンス、見つからない・不正なパスの場合は None
fn static_response(path: &str) -> Option<Response<std::io::Cursor<Vec<u8>>>> {
    let clean_path = path.trim_start_matches('/');
    let target_file = if clean_path.is_empty() { "index.html" } else { clean_path };

    // 親ディレクトリ参照・絶対パス・ドライブ指定を拒否
    let has_unsafe_component = std::path::Path::new(target_file)
        .components()
        .any(|c| !matches!(c, std::path::Component::Normal(_)));
    if has_unsafe_component || target_file.contains('\\') {
        return None;
    }

    for dir in &dist_search_dirs() {
        let file_path = dir.join(target_file);
        if file_path.is_file() {
            if let Ok(bytes) = std::fs::read(&file_path) {
                let mime = match file_path.extension().and_then(|e| e.to_str()).unwrap_or("") {
                    "html" => "text/html; charset=utf-8",
                    "js" => "application/javascript; charset=utf-8",
                    "css" => "text/css; charset=utf-8",
                    "svg" => "image/svg+xml",
                    "webp" => "image/webp",
                    "png" => "image/png",
                    "ico" => "image/x-icon",
                    _ => "application/octet-stream",
                };
                let mut resp = Response::from_data(bytes).with_status_code(StatusCode(200));
                resp.add_header(Header::from_bytes(&b"Content-Type"[..], mime.as_bytes()).unwrap());
                return Some(resp);
            }
        }
    }
    None
}

/// バックエンド HTTP サーバーをバックグラウンドスレッドで起動
///
/// 変更理由: Windows環境でのスレッド生成オーバーヘッドを排除する固定スレッドプール（12スレッド）を導入し、
/// インメモリキャッシュ（メタデータ＋WebPバイナリ）と連携してPicasa同等の極限レスポンスを実現。
/// また、ユーザー要求「サーバはクライアントによらず常駐することも可能とする」に対応し、
/// 常駐モードではフロントエンド通信途絶による自動停止を無効化する。
///
/// @param state アプリケーション共有状態
/// @param port リッスンするポート番号
/// @param auto_exit_on_idle フロントエンド無通信時に自動終了するか（常駐時はfalse）
pub fn start_http_server(state: Arc<AppState>, port: u16, auto_exit_on_idle: bool) {
    AUTO_EXIT_ON_IDLE.store(auto_exit_on_idle, Ordering::SeqCst);
    let addr = format!("127.0.0.1:{}", port);
    thread::spawn(move || {
        let server = match Server::http(&addr) {
            Ok(s) => {
                info!("Started ImageMediaViewer local HTTP server: http://{}", addr);
                Arc::new(s)
            }
            Err(e) => {
                error!("Failed to start local HTTP server ({}): {:?}", addr, e);
                return;
            }
        };

        // 固定ワーカースレッドプール（CPUコア数×2、8〜16スレッド）
        let num_cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        let worker_count = (num_cpus * 2).clamp(8, 16);
        info!("HTTP server worker thread pool started: {} threads", worker_count);

        let (tx, rx) = std::sync::mpsc::sync_channel::<tiny_http::Request>(512);
        let rx = Arc::new(Mutex::new(rx));

        for i in 0..worker_count {
            let rx = Arc::clone(&rx);
            let state = state.clone();
            thread::Builder::new()
                .name(format!("http-worker-{}", i))
                .spawn(move || {
                    while let Ok(request) = {
                        let lock = rx.lock().unwrap();
                        lock.recv()
                    } {
                        handle_http_request(request, &state);
                    }
                })
                .expect("Failed to spawn HTTP worker thread");
        }

        // フロントエンド監視スレッド（auto_exit_on_idle が有効な場合のみ稼働）:
        // 常駐モード時は自動終了せず、タスクトレイまたはAPI経由の明示的終了指示まで常駐し続ける。
        if auto_exit_on_idle {
            info!("Idle auto-exit is enabled (terminates after 10s of inactivity)");
            thread::Builder::new()
                .name("frontend-heartbeat-watcher".to_string())
                .spawn(|| {
                    loop {
                        thread::sleep(Duration::from_secs(2));
                        if CLIENT_CONNECTED.load(Ordering::Relaxed) {
                            let last = LAST_CLIENT_SEEN.load(Ordering::Relaxed);
                            let now = chrono::Utc::now().timestamp();
                            if now - last > 10 {
                                info!("Detected frontend termination (10s inactivity). Terminating backend.");
                                std::process::exit(0);
                            }
                        }
                    }
                })
                .expect("Failed to spawn heartbeat watcher thread");
        } else {
            info!("Running in resident server mode (remains in system tray after frontend exits)");
        }

        for request in server.incoming_requests() {
            if let Err(e) = tx.send(request) {
                warn!("HTTP request queue send error: {:?}", e);
            }
        }
    });
}

/// 単一 HTTP リクエストのルーティングと処理
fn handle_http_request(mut request: tiny_http::Request, state: &Arc<AppState>) {
    // 通信アクティビティを記録（ハートビート同期）
    record_client_activity();

    let origin = get_request_origin(&request);
    let origin_ref = origin.as_deref();

    // CSRF対策: ブラウザからのクロスオリジン通信で Origin がホワイトリストに含まれない場合は即座に拒否 (403 Forbidden)
    // 悪意のあるWebサイトからの単純リクエスト（<form> POST や fetch）によるシャットダウン・フォルダ改変等を完全遮断
    if let Some(ref orig) = origin {
        if !ALLOWED_ORIGINS.contains(&orig.as_str()) {
            warn!("CSRF Protection: Rejected request from unauthorized Origin: {}", orig);
            let resp = Response::from_string("Forbidden: Invalid Origin").with_status_code(StatusCode(403));
            let _ = request.respond(resp);
            return;
        }
    }

    let url_str = request.url().to_string();
    let method = request.method().clone();

    if method == Method::Options {
        respond_cors(request, options_response(), origin_ref);
        return;
    }

    let url_parts: Vec<&str> = url_str.split('?').collect();
    let path = url_parts[0];
    let query = url_parts.get(1).copied().unwrap_or("");

    // 1. サムネイル画像配信: /thumbs/:id
    // 変更理由: 
    // 1) タイムライン表示は速度最優先: インメモリキャッシュおよび thumb_store (SQLite) を最優先で確認し、
    //    同期ディスクメタデータ I/O を行わずに即時返却（0ms / <0.5ms）して 60fps 仮想スクロールを維持。
    // 2) キャッシュ未存在時のみ実ファイル変更検知を行い、更新があれば低優先度（JobPriority::Low）でキュー投入。
    // 3) ファイル更新時も既存キャッシュがあれば即座に返却して表示を優先し、未生成時のみ 503 で解放。
    if path.starts_with("/thumbs/") && method == Method::Get {
        let id_str = path.trim_start_matches("/thumbs/");
        if let Ok(id) = id_str.parse::<i64>() {
            if let Some(mut source) = state.get_thumb_source_cached(id) {
                // 1. インメモリWebPバイナリキャッシュにあれば即返却 (0ms)
                if let Some(cached_bytes) = state.get_thumbnail_bytes(&source.quick_hash) {
                    respond_cors(request, image_response((*cached_bytes).clone(), "image/webp"), origin_ref);
                    return;
                }

                // 2. サムネイル専用DB (thumbnails.db) から BLOB 取得 (< 0.5ms)
                if let Ok(Some(bytes)) = state.thumb_store.get(&source.quick_hash) {
                    let arc_bytes = Arc::new(bytes);
                    state.put_thumbnail_bytes(source.quick_hash.clone(), Arc::clone(&arc_bytes));
                    respond_cors(request, image_response((*arc_bytes).clone(), "image/webp"), origin_ref);
                    return;
                }

                // 3. 旧ディスクキャッシュ (thumbs/v1/{hash[0..2]}/{hash}.webp) にあれば読み込んでDB移行＆返却
                let thumb_path = get_thumbnail_path(&state.cache_dir, &source.quick_hash);
                if thumb_path.exists() {
                    if let Ok(bytes) = std::fs::read(&thumb_path) {
                        // thumbnails.db へ自動インポート（Lazy Migration）
                        state.thumb_store.put(&source.quick_hash, &bytes).ok();
                        let arc_bytes = Arc::new(bytes);
                        state.put_thumbnail_bytes(source.quick_hash.clone(), Arc::clone(&arc_bytes));
                        respond_cors(request, image_response((*arc_bytes).clone(), "image/webp"), origin_ref);
                        return;
                    }
                }

                // --- キャッシュ未存在時のみ実ファイル更新検知 (On-Access Change Detection) ---
                let file_path = Path::new(&source.file_path);
                if let Ok(meta) = std::fs::metadata(file_path) {
                    let cur_size = meta.len();
                    let cur_mtime = meta
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);

                    if cur_size != source.file_size || cur_mtime != source.file_mtime {
                        // ファイルが更新されている！更新有無をチェックしてDB更新
                        if let Ok(new_hash) = calculate_quick_hash(file_path, cur_size) {
                            let new_meta = extract_metadata(file_path, cur_mtime);
                            let conn = state.db.writer();
                            let _ = crate::db::repo::update_image_on_file_changed(
                                &conn,
                                source.id,
                                cur_size,
                                cur_mtime,
                                &new_hash,
                                new_meta.width,
                                new_meta.height,
                                new_meta.orientation,
                            );

                            let old_hash = source.quick_hash.clone();
                            source.file_size = cur_size;
                            source.file_mtime = cur_mtime;
                            source.quick_hash = new_hash;
                            source.width = new_meta.width;
                            source.height = new_meta.height;
                            source.orientation = new_meta.orientation;
                            source.thumb_status = 0;

                            state.update_cached_thumb_source(source.clone());

                            // 低優先度（JobPriority::Low）で新サムネイル生成キューに投入（表示優先）
                            state.thumb_pipeline.enqueue_source(&source, JobPriority::Low);

                            // 表示優先: 既存の旧サムネイルが存在する場合は即座に返却（スピナーにしない）
                            if let Some(cached_bytes) = state.get_thumbnail_bytes(&old_hash) {
                                respond_cors(request, image_response((*cached_bytes).clone(), "image/webp"), origin_ref);
                                return;
                            }
                            if let Ok(Some(bytes)) = state.thumb_store.get(&old_hash) {
                                let arc_bytes = Arc::new(bytes);
                                respond_cors(request, image_response((*arc_bytes).clone(), "image/webp"), origin_ref);
                                return;
                            }

                            // 既存キャッシュが一切ない場合のみ 503 を返却
                            let mut resp = Response::from_string("Thumbnail updating").with_status_code(StatusCode(503));
                            resp.add_header(Header::from_bytes(&b"Retry-After"[..], &b"1"[..]).unwrap());
                            resp.add_header(Header::from_bytes(&b"Cache-Control"[..], &b"no-store, no-cache, must-revalidate"[..]).unwrap());
                            respond_cors(request, resp, origin_ref);
                            return;
                        }
                    }
                }

                // 4. サムネイル未存在の場合:
                if source.thumb_status != 0 {
                    let conn = state.db.writer();
                    let _ = crate::db::repo::update_thumb_status(&conn, source.id, 0);
                }

                // 高速化パイプラインへ高優先度で投入
                state.thumb_pipeline.enqueue_source(&source, JobPriority::High);

                // HTTPスレッドは待機せず即座に 503 を返却（0ms解放）
                let mut resp = Response::from_string("Thumbnail generating").with_status_code(StatusCode(503));
                resp.add_header(Header::from_bytes(&b"Retry-After"[..], &b"1"[..]).unwrap());
                resp.add_header(Header::from_bytes(&b"Cache-Control"[..], &b"no-store, no-cache, must-revalidate"[..]).unwrap());
                respond_cors(request, resp, origin_ref);
                return;
            }
        }
        let resp = Response::from_string("Not Found").with_status_code(StatusCode(404));
        respond_cors(request, resp, origin_ref);
        return;
    }

    // 2. 原寸画像バイナリ配信: /raw/:id
    // 変更理由: 原寸画像には must-revalidate を付与し、外部編集時の更新検知とブラウザキャッシュ整合性を両立
    if path.starts_with("/raw/") && method == Method::Get {
        let id_str = path.trim_start_matches("/raw/");
        if let Ok(id) = id_str.parse::<i64>() {
            let file_info = match state.db.reader() {
                Ok(conn) => crate::db::repo::get_image_file_info(&conn, id).unwrap_or(None),
                Err(_) => None,
            };

            if let Some((orig_path_str, hash, orig_mtime, _)) = file_info {
                let orig_path = PathBuf::from(&orig_path_str);
                // 1. 実ファイルが存在する場合は原寸ファイルを配信
                if orig_path.exists() && orig_path.is_file() {
                    // 原寸アクセス時も更新をチェック
                    if let Ok(meta) = std::fs::metadata(&orig_path) {
                        let cur_mtime = meta
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                            .map(|d| d.as_secs() as i64)
                            .unwrap_or(0);
                        if cur_mtime != orig_mtime {
                            // 更新検知時はメタデータ更新をトリガー
                            if let Ok(new_hash) = calculate_quick_hash(&orig_path, meta.len()) {
                                let new_meta = extract_metadata(&orig_path, cur_mtime);
                                let conn = state.db.writer();
                                let _ = crate::db::repo::update_image_on_file_changed(
                                    &conn,
                                    id,
                                    meta.len(),
                                    cur_mtime,
                                    &new_hash,
                                    new_meta.width,
                                    new_meta.height,
                                    new_meta.orientation,
                                );

                                // 旧サムネイルキャッシュを無効化し、新サムネイルを高優先度で再生成キューに投入
                                state.invalidate_thumbnail_cache(&hash);
                                if let Some(mut updated_src) = state.get_thumb_source_cached(id) {
                                    updated_src.file_size = meta.len();
                                    updated_src.file_mtime = cur_mtime;
                                    updated_src.quick_hash = new_hash.clone();
                                    updated_src.width = new_meta.width;
                                    updated_src.height = new_meta.height;
                                    updated_src.orientation = new_meta.orientation;
                                    updated_src.thumb_status = 0;
                                    state.update_cached_thumb_source(updated_src.clone());
                                    state.thumb_pipeline.enqueue_source(&updated_src, JobPriority::Low);
                                }
                            }
                        }
                    }

                    if let Ok(bytes) = std::fs::read(&orig_path) {
                        let mime = get_mime_type(&orig_path);
                        respond_cors(request, raw_image_response(bytes, mime), origin_ref);
                        return;
                    }
                }
                // 2. 実ファイルが存在しない場合（モック等）はサムネイルをフォールバック配信
                if let Ok(Some(bytes)) = state.thumb_store.get(&hash) {
                    respond_cors(request, raw_image_response(bytes, "image/webp"), origin_ref);
                    return;
                }
                let thumb_path = get_thumbnail_path(&state.cache_dir, &hash);
                if thumb_path.exists() {
                    if let Ok(bytes) = std::fs::read(&thumb_path) {
                        respond_cors(request, raw_image_response(bytes, "image/webp"), origin_ref);
                        return;
                    }
                }
            }
        }
        let resp = Response::from_string("Not Found").with_status_code(StatusCode(404));
        respond_cors(request, resp, origin_ref);
        return;
    }

                // 3. REST API ルーティング
                let result = match (method, path) {
                    (Method::Get, "/api/watch_folders") => {
                        match state.db.reader() {
                            Ok(conn) => match crate::db::repo::get_watched_folders(&conn) {
                                Ok(folders) => json_response(&folders, 200),
                                Err(e) => error_response(AppError::from(e), 500),
                            },
                            Err(e) => error_response(AppError::from(e), 500),
                        }
                    }

                    (Method::Post, "/api/watch_folders") => {
                        let mut body_str = String::new();
                        let _ = request.as_reader().read_to_string(&mut body_str);
                        #[derive(serde::Deserialize)]
                        struct AddFolderReq { path: String }

                        match serde_json::from_str::<AddFolderReq>(&body_str) {
                            Ok(req) => {
                                let normalized = normalize_path(&req.path);
                                let path_obj = Path::new(&normalized);
                                if !path_obj.exists() || !path_obj.is_dir() {
                                    error_response(AppError::invalid_argument("指定されたディレクトリが存在しません"), 400)
                                } else {
                                    let existing_res = match state.db.reader() {
                                        Ok(conn) => crate::db::repo::get_watched_folders(&conn),
                                        Err(e) => Err(e),
                                    };

                                    match existing_res {
                                        Ok(existing) => {
                                            let mut is_child = false;
                                            for folder in &existing {
                                                if is_sub_directory(&folder.path, &normalized) {
                                                    is_child = true;
                                                    break;
                                                }
                                            }

                                            if is_child {
                                                error_response(AppError::invalid_argument("指定されたフォルダは既に登録済みの親フォルダの配下です"), 400)
                                            } else {
                                                let mut writer = state.db.writer();
                                                for folder in &existing {
                                                    if is_sub_directory(&normalized, &folder.path) {
                                                        let _ = crate::db::repo::remove_watched_folder(&mut writer, folder.id);
                                                    }
                                                }
                                                let now = chrono::Utc::now().timestamp();
                                                match crate::db::repo::add_watched_folder(&mut writer, &normalized, now) {
                                                    Ok(folder) => {
                                                        let s = state.clone();
                                                        let f_id = folder.id;
                                                        let f_path = folder.path.clone();
                                                        thread::spawn(move || {
                                                            let _ = crate::scanner::scan_folder_core(None, s, f_id, f_path, false);
                                                        });
                                                        json_response(&folder, 200)
                                                    }
                                                    Err(e) => error_response(AppError::from(e), 500),
                                                }
                                            }
                                        }
                                        Err(e) => error_response(AppError::from(e), 500),
                                    }
                                }
                            }
                            Err(_) => error_response(AppError::invalid_argument("Invalid JSON"), 400),
                        }
                    }

                    (Method::Delete, p) if p.starts_with("/api/watch_folders/") => {
                        let id_str = p.trim_start_matches("/api/watch_folders/");
                        if let Ok(id) = id_str.parse::<i64>() {
                            let mut writer = state.db.writer();
                            match crate::db::repo::remove_watched_folder(&mut writer, id) {
                                Ok(_) => json_response(&serde_json::json!({ "success": true }), 200),
                                Err(e) => error_response(AppError::from(e), 500),
                            }
                        } else {
                            error_response(AppError::invalid_argument("無効なID"), 400)
                        }
                    }

                    (Method::Post, "/api/rescan") => {
                        let mut body_str = String::new();
                        let _ = request.as_reader().read_to_string(&mut body_str);
                        #[derive(serde::Deserialize)]
                        struct RescanReq { folder_id: Option<i64> }
                        let req: RescanReq = serde_json::from_str(&body_str).unwrap_or(RescanReq { folder_id: None });

                        if let Ok(conn) = state.db.reader() {
                            if let Ok(folders) = crate::db::repo::get_watched_folders(&conn) {
                                for folder in folders {
                                    if req.folder_id.is_none() || req.folder_id == Some(folder.id) {
                                        let s = state.clone();
                                        let f_id = folder.id;
                                        let f_path = folder.path;
                                        thread::spawn(move || {
                                            let _ = crate::scanner::scan_folder_core(None, s, f_id, f_path, false);
                                        });
                                    }
                                }
                            }
                        }
                        json_response(&serde_json::json!({ "success": true }), 200)
                    }

                    (Method::Post, "/api/thumbnails/rescan_missing") => {
                        let writer = state.db.writer();
                        let reset_count = crate::db::repo::reset_failed_thumbnails(&writer).unwrap_or(0);
                        state.thumb_pipeline.trigger_background_refill();
                        info!("Reset thumbnail failure flags (Targets: {})", reset_count);
                        json_response(&serde_json::json!({
                            "success": true,
                            "resetCount": reset_count
                        }), 200)
                    }

                    (Method::Get, "/api/timeline/summary") => {
                        let folder_id = query.split('&').find_map(|pair| {
                            let mut parts = pair.split('=');
                            if parts.next() == Some("folderId") {
                                parts.next().and_then(|v| v.parse::<i64>().ok())
                            } else {
                                None
                            }
                        });

                        let sort = query.split('&').find_map(|pair| {
                            let mut parts = pair.split('=');
                            if parts.next() == Some("sort") {
                                match parts.next() {
                                    Some("takenAtAsc") => Some(crate::models::TimelineSort::TakenAtAsc),
                                    Some("folder") => Some(crate::models::TimelineSort::Folder),
                                    Some("nameAsc") => Some(crate::models::TimelineSort::NameAsc),
                                    Some("nameDesc") => Some(crate::models::TimelineSort::NameDesc),
                                    _ => Some(crate::models::TimelineSort::TakenAtDesc),
                                }
                            } else {
                                None
                            }
                        });

                        let current_version = state.current_catalog_version();
                        match state.db.reader() {
                            Ok(conn) => match crate::db::repo::get_timeline_summary(&conn, folder_id, current_version, sort) {
                                Ok(summary) => json_response(&summary, 200),
                                Err(e) => error_response(AppError::from(e), 500),
                            },
                            Err(e) => error_response(AppError::from(e), 500),
                        }
                    }

                    (Method::Post, "/api/timeline/images") => {
                        let mut body_str = String::new();
                        let _ = request.as_reader().read_to_string(&mut body_str);
                        match serde_json::from_str::<GetImagesPayload>(&body_str) {
                            Ok(payload) => {
                                let limit = payload.limit.clamp(1, 500);
                                match state.db.reader() {
                                    Ok(conn) => match crate::db::repo::get_timeline_images(&conn, payload.offset, limit, payload.folder_id, payload.sort) {
                                        Ok(images) => json_response(&images, 200),
                                        Err(e) => error_response(AppError::from(e), 500),
                                    },
                                    Err(e) => error_response(AppError::from(e), 500),
                                }
                            }
                            Err(_) => error_response(AppError::invalid_argument("Invalid JSON"), 400),
                        }
                    }


                    (Method::Get, p) if p.starts_with("/api/images/") => {
                        let id_str = p.trim_start_matches("/api/images/");
                        if let Ok(id) = id_str.parse::<i64>() {
                            match state.db.reader() {
                                Ok(conn) => match crate::db::repo::get_image_detail(&conn, id) {
                                    Ok(Some(detail)) => json_response(&detail, 200),
                                    Ok(None) => error_response(AppError::not_found("画像が見つかりません"), 404),
                                    Err(e) => error_response(AppError::from(e), 500),
                                },
                                Err(e) => error_response(AppError::from(e), 500),
                            }
                        } else {
                            error_response(AppError::invalid_argument("無効なID"), 400)
                        }
                    }

                    (Method::Post, "/api/viewport") => {
                        let mut body_str = String::new();
                        let _ = request.as_reader().read_to_string(&mut body_str);
                        #[derive(serde::Deserialize)]
                        #[serde(rename_all = "camelCase")]
                        struct ViewportReq {
                            visible_ids: Vec<i64>,
                            nearby_ids: Vec<i64>,
                        }

                        match serde_json::from_str::<ViewportReq>(&body_str) {
                            Ok(req) => match state.db.reader() {
                                Ok(conn) => {
                                    let visible = crate::db::repo::get_thumb_sources(&conn, &req.visible_ids)
                                        .unwrap_or_default();
                                    let nearby = crate::db::repo::get_thumb_sources(&conn, &req.nearby_ids)
                                        .unwrap_or_default();
                                    let gen = state.thumb_pipeline.set_viewport(&visible, &nearby);
                                    json_response(
                                        &serde_json::json!({
                                            "generation": gen,
                                            "visibleCount": visible.len(),
                                            "nearbyCount": nearby.len()
                                        }),
                                        200,
                                    )
                                }
                                Err(e) => error_response(AppError::from(e), 500),
                            },
                            Err(_) => error_response(AppError::invalid_argument("無効なJSON"), 400),
                        }
                    },
                    (Method::Post, "/api/clear_queue") => {
                        let gen = state.thumb_pipeline.clear_queue();
                        json_response(
                            &serde_json::json!({
                                "generation": gen,
                                "status": "cleared"
                            }),
                            200,
                        )
                    },

                    (Method::Get, "/api/thumb_progress") => {
                        match state.db.reader() {
                            Ok(conn) => match crate::db::repo::get_thumb_progress(&conn) {
                                Ok((done, failed, total)) => {
                                    let is_idle = state.thumb_pipeline.is_idle();
                                    #[derive(serde::Serialize)]
                                    #[serde(rename_all = "camelCase")]
                                    struct ThumbProgressResp {
                                        done: u64,
                                        failed: u64,
                                        total: u64,
                                        is_generating: bool,
                                    }
                                    json_response(
                                        &ThumbProgressResp {
                                            done,
                                            failed,
                                            total,
                                            is_generating: !is_idle && (done + failed) < total,
                                        },
                                        200,
                                    )
                                }
                                Err(e) => error_response(AppError::from(e), 500),
                            },
                            Err(e) => error_response(AppError::from(e), 500),
                        }
                    },

                    (Method::Get, "/api/thumbnails/failed") => {
                        match state.db.reader() {
                            Ok(conn) => match crate::db::repo::get_failed_thumb_images(&conn, 200) {
                                Ok(records) => json_response(&records, 200),
                                Err(e) => error_response(AppError::from(e), 500),
                            },
                            Err(e) => error_response(AppError::from(e), 500),
                        }
                    },

                    (Method::Get, "/api/logs") => {
                        let limit = query.split('&').find_map(|pair| {
                            let mut parts = pair.split('=');
                            if parts.next() == Some("limit") {
                                parts.next().and_then(|v| v.parse::<usize>().ok())
                            } else {
                                None
                            }
                        }).unwrap_or(200);

                        if let Some(lm) = crate::logger::get_log_manager() {
                            let logs = lm.get_recent_logs(limit);
                            json_response(&logs, 200)
                        } else {
                            json_response(&Vec::<crate::logger::LogEntry>::new(), 200)
                        }
                    },

                    (Method::Post, "/api/logs/open") => {
                        if let Some(lm) = crate::logger::get_log_manager() {
                            let dir = lm.log_dir();
                            let _ = opener::reveal(&dir);
                            json_response(&serde_json::json!({
                                "success": true,
                                "path": dir.to_string_lossy()
                            }), 200)
                        } else {
                            error_response(AppError::internal("ロガーが初期化されていません"), 500)
                        }
                    },

                    (Method::Post, "/api/heartbeat") | (Method::Get, "/api/heartbeat") | (Method::Get, "/api/health") => {
                        record_client_activity();
                        json_response(&serde_json::json!({
                            "alive": true,
                            "status": "ok",
                            "version": "0.1.0"
                        }), 200)
                    },

                    (Method::Post, "/api/shutdown") => {
                        if AUTO_EXIT_ON_IDLE.load(Ordering::SeqCst) {
                            info!("Shutting down backend on frontend request (--auto-exit mode)");
                            thread::spawn(|| {
                                thread::sleep(Duration::from_millis(80));
                                std::process::exit(0);
                            });
                            json_response(&serde_json::json!({ "success": true, "status": "shutting_down" }), 200)
                        } else {
                            info!("Resident server mode: Ignored shutdown request and maintained tray residence");
                            json_response(&serde_json::json!({ "success": true, "status": "resident_mode_kept" }), 200)
                        }
                    }

                    (Method::Post, "/api/prefetch_thumbnails") => {
                        let mut body_str = String::new();
                        let _ = request.as_reader().read_to_string(&mut body_str);
                        #[derive(serde::Deserialize)]
                        struct PrefetchReq { ids: Vec<i64> }
                        if let Ok(req) = serde_json::from_str::<PrefetchReq>(&body_str) {
                            if let Ok(conn) = state.db.reader() {
                                if let Ok(sources) = crate::db::repo::get_thumb_sources(&conn, &req.ids) {
                                    for src in sources {
                                        state.thumb_pipeline.enqueue_source(&src, JobPriority::Mid);
                                    }
                                }
                            }
                        }
                        json_response(&serde_json::json!({ "success": true }), 200)
                    }

                    (Method::Post, "/api/reveal_in_file_manager") => {
                        let mut body_str = String::new();
                        let _ = request.as_reader().read_to_string(&mut body_str);
                        #[derive(serde::Deserialize)]
                        struct RevealReq { id: i64 }
                        if let Ok(req) = serde_json::from_str::<RevealReq>(&body_str) {
                            if let Ok(conn) = state.db.reader() {
                                if let Ok(Some((orig_path, _, _, _))) = crate::db::repo::get_image_file_info(&conn, req.id) {
                                    #[cfg(target_os = "windows")]
                                    {
                                        let _ = std::process::Command::new("explorer")
                                            .args(["/select,", &orig_path])
                                            .spawn();
                                    }
                                    #[cfg(not(target_os = "windows"))]
                                    {
                                        let _ = opener::reveal(&orig_path);
                                    }
                                    json_response(&serde_json::json!({ "success": true }), 200)
                                } else {
                                    error_response(AppError::not_found("画像が見つかりません"), 404)
                                }
                            } else {
                                error_response(AppError::internal("DB接続失敗"), 500)
                            }
                        } else {
                            error_response(AppError::invalid_argument("無効なJSON"), 400)
                        }
                    }

                    (Method::Get, "/api/window_state") => {
                        let state = load_window_state();
                        json_response(&state, 200)
                    }

                    (Method::Post, "/api/window_state") => {
                        let mut body_str = String::new();
                        let _ = request.as_reader().read_to_string(&mut body_str);
                        match serde_json::from_str::<crate::models::WindowState>(&body_str) {
                            Ok(ws) => {
                                if let Err(e) = save_window_state(&ws) {
                                    error_response(AppError::from(e), 500)
                                } else {
                                    json_response(&serde_json::json!({ "success": true }), 200)
                                }
                            }
                            Err(_) => error_response(AppError::invalid_argument("無効なJSON"), 400),
                        }
                    }

                    // ---------------------------------------------------------
                    // ムードボード（グループ & キャンバス）API
                    // ---------------------------------------------------------
                    (Method::Get, "/api/boards") => {
                        match state.db.reader() {
                            Ok(conn) => match crate::db::repo::get_boards(&conn) {
                                Ok(boards) => json_response(&boards, 200),
                                Err(e) => error_response(AppError::from(e), 500),
                            },
                            Err(e) => error_response(AppError::from(e), 500),
                        }
                    }

                    (Method::Post, "/api/boards") => {
                        let mut body_str = String::new();
                        let _ = request.as_reader().read_to_string(&mut body_str);
                        match serde_json::from_str::<crate::models::CreateBoardPayload>(&body_str) {
                            Ok(payload) => {
                                let writer = state.db.writer();
                                let now = chrono::Utc::now().timestamp();
                                match crate::db::repo::create_board(&writer, &payload, now) {
                                    Ok(board) => json_response(&board, 200),
                                    Err(e) => error_response(AppError::from(e), 500),
                                }
                            }
                            Err(_) => error_response(AppError::invalid_argument("無効なJSON"), 400),
                        }
                    }

                    (Method::Get, p) if p.starts_with("/api/boards/") && p.ends_with("/items") => {
                        let id_str = p.trim_start_matches("/api/boards/").trim_end_matches("/items");
                        if let Ok(id) = id_str.parse::<i64>() {
                            match state.db.reader() {
                                Ok(conn) => match crate::db::repo::get_board_items(&conn, id) {
                                    Ok(items) => json_response(&items, 200),
                                    Err(e) => error_response(AppError::from(e), 500),
                                },
                                Err(e) => error_response(AppError::from(e), 500),
                            }
                        } else {
                            error_response(AppError::invalid_argument("無効なID"), 400)
                        }
                    }

                    (Method::Post, p) if p.starts_with("/api/boards/") && p.ends_with("/items") => {
                        let id_str = p.trim_start_matches("/api/boards/").trim_end_matches("/items");
                        if let Ok(board_id) = id_str.parse::<i64>() {
                            let mut body_str = String::new();
                            let _ = request.as_reader().read_to_string(&mut body_str);
                            #[derive(serde::Deserialize)]
                            #[serde(rename_all = "camelCase")]
                            struct AddItemsReq { image_ids: Vec<i64> }
                            match serde_json::from_str::<AddItemsReq>(&body_str) {
                                Ok(req) => {
                                    let writer = state.db.writer();
                                    let now = chrono::Utc::now().timestamp();
                                    match crate::db::repo::add_board_items(&writer, board_id, &req.image_ids, now) {
                                        Ok(items) => json_response(&items, 200),
                                        Err(e) => error_response(AppError::from(e), 500),
                                    }
                                }
                                Err(_) => error_response(AppError::invalid_argument("無効なJSON"), 400),
                            }
                        } else {
                            error_response(AppError::invalid_argument("無効なID"), 400)
                        }
                    }

                    (Method::Put, p) if p.starts_with("/api/boards/") => {
                        let id_str = p.trim_start_matches("/api/boards/");
                        if let Ok(id) = id_str.parse::<i64>() {
                            let mut body_str = String::new();
                            let _ = request.as_reader().read_to_string(&mut body_str);
                            match serde_json::from_str::<crate::models::UpdateBoardPayload>(&body_str) {
                                Ok(payload) => {
                                    let writer = state.db.writer();
                                    let now = chrono::Utc::now().timestamp();
                                    match crate::db::repo::update_board(&writer, id, &payload, now) {
                                        Ok(_) => json_response(&serde_json::json!({ "success": true }), 200),
                                        Err(e) => error_response(AppError::from(e), 500),
                                    }
                                }
                                Err(_) => error_response(AppError::invalid_argument("無効なJSON"), 400),
                            }
                        } else {
                            error_response(AppError::invalid_argument("無効なID"), 400)
                        }
                    }

                    (Method::Delete, p) if p.starts_with("/api/boards/") => {
                        let id_str = p.trim_start_matches("/api/boards/");
                        if let Ok(id) = id_str.parse::<i64>() {
                            let writer = state.db.writer();
                            match crate::db::repo::delete_board(&writer, id) {
                                Ok(_) => json_response(&serde_json::json!({ "success": true }), 200),
                                Err(e) => error_response(AppError::from(e), 500),
                            }
                        } else {
                            error_response(AppError::invalid_argument("無効なID"), 400)
                        }
                    }

                    (Method::Put, p) if p.starts_with("/api/board_items/") => {
                        let id_str = p.trim_start_matches("/api/board_items/");
                        if let Ok(id) = id_str.parse::<i64>() {
                            let mut body_str = String::new();
                            let _ = request.as_reader().read_to_string(&mut body_str);
                            match serde_json::from_str::<crate::models::UpdateBoardItemPayload>(&body_str) {
                                Ok(mut payload) => {
                                    payload.id = id;
                                    let writer = state.db.writer();
                                    let now = chrono::Utc::now().timestamp();
                                    match crate::db::repo::update_board_item(&writer, &payload, now) {
                                        Ok(_) => json_response(&serde_json::json!({ "success": true }), 200),
                                        Err(e) => error_response(AppError::from(e), 500),
                                    }
                                }
                                Err(_) => error_response(AppError::invalid_argument("無効なJSON"), 400),
                            }
                        } else {
                            error_response(AppError::invalid_argument("無効なID"), 400)
                        }
                    }

                    (Method::Delete, p) if p.starts_with("/api/board_items/") => {
                        let id_str = p.trim_start_matches("/api/board_items/");
                        if let Ok(id) = id_str.parse::<i64>() {
                            let writer = state.db.writer();
                            match crate::db::repo::delete_board_item(&writer, id) {
                                Ok(_) => json_response(&serde_json::json!({ "success": true }), 200),
                                Err(e) => error_response(AppError::from(e), 500),
                            }
                        } else {
                            error_response(AppError::invalid_argument("無効なID"), 400)
                        }
                    }

                    // ---------------------------------------------------------
                    // ムードボード テキストメモ（付箋）API
                    // ---------------------------------------------------------
                    (Method::Get, p) if p.starts_with("/api/boards/") && p.ends_with("/notes") => {
                        let id_str = p.trim_start_matches("/api/boards/").trim_end_matches("/notes");
                        if let Ok(id) = id_str.parse::<i64>() {
                            match state.db.reader() {
                                Ok(conn) => match crate::db::repo::get_board_notes(&conn, id) {
                                    Ok(notes) => json_response(&notes, 200),
                                    Err(e) => error_response(AppError::from(e), 500),
                                },
                                Err(e) => error_response(AppError::from(e), 500),
                            }
                        } else {
                            error_response(AppError::invalid_argument("無効なID"), 400)
                        }
                    }

                    (Method::Post, p) if p.starts_with("/api/boards/") && p.ends_with("/notes") => {
                        let id_str = p.trim_start_matches("/api/boards/").trim_end_matches("/notes");
                        if let Ok(board_id) = id_str.parse::<i64>() {
                            let mut body_str = String::new();
                            let _ = request.as_reader().read_to_string(&mut body_str);
                            match serde_json::from_str::<crate::models::CreateBoardNotePayload>(&body_str) {
                                Ok(mut payload) => {
                                    payload.board_id = board_id;
                                    let writer = state.db.writer();
                                    let now = chrono::Utc::now().timestamp();
                                    match crate::db::repo::create_board_note(&writer, &payload, now) {
                                        Ok(note) => json_response(&note, 200),
                                        Err(e) => error_response(AppError::from(e), 500),
                                    }
                                }
                                Err(_) => error_response(AppError::invalid_argument("無効なJSON"), 400),
                            }
                        } else {
                            error_response(AppError::invalid_argument("無効なID"), 400)
                        }
                    }

                    (Method::Put, p) if p.starts_with("/api/board_notes/") => {
                        let id_str = p.trim_start_matches("/api/board_notes/");
                        if let Ok(id) = id_str.parse::<i64>() {
                            let mut body_str = String::new();
                            let _ = request.as_reader().read_to_string(&mut body_str);
                            match serde_json::from_str::<crate::models::UpdateBoardNotePayload>(&body_str) {
                                Ok(mut payload) => {
                                    payload.id = id;
                                    let writer = state.db.writer();
                                    let now = chrono::Utc::now().timestamp();
                                    match crate::db::repo::update_board_note(&writer, &payload, now) {
                                        Ok(_) => json_response(&serde_json::json!({ "success": true }), 200),
                                        Err(e) => error_response(AppError::from(e), 500),
                                    }
                                }
                                Err(_) => error_response(AppError::invalid_argument("無効なJSON"), 400),
                            }
                        } else {
                            error_response(AppError::invalid_argument("無効なID"), 400)
                        }
                    }

                    (Method::Delete, p) if p.starts_with("/api/board_notes/") => {
                        let id_str = p.trim_start_matches("/api/board_notes/");
                        if let Ok(id) = id_str.parse::<i64>() {
                            let writer = state.db.writer();
                            match crate::db::repo::delete_board_note(&writer, id) {
                                Ok(_) => json_response(&serde_json::json!({ "success": true }), 200),
                                Err(e) => error_response(AppError::from(e), 500),
                            }
                        } else {
                            error_response(AppError::invalid_argument("無効なID"), 400)
                        }
                    }

                    (Method::Get, "/api/bookmarks") => {
                        match state.db.reader() {
                            Ok(conn) => match crate::db::repo::get_bookmarks(&conn) {
                                Ok(bookmarks) => json_response(&bookmarks, 200),
                                Err(e) => error_response(AppError::from(e), 500),
                            },
                            Err(e) => error_response(AppError::from(e), 500),
                        }
                    }

                    (Method::Post, "/api/bookmarks") => {
                        let mut body_str = String::new();
                        let _ = request.as_reader().read_to_string(&mut body_str);
                        match serde_json::from_str::<crate::models::CreateBookmarkPayload>(&body_str) {
                            Ok(payload) => {
                                let writer = state.db.writer();
                                let now = chrono::Utc::now().timestamp();
                                match crate::db::repo::create_bookmark(&writer, &payload, now) {
                                    Ok(bookmark) => json_response(&bookmark, 200),
                                    Err(e) => error_response(AppError::from(e), 500),
                                }
                            }
                            Err(_) => error_response(AppError::invalid_argument("無効なJSON"), 400),
                        }
                    }

                    (Method::Delete, p) if p.starts_with("/api/bookmarks/") => {
                        let id = p.trim_start_matches("/api/bookmarks/");
                        if !id.is_empty() {
                            let writer = state.db.writer();
                            match crate::db::repo::delete_bookmark(&writer, id) {
                                Ok(_) => json_response(&serde_json::json!({ "success": true }), 200),
                                Err(e) => error_response(AppError::from(e), 500),
                            }
                        } else {
                            error_response(AppError::invalid_argument("無効なID"), 400)
                        }
                    }


                    _ => {
                        if let Some(resp) = static_response(path) {
                            resp
                        } else {
                            Response::from_string("Not Found").with_status_code(StatusCode(404))
                        }
                    }
                };

                respond_cors(request, result, origin_ref);
}

