import React, { useCallback, useEffect, useRef, useState } from "react";
import {
  ArrowLeft,
  ChevronDown,
  ChevronsDown,
  ChevronsUp,
  ChevronUp,
  Crop,
  FlipHorizontal,
  FlipVertical,
  Layers,
  Lock,
  RotateCcw,
  RotateCw,
  Trash2,
  Unlock,
  ZoomIn,
  ZoomOut,
} from "lucide-react";
import { backendApi } from "../../lib/ipc";
import { getThumbnailUrl, getOriginalImageUrl } from "../../lib/thumbUrl";
import { useAppStore } from "../../store";
import type { Board, BoardItem } from "../../types/board";

/**
 * 資料参照用ムードボード／キャンバス画面コンポーネント (PureRefライク)
 *
 * 変更理由: イラストやデザインの参考資料として、選択した画像を自由な位置に配置し、
 * 非破壊で拡大・縮小・回転・クリッピング（トリミング）して閲覧できるようにするため。
 */
export const MoodboardCanvas: React.FC = () => {
  const activeBoardId = useAppStore((state) => state.activeBoardId);
  const setCurrentView = useAppStore((state) => state.setCurrentView);
  const setBoards = useAppStore((state) => state.setBoards);

  const containerRef = useRef<HTMLDivElement>(null);
  const isSpacePressedRef = useRef(false);

  const [board, setBoard] = useState<Board | null>(null);
  const [items, setItems] = useState<BoardItem[]>([]);
  const [selectedItemId, setSelectedItemId] = useState<number | null>(null);
  const [isCropping, setIsCropping] = useState<boolean>(false);

  // キャンバスのカメラ状態（パン・ズーム）
  const [pan, setPan] = useState<{ x: number; y: number }>({ x: 0, y: 0 });
  const [zoom, setZoom] = useState<number>(1);
  const [rotatingDegree, setRotatingDegree] = useState<number | null>(null);

  const panRef = useRef(pan);
  const zoomRef = useRef(zoom);
  const itemsRef = useRef(items);

  // マウス操作用のRef
  const isDraggingCanvasRef = useRef(false);
  const dragStartRef = useRef<{ x: number; y: number }>({ x: 0, y: 0 });
  const isDraggingItemRef = useRef(false);
  const isResizingItemRef = useRef(false);
  const isRotatingItemRef = useRef(false);
  const itemStartPosRef = useRef<{
    x: number;
    y: number;
    width: number;
    height: number;
    scale: number;
  }>({ x: 0, y: 0, width: 0, height: 0, scale: 1 });
  const rotateStartRef = useRef<{
    screenCenterX: number;
    screenCenterY: number;
    startAngle: number;
    initialRotation: number;
  }>({ screenCenterX: 0, screenCenterY: 0, startAngle: 0, initialRotation: 0 });

  // ドラッグ矩形クリッピング用ステートとRef
  const [cropBoxDrag, setCropBoxDrag] = useState<{
    startX: number;
    startY: number;
    currentX: number;
    currentY: number;
  } | null>(null);
  const isDraggingCropRef = useRef(false);
  const cropImageRectRef = useRef<{ left: number; top: number; width: number; height: number } | null>(null);

  useEffect(() => {
    panRef.current = pan;
  }, [pan]);
  useEffect(() => {
    zoomRef.current = zoom;
  }, [zoom]);
  useEffect(() => {
    itemsRef.current = items;
  }, [items]);

  // ボードおよびアイテムデータのロード
  const loadBoardData = useCallback(async () => {
    if (!activeBoardId) return;
    try {
      const allBoards = await backendApi.getBoards();
      setBoards(allBoards);
      const current = allBoards.find((b) => b.id === activeBoardId);
      if (current) {
        setBoard(current);
        setPan({ x: current.panX, y: current.panY });
        setZoom(current.zoom || 1);
        panRef.current = { x: current.panX, y: current.panY };
        zoomRef.current = current.zoom || 1;
      }
      const boardItems = await backendApi.getBoardItems(activeBoardId);
      setItems(boardItems);
      itemsRef.current = boardItems;
    } catch (err) {
      console.error("ボードデータの取得に失敗しました:", err);
    }
  }, [activeBoardId, setBoards]);

  useEffect(() => {
    loadBoardData();
  }, [loadBoardData]);

  // カメラ状態の自動保存（デバウンス500msおよびアンマウント時即時フラッシュ）
  const saveCameraTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const scheduleSaveCamera = useCallback(
    (newPan: { x: number; y: number }, newZoom: number) => {
      if (!activeBoardId) return;
      panRef.current = newPan;
      zoomRef.current = newZoom;
      if (saveCameraTimerRef.current) clearTimeout(saveCameraTimerRef.current);
      saveCameraTimerRef.current = setTimeout(() => {
        backendApi.updateBoard(activeBoardId, {
          panX: newPan.x,
          panY: newPan.y,
          zoom: newZoom,
        }).catch((err) => console.error("カメラ位置の保存に失敗:", err));
      }, 500);
    },
    [activeBoardId]
  );

  // アンマウント時に未保存のカメラ位置を即時フラッシュ保存
  useEffect(() => {
    return () => {
      if (saveCameraTimerRef.current) {
        clearTimeout(saveCameraTimerRef.current);
      }
      if (activeBoardId) {
        backendApi.updateBoard(activeBoardId, {
          panX: panRef.current.x,
          panY: panRef.current.y,
          zoom: zoomRef.current,
        }).catch((err) => console.error("アンマウント時カメラ位置の保存に失敗:", err));
      }
    };
  }, [activeBoardId]);

  // マウスホイールによるズーム（カーソル中心）
  const handleWheel = useCallback(
    (e: React.WheelEvent) => {
      e.preventDefault();
      e.stopPropagation();

      const zoomFactor = e.deltaY < 0 ? 1.15 : 0.87;
      const newZoom = Math.max(0.08, Math.min(6.0, zoom * zoomFactor));

      if (!containerRef.current) return;
      const rect = containerRef.current.getBoundingClientRect();
      const mouseX = e.clientX - rect.left;
      const mouseY = e.clientY - rect.top;

      // マウス位置を中心に拡縮するパン調整
      const newPanX = mouseX - (mouseX - pan.x) * (newZoom / zoom);
      const newPanY = mouseY - (mouseY - pan.y) * (newZoom / zoom);

      setZoom(newZoom);
      setPan({ x: newPanX, y: newPanY });
      scheduleSaveCamera({ x: newPanX, y: newPanY }, newZoom);
    },
    [pan, zoom, scheduleSaveCamera]
  );

  // キャンバス全体のドラッグ（パン）開始
  const handleCanvasMouseDown = useCallback(
    (e: React.MouseEvent) => {
      // 中ボタンクリック、またはSpaceキー押下中、または余白クリックでパン開始
      if (e.button === 1 || e.button === 0 || isSpacePressedRef.current) {
        if (e.target === containerRef.current || (e.target as HTMLElement).dataset.canvasBg) {
          isDraggingCanvasRef.current = true;
          dragStartRef.current = { x: e.clientX - pan.x, y: e.clientY - pan.y };
          setSelectedItemId(null);
          setIsCropping(false);
        }
      }
    },
    [pan]
  );

  // アイテムのドラッグ移動開始
  const handleItemMouseDown = useCallback(
    (e: React.MouseEvent, item: BoardItem) => {
      if (e.button !== 0 || isCropping) return;
      e.stopPropagation();

      setSelectedItemId(item.id);
      if (item.isLocked) return;

      isDraggingItemRef.current = true;
      dragStartRef.current = { x: e.clientX, y: e.clientY };
      itemStartPosRef.current = {
        x: item.x,
        y: item.y,
        width: item.width,
        height: item.height,
        scale: item.scale,
      };
    },
    [isCropping]
  );

  // アイテムのリサイズ（角ハンドル）開始
  // アイテムのリサイズ（角ハンドル）開始
  const handleResizeHandleMouseDown = useCallback(
    (e: React.MouseEvent, item: BoardItem) => {
      if (e.button !== 0) return;
      e.stopPropagation();

      isResizingItemRef.current = true;
      dragStartRef.current = { x: e.clientX, y: e.clientY };
      itemStartPosRef.current = {
        x: item.x,
        y: item.y,
        width: item.width,
        height: item.height,
        scale: item.scale,
      };
    },
    []
  );

  /**
   * アイテムのドラッグ回転開始
   *
   * 変更理由: ユーザー要求に基づき、アイテムをクリック選択後にハンドルをドラッグして
   * 直感的に0〜360度の任意角度へ無段階回転（Shiftキーで15度スナップ）できるようにするため。
   *
   * @param e マウスイベント
   * @param item 対象ボードアイテム
   */
  const handleRotateHandleMouseDown = useCallback(
    (e: React.MouseEvent, item: BoardItem) => {
      if (e.button !== 0 || item.isLocked || isCropping) return;
      e.stopPropagation();

      isRotatingItemRef.current = true;

      // アイテムの中心座標（キャンバス上のローカル座標）
      const itemCenterX = item.x + (item.width * item.scale) / 2;
      const itemCenterY = item.y + (item.height * item.scale) / 2;

      // スクリーン座標系での中心点（コンテナ要素基準）
      const containerRect = containerRef.current?.getBoundingClientRect();
      const originX = (containerRect?.left ?? 0) + itemCenterX * zoom + pan.x;
      const originY = (containerRect?.top ?? 0) + itemCenterY * zoom + pan.y;

      const mouseAngleRad = Math.atan2(e.clientY - originY, e.clientX - originX);
      const mouseAngleDeg = (mouseAngleRad * 180) / Math.PI;

      rotateStartRef.current = {
        screenCenterX: originX,
        screenCenterY: originY,
        startAngle: mouseAngleDeg,
        initialRotation: item.rotation,
      };

      setRotatingDegree(Math.round(item.rotation));
    },
    [isCropping, pan, zoom]
  );

  /**
   * クリッピングモード中の画像上ドラッグ開始
   *
   * 変更理由: ユーザー要求「ドラッグで矩形を作って切り取れるようにして」に基づき、
   * 画像上をマウスドラッグして直感的に切り取り枠を定義できるようにするため。
   */
  const handleCropMouseDown = useCallback((e: React.MouseEvent) => {
    e.stopPropagation();
    e.preventDefault();
    const target = e.currentTarget as HTMLElement;
    const rect = target.getBoundingClientRect();
    cropImageRectRef.current = rect;
    isDraggingCropRef.current = true;

    const relX = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
    const relY = Math.max(0, Math.min(1, (e.clientY - rect.top) / rect.height));

    setCropBoxDrag({
      startX: relX,
      startY: relY,
      currentX: relX,
      currentY: relY,
    });
  }, []);

  // マウス移動（ドラッグ処理）
  const handleMouseMove = useCallback(
    (e: React.MouseEvent) => {
      // 0. クリッピング矩形ドラッグ中
      if (isDraggingCropRef.current && cropImageRectRef.current && selectedItemId !== null) {
        const rect = cropImageRectRef.current;
        const relX = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
        const relY = Math.max(0, Math.min(1, (e.clientY - rect.top) / rect.height));
        setCropBoxDrag((prev) => (prev ? { ...prev, currentX: relX, currentY: relY } : null));
        return;
      }

      // 1. キャンバスのパン
      if (isDraggingCanvasRef.current) {
        const nextPan = {
          x: e.clientX - dragStartRef.current.x,
          y: e.clientY - dragStartRef.current.y,
        };
        setPan(nextPan);
        scheduleSaveCamera(nextPan, zoom);
        return;
      }

      // 2. アイテムの任意角度回転ドラッグ
      if (isRotatingItemRef.current && selectedItemId !== null) {
        const { screenCenterX, screenCenterY, startAngle, initialRotation } = rotateStartRef.current;
        const currentAngleRad = Math.atan2(e.clientY - screenCenterY, e.clientX - screenCenterX);
        const currentAngleDeg = (currentAngleRad * 180) / Math.PI;
        const deltaAngle = currentAngleDeg - startAngle;

        let nextRot = (initialRotation + deltaAngle) % 360;
        if (nextRot < 0) nextRot += 360;

        // Shiftキー押下時は 15度 刻みでスナップ
        if (e.shiftKey) {
          nextRot = Math.round(nextRot / 15) * 15;
          if (nextRot >= 360) nextRot = 0;
        }

        const normalizedRot = Math.round(nextRot * 10) / 10;
        setItems((prev) =>
          prev.map((it) => (it.id === selectedItemId ? { ...it, rotation: normalizedRot } : it))
        );
        setRotatingDegree(Math.round(normalizedRot));
        return;
      }

      // 3. アイテムの移動
      if (isDraggingItemRef.current && selectedItemId !== null) {
        const dx = (e.clientX - dragStartRef.current.x) / zoom;
        const dy = (e.clientY - dragStartRef.current.y) / zoom;
        const newX = Math.round(itemStartPosRef.current.x + dx);
        const newY = Math.round(itemStartPosRef.current.y + dy);

        setItems((prev) =>
          prev.map((it) => (it.id === selectedItemId ? { ...it, x: newX, y: newY } : it))
        );
        return;
      }

      // 4. アイテムの拡縮（リサイズ）
      if (isResizingItemRef.current && selectedItemId !== null) {
        const dx = (e.clientX - dragStartRef.current.x) / zoom;
        const baseW = itemStartPosRef.current.width * itemStartPosRef.current.scale;
        const nextW = Math.max(60, baseW + dx);
        const nextScale = nextW / itemStartPosRef.current.width;

        setItems((prev) =>
          prev.map((it) => (it.id === selectedItemId ? { ...it, scale: nextScale } : it))
        );
      }
    },
    [zoom, selectedItemId, scheduleSaveCamera]
  );

  // マウスアップ（変更確定）
  const handleMouseUp = useCallback(() => {
    if (isDraggingCanvasRef.current) {
      isDraggingCanvasRef.current = false;
    }

    if (isRotatingItemRef.current && selectedItemId !== null) {
      isRotatingItemRef.current = false;
      setRotatingDegree(null);
      const current = itemsRef.current.find((it) => it.id === selectedItemId);
      if (current) {
        backendApi.updateBoardItem(current.id, {
          rotation: current.rotation,
        }).catch((err) => console.error("アイテム回転の保存に失敗:", err));
      }
    }

    if (isDraggingItemRef.current && selectedItemId !== null) {
      isDraggingItemRef.current = false;
      const current = itemsRef.current.find((it) => it.id === selectedItemId);
      if (current) {
        backendApi.updateBoardItem(current.id, {
          x: current.x,
          y: current.y,
        }).catch((err) => console.error("アイテム移動の保存に失敗:", err));
      }
    }

    if (isResizingItemRef.current && selectedItemId !== null) {
      isResizingItemRef.current = false;
      const current = itemsRef.current.find((it) => it.id === selectedItemId);
      if (current) {
        backendApi.updateBoardItem(current.id, {
          scale: current.scale,
        }).catch((err) => console.error("アイテム拡縮の保存に失敗:", err));
      }
    }

    // クリッピング矩形ドラッグ確定
    if (isDraggingCropRef.current) {
      isDraggingCropRef.current = false;
      if (cropBoxDrag && selectedItemId !== null) {
        const minX = Math.min(cropBoxDrag.startX, cropBoxDrag.currentX);
        const maxX = Math.max(cropBoxDrag.startX, cropBoxDrag.currentX);
        const minY = Math.min(cropBoxDrag.startY, cropBoxDrag.currentY);
        const maxY = Math.max(cropBoxDrag.startY, cropBoxDrag.currentY);
        const w = maxX - minX;
        const h = maxY - minY;

        // 3%以上の有意なサイズがドラッグされた場合のみクリップ枠を適用
        if (w >= 0.03 && h >= 0.03) {
          const clampedX = Math.max(0, Math.min(0.97, minX));
          const clampedY = Math.max(0, Math.min(0.97, minY));
          const clampedW = Math.max(0.03, Math.min(1.0 - clampedX, w));
          const clampedH = Math.max(0.03, Math.min(1.0 - clampedY, h));

          setItems((prev) =>
            prev.map((it) =>
              it.id === selectedItemId
                ? {
                    ...it,
                    cropX: clampedX,
                    cropY: clampedY,
                    cropW: clampedW,
                    cropH: clampedH,
                  }
                : it
            )
          );
          backendApi.updateBoardItem(selectedItemId, {
            cropX: clampedX,
            cropY: clampedY,
            cropW: clampedW,
            cropH: clampedH,
          }).catch((err) => console.error("クリッピング保存失敗:", err));
        }
      }
      setCropBoxDrag(null);
    }
  }, [selectedItemId, cropBoxDrag]);

  /**
   * 選択アイテムを最前面へ移動
   *
   * 変更理由: ユーザー要求に基づき、ムードボード上の画像の重なり順（上下関係）を自由に変更可能にするため。
   */
  const handleBringToFront = useCallback(() => {
    if (selectedItemId === null) return;
    const currentItems = itemsRef.current;
    const target = currentItems.find((it) => it.id === selectedItemId);
    if (!target) return;

    const maxZ = currentItems.reduce((max, it) => Math.max(max, it.zIndex), 0);
    const nextZ = maxZ + 1;

    setItems((prev) =>
      prev.map((it) => (it.id === target.id ? { ...it, zIndex: nextZ } : it))
    );
    backendApi.updateBoardItem(target.id, { zIndex: nextZ }).catch((err) =>
      console.error("最前面移動の保存に失敗:", err)
    );
  }, [selectedItemId]);

  /**
   * 選択アイテムを最背面へ移動
   */
  const handleSendToBack = useCallback(() => {
    if (selectedItemId === null) return;
    const currentItems = itemsRef.current;
    const target = currentItems.find((it) => it.id === selectedItemId);
    if (!target) return;

    const minZ = currentItems.reduce((min, it) => Math.min(min, it.zIndex), 0);
    const nextZ = Math.min(0, minZ - 1);

    setItems((prev) =>
      prev.map((it) => (it.id === target.id ? { ...it, zIndex: nextZ } : it))
    );
    backendApi.updateBoardItem(target.id, { zIndex: nextZ }).catch((err) =>
      console.error("最背面移動の保存に失敗:", err)
    );
  }, [selectedItemId]);

  /**
   * 選択アイテムを前面へ移動（1段階手前へ）
   */
  const handleBringForward = useCallback(() => {
    if (selectedItemId === null) return;
    const currentItems = [...itemsRef.current].sort((a, b) => a.zIndex - b.zIndex);
    const targetIdx = currentItems.findIndex((it) => it.id === selectedItemId);
    if (targetIdx === -1) return;
    if (targetIdx === currentItems.length - 1) {
      handleBringToFront();
      return;
    }

    const target = currentItems[targetIdx];
    const above = currentItems[targetIdx + 1];

    let newTargetZ = above.zIndex;
    let newAboveZ = target.zIndex;
    if (newTargetZ <= newAboveZ) {
      newTargetZ = newAboveZ + 1;
    }

    setItems((prev) =>
      prev.map((it) => {
        if (it.id === target.id) return { ...it, zIndex: newTargetZ };
        if (it.id === above.id) return { ...it, zIndex: newAboveZ };
        return it;
      })
    );
    backendApi.updateBoardItem(target.id, { zIndex: newTargetZ }).catch(console.error);
    backendApi.updateBoardItem(above.id, { zIndex: newAboveZ }).catch(console.error);
  }, [selectedItemId, handleBringToFront]);

  /**
   * 選択アイテムを背面へ移動（1段階奥へ）
   */
  const handleSendBackward = useCallback(() => {
    if (selectedItemId === null) return;
    const currentItems = [...itemsRef.current].sort((a, b) => a.zIndex - b.zIndex);
    const targetIdx = currentItems.findIndex((it) => it.id === selectedItemId);
    if (targetIdx === -1) return;
    if (targetIdx === 0) {
      handleSendToBack();
      return;
    }

    const target = currentItems[targetIdx];
    const below = currentItems[targetIdx - 1];

    let newTargetZ = below.zIndex;
    let newBelowZ = target.zIndex;
    if (newTargetZ >= newBelowZ) {
      newTargetZ = Math.max(0, newBelowZ - 1);
    }

    setItems((prev) =>
      prev.map((it) => {
        if (it.id === target.id) return { ...it, zIndex: newTargetZ };
        if (it.id === below.id) return { ...it, zIndex: newBelowZ };
        return it;
      })
    );
    backendApi.updateBoardItem(target.id, { zIndex: newTargetZ }).catch(console.error);
    backendApi.updateBoardItem(below.id, { zIndex: newBelowZ }).catch(console.error);
  }, [selectedItemId, handleSendToBack]);

  /**
   * 選択アイテムの左右反転（水平反転）
   *
   * 変更理由: ユーザー要求に基づき、ムードボード上の画像を左右反転できるようにするため。
   */
  const handleToggleFlipH = useCallback(() => {
    if (selectedItemId === null) return;
    const target = itemsRef.current.find((it) => it.id === selectedItemId);
    if (!target) return;
    const nextFlipH = !target.flipH;
    setItems((prev) =>
      prev.map((it) => (it.id === target.id ? { ...it, flipH: nextFlipH } : it))
    );
    backendApi.updateBoardItem(target.id, { flipH: nextFlipH }).catch(console.error);
  }, [selectedItemId]);

  /**
   * 選択アイテムの上下反転（垂直反転）
   *
   * 変更理由: ユーザー要求に基づき、ムードボード上の画像を上下反転できるようにするため。
   */
  const handleToggleFlipV = useCallback(() => {
    if (selectedItemId === null) return;
    const target = itemsRef.current.find((it) => it.id === selectedItemId);
    if (!target) return;
    const nextFlipV = !target.flipV;
    setItems((prev) =>
      prev.map((it) => (it.id === target.id ? { ...it, flipV: nextFlipV } : it))
    );
    backendApi.updateBoardItem(target.id, { flipV: nextFlipV }).catch(console.error);
  }, [selectedItemId]);

  /**
   * 選択アイテムのクリッピングをリセット（画像全体表示に戻す）
   */
  const handleResetCrop = useCallback(() => {
    if (selectedItemId === null) return;
    setItems((prev) =>
      prev.map((it) =>
        it.id === selectedItemId
          ? { ...it, cropX: 0, cropY: 0, cropW: 1, cropH: 1 }
          : it
      )
    );
    backendApi.updateBoardItem(selectedItemId, {
      cropX: 0,
      cropY: 0,
      cropW: 1,
      cropH: 1,
    }).catch(console.error);
  }, [selectedItemId]);

  // キーボードショートカット（PureRef互換）
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (["INPUT", "TEXTAREA"].includes((e.target as HTMLElement).tagName)) return;

      // 削除
      if (e.key === "Delete" || e.key === "Backspace") {
        if (selectedItemId !== null && !isCropping) {
          e.preventDefault();
          const target = items.find((it) => it.id === selectedItemId);
          if (target && !target.isLocked) {
            backendApi.deleteBoardItem(selectedItemId).then(() => {
              setItems((prev) => prev.filter((it) => it.id !== selectedItemId));
              setSelectedItemId(null);
            });
          }
        }
      }

      // クリッピングモード切替 (Cキー)
      if (e.key === "c" || e.key === "C") {
        if (selectedItemId !== null) {
          e.preventDefault();
          setIsCropping((prev) => !prev);
        }
      }

      // Enterでクリッピング確定
      if (e.key === "Enter" && isCropping) {
        e.preventDefault();
        setIsCropping(false);
      }

      // 重なり順（上下関係）ショートカット: PageUp/PageDown, ]/[, Ctrl+]/Ctrl+[
      if (selectedItemId !== null && !isCropping) {
        if (e.key === "PageUp" || (e.ctrlKey && e.key === "]")) {
          e.preventDefault();
          if (e.shiftKey) {
            handleBringToFront();
          } else {
            handleBringForward();
          }
        } else if (e.key === "PageDown" || (e.ctrlKey && e.key === "[")) {
          e.preventDefault();
          if (e.shiftKey) {
            handleSendToBack();
          } else {
            handleSendBackward();
          }
        } else if (e.key === "]") {
          e.preventDefault();
          handleBringForward();
        } else if (e.key === "[") {
          e.preventDefault();
          handleSendBackward();
        }

        // 画像反転ショートカット: Hキー（左右反転）、Vキー（上下反転）
        if (e.key === "h" || e.key === "H") {
          e.preventDefault();
          handleToggleFlipH();
        } else if (e.key === "v" || e.key === "V") {
          e.preventDefault();
          handleToggleFlipV();
        }
      }

      // クリッピング中のリセット (Rキー)
      if (isCropping && (e.key === "r" || e.key === "R")) {
        e.preventDefault();
        handleResetCrop();
      }

      // Spaceキーによるキャンバスパン準備
      if (e.code === "Space" && !["INPUT", "TEXTAREA"].includes((e.target as HTMLElement).tagName)) {
        isSpacePressedRef.current = true;
      }
    };

    const handleKeyUp = (e: KeyboardEvent) => {
      if (e.code === "Space") {
        isSpacePressedRef.current = false;
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    window.addEventListener("keyup", handleKeyUp);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
      window.removeEventListener("keyup", handleKeyUp);
    };
  }, [
    selectedItemId,
    isCropping,
    handleBringToFront,
    handleBringForward,
    handleSendBackward,
    handleSendToBack,
    handleToggleFlipH,
    handleToggleFlipV,
    handleResetCrop,
  ]);

  const selectedItem = items.find((it) => it.id === selectedItemId);

  return (
    <div
      ref={containerRef}
      onWheel={handleWheel}
      onMouseDown={handleCanvasMouseDown}
      onMouseMove={handleMouseMove}
      onMouseUp={handleMouseUp}
      data-canvas-bg="true"
      style={{
        backgroundColor: board?.backgroundColor || "#1a1a20",
        cursor: isDraggingCanvasRef.current ? "grabbing" : "grab",
      }}
      className="relative w-full h-full overflow-hidden select-none"
    >
      {/* 1. 上部コントロールバー */}
      <div className="absolute top-4 left-4 z-50 flex items-center gap-3 bg-surface/90 backdrop-blur-md px-4 py-2 rounded-lg border border-border/40 shadow-xl">
        <button
          onClick={() => setCurrentView("timeline")}
          className="flex items-center gap-1.5 text-xs text-textSecondary hover:text-textPrimary bg-surfaceLight/30 hover:bg-surfaceLight/60 px-3 py-1.5 rounded transition"
        >
          <ArrowLeft className="w-4 h-4" />
          タイムラインに戻る
        </button>

        <div className="h-4 w-px bg-border/40" />

        <div className="flex items-center gap-2">
          <span className="font-semibold text-sm text-textPrimary">
            {board?.name || "ムードボード"}
          </span>
          <span className="text-xs text-textSecondary">
            ({items.length} 点の資料)
          </span>
        </div>
      </div>

      {/* 2. ズーム & ツールバー（右上） */}
      <div className="absolute top-4 right-4 z-50 flex items-center gap-2 bg-surface/90 backdrop-blur-md px-3 py-1.5 rounded-lg border border-border/40 shadow-xl text-xs">
        <button
          onClick={() => {
            const nextZoom = Math.max(0.1, zoom * 0.85);
            setZoom(nextZoom);
            scheduleSaveCamera(pan, nextZoom);
          }}
          title="縮小"
          className="p-1 hover:bg-surfaceLight/50 rounded text-textSecondary hover:text-textPrimary transition"
        >
          <ZoomOut className="w-4 h-4" />
        </button>
        <span className="w-12 text-center text-textSecondary font-mono">
          {Math.round(zoom * 100)}%
        </span>
        <button
          onClick={() => {
            const nextZoom = Math.min(5.0, zoom * 1.15);
            setZoom(nextZoom);
            scheduleSaveCamera(pan, nextZoom);
          }}
          title="拡大"
          className="p-1 hover:bg-surfaceLight/50 rounded text-textSecondary hover:text-textPrimary transition"
        >
          <ZoomIn className="w-4 h-4" />
        </button>
        <button
          onClick={() => {
            setZoom(1);
            setPan({ x: 0, y: 0 });
            scheduleSaveCamera({ x: 0, y: 0 }, 1);
          }}
          title="位置とズームをリセット"
          className="px-2 py-0.5 hover:bg-surfaceLight/50 rounded text-textSecondary hover:text-textPrimary transition ml-1"
        >
          リセット
        </button>
      </div>

      {/* 3. 選択アイテム用アクションバー（画面下中央） */}
      {selectedItem && (
        <div className="absolute bottom-6 left-1/2 -translate-x-1/2 z-50 flex items-center gap-3 bg-surface/95 backdrop-blur-md px-4 py-2 rounded-xl border border-border/60 shadow-2xl">
          <button
            onClick={() => setIsCropping((c) => !c)}
            className={`flex items-center gap-1.5 px-3 py-1.5 rounded text-xs font-medium transition ${
              isCropping
                ? "bg-accent text-white shadow-md shadow-accent/30"
                : "bg-surfaceLight/40 hover:bg-surfaceLight/80 text-textPrimary"
            }`}
          >
            <Crop className="w-4 h-4" />
            {isCropping ? "クリップ完了 (Enter)" : "クリッピング (C)"}
          </button>

          <div className="flex items-center gap-1 bg-surfaceLight/30 px-2 py-1 rounded">
            <span className="text-[11px] font-mono text-textSecondary w-10 text-center" title="現在の回転角度">
              {Math.round(selectedItem.rotation)}°
            </span>
            <button
              onClick={() => {
                const nextRot = (selectedItem.rotation + 90) % 360;
                setItems((prev) =>
                  prev.map((it) => (it.id === selectedItem.id ? { ...it, rotation: nextRot } : it))
                );
                backendApi.updateBoardItem(selectedItem.id, { rotation: nextRot });
              }}
              title="90度回転"
              className="p-1 hover:bg-surfaceLight/50 rounded text-textSecondary hover:text-textPrimary transition"
            >
              <RotateCw className="w-3.5 h-3.5" />
            </button>
            {selectedItem.rotation !== 0 && (
              <button
                onClick={() => {
                  setItems((prev) =>
                    prev.map((it) => (it.id === selectedItem.id ? { ...it, rotation: 0 } : it))
                  );
                  backendApi.updateBoardItem(selectedItem.id, { rotation: 0 });
                }}
                title="回転を0°にリセット"
                className="text-[10px] text-accent hover:underline px-1"
              >
                0°
              </button>
            )}
          </div>

          {/* 画像反転コントロール（左右反転・上下反転） */}
          <div
            className="flex items-center gap-0.5 bg-surfaceLight/30 px-1 py-0.5 rounded border border-border/40"
            title="画像の反転"
          >
            <button
              onClick={handleToggleFlipH}
              className={`p-1 rounded transition cursor-pointer ${
                selectedItem.flipH
                  ? "bg-accent text-white shadow-xs"
                  : "hover:bg-surfaceLight/50 text-textSecondary hover:text-textPrimary"
              }`}
              title="左右反転 (H)"
            >
              <FlipHorizontal className="w-3.5 h-3.5" />
            </button>
            <button
              onClick={handleToggleFlipV}
              className={`p-1 rounded transition cursor-pointer ${
                selectedItem.flipV
                  ? "bg-accent text-white shadow-xs"
                  : "hover:bg-surfaceLight/50 text-textSecondary hover:text-textPrimary"
              }`}
              title="上下反転 (V)"
            >
              <FlipVertical className="w-3.5 h-3.5" />
            </button>
          </div>

          {/* クリッピング中専用: リセットボタン */}
          {isCropping && (
            <button
              onClick={handleResetCrop}
              className="flex items-center gap-1 bg-surfaceLight/40 hover:bg-surfaceLight/80 text-textSecondary hover:text-textPrimary px-2.5 py-1 rounded text-xs transition border border-border/40"
              title="切り抜きを解除して元の全体表示に戻す (R)"
            >
              <RotateCcw className="w-3 h-3" />
              リセット
            </button>
          )}

          <button
            onClick={() => {
              const nextLocked = !selectedItem.isLocked;
              setItems((prev) =>
                prev.map((it) => (it.id === selectedItem.id ? { ...it, isLocked: nextLocked } : it))
              );
              backendApi.updateBoardItem(selectedItem.id, { isLocked: nextLocked });
            }}
            title={selectedItem.isLocked ? "ロック解除" : "位置固定ロック"}
            className="p-1.5 hover:bg-surfaceLight/50 rounded text-textSecondary hover:text-textPrimary transition"
          >
            {selectedItem.isLocked ? <Lock className="w-4 h-4 text-accent" /> : <Unlock className="w-4 h-4" />}
          </button>

          <div className="h-4 w-px bg-border/40" />

          {/* 上下関係（重なり順 / Z-Index）変更コントロール */}
          <div
            className="flex items-center gap-0.5 bg-surfaceLight/30 px-1 py-0.5 rounded border border-border/40"
            title="画像の重なり順（上下位置関係）を変更"
          >
            <button
              onClick={handleSendToBack}
              className="p-1 hover:bg-surfaceLight/50 rounded text-textSecondary hover:text-textPrimary transition cursor-pointer"
              title="最背面へ移動 (PageDown / Ctrl+Shift+[)"
            >
              <ChevronsDown className="w-3.5 h-3.5" />
            </button>
            <button
              onClick={handleSendBackward}
              className="p-1 hover:bg-surfaceLight/50 rounded text-textSecondary hover:text-textPrimary transition cursor-pointer"
              title="背面へ移動 ([ / Ctrl+[)"
            >
              <ChevronDown className="w-3.5 h-3.5" />
            </button>
            <div
              className="text-[10px] text-textSecondary font-mono px-1.5 select-none flex items-center gap-1"
              title={`重なり順: レイヤー ${selectedItem.zIndex}`}
            >
              <Layers className="w-3 h-3 text-accent" />
              <span>{selectedItem.zIndex}</span>
            </div>
            <button
              onClick={handleBringForward}
              className="p-1 hover:bg-surfaceLight/50 rounded text-textSecondary hover:text-textPrimary transition cursor-pointer"
              title="前面へ移動 (] / Ctrl+])"
            >
              <ChevronUp className="w-3.5 h-3.5" />
            </button>
            <button
              onClick={handleBringToFront}
              className="p-1 hover:bg-surfaceLight/50 rounded text-textSecondary hover:text-textPrimary transition cursor-pointer"
              title="最前面へ移動 (PageUp / Ctrl+Shift+])"
            >
              <ChevronsUp className="w-3.5 h-3.5" />
            </button>
          </div>

          <div className="h-4 w-px bg-border/40" />

          <button
            onClick={() => {
              backendApi.deleteBoardItem(selectedItem.id).then(() => {
                setItems((prev) => prev.filter((it) => it.id !== selectedItem.id));
                setSelectedItemId(null);
                setIsCropping(false);
              });
            }}
            title="ボードから削除 (Delete)"
            className="p-1.5 hover:bg-red-500/20 text-textSecondary hover:text-red-400 rounded transition"
          >
            <Trash2 className="w-4 h-4" />
          </button>
        </div>
      )}

      {/* 4. キャンバスワールド（アイテム群のトランスフォームコンテナ） */}
      <div
        data-canvas-bg="true"
        style={{
          transform: `translate(${pan.x}px, ${pan.y}px) scale(${zoom})`,
          transformOrigin: "0 0",
        }}
        className="absolute inset-0 pointer-events-none"
      >
        {items.map((item) => {
          const isSelected = selectedItemId === item.id;
          const displayWidth = item.width * item.scale;
          const displayHeight = item.height * item.scale;

          // 非破壊クリッピングの計算
          // cropX, cropY, cropW, cropH は 0.0〜1.0 の正規化比率
          const clipLeft = (item.cropX * 100).toFixed(2);
          const clipTop = (item.cropY * 100).toFixed(2);
          const clipRight = ((1.0 - (item.cropX + item.cropW)) * 100).toFixed(2);
          const clipBottom = ((1.0 - (item.cropY + item.cropH)) * 100).toFixed(2);

          const thumbSrc = getThumbnailUrl(item.imageId, item.rev);
          const rawSrc = getOriginalImageUrl(item.imageId);
          // ズームインしている時は高解像度画像を使用
          const imgSrc = zoom > 1.2 ? rawSrc : thumbSrc;

          return (
            <div
              key={item.id}
              onMouseDown={(e) => handleItemMouseDown(e, item)}
              onDoubleClick={() => {
                setSelectedItemId(item.id);
                setIsCropping(true);
              }}
              style={{
                position: "absolute",
                left: item.x,
                top: item.y,
                width: displayWidth,
                height: displayHeight,
                zIndex: item.zIndex,
                transform: `rotate(${item.rotation}deg)`,
                opacity: item.opacity,
                cursor: item.isLocked ? "default" : "move",
              }}
              className={`pointer-events-auto group select-none transition-shadow ${
                isSelected
                  ? "ring-2 ring-accent shadow-2xl shadow-accent/20"
                  : "hover:ring-1 hover:ring-border/80 shadow-md"
              }`}
            >
              {/* 画像描画コンテナ（非破壊クリッピング適用） */}
              <div
                style={{
                  width: "100%",
                  height: "100%",
                  clipPath: isCropping && isSelected
                    ? "none"
                    : `inset(${clipTop}% ${clipRight}% ${clipBottom}% ${clipLeft}%)`,
                }}
                className="w-full h-full overflow-hidden bg-surface rounded"
              >
                <img
                  src={imgSrc}
                  alt=""
                  draggable={false}
                  decoding="async"
                  style={{
                    transform: `scale(${item.flipH ? -1 : 1}, ${item.flipV ? -1 : 1})`,
                    opacity: isCropping && isSelected ? 0.45 : 1,
                  }}
                  className="w-full h-full object-cover pointer-events-none transition-opacity"
                />
              </div>

              {/* クリッピング編集モード時のドラッグ矩形選択オーバーレイ */}
              {isCropping && isSelected && (
                <div
                  onMouseDown={handleCropMouseDown}
                  className="absolute inset-0 pointer-events-auto cursor-crosshair bg-black/30"
                  title="ドラッグして切り取る矩形領域を指定してください"
                >
                  {/* ヘッダーガイドバッジ */}
                  <div className="absolute -top-7 left-0 bg-accent text-[11px] text-white px-2 py-0.5 rounded font-medium shadow flex items-center gap-1.5 whitespace-nowrap">
                    <Crop className="w-3 h-3" />
                    <span>ドラッグして切り取り領域を指定 (Enterで完了)</span>
                  </div>

                  {/* 確定済みの既存クリッピング枠（ドラッグ中でないときに表示） */}
                  {!cropBoxDrag && (item.cropW < 0.999 || item.cropH < 0.999 || item.cropX > 0.001 || item.cropY > 0.001) && (
                    <div
                      style={{
                        left: `${item.cropX * 100}%`,
                        top: `${item.cropY * 100}%`,
                        width: `${item.cropW * 100}%`,
                        height: `${item.cropH * 100}%`,
                      }}
                      className="absolute border-2 border-accent bg-accent/15 shadow-sm pointer-events-none"
                    >
                      <div className="absolute top-1 left-1 bg-accent/90 text-[9px] text-white px-1 py-0.2 rounded font-mono">
                        現在の範囲
                      </div>
                    </div>
                  )}

                  {/* マウスドラッグ中のリアルタイム選択矩形 */}
                  {cropBoxDrag && (
                    <div
                      style={{
                        left: `${Math.min(cropBoxDrag.startX, cropBoxDrag.currentX) * 100}%`,
                        top: `${Math.min(cropBoxDrag.startY, cropBoxDrag.currentY) * 100}%`,
                        width: `${Math.abs(cropBoxDrag.currentX - cropBoxDrag.startX) * 100}%`,
                        height: `${Math.abs(cropBoxDrag.currentY - cropBoxDrag.startY) * 100}%`,
                      }}
                      className="absolute border-2 border-dashed border-white bg-accent/30 shadow-2xl pointer-events-none"
                    >
                      {/* コーナーハンドル装飾 */}
                      <div className="absolute -top-1 -left-1 w-2.5 h-2.5 bg-accent border border-white" />
                      <div className="absolute -top-1 -right-1 w-2.5 h-2.5 bg-accent border border-white" />
                      <div className="absolute -bottom-1 -left-1 w-2.5 h-2.5 bg-accent border border-white" />
                      <div className="absolute -bottom-1 -right-1 w-2.5 h-2.5 bg-accent border border-white" />
                    </div>
                  )}
                </div>
              )}

              {/* 回転ハンドル（上辺中央の上部に配置） */}
              {isSelected && !item.isLocked && !isCropping && (
                <div
                  className="absolute -top-7 left-1/2 -translate-x-1/2 flex flex-col items-center pointer-events-auto select-none z-20"
                  title="ドラッグして任意角度に回転 (Shift+ドラッグで15°スナップ)"
                >
                  {/* 回転中の角度ツールチップ */}
                  {rotatingDegree !== null && isRotatingItemRef.current && (
                    <div className="absolute -top-6 bg-black/90 text-white font-mono text-[10px] px-1.5 py-0.5 rounded shadow whitespace-nowrap">
                      {rotatingDegree}°
                    </div>
                  )}
                  {/* 回転ハンドルボタン */}
                  <div
                    onMouseDown={(e) => handleRotateHandleMouseDown(e, item)}
                    className="w-5 h-5 bg-white text-accent hover:bg-accent hover:text-white border-2 border-accent rounded-full shadow-md flex items-center justify-center cursor-grab active:cursor-grabbing hover:scale-110 transition-transform"
                  >
                    <RotateCw className="w-3 h-3" />
                  </div>
                  {/* ハンドルとアイテム上辺を繋ぐバー */}
                  <div className="w-0.5 h-2 bg-accent" />
                </div>
              )}

              {/* リサイズハンドル（右下） */}
              {isSelected && !item.isLocked && !isCropping && (
                <div
                  onMouseDown={(e) => handleResizeHandleMouseDown(e, item)}
                  className="absolute -bottom-2 -right-2 w-4 h-4 bg-accent border-2 border-white rounded-full cursor-se-resize shadow-md hover:scale-125 transition-transform"
                />
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
};
