pub mod migrations;
pub mod repo;
pub mod thumb_store;

pub use thumb_store::ThumbnailStore;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use rusqlite::{Connection, OpenFlags, Result};
use tracing::info;

/// 接続ごとに必須となるPRAGMA設定を適用する
///
/// 変更理由: 仕様書§4「以下のPRAGMAは接続ごとに毎回設定する（特にforeign_keys）」の要件を徹底するため
///
/// @param conn SQLite接続
/// @return 成功時はOk(())
pub fn configure_connection(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA busy_timeout = 5000;
         PRAGMA cache_size = -16384;",
    )?;
    Ok(())
}

/// 読み取り接続プールの最大保持数
const READER_POOL_MAX: usize = 16;

/// データベース接続管理マネージャ
///
/// 変更理由: 仕様書§4「書き込み接続1本（Mutex）＋読み取り接続」の接続戦略を満たし、
/// WALモードによる高パフォーマンスな並行読み書きを実現するため。
/// 読み取り接続はプールで再利用し、リクエスト毎のオープン/PRAGMA実行コストを排除する。
#[derive(Clone)]
pub struct Database {
    db_path: PathBuf,
    writer: Arc<Mutex<Connection>>,
    reader_pool: Arc<Mutex<Vec<Connection>>>,
}

/// プールから貸し出された読み取り接続。Drop時に自動でプールへ返却される。
///
/// `Deref<Target = Connection>` を実装しているため、従来どおり `&conn` で
/// `&Connection` を要求する関数へそのまま渡せる（後方互換）。
pub struct PooledReader {
    conn: Option<Connection>,
    pool: Arc<Mutex<Vec<Connection>>>,
}

impl std::ops::Deref for PooledReader {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        self.conn.as_ref().expect("PooledReader: 接続は返却済み")
    }
}

impl Drop for PooledReader {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            if let Ok(mut pool) = self.pool.lock() {
                if pool.len() < READER_POOL_MAX {
                    pool.push(conn);
                }
            }
        }
    }
}

impl Database {
    /// 指定されたパスでデータベースを初期化しマイグレーションを適用する
    ///
    /// @param path データベースファイルのパス
    /// @return 成功時はDatabaseインスタンス
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let db_path = path.as_ref().to_path_buf();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }

        let mut write_conn = Connection::open(&db_path)?;
        configure_connection(&write_conn)?;
        migrations::apply_migrations(&mut write_conn)?;

        info!("データベースを正常に初期化しました: {:?}", db_path);

        Ok(Self {
            db_path,
            writer: Arc::new(Mutex::new(write_conn)),
            reader_pool: Arc::new(Mutex::new(Vec::new())),
        })
    }

    /// インメモリデータベース（テスト用）を初期化
    ///
    /// @return 成功時はテスト用Databaseインスタンス
    pub fn open_in_memory() -> Result<Self> {
        let mut write_conn = Connection::open_in_memory()?;
        configure_connection(&write_conn)?;
        migrations::apply_migrations(&mut write_conn)?;

        Ok(Self {
            db_path: PathBuf::from(":memory:"),
            writer: Arc::new(Mutex::new(write_conn)),
            reader_pool: Arc::new(Mutex::new(Vec::new())),
        })
    }

    /// 排他書き込み接続を取得する
    ///
    /// @return 書き込み接続のMutexGuard
    pub fn writer(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.writer.lock().expect("DB writer lock poisoned")
    }

    /// 並行読み取り専用接続をプールから取得する（無ければ新規オープン）
    ///
    /// @return 読み取り用接続（Drop時にプールへ返却）
    /// @throws rusqlite::Error 接続オープンに失敗した場合
    pub fn reader(&self) -> Result<PooledReader> {
        let pooled = self.reader_pool.lock().ok().and_then(|mut p| p.pop());
        let conn = match pooled {
            Some(c) => c,
            None => self.open_reader()?,
        };
        Ok(PooledReader {
            conn: Some(conn),
            pool: Arc::clone(&self.reader_pool),
        })
    }

    /// 読み取り接続を新規にオープンする
    fn open_reader(&self) -> Result<Connection> {
        if self.db_path == Path::new(":memory:") {
            // インメモリモードでは独立DBになるためテスト用途のみ
            Connection::open(&self.db_path)
        } else {
            let conn = Connection::open_with_flags(
                &self.db_path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            // 読み取り専用接続では journal_mode 変更は不要なため最小限のPRAGMAのみ
            conn.execute_batch(
                "PRAGMA busy_timeout = 5000;
                 PRAGMA cache_size = -8192;",
            )?;
            Ok(conn)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_db_init_and_pragmas() {
        let db = Database::open_in_memory().expect("インメモリDBのオープンに失敗");
        let conn = db.writer();

        // foreign_keysが有効か確認
        let fk: i32 = conn.pragma_query_value(None, "foreign_keys", |r| r.get(0)).unwrap();
        assert_eq!(fk, 1);

        // user_versionが5（マイグレーションv5適用済み）になっているか確認
        let uv: i32 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
        assert_eq!(uv, 5);
    }
}
