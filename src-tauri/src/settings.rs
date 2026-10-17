use serde::{Deserialize, Serialize};
use std::path::Path;
use ts_rs::TS;

/// ユーザー定義のカスタムコンテキストメニュー項目
///
/// 変更理由: 任意の外部アプリケーション連携を設定ファイルから登録可能にするため。
/// モバイル環境（Android）との互換性を確保するため、デフォルトでは環境依存の項目を配置せず空配列とする。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub struct CustomContextMenuItem {
    /// 項目の一意識別子
    pub id: String,
    /// メニューの表示名
    pub name: String,
    /// 起動対象アプリケーションのパスまたはコマンド名
    pub app_path: String,
    /// コマンドライン引数テンプレート（省略時は "{path}"）
    #[serde(default)]
    pub args: Option<String>,
    /// 対象ファイル拡張子リスト（空の場合はすべての拡張子に適用）
    #[serde(default)]
    pub target_extensions: Vec<String>,
    /// メニュー有効化フラグ
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// アプリケーション全体の設定データ構造
///
/// 変更理由: タスクトレイ常駐設定、言語、有効化プラグイン一覧、カスタムメニュー項目を専用の構造体で一元管理し、
/// ロジックとデータを明確に分離するため。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../src/types/generated/")]
pub struct AppSettings {
    /// メインウィンドウの「×」ボタンを押した際、タスクトレイに常駐させるかどうかのフラグ
    /// true: ウィンドウを非表示にしてタスクトレイに常駐（デフォルト）
    /// false: ウィンドウを閉じた際にアプリケーション全体を終了
    pub stay_in_tray: bool,
    /// アプリケーション全体の言語設定 ("ja" | "en")
    pub locale: String,
    /// 明示的に有効化されたプラグインID一覧
    #[serde(default = "default_enabled_plugins")]
    pub enabled_plugins: Vec<String>,
    /// ユーザー定義のカスタムコンテキストメニュー項目（環境依存の詳細を含めないためデフォルトは空配列）
    #[serde(default)]
    pub custom_context_menu_items: Vec<CustomContextMenuItem>,
}

/// デフォルトで有効化される安全なプラグイン一覧
pub fn default_enabled_plugins() -> Vec<String> {
    vec![
        "format.clipstudio".to_string(),
        "format.*".to_string(),
        "action.open_default".to_string(),
        "action.open_clip_studio".to_string(),
        "action.show_in_folder".to_string(),
        "action.copy_path".to_string(),
        "action.trash_file".to_string(),
    ]
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            stay_in_tray: true, // デフォルト ON
            locale: "ja".to_string(), // デフォルト 日本語
            enabled_plugins: default_enabled_plugins(),
            custom_context_menu_items: Vec::new(), // 環境非依存の空配列
        }
    }
}

