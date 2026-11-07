// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! Page layouts. Every color comes from the app's [`Palette`].

use crate::app::{self, App, Message, THRESHOLD_METRICS};
use crate::chart::{Chart, Meter, Spark};
use crate::icons;
use crate::kill::{Signal, Target};
use crate::model::{self, Page, Range, SortKey};
use crate::palette::Palette;
use cosmic::Element;
use cosmic::iced::alignment::Horizontal;
use cosmic::iced::font::{Family, Stretch, Style as FontStyle, Weight};
use cosmic::iced::widget::{Space, button, canvas, column, container, keyed_column, mouse_area, row, scrollable, svg, text};
use cosmic::iced::{Alignment, Background, Border, Color, Font, Length, Padding};
use masthead_api::detail::{GpuDetail, ProcDetail};
use masthead_core::config::{BarStyle, MemoryUnits, NetworkUnits, TemperatureUnits, TrayMode};
use masthead_core::{MetricId, format};

const fn font(family: &'static str, weight: Weight) -> Font {
    Font { family: Family::Name(family), weight, stretch: Stretch::Normal, style: FontStyle::Normal }
}

pub const SANS: Font = font("Space Grotesk", Weight::Normal);
pub const MEDIUM: Font = font("Space Grotesk", Weight::Medium);
pub const BOLD: Font = font("Space Grotesk", Weight::Bold);
pub const MONO: Font = font("JetBrains Mono", Weight::Normal);
pub const MONO_MEDIUM: Font = font("JetBrains Mono", Weight::Medium);

pub mod disk_page;

type El<'a> = Element<'a, Message>;

fn t<'a>(s: impl Into<String>, size: f32, f: Font, c: Color) -> text::Text<'a, cosmic::Theme, cosmic::Renderer> {
    text(s.into()).size(size).font(f).class(cosmic::theme::Text::Color(c))
}

/// Uppercase section label in the mono face.
fn eyebrow<'a>(s: &str, p: &Palette) -> El<'a> {
    t(s.to_uppercase(), 10.5, MONO_MEDIUM, p.fg_muted).into()
}

fn fill() -> Space {
    Space::new().width(Length::Fill)
}

fn boxed<'a>(content: impl Into<El<'a>>, p: Palette, bg: Color) -> container::Container<'a, Message, cosmic::Theme, cosmic::Renderer> {
    container(content).style(move |_| container::Style {
        background: Some(Background::Color(bg)),
        border: Border { color: p.border, width: 1.0, radius: 8.0.into() },
        text_color: Some(p.fg),
        ..container::Style::default()
    })
}

fn swatch<'a>(c: Color, w: impl Into<Length>, h: impl Into<Length>, radius: f32) -> El<'a> {
    container(Space::new().width(w).height(h))
        .style(move |_| container::Style {
            background: Some(Background::Color(c)),
            border: Border { radius: radius.into(), ..Border::default() },
            ..container::Style::default()
        })
        .into()
}

fn icon<'a>(h: svg::Handle, c: Color, size: f32) -> El<'a> {
    svg(h).width(size).height(size).class(cosmic::theme::Svg::custom(move |_| svg::Style { color: Some(c) })).into()
}

fn meter<'a>(fraction: f64, color: Color, p: &Palette, width: Length, height: f32) -> El<'a> {
    canvas(Meter { fraction: fraction as f32, color, track: p.surface3 }).width(width).height(height).into()
}

/// A flat button with a hover and selected background.
fn flat<'a>(content: impl Into<El<'a>>, msg: Message, p: Palette, selected: bool, pad: Padding) -> El<'a> {
    button(content)
        .on_press(msg)
        .padding(pad)
        .width(Length::Fill)
        .class(cosmic::theme::iced::Button::Custom(Box::new(move |_, status| {
            let bg = if selected {
                Some(Background::Color(p.accent_soft))
            } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                Some(Background::Color(p.surface2))
            } else {
                None
            };
            button::Style {
                background: bg,
                text_color: p.fg,
                border: Border { radius: 6.0.into(), ..Border::default() },
                border_radius: 6.0.into(),
                ..button::Style::default()
            }
        })))
        .into()
}

/// A small segmented-control button.
fn chip<'a>(label: &str, msg: Message, p: Palette, on: bool) -> El<'a> {
    button(t(label.to_owned(), 11.5, MONO_MEDIUM, if on { p.fg } else { p.fg_muted }))
        .on_press(msg)
        .padding([3, 9])
        .class(cosmic::theme::iced::Button::Custom(Box::new(move |_, status| {
            let bg = if on {
                Some(Background::Color(p.surface))
            } else if matches!(status, button::Status::Hovered) {
                Some(Background::Color(p.surface3))
            } else {
                None
            };
            button::Style {
                background: bg,
                text_color: p.fg,
                border: Border { radius: 4.0.into(), color: if on { p.border } else { Color::TRANSPARENT }, width: 1.0 },
                border_radius: 4.0.into(),
                ..button::Style::default()
            }
        })))
        .into()
}

fn panel_level(app: &App, m: MetricId) -> u32 {
    app.panel.iter().find(|i| i.id == m.as_str()).map_or(0, |i| i.level)
}

// ----------------------------------------------------------------------------
// Header
// ----------------------------------------------------------------------------

/// Quit Masthead, at the end of the header bar.
pub fn quit_button<'a>(p: &Palette) -> El<'a> {
    let p = *p;
    button(row![icon(icons::handle("power"), p.fg_muted, 14.0), t("Quit", 12.5, MEDIUM, p.fg_muted)].spacing(6).align_y(Alignment::Center))
        .on_press(Message::AskQuitAll)
        .padding([4, 10])
        .class(cosmic::theme::iced::Button::Custom(Box::new(move |_, status| button::Style {
            background: matches!(status, button::Status::Hovered | button::Status::Pressed).then_some(Background::Color(p.surface3)),
            text_color: p.fg,
            border: Border { radius: 6.0.into(), ..Border::default() },
            ..button::Style::default()
        })))
        .into()
}

