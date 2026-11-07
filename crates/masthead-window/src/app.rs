// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! The libcosmic application: state, messages, update and the top-level
//! layout. Page bodies live in [`crate::views`].

use crate::bus::{Command, EVENTS, Event, Req};
use crate::chart::Cache;
use crate::disk::{Disk, DiskMsg};
use crate::kill::{self, Signal, Target};
use crate::model::{self, Page, Range, Series, Sort, SortKey};
use crate::palette::{self, Palette};
use crate::views;
use cosmic::app::{Core, Task};
use cosmic::iced::{Length, Subscription};
use cosmic::{Application, ApplicationExt, Element};
use masthead_api::PanelItem;
use masthead_api::detail::{Describe, Detail};
use masthead_core::MetricId;
use masthead_core::config::{BarStyle, Config, MemoryUnits, NetworkUnits, TemperatureUnits, Theme, TrayMode};
use masthead_core::thresholds::Threshold;
use std::collections::{BTreeMap, HashMap};
use tokio::sync::mpsc::UnboundedSender;

pub const APP_ID: &str = "com.kitefoldlabs.Masthead";

/// While the process table shows, the window takes one sample in this many.
/// Every update redraws the whole window in software, and the table is the
/// most expensive page to draw. Three seconds matches GNOME System Monitor.
pub const PROCESS_EVERY: u32 = 3;

/// Detail samples between refetches of 1-minute history for the 1 h range.
const COARSE_REFRESH: u32 = 60;

#[derive(Debug, Clone)]
pub enum Message {
    Bus(Event),
    Select(Page),
    Range(Range),
    Sort(SortKey),
    Filter(String),
    BarMetric(MetricId, bool),
    MoveMetric(MetricId, i32),
    BarStyle(usize),
    MemUnits(usize),
    NetUnits(usize),
    TempUnits(usize),
    ThresholdInput(MetricId, bool, String),
    ApplyThresholds,
    Gpu(usize),
    TempSensor(usize),
    Virtual(bool),
    Tray(usize),
    Theme(usize),
    Notify(bool),
    HoldInput(String),
    ApplyHold,
    /// The kill button on a process row: ask to confirm.
    AskKill(u32),
    CancelKill,
    Kill(Signal),
    Killed(u32, Result<String, String>),
    /// The pointer entered (`true`) or left the process rows.
    HoldRows(bool),
    /// Opens the Disk usage page, scanning if it has not yet.
    OpenDiskUsage,
    Disk(DiskMsg),
}

/// Startup state handed to [`App::init`].
#[derive(Debug)]
pub struct Flags {
    pub page: Page,
    /// The desktop's preference.
    pub dark: bool,
    /// The theme setting, read from the config file before the first frame.
    pub theme: Theme,
    pub requests: UnboundedSender<Req>,
}

pub struct App {
    core: Core,
    pub pal: Palette,
    /// The desktop's light or dark preference; the theme setting can override it.
    system_dark: bool,
    pub page: Page,
    pub range: Range,
    pub describe: Option<Describe>,
    pub config: Option<Config>,
    pub panel: Vec<PanelItem>,
    pub detail: Option<Detail>,
    /// Series for the current page's chart, in [`model::series_for`] order.
    pub chart: Vec<Series>,
    pub chart_cache: Cache,
    generation: u64,
    coarse_ticks: u32,
    /// The last 60 s of every series, for the sidebar.
    pub spark: HashMap<&'static str, Series>,
    pub spark_cache: HashMap<MetricId, Cache>,
    pub sort: Sort,
    pub filter: String,
    /// The process table from the latest window-tier sample.
    pub procs: Vec<masthead_api::detail::ProcDetail>,
    pub thresholds: BTreeMap<MetricId, (String, String)>,
    pub hold: String,
    pub error: Option<String>,
    /// The process awaiting confirmation in the kill dialog.
    pub kill: Option<Target>,
    /// True while `kill` or the password dialog runs.
    pub killing: bool,
    /// What the last kill did, shown on the Processes page.
    pub kill_result: Option<Result<String, String>>,
    /// Process row order frozen while the pointer is over the rows, so a row
    /// does not move under it between aiming and clicking.
    pub held_order: Option<Vec<u32>>,
    pub disk: Disk,
    requests: UnboundedSender<Req>,
}

/// Dark or light, from the theme setting and the desktop's preference.
pub fn effective_dark(theme: Theme, system_dark: bool) -> bool {
    match theme {
        Theme::System => system_dark,
        Theme::Dark => true,
        Theme::Light => false,
    }
}

