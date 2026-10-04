# ImageMediaViewer Desktop Launcher
# Reliably starts backend and opens client with standalone window or default browser fallback

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$backendExe = Join-Path $scriptDir "src-tauri\target\debug\image-media-viewer.exe"
$appUrl = "http://127.0.0.1:14201/"

Write-Host "====================================================" -ForegroundColor Cyan
Write-Host "       ImageMediaViewer Desktop Launcher            " -ForegroundColor Cyan
Write-Host "====================================================" -ForegroundColor Cyan

# 1. Start backend server and wait for HTTP readiness
$running = Get-Process -Name "image-media-viewer" -ErrorAction SilentlyContinue
if (-not $running) {
    Write-Host "[1/2] Starting backend server..." -ForegroundColor Yellow
    Start-Process -FilePath $backendExe -WindowStyle Hidden
}

$serverReady = $false
for ($i = 0; $i -lt 10; $i++) {
    try {
        $res = Invoke-RestMethod -Uri "http://127.0.0.1:14201/api/heartbeat" -Method Post -TimeoutSec 1 -ErrorAction Stop
        if ($res.alive) {
            $serverReady = $true
            break
        }
    } catch {
        Start-Sleep -Milliseconds 400
    }
}

if ($serverReady) {
    Write-Host "[1/2] Backend server is ready (Port 14201)" -ForegroundColor Green
} else {
    Write-Host "[1/2] Connecting to backend server..." -ForegroundColor DarkGray
}

# 2. Launch Client Window
Write-Host "[2/2] Launching client..." -ForegroundColor Yellow

# Restore Window State
$windowStateFile = Join-Path $scriptDir "window_state.json"
$extraArgs = @()

if (Test-Path $windowStateFile) {
    try {
        $raw = Get-Content $windowStateFile -Raw -Encoding UTF8
        $json = $raw | ConvertFrom-Json
        if ($json.isMaximized) {
            $extraArgs += "--start-maximized"
            Write-Host "   -> Restoring window: Maximized" -ForegroundColor DarkGray
        } else {
            $w = 1280
            $h = 850
            if ($json.width -and [int]$json.width -gt 400) {
                $w = [int]$json.width
            }
            if ($json.height -and [int]$json.height -gt 300) {
                $h = [int]$json.height
            }
            $sizeStr = "{0},{1}" -f $w, $h
            $extraArgs += "--window-size=$sizeStr"
            Write-Host "   -> Restoring size: $w x $h" -ForegroundColor DarkGray

            if ($null -ne $json.x -and $null -ne $json.y) {
                $posStr = "{0},{1}" -f [int]$json.x, [int]$json.y
                $extraArgs += "--window-position=$posStr"
                Write-Host "   -> Restoring position: ($($json.x), $($json.y))" -ForegroundColor DarkGray
            }
        }
    } catch {}
}

# Search for browser candidates for standalone app window
$browserCandidates = @(
    (Join-Path $env:ProgramFiles "Google\Chrome\Application\chrome.exe"),
    (Join-Path ${env:ProgramFiles(x86)} "Google\Chrome\Application\chrome.exe"),
    (Join-Path ${env:ProgramFiles(x86)} "Microsoft\Edge\Application\msedge.exe"),
    (Join-Path $env:ProgramFiles "Microsoft\Edge\Application\msedge.exe")
)

$launched = $false
foreach ($b in $browserCandidates) {
    if (Test-Path $b) {
        try {
            $rand = Get-Random
            $tempProfile = Join-Path $env:TEMP "IMV_Profile_$rand"
            $appArgs = @(
                "--app=$appUrl",
                "--user-data-dir=$tempProfile",
                "--no-first-run",
                "--no-default-browser-check"
            ) + $extraArgs

            $p = Start-Process -FilePath $b -ArgumentList $appArgs -PassThru -ErrorAction SilentlyContinue
            if ($p) {
                Start-Sleep -Milliseconds 600
                if (-not $p.HasExited) {
                    $launched = $true
                    Write-Host "[OK] Desktop app window launched successfully!" -ForegroundColor Green
                    break
                }
            }
        } catch {}
    }
}

# If standalone app mode fails or no candidate is available,
# reliably open in the user's default browser (e.g. Firefox)
if (-not $launched) {
    Start-Process $appUrl
    Write-Host "[OK] Opened in default browser: $appUrl" -ForegroundColor Green
}