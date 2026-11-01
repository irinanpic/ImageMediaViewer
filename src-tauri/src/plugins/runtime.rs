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

// =================================================================================================
// アーカイブ（圧縮ファイル）仮想フォルダ・閲覧プラグイン基盤
// =================================================================================================

/// 仮想パス（`<archive_path>!/<inner_path>`）をアーカイブ実ファイルパスと内部相対パスに分割する
///
/// 変更理由: DB・キャッシュ機構を一切破壊せずにアーカイブ内画像を単一の `file_path` 文字列として
/// 透過的に取り扱い、サムネイル生成や原寸配信時に O(1) でアーカイブ実体と内部エントリを判別するため。
///
/// @param file_path 検査対象のファイルパス文字列
/// @return アーカイブ仮想パスの場合は `Some((archive_path, inner_path))`、通常ファイルは `None`
#[inline]
pub fn split_archive_virtual_path(file_path: &str) -> Option<(&str, &str)> {
    file_path.split_once("!/")
}

/// ファイル名・相対パス用の自然順ソート（Natural Sort）比較関数
///
/// 変更理由: アーカイブ内の連番画像（`page1.jpg`, `page2.jpg`, ..., `page10.jpg`）を
/// 辞書順（`page1.jpg`, `page10.jpg`, `page2.jpg`）ではなく人間の直感に沿ったページ順で並べるため。
///
/// @param a 比較文字列A
/// @param b 比較文字列B
/// @return 順序比較結果
pub fn compare_natural(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    let mut chars_a = a.chars().peekable();
    let mut chars_b = b.chars().peekable();

    loop {
        match (chars_a.peek(), chars_b.peek()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(&ca), Some(&cb)) => {
                if ca.is_ascii_digit() && cb.is_ascii_digit() {
                    let mut num_a_str = String::new();
                    while let Some(&c) = chars_a.peek() {
                        if c.is_ascii_digit() {
                            num_a_str.push(c);
                            chars_a.next();
                        } else {
                            break;
                        }
                    }
                    let mut num_b_str = String::new();
                    while let Some(&c) = chars_b.peek() {
                        if c.is_ascii_digit() {
                            num_b_str.push(c);
                            chars_b.next();
                        } else {
                            break;
                        }
                    }

                    // 先頭ゼロを除去した数値桁数・数値大小で比較
                    let trim_a = num_a_str.trim_start_matches('0');
                    let trim_b = num_b_str.trim_start_matches('0');
                    match trim_a.len().cmp(&trim_b.len()) {
                        Ordering::Equal => match trim_a.cmp(trim_b) {
                            Ordering::Equal => {
                                // 数値として同じ（例: "01" と "1"）なら桁数（ゼロの多さ）で決定的に比較
                                match num_a_str.len().cmp(&num_b_str.len()) {
                                    Ordering::Equal => continue,
                                    ord => return ord,
                                }
                            }
                            ord => return ord,
                        },
                        ord => return ord,
                    }
                } else {
                    let la = ca.to_ascii_lowercase();
                    let lb = cb.to_ascii_lowercase();
                    match la.cmp(&lb) {
                        Ordering::Equal => {
                            chars_a.next();
                            chars_b.next();
                        }
                        ord => return ord,
                    }
                }
            }
        }
    }
}

/// アーカイブ内エントリの軽量メタデータ（展開せずにセントラルディレクトリ等から取得）
///
/// 変更理由: 巨大アーカイブであってもデータ本体を一切解凍せず、一覧取得を数ミリ秒で完了させるため。
#[derive(Debug, Clone)]
pub struct ArchiveEntryMeta {
    /// アーカイブ内の正規化済み相対パス（区切り文字は常に `/`、先頭 `/` なし）
    pub inner_path: String,
    /// ファイル名単体（例: "001.png"）
    pub file_name: String,
    /// 非圧縮時ファイルサイズ（バイト）
    pub uncompressed_size: u64,
    /// 更新日時（UNIX秒、不明時はアーカイブ自体のmtimeにフォールバック）
    pub mtime_secs: Option<i64>,
    /// 拡張子（小文字、ドットなし: "jpg", "png", "webp" 等）
    pub extension: String,
}

/// アーカイブ（圧縮ファイル）プラグインの共通インターフェース
///
/// 変更理由: 標準内蔵のZIP/CBZプラグインだけでなく、外部コマンド（7z/unrar等）を用いた
/// プラグイン（.rar, .7z, .cbr, .cb7, .tar等）も同じトレイトで透過的に統合可能にするため。
pub trait ArchivePlugin: Send + Sync {
    /// プラグイン一意識別子（例: "archive.zip", "archive.sevenzip"）
    fn id(&self) -> &str;

