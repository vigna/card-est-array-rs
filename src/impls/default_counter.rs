/*
 * SPDX-FileCopyrightText: 2024 Matteo Dell'Acqua
 * SPDX-FileCopyrightText: 2025 Sebastiano Vigna
 *
 * SPDX-License-Identifier: Apache-2.0 OR LGPL-2.1-or-later
 */

use crate::traits::*;
use std::borrow::Borrow;

/// A default counter for generic [`Logic`] and backends.
pub struct DefaultEstimator<L: Logic, BL: Borrow<L>, B> {
    logic: BL,
    backend: B,
    _marker: std::marker::PhantomData<L>,
}

impl<L: Logic, BL: Borrow<L>, B> DefaultEstimator<L, BL, B> {
    /// Creates a new default counter for the specified logic and backend.
    ///
    /// # Arguments
    /// * `logic`: the estimator logic.
    /// * `backend`: the estimator's backend.
    pub fn new(logic: BL, backend: B) -> Self {
        Self {
            logic,
            backend,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<L: Logic + Clone, BL: Borrow<L>, B: AsRef<L::Backend>> AsRef<L::Backend>
    for DefaultEstimator<L, BL, B>
{
    fn as_ref(&self) -> &L::Backend {
        self.backend.as_ref()
    }
}

impl<L: Logic + Clone, BL: Borrow<L>, B: AsMut<L::Backend>> AsMut<L::Backend>
    for DefaultEstimator<L, BL, B>
{
    fn as_mut(&mut self) -> &mut L::Backend {
        self.backend.as_mut()
    }
}

impl<L: Logic + Clone, BL: Borrow<L>, B: AsRef<L::Backend>> Estimator<L>
    for DefaultEstimator<L, BL, B>
{
    type OwnedEstimator = DefaultEstimator<L, L, Box<L::Backend>>;

    fn logic(&self) -> &L {
        self.logic.borrow()
    }

    #[inline(always)]
    fn estimate(&self) -> f64 {
        self.logic.borrow().count(self.backend.as_ref())
    }
    #[inline(always)]
    fn into_owned(self) -> Self::OwnedEstimator {
        todo!()
    }
}

impl<L: Logic + Clone, BL: Borrow<L>, B: AsRef<L::Backend> + AsMut<L::Backend>> EstimatorMut<L>
    for DefaultEstimator<L, BL, B>
{
    #[inline(always)]
    fn add(&mut self, element: impl Borrow<L::Item>) {
        self.logic.borrow().add(self.backend.as_mut(), element)
    }

    #[inline(always)]
    fn clear(&mut self) {
        self.logic.borrow().clear(self.backend.as_mut())
    }

    #[inline(always)]
    fn set(&mut self, backend: &L::Backend) {
        self.logic.borrow().set(self.backend.as_mut(), backend);
    }
}

impl<L: Logic + MergeLogic + Clone, BL: Borrow<L>, B: AsRef<L::Backend> + AsMut<L::Backend>>
    MergeEstimator<L> for DefaultEstimator<L, BL, B>
{
    #[inline(always)]
    fn merge(&mut self, other: &L::Backend) {
        self.logic.borrow().merge(self.backend.as_mut(), other)
    }

    #[inline(always)]
    fn merge_with_helper(&mut self, other: &L::Backend, helper: &mut <L as MergeLogic>::Helper) {
        self.logic
            .borrow()
            .merge_with_helper(self.backend.as_mut(), other, helper)
    }
}
