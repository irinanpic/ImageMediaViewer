use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// タイムライン一覧表示用の軽量画像レコード
///
/// 変更理由: 仕様書§9のImageRecordインターフェースに準拠。不要なフィールドを除去しIPC転送量を最小化
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub struct ImageRecord {
    /// 画像ID (DB primary key)
    #[ts(type = "number")]
    pub id: i64,
    /// 撮影日時（壁時計時刻をUTCとみなしたUNIXエポック秒）
    #[ts(type = "number")]
    pub taken_at: i64,
    /// Orientation適用後の幅（ピクセル）
    pub width: Option<u32>,
    /// Orientation適用後の高さ（ピクセル）
    pub height: Option<u32>,
    /// 最終更新日時（ファイルmtime）。サムネイルURLのキャッシュバスターとして使用
    #[ts(type = "number")]
    pub rev: i64,
}

/// 撮影日時の取得元情報
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub enum TakenAtSource {
    Exif,
    Mtime,
}

/// タグ情報レコード
///
/// 変更理由: 画像へのタグ付与および詳細表示・タグ管理を実現するため
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub struct TagRecord {
    #[ts(type = "number")]
    pub id: i64,
    pub name: String,
    pub color: String,
    pub sort_order: i32,
    #[ts(type = "number")]
    pub created_at: i64,
}

/// 付与枚数付きのタグ情報（サイドバー・フィルタ・タグ編集モーダル用）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub struct TagWithCount {
    #[ts(type = "number")]
    pub id: i64,
    pub name: String,
    pub color: String,
    pub sort_order: i32,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub image_count: i64,
}

/// 複数選択された画像群に対するタグの付与状態
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub struct ImageTagAssignment {
    #[ts(type = "number")]
    pub tag_id: i64,
    pub name: String,
    pub color: String,
    /// "all": 対象全画像に付与済み, "some": 一部画像のみ付与済み, "none": 未付与
    pub status: String,
}

/// 監視フォルダ配下のサブフォルダ階層エントリ
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub struct SubfolderEntry {
    /// 監視ルートからの相対パス（例: "2024/Summer"）
    pub relative_path: String,
    /// 表示フォルダ名（例: "Summer"）
    pub name: String,
    /// 階層深さ（直下は 1）
    pub depth: u32,
    /// このサブフォルダおよび配下に含まれる画像総数
    #[ts(type = "number")]
    pub image_count: i64,
}

/// タイムライン検索・絞り込みフィルタ条件
///
/// 変更理由: ファイル名検索（AND/OR/NOT）、拡張子（含む/除外）、縦横比、サブフォルダ、
/// およびタグ（AND/OR/NOT）による複合高速絞り込みをタイムラインに提供するため
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub struct TimelineFilter {
    /// ファイル名検索クエリ（スペース区切り複数キーワード、`-word` でNOT除外）
    #[serde(default)]
    pub query: Option<String>,
    /// ファイル名複数キーワードの結合モード: "and" (デフォルト) | "or"
    #[serde(default)]
    pub query_mode: Option<String>,
    /// 含める拡張子リスト（小文字・ドットなし: ["png", "clip"] 等）
    #[serde(default)]
    pub extensions: Option<Vec<String>>,
    /// 除外する拡張子リスト（NOT条件）
    #[serde(default)]
    pub exclude_extensions: Option<Vec<String>>,
    /// 縦横比フィルタ: "landscape" (横長) | "portrait" (縦長) | "square" (正方形)
    #[serde(default)]
    pub aspect_ratio: Option<String>,
    /// サブフォルダ相対パス（例: "2024/Summer"）
    #[serde(default)]
    pub sub_folder_path: Option<String>,
    /// 含めるタグIDリスト
    #[serde(default)]
    #[ts(type = "number[] | null")]
    pub include_tag_ids: Option<Vec<i64>>,
    /// 除外するタグIDリスト（NOT条件）
    #[serde(default)]
    #[ts(type = "number[] | null")]
    pub exclude_tag_ids: Option<Vec<i64>>,
    /// 含めるタグの結合モード: "and" (すべて含む) | "or" (いずれか含む)
    #[serde(default)]
    pub tag_mode: Option<String>,
}

