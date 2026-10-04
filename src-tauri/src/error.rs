use serde::{Deserialize, Serialize};
use thiserror::Error;
use ts_rs::TS;

/// アプリケーション内で発生するエラーコードの定義
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub enum AppErrorCode {
    NotFound,
    Io,
    Db,
    InvalidArgument,
    Cancelled,
    Internal,
}

/// フロントエンドに返却される標準化されたエラー構造体
///
/// 変更理由: 仕様書§9のAppErrorインターフェースに準拠し、エラーコードとメッセージを統一的にハンドリングするため
#[derive(Debug, Clone, Serialize, Deserialize, Error, TS)]
#[error("{code:?}: {message}")]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct AppError {
    /// エラー分類コード
    pub code: AppErrorCode,
    /// エラー詳細メッセージ
    pub message: String,
}

impl AppError {
    /// 新規AppErrorインスタンスを生成
    ///
    /// @param code エラーコード
    /// @param message エラーメッセージ
    /// @return AppError
    pub fn new(code: AppErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// 見つからないエラー (NotFound)
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::NotFound, message)
    }

    /// I/Oエラー (Io)
    pub fn io(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::Io, message)
    }

    /// DBエラー (Db)
    pub fn db(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::Db, message)
    }

    /// 引数不正エラー (InvalidArgument)
    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::InvalidArgument, message)
    }

    /// 処理中断エラー (Cancelled)
    pub fn cancelled(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::Cancelled, message)
    }

    /// 内部エラー (Internal)
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::Internal, message)
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        Self::io(err.to_string())
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(err: rusqlite::Error) -> Self {
        Self::db(err.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        Self::internal(err.to_string())
    }
}