/// 設定ファイル（app_settings.json）から設定を読み込む
///
/// 変更理由: 過去のバージョンで保存された既存の app_settings.json において、
/// enabled_plugins が空配列だったり新設プラグイン（format.clipstudio 等）が含まれていない場合に
/// 自動的に推奨プラグインをマージし、全プラグインが無効化される不具合を防止するため。
///
/// @param data_dir 設定ファイルの保存先ディレクトリ（%APPDATA%/com.imagemediaviewer.desktop 等）
/// @return 読み込まれ、必要に応じてマイグレーションされた設定
pub fn load_settings(data_dir: &Path) -> AppSettings {
    let path = data_dir.join("app_settings.json");
    if let Ok(content) = std::fs::read_to_string(&path) {
        if let Ok(mut s) = serde_json::from_str::<AppSettings>(&content) {
            // 互換性マイグレーション: enabled_plugins が空配列、または古い設定でフォーマットプラグイン指定が欠落している場合、
            // 推奨デフォルトプラグインを自動的に補完・マージする
            if s.enabled_plugins.is_empty() {
                s.enabled_plugins = default_enabled_plugins();
            } else {
                let has_format_pattern = s
                    .enabled_plugins
                    .iter()
                    .any(|p| p == "*" || p == "format.*" || p.starts_with("format."));
                if !has_format_pattern {
                    s.enabled_plugins.push("format.clipstudio".to_string());
                    s.enabled_plugins.push("format.*".to_string());
                }
            }
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
        assert_eq!(default_settings.locale, "ja", "デフォルト設定で言語が 'ja' であること");
        assert!(
            default_settings.custom_context_menu_items.is_empty(),
            "環境依存を避けるため、デフォルトのカスタムメニュー項目は空配列であること"
        );
        assert!(
            default_settings.enabled_plugins.contains(&"format.clipstudio".to_string()),
            "デフォルトで format.clipstudio が有効化リストに含まれていること"
        );
    }

    #[test]
    fn test_settings_save_and_load() {
        let temp_dir = tempfile::tempdir().expect("一時ディレクトリの作成に失敗");
        let dir_path = temp_dir.path();

        // 1. 初回ロード時はデフォルト値が返ること
        let initial = load_settings(dir_path);
        assert_eq!(initial, AppSettings::default());

        // 2. 設定変更して保存
        let modified = AppSettings {
            stay_in_tray: false,
            locale: "en".to_string(),
            enabled_plugins: vec!["format.clipstudio".to_string()],
            custom_context_menu_items: vec![CustomContextMenuItem {
                id: "test-item".to_string(),
                name: "Test Action".to_string(),
                app_path: "echo".to_string(),
                args: Some("{path}".to_string()),
                target_extensions: vec!["png".to_string()],
                enabled: true,
            }],
        };
        save_settings(dir_path, &modified).expect("設定の保存に失敗");

        // 3. 再ロードして保存された値が反映されていること
        let loaded = load_settings(dir_path);
        assert_eq!(loaded, modified);
        assert!(!loaded.stay_in_tray);
        assert_eq!(loaded.locale, "en");
        assert_eq!(loaded.custom_context_menu_items.len(), 1);
        assert_eq!(loaded.custom_context_menu_items[0].name, "Test Action");
    }

    #[test]
    fn test_settings_migration_from_legacy_configs() {
        let temp_dir = tempfile::tempdir().expect("一時ディレクトリの作成に失敗");
        let dir_path = temp_dir.path();
        let settings_path = dir_path.join("app_settings.json");

        // 1. enabled_plugins が空配列の古い設定ファイル
        std::fs::write(&settings_path, r#"{"stay_in_tray": true, "locale": "ja", "enabled_plugins": []}"#).unwrap();
        let loaded = load_settings(dir_path);
        assert!(loaded.enabled_plugins.contains(&"format.clipstudio".to_string()));
        assert!(loaded.enabled_plugins.contains(&"format.*".to_string()));

        // 2. enabled_plugins が古いアクションのみでフォーマット指定を含まない設定ファイル
        std::fs::write(&settings_path, r#"{"stay_in_tray": true, "locale": "ja", "enabled_plugins": ["action.open_default"]}"#).unwrap();
        let loaded2 = load_settings(dir_path);
        assert!(loaded2.enabled_plugins.contains(&"action.open_default".to_string()));
        assert!(loaded2.enabled_plugins.contains(&"format.clipstudio".to_string()));
        assert!(loaded2.enabled_plugins.contains(&"format.*".to_string()));

        // 3. enabled_plugins に既に format.clipstudio が含まれる場合は重複しない
        std::fs::write(&settings_path, r#"{"stay_in_tray": true, "locale": "ja", "enabled_plugins": ["format.clipstudio", "action.open_clip_studio"]}"#).unwrap();
        let loaded3 = load_settings(dir_path);
        assert_eq!(loaded3.enabled_plugins.iter().filter(|p| *p == "format.clipstudio").count(), 1);
        assert_eq!(loaded3.enabled_plugins.iter().filter(|p| *p == "action.open_clip_studio").count(), 1);
    }

    #[test]
    fn export_bindings_settings() {
        CustomContextMenuItem::export().expect("Failed to export CustomContextMenuItem bindings");
        AppSettings::export().expect("Failed to export AppSettings bindings");
    }
}
