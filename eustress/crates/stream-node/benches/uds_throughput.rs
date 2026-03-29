//! # Unix Domain Socket throughput benchmarks
//!
//! Compares UDS vs TCP for same-host IPC latency.
//! UDS bypasses the TCP/IP stack — kernel copies directly between processes.
//!
//! **Unix only** — compiles to a no-op on Windows (tokio UnixStream is unix-only).
//!
//! Run with:
//! ```
//! cargo bench -p eustress-stream-node --bench uds_throughput
//! ```

// ─────────────────────────────────────────────────────────────────────────────
// Non-unix stub
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(not(unix))]
fn main() {
    eprintln!("Unix domain socket benchmarks only run on Unix (Linux/macOS).");
    eprintln!("On Windows, TCP loopback is the equivalent IPC transport.");
}

// ─────────────────────────────────────────────────────────────────────────────
// Unix implementation
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(unix)]
use std::sync::Arc;
#[cfg(unix)]
use std::time::Duration;

#[cfg(unix)]
use bytes::Bytes;
#[cfg(unix)]
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
#[cfg(unix)]
use tokio::runtime::Runtime;

#[cfg(unix)]
use eustress_stream::StreamConfig;
#[cfg(unix)]
use eustress_stream_node::uds::{UnixNode, UnixNodeClient};
#[cfg(unix)]
use eustress_stream_node::NodeConfig;

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(unix)]
fn fast_config() -> NodeConfig {
    NodeConfig {
        stream_config: StreamConfig::default()
            .in_memory()
            .with_ring_capacity(65536),
        ..NodeConfig::default()
    }
}

#[cfg(unix)]
fn tmp_socket(tag: &str) -> String {
    format!("/tmp/eustress_bench_{tag}.sock")
}

#[cfg(unix)]
async fn start_node(tag: &str) -> (Arc<UnixNode>, String) {
    let path = tmp_socket(tag);
    let node = UnixNode::start(&path, fast_config()).await.expect("start UDS node");
    (node, path)
}

#[cfg(unix)]
async fn connect(path: &str) -> UnixNodeClient {
    UnixNodeClient::connect(path).await.expect("connect UDS")
}

// ─────────────────────────────────────────────────────────────────────────────
// Benchmarks
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(unix)]
fn bench_uds_publish_no_sub(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let (node, path) = rt.block_on(start_node("no_sub"));
    let client = rt.block_on(connect(&path));
    let payload = Bytes::from(vec![0u8; 100]);

    let mut group = c.benchmark_group("uds");
    group.throughput(Throughput::Elements(1));
    group.bench_function("publish_100b_no_sub", |b| {
        b.to_async(&rt).iter(|| async {
            client.publish("bench_no_sub", payload.clone()).await.unwrap()
        });
    });
    group.finish();
    node.shutdown();
}

#[cfg(unix)]
fn bench_uds_publish_1_sub(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let (node, path) = rt.block_on(start_node("1sub"));
    let pub_client = rt.block_on(connect(&path));
    let sub_client = rt.block_on(connect(&path));

    let topic = "bench_1sub";
    let payload_100b = Bytes::from(vec![0u8; 100]);
    let payload_1k = Bytes::from(vec![0u8; 1024]);

    let mut rx = rt.block_on(sub_client.subscribe(topic, None)).unwrap();
    rt.spawn(async move { while rx.recv().await.is_some() {} });

    let mut group = c.benchmark_group("uds");
    group.throughput(Throughput::Elements(1));

    group.bench_function("publish_100b_1sub", |b| {
        b.to_async(&rt).iter(|| async {
            pub_client.publish(topic, payload_100b.clone()).await.unwrap()
        });
    });

    group.bench_with_input(
        BenchmarkId::new("publish_1k_1sub", "1k"),
        &payload_1k,
        |b, p| {
            b.to_async(&rt).iter(|| async {
                pub_client.publish(topic, p.clone()).await.unwrap()
            });
        },
    );

    group.finish();
    node.shutdown();
}

#[cfg(unix)]
fn bench_uds_8_subs(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let (node, path) = rt.block_on(start_node("8subs"));
    let pub_client = rt.block_on(connect(&path));
    let topic = "bench_8subs";
    let payload = Bytes::from(vec![0u8; 100]);

    for _ in 0..8 {
        let p = path.clone();
        let mut rx = rt.block_on(async move {
            connect(&p).await.subscribe(topic, None).await.unwrap()
        });
        rt.spawn(async move { while rx.recv().await.is_some() {} });
    }

    let mut group = c.benchmark_group("uds");
    group.throughput(Throughput::Elements(1));
    group.bench_function("publish_100b_8subs", |b| {
        b.to_async(&rt).iter(|| async {
            pub_client.publish(topic, payload.clone()).await.unwrap()
        });
    });
    group.finish();
    node.shutdown();
}

#[cfg(unix)]
fn bench_uds_batch_publish(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let (node, path) = rt.block_on(start_node("batch"));
    let client = rt.block_on(connect(&path));
    let payload_100b = Bytes::from(vec![0u8; 100]);

    let mut group = c.benchmark_group("uds");

    for batch_size in [1usize, 8, 16, 64, 256] {
        let msgs: Vec<(String, Bytes)> = (0..batch_size)
            .map(|_| ("batch_topic".to_string(), payload_100b.clone()))
            .collect();
        let msgs = Arc::new(msgs);

        group.throughput(Throughput::Elements(batch_size as u64));
        group.bench_with_input(
            BenchmarkId::new("batch_100b_no_sub", batch_size),
            &batch_size,
            |b, _| {
                let msgs = Arc::clone(&msgs);
                b.to_async(&rt).iter(|| async {
                    client.publish_batch((*msgs).clone()).await.unwrap()
                });
            },
        );
    }

    group.finish();
    node.shutdown();
}

// ─────────────────────────────────────────────────────────────────────────────
// Registration (unix only)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(unix)]
criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(500)
        .measurement_time(Duration::from_secs(10))
        .warm_up_time(Duration::from_secs(3));
    targets =
        bench_uds_publish_no_sub,
        bench_uds_publish_1_sub,
        bench_uds_8_subs,
        bench_uds_batch_publish,
}

#[cfg(unix)]
criterion_main!(benches);
