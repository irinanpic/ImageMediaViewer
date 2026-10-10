import { useEffect, useRef } from "react";
import { useAppStore } from "../../store";
import {
  calculateCenter,
  calculateDistance,
  calculatePinchZoom,
  calculateZoomCenterPan,
  clampViewerPan,
  detectSwipeDirection,
} from "../../utils/gestureMath";

export interface ViewerGestureOptions {
  /** 次の画像へ進むコールバック（等倍時スワイプ送り用） */
  onNext?: () => void;
  /** 前の画像へ戻るコールバック（等倍時スワイプ送り用） */
  onPrev?: () => void;
}

/**
 * 画像ビューアのズーム、パンドラッグ、ピンチイン/アウト、およびスワイプ操作を処理するフック
 *
 * 変更理由: 仕様書§8.4およびAndroid/タッチデバイス対応。
 * マウス環境でのホイールズーム・ドラッグパン・ダブルクリック等倍トグルを完全維持しつつ、
 * モバイル・エミュレータ環境向けに以下の操作を完全対応する：
 * 1. 2本指タッチによる直感的なピンチズーム・パン
 * 2. ズーム時の1本指タッチドラッグパン
 * 3. 等倍表示時の左右フリック／スワイプによる画像送り
 * 4. ダブルタップによる原寸ピクセル等倍（1:1）と画面フィットのトグル
 *
 * @param containerRef 画像表示領域コンテナのRef
 * @param imgRef 表示中の画像要素Ref（原寸ピクセル等倍計算用）
 * @param options 前後画像切り替えコールバックなどの追加オプション
 */
