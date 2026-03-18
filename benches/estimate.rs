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
/// (register_size = 5) with varying numbers of registers.
fn bench_estimate(c: &mut Criterion) {
    let mut group = c.benchmark_group("estimate");

    // log_2_num_reg values: 6, 8, 10, 12 → 64, 256, 1024, 4096 registers
    for &log2_regs in &[6, 8, 10, 12] {
        let num_regs = 1usize << log2_regs;

        let logic = HyperLogLogBuilder::new(1_000_000_000)
            .log_2_num_reg(log2_regs)
            .build::<usize>();

        let mut backend = vec![0usize; logic.backend_len()];

        // Populate registers realistically (1M elements).
        for i in 0..1_000_000usize {
            logic.add(&mut backend, i);
        }

        group.bench_with_input(
            BenchmarkId::from_parameter(num_regs),
            &num_regs,
            |b, _| {
                b.iter(|| black_box(logic.estimate(black_box(backend.as_slice()))));
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_estimate);
criterion_main!(benches);
