/**
 * 監視フォルダのサムネイル処理状態
 */
export type FolderThumbnailState = "empty" | "generating" | "warningDone" | "allDone";

/**
 * 監視フォルダのサムネイル進捗・完了状態を判定する
 *
 * 変更理由: パスワード付きZIPや0バイトファイル等の処理不可ファイルが存在する場合に、
 * フォルダステータスが永遠に「生成中」で停滞するのを防ぎ、「警告終了」として正しく識別するため。
 *
 * @param imageCount フォルダ内の登録画像総数
 * @param thumbCount サムネイル生成成功数
 * @param thumbFailedCount サムネイル生成失敗数（スキップ件数）
 * @return 判定された状態（"empty" | "generating" | "warningDone" | "allDone"）
 */
export function getFolderThumbnailState(
  imageCount: number,
  thumbCount?: number,
  thumbFailedCount?: number
): FolderThumbnailState {
  if (imageCount <= 0) {
    return "empty";
  }

  const success = thumbCount ?? 0;
  const failed = thumbFailedCount ?? 0;
  const processed = success + failed;

  if (processed < imageCount) {
    return "generating";
  }

  if (failed > 0) {
    return "warningDone";
  }

  return "allDone";
}

/**
 * 「すべての写真」全体のサムネイル進捗・完了状態を判定する
 *
 * @param totalImages 登録画像総数
 * @param thumbProgress サムネイル進捗情報
 * @return 判定された状態（"empty" | "generating" | "warningDone" | "allDone"）
 */
export function getAllPhotosThumbnailState(
  totalImages: number,
  thumbProgress: { done: number; failed: number; total: number } | null
): FolderThumbnailState {
  if (totalImages <= 0) {
    return "empty";
  }

  if (!thumbProgress || thumbProgress.total <= 0) {
    return "empty";
  }

  const processed = thumbProgress.done + (thumbProgress.failed ?? 0);

  if (processed < thumbProgress.total) {
    return "generating";
  }

  if (thumbProgress.failed > 0) {
    return "warningDone";
  }

  return "allDone";
}
