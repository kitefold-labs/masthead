<!--
  Copyright 2026 Kitefold Labs
  SPDX-License-Identifier: Apache-2.0
-->

# Development

## Prerequisites

| Tool | Version | Notes |
|---|---|---|
| Rust | 1.99 | Pinned in `rust-toolchain.toml`; rustup fetches it. |
| just | any | Task runner; `just --list`. |
| Build headers | | `pkg-config libxkbcommon-dev libwayland-dev libinput-dev libudev-dev libexpat1-dev libfontconfig-dev libfreetype-dev libegl-dev` (Debian/Ubuntu names), for libcosmic. |
| dbus | | `dbus-run-session` for the soak test. |
| Node | 22 | Linting the GNOME extension. |

The daemon, probe, core, API and bar crates build without any of the GUI
headers: `cargo build -p masthead-daemon -p masthead-bar`.

## Everyday commands

```sh
just build                 # debug build of everything
just test                  # cargo test --workspace
just lint                  # rustfmt, clippy -D warnings, typos
just soak 120              # resting budget over 120 s
masthead-daemon --print    # one sample of everything as JSON
MASTHEAD_LOG=debug target/debug/masthead-daemon
```

To run the daemon without touching your session's instance, use a private
bus:

```sh
dbus-run-session -- bash -c 'target/debug/masthead-daemon & sleep 1; target/debug/masthead-bar --format plain'
```

## Quality gates

CI (`.github/workflows/ci.yml`) runs on every pull request:

1. **Lint:** `cargo fmt --check`, `clippy --all-targets -D warnings` with the
   pedantic group, `typos`.
2. **Tests:** the whole workspace, plus line coverage of at least 90% on
   `masthead-probe`, `masthead-core` and `masthead-api` (the NVML binding and
   the generated D-Bus proxy are excluded: CI runners have no NVIDIA GPU).
3. **cargo-deny:** advisories, bans, sources and licenses. Runtime
   dependencies must be Apache-2.0 compatible; see `deny.toml`.
4. **Budget:** `scripts/soak.sh` for five minutes at rest and one minute with
   the window tier. Over budget fails the build.
5. **GNOME extension:** lint.
6. **Package:** AppStream and desktop-file validation, a `.deb` build.

## Rules the code keeps

- `unsafe` is denied workspace-wide. The two exceptions, the NVML binding and
  tokio fd registration, carry `SAFETY` comments.
- The steady-state sample path does not allocate. `crates/masthead-probe/tests/no_alloc.rs`
  enforces it against the real `/proc` and `/sys`.
- Faces never sample. If a face needs a number, it comes from the daemon.
- The GPL-licensed GNOME monitors (Resource Monitor, Vitals, Astra Monitor,
  TopHat) informed the design, but none of their code is used. Don't copy from
  them.

## Layout

```
crates/
  masthead-probe/    /proc and /sys readers, NVML shim, uevents
  masthead-core/     model, formatting, history, tiers, thresholds, config
  masthead-api/      D-Bus contract
  masthead-daemon/   sampler and D-Bus service
  masthead-bar/      Waybar / i3bar output
  masthead-window/   detail window (libcosmic)
  masthead-cosmic/   COSMIC applet (libcosmic)
shell/gnome/         GNOME Shell extension
data/                D-Bus, systemd, desktop, icon and AppStream files
scripts/             install, soak test, packaging
```
