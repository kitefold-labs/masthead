# Masthead for GNOME Shell

The GNOME Shell face of [Masthead](https://github.com/kitefold-labs/masthead):
system values in the top bar, a detail card on hover, the Masthead window on
click. GNOME Shell 45–50.

The extension only draws. `masthead-daemon` does all sampling and publishes
bar values on the session bus as `com.kitefoldlabs.Masthead1`; the extension
subscribes, renders what it receives and holds no sampling timers. Without
the daemon it shows a single dim Masthead item explaining what to install.

| Action | Result |
|--------|--------|
| Hover an item (300 ms) | Popover with a 60-second trend and details; closes 150 ms after the pointer leaves. Keyboard focus opens it too. |
| Left click, Enter or Space | Opens the window on that metric, switches to it, or closes it if it is already showing that metric. |
| Right click or Menu key | Open Masthead, Settings, Hide this metric. |

While a popover is open the extension holds a `hover:<metric>` subscription,
which makes the daemon add per-core load and top processes. It is released
when the popover closes. On disable, which GNOME does at the lock screen,
every subscription is released, so the daemon samples nothing while locked.

## Files

| File | Purpose |
|------|---------|
| `extension.js` | Bar items, hover timing, context menu, lifecycle |
| `client.js` | D-Bus calls and signals; reconnects when the daemon restarts |
| `popover.js` | The hover card |
| `draw.js` | Cairo sparklines and bars |
| `format.js` | Popover value formatting (mirrors `masthead-core`'s `format.rs`) |
| `stylesheet.css` | Halyard dark tokens |
| `icons/` | 16 px symbolic icons, fill-only so GNOME can recolor them. `icon-src/` holds the stroked originals; regenerate with Inkscape's *Stroke to Path*. |

## Install

The daemon must be installed first (it ships the D-Bus service file that
starts it on demand). Then:

```sh
make install          # gnome-extensions pack + install for this user
```

Log out and back in (Wayland cannot reload the shell), then:

```sh
gnome-extensions enable masthead@kitefoldlabs.com
```

## Develop

```sh
make check            # node --check on every module
make pack             # build masthead@kitefoldlabs.com.shell-extension.zip
```

To try changes without touching your session, run a nested shell:

```sh
dbus-run-session -- gnome-shell --devkit --wayland
```

and watch its output for `JS ERROR` lines. Configuration (bar metrics, style,
units, thresholds) lives in the daemon's `~/.config/masthead/config.toml` and
is changed through the Masthead window; the extension follows `ConfigChanged`.