/// Confirmation for Quit Masthead.
pub fn quit_dialog<'a>() -> El<'a> {
    cosmic::widget::dialog()
        .title("Quit Masthead?")
        .body(
            "The bar items go away and monitoring stops. Open Masthead from your applications to start it again; \
             it also starts at your next login.",
        )
        .primary_action(cosmic::widget::button::destructive("Quit Masthead").on_press(Message::QuitAll))
        .secondary_action(cosmic::widget::button::standard("Cancel").on_press(Message::CancelQuitAll))
        .into()
}

/// Logo and name for the start of the header bar.
pub fn brand<'a>(p: &Palette) -> El<'a> {
    row![svg(icons::logo()).width(22).height(22), t("Masthead", 14.0, BOLD, p.fg)]
        .spacing(8)
        .padding([0, 6])
        .align_y(Alignment::Center)
        .into()
}

// ----------------------------------------------------------------------------
// Sidebar
// ----------------------------------------------------------------------------

pub fn sidebar(app: &App) -> El<'_> {
    let p = app.pal;
    let mut col = column![].spacing(2).padding([10, 8]);
    let units = app.units();
    let mem_total = app.mem_total();
    for m in app.metrics() {
        let selected = app.page == Page::Metric(m) || (m == MetricId::Disk && app.page == Page::DiskUsage);
        let value = app.detail.as_ref().and_then(|d| model::short_value(m, d, units)).unwrap_or_else(|| "–".into());
        let level = panel_level(app, m);
        let vcolor = if level > 0 { p.level(level) } else { p.fg_muted };
        let names = model::series_for(m);
        let series: Vec<_> = names.iter().map(|s| app.spark.get(s).cloned().unwrap_or_default()).collect();
        let max = model::y_max(m, &series, mem_total);
        let spark: El<'_> = match app.spark_cache.get(&m) {
            Some(cache) => canvas(Spark { series, max, pal: p, cache }).width(Length::Fill).height(16).into(),
            None => Space::new().height(16).into(),
        };
        let head = row![
            icon(icons::metric(m), if selected { p.accent } else { p.fg_muted }, 15.0),
            t(m.title(), 13.0, MEDIUM, p.fg),
            fill(),
            t(value, 11.5, MONO, vcolor),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        let item = column![head, row![Space::new().width(23), spark]].spacing(4);
        col = col.push(flat(item, Message::Select(Page::Metric(m)), p, selected, Padding::from([7, 8])));
    }
    col = col.push(Space::new().height(10));
    for (page, name, label) in [(Page::Processes, "processes", "Processes"), (Page::Settings, "settings", "Settings")] {
        let selected = app.page == page;
        let r = row![icon(icons::handle(name), if selected { p.accent } else { p.fg_muted }, 15.0), t(label, 13.0, MEDIUM, p.fg)]
            .spacing(8)
            .align_y(Alignment::Center);
        col = col.push(flat(r, Message::Select(page), p, selected, Padding::from([8, 8])));
    }
    let side = container(scrollable(col).height(Length::Fill))
        .width(212)
        .height(Length::Fill)
        .style(move |_| container::Style { background: Some(Background::Color(p.surface)), ..container::Style::default() });
    row![side, swatch(Palette::over(p.border, p.bg), 1.0, Length::Fill, 0.0)].height(Length::Fill).into()
}

// ----------------------------------------------------------------------------
// Main area
// ----------------------------------------------------------------------------

pub fn main(app: &App) -> El<'_> {
    let p = app.pal;
    let mut col = column![].spacing(14).padding([16, 22, 22, 22]);
    if let Some(e) = &app.error {
        col = col.push(container(t(e.clone(), 13.0, SANS, p.warning)).padding([8, 12]).width(Length::Fill).style(move |_| {
            container::Style {
                background: Some(Background::Color(Palette::with_alpha(p.warning, 0.12))),
                border: Border { radius: 6.0.into(), ..Border::default() },
                ..container::Style::default()
            }
        }));
    }
    let body: El<'_> = match app.page {
        Page::Metric(m) => scrollable(col.push(metric_page(app, m))).height(Length::Fill).into(),
        Page::Settings => scrollable(col.push(settings_page(app))).height(Length::Fill).into(),
        // The page scrolls below its header, so results stay in view.
        Page::DiskUsage => col.push(disk_page::page(app)).height(Length::Fill).into(),
        // The process table sizes itself to the window instead of scrolling:
        // software rendering draws text inside a scroll clip several times
        // slower, and this table is the busiest thing the window draws.
        Page::Processes => {
            col.push(cosmic::iced::widget::responsive(move |size| processes_page(app, size.height))).height(Length::Fill).into()
        }
    };
    container(body)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(p.bg)),
            text_color: Some(p.fg),
            ..container::Style::default()
        })
        .into()
}

