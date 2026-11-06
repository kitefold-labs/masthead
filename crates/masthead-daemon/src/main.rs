// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! `masthead-daemon`: the only Masthead process that reads the kernel.
//!
//! One thread, one tokio current-thread runtime. Once a second it samples what
//! the current subscribers need, records history, and publishes bar items when
//! a visible value changes. With no subscribers, or while the session is
//! locked, it samples nothing.

mod glyphs;
mod log;
mod logind;
mod notify;
mod sampler;
mod service;
mod state;
mod tray;
mod watch;
mod window;

use futures_util::StreamExt;
use masthead_api::{BUS_NAME, INTERFACE, OBJECT_PATH, PanelItem};
use masthead_core::MetricId;
use masthead_core::config::{Config, TrayMode};
use masthead_core::thresholds::SustainedAlert;
use masthead_probe::Roots;
use masthead_probe::uevent::UeventSocket;
use sampler::Sampler;
use service::Service;
use state::{Request, Shared, SharedRef, lock};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use zbus::Connection;

const USAGE: &str = "masthead-daemon — samples system statistics for Masthead's bar faces

USAGE:
    masthead-daemon            Run on the session bus (normally started by D-Bus activation)
    masthead-daemon --print    Sample twice, print bar items and full detail as JSON, exit
    masthead-daemon --version

ENVIRONMENT:
    MASTHEAD_LOG=error|warn|info|debug   Log level (default warn)";

/// How long after startup to wait for a native face before showing tray items.
const TRAY_GRACE: Duration = Duration::from_secs(4);
/// How long after a native face was last subscribed before tray items may
/// appear. The GNOME extension unsubscribes on lock and resubscribes after
/// unlock; tray items must not flash up in between.
const NATIVE_FACE_GRACE: Duration = Duration::from_secs(30);

