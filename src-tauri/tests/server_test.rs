use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;

use image_media_viewer_lib::db::{Database, ThumbnailStore};
use image_media_viewer_lib::pipeline::ThumbnailPipeline;
use image_media_viewer_lib::state::AppState;

#[test]
fn test_http_server_heartbeat_and_health() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("catalog.db");
    let cache_dir = dir.path().join("cache");
    std::fs::create_dir_all(&cache_dir).unwrap();

    let db = Database::open(&db_path).unwrap();
    let thumb_store = Arc::new(ThumbnailStore::open(&cache_dir).unwrap());
    let thumb_pipeline = ThumbnailPipeline::new(db.clone(), Arc::clone(&thumb_store), cache_dir.clone());
    let app_state = Arc::new(AppState::new(db, thumb_store, dir.path().to_path_buf(), cache_dir, thumb_pipeline));

    // テスト用ポート 14299 でサーバーを起動（常駐モード auto_exit = false）
    let test_port = 14299;
    image_media_viewer_lib::server::start_http_server(app_state, test_port, false);

    // サーバーの起動待ち
    std::thread::sleep(Duration::from_millis(200));

    // 1. GET /api/health のテスト

    // 標準の HTTP リクエスト送信
    let client = std::net::TcpStream::connect(format!("127.0.0.1:{}", test_port));
    assert!(client.is_ok(), "サーバーポートに接続できること");

    if let Ok(mut stream) = client {
        use std::io::{Read, Write};
        let req = format!("GET /api/health HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n", test_port);
        stream.write_all(req.as_bytes()).unwrap();

        let mut res_buf = String::new();
        stream.read_to_string(&mut res_buf).unwrap();

        assert!(res_buf.contains("200 OK"), "200 OK が返されること: {}", res_buf);
        assert!(res_buf.contains("\"alive\":true"), "alive: true が含まれること: {}", res_buf);
        assert!(res_buf.contains("\"status\":\"ok\""), "status: ok が含まれること: {}", res_buf);
    }

    // 2. POST /api/heartbeat のテスト
    if let Ok(mut stream) = std::net::TcpStream::connect(format!("127.0.0.1:{}", test_port)) {
        use std::io::{Read, Write};
        let req = format!("POST /api/heartbeat HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", test_port);
        stream.write_all(req.as_bytes()).unwrap();

        let mut res_buf = String::new();
        stream.read_to_string(&mut res_buf).unwrap();

        assert!(res_buf.contains("200 OK"), "POST heartbeat で 200 OK が返されること: {}", res_buf);
        assert!(res_buf.contains("\"alive\":true"), "alive: true が含まれること: {}", res_buf);
    }

    // 3. GET /api/logs のテスト
    if let Ok(mut stream) = std::net::TcpStream::connect(format!("127.0.0.1:{}", test_port)) {
        use std::io::{Read, Write};
        let req = format!("GET /api/logs?limit=50 HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n", test_port);
        stream.write_all(req.as_bytes()).unwrap();

        let mut res_buf = String::new();
        stream.read_to_string(&mut res_buf).unwrap();

        assert!(res_buf.contains("200 OK"), "GET /api/logs で 200 OK が返されること: {}", res_buf);
    }

    // 4. GET /api/thumbnails/failed のテスト
    if let Ok(mut stream) = std::net::TcpStream::connect(format!("127.0.0.1:{}", test_port)) {
        use std::io::{Read, Write};
        let req = format!("GET /api/thumbnails/failed HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n", test_port);
        stream.write_all(req.as_bytes()).unwrap();

        let mut res_buf = String::new();
        stream.read_to_string(&mut res_buf).unwrap();

        assert!(res_buf.contains("200 OK"), "GET /api/thumbnails/failed で 200 OK が返されること: {}", res_buf);
        assert!(res_buf.contains("[]"), "初期状態では空配列が返ること: {}", res_buf);
    }
}
