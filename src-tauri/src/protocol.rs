use std::fs;
use std::path::Path;
use std::sync::Arc;
use tauri::http::{header, Method, Response, StatusCode};
use tracing::warn;

use crate::pipeline::thumbnail::get_thumbnail_path;
use crate::state::AppState;

/// CORSヘッダー付きのレスポンスビルダーを作成する
///
/// 変更理由: WebView2からのクロスオリジン（tauri.localhost -> thumb.localhost）リクエストを
/// ブラウザエンジン側でブロックさせず、確実に画像を表示させるため。
fn cors_response_builder(status: StatusCode) -> tauri::http::response::Builder {
    Response::builder()
        .status(status)
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(header::ACCESS_CONTROL_ALLOW_METHODS, "GET, HEAD, OPTIONS")
        .header(header::ACCESS_CONTROL_ALLOW_HEADERS, "*")
}

/// URIから画像IDを抽出するヘルパー
///
/// 変更理由: WindowsのWebView2や各種環境下で、URIが完全URL（http://thumb.localhost/42?v=1）、
/// カスタムスキーム（thumb://localhost/42）、あるいは相対パス（/42?v=1）など多様な形式で渡されるため、
/// いかなる形式からでも確実に画像IDを抽出できるようにする。
fn extract_image_id(uri: &str) -> Option<i64> {
    // 1. スキーム付き完全URLとしてパースを試みる
    if let Ok(parsed) = url::Url::parse(uri) {
        let path = parsed.path().trim_matches('/');
        if let Ok(id) = path.parse::<i64>() {
            return Some(id);
        }
    }

    // 2. クエリパラメータを除去
    let path = uri.split('?').next().unwrap_or(uri);

    // 3. 既知のプレフィックスを除去
    let trimmed = path
        .strip_prefix("thumb://localhost/").unwrap_or(path);
    let trimmed = trimmed
        .strip_prefix("original://localhost/").unwrap_or(trimmed);
    let trimmed = trimmed
        .strip_prefix("http://thumb.localhost/").unwrap_or(trimmed);
    let trimmed = trimmed
        .strip_prefix("http://original.localhost/").unwrap_or(trimmed);
    let trimmed = trimmed
        .strip_prefix("thumb:").unwrap_or(trimmed);
    let trimmed = trimmed
        .strip_prefix("original:").unwrap_or(trimmed);

    // 4. 前後のスラッシュを除去して数値パース
    let trimmed = trimmed.trim_matches('/');
    trimmed.parse::<i64>().ok()
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
            warn!("Invalid thumbnail URI: {}", uri);
            return Ok(cors_response_builder(StatusCode::BAD_REQUEST)
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
            warn!("Thumbnail target image record not found: id={}", image_id);
            return Ok(cors_response_builder(StatusCode::NOT_FOUND)
                .body(b"Image not found".to_vec())?);
        }
    };

    // 1. メモリキャッシュチェック
    if let Some(cached_bytes) = state.get_thumbnail_bytes(&quick_hash) {
        return Ok(cors_response_builder(StatusCode::OK)
            .header(header::CONTENT_TYPE, "image/webp")
            .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
            .body((*cached_bytes).clone())?);
    }

    // 2. サムネイル専用DB (thumbnails.db) から BLOB 取得
    if let Ok(Some(bytes)) = state.thumb_store.get(&quick_hash) {
        let arc_bytes = Arc::new(bytes.clone());
        state.put_thumbnail_bytes(quick_hash, arc_bytes);
        return Ok(cors_response_builder(StatusCode::OK)
            .header(header::CONTENT_TYPE, "image/webp")
            .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
            .body(bytes)?);
    }

    // 3. 旧ディスクキャッシュ (thumbs/v1/{hash[0..2]}/{hash}.webp) があれば読み込んでDB移行＆返却
    let thumb_path = get_thumbnail_path(&state.cache_dir, &quick_hash);
    if thumb_path.exists() {
        if let Ok(bytes) = fs::read(&thumb_path) {
            state.thumb_store.put(&quick_hash, &bytes).ok();
            let arc_bytes = Arc::new(bytes.clone());
            state.put_thumbnail_bytes(quick_hash, arc_bytes);
            return Ok(cors_response_builder(StatusCode::OK)
                .header(header::CONTENT_TYPE, "image/webp")
                .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
                .body(bytes)?);
        }
    }

    // 4. キャッシュ未存在の場合、原寸画像ファイルからオンデマンド生成
    let file_path = Path::new(&file_path_str);
    if !file_path.exists() {
        warn!("Thumbnail original file not found: path={}", file_path_str);
        return Ok(cors_response_builder(StatusCode::NOT_FOUND)
            .body(b"Original file not found".to_vec())?);
    }

    // メタ情報取得 (Orientation, 寸法)
    let meta = {
        let conn = state.db.reader()?;
        crate::db::repo::get_image_detail(&conn, image_id)?
    };
    let (orientation, width, height) = match meta {
        Some(d) => (1u32, d.width, d.height),
        None => (1u32, None, None),
    };

    let success = state
        .thumb_pipeline
        .request_and_wait(
            image_id,
            file_path.to_path_buf(),
            quick_hash.clone(),
            orientation,
            width,
            height,
        )
        .await;

    if success {
        if let Ok(Some(bytes)) = state.thumb_store.get(&quick_hash) {
            let arc_bytes = Arc::new(bytes.clone());
            state.put_thumbnail_bytes(quick_hash, arc_bytes);
            return Ok(cors_response_builder(StatusCode::OK)
                .header(header::CONTENT_TYPE, "image/webp")
                .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
                .body(bytes)?);
        }
    }

    warn!("Failed to generate thumbnail: id={}, path={}", image_id, file_path_str);
    Ok(cors_response_builder(StatusCode::NOT_FOUND)
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
            warn!("Invalid original image URI: {}", uri);
            return Ok(cors_response_builder(StatusCode::BAD_REQUEST)
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
            warn!("Original image record not found: id={}", image_id);
            return Ok(cors_response_builder(StatusCode::NOT_FOUND)
                .body(b"Image not found".to_vec())?);
        }
    };

    let file_path = Path::new(&file_path_str);
    if !file_path.exists() {
        warn!("Original image file not found: path={}", file_path_str);
        return Ok(cors_response_builder(StatusCode::NOT_FOUND)
            .body(b"Original file not found".to_vec())?);
    }

    match fs::read(file_path) {
        Ok(bytes) => {
            let mime = get_mime_type(format.as_deref(), file_path);
            Ok(cors_response_builder(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime)
                .body(bytes)?)
        }
        Err(e) => {
            warn!("Failed to read original image: {:?}, error: {:?}", file_path, e);
            Ok(cors_response_builder(StatusCode::INTERNAL_SERVER_ERROR)
                .body(b"Failed to read image".to_vec())?)
        }
    }
}

