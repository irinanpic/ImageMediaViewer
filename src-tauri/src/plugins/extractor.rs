//! 汎用画像抽出エンジンおよびバイナリプラグインホスト
//!
//! 変更理由: アプリ本体コアから特定のソフトウェア形式（ClipStudio等）やSQLite BLOB走査ロジックを
//! 分離し、C-ABI 動的ライブラリ（.dll/.so/.dylib）および独立実行バイナリ（CLI）を呼び出す
//! 汎用的なプラグインホスト機能を提供するため。

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
#[cfg(not(target_os = "ios"))]
use std::sync::{Arc, RwLock};

#[cfg(not(target_os = "ios"))]
use std::ffi::CString;

use super::manifest::PlatformBinaryConfig;

// -------------------------------------------------------------------------------------------------
// C-ABI 動的ライブラリシンボル定義（Windows, Linux, macOS, Android対応）
// -------------------------------------------------------------------------------------------------

#[cfg(not(target_os = "ios"))]
struct DynamicLibrarySymbols {
    _lib: libloading::Library,
    extract_metadata: unsafe extern "C" fn(*const std::os::raw::c_char, *mut u32, *mut u32) -> i32,
    extract_preview: unsafe extern "C" fn(*const std::os::raw::c_char, *mut *mut u8, *mut usize) -> i32,
    free_buffer: unsafe extern "C" fn(*mut u8, usize),
}

/// 独立プラグインバイナリ（C-ABI動的ライブラリ / CLI実行ファイル）呼び出しホスト
///
/// 変更理由: プラグイン本体を別プロセスまたは独立ライブラリとして分離し、
/// 安全性・クラッシュ隔離・超高速なインメモリ呼び出しを両立するため。
pub struct BinaryPluginHost {
    #[allow(dead_code)]
    plugin_dir: PathBuf,
    #[allow(dead_code)]
    lib_path: Option<PathBuf>,
    #[allow(dead_code)]
    raw_lib_name: Option<String>,
    exe_path: Option<PathBuf>,
    #[cfg(not(target_os = "ios"))]
    loaded_lib: RwLock<Option<Result<Arc<DynamicLibrarySymbols>, String>>>,
}

impl BinaryPluginHost {
    /// 新規インスタンスを生成
    ///
    /// @param plugin_dir プラグイン配置ディレクトリ
    /// @param lib_config 動的ライブラリ設定（省略可能）
    /// @param exe_config 実行バイナリ設定（省略可能）
    pub fn new(
        plugin_dir: PathBuf,
        lib_config: Option<&PlatformBinaryConfig>,
        exe_config: Option<&PlatformBinaryConfig>,
    ) -> Self {
        let raw_lib_name = lib_config
            .and_then(|c| c.resolve_for_current_os())
            .map(|s| s.to_string());

        let lib_path = raw_lib_name
            .as_ref()
            .map(|name| resolve_binary_path(&plugin_dir, name));

        let exe_path = exe_config
            .and_then(|c| c.resolve_for_current_os())
            .map(|name| resolve_binary_path(&plugin_dir, name));

        Self {
            plugin_dir,
            lib_path,
            raw_lib_name,
            exe_path,
            #[cfg(not(target_os = "ios"))]
            loaded_lib: RwLock::new(None),
        }
    }

