import { describe, it, expect } from "vitest";
import { buildRows, calculateColumns, DEFAULT_LAYOUT_OPTIONS } from "../buildRows";
import type { DayBucket } from "../../types/generated/DayBucket";

describe("buildRows (仮想スクロール行計算)", () => {
  it("列数計算が正確に行われること", () => {
    // 160pxセル + 4pxギャップ = 164pxピッチ
    // 幅 800px: (800 + 4) / 164 = 804 / 164 = 4.9 -> 4列
    expect(calculateColumns(800, 160, 4)).toBe(4);
    // 幅 100px (セルより狭い場合) -> 最小1列
    expect(calculateColumns(100, 160, 4)).toBe(1);
  });

  it("空のバケット配列の場合は空の行配列を返すこと", () => {
    const rows = buildRows([], 4);
    expect(rows).toEqual([]);
  });

  it("日別バケットからヘッダ行とセル行が正しく分割構築されること", () => {
    const buckets: DayBucket[] = [
      { day: "2024-06-15", count: 7 },
      { day: "2024-06-14", count: 2 },
    ];
    // 4列の場合:
    // 2024-06-15:
    //   Header (count: 7)
    //   Cells (startIndex: 0, count: 4)
    //   Cells (startIndex: 4, count: 3)
    // 2024-06-14:
    //   Header (count: 2)
    //   Cells (startIndex: 7, count: 2)
    const rows = buildRows(buckets, 4);

    expect(rows.length).toBe(5);

    expect(rows[0]).toEqual({
      kind: "header",
      day: "2024-06-15",
      count: 7,
      height: DEFAULT_LAYOUT_OPTIONS.headerHeight,
    });
    expect(rows[1]).toEqual({
      kind: "cells",
      startIndex: 0,
      count: 4,
      height: 164,
      day: "2024-06-15",
    });
    expect(rows[2]).toEqual({
      kind: "cells",
      startIndex: 4,
      count: 3,
      height: 164,
      day: "2024-06-15",
    });
    expect(rows[3]).toEqual({
      kind: "header",
      day: "2024-06-14",
      count: 2,
      height: DEFAULT_LAYOUT_OPTIONS.headerHeight,
    });
    expect(rows[4]).toEqual({
      kind: "cells",
      startIndex: 7,
      count: 2,
      height: 164,
      day: "2024-06-14",
    });
  });

  it("countが0以下のバケットは除外されること", () => {
    const buckets: DayBucket[] = [
      { day: "2024-06-15", count: 0 },
      { day: "2024-06-14", count: 3 },
    ];
    const rows = buildRows(buckets, 4);
    expect(rows.length).toBe(2);
    expect(rows[0].kind).toBe("header");
    expect(rows[0].day).toBe("2024-06-14");
  });
});
