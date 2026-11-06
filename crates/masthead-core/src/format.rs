//! Display formatting shared by every face, so the bar reads the same in
//! GNOME, COSMIC, Waybar and the tray.

use crate::config::{MemoryUnits, NetworkUnits, TemperatureUnits};

/// `23%`.
pub fn percent(v: f64) -> String {
    format!("{}%", v.round().clamp(0.0, 100.0) as u32)
}

/// Memory as a compact bar label: `18.4G` (binary GiB or decimal GB).
pub fn mem_short(bytes: u64, units: MemoryUnits) -> String {
    let div = match units {
        MemoryUnits::Binary => 1024f64.powi(3),
        MemoryUnits::Decimal => 1e9,
    };
    let g = bytes as f64 / div;
    if g >= 100.0 { format!("{g:.0}G") } else { format!("{g:.1}G") }
}

/// Memory with a unit for popovers: `18.4 GiB`.
pub fn mem_long(bytes: u64, units: MemoryUnits) -> String {
    let (div, unit) = match units {
        MemoryUnits::Binary => (1024f64.powi(3), "GiB"),
        MemoryUnits::Decimal => (1e9, "GB"),
    };
    let g = bytes as f64 / div;
    if g < 1.0 {
        let (mdiv, munit) = match units {
            MemoryUnits::Binary => (1024f64.powi(2), "MiB"),
            MemoryUnits::Decimal => (1e6, "MB"),
        };
        format!("{:.0} {munit}", bytes as f64 / mdiv)
    } else {
        format!("{g:.1} {unit}")
    }
}

fn scaled(v: f64, base: f64) -> (f64, usize) {
    let mut v = v.max(0.0);
    let mut i = 0;
    while v >= base && i < 4 {
        v /= base;
        i += 1;
    }
    (v, i)
}

fn trim_num(v: f64, i: usize) -> String {
    if i > 0 && v < 10.0 { format!("{v:.1}") } else { format!("{:.0}", v.round()) }
}

/// A transfer rate. `short` gives the bar form (`2.1M`), otherwise `2.1 MB/s`.
pub fn rate(bytes_per_sec: f64, units: NetworkUnits, short: bool) -> String {
    match units {
        NetworkUnits::Bytes => {
            let (v, i) = scaled(bytes_per_sec, 1000.0);
            let n = trim_num(v, i);
            if short {
                format!("{n}{}", ["B", "K", "M", "G", "T"][i])
            } else {
                format!("{n} {}", ["B/s", "kB/s", "MB/s", "GB/s", "TB/s"][i])
            }
        }
        NetworkUnits::Bits => {
            let (v, i) = scaled(bytes_per_sec * 8.0, 1000.0);
            let n = trim_num(v, i);
            if short {
                format!("{n}{}", ["b", "k", "m", "g", "t"][i])
            } else {
                format!("{n} {}", ["b/s", "kb/s", "Mb/s", "Gb/s", "Tb/s"][i])
            }
        }
    }
}

/// Disk rates are always shown in bytes.
pub fn disk_rate(bytes_per_sec: f64, short: bool) -> String {
    rate(bytes_per_sec, NetworkUnits::Bytes, short)
}

/// `64°` in the bar, `64 °C` elsewhere.
pub fn temp(celsius: f64, units: TemperatureUnits, short: bool) -> String {
    let (v, u) = match units {
        TemperatureUnits::Celsius => (celsius, "°C"),
        TemperatureUnits::Fahrenheit => (celsius * 9.0 / 5.0 + 32.0, "°F"),
    };
    if short { format!("{}°", v.round() as i64) } else { format!("{} {u}", v.round() as i64) }
}

/// Watts: `182 W`, `9.5 W`.
pub fn watts(w: f64) -> String {
    if w < 10.0 { format!("{w:.1} W") } else { format!("{w:.0} W") }
}

/// Uptime: `3d 4h`, `5h 12m`, `9m`.
pub fn duration(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    let (d, h, m) = (s / 86400, (s % 86400) / 3600, (s % 3600) / 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m")
    }
}

/// Bytes with an automatic unit, for totals and capacities: `48.2 GB`.
pub fn bytes(b: u64) -> String {
    let (v, i) = scaled(b as f64, 1000.0);
    format!("{} {}", trim_num(v, i), ["B", "kB", "MB", "GB", "TB"][i])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(percent(23.4), "23%");
        assert_eq!(percent(140.0), "100%");
        assert_eq!(mem_short(19_756_849_152, MemoryUnits::Binary), "18.4G");
        assert_eq!(mem_short(200_000_000_000, MemoryUnits::Decimal), "200G");
        assert_eq!(mem_long(19_756_849_152, MemoryUnits::Binary), "18.4 GiB");
        assert_eq!(mem_long(412 * 1024 * 1024, MemoryUnits::Binary), "412 MiB");
        assert_eq!(mem_long(5_000_000, MemoryUnits::Decimal), "5 MB");
        assert_eq!(mem_long(2_000_000_000, MemoryUnits::Decimal), "2.0 GB");
        assert_eq!(rate(2_100_000.0, NetworkUnits::Bytes, true), "2.1M");
        assert_eq!(rate(180_000.0, NetworkUnits::Bytes, true), "180K");
        assert_eq!(rate(512.0, NetworkUnits::Bytes, false), "512 B/s");
        assert_eq!(rate(-1.0, NetworkUnits::Bytes, false), "0 B/s");
        assert_eq!(rate(125_000.0, NetworkUnits::Bits, false), "1.0 Mb/s");
        assert_eq!(rate(125_000.0, NetworkUnits::Bits, true), "1.0m");
        assert_eq!(disk_rate(60e6, false), "60 MB/s");
        assert_eq!(temp(64.4, TemperatureUnits::Celsius, true), "64°");
        assert_eq!(temp(100.0, TemperatureUnits::Fahrenheit, false), "212 °F");
        assert_eq!(watts(182.3), "182 W");
        assert_eq!(watts(9.54), "9.5 W");
        assert_eq!(duration(3.0 * 86400.0 + 4.0 * 3600.0), "3d 4h");
        assert_eq!(duration(5.0 * 3600.0 + 720.0), "5h 12m");
        assert_eq!(duration(540.0), "9m");
        assert_eq!(bytes(48_200_000_000), "48 GB");
        assert_eq!(bytes(1_100_000), "1.1 MB");
    }
}
