// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! Charts. The lines and areas are painted into a [`bitmap`] that is kept
//! until a sample arrives; the chart's labels are drawn over it.

use crate::bitmap::{self, solid};
use crate::labels::{self, Label};
use crate::model::Series;
use crate::palette::Palette;
use cosmic::Element;
use cosmic::iced::widget::stack;
use cosmic::iced::{Length, Point, Rectangle, Size, alignment};
use tiny_skia::{FillRule, GradientStop, LinearGradient, PathBuilder, Pixmap, SpreadMode, Stroke, StrokeDash, Transform};

/// Points of one series mapped to the plot, split at gaps.
fn segments(values: &[f64], slots: usize, max: f64, plot: Rectangle) -> Vec<Vec<Point>> {
    let mut out: Vec<Vec<Point>> = Vec::new();
    let mut cur: Vec<Point> = Vec::new();
    let n = values.len();
    let slots = slots.max(n).max(2);
    for (i, v) in values.iter().enumerate() {
        if !v.is_finite() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            continue;
        }
        // Right-align: the newest sample sits at the right edge.
        let slot = slots - n + i;
        let x = plot.x + plot.width * slot as f32 / (slots - 1) as f32;
        let frac = if max > 0.0 { (v / max).clamp(0.0, 1.0) } else { 0.0 };
        let y = plot.y + plot.height * (1.0 - frac as f32);
        cur.push(Point::new(x, y));
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn line(points: &[Point]) -> Option<tiny_skia::Path> {
    let mut b = PathBuilder::new();
    for (i, p) in points.iter().enumerate() {
        if i == 0 { b.move_to(p.x, p.y) } else { b.line_to(p.x, p.y) }
    }
    b.finish()
}

fn area(points: &[Point], floor: f32) -> Option<tiny_skia::Path> {
    let (first, last) = (points.first()?, points.last()?);
    let mut b = PathBuilder::new();
    b.move_to(first.x, floor);
    for p in points {
        b.line_to(p.x, p.y);
    }
    b.line_to(last.x, floor);
    b.close();
    b.finish()
}

/// The plot area of a chart of `size`: room on the right for labels, below
/// for the time axis.
fn plot_rect(size: Size) -> Rectangle {
    let (pad_r, pad_b, pad_t) = (54.0, 20.0, 6.0);
    Rectangle { x: 0.0, y: pad_t, width: (size.width - pad_r).max(1.0), height: (size.height - pad_b - pad_t).max(1.0) }
}

fn grid_y(plot: Rectangle, g: u32) -> f32 {
    (plot.y + plot.height * (1.0 - g as f32 / 4.0)).round() + 0.5
}

/// A full chart: grid, right-hand labels, primary area series and an optional
/// dashed secondary series.
pub struct Chart<'a> {
    pub series: &'a [Series],
    pub slots: usize,
    pub max: f64,
    pub pal: Palette,
    pub label: Box<dyn Fn(f64) -> String + 'a>,
    pub range_label: String,
    /// The painted lines and grid.
    pub cache: &'a bitmap::Cache,
    pub mono: cosmic::iced::Font,
}

impl<'a> Chart<'a> {
    pub fn view<M: 'a>(self, height: f32) -> Element<'a, M> {
        let Chart { series, slots, max, pal, label, range_label, cache, mono } = self;
        let plot = bitmap::bitmap(cache, move |px, size, tf| {
            let plot = plot_rect(size);
            for g in 0..=4 {
                let y = grid_y(plot, g);
                if let Some(path) = line(&[Point::new(0.0, y), Point::new(plot.width, y)]) {
                    px.stroke_path(&path, &solid(pal.border), &Stroke { width: 1.0, ..Stroke::default() }, tf, None);
                }
            }
            paint_series(px, tf, series, slots, max, plot, &pal, true);
        })
        .width(Length::Fill)
        .height(height);
        let text = labels::labels(move |size| {
            let plot = plot_rect(size);
            let at = |content: String, position: Point, align_x: alignment::Horizontal, align_y: alignment::Vertical| Label {
                content,
                position,
                color: pal.fg_dim,
                size: 10.5,
                font: mono,
                align_x,
                align_y,
            };
            let foot = size.height - 4.0;
            let mut out: Vec<Label> = [0, 2, 4]
                .into_iter()
                .map(|g| {
                    at(
                        label(max * f64::from(g) / 4.0),
                        Point::new(plot.width + 8.0, grid_y(plot, g)),
                        alignment::Horizontal::Left,
                        alignment::Vertical::Center,
                    )
                })
                .collect();
            out.push(at(range_label.clone(), Point::new(0.0, foot), alignment::Horizontal::Left, alignment::Vertical::Bottom));
            out.push(at("now".to_owned(), Point::new(plot.width, foot), alignment::Horizontal::Right, alignment::Vertical::Bottom));
            out
        })
        .width(Length::Fill)
        .height(height);
        stack![plot, text].into()
    }
}

