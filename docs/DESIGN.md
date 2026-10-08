# ImageMediaViewer システム設計書 (Architecture & Design)

本書は、**ImageMediaViewer** の内部構造、データ設計、コンポーネント構成、高速化アルゴリズム、およびインターフェース仕様を記述したシステム設計書です。

---

## 1. システム全体アーキテクチャ

ImageMediaViewer は、**フロントエンド（React / TypeScript / Tailwind CSS）** と **バックエンド（Rust 高速ネイティブエンジン）** が同一プロセス内で完全一体化されたクロスプラットフォーム・デスクトップ/モバイルアプリケーションです。TCP/IP ポートを一切使用せず、Tauri Native IPC および内部カスタムプロトコル（`thumb://`, `original://`）によって高速・安全に連携します。

プラットフォーム固有の機能（タスクトレイ常駐制御、二重起動防止プラグイン等）は `#[cfg(desktop)]` によりデスクトップ限定に抽象化・分離されており、Windows 版の既存動作を 100% 維持したまま Android / ChromeOS 等へのクロスプラットフォーム展開が可能なアーキテクチャを採用しています。


```mermaid
flowchart TD
    subgraph App ["完全ポートレス一体型デスクトップアプリ (image-media-viewer.exe)"]
        subgraph UI ["ユーザーインターフェース (WebView2 / React)"]
            Timeline["仮想タイムライン (VirtualTimeline)"]
            Scrubber["高速スクロールバー (TimelineScrubber)"]
            Viewer["詳細画像ビューア (ImageViewerModal)"]
            Board["ムードボード (MoodboardCanvas)"]
            Sidebar["監視フォルダ管理 (Sidebar)"]
            Store["グローバル状態 (Zustand)"]
        end

        subgraph Transport ["メモリ内プロトコル & IPC (ポートレス)"]
            IPC["Native IPC (invoke commands)"]
            ThumbProtocol["カスタムプロトコル (thumb://)"]
            OrigProtocol["カスタムプロトコル (original://)"]
        end

        subgraph Core ["ネイティブエンジン (Rust)"]
            SingleInstance["二重起動防止 (tauri-plugin-single-instance)"]
            Tray["タスクトレイ常駐制御 (TrayIcon / Settings)"]
            Pipeline["サムネイルパイプライン (ThumbnailPipeline)"]
            WorkerPool["ワーカースレッド群 (CPU並列)"]
            Watcher["アイドルウォッチャー (IdleWatcher)"]
            Scanner["ディレクトリ走査 (Fast Walker)"]
        end

        subgraph Storage ["データ・ストレージ"]
            DB[("カタログDB (SQLite WALモード)\n%APPDATA%/com.imagemediaviewer.desktop/catalog.db")]
            ThumbDB[("サムネイル専用DB (SQLite WALモード WITHOUT ROWID)\n%LOCALAPPDATA%/com.imagemediaviewer.desktop/thumbnails.db")]
            OriginalFiles[("元画像ファイル (読み取り専用・完全保護)\nユーザー指定ディレクトリ")]
            SettingsFile[("アプリ設定 (app_settings.json)\n%APPDATA%/com.imagemediaviewer.desktop/app_settings.json")]
        end
    end

    Timeline --> Store
    Scrubber --> Store
    Viewer --> Store
    Board --> Store
    Sidebar --> Store

    Store --> IPC
    Timeline -. "サムネイル要求" .-> ThumbProtocol
    Viewer -. "原寸画像要求" .-> OrigProtocol

    IPC --> Core
    ThumbProtocol --> Core
    OrigProtocol --> Core

    Core --> DB
    Core --> ThumbDB
    Core --> SettingsFile
    ThumbProtocol --> ThumbDB
    OrigProtocol --> OriginalFiles
    Pipeline --> WorkerPool
    WorkerPool --> OriginalFiles
    WorkerPool --> ThumbDB
    WorkerPool --> DB
    Watcher --> Scanner
    Scanner --> DB
```

---

## 2. フロントエンド設計 (Frontend Architecture)

### 2.1 コンポーネントツリー
```text
src/
├── App.tsx                     # メインシェル、ビュー切替 (Timeline / Board)
├── components/
│   ├── Sidebar/
│   │   └── Sidebar.tsx         # 登録フォルダ一覧、ドラッグ並替、進捗バッジ、再作成ボタン
│   ├── StatusBar/
│   │   └── StatusBar.tsx       # 全体進捗バー、走査ステータス、生成中インジケータ
│   ├── TimelineGrid/
│   │   ├── VirtualTimeline.tsx # @tanstack/react-virtual による仮想化タイムライン
│   │   ├── TimelineToolbar.tsx # 検索、ソート順、グリッドサイズ、しおりボタン
│   │   ├── TimelineScrubber.tsx# Picasaスタイル縦型スクラブバー（年月・枚数バッジ追従）
│   │   ├── BookmarkPopover.tsx # しおり保存・一覧ポップオーバー
│   │   ├── GridCell.tsx        # 各画像セル、多層リトライ、スケルトン表示
│   │   └── DateHeader.tsx      # 日付（日別）ヘッダーブロック
│   ├── Viewer/
│   │   ├── ImageViewerModal.tsx# 原寸画像表示モーダル、回転・反転・パン・ズーム
│   │   └── useViewerGestures.ts# マウス・タッチジェスチャー制御
│   ├── Moodboard/
│   │   ├── MoodboardCanvas.tsx # 無限キャンバス、アイテム/メモ自由配置、クリッピング
│   │   └── AddToBoardModal.tsx # タイムライン画像追加モーダル
│   └── Common/
│       ├── ImageDetailPanel.tsx# 共通画像詳細パネル（タイムライン/ムードボード共用）
│       └── LogViewerModal.tsx  # システムログ & サムネイル失敗診断モーダル
├── hooks/
│   ├── usePagedImages.ts       # ページ単位の画像取得・キャッシュ・世代同期
│   ├── useTimelineLayout.ts    # ウィンドウ幅連動の動的列数追従 (4, 6, 8, 12列)
│   ├── useScrollVelocity.ts    # スクロール速度検知 (800px/s 閾値パージ)
│   ├── useViewportPrioritizer.ts# 現画面の画像IDをバックエンドへ優先通知
│   ├── useBackendEvents.ts     # 進捗・カタログ変更イベントのポーリング・購読
│   └── useWindowState.ts       # ハートビート送信、終了検知、ウィンドウ状態復元
└── utils/
    └── imageMetadata.ts        # フォーマット解析、可逆/非可逆判定、生データ比・圧縮率計算
```

