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
