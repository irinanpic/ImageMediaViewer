import React from "react";
import { Loader2, PanelLeftClose, PanelLeftOpen, ZoomIn, ZoomOut } from "lucide-react";
import { useAppStore } from "../../store";

export const StatusBar: React.FC = () => {
  const totalImages = useAppStore((state) => state.totalImages);
  const scanProgress = useAppStore((state) => state.scanProgress);
  const thumbProgress = useAppStore((state) => state.thumbProgress);
  const cellSize = useAppStore((state) => state.cellSize);
  const setCellSize = useAppStore((state) => state.setCellSize);
  const isSidebarOpen = useAppStore((state) => state.isSidebarOpen);
  const toggleSidebar = useAppStore((state) => state.toggleSidebar);

  const isScanning = scanProgress && scanProgress.phase !== "done";
  const isGeneratingThumbs =
    thumbProgress && thumbProgress.total > 0 && thumbProgress.done < thumbProgress.total;

  return (
    <footer className="h-8 bg-surface border-t border-border px-3 flex items-center justify-between text-xs text-textSecondary select-none z-10">
      <div className="flex items-center gap-3">
        <button
          onClick={toggleSidebar}
          className="p-1 hover:text-textPrimary transition"
          title={isSidebarOpen ? "サイドバーを閉じる" : "サイドバーを開く"}
        >
          {isSidebarOpen ? (
            <PanelLeftClose className="w-4 h-4" />
          ) : (
            <PanelLeftOpen className="w-4 h-4" />
          )}
        </button>

        <span>総画像数: <strong className="text-textPrimary">{totalImages.toLocaleString()}</strong> 枚</span>

        {/* 走査中ステータス */}
        {isScanning && (
          <div className="flex items-center gap-1.5 text-accent animate-pulse">
            <Loader2 className="w-3.5 h-3.5 animate-spin" />
            <span>
              走査中 ({scanProgress.phase === "walking" ? "探索中..." : `${scanProgress.processed} / ${scanProgress.discovered} 件`})
            </span>
          </div>
        )}

        {/* サムネイル生成ステータス */}
        {isGeneratingThumbs ? (
          <div className="flex items-center gap-1.5 text-emerald-400 font-medium">
            <Loader2 className="w-3.5 h-3.5 animate-spin" />
            <span>
              サムネイル生成中 ({thumbProgress.done.toLocaleString()} / {thumbProgress.total.toLocaleString()} 件 - {Math.round((thumbProgress.done / thumbProgress.total) * 100)}%)
            </span>
          </div>
        ) : thumbProgress && thumbProgress.total > 0 && thumbProgress.done >= thumbProgress.total && !isScanning ? (
          <div className="flex items-center gap-1 text-emerald-500/80 text-[11px]">
            <span>✓ サムネイル準備完了</span>
          </div>
        ) : null}
      </div>

      {/* サムネイルサイズスライダー（唯一の拡大縮小バーとして集約） */}
      <div
        className="flex items-center gap-1.5 py-0.5 px-2 rounded hover:bg-surfaceLight/50 transition cursor-pointer"
        onWheel={(e) => {
          e.preventDefault();
          e.stopPropagation();
          const delta = e.deltaY < 0 ? 10 : -10;
          setCellSize(Math.max(80, Math.min(320, cellSize + delta)));
        }}
        title="サムネイルサイズ (スライダー、ホイール、またはグリッド上でCtrl+ホイールで変更)"
      >
        <button
          onClick={() => setCellSize(Math.max(80, cellSize - 20))}
          className="p-1 hover:text-textPrimary text-textSecondary transition"
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
          className="p-1 hover:text-textPrimary text-textSecondary transition"
          title="サムネイル拡大"
        >
          <ZoomIn className="w-3.5 h-3.5" />
        </button>
        <span className="w-10 text-right tabular-nums font-mono text-[11px]">{cellSize}px</span>
      </div>
    </footer>
  );
};
