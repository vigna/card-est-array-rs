/*
 * SPDX-FileCopyrightText: 2025 Sebastiano Vigna
 *
 * SPDX-License-Identifier: Apache-2.0 OR LGPL-2.1-or-later
 */

use card_est_array::{
    impls::HyperLogLogBuilder,
    traits::{EstimationLogic, SliceEstimationLogic},
};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

/// Benchmarks `estimate` for HyperLogLog configured for 1B elements
/// (register_size = 5) with varying numbers of registers, comparing
/// LogLog-β correction (BETA=true) vs classic HLL (BETA=false).
fn bench_estimate(c: &mut Criterion) {
    let mut group = c.benchmark_group("estimate");

    for &log2_regs in &[6, 8, 10, 12] {
        let num_regs = 1usize << log2_regs;

        // BETA = true (LogLog-β)
        let logic_beta = HyperLogLogBuilder::new(1_000_000_000)
            .log2_num_regs(log2_regs)
            .build::<usize>();

        let mut backend_beta = vec![0usize; logic_beta.backend_len()];
        for i in 0..1_000_000usize {
            logic_beta.add(&mut backend_beta, i);
        }

        // BETA = false (classic HLL)
        let logic_classic = HyperLogLogBuilder::new(1_000_000_000)
            .log2_num_regs(log2_regs)
            .beta::<false>()
            .build::<usize>();

        let mut backend_classic = vec![0usize; logic_classic.backend_len()];
        for i in 0..1_000_000usize {
            logic_classic.add(&mut backend_classic, i);
        }

        group.bench_with_input(
            BenchmarkId::new("beta", num_regs),
            &num_regs,
            |b, _| {
                b.iter(|| black_box(logic_beta.estimate(black_box(backend_beta.as_slice()))));
            },
        );

        group.bench_with_input(
            BenchmarkId::new("classic", num_regs),
            &num_regs,
            |b, _| {
                b.iter(|| {
                    black_box(logic_classic.estimate(black_box(backend_classic.as_slice())))
                });
            },
        );
    }

    group.finish();
}

/// Benchmarks `estimate` with few elements so that many registers remain
/// zero, exercising the LogLog-β correction path (`beta_horner`).
fn bench_estimate_low_cardinality(c: &mut Criterion) {
    let mut group = c.benchmark_group("estimate_low_card");

    for &log2_regs in &[6, 8, 10, 12] {
        let num_regs = 1usize << log2_regs;
        // Add only num_regs / 4 elements so ~75% of registers stay zero.
        let num_elements = num_regs / 4;

        let logic_beta = HyperLogLogBuilder::new(1_000_000_000)
            .log2_num_regs(log2_regs)
            .build::<usize>();

        let mut backend_beta = vec![0usize; logic_beta.backend_len()];
        for i in 0..num_elements {
            logic_beta.add(&mut backend_beta, i);
        }

        let logic_classic = HyperLogLogBuilder::new(1_000_000_000)
            .log2_num_regs(log2_regs)
            .beta::<false>()
            .build::<usize>();

        let mut backend_classic = vec![0usize; logic_classic.backend_len()];
        for i in 0..num_elements {
            logic_classic.add(&mut backend_classic, i);
        }

        group.bench_with_input(
            BenchmarkId::new("beta", num_regs),
            &num_regs,
            |b, _| {
                b.iter(|| black_box(logic_beta.estimate(black_box(backend_beta.as_slice()))));
            },
        );

        group.bench_with_input(
            BenchmarkId::new("classic", num_regs),
            &num_regs,
            |b, _| {
                b.iter(|| {
                    black_box(logic_classic.estimate(black_box(backend_classic.as_slice())))
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_estimate, bench_estimate_low_cardinality);
criterion_main!(benches);
