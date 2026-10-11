import { describe, expect, it } from "vitest";
import { getFileName } from "../path";

/**
 * パス操作ユーティリティの単体テスト
 */
describe("getFileName 単体テスト", () => {
  it("Windows 形式のバックスラッシュ区切りパスからファイル名を抽出できること", () => {
    expect(getFileName("C:\\Users\\test\\Pictures\\photo.jpg")).toBe("photo.jpg");
    expect(getFileName("D:\\Images\\subfolder\\canvas.PNG")).toBe("canvas.PNG");
  });

  it("Unix / macOS / Android 形式のスラッシュ区切りパスからファイル名を抽出できること", () => {
    expect(getFileName("/home/user/Pictures/vacation.webp")).toBe("vacation.webp");
    expect(getFileName("/sdcard/DCIM/Camera/IMG_20261011.jpg")).toBe("IMG_20261011.jpg");
  });

  it("スラッシュとバックスラッシュが混在するパスでも正しくファイル名を抽出できること", () => {
    expect(getFileName("C:/Users/test\\Pictures/mixed_path.png")).toBe("mixed_path.png");
  });

  it("ファイル名単体または空文字列・null の場合も安全に処理できること", () => {
    expect(getFileName("single_file.jpg")).toBe("single_file.jpg");
    expect(getFileName("")).toBe("");
    expect(getFileName(null)).toBe("");
    expect(getFileName(undefined)).toBe("");
  });
});