impl TimelineFilter {
    /// フィルタ条件が実質的に空（無指定）かどうかを判定する
    pub fn is_empty(&self) -> bool {
        let has_query = self
            .query
            .as_ref()
            .map(|q| !q.trim().is_empty())
            .unwrap_or(false);
        let has_exts = self
            .extensions
            .as_ref()
            .map(|v| !v.is_empty())
            .unwrap_or(false);
        let has_excl_exts = self
            .exclude_extensions
            .as_ref()
            .map(|v| !v.is_empty())
            .unwrap_or(false);
        let has_aspect = self
            .aspect_ratio
            .as_ref()
            .map(|a| matches!(a.as_str(), "landscape" | "portrait" | "square"))
            .unwrap_or(false);
        let has_sub = self
            .sub_folder_path
            .as_ref()
            .map(|s| !s.trim().trim_matches(|c| c == '/' || c == '\\').is_empty())
            .unwrap_or(false);
        let has_inc_tags = self
            .include_tag_ids
            .as_ref()
            .map(|v| !v.is_empty())
            .unwrap_or(false);
        let has_exc_tags = self
            .exclude_tag_ids
            .as_ref()
            .map(|v| !v.is_empty())
            .unwrap_or(false);

        !(has_query || has_exts || has_excl_exts || has_aspect || has_sub || has_inc_tags || has_exc_tags)
    }

    /// インメモリキャッシュ（timeline_summary_cache）のキーとして使用する正規化文字列を生成する
    pub fn cache_key(&self) -> String {
        if self.is_empty() {
            return String::new();
        }
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// 単一画像の詳細情報（詳細ビュー・情報パネル用）
///
/// 変更理由: 仕様書§9のImageDetailインターフェースに準拠し、付与済みタグ一覧（tags）を追加
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub struct ImageDetail {
    #[ts(type = "number")]
    pub id: i64,
    #[ts(type = "number")]
    pub taken_at: i64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    #[ts(type = "number")]
    pub rev: i64,
    /// 所属監視フォルダID
    #[ts(type = "number")]
    pub folder_id: i64,
    /// 元ファイルの絶対パス
    pub file_path: String,
    /// ファイルサイズ（バイト）
    #[ts(type = "number")]
    pub file_size: u64,
    /// 画像フォーマット名（'jpeg', 'png', 'webp' 等）
    pub format: Option<String>,
    /// 撮影日時ソース
    pub taken_at_source: TakenAtSource,
    /// 元ファイルが現在アクセス可能か（オフライン判定）
    pub original_available: bool,
    /// Web等から取得した元の画像URL（Webドロップ由来の場合）
    pub source_url: Option<String>,
    /// この画像に付与されているタグ一覧
    #[serde(default)]
    pub tags: Vec<TagRecord>,
}

/// サムネイル生成に失敗した画像のレコード（原因分析・ログ確認用）
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub struct FailedImageRecord {
    /// 画像ID
    #[ts(type = "number")]
    pub id: i64,
    /// 元ファイルの絶対パス
    pub file_path: String,
    /// 画像フォーマット ('png', 'webp' 等)
    pub format: Option<String>,
    /// ファイルサイズ（バイト）
    #[ts(type = "number")]
    pub file_size: u64,
}

/// ログエントリのデータ構造（ログ取得・画面表示用）
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub struct LogEntry {
    /// 発生日時文字列 ("YYYY-MM-DD HH:MM:SS.mmm")
    pub timestamp: String,
    /// ログレベル ("INFO", "WARN", "ERROR", "DEBUG", "TRACE")
    pub level: String,
    /// ログのターゲットモジュール
    pub target: String,
    /// ログメッセージ本文
    pub message: String,
}

/// 日別集計バケット（タイムラインの日付ヘッダおよび仮想スクロール高さ計算用）
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub struct DayBucket {
    /// 日付文字列 ('YYYY-MM-DD')
    pub day: String,
    /// 当該日の画像総数
    #[ts(type = "number")]
    pub count: i64,
}

/// タイムライン全体サマリ情報
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub struct TimelineSummary {
    /// 登録画像総数
    #[ts(type = "number")]
    pub total: i64,
    /// 日別バケット一覧（日付降順）
    pub buckets: Vec<DayBucket>,
    /// カタログデータバージョン（データ更新検知用）
    #[ts(type = "number")]
    pub version: i64,
}

/// フォルダの稼働状態
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub enum FolderStatus {
    Online,
    Offline,
    Scanning,
}

/// 監視対象フォルダ情報
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub struct WatchedFolder {
    /// フォルダID
    #[ts(type = "number")]
    pub id: i64,
    /// 正規化済みディレクトリパス
    pub path: String,
    /// 登録画像数
    #[ts(type = "number")]
    pub image_count: i64,
    /// サムネイル生成済み数
    #[ts(type = "number")]
    pub thumb_count: i64,
    /// 状態
    pub status: FolderStatus,
}

/// タイムラインの表示ソート順
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub enum TimelineSort {
    /// 撮影日時 降順（新しい順）
    TakenAtDesc,
    /// 撮影日時 昇順（古い順）
    TakenAtAsc,
    /// 登録フォルダ順
    Folder,
    /// ファイル名 昇順（A → Z）
    NameAsc,
    /// ファイル名 降順（Z → A）
    NameDesc,
}

/// タイムライン画像一覧取得リクエストペイロード
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub struct GetImagesPayload {
    /// 取得開始オフセット
    #[ts(type = "number")]
    pub offset: i64,
    /// 取得上限数（最大500）
    #[ts(type = "number")]
    pub limit: i64,
    /// 特定フォルダによる絞り込み（オプション）
    #[ts(type = "number | null")]
    pub folder_id: Option<i64>,
    /// 表示ソート順（オプション、デフォルトはTakenAtDesc）
    pub sort: Option<TimelineSort>,
    /// 検索・拡張子・縦横比・サブフォルダ・タグによる複合絞り込み条件（オプション）
    #[serde(default)]
    pub filter: Option<TimelineFilter>,
}


/// 走査フェーズ
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub enum ScanPhase {
    Walking,
    Indexing,
    Cleanup,
    Done,
}

/// 走査進捗イベント通知ペイロード
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub struct ScanProgress {
    #[ts(type = "number")]
    pub folder_id: i64,
    pub phase: ScanPhase,
    #[ts(type = "number")]
    pub discovered: u64,
    #[ts(type = "number")]
    pub processed: u64,
}

/// サムネイル生成進捗イベント通知ペイロード
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub struct ThumbProgress {
    #[ts(type = "number")]
    pub done: u64,
    #[ts(type = "number")]
    pub failed: u64,
    #[ts(type = "number")]
    pub total: u64,
}

/// カタログ変更通知イベントペイロード
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub struct CatalogChangedPayload {
    #[ts(type = "number")]
    pub version: i64,
}

/// アプリケーションウィンドウの表示状態（サイズ・位置・最大化）
///
/// 変更理由: アプリ再起動時に前回のウィンドウサイズや位置、最大化状態を忠実に復元するため
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub struct WindowState {
    pub width: u32,
    pub height: u32,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub is_maximized: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 850,
            x: None,
            y: None,
            is_maximized: false,
        }
    }
}

