/*
 * SPDX-FileCopyrightText: 2024 Matteo Dell'Acqua
 * SPDX-FileCopyrightText: 2025 Sebastiano Vigna
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use super::DefaultEstimator;
use crate::traits::Word;
use crate::traits::*;
use sync_cell_slice::{SyncCell, SyncSlice};

/// An array for estimators implementing a shared [`EstimationLogic`], and whose
/// backend is a slice.
///
/// Note that we need a specific type for arrays of slice backends as one cannot
/// create a slice of slices.
pub struct SliceEstimatorArray<L, W = usize, S = Box<[W]>> {
    pub(super) logic: L,
    pub(super) backend: S,
    _marker: std::marker::PhantomData<W>,
}

/// A view of a [`SliceEstimatorArray`] as a [`SyncEstimatorArray`].
pub struct SyncSliceEstimatorArray<L, W = usize, S = Box<[SyncCell<W>]>> {
    pub(super) logic: L,
    pub(super) backend: S,
    _marker: std::marker::PhantomData<W>,
}

impl<L: SliceEstimationLogic<W> + Sync, W: Word, S: AsRef<[SyncCell<W>]> + Sync>
    SyncEstimatorArray<L> for SyncSliceEstimatorArray<L, W, S>
{
    unsafe fn set(&self, index: usize, content: &L::Backend) {
        assert_backend_len!(self.logic, content);
        let backend_len = self.logic.backend_len();
        let offset = index * backend_len;
        for (c, &b) in self.backend.as_ref()[offset..][..backend_len]
            .iter()
            .zip(content)
        {
            // SAFETY: the caller guarantees the absence of data races (see
            // the documentation of SyncEstimatorArray).
            unsafe { c.set(b) }
        }
    }

    #[inline(always)]
    fn logic(&self) -> &L {
        &self.logic
    }

    unsafe fn get(&self, index: usize, content: &mut L::Backend) {
        assert_backend_len!(self.logic, content);
        let backend_len = self.logic.backend_len();
        let offset = index * backend_len;
        for (b, c) in content
            .iter_mut()
            .zip(self.backend.as_ref()[offset..][..backend_len].iter())
        {
            // SAFETY: the caller guarantees the absence of data races (see
            // the documentation of SyncEstimatorArray).
            *b = unsafe { c.get() }
        }
    }

    unsafe fn clear(&self) {
        self.backend
            .as_ref()
            .iter()
            // SAFETY: the caller guarantees the absence of data races (see
            // the documentation of SyncEstimatorArray).
            .for_each(|c| unsafe { c.set(W::ZERO) })
    }

    #[inline(always)]
    fn len(&self) -> usize {
        self.backend.as_ref().len() / self.logic.backend_len()
    }
}

impl<L: SliceEstimationLogic<W> + Clone + Sync, W: Word, S: AsMut<[W]>> AsSyncArray<L>
    for SliceEstimatorArray<L, W, S>
{
    type SyncEstimatorArray<'a>
        = SyncSliceEstimatorArray<L, W, &'a [SyncCell<W>]>
    where
        Self: 'a;

    fn as_sync_array(&mut self) -> SyncSliceEstimatorArray<L, W, &[SyncCell<W>]> {
        SyncSliceEstimatorArray {
            logic: self.logic.clone(),
            backend: self.backend.as_mut().as_sync_slice(),
            _marker: std::marker::PhantomData,
        }
    }
}

impl<L, W, S: AsRef<[W]>> AsRef<[W]> for SliceEstimatorArray<L, W, S> {
    #[inline(always)]
    fn as_ref(&self) -> &[W] {
        self.backend.as_ref()
    }
}

impl<L, W, S: AsMut<[W]>> AsMut<[W]> for SliceEstimatorArray<L, W, S> {
    #[inline(always)]
    fn as_mut(&mut self) -> &mut [W] {
        self.backend.as_mut()
    }
}

impl<L: SliceEstimationLogic<W>, W: Word> SliceEstimatorArray<L, W, Box<[W]>> {
    /// Creates a new estimator array with the provided logic.
    ///
    /// # Arguments
    /// * `logic`: the estimation logic to use.
    /// * `len`: the number of the estimators in the array.
    pub fn new(logic: L, len: usize) -> Self {
        let num_backend_len = logic.backend_len();
        let backend = vec![W::ZERO; len * num_backend_len].into();
        Self {
            logic,
            backend,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<L: SliceEstimationLogic<W> + Clone, W: Word, S: AsRef<[W]>> EstimatorArray<L>
    for SliceEstimatorArray<L, W, S>
{
    type Estimator<'a>
        = DefaultEstimator<L, &'a L, &'a [W]>
    where
        Self: 'a;

    #[inline(always)]
    fn get_backend(&self, index: usize) -> &L::Backend {
        let offset = index * self.logic.backend_len();
        &self.backend.as_ref()[offset..][..self.logic.backend_len()]
    }

    #[inline(always)]
    fn logic(&self) -> &L {
        &self.logic
    }

    #[inline(always)]
    fn get_estimator(&self, index: usize) -> Self::Estimator<'_> {
        DefaultEstimator::new(&self.logic, self.get_backend(index))
    }

    #[inline(always)]
    fn len(&self) -> usize {
        let backend = self.backend.as_ref();
        debug_assert_eq!(backend.len() % self.logic.backend_len(), 0);
        backend.len() / self.logic.backend_len()
    }
}

impl<L: SliceEstimationLogic<W> + Clone, W: Word, S: AsRef<[W]> + AsMut<[W]>> EstimatorArrayMut<L>
    for SliceEstimatorArray<L, W, S>
{
    type EstimatorMut<'a>
        = DefaultEstimator<L, &'a L, &'a mut [W]>
    where
        Self: 'a;

    #[inline(always)]
    fn get_backend_mut(&mut self, index: usize) -> &mut L::Backend {
        let offset = index * self.logic.backend_len();
        &mut self.backend.as_mut()[offset..][..self.logic.backend_len()]
    }

    #[inline(always)]
    fn get_estimator_mut(&mut self, index: usize) -> Self::EstimatorMut<'_> {
        let logic = &self.logic;
        // We have to extract manually the backend because get_backend_mut
        // borrows self mutably, but we need to borrow just self.backend.
        let offset = index * self.logic.backend_len();
        let backend = &mut self.backend.as_mut()[offset..][..self.logic.backend_len()];

        DefaultEstimator::new(logic, backend)
    }

    #[inline(always)]
    fn clear(&mut self) {
        self.backend.as_mut().iter_mut().for_each(|v| *v = W::ZERO)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::impls::HyperLogLog8Builder;

    /// A `content` slice whose length does not match the logic must be
    /// rejected with a hard assertion; a `debug_assert` alone would let an
    /// over-long slice silently spill into the next counter in release
    /// builds.
    #[test]
    #[should_panic(expected = "does not match the expected length")]
    fn test_sync_set_wrong_length() {
        let logic = HyperLogLog8Builder::new().build::<u64>();
        let mut array = SliceEstimatorArray::new(logic, 2);
        let sync = array.as_sync_array();
        let content = vec![0u8; 17];
        // SAFETY: no other thread accesses the array.
        unsafe { sync.set(0, &content) };
    }

    /// An out-of-bounds index must panic rather than silently do nothing
    /// (`index == len()` used to be a no-op, as the unbounded `[offset..]`
    /// slice is empty there).
    #[test]
    #[should_panic]
    fn test_sync_set_index_out_of_bounds() {
        let logic = HyperLogLog8Builder::new().build::<u64>();
        let mut array = SliceEstimatorArray::new(logic, 2);
        let sync = array.as_sync_array();
        let content = vec![0u8; 16];
        // SAFETY: no other thread accesses the array.
        unsafe { sync.set(2, &content) };
    }
}
