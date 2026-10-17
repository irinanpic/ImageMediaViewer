use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;
use tauri::{AppHandle, Emitter};

use crate::db::repo::ThumbSource;
use crate::db::{Database, ThumbnailStore};
use crate::models::{ThumbProgress, TimelineSort, TimelineSummary};
use crate::pipeline::ThumbnailPipeline;

/// サムネイル WebP バイナリのインメモリ LRU キャッシュ（PC: 2,048枚 ≒ 約60〜80MB / モバイル: 512枚 ≒ 約15〜20MB）
///
/// 変更理由: VecDeque の線形探索 `position()` による $O(N)$ ロック保持を解消するため、
/// アクセス世代カウンタ（`access_counter`）を用いた遅延破棄方式により `get` / `put` / `remove` をすべて償却 $O(1)$ 化。
struct ThumbBinaryLruCache {
    map: HashMap<String, (Arc<Vec<u8>>, u64)>,
    order: VecDeque<(String, u64)>,
    access_counter: u64,
    max_items: usize,
}

impl ThumbBinaryLruCache {
    fn new(max_items: usize) -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            access_counter: 0,
            max_items,
        }
    }

    /// キャッシュから取得し、アクセス世代を更新してキュー末尾に積む（償却 O(1)）
    fn get(&mut self, hash: &str) -> Option<Arc<Vec<u8>>> {
        let entry = self.map.get_mut(hash)?;
        self.access_counter = self.access_counter.wrapping_add(1);
        let gen = self.access_counter;
        entry.1 = gen;
        let val = Arc::clone(&entry.0);
        self.order.push_back((hash.to_string(), gen));
        self.compact_order_if_needed();
        Some(val)
    }

    /// キャッシュに格納する（既存時は世代更新、上限到達時は有効な最古エントリを削除：償却 O(1)）
    fn put(&mut self, hash: String, data: Arc<Vec<u8>>) {
        self.access_counter = self.access_counter.wrapping_add(1);
        let gen = self.access_counter;

        if let Some(entry) = self.map.get_mut(&hash) {
            entry.0 = data;
            entry.1 = gen;
            self.order.push_back((hash, gen));
            self.compact_order_if_needed();
            return;
        }

        while self.map.len() >= self.max_items {
            if let Some((oldest_key, oldest_gen)) = self.order.pop_front() {
                // map内の最新世代と一致する場合のみ実際に削除（古い世代の履歴は読み飛ばす）
                if self.map.get(&oldest_key).map(|(_, g)| *g) == Some(oldest_gen) {
                    self.map.remove(&oldest_key);
                }
            } else {
                break;
            }
        }

        self.order.push_back((hash.clone(), gen));
        self.map.insert(hash, (data, gen));
        self.compact_order_if_needed();
    }

    /// キャッシュから削除する（O(1)：order内の古いタプルはpop時に自然破棄される）
    fn remove(&mut self, hash: &str) {
        self.map.remove(hash);
    }

    /// order キューが古い世代履歴で肥大化しすぎないよう定期的にコンパクションする
    fn compact_order_if_needed(&mut self) {
        let limit = self.max_items.saturating_mul(4).max(64);
        if self.order.len() > limit {
            let map_ref = &self.map;
            self.order
                .retain(|(k, g)| map_ref.get(k).map(|(_, cur_g)| *cur_g == *g).unwrap_or(false));
        }
    }
}

/// thumb_lookup_cache の最大保持件数（PC: 20,000件 / モバイル: 5,000件）
const MAX_LOOKUP_CACHE_ITEMS: usize = if cfg!(target_os = "android") {
    5_000
} else {
    20_000
};

/// アプリケーション全体で共有されるグローバル状態
///
/// 変更理由: DB接続、キャッシュパス、サムネイル生成パイプラインに加え、
/// 高速なレスポンスを実現するためのインメモリメタデータキャッシュ、
/// サムネイル進捗カウンタ（アトミック）、およびタイムラインサマリキャッシュを一元管理
pub struct AppState {
    /// データベース管理
    pub db: Database,
    /// サムネイル専用SQLiteデータベース管理
    pub thumb_store: Arc<ThumbnailStore>,
    /// アプリケーションデータディレクトリ（設定ファイル、DB保存先）
    pub data_dir: PathBuf,
    /// サムネイル等のキャッシュディレクトリ
    pub cache_dir: PathBuf,
    /// サムネイル生成バックグラウンドパイプライン
    pub thumb_pipeline: Arc<ThumbnailPipeline>,
    /// メモリキャッシュされたカタログバージョン
    pub catalog_version: AtomicI64,
    /// 走査キャンセル用フラグ
    pub is_scan_cancelled: Arc<AtomicBool>,
    /// サムネイル進捗カウンタ（総数、完了数、失敗数）
    pub thumb_total: AtomicU64,
    pub thumb_done: AtomicU64,
    pub thumb_failed: AtomicU64,
    /// Tauri AppHandle（UIイベント送信用）
    pub app_handle: RwLock<Option<AppHandle>>,
    /// サムネイル進捗イベントの前回送信時刻
    last_thumb_progress_emit: Mutex<Option<Instant>>,
    /// タイムラインサマリのインメモリキャッシュ ((folder_id, sort, version) -> TimelineSummary)
    timeline_summary_cache: RwLock<HashMap<(Option<i64>, TimelineSort, i64), TimelineSummary>>,
    /// 画像ID -> ThumbSource のインメモリハッシュキャッシュ（SQLiteクエリのバイパス用）
    thumb_lookup_cache: RwLock<HashMap<i64, ThumbSource>>,
    /// サムネイル WebP バイナリの LRU メモリキャッシュ（ディスクI/Oのバイパス用）
    thumb_binary_cache: Mutex<ThumbBinaryLruCache>,
    /// 画像フォーマット拡張プラグインレジストリ
    pub format_plugins: Arc<crate::plugins::FormatPluginRegistry>,
}

