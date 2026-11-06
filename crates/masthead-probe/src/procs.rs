//! The process table, scanned only while a popover or the window is open.
//!
//! Reading `/proc/[pid]/stat` for every process costs a few milliseconds on a
//! desktop with 600+ processes. At 1 Hz that alone would exceed the resting
//! budget, so the daemon calls this only for the hover and window tiers.

use crate::Roots;
use crate::parse::{fields, u64_of};
use crate::source::{Buf, Source};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Proc {
    pub pid: u32,
    pub name: String,
    pub uid: u32,
    /// Share of all CPU time over the last interval, 0–100.
    pub cpu: f64,
    pub rss_bytes: u64,
    pub threads: u32,
}

/// Fields we need from one `/proc/[pid]/stat` line.
#[derive(Debug, PartialEq, Eq)]
pub struct StatLine<'a> {
    pub comm: &'a [u8],
    pub ppid: u32,
    pub ticks: u64,
    pub threads: u32,
    pub rss_pages: u64,
}

/// `comm` sits in parentheses and may itself contain `)` and spaces, so split
/// at the last `)`.
pub fn parse_stat_line(b: &[u8]) -> Option<StatLine<'_>> {
    let open = b.iter().position(|c| *c == b'(')?;
    let close = b.iter().rposition(|c| *c == b')')?;
    let comm = b.get(open + 1..close)?;
    // After ") ": state(3) ppid(4) ... utime(14) stime(15) ... num_threads(20) ... rss(24)
    let f: Vec<&[u8]> = fields(b.get(close + 1..)?).take(22).collect();
    let n = |i: usize| f.get(i - 3).and_then(|x| u64_of(x));
    Some(StatLine { comm, ppid: n(4)? as u32, ticks: n(14)? + n(15)?, threads: n(20)? as u32, rss_pages: n(24)? })
}

#[derive(Debug)]
pub struct ProcTable {
    proc_root: std::path::PathBuf,
    prev: HashMap<u32, u64>,
    page_size: u64,
    buf: Buf,
    users: HashMap<u32, String>,
}

impl ProcTable {
    pub fn new(roots: &Roots) -> Self {
        Self {
            proc_root: roots.proc.clone(),
            prev: HashMap::new(),
            page_size: rustix::param::page_size() as u64,
            buf: Buf::with_capacity(1024),
            users: HashMap::new(),
        }
    }

    /// Scan all user-space processes. `cpu_ticks_delta` is the change in total
    /// CPU time (all cores) since the previous scan, from `/proc/stat`; pass 0
    /// on the first scan.
    pub fn scan(&mut self, cpu_ticks_delta: u64) -> Vec<Proc> {
        let Ok(rd) = std::fs::read_dir(&self.proc_root) else { return Vec::new() };
        let mut next_prev = HashMap::with_capacity(self.prev.len().max(256));
        let mut out = Vec::with_capacity(self.prev.len().max(256));
        for e in rd.filter_map(Result::ok) {
            let name = e.file_name();
            let Some(pid) = name.to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
            let dir = self.proc_root.join(name);
            let Ok(src) = Source::open(&dir.join("stat")) else { continue };
            let Ok(bytes) = src.read(&mut self.buf) else { continue };
            let Some(st) = parse_stat_line(bytes) else { continue };
            // Kernel threads are children of kthreadd (pid 2).
            if pid == 2 || st.ppid == 2 {
                continue;
            }
            let cpu = match (self.prev.get(&pid), cpu_ticks_delta) {
                (Some(&p), d) if d > 0 => st.ticks.saturating_sub(p) as f64 * 100.0 / d as f64,
                _ => 0.0,
            };
            next_prev.insert(pid, st.ticks);
            let uid = rustix::fs::stat(&dir).map_or(0, |s| s.st_uid);
            out.push(Proc {
                pid,
                name: String::from_utf8_lossy(st.comm).into_owned(),
                uid,
                cpu: cpu.min(100.0),
                rss_bytes: st.rss_pages * self.page_size,
                threads: st.threads,
            });
        }
        self.prev = next_prev;
        out
    }

