/**
 * ユーザー定義のカスタムコンテキストメニュー項目
 *
 * 変更理由: 任意の外部アプリケーション連携を設定ファイルから登録可能にするため。
 * モバイル環境（Android）との互換性を確保するため、デフォルトでは環境依存の項目を配置せず空配列とする。
 */
export interface CustomContextMenuItem {
  /** 項目の一意識別子 */
  id: string;
  /** メニューの表示名 */
  name: string;
  /** 起動対象アプリケーションのパスまたはコマンド名 */
  appPath: string;
  /** コマンドライン引数テンプレート（省略時は "{path}"） */
  args?: string;
  /** 対象ファイル拡張子リスト（空の場合はすべての拡張子に適用） */
  targetExtensions?: string[];
  /** メニュー有効化フラグ */
  enabled: boolean;
}

/**
 * アプリケーション設定データ構造
 *
 * 変更理由: タスクトレイ常駐設定、言語、有効化プラグイン一覧、カスタムメニュー項目を一元管理するため。
 */
export interface AppSettings {
  /** メインウィンドウ「×」ボタン時にタスクトレイへ常駐するか */
  stay_in_tray: boolean;
  /** 言語設定 ("ja" | "en") */
  locale: string;
  /** 明示的に有効化されたプラグインID一覧 */
  enabled_plugins: string[];
  /** ユーザー定義のカスタムコンテキストメニュー項目 */
  custom_context_menu_items: CustomContextMenuItem[];
}
