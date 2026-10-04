import { useEffect, useMemo, useState } from "react";
import { buildRows, calculateColumns, DEFAULT_LAYOUT_OPTIONS, type TimelineRow } from "../lib/buildRows";
import { useAppStore } from "../store";

/**
 * タイムラインの行レイアウトを管理するカスタムフック
 *
 * 変更理由: 仕様書§8.1「コンテナ幅から列数を決定し、日別バケットから全行の高さとインデックス範囲を確定する」
 *
 * @param containerRef スクロールコンテナのRef
 * @returns { rows, columns, containerWidth }
 */
export function useTimelineLayout(containerRef: React.RefObject<HTMLElement | null>) {
  const [containerWidth, setContainerWidth] = useState(
    typeof window !== "undefined" ? window.innerWidth - 260 : 1000
  );
  const buckets = useAppStore((state) => state.buckets);
  const cellSize = useAppStore((state) => state.cellSize);

  useEffect(() => {
    let observer: ResizeObserver | null = null;
    let animationFrameId: number | null = null;

    const measureWidth = () => {
      const el = containerRef.current;
      if (el && el.clientWidth > 0) {
        setContainerWidth(el.clientWidth);
      } else if (typeof window !== "undefined") {
        // フォールバック: サイドバー幅(約240px〜260px)を引いたウィンドウ幅
        const sidebarWidth = useAppStore.getState().isSidebarOpen ? 260 : 0;
        setContainerWidth(Math.max(300, window.innerWidth - sidebarWidth));
      }
    };

    // DOM要素へのアタッチを確実に待機してResizeObserverを開始
    const attachObserver = () => {
      const el = containerRef.current;
      if (el) {
        measureWidth();
        observer = new ResizeObserver((entries) => {
          for (const entry of entries) {
            const width = entry.contentRect.width;
            if (width > 0) {
              setContainerWidth(width);
            }
          }
        });
        observer.observe(el);
      } else {
        // 次のフレームで再試行
        animationFrameId = requestAnimationFrame(attachObserver);
      }
    };

    attachObserver();

    // ウィンドウのリサイズイベントも併せて即時監視
    const handleWindowResize = () => {
      measureWidth();
    };
    window.addEventListener("resize", handleWindowResize);

    return () => {
      if (animationFrameId !== null) {
        cancelAnimationFrame(animationFrameId);
      }
      if (observer) {
        observer.disconnect();
      }
      window.removeEventListener("resize", handleWindowResize);
    };
  }, [containerRef]);

  // サイドバーの開閉時にも幅を再測定
  const isSidebarOpen = useAppStore((state) => state.isSidebarOpen);
  useEffect(() => {
    const timer = setTimeout(() => {
      const el = containerRef.current;
      if (el && el.clientWidth > 0) {
        setContainerWidth(el.clientWidth);
      }
    }, 100);
    return () => clearTimeout(timer);
  }, [isSidebarOpen, containerRef]);

  const columns = useMemo(() => {
    // 左右パディング（px-4 = 16px、スクラブバー用 pr-7 = 28px、マージン計48px）を差し引いた実効コンテンツ幅で計算
    const effectiveWidth = Math.max(cellSize, containerWidth - 48);
    return calculateColumns(effectiveWidth, cellSize, DEFAULT_LAYOUT_OPTIONS.gap);
  }, [containerWidth, cellSize]);

  const rows: TimelineRow[] = useMemo(() => {
    return buildRows(buckets, columns, {
      ...DEFAULT_LAYOUT_OPTIONS,
      cellSize,
    });
  }, [buckets, columns, cellSize]);

  return { rows, columns, containerWidth };
}
