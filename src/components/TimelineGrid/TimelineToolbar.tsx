import React, { useState } from "react";
import { Menu, Plus, RotateCw, SlidersHorizontal } from "lucide-react";
import { BookmarkPopover } from "./BookmarkPopover";
import { ConnectionStatusIndicator } from "../Common/ConnectionStatusIndicator";
import { backendApi } from "../../lib/ipc";
import { usePagedImages } from "../../hooks/usePagedImages";
import { useResponsiveLayout } from "../../hooks/useResponsiveLayout";
import { useTranslation } from "../../locales";
import { useAppStore } from "../../store";
import type { TimelineSort } from "../../types/generated/TimelineSort";

/**
 * タイムライン表示ツールバー（表示順切替・ムードボード追加・現画面再読込・通信状態表示＆再接続）
 *
 * 変更理由: 上部サムネイルサイズスライダーを廃止してステータスバーへ一本化し、
 * 空いた上部右側に通信途絶検知・再接続（サーバー起動）コントローラーを配置する。
 * また、モバイル環境では左端にハンバーガーメニュー（≡）を配置し、サイドバードロワーを呼び出せるようにする。
 */
export const TimelineToolbar: React.FC = () => {
  const { t } = useTranslation();
  const { isCompact } = useResponsiveLayout();
  const isSidebarOpen = useAppStore((state) => state.isSidebarOpen);
  const toggleSidebar = useAppStore((state) => state.toggleSidebar);
  const timelineSort = useAppStore((state) => state.timelineSort);
  const setTimelineSort = useAppStore((state) => state.setTimelineSort);
  const totalImages = useAppStore((state) => state.totalImages);
  const selectedCellIndex = useAppStore((state) => state.selectedCellIndex);
  const selectedImageIds = useAppStore((state) => state.selectedImageIds);
  const openAddToBoardModal = useAppStore((state) => state.openAddToBoardModal);
  const refreshTimeline = useAppStore((state) => state.refreshTimeline);
  const selectedFolderId = useAppStore((state) => state.selectedFolderId);
  const setCatalogSummary = useAppStore((state) => state.setCatalogSummary);
  const { getImageByIndex } = usePagedImages();

  const [isRefreshing, setIsRefreshing] = useState(false);

  const selectedRecord = selectedCellIndex !== null ? getImageByIndex(selectedCellIndex) : null;

  const handleRefresh = async () => {
    if (isRefreshing) return;
    setIsRefreshing(true);
    try {
      // 1. 最新サマリを取得して総件数・バケットを最新同期
      const summary = await backendApi.getTimelineSummary(selectedFolderId ?? undefined, timelineSort);
      setCatalogSummary(summary.total, summary.buckets, summary.version);

      // 2. タイムライン再検証をキック（既存キャッシュを維持したまま差分マージ）
      refreshTimeline();

      // 3. 失敗サムネイルの再作成をバックグラウンド要求
      backendApi.rescanMissingThumbnails().catch(() => {});
    } catch (e) {
      console.error("再読み込みエラー:", e);
      refreshTimeline();
    } finally {
      setTimeout(() => setIsRefreshing(false), 500);
    }
  };

  const sortOptions: { value: TimelineSort; label: string }[] = [
    { value: "folder", label: t("timeline.sortFolder") },
    { value: "takenAtDesc", label: t("timeline.sortTakenAtDesc") },
    { value: "takenAtAsc", label: t("timeline.sortTakenAtAsc") },
    { value: "nameAsc", label: t("timeline.sortNameAsc") },
    { value: "nameDesc", label: t("timeline.sortNameDesc") },
  ];

  return (
    <div
      className="border-b border-border bg-surface flex items-center justify-between text-xs select-none z-10 shrink-0 min-h-10 px-3"
      style={{
        paddingTop: "var(--sat)",
        paddingLeft: "calc(0.75rem + var(--sal))",
        paddingRight: "calc(0.75rem + var(--sar))",
      }}
    >
      {/* 左側: ハンバーガーメニュー(モバイル時) + ソート切替 + 更新 */}
      <div className="flex items-center gap-1.5 flex-1 min-w-0">
        {/* モバイル時のドロワー開閉ハンバーガーボタン */}
        {isCompact && (
          <button
            onClick={toggleSidebar}
            className="p-1.5 text-textSecondary hover:text-textPrimary bg-surfaceLight/50 hover:bg-surfaceLight rounded-md transition mr-1 shrink-0 active:scale-95 cursor-pointer"
            title={isSidebarOpen ? t("statusBar.sidebarClose") : t("statusBar.sidebarOpen")}
          >
            <Menu className="w-4 h-4" />
          </button>
        )}

        <SlidersHorizontal className="w-3.5 h-3.5 text-textSecondary shrink-0 hidden sm:block" />
        {!isCompact && (
          <span className="text-textSecondary font-medium shrink-0">{t("timeline.sortOrder")}</span>
        )}
        <select
          value={timelineSort}
          onChange={(e) => setTimelineSort(e.target.value as TimelineSort)}
          className="bg-surfaceLight text-textPrimary border border-border rounded px-2 py-1 text-xs outline-none focus:border-accent cursor-pointer hover:bg-surfaceLight/80 transition max-w-[140px] truncate"
        >
          {sortOptions.map((opt) => (
            <option key={opt.value} value={opt.value}>
              {opt.label}
            </option>
          ))}
        </select>

        {!isCompact && (
          <span className="text-textSecondary/80 ml-1.5 shrink-0">
            {t("timeline.imageCount", { count: totalImages.toLocaleString() })}
          </span>
        )}

        {/* 現画面再読込ボタン */}
        <button
          onClick={handleRefresh}
          disabled={isRefreshing}
          className="flex items-center gap-1 bg-surfaceLight hover:bg-surfaceLight/80 text-textSecondary hover:text-textPrimary border border-border px-2 py-1 rounded text-xs font-medium transition shadow-xs cursor-pointer active:scale-95 disabled:opacity-60 shrink-0"
          title={t("timeline.refreshTooltip")}
        >
          <RotateCw className={`w-3.5 h-3.5 ${isRefreshing ? "animate-spin text-accent" : ""}`} />
          {!isCompact && <span>{t("timeline.refreshBtn")}</span>}
        </button>

        {/* 表示位置のしおり（ブックマーク）ポップオーバー */}
        <BookmarkPopover />

        {selectedImageIds.length > 0 ? (
          <button
            onClick={() => openAddToBoardModal(selectedImageIds)}
            className="flex items-center gap-1.5 bg-accent hover:bg-accent/90 text-white border border-accent/40 px-2.5 py-1 rounded text-xs font-medium transition ml-1 shadow-sm cursor-pointer shrink-0 truncate"
            title={t("timeline.addToBoardSelectedTooltip", { count: selectedImageIds.length })}
          >
            <Plus className="w-3.5 h-3.5 shrink-0" />
            <span className="truncate">{t("timeline.addToBoardSelected", { count: selectedImageIds.length })}</span>
          </button>
        ) : selectedRecord ? (
          <button
            onClick={() => openAddToBoardModal([selectedRecord.id])}
            className="flex items-center gap-1 bg-accent/20 hover:bg-accent text-accent hover:text-white border border-accent/40 px-2 py-1 rounded text-xs font-medium transition ml-1 shadow-sm cursor-pointer shrink-0"
            title={t("timeline.addToBoardCurrentTooltip")}
          >
            <Plus className="w-3.5 h-3.5 shrink-0" />
            {!isCompact && <span>{t("timeline.addToBoardCurrent")}</span>}
          </button>
        ) : null}
      </div>

      {/* 右側: 通信状態インジケータおよび再接続コントローラー */}
      <div className="shrink-0 ml-2">
        <ConnectionStatusIndicator />
      </div>
    </div>
  );
};
