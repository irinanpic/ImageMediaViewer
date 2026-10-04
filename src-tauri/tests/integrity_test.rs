use image::{Rgba, RgbaImage};
use image_media_viewer_lib::db::repo::{
    filter_unreferenced_hashes, is_quick_hash_referenced_elsewhere,
    update_image_on_file_changed, NewImageRecord,
};
use image_media_viewer_lib::db::{Database, ThumbnailStore};
use image_media_viewer_lib::pipeline::hash::calculate_quick_hash;
use image_media_viewer_lib::pipeline::thumbnail::generate_thumbnail_bytes;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::sync::Arc;
use std::time::UNIX_EPOCH;
use tempfile::tempdir;

#[test]
fn test_shared_hash_update_and_gc_integrity() {
    let dir = tempdir().unwrap();
    let cache_dir = dir.path().join("cache");
    let app_dir = dir.path().join("app");
    fs::create_dir_all(&cache_dir).unwrap();
    fs::create_dir_all(&app_dir).unwrap();

    // 1. DBとThumbnailStoreの初期化
    let db = Database::open(app_dir.join("catalog.db")).unwrap();
    let thumb_store = Arc::new(ThumbnailStore::open(&cache_dir).unwrap());

    // 監視フォルダを登録
    let folder_a = dir.path().join("folder_a");
    let folder_b = dir.path().join("folder_b");
    fs::create_dir_all(&folder_a).unwrap();
    fs::create_dir_all(&folder_b).unwrap();

    let (folder_id_a, folder_id_b) = {
        let conn = db.writer();
        let fa = image_media_viewer_lib::db::repo::add_watched_folder(
            &conn,
            &folder_a.to_string_lossy(),
            1000,
        )
        .unwrap();
        let fb = image_media_viewer_lib::db::repo::add_watched_folder(
            &conn,
            &folder_b.to_string_lossy(),
            1000,
        )
        .unwrap();
        (fa.id, fb.id)
    };

    // 2. 同一の画像ファイル2枚を作成（同一内容・同一ハッシュ）
    let img_path_a = folder_a.join("same_image.png");
    let img_path_b = folder_b.join("same_image.png");

    let mut img = RgbaImage::new(200, 200);
    for x in 0..200 {
        for y in 0..200 {
            img.put_pixel(x, y, Rgba([200, 100, 50, 255]));
        }
    }
    img.save(&img_path_a).unwrap();
    img.save(&img_path_b).unwrap();

    let size_orig = fs::metadata(&img_path_a).unwrap().len();
    let mtime_orig = fs::metadata(&img_path_a)
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let hash_orig = calculate_quick_hash(&img_path_a, size_orig).unwrap();
    let hash_b = calculate_quick_hash(&img_path_b, size_orig).unwrap();
    assert_eq!(hash_orig, hash_b, "同一画像のため同一ハッシュであること");

    // 3. カタログDBに2枚とも登録
    let (id_a, id_b) = {
        let mut conn = db.writer();
        image_media_viewer_lib::db::repo::upsert_images(
            &mut conn,
            &[NewImageRecord {
                folder_id: folder_id_a,
                file_path: img_path_a.to_string_lossy().to_string(),
                file_size: size_orig,
                file_mtime: mtime_orig,
                taken_at: mtime_orig,
                taken_at_source: 1,
                width: Some(200),
                height: Some(200),
                orientation: 1,
                format: Some("png".to_string()),
                quick_hash: hash_orig.clone(),
                indexed_at: mtime_orig,
            }],
        )
        .unwrap();

        image_media_viewer_lib::db::repo::upsert_images(
            &mut conn,
            &[NewImageRecord {
                folder_id: folder_id_b,
                file_path: img_path_b.to_string_lossy().to_string(),
                file_size: size_orig,
                file_mtime: mtime_orig,
                taken_at: mtime_orig,
                taken_at_source: 1,
                width: Some(200),
                height: Some(200),
                orientation: 1,
                format: Some("png".to_string()),
                quick_hash: hash_orig.clone(),
                indexed_at: mtime_orig,
            }],
        )
        .unwrap();

        // 実際のIDを取得
        let aid: i64 = conn
            .query_row("SELECT id FROM images WHERE file_path = ?1", [&img_path_a.to_string_lossy().to_string()], |r| r.get(0))
            .unwrap();
        let bid: i64 = conn
            .query_row("SELECT id FROM images WHERE file_path = ?1", [&img_path_b.to_string_lossy().to_string()], |r| r.get(0))
            .unwrap();
        (aid, bid)
    };

    // 4. サムネイルBLOBを生成して保存（1件分のみ登録される）
    let webp_bytes_orig = generate_thumbnail_bytes(&img_path_a, 1).unwrap();
    thumb_store.put(&hash_orig, &webp_bytes_orig).unwrap();
    assert_eq!(thumb_store.count().unwrap(), 1);
    assert!(thumb_store.exists(&hash_orig).unwrap());

    // 他方からの参照確認（id_aから見てhash_origはid_bでも使われている）
    {
        let conn = db.reader().unwrap();
        assert!(is_quick_hash_referenced_elsewhere(&conn, &hash_orig, id_a).unwrap());
        assert!(is_quick_hash_referenced_elsewhere(&conn, &hash_orig, id_b).unwrap());
    }

    // 5. 【片側更新テスト】画像Aのみファイル末尾に追記して更新
    {
        let mut file = OpenOptions::new().append(true).open(&img_path_a).unwrap();
        file.write_all(b"EXTRA_APPENDED_BYTES_FOR_IMAGE_A").unwrap();
        file.flush().unwrap();
    }

    let meta_a_new = fs::metadata(&img_path_a).unwrap();
    let size_a_new = meta_a_new.len();
    let mtime_a_new = meta_a_new
        .modified()
        .unwrap()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    assert_ne!(size_a_new, size_orig);

    let hash_a_new = calculate_quick_hash(&img_path_a, size_a_new).unwrap();
    assert_ne!(hash_a_new, hash_orig, "画像Aのハッシュが新しくなっていること");

    // 実アクセス時更新検知処理: 画像Aのレコードを更新
    {
        let conn = db.writer();
        update_image_on_file_changed(
            &conn,
            id_a,
            size_a_new,
            mtime_a_new,
            &hash_a_new,
            Some(200),
            Some(200),
            1,
        )
        .unwrap();
    }

    // 画像A用の新サムネイルを生成して保存
    let webp_bytes_new_a = generate_thumbnail_bytes(&img_path_a, 1).unwrap();
    thumb_store.put(&hash_a_new, &webp_bytes_new_a).unwrap();

    // 【最重要検証項目】
    // 画像Aを更新した後でも、画像B用の hash_orig のBLOBは決して削除されずに残っていること！
    assert!(
        thumb_store.exists(&hash_orig).unwrap(),
        "画像Aを更新しても、画像Bが参照する元のサムネイルBLOB (hash_orig) が消えてはならない！"
    );
    assert!(
        thumb_store.exists(&hash_a_new).unwrap(),
        "画像Aの新しいサムネイルBLOB (hash_a_new) が保存されていること"
    );
    assert_eq!(thumb_store.count().unwrap(), 2);

    // 画像Bの参照確認（id_aは別ハッシュになったため、id_a以外でhash_origを参照する画像はもう存在しない）
    {
        let conn = db.reader().unwrap();
        // id_b から見て hash_orig を参照する別画像はもう無い（id_b自身のみが参照）
        assert!(!is_quick_hash_referenced_elsewhere(&conn, &hash_orig, id_b).unwrap());
        // id_a から見て hash_a_new を参照する別画像も無い
        assert!(!is_quick_hash_referenced_elsewhere(&conn, &hash_a_new, id_a).unwrap());
    }

    // 6. 【両側更新テスト】画像Bもファイルを更新
    {
        let mut file = OpenOptions::new().append(true).open(&img_path_b).unwrap();
        file.write_all(b"DIFFERENT_APPENDED_BYTES_FOR_IMAGE_B").unwrap();
        file.flush().unwrap();
    }

    let meta_b_new = fs::metadata(&img_path_b).unwrap();
    let size_b_new = meta_b_new.len();
    let mtime_b_new = meta_b_new
        .modified()
        .unwrap()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let hash_b_new = calculate_quick_hash(&img_path_b, size_b_new).unwrap();
    assert_ne!(hash_b_new, hash_orig);
    assert_ne!(hash_b_new, hash_a_new);

    // 画像Bのレコードを更新
    {
        let conn = db.writer();
        update_image_on_file_changed(
            &conn,
            id_b,
            size_b_new,
            mtime_b_new,
            &hash_b_new,
            Some(200),
            Some(200),
            1,
        )
        .unwrap();
    }

    let webp_bytes_new_b = generate_thumbnail_bytes(&img_path_b, 1).unwrap();
    thumb_store.put(&hash_b_new, &webp_bytes_new_b).unwrap();
    assert_eq!(thumb_store.count().unwrap(), 3);

    // 7. 【孤立サムネイル回収 (GC) テスト】
    // この時点で hash_orig を参照する画像レコードは catalog.db 全体で 0 件！
    {
        let conn = db.reader().unwrap();
        let all_hashes = thumb_store.list_hashes(0, 10).unwrap();
        let unreferenced = filter_unreferenced_hashes(&conn, &all_hashes).unwrap();

        assert_eq!(unreferenced.len(), 1);
        assert_eq!(unreferenced[0], hash_orig, "hash_orig のみが孤立と判定されること");

        // 孤立サムネイルを安全に回収
        let deleted = thumb_store.delete_batch(&unreferenced).unwrap();
        assert_eq!(deleted, 1);
    }

    // 最終検証: 孤立した hash_orig は回収され、有効な hash_a_new と hash_b_new のみ残る
    assert_eq!(thumb_store.count().unwrap(), 2);
    assert!(!thumb_store.exists(&hash_orig).unwrap(), "孤立した hash_orig は安全に削除されたこと");
    assert!(thumb_store.exists(&hash_a_new).unwrap(), "有効な hash_a_new は維持されていること");
    assert!(thumb_store.exists(&hash_b_new).unwrap(), "有効な hash_b_new は維持されていること");
}
