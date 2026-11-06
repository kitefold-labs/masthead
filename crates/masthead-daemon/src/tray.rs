//! `StatusNotifierItem` fallback for desktops without a native Masthead face:
//! Plasma, XFCE, Cinnamon, MATE, Budgie, and GNOME with only the `AppIndicator`
//! extension.
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

type Pixmap = (i32, i32, Vec<u8>);

#[derive(Debug)]
struct Item {
    id: String,
    text: String,
    summary: String,
    icon: Vec<Pixmap>,
    daemon: Connection,
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
    fn icon_name(&self) -> &'static str {
        ""
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
        self.text.clone()
    }
    #[zbus(property, name = "XAyatanaLabelGuide")]
    fn x_ayatana_label_guide(&self) -> &'static str {
        "100%"
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

fn pixmap(it: &PanelItem, mem_total: u64) -> Vec<Pixmap> {
    let text = glyphs::icon_text(&it.id, it.value);
    vec![(glyphs::SIZE, glyphs::SIZE, glyphs::render(&text, fraction(it, mem_total), it.level))]
}

#[derive(Debug)]
pub struct Tray {
    conn: Connection,
    ids: Vec<String>,
    shown: Vec<PanelItem>,
    mem_total: u64,
}

impl Tray {
    pub async fn start(items: &[PanelItem], daemon: &Connection, mem_total: u64) -> zbus::Result<Self> {
        let conn = zbus::connection::Builder::session().build().await?;
        for it in items {
            let item = Item {
                id: it.id.clone(),
                text: it.text.clone(),
                summary: it.summary.clone(),
                icon: pixmap(it, mem_total),
                daemon: daemon.clone(),
            };
            conn.object_server().at(path(&it.id), item).await?;
            conn.call_method(Some(WATCHER), "/StatusNotifierWatcher", Some(WATCHER), "RegisterStatusNotifierItem", &(path(&it.id),))
                .await?;
        }
        log::info!("tray items shown for {} metrics", items.len());
        Ok(Self { conn, ids: items.iter().map(|i| i.id.clone()).collect(), shown: items.to_vec(), mem_total })
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
            {
                let mut g = iface.get_mut().await;
                g.text.clone_from(&it.text);
                g.summary.clone_from(&it.summary);
                g.icon = pixmap(it, self.mem_total);
            }
            let e = iface.signal_emitter();
            let _ = Item::new_icon(e).await;
            let _ = Item::new_tool_tip(e).await;
            let _ = Item::x_ayatana_new_label(e, &it.text, "100%").await;
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
        assert_eq!(pixmap(&it, 0)[0].2.len(), 32 * 32 * 4);
        let net = PanelItem { id: "net".into(), ..it };
        assert_eq!(fraction(&net, 1), None);
        assert_eq!(path("cpu"), "/StatusNotifierItem/cpu");
        assert_eq!(title("cpu"), "CPU");
        assert_eq!(title("x"), "x");
    }
}
