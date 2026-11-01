import React, { useState } from "react";
import { Check, ImageOff, Layers, Loader2, PenTool } from "lucide-react";
import { backendApi } from "../../lib/ipc";
import { getThumbnailUrl } from "../../lib/thumbUrl";
import { useTranslation } from "../../locales";
import { useAppStore } from "../../store";
import type { ImageRecord } from "../../types/generated/ImageRecord";

interface GridCellProps {
  record: ImageRecord | null;
  globalIndex: number;
  isScrollingFast: boolean;
  size: number;
}

// メモリに読み込み完了したサムネイルURLのグローバルキャッシュ（最大10,000件でFIFO破棄）
const MAX_LOADED_THUMB_URLS = 10000;
const loadedThumbUrls = new Set<string>();

/**
 * タイムライングリッド内の画像セルコンポーネント
 *
 * 変更理由: 仕様書§8.3および高速で快適な画像表示の追求。
 * 一度ロードしたサムネイルはメモリキャッシュ(loadedThumbUrls)により再マウント時でもゼロ遅延で即時表示し、
 * 高速スクロール中であってもロード済み画像はアンマウントせず滑らかに維持する。
 * また、Ctrl+クリックおよびドラッグ複数選択、チェックボックスによる選択をサポート。
 */
export const GridCell: React.FC<GridCellProps> = React.memo(
  ({ record, globalIndex, isScrollingFast, size }) => {
    const { t } = useTranslation();
    const openViewer = useAppStore((state) => state.openViewer);
    const selectedCellIndex = useAppStore((state) => state.selectedCellIndex);
    const setSelectedCellIndex = useAppStore((state) => state.setSelectedCellIndex);
    const toggleSelectImageId = useAppStore((state) => state.toggleSelectImageId);
    const timelineRefreshTick = useAppStore((state) => state.timelineRefreshTick);
    const openContextMenu = useAppStore((state) => state.openContextMenu);
    const setSelectedFolderId = useAppStore((state) => state.setSelectedFolderId);
    const updateFilterState = useAppStore((state) => state.updateFilterState);
    const setCurrentView = useAppStore((state) => state.setCurrentView);
    const refreshTimeline = useAppStore((state) => state.refreshTimeline);
    const timelineSort = useAppStore((state) => state.timelineSort);
    const setTimelineSort = useAppStore((state) => state.setTimelineSort);
    const setFolderHierarchy = useAppStore((state) => state.setFolderHierarchy);

    const isArchive = Boolean(record?.isArchive);
    const archiveName = record?.fileName || (record?.filePath ? record.filePath.split(/[/\\]/).pop() : "") || "";
    const archiveExt = archiveName.split(".").pop()?.toUpperCase() || "ZIP";

    // ClipStudio形式（.clip）判定: format識別子または拡張子から判定
    const isClip = Boolean(
      record?.format?.toLowerCase() === "clip" ||
      (record?.fileName && /\.clip$/i.test(record.fileName)) ||
      (record?.filePath && /\.clip$/i.test(record.filePath))
    );

    const [isOpeningArchive, setIsOpeningArchive] = useState(false);

    const isFocused = selectedCellIndex === globalIndex;
    const isMultiSelected = useAppStore((state) =>
      record ? state.selectedImageIds.includes(record.id) : false
    );
    const thumbUrl = record ? getThumbnailUrl(record.id, record.rev) : "";
    const isCached = thumbUrl ? loadedThumbUrls.has(thumbUrl) : false;

    const [retryCount, setRetryCount] = useState(0);
    const [hasError, setHasError] = useState(false);
    const [isLoaded, setIsLoaded] = useState(isCached);
    const [reloadTick, setReloadTick] = useState(0);

    // 画像IDが変わった場合は状態をリセット
    React.useEffect(() => {
      setRetryCount(0);
      setHasError(false);
      setReloadTick(0);
      setIsLoaded(thumbUrl ? loadedThumbUrls.has(thumbUrl) : false);
    }, [record?.id, thumbUrl]);

    // ツールバー等からの全体再読込（timelineRefreshTick）に応答してエラー状態のみリセット（表示優先）
    // 変更理由: 表示済み画像をスピナーに戻したり、全セルが一斉に再リクエストを投げて処理が重くなるのを防止
    React.useEffect(() => {
      if (timelineRefreshTick > 0) {
        if (hasError) {
          setRetryCount(0);
          setHasError(false);
          setReloadTick((t) => t + 1);
          setIsLoaded(false);
        }
      }
    }, [timelineRefreshTick, hasError]);

    // 急激なスクロール移動が終わった後（isScrollingFast: true -> false）、
    // ユーザー要求に従い retryCount >= 4 や hasError の状態を即座にリセットして現画面での再読み込みを仕切り直す
    const prevScrollingFast = React.useRef(isScrollingFast);
    React.useEffect(() => {
      if (prevScrollingFast.current && !isScrollingFast) {
        // 急激な移動が終了した瞬間: まだロードされていない、またはエラー状態のセルを即座に復活
        if (!isLoaded && !isCached) {
          setRetryCount(0);
          setHasError(false);
          setReloadTick((t) => t + 1);
        }
      }
      prevScrollingFast.current = isScrollingFast;
    }, [isScrollingFast, isLoaded, isCached]);

    // タッチデバイス向け長押し（500ms）複数選択用のRef
    // 変更理由: Rules of Hooks違反（React error #310）を防止するため、早期returnの前にすべてのフックを呼び出す
    const longPressTimerRef = React.useRef<number | null>(null);
    const touchStartPosRef = React.useRef<{ x: number; y: number } | null>(null);
    const isLongPressTriggeredRef = React.useRef(false);

    // セルアンマウント時に長押しタイマーを安全に解除
    React.useEffect(() => {
      return () => {
        if (longPressTimerRef.current) {
          window.clearTimeout(longPressTimerRef.current);
        }
      };
    }, []);

    // 読込失敗時の自動リトライ
    const handleError = React.useCallback(() => {
      if (isScrollingFast) {
        return;
      }

      if (retryCount < 4) {
        const delays = [150, 400, 900, 1800];
        const timer = setTimeout(() => {
          setRetryCount((c) => c + 1);
        }, delays[retryCount]);
        return () => clearTimeout(timer);
      } else {
        setHasError(true);
      }
    }, [retryCount, isScrollingFast]);

    const handleTouchStart = (e: React.TouchEvent) => {
      if (!record || e.touches.length !== 1) return;
      const touch = e.touches[0];
      touchStartPosRef.current = { x: touch.clientX, y: touch.clientY };
      isLongPressTriggeredRef.current = false;

      if (longPressTimerRef.current) {
        window.clearTimeout(longPressTimerRef.current);
      }

      longPressTimerRef.current = window.setTimeout(() => {
        isLongPressTriggeredRef.current = true;
        setSelectedCellIndex(globalIndex);
        if (typeof navigator !== "undefined" && navigator.vibrate) {
          try {
            navigator.vibrate(40);
          } catch {}
        }
        if (touchStartPosRef.current) {
          openContextMenu({
            x: touchStartPosRef.current.x,
            y: touchStartPosRef.current.y,
            record,
          });
        }
      }, 450);
    };

    const handleTouchMove = (e: React.TouchEvent) => {
      if (!touchStartPosRef.current || e.touches.length !== 1) return;
      const touch = e.touches[0];
      const dist = Math.hypot(
        touch.clientX - touchStartPosRef.current.x,
        touch.clientY - touchStartPosRef.current.y
      );
      if (dist > 10) {
        if (longPressTimerRef.current) {
          window.clearTimeout(longPressTimerRef.current);
          longPressTimerRef.current = null;
        }
      }
    };

    const handleTouchEnd = () => {
      if (longPressTimerRef.current) {
        window.clearTimeout(longPressTimerRef.current);
        longPressTimerRef.current = null;
      }
    };

    const handleOpenArchive = async () => {
      if (!record || isOpeningArchive) return;
      setIsOpeningArchive(true);
      try {
        const folderId = record.folderId ?? useAppStore.getState().selectedFolderId ?? 0;
        const targetPath = record.filePath || record.fileName || "";
        await backendApi.openArchiveFolder(folderId, targetPath);
        if (timelineSort !== "nameAsc" && timelineSort !== "nameDesc") {
          setTimelineSort("nameAsc");
        }
        if (folderId > 0) {
          const updatedTree = await backendApi.getFolderHierarchy(folderId);
          setFolderHierarchy(folderId, updatedTree);
        }
        setSelectedFolderId(folderId);
        updateFilterState({ subFolderPath: targetPath });
        setCurrentView("timeline");
        refreshTimeline();
      } catch (err) {
        console.error("アーカイブ展開エラー:", err);
      } finally {
        setIsOpeningArchive(false);
      }
    };

    const handleClick = (e: React.MouseEvent) => {
      if (isLongPressTriggeredRef.current) {
        isLongPressTriggeredRef.current = false;
        e.preventDefault();
        e.stopPropagation();
        return;
      }

      if (hasError) {
        setHasError(false);
        setRetryCount(0);
        setIsLoaded(false);
        return;
      }

      if (!record) return;

      // Ctrl + クリック（または Cmd + クリック）、または既に複数選択モード中の場合は選択トグル
      if (e.ctrlKey || e.metaKey || useAppStore.getState().selectedImageIds.length > 0) {
        e.preventDefault();
        e.stopPropagation();
        setSelectedCellIndex(globalIndex);
        toggleSelectImageId(record.id);
        return;
      }

      // アーカイブの場合は中身の仮想フォルダへドリルダウン展開
      if (isArchive) {
        setSelectedCellIndex(globalIndex);
        handleOpenArchive();
        return;
      }

      // 通常クリック時はビューアを起動
      setSelectedCellIndex(globalIndex);
      openViewer(record.id, globalIndex);
    };

    const handleDoubleClick = (e: React.MouseEvent) => {
      if (hasError || !record) return;
      e.preventDefault();
      e.stopPropagation();
      setSelectedCellIndex(globalIndex);
      if (isArchive) {
        handleOpenArchive();
      } else {
        openViewer(record.id, globalIndex);
      }
    };

    const handleCheckboxClick = (e: React.MouseEvent) => {
      if (!record) return;
      e.preventDefault();
      e.stopPropagation();
      setSelectedCellIndex(globalIndex);
      toggleSelectImageId(record.id);
    };

    // 右クリック時のコンテキストメニュー起動
    const handleContextMenu = (e: React.MouseEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (record) {
        setSelectedCellIndex(globalIndex);
        openContextMenu({
          x: e.clientX,
          y: e.clientY,
          record,
        });
      }
    };

    // 未ロードかつ高速スクロール中の場合、またはレコード取得中の場合のみスケルトン表示
    // 変更理由: すべてのReactフックが評価された後に早期returnを行うことで、フックの呼び出し数・順序が毎レンダリングで一定であることを維持する
    if (!record || (isScrollingFast && !isLoaded && !isCached)) {
      return (
        <div
          style={{ width: size, height: size }}
          className={`bg-surface rounded overflow-hidden flex items-center justify-center border ${
            isFocused ? "border-accent ring-2 ring-accent" : "border-border/30"
          }`}
        >
          <div className="w-8 h-8 rounded-full bg-surfaceLight/30 animate-pulse" />
        </div>
      );
    }

    // リフレッシュ回数やリトライ回数をクエリに付与してブラウザの画像エラーキャッシュをバイパス
    const queryParts: string[] = [];
    if (retryCount > 0) queryParts.push(`retry=${retryCount}`);
    if (hasError && timelineRefreshTick > 0) queryParts.push(`rf=${timelineRefreshTick}`);
    if (reloadTick > 0) queryParts.push(`rt=${reloadTick}`);
    const effectiveSrc = thumbUrl
      ? queryParts.length > 0
        ? `${thumbUrl}${thumbUrl.includes("?") ? "&" : "?"}${queryParts.join("&")}`
        : thumbUrl
      : "";

    return (
      <div
        data-grid-cell="true"
        data-image-id={record.id}
        data-global-index={globalIndex}
        style={{ width: size, height: size }}
        onClick={handleClick}
        onDoubleClick={handleDoubleClick}
        onContextMenu={handleContextMenu}
        onTouchStart={handleTouchStart}
        onTouchMove={handleTouchMove}
        onTouchEnd={handleTouchEnd}
        onTouchCancel={handleTouchEnd}
        title={
          hasError
            ? t("timeline.cellErrorTooltip")
            : isArchive
            ? `${archiveName}\n${t("sidebar.archiveClickToOpen")}`
            : record
            ? t("timeline.cellImageTooltip", { id: record.id })
            : ""
        }
        className={`relative rounded overflow-hidden cursor-pointer group transition-all duration-150 select-none ${
          isArchive ? "p-1.5" : "bg-surface"
        } ${
          isMultiSelected
            ? "ring-2 ring-accent ring-offset-2 ring-offset-background border-accent shadow-lg shadow-accent/30"
            : isFocused
            ? "ring-2 ring-accent/70 ring-offset-1 ring-offset-background border-accent/80"
            : "border border-transparent hover:border-accent hover:scale-[1.02]"
        }`}
      >
        {/* アーカイブ用：写真3Dスタック背景カード */}
        {isArchive && (
          <>
            {/* 背面カード2（最深層・右へ少し傾いた写真カード） */}
            <div className="absolute inset-2 bg-surfaceLight/60 rounded border border-border/40 rotate-4 translate-x-1.5 -translate-y-1 shadow-xs pointer-events-none group-hover:rotate-6 group-hover:translate-x-2 group-hover:-translate-y-1.5 transition-transform duration-200" />
            {/* 背面カード1（中間層・左へ少し傾いた写真カード） */}
            <div className="absolute inset-2 bg-surfaceLight/80 rounded border border-border/60 -rotate-2 -translate-x-1 -translate-y-0.5 shadow-md pointer-events-none group-hover:-rotate-4 group-hover:-translate-x-1.5 group-hover:-translate-y-1 transition-transform duration-200" />
          </>
        )}

        {/* 最前面コンテンツラッパー */}
        <div
          className={`relative w-full h-full bg-surface rounded overflow-hidden flex flex-col justify-between ${
            isArchive ? "border border-border/70 shadow-sm" : ""
          }`}
        >
          {!isLoaded && !hasError && !isCached && (
            <div className="absolute inset-0 flex items-center justify-center bg-surface">
              <Loader2 className="w-4 h-4 text-textSecondary/60 animate-spin" />
            </div>
          )}

          {hasError ? (
            <div className="absolute inset-0 flex flex-col items-center justify-center bg-surface text-textSecondary p-2 text-center hover:bg-surfaceLight/60 transition">
              {isArchive ? (
                <>
                  <Layers className="w-6 h-6 mb-1 text-amber-500/80" />
                  <span className="text-[10px] text-amber-400 font-medium">{archiveExt}</span>
                  <span className="text-[9px] text-textSecondary/80 mt-0.5">{t("sidebar.archiveClickToOpen")}</span>
                </>
              ) : (
                <>
                  <ImageOff className="w-5 h-5 mb-1 text-red-400" />
                  <span className="text-[10px] text-red-400 font-medium">{t("timeline.cellErrorRetry")}</span>
                  <span className="text-[9px] text-textSecondary/80 mt-0.5 group-hover:underline">{t("timeline.cellErrorRetryHint")}</span>
                </>
              )}
            </div>
          ) : (
            <img
              src={effectiveSrc}
              alt=""
              decoding="async"
              draggable={false}
              onLoad={() => {
                if (!loadedThumbUrls.has(thumbUrl) && loadedThumbUrls.size >= MAX_LOADED_THUMB_URLS) {
                  const oldest = loadedThumbUrls.values().next().value;
                  if (oldest !== undefined) {
                    loadedThumbUrls.delete(oldest);
                  }
                }
                loadedThumbUrls.add(thumbUrl);
                setIsLoaded(true);
              }}
              onError={handleError}
              className={`w-full h-full object-cover pointer-events-none ${
                isLoaded || isCached ? "opacity-100" : "opacity-0"
              }`}
            />
          )}

          {/* アーカイブ用オーバーレイとバッジ表示 */}
          {isArchive && (
            <>
              {/* 下部グラデーション（テキスト可読性確保） */}
              <div className="absolute inset-0 bg-gradient-to-t from-black/85 via-black/20 to-black/40 pointer-events-none" />

              {/* 上部ヘッダー（右上：スタックアイコン＆拡張子バッジ） */}
              <div className="absolute top-1.5 right-1.5 z-10 flex items-center gap-1 bg-black/60 text-amber-300 border border-amber-500/40 px-1.5 py-0.5 rounded text-[9px] font-bold font-mono shadow-xs backdrop-blur-xs">
                <Layers className="w-2.5 h-2.5 text-amber-400" />
                <span>{archiveExt}</span>
              </div>

              {/* 下部フッター（アーカイブファイル名） */}
              <div className="absolute bottom-0 inset-x-0 p-1.5 z-10 pointer-events-none">
                <p className="text-[11px] font-semibold text-white truncate drop-shadow-md">
                  {archiveName}
                </p>
                <p className="text-[9px] text-amber-300/80 truncate flex items-center gap-0.5 mt-0.5">
                  <span>クリックで展開</span>
                </p>
              </div>

              {/* 展開中ローディングオーバーレイ */}
              {isOpeningArchive && (
                <div className="absolute inset-0 z-30 bg-black/70 backdrop-blur-xs flex flex-col items-center justify-center text-amber-400 gap-1 pointer-events-none">
                  <Loader2 className="w-5 h-5 animate-spin" />
                  <span className="text-[10px] font-medium text-white">展開中...</span>
                </div>
              )}
            </>
          )}

          {/* 複数選択時の半透明オーバーレイ */}
          {isMultiSelected && (
            <div className="absolute inset-0 bg-accent/20 pointer-events-none border-2 border-accent rounded" />
          )}

          {/* ClipStudio用ファイル種別バッジ（右上） */}
          {/* 変更理由: ZIPアーカイブ同様に、ClipStudioファイル形式であることを一覧上で瞬時に判別できるようにするため */}
          {!isArchive && isClip && (
            <div
              className="absolute top-1.5 right-1.5 z-10 flex items-center gap-1 bg-black/65 text-sky-300 border border-sky-500/40 px-1.5 py-0.5 rounded text-[9px] font-bold font-mono shadow-xs backdrop-blur-xs pointer-events-none"
              title="CLIP STUDIO PAINT (.clip)"
            >
              <PenTool className="w-2.5 h-2.5 text-sky-400" />
              <span>CLIP</span>
            </div>
          )}

          {/* 左上チェックボックス（選択中、またはホバー時表示） */}
          <div
            onClick={handleCheckboxClick}
            className={`absolute top-1.5 left-1.5 z-20 w-5 h-5 rounded flex items-center justify-center transition-all cursor-pointer ${
              isMultiSelected
                ? "bg-accent text-white shadow-md scale-105"
                : "opacity-0 group-hover:opacity-100 bg-black/50 text-white/80 hover:bg-accent hover:text-white border border-white/60 shadow-xs"
            }`}
            title={isMultiSelected ? t("timeline.cellDeselectTooltip") : t("timeline.cellSelectTooltip")}
          >
            <Check className="w-3.5 h-3.5 stroke-[3]" />
          </div>
        </div>
      </div>
    );
  }
);


GridCell.displayName = "GridCell";
