# Arrays of Cardinality Estimators

[![crates.io](https://img.shields.io/crates/v/card-est-array.svg)](https://crates.io/crates/card-est-array)
[![docs.rs](https://docs.rs/card-est-array/badge.svg)](https://docs.rs/card-est-array)
[![rustc](https://img.shields.io/badge/rustc-1.85+-red.svg)](https://rust-lang.github.io/rfcs/2495-min-rust-version.html)
[![CI](https://github.com/vigna/card-est-array-rs/actions/workflows/rust.yml/badge.svg)](https://github.com/vigna/card-est-array-rs/actions)
![license](https://img.shields.io/crates/l/card-est-array)
[![downloads](https://img.shields.io/crates/d/card-est-array)](https://crates.io/crates/card-est-array)
[![coveralls](https://coveralls.io/repos/github/vigna/card-est-array-rs/badge.svg?branch=main)](https://coveralls.io/github/vigna/card-est-array-rs?branch=main)

Infrastructure for managing large arrays of cardinality estimators.

## Why

A _cardinality estimator_, AKA _(probabilistic) sketch_, AKA _probabilistic
counter_ is a probabilistic data structure that has an "add" primitive similarly
to a dictionary, but does not have a corresponding "contains": it is just
possible to query the _size_ of the estimator. In other words, a cardinality
estimator remembers the number of distinct elements that have been added to it.

The returned size is only an approximation of the real size, and the precision
can be tuned, but in exchange cardinality estimators use very little space: for
example, a [HyperLogLog cardinality estimator] uses
2ᵇ⌈log log _n_⌉ bits to achieve an average relative error of 1.04/√2ᵇ (log log _n_
≤ 6 for all practical datasets).

It is common, for example, to use cardinality estimators to measure the number
of unique users in click streams. But more interesting applications use the fact
that many cardinality estimators are _[mergeable]_, which means that given two
estimators it is possible to compute (in time linear in the size of the estimators)
a new estimator containing the union of the elements that have been added to the
two original estimators.

This idea is at the core of [ANF], an
algorithm for the computation of the neighborhood function (the function telling
how many pairs of nodes are within distance _t_) that used [Flajolet–Martin
cardinality estimators (then called probabilistic counters)]. The technique
was then extended to [log-logarithmic cardinality estimators], with a
significant reduction of the memory footprint, using [broadword
programming] to merge such estimators;
it became also evident that it could be used to compute many other interesting
properties, such as the distance distribution and all centralities based on the
node neighborhood functions (the functions telling, for each node, how many
other nodes are within distance _t_). The [HyperBall algorithm],
distributed with the [WebGraph framework], is a
highly engineered implementation of these ideas. It has been used, for example,
to compute the [degrees of separation of Facebook].

The purpose of this crate is to lay the foundation of the infrastructure that is
necessary to implement HyperBall in the [Rust port of the WebGraph framework].
We provide implementation of cardinality estimators and structures handling
large arrays of estimators sharing the same parameters and logic with minimal
memory overhead. Sharing parameters is essential for scaling to billions of
estimators, and this is why a separate structure for arrays of estimators is
necessary—just putting a billion estimators in a vector would waste a large
amount of space.

For this reason, sometimes the traits give a low-level feeling. While single
estimators are encapsulated in suitable structures, arrays of estimators are
made of the concatenation of estimator backends, and each estimator is manipulated
applying suitable methods to the backend. But it is the responsibility of the
user that backends and logic are matched.

Note that this setup is not restricted to arrays, as it makes it possible to
have collections of any kind containing backends sharing the same logic (e.g.,
hash maps or sets of backends).

## Provided estimators

We provide two implementations of HyperLogLog counters (with optional
[LogLog-β correction]). The provide a [`EstimationLogic`]s that can be used to manage arrays of estimators.

- [`HyperLogLog`] is a compact, bit-based implementation;
- [`HyperLogLog8`] is a faster byte-based implementation.

## Acknowledgments

This software has been partially supported by project SERICS (PE00000014) under
the NRRP MUR program funded by the EU - NGEU, and by project ANR COREGRAPHIE,
grant ANR-20-CE23-0002 of the French Agence Nationale de la Recherche. Views and
opinions expressed are however those of the authors only and do not necessarily
reflect those of the European Union or the Italian MUR. Neither the European
Union nor the Italian MUR can be held responsible for them.

[HyperLogLog cardinality estimator]: https://algo.inria.fr/flajolet/Publications/FlFuGaMe07.pdf
[mergeable]: https://docs.rs/card-est-array/latest/card_est_array/traits/trait.MergeEstimationLogic.html
[ANF]: https://doi.org/10.1145/775047.775059
[Flajolet–Martin cardinality estimators (then called probabilistic counters)]: https://doi.org/10.1016%2F0022-0000%2885%2990041-8
[log-logarithmic cardinality estimators]: https://algo.inria.fr/flajolet/Publications/FlFuGaMe07.pdf
[broadword programming]: https://doi.org/10.1145/1963405.1963493
[HyperBall algorithm]: https://doi.ieeecomputersociety.org/10.1109/ICDMW.2013.10
[WebGraph framework]: https://webgraph.di.unimi.it/
[degrees of separation of Facebook]: https://doi.org/10.1145/2380718.2380723
[`EstimationLogic`]: https://docs.rs/card-est-array/latest/card_est_array/traits/trait.EstimationLogic.html
[`HyperLogLog`]: https://docs.rs/card-est-array/latest/card_est_array/impls/hyper_log_log/struct.HyperLogLog.html
[`HyperLogLog8`]: https://docs.rs/card-est-array/latest/card_est_array/impls/hyper_log_log8/struct.HyperLogLog8.html
[LogLog-β correction]: https://docs.rs/card_est_array/latest/impls/hyper_log_log/fn.beta_horner.html
[Rust port of the WebGraph framework]: https://crates.io/crates/webgraph
