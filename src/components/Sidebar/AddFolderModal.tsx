import React, { useState, useEffect } from "react";
import { useTranslation } from "../../locales";
import { backendApi } from "../../lib/ipc";
import type { PresetFolder } from "../../types/generated/PresetFolder";

interface AddFolderModalProps {
  /** モーダルが開いているかどうか */
  isOpen: boolean;
  /** モーダルを閉じるハンドラ */
  onClose: () => void;
  /** フォルダパスが選択または入力された際のコールバック */
  onSelectPath: (path: string) => Promise<void>;
}

/**
 * モバイル向けフォルダ追加モーダルコンポーネント
 *
 * 変更理由: Android 等のモバイル環境において、ネイティブのフォルダ選択ダイアログ
 * （Folder picker is not implemented on mobile）が利用できない制約を解消するため。
 * 端末内の標準画像保存先（DCIM/Camera, Pictures等）をプリセットとしてワンタップで追加可能にし、
 * SDカードや任意の特定フォルダパスも直接入力して追加できるようにする。
 */
export const AddFolderModal: React.FC<AddFolderModalProps> = ({
  isOpen,
  onClose,
  onSelectPath,
}) => {
  const { t } = useTranslation();
  const [presets, setPresets] = useState<PresetFolder[]>([]);
  const [customPath, setCustomPath] = useState("");
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // プラットフォーム推奨プリセットの取得
  useEffect(() => {
    if (!isOpen) return;
    setError(null);
    backendApi.getPlatformInfo()
      .then((info) => {
        if (info.presets && info.presets.length > 0) {
          setPresets(info.presets);
        } else {
          // デフォルトのAndroid標準候補
          setPresets([
            { name: "📷 カメラ (Camera)", path: "/storage/emulated/0/DCIM/Camera", exists: true },
            { name: "📁 DCIM 全体", path: "/storage/emulated/0/DCIM", exists: true },
            { name: "🖼️ ピクチャ (Pictures)", path: "/storage/emulated/0/Pictures", exists: true },
            { name: "📥 ダウンロード (Download)", path: "/storage/emulated/0/Download", exists: true },
            { name: "💾 内部ストレージ", path: "/storage/emulated/0", exists: true },
          ]);
        }
      })
      .catch(() => {
        setPresets([
          { name: "📷 カメラ (Camera)", path: "/storage/emulated/0/DCIM/Camera", exists: true },
          { name: "📁 DCIM 全体", path: "/storage/emulated/0/DCIM", exists: true },
          { name: "🖼️ ピクチャ (Pictures)", path: "/storage/emulated/0/Pictures", exists: true },
          { name: "📥 ダウンロード (Download)", path: "/storage/emulated/0/Download", exists: true },
        ]);
      });
  }, [isOpen]);

  if (!isOpen) return null;

  // プリセットクリック時の処理
  const handleSelectPreset = async (path: string) => {
    try {
      setIsSubmitting(true);
      setError(null);
      await onSelectPath(path);
      onClose();
    } catch (err: any) {
      setError(err?.message || String(err));
    } finally {
      setIsSubmitting(false);
    }
  };

  // 手動入力パス追加時の処理
  const handleCustomSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    const trimmed = customPath.trim();
    if (!trimmed) return;
    try {
      setIsSubmitting(true);
      setError(null);
      await onSelectPath(trimmed);
      setCustomPath("");
      onClose();
    } catch (err: any) {
      setError(err?.message || String(err));
    } finally {
      setIsSubmitting(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/60 backdrop-blur-sm animate-fadeIn">
      <div
        className="w-full max-w-lg bg-neutral-900 border border-neutral-800 rounded-xl shadow-2xl flex flex-col max-h-[90vh] overflow-hidden"
        onClick={(e) => e.stopPropagation()}
      >
        {/* モーダルヘッダー */}
        <div className="flex items-center justify-between px-5 py-4 border-b border-neutral-800">
          <div className="flex items-center gap-2">
            <span className="text-xl">📂</span>
            <h3 className="font-semibold text-neutral-100 text-base">
              {t("sidebar.mobileAddFolderTitle")}
            </h3>
          </div>
          <button
            onClick={onClose}
            className="p-1 rounded-lg text-neutral-400 hover:text-neutral-200 hover:bg-neutral-800 transition-colors"
            title={t("common.close")}
          >
            ✕
          </button>
        </div>

        {/* モーダル本体 */}
        <div className="p-5 overflow-y-auto space-y-5">
          {error && (
            <div className="p-3 text-xs bg-red-950/60 border border-red-800/80 text-red-200 rounded-lg">
              {error}
            </div>
          )}

          {/* プリセット候補一覧 */}
          <div>
            <div className="text-xs font-semibold text-neutral-400 uppercase tracking-wider mb-2.5">
              {t("sidebar.mobilePresetsTitle")}
            </div>
            <div className="space-y-2">
              {presets.map((preset) => (
                <button
                  key={preset.path}
                  disabled={isSubmitting}
                  onClick={() => handleSelectPreset(preset.path)}
                  className="w-full text-left p-3 rounded-lg border border-neutral-800 bg-neutral-800/40 hover:bg-neutral-800 hover:border-neutral-700 transition-all flex items-center justify-between group active:scale-[0.99] disabled:opacity-50"
                >
                  <div className="min-w-0 pr-3">
                    <div className="font-medium text-sm text-neutral-200 group-hover:text-white flex items-center gap-2">
                      <span>{preset.name}</span>
                    </div>
                    <div className="text-xs text-neutral-400 truncate mt-0.5 font-mono">
                      {preset.path}
                    </div>
                  </div>
                  <span className="shrink-0 text-xs px-2 py-0.5 rounded border border-indigo-500/30 bg-indigo-500/10 text-indigo-400 font-medium">
                    {t("common.add")}
                  </span>
                </button>
              ))}
            </div>
          </div>

          {/* 手動パス入力フォーム */}
          <div className="pt-2 border-t border-neutral-800/80">
            <div className="text-xs font-semibold text-neutral-400 uppercase tracking-wider mb-2.5">
              {t("sidebar.mobileCustomPathTitle")}
            </div>
            <form onSubmit={handleCustomSubmit} className="flex gap-2">
              <input
                type="text"
                value={customPath}
                onChange={(e) => setCustomPath(e.target.value)}
                placeholder={t("sidebar.mobileCustomPathPlaceholder")}
                disabled={isSubmitting}
                className="flex-1 px-3 py-2 text-sm bg-neutral-950 border border-neutral-800 rounded-lg text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500"
              />
              <button
                type="submit"
                disabled={isSubmitting || !customPath.trim()}
                className="px-4 py-2 text-sm font-medium bg-indigo-600 text-white rounded-lg hover:bg-indigo-500 disabled:opacity-50 transition-colors shrink-0"
              >
                {t("common.add")}
              </button>
            </form>
          </div>

          {/* 権限に関する注釈 */}
          <div className="p-3 bg-neutral-800/30 border border-neutral-800/50 rounded-lg">
            <p className="text-xs text-neutral-400 leading-relaxed">
              {t("sidebar.mobilePermissionNotice")}
            </p>
          </div>
        </div>

        {/* モーダルフッター */}
        <div className="px-5 py-3 border-t border-neutral-800 bg-neutral-900/50 flex justify-end">
          <button
            onClick={onClose}
            className="px-4 py-2 text-xs font-medium text-neutral-300 hover:text-white bg-neutral-800 hover:bg-neutral-700 rounded-lg transition-colors"
          >
            {t("common.close")}
          </button>
        </div>
      </div>
    </div>
  );
};