/// プラットフォームごとのプリセットフォルダ候補
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub struct PresetFolder {
    pub name: String,
    pub path: String,
    pub exists: bool,
}

/// プラットフォーム情報および推奨プリセット
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub struct PlatformInfo {
    pub os: String,
    pub is_mobile: bool,
    pub presets: Vec<PresetFolder>,
}

/// ムードボード（グループ）情報
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub struct Board {
    #[ts(type = "number")]
    pub id: i64,
    pub name: String,
    pub background_color: String,
    pub pan_x: f64,
    pub pan_y: f64,
    pub zoom: f64,
    #[ts(type = "number")]
    pub item_count: i64,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}

/// ムードボード上の配置アイテム情報（非破壊変形・クリッピング対応）
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub struct BoardItem {
    #[ts(type = "number")]
    pub id: i64,
    #[ts(type = "number")]
    pub board_id: i64,
    #[ts(type = "number")]
    pub image_id: i64,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub scale: f64,
    pub rotation: f64,
    pub z_index: i32,
    pub crop_x: f64,
    pub crop_y: f64,
    pub crop_w: f64,
    pub crop_h: f64,
    pub opacity: f64,
    pub is_locked: bool,
    #[serde(default)]
    pub flip_h: bool,
    #[serde(default)]
    pub flip_v: bool,
    #[ts(type = "number")]
    pub rev: i64,
    /// Web等から取得した元の画像URL（Webドロップ由来の場合）
    pub source_url: Option<String>,
}

/// ボード新規作成ペイロード
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub struct CreateBoardPayload {
    pub name: String,
    pub background_color: Option<String>,
}

/// ボード更新ペイロード
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub struct UpdateBoardPayload {
    pub name: Option<String>,
    pub background_color: Option<String>,
    pub pan_x: Option<f64>,
    pub pan_y: Option<f64>,
    pub zoom: Option<f64>,
}

/// ボードアイテム一括追加ペイロード
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub struct AddBoardItemsPayload {
    #[ts(type = "number")]
    pub board_id: i64,
    #[ts(type = "number[]")]
    pub image_ids: Vec<i64>,
}

