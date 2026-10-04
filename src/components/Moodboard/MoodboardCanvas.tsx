import React, { useCallback, useEffect, useRef, useState } from "react";
import {
  ArrowLeft,
  ChevronDown,
  ChevronsDown,
  ChevronsUp,
  ChevronUp,
  Crop,
  Edit3,
  FlipHorizontal,
  FlipVertical,
  Info,
  Layers,
  LayoutGrid,
  Lock,
  RotateCcw,
  RotateCw,
  StickyNote,
  Trash2,
  Unlock,
  ZoomIn,
  ZoomOut,
} from "lucide-react";
import { backendApi } from "../../lib/ipc";
import { getThumbnailUrl, getOriginalImageUrl } from "../../lib/thumbUrl";
import { useAppStore } from "../../store";
import type { Board, BoardItem, BoardNote } from "../../types/board";
import type { ImageDetail } from "../../types/generated/ImageDetail";
import { ImageDetailPanel } from "../Common/ImageDetailPanel";

/** 付箋メモのカラーパレット定義 */
const NOTE_COLORS = [
  { label: "イエロー", bg: "#fef08a", text: "#1c1917", border: "#fde047" },
  { label: "スカイブルー", bg: "#bae6fd", text: "#0c4a6e", border: "#7dd3fc" },
  { label: "ミントグリーン", bg: "#bbf7d0", text: "#064e3b", border: "#86efac" },
  { label: "ピンク", bg: "#fbcfe8", text: "#831843", border: "#f472b6" },
  { label: "パープル", bg: "#e9d5ff", text: "#581c87", border: "#c084fc" },
  { label: "ダーク", bg: "#27272a", text: "#f4f4f5", border: "#3f3f46" },
];

/**
 * 資料参照用ムードボード／キャンバス画面コンポーネント (PureRefライク)
 *
 * 変更理由: イラストやデザインの参考資料として、選択した画像を自由な位置に配置し、
 * 非破壊で拡大・縮小・回転・クリッピング（トリミング）して閲覧できるようにするため。
 * また、画像詳細情報の確認やテキストメモ（付箋）の自由配置・編集機能を提供する。
 */
