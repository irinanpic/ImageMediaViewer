use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// プラグイン設定マニフェスト (plugin.json)
///
/// 変更理由: アプリ本体の再コンパイルを一切行うことなく、
/// ユーザーが独自画像フォーマットや外部連携アクションを完全動的に追加・実行可能にするため。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    /// プラグイン一意識別子 (例: "format.clipstudio")
    pub id: String,
    /// プラグイン名称 (例: "CLIP STUDIO PAINT (.clip) Support")
    pub name: String,
    /// プラグインバージョン (例: "1.0.0")
    #[serde(default)]
    pub version: Option<String>,
    /// プラグイン説明文
    #[serde(default)]
    pub description: Option<String>,
    /// サポートする拡張子一覧（小文字、ドットなし。例: ["clip"]）
    #[serde(default)]
    pub extensions: Vec<String>,
    /// ファイル先頭のマジックヘッダ（ASCII文字列またはHEX文字列）
    #[serde(default)]
    pub magic_header: Option<String>,
    /// 画像抽出エンジン設定（フォーマットプラグイン用）
    #[serde(default)]
    pub extractor: Option<ExtractorConfig>,
    /// プラグインが提供するコンテキストメニューアクション一覧
    #[serde(default)]
    pub actions: Vec<PluginActionDefinition>,
}

/// 画像データ抽出エンジンの設定
///
/// 変更理由: 特定ソフトウェア固有のロジック（SQLite BLOB走査等）を本体から完全に排除し、
/// 独立ビルドされたプラグインバイナリ（C-ABI動的ライブラリ/独立実行バイナリ）、バイナリスライス、外部コマンドなど
/// 汎用的な抽出エンジンのみを提供する構成にするため。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExtractorConfig {
    /// 独立プラグインバイナリ（C-ABI動的ライブラリまたは独立実行バイナリ）による画像抽出
    Binary {
        /// プラグイン動的ライブラリ（.dll, .so, .dylib）の指定
        #[serde(default)]
        library: Option<PlatformBinaryConfig>,
        /// プラグイン独立実行バイナリ（CLI）の指定
        #[serde(default)]
        executable: Option<PlatformBinaryConfig>,
    },
    /// 開始シグネチャと終端シグネチャによるバイナリ切り出し
    BinarySlice {
        /// 開始シグネチャ（ASCII文字列または16進文字列）
        start_signature: String,
        /// 終端シグネチャ（ASCII文字列または16進文字列）
        end_signature: String,
        /// 終端シグネチャを含めるか否か（既定値: true）
        #[serde(default = "default_true")]
        include_end_signature: bool,
        /// 抽出される画像のMIMEタイプ（既定値: "image/png"）
        #[serde(default = "default_mime_png")]
        mime_type: String,
    },
    /// 外部コマンド/スクリプト実行による画像抽出
    Command {
        /// 実行コマンドパス（プラグインディレクトリ相対または環境PATH）
        command: String,
        /// コマンド引数テンプレート（"{input}", "{output}"）
        #[serde(default)]
        args: Vec<String>,
        /// 出力画像フォーマット（既定値: "png"）
        #[serde(default = "default_output_format")]
        output_format: String,
    },
}

/// プラットフォーム別のバイナリファイル名/パス設定
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum PlatformBinaryConfig {
    /// 単一パス指定
    Single(String),
    /// OS別パス指定
    PerPlatform {
        #[serde(default)]
        windows: Option<String>,
        #[serde(default)]
        linux: Option<String>,
        #[serde(default)]
        macos: Option<String>,
        #[serde(default)]
        android: Option<String>,
        #[serde(default)]
        default: Option<String>,
    },
}

