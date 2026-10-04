import React from "react";
import {
  AlertTriangle,
  CheckCircle2,
  FileText,
  Loader2,
  PanelLeftClose,
  PanelLeftOpen,
  ZoomIn,
  ZoomOut,
} from "lucide-react";
import { useAppStore } from "../../store";

export const StatusBar: React.FC = () => {
  const totalImages = useAppStore((state) => state.totalImages);
  const scanProgress = useAppStore((state) => state.scanProgress);
  const thumbProgress = useAppStore((state) => state.thumbProgress);
  const cellSize = useAppStore((state) => state.cellSize);
  const setCellSize = useAppStore((state) => state.setCellSize);
  const isSidebarOpen = useAppStore((state) => state.isSidebarOpen);
  const toggleSidebar = useAppStore((state) => state.toggleSidebar);
  const openLogModal = useAppStore((state) => state.openLogModal);

  const isScanning = scanProgress && scanProgress.phase !== "done";
  const failedCount = thumbProgress?.failed ?? 0;
  const doneCount = thumbProgress?.done ?? 0;
  const totalCount = thumbProgress?.total ?? 0;
  const processedCount = doneCount + failedCount;

  // 生成完了・生成中判定（失敗数も含めて全体の完了を判定）
  const isGeneratingThumbs =
    thumbProgress && totalCount > 0 && processedCount < totalCount;
  const isAllThumbsProcessed =
    thumbProgress && totalCount > 0 && processedCount >= totalCount;

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
              サムネイル生成中 ({doneCount.toLocaleString()} / {totalCount.toLocaleString()} 件
              {failedCount > 0 && (
                <span className="text-amber-400 ml-1">[{failedCount}件失敗]</span>
              )}
              {" - "}{Math.round((processedCount / totalCount) * 100)}%)
            </span>
          </div>
        ) : isAllThumbsProcessed && !isScanning ? (
          failedCount > 0 ? (
            <button
              onClick={openLogModal}
              className="flex items-center gap-1.5 text-amber-400 hover:text-amber-300 bg-amber-500/10 hover:bg-amber-500/20 px-2 py-0.5 rounded transition text-[11px]"
              title="サムネイル生成に失敗したファイルがあります。クリックして詳細とログを確認"
            >
              <AlertTriangle className="w-3.5 h-3.5 text-amber-400 shrink-0" />
              <span>
                サムネイル完了 ({doneCount.toLocaleString()}件完了 /{" "}
                <strong className="underline underline-offset-2">{failedCount}件失敗</strong>)
              </span>
            </button>
          ) : (
            <div className="flex items-center gap-1 text-emerald-500/80 text-[11px]">
              <CheckCircle2 className="w-3.5 h-3.5" />
              <span>サムネイル準備完了 ({doneCount.toLocaleString()}件)</span>
            </div>
          )
        ) : null}
      </div>

      <div className="flex items-center gap-2">
        {/* ログ・診断確認ボタン */}
        <button
          onClick={openLogModal}
          className="flex items-center gap-1 py-1 px-2 rounded hover:bg-surfaceLight transition text-textSecondary hover:text-textPrimary text-[11px]"
          title="システムログ & エラー診断を開く"
        >
          <FileText className="w-3.5 h-3.5" />
          <span>ログ</span>
          {failedCount > 0 && (
            <span className="px-1.5 py-0.2 text-[10px] bg-amber-500/20 text-amber-400 rounded-full font-bold">
              {failedCount}
            </span>
          )}
        </button>

        <div className="h-3.5 w-px bg-border" />

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
      </div>
    </footer>
  );
};
