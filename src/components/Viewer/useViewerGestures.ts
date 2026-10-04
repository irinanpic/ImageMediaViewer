import { useEffect, useRef } from "react";
import { useAppStore } from "../../store";

/**
 * 画像ビューアのズームおよびパンドラッグ操作を処理するフック
 *
 * 変更理由: 仕様書§8.4「ズーム: ホイール、パン: ドラッグ、CSS transformでGPU合成」
 *
 * @param containerRef 画像表示領域コンテナのRef
 */
export function useViewerGestures(containerRef: React.RefObject<HTMLDivElement | null>) {
  const transform = useAppStore((state) => state.transform);
  const isViewerOpen = useAppStore((state) => state.isViewerOpen);
  const setZoomAndPan = useAppStore((state) => state.setZoomAndPan);
  const setPan = useAppStore((state) => state.setPan);
  const resetTransform = useAppStore((state) => state.resetTransform);

  const transformRef = useRef(transform);
  transformRef.current = transform;

  const isDragging = useRef(false);
  const dragStart = useRef({ x: 0, y: 0, panX: 0, panY: 0 });

  useEffect(() => {
    if (!isViewerOpen) return;

    // ホイールによるカーソル中心ズーム処理
    const handleWheel = (e: WheelEvent) => {
      // ツールバーのボタンなどへの操作は除外
      if ((e.target as HTMLElement)?.closest("button, input, select")) return;

      e.preventDefault();
      e.stopPropagation();

      const current = transformRef.current;
      const zoomFactor = e.deltaY < 0 ? 1.18 : 0.85;
      let newZoom = Math.max(0.5, Math.min(10, current.zoom * zoomFactor));

      const el = containerRef.current;
      let newPanX = 0;
      let newPanY = 0;

      // 等倍（1.0）付近またはそれ以下の場合は中央フィットにスナップ
      if (newZoom <= 1.05) {
        newZoom = 1.0;
        newPanX = 0;
        newPanY = 0;
      } else if (el) {
        // コンテナ中央を原点とするカーソル座標
        const rect = el.getBoundingClientRect();
        const cursorX = e.clientX - (rect.left + rect.width / 2);
        const cursorY = e.clientY - (rect.top + rect.height / 2);

        // カーソル下の画像座標を保つようにパン座標を調整
        newPanX = cursorX - ((cursorX - current.panX) / current.zoom) * newZoom;
        newPanY = cursorY - ((cursorY - current.panY) / current.zoom) * newZoom;

        // 画像が画面外へ完全に飛び出さないようパンを制限
        const maxPanX = (rect.width * (newZoom - 0.5)) / 2;
        const maxPanY = (rect.height * (newZoom - 0.5)) / 2;
        newPanX = Math.max(-maxPanX, Math.min(maxPanX, newPanX));
        newPanY = Math.max(-maxPanY, Math.min(maxPanY, newPanY));
      }

      setZoomAndPan(newZoom, newPanX, newPanY);
    };

    // ダブルクリックで等倍と拡大（2.0倍）をトグル
    const handleDoubleClick = (e: MouseEvent) => {
      if ((e.target as HTMLElement)?.closest("button, input, select")) return;

      const current = transformRef.current;
      if (current.zoom > 1.05) {
        resetTransform();
      } else {
        const el = containerRef.current;
        if (!el) return;
        const rect = el.getBoundingClientRect();
        const cursorX = e.clientX - (rect.left + rect.width / 2);
        const cursorY = e.clientY - (rect.top + rect.height / 2);
        const newZoom = 2.0;
        const newPanX = cursorX - cursorX * newZoom;
        const newPanY = cursorY - cursorY * newZoom;
        setZoomAndPan(newZoom, newPanX, newPanY);
      }
    };

    // マウスドラッグによるパン処理
    const handleMouseDown = (e: MouseEvent) => {
      if (e.button !== 0) return; // 左クリックのみ
      if ((e.target as HTMLElement)?.closest("button, input, select")) return;

      const current = transformRef.current;
      // ズーム中のみパン操作可能
      if (current.zoom > 1.0) {
        isDragging.current = true;
        dragStart.current = {
          x: e.clientX,
          y: e.clientY,
          panX: current.panX,
          panY: current.panY,
        };
        const el = containerRef.current;
        if (el) el.style.cursor = "grabbing";
      }
    };

    const handleMouseMove = (e: MouseEvent) => {
      if (!isDragging.current) return;
      const dx = e.clientX - dragStart.current.x;
      const dy = e.clientY - dragStart.current.y;
      setPan(dragStart.current.panX + dx, dragStart.current.panY + dy);
    };

    const handleMouseUp = () => {
      isDragging.current = false;
      const el = containerRef.current;
      if (el) {
        el.style.cursor = transformRef.current.zoom > 1.0 ? "grab" : "default";
      }
    };

    window.addEventListener("wheel", handleWheel, { passive: false });
    window.addEventListener("dblclick", handleDoubleClick);
    window.addEventListener("mousedown", handleMouseDown);
    window.addEventListener("mousemove", handleMouseMove);
    window.addEventListener("mouseup", handleMouseUp);

    return () => {
      window.removeEventListener("wheel", handleWheel);
      window.removeEventListener("dblclick", handleDoubleClick);
      window.removeEventListener("mousedown", handleMouseDown);
      window.removeEventListener("mousemove", handleMouseMove);
      window.removeEventListener("mouseup", handleMouseUp);
    };
  }, [containerRef, isViewerOpen, setZoomAndPan, setPan, resetTransform]);
}
