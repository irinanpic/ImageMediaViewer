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
 * 未読込み画像があれば読み込む、また更新有無を確認し、もし更新があれば差し替え…」に準拠。
 * 変更のないレコードについては既存のオブジェクト参照（oldRec）をそのまま流用し、
 * Reactの再レンダリングやサムネイル画像の再デコード・チラつきを防止する。
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
      // id, rev, width, height, takenAt が全て同一なら同一とみなし、既存の参照を維持
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
 * アプリケーション全体で共有されるタイムライン画像ページキャッシュマネージャー
 *
 * 変更理由: VirtualTimeline, TimelineToolbar, ImageViewerModal がそれぞれ個別のキャッシュを持つと、
 * タイムラインで読み込まれた画像をツールバーやモーダルが取得できず空振りする問題を防ぐため。
 * シングルトンとしてページキャッシュを統合し、全コンポーネントへリアクティブに通知する。
 */
export class SharedPageCacheManager {
  private cache = new Map<number, ImageRecord[]>();
  private fetchingPages = new Set<number>();
  private listeners = new Set<() => void>();
  private currentFolderId: number | null = null;
  private currentSort: string | null = null;
  private isInitialized = false;

  /**
   * フォルダまたはソート順が切り替わった場合にキャッシュを無効化
   */
  public syncScope(folderId: number | null, sort: string) {
    if (!this.isInitialized || this.currentFolderId !== folderId || this.currentSort !== sort) {
      this.currentFolderId = folderId;
      this.currentSort = sort;
      this.isInitialized = true;
      this.cache.clear();
      this.fetchingPages.clear();
      this.notify();
    }
  }

  /**
   * キャッシュ更新リスナーを購読
   */
  public subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  /**
   * 購読リスナーへ更新を通知
   */
  public notify() {
    for (const listener of this.listeners) {
      listener();
    }
  }

  /**
   * ページを取得（LRU更新）
   */
  public get(pageIndex: number): ImageRecord[] | undefined {
    const records = this.cache.get(pageIndex);
    if (records) {
      // アクセスされたページを末尾へ移動
      this.cache.delete(pageIndex);
      this.cache.set(pageIndex, records);
    }
    return records;
  }

  public has(pageIndex: number): boolean {
    return this.cache.has(pageIndex);
  }

  public isFetching(pageIndex: number): boolean {
    return this.fetchingPages.has(pageIndex);
  }

  public setFetching(pageIndex: number, fetching: boolean) {
    if (fetching) {
      this.fetchingPages.add(pageIndex);
    } else {
      this.fetchingPages.delete(pageIndex);
    }
  }

  /**
   * ページを保存（LRU上限管理）
   */
  public set(pageIndex: number, records: ImageRecord[]) {
    this.cache.delete(pageIndex);

    if (this.cache.size >= MAX_CACHED_PAGES) {
      const oldestKey = this.cache.keys().next().value;
      if (oldestKey !== undefined) {
        this.cache.delete(oldestKey);
      }
    }

    this.cache.set(pageIndex, records);
    this.notify();
  }

  public getCachedPageIndices(): number[] {
    return Array.from(this.cache.keys());
  }

  /**
   * 現在キャッシュされている全ページの画像IDをページインデックス昇順で取得する
   *
   * 変更理由: Ctrl+A 全選択時に `0..totalImages` ループで `getImageByIndex` を呼ぶと、
   * 10万枚規模で10万回のMap delete/set（LRU更新）が走りUIスレッドがブロックされるのを防ぐため。
   *
   * @returns キャッシュ済み全画像のID配列
   */
  public getAllLoadedIds(): number[] {
    const sortedPageIndices = Array.from(this.cache.keys()).sort((a, b) => a - b);
    const ids: number[] = [];
    for (const pageIdx of sortedPageIndices) {
      const records = this.cache.get(pageIdx);
      if (records) {
        for (const rec of records) {
          ids.push(rec.id);
        }
      }
    }
    return ids;
  }

  public clear() {
    this.cache.clear();
    this.fetchingPages.clear();
    this.notify();
  }
}

// モジュールシングルトンキャッシュ
export const sharedCache = new SharedPageCacheManager();

