/*
 * SPDX-FileCopyrightText: 2025 Luca Cappelletti
 *
 * SPDX-License-Identifier: Apache-2.0 OR LGPL-2.1-or-later
 */

//! [`HyperLogLogVl`]: value-list-augmented HyperLogLog.
//!
//! Backend layout: one header word + a body dual-purposed as a value-list
//! buffer or a register slab. The header's top bit selects mode:
//!
//! - `0`: value list. Stores exact `u64` values via [`super::ValueListCodec`]
//!   and returns `count` as the estimate.
//! - `1`: dense HLL. Byte-identical to [`super::super::HyperLogLog`].
//!
//! Promotion (value list -> dense) is one-way. On overflow the codec
//! contents are rehashed in place into a fresh register slab.

use super::super::DefaultEstimator;
use super::super::{HyperLogLog, HyperLogLogBuilder, HyperLogLogError, HyperLogLogHelper};
use super::codec::{GapCodecMetadata, ValueListCodec};
use super::merge_strategy::{MergeOutcome, MergeStrategy, SumOfCountsFastPath};
use crate::traits::{
    EstimationLogic, MergeEstimationLogic, SliceEstimationLogic, assert_backend_len,
};
use std::borrow::Borrow;
use std::hash::BuildHasher;

const MODE_BIT: u64 = 1 << 63;
const COUNT_MASK: u64 = 0x7FFF_FFFF_0000_0000;
const COUNT_SHIFT: u32 = 32;
const WRITER_TELL_MASK: u64 = 0x0000_0000_FFFF_FFFF;

#[inline]
fn is_dense(header: u64) -> bool {
    header & MODE_BIT != 0
}

#[inline]
fn read_metadata(header: u64) -> GapCodecMetadata {
    GapCodecMetadata {
        count: ((header & COUNT_MASK) >> COUNT_SHIFT) as u32,
        writer_tell: (header & WRITER_TELL_MASK) as u32,
    }
}

#[inline]
fn write_value_list_header(count: u32, writer_tell: u32) -> u64 {
    (u64::from(count) << COUNT_SHIFT) | u64::from(writer_tell)
}

#[inline]
fn dense_header() -> u64 {
    MODE_BIT
}

/// Value-list-augmented [`HyperLogLog`]. See the module doc.
///
/// ```
/// # use card_est_array::impls::{HyperLogLogVlBuilder, GammaCode, SumOfCountsFastPath};
/// let logic = HyperLogLogVlBuilder::<_, GammaCode, SumOfCountsFastPath, true>::new(1_000_000)
///     .log2_num_regs(10)
///     .build::<u64>()
///     .unwrap();
/// ```
#[derive(Debug)]
pub struct HyperLogLogVl<T, H, C, M = SumOfCountsFastPath, const BETA: bool = true> {
    dense_logic: HyperLogLog<T, H, u64, BETA>,
    data_capacity_bits: usize,
    /// Cheap ceiling on how many values `C` can pack into `data_capacity_bits`.
    /// See [`ValueListCodec::max_entries`]. Reads on every [`add`]
    /// (`EstimationLogic::add`) so we compute it once at build time.
    max_entries: usize,
    _marker: std::marker::PhantomData<(C, M)>,
}

// Manual `Clone`: `C`/`M` are `PhantomData` only, no `Clone` bound needed.
impl<T, H: Clone, C, M, const BETA: bool> Clone for HyperLogLogVl<T, H, C, M, BETA> {
    fn clone(&self) -> Self {
        Self {
            dense_logic: self.dense_logic.clone(),
            data_capacity_bits: self.data_capacity_bits,
            max_entries: self.max_entries,
            _marker: std::marker::PhantomData,
        }
    }
}

// Manual `PartialEq`: same reason as `Clone` above.
impl<T: PartialEq, H: PartialEq, C, M, const BETA: bool> PartialEq
    for HyperLogLogVl<T, H, C, M, BETA>
{
    fn eq(&self, other: &Self) -> bool {
        self.dense_logic == other.dense_logic
            && self.data_capacity_bits == other.data_capacity_bits
            && self.max_entries == other.max_entries
    }
}

impl<T, H, C, M, const BETA: bool> core::fmt::Display for HyperLogLogVl<T, H, C, M, BETA>
where
    T: LosslessU64,
    H: BuildHasher + Clone,
    C: ValueListCodec,
    M: MergeStrategy,
{
    /// Delegates to the wrapped dense estimator's [`Display`], prepending a
    /// short marker so log lines identify the value-list front end.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "value-list-augmented ")?;
        core::fmt::Display::fmt(&self.dense_logic, f)
    }
}