export function useViewerGestures(
  containerRef: React.RefObject<HTMLDivElement | null>,
  imgRef?: React.RefObject<HTMLImageElement | null>,
  options?: ViewerGestureOptions
) {
  const transform = useAppStore((state) => state.transform);
  const isViewerOpen = useAppStore((state) => state.isViewerOpen);
  const setZoomAndPan = useAppStore((state) => state.setZoomAndPan);
  const setPan = useAppStore((state) => state.setPan);
  const resetTransform = useAppStore((state) => state.resetTransform);

  const transformRef = useRef(transform);
  transformRef.current = transform;

  const optionsRef = useRef(options);
  optionsRef.current = options;

  // マウスドラッグ用Ref
  const isDraggingMouse = useRef(false);
  const mouseDragStart = useRef({ x: 0, y: 0, panX: 0, panY: 0 });

  // タッチ操作用Ref
  const isPinchingRef = useRef(false);
  const pinchStartDistRef = useRef(0);
  const pinchStartZoomRef = useRef(1);
  const pinchStartCenterRef = useRef({ x: 0, y: 0 });
  const pinchStartPanRef = useRef({ panX: 0, panY: 0 });

  const isTouchDraggingRef = useRef(false);
  const touchStartPosRef = useRef({ x: 0, y: 0, time: 0, panX: 0, panY: 0 });
  const lastTapTimeRef = useRef(0);
  const lastTapPosRef = useRef({ x: 0, y: 0 });

  useEffect(() => {
    if (!isViewerOpen) return;

    const el = containerRef.current;
    if (!el) return;

    // 原寸ピクセル等倍ズーム率の算出
    const compute100PercentZoom = (): number => {
      const img = imgRef?.current;
      if (!el || !img || img.naturalWidth === 0 || img.naturalHeight === 0) {
        return 2.0;
      }
      const rect = el.getBoundingClientRect();
      const current = transformRef.current;
      const isRotated = current.rotation % 180 !== 0;
      const effW = isRotated ? img.naturalHeight : img.naturalWidth;
      const effH = isRotated ? img.naturalWidth : img.naturalHeight;
      const fitScale = Math.min(rect.width / effW, rect.height / effH);
      return fitScale > 0 ? Math.max(1.5, Math.min(16, 1.0 / fitScale)) : 2.0;
    };

    // ダブルクリック／ダブルタップ時の等倍トグル処理
    const handleToggleZoomAt = (clientX: number, clientY: number) => {
      const current = transformRef.current;
      if (current.zoom > 1.05) {
        resetTransform();
      } else {
        const rect = el.getBoundingClientRect();
        const cursorX = clientX - (rect.left + rect.width / 2);
        const cursorY = clientY - (rect.top + rect.height / 2);
        const targetZoom = compute100PercentZoom();
        const newPanX = cursorX - cursorX * targetZoom;
        const newPanY = cursorY - cursorY * targetZoom;
        setZoomAndPan(targetZoom, newPanX, newPanY);
      }
    };

    // 1. マウスホイールズーム処理
    const handleWheel = (e: WheelEvent) => {
      if ((e.target as HTMLElement)?.closest("button, input, select")) return;
      e.preventDefault();
      e.stopPropagation();

      const current = transformRef.current;
      const zoomFactor = e.deltaY < 0 ? 1.18 : 0.85;
      let newZoom = Math.max(0.5, Math.min(16, current.zoom * zoomFactor));

      if (newZoom <= 1.05) {
        newZoom = 1.0;
        setZoomAndPan(1.0, 0, 0);
        return;
      }

      const rect = el.getBoundingClientRect();
      const cursorX = e.clientX - (rect.left + rect.width / 2);
      const cursorY = e.clientY - (rect.top + rect.height / 2);

      const { panX, panY } = calculateZoomCenterPan(
        cursorX,
        cursorY,
        current.zoom,
        newZoom,
        current.panX,
        current.panY
      );
      const clamped = clampViewerPan(panX, panY, newZoom, rect.width, rect.height);
      setZoomAndPan(newZoom, clamped.panX, clamped.panY);
    };

    // 2. ダブルクリック
    const handleDoubleClick = (e: MouseEvent) => {
      if ((e.target as HTMLElement)?.closest("button, input, select")) return;
      handleToggleZoomAt(e.clientX, e.clientY);
    };

    // 3. マウスドラッグパン
    const handleMouseDown = (e: MouseEvent) => {
      if (e.button !== 0) return;
      if ((e.target as HTMLElement)?.closest("button, input, select")) return;

      const current = transformRef.current;
      if (current.zoom > 1.0) {
        isDraggingMouse.current = true;
        mouseDragStart.current = {
          x: e.clientX,
          y: e.clientY,
          panX: current.panX,
          panY: current.panY,
        };
        el.style.cursor = "grabbing";
      }
    };

    const handleMouseMove = (e: MouseEvent) => {
      if (!isDraggingMouse.current) return;
      const current = transformRef.current;
      const dx = e.clientX - mouseDragStart.current.x;
      const dy = e.clientY - mouseDragStart.current.y;
      const rect = el.getBoundingClientRect();
      const clamped = clampViewerPan(
        mouseDragStart.current.panX + dx,
        mouseDragStart.current.panY + dy,
        current.zoom,
        rect.width,
        rect.height
      );
      setPan(clamped.panX, clamped.panY);
    };

    const handleMouseUp = () => {
      isDraggingMouse.current = false;
      el.style.cursor = transformRef.current.zoom > 1.0 ? "grab" : "default";
    };

    // 4. タッチジェスチャー処理 (Touch Events: ピンチズーム、パン、スワイプ、ダブルタップ)
    const handleTouchStart = (e: TouchEvent) => {
      if ((e.target as HTMLElement)?.closest("button, input, select")) return;

      const current = transformRef.current;
      const now = Date.now();

      if (e.touches.length === 2) {
        // 2本指ピンチ開始
        isPinchingRef.current = true;
        isTouchDraggingRef.current = false;
        const t1 = e.touches[0];
        const t2 = e.touches[1];
        pinchStartDistRef.current = calculateDistance(t1.clientX, t1.clientY, t2.clientX, t2.clientY);
        pinchStartZoomRef.current = current.zoom;
        pinchStartCenterRef.current = calculateCenter(t1.clientX, t1.clientY, t2.clientX, t2.clientY);
        pinchStartPanRef.current = { panX: current.panX, panY: current.panY };
      } else if (e.touches.length === 1) {
        // 1本指タッチ開始
        const touch = e.touches[0];
        isTouchDraggingRef.current = true;
        isPinchingRef.current = false;
        touchStartPosRef.current = {
          x: touch.clientX,
          y: touch.clientY,
          time: now,
          panX: current.panX,
          panY: current.panY,
        };

        // ダブルタップ検知（300ms以内かつ移動量25px未満）
        const timeDiff = now - lastTapTimeRef.current;
        const distFromLastTap = calculateDistance(
          touch.clientX,
          touch.clientY,
          lastTapPosRef.current.x,
          lastTapPosRef.current.y
        );

        if (timeDiff > 40 && timeDiff < 320 && distFromLastTap < 25) {
          handleToggleZoomAt(touch.clientX, touch.clientY);
          lastTapTimeRef.current = 0; // リセット
        } else {
          lastTapTimeRef.current = now;
          lastTapPosRef.current = { x: touch.clientX, y: touch.clientY };
        }
      }
    };

    const handleTouchMove = (e: TouchEvent) => {
      if ((e.target as HTMLElement)?.closest("button, input, select")) return;

      if (isPinchingRef.current && e.touches.length >= 2) {
        e.preventDefault(); // ブラウザ標準のピンチズーム・スクロールを抑止
        const t1 = e.touches[0];
        const t2 = e.touches[1];
        const currentDist = calculateDistance(t1.clientX, t1.clientY, t2.clientX, t2.clientY);
        const newZoom = calculatePinchZoom(
          pinchStartDistRef.current,
          currentDist,
          pinchStartZoomRef.current,
          0.5,
          16.0
        );

        const currentCenter = calculateCenter(t1.clientX, t1.clientY, t2.clientX, t2.clientY);
        const centerDiffX = currentCenter.x - pinchStartCenterRef.current.x;
        const centerDiffY = currentCenter.y - pinchStartCenterRef.current.y;

        const rect = el.getBoundingClientRect();
        const focusX = pinchStartCenterRef.current.x - (rect.left + rect.width / 2);
        const focusY = pinchStartCenterRef.current.y - (rect.top + rect.height / 2);

        const { panX, panY } = calculateZoomCenterPan(
          focusX,
          focusY,
          pinchStartZoomRef.current,
          newZoom,
          pinchStartPanRef.current.panX,
          pinchStartPanRef.current.panY
        );

        const clamped = clampViewerPan(
          panX + centerDiffX,
          panY + centerDiffY,
          newZoom,
          rect.width,
          rect.height
        );
        setZoomAndPan(newZoom, clamped.panX, clamped.panY);
      } else if (isTouchDraggingRef.current && e.touches.length === 1) {
        const touch = e.touches[0];
        const dx = touch.clientX - touchStartPosRef.current.x;
        const dy = touch.clientY - touchStartPosRef.current.y;
        const current = transformRef.current;

        if (current.zoom > 1.05) {
          // 拡大中は画像内をパン移動（スクロールを抑止）
          e.preventDefault();
          const rect = el.getBoundingClientRect();
          const clamped = clampViewerPan(
            touchStartPosRef.current.panX + dx,
            touchStartPosRef.current.panY + dy,
            current.zoom,
            rect.width,
            rect.height
          );
          setPan(clamped.panX, clamped.panY);
        } else {
          // 等倍表示時は、横スワイプならブラウザの画面引っ張り・縦スクロールを防止
          if (Math.abs(dx) > Math.abs(dy) && Math.abs(dx) > 10) {
            e.preventDefault();
          }
        }
      }
    };

    const handleTouchEnd = (e: TouchEvent) => {
      if (isPinchingRef.current) {
        if (e.touches.length < 2) {
          isPinchingRef.current = false;
          // ピンチ終了時に1.05以下なら等倍にスナップ
          if (transformRef.current.zoom <= 1.05) {
            resetTransform();
          }
        }
      }

      if (isTouchDraggingRef.current && e.touches.length === 0) {
        isTouchDraggingRef.current = false;
        const current = transformRef.current;

        // 等倍表示時のスワイプによる画像送り判定
        if (current.zoom <= 1.05) {
          const changedTouch = e.changedTouches[0];
          if (changedTouch) {
            const dx = changedTouch.clientX - touchStartPosRef.current.x;
            const dy = changedTouch.clientY - touchStartPosRef.current.y;
            const direction = detectSwipeDirection(dx, dy, 45);
            if (direction === "next") {
              optionsRef.current?.onNext?.();
            } else if (direction === "prev") {
              optionsRef.current?.onPrev?.();
            }
          }
        }
      }
    };

    // イベント登録
    window.addEventListener("wheel", handleWheel, { passive: false });
    window.addEventListener("dblclick", handleDoubleClick);
    window.addEventListener("mousedown", handleMouseDown);
    window.addEventListener("mousemove", handleMouseMove);
    window.addEventListener("mouseup", handleMouseUp);

    // タッチイベントはコンテナ要素にアタッチ（passive: false でスクロール抑止を有効化）
    el.addEventListener("touchstart", handleTouchStart, { passive: false });
    window.addEventListener("touchmove", handleTouchMove, { passive: false });
    window.addEventListener("touchend", handleTouchEnd, { passive: false });
    window.addEventListener("touchcancel", handleTouchEnd, { passive: false });

    return () => {
      window.removeEventListener("wheel", handleWheel);
      window.removeEventListener("dblclick", handleDoubleClick);
      window.removeEventListener("mousedown", handleMouseDown);
      window.removeEventListener("mousemove", handleMouseMove);
      window.removeEventListener("mouseup", handleMouseUp);

      el.removeEventListener("touchstart", handleTouchStart);
      window.removeEventListener("touchmove", handleTouchMove);
      window.removeEventListener("touchend", handleTouchEnd);
      window.removeEventListener("touchcancel", handleTouchEnd);
    };
  }, [containerRef, imgRef, isViewerOpen, setZoomAndPan, setPan, resetTransform]);
}
