use std::collections::HashMap;
use std::fs::File;
use std::path::Path;
use std::sync::{Arc, RwLock};

use crate::pipeline::exif::ExtractedMetadata;
use super::manifest::{InstalledPluginInfo, PluginActionDefinition, PluginManifest};
use super::runtime::{discover_plugins, get_plugin_search_dirs, DynamicFormatPlugin};

/// 画像フォーマット拡張プラグインインターフェース
///
/// 変更理由: アプリ本体を再ビルドすることなく、動的マニフェスト（plugin.json）に基づき
/// 多様な独自バイナリ形式やコンテナ形式の画像を安全・透過的に取り扱うため。
pub trait ImageFormatPlugin: Send + Sync {
    /// プラグイン識別子 (例: "format.clipstudio")
    fn id(&self) -> &str;

    /// プラグイン名称 (例: "CLIP STUDIO PAINT (.clip) Support")
    fn name(&self) -> &str;

    /// サポートする拡張子一覧 (小文字・ピリオドなし。例: ["clip"])
    fn supported_extensions(&self) -> &[String];

    /// メタデータ抽出（寸法、更新日時等）
    ///
    /// @param path 画像ファイルパス
    /// @param file オープン済みファイルハンドル
    /// @param mtime_secs ファイル更新日時のUNIX秒
    /// @return 抽出されたメタデータ構造体
    fn extract_metadata(
        &self,
        path: &Path,
        file: &mut File,
        mtime_secs: i64,
    ) -> Result<ExtractedMetadata, String>;

    /// サムネイル用バイナリ生成（WebPバイト列）
    ///
    /// @param path 画像ファイルパス
    /// @param orientation Exif Orientation (1-8)
    /// @return 生成されたWebPバイナリデータ
    fn generate_thumbnail(
        &self,
        path: &Path,
        orientation: u32,
    ) -> Result<Vec<u8>, String>;

    /// 原寸プレビュー用バイナリとMIMEタイプ (ブラウザ/WebView表示用)
    ///
    /// @param path 画像ファイルパス
    /// @return (バイナリ列, MIMEタイプ文字列)
    fn extract_original_preview(
        &self,
        path: &Path,
    ) -> Result<(Vec<u8>, String), String>;
}

/// プラグイン再読み込み結果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReloadResult {
    /// 今回のリロードで新規に追加された拡張子のリスト
    pub added_extensions: Vec<String>,
    /// 現在有効な全拡張子のリスト
    pub all_extensions: Vec<String>,
    /// ロードされたプラグイン総数
    pub loaded_count: usize,
}

/// 画像フォーマットプラグイン管理レジストリ内部状態
struct FormatPluginRegistryInner {
    plugins: HashMap<String, Arc<dyn ImageFormatPlugin>>,
    extension_map: HashMap<String, Arc<dyn ImageFormatPlugin>>,
    actions: Vec<PluginActionDefinition>,
    manifests: Vec<PluginManifest>,
}

/// プラグインIDが有効化リスト（完全一致、全一致 "*"、前方一致ワイルドカード "prefix*"、および明示的無効化 "!id"）に合致するか判定する
///
/// 変更理由: デフォルトで "*" や "format.*" による自動有効化を維持しつつ、
/// ユーザーがGUIから特定のプラグインを個別にOFFにした際（"!format.xxx"）に確実に無効化できるようにするため。
pub fn is_plugin_enabled_by_patterns(id: &str, enabled_plugins: &[String]) -> bool {
    let disabled_rule = format!("!{}", id);
    if enabled_plugins.iter().any(|p| p == &disabled_rule) {
        return false;
    }

    enabled_plugins.iter().any(|pattern| {
        if pattern.starts_with('!') {
            return false;
        }
        if pattern == "*" || pattern == id {
            true
        } else if let Some(prefix) = pattern.strip_suffix('*') {
            id.starts_with(prefix)
        } else {
            false
        }
    })
}

