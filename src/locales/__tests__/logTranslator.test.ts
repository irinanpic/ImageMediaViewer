import { describe, expect, it } from "vitest";
import { t } from "../index";
import { translateLogMessage } from "../logTranslator";

describe("translateLogMessage", () => {
  it("日本語ログを英語モード時に英語へ正しく翻訳する", () => {
    const rawJa = "言語設定を更新しました: locale = en";
    const translated = translateLogMessage(rawJa, (key, params) => t(key, params, "en"));
    expect(translated).toBe("Language updated: locale = en");
  });

  it("英語ログを日本語モード時に日本語へ正しく翻訳する", () => {
    const rawEn = "Language updated: locale = en";
    const translated = translateLogMessage(rawEn, (key, params) => t(key, params, "ja"));
    expect(translated).toBe("言語設定を更新しました: locale = en");
  });

  it("二重起動検知メッセージが日英両方で正しく変換される", () => {
    const rawJa = "二重起動を検知しました。既存のメインウィンドウを表示・復元します。";
    const translatedEn = translateLogMessage(rawJa, (key, params) => t(key, params, "en"));
    expect(translatedEn).toBe("Detected second instance: Restoring existing window to front");

    const translatedJa = translateLogMessage(rawJa, (key, params) => t(key, params, "ja"));
    expect(translatedJa).toBe("二重起動を検知しました。既存のメインウィンドウを表示・復元します。");
  });

  it("差分対象画像発見メッセージのフォルダ名と件数を抽出して正しく置換する", () => {
    const rawJa = "フォルダ [D:\\docs\\download] で差分対象画像 4 件を発見";
    const translatedEn = translateLogMessage(rawJa, (key, params) => t(key, params, "en"));
    expect(translatedEn).toBe("Found 4 changed images in [D:\\docs\\download]");

    const rawEn = "Found 4 changed images in [D:\\docs\\download]";
    const translatedJa = translateLogMessage(rawEn, (key, params) => t(key, params, "ja"));
    expect(translatedJa).toBe("フォルダ [D:\\docs\\download] で差分対象画像 4 件を発見");
  });

  it("未知のログメッセージはそのまま透過的に返却する", () => {
    const customLog = "Custom debug log with stacktrace: error at line 42";
    const translated = translateLogMessage(customLog, (key, params) => t(key, params, "en"));
    expect(translated).toBe(customLog);
  });
});