### 2.2 仮想スクロールと動的列レイアウト設計
* **ウィンドウ幅連動の列数自動計算**:
  * 画面幅 `< 768px`: 4列
  * `768px 〜 1280px`: 6列
  * `1280px 〜 1920px`: 8列
  * `> 1920px`: 12列
* **行データ構造 (`buildRows.ts`)**:
  * タイムラインは「日付ヘッダー行」と「画像サムネイル行」の2種類の行（Row）から構成。
  * 日別バケット情報から各日の画像枚数を列数で割って行数を算出し、フラットな仮想アイテム配列を生成。10万件であっても初期描画に必要なデータは数KBのバケット配列のみ。

### 2.3 キャッシュと世代同期 (`usePagedImages.ts`)
* 画面に表示される行のインデックスから、必要なページ（1ページ=50件）を動的に要求。
* インメモリキャッシュ（`cacheRef`）により、一度取得したページは再マウント時でも 0ms で描画。
* スクロールにより世代番号が進んだ場合でも、受信したページデータは安全にキャッシュに格納され、無条件で `setVersionTick` を発行してスケルトン固まりを防止。

### 2.4 しおり（ブックマーク）位置高精度復元アルゴリズム (`VirtualTimeline.tsx`, `BookmarkPopover.tsx`)
* **ビューポート左上画像アンカー (`firstVisibleItem` / `imageIndex`)**:
  * 仮想スクロール（`overscan`）が存在する場合、仮想要素リストの先頭アイテムは画面外（上部バッファ）に位置するため、そのまま保存すると復元時に画面が下にずれる課題を解消。
  * `containerRef.current.scrollTop` と各仮想アイテムの `start` / `end` 座標を交差判定し、**ビューポート最上端に現実に表示されている行**（`firstVisibleItem`）を特定。
  * ヘッダー行であれば日付（`day`）、画像セル行であればその行の先頭画像通し番号（`imageIndex = item.startIndex`）をしおりデータに永続化。
* **動的行逆引きスクロール (`scrollToIndex`)**:
  * ウィンドウリサイズやズーム倍率の変更によりグリッド列数（4, 6, 8, 12列）が変わると、同一行番号に同じ画像が存在しなくなる問題に対応。
  * スクロール復元時、現在の `rows` 配列から記録された `targetScroll.imageIndex` を含む行（`r.kind === "cells" && targetImgIdx >= r.startIndex && targetImgIdx < r.startIndex + r.count`）を動的に逆引き検索。
  * 特定された現在行インデックスに対し `virtualizer.scrollToIndex(foundRowIdx, { align: "start" })` を実行することで、列数やズームが異なっていても常に目的の画像行が画面最上端へ高精度に復元。

### 2.5 タイムライン複数選択とラバーバンドドラッグ選択設計 (`VirtualTimeline.tsx`, `GridCell.tsx`)
* **グローバル選択状態管理 (`store/index.ts`)**:
  * `selectedImageIds: number[]` を Zustand ストアで一元管理。
  * `toggleSelectImageId(id)`: 個別選択のトグル。
  * `setSelectedImageIds(ids)`: 範囲選択や全選択時の一括更新。
  * `clearSelectedImageIds()`: 一括選択解除。
* **仮想スクロール環境下での AABB 矩形交差判定**:
  * グリッド背景のドラッグ開始時（ドラッグ距離 > 6px）にラバーバンド選択モードへ移行。
  * ドラッグ開始点 $(x_1, y_1)$ と現在点 $(x_2, y_2)$ から選択矩形 $B = [\min(x_1, x_2), \max(x_1, x_2)] \times [\min(y_1, y_2), \max(y_1, y_2)]$ を算出。
  * 画面内に仮想マウントされている全画像セル（`[data-grid-cell="true"]`）の `getBoundingClientRect()` と $B$ との交差（Axis-Aligned Bounding Box Intersection）を高速判定し、交差した全画像の `data-image-id` を即座に `selectedImageIds` に反映。
* **操作性と安全ガード**:
  * スクラブバー、ツールバー、各種ボタン、モーダル上のクリックを `target.closest` によりドラッグ選択開始から除外。
  * グローバル `window.addEventListener("mouseup")` リスナーにより、ウィンドウ外やスクロールバー上でマウスを離した場合でも確実にドラッグ状態を解除。
  * `Esc` キー押下で即座に全選択解除、`Ctrl + A` で現在読み込み済みの全画像を全選択。
  * 選択中セルは青枠＋シャドウ＋半透明ブルーオーバーレイ＋左上チェックバッジで強調。未選択セルもホバー時に薄いチェックバッジを表示して選択可能性を明示。
* **一括アクションバー＆ツールバー連携**:
  * 1枚以上選択されている場合、タイムライン最上部に「○枚選択中」「➕ ムードボードに登録」「✕ 選択解除」の固定バーを表示。
  * ツールバー右側の「ボードに追加」ボタンも「ボードに追加 (○枚)」へと動的に切り替わり、既存の `AddToBoardModal` へ選択配列をシームレスに引き渡し。

### 2.6 ムードボードと共通メタデータ設計 (`MoodboardCanvas.tsx`, `ImageDetailPanel.tsx`, `imageMetadata.ts`)
* **画像アイテムとテキストメモ矩形のハイブリッド描画＆原寸高画質レンダリング**:
  * ムードボード上には、写真画像アイテム（`BoardItem`）とテキスト付箋メモ（`BoardNote`）が混在配置可能。
  * **原寸ファイルベース高精細描画**: タイムラインのようなサムネイル画像ではなく、常に実画像ファイル（`/raw/:id`）から読み込み、拡大縮小時も `image-rendering: auto`（バイキュービック/バイリニア補間）を適用してジャギーやボケのない最高画質表示を担保。
  * 共通の `z_index` を用いて、画像の上にメモを置いたり、メモの下に画像を潜り込ませる自然なコラージュ制御を実現。
  * 各アイテムはマウスドラッグによる自由な移動、右下ハンドルによる直感的なリサイズに対応。
  * 付箋メモはダブルクリックまたはアクションバーから即座にテキスト編集可能（`Ctrl+Enter` で確定）。カラーパレット（イエロー、ブルー、グリーン、ピンク、パープル、ダーク）の切替もワンクリックで反映。
