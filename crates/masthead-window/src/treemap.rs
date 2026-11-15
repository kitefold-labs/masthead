// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! The disk map: a squarified treemap of one folder, two levels deep.
//!
//! Each child of the folder is a rectangle sized by its share of the folder.
//! Folders show their own largest children inside, outlined, so the map
//! reads like a floor plan. Clicking a folder opens it.
//!
//! The tiles are painted into a [`bitmap`] once per folder and size, the
//! labels are drawn over it, and a canvas on top takes the pointer and draws
//! the hover outline.

use crate::bitmap::{self, solid};
use crate::labels::{self, Label};
use crate::palette::Palette;
use cosmic::Element;
use cosmic::iced::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use cosmic::iced::widget::stack;
use cosmic::iced::{Color, Font, Length, Point, Rectangle, Size, alignment, mouse};
use cosmic::{Renderer, Theme};
use masthead_disk::{Kind, Node};
use std::collections::HashSet;
use tiny_skia::{FillRule, PathBuilder, Pixmap, Transform};

/// At most this many rectangles per level; the rest are too small to see.
pub const MAX_TILES: usize = 80;
pub const INNER_TILES: usize = 24;
const GAP: f32 = 2.0;
const HEADER: f32 = 18.0;

/// Lays out `sizes` (largest first) in `r` with the squarified algorithm
/// (Bruls, Huizing and van Wijk), returning one rectangle per size.
pub fn squarify(sizes: &[u64], r: Rectangle) -> Vec<Rectangle> {
    let total: f64 = sizes.iter().map(|&s| s as f64).sum();
    if total <= 0.0 || r.width <= 0.0 || r.height <= 0.0 {
        return Vec::new();
    }
    let scale = f64::from(r.width) * f64::from(r.height) / total;
    let areas: Vec<f64> = sizes.iter().map(|&s| s as f64 * scale).collect();
    let mut out = Vec::with_capacity(areas.len());
    let mut free = r;
    let mut i = 0;
    while i < areas.len() {
        let side = f64::from(free.width.min(free.height));
        // Grow the row while its worst aspect ratio improves.
        let mut end = i + 1;
        let mut best = worst(&areas[i..end], side);
        while end < areas.len() {
            let w = worst(&areas[i..=end], side);
            if w > best {
                break;
            }
            best = w;
            end += 1;
        }
        let row = &areas[i..end];
        let sum: f64 = row.iter().sum();
        let thick = (sum / side) as f32;
        let mut offset = 0.0_f32;
        let horizontal = free.width >= free.height;
        for a in row {
            let len = (*a / f64::from(thick.max(f32::EPSILON))) as f32;
            out.push(if horizontal {
                Rectangle { x: free.x, y: free.y + offset, width: thick, height: len }
            } else {
                Rectangle { x: free.x + offset, y: free.y, width: len, height: thick }
            });
            offset += len;
        }
        if horizontal {
            free.x += thick;
            free.width = (free.width - thick).max(0.0);
        } else {
            free.y += thick;
            free.height = (free.height - thick).max(0.0);
        }
        i = end;
    }
    out
}

fn worst(row: &[f64], side: f64) -> f64 {
    let sum: f64 = row.iter().sum();
    let (min, max) = row.iter().fold((f64::MAX, 0.0_f64), |(lo, hi), &a| (lo.min(a), hi.max(a)));
    let s2 = side * side;
    let sum2 = sum * sum;
    (s2 * max / sum2).max(sum2 / (s2 * min))
}

/// Rectangles for a node's visible children, inset by the gap.
fn tiles(node: &Node, r: Rectangle, limit: usize) -> Vec<(usize, Rectangle)> {
    let kids: Vec<(usize, u64)> =
        node.children.iter().enumerate().filter(|(_, c)| c.size > 0).take(limit).map(|(i, c)| (i, c.size)).collect();
    let sizes: Vec<u64> = kids.iter().map(|k| k.1).collect();
    squarify(&sizes, r)
        .into_iter()
        .zip(kids)
        .map(|(rect, (i, _))| {
            (
                i,
                Rectangle {
                    x: rect.x + GAP / 2.0,
                    y: rect.y + GAP / 2.0,
                    width: (rect.width - GAP).max(0.0),
                    height: (rect.height - GAP).max(0.0),
                },
            )
        })
        .collect()
}

/// Whether a tile is big enough for its name and size.
fn labelled(r: Rectangle) -> bool {
    r.width >= 54.0 && r.height >= 30.0
}

pub struct Treemap<'a, M> {
    pub node: &'a Node,
    pub pal: Palette,
    /// Child indices of `node` that are caches, drawn in the warning color.
    pub caches: HashSet<usize>,
    /// (child, grandchild) pairs that are caches, for the inner tiles.
    pub inner_caches: HashSet<(usize, usize)>,
    /// The painted tiles.
    pub tiles: &'a bitmap::Cache,
    pub mono: Font,
    pub sans: Font,
    /// Message for a click on child `i` of `node`.
    pub on_open: Box<dyn Fn(usize) -> Option<M> + 'a>,
    pub format: fn(u64) -> String,
}

