/*
 * SPDX-FileCopyrightText: 2024 Matteo Dell'Acqua
 * SPDX-FileCopyrightText: 2025 Sebastiano Vigna
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

//! A [`HyperLogLog`] estimation logic with packed 5- or 6-bit registers,
//! using broadword programming for merges, together with its
//! [builder] and the [LogLog-β correction].
//!
//! [builder]: HyperLogLogBuilder
//! [LogLog-β correction]: beta_horner

use super::DefaultEstimator;
use crate::traits::Word;
use crate::traits::{
    EstimationLogic, MergeEstimationLogic, SliceEstimationLogic, assert_backend_len,
};
use num_primitive::{PrimitiveNumber, PrimitiveNumberAs};
use std::hash::*;
use std::num::NonZeroUsize;
use std::{borrow::Borrow, f64::consts::LN_2, fmt};

/// Error returned by [`HyperLogLogBuilder::build`] when the configuration is
/// invalid.
#[derive(Debug, Clone)]
pub enum HyperLogLogError {
    /// The register size derived from `num_elements` exceeds the maximum
    /// supported by the hash type.
    RegisterSizeTooLarge {
        register_size: usize,
        num_elements: usize,
        hash_bits: u32,
    },
    /// The estimator size in bits is not divisible by the bit width of the
    /// backend word type `W`, so registers cannot be packed exactly into whole
    /// words.
    UnalignedBackend {
        est_size_in_bits: usize,
        word_bits: usize,
        min_alignment: String,
        min_log2_num_regs: u32,
    },
}

impl fmt::Display for HyperLogLogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RegisterSizeTooLarge {
                register_size,
                num_elements,
                hash_bits,
            } => write!(
                f,
                "register size {} (derived from num_elements = {}) exceeds the maximum of 6 \
                supported with a {}-bit hash type; reduce num_elements or use a hash with more bits",
                register_size, num_elements, hash_bits,
            ),
            Self::UnalignedBackend {
                est_size_in_bits,
                word_bits,
                min_alignment,
                min_log2_num_regs,
            } => write!(
                f,
                "estimator size ({} bits) is not divisible by the word size ({} bits), \
                so register backends cannot be aligned; use {} or a smaller unsigned integer type \
                as the word type, or increase log2_num_regs (possibly by reducing \
                the relative standard deviation) to at least {}",
                est_size_in_bits, word_bits, min_alignment, min_log2_num_regs,
            ),
        }
    }
}

impl std::error::Error for HyperLogLogError {}

/// The type returned by the hash function.
type HashResult = u64;

/// LogLog-β polynomial coefficients for `log2_num_regs` in [4 . . 18].
///
/// Indexed as `LOGLOG_BETA[log2_num_regs - 4]`. Each row holds β₀, β₁, …,
/// β₇ used by [`beta_horner`].
const LOGLOG_BETA: [[f64; 8]; 15] = [
    // log2_num_regs =  4
    [
        -0.582581413904517,
        -1.935_300_357_560_05,
        11.079323758035073,
        -22.131357446444323,
        22.505391846630037,
        -12.000723834917984,
        3.220579408194167,
        -0.342225302271235,
    ],
    // log2_num_regs =  5
    [
        -0.7518999460733967,
        -0.959_003_007_774_876,
        5.599_737_132_214_161,
        -8.209_763_699_976_552,
        6.509_125_489_447_204,
        -2.683_029_373_432_373,
        0.5612891113138221,
        -0.0463331622196545,
    ],
    // log2_num_regs =  6
    [
        29.825_790_096_961_963,
        -31.328_708_333_772_592,
        -10.594_252_303_658_228,
        -11.572_012_568_909_962,
        3.818_875_437_390_749,
        -2.416_013_032_853_081,
        0.4542208940970826,
        -0.0575155452020420,
    ],
    // log2_num_regs =  7
    [
        2.810_292_129_082_006,
        -3.9780498518175995,
        1.3162680041351582,
        -3.925_248_633_580_59,
        2.008_083_575_394_647,
        -0.7527151937556955,
        0.1265569894242751,
        -0.0109946438726240,
    ],
    // log2_num_regs =  8
    [
        1.006_335_448_875_505_2,
        -2.005_806_664_051_124,
        1.643_697_493_665_141_2,
        -2.705_608_099_405_661_7,
        1.392_099_802_442_226,
        -0.464_703_742_721_831_9,
        0.07384282377269775,
        -0.00578554885254223,
    ],
    // log2_num_regs =  9
    [
        -0.09415657458167959,
        -0.781_309_759_245_505_3,
        1.715_149_467_507_124_6,
        -1.737_112_504_065_163_4,
        0.864_415_084_890_489_2,
        -0.23819027465047218,
        0.03343448400269076,
        -0.00207858528178157,
    ],
    // log2_num_regs =  10
    [
        -0.25935400670790054,
        -0.525_983_019_998_058_1,
        1.489_330_349_258_768_4,
        -1.296_427_140_849_935_7,
        0.622_847_562_172_216_2,
        -0.156_723_267_702_510_4,
        0.02054415903878563,
        -0.00112488483925502,
    ],
    // log2_num_regs =  11
    [
        -4.32325553856025e-01,
        -1.08450736399632e-01,
        6.09156550741120e-01,
        -1.65687801845180e-02,
        -7.95829341087617e-02,
        4.71830602102918e-02,
        -7.81372902346934e-03,
        5.84268708489995e-04,
    ],
    // log2_num_regs =  12
    [
        -3.84979202588598e-01,
        1.83162233114364e-01,
        1.30396688841854e-01,
        7.04838927629266e-02,
        -8.95893971464453e-03,
        1.13010036741605e-02,
        -1.94285569591290e-03,
        2.25435774024964e-04,
    ],
    // log2_num_regs =  13
    [
        -0.41655270946462997,
        -0.22146677040685156,
        0.38862131236999947,
        0.453_409_797_460_623_7,
        -0.36264738324476375,
        0.12304650053558529,
        -0.017_015_403_845_555_1,
        0.00102750367080838,
    ],
    // log2_num_regs =  14
    [
        -3.71009760230692e-01,
        9.78811941207509e-03,
        1.85796293324165e-01,
        2.03015527328432e-01,
        -1.16710521803686e-01,
        4.31106699492820e-02,
        -5.99583540511831e-03,
        4.49704299509437e-04,
    ],
    // log2_num_regs = 15
    [
        -0.38215145543875273,
        -0.890_694_005_360_908_4,
        0.376_023_357_746_788_7,
        0.993_359_774_406_823_8,
        -0.655_774_416_383_189_6,
        0.183_323_421_297_036_1,
        -0.02241529633062872,
        0.00121399789330194,
    ],
    // log2_num_regs = 16
    [
        -0.373_318_766_437_530_6,
        -1.417_040_774_481_23,
        0.40729184796612533,
        1.561_520_339_065_841_6,
        -0.992_422_335_342_861_3,
        0.260_646_813_994_830_9,
        -0.03053811369682807,
        0.00155770210179105,
    ],
    // log2_num_regs = 17
    [
        -0.36775502299404605,
        0.538_314_223_513_779_7,
        0.769_702_892_787_679_2,
        0.550_025_835_864_505_6,
        -0.745_755_882_611_469_4,
        0.257_118_357_858_219_5,
        -0.03437902606864149,
        0.00185949146371616,
    ],
    // log2_num_regs = 18
    [
        -0.364_796_233_259_605_4,
        0.997_304_123_286_350_3,
        1.553_543_862_300_812_2,
        1.259_326_771_980_289_2,
        -1.533_259_482_091_101_6,
        0.478_010_422_000_565_9,
        -0.05951025172951174,
        0.00291076804642205,
    ],
];

/// Computes the LogLog-β bias correction using Horner's method.
///
/// The method appears in the paper from Jason Qin, Denys Kim & Yumei Tung,
/// “[LogLog-Beta and More: A New Algorithm for Cardinality Estimation Based on
/// LogLog Counting]”, 2016.
///
/// # Panics
///
/// If `log2_num_regs` is not in [4 . . 18].
///
/// [LogLog-Beta and More: A New Algorithm for Cardinality Estimation Based on LogLog Counting]: https://arxiv.org/pdf/1612.02284
pub fn beta_horner(z: f64, log2_num_regs: usize) -> f64 {
    let beta = LOGLOG_BETA[log2_num_regs - 4];
    let zl = (z + 1.0).ln();
    let mut res = 0.0;
    for i in (1..8).rev() {
        res = res * zl + beta[i];
    }
    res * zl + beta[0] * z
}

/// Applies the post-loop correction to the harmonic mean and zero count.
///
/// This function is `#[inline(never)]` so that the register-iteration loop
/// in [`estimate`] compiles identically for both
/// `BETA=true` and `BETA=false`, preventing the post-loop formula from
/// influencing loop optimizations (vectorization, unrolling, scheduling).
///
/// [`estimate`]: EstimationLogic::estimate
#[inline(never)]
pub(crate) fn apply_correction<const BETA: bool>(
    harmonic_mean: f64,
    zeroes: usize,
    num_regs: usize,
    log2_num_regs: u32,
    alpha_m_m: f64,
) -> f64 {
    if BETA && zeroes != 0 && log2_num_regs <= 18 {
        // LogLog-β: a single formula that replaces both the raw HyperLogLog
        // estimate and the linear-counting small-range correction.
        let m = num_regs as f64;
        let z = zeroes as f64;
        let beta = beta_horner(z, log2_num_regs as usize);
        alpha_m_m * (m - z) / (m * (harmonic_mean + beta))
    } else {
        // Classic HyperLogLog raw estimate with linear-counting correction.
        let mut estimate = alpha_m_m / harmonic_mean;
        if zeroes != 0 && estimate < 2.5 * num_regs as f64 {
            let m = num_regs as f64;
            estimate = m * (m / zeroes as f64).ln();
        }
        estimate
    }
}

/// Estimation logic implementing the HyperLogLog algorithm.
///
/// This implementation uses 5- or 6-bit registers and [broadword
/// programming]. It thus uses the
/// minimum possible space, saving 37.5 or 25% space with respect to the
/// [`HyperLogLog8`] logic, which uses 8-bit
/// registers and byte-wise SIMD operations, but it is significantly slower than
/// the latter.
///
/// The choice between the two logics should be guided by the specific use case
/// and constraints of your application. Please try the included benchmarks to
/// have an idea of the difference in performance between the two logics in your
/// environment.
///
/// Instances are created through [`HyperLogLogBuilder`]:
///
/// ```
/// # use card_est_array::impls::HyperLogLogBuilder;
/// // Default: LogLog-β correction enabled, usize backend
/// let logic = HyperLogLogBuilder::new(1_000_000)
///     .log2_num_regs(8)
///     .build::<String>().unwrap();
///
/// // Disable LogLog-β, use classic HyperLogLog + linear-counting fallback
/// let logic = HyperLogLogBuilder::new(1_000_000)
///     .log2_num_regs(8)
///     .beta::<false>()
///     .build::<String>().unwrap();
/// ```
///
/// # Type parameters
///
/// - `T`: the type of elements to count (must implement [`Hash`]).
///
/// - `H`: the [`BuildHasher`] used to hash elements.
///
/// - `W`: the unsigned word type for the register backend (see below).
///
/// - `BETA`: when `true` (the default), the [LogLog-β] bias
///   correction is used during estimation. This provides better accuracy across
///   the full cardinality range through a single formula, eliminating the need
///   for a separate linear-counting correction. The cost is roughly 20ns per
///   estimate call when some registers are still zero; when all registers are
///   populated, the correction is skipped and performance is identical to the
///   classic formula. Set to `false` via [`HyperLogLogBuilder::beta`] to use the
///   original HyperLogLog formula instead.
///
/// # Backend alignment
///
/// `W` must be able to represent exactly the backend of an estimator. While
/// usually `usize` will work (and it is the default type chosen by
/// [`HyperLogLogBuilder::new`]), with odd register sizes and a small number
/// of registers it might be necessary to select a smaller type, resulting in
/// slower merges. For example, using 16 5-bit registers one needs to use
/// `u16`, whereas for 16 6-bit registers `u32` will be sufficient.
///
/// Formally, `W::BITS` must divide `(1 << log2_num_regs) * register_size`
/// (using [`HyperLogLog::register_size(num_elements)`]).
/// [`HyperLogLogBuilder::min_log2_num_regs`] returns the minimum value for
/// `log2_num_regs` that satisfies this property.
///
/// [broadword programming]: https://doi.org/10.1145/1963405.1963493
/// [`HyperLogLog8`]: crate::impls::HyperLogLog8
/// [LogLog-β]: beta_horner
/// [`HyperLogLog::register_size(num_elements)`]: HyperLogLog::register_size
pub struct HyperLogLog<T, H, W = usize, const BETA: bool = true> {
    build_hasher: H,
    register_size: usize,
    num_regs_minus_1: HashResult,
    log2_num_regs: u32,
    sentinel_mask: HashResult,
    num_regs: usize,
    pub(super) words_per_estimator: usize,
    alpha_m_m: f64,
    msb_mask: Box<[W]>,
    lsb_mask: Box<[W]>,
    _marker: std::marker::PhantomData<T>,
}

// We implement Clone, Debug, and PartialEq manually because we do not want
// to require the corresponding trait on T, which appears only in PhantomData.

impl<T, H: fmt::Debug, W: fmt::Debug, const BETA: bool> fmt::Debug for HyperLogLog<T, H, W, BETA> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HyperLogLog")
            .field("build_hasher", &self.build_hasher)
            .field("register_size", &self.register_size)
            .field("num_regs_minus_1", &self.num_regs_minus_1)
            .field("log2_num_regs", &self.log2_num_regs)
            .field("sentinel_mask", &self.sentinel_mask)
            .field("num_regs", &self.num_regs)
            .field("words_per_estimator", &self.words_per_estimator)
            .field("alpha_m_m", &self.alpha_m_m)
            .field("msb_mask", &self.msb_mask)
            .field("lsb_mask", &self.lsb_mask)
            .finish()
    }
}

impl<T, H: PartialEq, W: PartialEq, const BETA: bool> PartialEq for HyperLogLog<T, H, W, BETA> {
    fn eq(&self, other: &Self) -> bool {
        self.build_hasher == other.build_hasher
            && self.register_size == other.register_size
            && self.num_regs_minus_1 == other.num_regs_minus_1
            && self.log2_num_regs == other.log2_num_regs
            && self.sentinel_mask == other.sentinel_mask
            && self.num_regs == other.num_regs
            && self.words_per_estimator == other.words_per_estimator
            && self.alpha_m_m == other.alpha_m_m
            && self.msb_mask == other.msb_mask
            && self.lsb_mask == other.lsb_mask
    }
}

impl<T, H: Clone, W: Clone, const BETA: bool> Clone for HyperLogLog<T, H, W, BETA> {
    fn clone(&self) -> Self {
        Self {
            build_hasher: self.build_hasher.clone(),
            register_size: self.register_size,
            num_regs_minus_1: self.num_regs_minus_1,
            log2_num_regs: self.log2_num_regs,
            sentinel_mask: self.sentinel_mask,
            num_regs: self.num_regs,
            words_per_estimator: self.words_per_estimator,
            alpha_m_m: self.alpha_m_m,
            msb_mask: self.msb_mask.clone(),
            lsb_mask: self.lsb_mask.clone(),
            _marker: std::marker::PhantomData,
        }
    }
}

impl<T, H, W: Word, const BETA: bool> HyperLogLog<T, H, W, BETA> {
    /// Returns the base-2 logarithm of the number of registers per estimator.
    pub fn log2_num_regs(&self) -> u32 {
        self.log2_num_regs
    }

    /// Returns the value contained in a register of a given backend.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `index` is less than the number of
    /// registers per estimator (i.e., 2^[`log2_num_regs`]),
    /// and that `backend` has length at least
    /// [`SliceEstimationLogic::backend_len`].
    ///
    /// [`log2_num_regs`]: Self::log2_num_regs
    #[inline(always)]
    unsafe fn get_register_unchecked(&self, backend: impl AsRef<[W]>, index: usize) -> W {
        let backend = backend.as_ref();
        let bits = W::BITS as usize;
        let bit_width = self.register_size;
        let mask = W::MAX >> (bits - bit_width);
        let pos = index * bit_width;
        let word_index = pos / bits;
        let bit_index = pos % bits;

        if bit_index + bit_width <= bits {
            (unsafe { *backend.get_unchecked(word_index) } >> bit_index) & mask
        } else {
            ((unsafe { *backend.get_unchecked(word_index) } >> bit_index)
                | (unsafe { *backend.get_unchecked(word_index + 1) } << (bits - bit_index)))
                & mask
        }
    }

    /// Sets the value contained in a register of a given backend.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `index` is less than the number of
    /// registers per estimator (i.e., 2^[`log2_num_regs`]),
    /// and that `backend` has length at least
    /// [`SliceEstimationLogic::backend_len`].
    ///
    /// [`log2_num_regs`]: Self::log2_num_regs
    #[inline(always)]
    unsafe fn set_register_unchecked(
        &self,
        mut backend: impl AsMut<[W]>,
        index: usize,
        new_value: W,
    ) {
        let backend = backend.as_mut();
        let bits = W::BITS as usize;
        let bit_width = self.register_size;
        let mask = W::MAX >> (bits - bit_width);
        let pos = index * bit_width;
        let word_index = pos / bits;
        let bit_index = pos % bits;

        if bit_index + bit_width <= bits {
            let mut word = unsafe { *backend.get_unchecked_mut(word_index) };
            word &= !(mask << bit_index);
            word |= new_value << bit_index;
            unsafe { *backend.get_unchecked_mut(word_index) = word };
        } else {
            let mut word = unsafe { *backend.get_unchecked_mut(word_index) };
            word &= (W::ONE << bit_index) - W::ONE;
            word |= new_value << bit_index;
            unsafe { *backend.get_unchecked_mut(word_index) = word };

            let mut word = unsafe { *backend.get_unchecked_mut(word_index + 1) };
            word &= !(mask >> (bits - bit_index));
            word |= new_value >> (bits - bit_index);
            unsafe { *backend.get_unchecked_mut(word_index + 1) = word };
        }
    }
}

impl<T: Hash, H: BuildHasher + Clone, W: Word, const BETA: bool> SliceEstimationLogic<W>
    for HyperLogLog<T, H, W, BETA>
where
    u32: PrimitiveNumberAs<W>,
{
    #[inline(always)]
    fn backend_len(&self) -> usize {
        self.words_per_estimator
    }
}

impl<T: Hash, H: BuildHasher + Clone, W: Word, const BETA: bool> EstimationLogic
    for HyperLogLog<T, H, W, BETA>
where
    u32: PrimitiveNumberAs<W>,
{
    type Item = T;
    type Backend = [W];
    type Estimator<'a>
        = DefaultEstimator<Self, &'a Self, Box<[W]>>
    where
        T: 'a,
        W: 'a,
        H: 'a;

    fn new_estimator(&self) -> Self::Estimator<'_> {
        Self::Estimator::new(
            self,
            vec![W::ZERO; self.words_per_estimator].into_boxed_slice(),
        )
    }

    fn add(&self, backend: &mut Self::Backend, element: impl Borrow<T>) {
        assert_backend_len!(self, backend);
        let hash = self.build_hasher.hash_one(element.borrow());
        let register = (hash & self.num_regs_minus_1) as usize;
        // The number of trailing zeroes is certainly expressible in
        // a variable of type W
        let r = ((hash >> self.log2_num_regs) | self.sentinel_mask)
            .trailing_zeros()
            .as_to();

        debug_assert!(r < (W::ONE << self.register_size) - W::ONE);
        debug_assert!(register < self.num_regs);

        let current_value = unsafe { self.get_register_unchecked(&*backend, register) };
        let candidate_value = r + W::ONE;
        let new_value = std::cmp::max(current_value, candidate_value);
        if current_value != new_value {
            unsafe { self.set_register_unchecked(backend, register, new_value) };
        }
    }

    fn estimate(&self, backend: &[W]) -> f64 {
        assert_backend_len!(self, backend);
        let mut harmonic_mean = 0.0;
        let mut zeroes = 0usize;

        for i in 0..self.num_regs {
            let value: usize = unsafe { self.get_register_unchecked(backend, i).as_to() };
            if value == 0 {
                zeroes += 1;
            }
            // 2⁻ᵛ via IEEE 754: exponent = 1023 − v, zero mantissa.
            debug_assert!(value <= 1023);
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
    fn clear(&self, backend: &mut [W]) {
        backend.fill(W::ZERO);
    }

    #[inline(always)]
    fn set(&self, dst: &mut [W], src: &[W]) {
        debug_assert_eq!(dst.len(), src.len());
        dst.copy_from_slice(src);
    }
}

/// Helper for merge operations with [`HyperLogLog`] logic.
pub struct HyperLogLogHelper<W> {
    acc: Vec<W>,
    mask: Vec<W>,
}

impl<T: Hash, H: BuildHasher + Clone, W: Word, const BETA: bool> MergeEstimationLogic
    for HyperLogLog<T, H, W, BETA>
where
    u32: PrimitiveNumberAs<W>,
{
    type Helper = HyperLogLogHelper<W>;

    fn new_helper(&self) -> Self::Helper {
        HyperLogLogHelper {
            acc: vec![W::ZERO; self.words_per_estimator],
            mask: vec![W::ZERO; self.words_per_estimator],
        }
    }

    #[inline(always)]
    fn merge_with_helper(&self, dst: &mut [W], src: &[W], helper: &mut Self::Helper) {
        assert_backend_len!(self, dst);
        assert_backend_len!(self, src);
        merge_hyperloglog_bitwise(
            dst,
            src,
            self.msb_mask.as_ref(),
            self.lsb_mask.as_ref(),
            &mut helper.acc,
            &mut helper.mask,
            self.register_size,
        );
    }
}

impl<T, H, W, const BETA: bool> std::fmt::Display for HyperLogLog<T, H, W, BETA> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "HyperLogLog with relative standard deviation {:.5}% ({} registers/estimator, {} bits/register, {} bytes/estimator)",
            100.0 * HyperLogLog::rel_std(self.log2_num_regs),
            self.num_regs,
            self.register_size,
            (self.num_regs * self.register_size) / 8
        )
    }
}

/// Builder for [`HyperLogLog`] cardinality-estimation logic.
///
/// The builder lets you configure:
/// - the upper bound on the number of distinct elements
///   ([`new`] / [`num_elements`]);
/// - the number of registers, either directly
///   ([`log2_num_regs`]) or via a target relative
///   standard deviation ([`rsd`]);
/// - the backend word type ([`word_type`]);
/// - the hash function ([`build_hasher`]);
/// - whether [LogLog-β bias correction] is enabled
///   ([`beta`]).
///
/// Call [`build`] to obtain the configured [`HyperLogLog`]
/// logic.
///
/// [`new`]: Self::new
/// [`num_elements`]: Self::num_elements
/// [`log2_num_regs`]: Self::log2_num_regs
/// [`rsd`]: Self::rsd
/// [`word_type`]: Self::word_type
/// [`build_hasher`]: Self::build_hasher
/// [LogLog-β bias correction]: beta_horner
/// [`beta`]: Self::beta
/// [`build`]: Self::build
#[derive(Debug, Clone)]
pub struct HyperLogLogBuilder<H, W = usize, const BETA: bool = true> {
    build_hasher: H,
    log2_num_regs: u32,
    num_elements: usize,
    _marker: std::marker::PhantomData<W>,
}

impl HyperLogLogBuilder<BuildHasherDefault<DefaultHasher>> {
    /// Creates a new builder for a [`HyperLogLog`] logic with the default word
    /// type (the fixed-size equivalent of `usize`).
    ///
    /// # Panics
    ///
    /// If `num_elements` is zero.
    pub const fn new(num_elements: usize) -> Self {
        assert!(
            num_elements > 0,
            "the upper bound on the number of distinct elements must be positive"
        );
        Self {
            build_hasher: BuildHasherDefault::new(),
            log2_num_regs: 4,
            num_elements,
            _marker: std::marker::PhantomData,
        }
    }
}

fn min_alignment(bits: usize) -> String {
    if bits % 128 == 0 {
        "u128"
    } else if bits % 64 == 0 {
        "u64"
    } else if bits % 32 == 0 {
        "u32"
    } else if bits % 16 == 0 {
        "u16"
    } else {
        "u8"
    }
    .to_string()
}

impl HyperLogLog<(), (), (), true> {
    /// Returns the logarithm of the number of registers per estimator that are
    /// necessary to attain a given relative standard deviation.
    ///
    /// # Arguments
    /// * `rsd`: the relative standard deviation to be attained.
    pub fn log2_num_of_registers(rsd: f64) -> u32 {
        (((1.106 / rsd).powi(2)).log2().ceil() as u32).max(4)
    }

    /// Returns the relative standard deviation corresponding to a given number
    /// of registers per estimator.
    ///
    /// # Arguments
    ///
    /// * `log2_num_regs`: the logarithm of the number of registers per
    ///   estimator.
    pub fn rel_std(log2_num_regs: u32) -> f64 {
        let tmp = match log2_num_regs {
            4 => 1.106,
            5 => 1.070,
            6 => 1.054,
            7 => 1.046,
            _ => 1.04,
        };
        tmp / ((1u64 << log2_num_regs) as f64).sqrt()
    }

    /// Returns the register size in bits, given an upper bound on the number of
    /// distinct elements.
    ///
    /// # Arguments
    /// * `num_elements`: an upper bound on the number of distinct elements.
    pub fn register_size(num_elements: usize) -> usize {
        std::cmp::max(
            5,
            (((num_elements as f64).ln() / LN_2).ln() / LN_2).ceil() as usize,
        )
    }
}

impl<H, W: Word, const BETA: bool> HyperLogLogBuilder<H, W, BETA> {
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
        self.log2_num_regs(HyperLogLog::log2_num_of_registers(rsd))
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

    /// Returns the minimum value allowed for [`Self::log2_num_regs`] given the
    /// current value of [`Self::num_elements`].
    pub fn min_log2_num_regs(&self) -> u32 {
        let register_size = HyperLogLog::register_size(self.num_elements);
        let register_size =
            NonZeroUsize::try_from(register_size).expect("register size should be nonzero");
        let min_num_regs = W::BITS / highest_power_of_2_dividing(register_size);
        assert_eq!(min_num_regs, min_num_regs.next_power_of_two());
        // The backend alignment might allow fewer than 16 registers, but
        // log2_num_regs never goes below 4.
        min_num_regs.trailing_zeros().max(4) // log2(min_num_regs)
    }

    /// Sets the type `W` to use to represent backends.
    ///
    /// Note that the returned builder will have a different type if `W2` is
    /// different from `W`.
    ///
    /// See the [logic documentation] for the limitations on the
    /// choice of `W2`.
    ///
    /// [logic documentation]: HyperLogLog
    pub fn word_type<W2>(self) -> HyperLogLogBuilder<H, W2, BETA> {
        HyperLogLogBuilder {
            num_elements: self.num_elements,
            build_hasher: self.build_hasher,
            log2_num_regs: self.log2_num_regs,
            _marker: std::marker::PhantomData,
        }
    }

    /// Enables or disables the [LogLog-β bias correction] in
    /// the estimate.
    ///
    /// When enabled (the default), the estimate uses the LogLog-β formula for
    /// `log2_num_regs` 4–18, which provides better accuracy across the full
    /// cardinality range without a separate linear-counting correction, at the
    /// cost of roughly 20ns per estimate call when some registers are still
    /// zero. When all registers are populated, the correction is skipped and
    /// performance is identical to the classic formula.
    ///
    /// When disabled, the classic HyperLogLog formula with linear-counting
    /// fallback is used.
    ///
    /// ```
    /// # use card_est_array::impls::HyperLogLogBuilder;
    /// // Disable LogLog-β correction
    /// let logic = HyperLogLogBuilder::new(1_000_000)
    ///     .log2_num_regs(8)
    ///     .beta::<false>()
    ///     .build::<usize>().unwrap();
    /// ```
    ///
    /// [LogLog-β bias correction]: beta_horner
    pub fn beta<const BETA2: bool>(self) -> HyperLogLogBuilder<H, W, BETA2> {
        HyperLogLogBuilder {
            num_elements: self.num_elements,
            build_hasher: self.build_hasher,
            log2_num_regs: self.log2_num_regs,
            _marker: std::marker::PhantomData,
        }
    }

    /// Sets the upper bound on the number of elements.
    ///
    /// # Panics
    ///
    /// If `n` is zero.
    pub const fn num_elements(mut self, num_elements: usize) -> Self {
        assert!(
            num_elements > 0,
            "the upper bound on the number of distinct elements must be positive"
        );
        self.num_elements = num_elements;
        self
    }

    /// Sets the [`BuildHasher`] to use.
    ///
    /// Using this method you can select a specific hasher based on one or more
    /// seeds.
    pub fn build_hasher<H2>(self, build_hasher: H2) -> HyperLogLogBuilder<H2, W, BETA> {
        HyperLogLogBuilder {
            num_elements: self.num_elements,
            log2_num_regs: self.log2_num_regs,
            build_hasher,
            _marker: std::marker::PhantomData,
        }
    }

    /// Builds the logic.
    ///
    /// The type of objects the estimators keep track of is defined here by `T`,
    /// but it is usually inferred by the compiler.
    ///
    /// # Errors
    ///
    /// Returns [`HyperLogLogError::RegisterSizeTooLarge`] if the register size
    /// derived from `num_elements` exceeds the maximum supported by the hash
    /// type.
    ///
    /// Returns [`HyperLogLogError::UnalignedBackend`] if the estimator size in
    /// bits is not divisible by the bit width of `W`.
    pub fn build<T>(self) -> Result<HyperLogLog<T, H, W, BETA>, HyperLogLogError> {
        let bits = W::BITS as usize;
        let log2_num_regs = self.log2_num_regs;
        let num_elements = self.num_elements;
        let number_of_registers = 1usize << log2_num_regs;
        let register_size = HyperLogLog::register_size(num_elements);
        if register_size > 6 {
            return Err(HyperLogLogError::RegisterSizeTooLarge {
                register_size,
                num_elements,
                hash_bits: HashResult::BITS,
            });
        }
        let sentinel_mask = 1 << ((1 << register_size) - 2);
        let alpha = match log2_num_regs {
            4 => 0.673,
            5 => 0.697,
            6 => 0.709,
            _ => 0.7213 / (1.0 + 1.079 / number_of_registers as f64),
        };
        let num_regs_minus_1 = (number_of_registers - 1) as HashResult;

        let est_size_in_bits = number_of_registers
            .checked_mul(register_size)
            .expect("the estimator size in bits should fit a usize");

        // This ensures estimators are always aligned to W
        if est_size_in_bits % bits != 0 {
            return Err(HyperLogLogError::UnalignedBackend {
                est_size_in_bits,
                word_bits: bits,
                min_alignment: min_alignment(est_size_in_bits),
                min_log2_num_regs: self.min_log2_num_regs(),
            });
        }
        let est_size_in_words = est_size_in_bits / bits;

        let msb_mask = build_register_mask(
            est_size_in_words,
            register_size,
            W::ONE << (register_size - 1),
        );
        let lsb_mask = build_register_mask(est_size_in_words, register_size, W::ONE);

        Ok(HyperLogLog {
            num_regs: number_of_registers,
            num_regs_minus_1,
            log2_num_regs,
            register_size,
            alpha_m_m: alpha * (number_of_registers as f64).powi(2),
            sentinel_mask,
            build_hasher: self.build_hasher,
            msb_mask,
            lsb_mask,
            words_per_estimator: est_size_in_words,
            _marker: std::marker::PhantomData,
        })
    }
}

/// Builds a mask of `num_words` words by repeating a `register_size`-bit
/// pattern across all register positions.
fn build_register_mask<W: Word>(num_words: usize, register_size: usize, pattern: W) -> Box<[W]> {
    let bits = W::BITS as usize;
    let total_bits = num_words * bits;
    let mut result = vec![W::ZERO; num_words];
    let mut bit_pos = 0;
    while bit_pos < total_bits {
        let word_index = bit_pos / bits;
        let bit_index = bit_pos % bits;
        result[word_index] |= pattern << bit_index;
        if bit_index + register_size > bits && word_index + 1 < num_words {
            result[word_index + 1] |= pattern >> (bits - bit_index);
        }
        bit_pos += register_size;
    }
    result.into_boxed_slice()
}

/// Performs a multiple precision subtraction, leaving the result in the first operand.
/// The operands MUST have the same length.
///
/// # Arguments
/// * `x`: the first operand. This will contain the final result.
/// * `y`: the second operand that will be subtracted from `x`.
#[inline(always)]
pub(super) fn subtract<W: Word>(x: &mut [W], y: &[W]) {
    debug_assert_eq!(x.len(), y.len());
    let mut borrow = false;

    for (x_word, &y) in x.iter_mut().zip(y.iter()) {
        let mut x = *x_word;
        if !borrow {
            borrow = x < y;
        } else if x != W::ZERO {
            x = x.wrapping_sub(W::ONE);
            borrow = x < y;
        } else {
            x = x.wrapping_sub(W::ONE);
        }
        *x_word = x.wrapping_sub(y);
    }
}

fn merge_hyperloglog_bitwise<W: Word>(
    mut x: impl AsMut<[W]>,
    y: impl AsRef<[W]>,
    msb_mask: impl AsRef<[W]>,
    lsb_mask: impl AsRef<[W]>,
    acc: &mut Vec<W>,
    mask: &mut Vec<W>,
    register_size: usize,
) {
    let x = x.as_mut();
    let y = y.as_ref();
    let msb_mask = msb_mask.as_ref();
    let lsb_mask = lsb_mask.as_ref();

    debug_assert_eq!(x.len(), y.len());
    debug_assert_eq!(x.len(), msb_mask.len());
    debug_assert_eq!(x.len(), lsb_mask.len());

    let register_size_minus_1 = register_size - 1;
    let num_words_minus_1 = x.len() - 1;
    let shift_register_size_minus_1 = W::BITS as usize - register_size_minus_1;

    acc.clear();
    mask.clear();

    /* We work in two phases. Let H_r (msb_mask) be the mask with the
     * highest bit of each register (of size r) set, and L_r (lsb_mask)
     * be the mask with the lowest bit of each register set.
     * We describe the algorithm on a single word.
     *
     * In the first phase we perform an unsigned strict register-by-register
     * comparison of x and y, using the formula
     *
     * z = ((((y | H_r) - (x & !H_r)) | (y ^ x)) ^ (y | !x)) & H_r
     *
     * Then, we generate a register-by-register mask of all ones or
     * all zeroes, depending on the result of the comparison, using the
     * formula
     *
     * (((z >> r-1 | H_r) - L_r) | H_r) ^ z
     *
     * At that point, it is trivial to select from x and y the right values.
     */

    // We load y | H_r into the accumulator.
    acc.extend(
        y.iter()
            .zip(msb_mask)
            .map(|(&y_word, &msb_word)| y_word | msb_word),
    );

    // We load x & !H_r into mask as temporary storage.
    mask.extend(
        x.iter()
            .zip(msb_mask)
            .map(|(&x_word, &msb_word)| x_word & !msb_word),
    );

    // We subtract x & !H_r, using mask as temporary storage
    subtract(acc, mask);

    // We OR with y ^ x, XOR with (y | !x), and finally AND with H_r.
    acc.iter_mut()
        .zip(x.iter())
        .zip(y.iter())
        .zip(msb_mask.iter())
        .for_each(|(((acc_word, &x_word), &y_word), &msb_word)| {
            *acc_word = ((*acc_word | (y_word ^ x_word)) ^ (y_word | !x_word)) & msb_word
        });

    // We shift by register_size - 1 places and put the result into mask.
    {
        let (mask_last, mask_slice) = mask.split_last_mut().unwrap();
        let (&msb_last, msb_slice) = msb_mask.split_last().unwrap();
        mask_slice
            .iter_mut()
            .zip(acc[0..num_words_minus_1].iter())
            .zip(acc[1..].iter())
            .zip(msb_slice.iter())
            .rev()
            .for_each(|(((mask_word, &acc_word), &next_acc_word), &msb_word)| {
                // W is always unsigned so the shift is always with a 0
                *mask_word = (acc_word >> register_size_minus_1)
                    | (next_acc_word << shift_register_size_minus_1)
                    | msb_word
            });
        *mask_last = (acc[num_words_minus_1] >> register_size_minus_1) | msb_last;
    }

    // We subtract L_r from mask.
    subtract(mask, lsb_mask);

    // We OR with H_r and XOR with the accumulator.
    mask.iter_mut()
        .zip(msb_mask.iter())
        .zip(acc.iter())
        .for_each(|((mask_word, &msb_word), &acc_word)| {
            *mask_word = (*mask_word | msb_word) ^ acc_word
        });

    // Finally, we use mask to select the right bits from x and y and store the result.
    x.iter_mut()
        .zip(y.iter())
        .zip(mask.iter())
        .for_each(|((x_word, &y_word), &mask_word)| {
            *x_word = *x_word ^ ((*x_word ^ y_word) & mask_word);
        });
}

