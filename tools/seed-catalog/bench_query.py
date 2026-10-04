#!/usr/bin/env python3
"""
10万件データに対するDBクエリ性能計測スクリプト
"""
import sqlite3
import time
import os
from pathlib import Path

def get_db_path():
    appdata = os.environ.get("APPDATA")
    return Path(appdata) / "com.imagemediaviewer.app" / "catalog.db"

def main():
    db_path = get_db_path()
    conn = sqlite3.connect(str(db_path))
    cur = conn.cursor()

    cur.execute("PRAGMA journal_mode = WAL;")
    cur.execute("PRAGMA cache_size = -16384;")

    # 1. 総件数カウント
    t0 = time.perf_counter()
    cur.execute("SELECT COUNT(*) FROM images;")
    total = cur.fetchone()[0]
    t_count = (time.perf_counter() - t0) * 1000

    # 2. 日別サマリ計算 (getTimelineSummary)
    t0 = time.perf_counter()
    cur.execute("""
    SELECT strftime('%Y-%m-%d', taken_at, 'unixepoch') AS day, COUNT(*) AS cnt
    FROM images
    GROUP BY day
    ORDER BY day DESC;
    """)
    buckets = cur.fetchall()
    t_summary = (time.perf_counter() - t0) * 1000

    # 3. ページング取得 (200件) (getTimelineImages)
    t0 = time.perf_counter()
    cur.execute("""
    SELECT id, taken_at, width, height, file_mtime
    FROM images
    ORDER BY taken_at DESC, id DESC
    LIMIT 200 OFFSET 50000;
    """)
    page = cur.fetchall()
    t_page = (time.perf_counter() - t0) * 1000

    print(f"--- 10万件 DB性能計測結果 ---")
    print(f"総件数: {total:,} 件")
    print(f"総件数カウント: {t_count:.2f} ms")
    print(f"日別サマリ計算 ({len(buckets)}日分): {t_summary:.2f} ms")
    print(f"中央ページング取得 (OFFSET 50,000, LIMIT 200): {t_page:.2f} ms")

if __name__ == "__main__":
    main()
