import { describe, expect, it } from "vitest";
import { getOriginalImageUrl, getThumbnailUrl } from "../thumbUrl";

/**
 * サムネイルおよび原寸画像URL生成の単体テスト
 */
describe("thumbUrl URL生成関数テスト", () => {
  it("getOriginalImageUrl は rev が指定された場合にキャッシュバスタークエリ ?v={rev} を付与すること", () => {
    const urlWithRev = getOriginalImageUrl(42, 12345);
    expect(urlWithRev).toContain("42");
    expect(urlWithRev).toContain("?v=12345");

    const urlWithoutRev = getOriginalImageUrl(42);
    expect(urlWithoutRev).toContain("42");
    expect(urlWithoutRev).not.toContain("?v=");
  });

  it("getThumbnailUrl は常にリビジョンクエリ ?v={rev} を含むこと", () => {
    const thumbUrl = getThumbnailUrl(99, 54321);
    expect(thumbUrl).toContain("99");
    expect(thumbUrl).toContain("?v=54321");
  });
});