impl<T, H, C, M, const BETA: bool> HyperLogLogVl<T, H, C, M, BETA>
where
    T: LosslessU64,
    H: BuildHasher + Clone,
    C: ValueListCodec,
    M: MergeStrategy,
{
    /// Wraps a pre-built [`HyperLogLog`] with a value-list front end.
    pub fn new(dense_logic: HyperLogLog<T, H, u64, BETA>) -> Self {
        let data_words = dense_logic.backend_len();
        let data_capacity_bits = data_words * 64;
        let max_entries = C::max_entries(data_capacity_bits);
        Self {
            dense_logic,
            data_capacity_bits,
            max_entries,
            _marker: std::marker::PhantomData,
        }
    }

    /// Borrow the wrapped dense logic.
    pub fn dense_logic(&self) -> &HyperLogLog<T, H, u64, BETA> {
        &self.dense_logic
    }

    /// Number of `u64` words in each counter's data body.
    #[inline]
    pub fn data_words(&self) -> usize {
        self.dense_logic.backend_len()
    }

    /// Rehash every stored value into a fresh register slab in place.
    fn promote_to_dense(&self, header: &mut u64, data: &mut [u64]) {
        let metadata = read_metadata(*header);
        let stored: Vec<u64> = C::iter(metadata, data).collect();
        for word in data.iter_mut() {
            *word = 0;
        }
        *header = dense_header();
        for value in stored {
            self.dense_logic.add(data, T::from_u64(value));
        }
    }
}

/// Element types that round-trip through `u64` losslessly. Every implementor
/// MUST satisfy `Self::from_u64(x.to_u64()) == x`.
pub trait LosslessU64: Copy + core::hash::Hash + 'static {
    /// Value the codec sees.
    fn to_u64(self) -> u64;
    /// Roundtrip of [`to_u64`](Self::to_u64).
    fn from_u64(v: u64) -> Self;
}

macro_rules! impl_lossless_u64 {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl LosslessU64 for $ty {
                #[inline(always)] fn to_u64(self) -> u64 { self as u64 }
                #[inline(always)] fn from_u64(v: u64) -> Self { v as $ty }
            }
        )+
    };
}

impl_lossless_u64!(u8, u16, u32, u64, usize);

impl<T, H, C, M, const BETA: bool> SliceEstimationLogic<u64> for HyperLogLogVl<T, H, C, M, BETA>
where
    T: LosslessU64,
    H: BuildHasher + Clone,
    C: ValueListCodec,
    M: MergeStrategy,
{
    #[inline(always)]
    fn backend_len(&self) -> usize {
        // 1 header word + the underlying dense backend length.
        self.dense_logic.backend_len() + 1
    }
}

impl<T, H, C, M, const BETA: bool> EstimationLogic for HyperLogLogVl<T, H, C, M, BETA>
where
    T: LosslessU64,
    H: BuildHasher + Clone,
    C: ValueListCodec,
    M: MergeStrategy,
{
    type Item = T;
    type Backend = [u64];
    type Estimator<'a>
        = DefaultEstimator<Self, &'a Self, Box<[u64]>>
    where
        T: 'a,
        H: 'a,
        C: 'a,
        M: 'a;

    fn new_estimator(&self) -> Self::Estimator<'_> {
        let len = SliceEstimationLogic::<u64>::backend_len(self);
        Self::Estimator::new(self, vec![0u64; len].into_boxed_slice())
    }

    fn add(&self, backend: &mut Self::Backend, element: impl Borrow<T>) {
        assert_backend_len!(self, backend);
        let value: u64 = element.borrow().to_u64();
        let (header, data) = backend
            .split_first_mut()
            .expect("backend must be non-empty");
        if is_dense(*header) {
            self.dense_logic.add(data, element);
            return;
        }
        let mut metadata = read_metadata(*header);
        // Fast path: at capacity, insert cannot fit. Skip its O(N) decode
        // and promote.
        if (metadata.count as usize) >= self.max_entries {
            self.promote_to_dense(header, data);
            self.dense_logic.add(data, element);
            return;
        }
        match C::insert(&mut metadata, data, self.data_capacity_bits, value) {
            Ok(()) => {
                *header = write_value_list_header(metadata.count, metadata.writer_tell);
            }
            Err(()) => {
                // Body full: promote and fold the new element into dense.
                self.promote_to_dense(header, data);
                self.dense_logic.add(data, element);
            }
        }
    }

    fn estimate(&self, backend: &Self::Backend) -> f64 {
        assert_backend_len!(self, backend);
        let (header, data) = backend.split_first().expect("backend must be non-empty");
        if is_dense(*header) {
            return self.dense_logic.estimate(data);
        }
        let metadata = read_metadata(*header);
        f64::from(metadata.count)
    }

    #[inline(always)]
    fn clear(&self, backend: &mut Self::Backend) {
        for word in backend.iter_mut() {
            *word = 0;
        }
    }

    #[inline(always)]
    fn set(&self, dst: &mut Self::Backend, src: &Self::Backend) {
        debug_assert_eq!(dst.len(), src.len());
        dst.copy_from_slice(src);
    }
}

