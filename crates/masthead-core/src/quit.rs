// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! The "Quit Masthead" marker.
//!
//! Quitting from the window writes `$XDG_RUNTIME_DIR/masthead/quit`. While it
//! exists the daemon does not start, so faces that ask for it get nothing and
//! the bar stays empty. Opening the window removes it. `XDG_RUNTIME_DIR` is
//! cleared at logout, so the next login starts Masthead as usual.

use std::path::PathBuf;

/// The marker's path, if the session has a runtime directory.
pub fn marker() -> Option<PathBuf> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").filter(|d| !d.is_empty())?;
    Some(PathBuf::from(dir).join("masthead").join("quit"))
}

/// True if the user quit Masthead in this session.
pub fn requested() -> bool {
    marker().is_some_and(|m| m.exists())
}

/// Records that the user quit.
pub fn set() -> std::io::Result<()> {
    let m = marker().ok_or_else(|| std::io::Error::other("XDG_RUNTIME_DIR is not set"))?;
    if let Some(dir) = m.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(m, b"")
}

/// Forgets a quit, so the daemon may start again.
pub fn clear() {
    if let Some(m) = marker() {
        let _ = std::fs::remove_file(m);
    }
}
