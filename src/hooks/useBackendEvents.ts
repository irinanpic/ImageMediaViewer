import { useCallback, useEffect, useMemo } from "react";
import { listen } from "@tauri-apps/api/event";
import { backendApi } from "../lib/ipc";
import { isTauriEnvironment } from "../lib/thumbUrl";
import { useAppStore } from "../store";
import { isTimelineFilterActive, serializeTimelineFilter } from "../types/filter";
import type { CatalogChangedPayload } from "../types/generated/CatalogChangedPayload";
import type { ScanProgress } from "../types/generated/ScanProgress";
import type { ThumbProgress } from "../types/generated/ThumbProgress";

/**
 * バックエンドからのイベント通知を購読しストアを更新するフック
 *
 * 変更理由: 仕様書§9のイベント('scan-progress', 'thumb-progress', 'catalog-changed')を反映。
 * Tauri環境外（Electron/ブラウザ）でも安全に初期サマリを取得し、エラーにならないよう保護する。
 * また、フィルタ条件（filterState）およびタグ一覧（fetchTags）の同期も行う。
 */
export function useBackendEvents() {
  const setScanProgress = useAppStore((state) => state.setScanProgress);
  const setThumbProgress = useAppStore((state) => state.setThumbProgress);
  const setCatalogSummary = useAppStore((state) => state.setCatalogSummary);
  const setFolders = useAppStore((state) => state.setFolders);
  const selectedFolderId = useAppStore((state) => state.selectedFolderId);
  const timelineSort = useAppStore((state) => state.timelineSort);
  const filterState = useAppStore((state) => state.filterState);
  const fetchBookmarks = useAppStore((state) => state.fetchBookmarks);
  const fetchTags = useAppStore((state) => state.fetchTags);

  const filterKey = useMemo(() => serializeTimelineFilter(filterState), [filterState]);

  // 初回起動時のしおり・タグ同期
  useEffect(() => {
    fetchBookmarks();
    fetchTags();
  }, [fetchBookmarks, fetchTags]);

  // カタログサマリ & サムネイル進捗再取得処理
  const refreshSummary = useCallback(async () => {
    try {
      const latestState = useAppStore.getState();
      const activeFilter = isTimelineFilterActive(latestState.filterState)
        ? latestState.filterState
        : undefined;
      const summary = await backendApi.getTimelineSummary(
        latestState.selectedFolderId ?? undefined,
        latestState.timelineSort,
        activeFilter
      );
      setCatalogSummary(summary.total, summary.buckets, summary.version);
      const folders = await backendApi.getWatchFolders();
      setFolders(folders);
      fetchTags();

      // サムネイル進捗の取得
      const thumbInfo = await backendApi.getThumbProgress();
      setThumbProgress({ done: thumbInfo.done, failed: thumbInfo.failed, total: thumbInfo.total });
    } catch (err) {
      console.error("サマリ取得に失敗しました:", err);
    }
  }, [setCatalogSummary, setFolders, fetchTags, setThumbProgress]);

  useEffect(() => {
    refreshSummary();
  }, [selectedFolderId, timelineSort, filterKey, refreshSummary]);

  useEffect(() => {
    if (!isTauriEnvironment()) {
      // 非Tauri環境（Electron / Webブラウザ）では定期ポーリングでサマリを更新
      const interval = setInterval(refreshSummary, 3000);
      return () => clearInterval(interval);
    }

    let unlistenScan: (() => void) | undefined;
    let unlistenThumb: (() => void) | undefined;
    let unlistenCatalog: (() => void) | undefined;

    const setupListeners = async () => {
      try {
        unlistenScan = await listen<ScanProgress>("scan-progress", (event) => {
          setScanProgress(event.payload);
          if (event.payload.phase === "done") {
            refreshSummary();
          }
        });

        unlistenThumb = await listen<ThumbProgress>("thumb-progress", (event) => {
          setThumbProgress(event.payload);
          // 全件完了時にフォルダの生成数表示も同期
          if (event.payload.total > 0 && event.payload.done + event.payload.failed >= event.payload.total) {
            backendApi.getWatchFolders().then(setFolders).catch(() => {});
          }
        });

        unlistenCatalog = await listen<CatalogChangedPayload>("catalog-changed", (_event) => {
          refreshSummary();
        });
      } catch (e) {
        console.warn("イベントリスナー登録スキップ:", e);
      }
    };

    setupListeners();

    return () => {
      unlistenScan?.();
      unlistenThumb?.();
      unlistenCatalog?.();
    };
  }, [selectedFolderId]);
}
