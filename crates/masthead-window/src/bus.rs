// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! Everything on the session bus runs on one background thread with its own
//! current-thread tokio runtime. The UI talks to it through two channels:
//! [`Event`]s come in, [`Req`]s go out.
//!
//! The thread first claims `com.kitefoldlabs.Masthead.Window` and serves the
//! `Window1` interface, so D-Bus activation and a second launch both reach the
//! running window. It contacts the daemon only once the UI asks it to start.

use crate::model::{Page, Series};
use futures_util::StreamExt;
use masthead_api::detail::{Describe, Detail};
use masthead_api::{MastheadProxy, PanelItem, WINDOW_BUS_NAME, WINDOW_OBJECT_PATH, WindowProxy};
use masthead_core::config::Config;
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use zbus::fdo::{self, RequestNameFlags, RequestNameReply};

/// A call to the window's own interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Show this page, or close if it is already showing.
    Toggle(Page),
    /// Show this page and raise the window.
    Show(Page),
}

#[derive(Debug, Clone)]
pub enum Event {
    Command(Command),
    Describe(Box<Describe>),
    Config(Box<Config>),
    Panel(Vec<PanelItem>),
    Detail(Box<Detail>),
    /// History for one series, tagged with the request generation.
    History {
        generation: u64,
        index: usize,
        series: Series,
    },
    /// Sparkline history for one series (last 60 s).
    Spark {
        series: &'static str,
        data: Series,
    },
    /// The desktop's color scheme: true for dark.
    Dark(bool),
    /// The daemon could not be reached, or an operation failed.
    Error(String),
}

#[derive(Debug, Clone)]
pub enum Req {
    /// Subscribe to the window tier and start forwarding daemon events.
    Start,
    History {
        generation: u64,
        index: usize,
        series: &'static str,
        seconds: u32,
    },
    SetConfig(Box<Config>),
    /// Refetch the last 60 s of every series for the sidebar.
    Sparks,
    /// Forward one detail sample in this many. Each forwarded sample redraws
    /// the window, so pages that are expensive to draw ask for fewer.
    DetailEvery(u32),
    Quit,
}

/// The receiving end of the event channel, taken once by the UI subscription.
pub static EVENTS: Mutex<Option<UnboundedReceiver<Event>>> = Mutex::new(None);

struct WindowIface {
    tx: UnboundedSender<Event>,
}

fn page(metric: &str) -> fdo::Result<Page> {
    Page::parse(metric).ok_or_else(|| fdo::Error::InvalidArgs(format!("unknown page {metric:?}")))
}

#[zbus::interface(name = "com.kitefoldlabs.Masthead.Window1")]
impl WindowIface {
    fn toggle(&self, metric: &str) -> fdo::Result<()> {
        let _ = self.tx.send(Event::Command(Command::Toggle(page(metric)?)));
        Ok(())
    }

    fn show(&self, metric: &str) -> fdo::Result<()> {
        let _ = self.tx.send(Event::Command(Command::Show(page(metric)?)));
        Ok(())
    }
}

/// The outcome of claiming the window's bus name.
#[derive(Debug)]
pub enum Claim {
    /// This process owns the name; the bus thread is running. `dark` is the
    /// desktop color scheme read before the first frame, when available.
    Owned { events: UnboundedReceiver<Event>, requests: UnboundedSender<Req>, dark: Option<bool> },
    /// Another window was running and has been asked to show `page`.
    Forwarded,
}

/// What the bus thread reports once the name claim resolves.
enum Reply {
    /// Owned, with the color scheme if the portal answered.
    Owned(Option<bool>),
    Forwarded,
}

/// Start the bus thread and claim the name. Blocks until the claim resolves.
pub fn start(first: Page) -> Result<Claim, String> {
    let (claim_tx, claim_rx) = std::sync::mpsc::channel::<Result<Reply, String>>();
    let (ev_tx, ev_rx) = unbounded_channel::<Event>();
    let (req_tx, req_rx) = unbounded_channel::<Req>();
    std::thread::Builder::new()
        .name("masthead-bus".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = claim_tx.send(Err(e.to_string()));
                    return;
                }
            };
            rt.block_on(run(first, claim_tx, ev_tx, req_rx));
        })
        .map_err(|e| e.to_string())?;
    match claim_rx.recv().map_err(|_| "bus thread stopped".to_owned())?? {
        Reply::Owned(dark) => Ok(Claim::Owned { events: ev_rx, requests: req_tx, dark }),
        Reply::Forwarded => Ok(Claim::Forwarded),
    }
}

