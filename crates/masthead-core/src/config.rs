//! `~/.config/masthead/config.toml`. Every field has a default, so an empty or
//! partial file is valid. The daemon owns the file; faces change it through
//! the daemon's `SetConfig` method.

use crate::MetricId;
use crate::thresholds::Threshold;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryUnits {
    #[default]
    Binary,
    Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NetworkUnits {
    #[default]
    Bytes,
    Bits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TemperatureUnits {
    #[default]
    Celsius,
    Fahrenheit,
}

/// How faces draw each bar item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BarStyle {
    /// Icon and value.
    #[default]
    Values,
    /// Icon, value and a small trend line.
    Trend,
    /// Icon and trend line only.
    Compact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrayMode {
    /// Show tray items only when no native face is subscribed.
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Bar {
    pub metrics: Vec<MetricId>,
    pub style: BarStyle,
}

impl Default for Bar {
    fn default() -> Self {
        Self { metrics: vec![MetricId::Cpu, MetricId::Mem, MetricId::Gpu, MetricId::Net, MetricId::Temp], style: BarStyle::Values }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Units {
    pub memory: MemoryUnits,
    pub network: NetworkUnits,
    pub temperature: TemperatureUnits,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Devices {
    /// PCI address of the GPU the bar shows, or `auto` for the busiest discrete GPU.
    pub gpu: String,
    /// `chip/label` of the temperature sensor the bar shows, or `auto`.
    pub temperature: String,
    /// Interfaces counted toward the network total. Empty means every physical one.
    pub interfaces: Vec<String>,
    /// Count virtual interfaces (docker0, veth*, br-*) too.
    pub include_virtual_interfaces: bool,
    /// Read NVIDIA GPUs through NVIDIA's NVML library while GPU data is shown.
    /// It costs about 20 MB of memory; without it NVIDIA GPUs show no load.
    pub nvml: bool,
}

impl Default for Devices {
    fn default() -> Self {
        Self { gpu: "auto".into(), temperature: "auto".into(), interfaces: Vec::new(), include_virtual_interfaces: false, nvml: true }
    }
}

impl Devices {
    pub fn gpu_preference(&self) -> Option<&str> {
        (!self.gpu.is_empty() && self.gpu != "auto").then_some(self.gpu.as_str())
    }

    pub fn temperature_preference(&self) -> Option<&str> {
        (!self.temperature.is_empty() && self.temperature != "auto").then_some(self.temperature.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Tray {
    pub mode: TrayMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Notifications {
    /// Send a desktop notification when a metric stays critical.
    pub enabled: bool,
    pub hold_seconds: u64,
}

impl Default for Notifications {
    fn default() -> Self {
        Self { enabled: false, hold_seconds: 60 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub bar: Bar,
    pub units: Units,
    pub thresholds: BTreeMap<MetricId, Threshold>,
    pub devices: Devices,
    pub tray: Tray,
    pub notifications: Notifications,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bar: Bar::default(),
            units: Units::default(),
            thresholds: default_thresholds(),
            devices: Devices::default(),
            tray: Tray::default(),
            notifications: Notifications::default(),
        }
    }
}

pub fn default_thresholds() -> BTreeMap<MetricId, Threshold> {
    BTreeMap::from([
        (MetricId::Cpu, Threshold { warn: 85.0, crit: 95.0 }),
        (MetricId::Mem, Threshold { warn: 90.0, crit: 95.0 }),
        (MetricId::Temp, Threshold { warn: 80.0, crit: 90.0 }),
    ])
}

#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Parse(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Parse(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    /// `$XDG_CONFIG_HOME/masthead/config.toml`, else `~/.config/masthead/config.toml`.
    pub fn default_path() -> PathBuf {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .unwrap_or_else(|| PathBuf::from("."));
        base.join("masthead").join("config.toml")
    }

    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        let mut c: Config = toml::from_str(text).map_err(|e| ConfigError::Parse(e.to_string()))?;
        // Thresholds given in the file override defaults per metric.
        let mut t = default_thresholds();
        t.append(&mut c.thresholds);
        c.thresholds = t;
        c.validate()?;
        Ok(c)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        for (m, t) in &self.thresholds {
            if !(t.warn.is_finite() && t.crit.is_finite()) || t.warn > t.crit {
                return Err(ConfigError::Parse(format!("thresholds.{m}: warn must be finite and not above crit")));
            }
        }
        Ok(())
    }

    /// Load the file; a missing file gives the defaults.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        match std::fs::read_to_string(path) {
            Ok(s) => Self::parse(&s),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(ConfigError::Io(e)),
        }
    }

    pub fn to_toml(&self) -> String {
        toml::to_string(self).unwrap_or_default()
    }

    /// Write atomically: a temporary file in the same directory, then rename.
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        let dir = path.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(dir).map_err(ConfigError::Io)?;
        let tmp = dir.join(".config.toml.tmp");
        std::fs::write(&tmp, self.to_toml()).map_err(ConfigError::Io)?;
        std::fs::rename(&tmp, path).map_err(ConfigError::Io)
    }

    pub fn threshold(&self, m: MetricId) -> Option<Threshold> {
        self.thresholds.get(&m).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_partial() {
        let c = Config::parse("").unwrap();
        assert_eq!(c, Config::default());
        assert_eq!(c.bar.metrics.len(), 5);
        let c = Config::parse(
            "[bar]\nmetrics=[\"cpu\",\"disk\"]\nstyle=\"trend\"\n[units]\nnetwork=\"bits\"\n[thresholds.cpu]\nwarn=70\ncrit=90\n",
        )
        .unwrap();
        assert_eq!(c.bar.metrics, vec![MetricId::Cpu, MetricId::Disk]);
        assert_eq!(c.bar.style, BarStyle::Trend);
        assert_eq!(c.units.network, NetworkUnits::Bits);
        assert_eq!(c.threshold(MetricId::Cpu), Some(Threshold { warn: 70.0, crit: 90.0 }));
        assert_eq!(c.threshold(MetricId::Temp), Some(Threshold { warn: 80.0, crit: 90.0 }));
        assert_eq!(c.threshold(MetricId::Net), None);
    }

    #[test]
    fn rejects_bad() {
        assert!(Config::parse("[bar]\nmetrics=[\"fan\"]\n").is_err());
        assert!(Config::parse("[nope]\n").is_err());
        let e = Config::parse("[thresholds.cpu]\nwarn=99\ncrit=10\n").unwrap_err();
        assert!(e.to_string().contains("thresholds.cpu"));
    }

    #[test]
    fn devices() {
        let c = Config::default();
        assert_eq!(c.devices.gpu_preference(), None);
        assert_eq!(c.devices.temperature_preference(), None);
        let c = Config::parse("[devices]\ngpu=\"0000:01:00.0\"\ntemperature=\"k10temp/Tctl\"\n").unwrap();
        assert_eq!(c.devices.gpu_preference(), Some("0000:01:00.0"));
        assert_eq!(c.devices.temperature_preference(), Some("k10temp/Tctl"));
    }

    #[test]
    fn save_load_round_trip() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("masthead/config.toml");
        assert_eq!(Config::load(&p).unwrap(), Config::default());
        let mut c = Config::default();
        c.units.temperature = TemperatureUnits::Fahrenheit;
        c.notifications.enabled = true;
        c.save(&p).unwrap();
        assert_eq!(Config::load(&p).unwrap(), c);
        std::fs::write(&p, "garbage =").unwrap();
        assert!(Config::load(&p).is_err());
        let e = Config::load(d.path()).unwrap_err();
        assert!(!e.to_string().is_empty());
    }

    #[test]
    fn default_path_uses_xdg() {
        let p = Config::default_path();
        assert!(p.ends_with("masthead/config.toml"));
    }
}
