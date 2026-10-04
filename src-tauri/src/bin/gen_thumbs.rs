//! サムネイルキャッシュ用サンプルWebP生成ツール
//!
//! 変更理由: 仕様書§12対応。破損した32バイトWebPを置き換え、
//! ブラウザおよびデスクトップ上で視覚的に美しい256x256カラータイルWebPサムネイルを30種生成する。

use image::{Rgb, RgbImage};
use std::path::PathBuf;

fn main() {
    let localappdata = std::env::var("LOCALAPPDATA").expect("LOCALAPPDATA が未定義");
    let cache_dir = PathBuf::from(localappdata)
        .join("com.imagemediaviewer.app")
        .join("thumbs")
        .join("v1");

    println!("サムネイル保存先: {:?}", cache_dir);
    std::fs::create_dir_all(&cache_dir).expect("ディレクトリ作成失敗");

    // 30種類の異なるカラーパレットを定義
    let colors = [
        (220, 80, 80),   (80, 180, 220),  (80, 200, 120),  (230, 160, 50),
        (160, 90, 220),  (220, 100, 170), (70, 130, 230),  (100, 200, 190),
        (200, 120, 80),  (120, 180, 70),  (190, 80, 140),  (70, 180, 160),
        (220, 130, 130), (130, 170, 220), (140, 210, 150), (230, 190, 100),
        (180, 130, 230), (220, 140, 190), (120, 160, 220), (130, 210, 200),
        (210, 150, 110), (150, 200, 110), (200, 120, 170), (100, 190, 180),
        (230, 100, 100), (90, 190, 230),  (90, 210, 130),  (240, 170, 60),
        (170, 100, 230), (230, 110, 180),
    ];

    let width = 256u32;
    let height = 256u32;

    for i in 0..30 {
        let hash = format!("{:04x}", i).repeat(16);
        let prefix = &hash[0..2];
        let sub_dir = cache_dir.join(prefix);
        std::fs::create_dir_all(&sub_dir).expect("サブディレクトリ作成失敗");
        let dest = sub_dir.join(format!("{}.webp", hash));

        let (base_r, base_g, base_b) = colors[i % colors.len()];

        let mut img = RgbImage::new(width, height);
        for y in 0..height {
            for x in 0..width {
                // グラデーションとグリッド模様を付与
                let factor = (x as f32 / width as f32) * 0.4 + (y as f32 / height as f32) * 0.4;
                let r = ((base_r as f32 * (0.6 + factor)).clamp(0.0, 255.0)) as u8;
                let g = ((base_g as f32 * (0.6 + factor)).clamp(0.0, 255.0)) as u8;
                let b = ((base_b as f32 * (0.6 + factor)).clamp(0.0, 255.0)) as u8;

                // 境界線（フレーム効果）
                let is_border = x < 4 || x >= width - 4 || y < 4 || y >= height - 4;
                let pixel = if is_border {
                    Rgb([255, 255, 255])
                } else {
                    Rgb([r, g, b])
                };

                img.put_pixel(x, y, pixel);
            }
        }

        // WebP エンコード (品質 80)
        let rgba = image::DynamicImage::ImageRgb8(img).to_rgba8();
        let encoder = webp::Encoder::from_rgba(&rgba, width, height);
        let webp_data = encoder.encode(80.0);

        std::fs::write(&dest, &*webp_data).expect("WebP書き込み失敗");
        println!("生成完了 [{}/30]: {:?} ({} bytes)", i + 1, dest.file_name().unwrap(), webp_data.len());
    }

    println!("全30種のWebPサムネイル生成が正常に完了しました！");
}
