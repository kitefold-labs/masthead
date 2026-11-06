// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! Fixed-size history: 15 minutes at 1 s and 24 hours at 1 min.
//!
//! The coarse ring keeps the minimum and maximum of each minute so a peak in a
//! one-hour chart is never averaged away. Missing seconds (the session was
//! locked, the device was suspended) are stored as NaN and drawn as gaps.

/// A circular buffer of fixed capacity. Pushing never allocates.
#[derive(Debug, Clone)]
pub struct Ring<T: Copy> {
    data: Vec<T>,
    head: usize,
    len: usize,
}

impl<T: Copy> Ring<T> {
    pub fn new(capacity: usize, fill: T) -> Self {
        Self { data: vec![fill; capacity.max(1)], head: 0, len: 0 }
    }

    pub fn capacity(&self) -> usize {
        self.data.len()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn push(&mut self, v: T) {
        self.data[self.head] = v;
        self.head = (self.head + 1) % self.data.len();
        self.len = (self.len + 1).min(self.data.len());
    }

    /// The most recent `n` values, oldest first.
    pub fn last_n(&self, n: usize) -> impl Iterator<Item = T> + '_ {
        let n = n.min(self.len);
        let cap = self.data.len();
        let start = (self.head + cap - n) % cap;
        (0..n).map(move |i| self.data[(start + i) % cap])
    }

    pub fn latest(&self) -> Option<T> {
        if self.len == 0 { None } else { Some(self.data[(self.head + self.data.len() - 1) % self.data.len()]) }
    }
}

pub const FINE_SECONDS: usize = 15 * 60;
pub const COARSE_MINUTES: usize = 24 * 60;

/// One metric series at two resolutions.
#[derive(Debug, Clone)]
pub struct History {
    fine: Ring<f32>,
    coarse: Ring<(f32, f32)>,
    minute: Option<(f32, f32)>,
    in_minute: u32,
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

impl History {
    pub fn new() -> Self {
        Self {
            fine: Ring::new(FINE_SECONDS, f32::NAN),
            coarse: Ring::new(COARSE_MINUTES, (f32::NAN, f32::NAN)),
            minute: None,
            in_minute: 0,
        }
    }

    /// Record one second. NaN marks a gap.
    pub fn push(&mut self, v: f32) {
        self.fine.push(v);
        if !v.is_nan() {
            self.minute = Some(match self.minute {
                Some((lo, hi)) => (lo.min(v), hi.max(v)),
                None => (v, v),
            });
        }
        self.in_minute += 1;
        if self.in_minute == 60 {
            self.coarse.push(self.minute.take().unwrap_or((f32::NAN, f32::NAN)));
            self.in_minute = 0;
        }
    }

    /// Record `secs` missing seconds.
    pub fn push_gap(&mut self, secs: u64) {
        // Beyond a day nothing survives in either ring; cap the work.
        let n = secs.min((COARSE_MINUTES * 60) as u64);
        for _ in 0..n {
            self.push(f32::NAN);
        }
    }

    pub fn latest(&self) -> Option<f32> {
        self.fine.latest()
    }

    /// The last `n` one-second values.
    pub fn recent(&self, n: usize) -> Vec<f32> {
        self.fine.last_n(n).collect()
    }

    /// History for a chart covering `seconds`: `(step_seconds, min, max)`.
    /// Up to 15 minutes comes from the 1 s ring (min equals max); longer
    /// ranges come from the 1 min ring.
    pub fn range(&self, seconds: u32) -> (f64, Vec<f64>, Vec<f64>) {
        let secs = seconds as usize;
        if secs <= FINE_SECONDS {
            let v: Vec<f64> = self.fine.last_n(secs).map(f64::from).collect();
            (1.0, v.clone(), v)
        } else {
            let n = secs.div_ceil(60);
            let (lo, hi) = self.coarse.last_n(n).map(|(a, b)| (f64::from(a), f64::from(b))).unzip();
            (60.0, lo, hi)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_wraps() {
        let mut r = Ring::new(3, 0);
        assert!(r.is_empty());
        assert_eq!(r.latest(), None);
        for i in 1..=5 {
            r.push(i);
        }
        assert_eq!(r.len(), 3);
        assert_eq!(r.capacity(), 3);
        assert_eq!(r.last_n(10).collect::<Vec<_>>(), vec![3, 4, 5]);
        assert_eq!(r.last_n(2).collect::<Vec<_>>(), vec![4, 5]);
        assert_eq!(r.latest(), Some(5));
    }

    #[test]
    fn history_minutes_keep_peaks() {
        let mut h = History::new();
        for i in 0..120 {
            h.push(if i == 30 { 99.0 } else { 10.0 });
        }
        let (step, lo, hi) = h.range(3600);
        assert!((step - 60.0).abs() < f64::EPSILON);
        assert_eq!(lo.len(), 2);
        assert!((hi[0] - 99.0).abs() < 1e-6);
        assert!((lo[0] - 10.0).abs() < 1e-6);
        let (step, lo, _) = h.range(60);
        assert!((step - 1.0).abs() < f64::EPSILON);
        assert_eq!(lo.len(), 60);
        assert_eq!(h.recent(5).len(), 5);
        assert_eq!(h.latest(), Some(10.0));
    }

    #[test]
    fn gaps() {
        let mut h = History::new();
        h.push(1.0);
        h.push_gap(59);
        h.push_gap(60);
        let (_, lo, hi) = h.range(7200);
        assert_eq!(lo.len(), 2);
        assert!((lo[0] - 1.0).abs() < 1e-6 && (hi[0] - 1.0).abs() < 1e-6);
        assert!(lo[1].is_nan());
        assert!(h.latest().unwrap().is_nan());
        h.push_gap(u64::MAX);
        assert_eq!(h.recent(10_000).len(), FINE_SECONDS);
    }
}