* **高精度変形操作（回転バグ解消）＆永続化安全ガード**:
  * **回転ハンドル操作とアクティブRef同期**: 回転操作開始時、`rotatingItemIdRef.current = item.id` を設定し、`useCallback` の古いステート（stale closure）に依存せず常に操作対象の最新IDを参照。未選択の他画像が誤って回転する不具合を根絶。
  * React の非同期ステートやクロージャ依存による移動先座標・対象IDの取りこぼしを防ぐため、ドラッグ中・リサイズ中の最新座標（`currentNotePosRef`）および対象ID（`draggingNoteIdRef`）を `useRef` でリアルタイム同期。
  * ウィンドウ外や他UI要素上でマウスボタンを離した場合のドラッグ残留（マウス追従）を根絶するため、グローバルな `window.addEventListener("mouseup")` 安全ガードリスナーを装備。
  * テキスト入力中の誤ドラッグ防止（`stopPropagation`）およびブラウザ標準テキスト選択との競合防止制御を実装。
* **自動並び替え（Row-based Flow Packing）アルゴリズム (`handleAutoArrange`)**:
  * **手動実行トリガー**: ユーザーが意図して重ねたりレイアウトした配置を勝手に破壊しないよう、上部コントロールバーの「📐 自動並び替え」ボタンからのみ明示的に実行。
  * **実占有サイズの算出**:
    * 各画像アイテムの幅・高さ: $W_i = \text{item.width} \times \text{item.scale}$, $H_i = \text{item.height} \times \text{item.scale}$
    * 各付箋メモの幅・高さ: $W_n = \text{note.width}$, $H_n = \text{note.height}$
  * **行ベース・フロー配置**:
    * アイテム間隔（マージン）を $M = 28\text{px}$、行の最大幅ターゲットを $W_{\max} = 1800\text{px}$ と設定。
    * アイテムを左から右へ順に並べ、現在の行幅 $+ W_k + M > W_{\max}$ となった場合に次行へ折り返し。
    * 行内の最大高さ $\max(H_{\text{row}})$ にマージンを加算して次の行の Y 座標とし、すべての要素同士の重複（被り）を幾何学的に 100% 排除。
    * 矩形の角の衝突や視認性低下を防ぐため、整列時に回転角（`rotation`）を 0° にリセット。
  * **カメラの自動追従 (Zoom to Fit)**:
    * 全整列要素の外接矩形 $[\min X, \max X] \times [\min Y, \max Y]$ を計算。
    * キャンバスの表示領域（幅 $W_c$, 高さ $H_c$）と外接矩形寸法を比較し、全体が画面にすっぽり収まる最適なズーム率 $Z = \text{clamp}(0.9 \times \min(W_c / W_{\text{all}}, H_c / H_{\text{all}}), 0.2, 1.0)$ を算出。
    * ビューポート中央に配置されるパン座標 $(P_x, P_y)$ を設定し、整列直後に全体像を美しく俯瞰可能。
  * **非同期一括永続化**:
    * 整列後の新座標を `backendApi.updateBoardItem` および `backendApi.updateBoardNote` により SQLite DB へ一括永続化。
* **共通詳細情報パネル連携 (`ImageDetailPanel`)**:
  * タイムラインのビューアモーダルとムードボードのキャンバス右下で全く同一のコンポーネントを利用し、UI/UX の一貫性を担保。
  * 選択中画像の詳細をキーボードショートカット `I` またはアクションバーの「詳細情報」ボタンで即座に展開・格納。
* **画像メタデータ・圧縮率解析アルゴリズム (`imageMetadata.ts`)**:
  * **フォーマット正規化**: 拡張子および MIME タイプに基づき、大文字フォーマット名（JPEG, PNG, WebP, GIF, BMP, TIFF, AVIF, HEIC等）を統一。
  * **可逆 / 非可逆判定**:
    * PNG, BMP, GIF, TIFF: 可逆圧縮（Lossless）
    * JPEG: 非可逆圧縮（Lossy）
    * WebP: 拡張子・mime・メタデータから判定（デフォルトは WebP Lossy）
  * **生RGB比と圧縮率計算**:
    * 画像の非圧縮24bit RGB生データサイズを算出: $\text{RawSize} = \text{width} \times \text{height} \times 3 \text{ bytes}$
    * 削減率（Compression Ratio）: $\text{Reduction} = \frac{\text{RawSize} - \text{file\_size}}{\text{RawSize}} \times 100\%$
    * 生データ比: $\text{RawPercent} = \frac{\text{file\_size}}{\text{RawSize}} \times 100\%$
    * 1ピクセルあたり実効ビット数: $\text{bpp} = \frac{\text{file\_size} \times 8}{\text{width} \times \text{height}}$
    * 寸法不明時やBMP等の非圧縮ファイル時も適切なフォールバック表示を実施。

### 2.7 詳細画像ビューアの高品質レンダリング＆ピクセル等倍ズーム設計 (`ImageViewerModal.tsx`, `useViewerGestures.ts`)
* **速度優先（タイムライン）と品質優先（ビューア）の完全分離**:
  * タイムライン一覧では、スクロール性能（60fps）と省メモリを最優先し、軽量な WebP サムネイルのみを仮想スクロールで描画。
  * 詳細画像ビューアでは、しっかり鑑賞・確認する用途に特化し、原寸画像ファイル（`/raw/:id`）から同期デコード（`decoding="sync"`）で読み込み、最高品質のバイキュービック補間（`imageRendering: "auto"`）を適用。
* **真の 100% 原寸ピクセル等倍（1:1 Pixel Scale）アルゴリズム**:
  * 回転後の実画像寸法（$W, H$）とコンテナ表示寸法（$W_c, H_c$）から、画面フィット時の縮小率 $S_{\text{fit}} = \min(W_c / W, H_c / H)$ を算出。
  * 原寸 1 ピクセルがディスプレイの 1 ピクセルに完全一致するスケール倍率 $Z_{100\%} = 1.0 / S_{\text{fit}}$ を動的に計算。
  * `1` キー押下、上部ツールバーの等倍ボタン、またはダブルクリックにより、瞬時に $Z_{100\%}$（原寸等倍）とフィット表示（$Z=1.0$）をトグル切替。
* **ハードウェアアクセラレーションと高品質スケーリング**:
  * `translate3d` による 3D GPU 加速レイヤーを適用しつつ、`backfaceVisibility: "hidden"` によりズーム操作時のモアレ・ジャギーやチラつきを抑制。
  * 最大 16 倍までの高倍率ズームに対応し、高解像度写真の細部（ピント、微細な文字、テクスチャ）まで鮮明に確認可能。

---

## 3. バックエンド設計 (Backend Architecture)

### 3.1 ローカル HTTP サーバー設計 (`server.rs`)
* **固定スレッドプール**:
  * Windows 環境でのスレッド生成オーバーヘッドを排除するため、CPU コア数に応じた固定スレッドプール（8〜16スレッド）を起動。
