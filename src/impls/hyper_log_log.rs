/*
 * SPDX-FileCopyrightText: 2024 Matteo Dell'Acqua
 * SPDX-FileCopyrightText: 2025 Sebastiano Vigna
 *
 * SPDX-License-Identifier: Apache-2.0 OR LGPL-2.1-or-later
 */

use anyhow::{ensure, Result};
use common_traits::{CastableFrom, CastableInto, Number, UpcastableInto};
use std::hash::*;
use std::{borrow::Borrow, f64::consts::LN_2};
use sux::{
    bits::BitFieldVec,
    traits::{BitFieldSliceMut, Word},
};

use crate::traits::{EstimationLogic, MergeEstimationLogic, SliceEstimationLogic};

use super::DefaultEstimator;

/// The type returned by the hash function.
type HashResult = u64;

/// Estimator logic implementing the HyperLogLog algorithm.
///
/// Instances are built using [`HyperLogLogBuilder`], which provides convenient
/// ways to set the internal parameters.
///
/// Note that `T` can be any type satisfying the [`Hash`] trait. The parameter
/// `H` makes it possible to select a hashing algorithm, and `W` is the unsigned
/// type used to store backends.
///
/// An important constraint is that `W` must be able to represent exactly the
/// backend of an estimator. While usually `usize` will work (and it is the default
/// type chosen by [`new`](HyperLogLogBuilder::new)), with odd register sizes
/// and small number of registers it might be necessary to select a smaller
/// type, resulting in slower merges. For example, using 16 5-bit registers one
/// needs to use `u16`, whereas for 16 6-bit registers `u32` will be sufficient.
#[derive(Debug, PartialEq)]
pub struct HyperLogLog<T, H, W> {
    build_hasher: H,
    register_size: usize,
    num_registers_minus_1: HashResult,
    log_2_num_registers: usize,
    sentinel_mask: HashResult,
    num_registers: usize,
    pub(super) words_per_counter: usize,
    alpha_m_m: f64,
    msb_mask: Box<[W]>,
    lsb_mask: Box<[W]>,
    _marker: std::marker::PhantomData<T>,
}

// We implement Clone manually because we do not want to require that T is
// Clone.
impl<T, H: Clone, W: Clone> Clone for HyperLogLog<T, H, W> {
    fn clone(&self) -> Self {
        Self {
            build_hasher: self.build_hasher.clone(),
            register_size: self.register_size,
            num_registers_minus_1: self.num_registers_minus_1,
            log_2_num_registers: self.log_2_num_registers,
            sentinel_mask: self.sentinel_mask,
            num_registers: self.num_registers,
            words_per_counter: self.words_per_counter,
            alpha_m_m: self.alpha_m_m,
            msb_mask: self.msb_mask.clone(),
            lsb_mask: self.lsb_mask.clone(),
            _marker: std::marker::PhantomData,
        }
    }
}

impl<T, H: Clone, W: Word> HyperLogLog<T, H, W> {
    /// Returns the value contained in a register of a given backend.
    #[inline(always)]
    fn get_register_unchecked(&self, backend: impl AsRef<[W]>, index: usize) -> W {
        let backend = backend.as_ref();
        let bit_width = self.register_size;
        let mask = W::MAX >> (W::BITS - bit_width);
        let pos = index * bit_width;
        let word_index = pos / W::BITS;
        let bit_index = pos % W::BITS;

        if bit_index + bit_width <= W::BITS {
            (unsafe { *backend.get_unchecked(word_index) } >> bit_index) & mask
        } else {
            ((unsafe { *backend.get_unchecked(word_index) } >> bit_index)
                | (unsafe { *backend.get_unchecked(word_index + 1) } << (W::BITS - bit_index)))
                & mask
        }
    }

