import fs from "fs";
import path from "path";

/**
 * Cargo の肥大化しやすい内部キャッシュディレクトリ名（再帰走査スキップ対象）
 *
 * 変更理由: `cmd.exe /c attrib /s /d` で target 全体を無条件走査すると、
 * incremental / deps / .fingerprint 配下の数万〜十数万ファイルに対して同期I/Oが発生し、
 * インストーラービルド (`npm run build:installer`) が数十分間フリーズする問題があったため。
 * ディレクトリ自体の ReadOnly 解除は行いつつ、これら内部キャッシュ配下のファイル列挙をスキップすることで
 * 数ミリ秒で確実に ReadOnly 属性解除を完了させる。
 */
const SKIP_RECURSE_DIRS = new Set(["incremental", ".fingerprint", "deps"]);

/**
 * 対象ディレクトリ配下のフォルダおよび主要ファイルから ReadOnly 属性を再帰的に解除する（Windows 専用）
 *
 * 変更理由: Windows NTFS において target ディレクトリ配下（特に build/<crate>/out 等）に
 * ReadOnly 属性が付加されると、Cargo の内部クレート（autocfg等）が
 * 「output path is not a writable directory」でパニック（exit code 101）を起こし、
 * Windows Installer および Android ビルド双方が失敗する問題を高速かつ確実に解消するため。
 *
 * @param {string} dir 解除対象のディレクトリパス
 */
export function clearReadOnlyAttributes(dir) {
  if (process.platform !== "win32" || !dir || !fs.existsSync(dir)) return;

  const stack = [dir];
  while (stack.length > 0) {
    const current = stack.pop();
    if (!current) continue;

    try {
      fs.chmodSync(current, 0o777);
    } catch (_) {}

    let entries;
    try {
      entries = fs.readdirSync(current, { withFileTypes: true });
    } catch (_) {
      continue;
    }

    for (const entry of entries) {
      const fullPath = path.join(current, entry.name);
      if (entry.isDirectory()) {
        try {
          fs.chmodSync(fullPath, 0o777);
        } catch (_) {}
        if (!SKIP_RECURSE_DIRS.has(entry.name)) {
          stack.push(fullPath);
        }
      } else {
        try {
          fs.chmodSync(fullPath, 0o666);
        } catch (_) {}
      }
    }
  }
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

    const pluginsSrcDir = path.join(projectRoot, "plugins-src");
    if (fs.existsSync(pluginsSrcDir)) {
      try {
        for (const entry of fs.readdirSync(pluginsSrcDir, { withFileTypes: true })) {
          if (entry.isDirectory()) {
            dirs.push(path.join(pluginsSrcDir, entry.name, "target"));
          }
        }
      } catch (_) {}
    }

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
