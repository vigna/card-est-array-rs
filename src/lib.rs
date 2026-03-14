/*
 * SPDX-FileCopyrightText: 2024 Matteo Dell'Acqua
 * SPDX-FileCopyrightText: 2025 Sebastiano Vigna
 *
 * SPDX-License-Identifier: Apache-2.0 OR LGPL-2.1-or-later
 */

#![doc = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"))]

#[cfg(target_pointer_width = "16")]
type PlatformWord = u16;
#[cfg(target_pointer_width = "32")]
type PlatformWord = u32;
#[cfg(target_pointer_width = "64")]
type PlatformWord = u64;

pub mod impls;
pub mod traits;
