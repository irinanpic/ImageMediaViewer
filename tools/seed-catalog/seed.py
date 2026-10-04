#!/usr/bin/env python3
"""
10万件ダミーデータ投入スクリプト (tools/seed-catalog/seed.py)

変更理由: 仕様書§12「tools/seed-catalog で DB に 10万件を投入する。
quick_hash を数十種に使い回し、対応する実サムネを数十枚だけ用意することで、実画像なしでも10万枚のタイムラインを再現する」
"""

import os
import sys
import sqlite3
import time
import random
from pathlib import Path

TOTAL_RECORDS = 100_000
SAMPLE_HASH_COUNT = 30 # 30種類のサムネイルを使い回す

def get_app_dirs():
    appdata = os.environ.get("APPDATA")
    localappdata = os.environ.get("LOCALAPPDATA")
    if not appdata or not localappdata:
        raise RuntimeError("APPDATA または LOCALAPPDATA が未定義です")
    
    data_dir = Path(appdata) / "com.imagemediaviewer.app"
    cache_dir = Path(localappdata) / "com.imagemediaviewer.app"
    return data_dir, cache_dir

# 最小限の1x1 WebPバイナリ（正式なWebP仕様に準拠したロスレス1x1ピクセル）
import base64
MINIMAL_WEBP = base64.b64decode("UklGRkoAAABXRUJQVlA4WAoAAAAQAAAAAAAAAAAAQUxQSAwAAAARBxAR/Q9ERP8DAABWUDggGAAAADABAJ0BKgEAAQAAAP4AAA3AAP7mtQAAAA==")


def create_sample_thumbnails(cache_dir: Path, hashes: list[str]):
    thumbs_root = cache_dir / "thumbs" / "v1"
    thumbs_root.mkdir(parents=True, exist_ok=True)
    
    for h in hashes:
        prefix = h[:2]
        dest_dir = thumbs_root / prefix
        dest_dir.mkdir(parents=True, exist_ok=True)
        thumb_file = dest_dir / f"{h}.webp"
        if not thumb_file.exists():
            thumb_file.write_bytes(MINIMAL_WEBP)

