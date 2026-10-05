import { describe, it, expect } from "vitest";
import packageJson from "../../../package.json";
import tauriConfig from "../../../src-tauri/tauri.conf.json";

/**
 * ビルドおよびインストーラー設定の整合性検証テスト
 *
 * npmスクリプトおよびTauri設定ファイルが、インストール用ビルド（インストーラー生成）を
 * 正常に行える構成になっているかを自動検証します。
 */
describe("ビルドおよびインストーラー設定 (BuildConfig)", () => {
  it("package.json にインストール用ビルドスクリプト (build:installer) が定義されていること", () => {
    expect(packageJson.scripts).toBeDefined();
    expect(packageJson.scripts["build:installer"]).toBe("tauri build");
  });

  it("package.json に Tauri ビルドショートカット (tauri:build) が定義されていること", () => {
    expect(packageJson.scripts).toBeDefined();
    expect(packageJson.scripts["tauri:build"]).toBe("tauri build");
  });

  it("tauri.conf.json のバンドル設定が有効でインストーラーがターゲットに含まれていること", () => {
    expect(tauriConfig.bundle).toBeDefined();
    expect(tauriConfig.bundle.active).toBe(true);
    expect(tauriConfig.bundle.targets).toBe("all");
    expect(tauriConfig.bundle.windows).toBeDefined();
    expect(tauriConfig.bundle.windows.nsis).toBeDefined();
  });
});