/// Helper for [`HyperLogLogVl::merge_with_helper`]. Holds a
/// [`HyperLogLogHelper`] plus scratch buffers for the encoding attempt and
/// the [`EncodeAndDenseFold`](super::EncodeAndDenseFold) fallback.
pub struct HyperLogLogVlHelper {
    dense: HyperLogLogHelper<u64>,
    scratch: Vec<u64>,
    dense_scratch: Vec<u64>,
}

impl<T, H, C, M, const BETA: bool> MergeEstimationLogic for HyperLogLogVl<T, H, C, M, BETA>
where
    T: LosslessU64,
    H: BuildHasher + Clone,
    C: ValueListCodec + super::gap_codec::GapEncoder,
    M: MergeStrategy,
{
    type Helper = HyperLogLogVlHelper;

    fn new_helper(&self) -> Self::Helper {
        HyperLogLogVlHelper {
            dense: self.dense_logic.new_helper(),
            scratch: vec![0u64; self.data_words()],
            dense_scratch: vec![0u64; self.data_words()],
        }
    }

    fn merge_with_helper(
        &self,
        dst: &mut Self::Backend,
        src: &Self::Backend,
        helper: &mut Self::Helper,
    ) {
        assert_backend_len!(self, dst);
        assert_backend_len!(self, src);
        let (src_header_word, src_data) = src.split_first().expect("src backend non-empty");
        let src_header = *src_header_word;
        let src_is_dense = is_dense(src_header);
        let src_meta = read_metadata(src_header);

        let (dst_header, dst_data) = dst.split_first_mut().expect("dst backend non-empty");
        let dst_is_dense = is_dense(*dst_header);

        match (dst_is_dense, src_is_dense) {
            (true, true) => {
                self.dense_logic
                    .merge_with_helper(dst_data, src_data, &mut helper.dense);
            }
            (true, false) => {
                for value in C::iter(src_meta, src_data) {
                    self.dense_logic.add(dst_data, T::from_u64(value));
                }
            }
            (false, true) => {
                self.promote_to_dense(dst_header, dst_data);
                self.dense_logic
                    .merge_with_helper(dst_data, src_data, &mut helper.dense);
            }
            (false, false) => {
                let dst_meta = read_metadata(*dst_header);
                // Zero the dense fallback in case the strategy uses it.
                for word in helper.dense_scratch.iter_mut() {
                    *word = 0;
                }
                // Split helper's scratch pair so the strategy can encode into
                // `scratch` and dense-fold into `dense_scratch` in one pass.
                let scratch = &mut helper.scratch;
                let dense_scratch = &mut helper.dense_scratch;
                let dense_logic = &self.dense_logic;
                let outcome = M::merge_value_lists::<C, _>(
                    dst_meta,
                    dst_data,
                    src_meta,
                    src_data,
                    scratch,
                    self.data_capacity_bits,
                    self.max_entries,
                    |value| dense_logic.add(&mut *dense_scratch, T::from_u64(value)),
                );
                match outcome {
                    MergeOutcome::ValueList(scratch_meta) => {
                        dst_data.copy_from_slice(scratch);
                        *dst_header =
                            write_value_list_header(scratch_meta.count, scratch_meta.writer_tell);
                    }
                    MergeOutcome::Dense => {
                        dst_data.copy_from_slice(dense_scratch);
                        *dst_header = dense_header();
                    }
                    MergeOutcome::Promote => {
                        self.promote_to_dense(dst_header, dst_data);
                        for value in C::iter(src_meta, src_data) {
                            self.dense_logic.add(dst_data, T::from_u64(value));
                        }
                    }
                }
            }
        }
    }
}

/// Builder for a [`HyperLogLogVl`] logic.
///
/// ```
/// # use card_est_array::impls::{EncodeAndDenseFold, HyperLogLogVlBuilder, PiCode};
/// let logic = HyperLogLogVlBuilder::<_, PiCode<2>, EncodeAndDenseFold, true>::new(1_000_000)
///     .log2_num_regs(10)
///     .build::<u64>()
///     .unwrap();
/// ```
#[derive(Debug)]
pub struct HyperLogLogVlBuilder<H, C, M = SumOfCountsFastPath, const BETA: bool = true> {
    inner: HyperLogLogBuilder<H, u64, BETA>,
    _marker: std::marker::PhantomData<(C, M)>,
}

