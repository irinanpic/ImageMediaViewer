use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use crate::pipeline::exif::ExtractedMetadata;
use super::extractor::{
    extract_binary_slice, extract_dimensions_from_png, extract_via_command,
    parse_signature_bytes, BinaryPluginHost,
};
use super::format::ImageFormatPlugin;
use super::manifest::{ExtractorConfig, PluginManifest};

/// メタデータ抽出直後のサムネイル生成で同一プラグインファイルの再抽出を回避する短命リングキャッシュ上限
const PLUGIN_PREVIEW_CACHE_CAP: usize = 8;

struct PluginPreviewCacheEntry {
    path: PathBuf,
    file_size: u64,
    mtime_secs: i64,
    bytes: Vec<u8>,
    mime: String,
}

/// マニフェスト（plugin.json）駆動の動的画像フォーマットプラグイン
///
/// 変更理由: アプリ本体の再コンパイルを行うことなく、
/// 独立ビルドされたプラグインバイナリ（C-ABI動的ライブラリ/独立実行バイナリ）や
/// 宣言的バイナリ切り出し設定によって、独自フォーマットを動的に追加・実行可能にするため。
pub struct DynamicFormatPlugin {
    pub manifest: PluginManifest,
    pub plugin_dir: PathBuf,
    binary_host: Option<BinaryPluginHost>,
    preview_cache: Mutex<VecDeque<PluginPreviewCacheEntry>>,
}

impl DynamicFormatPlugin {
    /// 新規インスタンスを生成
    pub fn new(mut manifest: PluginManifest, plugin_dir: PathBuf) -> Self {
        // 拡張子の正規化（前後の空白除去、先頭ドット除去、小文字統一）
        manifest.extensions = manifest
            .extensions
            .into_iter()
            .map(|ext| ext.trim().trim_start_matches('.').to_ascii_lowercase())
            .filter(|ext| !ext.is_empty())
            .collect();

        let binary_host = if let Some(ExtractorConfig::Binary { library, executable }) = &manifest.extractor {
            Some(BinaryPluginHost::new(
                plugin_dir.clone(),
                library.as_ref(),
                executable.as_ref(),
            ))
        } else {
            None
        };

        Self {
            manifest,
            plugin_dir,
            binary_host,
            preview_cache: Mutex::new(VecDeque::with_capacity(PLUGIN_PREVIEW_CACHE_CAP)),
        }
    }

    /// ファイル先頭のマジックヘッダを検証する
    fn validate_magic_header(&self, file: &mut File) -> Result<(), String> {
        if let Some(magic_str) = &self.manifest.magic_header {
            let magic = parse_signature_bytes(magic_str);
            if !magic.is_empty() {
                let _ = file.seek(SeekFrom::Start(0));
                let mut buf = vec![0u8; magic.len()];
                file.read_exact(&mut buf)
                    .map_err(|e| format!("ヘッダ読み込み失敗: {}", e))?;
                if buf != magic {
                    return Err(format!(
                        "無効なファイル形式です（マジックヘッダ不一致: 定義={:?}, 実測={:?}）",
                        magic, buf
                    ));
                }
            }
        }
        Ok(())
    }

