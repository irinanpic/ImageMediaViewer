# 軽量クロスプラットフォーム画像カタログアプリ 要件定義・基本設計書 (v2)

---

## 0. この文書の使い方（AI駆動開発向けルール）

この文書は AI コーディングアシスタントに読ませて開発を進める前提で書かれている。AI・人間とも以下を守る。

1. **元画像への書き込み禁止。** 元ファイルの書き込み・移動・削除・リネームは一切実装しない。ファイル書き込みを許可するのは「カタログDB」と「キャッシュディレクトリ」のみ。
2. **1回の作業は1マイルストーン（§11）の範囲に限定する。** 完了時は各マイルストーンの DoD（完了条件）を実行して確認する。
3. **IPC の型は Rust 側を正とし、TypeScript 型は自動生成する**（`ts-rs`）。`src/types/generated/` は手編集しない。
4. **DBスキーマの変更はマイグレーション追加で行う**（`PRAGMA user_version`）。既存のマイグレーションを書き換えない。
5. **依存ライブラリは `cargo add` / `npm i` で最新版を入れ、公式ドキュメントで API を確認する。** 特に Tauri v2 は v1 と API・設定が大きく異なる（v1 の記述を流用しない）。
6. **スコープ外（§2.4）の機能は実装しない。** 迷ったら §13「未決事項」に追記して先に進まない。
7. §2.3 の性能数値を悪化させる変更（DOM数の増加、IPC回数の増加、無制限の並列化など）は入れない。

---

## 1. プロジェクト概要

* **目的:** かつての「Picasa」が持っていた「複数フォルダを跨いだ時系列・シームレスな高速画像閲覧体験」を現代の技術で再現する。
* **基本思想:**
  * 閲覧・整理に特化し、重厚な加工・編集機能は排除する。
  * フォルダ構造の物理配置と、アプリ上の論理表示（タイムライン）を分離する。
  * 低オーバーヘッド・省メモリを徹底する。
  * **カタログDBとサムネイルはすべて「再生成可能なキャッシュ」と位置づける。** 失っても元ファイルには影響せず、再走査で復元できる。

---

## 2. システム要件

### 2.1 対象プラットフォーム

| OS | WebView | 位置づけ |
| --- | --- | --- |
| Windows 10/11 | WebView2 | 主ターゲット |
| macOS 12+ | WKWebView | 主ターゲット |
| Linux (Ubuntu 22.04+) | WebKitGTK | ベストエフォート。スクロール性能は WebKitGTK 依存のため 60fps 目標の対象外 |

### 2.2 機能要件

優先度: **P0** = MVP必須 / **P1** = MVP後に早期対応 / **P2** = 将来