fn main() {
    log::init();
    let arg = std::env::args().nth(1);
    match arg.as_deref() {
        Some("--version" | "-V") => println!("masthead-daemon {}", env!("CARGO_PKG_VERSION")),
        Some("--help" | "-h") => println!("{USAGE}"),
        Some("--print") => {
            if let Err(e) = print_once() {
                eprintln!("masthead-daemon: {e}");
                std::process::exit(1);
            }
        }
        None => {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tokio runtime");
            if let Err(e) = rt.block_on(run()) {
                ::log::error!("{e}");
                eprintln!("masthead-daemon: {e}");
                std::process::exit(1);
            }
        }
        Some(other) => {
            eprintln!("masthead-daemon: unknown argument {other:?}\n\n{USAGE}");
            std::process::exit(2);
        }
    }
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn print_once() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = Config::load(&Config::default_path()).unwrap_or_default();
    let mut s = Sampler::open(Roots::default())?;
    let demand = masthead_core::tiers::Demand { panel: true, window: true, ..Default::default() };
    s.sample(&demand, &cfg)?;
    std::thread::sleep(Duration::from_secs(1));
    s.sample(&demand, &cfg)?;
    let out = serde_json::json!({
        "panel": s.panel_items(&cfg),
        "describe": s.describe(env!("CARGO_PKG_VERSION")),
        "detail": s.detail(None, &cfg, true, unix_now()),
    });
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

fn changed(a: &[PanelItem], b: &[PanelItem]) -> bool {
    a.len() != b.len() || a.iter().zip(b).any(|(x, y)| x.id != y.id || x.text != y.text || x.level != y.level || x.summary != y.summary)
}

#[allow(clippy::too_many_lines)] // one loop, read top to bottom
async fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Let the kernel batch our wakeups with others by up to 50 ms.
    let _ = rustix::thread::set_current_timer_slack(std::num::NonZeroU64::new(50_000_000));

    let config_path = Config::default_path();
    let config = Config::load(&config_path).unwrap_or_else(|e| {
        ::log::warn!("{}: {e}; using defaults", config_path.display());
        Config::default()
    });
    let mut sampler = Sampler::open(Roots::default())?;
    let shared: SharedRef = Arc::new(Mutex::new(Shared::new(config, config_path.clone())));
    lock(&shared).describe = serde_json::to_string(&sampler.describe(env!("CARGO_PKG_VERSION")))?;

    let (tx, mut rx) = mpsc::channel::<Request>(32);
    let service = Service { shared: shared.clone(), requests: tx.clone() };
    let conn = zbus::connection::Builder::session()
        .serve_at(OBJECT_PATH, service)
        .name(BUS_NAME)
        .build()
        .await
        .map_err(|e| format!("could not take {BUS_NAME} on the session bus (is another daemon running?): {e}"))?;

    let started = Instant::now();
    let watcher_present = Arc::new(std::sync::atomic::AtomicBool::new(false));
    spawn_owner_watch(&conn, shared.clone(), tx.clone(), watcher_present.clone()).await?;

    let lock_state = logind::LockState::connect().await;
    let mut uevent = UeventSocket::open().ok().and_then(|s| watch::register(s).ok());
    let config_fd = watch::watch(&config_path).ok();

    let mut interval = tokio::time::interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut paused_since: Option<Instant> = None;
    let mut last_panel: Vec<PanelItem> = Vec::new();
    let mut alerts: BTreeMap<MetricId, SustainedAlert> = BTreeMap::new();
    let mut tray: Option<tray::Tray> = None;
    let mut native_face_seen: Option<Instant> = None;
    // Debug timing: total time spent in sample-to-publish, logged each minute.
    let (mut busy, mut ticks) = (Duration::ZERO, 0u32);

    loop {
        let mut immediate = false;
        tokio::select! {
            _ = interval.tick() => {}
            req = rx.recv() => match req {
                Some(Request::Wake) => immediate = true,
                Some(Request::Detail { metric, reply }) => {
                    let cfg = lock(&shared).config.clone();
                    let with_procs = lock(&shared).demand().wants_processes();
                    let d = sampler.detail(metric, &cfg, with_procs, unix_now());
                    let _ = reply.send(serde_json::to_string(&d).unwrap_or_default());
                    continue;
                }
                None => return Ok(()),
            },
            r = async { uevent.as_mut().expect("guarded").readable_mut().await }, if uevent.is_some() => {
                if let Ok(mut g) = r {
                    g.clear_ready();
                    let changes = g.get_inner_mut().drain();
                    drop(g);
                    if changes.any() {
                        ::log::debug!("hotplug: {changes:?}");
                        sampler.rediscover(changes);
                        lock(&shared).describe = serde_json::to_string(&sampler.describe(env!("CARGO_PKG_VERSION"))).unwrap_or_default();
                    }
                }
                continue;
            }
            r = async { config_fd.as_ref().expect("guarded").readable().await }, if config_fd.is_some() => {
                if let Ok(mut g) = r {
                    g.clear_ready();
                    if let Some(fd) = &config_fd {
                        watch::drain(fd.get_ref());
                        watch::rewatch(fd.get_ref(), &config_path);
                    }
                    reload_config(&conn, &shared, &config_path).await;
                    immediate = true;
                }
            }
        }

        let locked = lock_state.locked();
        let (demand, cfg) = {
            let mut s = lock(&shared);
            let face_panel = s.subs.demand().panel;
            if face_panel {
                native_face_seen = Some(Instant::now());
            }
            // Auto shows tray items only when no face shows the bar.
            let want_tray = !locked
                && match s.config.tray.mode {
                    TrayMode::Never => false,
                    TrayMode::Always => true,
                    TrayMode::Auto => {
                        watcher_present.load(std::sync::atomic::Ordering::Relaxed)
                            && !face_panel
                            && started.elapsed() >= TRAY_GRACE
                            && native_face_seen.is_none_or(|t| t.elapsed() >= NATIVE_FACE_GRACE)
                    }
                };
            s.tray_active = want_tray;
            (s.demand(), s.config.clone())
        };
        // Hide tray items as soon as a native face takes over, or on lock.
        if !lock(&shared).tray_active && tray.is_some() {
            tray = None;
            ::log::info!("tray items hidden");
        }

        if !demand.any() || locked {
            paused_since.get_or_insert_with(Instant::now);
            continue;
        }
        if let Some(since) = paused_since.take() {
            let gap = since.elapsed().as_secs();
            for h in lock(&shared).history.values_mut() {
                h.push_gap(gap);
            }
            sampler.reprime();
        }

        if sampler.net_stale() {
            sampler.rediscover(masthead_probe::uevent::Changes::default());
        }
        let tick_start = Instant::now();
        let point = match sampler.sample(&demand, &cfg) {
            Ok(p) => p,
            Err(e) => {
                ::log::warn!("sample failed: {e}");
                continue;
            }
        };
        let items = sampler.panel_items(&cfg);
        {
            let mut s = lock(&shared);
            if !immediate {
                for (name, h) in &mut s.history {
                    h.push(point.get(name).unwrap_or(f32::NAN));
                }
            }
            s.panel.clone_from(&items);
        }
        // The first sample after start or a pause has no deltas yet; showing
        // its 0% for a second would be wrong.
        let primed = !point.cpu.is_nan();
        if primed && changed(&items, &last_panel) {
            let _ = conn.emit_signal(None::<()>, OBJECT_PATH, INTERFACE, "Panel", &(items.clone(),)).await;
            last_panel.clone_from(&items);
        }

        let t = unix_now();
        for m in &demand.hover {
            let d = sampler.detail(Some(*m), &cfg, true, t);
            if let Ok(json) = serde_json::to_string(&d) {
                let _ = conn.emit_signal(None::<()>, OBJECT_PATH, INTERFACE, "Detail", &(m.as_str(), json)).await;
            }
        }
        if demand.window {
            let d = sampler.detail(None, &cfg, true, t);
            if let Ok(json) = serde_json::to_string(&d) {
                let _ = conn.emit_signal(None::<()>, OBJECT_PATH, INTERFACE, "Detail", &("all", json)).await;
            }
        }

        if cfg.notifications.enabled && !immediate {
            for it in &items {
                let Ok(m) = it.id.parse::<MetricId>() else { continue };
                let level = masthead_core::Level::from_u32(it.level);
                if alerts.entry(m).or_default().update(level, t, cfg.notifications.hold_seconds) {
                    let summary = format!("{} at {}", m.title(), it.text);
                    let body = format!("Critical for {} seconds. {}", cfg.notifications.hold_seconds, it.summary);
                    notify::send(&conn, &summary, &body).await;
                }
            }
        }

        busy += tick_start.elapsed();
        ticks += 1;
        if ticks == 60 {
            ::log::debug!("tick: {:.0} µs average over 60 s", busy.as_secs_f64() * 1e6 / 60.0);
            (busy, ticks) = (Duration::ZERO, 0);
        }

        let want_tray = lock(&shared).tray_active;
        if want_tray {
            if tray.as_ref().is_some_and(|t| !t.same_items(&items)) {
                tray = None;
            }
            if let Some(t) = &mut tray {
                t.update(&items).await;
            } else {
                let mem_total = sampler.describe("").mem_total;
                match tray::Tray::start(&items, &conn, mem_total).await {
                    Ok(t) => tray = Some(t),
                    Err(e) => ::log::info!("tray unavailable: {e}"),
                }
            }
        }
    }
}

