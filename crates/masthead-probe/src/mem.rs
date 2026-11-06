//! Memory from `/proc/meminfo`.

use crate::Roots;
use crate::parse::{fields, lines, u64_of};
use crate::source::{Buf, Source};
use std::io;

/// Memory figures in bytes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MemInfo {
    pub total: u64,
    pub available: u64,
    pub free: u64,
    pub buffers: u64,
    pub cached: u64,
    pub sreclaimable: u64,
    pub shmem: u64,
    pub swap_total: u64,
    pub swap_free: u64,
}

impl MemInfo {
    /// `MemTotal − MemAvailable`, the figure GNOME System Monitor reports.
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.available)
    }

    /// Page cache, buffers and reclaimable slab.
    pub fn cache(&self) -> u64 {
        (self.cached + self.buffers + self.sreclaimable).saturating_sub(self.shmem)
    }

    pub fn swap_used(&self) -> u64 {
        self.swap_total.saturating_sub(self.swap_free)
    }
}

pub fn parse_meminfo(buf: &[u8]) -> MemInfo {
    let mut m = MemInfo::default();
    for line in lines(buf) {
        let Some(colon) = line.iter().position(|b| *b == b':') else { continue };
        let key = &line[..colon];
        let kb = fields(&line[colon + 1..]).next().and_then(u64_of).unwrap_or(0);
        let v = kb * 1024;
        match key {
            b"MemTotal" => m.total = v,
            b"MemAvailable" => m.available = v,
            b"MemFree" => m.free = v,
            b"Buffers" => m.buffers = v,
            b"Cached" => m.cached = v,
            b"SReclaimable" => m.sreclaimable = v,
            b"Shmem" => m.shmem = v,
            b"SwapTotal" => m.swap_total = v,
            b"SwapFree" => m.swap_free = v,
            _ => {}
        }
    }
    m
}

#[derive(Debug)]
pub struct Mem {
    src: Source,
    buf: Buf,
}

impl Mem {
    pub fn open(roots: &Roots) -> io::Result<Self> {
        Ok(Self { src: Source::open(&roots.proc.join("meminfo"))?, buf: Buf::with_capacity(4096) })
    }

    pub fn sample(&mut self) -> io::Result<MemInfo> {
        Ok(parse_meminfo(self.src.read(&mut self.buf)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses() {
        let m = parse_meminfo(
            b"MemTotal:       62500000 kB\nMemFree:         4600000 kB\nMemAvailable:   41000000 kB\n\
Buffers:          100000 kB\nCached:         30000000 kB\nSwapCached: 0 kB\nShmem: 500000 kB\n\
SReclaimable:    2000000 kB\nSwapTotal:       8388604 kB\nSwapFree:        8388604 kB\n",
        );
        assert_eq!(m.total, 62_500_000 * 1024);
        assert_eq!(m.used(), (62_500_000 - 41_000_000) * 1024);
        assert_eq!(m.cache(), (30_000_000 + 100_000 + 2_000_000 - 500_000) * 1024);
        assert_eq!(m.swap_used(), 0);
    }
}