    /// Sets the value contained in a register of a given backend.
    #[inline(always)]
    fn set_register_unchecked(&self, mut backend: impl AsMut<[W]>, index: usize, new_value: W) {
        let backend = backend.as_mut();
        let bit_width = self.register_size;
        let mask = W::MAX >> (W::BITS - bit_width);
        let pos = index * bit_width;
        let word_index = pos / W::BITS;
        let bit_index = pos % W::BITS;

        if bit_index + bit_width <= W::BITS {
            let mut word = unsafe { *backend.get_unchecked_mut(word_index) };
            word &= !(mask << bit_index);
            word |= new_value << bit_index;
            unsafe { *backend.get_unchecked_mut(word_index) = word };
        } else {
            let mut word = unsafe { *backend.get_unchecked_mut(word_index) };
            word &= (W::ONE << bit_index) - W::ONE;
            word |= new_value << bit_index;
            unsafe { *backend.get_unchecked_mut(word_index) = word };

            let mut word = unsafe { *backend.get_unchecked_mut(word_index + 1) };
            word &= !(mask >> (W::BITS - bit_index));
            word |= new_value >> (W::BITS - bit_index);
            unsafe { *backend.get_unchecked_mut(word_index + 1) = word };
        }
    }
}

impl<
        T: Hash,
        H: BuildHasher + Clone,
        W: Word + UpcastableInto<HashResult> + CastableFrom<HashResult>,
    > SliceEstimationLogic<W> for HyperLogLog<T, H, W>
{
    fn backend_len(&self) -> usize {
        self.words_per_counter
    }
}

impl<
        T: Hash,
        H: BuildHasher + Clone,
        W: Word + UpcastableInto<HashResult> + CastableFrom<HashResult>,
    > EstimationLogic for HyperLogLog<T, H, W>
{
    type Item = T;
    type Backend = [W];
    type Estimator<'a>
        = DefaultEstimator<Self, &'a Self, Box<[W]>>
    where
        T: 'a,
        W: 'a,
        H: 'a;

    fn new_estimator(&self) -> Self::Estimator<'_> {
        Self::Estimator::new(
            self,
            vec![W::ZERO; self.words_per_counter].into_boxed_slice(),
        )
    }

    fn add(&self, mut backend: &mut Self::Backend, element: impl Borrow<T>) {
        let x = self.build_hasher.hash_one(element.borrow());
        let j = x & self.num_registers_minus_1;
        let r =
            ((x >> self.log_2_num_registers) | self.sentinel_mask).trailing_zeros() as HashResult;
        let register = j as usize;

        debug_assert!(r < (1 << self.register_size) - 1);
        debug_assert!(register < self.num_registers);

        let current_value = self.get_register_unchecked(&mut backend, register);
        let candidate_value = r + 1;
        let new_value = std::cmp::max(current_value, candidate_value.cast());
        if current_value != new_value {
            self.set_register_unchecked(backend, register, new_value);
        }
    }

    fn estimate(&self, backend: &[W]) -> f64 {
        let mut harmonic_mean = 0.0;
        let mut zeroes = 0;

        for i in 0..self.num_registers {
            let value: u64 = self.get_register_unchecked(backend, i).upcast();
            if value == 0 {
                zeroes += 1;
            }
            harmonic_mean += 1.0 / (1 << value) as f64;
        }

        let mut estimate = self.alpha_m_m / harmonic_mean;
        if zeroes != 0 && estimate < 2.5 * self.num_registers as f64 {
            estimate = self.num_registers as f64 * (self.num_registers as f64 / zeroes as f64).ln();
        }
        estimate
    }

    fn clear(&self, backend: &mut [W]) {
        backend.as_mut().fill(W::ZERO);
    }

    fn set(&self, dst: &mut [W], src: &[W]) {
        debug_assert_eq!(dst.as_mut().len(), src.as_ref().len());
        dst.as_mut().copy_from_slice(src.as_ref());
    }
}

/// Helper for merge operations with [`HyperLogLog`] logic.
pub struct HyperLogLogHelper<W> {
    acc: Vec<W>,
    mask: Vec<W>,
}

impl<
        T: Hash,
        H: BuildHasher + Clone,
        W: Word + UpcastableInto<HashResult> + CastableFrom<HashResult>,
    > MergeEstimationLogic for HyperLogLog<T, H, W>
{
    type Helper = HyperLogLogHelper<W>;

    fn new_helper(&self) -> Self::Helper {
        HyperLogLogHelper {
            acc: vec![W::ZERO; self.words_per_counter],
            mask: vec![W::ZERO; self.words_per_counter],
        }
    }

    fn merge_with_helper(&self, dst: &mut [W], src: &[W], helper: &mut Self::Helper) {
        merge_hyperloglog_bitwise(
            dst,
            src,
            self.msb_mask.as_ref(),
            self.lsb_mask.as_ref(),
            &mut helper.acc,
            &mut helper.mask,
            self.register_size,
        );
    }
}

