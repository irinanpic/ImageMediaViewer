import { convertFileSrc } from "@tauri-apps/api/core";

/**
 * Tauri環境で実行されているかどうかを判定
 *
 * @returns Tauri環境内であれば true
 */
export function isTauriEnvironment(): boolean {
  return typeof window !== "undefined" && Boolean((window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__);
}

/**
 * 画像IDとリビジョンからサムネイルURLを生成する
 *
 * 変更理由: 仕様書§5.5および完全ポートレス一体型アーキテクチャ。
 * TCP/IPポートを一切介さず、Tauriネイティブのカスタムスキーム（thumb://）により
 * Rustのオンデマンド生成・WebP配信パイプラインから直接バイナリを取得する。
 *
 * @param imageId 画像ID
 * @param rev ファイル更新日時等のリビジョン番号（キャッシュバスター）
 * @returns サムネイル用URL文字列
 */
export function getThumbnailUrl(imageId: number, rev: number): string {
  try {
    const base = convertFileSrc(String(imageId), "thumb");
    return `${base}?v=${rev}`;
  } catch {
    return `http://thumb.localhost/${imageId}?v=${rev}`;
  }
}

/**
 * 画像IDから原寸画像配信用URLを生成する
 *
 * 変更理由: 仕様書§5.5および完全ポートレス一体型アーキテクチャ。
 * TCP/IPポートを一切介さず、Tauriネイティブのカスタムスキーム（original://）により
 * 原寸画像バイナリを直接取得する。
 *
 * @param imageId 画像ID
 * @param rev キャッシュバスティング用リビジョン（更新時刻等）
 * @returns 原寸配信用URL文字列
 */
export function getOriginalImageUrl(imageId: number, rev?: number): string {
  const query = rev ? `?v=${rev}` : "";
  try {
    return `${convertFileSrc(String(imageId), "original")}${query}`;
  } catch {
    return `http://original.localhost/${imageId}${query}`;
  }
}
