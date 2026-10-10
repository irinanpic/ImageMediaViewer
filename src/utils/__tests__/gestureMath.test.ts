import { describe, expect, it } from "vitest";
import {
  calculateCenter,
  calculateDistance,
  calculatePinchZoom,
  calculateZoomCenterPan,
  clampViewerPan,
  detectSwipeDirection,
} from "../gestureMath";

describe("gestureMath ユーティリティ", () => {
  it("calculateDistance: 2点間のユークリッド距離を正確に計算できること", () => {
    expect(calculateDistance(0, 0, 3, 4)).toBe(5);
    expect(calculateDistance(10, 20, 10, 20)).toBe(0);
  });

  it("calculateCenter: 2点間の中心座標を正確に計算できること", () => {
    const center = calculateCenter(10, 20, 30, 40);
    expect(center).toEqual({ x: 20, y: 30 });
  });

  it("calculatePinchZoom: 開始距離と現在距離からピンチズーム倍率を正確に算出すること", () => {
    // 距離が2倍になったらズームも2倍
    expect(calculatePinchZoom(100, 200, 1.0)).toBe(2.0);
    // 距離が半分になったらズームも半分
    expect(calculatePinchZoom(100, 50, 2.0)).toBe(1.0);
    // 上限・下限のクランプテスト
    expect(calculatePinchZoom(100, 3000, 1.0, 0.5, 16.0)).toBe(16.0);
    expect(calculatePinchZoom(100, 10, 1.0, 0.5, 16.0)).toBe(0.5);
    // ゼロ除算ガード
    expect(calculatePinchZoom(0, 100, 1.5)).toBe(1.5);
  });

  it("calculateZoomCenterPan: ズーム時のフォーカス中心点を維持するパン座標を算出すること", () => {
    // フォーカス中心が (0, 0) で以前のパンが (0, 0) の場合、ズームが変わってもパンは (0, 0)
    const pan = calculateZoomCenterPan(0, 0, 1.0, 2.0, 0, 0);
    expect(pan).toEqual({ panX: 0, panY: 0 });

    // 画面端 (100, 100) を中心にズームした場合の追従計算
    const panShift = calculateZoomCenterPan(100, 100, 1.0, 2.0, 0, 0);
    expect(panShift.panX).toBe(-100);
    expect(panShift.panY).toBe(-100);
  });

  it("clampViewerPan: コンテナサイズとズーム倍率に基づきパンを境界制限すること", () => {
    // 等倍（1.0）時はパンが強制的に 0 にリセットされること
    expect(clampViewerPan(50, 50, 1.0, 800, 600)).toEqual({ panX: 0, panY: 0 });

    // 2倍拡大時の最大制限
    const clamped = clampViewerPan(1000, 1000, 2.0, 800, 600);
    // maxPanX = (800 * 1.5) / 2 = 600
    // maxPanY = (600 * 1.5) / 2 = 450
    expect(clamped.panX).toBe(600);
    expect(clamped.panY).toBe(450);
  });

  it("detectSwipeDirection: 横スワイプ方向を正確に判定し、縦スクロールは除外すること", () => {
    // 左スワイプ（次へ）
    expect(detectSwipeDirection(-80, 10, 50)).toBe("next");
    // 右スワイプ（前へ）
    expect(detectSwipeDirection(80, -10, 50)).toBe("prev");
    // 閾値未満（移動量が小さい）
    expect(detectSwipeDirection(30, 5, 50)).toBeNull();
    // 縦方向の移動が大きい場合はスワイプと判定しない（スクロール保護）
    expect(detectSwipeDirection(60, 80, 50)).toBeNull();
  });
});
