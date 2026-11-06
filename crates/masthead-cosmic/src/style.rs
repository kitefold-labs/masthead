// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! Halyard's palette for the popover, and the panel's status colors.

use cosmic::iced::Color;
use cosmic::iced::font::{Family, Font, Weight};
use std::path::Path;

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a: 1.0 }
}

const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a }
}

/// Halyard design tokens (halyard-design/tokens.json) the popover uses.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub surface: Color,
    pub border: Color,
    pub fg: Color,
    pub muted: Color,
    pub accent: Color,
    pub accent_soft: Color,
    pub warn: Color,
    pub crit: Color,
    pub grid: Color,
}

pub const DARK: Palette = Palette {
    surface: rgb(0x15, 0x18, 0x1d),
    border: rgba(255, 255, 255, 0.10),
    fg: rgb(0xe9, 0xeb, 0xee),
    muted: rgb(0x8d, 0x94, 0x9b),
    accent: rgb(0x26, 0xd2, 0xee),
    accent_soft: rgba(0x26, 0xd2, 0xee, 0.14),
    warn: rgb(0xfb, 0xbf, 0x24),
    crit: rgb(0xf8, 0x71, 0x71),
    grid: rgba(255, 255, 255, 0.07),
};

pub const LIGHT: Palette = Palette {
    surface: rgb(0xff, 0xff, 0xff),
    border: rgba(0, 0, 0, 0.09),
    fg: rgb(0x16, 0x18, 0x1c),
    muted: rgb(0x5d, 0x64, 0x6c),
    accent: rgb(0x0c, 0x8f, 0xa8),
    accent_soft: rgba(0x0c, 0x9c, 0xb8, 0.10),
    warn: rgb(0xb8, 0x75, 0x03),
    crit: rgb(0xdc, 0x4d, 0x63),
    grid: rgba(0, 0, 0, 0.07),
};

/// The palette matching the active COSMIC theme.
pub fn palette() -> Palette {
    if cosmic::theme::active().cosmic().is_dark { DARK } else { LIGHT }
}

/// Amber or red for warn and critical; `None` keeps the theme's text color.
pub fn level_color(p: &Palette, level: masthead_core::Level) -> Option<Color> {
    match level {
        masthead_core::Level::Normal => None,
        masthead_core::Level::Warn => Some(p.warn),
        masthead_core::Level::Crit => Some(p.crit),
    }
}

fn has_font(dir: &Path, needle: &str, depth: u8) -> bool {
    let Ok(rd) = std::fs::read_dir(dir) else { return false };
    rd.filter_map(Result::ok).any(|e| {
        let p = e.path();
        if p.is_dir() {
            depth > 0 && has_font(&p, needle, depth - 1)
        } else {
            p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(needle))
        }
    })
}

/// `JetBrains` Mono when installed, else the system monospace face. Either way
/// digits have equal widths, so a changing value never shifts the panel.
pub fn value_font() -> Font {
    let mut dirs = vec![std::path::PathBuf::from("/usr/share/fonts"), std::path::PathBuf::from("/usr/local/share/fonts")];
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(Path::new(&home).join(".local/share/fonts"));
        dirs.push(Path::new(&home).join(".fonts"));
    }
    let family = if dirs.iter().any(|d| has_font(d, "JetBrainsMono", 3)) { Family::Name("JetBrains Mono") } else { Family::Monospace };
    Font { family, weight: Weight::Medium, ..Font::DEFAULT }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors() {
        assert!(level_color(&DARK, masthead_core::Level::Normal).is_none());
        assert_eq!(level_color(&DARK, masthead_core::Level::Crit), Some(DARK.crit));
        assert_eq!(level_color(&LIGHT, masthead_core::Level::Warn), Some(LIGHT.warn));
        let d = tempfile_dir();
        std::fs::create_dir_all(d.join("a/b")).unwrap();
        std::fs::write(d.join("a/b/JetBrainsMono-Medium.ttf"), b"").unwrap();
        assert!(has_font(&d, "JetBrainsMono", 3));
        assert!(!has_font(&d, "JetBrainsMono", 0));
        let _ = std::fs::remove_dir_all(&d);
        let _ = value_font();
    }

    fn tempfile_dir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("masthead-cosmic-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }
}
