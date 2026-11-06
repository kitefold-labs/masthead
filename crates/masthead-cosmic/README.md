<!--
  Copyright 2026 Kitefold Labs
  SPDX-License-Identifier: Apache-2.0
-->

# masthead-cosmic

Masthead for the COSMIC panel. Each bar item shows an icon and its value.
Resting the pointer on an item for 300 ms opens a popover with the last
minute as a sparkline and a few facts about the metric; clicking an item opens
the Masthead window on that metric. Amber and red appear only when a value
crosses its threshold.

The applet samples nothing. It subscribes to `masthead-daemon` over the
session bus: the `panel` tier while it runs, and `hover:<metric>` only while a
popover is open.

## Install

```sh
just install    # every Masthead binary and data file into ~/.local
```

The applet starts `masthead-daemon` through D-Bus activation. The installer
writes absolute paths into the desktop entries, so `~/.local/bin` need not be
on the session `PATH`. System packages install the same files under `/usr`.

## Add it to the panel

Settings → Desktop → Panel → Applets → Add applet → **Masthead**. It works in
the top or bottom panel, where items sit side by side, and in a left or right
panel, where each icon sits above a shortened value. Which metrics appear, in
what order, and their thresholds come from Masthead's settings, shared with
every other Masthead face.

## Debugging

`MASTHEAD_LOG=debug masthead-cosmic` prints the applet's UI events (hover,
subscription, popover lifecycle) to stderr. Outside COSMIC the applet runs as
a small transparent window, which is enough to check layout and popovers.

## Notes

- Popovers are anchored flush to the panel and draw their 6 px gap inside the
  surface. xdg-shell requires a popup to touch its parent, and mutter
  dismisses one that does not.
- The popover takes no input: the pointer passes through it, and leaving the
  item closes it after 150 ms.
- Values use JetBrains Mono when it is installed and the system monospace
  face otherwise, so a changing value never shifts the panel.

## License

The applet's source is Apache-2.0 like the rest of Masthead. libcosmic's
panel support links `cosmic-panel-config` and `xdg-shell-wrapper-config`,
which are GPL-3.0-only, so a built `masthead-cosmic` binary is distributed
under the GPL-3.0. No other Masthead binary links them.
