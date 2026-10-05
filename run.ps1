# ImageMediaViewer Desktop Launcher
# 完全ポートレス一体型ネイティブデスクトップアプリを起動します

[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$scriptDir = if ($PSScriptRoot) { $PSScriptRoot } elseif ($MyInvocation.MyCommand.Path) { Split-Path -Parent $MyInvocation.MyCommand.Path } else { (Get-Location).Path }
# 存在する実行ファイル候補の中から、更新日時が最も新しいものを選択
$candidatePaths = @(
    (Join-Path $scriptDir "src-tauri\target\release\image-media-viewer.exe"),
    (Join-Path $scriptDir "src-tauri\target\debug\image-media-viewer.exe"),
    (Join-Path $scriptDir "image-media-viewer.exe")
)

$backendExe = $candidatePaths | Where-Object { Test-Path $_ } | Sort-Object { (Get-Item $_).LastWriteTime } -Descending | Select-Object -First 1

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