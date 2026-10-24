use rusqlite::{params, Connection, Result};
use crate::models::{DayBucket, FolderStatus, ImageDetail, ImageRecord, TakenAtSource, TimelineSort, TimelineSummary, WatchedFolder};
use std::path::Path;


/// 新規登録または更新用の画像メタデータ構造体
#[derive(Debug, Clone)]
pub struct NewImageRecord {
    pub folder_id: i64,
    pub file_path: String,
    pub file_size: u64,
    pub file_mtime: i64,
    pub taken_at: i64,
    pub taken_at_source: i32,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub orientation: u32,
    pub format: Option<String>,
    pub quick_hash: String,
    pub indexed_at: i64,
    /// Webドロップ等で取得した元の画像URL
    pub source_url: Option<String>,
}

/// 監視対象フォルダを追加
///
/// @param conn DB書き込み接続
/// @param path 正規化された絶対パス
/// @param added_at 追加日時UNIX秒
/// @return 登録されたWatchedFolder
pub fn add_watched_folder(conn: &Connection, path: &str, added_at: i64) -> Result<WatchedFolder> {
    conn.execute(
        "INSERT INTO watched_folders (path, added_at, last_scanned_at) VALUES (?1, ?2, NULL);",
        params![path, added_at],
    )?;
    let id = conn.last_insert_rowid();
    Ok(WatchedFolder {
        id,
        path: path.to_string(),
        image_count: 0,
        thumb_count: 0,
        status: FolderStatus::Online,
    })
}

/// 監視対象フォルダを削除
///
/// 変更理由: ON DELETE CASCADEにより紐づくimagesレコードも一括削除されるが、
/// ムードボード（board_items）上に配置されている画像は消去せずスタンドアロン（folder_id = 0）へ退避する。
///
/// @param conn DB書き込み接続
/// @param id フォルダID
/// @return 成功時Ok(())
pub fn remove_watched_folder(conn: &Connection, id: i64) -> Result<()> {
    if id <= 0 {
        return Ok(());
    }
    conn.execute(
        "UPDATE images SET folder_id = 0 WHERE folder_id = ?1 AND EXISTS (SELECT 1 FROM board_items bi WHERE bi.image_id = images.id);",
        params![id],
    )?;
    conn.execute("DELETE FROM watched_folders WHERE id = ?1 AND id > 0;", params![id])?;
    Ok(())
}

/// 単一の画像レコードをID指定で削除する
///
/// 変更理由: ゴミ箱移動（trash_file）時にDBから該当画像メタデータを確実にパージするため
///
/// @param conn DB書き込み接続
/// @param id 画像ID
/// @return 成功時Ok(())
pub fn delete_image(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM images WHERE id = ?1;", params![id])?;
    Ok(())
}

/// 登録済み監視フォルダ一覧を取得（システム予約の id = 0 は除外）
///
/// @param conn DB接続
/// @return フォルダ一覧
pub fn get_watched_folders(conn: &Connection) -> Result<Vec<WatchedFolder>> {
    let mut stmt = conn.prepare_cached(
        "SELECT f.id, f.path, COUNT(i.id) AS cnt,
                COALESCE(SUM(CASE WHEN i.thumb_status = 1 THEN 1 ELSE 0 END), 0) AS thumb_cnt
         FROM watched_folders f
         LEFT JOIN images i ON f.id = i.folder_id
         WHERE f.id > 0
         GROUP BY f.id, f.path
         ORDER BY f.id ASC;",
    )?;

    let rows = stmt.query_map([], |row| {
        let path: String = row.get(1)?;
        let is_online = Path::new(&path).exists();
        let count: i64 = row.get(2)?;
        let thumb_count: i64 = row.get(3)?;
        Ok(WatchedFolder {
            id: row.get(0)?,
            path,
            image_count: count,
            thumb_count,
            status: if is_online {
                FolderStatus::Online
            } else {
                FolderStatus::Offline
            },
        })
    })?;

    let mut folders = Vec::new();
    for folder in rows {
        folders.push(folder?);
    }
    Ok(folders)
}

/// フォルダの最終走査日時を更新
pub fn update_last_scanned(conn: &Connection, id: i64, scanned_at: i64) -> Result<()> {
    conn.execute(
        "UPDATE watched_folders SET last_scanned_at = ?1 WHERE id = ?2;",
        params![scanned_at, id],
    )?;
    Ok(())
}

