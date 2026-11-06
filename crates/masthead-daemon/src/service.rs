// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! The `com.kitefoldlabs.Masthead1` interface.

use crate::state::{Request, Requests, SharedRef, lock};
use masthead_api::PanelItem;
use masthead_core::MetricId;
use masthead_core::config::Config;
use masthead_core::tiers::Tier;
use std::sync::Arc;
use tokio::sync::oneshot;
use zbus::Connection;
use zbus::fdo;
use zbus::message::Header;
use zbus::object_server::SignalEmitter;

#[derive(Debug)]
pub struct Service {
    pub shared: SharedRef,
    pub requests: Requests,
}

fn sender(h: &Header<'_>) -> fdo::Result<String> {
    h.sender().map(ToString::to_string).ok_or_else(|| fdo::Error::Failed("message has no sender".into()))
}

// zbus passes the message header by value and needs `&self` on every member.
#[allow(clippy::needless_pass_by_value, clippy::unused_self)]
#[zbus::interface(name = "com.kitefoldlabs.Masthead1")]
impl Service {
    fn describe(&self) -> String {
        lock(&self.shared).describe.clone()
    }

    fn subscribe(&self, tier: &str, #[zbus(header)] hdr: Header<'_>) -> fdo::Result<u32> {
        let tier: Tier = tier.parse().map_err(fdo::Error::InvalidArgs)?;
        let owner = sender(&hdr)?;
        let handle = lock(&self.shared).subs.subscribe(&owner, tier);
        log::debug!("{owner} subscribed to {tier} as {handle}");
        let _ = self.requests.try_send(Request::Wake);
        Ok(handle)
    }

    fn unsubscribe(&self, handle: u32, #[zbus(header)] hdr: Header<'_>) -> fdo::Result<()> {
        let owner = sender(&hdr)?;
        if lock(&self.shared).subs.unsubscribe(&owner, handle) {
            Ok(())
        } else {
            Err(fdo::Error::InvalidArgs(format!("no subscription {handle} owned by {owner}")))
        }
    }

    fn snapshot(&self) -> Vec<PanelItem> {
        lock(&self.shared).panel.clone()
    }

    async fn get_detail(&self, metric: &str) -> fdo::Result<String> {
        let metric =
            if metric == "all" { None } else { Some(metric.parse::<MetricId>().map_err(|e| fdo::Error::InvalidArgs(e.to_string()))?) };
        let (reply, rx) = oneshot::channel();
        self.requests.send(Request::Detail { metric, reply }).await.map_err(|_| fdo::Error::Failed("sampler stopped".into()))?;
        rx.await.map_err(|_| fdo::Error::Failed("sampler stopped".into()))
    }

    fn history(&self, series: &str, seconds: u32) -> fdo::Result<(f64, Vec<f64>, Vec<f64>)> {
        let s = lock(&self.shared);
        let h = s
            .history
            .get(series)
            .ok_or_else(|| fdo::Error::InvalidArgs(format!("unknown series {series:?}; expected one of {:?}", masthead_api::SERIES)))?;
        Ok(h.range(seconds.min(24 * 3600)))
    }

    fn toggle_window(&self, metric: &str, #[zbus(connection)] conn: &Connection) -> fdo::Result<()> {
        let m = metric.to_owned();
        if m != "settings" && m.parse::<MetricId>().is_err() {
            return Err(fdo::Error::InvalidArgs(format!("unknown metric {metric:?}")));
        }
        let conn = conn.clone();
        tokio::spawn(async move { crate::window::toggle(&conn, &m).await });
        Ok(())
    }

    fn get_config(&self) -> String {
        serde_json::to_string(&*lock(&self.shared).config).unwrap_or_default()
    }

    async fn set_config(&self, json: &str, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> fdo::Result<()> {
        let cfg: Config = serde_json::from_str(json).map_err(|e| fdo::Error::InvalidArgs(e.to_string()))?;
        cfg.validate().map_err(|e| fdo::Error::InvalidArgs(e.to_string()))?;
        let path = lock(&self.shared).config_path.clone();
        cfg.save(&path).map_err(|e| fdo::Error::Failed(format!("could not write {}: {e}", path.display())))?;
        let out = serde_json::to_string(&cfg).unwrap_or_default();
        lock(&self.shared).config = Arc::new(cfg);
        let _ = Self::config_changed(&emitter, &out).await;
        let _ = self.requests.try_send(Request::Wake);
        Ok(())
    }

    #[zbus(signal)]
    pub async fn config_changed(emitter: &SignalEmitter<'_>, json: &str) -> zbus::Result<()>;

    #[zbus(property)]
    fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_owned()
    }
}