/// ボードアイテム変形・クリッピング更新ペイロード
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub struct UpdateBoardItemPayload {
    #[serde(default)]
    #[ts(type = "number")]
    pub id: i64,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub scale: Option<f64>,
    pub rotation: Option<f64>,
    pub z_index: Option<i32>,
    pub crop_x: Option<f64>,
    pub crop_y: Option<f64>,
    pub crop_w: Option<f64>,
    pub crop_h: Option<f64>,
    pub opacity: Option<f64>,
    pub is_locked: Option<bool>,
    pub flip_h: Option<bool>,
    pub flip_v: Option<bool>,
}

/// ムードボード上のテキストメモ（付箋）情報
///
/// 変更理由: ムードボード上で画像だけでなくテキストによるメモ・注釈を自由配置できるようにするため
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub struct BoardNote {
    #[ts(type = "number")]
    pub id: i64,
    #[ts(type = "number")]
    pub board_id: i64,
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub scale: f64,
    pub rotation: f64,
    pub z_index: i32,
    pub color: String,
    pub font_size: i32,
    pub is_locked: bool,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}

/// メモ新規作成ペイロード
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub struct CreateBoardNotePayload {
    #[ts(type = "number")]
    pub board_id: i64,
    pub text: Option<String>,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub color: Option<String>,
    pub font_size: Option<i32>,
}

/// メモ更新ペイロード
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub struct UpdateBoardNotePayload {
    #[serde(default)]
    #[ts(type = "number")]
    pub id: i64,
    pub text: Option<String>,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub scale: Option<f64>,
    pub rotation: Option<f64>,
    pub z_index: Option<i32>,
    pub color: Option<String>,
    pub font_size: Option<i32>,
    pub is_locked: Option<bool>,
}

/// タイムラインしおり（ブックマーク）レコード
///
/// 変更理由: 閲覧位置（日付、通し番号、スクロール位置、フォルダ、ソート順）をDBに永続化するため
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub struct BookmarkRecord {
    pub id: String,
    pub title: String,
    #[ts(type = "number | null")]
    pub folder_id: Option<i64>,
    pub folder_name: Option<String>,
    pub sort: String,
    pub scroll_top: f64,
    #[ts(type = "number")]
    pub row_index: i32,
    #[ts(type = "number | null")]
    pub image_index: Option<i64>,
    pub day_label: String,
    #[ts(type = "number")]
    pub created_at: i64,
}

/// しおり作成ペイロード
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../src/types/generated/")]
pub struct CreateBookmarkPayload {
    pub id: Option<String>,
    pub title: String,
    #[ts(type = "number | null")]
    pub folder_id: Option<i64>,
    pub folder_name: Option<String>,
    pub sort: String,
    pub scroll_top: f64,
    #[ts(type = "number")]
    pub row_index: i32,
    #[ts(type = "number | null")]
    pub image_index: Option<i64>,
    pub day_label: String,
    #[ts(type = "number | null")]
    pub created_at: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_bindings_failed_image_record() {
        FailedImageRecord::export().expect("Failed to export FailedImageRecord bindings");
    }

    #[test]
    fn export_bindings_log_entry() {
        LogEntry::export().expect("Failed to export LogEntry bindings");
    }

    #[test]
    fn export_bindings_board_note() {
        BoardNote::export().expect("Failed to export BoardNote bindings");
        CreateBoardNotePayload::export().expect("Failed to export CreateBoardNotePayload bindings");
        UpdateBoardNotePayload::export().expect("Failed to export UpdateBoardNotePayload bindings");
    }

    #[test]
    fn export_bindings_bookmark() {
        BookmarkRecord::export().expect("Failed to export BookmarkRecord bindings");
        CreateBookmarkPayload::export().expect("Failed to export CreateBookmarkPayload bindings");
    }

    #[test]
    fn export_bindings_platform_info() {
        PresetFolder::export().expect("Failed to export PresetFolder bindings");
        PlatformInfo::export().expect("Failed to export PlatformInfo bindings");
    }

    #[test]
    fn export_bindings_image_detail_and_board_item() {
        ImageDetail::export().expect("Failed to export ImageDetail bindings");
        BoardItem::export().expect("Failed to export BoardItem bindings");
        TagRecord::export().expect("Failed to export TagRecord bindings");
        TagWithCount::export().expect("Failed to export TagWithCount bindings");
        ImageTagAssignment::export().expect("Failed to export ImageTagAssignment bindings");
        SubfolderEntry::export().expect("Failed to export SubfolderEntry bindings");
        TimelineFilter::export().expect("Failed to export TimelineFilter bindings");
        GetImagesPayload::export().expect("Failed to export GetImagesPayload bindings");
    }
}

