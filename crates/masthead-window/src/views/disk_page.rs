// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! The Disk usage page: a map of what uses the disk, and a list of what can
//! be cleaned up.

use super::{
    BOLD, Cols, El, MEDIUM, MONO, MONO_MEDIUM, SANS, boxed, chip, fill, icon, meter, small_button, space_color, swatch, t, table_row,
};
use crate::app::{App, Message};
use crate::disk::{Confirm, DiskMsg, DockerState, Tab};
use crate::icons;
use crate::palette::Palette;
use crate::treemap::Treemap;
use cosmic::iced::widget::{Space, button, canvas, column, container, row, scrollable};
use cosmic::iced::{Alignment, Background, Border, Color, Length, Padding};
use masthead_core::format::bytes;
use masthead_disk::findings::{Group, GroupId};
use masthead_disk::{Kind, Node, time};
use std::collections::HashSet;
use std::sync::atomic::Ordering::Relaxed;

/// Rows a group shows before "Show all".
const COLLAPSED_ROWS: usize = 8;
/// Rows the map's list shows.
const LIST_ROWS: usize = 60;

fn msg(m: DiskMsg) -> Message {
    Message::Disk(m)
}

pub fn page(app: &App) -> El<'_> {
    let p = app.pal;
    let d = &app.disk;
    let status: El<'_> = if let Some(pr) = &d.progress {
        let current = pr.current.lock().map(|c| c.clone()).unwrap_or_default();
        column![
            t(
                format!(
                    "Scanning · {} files · {} · {} folders not readable",
                    group_digits(pr.files.load(Relaxed)),
                    bytes(pr.bytes.load(Relaxed)),
                    group_digits(pr.denied.load(Relaxed))
                ),
                11.5,
                MONO,
                p.fg_muted
            ),
            t(current, 11.0, MONO, p.fg_dim).wrapping(cosmic::iced::widget::text::Wrapping::None),
        ]
        .spacing(2)
        .into()
    } else if let Some(s) = &d.scan {
        t(
            format!(
                "{} files in {:.0} s · scanned {} ago · only what your user can read",
                group_digits(s.tree.files),
                s.elapsed.as_secs_f64().max(1.0),
                time::age(d.scanned_at, time::now())
            ),
            11.5,
            MONO,
            p.fg_muted,
        )
        .into()
    } else {
        t("Find what uses your disk, and clean up containers, build outputs, caches and old downloads.", 11.5, SANS, p.fg_muted).into()
    };
    let action = if d.scanning() {
        small_button("Stop", Some(msg(DiskMsg::Stop)), p)
    } else {
        small_button(if d.scan.is_some() { "Scan again" } else { "Scan disk" }, (!d.busy).then(|| msg(DiskMsg::Scan)), p)
    };
    let head = row![column![t("Disk usage", 21.0, BOLD, p.fg), status].spacing(3).width(Length::Fill), action]
        .spacing(16)
        .align_y(Alignment::Start);

    // Rows that come and go stay inside `top`, so the scrollable below keeps
    // its place among the page's children and with it its scroll position.
    let mut top = column![head].spacing(14);
    if !d.space.is_empty() {
        top = top.push(space(app));
    }
    if let Some(m) = &d.message {
        top = top.push(match m {
            Ok(s) => t(s.clone(), 12.5, SANS, p.success),
            Err(s) => t(s.clone(), 12.5, SANS, p.danger),
        });
    }
    let Some(scan) = &d.scan else {
        return top.into();
    };
    let mut col = column![top].spacing(14);
    let reclaim = d.reclaimable();
    let mut tabs = row![chip("Map", msg(DiskMsg::Tab(Tab::Map)), p, d.tab == Tab::Map)].spacing(2);
    tabs = tabs.push(chip(&format!("Clean up · {}", bytes(reclaim)), msg(DiskMsg::Tab(Tab::Cleanup)), p, d.tab == Tab::Cleanup));
    let tabs = container(tabs).padding(2).style(move |_| container::Style {
        background: Some(Background::Color(p.surface2)),
        border: Border { color: p.border, width: 1.0, radius: 6.0.into() },
        ..container::Style::default()
    });
    col = col.push(tabs);
    let body = match d.tab {
        Tab::Map => map(app, &scan.tree),
        Tab::Cleanup => cleanup(app),
    };
    // Room on the right for the scrollbar.
    col.push(scrollable(container(body).padding(Padding { right: 12.0, ..Padding::ZERO })).height(Length::Fill)).height(Length::Fill).into()
}

