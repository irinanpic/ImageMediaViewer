import type { ContextMenuActionPlugin } from "./types";
import type { ImageRecord } from "../../types/generated/ImageRecord";
import type { ImageDetail } from "../../types/generated/ImageDetail";
import type { AppSettings, CustomContextMenuItem } from "../../types/settings";
import type { PluginActionDefinition } from "../../types/generated/PluginActionDefinition";
import { backendApi } from "../../lib/ipc";

/**
 * 指定されたプラグインIDまたはアクションIDが有効化パターンリストに合致するか判定する
 *
 * 変更理由: 特定プラグイン名への依存や複雑な正規表現によるヒューリスティック判定を廃止し、
 * 完全一致、ワイルドカード（"*", "prefix*"）、および明示的無効化（"!id"）のみでクリーンに判定するため。
 *
 * @param id 判定対象のプラグインIDまたはアクションID
 * @param enabledPlugins 有効化プラグインパターン配列
 * @returns 有効であれば true
 */
export function isPluginIdEnabled(id: string, enabledPlugins: string[]): boolean {
  if (enabledPlugins.includes(`!${id}`)) {
    return false;
  }

  return enabledPlugins.some((pattern) => {
    if (pattern.startsWith("!")) return false;
    if (pattern === "*" || pattern === id) return true;
    if (pattern.endsWith("*")) {
      const prefix = pattern.slice(0, -1);
      return id.startsWith(prefix);
    }
    return false;
  });
}

/**
 * 組み込み標準アクションプラグイン群
 *
 * 変更理由: 仕様書§2.2(F-12)および右クリックメニュー標準機能の実装。
 * 外部プラグイン固有のアクション（CLIP STUDIO PAINT等）は本体にハードコードせず、
 * すべて plugin.json からの動的登録（createDynamicPluginAction）に一元化する。
 */
export const builtInActionPlugins: ContextMenuActionPlugin[] = [
  {
    id: "action.open_default",
    label: (locale) => (locale === "en" ? "Open with Default App" : "既定のアプリで開く"),
    icon: "ExternalLink",
    order: 10,
    isAvailable: (_record, enabledPlugins) =>
      isPluginIdEnabled("action.open_default", enabledPlugins),
    execute: async (context) => {
      await backendApi.openFileWithDefaultApp({ id: context.record.id });
    },
  },
  {
    id: "action.show_in_folder",
    label: (locale) => (locale === "en" ? "Reveal in File Manager" : "フォルダで表示"),
    icon: "FolderOpen",
    order: 30,
    isAvailable: (_record, enabledPlugins) =>
      isPluginIdEnabled("action.show_in_folder", enabledPlugins),
    execute: async (context) => {
      await backendApi.showInFolder({ id: context.record.id });
    },
  },
  {
    id: "action.copy_path",
    label: (locale) => (locale === "en" ? "Copy File Path" : "ファイルパスをコピー"),
    icon: "Copy",
    order: 40,
    isAvailable: (_record, enabledPlugins) =>
      isPluginIdEnabled("action.copy_path", enabledPlugins),
    execute: async (context) => {
      let path = context.detail?.filePath;
      if (!path) {
        const detail = await backendApi.getImageDetail(context.record.id);
        path = detail.filePath;
      }
      if (path && navigator.clipboard) {
        await navigator.clipboard.writeText(path);
        if (typeof navigator !== "undefined" && navigator.vibrate) {
          try {
            navigator.vibrate(30);
          } catch {}
        }
        if (typeof document !== "undefined") {
          const existing = document.getElementById("imv-copy-toast");
          if (existing) existing.remove();
          const toast = document.createElement("div");
          toast.id = "imv-copy-toast";
          toast.className =
            "fixed bottom-6 left-1/2 -translate-x-1/2 z-[9999] bg-surface/95 text-textPrimary px-4 py-2 rounded-lg shadow-2xl border border-accent/60 text-xs font-medium flex items-center gap-2 pointer-events-none transition-all duration-300 animate-in fade-in";
          const fileName = path.split(/[\\/]/).pop() || path;
          toast.textContent = `📋 パスをコピーしました: ${fileName}`;
          document.body.appendChild(toast);
          setTimeout(() => {
            toast.style.opacity = "0";
            setTimeout(() => toast.remove(), 300);
          }, 2000);
        }
      }
    },
  },
  {
    id: "action.add_to_board",
    label: (locale) => (locale === "en" ? "Add to Moodboard..." : "ボードに追加..."),
    icon: "LayoutGrid",
    order: 50,
    isAvailable: () => true,
    execute: async (context) => {
      if (context.openAddToBoardModal) {
        context.openAddToBoardModal([context.record.id]);
      }
    },
  },
  {
    id: "action.trash_file",
    label: (locale) => (locale === "en" ? "Move to Trash" : "ゴミ箱へ移動"),
    icon: "Trash2",
    order: 90,
    isAvailable: (_record, enabledPlugins) =>
      isPluginIdEnabled("action.trash_file", enabledPlugins),
    execute: async (context) => {
      await backendApi.trashFile(context.record.id);
      if (context.refreshTimeline) {
        context.refreshTimeline();
      }
    },
  },
];

