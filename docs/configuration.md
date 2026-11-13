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
style = "values"          # values | trend | compact (trends: COSMIC applet only)

[units]
memory = "binary"         # binary (GiB) | decimal (GB)
network = "bytes"         # bytes | bits
temperature = "celsius"   # celsius | fahrenheit

[thresholds.cpu]          # amber at warn, red at crit
warn = 85
crit = 95
enabled = true            # false: no colors or notifications; keeps the numbers

[thresholds.disk]         # % of a filesystem's space used; colors the
warn = 90                 # window's space meters, not the Disk bar item
crit = 95

[devices]
gpu = "auto"              # or a PCI address: "0000:01:00.0"
temperature = "auto"      # or "chip/label": "k10temp/Tctl"
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