/// 画像メタデータをバッチUPSERT
///
/// 変更理由: 仕様書§6「500〜1,000件ごとにバッチUPSERT (ON CONFLICT(file_path) DO UPDATE)」
///
/// @param conn DB書き込み接続
/// @param records 登録対象レコード一覧
/// @return 成功時Ok(())
pub fn upsert_images(conn: &mut Connection, records: &[NewImageRecord]) -> Result<()> {
    if records.is_empty() {
        return Ok(());
    }

    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare_cached(
            "INSERT INTO images (
                folder_id, file_path, file_size, file_mtime, taken_at,
                taken_at_source, width, height, orientation, format,
                quick_hash, thumb_status, indexed_at, source_url
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0, ?12, ?13
             )
             ON CONFLICT(file_path) DO UPDATE SET
                folder_id = CASE WHEN excluded.folder_id > 0 THEN excluded.folder_id ELSE images.folder_id END,
                file_size = excluded.file_size,
                file_mtime = excluded.file_mtime,
                taken_at = excluded.taken_at,
                taken_at_source = excluded.taken_at_source,
                width = excluded.width,
                height = excluded.height,
                orientation = excluded.orientation,
                format = excluded.format,
                quick_hash = excluded.quick_hash,
                thumb_status = CASE WHEN images.file_mtime <> excluded.file_mtime THEN 0 ELSE images.thumb_status END,
                indexed_at = excluded.indexed_at,
                source_url = COALESCE(excluded.source_url, images.source_url);",
        )?;

        for r in records {
            stmt.execute(params![
                r.folder_id,
                r.file_path,
                r.file_size as i64,
                r.file_mtime,
                r.taken_at,
                r.taken_at_source,
                r.width,
                r.height,
                r.orientation,
                r.format,
                r.quick_hash,
                r.indexed_at,
                r.source_url,
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// タイムライン用の画像一覧をページング取得
///
/// 変更理由: 仕様書§4「ORDER BY は必ず (taken_at DESC, id DESC)」
///
/// タイムライン用の画像一覧をページング取得
///
/// 変更理由: 仕様書§4「表示ソート順に応じたページング取得対応」
///
/// @param conn DB読み取り接続
/// @param offset オフセット
/// @param limit 取得件数
/// @param folder_id フォルダ絞り込み（オプション）
/// @param sort_by 表示ソート順（オプション、デフォルトはTakenAtDesc）
/// @return ImageRecordの一覧
pub fn get_timeline_images(
    conn: &Connection,
    offset: i64,
    limit: i64,
    folder_id: Option<i64>,
    sort_by: Option<TimelineSort>,
) -> Result<Vec<ImageRecord>> {
    let sort = sort_by.unwrap_or(TimelineSort::TakenAtDesc);
    let order_sql = match sort {
        TimelineSort::TakenAtDesc => "ORDER BY taken_at DESC, id DESC",
        TimelineSort::TakenAtAsc => "ORDER BY taken_at ASC, id ASC",
        TimelineSort::Folder => "ORDER BY folder_id ASC, taken_at DESC, id DESC",
        TimelineSort::NameAsc => "ORDER BY file_path ASC, id ASC",
        TimelineSort::NameDesc => "ORDER BY file_path DESC, id DESC",
    };

    let mut images = Vec::new();

    if let Some(fid) = folder_id {
        let sql = format!(
            "SELECT id, taken_at, width, height, file_mtime
             FROM images
             WHERE folder_id = ?1
             {}
             LIMIT ?2 OFFSET ?3;",
            order_sql
        );
        let mut stmt = conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(params![fid, limit, offset], |row| {
            Ok(ImageRecord {
                id: row.get(0)?,
                taken_at: row.get(1)?,
                width: row.get(2)?,
                height: row.get(3)?,
                rev: row.get(4)?,
            })
        })?;
        for img in rows {
            images.push(img?);
        }
    } else {
        let sql = format!(
            "SELECT id, taken_at, width, height, file_mtime
             FROM images
             WHERE folder_id > 0
             {}
             LIMIT ?1 OFFSET ?2;",
            order_sql
        );
        let mut stmt = conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(params![limit, offset], |row| {
            Ok(ImageRecord {
                id: row.get(0)?,
                taken_at: row.get(1)?,
                width: row.get(2)?,
                height: row.get(3)?,
                rev: row.get(4)?,
            })
        })?;
        for img in rows {
            images.push(img?);
        }
    }

    Ok(images)
}

/// セクション別件数サマリと総件数を取得
///
/// 変更理由: 仕様書§4「表示ソート順に応じたセクションバケット（日付またはフォルダ単位）集計」
/// スタンドアロン画像（folder_id = 0）はタイムライン表示対象外とする。
///
/// @param conn DB読み取り接続
/// @param folder_id フォルダ絞り込み
/// @param version カタログバージョン
/// @param sort_by 表示ソート順（オプション）
/// @return TimelineSummary
pub fn get_timeline_summary(
    conn: &Connection,
    folder_id: Option<i64>,
    version: i64,
    sort_by: Option<TimelineSort>,
) -> Result<TimelineSummary> {
    let sort = sort_by.unwrap_or(TimelineSort::TakenAtDesc);
    let mut buckets = Vec::new();
    let total: i64;

    match sort {
        TimelineSort::Folder => {
            // フォルダ順: 各フォルダごとにバケットを作成
            total = if let Some(fid) = folder_id {
                conn.query_row("SELECT COUNT(*) FROM images WHERE folder_id = ?1;", params![fid], |row| row.get(0))?
            } else {
                conn.query_row("SELECT COUNT(*) FROM images WHERE folder_id > 0;", [], |row| row.get(0))?
            };

            if let Some(fid) = folder_id {
                let mut stmt = conn.prepare_cached(
                    "SELECT f.path, COUNT(i.id) AS cnt
                     FROM watched_folders f
                     INNER JOIN images i ON f.id = i.folder_id
                     WHERE f.id = ?1
                     GROUP BY f.id, f.path
                     HAVING cnt > 0
                     ORDER BY f.id ASC;",
                )?;
                let rows = stmt.query_map(params![fid], |row| {
                    let path: String = row.get(0)?;
                    let count: i64 = row.get(1)?;
                    Ok(DayBucket { day: path, count })
                })?;
                for b in rows {
                    buckets.push(b?);
                }
            } else {
                let mut stmt = conn.prepare_cached(
                    "SELECT f.path, COUNT(i.id) AS cnt
                     FROM watched_folders f
                     INNER JOIN images i ON f.id = i.folder_id
                     WHERE f.id > 0
                     GROUP BY f.id, f.path
                     HAVING cnt > 0
                     ORDER BY f.id ASC;",
                )?;
                let rows = stmt.query_map([], |row| {
                    let path: String = row.get(0)?;
                    let count: i64 = row.get(1)?;
                    Ok(DayBucket { day: path, count })
                })?;
                for b in rows {
                    buckets.push(b?);
                }
            }
        }
        TimelineSort::TakenAtAsc => {
            total = if let Some(fid) = folder_id {
                conn.query_row("SELECT COUNT(*) FROM images WHERE folder_id = ?1;", params![fid], |row| row.get(0))?
            } else {
                conn.query_row("SELECT COUNT(*) FROM images WHERE folder_id > 0;", [], |row| row.get(0))?
            };

            if let Some(fid) = folder_id {
                let mut stmt = conn.prepare_cached(
                    "SELECT strftime('%Y-%m-%d', taken_at, 'unixepoch') AS day, COUNT(*) AS cnt
                     FROM images
                     WHERE folder_id = ?1
                     GROUP BY day
                     ORDER BY day ASC;",
                )?;
                let rows = stmt.query_map(params![fid], |row| {
                    let day: Option<String> = row.get(0)?;
                    let day_str = day.unwrap_or_else(|| "1970-01-01".to_string());
                    let count: i64 = row.get(1)?;
                    Ok(DayBucket { day: day_str, count })
                })?;
                for b in rows {
                    buckets.push(b?);
                }
            } else {
                let mut stmt = conn.prepare_cached(
                    "SELECT strftime('%Y-%m-%d', taken_at, 'unixepoch') AS day, COUNT(*) AS cnt
                     FROM images
                     WHERE folder_id > 0
                     GROUP BY day
                     ORDER BY day ASC;",
                )?;
                let rows = stmt.query_map([], |row| {
                    let day: Option<String> = row.get(0)?;
                    let day_str = day.unwrap_or_else(|| "1970-01-01".to_string());
                    let count: i64 = row.get(1)?;
                    Ok(DayBucket { day: day_str, count })
                })?;
                for b in rows {
                    buckets.push(b?);
                }
            }
        }
        TimelineSort::NameAsc | TimelineSort::NameDesc => {
            total = if let Some(fid) = folder_id {
                conn.query_row("SELECT COUNT(*) FROM images WHERE folder_id = ?1;", params![fid], |row| row.get(0))?
            } else {
                conn.query_row("SELECT COUNT(*) FROM images WHERE folder_id > 0;", [], |row| row.get(0))?
            };

            let title = if sort == TimelineSort::NameAsc {
                "ファイル名 (A → Z)"
            } else {
                "ファイル名 (Z → A)"
            };
            if total > 0 {
                buckets.push(DayBucket {
                    day: title.to_string(),
                    count: total,
                });
            }
        }
        TimelineSort::TakenAtDesc => {
            total = if let Some(fid) = folder_id {
                conn.query_row("SELECT COUNT(*) FROM images WHERE folder_id = ?1;", params![fid], |row| row.get(0))?
            } else {
                conn.query_row("SELECT COUNT(*) FROM images WHERE folder_id > 0;", [], |row| row.get(0))?
            };

            if let Some(fid) = folder_id {
                let mut stmt = conn.prepare_cached(
                    "SELECT strftime('%Y-%m-%d', taken_at, 'unixepoch') AS day, COUNT(*) AS cnt
                     FROM images
                     WHERE folder_id = ?1
                     GROUP BY day
                     ORDER BY day DESC;",
                )?;
                let rows = stmt.query_map(params![fid], |row| {
                    let day: Option<String> = row.get(0)?;
                    let day_str = day.unwrap_or_else(|| "1970-01-01".to_string());
                    let count: i64 = row.get(1)?;
                    Ok(DayBucket { day: day_str, count })
                })?;
                for b in rows {
                    buckets.push(b?);
                }
            } else {
                let mut stmt = conn.prepare_cached(
                    "SELECT strftime('%Y-%m-%d', taken_at, 'unixepoch') AS day, COUNT(*) AS cnt
                     FROM images
                     WHERE folder_id > 0
                     GROUP BY day
                     ORDER BY day DESC;",
                )?;
                let rows = stmt.query_map([], |row| {
                    let day: Option<String> = row.get(0)?;
                    let day_str = day.unwrap_or_else(|| "1970-01-01".to_string());
                    let count: i64 = row.get(1)?;
                    Ok(DayBucket { day: day_str, count })
                })?;
                for b in rows {
                    buckets.push(b?);
                }
            }
        }
    }

    Ok(TimelineSummary {
        total,
        buckets,
        version,
    })
}


/// 単一画像の詳細情報を取得
pub fn get_image_detail(conn: &Connection, id: i64) -> Result<Option<ImageDetail>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, taken_at, width, height, file_mtime, folder_id, file_path,
                file_size, format, taken_at_source, source_url
         FROM images
         WHERE id = ?1;",
    )?;

    let mut rows = stmt.query(params![id])?;
    if let Some(row) = rows.next()? {
        let file_path: String = row.get(6)?;
        let exists = Path::new(&file_path).exists();
        let src_int: i32 = row.get(9)?;
        let taken_at_source = if src_int == 0 {
            TakenAtSource::Exif
        } else {
            TakenAtSource::Mtime
        };

        Ok(Some(ImageDetail {
            id: row.get(0)?,
            taken_at: row.get(1)?,
            width: row.get(2)?,
            height: row.get(3)?,
            rev: row.get(4)?,
            folder_id: row.get(5)?,
            file_path,
            file_size: row.get::<_, i64>(7)? as u64,
            format: row.get(8)?,
            taken_at_source,
            original_available: exists,
            source_url: row.get(10)?,
        }))
    } else {
        Ok(None)
    }
}

/// IDからファイルパス、クイックハッシュ、mtime、フォーマットを取得（URIプロトコル配信用）
pub fn get_image_file_info(
    conn: &Connection,
    id: i64,
) -> Result<Option<(String, String, i64, Option<String>)>> {
    let mut stmt = conn.prepare_cached(
        "SELECT file_path, quick_hash, file_mtime, format FROM images WHERE id = ?1;",
    )?;
    let mut rows = stmt.query(params![id])?;
    if let Some(row) = rows.next()? {
        Ok(Some((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
    } else {
        Ok(None)
    }
}

/// サムネイル生成に必要な画像情報
#[derive(Debug, Clone)]
pub struct ThumbSource {
    pub id: i64,
    pub file_path: String,
    pub file_size: u64,
    pub file_mtime: i64,
    pub quick_hash: String,
    pub orientation: u32,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// 0: 未生成, 1: 生成済, 2: 生成失敗
    pub thumb_status: i32,
}

fn map_thumb_source(row: &rusqlite::Row<'_>) -> Result<ThumbSource> {
    Ok(ThumbSource {
        id: row.get(0)?,
        file_path: row.get(1)?,
        file_size: row.get::<_, i64>(2)? as u64,
        file_mtime: row.get(3)?,
        quick_hash: row.get(4)?,
        orientation: row.get::<_, Option<u32>>(5)?.unwrap_or(1),
        width: row.get(6)?,
        height: row.get(7)?,
        thumb_status: row.get(8)?,
    })
}

const THUMB_SOURCE_COLUMNS: &str =
    "id, file_path, file_size, file_mtime, quick_hash, orientation, width, height, thumb_status";

/// 単一画像のサムネイル生成用情報を取得
///
/// @param conn DB接続
/// @param id 画像ID
/// @return 該当があれば ThumbSource
pub fn get_thumb_source(conn: &Connection, id: i64) -> Result<Option<ThumbSource>> {
    let sql = format!("SELECT {} FROM images WHERE id = ?1;", THUMB_SOURCE_COLUMNS);
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut rows = stmt.query(params![id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(map_thumb_source(row)?))
    } else {
        Ok(None)
    }
}

/// 複数画像のサムネイル生成用情報を一括取得
///
/// 変更理由: 表示範囲（数百件）の状態確認・優先投入を1クエリで行い、往復コストを抑えるため
///
/// @param conn DB接続
/// @param ids 画像ID配列（最大数百件程度を想定、内部で500件ずつ分割）
/// @return 取得できた ThumbSource の配列（順不同）
pub fn get_thumb_sources(conn: &Connection, ids: &[i64]) -> Result<Vec<ThumbSource>> {
    let mut out = Vec::with_capacity(ids.len());
    for chunk in ids.chunks(500) {
        let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT {} FROM images WHERE id IN ({});",
            THUMB_SOURCE_COLUMNS, placeholders
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(chunk.iter()), map_thumb_source)?;
        for r in rows {
            out.push(r?);
        }
    }
    Ok(out)
}

/// サムネイル生成未完了（thumb_status != 1: 未生成0または過去の失敗2）の画像を、指定ID以降から取得
///
/// 変更理由: バックグラウンド補充生成で全未完了画像を順次処理するためのカーソル方式取得。
/// 過去に失敗（2）した画像も高速化パイプラインで自動再作成できるように thumb_status != 1 を対象とする
///
/// @param conn DB接続
/// @param after_id このIDより大きいものを対象
/// @param limit 最大件数
pub fn get_pending_thumb_sources(
    conn: &Connection,
    after_id: i64,
    limit: usize,
) -> Result<Vec<ThumbSource>> {
    // 変更理由: 失敗（thumb_status = 2）した画像がバックグラウンドで無限に再投入されて
    // CPUやログを浪費するのを防止するため、未処理（0）のみを対象とする。
    // 手動再作成時は reset_failed_thumbnails で 0 に戻るため再試行可能。
    let sql = format!(
        "SELECT {} FROM images WHERE thumb_status = 0 AND id > ?1 ORDER BY id ASC LIMIT ?2;",
        THUMB_SOURCE_COLUMNS
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(params![after_id, limit as i64], map_thumb_source)?;
    let mut items = Vec::new();
    for item in rows {
        items.push(item?);
    }
    Ok(items)
}

/// サムネイル生成未完了（thumb_status = 0）の画像ID一覧を取得
pub fn get_pending_thumb_images(
    conn: &Connection,
    limit: usize,
) -> Result<Vec<(i64, String, String)>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, file_path, quick_hash FROM images WHERE thumb_status = 0 ORDER BY id ASC LIMIT ?1;",
    )?;
    let rows = stmt.query_map(params![limit as i64], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })?;
    let mut items = Vec::new();
    for item in rows {
        items.push(item?);
    }
    Ok(items)
}

/// 過去に生成失敗（thumb_status = 2）となった画像のステータスを未生成（0）に一括リセット
///
/// 変更理由: 高速化パイプライン（EXIF内蔵サムネイル最優先抽出）導入に伴い、
/// 過去にタイムアウトや失敗で止まっていたデジカメ画像等を一括で再作成可能にするため
pub fn reset_failed_thumbnails(conn: &Connection) -> Result<usize> {
    let count = conn.execute(
        "UPDATE images SET thumb_status = 0 WHERE thumb_status = 2;",
        [],
    )?;
    Ok(count)
}

/// サムネイル生成ステータスを更新
/// サムネイル生成ステータスを更新する（実際に値が変更された場合のみ true を返す）
///
/// 変更理由: 既に thumb_status = 1 の画像に対して viewport 表示時などで再更新が
/// 走った際に、無駄な進捗カウンタ二重加算（thumb_done の不整合な肥大化）を防止するため。
///
/// @param conn DB接続
/// @param id 画像ID
/// @param status 新ステータス（0: 未生成, 1: 成功, 2: 失敗）
/// @return 実際にステータスが変更された場合は true
pub fn update_thumb_status(conn: &Connection, id: i64, status: i32) -> Result<bool> {
    let affected = conn.execute(
        "UPDATE images SET thumb_status = ?1 WHERE id = ?2 AND thumb_status != ?1;",
        params![status, id],
    )?;
    Ok(affected > 0)
}

/// サムネイル生成ステータスを一括更新する（実際に変更された件数を返す）
///
/// 変更理由: バックグラウンド生成（Low優先度）時にトランザクションを1回に集約し、
/// SQLiteの排他ロック時間とWAL書き込み回数を最小化するため。
///
/// @param conn DB書き込み接続
/// @param success_ids 生成成功した画像IDリスト
/// @param failed_ids 生成失敗した画像IDリスト
/// @return (実成功遷移数, 実失敗遷移数)
pub fn update_thumb_status_batch(
    conn: &mut Connection,
    success_ids: &[i64],
    failed_ids: &[i64],
) -> Result<(usize, usize)> {
    if success_ids.is_empty() && failed_ids.is_empty() {
        return Ok((0, 0));
    }
    let tx = conn.transaction()?;
    let mut actual_success = 0;
    let mut actual_failed = 0;
    {
        if !success_ids.is_empty() {
            let mut stmt = tx.prepare_cached("UPDATE images SET thumb_status = 1 WHERE id = ?1 AND thumb_status != 1;")?;
            for &id in success_ids {
                if stmt.execute(params![id])? > 0 {
                    actual_success += 1;
                }
            }
        }
        if !failed_ids.is_empty() {
            let mut stmt = tx.prepare_cached("UPDATE images SET thumb_status = 2 WHERE id = ?1 AND thumb_status != 2;")?;
            for &id in failed_ids {
                if stmt.execute(params![id])? > 0 {
                    actual_failed += 1;
                }
            }
        }
    }
    tx.commit()?;
    Ok((actual_success, actual_failed))
}

/// サムネイル生成完了・失敗・全体数を集計（タイムライン対象の folder_id > 0 のみ集計）
///
/// 変更理由: 完了（1）だけでなく失敗件数（2）も取得し、
/// UIステータスバーで「進まない」と誤認されるのを防ぎ、失敗件数を明確にユーザーへ提示する
///
/// @return (完了数, 失敗数, 総画像数)
pub fn get_thumb_progress(conn: &Connection) -> Result<(u64, u64, u64)> {
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM images WHERE folder_id > 0;", [], |r| r.get(0))?;
    let done: i64 = conn.query_row("SELECT COUNT(*) FROM images WHERE folder_id > 0 AND thumb_status = 1;", [], |r| r.get(0))?;
    let failed: i64 = conn.query_row("SELECT COUNT(*) FROM images WHERE folder_id > 0 AND thumb_status = 2;", [], |r| r.get(0))?;
    Ok((done as u64, failed as u64, total as u64))
}

/// サムネイル生成に失敗した画像の一覧を取得（folder_id > 0 の監視フォルダ配下に限定）
///
/// 変更理由: get_thumb_progress の集計条件（folder_id > 0）と統一し、
/// ステータスバー件数とログモーダル一覧件数の整合性を保証するため。
///
/// @param limit 上限件数
/// @return 失敗した画像レコード配列
pub fn get_failed_thumb_images(conn: &Connection, limit: usize) -> Result<Vec<crate::models::FailedImageRecord>> {
    let sql = "SELECT id, file_path, format, file_size FROM images WHERE folder_id > 0 AND thumb_status = 2 ORDER BY id ASC LIMIT ?1;";
    let mut stmt = conn.prepare_cached(sql)?;
    let rows = stmt.query_map(params![limit as i64], |row| {
        Ok(crate::models::FailedImageRecord {
            id: row.get(0)?,
            file_path: row.get(1)?,
            format: row.get(2)?,
            file_size: row.get::<_, i64>(3)? as u64,
        })
    })?;
    let mut items = Vec::new();
    for item in rows {
        items.push(item?);
    }
    Ok(items)
}

/// ファイル名、ファイルサイズ、更新日時から既存の画像レコードを検索
///
/// 変更理由: ローカルファイルD&D時に既存の登録済み画像（監視フォルダ配下等）を特定し、
/// 不要なファイル複製を防止するため。
///
/// @param conn DB接続
/// @param filename ファイル名（例: "sample.png"）
/// @param file_size ファイルサイズ（バイト）
/// @param file_mtime 最終更新日時（UNIXエポック秒、オプション照合用）
/// @return 一致した (id, file_path)
pub fn find_image_by_filename_and_size(
    conn: &Connection,
    filename: &str,
    file_size: u64,
    file_mtime: Option<i64>,
) -> Result<Option<(i64, String)>> {
    let pattern1 = format!("%/{}", filename);
    let pattern2 = format!("%\\{}", filename);

    let mut stmt = conn.prepare_cached(
        "SELECT id, file_path, file_mtime FROM images
         WHERE file_size = ?1 AND (file_path LIKE ?2 OR file_path LIKE ?3 OR file_path = ?4);",
    )?;

    let mut rows = stmt.query(params![file_size as i64, pattern1, pattern2, filename])?;
    let mut candidates = Vec::new();
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let path: String = row.get(1)?;
        let mtime: i64 = row.get(2)?;
        candidates.push((id, path, mtime));
    }

    if candidates.is_empty() {
        return Ok(None);
    }

    // 更新日時が一致する候補があれば最優先
    if let Some(target_mtime) = file_mtime {
        if let Some((id, path, _)) = candidates.iter().find(|(_, _, m)| (*m - target_mtime).abs() <= 2) {
            return Ok(Some((*id, path.clone())));
        }
    }

    let (id, path, _) = candidates.remove(0);
    Ok(Some((id, path)))
}

