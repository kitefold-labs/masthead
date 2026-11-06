//! `StatusNotifierItem` fallback for desktops without a native Masthead face:
//! Plasma, XFCE, Cinnamon, MATE, Budgie, and GNOME with only the `AppIndicator`
//! extension.
//!
//! Hosts draw items differently. GNOME's `AppIndicator` extension shows a
//! text label beside the icon but never shows tooltips; Plasma and most others
//! ignore labels. So on GNOME each item is the metric's symbolic icon plus a
//! fixed-width label, and elsewhere the value is drawn into the icon.
//!
//! The items live on their own bus connection. Closing that connection is the
//! only reliable way to make every SNI host drop the items, so hiding the tray
//! means dropping [`Tray`].

use crate::glyphs;
use masthead_api::PanelItem;
use zbus::Connection;
use zbus::object_server::SignalEmitter;
use zbus::{ObjectPath, OwnedObjectPath};

pub const WATCHER: &str = "org.kde.StatusNotifierWatcher";

/// As wide as a digit in fonts with tabular figures, which most UI fonts have.
const FIGURE_SPACE: char = '\u{2007}';
/// As wide as a period.
const PUNCTUATION_SPACE: char = '\u{2008}';

type Pixmap = (i32, i32, Vec<u8>);

#[derive(Debug)]
struct Item {
    id: String,
    text: String,
    summary: String,
    icon: Vec<Pixmap>,
    /// The host shows labels: use a themed icon and put the value in the label.
    labels: bool,
    daemon: Connection,
}

impl Item {
    fn label(&self) -> String {
        if self.labels { steady_label(&self.id, &self.text) } else { String::new() }
    }
}

fn title(id: &str) -> String {
    id.parse::<masthead_core::MetricId>().map_or_else(|_| id.to_owned(), |m| m.title().to_owned())
}

// SNI properties are fixed per item and take `&self` because zbus requires it;
#[allow(clippy::unused_self)]
#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl Item {
    #[zbus(property)]
    fn category(&self) -> &'static str {
        "Hardware"
    }
    #[zbus(property)]
    fn id(&self) -> String {
        format!("masthead-{}", self.id)
    }
    #[zbus(property)]
    fn title(&self) -> String {
        format!("Masthead {}", title(&self.id))
    }
    #[zbus(property)]
    fn status(&self) -> &'static str {
        "Active"
    }
    #[zbus(property)]
    fn icon_name(&self) -> String {
        if self.labels { format!("com.kitefoldlabs.Masthead-{}-symbolic", self.id) } else { String::new() }
    }
    #[zbus(property)]
    fn icon_pixmap(&self) -> Vec<Pixmap> {
        self.icon.clone()
    }
    #[zbus(property)]
    fn tool_tip(&self) -> (String, Vec<Pixmap>, String, String) {
        (String::new(), Vec::new(), format!("{} {}", title(&self.id), self.text), self.summary.clone())
    }
    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn menu(&self) -> OwnedObjectPath {
        ObjectPath::from_static_str_unchecked("/NO_DBUSMENU").into()
    }
    /// Ubuntu's `AppIndicator` extension shows this text beside the icon.
    #[zbus(property, name = "XAyatanaLabel")]
    fn x_ayatana_label(&self) -> String {
        self.label()
    }
    #[zbus(property, name = "XAyatanaLabelGuide")]
    fn x_ayatana_label_guide(&self) -> &'static str {
        ""
    }

    async fn activate(&self, x: i32, y: i32) {
        let _ = (x, y);
        crate::window::toggle(&self.daemon, &self.id).await;
    }
    async fn secondary_activate(&self, x: i32, y: i32) {
        let _ = (x, y);
        crate::window::toggle(&self.daemon, &self.id).await;
    }
    async fn context_menu(&self, x: i32, y: i32) {
        let _ = (x, y);
        crate::window::toggle(&self.daemon, "settings").await;
    }
    fn scroll(&self, delta: i32, orientation: &str) {
        let _ = (delta, orientation);
    }

    #[zbus(signal)]
    async fn new_icon(e: &SignalEmitter<'_>) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn new_tool_tip(e: &SignalEmitter<'_>) -> zbus::Result<()>;
    #[zbus(signal, name = "XAyatanaNewLabel")]
    async fn x_ayatana_new_label(e: &SignalEmitter<'_>, label: &str, guide: &str) -> zbus::Result<()>;
}

fn path(id: &str) -> String {
    format!("/StatusNotifierItem/{id}")
}

fn fraction(it: &PanelItem, mem_total: u64) -> Option<f64> {
    match it.id.as_str() {
        "cpu" | "gpu" | "temp" | "battery" => Some(it.value / 100.0),
        "mem" if mem_total > 0 => Some(it.value / mem_total as f64),
        _ => None,
    }
}

/// The icon pixmap. With labels it is only a fallback for a missing themed
/// icon, so it carries the gauge but not the value, which the label shows.
fn pixmap(it: &PanelItem, mem_total: u64, labels: bool) -> Vec<Pixmap> {
    let text = if labels { String::new() } else { glyphs::icon_text(&it.id, it.value) };
    vec![(glyphs::SIZE, glyphs::SIZE, glyphs::render(&text, fraction(it, mem_total), it.level))]
}