    /// 動的ライブラリをロードしてシンボルを取得する（キャッシュ付き）
    ///
    /// 変更理由: プラグイン動的ライブラリ（.dll / .so / .dylib）から C-ABI 関数群をロードし、
    /// プロセス間通信（IPC）やプロセス起動のオーバーヘッドをゼロにして最高速に実行するため。
    /// Android では APK 内部の nativeLibraryDir からの直接ロード（ライブラリ名指定）にも対応する。
    #[cfg(not(target_os = "ios"))]
    fn get_or_load_library(&self) -> Result<Arc<DynamicLibrarySymbols>, String> {
        let read_guard = self.loaded_lib.read().map_err(|e| e.to_string())?;
        if let Some(ref res) = *read_guard {
            return res.clone();
        }
        drop(read_guard);

        let mut write_guard = self.loaded_lib.write().map_err(|e| e.to_string())?;
        if let Some(ref res) = *write_guard {
            return res.clone();
        }

        let mut load_errors = Vec::new();
        let mut loaded_lib_opt = None;

        // 1. Android 環境では APK の nativeLibraryDir (Bionic dlopen 検索パス) からの
        //    ライブラリ名単体（例: "libimv_clipstudio.so"）ロードを最優先する
        // 変更理由: Android 10+ (API 29+) では W^X セキュリティ制約によりアプリ内部ストレージ
        // 配下のパス指定 dlopen が拒絶されるため、OS 管理の nativeLibraryDir からの直接ロードを行う。
        #[cfg(target_os = "android")]
        if let Some(ref name) = self.raw_lib_name {
            match unsafe { libloading::Library::new(name) } {
                Ok(l) => {
                    tracing::info!("Android nativeLibraryDir より動的ライブラリロード成功: {}", name);
                    loaded_lib_opt = Some(l);
                }
                Err(e) => {
                    load_errors.push(format!("Android nativeLibraryDir ({}) でのロード失敗: {}", name, e));
                }
            }
        }

        // 2. 指定ファイルパスが存在する場合はそのパスからロードを試行（デスクトップ環境等）
        if loaded_lib_opt.is_none() {
            if let Some(ref p) = self.lib_path {
                if p.is_file() {
                    match unsafe { libloading::Library::new(p) } {
                        Ok(l) => {
                            tracing::info!("プラグイン動的ライブラリをファイルパスからロード成功: {}", p.display());
                            loaded_lib_opt = Some(l);
                        }
                        Err(e) => {
                            load_errors.push(format!("ファイルパス ({}) でのロード失敗: {}", p.display(), e));
                        }
                    }
                } else {
                    load_errors.push(format!("プラグイン動的ライブラリファイルが存在しません: {}", p.display()));
                }
            }
        }

        // 3. ファイルパスで見つからない/ロード失敗した場合、ライブラリ名単体でのシステムネイティブライブラリ検索を試行
        if loaded_lib_opt.is_none() {
            if let Some(ref name) = self.raw_lib_name {
                match unsafe { libloading::Library::new(name) } {
                    Ok(l) => {
                        tracing::info!("システム/APKネイティブライブラリパスよりロード成功: {}", name);
                        loaded_lib_opt = Some(l);
                    }
                    Err(e) => {
                        load_errors.push(format!("ライブラリ名 ({}) でのロード失敗: {}", name, e));
                    }
                }
            }
        }

        let lib = match loaded_lib_opt {
            Some(l) => l,
            None => {
                let err = format!(
                    "動的ライブラリのロードに失敗しました: {}",
                    if load_errors.is_empty() {
                        "動的ライブラリパスが設定されていません".to_string()
                    } else {
                        load_errors.join("; ")
                    }
                );
                tracing::warn!("{}", err);
                return Err(err);
            }
        };

        unsafe {
            // オプションで init 関数を実行
            if let Ok(init_fn) = lib.get::<unsafe extern "C" fn() -> i32>(b"imv_plugin_init") {
                let init_code = init_fn();
                if init_code != 0 {
                    return Err(format!("プラグイン初期化関数がエラーコードを出力しました: {}", init_code));
                }
            }

            let extract_metadata = match lib.get::<unsafe extern "C" fn(*const std::os::raw::c_char, *mut u32, *mut u32) -> i32>(b"imv_plugin_extract_metadata") {
                Ok(sym) => *sym,
                Err(e) => {
                    return Err(format!("シンボル 'imv_plugin_extract_metadata' が見つかりません: {}", e));
                }
            };

            let extract_preview = match lib.get::<unsafe extern "C" fn(*const std::os::raw::c_char, *mut *mut u8, *mut usize) -> i32>(b"imv_plugin_extract_preview") {
                Ok(sym) => *sym,
                Err(e) => {
                    return Err(format!("シンボル 'imv_plugin_extract_preview' が見つかりません: {}", e));
                }
            };

            let free_buffer = match lib.get::<unsafe extern "C" fn(*mut u8, usize)>(b"imv_plugin_free_buffer") {
                Ok(sym) => *sym,
                Err(e) => {
                    return Err(format!("シンボル 'imv_plugin_free_buffer' が見つかりません: {}", e));
                }
            };

            let symbols = Arc::new(DynamicLibrarySymbols {
                _lib: lib,
                extract_metadata,
                extract_preview,
                free_buffer,
            });

            *write_guard = Some(Ok(Arc::clone(&symbols)));
            Ok(symbols)
        }
    }

