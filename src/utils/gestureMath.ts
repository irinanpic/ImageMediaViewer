/**
 * タッチジェスチャーおよびポインター操作用計算ユーティリティ
 *
 * 変更理由: Android実機およびエミュレータでのタッチ操作（ピンチズーム・パン・スワイプ）において、
 * 演算の正確性と境界条件のテスト可能性を保証し、画面枠外飛び出しやゼロ除算を防止するため。
 */

/**
 * 2点間のユークリッド距離を算出する
 *
 * @param x1 始点X
 * @param y1 始点Y
 * @param x2 終点X
 * @param y2 終点Y
 * @return 2点間の距離（ピクセル）
 */
export function calculateDistance(x1: number, y1: number, x2: number, y2: number): number {
  return Math.hypot(x2 - x1, y2 - y1);
}

/**
 * 2点間の中心座標を算出する
 *
 * @param x1 点1のX
 * @param y1 点1のY
 * @param x2 点2のX
 * @param y2 点2のY
 * @return 中心点のX, Y座標
 */
export function calculateCenter(
  x1: number,
  y1: number,
  x2: number,
  y2: number
): { x: number; y: number } {
  return {
    x: (x1 + x2) / 2,
    y: (y1 + y2) / 2,
  };
}

/**
 * ピンチ操作時のズーム倍率を算出する
 *
 * @param initialDistance 操作開始時の2点間距離
 * @param currentDistance 現在の2点間距離
 * @param initialZoom 操作開始時のズーム倍率
 * @param minZoom 最小ズーム倍率（デフォルト: 0.5）
 * @param maxZoom 最大ズーム倍率（デフォルト: 16.0）
 * @return クランプされた新ズーム倍率
 */
export function calculatePinchZoom(
  initialDistance: number,
  currentDistance: number,
  initialZoom: number,
  minZoom = 0.5,
  maxZoom = 16.0
): number {
  if (initialDistance <= 0) return initialZoom;
  const ratio = currentDistance / initialDistance;
  const targetZoom = initialZoom * ratio;
  return Math.max(minZoom, Math.min(maxZoom, targetZoom));
}

/**
 * ズーム操作に伴う中心点維持のパン座標を算出する
 *
 * @param focusX フォーカス中心点（コンテナ中央を原点とするX座標）
 * @param focusY フォーカス中心点（コンテナ中央を原点とするY座標）
 * @param prevZoom 変更前のズーム倍率
 * @param newZoom 変更後のズーム倍率
 * @param prevPanX 変更前のパンX座標
 * @param prevPanY 変更前のパンY座標
 * @return 新しいパン座標
 */
export function calculateZoomCenterPan(
  focusX: number,
  focusY: number,
  prevZoom: number,
  newZoom: number,
  prevPanX: number,
  prevPanY: number
): { panX: number; panY: number } {
  if (prevZoom <= 0) return { panX: prevPanX, panY: prevPanY };
  const panX = focusX - ((focusX - prevPanX) / prevZoom) * newZoom;
  const panY = focusY - ((focusY - prevPanY) / prevZoom) * newZoom;
  return { panX, panY };
}

/**
 * 画像がコンテナ表示領域外へ過剰に飛び出さないようパン座標を制限する
 *
 * @param panX 目的パンX座標
 * @param panY 目的パンY座標
 * @param zoom 現在のズーム倍率
 * @param containerWidth コンテナ幅
 * @param containerHeight コンテナ高さ
 * @return クランプ後のパン座標
 */
export function clampViewerPan(
  panX: number,
  panY: number,
  zoom: number,
  containerWidth: number,
  containerHeight: number
): { panX: number; panY: number } {
  if (zoom <= 1.05 || containerWidth <= 0 || containerHeight <= 0) {
    return { panX: 0, panY: 0 };
  }
  const maxPanX = (containerWidth * (zoom - 0.5)) / 2;
  const maxPanY = (containerHeight * (zoom - 0.5)) / 2;
  return {
    panX: Math.max(-maxPanX, Math.min(maxPanX, panX)),
    panY: Math.max(-maxPanY, Math.min(maxPanY, panY)),
  };
}

/**
 * タッチスワイプによる画像送り方向を判定する
 *
 * @param deltaX 水平移動量 (endX - startX)
 * @param deltaY 垂直移動量 (endY - startY)
 * @param threshold スワイプ判定の最小閾値（ピクセル、デフォルト: 50）
 * @return "next"（左スワイプ＝次へ） | "prev"（右スワイプ＝前へ） | null（判定なし）
 */
export function detectSwipeDirection(
  deltaX: number,
  deltaY: number,
  threshold = 50
): "next" | "prev" | null {
  // 水平移動が垂直移動の1.2倍以上かつ閾値を超えている場合のみ横スワイプと判定
  if (Math.abs(deltaX) < threshold || Math.abs(deltaX) < Math.abs(deltaY) * 1.2) {
    return null;
  }
  return deltaX < 0 ? "next" : "prev";
}
