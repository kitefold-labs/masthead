// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! The bar's 16 px stroke icons, as SVG. Colors come from the svg style tint.

use cosmic::iced::widget::svg;
use masthead_core::MetricId;

fn source(name: &str) -> &'static str {
    match name {
        "cpu" => {
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="#000" stroke-width="1.5" stroke-linecap="round"><rect x="4" y="4" width="8" height="8" rx="1.6"/><path d="M6.3 1.6v2M9.7 1.6v2M6.3 12.4v2M9.7 12.4v2M1.6 6.3h2M1.6 9.7h2M12.4 6.3h2M12.4 9.7h2"/></svg>"##
        }
        "mem" => {
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="#000" stroke-width="1.5" stroke-linecap="round"><rect x="1.75" y="4" width="12.5" height="7.5" rx="1.5"/><path d="M5 6.6v2.3M8 6.6v2.3M11 6.6v2.3M4.5 11.5v1.8M11.5 11.5v1.8"/></svg>"##
        }
        "gpu" => {
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="#000" stroke-width="1.5" stroke-linecap="round"><rect x="1.75" y="3.5" width="12.5" height="9" rx="1.6"/><circle cx="9.6" cy="8" r="2.2"/><path d="M4.6 6.2v3.6"/></svg>"##
        }
        "net" => {
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="#000" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M5 2.5v10.5M2.6 10.6 5 13l2.4-2.4M11 13.5V3M8.6 5.4 11 3l2.4 2.4"/></svg>"##
        }
        "disk" => {
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="#000" stroke-width="1.5" stroke-linecap="round"><rect x="2" y="3" width="12" height="10" rx="2"/><path d="M4.8 10h.01M7.5 10h3.7"/></svg>"##
        }
        "temp" => {
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="#000" stroke-width="1.5" stroke-linecap="round"><path d="M8 2.2c.97 0 1.75.78 1.75 1.75v5.4a3.1 3.1 0 1 1-3.5 0v-5.4C6.25 2.98 7.03 2.2 8 2.2Z"/><path d="M8 7v4.2"/></svg>"##
        }
        "battery" => {
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="#000" stroke-width="1.5" stroke-linecap="round"><rect x="1.75" y="4.5" width="11" height="7" rx="1.6"/><path d="M14.5 7v2M4.5 7v2"/></svg>"##
        }
        "processes" => {
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="#000" stroke-width="1.5" stroke-linecap="round"><path d="M2.5 4h11M2.5 8h11M2.5 12h7"/></svg>"##
        }
        "kill" => {
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="#000" stroke-width="1.5" stroke-linecap="round"><path d="M4.5 4.5l7 7M11.5 4.5l-7 7"/></svg>"##
        }
        "power" => {
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="#000" stroke-width="1.5" stroke-linecap="round"><path d="M8 2v5.5"/><path d="M4.6 4.2a5 5 0 1 0 6.8 0"/></svg>"##
        }
        "folder" => {
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="#000" stroke-width="1.5" stroke-linejoin="round"><path d="M2 4.5c0-.8.6-1.5 1.4-1.5h2.8l1.4 1.6h5c.8 0 1.4.6 1.4 1.4v5.9c0 .8-.6 1.4-1.4 1.4H3.4c-.8 0-1.4-.6-1.4-1.4z"/></svg>"##
        }
        "trash" => {
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="#000" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M2.5 4.5h11M6 4.5V3h4v1.5M4 4.5l.7 8.6c.1.8.7 1.4 1.5 1.4h3.6c.8 0 1.4-.6 1.5-1.4l.7-8.6"/></svg>"##
        }
        "settings" => {
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="#000" stroke-width="1.5" stroke-linecap="round"><path d="M2.5 4.5h6M11.5 4.5h2M2.5 11.5h2M7.5 11.5h6"/><circle cx="10" cy="4.5" r="1.5"/><circle cx="6" cy="11.5" r="1.5"/></svg>"##
        }
        _ => unreachable!("no icon named {name}"),
    }
}

/// A handle for a metric's icon, or `processes`, `settings`, `kill`, `power`, `folder` or `trash`.
pub fn handle(name: &str) -> svg::Handle {
    svg::Handle::from_memory(source(name).as_bytes())
}

pub fn metric(m: MetricId) -> svg::Handle {
    handle(m.as_str())
}

/// The full-color app logo. Its dark tile reads on light and dark themes.
pub fn logo() -> svg::Handle {
    svg::Handle::from_memory(include_bytes!("../../../data/icons/hicolor/scalable/apps/com.kitefoldlabs.Masthead.svg").as_slice())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_is_svg() {
        let names = MetricId::ALL.iter().map(|m| m.as_str()).chain(["processes", "settings", "kill", "power", "folder", "trash"]);
        for n in names {
            assert!(source(n).starts_with("<svg"), "{n}");
        }
    }
}
