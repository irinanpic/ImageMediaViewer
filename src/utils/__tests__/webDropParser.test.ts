import { describe, it, expect } from "vitest";
import {
  extractSourceUrl,
  extractUrlsFromDataTransfer,
  extractDroppedFileCandidates,
  extractWebDropImageData,
  extractDroppedDirectoryInfo,
} from "../webDropParser";

describe("webDropParser", () => {
  describe("extractUrlsFromDataTransfer", () => {
    it("リンク付き画像（<a><img></a>）で imageUrl と sourceUrl が正しく分離して取得できること", () => {
      const dt = {
        getData: (format: string) => {
          if (format === "text/html") {
            return '<div class="item"><a href="https://news.example.com/articles/123"><img src="https://cdn.example.com/photo.jpg" alt="Photo" /></a></div>';
          }
          return "";
        },
      } as unknown as DataTransfer;

      const { imageUrl, sourceUrl } = extractUrlsFromDataTransfer(dt);
      expect(imageUrl).toBe("https://cdn.example.com/photo.jpg");
      expect(sourceUrl).toBe("https://news.example.com/articles/123");
    });

    it("リンクなし画像（単独<img>）では imageUrl と sourceUrl が同じ画像URLになること", () => {
      const dt = {
        getData: (format: string) => {
          if (format === "text/html") {
            return '<img src="https://cdn.example.com/single.png" alt="Single" />';
          }
          return "";
        },
      } as unknown as DataTransfer;

      const { imageUrl, sourceUrl } = extractUrlsFromDataTransfer(dt);
      expect(imageUrl).toBe("https://cdn.example.com/single.png");
      expect(sourceUrl).toBe("https://cdn.example.com/single.png");
    });

    it("text/uri-list のみの場合でも両方にURLが設定されること", () => {
      const dt = {
        getData: (format: string) => {
          if (format === "text/uri-list") {
            return "https://example.com/direct.webp\r\n";
          }
          return "";
        },
      } as unknown as DataTransfer;

      const { imageUrl, sourceUrl } = extractUrlsFromDataTransfer(dt);
      expect(imageUrl).toBe("https://example.com/direct.webp");
      expect(sourceUrl).toBe("https://example.com/direct.webp");
    });

    it("aタグのhrefが相対パス（/post/999）でも text/uri-list のベースURLから完全な記事URLを解決できること", () => {
      const dt = {
        getData: (format: string) => {
          if (format === "text/html") {
            return '<a href="/user/status/999"><img src="https://pbs.example.com/media/abc.jpg" /></a>';
          }
          if (format === "text/uri-list") {
            return "https://social.example.com/user/status/999\r\n";
          }
          return "";
        },
      } as unknown as DataTransfer;

      const { imageUrl, sourceUrl } = extractUrlsFromDataTransfer(dt);
      expect(imageUrl).toBe("https://pbs.example.com/media/abc.jpg");
      expect(sourceUrl).toBe("https://social.example.com/user/status/999");
    });

    it("プロトコル相対URL（//cdn.example.com/...）や srcset の最高解像度URLを正しく解決できること", () => {
      const dt = {
        getData: (format: string) => {
          if (format === "text/html") {
            return '<a href="https://gallery.example.com/works/5"><img srcset="//cdn.example.com/thumb_320.jpg 320w, //cdn.example.com/full_1920.jpg 1920w" /></a>';
          }
          return "";
        },
      } as unknown as DataTransfer;

      const { imageUrl, sourceUrl } = extractUrlsFromDataTransfer(dt);
      expect(imageUrl).toBe("https://cdn.example.com/full_1920.jpg");
      expect(sourceUrl).toBe("https://gallery.example.com/works/5");
    });
  });

  describe("extractDroppedFileCandidates", () => {
    it("FileList / File 配列から name, size, lastModified を正しく抽出できること", () => {
      const files = [
        { name: "test1.png", size: 1024, lastModified: 1700000000000 },
        { name: "project.clip", size: 2048576, lastModified: 1700000050000 },
      ] as unknown as File[];

      const candidates = extractDroppedFileCandidates(files);
      expect(candidates).toHaveLength(2);
      expect(candidates[0]).toEqual({
        name: "test1.png",
        size: 1024,
        lastModified: 1700000000000,
      });
      expect(candidates[1]).toEqual({
        name: "project.clip",
        size: 2048576,
        lastModified: 1700000050000,
      });
    });
  });

  describe("extractSourceUrl", () => {
    it("text/html 内の img タグの src から抽出できること", () => {
      const dt = {
        getData: (format: string) => {
          if (format === "text/html") {
            return '<div class="gallery"><img src="https://cdn.example.com/art.jpg" alt="Art" /></div>';
          }
          return "";
        },
      } as unknown as DataTransfer;

      expect(extractSourceUrl(dt)).toBe("https://cdn.example.com/art.jpg");
    });

    it("text/html 内の img タグの data-src から抽出できること（遅延読み込み画像）", () => {
      const dt = {
        getData: (format: string) => {
          if (format === "text/html") {
            return '<img src="data:image/svg+xml,placeholder" data-src="https://cdn.example.com/highres.png" />';
          }
          return "";
        },
      } as unknown as DataTransfer;

      expect(extractSourceUrl(dt)).toBe("https://cdn.example.com/highres.png");
    });

    it("text/html 内の a タグの href（リンク付き画像）から抽出できること", () => {
      const dt = {
        getData: (format: string) => {
          if (format === "text/html") {
            return '<a href="https://example.com/post/42"><img src="blob:http://localhost/blob-id" /></a>';
          }
          return "";
        },
      } as unknown as DataTransfer;

      expect(extractSourceUrl(dt)).toBe("https://example.com/post/42");
    });

    it("text/uri-list の改行やコメント行を適切に処理して先頭URLを抽出できること", () => {
      const dt = {
        getData: (format: string) => {
          if (format === "text/uri-list") {
            return "# RFC 2483 format\r\n# Comment\r\nhttps://example.com/photo.webp\r\nhttps://example.com/other.png\r\n";
          }
          return "";
        },
      } as unknown as DataTransfer;

      expect(extractSourceUrl(dt)).toBe("https://example.com/photo.webp");
    });

    it("text/x-moz-url (Firefox固有形式: URL + 改行 + タイトル) から抽出できること", () => {
      const dt = {
        getData: (format: string) => {
          if (format === "text/x-moz-url") {
            return "https://firefox.example.com/sketch.png\nSketch Title";
          }
          return "";
        },
      } as unknown as DataTransfer;

      expect(extractSourceUrl(dt)).toBe("https://firefox.example.com/sketch.png");
    });

    it("URL キー（Chromium互換）から抽出できること", () => {
      const dt = {
        getData: (format: string) => {
          if (format === "URL") {
            return "https://chromium.example.com/pic.jpeg";
          }
          return "";
        },
      } as unknown as DataTransfer;

      expect(extractSourceUrl(dt)).toBe("https://chromium.example.com/pic.jpeg");
    });

    it("text/plain 内の URL から抽出できること", () => {
      const dt = {
        getData: (format: string) => {
          if (format === "text/plain") {
            return "Check this image: https://plain.example.com/view.jpg in my post";
          }
          return "";
        },
      } as unknown as DataTransfer;

      expect(extractSourceUrl(dt)).toBe("https://plain.example.com/view.jpg");
    });

    it("不正な javascript: スキームは除外されること", () => {
      const dt = {
        getData: (format: string) => {
          if (format === "text/html") {
            return '<img src="javascript:alert(1)" />';
          }
          return "";
        },
      } as unknown as DataTransfer;

      expect(extractSourceUrl(dt)).toBeUndefined();
    });
  });

  describe("extractWebDropImageData", () => {
    it("nullが渡された場合はnullを返すこと", async () => {
      const res = await extractWebDropImageData(null);
      expect(res).toBeNull();
    });

    it("【Chrome挙動】files に仮想画像ファイルがあり、text/html に img タグがある場合、imageData と sourceUrl の両方が正しく取得できること", async () => {
      const dummyFile = new Blob(["dummy-png-content"], { type: "image/png" });
      Object.defineProperty(dummyFile, "name", { value: "download.png" });

      // Chrome では files にファイルが生成され、text/uri-list は空、text/html に img src が入る
      const mockDataTransfer = {
        files: [dummyFile],
        getData: (format: string) => {
          if (format === "text/html") {
            return '<meta charset="utf-8"><a href="https://example.com/article/1"><img src="https://images.unsplash.com/photo-1234.jpg?w=1080" alt="Unsplash Photo" /></a>';
          }
          if (format === "text/plain") return "Unsplash Photo";
          return "";
        },
      } as unknown as DataTransfer;

      const res = await extractWebDropImageData(mockDataTransfer);
      expect(res).not.toBeNull();
      expect(res?.imageData).toMatch(/^data:image\/png;base64,/);
      expect(res?.extension).toBe("png");
      expect(res?.imageUrl).toBe("https://images.unsplash.com/photo-1234.jpg?w=1080");
      expect(res?.sourceUrl).toBe("https://example.com/article/1");
    });

    it("【Firefox挙動】files がなく、text/uri-list または text/html のみの場合でも imageUrl / sourceUrl と extension を取得できること", async () => {
      const mockDataTransfer = {
        files: [],
        getData: (format: string) => {
          if (format === "text/uri-list") return "https://example.com/photos/cat.webp\r\n";
          return "";
        },
      } as unknown as DataTransfer;

      const res = await extractWebDropImageData(mockDataTransfer);
      expect(res).not.toBeNull();
      expect(res?.imageUrl).toBe("https://example.com/photos/cat.webp");
      expect(res?.sourceUrl).toBe("https://example.com/photos/cat.webp");
      expect(res?.extension).toBe("webp");
      expect(res?.imageData).toBeUndefined();
    });

    it("HTML形式から data:image/png;base64,... インライン画像を抽出できること", async () => {
      const base64Data = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
      const mockDataTransfer = {
        files: [],
        getData: (format: string) => {
          if (format === "text/html") return `<img src="${base64Data}" />`;
          return "";
        },
      } as unknown as DataTransfer;

      const res = await extractWebDropImageData(mockDataTransfer);
      expect(res).not.toBeNull();
      expect(res?.imageData).toBe(base64Data);
      expect(res?.extension).toBe("png");
    });

    it("画像に関係のないテキストの場合はnullを返すこと", async () => {
      const mockDataTransfer = {
        files: [],
        getData: () => "単なるテキスト情報です",
      } as unknown as DataTransfer;

      const res = await extractWebDropImageData(mockDataTransfer);
      expect(res).toBeNull();
    });
  });

  describe("extractDroppedDirectoryInfo", () => {
    it("itemsが空または非ディレクトリの場合はnullを返すこと", async () => {
      const mockDataTransfer = {
        items: [],
      } as unknown as DataTransfer;

      const res = await extractDroppedDirectoryInfo(mockDataTransfer);
      expect(res).toBeNull();
    });

    it("ディレクトリEntryが存在する場合にfolderNameとchildNamesを取得できること", async () => {
      const mockEntries = [
        { name: "img1.png", isFile: true, isDirectory: false },
        { name: "subfolder", isFile: false, isDirectory: true },
      ];

      const mockReader = {
        readEntries: (success: (entries: unknown[]) => void) => {
          if (!mockReader._called) {
            mockReader._called = true;
            success(mockEntries);
          } else {
            success([]);
          }
        },
        _called: false,
      };

      const mockDirEntry = {
        name: "MyFavorites",
        isDirectory: true,
        createReader: () => mockReader,
      };

      const mockItem = {
        webkitGetAsEntry: () => mockDirEntry,
      };

      const mockDataTransfer = {
        items: [mockItem],
      } as unknown as DataTransfer;

      const res = await extractDroppedDirectoryInfo(mockDataTransfer);
      expect(res).not.toBeNull();
      expect(res?.folderName).toBe("MyFavorites");
      expect(res?.childNames).toEqual(["img1.png", "subfolder"]);
    });
  });
});
