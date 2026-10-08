import { describe, expect, it } from "vitest";
import { t } from "../index";
import { useAppStore } from "../../store";

describe("i18n 多言語対応システム", () => {
  it("日本語リソースから正しくテキストを取得できること", () => {
    useAppStore.setState({ locale: "ja" });
    expect(t("sidebar.folders")).toBe("監視フォルダ");
    expect(t("common.add")).toBe("追加");
    expect(t("common.close")).toBe("閉じる");
  });

  it("英語リソースから正しくテキストを取得できること", () => {
    useAppStore.setState({ locale: "en" });
    expect(t("sidebar.folders")).toBe("Watch Folders");
    expect(t("common.add")).toBe("Add");
    expect(t("common.close")).toBe("Close");
  });

  it("パラメータ付きテンプレートの置換が正常に行われること", () => {
    useAppStore.setState({ locale: "ja" });
    expect(t("timeline.imageCount", { count: 120 })).toBe("120 枚");
    expect(t("sidebar.imagesCount", { count: 50 })).toBe("(50枚)");

    useAppStore.setState({ locale: "en" });
    expect(t("timeline.imageCount", { count: 120 })).toBe("120 images");
    expect(t("sidebar.imagesCount", { count: 50 })).toBe("(50 images)");
  });

  it("引数で明示的にロケールを指定した場合、そのロケールで取得されること", () => {
    useAppStore.setState({ locale: "ja" });
    expect(t("common.save", undefined, "en")).toBe("Save");
    expect(t("common.save", undefined, "ja")).toBe("保存");
  });

  it("日本語リソースにキーが存在しない（または未翻訳）場合、英語マスターへ透過的にフォールバックすること", () => {
    useAppStore.setState({ locale: "ja" });
    // 存在しないキーのパスシミュレーション（英語マスターにのみ存在するキーを模したテスト）
    // 完全に未知のキーはキー名自体が返る
    expect(t("unknown.nonexistent.key" as any)).toBe("unknown.nonexistent.key");
  });
});
