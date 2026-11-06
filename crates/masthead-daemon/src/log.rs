// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! Logging to stderr. Under systemd (`JOURNAL_STREAM` set) each line carries a
//! `<N>` priority prefix so journald records the right severity.

use log::{Level, LevelFilter, Log, Metadata, Record};

struct Stderr {
    journal: bool,
}

impl Log for Stderr {
    fn enabled(&self, m: &Metadata<'_>) -> bool {
        m.level() <= log::max_level()
    }

    fn log(&self, r: &Record<'_>) {
        if !self.enabled(r.metadata()) {
            return;
        }
        if self.journal {
            let p = match r.level() {
                Level::Error => 3,
                Level::Warn => 4,
                Level::Info => 6,
                Level::Debug | Level::Trace => 7,
            };
            eprintln!("<{p}>{}", r.args());
        } else {
            eprintln!("masthead-daemon {}: {}", r.level().as_str().to_lowercase(), r.args());
        }
    }

    fn flush(&self) {}
}

pub fn init() {
    let level = match std::env::var("MASTHEAD_LOG").ok().as_deref() {
        Some("error") => LevelFilter::Error,
        Some("debug") => LevelFilter::Debug,
        Some("trace") => LevelFilter::Trace,
        Some("off") => LevelFilter::Off,
        Some("info") => LevelFilter::Info,
        _ => LevelFilter::Warn,
    };
    let journal = std::env::var_os("JOURNAL_STREAM").is_some();
    if log::set_boxed_logger(Box::new(Stderr { journal })).is_ok() {
        log::set_max_level(level);
    }
}