    /// メタデータ（幅・高さ）を抽出する
    ///
    /// 変更理由: まず超高速な C-ABI 動的ライブラリでの呼び出しを試み、
    /// 利用できない環境や失敗時は独立実行バイナリ（CLI）へ透過的にフォールバックするため。
    ///
    /// @param path 画像ファイルパス
    /// @return (Option<幅>, Option<高さ>)
    pub fn extract_metadata(&self, path: &Path) -> Result<(Option<u32>, Option<u32>), String> {
        // 1. C-ABI 動的ライブラリ呼び出しの試行
        #[cfg(not(target_os = "ios"))]
        if let Ok(syms) = self.get_or_load_library() {
            let path_str = path.to_string_lossy();
            if let Ok(c_path) = CString::new(path_str.as_bytes()) {
                let mut width = 0u32;
                let mut height = 0u32;
                let code = unsafe {
                    (syms.extract_metadata)(c_path.as_ptr(), &mut width, &mut height)
                };
                if code == 0 && width > 0 && height > 0 {
                    return Ok((Some(width), Some(height)));
                }
            }
        }

        // 2. 独立実行バイナリ（CLI）フォールバック
        if let Some(ref exe) = self.exe_path {
            if exe.is_file() {
                let output = std::process::Command::new(exe)
                    .arg("metadata")
                    .arg(path)
                    .output()
                    .map_err(|e| format!("プラグインバイナリの実行に失敗 ({}): {}", exe.display(), e))?;

                if output.status.success() {
                    let stdout_str = String::from_utf8_lossy(&output.stdout);
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&stdout_str) {
                        let w = val.get("width").and_then(|v| v.as_u64()).map(|v| v as u32);
                        let h = val.get("height").and_then(|v| v.as_u64()).map(|v| v as u32);
                        if w.is_some() || h.is_some() {
                            return Ok((w, h));
                        }
                    }
                }
            }
        }

        // 3. プレビュー画像から寸法を抽出する最終フォールバック
        if let Ok((bytes, _)) = self.extract_preview(path) {
            let (w, h) = extract_dimensions_from_png(&bytes);
            return Ok((w, h));
        }

        Err("プラグインによるメタデータ抽出に失敗しました".to_string())
    }

    /// 原寸プレビュー画像バイナリを抽出する
    ///
    /// @param path 画像ファイルパス
    /// @return (バイナリ列, MIMEタイプ文字列)
    pub fn extract_preview(&self, path: &Path) -> Result<(Vec<u8>, String), String> {
        // 1. C-ABI 動的ライブラリ呼び出しの試行
        #[cfg(not(target_os = "ios"))]
        if let Ok(syms) = self.get_or_load_library() {
            let path_str = path.to_string_lossy();
            if let Ok(c_path) = CString::new(path_str.as_bytes()) {
                let mut out_buf: *mut u8 = std::ptr::null_mut();
                let mut out_len: usize = 0;

                let code = unsafe {
                    (syms.extract_preview)(c_path.as_ptr(), &mut out_buf, &mut out_len)
                };

                if code == 0 && !out_buf.is_null() && out_len > 0 {
                    let bytes = unsafe {
                        let slice = std::slice::from_raw_parts(out_buf, out_len);
                        let data = slice.to_vec();
                        (syms.free_buffer)(out_buf, out_len);
                        data
                    };

                    let mime = detect_image_mime(&bytes);
                    return Ok((bytes, mime));
                }
            }
        }

        // 2. 独立実行バイナリ（CLI）フォールバック: preview-pipe サブコマンド試行
        if let Some(ref exe) = self.exe_path {
            if exe.is_file() {
                // まず標準出力パイプ（preview-pipe）を試行
                let pipe_res = std::process::Command::new(exe)
                    .arg("preview-pipe")
                    .arg(path)
                    .output();

                if let Ok(output) = pipe_res {
                    if output.status.success() && !output.stdout.is_empty() {
                        let bytes = output.stdout;
                        let mime = detect_image_mime(&bytes);
                        return Ok((bytes, mime));
                    }
                }

                // パイプ非対応時は一時ファイル経由（preview）を試行
                let temp_output = std::env::temp_dir().join(format!(
                    "imv_plugin_preview_{}_{}.png",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_nanos())
                        .unwrap_or(0)
                ));

                let file_res = std::process::Command::new(exe)
                    .arg("preview")
                    .arg(path)
                    .arg(&temp_output)
                    .status();

                if let Ok(status) = file_res {
                    if status.success() && temp_output.is_file() {
                        let bytes_res = std::fs::read(&temp_output);
                        let _ = std::fs::remove_file(&temp_output);
                        if let Ok(bytes) = bytes_res {
                            let mime = detect_image_mime(&bytes);
                            return Ok((bytes, mime));
                        }
                    }
                }
                let _ = std::fs::remove_file(&temp_output);
            }
        }

        Err("プラグインによるプレビュー画像抽出に失敗しました".to_string())
    }
}

