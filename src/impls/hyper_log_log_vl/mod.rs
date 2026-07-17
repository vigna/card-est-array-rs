/*
 * SPDX-FileCopyrightText: 2025 Luca Cappelletti
 *
 * SPDX-License-Identifier: Apache-2.0 OR LGPL-2.1-or-later
 */

//! Value-list-augmented [`HyperLogLog`](super::HyperLogLog).
//!
//! Counters begin storing raw pre-hash `u64` values through a pluggable
//! [`ValueListCodec`]. On overflow they promote in place to the wrapped dense
//! [`HyperLogLog`](super::HyperLogLog) with byte-identical state.
//!
//! New codecs implement [`GapEncoder`](gap_codec::GapEncoder). A blanket impl
//! lifts it to [`ValueListCodec`]. The (value list, value list) merge arm is
//! selected at compile time via [`MergeStrategy`].

pub mod codec;
pub mod estimator;
pub mod gap_codec;
pub mod merge_strategy;
pub use estimator::{HyperLogLogVl, HyperLogLogVlBuilder, HyperLogLogVlHelper, LosslessU64};
pub use merge_strategy::{EncodeAndDenseFold, MergeOutcome, MergeStrategy, SumOfCountsFastPath};
mod gamma;
mod pi;
mod rice;
mod unary;
mod zeta;

pub use codec::{GapCodecMetadata, ValueListCodec};
pub use gamma::GammaCode;
pub use pi::PiCode;
pub use rice::RiceCode;
pub use unary::UnaryCode;
pub use zeta::ZetaCode;
