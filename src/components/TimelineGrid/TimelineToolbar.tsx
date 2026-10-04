import React from "react";
import { Plus, RotateCw, SlidersHorizontal } from "lucide-react";
import { BookmarkPopover } from "./BookmarkPopover";
import { ConnectionStatusIndicator } from "../Common/ConnectionStatusIndicator";
import { usePagedImages } from "../../hooks/usePagedImages";
import { useAppStore } from "../../store";
import type { TimelineSort } from "../../types/generated/TimelineSort";

/**
 * タイムライン表示ツールバー（表示順切替・ムードボード追加・現画面再読込・通信状態表示＆再接続）
 *
 * 変更理由: 上部サムネイルサイズスライダーを廃止してステータスバーへ一本化し、
 * 空いた上部右側に通信途絶検知・再接続（サーバー起動）コントローラーを配置する
 */
export const TimelineToolbar: React.FC = () => {
  const timelineSort = useAppStore((state) => state.timelineSort);
  const setTimelineSort = useAppStore((state) => state.setTimelineSort);
  const totalImages = useAppStore((state) => state.totalImages);
  const selectedCellIndex = useAppStore((state) => state.selectedCellIndex);
  const selectedImageIds = useAppStore((state) => state.selectedImageIds);
  const openAddToBoardModal = useAppStore((state) => state.openAddToBoardModal);
  const refreshTimeline = useAppStore((state) => state.refreshTimeline);
  const { getImageByIndex } = usePagedImages();

  const selectedRecord = selectedCellIndex !== null ? getImageByIndex(selectedCellIndex) : null;

  const sortOptions: { value: TimelineSort; label: string }[] = [
    { value: "folder", label: "📁 フォルダ順（Picasa流・シームレス）" },
    { value: "takenAtDesc", label: "📅 撮影日時（新しい順）" },
    { value: "takenAtAsc", label: "📅 撮影日時（古い順）" },
    { value: "nameAsc", label: "🔤 ファイル名（昇順）" },
    { value: "nameDesc", label: "🔤 ファイル名（降順）" },
  ];

  return (
    <div className="h-10 border-b border-border bg-surface flex items-center justify-between px-4 text-xs select-none z-10 shrink-0">
      {/* 左側: ソート切替セレクター */}
      <div className="flex items-center gap-2">
        <SlidersHorizontal className="w-3.5 h-3.5 text-textSecondary shrink-0" />
        <span className="text-textSecondary font-medium shrink-0">表示順:</span>
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
          {totalImages.toLocaleString()} 枚
        </span>

        {/* 現画面再読込ボタン: 読込不可画像のリトライとキューの即時リフレッシュ */}
        <button
          onClick={() => refreshTimeline()}
          className="flex items-center gap-1.5 bg-surfaceLight hover:bg-surfaceLight/80 text-textSecondary hover:text-textPrimary border border-border px-2.5 py-1 rounded text-xs font-medium transition ml-2 shadow-xs cursor-pointer active:scale-95"
          title="現画面の画像を再読み込み（読込不可の画像を再取得し、キューをリフレッシュ） [Rキー]"
        >
          <RotateCw className="w-3.5 h-3.5" />
          再読込
        </button>

        {/* 表示位置のしおり（ブックマーク）ポップオーバー */}
        <BookmarkPopover />

        {selectedImageIds.length > 0 ? (
          <button
            onClick={() => openAddToBoardModal(selectedImageIds)}
            className="flex items-center gap-1.5 bg-accent hover:bg-accent/90 text-white border border-accent/40 px-2.5 py-1 rounded text-xs font-medium transition ml-2 shadow-sm cursor-pointer"
            title={`${selectedImageIds.length}枚の画像をムードボードに資料として追加`}
          >
            <Plus className="w-3.5 h-3.5" />
            ボードに追加 ({selectedImageIds.length}枚)
          </button>
        ) : selectedRecord ? (
          <button
            onClick={() => openAddToBoardModal([selectedRecord.id])}
            className="flex items-center gap-1.5 bg-accent/20 hover:bg-accent text-accent hover:text-white border border-accent/40 px-2.5 py-1 rounded text-xs font-medium transition ml-2 shadow-sm cursor-pointer"
            title="選択中の画像をムードボードに資料として追加"
          >
            <Plus className="w-3.5 h-3.5" />
            ボードに追加
          </button>
        ) : null}
      </div>

      {/* 右側: 通信状態インジケータおよび再接続コントローラー */}
      <ConnectionStatusIndicator />
    </div>
  );
};
