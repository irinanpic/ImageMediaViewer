/**
 * タイムライン複合検索・フィルタおよびタグ・サブフォルダ階層に関する型定義とユーティリティ
 *
 * 変更理由: ファイル名検索（AND/OR/NOT）、拡張子フィルタ、縦横比フィルタ、タグ絞り込み、
 * サブフォルダ階層選択の状態管理とキャッシュキー生成を一元化するため。
 * Rust 側の models.rs（ts-rs 自動生成型）と完全にフィールド名を一致させる。
 */
import type { TimelineFilter } from "./generated/TimelineFilter";
import type { TagRecord } from "./generated/TagRecord";
import type { TagWithCount } from "./generated/TagWithCount";
import type { ImageTagAssignment } from "./generated/ImageTagAssignment";
import type { SubfolderEntry } from "./generated/SubfolderEntry";

export type {
  TimelineFilter,
  TagRecord,
  TagWithCount,
  ImageTagAssignment,
  SubfolderEntry,
};

/**
 * タイムラインフィルタの初期状態（未指定）
 */
export const DEFAULT_TIMELINE_FILTER: TimelineFilter = {
  query: null,
  queryMode: "and",
  extensions: null,
  excludeExtensions: null,
  aspectRatio: null,
  subFolderPath: null,
  includeTagIds: null,
  excludeTagIds: null,
  tagMode: "and",
};

/**
 * いずれかのフィルタ条件（サブフォルダ含む）が有効かどうかを判定する
 *
 * @param filter 判定対象のフィルタ状態
 * @returns 条件が1つ以上指定されていれば true
 */
export function isTimelineFilterActive(filter?: TimelineFilter | null): boolean {
  if (!filter) return false;
  if (filter.subFolderPath && filter.subFolderPath.trim() !== "") return true;
  if (filter.query && filter.query.trim() !== "") return true;
  if (filter.extensions && filter.extensions.length > 0) return true;
  if (filter.excludeExtensions && filter.excludeExtensions.length > 0) return true;
  if (filter.aspectRatio && filter.aspectRatio.trim() !== "") return true;
  if (filter.includeTagIds && filter.includeTagIds.length > 0) return true;
  if (filter.excludeTagIds && filter.excludeTagIds.length > 0) return true;
  return false;
}

/**
 * ツールバーの「フィルタ」ポップオーバーボタンに表示する適用中条件数を算出する
 * （検索テキストやサブフォルダ選択を除いたポップオーバー内フィルタ数）
 *
 * @param filter 判定対象のフィルタ状態
 * @returns ポップオーバー内の有効条件数
 */
export function countPopoverFilterConditions(filter?: TimelineFilter | null): number {
  if (!filter) return 0;
  let count = 0;
  if (filter.extensions && filter.extensions.length > 0) {
    count += filter.extensions.length;
  }
  if (filter.excludeExtensions && filter.excludeExtensions.length > 0) {
    count += filter.excludeExtensions.length;
  }
  if (filter.aspectRatio && filter.aspectRatio.trim() !== "") {
    count += 1;
  }
  if (filter.includeTagIds && filter.includeTagIds.length > 0) {
    count += filter.includeTagIds.length;
  }
  if (filter.excludeTagIds && filter.excludeTagIds.length > 0) {
    count += filter.excludeTagIds.length;
  }
  return count;
}

/**
 * ページキャッシュ (`SharedPageCacheManager`) のスコープ同期用にフィルタ条件を決定論的文字列へシリアライズする
 *
 * @param filter フィルタ状態
 * @returns 正規化されたキャッシュキー文字列（未指定時は ""）
 */
export function serializeTimelineFilter(filter?: TimelineFilter | null): string {
  if (!isTimelineFilterActive(filter) || !filter) {
    return "";
  }
  const incFmt = [...(filter.extensions ?? [])]
    .map((s) => s.toLowerCase().trim())
    .filter(Boolean)
    .sort();
  const excFmt = [...(filter.excludeExtensions ?? [])]
    .map((s) => s.toLowerCase().trim())
    .filter(Boolean)
    .sort();
  const incTags = [...(filter.includeTagIds ?? [])].sort((a, b) => a - b);
  const excTags = [...(filter.excludeTagIds ?? [])].sort((a, b) => a - b);

  return [
    `sub:${(filter.subFolderPath ?? "").trim()}`,
    `q:${(filter.query ?? "").trim()}:${(filter.queryMode ?? "and").toLowerCase()}`,
    `if:${incFmt.join(",")}`,
    `ef:${excFmt.join(",")}`,
    `ar:${(filter.aspectRatio ?? "").trim().toLowerCase()}`,
    `it:${incTags.join(",")}:${(filter.tagMode ?? "and").toLowerCase()}`,
    `et:${excTags.join(",")}`,
  ].join("|");
}