fn device_line(app: &App, m: MetricId) -> String {
    let d = app.detail.as_ref();
    let u = app.units();
    match m {
        MetricId::Cpu => {
            app.describe.as_ref().map(|x| format!("{} · {} cores · {} threads", x.cpu_model, x.cores, x.threads)).unwrap_or_default()
        }
        MetricId::Mem => d
            .and_then(|d| d.mem.as_ref())
            .map(|x| format!("{} usable · swap {}", format::mem_long(x.total, u.memory), format::mem_long(x.swap_total, u.memory)))
            .unwrap_or_default(),
        MetricId::Gpu => {
            d.and_then(|d| d.gpus.as_ref()).and_then(|g| g.iter().find(|g| g.primary)).map(|g| g.name.clone()).unwrap_or_default()
        }
        MetricId::Net => d
            .and_then(|d| d.net.as_ref())
            .map(|n| {
                let mut s: Vec<String> = n.interfaces.iter().filter(|i| i.counted).map(|i| i.name.clone()).collect();
                if n.hidden_virtual > 0 {
                    s.push(format!("{} virtual hidden", n.hidden_virtual));
                }
                s.join(" · ")
            })
            .unwrap_or_default(),
        MetricId::Disk => app
            .describe
            .as_ref()
            .map(|x| {
                let models: Vec<&str> = x.disks.iter().map(|d| d.model.as_str()).collect();
                match models.first() {
                    Some(first) if models.iter().all(|m| m == first) => format!("{} × {first}", models.len()),
                    _ => x.disks.iter().map(|d| d.name.clone()).collect::<Vec<_>>().join(" · "),
                }
            })
            .unwrap_or_default(),
        MetricId::Temp => {
            app.panel.iter().find(|i| i.id == "temp").map_or_else(|| "All sensors".to_owned(), |i| format!("Bar shows {}", i.summary))
        }
        MetricId::Battery => d
            .and_then(|d| d.batteries.as_ref())
            .map(|b| b.iter().map(|b| b.name.clone()).collect::<Vec<_>>().join(" · "))
            .unwrap_or_default(),
    }
}

fn metric_page(app: &App, m: MetricId) -> El<'_> {
    let p = app.pal;
    let u = app.units();
    let mut chips = row![].spacing(2);
    for r in Range::ALL {
        chips = chips.push(chip(r.label(), Message::Range(r), p, r == app.range));
    }
    let chips = container(chips).padding(2).style(move |_| container::Style {
        background: Some(Background::Color(p.surface2)),
        border: Border { color: p.border, width: 1.0, radius: 6.0.into() },
        ..container::Style::default()
    });
    let head = row![column![t(m.title(), 21.0, BOLD, p.fg), t(device_line(app, m), 11.5, MONO, p.fg_muted)].spacing(3), fill(), chips]
        .align_y(Alignment::Start);

    let mut col = column![head].spacing(14);
    if let Some(labels) = match m {
        MetricId::Net => Some(["Down", "Up"]),
        MetricId::Disk => Some(["Read", "Write"]),
        _ => None,
    } {
        col = col.push(
            row![
                swatch(p.accent, 12.0, 2.0, 1.0),
                t(labels[0], 12.0, SANS, p.fg_muted),
                Space::new().width(10),
                swatch(p.fg_muted, 12.0, 2.0, 1.0),
                t(labels[1], 12.0, SANS, p.fg_muted)
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        );
    }
    let mem_total = app.mem_total();
    let max = model::y_max(m, &app.chart, mem_total);
    let chart = Chart {
        series: &app.chart,
        slots: app.range.points(),
        max,
        pal: p,
        label: Box::new(move |v| model::fmt_axis(m, v, u)),
        range_label: format!("−{}", app.range.label()),
        cache: &app.chart_cache,
        mono: MONO,
    };
    col = col.push(canvas(chart).width(Length::Fill).height(196));

    let st = app.chart.first().map(model::stats).unwrap_or_default();
    let stat = |k: &str, v: Option<f64>| -> El<'_> {
        boxed(
            column![eyebrow(k, &p), t(v.map_or_else(|| "–".to_owned(), |v| model::fmt_value(m, v, u)), 17.0, MONO_MEDIUM, p.fg)].spacing(2),
            p,
            p.surface,
        )
        .padding([8, 12])
        .width(Length::Fill)
        .into()
    };
    col = col.push(row![stat("Now", st.now), stat("Average", st.avg), stat("Peak", st.peak)].spacing(10));
    col = col.push(detail(app, m));
    col.into()
}

fn kv<'a>(k: &str, v: String, p: &Palette) -> El<'a> {
    row![t(k.to_owned(), 12.5, SANS, p.fg_muted), fill(), t(v, 12.5, MONO_MEDIUM, p.fg)].align_y(Alignment::Center).into()
}