| ID | 優先度 | 機能名 | 詳細・仕様 |
| --- | --- | --- | --- |
| **F-01** | P0 | 監視フォルダ登録 | フォルダ選択ダイアログ（`tauri-plugin-dialog`）で任意ディレクトリ（複数可）を登録・解除。パスは正規化して保存（Windows は `\\?\` プレフィックス除去・ドライブレター大文字化）。**既登録フォルダの子を追加しようとした場合は拒否して通知。親を追加した場合は既存の子の登録を削除して親で再走査**（サムネは hash キーなので再利用される）。解除時は DB レコードのみ削除し、孤立したサムネイルは GC する。 |
| **F-02** | P0 | バックグラウンド再帰走査 | 拡張子ホワイトリスト（MVP: jpg/jpeg/png/webp/gif/bmp）で対象を抽出。隠しファイル/フォルダ（`.` 始まり、`$RECYCLE.BIN`、`@eaDir` 等）とシンボリックリンクはスキップ。**初回は全走査、2回目以降は (path, size, mtime) 比較の差分走査。** 消えたファイルのレコードは削除。ルートフォルダに到達できない場合（外付けHDD未接続等）は**レコードを削除せず「オフライン」扱い**とし、キャッシュ済みサムネでの閲覧は継続する。詳細は §6。 |
| **F-03** | P0 | Exif・メタデータ解析 | 撮影日時・解像度・Orientation・更新日時を取得。日時の取得優先順位と扱いは §5.1、Orientation は §5.2。解像度は**ヘッダのみ読む**（`imagesize`）。Exif がない・壊れている場合も登録は成功させる（mtime にフォールバック）。 |
| **F-04** | P0 | サムネイル非同期生成 | 仕様は §5.4、パイプラインは §7。画面内の画像を最優先で生成し、残りはバックグラウンドで生成。生成結果はディスクに永続化。 |
| **F-05** | P0 | タイムライン一覧表示 | フォルダを問わず全画像を「撮影日時（降順）」で単一タイムライン上に表示。**日付（日単位）ヘッダ付き・固定サイズ正方形セルのグリッド**を仮想スクロール（Virtualization）で描画。同時刻の画像は id 降順で順序を固定。詳細は §8。 |
| **F-06** | P0 | シングル画像表示 | サムネイルクリックで詳細ビューを開く。←/→ でタイムライン順に前後移動、Esc で閉じる。表示開始時は即座にサムネを出し、原寸の読込完了後に差し替える。前後1枚をプリロード。 |
| **F-07** | P0 | 一時的操作 (非破壊) | 詳細ビュー上で「回転（90度単位）」「拡大・縮小・パン」「左右/上下反転」を行う。状態は画像切替で既定リセット。元ファイルは変更しない。キー割当は §8.4。 |
| **F-08** | P0 | 進捗・状態表示 | ステータスバーに「走査中（n件）」「サムネイル生成 m / 全件」を表示。破損画像・欠損ファイルはプレースホルダ表示（クラッシュ・無限ローディングにしない）。 |
| **F-09** | P1 | 日付ジャンプ | タイムライン右端に年・月スクラバーを置き、10万枚規模でも目的の時期へ即移動できる（§9 の日別件数サマリを利用）。 |
| **F-10** | P1 | フォルダ絞り込み | サイドバーでフォルダを選ぶと、そのフォルダ配下のみのタイムラインを表示。 |
| **F-11** | P1 | ファイル監視 | `notify` で追加・削除・変更を自動反映。**ベストエフォート**とし、正は「起動時の差分走査＋手動再走査」。 |
| **F-12** | P1 | 情報パネル / エクスプローラで表示 | ファイル名・パス・撮影日時・解像度・サイズを表示。OSのファイルマネージャで該当ファイルを表示（`tauri-plugin-opener`）。 |
| **F-13** | P2 | グリッドサイズ変更 | サムネイルサイズのスライダ。 |
| **F-14** | P2 | 追加フォーマット | HEIC / AVIF / TIFF / RAW 等。WebView が直接表示できない形式は Rust 側で変換して配信する設計余地を残す。 |
| **F-15** | P2 | 重複検知 | 同一内容ファイルの検出・表示。 |

### 2.3 非機能要件

> 数値は**初期目標**。マイルストーン M0 の実測で見直し、確定値をこの表に反映する。

| 項目 | 要件仕様 | 計測条件 |
| --- | --- | --- |
| **スクロール** | 10万枚登録時、p95 フレーム時間 ≤ 16.7ms（60fps） | Windows/macOS。DevTools Performance で5秒間の連続スクロール＋スクロールバードラッグ。サムネはキャッシュ済み |
| **起動** | 10万枚カタログで、ウィンドウ表示から先頭画面のサムネ表示完了まで ≤ 1.0秒 | サムネ生成済み状態 |
| **初回走査** | メタデータ取り込み ≥ 1,000枚/秒 | ローカルSSD。ネットワーク/HDDは対象外（参考値） |
| **サムネ生成** | 12MP JPEG で ≥ 50枚/秒（8コア目安） | 参考値。10万枚で約30分 |
| **メモリ（Rustコア単体）** | アイドル ≤ 100MB / 高速スクロール時 ≤ 300MB | Rustプロセスのみ |
| **メモリ（全体）** | アイドル ≤ 300MB / 高速スクロール時 ≤ 600MB | **アプリ本体＋WebView子プロセスの合計**（Windows: msedgewebview2.exe 含む）。※WebView自体が常駐で数十〜百数十MBを占めるため、元案の「150MB/500MB」は全体合計では非現実的な可能性が高い。M0で実測する |
| **デコードのメモリ制御** | 同時デコードは**メモリ予算（既定256MB）内**。ピクセル数上限（既定 2億px）超の画像は生成失敗として扱う | デコード爆弾対策も兼ねる |
| **元ファイルの安全性** | 元ファイルは読み取り専用で開く。移動・書き換え・削除を絶対に行わない | 走査・サムネ生成の前後でフィクスチャのハッシュ・mtime が不変であることをテストで保証 |
| **耐障害性** | 壊れた画像・巨大画像・読み取り不能ファイルでアプリが落ちない（失敗は `thumb_status=2` として記録）。DB破損時は削除して再走査で復元可能 | 破損フィクスチャによるテスト |
| **保守性** | ドメインロジック（Rust）と描画層（React/TS）をIPC境界で完全分離。IPC型は自動生成 | — |
| **セキュリティ** | Tauri capabilities は最小権限。CSP を設定。カスタムプロトコルは**画像IDのみ受け付け、パス文字列を受け取らない**（任意ファイル読み取り防止）。ネットワーク通信なし | — |

### 2.4 スコープ外（MVPでやらないこと）

画像編集・加工、元ファイルの移動/リネーム/削除、タグ・評価・顔認識などの分類、クラウド同期・共有、動画、RAW/HEIC（P2）、複数ウィンドウ、プラグイン機構。

---

## 3. 技術スタック選定

### Backend (Rust 1.80+)

| 用途 | 採用 | 備考 |
| --- | --- | --- |
| フレームワーク | **Tauri v2** | プラグイン: `dialog`, `opener` |
| DB | `rusqlite`（feature: `bundled`） | OS差を避けるため SQLite を同梱ビルド |
| 走査 | `walkdir` + `rayon` | |
| Exif | `kamadak-exif` | クレート名 `kamadak-exif`、コード上は `exif` |
| 画像ヘッダ（寸法） | `imagesize` | デコードせずヘッダのみ読む |
| デコード | `image`（feature を jpeg/png/webp/gif/bmp に限定） | `image::Limits` でメモリ・寸法上限を設定 |
| リサイズ | `fast_image_resize` | `image` 標準のリサイズより高速 |
| WebPエンコード | `webp`（libwebp バインディング） | **`image` クレートの WebP エンコーダは lossless のみで品質指定不可**。「品質75」の lossy には `webp` クレートが必要 |
| ハッシュ | `blake3` | |
| 日時 | `chrono` | mtime のローカル時刻変換用 |
| 監視 | `notify` + `notify-debouncer-full` | |
| ログ/エラー | `tracing` / `thiserror` | |
| 型生成 | `ts-rs`（または `tauri-specta`） | Rust → TS 型の自動生成 |

> サムネイル生成の並列処理は **rayon ではなく、専用ワーカースレッド＋優先度付きキュー**で実装する（§7）。rayon は優先度制御ができないため、走査・Exif解析の並列化のみに使う。

### Frontend

TypeScript + React 18+ (Vite) / `@tanstack/react-virtual` / Tailwind CSS / Zustand / テスト: Vitest

---

## 4. データベース設計 (SQLite)

アプリのデータディレクトリ（Tauri の `app_data_dir()`）に `catalog.db` として保存する。

```sql
-- ★ 以下の PRAGMA は「接続ごと」に毎回設定する（特に foreign_keys）
PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA busy_timeout = 5000;
PRAGMA cache_size = -16384;      -- 16MiB

