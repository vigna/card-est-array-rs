/*
 * SPDX-FileCopyrightText: 2025 Sebastiano Vigna
 *
 * SPDX-License-Identifier: Apache-2.0 OR LGPL-2.1-or-later
 */

use card_est_array::{
    impls::{HyperLogLog, HyperLogLog8Builder, SliceEstimatorArray},
    traits::{
        EstimationLogic, Estimator, EstimatorArray, EstimatorArrayMut, EstimatorMut, MergeEstimator,
    },
};
use xxhash_rust::xxh3::Xxh3Builder;

/// The number of trials to run to ensure a bad seed does not
/// fail the test
const NUM_TRIALS: u64 = 100;
/// The number of successes required for the test to pass
const REQUIRED_TRIALS: u64 = 90;

#[cfg(feature = "slow_tests")]
const SIZES: &[usize] = &[1, 10, 100, 1000, 100_000];
#[cfg(not(feature = "slow_tests"))]
const SIZES: &[usize] = &[1, 10, 100, 1000];

fn do_test_single<const BETA: bool>() {
    let sizes = SIZES;
    let log2ms = [4, 6, 8, 12];

    for &size in sizes {
        for log2m in log2ms {
            let rsd = HyperLogLog::rel_std(log2m);
            let mut correct = 0;

            for trial in 0..NUM_TRIALS {
                let logic = HyperLogLog8Builder::new()
                    .log2_num_regs(log2m)
                    .build_hasher(Xxh3Builder::new().with_seed(trial))
                    .beta::<BETA>()
                    .build();
                let mut est = logic.new_estimator();
                let incr = (1 << 32) / size as i64;
                let mut x = i64::MIN;
                for _ in 0..size {
                    est.add(x);
                    x += incr;
                }

                let float_size = size as f64;

                if (float_size - est.estimate()).abs() / float_size < 2.0 * rsd {
                    correct += 1;
                }
            }

            assert!(
                correct >= REQUIRED_TRIALS,
                "assertion failed for size {} and log2m {} (BETA={}): correct = {} < {}",
                size,
                log2m,
                BETA,
                correct,
                REQUIRED_TRIALS
            );
        }
    }
}

#[test]
fn test_single() {
    do_test_single::<true>();
    do_test_single::<false>();
}

fn do_test_double<const BETA: bool>() {
    let sizes = SIZES;
    let log2ms = [4, 6, 8, 12];

    for &size in sizes {
        for log2m in log2ms {
            let rsd = HyperLogLog::rel_std(log2m);
            let mut correct_0 = 0;
            let mut correct_1 = 0;

            for trial in 0..NUM_TRIALS {
                let logic = HyperLogLog8Builder::new()
                    .log2_num_regs(log2m)
                    .build_hasher(Xxh3Builder::new().with_seed(trial))
                    .beta::<BETA>()
                    .build();
                let mut est_0 = logic.new_estimator();
                let mut est_1 = logic.new_estimator();
                let incr = (1 << 32) / size as i64;
                let mut x = i64::MIN;
                for _ in 0..size {
                    est_0.add(x);
                    est_1.add(x);
                    x += incr;
                }

                let float_size = size as f64;

                if (float_size - est_0.estimate()).abs() / float_size < 2.0 * rsd {
                    correct_0 += 1;
                }
                if (float_size - est_1.estimate()).abs() / float_size < 2.0 * rsd {
                    correct_1 += 1;
                }
            }

            assert!(
                correct_0 >= REQUIRED_TRIALS,
                "assertion failed for size {} and log2m {} (BETA={}): correct_0 = {} < {}",
                size,
                log2m,
                BETA,
                correct_0,
                REQUIRED_TRIALS
            );
            assert!(
                correct_1 >= REQUIRED_TRIALS,
                "assertion failed for size {} and log2m {} (BETA={}): correct_1 = {} < {}",
                size,
                log2m,
                BETA,
                correct_1,
                REQUIRED_TRIALS
            );
        }
    }
}

#[test]
fn test_double() {
    do_test_double::<true>();
    do_test_double::<false>();
}

