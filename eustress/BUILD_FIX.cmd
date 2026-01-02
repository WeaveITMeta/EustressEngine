@echo off
echo ========================================
echo EUSTRESS CLIENT - ONE-SHOT BUILD
echo ========================================
echo.

echo Stopping all Rust processes...
taskkill /F /IM cargo.exe 2>nul
taskkill /F /IM rustc.exe 2>nul
timeout /t 2 /nobreak >nul

echo Cleaning target directory...
if exist target rmdir /s /q target
timeout /t 1 /nobreak >nul

echo.
echo Building client (single-threaded to avoid locks)...
echo This will take 10-15 minutes. DO NOT close this window.
echo.

set CARGO_INCREMENTAL=0
set CARGO_BUILD_JOBS=1
cargo build --bin eustress-client -j 1

if %errorlevel% equ 0 (
    echo.
    echo ========================================
    echo BUILD SUCCESSFUL!
    echo ========================================
    echo.
    echo Run the client:
    echo   target\debug\eustress-client.exe
    echo.
    pause
) else (
    echo.
    echo ========================================
    echo BUILD FAILED
    echo ========================================
    echo.
    echo If you still get Error 32:
    echo 1. Run PowerShell as Administrator
    echo 2. Run: Add-MpPreference -ExclusionPath "E:\Workspace\EustressEngine"
    echo 3. Run this script again
    echo.
    pause
)
