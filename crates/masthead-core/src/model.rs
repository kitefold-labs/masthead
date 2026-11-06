//! Metric identifiers and alert levels.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// A metric that can appear in the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MetricId {
    Cpu,
    Mem,
    Gpu,
    Net,
    Disk,
    Temp,
    Battery,
}

impl MetricId {
    pub const ALL: [MetricId; 7] = [Self::Cpu, Self::Mem, Self::Gpu, Self::Net, Self::Disk, Self::Temp, Self::Battery];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Mem => "mem",
            Self::Gpu => "gpu",
            Self::Net => "net",
            Self::Disk => "disk",
            Self::Temp => "temp",
            Self::Battery => "battery",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Cpu => "CPU",
            Self::Mem => "Memory",
            Self::Gpu => "GPU",
            Self::Net => "Network",
            Self::Disk => "Disk",
            Self::Temp => "Temperature",
            Self::Battery => "Battery",
        }
    }
}

impl fmt::Display for MetricId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownMetric(pub String);

impl fmt::Display for UnknownMetric {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown metric {:?}", self.0)
    }
}

impl std::error::Error for UnknownMetric {}

impl FromStr for MetricId {
    type Err = UnknownMetric;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL.into_iter().find(|m| m.as_str() == s).ok_or_else(|| UnknownMetric(s.to_owned()))
    }
}

/// How urgent a value is. Faces color only `Warn` and `Crit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    #[default]
    Normal = 0,
    Warn = 1,
    Crit = 2,
}

impl Level {
    pub fn from_u32(v: u32) -> Self {
        match v {
            1 => Self::Warn,
            2 => Self::Crit,
            _ => Self::Normal,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Warn => "warn",
            Self::Crit => "crit",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        for m in MetricId::ALL {
            assert_eq!(m.as_str().parse::<MetricId>().unwrap(), m);
            assert_eq!(m.to_string(), m.as_str());
            assert_ne!(m.title(), "");
        }
        let e = "x".parse::<MetricId>().unwrap_err();
        assert!(e.to_string().contains("\"x\""));
        assert_eq!(Level::from_u32(2), Level::Crit);
        assert_eq!(Level::from_u32(1), Level::Warn);
        assert_eq!(Level::from_u32(9), Level::Normal);
        assert_eq!(Level::Warn.as_str(), "warn");
    }
}
