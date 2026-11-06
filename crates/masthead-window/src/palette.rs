//! Halyard's design tokens, and a libcosmic theme built from them so stock
//! widgets (toggles, inputs, dropdowns, scrollbars, the header bar) match.

// Colors are written as 0xRRGGBB to match the token tables.
#![allow(clippy::unreadable_literal)]

use cosmic::cosmic_theme::palette::{Srgb, Srgba};
use cosmic::iced::Color;
use std::sync::Arc;

/// One theme's tokens. Copy so view closures can capture it by value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub dark: bool,
    pub bg: Color,
    pub surface: Color,
    pub surface2: Color,
    pub surface3: Color,
    pub border: Color,
    pub fg: Color,
    pub fg_muted: Color,
    pub fg_dim: Color,
    pub accent: Color,
    pub accent_soft: Color,
    pub accent_line: Color,
    pub success: Color,
    pub warning: Color,
    pub danger: Color,
}

const fn rgb(hex: u32) -> Color {
    Color { r: ((hex >> 16) & 0xff) as f32 / 255.0, g: ((hex >> 8) & 0xff) as f32 / 255.0, b: (hex & 0xff) as f32 / 255.0, a: 1.0 }
}

const fn rgba(hex: u32, a: f32) -> Color {
    let c = rgb(hex);
    Color { a, ..c }
}

pub const DARK: Palette = Palette {
    dark: true,
    bg: rgb(0x0a0b0d),
    surface: rgb(0x101216),
    surface2: rgb(0x15181d),
    surface3: rgb(0x1b1f25),
    border: rgba(0xffffff, 0.09),
    fg: rgb(0xe9ebee),
    fg_muted: rgb(0x878e96),
    fg_dim: rgb(0x545b63),
    accent: rgb(0x26d2ee),
    accent_soft: rgba(0x26d2ee, 0.12),
    accent_line: rgba(0x26d2ee, 0.30),
    success: rgb(0x34d399),
    warning: rgb(0xfbbf24),
    danger: rgb(0xf87171),
};

pub const LIGHT: Palette = Palette {
    dark: false,
    bg: rgb(0xfbfbf9),
    surface: rgb(0xffffff),
    surface2: rgb(0xf4f4f1),
    surface3: rgb(0xececea),
    border: rgba(0x000000, 0.09),
    fg: rgb(0x16181c),
    fg_muted: rgb(0x5d646c),
    fg_dim: rgb(0x9aa0a7),
    accent: rgb(0x0c8fa8),
    accent_soft: rgba(0x0c9cb8, 0.10),
    accent_line: rgba(0x0c9cb8, 0.28),
    success: rgb(0x0c9f6e),
    warning: rgb(0xb87503),
    danger: rgb(0xdc4d63),
};

impl Palette {
    pub fn for_dark(dark: bool) -> Self {
        if dark { DARK } else { LIGHT }
    }

    /// The color for a daemon level: 0 normal, 1 warn, 2 critical.
    pub fn level(&self, level: u32) -> Color {
        match level {
            2 => self.danger,
            1 => self.warning,
            _ => self.fg,
        }
    }

    pub fn with_alpha(c: Color, a: f32) -> Color {
        Color { a, ..c }
    }

    /// `over` composited onto `under`, as an opaque color. Hairlines drawn
    /// this way stay faint where a translucent 1 px fill would not.
    pub fn over(over: Color, under: Color) -> Color {
        let a = over.a;
        Color { r: under.r * (1.0 - a) + over.r * a, g: under.g * (1.0 - a) + over.g * a, b: under.b * (1.0 - a) + over.b * a, a: 1.0 }
    }
}

fn srgb(c: Color) -> Srgb {
    Srgb::new(c.r, c.g, c.b)
}

fn srgba(c: Color) -> Srgba {
    Srgba::new(c.r, c.g, c.b, c.a)
}

/// A libcosmic theme carrying Halyard's background, surfaces, accent and status colors.
pub fn cosmic_theme(p: &Palette) -> cosmic::Theme {
    let builder = if p.dark { cosmic::cosmic_theme::ThemeBuilder::dark() } else { cosmic::cosmic_theme::ThemeBuilder::light() };
    let mut b = builder.bg_color(srgba(p.bg)).primary_container_bg(srgba(p.surface)).accent(srgb(p.accent)).text_tint(srgb(p.fg));
    b.secondary_container_bg = Some(srgba(p.surface2));
    b.success = Some(srgb(p.success));
    b.warning = Some(srgb(p.warning));
    b.destructive = Some(srgb(p.danger));
    let mut radii = b.corner_radii;
    // Halyard radii: sm 4, md 6, lg 8, xl 12.
    radii.radius_xs = [4.0; 4];
    radii.radius_s = [6.0; 4];
    radii.radius_m = [8.0; 4];
    radii.radius_l = [12.0; 4];
    radii.radius_xl = [12.0; 4];
    b.corner_radii = radii;
    cosmic::Theme::custom(Arc::new(b.build()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens() {
        assert!((DARK.bg.r - 10.0 / 255.0).abs() < 1e-6);
        assert!((LIGHT.accent.g - f32::from(0x8f_u8) / 255.0).abs() < 1e-6);
        assert!((DARK.border.a - 0.09).abs() < 1e-6);
        assert_eq!(Palette::for_dark(false), LIGHT);
        assert_eq!(DARK.level(2), DARK.danger);
        assert_eq!(DARK.level(1), DARK.warning);
        assert_eq!(DARK.level(0), DARK.fg);
        assert!((Palette::with_alpha(DARK.fg, 0.5).a - 0.5).abs() < 1e-6);
        let o = Palette::over(Color { r: 1.0, g: 1.0, b: 1.0, a: 0.5 }, Color::BLACK);
        assert!((o.r - 0.5).abs() < 1e-6 && (o.a - 1.0).abs() < 1e-6);
    }
}
