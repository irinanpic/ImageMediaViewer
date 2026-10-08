import React, { useState } from "react";
import { Plus, RotateCw, SlidersHorizontal } from "lucide-react";
import { BookmarkPopover } from "./BookmarkPopover";
import { ConnectionStatusIndicator } from "../Common/ConnectionStatusIndicator";
import { backendApi } from "../../lib/ipc";
import { usePagedImages } from "../../hooks/usePagedImages";
import { useTranslation } from "../../locales";
import { useAppStore } from "../../store";
import type { TimelineSort } from "../../types/generated/TimelineSort";

/**
 * タイムライン表示ツールバー（表示順切替・ムードボード追加・現画面再読込・通信状態表示＆再接続）
 *
 * 変更理由: 上部サムネイルサイズスライダーを廃止してステータスバーへ一本化し、
 * 空いた上部右側に通信途絶検知・再接続（サーバー起動）コントローラーを配置する。
 * 再読み込みボタン押下時は最新サマリ同期とインクリメンタル差分マージをシームレスに連携。
 */
export const TimelineToolbar: React.FC = () => {
  const { t } = useTranslation();
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
    <div className="h-10 border-b border-border bg-surface flex items-center justify-between px-4 text-xs select-none z-10 shrink-0">
      {/* 左側: ソート切替セレクター */}
      <div className="flex items-center gap-2">
        <SlidersHorizontal className="w-3.5 h-3.5 text-textSecondary shrink-0" />
        <span className="text-textSecondary font-medium shrink-0">{t("timeline.sortOrder")}</span>
        <select
          value={timelineSort}
          onChange={(e) => setTimelineSort(e.target.value as TimelineSort)}
          className="bg-surfaceLight text-textPrimary border border-border rounded px-2.5 py-1 text-xs outline-none focus:border-accent cursor-pointer hover:bg-surfaceLight/80 transition"
        >
          {sortOptions.map((opt) => (
            <option key={opt.value} value={opt.value}>
              {opt.label}
            </option>
          ))}
        </select>
        <span className="text-textSecondary/80 ml-2">
          {t("timeline.imageCount", { count: totalImages.toLocaleString() })}
        </span>

        {/* 現画面再読込ボタン: 既存表示を保ったまま差分・更新・失敗画像をインクリメンタル更新 */}
        <button
          onClick={handleRefresh}
          disabled={isRefreshing}
          className="flex items-center gap-1.5 bg-surfaceLight hover:bg-surfaceLight/80 text-textSecondary hover:text-textPrimary border border-border px-2.5 py-1 rounded text-xs font-medium transition ml-2 shadow-xs cursor-pointer active:scale-95 disabled:opacity-60"
          title={t("timeline.refreshTooltip")}
        >
          <RotateCw className={`w-3.5 h-3.5 ${isRefreshing ? "animate-spin text-accent" : ""}`} />
          {t("timeline.refreshBtn")}
        </button>

        {/* 表示位置のしおり（ブックマーク）ポップオーバー */}
        <BookmarkPopover />

        {selectedImageIds.length > 0 ? (
          <button
            onClick={() => openAddToBoardModal(selectedImageIds)}
            className="flex items-center gap-1.5 bg-accent hover:bg-accent/90 text-white border border-accent/40 px-2.5 py-1 rounded text-xs font-medium transition ml-2 shadow-sm cursor-pointer"
            title={t("timeline.addToBoardSelectedTooltip", { count: selectedImageIds.length })}
          >
            <Plus className="w-3.5 h-3.5" />
            {t("timeline.addToBoardSelected", { count: selectedImageIds.length })}
          </button>
        ) : selectedRecord ? (
          <button
            onClick={() => openAddToBoardModal([selectedRecord.id])}
            className="flex items-center gap-1.5 bg-accent/20 hover:bg-accent text-accent hover:text-white border border-accent/40 px-2.5 py-1 rounded text-xs font-medium transition ml-2 shadow-sm cursor-pointer"
            title={t("timeline.addToBoardCurrentTooltip")}
          >
            <Plus className="w-3.5 h-3.5" />
            {t("timeline.addToBoardCurrent")}
          </button>
        ) : null}
      </div>

      {/* 右側: 通信状態インジケータおよび再接続コントローラー */}
      <ConnectionStatusIndicator />
    </div>
  );
};
