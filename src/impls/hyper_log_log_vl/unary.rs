/*
 * SPDX-FileCopyrightText: 2025 Luca Cappelletti
 *
 * SPDX-License-Identifier: Apache-2.0 OR LGPL-2.1-or-later
 */

#[cfg(test)]
use super::codec::ValueListCodec;
use super::gap_codec::GapEncoder;
use dsi_bitstream::prelude::{
    BE, BitRead, BitWrite, BufBitReader, BufBitWriter, MemWordReader, MemWordWriterSlice,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnaryCode;

impl GapEncoder for UnaryCode {
    // unary(0) = "1".
    fn min_gap_bits() -> usize {
        1
    }

    #[inline]
    fn len_gap(g: u64) -> usize {
        // unary(n) writes `n + 1` bits (the value + a terminator). We encode
        // `g - 1` so a gap of 1 costs 1 bit.
        debug_assert!(g >= 1);
        g as usize
    }

    #[inline]
    fn write_gap(
        writer: &mut BufBitWriter<BE, MemWordWriterSlice<u64, &mut [u64]>>,
        g: u64,
    ) -> Result<usize, ()> {
        debug_assert!(g >= 1);
        writer.write_unary(g - 1).map_err(|_| ())
    }

    #[inline]
    fn read_gap(reader: &mut BufBitReader<BE, MemWordReader<u64, &[u64], true>>) -> Option<u64> {
        Some(reader.read_unary().ok()? + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::super::codec::GapCodecMetadata;
    use super::*;

    /// Dense small integers round-trip.
    #[test]
    fn dense_small_integers_round_trip() {
        let mut buf = std::vec![0u64; 16];
        let capacity_bits = buf.len() * 64;
        let mut metadata = GapCodecMetadata::default();
        for v in 0u64..32 {
            UnaryCode::insert(&mut metadata, &mut buf, capacity_bits, v).unwrap();
        }
        assert_eq!(metadata.count, 32);
        let decoded: Vec<u64> = UnaryCode::iter(metadata, &buf).collect();
        let expected: Vec<u64> = (0u64..32).rev().collect();
        assert_eq!(decoded, expected);
    }

    /// Predicted bit count matches `writer_tell`.
    #[test]
    fn writer_tell_matches_predicted_bits() {
        let mut buf = std::vec![0u64; 4];
        let capacity_bits = buf.len() * 64;
        let mut metadata = GapCodecMetadata::default();
        for v in 0u64..8 {
            UnaryCode::insert(&mut metadata, &mut buf, capacity_bits, v).unwrap();
        }
        // 8 values sorted desc: [7, 6, 5, 4, 3, 2, 1, 0]. Head = gamma(8) = 7 bits.
        // Gaps: 7 gaps of 1 = 7 bits. Total = 14 bits.
        assert_eq!(metadata.count, 8);
        assert_eq!(metadata.writer_tell, 14);
    }

    /// Duplicate inserts are no-ops.
    #[test]
    fn duplicate_insert_is_noop() {
        let mut buf = std::vec![0u64; 4];
        let capacity_bits = buf.len() * 64;
        let mut metadata = GapCodecMetadata::default();
        UnaryCode::insert(&mut metadata, &mut buf, capacity_bits, 3).unwrap();
        let before = metadata;
        let buf_before = buf.clone();
        UnaryCode::insert(&mut metadata, &mut buf, capacity_bits, 3).unwrap();
        assert_eq!(metadata, before);
        assert_eq!(buf, buf_before);
    }

    /// Overflow leaves state untouched.
    #[test]
    fn overflow_leaves_state_untouched() {
        let mut buf = std::vec![0u64; 1];
        let capacity_bits = buf.len() * 64;
        let mut metadata = GapCodecMetadata::default();
        UnaryCode::insert(&mut metadata, &mut buf, capacity_bits, 0).unwrap();
        let before = metadata;
        let buf_before = buf.clone();

        // Gap of 200 needs 200 bits under unary, well above 64.
        let result = UnaryCode::insert(&mut metadata, &mut buf, capacity_bits, 200);
        assert!(result.is_err());
        assert_eq!(metadata, before);
        assert_eq!(buf, buf_before);
    }
}
