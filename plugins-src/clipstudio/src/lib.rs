//! CLIP STUDIO PAINT (.clip) フォーマット拡張プラグイン
//!
//! 変更理由: アプリ本体コアから特定のソフトウェア形式（ClipStudio）およびSQLite BLOB走査ロジックを
//! 分離し、独立してビルド・配布・ロード可能なC-ABI動的ライブラリおよびCLIバイナリとして提供するため。

use std::collections::VecDeque;
use std::ffi::CStr;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::raw::c_char;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::UNIX_EPOCH;

/// 走査時の `extract_clip_metadata` と直後の `extract_clip_preview`（サムネイル生成）で
/// 同一 `.clip` ファイルの SQLite デシリアライズが2回走るのを防ぐための短命リングキャッシュ（最大8件）
const PREVIEW_CACHE_CAPACITY: usize = 8;

struct PreviewCacheEntry {
    path: PathBuf,
    file_size: u64,
    mtime_secs: i64,
    png_bytes: Vec<u8>,
}

static PREVIEW_CACHE: OnceLock<Mutex<VecDeque<PreviewCacheEntry>>> = OnceLock::new();

fn get_preview_cache() -> &'static Mutex<VecDeque<PreviewCacheEntry>> {
    PREVIEW_CACHE.get_or_init(|| Mutex::new(VecDeque::with_capacity(PREVIEW_CACHE_CAPACITY)))
}

/// マジックヘッダシグネチャ (CSFCHUNK)
pub const CSF_MAGIC: &[u8] = b"CSFCHUNK";

/// SQLiteチャンクシグネチャ (CHNKSQLi)
pub const SQLITE_CHUNK_SIG: &[u8] = b"CHNKSQLi";

/// SQLiteフォーマット3 マジックシグネチャ
pub const SQLITE_MAGIC_SIG: &[u8] = b"SQLite format 3\0";

/// PNG開始シグネチャ
pub const PNG_START_SIG: &[u8] = b"\x89PNG\r\n\x1a\n";

/// PNG終端シグネチャ (IEND)
pub const PNG_END_SIG: &[u8] = b"IEND\xae\x42\x60\x82";

/// ファイル内から指定バイト列の位置をチャンク走査で検索する
///
/// 変更理由: 大容量ファイル（数百MB〜GB）であってもメモリ消費を抑え、
/// 高速に目的のチャンクヘッダを探索するため。
///
/// @param file 走査対象ファイルハンドル
/// @param start_pos 走査開始オフセット
/// @param target 検索シグネチャバイト列
/// @return 見つかった場合は絶対ファイルオフセット
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

/// PNGバイト列の IHDR チャンクから幅と高さを抽出する
///
/// @param png_bytes PNG画像データ
/// @return (幅, 高さ)
pub fn extract_png_dimensions(png_bytes: &[u8]) -> Option<(u32, u32)> {
    if png_bytes.len() >= 24 && &png_bytes[12..16] == b"IHDR" {
        let width = u32::from_be_bytes([png_bytes[16], png_bytes[17], png_bytes[18], png_bytes[19]]);
        let height = u32::from_be_bytes([png_bytes[20], png_bytes[21], png_bytes[22], png_bytes[23]]);
        Some((width, height))
    } else {
        None
    }
}

