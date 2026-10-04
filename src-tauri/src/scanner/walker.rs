use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::UNIX_EPOCH;
use rayon::prelude::*;
use tauri::{AppHandle, Emitter};
use tracing::{info, warn};
use walkdir::WalkDir;

use crate::db::repo::NewImageRecord;
use crate::error::AppError;
use crate::models::{ScanPhase, ScanProgress};
use crate::pipeline::exif::extract_metadata;
use crate::pipeline::hash::calculate_quick_hash;
use crate::pipeline::JobPriority;
use crate::state::AppState;

const BATCH_SIZE: usize = 500;
const SUPPORTED_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "gif", "bmp"];

/// パスを正規化する（Windowsの拡張プレフィックス除去、ドライブ文字大文字化、セパレータ統一）
///
/// 変更理由: 仕様書§2.2(F-01)「パスは正規化して保存（Windowsは\\?\プレフィックス除去・ドライブレター大文字化）」
pub fn normalize_path(path_str: &str) -> String {
    let mut p = path_str.trim().replace('/', "\\");

    // \\?\ プレフィックス除去
    if p.starts_with(r"\\?\") {
        p = p[4..].to_string();
    }

    // ドライブレターの大文字化 (例: c:\ -> C:\)
    if p.len() >= 2 && p.chars().nth(1) == Some(':') {
        let drive = p.chars().next().unwrap().to_ascii_uppercase();
        p = format!("{}{}", drive, &p[1..]);
    }

    // 末尾のスラッシュ・バックスラッシュ除去（ルートドライブ C:\ を除く）
    if p.len() > 3 && p.ends_with('\\') {
        p.pop();
    }

    p
}

/// 拡張子が対応画像フォーマットであるか判定
fn is_supported_image(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            let lower = ext.to_ascii_lowercase();
            SUPPORTED_EXTENSIONS.contains(&lower.as_str())
        })
        .unwrap_or(false)
}

/// スキップすべき隠しディレクトリ・ファイルか判定
fn is_hidden_or_ignored(entry: &walkdir::DirEntry) -> bool {
    let name = entry.file_name().to_string_lossy();
    if name.starts_with('.') {
        return true;
    }
    let upper = name.to_ascii_uppercase();
    if upper == "$RECYCLE.BIN" || upper == "@EADIR" || upper == "SYSTEM VOLUME INFORMATION" {
        return true;
    }
    // シンボリックリンクはスキップ (§2.2 F-02)
    entry.path_is_symlink()
}

