/**
 * Webブラウザからのドラッグ＆ドロップデータ解析ユーティリティ
 *
 * 変更理由: ChromeやFirefox等のWebブラウザからムードボードへの画像D&D、
 * およびサイドバーへのフォルダD&Dをクロスプラットフォーム（Windows/Chromebook等）で
 * 安全かつ確実に処理するため。画像実体と記事URLの分離、相対URL解決、Base64データ転送に対応。
 */

export interface WebDropResult {
  /** 画像のBase64 Data URL (data:image/...;base64,...) */
  imageData?: string;
  /** 画像バイナリ（後方互換用） */
  imageBytes?: number[];
  /** 推定される画像拡張子 ('png', 'jpg', 'webp' 等) */
  extension?: string;
  /** 画像自体のダウンロード用URL (http:// または https://) */
  imageUrl?: string;
  /** 画像が掲載されていた元記事・WebページのURL (http:// または https://) */
  sourceUrl?: string;
}

export interface DroppedDirectoryInfo {
  /** ドロップされたフォルダ名 */
  folderName: string;
  /** 直下の子ファイル/サブフォルダ名一覧 */
  childNames: string[];
}

export interface DroppedFileCandidate {
  /** ファイル名（例: "sample.png", "artwork.clip"） */
  name: string;
  /** ファイルサイズ（バイト） */
  size: number;
  /** 最終更新日時（ミリ秒タイムスタンプ） */
  lastModified?: number;
}

/**
 * URL文字列を正規化し、プロトコル相対URLや相対パスをベースURL基準で完全な http(s) URL に解決する
 *
 * @param raw 抽出された生のURL文字列
 * @param baseUrl 相対パス解決用のベースURL（オプション）
 * @returns 正規化された http(s) URL（無効な場合は undefined）
 */
export function resolveHttpUrl(
  raw: string | null | undefined,
  baseUrl?: string
): string | undefined {
  if (!raw) return undefined;
  const decoded = raw.trim().replace(/&amp;/gi, "&");
  if (!decoded) return undefined;

  // 非HTTPスキームを除外
  if (/^(?:data|blob|javascript|mailto|about|file|vbscript):/i.test(decoded)) {
    return undefined;
  }

  if (decoded.startsWith("http://") || decoded.startsWith("https://")) {
    return decoded;
  }

  if (decoded.startsWith("//")) {
    return `https:${decoded}`;
  }

  if (baseUrl && (baseUrl.startsWith("http://") || baseUrl.startsWith("https://"))) {
    try {
      const resolved = new URL(decoded, baseUrl).href;
      if (resolved.startsWith("http://") || resolved.startsWith("https://")) {
        return resolved;
      }
    } catch {
      // 無効な相対URLは無視
    }
  }

  return undefined;
}

/**
 * srcset 属性文字列から最も高解像度（末尾）の画像URLを抽出する
 *
 * @param srcset imgタグのsrcset属性値
 * @param baseUrl 相対パス解決用ベースURL
 * @returns 解決された画像URL
 */
function extractBestUrlFromSrcset(
  srcset: string | null | undefined,
  baseUrl?: string
): string | undefined {
  if (!srcset) return undefined;
  const candidates = srcset
    .split(",")
    .map((part) => part.trim().split(/\s+/)[0])
    .filter(Boolean);

  // srcset は通常 低解像度 -> 高解像度 の順に並ぶため末尾から優先的に解決を試みる
  for (let i = candidates.length - 1; i >= 0; i--) {
    const resolved = resolveHttpUrl(candidates[i], baseUrl);
    if (resolved) return resolved;
  }
  return undefined;
}

/**
 * Blob または File オブジェクトを Base64 Data URL 文字列として読み込む
 *
 * 変更理由: 巨大画像（10MB〜）を number[] 配列として JSON 化するとメモリ消費と IPC 転送時間が
 * 著しく増大するため、Base64 文字列（FileReader.readAsDataURL）により転送効率を最大化する。
 *
 * @param blob 読み込み対象の Blob / File
 * @returns Base64 Data URL 文字列
 */
