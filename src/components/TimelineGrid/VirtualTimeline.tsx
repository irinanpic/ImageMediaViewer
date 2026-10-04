import React, { useCallback, useEffect, useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { usePagedImages } from "../../hooks/usePagedImages";
import { useScrollVelocity } from "../../hooks/useScrollVelocity";
import { useTimelineLayout } from "../../hooks/useTimelineLayout";
import { useViewportPrioritizer } from "../../hooks/useViewportPrioritizer";
import { useAppStore } from "../../store";
import { DateHeader } from "./DateHeader";
import { GridCell } from "./GridCell";
import { Image } from "lucide-react";
import { TimelineScrubber } from "./TimelineScrubber";

/**
 * タイムライングリッドの仮想スクロール表示コンポーネント
 *
 * 変更理由: 仕様書§8.1およびユーザー要求（Ctrl+ホイール拡大縮小、矢印キーによる選択移動、Enterでのオープン）の実装。
 */
export const VirtualTimeline: React.FC = () => {
  const containerRef = useRef<HTMLDivElement>(null);
  const { rows, columns } = useTimelineLayout(containerRef);
  const isScrollingFast = useScrollVelocity(containerRef);
  const { getImageByIndex, requestIndices, versionTick } = usePagedImages();
  const cellSize = useAppStore((state) => state.cellSize);
  const setCellSize = useAppStore((state) => state.setCellSize);
  const totalImages = useAppStore((state) => state.totalImages);
  const selectedCellIndex = useAppStore((state) => state.selectedCellIndex);
  const setSelectedCellIndex = useAppStore((state) => state.setSelectedCellIndex);
  const openViewer = useAppStore((state) => state.openViewer);
  const isViewerOpen = useAppStore((state) => state.isViewerOpen);
  const timelineRefreshTick = useAppStore((state) => state.timelineRefreshTick);
  const refreshTimeline = useAppStore((state) => state.refreshTimeline);
  const targetScroll = useAppStore((state) => state.targetScroll);
  const setCurrentVisibleInfo = useAppStore((state) => state.setCurrentVisibleInfo);

  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => containerRef.current,
    estimateSize: (index) => rows[index]?.height ?? cellSize,
    overscan: 12,
  });

  const virtualItems = virtualizer.getVirtualItems();

  // 表示範囲および周辺の画像IDをバックエンドの世代管理キューへ優先通知
  useViewportPrioritizer({
    virtualItems,
    rows,
    getImageByIndex,
    overscanRows: 12,
    dataVersion: versionTick,
    refreshTick: timelineRefreshTick,
  });

  // 可視範囲＋overscan内の画像インデックスを算出してページ取得リクエスト
  useEffect(() => {
    if (virtualItems.length === 0) return;

    const indicesToFetch: number[] = [];
    for (const item of virtualItems) {
      const row = rows[item.index];
      if (row && row.kind === "cells") {
        for (let i = 0; i < row.count; i++) {
          indicesToFetch.push(row.startIndex + i);
        }
      }
    }

    if (indicesToFetch.length > 0) {
      requestIndices(indicesToFetch);
    }
  }, [virtualItems, rows, requestIndices]);

  // Ctrl + マウスホイールによるサムネイルサイズの無段階変更（ブラウザ全体のズームを完全抑止）
  useEffect(() => {
    const handleWheel = (e: WheelEvent) => {
      // ビューアが開いているときはビューア側の処理に任せる
      if (useAppStore.getState().isViewerOpen) return;

      if (e.ctrlKey) {
        e.preventDefault();
        e.stopPropagation();
        const delta = e.deltaY < 0 ? 16 : -16;
        const current = useAppStore.getState().cellSize;
        const next = Math.max(80, Math.min(320, current + delta));
        setCellSize(next);
      }
    };

    window.addEventListener("wheel", handleWheel, { passive: false });
    return () => window.removeEventListener("wheel", handleWheel);
  }, [setCellSize]);

  // 指定インデックスのセルが表示されるようスクロール追従
  const scrollToCell = useCallback(
    (targetIndex: number) => {
      const rowIndex = rows.findIndex(
        (r) => r.kind === "cells" && targetIndex >= r.startIndex && targetIndex < r.startIndex + r.count
      );
      if (rowIndex !== -1) {
        virtualizer.scrollToIndex(rowIndex, { align: "auto" });
      }
    },
    [rows, virtualizer]
  );

  // しおりジャンプ等による指定スクロール位置への移動
  useEffect(() => {
    if (!targetScroll || !containerRef.current) return;
    if (typeof targetScroll.rowIndex === "number" && targetScroll.rowIndex >= 0) {
      virtualizer.scrollToIndex(targetScroll.rowIndex, { align: "start" });
    } else if (typeof targetScroll.scrollTop === "number") {
      containerRef.current.scrollTo({ top: targetScroll.scrollTop, behavior: "smooth" });
    }
  }, [targetScroll, virtualizer]);

  // 現在の可視位置のメタ情報をストアへ同期（しおり保存用）
  useEffect(() => {
    if (virtualItems.length === 0 || !containerRef.current) return;
    const topItem = virtualItems[0];
    if (!topItem) return;
    const row = rows[topItem.index];
    if (!row) return;

    let dayLabel = "";
    let imageIndex: number | null = null;
    if (row.kind === "header") {
      dayLabel = row.day;
    } else {
      imageIndex = row.startIndex;
      // 上位の直近ヘッダー行の日付を探す
      for (let i = topItem.index; i >= 0; i--) {
        if (rows[i]?.kind === "header") {
          dayLabel = (rows[i] as { day: string }).day;
          break;
        }
      }
    }

    setCurrentVisibleInfo({
      scrollTop: containerRef.current.scrollTop,
      rowIndex: topItem.index,
      imageIndex,
      dayLabel: dayLabel || "タイムライン",
    });
  }, [virtualItems, rows, setCurrentVisibleInfo]);

  // 矢印キー（↑↓←→）によるセル選択とEnter/Spaceによるビューア起動
  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (isViewerOpen || totalImages === 0) return;
      if (["INPUT", "TEXTAREA", "SELECT"].includes((e.target as HTMLElement).tagName)) return;

      let nextIndex = selectedCellIndex;

      switch (e.key) {
        case "ArrowRight":
          e.preventDefault();
          nextIndex = selectedCellIndex === null ? 0 : Math.min(totalImages - 1, selectedCellIndex + 1);
          break;
        case "ArrowLeft":
          e.preventDefault();
          nextIndex = selectedCellIndex === null ? 0 : Math.max(0, selectedCellIndex - 1);
          break;
        case "ArrowDown":
          e.preventDefault();
          nextIndex = selectedCellIndex === null ? 0 : Math.min(totalImages - 1, selectedCellIndex + columns);
          break;
        case "ArrowUp":
          e.preventDefault();
          nextIndex = selectedCellIndex === null ? 0 : Math.max(0, selectedCellIndex - columns);
          break;
        case "Home":
          e.preventDefault();
          nextIndex = 0;
          break;
        case "End":
          e.preventDefault();
          nextIndex = totalImages - 1;
          break;
        case "r":
        case "R":
        case "F5":
          e.preventDefault();
          refreshTimeline();
          return;
        case "Enter":
        case " ":
          e.preventDefault();
          if (selectedCellIndex !== null) {
            const rec = getImageByIndex(selectedCellIndex);
            if (rec) {
              openViewer(rec.id, selectedCellIndex);
            }
          }
          return;
        default:
          return;
      }

      if (nextIndex !== null && nextIndex !== selectedCellIndex) {
        setSelectedCellIndex(nextIndex);
        scrollToCell(nextIndex);
      }
    },
    [
      isViewerOpen,
      totalImages,
      selectedCellIndex,
      columns,
      getImageByIndex,
      openViewer,
      setSelectedCellIndex,
      scrollToCell,
    ]
  );

  if (totalImages === 0) {
    return (
      <div className="flex-1 flex flex-col items-center justify-center text-textSecondary select-none p-8">
        <div className="w-16 h-16 rounded-full bg-surface flex items-center justify-center mb-4">
          <Image className="w-8 h-8 text-textSecondary/60" />
        </div>
        <p className="text-base font-medium text-textPrimary">画像が見つかりません</p>
        <p className="text-sm mt-1 text-center max-w-sm">
          サイドバーの「フォルダを追加」から、画像が存在するフォルダを登録してください。
        </p>
      </div>
    );
  }

  return (
    <div className="flex-1 relative h-full overflow-hidden flex flex-col">
      <div
        ref={containerRef}
        tabIndex={0}
        onKeyDown={handleKeyDown}
        className="flex-1 overflow-y-auto overflow-x-hidden relative h-full bg-background select-none outline-none focus:ring-1 focus:ring-accent/40"
      >
        <div
          style={{
            height: `${virtualizer.getTotalSize()}px`,
            width: "100%",
            position: "relative",
          }}
        >
          {virtualItems.map((virtualRow) => {
            const row = rows[virtualRow.index];
            if (!row) return null;

            return (
              <div
                key={virtualRow.key}
                style={{
                  position: "absolute",
                  top: 0,
                  left: 0,
                  width: "100%",
                  height: `${virtualRow.size}px`,
                  transform: `translateY(${virtualRow.start}px)`,
                }}
              >
                {row.kind === "header" ? (
                  <DateHeader day={row.day} count={row.count} />
                ) : (
                  <div className="flex flex-wrap gap-1 px-4 pr-7">
                    {Array.from({ length: row.count }).map((_, cellIdx) => {
                      const globalIdx = row.startIndex + cellIdx;
                      const record = getImageByIndex(globalIdx);

                      return (
                        <GridCell
                          key={globalIdx}
                          record={record}
                          globalIndex={globalIdx}
                          isScrollingFast={isScrollingFast}
                          size={cellSize}
                        />
                      );
                    })}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      </div>

      {/* 画面右側の高速スクラブバー (Picasaスタイル) */}
      <TimelineScrubber
        containerRef={containerRef}
        rows={rows}
        totalImages={totalImages}
      />
    </div>
  );
};
