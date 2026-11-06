//! State shared between the D-Bus service and the sampling loop.

use masthead_api::PanelItem;
use masthead_core::MetricId;
use masthead_core::config::Config;
use masthead_core::ring::History;
use masthead_core::tiers::{Demand, Subscriptions};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};

#[derive(Debug)]
pub struct Shared {
    pub subs: Subscriptions,
    pub config: Arc<Config>,
    pub config_path: PathBuf,
    pub panel: Vec<PanelItem>,
    pub history: HashMap<&'static str, History>,
    pub describe: String,
    /// Tray items are showing, which needs panel-tier sampling.
    pub tray_active: bool,
}

impl Shared {
    pub fn new(config: Config, config_path: PathBuf) -> Self {
        Self {
            subs: Subscriptions::default(),
            config: Arc::new(config),
            config_path,
            panel: Vec::new(),
            history: masthead_api::SERIES.iter().map(|s| (*s, History::new())).collect(),
            describe: String::from("{}"),
            tray_active: false,
        }
    }

    pub fn demand(&self) -> Demand {
        let mut d = self.subs.demand();
        d.panel |= self.tray_active;
        d
    }
}

pub type SharedRef = Arc<Mutex<Shared>>;

/// Lock, recovering from a poisoned mutex: the state stays consistent because
/// every critical section is a few field assignments.
pub fn lock(s: &SharedRef) -> std::sync::MutexGuard<'_, Shared> {
    s.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Debug)]
pub enum Request {
    /// Demand or configuration changed: sample and publish now.
    Wake,
    /// Build detail from the latest sample.
    Detail { metric: Option<MetricId>, reply: oneshot::Sender<String> },
}

pub type Requests = mpsc::Sender<Request>;
