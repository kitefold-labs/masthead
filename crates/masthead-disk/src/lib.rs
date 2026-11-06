// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! Disk usage for Masthead's window: a parallel scan of every local
//! filesystem into a size tree small enough to keep in memory, plus finders
//! for space that can be reclaimed (Docker, build outputs, caches, Trash, old
//! downloads) and the actions that reclaim it.
//!
//! Nothing here needs privileges. Directories the user cannot read are
//! reported as unscanned space rather than read through a password prompt.

pub mod child;
pub mod clean;
pub mod docker;
pub mod findings;
pub mod mounts;
pub mod scan;
pub mod time;
pub mod tree;

pub use scan::{Progress, Scan, scan};
pub use tree::{Kind, Node};
