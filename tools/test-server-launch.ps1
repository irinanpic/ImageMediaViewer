# test-server-launch.ps1
# Verification test script for ImageMediaViewer URI Protocol & Server Launcher

$ErrorActionPreference = "Stop"
$scriptDir = Split-Path -Parent $PSScriptRoot
Set-Location $scriptDir

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host "  ImageMediaViewer Server & URI Protocol Test             " -ForegroundColor Cyan
Write-Host "==========================================================" -ForegroundColor Cyan

$allPassed = $true

# Test 1: Registry Key Check
Write-Host "`n[Test 1] Checking Registry Key HKCU:\Software\Classes\imagemediaviewer..." -ForegroundColor Yellow
$regKey = "HKCU:\Software\Classes\imagemediaviewer"
$cmdKey = "$regKey\shell\open\command"
if (Test-Path $cmdKey) {
    $cmdVal = (Get-ItemProperty -Path $cmdKey).'(default)'
    Write-Host "  -> [OK] Registry key exists." -ForegroundColor Green
    Write-Host "  -> Command: $cmdVal" -ForegroundColor DarkGray

    if ($cmdVal -like '*run-server.bat*') {
        Write-Host "  -> [OK] Command points to run-server.bat correctly." -ForegroundColor Green
    } else {
        Write-Host "  -> [WARN] Command syntax differs: $cmdVal" -ForegroundColor Magenta
        $allPassed = $false
    }
} else {
    Write-Host "  -> [FAIL] Registry key does not exist." -ForegroundColor Red
    $allPassed = $false
}

# Test 2: run-server.bat Syntax Check
Write-Host "`n[Test 2] Checking run-server.bat execution..." -ForegroundColor Yellow
$runServerBat = Join-Path $scriptDir "run-server.bat"
if (Test-Path $runServerBat) {
    & cmd.exe /c "call `"$runServerBat`" `"imagemediaviewer://launch`""
    $exitCode = $LASTEXITCODE
    if ($exitCode -eq 0) {
        Write-Host "  -> [OK] run-server.bat: Success (ExitCode: $exitCode)" -ForegroundColor Green
    } else {
        Write-Host "  -> [FAIL] run-server.bat failed with ExitCode: $exitCode" -ForegroundColor Red
        $allPassed = $false
    }
} else {
    Write-Host "  -> [FAIL] run-server.bat not found." -ForegroundColor Red
    $allPassed = $false
}

# Test 3: Binary Existence
Write-Host "`n[Test 3] Checking server binaries..." -ForegroundColor Yellow
$relExe = Join-Path $scriptDir "src-tauri\target\release\image-media-viewer.exe"
$dbgExe = Join-Path $scriptDir "src-tauri\target\debug\image-media-viewer.exe"
if (Test-Path $relExe) {
    Write-Host "  -> [OK] Release binary exists: $relExe" -ForegroundColor Green
} elseif (Test-Path $dbgExe) {
    Write-Host "  -> [OK] Debug binary exists: $dbgExe" -ForegroundColor Green
} else {
    Write-Host "  -> [FAIL] Binary not found." -ForegroundColor Red
    $allPassed = $false
}

Write-Host "`n==========================================================" -ForegroundColor Cyan
if ($allPassed) {
    Write-Host "  ALL TESTS PASSED!                                       " -ForegroundColor Green
} else {
    Write-Host "  SOME TESTS FAILED!                                      " -ForegroundColor Red
}
Write-Host "==========================================================" -ForegroundColor Cyan
exit $(if ($allPassed) { 0 } else { 1 })