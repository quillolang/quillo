//! Jobs the server runs on a schedule.
//!
//! ```text
//! schedule("5m", clean_sessions)              // every 5 minutes, on the clock (:00, :05, ...)
//! schedule("0 3 * * *", nightly_report)       // cron: at 03:00 UTC
//! schedule("@daily", () => db.exec("delete from tokens where expires < ?", now()))
//! ```
//!
//! A thread keeps time and, when a job is due, asks the server to run it
//! (`POST /_quillo/job/<name>`, with a secret only this process knows), so
//! a job runs like a request: on the event loop, waiting for the database
//! without holding anyone up, and finishing before the server stops. A job
//! still running when it is due again is not started twice.
//!
//! Several servers on one database run each job once per time: the first
//! to record the time in `_quillo_jobs` runs it, the others skip it.
//! `quillo run --no-jobs` runs none here.
//!
//! `every(ms, f)` at the top of a program is a tick: `f` every `ms`
//! milliseconds, on the clock, for a game's loop or a simulation's step
//! (FEEDBACK12.md, Q-080; branch feedback12-fixes). A thread of its own
//! sleeps to each next time and asks the server to run it, as a job is run
//! (`POST /_quillo/tick/<name>`), and waits for it: one that takes longer than
//! its interval skips the times it missed rather than piling up. Ticks are
//! each process's own (no `_quillo_jobs`), are not logged, and stop with the
//! program's version: a reload ends them and starts the new version's.
//!
//! The jobs and ticks of the services a program runs in its process are
//! scheduled with its own, as `<service>~<name>` (FEEDBACK12.md, Q-083).

use crate::interp::{fail, Ctrl, Interp};
use crate::value::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

type R = Result<Value, Ctrl>;

/// Set by `--no-jobs`.
pub static DISABLED: AtomicBool = AtomicBool::new(false);

pub const BUILTINS: &[(&str, BuiltinFn)] = &[("schedule", schedule)];

#[derive(Clone, Debug, PartialEq)]
pub enum When {
    /// Every so many seconds, counted from 1970 (so servers agree on when).
    Every(u64),
    Cron(Cron),
}

pub struct Job {
    pub name: String,
    pub when: When,
    pub f: Value,
}

/// What `every(ms, f)` at the top of a program set to run (see the top).
pub struct Tick {
    pub name: String,
    pub ms: u64,
    pub f: Value,
}

/// `every(ms, f, { name })` at the top of a program, outside any request:
/// registers a tick (the browser's `every` is compiled for components, and
/// one met while rendering on the server does nothing).
pub(crate) fn every_at_top(it: &mut Interp, args: &[Value], line: u32) -> R {
    let ms = match args.first() {
        Some(Value::Int(n)) if *n >= 1 => *n as u64,
        Some(Value::Float(f)) if *f >= 1.0 => *f as u64,
        _ => return fail(line, "`every` on the server takes at least 1 millisecond: every(50, step)"),
    };
    let f = match args.get(1) {
        Some(f @ (Value::Func(_) | Value::Builtin(..))) => f.clone(),
        _ => return fail(line, "`every` takes milliseconds and a function: every(100, step)"),
    };
    let named = match args.get(2) {
        Some(Value::Map(o)) => match o.get("name") {
            Some(Value::Str(n)) => Some(n.to_string()),
            None | Some(Value::Nil) => None,
            Some(v) => return fail(line, format!("`every`'s name is a str, got {}", v.type_name())),
        },
        None | Some(Value::Nil) => None,
        Some(v) => return fail(line, format!("`every` expects options as a map, got {}", v.type_name())),
    };
    let name = named.unwrap_or_else(|| match &f {
        Value::Func(c) if name_ok(&c.def.name) => c.def.name.clone(),
        _ => format!("every-{}", line),
    });
    if !name_ok(&name) {
        return fail(line, format!("`every`: a tick's name is letters, digits, `_`, `-` and `.`, got {:?}", name));
    }
    let site = Arc::make_mut(&mut it.site);
    if site.ticks.iter().any(|t| t.name == name) {
        return fail(line, format!("`every`: there is already a tick named {:?}; give this one another with {{ name: ... }}", name));
    }
    site.ticks.push(Arc::new(Tick { name, ms, f }));
    Ok(Value::Nil)
}

/// The secret a job request carries.
pub fn token() -> &'static str {
    static TOKEN: OnceLock<String> = OnceLock::new();
    TOKEN.get_or_init(|| crate::crypto::b64url(&crate::crypto::secret_bytes(32)))
}

