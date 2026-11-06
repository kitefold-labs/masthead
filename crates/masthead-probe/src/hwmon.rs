// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! Temperatures, fans, voltages and power from `/sys/class/hwmon`.

use crate::source::{Buf, Source};
use crate::{Roots, link_name, read_attr};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensorKind {
    /// Degrees Celsius.
    Temp,
    /// Revolutions per minute.
    Fan,
    /// Volts.
    Voltage,
    /// Watts.
    Power,
}

#[derive(Debug)]
pub struct Sensor {
    /// The hwmon chip name, such as `k10temp`, `nvme` or `amdgpu`.
    pub chip: String,
    /// The device behind the chip, such as `nvme0` or `0000:13:00.0`.
    pub device: Option<String>,
    /// The channel label, such as `Tctl` or `Composite`, else the file stem.
    pub label: String,
    pub kind: SensorKind,
    pub path: PathBuf,
    src: Option<Source>,
    pub value: Option<f64>,
}

impl Sensor {
    /// A stable identifier for configuration: `chip/label`.
    pub fn key(&self) -> String {
        format!("{}/{}", self.chip, self.label)
    }

    /// Read the current value, opening the file on first use.
    pub fn read(&mut self, buf: &mut Buf) -> Option<f64> {
        if self.src.is_none() {
            self.src = Source::open(&self.path).ok();
        }
        let raw = self.src.as_ref()?.read_i64(buf)? as f64;
        let v = match self.kind {
            SensorKind::Temp | SensorKind::Voltage => raw / 1000.0,
            SensorKind::Fan => raw,
            SensorKind::Power => raw / 1_000_000.0,
        };
        self.value = Some(v);
        self.value
    }
}

fn channel(file: &str) -> Option<(SensorKind, &str)> {
    for (prefix, kind) in [("temp", SensorKind::Temp), ("fan", SensorKind::Fan), ("in", SensorKind::Voltage), ("power", SensorKind::Power)]
    {
        if let Some(rest) = file.strip_prefix(prefix) {
            let n_end = rest.find(|c: char| !c.is_ascii_digit())?;
            if n_end == 0 {
                return None;
            }
            let suffix = &rest[n_end..];
            let wanted = if kind == SensorKind::Power { suffix == "_average" || suffix == "_input" } else { suffix == "_input" };
            if wanted {
                return Some((kind, &file[..prefix.len() + n_end]));
            }
        }
    }
    None
}

/// Every readable channel on every hwmon chip.
pub fn discover(roots: &Roots) -> Vec<Sensor> {
    let dir = roots.sys.join("class/hwmon");
    let Ok(rd) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut chips: Vec<PathBuf> = rd.filter_map(Result::ok).map(|e| dir.join(e.file_name())).collect();
    chips.sort_by_key(|p| p.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_prefix("hwmon")).and_then(|n| n.parse::<u32>().ok()));
    let mut out = Vec::new();
    for chip_dir in chips {
        out.extend(discover_chip(&chip_dir));
    }
    out
}

/// Channels of one hwmon directory (also used for `device/hwmon/hwmonN` under a GPU).
pub fn discover_chip(chip_dir: &Path) -> Vec<Sensor> {
    let chip = read_attr(&chip_dir.join("name")).unwrap_or_else(|| "hwmon".into());
    let device = link_name(&chip_dir.join("device"));
    let Ok(rd) = std::fs::read_dir(chip_dir) else { return Vec::new() };
    let mut files: Vec<String> = rd.filter_map(Result::ok).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    files.sort();
    let mut out: Vec<Sensor> = Vec::new();
    for f in &files {
        let Some((kind, stem)) = channel(f) else { continue };
        // power1_average and power1_input both exist on some chips; keep one.
        if kind == SensorKind::Power && f.ends_with("_input") && files.iter().any(|g| *g == format!("{stem}_average")) {
            continue;
        }
        let label = read_attr(&chip_dir.join(format!("{stem}_label"))).unwrap_or_else(|| stem.to_owned());
        out.push(Sensor { chip: chip.clone(), device: device.clone(), label, kind, path: chip_dir.join(f), src: None, value: None });
    }
    out
}

