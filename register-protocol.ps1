# ImageMediaViewer Custom URI Protocol (imagemediaviewer://) Registration Script
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$runBat = Join-Path $scriptDir "run.bat"

Write-Host "Registering ImageMediaViewer URI Protocol (imagemediaviewer://)..." -ForegroundColor Cyan

$regKey = "HKCU:\Software\Classes\imagemediaviewer"
New-Item -Path $regKey -Force | Out-Null
Set-ItemProperty -Path $regKey -Name "(Default)" -Value "URL:ImageMediaViewer Protocol" | Out-Null
Set-ItemProperty -Path $regKey -Name "URL Protocol" -Value "" | Out-Null

$cmdKey = "$regKey\shell\open\command"
New-Item -Path $cmdKey -Force | Out-Null
$cmdValue = "`"$runBat`" `"%1`""
Set-ItemProperty -Path $cmdKey -Name "(Default)" -Value $cmdValue | Out-Null

Write-Host "[OK] Registered URI Protocol: imagemediaviewer://" -ForegroundColor Green
Write-Host "     You can now launch the server from browsers or links." -ForegroundColor Green
