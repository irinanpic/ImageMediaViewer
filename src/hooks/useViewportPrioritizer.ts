import { useEffect, useRef } from "react";
import type { VirtualItem } from "@tanstack/react-virtual";
import type { TimelineRow } from "../lib/buildRows";
import type { ImageRecord } from "../types/generated/ImageRecord";
import { backendApi } from "../lib/ipc";

interface UseViewportPrioritizerProps {
  virtualItems: VirtualItem[];
  rows: TimelineRow[];
  getImageByIndex: (index: number) => ImageRecord | null;
  overscanRows?: number;
  dataVersion?: number;
  refreshTick?: number;
}

/**
 * タイムライン表示領域および周辺領域の画像IDをバックエンドの世代管理キューへ通知するカスタムフック
 *
 * 変更理由: Picasa並みの体感速度を実現するため。
 * 画面内に現在表示されているセル（High）およびその周辺（Mid）を最優先で生成させ、
 * スクロール通過した過去の画面外画像への無駄なリソース消費を防止する。
 * また、大スクロール直後のページ読み込み完了時（dataVersion更新時）や
 * ユーザーの手動再読込時（refreshTick更新時）にも即座にバックエンドキューを仕切り直す。
 *
 * @param props.virtualItems 仮想スクロールで管理されている現在のアクティブ行一覧
 * @param props.rows タイムラインの行定義一覧
 * @param props.getImageByIndex インデックスから画像レコードを取得する関数
 * @param props.overscanRows 先読み行数の目安
 * @param props.dataVersion ページデータ取得完了時のバージョン番号
 * @param props.refreshTick 手動リフレッシュ実行時のバージョン番号
 */
export function useViewportPrioritizer({
  virtualItems,
  rows,
  getImageByIndex,
  overscanRows = 8,
  dataVersion = 0,
  refreshTick = 0,
}: UseViewportPrioritizerProps) {
  const lastReportedKeyRef = useRef<string>("");
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const lastRefreshTickRef = useRef<number>(refreshTick);

  useEffect(() => {
    if (virtualItems.length === 0) return;

    const isManualRefresh = refreshTick !== lastRefreshTickRef.current;
    lastRefreshTickRef.current = refreshTick;

    if (timerRef.current) {
      clearTimeout(timerRef.current);
    }

    const reportViewport = () => {
      const totalVirtual = virtualItems.length;
      // overscan内の行と画面内行を識別
      const visibleStartIdx = Math.min(Math.floor(overscanRows / 2), Math.floor(totalVirtual / 4));
      const visibleEndIdx = Math.max(visibleStartIdx, totalVirtual - visibleStartIdx);

      const visibleIds: number[] = [];
      const nearbyIds: number[] = [];

      for (let i = 0; i < totalVirtual; i++) {
        const item = virtualItems[i];
        const row = rows[item.index];
        if (!row || row.kind !== "cells") continue;

        const isStrictlyVisible = i >= visibleStartIdx && i < visibleEndIdx;

        for (let c = 0; c < row.count; c++) {
          const globalIdx = row.startIndex + c;
          const rec = getImageByIndex(globalIdx);
          if (rec) {
            if (isStrictlyVisible) {
              visibleIds.push(rec.id);
            } else {
              nearbyIds.push(rec.id);
            }
          }
        }
      }

      // レコードがまだ1件も取得できていない場合はキーを更新せず次回再試行を許可
      if (visibleIds.length === 0 && nearbyIds.length === 0) {
        return;
      }

      // 変化がなければ送信を抑制（手動リフレッシュ時は強制送信）
      const key = `${visibleIds.slice(0, 10).join(",")}_${visibleIds.length}_${nearbyIds.length}`;
      if (!isManualRefresh && key === lastReportedKeyRef.current) return;
      lastReportedKeyRef.current = key;

      backendApi.setViewport(visibleIds, nearbyIds).catch((err) => {
        console.debug("ビューポート更新の通知をスキップしました:", err);
      });
    };

    // 手動リフレッシュ時は遅延なしで直ちに送信、通常スクロール時は50msデバウンス
    if (isManualRefresh) {
      reportViewport();
    } else {
      timerRef.current = setTimeout(reportViewport, 50);
    }

    return () => {
      if (timerRef.current) {
        clearTimeout(timerRef.current);
      }
    };
  }, [virtualItems, rows, getImageByIndex, overscanRows, dataVersion, refreshTick]);
}
