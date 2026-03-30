//! # EustressStream network throughput benchmarks
//!
//! Measures end-to-end pub/sub throughput over loopback TCP for:
//!
//! - Single node, no subscriber (pure write path)
//! - Single node, 1 subscriber (write + fan-out)
//! - Single node, 1 subscriber, 1 KB payloads
//! - Single node, 8 subscribers (fan-out stress)
//! - 10-node ForgeCluster, sharded publish (no subscriber)
//! - 10-node ForgeCluster, sharded publish (1 subscriber per node)
//!
//! Run with:
//! ```
//! cargo bench -p eustress-stream-node
//! ```

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use tokio::runtime::Runtime;

use eustress_stream::StreamConfig;
use eustress_stream_node::{ForgeCluster, NodeConfig, StreamNode, StreamNodeClient};

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn fast_config(ring_cap: usize) -> NodeConfig {
    NodeConfig {
        auto_increment: true,
        stream_config: StreamConfig::default()
            .in_memory()
            .with_ring_capacity(ring_cap),
        ..NodeConfig::default()
    }
}

async fn start_node() -> (StreamNode, SocketAddr) {
    let node = StreamNode::start(fast_config(65536))
        .await
        .expect("start node");
    let addr = node.listen_addr();
    (node, addr)
}

async fn connect(addr: SocketAddr) -> StreamNodeClient {
    // listen_addr() returns 0.0.0.0:PORT; connect needs a reachable address.
    let loopback: SocketAddr = format!("127.0.0.1:{}", addr.port()).parse().unwrap();
    StreamNodeClient::connect(loopback).await.expect("connect")
}

// ─────────────────────────────────────────────────────────────────────────────
// Single-node benchmarks
// ─────────────────────────────────────────────────────────────────────────────

fn bench_single_node_publish_no_sub(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let (node, addr) = rt.block_on(start_node());
    let client = rt.block_on(connect(addr));

    let payload = Bytes::from_static(b"x".repeat(100).leak());
    let topic = "bench_no_sub";

    let mut group = c.benchmark_group("single_node");
    group.throughput(Throughput::Elements(1));
    group.bench_function("publish_100b_no_sub", |b| {
        b.to_async(&rt).iter(|| async {
            client.publish(topic, payload.clone()).await.unwrap()
        });
    });

    group.finish();
    node.shutdown();
}

