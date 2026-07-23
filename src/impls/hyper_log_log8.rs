/*
 * SPDX-FileCopyrightText: 2025 Sebastiano Vigna
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

//! A [`HyperLogLog8`] estimation logic with byte-sized registers, using
//! SIMD-accelerated merges, together with its
//! [builder].
//!
//! [builder]: HyperLogLog8Builder

use super::DefaultEstimator;
use super::hyper_log_log::apply_correction;
use crate::traits::{
    EstimationLogic, MergeEstimationLogic, SliceEstimationLogic, assert_backend_len,
};
use std::borrow::Borrow;
use std::hash::*;
use std::{fmt, marker::PhantomData};

/// Estimation logic implementing the HyperLogLog algorithm with byte-sized
/// registers.
///
/// This implementation uses a full byte for each register instead of packed 5–
/// or 6-bit registers. This approach trades 60% (for 5-bit registers) or 33.3%
/// (for 6-bit registers) extra space with respect to
/// [`HyperLogLog`] for:
///
/// - fast register access (byte indexing instead of bit-field extraction);
/// - no backend alignment constraints;
/// - SIMD-accelerated merge using platform-specific byte-wise maximum
///   instructions (SSE2 on `x86`/`x86_64`, NEON on `aarch64`);
/// - no temporary allocations for merge operations.
///
/// The choice between the two logics should be guided by the specific use case
/// and constraints of your application. Please try the included benchmarks to
/// have an idea of the difference in performance between the two logics in your
/// environment.
///
/// Instances are created through [`HyperLogLog8Builder`]:
///
/// ```
/// # use card_est_array::impls::HyperLogLog8Builder;
/// // Default: LogLog-β correction enabled
/// let logic = HyperLogLog8Builder::new()
///     .log2_num_regs(8)
///     .build::<String>();
///
/// // Disable LogLog-β, use classic HLL + linear-counting fallback
/// let logic = HyperLogLog8Builder::new()
///     .log2_num_regs(8)
///     .beta::<false>()
///     .build::<String>();
/// ```
///
/// # Type parameters
///
/// - `T`: the type of elements to count (must implement [`Hash`]).
///
/// - `H`: the [`BuildHasher`] used to hash elements.
///
/// - `BETA`: when `true` (the default), the
///   [LogLog-β] bias correction is used
///   during estimation. See [`HyperLogLog`] for details.
///
/// [`HyperLogLog`]: super::HyperLogLog
/// [LogLog-β]: super::hyper_log_log::beta_horner
pub struct HyperLogLog8<T, H, const BETA: bool = true> {
    build_hasher: H,
    num_regs_minus_1: u64,
    log2_num_regs: u32,
    num_regs: usize,
    alpha_m_m: f64,
    _marker: PhantomData<T>,
}

// We implement Clone, Debug, and PartialEq manually because we do not want
// to require the corresponding trait on T, which appears only in PhantomData.

impl<T, H: fmt::Debug, const BETA: bool> fmt::Debug for HyperLogLog8<T, H, BETA> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HyperLogLog8")
            .field("build_hasher", &self.build_hasher)
            .field("num_regs_minus_1", &self.num_regs_minus_1)
            .field("log2_num_regs", &self.log2_num_regs)
            .field("num_regs", &self.num_regs)
            .field("alpha_m_m", &self.alpha_m_m)
            .finish()
    }
}

impl<T, H: PartialEq, const BETA: bool> PartialEq for HyperLogLog8<T, H, BETA> {
    fn eq(&self, other: &Self) -> bool {
        self.build_hasher == other.build_hasher
            && self.num_regs_minus_1 == other.num_regs_minus_1
            && self.log2_num_regs == other.log2_num_regs
            && self.num_regs == other.num_regs
            && self.alpha_m_m == other.alpha_m_m
    }
}

impl<T, H: Clone, const BETA: bool> Clone for HyperLogLog8<T, H, BETA> {
    fn clone(&self) -> Self {
        Self {
            build_hasher: self.build_hasher.clone(),
            num_regs_minus_1: self.num_regs_minus_1,
            log2_num_regs: self.log2_num_regs,
            num_regs: self.num_regs,
            alpha_m_m: self.alpha_m_m,
            _marker: PhantomData,
        }
    }
}

impl<T, H, const BETA: bool> HyperLogLog8<T, H, BETA> {
    /// Returns the base-2 logarithm of the number of registers per estimator.
    pub fn log2_num_regs(&self) -> u32 {
        self.log2_num_regs
    }
}

impl<T: Hash, H: BuildHasher + Clone, const BETA: bool> SliceEstimationLogic<u8>
    for HyperLogLog8<T, H, BETA>
{
    #[inline(always)]
    fn backend_len(&self) -> usize {
        self.num_regs
    }
}

impl<T: Hash, H: BuildHasher + Clone, const BETA: bool> EstimationLogic
    for HyperLogLog8<T, H, BETA>
{
    type Item = T;
    type Backend = [u8];
    type Estimator<'a>
        = DefaultEstimator<Self, &'a Self, Box<[u8]>>
    where
        T: 'a,
        H: 'a;

    fn new_estimator(&self) -> Self::Estimator<'_> {
        Self::Estimator::new(self, vec![0u8; self.num_regs].into_boxed_slice())
    }

    fn add(&self, backend: &mut Self::Backend, element: impl Borrow<T>) {
        assert_backend_len!(self, backend);
        let hash = self.build_hasher.hash_one(element.borrow());
        let register = (hash & self.num_regs_minus_1) as usize;
        let r = hash.rotate_right(self.log2_num_regs).trailing_zeros();

        debug_assert!(register < self.num_regs);

        backend[register] = backend[register].max(r as u8 + 1);
    }

    fn estimate(&self, backend: &[u8]) -> f64 {
        assert_backend_len!(self, backend);
        let mut harmonic_mean = 0.0;
        let mut zeroes = 0usize;

        for &value in backend {
            if value == 0 {
                zeroes += 1;
            }
            // 2⁻ᵛ via IEEE 754: exponent = 1023 − v, zero mantissa.
            harmonic_mean += f64::from_bits((1023 - value as u64) << 52);
        }

        apply_correction::<BETA>(
            harmonic_mean,
            zeroes,
            self.num_regs,
            self.log2_num_regs,
            self.alpha_m_m,
        )
    }

    #[inline(always)]
    fn clear(&self, backend: &mut [u8]) {
        backend.fill(0);
    }

    #[inline(always)]
    fn set(&self, dst: &mut [u8], src: &[u8]) {
        debug_assert_eq!(dst.len(), src.len());
        dst.copy_from_slice(src);
    }
}

impl<T: Hash, H: BuildHasher + Clone, const BETA: bool> MergeEstimationLogic
    for HyperLogLog8<T, H, BETA>
{
    type Helper = ();

    fn new_helper(&self) -> Self::Helper {}

    #[inline(always)]
    fn merge_with_helper(&self, dst: &mut [u8], src: &[u8], _helper: &mut Self::Helper) {
        // These are hard assertions because the SIMD kernels of
        // merge_max_bytes access memory in 16-byte blocks under the
        // assumption that the length is the backend length (a multiple of 16,
        // as log2_num_regs >= 4).
        assert_backend_len!(self, dst);
        assert_backend_len!(self, src);
        merge_max_bytes(dst, src);
    }
}

impl<T, H, const BETA: bool> fmt::Display for HyperLogLog8<T, H, BETA> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "HyperLogLog8 with relative standard deviation {:.5}% ({} registers/estimator, 8 bits/register, {} bytes/estimator)",
            100.0 * super::HyperLogLog::rel_std(self.log2_num_regs),
            self.num_regs,
            self.num_regs,
        )
    }
}

/// Builder for [`HyperLogLog8`] cardinality-estimation logic.
///
/// The builder lets you configure:
/// - the number of registers, either directly
///   ([`log2_num_regs`]) or via a target relative
///   standard deviation ([`rsd`]);
/// - the hash function ([`build_hasher`]);
/// - whether [LogLog-β bias correction] is
///   enabled ([`beta`]).
///
/// Call [`build`] to obtain the configured [`HyperLogLog8`]
/// logic.
///
/// [`log2_num_regs`]: Self::log2_num_regs
/// [`rsd`]: Self::rsd
/// [`build_hasher`]: Self::build_hasher
/// [LogLog-β bias correction]: super::hyper_log_log::beta_horner
/// [`beta`]: Self::beta
/// [`build`]: Self::build
#[derive(Debug, Clone)]
pub struct HyperLogLog8Builder<H, const BETA: bool = true> {
    build_hasher: H,
    log2_num_regs: u32,
}

impl HyperLogLog8Builder<BuildHasherDefault<DefaultHasher>> {
    /// Creates a new builder for a [`HyperLogLog8`] logic.
    pub const fn new() -> Self {
        Self {
            build_hasher: BuildHasherDefault::new(),
            log2_num_regs: 4,
        }
    }
}

impl Default for HyperLogLog8Builder<BuildHasherDefault<DefaultHasher>> {
    fn default() -> Self {
        Self::new()
    }
}

impl<H, const BETA: bool> HyperLogLog8Builder<H, BETA> {
    /// Sets the desired relative standard deviation.
    ///
    /// This is a high-level alternative to [`Self::log2_num_regs`]. Calling one
    /// after the other invalidates the work done by the first one.
    ///
    /// # Arguments
    /// * `rsd`: the relative standard deviation to be attained.
    ///
    /// # Panics
    ///
    /// If `rsd` is not a positive finite number, or so small that the
    /// logarithm of the required number of registers exceeds 31.
    pub fn rsd(self, rsd: f64) -> Self {
        assert!(
            rsd.is_finite() && rsd > 0.0,
            "the relative standard deviation must be a positive finite number"
        );
        self.log2_num_regs(super::HyperLogLog::log2_num_of_registers(rsd))
    }

    /// Sets the base-2 logarithm of the number of registers.
    ///
    /// This is a low-level alternative to [`Self::rsd`]. Calling one after the
    /// other invalidates the work done by the first one.
    ///
    /// # Arguments
    /// * `log2_num_regs`: the logarithm of the number of registers per
    ///   estimator.
    ///
    /// # Panics
    ///
    /// If `log2_num_regs` is less than 4 or greater than 31.
    pub const fn log2_num_regs(mut self, log2_num_regs: u32) -> Self {
        assert!(
            log2_num_regs >= 4,
            "the logarithm of the number of registers per estimator should be at least 4"
        );
        assert!(
            log2_num_regs <= 31,
            "the logarithm of the number of registers per estimator should be at most 31"
        );
        self.log2_num_regs = log2_num_regs;
        self
    }

    /// Enables or disables the [LogLog-β bias
    /// correction] in the estimate.
    ///
    /// See [`HyperLogLog8`] for details.
    ///
    /// [LogLog-β bias correction]: super::hyper_log_log::beta_horner
    pub fn beta<const BETA2: bool>(self) -> HyperLogLog8Builder<H, BETA2> {
        HyperLogLog8Builder {
            build_hasher: self.build_hasher,
            log2_num_regs: self.log2_num_regs,
        }
    }

    /// Sets the [`BuildHasher`] to use.
    ///
    /// Using this method you can select a specific hasher based on one or more
    /// seeds.
    pub fn build_hasher<H2>(self, build_hasher: H2) -> HyperLogLog8Builder<H2, BETA> {
        HyperLogLog8Builder {
            log2_num_regs: self.log2_num_regs,
            build_hasher,
        }
    }

    /// Builds the logic.
    ///
    /// The type of objects the estimators keep track of is defined here by `T`,
    /// but it is usually inferred by the compiler.
    pub fn build<T>(self) -> HyperLogLog8<T, H, BETA> {
        let log2_num_regs = self.log2_num_regs;
        let num_regs = 1usize << log2_num_regs;
        let alpha = match log2_num_regs {
            4 => 0.673,
            5 => 0.697,
            6 => 0.709,
            _ => 0.7213 / (1.0 + 1.079 / num_regs as f64),
        };

        HyperLogLog8 {
            num_regs,
            num_regs_minus_1: (num_regs - 1) as u64,
            log2_num_regs,
            alpha_m_m: alpha * (num_regs as f64).powi(2),
            build_hasher: self.build_hasher,
            _marker: PhantomData,
        }
    }
}

// ─── SIMD merge: byte-wise maximum ─────────────────────────────────────

#[allow(dead_code)]
fn merge_max_bytes_scalar(dst: &mut [u8], src: &[u8]) {
    for (d, &s) in dst.iter_mut().zip(src.iter()) {
        *d = (*d).max(s);
    }
}

#[cfg(target_arch = "x86")]
use std::arch::x86::*;
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse2")]
unsafe fn merge_max_bytes_sse2(dst: &mut [u8], src: &[u8]) {
    debug_assert_eq!(dst.len(), src.len());
    debug_assert!(dst.len() % 16 == 0);

    let n = dst.len();
    let dst_ptr = dst.as_mut_ptr();
    let src_ptr = src.as_ptr();
    let mut i = 0;
    while i < n {
        // SAFETY: i + 16 <= n because n is a multiple of 16 and i steps by 16.
        unsafe {
            let a = _mm_loadu_si128(dst_ptr.add(i) as *const __m128i);
            let b = _mm_loadu_si128(src_ptr.add(i) as *const __m128i);
            let max = _mm_max_epu8(a, b);
            _mm_storeu_si128(dst_ptr.add(i) as *mut __m128i, max);
        }
        i += 16;
    }
}

#[cfg(target_arch = "aarch64")]
use std::arch::aarch64::*;

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn merge_max_bytes_neon(dst: &mut [u8], src: &[u8]) {
    debug_assert_eq!(dst.len(), src.len());
    debug_assert!(dst.len() % 16 == 0);

    let n = dst.len();
    let dst_ptr = dst.as_mut_ptr();
    let src_ptr = src.as_ptr();
    let mut i = 0;
    while i < n {
        // SAFETY: i + 16 <= n because n is a multiple of 16 and i steps by 16.
        unsafe {
            let a = vld1q_u8(dst_ptr.add(i));
            let b = vld1q_u8(src_ptr.add(i));
            let max = vmaxq_u8(a, b);
            vst1q_u8(dst_ptr.add(i), max);
        }
        i += 16;
    }
}

#[cfg(target_arch = "x86_64")]
fn merge_max_bytes(dst: &mut [u8], src: &[u8]) {
    // SAFETY: SSE2 is always available on x86_64.
    unsafe { merge_max_bytes_sse2(dst, src) }
}

#[cfg(target_arch = "x86")]
fn merge_max_bytes(dst: &mut [u8], src: &[u8]) {
    if is_x86_feature_detected!("sse2") {
        // SAFETY: we just verified SSE2 is available.
        unsafe { merge_max_bytes_sse2(dst, src) }
    } else {
        merge_max_bytes_scalar(dst, src)
    }
}

#[cfg(target_arch = "aarch64")]
fn merge_max_bytes(dst: &mut [u8], src: &[u8]) {
    // SAFETY: NEON is always available on aarch64.
    unsafe { merge_max_bytes_neon(dst, src) }
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")))]
fn merge_max_bytes(dst: &mut [u8], src: &[u8]) {
    merge_max_bytes_scalar(dst, src)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Backends of the wrong length must be rejected with a hard assertion:
    /// the SIMD merge kernels rely on the backend length being a multiple of
    /// 16, so a `debug_assert` alone would make out-of-bounds access
    /// reachable from safe code in release builds.
    #[test]
    #[should_panic(expected = "does not match the expected length")]
    fn test_merge_backend_length_mismatch() {
        let logic = HyperLogLog8Builder::new().build::<u64>();
        let mut dst = vec![0u8; 8];
        let src = vec![0u8; 8];
        logic.merge(&mut dst, &src);
    }

    #[test]
    #[should_panic(expected = "at most 31")]
    fn test_log2_num_regs_too_large() {
        let _ = HyperLogLog8Builder::new().log2_num_regs(32);
    }

    /// `Debug`, `Clone`, and `PartialEq` on the logic must not put any bound
    /// on the item type `T`, and inherent methods must not require `H: Clone`.
    #[test]
    fn test_logic_traits_do_not_bound_t() {
        struct Plain;

        #[derive(Clone, Debug, PartialEq)]
        struct Bh;
        impl BuildHasher for Bh {
            type Hasher = DefaultHasher;
            fn build_hasher(&self) -> DefaultHasher {
                DefaultHasher::new()
            }
        }

        let a = HyperLogLog8Builder::new().build_hasher(Bh).build::<Plain>();
        let b = a.clone();
        assert!(!format!("{a:?}").is_empty());
        assert_eq!(a, b);

        // The getter must be available without `H: Clone`.
        struct NonCloneBh;
        impl BuildHasher for NonCloneBh {
            type Hasher = DefaultHasher;
            fn build_hasher(&self) -> DefaultHasher {
                DefaultHasher::new()
            }
        }
        let c = HyperLogLog8Builder::new()
            .build_hasher(NonCloneBh)
            .build::<u64>();
        assert_eq!(c.log2_num_regs(), 4);
    }
}
