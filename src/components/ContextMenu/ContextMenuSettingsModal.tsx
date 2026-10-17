import React, { useState, useEffect } from "react";
import { Plus, Trash2, X, Check, ExternalLink } from "lucide-react";
import { useAppStore } from "../../store";
import { useTranslation } from "../../locales";
import type { CustomContextMenuItem } from "../../types/settings";

/**
 * 右クリックメニューのカスタム項目（外部アプリ連携）を設定・管理するモーダル
 *
 * 変更理由: ユーザーが任意のペイントソフトやエディタをコンテキストメニューに追加可能にする。
 * 設定は app_settings.json に永続化され、デフォルト状態では環境非依存を維持するため空配列とする。
 */
export const ContextMenuSettingsModal: React.FC = () => {
  const { locale } = useTranslation();
  const isOpen = useAppStore((state) => state.isContextMenuSettingsOpen);
  const close = useAppStore((state) => state.closeContextMenuSettings);
  const appSettings = useAppStore((state) => state.appSettings);
  const updateAppSettings = useAppStore((state) => state.updateAppSettings);

  const [items, setItems] = useState<CustomContextMenuItem[]>([]);
  const [isSaving, setIsSaving] = useState(false);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);

  // モーダルオープン時に現在の設定をローカルステートへ同期
  useEffect(() => {
    if (isOpen && appSettings) {
      setItems(appSettings.custom_context_menu_items || []);
      setErrorMsg(null);
    }
  }, [isOpen, appSettings]);

  if (!isOpen) return null;

  // 新規項目の追加
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

  // 保存処理
  const handleSave = async () => {
    if (!appSettings) return;

    // バリデーション: 入力のある項目で名前またはパスが空のものを検出
    const validItems = items.filter((item) => item.name.trim() !== "" || item.appPath.trim() !== "");
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
      className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/60 backdrop-blur-sm animate-in fade-in duration-200"
    >
      <div className="bg-surface border border-border rounded-xl shadow-2xl w-full max-w-2xl max-h-[85vh] flex flex-col overflow-hidden text-textPrimary">
        {/* ヘッダー */}
        <div className="px-6 py-4 border-b border-border/60 flex items-center justify-between">
          <div>
            <h2 className="text-lg font-semibold flex items-center gap-2">
              <ExternalLink className="w-5 h-5 text-accent" />
              {locale === "en" ? "Custom Context Menu Settings" : "右クリックメニューのカスタム項目設定"}
            </h2>
            <p className="text-xs text-textSecondary mt-0.5">
              {locale === "en"
                ? "Register custom applications to launch directly from the right-click menu."
                : "右クリックメニューから直接起動できる外部アプリ（ペイントソフト等）を登録します。"}
            </p>
          </div>
          <button
            onClick={close}
            className="p-1.5 hover:bg-surfaceLight rounded-full text-textSecondary hover:text-textPrimary transition"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* エラーメッセージ */}
        {errorMsg && (
          <div className="mx-6 mt-3 p-2.5 bg-red-500/10 border border-red-500/30 text-red-400 text-xs rounded-lg">
            {errorMsg}
          </div>
        )}

        {/* 項目リスト */}
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
                      placeholder={locale === "en" ? "e.g. PaintTool SAI" : "例: ペイントで開く"}
                      onChange={(e) =>
                        handleUpdateItem(item.id, "name", e.target.value)
                      }
                      className="w-full px-3 py-1.5 bg-surface border border-border rounded text-sm focus:border-accent outline-none"
                    />
                  </div>

                  <div>
                    <label className="block text-xs font-medium text-textSecondary mb-1">
                      {locale === "en" ? "Application Path / Command" : "アプリパス / 実行コマンド"}
                      <span className="text-red-400 ml-0.5">*</span>
                    </label>
                    <input
                      type="text"
                      value={item.appPath}
                      placeholder={locale === "en" ? "e.g. mspaint.exe or C:\\..." : "例: mspaint.exe または フルパス"}
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
                      {locale === "en" ? '"{path}" is replaced with the image file path' : '"{path}" が実際のファイルパスに置換されます'}
                    </p>
                  </div>

                  <div>
                    <label className="block text-xs font-medium text-textSecondary mb-1">
                      {locale === "en" ? "Target Extensions (comma separated)" : "対象拡張子（カンマ区切り）"}
                    </label>
                    <input
                      type="text"
                      value={(item.targetExtensions || []).join(", ")}
                      placeholder={locale === "en" ? "e.g. png, jpg, clip (blank for all)" : "例: png, jpg, clip（空欄で全形式）"}
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
                      {locale === "en" ? "Leave blank to match all supported formats" : "空欄の場合は全フォーマットで表示されます"}
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

        {/* フッター */}
        <div className="px-6 py-3.5 border-t border-border/60 bg-surface/50 flex items-center justify-end gap-3">
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
              ? locale === "en" ? "Saving..." : "保存中..."
              : locale === "en" ? "Save Settings" : "設定を保存"}
          </button>
        </div>
      </div>
    </div>
  );
};
