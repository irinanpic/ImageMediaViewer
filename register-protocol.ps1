# ImageMediaViewer Custom URI Protocol (imagemediaviewer://) Registration Script
# Register custom URI protocol so browsers and links can launch the backend server directly.

$scriptDir = if ($PSScriptRoot) { $PSScriptRoot } elseif ($MyInvocation.MyCommand.Path) { Split-Path -Parent $MyInvocation.MyCommand.Path } else { (Get-Location).Path }
$runServerBat = Join-Path $scriptDir "run-server.bat"

Write-Host "Registering ImageMediaViewer URI Protocol (imagemediaviewer://)..." -ForegroundColor Cyan

$regKey = "HKCU:\Software\Classes\imagemediaviewer"
New-Item -Path $regKey -Force | Out-Null
Set-ItemProperty -Path $regKey -Name "(Default)" -Value "URL:ImageMediaViewer Protocol" | Out-Null
Set-ItemProperty -Path $regKey -Name "URL Protocol" -Value "" | Out-Null

$iconPath = Join-Path $scriptDir "src-tauri\icons\icon.ico"
if ($iconPath -and (Test-Path $iconPath)) {
    $iconKey = "$regKey\DefaultIcon"
    New-Item -Path $iconKey -Force | Out-Null
    Set-ItemProperty -Path $iconKey -Name "(Default)" -Value ('"{0}",0' -f $iconPath) | Out-Null
}

$cmdKey = "$regKey\shell\open\command"
New-Item -Path $cmdKey -Force | Out-Null

# Direct batch execution to prevent cmd.exe quote-stripping issues
$cmdValue = '"{0}" "%1"' -f $runServerBat
Set-ItemProperty -Path $cmdKey -Name "(Default)" -Value $cmdValue | Out-Null

Write-Host "[OK] Registered URI Protocol: imagemediaviewer://" -ForegroundColor Green
Write-Host "     Command: $cmdValue" -ForegroundColor Gray
Write-Host "     You can now launch the server from browsers or links." -ForegroundColor Green