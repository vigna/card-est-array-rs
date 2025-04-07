/*
 * SPDX-FileCopyrightText: 2024 Matteo Dell'Acqua
 * SPDX-FileCopyrightText: 2025 Sebastiano Vigna
 *
 * SPDX-License-Identifier: Apache-2.0 OR LGPL-2.1-or-later
 */

//! Traits for counters and arrays of counters.

mod card_est;
pub use card_est::*;
mod card_est_array;
pub use card_est_array::*;
