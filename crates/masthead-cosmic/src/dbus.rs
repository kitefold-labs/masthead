//! The connection to masthead-daemon, as an iced subscription.
//!
//! The applet never samples anything. It holds the daemon's `panel` tier for
//! as long as it runs and a `hover:<metric>` tier while a popover is open.
//! Detail signals are matched on their metric argument, so the bus delivers
//! only the open popover's detail and none of the window's.

use cosmic::iced::Subscription;
use cosmic::iced::futures::channel::mpsc::Sender;
use cosmic::iced::futures::{SinkExt, Stream, StreamExt};
use masthead_api::detail::{Describe, Detail};
use masthead_api::{MastheadProxy, PanelItem};
use masthead_core::config::Config;
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

/// Requests from the UI.
#[derive(Debug, Clone)]
pub enum Cmd {
    /// Hold `hover:<metric>` and stream its detail.
    Hover(String),
    /// Release a hover subscription and stop its detail stream.
    Unhover(u32),
    ToggleWindow(String),
    /// Fetch the last 60 s of a metric's history series.
    History(String),
}

/// What the daemon told us.
#[derive(Debug, Clone)]
pub enum Event {
    /// Commands for this connection go here.
    Ready(UnboundedSender<Cmd>),
    Connected {
        describe: Box<Describe>,
        config: Box<Config>,
    },
    Panel(Vec<PanelItem>),
    Detail(String, Box<Detail>),
    History(String, Vec<Vec<f64>>),
    Subscribed(String, u32),
    Config(Box<Config>),
    /// The daemon restarted; hover subscriptions were lost.
    Restarted,
    /// No daemon reachable. The worker retries.
    Disconnected(String),
}

pub fn subscription() -> Subscription<Event> {
    Subscription::run(worker)
}

fn worker() -> impl Stream<Item = Event> {
    cosmic::iced::stream::channel(64, async |mut out: Sender<Event>| {
        loop {
            if let Err(e) = session(&mut out).await {
                let _ = out.send(Event::Disconnected(e.to_string())).await;
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    })
}

async fn connect_state(proxy: &MastheadProxy<'_>, out: &mut Sender<Event>) -> zbus::Result<()> {
    let describe: Describe = serde_json::from_str(&proxy.describe().await?).unwrap_or_default();
    let config: Config = serde_json::from_str(&proxy.get_config().await?).unwrap_or_default();
    let _ = out.send(Event::Connected { describe: Box::new(describe), config: Box::new(config) }).await;
    let _ = out.send(Event::Panel(proxy.snapshot().await?)).await;
    Ok(())
}

async fn session(out: &mut Sender<Event>) -> zbus::Result<()> {
    let conn = zbus::Connection::session().await?;
    let proxy = MastheadProxy::new(&conn).await?;
    let mut panel = proxy.receive_panel().await?;
    let mut config = proxy.receive_config_changed().await?;
    let mut owner = proxy.inner().receive_owner_changed().await?;
    // Subscribing D-Bus-activates the daemon if it is not running yet.
    proxy.subscribe("panel").await?;
    connect_state(&proxy, out).await?;

    let (tx, mut rx) = unbounded_channel();
    let _ = out.send(Event::Ready(tx)).await;
    let mut detail: Option<(String, masthead_api::DetailStream)> = None;

    loop {
        tokio::select! {
            s = panel.next() => {
                let Some(s) = s else { return Ok(()) };
                if let Ok(a) = s.args() {
                    let _ = out.send(Event::Panel(a.items)).await;
                }
            }
            s = async { detail.as_mut().expect("guarded").1.next().await }, if detail.is_some() => {
                match s {
                    Some(s) => {
                        if let Ok(a) = s.args()
                            && let Ok(d) = serde_json::from_str::<Detail>(a.json) {
                                let _ = out.send(Event::Detail(a.metric.to_owned(), Box::new(d))).await;
                            }
                    }
                    None => detail = None,
                }
            }
            s = config.next() => {
                if let Some(Ok(a)) = s.map(|s| s.args().map(|a| a.json.to_owned()))
                    && let Ok(c) = serde_json::from_str::<Config>(&a) {
                        let _ = out.send(Event::Config(Box::new(c))).await;
                    }
            }
            o = owner.next() => {
                if let Some(Some(_)) = o {
                    proxy.subscribe("panel").await?;
                    connect_state(&proxy, out).await?;
                    let _ = out.send(Event::Restarted).await;
                }
            }
            cmd = rx.recv() => {
                let Some(cmd) = cmd else { return Ok(()) };
                match cmd {
                    Cmd::Hover(m) => {
                        // Start listening before subscribing so the first signal is not missed.
                        detail = Some((m.clone(), proxy.receive_detail_with_args(&[(0, m.as_str())]).await?));
                        let h = proxy.subscribe(&format!("hover:{m}")).await?;
                        let _ = out.send(Event::Subscribed(m.clone(), h)).await;
                        if let Ok(d) = proxy.get_detail(&m).await
                            && let Ok(d) = serde_json::from_str::<Detail>(&d) {
                                let _ = out.send(Event::Detail(m, Box::new(d))).await;
                            }
                    }
                    Cmd::Unhover(h) => {
                        detail = None;
                        let _ = proxy.unsubscribe(h).await;
                    }
                    Cmd::ToggleWindow(m) => {
                        let _ = proxy.toggle_window(&m).await;
                    }
                    Cmd::History(m) => {
                        let mut series = Vec::new();
                        for s in crate::model::series(&m) {
                            let (_, _, max) = proxy.history(s, 60).await.unwrap_or_default();
                            series.push(max);
                        }
                        let _ = out.send(Event::History(m, series)).await;
                    }
                }
            }
        }
    }
}
