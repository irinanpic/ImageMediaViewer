use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

pub use crate::models::LogEntry;

/// ログ管理用共有状態
#[derive(Clone)]
pub struct LogManager {
    file_writer: Arc<Mutex<Option<File>>>,
    recent_logs: Arc<Mutex<VecDeque<LogEntry>>>,
    max_recent_logs: usize,
    log_dir: PathBuf,
}

static GLOBAL_LOG_MANAGER: Mutex<Option<LogManager>> = Mutex::new(None);

impl LogManager {
    /// ログマネージャを初期化
    ///
    /// @param app_data_dir アプリケーションデータディレクトリ
    /// @param max_recent_logs メモリに保持する最大ログ行数
    pub fn new(app_data_dir: &Path, max_recent_logs: usize) -> Self {
        let log_dir = app_data_dir.join("logs");
        fs::create_dir_all(&log_dir).ok();

        let log_file_path = log_dir.join("app.log");
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_file_path)
            .ok();

        Self {
            file_writer: Arc::new(Mutex::new(file)),
            recent_logs: Arc::new(Mutex::new(VecDeque::with_capacity(max_recent_logs))),
            max_recent_logs,
            log_dir,
        }
    }

    /// ログエントリを記録（ファイルおよびインメモリバッファ）
    pub fn write_entry(&self, entry: LogEntry) {
        // 1. メモリバッファへ追加
        if let Ok(mut logs) = self.recent_logs.lock() {
            if logs.len() >= self.max_recent_logs {
                logs.pop_front();
            }
            logs.push_back(entry.clone());
        }

        // 2. ログファイルへ書き出し
        if let Ok(mut writer_guard) = self.file_writer.lock() {
            if let Some(file) = writer_guard.as_mut() {
                let line = format!(
                    "{} [{}] {} - {}\n",
                    entry.timestamp, entry.level, entry.target, entry.message
                );
                let _ = file.write_all(line.as_bytes());
                let _ = file.flush();
            }
        }
    }

    /// 直近のログ一覧を取得
    pub fn get_recent_logs(&self, limit: usize) -> Vec<LogEntry> {
        if let Ok(logs) = self.recent_logs.lock() {
            let start = logs.len().saturating_sub(limit);
            logs.iter().skip(start).cloned().collect()
        } else {
            Vec::new()
        }
    }

    /// ログディレクトリのパスを取得
    pub fn log_dir(&self) -> PathBuf {
        self.log_dir.clone()
    }
}

/// tracing 用のカスタム Layer 実装
pub struct MemoryAndFileLoggerLayer {
    manager: LogManager,
}

impl MemoryAndFileLoggerLayer {
    pub fn new(manager: LogManager) -> Self {
        Self { manager }
    }
}

struct MessageVisitor(String);
impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{:?}", value);
            // 余分な前後の引用符を除去
            if self.0.starts_with('"') && self.0.ends_with('"') && self.0.len() >= 2 {
                self.0 = self.0[1..self.0.len() - 1].to_string();
            }
        } else if self.0.is_empty() {
            self.0 = format!("{}={:?}", field.name(), value);
        } else {
            self.0.push_str(&format!(" {}={:?}", field.name(), value));
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.0 = value.to_string();
        } else if self.0.is_empty() {
            self.0 = format!("{}={}", field.name(), value);
        } else {
            self.0.push_str(&format!(" {}={}", field.name(), value));
        }
    }
}

impl<S: Subscriber> Layer<S> for MemoryAndFileLoggerLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = MessageVisitor(String::new());
        event.record(&mut visitor);

        let metadata = event.metadata();
        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string();
        let level = match *metadata.level() {
            Level::ERROR => "ERROR",
            Level::WARN => "WARN",
            Level::INFO => "INFO",
            Level::DEBUG => "DEBUG",
            Level::TRACE => "TRACE",
        }
        .to_string();

        let entry = LogEntry {
            timestamp,
            level,
            target: metadata.target().to_string(),
            message: visitor.0,
        };

        self.manager.write_entry(entry);
    }
}

/// グローバルロガーを初期化
pub fn init_logger(app_data_dir: &Path) -> Result<LogManager, Box<dyn std::error::Error>> {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let manager = LogManager::new(app_data_dir, 500);
    let manager_clone = manager.clone();

    // 標準出力フォーマッタ層
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_target(true);

    // カスタムファイル＆メモリ記録層
    let custom_layer = MemoryAndFileLoggerLayer::new(manager.clone());

    // フィルタ設定
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info,image_media_viewer=debug".into());

    let subscriber = tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .with(custom_layer);

    // すでに subscriber が設定されている場合はエラーを無視
    let _ = subscriber.try_init();

    let mut global = GLOBAL_LOG_MANAGER.lock().unwrap();
    *global = Some(manager_clone);

    Ok(manager)
}

/// グローバルなログマネージャを取得
pub fn get_log_manager() -> Option<LogManager> {
    GLOBAL_LOG_MANAGER.lock().unwrap().clone()
}