// Manual `Clone`, same reason as [`HyperLogLogVl`] above.
impl<H: Clone, C, M, const BETA: bool> Clone for HyperLogLogVlBuilder<H, C, M, BETA> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _marker: std::marker::PhantomData,
        }
    }
}

impl<C, M> HyperLogLogVlBuilder<std::hash::BuildHasherDefault<std::hash::DefaultHasher>, C, M, true>
where
    C: ValueListCodec,
    M: MergeStrategy,
{
    /// Fresh builder with defaults (hasher = default, BETA = true, u64 backend).
    pub fn new(num_elements: usize) -> Self {
        Self {
            inner: HyperLogLogBuilder::new(num_elements).word_type::<u64>(),
            _marker: std::marker::PhantomData,
        }
    }
}

impl<H, C, M, const BETA: bool> HyperLogLogVlBuilder<H, C, M, BETA>
where
    H: BuildHasher + Clone,
    C: ValueListCodec,
    M: MergeStrategy,
{
    /// Sets `log2_num_regs` on the wrapped dense builder.
    pub fn log2_num_regs(mut self, log2_num_regs: u32) -> Self {
        self.inner = self.inner.log2_num_regs(log2_num_regs);
        self
    }

    /// Replaces the hasher, switching the resulting logic's `H` type.
    /// Mirrors [`HyperLogLogBuilder::build_hasher`] so downstream callers can
    /// pin their study-wide hasher (e.g. `BuildHasherDefault<WyHash>`).
    pub fn build_hasher<H2>(self, build_hasher: H2) -> HyperLogLogVlBuilder<H2, C, M, BETA>
    where
        H2: BuildHasher + Clone,
    {
        HyperLogLogVlBuilder {
            inner: self.inner.build_hasher(build_hasher),
            _marker: std::marker::PhantomData,
        }
    }

    pub fn build<T>(self) -> Result<HyperLogLogVl<T, H, C, M, BETA>, HyperLogLogError>
    where
        T: LosslessU64,
    {
        let dense = self.inner.build::<T>()?;
        Ok(HyperLogLogVl::new(dense))
    }
}

#[cfg(test)]
mod tests {
    use super::super::GammaCode;
    use super::*;
    use crate::traits::{EstimationLogic, MergeEstimationLogic};

    fn build_logic() -> HyperLogLogVl<
        u64,
        std::hash::BuildHasherDefault<std::hash::DefaultHasher>,
        GammaCode,
        SumOfCountsFastPath,
        true,
    > {
        HyperLogLogVlBuilder::<_, GammaCode, SumOfCountsFastPath, true>::new(10_000)
            .log2_num_regs(10)
            .build::<u64>()
            .unwrap()
    }

    #[test]
    fn fresh_counter_is_value_list_empty() {
        let logic = build_logic();
        let backend: Vec<u64> = vec![0u64; SliceEstimationLogic::<u64>::backend_len(&logic)];
        assert_eq!(logic.estimate(&backend), 0.0);
    }

    #[test]
    fn adds_stay_exact_while_value_list_fits() {
        let logic = build_logic();
        let mut backend = vec![0u64; SliceEstimationLogic::<u64>::backend_len(&logic)];
        for i in 0u64..10 {
            logic.add(&mut backend, i);
        }
        assert_eq!(logic.estimate(&backend), 10.0);
    }

    #[test]
    fn promotion_to_dense_after_value_list_overflow() {
        let logic = build_logic();
        let mut backend = vec![0u64; SliceEstimationLogic::<u64>::backend_len(&logic)];
        // At P = 10, B = 5, data_capacity_bits = 1024 * 5 = 5120. gamma-coded
        // gaps of 1 pack to 1 bit each, plus a small head, so we can hold
        // thousands of consecutive small integers before overflowing. Insert
        // 10000 sequential values to force promotion.
        for i in 0u64..10_000 {
            logic.add(&mut backend, i);
        }
        // Header top bit set: dense mode.
        assert!(is_dense(backend[0]), "counter must have promoted to dense");
        let estimate = logic.estimate(&backend);
        assert!(
            estimate > 8_000.0 && estimate < 12_000.0,
            "dense estimate {estimate} off"
        );
    }

    #[test]
    fn dense_merge_matches_dense_only() {
        // Two counters populated with dense-triggering streams. After the merge
        // the estimate should be close to the union size (2N).
        let logic = build_logic();
        let mut a = vec![0u64; SliceEstimationLogic::<u64>::backend_len(&logic)];
        let mut b = vec![0u64; SliceEstimationLogic::<u64>::backend_len(&logic)];
        for i in 0u64..10_000 {
            logic.add(&mut a, i);
            logic.add(&mut b, 10_000 + i);
        }
        let mut helper = logic.new_helper();
        logic.merge_with_helper(&mut a, &b, &mut helper);
        let est = logic.estimate(&a);
        assert!(
            est > 16_000.0 && est < 24_000.0,
            "dense merge estimate {est} off"
        );
    }