    /// プラグイン表示名
    fn name(&self) -> &str;

    /// 対応するアーカイブ拡張子（小文字、ドットなし: ["zip", "cbz"] 等）
    fn supported_extensions(&self) -> &[String];

    /// アーカイブ内のエントリ一覧をデータ本体を展開せずに高速取得する
    ///
    /// @param archive_path アーカイブファイルの実パス
    /// @return 内部エントリのメタデータ配列
    fn list_entries(&self, archive_path: &Path) -> Result<Vec<ArchiveEntryMeta>, String>;

    /// アーカイブ内の指定エントリの生バイト列をメモリ上へ直接抽出する（一時ファイル不使用）
    ///
    /// @param archive_path アーカイブファイルの実パス
    /// @param inner_path アーカイブ内の相対パス（`/` 区切り）
    /// @return 抽出されたエントリの生バイト列
    fn extract_entry(&self, archive_path: &Path, inner_path: &str) -> Result<Vec<u8>, String>;
}

/// アーカイブ抽出結果の短命インメモリキャッシュ上限（エントリ数および最大合計バイト数）
const ARCHIVE_CACHE_MAX_ENTRIES: usize = 12;
const ARCHIVE_CACHE_MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;
/// 単一エントリの最大解凍サイズ上限（100MB: Zip Bomb対策）
const ARCHIVE_MAX_ENTRY_EXTRACT_BYTES: u64 = 100 * 1024 * 1024;

struct ArchiveExtractCacheEntry {
    archive_path: PathBuf,
    archive_mtime: i64,
    inner_path: String,
    bytes: Vec<u8>,
}

/// アーカイブ内エントリパスを正規化する（`\` を `/` に変換し、先頭スラッシュを除去）
///
/// @param raw_name アーカイブ内の生ファイル名
/// @return 安全な正規化済み相対パス。パストラバーサル（`..`）を含む場合は `None`
pub fn normalize_archive_inner_path(raw_name: &str) -> Option<String> {
    let replaced = raw_name.replace('\\', "/");
    let trimmed = replaced.trim_start_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    // パストラバーサル防止
    for seg in trimmed.split('/') {
        if seg == ".." {
            return None;
        }
    }
    Some(trimmed.to_string())
}

/// アーカイブ内の不可視メタデータファイル（macOS `__MACOSX/` や `._*` 等）かどうか判定する
fn is_ignored_archive_entry(inner_path: &str, file_name: &str) -> bool {
    if inner_path.ends_with('/') {
        return true;
    }
    if inner_path.starts_with("__MACOSX/") || inner_path.contains("/__MACOSX/") {
        return true;
    }
    if file_name.starts_with("._") || file_name.eq_ignore_ascii_case("thumbs.db") || file_name.eq_ignore_ascii_case(".ds_store") {
        return true;
    }
    false
}

/// Rustネイティブ `zip` クレートによる内蔵 ZIP / CBZ アーカイブ閲覧プラグイン
///
/// 変更理由: 外部プロセス起動を一切行わず、セントラルディレクトリの O(1) 走査と
/// インメモリ直接抽出によって超高速なアーカイブ内画像閲覧を実現するため。
pub struct BuiltinZipArchivePlugin {
    extensions: Vec<String>,
    cache: Mutex<VecDeque<ArchiveExtractCacheEntry>>,
}

impl BuiltinZipArchivePlugin {
    /// 内蔵ZIPプラグインインスタンスを生成
    pub fn new() -> Self {
        Self {
            extensions: vec!["zip".to_string(), "cbz".to_string()],
            cache: Mutex::new(VecDeque::with_capacity(ARCHIVE_CACHE_MAX_ENTRIES)),
        }
    }
}

impl Default for BuiltinZipArchivePlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ArchivePlugin for BuiltinZipArchivePlugin {
    fn id(&self) -> &str {
        "archive.zip"
    }

    fn name(&self) -> &str {
        "ZIP / CBZ アーカイブ閲覧プラグイン"
    }

    fn supported_extensions(&self) -> &[String] {
        &self.extensions
    }

    fn list_entries(&self, archive_path: &Path) -> Result<Vec<ArchiveEntryMeta>, String> {
        let file = File::open(archive_path)
            .map_err(|e| format!("ZIPファイルを開けません ({}): {}", archive_path.display(), e))?;
        let archive_mtime = file
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);

