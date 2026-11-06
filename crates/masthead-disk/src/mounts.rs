// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! Local filesystems to scan, from `/proc/self/mountinfo`.

use std::path::PathBuf;

/// Filesystem types that hold user data on a local disk.
const LOCAL: &[&str] = &["ext2", "ext3", "ext4", "xfs", "btrfs", "f2fs", "bcachefs", "zfs", "vfat", "exfat", "ntfs", "ntfs3", "jfs"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    pub path: PathBuf,
    pub fstype: String,
    pub device: String,
    /// `major:minor`, to skip a filesystem mounted twice.
    dev: String,
}

impl Mount {
    /// A mount for scanning a plain directory, for tests and examples.
    pub fn dir(path: PathBuf) -> Self {
        Self { path, fstype: String::new(), device: String::new(), dev: String::new() }
    }
}

/// Local filesystems, each once, at its shortest mount point.
pub fn local() -> Vec<Mount> {
    std::fs::read_to_string("/proc/self/mountinfo").map(|s| parse(&s)).unwrap_or_default()
}

fn parse(info: &str) -> Vec<Mount> {
    let mut out: Vec<Mount> = Vec::new();
    for line in info.lines() {
        let Some((left, right)) = line.split_once(" - ") else { continue };
        let l: Vec<&str> = left.split(' ').collect();
        let r: Vec<&str> = right.split(' ').collect();
        let (Some(dev), Some(root), Some(path), Some(fstype), Some(device)) = (l.get(2), l.get(3), l.get(4), r.first(), r.get(1)) else {
            continue;
        };
        // A bind mount of a subdirectory repeats space counted elsewhere.
        if *root != "/" || !LOCAL.contains(fstype) {
            continue;
        }
        let m = Mount {
            path: PathBuf::from(unescape(path)),
            fstype: (*fstype).to_owned(),
            device: (*device).to_owned(),
            dev: (*dev).to_owned(),
        };
        match out.iter_mut().find(|o| o.dev == m.dev) {
            Some(o) if m.path.as_os_str().len() < o.path.as_os_str().len() => *o = m,
            Some(_) => {}
            None => out.push(m),
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// mountinfo writes space, tab, newline and backslash as `\ooo`.
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

/// Every mount point, of any type. The walk does not descend into them, so a
/// bind mount of a folder on the same disk is not counted twice.
pub fn points() -> std::collections::HashSet<PathBuf> {
    std::fs::read_to_string("/proc/self/mountinfo")
        .map(|s| s.lines().filter_map(|l| l.split(' ').nth(4)).map(|p| PathBuf::from(unescape(p))).collect())
        .unwrap_or_default()
}

/// Bytes a filesystem reports as used.
pub fn used(path: &std::path::Path) -> Option<u64> {
    let s = rustix::fs::statvfs(path).ok()?;
    Some(s.f_blocks.saturating_sub(s.f_bfree) * s.f_frsize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_local_filesystems_once() {
        let info = "\
26 1 259:3 / / rw,relatime shared:1 - ext4 /dev/nvme1n1p3 rw
27 26 0:22 / /proc rw - proc proc rw
30 26 259:1 / /boot/efi rw - vfat /dev/nvme1n1p1 rw
31 26 259:3 /home/x/shared /srv/shared rw - ext4 /dev/nvme1n1p3 rw
32 26 7:1 / /snap/core/1 ro - squashfs /dev/loop1 ro
33 26 8:1 / /media/My\\040Disk rw - exfat /dev/sda1 rw
34 26 8:1 / /media/again/bind-of-it rw - exfat /dev/sda1 rw
";
        let m = parse(info);
        let paths: Vec<_> = m.iter().map(|m| m.path.to_string_lossy().into_owned()).collect();
        assert_eq!(paths, ["/", "/boot/efi", "/media/My Disk"]);
        assert_eq!(m[2].fstype, "exfat");
    }
}
