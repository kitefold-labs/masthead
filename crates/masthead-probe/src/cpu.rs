//! CPU load from `/proc/stat` and per-core frequency from cpufreq.

use crate::Roots;
use crate::parse::{fields, lines, u64_of};
use crate::source::{Buf, Source};
use std::io;

/// Cumulative CPU time in clock ticks, as reported by one `cpu` line.
/// `guest` and `guest_nice` are already included in `user` and `nice`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CpuTimes {
    pub user: u64,
    pub nice: u64,
    pub system: u64,
    pub idle: u64,
    pub iowait: u64,
    pub irq: u64,
    pub softirq: u64,
    pub steal: u64,
}

impl CpuTimes {
    pub fn total(&self) -> u64 {
        self.user + self.nice + self.system + self.idle + self.iowait + self.irq + self.softirq + self.steal
    }

    pub fn idle_all(&self) -> u64 {
        self.idle + self.iowait
    }

    fn parse(rest: &[u8]) -> Self {
        let mut it = fields(rest).map(u64_of);
        let mut next = || it.next().flatten().unwrap_or(0);
        Self { user: next(), nice: next(), system: next(), idle: next(), iowait: next(), irq: next(), softirq: next(), steal: next() }
    }
}

/// Busy share (0.0–1.0) between two samples, or `None` if no time passed.
/// Counters that went backwards (CPU hotplug) count as no time passed.
pub fn busy_fraction(prev: &CpuTimes, cur: &CpuTimes) -> Option<f64> {
    let dt = cur.total().checked_sub(prev.total())?;
    if dt == 0 {
        return None;
    }
    let di = cur.idle_all().saturating_sub(prev.idle_all()).min(dt);
    Some(1.0 - di as f64 / dt as f64)
}

/// One parse of `/proc/stat`.
#[derive(Debug, Clone, Default)]
pub struct StatSnapshot {
    pub total: CpuTimes,
    /// Indexed by CPU number. Offline CPUs keep their last values.
    pub cores: Vec<CpuTimes>,
    pub procs_running: u32,
    pub procs_blocked: u32,
}

/// Parse `/proc/stat` into `out`, reusing its storage.
pub fn parse_stat(buf: &[u8], out: &mut StatSnapshot) {
    for line in lines(buf) {
        if let Some(rest) = line.strip_prefix(b"cpu") {
            let (num, tail) = match rest.iter().position(u8::is_ascii_whitespace) {
                Some(p) => (&rest[..p], &rest[p..]),
                None => continue,
            };
            let t = CpuTimes::parse(tail);
            if num.is_empty() {
                out.total = t;
            } else if let Some(i) = u64_of(num).and_then(|n| usize::try_from(n).ok()) {
                if i >= out.cores.len() {
                    // Grows only when a CPU appears; steady state never lands here.
                    out.cores.resize(i + 1, CpuTimes::default());
                }
                out.cores[i] = t;
            }
        } else if let Some(rest) = line.strip_prefix(b"procs_running ") {
            out.procs_running = u64_of(rest.trim_ascii()).unwrap_or(0) as u32;
        } else if let Some(rest) = line.strip_prefix(b"procs_blocked ") {
            out.procs_blocked = u64_of(rest.trim_ascii()).unwrap_or(0) as u32;
        }
    }
}

/// Reader for `/proc/stat`.
#[derive(Debug)]
pub struct CpuStat {
    src: Source,
    buf: Buf,
}

impl CpuStat {
    pub fn open(roots: &Roots) -> io::Result<Self> {
        Ok(Self { src: Source::open(&roots.proc.join("stat"))?, buf: Buf::with_capacity(8192) })
    }

    pub fn sample(&mut self, out: &mut StatSnapshot) -> io::Result<()> {
        let bytes = self.src.read(&mut self.buf)?;
        parse_stat(bytes, out);
        Ok(())
    }
}

/// The CPU model name from `/proc/cpuinfo`. Discovery-time only.
pub fn model_name(roots: &Roots) -> Option<String> {
    let s = std::fs::read_to_string(roots.proc.join("cpuinfo")).ok()?;
    s.lines().find_map(|l| l.strip_prefix("model name").and_then(|r| r.split_once(':')).map(|(_, v)| v.trim().to_owned()))
}