/// Tauri Builder にサムネイルおよび原寸画像プロトコルを登録する
///
/// 変更理由: TCP/IPポートを一切介さず、WebViewから直接メモリ内IPCハンドラを呼び出し
/// WebPサムネイルおよび原寸画像バイナリを高速返却するため。
///
/// @param builder Tauri Builder インスタンス
/// @param state アプリケーション状態
/// @return プロトコル登録後の Tauri Builder
pub fn register_protocols<R: tauri::Runtime>(
    builder: tauri::Builder<R>,
    state: Arc<AppState>,
) -> tauri::Builder<R> {
    let state_thumb = Arc::clone(&state);
    let builder = builder.register_asynchronous_uri_scheme_protocol("thumb", move |_ctx, request, responder| {
        if request.method() == Method::OPTIONS {
            let resp = cors_response_builder(StatusCode::OK).body(Vec::new()).unwrap_or_default();
            responder.respond(resp);
            return;
        }
        let uri = request.uri().to_string();
        let s = Arc::clone(&state_thumb);
        tauri::async_runtime::spawn(async move {
            match handle_thumb_protocol(s, uri).await {
                Ok(resp) => responder.respond(resp),
                Err(e) => {
                    let err_resp = cors_response_builder(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(e.to_string().into_bytes())
                        .unwrap_or_default();
                    responder.respond(err_resp);
                }
            }
        });
    });

    let state_orig = Arc::clone(&state);
    builder.register_asynchronous_uri_scheme_protocol("original", move |_ctx, request, responder| {
        if request.method() == Method::OPTIONS {
            let resp = cors_response_builder(StatusCode::OK).body(Vec::new()).unwrap_or_default();
            responder.respond(resp);
            return;
        }
        let uri = request.uri().to_string();
        let s = Arc::clone(&state_orig);
        tauri::async_runtime::spawn(async move {
            match handle_original_protocol(s, uri).await {
                Ok(resp) => responder.respond(resp),
                Err(e) => {
                    let err_resp = cors_response_builder(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(e.to_string().into_bytes())
                        .unwrap_or_default();
                    responder.respond(err_resp);
                }
            }
        });
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_image_id() {
        assert_eq!(extract_image_id("http://thumb.localhost/42?v=123"), Some(42));
        assert_eq!(extract_image_id("thumb://localhost/42?v=123"), Some(42));
        assert_eq!(extract_image_id("http://original.localhost/100"), Some(100));
        assert_eq!(extract_image_id("original://localhost/100"), Some(100));
        assert_eq!(extract_image_id("thumb://localhost/42"), Some(42));
        assert_eq!(extract_image_id("thumb:42"), Some(42));
        assert_eq!(extract_image_id("/42?v=123"), Some(42));
        assert_eq!(extract_image_id("/42"), Some(42));
        assert_eq!(extract_image_id("42"), Some(42));
    }
}