/// .clip ファイルを検証し、内蔵されているプレビューPNGバイナリを抽出する
///
/// 変更理由: CLIP STUDIO PAINT形式内部のSQLiteデータベースからプレビューBLOBを抽出し、
/// 破損時や非標準形式でもシグネチャ探索フォールバックにより確実に画像を取り出すため。
/// また、直前のメタデータ抽出と直後のサムネイル生成で同一ファイルを2回連続デシリアライズしないよう、
/// `(path, file_size, mtime)` をキーとした最大8件のリングキャッシュで再利用する。
///
/// @param path .clip ファイルのパス
/// @return 抽出されたPNGバイナリ
pub fn extract_clip_preview(path: &Path) -> Result<Vec<u8>, String> {
    let mut file = File::open(path).map_err(|e| format!("ファイルオープン失敗: {}", e))?;

    let (file_size, mtime_secs) = match file.metadata() {
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
        if let Ok(cache) = get_preview_cache().lock() {
            if let Some(hit) = cache.iter().find(|e| {
                e.file_size == file_size && e.mtime_secs == mtime_secs && e.path == path
            }) {
                return Ok(hit.png_bytes.clone());
            }
        }
    }

    let store_in_cache = |bytes: &Vec<u8>| {
        if file_size > 0 && bytes.len() <= 16 * 1024 * 1024 {
            if let Ok(mut cache) = get_preview_cache().lock() {
                if cache.len() >= PREVIEW_CACHE_CAPACITY {
                    cache.pop_front();
                }
                cache.push_back(PreviewCacheEntry {
                    path: path.to_path_buf(),
                    file_size,
                    mtime_secs,
                    png_bytes: bytes.clone(),
                });
            }
        }
    };

    // 1. CSFCHUNK マジックヘッダ検証
    let mut header = [0u8; 8];
    file.read_exact(&mut header)
        .map_err(|e| format!("ヘッダ読み込み失敗: {}", e))?;
    if &header != CSF_MAGIC {
        return Err("無効な形式です（CSFCHUNKヘッダ不一致）".to_string());
    }

    let mut db_bytes_opt = None;

    // 2. CHNKSQLi チャンクの探索
    if let Ok(Some(pos)) = find_signature_in_file(&mut file, 0, SQLITE_CHUNK_SIG) {
        if file.seek(SeekFrom::Start(pos + SQLITE_CHUNK_SIG.len() as u64)).is_ok() {
            let mut len_bytes = [0u8; 8];
            if file.read_exact(&mut len_bytes).is_ok() {
                let chunk_len = u64::from_be_bytes(len_bytes);
                if chunk_len > 0 && chunk_len <= 100 * 1024 * 1024 {
                    let db_start = pos + 16; // 8byte sig + 8byte len
                    if file.seek(SeekFrom::Start(db_start)).is_ok() {
                        let mut buf = vec![0u8; chunk_len as usize];
                        if file.read_exact(&mut buf).is_ok() {
                            db_bytes_opt = Some(buf);
                        }
                    }
                }
            }
        }
    }

    // 3. フォールバック: SQLite format 3\0 シグネチャ探索
    if db_bytes_opt.is_none() {
        if let Ok(Some(pos)) = find_signature_in_file(&mut file, 0, SQLITE_MAGIC_SIG) {
            if file.seek(SeekFrom::Start(pos)).is_ok() {
                let mut head = [0u8; 100];
                if file.read_exact(&mut head).is_ok() {
                    let raw_page_size = u16::from_be_bytes([head[16], head[17]]);
                    let page_size: u64 = if raw_page_size == 1 { 65536 } else { raw_page_size as u64 };
                    let page_count = u32::from_be_bytes([head[28], head[29], head[30], head[31]]) as u64;

                    let db_len = if page_count > 0 && page_size >= 512 {
                        page_size * page_count
                    } else {
                        let file_len = if file_size > 0 {
                            file_size
                        } else {
                            file.metadata().map_err(|e| e.to_string())?.len()
                        };
                        file_len.saturating_sub(pos).min(100 * 1024 * 1024)
                    };

                    if file.seek(SeekFrom::Start(pos)).is_ok() {
                        let mut buf = vec![0u8; db_len as usize];
                        if file.read_exact(&mut buf).is_ok() {
                            db_bytes_opt = Some(buf);
                        }
                    }
                }
            }
        }
    }

    // 4. SQLite デシリアライズ & クエリ実行
    if let Some(db_bytes) = db_bytes_opt {
        let sz = db_bytes.len();
        let ptr = unsafe { rusqlite::ffi::sqlite3_malloc64(sz as u64) as *mut u8 };
        if !ptr.is_null() {
            unsafe {
                std::ptr::copy_nonoverlapping(db_bytes.as_ptr(), ptr, sz);
            }
            let owned_data = unsafe {
                rusqlite::serialize::OwnedData::from_raw_nonnull(std::ptr::NonNull::new(ptr).unwrap(), sz)
            };
            if let Ok(mut conn) = rusqlite::Connection::open_in_memory() {
                if conn.deserialize(rusqlite::DatabaseName::Main, owned_data, true).is_ok() {
                    if let Ok(mut stmt) = conn.prepare("SELECT ImageData FROM CanvasPreview LIMIT 1") {
                        if let Ok(blob) = stmt.query_row([], |row| row.get::<_, Vec<u8>>(0)) {
                            if !blob.is_empty() {
                                store_in_cache(&blob);
                                return Ok(blob);
                            }
                        }
                    }
                }
            }
        }
    }

    // 5. 破損時フォールバック: ファイル内部の PNG シグネチャ探索
    if let Ok(Some(start_pos)) = find_signature_in_file(&mut file, 0, PNG_START_SIG) {
        if let Ok(Some(end_pos)) = find_signature_in_file(&mut file, start_pos + PNG_START_SIG.len() as u64, PNG_END_SIG) {
            let total_len = (end_pos + PNG_END_SIG.len() as u64) - start_pos;
            if total_len > 0 && total_len <= 50 * 1024 * 1024 {
                if file.seek(SeekFrom::Start(start_pos)).is_ok() {
                    let mut buf = vec![0u8; total_len as usize];
                    if file.read_exact(&mut buf).is_ok() {
                        store_in_cache(&buf);
                        return Ok(buf);
                    }
                }
            }
        }
    }

    Err("ClipStudioファイルからのプレビュー画像抽出に失敗しました".to_string())
}