/**
 * ユーザー定義のカスタムメニュー設定項目から動的プラグインを生成する
 *
 * @param item カスタムメニュー設定項目
 * @param index インデックス
 * @return ContextMenuActionPlugin
 */
export function createCustomActionPlugin(
  item: CustomContextMenuItem,
  index: number
): ContextMenuActionPlugin {
  return {
    id: `custom.${item.id}`,
    label: () => item.name,
    icon: "Play",
    order: 100 + index,
    isAvailable: (_record, _enabledPlugins, detail) => {
      if (!item.enabled) return false;
      if (!item.targetExtensions || item.targetExtensions.length === 0) return true;
      if (!detail) return true; // detail取得前は表示許可

      const ext =
        detail.filePath.split(".").pop()?.toLowerCase() ||
        detail.format?.toLowerCase() ||
        "";
      return item.targetExtensions
        .map((e) => e.toLowerCase().trim().replace(/^\./, ""))
        .includes(ext);
    },
    execute: async (context) => {
      let filePath = context.detail?.filePath;
      if (!filePath) {
        const detail = await backendApi.getImageDetail(context.record.id);
        filePath = detail.filePath;
      }
      const args = item.args ? [item.args] : undefined;
      await backendApi.openFileWithApp({
        appPath: item.appPath,
        filePath,
        args,
      });
    },
  };
}

/**
 * プラグインマニフェストのアクション定義から動的コンテキストメニュープラグインを生成する
 *
 * 変更理由: plugin.json に記載されたアクションを動的にコンテキストメニューに登録可能にするため。
 * バックエンドが PluginActionDefinition に付与する親 pluginId を参照することで、
 * 複雑な文字列正規化を行うことなく正確に有効/無効判定を行う。
 *
 * @param def プラグインアクション定義
 * @return ContextMenuActionPlugin
 */