/// カタログバージョンを取得
pub fn get_catalog_version(conn: &Connection) -> Result<i64> {
    let val: String = conn.query_row(
        "SELECT value FROM meta WHERE key = 'catalog_version';",
        [],
        |r| r.get(0),
    )?;
    Ok(val.parse::<i64>().unwrap_or(1))
}

/// カタログバージョンをインクリメント
pub fn increment_catalog_version(conn: &Connection) -> Result<i64> {
    let current = get_catalog_version(conn)?;
    let new_val = current + 1;
    conn.execute(
        "UPDATE meta SET value = ?1 WHERE key = 'catalog_version';",
        params![new_val.to_string()],
    )?;
    Ok(new_val)
}

// -----------------------------------------------------------------------------
// ムードボード（グループ & キャンバス）操作
// -----------------------------------------------------------------------------

/// 全ボード一覧を取得（所属アイテム数も併せて集計）
///
/// 変更理由: 閲覧・カメラ位置の保存（pan/zoom）等によって `updated_at` が更新されても、
/// サイドバーの一覧順序が勝手に跳ね上がる挙動を防止するため、作成順（`created_at ASC, id ASC`）で安定取得する。
pub fn get_boards(conn: &Connection) -> Result<Vec<crate::models::Board>> {
    let mut stmt = conn.prepare_cached(
        "SELECT b.id, b.name, b.background_color, b.pan_x, b.pan_y, b.zoom,
                COUNT(bi.id) AS item_count, b.created_at, b.updated_at
         FROM boards b
         LEFT JOIN board_items bi ON b.id = bi.board_id
         GROUP BY b.id
         ORDER BY b.created_at ASC, b.id ASC;"
    )?;

    let rows = stmt.query_map([], |r| {
        Ok(crate::models::Board {
            id: r.get(0)?,
            name: r.get(1)?,
            background_color: r.get(2)?,
            pan_x: r.get(3)?,
            pan_y: r.get(4)?,
            zoom: r.get(5)?,
            item_count: r.get(6)?,
            created_at: r.get(7)?,
            updated_at: r.get(8)?,
        })
    })?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// 新規ボードを作成
pub fn create_board(
    conn: &Connection,
    payload: &crate::models::CreateBoardPayload,
    now: i64,
) -> Result<crate::models::Board> {
    let bg = payload.background_color.as_deref().unwrap_or("#1e1e24");
    conn.execute(
        "INSERT INTO boards (name, background_color, pan_x, pan_y, zoom, created_at, updated_at)
         VALUES (?1, ?2, 0.0, 0.0, 1.0, ?3, ?4);",
        params![payload.name, bg, now, now],
    )?;

    let id = conn.last_insert_rowid();
    Ok(crate::models::Board {
        id,
        name: payload.name.clone(),
        background_color: bg.to_string(),
        pan_x: 0.0,
        pan_y: 0.0,
        zoom: 1.0,
        item_count: 0,
        created_at: now,
        updated_at: now,
    })
}

/// ボードの設定・カメラ位置を更新
pub fn update_board(
    conn: &Connection,
    id: i64,
    payload: &crate::models::UpdateBoardPayload,
    now: i64,
) -> Result<()> {
    if let Some(ref name) = payload.name {
        conn.execute("UPDATE boards SET name = ?1, updated_at = ?2 WHERE id = ?3;", params![name, now, id])?;
    }
    if let Some(ref bg) = payload.background_color {
        conn.execute("UPDATE boards SET background_color = ?1, updated_at = ?2 WHERE id = ?3;", params![bg, now, id])?;
    }
    if let (Some(px), Some(py), Some(z)) = (payload.pan_x, payload.pan_y, payload.zoom) {
        conn.execute(
            "UPDATE boards SET pan_x = ?1, pan_y = ?2, zoom = ?3, updated_at = ?4 WHERE id = ?5;",
            params![px, py, z, now, id],
        )?;
    }
    Ok(())
}

/// ボードを削除（所属アイテムも CASCADE で自動削除）
pub fn delete_board(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM boards WHERE id = ?1;", params![id])?;
    Ok(())
}

/// 指定ボード上のアイテム一覧を取得
pub fn get_board_items(conn: &Connection, board_id: i64) -> Result<Vec<crate::models::BoardItem>> {
    let mut stmt = conn.prepare_cached(
        "SELECT bi.id, bi.board_id, bi.image_id, bi.x, bi.y, bi.width, bi.height,
                bi.scale, bi.rotation, bi.z_index, bi.crop_x, bi.crop_y, bi.crop_w, bi.crop_h,
                bi.opacity, bi.is_locked, bi.flip_h, bi.flip_v, i.file_mtime, i.source_url
         FROM board_items bi
         JOIN images i ON bi.image_id = i.id
         WHERE bi.board_id = ?1
         ORDER BY bi.z_index ASC, bi.id ASC;"
    )?;

    let rows = stmt.query_map(params![board_id], |r| {
        Ok(crate::models::BoardItem {
            id: r.get(0)?,
            board_id: r.get(1)?,
            image_id: r.get(2)?,
            x: r.get(3)?,
            y: r.get(4)?,
            width: r.get(5)?,
            height: r.get(6)?,
            scale: r.get(7)?,
            rotation: r.get(8)?,
            z_index: r.get(9)?,
            crop_x: r.get(10)?,
            crop_y: r.get(11)?,
            crop_w: r.get(12)?,
            crop_h: r.get(13)?,
            opacity: r.get(14)?,
            is_locked: r.get::<_, i32>(15)? != 0,
            flip_h: r.get::<_, i32>(16).unwrap_or(0) != 0,
            flip_v: r.get::<_, i32>(17).unwrap_or(0) != 0,
            rev: r.get(18)?,
            source_url: r.get(19)?,
        })
    })?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// ボードに複数画像を追加（グリッド状または横並びに初期配置）
pub fn add_board_items(
    conn: &Connection,
    board_id: i64,
    image_ids: &[i64],
    now: i64,
) -> Result<Vec<crate::models::BoardItem>> {
    add_board_items_at(conn, board_id, image_ids, None, None, now)
}

/// ボードの指定座標に複数画像を追加（座標未指定時は (40.0, 40.0) を起点にグリッド配置）
///
/// 変更理由: ムードボードへの直接ドラッグ＆ドロップやクリップボード貼り付け時に、
/// ドロップ位置・ポインタ位置を起点として画像アイテムを配置できるようにするため。
///
/// @param conn DB書き込み接続
/// @param board_id ボードID
/// @param image_ids 追加する画像ID配列
/// @param base_x 配置起点のX座標（オプション）
/// @param base_y 配置起点のY座標（オプション）
/// @param now 現在時刻UNIX秒
/// @return ボード上の全アイテム一覧
pub fn add_board_items_at(
    conn: &Connection,
    board_id: i64,
    image_ids: &[i64],
    base_x: Option<f64>,
    base_y: Option<f64>,
    now: i64,
) -> Result<Vec<crate::models::BoardItem>> {
    let max_z: i32 = conn
        .query_row(
            "SELECT COALESCE(MAX(z_index), 0) FROM (
                SELECT z_index FROM board_items WHERE board_id = ?1
                UNION ALL
                SELECT z_index FROM board_notes WHERE board_id = ?1
            );",
            params![board_id],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let start_x = base_x.unwrap_or(40.0);
    let start_y = base_y.unwrap_or(40.0);
    let center_first = base_x.is_some() && base_y.is_some();

    let mut current_z = max_z + 1;
    let mut offset_x = start_x;
    let mut offset_y = start_y;

    for (idx, &img_id) in image_ids.iter().enumerate() {
        // 画像の元解像度を取得（なければ標準280px）
        let (w, h) = conn
            .query_row(
                "SELECT width, height FROM images WHERE id = ?1;",
                params![img_id],
                |r| Ok((r.get::<_, Option<u32>>(0)?, r.get::<_, Option<u32>>(1)?)),
            )
            .unwrap_or((None, None));

        let width = 280.0;
        let height = match (w, h) {
            (Some(orig_w), Some(orig_h)) if orig_w > 0 => (orig_h as f64 / orig_w as f64) * width,
            _ => 280.0,
        };

        // 座標指定時は1枚目の中心が指定座標になるよう調整
        let item_x = if center_first && idx == 0 && image_ids.len() == 1 {
            start_x - width / 2.0
        } else {
            offset_x
        };
        let item_y = if center_first && idx == 0 && image_ids.len() == 1 {
            start_y - height / 2.0
        } else {
            offset_y
        };

        conn.execute(
            "INSERT INTO board_items (
                board_id, image_id, x, y, width, height, scale, rotation, z_index,
                crop_x, crop_y, crop_w, crop_h, opacity, is_locked, created_at, updated_at
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, 1.0, 0.0, ?7,
                0.0, 0.0, 1.0, 1.0, 1.0, 0, ?8, ?9
            );",
            params![board_id, img_id, item_x, item_y, width, height, current_z, now, now],
        )?;

        current_z += 1;
        offset_x += width + 24.0;
        if (idx + 1) % 4 == 0 {
            offset_x = start_x;
            offset_y += 320.0;
        }
    }

    conn.execute("UPDATE boards SET updated_at = ?1 WHERE id = ?2;", params![now, board_id])?;
    get_board_items(conn, board_id)
}

/// 指定ファイルパスが既存の監視フォルダ（id > 0）配下に属するか判定し、該当すればその folder_id、なければ 0 を返す
///
/// 変更理由: ムードボードへ直接ドロップされた画像が既に監視対象フォルダ内の画像であれば
/// そのフォルダに紐づけ、監視外の画像であればスタンドアロン（folder_id = 0）として登録するため。
///
/// @param conn DB接続
/// @param file_path 正規化された画像ファイルパス
/// @return 該当する folder_id（該当なしは 0）
pub fn find_matching_watched_folder_id(conn: &Connection, file_path: &str) -> Result<i64> {
    let folders = get_watched_folders(conn)?;
    let target = Path::new(file_path);
    let mut best_match: Option<(i64, usize)> = None;

    for f in folders {
        let folder_path = Path::new(&f.path);
        if target.starts_with(folder_path) {
            let len = f.path.len();
            if best_match.map_or(true, |(_, best_len)| len > best_len) {
                best_match = Some((f.id, len));
            }
        }
    }

    Ok(best_match.map(|(id, _)| id).unwrap_or(0))
}

/// 単一の画像レコードを登録または更新し、その画像IDを返す
///
/// 変更理由: ムードボードへの直接ファイルドロップやクリップボード貼り付け時に、
/// 1件ずつまた少数件の画像を即時DB登録して image_id を取得するため。
///
/// @param conn DB書き込み接続
/// @param record 登録する画像メタデータ
/// @return 登録または既存レコードの images.id
pub fn upsert_single_image_and_get_id(conn: &Connection, record: &NewImageRecord) -> Result<i64> {
    conn.execute(
        "INSERT INTO images (
            folder_id, file_path, file_size, file_mtime, taken_at,
            taken_at_source, width, height, orientation, format,
            quick_hash, thumb_status, indexed_at, source_url
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0, ?12, ?13
         )
         ON CONFLICT(file_path) DO UPDATE SET
            folder_id = CASE WHEN excluded.folder_id > 0 THEN excluded.folder_id ELSE images.folder_id END,
            file_size = excluded.file_size,
            file_mtime = excluded.file_mtime,
            taken_at = excluded.taken_at,
            taken_at_source = excluded.taken_at_source,
            width = excluded.width,
            height = excluded.height,
            orientation = excluded.orientation,
            format = excluded.format,
            quick_hash = excluded.quick_hash,
            thumb_status = CASE WHEN images.file_mtime <> excluded.file_mtime THEN 0 ELSE images.thumb_status END,
            indexed_at = excluded.indexed_at,
            source_url = COALESCE(excluded.source_url, images.source_url);",
        params![
            record.folder_id,
            record.file_path,
            record.file_size as i64,
            record.file_mtime,
            record.taken_at,
            record.taken_at_source,
            record.width,
            record.height,
            record.orientation,
            record.format,
            record.quick_hash,
            record.indexed_at,
            record.source_url,
        ],
    )?;

    let id: i64 = conn.query_row(
        "SELECT id FROM images WHERE file_path = ?1;",
        params![record.file_path],
        |r| r.get(0),
    )?;
    Ok(id)
}

