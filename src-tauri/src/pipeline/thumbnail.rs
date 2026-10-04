use exif::{In, Tag};
use fast_image_resize as fir;
use image::{DynamicImage, GenericImageView, ImageReader};
use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};

const MAX_PIXELS: u64 = 200_000_000; // 2億ピクセル上限（デコード爆弾対策）
const TARGET_SHORT_EDGE: u32 = 256;
const MAX_LONG_EDGE: u32 = 512;
// 変更理由: ユーザー要求「速度のためであればサムネイルは多少品質が落ちても構わない。一方でクリック時は実ファイルに当たる」。
// WebP品質を60.0に設定することで、エンコード速度の向上およびデータサイズを約40%削減し、ディスクI/Oと転送を高速化。
const WEBP_QUALITY: f32 = 60.0;

/// Orientation (1-8) に従って DynamicImage を回転・反転する
fn apply_orientation(img: DynamicImage, orientation: u32) -> DynamicImage {
    match orientation {
        2 => img.fliph(),
        3 => img.rotate180(),
        4 => img.flipv(),
        5 => img.fliph().rotate90(),
        6 => img.rotate90(),
        7 => img.fliph().rotate270(),
        8 => img.rotate270(),
        _ => img,
    }
}

/// サムネイルの目標寸法（幅, 高さ）を算出する
///
/// 変更理由: 仕様書§5.4「短辺256px、長辺最大512pxに制限。拡大はしない」
pub fn calculate_thumbnail_dimensions(width: u32, height: u32) -> (u32, u32) {
    if width == 0 || height == 0 {
        return (1, 1);
    }

    let short_edge = width.min(height) as f64;
    let long_edge = width.max(height) as f64;

    let ratio_short = TARGET_SHORT_EDGE as f64 / short_edge;
    let ratio_long = MAX_LONG_EDGE as f64 / long_edge;

    // 縮小比率（拡大はしないため 1.0 を上限とする）
    let ratio = ratio_short.min(ratio_long).min(1.0);

    let target_w = ((width as f64) * ratio).round().max(1.0) as u32;
    let target_h = ((height as f64) * ratio).round().max(1.0) as u32;

    (target_w, target_h)
}

/// サムネイルファイルの保存先パスを計算する
///
/// 変更理由: 仕様書§5.4「{app_cache_dir}/thumbs/v1/{hash[0..2]}/{hash}.webp」
pub fn get_thumbnail_path(cache_dir: &Path, quick_hash: &str) -> PathBuf {
    let prefix = if quick_hash.len() >= 2 {
        &quick_hash[0..2]
    } else {
        "00"
    };
    cache_dir
        .join("thumbs")
        .join("v1")
        .join(prefix)
        .join(format!("{}.webp", quick_hash))
}

/// EXIF から内蔵サムネイル（JPEGバイト列）の抽出を試みる
///
/// 変更理由: デジタル一眼・スマートフォン等の撮影ファイルには IFD1 に 160x120〜640x480 の
/// サムネイルが埋め込まれている。これを直接抽出・利用することで、数十MB・数千万画素の元画像を
/// フルサイズ展開することなく、数ミリ秒でWebPを生成可能となり劇的な高速化と省メモリを実現する。
///
/// @param source_path 画像ファイルパス
/// @return 抽出成功時は内蔵JPEGバイト列
fn try_extract_exif_thumbnail(source_path: &Path) -> Option<Vec<u8>> {
    let file = File::open(source_path).ok()?;
    let mut buf_reader = BufReader::new(file);
    let exif = exif::Reader::new().read_from_container(&mut buf_reader).ok()?;

    let offset_field = exif.get_field(Tag::JPEGInterchangeFormat, In::THUMBNAIL)?;
    let len_field = exif.get_field(Tag::JPEGInterchangeFormatLength, In::THUMBNAIL)?;

    let offset = offset_field.value.get_uint(0)? as usize;
    let len = len_field.value.get_uint(0)? as usize;

    let buf = exif.buf();
    if offset + len <= buf.len() && len > 0 {
        Some(buf[offset..offset + len].to_vec())
    } else {
        None
    }
}

