# ImageMediaViewer プラグイン開発・設定ガイド

本ディレクトリ（`plugins/`）は、ImageMediaViewer の拡張機能（画像フォーマット対応および外部ツール連携アクション）の開発・設定を行うためのドキュメントおよび標準プラグイン集です。
インストーラー（NSIS / MSI）によってインストールされた場合、アプリケーションのインストール先ディレクトリ直下（`\ImageMediaViewer\plugins\`）に本ディレクトリが展開されます。

---

## 1. プラグインアーキテクチャの概要

ImageMediaViewer は、高速な画像インデックス・閲覧パイプラインを核としつつ、特定形式（CLIP STUDIO PAINT 等）のデコード・抽出ロジックを本体コードから完全に分離し、独立バイナリ（C-ABI動的ライブラリ `.dll` / `.so` / `.dylib` および独立実行バイナリ CLI）として別クレート/別ターゲットでビルド・配置・動的ロードする完全分離型プラグインアーキテクチャを採用しています。

ClipStudio 形式（.clip）を含むすべての特定フォーマット抽出ロジックは本体コードから分離されており、マニフェストファイル（`plugin.json`）とプラグインバイナリを配置するだけで、**本体の再ビルド（`cargo build`）を一切行うことなく**ユーザー独自形式の追加や外部ツール連携アクションの登録が可能です。

### ディレクトリ構成
```text
plugins/
├── README.md                              # 本開発・設定ガイド（本書）
├── app_settings.sample.json               # 設定ファイル（app_settings.json）のサンプル
├── clipstudio/                            # 標準同梱 ClipStudio プラグイン（本体分離バイナリ）
│   ├── plugin.json                        # ClipStudio形式の抽出ルール・アクション定義マニフェスト
│   ├── imv_clipstudio.dll                 # C-ABI 動的ライブラリ（Windows環境）
│   └── clipstudio_plugin.exe              # 独立実行CLIバイナリ（フォールバック用）
├── sample_format/                         # ユーザー追加用サンプルフォーマットプラグイン
│   └── plugin.json                        # バイナリスライス抽出のサンプルマニフェスト
└── action_plugins/                        # 外部アクション設定サンプル
    └── custom_action_sample.json          # 外部ツール連携アクションの設定サンプル