/// 画像フォーマット拡張プラグイン管理レジストリ
///
/// 変更理由: 設定ファイル（app_settings.json）で有効化されたプラグインのみを安全に保持し、
/// 実行時の動的リロードおよびスレッドセーフな参照をRwLockにより保証するため。
pub struct FormatPluginRegistry {
    inner: RwLock<FormatPluginRegistryInner>,
}

impl FormatPluginRegistry {
    /// 有効化プラグインIDリストおよび検索ディレクトリ群に基づいてレジストリを初期化する
    ///
    /// @param enabled_plugins 有効化するプラグインIDの配列
    /// @param app_data_dir アプリケーションデータディレクトリ
    /// @return 初期化された FormatPluginRegistry
    pub fn new(enabled_plugins: &[String], app_data_dir: &Path) -> Self {
        let registry = Self::empty();
        registry.reload(enabled_plugins, app_data_dir);
        registry
    }

    /// 空のレジストリを作成（テスト・フォールバック用）
    pub fn empty() -> Self {
        Self {
            inner: RwLock::new(FormatPluginRegistryInner {
                plugins: HashMap::new(),
                extension_map: HashMap::new(),
                actions: Vec::new(),
                manifests: Vec::new(),
            }),
        }
    }

    /// プラグインを手動登録する（テスト用）
    ///
    /// @param plugin 登録するプラグインインスタンス
    pub fn register(&self, plugin: Arc<dyn ImageFormatPlugin>) {
        if let Ok(mut inner) = self.inner.write() {
            let id = plugin.id().to_string();
            for ext in plugin.supported_extensions() {
                inner.extension_map.insert(ext.to_ascii_lowercase(), Arc::clone(&plugin));
            }
            inner.plugins.insert(id, plugin);
        }
    }

    /// 拡張子に対応するプラグインを取得する
    ///
    /// @param ext 拡張子（ピリオド有無・大小文字不問）
    /// @return 対応プラグインが存在すれば Some(Arc<dyn ImageFormatPlugin>)
    pub fn get_by_extension(&self, ext: &str) -> Option<Arc<dyn ImageFormatPlugin>> {
        let clean = ext.trim().trim_start_matches('.').to_ascii_lowercase();
        let inner = self.inner.read().ok()?;
        inner.extension_map.get(&clean).cloned()
    }

    /// プラグインがサポートする追加拡張子一覧を取得する
    ///
    /// @return サポート拡張子リスト（ソート済み・重複排除）
    pub fn supported_extensions(&self) -> Vec<String> {
        if let Ok(inner) = self.inner.read() {
            let mut exts: Vec<String> = inner.extension_map.keys().cloned().collect();
            exts.sort();
            exts.dedup();
            exts
        } else {
            Vec::new()
        }
    }

    /// 指定されたプラグインIDが有効化されているか判定する
    ///
    /// @param id プラグインID
    /// @return 有効であれば true
    pub fn is_plugin_enabled(&self, id: &str) -> bool {
        if let Ok(inner) = self.inner.read() {
            inner.plugins.contains_key(id) || inner.manifests.iter().any(|m| m.id == id)
        } else {
            false
        }
    }

    /// 登録されている全プラグインアクション一覧を取得する
    pub fn get_actions(&self) -> Vec<PluginActionDefinition> {
        if let Ok(inner) = self.inner.read() {
            inner.actions.clone()
        } else {
            Vec::new()
        }
    }

    /// 指定IDのプラグインアクションを検索する
    pub fn find_action(&self, action_id: &str) -> Option<PluginActionDefinition> {
        let inner = self.inner.read().ok()?;
        inner.actions.iter().find(|a| a.id == action_id).cloned()
    }

    /// 登録されている全プラグインマニフェストを取得する
    pub fn get_manifests(&self) -> Vec<PluginManifest> {
        if let Ok(inner) = self.inner.read() {
            inner.manifests.clone()
        } else {
            Vec::new()
        }
    }