/// フォルダの再帰走査およびDBインデックス処理を実行する（共通コアロジック）
///
/// 変更理由: Tauri UI起動時およびHTTPサーバー経由の両方からヘッドレス走査を安全に実行可能にするため。
/// また、アイドル時バックグラウンド走査ではスロットリングによりCPU使用率とファン稼働を極小化する
///
/// @param app Tauri AppHandle（UI通知用、ヘッドレス時はNone）
/// @param state アプリケーション共有状態
/// @param folder_id 監視フォルダID
/// @param folder_path フォルダの絶対パス
/// @param is_background バックグラウンド省電力走査フラグ（true時はスロットリングを適用しユーザー操作時に即座に中断）
pub fn scan_folder_core(
    app: Option<&AppHandle>,
    state: Arc<AppState>,
    folder_id: i64,
    folder_path: String,
    is_background: bool,
) -> Result<(), AppError> {
    let root = Path::new(&folder_path);
    if !root.exists() {
        warn!("走査対象ルートフォルダが存在しません: {}", folder_path);
        return Ok(());
    }

    let cancel_flag = &state.is_scan_cancelled;
    cancel_flag.store(false, Ordering::SeqCst);

    // 1. Walking フェーズ
    if let Some(app) = app {
        let _ = app.emit(
            "scan-progress",
            ScanProgress {
                folder_id,
                phase: ScanPhase::Walking,
                discovered: 0,
                processed: 0,
            },
        );
    }

    // 既存の (path -> (size, mtime)) キャッシュをDBから取得
    let existing_map: HashMap<String, (u64, i64)> = {
        let conn = state.db.reader()?;
        let mut stmt = conn.prepare(
            "SELECT file_path, file_size, file_mtime FROM images WHERE folder_id = ?1;",
        )?;
        let rows = stmt.query_map([folder_id], |r| {
            Ok((r.get::<_, String>(0)?, (r.get::<_, i64>(1)? as u64, r.get::<_, i64>(2)?)))
        })?;
        let mut map = HashMap::new();
        for r in rows {
            let (p, sm) = r?;
            map.insert(p, sm);
        }
        map
    };

    let mut discovered_files = Vec::new();
    let mut current_paths_in_fs = HashSet::new();
    let mut file_count = 0usize;

    for entry in WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| !is_hidden_or_ignored(e))
    {
        if cancel_flag.load(Ordering::SeqCst) {
            info!("走査がキャンセルされました");
            return Ok(());
        }

        // バックグラウンド走査時: ユーザーのサムネイル要求が発生したら即座に中断してCPUを譲る
        if is_background && file_count % 50 == 0 && !state.thumb_pipeline.is_idle() {
            info!("ユーザー操作・サムネイル要求を検知したためバックグラウンド走査を一時中断します");
            return Ok(());
        }

        // バックグラウンド走査時: 100ファイル毎に5ms休止し、CPU使用率の急上昇とファンの回転を防止
        file_count += 1;
        if is_background && file_count % 100 == 0 {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                warn!("ディレクトリエントリ読み取り警告: {:?}", e);
                continue;
            }
        };

        if entry.file_type().is_file() {
            let path = entry.path();
            if is_supported_image(path) {
                let norm = normalize_path(&path.to_string_lossy());
                current_paths_in_fs.insert(norm.clone());

                if let Ok(meta) = fs::metadata(path) {
                    let size = meta.len();
                    let mtime = meta
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);

                    // 差分判定: DBに存在し、sizeとmtimeが同一ならスキップ (§6)
                    if let Some(&(ex_size, ex_mtime)) = existing_map.get(&norm) {
                        if ex_size == size && ex_mtime == mtime {
                            continue;
                        }
                    }

                    discovered_files.push((path.to_path_buf(), norm, size, mtime));
                }
            }
        }
    }

    let total_discovered = discovered_files.len() as u64;
    info!("フォルダ [{}] で差分対象画像 {} 件を発見", folder_path, total_discovered);

    // 2. Indexing フェーズ (並列ヘッダ読み + quick_hash + バッチ書き込み)
    if let Some(app) = app {
        let _ = app.emit(
            "scan-progress",
            ScanProgress {
                folder_id,
                phase: ScanPhase::Indexing,
                discovered: total_discovered,
                processed: 0,
            },
        );
    }

    let mut processed_count = 0u64;

    let chunk_size = if is_background { 100 } else { BATCH_SIZE };

    for chunk in discovered_files.chunks(chunk_size) {
        if cancel_flag.load(Ordering::SeqCst) {
            info!("走査がキャンセルされました");
            return Ok(());
        }

        // バックグラウンド走査時: ユーザー操作発生時は直ちに中断
        if is_background && !state.thumb_pipeline.is_idle() {
            info!("ユーザー操作・サムネイル要求を検知したためインデックス処理を一時中断します");
            return Ok(());
        }

        // rayon による並列メタデータ抽出
        let parsed_records: Vec<NewImageRecord> = chunk
            .par_iter()
            .filter_map(|(path, norm_path, size, mtime)| {
                let quick_hash = calculate_quick_hash(path, *size).ok()?;
                let meta = extract_metadata(path, *mtime);
                let now = chrono::Utc::now().timestamp();

                Some(NewImageRecord {
                    folder_id,
                    file_path: norm_path.clone(),
                    file_size: *size,
                    file_mtime: *mtime,
                    taken_at: meta.taken_at,
                    taken_at_source: meta.taken_at_source,
                    width: meta.width,
                    height: meta.height,
                    orientation: meta.orientation,
                    format: meta.format,
                    quick_hash,
                    indexed_at: now,
                })
            })
            .collect();

        // DBへバッチUPSERT
        {
            let mut conn = state.db.writer();
            if let Err(e) = crate::db::repo::upsert_images(&mut conn, &parsed_records) {
                warn!("バッチUPSERT失敗: {:?}", e);
            }
        }

        processed_count += chunk.len() as u64;
        if let Some(app) = app {
            let _ = app.emit(
                "scan-progress",
                ScanProgress {
                    folder_id,
                    phase: ScanPhase::Indexing,
                    discovered: total_discovered,
                    processed: processed_count,
                },
            );

            // カタログ変更通知 (UIへ順次反映)
            let ver = state.bump_catalog_version();
            let _ = app.emit("catalog-changed", serde_json::json!({ "version": ver }));
        } else {
            state.bump_catalog_version();
        }

        // バックグラウンド走査時: バッチ毎に30ms休止し、CPUコアの過熱とファンの回転を防止
        if is_background {
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
    }

    // 3. Cleanup フェーズ: 削除されたファイルの検知
    if let Some(app) = app {
        let _ = app.emit(
            "scan-progress",
            ScanProgress {
                folder_id,
                phase: ScanPhase::Cleanup,
                discovered: total_discovered,
                processed: processed_count,
            },
        );
    }

    let deleted_paths: Vec<String> = existing_map
        .keys()
        .filter(|p| !current_paths_in_fs.contains(*p))
        .cloned()
        .collect();

    if !deleted_paths.is_empty() {
        info!("削除されたファイル {} 件をDBから除去", deleted_paths.len());
        let conn = state.db.writer();
        for chunk in deleted_paths.chunks(500) {
            let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let sql = format!("DELETE FROM images WHERE folder_id = ?1 AND file_path IN ({});", placeholders);
            let mut params_vec: Vec<&dyn rusqlite::ToSql> = Vec::new();
            params_vec.push(&folder_id);
            for p in chunk {
                params_vec.push(p);
            }
            conn.execute(&sql, params_vec.as_slice()).ok();
        }
        let ver = state.bump_catalog_version();
        if let Some(app) = app {
            let _ = app.emit("catalog-changed", serde_json::json!({ "version": ver }));
        }
    }

    // 4. 未生成サムネイルをパイプライン（優先度 LOW）に投入
    // 変更理由: バックグラウンドアイドル走査で新規ファイルが0件の場合は無駄な大量再投入を回避
    let should_enqueue_thumbs = !is_background || total_discovered > 0;
    if should_enqueue_thumbs {
        let fetch_limit = if is_background { 100 } else { 2000 };
        let pending_thumbs = {
            let conn = state.db.reader()?;
            crate::db::repo::get_pending_thumb_images(&conn, fetch_limit)?
        };

        for (id, file_path_str, quick_hash) in pending_thumbs {
            state.thumb_pipeline.enqueue(
                id,
                PathBuf::from(file_path_str),
                quick_hash,
                1,
                None,
                None,
                JobPriority::Low,
            );
        }
    }

    // 走査完了記録
    let now = chrono::Utc::now().timestamp();
    {
        let conn = state.db.writer();
        crate::db::repo::update_last_scanned(&conn, folder_id, now).ok();
    }

    if let Some(app) = app {
        let _ = app.emit(
            "scan-progress",
            ScanProgress {
                folder_id,
                phase: ScanPhase::Done,
                discovered: total_discovered,
                processed: processed_count,
            },
        );
    }

    Ok(())
}

/// 既存Tauriコマンド用ラッパー
pub fn scan_folder(
    app: AppHandle,
    state: Arc<AppState>,
    folder_id: i64,
    folder_path: String,
) -> Result<(), AppError> {
    scan_folder_core(Some(&app), state, folder_id, folder_path, false)
}
