// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! Window state that has nothing to do with drawing: pages, time ranges,
//! chart series, statistics and the process table order. Kept free of UI
//! types so it can be unit-tested.

use masthead_api::detail::{Detail, ProcDetail};
use masthead_core::MetricId;
use masthead_core::config::Units;
use masthead_core::format;

/// What the main area shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Metric(MetricId),
    Processes,
    Settings,
    /// The disk map and cleanup, opened from the Disk page.
    DiskUsage,
}

impl Page {
    /// Parse a `Toggle`/`Show` argument: a metric id, `processes`, `settings` or `disk-usage`.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "settings" => Some(Self::Settings),
            "processes" => Some(Self::Processes),
            "disk-usage" => Some(Self::DiskUsage),
            _ => s.parse::<MetricId>().ok().map(Self::Metric),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Metric(m) => m.as_str(),
            Self::Processes => "processes",
            Self::Settings => "settings",
            Self::DiskUsage => "disk-usage",
        }
    }
}

/// The chart's time window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Range {
    #[default]
    M1,
    M5,
    M15,
    H1,
}

impl Range {
    pub const ALL: [Range; 4] = [Self::M1, Self::M5, Self::M15, Self::H1];

    pub fn seconds(self) -> u32 {
        match self {
            Self::M1 => 60,
            Self::M5 => 300,
            Self::M15 => 900,
            Self::H1 => 3600,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::M1 => "1m",
            Self::M5 => "5m",
            Self::M15 => "15m",
            Self::H1 => "1h",
        }
    }

    /// The daemon keeps 15 minutes at 1 s; longer ranges come at 1 min.
    pub fn step(self) -> f64 {
        if self.seconds() <= 900 { 1.0 } else { 60.0 }
    }

    /// Points the chart holds for this range.
    pub fn points(self) -> usize {
        (f64::from(self.seconds()) / self.step()) as usize
    }
}

/// History series for a metric. The first is the primary (area) series.
pub fn series_for(m: MetricId) -> &'static [&'static str] {
    match m {
        MetricId::Cpu => &["cpu"],
        MetricId::Mem => &["mem"],
        MetricId::Gpu => &["gpu"],
        MetricId::Net => &["net.rx", "net.tx"],
        MetricId::Disk => &["disk.read", "disk.write"],
        MetricId::Temp => &["temp"],
        MetricId::Battery => &["battery"],
    }
}

/// The value of one history series in a live detail sample, matching what the
/// daemon records for that series.
pub fn series_value(d: &Detail, series: &str) -> f64 {
    let v = match series {
        "cpu" => d.cpu.as_ref().map(|c| c.total),
        "mem" => d.mem.as_ref().map(|m| m.used as f64),
        "gpu" => d.gpus.as_ref().and_then(|g| g.iter().find(|g| g.primary).and_then(|g| g.util)),
        "net.rx" => d.net.as_ref().map(|n| n.rx),
        "net.tx" => d.net.as_ref().map(|n| n.tx),
        "disk.read" => d.disk.as_ref().map(|x| x.read),
        "disk.write" => d.disk.as_ref().map(|x| x.write),
        "temp" => d.cpu.as_ref().and_then(|c| c.package_temp_c),
        "battery" => d.batteries.as_ref().and_then(|b| b.first()).and_then(|b| b.percent),
        _ => None,
    };
    v.unwrap_or(f64::NAN)
}

/// A chart series: a lower and upper envelope at a fixed step. For 1 s data
/// the two are equal; for 1 min data `max` is the peak of each minute.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Series {
    pub step: f64,
    pub min: Vec<f64>,
    pub max: Vec<f64>,
}

impl Series {
    pub fn from_history(step: f64, min: Vec<f64>, max: Vec<f64>) -> Self {
        Self { step, min, max }
    }

