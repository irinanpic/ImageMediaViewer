pub mod commands;
pub mod db;
pub mod error;
pub mod models;
pub mod pipeline;
pub mod protocol;
pub mod scanner;
pub mod server;
pub mod state;

use std::sync::Arc;
use tauri::Manager;
use tracing::info;

use crate::db::Database;
use crate::pipeline::ThumbnailPipeline;
use crate::state::AppState;

/// Tauri アプリケーションのエントリポイント
///
/// 変更理由: Windows 11 Canary等のWebView2ランタイム初期化ハングを完全に防止するため、
/// デフォルトで軽量かつ堅牢なスタンドアロンHTTPサーバーモードで稼働し、
/// 独立ウィンドウ(Electron/Edge App Mode)およびブラウザへ安定したAPI・画像配信を提供する
pub fn run() {
    // ログ初期化
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,image_media_viewer=debug".into()),
        )
        .init();

    info!("ImageMediaViewer バックエンド初期化開始");

    let args: Vec<String> = std::env::args().collect();
    let use_tauri_gui = args.iter().any(|a| a == "--tauri-gui");

    let app_data_dir = std::env::var("APPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("com.imagemediaviewer.app");
    let app_cache_dir = std::env::var("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("com.imagemediaviewer.app");

    std::fs::create_dir_all(&app_data_dir).ok();
    std::fs::create_dir_all(&app_cache_dir).ok();

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
    crate::server::start_http_server(app_state.clone(), 14201);
    // 処理がないときに登録フォルダ内の更新を低優先度で自動チェックするアイドルウォッチャーを起動
    crate::scanner::start_idle_watcher(app_state.clone());
    info!("ImageMediaViewer バックエンドサーバー稼働開始: db_path={:?}, port=14201", db_path);

    if use_tauri_gui {
        info!("Tauri GUI モードを起動します");
        let builder = tauri::Builder::default()
            .plugin(tauri_plugin_dialog::init())
            .plugin(tauri_plugin_opener::init());

        builder
            .setup(move |app| {
                app.manage(app_state);
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
            ])
            .run(tauri::generate_context!())
            .expect("Tauri アプリケーション実行中にエラーが発生しました");
    } else {
        // スタンドアロンサーバーモード: メインスレッドを安全に維持
        info!("スタンドアロンサーバーモードで待機中...");
        loop {
            std::thread::sleep(std::time::Duration::from_secs(3600));
        }
    }
}
