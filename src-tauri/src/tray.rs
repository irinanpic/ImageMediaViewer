use std::path::PathBuf;
use std::sync::Arc;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::AppHandle;
use tracing::{error, info};

use crate::state::AppState;

/// クライアント（フロントエンド）を独立ウィンドウまたは既定ブラウザで起動する
///
/// 変更理由: Windows, macOS, Linux すべてのOSにおいて、Chrome/Edge/Chromium等の
/// アプリケーションモード（--app）で軽量・ネイティブ風にフロントエンドを起動し、
/// 見つからない場合はOS既定のブラウザで開く
pub fn launch_client() {
    let app_url = "http://127.0.0.1:14201/";

    // 1. 各OSにおけるChrome/Edge/Chromiumの候補パス探索
    let candidates = get_browser_candidates();
    let found = candidates.into_iter().find(|p| p.exists());

    if let Some(browser_exe) = found {
        let mut cmd = std::process::Command::new(&browser_exe);
        cmd.arg(format!("--app={}", app_url));
        cmd.arg("--no-first-run");
        cmd.arg("--no-default-browser-check");
        cmd.arg("--disable-background-mode");

        #[cfg(target_os = "windows")]
        if let Ok(local_app_data) = std::env::var("LocalAppData") {
            let profile_dir = PathBuf::from(local_app_data).join("ImageMediaViewer\\BrowserProfile");
            cmd.arg(format!("--user-data-dir={}", profile_dir.to_string_lossy()));
        }

        // ウィンドウ状態（サイズ・位置）の復元
        let state_path = PathBuf::from("window_state.json");
        if state_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&state_path) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                    if json["isMaximized"].as_bool().unwrap_or(false) {
                        cmd.arg("--start-maximized");
                    } else {
                        let w = json["width"].as_u64().unwrap_or(1280).max(400);
                        let h = json["height"].as_u64().unwrap_or(850).max(300);
                        cmd.arg(format!("--window-size={},{}", w, h));
                        if let (Some(x), Some(y)) = (json["x"].as_i64(), json["y"].as_i64()) {
                            cmd.arg(format!("--window-position={},{}", x, y));
                        }
                    }
                }
            }
        }

        match cmd.spawn() {
            Ok(_) => {
                info!("クライアントを独立アプリモードで起動しました: {:?}", browser_exe);
                return;
            }
            Err(e) => {
                error!("独立アプリモードでの起動に失敗、既定ブラウザへフォールバック: {:?}", e);
            }
        }
    }

    // 2. フォールバック: OS既定のブラウザで開く (クロスプラットフォーム)
    if let Err(e) = opener::open(app_url) {
        error!("既定ブラウザでの起動に失敗しました: {:?}", e);
    } else {
        info!("クライアントを既定ブラウザで開きました: {}", app_url);
    }
}

/// 各OSごとのブラウザバイナリ候補一覧を取得
fn get_browser_candidates() -> Vec<PathBuf> {
    let mut list = Vec::new();

    #[cfg(target_os = "windows")]
    {
        let pf = std::env::var("ProgramFiles").map(PathBuf::from).ok();
        let pf86 = std::env::var("ProgramFiles(x86)").map(PathBuf::from).ok();
        let local_app_data = std::env::var("LocalAppData").map(PathBuf::from).ok();

        for base in [pf, pf86, local_app_data].into_iter().flatten() {
            list.push(base.join("Google\\Chrome\\Application\\chrome.exe"));
            list.push(base.join("Microsoft\\Edge\\Application\\msedge.exe"));
        }
    }

    #[cfg(target_os = "macos")]
    {
        list.push(PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"));
        list.push(PathBuf::from("/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"));
        list.push(PathBuf::from("/Applications/Brave Browser.app/Contents/MacOS/Brave Browser"));
    }

    #[cfg(target_os = "linux")]
    {
        for path_str in &[
            "/usr/bin/google-chrome",
            "/usr/bin/google-chrome-stable",
            "/usr/bin/chromium",
            "/usr/bin/chromium-browser",
            "/usr/bin/microsoft-edge",
            "/snap/bin/chromium",
        ] {
            let p = PathBuf::from(path_str);
            if p.exists() {
                list.push(p);
            }
        }
    }

    list
}

/// タスクトレイ（システムトレイ）アイコンおよびメニューを初期化
///
/// @param app Tauri アプリケーションハンドル
/// @param state アプリケーション状態
/// @return 初期化結果
pub fn setup_system_tray(app: &AppHandle, state: Arc<AppState>) -> Result<(), Box<dyn std::error::Error>> {
    let open_client_i = MenuItem::with_id(app, "open_client", "クライアントを開く", true, None::<&str>)?;
    let rescan_i = MenuItem::with_id(app, "rescan", "フォルダを再走査", true, None::<&str>)?;
    let open_folder_i = MenuItem::with_id(app, "open_folder", "データフォルダを開く", true, None::<&str>)?;
    let open_logs_i = MenuItem::with_id(app, "open_logs", "ログフォルダを開く", true, None::<&str>)?;
    let quit_i = MenuItem::with_id(app, "quit", "終了", true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&open_client_i, &rescan_i, &open_folder_i, &open_logs_i, &quit_i])?;

    let state_clone = Arc::clone(&state);

    let mut tray_builder = TrayIconBuilder::with_id("imv_main_tray")
        .tooltip("ImageMediaViewer サーバー (稼働中)")
        .menu(&menu)
        .show_menu_on_left_click(false);

    // アイコン設定: アプリアイコンまたはバンドル内32x32アイコンを確実にデコードして適用
    let tray_icon = app.default_window_icon().cloned().or_else(|| {
        let img = image::load_from_memory(include_bytes!("../icons/32x32.png")).ok()?.to_rgba8();
        let (width, height) = img.dimensions();
        Some(tauri::image::Image::new_owned(img.into_raw(), width, height))
    });

    if let Some(icon) = tray_icon {
        tray_builder = tray_builder.icon(icon);
    }

    let tray = tray_builder
        .on_menu_event(move |app_handle, event| {
            match event.id.as_ref() {
                "open_client" => {
                    launch_client();
                }
                "rescan" => {
                    info!("トレイメニューから再走査を要求されました");
                    if let Ok(conn) = state_clone.db.reader() {
                        if let Ok(folders) = crate::db::repo::get_watched_folders(&conn) {
                            for folder in folders {
                                let s = Arc::clone(&state_clone);
                                let f_id = folder.id;
                                let f_path = folder.path;
                                std::thread::spawn(move || {
                                    let _ = crate::scanner::scan_folder_core(None, s, f_id, f_path, false);
                                });
                            }
                        }
                    }
                }
                "open_folder" => {
                    let cache_dir = &state_clone.cache_dir;
                    let _ = opener::reveal(cache_dir);
                }
                "open_logs" => {
                    if let Some(lm) = crate::logger::get_log_manager() {
                        let _ = opener::reveal(lm.log_dir());
                    }
                }
                "quit" => {
                    info!("トレイメニューから終了が指示されました。アプリケーションを終了します。");
                    app_handle.exit(0);
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|_tray, event| {
            // 左クリックまたはダブルクリックでクライアントを起動
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                launch_client();
            }
        })
        .build(app)?;

    // トレイインスタンスを保持
    let _ = tray;

    info!("システムトレイアイコンを登録しました");
    Ok(())
}