impl<'a, M: 'a> Treemap<'a, M> {
    pub fn view(self, height: f32) -> Element<'a, M> {
        let paint = Tiles { node: self.node, pal: self.pal, caches: self.caches.clone(), inner_caches: self.inner_caches.clone() };
        let painted = bitmap::bitmap(self.tiles, move |px, size, tf| paint.paint(px, size, tf)).width(Length::Fill).height(height);
        let (node, pal, sans, mono, format) = (self.node, self.pal, self.sans, self.mono, self.format);
        let names = labels::labels(move |size| {
            let mut out = Vec::new();
            for (i, r) in tiles(node, Rectangle::with_size(size), MAX_TILES) {
                if !labelled(r) {
                    continue;
                }
                let c = &node.children[i];
                let max_chars = ((r.width - 10.0) / 6.6) as usize;
                let at = |content: String, dy: f32, color: Color, size: f32, font: Font| Label {
                    content,
                    position: Point::new(r.x + 5.0, r.y + 4.0 + dy),
                    color,
                    size,
                    font,
                    align_x: alignment::Horizontal::Left,
                    align_y: alignment::Vertical::Top,
                };
                out.push(at(ellipsize(&c.name, max_chars), 0.0, pal.fg, 11.5, sans));
                out.push(at(format(c.size), 14.0, pal.fg_muted, 10.5, mono));
            }
            out
        })
        .width(Length::Fill)
        .height(height);
        let over = canvas::Canvas::new(self).width(Length::Fill).height(height);
        stack![painted, names, over].into()
    }
}

#[derive(Debug, Default)]
pub struct State {
    hovered: Option<usize>,
}

impl<M> canvas::Program<M, Theme, Renderer> for Treemap<'_, M> {
    type State = State;

    fn update(&self, state: &mut State, event: &canvas::Event, bounds: Rectangle, cursor: mouse::Cursor) -> Option<canvas::Action<M>> {
        let hit = cursor.position_in(bounds).and_then(|p| {
            tiles(self.node, Rectangle::with_size(bounds.size()), MAX_TILES).into_iter().find(|(_, r)| r.contains(p)).map(|(i, _)| i)
        });
        match event {
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. } | mouse::Event::CursorLeft) if hit != state.hovered => {
                state.hovered = hit;
                Some(canvas::Action::request_redraw())
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                hit.and_then(|i| (self.on_open)(i)).map(|m| canvas::Action::publish(m).and_capture())
            }
            _ => None,
        }
    }

    /// Only the hover outline; nothing while nothing is hovered, so the
    /// renderer has no live geometry to redraw each frame.
    fn draw(
        &self,
        state: &State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry<Renderer>> {
        let Some(i) = state.hovered else { return Vec::new() };
        let Some((_, r)) = tiles(self.node, Rectangle::with_size(bounds.size()), MAX_TILES).into_iter().find(|(j, _)| *j == i) else {
            return Vec::new();
        };
        let mut over = Frame::new(renderer, bounds.size());
        over.stroke(&Path::rectangle(r.position(), r.size()), Stroke::default().with_color(self.pal.fg).with_width(1.5));
        vec![over.into_geometry()]
    }

    fn mouse_interaction(&self, state: &State, _bounds: Rectangle, _cursor: mouse::Cursor) -> mouse::Interaction {
        match state.hovered {
            Some(i) if self.node.children.get(i).is_some_and(Node::is_dir) => mouse::Interaction::Pointer,
            _ => mouse::Interaction::default(),
        }
    }
}

/// Fill and outline for a tile.
fn colors(kind: Kind, cache: bool, p: &Palette) -> (Color, Color) {
    let base = match kind {
        _ if cache => p.warning,
        Kind::Unscanned => p.danger,
        Kind::Other => p.fg_dim,
        Kind::File => p.fg_muted,
        _ => p.accent,
    };
    let fill_alpha = match kind {
        Kind::Other | Kind::Unscanned => 0.10,
        Kind::File => 0.16,
        _ => 0.20,
    };
    (Palette::with_alpha(base, fill_alpha), Palette::with_alpha(base, 0.55))
}

/// What the tiles bitmap is painted from.
struct Tiles<'a> {
    node: &'a Node,
    pal: Palette,
    caches: HashSet<usize>,
    inner_caches: HashSet<(usize, usize)>,
}