def main():
    data_dir, cache_dir = get_app_dirs()
    data_dir.mkdir(parents=True, exist_ok=True)
    cache_dir.mkdir(parents=True, exist_ok=True)

    db_path = data_dir / "catalog.db"
    print(f"カタログDB: {db_path}")
    print(f"キャッシュディレクトリ: {cache_dir}")

    # 30種類のハッシュを生成
    sample_hashes = [f"{i:04x}" * 16 for i in range(SAMPLE_HASH_COUNT)]
    print(f"実サムネイルキャッシュ（{SAMPLE_HASH_COUNT}種）を生成中...")
    create_sample_thumbnails(cache_dir, sample_hashes)

    conn = sqlite3.connect(str(db_path))
    cur = conn.cursor()

    # PRAGMA 設定
    cur.execute("PRAGMA foreign_keys = ON;")
    cur.execute("PRAGMA journal_mode = WAL;")
    cur.execute("PRAGMA synchronous = NORMAL;")
    cur.execute("PRAGMA user_version = 1;")

    # テーブル初期化
    cur.executescript("""
    CREATE TABLE IF NOT EXISTS meta (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
    INSERT OR REPLACE INTO meta (key, value) VALUES ('thumb_spec_version', '1');
    INSERT OR REPLACE INTO meta (key, value) VALUES ('catalog_version', '2');

    CREATE TABLE IF NOT EXISTS watched_folders (
        id              INTEGER PRIMARY KEY AUTOINCREMENT,
        path            TEXT NOT NULL UNIQUE,
        added_at        INTEGER NOT NULL,
        last_scanned_at INTEGER
    );

    CREATE TABLE IF NOT EXISTS images (
        id              INTEGER PRIMARY KEY AUTOINCREMENT,
        folder_id       INTEGER NOT NULL REFERENCES watched_folders(id) ON DELETE CASCADE,
        file_path       TEXT NOT NULL UNIQUE,
        file_size       INTEGER NOT NULL,
        file_mtime      INTEGER NOT NULL,
        taken_at        INTEGER NOT NULL,
        taken_at_source INTEGER NOT NULL DEFAULT 0,
        width           INTEGER,
        height          INTEGER,
        orientation     INTEGER NOT NULL DEFAULT 1,
        format          TEXT,
        quick_hash      TEXT NOT NULL,
        thumb_status    INTEGER NOT NULL DEFAULT 0,
        indexed_at      INTEGER NOT NULL
    );

    CREATE INDEX IF NOT EXISTS idx_images_timeline ON images (taken_at DESC, id DESC);
    CREATE INDEX IF NOT EXISTS idx_images_folder   ON images (folder_id);
    CREATE INDEX IF NOT EXISTS idx_images_hash     ON images (quick_hash);
    CREATE INDEX IF NOT EXISTS idx_images_thumb_pending ON images (id) WHERE thumb_status = 0;
    """)

    # ダミー監視フォルダ登録
    dummy_folder_path = "C:\\DummyPhotos\\Seed"
    cur.execute("INSERT OR IGNORE INTO watched_folders (path, added_at, last_scanned_at) VALUES (?, ?, ?);",
                (dummy_folder_path, int(time.time()), int(time.time())))
    cur.execute("SELECT id FROM watched_folders WHERE path = ?;", (dummy_folder_path,))
    folder_id = cur.fetchone()[0]

    # 既存ダミーレコード削除
    cur.execute("DELETE FROM images WHERE folder_id = ?;", (folder_id,))
    conn.commit()

    print(f"ダミー画像 {TOTAL_RECORDS:,} 件を投入中...")
    start_time = time.time()

    # 過去3年分（約1,000日）に分散した撮影日時
    now = int(time.time())
    one_day = 86400
    total_days = 1000

    batch = []
    batch_size = 5000

    for i in range(TOTAL_RECORDS):
        day_offset = random.randint(0, total_days)
        time_in_day = random.randint(0, one_day - 1)
        taken_at = now - (day_offset * one_day) + time_in_day
        file_path = f"{dummy_folder_path}\\IMG_{i:06d}.jpg"
        file_size = random.randint(1_000_000, 8_000_000)
        file_mtime = taken_at
        width = 4000
        height = 3000
        orientation = 1
        format_name = "jpeg"
        quick_hash = sample_hashes[i % SAMPLE_HASH_COUNT]
        thumb_status = 1 # 生成済み扱い
        indexed_at = now

        batch.append((
            folder_id, file_path, file_size, file_mtime, taken_at,
            0, width, height, orientation, format_name, quick_hash,
            thumb_status, indexed_at
        ))

        if len(batch) >= batch_size:
            cur.executemany("""
            INSERT INTO images (
                folder_id, file_path, file_size, file_mtime, taken_at,
                taken_at_source, width, height, orientation, format,
                quick_hash, thumb_status, indexed_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """, batch)
            conn.commit()
            batch.clear()
            print(f"  {i + 1:,} 件投入完了...")

    if batch:
        cur.executemany("""
        INSERT INTO images (
            folder_id, file_path, file_size, file_mtime, taken_at,
            taken_at_source, width, height, orientation, format,
            quick_hash, thumb_status, indexed_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        """, batch)
        conn.commit()

    elapsed = time.time() - start_time
    print(f"投入完了！ 総件数: {TOTAL_RECORDS:,} 件, 所要時間: {elapsed:.2f}秒 ({TOTAL_RECORDS / elapsed:.0f} 件/秒)")

    # サマリの確認
    cur.execute("SELECT COUNT(*) FROM images;")
    count = cur.fetchone()[0]
    cur.execute("SELECT COUNT(DISTINCT strftime('%Y-%m-%d', taken_at, 'unixepoch')) FROM images;")
    days = cur.fetchone()[0]
    print(f"DB確認: 合計 {count:,} 枚, 日数: {days} 日")

    conn.close()

if __name__ == "__main__":
    main()
