use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering as AtomicOrdering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;
use tracing::{info, warn};

use crate::db::repo::ThumbSource;
use crate::db::{Database, ThumbnailStore};
use crate::pipeline::thumbnail::{generate_thumbnail_bytes, get_thumbnail_path};

/// サムネイル生成ジョブの優先度
///
/// - `High`: 現在画面に表示されているセル
/// - `Mid` : 画面の周辺（先読み範囲）
/// - `Low` : バックグラウンドでの全件補充生成
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum JobPriority {
    Low = 0,
    Mid = 1,
    High = 2,
}

/// バックグラウンド補充で一度にDBから読み込む件数
const REFILL_BATCH: usize = 400;
/// これより古い世代の表示要求ジョブは「画面外に流れた」とみなし破棄する
const STALE_GENERATION_GAP: u64 = 2;

pub struct ThumbnailJob {
    pub priority: JobPriority,
    pub image_id: i64,
    pub file_path: PathBuf,
    pub quick_hash: String,
    pub orientation: u32,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// 表示範囲の世代番号（大きいほど新しい要求）
    pub generation: u64,
    /// 投入順序
    pub sequence: u64,
}

impl PartialEq for ThumbnailJob {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for ThumbnailJob {}

impl PartialOrd for ThumbnailJob {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ThumbnailJob {
    /// 取り出し順序:
    /// 1. 優先度が高いもの
    /// 2. 世代が新しいもの（＝直近にスクロールして表示された範囲）
    /// 3. High/Mid は後入れ先出し（最新の要求ほど今見えている可能性が高い）、Low は先入れ先出し
    fn cmp(&self, other: &Self) -> Ordering {
        self.priority
            .cmp(&other.priority)
            .then_with(|| self.generation.cmp(&other.generation))
            .then_with(|| {
                if self.priority == JobPriority::Low {
                    other.sequence.cmp(&self.sequence)
                } else {
                    self.sequence.cmp(&other.sequence)
                }
            })
    }
}

pub enum Waiter {
    Async(tokio::sync::oneshot::Sender<bool>),
    Sync(std::sync::mpsc::SyncSender<bool>),
}

impl Waiter {
    fn send(self, val: bool) {
        match self {
            Waiter::Async(tx) => {
                let _ = tx.send(val);
            }
            Waiter::Sync(tx) => {
                let _ = tx.try_send(val);
            }
        }
    }
}

struct QueueState {
    heap: BinaryHeap<ThumbnailJob>,
    /// キュー内に存在するジョブの「最良の (優先度, 世代)」。重複投入の抑制に使用
    queued: HashMap<i64, (JobPriority, u64)>,
    /// 現在生成処理中の画像ID
    in_progress: HashSet<i64>,
    sequence_counter: u64,
    waiters: HashMap<i64, Vec<Waiter>>,
    /// バックグラウンド補充のDBカーソル（このIDより大きいものを次に読む）
    refill_cursor: i64,
}

/// サムネイル生成ワーカースレッド群および優先度付きキュー
///
/// 変更理由: 仕様書§7「優先度付きキュー + ワーカーstd::thread×N + メモリ予算」に準拠しつつ、
/// Picasa並みの体感速度のため以下を追加:
/// - 表示範囲の世代管理により「今見えている範囲」を常に最優先（LIFO）で処理
/// - スクロールで画面外に流れた古い要求は破棄し、無駄な生成でワーカーを塞がない
/// - キューが空いたらDBから未生成分を自動補充し、全件を事前生成しておく
/// - バックグラウンド生成の同時実行数を制限し、表示要求用のワーカーを常に確保
pub struct ThumbnailPipeline {
    queue_state: Mutex<QueueState>,
    condvar: Condvar,
    running: AtomicBool,
    pub thumb_store: Arc<ThumbnailStore>,
    cache_dir: PathBuf,
    db: Database,
    memory_budget_bytes: AtomicUsize,
    max_memory_budget: usize,
    generation: AtomicU64,
    active_low: AtomicUsize,
    max_low_concurrency: usize,
    completed_count: AtomicU64,
}

impl ThumbnailPipeline {
    pub fn new(
        db: Database,
        thumb_store: Arc<ThumbnailStore>,
        cache_dir: PathBuf,
    ) -> Arc<Self> {
        let max_budget = 384 * 1024 * 1024; // 384MB 予算

        // コア数に基づくワーカー数: コア数-1（最低2、最大8）
        let num_cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        let worker_count = num_cpus.saturating_sub(1).clamp(2, 8);
        // バックグラウンド補充はワーカーの半分まで（表示要求用に常に空きを残す）
        let max_low = (worker_count / 2).max(1);

        let pipeline = Arc::new(Self {
            queue_state: Mutex::new(QueueState {
                heap: BinaryHeap::new(),
                queued: HashMap::new(),
                in_progress: HashSet::new(),
                sequence_counter: 0,
                waiters: HashMap::new(),
                refill_cursor: 0,
            }),
            condvar: Condvar::new(),
            running: AtomicBool::new(true),
            thumb_store,
            cache_dir,
            db,
            memory_budget_bytes: AtomicUsize::new(0),
            max_memory_budget: max_budget,
            generation: AtomicU64::new(1),
            active_low: AtomicUsize::new(0),
            max_low_concurrency: max_low,
            completed_count: AtomicU64::new(0),
        });

        info!(
            "サムネイル生成ワーカースレッド起動: {} スレッド（バックグラウンド上限 {}）",
            worker_count, max_low
        );
        for i in 0..worker_count {
            let p = Arc::clone(&pipeline);
            thread::Builder::new()
                .name(format!("thumb-worker-{}", i))
                .spawn(move || p.worker_loop(i))
                .expect("サムネイルワーカースレッド起動失敗");
        }

        pipeline
    }

