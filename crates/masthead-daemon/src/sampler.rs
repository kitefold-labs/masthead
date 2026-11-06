//! One sample of everything the current demand needs, and the bar items and
//! detail payloads built from it.

use masthead_api::PanelItem;
use masthead_api::detail::{
    BatteryDetail, CpuDetail, Describe, Detail, DiskDetail, DiskDevDetail, DiskInfo, FsDetail, GpuDetail, GpuInfo, IfaceDetail, IfaceInfo,
    MemDetail, NetDetail, ProcDetail, SensorDetail, SensorInfo, SystemDetail,
};
use masthead_core::config::Config;
use masthead_core::format;
use masthead_core::tiers::Demand;
use masthead_core::{Level, MetricId};
use masthead_probe::cpu::{self, CpuFreq, CpuStat, StatSnapshot};
use masthead_probe::disk::{DiskRates, Disks};
use masthead_probe::gpu::{Gpu, Gpus};
use masthead_probe::hwmon::{self, Sensor, SensorKind};
use masthead_probe::mem::{Mem, MemInfo};
use masthead_probe::net::{Iface, IfaceKind, Net, NetRates};
use masthead_probe::power::{Battery, BatterySample};
use masthead_probe::procs::{self, Proc, ProcTable};
use masthead_probe::source::Buf;
use masthead_probe::system::{LoadAvg, System};
use masthead_probe::uevent::Changes;
use masthead_probe::{Roots, fs};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io;
use std::time::Instant;

/// How long GPU data can go unshown before NVML is unloaded.
const GPU_RELEASE_AFTER: std::time::Duration = std::time::Duration::from_mins(1);

/// Above this many processes the table is scanned every other second.
const BUSY_PROCESS_COUNT: usize = 1000;

#[derive(Debug)]
pub struct Sampler {
    roots: Roots,
    cpu: CpuStat,
    stat: StatSnapshot,
    prev: StatSnapshot,
    freq: CpuFreq,
    freq_mhz: Vec<f64>,
    core_pct: Vec<f32>,
    cpu_pct: f64,
    mem: Mem,
    mem_info: MemInfo,
    net: Net,
    net_rates: NetRates,
    disks: Option<Disks>,
    disk_rates: DiskRates,
    sensors: Vec<Sensor>,
    gpus: Gpus,
    batteries: Vec<Battery>,
    battery: Option<BatterySample>,
    system: System,
    load: LoadAvg,
    uptime: f64,
    procs: ProcTable,
    proc_list: Vec<Proc>,
    proc_total_at_scan: u64,
    proc_skip: bool,
    buf: Buf,
    last: Option<Instant>,
    primed: bool,
    levels: BTreeMap<MetricId, Level>,
    cpu_model: String,
    cores: u32,
    /// When GPU data was last needed; NVML is unloaded a minute after.
    gpu_wanted_at: Option<Instant>,
}

/// Values pushed into history each second.
#[derive(Debug, Clone, Copy, Default)]
pub struct HistoryPoint {
    pub cpu: f32,
    pub mem: f32,
    pub gpu: f32,
    pub net_rx: f32,
    pub net_tx: f32,
    pub disk_read: f32,
    pub disk_write: f32,
    pub temp: f32,
    pub battery: f32,
}

impl HistoryPoint {
    pub fn get(&self, series: &str) -> Option<f32> {
        Some(match series {
            "cpu" => self.cpu,
            "mem" => self.mem,
            "gpu" => self.gpu,
            "net.rx" => self.net_rx,
            "net.tx" => self.net_tx,
            "disk.read" => self.disk_read,
            "disk.write" => self.disk_write,
            "temp" => self.temp,
            "battery" => self.battery,
            _ => return None,
        })
    }
}

fn opt_f32(v: Option<f64>) -> f32 {
    v.map_or(f32::NAN, |x| x as f32)
}

fn kind_str(k: SensorKind) -> (&'static str, &'static str) {
    match k {
        SensorKind::Temp => ("temp", "°C"),
        SensorKind::Fan => ("fan", "RPM"),
        SensorKind::Voltage => ("voltage", "V"),
        SensorKind::Power => ("power", "W"),
    }
}

fn iface_kind(k: IfaceKind) -> &'static str {
    match k {
        IfaceKind::Ethernet => "ethernet",
        IfaceKind::Wireless => "wireless",
        IfaceKind::Virtual => "virtual",
        IfaceKind::Loopback => "loopback",
    }
}

