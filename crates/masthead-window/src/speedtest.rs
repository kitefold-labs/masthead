// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! The Network page's speed test, against Cloudflare's speed test service
//! (speed.cloudflare.com). It runs only when the user starts it.
//!
//! Latency is the TCP handshake time to the server. Download and upload each
//! send one request at a time, stepping up the request size until a request
//! takes long enough to measure, and stop at a time or byte budget. The result for a direction
//! is the 90th percentile of its requests' rates, as Cloudflare's own test
//! reports. Nothing is written to disk.

use crate::app::Message;
use cosmic::app::Task;
use std::io::{self, Read};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

pub const HOST: &str = "speed.cloudflare.com";
const DOWN_URL: &str = "https://speed.cloudflare.com/__down";
const UP_URL: &str = "https://speed.cloudflare.com/__up";

/// Each direction stops after this long or this many bytes, whichever is
/// first. Together they bound a run at 200 MB.
const PHASE_TIME: Duration = Duration::from_secs(8);
const DOWN_BUDGET: u64 = 150_000_000;
const UP_BUDGET: u64 = 50_000_000;
/// Request sizes, smallest first. A request that ends sooner than `SETTLED`
/// moves the next one up a step. These are sizes Cloudflare's own test
/// client uses; the service refuses some others, such as 16 MB and 100 MB.
const SIZES: [u64; 3] = [1_000_000, 10_000_000, 25_000_000];
const SETTLED: Duration = Duration::from_secs(1);
/// Shorter transfers mostly measure TCP slow start; they count only when no
/// request ran this long.
const MIN_SAMPLE: Duration = Duration::from_millis(200);
const PINGS: usize = 10;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// How often a running transfer updates the live rate.
const LIVE_EVERY: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Latency,
    Download,
    Upload,
}

/// Shared between the test thread and the window.
#[derive(Debug, Default)]
pub struct Live {
    /// The running phase and its latest rate in bytes per second.
    now: Mutex<Option<(Phase, f64)>>,
    stop: AtomicBool,
}