/// Columns: label, width (None = fill), right-aligned.
type Cols<'c> = &'c [(&'c str, Option<f32>, bool)];

fn cell(content: El<'_>, w: Option<f32>, right: bool) -> El<'_> {
    let c = container(content).width(w.map_or(Length::Fill, Length::Fixed));
    if right { c.align_x(Horizontal::Right).into() } else { c.into() }
}

fn table_head<'a>(cols: Cols<'_>, p: &Palette) -> El<'a> {
    let mut r = row![].spacing(10);
    for (label, w, right) in cols {
        r = r.push(cell(eyebrow(label, p), *w, *right));
    }
    column![r, swatch(p.border, Length::Fill, 1.0, 0.0)].spacing(5).into()
}

fn table_row<'a>(cells: Vec<El<'a>>, cols: Cols<'_>, p: &Palette) -> El<'a> {
    let mut r = row![].spacing(10).align_y(Alignment::Center);
    for (c, (_, w, right)) in cells.into_iter().zip(cols) {
        r = r.push(cell(c, *w, *right));
    }
    column![container(r).padding([5, 0]), swatch(p.border, Length::Fill, 1.0, 0.0)].into()
}

fn proc_rows<'a>(procs: &[ProcDetail], by_mem: bool, p: &Palette, u: masthead_core::config::Units) -> El<'a> {
    let cols: Cols<'_> =
        &[("Process", None, false), ("User", Some(110.0), false), (if by_mem { "Resident" } else { "CPU" }, Some(150.0), true)];
    let mut col = column![table_head(cols, p)];
    let top = procs.iter().map(|x| if by_mem { x.rss as f64 } else { x.cpu }).fold(0.0_f64, f64::max).max(if by_mem { 1.0 } else { 10.0 });
    for x in procs {
        let (v, label) = if by_mem { (x.rss as f64, format::mem_long(x.rss, u.memory)) } else { (x.cpu, format!("{:.1}%", x.cpu)) };
        let value =
            row![t(label, 12.5, MONO, p.fg), meter(v / top, p.accent, p, Length::Fixed(56.0), 4.0)].spacing(8).align_y(Alignment::Center);
        col = col.push(table_row(
            vec![t(x.name.clone(), 13.0, SANS, p.fg).into(), t(x.user.clone(), 12.5, SANS, p.fg_muted).into(), value.into()],
            cols,
            p,
        ));
    }
    if procs.is_empty() {
        col = col.push(t("Waiting for the first process scan…", 12.5, SANS, p.fg_muted));
    }
    col.into()
}

#[allow(clippy::too_many_lines)] // one arm per metric
fn detail(app: &App, m: MetricId) -> El<'_> {
    let p = app.pal;
    let u = app.units();
    let Some(d) = app.detail.as_ref() else {
        return t("Waiting for masthead-daemon…", 13.0, SANS, p.fg_muted).into();
    };
    let mut col = column![].spacing(10);
    match m {
        MetricId::Cpu => {
            if let Some(c) = &d.cpu {
                col = col.push(eyebrow("Threads", &p));
                let mut grid = column![].spacing(3);
                for chunk in c.per_core.chunks(16) {
                    let mut r = row![].spacing(3);
                    for v in chunk {
                        let a = 0.12 + 0.88 * (f64::from(*v) / 100.0).clamp(0.0, 1.0) as f32;
                        let cc = Palette::with_alpha(p.accent, a);
                        r = r.push(container(Space::new().height(18)).width(Length::Fill).style(move |_| container::Style {
                            background: Some(Background::Color(cc)),
                            border: Border { radius: 3.0.into(), ..Border::default() },
                            ..container::Style::default()
                        }));
                    }
                    grid = grid.push(r);
                }
                col = col.push(grid);
                let sys = d.system.as_ref();
                let facts = row![
                    boxed(
                        column![
                            kv("Load 1 · 5 · 15", format!("{:.2}  {:.2}  {:.2}", c.load[0], c.load[1], c.load[2]), &p),
                            kv(
                                "Average frequency",
                                if c.avg_freq_mhz > 0.0 { format!("{:.2} GHz", c.avg_freq_mhz / 1000.0) } else { "–".into() },
                                &p
                            ),
                        ]
                        .spacing(6),
                        p,
                        p.surface
                    )
                    .padding([10, 12])
                    .width(Length::Fill),
                    boxed(
                        column![
                            kv("Package", c.package_temp_c.map_or_else(|| "–".into(), |v| format::temp(v, u.temperature, false)), &p),
                            kv(
                                "Uptime · processes",
                                sys.map_or_else(|| "–".into(), |s| format!("{} · {}", format::duration(s.uptime), s.procs_total)),
                                &p
                            ),
                        ]
                        .spacing(6),
                        p,
                        p.surface
                    )
                    .padding([10, 12])
                    .width(Length::Fill),
                ]
                .spacing(10);
                col = col.push(facts);
                col = col.push(eyebrow("Top processes · CPU, 100% is one core", &p));
                col = col.push(proc_rows(&c.top, false, &p, u));
            }
        }
        MetricId::Mem => {
            if let Some(x) = &d.mem {
                // used + available = total; available splits into reclaimable cache and free.
                let total = x.total.max(1) as f64;
                let free = x.free.min(x.available);
                let cache = x.available.saturating_sub(free);
                let part = |v: u64| ((v as f64 / total) * 1000.0).round().max(1.0) as u16;
                col = col.push(eyebrow("Breakdown", &p));
                col = col.push(
                    row![
                        container(Space::new().height(10)).width(Length::FillPortion(part(x.used))).style(seg(p.accent)),
                        container(Space::new().height(10)).width(Length::FillPortion(part(cache))).style(seg(p.accent_line)),
                        container(Space::new().height(10)).width(Length::FillPortion(part(free))).style(seg(p.surface3)),
                    ]
                    .spacing(2),
                );
                let legend = |c: Color, label: String| -> El<'_> {
                    row![swatch(c, 8.0, 8.0, 2.0), t(label, 12.0, SANS, p.fg_muted)].spacing(6).align_y(Alignment::Center).into()
                };
                col = col.push(
                    row![
                        legend(p.accent, format!("Used {}", format::mem_long(x.used, u.memory))),
                        legend(p.accent_line, format!("Reclaimable cache {}", format::mem_long(cache, u.memory))),
                        legend(p.surface3, format!("Free {}", format::mem_long(free, u.memory))),
                        legend(
                            p.fg_dim,
                            format!("Swap {} of {}", format::mem_long(x.swap_used, u.memory), format::mem_long(x.swap_total, u.memory))
                        ),
                    ]
                    .spacing(16),
                );
                col = col.push(eyebrow("Top processes · resident memory", &p));
                col = col.push(proc_rows(&x.top, true, &p, u));
            }
        }
        MetricId::Gpu => {
            if let Some(gpus) = &d.gpus {
                let mut r = row![].spacing(10);
                for g in gpus {
                    r = r.push(gpu_card(g, &p, u));
                }
                col = col.push(r);
            }
        }
        MetricId::Net => {
            if let Some(n) = &d.net {
                col = col.push(eyebrow("Interfaces", &p));
                let cols: Cols<'_> = &[
                    ("Interface", None, false),
                    ("Down", Some(100.0), true),
                    ("Up", Some(100.0), true),
                    ("Since boot", Some(140.0), true),
                ];
                let mut tbl = column![table_head(cols, &p)];
                for i in &n.interfaces {
                    let speed = match i.speed_mbps {
                        Some(s) if s >= 1000 => format!(" · {} Gb/s", f64::from(s) / 1000.0),
                        Some(s) => format!(" · {s} Mb/s"),
                        None => String::new(),
                    };
                    let name = format!("{} · {}{speed}", i.name, i.kind);
                    let name_el: El<'_> =
                        row![t(name, 13.0, SANS, p.fg), t(if i.counted { "" } else { "  not counted" }, 11.5, SANS, p.fg_dim)].into();
                    tbl = tbl.push(table_row(
                        vec![
                            name_el,
                            t(format::rate(i.rx, u.network, false), 12.5, MONO, p.fg).into(),
                            t(format::rate(i.tx, u.network, false), 12.5, MONO, p.fg).into(),
                            t(format!("↓{} ↑{}", format::bytes(i.rx_total), format::bytes(i.tx_total)), 12.0, MONO, p.fg_muted).into(),
                        ],
                        cols,
                        &p,
                    ));
                }
                col = col.push(tbl);
                if n.hidden_virtual > 0 {
                    col = col.push(t(
                        format!(
                            "{} virtual interfaces (docker0, bridges, veth) are hidden and not counted. Change this in Settings.",
                            n.hidden_virtual
                        ),
                        12.5,
                        SANS,
                        p.fg_muted,
                    ));
                }
            }
        }
        MetricId::Disk => {
            if let Some(x) = &d.disk {
                col = col.push(eyebrow("Devices", &p));
                let cols: Cols<'_> =
                    &[("Device", None, false), ("Size", Some(90.0), true), ("Read", Some(100.0), true), ("Write", Some(100.0), true)];
                let mut tbl = column![table_head(cols, &p)];
                for dv in &x.devices {
                    tbl = tbl.push(table_row(
                        vec![
                            row![t(dv.name.clone(), 13.0, MONO_MEDIUM, p.fg), t(format!("  {}", dv.model), 12.5, SANS, p.fg_muted)].into(),
                            t(format::bytes(dv.size), 12.5, MONO, p.fg_muted).into(),
                            t(format::disk_rate(dv.read, false), 12.5, MONO, p.fg).into(),
                            t(format::disk_rate(dv.write, false), 12.5, MONO, p.fg).into(),
                        ],
                        cols,
                        &p,
                    ));
                }
                col = col.push(tbl);
                if !x.filesystems.is_empty() {
                    col = col.push(eyebrow("Space", &p));
                    for f in &x.filesystems {
                        let frac = if f.total > 0 { f.used as f64 / f.total as f64 } else { 0.0 };
                        let c = if frac >= 0.95 {
                            p.danger
                        } else if frac >= 0.9 {
                            p.warning
                        } else {
                            p.accent
                        };
                        col = col.push(
                            column![
                                row![
                                    t(f.mount.clone(), 13.0, MONO_MEDIUM, p.fg),
                                    t(format!("  {} · {}", f.fstype, f.device), 12.0, SANS, p.fg_dim),
                                    fill(),
                                    t(format!("{} of {}", format::bytes(f.used), format::bytes(f.total)), 12.5, MONO, p.fg)
                                ]
                                .align_y(Alignment::Center),
                                meter(frac, c, &p, Length::Fill, 6.0)
                            ]
                            .spacing(5),
                        );
                    }
                }
            }
            col = col.push(disk_usage_card(app));
        }
        MetricId::Temp => {
            if let Some(s) = &d.sensors {
                let warn = app.config.as_ref().and_then(|c| c.thresholds.get(&MetricId::Temp)).map_or(80.0, |t| t.warn);
                let cols: Cols<'_> = &[("Sensor", None, false), ("Value", Some(170.0), true)];
                for kind in ["temp", "fan", "voltage", "power"] {
                    let rows: Vec<_> = s.iter().filter(|x| x.kind == kind).collect();
                    if rows.is_empty() {
                        continue;
                    }
                    col = col.push(eyebrow(
                        match kind {
                            "temp" => "Temperatures",
                            "fan" => "Fans",
                            "voltage" => "Voltages",
                            _ => "Power",
                        },
                        &p,
                    ));
                    let mut tbl = column![table_head(cols, &p)];
                    for x in rows {
                        let (label, bar): (String, Option<(f64, Color)>) = match (kind, x.value) {
                            (_, None) => ("–".into(), None),
                            ("temp", Some(v)) => {
                                (format::temp(v, u.temperature, false), Some((v / 100.0, if v >= warn { p.warning } else { p.accent })))
                            }
                            ("fan", Some(v)) => (format!("{v:.0} RPM"), None),
                            ("voltage", Some(v)) => (format!("{v:.3} V"), None),
                            (_, Some(v)) => (format::watts(v), None),
                        };
                        let mut value = row![t(label, 12.5, MONO, p.fg)].spacing(8).align_y(Alignment::Center);
                        if let Some((f, c)) = bar {
                            value = value.push(meter(f, c, &p, Length::Fixed(60.0), 4.0));
                        }
                        tbl = tbl.push(table_row(vec![t(x.name.clone(), 13.0, SANS, p.fg).into(), value.into()], cols, &p));
                    }
                    col = col.push(tbl);
                }
            }
        }
        MetricId::Battery => {
            if let Some(b) = &d.batteries {
                for x in b {
                    col = col.push(
                        boxed(
                            column![
                                t(x.name.clone(), 13.0, MEDIUM, p.fg),
                                kv("Charge", x.percent.map_or_else(|| "–".into(), |v| format!("{v:.0}%")), &p),
                                meter(x.percent.unwrap_or(0.0) / 100.0, p.accent, &p, Length::Fill, 6.0),
                                kv("State", x.state.clone(), &p),
                                kv("Rate", x.watts.map_or_else(|| "–".into(), format::watts), &p),
                            ]
                            .spacing(7),
                            p,
                            p.surface,
                        )
                        .padding([10, 12]),
                    );
                }
            }
        }
    }
    col.into()
}