/// Bar metrics the thresholds editor offers.
pub const THRESHOLD_METRICS: [MetricId; 4] = [MetricId::Cpu, MetricId::Mem, MetricId::Gpu, MetricId::Temp];

pub const STYLE_OPTIONS: [&str; 3] = ["Values", "Values and trend", "Compact trend"];
pub const MEM_OPTIONS: [&str; 2] = ["Binary (GiB)", "Decimal (GB)"];
pub const NET_OPTIONS: [&str; 2] = ["Bytes (MB/s)", "Bits (Mb/s)"];
pub const TEMP_OPTIONS: [&str; 2] = ["Celsius", "Fahrenheit"];
pub const TRAY_OPTIONS: [&str; 3] = ["Only without a native bar", "Always", "Never"];
pub const THEME_OPTIONS: [&str; 3] = ["Follow the system", "Dark", "Light"];
pub const THEMES: [Theme; 3] = [Theme::System, Theme::Dark, Theme::Light];

impl App {
    fn send(&self, r: Req) {
        let _ = self.requests.send(r);
    }

    /// Metrics this machine has, in sidebar order.
    pub fn metrics(&self) -> Vec<MetricId> {
        let avail: Vec<MetricId> =
            self.describe.as_ref().map(|d| d.metrics.iter().filter_map(|m| m.parse().ok()).collect()).unwrap_or_default();
        MetricId::ALL.into_iter().filter(|m| avail.contains(m)).collect()
    }

    /// Total memory, from the live sample when the description lacks it.
    pub fn mem_total(&self) -> u64 {
        self.detail
            .as_ref()
            .and_then(|d| d.mem.as_ref())
            .map(|m| m.total)
            .filter(|t| *t > 0)
            .or_else(|| self.describe.as_ref().map(|d| d.mem_total))
            .unwrap_or(0)
    }

    pub fn units(&self) -> masthead_core::config::Units {
        self.config.as_ref().map(|c| c.units).unwrap_or_default()
    }

    /// Ask for history covering the current page and range.
    fn load_chart(&mut self) {
        self.generation += 1;
        self.coarse_ticks = 0;
        let Page::Metric(m) = self.page else {
            self.chart.clear();
            return;
        };
        let names = model::series_for(m);
        self.chart = vec![Series::default(); names.len()];
        self.chart_cache.clear();
        for (index, series) in names.iter().enumerate() {
            self.send(Req::History { generation: self.generation, index, series, seconds: self.range.seconds() });
        }
    }

    fn select(&mut self, p: Page) {
        if self.page != p {
            if self.page == Page::Processes {
                // Sparklines skipped samples while the process table showed.
                self.send(Req::Sparks);
            }
            self.send(Req::DetailEvery(if p == Page::Processes { PROCESS_EVERY } else { 1 }));
            self.page = p;
            self.load_chart();
        }
    }

    fn on_detail(&mut self, mut d: Detail) {
        if let Some(p) = d.processes.take() {
            self.procs = p;
        }
        for s in masthead_api::SERIES {
            let v = model::series_value(&d, s);
            self.spark.entry(s).or_default().push(v, 60);
        }
        for c in self.spark_cache.values() {
            c.clear();
        }
        if let Page::Metric(m) = self.page {
            if self.range.step() <= 1.0 {
                let cap = self.range.points();
                for (i, s) in model::series_for(m).iter().enumerate() {
                    if let Some(series) = self.chart.get_mut(i) {
                        series.step = 1.0;
                        series.push(model::series_value(&d, s), cap);
                    }
                }
                self.chart_cache.clear();
            } else {
                self.coarse_ticks += 1;
                if self.coarse_ticks >= COARSE_REFRESH {
                    self.load_chart();
                }
            }
        }
        self.detail = Some(d);
    }

    fn on_config(&mut self, c: Config) {
        self.thresholds = THRESHOLD_METRICS
            .iter()
            .map(|m| {
                let t = c.thresholds.get(m);
                (*m, (t.map(|t| fmt_num(t.warn)).unwrap_or_default(), t.map(|t| fmt_num(t.crit)).unwrap_or_default()))
            })
            .collect();
        self.hold = c.notifications.hold_seconds.to_string();
        self.config = Some(c);
    }