fn name_ok(n: &str) -> bool {
    !n.is_empty() && n.len() <= 64 && n.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
}

/// `schedule(when, f, { name })`, at the top of a program.
fn schedule(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let when = match args.first() {
        Some(Value::Str(s)) if s.contains(' ') || s.starts_with('@') => match Cron::parse(s) {
            Ok(c) => When::Cron(c),
            Err(e) => return fail(line, format!("`schedule`: {}", e)),
        },
        Some(v @ (Value::Str(_) | Value::Int(_))) => match crate::builtins::duration_seconds(v) {
            Some(n) if n > 0 => When::Every(n),
            _ => return fail(line, format!("`schedule` takes a duration such as \"5m\" or a cron line such as \"0 3 * * *\", got {}", v.repr())),
        },
        other => {
            return fail(line, format!("`schedule` takes a duration or a cron line first, got {}", other.map_or("nothing", |v| v.type_name())))
        }
    };
    let f = match args.get(1) {
        Some(f @ (Value::Func(_) | Value::Builtin(..))) => f.clone(),
        other => return fail(line, format!("`schedule` needs a function to run, got {}", other.map_or("nothing", |v| v.type_name()))),
    };
    let named = match args.get(2) {
        Some(Value::Map(o)) => match o.get("name") {
            Some(Value::Str(n)) => Some(n.to_string()),
            None | Some(Value::Nil) => None,
            Some(v) => return fail(line, format!("`schedule`'s name is a str, got {}", v.type_name())),
        },
        None | Some(Value::Nil) => None,
        Some(v) => return fail(line, format!("`schedule` expects options as a map, got {}", v.type_name())),
    };
    let name = named.unwrap_or_else(|| match &f {
        Value::Func(c) if name_ok(&c.def.name) => c.def.name.clone(),
        _ => format!("job-{}", line),
    });
    if !name_ok(&name) {
        return fail(line, format!("`schedule`: a job's name is letters, digits, `_`, `-` and `.`, got {:?}", name));
    }
    let site = Arc::make_mut(&mut it.site);
    if site.jobs.iter().any(|j| j.name == name) {
        return fail(line, format!("`schedule`: there is already a job named {:?}; give this one another with {{ name: ... }}", name));
    }
    site.jobs.push(Arc::new(Job { name, when, f }));
    Ok(Value::Nil)
}

/// A cron line, `minute hour day-of-month month day-of-week`, in UTC.
#[derive(Clone, Debug, PartialEq)]
pub struct Cron {
    minutes: u64,
    hours: u64,
    days: u64,
    months: u64,
    weekdays: u64,
    /// Whether day of month and day of week were both given, in which case
    /// either may match (as in Vixie cron).
    either_day: bool,
}

impl Cron {
    pub fn parse(s: &str) -> Result<Cron, String> {
        let line = match s.trim() {
            "@yearly" | "@annually" => "0 0 1 1 *",
            "@monthly" => "0 0 1 * *",
            "@weekly" => "0 0 * * 0",
            "@daily" | "@midnight" => "0 0 * * *",
            "@hourly" => "0 * * * *",
            other if other.starts_with('@') => return Err(format!("unknown {:?}; @hourly, @daily, @weekly, @monthly and @yearly are known", other)),
            other => other,
        };
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() != 5 {
            return Err(format!("a cron line has 5 fields (minute hour day month weekday), got {:?}", s));
        }
        let weekdays = field(f[4], 0, 7, "weekday")?;
        Ok(Cron {
            minutes: field(f[0], 0, 59, "minute")?,
            hours: field(f[1], 0, 23, "hour")?,
            days: field(f[2], 1, 31, "day")?,
            months: field(f[3], 1, 12, "month")?,
            // 7 is Sunday too.
            weekdays: (weekdays | (weekdays >> 7)) & 0x7f,
            either_day: f[2] != "*" && f[4] != "*",
        })
    }

    /// Whether the minute starting at `secs` (since 1970, UTC) matches.
    pub fn matches(&self, secs: u64) -> bool {
        let days = (secs / 86400) as i64;
        let (_, month, day) = crate::httpd::civil(days);
        let weekday = (days + 4).rem_euclid(7); // 1970-01-01 was a Thursday
        let bit = |set: u64, n: i64| set & (1 << n) != 0;
        let (dom, dow) = (bit(self.days, day), bit(self.weekdays, weekday));
        bit(self.minutes, ((secs / 60) % 60) as i64)
            && bit(self.hours, ((secs / 3600) % 24) as i64)
            && bit(self.months, month)
            && if self.either_day { dom || dow } else { dom && dow }
    }
}

