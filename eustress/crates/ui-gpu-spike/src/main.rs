//! Spike, not part of Studio. See `README.md` in this crate.

#[cfg(feature = "spike")]
mod spike;

#[cfg(feature = "spike")]
fn main() {
    spike::run();
}

#[cfg(not(feature = "spike"))]
fn main() {
    eprintln!(
        "ui-gpu-spike was built without its `spike` feature. Build it with:\n  \
         cargo build --release -p eustress-ui-gpu-spike --features spike"
    );
}