/// クイックハッシュとファイルサイズから既存の画像IDを検索
///
/// 変更理由: Webクリップ等で同一画像が既にDB（監視フォルダ等）に存在する場合に重複登録を避けるため
/// @param conn DB接続
/// @param quick_hash クイックハッシュ
/// @param file_size ファイルサイズ（バイト）
/// @return 既存画像ID（存在する場合）
pub fn find_image_by_hash(
    conn: &Connection,
    quick_hash: &str,
    file_size: u64,
) -> Result<Option<i64>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id FROM images WHERE quick_hash = ?1 AND file_size = ?2 LIMIT 1;",
    )?;
    let mut rows = stmt.query(params![quick_hash, file_size as i64])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row.get(0)?))
    } else {
        Ok(None)
    }
}

/// どのボードからも参照されなくなったクリップボード由来画像（folder_id = 0）をDBから削除し、削除対象情報を返す
///
/// 変更理由: クリップボードから直接貼り付けられた一時保存画像がムードボード上から削除された際、
/// 不要なDBレコード・サムネイル・内部保存ファイルが蓄積するのを防止するため。
///
/// @param conn DB書き込み接続
/// @param clipboard_dir クリップボード画像保存ディレクトリ
/// @return 削除された (image_id, file_path, quick_hash) の一覧
pub fn cleanup_orphan_clipboard_images(
    conn: &Connection,
    clipboard_dir: &Path,
) -> Result<Vec<(i64, String, String)>> {
    let mut stmt = conn.prepare_cached(
        "SELECT i.id, i.file_path, i.quick_hash
         FROM images i
         WHERE i.folder_id = 0
           AND NOT EXISTS (SELECT 1 FROM board_items bi WHERE bi.image_id = i.id);"
    )?;

    let rows = stmt.query_map([], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
    })?;

    let mut to_delete = Vec::new();
    for row in rows {
        let (id, file_path, quick_hash) = row?;
        if Path::new(&file_path).starts_with(clipboard_dir) {
            to_delete.push((id, file_path, quick_hash));
        }
    }

    for (id, _, _) in &to_delete {
        conn.execute("DELETE FROM images WHERE id = ?1;", params![id])?;
    }

    Ok(to_delete)
}

