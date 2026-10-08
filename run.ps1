# ImageMediaViewer Desktop Launcher
# 完全ポートレス一体型ネイティブデスクトップアプリを起動します

[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$scriptDir = if ($PSScriptRoot) { $PSScriptRoot } elseif ($MyInvocation.MyCommand.Path) { Split-Path -Parent $MyInvocation.MyCommand.Path } else { (Get-Location).Path }

# 実行ファイル候補の探索
# 注意: cargo test や cargo build (debug) で生成される debug バイナリは
# アセットが埋め込まれず devUrl (127.0.0.1:1420) を参照するため、
# 本番・アセット埋め込み済みの release バイナリを最優先で選択します。
$releaseCandidates = @(
    (Join-Path $scriptDir "image-media-viewer.exe"),
    (Join-Path $scriptDir "src-tauri\target\release\image-media-viewer.exe")
)

$backendExe = $releaseCandidates | Where-Object { Test-Path $_ } | Sort-Object { (Get-Item $_).LastWriteTime } -Descending | Select-Object -First 1

if (-not $backendExe) {
    $debugExe = Join-Path $scriptDir "src-tauri\target\debug\image-media-viewer.exe"
    if (Test-Path $debugExe) {
        $backendExe = $debugExe
    }
}

if (-not $backendExe -or -not (Test-Path $backendExe)) {
    Write-Host "アプリケーション実行ファイルが見つかりません。ビルドを実行してください。" -ForegroundColor Red
    Write-Host "実行コマンド: npm run build:installer" -ForegroundColor Yellow
    exit 1
}

$existing = Get-Process -Name "image-media-viewer" -ErrorAction SilentlyContinue
if ($existing) {
    Write-Host "ImageMediaViewer は既に起動しています。既存ウィンドウを最前面に復元します..." -ForegroundColor Yellow
} else {
    Write-Host "ImageMediaViewer を起動しています: $backendExe" -ForegroundColor Cyan
}

Start-Process -FilePath $backendExe