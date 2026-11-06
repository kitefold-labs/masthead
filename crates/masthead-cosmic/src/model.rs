// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! What the panel and the popover show, built from daemon data. No UI types,
//! so it is unit-tested directly.

use masthead_api::PanelItem;
use masthead_api::detail::{Describe, Detail};
use masthead_core::config::{MemoryUnits, Units};
use masthead_core::{Level, MetricId, format};

pub fn level(item: &PanelItem) -> Level {
    Level::from_u32(item.level)
}

pub fn title(id: &str) -> &'static str {
    id.parse::<MetricId>().map_or("Masthead", MetricId::title)
}

/// Text for a vertical panel, where width is scarce: `23%`, `18G`, `2.1M`.
pub fn compact_text(item: &PanelItem, units: Units) -> String {
    match item.id.as_str() {
        "net" => format::rate(item.value, units.network, true),
        "disk" => format::disk_rate(item.value, true),
        "mem" => {
            let div = match units.memory {
                MemoryUnits::Binary => f64::from(1u32 << 30),
                MemoryUnits::Decimal => 1e9,
            };
            format!("{:.0}G", item.value / div)
        }
        _ => item.text.clone(),
    }
}

/// The popover's large figure and the smaller text after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Big {
    pub value: String,
    pub suffix: String,
}

pub fn big(item: &PanelItem, detail: Option<&Detail>, units: Units) -> Big {
    let pct = |v: f64| Big { value: format!("{}", v.round().clamp(0.0, 100.0) as u32), suffix: "%".into() };
    match item.id.as_str() {
        "cpu" | "gpu" | "battery" if item.text.ends_with('%') => pct(item.value),
        "mem" => {
            let total = detail.and_then(|d| d.mem.as_ref()).map(|m| m.total);
            let used = format::mem_long(item.value as u64, units.memory);
            let (num, unit) = used.split_once(' ').unwrap_or((used.as_str(), ""));
            Big {
                value: num.to_owned(),
                suffix: total.map_or_else(|| unit.to_owned(), |t| format!("of {}", format::mem_long(t, units.memory))),
            }
        }
        "net" => {
            let (rx, tx) = detail.and_then(|d| d.net.as_ref()).map_or((item.value, 0.0), |n| (n.rx, n.tx));
            Big {
                value: format!("↓{}", format::rate(rx, units.network, true)),
                suffix: format!("↑{}", format::rate(tx, units.network, true)),
            }
        }
        "disk" => {
            let (r, w) = detail.and_then(|d| d.disk.as_ref()).map_or((item.value, 0.0), |d| (d.read, d.write));
            Big { value: format::disk_rate(r, true), suffix: format!("read · {} write", format::disk_rate(w, true)) }
        }
        "temp" => {
            let t = format::temp(item.value, units.temperature, false);
            let (num, unit) = t.split_once(' ').unwrap_or((t.as_str(), ""));
            Big { value: num.to_owned(), suffix: unit.to_owned() }
        }
        _ => Big { value: item.text.clone(), suffix: String::new() },
    }
}

/// The device line in the popover header.
pub fn device(item: &PanelItem, describe: &Describe, detail: Option<&Detail>) -> String {
    match item.id.as_str() {
        "cpu" => {
            if describe.threads > 0 {
                format!("{} · {}C/{}T", short_cpu(&describe.cpu_model), describe.cores, describe.threads)
            } else {
                short_cpu(&describe.cpu_model)
            }
        }
        "mem" => {
            let total = detail.and_then(|d| d.mem.as_ref()).map_or(describe.mem_total, |m| m.total);
            if total == 0 { String::new() } else { format!("{} total", format::mem_long(total, MemoryUnits::Binary)) }
        }
        "gpu" => detail
            .and_then(|d| d.gpus.as_ref())
            .and_then(|g| g.iter().find(|g| g.primary))
            .map(|g| g.name.clone())
            .or_else(|| describe.gpus.iter().find(|g| g.discrete).or(describe.gpus.first()).map(|g| g.name.clone()))
            .unwrap_or_default(),
        "net" => detail
            .and_then(|d| d.net.as_ref())
            .map(|n| n.interfaces.iter().filter(|i| i.counted).map(|i| i.name.as_str()).collect::<Vec<_>>().join(" · "))
            .unwrap_or_default(),
        "disk" => format!("{} devices", describe.disks.len()),
        "temp" => item.summary.clone(),
        _ => String::new(),
    }
}

