// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! Time each bar-tier source on this machine: `cargo run --release -p masthead-probe --example tick_cost`.

use masthead_probe::Roots;
use masthead_probe::cpu::{CpuStat, StatSnapshot};
use masthead_probe::disk::Disks;
use masthead_probe::gpu::Gpus;
use masthead_probe::hwmon;
use masthead_probe::mem::Mem;
use masthead_probe::net::{Iface, Net};
use masthead_probe::source::Buf;
use masthead_probe::system::System;
use std::time::Instant;

fn time(name: &str, n: u32, mut f: impl FnMut()) {
    for _ in 0..10 {
        f();
    }
    let t = Instant::now();
    for _ in 0..n {
        f();
    }
    println!("{name:<14} {:>8.1} µs", t.elapsed().as_secs_f64() * 1e6 / f64::from(n));
}

fn main() {
    let r = Roots::default();
    let n = 2000;
    let mut cpu = CpuStat::open(&r).unwrap();
    let mut st = StatSnapshot::default();
    time("/proc/stat", n, || cpu.sample(&mut st).unwrap());
    let mut mem = Mem::open(&r).unwrap();
    time("meminfo", n, || {
        mem.sample().unwrap();
    });
    let mut net = Net::open(&r).unwrap();
    time("net/dev", n, || {
        net.sample(1.0, Iface::physical).unwrap();
    });
    let mut d = Disks::open(&r).unwrap();
    time("diskstats", n, || {
        d.sample(1.0).unwrap();
    });
    let mut sys = System::open(&r).unwrap();
    time("loadavg", n, || {
        sys.loadavg().unwrap();
    });
    let mut sensors = hwmon::discover(&r);
    let i = hwmon::cpu_package(&sensors).unwrap_or(0);
    let mut b = Buf::default();
    time("cpu temp", n, || {
        sensors[i].read(&mut b);
    });
    let mut g = Gpus::open(&r);
    time("gpus sysfs", 200, || g.sample(false));
    time("gpus nvml", 200, || g.sample(true));
}
