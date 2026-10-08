use chrono::{DateTime, Datelike, Local, NaiveDateTime, TimeZone, Utc};
use exif::{In, Reader, Tag};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use tracing::warn;

/// 解析されたメタデータ結果
#[derive(Debug, Clone)]
pub struct ExtractedMetadata {
    pub taken_at: i64,
    pub taken_at_source: i32, // 0: Exif, 1: mtime
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub orientation: u32,
    pub format: Option<String>,
}

/// Exifの日時文字列 (YYYY:MM:DD HH:MM:SS) をパースし、壁時計時刻をUTCとみなしたepoch秒を返す
///
/// 変更理由: 仕様書§5.1「Exifの日時は壁時計時刻としてそのままUTCとみなしてepoch秒化する」
fn parse_exif_date_string(s: &str) -> Option<i64> {
    let trimmed = s.trim();
    if trimmed.starts_with("0000:") || trimmed.is_empty() {
        return None;
    }

    let naive = NaiveDateTime::parse_from_str(trimmed, "%Y:%m:%d %H:%M:%S").ok()?;
    let year = naive.year();
    if !(1900..=2100).contains(&year) {
        return None;
    }

    Some(Utc.from_utc_datetime(&naive).timestamp())
}

/// ファイルのメタデータを抽出する
///
/// @param path 画像ファイルパス
/// @param mtime_secs ファイル更新日時のUNIX秒
/// @return ExtractedMetadata
pub fn extract_metadata(path: &Path, mtime_secs: i64) -> ExtractedMetadata {
    let mut orientation = 1u32;
    let mut exif_taken_at: Option<i64> = None;

    // 1. Exif解析 (kamadak-exif)
    if let Ok(file) = File::open(path) {
        let mut buf_reader = BufReader::new(file);
        if let Ok(reader) = Reader::new().read_from_container(&mut buf_reader) {
            // Orientation
            if let Some(field) = reader.get_field(Tag::Orientation, In::PRIMARY) {
                if let Some(val) = field.value.get_uint(0) {
                    if (1..=8).contains(&val) {
                        orientation = val;
                    }
                }
            }

            // 日時優先順位: DateTimeOriginal -> DateTimeDigitized -> DateTime
            let date_tags = [
                Tag::DateTimeOriginal,
                Tag::DateTimeDigitized,
                Tag::DateTime,
            ];

            for tag in date_tags {
                if let Some(field) = reader.get_field(tag, In::PRIMARY) {
                    let date_str = field.display_value().to_string();
                    if let Some(ts) = parse_exif_date_string(&date_str) {
                        exif_taken_at = Some(ts);
                        break;
                    }
                }
            }
        }
    }

    // 2. 寸法とフォーマットのヘッダ解析 (imagesize)
    let format = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());

    let (mut raw_w, mut raw_h) = match imagesize::size(path) {
        Ok(dim) => (Some(dim.width as u32), Some(dim.height as u32)),
        Err(e) => {
            warn!("Failed to parse image header: {:?}, error: {:?}", path, e);
            (None, None)
        }
    };

    // 3. Orientation に応じた幅・高さの入れ替え (Orientation 5-8 は入替)
    if orientation >= 5 && orientation <= 8 {
        std::mem::swap(&mut raw_w, &mut raw_h);
    }

    // 4. taken_at の確定 (Exif or mtimeフォールバック)
    let (taken_at, taken_at_source) = if let Some(ts) = exif_taken_at {
        (ts, 0)
    } else {
        // mtime をローカル壁時計時刻としてUTCに揃える
        let local_dt: DateTime<Local> = DateTime::from_timestamp(mtime_secs, 0)
            .map(|utc| utc.with_timezone(&Local))
            .unwrap_or_else(Local::now);
        let naive = local_dt.naive_local();
        let normalized_epoch = Utc.from_utc_datetime(&naive).timestamp();
        (normalized_epoch, 1)
    };

    ExtractedMetadata {
        taken_at,
        taken_at_source,
        width: raw_w,
        height: raw_h,
        orientation,
        format,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_exif_date_string() {
        assert_eq!(
            parse_exif_date_string("2024:06:15 14:30:00"),
            Some(1718461800)
        );
        // 不正値
        assert_eq!(parse_exif_date_string("0000:00:00 00:00:00"), None);
        assert_eq!(parse_exif_date_string("1899:12:31 23:59:59"), None);
    }
}
