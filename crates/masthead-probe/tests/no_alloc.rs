// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! A steady-state sample of the bar-tier sources must not touch the heap.
//!
//! Runs against the real `/proc` and `/sys` of the machine running the tests.

use masthead_probe::Roots;
use masthead_probe::cpu::{CpuStat, StatSnapshot};
use masthead_probe::disk::Disks;
use masthead_probe::gpu::Gpus;
use masthead_probe::hwmon;
use masthead_probe::mem::Mem;
use masthead_probe::net::{Iface, Net};
use masthead_probe::source::Buf;
use masthead_probe::system::System;
use std::alloc::{GlobalAlloc, Layout, System as SysAlloc};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct Counting;

static COUNTING: AtomicBool = AtomicBool::new(false);
static ALLOCS: AtomicUsize = AtomicUsize::new(0);

#[allow(unsafe_code)]
// SAFETY: forwards to the system allocator unchanged; only counts calls.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: same contract as the caller's.
        unsafe { SysAlloc.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        // SAFETY: same contract as the caller's.
        unsafe { SysAlloc.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: same contract as the caller's.
        unsafe { SysAlloc.realloc(p, l, n) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

#[test]
fn steady_state_tick_does_not_allocate() {
    let roots = Roots::default();
    let mut cpu = CpuStat::open(&roots).unwrap();
    let mut stat = StatSnapshot::default();
    let mut mem = Mem::open(&roots).unwrap();
    let mut net = Net::open(&roots).unwrap();
    let mut disks = Disks::open(&roots).unwrap();
    let mut sys = System::open(&roots).unwrap();
    let mut sensors = hwmon::discover(&roots);
    let pkg = hwmon::cpu_package(&sensors);
    let mut gpus = Gpus::open(&roots);
    let mut buf = Buf::default();

    let mut tick = |count: bool| {
        COUNTING.store(count, Ordering::Relaxed);
        cpu.sample(&mut stat).unwrap();
        let _ = mem.sample().unwrap();
        let _ = net.sample(1.0, Iface::physical).unwrap();
        let _ = disks.sample(1.0).unwrap();
        let _ = sys.loadavg().unwrap();
        if let Some(i) = pkg {
            let _ = sensors[i].read(&mut buf);
        }
        // NVML is a C library; its own mallocs are not Rust allocations.
        gpus.sample(false);
        COUNTING.store(false, Ordering::Relaxed);
    };
    // Warm-up: buffers grow to the size of each file, sensors open their fds.
    for _ in 0..3 {
        tick(false);
    }
    ALLOCS.store(0, Ordering::Relaxed);
    for _ in 0..100 {
        tick(true);
    }
    assert_eq!(ALLOCS.load(Ordering::Relaxed), 0, "steady-state ticks allocated");
}
