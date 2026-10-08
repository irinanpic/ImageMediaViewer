import { useCallback, useEffect, useRef, useState } from "react";
import { backendApi } from "../lib/ipc";
import { useAppStore } from "../store";
import type { ImageRecord } from "../types/generated/ImageRecord";

const PAGE_SIZE = 200;
const MAX_CACHED_PAGES = 50;

/**
 * 既存のページレコードと新しく取得したページレコードを差分マージする
 *
 * 変更理由: ユーザー要求「再読込時、既に読み込まれている画像については基本的にそのまま利用しつつ
 * 未読込み画像があれば読み込む、また更新有無を確認し、もし更新があれば差し替え…」に完全準拠。
 * 変更のないレコードについては既存のオブジェクト参照（oldRec）をそのまま流用し、
 * Reactの再レンダリングやサムネイル画像の再デコード・チラつきを完全に防止する。
 *
 * @param oldRecords 既存のキャッシュ済みレコード配列（未キャッシュ時は undefined）
 * @param newRecords 新たにバックエンドから取得した最新レコード配列
 * @returns { merged: ImageRecord[]; changed: boolean } マージ後の配列と変更有無フラグ
 */
export function mergePageRecords(
  oldRecords: ImageRecord[] | undefined,
  newRecords: ImageRecord[]
): { merged: ImageRecord[]; changed: boolean } {
  if (!oldRecords) {
    return { merged: newRecords, changed: true };
  }

  let changed = oldRecords.length !== newRecords.length;
  const merged: ImageRecord[] = [];
  const maxLen = Math.max(oldRecords.length, newRecords.length);

  for (let i = 0; i < maxLen; i++) {
    const oldRec = oldRecords[i];
    const newRec = newRecords[i];

    if (oldRec && newRec) {
      // id, rev, width, height, takenAt が全て同一なら完全一致とみなし、既存の参照を維持
      if (
        oldRec.id === newRec.id &&
        oldRec.rev === newRec.rev &&
        oldRec.takenAt === newRec.takenAt &&
        oldRec.width === newRec.width &&
        oldRec.height === newRec.height
      ) {
        merged.push(oldRec);
      } else {
        // 更新あり（revや寸法、撮影日時などが変更された）
        merged.push(newRec);
        changed = true;
      }
    } else if (newRec) {
      // 新規画像追加
      merged.push(newRec);
      changed = true;
    } else {
      // 画像削除
      changed = true;
    }
  }

  return { merged, changed };
}

/**
 * タイムライン画像のページング取得およびLRUキャッシュを管理するカスタムフック
 *
 * 変更理由: 仕様書§8.2「ページ単位（200件）で取得、重複リクエスト排除、最大50ページのLRUキャッシュ」
 * およびインクリメンタル差分リロード（既存表示の維持、更新画像のインプレース置換、未読込画像の追加）。
 *
 * @returns { getImageByIndex, requestIndices, revalidatePages, ensureIndex, versionTick }
 */
export function usePagedImages() {
  const selectedFolderId = useAppStore((state) => state.selectedFolderId);
  const catalogVersion = useAppStore((state) => state.catalogVersion);
  const timelineSort = useAppStore((state) => state.timelineSort);
  const timelineRefreshTick = useAppStore((state) => state.timelineRefreshTick);

  // ページ番号 -> ImageRecord[] のキャッシュ
  const cacheRef = useRef<Map<number, ImageRecord[]>>(new Map());
  // 取得中のページ番号セット
  const fetchingPagesRef = useRef<Set<number>>(new Set());
  // 再レンダリングトリガー
  const [versionTick, setVersionTick] = useState(0);

  // フォルダ切替・ソート順変更時のみ、対象画像が根本から変わるためキャッシュを全消去
  useEffect(() => {
    cacheRef.current.clear();
    fetchingPagesRef.current.clear();
    setVersionTick((v) => v + 1);
  }, [selectedFolderId, timelineSort]);

  // 単一ページのフェッチ＆差分マージ処理
  const fetchPage = useCallback(
    async (pageIndex: number, isRevalidate: boolean = false) => {
      if (fetchingPagesRef.current.has(pageIndex)) return;
      if (!isRevalidate && cacheRef.current.has(pageIndex)) return;

      fetchingPagesRef.current.add(pageIndex);

      try {
        const records = await backendApi.getTimelineImages({
          offset: pageIndex * PAGE_SIZE,
          limit: PAGE_SIZE,
          folderId: selectedFolderId ?? null,
          sort: timelineSort,
        });

        const cache = cacheRef.current;
        const oldRecords = cache.get(pageIndex);
        const { merged, changed } = mergePageRecords(oldRecords, records);

        // キャッシュサイズ上限管理
        if (!oldRecords && cache.size >= MAX_CACHED_PAGES) {
          const oldestKey = cache.keys().next().value;
          if (oldestKey !== undefined) {
            cache.delete(oldestKey);
          }
        }

        // 差分があった場合（または新規取得時）のみキャッシュ更新と再描画トリガーを発行
        if (changed || !oldRecords) {
          cache.set(pageIndex, merged);
          setVersionTick((v) => v + 1);
        }
      } catch (err) {
        console.error(`ページ ${pageIndex} の取得に失敗しました:`, err);
      } finally {
        fetchingPagesRef.current.delete(pageIndex);
      }
    },
    [selectedFolderId, timelineSort]
  );

  // 手動再読込(timelineRefreshTick)またはカタログバージョン更新(catalogVersion)時:
  // 既存キャッシュを消去せず、現在保持しているキャッシュ済みページをバックグラウンドで差分更新
  useEffect(() => {
    const cachedPageIndices = Array.from(cacheRef.current.keys());
    if (cachedPageIndices.length === 0) return;

    for (const pageIndex of cachedPageIndices) {
      fetchPage(pageIndex, true);
    }
  }, [catalogVersion, timelineRefreshTick, fetchPage]);

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

        // 1. 画面内ページを最優先で取得
        for (const pageIndex of neededPages) {
          fetchPage(pageIndex, false);
        }

        // 2. 直近1ページのみ軽く遅延させてプリフェッチ
        if (prefetchPages.size > 0) {
          setTimeout(() => {
            if (currentGen === requestGenRef.current) {
              for (const pageIndex of prefetchPages) {
                fetchPage(pageIndex, false);
              }
            }
          }, 100);
        }
      }, 25);
    },
    [fetchPage]
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
        const oldRecords = cacheRef.current.get(pageIndex);
        const { merged } = mergePageRecords(oldRecords, records);
        cacheRef.current.set(pageIndex, merged);
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
