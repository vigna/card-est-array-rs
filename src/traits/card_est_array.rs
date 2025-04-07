/*
 * SPDX-FileCopyrightText: 2024 Matteo Dell'Acqua
 * SPDX-FileCopyrightText: 2025 Sebastiano Vigna
 *
 * SPDX-License-Identifier: Apache-2.0 OR LGPL-2.1-or-later
 */

use super::card_est::{Estimator, EstimatorMut, Logic};

/// An array of immutable estimators sharing a [`Logic`].
///
/// Arrays of counters are useful because they share the same logic, saving
/// space with respect to a slice of counters. Moreover, by hiding the
/// implementation, it is possible to create counter arrays for counters whose
/// [backends are slices](crate::impls::SliceEstimatorArray).
pub trait EstimatorArray<L: Logic + ?Sized> {
    /// The type of immutable estimator returned by
    /// [`get_counter`](EstimatorArray::get_counter).
    type Estimator<'a>: Estimator<L>
    where
        Self: 'a;

    /// Returns the logic used by the estimators in the array.
    fn logic(&self) -> &L;

    /// Returns the estimator at the specified index as an immutable estimator.
    ///
    /// Note that this method will usually require some allocation, as it needs
    /// to create a new instance of [`EstimatorArray::Estimator`].
    fn get_counter(&self, index: usize) -> Self::Estimator<'_>;

    /// Returns an immutable reference to the backend of the estimator at the
    /// specified index.
    ///
    /// This method will usually require no allocation.
    fn get_backend(&self, index: usize) -> &L::Backend;

    /// Returns the number of counters in the array.
    fn len(&self) -> usize;

    /// Returns `true` if the array contains no counters.
    #[inline(always)]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// An array of mutable estimators sharing a [`Logic`].
pub trait EstimatorArrayMut<L: Logic + ?Sized>: EstimatorArray<L> {
    /// The type of mutable estimator returned by
    /// [`get_counter_mut`](EstimatorArrayMut::get_counter_mut).
    type EstimatorMut<'a>: EstimatorMut<L>
    where
        Self: 'a;

    /// Returns the estimator at the specified index as a mutable estimator.
    ///
    /// Note that this method will usually require some allocation, as it needs
    /// to create a new instance of [`EstimatorArrayMut::EstimatorMut`].
    fn get_counter_mut(&mut self, index: usize) -> Self::EstimatorMut<'_>;

    /// Returns a mutable reference to the backend of the estimator at the
    /// specified index.
    ///
    /// This method will usually require no allocation.
    fn get_backend_mut(&mut self, index: usize) -> &mut L::Backend;

    /// Resets all counters in the array.
    fn clear(&mut self);
}

/// A trait for counter arrays that can be viewed as a [`SyncEstimatorArray`].
pub trait AsSyncArray<L: Logic + ?Sized> {
    type SyncEstimatorArray<'a>: SyncEstimatorArray<L>
    where
        Self: 'a;

    /// Converts a mutable reference to this type into a shared reference
    /// to a [`SyncEstimatorArray`].
    fn as_sync_array(&mut self) -> Self::SyncEstimatorArray<'_>;
}

/// An array of mutable estimators sharing a [`Logic`] that can be shared
/// between threads.
///
/// This trait has the same purpose of [`EstimatorArrayMut`], but can be shared
/// between threads as it implements interior mutability. It follows a logic
/// similar to a slice of
/// [`SyncCell`](https://crates.io/crates/sync_cell_slice/): it is possible to
/// get or set the backend of an estimator, but not to obtain a reference to a
/// backend.
///
/// # Safety
///
/// The methods of this trait are unsafe because multiple thread can
/// concurrently access the same counter array. The caller must ensure that
/// there are no data races.
pub trait SyncEstimatorArray<L: Logic + ?Sized>: Sync {
    /// Returns the logic used by the estimators in the array.
    fn logic(&self) -> &L;

    /// Sets the backend of the estimator at `index` to the given backend, using a
    /// shared reference to the estimator array.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the backend is correct for the logic of the
    /// counter array, and that there are no data races.
    unsafe fn set(&self, index: usize, content: &L::Backend);

    /// Copies the backend of the estimator at `index` to the given backend, using a
    /// shared reference to the estimator array.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the backend is correct for the logic of the
    /// counter array, and that there are no data races.
    unsafe fn get(&self, index: usize, content: &mut L::Backend);

    /// Clears all counters in the array.
    ///
    /// # Safety
    ///
    /// The caller must ensure that there are no data races.
    unsafe fn clear(&self);

    /// Returns the number of counters in the array.
    fn len(&self) -> usize;

    /// Returns `true` if the array contains no counters.
    #[inline(always)]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
