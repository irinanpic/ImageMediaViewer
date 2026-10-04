# ImageMediaViewer PowerShell ランチャー
# 変更理由: 前回の画面サイズ・位置・最大化状態を忠実に復元しネイティブアプリ同様の使い心地を提供

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$backendExe = Join-Path $scriptDir "src-tauri\target\debug\image-media-viewer.exe"
$appUrl = "http://127.0.0.1:14201/"

Write-Host "====================================================" -ForegroundColor Cyan
Write-Host "       ImageMediaViewer Desktop Launcher            " -ForegroundColor Cyan
Write-Host "====================================================" -ForegroundColor Cyan

# 1. バックエンド起動確認
$running = Get-Process -Name "image-media-viewer" -ErrorAction SilentlyContinue
if (-not $running) {
    Write-Host "[1/2] バックエンドサーバーを起動中..." -ForegroundColor Yellow
    Start-Process -FilePath $backendExe -WindowStyle Hidden
    Start-Sleep -Seconds 1
} else {
    Write-Host "[1/2] バックエンドサーバー稼働中 (ポート 14201)" -ForegroundColor Green
}

# 2. 独立デスクトップウィンドウ起動
Write-Host "[2/2] 独立ウィンドウを起動中..." -ForegroundColor Yellow

$pf86 = [System.Environment]::GetEnvironmentVariable("ProgramFiles(x86)")
$pf = [System.Environment]::GetEnvironmentVariable("ProgramFiles")
$rand = Get-Random
$userData = Join-Path $env:TEMP "IMV_Profile_$rand"

$browserCandidates = @(
    (Join-Path $pf "Google\Chrome\Application\chrome.exe"),
    (Join-Path $pf86 "Google\Chrome\Application\chrome.exe"),
    (Join-Path $env:LocalAppData "Google\Chrome\Application\chrome.exe"),
    (Join-Path $pf86 "Microsoft\Edge\Application\msedge.exe"),
    (Join-Path $pf "Microsoft\Edge\Application\msedge.exe"),
    (Join-Path $env:LocalAppData "Microsoft\Edge\Application\msedge.exe")
)

$targetBrowser = $null
foreach ($c in $browserCandidates) {
    if (Test-Path $c) {
        $targetBrowser = $c
        break
    }
}

# 3. 前回のウィンドウサイズ・位置・最大化状態を復元
$windowStateFile = Join-Path $scriptDir "window_state.json"
$appArgs = @(
    "--app=$appUrl",
    "--user-data-dir=$userData",
    "--no-first-run",
    "--no-default-browser-check"
)

if (Test-Path $windowStateFile) {
    try {
        $raw = Get-Content $windowStateFile -Raw -Encoding UTF8
        $json = $raw | ConvertFrom-Json
        if ($json.isMaximized) {
            $appArgs += "--start-maximized"
            Write-Host "   -> 前回状態を復元: 最大化" -ForegroundColor DarkGray
        } else {
            $w = if ($json.width -and $json.width -gt 400) { [int]$json.width } else { 1280 }
            $h = if ($json.height -and $json.height -gt 300) { [int]$json.height } else { 850 }
            $appArgs += "--window-size=$w,$h"
            Write-Host "   -> 前回サイズを復元: ${w}x${h}" -ForegroundColor DarkGray

            if ($null -ne $json.x -and $null -ne $json.y) {
                $appArgs += "--window-position=$($json.x),$($json.y)"
                Write-Host "   -> 前回位置を復元: ($($json.x), $($json.y))" -ForegroundColor DarkGray
            }
        }
    } catch {
        $appArgs += "--window-size=1280,850"
    }
} else {
    $appArgs += "--window-size=1280,850"
}

if ($targetBrowser) {
    Start-Process -FilePath $targetBrowser -ArgumentList $appArgs
    Write-Host "[OK] 独立ウィンドウを起動しました！" -ForegroundColor Green
} else {
    Start-Process $appUrl
    Write-Host "[OK] 標準ブラウザで開きました: $appUrl" -ForegroundColor Green
}