/// `AMD Ryzen 9 9950X3D 16-Core Processor` → `AMD Ryzen 9 9950X3D`.
pub fn short_cpu(model: &str) -> String {
    let mut words: Vec<&str> = model.split_whitespace().collect();
    while let Some(last) = words.last() {
        if last.eq_ignore_ascii_case("processor") || last.ends_with("-Core") || last.eq_ignore_ascii_case("cpu") {
            words.pop();
        } else {
            break;
        }
    }
    words.join(" ")
}

fn top_line(name: &str, value: &str) -> String {
    format!("{name} {value}")
}

/// The popover's label/value rows for a metric.
#[allow(clippy::too_many_lines)] // one arm per metric
pub fn rows(id: &str, detail: &Detail, units: Units) -> Vec<(String, String)> {
    let mut r: Vec<(String, String)> = Vec::new();
    let mut push = |k: &str, v: String| r.push((k.to_owned(), v));
    match id {
        "cpu" => {
            if let Some(c) = &detail.cpu {
                push("Load 1 · 5 · 15", format!("{:.2} {:.2} {:.2}", c.load[0], c.load[1], c.load[2]));
                if c.avg_freq_mhz > 0.0 {
                    push("Frequency", format!("{:.2} GHz avg", c.avg_freq_mhz / 1000.0));
                }
                if let Some(t) = c.package_temp_c {
                    push("Package", format::temp(t, units.temperature, false));
                }
                if let Some(p) = c.top.first() {
                    push("Top", top_line(&p.name, &format!("{:.1}%", p.cpu)));
                }
            }
            if let Some(s) = &detail.system {
                push("Uptime", format::duration(s.uptime));
            }
        }
        "mem" => {
            if let Some(m) = &detail.mem {
                push("Used", format::mem_long(m.used, units.memory));
                push("Cache", format::mem_long(m.cache, units.memory));
                push("Available", format::mem_long(m.available, units.memory));
                if m.swap_total > 0 {
                    push(
                        "Swap",
                        format!("{} of {}", format::mem_long(m.swap_used, units.memory), format::mem_long(m.swap_total, units.memory)),
                    );
                } else {
                    push("Swap", "none".into());
                }
                if let Some(p) = m.top.first() {
                    push("Top", top_line(&p.name, &format::mem_long(p.rss, units.memory)));
                }
            }
        }
        "gpu" => {
            for g in detail.gpus.iter().flatten() {
                if g.primary {
                    if let (Some(u), Some(t)) = (g.vram_used, g.vram_total) {
                        push("VRAM", format!("{} of {}", format::mem_long(u, units.memory), format::mem_long(t, units.memory)));
                    }
                    if let Some(t) = g.temp_c {
                        push("Temperature", format::temp(t, units.temperature, false));
                    }
                    match (g.power_w, g.power_limit_w) {
                        (Some(p), Some(l)) => push("Power", format!("{} of {}", format::watts(p), format::watts(l))),
                        (Some(p), None) => push("Power", format::watts(p)),
                        _ => {}
                    }
                } else {
                    let v = if g.suspended { "suspended".to_owned() } else { g.util.map_or_else(|| "–".to_owned(), format::percent) };
                    push(&g.name, v);
                }
            }
        }
        "net" => {
            if let Some(n) = &detail.net {
                push("Down", format::rate(n.rx, units.network, false));
                push("Up", format::rate(n.tx, units.network, false));
                for i in n.interfaces.iter().filter(|i| i.counted) {
                    let idle = i.rx < 1.0 && i.tx < 1.0;
                    let kind = if i.kind == "wireless" { "Wi-Fi" } else { "Ethernet" };
                    let v = if idle {
                        i.speed_mbps.map_or_else(|| format!("{kind} · idle"), |s| format!("{kind} · {}", link_speed(s)))
                    } else {
                        format!("↓{} ↑{}", format::rate(i.rx, units.network, true), format::rate(i.tx, units.network, true))
                    };
                    push(&i.name, v);
                }
            }
        }
        "disk" => {
            if let Some(d) = &detail.disk {
                push("Read", format::disk_rate(d.read, false));
                push("Write", format::disk_rate(d.write, false));
                for dev in &d.devices {
                    push(&dev.name, format!("r {} · w {}", format::disk_rate(dev.read, true), format::disk_rate(dev.write, true)));
                }
            }
        }
        "temp" => {
            let mut temps: Vec<_> = detail.sensors.iter().flatten().filter(|s| s.kind == "temp" && s.value.is_some()).collect();
            temps.sort_by(|a, b| b.value.unwrap_or(0.0).total_cmp(&a.value.unwrap_or(0.0)));
            for s in temps.into_iter().take(5) {
                push(&s.name, format::temp(s.value.unwrap_or(0.0), units.temperature, false));
            }
        }
        "battery" => {
            for b in detail.batteries.iter().flatten() {
                let mut v = b.percent.map_or_else(|| "–".to_owned(), format::percent);
                v.push_str(" · ");
                v.push_str(&b.state);
                if let Some(w) = b.watts {
                    v.push_str(" · ");
                    v.push_str(&format::watts(w));
                }
                push(&b.name, v);
            }
        }
        _ => {}
    }
    r
}

