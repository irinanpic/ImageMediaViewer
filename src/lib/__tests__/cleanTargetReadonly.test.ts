import { describe, expect, it } from "vitest";
import fs from "fs";
import path from "path";
import os from "os";
import { execSync } from "child_process";
// @ts-expect-error scripts配下のNode用ESMスクリプトを単体テストで直接インポートして検証
import { clearReadOnlyAttributes, cleanAllTargetReadOnly } from "../../../scripts/clean-target-readonly.mjs";

/**
 * Windows NTFS ReadOnly 属性解除機能の単体テスト
 */
describe("clean-target-readonly 単体テスト", () => {
  it("存在しないディレクトリに対しても例外を投げずに安全に完了すること", () => {
    const nonExistent = path.join(os.tmpdir(), "non-existent-dir-" + Date.now());
    expect(() => clearReadOnlyAttributes(nonExistent)).not.toThrow();
    expect(() => clearReadOnlyAttributes("")).not.toThrow();
  });

  it("ディレクトリおよび配下ファイルに ReadOnly 属性が付加されていた場合、正常に解除できること", () => {
    const testDir = path.join(os.tmpdir(), "readonly-test-" + Date.now());
    fs.mkdirSync(testDir, { recursive: true });
    const testFile = path.join(testDir, "test.txt");
    fs.writeFileSync(testFile, "hello");

    if (process.platform === "win32") {
      // 一旦 ReadOnly 属性を付加
      execSync(`cmd.exe /c attrib +r "${path.join(testDir, "*")}" /s /d`, { stdio: "ignore" });

      // 解除関数を実行
      clearReadOnlyAttributes(testDir);

      // 解除後に書き込み可能であるか検証
      expect(() => {
        fs.appendFileSync(testFile, " appended");
      }).not.toThrow();
    }

    // クリーンアップ
    try {
      if (process.platform === "win32") {
        execSync(`cmd.exe /c attrib -r "${path.join(testDir, "*")}" /s /d`, { stdio: "ignore" });
      }
      fs.rmSync(testDir, { recursive: true, force: true });
    } catch (_) {}
  });

  it("cleanAllTargetReadOnly が対象ディレクトリ群に対してエラーなく安全に実行されること", () => {
    const testDir = path.join(os.tmpdir(), "readonly-batch-test-" + Date.now());
    fs.mkdirSync(testDir, { recursive: true });
    try {
      expect(() => cleanAllTargetReadOnly([testDir])).not.toThrow();
    } finally {
      fs.rmSync(testDir, { recursive: true, force: true });
    }
  });
});