    /// サムネイルキャッシュのディレクトリ
    pub fn cache_dir(&self) -> &PathBuf {
        &self.cache_dir
    }

    /// 現在の表示範囲世代番号
    pub fn current_generation(&self) -> u64 {
        self.generation.load(AtomicOrdering::SeqCst)
    }

    /// これまでに生成完了したサムネイル数（起動後の累計）
    pub fn completed_count(&self) -> u64 {
        self.completed_count.load(AtomicOrdering::Relaxed)
    }

    /// ジョブを内部キューに投入する（ロック取得済み前提）
    ///
    /// 同一IDが既に同等以上の優先度・世代でキューにあれば投入しない。
    fn push_locked(
        &self,
        state: &mut QueueState,
        src: ThumbSourceRef<'_>,
        priority: JobPriority,
        generation: u64,
    ) -> bool {
        if state.in_progress.contains(&src.id) {
            return false;
        }
        if let Some(&(p, g)) = state.queued.get(&src.id) {
            if (p, g) >= (priority, generation) {
                return false;
            }
        }
        state.sequence_counter += 1;
        state.queued.insert(src.id, (priority, generation));
        state.heap.push(ThumbnailJob {
            priority,
            image_id: src.id,
            file_path: PathBuf::from(src.file_path),
            quick_hash: src.quick_hash.to_string(),
            orientation: src.orientation,
            width: src.width,
            height: src.height,
            generation,
            sequence: state.sequence_counter,
        });
        true
    }

    /// サムネイル生成ジョブをキューに追加（後方互換API）
    pub fn enqueue(
        &self,
        image_id: i64,
        file_path: PathBuf,
        quick_hash: String,
        orientation: u32,
        width: Option<u32>,
        height: Option<u32>,
        priority: JobPriority,
    ) {
        if self.thumb_store.exists(&quick_hash).unwrap_or(false)
            || get_thumbnail_path(&self.cache_dir, &quick_hash).exists()
        {
            return;
        }
        let path_str = file_path.to_string_lossy();
        let generation = self.current_generation();
        let mut state = self.queue_state.lock().unwrap();
        let pushed = self.push_locked(
            &mut state,
            ThumbSourceRef {
                id: image_id,
                file_path: &path_str,
                quick_hash: &quick_hash,
                orientation,
                width,
                height,
            },
            priority,
            generation,
        );
        if pushed {
            self.condvar.notify_one();
        }
    }