fn link_speed(mbps: u32) -> String {
    if mbps >= 1000 && mbps.is_multiple_of(100) { format!("{} Gb/s", f64::from(mbps) / 1000.0) } else { format!("{mbps} Mb/s") }
}

/// History series that feed the popover sparkline, primary first.
pub fn series(id: &str) -> &'static [&'static str] {
    match id {
        "cpu" => &["cpu"],
        "mem" => &["mem"],
        "gpu" => &["gpu"],
        "net" => &["net.rx", "net.tx"],
        "disk" => &["disk.read", "disk.write"],
        "temp" => &["temp"],
        "battery" => &["battery"],
        _ => &[],
    }
}

/// A round axis maximum at or above every finite value.
pub fn nice_max(series: &[Vec<f64>]) -> f64 {
    let m = series.iter().flatten().copied().filter(|v| v.is_finite()).fold(0.0f64, f64::max);
    if m <= 0.0 {
        return 1.0;
    }
    let p = 10f64.powf(m.log10().floor());
    for s in [1.0, 2.0, 2.5, 5.0, 10.0] {
        if s * p >= m {
            return s * p;
        }
    }
    10.0 * p
}

/// Total memory, from detail when the popover has it, else from `Describe`.
pub fn mem_total(describe: &Describe, detail: Option<&Detail>) -> u64 {
    detail.and_then(|d| d.mem.as_ref()).map(|m| m.total).filter(|t| *t > 0).unwrap_or(describe.mem_total)
}

/// The sparkline's vertical scale.
pub fn spark_max(id: &str, series: &[Vec<f64>], mem_total: u64) -> f64 {
    match id {
        "cpu" | "gpu" | "battery" | "temp" => 100.0,
        "mem" if mem_total > 0 => mem_total as f64,
        _ => nice_max(series),
    }
}

