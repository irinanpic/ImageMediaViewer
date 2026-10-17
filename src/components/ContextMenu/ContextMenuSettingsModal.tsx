import React, { useState, useEffect, useCallback } from "react";
import {
  Plus,
  Trash2,
  X,
  Check,
  ExternalLink,
  Puzzle,
  FolderOpen,
  FileArchive,
  RefreshCw,
} from "lucide-react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useAppStore } from "../../store";
import { useTranslation } from "../../locales";
import { backendApi } from "../../lib/ipc";
import type { CustomContextMenuItem } from "../../types/settings";
import type { InstalledPluginInfo } from "../../types/generated/InstalledPluginInfo";

/**
 * プラグイン管理および右クリックメニューのカスタム項目（外部アプリ連携）を設定・管理するモーダル
 *
 * 変更理由: ユーザーが外部開発されたプラグイン（ClipStudioプラグイン等）をZIPやフォルダから
 * ワンクリックで簡単に導入・有効/無効切替できるようにするとともに、
 * 任意のペイントソフトやエディタを右クリックメニューへ登録可能にするため。
 */
export const ContextMenuSettingsModal: React.FC = () => {
  const { locale } = useTranslation();
  const isOpen = useAppStore((state) => state.isContextMenuSettingsOpen);
  const close = useAppStore((state) => state.closeContextMenuSettings);
  const appSettings = useAppStore((state) => state.appSettings);
  const updateAppSettings = useAppStore((state) => state.updateAppSettings);
  const loadAppSettings = useAppStore((state) => state.loadAppSettings);
  const refreshTimeline = useAppStore((state) => state.refreshTimeline);

  const [activeTab, setActiveTab] = useState<"plugins" | "custom">("plugins");
  const [installedPlugins, setInstalledPlugins] = useState<InstalledPluginInfo[]>([]);
  const [isLoadingPlugins, setIsLoadingPlugins] = useState(false);
  const [items, setItems] = useState<CustomContextMenuItem[]>([]);
  const [isSaving, setIsSaving] = useState(false);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [successMsg, setSuccessMsg] = useState<string | null>(null);
  const [hasAllFilesAccess, setHasAllFilesAccess] = useState<boolean>(true);

  const isMobileEnv =
    typeof navigator !== "undefined" && /Android|iPhone|iPad|iPod/i.test(navigator.userAgent);

  const fetchPlugins = useCallback(async () => {
    setIsLoadingPlugins(true);
    try {
      const list = await backendApi.getInstalledPlugins();
      setInstalledPlugins(list);
    } catch (err) {
      console.warn("導入済みプラグイン一覧の取得に失敗:", err);
    } finally {
      setIsLoadingPlugins(false);
    }
  }, []);

  // モーダルオープン時に現在の設定とプラグイン一覧を同期
  useEffect(() => {
    if (isOpen) {
      if (appSettings) {
        setItems(appSettings.custom_context_menu_items || []);
      }
      setErrorMsg(null);
      setSuccessMsg(null);
      fetchPlugins();
      if (isMobileEnv) {
        backendApi
          .checkAllFilesAccess()
          .then((granted) => setHasAllFilesAccess(granted))
          .catch(() => {});
      }
    }
  }, [isOpen, appSettings, fetchPlugins, isMobileEnv]);

  // Escapeキー押下でモーダルを閉じる
  useEffect(() => {
    if (!isOpen) return;
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        close();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [isOpen, close]);

  if (!isOpen) return null;

  const handleRequestAllFilesAccess = async () => {
    try {
      const granted = await backendApi.requestAllFilesAccess();
      setHasAllFilesAccess(granted);
    } catch (err) {
      console.warn("すべてのファイルアクセス権限画面の起動に失敗:", err);
    }
  };

  // プラグインの有効/無効（ON/OFF）トグル切り替え
  const handleTogglePlugin = async (plugin: InstalledPluginInfo, nextEnabled: boolean) => {
    if (!appSettings) return;
    setErrorMsg(null);
    setSuccessMsg(null);

    const nextList = new Set(appSettings.enabled_plugins);
    const disableRule = `!${plugin.id}`;

    if (nextEnabled) {
      nextList.delete(disableRule);
      if (!nextList.has("*") && !nextList.has("format.*")) {
        nextList.add(plugin.id);
      }
    } else {
      nextList.delete(plugin.id);
      nextList.add(disableRule);
    }

    try {
      const updated = {
        ...appSettings,
        enabled_plugins: Array.from(nextList),
      };
      await updateAppSettings(updated);
      await fetchPlugins();
      refreshTimeline();
    } catch (err) {
      console.error("プラグイン状態変更エラー:", err);
      setErrorMsg(
        locale === "en"
          ? "Failed to update plugin status."
          : "プラグインの有効/無効状態の更新に失敗しました。"
      );
    }
  };

  // ZIPファイルまたは plugin.json からプラグインをインポート
  const handleImportPluginFile = async () => {
    setErrorMsg(null);
    setSuccessMsg(null);
    try {
      const selected = await openDialog({
        multiple: false,
        directory: false,
        filters: [
          {
            name: locale === "en" ? "Plugin Package (.zip, plugin.json)" : "プラグインパッケージ (.zip, plugin.json)",
            extensions: ["zip", "json"],
          },
        ],
      });
      if (!selected || typeof selected !== "string") return;

      setIsLoadingPlugins(true);
      const updatedList = await backendApi.installPlugin(selected);
      setInstalledPlugins(updatedList);
      await loadAppSettings();
      refreshTimeline();
      setSuccessMsg(
        locale === "en"
          ? "Plugin installed and enabled successfully."
          : "プラグインを導入し、有効化しました。"
      );
    } catch (err: unknown) {
      const msg =
        err && typeof err === "object" && "message" in err
          ? String((err as { message: unknown }).message)
          : String(err);
      setErrorMsg(msg);
    } finally {
      setIsLoadingPlugins(false);
    }
  };

  // フォルダからプラグインをインポート
  const handleImportPluginFolder = async () => {
    setErrorMsg(null);
    setSuccessMsg(null);
    try {
      const selected = await openDialog({
        multiple: false,
        directory: true,
      });
      if (!selected || typeof selected !== "string") return;

      setIsLoadingPlugins(true);
      const updatedList = await backendApi.installPlugin(selected);
      setInstalledPlugins(updatedList);
      await loadAppSettings();
      refreshTimeline();
      setSuccessMsg(
        locale === "en"
          ? "Plugin folder imported and enabled successfully."
          : "プラグインフォルダを導入し、有効化しました。"
      );
    } catch (err: unknown) {
      const msg =
        err && typeof err === "object" && "message" in err
          ? String((err as { message: unknown }).message)
          : String(err);
      setErrorMsg(msg);
    } finally {
      setIsLoadingPlugins(false);
    }
  };

  // プラグインフォルダをエクスプローラーで開く
  const handleOpenPluginsFolder = async () => {
    try {
      await backendApi.openPluginsFolder();
    } catch (err) {
      console.error("プラグインフォルダ表示エラー:", err);
    }
  };

  // プラグインの手動再読み込み
  const handleReloadPlugins = async () => {
    setErrorMsg(null);
    setSuccessMsg(null);
    setIsLoadingPlugins(true);
    try {
      if (appSettings) {
        await updateAppSettings(appSettings);
      }
      await fetchPlugins();
      await backendApi.rescan();
      refreshTimeline();
      setSuccessMsg(
        locale === "en" ? "Plugins reloaded." : "プラグインを再読み込みしました。"
      );
    } catch (err) {
      console.error("プラグイン再読み込みエラー:", err);
    } finally {
      setIsLoadingPlugins(false);
    }
  };

  // 新規カスタムメニュー項目の追加
  const handleAddItem = () => {
    const newItem: CustomContextMenuItem = {
      id: `custom-${Date.now()}`,
      name: "",
      appPath: "",
      args: "{path}",
      targetExtensions: [],
      enabled: true,
    };
    setItems([...items, newItem]);
  };

  // 項目の削除
  const handleDeleteItem = (id: string) => {
    setItems(items.filter((item) => item.id !== id));
  };

  // 各フィールドの更新
  const handleUpdateItem = (
    id: string,
    field: keyof CustomContextMenuItem,
    value: unknown
  ) => {
    setItems(
      items.map((item) => {
        if (item.id !== id) return item;
        return { ...item, [field]: value };
      })
    );
  };

  // カスタムメニュー設定の保存処理
  const handleSave = async () => {
    if (!appSettings) return;

    const validItems = items.filter(
      (item) => item.name.trim() !== "" || item.appPath.trim() !== ""
    );
    for (const it of validItems) {
      if (it.name.trim() === "" || it.appPath.trim() === "") {
        setErrorMsg(
          locale === "en"
            ? "Both Name and Application Path are required for each item."
            : "すべての項目で「表示名」と「アプリパス」を入力してください。"
        );
        return;
      }
    }

    setIsSaving(true);
    setErrorMsg(null);
    try {
      const updated = {
        ...appSettings,
        custom_context_menu_items: validItems,
      };
      await updateAppSettings(updated);
      close();
    } catch (err) {
      console.error("設定保存エラー:", err);
      setErrorMsg(
        locale === "en"
          ? "Failed to save settings. Please try again."
          : "設定の保存に失敗しました。もう一度お試しください。"
      );
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <div
      role="dialog"
      aria-modal="true"
      onClick={close}
      className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/60 backdrop-blur-sm animate-in fade-in duration-200"
    >
      <div
        onClick={(e) => e.stopPropagation()}
        className="bg-surface border border-border rounded-xl shadow-2xl w-full max-w-2xl max-h-[85vh] flex flex-col overflow-hidden text-textPrimary"
      >
        {/* ヘッダー */}
        <div className="px-6 pt-4 pb-3 border-b border-border/60 flex items-center justify-between">
          <div>
            <h2 className="text-lg font-semibold flex items-center gap-2">
              <Puzzle className="w-5 h-5 text-accent" />
              {locale === "en"
                ? "Plugins & Context Menu Settings"
                : "プラグイン・右クリックメニュー設定"}
            </h2>
            <p className="text-xs text-textSecondary mt-0.5">
              {locale === "en"
                ? "Import format/action plugins or register external apps for the context menu."
                : "外部プラグインの導入・ON/OFF切替、および右クリックメニューの外部アプリ連携を管理します。"}
            </p>
          </div>
          <button
            onClick={close}
            className="p-1.5 hover:bg-surfaceLight rounded-full text-textSecondary hover:text-textPrimary transition"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* タブバー */}
        <div className="px-6 flex items-center gap-2 border-b border-border/60 bg-surfaceLight/20">
          <button
            onClick={() => {
              setActiveTab("plugins");
              setErrorMsg(null);
              setSuccessMsg(null);
            }}
            className={`px-4 py-2.5 text-xs font-semibold border-b-2 transition flex items-center gap-1.5 ${
              activeTab === "plugins"
                ? "border-accent text-accent"
                : "border-transparent text-textSecondary hover:text-textPrimary"
            }`}
          >
            <Puzzle className="w-3.5 h-3.5" />
            {locale === "en" ? "Installed Plugins" : "プラグイン管理"}
            <span className="ml-1 px-1.5 py-0.2 rounded-full bg-surfaceLight text-[10px]">
              {installedPlugins.length}
            </span>
          </button>
          <button
            onClick={() => {
              setActiveTab("custom");
              setErrorMsg(null);
              setSuccessMsg(null);
            }}
            className={`px-4 py-2.5 text-xs font-semibold border-b-2 transition flex items-center gap-1.5 ${
              activeTab === "custom"
                ? "border-accent text-accent"
                : "border-transparent text-textSecondary hover:text-textPrimary"
            }`}
          >
            <ExternalLink className="w-3.5 h-3.5" />
            {locale === "en" ? "Custom App Actions" : "カスタム外部アプリ連携"}
            <span className="ml-1 px-1.5 py-0.2 rounded-full bg-surfaceLight text-[10px]">
              {items.length}
            </span>
          </button>
        </div>

        {/* メッセージ表示 */}
        {errorMsg && (
          <div className="mx-6 mt-3 p-2.5 bg-red-500/10 border border-red-500/30 text-red-400 text-xs rounded-lg">
            {errorMsg}
          </div>
        )}
        {successMsg && (
          <div className="mx-6 mt-3 p-2.5 bg-emerald-500/10 border border-emerald-500/30 text-emerald-400 text-xs rounded-lg">
            {successMsg}
          </div>
        )}

        {/* タブ1: プラグイン管理 */}
        {activeTab === "plugins" && (
          <div className="p-6 overflow-y-auto flex-1 space-y-4">
            {/* Android 環境での独自形式 (.clip 等) スキャン権限案内 */}
            {isMobileEnv && (
              <div className="p-3 rounded-lg bg-surfaceLight/50 border border-border/80 flex flex-col gap-2">
                <p className="text-[11px] text-textSecondary leading-relaxed">
                  {locale === "en"
                    ? "On Android 11+, custom format files (.clip, etc.) and deleting files created by other apps require 'All files access'."
                    : "Android 11以降で独自形式ファイル（.clip 等）を検出・削除するには「すべてのファイルへのアクセス」権限が必要です。"}
                </p>
                <div className="flex items-center justify-between gap-2">
                  <span
                    className={`text-[11px] font-medium ${
                      hasAllFilesAccess ? "text-emerald-400" : "text-amber-400"
                    }`}
                  >
                    {hasAllFilesAccess
                      ? locale === "en"
                        ? "All Files Access: Granted"
                        : "すべてのファイルへのアクセス: 許可済み"
                      : locale === "en"
                      ? "All Files Access: Not Granted"
                      : "すべてのファイルへのアクセス: 未許可"}
                  </span>
                  {!hasAllFilesAccess && (
                    <button
                      type="button"
                      onClick={handleRequestAllFilesAccess}
                      className="px-2.5 py-1 bg-accent/20 hover:bg-accent/30 text-accent border border-accent/40 rounded text-xs font-medium transition"
                    >
                      {locale === "en"
                        ? "Open Permission Settings..."
                        : "権限設定を開く..."}
                    </button>
                  )}
                </div>
              </div>
            )}

            {/* 簡単インポート＆フォルダ操作ツールバー */}
            <div className="flex flex-wrap items-center justify-between gap-2 bg-surfaceLight/30 p-3 rounded-lg border border-border/60">
              <div className="flex flex-wrap items-center gap-2">
                <button
                  onClick={handleImportPluginFile}
                  disabled={isLoadingPlugins}
                  className="px-3 py-1.5 bg-accent hover:bg-accent/90 text-white text-xs font-medium rounded-lg transition flex items-center gap-1.5 shadow-sm disabled:opacity-50"
                >
                  <FileArchive className="w-3.5 h-3.5" />
                  {locale === "en" ? "Import from ZIP..." : "ZIPから追加..."}
                </button>
                <button
                  onClick={handleImportPluginFolder}
                  disabled={isLoadingPlugins}
                  className="px-3 py-1.5 bg-surface hover:bg-surfaceLight border border-border text-textPrimary text-xs font-medium rounded-lg transition flex items-center gap-1.5 disabled:opacity-50"
                >
                  <FolderOpen className="w-3.5 h-3.5 text-accent" />
                  {locale === "en" ? "Import from Folder..." : "フォルダから追加..."}
                </button>
              </div>
              <div className="flex items-center gap-1.5">
                <button
                  onClick={handleOpenPluginsFolder}
                  title={
                    locale === "en"
                      ? "Open user plugins directory in file manager"
                      : "ユーザーのプラグイン配置フォルダを開く"
                  }
                  className="px-2.5 py-1.5 bg-surface hover:bg-surfaceLight border border-border/80 text-textSecondary hover:text-textPrimary text-xs rounded-lg transition flex items-center gap-1"
                >
                  <FolderOpen className="w-3.5 h-3.5" />
                  {locale === "en" ? "Open Folder" : "プラグインフォルダを開く"}
                </button>
                <button
                  onClick={handleReloadPlugins}
                  disabled={isLoadingPlugins}
                  title={locale === "en" ? "Reload plugins" : "プラグインを再読み込み"}
                  className="p-1.5 bg-surface hover:bg-surfaceLight border border-border/80 text-textSecondary hover:text-textPrimary rounded-lg transition disabled:opacity-50"
                >
                  <RefreshCw className={`w-3.5 h-3.5 ${isLoadingPlugins ? "animate-spin" : ""}`} />
                </button>
              </div>
            </div>

            {/* 導入済みプラグイン一覧 */}
            {installedPlugins.length === 0 ? (
              <div className="text-center py-8 text-textSecondary border border-dashed border-border/60 rounded-xl">
                <p className="text-sm">
                  {locale === "en"
                    ? "No format/action plugins installed."
                    : "導入されているプラグインはありません。"}
                </p>
                <p className="text-xs text-textSecondary/70 mt-1">
                  {locale === "en"
                    ? "Use 'Import from ZIP...' or 'Import from Folder...' to add a plugin."
                    : "上の「ZIPから追加...」または「フォルダから追加...」からプラグインを導入できます。"}
                </p>
              </div>
            ) : (
              <div className="space-y-3">
                {installedPlugins.map((plugin) => (
                  <div
                    key={plugin.id}
                    className={`p-4 border rounded-lg transition ${
                      plugin.enabled
                        ? "bg-surfaceLight/40 border-border/80"
                        : "bg-surfaceLight/10 border-border/40 opacity-70"
                    }`}
                  >
                    <div className="flex items-start justify-between gap-4">
                      <div className="min-w-0 flex-1">
                        <div className="flex flex-wrap items-center gap-2">
                          <span className="font-semibold text-sm text-textPrimary">
                            {plugin.name}
                          </span>
                          {plugin.version && (
                            <span className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-surface border border-border/60 text-textSecondary">
                              v{plugin.version}
                            </span>
                          )}
                          <span className="text-[10px] font-mono text-textSecondary/80">
                            ({plugin.id})
                          </span>
                        </div>
                        {plugin.description && (
                          <p className="text-xs text-textSecondary mt-1 leading-relaxed">
                            {plugin.description}
                          </p>
                        )}
                        <div className="flex flex-wrap items-center gap-3 mt-2.5 text-[11px] text-textSecondary">
                          {plugin.extensions.length > 0 && (
                            <div className="flex items-center gap-1">
                              <span>{locale === "en" ? "Formats:" : "対応拡張子:"}</span>
                              {plugin.extensions.map((ext) => (
                                <span
                                  key={ext}
                                  className="px-1.5 py-0.2 bg-accent/15 text-accent rounded font-mono font-medium"
                                >
                                  .{ext}
                                </span>
                              ))}
                            </div>
                          )}
                          {plugin.actionNames.length > 0 && (
                            <div className="flex items-center gap-1">
                              <span>{locale === "en" ? "Menu:" : "右クリック連携:"}</span>
                              <span className="text-textPrimary font-medium">
                                {plugin.actionNames.join(", ")}
                              </span>
                            </div>
                          )}
                        </div>
                      </div>

                      {/* ON/OFF 切替 */}
                      <label className="flex items-center gap-2 cursor-pointer select-none shrink-0">
                        <input
                          type="checkbox"
                          checked={plugin.enabled}
                          onChange={(e) => handleTogglePlugin(plugin, e.target.checked)}
                          className="w-4 h-4 rounded text-accent focus:ring-accent"
                        />
                        <span className="text-xs font-medium">
                          {plugin.enabled
                            ? locale === "en"
                              ? "Enabled"
                              : "有効"
                            : locale === "en"
                            ? "Disabled"
                            : "無効"}
                        </span>
                      </label>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </div>
        )}

        {/* タブ2: カスタム外部アプリ連携 */}
        {activeTab === "custom" && (
          <div className="p-6 overflow-y-auto flex-1 space-y-4">
            {items.length === 0 ? (
              <div className="text-center py-8 text-textSecondary border border-dashed border-border/60 rounded-xl">
                <p className="text-sm">
                  {locale === "en"
                    ? "No custom menu items registered."
                    : "カスタムメニュー項目はまだ登録されていません。"}
                </p>
                <p className="text-xs text-textSecondary/70 mt-1">
                  {locale === "en"
                    ? "Click the button below to add an external application."
                    : "「+ 項目を追加」ボタンを押して外部アプリケーションを登録できます。"}
                </p>
              </div>
            ) : (
              items.map((item, idx) => (
                <div
                  key={item.id}
                  className="p-4 bg-surfaceLight/40 border border-border/60 rounded-lg space-y-3 relative group"
                >
                  <div className="flex items-center justify-between gap-4">
                    <div className="flex items-center gap-2">
                      <input
                        type="checkbox"
                        id={`enabled-${item.id}`}
                        checked={item.enabled}
                        onChange={(e) =>
                          handleUpdateItem(item.id, "enabled", e.target.checked)
                        }
                        className="w-4 h-4 rounded text-accent focus:ring-accent"
                      />
                      <label
                        htmlFor={`enabled-${item.id}`}
                        className="text-xs font-medium cursor-pointer"
                      >
                        {locale === "en" ? "Enabled" : "有効"}
                      </label>
                      <span className="text-xs text-textSecondary ml-2">
                        #{idx + 1}
                      </span>
                    </div>

                    <button
                      onClick={() => handleDeleteItem(item.id)}
                      title={locale === "en" ? "Delete item" : "項目を削除"}
                      className="p-1.5 text-textSecondary hover:text-red-400 hover:bg-surfaceLight rounded transition"
                    >
                      <Trash2 className="w-4 h-4" />
                    </button>
                  </div>

                  <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                    <div>
                      <label className="block text-xs font-medium text-textSecondary mb-1">
                        {locale === "en" ? "Display Name" : "表示名"}
                        <span className="text-red-400 ml-0.5">*</span>
                      </label>
                      <input
                        type="text"
                        value={item.name}
                        placeholder={
                          locale === "en" ? "e.g. PaintTool SAI" : "例: ペイントで開く"
                        }
                        onChange={(e) =>
                          handleUpdateItem(item.id, "name", e.target.value)
                        }
                        className="w-full px-3 py-1.5 bg-surface border border-border rounded text-sm focus:border-accent outline-none"
                      />
                    </div>

                    <div>
                      <label className="block text-xs font-medium text-textSecondary mb-1">
                        {locale === "en"
                          ? "Application Path / Command"
                          : "アプリパス / 実行コマンド"}
                        <span className="text-red-400 ml-0.5">*</span>
                      </label>
                      <input
                        type="text"
                        value={item.appPath}
                        placeholder={
                          locale === "en"
                            ? "e.g. mspaint.exe or C:\\..."
                            : "例: mspaint.exe または フルパス"
                        }
                        onChange={(e) =>
                          handleUpdateItem(item.id, "appPath", e.target.value)
                        }
                        className="w-full px-3 py-1.5 bg-surface border border-border rounded text-sm focus:border-accent outline-none"
                      />
                    </div>
                  </div>

                  <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                    <div>
                      <label className="block text-xs font-medium text-textSecondary mb-1">
                        {locale === "en" ? "Arguments Template" : "引数テンプレート"}
                      </label>
                      <input
                        type="text"
                        value={item.args ?? "{path}"}
                        placeholder="{path}"
                        onChange={(e) =>
                          handleUpdateItem(item.id, "args", e.target.value)
                        }
                        className="w-full px-3 py-1.5 bg-surface border border-border rounded text-sm focus:border-accent outline-none font-mono text-xs"
                      />
                      <p className="text-[10px] text-textSecondary mt-0.5">
                        {locale === "en"
                          ? '"{path}" is replaced with the image file path'
                          : '"{path}" が実際のファイルパスに置換されます'}
                      </p>
                    </div>

                    <div>
                      <label className="block text-xs font-medium text-textSecondary mb-1">
                        {locale === "en"
                          ? "Target Extensions (comma separated)"
                          : "対象拡張子（カンマ区切り）"}
                      </label>
                      <input
                        type="text"
                        value={(item.targetExtensions || []).join(", ")}
                        placeholder={
                          locale === "en"
                            ? "e.g. png, jpg, clip (blank for all)"
                            : "例: png, jpg, clip（空欄で全形式）"
                        }
                        onChange={(e) => {
                          const raw = e.target.value;
                          const exts = raw
                            .split(",")
                            .map((s) => s.trim())
                            .filter((s) => s.length > 0);
                          handleUpdateItem(item.id, "targetExtensions", exts);
                        }}
                        className="w-full px-3 py-1.5 bg-surface border border-border rounded text-sm focus:border-accent outline-none"
                      />
                      <p className="text-[10px] text-textSecondary mt-0.5">
                        {locale === "en"
                          ? "Leave blank to match all supported formats"
                          : "空欄の場合は全フォーマットで表示されます"}
                      </p>
                    </div>
                  </div>
                </div>
              ))
            )}

            <button
              onClick={handleAddItem}
              className="w-full py-2.5 border border-dashed border-border/80 hover:border-accent/80 hover:bg-accent/5 text-textSecondary hover:text-accent rounded-lg flex items-center justify-center gap-2 text-sm font-medium transition"
            >
              <Plus className="w-4 h-4" />
              {locale === "en" ? "Add Custom Item" : "項目を追加"}
            </button>
          </div>
        )}

        {/* フッター */}
        <div className="px-6 py-3.5 border-t border-border/60 bg-surface/50 flex items-center justify-end gap-3">
          {activeTab === "plugins" ? (
            <button
              onClick={close}
              className="px-5 py-2 bg-accent hover:bg-accent/90 text-white text-sm font-medium rounded-lg transition"
            >
              {locale === "en" ? "Done" : "閉じる"}
            </button>
          ) : (
            <>
              <button
                onClick={close}
                disabled={isSaving}
                className="px-4 py-2 text-sm text-textSecondary hover:text-textPrimary transition"
              >
                {locale === "en" ? "Cancel" : "キャンセル"}
              </button>
              <button
                onClick={handleSave}
                disabled={isSaving}
                className="px-5 py-2 bg-accent hover:bg-accent/90 text-white text-sm font-medium rounded-lg transition flex items-center gap-1.5 disabled:opacity-50"
              >
                <Check className="w-4 h-4" />
                {isSaving
                  ? locale === "en"
                    ? "Saving..."
                    : "保存中..."
                  : locale === "en"
                  ? "Save Settings"
                  : "設定を保存"}
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  );
};
