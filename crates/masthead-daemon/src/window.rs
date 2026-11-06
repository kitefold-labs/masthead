//! Opening and closing the window.

use masthead_api::WindowProxy;
use std::path::PathBuf;
use zbus::Connection;

/// Toggle the window on `metric`. The first call D-Bus-activates
/// `masthead-window`; without an installed service file, start it directly.
pub async fn toggle(conn: &Connection, metric: &str) {
    match WindowProxy::new(conn).await {
        Ok(p) => match p.toggle(metric).await {
            Ok(()) => return,
            Err(e) => log::info!("window not reachable over D-Bus ({e}); starting it directly"),
        },
        Err(e) => log::warn!("window proxy: {e}"),
    }
    spawn(metric);
}

fn window_binary() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("masthead-window")))
        .filter(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from("masthead-window"))
}

fn spawn(metric: &str) {
    match std::process::Command::new(window_binary()).arg("--metric").arg(metric).spawn() {
        Ok(mut child) => {
            // Reap it when it exits so it does not linger as a zombie.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(e) => log::warn!("could not start masthead-window: {e}"),
    }
}