export function createDynamicPluginAction(
  def: PluginActionDefinition
): ContextMenuActionPlugin {
  const englishName = def.nameEn ?? (def as unknown as { name_en?: string | null }).name_en;
  return {
    id: def.id,
    label: (locale) => (locale === "en" && englishName ? englishName : def.name),
    icon: def.icon ?? "Play",
    order: def.order ?? 25,
    isAvailable: (_record, enabledPlugins, detail) => {
      // 1. 明示的にアクションまたは親プラグインが無効化("!id")されている場合は false
      if (enabledPlugins.includes(`!${def.id}`)) return false;
      if (def.pluginId && enabledPlugins.includes(`!${def.pluginId}`)) return false;

      // 2. アクションID、親プラグインID、またはフォーマットプラグイン一括指定("format.*")で有効か判定
      const isActionEnabled = isPluginIdEnabled(def.id, enabledPlugins);
      const isParentPluginEnabled = def.pluginId
        ? isPluginIdEnabled(def.pluginId, enabledPlugins)
        : isPluginIdEnabled("format.*", enabledPlugins);

      if (!isActionEnabled && !isParentPluginEnabled) return false;

      // 3. 対象拡張子の判定
      if (!def.extensions || def.extensions.length === 0) return true;
      if (!detail) return false;

      const ext =
        detail.filePath.split(".").pop()?.toLowerCase() ||
        detail.format?.toLowerCase() ||
        "";
      return def.extensions
        .map((e) => e.toLowerCase().trim().replace(/^\./, ""))
        .includes(ext);
    },
    execute: async (context) => {
      let filePath = context.detail?.filePath;
      if (!filePath) {
        const detail = await backendApi.getImageDetail(context.record.id);
        filePath = detail.filePath;
      }
      await backendApi.executePluginAction({
        actionId: def.id,
        id: context.record.id,
        path: filePath,
      });
    },
  };
}

/**
 * コンテキストメニューアクションプラグインレジストリ
 *
 * 変更理由: 設定ファイル（app_settings.json）で有効化されたプラグインおよび
 * 登録されたカスタム項目のみを動的に抽出し、安全に実行可能にするため。
 */
export class ContextMenuRegistry {
  private static plugins: ContextMenuActionPlugin[] = [...builtInActionPlugins];
  private static dynamicPluginActions: ContextMenuActionPlugin[] = [];

  /**
   * プラグインマニフェストから読み込んだアクション定義群を動的登録する
   *
   * @param defs プラグインアクション定義配列
   */
  public static loadPluginActions(defs: PluginActionDefinition[]): void {
    this.dynamicPluginActions = defs.map(createDynamicPluginAction);
  }

  /**
   * 追加のアクションプラグインを手動登録する
   *
   * @param plugin 追加プラグイン
   */
  public static register(plugin: ContextMenuActionPlugin): void {
    if (!this.plugins.some((p) => p.id === plugin.id)) {
      this.plugins.push(plugin);
    }
  }

  /**
   * 現在のコンテキストおよび設定に基づいて、表示可能なアクション一覧を取得する
   *
   * @param record 対象画像レコード
   * @param appSettings アプリケーション設定（有効プラグイン一覧・カスタム項目含む）
   * @param detail 対象画像の詳細情報（オプション）
   * @return ソート済みの利用可能アクションリスト
   */
  public static getAvailableActions(
    record: ImageRecord,
    appSettings: AppSettings | null,
    detail?: ImageDetail
  ): ContextMenuActionPlugin[] {
    const enabledPlugins = appSettings?.enabled_plugins ?? [
      "*",
      "format.*",
      "action.open_default",
      "action.show_in_folder",
      "action.copy_path",
      "action.trash_file",
    ];

    // 1. 標準組み込みプラグインから利用可能なものを抽出
    const available = this.plugins.filter((p) =>
      p.isAvailable(record, enabledPlugins, detail)
    );

    // 2. マニフェストから動的に読み込まれたアクションプラグインを追加
    for (const action of this.dynamicPluginActions) {
      if (
        !available.some((a) => a.id === action.id) &&
        action.isAvailable(record, enabledPlugins, detail)
      ) {
        available.push(action);
      }
    }

    // 3. カスタムメニュー項目を動的プラグイン化して追加
    if (appSettings?.custom_context_menu_items) {
      appSettings.custom_context_menu_items.forEach((item, idx) => {
        const customPlugin = createCustomActionPlugin(item, idx);
        if (customPlugin.isAvailable(record, enabledPlugins, detail)) {
          available.push(customPlugin);
        }
      });
    }

    // 4. order 順に昇順ソート
    return available.sort((a, b) => a.order - b.order);
  }
}
