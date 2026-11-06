//! Free space on local filesystems.

use crate::Roots;
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

pub fn usage(roots: &Roots) -> Vec<FsUsage> {
    let Ok(text) = std::fs::read_to_string(roots.proc.join("self/mounts")) else { return Vec::new() };
    parse_mounts(&text)
        .into_iter()
        .filter_map(|(device, mount, fstype)| {
            let st = rustix::fs::statvfs(Path::new(&mount)).ok()?;
            let total = st.f_blocks * st.f_frsize;
            let free = st.f_bfree * st.f_frsize;
            Some(FsUsage { mount, device, fstype, total_bytes: total, used_bytes: total.saturating_sub(free) })
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
    fn statvfs_root() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("proc/self")).unwrap();
        std::fs::write(d.path().join("proc/self/mounts"), format!("/dev/x {} ext4 rw 0 0\n", d.path().display())).unwrap();
        let u = usage(&Roots::under(d.path()));
        assert_eq!(u.len(), 1);
        assert!(u[0].total_bytes > 0);
    }
}
