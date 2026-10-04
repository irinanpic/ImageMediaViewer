import { useEffect, useRef, useState } from "react";
import { backendApi } from "../lib/ipc";

const SPEED_THRESHOLD = 800; // 800 px/秒（中〜高速スクロール時のHTTPリクエスト乱発を確実に抑止）
const STOP_DEBOUNCE_MS = 70;  // 停止後 70ms で素早く現画面の読み込みを開始

/**
 * スクロールコンテナの移動速度を監視し、高速スクロール中かどうかを判定するフック
 *
 * 変更理由: ユーザー要求「急激に何度も動かすと読込み遅延が発生する。溜まってしまった場合は一度クリアして現在位置から再開する」。
 * 閾値を800px/sに調整して不要なリクエスト乱発を防ぎ、急激な大スクロール検知時にはバックエンドキューを即時クリアする。
 *
 * @param containerRef スクロールコンテナ要素の参照
 * @returns 高速スクロール中であればtrue（画像の新規読込抑制・スケルトン維持）
 */
export function useScrollVelocity(containerRef: React.RefObject<HTMLElement | null>): boolean {
  const [isScrollingFast, setIsScrollingFast] = useState(false);
  const lastScrollTop = useRef(0);
  const lastTime = useRef(performance.now());
  const timerRef = useRef<number | null>(null);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;

    const handleScroll = () => {
      const now = performance.now();
      const currentScrollTop = el.scrollTop;
      const deltaTime = (now - lastTime.current) / 1000; // 秒

      if (deltaTime > 0.008) {
        const deltaPos = Math.abs(currentScrollTop - lastScrollTop.current);
        const velocity = deltaPos / deltaTime; // px/s

        if (velocity > SPEED_THRESHOLD) {
          setIsScrollingFast(true);
          // 急激な移動時はバックエンドに滞留している古い生成キューを即座にパージ
          backendApi.clearQueue().catch(() => {});
        }

        lastScrollTop.current = currentScrollTop;
        lastTime.current = now;
      }

      if (timerRef.current) {
        window.clearTimeout(timerRef.current);
      }

      timerRef.current = window.setTimeout(() => {
        setIsScrollingFast(false);
      }, STOP_DEBOUNCE_MS);
    };

    el.addEventListener("scroll", handleScroll, { passive: true });
    return () => {
      el.removeEventListener("scroll", handleScroll);
      if (timerRef.current) {
        window.clearTimeout(timerRef.current);
      }
    };
  }, [containerRef]);

  return isScrollingFast;
}
