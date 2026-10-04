import { invoke } from "@tauri-apps/api/core";
import type { AppError } from "../types/generated/AppError";
import type { ImageDetail } from "../types/generated/ImageDetail";
import type { ImageRecord } from "../types/generated/ImageRecord";
import type { GetImagesPayload } from "../types/generated/GetImagesPayload";
import type { TimelineSort } from "../types/generated/TimelineSort";
import type { TimelineSummary } from "../types/generated/TimelineSummary";
import type { WatchedFolder } from "../types/generated/WatchedFolder";
import type { WindowState } from "../types/generated/WindowState";
import type { FailedImageRecord } from "../types/generated/FailedImageRecord";
import type { LogEntry } from "../types/generated/LogEntry";
import type {
  Board,
  BoardItem,
  CreateBoardPayload,
  UpdateBoardPayload,
  UpdateBoardItemPayload,
} from "../types/board";
import { isTauriEnvironment } from "./thumbUrl";

function getBaseHttpUrl(): string {
  if (typeof window !== "undefined" && window.location && window.location.origin && window.location.origin.startsWith("http")) {
    return window.location.origin;
  }
  return "http://127.0.0.1:14201";
}

/**
 * Tauri IPC または ローカル HTTP API を呼び出す型安全ラッパー関数
 *
 * 変更理由: Tauri環境ではネイティブIPCを使用し、Electron独立ウィンドウまたは
 * Webブラウザ環境では自動的にローカルHTTPサーバーへフォールバックして全機能を提供する
 *
 * @param command コマンド名
 * @param args 引数オブジェクト
 * @returns 戻り値のPromise
 */
async function callIpc<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauriEnvironment()) {
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

  // HTTP API フォールバック
  try {
    let url = `${getBaseHttpUrl()}/api`;
    const options: RequestInit = {
      headers: { "Content-Type": "application/json" },
    };

    switch (command) {
      case "get_watch_folders":
        url += "/watch_folders";
        options.method = "GET";
        break;

      case "add_watch_folder":
        url += "/watch_folders";
        options.method = "POST";
        options.body = JSON.stringify({ path: args?.path });
        break;

      case "remove_watch_folder":
        url += `/watch_folders/${args?.id}`;
        options.method = "DELETE";
        break;

      case "rescan":
        url += "/rescan";
        options.method = "POST";
        options.body = JSON.stringify({ folder_id: args?.folderId });
        break;

      case "get_timeline_summary": {
        const folderId = args?.folderId;
        const sort = args?.sort;
        const params = new URLSearchParams();
        if (folderId !== undefined && folderId !== null) params.append("folderId", String(folderId));
        if (sort) params.append("sort", String(sort));
        const qs = params.toString();
        url += `/timeline/summary${qs ? `?${qs}` : ""}`;
        options.method = "GET";
        break;
      }


      case "get_timeline_images":
        url += "/timeline/images";
        options.method = "POST";
        options.body = JSON.stringify(args?.payload || {});
        break;

      case "get_image_detail":
        url += `/images/${args?.id}`;
        options.method = "GET";
        break;

      case "prefetch_thumbnails":
        url += "/prefetch_thumbnails";
        options.method = "POST";
        options.body = JSON.stringify({ ids: args?.ids || [] });
        break;

      case "set_viewport":
        url += "/viewport";
        options.method = "POST";
        options.body = JSON.stringify({
          visibleIds: args?.visible_ids ?? args?.visibleIds ?? [],
          nearbyIds: args?.nearby_ids ?? args?.nearbyIds ?? [],
        });
        break;

      case "clear_queue":
        url += "/clear_queue";
        options.method = "POST";
        break;

      case "rescan_missing_thumbnails":
        url += "/thumbnails/rescan_missing";
        options.method = "POST";
        break;

      case "get_thumb_progress":
        url += "/thumb_progress";
        options.method = "GET";
        break;

      case "reveal_in_file_manager":
        url += "/reveal_in_file_manager";
        options.method = "POST";
        options.body = JSON.stringify({ id: args?.id });
        break;

      case "get_window_state":
        url += "/window_state";
        options.method = "GET";
        break;

      case "save_window_state":
        url += "/window_state";
        options.method = "POST";
        options.body = JSON.stringify(args?.state);
        break;

      case "get_boards":
        url += "/boards";
        options.method = "GET";
        break;

      case "create_board":
        url += "/boards";
        options.method = "POST";
        options.body = JSON.stringify(args?.payload || {});
        break;

      case "update_board":
        url += `/boards/${args?.id}`;
        options.method = "PUT";
        options.body = JSON.stringify(args?.payload || {});
        break;

      case "delete_board":
        url += `/boards/${args?.id}`;
        options.method = "DELETE";
        break;

      case "get_board_items":
        url += `/boards/${args?.boardId}/items`;
        options.method = "GET";
        break;

      case "add_board_items":
        url += `/boards/${args?.boardId}/items`;
        options.method = "POST";
        options.body = JSON.stringify({ imageIds: args?.imageIds || [] });
        break;

      case "update_board_item":
        url += `/board_items/${args?.id}`;
        options.method = "PUT";
        options.body = JSON.stringify({ id: args?.id, ...(args?.payload || {}) });
        break;

      case "delete_board_item":
        url += `/board_items/${args?.id}`;
        options.method = "DELETE";
        break;

      case "heartbeat":
        url += "/heartbeat";
        options.method = "POST";
        break;

      case "get_failed_thumbnails": {
        const limit = args?.limit ?? 100;
        url += `/thumbnails/failed?limit=${limit}`;
        options.method = "GET";
        break;
      }

      case "get_logs": {
        const limit = args?.limit ?? 200;
        url += `/logs?limit=${limit}`;
        options.method = "GET";
        break;
      }

      case "open_log_folder":
        url += "/logs/open";
        options.method = "POST";
        break;

      default:
        throw { code: "Internal", message: `未知のコマンド: ${command}` } as AppError;
    }

    const res = await fetch(url, options);
    const data = await res.json();
    if (!res.ok) {
      throw (data as AppError);
    }
    return data as T;
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
   * サーバー生存確認（ハートビート）
   */
  checkHeartbeat: (): Promise<{ alive: boolean }> =>
    callIpc<{ alive: boolean }>("heartbeat"),

  /**
   * サーバーの稼働確認（高速軽量Ping）
   * @param timeoutMs タイムアウトミリ秒（デフォルト3000ms）
   * @returns 稼働しているかどうか
   */
  pingServer: async (timeoutMs = 3000): Promise<boolean> => {
    try {
      const controller = new AbortController();
      const timer = setTimeout(() => controller.abort(), timeoutMs);
      const res = await fetch(`${getBaseHttpUrl()}/api/heartbeat`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        signal: controller.signal,
      });
      clearTimeout(timer);
      return res.ok;
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
};

