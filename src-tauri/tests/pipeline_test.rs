use image::{Rgba, RgbaImage};
use image_media_viewer_lib::pipeline::exif::extract_metadata;
use image_media_viewer_lib::pipeline::hash::calculate_quick_hash;
use image_media_viewer_lib::pipeline::thumbnail::{calculate_thumbnail_dimensions, generate_thumbnail};
use std::fs::{self, File};
use std::io::Write;
use std::time::UNIX_EPOCH;
use tempfile::tempdir;

#[test]
fn test_thumbnail_dimensions_aspect_ratio_and_limits() {
    // 1. 通常横長 (4000x3000) -> 短辺256px基準 (341x256)
    let (w1, h1) = calculate_thumbnail_dimensions(4000, 3000);
    assert_eq!(h1, 256);
    assert_eq!(w1, 341);

    // 2. 超パノラマ横長 (2560x100) -> 短辺256にすると長辺が6553pxになってしまうため、長辺最大512px制限 (512x20)
    let (w2, h2) = calculate_thumbnail_dimensions(2560, 100);
    assert_eq!(w2, 512);
    assert_eq!(h2, 20);

    // 3. 小さい画像 (100x80) -> 拡大はしない (100x80)
    let (w3, h3) = calculate_thumbnail_dimensions(100, 80);
    assert_eq!(w3, 100);
    assert_eq!(h3, 80);
}

#[test]
fn test_immutability_and_pipeline_robustness() {
    let dir = tempdir().unwrap();
    let cache_dir = dir.path().join("cache");
    fs::create_dir_all(&cache_dir).unwrap();

    // 1. 正常なPNG画像を作成
    let normal_img_path = dir.path().join("test_normal.png");
    let mut img = RgbaImage::new(400, 300);
    for x in 0..400 {
        for y in 0..300 {
            img.put_pixel(x, y, Rgba([x as u8, y as u8, 128, 255]));
        }
    }
    img.save(&normal_img_path).unwrap();

    // 元画像のハッシュと mtime を記録
    let size_before = fs::metadata(&normal_img_path).unwrap().len();
    let mtime_before = fs::metadata(&normal_img_path)
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let hash_before = calculate_quick_hash(&normal_img_path, size_before).unwrap();

    // メタデータ抽出
    let meta = extract_metadata(&normal_img_path, mtime_before as i64);
    assert_eq!(meta.width, Some(400));
    assert_eq!(meta.height, Some(300));

    // サムネイル生成
    let thumb_path = generate_thumbnail(&normal_img_path, &hash_before, 1, &cache_dir).unwrap();
    assert!(thumb_path.exists());

    // 【重要: 仕様書§2.3/§12 元ファイル不変テスト】
    // 走査・サムネ生成の前後でフィクスチャのハッシュとmtimeが変わらないことを確認
    let size_after = fs::metadata(&normal_img_path).unwrap().len();
    let mtime_after = fs::metadata(&normal_img_path)
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let hash_after = calculate_quick_hash(&normal_img_path, size_after).unwrap();

    assert_eq!(size_before, size_after, "ファイルサイズが変更されてはいけません");
    assert_eq!(mtime_before, mtime_after, "ファイルmtimeが変更されてはいけません");
    assert_eq!(hash_before, hash_after, "ファイル内容ハッシュが変更されてはいけません");

    // 2. 破損画像テスト（クラッシュせずErrを返すこと）
    let corrupted_path = dir.path().join("corrupted.jpg");
    let mut file = File::create(&corrupted_path).unwrap();
    file.write_all(b"\xFF\xD8\xFF\xE0\x00\x10JFIF\x00\x01\x01\x00\x00\x01\x00\x01\x00\x00GARBAGE_DATA_TRUNCATED")
        .unwrap();
    file.flush().unwrap();

    let corrupt_hash = calculate_quick_hash(&corrupted_path, 35).unwrap();
    let corrupt_res = generate_thumbnail(&corrupted_path, &corrupt_hash, 1, &cache_dir);
    assert!(
        corrupt_res.is_err(),
        "破損画像に対してパニックせず安全にエラーを返さなければなりません"
    );
}