    #[test]
    fn value_list_merge_stays_value_list_when_it_fits() {
        let logic = build_logic();
        let mut a = vec![0u64; SliceEstimationLogic::<u64>::backend_len(&logic)];
        let mut b = vec![0u64; SliceEstimationLogic::<u64>::backend_len(&logic)];
        for i in 0u64..50 {
            logic.add(&mut a, i);
            logic.add(&mut b, 100 + i);
        }
        let mut helper = logic.new_helper();
        logic.merge_with_helper(&mut a, &b, &mut helper);
        assert!(!is_dense(a[0]), "small union must stay a value list");
        assert_eq!(logic.estimate(&a), 100.0);
    }

    #[test]
    fn value_list_merge_promotes_on_overflow() {
        let logic = build_logic();
        let mut a = vec![0u64; SliceEstimationLogic::<u64>::backend_len(&logic)];
        let mut b = vec![0u64; SliceEstimationLogic::<u64>::backend_len(&logic)];
        // Each operand holds a large disjoint range. Individually they fit as
        // value lists; combined they overflow.
        for i in 0u64..2_000 {
            logic.add(&mut a, i);
            logic.add(&mut b, 10_000_000 + i);
        }
        // If individually they're still value lists, the merge overflow path
        // is exercised. If either promoted, the (val, dense) path is exercised.
        let mut helper = logic.new_helper();
        logic.merge_with_helper(&mut a, &b, &mut helper);
        let est = logic.estimate(&a);
        assert!(
            est > 3_000.0,
            "post-merge estimate {est} must reflect ~4000 distinct elements",
        );
    }

    /// Load-bearing invariant: after promotion, the dense body must be
    /// byte-identical to the state you'd get by feeding the same elements
    /// straight into a fresh `HyperLogLog` with matching (P, B, hasher). If
    /// this test drifts, the A/B experiment stops isolating "value list vs
    /// no value list" and starts measuring a state divergence bug instead.
    #[test]
    fn promotion_produces_byte_identical_dense_state() {
        use crate::impls::HyperLogLogBuilder;
        let vl = build_logic();
        // Force promotion via the same sequential-integer stream as above.
        let mut vl_backend = vec![0u64; SliceEstimationLogic::<u64>::backend_len(&vl)];
        for i in 0u64..10_000 {
            vl.add(&mut vl_backend, i);
        }
        assert!(is_dense(vl_backend[0]), "must have promoted");

        // Direct-to-dense oracle, same P, matches wrapped dense_logic bit-for-bit.
        let oracle = HyperLogLogBuilder::new(10_000)
            .word_type::<u64>()
            .log2_num_regs(10)
            .build::<u64>()
            .unwrap();
        let mut oracle_backend = vec![0u64; oracle.backend_len()];
        for i in 0u64..10_000 {
            oracle.add(&mut oracle_backend, i);
        }

        // Skip the header word on the VL side, compare the body to the oracle.
        let vl_body = &vl_backend[1..];
        assert_eq!(
            vl_body,
            oracle_backend.as_slice(),
            "post-promotion body must equal a direct-to-dense HLL",
        );
        // Estimates must therefore match exactly too.
        assert_eq!(vl.estimate(&vl_backend), oracle.estimate(&oracle_backend));
    }

    /// Same as above but for `T = usize`, the concrete Item type HyperBall
    /// binds on. Guards `LosslessU64::{to_u64, from_u64}` roundtrip on the
    /// bit pattern the codec sees.
    #[test]
    fn promotion_byte_identical_for_usize_items() {
        use crate::impls::HyperLogLogBuilder;
        let vl: HyperLogLogVl<
            usize,
            std::hash::BuildHasherDefault<std::hash::DefaultHasher>,
            GammaCode,
            SumOfCountsFastPath,
            true,
        > = HyperLogLogVlBuilder::<_, GammaCode, SumOfCountsFastPath, true>::new(10_000)
            .log2_num_regs(10)
            .build::<usize>()
            .unwrap();
        let mut vl_backend = vec![0u64; SliceEstimationLogic::<u64>::backend_len(&vl)];
        for i in 0usize..10_000 {
            vl.add(&mut vl_backend, i);
        }
        assert!(is_dense(vl_backend[0]));

        let oracle = HyperLogLogBuilder::new(10_000)
            .word_type::<u64>()
            .log2_num_regs(10)
            .build::<usize>()
            .unwrap();
        let mut oracle_backend = vec![0u64; oracle.backend_len()];
        for i in 0usize..10_000 {
            oracle.add(&mut oracle_backend, i);
        }
        assert_eq!(&vl_backend[1..], oracle_backend.as_slice());
    }

