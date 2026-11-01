import React, { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  ChevronDown,
  ChevronRight,
  ChevronUp,
  FileArchive,
  Folder,
  FolderOpen,
  FolderPlus,
  FolderSync,
  Globe,
  GripVertical,
  Layers,
  LayoutGrid,
  Loader2,
  Plus,
  RefreshCw,
  Trash2,
  Upload,
  WifiOff,
  X,
} from "lucide-react";
import { backendApi } from "../../lib/ipc";
import { isTauriEnvironment, isMobileEnvironment } from "../../lib/thumbUrl";
import { useResponsiveLayout } from "../../hooks/useResponsiveLayout";
import { useTranslation } from "../../locales";
import { useAppStore } from "../../store";
import type { FolderTreeNode } from "../../types/filter";
import type { WatchedFolder } from "../../types/generated/WatchedFolder";
import {
  getAllPhotosThumbnailState,
  getFolderThumbnailState,
} from "./folderStatus";
import { AddFolderModal } from "./AddFolderModal";
import { extractDroppedDirectoryInfo } from "../../utils/webDropParser";

export const Sidebar: React.FC = () => {
  const { t, locale, setLocale } = useTranslation();
  const { isCompact } = useResponsiveLayout();
  const folders = useAppStore((state) => state.folders);
  const selectedFolderId = useAppStore((state) => state.selectedFolderId);
  const setSelectedFolderId = useAppStore((state) => state.setSelectedFolderId);
  const reorderFolders = useAppStore((state) => state.reorderFolders);
  const isSidebarOpen = useAppStore((state) => state.isSidebarOpen);
  const setSidebarOpen = useAppStore((state) => state.setSidebarOpen);
  const currentView = useAppStore((state) => state.currentView);
  const setCurrentView = useAppStore((state) => state.setCurrentView);
  const boards = useAppStore((state) => state.boards);
  const setBoards = useAppStore((state) => state.setBoards);
  const reorderBoards = useAppStore((state) => state.reorderBoards);
  const totalImages = useAppStore((state) => state.totalImages);
  const catalogVersion = useAppStore((state) => state.catalogVersion);
  const thumbProgress = useAppStore((state) => state.thumbProgress);
  const activeBoardId = useAppStore((state) => state.activeBoardId);
  const setActiveBoardId = useAppStore((state) => state.setActiveBoardId);
  const filterState = useAppStore((state) => state.filterState);
  const updateFilterState = useAppStore((state) => state.updateFilterState);
  const timelineSort = useAppStore((state) => state.timelineSort);
  const setTimelineSort = useAppStore((state) => state.setTimelineSort);
  const refreshTimeline = useAppStore((state) => state.refreshTimeline);
  const folderHierarchies = useAppStore((state) => state.folderHierarchies);
  const setFolderHierarchy = useAppStore((state) => state.setFolderHierarchy);
  const sidebarWidth = useAppStore((state) => state.sidebarWidth);
  const setSidebarWidth = useAppStore((state) => state.setSidebarWidth);

  // デスクトップ環境向けサイドバー幅リサイズドラッグ状態
  const [isResizing, setIsResizing] = useState(false);
  const isResizingRef = React.useRef(false);

  const handleMouseDownResize = (e: React.MouseEvent) => {
    e.preventDefault();
    isResizingRef.current = true;
    setIsResizing(true);
    const startX = e.clientX;
    const startWidth = sidebarWidth;

    const handleMouseMove = (moveEvent: MouseEvent) => {
      if (!isResizingRef.current) return;
      const delta = moveEvent.clientX - startX;
      const maxWidth = Math.min(600, Math.max(350, Math.floor(window.innerWidth * 0.5)));
      const nextWidth = Math.min(Math.max(startWidth + delta, 200), maxWidth);
      setSidebarWidth(nextWidth);
    };

    const handleMouseUp = () => {
      isResizingRef.current = false;
      setIsResizing(false);
      window.removeEventListener("mousemove", handleMouseMove);
      window.removeEventListener("mouseup", handleMouseUp);
    };

    window.addEventListener("mousemove", handleMouseMove);
    window.addEventListener("mouseup", handleMouseUp);
  };

  const handleDoubleClickReset = () => {
    setSidebarWidth(256);
  };

  // 監視フォルダごとのサブフォルダ展開状態と階層ツリーキャッシュ
  const [expandedFolderIds, setExpandedFolderIds] = useState<Record<number, boolean>>({});
  const [expandedNodeKeys, setExpandedNodeKeys] = useState<Record<string, boolean>>({});
  const [loadingArchiveKey, setLoadingArchiveKey] = useState<string | null>(null);

  // 展開中のフォルダはカタログ更新時にサブフォルダ・アーカイブ階層ツリーも自動再取得
  React.useEffect(() => {
    const expandedIds = Object.keys(expandedFolderIds)
      .map(Number)
      .filter((id) => expandedFolderIds[id]);
    if (expandedIds.length === 0) return;
    for (const fid of expandedIds) {
      backendApi
        .getFolderHierarchy(fid)
        .then((tree) => {
          setFolderHierarchy(fid, tree);
        })
        .catch(() => {});
    }
  }, [catalogVersion, expandedFolderIds, setFolderHierarchy]);

  // 変更理由: 監視フォルダの初期表示は閉じた状態をデフォルトとし、
  // ユーザーが明示的に展開操作を行うまで階層を開かない設計に統一。

  const handleToggleSubfolders = async (e: React.MouseEvent, folderId: number) => {
    e.stopPropagation();
    const nextExpanded = !expandedFolderIds[folderId];
    setExpandedFolderIds((prev) => ({ ...prev, [folderId]: nextExpanded }));
    if (nextExpanded && !folderHierarchies[folderId]) {
      try {
        const tree = await backendApi.getFolderHierarchy(folderId);
        setFolderHierarchy(folderId, tree);
      } catch (err) {
        console.warn("サブフォルダツリーの取得に失敗:", err);
      }
    }
  };

  // 初回および定期的なボード一覧取得
  React.useEffect(() => {
    backendApi.getBoards().then(setBoards).catch(console.error);
  }, [setBoards]);

  // フォルダパスを追加する共通処理
  const addFolderByPath = async (folderPath: string) => {
    const trimmed = folderPath.trim();
    if (!trimmed) return;
    try {
      await backendApi.addWatchFolder(trimmed);
      const updated = await backendApi.getWatchFolders();
      useAppStore.getState().setFolders(updated);
    } catch (err: any) {
      alert(t("sidebar.addFolderFailed", { error: err.message || err }));
    }
  };

  // Tauri ネイティブのドラッグ＆ドロップイベント監視
  // 変更理由: OSエクスプローラーからフォルダがドロップされた際、
  // セキュリティ制限による空パスを回避し、Tauri API経由で対象フォルダの絶対パスをダイレクトに自動登録する
  React.useEffect(() => {
    if (!isTauriEnvironment()) return;

    let unlistenFn: (() => void) | null = null;
    import("@tauri-apps/api/webview")
      .then(({ getCurrentWebview }) => {
        getCurrentWebview()
          .onDragDropEvent((event) => {
            // ムードボード表示中は MoodboardCanvas 側のドロップハンドラ（直接画像配置）へ委譲する
            if (useAppStore.getState().currentView === "board") {
              return;
            }
            if (event.payload.type === "drop") {
              const paths = event.payload.paths;
              if (paths && paths.length > 0) {
                for (const p of paths) {
                  addFolderByPath(p);
                }
              }
            }
          })
          .then((unlisten) => {
            unlistenFn = unlisten;
          })
          .catch(console.error);
      })
      .catch(console.error);

    return () => {
      if (unlistenFn) unlistenFn();
    };
  }, []);

  // ドラッグ＆ドロップ並び替え用の状態
  const [draggedFolderIndex, setDraggedFolderIndex] = useState<number | null>(null);
  const [dragOverFolderIndex, setDragOverFolderIndex] = useState<number | null>(null);
  const [draggedBoardIndex, setDraggedBoardIndex] = useState<number | null>(null);
  const [dragOverBoardIndex, setDragOverBoardIndex] = useState<number | null>(null);
  const [isExternalDragOver, setIsExternalDragOver] = useState(false);
  // フォルダ選択処理（モバイル時はドロワーを自動で閉じる）
  const handleSelectFolder = (id: number | null) => {
    setSelectedFolderId(id);
    updateFilterState({ subFolderPath: null });
    setCurrentView("timeline");
    if (isCompact) {
      setSidebarOpen(false);
    }
  };

  // サブフォルダまたはアーカイブ仮想フォルダ選択処理
  const handleSelectTreeNode = async (folderId: number, node: FolderTreeNode) => {
    const nodeKey = `${folderId}:${node.path}`;
    const isArchiveRelated = node.kind === "archive" || node.path.includes("!/");

    if (isArchiveRelated) {
      setLoadingArchiveKey(nodeKey);
      try {
        await backendApi.openArchiveFolder(folderId, node.path);
        // アーカイブ閲覧時はページ番号順（ファイル名昇順）に自動切り替え
        if (timelineSort !== "nameAsc" && timelineSort !== "nameDesc") {
          setTimelineSort("nameAsc");
        }
        // インデックス登録後の最新ツリー（枚数確定・内部サブフォルダ展開）を再取得
        const tree = await backendApi.getFolderHierarchy(folderId);
        setFolderHierarchy(folderId, tree);
        setExpandedNodeKeys((prev) => ({ ...prev, [nodeKey]: true }));
      } catch (err) {
        console.error("アーカイブのオンデマンド展開に失敗:", err);
      } finally {
        setLoadingArchiveKey(null);
      }
    }

    setSelectedFolderId(folderId);
    updateFilterState({ subFolderPath: node.path });
    setCurrentView("timeline");
    refreshTimeline();
    if (isCompact) {
      setSidebarOpen(false);
    }
  };

  // 子ノード群を再帰的にレンダリングするヘルパー関数
  const renderHierarchyNodes = (
    folderId: number,
    nodes: FolderTreeNode[],
    depth = 0
  ): React.ReactNode => {
    return nodes.map((node) => {
      const nodeKey = `${folderId}:${node.path}`;
      const isSelected =
        selectedFolderId === folderId && filterState.subFolderPath === node.path;
      const hasChildren = node.children && node.children.length > 0;
      const isNodeExpanded = expandedNodeKeys[nodeKey] ?? false;
      const isArchive = node.kind === "archive";
      const isLoadingThis = loadingArchiveKey === nodeKey;
      const indentPx = depth * 12;

      return (
        <div key={node.path} className="space-y-0.5">
          <div
            onClick={() => handleSelectTreeNode(folderId, node)}
            style={{ paddingLeft: `${6 + indentPx}px` }}
            title={
              isArchive && node.imageCount === null
                ? `${node.path} (${t("sidebar.archiveClickToOpen")})`
                : node.path
            }
            className={`w-full flex items-center justify-between pr-2 py-1 rounded text-xs transition cursor-pointer ${
              isSelected
                ? "bg-accent/25 text-accent font-medium border border-accent/40"
                : "text-textSecondary hover:bg-surfaceLight hover:text-textPrimary"
            }`}
          >
            <div className="flex items-center gap-1 truncate min-w-0 flex-1">
              {hasChildren ? (
                <button
                  type="button"
                  onClick={(e) => {
                    e.stopPropagation();
                    setExpandedNodeKeys((prev) => ({
                      ...prev,
                      [nodeKey]: !isNodeExpanded,
                    }));
                  }}
                  className="p-0.5 rounded hover:bg-surface text-textSecondary hover:text-accent shrink-0 cursor-pointer"
                >
                  {isNodeExpanded ? (
                    <ChevronDown className="w-3 h-3" />
                  ) : (
                    <ChevronRight className="w-3 h-3" />
                  )}
                </button>
              ) : (
                <span className="w-4 shrink-0" />
              )}

              {isLoadingThis ? (
                <Loader2 className="w-3.5 h-3.5 shrink-0 text-amber-400 animate-spin" />
              ) : isArchive ? (
                <FileArchive className="w-3.5 h-3.5 shrink-0 text-amber-400" />
              ) : (
                <Folder className="w-3.5 h-3.5 shrink-0 opacity-80" />
              )}
              <span className="truncate">{node.name}</span>
            </div>

            <span className="text-[10px] font-mono opacity-75 shrink-0 ml-1">
              {node.imageCount !== null && node.imageCount !== undefined
                ? `(${node.imageCount})`
                : "(-)"}
            </span>
          </div>

          {hasChildren && isNodeExpanded && (
            <div className="space-y-0.5">
              {renderHierarchyNodes(folderId, node.children, depth + 1)}
            </div>
          )}
        </div>
      );
    });
  };

  // ボード選択処理（モバイル時はドロワーを自動で閉じる）
  const handleSelectBoard = (id: number) => {
    setActiveBoardId(id);
    setCurrentView("board");
    if (isCompact) {
      setSidebarOpen(false);
    }
  };

  // モバイル向けフォルダ追加モーダルの表示状態
  const [isAddFolderModalOpen, setIsAddFolderModalOpen] = useState(false);

  // フォルダ追加ダイアログオープン
  const handleAddFolder = async () => {
    // モバイル環境（Android/iOS）では open({ directory: true }) が非対応のため専用モーダルを開く
    if (isMobileEnvironment()) {
      setIsAddFolderModalOpen(true);
      return;
    }

    try {
      let selected: string | null = null;
      if (isTauriEnvironment()) {
        try {
          const res = await open({
            directory: true,
            multiple: false,
            title: t("sidebar.selectFolderDialogTitle"),
          });
          if (typeof res === "string") selected = res;
        } catch (dialogErr: any) {
          // ネイティブダイアログが未対応・失敗した場合はフォールバックモーダルを開く
          console.warn("Native folder dialog failed, falling back to AddFolderModal:", dialogErr);
          setIsAddFolderModalOpen(true);
          return;
        }
      } else {
        selected = prompt(t("sidebar.enterPathPrompt"));
      }

      if (selected) {
        await addFolderByPath(selected);
      }
    } catch (err: any) {
      alert(t("sidebar.addFolderFailed", { error: err.message || err }));
    }
  };

  // フォルダ解除
  const handleRemoveFolder = async (e: React.MouseEvent, id: number) => {
    e.stopPropagation();
    if (!confirm(t("sidebar.removeFolderConfirm"))) {
      return;
    }

    try {
      await backendApi.removeWatchFolder(id);
      if (selectedFolderId === id) {
        setSelectedFolderId(null);
      }
      const updated = await backendApi.getWatchFolders();
      useAppStore.getState().setFolders(updated);
    } catch (err: any) {
      alert(t("sidebar.removeFolderFailed", { error: err.message || err }));
    }
  };

  // フォルダ個別再走査
  const handleRescanFolder = async (e: React.MouseEvent, id: number) => {
    e.stopPropagation();
    try {
      await backendApi.rescan(id);
    } catch (err: any) {
      alert(t("sidebar.rescanFailed", { error: err.message || err }));
    }
  };

  // 未生成・失敗サムネイルの一括再作成
  const handleRescanMissing = async (e?: React.MouseEvent) => {
    if (e) e.stopPropagation();
    try {
      await backendApi.rescanMissingThumbnails();
      const thumbInfo = await backendApi.getThumbProgress();
      useAppStore.getState().setThumbProgress({ done: thumbInfo.done, failed: thumbInfo.failed, total: thumbInfo.total });
      const updated = await backendApi.getWatchFolders();
      useAppStore.getState().setFolders(updated);
    } catch (err: any) {
      console.error("サムネイル再作成要求エラー:", err);
    }
  };

  // エクスプローラ等からのフォルダドロップ処理
  const handleExternalDrop = async (e: React.DragEvent) => {
    if (draggedFolderIndex !== null || draggedBoardIndex !== null) return;
    e.preventDefault();
    e.stopPropagation();
    setIsExternalDragOver(false);

    // ★ 重要: 非同期 await 前に Directory Entry 取得を開始しておく
    const dirInfoPromise = extractDroppedDirectoryInfo(e.dataTransfer);
    const files = Array.from(e.dataTransfer.files);

    // 1. ファイルオブジェクトから直接ローカルパスが取得できる場合
    let resolvedAny = false;
    for (const file of files) {
      const filePath = (file as any).path as string | undefined;
      if (filePath) {
        await addFolderByPath(filePath);
        resolvedAny = true;
      }
    }
    if (resolvedAny) return;

    // 2. DOM Directory Entryからフォルダ名と直下エントリ一覧を取得し、PC内の実パスを自動特定
    try {
      const dirInfo = await dirInfoPromise;
      if (dirInfo) {
        const added = await backendApi.resolveAndAddDroppedFolder(
          dirInfo.folderName,
          dirInfo.childNames
        );
        const updated = await backendApi.getWatchFolders();
        useAppStore.getState().setFolders(updated);
        useAppStore.getState().setSelectedFolderId(added.id);
        return;
      }
    } catch (err: any) {
      console.warn("フォルダ自動特定失敗:", err);
    }

    // 3. 自動特定できなかった場合のフォールバック（フォルダ追加モーダルを表示）
    setIsAddFolderModalOpen(true);
  };

  if (!isSidebarOpen) return null;

  const sidebarContent = (
    <aside
      onDragOver={(e) => {
        if (draggedFolderIndex !== null || draggedBoardIndex !== null) return;
        if (e.dataTransfer?.types?.includes("Files")) {
          e.preventDefault();
          setIsExternalDragOver(true);
        }
      }}
      onDragLeave={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node)) {
          setIsExternalDragOver(false);
        }
      }}
      onDrop={handleExternalDrop}
      className={`${
        isCompact
          ? "fixed top-0 left-0 bottom-0 w-72 max-w-[85vw] bg-surface z-50 flex flex-col shadow-2xl border-r border-border animate-in slide-in-from-left duration-200"
          : "bg-surface border-r border-border flex flex-col h-full select-none z-10 relative shrink-0"
      }`}
      style={{
        width: isCompact ? undefined : `${sidebarWidth}px`,
        minWidth: isCompact ? undefined : `${sidebarWidth}px`,
        maxWidth: isCompact ? undefined : "600px",
        paddingLeft: isCompact ? "calc(0.5rem + var(--sal))" : undefined,
        paddingTop: isCompact ? "var(--sat)" : undefined,
        paddingBottom: isCompact ? "var(--sab)" : undefined,
      }}
    >
      {/* ヘッダ */}
      <div className="p-4 border-b border-border flex items-center justify-between">
        <h2 className="text-sm font-bold text-textPrimary tracking-wide">{t("sidebar.folders")}</h2>
        <div className="flex items-center gap-1">
          <button
            onClick={handleAddFolder}
            title={t("sidebar.addFolder")}
            className="flex items-center gap-1 text-xs bg-accent hover:bg-accentHover text-white px-2.5 py-1.5 rounded transition font-medium cursor-pointer"
          >
            <FolderPlus className="w-4 h-4" />
            <span>{t("sidebar.addFolderBtn")}</span>
          </button>
          {isCompact && (
            <button
              onClick={() => setSidebarOpen(false)}
              className="p-1.5 text-textSecondary hover:text-textPrimary hover:bg-surfaceLight rounded-md transition ml-1 cursor-pointer"
              title="閉じる"
            >
              <X className="w-4 h-4" />
            </button>
          )}
        </div>
      </div>

      {/* フォルダ一覧リスト */}
      <div className="flex-1 overflow-y-auto p-2 space-y-1">
        {/* すべての写真（2段表示: 上段にタイトル、下段に枚数・ステータス） */}
        <button
          onClick={() => handleSelectFolder(null)}
          className={`w-full flex flex-col px-3 py-2 rounded text-sm transition gap-1 ${
            selectedFolderId === null
              ? "bg-accent/20 text-accent font-medium border border-accent/40"
              : "text-textSecondary hover:bg-surfaceLight hover:text-textPrimary"
          }`}
        >
          {/* 上段: アイコン + タイトル */}
          <div className="flex items-center gap-2 w-full min-w-0">
            <Layers className="w-4 h-4 shrink-0" />
            <span className="truncate font-medium">{t("sidebar.allPhotos")}</span>
          </div>

          {/* 下段: 枚数およびステータスバッジ */}
          {totalImages > 0 && (() => {
            const state = getAllPhotosThumbnailState(totalImages, thumbProgress);
            const thumbDone = thumbProgress?.done ?? 0;
            const thumbFailed = thumbProgress?.failed ?? 0;
            const thumbTotal = thumbProgress?.total ?? 0;

            return (
              <div className="flex items-center justify-between w-full min-w-0 pl-6 text-[11px] font-mono">
                <span className="text-textSecondary/70 text-[10px]">
                  {t("sidebar.imagesCount", { count: totalImages.toLocaleString() })}
                </span>
                {state === "generating" ? (
                  <span
                    className="text-amber-400 bg-amber-400/10 border border-amber-400/30 px-1.5 py-0.5 rounded text-[10px]"
                    title={t("sidebar.generating", {
                      done: thumbDone.toLocaleString(),
                      total: thumbTotal.toLocaleString(),
                    })}
                  >
                    {t("sidebar.generating", {
                      done: thumbDone.toLocaleString(),
                      total: thumbTotal.toLocaleString(),
                    })}
                  </span>
                ) : state === "warningDone" ? (
                  <span
                    className="text-amber-400 bg-amber-400/10 border border-amber-400/30 px-1.5 py-0.5 rounded text-[10px] font-medium"
                    title={t("sidebar.warningDoneTooltip", { failed: thumbFailed.toLocaleString() })}
                  >
                    {t("sidebar.warningDoneCount", {
                      done: thumbDone.toLocaleString(),
                      total: totalImages.toLocaleString(),
                    })}
                  </span>
                ) : state === "allDone" ? (
                  <span
                    className="text-emerald-400 bg-emerald-500/10 border border-emerald-500/30 px-1.5 py-0.5 rounded text-[10px] font-medium"
                    title={t("sidebar.allDoneTooltip")}
                  >
                    {t("sidebar.allDone", { total: totalImages.toLocaleString() })}
                  </span>
                ) : null}
              </div>
            );
          })()}
        </button>

        {/* 登録フォルダ一覧（ドラッグ＆ドロップで並び替え・サブフォルダ展開可能） */}
        {folders.map((folder: WatchedFolder, index: number) => {
          const folderName = folder.path.split(/[\\/]/).filter(Boolean).pop() || folder.path;
          const isSelectedRoot =
            selectedFolderId === folder.id && !filterState.subFolderPath;
          const isSelectedAny = selectedFolderId === folder.id;
          const isOffline = folder.status === "offline";
          const isDragOver =
            dragOverFolderIndex === index &&
            draggedFolderIndex !== null &&
            draggedFolderIndex !== index;
          const failedCount = folder.thumbFailedCount ?? 0;
          const folderThumbState = getFolderThumbnailState(
            folder.imageCount,
            folder.thumbCount,
            folder.thumbFailedCount
          );
          const isGenerating = folderThumbState === "generating";
          const isWarningDone = folderThumbState === "warningDone";
          const isAllDone = folderThumbState === "allDone";
          const isExpanded = Boolean(expandedFolderIds[folder.id]);
          const hierarchyNodes = folderHierarchies[folder.id] ?? [];

          return (
            <div key={folder.id} className="space-y-0.5">
              <div
                draggable
                onDragStart={(e) => {
                  setDraggedFolderIndex(index);
                  e.dataTransfer.effectAllowed = "move";
                  e.dataTransfer.setData("application/x-imv-folder-index", String(index));
                  e.dataTransfer.setData("text/plain", String(index));
                }}
                onDragOver={(e) => {
                  e.preventDefault();
                  e.dataTransfer.dropEffect = "move";
                  if (dragOverFolderIndex !== index) {
                    setDragOverFolderIndex(index);
                  }
                }}
                onDragLeave={(e) => {
                  if (e.currentTarget.contains(e.relatedTarget as Node)) return;
                  if (dragOverFolderIndex === index) {
                    setDragOverFolderIndex(null);
                  }
                }}
                onDrop={(e) => {
                  if (draggedFolderIndex === null && e.dataTransfer?.files?.length > 0) {
                    handleExternalDrop(e);
                    return;
                  }
                  e.preventDefault();
                  e.stopPropagation();
                  const rawIndex =
                    e.dataTransfer.getData("application/x-imv-folder-index") ||
                    e.dataTransfer.getData("text/plain");
                  const fromIndex =
                    rawIndex !== "" && !isNaN(Number(rawIndex))
                      ? Number(rawIndex)
                      : draggedFolderIndex;

                  if (fromIndex !== null && fromIndex !== index) {
                    reorderFolders(fromIndex, index);
                  }
                  setDraggedFolderIndex(null);
                  setDragOverFolderIndex(null);
                }}
                onDragEnd={() => {
                  setDraggedFolderIndex(null);
                  setDragOverFolderIndex(null);
                }}
                onClick={() => handleSelectFolder(folder.id)}
                className={`group flex flex-col px-2.5 py-1.5 rounded text-sm cursor-pointer transition relative gap-1 ${
                  isSelectedRoot
                    ? "bg-accent/20 text-accent font-medium border border-accent/40"
                    : isSelectedAny
                    ? "bg-accent/10 text-textPrimary border border-accent/25"
                    : "text-textSecondary hover:bg-surfaceLight hover:text-textPrimary"
                } ${draggedFolderIndex === index ? "opacity-40" : ""} ${
                  isDragOver ? "border-t-2 border-accent" : ""
                }`}
              >
                {/* 上段: ドラッグハンドル + 展開トグル + フォルダアイコン + フォルダ名 */}
                {/* 変更理由: フォルダ名が横幅不足で途切れるのを防止し、上段全体を使ってフル表示するため */}
                <div className="flex items-center gap-1.5 w-full min-w-0" title={folder.path}>
                  {/* ドラッグハンドル */}
                  <GripVertical className="w-3.5 h-3.5 text-textSecondary/40 group-hover:text-textSecondary/80 cursor-grab shrink-0 -ml-1 pointer-events-none" />

                  {/* サブフォルダ展開トグルボタン */}
                  <button
                    type="button"
                    onClick={(e) => handleToggleSubfolders(e, folder.id)}
                    onMouseDown={(e) => e.stopPropagation()}
                    title={
                      isExpanded
                        ? t("sidebar.collapseSubfoldersTooltip")
                        : t("sidebar.expandSubfoldersTooltip")
                    }
                    className="p-0.5 rounded hover:bg-surface text-textSecondary hover:text-accent transition shrink-0 cursor-pointer"
                  >
                    {isExpanded ? (
                      <ChevronDown className="w-3.5 h-3.5" />
                    ) : (
                      <ChevronRight className="w-3.5 h-3.5" />
                    )}
                  </button>

                  {isOffline ? (
                    <span title={t("sidebar.offlineTooltip")} className="pointer-events-none">
                      <WifiOff className="w-4 h-4 text-amber-500 shrink-0" />
                    </span>
                  ) : isExpanded ? (
                    <FolderOpen className="w-4 h-4 shrink-0 text-accent pointer-events-none" />
                  ) : (
                    <Folder className="w-4 h-4 shrink-0 text-textSecondary group-hover:text-accent pointer-events-none" />
                  )}
                  <span className="truncate font-medium text-textPrimary flex-1 min-w-0 pointer-events-none">
                    {folderName}
                  </span>
                </div>

                {/* 下段: ステータスバッジ（左） + アクションボタン群（右） */}
                {/* 変更理由: 選択時やホバー時に操作アイコンが表示されてもフォルダ名が縮まないようにするため */}
                <div className="flex items-center justify-between w-full min-w-0 pl-6 pr-0.5 mt-0.5">
                  {/* サムネイル生成状況・枚数バッジ */}
                  <span className="text-[11px] shrink-0 pointer-events-none">
                    {folder.imageCount === 0 ? (
                      <span className="text-textSecondary/60 font-mono text-[10px]">
                        {t("sidebar.imagesCount", { count: 0 })}
                      </span>
                    ) : isGenerating ? (
                      <span
                        className="text-amber-400 bg-amber-400/10 border border-amber-400/30 px-1.5 py-0.5 rounded text-[10px] font-mono"
                        title={t("sidebar.generating", {
                          done: folder.thumbCount ?? 0,
                          total: folder.imageCount,
                        })}
                      >
                        {t("sidebar.generatingCount", {
                          done: folder.thumbCount ?? 0,
                          total: folder.imageCount,
                        })}
                      </span>
                    ) : isWarningDone ? (
                      <span
                        className="text-amber-400 bg-amber-400/10 border border-amber-400/30 px-1.5 py-0.5 rounded text-[10px] font-medium"
                        title={t("sidebar.warningDoneTooltip", { failed: failedCount })}
                      >
                        {t("sidebar.warningDoneCount", {
                          done: folder.thumbCount ?? 0,
                          total: folder.imageCount,
                        })}
                      </span>
                    ) : isAllDone ? (
                      <span
                        className="text-emerald-400 bg-emerald-500/10 border border-emerald-500/30 px-1.5 py-0.5 rounded text-[10px] font-medium"
                        title={t("sidebar.allDoneTooltip")}
                      >
                        {t("sidebar.allDoneCount", { total: folder.imageCount })}
                      </span>
                    ) : (
                      <span className="text-textSecondary/80 font-mono text-[10px]">
                        {t("sidebar.imagesCount", { count: folder.imageCount })}
                      </span>
                    )}
                  </span>

                  {/* アクションボタン（モバイル環境または選択中は常時表示、PCホバー時も表示） */}
                  <div
                    className={`${
                      isMobileEnvironment() || isSelectedAny ? "flex" : "hidden group-hover:flex"
                    } items-center gap-0.5 shrink-0 pointer-events-auto`}
                    onMouseDown={(e) => e.stopPropagation()}
                  >
                    {index > 0 && (
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          reorderFolders(index, index - 1);
                        }}
                        title="上へ移動"
                        className="p-1 hover:text-accent transition text-textSecondary cursor-pointer"
                      >
                        <ChevronUp className="w-3.5 h-3.5" />
                      </button>
                    )}
                    {index < folders.length - 1 && (
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          reorderFolders(index, index + 1);
                        }}
                        title="下へ移動"
                        className="p-1 hover:text-accent transition text-textSecondary cursor-pointer"
                      >
                        <ChevronDown className="w-3.5 h-3.5" />
                      </button>
                    )}
                    <button
                      onClick={handleRescanMissing}
                      title={t("sidebar.recreateThumbsTooltip")}
                      className="p-1 hover:text-amber-400 transition text-textSecondary cursor-pointer"
                    >
                      <RefreshCw className="w-3.5 h-3.5" />
                    </button>
                    <button
                      onClick={(e) => handleRescanFolder(e, folder.id)}
                      title={t("sidebar.rescanFolderTooltip")}
                      className="p-1 hover:text-accent transition text-textSecondary cursor-pointer"
                    >
                      <FolderSync className="w-3.5 h-3.5" />
                    </button>
                    <button
                      onClick={(e) => handleRemoveFolder(e, folder.id)}
                      title={t("sidebar.removeFolderTooltip")}
                      className="p-1 hover:text-red-400 transition text-textSecondary cursor-pointer"
                    >
                      <Trash2 className="w-3.5 h-3.5" />
                    </button>
                  </div>
                </div>
              </div>

              {/* 展開時のサブフォルダ・アーカイブ階層ツリー */}
              {isExpanded && (
                <div className="space-y-0.5 pl-2 border-l border-border/50 ml-3.5 my-0.5">
                  {hierarchyNodes.length === 0 ? (
                    <div className="px-2.5 py-1 text-[11px] text-textSecondary/60 italic">
                      {t("sidebar.noSubfolders")}
                    </div>
                  ) : (
                    renderHierarchyNodes(folder.id, hierarchyNodes, 0)
                  )}
                </div>
              )}
            </div>
          );
        })}

        {/* 外部からのドラッグ＆ドロップ受け入れゾーン */}
        <div
          onDragOver={(e) => {
            if (draggedFolderIndex !== null || draggedBoardIndex !== null) return;
            e.preventDefault();
            setIsExternalDragOver(true);
          }}
          onDragLeave={() => setIsExternalDragOver(false)}
          onDrop={handleExternalDrop}
          onClick={handleAddFolder}
          className={`mt-3 p-3 border-2 border-dashed rounded-lg flex flex-col items-center justify-center text-center cursor-pointer transition ${
            isExternalDragOver
              ? "border-accent bg-accent/10 text-accent"
              : "border-border/60 text-textSecondary/60 hover:border-border hover:text-textSecondary"
          }`}
        >
          <Upload className="w-4 h-4 mb-1" />
          <span className="text-xs">{t("sidebar.dragDropDrop")}</span>
          <span className="text-[10px]">{t("sidebar.dragDropOrClick")}</span>
        </div>

        {/* ムードボード（資料グループ）セクション */}
        <div className="mt-6 pt-4 border-t border-border/40">
          <div className="flex items-center justify-between px-2 mb-2">
            <span className="text-[11px] font-semibold text-textSecondary uppercase tracking-wider flex items-center gap-1.5">
              <LayoutGrid className="w-3.5 h-3.5 text-accent" />
              {t("sidebar.moodboardSection")}
            </span>
            <button
              onClick={async () => {
                const name = prompt(t("sidebar.createBoardPrompt"));
                if (name && name.trim()) {
                  try {
                    const created = await backendApi.createBoard({ name: name.trim() });
                    const all = await backendApi.getBoards();
                    setBoards(all);
                    setActiveBoardId(created.id);
                    setCurrentView("board");
                  } catch (err: unknown) {
                    const errMsg = err instanceof Error ? err.message : String(err);
                    alert(t("sidebar.createBoardFailed", { error: errMsg }));
                  }
                }
              }}
              title={t("sidebar.createBoardTooltip")}
              className="p-1 hover:bg-surfaceLight rounded text-textSecondary hover:text-accent transition"
            >
              <Plus className="w-3.5 h-3.5" />
            </button>
          </div>

          <div className="space-y-1">
            {boards.length === 0 ? (
              <div className="px-3 py-2 text-[11px] text-textSecondary/60 italic">
                {t("sidebar.noBoardsCreated")}
              </div>
            ) : (
              boards.map((b, index) => {
                const isSelected = currentView === "board" && activeBoardId === b.id;
                const isDragOver =
                  dragOverBoardIndex === index &&
                  draggedBoardIndex !== null &&
                  draggedBoardIndex !== index;

                return (
                  <div
                    key={b.id}
                    draggable
                    onDragStart={(e) => {
                      setDraggedBoardIndex(index);
                      e.dataTransfer.effectAllowed = "move";
                      e.dataTransfer.setData("application/x-imv-board-index", String(index));
                      e.dataTransfer.setData("text/plain", String(index));
                    }}
                    onDragOver={(e) => {
                      e.preventDefault();
                      e.dataTransfer.dropEffect = "move";
                      if (dragOverBoardIndex !== index) {
                        setDragOverBoardIndex(index);
                      }
                    }}
                    onDragLeave={(e) => {
                      if (e.currentTarget.contains(e.relatedTarget as Node)) return;
                      if (dragOverBoardIndex === index) {
                        setDragOverBoardIndex(null);
                      }
                    }}
                    onDrop={(e) => {
                      e.preventDefault();
                      e.stopPropagation();
                      const rawIndex =
                        e.dataTransfer.getData("application/x-imv-board-index") ||
                        e.dataTransfer.getData("text/plain");
                      const fromIndex =
                        rawIndex !== "" && !isNaN(Number(rawIndex))
                          ? Number(rawIndex)
                          : draggedBoardIndex;

                      if (fromIndex !== null && fromIndex !== index) {
                        reorderBoards(fromIndex, index);
                      }
                      setDraggedBoardIndex(null);
                      setDragOverBoardIndex(null);
                    }}
                    onDragEnd={() => {
                      setDraggedBoardIndex(null);
                      setDragOverBoardIndex(null);
                    }}
                    onClick={() => handleSelectBoard(b.id)}
                    className={`flex items-center justify-between px-2.5 py-1.5 rounded-lg text-xs cursor-pointer group transition relative ${
                      isSelected
                        ? "bg-accent/20 text-accent font-medium border border-accent/40"
                        : "text-textSecondary hover:bg-surfaceLight hover:text-textPrimary"
                    } ${draggedBoardIndex === index ? "opacity-40" : ""} ${
                      isDragOver ? "border-t-2 border-accent" : ""
                    }`}
                  >
                    <div className="flex items-center gap-1.5 truncate flex-1 min-w-0 pointer-events-none">
                      <GripVertical className="w-3.5 h-3.5 text-textSecondary/40 group-hover:text-textSecondary/80 cursor-grab shrink-0 -ml-1" />
                      <LayoutGrid className="w-3.5 h-3.5 shrink-0 text-textSecondary group-hover:text-accent" />
                      <span className="truncate">{b.name}</span>
                      <span className="text-[10px] text-textSecondary/70 shrink-0">({b.itemCount})</span>
                    </div>

                    <div
                      className="flex items-center gap-0.5 shrink-0 pointer-events-auto"
                      onMouseDown={(e) => e.stopPropagation()}
                    >
                      {index > 0 && (
                        <button
                          onClick={(e) => {
                            e.stopPropagation();
                            reorderBoards(index, index - 1);
                          }}
                          title="上へ移動"
                          className="hidden group-hover:block p-1 hover:text-accent transition text-textSecondary"
                        >
                          <ChevronUp className="w-3 h-3" />
                        </button>
                      )}
                      {index < boards.length - 1 && (
                        <button
                          onClick={(e) => {
                            e.stopPropagation();
                            reorderBoards(index, index + 1);
                          }}
                          title="下へ移動"
                          className="hidden group-hover:block p-1 hover:text-accent transition text-textSecondary"
                        >
                          <ChevronDown className="w-3 h-3" />
                        </button>
                      )}
                      <button
                        onClick={async (e) => {
                          e.stopPropagation();
                          if (confirm(t("sidebar.deleteBoardConfirm", { name: b.name }))) {
                            await backendApi.deleteBoard(b.id);
                            const updated = await backendApi.getBoards();
                            setBoards(updated);
                            if (activeBoardId === b.id) {
                              setCurrentView("timeline");
                              setActiveBoardId(null);
                            }
                          }
                        }}
                        title={t("sidebar.deleteBoardTooltip")}
                        className="hidden group-hover:block p-1 hover:text-red-400 transition ml-1"
                      >
                        <Trash2 className="w-3 h-3" />
                      </button>
                    </div>
                  </div>
                );
              })
            )}
          </div>
        </div>
      </div>

      {/* サイドバー最下部: 言語切り替えトグル */}
      <div className="p-3 border-t border-border bg-surfaceLight/20 flex items-center justify-between text-xs text-textSecondary">
        <div className="flex items-center gap-1.5 font-medium">
          <Globe className="w-3.5 h-3.5 text-accent" />
          <span className="text-[11px]">{t("sidebar.language")}</span>
        </div>
        <div className="flex items-center bg-surface border border-border/80 rounded-lg p-0.5 text-[11px]">
          <button
            onClick={() => setLocale("ja")}
            className={`px-2 py-0.5 rounded transition font-medium ${
              locale === "ja"
                ? "bg-accent text-white shadow-xs"
                : "text-textSecondary hover:text-textPrimary"
            }`}
          >
            日本語
          </button>
          <button
            onClick={() => setLocale("en")}
            className={`px-2 py-0.5 rounded transition font-medium ${
              locale === "en"
                ? "bg-accent text-white shadow-xs"
                : "text-textSecondary hover:text-textPrimary"
            }`}
          >
            English
          </button>
        </div>
      </div>

      {/* デスクトップ環境用ドラッグスプリッター（ペイン幅変更リサイザー） */}
      {/* 変更理由: フォルダ名の長さやユーザー環境に応じてサイドバー幅を自由に調整可能にするため */}
      {!isCompact && (
        <div
          onMouseDown={handleMouseDownResize}
          onDoubleClick={handleDoubleClickReset}
          title="ドラッグで幅を変更（ダブルクリックでリセット）"
          className={`absolute top-0 right-0 bottom-0 w-1 hover:w-1.5 cursor-col-resize z-30 transition-colors ${
            isResizing ? "bg-accent w-1.5" : "hover:bg-accent/70 bg-transparent"
          } after:absolute after:inset-y-0 after:-left-2 after:-right-2 after:content-['']`}
        />
      )}
    </aside>
  );

  return (
    <>
      {/* モバイル時のドロワー背景オーバーレイ（タップでドロワーを閉じる） */}
      {isCompact && (
        <div
          onClick={() => setSidebarOpen(false)}
          className="fixed inset-0 bg-black/60 backdrop-blur-xs z-40 transition-opacity animate-in fade-in duration-200"
        />
      )}
      {sidebarContent}
      <AddFolderModal
        isOpen={isAddFolderModalOpen}
        onClose={() => setIsAddFolderModalOpen(false)}
        onSelectPath={addFolderByPath}
      />
    </>
  );
};
