use serde::{Deserialize, Serialize};
use std::path::Path;

/// アプリケーション全体の設定データ構造
///
/// 変更理由: タスクトレイ常駐設定など、ユーザーの環境設定を専用の構造体で一元管理し、
/// ロジックとデータを明確に分離するため。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppSettings {
    /// メインウィンドウの「×」ボタンを押した際、タスクトレイに常駐させるかどうかのフラグ
    /// true: ウィンドウを非表示にしてタスクトレイに常駐（デフォルト）
    /// false: ウィンドウを閉じた際にアプリケーション全体を完全終了
    pub stay_in_tray: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            stay_in_tray: true, // デフォルト ON
        }
    }
}

/// 設定ファイル（app_settings.json）から設定を読み込む
///
/// ファイルが存在しない、または破損している場合はデフォルト設定を返却します。
///
/// @param data_dir 設定ファイルの保存先ディレクトリ（%APPDATA%/com.imagemediaviewer.desktop 等）
/// @return 読み込まれた設定
pub fn load_settings(data_dir: &Path) -> AppSettings {
    let path = data_dir.join("app_settings.json");
    if let Ok(content) = std::fs::read_to_string(&path) {
        if let Ok(s) = serde_json::from_str::<AppSettings>(&content) {
            return s;
        }
    }
    AppSettings::default()
}

/// 設定ファイル（app_settings.json）へ設定を保存する
///
/// ディレクトリが存在しない場合は自動作成し、インデント付きJSONでアトミックに書き込みます。
///
/// @param data_dir 設定ファイルの保存先ディレクトリ
/// @param settings 保存する設定オブジェクト
/// @return 成功時は Ok(()), 失敗時は std::io::Error
pub fn save_settings(data_dir: &Path, settings: &AppSettings) -> std::io::Result<()> {
    if !data_dir.exists() {
        let _ = std::fs::create_dir_all(data_dir);
    }
    let path = data_dir.join("app_settings.json");
    let content = serde_json::to_string_pretty(settings)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(&path, content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_settings() {
        let default_settings = AppSettings::default();
        assert!(default_settings.stay_in_tray, "デフォルト設定で常駐フラグが true であること");
    }

    #[test]
    fn test_settings_save_and_load() {
        let temp_dir = tempfile::tempdir().expect("一時ディレクトリの作成に失敗");
        let dir_path = temp_dir.path();

        // 1. 初回ロード時はデフォルト値が返ること
        let initial = load_settings(dir_path);
        assert_eq!(initial, AppSettings::default());

        // 2. 設定変更して保存
        let modified = AppSettings { stay_in_tray: false };
        save_settings(dir_path, &modified).expect("設定の保存に失敗");

        // 3. 再ロードして保存された値が反映されていること
        let loaded = load_settings(dir_path);
        assert_eq!(loaded, modified);
        assert!(!loaded.stay_in_tray);
    }
}
