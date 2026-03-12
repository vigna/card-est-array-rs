/*
 * SPDX-FileCopyrightText: 2024 Matteo Dell'Acqua
 * SPDX-FileCopyrightText: 2025 Sebastiano Vigna
 *
 * SPDX-License-Identifier: Apache-2.0 OR LGPL-2.1-or-later
 */

use card_est_array::{
    impls::{HyperLogLog, HyperLogLogBuilder, SliceEstimatorArray},
    traits::{
        EstimationLogic, Estimator, EstimatorArray, EstimatorArrayMut, EstimatorMut,
        MergeEstimator, Word,
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

#[test]
fn test_min_log_2_num_reg() {
    fn test_word<W: Word>() {
        let sizes = SIZES;

        for &size in sizes {
            let builder = HyperLogLogBuilder::new(size).word_type::<W>();
            let min_log2m = builder.min_log_2_num_reg();

            let result =
                std::panic::catch_unwind(|| builder.clone().log_2_num_reg(min_log2m).build::<()>());
            assert!(
                result.is_ok(),
                "size={size} with W={} gives min_log2m={min_log2m}, which is not valid",
                std::any::type_name::<W>()
            );

            let result =
                std::panic::catch_unwind(|| builder.log_2_num_reg(min_log2m - 1).build::<()>());
            assert!(
                result.is_err(),
                "size={size} with W={} gives min_log2m={min_log2m}, but log2m={} is valid too",
                std::any::type_name::<W>(),
                min_log2m - 1,
            );
        }
    }

    test_word::<u16>();
    test_word::<u32>();
    test_word::<u64>();
    test_word::<u128>();
    test_word::<usize>();
}

#[test]
fn test_single() {
    let sizes = SIZES;
    let log2ms = [4, 6, 8, 12];

    for &size in sizes {
        for log2m in log2ms {
            let rsd = HyperLogLog::rel_std(log2m);
            let mut correct = 0;

            for trial in 0..NUM_TRIALS {
                let logic = HyperLogLogBuilder::new(size)
                    .word_type::<u16>()
                    .log_2_num_reg(log2m)
                    .build_hasher(Xxh3Builder::new().with_seed(trial))
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
                "assertion failed for size {} and log2m {}: correct = {} < {}",
                size,
                log2m,
                correct,
                REQUIRED_TRIALS
            );
        }
    }
}

#[test]
fn test_double() {
    let sizes = SIZES;
    let log2ms = [4, 6, 8, 12];

    for &size in sizes {
        for log2m in log2ms {
            let rsd = HyperLogLog::rel_std(log2m);
            let mut correct_0 = 0;
            let mut correct_1 = 0;

            for trial in 0..NUM_TRIALS {
                let logic = HyperLogLogBuilder::new(size)
                    .word_type::<u16>()
                    .log_2_num_reg(log2m)
                    .build_hasher(Xxh3Builder::new().with_seed(trial))
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
                "assertion failed for size {} and log2m {}: correct_0 = {} < {}",
                size,
                log2m,
                correct_0,
                REQUIRED_TRIALS
            );
            assert!(
                correct_1 >= REQUIRED_TRIALS,
                "assertion failed for size {} and log2m {}: correct_1 = {} < {}",
                size,
                log2m,
                correct_1,
                REQUIRED_TRIALS
            );
        }
    }
}

#[test]
fn test_merge() {
    let sizes = SIZES;
    let log2ms = [4, 6, 8, 12];

    for &size in sizes {
        for log2m in log2ms {
            let rsd = HyperLogLog::rel_std(log2m);
            let mut correct_0 = 0;
            let mut correct_1 = 0;

            for trial in 0..NUM_TRIALS {
                let logic = HyperLogLogBuilder::new(size)
                    .word_type::<u16>()
                    .log_2_num_reg(log2m)
                    .build_hasher(Xxh3Builder::new().with_seed(trial))
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
                "assertion failed for size {} and log2m {}: correct_0 = {} < {}",
                size,
                log2m,
                correct_0,
                REQUIRED_TRIALS
            );
            assert!(
                correct_1 >= REQUIRED_TRIALS,
                "assertion failed for size {} and log2m {}: correct_1 = {} < {}",
                size,
                log2m,
                correct_1,
                REQUIRED_TRIALS
            );
        }
    }
}

#[test]
fn test_merge_array() {
    let sizes = SIZES;
    let log2ms = [4, 6, 8, 12];

    for &size in sizes {
        for log2m in log2ms {
            let rsd = HyperLogLog::rel_std(log2m);
            let mut correct_0 = 0;
            let mut correct_1 = 0;

            for trial in 0..NUM_TRIALS {
                let logic = HyperLogLogBuilder::new(size)
                    .word_type::<u16>()
                    .log_2_num_reg(log2m)
                    .build_hasher(Xxh3Builder::new().with_seed(trial))
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
                "assertion failed for size {} and log2m {}: correct_0 = {} < {}",
                size,
                log2m,
                correct_0,
                REQUIRED_TRIALS
            );
            assert!(
                correct_1 >= REQUIRED_TRIALS,
                "assertion failed for size {} and log2m {}: correct_1 = {} < {}",
                size,
                log2m,
                correct_1,
                REQUIRED_TRIALS
            );
        }
    }
}