async fn reload_config(conn: &Connection, shared: &SharedRef, path: &std::path::Path) {
    let cfg = match Config::load(path) {
        Ok(c) => c,
        Err(e) => {
            ::log::warn!("{}: {e}; keeping the previous configuration", path.display());
            return;
        }
    };
    let json = {
        let mut s = lock(shared);
        if *s.config == cfg {
            return;
        }
        let json = serde_json::to_string(&cfg).unwrap_or_default();
        s.config = Arc::new(cfg);
        json
    };
    ::log::info!("configuration reloaded");
    let _ = conn.emit_signal(None::<()>, OBJECT_PATH, INTERFACE, "ConfigChanged", &(json,)).await;
}

/// Drop subscriptions of clients that leave the bus, and track whether an SNI
/// watcher is present.
async fn spawn_owner_watch(
    conn: &Connection,
    shared: SharedRef,
    tx: mpsc::Sender<Request>,
    watcher: Arc<std::sync::atomic::AtomicBool>,
) -> zbus::Result<()> {
    let dbus = zbus::fdo::DBusProxy::new(conn).await?;
    let present = dbus.name_has_owner(tray::WATCHER.try_into()?).await.unwrap_or(false);
    watcher.store(present, std::sync::atomic::Ordering::Relaxed);
    let mut stream = dbus.receive_name_owner_changed().await?;
    tokio::spawn(async move {
        while let Some(sig) = stream.next().await {
            let Ok(args) = sig.args() else { continue };
            let name = args.name().as_str();
            let gone = args.new_owner().is_none();
            if name == tray::WATCHER {
                watcher.store(!gone, std::sync::atomic::Ordering::Relaxed);
                let _ = tx.try_send(Request::Wake);
            } else if gone && name.starts_with(':') {
                let n = lock(&shared).subs.drop_owner(name);
                if n > 0 {
                    ::log::debug!("{name} left; dropped {n} subscriptions");
                    let _ = tx.try_send(Request::Wake);
                }
            }
        }
    });
    Ok(())
}