fn seg(c: Color) -> impl Fn(&cosmic::Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(c)),
        border: Border { radius: 2.0.into(), ..Border::default() },
        ..container::Style::default()
    }
}

fn gpu_card<'a>(g: &GpuDetail, p: &Palette, u: masthead_core::config::Units) -> El<'a> {
    let mut col =
        column![row![t(g.name.clone(), 13.5, MEDIUM, p.fg), fill(), t(if g.primary { "in the bar" } else { "" }, 11.0, MONO, p.accent)]]
            .spacing(7);
    if g.suspended {
        col = col.push(t("Suspended. Not queried, so it can stay asleep.", 12.5, SANS, p.fg_muted));
    } else {
        let util = g.util;
        col = col.push(kv("Utilisation", util.map_or_else(|| "not reported".into(), |v| format!("{v:.0}%")), p));
        if let Some(v) = util {
            col = col.push(meter(v / 100.0, p.accent, p, Length::Fill, 6.0));
        }
        if let (Some(used), Some(total)) = (g.vram_used, g.vram_total) {
            col = col.push(kv(
                if g.discrete { "VRAM" } else { "Dedicated memory" },
                format!("{} of {}", format::mem_long(used, u.memory), format::mem_long(total, u.memory)),
                p,
            ));
            col = col.push(meter(used as f64 / total.max(1) as f64, p.accent, p, Length::Fill, 6.0));
        }
        if let Some(w) = g.power_w {
            let label = g.power_limit_w.map_or_else(|| format::watts(w), |l| format!("{} of {}", format::watts(w), format::watts(l)));
            col = col.push(kv("Power", label, p));
            if let Some(l) = g.power_limit_w {
                col = col.push(meter(w / l.max(1.0), p.accent, p, Length::Fill, 6.0));
            }
        }
        if let Some(c) = g.temp_c {
            col = col.push(kv("Temperature", format::temp(c, u.temperature, false), p));
        }
    }
    col = col.push(t(format!("{} · {}", g.driver, g.pci), 11.0, MONO, p.fg_dim));
    boxed(col, *p, p.surface).padding([10, 12]).width(Length::Fill).into()
}

