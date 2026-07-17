/*
 * SPDX-FileCopyrightText: 2025 Luca Cappelletti
 *
 * SPDX-License-Identifier: Apache-2.0 OR LGPL-2.1-or-later
 */

//! Shared implementation for value-list codecs. Layout:
//! `[gamma(head)] [gap_code(v_{i-1} - v_i)]*`.

use super::codec::{GapCodecMetadata, ValueListCodec};
use dsi_bitstream::prelude::{
    BE, BitWrite, BufBitReader, BufBitWriter, GammaRead, GammaWrite, MemWordReader,
    MemWordWriterSlice, len_gamma,
};

/// Writes and reads a strictly-positive `u64` gap.
pub trait GapEncoder {
    /// Bit length of encoding `g = 1`.
    #[must_use]
    fn min_gap_bits() -> usize;

    /// Bit length of `g`.
    fn len_gap(g: u64) -> usize;

    #[allow(clippy::result_unit_err)]
    /// Write `g` and return the number of bits written.
    fn write_gap(
        writer: &mut BufBitWriter<BE, MemWordWriterSlice<u64, &mut [u64]>>,
        g: u64,
    ) -> Result<usize, ()>;

    /// Read the next gap.
    fn read_gap(reader: &mut BufBitReader<BE, MemWordReader<u64, &[u64], true>>) -> Option<u64>;
}

/// Bits needed to encode `values`.
#[inline]
fn predict_encoded_bits<G: GapEncoder>(values: &[u64]) -> usize {
    let mut predicted: usize = 0;
    if let Some((&first, rest)) = values.split_first() {
        predicted += len_gamma(first) as usize;
        let mut prev = first;
        for &current in rest {
            predicted += G::len_gap(prev - current);
            prev = current;
        }
    }
    predicted
}

/// Insert `value` into `buf`, updating `metadata`.
#[allow(clippy::result_unit_err)]
#[inline]
pub(crate) fn insert_generic<G: GapEncoder>(
    metadata: &mut GapCodecMetadata,
    buf: &mut [u64],
    buf_capacity_bits: usize,
    value: u64,
) -> Result<(), ()> {
    // `u64::MAX` is outside dsi-bitstream's gamma domain (`[0, u64::MAX)`) and
    // so cannot be stored as a list head. Reject it before touching `buf` so
    // the caller promotes to dense, which handles any hashable value.
    if value == u64::MAX {
        return Err(());
    }
    let mut values: Vec<u64> = iter_generic::<G>(*metadata, buf).collect();
    if values.binary_search_by(|probe| value.cmp(probe)).is_ok() {
        return Ok(());
    }
    let insert_at = values.partition_point(|&probe| probe > value);
    values.insert(insert_at, value);

    // Budget check BEFORE we touch `buf`, so failures leave the buffer
    // untouched (the caller relies on this to promote cleanly).
    if predict_encoded_bits::<G>(&values) > buf_capacity_bits {
        return Err(());
    }

    // Commit: clear `buf` and re-encode via the shared streaming encoder.
    let mut encoder = GapCodecEncoder::<G>::new(buf, buf_capacity_bits);
    for &v in &values {
        encoder.push(v)?;
    }
    *metadata = encoder.finish()?;
    Ok(())
}

/// Merged descending iterator over two sorted-descending value lists.
#[inline]
pub(crate) fn streaming_union<'a, G: GapEncoder + 'a>(
    dst_meta: GapCodecMetadata,
    dst_buf: &'a [u64],
    src_meta: GapCodecMetadata,
    src_buf: &'a [u64],
) -> impl Iterator<Item = u64> + 'a {
    StreamingUnion {
        dst: iter_generic::<G>(dst_meta, dst_buf).peekable(),
        src: iter_generic::<G>(src_meta, src_buf).peekable(),
    }
}

struct StreamingUnion<'a, G: GapEncoder> {
    dst: core::iter::Peekable<GapCodecIter<'a, G>>,
    src: core::iter::Peekable<GapCodecIter<'a, G>>,
}

impl<'a, G: GapEncoder> Iterator for StreamingUnion<'a, G> {
    type Item = u64;

    #[inline]
    fn next(&mut self) -> Option<u64> {
        match (self.dst.peek(), self.src.peek()) {
            (Some(&d), Some(&s)) => match d.cmp(&s) {
                core::cmp::Ordering::Greater => {
                    self.dst.next();
                    Some(d)
                }
                core::cmp::Ordering::Less => {
                    self.src.next();
                    Some(s)
                }
                core::cmp::Ordering::Equal => {
                    self.dst.next();
                    self.src.next();
                    Some(d)
                }
            },
            (Some(&d), None) => {
                self.dst.next();
                Some(d)
            }
            (None, Some(&s)) => {
                self.src.next();
                Some(s)
            }
            (None, None) => None,
        }
    }
}

/// Streaming bit-level encoder. Values MUST be pushed in strictly descending
/// order.
pub(crate) struct GapCodecEncoder<'a, G: GapEncoder> {
    writer: BufBitWriter<BE, MemWordWriterSlice<u64, &'a mut [u64]>>,
    written: usize,
    count: u32,
    previous: Option<u64>,
    capacity_bits: usize,
    _marker: std::marker::PhantomData<G>,
}

