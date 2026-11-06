// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! Network throughput from `/proc/net/dev`, physical interfaces only by default.

use crate::parse::{fields, lines, u64_of};
use crate::source::{Buf, Source};
use crate::{Roots, link_name, read_attr};
use std::io;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IfaceKind {
    Ethernet,
    Wireless,
    Virtual,
    Loopback,
}

/// An interface found at discovery.
#[derive(Debug, Clone)]
pub struct Iface {
    pub name: String,
    pub kind: IfaceKind,
    pub driver: Option<String>,
    /// Link speed in Mb/s, when the driver reports one.
    pub speed_mbps: Option<u32>,
    /// Cumulative counters from the last sample.
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    /// Rate over the last interval in bytes per second.
    pub rx_rate: f64,
    pub tx_rate: f64,
    primed: bool,
    seen: bool,
}

impl Iface {
    pub fn physical(&self) -> bool {
        matches!(self.kind, IfaceKind::Ethernet | IfaceKind::Wireless)
    }
}

/// List interfaces under `/sys/class/net`. An interface with a `device` link is
/// backed by hardware; everything else (docker0, br-*, veth*, tun) is virtual.
pub fn discover(roots: &Roots) -> Vec<Iface> {
    let dir = roots.sys.join("class/net");
    let Ok(rd) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut out: Vec<Iface> = rd
        .filter_map(Result::ok)
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let p = dir.join(&name);
            let kind = if name == "lo" {
                IfaceKind::Loopback
            } else if !p.join("device").exists() {
                IfaceKind::Virtual
            } else if p.join("wireless").exists() || p.join("phy80211").exists() {
                IfaceKind::Wireless
            } else {
                IfaceKind::Ethernet
            };
            let speed_mbps = read_attr(&p.join("speed")).and_then(|s| s.parse::<i64>().ok()).and_then(|v| u32::try_from(v).ok());
            Iface {
                driver: link_name(&p.join("device/driver")),
                name,
                kind,
                speed_mbps,
                rx_bytes: 0,
                tx_bytes: 0,
                rx_rate: 0.0,
                tx_rate: 0.0,
                primed: false,
                seen: false,
            }
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Totals across the selected interfaces, in bytes per second.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct NetRates {
    pub rx: f64,
    pub tx: f64,
}

#[derive(Debug)]
pub struct Net {
    src: Source,
    buf: Buf,
    pub ifaces: Vec<Iface>,
    /// Set when `/proc/net/dev` lists an interface discovery did not know about.
    pub stale: bool,
}

impl Net {
    pub fn open(roots: &Roots) -> io::Result<Self> {
        Ok(Self { src: Source::open(&roots.proc.join("net/dev"))?, buf: Buf::with_capacity(8192), ifaces: discover(roots), stale: false })
    }

    /// Replace the interface list, keeping counters for interfaces that remain.
    pub fn rediscover(&mut self, roots: &Roots) {
        let mut fresh = discover(roots);
        for f in &mut fresh {
            if let Some(old) = self.ifaces.iter().find(|o| o.name == f.name) {
                f.rx_bytes = old.rx_bytes;
                f.tx_bytes = old.tx_bytes;
                f.primed = old.primed;
            }
        }
        self.ifaces = fresh;
        self.stale = false;
    }

    /// Sample counters. `include` decides which interfaces count toward the
    /// total; rates are computed for every interface either way.
    pub fn sample(&mut self, dt_secs: f64, include: impl Fn(&Iface) -> bool) -> io::Result<NetRates> {
        let bytes = self.src.read(&mut self.buf)?;
        for i in &mut self.ifaces {
            i.seen = false;
        }
        for line in lines(bytes) {
            let Some(colon) = line.iter().position(|b| *b == b':') else { continue };
            let name = line[..colon].trim_ascii();
            let mut f = fields(&line[colon + 1..]);
            let rx = f.next().and_then(u64_of);
            let tx = f.nth(7).and_then(u64_of);
            let (Some(rx), Some(tx)) = (rx, tx) else { continue };
            match self.ifaces.iter_mut().find(|i| i.name.as_bytes() == name) {
                Some(i) => {
                    i.seen = true;
                    if i.primed && dt_secs > 0.0 {
                        // A counter that went backwards was reset (interface re-created).
                        i.rx_rate = rx.checked_sub(i.rx_bytes).map_or(0.0, |d| d as f64 / dt_secs);
                        i.tx_rate = tx.checked_sub(i.tx_bytes).map_or(0.0, |d| d as f64 / dt_secs);
                    }
                    i.rx_bytes = rx;
                    i.tx_bytes = tx;
                    i.primed = true;
                }
                None => self.stale = true,
            }
        }
        let mut total = NetRates::default();
        for i in &mut self.ifaces {
            if !i.seen {
                i.rx_rate = 0.0;
                i.tx_rate = 0.0;
                self.stale = true;
                continue;
            }
            if include(i) {
                total.rx += i.rx_rate;
                total.tx += i.tx_rate;
            }
        }
        Ok(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn fixture() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let net = d.path().join("sys/class/net");
        for n in ["lo", "enp14s0", "wlp15s0", "docker0"] {
            std::fs::create_dir_all(net.join(n)).unwrap();
        }
        let dev = d.path().join("sys/devices/pci/0000:0e:00.0");
        std::fs::create_dir_all(dev.join("driver_dir/r8169")).unwrap();
        symlink(&dev, net.join("enp14s0/device")).unwrap();
        symlink(dev.join("driver_dir/r8169"), dev.join("driver")).unwrap();
        std::fs::write(net.join("enp14s0/speed"), "2500\n").unwrap();
        symlink(&dev, net.join("wlp15s0/device")).unwrap();
        std::fs::create_dir_all(net.join("wlp15s0/wireless")).unwrap();
        std::fs::create_dir_all(d.path().join("proc/net")).unwrap();
        d
    }

    fn write_dev(d: &tempfile::TempDir, rx: u64, tx: u64) {
        let s = format!(
            "Inter-|   Receive                                                |  Transmit\n \
face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed\n    \
lo: 999 1 0 0 0 0 0 0 999 1 0 0 0 0 0 0\n\
enp14s0: {rx} 10 0 0 0 0 0 0 {tx} 10 0 0 0 0 0 0\n\
wlp15s0: 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n\
docker0: 5000 1 0 0 0 0 0 0 5000 1 0 0 0 0 0 0\n"
        );
        std::fs::write(d.path().join("proc/net/dev"), s).unwrap();
    }

    #[test]
    fn classifies_and_rates() {
        let d = fixture();
        write_dev(&d, 1000, 100);
        let roots = Roots::under(d.path());
        let mut n = Net::open(&roots).unwrap();
        let kinds: Vec<_> = n.ifaces.iter().map(|i| (i.name.as_str(), i.kind)).collect();
        assert!(kinds.contains(&("docker0", IfaceKind::Virtual)));
        assert!(kinds.contains(&("enp14s0", IfaceKind::Ethernet)));
        assert!(kinds.contains(&("wlp15s0", IfaceKind::Wireless)));
        assert!(kinds.contains(&("lo", IfaceKind::Loopback)));
        let eth = n.ifaces.iter().find(|i| i.name == "enp14s0").unwrap();
        assert_eq!(eth.speed_mbps, Some(2500));
        assert_eq!(eth.driver.as_deref(), Some("r8169"));

        assert_eq!(n.sample(1.0, Iface::physical).unwrap(), NetRates::default());
        write_dev(&d, 3000, 600);
        let r = n.sample(2.0, Iface::physical).unwrap();
        assert!((r.rx - 1000.0).abs() < 1e-9 && (r.tx - 250.0).abs() < 1e-9);
        // counter reset: no negative spike
        write_dev(&d, 10, 10);
        let r = n.sample(1.0, Iface::physical).unwrap();
        assert_eq!(r, NetRates::default());
        assert!(!n.stale);
    }

    #[test]
    fn unknown_interface_marks_stale() {
        let d = fixture();
        write_dev(&d, 1, 1);
        let roots = Roots::under(d.path());
        let mut n = Net::open(&roots).unwrap();
        std::fs::write(d.path().join("proc/net/dev"), "h\nh\nenx0: 1 0 0 0 0 0 0 0 1 0 0 0 0 0 0 0\n").unwrap();
        n.sample(1.0, Iface::physical).unwrap();
        assert!(n.stale);
        std::fs::create_dir_all(d.path().join("sys/class/net/enx0")).unwrap();
        n.rediscover(&roots);
        assert!(!n.stale);
        assert!(n.ifaces.iter().any(|i| i.name == "enx0"));
    }
}
