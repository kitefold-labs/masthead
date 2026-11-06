// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! Disk throughput from `/proc/diskstats`, whole physical devices only.

use crate::parse::{fields, lines, u64_of};
use crate::source::{Buf, Source};
use crate::{Roots, read_attr};
use std::io;

#[derive(Debug, Clone)]
pub struct Disk {
    pub name: String,
    pub model: Option<String>,
    pub size_bytes: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub read_rate: f64,
    pub write_rate: f64,
    primed: bool,
    seen: bool,
}

/// Block devices under `/sys/block` with a `device` link. That excludes loop,
/// ram, zram, dm-* and md* devices, which would double-count their members.
pub fn discover(roots: &Roots) -> Vec<Disk> {
    let dir = roots.sys.join("block");
    let Ok(rd) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut out: Vec<Disk> = rd
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let p = dir.join(&name);
            if !p.join("device").exists() {
                return None;
            }
            let sectors = read_attr(&p.join("size")).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
            if sectors == 0 {
                return None; // empty card reader slots, ejected media
            }
            Some(Disk {
                model: read_attr(&p.join("device/model")),
                size_bytes: sectors * 512,
                name,
                read_bytes: 0,
                write_bytes: 0,
                read_rate: 0.0,
                write_rate: 0.0,
                primed: false,
                seen: false,
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DiskRates {
    pub read: f64,
    pub write: f64,
}

#[derive(Debug)]
pub struct Disks {
    src: Source,
    buf: Buf,
    pub disks: Vec<Disk>,
}

impl Disks {
    pub fn open(roots: &Roots) -> io::Result<Self> {
        Ok(Self { src: Source::open(&roots.proc.join("diskstats"))?, buf: Buf::with_capacity(16384), disks: discover(roots) })
    }

    pub fn rediscover(&mut self, roots: &Roots) {
        let mut fresh = discover(roots);
        for f in &mut fresh {
            if let Some(old) = self.disks.iter().find(|o| o.name == f.name) {
                f.read_bytes = old.read_bytes;
                f.write_bytes = old.write_bytes;
                f.primed = old.primed;
            }
        }
        self.disks = fresh;
    }

    pub fn sample(&mut self, dt_secs: f64) -> io::Result<DiskRates> {
        let bytes = self.src.read(&mut self.buf)?;
        for d in &mut self.disks {
            d.seen = false;
        }
        for line in lines(bytes) {
            // major minor name reads merged sectors_read ms writes merged sectors_written ...
            let mut f = fields(line);
            let Some(name) = f.nth(2) else { continue };
            let Some(d) = self.disks.iter_mut().find(|d| d.name.as_bytes() == name) else { continue };
            let sr = f.nth(2).and_then(u64_of);
            let sw = f.nth(3).and_then(u64_of);
            let (Some(sr), Some(sw)) = (sr, sw) else { continue };
            // diskstats sectors are always 512 bytes, whatever the device's block size.
            let (r, w) = (sr * 512, sw * 512);
            if d.primed && dt_secs > 0.0 {
                d.read_rate = r.checked_sub(d.read_bytes).map_or(0.0, |x| x as f64 / dt_secs);
                d.write_rate = w.checked_sub(d.write_bytes).map_or(0.0, |x| x as f64 / dt_secs);
            }
            d.read_bytes = r;
            d.write_bytes = w;
            d.primed = true;
            d.seen = true;
        }
        let mut t = DiskRates::default();
        for d in &mut self.disks {
            if !d.seen {
                d.read_rate = 0.0;
                d.write_rate = 0.0;
            }
            t.read += d.read_rate;
            t.write += d.write_rate;
        }
        Ok(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_devices_only() {
        let d = tempfile::tempdir().unwrap();
        let blk = d.path().join("sys/block");
        for n in ["nvme0n1", "loop0", "dm-0"] {
            std::fs::create_dir_all(blk.join(n)).unwrap();
            std::fs::write(blk.join(n).join("size"), "7814037168\n").unwrap();
        }
        std::fs::create_dir_all(blk.join("nvme0n1/device")).unwrap();
        std::fs::write(blk.join("nvme0n1/device/model"), "Corsair MP700 PRO SE   \n").unwrap();
        std::fs::create_dir_all(d.path().join("proc")).unwrap();
        let ds = d.path().join("proc/diskstats");
        std::fs::write(
            &ds,
            " 259 0 nvme0n1 10 0 100 5 20 0 200 7 0 0 0\n 259 1 nvme0n1p1 1 0 50 1 1 0 50 1 0 0 0\n 7 0 loop0 1 0 999 0 0 0 0 0 0 0 0\n",
        )
        .unwrap();
        let roots = Roots::under(d.path());
        let mut disks = Disks::open(&roots).unwrap();
        assert_eq!(disks.disks.len(), 1);
        assert_eq!(disks.disks[0].model.as_deref(), Some("Corsair MP700 PRO SE"));
        assert_eq!(disks.disks[0].size_bytes, 7_814_037_168 * 512);
        disks.sample(1.0).unwrap();
        std::fs::write(&ds, " 259 0 nvme0n1 10 0 300 5 20 0 1200 7 0 0 0\n").unwrap();
        let r = disks.sample(1.0).unwrap();
        assert!((r.read - 200.0 * 512.0).abs() < 1e-9);
        assert!((r.write - 1000.0 * 512.0).abs() < 1e-9);
        disks.rediscover(&roots);
        assert_eq!(disks.disks[0].read_bytes, 300 * 512);
    }
}