    /// 検索ディレクトリから検出された全プラグインの一覧情報（有効・無効両方を含む）を取得する
    ///
    /// 変更理由: 設定モーダルの「プラグイン管理」タブにおいて、現在インストールされている全プラグインの
    /// 状態（有効/無効、対応拡張子、提供アクション、配置パス）を一覧表示・操作可能にするため。
    ///
    /// @param enabled_plugins 現在の有効化プラグインパターン一覧
    /// @param app_data_dir アプリケーションデータディレクトリ
    /// @return ソート済みの InstalledPluginInfo リスト
    pub fn list_installed_plugins(
        &self,
        enabled_plugins: &[String],
        app_data_dir: &Path,
    ) -> Vec<InstalledPluginInfo> {
        let search_dirs = get_plugin_search_dirs(app_data_dir);
        let discovered = discover_plugins(&search_dirs);
        let user_plugins_root = app_data_dir
            .join("plugins")
            .canonicalize()
            .unwrap_or_else(|_| app_data_dir.join("plugins"));

        let mut list: Vec<InstalledPluginInfo> = discovered
            .into_iter()
            .map(|(id, (manifest, plugin_dir))| {
                let enabled = is_plugin_enabled_by_patterns(&id, enabled_plugins);
                let canonical_dir = plugin_dir.canonicalize().unwrap_or_else(|_| plugin_dir.clone());
                let is_user_installed = canonical_dir.starts_with(&user_plugins_root);
                let extensions = manifest
                    .extensions
                    .iter()
                    .map(|ext| ext.trim().trim_start_matches('.').to_ascii_lowercase())
                    .filter(|ext| !ext.is_empty())
                    .collect();
                let action_names = manifest.actions.iter().map(|a| a.name.clone()).collect();

                InstalledPluginInfo {
                    id,
                    name: manifest.name,
                    version: manifest.version,
                    description: manifest.description,
                    extensions,
                    action_names,
                    enabled,
                    path: plugin_dir.to_string_lossy().to_string(),
                    is_user_installed,
                }
            })
            .collect();

        list.sort_by(|a, b| a.id.cmp(&b.id));
        list
    }

