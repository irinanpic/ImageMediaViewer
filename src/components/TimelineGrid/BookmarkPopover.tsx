import React, { useEffect, useRef, useState } from "react";
import { Bookmark as BookmarkIcon, ChevronRight, Clock, Folder, Trash2, X } from "lucide-react";
import { useTranslation } from "../../locales";
import { useAppStore } from "../../store";
import type { Bookmark } from "../../types/bookmark";

/**
 * タイムライン表示位置のしおり（ブックマーク）一覧・追加ポップオーバー
 *
 * 変更理由: ユーザーが閲覧中のタイムライン位置（日付、通し番号、スクロール位置）を
 * 自由に保存し、後から一覧からワンクリックでその位置へ即座に復帰できるようにするため。
 */
export const BookmarkPopover: React.FC = () => {
  const { t } = useTranslation();
  const [isOpen, setIsOpen] = useState(false);
  const [customTitle, setCustomTitle] = useState("");
  const popoverRef = useRef<HTMLDivElement>(null);

  const bookmarks = useAppStore((state) => state.bookmarks);
  const addBookmark = useAppStore((state) => state.addBookmark);
  const removeBookmark = useAppStore((state) => state.removeBookmark);
  const jumpToBookmark = useAppStore((state) => state.jumpToBookmark);
  const currentVisibleInfo = useAppStore((state) => state.currentVisibleInfo);
  const selectedFolderId = useAppStore((state) => state.selectedFolderId);
  const folders = useAppStore((state) => state.folders);
  const timelineSort = useAppStore((state) => state.timelineSort);
  const fetchBookmarks = useAppStore((state) => state.fetchBookmarks);

  // 現在選択中のフォルダ名を取得
  const currentFolder = selectedFolderId ? folders.find((f) => f.id === selectedFolderId) : null;
  const currentFolderName = currentFolder
    ? (currentFolder.path.split(/[\\/]/).filter(Boolean).pop() || currentFolder.path)
    : t("bookmark.all");

  // 現在位置に基づくデフォルトタイトルの自動生成
  const generateDefaultTitle = () => {
    if (!currentVisibleInfo) return t("bookmark.defaultTitleFallback");
    const day = currentVisibleInfo.dayLabel || t("timeline.timelinePosition");
    if (typeof currentVisibleInfo.imageIndex === "number") {
      return t("bookmark.defaultTitleWithIndex", {
        day,
        index: (currentVisibleInfo.imageIndex + 1).toLocaleString(),
      });
    }
    return t("bookmark.defaultTitleDay", { day });
  };

  // ポップオーバーを開いた際にデフォルトタイトルを入力欄にセット & 最新しおり一覧を同期
  useEffect(() => {
    if (isOpen) {
      setCustomTitle(generateDefaultTitle());
      fetchBookmarks();
    }
  }, [isOpen, currentVisibleInfo, fetchBookmarks]);

  // 外側クリックでポップオーバーを閉じる
  useEffect(() => {
    if (!isOpen) return;

    const handleClickOutside = (e: MouseEvent) => {
      if (popoverRef.current && !popoverRef.current.contains(e.target as Node)) {
        setIsOpen(false);
      }
    };

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setIsOpen(false);
      }
    };

    document.addEventListener("mousedown", handleClickOutside);
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("mousedown", handleClickOutside);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [isOpen]);

  // 現在位置の保存処理
  const handleSave = () => {
    if (!currentVisibleInfo) return;

    const title = customTitle.trim() || generateDefaultTitle();
    addBookmark({
      title,
      folderId: selectedFolderId,
      folderName: currentFolderName,
      sort: timelineSort,
      scrollTop: currentVisibleInfo.scrollTop,
      rowIndex: currentVisibleInfo.rowIndex,
      imageIndex: currentVisibleInfo.imageIndex,
      dayLabel: currentVisibleInfo.dayLabel,
    });

    setCustomTitle("");
  };

  // しおりクリックでジャンプして閉じる
  const handleJump = (bookmark: Bookmark) => {
    jumpToBookmark(bookmark);
    setIsOpen(false);
  };

  return (
    <div className="relative inline-block" ref={popoverRef}>
      {/* ツールバートリガーボタン */}
      <button
        onClick={() => setIsOpen((prev) => !prev)}
        className={`flex items-center gap-1.5 px-2.5 py-1 rounded text-xs font-medium transition cursor-pointer border shadow-xs ${
          isOpen
            ? "bg-accent text-white border-accent shadow-accent/20"
            : "bg-surfaceLight hover:bg-surfaceLight/80 text-textSecondary hover:text-textPrimary border-border"
        }`}
        title={t("bookmark.buttonTooltip")}
      >
        <BookmarkIcon className="w-3.5 h-3.5" />
        <span>{t("bookmark.button")}</span>
        {bookmarks.length > 0 && (
          <span className="ml-0.5 px-1.5 py-0.2 bg-accent/30 text-accent font-semibold rounded-full text-[10px] group-hover:bg-accent group-hover:text-white">
            {bookmarks.length}
          </span>
        )}
      </button>

      {/* ポップオーバー本体 */}
      {isOpen && (
        <div className="absolute right-0 mt-2 w-80 bg-surface/95 backdrop-blur-md border border-border rounded-xl shadow-2xl z-50 overflow-hidden flex flex-col text-xs text-textPrimary animate-in fade-in zoom-in-95 duration-100">
          {/* ポップオーバーヘッダー */}
          <div className="flex items-center justify-between px-3 py-2.5 border-b border-border bg-surfaceLight/40">
            <div className="flex items-center gap-1.5 font-semibold text-textPrimary">
              <BookmarkIcon className="w-4 h-4 text-accent" />
              <span>{t("bookmark.title")}</span>
            </div>
            <button
              onClick={() => setIsOpen(false)}
              className="p-1 rounded hover:bg-surfaceLight text-textSecondary hover:text-textPrimary transition"
            >
              <X className="w-3.5 h-3.5" />
            </button>
          </div>

          {/* 現在位置の追加フォーム */}
          <div className="p-3 border-b border-border/80 bg-surfaceLight/20 flex flex-col gap-2">
            <div className="text-[11px] text-textSecondary flex items-center justify-between">
              <span>{t("bookmark.currentPosition")}</span>
              <span className="font-medium text-textPrimary truncate max-w-[160px]">
                {currentVisibleInfo?.dayLabel ?? t("bookmark.positionStart")}
                {typeof currentVisibleInfo?.imageIndex === "number" && (
                  ` (${(currentVisibleInfo.imageIndex + 1).toLocaleString()}${t("common.images")})`
                )}
              </span>
            </div>
            <div className="flex gap-1.5">
              <input
                type="text"
                value={customTitle}
                onChange={(e) => setCustomTitle(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    handleSave();
                  }
                }}
                placeholder={t("bookmark.inputPlaceholder")}
                className="flex-1 bg-surface border border-border rounded px-2.5 py-1.5 text-xs outline-none focus:border-accent text-textPrimary placeholder:text-textSecondary/50"
              />
              <button
                onClick={handleSave}
                className="flex items-center gap-1 bg-accent hover:bg-accent/90 text-white px-3 py-1.5 rounded font-medium transition cursor-pointer shrink-0 shadow-sm"
              >
                <BookmarkIcon className="w-3.5 h-3.5" />
                <span>{t("bookmark.saveBtn")}</span>
              </button>
            </div>
          </div>

          {/* 登録済みしおり一覧 */}
          <div className="max-h-72 overflow-y-auto divide-y divide-border/40">
            {bookmarks.length === 0 ? (
              <div className="py-8 text-center text-textSecondary text-[11px] whitespace-pre-line">
                <BookmarkIcon className="w-8 h-8 text-textSecondary/30 mx-auto mb-2" />
                {t("bookmark.noBookmarks")}
              </div>
            ) : (
              bookmarks.map((b) => (
                <div
                  key={b.id}
                  className="flex items-center justify-between px-3 py-2.5 hover:bg-surfaceLight/60 group transition cursor-pointer"
                  onClick={() => handleJump(b)}
                >
                  <div className="flex-1 min-w-0 pr-2">
                    <div className="font-semibold text-textPrimary truncate group-hover:text-accent transition flex items-center gap-1">
                      <span>{b.title}</span>
                    </div>
                    <div className="flex items-center gap-2 text-[10px] text-textSecondary mt-0.5">
                      <span className="flex items-center gap-0.5 truncate">
                        <Folder className="w-2.5 h-2.5" />
                        {b.folderName ?? t("bookmark.all")}
                      </span>
                      <span>•</span>
                      <span className="flex items-center gap-0.5">
                        <Clock className="w-2.5 h-2.5" />
                        {new Date(b.createdAt).toLocaleDateString()}
                      </span>
                    </div>
                  </div>

                  <div className="flex items-center gap-1 shrink-0">
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        removeBookmark(b.id);
                      }}
                      className="p-1 rounded opacity-0 group-hover:opacity-100 hover:bg-red-500/20 hover:text-red-400 text-textSecondary transition"
                      title={t("bookmark.deleteTooltip")}
                    >
                      <Trash2 className="w-3.5 h-3.5" />
                    </button>
                    <ChevronRight className="w-3.5 h-3.5 text-textSecondary/50 group-hover:text-accent transition" />
                  </div>
                </div>
              ))
            )}
          </div>
        </div>
      )}
    </div>
  );
};