/// 相対または絶対パスからバイナリファイルの絶対パスを解決する
fn resolve_binary_path(plugin_dir: &Path, name_or_path: &str) -> PathBuf {
    let p = Path::new(name_or_path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        plugin_dir.join(p)
    }
}

/// 画像バイナリの先頭マジックバイトからMIMEタイプを自動判別する
fn detect_image_mime(bytes: &[u8]) -> String {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png".to_string()
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        "image/jpeg".to_string()
    } else if bytes.starts_with(b"RIFF") && bytes.len() >= 12 && &bytes[8..12] == b"WEBP" {
        "image/webp".to_string()
    } else {
        "image/png".to_string()
    }
}

// -------------------------------------------------------------------------------------------------
// 汎用ユーティリティ関数（バイナリスライス、外部コマンド、シグネチャ探索）
// -------------------------------------------------------------------------------------------------

/// 16進文字列またはASCII文字列をバイト列に変換する
pub fn parse_signature_bytes(s: &str) -> Vec<u8> {
    let trimmed = s.trim();
    if let Some(rest) = trimmed.strip_prefix("hex:") {
        if let Some(bytes) = decode_hex_str(rest.trim()) {
            return bytes;
        }
    }
    if let Some(rest) = trimmed.strip_prefix("0x") {
        if let Some(bytes) = decode_hex_str(rest.trim()) {
            return bytes;
        }
    }
    if let Some(rest) = trimmed.strip_prefix("ascii:") {
        return rest.as_bytes().to_vec();
    }
    if let Some(rest) = trimmed.strip_prefix("str:") {
        return rest.as_bytes().to_vec();
    }
    if trimmed.len() >= 4 && trimmed.len() % 2 == 0 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        if let Some(bytes) = decode_hex_str(trimmed) {
            return bytes;
        }
    }
    trimmed.as_bytes().to_vec()
}

fn decode_hex_str(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    let mut bytes = Vec::with_capacity(s.len() / 2);
    let chars: Vec<char> = s.chars().collect();
    for i in (0..chars.len()).step_by(2) {
        let hi = chars[i].to_digit(16)?;
        let lo = chars[i + 1].to_digit(16)?;
        bytes.push(((hi << 4) | lo) as u8);
    }
    Some(bytes)
}

/// ファイル内から指定シグネチャの位置をチャンク走査で検索する
pub fn find_signature_in_file(
    file: &mut File,
    start_pos: u64,
    target: &[u8],
) -> Result<Option<u64>, String> {
    if target.is_empty() {
        return Ok(Some(start_pos));
    }

    let chunk_size = (128 * 1024).max(target.len().saturating_mul(2));
    let overlap = target.len().saturating_sub(1);
    let mut buffer = vec![0u8; chunk_size + overlap];

    let mut cur_pos = start_pos;
    file.seek(SeekFrom::Start(cur_pos))
        .map_err(|e| format!("シーク失敗: {}", e))?;

    let mut carry_len = 0;

    loop {
        let read_len = file
            .read(&mut buffer[carry_len..carry_len + chunk_size])
            .map_err(|e| format!("ファイル読み込み失敗: {}", e))?;

        if read_len == 0 {
            break;
        }

        let valid_len = carry_len + read_len;
        let window = &buffer[..valid_len];

        if let Some(pos) = window.windows(target.len()).position(|w| w == target) {
            return Ok(Some(cur_pos - carry_len as u64 + pos as u64));
        }

        if valid_len >= overlap {
            buffer.copy_within(valid_len - overlap..valid_len, 0);
            carry_len = overlap;
            cur_pos += read_len as u64;
        } else {
            carry_len = valid_len;
            cur_pos += read_len as u64;
        }
    }

    Ok(None)
}

