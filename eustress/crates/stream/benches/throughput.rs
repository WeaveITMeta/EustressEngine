//! Throughput benchmarks for EustressStream.
//!
//! Run with:
//!   cargo bench -p eustress-stream

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use bytes::Bytes;
use eustress_stream::{EustressStream, StreamConfig};

fn bench_send_100b(c: &mut Criterion) {
    let stream   = EustressStream::new(StreamConfig::default().in_memory());
    let producer = stream.producer("bench");
    let payload  = Bytes::from(vec![0u8; 100]);

    let mut group = c.benchmark_group("send_100b");
    group.throughput(Throughput::Elements(1));
    group.bench_function("send_bytes_no_sub", |b| {
        b.iter(|| { producer.send_bytes(black_box(payload.clone())); });
    });
    group.finish();
}

fn bench_send_with_subscriber(c: &mut Criterion) {
    let stream = EustressStream::new(StreamConfig::default().in_memory());
    stream.subscribe("bench2", |view| { black_box(view.data.len()); }).unwrap();
    let producer = stream.producer("bench2");
    let payload  = Bytes::from(vec![0u8; 100]);

    let mut group = c.benchmark_group("send_100b_1sub");
    group.throughput(Throughput::Elements(1));
    group.bench_function("send_bytes_one_sub", |b| {
        b.iter(|| { producer.send_bytes(black_box(payload.clone())); });
    });
    group.finish();
}

fn bench_pod(c: &mut Criterion) {
    #[derive(Clone, Copy)]
    #[repr(C)]
    struct Transform { pos: [f32; 3], rot: [f32; 4], scale: [f32; 3] }
    unsafe impl bytemuck::Pod      for Transform {}
    unsafe impl bytemuck::Zeroable for Transform {}

    let stream = EustressStream::new(StreamConfig::default().in_memory());
    stream.subscribe("transforms", |view| { black_box(view.cast::<Transform>()); }).unwrap();
    let producer = stream.producer("transforms");
    let t = Transform { pos: [1.0; 3], rot: [0.0, 0.0, 0.0, 1.0], scale: [1.0; 3] };

    let mut group = c.benchmark_group("send_pod_transform");
    group.throughput(Throughput::Elements(1));
    group.bench_function("send_pod", |b| {
        b.iter(|| { producer.send_pod(black_box(&t)); });
    });
    group.finish();
}

criterion_group!(benches, bench_send_100b, bench_send_with_subscriber, bench_pod);
criterion_main!(benches);
