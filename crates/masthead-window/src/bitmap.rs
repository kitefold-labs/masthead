// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! A widget showing a bitmap that is painted on demand with tiny-skia and
//! kept until its content, size or scale changes.
//!
//! The software renderer rasterizes every path of a canvas on every frame,
//! clip-masked twice over. A bitmap is one copy, so charts and the disk map
//! are painted once per sample or navigation and blitted after that.

use cosmic::iced::advanced::image::Renderer as _;
use cosmic::iced::advanced::layout::{self, Layout, Limits};
use cosmic::iced::advanced::widget::Tree;
use cosmic::iced::advanced::{Widget, image, renderer};
use cosmic::iced::{Element, Length, Radians, Rectangle, Size, mouse};
use std::cell::RefCell;
use tiny_skia::{Pixmap, Transform};

/// The bitmap a [`Bitmap`] last painted, for the size and scale it was
/// painted at. Clear it when the content changes.
#[derive(Debug, Default)]
pub struct Cache(RefCell<Option<Entry>>);

#[derive(Debug)]
struct Entry {
    width: u32,
    height: u32,
    scale: f32,
    handle: image::Handle,
}

impl Cache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&self) {
        *self.0.borrow_mut() = None;
    }

    fn get(&self, width: u32, height: u32, scale: f32) -> Option<image::Handle> {
        self.0
            .borrow()
            .as_ref()
            .filter(|e| e.width == width && e.height == height && (e.scale - scale).abs() < f32::EPSILON)
            .map(|e| e.handle.clone())
    }
}

/// Paints into a pixmap. `size` is the logical size; `transform` scales
/// logical coordinates to the pixmap's pixels.
pub type Paint<'a> = Box<dyn Fn(&mut Pixmap, Size, Transform) + 'a>;

pub struct Bitmap<'a> {
    width: Length,
    height: Length,
    cache: &'a Cache,
    paint: Paint<'a>,
}

impl std::fmt::Debug for Bitmap<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bitmap")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("cache", self.cache)
            .finish_non_exhaustive()
    }
}

pub fn bitmap<'a>(cache: &'a Cache, paint: impl Fn(&mut Pixmap, Size, Transform) + 'a) -> Bitmap<'a> {
    Bitmap { width: Length::Fill, height: Length::Fill, cache, paint: Box::new(paint) }
}

impl Bitmap<'_> {
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    fn render(&self, width: u32, height: u32, scale: f32) -> Option<image::Handle> {
        let mut pixmap = Pixmap::new(width, height)?;
        let size = Size::new(width as f32 / scale, height as f32 / scale);
        (self.paint)(&mut pixmap, size, Transform::from_scale(scale, scale));
        // tiny-skia stores premultiplied pixels; the renderer wants straight RGBA.
        let mut rgba = Vec::with_capacity(pixmap.pixels().len() * 4);
        for p in pixmap.pixels() {
            let c = p.demultiply();
            rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
        }
        let handle = image::Handle::from_rgba(width, height, rgba);
        *self.cache.0.borrow_mut() = Some(Entry { width, height, scale, handle: handle.clone() });
        Some(handle)
    }
}

impl<Message> Widget<Message, cosmic::Theme, cosmic::Renderer> for Bitmap<'_> {
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
        style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let scale = style.scale_factor as f32;
        let (width, height) = ((bounds.width * scale).round() as u32, (bounds.height * scale).round() as u32);
        if width == 0 || height == 0 {
            return;
        }
        let Some(handle) = self.cache.get(width, height, scale).or_else(|| self.render(width, height, scale)) else {
            return;
        };
        let image = image::Image {
            handle,
            filter_method: image::FilterMethod::Nearest,
            rotation: Radians(0.0),
            border_radius: 0.0.into(),
            opacity: 1.0,
            snap: true,
        };
        renderer.draw_image(image, bounds, *viewport);
    }
}

impl<'a, Message: 'a> From<Bitmap<'a>> for Element<'a, Message, cosmic::Theme, cosmic::Renderer> {
    fn from(b: Bitmap<'a>) -> Self {
        Element::new(b)
    }
}

/// An iced color as tiny-skia sees it.
pub fn color(c: cosmic::iced::Color) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba(c.r, c.g, c.b, c.a).unwrap_or(tiny_skia::Color::TRANSPARENT)
}

/// A solid, anti-aliased paint.
pub fn solid(c: cosmic::iced::Color) -> tiny_skia::Paint<'static> {
    tiny_skia::Paint { shader: tiny_skia::Shader::SolidColor(color(c)), anti_alias: true, ..tiny_skia::Paint::default() }
}
