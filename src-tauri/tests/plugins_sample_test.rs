//! プラグインサンプルファイルおよび同梱リソースの検証テスト
//!
//! 変更理由: インストーラーに同梱される plugins ディレクトリ内の設定サンプル
//! （app_settings.sample.json, custom_action_sample.json）および標準フォーマットプラグインマニフェスト
//! （plugins/clipstudio/plugin.json, plugins/sample_format/plugin.json）が、
//! アプリ本体のデータ構造・仕様と整合していることを自動検証するため。

use std::fs;
use std::path::PathBuf;

use image_media_viewer_lib::plugins::{DynamicFormatPlugin, ImageFormatPlugin, PluginManifest};
use image_media_viewer_lib::settings::{AppSettings, CustomContextMenuItem};

/// プロジェクトルートの plugins ディレクトリのパスを取得する
fn get_plugins_dir() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.join("../plugins")
}

#[test]
fn test_app_settings_sample_json_validity() {
    let plugins_dir = get_plugins_dir();
    let sample_file = plugins_dir.join("app_settings.sample.json");
    assert!(sample_file.exists(), "app_settings.sample.json が存在すること");

    let content = fs::read_to_string(&sample_file).expect("app_settings.sample.json の読み込みに失敗");
    let settings: AppSettings = serde_json::from_str(&content)
        .expect("app_settings.sample.json が AppSettings 構造体へ正常にデシリアライズできること");

    assert!(settings.stay_in_tray, "常駐フラグが設定されていること");
    assert_eq!(settings.locale, "ja", "言語設定が ja であること");
    assert!(!settings.enabled_plugins.is_empty(), "有効化プラグイン一覧が空でないこと");
    assert!(
        settings.enabled_plugins.contains(&"format.clipstudio".to_string()),
        "format.clipstudio が含まれていること"
    );
    assert!(
        !settings.custom_context_menu_items.is_empty(),
        "カスタムコンテキストメニューサンプルが含まれていること"
    );
    for item in &settings.custom_context_menu_items {
        assert!(!item.id.is_empty(), "項目IDが空でないこと");
        assert!(!item.name.is_empty(), "項目名が空でないこと");
        assert!(!item.app_path.is_empty(), "appPath が空でないこと");
    }
}

#[test]
fn test_custom_action_sample_json_validity() {
    let plugins_dir = get_plugins_dir();
    let sample_file = plugins_dir.join("action_plugins/custom_action_sample.json");
    assert!(sample_file.exists(), "custom_action_sample.json が存在すること");

    let content = fs::read_to_string(&sample_file).expect("custom_action_sample.json の読み込みに失敗");
    let items: Vec<CustomContextMenuItem> = serde_json::from_str(&content)
        .expect("custom_action_sample.json が Vec<CustomContextMenuItem> へ正常にデシリアライズできること");

    assert!(!items.is_empty(), "アクションサンプル一覧が空でないこと");
    for item in &items {
        assert!(!item.id.is_empty(), "項目IDが空でないこと");
        assert!(!item.name.is_empty(), "項目名が空でないこと");
        assert!(!item.app_path.is_empty(), "appPath が空でないこと");
        assert!(item.args.is_some(), "引数テンプレートが指定されていること");
    }
}

#[test]
fn test_clip_studio_plugin_json_validity_and_runtime() {
    let plugins_dir = get_plugins_dir();
    let clip_dir = plugins_dir.join("clipstudio");
    let manifest_file = clip_dir.join("plugin.json");
    assert!(manifest_file.exists(), "plugins/clipstudio/plugin.json が存在すること");

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
    let plugin = DynamicFormatPlugin::new(manifest, clip_dir);
    assert_eq!(plugin.id(), "format.clipstudio");
    assert_eq!(plugin.name(), "CLIP STUDIO PAINT (.clip) Support");
    assert_eq!(plugin.supported_extensions(), &["clip"]);
}

#[test]
fn test_sample_format_plugin_json_validity() {
    let plugins_dir = get_plugins_dir();
    let sample_dir = plugins_dir.join("sample_format");
    let manifest_file = sample_dir.join("plugin.json");
    assert!(manifest_file.exists(), "plugins/sample_format/plugin.json が存在すること");

    let content = fs::read_to_string(&manifest_file).expect("plugin.json の読み込みに失敗");
    let manifest: PluginManifest = serde_json::from_str(&content)
        .expect("plugin.json が PluginManifest 構造体へ正常にデシリアライズできること");

    assert_eq!(manifest.id, "format.sample");
    assert_eq!(manifest.extensions, vec!["sampleimg".to_string()]);
    assert!(manifest.extractor.is_some());
}

