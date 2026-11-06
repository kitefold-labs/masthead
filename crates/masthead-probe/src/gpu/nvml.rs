// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! A minimal NVML binding, loaded with `dlopen` only when an NVIDIA device is
//! bound to the `nvidia` driver, awake, and its data is being shown.
//!
//! NVML and the `libcuda` it pulls in cost about 20 MB of resident memory,
//! four times the rest of the daemon. Callers unload it when no face shows
//! GPU data.
//!
//! Spawning `nvidia-smi` per sample, as several panel monitors do, costs a
//! process creation and driver initialisation every second. NVML is the
//! library `nvidia-smi` itself uses.
#![allow(unsafe_code)]

use libloading::Library;
use std::ffi::{CStr, c_char, c_int, c_uint, c_void};

type Ret = c_int;
type Device = *mut c_void;
const SUCCESS: Ret = 0;
const TEMPERATURE_GPU: c_uint = 0;

#[repr(C)]
#[derive(Default)]
struct Utilization {
    gpu: c_uint,
    memory: c_uint,
}

#[repr(C)]
#[derive(Default)]
struct Memory {
    total: u64,
    free: u64,
    used: u64,
}

#[repr(C)]
struct PciInfo {
    bus_id_legacy: [c_char; 16],
    domain: c_uint,
    bus: c_uint,
    device: c_uint,
    pci_device_id: c_uint,
    pci_sub_system_id: c_uint,
    bus_id: [c_char; 32],
}

type FnVoid = unsafe extern "C" fn() -> Ret;
type FnCount = unsafe extern "C" fn(*mut c_uint) -> Ret;
type FnHandle = unsafe extern "C" fn(c_uint, *mut Device) -> Ret;
type FnName = unsafe extern "C" fn(Device, *mut c_char, c_uint) -> Ret;
type FnPci = unsafe extern "C" fn(Device, *mut PciInfo) -> Ret;
type FnUtil = unsafe extern "C" fn(Device, *mut Utilization) -> Ret;
type FnMem = unsafe extern "C" fn(Device, *mut Memory) -> Ret;
type FnTemp = unsafe extern "C" fn(Device, c_uint, *mut c_uint) -> Ret;
type FnU32 = unsafe extern "C" fn(Device, *mut c_uint) -> Ret;

/// One NVML device and its PCI address.
#[derive(Debug, Clone)]
pub struct NvmlDevice {
    handle: Device,
    pub domain: u32,
    pub bus: u32,
    pub device: u32,
    pub name: String,
    pub power_limit_w: Option<f64>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NvmlSample {
    pub util: Option<f64>,
    pub vram_used: Option<u64>,
    pub vram_total: Option<u64>,
    pub temp_c: Option<f64>,
    pub power_w: Option<f64>,
}

pub struct Nvml {
    shutdown: FnVoid,
    util: FnUtil,
    mem: FnMem,
    temp: FnTemp,
    power: FnU32,
    pub devices: Vec<NvmlDevice>,
    // Dropped last, after `shutdown` has run in `Drop`.
    _lib: Library,
}

// SAFETY: NVML documents its API as thread-safe, and device handles stay valid
// until nvmlShutdown, which only `Drop` calls.
unsafe impl Send for Nvml {}
// SAFETY: as above; NvmlDevice only carries a handle that is used through Nvml.
unsafe impl Send for NvmlDevice {}

impl std::fmt::Debug for Nvml {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Nvml").field("devices", &self.devices).finish_non_exhaustive()
    }
}

macro_rules! sym {
    ($lib:expr, $name:literal, $ty:ty) => {{
        // SAFETY: the symbol has this signature in every NVML release since the
        // `_v2`/`_v3` variants named here were introduced (driver 325+).
        let s: libloading::Symbol<'_, $ty> = unsafe { $lib.get($name) }.map_err(|e| e.to_string())?;
        *s
    }};
}

