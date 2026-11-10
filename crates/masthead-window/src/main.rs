// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! `masthead-window`: Masthead's detail window.
//!
//! A separate process, started when a bar item is clicked and exited when the
//! window closes, so its memory goes back to the system. It samples nothing;
//! everything comes from `masthead-daemon` over the session bus.

mod app;
mod bus;
mod chart;
mod disk;
mod icons;
mod kill;
mod model;
mod palette;
mod treemap;
mod views;

use bus::{Claim, Command, Event};
use model::Page;
use std::borrow::Cow;
use std::time::Duration;

const USAGE: &str = "masthead-window — Masthead's detail window

USAGE:
    masthead-window [--metric ID]   Open on a metric (cpu, mem, gpu, net, disk,
                                    temp, battery), processes, settings or
                                    disk-usage
    masthead-window --service       Wait for a D-Bus Toggle or Show call, then open
                                    (what D-Bus activation runs)
    masthead-window --version

ENVIRONMENT:
    MASTHEAD_THEME=dark|light       Override the desktop's color scheme (the
                                    Theme setting still takes precedence)";

/// How long an activated service waits for its first call before giving up.
const SERVICE_WAIT: Duration = Duration::from_secs(25);

const FONTS: &[&[u8]] = &[
    include_bytes!("../fonts/SpaceGrotesk-Regular.ttf"),
    include_bytes!("../fonts/SpaceGrotesk-Medium.ttf"),
    include_bytes!("../fonts/SpaceGrotesk-Bold.ttf"),
    include_bytes!("../fonts/JetBrainsMono-Regular.ttf"),
    include_bytes!("../fonts/JetBrainsMono-Medium.ttf"),
];

#[derive(Debug, PartialEq, Eq)]
enum Mode {
    Open(Page),
    Service,
}

fn parse_args(args: &[String]) -> Result<Option<Mode>, String> {
    let mut mode = Mode::Open(Page::Metric(masthead_core::MetricId::Cpu));
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--metric" | "-m" => {
                let v = it.next().ok_or("--metric needs a value")?;
                mode = Mode::Open(Page::parse(v).ok_or_else(|| format!("unknown metric {v:?}"))?);
            }
            "--service" | "--gapplication-service" => mode = Mode::Service,
            "--help" | "-h" => {
                println!("{USAGE}");
                return Ok(None);
            }
            "--version" | "-V" => {
                println!("masthead-window {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(Some(mode))
}

fn load_fonts() {
    let mut fs = cosmic::iced::advanced::graphics::text::font_system().write().expect("font system");
    for f in FONTS {
        fs.load_font(Cow::Borrowed(f));
    }
}

/// Whether winit will open the window on Wayland, which it does whenever
/// `WAYLAND_DISPLAY` or `WAYLAND_SOCKET` is set and not empty.
fn on_wayland(var: impl Fn(&str) -> Option<String>) -> bool {
    ["WAYLAND_DISPLAY", "WAYLAND_SOCKET"].iter().any(|k| var(k).is_some_and(|v| !v.is_empty()))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // The Disk usage page runs its scan in a child copy of this program.
    if args.first().map(String::as_str) == Some(masthead_disk::child::ARG) {
        let code = i32::from(masthead_disk::child::serve(args.get(1).map(Into::into)).is_err());
        std::process::exit(code);
    }
    let mode = match parse_args(&args) {
        Ok(Some(m)) => m,
        Ok(None) => return,
        Err(e) => {
            eprintln!("masthead-window: {e}\n\n{USAGE}");
            std::process::exit(2);
        }
    };
    let first = match mode {
        Mode::Open(p) => p,
        Mode::Service => Page::Metric(masthead_core::MetricId::Cpu),
    };
    // Opening the window undoes Quit Masthead, so the daemon can start again.
    masthead_core::quit::clear();
    let (mut events, requests, dark) = match bus::start(first) {
        Ok(Claim::Owned { events, requests, dark }) => (events, requests, dark),
        Ok(Claim::Forwarded) => return,
        Err(e) => {
            eprintln!("masthead-window: {e}");
            std::process::exit(1);
        }
    };
    let page = match mode {
        Mode::Open(p) => p,
        Mode::Service => {
            // Activation started us because of a call; wait for it.
            let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().expect("runtime");
            let got = rt.block_on(async {
                tokio::time::timeout(SERVICE_WAIT, async {
                    loop {
                        match events.recv().await {
                            Some(Event::Command(Command::Toggle(p) | Command::Show(p))) => break Some(p),
                            Some(_) => {}
                            None => break None,
                        }
                    }
                })
                .await
            });
            match got {
                Ok(Some(p)) => p,
                _ => std::process::exit(0),
            }
        }
    };
    if let Ok(mut g) = bus::EVENTS.lock() {
        *g = Some(events);
    }
    load_fonts();
    let dark = dark.unwrap_or(true);
    // Read the theme setting now so the first frame already uses it; the
    // daemon sends the full configuration a moment later.
    let theme =
        masthead_core::config::Config::load(&masthead_core::config::Config::default_path()).map(|c| c.window.theme).unwrap_or_default();
    let pal = palette::Palette::for_dark(app::effective_dark(theme, dark));
    // The software renderer cannot draw into the ARGB visual X11 gives a
    // transparent window, so on X11 the window is opaque with square corners.
    let wayland = on_wayland(|k| std::env::var(k).ok());
    let settings = cosmic::app::Settings::default()
        .size(cosmic::iced::Size::new(1040.0, 720.0))
        .size_limits(cosmic::iced::Limits::NONE.min_width(760.0).min_height(520.0))
        .default_font(views::SANS)
        .default_text_size(14.0)
        .theme(palette::cosmic_theme(&pal))
        .transparent(wayland);
    if let Err(e) = cosmic::app::run::<app::App>(settings, app::Flags { page, dark, theme, sharp_corners: !wayland, requests }) {
        eprintln!("masthead-window: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn args() {
        assert_eq!(parse_args(&a(&[])).unwrap(), Some(Mode::Open(Page::Metric(masthead_core::MetricId::Cpu))));
        assert_eq!(parse_args(&a(&["--metric", "net"])).unwrap(), Some(Mode::Open(Page::Metric(masthead_core::MetricId::Net))));
        assert_eq!(parse_args(&a(&["--metric", "settings"])).unwrap(), Some(Mode::Open(Page::Settings)));
        assert_eq!(parse_args(&a(&["--service"])).unwrap(), Some(Mode::Service));
        assert!(parse_args(&a(&["--metric"])).is_err());
        assert!(parse_args(&a(&["--metric", "fan"])).is_err());
        assert!(parse_args(&a(&["--bogus"])).is_err());
        assert_eq!(FONTS.len(), 5);
    }

    #[test]
    fn wayland_detection_matches_winit() {
        let env = |pairs: &'static [(&str, &str)]| move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| (*v).to_owned());
        assert!(on_wayland(env(&[("WAYLAND_DISPLAY", "wayland-0"), ("DISPLAY", ":0")])));
        assert!(on_wayland(env(&[("WAYLAND_SOCKET", "3")])));
        assert!(!on_wayland(env(&[("DISPLAY", ":0")])));
        assert!(!on_wayland(env(&[("WAYLAND_DISPLAY", ""), ("DISPLAY", ":0")])));
    }
}
