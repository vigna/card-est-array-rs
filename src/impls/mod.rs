/*
 * SPDX-FileCopyrightText: 2024 Matteo Dell'Acqua
 * SPDX-FileCopyrightText: 2025 Sebastiano Vigna
 *
 * SPDX-License-Identifier: Apache-2.0 OR LGPL-2.1-or-later
 */

//! Traits for counters and arrays of counters.

mod hyper_log_log;
pub use hyper_log_log::*;

mod slice_counter_array;
pub use slice_counter_array::*;

mod default_counter;
pub use default_counter::*;
