import React from "react";
import { FolderOpen, X } from "lucide-react";
import { backendApi } from "../../lib/ipc";
import { useTranslation } from "../../locales";
import type { ImageDetail } from "../../types/generated/ImageDetail";
import { analyzeImageMetadata } from "../../utils/imageMetadata";

export interface ImageDetailPanelProps {
  /** 表示対象の画像詳細データ */
  detail: ImageDetail;
  /** パネルを閉じるコールバック（省略時は閉じるボタン非表示） */
  onClose?: () => void;
  /** タイトル（省略時はデフォルトタイトル） */
  title?: string;
  /** カスタムCSSクラス */
  className?: string;
}

/**
 * タイムライン詳細ビューアおよびムードボードで共通利用される画像詳細情報パネル
 *
 * 変更理由: 仕様要求「画像フォーマット情報、可逆/非可逆、ファイルサイズ/圧縮率情報を追加」および
 * 「タイムラインとムードボードどちらにも追加」に基づき、DRY原則に沿って共通パネル化
 */
export const ImageDetailPanel: React.FC<ImageDetailPanelProps> = ({
  detail,
  onClose,
  title,
  className = "",
}) => {
  const { t } = useTranslation();
  const meta = analyzeImageMetadata(detail);
  const fileName = detail.filePath.split(/[\\/]/).pop() || "";
  const displayTitle = title || t("metadata.title");

  // アスペクト比の計算
  let aspectRatioText = "";
  if (detail.width && detail.height && detail.height > 0) {
    const ratio = (detail.width / detail.height).toFixed(2);
    aspectRatioText = ` (${ratio}:1)`;
  }

  const handleReveal = () => {
    backendApi.revealInFileManager(detail.id);
  };

  return (
    <div
      className={`bg-surface/95 border border-border p-4 rounded-xl shadow-2xl backdrop-blur-md text-xs text-textSecondary select-text space-y-2.5 ${className}`}
    >
      {/* ヘッダー */}
      <div className="flex items-center justify-between pb-2 border-b border-border/80 text-textPrimary font-semibold">
        <span className="truncate pr-2">{displayTitle}</span>
        <div className="flex items-center gap-2 shrink-0">
          <button
            onClick={handleReveal}
            title={t("metadata.explorerTooltip")}
            className="flex items-center gap-1 text-accent hover:text-accent/80 hover:underline transition cursor-pointer"
          >
            <FolderOpen className="w-3.5 h-3.5" />
            <span>{t("metadata.explorerBtn")}</span>
          </button>
          {onClose && (
            <button
              onClick={onClose}
              title={t("metadata.closeTooltip")}
              className="p-0.5 text-textSecondary hover:text-textPrimary hover:bg-surfaceLight rounded transition cursor-pointer"
            >
              <X className="w-3.5 h-3.5" />
            </button>
          )}
        </div>
      </div>

      {/* ファイル名・パス */}
      <div className="space-y-1">
        <div className="font-medium text-textPrimary break-all line-clamp-2" title={fileName}>
          {fileName}
        </div>
        <div className="text-[11px] text-textSecondary/80 break-all leading-tight max-h-12 overflow-y-auto pr-1">
          {detail.filePath}
        </div>
      </div>

      {/* メタデータテーブル */}
      <div className="space-y-1.5 pt-1 border-t border-border/50 text-[11px]">
        {/* 解像度 */}
        <div className="flex justify-between items-center">
          <span className="text-textSecondary">{t("metadata.resolution")}</span>
          <span className="text-textPrimary font-mono font-medium">
            {detail.width && detail.height
              ? `${detail.width.toLocaleString()} × ${detail.height.toLocaleString()} px${aspectRatioText}`
              : t("metadata.resolutionUnknown")}
          </span>
        </div>

        {/* フォーマット */}
        <div className="flex justify-between items-center">
          <span className="text-textSecondary">{t("metadata.format")}</span>
          <span className="inline-flex items-center gap-1.5">
            <span className="px-1.5 py-0.5 bg-surfaceLight border border-border/60 rounded text-accent font-semibold">
              {meta.formatName}
            </span>
          </span>
        </div>

        {/* 圧縮方式 (可逆 / 非可逆) */}
        <div className="flex justify-between items-center">
          <span className="text-textSecondary">{t("metadata.compressionType")}</span>
          <span className="text-textPrimary font-medium">{meta.compressionType}</span>
        </div>

        {/* ファイルサイズ */}
        <div className="flex justify-between items-center">
          <span className="text-textSecondary">{t("metadata.fileSize")}</span>
          <span className="text-textPrimary font-mono">{meta.fileSizeDisplay}</span>
        </div>

        {/* 圧縮率 / 削減率 */}
        <div className="flex flex-col gap-0.5 pt-0.5">
          <div className="flex justify-between items-center">
            <span className="text-textSecondary">{t("metadata.compressionRatio")}</span>
            <span className="text-accent font-medium">{meta.compressionRatioDisplay}</span>
          </div>
          {meta.rawSizeDisplay && (
            <div className="flex justify-between items-center text-[10px] text-textSecondary/70">
              <span>{t("metadata.rawEstimated")}</span>
              <span className="font-mono">{meta.rawSizeDisplay}</span>
            </div>
          )}
        </div>

        {/* 撮影日時 */}
        <div className="flex justify-between items-center pt-1 border-t border-border/50">
          <span className="text-textSecondary">{t("metadata.takenAt")}</span>
          <span className="text-textPrimary font-mono">
            {new Date(detail.takenAt * 1000).toISOString().replace("T", " ").substring(0, 19)}
          </span>
        </div>

        {/* 日時ソース */}
        <div className="flex justify-between items-center">
          <span className="text-textSecondary">{t("metadata.source")}</span>
          <span className="text-textPrimary uppercase font-medium">
            {detail.takenAtSource === "exif" ? t("metadata.sourceExif") : t("metadata.sourceMtime")}
          </span>
        </div>
      </div>
    </div>
  );
};