export async function readBlobAsDataUrl(blob: Blob): Promise<string> {
  if (typeof FileReader !== "undefined") {
    return new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => {
        if (typeof reader.result === "string") {
          resolve(reader.result);
        } else {
          reject(new Error("Data URLの読み取り結果が文字列ではありません"));
        }
      };
      reader.onerror = () => reject(reader.error || new Error("Blobの読み取りに失敗しました"));
      reader.readAsDataURL(blob);
    });
  }

  // Node.js / テスト環境用フォールバック
  const buffer = await blob.arrayBuffer();
  const base64 =
    typeof Buffer !== "undefined"
      ? Buffer.from(buffer).toString("base64")
      : btoa(String.fromCharCode(...new Uint8Array(buffer)));
  const mime = blob.type || "application/octet-stream";
  return `data:${mime};base64,${base64}`;
}

/**
 * FileList または File 配列からドロップ候補情報を抽出する
 *
 * @param files ドロップされたファイル群
 * @returns バックエンド検索用候補配列
 */
export function extractDroppedFileCandidates(files: FileList | File[]): DroppedFileCandidate[] {
  return Array.from(files).map((f) => ({
    name: f.name,
    size: f.size,
    lastModified: f.lastModified,
  }));
}

/**
 * DataTransfer 内から画像URL（ダウンロード用）と元記事・WebページURL（出典参照用）を分離して抽出する
 *
 * 変更理由: リンク付き画像（<a href="article"><img src="image"/></a>）をドロップした際、
 * 画像URLではなく記事URLをダウンロードしようとして HTML を画像として誤保存する事故や、
 * 相対パス（href="/post/123"）のリンクで text/uri-list がスキップされ記事URLが失われる問題を防止するため。
 *
 * @param dataTransfer ドラッグまたはペーストイベントの DataTransfer オブジェクト
 * @returns imageUrl（画像直リンク）および sourceUrl（出典WebページURL）
 */