/// ボードアイテムの変形・位置・クリッピングを更新
pub fn update_board_item(
    conn: &Connection,
    payload: &crate::models::UpdateBoardItemPayload,
    now: i64,
) -> Result<()> {
    let mut updates = Vec::new();
    let mut vals: Vec<rusqlite::types::Value> = Vec::new();

    if let Some(x) = payload.x {
        updates.push("x = ?");
        vals.push(rusqlite::types::Value::Real(x));
    }
    if let Some(y) = payload.y {
        updates.push("y = ?");
        vals.push(rusqlite::types::Value::Real(y));
    }
    if let Some(w) = payload.width {
        updates.push("width = ?");
        vals.push(rusqlite::types::Value::Real(w));
    }
    if let Some(h) = payload.height {
        updates.push("height = ?");
        vals.push(rusqlite::types::Value::Real(h));
    }
    if let Some(s) = payload.scale {
        updates.push("scale = ?");
        vals.push(rusqlite::types::Value::Real(s));
    }
    if let Some(r) = payload.rotation {
        updates.push("rotation = ?");
        vals.push(rusqlite::types::Value::Real(r));
    }
    if let Some(z) = payload.z_index {
        updates.push("z_index = ?");
        vals.push(rusqlite::types::Value::Integer(z as i64));
    }
    if let Some(cx) = payload.crop_x {
        updates.push("crop_x = ?");
        vals.push(rusqlite::types::Value::Real(cx));
    }
    if let Some(cy) = payload.crop_y {
        updates.push("crop_y = ?");
        vals.push(rusqlite::types::Value::Real(cy));
    }
    if let Some(cw) = payload.crop_w {
        updates.push("crop_w = ?");
        vals.push(rusqlite::types::Value::Real(cw));
    }
    if let Some(ch) = payload.crop_h {
        updates.push("crop_h = ?");
        vals.push(rusqlite::types::Value::Real(ch));
    }
    if let Some(op) = payload.opacity {
        updates.push("opacity = ?");
        vals.push(rusqlite::types::Value::Real(op));
    }
    if let Some(l) = payload.is_locked {
        updates.push("is_locked = ?");
        vals.push(rusqlite::types::Value::Integer(if l { 1 } else { 0 }));
    }
    if let Some(fh) = payload.flip_h {
        updates.push("flip_h = ?");
        vals.push(rusqlite::types::Value::Integer(if fh { 1 } else { 0 }));
    }
    if let Some(fv) = payload.flip_v {
        updates.push("flip_v = ?");
        vals.push(rusqlite::types::Value::Integer(if fv { 1 } else { 0 }));
    }

    if updates.is_empty() {
        return Ok(());
    }

    updates.push("updated_at = ?");
    vals.push(rusqlite::types::Value::Integer(now));

    let sql = format!(
        "UPDATE board_items SET {} WHERE id = ?;",
        updates.join(", ")
    );
    vals.push(rusqlite::types::Value::Integer(payload.id));

    let params_from_iter = rusqlite::params_from_iter(vals);
    conn.execute(&sql, params_from_iter)?;
    Ok(())
}

