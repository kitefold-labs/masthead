// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! `masthead-cosmic`: Masthead's values in the COSMIC panel.
//!
//! Each bar item shows an icon and its value. Resting the pointer on an item
//! opens a popover with more detail and the last minute as a sparkline;
//! clicking an item opens the Masthead window on that metric. All data comes
//! from masthead-daemon over D-Bus; the applet samples nothing itself.

mod app;
mod dbus;
mod hover;
mod model;
mod style;

/// `MASTHEAD_LOG=debug` prints UI events to stderr.
pub fn debug_enabled() -> bool {
    static ON: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| std::env::var("MASTHEAD_LOG").is_ok_and(|v| v == "debug"));
    *ON
}

fn main() -> cosmic::iced::Result {
    match std::env::args().nth(1).as_deref() {
        Some("--version" | "-V") => {
            println!("masthead-cosmic {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("--help" | "-h") => {
            println!("masthead-cosmic — Masthead for the COSMIC panel. Add it in Settings → Desktop → Panel → Applets.");
            Ok(())
        }
        _ => cosmic::applet::run::<app::Applet>(()),
    }
}