/**
 * タイムライン画像のページング取得およびLRUキャッシュを管理するカスタムフック
 *
 * 変更理由: 仕様書§8.2「ページ単位（200件）で取得、重複リクエスト排除、最大50ページのLRUキャッシュ」
 * およびインクリメンタル差分リロード。コンポーネント間で同一キャッシュを共有し、
 * TimelineToolbar や ImageViewerModal でも即時に画像情報を参照できるようにする。
 *
 * @returns { getImageByIndex, getAllLoadedIds, requestIndices, revalidatePages, ensureIndex, versionTick }
 */
export function usePagedImages() {
  const selectedFolderId = useAppStore((state) => state.selectedFolderId);
  const catalogVersion = useAppStore((state) => state.catalogVersion);
  const timelineSort = useAppStore((state) => state.timelineSort);
  const timelineRefreshTick = useAppStore((state) => state.timelineRefreshTick);

  const [versionTick, setVersionTick] = useState(0);

  // 共有キャッシュ更新通知の購読
  useEffect(() => {
    return sharedCache.subscribe(() => {
      setVersionTick((v) => v + 1);
    });
  }, []);

  // フォルダまたはソート順の変更を同期
  useEffect(() => {
    sharedCache.syncScope(selectedFolderId ?? null, timelineSort);
  }, [selectedFolderId, timelineSort]);

  // 単一ページのフェッチ＆差分マージ処理
  const fetchPage = useCallback(
    async (pageIndex: number, isRevalidate: boolean = false) => {
      if (sharedCache.isFetching(pageIndex)) return;
      if (!isRevalidate && sharedCache.has(pageIndex)) return;

      sharedCache.setFetching(pageIndex, true);

      try {
        const records = await backendApi.getTimelineImages({
          offset: pageIndex * PAGE_SIZE,
          limit: PAGE_SIZE,
          folderId: selectedFolderId ?? null,
          sort: timelineSort,
        });

        const oldRecords = sharedCache.get(pageIndex);
        const { merged, changed } = mergePageRecords(oldRecords, records);

        // 差分があった場合または新規取得時のみキャッシュ更新と再描画トリガーを発行
        if (changed || !oldRecords) {
          sharedCache.set(pageIndex, merged);
        }
      } catch (err) {
        console.error(`ページ ${pageIndex} の取得に失敗しました:`, err);
      } finally {
        sharedCache.setFetching(pageIndex, false);
      }
    },
    [selectedFolderId, timelineSort]
  );

  // 手動再読込(timelineRefreshTick)またはカタログバージョン更新(catalogVersion)時:
  // 保持しているキャッシュ済みページをバックグラウンドで差分更新
  useEffect(() => {
    const cachedPageIndices = sharedCache.getCachedPageIndices();
    if (cachedPageIndices.length === 0) return;

    for (const pageIndex of cachedPageIndices) {
      fetchPage(pageIndex, true);
    }
  }, [catalogVersion, timelineRefreshTick, fetchPage]);

  /**
   * 指定したグローバルインデックスの画像レコードを取得（共有キャッシュにあれば即時返却）
   */
  const getImageByIndex = useCallback((index: number): ImageRecord | null => {
    const pageIndex = Math.floor(index / PAGE_SIZE);
    const offsetInPage = index % PAGE_SIZE;
    const page = sharedCache.get(pageIndex);
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
          if (!sharedCache.has(pageIndex) && !sharedCache.isFetching(pageIndex)) {
            neededPages.add(pageIndex);
          }
        }

        if (neededPages.size === 0) return;

        // 画面外のプリフェッチは直近の1ページのみに限定（キュー爆発防止）
        const prefetchPages = new Set<number>();
        for (const p of neededPages) {
          const next = p + 1;
          if (!sharedCache.has(next) && !sharedCache.isFetching(next) && !neededPages.has(next)) {
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
        const oldRecords = sharedCache.get(pageIndex);
        const { merged } = mergePageRecords(oldRecords, records);
        sharedCache.set(pageIndex, merged);
        return merged[offsetInPage] ?? null;
      } catch (err) {
        console.error(`インデックス ${index} の取得に失敗しました:`, err);
        return null;
      }
    },
    [getImageByIndex, selectedFolderId, timelineSort]
  );

  const getAllLoadedIds = useCallback((): number[] => {
    return sharedCache.getAllLoadedIds();
  }, []);

  return { getImageByIndex, getAllLoadedIds, requestIndices, ensureIndex, versionTick };
}
