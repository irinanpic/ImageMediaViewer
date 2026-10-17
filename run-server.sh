#!/bin/bash
# ImageMediaViewer Launcher for macOS / Linux
set -e

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [ -f "$DIR/image-media-viewer" ]; then
    "$DIR/image-media-viewer" "$@"
elif [ -f "$DIR/src-tauri/target/release/image-media-viewer" ]; then
    "$DIR/src-tauri/target/release/image-media-viewer" "$@"
elif [ -f "$DIR/src-tauri/target/debug/image-media-viewer" ]; then
    "$DIR/src-tauri/target/debug/image-media-viewer" "$@"
else
    echo "ImageMediaViewer 実行ファイルが見つかりません。ビルドを実行してください。"
    echo "実行コマンド: npm run build"
    exit 1
fi
