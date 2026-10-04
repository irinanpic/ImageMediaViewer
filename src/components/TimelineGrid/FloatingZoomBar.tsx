import React from "react";
import { ZoomIn, ZoomOut } from "lucide-react";
import { useAppStore } from "../../store";

/**
 * Picasaスタイルの右下フローティング・サムネイルサイズ変更バー
 *
 * 変更理由: Picasaと同様、タイムライン閲覧中にいつでも直感的にサムネイルサイズを変更できるようにする。
 * スライダー操作、クリック、およびバー上でのホイール回転（Ctrl不要）に対応。
 */
export const FloatingZoomBar: React.FC = () => {
  const cellSize = useAppStore((state) => state.cellSize);
  const setCellSize = useAppStore((state) => state.setCellSize);
  const isViewerOpen = useAppStore((state) => state.isViewerOpen);

  if (isViewerOpen) return null;

  return (
    <div
      onWheel={(e) => {
        e.preventDefault();
        e.stopPropagation();
        const delta = e.deltaY < 0 ? 15 : -15;
        setCellSize(Math.max(80, Math.min(320, cellSize + delta)));
      }}
      className="absolute bottom-5 right-6 z-20 flex items-center gap-2 bg-surface/90 hover:bg-surface border border-border shadow-lg backdrop-blur-md px-3 py-1.5 rounded-full select-none transition-all duration-200 hover:shadow-xl group"
      title="サムネイルサイズ調整（スライダー操作、ホイール、またはグリッド上でCtrl+ホイール）"
    >
      <button
        onClick={() => setCellSize(Math.max(80, cellSize - 20))}
        className="text-textSecondary hover:text-textPrimary p-1 rounded-full hover:bg-surfaceLight transition"
        title="サムネイル縮小"
      >
        <ZoomOut className="w-3.5 h-3.5" />
      </button>

      <input
        type="range"
        min="80"
        max="320"
        step="10"
        value={cellSize}
        onChange={(e) => setCellSize(Number(e.target.value))}
        className="w-24 h-1.5 bg-surfaceLight rounded-lg appearance-none cursor-pointer accent-accent"
      />

      <button
        onClick={() => setCellSize(Math.min(320, cellSize + 20))}
        className="text-textSecondary hover:text-textPrimary p-1 rounded-full hover:bg-surfaceLight transition"
        title="サムネイル拡大"
      >
        <ZoomIn className="w-3.5 h-3.5" />
      </button>

      <span className="text-[11px] font-mono text-textSecondary w-10 text-right tabular-nums">
        {cellSize}px
      </span>
    </div>
  );
};
