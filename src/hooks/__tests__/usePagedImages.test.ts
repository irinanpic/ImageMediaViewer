import { describe, expect, it, vi } from "vitest";
import { mergePageRecords, SharedPageCacheManager } from "../usePagedImages";
import type { ImageRecord } from "../../types/generated/ImageRecord";

describe("usePagedImages - mergePageRecords (インクリメンタル差分マージ)", () => {
  const sample1: ImageRecord = {
    id: 101,
    takenAt: 1600000000,
    width: 1920,
    height: 1080,
    rev: 1001,
  };

  const sample2: ImageRecord = {
    id: 102,
    takenAt: 1600000100,
    width: 800,
    height: 600,
    rev: 1002,
  };

  const sample3: ImageRecord = {
    id: 103,
    takenAt: 1600000200,
    width: 1280,
    height: 720,
    rev: 1003,
  };

  it("初回ロード（oldRecords が undefined）時はそのまま newRecords を返し changed=true とする", () => {
    const result = mergePageRecords(undefined, [sample1, sample2]);
    expect(result.changed).toBe(true);
    expect(result.merged).toEqual([sample1, sample2]);
  });

  it("内容に変化がない場合は既存のオブジェクト参照を維持し changed=false とする", () => {
    const oldList = [sample1, sample2];
    const newList: ImageRecord[] = [
      { ...sample1 },
      { ...sample2 },
    ];

    const result = mergePageRecords(oldList, newList);
    expect(result.changed).toBe(false);
    expect(result.merged.length).toBe(2);
    expect(result.merged[0]).toBe(sample1);
    expect(result.merged[1]).toBe(sample2);
  });

  it("画像ファイルが更新（rev変更）された場合はその画像のみ新レコードに差し替える", () => {
    const oldList = [sample1, sample2];
    const updatedSample2: ImageRecord = {
      ...sample2,
      rev: 2002,
    };
    const newList = [sample1, updatedSample2];

    const result = mergePageRecords(oldList, newList);
    expect(result.changed).toBe(true);
    expect(result.merged.length).toBe(2);
    expect(result.merged[0]).toBe(sample1);
    expect(result.merged[1]).toBe(updatedSample2);
    expect(result.merged[1].rev).toBe(2002);
  });

  it("寸法（width/height）が変更された場合も新レコードに差し替える", () => {
    const oldList = [sample1];
    const resizedSample1: ImageRecord = {
      ...sample1,
      width: 2560,
      height: 1440,
    };

    const result = mergePageRecords(oldList, [resizedSample1]);
    expect(result.changed).toBe(true);
    expect(result.merged[0]).toBe(resizedSample1);
  });

  it("新規画像が追加された場合は末尾に追加して changed=true とする", () => {
    const oldList = [sample1, sample2];
    const newList = [sample1, sample2, sample3];

    const result = mergePageRecords(oldList, newList);
    expect(result.changed).toBe(true);
    expect(result.merged.length).toBe(3);
    expect(result.merged[0]).toBe(sample1);
    expect(result.merged[1]).toBe(sample2);
    expect(result.merged[2]).toBe(sample3);
  });

  it("画像が削除された場合は除外して changed=true とする", () => {
    const oldList = [sample1, sample2, sample3];
    const newList = [sample1, sample3];

    const result = mergePageRecords(oldList, newList);
    expect(result.changed).toBe(true);
    expect(result.merged.length).toBe(2);
    expect(result.merged[0]).toBe(sample1);
    expect(result.merged[1]).toBe(sample3);
  });
});

describe("SharedPageCacheManager (共有LRUキャッシュマネージャー)", () => {
  it("ページの保存と取得、存在確認が正しく機能する", () => {
    const manager = new SharedPageCacheManager();
    manager.syncScope(null, "folder");

    const page0: ImageRecord[] = [
      { id: 1, takenAt: 100, width: 100, height: 100, rev: 1 },
    ];
    manager.set(0, page0);

    expect(manager.has(0)).toBe(true);
    expect(manager.has(1)).toBe(false);
    expect(manager.get(0)).toEqual(page0);
  });

  it("LRU上限（50ページ）を超えた場合、最も過去に参照されたページが破棄される", () => {
    const manager = new SharedPageCacheManager();
    manager.syncScope(null, "folder");

    // 0 から 49 ページを登録
    for (let i = 0; i < 50; i++) {
      manager.set(i, [{ id: i, takenAt: i, width: 100, height: 100, rev: 1 }]);
    }
    expect(manager.getCachedPageIndices().length).toBe(50);
    expect(manager.has(0)).toBe(true);

    // ページ 0 を参照（LRUにより最新へ更新）
    manager.get(0);

    // 新規に 50 ページ目を登録
    manager.set(50, [{ id: 50, takenAt: 50, width: 100, height: 100, rev: 1 }]);

    expect(manager.getCachedPageIndices().length).toBe(50);
    // 0 は参照されたため生存し、1 が最古として破棄される
    expect(manager.has(0)).toBe(true);
    expect(manager.has(1)).toBe(false);
    expect(manager.has(50)).toBe(true);
  });

  it("キャッシュ変更時に購読リスナーへ通知される", () => {
    const manager = new SharedPageCacheManager();
    const listener = vi.fn();
    const unsubscribe = manager.subscribe(listener);

    manager.set(0, []);
    expect(listener).toHaveBeenCalledTimes(1);

    manager.clear();
    expect(listener).toHaveBeenCalledTimes(2);

    unsubscribe();
    manager.set(1, []);
    expect(listener).toHaveBeenCalledTimes(2);
  });

  it("syncScope でスコープ（フォルダ・ソート）が変化した時のみキャッシュが無効化される", () => {
    const manager = new SharedPageCacheManager();
    manager.syncScope(1, "folder");
    manager.set(0, []);
    expect(manager.has(0)).toBe(true);

    // 同一スコープならキャッシュ維持
    manager.syncScope(1, "folder");
    expect(manager.has(0)).toBe(true);

    // ソート順変更でクリア
    manager.syncScope(1, "takenAtDesc");
    expect(manager.has(0)).toBe(false);

    manager.set(0, []);
    // フォルダ変更でクリア
    manager.syncScope(2, "takenAtDesc");
    expect(manager.has(0)).toBe(false);
  });

  it("getAllLoadedIds がページインデックス昇順で全キャッシュ済みIDを返し、LRU順序を崩さないこと", () => {
    const manager = new SharedPageCacheManager();
    manager.syncScope(null, "folder");

    // ページ2を先に、ページ0を後にセット
    manager.set(2, [
      { id: 201, takenAt: 100, width: 100, height: 100, rev: 1 },
      { id: 202, takenAt: 100, width: 100, height: 100, rev: 1 },
    ]);
    manager.set(0, [
      { id: 1, takenAt: 100, width: 100, height: 100, rev: 1 },
      { id: 2, takenAt: 100, width: 100, height: 100, rev: 1 },
    ]);

    const ids = manager.getAllLoadedIds();
    // ページ昇順（ページ0 -> ページ2）でIDが返ること
    expect(ids).toEqual([1, 2, 201, 202]);
    // Map の内部キー順（LRU順：2 -> 0）が崩れていないこと
    expect(manager.getCachedPageIndices()).toEqual([2, 0]);
  });
});
