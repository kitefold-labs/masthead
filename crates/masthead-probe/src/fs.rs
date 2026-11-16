// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! Free space on local filesystems.

use crate::Roots;
use crate::source::{Buf, Source};
use std::path::Path;

/// Filesystem types we report. Network filesystems are left out on purpose:
/// `statvfs` on a hung NFS or SMB mount blocks the calling thread.
const LOCAL: &[&str] =
    &["ext2", "ext3", "ext4", "btrfs", "xfs", "f2fs", "vfat", "exfat", "ntfs3", "ntfs", "zfs", "bcachefs", "jfs", "reiserfs"];

#[derive(Debug, Clone, PartialEq)]
pub struct FsUsage {
    pub mount: String,
    pub device: String,
    pub fstype: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
}

/// Unescape the octal sequences `/proc/self/mounts` uses for spaces and tabs.
fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 3 < b.len() && b[i + 1..i + 4].iter().all(|c| (b'0'..=b'7').contains(c)) {
            out.push((b[i + 1] - b'0') * 64 + (b[i + 2] - b'0') * 8 + (b[i + 3] - b'0'));
            i += 4;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Parse a mounts table into (device, mount point, type), one entry per device.
pub fn parse_mounts(text: &str) -> Vec<(String, String, String)> {
    let mut out: Vec<(String, String, String)> = Vec::new();
    for line in text.lines() {
        let mut f = line.split_ascii_whitespace();
        let (Some(dev), Some(mnt), Some(ty)) = (f.next(), f.next(), f.next()) else { continue };
        if !LOCAL.contains(&ty) {
            continue;
        }
        let mnt = unescape(mnt);
        // btrfs subvolumes and bind mounts repeat the device; keep the shortest mount point.
        match out.iter_mut().find(|e| e.0 == dev) {
            Some(e) if mnt.len() < e.1.len() => e.1 = mnt,
            Some(_) => {}
            None => out.push((dev.to_owned(), mnt, ty.to_owned())),
        }
    }
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

/// Total and used bytes of the filesystem holding `path`. One `statvfs`, so
/// call it only on local mounts.
pub fn space(path: &Path) -> Option<(u64, u64)> {
    let st = rustix::fs::statvfs(path).ok()?;
    let total = st.f_blocks * st.f_frsize;
    let free = st.f_bfree * st.f_frsize;
    Some((total, total.saturating_sub(free)))
}

/// Whether an escaped mount point from a mounts table names `want`.
fn mount_is(escaped: &[u8], want: &[u8]) -> bool {
    let (mut i, mut j) = (0, 0);
    while i < escaped.len() {
        let octal = escaped[i] == b'\\' && escaped.len() >= i + 4 && escaped[i + 1..i + 4].iter().all(|c| (b'0'..=b'7').contains(c));
        let c = if octal {
            i += 4;
            (escaped[i - 3] - b'0') * 64 + (escaped[i - 2] - b'0') * 8 + (escaped[i - 1] - b'0')
        } else {
            i += 1;
            escaped[i - 1]
        };
        if want.get(j) != Some(&c) {
            return false;
        }
        j += 1;
    }
    j == want.len()
}

/// Whether a mounts table has `mount` as the mount point of a local filesystem.
pub fn is_local_mount(table: &[u8], mount: &str) -> bool {
    table.split(|b| *b == b'\n').any(|line| {
        let mut f = line.split(u8::is_ascii_whitespace).filter(|s| !s.is_empty());
        let (Some(_), Some(mnt), Some(ty)) = (f.next(), f.next(), f.next()) else { return false };
        LOCAL.iter().any(|l| l.as_bytes() == ty) && mount_is(mnt, mount.as_bytes())
    })
}

/// How many samples [`MountSpace`] trusts a mount check before reading the
/// mounts table again.
const RECHECK_SAMPLES: u32 = 30;

/// Space used on one chosen mount point, for the bar. It reads the mounts
/// table when the mount point changes and every 30 samples, and calls
/// `statvfs` only while the path is a local mount: an unmounted path would
/// report the filesystem under it, and a network mount can block.
#[derive(Debug)]
pub struct MountSpace {
    mounts: Option<Source>,
    buf: Buf,
    mount: String,
    local: bool,
    /// Samples left before the next mount check; 0 checks on the next sample.
    fresh: u32,
}

impl MountSpace {
    pub fn open(roots: &Roots) -> Self {
        Self {
            mounts: Source::open(&roots.proc.join("self/mounts")).ok(),
            buf: Buf::with_capacity(4096),
            mount: String::new(),
            local: false,
            fresh: 0,
        }
    }

    /// Total and used bytes on `mount`, or `None` when it is not a local mount.
    pub fn sample(&mut self, mount: &str) -> Option<(u64, u64)> {
        if self.fresh == 0 || self.mount != mount {
            mount.clone_into(&mut self.mount);
            self.local = self.mounts.as_ref().and_then(|s| s.read(&mut self.buf).ok()).is_some_and(|t| is_local_mount(t, mount));
            self.fresh = RECHECK_SAMPLES;
        }
        self.fresh -= 1;
        if self.local { space(Path::new(&self.mount)) } else { None }
    }

    /// Check the mounts table again on the next sample, after a block device change.
    pub fn recheck(&mut self) {
        self.fresh = 0;
    }
}

pub fn usage(roots: &Roots) -> Vec<FsUsage> {
    let Ok(text) = std::fs::read_to_string(roots.proc.join("self/mounts")) else { return Vec::new() };
    parse_mounts(&text)
        .into_iter()
        .filter_map(|(device, mount, fstype)| {
            let (total, used) = space(Path::new(&mount))?;
            Some(FsUsage { mount, device, fstype, total_bytes: total, used_bytes: used })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mounts() {
        let m = parse_mounts(
            "/dev/nvme0n1p2 / ext4 rw 0 0\nproc /proc proc rw 0 0\n/dev/nvme1n1p1 /home btrfs rw 0 0\n\
/dev/nvme1n1p1 /home/x\\040y btrfs rw 0 0\nserver:/x /mnt/nfs nfs4 rw 0 0\n",
        );
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].1, "/");
        assert_eq!(m[1].1, "/home");
        assert_eq!(unescape("/a\\040b"), "/a b");
        assert_eq!(unescape("/a\\04"), "/a\\04");
    }

    #[test]
    fn local_mounts() {
        let t = b"/dev/nvme0n1p2 / ext4 rw 0 0\n/dev/sda1 /mnt/my\\040data xfs rw 0 0\nserver:/x /mnt/nfs nfs4 rw 0 0\n";
        assert!(is_local_mount(t, "/"));
        assert!(is_local_mount(t, "/mnt/my data"));
        assert!(!is_local_mount(t, "/mnt/my"));
        assert!(!is_local_mount(t, "/mnt/my data2"));
        assert!(!is_local_mount(t, "/mnt/nfs"), "network filesystems can block statvfs");
        assert!(!is_local_mount(t, "/home"));
        assert!(mount_is(b"/a\\04", b"/a\\04"));
    }

    #[test]
    fn mount_space() {
        let d = tempfile::tempdir().unwrap();
        let mounts = d.path().join("proc/self/mounts");
        std::fs::create_dir_all(mounts.parent().unwrap()).unwrap();
        let other = d.path().join("data");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(&mounts, format!("/dev/x {} ext4 rw 0 0\n", d.path().display())).unwrap();
        let mut m = MountSpace::open(&Roots::under(d.path()));
        let root = d.path().to_str().unwrap();
        assert!(m.sample(root).is_some_and(|(total, used)| total > 0 && used <= total));
        assert_eq!(m.sample(other.to_str().unwrap()), None, "not a mount point");
        // Unmounted: the next check notices.
        std::fs::write(&mounts, "").unwrap();
        assert!(m.sample(root).is_none(), "a changed mount point is checked at once");
        std::fs::write(&mounts, format!("/dev/x {} ext4 rw 0 0\n", d.path().display())).unwrap();
        assert!(m.sample(root).is_none(), "trusted until the next check");
        m.recheck();
        assert!(m.sample(root).is_some());
    }

    #[test]
    fn statvfs_root() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("proc/self")).unwrap();
        std::fs::write(d.path().join("proc/self/mounts"), format!("/dev/x {} ext4 rw 0 0\n", d.path().display())).unwrap();
        let u = usage(&Roots::under(d.path()));
        assert_eq!(u.len(), 1);
        assert!(u[0].total_bytes > 0);
    }
}