impl PlatformBinaryConfig {
    /// 現在のOSに応じたバイナリパスを解決する
    pub fn resolve_for_current_os(&self) -> Option<&str> {
        match self {
            Self::Single(path) => Some(path.as_str()),
            Self::PerPlatform {
                windows,
                linux,
                macos,
                android,
                default,
            } => {
                let _ = (windows, linux, macos, android, default);
                #[cfg(target_os = "windows")]
                {
                    windows.as_deref().or(default.as_deref())
                }
                #[cfg(target_os = "android")]
                {
                    android
                        .as_deref()
                        .or(linux.as_deref())
                        .or(default.as_deref())
                }
                #[cfg(all(target_os = "linux", not(target_os = "android")))]
                {
                    linux.as_deref().or(default.as_deref())
                }
                #[cfg(target_os = "macos")]
                {
                    macos.as_deref().or(default.as_deref())
                }
                #[cfg(not(any(
                    target_os = "windows",
                    target_os = "linux",
                    target_os = "macos",
                    target_os = "android"
                )))]
                {
                    default.as_deref()
                }
            }
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_mime_png() -> String {
    "image/png".to_string()
}

fn default_output_format() -> String {
    "png".to_string()
}

/// プラグインが提供するコンテキストメニューアクション定義
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub struct PluginActionDefinition {
    /// アクション一意識別子 (例: "action.open_clip_studio")
    pub id: String,
    /// メニュー表示名（日本語）
    pub name: String,
    /// メニュー表示名（英語）
    #[serde(default)]
    #[serde(alias = "name_en")]
    pub name_en: Option<String>,
    /// アイコン名 (Lucide アイコン名)
    #[serde(default)]
    pub icon: Option<String>,
    /// メニュー表示順序
    #[serde(default)]
    pub order: Option<i32>,
    /// 有効化する拡張子リスト（空の場合はすべて）
    #[serde(default)]
    pub extensions: Vec<String>,
    /// コマンド実行定義
    #[serde(default)]
    pub command: Option<PluginActionCommand>,
}

/// プラグインアクションのプロセス起動定義
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub struct PluginActionCommand {
    /// Windows向け実行パス候補（先頭から存在確認して実行）
    #[serde(default)]
    pub windows: Vec<String>,
    /// デフォルト/POSIX向けコマンド名またはパス
    #[serde(default)]
    pub default: Option<String>,
    /// コマンド引数テンプレート（"{path}" が対象パスに置換）
    #[serde(default)]
    pub args: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_bindings_plugins() {
        PluginActionDefinition::export().expect("Failed to export PluginActionDefinition bindings");
        PluginActionCommand::export().expect("Failed to export PluginActionCommand bindings");
    }

    #[test]
    fn test_platform_binary_config_android_deserialize() {
        let json_str = r#"{
            "windows": "imv_clipstudio.dll",
            "linux": "libimv_clipstudio.so",
            "macos": "libimv_clipstudio.dylib",
            "android": "libimv_clipstudio.so"
        }"#;

        let config: PlatformBinaryConfig = serde_json::from_str(json_str).unwrap();
        match config {
            PlatformBinaryConfig::PerPlatform { windows, linux, macos, android, default } => {
                assert_eq!(windows, Some("imv_clipstudio.dll".to_string()));
                assert_eq!(linux, Some("libimv_clipstudio.so".to_string()));
                assert_eq!(macos, Some("libimv_clipstudio.dylib".to_string()));
                assert_eq!(android, Some("libimv_clipstudio.so".to_string()));
                assert_eq!(default, None);
            }
            _ => panic!("Expected PerPlatform variant"),
        }
    }

    #[test]
    fn test_platform_binary_config_resolve() {
        let config = PlatformBinaryConfig::Single("my_tool".to_string());
        assert_eq!(config.resolve_for_current_os(), Some("my_tool"));

        let per_plat = PlatformBinaryConfig::PerPlatform {
            windows: Some("win.dll".to_string()),
            linux: Some("linux.so".to_string()),
            macos: Some("mac.dylib".to_string()),
            android: Some("android.so".to_string()),
            default: Some("default.bin".to_string()),
        };

        #[cfg(target_os = "windows")]
        assert_eq!(per_plat.resolve_for_current_os(), Some("win.dll"));
        #[cfg(target_os = "linux")]
        assert_eq!(per_plat.resolve_for_current_os(), Some("linux.so"));
        #[cfg(target_os = "macos")]
        assert_eq!(per_plat.resolve_for_current_os(), Some("mac.dylib"));
        #[cfg(target_os = "android")]
        assert_eq!(per_plat.resolve_for_current_os(), Some("android.so"));
    }
}
