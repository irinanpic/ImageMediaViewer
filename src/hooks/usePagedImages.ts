import { useCallback, useEffect, useRef, useState } from "react";
import { backendApi } from "../lib/ipc";
import { useAppStore } from "../store";
import type { ImageRecord } from "../types/generated/ImageRecord";

const PAGE_SIZE = 200;
const MAX_CACHED_PAGES = 50;

/**
 * タイムライン画像のページング取得およびLRUキャッシュを管理するカスタムフック
 *
 * 変更理由: 仕様書§8.2「ページ単位（200件）で取得、重複リクエスト排除、最大50ページ（約1万件）のLRUキャッシュ」
 *
 * @returns { getImageByIndex, requestIndices, clearCache }
 */
export function usePagedImages() {
  const selectedFolderId = useAppStore((state) => state.selectedFolderId);
  const catalogVersion = useAppStore((state) => state.catalogVersion);
  const timelineSort = useAppStore((state) => state.timelineSort);

  // ページ番号 -> ImageRecord[] のキャッシュ
  const cacheRef = useRef<Map<number, ImageRecord[]>>(new Map());
  // 取得中のページ番号セット
  const fetchingPagesRef = useRef<Set<number>>(new Set());
  // 再レンダリングトリガー
  const [versionTick, setVersionTick] = useState(0);

  const timelineRefreshTick = useAppStore((state) => state.timelineRefreshTick);

  // バージョン変更、フォルダ切替、ソート順変更、手動再読込(timelineRefreshTick)時にキャッシュクリア
  useEffect(() => {
    cacheRef.current.clear();
    fetchingPagesRef.current.clear();
    setVersionTick((v) => v + 1);
  }, [catalogVersion, selectedFolderId, timelineSort, timelineRefreshTick]);


  /**
   * 指定したグローバルインデックスの画像レコードを取得（キャッシュにあれば即時返却）
   */
  const getImageByIndex = useCallback((index: number): ImageRecord | null => {
    const pageIndex = Math.floor(index / PAGE_SIZE);
    const offsetInPage = index % PAGE_SIZE;
    const page = cacheRef.current.get(pageIndex);
    if (page && offsetInPage < page.length) {
      return page[offsetInPage];
    }
    return null;
  }, []);

  // 取得要求の世代番号（大スクロール時の古い要求破棄に使用）
  const requestGenRef = useRef(0);
  const debounceTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  /**
   * 表示範囲で必要となるインデックス群のページを取得リクエストする
   */
  const requestIndices = useCallback(
    (indices: number[]) => {
      if (indices.length === 0) return;

      if (debounceTimerRef.current) {
        clearTimeout(debounceTimerRef.current);
      }

      debounceTimerRef.current = setTimeout(async () => {
        const currentGen = ++requestGenRef.current;

        const neededPages = new Set<number>();
        for (const idx of indices) {
          const pageIndex = Math.floor(idx / PAGE_SIZE);
          if (!cacheRef.current.has(pageIndex) && !fetchingPagesRef.current.has(pageIndex)) {
            neededPages.add(pageIndex);
          }
        }

        if (neededPages.size === 0) return;

        // 画面外のプリフェッチは直近の1ページのみに限定（キュー爆発防止）
        const prefetchPages = new Set<number>();
        for (const p of neededPages) {
          const next = p + 1;
          if (!cacheRef.current.has(next) && !fetchingPagesRef.current.has(next) && !neededPages.has(next)) {
            prefetchPages.add(next);
          }
        }

        const fetchPage = async (pageIndex: number) => {
          if (fetchingPagesRef.current.has(pageIndex) || cacheRef.current.has(pageIndex)) return;
          fetchingPagesRef.current.add(pageIndex);

          try {
            const records = await backendApi.getTimelineImages({
              offset: pageIndex * PAGE_SIZE,
              limit: PAGE_SIZE,
              folderId: selectedFolderId ?? null,
              sort: timelineSort,
            });

            // 大スクロール等で既に別の世代に進んでいた場合でもキャッシュには入れる
            const cache = cacheRef.current;
            if (cache.size >= MAX_CACHED_PAGES) {
              const oldestKey = cache.keys().next().value;
              if (oldestKey !== undefined) {
                cache.delete(oldestKey);
              }
            }
            cache.set(pageIndex, records);

            // キャッシュに追加されたら即座に再描画トリガーを発行
            // （古い世代とみなされて再描画がスキップされ、セルがスケルトンのまま固まる事態を完全防止）
            setVersionTick((v) => v + 1);
          } catch (err) {
            console.error(`ページ ${pageIndex} の取得に失敗しました:`, err);
          } finally {
            fetchingPagesRef.current.delete(pageIndex);
          }
        };

        // 1. 画面内ページを最優先で取得
        for (const pageIndex of neededPages) {
          fetchPage(pageIndex);
        }

        // 2. 直近1ページのみ軽く遅延させてプリフェッチ
        if (prefetchPages.size > 0) {
          setTimeout(() => {
            if (currentGen === requestGenRef.current) {
              for (const pageIndex of prefetchPages) {
                fetchPage(pageIndex);
              }
            }
          }, 100);
        }
      }, 25);
    },
    [selectedFolderId, timelineSort]
  );

  /**
   * 指定したインデックスの画像レコードを確実に取得する（キャッシュ未ロード時は即時API呼び出し）
   */
  const ensureIndex = useCallback(
    async (index: number): Promise<ImageRecord | null> => {
      const cached = getImageByIndex(index);
      if (cached) return cached;

      const pageIndex = Math.floor(index / PAGE_SIZE);
      const offsetInPage = index % PAGE_SIZE;

      try {
        const records = await backendApi.getTimelineImages({
          offset: pageIndex * PAGE_SIZE,
          limit: PAGE_SIZE,
          folderId: selectedFolderId ?? null,
          sort: timelineSort,
        });
        cacheRef.current.set(pageIndex, records);
        setVersionTick((v) => v + 1);
        return records[offsetInPage] ?? null;
      } catch (err) {
        console.error(`インデックス ${index} の取得に失敗しました:`, err);
        return null;
      }
    },
    [getImageByIndex, selectedFolderId, timelineSort]
  );

  return { getImageByIndex, requestIndices, ensureIndex, versionTick };
}
