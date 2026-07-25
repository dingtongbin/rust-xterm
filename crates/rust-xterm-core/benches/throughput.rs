//! Benchmark: measure throughput of `TerminalManager::write`.
//!
//! Run with: `cargo bench`

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use rust_xterm_core::{TerminalManager, TerminalSize};
use std::time::Instant;

fn bench_write_ansi(c: &mut Criterion) {
    let mut group = c.benchmark_group("write");
    group.throughput(Throughput::Bytes(1024));

    // 1 KB of mixed ANSI + text
    let payload: Vec<u8> = b"\x1b[31mHello\x1b[0m \x1b[1mWorld\x1b[0m\n".repeat(64);

    group.bench_function("ansi_1kb", |b| {
        b.iter(|| {
            let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));
            mgr.write(&payload);
        });
    });

    group.finish();
}

fn bench_write_utf8_cjk(c: &mut Criterion) {
    let mut group = c.benchmark_group("write");
    group.throughput(Throughput::Bytes(1024));

    // 1 KB of CJK text (UTF-8)
    let payload: Vec<u8> = "你好世界，这是 rust-xterm 的性能测试。\n"
        .repeat(32)
        .into_bytes();

    group.bench_function("utf8_cjk_1kb", |b| {
        b.iter(|| {
            let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));
            mgr.write(&payload);
        });
    });

    group.finish();
}

fn bench_poll_frame(c: &mut Criterion) {
    let mut group = c.benchmark_group("poll_frame");

    group.bench_function("idle", |b| {
        b.iter(|| {
            let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));
            mgr.poll_frame(Instant::now())
        });
    });

    group.bench_function("with_damage", |b| {
        b.iter(|| {
            let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));
            mgr.write(b"hello\n");
            mgr.poll_frame(Instant::now())
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_write_ansi,
    bench_write_utf8_cjk,
    bench_poll_frame
);
criterion_main!(benches);
