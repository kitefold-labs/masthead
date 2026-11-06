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
        "settings" => {
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="#000" stroke-width="1.5" stroke-linecap="round"><path d="M2.5 4.5h6M11.5 4.5h2M2.5 11.5h2M7.5 11.5h6"/><circle cx="10" cy="4.5" r="1.5"/><circle cx="6" cy="11.5" r="1.5"/></svg>"##
        }
        _ => {
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32" fill="none" stroke-width="2.5"><rect x="12" y="9.5" width="8" height="18" rx="4" stroke="#e9ebee"/><rect x="5.5" y="4.5" width="21" height="8" rx="4" stroke="#26d2ee"/></svg>"##
        }
    }
}

/// A handle for a metric's icon, or `processes`, `settings`, `mark`.
pub fn handle(name: &str) -> svg::Handle {
    svg::Handle::from_memory(source(name).as_bytes())
}

pub fn metric(m: MetricId) -> svg::Handle {
    handle(m.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_is_svg() {
        for n in ["cpu", "mem", "gpu", "net", "disk", "temp", "battery", "processes", "settings", "mark"] {
            assert!(source(n).starts_with("<svg"), "{n}");
        }
    }
}
