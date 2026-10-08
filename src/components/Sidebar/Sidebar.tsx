import React, { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Folder,
  FolderPlus,
  FolderSync,
  Globe,
  GripVertical,
  Layers,
  LayoutGrid,
  Plus,
  RefreshCw,
  Trash2,
  Upload,
  WifiOff,
} from "lucide-react";
import { backendApi } from "../../lib/ipc";
import { isTauriEnvironment } from "../../lib/thumbUrl";
import { useTranslation } from "../../locales";
import { useAppStore } from "../../store";
import type { WatchedFolder } from "../../types/generated/WatchedFolder";

export const Sidebar: React.FC = () => {
  const { t, locale, setLocale } = useTranslation();
  const folders = useAppStore((state) => state.folders);
  const selectedFolderId = useAppStore((state) => state.selectedFolderId);
  const setSelectedFolderId = useAppStore((state) => state.setSelectedFolderId);
  const reorderFolders = useAppStore((state) => state.reorderFolders);
  const isSidebarOpen = useAppStore((state) => state.isSidebarOpen);
  const currentView = useAppStore((state) => state.currentView);
  const setCurrentView = useAppStore((state) => state.setCurrentView);
  const boards = useAppStore((state) => state.boards);
  const setBoards = useAppStore((state) => state.setBoards);
  const totalImages = useAppStore((state) => state.totalImages);
  const thumbProgress = useAppStore((state) => state.thumbProgress);
  const activeBoardId = useAppStore((state) => state.activeBoardId);
  const setActiveBoardId = useAppStore((state) => state.setActiveBoardId);

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
  // セキュリティ制限による空パスを回避し、Tauri API経由で完全な絶対パスをダイレクトに自動登録する
  React.useEffect(() => {
    if (!isTauriEnvironment()) return;

    let unlistenFn: (() => void) | null = null;
    import("@tauri-apps/api/webview")
      .then(({ getCurrentWebview }) => {
        getCurrentWebview()
          .onDragDropEvent((event) => {
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
  const [isExternalDragOver, setIsExternalDragOver] = useState(false);

  // フォルダ追加ダイアログオープン
  const handleAddFolder = async () => {
    try {
      let selected: string | null = null;
      if (isTauriEnvironment()) {
        const res = await open({
          directory: true,
          multiple: false,
          title: t("sidebar.selectFolderDialogTitle"),
        });
        if (typeof res === "string") selected = res;
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
    e.preventDefault();
    setIsExternalDragOver(false);

    const files = Array.from(e.dataTransfer.files);
    if (files.length === 0) return;

    // 既存フォルダから親パスを推測して初期候補を生成
    let parentHint = "";
    if (folders.length > 0) {
      const lastPath = folders[0].path;
      const parts = lastPath.split(/[\\/]/).filter(Boolean);
      if (parts.length > 1) {
        parts.pop();
        parentHint = parts.join("\\");
      }
    }

    for (const file of files) {
      const filePath = (file as any).path as string | undefined;
      if (filePath) {
        // パスが取得できる場合（Electronや特定環境）
        await addFolderByPath(filePath);
      } else {
        // 通常のブラウザ環境ではセキュリティ制限でフルパスが取得できないためプロンプトで確認
        const guessedName = file.name;
        const suggestedPath = parentHint ? `${parentHint}\\${guessedName}` : `C:\\Pictures\\${guessedName}`;
        const inputPath = prompt(
          t("sidebar.dragDropPrompt", { name: guessedName }),
          suggestedPath
        );
        if (inputPath) {
          await addFolderByPath(inputPath);
        }
      }
    }
  };

  if (!isSidebarOpen) return null;

  return (
    <aside className="w-64 bg-surface border-r border-border flex flex-col h-full select-none z-10">
      {/* ヘッダ */}
      <div className="p-4 border-b border-border flex items-center justify-between">
        <h2 className="text-sm font-bold text-textPrimary tracking-wide">{t("sidebar.folders")}</h2>
        <button
          onClick={handleAddFolder}
          title={t("sidebar.addFolder")}
          className="flex items-center gap-1 text-xs bg-accent hover:bg-accentHover text-white px-2.5 py-1.5 rounded transition font-medium"
        >
          <FolderPlus className="w-4 h-4" />
          <span>{t("sidebar.addFolderBtn")}</span>
        </button>
      </div>

      {/* フォルダ一覧リスト */}
      <div className="flex-1 overflow-y-auto p-2 space-y-1">
        {/* すべての写真 */}
        <button
          onClick={() => setSelectedFolderId(null)}
          className={`w-full flex items-center justify-between px-3 py-2 rounded text-sm transition ${
            selectedFolderId === null
              ? "bg-accent/20 text-accent font-medium border border-accent/40"
              : "text-textSecondary hover:bg-surfaceLight hover:text-textPrimary"
          }`}
        >
          <div className="flex items-center gap-2 truncate">
            <Layers className="w-4 h-4 shrink-0" />
            <span className="truncate">{t("sidebar.allPhotos")}</span>
          </div>
          {totalImages > 0 && (
            <div className="text-[11px] shrink-0 font-mono flex items-center gap-1">
              {thumbProgress && thumbProgress.total > 0 && thumbProgress.done < thumbProgress.total ? (
                <span className="text-amber-400 bg-amber-400/10 border border-amber-400/30 px-1.5 py-0.5 rounded text-[10px]" title={t("sidebar.generating", { done: thumbProgress.done.toLocaleString(), total: totalImages.toLocaleString() })}>
                  {t("sidebar.generating", { done: thumbProgress.done.toLocaleString(), total: totalImages.toLocaleString() })}
                </span>
              ) : thumbProgress && thumbProgress.total > 0 && thumbProgress.done >= thumbProgress.total ? (
                <span className="text-emerald-400 bg-emerald-500/10 border border-emerald-500/30 px-1.5 py-0.5 rounded text-[10px] font-medium" title={t("sidebar.allDoneTooltip")}>
                  {t("sidebar.allDone", { total: totalImages.toLocaleString() })}
                </span>
              ) : (
                <span className="text-textSecondary/80 font-mono text-[10px]">{t("sidebar.imagesCount", { count: totalImages.toLocaleString() })}</span>
              )}
            </div>
          )}
        </button>

        {/* 登録フォルダ一覧（ドラッグ＆ドロップで並び替え可能） */}
        {folders.map((folder: WatchedFolder, index: number) => {
          const folderName = folder.path.split(/[\\/]/).filter(Boolean).pop() || folder.path;
          const isSelected = selectedFolderId === folder.id;
          const isOffline = folder.status === "offline";
          const isDragOver = dragOverFolderIndex === index;
          const isGenerating = folder.thumbCount !== undefined && folder.thumbCount < folder.imageCount;
          const isAllDone = folder.thumbCount !== undefined && folder.thumbCount >= folder.imageCount && folder.imageCount > 0;

          return (
            <div
              key={folder.id}
              draggable
              onDragStart={(e) => {
                setDraggedFolderIndex(index);
                e.dataTransfer.effectAllowed = "move";
                e.dataTransfer.setData("text/plain", String(index));
              }}
              onDragOver={(e) => {
                e.preventDefault();
                e.dataTransfer.dropEffect = "move";
                if (dragOverFolderIndex !== index) {
                  setDragOverFolderIndex(index);
                }
              }}
              onDragLeave={() => {
                if (dragOverFolderIndex === index) {
                  setDragOverFolderIndex(null);
                }
              }}
              onDrop={(e) => {
                e.preventDefault();
                e.stopPropagation();
                if (draggedFolderIndex !== null && draggedFolderIndex !== index) {
                  reorderFolders(draggedFolderIndex, index);
                }
                setDraggedFolderIndex(null);
                setDragOverFolderIndex(null);
              }}
              onDragEnd={() => {
                setDraggedFolderIndex(null);
                setDragOverFolderIndex(null);
              }}
              onClick={() => setSelectedFolderId(folder.id)}
              className={`group flex items-center justify-between px-2.5 py-2 rounded text-sm cursor-pointer transition relative ${
                isSelected
                  ? "bg-accent/20 text-accent font-medium border border-accent/40"
                  : "text-textSecondary hover:bg-surfaceLight hover:text-textPrimary"
              } ${isDragOver ? "border-t-2 border-accent" : ""}`}
            >
              <div className="flex items-center gap-1.5 truncate flex-1 min-w-0" title={folder.path}>
                {/* ドラッグハンドル */}
                <GripVertical className="w-3.5 h-3.5 text-textSecondary/40 group-hover:text-textSecondary/80 cursor-grab shrink-0 -ml-1" />

                {isOffline ? (
                  <span title={t("sidebar.offlineTooltip")}>
                    <WifiOff className="w-4 h-4 text-amber-500 shrink-0" />
                  </span>
                ) : (
                  <Folder className="w-4 h-4 shrink-0 text-textSecondary group-hover:text-accent" />
                )}
                <span className="truncate">{folderName}</span>

                {/* サムネイル生成状況・枚数バッジ */}
                <span className="text-[11px] shrink-0 ml-1">
                  {folder.imageCount === 0 ? (
                    <span className="text-textSecondary/60 font-mono text-[10px]">{t("sidebar.imagesCount", { count: 0 })}</span>
                  ) : isGenerating ? (
                    <span
                      className="text-amber-400 bg-amber-400/10 border border-amber-400/30 px-1.5 py-0.5 rounded text-[10px] font-mono"
                      title={t("sidebar.generating", { done: folder.thumbCount ?? 0, total: folder.imageCount })}
                    >
                      {t("sidebar.generatingCount", { done: folder.thumbCount ?? 0, total: folder.imageCount })}
                    </span>
                  ) : isAllDone ? (
                    <span
                      className="text-emerald-400 bg-emerald-500/10 border border-emerald-500/30 px-1.5 py-0.5 rounded text-[10px] font-medium"
                      title={t("sidebar.allDoneTooltip")}
                    >
                      {t("sidebar.allDoneCount", { total: folder.imageCount })}
                    </span>
                  ) : (
                    <span className="text-textSecondary/80 font-mono text-[10px]">{t("sidebar.imagesCount", { count: folder.imageCount })}</span>
                  )}
                </span>
              </div>

              {/* アクションボタン */}
              <div className="hidden group-hover:flex items-center gap-1 shrink-0 ml-2">
                <button
                  onClick={handleRescanMissing}
                  title={t("sidebar.recreateThumbsTooltip")}
                  className="p-1 hover:text-amber-400 transition"
                >
                  <RefreshCw className="w-3.5 h-3.5" />
                </button>
                <button
                  onClick={(e) => handleRescanFolder(e, folder.id)}
                  title={t("sidebar.rescanFolderTooltip")}
                  className="p-1 hover:text-accent transition"
                >
                  <FolderSync className="w-3.5 h-3.5" />
                </button>
                <button
                  onClick={(e) => handleRemoveFolder(e, folder.id)}
                  title={t("sidebar.removeFolderTooltip")}
                  className="p-1 hover:text-red-400 transition"
                >
                  <Trash2 className="w-3.5 h-3.5" />
                </button>
              </div>
            </div>
          );
        })}

        {/* 外部からのドラッグ＆ドロップ受け入れゾーン */}
        <div
          onDragOver={(e) => {
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
              boards.map((b) => {
                const isSelected = currentView === "board" && activeBoardId === b.id;
                return (
                  <div
                    key={b.id}
                    onClick={() => {
                      setActiveBoardId(b.id);
                      setCurrentView("board");
                    }}
                    className={`flex items-center justify-between px-2.5 py-1.5 rounded-lg text-xs cursor-pointer group transition ${
                      isSelected
                        ? "bg-accent/20 text-accent font-medium border border-accent/40"
                        : "text-textSecondary hover:bg-surfaceLight hover:text-textPrimary"
                    }`}
                  >
                    <div className="flex items-center gap-2 truncate flex-1 min-w-0">
                      <LayoutGrid className="w-3.5 h-3.5 shrink-0 text-textSecondary group-hover:text-accent" />
                      <span className="truncate">{b.name}</span>
                      <span className="text-[10px] text-textSecondary/70 shrink-0">({b.itemCount})</span>
                    </div>

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
    </aside>
  );
};
