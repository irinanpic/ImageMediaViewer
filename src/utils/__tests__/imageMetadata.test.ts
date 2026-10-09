import { describe, expect, it } from "vitest";
import { analyzeImageMetadata, getCompressionType, getImageFormatName } from "../imageMetadata";
import { t } from "../../locales";
import { useAppStore } from "../../store";

/**
 * 画像メタデータ解析および圧縮率・多言語化の単体テスト
 *
 * 変更理由: ユーザー指摘「Image Detailを表示すると英語設定でも『可逆圧縮』『削減』『生データ比』が表示される」
 * の不具合を根本解決し、言語切り替え（ja / en）に応じて圧縮方式や削減率が100%正しく多言語化されることを検証する。
 */
describe("imageMetadata 多言語化およびメタデータ解析テスト", () => {
  it("日本語設定時、フォーマットに応じた日本語の圧縮種別が返ること", () => {
    useAppStore.setState({ locale: "ja" });

    expect(getCompressionType("PNG", t)).toBe("可逆圧縮 (Lossless)");
    expect(getCompressionType("JPEG", t)).toBe("非可逆圧縮 (Lossy)");
    expect(getCompressionType("GIF", t)).toBe("可逆圧縮 (Lossless / 256色)");
    expect(getCompressionType("BMP", t)).toBe("非圧縮 / 可逆 (Uncompressed)");
    expect(getCompressionType("WebP", t)).toBe("非可逆 / 可逆両対応 (WebP)");
    expect(getCompressionType("AVIF", t)).toBe("非可逆 / 可逆両対応 (AVIF)");
  });

  it("英語設定時、フォーマットに応じた英語の圧縮種別が返ること", () => {
    useAppStore.setState({ locale: "en" });

    expect(getCompressionType("PNG", t)).toBe("Lossless");
    expect(getCompressionType("JPEG", t)).toBe("Lossy");
    expect(getCompressionType("GIF", t)).toBe("Lossless (256 colors)");
    expect(getCompressionType("BMP", t)).toBe("Uncompressed");
    expect(getCompressionType("WebP", t)).toBe("Lossy / Lossless (WebP)");
    expect(getCompressionType("AVIF", t)).toBe("Lossy / Lossless (AVIF)");
  });

  it("日本語設定時、圧縮率・削減率が正しく日本語フォーマットされること", () => {
    useAppStore.setState({ locale: "ja" });

    // 幅 1000, 高さ 1000, 24bit生データ = 3,000,000 Bytes (約 2.86 MB)
    // ファイルサイズ = 300,000 Bytes (生データの10% = 90%削減)
    const detail = {
      width: 1000,
      height: 1000,
      fileSize: 300000,
      format: "png",
      filePath: "/test/image.png",
    };

    const meta = analyzeImageMetadata(detail, t);
    expect(meta.formatName).toBe("PNG");
    expect(meta.compressionType).toBe("可逆圧縮 (Lossless)");
    expect(meta.compressionRatioDisplay).toContain("90.0% 削減");
    expect(meta.compressionRatioDisplay).toContain("生データ比 10.0%");
  });

  it("英語設定時、圧縮率・削減率が正しく英語フォーマットされること", () => {
    useAppStore.setState({ locale: "en" });

    const detail = {
      width: 1000,
      height: 1000,
      fileSize: 300000,
      format: "png",
      filePath: "/test/image.png",
    };

    const meta = analyzeImageMetadata(detail, t);
    expect(meta.formatName).toBe("PNG");
    expect(meta.compressionType).toBe("Lossless");
    expect(meta.compressionRatioDisplay).toContain("90.0% reduction");
    expect(meta.compressionRatioDisplay).toContain("10.0% of raw");
    // 日本語の「削減」「生データ比」「可逆圧縮」が含まれていないことを厳格に検証
    expect(meta.compressionRatioDisplay).not.toContain("削減");
    expect(meta.compressionRatioDisplay).not.toContain("生データ比");
    expect(meta.compressionType).not.toContain("可逆圧縮");
  });

  it("BMP画像（非圧縮）の多言語表示テスト", () => {
    const detail = {
      width: 800,
      height: 600,
      fileSize: 1440000,
      format: "bmp",
      filePath: "/test/sample.bmp",
    };

    useAppStore.setState({ locale: "ja" });
    const metaJa = analyzeImageMetadata(detail, t);
    expect(metaJa.compressionRatioDisplay).toBe("非圧縮 (生データ比 100%)");

    useAppStore.setState({ locale: "en" });
    const metaEn = analyzeImageMetadata(detail, t);
    expect(metaEn.compressionRatioDisplay).toBe("Uncompressed (100% of raw)");
  });

  it("画像フォーマット名の算出テスト", () => {
    expect(getImageFormatName("jpg", "test.jpg")).toBe("JPEG");
    expect(getImageFormatName(null, "photo.png")).toBe("PNG");
    expect(getImageFormatName(undefined, "icon.svg")).toBe("SVG");
  });

  it("解像度未取得時の多言語表示テスト", () => {
    const detail = {
      width: null,
      height: null,
      fileSize: 50000,
      format: "jpg",
      filePath: "/test/corrupt.jpg",
    };

    useAppStore.setState({ locale: "ja" });
    const metaJa = analyzeImageMetadata(detail, t);
    expect(metaJa.compressionRatioDisplay).toBe("解像度未取得のため計算不可");

    useAppStore.setState({ locale: "en" });
    const metaEn = analyzeImageMetadata(detail, t);
    expect(metaEn.compressionRatioDisplay).toBe("Unable to calculate (resolution unknown)");
  });
});
