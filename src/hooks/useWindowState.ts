import { useEffect, useRef } from "react";
import { backendApi } from "../lib/ipc";
import type { WindowState } from "../types/generated/WindowState";

/**
 * 独立ウィンドウのサイズ・位置・最大化状態を監視し、バックエンドへ永続化するフック
 *
 * 変更理由: アプリを閉じて開き直した際に、前回の画面サイズ・位置・最大化状態を忠実に復元し、
 * ネイティブアプリと同様の快適な使い心地を提供するため。
 */
export function useWindowState() {
  const saveTimerRef = useRef<number | null>(null);

  useEffect(() => {
    // 状態を収集してバックエンドに保存する関数
    const persistWindowState = () => {
      // 最小化中や非表示中は保存しない
      if (typeof window === "undefined" || window.outerWidth <= 100 || window.outerHeight <= 100) {
        return;
      }

      // 最大化判定: 利用可能画面領域とほぼ同等かチェック
      const isMaximized =
        Math.abs(window.outerWidth - window.screen.availWidth) <= 16 &&
        Math.abs(window.outerHeight - window.screen.availHeight) <= 16;

      const state: WindowState = {
        width: Math.round(window.outerWidth),
        height: Math.round(window.outerHeight),
        x: Math.round(window.screenX),
        y: Math.round(window.screenY),
        isMaximized,
      };

      backendApi.saveWindowState(state).catch((err) => {
        console.warn("ウィンドウ状態の保存に失敗:", err);
      });
    };

    // リサイズ時のデバウンス保存ハンドラ
    const handleResize = () => {
      if (saveTimerRef.current !== null) {
        window.clearTimeout(saveTimerRef.current);
      }
      saveTimerRef.current = window.setTimeout(persistWindowState, 350);
    };

    // 初期マウント時の保存（初期サイズを記録）
    const initialTimer = window.setTimeout(persistWindowState, 1000);

    // バックエンド終了同期用: 2.5秒ごとの定期ハートビート送信
    const getBaseUrl = () => {
      if (typeof window !== "undefined" && window.location && window.location.origin && window.location.origin.startsWith("http")) {
        return window.location.origin;
      }
      return "http://127.0.0.1:14201";
    };

    const heartbeatInterval = window.setInterval(() => {
      fetch(`${getBaseUrl()}/api/heartbeat`, { method: "POST" }).catch(() => {});
    }, 2500);

    // ウィンドウ終了時の明示的シャットダウン送信
    const handleUnload = () => {
      persistWindowState();
      try {
        const shutdownUrl = `${getBaseUrl()}/api/shutdown`;
        if (navigator.sendBeacon) {
          navigator.sendBeacon(shutdownUrl, "");
        } else {
          fetch(shutdownUrl, { method: "POST", keepalive: true }).catch(() => {});
        }
      } catch {
        // シャットダウン送信エラーは無視
      }
    };

    window.addEventListener("resize", handleResize);
    window.addEventListener("beforeunload", handleUnload);
    window.addEventListener("pagehide", handleUnload);

    return () => {
      window.clearTimeout(initialTimer);
      window.clearInterval(heartbeatInterval);
      if (saveTimerRef.current !== null) {
        window.clearTimeout(saveTimerRef.current);
      }
      window.removeEventListener("resize", handleResize);
      window.removeEventListener("beforeunload", handleUnload);
      window.removeEventListener("pagehide", handleUnload);
    };
  }, []);
}