    /// Switches the palette if the theme setting or the desktop's preference
    /// now calls for the other one.
    fn apply_theme(&mut self) -> Task<Message> {
        let theme = self.config.as_ref().map_or(Theme::System, |c| c.window.theme);
        let dark = effective_dark(theme, self.system_dark);
        if self.pal.dark == dark {
            return Task::none();
        }
        self.pal = Palette::for_dark(dark);
        self.chart_cache.clear();
        for c in self.spark_cache.values() {
            c.clear();
        }
        cosmic::command::set_theme(palette::cosmic_theme(&self.pal))
    }

    /// Change the configuration locally and send it to the daemon.
    fn edit(&mut self, f: impl FnOnce(&mut Config)) {
        if let Some(c) = &mut self.config {
            f(c);
            self.error = None;
            let c = c.clone();
            self.send(Req::SetConfig(Box::new(c)));
        }
    }

    pub fn gpu_choices(&self) -> (Vec<String>, Vec<String>) {
        let mut labels = vec!["Automatic (busiest discrete GPU)".to_owned()];
        let mut keys = vec!["auto".to_owned()];
        for g in self.describe.iter().flat_map(|d| &d.gpus) {
            labels.push(format!("{} ({})", g.name, g.pci));
            keys.push(g.pci.clone());
        }
        (labels, keys)
    }

    pub fn sensor_choices(&self) -> (Vec<String>, Vec<String>) {
        let mut labels = vec!["Automatic (CPU package)".to_owned()];
        let mut keys = vec!["auto".to_owned()];
        for s in self.describe.iter().flat_map(|d| &d.sensors).filter(|s| s.kind == "temp") {
            labels.push(s.name.clone());
            keys.push(s.key.clone());
        }
        (labels, keys)
    }
}

fn fmt_num(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 { format!("{}", v.round() as i64) } else { format!("{v}") }
}

fn events() -> impl futures_util::Stream<Item = Message> {
    let rx = EVENTS.lock().ok().and_then(|mut g| g.take());
    futures_util::stream::unfold(rx, |rx| async move {
        let mut rx = rx?;
        let ev = rx.recv().await?;
        Some((Message::Bus(ev), Some(rx)))
    })
}

impl Application for App {
    type Executor = cosmic::executor::Default;
    type Flags = Flags;
    type Message = Message;
    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(mut core: Core, flags: Flags) -> (Self, Task<Message>) {
        // The name sits beside the logo in header_start, not centered.
        core.window.header_title = String::new();
        core.window.show_maximize = true;
        core.window.content_container = false;
        let mut app = App {
            core,
            pal: Palette::for_dark(effective_dark(flags.theme, flags.dark)),
            system_dark: flags.dark,
            page: flags.page,
            range: Range::default(),
            describe: None,
            config: None,
            panel: Vec::new(),
            detail: None,
            chart: Vec::new(),
            chart_cache: Cache::new(),
            generation: 0,
            coarse_ticks: 0,
            spark: HashMap::new(),
            spark_cache: MetricId::ALL.iter().map(|m| (*m, Cache::new())).collect(),
            sort: Sort::default(),
            filter: String::new(),
            procs: Vec::new(),
            thresholds: BTreeMap::new(),
            hold: String::new(),
            error: None,
            kill: None,
            killing: false,
            kill_result: None,
            held_order: None,
            disk: Disk::default(),
            requests: flags.requests,
        };
        app.send(Req::Start);
        if app.page == Page::Processes {
            app.send(Req::DetailEvery(PROCESS_EVERY));
        }
        app.load_chart();
        let title = match app.core.main_window_id() {
            Some(id) => app.set_window_title("Masthead".into(), id),
            None => Task::none(),
        };
        let scan = app.scan_if_new();
        (app, Task::batch([title, scan]))
    }

