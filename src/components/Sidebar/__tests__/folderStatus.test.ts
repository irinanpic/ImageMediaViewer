import { describe, expect, it } from "vitest";
import { getAllPhotosThumbnailState, getFolderThumbnailState } from "../folderStatus";

describe("folderStatus determination", () => {
  describe("getFolderThumbnailState", () => {
    it("画像枚数が0の場合は empty を返すこと", () => {
      expect(getFolderThumbnailState(0, 0, 0)).toBe("empty");
      expect(getFolderThumbnailState(0)).toBe("empty");
    });

    it("処理済み件数が画像枚数未満の場合は generating を返すこと", () => {
      // 成功2件、失敗0件、合計10件 -> generating
      expect(getFolderThumbnailState(10, 2, 0)).toBe("generating");
      // 成功2件、失敗1件、合計10件 -> generating
      expect(getFolderThumbnailState(10, 2, 1)).toBe("generating");
      // 未定義の場合も generating
      expect(getFolderThumbnailState(5)).toBe("generating");
    });

    it("全件成功した場合は allDone を返すこと", () => {
      expect(getFolderThumbnailState(10, 10, 0)).toBe("allDone");
      expect(getFolderThumbnailState(5, 5)).toBe("allDone");
    });

    it("パスワード付きZIPや0バイト等で失敗が含まれる場合、生成中にならず warningDone を返すこと", () => {
      // 合計10件のうち成功9件、失敗1件（パスワード保護等） -> 処理完了だが警告
      expect(getFolderThumbnailState(10, 9, 1)).toBe("warningDone");
      // 全て失敗した場合 -> 処理完了だが警告
      expect(getFolderThumbnailState(5, 0, 5)).toBe("warningDone");
    });
  });

  describe("getAllPhotosThumbnailState", () => {
    it("総画像数が0または進捗情報がない場合は empty を返すこと", () => {
      expect(getAllPhotosThumbnailState(0, null)).toBe("empty");
      expect(getAllPhotosThumbnailState(10, null)).toBe("empty");
      expect(getAllPhotosThumbnailState(10, { done: 0, failed: 0, total: 0 })).toBe("empty");
    });

    it("未処理がある場合は generating を返すこと", () => {
      expect(
        getAllPhotosThumbnailState(100, { done: 50, failed: 0, total: 100 })
      ).toBe("generating");
      expect(
        getAllPhotosThumbnailState(100, { done: 50, failed: 10, total: 100 })
      ).toBe("generating");
    });

    it("全件成功完了した場合は allDone を返すこと", () => {
      expect(
        getAllPhotosThumbnailState(100, { done: 100, failed: 0, total: 100 })
      ).toBe("allDone");
    });

    it("失敗が存在して全件処理完了した場合は warningDone を返すこと", () => {
      expect(
        getAllPhotosThumbnailState(100, { done: 95, failed: 5, total: 100 })
      ).toBe("warningDone");
    });
  });
});