export function extractUrlsFromDataTransfer(
  dataTransfer: DataTransfer
): { imageUrl?: string; sourceUrl?: string } {
  let imageUrl: string | undefined;
  let sourceUrl: string | undefined;

  // 1. まず text/uri-list, text/x-moz-url, URL, text/plain からブラウザ解決済みの絶対URL群を収集
  const uriCandidates: string[] = [];

  const uriList = dataTransfer.getData("text/uri-list");
  if (uriList) {
    for (const rawLine of uriList.split(/\r?\n/)) {
      const line = rawLine.trim();
      if (!line || line.startsWith("#")) continue;
      const resolved = resolveHttpUrl(line);
      if (resolved && !uriCandidates.includes(resolved)) {
        uriCandidates.push(resolved);
      }
    }
  }

  const mozUrl = dataTransfer.getData("text/x-moz-url");
  if (mozUrl) {
    const firstLine = mozUrl.split(/\r?\n/)[0]?.trim();
    const resolved = resolveHttpUrl(firstLine);
    if (resolved && !uriCandidates.includes(resolved)) {
      uriCandidates.push(resolved);
    }
  }

  const directUrl = dataTransfer.getData("URL");
  if (directUrl) {
    const resolved = resolveHttpUrl(directUrl);
    if (resolved && !uriCandidates.includes(resolved)) {
      uriCandidates.push(resolved);
    }
  }

  const plainText = dataTransfer.getData("text/plain");
  if (plainText) {
    const trimmed = plainText.trim();
    const directPlain = resolveHttpUrl(trimmed);
    if (directPlain) {
      if (!uriCandidates.includes(directPlain)) uriCandidates.push(directPlain);
    } else {
      const urlMatch = trimmed.match(/https?:\/\/[^\s"'<>]+/i);
      const matchedUrl = resolveHttpUrl(urlMatch?.[0]);
      if (matchedUrl && !uriCandidates.includes(matchedUrl)) {
        uriCandidates.push(matchedUrl);
      }
    }
  }

  const fallbackBaseUrl = uriCandidates[0];

  // 2. text/html からの解析（DOMParser または正規表現）
  const htmlData = dataTransfer.getData("text/html");
  if (htmlData) {
    let baseUrl = fallbackBaseUrl;

    if (typeof DOMParser !== "undefined") {
      try {
        const parser = new DOMParser();
        const doc = parser.parseFromString(htmlData, "text/html");

        const baseElem = doc.querySelector("base[href]");
        const baseHref = resolveHttpUrl(baseElem?.getAttribute("href"));
        if (baseHref) {
          baseUrl = baseHref;
        }

        // 親または同一コンテキストの a タグ（記事URL）を先に確認してベースURL候補にも活用
        const imgElem = doc.querySelector("img");
        const anchor = imgElem?.closest("a") || doc.querySelector("a");
        if (anchor) {
          const resolvedHref = resolveHttpUrl(anchor.getAttribute("href"), baseUrl);
          if (resolvedHref) {
            sourceUrl = resolvedHref;
            if (!baseUrl) baseUrl = resolvedHref;
          }
        }

        // img タグの探索
        if (imgElem) {
          const candidates = [
            imgElem.getAttribute("src"),
            imgElem.getAttribute("data-src"),
            imgElem.getAttribute("data-original"),
            imgElem.getAttribute("data-actualsrc"),
          ];
          for (const raw of candidates) {
            const resolved = resolveHttpUrl(raw, baseUrl);
            if (resolved) {
              imageUrl = resolved;
              break;
            }
          }
          if (!imageUrl) {
            imageUrl = extractBestUrlFromSrcset(imgElem.getAttribute("srcset"), baseUrl);
          }
        }
      } catch {
        // 正規表現フォールバックへ
      }
    }

    if (!sourceUrl) {
      const hrefMatch = htmlData.match(/<a\b[^>]*?\bhref=["']([^"']+)["']/i);
      if (hrefMatch) {
        const resolvedHref = resolveHttpUrl(hrefMatch[1], baseUrl);
        if (resolvedHref) {
          sourceUrl = resolvedHref;
          if (!baseUrl) baseUrl = resolvedHref;
        }
      }
    }

    if (!imageUrl) {
      const imgTagMatch = htmlData.match(/<img\b([^>]+)>/i);
      if (imgTagMatch) {
        const attrs = imgTagMatch[1];
        for (const attrName of ["src", "data-src", "data-original", "data-actualsrc"]) {
          const m = attrs.match(new RegExp(`\\b${attrName}=["']([^"']+)["']`, "i"));
          if (m) {
            const resolved = resolveHttpUrl(m[1], baseUrl);
            if (resolved) {
              imageUrl = resolved;
              break;
            }
          }
        }
        if (!imageUrl) {
          const srcsetMatch = attrs.match(/\bsrcset=["']([^"']+)["']/i);
          if (srcsetMatch) {
            imageUrl = extractBestUrlFromSrcset(srcsetMatch[1], baseUrl);
          }
        }
      }
    }
  }

  // 3. 収集した uriCandidates による補完
  if (!sourceUrl && uriCandidates.length > 0) {
    // imageUrl と異なるURL（記事URL等）が uriCandidates にあれば優先的に sourceUrl とする
    const distinctUri = imageUrl
      ? uriCandidates.find((u) => u !== imageUrl)
      : undefined;
    sourceUrl = distinctUri || uriCandidates[0];
  }

  if (!imageUrl && uriCandidates.length > 0) {
    imageUrl = uriCandidates[0];
  }

  // 片方のみ見つかった場合の最終補完
  if (imageUrl && !sourceUrl) {
    sourceUrl = imageUrl;
  }
  if (!imageUrl && sourceUrl) {
    imageUrl = sourceUrl;
  }

  return { imageUrl, sourceUrl };
}

/**
 * DataTransfer からWeb画像または掲載Webページの元URLを抽出する（後方互換用）
 *
 * @param dataTransfer ドラッグイベントの DataTransfer オブジェクト
 * @returns 抽出された http(s) URL（見つからない場合は undefined）
 */
export function extractSourceUrl(dataTransfer: DataTransfer): string | undefined {
  const { sourceUrl, imageUrl } = extractUrlsFromDataTransfer(dataTransfer);
  return sourceUrl || imageUrl;
}

/**
 * DragEvent / ClipboardEvent の DataTransfer から Web 画像データを抽出する
 *
 * 変更理由: Base64 Data URL による高効率転送と、リンク付き画像での imageUrl / sourceUrl 分離に対応。
 * また DataTransfer の内容は最初の await より前にすべて同期取得し、非同期処理後のデータ消失を防止する。
 *
 * @param dataTransfer ドラッグまたはペーストイベントの DataTransfer オブジェクト
 * @returns 抽出された画像データ（Base64またはURL）。Web画像でない場合は null
 */
export async function extractWebDropImageData(
  dataTransfer: DataTransfer | null
): Promise<WebDropResult | null> {
  if (!dataTransfer) return null;

  // ★ 重要: DataTransfer のデータは最初の await の前にすべて同期的に読み取る
  const { imageUrl, sourceUrl } = extractUrlsFromDataTransfer(dataTransfer);
  const files = Array.from(dataTransfer.files || []);
  const htmlData = dataTransfer.getData("text/html") || "";

  // 1. HTML形式からインライン Data URL (data:image/...) の確認
  let inlineDataUri: string | undefined;
  let inlineExt: string | undefined;
  if (htmlData) {
    const dataUrlMatch = htmlData.match(/src=["'](data:image\/[a-zA-Z0-9+.-]+;base64,[^"']+)["']/i);
    if (dataUrlMatch) {
      inlineDataUri = dataUrlMatch[1];
      const mimeMatch = inlineDataUri.match(/^data:image\/([a-zA-Z0-9+.-]+);/i);
      inlineExt = mimeMatch ? mimeMatch[1].toLowerCase().replace("jpeg", "jpg") : undefined;
    }
  }

  // 2. ファイルエントリに画像ファイルが含まれている場合 (Chrome等でのWeb画像直接ドラッグ時)
  const imgFile = files.find((f) => f.type.startsWith("image/"));
  if (imgFile) {
    // ローカルファイルパス（file.path）が存在する場合はエクスプローラからのローカルドロップなのでWeb画像として扱わない
    const hasLocalPath = Boolean((imgFile as File & { path?: string }).path);
    if (!hasLocalPath) {
      const dataUrl = await readBlobAsDataUrl(imgFile);
      const extMatch = imgFile.name.match(/\.([a-zA-Z0-9]+)$/);
      const extension = extMatch
        ? extMatch[1].toLowerCase()
        : imgFile.type.split("/")[1]?.toLowerCase().replace("jpeg", "jpg");

      return {
        imageData: dataUrl,
        extension,
        imageUrl,
        sourceUrl: sourceUrl || imageUrl,
      };
    }
  }

  // 3. インライン Data URL 画像がある場合
  if (inlineDataUri) {
    return {
      imageData: inlineDataUri,
      extension: inlineExt,
      sourceUrl: sourceUrl || imageUrl,
    };
  }

  // 4. files にファイルがない場合（Firefox等）: imageUrl / sourceUrl からの取得
  if (imageUrl || sourceUrl) {
    const targetUrl = imageUrl || sourceUrl!;
    const urlPath = targetUrl.split("?")[0].split("#")[0];
    const extMatch = urlPath.match(/\.([a-zA-Z0-9]+)$/);
    const extension = extMatch ? extMatch[1].toLowerCase() : undefined;

    return {
      imageUrl: targetUrl,
      sourceUrl: sourceUrl || targetUrl,
      extension,
    };
  }

  return null;
}

/**
 * DragEvent からドロップされたフォルダの名前および直下のエントリ一覧を非同期抽出する
 *
 * @param dataTransfer ドラッグイベントの DataTransfer オブジェクト
 * @returns フォルダ情報。フォルダでない場合は null
 */
export async function extractDroppedDirectoryInfo(
  dataTransfer: DataTransfer | null
): Promise<DroppedDirectoryInfo | null> {
  if (!dataTransfer || !dataTransfer.items || dataTransfer.items.length === 0) {
    return null;
  }

  const item = dataTransfer.items[0];
  const entry = item.webkitGetAsEntry ? item.webkitGetAsEntry() : null;

  if (!entry || !entry.isDirectory) {
    return null;
  }

  const dirEntry = entry as FileSystemDirectoryEntry;
  const folderName = dirEntry.name;
  const childNames: string[] = [];

  try {
    const reader = dirEntry.createReader();
    await new Promise<void>((resolve) => {
      const readBatch = () => {
        reader.readEntries((entries) => {
          if (!entries || entries.length === 0 || childNames.length >= 200) {
            resolve();
            return;
          }
          for (const e of entries) {
            childNames.push(e.name);
          }
          readBatch();
        }, () => {
          resolve();
        });
      };
      readBatch();
    });
  } catch {
    // 読み取り失敗時でもフォルダ名のみで特定を試みる
  }

  return {
    folderName,
    childNames,
  };
}
