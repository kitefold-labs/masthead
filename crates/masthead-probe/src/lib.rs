//! Readers for the Linux kernel statistics Masthead displays.
//!
//! Every reader keeps its file descriptors open and re-reads them with
//! `pread(fd, buf, 0)`, so one sample costs one syscall per source. Parsers
//! work on byte slices and write into preallocated structures; after the first
//! sample a steady-state tick performs no heap allocation (see
//! `tests/no_alloc.rs`).
//!
//! Discovery functions take a root directory so tests can point them at a
//! fixture tree instead of the real `/proc` and `/sys`.

pub mod cpu;
pub mod disk;
pub mod fs;
pub mod gpu;
pub mod hwmon;
pub mod mem;
pub mod net;
pub mod parse;
pub mod power;
pub mod procs;
pub mod source;
pub mod system;
pub mod uevent;

use std::path::{Path, PathBuf};

/// Where to find procfs and sysfs. Production uses `/proc` and `/sys`.
#[derive(Debug, Clone)]
pub struct Roots {
    pub proc: PathBuf,
    pub sys: PathBuf,
}

impl Default for Roots {
    fn default() -> Self {
        Self { proc: PathBuf::from("/proc"), sys: PathBuf::from("/sys") }
    }
}

impl Roots {
    /// Roots for a fixture tree containing `proc/` and `sys/` subdirectories.
    pub fn under(base: &Path) -> Self {
        Self { proc: base.join("proc"), sys: base.join("sys") }
    }
}

/// Read a small sysfs attribute as a trimmed string. Discovery-time only.
pub(crate) fn read_attr(path: &Path) -> Option<String> {
    let s = std::fs::read_to_string(path).ok()?;
    let t = s.trim();
    if t.is_empty() { None } else { Some(t.to_owned()) }
}

/// The final path component of a symlink target, e.g. the driver name behind
/// `device/driver`. Discovery-time only.
pub(crate) fn link_name(path: &Path) -> Option<String> {
    let target = std::fs::read_link(path).ok()?;
    target.file_name().map(|n| n.to_string_lossy().into_owned())
}
