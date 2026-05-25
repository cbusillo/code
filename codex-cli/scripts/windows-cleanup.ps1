<#
Helper to recover from EBUSY/EPERM during global npm upgrades on Windows.
Closes running processes and removes stale package folders.

Usage (PowerShell):
  Set-ExecutionPolicy -Scope Process Bypass -Force
  ./codex-cli/scripts/windows-cleanup.ps1
#>

$ErrorActionPreference = 'SilentlyContinue'

Write-Host "Stopping running Code/Coder processes..."
taskkill /IM code-x86_64-pc-windows-msvc.exe /F 2>$null | Out-Null
taskkill /IM code.exe /F 2>$null | Out-Null
taskkill /IM coder.exe /F 2>$null | Out-Null

Write-Host "Removing old global package (if present)..."
$npmRoot = (& npm root -g).Trim()
$pkgPath = Join-Path $npmRoot "@just-every\code"
if (Test-Path $pkgPath) {
  try { Remove-Item -LiteralPath $pkgPath -Recurse -Force -ErrorAction Stop } catch {}
}

Write-Host "Removing temp staging directories (if present)..."
Get-ChildItem -LiteralPath (Join-Path $npmRoot "@just-every") -Force -ErrorAction SilentlyContinue |
  Where-Object { $_.Name -like '.code-*' } |
  ForEach-Object {
    try { Remove-Item -LiteralPath $_.FullName -Recurse -Force -ErrorAction Stop } catch {}
  }

Write-Host "Cleanup complete. Install the current GitHub Release or run: code update --yes"
