#!/usr/bin/env bash
# Source this before a wasm32 build of eustress-player-web:
#
#   source eustress/scripts/wasm-luau-env.sh
#   cargo build --release --target wasm32-unknown-unknown -p eustress-player-web --features webgpu
#
# mlua's Luau VM is C++. clang has no C or C++ standard library of its own for
# wasm32-unknown-unknown, so the VM is compiled against wasi-sysroot (libc,
# libc++), with C++ exceptions as WebAssembly exceptions, because Luau raises
# script errors as C++ exceptions. Only the wasm32-unknown-unknown target's
# C and C++ flags change; a native build is untouched.
#
# The sysroot: WASI_SYSROOT, or .tools/wasi-sysroot-34.0 at the repository
# root (wasi-sysroot-34.0.tar.gz from github.com/WebAssembly/wasi-sdk, release
# wasi-sdk-34; only include/wasm32-wasip1 and lib/wasm32-wasip1 are needed).
# clang: LLVM 22 or later on PATH, or WASM_CLANG_DIR.

_luau_env_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
export WASI_SYSROOT="${WASI_SYSROOT:-$_luau_env_root/.tools/wasi-sysroot-34.0}"
# On Windows (Git Bash), clang and rustc read Windows paths, not /e/... ones.
if command -v cygpath >/dev/null 2>&1; then
  WASI_SYSROOT="$(cygpath -m "$WASI_SYSROOT")"
fi
if [[ ! -f "$WASI_SYSROOT/lib/wasm32-wasip1/eh/libc++.a" ]]; then
  echo "wasm-luau-env: no wasi-sysroot at $WASI_SYSROOT (see the header of this file)" >&2
  return 1 2>/dev/null || exit 1
fi
if [[ -n "${WASM_CLANG_DIR:-}" ]]; then
  export PATH="$WASM_CLANG_DIR:$PATH"
elif [[ -d "/c/Program Files/LLVM/bin" ]]; then
  export PATH="/c/Program Files/LLVM/bin:$PATH"
fi

# cc-rs puts its own --target=wasm32-unknown-unknown first; clang takes the
# last --target, so the sysroot's wasm32-wasip1 headers apply. The objects
# link into the wasm32-unknown-unknown module unchanged.
#
# - -fwasm-exceptions with the standard (not legacy) instructions: the
#   sysroot's libc++abi and libunwind are built that way.
# - The libc++ headers are the sysroot's exception-enabled copy (eh/), named
#   explicitly because upstream clang does not pick it on its own.
# - stdlib.h is force-included: Luau's parser calls strtod and strtoull
#   without including it, relying on other standard libraries pulling it in.
_luau_env_flags="--target=wasm32-wasip1 --sysroot=$WASI_SYSROOT -fwasm-exceptions -mllvm -wasm-use-legacy-eh=false"
export CC_wasm32_unknown_unknown=clang
export CXX_wasm32_unknown_unknown=clang++
export AR_wasm32_unknown_unknown=llvm-ar
export CFLAGS_wasm32_unknown_unknown="$_luau_env_flags"
export CXXFLAGS_wasm32_unknown_unknown="$_luau_env_flags -nostdinc++ -isystem $WASI_SYSROOT/include/wasm32-wasip1/eh/c++/v1 -include stdlib.h"
# luau0-src names the C++ library to link; the rest of the runtime is the
# browser Player's build.rs.
export CXXSTDLIB_wasm32_unknown_unknown=c++
unset _luau_env_root _luau_env_flags
