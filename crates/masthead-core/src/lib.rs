//! Logic shared by the daemon and every Masthead face. No I/O beyond reading
//! and writing the config file.

pub mod config;
pub mod format;
pub mod model;
pub mod ring;
pub mod thresholds;
pub mod tiers;

pub use model::{Level, MetricId};
