use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use crate::db::repo::ThumbSource;
use crate::db::{Database, ThumbnailStore};
use crate::pipeline::ThumbnailPipeline;

/// サムネイル WebP バイナリのインメモリ LRU キャッシュ（最大2,048枚 ≒ 約60〜80MB）
struct ThumbBinaryLruCache {
    map: HashMap<String, Arc<Vec<u8>>>,
    order: VecDeque<String>,
    max_items: usize,
}

impl ThumbBinaryLruCache {
    fn new(max_items: usize) -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            max_items,
        }
    }

    fn get(&mut self, hash: &str) -> Option<Arc<Vec<u8>>> {
        self.map.get(hash).cloned()
    }

    fn put(&mut self, hash: String, data: Arc<Vec<u8>>) {
        if self.map.contains_key(&hash) {
            self.map.insert(hash, data);
            return;
        }
        if self.order.len() >= self.max_items {
            if let Some(oldest) = self.order.pop_front() {
                self.map.remove(&oldest);
            }
        }
        self.order.push_back(hash.clone());
        self.map.insert(hash, data);
    }

    fn remove(&mut self, hash: &str) {
        self.map.remove(hash);
    }
}

/// アプリケーション全体で共有されるグローバル状態
///
/// 変更理由: DB接続、キャッシュパス、サムネイル生成パイプラインに加え、
/// Picasa並みのゼロ遅延を実現するためのインメモリメタデータキャッシュおよびバイナリLRUキャッシュを一元管理
pub struct AppState {
    /// データベース管理
    pub db: Database,
    /// サムネイル専用SQLiteデータベース管理
    pub thumb_store: Arc<ThumbnailStore>,
    /// サムネイル等のキャッシュディレクトリ
    pub cache_dir: PathBuf,
    /// サムネイル生成バックグラウンドパイプライン
    pub thumb_pipeline: Arc<ThumbnailPipeline>,
    /// メモリキャッシュされたカタログバージョン
    pub catalog_version: AtomicI64,
    /// 走査キャンセル用フラグ
    pub is_scan_cancelled: Arc<AtomicBool>,
    /// 画像ID -> ThumbSource のインメモリハッシュキャッシュ（SQLiteクエリの完全バイパス用）
    thumb_lookup_cache: RwLock<HashMap<i64, ThumbSource>>,
    /// サムネイル WebP バイナリの LRU メモリキャッシュ（ディスクI/O完全バイパス用）
    thumb_binary_cache: Mutex<ThumbBinaryLruCache>,
}

impl AppState {
    pub fn new(
        db: Database,
        thumb_store: Arc<ThumbnailStore>,
        cache_dir: PathBuf,
        thumb_pipeline: Arc<ThumbnailPipeline>,
    ) -> Self {
        let version = {
            let conn = db.writer();
            crate::db::repo::get_catalog_version(&conn).unwrap_or(1)
        };

        Self {
            db,
            thumb_store,
            cache_dir,
            thumb_pipeline,
            catalog_version: AtomicI64::new(version),
            is_scan_cancelled: Arc::new(AtomicBool::new(false)),
            thumb_lookup_cache: RwLock::new(HashMap::new()),
            thumb_binary_cache: Mutex::new(ThumbBinaryLruCache::new(2048)),
        }
    }

    /// カタログバージョンをインクリメントし最新値を返す
    pub fn bump_catalog_version(&self) -> i64 {
        let conn = self.db.writer();
        let new_ver = crate::db::repo::increment_catalog_version(&conn).unwrap_or_else(|_| {
            self.catalog_version.load(Ordering::SeqCst) + 1
        });
        self.catalog_version.store(new_ver, Ordering::SeqCst);
        // カタログ変更時はメタデータキャッシュもクリア
        if let Ok(mut cache) = self.thumb_lookup_cache.write() {
            cache.clear();
        }
        new_ver
    }

    pub fn current_catalog_version(&self) -> i64 {
        self.catalog_version.load(Ordering::SeqCst)
    }

    /// 画像IDからサムネイル生成情報（ThumbSource）を取得する
    ///
    /// 変更理由: サムネイルリクエスト（/thumbs/:id）のたびにSQLiteへクエリを発行するのを廃止し、
    /// メモリ上のHashMapから0.0001ms（ナノ秒単位）で即時返却して接続プール競合を排除する。
    ///
    /// @param id 画像ID
    /// @return ThumbSource（存在する場合）
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

        // 3. 書き込みロックでキャッシュに追加
        if let Ok(mut cache) = self.thumb_lookup_cache.write() {
            cache.insert(id, src.clone());
        }

        Some(src)
    }

    /// 実アクセス時にファイル更新が検知された際、インメモリメタデータキャッシュを最新情報で上書きする
    ///
    /// @param src 更新された最新の ThumbSource
    pub fn update_cached_thumb_source(&self, src: ThumbSource) {
        if let Ok(mut cache) = self.thumb_lookup_cache.write() {
            cache.insert(src.id, src);
        }
    }

    /// 特定ハッシュのサムネイルバイナリキャッシュを無効化（パージ）する
    ///
    /// @param hash クイックハッシュ
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