/// DynamicImage を指定 Orientation で回転し、目標サイズへ縮小リサイズして WebP エンコードする
///
/// @param img デコード済み画像
/// @param orientation Exif Orientation (1-8)
/// @return WebPバイナリデータ
fn resize_and_encode_webp(img: DynamicImage, orientation: u32) -> Result<webp::WebPMemory, String> {
    let oriented_img = apply_orientation(img, orientation);
    let (w, h) = oriented_img.dimensions();

    let (target_w, target_h) = calculate_thumbnail_dimensions(w, h);

    let rgba_img = oriented_img.to_rgba8();
    let src_image = fir::images::Image::from_vec_u8(
        w,
        h,
        rgba_img.into_raw(),
        fir::PixelType::U8x4,
    )
    .map_err(|e| format!("fir::Image作成失敗: {:?}", e))?;

    let mut dst_image = fir::images::Image::new(
        target_w,
        target_h,
        fir::PixelType::U8x4,
    );

    let mut resizer = fir::Resizer::new();
    resizer
        .resize(&src_image, &mut dst_image, None)
        .map_err(|e| format!("リサイズ処理失敗: {:?}", e))?;

    let encoder = webp::Encoder::from_rgba(dst_image.buffer(), target_w, target_h);
    Ok(encoder.encode(WEBP_QUALITY))
}

/// WebP バイナリデータをアトミックにディスクへ書き出す（.tmp 書き出し後に rename）
///
/// @param dest_path 最終保存先パス
/// @param quick_hash クイックハッシュ（一時ファイル名用）
/// @param webp_data WebPバイト列
fn save_webp_atomically(
    dest_path: &Path,
    quick_hash: &str,
    webp_data: &[u8],
) -> Result<PathBuf, String> {
    let parent_dir = dest_path.parent().ok_or("親ディレクトリ取得失敗")?;
    fs::create_dir_all(parent_dir).map_err(|e| format!("ディレクトリ作成失敗: {}", e))?;

    let tmp_path = parent_dir.join(format!("{}.tmp", quick_hash));
    fs::write(&tmp_path, webp_data).map_err(|e| format!("一時ファイル書き込み失敗: {}", e))?;

    if dest_path.exists() {
        fs::remove_file(dest_path).ok();
    }
    fs::rename(&tmp_path, dest_path).map_err(|e| format!("リネーム失敗: {}", e))?;

    Ok(dest_path.to_path_buf())
}

/// 単一画像のサムネイル WebP バイナリデータを生成する（メモリ上に直接生成）
///
/// 変更理由: SQLite BLOB（thumbnails.db）への直接格納を可能にし、
/// ファイルシステムの I/O オーバーヘッドを完全排除するため。
/// EXIF内蔵サムネイルを最優先で検出し、元画像のフルサイズ展開をスキップすることで
/// 巨大画像であっても数ミリ秒でWebPを生成可能にする。
///
/// @param source_path 元画像ファイルパス
/// @param orientation Exif Orientation (1-8)
/// @return 成功時は WebP バイナリデータ (Vec<u8>)
pub fn generate_thumbnail_bytes(
    source_path: &Path,
    orientation: u32,
) -> Result<Vec<u8>, String> {
    // 1. 高速化最優先: EXIF埋め込みサムネイルの抽出を試みる
    // デジタルカメラ等の撮影データは数MB〜数十MBの元画像フル展開を完全にスキップ可能
    if let Some(embedded_bytes) = try_extract_exif_thumbnail(source_path) {
        if let Ok(img) = image::load_from_memory(&embedded_bytes) {
            let (ew, eh) = img.dimensions();
            // 内蔵サムネイルが極小でなければ採用（幅または高さが80px以上）
            if ew >= 80 || eh >= 80 {
                let webp_memory = resize_and_encode_webp(img, orientation)?;
                return Ok(webp_memory.to_vec());
            }
        }
    }

    // 2. 内蔵サムネイルがない場合は元画像ファイルを開き制限付きリーダーで安全にデコード
    let file = File::open(source_path).map_err(|e| format!("ファイルオープン失敗: {}", e))?;
    let mut reader = ImageReader::new(BufReader::new(file))
        .with_guessed_format()
        .map_err(|e| format!("フォーマット推測失敗: {}", e))?;

    // デコード制限設定（デコード爆弾・メモリ枯渇対策）
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(20_000);
    limits.max_image_height = Some(20_000);
    limits.max_alloc = Some(256 * 1024 * 1024); // 256MB
    reader.limits(limits);

    let img = reader
        .decode()
        .map_err(|e| format!("デコード失敗: {}", e))?;

    // ピクセル数制限確認
    let (orig_w, orig_h) = img.dimensions();
    if (orig_w as u64) * (orig_h as u64) > MAX_PIXELS {
        return Err("ピクセル数上限（2億px）を超過しています".to_string());
    }

    // 3. fast_image_resize によるリサイズ & WebP エンコード
    let webp_memory = resize_and_encode_webp(img, orientation)?;
    Ok(webp_memory.to_vec())
}