#[allow(clippy::too_many_lines)] // the bus thread's whole lifecycle, top to bottom
async fn run(
    first: Page,
    claim: std::sync::mpsc::Sender<Result<Reply, String>>,
    tx: UnboundedSender<Event>,
    mut rx: UnboundedReceiver<Req>,
) {
    let conn = match zbus::Connection::session().await {
        Ok(c) => c,
        Err(e) => {
            let _ = claim.send(Err(format!("session bus: {e}")));
            return;
        }
    };
    if let Err(e) = conn.object_server().at(WINDOW_OBJECT_PATH, WindowIface { tx: tx.clone() }).await {
        let _ = claim.send(Err(e.to_string()));
        return;
    }
    let name = zbus::names::WellKnownName::from_static_str_unchecked(WINDOW_BUS_NAME);
    match conn.request_name_with_flags(name, RequestNameFlags::DO_NOT_QUEUE).await {
        Ok(RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner) => {
            let dark = match forced_scheme() {
                Some(d) => Some(d),
                None => tokio::time::timeout(Duration::from_millis(500), read_color_scheme(&conn)).await.ok().flatten(),
            };
            let _ = claim.send(Ok(Reply::Owned(dark)));
        }
        Ok(_) | Err(zbus::Error::NameTaken) => {
            // Another window owns the name: ask it to show the page and exit.
            let r = async { WindowProxy::new(&conn).await?.show(first.as_str()).await }.await;
            let _ = claim.send(r.map(|()| Reply::Forwarded).map_err(|e| e.to_string()));
            return;
        }
        Err(e) => {
            let _ = claim.send(Err(e.to_string()));
            return;
        }
    }

    spawn_color_scheme(&conn, tx.clone());

    // Wait for the UI before contacting the daemon.
    loop {
        match rx.recv().await {
            Some(Req::Start) => break,
            Some(Req::Quit) | None => std::process::exit(0),
            Some(_) => {}
        }
    }
    let daemon = match MastheadProxy::new(&conn).await {
        Ok(p) => p,
        Err(e) => {
            let _ = tx.send(Event::Error(e.to_string()));
            return;
        }
    };
    let mut handle = connect(&daemon, &tx).await;
    let mut panel = daemon.receive_panel().await.ok();
    let mut detail = daemon.receive_detail().await.ok();
    let mut config = daemon.receive_config_changed().await.ok();
    let mut owner = daemon.inner().receive_owner_changed().await.ok();
    let mut last_levels: Vec<(String, u32)> = Vec::new();
    let (mut every, mut skipped) = (1u32, 0u32);

    loop {
        tokio::select! {
            Some(sig) = next(&mut panel) => {
                // The window only uses bar items for their alert levels, and
                // every forwarded event costs a full re-render; skip the rest.
                if let Ok(a) = sig.args() {
                    let levels: Vec<(String, u32)> = a.items.iter().map(|i| (i.id.clone(), i.level)).collect();
                    if levels != last_levels {
                        last_levels = levels;
                        let _ = tx.send(Event::Panel(a.items));
                    }
                }
            }
            Some(sig) = next(&mut detail) => {
                if let Ok(a) = sig.args()
                    && a.metric == "all"
                {
                    skipped += 1;
                    if skipped >= every {
                        skipped = 0;
                        if let Ok(d) = serde_json::from_str::<Detail>(a.json) {
                            let _ = tx.send(Event::Detail(Box::new(d)));
                        }
                    }
                }
            }
            Some(sig) = next(&mut config) => {
                if let Ok(a) = sig.args()
                    && let Ok(c) = serde_json::from_str::<Config>(a.json) {
                        let _ = tx.send(Event::Config(Box::new(c)));
                    }
            }
            Some(o) = next(&mut owner) => {
                // The daemon restarted: our subscription went with it.
                if o.is_some() {
                    handle = connect(&daemon, &tx).await;
                }
            }
            req = rx.recv() => match req {
                Some(Req::History { generation, index, series, seconds }) => {
                    match daemon.history(series, seconds).await {
                        Ok((step, min, max)) => {
                            let _ = tx.send(Event::History { generation, index, series: Series::from_history(step, min, max) });
                        }
                        Err(e) => {
                            let _ = tx.send(Event::Error(format!("history: {e}")));
                        }
                    }
                }
                Some(Req::SetConfig(cfg)) => {
                    let json = serde_json::to_string(&*cfg).unwrap_or_default();
                    if let Err(e) = daemon.set_config(&json).await {
                        let _ = tx.send(Event::Error(format!("could not save settings: {e}")));
                        if let Some(c) = get_config(&daemon).await {
                            let _ = tx.send(Event::Config(Box::new(c)));
                        }
                    }
                }
                Some(Req::Sparks) => send_sparks(&daemon, &tx).await,
                Some(Req::DetailEvery(n)) => every = n.max(1),
                Some(Req::Start) => {}
                Some(Req::Quit) | None => {
                    if let Some(h) = handle {
                        let _ = tokio::time::timeout(Duration::from_millis(300), daemon.unsubscribe(h)).await;
                    }
                    std::process::exit(0);
                }
            }
        }
    }
}

