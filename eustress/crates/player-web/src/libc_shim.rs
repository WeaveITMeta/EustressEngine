//! The libc entry points Luau reaches that the browser has no host for.
//!
//! The Luau VM (mlua's `mlua-sys`) is C++ built against wasi-sysroot's libc
//! and libc++ (`build.rs`, `scripts/wasm-luau-env.sh`). wasi-libc leaves a
//! few calls to a WASI host: time, the environment, and the standard streams.
//! A web page has no WASI host, so this module defines each one here, and the
//! linker resolves them inside the module instead of importing them. The
//! module then imports nothing but what wasm-bindgen's glue gives it.
//!
//! - Time is real: the monotonic clock and `clock()` read the page's
//!   performance clock, and the realtime clock reads the wall clock (both
//!   through `web_time`). Luau's `os.clock` and its GC pacing read `clock()`,
//!   in nanoseconds (wasi-libc's `CLOCKS_PER_SEC`).
//! - There is no environment and no file descriptor. Writes to stdout and
//!   stderr are counted as written and dropped: the Player's scripts print
//!   through the tree's Output, never through libc.
//! - `proc_exit` traps. Nothing in Luau calls `exit`.

use std::sync::OnceLock;

use web_time::{Instant, SystemTime, UNIX_EPOCH};

/// WASI `errno` for "bad file descriptor".
const ERRNO_BADF: i32 = 8;
/// WASI `errno` for "invalid argument".
const ERRNO_INVAL: i32 = 28;
/// WASI clock ids.
const CLOCK_REALTIME: i32 = 0;

fn since_start() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    let start = *START.get_or_init(Instant::now);
    u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

/// `clock()`: the processor time used, in `CLOCKS_PER_SEC` (nanoseconds).
/// A page has one thread, so its monotonic time since start stands in.
#[no_mangle]
pub extern "C" fn clock() -> i64 {
    since_start() as i64
}

#[no_mangle]
pub unsafe extern "C" fn __imported_wasi_snapshot_preview1_clock_time_get(
    id: i32,
    _precision: i64,
    out: *mut u64,
) -> i32 {
    if out.is_null() {
        return ERRNO_INVAL;
    }
    let now = if id == CLOCK_REALTIME {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
            .unwrap_or(0)
    } else {
        since_start()
    };
    out.write_unaligned(now);
    0
}

#[no_mangle]
pub unsafe extern "C" fn __imported_wasi_snapshot_preview1_environ_sizes_get(
    count: *mut u32,
    size: *mut u32,
) -> i32 {
    if count.is_null() || size.is_null() {
        return ERRNO_INVAL;
    }
    count.write_unaligned(0);
    size.write_unaligned(0);
    0
}

#[no_mangle]
pub extern "C" fn __imported_wasi_snapshot_preview1_environ_get(_environ: i32, _buf: i32) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn __imported_wasi_snapshot_preview1_fd_close(_fd: i32) -> i32 {
    ERRNO_BADF
}

#[no_mangle]
pub extern "C" fn __imported_wasi_snapshot_preview1_fd_fdstat_get(_fd: i32, _out: i32) -> i32 {
    ERRNO_BADF
}

#[no_mangle]
pub extern "C" fn __imported_wasi_snapshot_preview1_fd_seek(
    _fd: i32,
    _offset: i64,
    _whence: i32,
    _out: i32,
) -> i32 {
    ERRNO_BADF
}

#[repr(C)]
pub struct Ciovec {
    buf: *const u8,
    len: usize,
}

#[no_mangle]
pub unsafe extern "C" fn __imported_wasi_snapshot_preview1_fd_write(
    _fd: i32,
    iovs: *const Ciovec,
    count: usize,
    written: *mut usize,
) -> i32 {
    if written.is_null() || (count > 0 && iovs.is_null()) {
        return ERRNO_INVAL;
    }
    let total = (0..count).map(|i| (*iovs.add(i)).len).sum();
    written.write_unaligned(total);
    0
}

#[no_mangle]
pub extern "C" fn __imported_wasi_snapshot_preview1_proc_exit(_code: i32) -> ! {
    core::arch::wasm32::unreachable()
}