* **サムネイル配信ハンドラ (`/thumbs/:id`) の高速配信＆実アクセス時更新検知**:
  ```text
  クライアント要求: GET /thumbs/:id
       │
       ├─ [1] 実アクセス時更新検知 (On-Access Change Detection):
       │        └─ std::fs::metadata で実ファイルの (size, mtime) を軽量確認 (< 0.05ms)
       │        └─ 変更検知時: 新 quick_hash / EXIF 再計算 → images レコード更新
       │                        → 新サムネ生成キュー投入 (High) → 503 即時返却 (リトライ誘導)
       │                        （CAS原則: 旧ハッシュのBLOBは他画像が共有している可能性があるため即時削除せず維持）
       │
       ├─ [2] インメモリ WebP キャッシュにあれば即返却 (0ms)
       ├─ [3] サムネイル専用DB (thumbnails.db) にあれば mmap ゼロコピー返却 (< 0.5ms)
       ├─ [4] 旧ファイルキャッシュ (thumbs/v1/{hash}.webp) があれば DB へ自動移行して返却 (< 1ms)
       └─ [5] サムネイル未存在の場合:
               ├─ パイプラインへ高優先度 (High) でジョブ投入
               └─ HTTPスレッドは待機せず直ちに「503 Service Unavailable」を返却 (0ms)
                  （HTTPスレッドは即解放され、次のAPI通信を一切邪魔しない）
  ```

### 3.2 高速サムネイル生成パイプライン (`queue.rs`, `thumbnail.rs`)
* **優先度ヒープキュー (Priority Queue)**:
  * `High`: 現在画面に見えている画像セル
  * `Mid`: 画面の周辺（先読み対象）
  * `Low`: バックグラウンド自動補完（空き時間に全件走査生成）
* **世代管理 (Generation Tracking)**:
  * ユーザーがスクロールするたびに世代番号（`generation`）をインクリメント。
  * 古い世代の待機要求は即座に破棄（パージ）され、現在見えている位置の画像に全 CPU リソースを集中。
* **EXIF IFD1 内蔵サムネイル最優先抽出アルゴリズム**:
  ```text
  元画像ファイル (source_path)
       │
       ├─ EXIF解析 (kamadak-exif)
       │   └─ IFD1 に Tag::JPEGInterchangeFormat が存在するか？
       │        ├─ [YES]: 内蔵サムネイルJPEGバイトを抽出
       │        │         └─ 数MB〜数十MBの元画像フル展開を 100% 回避
       │        │         └─ 内蔵JPEGを直接デコード・WebP化 (所要時間: 1〜3ms)
       │        └─ [NO]: 元画像全体を制限付きリーダーで安全にデコード (フォールバック)
       │
       ├─ fast_image_resize による SIMD 縮小リサイズ
       ├─ WebP lossy エンコード (品質 60.0)
       └─ サムネイル専用DB (thumbnails.db) へ BLOB 格納 (WITHOUT ROWID)
  ```

### 3.3 自動差分走査＆アイドルウォッチャー＆孤立サムネイルGC (`walker.rs`, `watcher.rs`)
* **高速走査**: `walkdir` による再帰走査と `rayon` による並行メタデータ抽出。
* **クイックハッシュ (`hash.rs`)**:
  * ファイル先頭・中間・末尾の各 64KB ＋ ファイルサイズを組み合わせた BLAKE3 ハッシュにより、巨大画像やRAWでも一瞬で重複検知・一意識別キーを生成。
* **アイドルウォッチャー**:
  * サムネイルキューが空、かつユーザー操作がないアイドル時（45秒経過後）に登録フォルダの差分スキャンを自動実行。
* **孤立サムネイル回収 (Garbage Collection: GC)**:
  * フォルダ差分スキャン完了後のアイドル時に、`thumbnails.db` のハッシュ一覧と `catalog.db` の `images` テーブルを照合。
  * 画像ファイルの更新や削除によりどの画像からも参照されなくなった孤立 BLOB（参照数 0）を特定し、安全に増分一括削除（`delete_batch`）。キューへのジョブ到着時は即座に中断。

---

## 4. データベース設計 (Database Schema)

カタログデータは SQLite データベース（WAL モード）で管理されます。

### 4.1 テーブル定義

#### ① `watched_folders` (監視対象フォルダ)
```sql
CREATE TABLE watched_folders (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    path TEXT NOT NULL UNIQUE,          -- 正規化済みディレクトリパス
    last_scanned_at INTEGER NOT NULL    -- 最終走査日時 (UNIXエポック秒)
);
```

#### ② `images` (画像メタデータ)
```sql
CREATE TABLE images (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    folder_id INTEGER NOT NULL REFERENCES watched_folders(id) ON DELETE CASCADE,
    file_path TEXT NOT NULL UNIQUE,     -- 元ファイルの絶対パス
    file_size INTEGER NOT NULL,         -- ファイルサイズ (バイト)
    file_mtime INTEGER NOT NULL,        -- ファイル更新日時 (UNIXエポック秒)
    taken_at INTEGER NOT NULL,          -- 撮影日時 (EXIFまたはmtime)
    taken_at_source INTEGER NOT NULL,   -- 0: Exif, 1: mtime
    width INTEGER,                      -- 幅 (Orientation適用後)
    height INTEGER,                     -- 高さ (Orientation適用後)
    orientation INTEGER NOT NULL DEFAULT 1, -- EXIF Orientation (1〜8)
    format TEXT,                        -- 拡張子 / フォーマット ('jpg', 'png' 等)
    quick_hash TEXT NOT NULL,           -- BLAKE3 クイックハッシュ (サムネファイル名)
    thumb_status INTEGER NOT NULL DEFAULT 0, -- 0: 未生成, 1: 生成済, 2: 失敗
    indexed_at INTEGER NOT NULL         -- カタログ登録日時
);

CREATE INDEX idx_images_taken_at_id ON images(taken_at DESC, id DESC);
CREATE INDEX idx_images_folder_id ON images(folder_id);
CREATE INDEX idx_images_thumb_status ON images(thumb_status);
CREATE INDEX idx_images_quick_hash ON images(quick_hash);
```

#### ③ `boards` (ムードボード)
```sql
CREATE TABLE boards (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    background_color TEXT NOT NULL DEFAULT '#121214',
    pan_x REAL NOT NULL DEFAULT 0.0,
    pan_y REAL NOT NULL DEFAULT 0.0,
    zoom REAL NOT NULL DEFAULT 1.0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
```