/// Used and free space on each local filesystem.
fn space(app: &App) -> El<'_> {
    let p = app.pal;
    let mut col = column![].spacing(8);
    for (mount, s) in &app.disk.space {
        let frac = s.used as f64 / s.total as f64;
        col = col.push(
            column![
                row![
                    t(mount.to_string_lossy().into_owned(), 13.0, MONO_MEDIUM, p.fg),
                    fill(),
                    t(format!("{} used · {} free of {}", bytes(s.used), bytes(s.free), bytes(s.total)), 12.5, MONO, p.fg)
                ]
                .align_y(Alignment::Center),
                meter(frac, space_color(app, frac), &p, Length::Fill, 6.0)
            ]
            .spacing(5),
        );
    }
    col.into()
}

// ----------------------------------------------------------------------------
// Map
// ----------------------------------------------------------------------------

#[allow(clippy::too_many_lines)] // breadcrumb, map, legend and list, top to bottom
fn map<'a>(app: &'a App, tree: &'a Node) -> El<'a> {
    let p = app.pal;
    let d = &app.disk;
    let Some(node) = tree.at(&d.path) else { return Space::new().into() };

    // Breadcrumb: every ancestor is a link.
    let mut crumbs = row![].spacing(4).align_y(Alignment::Center);
    let mut n = tree;
    for depth in 0..=d.path.len() {
        if depth > 0 {
            n = &n.children[d.path[depth - 1]];
            crumbs = crumbs.push(t("›", 13.0, SANS, p.fg_dim));
        }
        let here = depth == d.path.len();
        let label = t(n.name.to_string(), 13.0, if here { MEDIUM } else { SANS }, if here { p.fg } else { p.accent });
        crumbs = crumbs.push(if here { El::from(label) } else { link(label.into(), msg(DiskMsg::Go(d.path[..depth].to_vec()))) });
    }
    let summary = t(
        format!("{} · {} files · last change {} ago", bytes(node.size), group_digits(node.files), time::age(node.newest, time::now())),
        11.5,
        MONO,
        p.fg_muted,
    );

    // Children of this folder that the Cleanup tab lists.
    let here = tree.fs_path(&d.path);
    let is_cache = |p: &std::path::Path| d.cache_paths.contains(p);
    let mut caches: HashSet<usize> = HashSet::new();
    let mut inner_caches: HashSet<(usize, usize)> = HashSet::new();
    if let Some(h) = &here {
        for (i, c) in node.children.iter().enumerate().take(crate::treemap::MAX_TILES) {
            let cp = h.join(&*c.name);
            if is_cache(&cp) {
                caches.insert(i);
            } else if c.is_dir() {
                for (j, g) in c.children.iter().enumerate().take(crate::treemap::INNER_TILES) {
                    if is_cache(&cp.join(&*g.name)) {
                        inner_caches.insert((i, j));
                    }
                }
            }
        }
    }
    let path = d.path.clone();
    let tm = Treemap {
        node,
        pal: p,
        caches: caches.clone(),
        inner_caches,
        cache: &d.map_cache,
        mono: MONO,
        sans: SANS,
        on_open: Box::new(move |i| {
            let mut to = path.clone();
            to.push(i);
            Some(msg(DiskMsg::Go(to)))
        }),
        format: bytes,
    };
    let map_el: El<'_> = canvas(tm).width(Length::Fill).height(340).into();

    let legend = row![
        legend_item("Folder", p.accent, &p),
        legend_item("File", p.fg_muted, &p),
        legend_item("Can be cleaned up", p.warning, &p),
        legend_item("Smaller items", p.fg_dim, &p),
        legend_item("Not readable", p.danger, &p),
    ]
    .spacing(14);

    // Ranked list.
    let cols: Cols<'_> = &[
        ("Name", None, false),
        ("", Some(140.0), false),
        ("Size", Some(90.0), true),
        ("Changed", Some(100.0), true),
        ("", Some(64.0), true),
    ];
    let mut tbl = column![super::table_head(cols, &p)];
    let now = time::now();
    for (i, c) in node.children.iter().enumerate().take(LIST_ROWS) {
        let mut to = d.path.clone();
        to.push(i);
        let fs = tree.fs_path(&to);
        let glyph = if c.is_dir() { "▸ " } else { "  " };
        let color = match c.kind {
            Kind::Unscanned => p.danger,
            Kind::Other => p.fg_muted,
            _ if caches.contains(&i) => p.warning,
            _ => p.fg,
        };
        let name = t(format!("{glyph}{}", c.name), 13.0, SANS, color);
        let name: El<'_> = if c.is_dir() { link(name.into(), msg(DiskMsg::Go(to.clone()))) } else { name.into() };
        let frac = if node.size > 0 { c.size as f64 / node.size as f64 } else { 0.0 };
        let share = row![
            meter(frac, if caches.contains(&i) { p.warning } else { p.accent }, &p, Length::Fixed(90.0), 5.0),
            t(format!("{:>3.0}%", frac * 100.0), 11.0, MONO, p.fg_muted)
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        let changed = if c.newest > 0 { time::age(c.newest, now) } else { String::new() };
        let mut actions = row![].spacing(2);
        if let Some(fs) = fs.filter(|_| matches!(c.kind, Kind::Dir | Kind::File)) {
            let trash = d.trashable(&fs);
            actions = actions.push(icon_button("folder", msg(DiskMsg::ShowInFiles(fs)), false, p));
            if trash {
                actions = actions.push(icon_button("trash", msg(DiskMsg::AskTrash(to.clone())), true, p));
            }
        }
        tbl = tbl.push(table_row(
            vec![name, share.into(), t(bytes(c.size), 12.5, MONO, p.fg).into(), t(changed, 12.0, MONO, p.fg_muted).into(), actions.into()],
            cols,
            &p,
        ));
    }
    if node.children.len() > LIST_ROWS {
        let rest: u64 = node.children[LIST_ROWS..].iter().map(|c| c.size).sum();
        tbl = tbl.push(
            container(t(format!("{} more items, {}", node.children.len() - LIST_ROWS, bytes(rest)), 12.0, SANS, p.fg_muted))
                .padding([6, 0]),
        );
    }
    if node.children.is_empty() {
        tbl = tbl.push(container(t("Nothing here is large enough to list.", 12.5, SANS, p.fg_muted)).padding([8, 0]));
    }

    column![row![crumbs, fill(), summary].align_y(Alignment::Center), map_el, legend, tbl].spacing(10).into()
}

