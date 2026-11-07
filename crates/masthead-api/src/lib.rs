// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! The `com.kitefoldlabs.Masthead1` D-Bus contract.
//!
//! The daemon implements it; every face uses [`MastheadProxy`]. Bar values
//! travel as typed structs; richer detail travels as JSON (see [`detail`]) so
//! the GNOME extension can read it with `JSON.parse` and fields can be added
//! without breaking older faces.

pub mod detail;

use serde::{Deserialize, Serialize};
use zbus::zvariant::Type;

pub const BUS_NAME: &str = "com.kitefoldlabs.Masthead1";
pub const OBJECT_PATH: &str = "/com/kitefoldlabs/Masthead1";
pub const INTERFACE: &str = "com.kitefoldlabs.Masthead1";

pub const WINDOW_BUS_NAME: &str = "com.kitefoldlabs.Masthead.Window";
pub const WINDOW_OBJECT_PATH: &str = "/com/kitefoldlabs/Masthead/Window";
pub const WINDOW_INTERFACE: &str = "com.kitefoldlabs.Masthead.Window1";

/// History series accepted by `History`.
pub const SERIES: &[&str] = &["cpu", "mem", "gpu", "net.rx", "net.tx", "disk.read", "disk.write", "temp", "battery"];

/// One bar item. D-Bus signature `(sdsus)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct PanelItem {
    /// Metric id: `cpu`, `mem`, `gpu`, `net`, `disk`, `temp`, `battery`.
    pub id: String,
    /// The raw value: percent, bytes, bytes per second (download for `net`,
    /// read plus write for `disk`) or °C.
    pub value: f64,
    /// Formatted for the bar, already in the user's units: `23%`, `↓2.1M ↑180K`.
    pub text: String,
    /// 0 normal, 1 warn, 2 critical.
    pub level: u32,
    /// One line for tooltips: `load 4.12 · 64 °C`.
    pub summary: String,
}

/// The daemon's interface, as seen by a client.
#[zbus::proxy(
    interface = "com.kitefoldlabs.Masthead1",
    default_service = "com.kitefoldlabs.Masthead1",
    default_path = "/com/kitefoldlabs/Masthead1"
)]
pub trait Masthead {
    /// Hardware found at startup, as [`detail::Describe`] JSON.
    fn describe(&self) -> zbus::Result<String>;

    /// Ask for a sampling tier: `panel`, `hover:<metric>` or `window`.
    /// The subscription ends with `Unsubscribe` or when this connection closes.
    fn subscribe(&self, tier: &str) -> zbus::Result<u32>;

    fn unsubscribe(&self, handle: u32) -> zbus::Result<()>;

    /// The current bar items, for a first paint before the next `Panel` signal.
    fn snapshot(&self) -> zbus::Result<Vec<PanelItem>>;

    /// Detail for one metric (or `all`) as [`detail::Detail`] JSON. Without a
    /// matching hover or window subscription, process lists are empty.
    fn get_detail(&self, metric: &str) -> zbus::Result<String>;

    /// `(step_seconds, min, max)` covering the last `seconds`. NaN marks gaps.
    fn history(&self, series: &str, seconds: u32) -> zbus::Result<(f64, Vec<f64>, Vec<f64>)>;

    /// Open the window on `metric`, switch it to `metric`, or close it if it
    /// already shows `metric`.
    fn toggle_window(&self, metric: &str) -> zbus::Result<()>;

    /// The configuration as JSON.
    fn get_config(&self) -> zbus::Result<String>;

    /// Replace the configuration (JSON). Validated, written to disk, applied.
    fn set_config(&self, json: &str) -> zbus::Result<()>;

    /// Exit after replying. Used by Quit Masthead, which first writes the
    /// session's quit marker so nothing starts the daemon again.
    fn quit(&self) -> zbus::Result<()>;

    /// Bar items, sent only when a text or level changed.
    #[zbus(signal)]
    fn panel(&self, items: Vec<PanelItem>) -> zbus::Result<()>;

    /// Detail JSON for a hovered metric, or `all` for the window, once a second.
    #[zbus(signal)]
    fn detail(&self, metric: &str, json: &str) -> zbus::Result<()>;

    /// The configuration changed, as JSON.
    #[zbus(signal)]
    fn config_changed(&self, json: &str) -> zbus::Result<()>;

    #[zbus(property)]
    fn version(&self) -> zbus::Result<String>;
}

/// The window's interface, implemented by `masthead-window` and called by the
/// daemon. D-Bus activation starts the window on the first call.
#[zbus::proxy(
    interface = "com.kitefoldlabs.Masthead.Window1",
    default_service = "com.kitefoldlabs.Masthead.Window",
    default_path = "/com/kitefoldlabs/Masthead/Window"
)]
pub trait Window {
    /// Show `metric`, or close the window if it is already showing it.
    fn toggle(&self, metric: &str) -> zbus::Result<()>;

    /// Show `metric` and raise the window.
    fn show(&self, metric: &str) -> zbus::Result<()>;
}
