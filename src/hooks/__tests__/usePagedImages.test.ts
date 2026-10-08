import { describe, expect, it } from "vitest";
import { mergePageRecords } from "../usePagedImages";
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

  it("内容に変化がない場合は既存のオブジェクト参照を100%維持し changed=false とする", () => {
    const oldList = [sample1, sample2];
    // 同じ値を持つ別のオブジェクトインスタンス
    const newList: ImageRecord[] = [
      { ...sample1 },
      { ...sample2 },
    ];

    const result = mergePageRecords(oldList, newList);
    expect(result.changed).toBe(false);
    expect(result.merged.length).toBe(2);
    // オブジェクト参照が oldList のものと同一であること（Reactの再レンダリング防止）
    expect(result.merged[0]).toBe(sample1);
    expect(result.merged[1]).toBe(sample2);
  });

  it("画像ファイルが更新（rev変更）された場合はその画像のみ新レコードに差し替える", () => {
    const oldList = [sample1, sample2];
    const updatedSample2: ImageRecord = {
      ...sample2,
      rev: 2002, // mtime更新
    };
    const newList = [sample1, updatedSample2];

    const result = mergePageRecords(oldList, newList);
    expect(result.changed).toBe(true);
    expect(result.merged.length).toBe(2);
    // sample1 は変更がないため既存参照を維持
    expect(result.merged[0]).toBe(sample1);
    // sample2 は更新されたため新レコードに差し替え
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
    // インデックス1は旧sample2からsample3に置き換わる
    expect(result.merged[1]).toBe(sample3);
  });
});
