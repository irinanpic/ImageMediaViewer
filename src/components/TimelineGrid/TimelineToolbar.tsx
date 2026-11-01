import React, { useEffect, useRef, useState } from "react";
import {
  BookOpen,
  FileArchive,
  Filter,
  FolderTree,
  Menu,
  Plus,
  RotateCw,
  Search,
  SlidersHorizontal,
  Tag,
  X,
} from "lucide-react";
import { BookmarkPopover } from "./BookmarkPopover";
import { backendApi } from "../../lib/ipc";
import { usePagedImages } from "../../hooks/usePagedImages";
import { useResponsiveLayout } from "../../hooks/useResponsiveLayout";
import { useTranslation } from "../../locales";
import { useAppStore } from "../../store";
import {
  countPopoverFilterConditions,
  isTimelineFilterActive,
} from "../../types/filter";
import type { TimelineSort } from "../../types/generated/TimelineSort";

/**
 * フィルタポップオーバーで選択可能な対応フォーマット一覧
 *
 * 変更理由: スキャナーおよびプラグインで実際にサポートされている画像形式のみに限定し、
 * 非対応形式（svg, avif, tiff）を一覧から除外してユーザーの混乱を防止する。
 */
const FILTERABLE_FORMATS = [
  "jpg",
  "png",
  "webp",
  "gif",
  "bmp",
  "clip",
];

/**
 * タイムライン表示ツールバー（表示順切替・検索・複合フィルタ・しおり・ムードボード追加・現画面再読込）
 *
 * 変更理由: 仕様要求「ファイル名検索（AND/OR/NOT）」「拡張子・縦横比・タグによる複合絞り込み」を実装し、
 * 画面遷移（ビューア開閉やムードボード遷移）時にもストアの filterState を完全維持する。
 */