impl Sampler {
    pub fn open(roots: Roots) -> io::Result<Self> {
        let stat_reader = CpuStat::open(&roots)?;
        let mut stat = StatSnapshot::default();
        let mut s = Self {
            cpu: stat_reader,
            freq: CpuFreq::open(&roots, 0),
            freq_mhz: Vec::new(),
            core_pct: Vec::new(),
            cpu_pct: 0.0,
            mem: Mem::open(&roots)?,
            mem_info: MemInfo::default(),
            net: Net::open(&roots)?,
            net_rates: NetRates::default(),
            disks: Disks::open(&roots).ok(),
            disk_rates: DiskRates::default(),
            sensors: hwmon::discover(&roots),
            gpus: Gpus::open(&roots),
            batteries: masthead_probe::power::discover(&roots),
            battery: None,
            system: System::open(&roots)?,
            load: LoadAvg::default(),
            uptime: 0.0,
            procs: ProcTable::new(&roots),
            proc_list: Vec::new(),
            proc_total_at_scan: 0,
            proc_skip: false,
            buf: Buf::with_capacity(64),
            last: None,
            primed: false,
            levels: BTreeMap::new(),
            cpu_model: cpu::model_name(&roots).unwrap_or_else(|| "CPU".into()),
            cores: 0,
            gpu_wanted_at: None,
            stat: StatSnapshot::default(),
            prev: StatSnapshot::default(),
            roots,
        };
        s.cpu.sample(&mut stat)?;
        // Describe() reports the memory total before the first tick.
        s.mem_info = s.mem.sample()?;
        let threads = stat.cores.len();
        s.cores = cpu::physical_cores(&s.roots).map_or(threads as u32, |c| c as u32);
        s.freq = CpuFreq::open(&s.roots, threads);
        s.stat = stat;
        Ok(s)
    }

    /// Forget previous counters, so the first sample after a pause does not
    /// report a spike spanning the pause.
    pub fn reprime(&mut self) {
        self.primed = false;
        self.last = None;
    }

    /// Re-read device lists after a hotplug event.
    pub fn rediscover(&mut self, c: Changes) {
        if c.net || self.net.stale {
            self.net.rediscover(&self.roots);
        }
        if c.block
            && let Some(d) = &mut self.disks
        {
            d.rediscover(&self.roots);
        }
        if c.hwmon {
            self.sensors = hwmon::discover(&self.roots);
        }
        if c.power_supply {
            self.batteries = masthead_probe::power::discover(&self.roots);
        }
        if c.drm {
            self.gpus = Gpus::open(&self.roots);
        }
    }

    pub fn net_stale(&self) -> bool {
        self.net.stale
    }

    fn bar_sensor(&self, cfg: &Config) -> Option<usize> {
        if let Some(key) = cfg.devices.temperature_preference()
            && let Some(i) = self.sensors.iter().position(|s| s.key() == key)
        {
            return Some(i);
        }
        hwmon::cpu_package(&self.sensors).or_else(|| self.sensors.iter().position(|s| s.kind == SensorKind::Temp))
    }

    fn counted(cfg: &Config, i: &Iface) -> bool {
        if !cfg.devices.interfaces.is_empty() {
            return cfg.devices.interfaces.contains(&i.name);
        }
        i.physical() || (cfg.devices.include_virtual_interfaces && i.kind == IfaceKind::Virtual)
    }

