//! What the server counts for `GET /_quillo/metrics` (with `--metrics`), in
//! Prometheus's text format, and the request log's shape (`--log`). Each
//! request adds to a few atomic counters; nothing else is kept.

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicU8, Ordering::Relaxed};
use std::sync::OnceLock;
use std::time::Instant;

/// Set by `--metrics`.
pub static ENABLED: AtomicBool = AtomicBool::new(false);

/// The request log: `TEXT` (the default), `JSON` lines, or `OFF`.
pub static LOG: AtomicU8 = AtomicU8::new(TEXT);
pub const TEXT: u8 = 0;
pub const JSON: u8 = 1;
pub const OFF: u8 = 2;

/// Answers by status class: 1xx to 5xx.
static ANSWERED: [AtomicU64; 5] = [const { AtomicU64::new(0) }; 5];
/// Upper bounds (seconds) of the latency histogram's buckets.
const BUCKETS: [f64; 10] = [0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 1.0, 5.0];
/// Requests per bucket (the last for longer ones), not yet cumulative.
static LATENCY: [AtomicU64; 11] = [const { AtomicU64::new(0) }; 11];
static LATENCY_MICROS: AtomicU64 = AtomicU64::new(0);
/// Being answered now.
pub static IN_PROGRESS: AtomicI64 = AtomicI64::new(0);
/// Open client connections.
pub static CONNECTIONS: AtomicI64 = AtomicI64::new(0);
/// Pages' bundles kept in memory (FEEDBACK7.md, Q-063; branch feedback7-fixes).
pub static BUNDLES: AtomicI64 = AtomicI64::new(0);
static STARTED: OnceLock<Instant> = OnceLock::new();

/// Starts the clock for `quillo_uptime_seconds`.
pub fn start() {
    STARTED.get_or_init(Instant::now);
}

/// What became of prefetches: answered, refused, and used by a click (FEEDBACK22.md, Q-181;
/// branch feedback22-fixes).
static PREFETCHES: [AtomicU64; 3] = [const { AtomicU64::new(0) }; 3];

/// A prefetch was answered with `status`.
pub fn prefetched(status: u16) {
    PREFETCHES[if status == 503 { 1 } else { 0 }].fetch_add(1, Relaxed);
}

/// A page says it came from a prefetch.
pub fn prefetch_used() {
    PREFETCHES[2].fetch_add(1, Relaxed);
}

/// What became of tries of calls made later: delivered, to be tried again,
/// and given up on (FEEDBACK23.md, Q-184; branch feedback23-fixes).
static LATER: [AtomicU64; 3] = [const { AtomicU64::new(0) }; 3];

pub fn later(result: usize) {
    LATER[result].fetch_add(1, Relaxed);
}

/// Live data, which nothing showed from outside (FEEDBACK24.md, Q-195; branch
/// feedback24-fixes): pages waiting, those refused a place, pages woken by a
/// change, runs of loaders (made, shared) and answers (same, patch, whole).
pub static LIVE_PAGES: AtomicI64 = AtomicI64::new(0);
pub static LIVE_REFUSED: AtomicU64 = AtomicU64::new(0);
static LIVE_WAKES: AtomicU64 = AtomicU64::new(0);
static LIVE_RUNS: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
static LIVE_ANSWERS: [AtomicU64; 3] = [const { AtomicU64::new(0) }; 3];

pub fn live_woke(pages: u64) {
    LIVE_WAKES.fetch_add(pages, Relaxed);
}

pub fn live_run(shared: bool) {
    LIVE_RUNS[shared as usize].fetch_add(1, Relaxed);
}

/// 0: the same; 1: a patch; 2: the whole value.
pub fn live_answer(kind: usize) {
    LIVE_ANSWERS[kind].fetch_add(1, Relaxed);
}

/// Counts a page as waiting while it lives.
pub struct LivePage;

impl LivePage {
    pub fn new() -> LivePage {
        LIVE_PAGES.fetch_add(1, Relaxed);
        LivePage
    }
}

