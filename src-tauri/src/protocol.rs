use std::fs;
use std::path::Path;
use std::sync::Arc;
use tauri::http::{header, Response, StatusCode};
use tracing::warn;

use crate::pipeline::thumbnail::get_thumbnail_path;
use crate::state::AppState;

/// URIから画像IDを抽出するヘルパー
///
/// 例: "thumb://localhost/42?v=123" または "http://thumb.localhost/42" -> 42
fn extract_image_id(uri: &str) -> Option<i64> {
    let parsed = url::Url::parse(uri).ok()?;
    let path = parsed.path().trim_start_matches('/');
    path.parse::<i64>().ok()
}

/// MIMEタイプを判定する
fn get_mime_type(format: Option<&str>, path: &Path) -> &'static str {
    if let Some(fmt) = format {
        match fmt.to_lowercase().as_str() {
            "jpeg" | "jpg" => "image/jpeg",
            "png" => "image/png",
            "webp" => "image/webp",
            "gif" => "image/gif",
            "bmp" => "image/bmp",
            _ => "application/octet-stream",
        }
    } else {
        match path.extension().and_then(|ext| ext.to_str()).unwrap_or("").to_lowercase().as_str() {
            "jpg" | "jpeg" => "image/jpeg",
            "png" => "image/png",
            "webp" => "image/webp",
            "gif" => "image/gif",
            "bmp" => "image/bmp",
            _ => "application/octet-stream",
        }
    }
}

/// `thumb://` プロトコルの非同期ハンドラ
///
/// 変更理由: 仕様書§5.5「thumb://localhost/{imageId}?v={rev} サムネWebPを返す。未生成なら最優先でキューに積み、生成完了後に応答する」
pub async fn handle_thumb_protocol(
    state: Arc<AppState>,
    uri: String,
) -> Result<Response<Vec<u8>>, Box<dyn std::error::Error + Send + Sync>> {
    let image_id = match extract_image_id(&uri) {
        Some(id) => id,
        None => {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(b"Invalid image id".to_vec())?);
        }
    };

    // DBから画像情報を取得
    let info = {
        let conn = state.db.reader()?;
        crate::db::repo::get_image_file_info(&conn, image_id)?
    };

    let (file_path_str, quick_hash, _, _) = match info {
        Some(item) => item,
        None => {
            return Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(b"Image not found".to_vec())?);
        }
    };

    let thumb_path = get_thumbnail_path(&state.cache_dir, &quick_hash);

    // キャッシュが存在すればそのまま返す
    if thumb_path.exists() {
        if let Ok(bytes) = fs::read(&thumb_path) {
            return Ok(Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "image/webp")
                .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
                .body(bytes)?);
        }
    }

    // キャッシュが存在しない場合、最優先キューで生成待機
    let file_path = Path::new(&file_path_str);
    if !file_path.exists() {
        return Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(b"Original file not found".to_vec())?);
    }

    // メタ情報取得 (Orientation, 寸法)
    let meta = {
        let conn = state.db.reader()?;
        crate::db::repo::get_image_detail(&conn, image_id)?
    };
    let (orientation, width, height) = match meta {
        Some(d) => (1u32, d.width, d.height), // デフォルトまたはDB値
        None => (1u32, None, None),
    };

    let success = state
        .thumb_pipeline
        .request_and_wait(
            image_id,
            file_path.to_path_buf(),
            quick_hash,
            orientation,
            width,
            height,
        )
        .await;

    if success && thumb_path.exists() {
        if let Ok(bytes) = fs::read(&thumb_path) {
            return Ok(Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "image/webp")
                .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
                .body(bytes)?);
        }
    }

    Ok(Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(b"Failed to generate thumbnail".to_vec())?)
}

/// `original://` プロトコルの非同期ハンドラ
///
/// 変更理由: 仕様書§5.5「original://localhost/{imageId} 原寸画像を返す」
pub async fn handle_original_protocol(
    state: Arc<AppState>,
    uri: String,
) -> Result<Response<Vec<u8>>, Box<dyn std::error::Error + Send + Sync>> {
    let image_id = match extract_image_id(&uri) {
        Some(id) => id,
        None => {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(b"Invalid image id".to_vec())?);
        }
    };

    let info = {
        let conn = state.db.reader()?;
        crate::db::repo::get_image_file_info(&conn, image_id)?
    };

    let (file_path_str, _, _, format) = match info {
        Some(item) => item,
        None => {
            return Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(b"Image not found".to_vec())?);
        }
    };

    let file_path = Path::new(&file_path_str);
    if !file_path.exists() {
        return Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(b"Original file not found".to_vec())?);
    }

    match fs::read(file_path) {
        Ok(bytes) => {
            let mime = get_mime_type(format.as_deref(), file_path);
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime)
                .body(bytes)?)
        }
        Err(e) => {
            warn!("原寸画像読み取り失敗: {:?}, error: {:?}", file_path, e);
            Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(b"Failed to read image".to_vec())?)
        }
    }
}
