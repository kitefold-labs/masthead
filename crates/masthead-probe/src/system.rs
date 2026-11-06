// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! Load average and uptime.

use crate::Roots;
use crate::parse::{f64_of, fields, u64_of};
use crate::source::{Buf, Source};
use std::io;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LoadAvg {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
    pub running: u32,
    pub total: u32,
}

pub fn parse_loadavg(buf: &[u8]) -> LoadAvg {
    let mut f = fields(buf);
    let one = f.next().and_then(f64_of).unwrap_or(0.0);
    let five = f.next().and_then(f64_of).unwrap_or(0.0);
    let fifteen = f.next().and_then(f64_of).unwrap_or(0.0);
    let (running, total) = f
        .next()
        .and_then(|rt| {
            let slash = rt.iter().position(|b| *b == b'/')?;
            Some((u64_of(&rt[..slash])? as u32, u64_of(&rt[slash + 1..])? as u32))
        })
        .unwrap_or((0, 0));
    LoadAvg { one, five, fifteen, running, total }
}

#[derive(Debug)]
pub struct System {
    loadavg: Source,
    uptime: Source,
    buf: Buf,
}

impl System {
    pub fn open(roots: &Roots) -> io::Result<Self> {
        Ok(Self {
            loadavg: Source::open(&roots.proc.join("loadavg"))?,
            uptime: Source::open(&roots.proc.join("uptime"))?,
            buf: Buf::with_capacity(128),
        })
    }

    pub fn loadavg(&mut self) -> io::Result<LoadAvg> {
        Ok(parse_loadavg(self.loadavg.read(&mut self.buf)?))
    }

    /// Seconds since boot.
    pub fn uptime(&mut self) -> io::Result<f64> {
        let b = self.uptime.read(&mut self.buf)?;
        Ok(fields(b).next().and_then(f64_of).unwrap_or(0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses() {
        let l = parse_loadavg(b"4.12 3.80 3.55 3/1201 99999\n");
        assert!((l.one - 4.12).abs() < 1e-9);
        assert!((l.fifteen - 3.55).abs() < 1e-9);
        assert_eq!((l.running, l.total), (3, 1201));
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("proc")).unwrap();
        std::fs::write(d.path().join("proc/loadavg"), "0.50 0.40 0.30 1/300 7\n").unwrap();
        std::fs::write(d.path().join("proc/uptime"), "12345.67 99999.00\n").unwrap();
        let mut s = System::open(&Roots::under(d.path())).unwrap();
        assert!((s.uptime().unwrap() - 12345.67).abs() < 1e-9);
        assert_eq!(s.loadavg().unwrap().total, 300);
    }
}