/// ボードアイテムを削除
pub fn delete_board_item(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM board_items WHERE id = ?1;", params![id])?;
    Ok(())
}

/// 指定ボード上のメモ一覧を取得
///
/// 変更理由: ムードボード上に配置されたテキストメモ（付箋）を読み出すため
///
/// @param conn DB接続
/// @param board_id ボードID
/// @return メモ一覧 (z_index 順)
pub fn get_board_notes(conn: &Connection, board_id: i64) -> Result<Vec<crate::models::BoardNote>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, board_id, text, x, y, width, height, scale, rotation, z_index,
                color, font_size, is_locked, created_at, updated_at
         FROM board_notes
         WHERE board_id = ?1
         ORDER BY z_index ASC, id ASC;"
    )?;

    let rows = stmt.query_map(params![board_id], |r| {
        Ok(crate::models::BoardNote {
            id: r.get(0)?,
            board_id: r.get(1)?,
            text: r.get(2)?,
            x: r.get(3)?,
            y: r.get(4)?,
            width: r.get(5)?,
            height: r.get(6)?,
            scale: r.get(7)?,
            rotation: r.get(8)?,
            z_index: r.get(9)?,
            color: r.get(10)?,
            font_size: r.get(11)?,
            is_locked: r.get::<_, i32>(12)? != 0,
            created_at: r.get(13)?,
            updated_at: r.get(14)?,
        })
    })?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// ボードに新規メモを作成
///
/// 変更理由: ムードボード上に新しい付箋・テキストメモを配置するため
///
/// @param conn DB接続
/// @param payload メモ作成データ
/// @param now 作成日時エポック秒
/// @return 作成された BoardNote
pub fn create_board_note(
    conn: &Connection,
    payload: &crate::models::CreateBoardNotePayload,
    now: i64,
) -> Result<crate::models::BoardNote> {
    let max_z: i32 = conn
        .query_row(
            "SELECT COALESCE(MAX(z_index), 0) FROM (
                SELECT z_index FROM board_items WHERE board_id = ?1
                UNION ALL
                SELECT z_index FROM board_notes WHERE board_id = ?1
            );",
            params![payload.board_id],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let z_index = max_z + 1;
    let text = payload.text.clone().unwrap_or_default();
    let x = payload.x.unwrap_or(40.0);
    let y = payload.y.unwrap_or(40.0);
    let width = payload.width.unwrap_or(240.0);
    let height = payload.height.unwrap_or(160.0);
    let color = payload.color.clone().unwrap_or_else(|| "#fef08a".to_string());
    let font_size = payload.font_size.unwrap_or(14);

    conn.execute(
        "INSERT INTO board_notes (
            board_id, text, x, y, width, height, scale, rotation, z_index,
            color, font_size, is_locked, created_at, updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, 1.0, 0.0, ?7,
            ?8, ?9, 0, ?10, ?11
        );",
        params![
            payload.board_id,
            text,
            x,
            y,
            width,
            height,
            z_index,
            color,
            font_size,
            now,
            now
        ],
    )?;

    let id = conn.last_insert_rowid();
    conn.execute("UPDATE boards SET updated_at = ?1 WHERE id = ?2;", params![now, payload.board_id])?;

    Ok(crate::models::BoardNote {
        id,
        board_id: payload.board_id,
        text,
        x,
        y,
        width,
        height,
        scale: 1.0,
        rotation: 0.0,
        z_index,
        color,
        font_size,
        is_locked: false,
        created_at: now,
        updated_at: now,
    })
}

/// ボードメモの更新
///
/// 変更理由: メモのテキスト内容、位置、サイズ、カラー、ロック状態等の変更を保存するため
///
/// @param conn DB接続
/// @param payload 更新内容
/// @param now 更新日時エポック秒
pub fn update_board_note(
    conn: &Connection,
    payload: &crate::models::UpdateBoardNotePayload,
    now: i64,
) -> Result<()> {
    let mut updates = Vec::new();
    let mut vals: Vec<rusqlite::types::Value> = Vec::new();

    if let Some(ref text) = payload.text {
        updates.push("text = ?");
        vals.push(rusqlite::types::Value::Text(text.clone()));
    }
    if let Some(x) = payload.x {
        updates.push("x = ?");
        vals.push(rusqlite::types::Value::Real(x));
    }
    if let Some(y) = payload.y {
        updates.push("y = ?");
        vals.push(rusqlite::types::Value::Real(y));
    }
    if let Some(w) = payload.width {
        updates.push("width = ?");
        vals.push(rusqlite::types::Value::Real(w));
    }
    if let Some(h) = payload.height {
        updates.push("height = ?");
        vals.push(rusqlite::types::Value::Real(h));
    }
    if let Some(s) = payload.scale {
        updates.push("scale = ?");
        vals.push(rusqlite::types::Value::Real(s));
    }
    if let Some(r) = payload.rotation {
        updates.push("rotation = ?");
        vals.push(rusqlite::types::Value::Real(r));
    }
    if let Some(z) = payload.z_index {
        updates.push("z_index = ?");
        vals.push(rusqlite::types::Value::Integer(z as i64));
    }
    if let Some(ref c) = payload.color {
        updates.push("color = ?");
        vals.push(rusqlite::types::Value::Text(c.clone()));
    }
    if let Some(fs) = payload.font_size {
        updates.push("font_size = ?");
        vals.push(rusqlite::types::Value::Integer(fs as i64));
    }
    if let Some(l) = payload.is_locked {
        updates.push("is_locked = ?");
        vals.push(rusqlite::types::Value::Integer(if l { 1 } else { 0 }));
    }

    if updates.is_empty() {
        return Ok(());
    }

    updates.push("updated_at = ?");
    vals.push(rusqlite::types::Value::Integer(now));

    let sql = format!(
        "UPDATE board_notes SET {} WHERE id = ?;",
        updates.join(", ")
    );
    vals.push(rusqlite::types::Value::Integer(payload.id));

    let params_from_iter = rusqlite::params_from_iter(vals);
    conn.execute(&sql, params_from_iter)?;
    Ok(())
}

/// ボードメモの削除
///
/// 変更理由: 不要になったメモをボードから削除するため
///
/// @param conn DB接続
/// @param id メモID
pub fn delete_board_note(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM board_notes WHERE id = ?1;", params![id])?;
    Ok(())
}

/// タイムラインの全しおり（ブックマーク）を取得
///
/// 変更理由: 保存されたタイムライン位置一覧を取得するため
///
/// @param conn DB読み取り接続
/// @return しおりレコード一覧（作成日時の新しい順）
pub fn get_bookmarks(conn: &Connection) -> Result<Vec<crate::models::BookmarkRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, folder_id, folder_name, sort, scroll_top, row_index, image_index, day_label, created_at
         FROM bookmarks
         ORDER BY created_at DESC;"
    )?;

    let rows = stmt.query_map([], |r| {
        Ok(crate::models::BookmarkRecord {
            id: r.get(0)?,
            title: r.get(1)?,
            folder_id: r.get(2)?,
            folder_name: r.get(3)?,
            sort: r.get(4)?,
            scroll_top: r.get(5)?,
            row_index: r.get(6)?,
            image_index: r.get(7)?,
            day_label: r.get(8)?,
            created_at: r.get(9)?,
        })
    })?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// タイムラインのしおりを作成または保存