    /// マニフェストの設定に基づいて画像バイナリとMIMEタイプを抽出する（短命キャッシュ付き）
    fn extract_image_bytes(
        &self,
        path: &Path,
        file_opt: Option<&mut File>,
    ) -> Result<(Vec<u8>, String), String> {
        let (file_size, mtime_secs) = match std::fs::metadata(path) {
            Ok(meta) => {
                let sz = meta.len();
                let mt = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                (sz, mt)
            }
            Err(_) => (0, 0),
        };

        if file_size > 0 {
            if let Ok(cache) = self.preview_cache.lock() {
                if let Some(hit) = cache.iter().find(|e| {
                    e.file_size == file_size && e.mtime_secs == mtime_secs && e.path == path
                }) {
                    return Ok((hit.bytes.clone(), hit.mime.clone()));
                }
            }
        }

        let extractor = self
            .manifest
            .extractor
            .as_ref()
            .ok_or_else(|| "マニフェストにextractorが定義されていません".to_string())?;

        let result = match extractor {
            ExtractorConfig::Binary { .. } => {
                if let Some(ref host) = self.binary_host {
                    host.extract_preview(path)
                } else {
                    Err("バイナリプラグインホストが初期化されていません".to_string())
                }
            }
            ExtractorConfig::BinarySlice {
                start_signature,
                end_signature,
                include_end_signature,
                mime_type,
            } => {
                let mut opened_file;
                let file = match file_opt {
                    Some(f) => f,
                    None => {
                        opened_file = File::open(path)
                            .map_err(|e| format!("ファイルオープン失敗 ({}): {}", path.display(), e))?;
                        &mut opened_file
                    }
                };

                self.validate_magic_header(file)?;

                let bytes = extract_binary_slice(
                    file,
                    start_signature,
                    end_signature,
                    *include_end_signature,
                )?;

                Ok((bytes, mime_type.clone()))
            }
            ExtractorConfig::Command {
                command,
                args,
                output_format,
            } => {
                if let Some(file) = file_opt {
                    self.validate_magic_header(file)?;
                } else if let Ok(mut f) = File::open(path) {
                    self.validate_magic_header(&mut f)?;
                }

                let bytes = extract_via_command(
                    path,
                    &self.plugin_dir,
                    command,
                    args,
                    output_format,
                )?;

                let mime = format!("image/{}", output_format);
                Ok((bytes, mime))
            }
        };

        if let Ok((ref bytes, ref mime)) = result {
            if file_size > 0 && bytes.len() <= 16 * 1024 * 1024 {
                if let Ok(mut cache) = self.preview_cache.lock() {
                    if cache.len() >= PLUGIN_PREVIEW_CACHE_CAP {
                        cache.pop_front();
                    }
                    cache.push_back(PluginPreviewCacheEntry {
                        path: path.to_path_buf(),
                        file_size,
                        mtime_secs,
                        bytes: bytes.clone(),
                        mime: mime.clone(),
                    });
                }
            }
        }

        result
    }
}

impl ImageFormatPlugin for DynamicFormatPlugin {
    fn id(&self) -> &str {
        &self.manifest.id
    }

    fn name(&self) -> &str {
        &self.manifest.name
    }

    fn supported_extensions(&self) -> &[String] {
        &self.manifest.extensions
    }

    fn extract_metadata(
        &self,
        path: &Path,
        file: &mut File,
        mtime_secs: i64,
    ) -> Result<ExtractedMetadata, String> {
        let (width, height) = if let Some(ref host) = self.binary_host {
            // 独立プラグインバイナリ（C-ABI動的ライブラリ/CLIバイナリ）から高速取得
            host.extract_metadata(path).unwrap_or((None, None))
        } else {
            let (bytes, _) = self.extract_image_bytes(path, Some(file))?;
            extract_dimensions_from_png(&bytes)
        };

        tracing::debug!(
            "DynamicFormatPlugin metadata extracted: id={}, path={:?}, width={:?}, height={:?}",
            self.manifest.id,
            path,
            width,
            height
        );

        let ext = self.manifest.extensions.first().cloned();

        Ok(ExtractedMetadata {
            taken_at: mtime_secs,
            taken_at_source: 1, // mtime
            width,
            height,
            orientation: 1,
            format: ext,
        })
    }

    fn generate_thumbnail(
        &self,
        path: &Path,
        orientation: u32,
    ) -> Result<Vec<u8>, String> {
        let (bytes, _) = self.extract_image_bytes(path, None)?;

        let img = image::load_from_memory(&bytes)
            .map_err(|e| format!("画像バイナリのデコードに失敗 (id={}): {}", self.manifest.id, e))?;

        let webp_mem = crate::pipeline::thumbnail::resize_and_encode_webp(img, orientation)?;
        Ok(webp_mem.to_vec())
    }

    fn extract_original_preview(
        &self,
        path: &Path,
    ) -> Result<(Vec<u8>, String), String> {
        self.extract_image_bytes(path, None)
    }
}

