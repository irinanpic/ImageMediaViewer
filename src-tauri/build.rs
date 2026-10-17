/**
 * ビルドスクリプトのエントリポイント
 *
 * 変更理由: クロスコンパイル環境（Windowsホスト上でAndroid向けビルドを行う場合など）において、
 * ターゲットがAndroid Clang等の非MSVCツールチェーンである場合にWindows用MSVCリンカー引数が
 * 渡されてビルドエラーとなるのを防止・抑止する。
 * `CARGO_CFG_TARGET_OS` および `CARGO_CFG_TARGET_ENV` 環境変数を参照し、
 * ターゲットOSがWindowsかつMSVC環境の場合にのみマニフェスト依存フラグを出力する。
 */
fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();

    if target_os == "windows" && target_env == "msvc" {
        println!(r#"cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"#);
    }

    tauri_build::build();
}

