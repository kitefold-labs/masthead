<!--
  Copyright 2026 Kitefold Labs
  SPDX-License-Identifier: Apache-2.0
-->

# D-Bus API

The daemon owns `com.kitefoldlabs.Masthead1` on the session bus and serves
interface `com.kitefoldlabs.Masthead1` at `/com/kitefoldlabs/Masthead1`.
Calling any method starts it through D-Bus activation. Rust clients use
`masthead_api::MastheadProxy`; the types live in `crates/masthead-api`.

The `1` in the name is the API version. Fields may be added to the JSON
payloads at any time; anything else that changes gets a new interface name.

## Methods

| Method | Signature | Description |
|---|---|---|
| `Describe` | `() → s` | Hardware found at startup, as JSON (`detail::Describe`): CPU model, core and thread counts, memory total, GPUs, sensors, interfaces, disks, metrics this machine can show. |
| `Subscribe` | `(s tier) → u` | Ask for a sampling tier: `panel`, `hover:<metric>` or `window`. Returns a handle. The subscription ends on `Unsubscribe` or when the calling connection closes. |
| `Unsubscribe` | `(u handle) → ()` | Only the connection that subscribed can unsubscribe. |
| `Snapshot` | `() → a(sdsus)` | The current bar items, for a first paint. |
| `GetDetail` | `(s metric) → s` | Detail JSON for a metric or `all`, from the latest sample. Process lists are empty unless a hover or window subscription is active. |
| `History` | `(s series, u seconds) → (d, ad, ad)` | `(step, min, max)` covering the last `seconds` (at most 86,400). Up to 900 s the step is 1 s and `min` equals `max`; beyond that the step is 60 s with each minute's extremes. NaN marks gaps. |
| `ToggleWindow` | `(s metric) → ()` | Open the window on `metric` (or `settings`), switch it there, or close it if it already shows that metric. |
| `GetConfig` | `() → s` | The configuration as JSON. |
| `SetConfig` | `(s json) → ()` | Validate, write `config.toml` atomically, apply, and emit `ConfigChanged`. |

Metric ids: `cpu`, `mem`, `gpu`, `net`, `disk`, `temp`, `battery`.

History series: `cpu`, `mem` (bytes used), `gpu`, `net.rx`, `net.tx`,
`disk.read`, `disk.write` (bytes per second), `temp` (°C), `battery` (%).

## Signals

| Signal | Signature | When |
|---|---|---|
| `Panel` | `(a(sdsus) items)` | A bar item's text, level or summary changed. At most once a second. |
| `Detail` | `(s metric, s json)` | Once a second for each hovered metric, and as `all` while the window is open. |
| `ConfigChanged` | `(s json)` | After `SetConfig` or an edit of `config.toml`. |

## Bar items: `(sdsus)`

| Field | Meaning |
|---|---|
| `id` | Metric id. Items arrive in the order of `bar.metrics` in the config. |
| `value` | Raw value: percent, bytes (`mem`), bytes per second (`net` download, `disk` read plus write), °C. |
| `text` | Formatted in the user's units: `23%`, `18.4G`, `↓2.1M ↑180K`, `64°`. |
| `level` | 0 normal, 1 warn, 2 critical. |
| `summary` | One line for a tooltip: `load 4.12 · 64 °C`. |

## Properties

| Property | Type | |
|---|---|---|
| `Version` | `s` | Daemon version. |

## The window's interface

`masthead-window` owns `com.kitefoldlabs.Masthead.Window` and serves
`com.kitefoldlabs.Masthead.Window1` at `/com/kitefoldlabs/Masthead/Window`:

| Method | Signature | |
|---|---|---|
| `Toggle` | `(s metric) → ()` | Show `metric`, or close if it is already showing. |
| `Show` | `(s metric) → ()` | Show `metric` and raise the window. |

## Try it

```sh
busctl --user call com.kitefoldlabs.Masthead1 /com/kitefoldlabs/Masthead1 \
    com.kitefoldlabs.Masthead1 GetDetail s cpu
busctl --user call com.kitefoldlabs.Masthead1 /com/kitefoldlabs/Masthead1 \
    com.kitefoldlabs.Masthead1 History su cpu 60
masthead-daemon --print     # one sample of everything, as JSON, without D-Bus
```
