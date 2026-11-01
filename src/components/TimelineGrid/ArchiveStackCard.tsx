import React, { useState } from "react";
import { FileArchive, Layers, Loader2 } from "lucide-react";
import { backendApi } from "../../lib/ipc";
import { getThumbnailUrl } from "../../lib/thumbUrl";
import { useTranslation } from "../../locales";
import { useAppStore } from "../../store";
import type { FolderTreeNode } from "../../types/filter";

export interface ArchiveStackCardProps {
  /** 対象アーカイブのツリーノード */
  node: FolderTreeNode;
  /** 所属する監視フォルダID */
  folderId: number;
  /** タイルの表示サイズ（幅・高さ px） */
  size?: number;
}

/**
 * タイムライン一覧でアーカイブ（ZIP/CBZ等）を複数画像が積み上がった見た目で表現するスタックカード
 *
 * 変更理由: 仕様書§1.2およびユーザー要求「一覧にも複数画像が積み上がった見た目のアイコンで表示」に準拠。
 * 複数枚の写真が奥から手前に重なり合っている3Dスタックデザインを提供し、
 * 表紙サムネイルまたはアーカイブアイコンと枚数バッジを表示。
 * クリックでアーカイブをオンデマンド展開して中身へ直接ドリルダウンできる。
 *
 * @param props ArchiveStackCardProps
 */
