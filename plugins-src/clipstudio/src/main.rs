//! CLIP STUDIO PAINT (.clip) フォーマット拡張プラグイン 独立実行バイナリ
//!
//! 変更理由: C-ABI 動的ライブラリロードが制限される環境でのフォールバック実行、
//! およびコマンドラインからの直接検証・デバッグを可能にするため。

use std::env;
use std::io::{self, Write};
use std::path::Path;
use serde_json::json;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        std::process::exit(1);
    }

    let subcmd = &args[1];
    match subcmd.as_str() {
        "info" => {
            let info = json!({
                "id": "format.clipstudio",
                "name": "CLIP STUDIO PAINT (.clip) Support",
                "version": "1.0.0",
                "extensions": ["clip"]
            });
            println!("{}", info);
        }
        "metadata" => {
            if args.len() < 3 {
                eprintln!("エラー: 対象ファイルパスを指定してください");
                std::process::exit(1);
            }
            let path = Path::new(&args[2]);
            match imv_clipstudio::extract_clip_metadata(path) {
                Ok((width, height)) => {
                    let out = json!({
                        "width": width,
                        "height": height,
                        "format": "clip"
                    });
                    println!("{}", out);
                }
                Err(e) => {
                    eprintln!("メタデータ抽出エラー: {}", e);
                    std::process::exit(2);
                }
            }
        }
        "preview" => {
            if args.len() < 4 {
                eprintln!("エラー: 対象ファイルパスと出力先パスを指定してください");
                std::process::exit(1);
            }
            let input_path = Path::new(&args[2]);
            let output_path = Path::new(&args[3]);

            match imv_clipstudio::extract_clip_preview(input_path) {
                Ok(bytes) => {
                    if let Err(e) = std::fs::write(output_path, &bytes) {
                        eprintln!("出力ファイル書き込みエラー: {}", e);
                        std::process::exit(3);
                    }
                }
                Err(e) => {
                    eprintln!("プレビュー抽出エラー: {}", e);
                    std::process::exit(2);
                }
            }
        }
        "preview-pipe" => {
            if args.len() < 3 {
                eprintln!("エラー: 対象ファイルパスを指定してください");
                std::process::exit(1);
            }
            let input_path = Path::new(&args[2]);
            match imv_clipstudio::extract_clip_preview(input_path) {
                Ok(bytes) => {
                    let mut stdout = io::stdout().lock();
                    if let Err(e) = stdout.write_all(&bytes) {
                        eprintln!("標準出力書き込みエラー: {}", e);
                        std::process::exit(3);
                    }
                }
                Err(e) => {
                    eprintln!("プレビュー抽出エラー: {}", e);
                    std::process::exit(2);
                }
            }
        }
        _ => {
            eprintln!("未知のサブコマンド: {}", subcmd);
            print_usage();
            std::process::exit(1);
        }
    }
}

fn print_usage() {
    eprintln!("CLIP STUDIO PAINT (.clip) Plugin CLI");
    eprintln!("使用法:");
    eprintln!("  clipstudio_plugin info");
    eprintln!("  clipstudio_plugin metadata <file.clip>");
    eprintln!("  clipstudio_plugin preview <file.clip> <output.png>");
    eprintln!("  clipstudio_plugin preview-pipe <file.clip>");
}
