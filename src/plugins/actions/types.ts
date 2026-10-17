import type { ImageRecord } from "../../types/generated/ImageRecord";
import type { ImageDetail } from "../../types/generated/ImageDetail";

/**
 * 右クリックアクション実行時のコンテキスト情報
 *
 * 変更理由: アクション実行に必要な対象画像情報およびUI操作コールバックを一元提供するため。
 */
export interface ActionContext {
  /** 対象画像の軽量レコード */
  record: ImageRecord;
  /** 詳細メタデータ（取得済みの場合） */
  detail?: ImageDetail;
  /** タイムライン再描画トリガー */
  refreshTimeline?: () => void;
  /** ビューワオープン関数 */
  openViewer?: (id: number, index: number) => void;
  /** ムードボード追加モーダルオープン関数 */
  openAddToBoardModal?: (imageIds: number[]) => void;
}

/**
 * コンテキストメニューアクション拡張プラグインインターフェース
 *
 * 変更理由: 右クリックメニュー項目および実行アクションを疎結合に定義・拡張可能にするため。
 */
export interface ContextMenuActionPlugin {
  /** プラグイン一意識別子 (例: "action.open_default") */
  id: string;
  /** 表示ラベル解決関数（言語コードに応じた文字列を返却） */
  label: (locale: string) => string;
  /** アイコン識別名 */
  icon?: string;
  /** メニュー表示順序（昇順） */
  order: number;
  /** メニュー表示条件判定（利用可能であれば true） */
  isAvailable: (record: ImageRecord, enabledPlugins: string[], detail?: ImageDetail) => boolean;
  /** アクション実行処理 */
  execute: (context: ActionContext) => Promise<void>;
}
