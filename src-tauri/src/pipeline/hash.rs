use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const CHUNK_SIZE: usize = 64 * 1024; // 64KiB

/// quick_hash を算出する
///
/// 変更理由: 仕様書§5.3「BLAKE3( file_size(u64 LE) || 先頭64KiB || 中央64KiB || 末尾64KiB )」に準拠
///
/// @param path 対象ファイルパス
/// @param file_size ファイルサイズ（バイト）
/// @return BLAKE3ハッシュの16進数文字列
pub fn calculate_quick_hash(path: &Path, file_size: u64) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();

    // ファイルサイズをu64リトルエンディアンで書き込み
    hasher.update(&file_size.to_le_bytes());

    let threshold = (CHUNK_SIZE * 3) as u64;

    if file_size <= threshold {
        // 64KiB * 3 未満の場合はファイル全体を読む
        let mut buffer = Vec::with_capacity(file_size as usize);
        file.read_to_end(&mut buffer)?;
        hasher.update(&buffer);
    } else {
        let mut buf = [0u8; CHUNK_SIZE];

        // 1. 先頭 64KiB
        let bytes_read = file.read(&mut buf)?;
        hasher.update(&buf[..bytes_read]);

        // 2. 中央 64KiB
        let mid_offset = (file_size / 2).saturating_sub((CHUNK_SIZE / 2) as u64);
        file.seek(SeekFrom::Start(mid_offset))?;
        let bytes_read = file.read(&mut buf)?;
        hasher.update(&buf[..bytes_read]);

        // 3. 末尾 64KiB
        let end_offset = file_size.saturating_sub(CHUNK_SIZE as u64);
        file.seek(SeekFrom::Start(end_offset))?;
        let bytes_read = file.read(&mut buf)?;
        hasher.update(&buf[..bytes_read]);
    }

    Ok(hasher.finalize().to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_quick_hash_small_and_large_files() {
        let mut small = NamedTempFile::new().unwrap();
        small.write_all(b"Hello Antigravity").unwrap();
        small.flush().unwrap();
        let hash1 = calculate_quick_hash(small.path(), 17).unwrap();
        let hash2 = calculate_quick_hash(small.path(), 17).unwrap();
        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 64);
    }
}