        let mut archive = zip::ZipArchive::new(file)
            .map_err(|e| format!("ZIP構造の解析に失敗 ({}): {}", archive_path.display(), e))?;

        let mut entries = Vec::with_capacity(archive.len());
        for i in 0..archive.len() {
            // 変更理由: by_index_raw を使用してデータストリームを一切解凍せずセントラルディレクトリ情報のみをO(1)で取得する
            let raw_file = match archive.by_index_raw(i) {
                Ok(f) => f,
                Err(_) => continue,
            };
            if raw_file.is_dir() {
                continue;
            }
            let raw_name = raw_file.name();
            let inner_path = match normalize_archive_inner_path(raw_name) {
                Some(p) => p,
                None => continue,
            };
            let file_name = inner_path
                .rsplit('/')
                .next()
                .unwrap_or(&inner_path)
                .to_string();

            if is_ignored_archive_entry(&inner_path, &file_name) {
                continue;
            }

            let extension = Path::new(&file_name)
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase())
                .unwrap_or_default();
            if extension.is_empty() {
                continue;
            }

            let uncompressed_size = raw_file.size();
            if uncompressed_size == 0 || uncompressed_size > ARCHIVE_MAX_ENTRY_EXTRACT_BYTES {
                continue;
            }

            entries.push(ArchiveEntryMeta {
                inner_path,
                file_name,
                uncompressed_size,
                mtime_secs: archive_mtime,
                extension,
            });
        }

        Ok(entries)
    }

    fn extract_entry(&self, archive_path: &Path, inner_path: &str) -> Result<Vec<u8>, String> {
        let normalized_target = normalize_archive_inner_path(inner_path)
            .ok_or_else(|| format!("不正なアーカイブ内パスです: {}", inner_path))?;

        let archive_mtime = std::fs::metadata(archive_path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        // 1. 短命リングキャッシュの確認
        if let Ok(cache) = self.cache.lock() {
            if let Some(hit) = cache.iter().find(|c| {
                c.archive_mtime == archive_mtime
                    && c.inner_path == normalized_target
                    && c.archive_path == archive_path
            }) {
                return Ok(hit.bytes.clone());
            }
        }

        let file = File::open(archive_path)
            .map_err(|e| format!("ZIPファイルを開けません ({}): {}", archive_path.display(), e))?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|e| format!("ZIP構造の解析に失敗 ({}): {}", archive_path.display(), e))?;

        // 2. まず直接 by_name で検索し、見つからない場合（バックスラッシュ区切り格納ZIP等）はインデックス走査
        let mut extracted_bytes: Option<Vec<u8>> = None;

        {
            if let Ok(mut zf) = archive.by_name(&normalized_target) {
                if zf.size() > ARCHIVE_MAX_ENTRY_EXTRACT_BYTES {
                    return Err(format!(
                        "アーカイブ内エントリがサイズ上限(100MB)を超過しています: {} bytes",
                        zf.size()
                    ));
                }
                let mut buf = Vec::with_capacity(zf.size() as usize);
                zf.by_ref()
                    .take(ARCHIVE_MAX_ENTRY_EXTRACT_BYTES)
                    .read_to_end(&mut buf)
                    .map_err(|e| format!("ZIPエントリの展開に失敗 ({}): {}", normalized_target, e))?;
                extracted_bytes = Some(buf);
            }
        }

        if extracted_bytes.is_none() {
            let backslash_name = normalized_target.replace('/', "\\");
            if let Ok(mut zf) = archive.by_name(&backslash_name) {
                if zf.size() > ARCHIVE_MAX_ENTRY_EXTRACT_BYTES {
                    return Err(format!(
                        "アーカイブ内エントリがサイズ上限(100MB)を超過しています: {} bytes",
                        zf.size()
                    ));
                }
                let mut buf = Vec::with_capacity(zf.size() as usize);
                zf.by_ref()
                    .take(ARCHIVE_MAX_ENTRY_EXTRACT_BYTES)
                    .read_to_end(&mut buf)
                    .map_err(|e| format!("ZIPエントリの展開に失敗 ({}): {}", normalized_target, e))?;
                extracted_bytes = Some(buf);
            }
        }

        if extracted_bytes.is_none() {
            let mut matched_idx = None;
            for i in 0..archive.len() {
                if let Ok(raw) = archive.by_index_raw(i) {
                    if let Some(norm) = normalize_archive_inner_path(raw.name()) {
                        if norm == normalized_target {
                            matched_idx = Some(i);
                            break;
                        }
                    }
                }
            }
            if let Some(idx) = matched_idx {
                let mut zf = archive
                    .by_index(idx)
                    .map_err(|e| format!("ZIPエントリ取得失敗 ({}): {}", normalized_target, e))?;
                if zf.size() > ARCHIVE_MAX_ENTRY_EXTRACT_BYTES {
                    return Err(format!(
                        "アーカイブ内エントリがサイズ上限(100MB)を超過しています: {} bytes",
                        zf.size()
                    ));
                }
                let mut buf = Vec::with_capacity(zf.size() as usize);
                zf.by_ref()
                    .take(ARCHIVE_MAX_ENTRY_EXTRACT_BYTES)
                    .read_to_end(&mut buf)
                    .map_err(|e| format!("ZIPエントリの展開に失敗 ({}): {}", normalized_target, e))?;
                extracted_bytes = Some(buf);
            }
        }

        let bytes = extracted_bytes.ok_or_else(|| {
            format!(
                "ZIP内に該当エントリが見つかりません: {}!/{}",
                archive_path.display(),
                normalized_target
            )
        })?;

        // 3. 短命リングバッファへ格納（最大12件・合計64MB以内）
        if bytes.len() <= 16 * 1024 * 1024 {
            if let Ok(mut cache) = self.cache.lock() {
                while cache.len() >= ARCHIVE_CACHE_MAX_ENTRIES
                    || (cache.iter().map(|c| c.bytes.len()).sum::<usize>() + bytes.len()
                        > ARCHIVE_CACHE_MAX_TOTAL_BYTES
                        && !cache.is_empty())
                {
                    cache.pop_front();
                }
                cache.push_back(ArchiveExtractCacheEntry {
                    archive_path: archive_path.to_path_buf(),
                    archive_mtime,
                    inner_path: normalized_target,
                    bytes: bytes.clone(),
                });
            }
        }

        Ok(bytes)
    }
}

