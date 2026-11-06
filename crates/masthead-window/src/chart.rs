//! Charts on an iced canvas. Each chart keeps its geometry in a
//! [`canvas::Cache`] that is cleared only when a new sample arrives, so
//! nothing is re-tessellated between samples.

use crate::model::Series;
use crate::palette::Palette;
use cosmic::iced::widget::canvas::{self, Frame, Geometry, LineDash, Path, Stroke, Text, gradient};
use cosmic::iced::{Color, Pixels, Point, Rectangle, Size, alignment, mouse};
use cosmic::{Renderer, Theme};

pub type Cache = canvas::Cache<Renderer>;

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

fn line(points: &[Point]) -> Path {
    Path::new(|b| {
        for (i, p) in points.iter().enumerate() {
            if i == 0 { b.move_to(*p) } else { b.line_to(*p) }
        }
    })
}

fn area(points: &[Point], floor: f32) -> Path {
    Path::new(|b| {
        let (Some(first), Some(last)) = (points.first(), points.last()) else { return };
        b.move_to(Point::new(first.x, floor));
        for p in points {
            b.line_to(*p);
        }
        b.line_to(Point::new(last.x, floor));
        b.close();
    })
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
    pub cache: &'a Cache,
    pub mono: cosmic::iced::Font,
}

impl<M> canvas::Program<M, Theme, Renderer> for Chart<'_> {
    type State = ();

    fn draw(&self, _state: &(), renderer: &Renderer, _theme: &Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<Geometry<Renderer>> {
        let g = self.cache.draw(renderer, bounds.size(), |frame: &mut Frame<Renderer>| {
            let size = frame.size();
            let (pad_r, pad_b, pad_t) = (54.0, 20.0, 6.0);
            let plot = Rectangle { x: 0.0, y: pad_t, width: (size.width - pad_r).max(1.0), height: (size.height - pad_b - pad_t).max(1.0) };
            let p = self.pal;
            for g in 0..=4 {
                let y = (plot.y + plot.height * (1.0 - g as f32 / 4.0)).round() + 0.5;
                frame.stroke(
                    &line(&[Point::new(0.0, y), Point::new(plot.width, y)]),
                    Stroke::default().with_color(p.border).with_width(1.0),
                );
                if g % 2 == 0 {
                    frame.fill_text(Text {
                        content: (self.label)(self.max * f64::from(g) / 4.0),
                        position: Point::new(plot.width + 8.0, y),
                        color: p.fg_dim,
                        size: Pixels(10.5),
                        font: self.mono,
                        align_y: alignment::Vertical::Center,
                        ..Text::default()
                    });
                }
            }
            let foot = size.height - 4.0;
            frame.fill_text(Text {
                content: self.range_label.clone(),
                position: Point::new(0.0, foot),
                color: p.fg_dim,
                size: Pixels(10.5),
                font: self.mono,
                align_y: alignment::Vertical::Bottom,
                ..Text::default()
            });
            frame.fill_text(Text {
                content: "now".to_owned(),
                position: Point::new(plot.width, foot),
                color: p.fg_dim,
                size: Pixels(10.5),
                font: self.mono,
                align_x: alignment::Horizontal::Right.into(),
                align_y: alignment::Vertical::Bottom,
                ..Text::default()
            });
            draw_series(frame, self.series, self.slots, self.max, plot, &p, true);
        });
        vec![g]
    }
}

fn draw_series(frame: &mut Frame<Renderer>, series: &[Series], slots: usize, max: f64, plot: Rectangle, p: &Palette, endpoint: bool) {
    let floor = plot.y + plot.height;
    for (i, s) in series.iter().enumerate().rev() {
        let segs = segments(&s.max, slots, max, plot);
        if i == 0 {
            let fill = gradient::Linear::new(Point::new(0.0, plot.y), Point::new(0.0, floor))
                .add_stop(0.0, Palette::with_alpha(p.accent, if p.dark { 0.28 } else { 0.18 }))
                .add_stop(1.0, Palette::with_alpha(p.accent, 0.0));
            for seg in &segs {
                if seg.len() > 1 {
                    frame.fill(&area(seg, floor), fill);
                }
                frame.stroke(&line(seg), Stroke::default().with_color(p.accent).with_width(1.6));
            }
            if endpoint && let Some(last) = segs.last().and_then(|s| s.last()) {
                frame.fill(&Path::circle(*last, 3.0), p.accent);
            }
        } else {
            for seg in &segs {
                let stroke = Stroke {
                    line_dash: LineDash { segments: &[4.0, 3.0], offset: 0 },
                    ..Stroke::default().with_color(p.fg_muted).with_width(1.25)
                };
                frame.stroke(&line(seg), stroke);
            }
        }
    }
}

/// A sidebar sparkline: no grid or labels.
pub struct Spark<'a> {
    pub series: Vec<Series>,
    pub max: f64,
    pub pal: Palette,
    pub cache: &'a Cache,
}

impl<M> canvas::Program<M, Theme, Renderer> for Spark<'_> {
    type State = ();

    fn draw(&self, _state: &(), renderer: &Renderer, _theme: &Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<Geometry<Renderer>> {
        let g = self.cache.draw(renderer, bounds.size(), |frame: &mut Frame<Renderer>| {
            let s = frame.size();
            let plot = Rectangle { x: 0.0, y: 1.0, width: s.width, height: (s.height - 2.0).max(1.0) };
            draw_series(frame, &self.series, 60, self.max, plot, &self.pal, false);
        });
        vec![g]
    }
}

/// A horizontal meter: track plus fill, `fraction` 0–1.
pub struct Meter {
    pub fraction: f32,
    pub color: Color,
    pub track: Color,
}

impl<M> canvas::Program<M, Theme, Renderer> for Meter {
    type State = ();

    fn draw(&self, _state: &(), renderer: &Renderer, _theme: &Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<Geometry<Renderer>> {
        let mut frame = Frame::new(renderer, bounds.size());
        let h = bounds.height;
        let r = h / 2.0;
        frame.fill(&Path::rounded_rectangle(Point::ORIGIN, bounds.size(), r.into()), self.track);
        let w = (bounds.width * self.fraction.clamp(0.0, 1.0)).max(if self.fraction > 0.0 { h } else { 0.0 });
        if w > 0.0 {
            frame.fill(&Path::rounded_rectangle(Point::ORIGIN, Size::new(w, h), r.into()), self.color);
        }
        vec![frame.into_geometry()]
    }
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
}
