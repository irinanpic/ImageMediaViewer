import type { ContextMenuActionPlugin } from "./types";
import type { ImageRecord } from "../../types/generated/ImageRecord";
import type { ImageDetail } from "../../types/generated/ImageDetail";
import type { AppSettings, CustomContextMenuItem } from "../../types/settings";
import type { PluginActionDefinition } from "../../types/generated/PluginActionDefinition";
import { backendApi } from "../../lib/ipc";

/**
 * 組み込み標準アクションプラグイン群
 *
 * 変更理由: 仕様書§2.2(F-12)および右クリックメニュー標準機能の実装。
 */
export const builtInActionPlugins: ContextMenuActionPlugin[] = [
  {
    id: "action.open_default",
    label: (locale) => (locale === "en" ? "Open with Default App" : "既定のアプリで開く"),
    icon: "ExternalLink",
    order: 10,
    isAvailable: (_record, enabledPlugins) => enabledPlugins.includes("action.open_default"),
    execute: async (context) => {
      await backendApi.openFileWithDefaultApp({ id: context.record.id });
    },
  },
  {
    id: "action.open_clip_studio",
    label: (locale) => (locale === "en" ? "Open with CLIP STUDIO PAINT" : "CLIP STUDIO PAINTで開く"),
    icon: "Palette",
    order: 20,
    isAvailable: (_record, enabledPlugins, detail) => {
      if (!enabledPlugins.includes("action.open_clip_studio")) return false;
      if (!detail) return false;
      const isClip =
        detail.format?.toLowerCase() === "clip" ||
        detail.filePath.toLowerCase().endsWith(".clip");
      return isClip;
    },
    execute: async (context) => {
      await backendApi.openInClipStudio({ id: context.record.id });
    },
  },
  {
    id: "action.show_in_folder",
    label: (locale) => (locale === "en" ? "Reveal in File Manager" : "フォルダで表示"),
    icon: "FolderOpen",
    order: 30,
    isAvailable: (_record, enabledPlugins) => enabledPlugins.includes("action.show_in_folder"),
    execute: async (context) => {
      await backendApi.showInFolder({ id: context.record.id });
    },
  },
  {
    id: "action.copy_path",
    label: (locale) => (locale === "en" ? "Copy File Path" : "ファイルパスをコピー"),
    icon: "Copy",
    order: 40,
    isAvailable: (_record, enabledPlugins) => enabledPlugins.includes("action.copy_path"),
    execute: async (context) => {
      let path = context.detail?.filePath;
      if (!path) {
        const detail = await backendApi.getImageDetail(context.record.id);
        path = detail.filePath;
      }
      if (path && navigator.clipboard) {
        await navigator.clipboard.writeText(path);
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
    isAvailable: (_record, enabledPlugins) => enabledPlugins.includes("action.trash_file"),
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
      // 1. プラグインIDまたはアクションIDが有効化されているか（完全一致、"*"、"prefix*" ワイルドカード）
      const defIdNormalized = def.id.toLowerCase().replace(/[^a-z0-9]/g, "");
      const isEnabled =
        enabledPlugins.includes("*") ||
        enabledPlugins.includes(def.id) ||
        enabledPlugins.some((p) => {
          if (p === "*") return true;
          if (p.endsWith("*")) {
            const prefix = p.slice(0, -1);
            if (def.id.startsWith(prefix)) return true;
            const pNormalized = prefix.toLowerCase().replace(/^format/, "").replace(/[^a-z0-9]/g, "");
            if (pNormalized.length === 0) {
              // "format.*" の場合、フォーマットプラグイン由来のアクション（action.open_xxx等）も有効とする
              return true;
            }
            return defIdNormalized.includes(pNormalized);
          }
          const pNormalized = p.toLowerCase().replace(/^format/, "").replace(/[^a-z0-9]/g, "");
          return pNormalized.length > 0 && defIdNormalized.includes(pNormalized);
        });
      if (!isEnabled) return false;

      // 2. 対象拡張子の判定
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
      "format.clipstudio",
      "format.*",
      "action.open_default",
      "action.open_clip_studio",
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
