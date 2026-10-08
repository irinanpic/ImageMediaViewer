import React, { useState } from "react";
import {
  AlertCircle,
  CheckCircle2,
  Copy,
  Loader2,
  Play,
  RefreshCw,
  Wifi,
  WifiOff,
} from "lucide-react";
import { useConnectionMonitor } from "../../hooks/useConnectionMonitor";
import { useTranslation } from "../../locales";
import { useAppStore } from "../../store";

/**
 * クライアントの実行環境OSを判定
 */
function getPlatform(): "windows" | "mac" | "linux" {
  if (typeof navigator === "undefined") return "windows";
  const ua = navigator.userAgent.toLowerCase();
  const platform = (navigator as unknown as { userAgentData?: { platform?: string } }).userAgentData?.platform?.toLowerCase() || navigator.platform?.toLowerCase() || "";
  if (ua.includes("win") || platform.includes("win")) return "windows";
  if (ua.includes("mac") || platform.includes("mac")) return "mac";
  if (ua.includes("linux") || platform.includes("linux")) return "linux";
  return "windows";
}

/**
 * 画面上部右側に常時配置される通信状態インジケータおよび再接続コントローラー
 *
 * 変更理由: 仕様要求「何らかの理由で通信が途絶・サーバが停止した際にクライアント側で状況を把握し、
 * 再接続（サーバ起動・接続確立）を行えるように上部右側に配置」
 * - Windows固有のrun.bat依存を解消し、OSに応じたサーバー単体起動コマンド（run-server.bat / run-server.sh）に対応
 * - クライアントウィンドウが多重起動しないようサーバーのみを起動
 */
