import { describe, it, expect } from "vitest";
import {
  builtInActionPlugins,
  createCustomActionPlugin,
  createDynamicPluginAction,
  ContextMenuRegistry,
  isPluginIdEnabled,
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

  it("本体 builtInActionPlugins には特定外部ソフト（ClipStudio等）がハードコードされていないこと", () => {
    const clipInBuiltIn = builtInActionPlugins.find(
      (p) => p.id === "action.open_clip_studio"
    );
    expect(clipInBuiltIn).toBeUndefined();
  });

  it("isPluginIdEnabled がワイルドカードおよび否定パターン(!id)を正しく判定すること", () => {
    expect(isPluginIdEnabled("format.clipstudio", ["*"])).toBe(true);
    expect(isPluginIdEnabled("format.clipstudio", ["format.*"])).toBe(true);
    expect(
      isPluginIdEnabled("format.clipstudio", ["*", "!format.clipstudio"])
    ).toBe(false);
    expect(isPluginIdEnabled("action.open_default", ["format.*"])).toBe(false);
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

  it("plugins-src/clipstudio/plugin.json のアクション定義が動的アクションプラグインとして正常にロード・実行可能であること", async () => {
    const fs = await import("node:fs");
    const path = await import("node:path");

    const clipManifestPath = path.resolve(
      __dirname,
      "../../../../plugins-src/clipstudio/plugin.json"
    );
    expect(fs.existsSync(clipManifestPath)).toBe(true);

    const clipManifest = JSON.parse(fs.readFileSync(clipManifestPath, "utf-8"));
    expect(clipManifest.id).toBe("format.clipstudio");
    expect(clipManifest.actions.length).toBeGreaterThan(0);

    const actionDef = {
      ...clipManifest.actions[0],
      pluginId: clipManifest.id,
    };
    const dynamicAction = createDynamicPluginAction(actionDef);

    expect(dynamicAction.id).toBe("action.open_clip_studio");
    expect(dynamicAction.label("ja")).toBe("CLIP STUDIO PAINTで開く");
    expect(dynamicAction.label("en")).toBe("Open with CLIP STUDIO PAINT");

    // .clip では有効（完全一致、format.* 前方一致、* 全一致）
    expect(
      dynamicAction.isAvailable(dummyClipRecord, ["format.clipstudio"], dummyClipDetail)
    ).toBe(true);
    expect(
      dynamicAction.isAvailable(dummyClipRecord, ["format.*"], dummyClipDetail)
    ).toBe(true);
    expect(
      dynamicAction.isAvailable(dummyClipRecord, ["*"], dummyClipDetail)
    ).toBe(true);

    // 親プラグインが "!format.clipstudio" で明示的に無効化されている場合は無効
    expect(
      dynamicAction.isAvailable(
        dummyClipRecord,
        ["*", "!format.clipstudio"],
        dummyClipDetail
      )
    ).toBe(false);

    // .jpg では無効（拡張子フィルタにより除外）
    expect(
      dynamicAction.isAvailable(dummyRecord, ["format.clipstudio"], dummyJpgDetail)
    ).toBe(false);

    // ContextMenuRegistry.loadPluginActions の統合テスト
    ContextMenuRegistry.loadPluginActions([actionDef]);
    const available = ContextMenuRegistry.getAvailableActions(
      dummyClipRecord,
      {
        stay_in_tray: true,
        locale: "ja",
        enabled_plugins: ["format.clipstudio"],
        custom_context_menu_items: [],
      },
      dummyClipDetail
    );
    expect(available.some((a) => a.id === "action.open_clip_studio")).toBe(true);

    const availableWildcard = ContextMenuRegistry.getAvailableActions(
      dummyClipRecord,
      {
        stay_in_tray: true,
        locale: "ja",
        enabled_plugins: ["format.*"],
        custom_context_menu_items: [],
      },
      dummyClipDetail
    );
    expect(availableWildcard.some((a) => a.id === "action.open_clip_studio")).toBe(
      true
    );
  });

  it("action.trash_file が確認ダイアログキャンセル時に削除せず、承認時にビューアと選択状態を同期クリーンアップすること", async () => {
    const { vi } = await import("vitest");
    const { useAppStore } = await import("../../../store");
    const { backendApi } = await import("../../../lib/ipc");

    const trashAction = builtInActionPlugins.find((p) => p.id === "action.trash_file")!;
    expect(trashAction).toBeDefined();

    const trashSpy = vi.spyOn(backendApi, "trashFile").mockResolvedValue(undefined);
    const hadWindow = typeof globalThis.window !== "undefined";
    if (!hadWindow) {
      (globalThis as unknown as { window: Record<string, unknown> }).window = {};
    }
    const origConfirm = window.confirm;

    try {
      // 1. キャンセル時は trashFile が呼ばれないこと
      window.confirm = vi.fn().mockReturnValue(false);
      useAppStore.setState({
        isViewerOpen: true,
        activeImageId: 1,
        selectedImageIds: [1, 2],
      });

      await trashAction.execute({ record: dummyRecord, detail: dummyJpgDetail });
      expect(trashSpy).not.toHaveBeenCalled();
      expect(useAppStore.getState().isViewerOpen).toBe(true);
      expect(useAppStore.getState().selectedImageIds).toEqual([1, 2]);

      // 2. 承認時は trashFile が呼ばれ、activeImageId が一致していればビューアを閉じ、selectedImageIds から除外されること
      window.confirm = vi.fn().mockReturnValue(true);
      await trashAction.execute({ record: dummyRecord, detail: dummyJpgDetail });
      expect(trashSpy).toHaveBeenCalledWith(1);
      expect(useAppStore.getState().isViewerOpen).toBe(false);
      expect(useAppStore.getState().activeImageId).toBeNull();
      expect(useAppStore.getState().selectedImageIds).toEqual([2]);
    } finally {
      if (hadWindow) {
        window.confirm = origConfirm;
      } else {
        delete (globalThis as unknown as { window?: unknown }).window;
      }
      trashSpy.mockRestore();
    }
  });
});