/// プラグイン検索対象ディレクトリのリストを網羅的に解決する
///
/// 変更理由: インストール環境（exe直下のplugins、またはresources/plugins）、
/// ユーザーデータ領域（%APPDATA%/.../plugins）、開発環境（CARGO_MANIFEST_DIR/../plugins）、
/// およびexe親ディレクトリ等を漏れなく探索し、環境差異によるプラグイン未検出を防止するため。
///
/// @param app_data_dir アプリケーションデータディレクトリ
/// @return 探索対象ディレクトリ一覧（優先順位順）
pub fn get_plugin_search_dirs(app_data_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    let mut add_dir = |p: PathBuf| {
        let candidate = p.canonicalize().unwrap_or(p);
        if candidate.is_dir() && !dirs.contains(&candidate) {
            dirs.push(candidate);
        }
    };

    // 1. ユーザーデータ領域配下の plugins ディレクトリ (%APPDATA%/.../plugins)
    add_dir(app_data_dir.join("plugins"));

    #[cfg(target_os = "android")]
    {
        // 変更理由: Android 環境において、内部ストレージ（/data/user/0/.../files/plugins および
        // /data/data/.../files/plugins）に展開されたマニフェストを漏れなく探索するため。
        add_dir(PathBuf::from("/data/data/com.imagemediaviewer.desktop/files/plugins"));
        add_dir(PathBuf::from("/data/user/0/com.imagemediaviewer.desktop/files/plugins"));
    }

    // 2. カレント作業ディレクトリ配下の plugins（絶対パス解決）
    if let Ok(cwd) = std::env::current_dir() {
        add_dir(cwd.join("plugins"));
        // ワークスペース直下の plugins（src-tauri 等からの実行時対応）
        if let Some(parent) = cwd.parent() {
            add_dir(parent.join("plugins"));
        }
    }

    // 3. 実行ファイル（.exe）配置ディレクトリ相対およびリソース配下
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            // exe直下: <InstallDir>/plugins
            add_dir(exe_dir.join("plugins"));
            // Tauri bundle resources配下: <InstallDir>/resources/plugins
            add_dir(exe_dir.join("resources").join("plugins"));

            // macOS アプリケーションバンドル対応: <AppBundle>/Contents/Resources/plugins
            if let Some(contents_dir) = exe_dir.parent() {
                add_dir(contents_dir.join("Resources").join("plugins"));
                add_dir(contents_dir.join("resources").join("plugins"));
            }

            // 開発時: target/debug や target/release からの親ディレクトリ探索
            if let Some(parent1) = exe_dir.parent() {
                add_dir(parent1.join("plugins"));
                if let Some(parent2) = parent1.parent() {
                    add_dir(parent2.join("plugins"));
                    if let Some(parent3) = parent2.parent() {
                        add_dir(parent3.join("plugins"));
                    }
                }
            }
        }
    }

    // 4. CARGO_MANIFEST_DIR 相対 (開発・テスト実行時)
    let cargo_manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../plugins");
    add_dir(cargo_manifest);

    dirs
}