#### ④ `board_items` (ボード配置アイテム)
```sql
CREATE TABLE board_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    board_id INTEGER NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
    image_id INTEGER NOT NULL REFERENCES images(id) ON DELETE CASCADE,
    x REAL NOT NULL DEFAULT 0.0,
    y REAL NOT NULL DEFAULT 0.0,
    width REAL NOT NULL DEFAULT 240.0,
    height REAL NOT NULL DEFAULT 180.0,
    rotation REAL NOT NULL DEFAULT 0.0,
    z_index INTEGER NOT NULL DEFAULT 0,
    crop_x REAL NOT NULL DEFAULT 0.0,
    crop_y REAL NOT NULL DEFAULT 0.0,
    crop_width REAL NOT NULL DEFAULT 1.0,
    crop_height REAL NOT NULL DEFAULT 1.0,
    flip_h INTEGER NOT NULL DEFAULT 0,
    flip_v INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
```

#### ⑤ `board_notes` (ボード配置テキストメモ: マイグレーション v5)
```sql
CREATE TABLE board_notes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    board_id INTEGER NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
    text TEXT NOT NULL DEFAULT '',
    x REAL NOT NULL DEFAULT 0.0,
    y REAL NOT NULL DEFAULT 0.0,
    width REAL NOT NULL DEFAULT 200.0,
    height REAL NOT NULL DEFAULT 150.0,
    color TEXT NOT NULL DEFAULT '#fef08a', -- 付箋背景カラーコード
    z_index INTEGER NOT NULL DEFAULT 0,
    is_locked INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX idx_board_notes_board_id ON board_notes(board_id);
```

#### ⑥ `bookmarks` (タイムラインしおり: マイグレーション v6)
```sql
CREATE TABLE bookmarks (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    folder_id INTEGER REFERENCES watched_folders(id) ON DELETE SET NULL,
    folder_name TEXT,
    sort TEXT NOT NULL,
    scroll_top REAL NOT NULL,
    row_index INTEGER NOT NULL,
    image_index INTEGER,
    day_label TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE INDEX idx_bookmarks_created ON bookmarks(created_at DESC);
```

#### ⑦ `thumbnails` (サムネイル専用BLOBストア: `%LOCALAPPDATA%/.../cache/thumbnails.db`)
```sql
CREATE TABLE thumbnails (
    quick_hash  TEXT PRIMARY KEY,       -- BLAKE3 サンプリングハッシュ
    data        BLOB NOT NULL,          -- WebP バイナリ
    byte_size   INTEGER NOT NULL,       -- バイト数
    created_at  INTEGER NOT NULL        -- 生成日時 (UNIXエポック秒)
) WITHOUT ROWID;
```
* **WITHOUT ROWID の採用**: B-Tree リーフに直接 BLOB を配置し、インデックス引きとテーブル探索の二重ルックアップを排除して SQLite 公式ベンチマーク通りの最高速度を達成。
* **最適化 PRAGMA**: `page_size = 8192` (8KB), `mmap_size = 268435456` (256MB), `journal_mode = WAL`, `synchronous = NORMAL`。

---

## 5. 通信インターフェース仕様 (REST API)

| メソッド | パス | 機能概要 | レスポンス / 挙動 |
| :--- | :--- | :--- | :--- |
| `GET` | `/thumbs/:id` | サムネイル画像配信 | `image/webp` (キャッシュヒット時 200、生成中は 503 + Retry-After) |
| `GET` | `/raw/:id` | 原寸元画像配信 (ビューア用) | 各種 MIME タイプ (jpeg, png, 等) |
| `GET` | `/api/watch_folders` | 登録フォルダ一覧取得 | `WatchedFolder[]` (登録数、サムネ完了数、オンライン状態) |
| `POST` | `/api/watch_folders` | 新規フォルダ登録 | `{ path: string }` |
| `DELETE` | `/api/watch_folders/:id` | フォルダ登録解除 | レコードのみ削除（元ファイル保護） |
| `POST` | `/api/rescan` | フォルダ再走査実行 | `{ folder_id?: number }` |
| `GET` | `/api/timeline/summary` | タイムライン日別サマリ | `{ total: number, buckets: DayBucket[], version: number }` |
| `GET` | `/api/timeline/images` | タイムライン画像一覧取得 | `GetImagesPayload` に応じた `ImageRecord[]` |
| `GET` | `/api/images/:id` | 画像メタデータ詳細取得 | `ImageDetail` (EXIF情報含む) |
| `GET` | `/api/boards` | ボード一覧取得 | `Board[]` |
| `POST` | `/api/boards` | 新規ボード作成 | `{ name: string }` → `Board` |
| `GET` | `/api/boards/:id/items` | ボード内アイテム一覧取得 | `BoardItem[]` |
| `POST` | `/api/boards/:id/items` | ボード内画像追加 | `AddBoardItemsPayload` → `BoardItem[]` |
| `PUT` | `/api/board_items/:id` | ボードアイテム更新 | `UpdateBoardItemPayload` → `BoardItem` |
| `DELETE` | `/api/board_items/:id` | ボードアイテム削除 | 200 OK |
| `GET` | `/api/boards/:id/notes` | ボード内メモ一覧取得 | `BoardNote[]` |
| `POST` | `/api/boards/:id/notes` | 新規メモ作成 | `CreateBoardNotePayload` → `BoardNote` |
| `PUT` | `/api/board_notes/:id` | メモ更新 (位置/サイズ/テキスト/色/Z/ロック) | `UpdateBoardNotePayload` → `BoardNote` |
| `DELETE` | `/api/board_notes/:id` | メモ削除 | 200 OK |
| `POST` | `/api/viewport` | 現画面表示範囲の通知 | `{ visibleIds: number[], nearbyIds: number[] }` |
| `POST` | `/api/clear_queue` | サムネイルキュー即時パージ | 古い待機リクエストを一掃解放 |
| `POST` | `/api/thumbnails/rescan_missing` | 未生成・失敗サムネ一括再作成 | 失敗ステータスリセット ＆ バックグラウンド生成開始 |
| `GET` | `/api/thumb_progress` | サムネイル全体進捗取得 | `{ done: number, failed: number, total: number, isGenerating: boolean }` |
| `GET` | `/api/thumbnails/failed` | 失敗画像レコード一覧取得 | `FailedImageRecord[]`（破損・空ファイル診断） |
| `GET` | `/api/logs` | 直近ログ取得 | `LogEntry[]`（メモリリングバッファから最新順取得） |
| `POST` | `/api/logs/open` | ログ保存先フォルダ表示 | OSファイルマネージャでログ保存先を開く |
| `GET` / `POST` | `/api/heartbeat` | フロントエンド・サーバー死活確認 | `{ alive: true, status: "ok", version: "0.1.0" }` |
| `GET` | `/api/health` | サーバーヘルスチェック | `{ alive: true, status: "ok", version: "0.1.0" }` |
| `POST` | `/api/shutdown` | バックエンド終了要求 | バックエンドプロセスを安全に終了 |

