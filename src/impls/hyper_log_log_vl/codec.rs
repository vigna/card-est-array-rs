/*
 * SPDX-FileCopyrightText: 2025 Luca Cappelletti
 *
 * SPDX-License-Identifier: Apache-2.0 OR LGPL-2.1-or-later
 */
//! The [`ValueListCodec`] trait.

/// Value-list header. Packs into the estimator's header word.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct GapCodecMetadata {
    /// Number of stored entries.
    pub count: u32,
    /// Bit position past the last written bit.
    pub writer_tell: u32,
}

/// Codec for a sorted-descending list of `u64` values in a `[u64]` buffer.
/// Default-constructed [`GapCodecMetadata`] MUST denote the empty list.
pub trait ValueListCodec {
    /// Splices `value` into the list. Duplicates are ignored. On `Err(())`,
    /// `buf` and `metadata` are left untouched.
    #[allow(clippy::result_unit_err)]
    fn insert(
        metadata: &mut GapCodecMetadata,
        buf: &mut [u64],
        buf_capacity_bits: usize,
        value: u64,
    ) -> Result<(), ()>;

    /// Iterate values in descending order.
    fn iter<'a>(metadata: GapCodecMetadata, buf: &'a [u64]) -> impl Iterator<Item = u64> + 'a;

    /// Strict upper bound on entries that fit in `buf_capacity_bits` under the
    /// codec's most compressible input.
    fn max_entries(buf_capacity_bits: usize) -> usize;

    /// Streaming O(N + M) two-pointer union into `out_buf`. On `Err(())` the
    /// output is in unspecified partial state and the caller must promote.
    ///
    /// The default is O((N + M)^2). Codecs SHOULD override.
    #[allow(clippy::result_unit_err)]
    fn merge_into(
        out_meta: &mut GapCodecMetadata,
        out_buf: &mut [u64],
        dst_meta: GapCodecMetadata,
        dst_buf: &[u64],
        src_meta: GapCodecMetadata,
        src_buf: &[u64],
        buf_capacity_bits: usize,
    ) -> Result<(), ()> {
        *out_meta = GapCodecMetadata::default();
        for v in Self::iter(dst_meta, dst_buf) {
            Self::insert(out_meta, out_buf, buf_capacity_bits, v)?;
        }
        for v in Self::iter(src_meta, src_buf) {
            Self::insert(out_meta, out_buf, buf_capacity_bits, v)?;
        }
        Ok(())
    }
}