    /// Append one live 1 s sample, keeping at most `cap` points.
    pub fn push(&mut self, v: f64, cap: usize) {
        self.min.push(v);
        self.max.push(v);
        if self.min.len() > cap {
            let drop = self.min.len() - cap;
            self.min.drain(..drop);
            self.max.drain(..drop);
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.max.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.max.is_empty()
    }

    pub fn latest(&self) -> Option<f64> {
        self.max.iter().rev().copied().find(|v| v.is_finite())
    }
}

/// Now, average and peak over the visible data, ignoring gaps.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Stats {
    pub now: Option<f64>,
    pub avg: Option<f64>,
    pub peak: Option<f64>,
}

pub fn stats(s: &Series) -> Stats {
    let finite: Vec<f64> = s.max.iter().copied().filter(|v| v.is_finite()).collect();
    if finite.is_empty() {
        return Stats::default();
    }
    // The average uses the lower envelope where it exists so 1 min data is not
    // biased toward its peaks.
    let lows: Vec<f64> = s.min.iter().copied().filter(|v| v.is_finite()).collect();
    let base = if lows.is_empty() { &finite } else { &lows };
    Stats { now: s.latest(), avg: Some(base.iter().sum::<f64>() / base.len() as f64), peak: finite.iter().copied().reduce(f64::max) }
}

/// A y-axis maximum that is a round number at or above every value.
pub fn nice_max(values: impl IntoIterator<Item = f64>) -> f64 {
    let m = values.into_iter().filter(|v| v.is_finite()).fold(0.0_f64, f64::max);
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortKey {
    #[default]
    Cpu,
    Memory,
    Name,
    Pid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sort {
    pub key: SortKey,
    pub descending: bool,
}

impl Default for Sort {
    fn default() -> Self {
        Self { key: SortKey::Cpu, descending: true }
    }
}

impl Sort {
    /// Clicking the current column flips direction; a new column starts with
    /// its natural direction (largest first for numbers, A–Z for names).
    pub fn click(self, key: SortKey) -> Self {
        if key == self.key {
            Self { key, descending: !self.descending }
        } else {
            Self { key, descending: !matches!(key, SortKey::Name | SortKey::Pid) }
        }
    }

    pub fn apply(self, procs: &mut [ProcDetail]) {
        procs.sort_by(|a, b| {
            let o = match self.key {
                SortKey::Cpu => a.cpu.total_cmp(&b.cpu).then(a.rss.cmp(&b.rss)),
                SortKey::Memory => a.rss.cmp(&b.rss),
                SortKey::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.pid.cmp(&b.pid)),
                SortKey::Pid => a.pid.cmp(&b.pid),
            };
            if self.descending { o.reverse() } else { o }
        });
    }
}

/// Height of one process-table row, including its divider.
pub const ROW_HEIGHT: f32 = 29.0;
/// Space above the first row: page title, filter and column headings.
pub const TABLE_TOP: f32 = 96.0;

/// How many process rows fit in `height` pixels of page.
pub fn rows_that_fit(height: f32) -> usize {
    (((height - TABLE_TOP) / ROW_HEIGHT).floor().max(3.0)) as usize
}

/// Keep only processes whose name or user contains `filter` (case-insensitive).
pub fn filter_procs<'a>(procs: &'a [ProcDetail], filter: &str) -> Vec<&'a ProcDetail> {
    let f = filter.trim().to_lowercase();
    procs.iter().filter(|p| f.is_empty() || p.name.to_lowercase().contains(&f) || p.user.to_lowercase().contains(&f)).collect()
}

/// Puts `procs` in the row order `order` that was on screen, so rows stay
/// still under the pointer. Processes not in `order` go last.
pub fn hold_order(procs: &mut [ProcDetail], order: &[u32]) {
    procs.sort_by_key(|p| order.iter().position(|&pid| pid == p.pid).unwrap_or(usize::MAX));
}

/// Move `m` one place up (`-1`) or down (`+1`) in the bar order.
pub fn move_metric(list: &mut [MetricId], m: MetricId, delta: i32) {
    let Some(i) = list.iter().position(|x| *x == m) else { return };
    let Some(j) = i.checked_add_signed(delta as isize) else { return };
    if j < list.len() {
        list.swap(i, j);
    }
}