/// The Disk page's way into the disk map and cleanup.
fn disk_usage_card(app: &App) -> El<'_> {
    let p = app.pal;
    let d = &app.disk;
    let note = match (&d.scan, d.scanning()) {
        (_, true) => "Scanning now.".to_owned(),
        (Some(_), false) => format!("{} can be cleaned up.", format::bytes(d.reclaimable())),
        (None, false) => {
            "See which folders and files use the space, and clean up container images, build outputs, caches and old downloads.".to_owned()
        }
    };
    let label = if d.scan.is_some() || d.scanning() { "Open disk usage" } else { "Scan disk usage" };
    boxed(
        row![
            column![t("Disk usage", 14.0, BOLD, p.fg), t(note, 12.0, SANS, p.fg_muted)].spacing(3).width(Length::Fill),
            small_button(label, Some(Message::OpenDiskUsage), p),
        ]
        .spacing(16)
        .align_y(Alignment::Center),
        p,
        p.surface,
    )
    .padding([12, 14])
    .width(Length::Fill)
    .into()
}

// ----------------------------------------------------------------------------
// Processes
// ----------------------------------------------------------------------------

fn processes_page(app: &App, height: f32) -> El<'_> {
    let p = app.pal;
    let u = app.units();
    let mut procs: Vec<ProcDetail> = app.procs.clone();
    app.sort.apply(&mut procs);
    if let Some(order) = &app.held_order {
        model::hold_order(&mut procs, order);
    }
    let mut shown = model::filter_procs(&procs, &app.filter);
    let matched = shown.len();
    shown.truncate(model::rows_that_fit(height));
    let head = row![
        column![
            t("Processes", 21.0, BOLD, p.fg),
            t(
                format!(
                    "{} of {matched} shown · sorted and filtered across the busiest {} · CPU: 100% is one core",
                    shown.len(),
                    procs.len()
                ),
                11.5,
                MONO,
                p.fg_muted
            )
        ]
        .spacing(3),
        fill(),
        kill_status(app),
        cosmic::widget::text_input("Filter by name or user", &app.filter).on_input(Message::Filter).width(240),
    ]
    .spacing(16)
    .align_y(Alignment::Center);

    let arrow = |k: SortKey| if app.sort.key == k { if app.sort.descending { " ↓" } else { " ↑" } } else { "" };
    let sort_head = |label: &str, k: SortKey, w: Option<f32>, right: bool| -> El<'_> {
        let b = button(t(
            format!("{}{}", label.to_uppercase(), arrow(k)),
            10.5,
            MONO_MEDIUM,
            if app.sort.key == k { p.fg } else { p.fg_muted },
        ))
        .on_press(Message::Sort(k))
        .padding(0)
        .class(cosmic::theme::iced::Button::Custom(Box::new(move |_, _| button::Style {
            background: None,
            text_color: p.fg,
            ..button::Style::default()
        })));
        cell(b.into(), w, right)
    };
    let header = row![
        sort_head("Name", SortKey::Name, None, false),
        cell(eyebrow("User", &p), Some(110.0), false),
        sort_head("PID", SortKey::Pid, Some(80.0), true),
        sort_head("CPU", SortKey::Cpu, Some(90.0), true),
        sort_head("Memory", SortKey::Memory, Some(110.0), true),
        cell(Space::new().into(), Some(KILL_COL), true),
    ]
    .spacing(10);
    let cols: Cols<'_> = &[
        ("", None, false),
        ("", Some(110.0), false),
        ("", Some(80.0), true),
        ("", Some(90.0), true),
        ("", Some(110.0), true),
        ("", Some(KILL_COL), true),
    ];
    let mut tbl = column![column![header, swatch(p.border, Length::Fill, 1.0, 0.0)].spacing(5)];
    // Each cell is one text: in software rendering the table's cost is its
    // text count, so it carries no bars and no thread column.
    // Keyed by PID so a row keeps its laid-out text when the order changes;
    // only cells whose text changed are shaped again.
    let rows = shown.into_iter().map(|x| {
        let r = table_row(
            vec![
                t(x.name.clone(), 13.0, SANS, p.fg).into(),
                t(x.user.clone(), 12.5, SANS, p.fg_muted).into(),
                t(x.pid.to_string(), 12.0, MONO, p.fg_muted).into(),
                t(format!("{:.1}%", x.cpu), 12.5, MONO, p.fg).into(),
                t(format::mem_long(x.rss, u.memory), 12.5, MONO, p.fg).into(),
                kill_button(x.pid, app.killing, p),
            ],
            cols,
            &p,
        );
        (x.pid, r)
    });
    tbl = tbl.push(mouse_area(keyed_column(rows)).on_enter(Message::HoldRows(true)).on_exit(Message::HoldRows(false)));
    if procs.is_empty() {
        tbl = tbl.push(container(t("Waiting for the first process scan…", 12.5, SANS, p.fg_muted)).padding([8, 0]));
    }
    column![head, tbl].spacing(16).into()
}

