use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};
use tracing::info;

use crate::locales::get_tray_messages;
use crate::state::AppState;

/// タスクトレイメニュー項目のハンドル保持用構造体
///
/// 変更理由: アプリケーション実行中に言語設定が変更された際、
/// 各メニュー項目のラベルテキストおよびツールチップを即時に更新できるようにするため。
#[derive(Clone)]
pub struct TrayMenuHandles {
    pub open_client: MenuItem<tauri::Wry>,
    pub stay_in_tray: CheckMenuItem<tauri::Wry>,
    pub rescan: MenuItem<tauri::Wry>,
    pub open_folder: MenuItem<tauri::Wry>,
    pub open_logs: MenuItem<tauri::Wry>,
    pub quit: MenuItem<tauri::Wry>,
}

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

/// タスクトレイメニューの表示言語を動的に更新する
///
/// 変更理由: フロントエンドでユーザーが言語を切り替えた際、
/// アプリ再起動を必要とせず即座にタスクトレイメニューの言語も同期更新するため。
///
/// @param app Tauri アプリケーションハンドル
/// @param locale 新しい言語コード ("ja" | "en")
pub fn update_tray_locale(app: &AppHandle, locale: &str) {
    let msgs = get_tray_messages(locale);
    if let Some(handles) = app.try_state::<TrayMenuHandles>() {
        let _ = handles.open_client.set_text(msgs.open_client);
        let _ = handles.stay_in_tray.set_text(msgs.stay_in_tray);
        let _ = handles.rescan.set_text(msgs.rescan);
        let _ = handles.open_folder.set_text(msgs.open_data_folder);
        let _ = handles.open_logs.set_text(msgs.open_log_folder);
        let _ = handles.quit.set_text(msgs.quit);
    }
    if let Some(tray) = app.tray_by_id("imv_main_tray") {
        let _ = tray.set_tooltip(Some(msgs.tooltip));
    }
}

/// タスクトレイ（システムトレイ）アイコンおよびメニューを初期化
///
/// 変更理由: メインウィンドウ終了時の常駐切替（「常駐する」メニュー）および
/// 設定の自動永続化、ウィンドウ直接表示、多言語表示に対応するため。
///
/// @param app Tauri アプリケーションハンドル
/// @param state アプリケーション状態
/// @param stay_in_tray 常駐フラグの共有アトミック参照
/// @param app_data_dir 設定保存先ディレクトリ
/// @param initial_locale 初期表示言語 ("ja" | "en")
/// @return 初期化結果
pub fn setup_system_tray(
    app: &AppHandle,
    state: Arc<AppState>,
    stay_in_tray: Arc<AtomicBool>,
    app_data_dir: PathBuf,
    initial_locale: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let msgs = get_tray_messages(initial_locale);
    let open_client_i = MenuItem::with_id(app, "open_client", msgs.open_client, true, None::<&str>)?;
    let initial_stay = stay_in_tray.load(Ordering::Relaxed);
    let stay_in_tray_i = CheckMenuItem::with_id(app, "stay_in_tray", msgs.stay_in_tray, true, initial_stay, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let rescan_i = MenuItem::with_id(app, "rescan", msgs.rescan, true, None::<&str>)?;
    let open_folder_i = MenuItem::with_id(app, "open_folder", msgs.open_data_folder, true, None::<&str>)?;
    let open_logs_i = MenuItem::with_id(app, "open_logs", msgs.open_log_folder, true, None::<&str>)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let quit_i = MenuItem::with_id(app, "quit", msgs.quit, true, None::<&str>)?;

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

    // メニューハンドルを管理状態に登録
    let handles = TrayMenuHandles {
        open_client: open_client_i.clone(),
        stay_in_tray: stay_in_tray_i.clone(),
        rescan: rescan_i.clone(),
        open_folder: open_folder_i.clone(),
        open_logs: open_logs_i.clone(),
        quit: quit_i.clone(),
    };
    app.manage(handles);

    let state_clone = Arc::clone(&state);
    let stay_clone = Arc::clone(&stay_in_tray);
    let stay_menu_item = stay_in_tray_i.clone();
    let data_dir_clone = app_data_dir.clone();

    let mut tray_builder = TrayIconBuilder::with_id("imv_main_tray")
        .tooltip(msgs.tooltip)
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
                    let mut settings = crate::settings::load_settings(&data_dir_clone);
                    settings.stay_in_tray = new_val;
                    if let Err(e) = crate::settings::save_settings(&data_dir_clone, &settings) {
                        tracing::warn!("Failed to save resident setting: {:?}", e);
                    } else {
                        info!("Resident setting changed: stay_in_tray = {}", new_val);
                    }
                }
                "rescan" => {
                    info!("Rescan requested from tray menu");
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
                    info!("Exit requested from tray menu. Terminating application.");
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

    info!("System tray icon registered (Resident default: {})", initial_stay);
    Ok(())
}