/// 単一画像のサムネイルを生成し、アトミックにファイル保存する（後方互換用）
///
/// @param source_path 元画像ファイルパス
/// @param quick_hash クイックハッシュ
/// @param orientation Exif Orientation (1-8)
/// @param cache_dir キャッシュディレクトリ
/// @return 成功時は保存されたサムネイルファイルのパス
pub fn generate_thumbnail(
    source_path: &Path,
    quick_hash: &str,
    orientation: u32,
    cache_dir: &Path,
) -> Result<PathBuf, String> {
    let dest_path = get_thumbnail_path(cache_dir, quick_hash);
    if dest_path.exists() {
        return Ok(dest_path);
    }

    let webp_bytes = generate_thumbnail_bytes(source_path, orientation)?;
    save_webp_atomically(&dest_path, quick_hash, &webp_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_thumbnail_dimensions() {
        // 4000x3000 (短辺3000): 短辺256pxへ縮小 (256/3000 = 0.085333...) -> (341, 256)
        let (w, h) = calculate_thumbnail_dimensions(4000, 3000);
        assert_eq!(h, 256);
        assert_eq!(w, 341);

        // 6000x1000 (極端な横長): 長辺512px上限が優先される -> (512, 85)
        let (w2, h2) = calculate_thumbnail_dimensions(6000, 1000);
        assert_eq!(w2, 512);
        assert_eq!(h2, 85);

        // 小さい画像 (150x100): 拡大しない (1.0上限)
        let (w3, h3) = calculate_thumbnail_dimensions(150, 100);
        assert_eq!(w3, 150);
        assert_eq!(h3, 100);
    }

    #[test]
    fn test_get_thumbnail_path() {
        let cache_dir = Path::new("/cache");
        let path = get_thumbnail_path(cache_dir, "ab12cd34ef");
        assert_eq!(
            path,
            PathBuf::from("/cache/thumbs/v1/ab/ab12cd34ef.webp")
        );
    }

    #[test]
    fn test_generate_thumbnail_from_scratch() {
        let temp_dir = tempfile::tempdir().unwrap();
        let src_path = temp_dir.path().join("sample.png");
        let cache_dir = temp_dir.path().join("cache");

        // テスト画像作成 (800x600 PNG)
        let img = image::RgbaImage::new(800, 600);
        img.save(&src_path).unwrap();

        let thumb_path = generate_thumbnail(&src_path, "samplehash123", 1, &cache_dir).unwrap();
        assert!(thumb_path.exists());
        assert!(fs::metadata(&thumb_path).unwrap().len() > 0);

        // WebPバイナリ直接生成のテスト
        let bytes = generate_thumbnail_bytes(&src_path, 1).unwrap();
        assert!(!bytes.is_empty());
        // WebPヘッダ (RIFF....WEBP) を確認
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WEBP");
    }
}

