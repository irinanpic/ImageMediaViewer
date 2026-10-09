import { invoke } from "@tauri-apps/api/core";
import type { AppError } from "../types/generated/AppError";
import type { ImageDetail } from "../types/generated/ImageDetail";
import type { ImageRecord } from "../types/generated/ImageRecord";
import type { GetImagesPayload } from "../types/generated/GetImagesPayload";
import type { TimelineSort } from "../types/generated/TimelineSort";
import type { TimelineSummary } from "../types/generated/TimelineSummary";
import type { WatchedFolder } from "../types/generated/WatchedFolder";
import type { WindowState } from "../types/generated/WindowState";
import type { PlatformInfo } from "../types/generated/PlatformInfo";
import type { FailedImageRecord } from "../types/generated/FailedImageRecord";
import type { LogEntry } from "../types/generated/LogEntry";
import type {
  Board,
  BoardItem,
  BoardNote,
  CreateBoardNotePayload,
  CreateBoardPayload,
  UpdateBoardItemPayload,
  UpdateBoardNotePayload,
  UpdateBoardPayload,
} from "../types/board";
import type { Bookmark } from "../types/bookmark";
/**
 * Tauri ネイティブ IPC を呼び出す型安全ラッパー関数
 *
 * 変更理由: TCP/IPポートを一切使用しない完全ポートレス一体型アーキテクチャ。
 * Tauri Native IPC（メモリ内メッセージパイプ）により、ポート競合リスクゼロで
 * 高速かつ型安全にRustバックエンドと通信する。
 *
 * @param command コマンド名
 * @param args 引数オブジェクト
 * @returns 戻り値のPromise
 */
async function callIpc<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (err: unknown) {
    if (err && typeof err === "object" && "code" in err && "message" in err) {
      throw err as AppError;
    }
    throw {
      code: "Internal",
      message: String(err),
    } as AppError;
  }
}

/**
 * バックエンドIPC / HTTP 呼び出しAPI群
 */