    /// プラグインディレクトリを再走査し、enabled_plugins に合致するプラグインを動的リロードする
    ///
    /// 変更理由: ユーザーが新しいプラグインフォルダを追加したり、設定画面でプラグインを有効化した際、
    /// 本体の再起動や再ビルドを不要にし、即座に拡張子監視とアクションを反映させるため。
    /// ワイルドカード（"*" や "format.*"）および個別無効化（"!id"）による指定にも対応。
    ///
    /// @param enabled_plugins 有効化プラグインID一覧
    /// @param app_data_dir アプリケーションデータディレクトリ
    /// @return リロード結果（新規追加された拡張子一覧等）
    pub fn reload(&self, enabled_plugins: &[String], app_data_dir: &Path) -> ReloadResult {
        let prev_extensions = self.supported_extensions();

        let search_dirs = get_plugin_search_dirs(app_data_dir);
        let discovered = discover_plugins(&search_dirs);

        let mut new_plugins: HashMap<String, Arc<dyn ImageFormatPlugin>> = HashMap::new();
        let mut new_extension_map: HashMap<String, Arc<dyn ImageFormatPlugin>> = HashMap::new();
        let mut new_actions: Vec<PluginActionDefinition> = Vec::new();
        let mut new_manifests: Vec<PluginManifest> = Vec::new();

        for (id, (manifest, plugin_dir)) in discovered {
            if is_plugin_enabled_by_patterns(&id, enabled_plugins) {
                // アクションの収集（親プラグインIDを自動付与し、個別アクション無効化設定も考慮）
                for action in &manifest.actions {
                    if !is_plugin_enabled_by_patterns(&action.id, enabled_plugins) {
                        continue;
                    }
                    if !new_actions.iter().any(|a| a.id == action.id) {
                        let mut act = action.clone();
                        if act.plugin_id.is_none() {
                            act.plugin_id = Some(id.clone());
                        }
                        new_actions.push(act);
                    }
                }

                // フォーマット抽出エンジンが存在する場合、DynamicFormatPlugin を生成
                if manifest.extractor.is_some() {
                    let dynamic_plugin = Arc::new(DynamicFormatPlugin::new(manifest.clone(), plugin_dir));
                    for ext in dynamic_plugin.supported_extensions() {
                        let clean_ext = ext.trim().trim_start_matches('.').to_ascii_lowercase();
                        if !clean_ext.is_empty() {
                            new_extension_map.insert(clean_ext, Arc::clone(&dynamic_plugin) as Arc<dyn ImageFormatPlugin>);
                        }
                    }
                    new_plugins.insert(id, dynamic_plugin as Arc<dyn ImageFormatPlugin>);
                }

                new_manifests.push(manifest);
            }
        }

        // アクションのソート（order昇順）
        new_actions.sort_by_key(|a| a.order.unwrap_or(50));

        let loaded_count = new_plugins.len();

        let mut all_extensions: Vec<String> = new_extension_map.keys().cloned().collect();
        all_extensions.sort();
        all_extensions.dedup();

        let added_extensions: Vec<String> = all_extensions
            .iter()
            .filter(|ext| !prev_extensions.contains(ext))
            .cloned()
            .collect();

        if let Ok(mut inner) = self.inner.write() {
            inner.plugins = new_plugins;
            inner.extension_map = new_extension_map;
            inner.actions = new_actions;
            inner.manifests = new_manifests;
        }

        ReloadResult {
            added_extensions,
            all_extensions,
            loaded_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_format_plugin_registry_empty_and_register() {
        let registry = FormatPluginRegistry::empty();
        assert!(registry.supported_extensions().is_empty());
        assert!(!registry.is_plugin_enabled("format.clipstudio"));
        assert!(registry.get_by_extension("clip").is_none());
    }

    #[test]
    fn test_is_plugin_enabled_by_patterns_with_negation() {
        let patterns = vec![
            "*".to_string(),
            "format.*".to_string(),
            "!format.disabled_one".to_string(),
        ];
        assert!(is_plugin_enabled_by_patterns("format.myformat", &patterns));
        assert!(is_plugin_enabled_by_patterns("format.clipstudio", &patterns));
        assert!(!is_plugin_enabled_by_patterns("format.disabled_one", &patterns));
    }

    #[test]
    fn test_format_plugin_registry_dynamic_reload() {
        let temp_dir = tempdir().unwrap();
        let plugins_dir = temp_dir.path().join("plugins");
        let my_plugin_dir = plugins_dir.join("my_format");
        std::fs::create_dir_all(&my_plugin_dir).unwrap();

        let manifest_content = r#"{
            "id": "format.myformat",
            "name": "My Format Support",
            "version": "1.0.0",
            "extensions": ["myfmt", "myf"],
            "extractor": {
                "type": "binary_slice",
                "start_signature": "89504E470D0A1A0A",
                "end_signature": "49454E44AE426082",
                "include_end_signature": true,
                "mime_type": "image/png"
            },
            "actions": [
                {
                    "id": "action.open_my_format",
                    "name": "My Formatで開く",
                    "order": 15,
                    "extensions": ["myfmt"]
                }
            ]
        }"#;

        std::fs::write(my_plugin_dir.join("plugin.json"), manifest_content).unwrap();

        // 1. 初回ロード（enabled_plugins に含めない場合はロードされないこと）
        let registry = FormatPluginRegistry::empty();
        let res1 = registry.reload(&[], temp_dir.path());
        assert_eq!(res1.loaded_count, 0);
        assert!(res1.added_extensions.is_empty());
        assert!(registry.get_by_extension("myfmt").is_none());

        // 2. 有効化してリロード（新規拡張子が検出され、アクションに plugin_id が付与されること）
        let res2 = registry.reload(&["format.myformat".to_string(), "*".to_string()], temp_dir.path());
        assert!(res2.loaded_count >= 1);
        assert!(res2.added_extensions.contains(&"myf".to_string()));
        assert!(res2.added_extensions.contains(&"myfmt".to_string()));
        assert!(registry.is_plugin_enabled("format.myformat"));
        assert!(registry.get_by_extension("myfmt").is_some());
        assert!(registry.get_by_extension("myf").is_some());
        let my_act = registry.find_action("action.open_my_format").expect("アクションが登録されていること");
        assert_eq!(my_act.plugin_id, Some("format.myformat".to_string()));

        // 3. 再度同じ設定でリロードした場合、added_extensions は空になること
        let res3 = registry.reload(&["format.myformat".to_string(), "*".to_string()], temp_dir.path());
        assert!(res3.added_extensions.is_empty());

        // 4. 個別無効化 "!format.myformat" を指定すると無効化されること
        let _res4 = registry.reload(&["*".to_string(), "!format.myformat".to_string()], temp_dir.path());
        assert!(!registry.is_plugin_enabled("format.myformat"));
        assert!(registry.get_by_extension("myfmt").is_none());
        assert!(registry.find_action("action.open_my_format").is_none());

        // 5. list_installed_plugins で全プラグインと有効/無効状態が取得できること
        let installed = registry.list_installed_plugins(&["*".to_string(), "!format.myformat".to_string()], temp_dir.path());
        let my_info = installed.iter().find(|p| p.id == "format.myformat").expect("myformatが一覧に含まれること");
        assert!(!my_info.enabled);
        assert!(my_info.is_user_installed);
        assert_eq!(my_info.extensions, vec!["myfmt".to_string(), "myf".to_string()]);
    }

    #[test]
    fn test_plugin_directory_precedence() {
        let temp_dir1 = tempdir().unwrap();
        let temp_dir2 = tempdir().unwrap();

        // ディレクトリ1（ユーザー設定側）にカスタム版を配置
        let dir1_plugin = temp_dir1.path().join("my_format");
        std::fs::create_dir_all(&dir1_plugin).unwrap();
        std::fs::write(
            dir1_plugin.join("plugin.json"),
            r#"{"id":"format.override","name":"User Custom Override","extensions":["cust1"]}"#,
        ).unwrap();

        // ディレクトリ2（本体同梱側）に同IDのデフォルト版を配置
        let dir2_plugin = temp_dir2.path().join("my_format");
        std::fs::create_dir_all(&dir2_plugin).unwrap();
        std::fs::write(
            dir2_plugin.join("plugin.json"),
            r#"{"id":"format.override","name":"Bundled Default","extensions":["cust2"]}"#,
        ).unwrap();

        // ディレクトリ1（優先） -> ディレクトリ2の順で探索
        let search_dirs = vec![temp_dir1.path().to_path_buf(), temp_dir2.path().to_path_buf()];
        let discovered = discover_plugins(&search_dirs);

        assert_eq!(discovered.len(), 1);
        let (manifest, _) = discovered.get("format.override").unwrap();
        assert_eq!(manifest.name, "User Custom Override", "上位ディレクトリの設定が優先されること");
        assert_eq!(manifest.extensions, vec!["cust1".to_string()]);
    }

    #[test]
    fn test_extension_sanitization() {
        let temp_dir = tempdir().unwrap();
        let plugins_dir = temp_dir.path().join("plugins");
        let plugin_dir = plugins_dir.join("dot_format");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::write(
            plugin_dir.join("plugin.json"),
            r#"{
                "id": "format.dotformat",
                "name": "Dot Format",
                "extensions": [".dotfmt", "  spacefmt  ", "CLEANFMT"],
                "extractor": {
                    "type": "binary_slice",
                    "start_signature": "00",
                    "end_signature": "FF"
                }
            }"#,
        ).unwrap();

        let registry = FormatPluginRegistry::new(&["*".to_string()], temp_dir.path());
        let exts = registry.supported_extensions();
        assert!(exts.contains(&"dotfmt".to_string()), "先頭ドットが除去されて小文字で登録されること");
        assert!(exts.contains(&"spacefmt".to_string()), "前後の空白が除去されて登録されること");
        assert!(exts.contains(&"cleanfmt".to_string()), "大文字が小文字化されて登録されること");
        assert!(registry.get_by_extension("dotfmt").is_some());
        assert!(registry.get_by_extension(".dotfmt").is_some());
        assert!(registry.get_by_extension("DOTFMT").is_some());
    }
}
