// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! Threshold evaluation with hysteresis, and sustained-alert tracking.

use crate::Level;
use serde::{Deserialize, Serialize};

/// Values at or above `warn` are amber, at or above `crit` red. Units are the
/// metric's own: percent for CPU, GPU and memory, °C for temperature.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Threshold {
    pub warn: f64,
    pub crit: f64,
}

/// How far a value must fall below a threshold before its level drops, so a
/// value hovering at 85% does not flicker between colors.
pub const HYSTERESIS: f64 = 2.0;

impl Threshold {
    pub fn level(&self, value: f64, previous: Level) -> Level {
        let crit_exit = if previous == Level::Crit { self.crit - HYSTERESIS } else { self.crit };
        let warn_exit = if previous >= Level::Warn { self.warn - HYSTERESIS } else { self.warn };
        if value >= crit_exit {
            Level::Crit
        } else if value >= warn_exit {
            Level::Warn
        } else {
            Level::Normal
        }
    }
}

/// Fires once when a metric has stayed critical for `hold` seconds, and re-arms
/// after it leaves the critical level.
#[derive(Debug, Clone, Default)]
pub struct SustainedAlert {
    since: Option<u64>,
    fired: bool,
}

impl SustainedAlert {
    /// Feed the current level at time `now` (seconds). Returns true exactly once
    /// per critical episode.
    pub fn update(&mut self, level: Level, now: u64, hold: u64) -> bool {
        if level != Level::Crit {
            self.since = None;
            self.fired = false;
            return false;
        }
        let since = *self.since.get_or_insert(now);
        if !self.fired && now.saturating_sub(since) >= hold {
            self.fired = true;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hysteresis() {
        let t = Threshold { warn: 85.0, crit: 95.0 };
        assert_eq!(t.level(50.0, Level::Normal), Level::Normal);
        assert_eq!(t.level(85.0, Level::Normal), Level::Warn);
        assert_eq!(t.level(84.0, Level::Warn), Level::Warn);
        assert_eq!(t.level(82.9, Level::Warn), Level::Normal);
        assert_eq!(t.level(96.0, Level::Warn), Level::Crit);
        assert_eq!(t.level(94.0, Level::Crit), Level::Crit);
        assert_eq!(t.level(92.0, Level::Crit), Level::Warn);
    }

    #[test]
    fn sustained() {
        let mut a = SustainedAlert::default();
        assert!(!a.update(Level::Crit, 100, 60));
        assert!(!a.update(Level::Crit, 159, 60));
        assert!(a.update(Level::Crit, 160, 60));
        assert!(!a.update(Level::Crit, 200, 60));
        assert!(!a.update(Level::Warn, 201, 60));
        assert!(!a.update(Level::Crit, 300, 60));
        assert!(a.update(Level::Crit, 360, 60));
    }
}
