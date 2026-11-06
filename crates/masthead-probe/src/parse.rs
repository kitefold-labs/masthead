// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! Byte-slice parsing helpers. None of these allocate.

/// Split on ASCII whitespace, skipping empty fields.
pub fn fields(line: &[u8]) -> impl Iterator<Item = &[u8]> {
    line.split(u8::is_ascii_whitespace).filter(|f| !f.is_empty())
}

/// Split into lines on `\n`.
pub fn lines(buf: &[u8]) -> impl Iterator<Item = &[u8]> {
    buf.split(|b| *b == b'\n')
}

/// Parse an unsigned decimal integer. The whole slice must be digits.
pub fn u64_of(s: &[u8]) -> Option<u64> {
    if s.is_empty() {
        return None;
    }
    let mut v: u64 = 0;
    for &b in s {
        if !b.is_ascii_digit() {
            return None;
        }
        v = v.checked_mul(10)?.checked_add(u64::from(b - b'0'))?;
    }
    Some(v)
}

/// Parse a signed decimal integer (sysfs temperatures can be negative).
pub fn i64_of(s: &[u8]) -> Option<i64> {
    match s.split_first() {
        Some((b'-', rest)) => u64_of(rest).and_then(|v| i64::try_from(v).ok()).map(|v| -v),
        _ => u64_of(s).and_then(|v| i64::try_from(v).ok()),
    }
}

/// Parse a simple decimal like `4.12` or `12345.67`. No exponents.
pub fn f64_of(s: &[u8]) -> Option<f64> {
    let (neg, s) = match s.split_first() {
        Some((b'-', rest)) => (true, rest),
        _ => (false, s),
    };
    let mut parts = s.splitn(2, |b| *b == b'.');
    let int = parts.next()?;
    let mut v = if int.is_empty() { 0.0 } else { u64_of(int)? as f64 };
    if let Some(frac) = parts.next()
        && !frac.is_empty()
    {
        let f = u64_of(frac)? as f64;
        v += f / 10f64.powi(i32::try_from(frac.len()).ok()?);
    }
    Some(if neg { -v } else { v })
}

/// Trim ASCII whitespace from both ends.
pub fn trim(s: &[u8]) -> &[u8] {
    s.trim_ascii()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers() {
        assert_eq!(u64_of(b"0"), Some(0));
        assert_eq!(u64_of(b"18446744073709551615"), Some(u64::MAX));
        assert_eq!(u64_of(b"18446744073709551616"), None);
        assert_eq!(u64_of(b""), None);
        assert_eq!(u64_of(b"12a"), None);
        assert_eq!(i64_of(b"-4500"), Some(-4500));
        assert_eq!(i64_of(b"4500"), Some(4500));
    }

    #[test]
    fn floats() {
        assert!((f64_of(b"4.12").unwrap() - 4.12).abs() < 1e-9);
        assert!((f64_of(b"12345.6").unwrap() - 12345.6).abs() < 1e-9);
        assert!((f64_of(b"-0.5").unwrap() + 0.5).abs() < 1e-9);
        assert_eq!(f64_of(b"7"), Some(7.0));
        assert_eq!(f64_of(b"7."), Some(7.0));
        assert_eq!(f64_of(b"x"), None);
    }

    #[test]
    fn splitting() {
        let f: Vec<_> = fields(b"  a  bb\tc ").collect();
        assert_eq!(f, vec![&b"a"[..], b"bb", b"c"]);
        assert_eq!(lines(b"a\nb").count(), 2);
        assert_eq!(trim(b" x\n"), b"x");
    }
}
