#!/usr/bin/env bash
# ImageMediaViewer Server-only Background Launcher (macOS / Linux)
# 変更理由: クライアントウィンドウを多重起動せず、サーバー単体のみを常駐起動する

set -e
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# 既に起動中か確認
if pgrep -f "image-media-viewer" > /dev/null 2>&1; then
    echo "[ImageMediaViewer] Server is already running."
    exit 0
fi

# デバッグバイナリまたはリリースバイナリの存在確認と起動
if [ -f "src-tauri/target/debug/image-media-viewer" ]; then
    nohup "./src-tauri/target/debug/image-media-viewer" > /dev/null 2>&1 &
    echo "[ImageMediaViewer] Server started (debug binary)."
    exit 0
elif [ -f "src-tauri/target/release/image-media-viewer" ]; then
    nohup "./src-tauri/target/release/image-media-viewer" > /dev/null 2>&1 &
    echo "[ImageMediaViewer] Server started (release binary)."
    exit 0
elif [ -f "./image-media-viewer" ]; then
    nohup "./image-media-viewer" > /dev/null 2>&1 &
    echo "[ImageMediaViewer] Server started."
    exit 0
else
    echo "[ERROR] image-media-viewer binary not found. Please build the project first." >&2
    exit 1
fi