#[test]
fn test_bundled_plugins_loadable_with_default_settings() {
    let plugins_dir = get_plugins_dir();
    let default_plugins = image_media_viewer_lib::settings::default_enabled_plugins();

    let registry = image_media_viewer_lib::plugins::FormatPluginRegistry::new(
        &default_plugins,
        &plugins_dir,
    );

    assert!(
        registry.is_plugin_enabled("format.clipstudio"),
        "ClipStudioプラグインがデフォルト設定で有効化されていること"
    );
    assert!(
        registry.is_plugin_enabled("format.sample"),
        "SampleFormatプラグインがformat.*ワイルドカードでデフォルト有効化されていること"
    );
    let exts = registry.supported_extensions();
    assert!(exts.contains(&"clip".to_string()), "clip拡張子が登録されていること");
    assert!(exts.contains(&"sampleimg".to_string()), "sampleimg拡張子が登録されていること");
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
    use std::sync::Arc;
    use tempfile::tempdir;

    let plugins_dir = get_plugins_dir();
    let default_plugins = image_media_viewer_lib::settings::default_enabled_plugins();

    // 1. レジストリを初期化し、ClipStudioプラグイン（外部バイナリ）がロードされることを検証
    let registry = Arc::new(image_media_viewer_lib::plugins::FormatPluginRegistry::new(
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

    println!("Plugin get_by_extension clip: {:?}", registry.get_by_extension("clip").is_some());
    println!("Supported extensions: {:?}", registry.supported_extensions());

    // フォルダをスキャン
    image_media_viewer_lib::scanner::scan_folder_core(
        None,
        Arc::clone(&app_state),
        folder_id,
        folder_path_str.clone(),
        false,
    ).expect("フォルダスキャンが正常に完了すること");

    // DB に .clip ファイルがインデックスされているか検証
    let reader = db.reader().unwrap();
    let img_count: i64 = reader.query_row("SELECT count(*) FROM images;", [], |r| r.get(0)).unwrap();
    println!("Images count in DB: {}", img_count);
    let watched_count: i64 = reader.query_row("SELECT count(*) FROM watched_folders;", [], |r| r.get(0)).unwrap();
    println!("Watched folders count in DB: {}", watched_count);

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
    use std::sync::Arc;
    use tempfile::tempdir;

    let app_dir = tempdir().unwrap();
    let plugins_dir = get_plugins_dir();

    // 1. フォーマットプラグイン導入前の古い app_settings.json を再現保存
    let legacy_json = r#"{"stay_in_tray": true, "locale": "ja", "enabled_plugins": ["action.open_default"]}"#;
    fs::write(app_dir.path().join("app_settings.json"), legacy_json).unwrap();

    // 2. load_settings による互換性マイグレーションの実行
    let loaded_settings = image_media_viewer_lib::settings::load_settings(app_dir.path());
    assert!(
        loaded_settings.enabled_plugins.contains(&"format.clipstudio".to_string()),
        "古い設定ファイルであっても自動マイグレーションにより format.clipstudio が追加されること"
    );
    assert!(
        loaded_settings.enabled_plugins.contains(&"format.*".to_string()),
        "古い設定ファイルであっても自動マイグレーションにより format.* が追加されること"
    );

    // 3. レジストリを初期化し、clip 拡張子がサポート対象として登録されていることを検証
    let registry = Arc::new(image_media_viewer_lib::plugins::FormatPluginRegistry::new(
        &loaded_settings.enabled_plugins,
        &plugins_dir,
    ));
    assert!(
        registry.supported_extensions().contains(&"clip".to_string()),
        "マイグレーション後の設定により 'clip' 拡張子が走査対象に追加されていること"
    );

    // 4. 監視フォルダ内の .clip ファイルが walker により走査・登録されることを検証
    let folder_dir = tempdir().unwrap();
    let clip_path = folder_dir.path().join("legacy_migrated.clip");
    let clip_bytes = create_test_clip_file(320, 240);
    fs::write(&clip_path, &clip_bytes).unwrap();

    let cache_dir = tempdir().unwrap();
    let db_path = app_dir.path().join("catalog.db");
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
        app_dir.path().to_path_buf(),
        cache_dir.path().to_path_buf(),
        thumb_pipeline,
        Arc::clone(&registry),
    ));

    let folder_str = folder_dir.path().to_string_lossy().to_string();
    let folder_id = {
        let mut writer = db.writer();
        image_media_viewer_lib::db::repo::add_watched_folder(&mut writer, &folder_str, 1720000000)
            .unwrap()
            .id
    };

    image_media_viewer_lib::scanner::scan_folder_core(
        None,
        Arc::clone(&app_state),
        folder_id,
        folder_str,
        false,
    ).expect("スキャン完了");

    let reader = db.reader().unwrap();
    let count: i64 = reader.query_row("SELECT count(*) FROM images WHERE format = 'clip';", [], |r| r.get(0)).unwrap();
    assert_eq!(count, 1, "古い設定から移行した環境でも .clip ファイルが正常にインデックスされること");
}

