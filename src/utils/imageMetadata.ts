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
      return ext ? ext.toUpperCase() : "UNKNOWN";
  }
}

/** 翻訳関数の型定義 */
export type TranslationFunction = (key: string, params?: Record<string, string | number>) => string;

/**
 * フォーマットに応じた可逆／非可逆圧縮の種別を取得
 *
 * 変更理由: 多言語対応。英語設定時には英語表記（Lossless, Lossy等）を返し、
 * 日本語設定時には従来の日本語併記（可逆圧縮 (Lossless)等）を返す。
 *
 * @param formatName 正規化されたフォーマット名
 * @param t 翻訳関数（省略時はデフォルト英語/バイリンガル表記）
 * @returns 圧縮種別テキスト
 */
export function getCompressionType(formatName: string, t?: TranslationFunction): string {
  switch (formatName) {
    case "JPEG":
      return t ? t("metadata.compressionLossy") : "非可逆圧縮 (Lossy)";
    case "PNG":
      return t ? t("metadata.compressionLossless") : "可逆圧縮 (Lossless)";
    case "GIF":
      return t ? t("metadata.compressionLossless256") : "可逆圧縮 (Lossless / 256色)";
    case "BMP":
      return t ? t("metadata.compressionUncompressed") : "非圧縮 / 可逆 (Uncompressed)";
    case "WebP":
      return t ? t("metadata.compressionWebP") : "非可逆 / 可逆両対応 (WebP)";
    case "AVIF":
      return t ? t("metadata.compressionAvif") : "非可逆 / 可逆両対応 (AVIF)";
    case "TIFF":
      return t ? t("metadata.compressionTiff") : "可逆 / 非圧縮 (TIFF)";
    case "SVG":
      return t ? t("metadata.compressionVector") : "ベクター形式 (Vector)";
    default:
      return t ? t("metadata.compressionUnknown") : "不明";
  }
}

/**
 * 画像の詳細情報からフォーマット、可逆/非可逆、圧縮率情報を包括的に解析・算出
 *
 * 変更理由: 多言語対応。「削減」「生データ比」「可逆圧縮」等の文言を言語設定に応じて動的切り替え。
 *
 * @param detail 画像詳細情報（幅、高さ、ファイルサイズ、フォーマット、パス）
 * @param t 翻訳関数（省略時は日本語表記をフォールバックとして保持）
 * @returns 解析結果オブジェクト
 */
export function analyzeImageMetadata(
  detail: {
    width?: number | null;
    height?: number | null;
    fileSize: number;
    format?: string | null;
    filePath?: string;
  },
  t?: TranslationFunction
): FormattedImageMetadata {
  const rawFormat = getImageFormatName(detail.format, detail.filePath);
  const formatName = rawFormat === "UNKNOWN" && t ? t("metadata.formatUnknown") : rawFormat;
  const compressionType = getCompressionType(rawFormat, t);

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
    if (rawFormat === "BMP") {
      compressionRatioDisplay = t
        ? t("metadata.ratioUncompressedBmp")
        : "非圧縮 (生データ比 100%)";
    } else if (detail.fileSize < rawBytes) {
      const reduction = ((1 - detail.fileSize / rawBytes) * 100).toFixed(1);
      const ratio = ((detail.fileSize / rawBytes) * 100).toFixed(1);
      compressionRatioDisplay = t
        ? t("metadata.ratioReduction", { reduction, ratio, bpp: bppDisplay })
        : `${reduction}% 削減 (生データ比 ${ratio}% / ${bppDisplay})`;
    } else {
      const ratio = ((detail.fileSize / rawBytes) * 100).toFixed(1);
      compressionRatioDisplay = t
        ? t("metadata.ratioOverRaw", { ratio, bpp: bppDisplay })
        : `生データ比 ${ratio}% (${bppDisplay})`;
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
    compressionRatioDisplay: t
      ? t("metadata.ratioResolutionUnknown")
      : "解像度未取得のため計算不可",
    bppDisplay: null,
  };
}