/// 検索ディレクトリ群から全有効プラグインマニフェスト（plugin.json）を検出・収集する
///
/// 変更理由: 優先順位（ユーザーデータ領域 %APPDATA%/plugins が最優先、次いでインストール先・開発環境）を遵守し、
/// 巨大ファイルの読み込み制限（5MB）および構文解析失敗時のログ警告出力を行うため。
///
/// @param search_dirs 探索先ディレクトリのリスト
/// @return プラグインID -> (マニフェスト, プラグイン配置ディレクトリ)
pub fn discover_plugins(search_dirs: &[PathBuf]) -> HashMap<String, (PluginManifest, PathBuf)> {
    let mut discovered = HashMap::new();

    for base_dir in search_dirs {
        if !base_dir.is_dir() {
            continue;
        }

        // 1. サブディレクトリ配下の plugin.json を探索 (例: plugins/clipstudio/plugin.json)
        if let Ok(entries) = std::fs::read_dir(base_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let manifest_file = path.join("plugin.json");
                    if manifest_file.is_file() {
                        if let Ok(meta) = manifest_file.metadata() {
                            if meta.len() > 5 * 1024 * 1024 {
                                tracing::warn!("プラグインマニフェストが上限サイズ(5MB)を超過しています: {:?}", manifest_file);
                                continue;
                            }
                        }
                        match std::fs::read_to_string(&manifest_file) {
                            Ok(content) => match serde_json::from_str::<PluginManifest>(&content) {
                                Ok(manifest) => {
                                    // 優先順位を尊重（上位探索ディレクトリを優先するため or_insert）
                                    discovered.entry(manifest.id.clone()).or_insert((manifest, path));
                                }
                                Err(e) => {
                                    tracing::warn!("プラグインマニフェストの解析に失敗 ({:?}): {}", manifest_file, e);
                                }
                            },
                            Err(e) => {
                                tracing::warn!("プラグインマニフェストの読み込みに失敗 ({:?}): {}", manifest_file, e);
                            }
                        }
                    }
                }
            }
        }

        // 2. ディレクトリ直下の plugin.json を探索
        let direct_manifest = base_dir.join("plugin.json");
        if direct_manifest.is_file() {
            if let Ok(meta) = direct_manifest.metadata() {
                if meta.len() > 5 * 1024 * 1024 {
                    tracing::warn!("プラグインマニフェストが上限サイズ(5MB)を超過しています: {:?}", direct_manifest);
                    continue;
                }
            }
            match std::fs::read_to_string(&direct_manifest) {
                Ok(content) => match serde_json::from_str::<PluginManifest>(&content) {
                    Ok(manifest) => {
                        discovered.entry(manifest.id.clone()).or_insert((manifest, base_dir.clone()));
                    }
                    Err(e) => {
                        tracing::warn!("プラグインマニフェストの解析に失敗 ({:?}): {}", direct_manifest, e);
                    }
                },
                Err(e) => {
                    tracing::warn!("プラグインマニフェストの読み込みに失敗 ({:?}): {}", direct_manifest, e);
                }
            }
        }
    }

    discovered
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_binary_slice_plugin_extraction() {
        let temp_dir = tempdir().unwrap();
        let custom_file = temp_dir.path().join("sample.custom");

        let mut data = Vec::new();
        data.extend_from_slice(b"CUSTOM_CONTAINER_START");
        data.extend_from_slice(b"PREFIX_SIGNATURE_XYZ");

        let img = image::RgbaImage::new(100, 80);
        let mut png_buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut png_buf, image::ImageFormat::Png).unwrap();
        let png_bytes = png_buf.into_inner();
        data.extend_from_slice(&png_bytes);

        data.extend_from_slice(b"SUFFIX_SIGNATURE_ZYX");
        data.extend_from_slice(b"TRAILING_CUSTOM_DATA");

        std::fs::write(&custom_file, &data).unwrap();

        let manifest = PluginManifest {
            id: "format.custom".to_string(),
            name: "Custom Slice Format".to_string(),
            version: Some("1.0.0".to_string()),
            description: None,
            extensions: vec!["custom".to_string()],
            magic_header: Some("CUSTOM_CONTAINER".to_string()),
            extractor: Some(ExtractorConfig::BinarySlice {
                start_signature: "89504E470D0A1A0A".to_string(),
                end_signature: "49454E44AE426082".to_string(),
                include_end_signature: true,
                mime_type: "image/png".to_string(),
            }),
            actions: Vec::new(),
        };

        let plugin = DynamicFormatPlugin::new(manifest, temp_dir.path().to_path_buf());
        let mut file = File::open(&custom_file).unwrap();
        let meta = plugin.extract_metadata(&custom_file, &mut file, 1600000000).unwrap();
        assert_eq!(meta.width, Some(100));
        assert_eq!(meta.height, Some(80));

        let (preview, mime) = plugin.extract_original_preview(&custom_file).unwrap();
        assert_eq!(mime, "image/png");
        assert!(preview.starts_with(b"\x89PNG\r\n\x1a\n"));
    }
}