///
/// 変更理由: 閲覧位置（日付、通し番号、スクロール位置、フォルダ、ソート順）をDBに永続化するため
///
/// @param conn DB書き込み接続
/// @param payload しおり作成データ
/// @param now 作成日時エポック秒
/// @return 作成されたBookmarkRecord
pub fn create_bookmark(
    conn: &Connection,
    payload: &crate::models::CreateBookmarkPayload,
    now: i64,
) -> Result<crate::models::BookmarkRecord> {
    let id = payload.id.clone().unwrap_or_else(|| {
        let hash = blake3::hash(format!("{}_{}_{}", now, payload.title, payload.scroll_top).as_bytes());
        format!("bm_{}", &hash.to_hex()[..16])
    });
    let created_at = payload.created_at.unwrap_or(now);

    conn.execute(
        "INSERT OR REPLACE INTO bookmarks (
            id, title, folder_id, folder_name, sort,
            scroll_top, row_index, image_index, day_label, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10);",
        params![
            id,
            payload.title,
            payload.folder_id,
            payload.folder_name,
            payload.sort,
            payload.scroll_top,
            payload.row_index,
            payload.image_index,
            payload.day_label,
            created_at,
        ],
    )?;

    Ok(crate::models::BookmarkRecord {
        id,
        title: payload.title.clone(),
        folder_id: payload.folder_id,
        folder_name: payload.folder_name.clone(),
        sort: payload.sort.clone(),
        scroll_top: payload.scroll_top,
        row_index: payload.row_index,
        image_index: payload.image_index,
        day_label: payload.day_label.clone(),
        created_at,
    })
}

/// タイムラインのしおりを削除
///
/// 変更理由: 不要になったしおりを削除するため
///
/// @param conn DB書き込み接続
/// @param id しおりID
pub fn delete_bookmark(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM bookmarks WHERE id = ?1;", params![id])?;
    Ok(())
}


/// 実アクセス時に画像ファイルの更新（サイズ・更新日時）が検知された際、画像レコードをアトミックに更新する
///
/// 変更理由: ユーザー要求「画像そのものが追記などで更新された場合に検知してサムネイルを更新する処理」。
/// 実アクセス時に検知した新しいサイズ・mtime・quick_hash・寸法・向きをカタログDBに保存し、
/// thumb_status を 0（未生成）にリセットして新サムネイルの生成を可能にする。
///
/// @param conn DB書き込み接続
/// @param id 画像ID
/// @param file_size 新しいファイルサイズ（バイト）
/// @param file_mtime 新しいファイル更新日時（UNIXエポック秒）
/// @param quick_hash 新しく算出されたクイックハッシュ
/// @param width 画像幅
/// @param height 画像高さ
/// @param orientation Exif Orientation
/// @return 成功時 Ok(())
pub fn update_image_on_file_changed(
    conn: &Connection,
    id: i64,
    file_size: u64,
    file_mtime: i64,
    quick_hash: &str,
    width: Option<u32>,
    height: Option<u32>,
    orientation: u32,
) -> Result<()> {
    conn.execute(
        "UPDATE images
         SET file_size = ?1,
             file_mtime = ?2,
             quick_hash = ?3,
             width = ?4,
             height = ?5,
             orientation = ?6,
             thumb_status = 0
         WHERE id = ?7;",
        params![
            file_size as i64,
            file_mtime,
            quick_hash,
            width,
            height,
            orientation,
            id,
        ],
    )?;
    Ok(())
}

/// 指定されたクイックハッシュが、指定ID以外の画像レコードから参照されているか判定
///
/// 変更理由: 同一画像が複数箇所に存在する場合、一方のみが更新された際に
/// 古いハッシュのサムネイルBLOBが他方の画像でまだ使われているかを確認し、
/// 誤って削除されるのを防止するため。
///
/// @param conn DB読み取り接続
/// @param quick_hash クイックハッシュ
/// @param exclude_id 除外する画像ID
/// @return 他に参照が存在すれば true
pub fn is_quick_hash_referenced_elsewhere(
    conn: &Connection,
    quick_hash: &str,
    exclude_id: i64,
) -> Result<bool> {
    let mut stmt = conn.prepare_cached(
        "SELECT 1 FROM images WHERE quick_hash = ?1 AND id != ?2 LIMIT 1;",
    )?;
    let mut rows = stmt.query(params![quick_hash, exclude_id])?;
    Ok(rows.next()?.is_some())
}