/// Builds a [`HyperLogLog`] counter logic.
#[derive(Debug, Clone)]
pub struct HyperLogLogBuilder<H, W = usize> {
    build_hasher: H,
    log_2_num_registers: usize,
    n: usize,
    _marker: std::marker::PhantomData<(H, W)>,
}

impl HyperLogLogBuilder<BuildHasherDefault<DefaultHasher>, usize> {
    /// Creates a new builder for a [`HyperLogLog`] logic with a default word
    /// type of `usize`.
    pub fn new(n: usize) -> Self {
        Self {
            build_hasher: BuildHasherDefault::default(),
            log_2_num_registers: 4,
            n,
            _marker: std::marker::PhantomData,
        }
    }
}

fn min_alignment(bits: usize) -> String {
    if bits % 128 == 0 {
        "u128"
    } else if bits % 64 == 0 {
        "u64"
    } else if bits % 32 == 0 {
        "u32"
    } else if bits % 16 == 0 {
        "u16"
    } else {
        "u8"
    }
    .to_string()
}

impl HyperLogLog<(), (), ()> {
    /// Returns the logarithm of the number of registers per counter that are
    /// necessary to attain a given relative standard deviation.
    ///
    /// # Arguments
    /// * `rsd`: the relative standard deviation to be attained.
    pub fn log_2_num_of_registers(rsd: f64) -> usize {
        ((1.106 / rsd).pow(2.0)).log2().ceil() as usize
    }

    /// Returns the relative standard deviation corresponding to a given number
    /// of registers per counter.
    ///
    /// # Arguments
    ///
    /// * `log_2_num_registers`: the logarithm of the number of registers per
    ///   counter.
    pub fn rel_std(log_2_num_registers: usize) -> f64 {
        let tmp = match log_2_num_registers {
            4 => 1.106,
            5 => 1.070,
            6 => 1.054,
            7 => 1.046,
            _ => 1.04,
        };
        tmp / ((1 << log_2_num_registers) as f64).sqrt()
    }

    /// Returns the register size in bits, given an upper bound on the number of
    /// distinct elements.
    ///
    /// # Arguments
    /// * `n`: an upper bound on the number of distinct elements.
    pub fn register_size(n: usize) -> usize {
        std::cmp::max(5, (((n as f64).ln() / LN_2) / LN_2).ln().ceil() as usize)
    }
}

impl<H, W: Word> HyperLogLogBuilder<H, W> {
    /// Sets the desired relative standard deviation.
    ///
    /// ## Note
    ///
    /// This is a high-level alternative to [`Self::log_2_num_reg`]. Calling one
    /// after the other invalidates the work done by the first one.
    ///
    /// # Arguments
    /// * `rsd`: the relative standard deviation to be attained.
    pub fn rsd(self, rsd: f64) -> Self {
        self.log_2_num_reg(HyperLogLog::log_2_num_of_registers(rsd))
    }

    /// Sets the base-2 logarithm of the number of register.
    ///
    /// ## Note
    /// This is a low-level alternative to [`Self::rsd`].
    /// Calling one after the other invalidates the work done by the first one.
    ///
    /// # Arguments
    /// * `log_2_num_registers`: the logarithm of the number of registers per counter.
    pub fn log_2_num_reg(mut self, log_2_num_registers: usize) -> Self {
        self.log_2_num_registers = log_2_num_registers;
        self
    }

    /// Sets the type `W` to use to represent backends.
    ///
    /// See the [`logic documentation`](HyperLogLog) for the limitations on the
    /// choice of `W2`.
    pub fn word_type<W2>(self) -> HyperLogLogBuilder<H, W2> {
        HyperLogLogBuilder {
            n: self.n,
            build_hasher: self.build_hasher,
            log_2_num_registers: self.log_2_num_registers,
            _marker: std::marker::PhantomData,
        }
    }

