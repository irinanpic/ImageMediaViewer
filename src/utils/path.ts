/**
 * パス操作関連のユーティリティ関数
 */

/**
 * ファイルパスからファイル名を安全に抽出する（WindowsおよびUnix/POSIX両対応）
 *
 * 変更理由: Windowsのバックスラッシュ（\）およびUnix/macOS/Android等のスラッシュ（/）の
 * 双方に対応し、プラットフォーム依存による表示崩れを防止する（DRY原則）。
 *
 * @param filePath 対象のファイルパス
 * @returns 抽出されたファイル名（空文字の場合は空文字を返す）
 */
export function getFileName(filePath?: string | null): string {
  if (!filePath) return "";
  return filePath.split(/[\\/]/).pop() || "";
}