/// 外部CLIツール（`7z`, `unrar`, `tar` 等）および `plugin.json` を用いた動的アーカイブプラグイン
///
/// 変更理由: アプリ本体の再コンパイルなしに、ユーザーが `plugins/` フォルダへマニフェストを配置するだけで
/// `.7z`, `.cb7`, `.rar`, `.cbr`, `.tar` 等の圧縮形式を仮想フォルダとして透過的に閲覧可能にするため。
pub struct ExternalCliArchivePlugin {
    pub manifest: PluginManifest,
    pub plugin_dir: PathBuf,
    cache: Mutex<VecDeque<ArchiveExtractCacheEntry>>,
}

impl ExternalCliArchivePlugin {
    /// マニフェストから外部CLIアーカイブプラグインを生成
    pub fn new(mut manifest: PluginManifest, plugin_dir: PathBuf) -> Self {
        manifest.extensions = manifest
            .extensions
            .into_iter()
            .map(|ext| ext.trim().trim_start_matches('.').to_ascii_lowercase())
            .filter(|ext| !ext.is_empty())
            .collect();

        Self {
            manifest,
            plugin_dir,
            cache: Mutex::new(VecDeque::with_capacity(ARCHIVE_CACHE_MAX_ENTRIES)),
        }
    }

    fn resolve_cmd(&self, cmd: &str) -> PathBuf {
        let p = Path::new(cmd);
        if p.is_absolute() {
            return p.to_path_buf();
        }
        let in_plugin = self.plugin_dir.join(cmd);
        if in_plugin.is_file() {
            return in_plugin;
        }
        #[cfg(target_os = "windows")]
        {
            let in_plugin_exe = self.plugin_dir.join(format!("{}.exe", cmd));
            if in_plugin_exe.is_file() {
                return in_plugin_exe;
            }
        }
        PathBuf::from(cmd)
    }
}

impl ArchivePlugin for ExternalCliArchivePlugin {
    fn id(&self) -> &str {
        &self.manifest.id
    }

    fn name(&self) -> &str {
        &self.manifest.name
    }

    fn supported_extensions(&self) -> &[String] {
        &self.manifest.extensions
    }