impl Default for LivePage {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for LivePage {
    fn drop(&mut self) {
        LIVE_PAGES.fetch_sub(1, Relaxed);
    }
}

/// A request was answered with `status` after `secs`.
pub fn answered(status: u16, secs: f64) {
    if !ENABLED.load(Relaxed) {
        return;
    }
    let class = (status / 100).clamp(1, 5) as usize - 1;
    ANSWERED[class].fetch_add(1, Relaxed);
    let b = BUCKETS.iter().position(|&le| secs <= le).unwrap_or(BUCKETS.len());
    LATENCY[b].fetch_add(1, Relaxed);
    LATENCY_MICROS.fetch_add((secs * 1e6) as u64, Relaxed);
}

/// The counters, as Prometheus reads them.
pub fn render() -> String {
    use std::fmt::Write;
    let mut s = String::new();
    s.push_str("# HELP quillo_requests_total Requests answered, by status class.\n# TYPE quillo_requests_total counter\n");
    for (i, n) in ANSWERED.iter().enumerate() {
        let _ = writeln!(s, "quillo_requests_total{{status=\"{}xx\"}} {}", i + 1, n.load(Relaxed));
    }
    s.push_str("# HELP quillo_request_duration_seconds Time to answer a request.\n# TYPE quillo_request_duration_seconds histogram\n");
    let mut total = 0;
    for (i, le) in BUCKETS.iter().enumerate() {
        total += LATENCY[i].load(Relaxed);
        let _ = writeln!(s, "quillo_request_duration_seconds_bucket{{le=\"{}\"}} {}", le, total);
    }
    total += LATENCY[BUCKETS.len()].load(Relaxed);
    let _ = writeln!(s, "quillo_request_duration_seconds_bucket{{le=\"+Inf\"}} {}", total);
    let _ = writeln!(s, "quillo_request_duration_seconds_sum {}", LATENCY_MICROS.load(Relaxed) as f64 / 1e6);
    let _ = writeln!(s, "quillo_request_duration_seconds_count {}", total);
    s.push_str("# HELP quillo_prefetches_total Pages asked for ahead of a click: answered, refused, and used by a click.\n# TYPE quillo_prefetches_total counter\n");
    for (name, n) in ["answered", "refused", "used"].iter().zip(&PREFETCHES) {
        let _ = writeln!(s, "quillo_prefetches_total{{result=\"{}\"}} {}", name, n.load(Relaxed));
    }
    s.push_str("# HELP quillo_live_pages Pages waiting for live data.\n# TYPE quillo_live_pages gauge\n");
    let _ = writeln!(s, "quillo_live_pages {}", LIVE_PAGES.load(Relaxed));
    s.push_str("# HELP quillo_live_refused_total Pages refused a place to wait for live data.\n# TYPE quillo_live_refused_total counter\n");
    let _ = writeln!(s, "quillo_live_refused_total {}", LIVE_REFUSED.load(Relaxed));
    s.push_str("# HELP quillo_live_wakes_total Pages woken by a change to what they follow.\n# TYPE quillo_live_wakes_total counter\n");
    let _ = writeln!(s, "quillo_live_wakes_total {}", LIVE_WAKES.load(Relaxed));
    s.push_str("# HELP quillo_live_runs_total Live loaders asked for: run, or given a run another page made.\n# TYPE quillo_live_runs_total counter\n");
    for (name, n) in ["ran", "shared"].iter().zip(&LIVE_RUNS) {
        let _ = writeln!(s, "quillo_live_runs_total{{outcome=\"{}\"}} {}", name, n.load(Relaxed));
    }
    s.push_str("# HELP quillo_live_answers_total Answers to live loaders: nothing new, a patch, or the whole value.\n# TYPE quillo_live_answers_total counter\n");
    for (name, n) in ["same", "patch", "whole"].iter().zip(&LIVE_ANSWERS) {
        let _ = writeln!(s, "quillo_live_answers_total{{kind=\"{}\"}} {}", name, n.load(Relaxed));
    }
    s.push_str("# HELP quillo_later_total Tries of calls made later: delivered, to be tried again, and given up on.\n# TYPE quillo_later_total counter\n");
    for (name, n) in ["delivered", "again", "failed"].iter().zip(&LATER) {
        let _ = writeln!(s, "quillo_later_total{{result=\"{}\"}} {}", name, n.load(Relaxed));
    }
    s.push_str("# HELP quillo_requests_in_progress Requests being answered.\n# TYPE quillo_requests_in_progress gauge\n");
    let _ = writeln!(s, "quillo_requests_in_progress {}", IN_PROGRESS.load(Relaxed));
    s.push_str("# HELP quillo_connections Open client connections.\n# TYPE quillo_connections gauge\n");
    let _ = writeln!(s, "quillo_connections {}", CONNECTIONS.load(Relaxed));
    s.push_str("# HELP quillo_udp_received_total UDP packets received on ports the program listens on.\n# TYPE quillo_udp_received_total counter\n");
    let _ = writeln!(s, "quillo_udp_received_total {}", crate::udp::RECEIVED.load(Relaxed));
    s.push_str("# HELP quillo_udp_dropped_total UDP packets dropped because their port's queue was full.\n# TYPE quillo_udp_dropped_total counter\n");
    let _ = writeln!(s, "quillo_udp_dropped_total {}", crate::udp::DROPPED.load(Relaxed));
    s.push_str("# HELP quillo_channel_pages Pages connected for channels.\n# TYPE quillo_channel_pages gauge\n");
    let _ = writeln!(s, "quillo_channel_pages {}", crate::wt::connected());
    s.push_str("# HELP quillo_channel_dropped_total Channel messages dropped because a queue was full.\n# TYPE quillo_channel_dropped_total counter\n");
    let _ = writeln!(s, "quillo_channel_dropped_total {}", crate::wt::DROPPED.load(Relaxed));
    s.push_str("# HELP quillo_bundles Pages' bundles kept in memory, one per set of components.\n# TYPE quillo_bundles gauge\n");
    let _ = writeln!(s, "quillo_bundles {}", BUNDLES.load(Relaxed));
    s.push_str("# HELP quillo_uptime_seconds Seconds since the server started.\n# TYPE quillo_uptime_seconds gauge\n");
    let _ = writeln!(s, "quillo_uptime_seconds {:.0}", STARTED.get().map_or(0.0, |t| t.elapsed().as_secs_f64()));
    s
}

/// Counts a request as in progress while it lives.
pub struct InProgress;

impl InProgress {
    pub fn new() -> InProgress {
        IN_PROGRESS.fetch_add(1, Relaxed);
        InProgress
    }
}

impl Default for InProgress {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for InProgress {
    fn drop(&mut self) {
        IN_PROGRESS.fetch_sub(1, Relaxed);
    }
}