    fn header_start(&self) -> Vec<Element<'_, Message>> {
        vec![views::brand(&self.pal)]
    }

    fn subscription(&self) -> Subscription<Message> {
        let bus = Subscription::run(events);
        if self.disk.scanning() {
            let tick = cosmic::iced::time::every(std::time::Duration::from_millis(250)).map(|_| Message::Disk(DiskMsg::Tick));
            Subscription::batch([bus, tick])
        } else {
            bus
        }
    }

    fn on_close_requested(&self, _id: cosmic::iced::window::Id) -> Option<Message> {
        self.send(Req::Quit);
        None
    }

    #[allow(clippy::too_many_lines)] // one arm per message
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Bus(ev) => match ev {
                Event::Command(Command::Toggle(p)) => {
                    if p == self.page {
                        self.send(Req::Quit);
                    } else {
                        self.select(p);
                        return Task::batch([self.raise(), self.scan_if_new()]);
                    }
                }
                Event::Command(Command::Show(p)) => {
                    self.select(p);
                    return Task::batch([self.raise(), self.scan_if_new()]);
                }
                Event::Describe(d) => {
                    self.describe = Some(*d);
                    // The requested metric may not exist here (no battery).
                    if let Page::Metric(m) = self.page
                        && !self.metrics().contains(&m)
                    {
                        self.select(Page::Metric(MetricId::Cpu));
                    }
                    self.error = None;
                }
                Event::Config(c) => {
                    self.on_config(*c);
                    return self.apply_theme();
                }
                Event::Panel(items) => self.panel = items,
                Event::Detail(d) => self.on_detail(*d),
                Event::History { generation, index, series } => {
                    if generation == self.generation
                        && let Some(s) = self.chart.get_mut(index)
                    {
                        *s = series;
                        self.chart_cache.clear();
                    }
                }
                Event::Spark { series, data } => {
                    self.spark.insert(series, data);
                    for c in self.spark_cache.values() {
                        c.clear();
                    }
                }
                Event::Dark(d) => {
                    self.system_dark = d;
                    return self.apply_theme();
                }
                Event::Error(e) => self.error = Some(e),
            },
            Message::Select(p) => self.select(p),
            Message::Range(r) => {
                if r != self.range {
                    self.range = r;
                    self.load_chart();
                }
            }
            Message::Sort(k) => self.sort = self.sort.click(k),
            Message::Filter(f) => self.filter = f,
            Message::BarMetric(m, on) => self.edit(|c| {
                if on {
                    if !c.bar.metrics.contains(&m) {
                        c.bar.metrics.push(m);
                    }
                } else {
                    c.bar.metrics.retain(|x| *x != m);
                }
            }),
            Message::MoveMetric(m, d) => self.edit(|c| model::move_metric(&mut c.bar.metrics, m, d)),
            Message::BarStyle(i) => self.edit(|c| c.bar.style = [BarStyle::Values, BarStyle::Trend, BarStyle::Compact][i.min(2)]),
            Message::MemUnits(i) => self.edit(|c| c.units.memory = if i == 0 { MemoryUnits::Binary } else { MemoryUnits::Decimal }),
            Message::NetUnits(i) => self.edit(|c| c.units.network = if i == 0 { NetworkUnits::Bytes } else { NetworkUnits::Bits }),
            Message::TempUnits(i) => {
                self.edit(|c| c.units.temperature = if i == 0 { TemperatureUnits::Celsius } else { TemperatureUnits::Fahrenheit });
            }
            Message::ThresholdInput(m, warn, v) => {
                if let Some(e) = self.thresholds.get_mut(&m) {
                    if warn { e.0 = v } else { e.1 = v }
                }
            }
            Message::ApplyThresholds => match parse_thresholds(&self.thresholds) {
                Ok(t) => self.edit(|c| {
                    for m in THRESHOLD_METRICS {
                        c.thresholds.remove(&m);
                    }
                    c.thresholds.extend(t);
                }),
                Err(e) => self.error = Some(e),
            },
            Message::Gpu(i) => {
                let (_, keys) = self.gpu_choices();
                if let Some(k) = keys.get(i).cloned() {
                    self.edit(|c| c.devices.gpu = k);
                }
            }
            Message::TempSensor(i) => {
                let (_, keys) = self.sensor_choices();
                if let Some(k) = keys.get(i).cloned() {
                    self.edit(|c| c.devices.temperature = k);
                }
            }
            Message::Virtual(on) => self.edit(|c| c.devices.include_virtual_interfaces = on),
            Message::Theme(i) => {
                self.edit(|c| c.window.theme = THEMES[i.min(2)]);
                return self.apply_theme();
            }
            Message::Tray(i) => self.edit(|c| c.tray.mode = [TrayMode::Auto, TrayMode::Always, TrayMode::Never][i.min(2)]),
            Message::Notify(on) => self.edit(|c| c.notifications.enabled = on),
            Message::HoldInput(v) => self.hold = v,
            Message::ApplyHold => match self.hold.trim().parse::<u64>() {
                Ok(n) if (5..=3600).contains(&n) => self.edit(|c| c.notifications.hold_seconds = n),
                _ => self.error = Some("Hold time must be a whole number of seconds from 5 to 3600.".into()),
            },
            Message::AskKill(pid) => {
                let found = self.procs.iter().find(|p| p.pid == pid).and_then(|p| Target::new(pid, p.name.clone(), p.user.clone()));
                match found {
                    Some(t) => self.kill = Some(t),
                    None => self.kill_result = Some(Err(format!("Process {pid} had already exited."))),
                }
            }
            Message::CancelKill => self.kill = None,
            Message::Kill(signal) => {
                if let Some(target) = self.kill.take() {
                    self.killing = true;
                    self.kill_result = None;
                    let pid = target.pid;
                    return cosmic::task::future(async move {
                        let done = tokio::task::spawn_blocking(move || kill::send(&target, signal)).await;
                        Message::Killed(pid, done.unwrap_or_else(|e| Err(format!("Could not end the process: {e}"))))
                    });
                }
            }
            Message::Killed(pid, result) => {
                self.killing = false;
                // Drop the row now rather than at the next scan. A process
                // that ignored SIGTERM comes back with that scan.
                if result.is_ok() {
                    self.procs.retain(|p| p.pid != pid);
                }
                self.kill_result = Some(result);
            }
            Message::OpenDiskUsage => {
                self.select(Page::DiskUsage);
                return self.scan_if_new();
            }
            Message::Disk(m) => return self.disk.update(m),
            Message::HoldRows(hold) => {
                self.held_order = hold.then(|| {
                    let mut procs = self.procs.clone();
                    self.sort.apply(&mut procs);
                    procs.iter().map(|p| p.pid).collect()
                });
            }
        }
        Task::none()
    }

    fn dialog(&self) -> Option<Element<'_, Message>> {
        self.kill.as_ref().map(views::kill_dialog).or_else(|| views::disk_page::dialog(self))
    }

    fn view(&self) -> Element<'_, Message> {
        let side = views::sidebar(self);
        let main = views::main(self);
        cosmic::iced::widget::row![side, main].width(Length::Fill).height(Length::Fill).into()
    }
}

