//! Reload `config.toml` when it changes on disk.

use rustix::fd::OwnedFd;
use rustix::fs::inotify::{self, CreateFlags, WatchFlags};
use std::path::Path;
use tokio::io::unix::AsyncFd;

/// Watch the config directory and its parent (so creating the directory is
/// noticed too). Returns an fd that becomes readable on any change.
pub fn watch(config_path: &Path) -> std::io::Result<AsyncFd<OwnedFd>> {
    let fd = inotify::init(CreateFlags::CLOEXEC | CreateFlags::NONBLOCK)?;
    let flags = WatchFlags::CLOSE_WRITE | WatchFlags::MOVED_TO | WatchFlags::CREATE | WatchFlags::DELETE | WatchFlags::MOVED_FROM;
    if let Some(dir) = config_path.parent() {
        let _ = inotify::add_watch(&fd, dir, flags);
        if let Some(parent) = dir.parent() {
            let _ = inotify::add_watch(&fd, parent, WatchFlags::CREATE | WatchFlags::MOVED_TO);
        }
    }
    register(fd)
}

/// Register an fd-owning value with the reactor.
///
/// Callers pass `OwnedFd` or `UeventSocket`, which owns its `OwnedFd`: the
/// descriptor stays open and unchanged until the value is dropped.
#[allow(unsafe_code)]
pub fn register<T: std::os::fd::AsRawFd>(inner: T) -> std::io::Result<AsyncFd<T>> {
    // SAFETY: `inner` owns its descriptor (see above), so it remains open and
    // refers to the same file description for the AsyncFd's whole lifetime.
    unsafe { AsyncFd::register(inner) }.map_err(std::io::Error::from)
}

/// Discard pending events.
pub fn drain(fd: &OwnedFd) {
    let mut buf = [0u8; 4096];
    while matches!(rustix::io::read(fd, &mut buf), Ok(n) if n > 0) {}
}

/// Re-add the directory watch, for when the directory was created after startup.
pub fn rewatch(fd: &OwnedFd, config_path: &Path) {
    if let Some(dir) = config_path.parent() {
        let flags = WatchFlags::CLOSE_WRITE | WatchFlags::MOVED_TO | WatchFlags::CREATE | WatchFlags::DELETE | WatchFlags::MOVED_FROM;
        let _ = inotify::add_watch(fd, dir, flags);
    }
}