fn legend_item<'a>(label: &str, c: Color, p: &Palette) -> El<'a> {
    row![
        container(Space::new().width(10).height(10)).style(move |_| container::Style {
            background: Some(Background::Color(Palette::with_alpha(c, 0.3))),
            border: Border { color: Palette::with_alpha(c, 0.7), width: 1.0, radius: 2.0.into() },
            ..container::Style::default()
        }),
        t(label.to_owned(), 11.5, SANS, p.fg_muted)
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .into()
}

/// Text that acts as a link.
fn link(content: El<'_>, m: Message) -> El<'_> {
    button(content).on_press(m).padding(0).class(cosmic::theme::iced::Button::Custom(Box::new(|_, _| button::Style::default()))).into()
}

fn icon_button<'a>(name: &str, m: Message, danger: bool, p: Palette) -> El<'a> {
    button(icon(icons::handle(name), p.fg_muted, 14.0))
        .on_press(m)
        .padding(4)
        .class(cosmic::theme::iced::Button::Custom(Box::new(move |_, status| {
            let hot = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: hot.then_some(Background::Color(if danger { Palette::with_alpha(p.danger, 0.15) } else { p.surface3 })),
                text_color: p.fg,
                border: Border { radius: 4.0.into(), ..Border::default() },
                ..button::Style::default()
            }
        })))
        .into()
}

// ----------------------------------------------------------------------------
// Cleanup
// ----------------------------------------------------------------------------

fn cleanup(app: &App) -> El<'_> {
    let p = app.pal;
    let d = &app.disk;
    let mut col = column![].spacing(16);
    let engine = d.docker.as_ref().map_or("Docker", |x| x.engine);
    match &d.docker_state {
        DockerState::Loading => col = col.push(t(format!("Asking {engine} for image, container and cache sizes…"), 12.5, SANS, p.fg_muted)),
        DockerState::Failed(e) => col = col.push(t(format!("{engine}: {e}"), 12.5, SANS, p.warning)),
        DockerState::Missing => {
            col = col.push(t("No Docker or Podman socket was found, so container images are not listed.", 12.5, SANS, p.fg_muted));
        }
        DockerState::Ready | DockerState::NotAsked => {}
    }
    if let Some(c) = &d.cleaning {
        col = col.push(t(format!("Cleaning up {} of {}…", c.current(), c.total), 12.5, SANS, p.fg_muted));
    } else if d.busy {
        col = col.push(t("Cleaning up…", 12.5, SANS, p.fg_muted));
    }
    for g in d.all_groups() {
        col = col.push(group(app, g));
    }
    if d.all_groups().next().is_none() && d.docker_state != DockerState::Loading {
        col = col.push(t("Nothing large enough to clean up was found.", 12.5, SANS, p.fg_muted));
    }
    col.into()
}

