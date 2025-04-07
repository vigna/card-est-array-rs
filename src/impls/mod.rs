/*
 * SPDX-FileCopyrightText: 2024 Matteo Dell'Acqua
 * SPDX-FileCopyrightText: 2025 Sebastiano Vigna
 *
 * SPDX-License-Identifier: Apache-2.0 OR LGPL-2.1-or-later
 */

//! Traits for estimators and arrays of estimators.

mod hyper_log_log;
pub use hyper_log_log::*;

mod slice_estimator_array;
pub use slice_estimator_array::*;

mod default_estimator;
pub use default_estimator::*;