impl AppState {
    pub fn new(
        db: Database,
        thumb_store: Arc<ThumbnailStore>,
        data_dir: PathBuf,
        cache_dir: PathBuf,
        thumb_pipeline: Arc<ThumbnailPipeline>,
        format_plugins: Arc<crate::plugins::FormatPluginRegistry>,
    ) -> Self {
        let version = {
            let conn = db.writer();
            crate::db::repo::get_catalog_version(&conn).unwrap_or(1)
        };

        // デスクトップ環境: 2048枚（約60〜80MB）/ モバイル環境: 512枚（約15〜20MB）
        let max_binary_cache_items = if cfg!(target_os = "android") {
            512
        } else {
            2048
        };

        Self {
            db,
            thumb_store,
            data_dir,
            cache_dir,
            thumb_pipeline,
            catalog_version: AtomicI64::new(version),
            is_scan_cancelled: Arc::new(AtomicBool::new(false)),
            thumb_total: AtomicU64::new(0),
            thumb_done: AtomicU64::new(0),
            thumb_failed: AtomicU64::new(0),
            app_handle: RwLock::new(None),
            last_thumb_progress_emit: Mutex::new(None),
            timeline_summary_cache: RwLock::new(HashMap::new()),
            thumb_lookup_cache: RwLock::new(HashMap::new()),
            thumb_binary_cache: Mutex::new(ThumbBinaryLruCache::new(max_binary_cache_items)),
            format_plugins,
        }
    }

    /// インメモリLRUキャッシュから特定のサムネイルを削除する
    ///
    /// 変更理由: ゴミ箱移動やファイル削除時に古いキャッシュが残るのを防止するため。
    pub fn remove_thumb_from_memory(&self, quick_hash: &str) {
        if let Ok(mut cache) = self.thumb_binary_cache.lock() {
            cache.remove(quick_hash);
        }
    }

    /// 指定パスが現在サポートされている画像フォーマット（標準形式または有効化されたプラグイン）か判定する
    pub fn is_supported_image(&self, path: &std::path::Path) -> bool {
        let ext = match path.extension().and_then(|e| e.to_str()) {
            Some(e) => e.to_ascii_lowercase(),
            None => return false,
        };
        if crate::scanner::walker::is_standard_image_extension(&ext) {
            return true;
        }
        self.format_plugins.get_by_extension(&ext).is_some()
    }

    /// イベント送信用 Tauri AppHandle をセットする
    pub fn set_app_handle(&self, handle: AppHandle) {
        if let Ok(mut h) = self.app_handle.write() {
            *h = Some(handle);
        }
    }

    /// サムネイル進捗カウンタをDBの現在値で初期化する
    pub fn init_thumb_progress(&self) {
        if let Ok(conn) = self.db.reader() {
            if let Ok((done, failed, total)) = crate::db::repo::get_thumb_progress(&conn) {
                self.thumb_done.store(done, Ordering::SeqCst);
                self.thumb_failed.store(failed, Ordering::SeqCst);
                self.thumb_total.store(total, Ordering::SeqCst);
            }
        }
    }

    /// サムネイル進捗イベントをスロットル（200ms間隔）または全件完了時に送信する
    pub fn emit_thumb_progress_throttled(&self) {
        let raw_done = self.thumb_done.load(Ordering::Relaxed);
        let raw_failed = self.thumb_failed.load(Ordering::Relaxed);
        let total = self.thumb_total.load(Ordering::Relaxed);

        let done = raw_done.min(total);
        let failed = raw_failed.min(total.saturating_sub(done));

        let is_complete = total > 0 && (done + failed >= total);

        let mut last_emit = match self.last_thumb_progress_emit.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };

        let now = Instant::now();
        let should_emit = if let Some(last) = *last_emit {
            is_complete || now.duration_since(last) >= std::time::Duration::from_millis(200)
        } else {
            true
        };