fn do_test_merge<const BETA: bool>() {
    let sizes = SIZES;
    let log2ms = [4, 6, 8, 12];

    for &size in sizes {
        for log2m in log2ms {
            let rsd = HyperLogLog::rel_std(log2m);
            let mut correct_0 = 0;
            let mut correct_1 = 0;

            for trial in 0..NUM_TRIALS {
                let logic = HyperLogLog8Builder::new()
                    .log2_num_regs(log2m)
                    .build_hasher(Xxh3Builder::new().with_seed(trial))
                    .beta::<BETA>()
                    .build();
                let mut est_0 = logic.new_estimator();
                let mut est_1 = logic.new_estimator();
                let incr = (1 << 32) / (size * 2) as i64;
                let mut x = i64::MIN;
                for _ in 0..size {
                    est_0.add(x);
                    x += incr;
                    est_1.add(x);
                    x += incr;
                }

                est_0.merge(est_1.as_ref());

                let float_size = size as f64;

                if (float_size * 2.0 - est_0.estimate()).abs() / (float_size * 2.0) < 2.0 * rsd {
                    correct_0 += 1;
                }
                if (float_size - est_1.estimate()).abs() / (float_size * 2.0) < 2.0 * rsd {
                    correct_1 += 1;
                }
            }

            assert!(
                correct_0 >= REQUIRED_TRIALS,
                "assertion failed for size {} and log2m {} (BETA={}): correct_0 = {} < {}",
                size,
                log2m,
                BETA,
                correct_0,
                REQUIRED_TRIALS
            );
            assert!(
                correct_1 >= REQUIRED_TRIALS,
                "assertion failed for size {} and log2m {} (BETA={}): correct_1 = {} < {}",
                size,
                log2m,
                BETA,
                correct_1,
                REQUIRED_TRIALS
            );
        }
    }
}

#[test]
fn test_merge() {
    do_test_merge::<true>();
    do_test_merge::<false>();
}

fn do_test_merge_array<const BETA: bool>() {
    let sizes = SIZES;
    let log2ms = [4, 6, 8, 12];

    for &size in sizes {
        for log2m in log2ms {
            let rsd = HyperLogLog::rel_std(log2m);
            let mut correct_0 = 0;
            let mut correct_1 = 0;

            for trial in 0..NUM_TRIALS {
                let logic = HyperLogLog8Builder::new()
                    .log2_num_regs(log2m)
                    .build_hasher(Xxh3Builder::new().with_seed(trial))
                    .beta::<BETA>()
                    .build();
                let mut estimators = SliceEstimatorArray::new(logic, 2);
                let incr = (1 << 32) / (size * 2) as i64;
                let mut x = i64::MIN;
                for _ in 0..size {
                    estimators.get_estimator_mut(0).add(x);
                    x += incr;
                    estimators.get_estimator_mut(1).add(x);
                    x += incr;
                }

                let to_merge = estimators.get_backend(1).to_vec();
                let mut est = estimators.get_estimator_mut(0);
                est.merge(&to_merge);

                let float_size = size as f64;

                if (float_size * 2.0 - estimators.get_estimator(0).estimate()).abs()
                    / (float_size * 2.0)
                    < 2.0 * rsd
                {
                    correct_0 += 1;
                }
                if (float_size - estimators.get_estimator(1).estimate()).abs() / (float_size * 2.0)
                    < 2.0 * rsd
                {
                    correct_1 += 1;
                }
            }

            assert!(
                correct_0 >= REQUIRED_TRIALS,
                "assertion failed for size {} and log2m {} (BETA={}): correct_0 = {} < {}",
                size,
                log2m,
                BETA,
                correct_0,
                REQUIRED_TRIALS
            );
            assert!(
                correct_1 >= REQUIRED_TRIALS,
                "assertion failed for size {} and log2m {} (BETA={}): correct_1 = {} < {}",
                size,
                log2m,
                BETA,
                correct_1,
                REQUIRED_TRIALS
            );
        }
    }
}

#[test]
fn test_merge_array() {
    do_test_merge_array::<true>();
    do_test_merge_array::<false>();
}