/// バイナリスライス（開始シグネチャと終端シグネチャによる切り出し）による画像抽出
pub fn extract_binary_slice(
    file: &mut File,
    start_sig: &str,
    end_sig: &str,
    include_end_sig: bool,
) -> Result<Vec<u8>, String> {
    let start_bytes = parse_signature_bytes(start_sig);
    let end_bytes = parse_signature_bytes(end_sig);

    let start_pos = find_signature_in_file(file, 0, &start_bytes)?
        .ok_or_else(|| "開始シグネチャが見つかりません".to_string())?;

    let end_pos = find_signature_in_file(file, start_pos + start_bytes.len() as u64, &end_bytes)?
        .ok_or_else(|| "終端シグネチャが見つかりません".to_string())?;

    let total_len = if include_end_sig {
        (end_pos + end_bytes.len() as u64) - start_pos
    } else {
        end_pos - start_pos
    };

    if total_len == 0 || total_len > 100 * 1024 * 1024 {
        return Err("切り出しバイナリサイズが不正です".to_string());
    }

    file.seek(SeekFrom::Start(start_pos))
        .map_err(|e| format!("ファイルシーク失敗: {}", e))?;

    let mut buf = vec![0u8; total_len as usize];
    file.read_exact(&mut buf)
        .map_err(|e| format!("バイナリ読み込み失敗: {}", e))?;

    Ok(buf)
}

/// 外部コマンド実行によるサムネイル/プレビュー画像抽出
pub fn extract_via_command(
    path: &Path,
    plugin_dir: &Path,
    command: &str,
    args: &[String],
    output_format: &str,
) -> Result<Vec<u8>, String> {
    let cmd_path = if Path::new(command).is_absolute() {
        PathBuf::from(command)
    } else {
        let relative_in_plugin = plugin_dir.join(command);
        if relative_in_plugin.is_file() {
            relative_in_plugin
        } else {
            PathBuf::from(command)
        }
    };

    let safe_ext: String = output_format
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    let safe_ext = if safe_ext.is_empty() { "png" } else { &safe_ext };

    let temp_output = std::env::temp_dir().join(format!(
        "imv_ext_{}_{}.{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
        safe_ext
    ));

    let mut final_args = Vec::new();
    let input_str = path.to_string_lossy().to_string();
    let output_str = temp_output.to_string_lossy().to_string();

    for arg in args {
        final_args.push(
            arg.replace("{input}", &input_str)
                .replace("{output}", &output_str),
        );
    }

    let status = std::process::Command::new(&cmd_path)
        .args(&final_args)
        .status()
        .map_err(|e| {
            let _ = std::fs::remove_file(&temp_output);
            format!("外部ツールの実行に失敗 ({}): {}", cmd_path.display(), e)
        })?;

    if !status.success() {
        let _ = std::fs::remove_file(&temp_output);
        return Err(format!("外部ツールがエラーコードを出力しました: {:?}", status.code()));
    }

    let read_result = std::fs::read(&temp_output)
        .map_err(|e| format!("抽出画像の読み込みに失敗: {}", e));
    let _ = std::fs::remove_file(&temp_output);

    read_result
}

/// PNGバイト列の IHDR チャンクから幅と高さをO(1)で抽出する
pub fn extract_dimensions_from_png(png_bytes: &[u8]) -> (Option<u32>, Option<u32>) {
    if png_bytes.len() >= 24 && &png_bytes[12..16] == b"IHDR" {
        let width = u32::from_be_bytes([png_bytes[16], png_bytes[17], png_bytes[18], png_bytes[19]]);
        let height = u32::from_be_bytes([png_bytes[20], png_bytes[21], png_bytes[22], png_bytes[23]]);
        (Some(width), Some(height))
    } else if let Ok(size) = imagesize::blob_size(png_bytes) {
        (Some(size.width as u32), Some(size.height as u32))
    } else {
        (None, None)
    }
}