    /// Sets the upper bound on the number of elements.
    pub fn num_elements(mut self, n: usize) -> Self {
        self.n = n;
        self
    }

    /// Sets the [`BuildHasher`] to use.
    ///
    /// Using this method you can select a specific hashed based on one or more
    /// seeds.
    pub fn build_hasher<H2>(self, build_hasher: H2) -> HyperLogLogBuilder<H2, W> {
        HyperLogLogBuilder {
            n: self.n,
            log_2_num_registers: self.log_2_num_registers,
            build_hasher,
            _marker: std::marker::PhantomData,
        }
    }

    /// Builds the logic.
    ///
    /// The type of objects the estimators keep track of is defined here by `T`,
    /// but it is usually inferred by the compiler.
    ///
    /// # Errors
    ///
    /// Errors will be caused by consistency checks (at least 16 registers per
    /// counter, backend bits divisible exactly `W::BITS`)
    pub fn build<T>(self) -> Result<HyperLogLog<T, H, W>> {
        let log_2_num_registers = self.log_2_num_registers;
        let num_elements = self.n;

        // This ensures counters are at least 16-bit-aligned.
        ensure!(
            log_2_num_registers >= 4,
            "the logarithm of the number of registers per counter should be at least 4; got {}",
            log_2_num_registers
        );

        let number_of_registers = 1 << log_2_num_registers;
        let register_size = HyperLogLog::register_size(num_elements);
        let sentinel_mask = 1 << ((1 << register_size) - 2);
        let alpha = match log_2_num_registers {
            4 => 0.673,
            5 => 0.697,
            6 => 0.709,
            _ => 0.7213 / (1.0 + 1.079 / number_of_registers as f64),
        };
        let num_registers_minus_1 = (number_of_registers - 1) as HashResult;

        let counter_size_in_bits = number_of_registers * register_size;

        // This ensures counters are always aligned to W
        ensure!(
            counter_size_in_bits % W::BITS == 0,
            "W should allow counter backends to be aligned. Use {} or smaller unsigned integer types",
            min_alignment(counter_size_in_bits)
        );
        let counter_size_in_words = counter_size_in_bits / W::BITS;

        let mut msb = BitFieldVec::new(register_size, number_of_registers);
        let mut lsb = BitFieldVec::new(register_size, number_of_registers);
        let msb_w = W::ONE << (register_size - 1);
        let lsb_w = W::ONE;
        for i in 0..number_of_registers {
            msb.set(i, msb_w);
            lsb.set(i, lsb_w);
        }

        Ok(HyperLogLog {
            num_registers: number_of_registers,
            num_registers_minus_1,
            log_2_num_registers,
            register_size,
            alpha_m_m: alpha * (number_of_registers as f64).powi(2),
            sentinel_mask,
            build_hasher: self.build_hasher,
            msb_mask: msb.as_slice().into(),
            lsb_mask: lsb.as_slice().into(),
            words_per_counter: counter_size_in_words,
            _marker: std::marker::PhantomData,
        })
    }
}

impl<T, H, W> std::fmt::Display for HyperLogLog<T, H, W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "HyperLogLog with relative standard deviation: {}% ({} registers/counter, {} bits/register, {} bytes/counter)",
            100.0 * HyperLogLog::rel_std(self.log_2_num_registers),
            self.num_registers,
            self.register_size,
            (self.num_registers * self.register_size) / 8
        )
    }
}

/// Performs a multiple precision subtraction, leaving the result in the first operand.
/// The operands MUST have the same length.
///
/// # Arguments
/// * `x`: the first operand. This will contain the final result.
/// * `y`: the second operand that will be subtracted from `x`.
#[inline(always)]
pub(super) fn subtract<W: Word>(x: &mut [W], y: &[W]) {
    debug_assert_eq!(x.len(), y.len());
    let mut borrow = false;

    for (x_word, &y) in x.iter_mut().zip(y.iter()) {
        let mut x = *x_word;
        if !borrow {
            borrow = x < y;
        } else if x != W::ZERO {
            x = x.wrapping_sub(W::ONE);
            borrow = x < y;
        } else {
            x = x.wrapping_sub(W::ONE);
        }
        *x_word = x.wrapping_sub(y);
    }
}

