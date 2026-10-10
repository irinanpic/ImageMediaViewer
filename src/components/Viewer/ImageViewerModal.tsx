import React, { useEffect, useRef, useState } from "react";
import {
  ChevronLeft,
  ChevronRight,
  Info,
  LayoutGrid,
  Maximize2,
  Minimize2,
  RotateCw,
  X,
} from "lucide-react";
import { usePagedImages } from "../../hooks/usePagedImages";
import { backendApi } from "../../lib/ipc";
import { getOriginalImageUrl, getThumbnailUrl } from "../../lib/thumbUrl";
import { useTranslation } from "../../locales";
import { useAppStore } from "../../store";
import type { ImageDetail } from "../../types/generated/ImageDetail";
import { ImageDetailPanel } from "../Common/ImageDetailPanel";
import { useViewerGestures } from "./useViewerGestures";

export const ImageViewerModal: React.FC = () => {
  const { t } = useTranslation();
  const isViewerOpen = useAppStore((state) => state.isViewerOpen);
  const activeImageId = useAppStore((state) => state.activeImageId);
  const activeImageIndex = useAppStore((state) => state.activeImageIndex);
  const transform = useAppStore((state) => state.transform);
  const totalImages = useAppStore((state) => state.totalImages);
  const openAddToBoardModal = useAppStore((state) => state.openAddToBoardModal);

  const closeViewer = useAppStore((state) => state.closeViewer);
  const setViewerIndex = useAppStore((state) => state.setViewerIndex);
  const rotateClockwise = useAppStore((state) => state.rotateClockwise);
  const rotateCounterClockwise = useAppStore((state) => state.rotateCounterClockwise);
  const toggleFlipH = useAppStore((state) => state.toggleFlipH);
  const toggleFlipV = useAppStore((state) => state.toggleFlipV);
  const resetTransform = useAppStore((state) => state.resetTransform);
  const setZoom = useAppStore((state) => state.setZoom);

  const { getImageByIndex, ensureIndex } = usePagedImages();
  const [detail, setDetail] = useState<ImageDetail | null>(null);
  const [showInfo, setShowInfo] = useState(false);
  const [originalLoaded, setOriginalLoaded] = useState(false);
  const [originalError, setOriginalError] = useState(false);

  const containerRef = useRef<HTMLDivElement>(null);
  const imgRef = useRef<HTMLImageElement>(null);

  // 前後への移動処理
  const handlePrev = async () => {
    if (activeImageIndex !== null && activeImageIndex > 0) {
      const nextIndex = activeImageIndex - 1;
      const rec = await ensureIndex(nextIndex);
      if (rec) {
        setViewerIndex(rec.id, nextIndex);
      }
    }
  };

  const handleNext = async () => {
    if (activeImageIndex !== null && activeImageIndex < totalImages - 1) {
      const nextIndex = activeImageIndex + 1;
      const rec = await ensureIndex(nextIndex);
      if (rec) {
        setViewerIndex(rec.id, nextIndex);
      }
    }
  };

  useViewerGestures(containerRef, imgRef, {
    onNext: handleNext,
    onPrev: handlePrev,
  });

  // 回転後の元画像解像度とコンテナ寸法から真の100%原寸ピクセル等倍（1:1）ズーム率を算出
  // 変更理由: フィット表示（縮小）から元画像の本来のピクセル解像度で細部をクリアに確認できるようにするため
  const get100PercentZoom = (): number => {
    const el = containerRef.current;
    const img = imgRef.current;
    if (!el) return 2.0;

    const naturalW = img?.naturalWidth || detail?.width || 0;
    const naturalH = img?.naturalHeight || detail?.height || 0;
    if (naturalW === 0 || naturalH === 0) return 2.0;

    const isRotated = transform.rotation % 180 !== 0;
    const effW = isRotated ? naturalH : naturalW;
    const effH = isRotated ? naturalW : naturalH;

    const fitScale = Math.min(el.clientWidth / effW, el.clientHeight / effH);
    if (fitScale <= 0) return 2.0;

    // 元画像の1ピクセル = 画面の1ピクセルとなるスケール
    return Math.max(1.2, Math.min(16, 1.0 / fitScale));
  };

  // 現在の画像レコード取得
  const currentRecord = activeImageIndex !== null ? getImageByIndex(activeImageIndex) : null;

  // 画像切り替え時に詳細メタデータを取得
  useEffect(() => {
    if (!activeImageId) return;
    setOriginalLoaded(false);
    setOriginalError(false);

    backendApi
      .getImageDetail(activeImageId)
      .then((d) => setDetail(d))
      .catch((err) => console.error("画像詳細取得失敗:", err));
  }, [activeImageId]);

  // 前後の画像プリロード
  useEffect(() => {
    if (activeImageIndex === null) return;
    const preloadIndices = [activeImageIndex - 1, activeImageIndex + 1];
    for (const idx of preloadIndices) {
      if (idx >= 0 && idx < totalImages) {
        const rec = getImageByIndex(idx);
        if (rec) {
          const img = new Image();
          img.src = getOriginalImageUrl(rec.id, rec.rev);
        }
      }
    }
  }, [activeImageIndex, totalImages, getImageByIndex]);



  // キーボードショートカット (§8.4)
  useEffect(() => {
    if (!isViewerOpen) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      // 入力フォーム等フォーカス中は無視
      if (["INPUT", "TEXTAREA"].includes((e.target as HTMLElement).tagName)) return;

      switch (e.key) {
        case "ArrowLeft":
          e.preventDefault();
          handlePrev();
          break;
        case "ArrowRight":
          e.preventDefault();
          handleNext();
          break;
        case "Escape":
          e.preventDefault();
          closeViewer();
          break;
        case "r":
        case "R":
          e.preventDefault();
          if (e.shiftKey) {
            rotateCounterClockwise();
          } else {
            rotateClockwise();
          }
          break;
        case "h":
        case "H":
          e.preventDefault();
          toggleFlipH();
          break;
        case "v":
        case "V":
          e.preventDefault();
          toggleFlipV();
          break;
        case "0":
          e.preventDefault();
          resetTransform();
          break;
        case "1":
          e.preventDefault();
          if (transform.zoom > 1.05) {
            resetTransform();
          } else {
            setZoom(get100PercentZoom());
          }
          break;
        case "+":
        case "=":
          e.preventDefault();
          setZoom(transform.zoom * 1.2);
          break;
        case "-":
          e.preventDefault();
          setZoom(transform.zoom * 0.8);
          break;
        case "i":
        case "I":
          e.preventDefault();
          setShowInfo((prev) => !prev);
          break;
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [
    isViewerOpen,
    activeImageIndex,
    totalImages,
    transform,
    closeViewer,
    rotateClockwise,
    rotateCounterClockwise,
    toggleFlipH,
    toggleFlipV,
    resetTransform,
    setZoom,
  ]);

  if (!isViewerOpen || !activeImageId) return null;

  // CSS Transform の合成 (GPU高速描画 & 高品位バイキュービック拡縮)
  // 変更理由: しっかり見る用途での高画質拡縮要求に対応するため、
  // imageRendering: auto、3D加速、バックフェイス不可視化を適用してジャギーやボケのない鮮明な拡大縮小を実現
  const transformStyle: React.CSSProperties = {
    transform: `translate3d(${transform.panX}px, ${transform.panY}px, 0) scale(${
      transform.zoom
    }) rotate(${transform.rotation}deg) scaleX(${transform.flipH ? -1 : 1}) scaleY(${
      transform.flipV ? -1 : 1
    })`,
    transformOrigin: "center center",
    transition: transform.zoom === 1 && transform.panX === 0 ? "transform 0.15s ease-out" : "none",
    imageRendering: "auto",
    WebkitBackfaceVisibility: "hidden",
    backfaceVisibility: "hidden",
  };

  const thumbUrl = activeImageId
    ? getThumbnailUrl(activeImageId, currentRecord?.rev ?? (detail?.takenAt ?? 1))
    : "";
  const originalUrl = activeImageId
    ? getOriginalImageUrl(activeImageId, currentRecord?.rev ?? (detail?.takenAt ?? 1))
    : "";

  return (
    <div className="fixed inset-0 z-50 bg-black/95 flex flex-col select-none">
      {/* 上部ツールバー */}
      <div
        className="flex items-center justify-between px-4 py-3 bg-gradient-to-b from-black/80 to-transparent z-10 text-white"
        style={{
          paddingTop: "calc(0.75rem + var(--sat))",
          paddingLeft: "calc(1rem + var(--sal))",
          paddingRight: "calc(1rem + var(--sar))",
        }}
      >
        <div className="flex items-center gap-2 text-sm text-textSecondary">
          <span>{activeImageIndex !== null ? `${activeImageIndex + 1} / ${totalImages}` : ""}</span>
          {detail && <span className="text-textPrimary font-medium truncate max-w-xs">{detail.filePath.split("\\").pop()}</span>}
        </div>

        <div className="flex items-center gap-3">
          <button
            onClick={() => rotateClockwise()}
            title={t("viewer.rotateRightTooltip")}
            className="p-2 hover:bg-white/10 rounded-full transition"
          >
            <RotateCw className="w-5 h-5" />
          </button>
          <button
            onClick={() => toggleFlipH()}
            title={t("viewer.flipHTooltip")}
            className="p-2 hover:bg-white/10 rounded-full transition text-xs font-bold"
          >
            ↔
          </button>
          <button
            onClick={() => toggleFlipV()}
            title={t("viewer.flipVTooltip")}
            className="p-2 hover:bg-white/10 rounded-full transition text-xs font-bold"
          >
            ↕
          </button>
          <button
            onClick={() => (transform.zoom > 1.05 ? resetTransform() : setZoom(get100PercentZoom()))}
            title={transform.zoom > 1.05 ? t("viewer.fitScreenTooltip") : t("viewer.originalPixelTooltip")}
            className="p-2 hover:bg-white/10 rounded-full transition"
          >
            {transform.zoom > 1.05 ? <Minimize2 className="w-5 h-5" /> : <Maximize2 className="w-5 h-5" />}
          </button>
          <button
            onClick={() => setShowInfo(!showInfo)}
            title={t("viewer.infoTooltip")}
            className={`p-2 rounded-full transition ${showInfo ? "bg-accent text-white" : "hover:bg-white/10"}`}
          >
            <Info className="w-5 h-5" />
          </button>
          {activeImageId && (
            <button
              onClick={() => openAddToBoardModal([activeImageId])}
              title={t("viewer.addToBoardTooltip")}
              className="p-2 hover:bg-white/10 rounded-full transition text-accent"
            >
              <LayoutGrid className="w-5 h-5" />
            </button>
          )}
          <button
            onClick={closeViewer}
            title={t("viewer.closeTooltip")}
            className="p-2 hover:bg-white/10 rounded-full transition text-red-400"
          >
            <X className="w-5 h-5" />
          </button>
        </div>
      </div>

      {/* メイン画像領域 */}
      <div
        ref={containerRef}
        style={{ touchAction: "none" }}
        className="flex-1 relative overflow-hidden flex items-center justify-center cursor-grab"
      >
        {/* サムネイル（即時表示プレビュー：原寸ロード完了まで、または原寸取得失敗時に表示） */}
        {thumbUrl && (!originalLoaded || originalError) && (
          <img
            src={thumbUrl}
            alt=""
            style={transformStyle}
            className={`max-h-full max-w-full object-contain transition-all pointer-events-none ${
              originalError ? "filter-none opacity-100" : "filter blur-sm opacity-90"
            }`}
          />
        )}

        {/* 原寸画像（ロード完了後にクリア表示・高品質バイキュービック拡縮） */}
        {originalUrl && !originalError && (
          <img
            ref={imgRef}
            src={originalUrl}
            alt=""
            decoding="sync"
            style={transformStyle}
            onLoad={() => setOriginalLoaded(true)}
            onError={() => setOriginalError(true)}
            className={`max-h-full max-w-full object-contain transition-opacity duration-200 pointer-events-none select-none ${
              originalLoaded ? "opacity-100" : "opacity-0 absolute"
            }`}
          />
        )}


        {/* 前後ボタン */}
        {activeImageIndex !== null && activeImageIndex > 0 && (
          <button
            onClick={handlePrev}
            style={{ left: "calc(1rem + var(--sal))" }}
            className="absolute top-1/2 -translate-y-1/2 p-3 bg-black/50 hover:bg-black/80 text-white rounded-full transition backdrop-blur z-10"
          >
            <ChevronLeft className="w-6 h-6" />
          </button>
        )}
        {activeImageIndex !== null && activeImageIndex < totalImages - 1 && (
          <button
            onClick={handleNext}
            style={{ right: "calc(1rem + var(--sar))" }}
            className="absolute top-1/2 -translate-y-1/2 p-3 bg-black/50 hover:bg-black/80 text-white rounded-full transition backdrop-blur z-10"
          >
            <ChevronRight className="w-6 h-6" />
          </button>
        )}
      </div>

      {/* 詳細情報パネル (フォーマット・可逆/非可逆・圧縮率対応) */}
      {showInfo && detail && (
        <div
          style={{
            right: "calc(1rem + var(--sar))",
            bottom: "calc(1rem + var(--sab))",
          }}
          className="absolute z-20"
        >
          <ImageDetailPanel
            detail={detail}
            onClose={() => setShowInfo(false)}
            className="w-84 max-w-[90vw] max-h-[85vh] overflow-y-auto"
          />
        </div>
      )}
    </div>
  );
};