```

### プラグインの種類
1. **動的画像フォーマットプラグイン (`plugin.json` + 独立バイナリ)**:
   - 独自バイナリ形式や特定ソフトウェア（CLIP STUDIO PAINT 等）のファイルから、内蔵サムネイル・プレビュー画像・メタデータを抽出し、カタログ化および原寸ビューワ表示を可能にする拡張。
   - 高速なメモリ内呼び出し（C-ABI `cdylib`）と、安全な独立プロセスフォールバック（CLIバイナリ）のハイブリッド駆動に対応。本体再コンパイルは不要です。
2. **プラグインアクション (`plugin.json` 内 `actions`)**:
   - 右クリックメニュー（コンテキストメニュー）に、対象形式専用の外部アプリ起動メニュー項目（例: 「CLIP STUDIO PAINTで開く」）を自動登録する拡張。
3. **カスタムメニュー項目 (`app_settings.json` 内 `custom_context_menu_items`)**:
   - ユーザーが設定UIから任意のペイントソフトや外部ツールを右クリックメニューへ登録する拡張。

---

## 2. マニフェスト（`plugin.json`）仕様

プラグインディレクトリ配下に `plugin.json` を配置することで、ランタイムが起動時および設定保存時に自動認識します。

### 2.1 マニフェスト基本構造
```json
{
  "id": "format.clipstudio",
  "name": "CLIP STUDIO PAINT (.clip) Support",
  "version": "1.0.0",
  "description": "CLIP STUDIO PAINT形式（.clip）の内蔵プレビュー画像抽出およびアプリ連携プラグイン（本体分離バイナリ）",
  "extensions": [
    "clip"
  ],
  "magic_header": "CSFCHUNK",
  "extractor": {
    "type": "binary",
    "library": {
      "windows": "imv_clipstudio.dll",
      "linux": "libimv_clipstudio.so",
      "macos": "libimv_clipstudio.dylib",
      "android": "libimv_clipstudio.so"
    },
    "executable": {
      "windows": "clipstudio_plugin.exe",
      "default": "clipstudio_plugin"
    }
  },
  "actions": [
    {
      "id": "action.open_clip_studio",
      "name": "CLIP STUDIO PAINTで開く",
      "name_en": "Open with CLIP STUDIO PAINT",
      "icon": "Palette",
      "order": 20,
      "extensions": [
        "clip"
      ],
      "command": {
        "windows": [
          "C:\\Program Files\\CELSYS\\CLIP STUDIO 1.5\\CLIP STUDIO PAINT\\CLIPStudioPaint.exe",
          "C:\\Program Files\\CELSYS\\CLIP STUDIO 1.5 EN\\CLIP STUDIO PAINT\\CLIPStudioPaint.exe",
          "C:\\Program Files\\CELSYS\\CLIP STUDIO PAINT\\CLIPStudioPaint.exe"
        ],
        "default": "CLIPStudioPaint",
        "args": [
          "{path}"
        ]
      }
    }
  ]
}
```

### 2.2 抽出タイプ (`extractor.type`)

| タイプ | 説明 | 適用例 |
| :--- | :--- | :--- |
| `binary` | 独立ビルドされた動的ライブラリ（C-ABI cdylib）または独立CLIバイナリを呼び出して画像やメタデータを抽出。C-ABIメモリ内直接呼び出し（`libloading`）を最優先とし、未ロード時や環境非対応時はCLI実行へ自動フォールバック。 | CLIP STUDIO PAINT（.clip）、各種独自プロプライエタリ形式等 |
| `binary_slice` | ファイル内部から開始シグネチャと終端シグネチャ（16進文字列またはASCII）で画像バイナリを切り出し。 | ヘッダ内蔵サムネイル形式等 |
| `command` | 外部ツールやスクリプトを実行し、`{input}` から `{output}` 画像を抽出・生成。 | 外部RAW現像ツール、コマンドライン変換器等 |

#### A. `binary` の設定パラメータ
- `library`: プラットフォーム別動的ライブラリ名または相対パス（`windows`, `linux`, `macos`, `android`, `default`）。
  - Windows: `imv_clipstudio.dll`
  - Linux: `libimv_clipstudio.so`
  - macOS: `libimv_clipstudio.dylib`
  - Android: `libimv_clipstudio.so`（APK 内 `jniLibs/arm64-v8a/`, `jniLibs/x86_64/` または `nativeLibraryDir` から Bionic `dlopen` により直接動的ロード。起動時に `assets/plugins` の `plugin.json` は内部ストレージへ自動展開されます）
- `executable`: プラットフォーム別独立CLI実行バイナリ名または相対パス（`windows`, `linux`, `macos`, `default`）。デスクトップ環境でのフォールバック実行用。

##### C-ABI 動的ライブラリ エクスポート仕様
動的ライブラリ（`.dll` / `.so` / `.dylib`）は以下の C-ABI 関数をエクスポートします：
```rust
#[no_mangle]
pub unsafe extern "C" fn imv_plugin_init() -> i32;

#[no_mangle]
pub unsafe extern "C" fn imv_plugin_extract_metadata(
    path: *const std::os::raw::c_char,
    out_width: *mut u32,
    out_height: *mut u32,
) -> i32;

#[no_mangle]
pub unsafe extern "C" fn imv_plugin_extract_preview(
    path: *const std::os::raw::c_char,
    out_buf: *mut *mut u8,
    out_len: *mut usize,
) -> i32;

