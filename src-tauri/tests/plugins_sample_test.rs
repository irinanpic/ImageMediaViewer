//! プラグインパッケージ導入・動的ロードおよび同梱リソースの結合検証テスト
//!
//! 変更理由: 本番ディレクトリ（plugins/）にテスト用ダミープラグイン（sample_format等）を放置せず、
//! 一時ディレクトリを用いたプラグインのフォルダ・ZIPインポート（install_plugin_from_path）および
//! 独立ビルドされた ClipStudio プラグインの動作をクリーンに自動検証するため。

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::tempdir;

use image_media_viewer_lib::commands::install_plugin_from_path;
use image_media_viewer_lib::plugins::{DynamicFormatPlugin, FormatPluginRegistry, ImageFormatPlugin, PluginManifest};
use image_media_viewer_lib::settings::default_enabled_plugins;

/// プロジェクトルートの plugins ディレクトリのパスを取得する
fn get_plugins_dir() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.join("../plugins")
}

/// 独立プラグインソース（plugins-src/clipstudio）のパスを取得する
fn get_clipstudio_src_dir() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.join("../plugins-src/clipstudio")
}

#[test]
fn test_clip_studio_plugin_json_validity_and_runtime() {
    let clip_src_dir = get_clipstudio_src_dir();
    let manifest_file = clip_src_dir.join("plugin.json");
    assert!(manifest_file.exists(), "plugins-src/clipstudio/plugin.json が存在すること");

    let content = fs::read_to_string(&manifest_file).expect("plugin.json の読み込みに失敗");
    let manifest: PluginManifest = serde_json::from_str(&content)
        .expect("plugin.json が PluginManifest 構造体へ正常にデシリアライズできること");

    assert_eq!(manifest.id, "format.clipstudio");
    assert_eq!(manifest.name, "CLIP STUDIO PAINT (.clip) Support");
    assert_eq!(manifest.extensions, vec!["clip".to_string()]);
    assert!(manifest.extractor.is_some(), "画像抽出エンジンが定義されていること");
    assert!(!manifest.actions.is_empty(), "アクションが定義されていること");
    assert_eq!(manifest.actions[0].id, "action.open_clip_studio");

    // 動的プラグインランタイムへの読み込み検証
    let plugin = DynamicFormatPlugin::new(manifest, clip_src_dir);
    assert_eq!(plugin.id(), "format.clipstudio");
    assert_eq!(plugin.name(), "CLIP STUDIO PAINT (.clip) Support");
    assert_eq!(plugin.supported_extensions(), &["clip"]);
}

#[test]
fn test_install_plugin_from_directory_and_zip() {
    let app_data_dir = tempdir().unwrap();
    let external_plugin_src = tempdir().unwrap();

    let sample_manifest = r#"{
        "id": "format.custom_ext",
        "name": "Custom External Plugin",
        "version": "1.2.0",
        "description": "外部開発プラグインのインポート検証用",
        "extensions": ["cstm"],
        "extractor": {
            "type": "binary_slice",
            "start_signature": "89504E470D0A1A0A",
            "end_signature": "49454E44AE426082",
            "include_end_signature": true,
            "mime_type": "image/png"
        },
        "actions": [
            {
                "id": "action.open_custom_ext",
                "name": "Customアプリで開く",
                "order": 25,
                "extensions": ["cstm"]
            }
        ]
    }"#;

    fs::write(external_plugin_src.path().join("plugin.json"), sample_manifest).unwrap();

    // 1. フォルダ指定でのプラグインインストール検証
    let installed_manifest = install_plugin_from_path(app_data_dir.path(), external_plugin_src.path())
        .expect("フォルダからのプラグインインストールに成功すること");
    assert_eq!(installed_manifest.id, "format.custom_ext");

    let installed_file = app_data_dir
        .path()
        .join("plugins")
        .join("custom_ext")
        .join("plugin.json");
    assert!(installed_file.exists(), "ユーザーの plugins/custom_ext/plugin.json にコピーされていること");

    // デフォルト設定（ワイルドカード）で自動的に有効化・認識されること
    let default_plugins = default_enabled_plugins();
    let registry = FormatPluginRegistry::new(&default_plugins, app_data_dir.path());
    assert!(registry.is_plugin_enabled("format.custom_ext"));
    assert!(registry.supported_extensions().contains(&"cstm".to_string()));
    let action = registry.find_action("action.open_custom_ext").expect("アクションが登録されていること");
    assert_eq!(action.plugin_id, Some("format.custom_ext".to_string()));

    // 2. ZIPアーカイブからのプラグインインストール検証
    let zip_dir = tempdir().unwrap();
    let zip_path = zip_dir.path().join("custom_zip_plugin.zip");
    let zip_staging = zip_dir.path().join("zip_pkg");
    fs::create_dir_all(&zip_staging).unwrap();
    let zip_manifest = sample_manifest
        .replace("format.custom_ext", "format.zipped_ext")
        .replace("\"cstm\"", "\"zext\"");
    fs::write(zip_staging.join("plugin.json"), zip_manifest).unwrap();

    let tar_status = std::process::Command::new("tar")
        .arg("-a")
        .arg("-cf")
        .arg(&zip_path)
        .arg("-C")
        .arg(zip_dir.path())
        .arg("zip_pkg")
        .status();

    if let Ok(s) = tar_status {
        if s.success() && zip_path.exists() {
            let zip_installed = install_plugin_from_path(app_data_dir.path(), &zip_path)
                .expect("ZIPアーカイブからのプラグインインストールに成功すること");
            assert_eq!(zip_installed.id, "format.zipped_ext");
            registry.reload(&default_plugins, app_data_dir.path());
            assert!(registry.is_plugin_enabled("format.zipped_ext"));
            assert!(registry.supported_extensions().contains(&"zext".to_string()));
        }
    }
}