/// What the last kill did, beside the filter so the table does not move.
fn kill_status(app: &App) -> El<'_> {
    let p = app.pal;
    match (&app.kill_result, app.killing) {
        (_, true) => t("Ending the process…", 12.5, SANS, p.fg_muted).into(),
        (Some(Ok(m)), _) => t(m.clone(), 12.5, SANS, p.fg_muted).into(),
        (Some(Err(m)), _) => t(m.clone(), 12.5, SANS, p.danger).into(),
        (None, false) => Space::new().into(),
    }
}

const KILL_COL: f32 = 24.0;

/// The per-row button that opens the kill dialog. Red on hover.
fn kill_button<'a>(pid: u32, busy: bool, p: Palette) -> El<'a> {
    let b = button(icon(icons::handle("kill"), p.fg_muted, 13.0)).padding(3).class(cosmic::theme::iced::Button::Custom(Box::new(
        move |_, status| {
            let hot = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: hot.then_some(Background::Color(Palette::with_alpha(p.danger, 0.15))),
                text_color: p.fg,
                border: Border { radius: 4.0.into(), ..Border::default() },
                ..button::Style::default()
            }
        },
    )));
    let b = if busy { b } else { b.on_press(Message::AskKill(pid)) };
    cosmic::widget::tooltip(b, t("End process…", 12.0, SANS, p.fg), cosmic::widget::tooltip::Position::Left).into()
}

/// Confirmation for ending `target`.
pub fn kill_dialog<'a>(target: &Target) -> El<'a> {
    let Target { pid, name, user, privileged, .. } = target;
    let owner =
        if *privileged { format!("\n\nThis process belongs to {user}. You will be asked for your password.") } else { String::new() };
    let body = format!(
        "{name} (PID {pid}, user {user}) will be asked to quit. Unsaved work in it may be lost. \
         Force kill ends it at once, without letting it clean up.{owner}"
    );
    cosmic::widget::dialog()
        .title(format!("End {name}?"))
        .body(body)
        .primary_action(cosmic::widget::button::destructive("End process").on_press(Message::Kill(Signal::Term)))
        .secondary_action(cosmic::widget::button::standard("Cancel").on_press(Message::CancelKill))
        .tertiary_action(cosmic::widget::button::text("Force kill").on_press(Message::Kill(Signal::Kill)))
        .into()
}

// ----------------------------------------------------------------------------
// Settings
// ----------------------------------------------------------------------------

fn setting<'a>(label: &str, hint: &str, control: El<'a>, p: &Palette) -> El<'a> {
    let mut left = column![t(label.to_owned(), 13.5, MEDIUM, p.fg)].spacing(2);
    if !hint.is_empty() {
        left = left.push(t(hint.to_owned(), 12.0, SANS, p.fg_muted));
    }
    row![left.width(Length::Fill), control].spacing(16).align_y(Alignment::Center).into()
}

fn section<'a>(title: &str, rows: Vec<El<'a>>, p: Palette) -> El<'a> {
    let mut col = column![].spacing(12);
    let n = rows.len();
    for (i, r) in rows.into_iter().enumerate() {
        col = col.push(r);
        if i + 1 < n {
            col = col.push(swatch(p.border, Length::Fill, 1.0, 0.0));
        }
    }
    column![eyebrow(title, &p), boxed(col, p, p.surface).padding([12, 14]).width(Length::Fill)].spacing(8).into()
}

fn small_button<'a>(label: &str, msg: Option<Message>, p: Palette) -> El<'a> {
    let enabled = msg.is_some();
    let mut b = button(t(label.to_owned(), 12.0, MONO_MEDIUM, if enabled { p.fg } else { p.fg_dim })).padding([3, 8]).class(
        cosmic::theme::iced::Button::Custom(Box::new(move |_, status| button::Style {
            background: Some(Background::Color(if matches!(status, button::Status::Hovered) { p.surface3 } else { p.surface2 })),
            text_color: p.fg,
            border: Border { color: p.border, width: 1.0, radius: 4.0.into() },
            border_radius: 4.0.into(),
            ..button::Style::default()
        })),
    );
    if let Some(m) = msg {
        b = b.on_press(m);
    }
    b.into()
}

