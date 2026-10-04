import { useCallback, useEffect, useRef } from "react";
import { backendApi } from "../lib/ipc";
import { useAppStore } from "../store";

/**
 * サーバーとの通信状態を定期監視し、自動再接続およびデータ同期を行うフック
 *
 * 変更理由: 長時間放置やPCスリープ等による通信途絶・サーバー停止を検知し、
 * ユーザーに状況を明示するとともに、復帰時に自動でタイムライン等の最新データを再同期する
 */
export function useConnectionMonitor() {
  const connectionStatus = useAppStore((state) => state.connectionStatus);
  const setConnectionStatus = useAppStore((state) => state.setConnectionStatus);
  const markConnected = useAppStore((state) => state.markConnected);
  const markDisconnected = useAppStore((state) => state.markDisconnected);
  const refreshTimeline = useAppStore((state) => state.refreshTimeline);

  // 連続失敗カウント
  const failureCountRef = useRef(0);
  const isCheckingRef = useRef(false);

  /**
   * サーバーへの接続確認（Ping）を1回実行
   */
  const checkConnection = useCallback(async () => {
    if (isCheckingRef.current) return;
    isCheckingRef.current = true;

    try {
      const isAlive = await backendApi.pingServer(2500);

      if (isAlive) {
        failureCountRef.current = 0;
        // 切断状態から復帰した場合はデータを自動リフレッシュ
        if (connectionStatus !== "connected") {
          markConnected();
          refreshTimeline();
        } else {
          markConnected();
        }
      } else {
        failureCountRef.current += 1;
        // 2回以上連続失敗したら切断と判定
        if (failureCountRef.current >= 2) {
          markDisconnected();
        }
      }
    } catch {
      failureCountRef.current += 1;
      if (failureCountRef.current >= 2) {
        markDisconnected();
      }
    } finally {
      isCheckingRef.current = false;
    }
  }, [connectionStatus, markConnected, markDisconnected, refreshTimeline]);

  /**
   * 手動で再接続を試行
   */
  const retryConnection = useCallback(async () => {
    setConnectionStatus("connecting");
    failureCountRef.current = 0;
    try {
      const isAlive = await backendApi.pingServer(3000);
      if (isAlive) {
        markConnected();
        refreshTimeline();
        return true;
      } else {
        markDisconnected();
        return false;
      }
    } catch {
      markDisconnected();
      return false;
    }
  }, [setConnectionStatus, markConnected, markDisconnected, refreshTimeline]);

  // 定期ポーリング監視
  useEffect(() => {
    // 初回確認
    checkConnection();

    // 接続状態に応じてポーリング間隔を調整（切断時はより高頻度で復帰を試行）
    const intervalMs = connectionStatus === "disconnected" ? 2000 : 3000;
    const timer = setInterval(() => {
      checkConnection();
    }, intervalMs);

    return () => clearInterval(timer);
  }, [checkConnection, connectionStatus]);

  // ウィンドウフォーカス・表示復帰時に即座に確認
  useEffect(() => {
    const handleFocus = () => {
      checkConnection();
    };

    window.addEventListener("focus", handleFocus);
    document.addEventListener("visibilitychange", handleFocus);

    return () => {
      window.removeEventListener("focus", handleFocus);
      document.removeEventListener("visibilitychange", handleFocus);
    };
  }, [checkConnection]);

  return {
    connectionStatus,
    retryConnection,
  };
}