    /// Trait-surface gate for HyperBall consumption. `SliceEstimatorArray`'s
    /// bounds are `L: SliceEstimationLogic<W> + Clone + Sync`; the
    /// `SpillStore` and `SyncSliceEstimatorArray` paths add further `Sync`
    /// requirements on the word type. This test constructs the exact
    /// instantiation HyperBall will build in the sibling `hyperball-counters`
    /// crate and drives one add + one merge through it, so any missing bound
    /// on `HyperLogLogVl` breaks the build here rather than at the consumer.
    #[test]
    fn integrates_with_slice_estimator_array_for_hyperball() {
        use crate::impls::SliceEstimatorArray;
        use crate::traits::{
            Estimator, EstimatorArray, EstimatorArrayMut, EstimatorMut, MergeEstimator,
        };

        type Vl = HyperLogLogVl<
            usize,
            std::hash::BuildHasherDefault<std::hash::DefaultHasher>,
            GammaCode,
            SumOfCountsFastPath,
            true,
        >;
        let logic: Vl =
            HyperLogLogVlBuilder::<_, GammaCode, SumOfCountsFastPath, true>::new(10_000)
                .log2_num_regs(10)
                .build::<usize>()
                .unwrap();

        // Two-slot array (mirrors HyperBall's array_0 / array_1 double-buffer).
        let mut array: SliceEstimatorArray<Vl, u64, Box<[u64]>> =
            SliceEstimatorArray::new(logic, 2);

        {
            let mut a = array.get_estimator_mut(0);
            for i in 0usize..40 {
                a.add(i);
            }
        }
        {
            let mut b = array.get_estimator_mut(1);
            for i in 100usize..140 {
                b.add(i);
            }
        }

        // Union b <- b | a via the trait-object merge path HyperBall uses.
        let src_backend: Vec<u64> = array.get_backend(0).to_vec();
        {
            let mut dst = array.get_estimator_mut(1);
            dst.merge(src_backend.as_slice());
        }

        let est = array.get_estimator(1).estimate();
        assert_eq!(est, 80.0, "value-list union of two disjoint 40-item sets");
    }

    /// The `add` fast-path (skip codec insert once `count >= max_entries`) must
    /// still produce a byte-identical dense state to a direct-to-dense
    /// insertion of the same stream. If this diverges, the perf shortcut
    /// silently changed the sketch and the paper's A/B is meaningless.
    #[test]
    fn ceiling_fast_path_preserves_byte_parity() {
        use crate::impls::HyperLogLogBuilder;

        let vl = build_logic();
        // Insert enough consecutive integers to push the gamma-coded value list
        // well past the ceiling and force the fast-path arm to fire many
        // times over the promotion boundary.
        let mut vl_backend = vec![0u64; SliceEstimationLogic::<u64>::backend_len(&vl)];
        for i in 0u64..vl.max_entries as u64 + 5_000 {
            vl.add(&mut vl_backend, i);
        }
        assert!(is_dense(vl_backend[0]));

        let oracle = HyperLogLogBuilder::new(10_000)
            .word_type::<u64>()
            .log2_num_regs(10)
            .build::<u64>()
            .unwrap();
        let mut oracle_backend = vec![0u64; oracle.backend_len()];
        for i in 0u64..vl.max_entries as u64 + 5_000 {
            oracle.add(&mut oracle_backend, i);
        }

        assert_eq!(
            &vl_backend[1..],
            oracle_backend.as_slice(),
            "fast-path promotion must produce a byte-identical dense body",
        );
    }

    /// `C::merge_into` (streaming two-pointer) must return the same value
    /// set as inserting each src value into dst one at a time (the semantic
    /// baseline). Uses medium-sized value lists so both operands stay in
    /// value-list mode and the streaming path is exercised.
    #[test]
    fn streaming_merge_matches_iterative_insert() {
        let logic = build_logic();
        let mut a = vec![0u64; SliceEstimationLogic::<u64>::backend_len(&logic)];
        let mut b = vec![0u64; SliceEstimationLogic::<u64>::backend_len(&logic)];
        // Both operands populated well inside the gamma ceiling (5120 for
        // P=10 B=5), with overlapping ranges to exercise the tie-breaking
        // arm of the two-pointer union.
        for i in 0u64..300 {
            logic.add(&mut a, i);
        }
        for i in 200u64..500 {
            logic.add(&mut b, i);
        }
        let mut helper = logic.new_helper();
        logic.merge_with_helper(&mut a, &b, &mut helper);
        assert!(!is_dense(a[0]), "small union must stay a value list");
        // Union of [0, 300) and [200, 500) has 500 distinct values.
        assert_eq!(logic.estimate(&a), 500.0);
    }

