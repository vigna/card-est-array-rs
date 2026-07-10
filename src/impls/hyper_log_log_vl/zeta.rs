/*
 * SPDX-FileCopyrightText: 2025 Luca Cappelletti
 *
 * SPDX-License-Identifier: Apache-2.0 OR LGPL-2.1-or-later
 */

#[cfg(test)]
use super::codec::ValueListCodec;
use super::gap_codec::GapEncoder;
use dsi_bitstream::prelude::{
    BE, BufBitReader, BufBitWriter, MemWordReader, MemWordWriterSlice, ZetaRead, ZetaWrite,
    len_zeta,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZetaCode<const K: usize>;

impl<const K: usize> GapEncoder for ZetaCode<K> {
    // Smallest encoding under Zeta(K).
    fn min_gap_bits() -> usize {
        len_zeta(0, K)
    }

    #[inline]
    fn len_gap(g: u64) -> usize {
        debug_assert!(g >= 1);
        len_zeta(g - 1, K)
    }

    #[inline]
    fn write_gap(
        writer: &mut BufBitWriter<BE, MemWordWriterSlice<u64, &mut [u64]>>,
        g: u64,
    ) -> Result<usize, ()> {
        debug_assert!(g >= 1);
        writer.write_zeta(g - 1, K).map_err(|_| ())
    }

    #[inline]
    fn read_gap(reader: &mut BufBitReader<BE, MemWordReader<u64, &[u64], true>>) -> Option<u64> {
        Some(reader.read_zeta(K).ok()? + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::super::codec::GapCodecMetadata;
    use super::*;

    fn buffer(bits: usize) -> Vec<u64> {
        std::vec![0u64; bits.div_ceil(64)]
    }

    #[test]
    fn empty_round_trip() {
        let buf = buffer(64);
        let metadata = GapCodecMetadata::default();
        let decoded: Vec<u64> = ZetaCode::<3>::iter(metadata, &buf).collect();
        assert!(decoded.is_empty());
    }

    #[test]
    fn dense_small_integers_round_trip_at_k3() {
        let mut buf = buffer(512);
        let capacity_bits = buf.len() * 64;
        let mut metadata = GapCodecMetadata::default();
        for v in 0u64..32 {
            ZetaCode::<3>::insert(&mut metadata, &mut buf, capacity_bits, v).unwrap();
        }
        assert_eq!(metadata.count, 32);
        let decoded: Vec<u64> = ZetaCode::<3>::iter(metadata, &buf).collect();
        let expected: Vec<u64> = (0u64..32).rev().collect();
        assert_eq!(decoded, expected);
    }

    #[test]
    fn dense_small_integers_round_trip_at_k2() {
        let mut buf = buffer(512);
        let capacity_bits = buf.len() * 64;
        let mut metadata = GapCodecMetadata::default();
        for v in 0u64..32 {
            ZetaCode::<2>::insert(&mut metadata, &mut buf, capacity_bits, v).unwrap();
        }
        assert_eq!(metadata.count, 32);
        let decoded: Vec<u64> = ZetaCode::<2>::iter(metadata, &buf).collect();
        let expected: Vec<u64> = (0u64..32).rev().collect();
        assert_eq!(decoded, expected);
    }

    #[test]
    fn duplicate_insert_is_noop() {
        let mut buf = buffer(128);
        let capacity_bits = buf.len() * 64;
        let mut metadata = GapCodecMetadata::default();
        ZetaCode::<3>::insert(&mut metadata, &mut buf, capacity_bits, 7).unwrap();
        let before = metadata;
        let buf_before = buf.clone();
        ZetaCode::<3>::insert(&mut metadata, &mut buf, capacity_bits, 7).unwrap();
        assert_eq!(metadata, before);
        assert_eq!(buf, buf_before);
    }
}