/// 渡されたハッシュ一覧のうち、images テーブルから1件も参照されていない孤立ハッシュを抽出
///
/// 変更理由: アイドル時孤立サムネイル回収（GC）において、全画像が更新または削除されて
/// 不要となった旧サムネイルBLOBを特定し、安全にクリーンアップするため。
///
/// @param conn DB読み取り接続
/// @param hashes 検査対象のクイックハッシュ一覧
/// @return 参照数ゼロのクイックハッシュ一覧
pub fn filter_unreferenced_hashes(
    conn: &Connection,
    hashes: &[String],
) -> Result<Vec<String>> {
    let mut unreferenced = Vec::new();
    let mut stmt = conn.prepare_cached(
        "SELECT 1 FROM images WHERE quick_hash = ?1 LIMIT 1;",
    )?;

    for hash in hashes {
        let mut rows = stmt.query(params![hash])?;
        if rows.next()?.is_none() {
            unreferenced.push(hash.clone());
        }
    }

    Ok(unreferenced)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::models::{CreateBoardNotePayload, CreateBoardPayload, UpdateBoardNotePayload};

    #[test]
    fn test_board_notes_crud() {
        let db = Database::open_in_memory().expect("インメモリDBのオープンに失敗");
        let conn = db.writer();

        // 1. ボード作成
        let board = create_board(
            &conn,
            &CreateBoardPayload {
                name: "テストボード".to_string(),
                background_color: None,
            },
            1000,
        )
        .expect("ボード作成失敗");

        // 2. メモ作成
        let note = create_board_note(
            &conn,
            &CreateBoardNotePayload {
                board_id: board.id,
                text: Some("アイデアマッピングメモ".to_string()),
                x: Some(100.0),
                y: Some(150.0),
                width: Some(250.0),
                height: Some(180.0),
                color: Some("#fef08a".to_string()),
                font_size: Some(16),
            },
            1001,
        )
        .expect("メモ作成失敗");

        assert_eq!(note.text, "アイデアマッピングメモ");
        assert_eq!(note.x, 100.0);
        assert_eq!(note.y, 150.0);
        assert_eq!(note.color, "#fef08a");
        assert_eq!(note.font_size, 16);
        assert!(!note.is_locked);

        // 3. メモ一覧取得
        let notes = get_board_notes(&conn, board.id).expect("メモ一覧取得失敗");
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].id, note.id);

        // 4. メモ更新
        update_board_note(
            &conn,
            &UpdateBoardNotePayload {
                id: note.id,
                text: Some("更新されたメモ内容".to_string()),
                x: Some(200.0),
                y: None,
                width: None,
                height: None,
                scale: None,
                rotation: None,
                z_index: None,
                color: Some("#bae6fd".to_string()),
                font_size: None,
                is_locked: Some(true),
            },
            1002,
        )
        .expect("メモ更新失敗");

        let updated_notes = get_board_notes(&conn, board.id).expect("更新後メモ一覧取得失敗");
        assert_eq!(updated_notes[0].text, "更新されたメモ内容");
        assert_eq!(updated_notes[0].x, 200.0);
        assert_eq!(updated_notes[0].color, "#bae6fd");
        assert!(updated_notes[0].is_locked);

        // 5. メモ削除
        delete_board_note(&conn, note.id).expect("メモ削除失敗");
        let after_delete = get_board_notes(&conn, board.id).expect("削除後メモ一覧取得失敗");
        assert_eq!(after_delete.len(), 0);
    }

    #[test]
    fn test_boards_stable_ordering() {
        let db = Database::open_in_memory().expect("インメモリDBのオープンに失敗");
        let conn = db.writer();

        // 1. ボードを3つ順番に作成
        let b1 = create_board(&conn, &CreateBoardPayload { name: "Board A".to_string(), background_color: None }, 1000).expect("作成失敗");
        let b2 = create_board(&conn, &CreateBoardPayload { name: "Board B".to_string(), background_color: None }, 2000).expect("作成失敗");
        let b3 = create_board(&conn, &CreateBoardPayload { name: "Board C".to_string(), background_color: None }, 3000).expect("作成失敗");

        let initial_boards = get_boards(&conn).expect("取得失敗");
        assert_eq!(initial_boards.len(), 3);
        assert_eq!(initial_boards[0].id, b1.id);
        assert_eq!(initial_boards[1].id, b2.id);
        assert_eq!(initial_boards[2].id, b3.id);

        // 2. 先頭のボード（b1）を後から閲覧・パン操作し、updated_at が最も新しくなったとする
        update_board(
            &conn,
            b1.id,
            &crate::models::UpdateBoardPayload {
                name: None,
                background_color: None,
                pan_x: Some(150.0),
                pan_y: Some(250.0),
                zoom: Some(1.5),
            },
            9999, // 最も新しい時刻
        ).expect("更新失敗");

        // 3. get_boards の結果が updated_at の影響を受けず、作成順（b1, b2, b3）で維持されることを検証
        let boards_after_update = get_boards(&conn).expect("取得失敗");
        assert_eq!(boards_after_update.len(), 3);
        assert_eq!(boards_after_update[0].id, b1.id);
        assert_eq!(boards_after_update[1].id, b2.id);
        assert_eq!(boards_after_update[2].id, b3.id);
    }

    #[test]
    fn test_bookmarks_crud() {
        let db = Database::open_in_memory().expect("インメモリDBのオープンに失敗");
        let conn = db.writer();

        // 1. しおり作成
        let bm = create_bookmark(
            &conn,
            &crate::models::CreateBookmarkPayload {
                id: Some("bm_test_1".to_string()),
                title: "テストしおり".to_string(),
                folder_id: None,
                folder_name: Some("すべての画像".to_string()),
                sort: "date_desc".to_string(),
                scroll_top: 1250.0,
                row_index: 10,
                image_index: Some(42),
                day_label: "2024-05-01".to_string(),
                created_at: Some(1000),
            },
            1000,
        )
        .expect("しおり作成失敗");

        assert_eq!(bm.id, "bm_test_1");
        assert_eq!(bm.title, "テストしおり");
        assert_eq!(bm.scroll_top, 1250.0);

        // 2. しおり一覧取得
        let bookmarks = get_bookmarks(&conn).expect("しおり一覧取得失敗");
        assert_eq!(bookmarks.len(), 1);
        assert_eq!(bookmarks[0].id, "bm_test_1");

        // 3. しおり削除
        delete_bookmark(&conn, "bm_test_1").expect("しおり削除失敗");
        let after = get_bookmarks(&conn).expect("削除後しおり一覧取得失敗");
        assert_eq!(after.len(), 0);
    }

    #[test]
    fn test_standalone_moodboard_images_and_lifecycle() {
        let db = Database::open_in_memory().expect("インメモリDBのオープンに失敗");
        let mut conn = db.writer();

        // 1. 初期状態でシステム予約の folder_id = 0 は監視フォルダ一覧に含まれないことを確認
        let folders = get_watched_folders(&conn).expect("監視フォルダ一覧取得失敗");
        assert_eq!(folders.len(), 0);

        // 2. スタンドアロン画像（folder_id = 0）を2件登録（外部ドロップ画像＋クリップボード画像）
        let drop_img_id = upsert_single_image_and_get_id(
            &conn,
            &NewImageRecord {
                folder_id: 0,
                file_path: "D:/unwatched/ref_pose.png".to_string(),
                file_size: 10240,
                file_mtime: 1700000000,
                taken_at: 1700000000,
                taken_at_source: 1,
                width: Some(800),
                height: Some(600),
                orientation: 1,
                format: Some("png".to_string()),
                quick_hash: "hash_drop_1".to_string(),
                indexed_at: 1700000000,
                source_url: Some("https://example.com/pose.png".to_string()),
            },
        )
        .expect("ドロップ画像の登録失敗");

        let clip_dir = Path::new("D:/appdata/clipboard_images");
        let clip_img_id = upsert_single_image_and_get_id(
            &conn,
            &NewImageRecord {
                folder_id: 0,
                file_path: "D:/appdata/clipboard_images/clip_abc123.png".to_string(),
                file_size: 4096,
                file_mtime: 1700000001,
                taken_at: 1700000001,
                taken_at_source: 1,
                width: Some(400),
                height: Some(300),
                orientation: 1,
                format: Some("png".to_string()),
                quick_hash: "hash_clip_1".to_string(),
                indexed_at: 1700000001,
                source_url: None,
            },
        )
        .expect("クリップボード画像の登録失敗");

        // 3. タイムラインやサムネイル全体進捗にスタンドアロン画像が混入しないことを確認
        let tl_images = get_timeline_images(&conn, 0, 100, None, None).expect("タイムライン取得失敗");
        assert_eq!(tl_images.len(), 0);
        for sort in [
            TimelineSort::TakenAtDesc,
            TimelineSort::TakenAtAsc,
            TimelineSort::Folder,
            TimelineSort::NameAsc,
            TimelineSort::NameDesc,
        ] {
            let summary = get_timeline_summary(&conn, None, 1, Some(sort)).expect("サマリ取得失敗");
            assert_eq!(summary.total, 0);
            assert_eq!(summary.buckets.len(), 0);
        }
        let (_, _, thumb_total) = get_thumb_progress(&conn).expect("進捗取得失敗");
        assert_eq!(thumb_total, 0);

        // 4. ボードを作成して2件を座標指定で配置
        let board = create_board(
            &conn,
            &CreateBoardPayload {
                name: "資料ボード".to_string(),
                background_color: None,
            },
            1700000010,
        )
        .expect("ボード作成失敗");

        let items = add_board_items_at(
            &conn,
            board.id,
            &[drop_img_id, clip_img_id],
            Some(200.0),
            Some(300.0),
            1700000011,
        )
        .expect("ボードアイテム追加失敗");
        assert_eq!(items.len(), 2);

        // 5. 後から親フォルダ D:/unwatched が監視フォルダ登録＆走査された場合、folder_id が昇格しボード配置も維持されることを確認
        let watched = add_watched_folder(&conn, "D:/unwatched", 1700000020).expect("監視フォルダ追加失敗");
        upsert_images(
            &mut conn,
            &[NewImageRecord {
                folder_id: watched.id,
                file_path: "D:/unwatched/ref_pose.png".to_string(),
                file_size: 10240,
                file_mtime: 1700000000,
                taken_at: 1700000000,
                taken_at_source: 1,
                width: Some(800),
                height: Some(600),
                orientation: 1,
                format: Some("png".to_string()),
                quick_hash: "hash_drop_1".to_string(),
                indexed_at: 1700000021,
                source_url: None,
            }],
        )
        .expect("バッチUPSERT失敗");

        let detail = get_image_detail(&conn, drop_img_id)
            .expect("詳細取得失敗")
            .expect("画像が存在しない");
        assert_eq!(detail.folder_id, watched.id);
        // COALESCE により元の source_url が維持されていることを確認
        assert_eq!(detail.source_url, Some("https://example.com/pose.png".to_string()));

        // タイムラインには昇格した1件のみ表示される
        let tl_after = get_timeline_images(&conn, 0, 100, None, None).expect("タイムライン取得失敗");
        assert_eq!(tl_after.len(), 1);
        assert_eq!(tl_after[0].id, drop_img_id);

        // 6. 監視フォルダを解除しても、ムードボード上に配置されている画像は folder_id = 0 に退避して残ることを確認
        remove_watched_folder(&conn, watched.id).expect("監視フォルダ削除失敗");
        let detail_after_remove = get_image_detail(&conn, drop_img_id)
            .expect("詳細取得失敗")
            .expect("ボード上の画像が誤って削除された");
        assert_eq!(detail_after_remove.folder_id, 0);
        let board_items_after = get_board_items(&conn, board.id).expect("ボードアイテム取得失敗");
        assert_eq!(board_items_after.len(), 2);
        let drop_board_item = board_items_after
            .iter()
            .find(|i| i.image_id == drop_img_id)
            .unwrap();
        assert_eq!(drop_board_item.source_url, Some("https://example.com/pose.png".to_string()));

        // 7. クリップボード画像をボードから削除すると、cleanup_orphan_clipboard_images で回収され、通常の外部ファイルは消えないことを確認
        let clip_board_item = board_items_after
            .iter()
            .find(|i| i.image_id == clip_img_id)
            .unwrap();
        delete_board_item(&conn, clip_board_item.id).expect("ボードアイテム削除失敗");

        let cleaned = cleanup_orphan_clipboard_images(&conn, clip_dir).expect("クリーンアップ失敗");
        assert_eq!(cleaned.len(), 1);
        assert_eq!(cleaned[0].0, clip_img_id);
        assert!(get_image_detail(&conn, clip_img_id).unwrap().is_none());
        assert!(get_image_detail(&conn, drop_img_id).unwrap().is_some());
    }
}



