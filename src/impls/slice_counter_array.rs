/*
 * SPDX-FileCopyrightText: 2024 Matteo Dell'Acqua
 * SPDX-FileCopyrightText: 2025 Sebastiano Vigna
 *
 * SPDX-License-Identifier: Apache-2.0 OR LGPL-2.1-or-later
 */

use super::DefaultCounter;
use crate::traits::*;
use sux::traits::Word;
use sync_cell_slice::{SyncCell, SyncSlice};

/// An array for counters implementing a shared [`CounterLogic`], and whose
/// backend is a slice.
///
/// Note that we need a specific type for arrays of slice backends as one cannot
/// create a slice of slices.
pub struct SliceCounterArray<L, W, S> {
    pub(super) logic: L,
    pub(super) backend: S,
    _marker: std::marker::PhantomData<W>,
}

/// A view of a [`SliceCounterArray`] as a [`SyncCounterArray`].
pub struct SyncSliceCounterArray<L, W, S> {
    pub(super) logic: L,
    pub(super) backend: S,
    _marker: std::marker::PhantomData<W>,
}

unsafe impl<L, W, S> Sync for SyncSliceCounterArray<L, W, S>
where
    L: Sync,
    W: Sync,
    S: Sync,
{
}

impl<L: SliceCounterLogic<W> + Sync, W: Word, S: AsRef<[SyncCell<W>]> + Sync> SyncCounterArray<L>
    for SyncSliceCounterArray<L, W, S>
{
    unsafe fn set(&self, index: usize, content: &L::Backend) {
        debug_assert!(content.as_ref().len() == self.logic.backend_len());
        let offset = index * self.logic.backend_len();
        for (c, &b) in self.backend.as_ref()[offset..].iter().zip(content.as_ref()) {
            c.set(b)
        }
    }

    fn logic(&self) -> &L {
        &self.logic
    }

    unsafe fn get(&self, index: usize, backend: &mut L::Backend) {
        debug_assert!(backend.as_ref().len() == self.logic.backend_len());
        let offset = index * self.logic.backend_len();
        for (b, c) in backend
            .iter_mut()
            .zip(self.backend.as_ref()[offset..].iter())
        {
            *b = c.get();
        }
    }

    unsafe fn clear(&self) {
        self.backend.as_ref().iter().for_each(|c| c.set(W::ZERO))
    }

    fn len(&self) -> usize {
        self.backend.as_ref().len() / self.logic.backend_len()
    }
}

impl<L: SliceCounterLogic<W>, W, S: AsRef<[W]>> SliceCounterArray<L, W, S> {
    /// Returns the number of counters in the array.
    #[inline(always)]
    pub fn len(&self) -> usize {
        let backend = self.backend.as_ref();
        debug_assert!(backend.len() % self.logic.backend_len() == 0);
        backend.len() / self.logic.backend_len()
    }

    /// Returns `true` if the array contains no elements.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.backend.as_ref().is_empty()
    }
}

impl<L: SliceCounterLogic<W> + Clone + Sync, W: Word, S: AsMut<[W]>> AsSyncArray<L>
    for SliceCounterArray<L, W, S>
{
    type SyncCounterArray<'a>
        = SyncSliceCounterArray<L, W, &'a [SyncCell<W>]>
    where
        Self: 'a;

    fn as_sync_array(&mut self) -> SyncSliceCounterArray<L, W, &[SyncCell<W>]> {
        SyncSliceCounterArray {
            logic: self.logic.clone(),
            backend: self.backend.as_mut().as_sync_slice(),
            _marker: std::marker::PhantomData,
        }
    }
}

impl<L, W, S: AsRef<[W]>> AsRef<[W]> for SliceCounterArray<L, W, S> {
    fn as_ref(&self) -> &[W] {
        self.backend.as_ref()
    }
}

impl<L, W, S: AsMut<[W]>> AsMut<[W]> for SliceCounterArray<L, W, S> {
    fn as_mut(&mut self) -> &mut [W] {
        self.backend.as_mut()
    }
}

impl<L: SliceCounterLogic<W>, W: Word> SliceCounterArray<L, W, Box<[W]>> {
    /// Creates a new counter slice with the provided logic allocating in-memory.
    ///
    /// # Arguments
    /// * `logic`: the counter logic to use.
    /// * `len`: the number of the counters in the array.
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

impl<L: SliceCounterLogic<W> + Clone, W: Word, S: AsRef<[W]>> CounterArray<L>
    for SliceCounterArray<L, W, S>
{
    type Counter<'a>
        = DefaultCounter<L, &'a L, &'a [W]>
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
    fn get_counter(&self, index: usize) -> Self::Counter<'_> {
        DefaultCounter::new(&self.logic, self.get_backend(index))
    }

    #[inline(always)]
    fn len(&self) -> usize {
        self.len()
    }
}

impl<L: SliceCounterLogic<W> + Clone, W: Word, S: AsRef<[W]> + AsMut<[W]>> CounterArrayMut<L>
    for SliceCounterArray<L, W, S>
{
    type CounterMut<'a>
        = DefaultCounter<L, &'a L, &'a mut [W]>
    where
        Self: 'a;

    #[inline(always)]
    fn get_backend_mut(&mut self, index: usize) -> &mut L::Backend {
        let offset = index * self.logic.backend_len();
        &mut self.backend.as_mut()[offset..][..self.logic.backend_len()]
    }

    #[inline(always)]
    fn get_counter_mut(&mut self, index: usize) -> Self::CounterMut<'_> {
        let logic = &self.logic;
        // We have to extract manually the backend because get_backend_mut
        // borrows self mutably, but we need to borrow just self.backend.
        let offset = index * self.logic.backend_len();
        let backend = &mut self.backend.as_mut()[offset..][..self.logic.backend_len()];

        DefaultCounter::new(logic, backend)
    }

    #[inline(always)]
    fn clear(&mut self) {
        self.backend.as_mut().iter_mut().for_each(|v| *v = W::ZERO)
    }
}
