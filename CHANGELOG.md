# Change Log

## [Unreleased]

### Changed

- `HyperLogLogBuilder::log2_num_regs` and `HyperLogLog8Builder::log2_num_regs`
  now panic if the argument is greater than 31, and `rsd` now panics if the
  argument is not a positive finite number.

- `HyperLogLogBuilder::min_log2_num_regs` never returns less than 4, the
  minimum accepted by `log2_num_regs`.

- Merge methods now check the length of both backends with a hard assertion,
  like `add` and `estimate`; the same happens for the backends passed to
  `SyncSliceEstimatorArray::get`/`set`, and out-of-bounds indices now panic
  instead of being silently ignored.

- `Debug` and `PartialEq` for `HyperLogLog` and `HyperLogLog8` are now
  implemented manually and no longer require bounds on the item type `T`;
  several unnecessary trait bounds (e.g., `H: Clone` on getters, `L: Clone`
  on `AsRef`/`AsMut` for `DefaultEstimator`) have been removed.

- The default type of the third parameter of `SyncSliceEstimatorArray` is
  now `Box<[SyncCell<W>]>`, as the previous default `Box<[W]>` satisfied no
  implementation.

- The `Display` implementations of `HyperLogLog` and `HyperLogLog8` now
  print the relative standard deviation with five decimal digits.

### Fixed

- `HyperLogLog::register_size` misplaced a division by ln 2, computing
  ln(log₂ n / ln 2) instead of log₂ log₂ n: as a result, 6-bit registers
  were never used, even for more than 2³² distinct elements.

- `HyperLogLog8`'s SIMD merge kernels relied on debug-only assertions for
  memory safety: in release builds, merging backends of the wrong length
  could access memory out of bounds.

## [0.6.0] - 2026-03-21

### New

- New `HyperLogLog8` estimation logic with byte-sized register uses
  33 to 60% extra space but is an order of magnitude faster.
  
### Fixed

- `HyperLogLogBuilder::build` now returns a `Result`, and will return
  an error if the register size and number is incompatible with the
  word of the backend.
  
### Changed

- `HyperLogLogBuilder::build` now returns a `Result`, and will return
  an error if the register size and number is incompatible with the
  word of the backend.

### Fixed

- `HyperLogLog::get_register_unchecked` and
  `HyperLogLog::set_register_unchecked` are now `unsafe`.

- `HyperLogLog::add` and `HyperLogLog::estimate` now check the length of the
  backend.

- `HyperLogLogBuilder::rsd` can no longer set the `log2_num_reg` parameter to
  values smaller than 4.

## [0.5.0] - 2026-03-19

### Improved

- New slightly faster estimation of HyperLogLog cardinality using
  fabricated `f64` values.

### Changed

- `HyperLogLog` methods names use `log2` instead of `log_2` and `regs`
  instead of `registers`.

- `HyperLogLogBuilder` now returns meaningful errors instead of panicking when
  parameters are out of bounds.

## [0.4.0] - 2026-03-15

### New

- New function `HyperLogLogBuilder::min_log_2_num_reg`.

### Changed

- Removed dependency from `num-traits`.

## [0.3.1] - 2026-03-07

### Changed

- Fixed `num-primitive` version at 0.2.1.

## [0.3.0] - 2026-03-07

### Changed

- Removed dependency from `common_traits`, `sux`, `value_traits`, and `anyhow`,
  replacing `common_traits` with `num-primitive` and `num-traits`.

## [0.2.1] - 2026-02-15

### Changed

- Updated all dependencies.

- `slow_tests` feature for slow tests.

### Improved

- Implemented `DefaultEstimator::into_owned`.

## [0.2.0] - 2025-10-16

### Changed

- Updated all dependencies.

## [0.1.0] - 2025-04-07

### New

- First release.
