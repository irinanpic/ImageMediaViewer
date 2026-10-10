import { describe, expect, it } from "vitest";
import { resolveIsCompact } from "../useResponsiveLayout";

describe("useResponsiveLayout - resolveIsCompact (レイアウト判定ロジック)", () => {
  it("Android/モバイル環境では画面幅に関わらず常に isCompact=true になること (物理画面サイズの考慮)", () => {
    // 画面幅が1920pxなどの高精細端末であっても、Androidなら常にモバイル版
    expect(resolveIsCompact("auto", true, 1920)).toBe(true);
    expect(resolveIsCompact("auto", true, 1080)).toBe(true);
    expect(resolveIsCompact("auto", true, 800)).toBe(true);
    expect(resolveIsCompact("auto", true, 360)).toBe(true);
  });

  it("PC環境では画面幅768px未満のときのみ isCompact=true になること", () => {
    expect(resolveIsCompact("auto", false, 600)).toBe(true);
    expect(resolveIsCompact("auto", false, 767)).toBe(true);
    expect(resolveIsCompact("auto", false, 768)).toBe(false);
    expect(resolveIsCompact("auto", false, 1200)).toBe(false);
  });

  it("手動設定 layoutMode='desktop' の場合、Android環境であっても大画面PCレイアウト(isCompact=false)にできること (将来のAndroid PC対応)", () => {
    expect(resolveIsCompact("desktop", true, 1200)).toBe(false);
    expect(resolveIsCompact("desktop", true, 600)).toBe(false);
  });

  it("手動設定 layoutMode='mobile' の場合、PC環境であってもモバイルUI(isCompact=true)を強制できること", () => {
    expect(resolveIsCompact("mobile", false, 1920)).toBe(true);
  });
});