#[allow(clippy::too_many_arguments)] // one call site per chart kind
fn paint_series(px: &mut Pixmap, tf: Transform, series: &[Series], slots: usize, max: f64, plot: Rectangle, p: &Palette, endpoint: bool) {
    let floor = plot.y + plot.height;
    for (i, s) in series.iter().enumerate().rev() {
        let segs = segments(&s.max, slots, max, plot);
        if i == 0 {
            let top = Palette::with_alpha(p.accent, if p.dark { 0.28 } else { 0.18 });
            let fill = LinearGradient::new(
                tiny_skia::Point::from_xy(0.0, plot.y),
                tiny_skia::Point::from_xy(0.0, floor),
                vec![GradientStop::new(0.0, bitmap::color(top)), GradientStop::new(1.0, bitmap::color(Palette::with_alpha(p.accent, 0.0)))],
                SpreadMode::Pad,
                Transform::identity(),
            );
            for seg in &segs {
                if seg.len() > 1
                    && let (Some(shader), Some(path)) = (fill.clone(), area(seg, floor))
                {
                    let paint = tiny_skia::Paint { shader, anti_alias: true, ..tiny_skia::Paint::default() };
                    px.fill_path(&path, &paint, FillRule::Winding, tf, None);
                }
                if let Some(path) = line(seg) {
                    let stroke = Stroke { width: 1.6, line_join: tiny_skia::LineJoin::Round, ..Stroke::default() };
                    px.stroke_path(&path, &solid(p.accent), &stroke, tf, None);
                }
            }
            if endpoint
                && let Some(last) = segs.last().and_then(|s| s.last())
                && let Some(dot) = PathBuilder::from_circle(last.x, last.y, 3.0)
            {
                px.fill_path(&dot, &solid(p.accent), FillRule::Winding, tf, None);
            }
        } else {
            for seg in &segs {
                if let Some(path) = line(seg) {
                    let stroke = Stroke { width: 1.25, dash: StrokeDash::new(vec![4.0, 3.0], 0.0), ..Stroke::default() };
                    px.stroke_path(&path, &solid(p.fg_muted), &stroke, tf, None);
                }
            }
        }
    }
}

/// A sidebar sparkline: no grid or labels.
pub fn spark<'a, M: 'a>(series: Vec<Series>, max: f64, pal: Palette, cache: &'a bitmap::Cache) -> Element<'a, M> {
    bitmap::bitmap(cache, move |px, size, tf| {
        let plot = Rectangle { x: 0.0, y: 1.0, width: size.width, height: (size.height - 2.0).max(1.0) };
        paint_series(px, tf, &series, 60, max, plot, &pal, false);
    })
    .width(Length::Fill)
    .height(16)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_split_at_gaps_and_right_align() {
        let plot = Rectangle { x: 0.0, y: 0.0, width: 100.0, height: 50.0 };
        let s = segments(&[1.0, f64::NAN, 2.0, 4.0], 5, 4.0, plot);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].len(), 1);
        // 4 values in 5 slots: the first lands in slot 1 of 0..4.
        assert!((s[0][0].x - 25.0).abs() < 1e-3);
        assert!((s[1][1].x - 100.0).abs() < 1e-3);
        assert!((s[1][1].y - 0.0).abs() < 1e-3);
        assert!((s[1][0].y - 25.0).abs() < 1e-3);
        assert_eq!(segments(&[], 60, 1.0, plot).len(), 0);
        let clamped = segments(&[9.0], 1, 1.0, plot);
        assert!((clamped[0][0].y).abs() < 1e-3);
        let zero = segments(&[1.0], 2, 0.0, plot);
        assert!((zero[0][0].y - 50.0).abs() < 1e-3);
    }

    #[test]
    fn paints_without_panicking() {
        // 200×60 pixels at scale 2: the plot is laid out in 100×30 logical units.
        let mut px = Pixmap::new(200, 60).unwrap();
        let s = Series::from_history(1.0, vec![1.0, f64::NAN, 3.0, 2.0], vec![1.0, f64::NAN, 3.0, 2.0]);
        let plot = plot_rect(Size::new(100.0, 30.0));
        paint_series(&mut px, Transform::from_scale(2.0, 2.0), &[s.clone(), s], 60, 4.0, plot, &crate::palette::DARK, true);
        assert!(px.pixels().iter().any(|p| p.alpha() > 0));
        assert!(line(&[]).is_none());
        assert!(area(&[Point::ORIGIN], 10.0).is_some());
    }
}
