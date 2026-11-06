//! Output formats.

use masthead_api::PanelItem;
use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    #[default]
    Waybar,
    I3bar,
    Plain,
}

#[derive(Debug, Clone)]
pub struct Options {
    pub format: Format,
    pub metrics: Vec<String>,
    pub labels: bool,
    /// Extra subscriptions held for benchmarking (`--tier`).
    pub extra_tiers: Vec<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self { format: Format::Waybar, metrics: Vec::new(), labels: true, extra_tiers: Vec::new() }
    }
}

fn selected<'a>(o: &Options, items: &'a [PanelItem]) -> Vec<&'a PanelItem> {
    if o.metrics.is_empty() { items.iter().collect() } else { o.metrics.iter().filter_map(|m| items.iter().find(|i| i.id == *m)).collect() }
}

fn label(o: &Options, it: &PanelItem) -> String {
    // Network text already carries arrows; a label would only add width.
    if o.labels && it.id != "net" { format!("{} {}", it.id, it.text) } else { it.text.clone() }
}

fn level_name(l: u32) -> &'static str {
    masthead_core::Level::from_u32(l).as_str()
}

fn title(id: &str) -> &'static str {
    id.parse::<masthead_core::MetricId>().map_or("", masthead_core::MetricId::title)
}

/// Halyard's dark-theme status colors.
fn color(level: u32) -> Option<&'static str> {
    match level {
        2 => Some("#f87171"),
        1 => Some("#fbbf24"),
        _ => None,
    }
}

pub fn line(o: &Options, items: &[PanelItem]) -> String {
    let sel = selected(o, items);
    match o.format {
        Format::Plain => sel.iter().map(|i| label(o, i)).collect::<Vec<_>>().join("  "),
        Format::Waybar => {
            let single = sel.len() == 1;
            let text = if single && o.metrics.len() == 1 && !o.labels {
                sel[0].text.clone()
            } else {
                sel.iter().map(|i| label(o, i)).collect::<Vec<_>>().join("  ")
            };
            let tooltip = sel.iter().map(|i| format!("{} {}\n{}", title(&i.id), i.text, i.summary)).collect::<Vec<_>>().join("\n\n");
            let worst = sel.iter().map(|i| i.level).max().unwrap_or(0);
            let mut class = vec!["masthead".to_owned(), level_name(worst).to_owned()];
            class.extend(sel.iter().map(|i| i.id.clone()));
            let mut v = json!({ "text": text, "tooltip": tooltip, "class": class });
            if single && matches!(sel[0].id.as_str(), "cpu" | "gpu" | "temp" | "battery") {
                v["percentage"] = json!(sel[0].value.round().clamp(0.0, 100.0) as u32);
            }
            v.to_string()
        }
        Format::I3bar => {
            let blocks: Vec<_> = sel
                .iter()
                .map(|i| {
                    let mut b = json!({ "name": format!("masthead-{}", i.id), "full_text": label(o, i), "separator_block_width": 14 });
                    if let Some(c) = color(i.level) {
                        b["color"] = json!(c);
                    }
                    b
                })
                .collect();
            format!("{},", serde_json::Value::Array(blocks))
        }
    }
}

pub fn i3bar_header() -> String {
    "{\"version\":1,\"click_events\":true}\n[".to_owned()
}

/// The metric clicked, from an i3bar click-event line.
pub fn i3bar_click(line: &str) -> Option<String> {
    let l = line.trim().trim_start_matches(',').trim_start_matches('[');
    let v: serde_json::Value = serde_json::from_str(l).ok()?;
    v.get("name")?.as_str()?.strip_prefix("masthead-").map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items() -> Vec<PanelItem> {
        vec![
            PanelItem { id: "cpu".into(), value: 23.4, text: "23%".into(), level: 0, summary: "load 4.12".into() },
            PanelItem { id: "net".into(), value: 1.0, text: "↓2.1M ↑180K".into(), level: 0, summary: "↓ 2.1 MB/s".into() },
            PanelItem { id: "temp".into(), value: 91.0, text: "91°".into(), level: 2, summary: "k10temp Tctl".into() },
        ]
    }

    #[test]
    fn waybar() {
        let o = Options { metrics: vec!["cpu".into()], labels: false, ..Options::default() };
        let v: serde_json::Value = serde_json::from_str(&line(&o, &items())).unwrap();
        assert_eq!(v["text"], "23%");
        assert_eq!(v["percentage"], 23);
        assert_eq!(v["tooltip"], "CPU 23%\nload 4.12");
        assert_eq!(v["class"], json!(["masthead", "normal", "cpu"]));
        let all: serde_json::Value = serde_json::from_str(&line(&Options::default(), &items())).unwrap();
        assert_eq!(all["text"], "cpu 23%  ↓2.1M ↑180K  temp 91°");
        assert_eq!(all["class"][1], "crit");
        assert!(all.get("percentage").is_none());
    }

    #[test]
    fn i3bar() {
        let o = Options { format: Format::I3bar, ..Options::default() };
        let l = line(&o, &items());
        assert!(l.ends_with("],"));
        let v: serde_json::Value = serde_json::from_str(l.trim_end_matches(',')).unwrap();
        assert_eq!(v[2]["color"], "#f87171");
        assert!(v[0].get("color").is_none());
        assert!(i3bar_header().starts_with("{\"version\":1"));
        assert_eq!(i3bar_click(r#",{"name":"masthead-cpu","button":1}"#).as_deref(), Some("cpu"));
        assert_eq!(i3bar_click(r#"[{"name":"other"}"#), None);
        assert_eq!(i3bar_click("garbage"), None);
    }

    #[test]
    fn plain() {
        let o = Options { format: Format::Plain, metrics: vec!["temp".into(), "nope".into()], ..Options::default() };
        assert_eq!(line(&o, &items()), "temp 91°");
        assert_eq!(color(1), Some("#fbbf24"));
    }
}
