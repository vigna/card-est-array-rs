/*
 * SPDX-FileCopyrightText: 2024 Matteo Dell'Acqua
 * SPDX-FileCopyrightText: 2025 Sebastiano Vigna
 *
 * SPDX-License-Identifier: Apache-2.0 OR LGPL-2.1-or-later
 */

use std::borrow::Borrow;

/// A kind of cardinality estimator.
///
/// Implementations of this trait describe the behavior of a kind of cardinality
/// estimator. Instances contains usually parameters that further refine the
/// behavior and the precision of the estimator.
///
/// The trait contain the following items:
///
/// * Three associated types:
///     - `Item`: the type of items the estimator accepts.
///     - `Backend`: the type of the estimator backend, that is, the raw,
///       concrete representation of the estimator state.
///     - `Estimator<'a>`: the type of a estimator of this kind.
/// * A method to create a new estimator:
///   [`new_estimator`](EstimationLogic::new_estimator).
/// * A method to add elements to an estimator, given its backend:
///   [`add`](EstimationLogic::add).
/// * Methods to manipulate backends: [`estimate`](EstimationLogic::estimate),
///   [`clear`](EstimationLogic::clear), and [`set`](EstimationLogic::set).
///
/// By providing methods based on backends, an [`EstimationLogic`] can be used
/// to manipulate families of estimators with the same backend and the same
/// configuration (i.e., precision) in a controlled way, and saving space by
/// sharing common parameters. This is particularly useful to build [arrays of
/// cardinality estimators](crate::traits::EstimatorArray), which are array of
/// estimators sharing the same logic.
///
/// If you plan to use a small number of non-related estimators, we suggest you
/// [create](EstimationLogic::new_estimator) them and use their methods. More
/// complex applications, coordinating large numbers of estimators, will find
/// backed-based methods useful.
pub trait EstimationLogic {
    /// The type of items.
    type Item;
    /// The type of the backend.
    type Backend: ?Sized;
    /// The type of an estimator.
    type Estimator<'a>: EstimatorMut<Self>
    where
        Self: 'a;

    /// Adds an element to an estimator with the given backend.
    fn add(&self, backend: &mut Self::Backend, element: impl Borrow<Self::Item>);

    /// Returns an estimation of the number of distinct elements that have been
    /// added to an estimator with the given backend so far.
    fn estimate(&self, backend: &Self::Backend) -> f64;

    /// Clears a backend, making it empty.
    fn clear(&self, backend: &mut Self::Backend);

    /// Sets the contents of `dst` to the contents of `src`.
    fn set(&self, dst: &mut Self::Backend, src: &Self::Backend);

    /// Creates a new empty estimator using this logic.
    fn new_estimator(&self) -> Self::Estimator<'_>;
}

/// An extension of [`EstimationLogic`] providing methods to merge backends.
///
/// Some kind of estimators make available a *merge* operation, which,
/// given two estimators, returns an estimator with the same state
/// one would obtain by adding to an empty estimator all the elements
/// added to the two estimators, computing, in practice, a set union.
pub trait MergeEstimationLogic: EstimationLogic {
    /// The type of the helper use in merge calculations.
    ///
    /// Merge calculation might require temporary allocations. To mitigate
    /// excessive allocation, it is possible to [obtain a
    /// helper](MergeEstimationLogic::new_helper) and reusing it for several
    /// [merge operations](MergeEstimationLogic::merge_with_helper)
    type Helper;

    /// Creates a new helper to use in merge operations.
    fn new_helper(&self) -> Self::Helper;

    /// Merges `src` into `dst`.
    fn merge(&self, dst: &mut Self::Backend, src: &Self::Backend) {
        let mut helper = self.new_helper();
        self.merge_with_helper(dst, src, &mut helper);
    }

    /// Merges `src` into `dst` using the provided helper to avoid allocations.
    fn merge_with_helper(
        &self,
        dst: &mut Self::Backend,
        src: &Self::Backend,
        helper: &mut Self::Helper,
    );
}

/// Trait implemented by [estimation logics](EstimationLogic) whose backend is a
/// slice of elements of some type.
pub trait SliceEstimationLogic<T>: EstimationLogic<Backend = [T]> {
    /// The number of elements of type `T` in a backend.
    fn backend_len(&self) -> usize;
}

/// An immutable estimator.
///
/// Immutable estimators are usually immutable views over some larger structure,
/// or they contain some useful immutable state that can be reused.
///
/// An estimator must implement [`AsRef`] so to return a reference to its backend.
pub trait Estimator<L: EstimationLogic + ?Sized>: AsRef<L::Backend> {
    /// The type returned by [`Estimator::into_owned`].
    type OwnedEstimator: EstimatorMut<L>;

    /// Returns the logic of the estimator.
    fn logic(&self) -> &L;

    /// Returns an estimation of the number of distinct elements that have been
    /// added to the estimator so far.
    fn estimate(&self) -> f64;

    /// Converts this estimator into an owned version capable of mutation.
    fn into_owned(self) -> Self::OwnedEstimator;
}

/// A mutable estimator.
///
/// A mutable estimator must implement [`AsMut`] so to return a mutable
/// reference to its backend.
pub trait EstimatorMut<L: EstimationLogic + ?Sized>: Estimator<L> + AsMut<L::Backend> {
    /// Adds an element to the estimator.
    fn add(&mut self, element: impl Borrow<L::Item>);

    /// Clears the estimator, making it empty.
    fn clear(&mut self);

    /// Sets the contents of `self` to the the given backend.
    ///
    /// If you need to set to the content of another estimator, just use
    /// [`as_ref`](AsRef) on the estimator. This approach makes it
    /// possible to extract content from both owned and non-owned estimators.
    fn set(&mut self, backend: &L::Backend);
}

/// An estimator capable of merging.
pub trait MergeEstimator<L: MergeEstimationLogic + ?Sized>: EstimatorMut<L> {
    /// Merges a backend into `self`.
    ///
    /// If you need to merge with the content of another estimator, just use
    /// [`as_ref`](AsRef) on the estimator. This approach
    /// makes it possible to merge both owned and non-owned estimators.
    fn merge(&mut self, backend: &L::Backend) {
        let mut helper = self.logic().new_helper();
        self.merge_with_helper(backend, &mut helper);
    }

    /// Merges a backend into `self` using the provided helper to avoid
    /// excessive allocations.
    ///
    /// If you need to merge with the content of another estimator, just use
    /// [`as_ref`](AsRef) on the estimator. This approach makes it
    /// possible to merge both owned and non-owned estimators.
    fn merge_with_helper(&mut self, backend: &L::Backend, helper: &mut L::Helper);
}