    /// 単一の ThumbSource を現在世代で投入する
    ///
    /// @param src 画像情報
    /// @param priority 優先度
    pub fn enqueue_source(&self, src: &ThumbSource, priority: JobPriority) {
        let generation = self.current_generation();
        let mut state = self.queue_state.lock().unwrap();
        if self.push_locked(&mut state, ThumbSourceRef::from(src), priority, generation) {
            self.condvar.notify_one();
        }
    }

    /// 表示範囲（ビューポート）を更新し、現画面のサムネイル生成に完全に仕切り直す
    ///
    /// 変更理由: ユーザー要求「激しく動かすのを止めた場合でも復帰が遅い。溜まってしまった場合は状況に関係なく一度クリアして現在位置での読込みから再開する」。
    /// 画面外に流れた過去ジョブを待機者（waiters）ごと即座に破棄・失敗解放し、ブロックされていたHTTPワーカースレッドを一瞬で解放して現画面に専任させる。
    ///
    /// @param visible 現在画面に見えている未生成画像
    /// @param nearby 画面周辺（先読み範囲）の未生成画像
    /// @return 新しい世代番号
    pub fn set_viewport(&self, visible: &[ThumbSource], nearby: &[ThumbSource]) -> u64 {
        let generation = self.generation.fetch_add(1, AtomicOrdering::SeqCst) + 1;
        let mut state = self.queue_state.lock().unwrap();

        // 現画面および周辺の画像IDセット
        let mut valid_ids = HashSet::with_capacity(visible.len() + nearby.len());
        for s in visible {
            valid_ids.insert(s.id);
        }
        for s in nearby {
            valid_ids.insert(s.id);
        }

        // 過去の待機者のうち、現画面・周辺に含まれないものを即座に解放（false送信）してHTTPスレッドを解放
        let mut stale_waiters = Vec::new();
        state.waiters.retain(|img_id, waiters| {
            if !valid_ids.contains(img_id) {
                stale_waiters.append(waiters);
                false
            } else {
                true
            }
        });
        for w in stale_waiters {
            w.send(false);
        }

        // 過去の未処理ジョブを一掃破棄（現画面・周辺に含まれるジョブのみ温存）
        if !state.heap.is_empty() {
            let old_heap = std::mem::take(&mut state.heap);
            state.queued.clear();
            for job in old_heap.into_vec() {
                if valid_ids.contains(&job.image_id) {
                    state.queued.insert(job.image_id, (job.priority, job.generation));
                    state.heap.push(job);
                }
            }
        }

        // 周辺 → 表示 の順で投入（同一世代内のLIFOにより表示範囲が先に処理される）
        for src in nearby {
            self.push_locked(&mut state, ThumbSourceRef::from(src), JobPriority::Mid, generation);
        }
        for src in visible {
            self.push_locked(&mut state, ThumbSourceRef::from(src), JobPriority::High, generation);
        }

        drop(state);
        self.condvar.notify_all();
        generation
    }

    /// キュー内の未処理ジョブおよび待機者を状況に関係なく完全に一掃クリアする
    ///
    /// 変更理由: ユーザー要求「溜まってしまった場合は状況に関係なく一度クリアして現在位置での読込みから再開する」
    pub fn clear_queue(&self) -> u64 {
        let generation = self.generation.fetch_add(1, AtomicOrdering::SeqCst) + 1;
        let mut state = self.queue_state.lock().unwrap();

        // 全待機者に即座に解放（false）を送信してブロック中のスレッドを解放
        for (_, waiters) in state.waiters.drain() {
            for w in waiters {
                w.send(false);
            }
        }

        state.heap.clear();
        state.queued.clear();

        drop(state);
        self.condvar.notify_all();
        generation
    }

    /// パイプラインが現在アイドル（未処理ジョブがなく、処理中ジョブも0）であるか確認
    pub fn is_idle(&self) -> bool {
        let state = self.queue_state.lock().unwrap();
        state.heap.is_empty() && state.in_progress.is_empty()
    }

    /// パイプラインが現在稼働中であるか確認
    pub fn is_running(&self) -> bool {
        self.running.load(AtomicOrdering::SeqCst)
    }

