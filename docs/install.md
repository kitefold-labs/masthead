# Installing Masthead

## From source

```sh
just install              # release build, then scripts/install.sh into ~/.local
just uninstall
```

`scripts/install.sh` puts these in place (paths for a user install):

| File | Where |
|---|---|
| `masthead-daemon`, `masthead-window`, `masthead-bar`, `masthead-cosmic` | `~/.local/bin` |
| D-Bus activation files for the daemon and the window | `~/.local/share/dbus-1/services` |
| `masthead.service` (systemd user unit) | `~/.config/systemd/user` |
| Desktop entries, icons, AppStream metadata | `~/.local/share/...` |
| GNOME Shell extension | `~/.local/share/gnome-shell/extensions/masthead@kitefoldlabs.com` |

Nothing needs enabling for the daemon: the first face that subscribes starts
it through D-Bus activation, and systemd restarts it if it fails. The unit
also caps it at 5% of a core and 128 MB, far above its budget, so a fault
cannot hurt the session.

The D-Bus activation files and desktop entries use absolute paths, so
`~/.local/bin` only needs to be on your `PATH` for commands in bar configs
such as Waybar's `exec` and `on-click`.

## Per desktop

### GNOME Shell 45–50

GNOME Shell on Wayland loads new extensions only at login. Log out and back
in, then:

```sh
gnome-extensions enable masthead@kitefoldlabs.com
```

The extension disables itself on the lock screen, which releases its
subscriptions, and the daemon stops sampling until you unlock.

### COSMIC

Add *Masthead* in Settings → Desktop → Panel → Configure panel applets. See
[crates/masthead-cosmic/README.md](../crates/masthead-cosmic/README.md).

### Waybar

One module per metric gives each its own tooltip, click target and CSS class:

```json
"modules-right": ["custom/masthead-cpu", "custom/masthead-mem", "custom/masthead-net"],
"custom/masthead-cpu": {
    "exec": "masthead-bar --metric cpu --no-labels",
    "return-type": "json",
    "format": " {}",
    "on-click": "masthead-bar --toggle cpu"
}
```

Each line carries `text`, `tooltip`, `percentage` (for percentages) and
`class`: `masthead`, the level (`normal`, `warn`, `crit`) and the metric id,
so `#custom-masthead-cpu.crit { color: #f87171; }` works. Ironbar and eww
read the same JSON.

### i3 and swaybar

```
bar {
    status_command masthead-bar --format i3bar
}
```

Clicks open the window on the clicked metric.

### KDE Plasma, XFCE, Cinnamon, MATE, Budgie

Nothing to configure. When no native face is running and the desktop has a
tray (a StatusNotifierWatcher), the daemon shows one tray item per bar metric,
with the value drawn into the icon and details in the tooltip. Click opens the
window; right-click opens its settings. `[tray] mode` in the config can force
it on or off.

## Packages

```sh
just deb                  # dist/masthead_<version>_<arch>.deb
```

The package installs everything system-wide under `/usr`, including the GNOME
extension. A PPA, COPR, AUR package, Flathub (window only: inside a Flatpak
sandbox `/proc` shows only the sandbox's own processes, so the daemon must run
on the host) and an extensions.gnome.org listing are planned.