export const ConnectionStatusIndicator: React.FC = () => {
  const { t } = useTranslation();
  const { connectionStatus, retryConnection } = useConnectionMonitor();
  const lastConnectedAt = useAppStore((state) => state.lastConnectedAt);
  const [isOpen, setIsOpen] = useState(false);
  const [copied, setCopied] = useState(false);
  const [isRetrying, setIsRetrying] = useState(false);
  const [isLaunching, setIsLaunching] = useState(false);

  const platform = getPlatform();
  const serverCommand = platform === "windows" ? ".\\run-server.bat" : "./run-server.sh";
  const scriptName = platform === "windows" ? "run-server.bat" : "run-server.sh";

  const handleManualRetry = async () => {
    setIsRetrying(true);
    await retryConnection();
    setIsRetrying(false);
  };

  /**
   * カスタムURIスキーム経由でサーバー単体起動を試行（クライアント多重起動防止）
   * 変更理由: Chromium系ブラウザではiframeからのカスタムURIスキーム呼び出しが
   * セキュリティポリシーによりサイレントにブロックされるため、
   * ユーザー操作コンテキストでのトップレベルナビゲーション（window.location.href）を採用。
   * 外部プロトコルのため現在のWebページはアンロードされず、そのまま存続して再接続を監視できる。
   */
  const handleLaunchServer = () => {
    setIsLaunching(true);

    try {
      window.location.href = "imagemediaviewer://launch";
    } catch {
      window.open("imagemediaviewer://launch", "_self");
    }

    // 起動後に段階的に再接続を試行
    setTimeout(() => { handleManualRetry(); }, 1200);
    setTimeout(() => { handleManualRetry(); }, 2500);
    setTimeout(() => { handleManualRetry(); }, 4000);
    setTimeout(() => { handleManualRetry(); }, 6000);
    setTimeout(() => {
      handleManualRetry();
      setIsLaunching(false);
    }, 8000);
  };

  const handleCopyCommand = () => {
    navigator.clipboard.writeText(serverCommand);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const isConnected = connectionStatus === "connected";
  const isDisconnected = connectionStatus === "disconnected";
  const isConnecting = connectionStatus === "connecting" || isRetrying;

  const lastSeenStr = lastConnectedAt
    ? new Date(lastConnectedAt).toLocaleTimeString()
    : t("connection.lastSeenUnconnected");

  const statusTooltip = isConnected
    ? t("connection.statusTooltipConnected")
    : isConnecting
    ? t("connection.statusTooltipConnecting")
    : t("connection.statusTooltipDisconnected");

  return (
    <div className="relative">
      {/* 上部バー表示ボタン / バッジ */}
      <button
        onClick={() => setIsOpen(!isOpen)}
        className={`flex items-center gap-1.5 px-2 py-1 rounded text-xs transition border cursor-pointer select-none ${
          isConnected
            ? "bg-surfaceLight/60 hover:bg-surfaceLight text-emerald-400 border-border hover:border-emerald-500/50"
            : isConnecting
            ? "bg-amber-500/10 text-amber-300 border-amber-500/40 animate-pulse"
            : "bg-rose-500/20 text-rose-300 border-rose-500/60 shadow-xs hover:bg-rose-500/30"
        }`}
        title={statusTooltip}
      >
        {isConnected ? (
          <>
            <span className="w-2 h-2 rounded-full bg-emerald-400 inline-block animate-pulse" />
            <Wifi className="w-3.5 h-3.5 text-emerald-400" />
            <span className="text-[11px] font-medium text-textSecondary hover:text-textPrimary">{t("connection.connectedBadge")}</span>
          </>
        ) : isConnecting ? (
          <>
            <Loader2 className="w-3.5 h-3.5 animate-spin text-amber-300" />
            <span className="text-[11px] font-medium">{t("connection.checkingBadge")}</span>
          </>
        ) : (
          <>
            <WifiOff className="w-3.5 h-3.5 text-rose-400 animate-bounce" />
            <span className="text-[11px] font-bold text-rose-300">{t("connection.disconnectedBadge")}</span>
            <span className="bg-rose-500 text-white rounded px-1.5 py-0.2 text-[10px] ml-0.5">{t("connection.reconnectBtn")}</span>
          </>
        )}
      </button>

      {/* 詳細ポップオーバー */}
      {isOpen && (
        <>
          <div
            className="fixed inset-0 z-40"
            onClick={() => setIsOpen(false)}
          />
          <div className="absolute right-0 top-full mt-1.5 w-76 bg-surface border border-border rounded-lg shadow-2xl p-3 z-50 text-textPrimary text-xs space-y-3 backdrop-blur-md animate-in fade-in zoom-in-95 duration-100">
            <div className="flex items-center justify-between border-b border-border pb-2">
              <span className="font-semibold text-textPrimary flex items-center gap-1.5">
                {isConnected ? (
                  <CheckCircle2 className="w-4 h-4 text-emerald-400" />
                ) : (
                  <AlertCircle className="w-4 h-4 text-rose-400" />
                )}
                {t("connection.popoverTitle")}
              </span>
              <span
                className={`text-[10px] font-medium px-1.5 py-0.5 rounded ${
                  isConnected
                    ? "bg-emerald-500/20 text-emerald-300"
                    : "bg-rose-500/20 text-rose-300"
                }`}
              >
                {isConnected ? "ONLINE" : "OFFLINE"}
              </span>
            </div>

            <div className="space-y-1.5 text-textSecondary text-[11px]">
              <div className="flex justify-between">
                <span>{t("connection.serverUrl")}</span>
                <span className="font-mono text-textPrimary">http://127.0.0.1:14201</span>
              </div>
              <div className="flex justify-between">
                <span>{t("connection.lastSeen")}</span>
                <span className="text-textPrimary">{lastSeenStr}</span>
              </div>
              <div className="flex justify-between">
                <span>{t("connection.daemonStatus")}</span>
                <span className="text-textPrimary">
                  {isConnected ? t("connection.daemonRunning") : t("connection.daemonStopped")}
                </span>
              </div>
            </div>

            {/* 切断時のガイダンスとアクション */}
            {isDisconnected && (
              <div className="bg-rose-950/30 border border-rose-500/30 rounded p-2 text-rose-200 text-[11px] space-y-1.5">
                <p className="font-medium text-rose-300">{t("connection.disconnectedWarn")}</p>
                <p className="text-rose-300/80 leading-relaxed text-[10px]">
                  {t("connection.disconnectedHint")}
                </p>

                <div className="pt-1 flex flex-col gap-1.5">
                  <button
                    onClick={handleLaunchServer}
                    disabled={isLaunching || isConnecting}
                    className="w-full flex items-center justify-center gap-1.5 bg-accent hover:bg-accent/90 disabled:opacity-50 text-white font-medium py-1.5 px-2 rounded shadow-xs cursor-pointer transition active:scale-98"
                    title="バックエンドサーバーのみを起動（クライアントは多重起動しません）"
                  >
                    {isLaunching ? (
                      <>
                        <Loader2 className="w-3.5 h-3.5 animate-spin" />
                        {t("connection.launchingServerBtn")}
                      </>
                    ) : (
                      <>
                        <Play className="w-3.5 h-3.5" />
                        {t("connection.launchServerBtn")}
                      </>
                    )}
                  </button>
                  <p className="text-[10px] text-textSecondary text-center">
                    {t("connection.launchServerNote")}
                  </p>

                  <div className="flex flex-col gap-1 pt-1 border-t border-rose-500/20">
                    <p className="text-[10px] text-rose-300/80">
                      {t("connection.launchManualHint")}
                    </p>
                    <button
                      onClick={handleCopyCommand}
                      className="w-full flex items-center justify-center gap-1 bg-surfaceLight hover:bg-surfaceLight/80 text-textSecondary hover:text-textPrimary border border-border py-1 px-2 rounded transition cursor-pointer text-[10px]"
                      title={`起動コマンド (${serverCommand}) をコピー`}
                    >
                      <Copy className="w-3 h-3" />
                      {copied ? t("connection.copyCommandDone", { script: scriptName }) : t("connection.copyCommandBtn", { script: scriptName })}
                    </button>
                  </div>
                </div>
              </div>
            )}

            {/* 再接続・確認ボタン */}
            <div className="pt-1 flex items-center gap-2">
              <button
                onClick={handleManualRetry}
                disabled={isConnecting}
                className="flex-1 flex items-center justify-center gap-1.5 bg-surfaceLight hover:bg-surfaceLight/80 text-textPrimary border border-border py-1.5 px-3 rounded font-medium transition cursor-pointer disabled:opacity-50"
              >
                <RefreshCw className={`w-3.5 h-3.5 ${isConnecting ? "animate-spin" : ""}`} />
                {isConnecting ? t("connection.retryingBtn") : t("connection.retryBtn")}
              </button>
            </div>
          </div>
        </>
      )}
    </div>
  );
};