export const TimelineToolbar: React.FC = () => {
  const { t } = useTranslation();
  const { isCompact } = useResponsiveLayout();
  const isSidebarOpen = useAppStore((state) => state.isSidebarOpen);
  const toggleSidebar = useAppStore((state) => state.toggleSidebar);
  const timelineSort = useAppStore((state) => state.timelineSort);
  const setTimelineSort = useAppStore((state) => state.setTimelineSort);
  const totalImages = useAppStore((state) => state.totalImages);
  const selectedCellIndex = useAppStore((state) => state.selectedCellIndex);
  const selectedImageIds = useAppStore((state) => state.selectedImageIds);
  const openAddToBoardModal = useAppStore((state) => state.openAddToBoardModal);
  const refreshTimeline = useAppStore((state) => state.refreshTimeline);
  const selectedFolderId = useAppStore((state) => state.selectedFolderId);
  const setCatalogSummary = useAppStore((state) => state.setCatalogSummary);
  const filterState = useAppStore((state) => state.filterState);
  const updateFilterState = useAppStore((state) => state.updateFilterState);
  const clearFilterState = useAppStore((state) => state.clearFilterState);
  const tags = useAppStore((state) => state.tags);
  const openViewer = useAppStore((state) => state.openViewer);
  const { getImageByIndex, ensureIndex } = usePagedImages();

  const [isRefreshing, setIsRefreshing] = useState(false);
  const [isFilterOpen, setIsFilterOpen] = useState(false);
  const [searchInput, setSearchInput] = useState(filterState.query ?? "");
  const filterPopoverRef = useRef<HTMLDivElement>(null);

  // 外部（フィルタ一括解除など）で filterState.query が変更された場合に入力欄へ同期
  useEffect(() => {
    setSearchInput(filterState.query ?? "");
  }, [filterState.query]);

  // 検索テキストの250msデバウンス反映
  useEffect(() => {
    const timer = setTimeout(() => {
      const currentQuery = filterState.query ?? "";
      if (searchInput !== currentQuery) {
        updateFilterState({ query: searchInput.trim() ? searchInput : null });
      }
    }, 250);
    return () => clearTimeout(timer);
  }, [searchInput, filterState.query, updateFilterState]);

  // フィルタポップオーバー外クリックで閉じる
  useEffect(() => {
    if (!isFilterOpen) return;
    const handlePointerDown = (e: MouseEvent | TouchEvent) => {
      if (
        filterPopoverRef.current &&
        !filterPopoverRef.current.contains(e.target as Node)
      ) {
        setIsFilterOpen(false);
      }
    };
    window.addEventListener("mousedown", handlePointerDown);
    window.addEventListener("touchstart", handlePointerDown);
    return () => {
      window.removeEventListener("mousedown", handlePointerDown);
      window.removeEventListener("touchstart", handlePointerDown);
    };
  }, [isFilterOpen]);

  const selectedRecord =
    selectedCellIndex !== null ? getImageByIndex(selectedCellIndex) : null;

  const handleRefresh = async () => {
    if (isRefreshing) return;
    setIsRefreshing(true);
    try {
      const activeFilter = isTimelineFilterActive(filterState)
        ? filterState
        : undefined;
      // 1. 最新サマリを取得して総件数・バケットを最新同期
      const summary = await backendApi.getTimelineSummary(
        selectedFolderId ?? undefined,
        timelineSort,
        activeFilter
      );
      setCatalogSummary(summary.total, summary.buckets, summary.version);

      // 2. タイムライン再検証をキック（既存キャッシュを維持したまま差分マージ）
      refreshTimeline();

      // 3. 失敗サムネイルの再作成をバックグラウンド要求
      backendApi.rescanMissingThumbnails().catch(() => {});
    } catch (e) {
      console.error("再読み込みエラー:", e);
      refreshTimeline();
    } finally {
      setTimeout(() => setIsRefreshing(false), 500);
    }
  };

  // 拡張子チップの3状態サイクル切替: 未選択 -> 含む(青) -> 除外:NOT(赤) -> 未選択
  const handleCycleFormat = (ext: string) => {
    const inc = filterState.extensions ?? [];
    const exc = filterState.excludeExtensions ?? [];

    if (inc.includes(ext)) {
      const nextInc = inc.filter((item) => item !== ext);
      const nextExc = [...exc, ext];
      updateFilterState({
        extensions: nextInc.length > 0 ? nextInc : null,
        excludeExtensions: nextExc,
      });
    } else if (exc.includes(ext)) {
      const nextExc = exc.filter((item) => item !== ext);
      updateFilterState({
        excludeExtensions: nextExc.length > 0 ? nextExc : null,
      });
    } else {
      const nextInc = [...inc, ext];
      updateFilterState({
        extensions: nextInc,
      });
    }
  };

  // タグチップの3状態サイクル切替: 未選択 -> 含む(青) -> 除外:NOT(赤) -> 未選択
  const handleCycleTag = (tagId: number) => {
    const inc = filterState.includeTagIds ?? [];
    const exc = filterState.excludeTagIds ?? [];

    if (inc.includes(tagId)) {
      const nextInc = inc.filter((id) => id !== tagId);
      const nextExc = [...exc, tagId];
      updateFilterState({
        includeTagIds: nextInc.length > 0 ? nextInc : null,
        excludeTagIds: nextExc,
      });
    } else if (exc.includes(tagId)) {
      const nextExc = exc.filter((id) => id !== tagId);
      updateFilterState({
        excludeTagIds: nextExc.length > 0 ? nextExc : null,
      });
    } else {
      const nextInc = [...inc, tagId];
      updateFilterState({
        includeTagIds: nextInc,
      });
    }
  };

  const sortOptions: { value: TimelineSort; label: string }[] = [
    { value: "folder", label: t("timeline.sortFolder") },
    { value: "takenAtDesc", label: t("timeline.sortTakenAtDesc") },
    { value: "takenAtAsc", label: t("timeline.sortTakenAtAsc") },
    { value: "nameAsc", label: t("timeline.sortNameAsc") },
    { value: "nameDesc", label: t("timeline.sortNameDesc") },
  ];

  const hasAnyActiveFilter = isTimelineFilterActive(filterState);
  const popoverConditionCount = countPopoverFilterConditions(filterState);
  const queryModeUpper = (filterState.queryMode ?? "and").toUpperCase();
  const tagModeUpper = (filterState.tagMode ?? "and").toUpperCase();

  // 現在選択されているサブフォルダパスがアーカイブ（またはアーカイブ内フォルダ）かどうかを判定
  const subFolderPath = filterState.subFolderPath ?? "";
  const isArchiveSubFolder =
    subFolderPath.includes("!/") ||
    /\.(zip|cbz|rar|cbr|7z|cb7|tar)$/i.test(subFolderPath);
  const archiveDisplayName = isArchiveSubFolder
    ? subFolderPath.split("!/")[0].split(/[\\/]/).pop() || subFolderPath
    : "";

  const handleReadFromStart = async () => {
    if (totalImages <= 0) return;
    const firstRecord = getImageByIndex(0) ?? (await ensureIndex(0));
    if (firstRecord) {
      openViewer(firstRecord.id, 0);
    }
  };

  return (
    <div
      className="border-b border-border bg-surface flex flex-wrap items-center justify-between gap-1.5 text-xs select-none z-20 shrink-0 min-h-10 px-3 py-1"
      style={{
        paddingTop: "calc(0.25rem + var(--sat))",
        paddingLeft: "calc(0.75rem + var(--sal))",
        paddingRight: "calc(0.75rem + var(--sar))",
      }}
    >
      {/* 左側: ハンバーガーメニュー(モバイル時) + ソート切替 + 検索ボックス + フィルタ + 更新 */}
      <div className="flex flex-wrap items-center gap-1.5 flex-1 min-w-0">
        {/* モバイル時のドロワー開閉ハンバーガーボタン */}
        {isCompact && (
          <button
            onClick={toggleSidebar}
            className="p-1.5 text-textSecondary hover:text-textPrimary bg-surfaceLight/50 hover:bg-surfaceLight rounded-md transition mr-1 shrink-0 active:scale-95 cursor-pointer"
            title={
              isSidebarOpen
                ? t("statusBar.sidebarClose")
                : t("statusBar.sidebarOpen")
            }
          >
            <Menu className="w-4 h-4" />
          </button>
        )}

        <SlidersHorizontal className="w-3.5 h-3.5 text-textSecondary shrink-0 hidden sm:block" />
        {!isCompact && (
          <span className="text-textSecondary font-medium shrink-0">
            {t("timeline.sortOrder")}
          </span>
        )}
        <select
          value={timelineSort}
          onChange={(e) => setTimelineSort(e.target.value as TimelineSort)}
          className="bg-surfaceLight text-textPrimary border border-border rounded px-2 py-1 text-xs outline-none focus:border-accent cursor-pointer hover:bg-surfaceLight/80 transition max-w-[140px] truncate"
        >
          {sortOptions.map((opt) => (
            <option key={opt.value} value={opt.value}>
              {opt.label}
            </option>
          ))}
        </select>

        {!isCompact && (
          <span className="text-textSecondary/80 ml-1 shrink-0 font-mono">
            {t("timeline.imageCount", { count: totalImages.toLocaleString() })}
          </span>
        )}

        {/* ファイル名検索ボックス + AND/OR トグル */}
        <div className="flex items-center bg-surfaceLight border border-border focus-within:border-accent rounded px-2 py-0.5 gap-1 min-w-[140px] max-w-[240px] flex-1 transition">
          <Search className="w-3.5 h-3.5 text-textSecondary shrink-0" />
          <input
            type="text"
            value={searchInput}
            onChange={(e) => setSearchInput(e.target.value)}
            placeholder={t("timeline.searchPlaceholder")}
            className="bg-transparent text-textPrimary text-xs outline-none flex-1 min-w-0 py-0.5 placeholder:text-textSecondary/60"
          />
          {searchInput && (
            <button
              type="button"
              onClick={() => {
                setSearchInput("");
                updateFilterState({ query: null });
              }}
              title={t("timeline.searchClearTooltip")}
              className="text-textSecondary hover:text-textPrimary p-0.5 cursor-pointer shrink-0"
            >
              <X className="w-3 h-3" />
            </button>
          )}
          <button
            type="button"
            onClick={() =>
              updateFilterState({
                queryMode: queryModeUpper === "OR" ? "and" : "or",
              })
            }
            title={t("timeline.searchModeTooltip", { mode: queryModeUpper })}
            className={`px-1 py-0.5 rounded text-[10px] font-bold transition cursor-pointer shrink-0 ${
              queryModeUpper === "OR"
                ? "bg-amber-500/20 text-amber-300 border border-amber-500/40"
                : "bg-surface text-textSecondary hover:text-textPrimary border border-border/60"
            }`}
          >
            {queryModeUpper}
          </button>
        </div>

        {/* 複合フィルタ（拡張子・縦横比・タグ）ポップオーバーボタン */}
        <div className="relative" ref={filterPopoverRef}>
          <button
            type="button"
            onClick={() => setIsFilterOpen((prev) => !prev)}
            title={t("timeline.filterBtnTooltip")}
            className={`flex items-center gap-1 px-2 py-1 rounded text-xs font-medium border transition cursor-pointer shrink-0 ${
              popoverConditionCount > 0 || isFilterOpen
                ? "bg-accent/20 text-accent border-accent/50"
                : "bg-surfaceLight hover:bg-surfaceLight/80 text-textSecondary hover:text-textPrimary border-border"
            }`}
          >
            <Filter className="w-3.5 h-3.5" />
            {!isCompact && <span>{t("timeline.filterBtn")}</span>}
            {popoverConditionCount > 0 && (
              <span className="bg-accent text-white text-[10px] font-bold px-1.5 py-0.2 rounded-full leading-tight">
                {popoverConditionCount}
              </span>
            )}
          </button>

          {isFilterOpen && (
            <div className="absolute left-0 top-full mt-1.5 w-80 max-w-[90vw] bg-surface/95 backdrop-blur-md border border-border rounded-xl shadow-2xl p-3.5 z-50 space-y-3.5 animate-in fade-in zoom-in-95 duration-100">
              {/* 1. 拡張子フィルタ */}
              <div className="space-y-1.5">
                <div className="text-[11px] font-semibold text-textSecondary">
                  {t("timeline.filterSectionFormats")}
                </div>
                <div className="flex flex-wrap gap-1.5">
                  {FILTERABLE_FORMATS.map((ext) => {
                    const isIncluded = (filterState.extensions ?? []).includes(ext);
                    const isExcluded = (filterState.excludeExtensions ?? []).includes(ext);
                    return (
                      <button
                        key={ext}
                        type="button"
                        onClick={() => handleCycleFormat(ext)}
                        className={`px-2 py-0.5 rounded text-[11px] font-mono font-medium border transition cursor-pointer ${
                          isIncluded
                            ? "bg-accent text-white border-accent shadow-xs"
                            : isExcluded
                            ? "bg-red-500/20 text-red-300 border-red-500/50 line-through"
                            : "bg-surfaceLight text-textSecondary hover:text-textPrimary border-border/70"
                        }`}
                      >
                        {isExcluded ? `NOT .${ext}` : `.${ext}`}
                      </button>
                    );
                  })}
                </div>
              </div>

              {/* 2. 縦横比フィルタ */}
              <div className="space-y-1.5 pt-2 border-t border-border/50">
                <div className="text-[11px] font-semibold text-textSecondary">
                  {t("timeline.filterSectionAspectRatio")}
                </div>
                <div className="grid grid-cols-4 gap-1">
                  {[
                    { value: null, label: t("timeline.aspectAll") },
                    { value: "landscape", label: t("timeline.aspectLandscape") },
                    { value: "portrait", label: t("timeline.aspectPortrait") },
                    { value: "square", label: t("timeline.aspectSquare") },
                  ].map((opt) => {
                    const isSelected = (filterState.aspectRatio ?? null) === opt.value;
                    return (
                      <button
                        key={opt.value ?? "all"}
                        type="button"
                        onClick={() => updateFilterState({ aspectRatio: opt.value })}
                        className={`py-1 px-1.5 rounded text-[11px] font-medium border transition cursor-pointer truncate ${
                          isSelected
                            ? "bg-accent text-white border-accent"
                            : "bg-surfaceLight text-textSecondary hover:text-textPrimary border-border/70"
                        }`}
                      >
                        {opt.label}
                      </button>
                    );
                  })}
                </div>
              </div>

              {/* 3. タグ絞り込み */}
              <div className="space-y-1.5 pt-2 border-t border-border/50">
                <div className="flex items-center justify-between">
                  <span className="text-[11px] font-semibold text-textSecondary flex items-center gap-1">
                    <Tag className="w-3 h-3 text-accent" />
                    {t("timeline.filterSectionTags")}
                  </span>
                  <button
                    type="button"
                    onClick={() =>
                      updateFilterState({
                        tagMode: tagModeUpper === "OR" ? "and" : "or",
                      })
                    }
                    className={`px-1.5 py-0.5 rounded text-[10px] font-bold border transition cursor-pointer ${
                      tagModeUpper === "OR"
                        ? "bg-amber-500/20 text-amber-300 border-amber-500/40"
                        : "bg-surfaceLight text-textSecondary hover:text-textPrimary border-border/70"
                    }`}
                  >
                    {tagModeUpper}
                  </button>
                </div>

                {tags.length === 0 ? (
                  <div className="text-[11px] text-textSecondary/60 italic py-1">
                    {t("timeline.filterNoTags")}
                  </div>
                ) : (
                  <div className="flex flex-wrap gap-1.5 max-h-36 overflow-y-auto pr-1">
                    {tags.map((tag) => {
                      const isInc = (filterState.includeTagIds ?? []).includes(tag.id);
                      const isExc = (filterState.excludeTagIds ?? []).includes(tag.id);
                      return (
                        <button
                          key={tag.id}
                          type="button"
                          onClick={() => handleCycleTag(tag.id)}
                          className={`inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[11px] font-medium border transition cursor-pointer ${
                            isInc
                              ? "bg-accent text-white border-accent shadow-xs"
                              : isExc
                              ? "bg-red-500/20 text-red-300 border-red-500/50 line-through"
                              : "bg-surfaceLight text-textSecondary hover:text-textPrimary border-border/70"
                          }`}
                        >
                          <span
                            className="w-2 h-2 rounded-full shrink-0"
                            style={{ backgroundColor: tag.color || "#6366f1" }}
                          />
                          <span className="truncate max-w-[120px]">
                            {isExc ? `NOT ${tag.name}` : tag.name}
                          </span>
                          <span className="text-[10px] opacity-75 font-mono">
                            ({tag.imageCount})
                          </span>
                        </button>
                      );
                    })}
                  </div>
                )}
              </div>
            </div>
          )}
        </div>

        {/* サブフォルダまたはアーカイブ選択中バッジ & 先頭から読むボタン */}
        {filterState.subFolderPath && (
          <div
            className={`inline-flex items-center gap-1 px-2 py-0.5 rounded text-[11px] font-medium max-w-[220px] border ${
              isArchiveSubFolder
                ? "bg-amber-500/15 text-amber-300 border-amber-500/40"
                : "bg-accent/15 text-accent border-accent/40"
            }`}
          >
            {isArchiveSubFolder ? (
              <FileArchive className="w-3.5 h-3.5 shrink-0 text-amber-400" />
            ) : (
              <FolderTree className="w-3 h-3 shrink-0" />
            )}
            <span className="truncate" title={filterState.subFolderPath}>
              {isArchiveSubFolder
                ? `${archiveDisplayName} (${totalImages.toLocaleString()})`
                : filterState.subFolderPath}
            </span>
            <button
              type="button"
              onClick={() => updateFilterState({ subFolderPath: null })}
              className="hover:text-white cursor-pointer shrink-0"
            >
              <X className="w-3 h-3" />
            </button>
          </div>
        )}

        {isArchiveSubFolder && totalImages > 0 && (
          <button
            type="button"
            onClick={handleReadFromStart}
            title={t("timeline.readFromStartTooltip")}
            className="inline-flex items-center gap-1 bg-amber-500 hover:bg-amber-400 text-black font-semibold px-2.5 py-1 rounded text-xs transition shadow-xs cursor-pointer shrink-0 active:scale-95"
          >
            <BookOpen className="w-3.5 h-3.5 shrink-0" />
            <span>{t("timeline.readFromStartBtn")}</span>
          </button>
        )}

        {/* 全フィルタ一括解除ボタン */}
        {hasAnyActiveFilter && (
          <button
            type="button"
            onClick={() => {
              setSearchInput("");
              clearFilterState();
            }}
            title={t("timeline.clearFilterTooltip")}
            className="flex items-center gap-1 bg-red-500/15 hover:bg-red-500/25 text-red-300 border border-red-500/30 px-2 py-1 rounded text-xs font-medium transition cursor-pointer shrink-0"
          >
            <X className="w-3.5 h-3.5" />
            {!isCompact && <span>{t("timeline.clearFilterBtn")}</span>}
          </button>
        )}

        {/* 現画面再読込ボタン */}
        <button
          onClick={handleRefresh}
          disabled={isRefreshing}
          className="flex items-center gap-1 bg-surfaceLight hover:bg-surfaceLight/80 text-textSecondary hover:text-textPrimary border border-border px-2 py-1 rounded text-xs font-medium transition shadow-xs cursor-pointer active:scale-95 disabled:opacity-60 shrink-0"
          title={t("timeline.refreshTooltip")}
        >
          <RotateCw
            className={`w-3.5 h-3.5 ${
              isRefreshing ? "animate-spin text-accent" : ""
            }`}
          />
          {!isCompact && <span>{t("timeline.refreshBtn")}</span>}
        </button>

        {/* 表示位置のしおり（ブックマーク）ポップオーバー */}
        <BookmarkPopover />

        {selectedImageIds.length > 0 ? (
          <button
            onClick={() => openAddToBoardModal(selectedImageIds)}
            className="flex items-center gap-1.5 bg-accent hover:bg-accent/90 text-white border border-accent/40 px-2.5 py-1 rounded text-xs font-medium transition ml-1 shadow-sm cursor-pointer shrink-0 truncate"
            title={t("timeline.addToBoardSelectedTooltip", {
              count: selectedImageIds.length,
            })}
          >
            <Plus className="w-3.5 h-3.5 shrink-0" />
            <span className="truncate">
              {t("timeline.addToBoardSelected", {
                count: selectedImageIds.length,
              })}
            </span>
          </button>
        ) : selectedRecord ? (
          <button
            onClick={() => openAddToBoardModal([selectedRecord.id])}
            className="flex items-center gap-1 bg-accent/20 hover:bg-accent text-accent hover:text-white border border-accent/40 px-2 py-1 rounded text-xs font-medium transition ml-1 shadow-sm cursor-pointer shrink-0"
            title={t("timeline.addToBoardCurrentTooltip")}
          >
            <Plus className="w-3.5 h-3.5 shrink-0" />
            {!isCompact && <span>{t("timeline.addToBoardCurrent")}</span>}
          </button>
        ) : null}
      </div>
    </div>
  );
};