        if should_emit {
            *last_emit = Some(now);
            drop(last_emit);

            if let Ok(app_guard) = self.app_handle.read() {
                if let Some(app) = app_guard.as_ref() {
                    let _ = app.emit(
                        "thumb-progress",
                        ThumbProgress {
                            done,
                            failed,
                            total,
                        },
                    );
                }
            }
        }
    }

    /// カタログバージョンをインクリメントし最新値を返す
    pub fn bump_catalog_version(&self) -> i64 {
        let conn = self.db.writer();
        let new_ver = crate::db::repo::increment_catalog_version(&conn).unwrap_or_else(|_| {
            self.catalog_version.load(Ordering::SeqCst) + 1
        });
        self.catalog_version.store(new_ver, Ordering::SeqCst);

        // カタログ変更時はメタデータキャッシュおよびタイムラインサマリキャッシュをクリア
        if let Ok(mut cache) = self.thumb_lookup_cache.write() {
            cache.clear();
        }
        if let Ok(mut summary_cache) = self.timeline_summary_cache.write() {
            summary_cache.clear();
        }
        new_ver
    }

    pub fn current_catalog_version(&self) -> i64 {
        self.catalog_version.load(Ordering::SeqCst)
    }

    /// キャッシュされたタイムラインサマリを取得
    pub fn get_cached_timeline_summary(
        &self,
        folder_id: Option<i64>,
        sort: TimelineSort,
        version: i64,
    ) -> Option<TimelineSummary> {
        let cache = self.timeline_summary_cache.read().ok()?;
        cache.get(&(folder_id, sort, version)).cloned()
    }

    /// タイムラインサマリをキャッシュに保存
    pub fn put_cached_timeline_summary(
        &self,
        folder_id: Option<i64>,
        sort: TimelineSort,
        version: i64,
        summary: TimelineSummary,
    ) {
        if let Ok(mut cache) = self.timeline_summary_cache.write() {
            cache.insert((folder_id, sort, version), summary);
        }
    }

    /// 画像IDからサムネイル生成情報（ThumbSource）を取得する
    pub fn get_thumb_source_cached(&self, id: i64) -> Option<ThumbSource> {
        // 1. まず読み取りロックでメモリキャッシュを検索
        if let Ok(cache) = self.thumb_lookup_cache.read() {
            if let Some(src) = cache.get(&id) {
                return Some(src.clone());
            }
        }

        // 2. キャッシュミス時は SQLite から取得
        let src = match self.db.reader() {
            Ok(conn) => crate::db::repo::get_thumb_source(&conn, id).unwrap_or(None),
            Err(_) => None,
        }?;

        // 3. 書き込みロックでキャッシュに追加（上限超過時はクリアしてメモリ無制限膨張を防止）
        if let Ok(mut cache) = self.thumb_lookup_cache.write() {
            if cache.len() >= MAX_LOOKUP_CACHE_ITEMS {
                cache.clear();
            }
            cache.insert(id, src.clone());
        }

        Some(src)
    }

    /// 実アクセス時にファイル更新が検知された際、インメモリメタデータキャッシュを最新情報で上書きする
    pub fn update_cached_thumb_source(&self, src: ThumbSource) {
        if let Ok(mut cache) = self.thumb_lookup_cache.write() {
            if cache.len() >= MAX_LOOKUP_CACHE_ITEMS && !cache.contains_key(&src.id) {
                cache.clear();
            }
            cache.insert(src.id, src);
        }
    }

    /// 特定ハッシュのサムネイルバイナリキャッシュを無効化（パージ）する
    pub fn invalidate_thumbnail_cache(&self, hash: &str) {
        if let Ok(mut cache) = self.thumb_binary_cache.lock() {
            cache.remove(hash);
        }
    }

    /// サムネイル WebP バイナリをインメモリキャッシュから取得
    pub fn get_thumbnail_bytes(&self, hash: &str) -> Option<Arc<Vec<u8>>> {
        self.thumb_binary_cache.lock().ok()?.get(hash)
    }

    /// サムネイル WebP バイナリをインメモリキャッシュに保存
    pub fn put_thumbnail_bytes(&self, hash: String, bytes: Arc<Vec<u8>>) {
        if let Ok(mut cache) = self.thumb_binary_cache.lock() {
            cache.put(hash, bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thumb_binary_lru_cache_o1_eviction_and_order() {
        let mut cache = ThumbBinaryLruCache::new(3);
        let d1 = Arc::new(vec![1u8]);
        let d2 = Arc::new(vec![2u8]);
        let d3 = Arc::new(vec![3u8]);
        let d4 = Arc::new(vec![4u8]);

        cache.put("h1".to_string(), Arc::clone(&d1));
        cache.put("h2".to_string(), Arc::clone(&d2));
        cache.put("h3".to_string(), Arc::clone(&d3));

        // h1 にアクセスして最新化 -> 最古は h2 になるはず
        assert_eq!(cache.get("h1"), Some(Arc::clone(&d1)));

        // h4 を追加すると上限3のため最古の h2 が削除される
        cache.put("h4".to_string(), Arc::clone(&d4));
        assert!(cache.get("h2").is_none());
        assert_eq!(cache.get("h1"), Some(Arc::clone(&d1)));
        assert_eq!(cache.get("h3"), Some(Arc::clone(&d3)));
        assert_eq!(cache.get("h4"), Some(Arc::clone(&d4)));

        // remove の検証
        cache.remove("h3");
        assert!(cache.get("h3").is_none());
    }
}
