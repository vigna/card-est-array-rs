/*
 * SPDX-FileCopyrightText: 2024 Matteo Dell'Acqua
 * SPDX-FileCopyrightText: 2025 Sebastiano Vigna
 *
 * SPDX-License-Identifier: Apache-2.0 OR LGPL-2.1-or-later
 */

use std::borrow::Borrow;

/// A kind of counter.
///
/// Implementations of this trait describe the behavior of a kind of counter
/// (usually, some kind of probability counter, AKA cardinality estimator).
/// Instances contains usually parameters that further refine the counter
/// behavior.
///
/// The trait contain the following items:
///
/// * Three associated types:
///     - `Item`: the type of items the counter accepts.
///     - `Backend`: the type of the counter backend, that is, the raw, concrete
///       representation of the counter state.
///     - `Counter<'a>`: the type of a counter of this kind.
/// * A method to create a new counter:
///   [`new_counter`](CounterLogic::new_counter).
/// * A method to add elements to a backend: [`add`](CounterLogic::add).
/// * Methods to manipulate backends: [`count`](CounterLogic::count),
///   [`clear`](CounterLogic::clear), and [`set`](CounterLogic::set).
///
/// By providing methods based on backends, a [`CounterLogic`] can be used to
/// manipulate families of counters with the same backend and the same
/// configuration (i.e., precision) in a controlled way, and saving space by
/// sharing common parameters. This is particularly useful to build [counter
/// arrays](crate::traits::CounterArray), which are array of counters sharing
/// the same logic.
///
/// If you plan to use a small number of non-related counters, we suggest you
/// [create](CounterLogic::new_counter) them and use their methods. More complex
/// applications, coordinating large numbers of counters, will find backed-based
/// methods useful.
pub trait CounterLogic {
    /// The type of items this counter accepts.
    type Item;
    /// The type of the counter backend
    type Backend: ?Sized;
    /// The type of a counter.
    type Counter<'a>: CounterMut<Self>
    where
        Self: 'a;

    /// Adds an element to a counter with the given backend.
    fn add(&self, backend: &mut Self::Backend, element: impl Borrow<Self::Item>);

    /// Returns the count (possibly an estimation) of the number of distinct
    /// elements that have been added to a counter with the given backend so
    /// far.
    fn count(&self, backend: &Self::Backend) -> f64;

    /// Clears a backend, making it empty.
    fn clear(&self, backend: &mut Self::Backend);

    /// Sets the contents of `dst` to the contents of `src`.
    fn set(&self, dst: &mut Self::Backend, src: &Self::Backend);

    /// Creates a new empty counter with this logic.
    fn new_counter(&self) -> Self::Counter<'_>;
}

/// An extension of [`CounterLogic`] providing methods to merge backends.
///
/// Some kind of counters make available a *merge* operation, which,
/// given two counters, returns a counter with the same state
/// one would obtain by adding to an empty counter all the elements
/// added to the two counters, computing, in practice, a set union.
pub trait MergeCounterLogic: CounterLogic {
    /// The type of the helper use in merge calculations.
    ///
    /// Merge calculation might require temporary allocations. To mitigate
    /// excessive allocation, it is possible to [obtain a
    /// helper](MergeCounterLogic::new_helper) and reusing it for several [merge
    /// operations](MergeCounterLogic::merge_with_helper)
    type Helper;

    /// Creates a new helper to use in merge operations.
    fn new_helper(&self) -> Self::Helper;

    /// Merges `src` into `dst`.
    fn merge(&self, dst: &mut Self::Backend, src: &Self::Backend) {
        let mut helper = self.new_helper();
        self.merge_with_helper(dst, src, &mut helper);
    }

    /// Merges `src` into `dst` using the provided helper to avoid
    /// allocations.
    fn merge_with_helper(
        &self,
        dst: &mut Self::Backend,
        src: &Self::Backend,
        helper: &mut Self::Helper,
    );
}

/// Trait implemented by [counter logics](CounterLogic) whose backend is a slice
/// of elements of some type.
pub trait SliceCounterLogic<T>: CounterLogic<Backend = [T]> {
    /// The number of elements of type `T` in a backend.
    fn backend_len(&self) -> usize;
}

/// An immutable counter.
///
/// Immutable counters are usually immutable views over some larger structure,
/// or they contain some useful immutable state that can be reused.
///
/// A counter must implement [`AsRef`] so to return a reference to its backend.
pub trait Counter<L: CounterLogic + ?Sized>: AsRef<L::Backend> {
    /// The type returned by [`Counter::into_owned`].
    type OwnedCounter: CounterMut<L>;

    /// Returns the logic of the counter.
    fn logic(&self) -> &L;

    /// Returns the count (possibly an estimation) of the number of distinct
    /// elements that have been added to the counter so far.
    fn count(&self) -> f64;

    /// Converts this counter into an owned version capable of mutation.
    fn into_owned(self) -> Self::OwnedCounter;
}

/// A mutable counter.
///
/// A mutable counter must implement [`AsMut`] so to return a mutable reference
/// to its backend.
pub trait CounterMut<L: CounterLogic + ?Sized>: Counter<L> + AsMut<L::Backend> {
    /// Adds an element to the counter.
    fn add(&mut self, element: impl Borrow<L::Item>);

    /// Clears the counter, making it empty.
    fn clear(&mut self);

    /// Sets the contents of `self` to the the given backend.
    ///
    /// If you need to set to the content of another counter, just use
    /// [`as_ref`](AsRef) on the counter. This approach makes it
    /// possible to extract content from both owned and non-owned counters.
    fn set(&mut self, backend: &L::Backend);
}

/// A counter capable of merging.
pub trait MergeCounter<L: MergeCounterLogic + ?Sized>: CounterMut<L> {
    /// Merges a backend into `self`.
    ///
    /// If you need to merge with the content of another counter, just use
    /// [`as_ref`](AsRef) on the counter. This approach
    /// makes it possible to merge both owned and non-owned counters.
    fn merge(&mut self, backend: &L::Backend) {
        let mut helper = self.logic().new_helper();
        self.merge_with_helper(backend, &mut helper);
    }

    /// Merges a backend into `self` using the provided helper to avoid
    /// excessive allocations.
    ///
    /// If you need to merge with the content of another counter, just use
    /// [`as_ref`](AsRef) on the counter. This approach makes it
    /// possible to merge both owned and non-owned counters.
    fn merge_with_helper(&mut self, backend: &L::Backend, helper: &mut L::Helper);
}