    /// Read every source the demand needs.
    pub fn sample(&mut self, demand: &Demand, cfg: &Config) -> io::Result<HistoryPoint> {
        let now = Instant::now();
        let dt = self.last.map_or(0.0, |l| now.duration_since(l).as_secs_f64());
        self.last = Some(now);

        std::mem::swap(&mut self.prev, &mut self.stat);
        self.cpu.sample(&mut self.stat)?;
        if self.primed {
            self.cpu_pct = cpu::busy_fraction(&self.prev.total, &self.stat.total).map_or(self.cpu_pct, |f| f * 100.0);
        }
        if demand.wants_cores() {
            self.core_pct.resize(self.stat.cores.len(), 0.0);
            for (i, c) in self.stat.cores.iter().enumerate() {
                let p = self.prev.cores.get(i).and_then(|p| cpu::busy_fraction(p, c)).unwrap_or(0.0);
                self.core_pct[i] = (p * 100.0) as f32;
            }
            self.freq.sample(&mut self.freq_mhz);
        }

        self.mem_info = self.mem.sample()?;
        let dt_rates = if self.primed { dt } else { 0.0 };
        self.net_rates = self.net.sample(dt_rates, |i| Self::counted(cfg, i))?;
        if let Some(d) = &mut self.disks {
            self.disk_rates = d.sample(dt_rates)?;
        }

        let detail_sensors = demand.window || demand.hover.contains(&MetricId::Temp);
        let bar_sensor = self.bar_sensor(cfg);
        for (i, s) in self.sensors.iter_mut().enumerate() {
            if detail_sensors || Some(i) == bar_sensor {
                s.read(&mut self.buf);
            }
        }
        let gpu_wanted = cfg.bar.metrics.contains(&MetricId::Gpu) || demand.window || demand.hover.contains(&MetricId::Gpu);
        if gpu_wanted {
            self.gpu_wanted_at = Some(now);
            self.gpus.sample(cfg.devices.nvml);
        } else if self.gpu_wanted_at.is_none_or(|t| now.duration_since(t) > GPU_RELEASE_AFTER) {
            self.gpus.release_nvml();
        }
        self.battery = self.batteries.first().map(|b| b.sample(&mut self.buf));
        self.load = self.system.loadavg()?;

        if demand.wants_processes() {
            let busy = self.proc_list.len() > BUSY_PROCESS_COUNT;
            self.proc_skip = busy && !self.proc_skip;
            if !self.proc_skip {
                let total = self.stat.total.total();
                let delta = if self.proc_total_at_scan == 0 { 0 } else { total.saturating_sub(self.proc_total_at_scan) };
                self.proc_list = self.procs.scan(delta);
                self.proc_total_at_scan = total;
            }
        } else if !self.proc_list.is_empty() {
            self.proc_list = Vec::new();
            self.proc_total_at_scan = 0;
        }
        let was_primed = self.primed;
        self.primed = true;

        let gpu = self.gpus.primary(cfg.devices.gpu_preference());
        Ok(HistoryPoint {
            cpu: if was_primed { self.cpu_pct as f32 } else { f32::NAN },
            mem: self.mem_info.used() as f32,
            gpu: opt_f32(gpu.and_then(|g| g.last.util)),
            net_rx: if was_primed { self.net_rates.rx as f32 } else { f32::NAN },
            net_tx: if was_primed { self.net_rates.tx as f32 } else { f32::NAN },
            disk_read: if was_primed { self.disk_rates.read as f32 } else { f32::NAN },
            disk_write: if was_primed { self.disk_rates.write as f32 } else { f32::NAN },
            temp: opt_f32(bar_sensor.and_then(|i| self.sensors[i].value)),
            battery: opt_f32(self.battery.and_then(|b| b.percent)),
        })
    }

    /// Metrics this machine can show.
    pub fn available(&self) -> Vec<MetricId> {
        let mut v = vec![MetricId::Cpu, MetricId::Mem];
        if !self.gpus.list.is_empty() {
            v.push(MetricId::Gpu);
        }
        v.push(MetricId::Net);
        if self.disks.as_ref().is_some_and(|d| !d.disks.is_empty()) {
            v.push(MetricId::Disk);
        }
        if self.sensors.iter().any(|s| s.kind == SensorKind::Temp) {
            v.push(MetricId::Temp);
        }
        if !self.batteries.is_empty() {
            v.push(MetricId::Battery);
        }
        v
    }

    fn level(&mut self, m: MetricId, value: Option<f64>, cfg: &Config) -> Level {
        let prev = self.levels.get(&m).copied().unwrap_or_default();
        let l = match (m, value) {
            (_, None) => Level::Normal,
            (MetricId::Battery, Some(pct)) => {
                let discharging = self.battery.is_some_and(|b| b.state == masthead_probe::power::ChargeState::Discharging);
                if !discharging {
                    Level::Normal
                } else if pct <= 10.0 {
                    Level::Crit
                } else if pct <= 20.0 {
                    Level::Warn
                } else {
                    Level::Normal
                }
            }
            (_, Some(v)) => cfg.threshold(m).map_or(Level::Normal, |t| t.level(v, prev)),
        };
        self.levels.insert(m, l);
        l
    }

