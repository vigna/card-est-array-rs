/*
 * SPDX-FileCopyrightText: 2025 Luca Cappelletti
 *
 * SPDX-License-Identifier: Apache-2.0 OR LGPL-2.1-or-later
 */

//! Merge-strategy markers for the (value list, value list) arm of
//! [`super::HyperLogLogVl::merge_with_helper`].
//!
//! - [`SumOfCountsFastPath`] (default): promote whenever `dst.count + src.count`
//!   exceeds the codec ceiling.
//! - [`EncodeAndDenseFold`]: single decode pass that encodes and dense-folds
//!   in parallel. Keeps more value lists at ~2-3x wall clock.

use super::codec::{GapCodecMetadata, ValueListCodec};
use super::gap_codec::{GapCodecEncoder, GapEncoder, streaming_union};

/// Result of a (value list, value list) merge.
#[derive(Debug, Clone, Copy)]
pub enum MergeOutcome {
    /// Scratch holds the encoded union.
    ValueList(GapCodecMetadata),
    /// The caller's dense fold buffer holds the complete dense state.
    Dense,
    /// Caller runs its own promote path.
    Promote,
}

/// Compile-time selector for the (value list, value list) merge arm.
///
/// ```
/// # use card_est_array::impls::{
/// #     EncodeAndDenseFold, HyperLogLogVlBuilder, GammaCode, SumOfCountsFastPath,
/// # };
/// let a = HyperLogLogVlBuilder::<_, GammaCode, SumOfCountsFastPath, true>::new(1_000_000)
///     .log2_num_regs(10)
///     .build::<u64>()
///     .unwrap();
/// let b = HyperLogLogVlBuilder::<_, GammaCode, EncodeAndDenseFold, true>::new(1_000_000)
///     .log2_num_regs(10)
///     .build::<u64>()
///     .unwrap();
/// ```
pub trait MergeStrategy {
    /// Perform the (value list, value list) merge. See [`MergeOutcome`] for
    /// the caller contract each variant implies. `dense_fold` is called only
    /// by strategies that need it.
    #[allow(clippy::too_many_arguments)]
    fn merge_value_lists<C, F>(
        dst_meta: GapCodecMetadata,
        dst_data: &[u64],
        src_meta: GapCodecMetadata,
        src_data: &[u64],
        scratch: &mut [u64],
        buf_capacity_bits: usize,
        max_entries: usize,
        dense_fold: F,
    ) -> MergeOutcome
    where
        C: ValueListCodec + GapEncoder,
        F: FnMut(u64);
}

/// Promote whenever `dst.count + src.count` exceeds the codec ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SumOfCountsFastPath;

impl MergeStrategy for SumOfCountsFastPath {
    #[inline]
    fn merge_value_lists<C, F>(
        dst_meta: GapCodecMetadata,
        dst_data: &[u64],
        src_meta: GapCodecMetadata,
        src_data: &[u64],
        scratch: &mut [u64],
        buf_capacity_bits: usize,
        max_entries: usize,
        _dense_fold: F,
    ) -> MergeOutcome
    where
        C: ValueListCodec + GapEncoder,
        F: FnMut(u64),
    {
        let combined = (dst_meta.count as usize) + (src_meta.count as usize);
        if combined > max_entries {
            return MergeOutcome::Promote;
        }
        let mut scratch_meta = GapCodecMetadata::default();
        match C::merge_into(
            &mut scratch_meta,
            scratch,
            dst_meta,
            dst_data,
            src_meta,
            src_data,
            buf_capacity_bits,
        ) {
            Ok(()) => MergeOutcome::ValueList(scratch_meta),
            Err(()) => MergeOutcome::Promote,
        }
    }
}

/// Single decode pass that encodes and dense-folds in parallel. Falls back to
/// the dense state on encoding failure. Cost: ~2-3x wall clock at `P = 10,
/// B = 5` with `GammaCode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EncodeAndDenseFold;

impl MergeStrategy for EncodeAndDenseFold {
    #[inline]
    fn merge_value_lists<C, F>(
        dst_meta: GapCodecMetadata,
        dst_data: &[u64],
        src_meta: GapCodecMetadata,
        src_data: &[u64],
        scratch: &mut [u64],
        buf_capacity_bits: usize,
        max_entries: usize,
        mut dense_fold: F,
    ) -> MergeOutcome
    where
        C: ValueListCodec + GapEncoder,
        F: FnMut(u64),
    {
        let sum_of_counts = (dst_meta.count as usize) + (src_meta.count as usize);
        if sum_of_counts <= max_entries {
            // No doubt: encode directly.
            let mut scratch_meta = GapCodecMetadata::default();
            return match C::merge_into(
                &mut scratch_meta,
                scratch,
                dst_meta,
                dst_data,
                src_meta,
                src_data,
                buf_capacity_bits,
            ) {
                Ok(()) => MergeOutcome::ValueList(scratch_meta),
                Err(()) => MergeOutcome::Promote,
            };
        }

        // Doubt case: single-pass parallel encode + dense fold.
        let mut encoder = GapCodecEncoder::<C>::new(scratch, buf_capacity_bits);
        let mut encode_failed = false;
        for value in streaming_union::<C>(dst_meta, dst_data, src_meta, src_data) {
            dense_fold(value);
            if !encode_failed && encoder.push(value).is_err() {
                encode_failed = true;
            }
        }
        if encode_failed {
            return MergeOutcome::Dense;
        }
        match encoder.finish() {
            Ok(meta) => MergeOutcome::ValueList(meta),
            Err(()) => MergeOutcome::Dense,
        }
    }
}
