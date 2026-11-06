// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! System batteries from `/sys/class/power_supply`.

use crate::source::{Buf, Source};
use crate::{Roots, read_attr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChargeState {
    Charging,
    Discharging,
    Full,
    NotCharging,
    Unknown,
}

impl ChargeState {
    fn parse(s: &[u8]) -> Self {
        match s {
            b"Charging" => Self::Charging,
            b"Discharging" => Self::Discharging,
            b"Full" => Self::Full,
            b"Not charging" => Self::NotCharging,
            _ => Self::Unknown,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Charging => "charging",
            Self::Discharging => "discharging",
            Self::Full => "full",
            Self::NotCharging => "not charging",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BatterySample {
    pub percent: Option<f64>,
    /// Charge or discharge rate in watts.
    pub watts: Option<f64>,
    pub state: ChargeState,
}

#[derive(Debug)]
pub struct Battery {
    pub name: String,
    capacity: Option<Source>,
    power_now: Option<Source>,
    current_now: Option<Source>,
    voltage_now: Option<Source>,
    status: Option<Source>,
}

/// System batteries only: `type` is `Battery` and `scope` is not `Device`,
/// which excludes mice, keyboards and headsets.
pub fn discover(roots: &Roots) -> Vec<Battery> {
    let dir = roots.sys.join("class/power_supply");
    let Ok(rd) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut out: Vec<Battery> = rd
        .filter_map(Result::ok)
        .filter_map(|e| {
            let p = dir.join(e.file_name());
            if read_attr(&p.join("type")).as_deref() != Some("Battery") || read_attr(&p.join("scope")).as_deref() == Some("Device") {
                return None;
            }
            let open = |f: &str| Source::open(&p.join(f)).ok();
            Some(Battery {
                name: e.file_name().to_string_lossy().into_owned(),
                capacity: open("capacity"),
                power_now: open("power_now"),
                current_now: open("current_now"),
                voltage_now: open("voltage_now"),
                status: open("status"),
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

impl Battery {
    pub fn sample(&self, buf: &mut Buf) -> BatterySample {
        let percent = self.capacity.as_ref().and_then(|s| s.read_i64(buf)).map(|v| v as f64);
        let watts = self.power_now.as_ref().and_then(|s| s.read_i64(buf)).map(|uw| uw.unsigned_abs() as f64 / 1e6).or_else(|| {
            let ua = self.current_now.as_ref()?.read_i64(buf)?;
            let uv = self.voltage_now.as_ref()?.read_i64(buf)?;
            Some((ua.unsigned_abs() as f64) * (uv.unsigned_abs() as f64) / 1e12)
        });
        let state = self.status.as_ref().and_then(|s| s.read_word(buf)).map_or(ChargeState::Unknown, ChargeState::parse);
        BatterySample { percent, watts, state }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batteries() {
        let d = tempfile::tempdir().unwrap();
        let ps = d.path().join("sys/class/power_supply");
        let b = ps.join("BAT0");
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(b.join("type"), "Battery\n").unwrap();
        std::fs::write(b.join("capacity"), "81\n").unwrap();
        std::fs::write(b.join("current_now"), "1000000\n").unwrap();
        std::fs::write(b.join("voltage_now"), "12000000\n").unwrap();
        std::fs::write(b.join("status"), "Discharging\n").unwrap();
        let m = ps.join("hidpp_battery_0");
        std::fs::create_dir_all(&m).unwrap();
        std::fs::write(m.join("type"), "Battery\n").unwrap();
        std::fs::write(m.join("scope"), "Device\n").unwrap();
        let ac = ps.join("AC");
        std::fs::create_dir_all(&ac).unwrap();
        std::fs::write(ac.join("type"), "Mains\n").unwrap();

        let bats = discover(&Roots::under(d.path()));
        assert_eq!(bats.len(), 1);
        let s = bats[0].sample(&mut Buf::default());
        assert_eq!(s.percent, Some(81.0));
        assert!((s.watts.unwrap() - 12.0).abs() < 1e-9);
        assert_eq!(s.state, ChargeState::Discharging);
        assert_eq!(s.state.as_str(), "discharging");
        std::fs::write(b.join("power_now"), "-9500000\n").unwrap();
        let bats = discover(&Roots::under(d.path()));
        assert!((bats[0].sample(&mut Buf::default()).watts.unwrap() - 9.5).abs() < 1e-9);
    }
}
