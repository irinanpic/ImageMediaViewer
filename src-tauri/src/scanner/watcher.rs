//! ファイルシステム監視およびアイドル時更新モジュール
//!
//! 変更理由: ユーザー要求「処理がないときにフォルダ内の更新もチェックするようにしてください。優先度は低く、作業中には実施しなくても構いません」
//! サムネイル生成キューが完全に空で、作業が行われていないアイドル状態のときに、登録フォルダの更新チェックを自動実行する。

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tracing::debug;

use crate::state::AppState;

/// アイドル時のフォルダ更新チェックループを開始
pub fn start_idle_watcher(state: Arc<AppState>) {
    thread::Builder::new()
        .name("idle-folder-watcher".to_string())
        .spawn(move || {
            let mut last_check = Instant::now();
            let check_interval = Duration::from_secs(45); // 45秒間隔

            loop {
                thread::sleep(Duration::from_secs(5));

                // 終了確認
                if !state.thumb_pipeline.is_running() {
                    break;
                }

                // 前回のチェックから指定時間が経過しているか確認
                if last_check.elapsed() < check_interval {
                    continue;
                }

                // サムネイル生成パイプラインがアイドル（作業中ではない）か確認
                if !state.thumb_pipeline.is_idle() {
                    continue;
                }

                // 登録フォルダ一覧を取得
                let folders = match state.db.reader() {
                    Ok(conn) => crate::db::repo::get_watched_folders(&conn).unwrap_or_default(),
                    Err(_) => continue,
                };

                if folders.is_empty() {
                    last_check = Instant::now();
                    continue;
                }

                debug!("アイドル状態を検知: 登録フォルダの更新チェックを開始します");

                let mut any_interrupted = false;
                for folder in folders {
                    // スキャン中またはユーザー操作でキューにジョブが入ったら即座に中断
                    if !state.thumb_pipeline.is_idle() {
                        debug!("ユーザー操作・ジョブ発生を検知: アイドルチェックを中断します");
                        any_interrupted = true;
                        break;
                    }

                    // 差分スキャンの実行（ヘッドレス）
                    let _ = crate::scanner::scan_folder_core(None, Arc::clone(&state), folder.id, folder.path);
                }

                last_check = Instant::now();
                if !any_interrupted {
                    debug!("アイドル時フォルダ更新チェック完了");
                    // フォルダチェック完了後、引き続きアイドルであれば孤立サムネイルGCを実行
                    if state.thumb_pipeline.is_idle() {
                        run_thumbnail_gc(&state);
                    }
                }
            }
        })
        .expect("アイドルウォッカースレッド起動失敗");
}

/// アイドル時に参照ゼロの孤立サムネイルを回収する（Garbage Collection）
///
/// 変更理由: 仕様書§2.2 F-01「孤立したサムネイルはGCする」および整合性設計に基づき、
/// 画像ファイルの更新や削除によりどの画像からも参照されなくなった旧サムネイルBLOBを
/// バックグラウンドで安全に特定・削除してストレージ肥大化を防止する。
///
/// @param state アプリケーション状態
fn run_thumbnail_gc(state: &Arc<AppState>) {
    let mut offset = 0;
    let batch_size = 200;

    loop {
        // パイプラインが作業を始めたら即座に中断
        if !state.thumb_pipeline.is_idle() || !state.thumb_pipeline.is_running() {
            break;
        }

        let hashes = match state.thumb_store.list_hashes(offset, batch_size) {
            Ok(h) if !h.is_empty() => h,
            _ => break,
        };

        let hashes_len = hashes.len();

        let unreferenced = match state.db.reader() {
            Ok(conn) => crate::db::repo::filter_unreferenced_hashes(&conn, &hashes).unwrap_or_default(),
            Err(_) => break,
        };

        if !unreferenced.is_empty() {
            if let Ok(deleted) = state.thumb_store.delete_batch(&unreferenced) {
                if deleted > 0 {
                    tracing::info!("孤立サムネイル GC: {} 件の不要なBLOBを回収しました", deleted);
                }
            }
        }

        // 削除されなかった分だけオフセットを進める
        let retained = hashes_len.saturating_sub(unreferenced.len());
        offset += retained;

        if hashes_len < batch_size {
            break;
        }
    }
}