    /// サムネイル生成を最優先で要求し、完了（成功/失敗）まで待機する
    ///
    /// Tauri カスタムプロトコル経由の要求で使用（HTTP経由では接続を塞がないよう使用しない）。
    pub async fn request_and_wait(
        &self,
        image_id: i64,
        file_path: PathBuf,
        quick_hash: String,
        orientation: u32,
        width: Option<u32>,
        height: Option<u32>,
    ) -> bool {
        let dest = get_thumbnail_path(&self.cache_dir, &quick_hash);
        if dest.exists() {
            return true;
        }

        let (tx, rx) = tokio::sync::oneshot::channel();
        {
            let path_str = file_path.to_string_lossy();
            let generation = self.current_generation();
            let mut state = self.queue_state.lock().unwrap();
            state.waiters.entry(image_id).or_default().push(Waiter::Async(tx));
            self.push_locked(
                &mut state,
                ThumbSourceRef {
                    id: image_id,
                    file_path: &path_str,
                    quick_hash: &quick_hash,
                    orientation,
                    width,
                    height,
                },
                JobPriority::High,
                generation,
            );
            self.condvar.notify_one();
        }

        rx.await.unwrap_or(false)
    }

    /// サムネイル生成を最優先で要求し、同期スレッドから指定タイムアウトまで待機する
    ///
    /// 変更理由: HTTPサーバー（/thumbs/:id）の同期スレッドからTokioランタイムを新規生成することなく、
    /// 直接軽量な同期チャンネルで生成完了を待機できるようにするため
    ///
    /// @param src サムネイル元情報
    /// @param timeout 最大待機時間
    /// @return タイムアウト内に生成成功した場合はtrue
    pub fn request_and_wait_sync(&self, src: &ThumbSource, timeout: Duration) -> bool {
        if self.thumb_store.exists(&src.quick_hash).unwrap_or(false)
            || get_thumbnail_path(&self.cache_dir, &src.quick_hash).exists()
        {
            return true;
        }

        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        {
            let generation = self.current_generation();
            let mut state = self.queue_state.lock().unwrap();
            state
                .waiters
                .entry(src.id)
                .or_default()
                .push(Waiter::Sync(tx));
            self.push_locked(
                &mut state,
                ThumbSourceRef::from(src),
                JobPriority::High,
                generation,
            );
            self.condvar.notify_one();
        }

        match rx.recv_timeout(timeout) {
            Ok(success) => success,
            Err(_) => false,
        }
    }

    /// キューが空のとき、DBから未生成のサムネイルを補充する
    /// バックグラウンド補充カーソルを先頭（0）にリセットし、ワーカースレッドを起こして未完了分の処理を開始
    ///
    /// 変更理由: ユーザー要求「サムネイルのない画像を見つけた場合にサムネイル化処理を行うようにして」。
    /// 未完了画像や過去の失敗画像の再作成を即時トリガーするため。
    pub fn trigger_background_refill(&self) {
        let mut state = self.queue_state.lock().unwrap();
        state.refill_cursor = 0;
        drop(state);
        self.condvar.notify_all();
    }

    /// キューが空のとき、DBから未生成のサムネイルを補充する
    ///
    /// 変更理由: 末尾IDまで進んだ後でも、ID=0以降に未生成画像（thumb_status != 1）が存在する場合は
    /// 即座に先頭から補充して滞留を防ぐ。
    ///
    /// @return 補充した件数
    fn refill_background(&self) -> usize {
        let cursor = { self.queue_state.lock().unwrap().refill_cursor };
        let mut batch = match self.db.reader() {
            Ok(conn) => crate::db::repo::get_pending_thumb_sources(&conn, cursor, REFILL_BATCH)
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        };

        let mut state = self.queue_state.lock().unwrap();
        if batch.is_empty() {
            if cursor > 0 {
                // カーソルが末尾付近にある場合は先頭から再探索
                state.refill_cursor = 0;
                drop(state);
                let first_batch = match self.db.reader() {
                    Ok(conn) => crate::db::repo::get_pending_thumb_sources(&conn, 0, REFILL_BATCH)
                        .unwrap_or_default(),
                    Err(_) => Vec::new(),
                };
                if first_batch.is_empty() {
                    return 0;
                }
                batch = first_batch;
                state = self.queue_state.lock().unwrap();
            } else {
                return 0;
            }
        }

        state.refill_cursor = batch.last().map(|s| s.id).unwrap_or(cursor);
        let generation = self.current_generation();
        let mut n = 0;
        for src in &batch {
            if self.push_locked(&mut state, ThumbSourceRef::from(src), JobPriority::Low, generation) {
                n += 1;
            }
        }
        n
    }