impl<'a, G: GapEncoder> GapCodecEncoder<'a, G> {
    /// Fresh encoder over `buf`. Zeroes it first.
    #[inline]
    pub(crate) fn new(buf: &'a mut [u64], capacity_bits: usize) -> Self {
        for word in buf.iter_mut() {
            *word = 0;
        }
        Self {
            writer: BufBitWriter::<BE, _>::new(MemWordWriterSlice::new(buf)),
            written: 0,
            count: 0,
            previous: None,
            capacity_bits,
            _marker: std::marker::PhantomData,
        }
    }

    /// Push `value`. `Err(())` on buffer or count overflow.
    #[allow(clippy::result_unit_err)]
    #[inline]
    pub(crate) fn push(&mut self, value: u64) -> Result<(), ()> {
        // A `u64::MAX` head is unencodable by gamma (see `insert_generic`).
        if self.previous.is_none() && value == u64::MAX {
            return Err(());
        }
        let cost = match self.previous {
            None => len_gamma(value) as usize,
            Some(prev) => G::len_gap(prev - value),
        };
        if self.written + cost > self.capacity_bits {
            return Err(());
        }
        let bits = match self.previous {
            None => self.writer.write_gamma(value).map_err(|_| ())?,
            Some(prev) => G::write_gap(&mut self.writer, prev - value)?,
        };
        self.written += bits;
        self.previous = Some(value);
        self.count = self.count.checked_add(1).ok_or(())?;
        Ok(())
    }

    /// Flush and produce the metadata.
    #[allow(clippy::result_unit_err)]
    #[inline]
    pub(crate) fn finish(mut self) -> Result<GapCodecMetadata, ()> {
        self.writer.flush().map_err(|_| ())?;
        Ok(GapCodecMetadata {
            count: self.count,
            writer_tell: u32::try_from(self.written).map_err(|_| ())?,
        })
    }
}

/// Streaming two-pointer union into `out_buf`.
#[allow(clippy::result_unit_err)]
#[inline]
pub(crate) fn merge_generic<G: GapEncoder>(
    out_meta: &mut GapCodecMetadata,
    out_buf: &mut [u64],
    dst_meta: GapCodecMetadata,
    dst_buf: &[u64],
    src_meta: GapCodecMetadata,
    src_buf: &[u64],
    buf_capacity_bits: usize,
) -> Result<(), ()> {
    *out_meta = GapCodecMetadata::default();
    let mut encoder = GapCodecEncoder::<G>::new(out_buf, buf_capacity_bits);
    for value in streaming_union::<G>(dst_meta, dst_buf, src_meta, src_buf) {
        encoder.push(value)?;
    }
    *out_meta = encoder.finish()?;
    Ok(())
}

#[inline]
pub(crate) fn iter_generic<'a, G: GapEncoder>(
    metadata: GapCodecMetadata,
    buf: &'a [u64],
) -> GapCodecIter<'a, G> {
    GapCodecIter::new(metadata, buf)
}

/// Decoder over a gap-coded value list.
pub(crate) struct GapCodecIter<'a, G: GapEncoder> {
    reader: BufBitReader<BE, MemWordReader<u64, &'a [u64], true>>,
    remaining: u32,
    previous: Option<u64>,
    _phantom: core::marker::PhantomData<G>,
}

impl<'a, G: GapEncoder> GapCodecIter<'a, G> {
    fn new(metadata: GapCodecMetadata, buf: &'a [u64]) -> Self {
        Self {
            reader: BufBitReader::<BE, _>::new(MemWordReader::new_inf(buf)),
            remaining: metadata.count,
            previous: None,
            _phantom: core::marker::PhantomData,
        }
    }
}

impl<'a, G: GapEncoder> Iterator for GapCodecIter<'a, G> {
    type Item = u64;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        let value = match self.previous {
            None => self.reader.read_gamma().ok()?,
            Some(prev) => {
                let gap = G::read_gap(&mut self.reader)?;
                prev.checked_sub(gap)?
            }
        };
        self.previous = Some(value);
        Some(value)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining as usize, Some(self.remaining as usize))
    }
}

/// Blanket [`ValueListCodec`] impl for any `T: GapEncoder`.
impl<T: GapEncoder> ValueListCodec for T
where
    T: 'static,
{
    fn insert(
        metadata: &mut GapCodecMetadata,
        buf: &mut [u64],
        buf_capacity_bits: usize,
        value: u64,
    ) -> Result<(), ()> {
        insert_generic::<T>(metadata, buf, buf_capacity_bits, value)
    }

    fn iter<'a>(metadata: GapCodecMetadata, buf: &'a [u64]) -> impl Iterator<Item = u64> + 'a {
        iter_generic::<T>(metadata, buf)
    }

    #[inline]
    fn max_entries(buf_capacity_bits: usize) -> usize {
        // Head gamma cost (at most ~65 bits) is ignored. Skipping it only
        // loosens the bound.
        buf_capacity_bits / T::min_gap_bits().max(1)
    }

    #[inline]
    fn merge_into(
        out_meta: &mut GapCodecMetadata,
        out_buf: &mut [u64],
        dst_meta: GapCodecMetadata,
        dst_buf: &[u64],
        src_meta: GapCodecMetadata,
        src_buf: &[u64],
        buf_capacity_bits: usize,
    ) -> Result<(), ()> {
        merge_generic::<T>(
            out_meta,
            out_buf,
            dst_meta,
            dst_buf,
            src_meta,
            src_buf,
            buf_capacity_bits,
        )
    }
}
