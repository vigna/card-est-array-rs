/*
 * SPDX-FileCopyrightText: 2025 Sebastiano Vigna
 *
 * SPDX-License-Identifier: Apache-2.0 OR LGPL-2.1-or-later
 */

use card_est_array::{
    impls::{HyperLogLog8Builder, HyperLogLogBuilder},
    traits::{EstimationLogic, MergeEstimationLogic, SliceEstimationLogic},
};
use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};

/// Benchmarks `add` for HyperLogLog with 5-bit registers (1B elements),
/// 6-bit registers (10B elements), and HyperLogLog8 (byte registers).
fn bench_add(c: &mut Criterion) {
    let mut group = c.benchmark_group("add");

    for &log2_regs in &[6, 8, 10, 12] {
        let num_regs = 1usize << log2_regs;

        // HyperLogLog with 5-bit registers (1B elements, u64 backend)
        {
            let hll = HyperLogLogBuilder::new(1_000_000_000)
                .log2_num_regs(log2_regs)
                .build::<usize>()
                .unwrap();
            let mut backend = vec![0usize; hll.backend_len()];
            for i in 0..10_000usize {
                hll.add(&mut backend, i);
            }
            let mut i = 10_000usize;
            group.bench_function(BenchmarkId::new("hll-5bit", num_regs), |b| {
                b.iter(|| {
                    i = i.wrapping_add(1);
                    hll.add(black_box(&mut backend), black_box(i));
                });
            });
        }

        // HyperLogLog with 6-bit registers (10B elements, u64 backend)
        {
            let hll = HyperLogLogBuilder::new(10_000_000_000)
                .log2_num_regs(log2_regs)
                .build::<usize>()
                .unwrap();
            let mut backend = vec![0usize; hll.backend_len()];
            for i in 0..10_000usize {
                hll.add(&mut backend, i);
            }
            let mut i = 10_000usize;
            group.bench_function(BenchmarkId::new("hll-6bit", num_regs), |b| {
                b.iter(|| {
                    i = i.wrapping_add(1);
                    hll.add(black_box(&mut backend), black_box(i));
                });
            });
        }

        // HyperLogLog8
        {
            let hll8 = HyperLogLog8Builder::new()
                .log2_num_regs(log2_regs)
                .build::<usize>();
            let mut backend = vec![0u8; hll8.backend_len()];
            for i in 0..10_000usize {
                hll8.add(&mut backend, i);
            }
            let mut i = 10_000usize;
            group.bench_function(BenchmarkId::new("hll-byte", num_regs), |b| {
                b.iter(|| {
                    i = i.wrapping_add(1);
                    hll8.add(black_box(&mut backend), black_box(i));
                });
            });
        }
    }

    group.finish();
}

/// Benchmarks `merge` for HyperLogLog with 5-bit registers (broadword),
/// 6-bit registers (broadword), and HyperLogLog8 (SIMD).
fn bench_merge(c: &mut Criterion) {
    let mut group = c.benchmark_group("merge");

    for &log2_regs in &[6, 8, 10, 12] {
        let num_regs = 1usize << log2_regs;

        // HyperLogLog with 5-bit registers (1B elements, u64 backend)
        {
            let hll = HyperLogLogBuilder::new(1_000_000_000)
                .log2_num_regs(log2_regs)
                .build::<usize>()
                .unwrap();
            let mut dst = vec![0usize; hll.backend_len()];
            let mut src = vec![0usize; hll.backend_len()];
            for i in 0..10_000usize {
                hll.add(&mut dst, i * 2);
                hll.add(&mut src, i * 2 + 1);
            }
            let mut helper = hll.new_helper();
            group.bench_function(BenchmarkId::new("hll-5bit", num_regs), |b| {
                b.iter(|| {
                    hll.merge_with_helper(black_box(&mut dst), black_box(&src), &mut helper);
                });
            });
        }

        // HyperLogLog with 6-bit registers (10B elements, u64 backend)
        {
            let hll = HyperLogLogBuilder::new(10_000_000_000)
                .log2_num_regs(log2_regs)
                .build::<usize>()
                .unwrap();
            let mut dst = vec![0usize; hll.backend_len()];
            let mut src = vec![0usize; hll.backend_len()];
            for i in 0..10_000usize {
                hll.add(&mut dst, i * 2);
                hll.add(&mut src, i * 2 + 1);
            }
            let mut helper = hll.new_helper();
            group.bench_function(BenchmarkId::new("hll-6bit", num_regs), |b| {
                b.iter(|| {
                    hll.merge_with_helper(black_box(&mut dst), black_box(&src), &mut helper);
                });
            });
        }

        // HyperLogLog8
        {
            let hll8 = HyperLogLog8Builder::new()
                .log2_num_regs(log2_regs)
                .build::<usize>();
            let mut dst = vec![0u8; hll8.backend_len()];
            let mut src = vec![0u8; hll8.backend_len()];
            for i in 0..10_000usize {
                hll8.add(&mut dst, i * 2);
                hll8.add(&mut src, i * 2 + 1);
            }
            group.bench_function(BenchmarkId::new("hll-byte", num_regs), |b| {
                b.iter(|| {
                    hll8.merge(black_box(&mut dst), black_box(&src));
                });
            });
        }
    }

    group.finish();
}

criterion_group!(benches, bench_add, bench_merge);
criterion_main!(benches);