    fn primary_gpu<'a>(&'a self, cfg: &Config) -> Option<&'a Gpu> {
        self.gpus.primary(cfg.devices.gpu_preference())
    }

    /// The bar items in configured order, skipping metrics this machine lacks.
    pub fn panel_items(&mut self, cfg: &Config) -> Vec<PanelItem> {
        let avail = self.available();
        let u = cfg.units;
        let mut out = Vec::with_capacity(cfg.bar.metrics.len());
        for &m in &cfg.bar.metrics {
            if !avail.contains(&m) {
                continue;
            }
            let (value, text, summary, level_input) = match m {
                MetricId::Cpu => {
                    let temp = self.bar_sensor(cfg).and_then(|i| self.sensors[i].value);
                    let mut s = format!("load {:.2}", self.load.one);
                    if let Some(t) = temp {
                        let _ = write!(s, " · {}", format::temp(t, u.temperature, false));
                    }
                    (self.cpu_pct, format::percent(self.cpu_pct), s, Some(self.cpu_pct))
                }
                MetricId::Mem => {
                    let used = self.mem_info.used();
                    let pct = if self.mem_info.total > 0 { used as f64 * 100.0 / self.mem_info.total as f64 } else { 0.0 };
                    let s = format!(
                        "{} of {} · swap {}",
                        format::mem_long(used, u.memory),
                        format::mem_long(self.mem_info.total, u.memory),
                        format::mem_long(self.mem_info.swap_used(), u.memory)
                    );
                    (used as f64, format::mem_short(used, u.memory), s, Some(pct))
                }
                MetricId::Gpu => {
                    let g = self.primary_gpu(cfg);
                    let util = g.and_then(|g| g.last.util);
                    let text = if g.is_some_and(|g| g.last.suspended) {
                        "off".to_owned()
                    } else {
                        util.map_or_else(|| "–".to_owned(), format::percent)
                    };
                    let mut s = g.map(|g| g.name.clone()).unwrap_or_default();
                    if let Some(g) = g {
                        if let (Some(used), Some(total)) = (g.last.vram_used, g.last.vram_total) {
                            let _ = write!(s, " · {} of {}", format::mem_long(used, u.memory), format::mem_long(total, u.memory));
                        }
                        if let Some(t) = g.last.temp_c {
                            let _ = write!(s, " · {}", format::temp(t, u.temperature, false));
                        }
                        if g.last.suspended {
                            s.push_str(" · suspended");
                        }
                    }
                    (util.unwrap_or(0.0), text, s, util)
                }
                MetricId::Net => {
                    let r = self.net_rates;
                    let text = format!("↓{} ↑{}", format::rate(r.rx, u.network, true), format::rate(r.tx, u.network, true));
                    let s = format!("↓ {}  ↑ {}", format::rate(r.rx, u.network, false), format::rate(r.tx, u.network, false));
                    (r.rx, text, s, None)
                }
                MetricId::Disk => {
                    let r = self.disk_rates;
                    let text = format!("r{} w{}", format::disk_rate(r.read, true), format::disk_rate(r.write, true));
                    let s = format!("read {} · write {}", format::disk_rate(r.read, false), format::disk_rate(r.write, false));
                    (r.read + r.write, text, s, None)
                }
                MetricId::Temp => {
                    let i = self.bar_sensor(cfg);
                    let v = i.and_then(|i| self.sensors[i].value);
                    let text = v.map_or_else(|| "–".to_owned(), |t| format::temp(t, u.temperature, true));
                    let s = i.map(|i| hwmon::display_name(&self.sensors[i])).unwrap_or_default();
                    (v.unwrap_or(0.0), text, s, v)
                }
                MetricId::Battery => {
                    let b = self.battery;
                    let pct = b.and_then(|b| b.percent);
                    let text = pct.map_or_else(|| "–".to_owned(), format::percent);
                    let s = b
                        .map(|b| {
                            let mut s = b.state.as_str().to_owned();
                            if let Some(w) = b.watts {
                                let _ = write!(s, " · {}", format::watts(w));
                            }
                            s
                        })
                        .unwrap_or_default();
                    (pct.unwrap_or(0.0), text, s, pct)
                }
            };
            let level = self.level(m, level_input, cfg);
            out.push(PanelItem { id: m.as_str().to_owned(), value, text, level: level as u32, summary });
        }
        out
    }

    fn proc_detail(&mut self, p: &Proc) -> ProcDetail {
        ProcDetail { pid: p.pid, name: p.name.clone(), user: self.procs.user_name(p.uid), cpu: p.cpu, rss: p.rss_bytes, threads: p.threads }
    }

    fn cpu_detail(&mut self, cfg: &Config, top: usize) -> CpuDetail {
        let freqs: Vec<f32> = self.freq_mhz.iter().map(|f| *f as f32).collect();
        let live: Vec<f64> = self.freq_mhz.iter().copied().filter(|f| *f > 0.0).collect();
        let avg = if live.is_empty() { 0.0 } else { live.iter().sum::<f64>() / live.len() as f64 };
        let top_list: Vec<Proc> = if top > 0 { procs::top_by_cpu(self.proc_list.clone(), top) } else { Vec::new() };
        let top = top_list.iter().map(|p| self.proc_detail(p)).collect();
        CpuDetail {
            model: self.cpu_model.clone(),
            cores: self.cores,
            threads: self.stat.cores.len() as u32,
            total: self.cpu_pct,
            per_core: self.core_pct.clone(),
            freq_mhz: freqs,
            avg_freq_mhz: avg,
            package_temp_c: self.bar_sensor(cfg).and_then(|i| self.sensors[i].value),
            load: [self.load.one, self.load.five, self.load.fifteen],
            top,
        }
    }

    fn mem_detail(&mut self, top: usize) -> MemDetail {
        let m = self.mem_info;
        let top_list: Vec<Proc> = if top > 0 { procs::top_by_rss(self.proc_list.clone(), top) } else { Vec::new() };
        MemDetail {
            total: m.total,
            used: m.used(),
            cache: m.cache(),
            available: m.available,
            free: m.free,
            swap_total: m.swap_total,
            swap_used: m.swap_used(),
            top: top_list.iter().map(|p| self.proc_detail(p)).collect(),
        }
    }

    fn gpu_details(&self, cfg: &Config) -> Vec<GpuDetail> {
        let primary = self.primary_gpu(cfg).map(|g| g.pci.clone());
        self.gpus
            .list
            .iter()
            .map(|g| GpuDetail {
                pci: g.pci.clone(),
                name: g.name.clone(),
                vendor: g.vendor.as_str().to_owned(),
                driver: g.driver.clone().unwrap_or_default(),
                discrete: g.discrete,
                primary: primary.as_deref() == Some(g.pci.as_str()),
                util: g.last.util,
                vram_used: g.last.vram_used,
                vram_total: g.last.vram_total,
                temp_c: g.last.temp_c,
                power_w: g.last.power_w,
                power_limit_w: g.power_limit_w,
                suspended: g.last.suspended,
            })
            .collect()
    }

    fn net_detail(&self, cfg: &Config) -> NetDetail {
        let show_virtual = cfg.devices.include_virtual_interfaces;
        let mut hidden = 0;
        let interfaces = self
            .net
            .ifaces
            .iter()
            .filter(|i| {
                let shown = i.physical() || (show_virtual && i.kind == IfaceKind::Virtual) || cfg.devices.interfaces.contains(&i.name);
                if !shown && i.kind == IfaceKind::Virtual {
                    hidden += 1;
                }
                shown
            })
            .map(|i| IfaceDetail {
                name: i.name.clone(),
                kind: iface_kind(i.kind).to_owned(),
                driver: i.driver.clone().unwrap_or_default(),
                speed_mbps: i.speed_mbps,
                rx: i.rx_rate,
                tx: i.tx_rate,
                rx_total: i.rx_bytes,
                tx_total: i.tx_bytes,
                counted: Self::counted(cfg, i),
            })
            .collect();
        NetDetail { rx: self.net_rates.rx, tx: self.net_rates.tx, interfaces, hidden_virtual: hidden }
    }

    fn disk_detail(&self, with_fs: bool) -> DiskDetail {
        let devices = self
            .disks
            .as_ref()
            .map(|d| {
                d.disks
                    .iter()
                    .map(|x| DiskDevDetail {
                        name: x.name.clone(),
                        model: x.model.clone().unwrap_or_default(),
                        size: x.size_bytes,
                        read: x.read_rate,
                        write: x.write_rate,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let filesystems = if with_fs {
            fs::usage(&self.roots)
                .into_iter()
                .map(|f| FsDetail { mount: f.mount, device: f.device, fstype: f.fstype, total: f.total_bytes, used: f.used_bytes })
                .collect()
        } else {
            Vec::new()
        };
        DiskDetail { read: self.disk_rates.read, write: self.disk_rates.write, devices, filesystems }
    }

    fn sensor_details(&self) -> Vec<SensorDetail> {
        let mut v: Vec<SensorDetail> = self
            .sensors
            .iter()
            .map(|s| {
                let (kind, unit) = kind_str(s.kind);
                SensorDetail { key: s.key(), name: hwmon::display_name(s), kind: kind.into(), value: s.value, unit: unit.into() }
            })
            .collect();
        // NVIDIA's proprietary driver exposes no hwmon; add its NVML temperature.
        for g in &self.gpus.list {
            if g.vendor == masthead_probe::gpu::Vendor::Nvidia
                && let Some(t) = g.last.temp_c
            {
                v.push(SensorDetail {
                    key: format!("gpu/{}", g.pci),
                    name: g.name.clone(),
                    kind: "temp".into(),
                    value: Some(t),
                    unit: "°C".into(),
                });
            }
        }
        v
    }

    fn battery_details(&mut self) -> Vec<BatteryDetail> {
        let mut out = Vec::new();
        for b in &self.batteries {
            let s = b.sample(&mut self.buf);
            out.push(BatteryDetail { name: b.name.clone(), percent: s.percent, watts: s.watts, state: s.state.as_str().into() });
        }
        out
    }

    fn system_detail(&mut self) -> SystemDetail {
        self.uptime = self.system.uptime().unwrap_or(self.uptime);
        SystemDetail {
            uptime: self.uptime,
            load: [self.load.one, self.load.five, self.load.fifteen],
            procs_total: self.load.total,
            procs_running: self.load.running,
        }
    }

    /// Detail for one metric (`with_procs` adds the top processes), or for
    /// every metric plus the process table when `metric` is `None`.
    pub fn detail(&mut self, metric: Option<MetricId>, cfg: &Config, with_procs: bool, t: u64) -> Detail {
        let mut d = Detail { t, ..Detail::default() };
        let top = if with_procs { 5 } else { 0 };
        match metric {
            Some(MetricId::Cpu) => {
                d.cpu = Some(self.cpu_detail(cfg, top));
                d.system = Some(self.system_detail());
            }
            Some(MetricId::Mem) => d.mem = Some(self.mem_detail(top)),
            Some(MetricId::Gpu) => d.gpus = Some(self.gpu_details(cfg)),
            Some(MetricId::Net) => d.net = Some(self.net_detail(cfg)),
            Some(MetricId::Disk) => d.disk = Some(self.disk_detail(false)),
            Some(MetricId::Temp) => d.sensors = Some(self.sensor_details()),
            Some(MetricId::Battery) => d.batteries = Some(self.battery_details()),
            None => {
                d.system = Some(self.system_detail());
                d.cpu = Some(self.cpu_detail(cfg, top));
                d.mem = Some(self.mem_detail(top));
                d.gpus = Some(self.gpu_details(cfg));
                d.net = Some(self.net_detail(cfg));
                d.disk = Some(self.disk_detail(true));
                d.sensors = Some(self.sensor_details());
                d.batteries = Some(self.battery_details());
                if with_procs {
                    let mut list = self.proc_list.clone();
                    list.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then(b.rss_bytes.cmp(&a.rss_bytes)));
                    list.truncate(200);
                    d.processes = Some(list.iter().map(|p| self.proc_detail(p)).collect());
                }
            }
        }
        d
    }

    pub fn describe(&self, version: &str) -> Describe {
        let uname = rustix::system::uname();
        Describe {
            version: version.to_owned(),
            hostname: uname.nodename().to_string_lossy().into_owned(),
            kernel: uname.release().to_string_lossy().into_owned(),
            cpu_model: self.cpu_model.clone(),
            cores: self.cores,
            threads: self.stat.cores.len() as u32,
            mem_total: self.mem_info.total,
            gpus: self
                .gpus
                .list
                .iter()
                .map(|g| GpuInfo { pci: g.pci.clone(), name: g.name.clone(), vendor: g.vendor.as_str().into(), discrete: g.discrete })
                .collect(),
            sensors: self
                .sensors
                .iter()
                .map(|s| SensorInfo { key: s.key(), name: hwmon::display_name(s), kind: kind_str(s.kind).0.into() })
                .collect(),
            interfaces: self.net.ifaces.iter().map(|i| IfaceInfo { name: i.name.clone(), kind: iface_kind(i.kind).into() }).collect(),
            disks: self
                .disks
                .as_ref()
                .map(|d| {
                    d.disks
                        .iter()
                        .map(|x| DiskInfo { name: x.name.clone(), model: x.model.clone().unwrap_or_default(), size: x.size_bytes })
                        .collect()
                })
                .unwrap_or_default(),
            has_battery: !self.batteries.is_empty(),
            metrics: self.available().iter().map(|m| m.as_str().to_owned()).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use masthead_core::tiers::Demand;

    /// A minimal fixture machine.
    fn machine() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let p = d.path();
        std::fs::create_dir_all(p.join("proc/net")).unwrap();
        std::fs::create_dir_all(p.join("proc/self")).unwrap();
        std::fs::write(p.join("proc/stat"), "cpu  100 0 0 900 0 0 0 0\ncpu0 50 0 0 450 0 0 0 0\ncpu1 50 0 0 450 0 0 0 0\n").unwrap();
        std::fs::write(p.join("proc/meminfo"), "MemTotal: 1048576 kB\nMemAvailable: 524288 kB\nSwapTotal: 0 kB\nSwapFree: 0 kB\n").unwrap();
        std::fs::write(p.join("proc/net/dev"), "a\nb\n").unwrap();
        std::fs::write(p.join("proc/diskstats"), "").unwrap();
        std::fs::write(p.join("proc/loadavg"), "1.50 1.00 0.50 2/100 1\n").unwrap();
        std::fs::write(p.join("proc/uptime"), "100.0 0\n").unwrap();
        std::fs::write(p.join("proc/self/mounts"), "").unwrap();
        std::fs::write(p.join("proc/cpuinfo"), "model name\t: Test CPU\n").unwrap();
        let h = p.join("sys/class/hwmon/hwmon0");
        std::fs::create_dir_all(&h).unwrap();
        std::fs::write(h.join("name"), "k10temp\n").unwrap();
        std::fs::write(h.join("temp1_input"), "91000\n").unwrap();
        std::fs::write(h.join("temp1_label"), "Tctl\n").unwrap();
        d
    }

    #[test]
    fn samples_fixture_machine() {
        let d = machine();
        let mut s = Sampler::open(Roots::under(d.path())).unwrap();
        let cfg = Config::default();
        let demand = Demand { panel: true, ..Demand::default() };
        let first = s.sample(&demand, &cfg).unwrap();
        assert!(first.cpu.is_nan(), "first sample has no CPU delta");
        std::fs::write(d.path().join("proc/stat"), "cpu  200 0 0 1000 0 0 0 0\ncpu0 100 0 0 500 0 0 0 0\ncpu1 100 0 0 500 0 0 0 0\n")
            .unwrap();
        let p = s.sample(&demand, &cfg).unwrap();
        assert!((p.cpu - 50.0).abs() < 1e-3);
        assert!((p.mem - 536_870_912.0).abs() < 1.0);
        assert!((p.temp - 91.0).abs() < 1e-3);
        assert!(p.gpu.is_nan());
        assert_eq!(p.get("net.rx"), Some(0.0));
        assert_eq!(p.get("nope"), None);

        let items = s.panel_items(&cfg);
        let ids: Vec<_> = items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["cpu", "mem", "net", "temp"], "no GPU in the fixture");
        assert_eq!(items[0].text, "50%");
        assert_eq!(items[0].summary, "load 1.50 · 91 °C");
        assert_eq!(items[1].text, "0.5G");
        assert_eq!(items[3].text, "91°");
        assert_eq!(items[3].level, Level::Crit as u32);

        let all = s.detail(None, &cfg, true, 7);
        assert_eq!(all.t, 7);
        assert_eq!(all.cpu.as_ref().unwrap().model, "Test CPU");
        assert_eq!(all.sensors.as_ref().unwrap().len(), 1);
        assert!(all.processes.is_some());
        let cpu = s.detail(Some(MetricId::Cpu), &cfg, false, 0);
        assert!(cpu.cpu.unwrap().top.is_empty());
        let desc = s.describe("0.0.0");
        assert_eq!(desc.mem_total, 1_073_741_824);
        assert_eq!(desc.metrics, ["cpu", "mem", "net", "temp"]);
        s.reprime();
        assert!(s.sample(&demand, &cfg).unwrap().cpu.is_nan());
        s.rediscover(Changes { net: true, block: true, hwmon: true, power_supply: true, drm: true });
        assert!(!s.net_stale());
    }
}
