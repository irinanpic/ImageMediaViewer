/**
 * 画像メタデータおよびフォーマット・圧縮率解析ユーティリティ
 *
 * 変更理由: ユーザー要求「詳細情報に画像フォーマット情報、可逆/非可逆、
 * ファイルサイズ/圧縮率情報を追加」に基づき、タイムラインおよびムードボードで
 * 共通利用可能な解析・フォーマットロジックを提供する
 */

export interface FormattedImageMetadata {
  /** 画像フォーマット名（例: "JPEG", "PNG", "WebP" 等） */
  formatName: string;
  /** 圧縮方式（可逆 / 非可逆）の説明テキスト */
  compressionType: string;
  /** ファイルサイズ表示文字列（例: "3.45 MB (3,617,592 B)"） */
  fileSizeDisplay: string;
  /** 非圧縮生データサイズ（例: "34.33 MB"）※幅・高さが存在する場合 */
  rawSizeDisplay: string | null;
  /** 圧縮率・削減率のサマリテキスト（例: "94.2% 削減 (生データ比 5.8% / 1.39 bpp)"） */
  compressionRatioDisplay: string;
  /** ピクセルあたりビット数 (bpp) */
  bppDisplay: string | null;
}

/**
 * バイト数を読みやすい形式（B, KB, MB, GB）に変換
 *
 * @param bytes バイト数
 * @returns フォーマット済み文字列
 */
export function formatBytes(bytes: number): string {
  if (bytes === 0) return "0 B";
  if (bytes < 1024) return `${bytes} B`;
  const kb = bytes / 1024;
  if (kb < 1024) return `${kb.toFixed(2)} KB`;
  const mb = kb / 1024;
  if (mb < 1024) return `${mb.toFixed(2)} MB`;
  const gb = mb / 1024;
  return `${gb.toFixed(2)} GB`;
}

/**
 * ファイルパスやフォーマット文字列から正規化されたフォーマット名を算出
 *
 * @param format DB上のフォーマット値（拡張子）
 * @param filePath ファイルパス
 * @returns フォーマット大文字表記
 */
export function getImageFormatName(format?: string | null, filePath?: string): string {
  let ext = (format || "").toLowerCase().trim();
  if (!ext && filePath) {
    const parts = filePath.split(".");
    if (parts.length > 1) {
      ext = parts.pop()!.toLowerCase().trim();
    }
  }

  switch (ext) {
    case "jpg":
    case "jpeg":
      return "JPEG";
    case "png":
      return "PNG";
    case "webp":
      return "WebP";
    case "gif":
      return "GIF";
    case "bmp":
      return "BMP";
    case "svg":
      return "SVG";
    case "tiff":
    case "tif":
      return "TIFF";
    case "avif":
      return "AVIF";
    default:
      return ext ? ext.toUpperCase() : "不明";
  }
}

/**
 * フォーマットに応じた可逆／非可逆圧縮の種別を取得
 *
 * @param formatName 正規化されたフォーマット名
 * @returns 圧縮種別テキスト
 */
export function getCompressionType(formatName: string): string {
  switch (formatName) {
    case "JPEG":
      return "非可逆圧縮 (Lossy)";
    case "PNG":
      return "可逆圧縮 (Lossless)";
    case "GIF":
      return "可逆圧縮 (Lossless / 256色)";
    case "BMP":
      return "非圧縮 / 可逆 (Uncompressed)";
    case "WebP":
      return "非可逆 / 可逆両対応 (WebP)";
    case "AVIF":
      return "非可逆 / 可逆両対応 (AVIF)";
    case "TIFF":
      return "可逆 / 非圧縮 (TIFF)";
    case "SVG":
      return "ベクター形式 (Vector)";
    default:
      return "不明";
  }
}

/**
 * 画像の詳細情報からフォーマット、可逆/非可逆、圧縮率情報を包括的に解析・算出
 *
 * @param detail 画像詳細情報（幅、高さ、ファイルサイズ、フォーマット、パス）
 * @returns 解析結果オブジェクト
 */
export function analyzeImageMetadata(detail: {
  width?: number | null;
  height?: number | null;
  fileSize: number;
  format?: string | null;
  filePath?: string;
}): FormattedImageMetadata {
  const formatName = getImageFormatName(detail.format, detail.filePath);
  const compressionType = getCompressionType(formatName);

  // ファイルサイズ表記: 例 "3.45 MB (3,617,592 B)"
  const formattedSize = formatBytes(detail.fileSize);
  const fileSizeDisplay = `${formattedSize} (${detail.fileSize.toLocaleString()} B)`;

  const width = detail.width ?? null;
  const height = detail.height ?? null;

  if (width && height && width > 0 && height > 0) {
    // 24bit RGB非圧縮時の生データサイズ（1ピクセルあたり3バイト）
    const rawBytes = width * height * 3;
    const rawSizeDisplay = formatBytes(rawBytes);

    // bits per pixel (bpp) = (fileSize * 8) / (width * height)
    const bpp = (detail.fileSize * 8) / (width * height);
    const bppDisplay = `${bpp.toFixed(2)} bpp`;

    let compressionRatioDisplay = "";
    if (formatName === "BMP") {
      compressionRatioDisplay = `非圧縮 (生データ比 100%)`;
    } else if (detail.fileSize < rawBytes) {
      const reduction = ((1 - detail.fileSize / rawBytes) * 100).toFixed(1);
      const ratio = ((detail.fileSize / rawBytes) * 100).toFixed(1);
      compressionRatioDisplay = `${reduction}% 削減 (生データ比 ${ratio}% / ${bppDisplay})`;
    } else {
      const ratio = ((detail.fileSize / rawBytes) * 100).toFixed(1);
      compressionRatioDisplay = `生データ比 ${ratio}% (${bppDisplay})`;
    }

    return {
      formatName,
      compressionType,
      fileSizeDisplay,
      rawSizeDisplay,
      compressionRatioDisplay,
      bppDisplay,
    };
  }

  return {
    formatName,
    compressionType,
    fileSizeDisplay,
    rawSizeDisplay: null,
    compressionRatioDisplay: "解像度未取得のため計算不可",
    bppDisplay: null,
  };
}