#[no_mangle]
pub unsafe extern "C" fn imv_plugin_free_buffer(buf: *mut u8, len: usize);
```

##### 独立実行 CLI バイナリ仕様
独立CLIバイナリは以下のサブコマンドを実装します：
- `info`: プラグイン情報・対応拡張子を JSON で stdout に出力
- `metadata <FILE_PATH>`: 幅・高さ・フォーマットを JSON（`{"width": 1200, "height": 800, "format": "clip"}`）で stdout に出力
- `preview <FILE_PATH> <OUT_PATH>`: プレビュー画像をファイルへ出力
- `preview-pipe <FILE_PATH>`: プレビュー画像の生バイト列を stdout に直接出力

#### B. `binary_slice` の設定パラメータ
- `start_signature`: 切り出し開始バイト列（16進文字列またはASCII。例: `"89504E470D0A1A0A"`）。
- `end_signature`: 切り出し終了バイト列（例: `"49454E44AE426082"`）。
- `include_end_signature`: 終端バイト列を含めるか否か（既定値: `true`）。
- `mime_type`: 抽出画像のMIMEタイプ（既定値: `"image/png"`）。

#### C. `command` の設定パラメータ
- `command`: 実行するコマンド名またはプラグインディレクトリ相対パス。
- `args`: コマンドライン引数配列（`"{input}"` が元ファイルパス、`"{output}"` が出力先一時画像パスへ置換）。
- `output_format`: 出力画像形式（`"png"`, `"webp"`, `"jpeg"` 等、既定値: `"png"`）。

---

## 3. スキャナーの動的走査拡張と自動再走査

1. **プラグイン配置場所**:
   - アプリインストール先ディレクトリ: `<InstallDir>\plugins\<plugin_name>\plugin.json`
   - ユーザーデータディレクトリ: `%APPDATA%\com.imagemediaviewer.desktop\plugins\<plugin_name>\plugin.json`
2. **動的登録と走査合流**:
   - アプリ起動時および設定画面保存時、上記ディレクトリを自動スキャンします。
   - `app_settings.json` の `enabled_plugins` に記載されたプラグインが有効化されます。
   - 有効化された全プラグインの `extensions`（例: `["clip"]`）がディレクトリ監視スキャナー（`walker.rs`）へ動的に合流します。
3. **新規拡張子の自動再走査**:
   - 新しい拡張子が追加された場合、登録済み監視フォルダに対してバックグラウンドで自動的に再走査がトリガーされ、該当拡張子のファイルがカタログDBへ自動取り込みされます。

---

## 4. ユーザーによる新規プラグインの追加手順（再ビルド不要）

本体のソースコード修正や再コンパイル（`cargo build`）は**一切不要**です。

1. `plugins/clipstudio/` または `plugins/sample_format/` をコピーし、`plugins/my_format/` を作成します。
   （ユーザー固有プラグインとして `%APPDATA%\com.imagemediaviewer.desktop\plugins\my_format\` に配置しても構いません）
2. `plugins/my_format/plugin.json` を開き、対象拡張子（`extensions`）、抽出方式（`extractor`）、および右クリックアクション（`actions`）を編集します。
3. 設定ファイル（`app_settings.json`）の `enabled_plugins` にプラグインの ID（例: `"format.my_format"`）を追加、またはUI設定から有効化します。
4. 即座に対象拡張子のファイルがスキャンされ、タイムラインやビューワで閲覧可能になります。

---

## 5. 検証・テスト実行手順

プラグインやランタイムの変更後は、以下のコマンドで動作検証を行います。

```bash
# 1. 独立プラグインバイナリビルド（cdylib + CLI bin を release ビルドして plugins/ 配下へ自動同期）
npm run build:plugins

# 2. バックエンド単体テスト
cd src-tauri
cargo test

# 3. フロントエンド単体テスト
npm test

# 4. 本番Webアセットビルド
npm run build

# 5. Windowsインストーラー生成（プラグインバイナリ同梱）
npm run build:installer

# 6. Androidパッケージビルド
npm run build:android:release
```
