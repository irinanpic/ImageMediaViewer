import { execSync } from "child_process";
import fs from "fs";
import path from "path";

/**
 * 対象ディレクトリ配下の全ファイルおよびフォルダから ReadOnly 属性を再帰的に解除する（Windows 専用）
 *
 * 変更理由: Windows NTFS において target ディレクトリ配下に ReadOnly 属性が付加されると、
 * Cargo の内部クレート（autocfg等）が「output path is not a writable directory」
 * でパニック（exit code 101）を起こし、Windows Installer および Android ビルド双方が
 * 失敗する問題を根本から解消するため。高速かつ確実に全階層を解除できるよう native な attrib コマンドを使用する。
 *
 * @param {string} dir 解除対象のディレクトリパス
 */
export function clearReadOnlyAttributes(dir) {
  if (process.platform !== "win32" || !dir || !fs.existsSync(dir)) return;
  try {
    execSync(`cmd.exe /c attrib -r "${path.join(dir, "*")}" /s /d`, { stdio: "ignore" });
  } catch (_) {}
}

/**
 * プロジェクト内外のすべての Cargo target および Android build 出力先ディレクトリから
 * ReadOnly 属性を一括解除する
 *
 * @param {string[]} [customDirs] 対象ディレクトリ一覧（省略時はプロジェクト既定値を使用）
 */
export function cleanAllTargetReadOnly(customDirs) {
  if (process.platform !== "win32") return;

  let dirs = customDirs;
  if (!dirs) {
    const projectRoot = process.cwd();
    dirs = [
      path.join(projectRoot, "src-tauri", "target"),
      path.join(projectRoot, "src-tauri", "gen", "android", "app", "build"),
    ];

    const localAppData = process.env.LOCALAPPDATA || process.env.TEMP || "";
    if (localAppData) {
      dirs.push(path.join(localAppData, "ImageMediaViewer", "target"));
    }

    if (process.env.CARGO_TARGET_DIR) {
      dirs.push(process.env.CARGO_TARGET_DIR);
    }
  }

  // 重複パスを除去して安全かつ最小限の呼び出しで解除
  const uniqueDirs = Array.from(new Set(dirs.filter(Boolean)));
  for (const d of uniqueDirs) {
    clearReadOnlyAttributes(d);
  }
}

// スクリプトとして直接実行された場合の処理
if (process.argv[1] && process.argv[1].replace(/\\/g, "/").endsWith("clean-target-readonly.mjs")) {
  cleanAllTargetReadOnly();
}