#[allow(clippy::too_many_lines)] // one form, read top to bottom
fn settings_page(app: &App) -> El<'_> {
    let p = app.pal;
    let Some(cfg) = app.config.as_ref() else {
        return t("Waiting for masthead-daemon…", 13.0, SANS, p.fg_muted).into();
    };
    let head =
        column![t("Settings", 21.0, BOLD, p.fg), t("Changes apply to every Masthead bar immediately", 11.5, MONO, p.fg_muted)].spacing(3);

    // Bar metrics: enabled ones in bar order, then the rest.
    let avail = app.metrics();
    let mut order: Vec<MetricId> = cfg.bar.metrics.iter().copied().filter(|m| avail.contains(m)).collect();
    order.extend(avail.iter().copied().filter(|m| !cfg.bar.metrics.contains(m)));
    let mut bar_rows: Vec<El<'_>> = Vec::new();
    for m in order {
        let on = cfg.bar.metrics.contains(&m);
        let pos = cfg.bar.metrics.iter().position(|x| *x == m);
        let up = pos.filter(|i| *i > 0).map(|_| Message::MoveMetric(m, -1));
        let down = pos.filter(|i| i + 1 < cfg.bar.metrics.len()).map(|_| Message::MoveMetric(m, 1));
        let controls = row![
            small_button("↑", up, p),
            small_button("↓", down, p),
            cosmic::widget::toggler(on).on_toggle(move |v| Message::BarMetric(m, v))
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        let r = row![
            icon(icons::metric(m), if on { p.accent } else { p.fg_dim }, 15.0),
            t(m.title(), 13.5, MEDIUM, if on { p.fg } else { p.fg_muted }),
            fill(),
            controls
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        bar_rows.push(r.into());
    }
    let style_idx = match cfg.bar.style {
        BarStyle::Values => 0,
        BarStyle::Trend => 1,
        BarStyle::Compact => 2,
    };
    bar_rows.push(setting(
        "Style",
        "How each item draws in the bar",
        cosmic::widget::dropdown(&app::STYLE_OPTIONS[..], Some(style_idx), Message::BarStyle).into(),
        &p,
    ));

    let units = vec![
        setting(
            "Memory",
            "",
            cosmic::widget::dropdown(&app::MEM_OPTIONS[..], Some(usize::from(cfg.units.memory == MemoryUnits::Decimal)), Message::MemUnits)
                .into(),
            &p,
        ),
        setting(
            "Network",
            "",
            cosmic::widget::dropdown(&app::NET_OPTIONS[..], Some(usize::from(cfg.units.network == NetworkUnits::Bits)), Message::NetUnits)
                .into(),
            &p,
        ),
        setting(
            "Temperature",
            "",
            cosmic::widget::dropdown(
                &app::TEMP_OPTIONS[..],
                Some(usize::from(cfg.units.temperature == TemperatureUnits::Fahrenheit)),
                Message::TempUnits,
            )
            .into(),
            &p,
        ),
    ];

    let mut thresholds: Vec<El<'_>> = Vec::new();
    for m in THRESHOLD_METRICS {
        let (w, c) = app.thresholds.get(&m).cloned().unwrap_or_default();
        let unit = if m == MetricId::Temp { "°C" } else { "%" };
        let inputs = row![
            t("warn", 11.5, MONO, p.fg_muted),
            cosmic::widget::text_input("–", w)
                .on_input(move |v| Message::ThresholdInput(m, true, v))
                .on_submit(|_| Message::ApplyThresholds)
                .width(70),
            t("critical", 11.5, MONO, p.fg_muted),
            cosmic::widget::text_input("–", c)
                .on_input(move |v| Message::ThresholdInput(m, false, v))
                .on_submit(|_| Message::ApplyThresholds)
                .width(70),
            t(unit, 11.5, MONO, p.fg_muted),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        thresholds.push(setting(m.title(), "", inputs.into(), &p));
    }
    thresholds.push(
        row![
            t("Leave both empty for no threshold. Memory is a share of total.", 12.0, SANS, p.fg_muted),
            fill(),
            small_button("Apply", Some(Message::ApplyThresholds), p)
        ]
        .align_y(Alignment::Center)
        .into(),
    );

    let (gpu_labels, gpu_keys) = app.gpu_choices();
    let gpu_idx = gpu_keys.iter().position(|k| *k == cfg.devices.gpu).unwrap_or(0);
    let (sen_labels, sen_keys) = app.sensor_choices();
    let sen_idx = sen_keys.iter().position(|k| *k == cfg.devices.temperature).unwrap_or(0);
    let devices = vec![
        setting("GPU in the bar", "", cosmic::widget::dropdown(gpu_labels, Some(gpu_idx), Message::Gpu).into(), &p),
        setting("Temperature sensor", "", cosmic::widget::dropdown(sen_labels, Some(sen_idx), Message::TempSensor).into(), &p),
        setting(
            "Count virtual interfaces",
            "docker0, bridges and veth pairs double-count container traffic",
            cosmic::widget::toggler(cfg.devices.include_virtual_interfaces).on_toggle(Message::Virtual).into(),
            &p,
        ),
    ];

    let tray_idx = match cfg.tray.mode {
        TrayMode::Auto => 0,
        TrayMode::Always => 1,
        TrayMode::Never => 2,
    };
    let other = vec![
        setting(
            "Tray icons",
            "For panels without a native Masthead bar",
            cosmic::widget::dropdown(&app::TRAY_OPTIONS[..], Some(tray_idx), Message::Tray).into(),
            &p,
        ),
        setting(
            "Notify when critical",
            "A desktop notification when a metric stays critical",
            cosmic::widget::toggler(cfg.notifications.enabled).on_toggle(Message::Notify).into(),
            &p,
        ),
        setting(
            "Critical for at least",
            "",
            row![
                cosmic::widget::text_input("60", &app.hold).on_input(Message::HoldInput).on_submit(|_| Message::ApplyHold).width(70),
                t("seconds", 11.5, MONO, p.fg_muted),
                small_button("Apply", Some(Message::ApplyHold), p)
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into(),
            &p,
        ),
    ];

    let theme_idx = app::THEMES.iter().position(|t| *t == cfg.window.theme).unwrap_or(0);
    let appearance = vec![setting(
        "Theme",
        "Colors of this window. The bar follows your desktop.",
        cosmic::widget::dropdown(&app::THEME_OPTIONS[..], Some(theme_idx), Message::Theme).into(),
        &p,
    )];

    column![
        head,
        section("Appearance", appearance, p),
        section("Bar", bar_rows, p),
        section("Units", units, p),
        section("Thresholds", thresholds, p),
        section("Devices", devices, p),
        section("Tray and alerts", other, p),
        section(
            "Masthead",
            vec![setting(
                "Quit Masthead",
                "Removes the bar items and stops monitoring until you open Masthead again or log in",
                small_button("Quit Masthead", Some(Message::AskQuitAll), p),
                &p,
            )],
            p,
        )
    ]
    .spacing(18)
    .max_width(760)
    .into()
}