/// .clip ファイルから寸法（幅と高さ）を抽出する
///
/// @param path .clip ファイルのパス
/// @return (幅, 高さ)
pub fn extract_clip_metadata(path: &Path) -> Result<(u32, u32), String> {
    let png_bytes = extract_clip_preview(path)?;
    extract_png_dimensions(&png_bytes).ok_or_else(|| "PNG寸法の解析に失敗しました".to_string())
}

// -------------------------------------------------------------------------------------------------
// C-ABI エクスポート関数インターフェース
// -------------------------------------------------------------------------------------------------

/// プラグインの初期化とヘルスチェック
///
/// @return 0 (正常)
#[no_mangle]
pub unsafe extern "C" fn imv_plugin_init() -> i32 {
    0
}

/// メタデータ（画像寸法）を抽出する
///
/// @param path_utf8 ファイルパス（UTF-8 C文字列）
/// @param out_width 幅の出力先ポインタ
/// @param out_height 高さの出力先ポインタ
/// @return 0: 成功, -1: 引数エラー, -2: パス変換失敗, -3: 抽出失敗
#[no_mangle]
pub unsafe extern "C" fn imv_plugin_extract_metadata(
    path_utf8: *const c_char,
    out_width: *mut u32,
    out_height: *mut u32,
) -> i32 {
    let result = std::panic::catch_unwind(|| {
        if path_utf8.is_null() || out_width.is_null() || out_height.is_null() {
            return -1;
        }

        let c_str = unsafe { CStr::from_ptr(path_utf8) };
        let path_str = match c_str.to_str() {
            Ok(s) => s,
            Err(_) => return -2,
        };

        let path = Path::new(path_str);
        match extract_clip_metadata(path) {
            Ok((w, h)) => {
                unsafe {
                    *out_width = w;
                    *out_height = h;
                }
                0
            }
            Err(_) => -3,
        }
    });

    result.unwrap_or(-99)
}

