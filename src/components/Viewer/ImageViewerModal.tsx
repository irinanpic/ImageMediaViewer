import React, { useEffect, useRef, useState } from "react";
import {
  ChevronLeft,
  ChevronRight,
  FolderOpen,
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
import { useAppStore } from "../../store";
import type { ImageDetail } from "../../types/generated/ImageDetail";
import { useViewerGestures } from "./useViewerGestures";

export const ImageViewerModal: React.FC = () => {
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
  useViewerGestures(containerRef);

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
          img.src = getOriginalImageUrl(rec.id);
        }
      }
    }
  }, [activeImageIndex, totalImages, getImageByIndex]);

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
          setZoom(1);
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

  // CSS Transform の合成 (GPU高速描画)
  const transformStyle: React.CSSProperties = {
    transform: `translate(${transform.panX}px, ${transform.panY}px) scale(${
      transform.zoom
    }) rotate(${transform.rotation}deg) scaleX(${transform.flipH ? -1 : 1}) scaleY(${
      transform.flipV ? -1 : 1
    })`,
    transition: transform.zoom === 1 && transform.panX === 0 ? "transform 0.15s ease-out" : "none",
  };

  const thumbUrl = activeImageId
    ? getThumbnailUrl(activeImageId, currentRecord?.rev ?? (detail?.takenAt ?? 1))
    : "";
  const originalUrl = activeImageId ? getOriginalImageUrl(activeImageId) : "";

  return (
    <div className="fixed inset-0 z-50 bg-black/95 flex flex-col select-none">
      {/* 上部ツールバー */}
      <div className="flex items-center justify-between px-4 py-3 bg-gradient-to-b from-black/80 to-transparent z-10 text-white">
        <div className="flex items-center gap-2 text-sm text-textSecondary">
          <span>{activeImageIndex !== null ? `${activeImageIndex + 1} / ${totalImages}` : ""}</span>
          {detail && <span className="text-textPrimary font-medium truncate max-w-xs">{detail.filePath.split("\\").pop()}</span>}
        </div>

        <div className="flex items-center gap-3">
          <button
            onClick={() => rotateClockwise()}
            title="右に回転 (R)"
            className="p-2 hover:bg-white/10 rounded-full transition"
          >
            <RotateCw className="w-5 h-5" />
          </button>
          <button
            onClick={() => toggleFlipH()}
            title="左右反転 (H)"
            className="p-2 hover:bg-white/10 rounded-full transition text-xs font-bold"
          >
            ↔
          </button>
          <button
            onClick={() => toggleFlipV()}
            title="上下反転 (V)"
            className="p-2 hover:bg-white/10 rounded-full transition text-xs font-bold"
          >
            ↕
          </button>
          <button
            onClick={() => (transform.zoom === 1 ? resetTransform() : setZoom(1))}
            title="等倍 / フィット切替 (1 / 0)"
            className="p-2 hover:bg-white/10 rounded-full transition"
          >
            {transform.zoom === 1 ? <Minimize2 className="w-5 h-5" /> : <Maximize2 className="w-5 h-5" />}
          </button>
          <button
            onClick={() => setShowInfo(!showInfo)}
            title="情報パネル (I)"
            className={`p-2 rounded-full transition ${showInfo ? "bg-accent text-white" : "hover:bg-white/10"}`}
          >
            <Info className="w-5 h-5" />
          </button>
          {activeImageId && (
            <button
              onClick={() => openAddToBoardModal([activeImageId])}
              title="この画像をムードボードに資料として追加"
              className="p-2 hover:bg-white/10 rounded-full transition text-accent"
            >
              <LayoutGrid className="w-5 h-5" />
            </button>
          )}
          <button
            onClick={closeViewer}
            title="閉じる (Esc)"
            className="p-2 hover:bg-white/10 rounded-full transition text-red-400"
          >
            <X className="w-5 h-5" />
          </button>
        </div>
      </div>

      {/* メイン画像領域 */}
      <div
        ref={containerRef}
        className="flex-1 relative overflow-hidden flex items-center justify-center cursor-grab"
        onDoubleClick={() => (transform.zoom === 1 ? setZoom(2) : resetTransform())}
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

        {/* 原寸画像（ロード完了後にクリア表示） */}
        {originalUrl && !originalError && (
          <img
            src={originalUrl}
            alt=""
            style={transformStyle}
            onLoad={() => setOriginalLoaded(true)}
            onError={() => setOriginalError(true)}
            className={`max-h-full max-w-full object-contain transition-opacity duration-200 pointer-events-none ${
              originalLoaded ? "opacity-100" : "opacity-0 absolute"
            }`}
          />
        )}


        {/* 前後ボタン */}
        {activeImageIndex !== null && activeImageIndex > 0 && (
          <button
            onClick={handlePrev}
            className="absolute left-4 top-1/2 -translate-y-1/2 p-3 bg-black/50 hover:bg-black/80 text-white rounded-full transition backdrop-blur z-10"
          >
            <ChevronLeft className="w-6 h-6" />
          </button>
        )}
        {activeImageIndex !== null && activeImageIndex < totalImages - 1 && (
          <button
            onClick={handleNext}
            className="absolute right-4 top-1/2 -translate-y-1/2 p-3 bg-black/50 hover:bg-black/80 text-white rounded-full transition backdrop-blur z-10"
          >
            <ChevronRight className="w-6 h-6" />
          </button>
        )}
      </div>

      {/* 情報パネル (F-12) */}
      {showInfo && detail && (
        <div className="absolute right-4 bottom-4 w-80 bg-surface/95 border border-border p-4 rounded-lg shadow-xl backdrop-blur z-20 text-xs text-textSecondary space-y-2">
          <div className="flex items-center justify-between pb-2 border-b border-border text-textPrimary font-semibold">
            <span>ファイル情報</span>
            <button
              onClick={() => backendApi.revealInFileManager(detail.id)}
              className="flex items-center gap-1 text-accent hover:underline"
            >
              <FolderOpen className="w-3.5 h-3.5" />
              <span>エクスプローラ</span>
            </button>
          </div>
          <div>
            <span className="font-medium text-textPrimary">パス: </span>
            <span className="break-all">{detail.filePath}</span>
          </div>
          <div className="flex justify-between">
            <span>解像度:</span>
            <span className="text-textPrimary">
              {detail.width && detail.height ? `${detail.width} × ${detail.height}` : "不明"}
            </span>
          </div>
          <div className="flex justify-between">
            <span>サイズ:</span>
            <span className="text-textPrimary">{(detail.fileSize / 1024 / 1024).toFixed(2)} MB</span>
          </div>
          <div className="flex justify-between">
            <span>撮影日時:</span>
            <span className="text-textPrimary">
              {new Date(detail.takenAt * 1000).toISOString().replace("T", " ").substring(0, 19)}
            </span>
          </div>
          <div className="flex justify-between">
            <span>日時ソース:</span>
            <span className="text-textPrimary uppercase">{detail.takenAtSource}</span>
          </div>
        </div>
      )}
    </div>
  );
};
