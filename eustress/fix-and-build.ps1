#!/usr/bin/env pwsh
# Fix file locking and build client

Write-Host "🔧 Fixing Windows file locking issues..." -ForegroundColor Cyan

# Kill any lingering cargo processes
Write-Host "Killing cargo processes..."
Get-Process -Name "cargo" -ErrorAction SilentlyContinue | Stop-Process -Force
Get-Process -Name "rustc" -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 2

# Add Windows Defender exclusion (requires admin)
Write-Host ""
Write-Host "Adding Windows Defender exclusion for target directory..."
Write-Host "(This requires Administrator privileges - if it fails, run PowerShell as Admin)"
try {
    Add-MpPreference -ExclusionPath "$PSScriptRoot\target" -ErrorAction Stop
    Write-Host "✅ Windows Defender exclusion added" -ForegroundColor Green
} catch {
    Write-Host "⚠️  Could not add exclusion - you may need to run as Administrator" -ForegroundColor Yellow
    Write-Host "   Or manually add: $PSScriptRoot\target" -ForegroundColor Yellow
}

# Clean everything
Write-Host ""
Write-Host "Cleaning build artifacts..."
if (Test-Path "target") {
    Remove-Item -Path "target" -Recurse -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 1
}

# Build with optimizations to avoid file locks
Write-Host ""
Write-Host "Building eustress-client (this will take 5-10 minutes)..." -ForegroundColor Cyan
Write-Host ""

$env:CARGO_INCREMENTAL = "0"  # Disable incremental compilation
$env:RUSTFLAGS = "-C codegen-units=1"  # Single codegen unit reduces file handles

cargo build --bin eustress-client --release

if ($LASTEXITCODE -eq 0) {
    Write-Host ""
    Write-Host "✅ BUILD SUCCESSFUL!" -ForegroundColor Green
    Write-Host ""
    Write-Host "Run the client with:" -ForegroundColor Cyan
    Write-Host "  cargo run --bin eustress-client --release" -ForegroundColor Yellow
    Write-Host ""
    Write-Host "Or directly:" -ForegroundColor Cyan
    Write-Host "  .\target\release\eustress-client.exe" -ForegroundColor Yellow
} else {
    Write-Host ""
    Write-Host "❌ Build failed" -ForegroundColor Red
    Write-Host ""
    Write-Host "If you keep getting Error 32, try:" -ForegroundColor Yellow
    Write-Host "1. Run this script as Administrator"
    Write-Host "2. Temporarily disable Windows Defender Real-time Protection"
    Write-Host "3. Reboot and try again"
    Write-Host "4. Use WSL2: wsl cargo build --bin eustress-client --release"
}