impl Live {
    pub fn now(&self) -> Option<(Phase, f64)> {
        *self.now.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn set(&self, phase: Phase, rate: f64) {
        *self.now.lock().unwrap_or_else(PoisonError::into_inner) = Some((phase, rate));
    }

    fn stopped(&self) -> bool {
        self.stop.load(Relaxed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Outcome {
    pub latency: Duration,
    /// Mean difference between consecutive handshakes.
    pub jitter: Duration,
    /// Bytes per second.
    pub down: f64,
    pub up: f64,
}

/// One request: bytes moved and how long the body took.
#[derive(Debug, Clone, Copy)]
struct Sample {
    bytes: u64,
    time: Duration,
}

impl Sample {
    fn rate(self) -> f64 {
        self.bytes as f64 / self.time.as_secs_f64().max(1e-6)
    }
}

/// Run the test. `Ok(None)` means it was stopped.
pub fn run(live: &Live) -> Result<Option<Outcome>, String> {
    live.set(Phase::Latency, 0.0);
    let addr = (HOST, 443)
        .to_socket_addrs()
        .map_err(|e| format!("Could not look up {HOST}: {e}"))?
        .next()
        .ok_or_else(|| format!("{HOST} has no address."))?;
    let Some(pings) = ping(addr, live)? else { return Ok(None) };
    let (latency, jitter) = latency_stats(&pings);

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_global(Some(Duration::from_secs(60)))
        .user_agent(concat!("masthead/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let Some(down) = measure(live, Phase::Download, DOWN_BUDGET, |size| download(&agent, size, live))? else { return Ok(None) };
    let Some(up) = measure(live, Phase::Upload, UP_BUDGET, |size| upload(&agent, size, live))? else { return Ok(None) };
    Ok(Some(Outcome { latency, jitter, down, up }))
}

fn ping(addr: SocketAddr, live: &Live) -> Result<Option<Vec<Duration>>, String> {
    let mut out = Vec::with_capacity(PINGS);
    for _ in 0..PINGS {
        if live.stopped() {
            return Ok(None);
        }
        let t = Instant::now();
        TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).map_err(|e| format!("Could not reach {HOST}: {e}"))?;
        out.push(t.elapsed());
    }
    Ok(Some(out))
}

/// Median latency, and jitter as the mean change between consecutive pings.
fn latency_stats(pings: &[Duration]) -> (Duration, Duration) {
    let mut sorted = pings.to_vec();
    sorted.sort_unstable();
    let median = sorted.get(sorted.len() / 2).copied().unwrap_or_default();
    let diffs: Vec<Duration> = pings.windows(2).map(|w| w[0].abs_diff(w[1])).collect();
    let jitter = if diffs.is_empty() { Duration::ZERO } else { diffs.iter().sum::<Duration>() / diffs.len() as u32 };
    (median, jitter)
}

/// Run requests in one direction until the time or byte budget runs out.
/// Returns the rate in bytes per second, or `None` if stopped.
fn measure(
    live: &Live,
    phase: Phase,
    budget: u64,
    mut transfer: impl FnMut(u64) -> Result<Option<Sample>, String>,
) -> Result<Option<f64>, String> {
    live.set(phase, 0.0);
    let start = Instant::now();
    let mut step = 0;
    let mut total = 0;
    let mut samples = Vec::new();
    while start.elapsed() < PHASE_TIME && total + SIZES[step] <= budget {
        let Some(s) = transfer(SIZES[step])? else { return Ok(None) };
        total += SIZES[step];
        live.set(phase, s.rate());
        if s.time < SETTLED {
            step = (step + 1).min(SIZES.len() - 1);
        }
        samples.push(s);
    }
    Ok(Some(summarize(&samples)))
}

/// The 90th percentile of the requests that ran at least `MIN_SAMPLE`, or of
/// all of them when none did.
fn summarize(samples: &[Sample]) -> f64 {
    let long: Vec<f64> = samples.iter().filter(|s| s.time >= MIN_SAMPLE).map(|s| s.rate()).collect();
    let mut rates = if long.is_empty() { samples.iter().map(|s| s.rate()).collect() } else { long };
    if rates.is_empty() {
        return 0.0;
    }
    rates.sort_by(f64::total_cmp);
    let i = (rates.len() * 9).div_ceil(10).saturating_sub(1);
    rates[i]
}

fn download(agent: &ureq::Agent, size: u64, live: &Live) -> Result<Option<Sample>, String> {
    let mut resp = agent.get(DOWN_URL).query("bytes", size.to_string()).call().map_err(|e| http_error("Download", &e))?;
    let mut body = resp.body_mut().as_reader();
    let mut buf = vec![0u8; 64 * 1024];
    // Timed from the first byte, so the request's round trip is not counted.
    let mut first: Option<Instant> = None;
    let mut bytes = 0u64;
    let mut shown = Instant::now();
    loop {
        if live.stopped() {
            return Ok(None);
        }
        let n = body.read(&mut buf).map_err(|e| format!("Download failed: {e}"))?;
        if n == 0 {
            break;
        }
        match first {
            None => first = Some(Instant::now()),
            Some(t) => {
                bytes += n as u64;
                if shown.elapsed() >= LIVE_EVERY {
                    shown = Instant::now();
                    live.set(Phase::Download, Sample { bytes, time: t.elapsed() }.rate());
                }
            }
        }
    }
    Ok(Some(Sample { bytes, time: first.map(|t| t.elapsed()).unwrap_or_default() }))
}

fn upload(agent: &ureq::Agent, size: u64, live: &Live) -> Result<Option<Sample>, String> {
    let mut zeros = Zeros { left: size, sent: 0, first: None, shown: Instant::now(), live };
    let res = agent
        .post(UP_URL)
        .header("content-type", "application/octet-stream")
        .header("content-length", size.to_string())
        .send(ureq::SendBody::from_reader(&mut zeros));
    if live.stopped() {
        return Ok(None);
    }
    res.map_err(|e| http_error("Upload", &e))?;
    // Timed from the first byte sent to the server's reply.
    Ok(Some(Sample { bytes: size, time: zeros.first.map(|t| t.elapsed()).unwrap_or_default() }))
}

fn http_error(what: &str, e: &ureq::Error) -> String {
    match e {
        ureq::Error::StatusCode(429) => "Cloudflare is limiting speed tests from this network. Try again in a few minutes.".to_owned(),
        ureq::Error::StatusCode(403) => "Cloudflare refused the test (HTTP 403).".to_owned(),
        _ => format!("{what} failed: {e}"),
    }
}

/// An upload body of `left` zero bytes that reports progress and can stop.
struct Zeros<'a> {
    left: u64,
    sent: u64,
    first: Option<Instant>,
    shown: Instant,
    live: &'a Live,
}

impl Read for Zeros<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.live.stopped() {
            return Err(io::Error::other("stopped"));
        }
        let n = buf.len().min(usize::try_from(self.left).unwrap_or(usize::MAX));
        buf[..n].fill(0);
        self.left -= n as u64;
        self.sent += n as u64;
        let first = *self.first.get_or_insert_with(Instant::now);
        if self.shown.elapsed() >= LIVE_EVERY {
            self.shown = Instant::now();
            self.live.set(Phase::Upload, Sample { bytes: self.sent, time: first.elapsed() }.rate());
        }
        Ok(n)
    }
}

// --- window state -----------------------------------------------------------

#[derive(Debug, Clone)]
pub enum SpeedMsg {
    Start,
    Stop,
    /// Redraws the live rate.
    Tick,
    Done(Result<Option<Outcome>, String>),
}

/// The speed test card's state. Results last until the window closes.
#[derive(Debug, Default)]
pub struct Speed {
    /// Set while a test runs.
    pub live: Option<Arc<Live>>,
    pub result: Option<Result<Outcome, String>>,
}

impl Speed {
    pub fn running(&self) -> bool {
        self.live.is_some()
    }

    pub fn update(&mut self, msg: SpeedMsg) -> Task<Message> {
        match msg {
            SpeedMsg::Start if !self.running() => {
                let live = Arc::new(Live::default());
                self.live = Some(live.clone());
                return cosmic::task::future(async move {
                    let r = tokio::task::spawn_blocking(move || run(&live)).await.unwrap_or_else(|e| Err(e.to_string()));
                    Message::Speed(SpeedMsg::Done(r))
                });
            }
            SpeedMsg::Stop => {
                if let Some(l) = &self.live {
                    l.stop.store(true, Relaxed);
                }
            }
            SpeedMsg::Done(r) => {
                self.live = None;
                match r {
                    Ok(Some(o)) => self.result = Some(Ok(o)),
                    Ok(None) => {}
                    Err(e) => self.result = Some(Err(e)),
                }
            }
            SpeedMsg::Start | SpeedMsg::Tick => {}
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(v: u64) -> Duration {
        Duration::from_millis(v)
    }

    #[test]
    fn latency_is_median_and_jitter_mean_change() {
        let (lat, jit) = latency_stats(&[ms(10), ms(14), ms(12), ms(30), ms(11)]);
        assert_eq!(lat, ms(12));
        // |14-10| + |12-14| + |30-12| + |11-30| = 4 + 2 + 18 + 19 = 43, over 4.
        assert_eq!(jit, Duration::from_micros(10_750));
        assert_eq!(latency_stats(&[]), (Duration::ZERO, Duration::ZERO));
    }

    #[test]
    fn summary_ignores_short_requests_unless_all_are_short() {
        let s = |bytes, t| Sample { bytes, time: ms(t) };
        // 1 MB in 50 ms is 20 MB/s but too short to count.
        let mixed = [s(1_000_000, 50), s(4_000_000, 400), s(16_000_000, 1600), s(16_000_000, 1000)];
        assert!((summarize(&mixed) - 16_000_000.0).abs() < 1.0);
        let short = [s(1_000_000, 100), s(1_000_000, 50)];
        assert!((summarize(&short) - 20_000_000.0).abs() < 1.0);
        assert!(summarize(&[]).abs() < f64::EPSILON);
    }

    #[test]
    fn requests_grow_and_stay_within_the_budget() {
        let live = Live::default();
        let mut sizes = Vec::new();
        // A link that moves 20 MB per second.
        let rate = measure(&live, Phase::Download, 70_000_000, |size| {
            sizes.push(size);
            Ok(Some(Sample { bytes: size, time: Duration::from_secs_f64(size as f64 / 20e6) }))
        })
        .unwrap();
        // 1 + 10 + 25 + 25 = 61 MB; another 25 MB would exceed 70 MB.
        assert_eq!(sizes, [1_000_000, 10_000_000, 25_000_000, 25_000_000]);
        assert!((rate.unwrap() - 20e6).abs() < 1.0);
    }

    #[test]
    fn stopping_returns_none() {
        let live = Live::default();
        live.stop.store(true, Relaxed);
        assert_eq!(measure(&live, Phase::Upload, UP_BUDGET, |_| Ok(None)).unwrap(), None);
    }
}
