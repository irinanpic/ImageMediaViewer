# CLIP STUDIO PAINT (`.clip`) Format Plugin for ImageMediaViewer

ImageMediaViewer 本体から完全に分離された、CLIP STUDIO PAINT（`.clip`）形式向けの独立プラグインプロジェクトです。

## 機能概要
- **高速メタデータ・プレビュー抽出**: `.clip` コンテナ内の `CSFCHUNK` / `CHNKSQLi` チャンクから埋め込み SQLite データベースを切り出し、インメモリで `CanvasPreview` の PNG バイナリおよび画像寸法を抽出します。
- **C-ABI 動的ライブラリ (`imv_clipstudio.dll` / `.so` / `.dylib`)**: プロセス起動オーバーヘッドなしで ImageMediaViewer 本体から高速に呼び出されます。
- **CLI 実行バイナリ (`clipstudio_plugin`)**: 単体でのデバッグ・検証や CLI フォールバックとして使用できます。
- **外部連携アクション**: 右クリックメニューに「CLIP STUDIO PAINTで開く」を動的に追加します。

## 単体ビルド・テスト手順

### 1. 単体テストの実行
```bash
cd plugins-src/clipstudio
cargo test
```

### 2. ビルドと配布パッケージ（ZIP）の作成
プロジェクトルートから以下のコマンドを実行すると、ビルドおよび配布用ZIPパッケージ（`dist-plugins/clipstudio-plugin-v1.0.0-<os>.zip`）が生成されます。

```bash
npm run package:plugins
```

## ユーザーによる導入方法
1. ImageMediaViewer の右クリックメニュー「**メニュー設定...**」を開きます。
2. 「**プラグイン管理**」タブを選択します。
3. 「**ZIPから追加...**」または「**フォルダから追加...**」をクリックし、配布された `.zip` ファイルまたは `clipstudio` フォルダを選択するだけで即座に導入・有効化されます。
