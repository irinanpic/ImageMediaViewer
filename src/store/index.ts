import { create } from "zustand";
import type { DayBucket } from "../types/generated/DayBucket";
import type { ScanProgress } from "../types/generated/ScanProgress";
import type { ThumbProgress } from "../types/generated/ThumbProgress";
import type { TimelineSort } from "../types/generated/TimelineSort";
import type { WatchedFolder } from "../types/generated/WatchedFolder";
import type { Board } from "../types/board";
import type { Bookmark } from "../types/bookmark";

export interface ViewerTransformState {
  rotation: 0 | 90 | 180 | 270;
  flipH: boolean;
  flipV: boolean;
  zoom: number;
  panX: number;
  panY: number;
}

const DEFAULT_TRANSFORM: ViewerTransformState = {
  rotation: 0,
  flipH: false,
  flipV: false,
  zoom: 1,
  panX: 0,
  panY: 0,
};

interface AppStoreState {
  // カタログスライス
  totalImages: number;
  buckets: DayBucket[];
  catalogVersion: number;
  selectedFolderId: number | null;
  folders: WatchedFolder[];
  timelineSort: TimelineSort;
  setCatalogSummary: (total: number, buckets: DayBucket[], version: number) => void;
  setSelectedFolderId: (folderId: number | null) => void;
  setFolders: (folders: WatchedFolder[]) => void;
  reorderFolders: (startIndex: number, endIndex: number) => void;
  setTimelineSort: (sort: TimelineSort) => void;

  // ムードボードスライス
  currentView: "timeline" | "board";
  setCurrentView: (view: "timeline" | "board") => void;
  boards: Board[];
  activeBoardId: number | null;
  setBoards: (boards: Board[]) => void;
  setActiveBoardId: (id: number | null) => void;
  isAddToBoardModalOpen: boolean;
  addToBoardTargetImageIds: number[];
  openAddToBoardModal: (imageIds: number[]) => void;
  closeAddToBoardModal: () => void;

  // ビューアスライス
  activeImageId: number | null;
  activeImageIndex: number | null;
  isViewerOpen: boolean;
  transform: ViewerTransformState;
  openViewer: (id: number, index: number) => void;
  closeViewer: () => void;
  setViewerIndex: (id: number, index: number) => void;
  rotateClockwise: () => void;
  rotateCounterClockwise: () => void;
  toggleFlipH: () => void;
  toggleFlipV: () => void;
  setZoom: (zoom: number) => void;
  setPan: (x: number, y: number) => void;
  setZoomAndPan: (zoom: number, panX: number, panY: number) => void;
  resetTransform: () => void;

  // スキャン・サムネイル進捗スライス
  scanProgress: ScanProgress | null;
  thumbProgress: ThumbProgress | null;
  setScanProgress: (progress: ScanProgress | null) => void;
  setThumbProgress: (progress: ThumbProgress | null) => void;

  // UIスライス
  cellSize: number;
  setCellSize: (size: number) => void;
  selectedCellIndex: number | null;
  setSelectedCellIndex: (index: number | null) => void;
  isSidebarOpen: boolean;
  toggleSidebar: () => void;
  timelineRefreshTick: number;
  refreshTimeline: () => void;
  isLogModalOpen: boolean;
  openLogModal: () => void;
  closeLogModal: () => void;

  // 通信状態スライス
  connectionStatus: "connected" | "connecting" | "disconnected";
  lastConnectedAt: number | null;
  setConnectionStatus: (status: "connected" | "connecting" | "disconnected") => void;
  markConnected: () => void;
  markDisconnected: () => void;

