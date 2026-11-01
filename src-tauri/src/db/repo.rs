use rusqlite::{params, Connection, Result};
use crate::models::{
    DayBucket, FolderStatus, FolderTreeNode, ImageDetail, ImageRecord, ImageTagAssignment,
    SubfolderEntry, TagRecord, TagWithCount, TakenAtSource, TimelineFilter, TimelineSort,
    TimelineSummary, WatchedFolder,
};
use std::collections::{BTreeMap, HashMap, HashSet};
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
        thumb_failed_count: 0,
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
    // 変更理由: オンデマンド展開されたアーカイブ内エントリ（*!/*）がルート監視フォルダの画像枚数を
    // 意図せず肥大化させないよう、通常ファイルのみを集計対象とする。
    let mut stmt = conn.prepare_cached(
        "SELECT f.id, f.path, COUNT(i.id) AS cnt,
                COALESCE(SUM(CASE WHEN i.thumb_status = 1 THEN 1 ELSE 0 END), 0) AS thumb_cnt,
                COALESCE(SUM(CASE WHEN i.thumb_status = 2 THEN 1 ELSE 0 END), 0) AS failed_cnt
         FROM watched_folders f
         LEFT JOIN images i ON f.id = i.folder_id AND i.file_path NOT GLOB '*!/*'
         WHERE f.id > 0
         GROUP BY f.id, f.path
         ORDER BY f.id ASC;",
    )?;

    let rows = stmt.query_map([], |row| {
        let path: String = row.get(1)?;
        let is_online = Path::new(&path).exists();
        let count: i64 = row.get(2)?;
        let thumb_count: i64 = row.get(3)?;
        let thumb_failed_count: i64 = row.get(4)?;
        Ok(WatchedFolder {
            id: row.get(0)?,
            path,
            image_count: count,
            thumb_count,
            thumb_failed_count,
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

/// ファイル名検索クエリを正のキーワード群と負（除外）のキーワード群に分解する
///
/// 変更理由: スペース（半角・全角）区切りの複数キーワード検索において、先頭に `-` または `!` が付いた
/// トークンを除外（NOT）条件として抽出し、それ以外を AND/OR 条件として扱うため。
pub fn parse_search_query_tokens(query: &str) -> (Vec<String>, Vec<String>) {
    let mut positive = Vec::new();
    let mut negative = Vec::new();

    for token in query.split(|c: char| c.is_whitespace()) {
        let trimmed = token.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(neg) = trimmed.strip_prefix('-').or_else(|| trimmed.strip_prefix('!')) {
            let neg_clean = neg.trim();
            if !neg_clean.is_empty() {
                negative.push(neg_clean.to_lowercase());
            }
        } else {
            positive.push(trimmed.to_lowercase());
        }
    }

    (positive, negative)
}

/// LIKE パターン用に特殊文字（`\`, `%`, `_`）をエスケープする
fn escape_like_pattern(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '\\' | '%' | '_' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// 拡張子名を正規化し、jpeg/jpg などの同義フォーマット展開リストを返す
fn expand_format_synonyms(ext: &str) -> Vec<String> {
    let clean = ext.trim().trim_start_matches('.').to_ascii_lowercase();
    match clean.as_str() {
        "jpg" | "jpeg" => vec!["jpg".to_string(), "jpeg".to_string()],
        "tif" | "tiff" => vec!["tif".to_string(), "tiff".to_string()],
        "" => Vec::new(),
        _ => vec![clean],
    }
}

/// タイムライン検索・絞り込み用のSQL WHERE句とバインドパラメータを組み立てる
///
/// 変更理由: `get_timeline_images` と `get_timeline_summary` の双方で完全に同一の絞り込み条件を適用し、
/// 10万〜30万件規模でも複合インデックスを活用した高速検索を行うため。
/// また、アーカイブ仮想フォルダ方式の原則に従い、通常閲覧時（全画像・監視フォルダルート・通常サブフォルダ）には
/// アーカイブ内エントリ（`*!/*`）を除外し、アーカイブ仮想ノードが選択された場合のみインデックス前方一致で高速抽出する。
fn build_timeline_where_clause(
    conn: &Connection,
    table_alias: &str,
    folder_id: Option<i64>,
    filter: Option<&TimelineFilter>,
) -> Result<(String, Vec<rusqlite::types::Value>)> {
    let pfx = if table_alias.is_empty() {
        String::new()
    } else {
        format!("{}.", table_alias)
    };

    let mut clauses: Vec<String> = Vec::new();
    let mut params_vec: Vec<rusqlite::types::Value> = Vec::new();

    if let Some(fid) = folder_id {
        clauses.push(format!("{}folder_id = ?", pfx));
        params_vec.push(rusqlite::types::Value::Integer(fid));
    } else {
        clauses.push(format!("{}folder_id > 0", pfx));
    }

    let Some(f) = filter else {
        // フィルタ無指定（全画像表示または監視フォルダルート表示）ではアーカイブ内部エントリを除外
        clauses.push(format!("{}file_path NOT GLOB '*!/*'", pfx));
        return Ok((clauses.join(" AND "), params_vec));
    };

    // 1. サブフォルダまたはアーカイブ仮想フォルダ前方一致レンジフィルタ (B-Tree インデックス活用)
    let mut is_archive_target = false;
    if let Some(fid) = folder_id {
        if let Some(ref sub_rel) = f.sub_folder_path {
            let trimmed_sub = sub_rel.trim();
            if !trimmed_sub.is_empty() {
                if let Ok(root_path) = conn.query_row(
                    "SELECT path FROM watched_folders WHERE id = ?1;",
                    params![fid],
                    |r| r.get::<_, String>(0),
                ) {
                    let norm_root = root_path.replace('\\', "/").trim_end_matches('/').to_string();
                    let win_root = norm_root.replace('/', "\\");

                    // sub_rel が既に root_path から始まる絶対パスか、相対パスかを正規化して統一
                    let norm_sub_full = trimmed_sub.replace('\\', "/");
                    let rel_from_root = if norm_sub_full.to_lowercase().starts_with(&format!("{}/", norm_root.to_lowercase())) {
                        let chars_to_skip = norm_root.chars().count() + 1;
                        norm_sub_full
                            .char_indices()
                            .nth(chars_to_skip)
                            .map(|(idx, _)| norm_sub_full[idx..].to_string())
                            .unwrap_or_default()
                    } else {
                        norm_sub_full.trim_matches('/').to_string()
                    };

                    if !rel_from_root.is_empty() {
                        if let Some((arch_rel, inner_sub)) = rel_from_root.split_once("!/") {
                            // パターンA: アーカイブまたはアーカイブ内サブフォルダが明示指定された場合 ("manga.zip!/" or "manga.zip!/ch01")
                            is_archive_target = true;
                            let clean_inner = inner_sub.trim_matches('/');
                            let arch_slash = format!("{}/{}", norm_root, arch_rel.trim_matches('/'));
                            let arch_back = format!("{}\\{}", win_root, arch_rel.trim_matches('/').replace('/', "\\"));

                            let (lower_slash, upper_slash, lower_back, upper_back) = if clean_inner.is_empty() {
                                (
                                    format!("{}!/", arch_slash),
                                    format!("{}!0", arch_slash),
                                    format!("{}!/", arch_back),
                                    format!("{}!0", arch_back),
                                )
                            } else {
                                (
                                    format!("{}!/{}/", arch_slash, clean_inner),
                                    format!("{}!/{}0", arch_slash, clean_inner),
                                    format!("{}!/{}/", arch_back, clean_inner),
                                    format!("{}!/{}0", arch_back, clean_inner),
                                )
                            };

                            clauses.push(format!(
                                "(({0}file_path >= ? AND {0}file_path < ?) OR ({0}file_path >= ? AND {0}file_path < ?))",
                                pfx
                            ));
                            params_vec.push(rusqlite::types::Value::Text(lower_slash));
                            params_vec.push(rusqlite::types::Value::Text(upper_slash));
                            params_vec.push(rusqlite::types::Value::Text(lower_back));
                            params_vec.push(rusqlite::types::Value::Text(upper_back));
                        } else {
                            // パターンB: "!/ "を含まないパス指定（"2024/Summer" または "manga.zip"）
                            // アーカイブファイル自体のパスとして DB に `.../manga.zip!/...` が存在するかを高速チェックし、
                            // または拡張子・実ファイル判定でアーカイブか通常ディレクトリかを両方カバーする。
                            let arch_slash = format!("{}/{}", norm_root, rel_from_root);
                            let arch_back = format!("{}\\{}", win_root, rel_from_root.replace('/', "\\"));
                            let candidate_path = Path::new(&arch_back);
                            let looks_like_archive_file = candidate_path.is_file() || {
                                let lower_arch_s = format!("{}!/", arch_slash);
                                let upper_arch_s = format!("{}!0", arch_slash);
                                let lower_arch_b = format!("{}!/", arch_back);
                                let upper_arch_b = format!("{}!0", arch_back);
                                conn.query_row(
                                    "SELECT 1 FROM images WHERE folder_id = ?1 AND ((file_path >= ?2 AND file_path < ?3) OR (file_path >= ?4 AND file_path < ?5)) LIMIT 1;",
                                    params![fid, lower_arch_s, upper_arch_s, lower_arch_b, upper_arch_b],
                                    |_| Ok(true),
                                )
                                .unwrap_or(false)
                            };

                            if looks_like_archive_file {
                                is_archive_target = true;
                                let lower_slash = format!("{}!/", arch_slash);
                                let upper_slash = format!("{}!0", arch_slash);
                                let lower_back = format!("{}!/", arch_back);
                                let upper_back = format!("{}!0", arch_back);

                                clauses.push(format!(
                                    "(({0}file_path >= ? AND {0}file_path < ?) OR ({0}file_path >= ? AND {0}file_path < ?))",
                                    pfx
                                ));
                                params_vec.push(rusqlite::types::Value::Text(lower_slash));
                                params_vec.push(rusqlite::types::Value::Text(upper_slash));
                                params_vec.push(rusqlite::types::Value::Text(lower_back));
                                params_vec.push(rusqlite::types::Value::Text(upper_back));
                            } else {
                                let sub_prefix_slash = format!("{}/", arch_slash);
                                let sub_upper_slash = format!("{}0", arch_slash);
                                let sub_prefix_back = format!("{}\\", arch_back);
                                let sub_upper_back = format!("{}}}", arch_back);

                                clauses.push(format!(
                                    "(({0}file_path >= ? AND {0}file_path < ?) OR ({0}file_path >= ? AND {0}file_path < ?))",
                                    pfx
                                ));
                                params_vec.push(rusqlite::types::Value::Text(sub_prefix_slash));
                                params_vec.push(rusqlite::types::Value::Text(sub_upper_slash));
                                params_vec.push(rusqlite::types::Value::Text(sub_prefix_back));
                                params_vec.push(rusqlite::types::Value::Text(sub_upper_back));
                            }
                        }
                    }
                }
            }
        }
    }

    // アーカイブ仮想フォルダを選択していない場合は、アーカイブ内エントリ（*!/*）をタイムラインから除外する
    if !is_archive_target {
        clauses.push(format!("{}file_path NOT GLOB '*!/*'", pfx));
    }

    // 2. ファイル名検索（AND / OR / NOT）
    if let Some(ref q) = f.query {
        let (pos_tokens, neg_tokens) = parse_search_query_tokens(q);
        if !pos_tokens.is_empty() {
            let is_or = f
                .query_mode
                .as_deref()
                .map(|m| m.eq_ignore_ascii_case("or"))
                .unwrap_or(false);
            let mut token_conds = Vec::with_capacity(pos_tokens.len());
            for tok in pos_tokens {
                let pat = format!("%{}%", escape_like_pattern(&tok));
                token_conds.push(format!("LOWER({}file_path) LIKE ? ESCAPE '\\'", pfx));
                params_vec.push(rusqlite::types::Value::Text(pat));
            }
            let joiner = if is_or { " OR " } else { " AND " };
            clauses.push(format!("({})", token_conds.join(joiner)));
        }
        for neg in neg_tokens {
            let pat = format!("%{}%", escape_like_pattern(&neg));
            clauses.push(format!("LOWER({}file_path) NOT LIKE ? ESCAPE '\\'", pfx));
            params_vec.push(rusqlite::types::Value::Text(pat));
        }
    }

    // 3. 拡張子フィルタ（含む）
    if let Some(ref exts) = f.extensions {
        let mut expanded = Vec::new();
        for e in exts {
            for syn in expand_format_synonyms(e) {
                if !expanded.contains(&syn) {
                    expanded.push(syn);
                }
            }
        }
        if !expanded.is_empty() {
            let mut ext_conds = Vec::new();
            let placeholders = expanded.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            ext_conds.push(format!("LOWER(COALESCE({}format, '')) IN ({})", pfx, placeholders));
            for syn in &expanded {
                params_vec.push(rusqlite::types::Value::Text(syn.clone()));
            }
            for syn in &expanded {
                ext_conds.push(format!("LOWER({}file_path) LIKE ?", pfx));
                params_vec.push(rusqlite::types::Value::Text(format!("%.{}", syn)));
            }
            clauses.push(format!("({})", ext_conds.join(" OR ")));
        }
    }

    // 4. 拡張子フィルタ（除外：NOT）
    if let Some(ref ex_exts) = f.exclude_extensions {
        let mut expanded = Vec::new();
        for e in ex_exts {
            for syn in expand_format_synonyms(e) {
                if !expanded.contains(&syn) {
                    expanded.push(syn);
                }
            }
        }
        if !expanded.is_empty() {
            let placeholders = expanded.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            clauses.push(format!(
                "LOWER(COALESCE({}format, '')) NOT IN ({})",
                pfx, placeholders
            ));
            for syn in &expanded {
                params_vec.push(rusqlite::types::Value::Text(syn.clone()));
            }
            for syn in &expanded {
                clauses.push(format!("LOWER({}file_path) NOT LIKE ?", pfx));
                params_vec.push(rusqlite::types::Value::Text(format!("%.{}", syn)));
            }
        }
    }

    // 5. 縦横比フィルタ（landscape / portrait / square）
    if let Some(ref aspect) = f.aspect_ratio {
        match aspect.as_str() {
            "landscape" => {
                clauses.push(format!(
                    "({0}width IS NOT NULL AND {0}height IS NOT NULL AND {0}width > {0}height)",
                    pfx
                ));
            }
            "portrait" => {
                clauses.push(format!(
                    "({0}width IS NOT NULL AND {0}height IS NOT NULL AND {0}height > {0}width)",
                    pfx
                ));
            }
            "square" => {
                clauses.push(format!(
                    "({0}width IS NOT NULL AND {0}height IS NOT NULL AND {0}width = {0}height)",
                    pfx
                ));
            }
            _ => {}
        }
    }

    // 6. タグフィルタ（含む：AND / OR）
    if let Some(ref inc_tags) = f.include_tag_ids {
        let mut unique_ids: Vec<i64> = inc_tags.clone();
        unique_ids.sort_unstable();
        unique_ids.dedup();
        if !unique_ids.is_empty() {
            let is_or = f
                .tag_mode
                .as_deref()
                .map(|m| m.eq_ignore_ascii_case("or"))
                .unwrap_or(false);
            let placeholders = unique_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            if is_or || unique_ids.len() == 1 {
                clauses.push(format!(
                    "EXISTS (SELECT 1 FROM image_tags it WHERE it.image_id = {}id AND it.tag_id IN ({}))",
                    pfx, placeholders
                ));
                for tid in unique_ids {
                    params_vec.push(rusqlite::types::Value::Integer(tid));
                }
            } else {
                let count_needed = unique_ids.len() as i64;
                clauses.push(format!(
                    "(SELECT COUNT(DISTINCT it.tag_id) FROM image_tags it WHERE it.image_id = {}id AND it.tag_id IN ({})) = ?",
                    pfx, placeholders
                ));
                for tid in unique_ids {
                    params_vec.push(rusqlite::types::Value::Integer(tid));
                }
                params_vec.push(rusqlite::types::Value::Integer(count_needed));
            }
        }
    }

    // 7. タグフィルタ（除外：NOT）
    if let Some(ref exc_tags) = f.exclude_tag_ids {
        let mut unique_ids: Vec<i64> = exc_tags.clone();
        unique_ids.sort_unstable();
        unique_ids.dedup();
        if !unique_ids.is_empty() {
            let placeholders = unique_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            clauses.push(format!(
                "NOT EXISTS (SELECT 1 FROM image_tags it_ex WHERE it_ex.image_id = {}id AND it_ex.tag_id IN ({}))",
                pfx, placeholders
            ));
            for tid in unique_ids {
                params_vec.push(rusqlite::types::Value::Integer(tid));
            }
        }
    }

    Ok((clauses.join(" AND "), params_vec))
}

/// タイムライン用の画像一覧をページング取得
///
/// 変更理由: 仕様書§4「表示ソート順および複合フィルタ条件に応じたページング取得対応」
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
    get_timeline_images_filtered(conn, offset, limit, folder_id, sort_by, None)
}

/// フィルタ条件付きでタイムライン用の画像一覧をページング取得
pub fn get_timeline_images_filtered(
    conn: &Connection,
    offset: i64,
    limit: i64,
    folder_id: Option<i64>,
    sort_by: Option<TimelineSort>,
    filter: Option<&TimelineFilter>,
) -> Result<Vec<ImageRecord>> {
    let sort = sort_by.unwrap_or(TimelineSort::TakenAtDesc);
    // 変更理由: アーカイブ閲覧時およびファイル名ソート時において、`page1.jpg, page2.jpg, ..., page10.jpg` の
    // 自然順（COLLATE NATURAL）が維持されるようにし、漫画・連番作品が正しいページ順で表示されるようにする。
    let is_archive_filter = filter
        .and_then(|f| f.sub_folder_path.as_deref())
        .map(|s| {
            let trimmed = s.trim();
            trimmed.contains("!/")
                || Path::new(trimmed)
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|ext| {
                        matches!(
                            ext.to_ascii_lowercase().as_str(),
                            "zip" | "cbz" | "rar" | "cbr" | "7z" | "cb7" | "tar"
                        )
                    })
                    .unwrap_or(false)
        })
        .unwrap_or(false);

    let order_sql = match sort {
        TimelineSort::TakenAtDesc => "ORDER BY i.taken_at DESC, i.id DESC",
        TimelineSort::TakenAtAsc => "ORDER BY i.taken_at ASC, i.id ASC",
        TimelineSort::Folder => {
            if is_archive_filter {
                "ORDER BY i.folder_id ASC, i.file_path COLLATE NATURALSORT ASC, i.id ASC"
            } else {
                "ORDER BY i.folder_id ASC, i.taken_at DESC, i.id DESC"
            }
        }
        TimelineSort::NameAsc => "ORDER BY i.file_path COLLATE NATURALSORT ASC, i.id ASC",
        TimelineSort::NameDesc => "ORDER BY i.file_path COLLATE NATURALSORT DESC, i.id DESC",
    };

    let (where_sql, mut params_vec) = build_timeline_where_clause(conn, "i", folder_id, filter)?;
    let sql = format!(
        "SELECT i.id, i.taken_at, i.width, i.height, i.file_mtime, i.file_path, i.format, i.folder_id
         FROM images i
         WHERE {}
         {}
         LIMIT ? OFFSET ?;",
        where_sql, order_sql
    );

    params_vec.push(rusqlite::types::Value::Integer(limit));
    params_vec.push(rusqlite::types::Value::Integer(offset));

    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(params_vec), |row| {
        let file_path: String = row.get(5)?;
        let format_opt: Option<String> = row.get(6)?;
        let folder_id: i64 = row.get(7)?;

        // アーカイブ本体であるか判定（!/ を含まず、formatまたは拡張子がアーカイブ形式であること）
        let is_arch = !file_path.contains("!/") && {
            let ext = format_opt.as_deref().or_else(|| {
                Path::new(&file_path).extension().and_then(|e| e.to_str())
            }).unwrap_or("");
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "zip" | "cbz" | "rar" | "cbr" | "7z" | "cb7" | "tar"
            )
        };

        let (is_archive, file_name, out_file_path, out_folder_id) = if is_arch {
            let name = Path::new(&file_path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| file_path.clone());
            (Some(true), Some(name), Some(file_path), Some(folder_id))
        } else {
            (None, None, None, None)
        };

        Ok(ImageRecord {
            id: row.get(0)?,
            taken_at: row.get(1)?,
            width: row.get(2)?,
            height: row.get(3)?,
            rev: row.get(4)?,
            is_archive,
            file_name,
            file_path: out_file_path,
            folder_id: out_folder_id,
            format: format_opt,
        })
    })?;

    let mut images = Vec::new();
    for img in rows {
        images.push(img?);
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
    get_timeline_summary_filtered(conn, folder_id, version, sort_by, None)
}

/// フィルタ条件付きでセクション別件数サマリと総件数を取得
pub fn get_timeline_summary_filtered(
    conn: &Connection,
    folder_id: Option<i64>,
    version: i64,
    sort_by: Option<TimelineSort>,
    filter: Option<&TimelineFilter>,
) -> Result<TimelineSummary> {
    let sort = sort_by.unwrap_or(TimelineSort::TakenAtDesc);
    let mut buckets = Vec::new();

    let (where_sql, params_vec) = build_timeline_where_clause(conn, "i", folder_id, filter)?;
    let count_sql = format!("SELECT COUNT(*) FROM images i WHERE {};", where_sql);
    let total: i64 = conn.query_row(
        &count_sql,
        rusqlite::params_from_iter(params_vec.clone()),
        |row| row.get(0),
    )?;

    match sort {
        TimelineSort::Folder => {
            let group_sql = format!(
                "SELECT f.path, COUNT(i.id) AS cnt
                 FROM watched_folders f
                 INNER JOIN images i ON f.id = i.folder_id
                 WHERE {}
                 GROUP BY f.id, f.path
                 HAVING cnt > 0
                 ORDER BY f.id ASC;",
                where_sql
            );
            let mut stmt = conn.prepare_cached(&group_sql)?;
            let rows = stmt.query_map(rusqlite::params_from_iter(params_vec), |row| {
                let path: String = row.get(0)?;
                let count: i64 = row.get(1)?;
                Ok(DayBucket { day: path, count })
            })?;
            for b in rows {
                buckets.push(b?);
            }
        }
        TimelineSort::TakenAtAsc => {
            let group_sql = format!(
                "SELECT strftime('%Y-%m-%d', i.taken_at, 'unixepoch') AS day, COUNT(*) AS cnt
                 FROM images i
                 WHERE {}
                 GROUP BY day
                 ORDER BY day ASC;",
                where_sql
            );
            let mut stmt = conn.prepare_cached(&group_sql)?;
            let rows = stmt.query_map(rusqlite::params_from_iter(params_vec), |row| {
                let day: Option<String> = row.get(0)?;
                let day_str = day.unwrap_or_else(|| "1970-01-01".to_string());
                let count: i64 = row.get(1)?;
                Ok(DayBucket { day: day_str, count })
            })?;
            for b in rows {
                buckets.push(b?);
            }
        }
        TimelineSort::NameAsc | TimelineSort::NameDesc => {
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
            let group_sql = format!(
                "SELECT strftime('%Y-%m-%d', i.taken_at, 'unixepoch') AS day, COUNT(*) AS cnt
                 FROM images i
                 WHERE {}
                 GROUP BY day
                 ORDER BY day DESC;",
                where_sql
            );
            let mut stmt = conn.prepare_cached(&group_sql)?;
            let rows = stmt.query_map(rusqlite::params_from_iter(params_vec), |row| {
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
        // 変更理由: アーカイブ仮想パス（`<archive_path>!/<inner_path>`）の場合は親アーカイブファイルの存在を確認する
        let exists = if let Some((archive_path, _)) = crate::plugins::split_archive_virtual_path(&file_path) {
            Path::new(archive_path).exists()
        } else {
            Path::new(&file_path).exists()
        };
        let src_int: i32 = row.get(9)?;
        let taken_at_source = if src_int == 0 {
            TakenAtSource::Exif
        } else {
            TakenAtSource::Mtime
        };

        let tags = get_tags_for_single_image(conn, id).unwrap_or_default();

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
            tags,
        }))
    } else {
        Ok(None)
    }
}

/// 単一画像に付与されているタグ一覧を取得する
pub fn get_tags_for_single_image(conn: &Connection, image_id: i64) -> Result<Vec<TagRecord>> {
    let mut stmt = conn.prepare_cached(
        "SELECT t.id, t.name, t.color, t.sort_order, t.created_at
         FROM tags t
         INNER JOIN image_tags it ON t.id = it.tag_id
         WHERE it.image_id = ?1
         ORDER BY t.sort_order ASC, t.name COLLATE NOCASE ASC, t.id ASC;",
    )?;
    let rows = stmt.query_map(params![image_id], |r| {
        Ok(TagRecord {
            id: r.get(0)?,
            name: r.get(1)?,
            color: r.get(2)?,
            sort_order: r.get(3)?,
            created_at: r.get(4)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
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

// -----------------------------------------------------------------------------
// サブフォルダ階層ツリー取得 & タグ管理 CRUD
// -----------------------------------------------------------------------------

/// 指定された監視フォルダ配下のサブフォルダ一覧とそれぞれの画像枚数をDB内のfile_pathから高速構築する
///
/// 変更理由: ディスクI/O（ディレクトリ再走査）を一切発生させず、インデックススキャンのみで
/// サブフォルダ階層と各階層の累積画像枚数を数ミリ秒で算出するため。
///
/// @param conn DB読み取り接続
/// @param folder_id 監視フォルダID
/// @return ソート済みの SubfolderEntry リスト
pub fn get_subfolder_tree(conn: &Connection, folder_id: i64) -> Result<Vec<SubfolderEntry>> {
    let root_path: String = match conn.query_row(
        "SELECT path FROM watched_folders WHERE id = ?1;",
        params![folder_id],
        |r| r.get(0),
    ) {
        Ok(p) => p,
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let norm_root = root_path.replace('\\', "/").trim_end_matches('/').to_string();
    let root_prefix = format!("{}/", norm_root);
    let root_prefix_lower = root_prefix.to_lowercase();

    let mut stmt = conn.prepare_cached(
        "SELECT file_path FROM images WHERE folder_id = ?1;",
    )?;
    let rows = stmt.query_map(params![folder_id], |r| r.get::<_, String>(0))?;

    let mut counts: BTreeMap<String, i64> = BTreeMap::new();

    for row in rows {
        let fp = row?;
        // 変更理由: アーカイブ内部エントリ（*!/*）は get_folder_hierarchy のアーカイブノード配下で扱うため、
        // 従来のフラットなディレクトリ一覧からは除外する。
        if fp.contains("!/") {
            continue;
        }
        let norm_fp = fp.replace('\\', "/");
        let rel = if norm_fp.starts_with(&root_prefix) {
            &norm_fp[root_prefix.len()..]
        } else if norm_fp.to_lowercase().starts_with(&root_prefix_lower) {
            // Windows のドライブレター大小文字差を考慮した切り出し
            let prefix_chars = root_prefix.chars().count();
            match norm_fp.char_indices().nth(prefix_chars) {
                Some((byte_idx, _)) => &norm_fp[byte_idx..],
                None => continue,
            }
        } else {
            continue;
        };

        // ファイル名を除いたディレクトリパーツを取り出す
        let parts: Vec<&str> = rel.split('/').filter(|s| !s.is_empty()).collect();
        if parts.len() <= 1 {
            // ルート直下のファイルはサブフォルダではないためスキップ
            continue;
        }

        let dir_parts = &parts[..parts.len() - 1];
        let mut accum = String::new();
        for (idx, part) in dir_parts.iter().enumerate() {
            if idx > 0 {
                accum.push('/');
            }
            accum.push_str(part);
            *counts.entry(accum.clone()).or_insert(0) += 1;
        }
    }

    let mut result = Vec::with_capacity(counts.len());
    for (rel_path, count) in counts {
        let name = rel_path
            .rsplit('/')
            .next()
            .unwrap_or(&rel_path)
            .to_string();
        let depth = rel_path.split('/').count() as u32;
        result.push(SubfolderEntry {
            relative_path: rel_path,
            name,
            depth,
            image_count: count,
        });
    }

    Ok(result)
}

/// アーカイブ内画像の寸法（width, height）が未記録だった場合にデコード後の寸法で更新する
///
/// 変更理由: `open_archive_folder` 時点では高速化のためデータ本体を解凍せず `width = None, height = None` で登録し、
/// サムネイル生成や原寸表示で初めてデコードされたタイミングで寸法を補完するため。
pub fn update_image_dimensions_if_empty(
    conn: &Connection,
    id: i64,
    width: u32,
    height: u32,
) -> Result<()> {
    conn.execute(
        "UPDATE images SET width = ?1, height = ?2 WHERE id = ?3 AND (width IS NULL OR height IS NULL);",
        params![width, height, id],
    )?;
    Ok(())
}

/// 指定アーカイブが未変更のまま既にDBへインデックス済みであればその件数を返す（0msキャッシュヒット判定用）
///
/// 変更理由: ユーザーがサイドバーで同じアーカイブを再選択した際、アーカイブの `mtime` が変わっていなければ
/// セントラルディレクトリ再走査やDB書き込みを一切行わずに即座に件数を返すため。
pub fn get_indexed_archive_count_if_unchanged(
    conn: &Connection,
    folder_id: i64,
    archive_path: &str,
    archive_mtime: i64,
) -> Result<Option<usize>> {
    let norm_slash = archive_path.replace('\\', "/");
    let norm_back = norm_slash.replace('/', "\\");
    let lower_s = format!("{}!/", norm_slash);
    let upper_s = format!("{}!0", norm_slash);
    let lower_b = format!("{}!/", norm_back);
    let upper_b = format!("{}!0", norm_back);

    let (count, min_mtime, max_mtime): (i64, Option<i64>, Option<i64>) = conn.query_row(
        "SELECT COUNT(*), MIN(file_mtime), MAX(file_mtime)
         FROM images
         WHERE folder_id = ?1
           AND ((file_path >= ?2 AND file_path < ?3) OR (file_path >= ?4 AND file_path < ?5));",
        params![folder_id, lower_s, upper_s, lower_b, upper_b],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;

    if count > 0 && min_mtime == Some(archive_mtime) && max_mtime == Some(archive_mtime) {
        Ok(Some(count as usize))
    } else {
        Ok(None)
    }
}

/// アーカイブ内エントリ群を単一トランザクションで一括UPSERTし、アーカイブから消えた古いエントリを掃除する
///
/// 変更理由: オンデマンド展開時に自然順（Natural Sort）でソート済みのエントリを一括登録し、
/// 再パック等で削除された内部エントリがDBに残留するのを防ぐため。
pub fn upsert_archive_entries(
    conn: &mut Connection,
    folder_id: i64,
    archive_path: &str,
    records: &[NewImageRecord],
) -> Result<usize> {
    let norm_slash = archive_path.replace('\\', "/");
    let norm_back = norm_slash.replace('/', "\\");
    let lower_s = format!("{}!/", norm_slash);
    let upper_s = format!("{}!0", norm_slash);
    let lower_b = format!("{}!/", norm_back);
    let upper_b = format!("{}!0", norm_back);

    let new_paths: HashSet<&str> = records.iter().map(|r| r.file_path.as_str()).collect();

    let tx = conn.transaction()?;
    {
        // 1. 既存の該当アーカイブ内エントリのうち、今回のリストに含まれないものを削除
        let mut existing_to_remove: Vec<i64> = Vec::new();
        {
            let mut sel_stmt = tx.prepare_cached(
                "SELECT id, file_path FROM images
                 WHERE folder_id = ?1
                   AND ((file_path >= ?2 AND file_path < ?3) OR (file_path >= ?4 AND file_path < ?5));",
            )?;
            let rows = sel_stmt.query_map(
                params![folder_id, lower_s, upper_s, lower_b, upper_b],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
            )?;
            for row in rows {
                let (id, fp) = row?;
                if !new_paths.contains(fp.as_str()) {
                    existing_to_remove.push(id);
                }
            }
        }
        if !existing_to_remove.is_empty() {
            let mut del_stmt = tx.prepare_cached(
                "DELETE FROM images WHERE id = ?1 AND NOT EXISTS (SELECT 1 FROM board_items bi WHERE bi.image_id = images.id);",
            )?;
            for id in existing_to_remove {
                del_stmt.execute(params![id])?;
            }
        }

        // 2. 全エントリを一括UPSERT
        if !records.is_empty() {
            let mut ins_stmt = tx.prepare_cached(
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
                    width = COALESCE(excluded.width, images.width),
                    height = COALESCE(excluded.height, images.height),
                    orientation = excluded.orientation,
                    format = excluded.format,
                    quick_hash = excluded.quick_hash,
                    thumb_status = CASE WHEN images.quick_hash <> excluded.quick_hash THEN 0 ELSE images.thumb_status END,
                    indexed_at = excluded.indexed_at;",
            )?;

            for r in records {
                ins_stmt.execute(params![
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
    }
    tx.commit()?;
    Ok(records.len())
}

/// 監視フォルダ配下のディレクトリ階層とアーカイブ仮想フォルダを統合した再帰ツリーを構築する
///
/// 変更理由: サイドバー上で通常のサブフォルダとアーカイブファイル（.zip, .cbz 等）、および
/// 展開済みアーカイブ内のサブディレクトリをシームレスな階層ツリーとして表示・選択可能にするため。
///
/// @param conn DB読み取り接続
/// @param folder_id 監視フォルダID
/// @param archive_extensions 現在有効なアーカイブ拡張子リスト（小文字・ドットなし）
/// @return ルート直下の FolderTreeNode リスト（自然順ソート済み）
pub fn get_folder_hierarchy(
    conn: &Connection,
    folder_id: i64,
    archive_extensions: &[String],
) -> Result<Vec<FolderTreeNode>> {
    let root_path: String = match conn.query_row(
        "SELECT path FROM watched_folders WHERE id = ?1;",
        params![folder_id],
        |r| r.get(0),
    ) {
        Ok(p) => p,
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let norm_root = root_path.replace('\\', "/").trim_end_matches('/').to_string();
    let root_prefix = format!("{}/", norm_root);
    let root_prefix_lower = root_prefix.to_lowercase();

    // 1. DB 内の file_path を1パス走査し、通常サブフォルダの画像数と、インデックス済みアーカイブの画像数・内部サブフォルダ、および表紙（1枚目）を集計
    let mut dir_counts: BTreeMap<String, i64> = BTreeMap::new();
    let mut archive_counts: HashMap<String, i64> = HashMap::new();
    let mut archive_sub_counts: HashMap<String, BTreeMap<String, i64>> = HashMap::new();
    let mut archive_covers: HashMap<String, (String, i64, i64)> = HashMap::new(); // clean_arch -> (min_inner_path, id, rev)

    let mut stmt = conn.prepare_cached("SELECT file_path, id, file_mtime FROM images WHERE folder_id = ?1;")?;
    let rows = stmt.query_map(params![folder_id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))
    })?;

    for row in rows {
        let (fp, img_id, img_mtime) = row?;
        let norm_fp = fp.replace('\\', "/");
        let rel = if norm_fp.starts_with(&root_prefix) {
            &norm_fp[root_prefix.len()..]
        } else if norm_fp.to_lowercase().starts_with(&root_prefix_lower) {
            let prefix_chars = root_prefix.chars().count();
            match norm_fp.char_indices().nth(prefix_chars) {
                Some((byte_idx, _)) => &norm_fp[byte_idx..],
                None => continue,
            }
        } else {
            continue;
        };

        if let Some((arch_rel, inner_rel)) = rel.split_once("!/") {
            let clean_arch = arch_rel.trim_matches('/').to_string();
            if clean_arch.is_empty() {
                continue;
            }
            *archive_counts.entry(clean_arch.clone()).or_insert(0) += 1;

            // アーカイブの表紙（自然順で最小のエントリ）を記録
            let inner_str = inner_rel.to_string();
            archive_covers
                .entry(clean_arch.clone())
                .and_modify(|(curr_min, curr_id, curr_rev)| {
                    if crate::plugins::compare_natural(&inner_str, curr_min) == std::cmp::Ordering::Less {
                        *curr_min = inner_str.clone();
                        *curr_id = img_id;
                        *curr_rev = img_mtime;
                    }
                })
                .or_insert_with(|| (inner_str, img_id, img_mtime));

            // アーカイブの親ディレクトリが存在する場合、ツリー上に親ディレクトリノードを確保（件数自体は通常画像のみ加算するため0初期化）
            let arch_parts: Vec<&str> = clean_arch.split('/').filter(|s| !s.is_empty()).collect();
            if arch_parts.len() > 1 {
                let mut accum = String::new();
                for (idx, part) in arch_parts[..arch_parts.len() - 1].iter().enumerate() {
                    if idx > 0 {
                        accum.push('/');
                    }
                    accum.push_str(part);
                    dir_counts.entry(accum.clone()).or_insert(0);
                }
            }

            // アーカイブ内部のサブディレクトリ階層を集計
            let inner_parts: Vec<&str> = inner_rel.split('/').filter(|s| !s.is_empty()).collect();
            if inner_parts.len() > 1 {
                let sub_map = archive_sub_counts.entry(clean_arch).or_default();
                let mut inner_accum = String::new();
                for (idx, part) in inner_parts[..inner_parts.len() - 1].iter().enumerate() {
                    if idx > 0 {
                        inner_accum.push('/');
                    }
                    inner_accum.push_str(part);
                    *sub_map.entry(inner_accum.clone()).or_insert(0) += 1;
                }
            }
        } else {
            let parts: Vec<&str> = rel.split('/').filter(|s| !s.is_empty()).collect();
            if parts.len() <= 1 {
                continue;
            }
            let dir_parts = &parts[..parts.len() - 1];
            let mut accum = String::new();
            for (idx, part) in dir_parts.iter().enumerate() {
                if idx > 0 {
                    accum.push('/');
                }
                accum.push_str(part);
                *dir_counts.entry(accum.clone()).or_insert(0) += 1;
            }
        }
    }

    // 2. 監視フォルダ配下のディスク上に存在するアーカイブファイル（未展開含む）を軽量走査してマージ
    let ext_set: HashSet<String> = archive_extensions
        .iter()
        .map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase())
        .filter(|e| !e.is_empty())
        .collect();

    let mut discovered_archives: BTreeMap<String, Option<i64>> = BTreeMap::new();
    for (arch_rel, cnt) in &archive_counts {
        discovered_archives.insert(arch_rel.clone(), Some(*cnt));
    }

    if !ext_set.is_empty() && Path::new(&root_path).is_dir() {
        for entry in walkdir::WalkDir::new(&root_path)
            .follow_links(false)
            .max_depth(8)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let p = entry.path();
            let ext = match p.extension().and_then(|e| e.to_str()) {
                Some(e) => e.to_ascii_lowercase(),
                None => continue,
            };
            if !ext_set.contains(&ext) {
                continue;
            }

            let norm_p = p.to_string_lossy().replace('\\', "/");
            let rel = if norm_p.starts_with(&root_prefix) {
                norm_p[root_prefix.len()..].to_string()
            } else if norm_p.to_lowercase().starts_with(&root_prefix_lower) {
                let prefix_chars = root_prefix.chars().count();
                match norm_p.char_indices().nth(prefix_chars) {
                    Some((byte_idx, _)) => norm_p[byte_idx..].to_string(),
                    None => continue,
                }
            } else {
                continue;
            };

            let clean_rel = rel.trim_matches('/').to_string();
            if clean_rel.is_empty() {
                continue;
            }

            discovered_archives
                .entry(clean_rel.clone())
                .or_insert_with(|| archive_counts.get(&clean_rel).copied());

            // アーカイブが格納されている親ディレクトリもツリーに表示されるよう保証
            let parts: Vec<&str> = clean_rel.split('/').filter(|s| !s.is_empty()).collect();
            if parts.len() > 1 {
                let mut accum = String::new();
                for (idx, part) in parts[..parts.len() - 1].iter().enumerate() {
                    if idx > 0 {
                        accum.push('/');
                    }
                    accum.push_str(part);
                    dir_counts.entry(accum.clone()).or_insert(0);
                }
            }
        }
    }

    // 3. フラットなパス群から再帰的な FolderTreeNode ツリーを組み立てるヘルパー
    fn build_nested_dir_tree(
        parent_rel: &str,
        dir_map: &BTreeMap<String, i64>,
        archives_by_parent: &HashMap<String, Vec<FolderTreeNode>>,
    ) -> Vec<FolderTreeNode> {
        let mut nodes = Vec::new();
        let target_depth = if parent_rel.is_empty() {
            1
        } else {
            parent_rel.split('/').count() + 1
        };
        let prefix = if parent_rel.is_empty() {
            String::new()
        } else {
            format!("{}/", parent_rel)
        };

        for (rel_path, &cnt) in dir_map {
            let depth = rel_path.split('/').count();
            if depth != target_depth {
                continue;
            }
            if !prefix.is_empty() && !rel_path.starts_with(&prefix) {
                continue;
            }
            let name = rel_path
                .rsplit('/')
                .next()
                .unwrap_or(rel_path)
                .to_string();
            let children = build_nested_dir_tree(rel_path, dir_map, archives_by_parent);
            nodes.push(FolderTreeNode {
                kind: "directory".to_string(),
                path: rel_path.clone(),
                name,
                image_count: Some(cnt),
                cover_image_id: None,
                cover_rev: None,
                children,
            });
        }

        if let Some(arch_list) = archives_by_parent.get(parent_rel) {
            nodes.extend(arch_list.iter().cloned());
        }

        // ディレクトリを先に、アーカイブを後に、それぞれ自然順（Natural Sort）でソート
        nodes.sort_by(|a, b| {
            let kind_ord = (a.kind == "archive").cmp(&(b.kind == "archive"));
            if kind_ord != std::cmp::Ordering::Equal {
                return kind_ord;
            }
            crate::plugins::compare_natural(&a.name, &b.name)
        });

        nodes
    }

    fn build_archive_inner_tree(
        arch_rel: &str,
        parent_inner: &str,
        sub_map: &BTreeMap<String, i64>,
    ) -> Vec<FolderTreeNode> {
        let mut nodes = Vec::new();
        let target_depth = if parent_inner.is_empty() {
            1
        } else {
            parent_inner.split('/').count() + 1
        };
        let prefix = if parent_inner.is_empty() {
            String::new()
        } else {
            format!("{}/", parent_inner)
        };

        for (inner_rel, &cnt) in sub_map {
            let depth = inner_rel.split('/').count();
            if depth != target_depth {
                continue;
            }
            if !prefix.is_empty() && !inner_rel.starts_with(&prefix) {
                continue;
            }
            let name = inner_rel
                .rsplit('/')
                .next()
                .unwrap_or(inner_rel)
                .to_string();
            let children = build_archive_inner_tree(arch_rel, inner_rel, sub_map);
            nodes.push(FolderTreeNode {
                kind: "directory".to_string(),
                path: format!("{}!/{}", arch_rel, inner_rel),
                name,
                image_count: Some(cnt),
                cover_image_id: None,
                cover_rev: None,
                children,
            });
        }

        nodes.sort_by(|a, b| crate::plugins::compare_natural(&a.name, &b.name));
        nodes
    }

    let mut archives_by_parent: HashMap<String, Vec<FolderTreeNode>> = HashMap::new();
    for (arch_rel, count_opt) in discovered_archives {
        let (parent_dir, file_name) = match arch_rel.rsplit_once('/') {
            Some((p, n)) => (p.to_string(), n.to_string()),
            None => (String::new(), arch_rel.clone()),
        };

        let inner_children = archive_sub_counts
            .get(&arch_rel)
            .map(|sub_map| build_archive_inner_tree(&arch_rel, "", sub_map))
            .unwrap_or_default();

        let cover_info = archive_covers.get(&arch_rel);
        let cover_image_id = cover_info.map(|c| c.1);
        let cover_rev = cover_info.map(|c| c.2);

        archives_by_parent
            .entry(parent_dir)
            .or_default()
            .push(FolderTreeNode {
                kind: "archive".to_string(),
                path: arch_rel,
                name: file_name,
                image_count: count_opt,
                cover_image_id,
                cover_rev,
                children: inner_children,
            });
    }

    Ok(build_nested_dir_tree("", &dir_counts, &archives_by_parent))
}

/// 登録されている全タグ一覧（付与されている画像枚数付き）を取得する
///
/// @param conn DB読み取り接続
/// @return TagWithCount の配列
pub fn get_tags(conn: &Connection) -> Result<Vec<TagWithCount>> {
    let mut stmt = conn.prepare_cached(
        "SELECT t.id, t.name, t.color, t.sort_order, t.created_at,
                COUNT(i.id) AS image_count
         FROM tags t
         LEFT JOIN image_tags it ON t.id = it.tag_id
         LEFT JOIN images i ON it.image_id = i.id AND i.folder_id > 0
         GROUP BY t.id
         ORDER BY t.sort_order ASC, t.name COLLATE NOCASE ASC, t.id ASC;",
    )?;

    let rows = stmt.query_map([], |r| {
        Ok(TagWithCount {
            id: r.get(0)?,
            name: r.get(1)?,
            color: r.get(2)?,
            sort_order: r.get(3)?,
            created_at: r.get(4)?,
            image_count: r.get(5)?,
        })
    })?;

    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// 新規タグを作成する（同名タグが既に存在する場合は既存のTagRecordを返す）
///
/// @param conn DB書き込み接続
/// @param name タグ名
/// @param color カラーコード（省略時は "#6366f1"）
/// @param now 作成日時UNIX秒
/// @return 作成または既存の TagRecord
pub fn create_tag(
    conn: &Connection,
    name: &str,
    color: Option<&str>,
    now: i64,
) -> Result<TagRecord> {
    let clean_name = name.trim();
    let tag_color = color
        .map(|c| c.trim())
        .filter(|c| !c.is_empty())
        .unwrap_or("#6366f1");

    // 既存チェック（COLLATE NOCASE）
    if let Ok(existing) = conn.query_row(
        "SELECT id, name, color, sort_order, created_at FROM tags WHERE name = ?1 COLLATE NOCASE;",
        params![clean_name],
        |r| {
            Ok(TagRecord {
                id: r.get(0)?,
                name: r.get(1)?,
                color: r.get(2)?,
                sort_order: r.get(3)?,
                created_at: r.get(4)?,
            })
        },
    ) {
        return Ok(existing);
    }

    let max_order: i32 = conn
        .query_row(
            "SELECT COALESCE(MAX(sort_order), -1) FROM tags;",
            [],
            |r| r.get(0),
        )
        .unwrap_or(-1);
    let next_order = max_order + 1;

    conn.execute(
        "INSERT INTO tags (name, color, sort_order, created_at) VALUES (?1, ?2, ?3, ?4);",
        params![clean_name, tag_color, next_order, now],
    )?;
    let id = conn.last_insert_rowid();

    Ok(TagRecord {
        id,
        name: clean_name.to_string(),
        color: tag_color.to_string(),
        sort_order: next_order,
        created_at: now,
    })
}

/// 既存タグの名前・カラー・表示順を更新する
pub fn update_tag(
    conn: &Connection,
    id: i64,
    name: Option<&str>,
    color: Option<&str>,
    sort_order: Option<i32>,
) -> Result<TagRecord> {
    if let Some(n) = name {
        let clean = n.trim();
        if !clean.is_empty() {
            conn.execute(
                "UPDATE tags SET name = ?1 WHERE id = ?2;",
                params![clean, id],
            )?;
        }
    }
    if let Some(c) = color {
        let clean = c.trim();
        if !clean.is_empty() {
            conn.execute(
                "UPDATE tags SET color = ?1 WHERE id = ?2;",
                params![clean, id],
            )?;
        }
    }
    if let Some(so) = sort_order {
        conn.execute(
            "UPDATE tags SET sort_order = ?1 WHERE id = ?2;",
            params![so, id],
        )?;
    }

    conn.query_row(
        "SELECT id, name, color, sort_order, created_at FROM tags WHERE id = ?1;",
        params![id],
        |r| {
            Ok(TagRecord {
                id: r.get(0)?,
                name: r.get(1)?,
                color: r.get(2)?,
                sort_order: r.get(3)?,
                created_at: r.get(4)?,
            })
        },
    )
}

/// タグを削除する（ON DELETE CASCADEによりimage_tagsも自動削除）
pub fn delete_tag(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM tags WHERE id = ?1;", params![id])?;
    Ok(())
}

/// 指定した画像ID群に対する全タグの付与状態（"all" / "some" / "none"）を取得する
///
/// 変更理由: 複数画像を選択してタグ編集モーダルを開いた際、全員に付いているタグ（☑）、
/// 一部のみに付いているタグ（[-]）、誰にも付いていないタグ（☐）を即座に判別するため。
pub fn get_tags_for_images(
    conn: &Connection,
    image_ids: &[i64],
) -> Result<Vec<ImageTagAssignment>> {
    let all_tags = get_tags(conn)?;
    if all_tags.is_empty() {
        return Ok(Vec::new());
    }

    let mut unique_img_ids: Vec<i64> = image_ids.to_vec();
    unique_img_ids.sort_unstable();
    unique_img_ids.dedup();
    let total_targets = unique_img_ids.len();

    if total_targets == 0 {
        return Ok(all_tags
            .into_iter()
            .map(|t| ImageTagAssignment {
                tag_id: t.id,
                name: t.name,
                color: t.color,
                status: "none".to_string(),
            })
            .collect());
    }

    let mut assigned_counts: HashMap<i64, usize> = HashMap::new();
    for chunk in unique_img_ids.chunks(500) {
        let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT tag_id, COUNT(DISTINCT image_id) FROM image_tags WHERE image_id IN ({}) GROUP BY tag_id;",
            placeholders
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(chunk.iter()), |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)? as usize))
        })?;
        for r in rows {
            let (tid, cnt) = r?;
            *assigned_counts.entry(tid).or_insert(0) += cnt;
        }
    }

    let result = all_tags
        .into_iter()
        .map(|t| {
            let cnt = assigned_counts.get(&t.id).copied().unwrap_or(0);
            let status = if cnt == 0 {
                "none"
            } else if cnt >= total_targets {
                "all"
            } else {
                "some"
            };
            ImageTagAssignment {
                tag_id: t.id,
                name: t.name,
                color: t.color,
                status: status.to_string(),
            }
        })
        .collect();

    Ok(result)
}

/// 複数画像に対してタグの一括追加および一括解除を単一トランザクションで高速実行する
///
/// @param conn DB書き込み接続
/// @param image_ids 対象画像IDリスト
/// @param add_tag_ids 付与するタグIDリスト
/// @param remove_tag_ids 解除するタグIDリスト
/// @param now 現在時刻UNIX秒
pub fn update_images_tags(
    conn: &mut Connection,
    image_ids: &[i64],
    add_tag_ids: &[i64],
    remove_tag_ids: &[i64],
    now: i64,
) -> Result<()> {
    if image_ids.is_empty() || (add_tag_ids.is_empty() && remove_tag_ids.is_empty()) {
        return Ok(());
    }

    let mut unique_imgs: Vec<i64> = image_ids.to_vec();
    unique_imgs.sort_unstable();
    unique_imgs.dedup();

    let remove_set: HashSet<i64> = remove_tag_ids.iter().copied().collect();
    let add_list: Vec<i64> = add_tag_ids
        .iter()
        .copied()
        .filter(|tid| !remove_set.contains(tid))
        .collect();

    let tx = conn.transaction()?;
    {
        if !remove_tag_ids.is_empty() {
            let mut del_stmt = tx.prepare_cached(
                "DELETE FROM image_tags WHERE tag_id = ?1 AND image_id = ?2;",
            )?;
            for &tid in remove_tag_ids {
                for &iid in &unique_imgs {
                    del_stmt.execute(params![tid, iid])?;
                }
            }
        }

        if !add_list.is_empty() {
            let mut ins_stmt = tx.prepare_cached(
                "INSERT OR IGNORE INTO image_tags (tag_id, image_id, created_at) VALUES (?1, ?2, ?3);",
            )?;
            for &tid in &add_list {
                for &iid in &unique_imgs {
                    ins_stmt.execute(params![tid, iid, now])?;
                }
            }
        }
    }
    tx.commit()?;
    Ok(())
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

    #[test]
    fn test_parse_search_query_tokens_and_tags_and_filters() {
        // 1. 検索クエリトークン分割のテスト（空白・全角空白・マイナスプレフィックス除外語）
        let (inc, exc) = parse_search_query_tokens("pose  hand　-draft -sketch -");
        assert_eq!(inc, vec!["pose".to_string(), "hand".to_string()]);
        assert_eq!(exc, vec!["draft".to_string(), "sketch".to_string()]);

        // 2. インメモリDBの作成とマイグレーションv10の検証
        let db = Database::open_in_memory().expect("インメモリDBのオープンに失敗");
        let mut conn = db.writer();

        let folder = add_watched_folder(&conn, "D:/artworks", 1700000000).expect("フォルダ登録失敗");

        // サブフォルダ構造・縦横比・拡張子が異なる3枚のテスト画像を登録
        let img1 = upsert_single_image_and_get_id(
            &conn,
            &NewImageRecord {
                folder_id: folder.id,
                file_path: "D:/artworks/chara/hero_pose_01.png".to_string(),
                file_size: 1000,
                file_mtime: 1700000100,
                taken_at: 1700000100,
                taken_at_source: 1,
                width: Some(1920),
                height: Some(1080), // landscape
                orientation: 1,
                format: Some("png".to_string()),
                quick_hash: "h_img1".to_string(),
                indexed_at: 1700000100,
                source_url: None,
            },
        )
        .unwrap();

        let img2 = upsert_single_image_and_get_id(
            &conn,
            &NewImageRecord {
                folder_id: folder.id,
                file_path: "D:/artworks/chara/sub/heroine_portrait_draft.jpg".to_string(),
                file_size: 2000,
                file_mtime: 1700000200,
                taken_at: 1700000200,
                taken_at_source: 1,
                width: Some(1000),
                height: Some(1500), // portrait
                orientation: 1,
                format: Some("jpeg".to_string()),
                quick_hash: "h_img2".to_string(),
                indexed_at: 1700000200,
                source_url: None,
            },
        )
        .unwrap();

        let img3 = upsert_single_image_and_get_id(
            &conn,
            &NewImageRecord {
                folder_id: folder.id,
                file_path: "D:/artworks/bg/icon_square.webp".to_string(),
                file_size: 3000,
                file_mtime: 1700000300,
                taken_at: 1700000300,
                taken_at_source: 1,
                width: Some(500),
                height: Some(500), // square
                orientation: 1,
                format: Some("webp".to_string()),
                quick_hash: "h_img3".to_string(),
                indexed_at: 1700000300,
                source_url: None,
            },
        )
        .unwrap();

        // 3. サブフォルダ階層ツリーのテスト
        let tree = get_subfolder_tree(&conn, folder.id).expect("サブフォルダツリー取得失敗");
        // "bg" (1枚), "chara" (配下含め2枚), "chara/sub" (1枚)
        assert_eq!(tree.len(), 3);
        let chara_node = tree.iter().find(|e| e.relative_path == "chara").unwrap();
        assert_eq!(chara_node.image_count, 2);
        assert_eq!(chara_node.depth, 1);
        let sub_node = tree.iter().find(|e| e.relative_path == "chara/sub").unwrap();
        assert_eq!(sub_node.image_count, 1);
        assert_eq!(sub_node.depth, 2);

        // 4. タグ作成・一括付与・部分付与状態（all / some / none）のテスト
        let tag_fav = create_tag(&conn, "お気に入り", Some("#ef4444"), 1700000500).unwrap();
        let tag_ref = create_tag(&conn, "資料", Some("#3b82f6"), 1700000501).unwrap();
        let tag_unused = create_tag(&conn, "未使用", None, 1700000502).unwrap();

        // img1, img2 の両方に tag_fav を付与し、img1 のみに tag_ref を付与
        update_images_tags(&mut conn, &[img1, img2], &[tag_fav.id], &[], 1700000600).unwrap();
        update_images_tags(&mut conn, &[img1], &[tag_ref.id], &[], 1700000601).unwrap();

        let assignments = get_tags_for_images(&conn, &[img1, img2]).unwrap();
        let fav_state = assignments.iter().find(|a| a.tag_id == tag_fav.id).unwrap();
        let ref_state = assignments.iter().find(|a| a.tag_id == tag_ref.id).unwrap();
        let unused_state = assignments.iter().find(|a| a.tag_id == tag_unused.id).unwrap();
        assert_eq!(fav_state.status, "all");
        assert_eq!(ref_state.status, "some");
        assert_eq!(unused_state.status, "none");

        // 詳細取得時にタグが含まれることを確認
        let detail1 = get_image_detail(&conn, img1).unwrap().unwrap();
        assert_eq!(detail1.tags.len(), 2);

        // 5. 複合フィルタ検索のテスト
        // (a) サブフォルダ "chara" 絞り込み -> img1, img2
        let f_sub = crate::models::TimelineFilter {
            sub_folder_path: Some("chara".to_string()),
            ..Default::default()
        };
        let res_sub = get_timeline_images_filtered(&conn, 0, 50, Some(folder.id), None, Some(&f_sub)).unwrap();
        assert_eq!(res_sub.len(), 2);

        // (b) ファイル名検索 "hero -draft" (AND + NOT) -> img1 のみ（img2 は draft を含むため除外）
        let f_search = crate::models::TimelineFilter {
            query: Some("hero -draft".to_string()),
            ..Default::default()
        };
        let res_search = get_timeline_images_filtered(&conn, 0, 50, Some(folder.id), None, Some(&f_search)).unwrap();
        assert_eq!(res_search.len(), 1);
        assert_eq!(res_search[0].id, img1);

        // (c) 拡張子 jpg 指定（jpeg 同義語展開確認） -> img2 のみ
        let f_fmt = crate::models::TimelineFilter {
            extensions: Some(vec!["jpg".to_string()]),
            ..Default::default()
        };
        let res_fmt = get_timeline_images_filtered(&conn, 0, 50, Some(folder.id), None, Some(&f_fmt)).unwrap();
        assert_eq!(res_fmt.len(), 1);
        assert_eq!(res_fmt[0].id, img2);

        // (d) 縦横比 square 指定 -> img3 のみ
        let f_ar = crate::models::TimelineFilter {
            aspect_ratio: Some("square".to_string()),
            ..Default::default()
        };
        let res_ar = get_timeline_images_filtered(&conn, 0, 50, Some(folder.id), None, Some(&f_ar)).unwrap();
        assert_eq!(res_ar.len(), 1);
        assert_eq!(res_ar[0].id, img3);

        // (e) タグ絞り込み：tag_fav を含み、tag_ref を除外(NOT) -> img2 のみ
        let f_tag = crate::models::TimelineFilter {
            include_tag_ids: Some(vec![tag_fav.id]),
            exclude_tag_ids: Some(vec![tag_ref.id]),
            ..Default::default()
        };
        let res_tag = get_timeline_images_filtered(&conn, 0, 50, Some(folder.id), None, Some(&f_tag)).unwrap();
        assert_eq!(res_tag.len(), 1);
        assert_eq!(res_tag[0].id, img2);
        let sum_tag = get_timeline_summary_filtered(&conn, Some(folder.id), 1, None, Some(&f_tag)).unwrap();
        assert_eq!(sum_tag.total, 1);
    }

    #[test]
    fn test_folder_hierarchy_discovers_unopened_archives_on_disk() {
        // 変更理由: 未展開（DB未登録）のZIPがディスク走査でサイドバーツリーに検出されることを検証するため
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("a.zip"), b"PK").unwrap();
        std::fs::write(root.join("sub").join("b.CBZ"), b"PK").unwrap();

        let db = Database::open_in_memory().unwrap();
        let conn = db.writer();
        let folder = add_watched_folder(&conn, &root.to_string_lossy(), 1700000000).unwrap();
        let tree = get_folder_hierarchy(&conn, folder.id, &["zip".to_string(), "cbz".to_string()]).unwrap();

        assert!(tree.iter().any(|n| n.kind == "archive" && n.name == "a.zip" && n.image_count.is_none()));
        let sub = tree.iter().find(|n| n.kind == "directory" && n.name == "sub").expect("sub未検出");
        assert!(sub.children.iter().any(|n| n.kind == "archive" && n.name == "b.CBZ"));
    }

    #[test]
    fn test_archive_virtual_folder_exclusion_and_natural_sort() {
        let db = Database::open_in_memory().expect("インメモリDBのオープンに失敗");
        let mut conn = db.writer();

        let folder = add_watched_folder(&conn, "D:/manga", 1700000000).expect("フォルダ登録失敗");

        // 1. 通常フォルダ内の画像 1枚を登録
        let normal_img = upsert_single_image_and_get_id(
            &conn,
            &NewImageRecord {
                folder_id: folder.id,
                file_path: "D:/manga/cover.png".to_string(),
                file_size: 1200,
                file_mtime: 1700000100,
                taken_at: 1700000100,
                taken_at_source: 1,
                width: Some(800),
                height: Some(1200),
                orientation: 1,
                format: Some("png".to_string()),
                quick_hash: "h_cover".to_string(),
                indexed_at: 1700000100,
                source_url: None,
            },
        )
        .unwrap();

        // 2. アーカイブ仮想フォルダのエントリ（10.jpg, 1.jpg, 2.jpg）をオンデマンド登録
        let archive_records = vec![
            NewImageRecord {
                folder_id: folder.id,
                file_path: "D:/manga/book.zip!/vol1/10.jpg".to_string(),
                file_size: 5000,
                file_mtime: 1700000500,
                taken_at: 1700000200,
                taken_at_source: 1,
                width: None,
                height: None,
                orientation: 1,
                format: Some("jpg".to_string()),
                quick_hash: "h_10".to_string(),
                indexed_at: 1700000600,
                source_url: None,
            },
            NewImageRecord {
                folder_id: folder.id,
                file_path: "D:/manga/book.zip!/vol1/1.jpg".to_string(),
                file_size: 4000,
                file_mtime: 1700000500,
                taken_at: 1700000200,
                taken_at_source: 1,
                width: None,
                height: None,
                orientation: 1,
                format: Some("jpg".to_string()),
                quick_hash: "h_1".to_string(),
                indexed_at: 1700000600,
                source_url: None,
            },
            NewImageRecord {
                folder_id: folder.id,
                file_path: "D:/manga/book.zip!/vol1/2.jpg".to_string(),
                file_size: 4500,
                file_mtime: 1700000500,
                taken_at: 1700000200,
                taken_at_source: 1,
                width: None,
                height: None,
                orientation: 1,
                format: Some("jpg".to_string()),
                quick_hash: "h_2".to_string(),
                indexed_at: 1700000600,
                source_url: None,
            },
        ];
        let count = upsert_archive_entries(
            &mut conn,
            folder.id,
            "D:/manga/book.zip",
            &archive_records,
        )
        .expect("アーカイブエントリ登録失敗");
        assert_eq!(count, 3);

        // 3. 「すべての写真」および「監視フォルダ直下」ではアーカイブ内画像が除外され、cover.png(1枚)のみ返ること
        let all_photos = get_timeline_images_filtered(&conn, 0, 50, None, None, None).unwrap();
        assert_eq!(all_photos.len(), 1);
        assert_eq!(all_photos[0].id, normal_img);

        let root_folder = get_timeline_images_filtered(&conn, 0, 50, Some(folder.id), None, None).unwrap();
        assert_eq!(root_folder.len(), 1);
        assert_eq!(root_folder[0].id, normal_img);

        let watched = get_watched_folders(&conn).unwrap();
        assert_eq!(watched[0].image_count, 1);

        // 4. アーカイブ仮想フォルダ "book.zip" 選択時はアーカイブ内3枚が取得され、自然順 (1.jpg -> 2.jpg -> 10.jpg) に並ぶこと
        let f_archive = crate::models::TimelineFilter {
            sub_folder_path: Some("book.zip".to_string()),
            ..Default::default()
        };
        let archive_imgs = get_timeline_images_filtered(
            &conn,
            0,
            50,
            Some(folder.id),
            Some(crate::models::TimelineSort::NameAsc),
            Some(&f_archive),
        )
        .unwrap();
        assert_eq!(archive_imgs.len(), 3);

        // 各IDのfile_pathを確認して自然順になっているか検証
        let paths: Vec<String> = archive_imgs
            .iter()
            .map(|r| {
                conn.query_row(
                    "SELECT file_path FROM images WHERE id = ?1",
                    params![r.id],
                    |row| row.get::<_, String>(0),
                )
                .unwrap()
            })
            .collect();
        assert_eq!(
            paths,
            vec![
                "D:/manga/book.zip!/vol1/1.jpg".to_string(),
                "D:/manga/book.zip!/vol1/2.jpg".to_string(),
                "D:/manga/book.zip!/vol1/10.jpg".to_string(),
            ]
        );

        // 5. アーカイブ本体ファイル自身（D:/manga/book.zip）が登録された場合、通常画像と混在してタイムラインに選出され、is_archiveがtrueになること
        let arch_record = NewImageRecord {
            folder_id: folder.id,
            file_path: "D:/manga/book.zip".to_string(),
            file_size: 50000,
            file_mtime: 1700000500,
            taken_at: 1700000500,
            taken_at_source: 1,
            width: None,
            height: None,
            orientation: 1,
            format: Some("zip".to_string()),
            quick_hash: "h_zip_root".to_string(),
            indexed_at: 1700000600,
            source_url: None,
        };
        let arch_img_id = upsert_single_image_and_get_id(&conn, &arch_record).expect("アーカイブ本体登録失敗");

        // 「監視フォルダ直下」で通常画像 + アーカイブ本体の計2件が取得できること
        let mixed = get_timeline_images_filtered(&conn, 0, 50, Some(folder.id), None, None).unwrap();
        assert_eq!(mixed.len(), 2);

        let arch_item = mixed.iter().find(|r| r.id == arch_img_id).expect("アーカイブ本体レコードが見つかること");
        assert_eq!(arch_item.is_archive, Some(true));
        assert_eq!(arch_item.file_name.as_deref(), Some("book.zip"));
        assert_eq!(arch_item.file_path.as_deref(), Some("D:/manga/book.zip"));

        let normal_item = mixed.iter().find(|r| r.id == normal_img).expect("通常画像レコードが見つかること");
        assert_eq!(normal_item.is_archive, None);
        assert_eq!(normal_item.file_name, None);
    }
}