export const MoodboardCanvas: React.FC = () => {
  const activeBoardId = useAppStore((state) => state.activeBoardId);
  const setCurrentView = useAppStore((state) => state.setCurrentView);
  const setBoards = useAppStore((state) => state.setBoards);

  const containerRef = useRef<HTMLDivElement>(null);
  const isSpacePressedRef = useRef(false);

  const [board, setBoard] = useState<Board | null>(null);
  const [items, setItems] = useState<BoardItem[]>([]);
  const [notes, setNotes] = useState<BoardNote[]>([]);
  const [selectedItemId, setSelectedItemId] = useState<number | null>(null);
  const [selectedNoteId, setSelectedNoteId] = useState<number | null>(null);
  const [editingNoteId, setEditingNoteId] = useState<number | null>(null);
  const [editingText, setEditingText] = useState<string>("");
  const [isCropping, setIsCropping] = useState<boolean>(false);

  // 詳細情報パネル用ステート
  const [showImageDetail, setShowImageDetail] = useState<boolean>(false);
  const [selectedImageDetail, setSelectedImageDetail] = useState<ImageDetail | null>(null);

  // キャンバスのカメラ状態（パン・ズーム）
  const [pan, setPan] = useState<{ x: number; y: number }>({ x: 0, y: 0 });
  const [zoom, setZoom] = useState<number>(1);
  const [rotatingDegree, setRotatingDegree] = useState<number | null>(null);

  const panRef = useRef(pan);
  const zoomRef = useRef(zoom);
  const itemsRef = useRef(items);
  const notesRef = useRef(notes);

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

  // メモ用ドラッグ・リサイズRef
  const isDraggingNoteRef = useRef(false);
  const isResizingNoteRef = useRef(false);
  const draggingItemIdRef = useRef<number | null>(null);
  const currentItemPosRef = useRef<{ x: number; y: number }>({ x: 0, y: 0 });
  const draggingNoteIdRef = useRef<number | null>(null);
  const resizingNoteIdRef = useRef<number | null>(null);
  const currentNotePosRef = useRef<{ x: number; y: number }>({ x: 0, y: 0 });
  const currentNoteSizeRef = useRef<{ width: number; height: number }>({ width: 0, height: 0 });
  const noteStartPosRef = useRef<{
    x: number;
    y: number;
    width: number;
    height: number;
  }>({ x: 0, y: 0, width: 0, height: 0 });

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
  useEffect(() => {
    notesRef.current = notes;
  }, [notes]);


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
      const [boardItems, boardNotes] = await Promise.all([
        backendApi.getBoardItems(activeBoardId),
        backendApi.getBoardNotes(activeBoardId),
      ]);
      setItems(boardItems);
      itemsRef.current = boardItems;
      setNotes(boardNotes);
      notesRef.current = boardNotes;
    } catch (err) {
      console.error("ボードデータの取得に失敗しました:", err);
    }
  }, [activeBoardId, setBoards]);

  useEffect(() => {
    loadBoardData();
  }, [loadBoardData]);

  // 選択画像アイテムの詳細情報を自動取得
  useEffect(() => {
    if (selectedItemId !== null) {
      const item = items.find((it) => it.id === selectedItemId);
      if (item) {
        backendApi
          .getImageDetail(item.imageId)
          .then((d) => setSelectedImageDetail(d))
          .catch((err) => console.error("画像詳細取得失敗:", err));
      } else {
        setSelectedImageDetail(null);
      }
    } else {
      setSelectedImageDetail(null);
      setShowImageDetail(false);
    }
  }, [selectedItemId, items]);

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
          setSelectedNoteId(null);
          setEditingNoteId(null);
          setIsCropping(false);
          setShowImageDetail(false);
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
      setSelectedNoteId(null);
      setEditingNoteId(null);
      if (item.isLocked) return;

      isDraggingItemRef.current = true;
      draggingItemIdRef.current = item.id;
      dragStartRef.current = { x: e.clientX, y: e.clientY };
      itemStartPosRef.current = {
        x: item.x,
        y: item.y,
        width: item.width,
        height: item.height,
        scale: item.scale,
      };
      currentItemPosRef.current = { x: item.x, y: item.y };
    },
    [isCropping]
  );

  // アイテムのリサイズ（角ハンドル）開始
  const handleResizeHandleMouseDown = useCallback(
    (e: React.MouseEvent, item: BoardItem) => {
      if (e.button !== 0) return;
      e.stopPropagation();
      e.preventDefault();

      isResizingItemRef.current = true;
      draggingItemIdRef.current = item.id;
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
   * メモのドラッグ移動開始
   */
  const handleNoteMouseDown = useCallback(
    (e: React.MouseEvent, note: BoardNote) => {
      if (e.button !== 0) return;
      e.stopPropagation();

      setSelectedNoteId(note.id);
      setSelectedItemId(null);
      setShowImageDetail(false);
      if (note.isLocked) return;

      // 編集中の場合はテキストエリアの入力を優先し、ドラッグを開始しない
      if (editingNoteId === note.id) {
        return;
      }

      // ブラウザ標準の要素ドラッグ・テキスト選択の競合を抑止
      e.preventDefault();

      isDraggingNoteRef.current = true;
      draggingNoteIdRef.current = note.id;
      dragStartRef.current = { x: e.clientX, y: e.clientY };
      noteStartPosRef.current = {
        x: note.x,
        y: note.y,
        width: note.width,
        height: note.height,
      };
      currentNotePosRef.current = { x: note.x, y: note.y };
    },
    [editingNoteId]
  );

  /**
   * メモのリサイズ開始
   */
  const handleNoteResizeMouseDown = useCallback(
    (e: React.MouseEvent, note: BoardNote) => {
      if (e.button !== 0 || note.isLocked) return;
      e.stopPropagation();
      e.preventDefault();

      isResizingNoteRef.current = true;
      resizingNoteIdRef.current = note.id;
      dragStartRef.current = { x: e.clientX, y: e.clientY };
      noteStartPosRef.current = {
        x: note.x,
        y: note.y,
        width: note.width,
        height: note.height,
      };
      currentNoteSizeRef.current = { width: note.width, height: note.height };
    },
    []
  );

  /**
   * 新規メモ作成（キャンバス中央に配置）
   */
  const handleCreateNote = useCallback(async () => {
    if (!activeBoardId) return;
    const container = containerRef.current;
    const containerWidth = container ? container.clientWidth : 800;
    const containerHeight = container ? container.clientHeight : 600;

    // 現在のカメラ中央に配置
    const centerX = -pan.x / zoom + (containerWidth / 2) / zoom - 120;
    const centerY = -pan.y / zoom + (containerHeight / 2) / zoom - 80;

    try {
      const newNote = await backendApi.createBoardNote({
        boardId: activeBoardId,
        text: "新規メモ",
        x: Math.round(centerX),
        y: Math.round(centerY),
        width: 240,
        height: 160,
        color: "#fef08a",
        fontSize: 14,
      });
      setNotes((prev) => [...prev, newNote]);
      setSelectedItemId(null);
      setSelectedNoteId(newNote.id);
      setEditingNoteId(newNote.id);
      setEditingText("新規メモ");
    } catch (err) {
      console.error("メモ作成失敗:", err);
    }
  }, [activeBoardId, pan, zoom]);


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
      if (isDraggingItemRef.current && draggingItemIdRef.current !== null) {
        const itemId = draggingItemIdRef.current;
        const dx = (e.clientX - dragStartRef.current.x) / zoom;
        const dy = (e.clientY - dragStartRef.current.y) / zoom;
        const newX = Math.round(itemStartPosRef.current.x + dx);
        const newY = Math.round(itemStartPosRef.current.y + dy);

        currentItemPosRef.current = { x: newX, y: newY };
        setItems((prev) => {
          const next = prev.map((it) => (it.id === itemId ? { ...it, x: newX, y: newY } : it));
          itemsRef.current = next;
          return next;
        });
        return;
      }

      // 4. アイテムの拡縮（リサイズ）
      if (isResizingItemRef.current && draggingItemIdRef.current !== null) {
        const itemId = draggingItemIdRef.current;
        const dx = (e.clientX - dragStartRef.current.x) / zoom;
        const baseW = itemStartPosRef.current.width * itemStartPosRef.current.scale;
        const nextW = Math.max(60, baseW + dx);
        const nextScale = nextW / itemStartPosRef.current.width;

        setItems((prev) => {
          const next = prev.map((it) => (it.id === itemId ? { ...it, scale: nextScale } : it));
          itemsRef.current = next;
          return next;
        });
      }

      // 5. メモの移動
      if (isDraggingNoteRef.current && draggingNoteIdRef.current !== null) {
        const noteId = draggingNoteIdRef.current;
        const dx = (e.clientX - dragStartRef.current.x) / zoom;
        const dy = (e.clientY - dragStartRef.current.y) / zoom;
        const newX = Math.round(noteStartPosRef.current.x + dx);
        const newY = Math.round(noteStartPosRef.current.y + dy);

        currentNotePosRef.current = { x: newX, y: newY };
        setNotes((prev) => {
          const next = prev.map((n) => (n.id === noteId ? { ...n, x: newX, y: newY } : n));
          notesRef.current = next;
          return next;
        });
        return;
      }

      // 6. メモの拡縮（リサイズ）
      if (isResizingNoteRef.current && resizingNoteIdRef.current !== null) {
        const noteId = resizingNoteIdRef.current;
        const dx = (e.clientX - dragStartRef.current.x) / zoom;
        const dy = (e.clientY - dragStartRef.current.y) / zoom;
        const nextW = Math.max(120, Math.round(noteStartPosRef.current.width + dx));
        const nextH = Math.max(80, Math.round(noteStartPosRef.current.height + dy));

        currentNoteSizeRef.current = { width: nextW, height: nextH };
        setNotes((prev) => {
          const next = prev.map((n) => (n.id === noteId ? { ...n, width: nextW, height: nextH } : n));
          notesRef.current = next;
          return next;
        });
      }
    },
    [zoom, scheduleSaveCamera]
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

    if (isDraggingItemRef.current) {
      isDraggingItemRef.current = false;
      const itemId = draggingItemIdRef.current;
      draggingItemIdRef.current = null;
      if (itemId !== null) {
        const { x, y } = currentItemPosRef.current;
        backendApi.updateBoardItem(itemId, { x, y }).catch((err) =>
          console.error("アイテム移動の保存に失敗:", err)
        );
      }
    }

    if (isResizingItemRef.current) {
      isResizingItemRef.current = false;
      const itemId = draggingItemIdRef.current;
      draggingItemIdRef.current = null;
      if (itemId !== null) {
        const current = itemsRef.current.find((it) => it.id === itemId);
        if (current) {
          backendApi.updateBoardItem(current.id, {
            scale: current.scale,
          }).catch((err) => console.error("アイテム拡縮の保存に失敗:", err));
        }
      }
    }

    // メモ移動の確定保存（RefのIDと最新座標を使って確実に保存）
    if (isDraggingNoteRef.current) {
      isDraggingNoteRef.current = false;
      const noteId = draggingNoteIdRef.current;
      draggingNoteIdRef.current = null;
      if (noteId !== null) {
        const { x, y } = currentNotePosRef.current;
        backendApi.updateBoardNote(noteId, { x, y }).catch((err) =>
          console.error("メモ移動の保存に失敗:", err)
        );
      }
    }

    // メモリサイズの確定保存
    if (isResizingNoteRef.current) {
      isResizingNoteRef.current = false;
      const noteId = resizingNoteIdRef.current;
      resizingNoteIdRef.current = null;
      if (noteId !== null) {
        const { width, height } = currentNoteSizeRef.current;
        backendApi.updateBoardNote(noteId, { width, height }).catch((err) =>
          console.error("メモ拡縮の保存に失敗:", err)
        );
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

  // -------------------------------------------------------------
  // メモ（付箋）操作ハンドラ群
  // -------------------------------------------------------------
  /**
   * メモのテキスト保存
   */
  const handleSaveNoteText = useCallback((noteId: number, text: string) => {
    setNotes((prev) =>
      prev.map((n) => (n.id === noteId ? { ...n, text } : n))
    );
    setEditingNoteId(null);
    backendApi.updateBoardNote(noteId, { text }).catch((err) =>
      console.error("メモテキストの保存に失敗:", err)
    );
  }, []);

  /**
   * メモのカラー変更
   */
  const handleChangeNoteColor = useCallback((noteId: number, color: string) => {
    setNotes((prev) =>
      prev.map((n) => (n.id === noteId ? { ...n, color } : n))
    );
    backendApi.updateBoardNote(noteId, { color }).catch((err) =>
      console.error("メモカラーの保存に失敗:", err)
    );
  }, []);

  /**
   * メモのロック状態切替
   */
  const handleToggleNoteLock = useCallback((noteId: number) => {
    const target = notesRef.current.find((n) => n.id === noteId);
    if (!target) return;
    const nextLocked = !target.isLocked;
    setNotes((prev) =>
      prev.map((n) => (n.id === noteId ? { ...n, isLocked: nextLocked } : n))
    );
    backendApi.updateBoardNote(noteId, { isLocked: nextLocked }).catch((err) =>
      console.error("メモロックの保存に失敗:", err)
    );
  }, []);

  /**
   * メモの削除
   */
  const handleDeleteNote = useCallback((noteId: number) => {
    const target = notesRef.current.find((n) => n.id === noteId);
    if (!target || target.isLocked) return;
    backendApi.deleteBoardNote(noteId).then(() => {
      setNotes((prev) => prev.filter((n) => n.id !== noteId));
      if (selectedNoteId === noteId) {
        setSelectedNoteId(null);
        setEditingNoteId(null);
      }
    }).catch((err) => console.error("メモ削除に失敗:", err));
  }, [selectedNoteId]);

  /**
   * メモを最前面へ移動
   */
  const handleBringNoteToFront = useCallback(() => {
    if (selectedNoteId === null) return;
    const target = notesRef.current.find((n) => n.id === selectedNoteId);
    if (!target) return;

    const maxItemZ = itemsRef.current.reduce((max, it) => Math.max(max, it.zIndex), 0);
    const maxNoteZ = notesRef.current.reduce((max, n) => Math.max(max, n.zIndex), 0);
    const nextZ = Math.max(maxItemZ, maxNoteZ) + 1;

    setNotes((prev) =>
      prev.map((n) => (n.id === target.id ? { ...n, zIndex: nextZ } : n))
    );
    backendApi.updateBoardNote(target.id, { zIndex: nextZ }).catch(console.error);
  }, [selectedNoteId]);

  /**
   * メモを最背面へ移動
   */
  const handleSendNoteToBack = useCallback(() => {
    if (selectedNoteId === null) return;
    const target = notesRef.current.find((n) => n.id === selectedNoteId);
    if (!target) return;

    const minItemZ = itemsRef.current.reduce((min, it) => Math.min(min, it.zIndex), 0);
    const minNoteZ = notesRef.current.reduce((min, n) => Math.min(min, n.zIndex), 0);
    const nextZ = Math.min(0, Math.min(minItemZ, minNoteZ) - 1);

    setNotes((prev) =>
      prev.map((n) => (n.id === target.id ? { ...n, zIndex: nextZ } : n))
    );
    backendApi.updateBoardNote(target.id, { zIndex: nextZ }).catch(console.error);
  }, [selectedNoteId]);

  /**
   * 画像詳細情報パネルの表示切替
   */
  const handleToggleImageDetail = useCallback(() => {
    if (selectedItemId === null) return;
    setShowImageDetail((prev) => !prev);
  }, [selectedItemId]);

  /**
   * ボード上の全要素（画像アイテムおよびテキストメモ）の自動並び替え
   *
   * 変更理由: ユーザー要求に基づき、ムードボード上の画像やメモが互いに被らないよう、
   * 手動操作で一括して自動整列（行ベースのパッキング配置）し、画面中央にフィットさせるため。
   */
  const handleAutoArrange = useCallback(async () => {
    const currentItems = itemsRef.current;
    const currentNotes = notesRef.current;
    if (currentItems.length === 0 && currentNotes.length === 0) return;

    // 全要素を統合し、z_index 順（昇順）に並べる
    type SortableElement =
      | { type: "item"; raw: BoardItem; width: number; height: number; zIndex: number }
      | { type: "note"; raw: BoardNote; width: number; height: number; zIndex: number };

    const elements: SortableElement[] = [
      ...currentItems.map((it) => ({
        type: "item" as const,
        raw: it,
        width: Math.round(it.width * it.scale),
        height: Math.round(it.height * it.scale),
        zIndex: it.zIndex,
      })),
      ...currentNotes.map((n) => ({
        type: "note" as const,
        raw: n,
        width: Math.round(n.width * n.scale),
        height: Math.round(n.height * n.scale),
        zIndex: n.zIndex,
      })),
    ];

    // z_index 昇順でソート（追加順・レイヤー順の自然な並び）
    elements.sort((a, b) => a.zIndex - b.zIndex);

    const GAP = 28;
    const totalCount = elements.length;

    // 列数の動的決定（アイテム数に応じて 2〜6 列）
    const targetCols = Math.max(2, Math.min(6, Math.ceil(Math.sqrt(totalCount * 1.3))));
    const avgWidth = elements.reduce((sum, el) => sum + el.width, 0) / totalCount;
    const maxRowWidth = Math.max(800, targetCols * (avgWidth + GAP));

    let curX = 0;
    let curY = 0;
    let rowMaxHeight = 0;
    let maxOverallWidth = 0;

    const itemUpdates: { id: number; x: number; y: number }[] = [];
    const noteUpdates: { id: number; x: number; y: number }[] = [];

    for (const el of elements) {
      // 最初の要素でなく、行幅を超える場合は改行
      if (curX > 0 && curX + el.width > maxRowWidth) {
        maxOverallWidth = Math.max(maxOverallWidth, curX - GAP);
        curX = 0;
        curY += rowMaxHeight + GAP;
        rowMaxHeight = 0;
      }

      if (el.type === "item") {
        itemUpdates.push({ id: el.raw.id, x: curX, y: curY });
      } else {
        noteUpdates.push({ id: el.raw.id, x: curX, y: curY });
      }

      curX += el.width + GAP;
      rowMaxHeight = Math.max(rowMaxHeight, el.height);
    }

    maxOverallWidth = Math.max(maxOverallWidth, curX - GAP);
    const maxOverallHeight = curY + rowMaxHeight;

    // ステートとRefを即座に更新（rotationも0°にリセットして重なりを完全排除）
    const newItemPosMap = new Map(itemUpdates.map((u) => [u.id, u]));
    const newNotePosMap = new Map(noteUpdates.map((u) => [u.id, u]));

    const nextItems = currentItems.map((it) => {
      const pos = newItemPosMap.get(it.id);
      return pos ? { ...it, x: pos.x, y: pos.y, rotation: 0 } : it;
    });
    const nextNotes = currentNotes.map((n) => {
      const pos = newNotePosMap.get(n.id);
      return pos ? { ...n, x: pos.x, y: pos.y, rotation: 0 } : n;
    });

    setItems(nextItems);
    itemsRef.current = nextItems;
    setNotes(nextNotes);
    notesRef.current = nextNotes;

    // カメラを全要素の中央に自動フィット
    const container = containerRef.current;
    if (container) {
      const containerW = container.clientWidth || 1000;
      const containerH = container.clientHeight || 700;
      const padding = 80;

      const fitZoom = Math.min(
        1.0,
        Math.max(
          0.12,
          Math.min(
            (containerW - padding * 2) / Math.max(1, maxOverallWidth),
            (containerH - padding * 2) / Math.max(1, maxOverallHeight)
          )
        )
      );

      const newPanX = Math.round((containerW - maxOverallWidth * fitZoom) / 2);
      const newPanY = Math.round((containerH - maxOverallHeight * fitZoom) / 2);

      setZoom(fitZoom);
      setPan({ x: newPanX, y: newPanY });
      scheduleSaveCamera({ x: newPanX, y: newPanY }, fitZoom);
    }

    // SQLite DB へ非同期一括永続化
    try {
      const itemPromises = itemUpdates.map((u) =>
        backendApi.updateBoardItem(u.id, { x: u.x, y: u.y, rotation: 0 })
      );
      const notePromises = noteUpdates.map((u) =>
        backendApi.updateBoardNote(u.id, { x: u.x, y: u.y, rotation: 0 })
      );
      await Promise.all([...itemPromises, ...notePromises]);
    } catch (err) {
      console.error("自動並び替えの保存に失敗しました:", err);
    }
  }, [scheduleSaveCamera]);

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
        } else if (selectedNoteId !== null && editingNoteId === null) {
          e.preventDefault();
          handleDeleteNote(selectedNoteId);
        }
      }

      // 詳細情報トグル (Iキー)
      if (e.key === "i" || e.key === "I") {
        if (selectedItemId !== null) {
          e.preventDefault();
          handleToggleImageDetail();
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

  /**
   * グローバルなマウスアップ・安全ガードリスナー
   *
   * 変更理由: マウスをコンテナ外で離したり、他UI要素上で離した場合でも
   * 確実にドラッグ状態を終了し位置を保存するため。
   */
  useEffect(() => {
    const handleGlobalMouseUp = () => {
      if (
        isDraggingCanvasRef.current ||
        isDraggingItemRef.current ||
        isResizingItemRef.current ||
        isRotatingItemRef.current ||
        isDraggingNoteRef.current ||
        isResizingNoteRef.current ||
        isDraggingCropRef.current
      ) {
        handleMouseUp();
      }
    };

    window.addEventListener("mouseup", handleGlobalMouseUp);
    return () => {
      window.removeEventListener("mouseup", handleGlobalMouseUp);
    };
  }, [handleMouseUp]);

  const selectedItem = items.find((it) => it.id === selectedItemId);
  const selectedNote = notes.find((n) => n.id === selectedNoteId);

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
            ({items.length} 点の資料{notes.length > 0 ? `, ${notes.length} 件のメモ` : ""})
          </span>
        </div>

        <div className="h-4 w-px bg-border/40" />

        {/* メモ追加ボタン */}
        <button
          onClick={handleCreateNote}
          className="flex items-center gap-1.5 text-xs text-white bg-accent hover:bg-accent/90 px-3 py-1.5 rounded shadow-sm transition active:scale-98 cursor-pointer font-medium"
          title="キャンバス中央にテキストメモ（付箋）を配置"
        >
          <StickyNote className="w-3.5 h-3.5" />
          メモを追加
        </button>

        {/* 自動並び替えボタン（被り解消・タイル整列） */}
        {(items.length > 0 || notes.length > 0) && (
          <button
            onClick={handleAutoArrange}
            className="flex items-center gap-1.5 text-xs text-textPrimary bg-surfaceLight/50 hover:bg-surfaceLight/80 border border-border/60 hover:border-accent px-3 py-1.5 rounded shadow-sm transition active:scale-98 cursor-pointer font-medium"
            title="ボード上の画像とメモが被らないように自動整列して画面中央にフィット"
          >
            <LayoutGrid className="w-3.5 h-3.5 text-accent" />
            自動並び替え
          </button>
        )}
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

      {/* 3-A. 選択画像アイテム用アクションバー（画面下中央） */}
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

          {/* 詳細情報表示ボタン (I) */}
          <button
            onClick={handleToggleImageDetail}
            className={`flex items-center gap-1 px-2.5 py-1 rounded text-xs transition cursor-pointer ${
              showImageDetail
                ? "bg-accent text-white shadow-xs"
                : "bg-surfaceLight/40 hover:bg-surfaceLight/80 text-textSecondary hover:text-textPrimary"
            }`}
            title="画像の詳細情報を表示 (I)"
          >
            <Info className="w-3.5 h-3.5" />
            <span>詳細情報</span>
          </button>

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

      {/* 3-B. 選択メモ用アクションバー（画面下中央） */}
      {selectedNote && (
        <div className="absolute bottom-6 left-1/2 -translate-x-1/2 z-50 flex items-center gap-3 bg-surface/95 backdrop-blur-md px-4 py-2 rounded-xl border border-border/60 shadow-2xl">
          {/* テキスト編集トグル */}
          <button
            onClick={() => {
              if (editingNoteId === selectedNote.id) {
                handleSaveNoteText(selectedNote.id, editingText);
              } else {
                setEditingNoteId(selectedNote.id);
                setEditingText(selectedNote.text);
              }
            }}
            className={`flex items-center gap-1 px-2.5 py-1 rounded text-xs font-medium transition cursor-pointer ${
              editingNoteId === selectedNote.id
                ? "bg-accent text-white shadow-xs"
                : "bg-surfaceLight/40 hover:bg-surfaceLight/80 text-textPrimary"
            }`}
            title="テキストを編集"
          >
            <Edit3 className="w-3.5 h-3.5" />
            <span>{editingNoteId === selectedNote.id ? "完了" : "テキスト編集"}</span>
          </button>

          {/* カラーパレット */}
          <div className="flex items-center gap-1.5 bg-surfaceLight/30 px-2 py-1 rounded border border-border/40" title="メモの色を変更">
            {NOTE_COLORS.map((c) => (
              <button
                key={c.bg}
                onClick={() => handleChangeNoteColor(selectedNote.id, c.bg)}
                style={{ backgroundColor: c.bg }}
                className={`w-4 h-4 rounded-full border transition hover:scale-125 cursor-pointer ${
                  selectedNote.color.toLowerCase() === c.bg.toLowerCase()
                    ? "ring-2 ring-accent ring-offset-1 ring-offset-surface scale-115 border-white/60"
                    : "border-black/20"
                }`}
                title={c.label}
              />
            ))}
          </div>

          {/* ロック切替 */}
          <button
            onClick={() => handleToggleNoteLock(selectedNote.id)}
            title={selectedNote.isLocked ? "ロック解除" : "位置固定ロック"}
            className="p-1.5 hover:bg-surfaceLight/50 rounded text-textSecondary hover:text-textPrimary transition cursor-pointer"
          >
            {selectedNote.isLocked ? <Lock className="w-4 h-4 text-accent" /> : <Unlock className="w-4 h-4" />}
          </button>

          <div className="h-4 w-px bg-border/40" />

          {/* 重なり順変更 */}
          <div
            className="flex items-center gap-0.5 bg-surfaceLight/30 px-1 py-0.5 rounded border border-border/40"
            title="メモの重なり順を変更"
          >
            <button
              onClick={handleSendNoteToBack}
              className="p-1 hover:bg-surfaceLight/50 rounded text-textSecondary hover:text-textPrimary transition cursor-pointer"
              title="最背面へ移動"
            >
              <ChevronsDown className="w-3.5 h-3.5" />
            </button>
            <div
              className="text-[10px] text-textSecondary font-mono px-1.5 select-none flex items-center gap-1"
              title={`重なり順: レイヤー ${selectedNote.zIndex}`}
            >
              <Layers className="w-3 h-3 text-accent" />
              <span>{selectedNote.zIndex}</span>
            </div>
            <button
              onClick={handleBringNoteToFront}
              className="p-1 hover:bg-surfaceLight/50 rounded text-textSecondary hover:text-textPrimary transition cursor-pointer"
              title="最前面へ移動"
            >
              <ChevronsUp className="w-3.5 h-3.5" />
            </button>
          </div>

          <div className="h-4 w-px bg-border/40" />

          {/* 削除ボタン */}
          <button
            onClick={() => handleDeleteNote(selectedNote.id)}
            title="メモを削除 (Delete)"
            className="p-1.5 hover:bg-red-500/20 text-textSecondary hover:text-red-400 rounded transition cursor-pointer"
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

        {/* メモ（付箋）一覧の描画 */}
        {notes.map((note) => {
          const isSelected = selectedNoteId === note.id;
          const isEditing = editingNoteId === note.id;
          const displayWidth = note.width * note.scale;
          const displayHeight = note.height * note.scale;

          const colorDef = NOTE_COLORS.find(
            (c) => c.bg.toLowerCase() === note.color.toLowerCase()
          ) || { bg: note.color, text: "#1c1917", border: "#e4e4e7" };

          return (
            <div
              key={`note-${note.id}`}
              onMouseDown={(e) => handleNoteMouseDown(e, note)}
              onDoubleClick={(e) => {
                e.stopPropagation();
                setSelectedNoteId(note.id);
                setSelectedItemId(null);
                setEditingNoteId(note.id);
                setEditingText(note.text);
              }}
              style={{
                position: "absolute",
                left: note.x,
                top: note.y,
                width: displayWidth,
                height: displayHeight,
                zIndex: note.zIndex,
                transform: `rotate(${note.rotation}deg)`,
                backgroundColor: note.color,
                color: colorDef.text,
                cursor: note.isLocked
                  ? "default"
                  : isEditing
                  ? "default"
                  : isDraggingNoteRef.current && draggingNoteIdRef.current === note.id
                  ? "grabbing"
                  : "grab",
              }}
              className={`pointer-events-auto select-none rounded-xl p-3 shadow-lg transition-shadow flex flex-col justify-between border ${
                isSelected
                  ? "ring-2 ring-accent shadow-2xl shadow-accent/25 border-transparent"
                  : "hover:ring-1 hover:ring-border/80 border-black/10"
              }`}
            >
              {/* ヘッダー・ピン風アクセント */}
              <div className="flex items-center justify-between pb-1 border-b border-black/10 text-[10px] opacity-75 cursor-grab active:cursor-grabbing">
                <span className="font-semibold flex items-center gap-1">
                  <StickyNote className="w-3 h-3" />
                  メモ
                </span>
                {note.isLocked && <Lock className="w-3 h-3 text-accent" />}
              </div>

              {/* メモ本文または編集textarea */}
              <div className="flex-1 w-full overflow-hidden mt-1 text-xs">
                {isEditing ? (
                  <textarea
                    autoFocus
                    value={editingText}
                    onMouseDown={(e) => e.stopPropagation()}
                    onChange={(e) => setEditingText(e.target.value)}
                    onBlur={() => handleSaveNoteText(note.id, editingText)}
                    onKeyDown={(e) => {
                      if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
                        e.preventDefault();
                        handleSaveNoteText(note.id, editingText);
                      }
                      e.stopPropagation();
                    }}
                    style={{ color: colorDef.text }}
                    className="w-full h-full bg-transparent resize-none outline-none leading-relaxed text-xs font-sans cursor-text"
                    placeholder="メモを入力..."
                  />
                ) : (
                  <div className="w-full h-full whitespace-pre-wrap break-words leading-relaxed select-none overflow-y-auto">
                    {note.text || <span className="opacity-40 italic">（ダブルクリックで編集）</span>}
                  </div>
                )}
              </div>

              {/* リサイズハンドル（右下） */}
              {isSelected && !note.isLocked && (
                <div
                  onMouseDown={(e) => handleNoteResizeMouseDown(e, note)}
                  className="absolute -bottom-2 -right-2 w-4 h-4 bg-accent border-2 border-white rounded-full cursor-se-resize shadow-md hover:scale-125 transition-transform"
                />
              )}
            </div>
          );
        })}
      </div>

      {/* 5. 画像詳細情報パネル (タイムラインと共通) */}
      {showImageDetail && selectedImageDetail && (
        <div className="absolute right-4 bottom-24 z-50">
          <ImageDetailPanel
            detail={selectedImageDetail}
            onClose={() => setShowImageDetail(false)}
            className="w-84 max-h-[80vh] overflow-y-auto"
          />
        </div>
      )}
    </div>
  );
};