    /// A scale test: many-merge burst on a single dst counter. Runs a batch of
    /// unions that individually stay under the codec ceiling but collectively
    /// exercise the streaming merge on medium-sized operands. If this test
    /// times out, we've regressed to quadratic.
    ///
    /// Bit budget is the actual gate here, not the codec's max-entries
    /// ceiling. Each 30-entry batch here transitions with a ~11-bit big-gap
    /// gamma plus 29 gap-1 encodings (~1 bit each), so per-batch cost is
    /// roughly 40 bits. 30 batches keeps us safely inside the 5120-bit body.
    #[test]
    fn streaming_merge_stays_linear_on_many_calls() {
        let logic = build_logic();
        let mut dst = vec![0u64; SliceEstimationLogic::<u64>::backend_len(&logic)];
        // Prime dst with a modest run.
        for i in 0u64..200 {
            logic.add(&mut dst, i);
        }
        let mut helper = logic.new_helper();
        for batch in 0u64..30 {
            let mut src = vec![0u64; SliceEstimationLogic::<u64>::backend_len(&logic)];
            let start = 1_000_000 + batch * 100;
            for i in 0u64..30 {
                logic.add(&mut src, start + i);
            }
            logic.merge_with_helper(&mut dst, &src, &mut helper);
        }
        // Primed 200 + 30 batches * 30 disjoint = 1100 distinct entries.
        assert!(!is_dense(dst[0]), "must not have promoted");
        assert_eq!(logic.estimate(&dst), 1100.0);
    }

    /// The `M` type parameter selects the (val, val) merge strategy at compile time
    /// with no runtime dispatch cost. Instantiating the estimator under both shipped
    /// markers (`SumOfCountsFastPath` and `EncodeAndDenseFold`) proves the strategy is
    /// a genuine compile-time knob and that both produce correct estimates on a
    /// clean-fit union.
    #[test]
    fn merge_strategy_is_a_compile_time_parameter() {
        use super::super::EncodeAndDenseFold;

        fn build_logic_with<M: MergeStrategy + Clone>() -> HyperLogLogVl<
            u64,
            std::hash::BuildHasherDefault<std::hash::DefaultHasher>,
            GammaCode,
            M,
            true,
        > {
            HyperLogLogVlBuilder::<_, GammaCode, M, true>::new(10_000)
                .log2_num_regs(10)
                .build::<u64>()
                .unwrap()
        }

        for label in ["fastpath", "encode_dense"] {
            let estimate = if label == "fastpath" {
                let logic = build_logic_with::<SumOfCountsFastPath>();
                let bl = SliceEstimationLogic::<u64>::backend_len(&logic);
                let mut a = vec![0u64; bl];
                let mut b = vec![0u64; bl];
                for i in 0u64..50 {
                    logic.add(&mut a, i);
                    logic.add(&mut b, 100 + i);
                }
                let mut helper = logic.new_helper();
                logic.merge_with_helper(&mut a, &b, &mut helper);
                logic.estimate(&a)
            } else {
                let logic = build_logic_with::<EncodeAndDenseFold>();
                let bl = SliceEstimationLogic::<u64>::backend_len(&logic);
                let mut a = vec![0u64; bl];
                let mut b = vec![0u64; bl];
                for i in 0u64..50 {
                    logic.add(&mut a, i);
                    logic.add(&mut b, 100 + i);
                }
                let mut helper = logic.new_helper();
                logic.merge_with_helper(&mut a, &b, &mut helper);
                logic.estimate(&a)
            };
            assert_eq!(
                estimate, 100.0,
                "strategy {label}: union of two 50-item sets"
            );
        }
    }