---

## 6. プロセス・ライフサイクルとシステムトレイ設計

```mermaid
flowchart LR
    subgraph OS ["OS ネイティブ環境 (Windows / macOS / Linux)"]
        Tray["システムトレイ (Taskbar / MenuBar / AppIndicator)"]
        Protocol["カスタムURIプロトコル (imagemediaviewer://)"]
    end

    subgraph Backend ["バックエンドサーバー (Rust / Port 14201)"]
        MainWindow["Tauri メインウィンドウ (WebView2 / React UI)"]
        NativeIPC["Tauri Native IPC (メモリ内通信)"]
        CustomProtocol["カスタム URI プロトコル (thumb://, original://)"]
        TrayHandler["トレイイベントハンドラ (TrayIcon)"]
        ScannerCore["バックグラウンド走査 / サムネ生成"]
        SettingsManager["設定永続化 (app_settings.json)"]
    end

    Tray -->|左クリック / 右クリックメニュー| AppProcess
    TrayHandler -->|クライアントを開く| MainWindow
    TrayHandler -->|常駐するトグル| SettingsManager
    MainWindow <-->|IPC| NativeIPC
    MainWindow <-->|サムネ・画像取得| CustomProtocol
```

### 6.1 完全ポートレス一体化とタスクトレイ常駐制御 (Portless Lifecycle & Tray)
* **完全ポートレス一体型アーキテクチャ**:
  * TCP/IPポート（14201 等）を一切開かない完全ポートレス構成を採用。
  * データ通信は Tauri Native IPC（メモリ内メッセージパイプ）、画像・サムネイル配信は内部プロトコル（`thumb://`, `original://`）で直接完結。
  * ポート競合、ファイアウォール警告、およびサーバー・クライアント間の非同期による片肺停止を根本的に解消。
* **タスクトレイ常駐ライフサイクル**:
  * **デフォルト常駐（ON）**: メインウィンドウの「×」ボタンを押した際、`WindowEvent::CloseRequested` を捕捉して `api.prevent_close()` を実行し、ウィンドウを非表示（Hide）にしてタスクトレイに常駐。
  * **ウィンドウの復元**: タスクトレイアイコンの左クリック、または右クリックメニューの「クライアントを開く」から、非表示中のメインウィンドウを最前面に即座に再表示。
  * **完全停止**: タスクトレイメニューの「終了」を実行することで、アプリケーション全体を完全停止（`app_handle.exit(0)`）。
* **「常駐する」設定メニューと設定の永続化**:
  * タスクトレイ右クリックメニューに「✔ 常駐する」（`CheckMenuItem`）を搭載。初期状態は **ON**。
  * クリックにより ON / OFF を切り替え可能。設定値は `%APPDATA%\com.imagemediaviewer.desktop\app_settings.json` へ自動保存され、次回起動時にも復元。
  * **OFF の場合**: メインウィンドウの「×」ボタンを押すと、トレイに残らずアプリケーション全体が即座に完全終了。
* **トレイ右クリックメニュー構成**:
  * **クライアントを開く**: メインウィンドウを表示・最前面化。
  * **✔ 常駐する**: 常駐モードの有効/無効を切り替え（設定は自動保存）。
  * ──────────────（セパレータ）
  * **フォルダを再走査**: 登録済みフォルダの差分スキャンをバックグラウンド実行。
  * **データフォルダを開く**: カタログDBおよびサムネイルキャッシュのディレクトリをOSファイルマネージャで開く。
  * **ログフォルダを開く**: ログ保存先ディレクトリを開く。
  * ──────────────（セパレータ）
  * **終了**: アプリケーション全体を完全終了。

### 6.2 通信状態監視と再接続 (Heartbeat & Multi-platform Recovery)
* **常時監視**: クライアント上部右側の `ConnectionStatusIndicator` により、定期ハートビート（`/api/heartbeat`）で通信状態を可視化。
* **多重起動防止型ワンクリック起動**:
  * カスタムプロトコル `imagemediaviewer://launch` のターゲットを、クライアント起動スクリプト（`run.bat`）からサーバー専用ランチャー（`run-server.bat`）へ設定。
  * `run-server.bat` はリリース版バイナリ（`image-media-viewer.exe`）を優先探索し、`--server-only` 引数を渡して WMI（`Win32_Process.Create`）経由で呼び出し元の親プロセス（ブラウザやシェル等の Job Object）から完全に切り離された独立バックグラウンドプロセスとして起動。
  * バックエンドバイナリ起動時の無条件クライアント表示呼び出し（`launch_client()`）を NOP 化することで、サーバー起動時に余計な Chrome ウィンドウがもう一つ開いてしまう多重起動問題を完全抑止。すでに開いているクライアント画面のみが自動再接続されてシームレスに継続利用可能。
  * フロントエンドからの呼び出しは、最新の Chromium セキュリティポリシー（iframe からの外部プロトコル遮断）に対応し、トップレベルナビゲーション（`window.location.href = "imagemediaviewer://launch"`）を採用。タイマーと段階的バックオフリトライ（1.2秒、3秒、5秒、7.5秒）により、画面遷移を発生させず接続復旧を待機。
* **URI プロトコルの自動登録と自己修復**:
  * バックエンド本体（`image-media-viewer.exe`）の初期化エントリポイント（`lib.rs` 内 `ensure_custom_protocol_registered()`）およびデスクトップランチャー（`run.ps1`）の双方に、Windows レジストリ（`HKCU:\Software\Classes\imagemediaviewer`）の自動登録・自己修復ロジックを統合。
  * インストーラーによるインストール後（`C:\Program Files\ImageMediaViewer` 等）の初回起動時でも、バッチスクリプトに依存せず自身の EXE フルパス（`image-media-viewer.exe --server-only "%1"`）が管理者権限不要（HKCU）で即座に登録される。
  * プロジェクト移動、再インストール、ポータブル展開時にも常に現在実行中の EXE パスへ自動同期。
  * プロトコルハンドラ呼び出し時に OS から渡される引数（`%1`）を安全に受け取れるコマンドライン構文を採用。
  * バッチファイル（`run-server.bat`）内の構文エラーを防止し、CRLF 改行コードを保証。インストーラ配布物（`bundle.resources`）にも同梱。