impl Nvml {
    /// Load `libnvidia-ml.so.1` and enumerate devices.
    pub fn load() -> Result<Self, String> {
        // SAFETY: loading the NVIDIA management library runs its initialisers,
        // which have no preconditions on the caller.
        let lib = unsafe { Library::new("libnvidia-ml.so.1") }.map_err(|e| e.to_string())?;
        let init = sym!(lib, b"nvmlInit_v2\0", FnVoid);
        let shutdown = sym!(lib, b"nvmlShutdown\0", FnVoid);
        let count = sym!(lib, b"nvmlDeviceGetCount_v2\0", FnCount);
        let handle = sym!(lib, b"nvmlDeviceGetHandleByIndex_v2\0", FnHandle);
        let name = sym!(lib, b"nvmlDeviceGetName\0", FnName);
        let pci = sym!(lib, b"nvmlDeviceGetPciInfo_v3\0", FnPci);
        let util = sym!(lib, b"nvmlDeviceGetUtilizationRates\0", FnUtil);
        let mem = sym!(lib, b"nvmlDeviceGetMemoryInfo\0", FnMem);
        let temp = sym!(lib, b"nvmlDeviceGetTemperature\0", FnTemp);
        let power = sym!(lib, b"nvmlDeviceGetPowerUsage\0", FnU32);
        let limit = sym!(lib, b"nvmlDeviceGetEnforcedPowerLimit\0", FnU32);

        // SAFETY: no arguments.
        let r = unsafe { init() };
        if r != SUCCESS {
            return Err(format!("nvmlInit_v2 returned {r}"));
        }
        let mut n: c_uint = 0;
        // SAFETY: `n` is a valid out-pointer.
        if unsafe { count(&raw mut n) } != SUCCESS {
            n = 0;
        }
        let mut devices = Vec::new();
        for i in 0..n {
            let mut h: Device = std::ptr::null_mut();
            // SAFETY: `h` is a valid out-pointer; `i` is below the device count.
            if unsafe { handle(i, &raw mut h) } != SUCCESS {
                continue;
            }
            let mut buf = [0 as c_char; 96];
            // SAFETY: the buffer is 96 bytes, NVML_DEVICE_NAME_V2_BUFFER_SIZE.
            let nm = if unsafe { name(h, buf.as_mut_ptr(), 96) } == SUCCESS {
                // SAFETY: NVML NUL-terminates the name on success.
                unsafe { CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().into_owned()
            } else {
                "NVIDIA GPU".to_owned()
            };
            let mut p =
                PciInfo { bus_id_legacy: [0; 16], domain: 0, bus: 0, device: 0, pci_device_id: 0, pci_sub_system_id: 0, bus_id: [0; 32] };
            // SAFETY: `p` matches nvmlPciInfo_t as defined for the _v3 entry point.
            if unsafe { pci(h, &raw mut p) } != SUCCESS {
                continue;
            }
            let mut mw: c_uint = 0;
            // SAFETY: valid handle and out-pointer.
            let power_limit_w = (unsafe { limit(h, &raw mut mw) } == SUCCESS).then(|| f64::from(mw) / 1000.0);
            devices.push(NvmlDevice { handle: h, domain: p.domain, bus: p.bus, device: p.device, name: nm, power_limit_w });
        }
        Ok(Self { shutdown, util, mem, temp, power, devices, _lib: lib })
    }

    /// Index of the device at a PCI address, if NVML knows it.
    pub fn find(&self, domain: u32, bus: u32, device: u32) -> Option<usize> {
        self.devices.iter().position(|d| d.domain == domain && d.bus == bus && d.device == device)
    }

    pub fn sample(&self, idx: usize) -> NvmlSample {
        let Some(d) = self.devices.get(idx) else { return NvmlSample::default() };
        let h = d.handle;
        let mut s = NvmlSample::default();
        let mut u = Utilization::default();
        // SAFETY (all calls below): `h` came from nvmlDeviceGetHandleByIndex_v2
        // and NVML has not been shut down; every pointer is a live local.
        if unsafe { (self.util)(h, &raw mut u) } == SUCCESS {
            s.util = Some(f64::from(u.gpu));
        }
        let mut m = Memory::default();
        if unsafe { (self.mem)(h, &raw mut m) } == SUCCESS {
            s.vram_used = Some(m.used);
            s.vram_total = Some(m.total);
        }
        let mut t: c_uint = 0;
        if unsafe { (self.temp)(h, TEMPERATURE_GPU, &raw mut t) } == SUCCESS {
            s.temp_c = Some(f64::from(t));
        }
        let mut mw: c_uint = 0;
        if unsafe { (self.power)(h, &raw mut mw) } == SUCCESS {
            s.power_w = Some(f64::from(mw) / 1000.0);
        }
        let _ = (u.memory, m.free);
        s
    }
}

impl Drop for Nvml {
    fn drop(&mut self) {
        // SAFETY: called once, before the library is unloaded.
        unsafe {
            (self.shutdown)();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs only where the NVIDIA driver is installed; elsewhere loading fails
    /// cleanly, which is itself the behaviour under test.
    #[test]
    fn loads_when_present() {
        match Nvml::load() {
            Ok(n) => {
                for (i, d) in n.devices.iter().enumerate() {
                    assert_ne!(d.name, "");
                    assert_eq!(n.find(d.domain, d.bus, d.device), Some(i));
                    let s = n.sample(i);
                    assert!(s.util.is_some_and(|u| (0.0..=100.0).contains(&u)));
                    assert!(s.vram_total.is_some_and(|t| t > 0));
                }
                assert_eq!(n.sample(usize::MAX).util, None);
                assert!(format!("{n:?}").contains("Nvml"));
            }
            Err(e) => assert_ne!(e, ""),
        }
    }
}
