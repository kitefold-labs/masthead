//! GPUs: amdgpu through sysfs, NVIDIA through NVML, Intel best effort.
//!
//! Before any query the device's `power/runtime_status` is checked. On hybrid
//! laptops a query can pull a runtime-suspended discrete GPU out of D3cold,
//! which costs watts; a suspended GPU is reported as such and left alone.

pub mod nvml;

use crate::hwmon::{self, Sensor, SensorKind};
use crate::source::{Buf, Source};
use crate::{Roots, link_name, read_attr};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vendor {
    Amd,
    Nvidia,
    Intel,
    Other,
}

impl Vendor {
    fn from_id(id: u32) -> Self {
        match id {
            0x1002 => Self::Amd,
            0x10de => Self::Nvidia,
            0x8086 => Self::Intel,
            _ => Self::Other,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Amd => "amd",
            Self::Nvidia => "nvidia",
            Self::Intel => "intel",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GpuSample {
    /// Busy percentage, 0–100.
    pub util: Option<f64>,
    pub vram_used: Option<u64>,
    pub vram_total: Option<u64>,
    pub temp_c: Option<f64>,
    pub power_w: Option<f64>,
    pub suspended: bool,
}

#[derive(Debug)]
enum Backend {
    Amd { busy: Option<Source>, vram_used: Option<Source>, vram_total: Option<u64> },
    Nvidia { nvml_index: Option<usize> },
    None,
}

#[derive(Debug)]
pub struct Gpu {
    pub card: String,
    /// PCI address, e.g. `0000:01:00.0`.
    pub pci: String,
    pci_parts: Option<(u32, u32, u32)>,
    pub vendor: Vendor,
    pub driver: Option<String>,
    pub name: String,
    pub discrete: bool,
    pub power_limit_w: Option<f64>,
    backend: Backend,
    runtime_status: Option<Source>,
    sensors: Vec<Sensor>,
    pub last: GpuSample,
}

/// Parse `0000:01:00.0` into (domain, bus, device).
fn parse_pci(addr: &str) -> Option<(u32, u32, u32)> {
    let mut it = addr.split([':', '.']);
    let d = u32::from_str_radix(it.next()?, 16).ok()?;
    let b = u32::from_str_radix(it.next()?, 16).ok()?;
    let v = u32::from_str_radix(it.next()?, 16).ok()?;
    Some((d, b, v))
}

fn hex_attr(p: &Path) -> Option<u32> {
    let s = read_attr(p)?;
    u32::from_str_radix(s.trim_start_matches("0x"), 16).ok()
}

/// Look a device up in the system's `pci.ids`, preferring the bracketed
/// marketing name: `Granite Ridge [Radeon Graphics]` gives `Radeon Graphics`.
pub fn pci_name(ids_paths: &[PathBuf], vendor: u32, device: u32) -> Option<String> {
    use std::io::BufRead;
    let file = ids_paths.iter().find_map(|p| std::fs::File::open(p).ok())?;
    let (vtag, dtag) = (format!("{vendor:04x}  "), format!("\t{device:04x}  "));
    let mut in_vendor = false;
    for line in std::io::BufReader::new(file).lines() {
        let line = line.ok()?;
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if !line.starts_with('\t') {
            if in_vendor {
                return None;
            }
            in_vendor = line.starts_with(&vtag);
        } else if in_vendor && line.starts_with(&dtag) {
            let full = line[dtag.len()..].trim();
            let name = match (full.find('['), full.rfind(']')) {
                (Some(a), Some(b)) if b > a => &full[a + 1..b],
                _ => full,
            };
            return Some(name.to_owned());
        }
    }
    None
}

fn default_ids() -> Vec<PathBuf> {
    vec![PathBuf::from("/usr/share/hwdata/pci.ids"), PathBuf::from("/usr/share/misc/pci.ids"), PathBuf::from("/usr/share/pci.ids")]
}

/// GPUs under `/sys/class/drm/card*`.
pub fn discover(roots: &Roots) -> Vec<Gpu> {
    discover_with_ids(roots, &default_ids())
}

pub fn discover_with_ids(roots: &Roots, ids: &[PathBuf]) -> Vec<Gpu> {
    let dir = roots.sys.join("class/drm");
    let Ok(rd) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut cards: Vec<String> = rd
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.strip_prefix("card").is_some_and(|r| !r.is_empty() && r.bytes().all(|b| b.is_ascii_digit())))
        .collect();
    cards.sort_by_key(|c| c[4..].parse::<u32>().unwrap_or(0));
    let mut out = Vec::new();
    for card in cards {
        let dev = dir.join(&card).join("device");
        let Some(vid) = hex_attr(&dev.join("vendor")) else { continue };
        let did = hex_attr(&dev.join("device")).unwrap_or(0);
        let vendor = Vendor::from_id(vid);
        let pci =
            std::fs::canonicalize(&dev).ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_default();
        let driver = link_name(&dev.join("driver"));
        let prefix = match vendor {
            Vendor::Amd => "AMD ",
            Vendor::Nvidia => "NVIDIA ",
            Vendor::Intel => "Intel ",
            Vendor::Other => "",
        };
        let name = read_attr(&dev.join("product_name"))
            .or_else(|| pci_name(ids, vid, did).map(|n| format!("{prefix}{n}")))
            .unwrap_or_else(|| format!("{prefix}GPU {pci}"));
        let mut sensors = Vec::new();
        if let Ok(rd) = std::fs::read_dir(dev.join("hwmon")) {
            for e in rd.filter_map(Result::ok) {
                sensors
                    .extend(hwmon::discover_chip(&e.path()).into_iter().filter(|s| matches!(s.kind, SensorKind::Temp | SensorKind::Power)));
            }
        }
        let (backend, discrete) = match (vendor, driver.as_deref()) {
            (Vendor::Amd, Some("amdgpu")) => {
                let vram_total = read_attr(&dev.join("mem_info_vram_total")).and_then(|s| s.parse::<u64>().ok());
                // APUs report a small carve-out as VRAM; dGPUs carry 4 GiB or more.
                let discrete = vram_total.is_some_and(|v| v >= 4 << 30);
                (
                    Backend::Amd {
                        busy: Source::open(&dev.join("gpu_busy_percent")).ok(),
                        vram_used: Source::open(&dev.join("mem_info_vram_used")).ok(),
                        vram_total,
                    },
                    discrete,
                )
            }
            (Vendor::Nvidia, Some("nvidia")) => (Backend::Nvidia { nvml_index: None }, true),
            (Vendor::Nvidia, _) => (Backend::None, true),
            (Vendor::Intel, _) => (Backend::None, dev.join("tile0").exists()),
            _ => (Backend::None, false),
        };
        out.push(Gpu {
            card: card.clone(),
            pci_parts: parse_pci(&pci),
            pci,
            vendor,
            driver,
            name,
            discrete,
            power_limit_w: None,
            backend,
            runtime_status: Source::open(&dev.join("power/runtime_status")).ok(),
            sensors,
            last: GpuSample::default(),
        });
    }
    // The NVIDIA driver can register more than one DRM node per device.
    out.dedup_by(|a, b| a.pci == b.pci && !a.pci.is_empty());
    out
}

#[derive(Debug)]
enum NvmlState {
    Unloaded,
    Ready(nvml::Nvml),
    Failed,
}

/// All GPUs and the shared NVML handle.
#[derive(Debug)]
pub struct Gpus {
    pub list: Vec<Gpu>,
    nvml: NvmlState,
    buf: Buf,
}

impl Gpus {
    pub fn open(roots: &Roots) -> Self {
        Self { list: discover(roots), nvml: NvmlState::Unloaded, buf: Buf::with_capacity(64) }
    }

    pub fn from_list(list: Vec<Gpu>) -> Self {
        Self { list, nvml: NvmlState::Unloaded, buf: Buf::with_capacity(64) }
    }

    pub fn nvml_loaded(&self) -> bool {
        matches!(self.nvml, NvmlState::Ready(_))
    }

    fn ensure_nvml(&mut self) {
        if !matches!(self.nvml, NvmlState::Unloaded) {
            return;
        }
        match nvml::Nvml::load() {
            Ok(n) => {
                for g in &mut self.list {
                    if let (Backend::Nvidia { nvml_index }, Some((d, b, v))) = (&mut g.backend, g.pci_parts) {
                        *nvml_index = n.find(d, b, v);
                        if let Some(i) = *nvml_index {
                            g.name.clone_from(&n.devices[i].name);
                            g.power_limit_w = n.devices[i].power_limit_w;
                        }
                    }
                }
                self.nvml = NvmlState::Ready(n);
            }
            Err(e) => {
                log::info!("NVML unavailable: {e}");
                self.nvml = NvmlState::Failed;
            }
        }
    }

    /// Unload NVML, returning its memory. The next NVIDIA sample loads it again.
    pub fn release_nvml(&mut self) {
        if matches!(self.nvml, NvmlState::Ready(_)) {
            log::debug!("unloading NVML");
            self.nvml = NvmlState::Unloaded;
            for g in &mut self.list {
                if let Backend::Nvidia { nvml_index } = &mut g.backend {
                    *nvml_index = None;
                }
            }
        }
    }

    /// Sample every GPU that is awake. With `use_nvml` false, NVIDIA devices
    /// report only what sysfs has and NVML is never loaded.
    pub fn sample(&mut self, use_nvml: bool) {
        if !use_nvml {
            self.release_nvml();
        }
        let mut want_nvml = false;
        for g in &mut self.list {
            g.last.suspended = g.runtime_status.as_ref().and_then(|s| s.read_word(&mut self.buf)).is_some_and(|w| w == b"suspended");
            if !g.last.suspended && matches!(g.backend, Backend::Nvidia { .. }) {
                want_nvml = true;
            }
        }
        // NVML is loaded only once an NVIDIA device is awake: nvmlInit itself
        // would wake a suspended one.
        if want_nvml && use_nvml {
            self.ensure_nvml();
        }
        for g in &mut self.list {
            if g.last.suspended {
                g.last = GpuSample { suspended: true, ..GpuSample::default() };
                continue;
            }
            let mut s = GpuSample::default();
            match &g.backend {
                Backend::Amd { busy, vram_used, vram_total } => {
                    s.util = busy.as_ref().and_then(|b| b.read_i64(&mut self.buf)).map(|v| v as f64);
                    s.vram_used = vram_used.as_ref().and_then(|b| b.read_i64(&mut self.buf)).and_then(|v| u64::try_from(v).ok());
                    s.vram_total = *vram_total;
                }
                Backend::Nvidia { nvml_index: Some(i) } => {
                    if let NvmlState::Ready(n) = &self.nvml {
                        let ns = n.sample(*i);
                        s.util = ns.util;
                        s.vram_used = ns.vram_used;
                        s.vram_total = ns.vram_total;
                        s.temp_c = ns.temp_c;
                        s.power_w = ns.power_w;
                    }
                }
                Backend::Nvidia { nvml_index: None } | Backend::None => {}
            }
            for sen in &mut g.sensors {
                match sen.kind {
                    SensorKind::Temp if s.temp_c.is_none() => s.temp_c = sen.read(&mut self.buf),
                    SensorKind::Power if s.power_w.is_none() => s.power_w = sen.read(&mut self.buf),
                    _ => {}
                }
            }
            g.last = s;
        }
    }

    /// The GPU the bar shows: the configured PCI address, else the busiest
    /// awake discrete GPU, else the busiest of any.
    pub fn primary(&self, preferred_pci: Option<&str>) -> Option<&Gpu> {
        if let Some(p) = preferred_pci
            && let Some(g) = self.list.iter().find(|g| g.pci == p)
        {
            return Some(g);
        }
        let busiest = |discrete: bool| {
            self.list
                .iter()
                .filter(|g| g.discrete == discrete && g.last.util.is_some())
                .max_by(|a, b| a.last.util.unwrap_or(0.0).total_cmp(&b.last.util.unwrap_or(0.0)))
        };
        busiest(true).or_else(|| busiest(false)).or_else(|| self.list.iter().find(|g| g.discrete)).or_else(|| self.list.first())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn card(base: &Path, n: u32, pci: &str, vendor: &str, device: &str, driver: &str) -> PathBuf {
        let devdir = base.join("sys/devices/pci0000:00").join(pci);
        std::fs::create_dir_all(&devdir).unwrap();
        std::fs::write(devdir.join("vendor"), format!("{vendor}\n")).unwrap();
        std::fs::write(devdir.join("device"), format!("{device}\n")).unwrap();
        let drv = base.join("sys/bus/pci/drivers").join(driver);
        std::fs::create_dir_all(&drv).unwrap();
        symlink(&drv, devdir.join("driver")).unwrap();
        let c = base.join("sys/class/drm").join(format!("card{n}"));
        std::fs::create_dir_all(&c).unwrap();
        symlink(&devdir, c.join("device")).unwrap();
        devdir
    }

    #[test]
    fn amd_and_nvidia() {
        let d = tempfile::tempdir().unwrap();
        let ids = d.path().join("pci.ids");
        std::fs::write(&ids, "# c\n1002  Advanced Micro Devices, Inc. [AMD/ATI]\n\t13c0  Granite Ridge [Radeon Graphics]\n10de  NVIDIA Corporation\n\t2b85  GB202 [GeForce RTX 5090]\n").unwrap();
        let amd = card(d.path(), 1, "0000:13:00.0", "0x1002", "0x13c0", "amdgpu");
        std::fs::write(amd.join("gpu_busy_percent"), "7\n").unwrap();
        std::fs::write(amd.join("mem_info_vram_used"), "419430400\n").unwrap();
        std::fs::write(amd.join("mem_info_vram_total"), "2147483648\n").unwrap();
        std::fs::create_dir_all(amd.join("power")).unwrap();
        std::fs::write(amd.join("power/runtime_status"), "active\n").unwrap();
        let hw = amd.join("hwmon/hwmon8");
        std::fs::create_dir_all(&hw).unwrap();
        std::fs::write(hw.join("name"), "amdgpu\n").unwrap();
        std::fs::write(hw.join("temp1_input"), "45000\n").unwrap();
        std::fs::write(hw.join("temp1_label"), "edge\n").unwrap();
        std::fs::write(hw.join("power1_input"), "12000000\n").unwrap();
        let nv = card(d.path(), 0, "0000:01:00.0", "0x10de", "0x2b85", "nouveau");
        std::fs::create_dir_all(nv.join("power")).unwrap();
        std::fs::write(nv.join("power/runtime_status"), "suspended\n").unwrap();
        std::fs::create_dir_all(d.path().join("sys/class/drm/card0-DP-1")).unwrap();

        let list = discover_with_ids(&Roots::under(d.path()), &[ids]);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].card, "card0");
        assert_eq!(list[0].name, "NVIDIA GeForce RTX 5090");
        assert_eq!(list[1].name, "AMD Radeon Graphics");
        assert_eq!(list[1].pci, "0000:13:00.0");
        assert!(!list[1].discrete);
        let mut g = Gpus::from_list(list);
        g.sample(true);
        assert!(g.list[0].last.suspended);
        assert!(!g.nvml_loaded());
        let a = &g.list[1].last;
        assert_eq!(a.util, Some(7.0));
        assert_eq!(a.vram_used, Some(419_430_400));
        assert_eq!(a.temp_c, Some(45.0));
        assert_eq!(a.power_w, Some(12.0));
        assert_eq!(g.primary(None).unwrap().card, "card1");
        assert_eq!(g.primary(Some("0000:01:00.0")).unwrap().card, "card0");
        g.release_nvml();
        g.sample(false);
        assert_eq!(g.list[1].last.util, Some(7.0));
    }

    #[test]
    fn pci_parsing() {
        assert_eq!(parse_pci("0000:01:00.0"), Some((0, 1, 0)));
        assert_eq!(parse_pci("0001:a3:1f.2"), Some((1, 0xa3, 0x1f)));
        assert_eq!(parse_pci("junk"), None);
        assert_eq!(Vendor::from_id(0x8086).as_str(), "intel");
        assert_eq!(pci_name(&[PathBuf::from("/nonexistent")], 1, 1), None);
    }
}