async fn next<S: futures_util::Stream + Unpin>(s: &mut Option<S>) -> Option<S::Item> {
    match s {
        Some(s) => s.next().await,
        None => std::future::pending().await,
    }
}

async fn get_config(daemon: &MastheadProxy<'_>) -> Option<Config> {
    serde_json::from_str(&daemon.get_config().await.ok()?).ok()
}

/// Subscribe to the window tier (D-Bus-activating the daemon if needed) and
/// send the hardware description, configuration and sparkline history.
async fn connect(daemon: &MastheadProxy<'_>, tx: &UnboundedSender<Event>) -> Option<u32> {
    let handle = match daemon.subscribe("window").await {
        Ok(h) => h,
        Err(e) => {
            let _ = tx.send(Event::Error(format!("masthead-daemon is not reachable: {e}")));
            return None;
        }
    };
    if let Ok(d) = daemon.describe().await.map(|s| serde_json::from_str::<Describe>(&s))
        && let Ok(d) = d
    {
        let _ = tx.send(Event::Describe(Box::new(d)));
    }
    if let Some(c) = get_config(daemon).await {
        let _ = tx.send(Event::Config(Box::new(c)));
    }
    if let Ok(items) = daemon.snapshot().await {
        let _ = tx.send(Event::Panel(items));
    }
    if let Ok(json) = daemon.get_detail("all").await
        && let Ok(d) = serde_json::from_str::<Detail>(&json)
    {
        let _ = tx.send(Event::Detail(Box::new(d)));
    }
    send_sparks(daemon, tx).await;
    Some(handle)
}

async fn send_sparks(daemon: &MastheadProxy<'_>, tx: &UnboundedSender<Event>) {
    for s in masthead_api::SERIES {
        if let Ok((step, min, max)) = daemon.history(s, 60).await {
            let _ = tx.send(Event::Spark { series: s, data: Series::from_history(step, min, max) });
        }
    }
}

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const SETTINGS: &str = "org.freedesktop.portal.Settings";

/// `color-scheme` 1 means prefer dark; 0 (no preference) and 2 mean light.
fn scheme_is_dark(v: &zbus::OwnedVariant) -> Option<bool> {
    u32::try_from(&**v).ok().map(|n| n == 1)
}

async fn read_color_scheme(conn: &zbus::Connection) -> Option<bool> {
    let p = zbus::Proxy::new(conn, PORTAL, PORTAL_PATH, SETTINGS).await.ok()?;
    let reply = p.call_method("ReadOne", &("org.freedesktop.appearance", "color-scheme")).await.ok()?;
    scheme_is_dark(&reply.body().deserialize::<zbus::OwnedVariant>().ok()?)
}

/// Follow changes to `org.freedesktop.appearance color-scheme`.
fn spawn_color_scheme(conn: &zbus::Connection, tx: UnboundedSender<Event>) {
    if forced_scheme().is_some() {
        return;
    }
    let conn = conn.clone();
    tokio::spawn(async move {
        let Ok(p) = zbus::Proxy::new(&conn, PORTAL, PORTAL_PATH, SETTINGS).await else { return };
        let Ok(mut changes) = p.receive_signal("SettingChanged").await else { return };
        while let Some(msg) = changes.next().await {
            if let Ok((ns, key, v)) = msg.body().deserialize::<(String, String, zbus::OwnedVariant)>()
                && ns == "org.freedesktop.appearance"
                && key == "color-scheme"
                && let Some(d) = scheme_is_dark(&v)
            {
                let _ = tx.send(Event::Dark(d));
            }
        }
    });
}

/// `MASTHEAD_THEME=dark|light` overrides the desktop preference.
pub fn forced_scheme() -> Option<bool> {
    match std::env::var("MASTHEAD_THEME").ok().as_deref() {
        Some("dark") => Some(true),
        Some("light") => Some(false),
        _ => None,
    }
}
