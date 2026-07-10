/*
 * SPDX-FileCopyrightText: 2025 Luca Cappelletti
 *
 * SPDX-License-Identifier: Apache-2.0 OR LGPL-2.1-or-later
 */

#[cfg(test)]
use super::codec::ValueListCodec;
use super::gap_codec::GapEncoder;
use dsi_bitstream::prelude::{
    BE, BufBitReader, BufBitWriter, GammaRead, GammaWrite, MemWordReader, MemWordWriterSlice,
    len_gamma,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GammaCode;

impl GapEncoder for GammaCode {
    // gamma(1) = "1".
    fn min_gap_bits() -> usize {
        1
    }

    #[inline]
    fn len_gap(g: u64) -> usize {
        debug_assert!(g >= 1);
        // `dsi_bitstream::codes::len_gamma(n)` returns the length of `γ(n+1)`,
        // so `len_gamma(g - 1)` is the length of `γ(g)`.
        len_gamma(g - 1) as usize
    }

    #[inline]
    fn write_gap(
        writer: &mut BufBitWriter<BE, MemWordWriterSlice<u64, &mut [u64]>>,
        g: u64,
    ) -> Result<usize, ()> {
        debug_assert!(g >= 1);
        writer.write_gamma(g - 1).map_err(|_| ())
    }

    #[inline]
    fn read_gap(reader: &mut BufBitReader<BE, MemWordReader<u64, &[u64], true>>) -> Option<u64> {
        Some(reader.read_gamma().ok()? + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::super::codec::GapCodecMetadata;
    use super::*;

    /// Empty list round-trips.
    #[test]
    fn empty_round_trip() {
        let buf = std::vec![0u64; 4];
        let metadata = GapCodecMetadata::default();
        assert_eq!(metadata.count, 0);
        assert_eq!(metadata.writer_tell, 0);
        let decoded: Vec<u64> = GammaCode::iter(metadata, &buf).collect();
        assert!(decoded.is_empty());
    }

    /// Single value round-trips.
    #[test]
    fn single_value_round_trip() {
        let mut buf = std::vec![0u64; 4];
        let capacity_bits = buf.len() * 64;
        let mut metadata = GapCodecMetadata::default();
        GammaCode::insert(&mut metadata, &mut buf, capacity_bits, 42u64).unwrap();
        assert_eq!(metadata.count, 1);
        let decoded: Vec<u64> = GammaCode::iter(metadata, &buf).collect();
        assert_eq!(decoded, std::vec![42u64]);
    }

    /// Dense small integers round-trip.
    #[test]
    fn dense_small_integers_round_trip() {
        let mut buf = std::vec![0u64; 16];
        let capacity_bits = buf.len() * 64;
        let mut metadata = GapCodecMetadata::default();
        for v in 0u64..32 {
            GammaCode::insert(&mut metadata, &mut buf, capacity_bits, v).unwrap();
        }
        assert_eq!(metadata.count, 32);
        let decoded: Vec<u64> = GammaCode::iter(metadata, &buf).collect();
        let expected: Vec<u64> = (0u64..32).rev().collect();
        assert_eq!(decoded, expected);
    }

    /// Duplicate inserts are no-ops.
    #[test]
    fn duplicate_insert_is_noop() {
        let mut buf = std::vec![0u64; 4];
        let capacity_bits = buf.len() * 64;
        let mut metadata = GapCodecMetadata::default();
        GammaCode::insert(&mut metadata, &mut buf, capacity_bits, 7).unwrap();
        let before = metadata;
        let buf_before = buf.clone();
        GammaCode::insert(&mut metadata, &mut buf, capacity_bits, 7).unwrap();
        assert_eq!(metadata, before);
        assert_eq!(buf, buf_before);
    }

    /// Overflow leaves state untouched.
    #[test]
    fn overflow_leaves_state_untouched() {
        let mut buf = std::vec![0u64; 1];
        let capacity_bits = buf.len() * 64;
        let mut metadata = GapCodecMetadata::default();
        GammaCode::insert(&mut metadata, &mut buf, capacity_bits, 0).unwrap();
        let before = metadata;
        let buf_before = buf.clone();

        let big = 1u64 << 40;
        let result = GammaCode::insert(&mut metadata, &mut buf, capacity_bits, big);
        assert!(result.is_err(), "insert should refuse to overflow");
        assert_eq!(metadata, before);
        assert_eq!(buf, buf_before);
    }
}