/// A value as the stats row and tables show it: `23.4%`, `18.4 GiB`, `2.1 MB/s`, `64 °C`.
pub fn fmt_value(m: MetricId, v: f64, u: Units) -> String {
    if !v.is_finite() {
        return "–".to_owned();
    }
    match m {
        MetricId::Cpu | MetricId::Gpu | MetricId::Battery => format!("{v:.1}%"),
        MetricId::Mem => format::mem_long(v.max(0.0) as u64, u.memory),
        MetricId::Net => format::rate(v, u.network, false),
        MetricId::Disk => format::disk_rate(v, false),
        MetricId::Temp => format::temp(v, u.temperature, false),
    }
}

/// A y-axis label: `50%`, `16G`, `2.5M`, `60°`.
pub fn fmt_axis(m: MetricId, v: f64, u: Units) -> String {
    match m {
        MetricId::Cpu | MetricId::Gpu | MetricId::Battery => format!("{}%", v.round() as i64),
        MetricId::Mem => format::mem_short(v.max(0.0) as u64, u.memory).replace(".0G", "G"),
        MetricId::Net => format::rate(v, u.network, true),
        MetricId::Disk => format::disk_rate(v, true),
        MetricId::Temp => format::temp(v, u.temperature, true),
    }
}

/// The compact value the sidebar shows for a metric, from a live sample.
pub fn short_value(m: MetricId, d: &Detail, u: Units) -> Option<String> {
    let v = series_value(d, series_for(m)[0]);
    match m {
        MetricId::Net => {
            let n = d.net.as_ref()?;
            Some(format!("↓{}", format::rate(n.rx, u.network, true)))
        }
        MetricId::Disk => {
            let x = d.disk.as_ref()?;
            Some(format::disk_rate(x.read + x.write, true))
        }
        MetricId::Gpu if d.gpus.as_ref()?.iter().find(|g| g.primary).is_some_and(|g| g.suspended) => Some("off".to_owned()),
        _ if !v.is_finite() => None,
        MetricId::Cpu | MetricId::Gpu | MetricId::Battery => Some(format::percent(v)),
        MetricId::Mem => Some(format::mem_short(v as u64, u.memory)),
        MetricId::Temp => Some(format::temp(v, u.temperature, true)),
    }
}

