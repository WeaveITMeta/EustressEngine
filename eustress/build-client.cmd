@echo off
echo Cleaning previous build...
cargo clean

echo.
echo Building client (single-threaded to avoid file locks)...
set CARGO_BUILD_JOBS=1
cargo build --bin eustress-client -j 1

echo.
echo Done! Run with: cargo run --bin eustress-client
