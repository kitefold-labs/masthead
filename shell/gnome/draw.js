// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

// Cairo drawing for sparklines and small bar charts.

/** Halyard dark-theme colors, as [r, g, b] in 0–1. */
export const COLORS = {
    accent: [0x26 / 255, 0xd2 / 255, 0xee / 255],
    fg: [0xe9 / 255, 0xeb / 255, 0xee / 255],
    dim: [0x8d / 255, 0x94 / 255, 0x9b / 255],
    warn: [0xfb / 255, 0xbf / 255, 0x24 / 255],
    crit: [0xf8 / 255, 0x71 / 255, 0x71 / 255],
};

export function levelColor(level) {
    return level === 2 ? COLORS.crit : level === 1 ? COLORS.warn : COLORS.accent;
}

/** A tidy upper bound: the smallest 1, 2, 2.5, 5 × 10ⁿ at or above the peak. */
export function niceMax(seriesList) {
    let m = 0;
    for (const s of seriesList) {
        for (const v of s) {
            if (Number.isFinite(v) && v > m)
                m = v;
        }
    }
    if (m <= 0)
        return 1;
    const p = 10 ** Math.floor(Math.log10(m));
    for (const k of [1, 2, 2.5, 5, 10]) {
        if (k * p >= m)
            return k * p;
    }
    return 10 * p;
}

/**
 * Draw one or two series into a cairo context. The first is an accent line
 * with a soft fill; the second a dim dashed line. NaN values leave gaps.
 *
 * @param {object} cr - cairo context
 * @param {number} w - width in logical pixels
 * @param {number} h - height in logical pixels
 * @param {number[][]} seriesList - values, oldest first
 * @param {number} max - value at the top edge
 * @param {number[]} color - [r, g, b] for the first series
 */
export function sparkline(cr, w, h, seriesList, max, color = COLORS.accent) {
    seriesList.forEach((values, si) => {
        const n = values.length;
        if (n < 2)
            return;
        const x = i => (i / (n - 1)) * w;
        const y = v => h - 1 - Math.min(1, Math.max(0, v / max)) * (h - 2);
        const segments = [];
        let seg = [];
        values.forEach((v, i) => {
            if (Number.isFinite(v)) {
                seg.push([x(i), y(v)]);
            } else if (seg.length) {
                segments.push(seg);
                seg = [];
            }
        });
        if (seg.length)
            segments.push(seg);
        for (const s of segments) {
            if (s.length < 2)
                continue;
            cr.moveTo(...s[0]);
            for (const p of s.slice(1))
                cr.lineTo(...p);
            if (si === 0) {
                cr.setSourceRGBA(...color, 1);
                cr.setLineWidth(1.25);
                cr.setDash([], 0);
                cr.strokePreserve();
                cr.lineTo(s[s.length - 1][0], h);
                cr.lineTo(s[0][0], h);
                cr.closePath();
                cr.setSourceRGBA(...color, 0.16);
                cr.fill();
            } else {
                cr.setSourceRGBA(...COLORS.dim, 1);
                cr.setLineWidth(1);
                cr.setDash([3, 2], 0);
                cr.stroke();
                cr.setDash([], 0);
            }
        }
    });
}

/** Vertical bars, one per value in 0–100, e.g. per-thread CPU load. */
export function bars(cr, w, h, values) {
    const n = values.length;
    if (!n)
        return;
    const gap = n > 24 ? 1 : 2;
    const bw = Math.max(1, (w - gap * (n - 1)) / n);
    values.forEach((v, i) => {
        const f = Math.min(1, Math.max(0, v / 100));
        const bh = Math.max(2, f * h);
        cr.setSourceRGBA(...COLORS.accent, 0.35 + f * 0.65);
        cr.rectangle(i * (bw + gap), h - bh, bw, bh);
        cr.fill();
    });
}

/** A horizontal stacked bar: parts are [fraction, [r,g,b], alpha]. */
export function stack(cr, w, h, parts) {
    cr.setSourceRGBA(1, 1, 1, 0.07);
    cr.rectangle(0, 0, w, h);
    cr.fill();
    let x = 0;
    for (const [f, rgb, a] of parts) {
        const pw = Math.max(0, Math.min(1, f)) * w;
        cr.setSourceRGBA(...rgb, a);
        cr.rectangle(x, 0, pw, h);
        cr.fill();
        x += pw;
    }
}
