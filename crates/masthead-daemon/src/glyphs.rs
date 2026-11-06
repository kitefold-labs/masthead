//! Bar values drawn into a 32×32 tray icon.
//!
//! Plasma and several other tray hosts ignore SNI text labels, so the daemon
//! renders the value into the icon pixmap itself with a built-in 4×7 digit
//! font. No font library is linked.

/// Rows of a 4-pixel-wide glyph, most significant bit on the left.
fn glyph(c: char) -> [u8; 7] {
    match c {
        '0' => [0x6, 0x9, 0xB, 0xD, 0x9, 0x9, 0x6],
        '1' => [0x2, 0x6, 0x2, 0x2, 0x2, 0x2, 0x7],
        '2' => [0x6, 0x9, 0x1, 0x2, 0x4, 0x8, 0xF],
        '3' => [0xE, 0x1, 0x1, 0x6, 0x1, 0x1, 0xE],
        '4' => [0x2, 0x6, 0xA, 0xA, 0xF, 0x2, 0x2],
        '5' => [0xF, 0x8, 0xE, 0x1, 0x1, 0x9, 0x6],
        '6' => [0x6, 0x8, 0x8, 0xE, 0x9, 0x9, 0x6],
        '7' => [0xF, 0x1, 0x2, 0x2, 0x4, 0x4, 0x4],
        '8' => [0x6, 0x9, 0x9, 0x6, 0x9, 0x9, 0x6],
        '9' => [0x6, 0x9, 0x9, 0x7, 0x1, 0x1, 0x6],
        '%' => [0xC, 0xD, 0x2, 0x4, 0xB, 0x3, 0x0],
        'G' => [0x6, 0x9, 0x8, 0xB, 0x9, 0x9, 0x7],
        'M' => [0x9, 0xF, 0xF, 0x9, 0x9, 0x9, 0x9],
        'K' => [0x9, 0xA, 0xC, 0xC, 0xA, 0x9, 0x9],
        'B' => [0xE, 0x9, 0x9, 0xE, 0x9, 0x9, 0xE],
        '°' => [0x6, 0x9, 0x6, 0x0, 0x0, 0x0, 0x0],
        '.' => [0x0, 0x0, 0x0, 0x0, 0x0, 0x6, 0x6],
        '-' => [0x0, 0x0, 0x0, 0xF, 0x0, 0x0, 0x0],
        _ => [0; 7],
    }
}

pub const SIZE: i32 = 32;
const SCALE: usize = 2;

/// A compact form of a bar value that fits three glyphs: `23`, `18G`, `.2M`, `64°`.
pub fn icon_text(id: &str, value: f64) -> String {
    match id {
        "mem" => format!("{}G", (value / f64::from(1u32 << 30)).round() as u64),
        "net" | "disk" => {
            let units = ['B', 'K', 'M', 'G'];
            let (mut v, mut i) = (value.max(0.0), 0);
            while v >= 1000.0 && i < 3 {
                v /= 1000.0;
                i += 1;
            }
            if v >= 100.0 && i < 3 {
                format!(".{}{}", (v / 100.0).round() as u64 % 10, units[i + 1])
            } else {
                format!("{}{}", v.round() as u64, units[i])
            }
        }
        "temp" => format!("{}°", value.round() as i64),
        _ => format!("{}", value.round().clamp(0.0, 999.0) as u64),
    }
}

fn level_color(level: u32) -> [u8; 4] {
    // ARGB
    match level {
        2 => [0xFF, 0xF8, 0x71, 0x71],
        1 => [0xFF, 0xFB, 0xBF, 0x24],
        _ => [0xFF, 0xE9, 0xEB, 0xEE],
    }
}

/// Render `text` and an optional gauge (`fraction` 0–1) as ARGB32 in network
/// byte order, the format SNI's `IconPixmap` expects.
pub fn render(text: &str, fraction: Option<f64>, level: u32) -> Vec<u8> {
    let n = SIZE as usize;
    let mut px = vec![0u8; n * n * 4];
    let put = |x: usize, y: usize, c: [u8; 4], px: &mut Vec<u8>| {
        if x < n && y < n {
            let i = (y * n + x) * 4;
            px[i..i + 4].copy_from_slice(&c);
        }
    };
    let chars: Vec<char> = text.chars().take(3).collect();
    let gw = 4 * SCALE;
    let gap = SCALE;
    let width = chars.len() * gw + chars.len().saturating_sub(1) * gap;
    let x0 = n.saturating_sub(width) / 2;
    let y0 = if fraction.is_some() { 5 } else { 9 };
    let fg = level_color(level);
    let shadow = [0x99, 0x00, 0x00, 0x00];
    // Shadow first so a light tray background still shows the digits.
    for pass in 0..2 {
        for (ci, ch) in chars.iter().enumerate() {
            let rows = glyph(*ch);
            for (ry, row) in rows.iter().enumerate() {
                for rx in 0..4 {
                    if row & (0x8 >> rx) == 0 {
                        continue;
                    }
                    for dy in 0..SCALE {
                        for dx in 0..SCALE {
                            let x = x0 + ci * (gw + gap) + rx * SCALE + dx;
                            let y = y0 + ry * SCALE + dy;
                            if pass == 0 {
                                put(x + 1, y + 1, shadow, &mut px);
                            } else {
                                put(x, y, fg, &mut px);
                            }
                        }
                    }
                }
            }
        }
    }
    if let Some(f) = fraction {
        let (gx, gy, gw, gh) = (2usize, 24usize, n - 4, 4usize);
        let fill = (f.clamp(0.0, 1.0) * gw as f64).round() as usize;
        let accent = if level == 0 { [0xFF, 0x26, 0xD2, 0xEE] } else { fg };
        for y in gy..gy + gh {
            for x in gx..gx + gw {
                put(x, y, if x - gx < fill { accent } else { [0x55, 0xFF, 0xFF, 0xFF] }, &mut px);
            }
        }
    }
    px
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_text() {
        assert_eq!(icon_text("cpu", 23.4), "23");
        assert_eq!(icon_text("cpu", 100.0), "100");
        assert_eq!(icon_text("mem", 19_756_849_152.0), "18G");
        assert_eq!(icon_text("net", 180_000.0), ".2M");
        assert_eq!(icon_text("net", 2_100_000.0), "2M");
        assert_eq!(icon_text("disk", 512.0), ".5K");
        assert_eq!(icon_text("net", 12.0), "12B");
        assert_eq!(icon_text("temp", 64.4), "64°");
    }

    #[test]
    fn renders() {
        let px = render("23%", Some(0.5), 0);
        assert_eq!(px.len(), 32 * 32 * 4);
        assert!(px.chunks(4).any(|c| c == [0xFF, 0xE9, 0xEB, 0xEE]));
        assert!(px.chunks(4).any(|c| c == [0xFF, 0x26, 0xD2, 0xEE]));
        let warn = render("99", None, 2);
        assert!(warn.chunks(4).any(|c| c == [0xFF, 0xF8, 0x71, 0x71]));
        assert!(render("x", None, 1).iter().all(|b| *b == 0));
        let _ = glyph('-');
    }
}