    /// 次に処理すべきジョブを取り出す（古い世代・重複・処理中のものは読み飛ばす）
    fn next_job(&self) -> Option<ThumbnailJob> {
        let mut idle_rounds = 0u32;
        loop {
            if !self.running.load(AtomicOrdering::SeqCst) {
                return None;
            }
            let mut state = self.queue_state.lock().unwrap();
            let current_gen = self.current_generation();

            while let Some(top) = state.heap.peek() {
                // バックグラウンドジョブは同時実行数を制限
                if top.priority == JobPriority::Low
                    && self.active_low.load(AtomicOrdering::SeqCst) >= self.max_low_concurrency
                {
                    break;
                }
                let job = state.heap.pop().unwrap();

                // キュー上の最良エントリでなければ（より高優先度で再投入済み）読み飛ばす
                let is_best = state
                    .queued
                    .get(&job.image_id)
                    .map(|&(p, g)| p == job.priority && g == job.generation)
                    .unwrap_or(false);
                if !is_best || state.in_progress.contains(&job.image_id) {
                    continue;
                }
                state.queued.remove(&job.image_id);

                // 画面外に流れた古い表示要求は破棄（待機者がいれば処理する）
                let stale = job.priority != JobPriority::Low
                    && job.generation + STALE_GENERATION_GAP < current_gen
                    && !state.waiters.contains_key(&job.image_id);
                if stale {
                    continue;
                }

                state.in_progress.insert(job.image_id);
                if job.priority == JobPriority::Low {
                    self.active_low.fetch_add(1, AtomicOrdering::SeqCst);
                }
                return Some(job);
            }

            let heap_empty = state.heap.is_empty();
            if heap_empty && idle_rounds == 0 {
                drop(state);
                idle_rounds += 1;
                if self.refill_background() > 0 {
                    continue;
                }
                state = self.queue_state.lock().unwrap();
            }

            // 新しいジョブの到着、またはバックグラウンド枠の空きを待つ
            let timeout = if heap_empty {
                Duration::from_secs(5)
            } else {
                Duration::from_millis(50)
            };
            let (guard, res) = self.condvar.wait_timeout(state, timeout).unwrap();
            drop(guard);
            if res.timed_out() && heap_empty {
                // 一定時間アイドルなら再度補充を試みる
                idle_rounds = 0;
            }
        }
    }

    /// ワーカースレッドのメインループ
    fn worker_loop(&self, worker_id: usize) {
        while let Some(job) = self.next_job() {
            let is_already_done = self.thumb_store.exists(&job.quick_hash).unwrap_or(false)
                || get_thumbnail_path(&self.cache_dir, &job.quick_hash).exists();
            let is_success = if is_already_done {
                // 同一ハッシュの別画像で生成済み、または他経路で生成済み
                let conn = self.db.writer();
                crate::db::repo::update_thumb_status(&conn, job.image_id, 1).ok();
                true
            } else {
                self.process_job(&job)
            };

            if job.priority == JobPriority::Low {
                self.active_low.fetch_sub(1, AtomicOrdering::SeqCst);
            }
            self.completed_count.fetch_add(1, AtomicOrdering::Relaxed);

            let mut state = self.queue_state.lock().unwrap();
            state.in_progress.remove(&job.image_id);
            if let Some(waiters) = state.waiters.remove(&job.image_id) {
                for waiter in waiters {
                    waiter.send(is_success);
                }
            }
            drop(state);
            // バックグラウンド枠が空いた可能性があるため他ワーカーを起こす
            self.condvar.notify_one();
        }
        info!("サムネイルワーカー {} を停止しました", worker_id);
    }