fn highest_power_of_2_dividing(n: NonZeroUsize) -> u32 {
    1 << n.trailing_zeros()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_highest_power_of_2_dividing() {
        let powers_of_2: Vec<_> = (1..=20)
            .map(|n| highest_power_of_2_dividing(n.try_into().unwrap()))
            .collect();
        assert_eq!(
            powers_of_2,
            vec![1, 2, 1, 4, 1, 2, 1, 8, 1, 2, 1, 4, 1, 2, 1, 16, 1, 2, 1, 4]
        );
    }

    /// The register size must be max(5, ⌈log₂ log₂ n⌉): 5 bits up to 2³²
    /// distinct elements, 6 bits beyond.
    #[cfg(target_pointer_width = "64")]
    #[test]
    fn test_register_size() {
        assert_eq!(HyperLogLog::register_size(1_000), 5);
        assert_eq!(HyperLogLog::register_size(1 << 32), 5);
        assert_eq!(HyperLogLog::register_size((1 << 32) + 1), 6);
        assert_eq!(HyperLogLog::register_size(10_000_000_000), 6);
        assert_eq!(HyperLogLog::register_size(usize::MAX), 6);
    }

    /// End-to-end coverage of the 6-bit register configuration (add, the
    /// word-straddling register accessors, the broadword merge, estimate).
    #[cfg(target_pointer_width = "64")]
    #[test]
    fn test_six_bit_registers() -> anyhow::Result<()> {
        let logic = HyperLogLogBuilder::new(10_000_000_000)
            .word_type::<u64>()
            .log2_num_regs(8)
            .build::<u64>()?;
        let mut a = vec![0u64; logic.backend_len()];
        let mut b = vec![0u64; logic.backend_len()];
        for i in 0u64..10_000 {
            logic.add(&mut a, i);
            logic.add(&mut b, 10_000 + i);
        }
        let est = logic.estimate(&a);
        assert!(
            (est - 10_000.0).abs() / 10_000.0 < 0.2,
            "estimate {est} too far from 10000"
        );
        let mut helper = logic.new_helper();
        logic.merge_with_helper(&mut a, &b, &mut helper);
        let est = logic.estimate(&a);
        assert!(
            (est - 20_000.0).abs() / 20_000.0 < 0.2,
            "merged estimate {est} too far from 20000"
        );
        Ok(())
    }

    #[test]
    #[should_panic(expected = "at most 31")]
    fn test_log2_num_regs_too_large() {
        let _ = HyperLogLogBuilder::new(1000).log2_num_regs(32);
    }

    #[test]
    #[should_panic(expected = "positive finite")]
    fn test_rsd_zero() {
        let _ = HyperLogLogBuilder::new(1000).rsd(0.0);
    }

    #[test]
    #[should_panic(expected = "positive finite")]
    fn test_rsd_nan() {
        let _ = HyperLogLogBuilder::new(1000).rsd(f64::NAN);
    }

    /// `min_log2_num_regs` must never return less than the minimum accepted
    /// by `log2_num_regs` (with `W = u8` the alignment computation alone
    /// would yield 3).
    #[test]
    fn test_min_log2_num_regs_at_least_4() {
        let builder = HyperLogLogBuilder::new(1000).word_type::<u8>();
        assert_eq!(builder.min_log2_num_regs(), 4);
    }

    /// `Debug`, `Clone`, and `PartialEq` on the logic must not put any bound
    /// on the item type `T`, which appears only in `PhantomData`.
    #[test]
    fn test_logic_traits_do_not_bound_t() -> anyhow::Result<()> {
        struct Plain;

        #[derive(Clone, Debug, PartialEq)]
        struct Bh;
        impl BuildHasher for Bh {
            type Hasher = DefaultHasher;
            fn build_hasher(&self) -> DefaultHasher {
                DefaultHasher::new()
            }
        }

        let a = HyperLogLogBuilder::new(1000)
            .word_type::<u16>()
            .build_hasher(Bh)
            .build::<Plain>()?;
        let b = a.clone();
        assert!(!format!("{a:?}").is_empty());
        assert_eq!(a, b);
        Ok(())
    }

    #[test]
    #[should_panic(expected = "does not match the expected length")]
    fn test_merge_backend_length_mismatch() {
        let logic = HyperLogLogBuilder::new(1000)
            .word_type::<u16>()
            .build::<u64>()
            .unwrap();
        let mut dst = vec![0u16; logic.backend_len()];
        let src = vec![0u16; logic.backend_len() - 1];
        let mut helper = logic.new_helper();
        logic.merge_with_helper(&mut dst, &src, &mut helper);
    }
}