/// Keep the last `n` values of a series for a trend line in the bar.
pub fn push_trend(buf: &mut std::collections::VecDeque<f64>, v: f64, n: usize) {
    buf.push_back(v);
    while buf.len() > n {
        buf.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use masthead_api::detail::{CpuDetail, GpuDetail, IfaceDetail, MemDetail, NetDetail, ProcDetail, SensorDetail, SystemDetail};
    use masthead_core::config::NetworkUnits;

    fn item(id: &str, value: f64, text: &str) -> PanelItem {
        PanelItem { id: id.into(), value, text: text.into(), level: 0, summary: "k10temp Tctl".into() }
    }

    #[test]
    fn compact() {
        let u = Units::default();
        assert_eq!(compact_text(&item("mem", 19_756_849_152.0, "18.4G"), u), "18G");
        assert_eq!(compact_text(&item("net", 2_100_000.0, "↓2.1M ↑180K"), u), "2.1M");
        assert_eq!(compact_text(&item("disk", 60e6, "r60M w3M"), u), "60M");
        assert_eq!(compact_text(&item("cpu", 23.0, "23%"), u), "23%");
        let bits = Units { network: NetworkUnits::Bits, ..Units::default() };
        assert_eq!(compact_text(&item("net", 125_000.0, ""), bits), "1.0m");
    }

    #[test]
    fn big_values() {
        let u = Units::default();
        assert_eq!(big(&item("cpu", 23.4, "23%"), None, u), Big { value: "23".into(), suffix: "%".into() });
        let d = Detail { mem: Some(MemDetail { total: 63_752_105_984, ..MemDetail::default() }), ..Detail::default() };
        assert_eq!(big(&item("mem", 19_756_849_152.0, "18.4G"), Some(&d), u), Big { value: "18.4".into(), suffix: "of 59.4 GiB".into() });
        assert_eq!(big(&item("mem", 19_756_849_152.0, "18.4G"), None, u).suffix, "GiB");
        assert_eq!(big(&item("temp", 64.4, "64°"), None, u), Big { value: "64".into(), suffix: "°C".into() });
        let n = Detail { net: Some(NetDetail { rx: 2_100_000.0, tx: 180_000.0, ..NetDetail::default() }), ..Detail::default() };
        assert_eq!(big(&item("net", 0.0, ""), Some(&n), u), Big { value: "↓2.1M".into(), suffix: "↑180K".into() });
        assert_eq!(big(&item("gpu", 0.0, "off"), None, u).value, "off");
        assert_eq!(big(&item("disk", 5e6, ""), None, u).value, "5.0M");
    }

    #[test]
    fn cpu_names() {
        assert_eq!(short_cpu("AMD Ryzen 9 9950X3D 16-Core Processor"), "AMD Ryzen 9 9950X3D");
        assert_eq!(short_cpu("Intel(R) Core(TM) i7-1185G7 @ 3.00GHz"), "Intel(R) Core(TM) i7-1185G7 @ 3.00GHz");
        let desc = Describe { cpu_model: "AMD Ryzen 9 9950X3D 16-Core Processor".into(), cores: 16, threads: 32, ..Describe::default() };
        assert_eq!(device(&item("cpu", 0.0, ""), &desc, None), "AMD Ryzen 9 9950X3D · 16C/32T");
        assert_eq!(device(&item("temp", 0.0, ""), &desc, None), "k10temp Tctl");
    }

    #[test]
    fn detail_rows() {
        let u = Units::default();
        let d = Detail {
            system: Some(SystemDetail { uptime: 93_250.0, ..SystemDetail::default() }),
            cpu: Some(CpuDetail {
                load: [4.12, 3.8, 3.55],
                avg_freq_mhz: 4820.0,
                package_temp_c: Some(64.2),
                top: vec![ProcDetail { name: "rustc".into(), cpu: 9.84, ..ProcDetail::default() }],
                ..CpuDetail::default()
            }),
            gpus: Some(vec![
                GpuDetail {
                    name: "NVIDIA GeForce RTX 5090".into(),
                    primary: true,
                    vram_used: Some(2 << 30),
                    vram_total: Some(32 << 30),
                    temp_c: Some(35.0),
                    power_w: Some(182.0),
                    power_limit_w: Some(575.0),
                    ..GpuDetail::default()
                },
                GpuDetail { name: "AMD Radeon Graphics".into(), util: Some(3.0), ..GpuDetail::default() },
                GpuDetail { name: "Sleeping".into(), suspended: true, ..GpuDetail::default() },
            ]),
            net: Some(NetDetail {
                rx: 1000.0,
                tx: 10.0,
                interfaces: vec![
                    IfaceDetail {
                        name: "enp14s0".into(),
                        kind: "ethernet".into(),
                        speed_mbps: Some(2500),
                        counted: true,
                        ..IfaceDetail::default()
                    },
                    IfaceDetail { name: "wlp15s0".into(), kind: "wireless".into(), rx: 1000.0, counted: true, ..IfaceDetail::default() },
                    IfaceDetail { name: "docker0".into(), kind: "virtual".into(), counted: false, ..IfaceDetail::default() },
                ],
                hidden_virtual: 0,
            }),
            sensors: Some(vec![
                SensorDetail { name: "nvme0 Composite".into(), kind: "temp".into(), value: Some(41.0), ..SensorDetail::default() },
                SensorDetail { name: "k10temp Tctl".into(), kind: "temp".into(), value: Some(66.0), ..SensorDetail::default() },
                SensorDetail { name: "fan1".into(), kind: "fan".into(), value: Some(1200.0), ..SensorDetail::default() },
            ]),
            ..Detail::default()
        };
        let cpu = rows("cpu", &d, u);
        assert_eq!(cpu[0], ("Load 1 · 5 · 15".into(), "4.12 3.80 3.55".into()));
        assert_eq!(cpu[1].1, "4.82 GHz avg");
        assert_eq!(cpu[2].1, "64 °C");
        assert_eq!(cpu[3].1, "rustc 9.8%");
        assert_eq!(cpu[4], ("Uptime".into(), "1d 1h".into()));
        let gpu = rows("gpu", &d, u);
        assert_eq!(gpu[0], ("VRAM".into(), "2.0 GiB of 32.0 GiB".into()));
        assert_eq!(gpu[2].1, "182 W of 575 W");
        assert_eq!(gpu[3], ("AMD Radeon Graphics".into(), "3%".into()));
        assert_eq!(gpu[4].1, "suspended");
        let net = rows("net", &d, u);
        assert_eq!(net.len(), 4, "docker0 is not counted");
        assert_eq!(net[2], ("enp14s0".into(), "Ethernet · 2.5 Gb/s".into()));
        assert_eq!(net[3].1, "↓1.0K ↑0B");
        let temp = rows("temp", &d, u);
        assert_eq!(temp[0], ("k10temp Tctl".into(), "66 °C".into()));
        assert_eq!(temp.len(), 2);
        let mem = rows("mem", &Detail { mem: Some(MemDetail::default()), ..Detail::default() }, u);
        assert_eq!(mem[3], ("Swap".into(), "none".into()));
        assert_eq!(rows("x", &d, u).len(), 0);
    }

    #[test]
    fn scales() {
        assert_eq!(series("net"), &["net.rx", "net.tx"]);
        assert_eq!(series("nope").len(), 0);
        assert!((nice_max(&[vec![0.0, 180.0, f64::NAN]]) - 200.0).abs() < 1e-9);
        assert!((nice_max(&[vec![]]) - 1.0).abs() < 1e-9);
        assert!((nice_max(&[vec![9.9e6]]) - 1e7).abs() < 1e-3);
        let desc = Describe { mem_total: 64, ..Describe::default() };
        let d = Detail { mem: Some(MemDetail { total: 128, ..MemDetail::default() }), ..Detail::default() };
        assert_eq!(mem_total(&desc, None), 64);
        assert_eq!(mem_total(&desc, Some(&d)), 128);
        assert_eq!(device(&item("mem", 0.0, ""), &Describe::default(), None), "");
        assert!((spark_max("mem", &[], 64) - 64.0).abs() < 1e-9);
        assert!((spark_max("mem", &[vec![3.0]], 0) - 5.0).abs() < 1e-9);
        assert!((spark_max("cpu", &[], 0) - 100.0).abs() < 1e-9);
        let mut t = std::collections::VecDeque::new();
        for i in 0..10 {
            push_trend(&mut t, f64::from(i), 3);
        }
        assert_eq!(t.iter().copied().collect::<Vec<_>>(), vec![7.0, 8.0, 9.0]);
        assert_eq!(title("mem"), "Memory");
        assert_eq!(level(&item("cpu", 0.0, "")), Level::Normal);
        assert_eq!(link_speed(100), "100 Mb/s");
    }
}