fn bench_single_node_publish_1_sub(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let (node, addr) = rt.block_on(start_node());
    let pub_client = rt.block_on(connect(addr));
    let sub_client = rt.block_on(connect(addr));

    let topic = "bench_1_sub";
    let payload_100b = Bytes::from(vec![0u8; 100]);
    let payload_1k = Bytes::from(vec![0u8; 1024]);

    // Subscribe — drive receiver in a background task so the channel never fills
    let mut rx = rt.block_on(async {
        sub_client.subscribe(topic, None).await.unwrap()
    });
    rt.spawn(async move {
        while rx.recv().await.is_some() {}
    });

    let mut group = c.benchmark_group("single_node");
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

fn bench_single_node_8_subs(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let (node, addr) = rt.block_on(start_node());
    let pub_client = rt.block_on(connect(addr));

    let topic = "bench_8_subs";
    let payload = Bytes::from(vec![1u8; 100]);

    // 8 subscribers
    for _ in 0..8 {
        let addr = addr;
        let t = topic;
        let mut rx = rt.block_on(async move {
            let c = connect(addr).await;
            c.subscribe(t, None).await.unwrap()
        });
        rt.spawn(async move {
            while rx.recv().await.is_some() {}
        });
    }

    let mut group = c.benchmark_group("single_node");
    group.throughput(Throughput::Elements(1));
    group.bench_function("publish_100b_8subs", |b| {
        b.to_async(&rt).iter(|| async {
            pub_client.publish(topic, payload.clone()).await.unwrap()
        });
    });

    group.finish();
    node.shutdown();
}

// ─────────────────────────────────────────────────────────────────────────────
// ForgeCluster benchmarks
// ─────────────────────────────────────────────────────────────────────────────

fn bench_cluster_publish_no_sub(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    let cluster = rt.block_on(async {
        ForgeCluster::start(34000, 10, fast_config(65536))
            .await
            .expect("start cluster")
    });

    // Connect one client per node for sharded publishing
    let addrs = cluster.node_addrs();
    let clients: Vec<StreamNodeClient> = rt.block_on(async {
        let mut v = Vec::with_capacity(addrs.len());
        for addr in &addrs {
            v.push(connect(*addr).await);
        }
        v
    });

    let topics: Vec<String> = (0..10).map(|i| format!("cluster_bench_{i}")).collect();
    let payload = Bytes::from(vec![0u8; 100]);
    let clients = Arc::new(clients);
    let topics = Arc::new(topics);

    let mut group = c.benchmark_group("cluster_10");
    group.throughput(Throughput::Elements(1));
    group.bench_function("sharded_publish_100b_no_sub", |b| {
        let clients = Arc::clone(&clients);
        let topics = Arc::clone(&topics);
        let payload = payload.clone();
        b.to_async(&rt).iter_custom(|iters| {
            let clients = Arc::clone(&clients);
            let topics = Arc::clone(&topics);
            let payload = payload.clone();
            async move {
                let start = std::time::Instant::now();
                for i in 0..iters as usize {
                    let idx = i % clients.len();
                    clients[idx]
                        .publish(&topics[idx], payload.clone())
                        .await
                        .unwrap();
                }
                start.elapsed()
            }
        });
    });

    group.finish();
    cluster.shutdown();
}

fn bench_cluster_publish_1_sub(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    let cluster = rt.block_on(async {
        ForgeCluster::start(35000, 10, fast_config(65536))
            .await
            .expect("start cluster")
    });

    let addrs = cluster.node_addrs();

    // Connect pub + sub clients per node
    let clients: Vec<(StreamNodeClient, StreamNodeClient)> = rt.block_on(async {
        let mut v = Vec::with_capacity(addrs.len());
        for addr in &addrs {
            v.push((connect(*addr).await, connect(*addr).await));
        }
        v
    });

    let topics: Vec<String> = (0..10).map(|i| format!("cluster_sub_{i}")).collect();

    // Subscribe on each node, drain in background
    for (i, (_, sub)) in clients.iter().enumerate() {
        let mut rx = rt.block_on(sub.subscribe(&topics[i], None)).unwrap();
        rt.spawn(async move {
            while rx.recv().await.is_some() {}
        });
    }

    let pub_clients: Vec<StreamNodeClient> = clients.into_iter().map(|(p, _)| p).collect();
    let pub_clients = Arc::new(pub_clients);
    let topics = Arc::new(topics);
    let payload = Bytes::from(vec![0u8; 100]);

    let mut group = c.benchmark_group("cluster_10");
    group.throughput(Throughput::Elements(1));
    group.bench_function("sharded_publish_100b_1sub", |b| {
        let clients = Arc::clone(&pub_clients);
        let topics = Arc::clone(&topics);
        let payload = payload.clone();
        b.to_async(&rt).iter_custom(|iters| {
            let clients = Arc::clone(&clients);
            let topics = Arc::clone(&topics);
            let payload = payload.clone();
            async move {
                let start = std::time::Instant::now();
                for i in 0..iters as usize {
                    let idx = i % clients.len();
                    clients[idx]
                        .publish(&topics[idx], payload.clone())
                        .await
                        .unwrap();
                }
                start.elapsed()
            }
        });
    });

    group.finish();
    cluster.shutdown();
}

// ─────────────────────────────────────────────────────────────────────────────
// Round 1: Batch publish over TCP
// ─────────────────────────────────────────────────────────────────────────────

fn bench_tcp_batch_publish(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let (node, addr) = rt.block_on(start_node());
    let client = rt.block_on(connect(addr));

    let payload_100b = Bytes::from(vec![0u8; 100]);

    let mut group = c.benchmark_group("tcp_batch");

    for batch_size in [1usize, 8, 16, 64, 256] {
        let msgs: Vec<(String, Bytes)> = (0..batch_size)
            .map(|_| ("batch_topic".to_string(), payload_100b.clone()))
            .collect();
        let msgs = Arc::new(msgs);

        // Throughput = batch_size elements per sample
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
// Round 2: Zero-copy single-topic batch (PublishBatchTopic + BatchAckCompact)
// ─────────────────────────────────────────────────────────────────────────────

/// Benchmarks `publish_batch_topic` — single-topic variant that returns a
/// `BatchAckCompact { first_offset, count }` (12 bytes) instead of
/// `BatchAck { offsets: Vec<u64> }` (N × 8 bytes).
fn bench_tcp_batch_topic(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let (node, addr) = rt.block_on(start_node());
    let client = rt.block_on(connect(addr));

    let payload_100b = Bytes::from(vec![0u8; 100]);
    let topic = "batch_topic_compact";

    let mut group = c.benchmark_group("tcp_batch_topic");

    for batch_size in [1usize, 8, 16, 64, 256, 1024] {
        let payloads: Vec<Bytes> = (0..batch_size).map(|_| payload_100b.clone()).collect();
        let payloads = Arc::new(payloads);

        group.throughput(Throughput::Elements(batch_size as u64));
        group.bench_with_input(
            BenchmarkId::new("compact_100b_no_sub", batch_size),
            &batch_size,
            |b, _| {
                let payloads = Arc::clone(&payloads);
                b.to_async(&rt).iter(|| async {
                    client
                        .publish_batch_topic(topic, (*payloads).clone())
                        .await
                        .unwrap()
                });
            },
        );
    }

    group.finish();
    node.shutdown();
}

// ─────────────────────────────────────────────────────────────────────────────
// Round 5: QUIC single publish + batch
// ─────────────────────────────────────────────────────────────────────────────

fn bench_quic_single_publish(c: &mut Criterion) {
    use eustress_stream_node::quic::{QuicNode, QuicNodeClient};

    let rt = Runtime::new().unwrap();

    let (quic_node, cert) = rt.block_on(async {
        let cfg = fast_config(65536);
        let n = QuicNode::start(cfg).await.expect("quic node");
        let cert = n.cert_der().clone();
        (n, cert)
    });

    let quic_addr: SocketAddr = format!("127.0.0.1:{}", quic_node.listen_addr().port())
        .parse()
        .unwrap();

    let client = rt.block_on(async {
        QuicNodeClient::connect(quic_addr, &cert).await.expect("quic connect")
    });

    let payload_100b = Bytes::from(vec![0u8; 100]);

    let mut group = c.benchmark_group("quic");
    group.throughput(Throughput::Elements(1));

    group.bench_function("publish_100b_no_sub", |b| {
        b.to_async(&rt).iter(|| async {
            client.publish("quic_bench", payload_100b.clone()).await.unwrap()
        });
    });

    group.finish();
    quic_node.shutdown();
}

fn bench_quic_batch_publish(c: &mut Criterion) {
    use eustress_stream_node::quic::{QuicNode, QuicNodeClient};

    let rt = Runtime::new().unwrap();

    let (quic_node, cert) = rt.block_on(async {
        let cfg = fast_config(65536);
        let n = QuicNode::start(cfg).await.expect("quic node");
        let cert = n.cert_der().clone();
        (n, cert)
    });

    let quic_addr: SocketAddr = format!("127.0.0.1:{}", quic_node.listen_addr().port())
        .parse()
        .unwrap();

    let client = rt.block_on(async {
        QuicNodeClient::connect(quic_addr, &cert).await.expect("quic connect")
    });

    let payload_100b = Bytes::from(vec![0u8; 100]);

    let mut group = c.benchmark_group("quic");

    for batch_size in [1usize, 8, 16, 64, 256] {
        let msgs: Vec<(String, Bytes)> = (0..batch_size)
            .map(|_| ("quic_batch".to_string(), payload_100b.clone()))
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
    quic_node.shutdown();
}

// ─────────────────────────────────────────────────────────────────────────────
// Criterion registration
// ─────────────────────────────────────────────────────────────────────────────

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(500)
        .measurement_time(Duration::from_secs(10))
        .warm_up_time(Duration::from_secs(3));
    targets =
        bench_single_node_publish_no_sub,
        bench_single_node_publish_1_sub,
        bench_single_node_8_subs,
        bench_cluster_publish_no_sub,
        bench_cluster_publish_1_sub,
        bench_tcp_batch_publish,
        bench_tcp_batch_topic,
        bench_quic_single_publish,
        bench_quic_batch_publish,
}

criterion_main!(benches);
