use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};
use tracing::info;

use crate::state::AppState;

/// メインウィンドウを表示し最前面にフォーカスする
///
/// 変更理由: ポートレス一体化により外部ブラウザを起動する必要がなくなったため、
/// 自身がホストしている Tauri ネイティブウィンドウ（main）を直接表示・アクティブ化する。
///
/// @param app Tauri アプリケーションハンドル
pub fn show_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// タスクトレイ（システムトレイ）アイコンおよびメニューを初期化
///
/// 変更理由: メインウィンドウ終了時の常駐切替（「常駐する」メニュー）および
/// 設定の自動永続化、ウィンドウ直接表示に対応するため。
///
/// @param app Tauri アプリケーションハンドル
/// @param state アプリケーション状態
/// @param stay_in_tray 常駐フラグの共有アトミック参照
/// @param app_data_dir 設定保存先ディレクトリ
/// @return 初期化結果
pub fn setup_system_tray(
    app: &AppHandle,
    state: Arc<AppState>,
    stay_in_tray: Arc<AtomicBool>,
    app_data_dir: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let open_client_i = MenuItem::with_id(app, "open_client", "クライアントを開く", true, None::<&str>)?;
    let initial_stay = stay_in_tray.load(Ordering::Relaxed);
    let stay_in_tray_i = CheckMenuItem::with_id(app, "stay_in_tray", "常駐する", true, initial_stay, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let rescan_i = MenuItem::with_id(app, "rescan", "フォルダを再走査", true, None::<&str>)?;
    let open_folder_i = MenuItem::with_id(app, "open_folder", "データフォルダを開く", true, None::<&str>)?;
    let open_logs_i = MenuItem::with_id(app, "open_logs", "ログフォルダを開く", true, None::<&str>)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let quit_i = MenuItem::with_id(app, "quit", "終了", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &open_client_i,
            &stay_in_tray_i,
            &sep1,
            &rescan_i,
            &open_folder_i,
            &open_logs_i,
            &sep2,
            &quit_i,
        ],
    )?;

    let state_clone = Arc::clone(&state);
    let stay_clone = Arc::clone(&stay_in_tray);
    let stay_menu_item = stay_in_tray_i.clone();
    let data_dir_clone = app_data_dir.clone();

    let mut tray_builder = TrayIconBuilder::with_id("imv_main_tray")
        .tooltip("ImageMediaViewer (稼働中)")
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
                    show_main_window(app_handle);
                }
                "stay_in_tray" => {
                    let cur = stay_clone.load(Ordering::Relaxed);
                    let new_val = !cur;
                    stay_clone.store(new_val, Ordering::Relaxed);
                    let _ = stay_menu_item.set_checked(new_val);
                    let settings = crate::settings::AppSettings {
                        stay_in_tray: new_val,
                    };
                    if let Err(e) = crate::settings::save_settings(&data_dir_clone, &settings) {
                        tracing::warn!("常駐設定の保存に失敗しました: {:?}", e);
                    } else {
                        info!("常駐設定を変更しました: stay_in_tray = {}", new_val);
                    }
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
                    info!("トレイメニューから終了が指示されました。アプリケーションを完全終了します。");
                    app_handle.exit(0);
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            // 左クリックでメインウィンドウを表示
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;

    let _ = tray;

    info!("システムトレイアイコンを登録しました（常駐初期状態: {}）", initial_stay);
    Ok(())
}
