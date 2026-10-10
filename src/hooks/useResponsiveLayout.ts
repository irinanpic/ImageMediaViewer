import { useEffect, useState } from "react";
import { isMobileEnvironment } from "../lib/thumbUrl";

/**
 * レイアウト表示モードの設定型
 *
 * - auto: プラットフォームおよび画面幅による自動判定（Android環境は物理画面サイズを考慮し常にモバイル扱い）
 * - mobile: 強制的にモバイル（コンパクト）UIを適用
 * - desktop: 強制的に大画面デスクトップUIを適用（将来のAndroid PC / Googlebook用）
 */
export type LayoutModeSetting = "auto" | "mobile" | "desktop";

export interface ResponsiveLayoutState {
  /** コンパクト（モバイル最適化）レイアウトを適用すべきかどうか */
  isCompact: boolean;
  /** モバイルネイティブ環境（Android / iOS）かどうか */
  isMobileDevice: boolean;
  /** 現在の画面幅 (px) */
  screenWidth: number;
  /** 現在の画面高さ (px) */
  screenHeight: number;
  /** 横向き（Landscape）かどうか */
  isLandscape: boolean;
  /** 手動レイアウトモード設定 */
  layoutMode: LayoutModeSetting;
  /** 手動レイアウトモード変更関数 */
  setLayoutMode: (mode: LayoutModeSetting) => void;
}

/**
 * 画面サイズおよびプラットフォーム（Android / PC）に応じたレスポンシブレイアウト判定フック
 *
 * 変更理由: Android実機では解像度ピクセル数が高くても物理画面が小さいため、
 * 現行のAndroid環境下では常にモバイル向けUI（ドロワーサイドバー、統合スリムツールバー）を適用しつつ、
 * 将来のGooglebook等のAndroid PC向けに大画面モードへの切り替え性を担保するため。
 *
 * @returns {@link ResponsiveLayoutState}
 */
export function useResponsiveLayout(): ResponsiveLayoutState {
  const [layoutMode, setLayoutMode] = useState<LayoutModeSetting>("auto");
  const [screenWidth, setScreenWidth] = useState<number>(
    typeof window !== "undefined" ? window.innerWidth : 1024
  );
  const [screenHeight, setScreenHeight] = useState<number>(
    typeof window !== "undefined" ? window.innerHeight : 768
  );

  useEffect(() => {
    if (typeof window === "undefined") return;

    const handleResize = () => {
      setScreenWidth(window.innerWidth);
      setScreenHeight(window.innerHeight);
    };

    window.addEventListener("resize", handleResize);
    window.addEventListener("orientationchange", handleResize);
    return () => {
      window.removeEventListener("resize", handleResize);
      window.removeEventListener("orientationchange", handleResize);
    };
  }, []);

  const isMobileDevice = isMobileEnvironment();
  const isLandscape = screenWidth > screenHeight;
  const isCompact = resolveIsCompact(layoutMode, isMobileDevice, screenWidth);

  return {
    isCompact,
    isMobileDevice,
    screenWidth,
    screenHeight,
    isLandscape,
    layoutMode,
    setLayoutMode,
  };
}

/**
 * コンパクト（モバイル）レイアウトを適用すべきかの判定ロジック
 *
 * @param layoutMode 手動設定モード ('auto' | 'mobile' | 'desktop')
 * @param isMobileDevice モバイル（Android/iOS）端末かどうか
 * @param screenWidth 現在の画面幅 (px)
 * @returns コンパクトレイアウトを適用する場合 true
 */
export function resolveIsCompact(
  layoutMode: LayoutModeSetting,
  isMobileDevice: boolean,
  screenWidth: number
): boolean {
  if (layoutMode === "mobile") return true;
  if (layoutMode === "desktop") return false;

  // auto モード:
  // 1. Android / iOS 環境では物理画面サイズが小さいため、画面解像度に関わらず一律モバイル版扱いとする
  // 2. PC / デスクトップ環境では画面幅 768px 未満のときにモバイル版扱いとする
  if (isMobileDevice) {
    return true;
  }
  return screenWidth < 768;
}
