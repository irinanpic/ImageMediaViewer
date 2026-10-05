pub mod commands;
pub mod db;
pub mod error;
pub mod logger;
pub mod models;
pub mod pipeline;
pub mod protocol;
pub mod scanner;
pub mod server;
pub mod settings;
pub mod state;
pub mod tray;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::Manager;
use tracing::info;

use crate::db::Database;
use crate::pipeline::ThumbnailPipeline;
use crate::state::AppState;

/// Tauri アプリケーションのエントリポイント
///
/// 変更理由: TCP/IPポートを一切使用しない完全ポートレス一体型アーキテクチャへの刷新。
/// Tauri Native IPC とカスタムプロトコル（thumb://, original://）により、
/// ポート競合ゼロ、ファイアウォール警告ゼロの安定したネイティブデスクトップアプリを実現する。
/// メインウィンドウのクローズ時は常駐設定（stay_in_tray）に応じてトレイ常駐または完全終了を行う。
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
    if !app_data_dir.join("catalog.db").exists() {
        if let Some(parent) = app_data_dir.parent() {
            let old_dir = parent.join("com.imagemediaviewer.app");
            if old_dir.join("catalog.db").exists() {
                for file_name in &["catalog.db", "catalog.db-wal", "catalog.db-shm", "window_state.json", "app_settings.json"] {
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

    // アプリケーション設定（常駐フラグ等）の読み込み
    let app_settings = crate::settings::load_settings(&app_data_dir);
    let stay_in_tray = Arc::new(AtomicBool::new(app_settings.stay_in_tray));

    info!(
        "ImageMediaViewer 初期化開始 (ログ先: {:?}, 常駐設定: {})",
        app_data_dir.join("logs"),
        app_settings.stay_in_tray
    );

    let db_path = app_data_dir.join("catalog.db");
    let db = Database::open(&db_path).expect("カタログDBの初期化に失敗");

    // サムネイル専用DB（thumbnails.db）をキャッシュディレクトリ内に初期化
    let thumb_store = Arc::new(
        crate::db::ThumbnailStore::open(&app_cache_dir)
            .expect("サムネイル専用DBの初期化に失敗"),
    );

    // 過去に失敗（thumb_status = 2）と判定されていた画像を未生成（0）へリセット
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

    // アイドルウォッチャーをバックグラウンド起動
    crate::scanner::start_idle_watcher(app_state.clone());

    let state_for_tray = Arc::clone(&app_state);
    let stay_for_tray = Arc::clone(&stay_in_tray);
    let stay_for_window = Arc::clone(&stay_in_tray);
    let app_data_dir_clone = app_data_dir.clone();

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            info!("二重起動を検知しました。既存のメインウィンドウを表示・復元します。");
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init());

    // ポートレスカスタムプロトコル（thumb://, original://）の登録
    let builder = crate::protocol::register_protocols(builder, Arc::clone(&app_state));

    let builder = builder
        .on_window_event(move |window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if stay_for_window.load(Ordering::Relaxed) {
                    // 常駐 ON: ウィンドウを閉じるのをキャンセルし、非表示にしてタスクトレイに残す
                    api.prevent_close();
                    let _ = window.hide();
                    info!("メインウィンドウを非表示にし、タスクトレイ常駐に移行しました");
                } else {
                    // 常駐 OFF: アプリケーション全体を完全終了する
                    info!("常駐設定がOFFのため、ウィンドウ終了に伴いアプリケーションを完全終了します");
                    window.app_handle().exit(0);
                }
            }
        })
        .setup(move |app| {
            app.manage(app_state);

            // システムトレイアイコンおよび右クリックメニューの登録
            if let Err(e) = crate::tray::setup_system_tray(&app.handle(), state_for_tray, stay_for_tray, app_data_dir_clone) {
                tracing::warn!("システムトレイアイコンの登録に失敗しました: {:?}", e);
            }

            // メインウィンドウを表示
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
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
            commands::clear_queue,
            commands::rescan_missing_thumbnails,
            commands::get_thumb_progress,
            commands::get_window_state,
            commands::save_window_state,
            commands::get_boards,
            commands::create_board,
            commands::update_board,
            commands::delete_board,
            commands::get_board_items,
            commands::add_board_items,
            commands::update_board_item,
            commands::delete_board_item,
            commands::get_board_notes,
            commands::create_board_note,
            commands::update_board_note,
            commands::delete_board_note,
            commands::get_bookmarks,
            commands::create_bookmark,
            commands::delete_bookmark,
            commands::heartbeat,
        ]);

    builder
        .run(tauri::generate_context!())
        .expect("Tauri アプリケーション実行中にエラーが発生しました");
}
