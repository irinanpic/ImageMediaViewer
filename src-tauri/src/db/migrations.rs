use rusqlite::{Connection, Result};
use tracing::info;

/// データベースのマイグレーションを順次適用する
///
/// 変更理由: 仕様書§4および§0-4に準拠し、PRAGMA user_versionによる厳密なスキーマバージョニングを実施するため
///
/// @param conn SQLite接続（排他接続）
/// @return 成功時はOk(()), 失敗時はrusqlite::Error
pub fn apply_migrations(conn: &mut Connection) -> Result<()> {
    let current_version: i32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    info!("Current DB schema version: {}", current_version);

    if current_version < 1 {
        info!("Applying migration v1...");
        let tx = conn.transaction()?;

        // メタテーブル
        tx.execute(
            "CREATE TABLE IF NOT EXISTS meta (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
            [],
        )?;

        // 初期メタデータ設定
        tx.execute(
            "INSERT OR IGNORE INTO meta (key, value) VALUES ('thumb_spec_version', '1');",
            [],
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO meta (key, value) VALUES ('catalog_version', '1');",
            [],
        )?;

        // 監視対象フォルダテーブル
        tx.execute(
            "CREATE TABLE IF NOT EXISTS watched_folders (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                path            TEXT NOT NULL UNIQUE,
                added_at        INTEGER NOT NULL,
                last_scanned_at INTEGER
            );",
            [],
        )?;

        // 画像メタデータテーブル
        tx.execute(
            "CREATE TABLE IF NOT EXISTS images (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                folder_id       INTEGER NOT NULL REFERENCES watched_folders(id) ON DELETE CASCADE,
                file_path       TEXT NOT NULL UNIQUE,
                file_size       INTEGER NOT NULL,
                file_mtime      INTEGER NOT NULL,
                taken_at        INTEGER NOT NULL,
                taken_at_source INTEGER NOT NULL DEFAULT 0,
                width           INTEGER,
                height          INTEGER,
                orientation     INTEGER NOT NULL DEFAULT 1,
                format          TEXT,
                quick_hash      TEXT NOT NULL,
                thumb_status    INTEGER NOT NULL DEFAULT 0,
                indexed_at      INTEGER NOT NULL
            );",
            [],
        )?;

        // インデックス群
        tx.execute(
            "CREATE INDEX IF NOT EXISTS idx_images_timeline ON images (taken_at DESC, id DESC);",
            [],
        )?;
        tx.execute(
            "CREATE INDEX IF NOT EXISTS idx_images_folder ON images (folder_id);",
            [],
        )?;
        tx.execute(
            "CREATE INDEX IF NOT EXISTS idx_images_hash ON images (quick_hash);",
            [],
        )?;
        tx.execute(
            "CREATE INDEX IF NOT EXISTS idx_images_thumb_pending ON images (id) WHERE thumb_status = 0;",
            [],
        )?;

        // user_version更新
        tx.pragma_update(None, "user_version", 1)?;
        tx.commit()?;
        info!("Completed migration v1 (user_version = 1)");
    }

    if current_version < 2 {
        info!("Applying migration v2...");
        let tx = conn.transaction()?;

        // ボード（グループ）管理テーブル
        tx.execute(
            "CREATE TABLE IF NOT EXISTS boards (
                id               INTEGER PRIMARY KEY AUTOINCREMENT,
                name             TEXT NOT NULL,
                background_color TEXT NOT NULL DEFAULT '#1e1e24',
                pan_x            REAL NOT NULL DEFAULT 0.0,
                pan_y            REAL NOT NULL DEFAULT 0.0,
                zoom             REAL NOT NULL DEFAULT 1.0,
                created_at       INTEGER NOT NULL,
                updated_at       INTEGER NOT NULL
            );",
            [],
        )?;

        // ボード上の配置アイテムテーブル（多対多・非破壊変形・クリッピング）
        tx.execute(
            "CREATE TABLE IF NOT EXISTS board_items (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                board_id   INTEGER NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
                image_id   INTEGER NOT NULL REFERENCES images(id) ON DELETE CASCADE,
                x          REAL NOT NULL DEFAULT 0.0,
                y          REAL NOT NULL DEFAULT 0.0,
                width      REAL NOT NULL,
                height     REAL NOT NULL,
                scale      REAL NOT NULL DEFAULT 1.0,
                rotation   REAL NOT NULL DEFAULT 0.0,
                z_index    INTEGER NOT NULL DEFAULT 0,
                crop_x     REAL NOT NULL DEFAULT 0.0,
                crop_y     REAL NOT NULL DEFAULT 0.0,
                crop_w     REAL NOT NULL DEFAULT 1.0,
                crop_h     REAL NOT NULL DEFAULT 1.0,
                opacity    REAL NOT NULL DEFAULT 1.0,
                is_locked  INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );",
            [],
        )?;

        tx.execute(
            "CREATE INDEX IF NOT EXISTS idx_board_items_board_id ON board_items (board_id);",
            [],
        )?;
        tx.execute(
            "CREATE INDEX IF NOT EXISTS idx_board_items_image_id ON board_items (image_id);",
            [],
        )?;

        tx.pragma_update(None, "user_version", 2)?;
        tx.commit()?;
        info!("Completed migration v2 (user_version = 2)");
    }

    if current_version < 3 {
        info!("Applying migration v3...");
        let tx = conn.transaction()?;

        // フォルダ順タイムライン用複合インデックス（folder_id, taken_at DESC, id DESC）
        // 変更理由: 45,000件規模でfilesortを排除し、フォルダ順表示でのオフセット取得を即時化
        tx.execute(
            "CREATE INDEX IF NOT EXISTS idx_images_folder_timeline ON images (folder_id, taken_at DESC, id DESC);",
            [],
        )?;

        // ファイル名昇順・降順ソート用インデックス
        tx.execute(
            "CREATE INDEX IF NOT EXISTS idx_images_filepath ON images (file_path ASC, id ASC);",
            [],
        )?;
        tx.execute(
            "CREATE INDEX IF NOT EXISTS idx_images_filepath_desc ON images (file_path DESC, id DESC);",
            [],
        )?;

        tx.pragma_update(None, "user_version", 3)?;
        tx.commit()?;
        info!("Completed migration v3 (user_version = 3)");
    }

    if current_version < 4 {
        info!("Applying migration v4...");
        let tx = conn.transaction()?;

        tx.execute(
            "ALTER TABLE board_items ADD COLUMN flip_h INTEGER NOT NULL DEFAULT 0;",
            [],
        )?;
        tx.execute(
            "ALTER TABLE board_items ADD COLUMN flip_v INTEGER NOT NULL DEFAULT 0;",
            [],
        )?;

        tx.pragma_update(None, "user_version", 4)?;
        tx.commit()?;
        info!("Completed migration v4 (user_version = 4)");
    }

    if current_version < 5 {
        info!("Applying migration v5...");
        let tx = conn.transaction()?;

        tx.execute(
            "CREATE TABLE IF NOT EXISTS board_notes (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                board_id   INTEGER NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
                text       TEXT NOT NULL DEFAULT '',
                x          REAL NOT NULL DEFAULT 0.0,
                y          REAL NOT NULL DEFAULT 0.0,
                width      REAL NOT NULL DEFAULT 240.0,
                height     REAL NOT NULL DEFAULT 160.0,
                scale      REAL NOT NULL DEFAULT 1.0,
                rotation   REAL NOT NULL DEFAULT 0.0,
                z_index    INTEGER NOT NULL DEFAULT 0,
                color      TEXT NOT NULL DEFAULT '#fef08a',
                font_size  INTEGER NOT NULL DEFAULT 14,
                is_locked  INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );",
            [],
        )?;

        tx.execute(
            "CREATE INDEX IF NOT EXISTS idx_board_notes_board_id ON board_notes (board_id);",
            [],
        )?;

        tx.pragma_update(None, "user_version", 5)?;
        tx.commit()?;
        info!("Completed migration v5 (user_version = 5)");
    }

    if current_version < 6 {
        info!("Applying migration v6...");
        let tx = conn.transaction()?;

        // タイムラインしおり（ブックマーク）テーブル
        // 変更理由: 閲覧位置（日付、通し番号、スクロール位置、フォルダ、ソート順）をDBに永続化し、
        // アプリ再起動やブラウザプロファイル変更後も確実に保持するため
        tx.execute(
            "CREATE TABLE IF NOT EXISTS bookmarks (
                id          TEXT PRIMARY KEY,
                title       TEXT NOT NULL,
                folder_id   INTEGER REFERENCES watched_folders(id) ON DELETE SET NULL,
                folder_name TEXT,
                sort        TEXT NOT NULL,
                scroll_top  REAL NOT NULL,
                row_index   INTEGER NOT NULL,
                image_index INTEGER,
                day_label   TEXT NOT NULL,
                created_at  INTEGER NOT NULL
            );",
            [],
        )?;

        tx.execute(
            "CREATE INDEX IF NOT EXISTS idx_bookmarks_created ON bookmarks (created_at DESC);",
            [],
        )?;

        tx.pragma_update(None, "user_version", 6)?;
        tx.commit()?;
        info!("Completed migration v6 (user_version = 6)");
    }

    Ok(())
}