  // しおり（ブックマーク）スライス
  bookmarks: Bookmark[];
  addBookmark: (bookmark: Omit<Bookmark, "id" | "createdAt">) => void;
  removeBookmark: (id: string) => void;
  targetScroll: { scrollTop: number; rowIndex?: number; timestamp: number } | null;
  jumpToBookmark: (bookmark: Bookmark) => void;
  currentVisibleInfo: { scrollTop: number; rowIndex: number; imageIndex: number | null; dayLabel: string } | null;
  setCurrentVisibleInfo: (info: { scrollTop: number; rowIndex: number; imageIndex: number | null; dayLabel: string }) => void;
}

export const useAppStore = create<AppStoreState>((set) => ({
  // カタログ初期状態
  totalImages: 0,
  buckets: [],
  catalogVersion: 1,
  selectedFolderId: null,
  folders: [],
  timelineSort: "folder",
  setCatalogSummary: (total, buckets, version) =>
    set({ totalImages: total, buckets, catalogVersion: version }),
  setSelectedFolderId: (selectedFolderId) => set({ selectedFolderId }),
  setFolders: (folders) => {
    try {
      const savedOrder = JSON.parse(localStorage.getItem("imv_folder_order") || "[]") as number[];
      if (Array.isArray(savedOrder) && savedOrder.length > 0) {
        const orderMap = new Map(savedOrder.map((id, idx) => [id, idx]));
        folders.sort((a, b) => {
          const idxA = orderMap.has(a.id) ? orderMap.get(a.id)! : 9999;
          const idxB = orderMap.has(b.id) ? orderMap.get(b.id)! : 9999;
          return idxA - idxB;
        });
      }
    } catch {
      // localStorageパース失敗時はそのまま
    }
    set({ folders });
  },
  reorderFolders: (startIndex, endIndex) =>
    set((state) => {
      const nextFolders = [...state.folders];
      const [moved] = nextFolders.splice(startIndex, 1);
      if (!moved) return state;
      nextFolders.splice(endIndex, 0, moved);
      try {
        localStorage.setItem("imv_folder_order", JSON.stringify(nextFolders.map((f) => f.id)));
      } catch {
        // localStorage保存失敗時は無視
      }
      return { folders: nextFolders };
    }),
  setTimelineSort: (timelineSort) => set({ timelineSort }),

  // ムードボード初期状態
  currentView: "timeline",
  setCurrentView: (currentView) => set({ currentView }),
  boards: [],
  activeBoardId: null,
  setBoards: (boards) => set({ boards }),
  setActiveBoardId: (activeBoardId) => set({ activeBoardId }),
  isAddToBoardModalOpen: false,
  addToBoardTargetImageIds: [],
  openAddToBoardModal: (imageIds) =>
    set({ isAddToBoardModalOpen: true, addToBoardTargetImageIds: imageIds }),
  closeAddToBoardModal: () =>
    set({ isAddToBoardModalOpen: false, addToBoardTargetImageIds: [] }),

  // ビューア初期状態
  activeImageId: null,
  activeImageIndex: null,
  isViewerOpen: false,
  transform: DEFAULT_TRANSFORM,
  openViewer: (id, index) =>
    set({
      activeImageId: id,
      activeImageIndex: index,
      isViewerOpen: true,
      transform: DEFAULT_TRANSFORM,
    }),
  closeViewer: () =>
    set({
      isViewerOpen: false,
      activeImageId: null,
      activeImageIndex: null,
      transform: DEFAULT_TRANSFORM,
    }),
  setViewerIndex: (id, index) =>
    set({
      activeImageId: id,
      activeImageIndex: index,
      transform: DEFAULT_TRANSFORM,
    }),
  rotateClockwise: () =>
    set((state) => ({
      transform: {
        ...state.transform,
        rotation: ((state.transform.rotation + 90) % 360) as 0 | 90 | 180 | 270,
      },
    })),
  rotateCounterClockwise: () =>
    set((state) => ({
      transform: {
        ...state.transform,
        rotation: ((state.transform.rotation + 270) % 360) as 0 | 90 | 180 | 270,
      },
    })),
  toggleFlipH: () =>
    set((state) => ({
      transform: { ...state.transform, flipH: !state.transform.flipH },
    })),
  toggleFlipV: () =>
    set((state) => ({
      transform: { ...state.transform, flipV: !state.transform.flipV },
    })),
  setZoom: (zoom) =>
    set((state) => ({
      transform: { ...state.transform, zoom: Math.max(0.1, Math.min(zoom, 10)) },
    })),
  setPan: (panX, panY) =>
    set((state) => ({
      transform: { ...state.transform, panX, panY },
    })),
  setZoomAndPan: (zoom, panX, panY) =>
    set((state) => ({
      transform: {
        ...state.transform,
        zoom: Math.max(0.1, Math.min(zoom, 10)),
        panX,
        panY,
      },
    })),
  resetTransform: () => set({ transform: DEFAULT_TRANSFORM }),

  // 進捗初期状態
  scanProgress: null,
  thumbProgress: null,
  setScanProgress: (scanProgress) => set({ scanProgress }),
  setThumbProgress: (thumbProgress) => set({ thumbProgress }),

  // UI初期状態
  cellSize: 160,
  setCellSize: (cellSize) => set({ cellSize }),
  selectedCellIndex: null,
  setSelectedCellIndex: (selectedCellIndex) => set({ selectedCellIndex }),
  isSidebarOpen: true,
  toggleSidebar: () => set((state) => ({ isSidebarOpen: !state.isSidebarOpen })),
  timelineRefreshTick: 0,
  refreshTimeline: () =>
    set((state) => ({ timelineRefreshTick: state.timelineRefreshTick + 1 })),
  isLogModalOpen: false,
  openLogModal: () => set({ isLogModalOpen: true }),
  closeLogModal: () => set({ isLogModalOpen: false }),

  // 通信状態初期値
  connectionStatus: "connected",
  lastConnectedAt: Date.now(),
  setConnectionStatus: (connectionStatus) => set({ connectionStatus }),
  markConnected: () => set({ connectionStatus: "connected", lastConnectedAt: Date.now() }),
  markDisconnected: () => set({ connectionStatus: "disconnected" }),

  // しおり初期状態
  bookmarks: (() => {
    try {
      const saved = localStorage.getItem("imv_bookmarks");
      return saved ? JSON.parse(saved) : [];
    } catch {
      return [];
    }
  })(),
  addBookmark: (bookmarkData) =>
    set((state) => {
      const newBookmark: Bookmark = {
        ...bookmarkData,
        id: crypto.randomUUID(),
        createdAt: Date.now(),
      };
      const nextBookmarks = [newBookmark, ...state.bookmarks];
      try {
        localStorage.setItem("imv_bookmarks", JSON.stringify(nextBookmarks));
      } catch {
        // 保存失敗時は何もしない
      }
      return { bookmarks: nextBookmarks };
    }),
  removeBookmark: (id) =>
    set((state) => {
      const nextBookmarks = state.bookmarks.filter((b) => b.id !== id);
      try {
        localStorage.setItem("imv_bookmarks", JSON.stringify(nextBookmarks));
      } catch {
        // 保存失敗時は何もしない
      }
      return { bookmarks: nextBookmarks };
    }),
  targetScroll: null,
  jumpToBookmark: (bookmark) =>
    set((state) => {
      const updates: Partial<AppStoreState> = {
        targetScroll: {
          scrollTop: bookmark.scrollTop,
          rowIndex: bookmark.rowIndex,
          timestamp: Date.now(),
        },
      };
      if (bookmark.folderId !== state.selectedFolderId) {
        updates.selectedFolderId = bookmark.folderId;
      }
      if (bookmark.sort !== state.timelineSort) {
        updates.timelineSort = bookmark.sort;
      }
      if (state.currentView !== "timeline") {
        updates.currentView = "timeline";
      }
      return updates;
    }),
  currentVisibleInfo: null,
  setCurrentVisibleInfo: (info) => set({ currentVisibleInfo: info }),
}));