/// Physical core count: distinct `(physical id, core id)` pairs in cpuinfo.
pub fn physical_cores(roots: &Roots) -> Option<usize> {
    let s = std::fs::read_to_string(roots.proc.join("cpuinfo")).ok()?;
    let mut seen = std::collections::BTreeSet::new();
    let (mut phys, mut core) = (None, None);
    for l in s.lines() {
        if let Some((k, v)) = l.split_once(':') {
            match k.trim() {
                "physical id" => phys = v.trim().parse::<u32>().ok(),
                "core id" => core = v.trim().parse::<u32>().ok(),
                _ => {}
            }
        } else if l.trim().is_empty()
            && let (Some(p), Some(c)) = (phys.take(), core.take())
        {
            seen.insert((p, c));
        }
    }
    if let (Some(p), Some(c)) = (phys, core) {
        seen.insert((p, c));
    }
    if seen.is_empty() { None } else { Some(seen.len()) }
}

/// Per-core current frequency in kHz from `scaling_cur_freq`.
#[derive(Debug)]
pub struct CpuFreq {
    srcs: Vec<Option<Source>>,
    buf: Buf,
}

impl CpuFreq {
    pub fn open(roots: &Roots, cores: usize) -> Self {
        let srcs =
            (0..cores).map(|i| Source::open(&roots.sys.join(format!("devices/system/cpu/cpu{i}/cpufreq/scaling_cur_freq"))).ok()).collect();
        Self { srcs, buf: Buf::with_capacity(32) }
    }

    /// Fill `out_mhz` (one entry per core, 0 when unavailable).
    pub fn sample(&mut self, out_mhz: &mut Vec<f64>) {
        out_mhz.resize(self.srcs.len(), 0.0);
        for (i, s) in self.srcs.iter().enumerate() {
            out_mhz[i] = s.as_ref().and_then(|s| s.read_i64(&mut self.buf)).map_or(0.0, |khz| khz as f64 / 1000.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAT: &[u8] = b"cpu  100 0 50 800 50 0 0 0 0 0\n\
cpu0 50 0 25 400 25 0 0 0 0 0\n\
cpu1 50 0 25 400 25 0 0 0 0 0\n\
intr 1 2 3\nctxt 99\nprocs_running 3\nprocs_blocked 1\n";

    #[test]
    fn parses_stat() {
        let mut s = StatSnapshot::default();
        parse_stat(STAT, &mut s);
        assert_eq!(s.total.user, 100);
        assert_eq!(s.total.total(), 1000);
        assert_eq!(s.cores.len(), 2);
        assert_eq!(s.cores[1].idle, 400);
        assert_eq!(s.procs_running, 3);
        assert_eq!(s.procs_blocked, 1);
    }

    #[test]
    fn busy() {
        let a = CpuTimes { user: 100, idle: 900, ..Default::default() };
        let b = CpuTimes { user: 150, idle: 950, ..Default::default() };
        assert!((busy_fraction(&a, &b).unwrap() - 0.5).abs() < 1e-9);
        assert_eq!(busy_fraction(&a, &a), None);
        assert_eq!(busy_fraction(&b, &a), None);
    }

    #[test]
    fn cpuinfo() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("proc")).unwrap();
        std::fs::write(
            dir.path().join("proc/cpuinfo"),
            "processor\t: 0\nmodel name\t: AMD Ryzen 9 9950X3D 16-Core Processor\nphysical id\t: 0\ncore id\t: 0\n\n\
processor\t: 1\nmodel name\t: AMD Ryzen 9 9950X3D 16-Core Processor\nphysical id\t: 0\ncore id\t: 0\n\n\
processor\t: 2\nphysical id\t: 0\ncore id\t: 1\n",
        )
        .unwrap();
        let r = Roots::under(dir.path());
        assert_eq!(model_name(&r).as_deref(), Some("AMD Ryzen 9 9950X3D 16-Core Processor"));
        assert_eq!(physical_cores(&r), Some(2));
    }

    #[test]
    fn freq() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sys/devices/system/cpu/cpu0/cpufreq");
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(p.join("scaling_cur_freq"), "4820000\n").unwrap();
        let mut f = CpuFreq::open(&Roots::under(dir.path()), 2);
        let mut out = Vec::new();
        f.sample(&mut out);
        assert_eq!(out, vec![4820.0, 0.0]);
    }
}
