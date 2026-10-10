import React, { useCallback, useEffect, useRef, useState } from "react";
import type { TimelineRow } from "../../lib/buildRows";
import { useTranslation } from "../../locales";

interface TimelineScrubberProps {
  containerRef: React.RefObject<HTMLDivElement | null>;
  rows: TimelineRow[];
  totalImages: number;
}

/**
 * タイムライン右側の高速スクラブ／スクロールバーコンポーネント (Picasaスタイル)
 *
 * 変更理由: 数万枚のライブラリでもマウスドラッグで一気に目的の年月・画像位置へ
 * 直感的にスクロールできるようにするため。ドラッグ中は日付と通し番号をバッジ表示する。
 *
 * @param props.containerRef スクロールコンテナ要素の参照
 * @param props.rows タイムラインの行データ配列
 * @param props.totalImages 総画像枚数
 */
export const TimelineScrubber: React.FC<TimelineScrubberProps> = ({
  containerRef,
  rows,
  totalImages,
}) => {
  const { t } = useTranslation();
  const barRef = useRef<HTMLDivElement>(null);
  const [scrollRatio, setScrollRatio] = useState(0);
  const [thumbHeightRatio, setThumbHeightRatio] = useState(0.1);
  const [isDragging, setIsDragging] = useState(false);
  const [isHovered, setIsHovered] = useState(false);
  const [currentInfo, setCurrentInfo] = useState<{ label: string; index: number } | null>(null);

  const isDraggingRef = useRef(false);

  // コンテナのスクロール位置を監視してつまみ位置を更新
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;

    const updateThumb = () => {
      const { scrollTop, scrollHeight, clientHeight } = el;
      const maxScroll = scrollHeight - clientHeight;
      if (maxScroll > 0) {
        const ratio = Math.max(0, Math.min(1, scrollTop / maxScroll));
        setScrollRatio(ratio);
        const hRatio = Math.max(0.04, Math.min(0.3, clientHeight / scrollHeight));
        setThumbHeightRatio(hRatio);

        // 現在位置の情報（日付・インデックス）を推計
        const approxRowIdx = Math.floor(ratio * (rows.length - 1));
        const row = rows[approxRowIdx];
        if (row) {
          if (row.kind === "header") {
            setCurrentInfo({ label: row.day, index: 0 });
          } else {
            // 直近のヘッダー日付を探す
            let day = "";
            for (let i = approxRowIdx; i >= 0; i--) {
              if (rows[i]?.kind === "header") {
                day = (rows[i] as { day: string }).day;
                break;
              }
            }
            setCurrentInfo({ label: day, index: row.startIndex });
          }
        }
      }
    };

    updateThumb();
    el.addEventListener("scroll", updateThumb, { passive: true });
    window.addEventListener("resize", updateThumb);

    return () => {
      el.removeEventListener("scroll", updateThumb);
      window.removeEventListener("resize", updateThumb);
    };
  }, [containerRef, rows]);

  // 指定Y座標からスクロール位置を計算して適用
  const scrollToClientY = useCallback(
    (clientY: number) => {
      const bar = barRef.current;
      const el = containerRef.current;
      if (!bar || !el) return;

      const rect = bar.getBoundingClientRect();
      const relativeY = clientY - rect.top;
      const ratio = Math.max(0, Math.min(1, relativeY / rect.height));

      const maxScroll = el.scrollHeight - el.clientHeight;
      el.scrollTop = ratio * maxScroll;
      setScrollRatio(ratio);
    },
    [containerRef]
  );

  // つまみ・バーのポインターダウン（マウスまたはタッチによるドラッグ開始）
  const handlePointerDown = useCallback(
    (e: React.PointerEvent) => {
      if (e.button !== 0) return;
      e.preventDefault();
      e.stopPropagation();

      try {
        (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
      } catch {
        // 一部環境用フォールバック
      }

      isDraggingRef.current = true;
      setIsDragging(true);
      scrollToClientY(e.clientY);
    },
    [scrollToClientY]
  );

  const handlePointerMove = useCallback(
    (e: React.PointerEvent) => {
      if (!isDraggingRef.current) return;
      e.preventDefault();
      scrollToClientY(e.clientY);
    },
    [scrollToClientY]
  );

  const handlePointerUp = useCallback(
    (e: React.PointerEvent) => {
      if (!isDraggingRef.current) return;
      try {
        (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
      } catch {
        // フォールバック
      }
      isDraggingRef.current = false;
      setIsDragging(false);
    },
    []
  );

  if (totalImages === 0) return null;

  const barHeight = barRef.current?.clientHeight ?? 400;
  const thumbHeightPx = Math.max(36, barHeight * thumbHeightRatio);
  const maxTopPx = barHeight - thumbHeightPx;
  const thumbTopPx = scrollRatio * maxTopPx;

  return (
    <div
      ref={barRef}
      data-scrubber="true"
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={handlePointerUp}
      onPointerCancel={handlePointerUp}
      onMouseEnter={() => setIsHovered(true)}
      onMouseLeave={() => !isDragging && setIsHovered(false)}
      style={{ touchAction: "none" }}
      className="absolute top-0 right-0 bottom-0 w-6 z-20 flex justify-center cursor-pointer select-none group bg-background/20 hover:bg-surface/50 backdrop-blur-xs transition-colors"
      title={t("timeline.scrubberTooltip")}
    >
      {/* スクロールレール（細い縦線） */}
      <div className="absolute top-2 bottom-2 w-1 bg-border/40 rounded-full group-hover:bg-border/80 transition-colors" />

      {/* スクロールつまみ（ノブ） */}
      <div
        style={{
          height: `${thumbHeightPx}px`,
          transform: `translateY(${thumbTopPx}px)`,
        }}
        className={`absolute top-0 w-3 rounded-full shadow-md transition-colors ${
          isDragging
            ? "bg-accent scale-110 shadow-accent/40"
            : isHovered
            ? "bg-accent/80"
            : "bg-surfaceLight hover:bg-accent/70 border border-border/80"
        }`}
      />

      {/* ドラッグ・ホバー中に左側に現れる日付・枚数バッジ */}
      {(isDragging || isHovered) && currentInfo && (
        <div
          style={{
            top: `${Math.min(barHeight - 48, Math.max(16, thumbTopPx))}px`,
          }}
          className="absolute right-8 z-30 pointer-events-none bg-surface/95 text-textPrimary text-xs px-3 py-1.5 rounded-lg border border-border shadow-2xl flex flex-col items-end whitespace-nowrap animate-in fade-in zoom-in-95 duration-100"
        >
          <div className="font-semibold text-accent text-[13px]">{currentInfo.label || t("timeline.timelinePosition")}</div>
          <div className="text-[11px] text-textSecondary font-mono mt-0.5">
            {currentInfo.index > 0 ? `${currentInfo.index.toLocaleString()} ${t("common.images")}` : ""} / {totalImages.toLocaleString()} {t("common.images")}
          </div>
        </div>
      )}
    </div>
  );
};
