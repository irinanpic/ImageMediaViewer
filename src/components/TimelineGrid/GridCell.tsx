import React, { useState } from "react";
import { Check, ImageOff, Loader2 } from "lucide-react";
import { getThumbnailUrl } from "../../lib/thumbUrl";
import { useAppStore } from "../../store";
import type { ImageRecord } from "../../types/generated/ImageRecord";

interface GridCellProps {
  record: ImageRecord | null;
  globalIndex: number;
  isScrollingFast: boolean;
  size: number;
}

// メモリに読み込み完了したサムネイルURLのグローバルキャッシュ
const loadedThumbUrls = new Set<string>();

/**
 * タイムライングリッド内の画像セルコンポーネント
 *
 * 変更理由: 仕様書§8.3およびPicasa並みの快適さ追求。
 * 一度ロードしたサムネイルはメモリキャッシュ(loadedThumbUrls)により再マウント時でもゼロ遅延で即時表示し、
 * 高速スクロール中であってもロード済み画像はアンマウントせず滑らかに維持する。
 * また、Ctrl+クリックおよびドラッグ複数選択、チェックボックスによる選択をサポート。
 */
export const GridCell: React.FC<GridCellProps> = React.memo(
  ({ record, globalIndex, isScrollingFast, size }) => {
    const openViewer = useAppStore((state) => state.openViewer);
    const selectedCellIndex = useAppStore((state) => state.selectedCellIndex);
    const setSelectedCellIndex = useAppStore((state) => state.setSelectedCellIndex);
    const selectedImageIds = useAppStore((state) => state.selectedImageIds);
    const toggleSelectImageId = useAppStore((state) => state.toggleSelectImageId);
    const timelineRefreshTick = useAppStore((state) => state.timelineRefreshTick);

    const isFocused = selectedCellIndex === globalIndex;
    const isMultiSelected = record ? selectedImageIds.includes(record.id) : false;
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

    // ツールバー等からの全体再読込（timelineRefreshTick）に応答してエラー状態をリセット
    React.useEffect(() => {
      if (timelineRefreshTick > 0) {
        setRetryCount(0);
        setHasError(false);
        setReloadTick(0);
        setIsLoaded(thumbUrl ? loadedThumbUrls.has(thumbUrl) : false);
      }
    }, [timelineRefreshTick, thumbUrl]);

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

    // 未ロードかつ高速スクロール中の場合、またはレコード取得中の場合のみスケルトン表示
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

    const handleClick = (e: React.MouseEvent) => {
      if (hasError) {
        setHasError(false);
        setRetryCount(0);
        setIsLoaded(false);
        return;
      }

      // Ctrl + クリック（または Cmd + クリック）、または既に複数選択モード中の場合は選択トグル
      if (e.ctrlKey || e.metaKey || selectedImageIds.length > 0) {
        e.preventDefault();
        e.stopPropagation();
        setSelectedCellIndex(globalIndex);
        toggleSelectImageId(record.id);
        return;
      }

      // 通常クリック時はビューアを起動
      setSelectedCellIndex(globalIndex);
      openViewer(record.id, globalIndex);
    };

    const handleDoubleClick = (e: React.MouseEvent) => {
      if (hasError) return;
      e.preventDefault();
      e.stopPropagation();
      setSelectedCellIndex(globalIndex);
      openViewer(record.id, globalIndex);
    };

    const handleCheckboxClick = (e: React.MouseEvent) => {
      e.preventDefault();
      e.stopPropagation();
      setSelectedCellIndex(globalIndex);
      toggleSelectImageId(record.id);
    };

    // リフレッシュ回数やリトライ回数をクエリに付与してブラウザの画像エラーキャッシュをバイパス
    const queryParts: string[] = [];
    if (retryCount > 0) queryParts.push(`retry=${retryCount}`);
    if (timelineRefreshTick > 0) queryParts.push(`rf=${timelineRefreshTick}`);
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
        title={hasError ? "クリックして再読込を試行" : record ? `画像 #${record.id} (Ctrl+クリックで複数選択)` : ""}
        className={`relative bg-surface rounded overflow-hidden cursor-pointer group transition-all duration-150 select-none ${
          isMultiSelected
            ? "ring-2 ring-accent ring-offset-2 ring-offset-background border-accent shadow-lg shadow-accent/30"
            : isFocused
            ? "ring-2 ring-accent/70 ring-offset-1 ring-offset-background border-accent/80"
            : "border border-transparent hover:border-accent hover:scale-[1.02]"
        }`}
      >
        {!isLoaded && !hasError && !isCached && (
          <div className="absolute inset-0 flex items-center justify-center bg-surface">
            <Loader2 className="w-4 h-4 text-textSecondary/60 animate-spin" />
          </div>
        )}

        {hasError ? (
          <div className="absolute inset-0 flex flex-col items-center justify-center bg-surface text-textSecondary p-2 text-center hover:bg-surfaceLight/60 transition">
            <ImageOff className="w-5 h-5 mb-1 text-red-400" />
            <span className="text-[10px] text-red-400 font-medium">読込不可</span>
            <span className="text-[9px] text-textSecondary/80 mt-0.5 group-hover:underline">クリックで再試行</span>
          </div>
        ) : (
          <img
            src={effectiveSrc}
            alt=""
            decoding="async"
            draggable={false}
            onLoad={() => {
              loadedThumbUrls.add(thumbUrl);
              setIsLoaded(true);
            }}
            onError={handleError}
            className={`w-full h-full object-cover pointer-events-none ${
              isLoaded || isCached ? "opacity-100" : "opacity-0"
            }`}
          />
        )}

        {/* 複数選択時の半透明オーバーレイ */}
        {isMultiSelected && (
          <div className="absolute inset-0 bg-accent/20 pointer-events-none border-2 border-accent rounded" />
        )}

        {/* 左上チェックボックス（選択中、またはホバー時表示） */}
        <div
          onClick={handleCheckboxClick}
          className={`absolute top-1.5 left-1.5 z-10 w-5 h-5 rounded flex items-center justify-center transition-all cursor-pointer ${
            isMultiSelected
              ? "bg-accent text-white shadow-md scale-105"
              : "opacity-0 group-hover:opacity-100 bg-black/50 text-white/80 hover:bg-accent hover:text-white border border-white/60 shadow-xs"
          }`}
          title={isMultiSelected ? "選択を解除" : "選択に追加"}
        >
          <Check className="w-3.5 h-3.5 stroke-[3]" />
        </div>
      </div>
    );
  }
);


GridCell.displayName = "GridCell";