/// `*`, `5`, `1-5`, `*/15`, `0-30/10`, and lists of those.
fn field(s: &str, lo: u64, hi: u64, what: &str) -> Result<u64, String> {
    let bad = || format!("the {} field {:?} is not a number from {} to {}, a range, a list or a step", what, s, lo, hi);
    let mut set = 0u64;
    for part in s.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((r, st)) => (r, st.parse::<u64>().ok().filter(|&n| n > 0).ok_or_else(bad)?),
            None => (part, 1),
        };
        let (a, b) = if range == "*" {
            (lo, hi)
        } else if let Some((a, b)) = range.split_once('-') {
            (a.parse().map_err(|_| bad())?, b.parse().map_err(|_| bad())?)
        } else {
            let n: u64 = range.parse().map_err(|_| bad())?;
            (n, if step > 1 { hi } else { n })
        };
        if a < lo || b > hi || a > b {
            return Err(bad());
        }
        let mut n = a;
        while n <= b {
            set |= 1 << n;
            n += step;
        }
    }
    Ok(set)
}

/// The time a job is due at or before `now`, if it is due in this second's
/// minute or interval.
pub fn slot(when: &When, now: u64) -> Option<u64> {
    match when {
        When::Every(n) => Some(now / n * n),
        When::Cron(c) => {
            let minute = now / 60 * 60;
            c.matches(minute).then_some(minute)
        }
    }
}

/// Runs the scheduler: `jobs` gives the program's jobs as they are now
/// (they change when `--watch` reloads it), and each due job is asked of
/// the server at `addr`.
pub fn start(addr: std::net::SocketAddr, jobs: impl Fn() -> Vec<(String, When)> + Send + 'static) {
    if DISABLED.load(Ordering::Relaxed) {
        return;
    }
    let addr = loopback(addr);
    let spawned = std::thread::Builder::new().name("quillo-jobs".into()).spawn(move || {
        use std::collections::{HashMap, HashSet};
        let mut last: HashMap<String, u64> = HashMap::new();
        let running: Arc<std::sync::Mutex<HashSet<String>>> = Default::default();
        // Jobs whose skipping was said: once for each run that goes on
        // (FEEDBACK12.md, Q-081; branch feedback12-fixes).
        let mut said: HashSet<String> = HashSet::new();
        loop {
            if crate::httpd::stopping() {
                return;
            }
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            for (name, when) in jobs() {
                let Some(due) = slot(&when, now) else { continue };
                // A job first seen now waits for its next time.
                let prev = last.entry(name.clone()).or_insert(due);
                if due <= *prev {
                    continue;
                }
                *prev = due;
                if !running.lock().unwrap_or_else(|e| e.into_inner()).insert(name.clone()) {
                    if said.insert(name.clone()) {
                        eprintln!("quillo: job {} is still running; skipped until it ends", name);
                    }
                    continue;
                }
                said.remove(&name);
                let running = running.clone();
                std::thread::spawn(move || {
                    if let Err(e) = ask(addr, "job", &name, due) {
                        eprintln!("quillo: job {} could not start: {}", name, e);
                    }
                    running.lock().unwrap_or_else(|e| e.into_inner()).remove(&name);
                });
            }
            // Wake just after the next second starts.
            let into = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_millis());
            std::thread::sleep(std::time::Duration::from_millis(1005 - into as u64));
        }
    });
    if let Err(e) = spawned {
        eprintln!("quillo: cannot start the job scheduler: {}", e);
    }
}

