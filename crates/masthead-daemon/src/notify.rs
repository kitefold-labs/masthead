//! Desktop notifications for metrics that stay critical.

use std::collections::HashMap;
use zbus::Connection;
use zbus::Variant as Value;

pub async fn send(conn: &Connection, summary: &str, body: &str) {
    let hints: HashMap<&str, Value<'_>> =
        HashMap::from([("urgency", Value::U8(2)), ("desktop-entry", Value::from("com.kitefoldlabs.Masthead"))]);
    let actions: Vec<&str> = Vec::new();
    let r = conn
        .call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &("Masthead", 0u32, "com.kitefoldlabs.Masthead", summary, body, actions, hints, -1i32),
        )
        .await;
    if let Err(e) = r {
        log::warn!("notification failed: {e}");
    }
}
