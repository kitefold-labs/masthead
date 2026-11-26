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

/// libcosmic's `applet::run` starts iced's default executor, a multi-thread
/// tokio runtime with one worker per core, and ignores `Application::Executor`
/// (so `SingleThreadExecutor` has no effect there). Masthead's async work is a
/// single D-Bus subscription and two timers, so cap the pool through the
/// variable tokio reads when that runtime is built. A value the user set wins.
/// libcosmic exposes no knob for this; remove once it does.
fn cap_tokio_workers() {
    if std::env::var_os("TOKIO_WORKER_THREADS").is_none() {
        // SAFETY: called first in `main`, before any other thread exists.
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var("TOKIO_WORKER_THREADS", "1");
        }
    }
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
        _ => {
            cap_tokio_workers();
            cosmic::applet::run::<app::Applet>(())
        }
    }
}