    /// 1件のサムネイルを生成しDBのステータスを更新する
    fn process_job(&self, job: &ThumbnailJob) -> bool {
        // メモリ予算の見積もり: 幅 × 高さ × 4バイト (RGBA)。
        // JPEGは縮小デコードされるため実際はこれより大幅に小さい。
        let est_bytes = job
            .width
            .unwrap_or(2000)
            .saturating_mul(job.height.unwrap_or(2000))
            .saturating_mul(4) as usize;
        let est_bytes = est_bytes.min(self.max_memory_budget);

        while self.running.load(AtomicOrdering::SeqCst) {
            let cur = self.memory_budget_bytes.load(AtomicOrdering::SeqCst);
            if cur > 0 && cur + est_bytes > self.max_memory_budget {
                thread::sleep(Duration::from_millis(10));
                continue;
            }
            self.memory_budget_bytes.fetch_add(est_bytes, AtomicOrdering::SeqCst);
            break;
        }

        let result = generate_thumbnail_bytes(
            &job.file_path,
            job.orientation,
        );

        self.memory_budget_bytes.fetch_sub(est_bytes, AtomicOrdering::SeqCst);

        let conn = self.db.writer();
        match result {
            Ok(webp_bytes) => {
                // サムネイル専用DB（thumbnails.db）へBLOB格納
                if let Err(e) = self.thumb_store.put(&job.quick_hash, &webp_bytes) {
                    warn!("サムネイルBLOB保存失敗 (hash: {}): {}", job.quick_hash, e);
                }
                crate::db::repo::update_thumb_status(&conn, job.image_id, 1).ok();
                true
            }
            Err(e) => {
                warn!("サムネイル生成失敗 (id: {}): {}", job.image_id, e);
                crate::db::repo::update_thumb_status(&conn, job.image_id, 2).ok();
                false
            }
        }
    }

    pub fn shutdown(&self) {
        self.running.store(false, AtomicOrdering::SeqCst);
        self.condvar.notify_all();
    }
}

/// ジョブ投入用の借用参照（不要なクローンを避ける）
#[derive(Clone, Copy)]
struct ThumbSourceRef<'a> {
    id: i64,
    file_path: &'a str,
    quick_hash: &'a str,
    orientation: u32,
    width: Option<u32>,
    height: Option<u32>,
}

impl<'a> From<&'a ThumbSource> for ThumbSourceRef<'a> {
    fn from(s: &'a ThumbSource) -> Self {
        Self {
            id: s.id,
            file_path: &s.file_path,
            quick_hash: &s.quick_hash,
            orientation: s.orientation,
            width: s.width,
            height: s.height,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(priority: JobPriority, generation: u64, sequence: u64) -> ThumbnailJob {
        ThumbnailJob {
            priority,
            image_id: sequence as i64,
            file_path: PathBuf::new(),
            quick_hash: String::new(),
            orientation: 1,
            width: None,
            height: None,
            generation,
            sequence,
        }
    }

    #[test]
    fn test_job_ordering_prefers_priority_then_newer_generation_then_lifo() {
        let mut heap = BinaryHeap::new();
        heap.push(job(JobPriority::Low, 9, 1));
        heap.push(job(JobPriority::High, 1, 2));
        heap.push(job(JobPriority::High, 2, 3));
        heap.push(job(JobPriority::High, 2, 4));
        heap.push(job(JobPriority::Mid, 9, 5));

        // 優先度High・世代2の中では後から投入された(seq 4)が先
        assert_eq!(heap.pop().unwrap().sequence, 4);
        assert_eq!(heap.pop().unwrap().sequence, 3);
        assert_eq!(heap.pop().unwrap().sequence, 2);
        assert_eq!(heap.pop().unwrap().sequence, 5);
        assert_eq!(heap.pop().unwrap().sequence, 1);
    }

    #[test]
    fn test_low_priority_is_fifo() {
        let mut heap = BinaryHeap::new();
        heap.push(job(JobPriority::Low, 1, 10));
        heap.push(job(JobPriority::Low, 1, 11));
        assert_eq!(heap.pop().unwrap().sequence, 10);
    }
}