-- スキーマバージョンは PRAGMA user_version で管理し、マイグレーションを順次適用する

CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL            -- 例: thumb_spec_version = '1'
);

-- 監視対象ルートフォルダ
CREATE TABLE IF NOT EXISTS watched_folders (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    path            TEXT NOT NULL UNIQUE,   -- 正規化済みパス
    added_at        INTEGER NOT NULL,
    last_scanned_at INTEGER
);

-- 画像メタデータ
CREATE TABLE IF NOT EXISTS images (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    folder_id       INTEGER NOT NULL REFERENCES watched_folders(id) ON DELETE CASCADE,
    file_path       TEXT NOT NULL UNIQUE,   -- 元ファイルの絶対パス（正規化済み・UTF-8のみ）
    file_size       INTEGER NOT NULL,       -- バイト数
    file_mtime      INTEGER NOT NULL,       -- 最終更新日時 (UNIXエポック秒)
    taken_at        INTEGER NOT NULL,       -- 撮影日時（§5.1: 壁時計時刻をUTCとみなしたepoch秒）
    taken_at_source INTEGER NOT NULL DEFAULT 0, -- 0:Exif 1:mtime
    width           INTEGER,                -- Orientation 適用後の幅（§5.2）
    height          INTEGER,                -- Orientation 適用後の高さ
    orientation     INTEGER NOT NULL DEFAULT 1, -- Exif Orientation (1-8)
    format          TEXT,                   -- 'jpeg','png','webp','gif','bmp'
    quick_hash      TEXT NOT NULL,          -- §5.3 のサンプリングBLAKE3。サムネキャッシュのキー
    thumb_status    INTEGER NOT NULL DEFAULT 0, -- 0:未生成 1:生成済 2:失敗
    indexed_at      INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_images_timeline ON images (taken_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_images_folder   ON images (folder_id);
CREATE INDEX IF NOT EXISTS idx_images_hash     ON images (quick_hash);
-- 未生成サムネの取得用（部分インデックス。thumb_status 単体の索引は選択性が低く無駄）
CREATE INDEX IF NOT EXISTS idx_images_thumb_pending ON images (id) WHERE thumb_status = 0;

-- マイグレーション v3: タイムライン複合ソート最適化（filesort完全排除）
CREATE INDEX IF NOT EXISTS idx_images_folder_timeline ON images (folder_id, taken_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_images_filepath ON images (file_path ASC, id ASC);
CREATE INDEX IF NOT EXISTS idx_images_filepath_desc ON images (file_path DESC, id DESC);
```

### 主要クエリ

```sql
-- 範囲取得（ページング）。ORDER BY は必ず (taken_at DESC, id DESC)
SELECT id, taken_at, width, height, file_mtime
FROM images
ORDER BY taken_at DESC, id DESC
LIMIT ?1 OFFSET ?2;

-- 日別件数サマリ（レイアウト計算・日付ジャンプ用）
SELECT strftime('%Y-%m-%d', taken_at, 'unixepoch') AS day, COUNT(*) AS cnt
FROM images
GROUP BY day
ORDER BY day DESC;
```

* サマリは 10万件でも数十ms想定。バックエンドでメモリキャッシュし、`catalog_version` が変わった時だけ再計算する。

### 接続戦略

* **書き込み接続1本（Mutex）＋ 読み取り接続 2〜4本**。WAL により読み書きは並行できる。
* DBアクセスは Tauri コマンド内で `spawn_blocking`（または専用スレッド）に逃がし、async ランタイムをブロックしない。
* 走査の書き込みは 500〜1,000件単位のバッチ・1トランザクション。

---

## 5. 主要な設計判断

### 5.1 撮影日時（taken_at）の扱い

Exif の `DateTimeOriginal` は**タイムゾーン情報を持たないローカル時刻**。そのまま epoch に変換するとPCのTZ設定で日付境界がずれるため、次のルールにする。

* Exif の日時は「壁時計時刻」としてそのまま UTC とみなして epoch 秒化する。表示・日付グルーピングも UTC として整形する（OSのTZを再適用しない）。
* mtime へのフォールバック時は、OSのローカルTZで壁時計時刻に変換してから同じ形式に揃える（`taken_at_source = 1`）。
* 取得優先順位: `DateTimeOriginal` → `DateTimeDigitized` → `DateTime` → ファイル mtime。
* 不正値（`0000:00:00 00:00:00`、1900年より前・2100年より後など）は次の候補に進む。
* `OffsetTimeOriginal` は MVP では無視する。

### 5.2 Exif Orientation

* Orientation(1–8) を読み取り `orientation` に保存する。
* **サムネイルは回転適用済みで生成する。**
* **`width` / `height` は Orientation 適用後の値を保存する**（Orientation 5–8 は幅と高さを入れ替える）。
* 原寸表示では、WebView の `<img>` が既定で Exif Orientation を自動適用する（`image-orientation: from-image`）ため、フロントで追加補正しない（**二重回転に注意**）。F-07 の回転はその上に重ねる。3OSで挙動を M0/M5 で確認する。

### 5.3 quick_hash

* `BLAKE3( file_size(u64 LE) ‖ 先頭64KiB ‖ 中央64KiB ‖ 末尾64KiB )`（64KiB×3 未満のファイルは全体）。
* 用途は**サムネイルキャッシュのキー**（ファイルを移動・コピーしてもキャッシュを再利用できる）。
* 全体ハッシュではないため「内容の同一性の保証」には使わない（重複検知 F-15 では別途精査が必要）。
* 同一パスの内容変更は `(size, mtime)` の差分で検知し、ハッシュとサムネを再生成する。
* 元案の「先頭ブロックのみ」はヘッダ（Exif）が同一な別画像で衝突しやすいため、サンプリング方式に変更。

### 5.4 サムネイル仕様（spec version = 1）

| 項目 | 仕様 |
| --- | --- |
| サイズ | 短辺 256px（拡大はしない）。**ただし長辺は最大 512px に制限**（パノラマ等で 256×2560 のような巨大サムネが出るのを防ぐ） |
| 向き | Orientation 適用済み |
| 形式 | lossy WebP、品質75（`webp` クレート）。アルファは保持 |
| アニメGIF | 先頭フレームのみ |
| 保存先 | `{app_cache_dir}/thumbs/v1/{hash[0..2]}/{hash}.webp`（`app_cache_dir()` を使用。`~/.cache` 固定にしない）。**`.tmp` に書いて rename でアトミックに配置** |
| 容量見積 | 1枚 約8〜20KB → 10万枚で 1〜2GB 程度（要実測） |
| 仕様変更時 | spec version を上げ、`thumbs/v2/` に切り替え、`thumb_status` を一括リセット、旧ディレクトリは GC |

> 表示セルが 160 CSS px の場合、DPR 1.5 で約240px。256px はこの前後を想定した値。グリッドサイズ変更（F-13）を入れる際は再検討する。

### 5.5 画像配信：カスタム URI スキーム

元案の `asset://`（Tauri v1 系の発想）は、v2 では `assetProtocol` の有効化とスコープ設定が必要で、パスをフロントに晒す点も好ましくない。**カスタムURIスキームで ID ベースの配信**に変更する。

| スキーム | 内容 |
| --- | --- |
| `thumb://localhost/{imageId}?v={rev}` | サムネWebPを返す。キャッシュがあればファイルから返す。**未生成なら最優先でキューに積み、生成完了後に応答する**（非同期プロトコル）。失敗時は 404 |
| `original://localhost/{imageId}` | 原寸画像を返す（MVPは WebView が直接表示できる形式のみ、バイト列をそのまま返す。MIME は `format` から決定） |

* Rust 側は `register_asynchronous_uri_scheme_protocol` で登録する。
* ID → パスの解決は DB で行う。**URL にパスを含めない。**
* Windows では実URLが `http://thumb.localhost/{id}` 形式になるため、フロントは `convertFileSrc(String(id), 'thumb')`（`@tauri-apps/api/core`）で URL を生成し、OS差を吸収する。
* `tauri.conf.json` の CSP に `img-src 'self' thumb: http://thumb.localhost original: http://original.localhost data: blob:` を設定する。
* `?v={rev}`（= `file_mtime`）はキャッシュバスター。ファイル変更時に WebView のキャッシュを無効化する。

---

## 6. インデックス（走査）パイプライン

```
[走査 (walkdir)] ─ 拡張子フィルタ ─> (path,size,mtime) を DB と突き合わせ
        │                              ├ 新規 / 変更 → 次工程へ
        │                              ├ 変化なし   → スキップ
        │                              └ DBにあるが実在しない → 削除候補
        ▼
[rayon 並列] ヘッダ読み（Exif・imagesize）＋ quick_hash
        ▼
[書き込み] 500〜1,000件ごとにバッチ UPSERT (ON CONFLICT(file_path) DO UPDATE)
        ▼
 catalog-changed イベント（スロットル 500ms〜1s）→ UI へ順次反映
        ▼
[全走査完了後] 削除候補のレコード削除 → thumb_status=0 をサムネキューへ（低優先）
```

* 進捗は `scan-progress` イベントで通知。フォルダ解除・終了時にキャンセル可能にする。
* 走査並列度は `min(4, コア数)` を既定とし、設定可能にする。
* 初回走査中でも UI は即座に使える（バッチごとにタイムラインへ反映）。

### エッジケース

| ケース | 方針 |
| --- | --- |
| 壊れた/途中で切れた画像 | 登録は行い、サムネ生成失敗で `thumb_status=2`。タイムラインにはプレースホルダ |
| 非UTF-8のファイル名 | MVPでは無視してログに記録 |
| 外付けHDD・ネットワークドライブ未接続 | ルートが到達不能ならレコードを削除せず「オフライン」。詳細表示時は「元ファイルが見つかりません」 |
| コピー/取り込み中のファイル | mtime が直近 数秒以内のものは今回スキップし、次回走査・watcher で拾う（デバウンス 2秒） |
| 同じファイルが複数経路から到達可能 | `file_path UNIQUE` ＋ UPSERT で重複登録しない |
| 異常な巨大画像 | `image::Limits` とピクセル数上限で失敗扱い |
| 内容が同一の別ファイル | MVPでは両方表示（重複排除は F-15） |
| inotify 上限（Linux）/ ネットワークドライブ | watcher 失敗時は警告して手動再走査にフォールバック |

---

## 7. サムネイルパイプライン

```
[走査スレッド] ──> SQLite に登録 (thumb_status: 0)
                          │
        ┌─────────────────┴──────────────────────────┐
        │ ① thumb:// リクエスト（画面内）→ 優先度 HIGH │
        │ ② prefetch コマンド（画面外近傍）→ 優先度 MID│
        │ ③ 未生成レコード全件 → 優先度 LOW           │
        └─────────────────┬──────────────────────────┘
                          ▼
         [優先度付きキュー  Mutex<BinaryHeap<Job>> + Condvar]
          ・同一 image id のジョブは重複排除（in-flight マップに待機者を追加）
                          ▼
         [ワーカー std::thread × N  (既定 N = min(コア数-1, 4))]
          ① メモリ予算セマフォを取得（見積 = 幅×高さ×4バイト）
          ② 原寸デコード（image::Limits 適用）
          ③ Orientation 適用 → fast_image_resize で縮小
          ④ lossy WebP エンコード
          ⑤ .tmp に書き出し → rename → 応答（待機中の thumb:// リクエストへ）
          ⑥ thumb_status を更新（100件 or 200ms 単位でバッチ UPDATE）
```

* 24MP の画像は RGBA デコードで約96MBになる。**同時デコード数ではなく「メモリ予算」で制御**し、メモリ目標を守る。
* 失敗は `thumb_status=2` として記録し、ログに理由を出す。再試行は「size/mtime が変わった時」または手動「再生成」のみ。
* `thumb-progress`（完了数/全体）イベントをスロットルして通知する。
* P1 以降: ワーカーのスレッド優先度を下げ、UI の応答性を確保する。
* **HTTP配信とインメモリハイブリッド最適化**:
  * **固定スレッドプール**: Windows OS のスレッド生成コストを完全排除するため、HTTP サーバーは 12 スレッド（CPUコア数×2）の固定ワーカースレッドプールで並行処理。
  * **インメモリ・メタデータ解決 (`ThumbLookupCache`)**: サムネイル配信時の画像ID→ハッシュ解決をメモリ上の `RwLock<HashMap<i64, ThumbSource>>` で行い、毎回の SQLite クエリを完全にバイパス（0.0001ms）。
  * **インメモリ・サムネイルLRUバイナリキャッシュ (`ThumbBinaryLruCache`)**: 直近生成・取得された最大 2,048 枚（約60〜80MB）の WebP バイナリを RAM に保持し、ディスク I/O ゼロで即時応答。
* 最適化候補（M3 の PoC 後に判断）: JPEG は縮小デコード（DCTスケーリング。例: `jpeg-decoder` の `scale` や libjpeg-turbo）で大幅に高速化できる可能性がある。

---

## 8. フロントエンド設計

### 8.1 タイムラインのレイアウト

* **固定サイズの正方形セル**（既定 160 CSS px、gap 4px）を `object-fit: cover` で表示する。
  * 列数 = `floor((幅 + gap) / (セル + gap))`
* **全レコードを取得しなくてもスクロール全長が確定する**よう、`getTimelineSummary` の日別件数からレイアウトを計算する。

```ts
type Row =
  | { kind: 'header'; day: string; count: number; height: number }
  | { kind: 'cells';  startIndex: number; count: number; height: number }; // startIndex は全体通し番号

function buildRows(buckets: DayBucket[], columns: number): Row[] { /* 純関数 */ }
```

* `useVirtualizer` の `count = rows.length`、`estimateSize` は行高さ（固定なので正確）、overscan は 3〜5行。
* `buildRows` は **純関数としてユニットテスト**する（列数変更・空日・末尾の端数行など）。再計算は列数変更・サマリ更新時のみ（日数は高々数千のため軽量）。

### 8.2 データ取得

* ページ単位（200件）で `getTimelineImages({offset, limit})` を呼ぶ。表示行 → 通し番号範囲 → 必要ページを算出し、重複リクエストを排除する。
* フロントのページキャッシュは LRU（最大 約50ページ ≒ 1万件）。未取得セルはスケルトン表示。
* `catalog-changed`（`version` 付き）を受けたらキャッシュを破棄して再取得する。MVP ではスクロール位置は先頭可視行の通し番号で維持する（新規挿入によるズレは許容。P1 で imageId アンカー化）。

### 8.3 スクロール性能とリフレッシュのルール

* DOM に存在するセルは「可視＋overscan」のみ（目安: 200以下）。デコード済みビットマップは 1枚 約350KB（256×341×4）なので、200枚で約70MB。**これ以上増やさない**。
* セルは `React.memo`、`<img>` は `decoding="async"` `draggable={false}`。
* **スクロール速度が閾値（1,500px/s）を超えている間や急激な移動中は `<img src>` を新規にセットしない**（スケルトン表示）。停止後 約100ms 待ってから読込を開始する。これでサムネ要求の洪水とブラウザのHTTP接続スロット詰まり・ワーカーの無駄な生成を防ぐ。
* **世代管理ビューポート連携**: 大スクロール直後、ページデータ取得完了（`dataVersion` 更新）と連動して画面内セルID（High）と周辺（Mid）を即時にバックエンドへ通知し、キューを現画面最優先で仕切り直す。
* **現画面再読込（リフレッシュ）機能**:
  * タイムアウトや高負荷で「読込不可」となった場合でも、画面位置（スクロール位置）を変えずに現画面の画像群を再読込可能。
  * ツールバー上の「再読込（🔄）」ボタン、またはタイムライン上の `R` / `F5` キーで起動。
  * 実行時は直ちに `setViewport` を強制送信してキューを一掃・最優先化し、各セルのエラー状態をリセットしてキャッシュバイパスクエリ（`?rf=...`）付きで再取得する。
  * 「読込不可」となったセル単体をクリックすることでも個別リトライが可能。
  * 自動リトライは 250ms, 600ms, 1200ms, 2400ms の最大4回粘り強く試行する。
* 画面外に出たセルは unmount してビットマップを解放させる。
* スクロール位置など高頻度で変わる値は Zustand に入れない（ref / virtualizer 内部で完結させる）。
* ホバー等でレイアウトを動かさない。`will-change` を濫用しない。

### 8.4 ビューア

| 操作 | キー/入力 |
| --- | --- |
| 前/次の画像（タイムライン順） | ← / → |
| 閉じる | Esc |
| 右回転 / 左回転 | R / Shift+R |
| 左右反転 / 上下反転 | H / V |
| ズーム | ホイール（ポインタ中心）、+ / − |
| フィット表示 / 等倍 | 0 / 1、ダブルクリックで切替 |
| パン | ドラッグ |
| 情報パネル (P1) | I |

* 変換は単一要素の CSS transform（translate / scale / rotate / flip）で合成し、GPU で処理する。
* 状態: `{ rotation: 0|90|180|270, flipH, flipV, zoom, panX, panY }`。画像切替でリセット。
* **フィット計算は回転後の寸法で行う**（90°/270° は幅と高さを入れ替える）。
* 原寸が極端に大きい場合（例: 50MP ≒ デコード後200MB）は WebView のメモリを圧迫する。MVPは許容し、P2 で縮小プレビュー配信を検討する。

### 8.5 状態管理（Zustand スライス）

`catalog`（total, buckets, version）/ `viewer`（現在 index、変換状態）/ `scan`（走査・サムネ進捗）/ `ui`（セルサイズ等）

---

## 9. IPC (Tauri コマンド / イベント) インターフェース定義

**Rust 側の構造体を正とし、`ts-rs` で TS 型を生成する**（`#[serde(rename_all = "camelCase")]` を付与）。以下は生成後のイメージ。

```typescript
// src/types/generated/ （自動生成。手編集禁止）

export interface ImageRecord {
  id: number;
  takenAt: number;          // 壁時計時刻をUTCとみなしたUNIX秒 (§5.1)
  width: number | null;     // Orientation 適用後
  height: number | null;
  rev: number;              // file_mtime。thumb URL のキャッシュバスター
}
// サムネURLはフロントで convertFileSrc(String(id), 'thumb') + `?v=${rev}` から生成する

export interface ImageDetail extends ImageRecord {
  folderId: number;
  filePath: string;
  fileSize: number;
  format: string | null;
  takenAtSource: 'exif' | 'mtime';
  originalAvailable: boolean;   // 元ファイルが存在するか
}

export interface DayBucket { day: string; count: number }  // 'YYYY-MM-DD'、降順

export interface TimelineSummary {
  total: number;
  buckets: DayBucket[];
  version: number;          // catalog_version
}

export interface WatchedFolder {
  id: number;
  path: string;
  imageCount: number;
  status: 'online' | 'offline' | 'scanning';
}

export interface GetImagesPayload {
  offset: number;
  limit: number;            // 上限 500
  folderId?: number;        // F-10
}

export interface ScanProgress {
  folderId: number;
  phase: 'walking' | 'indexing' | 'cleanup' | 'done';
  discovered: number;
  processed: number;
}

export interface AppError {
  code: 'NotFound' | 'Io' | 'Db' | 'InvalidArgument' | 'Cancelled' | 'Internal';
  message: string;
}
```

```typescript
// 呼び出しAPI（すべて Promise<T> で、失敗時は AppError を reject）
export interface BackendAPI {
  addWatchFolder(path: string): Promise<WatchedFolder>;
  removeWatchFolder(id: number): Promise<void>;
  getWatchFolders(): Promise<WatchedFolder[]>;
  rescan(folderId?: number): Promise<void>;
  getTimelineSummary(opts?: { folderId?: number }): Promise<TimelineSummary>;
  getTimelineImages(payload: GetImagesPayload): Promise<ImageRecord[]>;
  getImageDetail(id: number): Promise<ImageDetail>;
  prefetchThumbnails(ids: number[]): Promise<void>;   // 優先度 MID でキューへ
  revealInFileManager(id: number): Promise<void>;     // P1
}

// バックエンド → フロントエンド イベント
//   'scan-progress'   : ScanProgress
//   'thumb-progress'  : { done: number; total: number }
//   'catalog-changed' : { version: number }   // スロットル済み
```

元案からの変更点: `getImageCount` は `getTimelineSummary.total` に統合 / `thumbPath` は廃止（ID から URL を組み立て、IPCペイロードも削減）/ `year`・`month` 絞り込みは日別サマリ＋日付ジャンプに置換 / `getWatchFolders` は件数・状態付きのオブジェクトを返す / エラー型・イベントを追加。

---

## 10. プロジェクト構成案（ディレクトリ構造）

```text
├── docs/
│   ├── spec.md                   # 本書
│   └── perf.md                   # M0 以降の実測記録
├── src/                          # フロントエンド (React / TypeScript)
│   ├── components/
│   │   ├── TimelineGrid/
│   │   │   ├── VirtualTimeline.tsx
│   │   │   ├── GridCell.tsx
│   │   │   └── DateHeader.tsx
│   │   ├── Viewer/
│   │   │   ├── ImageViewerModal.tsx
│   │   │   └── useViewerGestures.ts
│   │   ├── Sidebar/              # フォルダ一覧
│   │   └── StatusBar/            # 走査・サムネ進捗
│   ├── hooks/
│   │   ├── useTimelineLayout.ts  # buildRows を呼ぶ
│   │   ├── usePagedImages.ts     # ページ取得・LRU
│   │   ├── useScrollVelocity.ts
│   │   └── useBackendEvents.ts
│   ├── lib/
│   │   ├── ipc.ts                # invoke ラッパー（型付き・エラー整形）
│   │   ├── thumbUrl.ts           # convertFileSrc ラッパー
│   │   └── buildRows.ts          # 純関数（テスト対象）
│   ├── store/                    # Zustand スライス
│   ├── types/generated/          # ts-rs 出力（手編集禁止）
│   ├── App.tsx
│   └── main.tsx
├── src-tauri/                    # バックエンド (Rust / Tauri v2)
│   ├── Cargo.toml
│   ├── tauri.conf.json           # CSP・カスタムプロトコル用設定
│   ├── capabilities/
│   │   └── default.json          # 最小権限（dialog 等のみ）
│   ├── tests/fixtures/           # 検証用画像（Orientation 1-8、Exifなし、破損、超横長、アルファ付き 等）
│   └── src/
│       ├── main.rs
│       ├── lib.rs                # Tauri v2 のエントリ（run()）
│       ├── state.rs              # AppState（DB接続・キュー・設定）
│       ├── error.rs              # AppError（thiserror, serde 対応）
│       ├── commands.rs           # IPC ハンドラ
│       ├── protocol.rs           # thumb:// / original:// ハンドラ
│       ├── db/
│       │   ├── mod.rs            # 接続管理・PRAGMA
│       │   ├── migrations.rs     # user_version ベースの移行
│       │   └── repo.rs           # クエリ集約
│       ├── scanner/
│       │   ├── mod.rs
│       │   ├── walker.rs
│       │   └── watcher.rs        # P1
│       └── pipeline/
│           ├── mod.rs
│           ├── exif.rs           # 日時・Orientation 解析
│           ├── hash.rs           # quick_hash
│           ├── queue.rs          # 優先度付きキュー・メモリ予算
│           └── thumbnail.rs
├── tools/
│   └── seed-catalog/             # 開発用：ダミー10万件をDBに投入するツール（§12）
└── README.md
```

---

## 11. 開発マイルストーン（DoD付き）

| # | 内容 | 完了条件 (DoD) |
| --- | --- | --- |
| **M0** | **PoC・計測（Spike）**: Tauri v2 雛形＋ダミー10万件の仮想スクロール＋`thumb://` で固定画像を返す最小実装 | 各OSで実測し `docs/perf.md` に記録。スクロールのフレーム時間とメモリ（Rust単体／全体合計）を確認し、§2.3 の目標値を必要なら改訂 |
| **M1** | 基盤: DB（接続・PRAGMA・マイグレーション・スキーマ）、`AppError`、`ts-rs` 型生成、capabilities、ログ | `cargo test` で DB テストが通る。TS 型が生成され、フロントから `getWatchFolders` を呼べる |
| **M2** | 走査・メタデータ（F-01〜F-03）: フォルダ追加→走査→DB登録、差分再走査、削除検知、オフライン判定 | 1,000枚のフォルダで再走査時に「処理0件」になる。Exifなし・破損画像を含むフィクスチャでパニックしない。Orientation 付き画像の width/height が適用後の値になっている |
| **M3** | サムネイル（F-04）: 優先度キュー、メモリ予算、`thumb://`、アトミック書き込み | 1,000枚の生成完了。再起動後に再生成されない。破損画像は `thumb_status=2`。ピーク時メモリが予算内 |
| **M4** | タイムライン（F-05, F-08）: サマリ・ページング・日付ヘッダ・仮想スクロール・速度連動の読込抑制 | seed 10万件で §2.3 のスクロール/メモリ目標を満たす（満たせない場合は原因を `docs/perf.md` に記録） |
| **M5** | ビューア（F-06, F-07）: 前後移動・ズーム/パン・回転/反転・プリロード | Orientation 付き画像が正しい向きで表示され、回転/反転が二重適用にならない。3OS で確認 |
| **M6** | 仕上げ: エラー/空状態表示、watcher（F-11）、日付ジャンプ（F-09）、フォルダ絞り込み（F-10）、情報パネル（F-12）、配布ビルド | 通しの手動テストシナリオ（フォルダ追加→閲覧→削除→再起動）が通る |

---

## 12. テスト・計測方針

* **Rust ユニットテスト:** Exif 日時パース（不正値・フォールバック順）、Orientation による幅高さの入替、`quick_hash` の安定性、マイグレーション、サマリ SQL。`tests/fixtures/` に Orientation 1–8、Exifなし、破損JPEG、超横長、アルファ付きPNG を用意する（巨大画像はテスト時に自動生成）。
* **元ファイル不変テスト:** 走査・サムネ生成の前後でフィクスチャのハッシュと mtime が変わらないことを確認する。
* **フロントのテスト（Vitest）:** `buildRows` の境界条件。
* **性能テスト用ダミーデータ:** `tools/seed-catalog` で DB に 10万件を投入する。**quick_hash を数十種に使い回し、対応する実サムネを数十枚だけ用意する**ことで、実画像なしでも「10万枚のタイムライン」を再現できる。
* **計測手順:** スクロールは DevTools Performance（5秒間）、メモリは OS のタスクマネージャ/アクティビティモニタで**本体＋WebView子プロセスの合計**を記録し、`docs/perf.md` に日付・OS・件数とともに残す。

---

## 13. リスクと未決事項

| 項目 | 内容 |
| --- | --- |
| **メモリ目標の達成可否** | WebView 常駐分を含めた全体合計で目標を満たせるか不明。M0 で確定する |
| **Linux (WebKitGTK)** | スクロール性能が他OSより劣る可能性。MVPではベストエフォート |
| **HEIC 対応の要否** | iPhone 写真が主体なら P2 から引き上げる必要がある（libheif のビルド・ライセンス、WebView 非対応のため変換配信が必要） |
| **開発主OS・配布対象OS** | 未決。M0 前に確定する |
| **F-07 の回転状態の永続化** | 現仕様は「閉じたらリセット」。永続化が必要なら DB 拡張が要る |
| **重複ファイルの扱い** | MVPは両方表示。F-15 で再検討 |
| **P1機能の取捨選択** | 日付ジャンプ・フォルダ絞り込みなどの優先順位 |
