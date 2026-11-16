<!--
  Copyright 2026 Kitefold Labs
  SPDX-License-Identifier: Apache-2.0
-->

# Configuration

`~/.config/masthead/config.toml`, edited from the window's Settings page or
by hand. The daemon watches the file and applies changes immediately. Every
key is optional; these are the defaults:

```toml
[bar]
metrics = ["cpu", "mem", "gpu", "net", "temp"]   # also: disk, battery
style = "values"          # values | trend | compact
disk = "io"               # io: read and write rates | space: % used of devices.filesystem

[units]
memory = "binary"         # binary (GiB) | decimal (GB)
network = "bytes"         # bytes | bits
temperature = "celsius"   # celsius | fahrenheit

[thresholds.cpu]          # amber at warn, red at crit
warn = 85
crit = 95

[thresholds.disk]         # % of a filesystem's space used; colors the
warn = 90                 # window's space meters, and the Disk bar item
crit = 95                 # when bar.disk = "space"

[devices]
gpu = "auto"              # or a PCI address: "0000:01:00.0"
temperature = "auto"      # or "chip/label": "k10temp/Tctl"
filesystem = "/"          # mount point for bar.disk = "space": "/home"
interfaces = []           # empty: every physical interface
include_virtual_interfaces = false
nvml = true               # false saves ~20 MB, loses NVIDIA load and VRAM

[tray]
mode = "auto"             # auto | always | never

[notifications]
enabled = false           # notify when a metric stays critical
hold_seconds = 60

[window]
theme = "system"          # system | dark | light: the detail window's colors
```

Physical network interfaces only by default: `docker0`, `veth*` and `br-*`
would double-count container traffic.

`devices.filesystem` must be the mount point of a local filesystem (ext4,
btrfs, xfs and the like), as `/proc/self/mounts` lists it. Any other path,
including a folder inside a filesystem, an unmounted path or a network
mount, shows `–` instead of the space of whatever holds it. The daemon checks
the mounts table when the setting changes, when a disk is added or removed,
and every 30 seconds.
