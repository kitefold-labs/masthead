// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! JSON payloads carried by `Describe`, `GetDetail` and the `Detail` signal.
//!
//! Every field is optional or defaulted on the reading side, so a face built
//! against an older daemon keeps working when fields are added.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Describe {
    pub version: String,
    pub hostname: String,
    pub kernel: String,
    pub cpu_model: String,
    pub cores: u32,
    pub threads: u32,
    pub mem_total: u64,
    pub gpus: Vec<GpuInfo>,
    pub sensors: Vec<SensorInfo>,
    pub interfaces: Vec<IfaceInfo>,
    pub disks: Vec<DiskInfo>,
    pub has_battery: bool,
    /// Metrics this machine can show.
    pub metrics: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GpuInfo {
    pub pci: String,
    pub name: String,
    pub vendor: String,
    pub discrete: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SensorInfo {
    pub key: String,
    pub name: String,
    pub kind: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct IfaceInfo {
    pub name: String,
    /// `ethernet`, `wireless`, `virtual` or `loopback`.
    pub kind: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiskInfo {
    pub name: String,
    pub model: String,
    pub size: u64,
}

/// Detail for one metric or for `all`. Sections not requested are absent.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Detail {
    /// Unix time of the sample, in seconds.
    pub t: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<SystemDetail>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu: Option<CpuDetail>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mem: Option<MemDetail>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpus: Option<Vec<GpuDetail>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub net: Option<NetDetail>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disk: Option<DiskDetail>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sensors: Option<Vec<SensorDetail>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub batteries: Option<Vec<BatteryDetail>>,
    /// Window tier only: the process table, busiest first.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub processes: Option<Vec<ProcDetail>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SystemDetail {
    pub uptime: f64,
    pub load: [f64; 3],
    pub procs_total: u32,
    pub procs_running: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CpuDetail {
    pub model: String,
    pub cores: u32,
    pub threads: u32,
    /// Busy percent across all cores.
    pub total: f64,
    /// Busy percent per logical CPU. Hover and window tiers.
    pub per_core: Vec<f32>,
    /// MHz per logical CPU. Hover and window tiers.
    pub freq_mhz: Vec<f32>,
    pub avg_freq_mhz: f64,
    pub package_temp_c: Option<f64>,
    pub load: [f64; 3],
    /// Top processes by CPU share.
    pub top: Vec<ProcDetail>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MemDetail {
    pub total: u64,
    pub used: u64,
    pub cache: u64,
    pub available: u64,
    pub free: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    /// Top processes by resident memory.
    pub top: Vec<ProcDetail>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GpuDetail {
    pub pci: String,
    pub name: String,
    pub vendor: String,
    pub driver: String,
    pub discrete: bool,
    /// The GPU the bar shows.
    pub primary: bool,
    pub util: Option<f64>,
    pub vram_used: Option<u64>,
    pub vram_total: Option<u64>,
    pub temp_c: Option<f64>,
    pub power_w: Option<f64>,
    pub power_limit_w: Option<f64>,
    /// Runtime-suspended; not queried.
    pub suspended: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NetDetail {
    pub rx: f64,
    pub tx: f64,
    pub interfaces: Vec<IfaceDetail>,
    /// Virtual interfaces left out of the list and the total.
    pub hidden_virtual: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct IfaceDetail {
    pub name: String,
    pub kind: String,
    pub driver: String,
    pub speed_mbps: Option<u32>,
    pub rx: f64,
    pub tx: f64,
    pub rx_total: u64,
    pub tx_total: u64,
    /// Counted toward the bar total.
    pub counted: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiskDetail {
    pub read: f64,
    pub write: f64,
    pub devices: Vec<DiskDevDetail>,
    /// Window tier only.
    pub filesystems: Vec<FsDetail>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiskDevDetail {
    pub name: String,
    pub model: String,
    pub size: u64,
    pub read: f64,
    pub write: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FsDetail {
    pub mount: String,
    pub device: String,
    pub fstype: String,
    pub total: u64,
    pub used: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SensorDetail {
    /// `chip/label`, used in config.
    pub key: String,
    pub name: String,
    /// `temp`, `fan`, `voltage` or `power`.
    pub kind: String,
    pub value: Option<f64>,
    /// `°C`, `RPM`, `V` or `W`.
    pub unit: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BatteryDetail {
    pub name: String,
    pub percent: Option<f64>,
    pub watts: Option<f64>,
    pub state: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProcDetail {
    pub pid: u32,
    pub name: String,
    pub user: String,
    /// Percent of one core, as `top` shows it: 100 is one core busy.
    pub cpu: f64,
    pub rss: u64,
    pub threads: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tolerant_parsing() {
        let d: Detail = serde_json::from_str(r#"{"t":5,"cpu":{"total":12.5,"new_field":1},"future":[]}"#).unwrap();
        assert_eq!(d.t, 5);
        assert!((d.cpu.unwrap().total - 12.5).abs() < 1e-9);
        let s = serde_json::to_string(&Detail::default()).unwrap();
        assert_eq!(s, r#"{"t":0}"#);
        let x: Describe = serde_json::from_str("{}").unwrap();
        assert_eq!(x.gpus.len(), 0);
    }
}
