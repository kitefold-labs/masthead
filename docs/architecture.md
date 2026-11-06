<!--
  Copyright 2026 Kitefold Labs
  SPDX-License-Identifier: Apache-2.0
-->

# Architecture

One daemon reads the kernel. Every face (the GNOME extension, the COSMIC
applet, `masthead-bar`, the tray items and the window) subscribes to it over
the session bus and draws what it is sent. Adding a desktop never adds
sampling cost.

```
/proc, /sys, NVML ──► masthead-daemon ──► session D-Bus ──► GNOME extension
                       sampler                                COSMIC applet
                       tiers                                  masthead-bar
                       history                                SNI tray items (built in)
                                                              masthead-window
```

## Crates

| Crate | Kind | Role |
|---|---|---|
| `masthead-probe` | lib | Readers for `/proc` and `/sys`, the NVML shim, netlink uevents. No allocation in steady state. |
| `masthead-core` | lib | Metric ids, formatting, history rings, subscription tiers, thresholds, config. No I/O except the config file. |
| `masthead-api` | lib | The D-Bus contract: names, wire types, detail JSON, client proxies. |
| `masthead-daemon` | bin | Sampling loop, D-Bus service, SNI tray fallback, notifications. |
| `masthead-bar` | bin | JSON lines for Waybar, Ironbar, eww; the i3bar protocol. |
| `masthead-window` | bin | The detail window (iced via libcosmic). |
| `masthead-cosmic` | bin | The COSMIC panel applet (libcosmic). |
| `shell/gnome` | GJS | The GNOME Shell extension. |

## Tiers

A face asks for what it is about to show, and gives it back when it stops:

| Tier | Asked for by | Adds |
|---|---|---|
| `panel` | every bar face, tray items | CPU, memory, network, disk, GPU and one temperature sensor at 1 Hz |
| `hover:<metric>` | an open popover | per-thread load and frequency, top 5 processes, every sensor for `temp` |
| `window` | the open window | the full process table, every sensor, filesystem space |

Each subscription belongs to the D-Bus connection that made it. When that
connection closes, crashed or not, its subscriptions end. With no
subscriptions, or while logind reports the session locked, the daemon
samples nothing and records the gap in history as NaN.

The process table is the expensive part: reading `/proc/[pid]/stat` for 600+
processes takes milliseconds. Only the hover and window tiers pay for it, and
above 1,000 processes it is read every other second.

## The daemon

- One thread, a tokio current-thread runtime, a 1 Hz interval with 50 ms of
  timer slack so the kernel can batch the wakeup with others.
- Every source file stays open and is re-read with `pread(fd, buf, 0)`.
  Parsers write into preallocated structures; `tests/no_alloc.rs` fails if a
  steady-state tick allocates.
- Bar items go out in a `Panel` signal only when a formatted text, level or
  summary changed. The GNOME extension then updates only the labels whose text
  changed: the shell repainting is the real cost of a panel monitor.
- History: 15 minutes at 1 s and 24 hours at 1 min, the latter keeping each
  minute's minimum and maximum so peaks survive. About 140 KB for all nine series, held in
  memory only.
- Hotplug through a netlink `KOBJECT_UEVENT` socket: interfaces, disks, sensors,
  batteries and GPUs appear and disappear without polling sysfs or linking
  libudev.
- GPUs: amdgpu through sysfs; NVIDIA through NVML loaded with `dlopen`, only
  while GPU data is shown, and never while the device is runtime-suspended.
  Waking a sleeping laptop dGPU to read it would cost watts.
- `config.toml` is watched with inotify. `SetConfig` writes it atomically.

### Why not in the shell

Popular GNOME monitors sample inside the gnome-shell process. Their issue
trackers show where that leads: shell crashes on GNOME 50, an array that grew
to 200 MB, CPU spikes that build over days after lock and unlock, and an
`nvidia-smi` process spawned every second. A bug in Masthead's daemon restarts
the daemon (`Restart=on-failure`); it cannot take the session with it.

## Faces

Every face does the same three things: `Subscribe("panel")` and paint from
`Snapshot()`; update on `Panel`; on hover `Subscribe("hover:<metric>")`,
fetch `History`, render `Detail` signals, and `Unsubscribe` on close. A click
calls `ToggleWindow(metric)`.

The tray fallback is part of the daemon. With `[tray] mode = "auto"` it shows
items only when a StatusNotifierWatcher exists and no face holds a `panel`
subscription, after a four-second grace period at startup so the GNOME
extension can connect first. The items live on their own bus connection;
hiding them closes it, which every SNI host understands. Plasma ignores tray
labels, so the daemon draws each value into the icon pixmap with a built-in
4×7 digit font.

## The window

`masthead-window` is a separate process, started by D-Bus activation on the
first `ToggleWindow` and exiting when closed so its memory goes back to the
system. It owns `com.kitefoldlabs.Masthead.Window`; the daemon calls its
`Toggle(metric)`. It loads history with `History` and then follows the
`Detail("all")` signal. It is read-only: it never signals or renices
processes, so it needs no privileges.