    /// `EncodeAndDenseFold` must (a) salvage the value list when substantial
    /// operand overlap keeps the actual union under the codec ceiling and (b)
    /// install a dense state directly on genuine overflow.
    #[test]
    fn encode_and_dense_fold_covers_both_outcomes() {
        use super::super::EncodeAndDenseFold;

        fn build<M: MergeStrategy + Clone>() -> HyperLogLogVl<
            u64,
            std::hash::BuildHasherDefault<std::hash::DefaultHasher>,
            GammaCode,
            M,
            true,
        > {
            HyperLogLogVlBuilder::<_, GammaCode, M, true>::new(10_000)
                .log2_num_regs(10)
                .build::<u64>()
                .unwrap()
        }

        // (a) High-overlap union that fits under ceiling: stays value-list.
        {
            let logic = build::<EncodeAndDenseFold>();
            let bl = SliceEstimationLogic::<u64>::backend_len(&logic);
            let mut a = vec![0u64; bl];
            let mut b = vec![0u64; bl];
            let overlap: Vec<u64> = (0u64..2500).collect();
            let a_extra: Vec<u64> = (2500u64..3000).collect();
            let b_extra: Vec<u64> = (3000u64..3500).collect();
            for &v in overlap.iter().chain(a_extra.iter()) {
                logic.add(&mut a, v);
            }
            for &v in overlap.iter().chain(b_extra.iter()) {
                logic.add(&mut b, v);
            }
            let mut helper = logic.new_helper();
            logic.merge_with_helper(&mut a, &b, &mut helper);
            assert!(
                !is_dense(a[0]),
                "EncodeAndDenseFold must preserve value list on high-overlap union",
            );
            assert_eq!(logic.estimate(&a), 3500.0);
        }

        // (b) Genuine overflow: no overlap, sum exceeds ceiling. Strategy must
        // install the dense state directly (mode bit set, estimate reasonable).
        {
            let logic = build::<EncodeAndDenseFold>();
            let bl = SliceEstimationLogic::<u64>::backend_len(&logic);
            let mut a = vec![0u64; bl];
            let mut b = vec![0u64; bl];
            for i in 0u64..3000 {
                logic.add(&mut a, i);
            }
            for i in 100_000u64..103_000 {
                logic.add(&mut b, i);
            }
            let mut helper = logic.new_helper();
            logic.merge_with_helper(&mut a, &b, &mut helper);
            assert!(
                is_dense(a[0]),
                "EncodeAndDenseFold must promote when the disjoint union overflows",
            );
            let est = logic.estimate(&a);
            assert!(
                est > 5_000.0 && est < 7_000.0,
                "disjoint 6000-element union estimate should be near 6000, got {est}",
            );
        }
    }

    /// The (val, val) arm zeros `helper.dense_scratch` before every call so a
    /// prior merge cannot contaminate the current one. If we ever regress that,
    /// consecutive `Dense` outcomes on the same helper would return register
    /// slabs computed against stale registers (HLL `add` takes max with the
    /// current register value, so leftover state biases the estimate upward).
    ///
    /// This test exercises the invariant end-to-end: two independent
    /// (val, val) merges through the same helper, both producing Dense
    /// outcomes, and the second's dense state must be identical to a
    /// direct-to-dense insertion of only its own operand union — i.e. no bits
    /// leak from the first merge.
    #[test]
    fn encode_and_dense_fold_helper_reuse_is_contamination_free() {
        use super::super::EncodeAndDenseFold;
        use crate::impls::HyperLogLogBuilder;

        let vl: HyperLogLogVl<
            u64,
            std::hash::BuildHasherDefault<std::hash::DefaultHasher>,
            GammaCode,
            EncodeAndDenseFold,
            true,
        > = HyperLogLogVlBuilder::<_, GammaCode, EncodeAndDenseFold, true>::new(10_000)
            .log2_num_regs(10)
            .build::<u64>()
            .unwrap();
        let bl = SliceEstimationLogic::<u64>::backend_len(&vl);

        let mut helper = vl.new_helper();

        // First merge: two disjoint 3k-element runs starting from very
        // different bases so the union overflows the codec and the strategy
        // returns Dense. This will populate helper.dense_scratch.
        let first_result = {
            let mut a = vec![0u64; bl];
            let mut b = vec![0u64; bl];
            for i in 0u64..3_000 {
                vl.add(&mut a, i);
            }
            for i in 10_000_000u64..10_003_000 {
                vl.add(&mut b, i);
            }
            vl.merge_with_helper(&mut a, &b, &mut helper);
            assert!(is_dense(a[0]), "first merge must go Dense");
            a
        };
        let _ = first_result;

        // Second merge with the SAME helper on a different operand pair.
        // Must produce Dense state identical to a direct-to-dense insertion
        // of the second pair's union alone.
        let mut c = vec![0u64; bl];
        let mut d = vec![0u64; bl];
        for i in 0u64..3_000 {
            vl.add(&mut c, 500_000 + i);
        }
        for i in 0u64..3_000 {
            vl.add(&mut d, 700_000 + i);
        }
        vl.merge_with_helper(&mut c, &d, &mut helper);
        assert!(is_dense(c[0]), "second merge must go Dense");

        // Oracle: direct-to-dense over the SAME second-pair values only.
        let oracle = HyperLogLogBuilder::new(10_000)
            .word_type::<u64>()
            .log2_num_regs(10)
            .build::<u64>()
            .unwrap();
        let mut oracle_backend = vec![0u64; oracle.backend_len()];
        for i in 0u64..3_000 {
            oracle.add(&mut oracle_backend, 500_000 + i);
        }
        for i in 0u64..3_000 {
            oracle.add(&mut oracle_backend, 700_000 + i);
        }

        assert_eq!(
            &c[1..],
            oracle_backend.as_slice(),
            "second Dense merge must not carry state from the first",
        );
    }
}
