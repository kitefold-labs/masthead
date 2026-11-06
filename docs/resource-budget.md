# Resource budget

| | At rest (bar only) | Popover open | Window open |
|---|---|---|---|
| CPU, one core | ≤ 0.1% | ≤ 0.4% | ≤ 3% |
| Memory | daemon ≤ 6 MB | + 0 | window ≤ 120 MB, freed on close |
| Wakeups | 1 per second, with 50 ms timer slack | 1 per second | 1 per second |
| Disk writes | 0 | 0 | only when settings change |

## Measured

Ryzen 9 9950X3D, RTX 5090 and Radeon iGPU, Ubuntu 26.04, kernel 7.0, release
build, `scripts/soak.sh` with one `masthead-bar` subscriber:

| Run | CPU | RSS | Disk writes | Threads |
|---|---|---|---|---|
| Resting, NVML off, 60 s | 36 ms (0.060%) | 4.9 MB | 0 | 1 |
| Resting, NVML on, 120 s | 70 ms (0.058%) | 25.4 MB | 0 | 2 |

Both runs overlapped a libcosmic build that kept the load average between 8
and 18, which inflates CPU time through cache misses and migrations. CPU time
comes from `/proc/<pid>/schedstat` in nanoseconds (the NVML row from the
older 10 ms-tick counter).

Per-source cost of one sample (`cargo run --release -p masthead-probe --example tick_cost`):

| Source | Time |
|---|---|
| `/proc/net/dev` (25 interfaces, 22 of them virtual) | 59 µs |
| `/proc/diskstats` | 19 µs |
| `/proc/stat` (32 threads) | 13 µs |
| `/proc/meminfo` | 2 µs |
| CPU temperature (`k10temp`) | 2 µs |
| GPUs via NVML | 6 µs |
| GPUs via sysfs | 2 µs |
| `/proc/loadavg` | 0.4 µs |

About 100 µs of reading per second. The rest of each tick is formatting, the
D-Bus signal, and the runtime.

Faces and the window, release builds under a headless GNOME Shell 50, 60 s
per state:

| Process | State | CPU, one core | RSS |
|---|---|---|---|
| `masthead-cosmic` | Idle | 0.12% | 25 MB |
| `masthead-cosmic` | Popover open | 0.30% | 26.7 MB |
| `masthead-window` | Settings page | 0.3% | 33 MB |
| `masthead-window` | CPU page (chart, per-core grid, top processes) | 1.5–1.9% | 33 MB |
| `masthead-window` | Processes page | 1.2% | 33 MB |

The COSMIC applet is over the resting CPU budget. Its cost is libcosmic's: an
empty libcosmic applet starts 38 threads and redraws in software. Trimming it
is tracked as an open issue. The window renders with tiny-skia; the `wgpu`
feature cuts its CPU to about 0.35% but maps the GPU driver into the process
(170–210 MB RSS on NVIDIA), so software rendering is the default.

## NVIDIA

NVIDIA's NVML library and the `libcuda` it loads add about 20 MB of resident
memory, four times everything else in the daemon. No `nvmlInitWithFlags`
option reduces it. So the daemon:

- loads NVML only while a face shows GPU data (the bar's `gpu` item, a GPU
  popover or the window);
- unloads it a minute after GPU data was last shown, which returned RSS from
  25.4 MB to 9.2 MB in testing (glibc keeps some freed heap);
- never loads it while every NVIDIA GPU is runtime-suspended;
- skips it entirely with `[devices] nvml = false`, at the cost of NVIDIA GPU
  load and VRAM readings.

## How it is enforced

- **`scripts/soak.sh`** runs the daemon on a private bus with a temporary
  config and reads its CPU ticks from `/proc/<pid>/stat`, `VmRSS` from
  `/proc/<pid>/status` and `write_bytes` from `/proc/<pid>/io`. Over budget
  exits non-zero. CI runs five minutes at rest and one minute with the window
  tier.
- **`crates/masthead-probe/tests/no_alloc.rs`** counts heap allocations during
  100 steady-state samples of every bar-tier source and fails on any.
- **The systemd unit** caps the daemon at `CPUQuota=5%` and `MemoryMax=128M`,
  well above the budget. A fault is throttled instead of hurting the session.

## Why the faces are cheap

The daemon sends `Panel` only when a formatted text, level or summary changes,
and the GNOME extension only calls `set_text` on a label whose text changed.
Values use tabular figures in a fixed-width label, so an update never
relayouts the bar. A panel monitor's main cost is usually the shell
repainting, not the sampling.