/// The sensor that best represents CPU package temperature.
pub fn cpu_package(sensors: &[Sensor]) -> Option<usize> {
    const PREFS: &[(&str, &str)] = &[
        ("k10temp", "Tctl"),
        ("k10temp", "Tdie"),
        ("zenpower", "Tdie"),
        ("zenpower", "Tctl"),
        ("coretemp", "Package id 0"),
        ("cpu_thermal", "temp1"),
        ("acpitz", "temp1"),
    ];
    PREFS.iter().find_map(|(chip, label)| sensors.iter().position(|s| s.kind == SensorKind::Temp && s.chip == *chip && s.label == *label))
}

/// A human label: `nvme0 Composite`, `k10temp Tctl`.
pub fn display_name(s: &Sensor) -> String {
    match (s.chip.as_str(), s.device.as_deref()) {
        ("nvme", Some(dev)) => format!("{dev} {}", s.label),
        ("spd5118" | "jc42", Some(dev)) => format!("DIMM {dev}"),
        _ => format!("{} {}", s.chip, s.label),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn discovers_and_reads() {
        let d = tempfile::tempdir().unwrap();
        let h = d.path().join("sys/class/hwmon");
        let k = h.join("hwmon2");
        std::fs::create_dir_all(&k).unwrap();
        std::fs::write(k.join("name"), "k10temp\n").unwrap();
        std::fs::write(k.join("temp1_input"), "64250\n").unwrap();
        std::fs::write(k.join("temp1_label"), "Tctl\n").unwrap();
        std::fs::write(k.join("temp3_input"), "-1000\n").unwrap();
        std::fs::write(k.join("temp1_crit"), "100000\n").unwrap();
        let n = h.join("hwmon10");
        std::fs::create_dir_all(&n).unwrap();
        std::fs::write(n.join("name"), "nvme\n").unwrap();
        std::fs::write(n.join("temp1_input"), "41850\n").unwrap();
        std::fs::write(n.join("temp1_label"), "Composite\n").unwrap();
        std::fs::write(n.join("fan1_input"), "1200\n").unwrap();
        std::fs::write(n.join("in0_input"), "1212\n").unwrap();
        std::fs::write(n.join("power1_average"), "182000000\n").unwrap();
        std::fs::write(n.join("power1_input"), "1\n").unwrap();
        let dev = d.path().join("sys/devices/nvme0");
        std::fs::create_dir_all(&dev).unwrap();
        symlink(&dev, n.join("device")).unwrap();

        let mut s = discover(&Roots::under(d.path()));
        assert_eq!(s.len(), 6, "{s:#?}");
        let cpu = cpu_package(&s).unwrap();
        let mut b = Buf::default();
        assert_eq!(s[cpu].read(&mut b), Some(64.25));
        assert_eq!(s[cpu].key(), "k10temp/Tctl");
        assert_eq!(display_name(&s[cpu]), "k10temp Tctl");
        assert_eq!(s[1].read(&mut b), Some(-1.0));
        let nv = s.iter().position(|x| x.chip == "nvme" && x.kind == SensorKind::Temp).unwrap();
        assert_eq!(display_name(&s[nv]), "nvme0 Composite");
        let fan = s.iter_mut().find(|x| x.kind == SensorKind::Fan).unwrap();
        assert_eq!(fan.read(&mut b), Some(1200.0));
        let p = s.iter_mut().find(|x| x.kind == SensorKind::Power).unwrap();
        assert_eq!(p.read(&mut b), Some(182.0));
        let v = s.iter_mut().find(|x| x.kind == SensorKind::Voltage).unwrap();
        assert!((v.read(&mut b).unwrap() - 1.212).abs() < 1e-9);
    }

    #[test]
    fn channel_names() {
        assert_eq!(channel("temp1_input"), Some((SensorKind::Temp, "temp1")));
        assert_eq!(channel("temp_input"), None);
        assert_eq!(channel("temp1_max"), None);
        assert_eq!(channel("intrusion0_alarm"), None);
        assert_eq!(channel("power2_average"), Some((SensorKind::Power, "power2")));
    }
}
