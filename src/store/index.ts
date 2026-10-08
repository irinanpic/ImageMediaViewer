import { create } from "zustand";
import type { DayBucket } from "../types/generated/DayBucket";
import type { ScanProgress } from "../types/generated/ScanProgress";
import type { ThumbProgress } from "../types/generated/ThumbProgress";
import type { TimelineSort } from "../types/generated/TimelineSort";
import type { WatchedFolder } from "../types/generated/WatchedFolder";
import type { Board } from "../types/board";
import type { Bookmark } from "../types/bookmark";
import { backendApi } from "../lib/ipc";

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
  // 多言語（i18n）スライス
  locale: "ja" | "en";
  setLocale: (locale: "ja" | "en") => void;

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
  selectedImageIds: number[];
  setSelectedImageIds: (ids: number[]) => void;
  toggleSelectImageId: (id: number) => void;
  clearSelectedImageIds: () => void;
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
  setBookmarks: (bookmarks: Bookmark[]) => void;
  fetchBookmarks: () => Promise<void>;
  addBookmark: (bookmark: Omit<Bookmark, "id" | "createdAt">) => void;
  removeBookmark: (id: string) => void;
  targetScroll: {
    scrollTop: number;
    rowIndex?: number;
    imageIndex?: number | null;
    dayLabel?: string;
    timestamp: number;
  } | null;
  jumpToBookmark: (bookmark: Bookmark) => void;
  currentVisibleInfo: { scrollTop: number; rowIndex: number; imageIndex: number | null; dayLabel: string } | null;
  setCurrentVisibleInfo: (info: { scrollTop: number; rowIndex: number; imageIndex: number | null; dayLabel: string }) => void;
}

export const useAppStore = create<AppStoreState>((set) => ({
  // 多言語（i18n）初期状態
  locale: (() => {
    try {
      const saved = localStorage.getItem("imv_locale");
      return saved === "en" || saved === "ja" ? saved : "ja";
    } catch {
      return "ja";
    }
  })(),
  setLocale: (locale) => {
    try {
      localStorage.setItem("imv_locale", locale);
    } catch {
      // localStorage 保存失敗時は無視
    }
    set({ locale });
    backendApi.setLocale(locale).catch(() => {
      // バックエンド通信不可時やブラウザ環境での例外は安全に無視
    });
  },

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
  selectedImageIds: [],
  setSelectedImageIds: (selectedImageIds) => set({ selectedImageIds }),
  toggleSelectImageId: (id) =>
    set((state) => {
      const exists = state.selectedImageIds.includes(id);
      const next = exists
        ? state.selectedImageIds.filter((item) => item !== id)
        : [...state.selectedImageIds, id];
      return { selectedImageIds: next };
    }),
  clearSelectedImageIds: () => set({ selectedImageIds: [] }),
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

  // しおり初期状態（ローカルキャッシュから初期表示を高速復元）
  bookmarks: (() => {
    try {
      const saved = localStorage.getItem("imv_bookmarks");
      return saved ? JSON.parse(saved) : [];
    } catch {
      return [];
    }
  })(),
  setBookmarks: (bookmarks) => {
    try {
      localStorage.setItem("imv_bookmarks", JSON.stringify(bookmarks));
    } catch {
      // 保存失敗時は何もしない
    }
    set({ bookmarks });
  },
  fetchBookmarks: async () => {
    try {
      const remoteBookmarks = await backendApi.getBookmarks();
      if (remoteBookmarks && Array.isArray(remoteBookmarks)) {
        if (remoteBookmarks.length > 0) {
          // DBにしおりが存在する場合、それを正として反映
          try {
            localStorage.setItem("imv_bookmarks", JSON.stringify(remoteBookmarks));
          } catch {}
          set({ bookmarks: remoteBookmarks });
        } else {
          // DBが空の場合、localStorageに既存のしおりがあればマイグレーションとしてDBへ一括保存
          let localSaved: Bookmark[] = [];
          try {
            const saved = localStorage.getItem("imv_bookmarks");
            if (saved) localSaved = JSON.parse(saved);
          } catch {}

          if (localSaved.length > 0) {
            for (const b of localSaved) {
              await backendApi.createBookmark({
                id: b.id,
                title: b.title,
                folderId: b.folderId,
                folderName: b.folderName,
                sort: b.sort,
                scrollTop: b.scrollTop,
                rowIndex: b.rowIndex,
                imageIndex: b.imageIndex,
                dayLabel: b.dayLabel,
                createdAt: b.createdAt,
              });
            }
            set({ bookmarks: localSaved });
          }
        }
      }
    } catch (err) {
      console.warn("しおりのDB取得に失敗しました（ローカルキャッシュを使用）:", err);
    }
  },
  addBookmark: (bookmarkData) => {
    const newBookmark: Bookmark = {
      ...bookmarkData,
      id: crypto.randomUUID(),
      createdAt: Date.now(),
    };
    set((state) => {
      const nextBookmarks = [newBookmark, ...state.bookmarks];
      try {
        localStorage.setItem("imv_bookmarks", JSON.stringify(nextBookmarks));
      } catch {}
      return { bookmarks: nextBookmarks };
    });

    // バックエンドSQLiteへ非同期永続化
    backendApi.createBookmark({
      id: newBookmark.id,
      title: newBookmark.title,
      folderId: newBookmark.folderId,
      folderName: newBookmark.folderName,
      sort: newBookmark.sort,
      scrollTop: newBookmark.scrollTop,
      rowIndex: newBookmark.rowIndex,
      imageIndex: newBookmark.imageIndex,
      dayLabel: newBookmark.dayLabel,
      createdAt: newBookmark.createdAt,
    }).catch((err) => {
      console.error("しおりのDB保存に失敗しました:", err);
    });
  },
  removeBookmark: (id) => {
    set((state) => {
      const nextBookmarks = state.bookmarks.filter((b) => b.id !== id);
      try {
        localStorage.setItem("imv_bookmarks", JSON.stringify(nextBookmarks));
      } catch {}
      return { bookmarks: nextBookmarks };
    });

    // バックエンドSQLiteから非同期削除
    backendApi.deleteBookmark(id).catch((err) => {
      console.error("しおりのDB削除に失敗しました:", err);
    });
  },
  targetScroll: null,
  jumpToBookmark: (bookmark) =>
    set((state) => {
      const updates: Partial<AppStoreState> = {
        targetScroll: {
          scrollTop: bookmark.scrollTop,
          rowIndex: bookmark.rowIndex,
          imageIndex: bookmark.imageIndex,
          dayLabel: bookmark.dayLabel,
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

