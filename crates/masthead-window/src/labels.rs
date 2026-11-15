// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! Text at computed positions: chart axis labels, names on the disk map.
//!
//! Drawn straight through the renderer with a tight clip rectangle per label.
//! Canvas text carries the whole canvas as its clip rectangle, and the
//! software renderer masks every glyph of a text whose clip rectangle touches
//! the edge of a scrolling area, rebuilding the window-sized mask each time.

use cosmic::iced::advanced::layout::{self, Layout, Limits};
use cosmic::iced::advanced::text::Renderer as _;
use cosmic::iced::advanced::widget::Tree;
use cosmic::iced::advanced::{Widget, renderer, text};
use cosmic::iced::{Color, Element, Font, Length, Pixels, Point, Rectangle, Size, alignment, mouse};

#[derive(Debug, Clone)]
pub struct Label {
    pub content: String,
    /// The anchor, relative to the widget, that `align_x` and `align_y` place
    /// the text against.
    pub position: Point,
    pub color: Color,
    pub size: f32,
    pub font: Font,
    pub align_x: alignment::Horizontal,
    pub align_y: alignment::Vertical,
}

impl Label {
    /// A rectangle around the text, from a generous width estimate.
    fn bounds(&self, origin: Point) -> Rectangle {
        let width = self.content.chars().count() as f32 * self.size * 0.75 + 6.0;
        let height = self.size * 1.5;
        let x = match self.align_x {
            alignment::Horizontal::Left => self.position.x,
            alignment::Horizontal::Center => self.position.x - width / 2.0,
            alignment::Horizontal::Right => self.position.x - width,
        };
        let y = match self.align_y {
            alignment::Vertical::Top => self.position.y,
            alignment::Vertical::Center => self.position.y - height / 2.0,
            alignment::Vertical::Bottom => self.position.y - height,
        };
        Rectangle { x: origin.x + x - 1.0, y: origin.y + y - 1.0, width: width + 2.0, height: height + 2.0 }
    }
}

pub struct Labels<'a> {
    width: Length,
    height: Length,
    layout: Box<dyn Fn(Size) -> Vec<Label> + 'a>,
}

impl std::fmt::Debug for Labels<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Labels").field("width", &self.width).field("height", &self.height).finish_non_exhaustive()
    }
}

/// Labels placed by `layout` for the widget's size.
pub fn labels<'a>(layout: impl Fn(Size) -> Vec<Label> + 'a) -> Labels<'a> {
    Labels { width: Length::Fill, height: Length::Fill, layout: Box::new(layout) }
}

impl Labels<'_> {
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }
}

impl<Message> Widget<Message, cosmic::Theme, cosmic::Renderer> for Labels<'_> {
    fn size(&self) -> Size<Length> {
        Size { width: self.width, height: self.height }
    }

    fn layout(&mut self, _tree: &mut Tree, _renderer: &cosmic::Renderer, limits: &Limits) -> layout::Node {
        layout::atomic(limits, self.width, self.height)
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut cosmic::Renderer,
        _theme: &cosmic::Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let Some(visible) = bounds.intersection(viewport) else { return };
        let origin = bounds.position();
        for label in (self.layout)(bounds.size()) {
            let Some(clip) = label.bounds(origin).intersection(&visible) else { continue };
            // Finite bounds: with infinite ones cosmic-text lays out one glyph.
            let text = text::Text {
                content: label.content,
                bounds: Size::new(4096.0, label.size * 2.0),
                size: Pixels(label.size),
                line_height: text::LineHeight::default(),
                font: label.font,
                align_x: label.align_x.into(),
                align_y: label.align_y,
                shaping: text::Shaping::Basic,
                wrapping: text::Wrapping::None,
                ellipsize: text::Ellipsize::None,
            };
            renderer.fill_text(text, origin + (label.position - Point::ORIGIN), label.color, clip);
        }
    }
}

impl<'a, Message: 'a> From<Labels<'a>> for Element<'a, Message, cosmic::Theme, cosmic::Renderer> {
    fn from(l: Labels<'a>) -> Self {
        Element::new(l)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_follow_alignment() {
        let l = Label {
            content: "50%".into(),
            position: Point::new(100.0, 20.0),
            color: Color::WHITE,
            size: 10.0,
            font: Font::DEFAULT,
            align_x: alignment::Horizontal::Right,
            align_y: alignment::Vertical::Center,
        };
        let b = l.bounds(Point::new(5.0, 5.0));
        assert!(b.x < 105.0 && b.x + b.width > 105.0, "{b:?}");
        assert!(b.y < 25.0 && b.y + b.height > 25.0, "{b:?}");
    }
}