export const backendApi = {
  /**
   * 監視フォルダを追加
   * @param path 対象フォルダの絶対パス
   */
  addWatchFolder: (path: string): Promise<WatchedFolder> =>
    callIpc<WatchedFolder>("add_watch_folder", { path }),

  /**
   * 監視フォルダを解除
   * @param id フォルダID
   */
  removeWatchFolder: (id: number): Promise<void> =>
    callIpc<void>("remove_watch_folder", { id }),

  /**
   * 登録済み監視フォルダ一覧を取得
   */
  getWatchFolders: (): Promise<WatchedFolder[]> =>
    callIpc<WatchedFolder[]>("get_watch_folders"),

  /**
   * フォルダを再走査
   * @param folderId 対象フォルダID（省略時は全フォルダ）
   */
  rescan: (folderId?: number): Promise<void> =>
    callIpc<void>("rescan", { folderId }),

  /**
   * タイムライン全体サマリを取得
   * @param folderId フォルダ絞り込み（オプション）
   * @param sort 表示ソート順（オプション）
   */
  getTimelineSummary: (folderId?: number, sort?: TimelineSort): Promise<TimelineSummary> =>
    callIpc<TimelineSummary>("get_timeline_summary", { folderId, sort }),


  /**
   * タイムライン用の画像一覧をページング取得
   * @param payload 取得オフセット・上限数・フォルダID
   */
  getTimelineImages: (payload: GetImagesPayload): Promise<ImageRecord[]> =>
    callIpc<ImageRecord[]>("get_timeline_images", { payload }),

  /**
   * 単一画像詳細情報を取得
   * @param id 画像ID
   */
  getImageDetail: (id: number): Promise<ImageDetail> =>
    callIpc<ImageDetail>("get_image_detail", { id }),

  /**
   * 画面外近傍サムネイルの事前生成を要求
   * @param ids 画像ID配列
   */
  prefetchThumbnails: (ids: number[]): Promise<void> =>
    callIpc<void>("prefetch_thumbnails", { ids }),

  /**
   * 表示範囲（画面内および周辺）の画像ID群をバックエンドの世代管理キューに通知
   * @param visibleIds 画面内セルID配列
   * @param nearbyIds 周辺セルID配列
   */
  setViewport: (
    visibleIds: number[],
    nearbyIds: number[]
  ): Promise<{ generation: number; visibleCount: number; nearbyCount: number }> =>
    callIpc<{ generation: number; visibleCount: number; nearbyCount: number }>("set_viewport", {
      visible_ids: visibleIds,
      nearby_ids: nearbyIds,
    }),

  /**
   * サムネイル生成キューおよび待機スレッドを無条件に一掃クリア
   */
  clearQueue: (): Promise<{ generation: number; status: string }> =>
    callIpc<{ generation: number; status: string }>("clear_queue"),

  /**
   * 未生成または過去に失敗したサムネイルの再作成・一括スキャンを開始
   */
  rescanMissingThumbnails: (): Promise<{ success: boolean; resetCount: number }> =>
    callIpc<{ success: boolean; resetCount: number }>("rescan_missing_thumbnails"),

  /**
   * サムネイル生成完了・失敗・全体件数・処理中状態を取得
   */
  getThumbProgress: (): Promise<{ done: number; failed: number; total: number; isGenerating: boolean }> =>
    callIpc<{ done: number; failed: number; total: number; isGenerating: boolean }>("get_thumb_progress"),

  /**
   * OSのファイルマネージャ（エクスプローラ）でファイルを表示
   * @param id 画像ID
   */
  revealInFileManager: (id: number): Promise<void> =>
    callIpc<void>("reveal_in_file_manager", { id }),

  /**
   * 保存されたウィンドウサイズ・位置・最大化状態を取得
   */
  getWindowState: (): Promise<WindowState> =>
    callIpc<WindowState>("get_window_state"),

  /**
   * 現在のウィンドウサイズ・位置・最大化状態を保存
   * @param state ウィンドウ状態
   */
  saveWindowState: (state: WindowState): Promise<void> =>
    callIpc<void>("save_window_state", { state }),

  /**
   * 全ムードボード一覧を取得
   */
  getBoards: (): Promise<Board[]> =>
    callIpc<Board[]>("get_boards"),

  /**
   * 新規ムードボードを作成
   * @param payload 作成ペイロード
   */
  createBoard: (payload: CreateBoardPayload): Promise<Board> =>
    callIpc<Board>("create_board", { payload }),

  /**
   * ボード設定・カメラ位置を更新
   * @param id ボードID
   * @param payload 更新ペイロード
   */
  updateBoard: (id: number, payload: UpdateBoardPayload): Promise<{ success: boolean }> =>
    callIpc<{ success: boolean }>("update_board", { id, payload }),

  /**
   * ボードを削除
   * @param id ボードID
   */
  deleteBoard: (id: number): Promise<{ success: boolean }> =>
    callIpc<{ success: boolean }>("delete_board", { id }),

  /**
   * 指定ボードの配置アイテム一覧を取得
   * @param boardId ボードID
   */
  getBoardItems: (boardId: number): Promise<BoardItem[]> =>
    callIpc<BoardItem[]>("get_board_items", { boardId }),

  /**
   * ボードに画像アイテムを一括追加
   * @param boardId ボードID
   * @param imageIds 画像ID配列
   */
  addBoardItems: (boardId: number, imageIds: number[]): Promise<BoardItem[]> =>
    callIpc<BoardItem[]>("add_board_items", { boardId, imageIds }),

  /**
   * ボードアイテムの変形・位置・クリッピングを更新
   * @param id アイテムID
   * @param payload 更新ペイロード
   */
  updateBoardItem: (id: number, payload: UpdateBoardItemPayload): Promise<{ success: boolean }> =>
    callIpc<{ success: boolean }>("update_board_item", { id, payload }),

  /**
   * ボードアイテムを削除
   * @param id アイテムID
   */
  deleteBoardItem: (id: number): Promise<{ success: boolean }> =>
    callIpc<{ success: boolean }>("delete_board_item", { id }),

  /**
   * 指定ボードのメモ（付箋）一覧を取得
   * @param boardId ボードID
   */
  getBoardNotes: (boardId: number): Promise<BoardNote[]> =>
    callIpc<BoardNote[]>("get_board_notes", { boardId }),

  /**
   * ボードに新規メモを作成
   * @param payload メモ作成ペイロード
   */
  createBoardNote: (payload: CreateBoardNotePayload): Promise<BoardNote> =>
    callIpc<BoardNote>("create_board_note", { boardId: payload.boardId, payload }),

  /**
   * ボードメモの更新（テキスト、位置、サイズ、カラー、ロック等）
   * @param id メモID
   * @param payload 更新ペイロード
   */
  updateBoardNote: (id: number, payload: UpdateBoardNotePayload): Promise<{ success: boolean }> =>
    callIpc<{ success: boolean }>("update_board_note", { id, payload }),

  /**
   * ボードメモを削除
   * @param id メモID
   */
  deleteBoardNote: (id: number): Promise<{ success: boolean }> =>
    callIpc<{ success: boolean }>("delete_board_note", { id }),

  /**
   * タイムラインのしおり（ブックマーク）一覧を取得
   */
  getBookmarks: (): Promise<Bookmark[]> =>
    callIpc<Bookmark[]>("get_bookmarks"),

  /**
   * タイムラインのしおりを新規登録・永続化
   * @param payload しおりデータ
   */
  createBookmark: (payload: Omit<Bookmark, "createdAt"> & { createdAt?: number }): Promise<Bookmark> =>
    callIpc<Bookmark>("create_bookmark", { payload }),

  /**
   * タイムラインのしおりを削除
   * @param id しおりID
   */
  deleteBookmark: (id: string): Promise<{ success: boolean }> =>
    callIpc<{ success: boolean }>("delete_bookmark", { id }),

  /**
   * サーバー生存確認（ハートビート）
   */
  checkHeartbeat: (): Promise<{ alive: boolean }> =>
    callIpc<{ alive: boolean }>("heartbeat"),

  /**
   * サーバーの稼働確認（高速軽量Ping）
   * @param _timeoutMs タイムアウトミリ秒（互換性のためのオプショナル引数）
   * @returns 稼働しているかどうか
   */
  pingServer: async (_timeoutMs?: number): Promise<boolean> => {
    try {
      const res = await invoke<{ alive: boolean }>("heartbeat");
      return Boolean(res.alive);
    } catch {
      return false;
    }
  },

  /**
   * サムネイル生成に失敗した画像の一覧を取得
   * @param limit 取得件数上限（デフォルト100）
   */
  getFailedThumbnails: (limit = 100): Promise<FailedImageRecord[]> =>
    callIpc<FailedImageRecord[]>("get_failed_thumbnails", { limit }),

  /**
   * 直近のログ一覧を取得
   * @param limit 取得件数上限（デフォルト200）
   */
  getLogs: (limit = 200): Promise<LogEntry[]> =>
    callIpc<LogEntry[]>("get_logs", { limit }),

  /**
   * ログ保存先ディレクトリをOSのファイルマネージャで開く
   */
  openLogFolder: (): Promise<{ success: boolean; path: string }> =>
    callIpc<{ success: boolean; path: string }>("open_log_folder"),

  /**
   * バックエンドの現在の言語設定を取得
   */
  getLocale: (): Promise<string> =>
    callIpc<string>("get_locale"),

  /**
   * バックエンドの言語設定を更新し、タスクトレイメニューも同期更新
   * @param locale 言語コード ("ja" | "en")
   */
  setLocale: (locale: string): Promise<string> =>
    callIpc<string>("set_locale", { locale }),

  /**
   * プラットフォーム情報および端末の推奨画像フォルダ候補を取得
   */
  getPlatformInfo: (): Promise<PlatformInfo> =>
    callIpc<PlatformInfo>("get_platform_info"),
};

