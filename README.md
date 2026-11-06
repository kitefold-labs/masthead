# Masthead

*by Kitefold Labs*

A Linux system monitor that lives in the top bar. Numbers at a glance, detail
on hover, graphs on click, and a resting cost under 0.1% of one CPU core with
zero disk writes.

> The masthead is the top of the mast: where the halyard runs to, where the
> wind indicator rides, and where the lookout stood.

- **License:** [Apache License 2.0](LICENSE)
- **App ID:** `com.kitefoldlabs.Masthead`
- **Status:** in development, version 0.1

## What it shows

| Metric | Bar | Hover | Window |
|---|---|---|---|
| CPU | busy % | per-thread load, frequency, load average, package temperature, top processes | history, per-thread heat grid, process table |
| Memory | used | used, cache, available, swap, top processes by memory | history, breakdown |
| GPU | busiest discrete GPU % | every GPU: load, VRAM, power, temperature | history, per-device cards |
| Network | ↓ ↑ rates | interfaces, link speed | history of both directions, totals |
| Disk | read/write (off by default) | devices | history, free space per filesystem |
| Temperature | CPU package | every sensor | temperatures, fans, voltages, power |
| Battery | charge (laptops) | state, watts | history |

Physical network interfaces only by default: `docker0`, `veth*` and `br-*`
would double-count container traffic.

## Where it runs

| Desktop | How Masthead appears |
|---|---|
| GNOME Shell 45–50 | Native extension: bar items, hover popovers, click for the window |
| COSMIC | Native panel applet |
| Hyprland, Sway, niri, river | `masthead-bar` for Waybar, Ironbar or eww |
| i3, swaybar | `masthead-bar --format i3bar` |
| KDE Plasma, XFCE, Cinnamon, MATE, Budgie | Tray icons with the value drawn in, tooltip on hover |

On Wayland an application cannot place a popup next to a panel icon; only the
shell can. That is why each desktop gets a small native face, all fed by one
daemon. See [docs/architecture.md](docs/architecture.md).

## Install

From source, for the current user (needs Rust 1.95 and the libcosmic build
dependencies listed in [docs/development.md](docs/development.md)):

```sh
just install          # or: cargo build --release && scripts/install.sh
```

Then, per desktop:

- **GNOME:** log out and back in, then `gnome-extensions enable masthead@kitefoldlabs.com`.
- **COSMIC:** Settings → Desktop → Panel → Configure panel applets → add *Masthead*.
- **Waybar:** add a module per metric:

  ```json
  "custom/masthead-cpu": {
      "exec": "masthead-bar --metric cpu --no-labels",
      "return-type": "json",
      "on-click": "masthead-bar --toggle cpu"
  }
  ```

- **i3 / sway:** `status_command masthead-bar --format i3bar`
- **Tray desktops:** nothing to do; tray items appear when no native face is running.

`just deb` builds a Debian package. More in [docs/install.md](docs/install.md).

## Resource use

| | At rest (bar only) | Popover open | Window open |
|---|---|---|---|
| CPU, one core | ≤ 0.1% | ≤ 0.4% | ≤ 3% |
| Memory | daemon ≤ 6 MB | + 0 | window ≤ 120 MB, freed on close |
| Disk writes | 0 | 0 | only when settings change |

On NVIDIA systems, NVIDIA's NVML library adds about 20 MB to the daemon while
GPU data is shown. It is unloaded a minute after the GPU leaves the bar, and
`[devices] nvml = false` turns it off. Measurements and method:
[docs/resource-budget.md](docs/resource-budget.md). CI fails any change that
breaks the resting budget.

## Configuration

`~/.config/masthead/config.toml`, edited from the window's Settings view or
by hand (changes apply immediately). Every key is optional:

```toml
[bar]
metrics = ["cpu", "mem", "gpu", "net", "temp"]
style = "values"          # values | trend | compact

[units]
memory = "binary"         # binary (GiB) | decimal (GB)
network = "bytes"         # bytes | bits
temperature = "celsius"   # celsius | fahrenheit

[thresholds.cpu]
warn = 85
crit = 95

[devices]
gpu = "auto"              # or a PCI address: "0000:01:00.0"
temperature = "auto"      # or "chip/label": "k10temp/Tctl"
interfaces = []           # empty: every physical interface
include_virtual_interfaces = false
nvml = true

[tray]
mode = "auto"             # auto | always | never

[notifications]
enabled = false           # notify when a metric stays critical
hold_seconds = 60
```

## Documents

| Doc | What it covers |
|---|---|
| [docs/architecture.md](docs/architecture.md) | Daemon, tiers, faces, and why |
| [docs/dbus-api.md](docs/dbus-api.md) | The `com.kitefoldlabs.Masthead1` interface |
| [docs/resource-budget.md](docs/resource-budget.md) | Budgets, measurements, how CI enforces them |
| [docs/install.md](docs/install.md) | Per-desktop setup and packaging |
| [docs/development.md](docs/development.md) | Building, testing, quality gates |
| [shell/gnome/README.md](shell/gnome/README.md) | The GNOME Shell extension |
| [crates/masthead-cosmic/README.md](crates/masthead-cosmic/README.md) | The COSMIC applet |
