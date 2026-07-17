/*
 * SPDX-FileCopyrightText: 2025 Luca Cappelletti
 *
 * SPDX-License-Identifier: Apache-2.0 OR LGPL-2.1-or-later
 */

#[cfg(test)]
use super::codec::ValueListCodec;
use super::gap_codec::GapEncoder;
use dsi_bitstream::prelude::{
    BE, BufBitReader, BufBitWriter, MemWordReader, MemWordWriterSlice, RiceRead, RiceWrite,
    len_rice,
};

/// Value-list codec whose gaps are Rice codes with parameter `2^LOG2_B`.
///
/// `LOG2_B` sets how many low bits are carried below the unary quotient.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RiceCode<const LOG2_B: usize>;

impl<const LOG2_B: usize> GapEncoder for RiceCode<LOG2_B> {
    // Rice-K at gap 1: unary quotient 0 plus LOG2_B binary bits.
    fn min_gap_bits() -> usize {
        1 + LOG2_B
    }

    #[inline]
    fn len_gap(g: u64) -> usize {
        debug_assert!(g >= 1);
        len_rice(g - 1, LOG2_B)
    }

    #[inline]
    fn write_gap(
        writer: &mut BufBitWriter<BE, MemWordWriterSlice<u64, &mut [u64]>>,
        g: u64,
    ) -> Result<usize, ()> {
        debug_assert!(g >= 1);
        writer.write_rice(g - 1, LOG2_B).map_err(|_| ())
    }

    #[inline]
    fn read_gap(reader: &mut BufBitReader<BE, MemWordReader<u64, &[u64], true>>) -> Option<u64> {
        Some(reader.read_rice(LOG2_B).ok()? + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::super::codec::GapCodecMetadata;
    use super::*;

    /// Rice-3 dense small integers round-trip.
    #[test]
    fn test_dense_small_integers_rice3_round_trip() {
        let mut buf = std::vec![0u64; 16];
        let capacity_bits = buf.len() * 64;
        let mut metadata = GapCodecMetadata::default();
        for v in 0u64..32 {
            RiceCode::<3>::insert(&mut metadata, &mut buf, capacity_bits, v).unwrap();
        }
        assert_eq!(metadata.count, 32);
        let decoded: Vec<u64> = RiceCode::<3>::iter(metadata, &buf).collect();
        let expected: Vec<u64> = (0u64..32).rev().collect();
        assert_eq!(decoded, expected);
    }

    /// Rice-0 matches unary.
    #[test]
    fn test_rice_0_matches_unary_semantics() {
        let mut buf = std::vec![0u64; 4];
        let capacity_bits = buf.len() * 64;
        let mut metadata = GapCodecMetadata::default();
        for v in 0u64..8 {
            RiceCode::<0>::insert(&mut metadata, &mut buf, capacity_bits, v).unwrap();
        }
        assert_eq!(metadata.count, 8);
        // Same head + gap layout as UnaryCode: 8 values, 7 gaps of 1, head
        // gamma(8) = 7 bits, gaps = 7 bits. Total 14 bits.
        assert_eq!(metadata.writer_tell, 14);
    }

    /// Rice-8 round-trip.
    #[test]
    fn test_rice_8_round_trip() {
        let mut buf = std::vec![0u64; 16];
        let capacity_bits = buf.len() * 64;
        let mut metadata = GapCodecMetadata::default();
        let mut state: u64 = 0xC0FF_EE00_C0FF_EE00;
        let mut inserted: Vec<u64> = Vec::new();
        for _ in 0..32 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let v = state >> 40; // ~24-bit values, gaps in tens-of-thousands range.
            if RiceCode::<8>::insert(&mut metadata, &mut buf, capacity_bits, v).is_ok()
                && !inserted.contains(&v)
            {
                inserted.push(v);
            }
        }
        inserted.sort_by(|a, b| b.cmp(a));
        let decoded: Vec<u64> = RiceCode::<8>::iter(metadata, &buf).collect();
        assert_eq!(decoded, inserted);
    }

    /// Duplicate inserts are no-ops.
    #[test]
    fn test_duplicate_insert_is_noop() {
        let mut buf = std::vec![0u64; 4];
        let capacity_bits = buf.len() * 64;
        let mut metadata = GapCodecMetadata::default();
        RiceCode::<3>::insert(&mut metadata, &mut buf, capacity_bits, 5).unwrap();
        let before = metadata;
        let buf_before = buf.clone();
        RiceCode::<3>::insert(&mut metadata, &mut buf, capacity_bits, 5).unwrap();
        assert_eq!(metadata, before);
        assert_eq!(buf, buf_before);
    }
}