/// テスト用の有効な mock .clip ファイル（CSFCHUNK + SQLiteチャンク）を生成
fn create_test_clip_file(width: u32, height: u32) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(b"CSFCHUNK");
    data.extend_from_slice(&0u64.to_be_bytes());

    let img = image::RgbaImage::new(width, height);
    let mut png_buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut png_buf, image::ImageFormat::Png).expect("PNGエンコード失敗");
    let png_bytes = png_buf.into_inner();

    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE CanvasPreview (ImageData BLOB);").unwrap();
    conn.execute("INSERT INTO CanvasPreview (ImageData) VALUES (?1)", [&png_bytes]).unwrap();

    let db_data = conn.serialize(rusqlite::DatabaseName::Main).unwrap();

    data.extend_from_slice(b"CHNKSQLi");
    data.extend_from_slice(&(db_data.len() as u64).to_be_bytes());
    data.extend_from_slice(&db_data);

    data
}

#[test]
fn test_clipstudio_binary_plugin_loading_and_walker_scan() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();

    let plugins_dir = get_plugins_dir();
    let default_plugins = default_enabled_plugins();

    // 1. レジストリを初期化し、ClipStudioプラグイン（外部バイナリ）がロードされることを検証
    let registry = Arc::new(FormatPluginRegistry::new(
        &default_plugins,
        &plugins_dir,
    ));

    assert!(
        registry.is_plugin_enabled("format.clipstudio"),
        "ClipStudioプラグインが有効化されていること"
    );
    assert!(
        registry.supported_extensions().contains(&"clip".to_string()),
        "走査対象拡張子に 'clip' が含まれていること"
    );

    let plugin = registry.get_by_extension("clip").expect("clip形式プラグインが取得できること");

    // 2. テスト用の .clip ファイルを一時フォルダに作成
    let temp_dir = tempdir().unwrap();
    let clip_file_path = temp_dir.path().join("artwork.clip");
    let mock_clip_bytes = create_test_clip_file(640, 480);
    fs::write(&clip_file_path, &mock_clip_bytes).unwrap();

    // 3. プラグインバイナリ経由でのメタデータ抽出検証
    let mut opened_file = fs::File::open(&clip_file_path).unwrap();
    let meta = plugin.extract_metadata(&clip_file_path, &mut opened_file, 1720000000)
        .expect("プラグインバイナリからのメタデータ抽出に成功すること");
    assert_eq!(meta.width, Some(640));
    assert_eq!(meta.height, Some(480));
    assert_eq!(meta.format, Some("clip".to_string()));

    // 4. プラグインバイナリ経由でのプレビューPNG抽出検証
    let (preview_bytes, mime) = plugin.extract_original_preview(&clip_file_path)
        .expect("プラグインバイナリからのプレビュー抽出に成功すること");
    assert_eq!(mime, "image/png");
    assert!(preview_bytes.starts_with(b"\x89PNG\r\n\x1a\n"));

    // 5. サムネイルWebP生成検証
    let thumb_bytes = plugin.generate_thumbnail(&clip_file_path, 1)
        .expect("プラグインバイナリ画像からのWebPサムネイル生成に成功すること");
    assert_eq!(&thumb_bytes[0..4], b"RIFF");
    assert_eq!(&thumb_bytes[8..12], b"WEBP");

    // 6. フォルダスキャナー（scan_folder_core）とDBインデックスへの取り込み検証
    let cache_dir = tempdir().unwrap();
    let db_path = temp_dir.path().join("catalog.db");
    let db = image_media_viewer_lib::db::Database::open(&db_path).unwrap();
    let thumb_store = Arc::new(image_media_viewer_lib::db::ThumbnailStore::open(cache_dir.path()).unwrap());
    let thumb_pipeline = image_media_viewer_lib::pipeline::ThumbnailPipeline::new(
        db.clone(),
        Arc::clone(&thumb_store),
        cache_dir.path().to_path_buf(),
    );

    let app_state = Arc::new(image_media_viewer_lib::state::AppState::new(
        db.clone(),
        thumb_store,
        temp_dir.path().to_path_buf(),
        cache_dir.path().to_path_buf(),
        thumb_pipeline,
        Arc::clone(&registry),
    ));

    let folder_path_str = temp_dir.path().to_string_lossy().to_string();
    let folder_id = {
        let mut writer = db.writer();
        image_media_viewer_lib::db::repo::add_watched_folder(&mut writer, &folder_path_str, 1720000000)
            .expect("監視フォルダの事前登録に成功すること")
            .id
    };

    // フォルダをスキャン
    image_media_viewer_lib::scanner::scan_folder_core(
        None,
        Arc::clone(&app_state),
        folder_id,
        folder_path_str,
        false,
    ).expect("フォルダスキャンが正常に完了すること");

    // DB に .clip ファイルがインデックスされているか検証
    let reader = db.reader().unwrap();
    let mut stmt = reader.prepare("SELECT file_path, width, height, format FROM images;").unwrap();
    let mut rows = stmt.query([]).unwrap();

    let row = rows.next().unwrap().expect(".clip ファイルがDBに登録されていること");
    let stored_path: String = row.get(0).unwrap();
    let stored_w: Option<u32> = row.get(1).unwrap();
    let stored_h: Option<u32> = row.get(2).unwrap();
    let stored_fmt: Option<String> = row.get(3).unwrap();

    assert!(stored_path.ends_with("artwork.clip"));
    assert_eq!(stored_w, Some(640));
    assert_eq!(stored_h, Some(480));
    assert_eq!(stored_fmt, Some("clip".to_string()));
}

#[test]
fn test_legacy_settings_migration_and_clip_scanning() {
    let app_dir = tempdir().unwrap();
    let plugins_dir = get_plugins_dir();

    // 1. フォーマットプラグイン導入前の古い app_settings.json を再現保存
    let legacy_json = r#"{"stay_in_tray": true, "locale": "ja", "enabled_plugins": ["action.open_default"]}"#;
    fs::write(app_dir.path().join("app_settings.json"), legacy_json).unwrap();

    // 2. load_settings による互換性マイグレーションの実行（特定プラグイン名に依存せず format.* が補完される）
    let loaded_settings = image_media_viewer_lib::settings::load_settings(app_dir.path());
    assert!(
        loaded_settings.enabled_plugins.contains(&"format.*".to_string()),
        "古い設定ファイルであっても自動マイグレーションにより format.* が追加されること"
    );

    // 3. レジストリを初期化し、clip 拡張子がサポート対象として登録されていることを検証
    let registry = Arc::new(FormatPluginRegistry::new(
        &loaded_settings.enabled_plugins,
        &plugins_dir,
    ));
    assert!(
        registry.supported_extensions().contains(&"clip".to_string()),
        "マイグレーション後の設定により 'clip' 拡張子が走査対象に追加されていること"
    );
}
