import React, { useCallback, useEffect, useState } from "react";
import {
  AlertCircle,
  AlertTriangle,
  Check,
  Copy,
  ExternalLink,
  FolderOpen,
  Info,
  RefreshCw,
  X,
} from "lucide-react";
import { backendApi } from "../../lib/ipc";
import { useAppStore } from "../../store";
import type { FailedImageRecord } from "../../types/generated/FailedImageRecord";
import type { LogEntry } from "../../types/generated/LogEntry";

/**
 * バイト数を人間が読みやすい単位（B, KB, MB, GB）に変換する
 *
 * @param bytes バイト数
 * @returns フォーマット済み文字列
 */
function formatBytes(bytes: number): string {
  if (bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(1))} ${sizes[i]}`;
}

/**
 * ログレベルに応じた文字色・背景色クラスを取得
 *
 * @param level ログレベル文字列
 * @returns Tailwind CSS クラス名
 */
function getLevelBadgeClass(level: string): string {
  switch (level.toUpperCase()) {
    case "ERROR":
      return "bg-rose-500/20 text-rose-400 border border-rose-500/30";
    case "WARN":
      return "bg-amber-500/20 text-amber-400 border border-amber-500/30";
    case "INFO":
      return "bg-sky-500/20 text-sky-400 border border-sky-500/30";
    case "DEBUG":
    case "TRACE":
    default:
      return "bg-slate-500/20 text-slate-400 border border-slate-500/30";
  }
}

/**
 * システムログおよびサムネイル生成失敗画像の確認・診断モーダル
 */
export const LogViewerModal: React.FC = () => {
  const isLogModalOpen = useAppStore((state) => state.isLogModalOpen);
  const closeLogModal = useAppStore((state) => state.closeLogModal);

  const [activeTab, setActiveTab] = useState<"failed" | "logs">("failed");
  const [failedImages, setFailedImages] = useState<FailedImageRecord[]>([]);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [logFilter, setLogFilter] = useState<"all" | "errors" | "warn_error">("all");
  const [isLoading, setIsLoading] = useState<boolean>(false);
  const [isCopied, setIsCopied] = useState<boolean>(false);
  const [actionMessage, setActionMessage] = useState<string | null>(null);

  // 失敗画像一覧およびログを取得
  const fetchData = useCallback(async () => {
    setIsLoading(true);
    try {
      const [failedRes, logRes] = await Promise.all([
        backendApi.getFailedThumbnails(200).catch(() => [] as FailedImageRecord[]),
        backendApi.getLogs(300).catch(() => [] as LogEntry[]),
      ]);
      setFailedImages(failedRes);
      setLogs(logRes);
    } catch {
      // 取得失敗時は何もしない
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    if (isLogModalOpen) {
      fetchData();
    }
  }, [isLogModalOpen, fetchData]);

  // Escキーで閉じる
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape" && isLogModalOpen) {
        closeLogModal();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [isLogModalOpen, closeLogModal]);

  if (!isLogModalOpen) return null;

  // ログのフィルタリング
  const filteredLogs = logs.filter((log) => {
    if (logFilter === "errors") return log.level.toUpperCase() === "ERROR";
    if (logFilter === "warn_error")
      return log.level.toUpperCase() === "ERROR" || log.level.toUpperCase() === "WARN";
    return true;
  });

  // ログコピー処理
  const handleCopyLogs = async () => {
    const text = filteredLogs
      .map((l) => `${l.timestamp} [${l.level}] ${l.target} - ${l.message}`)
      .join("\n");
    try {
      await navigator.clipboard.writeText(text);
      setIsCopied(true);
      setTimeout(() => setIsCopied(false), 2000);
    } catch {
      // コピー失敗時は無視
    }
  };

  // ログフォルダを開く
  const handleOpenLogFolder = async () => {
    try {
      const res = await backendApi.openLogFolder();
      if (res.path) {
        setActionMessage(`ログフォルダを開きました: ${res.path}`);
        setTimeout(() => setActionMessage(null), 4000);
      }
    } catch (e: unknown) {
      const err = e as { message?: string };
      setActionMessage(`開けませんでした: ${err?.message || String(e)}`);
      setTimeout(() => setActionMessage(null), 4000);
    }
  };

  // 失敗画像の再試行
  const handleRetryFailed = async () => {
    try {
      setIsLoading(true);
      const res = await backendApi.rescanMissingThumbnails();
      setActionMessage(`再生成を開始しました (対象: ${res.resetCount}件)`);
      setTimeout(() => setActionMessage(null), 4000);
      await fetchData();
    } catch (e: unknown) {
      const err = e as { message?: string };
      setActionMessage(`エラー: ${err?.message || String(e)}`);
      setTimeout(() => setActionMessage(null), 4000);
    } finally {
      setIsLoading(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm p-4"
      onClick={closeLogModal}
    >
      <div
        className="bg-surface border border-border rounded-xl shadow-2xl w-full max-w-4xl h-[85vh] flex flex-col overflow-hidden animate-in fade-in zoom-in-95 duration-150"
        onClick={(e) => e.stopPropagation()}
      >
        {/* ヘッダー */}
        <div className="flex items-center justify-between px-5 py-3.5 border-b border-border bg-surfaceLight/30">
          <div className="flex items-center gap-2.5">
            <AlertCircle className="w-5 h-5 text-accent" />
            <h2 className="text-base font-semibold text-textPrimary">システムログ & エラー診断</h2>
          </div>
          <button
            onClick={closeLogModal}
            className="p-1.5 rounded-lg text-textSecondary hover:text-textPrimary hover:bg-surfaceLight transition"
            title="閉じる (Esc)"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* タブナビゲーション */}
        <div className="flex items-center justify-between px-5 border-b border-border bg-surface">
          <div className="flex gap-4">
            <button
              onClick={() => setActiveTab("failed")}
              className={`py-3 text-xs font-medium border-b-2 transition flex items-center gap-1.5 ${
                activeTab === "failed"
                  ? "border-accent text-accent"
                  : "border-transparent text-textSecondary hover:text-textPrimary"
              }`}
            >
              <AlertTriangle className="w-3.5 h-3.5" />
              <span>失敗画像一覧</span>
              <span
                className={`ml-1 px-1.5 py-0.2 rounded-full text-[10px] font-bold ${
                  failedImages.length > 0
                    ? "bg-amber-500/20 text-amber-400"
                    : "bg-surfaceLight text-textSecondary"
                }`}
              >
                {failedImages.length}
              </span>
            </button>

            <button
              onClick={() => setActiveTab("logs")}
              className={`py-3 text-xs font-medium border-b-2 transition flex items-center gap-1.5 ${
                activeTab === "logs"
                  ? "border-accent text-accent"
                  : "border-transparent text-textSecondary hover:text-textPrimary"
              }`}
            >
              <Info className="w-3.5 h-3.5" />
              <span>システムログ</span>
              <span className="ml-1 px-1.5 py-0.2 rounded-full text-[10px] bg-surfaceLight text-textSecondary">
                {logs.length}
              </span>
            </button>
          </div>

          {/* ツールバー */}
          <div className="flex items-center gap-2">
            {activeTab === "logs" && (
              <div className="flex items-center gap-1 bg-surfaceLight/50 p-0.5 rounded text-[11px] border border-border">
                <button
                  onClick={() => setLogFilter("all")}
                  className={`px-2 py-0.5 rounded transition ${
                    logFilter === "all" ? "bg-accent text-white font-medium" : "text-textSecondary hover:text-textPrimary"
                  }`}
                >
                  すべて
                </button>
                <button
                  onClick={() => setLogFilter("warn_error")}
                  className={`px-2 py-0.5 rounded transition ${
                    logFilter === "warn_error" ? "bg-accent text-white font-medium" : "text-textSecondary hover:text-textPrimary"
                  }`}
                >
                  警告・エラー
                </button>
                <button
                  onClick={() => setLogFilter("errors")}
                  className={`px-2 py-0.5 rounded transition ${
                    logFilter === "errors" ? "bg-accent text-white font-medium" : "text-textSecondary hover:text-textPrimary"
                  }`}
                >
                  エラーのみ
                </button>
              </div>
            )}

            <button
              onClick={fetchData}
              disabled={isLoading}
              className="p-1.5 rounded hover:bg-surfaceLight text-textSecondary hover:text-textPrimary transition"
              title="最新に更新"
            >
              <RefreshCw className={`w-4 h-4 ${isLoading ? "animate-spin" : ""}`} />
            </button>

            {activeTab === "logs" && (
              <button
                onClick={handleCopyLogs}
                className="flex items-center gap-1 px-2.5 py-1 text-xs rounded bg-surfaceLight hover:bg-surfaceLight/80 text-textPrimary border border-border transition"
                title="ログをクリップボードにコピー"
              >
                {isCopied ? <Check className="w-3.5 h-3.5 text-emerald-400" /> : <Copy className="w-3.5 h-3.5" />}
                <span>{isCopied ? "コピー完了" : "コピー"}</span>
              </button>
            )}

            <button
              onClick={handleOpenLogFolder}
              className="flex items-center gap-1 px-2.5 py-1 text-xs rounded bg-surfaceLight hover:bg-surfaceLight/80 text-textPrimary border border-border transition"
              title="ログファイルが保存されているフォルダをエクスプローラで開く"
            >
              <FolderOpen className="w-3.5 h-3.5" />
              <span>ログフォルダを開く</span>
            </button>
          </div>
        </div>

        {/* 通知メッセージバー */}
        {actionMessage && (
          <div className="bg-sky-500/15 border-b border-sky-500/30 px-5 py-1.5 text-xs text-sky-300 flex items-center justify-between">
            <span>{actionMessage}</span>
            <button onClick={() => setActionMessage(null)} className="hover:text-white">
              <X className="w-3.5 h-3.5" />
            </button>
          </div>
        )}

        {/* コンテンツ領域 */}
        <div className="flex-1 overflow-auto p-5 bg-background">
          {activeTab === "failed" ? (
            <div className="space-y-4">
              <div className="flex items-center justify-between">
                <p className="text-xs text-textSecondary leading-relaxed">
                  画像ファイルの破損、0バイトの空ファイル、または非対応の動画/アニメーション形式等の理由で、
                  サムネイルを正常に生成できなかったファイルです。
                </p>
                {failedImages.length > 0 && (
                  <button
                    onClick={handleRetryFailed}
                    disabled={isLoading}
                    className="shrink-0 flex items-center gap-1.5 px-3 py-1 text-xs rounded bg-accent/20 hover:bg-accent/30 text-accent font-medium border border-accent/40 transition"
                  >
                    <RefreshCw className={`w-3.5 h-3.5 ${isLoading ? "animate-spin" : ""}`} />
                    <span>失敗画像の再生成を試みる</span>
                  </button>
                )}
              </div>

              {failedImages.length === 0 ? (
                <div className="flex flex-col items-center justify-center py-16 text-center text-textSecondary">
                  <Check className="w-10 h-10 text-emerald-400 mb-2" />
                  <p className="text-sm font-medium text-textPrimary">サムネイル生成の失敗はありません</p>
                  <p className="text-xs mt-1">すべての画像が正常に処理されました。</p>
                </div>
              ) : (
                <div className="border border-border rounded-lg overflow-hidden bg-surface">
                  <div className="grid grid-cols-[1fr_80px_100px_90px] px-4 py-2 bg-surfaceLight/50 border-b border-border text-[11px] font-semibold text-textSecondary uppercase tracking-wider">
                    <div>ファイルパス</div>
                    <div className="text-right">サイズ</div>
                    <div className="text-center">形式</div>
                    <div className="text-right">操作</div>
                  </div>
                  <div className="divide-y divide-border/60">
                    {failedImages.map((item) => (
                      <div
                        key={item.id}
                        className="grid grid-cols-[1fr_80px_100px_90px] items-center px-4 py-2.5 text-xs hover:bg-surfaceLight/30 transition font-mono"
                      >
                        <div className="truncate pr-2" title={item.filePath}>
                          <span className="text-textPrimary">{item.filePath}</span>
                          {item.fileSize === 0 && (
                            <span className="ml-2 text-[10px] text-rose-400 font-sans font-medium bg-rose-500/10 px-1.5 py-0.2 rounded border border-rose-500/20">
                              0バイト空ファイル
                            </span>
                          )}
                        </div>
                        <div className={`text-right ${item.fileSize === 0 ? "text-rose-400 font-bold" : "text-textSecondary"}`}>
                          {formatBytes(item.fileSize)}
                        </div>
                        <div className="text-center text-textSecondary uppercase">
                          {item.format || "不明"}
                        </div>
                        <div className="text-right">
                          <button
                            onClick={() => backendApi.revealInFileManager(item.id)}
                            className="inline-flex items-center gap-1 text-[11px] text-accent hover:underline"
                            title="エクスプローラでファイルを表示"
                          >
                            <span>表示</span>
                            <ExternalLink className="w-3 h-3" />
                          </button>
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </div>
          ) : (
            <div className="h-full flex flex-col space-y-2">
              <div className="flex items-center justify-between text-xs text-textSecondary">
                <span>直近 {filteredLogs.length} 件のログ（最新順）</span>
                <span className="text-[11px]">ログ保存先: %APPDATA%\com.imagemediaviewer.app\logs\app.log</span>
              </div>

              <div className="flex-1 bg-surface border border-border rounded-lg p-3 overflow-auto font-mono text-[11px] leading-relaxed select-text space-y-1">
                {filteredLogs.length === 0 ? (
                  <p className="text-textSecondary text-center py-10">該当するログはありません</p>
                ) : (
                  [...filteredLogs].reverse().map((log, idx) => (
                    <div key={idx} className="flex items-start gap-2 hover:bg-surfaceLight/30 px-1 py-0.5 rounded">
                      <span className="text-textSecondary/70 shrink-0 text-[10px] pt-0.5">{log.timestamp}</span>
                      <span className={`px-1.5 py-0.2 rounded text-[10px] font-bold shrink-0 ${getLevelBadgeClass(log.level)}`}>
                        {log.level}
                      </span>
                      <span className="text-accent/80 shrink-0 max-w-[140px] truncate" title={log.target}>
                        [{log.target}]
                      </span>
                      <span className={`break-all ${log.level === "ERROR" ? "text-rose-300 font-semibold" : log.level === "WARN" ? "text-amber-300" : "text-textPrimary"}`}>
                        {log.message}
                      </span>
                    </div>
                  ))
                )}
              </div>
            </div>
          )}
        </div>

        {/* フッター */}
        <div className="px-5 py-2.5 border-t border-border bg-surfaceLight/20 flex items-center justify-between text-xs text-textSecondary">
          <span>Escキーまたは背景クリックで閉じることができます</span>
          <button
            onClick={closeLogModal}
            className="px-4 py-1.5 rounded-lg bg-surfaceLight hover:bg-surfaceLight/80 text-textPrimary border border-border transition text-xs font-medium"
          >
            閉じる
          </button>
        </div>
      </div>
    </div>
  );
};