export const ArchiveStackCard: React.FC<ArchiveStackCardProps> = ({
  node,
  folderId,
  size = 160,
}) => {
  const { t } = useTranslation();
  const setSelectedFolderId = useAppStore((state) => state.setSelectedFolderId);
  const updateFilterState = useAppStore((state) => state.updateFilterState);
  const setCurrentView = useAppStore((state) => state.setCurrentView);
  const refreshTimeline = useAppStore((state) => state.refreshTimeline);
  const timelineSort = useAppStore((state) => state.timelineSort);
  const setTimelineSort = useAppStore((state) => state.setTimelineSort);
  const setFolderHierarchy = useAppStore((state) => state.setFolderHierarchy);

  const [isLoading, setIsLoading] = useState(false);
  const [thumbError, setThumbError] = useState(false);

  // 対象画像が変わった場合はエラー状態をリセット
  React.useEffect(() => {
    setThumbError(false);
  }, [node.coverImageId]);

  const ext = node.name.split(".").pop()?.toUpperCase() || "ZIP";
  const thumbUrl = node.coverImageId
    ? getThumbnailUrl(node.coverImageId, node.coverRev ?? 1)
    : null;

  const handleClick = async (e: React.MouseEvent) => {
    e.stopPropagation();
    if (isLoading) return;

    setIsLoading(true);
    try {
      await backendApi.openArchiveFolder(folderId, node.path);
      // アーカイブ閲覧時はページ番号順（ファイル名昇順）に自動切り替え
      if (timelineSort !== "nameAsc" && timelineSort !== "nameDesc") {
        setTimelineSort("nameAsc");
      }
      // 最新のツリー情報を再取得してストアを更新
      const updatedTree = await backendApi.getFolderHierarchy(folderId);
      setFolderHierarchy(folderId, updatedTree);

      setSelectedFolderId(folderId);
      updateFilterState({ subFolderPath: node.path });
      setCurrentView("timeline");
      refreshTimeline();
    } catch (err) {
      console.error("アーカイブ展開エラー:", err);
    } finally {
      setIsLoading(false);
    }
  };

  return (
    <div
      onClick={handleClick}
      style={{ width: `${size}px`, height: `${size}px` }}
      className="relative select-none cursor-pointer group p-2 shrink-0 animate-in fade-in zoom-in-95 duration-200"
      title={`${node.name}\n${
        node.imageCount !== null
          ? `${node.imageCount} 枚`
          : t("sidebar.archiveClickToOpen")
      }`}
    >
      {/* 背面カード 2（最深層・右へ少し傾いた写真カード） */}
      <div className="absolute inset-2 bg-surfaceLight/60 rounded-xl border border-border/40 rotate-4 translate-x-2 -translate-y-1.5 shadow-xs transition-transform duration-300 ease-out group-hover:rotate-6 group-hover:translate-x-3 group-hover:-translate-y-2 pointer-events-none" />

      {/* 背面カード 1（中間層・左へ少し傾いた写真カード） */}
      <div className="absolute inset-2 bg-surfaceLight/80 rounded-xl border border-border/60 -rotate-2 -translate-x-1.5 -translate-y-1 shadow-md transition-transform duration-300 ease-out group-hover:-rotate-4 group-hover:-translate-x-2.5 group-hover:-translate-y-1.5 pointer-events-none" />

      {/* 最前面メインカード（表紙画像またはアーカイブスタックアイコン） */}
      <div className="relative w-full h-full bg-surface rounded-xl border border-border/80 shadow-lg overflow-hidden flex flex-col justify-between group-hover:border-accent group-hover:shadow-accent/25 transition-all duration-300 group-hover:scale-[1.02]">
        {/* 背景サムネイル画像（展開済みで1枚目が存在し、かつ読込可能な場合） */}
        {thumbUrl && !thumbError ? (
          <img
            src={thumbUrl}
            alt={node.name}
            onError={() => setThumbError(true)}
            className="absolute inset-0 w-full h-full object-cover transition-transform duration-500 group-hover:scale-105 pointer-events-none"
          />
        ) : (
          <div className="absolute inset-0 bg-gradient-to-br from-amber-500/15 via-surfaceLight/60 to-surface flex flex-col items-center justify-center pointer-events-none">
            <div className="relative">
              <Layers className="w-10 h-10 text-amber-400/70 transition-transform duration-300 group-hover:scale-110" />
              <FileArchive className="w-5 h-5 text-amber-500 absolute -bottom-1 -right-1" />
            </div>
          </div>
        )}

        {/* グラデーションオーバーレイ（テキスト視認性確保） */}
        <div className="absolute inset-0 bg-gradient-to-t from-black/85 via-black/30 to-black/60 pointer-events-none" />

        {/* 上部ヘッダー（拡張子バッジ & 画像枚数バッジ） */}
        <div className="relative z-10 flex items-center justify-between p-2">
          <span className="px-1.5 py-0.5 rounded text-[10px] font-bold font-mono bg-amber-500 text-black shadow-xs">
            {ext}
          </span>

          <span
            className={`px-2 py-0.5 rounded-full text-[10px] font-semibold flex items-center gap-1 shadow-xs backdrop-blur-xs ${
              node.imageCount !== null
                ? "bg-black/60 text-amber-300 border border-amber-500/40"
                : "bg-amber-500/20 text-amber-200 border border-amber-500/30"
            }`}
          >
            <Layers className="w-3 h-3 text-amber-400" />
            {node.imageCount !== null ? `${node.imageCount}枚` : "開く"}
          </span>
        </div>

        {/* ローディングスピナー（展開処理中） */}
        {isLoading && (
          <div className="absolute inset-0 z-20 bg-black/60 backdrop-blur-xs flex flex-col items-center justify-center text-amber-400 gap-1.5">
            <Loader2 className="w-6 h-6 animate-spin" />
            <span className="text-[11px] font-medium text-white">展開中...</span>
          </div>
        )}

        {/* 下部フッター（アーカイブファイル名） */}
        <div className="relative z-10 p-2">
          <p className="text-xs font-semibold text-white truncate drop-shadow-md">
            {node.name}
          </p>
          <p className="text-[10px] text-white/70 truncate flex items-center gap-1 mt-0.5">
            <FileArchive className="w-3 h-3 shrink-0 text-amber-400" />
            <span>クリックで開く</span>
          </p>
        </div>
      </div>
    </div>
  );
};
