/*
 * SPDX-FileCopyrightText: 2024 Matteo Dell'Acqua
 * SPDX-FileCopyrightText: 2025 Sebastiano Vigna
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

mod estimator;
pub use estimator::*;
mod estimator_array;
pub use estimator_array::*;

use num_primitive::PrimitiveUnsigned;

/// A convenience trait bundling the bounds required for word types used as
/// backends for estimators, plus constants for zero and one (which avoid a
/// dependence from the [`num-traits`] crate).
///
/// [`num-traits`]: https://crates.io/crates/num-traits
pub trait Word: PrimitiveUnsigned {
    const ZERO: Self;
    const ONE: Self;
}

macro_rules! impl_word {
    ($($ty:ty),*) => {
        $(impl Word for $ty {
            const ZERO: Self = 0;
            const ONE: Self = 1;
        })*
    };
}

impl_word!(u8, u16, u32, u64, u128, usize);
