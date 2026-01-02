# Eustress Monorepo - Build & Run Script
# Quick commands for common tasks

param(
    [Parameter(Position=0)]
    [ValidateSet("engine", "client", "both", "check", "test", "clean")]
    [string]$Target = "engine",
    
    [switch]$Release,
    [switch]$Watch,
    [switch]$Features
)

$ErrorActionPreference = "Stop"

Write-Host "🎮 Eustress Monorepo Build Script" -ForegroundColor Cyan
Write-Host "=================================" -ForegroundColor Cyan
Write-Host ""

function Build-Target {
    param($BinName, $Profile)
    
    $cmd = "cargo"
    $args = @("build")
    
    if ($Profile -eq "release") {
        $args += "--release"
    }
    
    $args += "--bin", $BinName
    
    Write-Host "🔨 Building $BinName ($Profile)..." -ForegroundColor Yellow
    & $cmd $args
    
    if ($LASTEXITCODE -eq 0) {
        Write-Host "✅ $BinName built successfully!" -ForegroundColor Green
        return $true
    } else {
        Write-Host "❌ Build failed!" -ForegroundColor Red
        return $false
    }
}

function Run-Target {
    param($BinName, $Profile)
    
    $cmd = "cargo"
    $args = @("run", "--bin", $BinName)
    
    if ($Profile -eq "release") {
        $args += "--release"
    }
    
    Write-Host "🚀 Running $BinName..." -ForegroundColor Green
    & $cmd $args
}

# Determine profile
$profile = if ($Release) { "release" } else { "debug" }

# Execute based on target
switch ($Target) {
    "engine" {
        if (Build-Target "eustress-engine" $profile) {
            Run-Target "eustress-engine" $profile
        }
    }
    
    "client" {
        if (Build-Target "eustress-client" $profile) {
            Run-Target "eustress-client" $profile
        }
    }
    
    "both" {
        Write-Host "Building both binaries..." -ForegroundColor Cyan
        cargo build --workspace $(if ($Release) { "--release" })
    }
    
    "check" {
        Write-Host "🔍 Checking workspace..." -ForegroundColor Cyan
        cargo check --workspace
    }
    
    "test" {
        Write-Host "🧪 Running tests..." -ForegroundColor Cyan
        cargo test --workspace
    }
    
    "clean" {
        Write-Host "🧹 Cleaning workspace..." -ForegroundColor Yellow
        cargo clean
        Write-Host "✅ Clean complete!" -ForegroundColor Green
    }
}

Write-Host ""
Write-Host "📖 Quick Reference:" -ForegroundColor Cyan
Write-Host "  .\build-and-run.ps1 engine          # Run engine (studio)" -ForegroundColor Gray
Write-Host "  .\build-and-run.ps1 client          # Run client" -ForegroundColor Gray
Write-Host "  .\build-and-run.ps1 engine -Release # Release build" -ForegroundColor Gray
Write-Host "  .\build-and-run.ps1 check           # Check all crates" -ForegroundColor Gray
Write-Host "  .\build-and-run.ps1 test            # Run all tests" -ForegroundColor Gray