fn group<'a>(app: &'a App, g: &'a Group) -> El<'a> {
    let p = app.pal;
    let d = &app.disk;
    let chosen: Vec<&masthead_disk::findings::Item> = g.items.iter().filter(|i| d.selected.contains(&i.action)).collect();
    let chosen_size: u64 = chosen.iter().map(|i| i.size).sum();
    let any_stale = g.items.iter().any(|i| i.stale);
    let clean_label =
        if chosen.is_empty() { "Clean up".to_owned() } else { format!("Clean up {} · {}", chosen.len(), bytes(chosen_size)) };
    let head = row![
        column![
            row![
                t(g.title, 15.0, BOLD, p.fg),
                t(format!("  {} in {}", bytes(g.size()), plural(g.items.len(), "item")), 12.0, MONO, p.fg_muted)
            ]
            .align_y(Alignment::Center),
            t(g.about.clone(), 12.0, SANS, p.fg_muted),
        ]
        .spacing(3)
        .width(Length::Fill),
        small_button("Select stale", any_stale.then(|| msg(DiskMsg::SelectStale(g.id))), p),
        small_button("Clear", (!chosen.is_empty()).then(|| msg(DiskMsg::ClearGroup(g.id))), p),
        small_button(&clean_label, (!chosen.is_empty() && !d.busy).then(|| msg(DiskMsg::AskClean(g.id))), p),
    ]
    .spacing(8)
    .align_y(Alignment::Start);

    let expanded = d.expanded.contains(&g.id);
    let mut rows = column![].spacing(0);
    for i in g.items.iter().take(if expanded { usize::MAX } else { COLLAPSED_ROWS }) {
        let on = d.selected.contains(&i.action);
        let action = i.action.clone();
        let check = cosmic::iced::widget::checkbox(on).on_toggle(move |v| msg(DiskMsg::Toggle(action.clone(), v))).size(15);
        let mut title = row![t(i.title.clone(), 13.0, MEDIUM, p.fg)].spacing(8).align_y(Alignment::Center);
        if i.stale {
            title = title.push(badge("stale", p.warning));
        }
        let r = row![
            check,
            column![title, t(i.detail.clone(), 11.5, SANS, p.fg_muted)].spacing(2).width(Length::Fill),
            t(bytes(i.size), 12.5, MONO, p.fg)
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        rows = rows.push(column![container(r).padding([7, 0]), swatch(p.border, Length::Fill, 1.0, 0.0)]);
    }
    if g.items.len() > COLLAPSED_ROWS {
        let label = if expanded { "Show fewer".to_owned() } else { format!("Show all {}", g.items.len()) };
        rows = rows.push(container(small_button(&label, Some(msg(DiskMsg::Expand(g.id))), p)).padding([8, 0, 0, 0]));
    }
    boxed(column![head, rows].spacing(10), p, p.surface).padding([12, 14]).width(Length::Fill).into()
}

fn badge<'a>(label: &str, c: Color) -> El<'a> {
    container(t(label.to_owned(), 10.0, MONO_MEDIUM, c))
        .padding([1, 5])
        .style(move |_| container::Style {
            background: Some(Background::Color(Palette::with_alpha(c, 0.12))),
            border: Border { radius: 3.0.into(), ..Border::default() },
            ..container::Style::default()
        })
        .into()
}

// ----------------------------------------------------------------------------
// Confirmation
// ----------------------------------------------------------------------------

pub fn dialog(app: &App) -> Option<El<'_>> {
    let c = app.disk.confirm.as_ref()?;
    let (title, body, verb) = match c {
        Confirm::Trash { path, name, size, dir } => (
            format!("Move {name} to the Trash?"),
            format!(
                "{} ({}{}) goes to the Trash. You can restore it from there. The space is freed when the Trash is emptied.",
                path.display(),
                bytes(*size),
                if *dir { ", with everything in it" } else { "" }
            ),
            "Move to Trash",
        ),
        Confirm::Clean { group, items } => {
            let total: u64 = items.iter().map(|i| i.2).sum();
            let mut names: Vec<String> = items.iter().take(5).map(|i| format!("• {}", i.1)).collect();
            if items.len() > 5 {
                names.push(format!("…and {} more", items.len() - 5));
            }
            let how = match group {
                GroupId::DockerImages => "The images are removed from Docker. Pulling or building them again restores them.",
                GroupId::DockerBuildCache => "The build cache is pruned. Your next build may take longer.",
                GroupId::DockerContainers => "The containers are removed, with any changes made inside them.",
                GroupId::DockerVolumes => "The volumes and the data in them are deleted for good.",
                GroupId::BuildOutputs => "The folders are deleted for good. Your next build or install recreates them.",
                GroupId::AppCaches => "The caches are deleted for good. Applications rebuild them as needed.",
                GroupId::Toolchains => "The toolchains are uninstalled with rustup. rustup can install them again.",
                GroupId::Trash => "Everything in the Trash is deleted for good.",
                GroupId::Downloads => "The files go to the Trash. Empty it to free the space.",
            };
            let n = items.len();
            let amount = if matches!(group, GroupId::Downloads) {
                format!("{} in total.", bytes(total))
            } else {
                format!("About {} will be freed.", bytes(total))
            };
            (
                format!("Clean up {}?", plural(n, "item")),
                format!("{}\n\n{how} {amount}", names.join("\n")),
                if matches!(group, GroupId::Downloads) { "Move to Trash" } else { "Clean up" },
            )
        }
    };
    Some(
        cosmic::widget::dialog()
            .title(title)
            .body(body)
            .primary_action(cosmic::widget::button::destructive(verb).on_press(msg(DiskMsg::Confirm)))
            .secondary_action(cosmic::widget::button::standard("Cancel").on_press(msg(DiskMsg::Dismiss)))
            .into(),
    )
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 { format!("1 {word}") } else { format!("{n} {word}s") }
}

/// 6020365 → "6,020,365".
fn group_digits(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits() {
        assert_eq!(group_digits(6_020_365), "6,020,365");
        assert_eq!(group_digits(999), "999");
        assert_eq!(group_digits(1000), "1,000");
    }
}
