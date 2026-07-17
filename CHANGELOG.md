# Change Log

## [0.6.0] - 2026-03-21

### New

- New `HyperLogLog8` estimation logic with byte-sized register uses
  33 to 60% extra space but is an order of magnitude faster.
- New `HyperLogLogVl` estimation logic. Counters start as an exact, prefix-coded value list and promote in place to a byte-identical dense `HyperLogLog` on overflow, improving accuracy at low cardinality on power-law graphs.

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