/// The chart's y-axis maximum: fixed for percentages, total memory for
/// memory, and a round number above the data for rates and temperatures.
pub fn y_max(m: MetricId, series: &[Series], mem_total: u64) -> f64 {
    let data = || series.iter().flat_map(|s| s.max.iter().copied());
    match m {
        MetricId::Cpu | MetricId::Gpu | MetricId::Battery => 100.0,
        MetricId::Mem if mem_total > 0 => mem_total as f64,
        MetricId::Temp => nice_max(data()).max(100.0),
        _ => nice_max(data()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use masthead_api::detail::{CpuDetail, GpuDetail, MemDetail, NetDetail};

    #[test]
    fn pages() {
        assert_eq!(Page::parse("cpu"), Some(Page::Metric(MetricId::Cpu)));
        assert_eq!(Page::parse("settings"), Some(Page::Settings));
        assert_eq!(Page::parse("processes"), Some(Page::Processes));
        assert_eq!(Page::parse("nope"), None);
        for p in [Page::Metric(MetricId::Net), Page::Processes, Page::Settings, Page::DiskUsage] {
            assert_eq!(Page::parse(p.as_str()), Some(p));
        }
    }

    #[test]
    fn ranges() {
        assert_eq!(Range::default(), Range::M1);
        assert_eq!(Range::M15.points(), 900);
        assert_eq!(Range::H1.points(), 60);
        assert!((Range::H1.step() - 60.0).abs() < f64::EPSILON);
        let labels: Vec<_> = Range::ALL.iter().map(|r| r.label()).collect();
        assert_eq!(labels, ["1m", "5m", "15m", "1h"]);
    }

    #[test]
    fn series_from_detail() {
        let d = Detail {
            cpu: Some(CpuDetail { total: 12.5, package_temp_c: Some(64.0), ..CpuDetail::default() }),
            mem: Some(MemDetail { used: 1024, ..MemDetail::default() }),
            gpus: Some(vec![
                GpuDetail { util: Some(9.0), ..GpuDetail::default() },
                GpuDetail { primary: true, util: Some(41.0), ..GpuDetail::default() },
            ]),
            net: Some(NetDetail { rx: 5.0, tx: 6.0, ..NetDetail::default() }),
            ..Detail::default()
        };
        assert!((series_value(&d, "cpu") - 12.5).abs() < 1e-9);
        assert!((series_value(&d, "mem") - 1024.0).abs() < 1e-9);
        assert!((series_value(&d, "gpu") - 41.0).abs() < 1e-9);
        assert!((series_value(&d, "net.tx") - 6.0).abs() < 1e-9);
        assert!((series_value(&d, "temp") - 64.0).abs() < 1e-9);
        assert!(series_value(&d, "disk.read").is_nan());
        assert!(series_value(&d, "battery").is_nan());
        assert!(series_value(&d, "bogus").is_nan());
        for m in MetricId::ALL {
            assert_ne!(series_for(m).len(), 0);
        }
        assert_eq!(series_for(MetricId::Net), ["net.rx", "net.tx"]);
    }

    #[test]
    fn series_push_and_stats() {
        let mut s = Series::from_history(1.0, vec![1.0, f64::NAN, 3.0], vec![1.0, f64::NAN, 3.0]);
        s.push(5.0, 3);
        assert_eq!(s.len(), 3);
        assert!(s.min[0].is_nan());
        assert_eq!(s.latest(), Some(5.0));
        let st = stats(&s);
        assert_eq!(st.now, Some(5.0));
        assert_eq!(st.peak, Some(5.0));
        assert!((st.avg.unwrap() - 4.0).abs() < 1e-9);
        assert_eq!(stats(&Series::default()), Stats::default());
        assert!(Series::default().is_empty());
        let coarse = Series::from_history(60.0, vec![1.0, 1.0], vec![9.0, 3.0]);
        let st = stats(&coarse);
        assert_eq!(st.peak, Some(9.0));
        assert!((st.avg.unwrap() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn nice() {
        assert!((nice_max([0.0]) - 1.0).abs() < 1e-9);
        assert!((nice_max([73.0]) - 100.0).abs() < 1e-9);
        assert!((nice_max([2_100_000.0, f64::NAN]) - 2_500_000.0).abs() < 1e-3);
        assert!((nice_max([180.0]) - 200.0).abs() < 1e-9);
        assert!((nice_max([450.0]) - 500.0).abs() < 1e-9);
    }

    fn p(pid: u32, name: &str, cpu: f64, rss: u64) -> ProcDetail {
        ProcDetail { pid, name: name.into(), user: "nate".into(), cpu, rss, threads: 1 }
    }

    #[test]
    fn sorting_and_filtering() {
        let mut v = vec![p(3, "rustc", 40.0, 10), p(1, "Firefox", 2.0, 300), p(2, "bash", 0.0, 5)];
        Sort::default().apply(&mut v);
        assert_eq!(v[0].name, "rustc");
        let s = Sort::default().click(SortKey::Name);
        assert!(!s.descending);
        s.apply(&mut v);
        assert_eq!(v.iter().map(|x| x.pid).collect::<Vec<_>>(), [2, 1, 3]);
        let s = s.click(SortKey::Name);
        assert!(s.descending);
        let m = s.click(SortKey::Memory);
        assert!(m.descending);
        m.apply(&mut v);
        assert_eq!(v[0].name, "Firefox");
        Sort::default().click(SortKey::Pid).apply(&mut v);
        assert_eq!(v[0].pid, 1);
        assert_eq!(filter_procs(&v, "FIRE").len(), 1);
        assert_eq!(filter_procs(&v, " ").len(), 3);
        assert_eq!(filter_procs(&v, "nate").len(), 3);
    }

    #[test]
    fn formatting() {
        let u = Units::default();
        assert_eq!(fmt_value(MetricId::Cpu, 23.44, u), "23.4%");
        assert_eq!(fmt_value(MetricId::Mem, 19_756_849_152.0, u), "18.4 GiB");
        assert_eq!(fmt_value(MetricId::Net, 2_100_000.0, u), "2.1 MB/s");
        assert_eq!(fmt_value(MetricId::Temp, 64.4, u), "64 °C");
        assert_eq!(fmt_value(MetricId::Disk, f64::NAN, u), "–");
        assert_eq!(fmt_axis(MetricId::Cpu, 50.0, u), "50%");
        assert_eq!(fmt_axis(MetricId::Mem, 16.0 * 1024.0 * 1024.0 * 1024.0, u), "16G");
        assert_eq!(fmt_axis(MetricId::Net, 2_500_000.0, u), "2.5M");
        assert_eq!(fmt_axis(MetricId::Temp, 60.0, u), "60°");
        let d = Detail {
            cpu: Some(CpuDetail { total: 23.0, package_temp_c: Some(64.0), ..CpuDetail::default() }),
            net: Some(NetDetail { rx: 27_000.0, ..NetDetail::default() }),
            gpus: Some(vec![GpuDetail { primary: true, suspended: true, ..GpuDetail::default() }]),
            ..Detail::default()
        };
        assert_eq!(short_value(MetricId::Cpu, &d, u).as_deref(), Some("23%"));
        assert_eq!(short_value(MetricId::Net, &d, u).as_deref(), Some("↓27K"));
        assert_eq!(short_value(MetricId::Temp, &d, u).as_deref(), Some("64°"));
        assert_eq!(short_value(MetricId::Gpu, &d, u).as_deref(), Some("off"));
        assert_eq!(short_value(MetricId::Mem, &d, u), None);
        assert_eq!(short_value(MetricId::Disk, &d, u), None);
        let s = [Series::from_history(1.0, vec![120.0], vec![120.0])];
        assert!((y_max(MetricId::Cpu, &s, 0) - 100.0).abs() < 1e-9);
        assert!((y_max(MetricId::Mem, &s, 64) - 64.0).abs() < 1e-9);
        assert!((y_max(MetricId::Temp, &s, 0) - 200.0).abs() < 1e-9);
        assert!((y_max(MetricId::Net, &s, 0) - 200.0).abs() < 1e-9);
    }

    #[test]
    fn rows_fit() {
        assert_eq!(rows_that_fit(0.0), 3);
        assert_eq!(rows_that_fit(TABLE_TOP + ROW_HEIGHT * 20.5), 20);
    }

    #[test]
    fn reorder() {
        let mut l = vec![MetricId::Cpu, MetricId::Mem, MetricId::Net];
        move_metric(&mut l, MetricId::Mem, -1);
        assert_eq!(l, [MetricId::Mem, MetricId::Cpu, MetricId::Net]);
        move_metric(&mut l, MetricId::Mem, -1);
        assert_eq!(l[0], MetricId::Mem);
        move_metric(&mut l, MetricId::Net, 1);
        assert_eq!(l[2], MetricId::Net);
        move_metric(&mut l, MetricId::Gpu, 1);
        assert_eq!(l.len(), 3);
    }

    #[test]
    fn held_order_keeps_rows_still() {
        let p = |pid, cpu| ProcDetail { pid, cpu, ..ProcDetail::default() };
        let mut procs = vec![p(1, 9.0), p(2, 50.0), p(3, 1.0), p(4, 99.0)];
        hold_order(&mut procs, &[3, 1, 2]);
        assert_eq!(procs.iter().map(|p| p.pid).collect::<Vec<_>>(), [3, 1, 2, 4]);
    }
}
