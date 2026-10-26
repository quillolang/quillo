//! What the server counts for `GET /_quillo/metrics` (with `--metrics`), in
//! Prometheus's text format, and the request log's shape (`--log`) and
//! where it goes (`--log-file`, `--log-stdout`). Each request adds to a few
//! atomic counters; nothing else is kept.
//!
//! The request log goes to stderr unless told otherwise. With `--log-file
//! logs/requests.log` it goes to a file a day (`logs/requests-2026-10-27.log`,
//! by the UTC date), and with `--log-keep 90` files older than 90 days are
//! removed as a new day's starts. `--log-stdout` sends it to stdout (as well
//! as the file, with both). Lines are written in batches, at least once a
//! second.

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
/// `--log-query`: JSON lines keep the query string (its sensitive values
/// masked); without it, `path` is the path alone.
pub static LOG_QUERY: AtomicBool = AtomicBool::new(false);
/// `--log-stdout`: the request log on stdout.
pub static LOG_STDOUT: AtomicBool = AtomicBool::new(false);

/// `--log-file`: a file a day, and how many days of them are kept (0: all).
struct Daily {
    base: std::path::PathBuf,
    keep: u32,
    day: i64,
    file: Option<std::fs::File>,
}

static LOG_FILE: std::sync::Mutex<Option<Daily>> = std::sync::Mutex::new(None);
/// Lines not written yet, for the file and stdout.
static PENDING: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// Sends the request log to files named after `base`, one a day, keeping
/// `keep` days of them (0: all). Checks that the first can be written.
pub fn log_to_file(base: &std::path::Path, keep: u32) -> Result<(), String> {
    let mut d = Daily { base: base.to_path_buf(), keep, day: -1, file: None };
    d.open(today())?;
    *LOG_FILE.lock().unwrap_or_else(|e| e.into_inner()) = Some(d);
    Ok(())
}

fn today() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64) / 86400
}

impl Daily {
    /// The file for `day`: the base name with the date before its extension.
    fn path_for(&self, day: i64) -> std::path::PathBuf {
        let (y, m, d) = crate::httpd::civil(day);
        let stem = self.base.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let ext = self.base.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
        self.base.with_file_name(format!("{}-{}-{:02}-{:02}{}", stem, y, m, d, ext))
    }

    fn open(&mut self, day: i64) -> Result<(), String> {
        let p = self.path_for(day);
        if let Some(dir) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
        }
        let f = std::fs::OpenOptions::new().create(true).append(true).open(&p).map_err(|e| format!("{}: {}", p.display(), e))?;
        self.file = Some(f);
        self.day = day;
        self.prune(day);
        Ok(())
    }

    /// Removes the days' files older than `keep` days.
    fn prune(&self, day: i64) {
        if self.keep == 0 {
            return;
        }
        let Some(dir) = self.base.parent().map(|d| if d.as_os_str().is_empty() { std::path::Path::new(".") } else { d }) else { return };
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        // Names sort by their dates: the oldest kept is today's less
        // `keep - 1` days.
        let oldest = self.path_for(day - self.keep as i64 + 1).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let stem = self.base.file_stem().map(|s| format!("{}-", s.to_string_lossy())).unwrap_or_default();
        let ext = self.base.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Some(date) = name.strip_prefix(&stem).and_then(|r| r.strip_suffix(&ext)) else { continue };
            let dated = date.len() == 10 && date.bytes().enumerate().all(|(i, b)| if i == 4 || i == 7 { b == b'-' } else { b.is_ascii_digit() });
            if dated && name < oldest {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

/// Adds a line to the request log: to stderr (in batches), or to the file
/// and stdout when the log goes there.
pub fn log_line(line: std::fmt::Arguments) {
    let to_file = LOG_FILE.lock().unwrap_or_else(|e| e.into_inner()).is_some();
    if !to_file && !LOG_STDOUT.load(Relaxed) {
        crate::httpd::log_fmt(line);
        return;
    }
    use std::fmt::Write as _;
    let mut p = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    let _ = p.write_fmt(line);
    if p.len() > 32 << 10 {
        let text = std::mem::take(&mut *p);
        drop(p);
        write_out(&text);
    }
}

/// Writes out what `log_line` kept (called with the stderr log's batches).
pub fn flush() {
    let text = std::mem::take(&mut *PENDING.lock().unwrap_or_else(|e| e.into_inner()));
    if !text.is_empty() {
        write_out(&text);
    }
}

fn write_out(text: &str) {
    use std::io::Write;
    if LOG_STDOUT.load(Relaxed) {
        let _ = std::io::stdout().lock().write_all(text.as_bytes());
    }
    let mut g = LOG_FILE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(d) = g.as_mut() {
        let day = today();
        if day != d.day {
            if let Err(e) = d.open(day) {
                eprintln!("quillo: the request log: {}", e);
            }
        }
        if let Some(f) = d.file.as_mut() {
            if let Err(e) = f.write_all(text.as_bytes()) {
                eprintln!("quillo: the request log: {}", e);
            }
        }
    }
}

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
static STARTED: OnceLock<Instant> = OnceLock::new();

/// Starts the clock for `quillo_uptime_seconds`.
pub fn start() {
    STARTED.get_or_init(Instant::now);
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
