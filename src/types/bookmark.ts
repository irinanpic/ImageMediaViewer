import type { TimelineSort } from "./generated/TimelineSort";

/**
 * タイムライン表示位置のしおり（ブックマーク）情報
 *
 * 変更理由: 閲覧中のタイムライン位置（日付、通し番号、スクロール位置、フォルダ、ソート順）を
 * 記録・永続化し、いつでもワンクリックでその位置へ復帰できるようにするため。
 */
export interface Bookmark {
  id: string;
  title: string;
  folderId: number | null;
  folderName?: string;
  sort: TimelineSort;
  scrollTop: number;
  rowIndex: number;
  imageIndex: number | null;
  dayLabel: string;
  createdAt: number;
}