impl App {
    /// Starts the first disk scan when the Disk usage page opens.
    fn scan_if_new(&mut self) -> Task<Message> {
        if self.page == Page::DiskUsage && self.disk.scan.is_none() && !self.disk.scanning() { self.disk.start() } else { Task::none() }
    }

    fn raise(&self) -> Task<Message> {
        match self.core.main_window_id() {
            Some(id) => cosmic::iced::window::gain_focus(id),
            None => Task::none(),
        }
    }
}

/// Parse the thresholds editor. An empty pair removes that metric's threshold.
pub fn parse_thresholds(drafts: &BTreeMap<MetricId, (String, String)>) -> Result<BTreeMap<MetricId, Threshold>, String> {
    let mut out = BTreeMap::new();
    for (m, (w, c)) in drafts {
        let (w, c) = (w.trim(), c.trim());
        if w.is_empty() && c.is_empty() {
            continue;
        }
        let warn: f64 = w.parse().map_err(|_| format!("{}: warn must be a number.", m.title()))?;
        let crit: f64 = c.parse().map_err(|_| format!("{}: critical must be a number.", m.title()))?;
        if warn > crit {
            return Err(format!("{}: warn must not be above critical.", m.title()));
        }
        out.insert(*m, Threshold { warn, crit });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_setting_overrides_the_desktop() {
        assert!(effective_dark(Theme::System, true));
        assert!(!effective_dark(Theme::System, false));
        assert!(effective_dark(Theme::Dark, false));
        assert!(!effective_dark(Theme::Light, true));
        assert_eq!(THEMES.len(), THEME_OPTIONS.len());
    }

    #[test]
    fn thresholds_editor() {
        let mut d = BTreeMap::new();
        d.insert(MetricId::Cpu, ("85".to_owned(), "95".to_owned()));
        d.insert(MetricId::Gpu, (String::new(), String::new()));
        let t = parse_thresholds(&d).unwrap();
        assert_eq!(t.len(), 1);
        assert!((t[&MetricId::Cpu].crit - 95.0).abs() < 1e-9);
        d.insert(MetricId::Temp, ("90".to_owned(), "80".to_owned()));
        assert!(parse_thresholds(&d).unwrap_err().contains("Temperature"));
        d.insert(MetricId::Temp, ("x".to_owned(), "80".to_owned()));
        assert!(parse_thresholds(&d).unwrap_err().contains("warn"));
        d.insert(MetricId::Temp, ("70".to_owned(), "y".to_owned()));
        assert!(parse_thresholds(&d).unwrap_err().contains("critical"));
        assert_eq!(fmt_num(85.0), "85");
        assert_eq!(fmt_num(85.5), "85.5");
    }
}
