import React, { useCallback, useEffect, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { usePagedImages } from "../../hooks/usePagedImages";
import { useScrollVelocity } from "../../hooks/useScrollVelocity";
import { useTimelineLayout } from "../../hooks/useTimelineLayout";
import { useViewportPrioritizer } from "../../hooks/useViewportPrioritizer";
import { useAppStore } from "../../store";
import { DateHeader } from "./DateHeader";
import { GridCell } from "./GridCell";
import { CheckSquare, Image, LayoutGrid, X } from "lucide-react";
import { TimelineScrubber } from "./TimelineScrubber";

/**
 * タイムライングリッドの仮想スクロール表示コンポーネント
 *
 * 変更理由: 仕様書§8.1およびユーザー要求（Ctrl+ホイール拡大縮小、矢印キーによる選択移動、Enterでのオープン、
 * マウスドラッグ矩形選択およびCtrl+クリックによる複数選択、上部ムードボード一括登録アクションバー）の実装。
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
  const selectedImageIds = useAppStore((state) => state.selectedImageIds);
  const setSelectedImageIds = useAppStore((state) => state.setSelectedImageIds);
  const clearSelectedImageIds = useAppStore((state) => state.clearSelectedImageIds);
  const openAddToBoardModal = useAppStore((state) => state.openAddToBoardModal);
  const openViewer = useAppStore((state) => state.openViewer);
  const isViewerOpen = useAppStore((state) => state.isViewerOpen);
  const timelineRefreshTick = useAppStore((state) => state.timelineRefreshTick);
  const refreshTimeline = useAppStore((state) => state.refreshTimeline);
  const targetScroll = useAppStore((state) => state.targetScroll);
  const setCurrentVisibleInfo = useAppStore((state) => state.setCurrentVisibleInfo);

  // ドラッグ矩形選択（ラバーバンド選択）用のステートとRef
  const [selectionBox, setSelectionBox] = useState<{
    startX: number;
    startY: number;
    currentX: number;
    currentY: number;
  } | null>(null);
  const isSelectingRef = useRef(false);
  const dragStartPosRef = useRef<{ x: number; y: number } | null>(null);
  const initialSelectedIdsRef = useRef<number[]>([]);

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

  // マウスドラッグ矩形選択（ラバーバンド選択）
  const handleTimelineMouseDown = useCallback(
    (e: React.MouseEvent) => {
      if (e.button !== 0 || isViewerOpen) return;
      const target = e.target as HTMLElement;
      // スクラブバーやボタンなどのクリックはドラッグ矩形選択から除外
      if (
        target.closest("[data-scrubber='true']") ||
        target.closest("button") ||
        target.closest("select") ||
        target.closest("input")
      ) {
        return;
      }

      dragStartPosRef.current = { x: e.clientX, y: e.clientY };
      isSelectingRef.current = false;
      initialSelectedIdsRef.current = e.ctrlKey || e.metaKey ? [...selectedImageIds] : [];
    },
    [isViewerOpen, selectedImageIds]
  );

  const handleTimelineMouseMove = useCallback(
    (e: React.MouseEvent) => {
      if (!dragStartPosRef.current || isViewerOpen) return;

      const dx = e.clientX - dragStartPosRef.current.x;
      const dy = e.clientY - dragStartPosRef.current.y;
      const dist = Math.hypot(dx, dy);

      // 6px以上移動した時点でドラッグ矩形選択モードへ突入
      if (!isSelectingRef.current && dist > 6) {
        isSelectingRef.current = true;
      }

      if (isSelectingRef.current) {
        const startX = dragStartPosRef.current.x;
        const startY = dragStartPosRef.current.y;
        const currentX = e.clientX;
        const currentY = e.clientY;

        setSelectionBox({ startX, startY, currentX, currentY });

        const boxLeft = Math.min(startX, currentX);
        const boxRight = Math.max(startX, currentX);
        const boxTop = Math.min(startY, currentY);
        const boxBottom = Math.max(startY, currentY);

        if (containerRef.current) {
          const cells = containerRef.current.querySelectorAll<HTMLElement>("[data-grid-cell='true']");
          const intersectedIds: number[] = [];

          cells.forEach((cell) => {
            const rect = cell.getBoundingClientRect();
            // AABB矩形交差判定
            const intersects = !(
              rect.right < boxLeft ||
              rect.left > boxRight ||
              rect.bottom < boxTop ||
              rect.top > boxBottom
            );

            if (intersects) {
              const imageIdStr = cell.dataset.imageId;
              if (imageIdStr) {
                const id = Number(imageIdStr);
                if (!isNaN(id)) {
                  intersectedIds.push(id);
                }
              }
            }
          });

          const combined = Array.from(
            new Set([...initialSelectedIdsRef.current, ...intersectedIds])
          );
          setSelectedImageIds(combined);
        }
      }
    },
    [isViewerOpen, setSelectedImageIds]
  );

  const handleTimelineMouseUp = useCallback(() => {
    isSelectingRef.current = false;
    dragStartPosRef.current = null;
    setSelectionBox(null);
  }, []);

  // グローバルなマウスアップ安全ガード
  useEffect(() => {
    const handleGlobalMouseUp = () => {
      if (dragStartPosRef.current || isSelectingRef.current) {
        handleTimelineMouseUp();
      }
    };

    window.addEventListener("mouseup", handleGlobalMouseUp);
    return () => window.removeEventListener("mouseup", handleGlobalMouseUp);
  }, [handleTimelineMouseUp]);

  // 矢印キー（↑↓←→）によるセル選択とEnter/Spaceによるビューア起動、Escでの選択解除
  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (isViewerOpen || totalImages === 0) return;
      if (["INPUT", "TEXTAREA", "SELECT"].includes((e.target as HTMLElement).tagName)) return;

      // Escキーで複数選択を解除
      if (e.key === "Escape") {
        if (selectedImageIds.length > 0) {
          e.preventDefault();
          clearSelectedImageIds();
          return;
        }
      }

      // Ctrl + A ですべて選択（現在ロード済み画像、または最大500件）
      if ((e.ctrlKey || e.metaKey) && (e.key === "a" || e.key === "A")) {
        e.preventDefault();
        const allLoadedIds: number[] = [];
        for (let i = 0; i < totalImages; i++) {
          const rec = getImageByIndex(i);
          if (rec) allLoadedIds.push(rec.id);
        }
        if (allLoadedIds.length > 0) {
          setSelectedImageIds(allLoadedIds);
        }
        return;
      }

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
      selectedImageIds,
      columns,
      getImageByIndex,
      openViewer,
      setSelectedCellIndex,
      setSelectedImageIds,
      clearSelectedImageIds,
      scrollToCell,
      refreshTimeline,
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
      {/* 画面上部の複数選択アクションバー（選択中のみ表示） */}
      {selectedImageIds.length > 0 && (
        <div className="absolute top-3 left-1/2 -translate-x-1/2 z-40 flex items-center gap-3 bg-surface/95 backdrop-blur-md px-4 py-2 rounded-xl border border-accent/60 shadow-2xl animate-in fade-in slide-in-from-top-3">
          <div className="flex items-center gap-2">
            <CheckSquare className="w-4 h-4 text-accent" />
            <span className="font-semibold text-xs text-textPrimary whitespace-nowrap">
              {selectedImageIds.length} 枚選択中
            </span>
          </div>

          <div className="h-4 w-px bg-border/50" />

          {/* ムードボード登録ボタン */}
          <button
            onClick={() => openAddToBoardModal(selectedImageIds)}
            className="flex items-center gap-1.5 bg-accent hover:bg-accent/90 text-white px-3.5 py-1.5 rounded-lg text-xs font-medium shadow-md shadow-accent/25 transition cursor-pointer active:scale-95 whitespace-nowrap"
            title="選択したすべての画像をムードボードに一括登録"
          >
            <LayoutGrid className="w-3.5 h-3.5" />
            ムードボードに登録
          </button>

          <div className="h-4 w-px bg-border/50" />

          {/* 選択解除ボタン */}
          <button
            onClick={clearSelectedImageIds}
            className="flex items-center gap-1 text-xs text-textSecondary hover:text-textPrimary px-2 py-1 rounded hover:bg-surfaceLight/50 transition cursor-pointer whitespace-nowrap"
            title="選択を解除 (Esc)"
          >
            <X className="w-3.5 h-3.5" />
            選択解除
          </button>
        </div>
      )}

      {/* ドラッグ矩形選択（ラバーバンド）ボックス描画 */}
      {selectionBox && (
        <div
          style={{
            position: "fixed",
            left: Math.min(selectionBox.startX, selectionBox.currentX),
            top: Math.min(selectionBox.startY, selectionBox.currentY),
            width: Math.abs(selectionBox.currentX - selectionBox.startX),
            height: Math.abs(selectionBox.currentY - selectionBox.startY),
            pointerEvents: "none",
            zIndex: 9999,
          }}
          className="border-2 border-accent bg-accent/20 rounded shadow-md pointer-events-none"
        />
      )}

      <div
        ref={containerRef}
        tabIndex={0}
        onKeyDown={handleKeyDown}
        onMouseDown={handleTimelineMouseDown}
        onMouseMove={handleTimelineMouseMove}
        onMouseUp={handleTimelineMouseUp}
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
