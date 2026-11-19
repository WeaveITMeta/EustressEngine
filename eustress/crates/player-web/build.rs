//! Linking the Luau VM's C and C++ runtime into the browser build.
//!
//! mlua's Luau is C++. For `wasm32-unknown-unknown` it is compiled against
//! wasi-sysroot (the compile flags are `eustress/scripts/wasm-luau-env.sh`),
//! and the module links that sysroot's libc++, libc++abi, libunwind (Luau
//! throws C++ exceptions, as WebAssembly exceptions) and libc. The few calls
//! libc leaves to a WASI host are defined in `src/libc_shim.rs`.
//!
//! The sysroot is `WASI_SYSROOT`, or `.tools/wasi-sysroot-34.0` at the
//! repository root. A native build of this crate links nothing here.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=WASI_SYSROOT");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() != Ok("wasm32") {
        return;
    }
    let sysroot = std::env::var_os("WASI_SYSROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../.tools/wasi-sysroot-34.0")
        });
    let lib = sysroot.join("lib").join("wasm32-wasip1");
    let eh = lib.join("eh");
    for needed in [eh.join("libc++.a"), eh.join("libc++abi.a"), eh.join("libunwind.a"), lib.join("libc.a")] {
        if !needed.is_file() {
            panic!(
                "the browser Player links Luau against wasi-sysroot, and {} is missing. \
                 Unpack wasi-sysroot-34.0 (github.com/WebAssembly/wasi-sdk, release wasi-sdk-34) \
                 to .tools/ at the repository root or set WASI_SYSROOT, then build through \
                 eustress/scripts/wasm-luau-env.sh (see docs/launch/BROWSER_PLAYER_STAGING.md)",
                needed.display()
            );
        }
    }
    println!("cargo:rustc-link-search=native={}", eh.display());
    println!("cargo:rustc-link-search=native={}", lib.display());
    for name in ["c++", "c++abi", "unwind", "c"] {
        println!("cargo:rustc-link-lib={name}");
    }
}
