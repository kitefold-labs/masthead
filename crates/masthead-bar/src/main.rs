//! `masthead-bar`: bar values for status bars that read a command's output.
//!
//! It subscribes to the daemon's panel tier and prints one line each time a
//! value it shows changes. It never samples anything itself.

mod render;

use futures_util::StreamExt;
use masthead_api::{MastheadProxy, PanelItem};
use render::{Format, Options};
use std::io::Write;
use tokio::io::AsyncBufReadExt;

const USAGE: &str = "masthead-bar — Masthead values for Waybar, Ironbar, eww, i3bar and swaybar

USAGE:
    masthead-bar [--format waybar|i3bar|plain] [--metric ID]... [--no-labels]
    masthead-bar --toggle ID      Open, switch or close the Masthead window

OPTIONS:
    --format FORMAT   waybar (default): one JSON object per line with text,
                      tooltip, class and percentage.
                      i3bar: the i3bar protocol with click events (also swaybar).
                      plain: one line of text.
    --metric ID       Show only this metric (cpu, mem, gpu, net, disk, temp,
                      battery). Repeat for several. Default: the bar metrics
                      chosen in Masthead's settings.
    --no-labels       Leave out the metric name before each value.
    --tier TIER       Also hold a hover:<metric> or window subscription.
                      For benchmarks; normal bars never need it.

WAYBAR:
    \"custom/masthead-cpu\": {
        \"exec\": \"masthead-bar --metric cpu\",
        \"return-type\": \"json\",
        \"on-click\": \"masthead-bar --toggle cpu\"
    }";

fn parse_args() -> Result<(Options, Option<String>), String> {
    let mut o = Options::default();
    let mut toggle = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--format" => {
                o.format = match args.next().as_deref() {
                    Some("waybar") => Format::Waybar,
                    Some("i3bar" | "swaybar") => Format::I3bar,
                    Some("plain") => Format::Plain,
                    other => return Err(format!("unknown format {other:?}")),
                };
            }
            "--metric" => {
                let m = args.next().ok_or("--metric needs a metric id")?;
                m.parse::<masthead_core::MetricId>().map_err(|e| e.to_string())?;
                o.metrics.push(m);
            }
            "--no-labels" => o.labels = false,
            "--tier" => {
                let t = args.next().ok_or("--tier needs a tier")?;
                t.parse::<masthead_core::tiers::Tier>()?;
                o.extra_tiers.push(t);
            }
            "--toggle" => toggle = Some(args.next().ok_or("--toggle needs a metric id")?),
            "--help" | "-h" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            "--version" | "-V" => {
                println!("masthead-bar {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok((o, toggle))
}

fn main() {
    let (opts, toggle) = match parse_args() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("masthead-bar: {e}\n\n{USAGE}");
            std::process::exit(2);
        }
    };
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tokio runtime");
    let r = rt.block_on(async {
        if let Some(m) = toggle {
            let conn = zbus::Connection::session().await?;
            MastheadProxy::new(&conn).await?.toggle_window(&m).await?;
            return Ok(());
        }
        run(opts).await
    });
    if let Err(e) = r {
        eprintln!("masthead-bar: {e}");
        std::process::exit(1);
    }
}

fn emit(line: &str) {
    let mut out = std::io::stdout().lock();
    // A closed pipe means the bar went away; exit quietly.
    if writeln!(out, "{line}").and_then(|()| out.flush()).is_err() {
        std::process::exit(0);
    }
}

async fn run(opts: Options) -> zbus::Result<()> {
    let conn = zbus::Connection::session().await?;
    let proxy = MastheadProxy::new(&conn).await?;
    let mut panel = proxy.receive_panel().await?;
    let mut owner = proxy.inner().receive_owner_changed().await?;
    // Subscribing D-Bus-activates the daemon if it is not running.
    proxy.subscribe("panel").await?;
    for t in &opts.extra_tiers {
        proxy.subscribe(t).await?;
    }
    if opts.format == Format::I3bar {
        emit(&render::i3bar_header());
    }
    let mut last = String::new();
    let mut show = |items: &[PanelItem]| {
        let line = render::line(&opts, items);
        if line != last {
            emit(&line);
            last = line;
        }
    };
    show(&proxy.snapshot().await.unwrap_or_default());

    let mut stdin = tokio::io::BufReader::new(tokio::io::stdin()).lines();
    let clicks = opts.format == Format::I3bar;
    loop {
        tokio::select! {
            sig = panel.next() => {
                let Some(sig) = sig else { return Ok(()) };
                if let Ok(args) = sig.args() {
                    show(&args.items);
                }
            }
            o = owner.next() => {
                // The daemon restarted: subscribe again.
                if let Some(Some(_)) = o {
                    let _ = proxy.subscribe("panel").await;
                }
            }
            line = stdin.next_line(), if clicks => {
                match line {
                    Ok(Some(l)) => {
                        if let Some(m) = render::i3bar_click(&l) {
                            let _ = proxy.toggle_window(&m).await;
                        }
                    }
                    _ => return Ok(()),
                }
            }
        }
    }
}
