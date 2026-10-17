// =============================================================================
// ImageMediaViewer - Android Native Media Plugin Bridge
// =============================================================================

use tauri::{Runtime, plugin::TauriPlugin};

#[cfg(target_os = "android")]
use tauri::Manager;
#[cfg(target_os = "android")]
use tauri::plugin::PluginHandle;

#[cfg(target_os = "android")]
pub struct AndroidMediaHandle<R: Runtime>(pub PluginHandle<R>);

/// Android ネイティブ連携プラグインの初期化
///
/// 変更理由: Android 7+ における FileUriExposedException の回避、
/// FileProvider による安全な content:// URI 生成、OS アプリ選択シート（Intent Chooser）の表示、
/// DocumentsContract を用いた Files アプリ内フォルダのダイレクト表示、
/// および Scoped Storage 環境下での確実なファイル削除を Android ネイティブ（Kotlin）層と連携して実現するため。
#[cfg(target_os = "android")]
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    tauri::plugin::Builder::new("android_media")
        .setup(|app, api| {
            let handle = api.register_android_plugin("com.imagemediaviewer.desktop", "AndroidMediaPlugin")?;
            app.manage(AndroidMediaHandle(handle));
            Ok(())
        })
        .build()
}

#[cfg(not(target_os = "android"))]
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    tauri::plugin::Builder::new("android_media").build()
}

/// Android 環境でファイルを既定または指定アプリ（CLIP STUDIO PAINT 等）で開く
#[cfg(target_os = "android")]
pub fn open_file(
    app: &tauri::AppHandle,
    path: &str,
    mime_type: Option<&str>,
    package_name: Option<&str>,
) -> Result<(), crate::error::AppError> {
    let handle = app
        .try_state::<AndroidMediaHandle<tauri::Wry>>()
        .ok_or_else(|| crate::error::AppError::internal("AndroidMediaPlugin が初期化されていません"))?;

    handle
        .0
        .run_mobile_plugin::<serde_json::Value>(
            "openFile",
            serde_json::json!({
                "path": path,
                "mimeType": mime_type,
                "packageName": package_name,
            }),
        )
        .map_err(|e| crate::error::AppError::internal(format!("外部アプリでのファイルオープンに失敗: {}", e)))?;

    Ok(())
}

/// Android 環境でファイルが属する親フォルダをシステムファイルマネージャで開く
#[cfg(target_os = "android")]
pub fn open_folder(
    app: &tauri::AppHandle,
    path: &str,
) -> Result<(), crate::error::AppError> {
    let handle = app
        .try_state::<AndroidMediaHandle<tauri::Wry>>()
        .ok_or_else(|| crate::error::AppError::internal("AndroidMediaPlugin が初期化されていません"))?;

    handle
        .0
        .run_mobile_plugin::<serde_json::Value>(
            "openFolder",
            serde_json::json!({
                "path": path,
            }),
        )
        .map_err(|e| crate::error::AppError::internal(format!("フォルダの表示に失敗: {}", e)))?;

    Ok(())
}

/// Android 環境でファイルを安全に削除する（Scoped Storage / MediaStore フォールバック対応）
#[cfg(target_os = "android")]
pub fn trash_file(
    app: &tauri::AppHandle,
    path: &str,
) -> Result<(), crate::error::AppError> {
    let handle = app
        .try_state::<AndroidMediaHandle<tauri::Wry>>()
        .ok_or_else(|| crate::error::AppError::internal("AndroidMediaPlugin が初期化されていません"))?;

    handle
        .0
        .run_mobile_plugin::<serde_json::Value>(
            "trashFile",
            serde_json::json!({
                "path": path,
            }),
        )
        .map_err(|e| crate::error::AppError::internal(format!("ファイルの削除に失敗: {}", e)))?;

    Ok(())
}
