# masthead-window

The detail window: a sidebar of metrics with sparklines, a chart with 1 m /
5 m / 15 m / 1 h ranges, per-device detail, a read-only process table and the
settings form. It is built on iced through libcosmic and styled with Halyard's
tokens (`src/palette.rs`), Space Grotesk for UI text and JetBrains Mono for
values.

It samples nothing. Everything comes from `masthead-daemon` over the session
bus: it subscribes to the `window` tier, loads history for the visible chart,
then follows the `Detail("all")` signal. Closing the window exits the process.

## Starting it

| Command | What it does |
|---|---|
| `masthead-window [--metric ID]` | Opens on a metric, `processes` or `settings`. If a window is already running, asks it to show that page and exits. |
| `masthead-window --service` | What D-Bus activation runs. Claims `com.kitefoldlabs.Masthead.Window` and opens on the first `Toggle` or `Show` call. |

The window serves `com.kitefoldlabs.Masthead.Window1` at
`/com/kitefoldlabs/Masthead/Window`: `Toggle(s)` shows a page or closes the
window if that page is already showing; `Show(s)` shows and raises it.

`MASTHEAD_THEME=dark|light` overrides the desktop color scheme, which is
otherwise read from the settings portal (`org.freedesktop.appearance
color-scheme`).

## Rendering and cost

The default renderer is tiny-skia (software). Measured on a Ryzen 9 9950X3D
under a headless GNOME Shell 50 at 1040×720, 60 s per page, release build:

| Page | CPU, share of one core | RSS |
|---|---|---|
| Settings | 0.3% | 33 MB |
| CPU (chart, heat grid, top processes) | 1.5–1.9% | 33 MB |
| Processes | 1.2% | 33 MB |

The `wgpu` feature renders on the GPU instead. It cut CPU to about 0.35% on
every page, but RSS rose to 170–210 MB on an NVIDIA machine because the
driver's libraries are mapped into the process, which is over the 120 MB
budget. Software rendering stays the default.

Every update redraws the whole window in software, so the window keeps
updates rare and each frame cheap:

- bar items (`Panel` signals) are forwarded only when an alert level changes;
- chart geometry is cached and rebuilt only when a sample arrives;
- the process table fits the window instead of scrolling, has one text per
  cell and no per-row bars (each canvas bar cost more than its row's text),
  and refreshes every 3 s, as GNOME System Monitor does.

## Fonts

`fonts/` holds Space Grotesk (Florian Karsten) and JetBrains Mono (JetBrains),
both under the SIL Open Font License 1.1; see the `OFL-*.txt` files.