    /// The login name for a uid, from `/etc/passwd`, cached.
    pub fn user_name(&mut self, uid: u32) -> String {
        if self.users.is_empty()
            && let Ok(s) = std::fs::read_to_string("/etc/passwd")
        {
            self.users = parse_passwd(&s);
        }
        self.users.get(&uid).cloned().unwrap_or_else(|| uid.to_string())
    }
}

pub fn parse_passwd(s: &str) -> HashMap<u32, String> {
    s.lines()
        .filter_map(|l| {
            let mut f = l.split(':');
            let name = f.next()?;
            let uid = f.nth(1)?.parse().ok()?;
            Some((uid, name.to_owned()))
        })
        .collect()
}

/// Sort descending by CPU share and keep the first `n`.
pub fn top_by_cpu(mut v: Vec<Proc>, n: usize) -> Vec<Proc> {
    v.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then(b.rss_bytes.cmp(&a.rss_bytes)));
    v.truncate(n);
    v
}

/// Sort descending by resident memory and keep the first `n`.
pub fn top_by_rss(mut v: Vec<Proc>, n: usize) -> Vec<Proc> {
    v.sort_by_key(|p| std::cmp::Reverse(p.rss_bytes));
    v.truncate(n);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: &[u8] = b"4242 (Web Content (x)) S 1000 4242 4242 0 -1 4194560 100 0 0 0 250 50 0 0 20 0 31 0 9999 1000000 2560 18446744073709551615 0 0 0 0 0 0 0 0 0 0 0 0 17 3 0 0 0 0 0\n";

    #[test]
    fn stat_line() {
        let s = parse_stat_line(LINE).unwrap();
        assert_eq!(s.comm, b"Web Content (x)");
        assert_eq!(s.ppid, 1000);
        assert_eq!(s.ticks, 300);
        assert_eq!(s.threads, 31);
        assert_eq!(s.rss_pages, 2560);
        assert_eq!(parse_stat_line(b"1 (x"), None);
    }

    #[test]
    fn scan_fixture() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("proc");
        for (pid, ppid, ut) in [(100u32, 1u32, 10u64), (3, 2, 5)] {
            std::fs::create_dir_all(p.join(pid.to_string())).unwrap();
            std::fs::write(
                p.join(pid.to_string()).join("stat"),
                format!("{pid} (proc{pid}) S {ppid} 0 0 0 0 0 0 0 0 0 {ut} 0 0 0 20 0 2 0 0 0 10 0\n"),
            )
            .unwrap();
        }
        std::fs::create_dir_all(p.join("self")).unwrap();
        let mut t = ProcTable::new(&Roots::under(d.path()));
        let first = t.scan(0);
        assert_eq!(first.len(), 1);
        assert!(first[0].cpu.abs() < f64::EPSILON);
        std::fs::write(p.join("100/stat"), "100 (proc100) S 1 0 0 0 0 0 0 0 0 0 60 0 0 0 20 0 2 0 0 0 10 0\n").unwrap();
        let second = t.scan(100);
        assert!((second[0].cpu - 50.0).abs() < 1e-9);
        assert_eq!(second[0].rss_bytes, 10 * rustix::param::page_size() as u64);
        let top = top_by_cpu(second.clone(), 5);
        assert_eq!(top.len(), 1);
        assert_eq!(top_by_rss(second, 0).len(), 0);
        let u = parse_passwd("root:x:0:0:root:/root:/bin/bash\nnkoenig:x:1000:1000::/home/n:/bin/bash\nbad\n");
        assert_eq!(u.get(&1000).map(String::as_str), Some("nkoenig"));
        assert!(!t.user_name(0).is_empty());
    }
}
