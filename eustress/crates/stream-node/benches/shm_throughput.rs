//! # Shared Memory Ring Buffer throughput benchmarks
//!
//! Round 6: cross-platform IPC via mmap'd file.
//! No sockets, no TCP/IP stack — just atomic head pointer + memcpy.
//!
//! ## What is measured
//!
//! - **Publish latency**: time to write one message into the ring (producer side only)
//! - **Roundtrip (local)**: write + immediate read-back in the same process
//! - **Batch publish**: N messages, one atomic store at the end
//!
//! Note: SHM publish does NOT wait for an ack. The metric is pure write throughput.
//! Compare against TCP/QUIC RTT numbers by interpreting throughput: if TCP sequential
//! achieves 10K msg/s (~100 µs/msg), and SHM achieves 10M msg/s (~0.1 µs/msg),
//! SHM is ~1000× faster for same-host publish.
//!
//! Run with:
//! ```
//! cargo bench -p eustress-stream-node --bench shm_throughput
//! ```

use std::hint::black_box;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use eustress_stream_node::shm::ShmChannel;

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

const RING_SIZE: usize = 64 * 1024 * 1024; // 64 MiB

// ─────────────────────────────────────────────────────────────────────────────
// Publish-only (no consumer, no ack) — pure write throughput
// ─────────────────────────────────────────────────────────────────────────────

fn bench_shm_publish_no_consumer(c: &mut Criterion) {
    let mut ch = ShmChannel::new(RING_SIZE).expect("create SHM channel");
    let payload_100b = vec![0u8; 100];
    let payload_1k   = vec![0u8; 1024];

    let mut group = c.benchmark_group("shm");
    group.throughput(Throughput::Elements(1));

    group.bench_function("publish_100b_no_consumer", |b| {
        b.iter(|| {
            // Re-open the ring if full (drain first).
            let mut producer = ch.producer();
            match producer.publish(&payload_100b) {
                Ok(off) => black_box(off),
                Err(_) => {
                    // Ring full — drain and retry.
                    ch.consumer().poll(|_| {});
                    ch.producer().publish(&payload_100b).unwrap_or(0)
                }
            }
        });
    });

    group.bench_function("publish_1k_no_consumer", |b| {
        b.iter(|| {
            let mut producer = ch.producer();
            match producer.publish(&payload_1k) {
                Ok(off) => black_box(off),
                Err(_) => {
                    ch.consumer().poll(|_| {});
                    ch.producer().publish(&payload_1k).unwrap_or(0)
                }
            }
        });
    });

    group.finish();
}

// ─────────────────────────────────────────────────────────────────────────────
// Publish + immediate poll (same-process roundtrip, no network)
// ─────────────────────────────────────────────────────────────────────────────

fn bench_shm_roundtrip(c: &mut Criterion) {
    let mut ch = ShmChannel::new(RING_SIZE).expect("create SHM channel");
    let payload = vec![0u8; 100];

    let mut group = c.benchmark_group("shm");
    group.throughput(Throughput::Elements(1));

    group.bench_function("roundtrip_100b", |b| {
        b.iter(|| {
            ch.producer().publish(&payload).expect("publish");
            let mut count = 0usize;
            ch.consumer().poll(|data| {
                black_box(data);
                count += 1;
            });
            black_box(count)
        });
    });

    group.finish();
}

// ─────────────────────────────────────────────────────────────────────────────
// Batch publish — N messages, one atomic fence
// ─────────────────────────────────────────────────────────────────────────────

fn bench_shm_batch_publish(c: &mut Criterion) {
    let mut ch = ShmChannel::new(RING_SIZE).expect("create SHM channel");
    let payload_100b = vec![0u8; 100];

    let mut group = c.benchmark_group("shm");

    for batch_size in [1usize, 8, 16, 64, 256, 1024] {
        let payloads: Vec<&[u8]> = (0..batch_size).map(|_| payload_100b.as_slice()).collect();

        group.throughput(Throughput::Elements(batch_size as u64));
        group.bench_with_input(
            BenchmarkId::new("batch_100b_no_consumer", batch_size),
            &batch_size,
            |b, _| {
                b.iter(|| {
                    // Drain before writing to keep the ring from filling.
                    ch.consumer().poll(|_| {});
                    ch.producer().publish_batch(&payloads).expect("batch publish");
                });
            },
        );
    }

    group.finish();
}

// ─────────────────────────────────────────────────────────────────────────────
// Criterion registration
// ─────────────────────────────────────────────────────────────────────────────

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(1000)
        .measurement_time(Duration::from_secs(10))
        .warm_up_time(Duration::from_secs(3));
    targets =
        bench_shm_publish_no_consumer,
        bench_shm_roundtrip,
        bench_shm_batch_publish,
}

criterion_main!(benches);