fn merge_hyperloglog_bitwise<W: Word>(
    mut x: impl AsMut<[W]>,
    y: impl AsRef<[W]>,
    msb_mask: impl AsRef<[W]>,
    lsb_mask: impl AsRef<[W]>,
    acc: &mut Vec<W>,
    mask: &mut Vec<W>,
    register_size: usize,
) {
    let x = x.as_mut();
    let y = y.as_ref();
    let msb_mask = msb_mask.as_ref();
    let lsb_mask = lsb_mask.as_ref();

    debug_assert_eq!(x.len(), y.len());
    debug_assert_eq!(x.len(), msb_mask.len());
    debug_assert_eq!(x.len(), lsb_mask.len());

    let register_size_minus_1 = register_size - 1;
    let num_words_minus_1 = x.len() - 1;
    let shift_register_size_minus_1 = W::BITS - register_size_minus_1;

    acc.clear();
    mask.clear();

    /* We work in two phases. Let H_r (msb_mask) be the mask with the
     * highest bit of each register (of size r) set, and L_r (lsb_mask)
     * be the mask with the lowest bit of each register set.
     * We describe the algorithm on a single word.
     *
     * In the first phase we perform an unsigned strict register-by-register
     * comparison of x and y, using the formula
     *
     * z = ((((y | H_r) - (x & !H_r)) | (y ^ x)) ^ (y | !x)) & H_r
     *
     * Then, we generate a register-by-register mask of all ones or
     * all zeroes, depending on the result of the comparison, using the
     * formula
     *
     * (((z >> r-1 | H_r) - L_r) | H_r) ^ z
     *
     * At that point, it is trivial to select from x and y the right values.
     */

    // We load y | H_r into the accumulator.
    acc.extend(
        y.iter()
            .zip(msb_mask)
            .map(|(&y_word, &msb_word)| y_word | msb_word),
    );

    // We load x & !H_r into mask as temporary storage.
    mask.extend(
        x.iter()
            .zip(msb_mask)
            .map(|(&x_word, &msb_word)| x_word & !msb_word),
    );

    // We subtract x & !H_r, using mask as temporary storage
    subtract(acc, mask);

    // We OR with y ^ x, XOR with (y | !x), and finally AND with H_r.
    acc.iter_mut()
        .zip(x.iter())
        .zip(y.iter())
        .zip(msb_mask.iter())
        .for_each(|(((acc_word, &x_word), &y_word), &msb_word)| {
            *acc_word = ((*acc_word | (y_word ^ x_word)) ^ (y_word | !x_word)) & msb_word
        });

    // We shift by register_size - 1 places and put the result into mask.
    {
        let (mask_last, mask_slice) = mask.split_last_mut().unwrap();
        let (&msb_last, msb_slice) = msb_mask.split_last().unwrap();
        mask_slice
            .iter_mut()
            .zip(acc[0..num_words_minus_1].iter())
            .zip(acc[1..].iter())
            .zip(msb_slice.iter())
            .rev()
            .for_each(|(((mask_word, &acc_word), &next_acc_word), &msb_word)| {
                // W is always unsigned so the shift is always with a 0
                *mask_word = (acc_word >> register_size_minus_1)
                    | (next_acc_word << shift_register_size_minus_1)
                    | msb_word
            });
        *mask_last = (acc[num_words_minus_1] >> register_size_minus_1) | msb_last;
    }

    // We subtract L_r from mask.
    subtract(mask, lsb_mask);

    // We OR with H_r and XOR with the accumulator.
    mask.iter_mut()
        .zip(msb_mask.iter())
        .zip(acc.iter())
        .for_each(|((mask_word, &msb_word), &acc_word)| {
            *mask_word = (*mask_word | msb_word) ^ acc_word
        });

    // Finally, we use mask to select the right bits from x and y and store the result.
    x.iter_mut()
        .zip(y.iter())
        .zip(mask.iter())
        .for_each(|((x_word, &y_word), &mask_word)| {
            *x_word = *x_word ^ ((*x_word ^ y_word) & mask_word);
        });
}