* **OS自動判定によるクロスプラットフォーム回復対応**:
  * フロントエンドがユーザーの `navigator.userAgent` / `navigator.platform` から稼働OS（Windows / macOS / Linux）を自動判定。
  * 手動回復用コマンドのコピーボタンを、Windows環境では `.\run-server.bat`、macOS / Linux 環境では `./run-server.sh` に自動で切り替え。
* **段階的自動リトライ機構とユーザーフィードバック**:
  * URIスキーム呼び出し直後、ボタン上に「サーバー起動を試行中...」とスピナーを表示。
  * OSプロセス起動のタイムラグ（1.2秒、3秒、5秒、7.5秒）を考慮した段階的自動再接続リトライを実施し、復旧成功率を大幅に向上。万一自動起動しない場合もワンクリックで起動コマンドをコピーして手動実行可能。
* **自動復帰**: 切断状態からサーバーが再起動・復帰した場合、自動的にタイムラインや画像サマリを再同期。

---

## 7. ログ・エラー診断アーキテクチャ (Logging & Diagnostics)

```mermaid
flowchart TD
    subgraph Engine ["ネイティブエンジン内部"]
        TracingEvents["tracing イベント\n(info!, warn!, error!)"]
        CustomLayer["カスタムレイヤー\n(MemoryAndFileLoggerLayer)"]
        LogMgr["ログマネージャ\n(LogManager)"]
        LogFile["ログファイル永続化\n%APPDATA%/.../logs/app.log"]
        RingBuffer["インメモリリングバッファ\n(直近500件)"]
    end

    subgraph StatusManagement ["サムネイル状態管理"]
        Queue["キューワーカー (queue.rs)"]
        Repo["DBリポジトリ (repo.rs)"]
        CatalogDB[("カタログDB (imagesテーブル)")]
    end

    subgraph ClientDiag ["クライアントUI (診断)"]
        StatusBar["StatusBar (進捗・失敗バッジ)"]
        LogModal["LogViewerModal (ログ・失敗一覧)"]
        Tray["システムトレイメニュー"]
    end

    TracingEvents --> CustomLayer
    CustomLayer --> LogMgr
    LogMgr --> LogFile
    LogMgr --> RingBuffer

    Queue -->|生成失敗時 (thumb_status=2)| Repo
    Repo -->|未生成(0)と失敗(2)を厳密分離| CatalogDB
    Queue -->|エラー詳細・パス・画像ID出力| TracingEvents

    RingBuffer -->|GET /api/logs| LogModal
    CatalogDB -->|GET /api/thumbnails/failed| LogModal
    CatalogDB -->|GET /api/thumb_progress| StatusBar
    Tray -->|POST /api/logs/open| LogFile
```

### 7.1 ログ多層出力とインメモリバッファ
* **多層出力パイプライン**:
  * `tracing-subscriber` のカスタム `Layer` 実装により、標準出力へのリアルタイム出力と同時に、ファイル永続出力および最新500件のインメモリリングバッファへ逐次記録。
* **ファイル永続化**:
  * `%APPDATA%\com.imagemediaviewer.desktop\logs\app.log` へ常時アペンド保存。Windows GUI / 非表示バックグラウンド稼働時でもログが消失しない。
* **低レイテンシAPI取得**:
  * クライアントは `/api/logs` により、ディスク読み取り負荷なしにインメモリバッファから瞬時に直近ログを取得可能。

### 7.2 サムネイル生成失敗管理と無限ループ防止
* **状態コードの厳密分離**:
  * `thumb_status = 0`: 未生成（生成対象）
  * `thumb_status = 1`: 生成完了
  * `thumb_status = 2`: 生成失敗（破損画像、0バイトファイル、未対応動画/特殊形式等）
* **補充ループ防止**:
  * `get_pending_thumb_sources` で `thumb_status = 0` のみを抽出対象とし、失敗した画像（`thumb_status = 2`）がキューへ延々と再投入される無限ループを根絶。
* **原因特定・診断UI**:
  * `StatusBar` で「○件完了 / △件失敗」を明確に示し、ワンクリックで `LogViewerModal` を起動。失敗画像のパス・サイズ・形式を確認し、エクスプローラでの直接確認およびワンクリック再試行（リセット＆再スキャン）を実行可能。

---

## 8. 省電力・静音スロットリングアーキテクチャ (Eco & Quiet Throttling)

常駐型デスクトップアプリケーションとして、長期間のバックグラウンド稼働時にCPUファンが高速回転したりCPU使用率を無駄に消費しないための多層スロットリング制御を実装しています。

```mermaid
flowchart TD
    subgraph IdleDetection ["アイドル監視制御 (watcher.rs)"]
        IdleCheck{"パイプラインが\n完全アイドルか？"}
        CoolingTimer{"30秒間の\nクーリング経過？"}
        IntervalTimer{"前回の走査から\n5分経過？"}
    end

    subgraph ThrottledScan ["省電力スロットル走査 (walker.rs)"]
        WalkLoop["ファイル走査ループ (WalkDir)"]
        SleepWalk["100ファイル毎に 5ms 休止"]
        UserInterrupt{"ユーザー操作検知\n(!is_idle)"}
        Abort["走査を即座に中断して譲る"]
        IndexLoop["メタデータ抽出・DB登録"]
        SleepIndex["100件毎に 30ms 休止"]
    end

    subgraph PipelineThrottling ["パイプライン省電力制御 (queue.rs)"]
        JobType{"ジョブ優先度判定"}
        HighMid["High / Mid (表示中)"]
        FastProcess["休止なし・最高速生成 (1〜2ms)"]
        LowJob["Low (バックグラウンド)"]
        CoolingSleep["生成後に 20ms 休止 (ファン回転抑止)"]
        EmptyWait["ヒープ空・未生成なし時は 60秒 静止待機 (CPU 0.0%)"]
    end

    IdleCheck -->|Yes| CoolingTimer
    CoolingTimer -->|Yes| IntervalTimer
    IntervalTimer -->|Yes| WalkLoop

    WalkLoop --> SleepWalk
    WalkLoop --> UserInterrupt
    UserInterrupt -->|Yes| Abort
    UserInterrupt -->|No| IndexLoop
    IndexLoop --> SleepIndex

    JobType -->|表示要求| HighMid --> FastProcess
    JobType -->|バックグラウンド| LowJob --> CoolingSleep
    JobType -->|キュー空| EmptyWait
```