/// Runs the program's ticks: `ticks` gives them as they are now (name and
/// interval; they change when a reload makes a new version), and each has a
/// thread that sleeps to its next time on the clock and asks the server at
/// `addr` to run it, waiting for it to end. A tick gone from the list (or whose
/// interval changed) stops; a new one starts within a tenth of a second.
pub fn start_ticks(addr: std::net::SocketAddr, ticks: impl Fn() -> Vec<(String, u64)> + Send + Sync + 'static) {
    use std::collections::HashMap;
    use std::sync::atomic::AtomicBool;
    let addr = loopback(addr);
    let ticks = Arc::new(ticks);
    let spawned = std::thread::Builder::new().name("quillo-ticks".into()).spawn(move || {
        // Each running tick, by name: its interval and whether it is to go on.
        let mut threads: HashMap<String, (u64, Arc<AtomicBool>)> = HashMap::new();
        loop {
            if crate::httpd::stopping() {
                for (_, (_, on)) in threads.drain() {
                    on.store(false, Ordering::Relaxed);
                }
                return;
            }
            let now: Vec<(String, u64)> = ticks();
            threads.retain(|name, (ms, on)| {
                let keep = now.iter().any(|(n, m)| n == name && m == ms);
                if !keep {
                    on.store(false, Ordering::Relaxed);
                }
                keep
            });
            for (name, ms) in now {
                if threads.contains_key(&name) {
                    continue;
                }
                let on = Arc::new(AtomicBool::new(true));
                threads.insert(name.clone(), (ms, on.clone()));
                let spawned = std::thread::Builder::new().name(format!("quillo-tick-{}", name)).spawn(move || run_tick(addr, &name, ms, &on));
                if let Err(e) = spawned {
                    eprintln!("quillo: cannot start a thread for tick: {}", e);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    });
    if let Err(e) = spawned {
        eprintln!("quillo: cannot start the ticks: {}", e);
    }
}

/// One tick's thread: to each next multiple of `ms` (in milliseconds since
/// 1970, so it does not drift), then run it, while `on` holds.
fn run_tick(addr: std::net::SocketAddr, name: &str, ms: u64, on: &std::sync::atomic::AtomicBool) {
    let epoch_ms = || std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    let mut said = false;
    while on.load(Ordering::Relaxed) && !crate::httpd::stopping() {
        let now = epoch_ms();
        let due = (now / ms + 1) * ms;
        // In steps, so a tick that stops (a reload, the server stopping) stops soon.
        loop {
            let now = epoch_ms();
            if now >= due || !on.load(Ordering::Relaxed) || crate::httpd::stopping() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis((due - now).min(100)));
        }
        if !on.load(Ordering::Relaxed) || crate::httpd::stopping() {
            return;
        }
        match ask(addr, "tick", name, due) {
            Ok(()) => said = false,
            Err(e) if !said => {
                eprintln!("quillo: tick {} could not run: {}", name, e);
                said = true;
            }
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(ms.max(100))),
        }
    }
}

/// Where this process reaches the server listening on `addr`: itself, for
/// a server listening on every address.
pub fn loopback(addr: std::net::SocketAddr) -> std::net::SocketAddr {
    match addr.ip() {
        ip if ip.is_unspecified() && ip.is_ipv4() => std::net::SocketAddr::from(([127, 0, 0, 1], addr.port())),
        ip if ip.is_unspecified() => std::net::SocketAddr::from((std::net::Ipv6Addr::LOCALHOST, addr.port())),
        _ => addr,
    }
}

/// Asks the server to run a job (`kind` "job") or a tick ("tick"), and waits
/// for it to finish.
fn ask(addr: std::net::SocketAddr, kind: &str, name: &str, due: u64) -> std::io::Result<()> {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(5))?;
    write!(
        s,
        "POST /_quillo/{}/{}?at={} HTTP/1.1\r\nHost: localhost\r\nX-Quillo-Job: {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        kind,
        name,
        due,
        token()
    )?;
    let mut out = Vec::new();
    s.read_to_end(&mut out)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cron_lines() {
        // 2026-10-24 (a Saturday) 03:00 UTC.
        let sat_3am = 1_792_810_800;
        assert!(Cron::parse("0 3 * * *").unwrap().matches(sat_3am));
        assert!(!Cron::parse("0 3 * * *").unwrap().matches(sat_3am + 60));
        assert!(Cron::parse("*/15 * * * *").unwrap().matches(sat_3am + 15 * 60));
        assert!(!Cron::parse("*/15 * * * *").unwrap().matches(sat_3am + 10 * 60));
        assert!(Cron::parse("0 3 * * 6").unwrap().matches(sat_3am));
        assert!(Cron::parse("0 3 * * 1-5").unwrap().matches(sat_3am) == false);
        assert!(Cron::parse("0 3 24 10 *").unwrap().matches(sat_3am));
        // Day of month or day of week, when both are given.
        assert!(Cron::parse("0 3 1 * 6").unwrap().matches(sat_3am));
        assert!(Cron::parse("0 0 * * 7").unwrap().matches(sat_3am + 21 * 3600));
        assert!(Cron::parse("@daily").unwrap().matches(sat_3am - 3 * 3600));
        assert!(Cron::parse("0 3 * *").is_err() && Cron::parse("61 * * * *").is_err() && Cron::parse("@often").is_err());
        assert_eq!(slot(&When::Every(300), 1_000_123), Some(999_900));
    }
}
