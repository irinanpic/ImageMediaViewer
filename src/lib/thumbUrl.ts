import { convertFileSrc } from "@tauri-apps/api/core";


/**
 * Tauri環境で実行されているかどうかを判定
 */
export function isTauriEnvironment(): boolean {
  return typeof window !== "undefined" && Boolean((window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__);
}

function getBaseHttpUrl(): string {
  if (typeof window !== "undefined" && window.location && window.location.origin && window.location.origin.startsWith("http")) {
    return window.location.origin;
  }
  return "http://127.0.0.1:14201";
}

/**
 * 画像IDとリビジョンからサムネイルURLを生成する
 *
 * 変更理由: 仕様書§5.5および独立ウィンドウ/Webブラウザ両対応。
 * Tauri環境ではカスタムスキーム、それ以外では現在のオリジン（または127.0.0.1:14201）のURLを返却
 *
 * @param imageId 画像ID
 * @param rev ファイル更新日時等のリビジョン番号（キャッシュバスター）
 * @returns サムネイル用URL文字列
 */
export function getThumbnailUrl(imageId: number, rev: number): string {
  if (isTauriEnvironment()) {
    try {
      const base = convertFileSrc(String(imageId), "thumb");
      return `${base}?v=${rev}`;
    } catch {
      // フォールバック
    }
  }
  return `${getBaseHttpUrl()}/thumbs/${imageId}?v=${rev}`;
}

/**
 * 画像IDから原寸画像配信用URLを生成する
 *
 * 変更理由: 仕様書§5.5および独立ウィンドウ/Webブラウザ両対応
 *
 * @param imageId 画像ID
 * @returns 原寸配信用URL文字列
 */
export function getOriginalImageUrl(imageId: number): string {
  if (isTauriEnvironment()) {
    try {
      return convertFileSrc(String(imageId), "original");
    } catch {
      // フォールバック
    }
  }
  return `${getBaseHttpUrl()}/raw/${imageId}`;
}