/// プレビュー画像（PNGバイナリ）を抽出する
///
/// 変更理由: アプリ本体がメモリへ直接PNG画像を受け取り、WebP変換やビューワ表示へ受け渡すため。
/// 取得したバッファは `imv_plugin_free_buffer` で解放する。
///
/// @param path_utf8 ファイルパス（UTF-8 C文字列）
/// @param out_buf バッファポインタの出力先
/// @param out_len バッファ長の出力先
/// @return 0: 成功, -1: 引数エラー, -2: パス変換失敗, -3: 抽出失敗
#[no_mangle]
pub unsafe extern "C" fn imv_plugin_extract_preview(
    path_utf8: *const c_char,
    out_buf: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    let result = std::panic::catch_unwind(|| {
        if path_utf8.is_null() || out_buf.is_null() || out_len.is_null() {
            return -1;
        }

        let c_str = unsafe { CStr::from_ptr(path_utf8) };
        let path_str = match c_str.to_str() {
            Ok(s) => s,
            Err(_) => return -2,
        };

        let path = Path::new(path_str);
        match extract_clip_preview(path) {
            Ok(bytes) => {
                let len = bytes.len();
                let mut boxed_slice = bytes.into_boxed_slice();
                let ptr = boxed_slice.as_mut_ptr();
                std::mem::forget(boxed_slice);

                unsafe {
                    *out_buf = ptr;
                    *out_len = len;
                }
                0
            }
            Err(_) => -3,
        }
    });

    result.unwrap_or(-99)
}

/// プラグインによって確保されたバイナリバッファを解放する
///
/// @param buf 解放対象バッファのポインタ
/// @param len バッファサイズ
#[no_mangle]
pub unsafe extern "C" fn imv_plugin_free_buffer(buf: *mut u8, len: usize) {
    let _ = std::panic::catch_unwind(|| {
        if !buf.is_null() && len > 0 {
            let slice = unsafe { std::slice::from_raw_parts_mut(buf, len) };
            let _ = unsafe { Box::from_raw(slice) };
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tempfile::tempdir;

    fn create_test_clip_file(width: u32, height: u32) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(CSF_MAGIC);
        data.extend_from_slice(&0u64.to_be_bytes());

        let img = image::RgbaImage::new(width, height);
        let mut png_buf = Cursor::new(Vec::new());
        img.write_to(&mut png_buf, image::ImageFormat::Png).expect("PNGエンコード失敗");
        let png_bytes = png_buf.into_inner();

        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE CanvasPreview (ImageData BLOB);").unwrap();
        conn.execute("INSERT INTO CanvasPreview (ImageData) VALUES (?1)", [&png_bytes]).unwrap();

        let db_data = conn.serialize(rusqlite::DatabaseName::Main).unwrap();

        data.extend_from_slice(SQLITE_CHUNK_SIG);
        data.extend_from_slice(&(db_data.len() as u64).to_be_bytes());
        data.extend_from_slice(&db_data);

        data
    }

    #[test]
    fn test_extract_clip_preview_and_metadata() {
        let temp_dir = tempdir().unwrap();
        let clip_path = temp_dir.path().join("test.clip");
        let mock_data = create_test_clip_file(480, 320);
        std::fs::write(&clip_path, &mock_data).unwrap();

        let preview = extract_clip_preview(&clip_path).unwrap();
        assert!(preview.starts_with(PNG_START_SIG));

        let (w, h) = extract_clip_metadata(&clip_path).unwrap();
        assert_eq!(w, 480);
        assert_eq!(h, 320);
    }

    #[test]
    fn test_c_abi_functions() {
        let temp_dir = tempdir().unwrap();
        let clip_path = temp_dir.path().join("c_abi_test.clip");
        let mock_data = create_test_clip_file(640, 480);
        std::fs::write(&clip_path, &mock_data).unwrap();

        let path_c = std::ffi::CString::new(clip_path.to_str().unwrap()).unwrap();

        unsafe {
            assert_eq!(imv_plugin_init(), 0);

            let mut width = 0u32;
            let mut height = 0u32;
            let res = imv_plugin_extract_metadata(path_c.as_ptr(), &mut width, &mut height);
            assert_eq!(res, 0);
            assert_eq!(width, 640);
            assert_eq!(height, 480);

            let mut buf: *mut u8 = std::ptr::null_mut();
            let mut len: usize = 0;
            let res_prev = imv_plugin_extract_preview(path_c.as_ptr(), &mut buf, &mut len);
            assert_eq!(res_prev, 0);
            assert!(!buf.is_null());
            assert!(len > 0);

            let slice = std::slice::from_raw_parts(buf, len);
            assert!(slice.starts_with(PNG_START_SIG));

            imv_plugin_free_buffer(buf, len);
        }
    }
}