    fn list_entries(&self, archive_path: &Path) -> Result<Vec<ArchiveEntryMeta>, String> {
        let list_cmd = self
            .manifest
            .archive_list_command
            .as_ref()
            .filter(|c| !c.is_empty())
            .ok_or_else(|| format!("プラグイン {} に archive_list_command が定義されていません", self.manifest.id))?;

        let exe = self.resolve_cmd(&list_cmd[0]);
        let archive_str = archive_path.to_string_lossy().to_string();
        let args: Vec<String> = list_cmd[1..]
            .iter()
            .map(|a| a.replace("{archive_path}", &archive_str))
            .collect();

        let archive_mtime = std::fs::metadata(archive_path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);

        let output = std::process::Command::new(&exe)
            .args(&args)
            .output()
            .map_err(|e| format!("外部アーカイブコマンドの実行に失敗 ({}): {}", exe.display(), e))?;

        if !output.status.success() {
            return Err(format!(
                "外部アーカイブ一覧取得コマンドがエラー終了しました: {:?}",
                output.status.code()
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut entries = Vec::new();

        // 7z -slt 形式（Path = ..., Size = ..., Folder = -）または単純な1行1パス形式の両方をパース
        if stdout.contains("Path = ") && stdout.contains("----------") {
            let body = stdout.split("----------").nth(1).unwrap_or(&stdout);
            let mut cur_path: Option<String> = None;
            let mut cur_size: u64 = 0;
            let mut cur_is_dir = false;

            let mut flush_block = |path_opt: Option<String>, size: u64, is_dir: bool| {
                if is_dir {
                    return;
                }
                if let Some(raw_p) = path_opt {
                    if let Some(inner_path) = normalize_archive_inner_path(&raw_p) {
                        let file_name = inner_path
                            .rsplit('/')
                            .next()
                            .unwrap_or(&inner_path)
                            .to_string();
                        if is_ignored_archive_entry(&inner_path, &file_name) {
                            return;
                        }
                        let extension = Path::new(&file_name)
                            .extension()
                            .and_then(|e| e.to_str())
                            .map(|e| e.to_ascii_lowercase())
                            .unwrap_or_default();
                        if !extension.is_empty() && size > 0 && size <= ARCHIVE_MAX_ENTRY_EXTRACT_BYTES {
                            entries.push(ArchiveEntryMeta {
                                inner_path,
                                file_name,
                                uncompressed_size: size,
                                mtime_secs: archive_mtime,
                                extension,
                            });
                        }
                    }
                }
            };

            for line in body.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    flush_block(cur_path.take(), cur_size, cur_is_dir);
                    cur_size = 0;
                    cur_is_dir = false;
                } else if let Some(rest) = trimmed.strip_prefix("Path = ") {
                    cur_path = Some(rest.trim().to_string());
                } else if let Some(rest) = trimmed.strip_prefix("Size = ") {
                    cur_size = rest.trim().parse::<u64>().unwrap_or(0);
                } else if let Some(rest) = trimmed.strip_prefix("Folder = ") {
                    if rest.trim() == "+" {
                        cur_is_dir = true;
                    }
                } else if let Some(rest) = trimmed.strip_prefix("Attributes = ") {
                    if rest.trim().starts_with('D') {
                        cur_is_dir = true;
                    }
                }
            }
            flush_block(cur_path.take(), cur_size, cur_is_dir);
        } else {
            for line in stdout.lines() {
                let raw = line.trim();
                if raw.is_empty() {
                    continue;
                }
                if let Some(inner_path) = normalize_archive_inner_path(raw) {
                    let file_name = inner_path
                        .rsplit('/')
                        .next()
                        .unwrap_or(&inner_path)
                        .to_string();
                    if is_ignored_archive_entry(&inner_path, &file_name) {
                        continue;
                    }
                    let extension = Path::new(&file_name)
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(|e| e.to_ascii_lowercase())
                        .unwrap_or_default();
                    if !extension.is_empty() {
                        entries.push(ArchiveEntryMeta {
                            inner_path,
                            file_name,
                            uncompressed_size: 1,
                            mtime_secs: archive_mtime,
                            extension,
                        });
                    }
                }
            }
        }

        Ok(entries)
    }

    fn extract_entry(&self, archive_path: &Path, inner_path: &str) -> Result<Vec<u8>, String> {
        let normalized_target = normalize_archive_inner_path(inner_path)
            .ok_or_else(|| format!("不正なアーカイブ内パスです: {}", inner_path))?;

        let archive_mtime = std::fs::metadata(archive_path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        if let Ok(cache) = self.cache.lock() {
            if let Some(hit) = cache.iter().find(|c| {
                c.archive_mtime == archive_mtime
                    && c.inner_path == normalized_target
                    && c.archive_path == archive_path
            }) {
                return Ok(hit.bytes.clone());
            }
        }

        let extract_cmd = self
            .manifest
            .archive_extract_command
            .as_ref()
            .filter(|c| !c.is_empty())
            .ok_or_else(|| format!("プラグイン {} に archive_extract_command が定義されていません", self.manifest.id))?;

        let exe = self.resolve_cmd(&extract_cmd[0]);
        let archive_str = archive_path.to_string_lossy().to_string();
        let args: Vec<String> = extract_cmd[1..]
            .iter()
            .map(|a| {
                a.replace("{archive_path}", &archive_str)
                    .replace("{entry_path}", &normalized_target)
            })
            .collect();

        let output = std::process::Command::new(&exe)
            .args(&args)
            .output()
            .map_err(|e| format!("外部アーカイブ抽出コマンドの実行に失敗 ({}): {}", exe.display(), e))?;

        if !output.status.success() || output.stdout.is_empty() {
            return Err(format!(
                "外部アーカイブ抽出コマンドが失敗または空出力でした: {:?}",
                output.status.code()
            ));
        }

        let bytes = output.stdout;
        if bytes.len() as u64 > ARCHIVE_MAX_ENTRY_EXTRACT_BYTES {
            return Err("アーカイブ内エントリがサイズ上限(100MB)を超過しています".to_string());
        }

        if bytes.len() <= 16 * 1024 * 1024 {
            if let Ok(mut cache) = self.cache.lock() {
                while cache.len() >= ARCHIVE_CACHE_MAX_ENTRIES
                    || (cache.iter().map(|c| c.bytes.len()).sum::<usize>() + bytes.len()
                        > ARCHIVE_CACHE_MAX_TOTAL_BYTES
                        && !cache.is_empty())
                {
                    cache.pop_front();
                }
                cache.push_back(ArchiveExtractCacheEntry {
                    archive_path: archive_path.to_path_buf(),
                    archive_mtime,
                    inner_path: normalized_target,
                    bytes: bytes.clone(),
                });
            }
        }

        Ok(bytes)
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
            plugin_type: None,
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
            archive_list_command: None,
            archive_extract_command: None,
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

    #[test]
    fn test_split_archive_virtual_path() {
        assert_eq!(
            split_archive_virtual_path(r"C:\Pictures\manga_vol1.zip!/ch01/001.png"),
            Some((r"C:\Pictures\manga_vol1.zip", "ch01/001.png"))
        );
        assert_eq!(
            split_archive_virtual_path(r"C:\Pictures\normal_photo.jpg"),
            None
        );
    }

    #[test]
    fn test_compare_natural() {
        let mut names = vec![
            "page10.jpg",
            "page2.jpg",
            "page1.jpg",
            "page20.jpg",
            "ch02/001.png",
            "ch01/010.png",
            "ch01/002.png",
        ];
        names.sort_by(|a, b| compare_natural(a, b));
        assert_eq!(
            names,
            vec![
                "ch01/002.png",
                "ch01/010.png",
                "ch02/001.png",
                "page1.jpg",
                "page2.jpg",
                "page10.jpg",
                "page20.jpg",
            ]
        );
    }

    #[test]
    fn test_builtin_zip_archive_plugin() {
        use std::io::Write;
        let temp_dir = tempdir().unwrap();
        let zip_path = temp_dir.path().join("test_book.cbz");

        {
            let file = File::create(&zip_path).unwrap();
            let mut zip_writer = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);

            zip_writer.start_file("ch1/002.png", options).unwrap();
            zip_writer.write_all(b"PNG_DATA_2").unwrap();

            zip_writer.start_file("ch1/001.jpg", options).unwrap();
            zip_writer.write_all(b"JPG_DATA_1").unwrap();

            // macOS ゴミファイルは除外されること
            zip_writer.start_file("__MACOSX/ch1/._001.jpg", options).unwrap();
            zip_writer.write_all(b"MAC_JUNK").unwrap();

            zip_writer.finish().unwrap();
        }

        let plugin = BuiltinZipArchivePlugin::new();
        let entries = plugin.list_entries(&zip_path).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].inner_path, "ch1/002.png");
        assert_eq!(entries[1].inner_path, "ch1/001.jpg");

        let data1 = plugin.extract_entry(&zip_path, "ch1/001.jpg").unwrap();
        assert_eq!(data1, b"JPG_DATA_1");

        // 2回目はインメモリキャッシュから高速返却されること
        let data1_cached = plugin.extract_entry(&zip_path, "ch1/001.jpg").unwrap();
        assert_eq!(data1_cached, b"JPG_DATA_1");
    }
}
