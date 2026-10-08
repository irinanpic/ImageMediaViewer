import type { TranslationKey } from "./index";

/**
 * ログメッセージのマッチングルール定義
 */
interface LogMessagePattern {
  /** 日本語および英語のメッセージに一致する正規表現 */
  patterns: RegExp[];
  /** 対応する翻訳キー */
  key: TranslationKey;
  /** 正規表現のキャプチャグループからパラメータオブジェクトを生成するマッパー */
  extractParams?: (match: RegExpMatchArray) => Record<string, string | number>;
}

/**
 * 主要なシステムログメッセージのパターン定義
 *
 * 変更理由: バックエンド（Rust）がどちらの言語でログを出力していても、
 * また過去に保存されたログファイル（app.log）の内容であっても、
 * フロントエンドUI（LogViewerModal）上で現在の選択言語（ja/en）へ動的・透過的に多言語翻訳して表示するため。
 */
const LOG_PATTERNS: LogMessagePattern[] = [
  {
    patterns: [
      /ImageMediaViewer 初期化開始 \(ログ先: "(.+?)", 常駐設定: (true|false)\)/,
      /ImageMediaViewer initialization started \(Logs: "(.+?)", Resident: (true|false)\)/,
    ],
    key: "logMessages.initStarting",
    extractParams: (m) => ({ logs: m[1], resident: m[2] }),
  },
  {
    patterns: [
      /現在のDBスキーマバージョン: (\d+)/,
      /Current DB schema version: (\d+)/,
    ],
    key: "logMessages.schemaVersion",
    extractParams: (m) => ({ version: Number(m[1]) }),
  },
  {
    patterns: [
      /データベースを正常に初期化しました: "(.+?)"/,
      /Database initialized successfully: "(.+?)"/,
      /Database initialized successfully: (.+)/,
    ],
    key: "logMessages.dbInitSuccess",
    extractParams: (m) => ({ path: m[1] }),
  },
  {
    patterns: [
      /サムネイル専用DBを正常に初期化しました: "(.+?)"/,
      /Thumbnail DB initialized successfully: "(.+?)"/,
      /Thumbnail DB initialized successfully: (.+)/,
    ],
    key: "logMessages.thumbDbInitSuccess",
    extractParams: (m) => ({ path: m[1] }),
  },
  {
    patterns: [
      /サムネイル生成ワーカースレッド起動: (\d+) スレッド（バックグラウンド上限 (\d+)）/,
      /Thumbnail worker threads started: (\d+) threads \(Background limit: (\d+)\)/,
    ],
    key: "logMessages.workerThreadsStarted",
    extractParams: (m) => ({ threads: Number(m[1]), limit: Number(m[2]) }),
  },
  {
    patterns: [
      /システムトレイアイコンを登録しました（常駐初期状態: (true|false)）/,
      /System tray icon registered \(Resident default: (true|false)\)/,
    ],
    key: "logMessages.trayRegistered",
    extractParams: (m) => ({ resident: m[1] }),
  },
  {
    patterns: [
      /言語設定を更新しました: locale = (\w+)/,
      /Language updated: locale = (\w+)/,
    ],
    key: "logMessages.localeUpdated",
    extractParams: (m) => ({ locale: m[1] }),
  },
  {
    patterns: [
      /二重起動を検知しました。既存のメインウィンドウを表示・復元します。/,
      /二重起動を検知しました: 既存ウィンドウを最前面に復元します/,
      /Detected second instance: Restoring existing window to front/,
    ],
    key: "logMessages.secondInstanceDetected",
  },
  {
    patterns: [
      /メインウィンドウを非表示にし、タスクトレイ常駐に移行しました/,
      /Hidden main window and transitioned to system tray/,
    ],
    key: "logMessages.windowHiddenToTray",
  },
  {
    patterns: [
      /常駐設定がOFFのため、ウィンドウ終了に伴いアプリケーションを完全終了します/,
      /常駐設定がOFFのため、アプリケーションを完全終了します/,
      /Resident setting is OFF; Terminating application on window close/,
    ],
    key: "logMessages.windowClosedExit",
  },
  {
    patterns: [
      /トレイメニューから終了が指示されました。アプリケーションを完全終了します。/,
      /Exit requested from tray menu\. Terminating application\./,
    ],
    key: "logMessages.trayMenuExit",
  },
  {
    patterns: [
      /トレイメニューから再走査を要求されました/,
      /Rescan requested from tray menu/,
    ],
    key: "logMessages.trayMenuRescan",
  },
  {
    patterns: [
      /常駐設定を変更しました: stay_in_tray = (true|false)/,
      /Resident setting changed: stay_in_tray = (true|false)/,
    ],
    key: "logMessages.stayInTrayChanged",
    extractParams: (m) => ({ value: m[1] }),
  },
  {
    patterns: [
      /フォルダ走査中にエラーが発生しました: (.+)/,
      /Error during folder scanning: (.+)/,
    ],
    key: "logMessages.folderScanError",
    extractParams: (m) => ({ error: m[1] }),
  },
  {
    patterns: [
      /サムネイルURIから画像IDを抽出できませんでした: (.+)/,
      /Invalid thumbnail URI: (.+)/,
    ],
    key: "logMessages.thumbnailUriInvalid",
    extractParams: (m) => ({ uri: m[1] }),
  },
  {
    patterns: [
      /サムネイル対象の画像レコードが存在しません: id=(\d+)/,
      /Thumbnail target image record not found: id=(\d+)/,
    ],
    key: "logMessages.thumbnailRecordNotFound",
    extractParams: (m) => ({ id: Number(m[1]) }),
  },
  {
    patterns: [
      /サムネイル元の原寸画像ファイルが存在しません: path=(.+)/,
      /Thumbnail original file not found: path=(.+)/,
    ],
    key: "logMessages.thumbnailOriginalMissing",
    extractParams: (m) => ({ path: m[1] }),
  },
  {
    patterns: [
      /サムネイルの生成に失敗しました: id=(\d+), path=(.+)/,
      /Failed to generate thumbnail: id=(\d+), path=(.+)/,
    ],
    key: "logMessages.thumbnailGenFailed",
    extractParams: (m) => ({ id: Number(m[1]), path: m[2] }),
  },
  {
    patterns: [
      /原寸画像URIから画像IDを抽出できませんでした: (.+)/,
      /Invalid original image URI: (.+)/,
    ],
    key: "logMessages.originalUriInvalid",
    extractParams: (m) => ({ uri: m[1] }),
  },
  {
    patterns: [
      /原寸画像のレコードが存在しません: id=(\d+)/,
      /Original image record not found: id=(\d+)/,
    ],
    key: "logMessages.originalRecordNotFound",
    extractParams: (m) => ({ id: Number(m[1]) }),
  },
  {
    patterns: [
      /原寸画像ファイルが存在しません: path=(.+)/,
      /Original image file not found: path=(.+)/,
    ],
    key: "logMessages.originalFileMissing",
    extractParams: (m) => ({ path: m[1] }),
  },
  {
    patterns: [
      /原寸画像読み取り失敗: (.+?), error: (.+)/,
      /Failed to read original image: (.+?), error: (.+)/,
    ],
    key: "logMessages.originalReadFailed",
    extractParams: (m) => ({ path: m[1], error: m[2] }),
  },
  {
    patterns: [
      /過去の生成失敗画像 (\d+) 件を再生成対象にリセットしました/,
      /過去の生成失敗フラグをリセットしました \(対象: (\d+) 件\)/,
      /Reset thumbnail failure flags \(Targets: (\d+)\)/,
    ],
    key: "logMessages.resetFailureFlags",
    extractParams: (m) => ({ count: Number(m[1]) }),
  },
  {
    patterns: [
      /アイドル状態検知: 登録フォルダの省電力バックグラウンド更新チェックを開始します/,
      /Idle detected: Starting background folder update check/,
    ],
    key: "logMessages.idleScanStarted",
  },
  {
    patterns: [
      /ユーザー操作・ジョブ発生を検知: バックグラウンドチェックを中断します/,
      /User action or jobs detected: Suspending background check/,
    ],
    key: "logMessages.idleScanInterrupted",
  },
  {
    patterns: [
      /省電力バックグラウンドフォルダ更新チェック完了/,
      /Background folder update check finished/,
    ],
    key: "logMessages.idleScanFinished",
  },
  {
    patterns: [
      /フォルダ \[(.+?)\] で差分対象画像 (\d+) 件を発見/,
      /Found (\d+) changed images in \[(.+?)\]/,
    ],
    key: "logMessages.folderDiffDiscovered",
    extractParams: (m) => {
      // 英語と日本語でキャプチャ順が異なる場合のハンドリング
      if (isNaN(Number(m[1]))) {
        return { folder: m[1], count: Number(m[2]) };
      }
      return { folder: m[2], count: Number(m[1]) };
    },
  },
  {
    patterns: [
      /孤立サムネイル GC: (\d+) 件の不要なBLOBを回収しました/,
      /Orphaned thumbnails GC: Cleaned up (\d+) unreferenced BLOBs/,
    ],
    key: "logMessages.orphanedThumbnailsGc",
    extractParams: (m) => ({ count: Number(m[1]) }),
  },
  {
    patterns: [
      /マイグレーション v(\d+) を適用中\.\.\./,
      /Applying migration v(\d+)\.\.\./,
    ],
    key: "logMessages.migrationApplying",
    extractParams: (m) => ({ version: Number(m[1]) }),
  },
  {
    patterns: [
      /マイグレーション v(\d+) の適用完了 \(user_version = \d+\)/,
      /Completed migration v(\d+) \(user_version = \d+\)/,
    ],
    key: "logMessages.migrationCompleted",
    extractParams: (m) => ({ version: Number(m[1]) }),
  },
];

/**
 * ログメッセージを現在選択中の言語に翻訳・整形する
 *
 * @param rawMessage バックエンドやログファイルから取得された生のメッセージ文字列
 * @param t 翻訳関数
 * @returns 翻訳されたメッセージ（一致するパターンがない場合は生のメッセージを返却）
 */
export function translateLogMessage(
  rawMessage: string,
  t: (key: TranslationKey, params?: Record<string, string | number>) => string
): string {
  if (!rawMessage) return "";

  for (const item of LOG_PATTERNS) {
    for (const pattern of item.patterns) {
      const match = rawMessage.match(pattern);
      if (match) {
        const params = item.extractParams ? item.extractParams(match) : undefined;
        return t(item.key, params);
      }
    }
  }

  // 既知パターンに一致しない動的メッセージはそのまま表示
  return rawMessage;
}