/// Pad every number in a bar text to three digits, plus a decimal point for
/// memory and rates, so the label keeps one width as the value changes. `7%`
/// gets two figure spaces in front, `↓37K` a figure space and a punctuation
/// space (the missing decimal point) before the digits.
fn steady_label(id: &str, text: &str) -> String {
    let dotted = matches!(id, "mem" | "net" | "disk");
    let mut out = String::with_capacity(text.len() + 16);
    for (i, token) in text.split(' ').enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let start = token.find(|c: char| c.is_ascii_digit()).unwrap_or(token.len());
        let (prefix, rest) = token.split_at(start);
        let end = rest.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(rest.len());
        let number = &rest[..end];
        let digits = number.chars().filter(char::is_ascii_digit).count();
        out.push_str(prefix);
        out.extend(std::iter::repeat_n(FIGURE_SPACE, 3usize.saturating_sub(digits)));
        if dotted && !number.contains('.') {
            out.push(PUNCTUATION_SPACE);
        }
        out.push_str(rest);
    }
    out
}

/// Whether the tray host is GNOME Shell, whose `AppIndicator` extension shows
/// labels. Other hosts get the value drawn into the icon.
async fn host_shows_labels(conn: &Connection) -> bool {
    let Ok(dbus) = zbus::fdo::DBusProxy::new(conn).await else { return false };
    let Ok(name) = zbus::names::BusName::try_from(WATCHER) else { return false };
    let Ok(owner) = dbus.get_name_owner(name).await else { return false };
    let Ok(pid) = dbus.get_connection_unix_process_id(owner.into()).await else { return false };
    std::fs::read_to_string(format!("/proc/{pid}/comm")).is_ok_and(|c| c.trim_end() == "gnome-shell")
}

#[derive(Debug)]
pub struct Tray {
    conn: Connection,
    ids: Vec<String>,
    shown: Vec<PanelItem>,
    mem_total: u64,
    labels: bool,
}

impl Tray {
    pub async fn start(items: &[PanelItem], daemon: &Connection, mem_total: u64) -> zbus::Result<Self> {
        let conn = zbus::connection::Builder::session().build().await?;
        let labels = host_shows_labels(&conn).await;
        for it in items {
            let item = Item {
                id: it.id.clone(),
                text: it.text.clone(),
                summary: it.summary.clone(),
                icon: pixmap(it, mem_total, labels),
                labels,
                daemon: daemon.clone(),
            };
            conn.object_server().at(path(&it.id), item).await?;
            conn.call_method(Some(WATCHER), "/StatusNotifierWatcher", Some(WATCHER), "RegisterStatusNotifierItem", &(path(&it.id),))
                .await?;
        }
        log::info!("tray items shown for {} metrics{}", items.len(), if labels { " with labels" } else { "" });
        Ok(Self { conn, ids: items.iter().map(|i| i.id.clone()).collect(), shown: items.to_vec(), mem_total, labels })
    }

    /// Whether the item set changed, which needs a restart.
    pub fn same_items(&self, items: &[PanelItem]) -> bool {
        self.ids.len() == items.len() && self.ids.iter().zip(items).all(|(a, b)| *a == b.id)
    }

    pub async fn update(&mut self, items: &[PanelItem]) {
        for (it, old) in items.iter().zip(self.shown.iter()) {
            if it.text == old.text && it.level == old.level && it.summary == old.summary {
                continue;
            }
            let Ok(iface) = self.conn.object_server().interface::<_, Item>(path(&it.id)).await else { continue };
            let label = {
                let mut g = iface.get_mut().await;
                g.text.clone_from(&it.text);
                g.summary.clone_from(&it.summary);
                g.icon = pixmap(it, self.mem_total, self.labels);
                g.label()
            };
            let e = iface.signal_emitter();
            // A themed icon does not change with the value; only the level
            // can change the fallback pixmap.
            if !self.labels || it.level != old.level {
                let _ = Item::new_icon(e).await;
            }
            let _ = Item::new_tool_tip(e).await;
            if self.labels {
                let _ = Item::x_ayatana_new_label(e, &label, "").await;
            }
        }
        self.shown = items.to_vec();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixmaps() {
        let it = PanelItem { id: "mem".into(), value: 8.0 * f64::from(1u32 << 30), text: "8.0G".into(), level: 0, summary: String::new() };
        assert_eq!(fraction(&it, 16 << 30), Some(0.5));
        assert_eq!(pixmap(&it, 0, false)[0].2.len(), 32 * 32 * 4);
        let net = PanelItem { id: "net".into(), ..it };
        assert_eq!(fraction(&net, 1), None);
        assert_eq!(path("cpu"), "/StatusNotifierItem/cpu");
        assert_eq!(title("cpu"), "CPU");
        assert_eq!(title("x"), "x");
    }

    /// Width of a label in a font with tabular digits: digits and figure
    /// spaces are 10, periods and punctuation spaces 3, spaces 4, anything
    /// else 12.
    fn width(s: &str) -> u32 {
        s.chars()
            .map(|c| match c {
                '0'..='9' | FIGURE_SPACE => 10,
                '.' | PUNCTUATION_SPACE => 3,
                ' ' => 4,
                _ => 12,
            })
            .sum()
    }

    #[test]
    fn labels_keep_their_width() {
        let same = |id: &str, texts: &[&str]| {
            let w: Vec<u32> = texts.iter().map(|t| width(&steady_label(id, t))).collect();
            assert!(w.windows(2).all(|p| p[0] == p[1]), "{id}: {texts:?} -> {w:?}");
        };
        same("cpu", &["0%", "7%", "42%", "100%"]);
        same("temp", &["9°", "57°", "101°"]);
        same("mem", &["9.8G", "21.1G", "128G"]);
        same("net", &["↓0B ↑0B", "↓37K ↑67K", "↓8.5K ↑130K", "↓1.2M ↑999K"]);
        assert_eq!(steady_label("cpu", "7%"), "\u{2007}\u{2007}7%");
        assert_eq!(steady_label("net", "↓37K ↑8.5K"), "↓\u{2007}\u{2008}37K ↑\u{2007}8.5K");
        assert_eq!(steady_label("gpu", "–"), "–\u{2007}\u{2007}\u{2007}");
    }
}
