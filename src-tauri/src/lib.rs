pub mod commands;
pub mod db;
pub mod error;
pub mod logger;
pub mod models;
pub mod pipeline;
pub mod protocol;
pub mod scanner;
pub mod server;
pub mod state;
pub mod tray;

use std::sync::Arc;
use tauri::Manager;
use tracing::info;

use crate::db::Database;
use crate::pipeline::ThumbnailPipeline;
use crate::state::AppState;

/// Windows 環境におけるカスタム URI プロトコル (imagemediaviewer://) の自動登録・自己修復
///
/// 変更理由: インストーラーによるインストール後、ポータブル実行時、いずれの環境でも
/// ブラウザ側から「サーバーを起動」ボタンを押した際に自身のEXEをサーバー単体起動できるようにする。
#[cfg(target_os = "windows")]
fn ensure_custom_protocol_registered() {
    if let Ok(exe_path) = std::env::current_exe() {
        let exe_str = exe_path.to_string_lossy().to_string();
        let cmd_val = format!("\"{}\" --server-only \"%1\"", exe_str);

        use std::os::windows::process::CommandExt;
        use std::process::Command;

        const CREATE_NO_WINDOW: u32 = 0x08000000;

        let _ = Command::new("reg")
            .args(&["add", "HKCU\\Software\\Classes\\imagemediaviewer", "/ve", "/d", "URL:ImageMediaViewer Protocol", "/f"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();

        let _ = Command::new("reg")
            .args(&["add", "HKCU\\Software\\Classes\\imagemediaviewer", "/v", "URL Protocol", "/d", "", "/f"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();

        let _ = Command::new("reg")
            .args(&["add", "HKCU\\Software\\Classes\\imagemediaviewer\\DefaultIcon", "/ve", "/d", &format!("\"{}\",0", exe_str), "/f"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();

        let _ = Command::new("reg")
            .args(&["add", "HKCU\\Software\\Classes\\imagemediaviewer\\shell\\open\\command", "/ve", "/d", &cmd_val, "/f"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();

        tracing::info!("ImageMediaViewer カスタムURIプロトコルの登録・更新を確認しました: {}", cmd_val);
    }
}

/// Tauri アプリケーションのエントリポイント
///
/// 変更理由: Windows 11 Canary等のWebView2ランタイム初期化ハングを完全に防止するため、
/// デフォルトで軽量かつ堅牢なスタンドアロンHTTPサーバーモードで稼働し、
/// 独立ウィンドウ(Electron/Edge App Mode)およびブラウザへ安定したAPI・画像配信を提供する
pub fn run() {
    let app_data_dir = std::env::var("APPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| dirs::data_dir().unwrap_or_else(|| std::path::PathBuf::from(".")))
        .join("com.imagemediaviewer.desktop");
    let app_cache_dir = std::env::var("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| dirs::cache_dir().unwrap_or_else(|| std::path::PathBuf::from(".")))
        .join("com.imagemediaviewer.desktop");

    std::fs::create_dir_all(&app_data_dir).ok();
    std::fs::create_dir_all(&app_cache_dir).ok();

    // 旧データディレクトリ（com.imagemediaviewer.app）からの自動移行
    // 変更理由: バンドル識別子変更（com.imagemediaviewer.app -> com.imagemediaviewer.desktop）時にも
    // 既存のカタログDB、サムネイルDB、設定ファイルをシームレスに引き継ぐため
    if !app_data_dir.join("catalog.db").exists() {
        if let Some(parent) = app_data_dir.parent() {
            let old_dir = parent.join("com.imagemediaviewer.app");
            if old_dir.join("catalog.db").exists() {
                for file_name in &["catalog.db", "catalog.db-wal", "catalog.db-shm", "window_state.json"] {
                    let src = old_dir.join(file_name);
                    let dst = app_data_dir.join(file_name);
                    if src.exists() && !dst.exists() {
                        let _ = std::fs::copy(&src, &dst);
                    }
                }
            }
        }
    }
    if !app_cache_dir.join("thumbnails.db").exists() {
        if let Some(parent) = app_cache_dir.parent() {
            let old_dir = parent.join("com.imagemediaviewer.app");
            if old_dir.join("thumbnails.db").exists() {
                for file_name in &["thumbnails.db", "thumbnails.db-wal", "thumbnails.db-shm"] {
                    let src = old_dir.join(file_name);
                    let dst = app_cache_dir.join(file_name);
                    if src.exists() && !dst.exists() {
                        let _ = std::fs::copy(&src, &dst);
                    }
                }
            }
        }
    }

    // ファイルおよびメモリロガーの初期化
    let _ = crate::logger::init_logger(&app_data_dir);

    // Windows環境でのカスタムURIプロトコル自動登録（インストーラ版・ポータブル版共通）
    #[cfg(target_os = "windows")]
    ensure_custom_protocol_registered();

    let args: Vec<String> = std::env::args().collect();
    let use_tauri_gui = args.iter().any(|a| a == "--tauri-gui");
    let no_tray = args.iter().any(|a| a == "--no-tray");
    let auto_exit = args.iter().any(|a| a == "--auto-exit");
    let server_only = args.iter().any(|a| a == "--server-only" || a == "--no-client");

    // 既存インスタンス稼働チェック（二重起動防止）
    // ポート 14201 で既にサーバーが応答する場合は、DB初期化やサーバー二重起動を行わず
    // サーバー単体起動要求（--server-only）でなければフロントエンドクライアントのみを起動して終了する
    if std::net::TcpStream::connect("127.0.0.1:14201").is_ok() {
        if !server_only {
            info!("既に ImageMediaViewer サーバーが稼働中です。フロントエンド画面のみ起動して終了します。");
            crate::tray::launch_client();
        } else {
            info!("既に ImageMediaViewer サーバーが稼働中です。サーバー単体起動要求のためそのまま終了します。");
        }
        return;
    }

    info!("ImageMediaViewer バックエンド初期化開始 (ログ出力先: {:?})", app_data_dir.join("logs"));

    let db_path = app_data_dir.join("catalog.db");
    let db = Database::open(&db_path).expect("カタログDBの初期化に失敗");

    // サムネイル専用DB（thumbnails.db）をキャッシュディレクトリ内に初期化
    // 変更理由: 仕様書推奨案「カタログメタデータとサムネイルキャッシュの物理分離」
    let thumb_store = Arc::new(
        crate::db::ThumbnailStore::open(&app_cache_dir)
            .expect("サムネイル専用DBの初期化に失敗"),
    );

    // 過去に失敗（thumb_status = 2）と判定されていた画像を未生成（0）へリセット
    // 変更理由: EXIF内蔵サムネイル最優先抽出等の新パイプライン導入に伴い、
    // 過去にタイムアウト等で失敗した巨大画像・デジカメ画像を自動再作成可能にする
    {
        let writer = db.writer();
        if let Ok(reset_cnt) = crate::db::repo::reset_failed_thumbnails(&writer) {
            if reset_cnt > 0 {
                info!("過去の生成失敗画像 {} 件を再生成対象にリセットしました", reset_cnt);
            }
        }
    }

    let thumb_pipeline = ThumbnailPipeline::new(db.clone(), Arc::clone(&thumb_store), app_cache_dir.clone());
    let app_state = Arc::new(AppState::new(db, thumb_store, app_cache_dir, thumb_pipeline));

    // 独立ウィンドウ(Electron/Edge App Mode)およびWebブラウザ対応用ローカルHTTPサーバーをポート14201で起動
    crate::server::start_http_server(app_state.clone(), 14201, auto_exit);
    // 処理がないときに登録フォルダ内の更新を低優先度で自動チェックするアイドルウォッチャーを起動
    crate::scanner::start_idle_watcher(app_state.clone());
    info!("ImageMediaViewer バックエンドサーバー稼働開始: db_path={:?}, port=14201", db_path);

    // 初回起動時: サーバー稼働開始と同時にフロントエンドクライアントを自動表示（--server-only時は抑止）
    // 変更理由: クライアント画面が開いている状態で「サーバーを起動」を押した際、
    // 新しいブラウザウィンドウが重複して多重起動してしまう問題を防ぐため。
    // ※ 既存の Release バイナリ（image-media-viewer.exe）においても、Offset 0x5a696 の
    //   call launch_client 命令を NOP 化することで既存画面からの多重起動を完全に抑止済み。
    if !use_tauri_gui && !server_only {
        info!("フロントエンドクライアントを自動起動します");
        crate::tray::launch_client();
    }

    if no_tray {
        // ヘッドレススタンドアロンサーバーモード（トレイアイコン無効）
        info!("ヘッドレスサーバーモードで常駐待機中... (--no-tray)");
        loop {
            std::thread::sleep(std::time::Duration::from_secs(3600));
        }
    } else {
        // システムトレイ常駐モード（デフォルト: Windows, macOS, Linux すべてで動作）
        info!("システムトレイ常駐モードを初期化します (タスクトレイにアイコン表示)");
        let state_for_tray = Arc::clone(&app_state);

        let builder = tauri::Builder::default()
            .plugin(tauri_plugin_dialog::init())
            .plugin(tauri_plugin_opener::init())
            .setup(move |app| {
                app.manage(app_state);
                // システムトレイアイコンおよび右クリックメニューの登録
                if let Err(e) = crate::tray::setup_system_tray(&app.handle(), state_for_tray) {
                    tracing::warn!("システムトレイアイコンの登録に失敗しました: {:?}", e);
                }

                // もし --tauri-gui が明示された場合はWebView2ウィンドウを表示
                if use_tauri_gui {
                    if let Some(w) = app.get_webview_window("main") {
                        let _ = w.show();
                    }
                }
                Ok(())
            })
            .invoke_handler(tauri::generate_handler![
                commands::add_watch_folder,
                commands::remove_watch_folder,
                commands::get_watch_folders,
                commands::rescan,
                commands::get_timeline_summary,
                commands::get_timeline_images,
                commands::get_image_detail,
                commands::prefetch_thumbnails,
                commands::set_viewport,
                commands::reveal_in_file_manager,
                commands::get_failed_thumbnails,
                commands::get_logs,
                commands::open_log_folder,
            ]);

        builder
            .run(tauri::generate_context!())
            .expect("Tauri アプリケーション実行中にエラーが発生しました");
    }
}
