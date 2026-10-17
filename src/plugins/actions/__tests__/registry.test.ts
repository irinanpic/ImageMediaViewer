import { describe, it, expect } from "vitest";
import {
  builtInActionPlugins,
  createCustomActionPlugin,
  createDynamicPluginAction,
  ContextMenuRegistry,
} from "../registry";
import type { ImageRecord } from "../../../types/generated/ImageRecord";
import type { ImageDetail } from "../../../types/generated/ImageDetail";
import type { AppSettings, CustomContextMenuItem } from "../../../types/settings";

describe("ContextMenuRegistry & Plugins", () => {
  const dummyRecord: ImageRecord = {
    id: 1,
    takenAt: 1600000000,
    width: 800,
    height: 600,
    rev: 1,
  };

  const dummyClipRecord: ImageRecord = {
    id: 2,
    takenAt: 1600000000,
    width: 1920,
    height: 1080,
    rev: 1,
  };

  const dummyClipDetail: ImageDetail = {
    id: 2,
    takenAt: 1600000000,
    width: 1920,
    height: 1080,
    rev: 1,
    folderId: 1,
    filePath: "/test/illustration.clip",
    fileSize: 2048,
    format: "clip",
    takenAtSource: "mtime",
    originalAvailable: true,
  };

  const dummyJpgDetail: ImageDetail = {
    id: 1,
    takenAt: 1600000000,
    width: 800,
    height: 600,
    rev: 1,
    folderId: 1,
    filePath: "/test/image.jpg",
    fileSize: 1024,
    format: "jpeg",
    takenAtSource: "mtime",
    originalAvailable: true,
  };

  it("CLIP STUDIO アクションプラグインは .clip ファイルのみで有効判定されること", () => {
    const clipPlugin = builtInActionPlugins.find(
      (p) => p.id === "action.open_clip_studio"
    );
    expect(clipPlugin).toBeDefined();

    const enabled = ["action.open_clip_studio"];

    // JPG では無効
    expect(clipPlugin!.isAvailable(dummyRecord, enabled, dummyJpgDetail)).toBe(
      false
    );

    // .clip では有効
    expect(
      clipPlugin!.isAvailable(dummyClipRecord, enabled, dummyClipDetail)
    ).toBe(true);

    // 有効プラグイン一覧に含まれていない場合は .clip でも無効
    expect(clipPlugin!.isAvailable(dummyClipRecord, [], dummyClipDetail)).toBe(
      false
    );
  });

  it("createCustomActionPlugin が拡張子フィルタリングを正しく処理すること", () => {
    const customItem: CustomContextMenuItem = {
      id: "ps",
      name: "Photoshopで開く",
      appPath: "/usr/bin/photoshop",
      args: "-f",
      targetExtensions: ["psd", ".clip"],
      enabled: true,
    };

    const plugin = createCustomActionPlugin(customItem, 0);
    expect(plugin.id).toBe("custom.ps");
    expect(plugin.label("ja")).toBe("Photoshopで開く");
    expect(plugin.order).toBe(100);

    // .clip はマッチする
    expect(plugin.isAvailable(dummyClipRecord, [], dummyClipDetail)).toBe(true);

    // .jpg はマッチしない
    expect(plugin.isAvailable(dummyRecord, [], dummyJpgDetail)).toBe(false);

    // targetExtensions が空の場合は全拡張子で有効
    const allPlugin = createCustomActionPlugin(
      { ...customItem, targetExtensions: [] },
      1
    );
    expect(allPlugin.isAvailable(dummyRecord, [], dummyJpgDetail)).toBe(true);

    // enabled: false の場合は無効
    const disabledPlugin = createCustomActionPlugin(
      { ...customItem, enabled: false },
      2
    );
    expect(disabledPlugin.isAvailable(dummyClipRecord, [], dummyClipDetail)).toBe(
      false
    );
  });

  it("ContextMenuRegistry.getAvailableActions が優先度順でアクションを返すこと", () => {
    const settings: AppSettings = {
      stay_in_tray: false,
      locale: "ja",
      enabled_plugins: ["action.open_default", "action.copy_path"],
      custom_context_menu_items: [
        {
          id: "custom1",
          name: "外部ビューワ",
          appPath: "/bin/viewer",
          targetExtensions: [],
          enabled: true,
        },
      ],
    };

    const actions = ContextMenuRegistry.getAvailableActions(
      dummyRecord,
      settings,
      dummyJpgDetail
    );

    const ids = actions.map((a) => a.id);
    expect(ids).toContain("action.open_default");
    expect(ids).toContain("action.copy_path");
    expect(ids).toContain("action.add_to_board"); // 常に有効
    expect(ids).toContain("custom.custom1");
    expect(ids).not.toContain("action.show_in_folder"); // enabled_plugins にない

    // 順序が昇順になっていること
    for (let i = 0; i < actions.length - 1; i++) {
      expect(actions[i].order).toBeLessThanOrEqual(actions[i + 1].order);
    }
  });

  it("同梱サンプル app_settings.sample.json および custom_action_sample.json が正常に機能すること", async () => {
    const fs = await import("node:fs");
    const path = await import("node:path");

    // 1. app_settings.sample.json の検証
    const sampleSettingsPath = path.resolve(__dirname, "../../../../plugins/app_settings.sample.json");
    expect(fs.existsSync(sampleSettingsPath)).toBe(true);

    const sampleSettingsContent = fs.readFileSync(sampleSettingsPath, "utf-8");
    const sampleSettings: AppSettings = JSON.parse(sampleSettingsContent);

    expect(sampleSettings.stay_in_tray).toBe(true);
    expect(sampleSettings.locale).toBe("ja");
    expect(sampleSettings.enabled_plugins).toContain("format.clipstudio");
    expect(sampleSettings.enabled_plugins).toContain("action.open_clip_studio");
    expect(sampleSettings.custom_context_menu_items.length).toBeGreaterThan(0);

    // PNG画像でのアクション解決検証
    const pngActions = ContextMenuRegistry.getAvailableActions(
      dummyRecord,
      sampleSettings,
      dummyJpgDetail
    );
    const pngActionIds = pngActions.map((a) => a.id);
    expect(pngActionIds).toContain("action.open_default");
    expect(pngActionIds).toContain("action.show_in_folder");
    expect(pngActionIds).toContain("action.copy_path");
    expect(pngActionIds).toContain("action.trash_file");
    // custom_context_menu_items の paint_tool_integration は enabled: true かつ png 対応
    expect(pngActionIds).toContain("custom.paint_tool_integration");
    // external_editor_integration は enabled: false
    expect(pngActionIds).not.toContain("custom.external_editor_integration");

    // CLIP画像でのアクション解決検証
    const clipActions = ContextMenuRegistry.getAvailableActions(
      dummyClipRecord,
      sampleSettings,
      dummyClipDetail
    );
    const clipActionIds = clipActions.map((a) => a.id);
    expect(clipActionIds).toContain("action.open_clip_studio");

    // 2. custom_action_sample.json の検証
    const customActionPath = path.resolve(__dirname, "../../../../plugins/action_plugins/custom_action_sample.json");
    expect(fs.existsSync(customActionPath)).toBe(true);

    const customActionContent = fs.readFileSync(customActionPath, "utf-8");
    const customItems: CustomContextMenuItem[] = JSON.parse(customActionContent);

    expect(customItems.length).toBe(3);
    const enabledCustomItems = customItems.filter((i) => i.enabled);
    expect(enabledCustomItems.length).toBe(2);

    customItems.forEach((item, index) => {
      const plugin = createCustomActionPlugin(item, index);
      expect(plugin.id).toBe(`custom.${item.id}`);
      expect(plugin.label("ja")).toBe(item.name);
    });
  });

  it("plugins/clipstudio/plugin.json のアクション定義が動的アクションプラグインとして正常にロード・実行可能であること", async () => {
    const fs = await import("node:fs");
    const path = await import("node:path");

    const clipManifestPath = path.resolve(__dirname, "../../../../plugins/clipstudio/plugin.json");
    expect(fs.existsSync(clipManifestPath)).toBe(true);

    const clipManifest = JSON.parse(fs.readFileSync(clipManifestPath, "utf-8"));
    expect(clipManifest.id).toBe("format.clipstudio");
    expect(clipManifest.actions.length).toBeGreaterThan(0);

    const actionDef = clipManifest.actions[0];
    const dynamicAction = createDynamicPluginAction(actionDef);

    expect(dynamicAction.id).toBe("action.open_clip_studio");
    expect(dynamicAction.label("ja")).toBe("CLIP STUDIO PAINTで開く");
    expect(dynamicAction.label("en")).toBe("Open with CLIP STUDIO PAINT");

    // .clip では有効（完全一致、format.* 前方一致、* 全一致）
    expect(dynamicAction.isAvailable(dummyClipRecord, ["format.clipstudio"], dummyClipDetail)).toBe(true);
    expect(dynamicAction.isAvailable(dummyClipRecord, ["format.*"], dummyClipDetail)).toBe(true);
    expect(dynamicAction.isAvailable(dummyClipRecord, ["*"], dummyClipDetail)).toBe(true);
    // .jpg では無効（拡張子フィルタにより除外）
    expect(dynamicAction.isAvailable(dummyRecord, ["format.clipstudio"], dummyJpgDetail)).toBe(false);

    // ContextMenuRegistry.loadPluginActions の統合テスト
    ContextMenuRegistry.loadPluginActions(clipManifest.actions);
    const available = ContextMenuRegistry.getAvailableActions(
      dummyClipRecord,
      { stay_in_tray: true, locale: "ja", enabled_plugins: ["format.clipstudio"], custom_context_menu_items: [] },
      dummyClipDetail
    );
    expect(available.some((a) => a.id === "action.open_clip_studio")).toBe(true);

    const availableWildcard = ContextMenuRegistry.getAvailableActions(
      dummyClipRecord,
      { stay_in_tray: true, locale: "ja", enabled_plugins: ["format.*"], custom_context_menu_items: [] },
      dummyClipDetail
    );
    expect(availableWildcard.some((a) => a.id === "action.open_clip_studio")).toBe(true);
  });
});

