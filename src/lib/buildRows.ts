import type { DayBucket } from "../types/generated/DayBucket";

/**
 * 仮想スクロール描画用の行データ型定義
 *
 * 変更理由: 仕様書§8.1「全レコードを取得しなくてもスクロール全長が確定するよう、日別件数からレイアウトを計算する」
 */
export type TimelineRow =
  | {
      kind: "header";
      day: string;
      count: number;
      height: number;
    }
  | {
      kind: "cells";
      startIndex: number; // タイムライン全体での通し開始インデックス (0-based)
      count: number;      // この行に含まれる画像セルの数 (1 〜 columns)
      height: number;     // 行の高さ (セルサイズ + ギャップ)
      day: string;        // 所属する日付
    };

export interface LayoutOptions {
  cellSize: number;       // セルの幅・高さ (px)
  gap: number;            // セル間および行間のマージン (px)
  headerHeight: number;   // 日付ヘッダ行の高さ (px)
}

export const DEFAULT_LAYOUT_OPTIONS: LayoutOptions = {
  cellSize: 160,
  gap: 4,
  headerHeight: 48,
};

/**
 * ビューポート幅から表示列数を計算する
 *
 * @param containerWidth コンテナ幅 (px)
 * @param cellSize セルサイズ (px)
 * @param gap ギャップ (px)
 * @returns 1以上の列数
 */
export function calculateColumns(containerWidth: number, cellSize: number, gap: number): number {
  if (containerWidth <= 0) return 1;
  const cols = Math.floor((containerWidth + gap) / (cellSize + gap));
  return Math.max(1, cols);
}

/**
 * 日別件数バケットから仮想スクロール行の一覧を計算する純関数
 *
 * 変更理由: 仕様書§8.1「buildRows は純関数としてユニットテストする」
 *
 * @param buckets 日別件数バケット一覧（日付降順）
 * @param columns 1行あたりのセル数
 * @param options レイアウトオプション
 * @returns 仮想スクロール行の配列
 */
export function buildRows(
  buckets: DayBucket[],
  columns: number,
  options: LayoutOptions = DEFAULT_LAYOUT_OPTIONS
): TimelineRow[] {
  const rows: TimelineRow[] = [];
  const safeCols = Math.max(1, columns);
  const rowHeight = options.cellSize + options.gap;

  let globalIndex = 0;

  for (const bucket of buckets) {
    if (bucket.count <= 0) continue;

    // 1. 日付ヘッダ行を追加
    rows.push({
      kind: "header",
      day: bucket.day,
      count: bucket.count,
      height: options.headerHeight,
    });

    // 2. 当該日の画像セル行を分割追加
    let remaining = bucket.count;
    while (remaining > 0) {
      const countInRow = Math.min(remaining, safeCols);
      rows.push({
        kind: "cells",
        startIndex: globalIndex,
        count: countInRow,
        height: rowHeight,
        day: bucket.day,
      });

      globalIndex += countInRow;
      remaining -= countInRow;
    }
  }

  return rows;
}