### 8.1 アイドル走査スロットリング (walker.rs & watcher.rs)
* **適正な実行インターバルとクーリング期間**:
  * 差分走査の間隔を従来の45秒から **5分（300秒）** に拡大。
  * サムネイルパイプラインがアイドルになった後、**30秒間のクーリング期間** を待機してから走査を開始し、ユーザーの連続作業の合間に無駄なI/Oが発生するのを抑制。
* **Walking フェーズの I/O スロットリング**:
  * 100ファイル走査するごとに **5ms のスレッド休止** を挿入。ディスクI/Oキューの過熱とCPU 1コア100%張り付きを防ぎ、平均CPU負荷 1〜3% で静かに走査を完了。
* **ユーザー最優先の即時中断 (Preemption)**:
  * バックグラウンド走査中、50ファイル毎および各バッチ処理時にパイプラインのアイドル状態を監視。ユーザーがスクロールや画像クリック等を行った場合は、即座に走査を安全中断してCPU・ディスクリソースを最優先でUIへ明け渡す。
* **Indexing フェーズの負荷分散**:
  * メタデータ抽出バッチを100件単位に縮小し、バッチ毎に **30ms のクーリング休止** を挿入。

### 8.2 バックグラウンドサムネイル生成のクーリング休止 (queue.rs)
* **表示中ジョブとバックグラウンドジョブの分離**:
  * ユーザーが見ている画面（High / Mid 優先度）は休止なしで **最速生成（1〜2ms / 枚）** を維持。
  * 画面外の事前生成（Low 優先度）は、1枚のサムネイル生成完了ごとに **20ms のクーリング休止** を挿入。
  * これにより、何千枚ものサムネイルを連続生成している最中でも、CPU使用率は 10〜15% 程度に抑制され、**ノートPCやデスクトップのCPUファンが唸るのを確実に防止**。

### 8.3 ワーカースレッドの完全アイドル待機 (queue.rs)
* **無駄な定期起床の根絶**:
  * キューが空で未生成サムネイルもない定常時、ワーカースレッド群の条件変数タイムアウトを従来の5秒から **60秒** へ延長。
  * 常駐アイドル時のCPU使用率は **0.0%** を維持し、バッテリー消費や発熱を最小限に抑える。
  * 新規画像の追加や表示スクロールが発生した際は、`Condvar::notify_one` / `notify_all` により **0ミリ秒で即座に起床** するため、応答性への悪影響は皆無。

---

## 9. 多言語対応 (i18n) アーキテクチャ

ImageMediaViewer は、プログラム本体のロジックと言語リソースを完全に分離し、外部ライブラリ（i18next 等）を追加せずに型安全・超軽量・ゼロアロケーションの独自多言語機構を採用しています。

```mermaid
flowchart LR
    subgraph Frontend ["フロントエンド (React / TypeScript)"]
        UIComp["UIコンポーネント (t() / useTranslation)"]
        StoreLocale["Zustand store (locale: ja | en)"]
        Fallback["透過的フォールバック (enマスター)"]
        JaRes["日本語リソース (ja.ts)"]
        EnRes["英語マスターリソース (en.ts)"]
    end

    subgraph Backend ["バックエンド (Rust / Tauri v2)"]
        Tray["OSタスクトレイメニュー (update_tray_locale)"]
        Settings["設定永続化 (app_settings.json)"]
        RustLocales["Rust言語リソース (locales.rs)"]
    end

    StoreLocale -->|言語変更| UIComp
    UIComp --> Fallback
    Fallback --> JaRes
    Fallback -. "翻訳未達キー" .-> EnRes
    StoreLocale -->|backendApi.setLocale| Settings
    Settings --> Tray
    Tray --> RustLocales
```

### 9.1 フロントエンド多言語機構 (`src/locales/`)
* **独立言語リソース部品 (`en.ts`, `ja.ts`)**:
  * 英語 (`en.ts`) をマスター型定義（`TranslationSchema`）とし、すべてのカテゴリ（`sidebar`, `statusBar`, `timeline`, `viewer`, `metadata`, `moodboard`, `logModal`, `connection`, `addToBoardModal`, `errorBoundary` 等）のキー構造を型推論。
  * 新たな言語を追加する際は、単に辞書オブジェクト（`zh.ts`, `es.ts` 等）を追加・登録するだけで容易に拡張可能。
* **開発先行時の自動英語フォールバック**:
  * 機能開発が先行し、特定言語リソースへの翻訳登録が追いつかない場合でも、キー単位で自動的に英語マスターリソースから取得して表示。未翻訳箇所でアプリがクラッシュしたりキー名（`timeline.toolbar.reload` 等）がそのまま露出することを防止。
* **ホットパスのゼロアロケーション・性能劣化防止**:
  * タイムライン仮想スクロールグリッド（`GridCell`）等、1秒間に数百回呼び出される可能性のあるホットパスでのオーバーヘッドを徹底排除。
  * `getNestedValue`: ドット区切り文字列（`category.item`）を split 配列生成せず、`indexOf('.')` で直接切り出すゼロヒープアロケーション検索を実装。
  * 置換パラメータ（`{param}`）が含まれない静的テキストは、正規表現マッチングを完全バイパスして即座に返却。
* **UI言語切替トグル**:
  * サイドバー下部に「日本語 / English」切り替えUIを常設。LocalStorage およびバックエンド設定へ即時永続化。

### 9.2 Rust バックエンド多言語機構 (`src-tauri/src/locales.rs`, `tray.rs`)
* **タスクトレイの動的言語切替**:
  * OSタスクトレイのメニュー（「メイン画面を開く」「常に最前面表示」「常駐する」「終了」）およびトレイアイコンのツールチップを、アプリを再起動することなく実行時に即座に切り替え。
  * `setup_system_tray` 時に作成した各メニューアイテムのハンドルを `TrayMenuHandles` 構造体として `app.manage` に保持し、`set_locale` コマンド呼び出し時に `tray.rs` の `update_tray_locale` が各ハンドルの `set_text` を呼んで即時更新。
* **ゼロアロケーション `&'static str` 参照**:
  * バックエンドのトレイメッセージはヒープ割り当てを行わず、静的参照（`&'static TrayMessages`）を直接返却。未知の言語コードに対しても英語（`TRAY_EN`）へ安全にフォールバック。
* **設定の自動同期永続化**:
  * 言語設定は `AppSettings` の `locale` フィールドにシリアライズされ、次回起動時もトレイメニューおよびUIが前回の言語で復元。