impl Tiles<'_> {
    fn paint(&self, px: &mut Pixmap, size: Size, tf: Transform) {
        for (i, r) in tiles(self.node, Rectangle::with_size(size), MAX_TILES) {
            self.tile(px, tf, i, &self.node.children[i], r, self.caches.contains(&i));
        }
    }

    fn tile(&self, px: &mut Pixmap, tf: Transform, i: usize, c: &Node, r: Rectangle, cache: bool) {
        if r.width < 1.0 || r.height < 1.0 {
            return;
        }
        let p = &self.pal;
        let (fill, line) = colors(c.kind, cache, p);
        if let Some(path) = rounded(r, 3.0) {
            px.fill_path(&path, &solid(fill), FillRule::Winding, tf, None);
            px.stroke_path(&path, &solid(line), &tiny_skia::Stroke::default(), tf, None);
        }
        // A folder shows its own largest items below its label.
        if c.is_dir() && !c.children.is_empty() {
            let top = if labelled(r) { HEADER + 16.0 } else { 2.0 };
            let inner = Rectangle { x: r.x + 3.0, y: r.y + top, width: r.width - 6.0, height: r.height - top - 3.0 };
            if inner.width >= 8.0 && inner.height >= 8.0 {
                for (j, ir) in tiles(c, inner, INNER_TILES) {
                    if ir.width < 2.0 || ir.height < 2.0 {
                        continue;
                    }
                    let inner_cache = self.inner_caches.contains(&(i, j));
                    let (f, l) = colors(c.children[j].kind, inner_cache, p);
                    let Some(rect) = tiny_skia::Rect::from_xywh(ir.x, ir.y, ir.width, ir.height) else { continue };
                    let ip = PathBuilder::from_rect(rect);
                    if inner_cache {
                        px.fill_path(&ip, &solid(f), FillRule::Winding, tf, None);
                    }
                    let outline = Palette::with_alpha(l, if inner_cache { 0.8 } else { 0.35 });
                    px.stroke_path(&ip, &solid(outline), &tiny_skia::Stroke::default(), tf, None);
                }
            }
        }
    }
}

/// A rectangle with rounded corners, inset half a pixel so its 1 px outline
/// lands on whole pixels.
fn rounded(r: Rectangle, radius: f32) -> Option<tiny_skia::Path> {
    let (x, y, w, h) = (r.x + 0.5, r.y + 0.5, r.width - 1.0, r.height - 1.0);
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let rad = radius.min(w / 2.0).min(h / 2.0);
    // Cubic Bézier circle approximation.
    let k = rad * 0.552_28;
    let mut b = PathBuilder::new();
    b.move_to(x + rad, y);
    b.line_to(x + w - rad, y);
    b.cubic_to(x + w - rad + k, y, x + w, y + rad - k, x + w, y + rad);
    b.line_to(x + w, y + h - rad);
    b.cubic_to(x + w, y + h - rad + k, x + w - rad + k, y + h, x + w - rad, y + h);
    b.line_to(x + rad, y + h);
    b.cubic_to(x + rad - k, y + h, x, y + h - rad + k, x, y + h - rad);
    b.line_to(x, y + rad);
    b.cubic_to(x, y + rad - k, x + rad - k, y, x + rad, y);
    b.close();
    b.finish()
}

fn ellipsize(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_owned()
    } else if max <= 1 {
        String::new()
    } else {
        let mut out: String = s.chars().take(max - 1).collect();
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn squarify_fills_the_area_without_overlap() {
        let r = Rectangle { x: 0.0, y: 0.0, width: 600.0, height: 400.0 };
        let sizes = [6, 6, 4, 3, 2, 2, 1];
        let rs = squarify(&sizes, r);
        assert_eq!(rs.len(), sizes.len());
        let area: f32 = rs.iter().map(|r| r.width * r.height).sum();
        assert!((area - 240_000.0).abs() < 1.0, "{area}");
        for (i, a) in rs.iter().enumerate() {
            assert!(a.x >= -0.01 && a.y >= -0.01 && a.x + a.width <= 600.01 && a.y + a.height <= 400.01);
            // Area proportional to size.
            let want = 240_000.0 * sizes[i] as f32 / 24.0;
            assert!((a.width * a.height - want).abs() < 1.0);
            for b in &rs[i + 1..] {
                let overlap_w = (a.x + a.width).min(b.x + b.width) - a.x.max(b.x);
                let overlap_h = (a.y + a.height).min(b.y + b.height) - a.y.max(b.y);
                assert!(overlap_w <= 0.01 || overlap_h <= 0.01);
            }
        }
        assert_eq!(squarify(&[], r).len(), 0);
        assert_eq!(squarify(&[0, 0], r).len(), 0);
    }

    #[test]
    fn ellipsis() {
        assert_eq!(ellipsize("node_modules", 20), "node_modules");
        assert_eq!(ellipsize("node_modules", 5), "node…");
    }

    #[test]
    fn paints_tiles() {
        let mut dir = Node::new("home", Kind::Dir);
        dir.children = (0..30).map(|i| Node::file(format!("f{i}"), 1000 / (i + 1), 0)).collect();
        let mut root = Node::new("/", Kind::Mount);
        root.children = vec![dir, Node::file("big.iso", 5000, 0), Node::new("x", Kind::Unscanned)];
        root.children[2].size = 300;
        root.sum_children(0);
        let t = Tiles { node: &root, pal: crate::palette::DARK, caches: HashSet::from([0]), inner_caches: HashSet::from([(0, 1)]) };
        let mut px = Pixmap::new(400, 200).unwrap();
        t.paint(&mut px, Size::new(200.0, 100.0), Transform::from_scale(2.0, 2.0));
        assert!(px.pixels().iter().any(|p| p.alpha() > 0));
        assert!(rounded(Rectangle { x: 0.0, y: 0.0, width: 0.5, height: 10.0 }, 3.0).is_none());
    }
}
