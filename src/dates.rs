//! `date`: calendar dates, date-times and time zones, the same on the server
//! and in the browser. Back in Quillo after 205d7cd removed it: every app
//! since wrote its own calendar arithmetic, and one the EU's summer-time rule
//! (FEEDBACK2.md, Q-027; branch feedback6-fixes). Business days, Easter and
//! the day counts of interest were not brought back: a program does those.
//!
//! ```text
//! date.today()  date.today("Europe/Paris")  date.now()    // "2026-10-27", "2026-10-27T14:30:00Z"
//! date.add_months("2026-01-31", 1)                         // "2026-02-28": clamped, not March
//! date.to_zone("2026-10-27T14:30:00Z", "America/New_York") // "2026-10-27T10:30:00-04:00"
//! date.at("2026-11-05", "15:00", "Europe/Lisbon")         // "2026-11-05T15:00:00Z"
//! ```
//!
//! There is no date type. A date is its ISO 8601 text, "2026-10-27"; a
//! date-time is RFC 3339 text with an offset, "2026-10-27T14:30:00Z" or
//! "2026-10-27T15:30:00+01:00". Such text sorts in time order (date-times
//! when they share an offset), and goes through JSON, databases and the
//! browser as it is. Every function checks what it is given: a malformed
//! date is an error, and `date.parse` gives an error value instead.
//!
//! The calendar is the proleptic Gregorian one, years 1 to 9999. A date is
//! worked on as a count of days from 1970-01-01 (Howard Hinnant's
//! algorithms), so arithmetic is on integers, and the browser's copy of
//! this code (src/client/runtime.js) gives the same answers. Time zones
//! come from the system's database through jiff, or from jiff's bundled
//! copy where there is none; the browser asks Intl.DateTimeFormat. Only the
//! clock (`date.today`, `date.now`) differs between the two.

use crate::interp::{fail, Ctrl, Interp};
use crate::value::*;

type R = Result<Value, Ctrl>;

const FNS: &[(&str, BuiltinFn)] = &[
    ("date.today", today),
    ("date.now", now),
    ("date.parse", parse),
    ("date.valid", valid),
    ("date.valid_zone", valid_zone),
    ("date.of", of),
    ("date.parts", parts),
    ("date.weekday", weekday_fn),
    ("date.is_leap_year", is_leap_year),
    ("date.days_in_month", days_in_month),
    ("date.add_days", add_days),
    ("date.add_months", add_months_fn),
    ("date.add_years", add_years),
    ("date.days_between", days_between),
    ("date.months_between", months_between),
    ("date.start_of_month", start_of_month),
    ("date.end_of_month", end_of_month),
    ("date.to_zone", to_zone),
    ("date.date_in", date_in),
    ("date.at", at),
    ("date.format", format),
    ("date.ms", ms),
    ("date.from_ms", from_ms),
    ("date.compare", compare),
];

/// The clock's functions, which a sandboxed script does without.
const CLOCK: &[&str] = &["date.today", "date.now"];

pub fn install(m: &mut Map) {
    for (name, f) in FNS {
        m.insert(name.trim_start_matches("date.").into(), Value::Builtin(name, *f));
    }
}

/// The module without the clock: what `run_script` gives a script.
pub fn install_pure(m: &mut Map) {
    for (name, f) in FNS.iter().filter(|(n, _)| !CLOCK.contains(n)) {
        m.insert(name.trim_start_matches("date.").into(), Value::Builtin(name, *f));
    }
}

// ---- the calendar ------------------------------------------------------

/// Days from 1970-01-01 to y-m-d.
pub fn days(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// The year, month and day of a day number.
pub fn civil(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + (m <= 2) as i64, m, d)
}

/// 1 is Monday, 7 Sunday (ISO). 1970-01-01 was a Thursday.
fn weekday(z: i64) -> i64 {
    (z + 3).rem_euclid(7) + 1
}

fn leap(y: i64) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}

fn month_len(y: i64, m: i64) -> i64 {
    match m {
        2 if leap(y) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn in_range(y: i64) -> bool {
    (1..=9999).contains(&y)
}

fn date_text(z: i64) -> String {
    let (y, m, d) = civil(z);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

/// The same month n months on, its day clamped to the month's length.
fn add_months(z: i64, n: i64) -> Option<i64> {
    let (y, m, d) = civil(z);
    // `unsigned_abs`: the smallest int has no `abs` (FEEDBACK16.md, Q-141; branch
    // feedback20-fixes).
    if n.unsigned_abs() > 120_000 {
        return None;
    }
    let total = y * 12 + (m - 1) + n;
    let (ny, nm) = (total.div_euclid(12), total.rem_euclid(12) + 1);
    in_range(ny).then(|| days(ny, nm, d.min(month_len(ny, nm))))
}

fn num(b: &[u8]) -> Option<i64> {
    if b.is_empty() || !b.iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(b.iter().fold(0, |n, c| n * 10 + (c - b'0') as i64))
}

fn ymd(y: i64, m: i64, d: i64) -> Option<i64> {
    (in_range(y) && (1..=12).contains(&m) && d >= 1 && d <= month_len(y, m)).then(|| days(y, m, d))
}

/// "2026-10-27", exactly.
pub fn parse_date(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    ymd(num(&b[..4])?, num(&b[5..7])?, num(&b[8..])?)
}

/// A date-time: the local date and time as written, and its offset.
struct Stamp {
    day: i64,
    sec: i64,
    /// The fraction of a second as written (".250"), kept as it is.
    frac: String,
    /// Seconds east of UTC.
    off: i64,
}

impl Stamp {
    fn instant(&self) -> i64 {
        self.day * 86400 + self.sec - self.off
    }
}

fn hms(b: &[u8]) -> Option<i64> {
    let (h, m, s) = (num(&b[..2])?, num(&b[3..5])?, num(&b[6..8])?);
    (b[2] == b':' && b[5] == b':' && h < 24 && m < 60 && s < 60).then(|| h * 3600 + m * 60 + s)
}

/// "Z", "+01:00", "-04:00" (and "+00:09:21", the old local mean times).
fn parse_offset(b: &[u8]) -> Option<i64> {
    if b == b"Z" {
        return Some(0);
    }
    let sign = match b.first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let (h, m) = (num(b.get(1..3)?)?, num(b.get(4..6)?)?);
    let s = match b.len() {
        6 => 0,
        9 if b[6] == b':' => num(&b[7..9])?,
        _ => return None,
    };
    (b[3] == b':' && h < 24 && m < 60 && s < 60).then(|| sign * (h * 3600 + m * 60 + s))
}

/// "2026-10-27T14:30:00Z", "2026-10-27T14:30:00.250+01:00": RFC 3339, as
/// this module writes it.
fn parse_stamp(s: &str) -> Option<Stamp> {
    let b = s.as_bytes();
    if b.len() < 20 || !s.is_ascii() || b[10] != b'T' {
        return None;
    }
    let day = parse_date(&s[..10])?;
    let sec = hms(&b[11..19])?;
    let mut i = 19;
    if b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == 20 || i > 29 {
            return None;
        }
    }
    let off = parse_offset(&b[i..])?;
    Some(Stamp { day, sec, frac: s[19..i].to_string(), off })
}

fn offset_text(off: i64) -> String {
    if off == 0 {
        return "Z".into();
    }
    let a = off.abs();
    let sign = if off < 0 { '-' } else { '+' };
    if a % 60 != 0 {
        format!("{}{:02}:{:02}:{:02}", sign, a / 3600, a % 3600 / 60, a % 60)
    } else {
        format!("{}{:02}:{:02}", sign, a / 3600, a % 3600 / 60)
    }
}

/// The text of an instant as seen at an offset, or None past the years.
fn stamp_text(instant: i64, frac: &str, off: i64) -> Option<String> {
    let local = instant + off;
    let (day, sec) = (local.div_euclid(86400), local.rem_euclid(86400));
    in_range(civil(day).0).then(|| {
        format!("{}T{:02}:{:02}:{:02}{}{}", date_text(day), sec / 3600, sec % 3600 / 60, sec % 60, frac, offset_text(off))
    })
}

/// For the fuzz targets (fuzz/): what is read is written back the same.
#[cfg(fuzzing)]
pub mod fuzzing {
    use super::*;

    pub fn round_trip(s: &str) {
        if let Some(z) = parse_date(s) {
            assert_eq!(date_text(z), s);
        }
        if let Some(st) = parse_stamp(s) {
            // As written, but for the offset: `+00:00` is written `Z`.
            let back = stamp_text(st.instant(), &st.frac, st.off).expect("a date-time read is in range");
            let again = parse_stamp(&back).expect("a date-time written reads");
            assert_eq!((again.instant(), again.frac, again.off), (st.instant(), st.frac, st.off), "{} became {}", s, back);
        }
    }
}

// ---- arguments ---------------------------------------------------------

fn got(v: Option<&Value>) -> String {
    match v {
        Some(v @ Value::Str(_)) => v.repr(),
        Some(v) => v.type_name().into(),
        None => "nothing".into(),
    }
}

fn given(args: &[Value], i: usize) -> Option<&Value> {
    args.get(i).filter(|v| !matches!(v, Value::Nil))
}

fn date_arg(args: &[Value], i: usize, f: &str, line: u32) -> Result<i64, Ctrl> {
    match args.get(i) {
        Some(Value::Str(s)) => match parse_date(s) {
            Some(z) => Ok(z),
            None if parse_stamp(s).is_some() => {
                fail(line, format!("`{}` expects a date (YYYY-MM-DD) as argument {}, got the date-time {} (`date.date_in` gives its date)", f, i + 1, got(args.get(i))))
            }
            None => fail(line, format!("`{}` expects a date (YYYY-MM-DD) as argument {}, got {}", f, i + 1, got(args.get(i)))),
        },
        other => fail(line, format!("`{}` expects a date (YYYY-MM-DD) as argument {}, got {}", f, i + 1, got(other))),
    }
}

fn stamp_arg(args: &[Value], i: usize, f: &str, line: u32) -> Result<Stamp, Ctrl> {
    match args.get(i) {
        Some(Value::Str(s)) => match parse_stamp(s) {
            Some(t) => Ok(t),
            None => fail(line, format!("`{}` expects a date-time (like 2026-10-27T14:30:00Z) as argument {}, got {}", f, i + 1, got(args.get(i)))),
        },
        other => fail(line, format!("`{}` expects a date-time (like 2026-10-27T14:30:00Z) as argument {}, got {}", f, i + 1, got(other))),
    }
}

/// A date, or a date-time (with its time and offset).
fn either_arg(args: &[Value], i: usize, f: &str, line: u32) -> Result<(i64, Option<Stamp>), Ctrl> {
    if let Some(Value::Str(s)) = args.get(i) {
        if let Some(z) = parse_date(s) {
            return Ok((z, None));
        }
        if let Some(t) = parse_stamp(s) {
            return Ok((t.day, Some(t)));
        }
    }
    fail(line, format!("`{}` expects a date or a date-time as argument {}, got {}", f, i + 1, got(args.get(i))))
}

fn int_arg(args: &[Value], i: usize, f: &str, line: u32) -> Result<i64, Ctrl> {
    match args.get(i) {
        Some(Value::Int(n)) => Ok(*n),
        Some(Value::Float(x)) if x.fract() == 0.0 && x.abs() < 9.0e15 => Ok(*x as i64),
        other => fail(line, format!("`{}` expects an int as argument {}, got {}", f, i + 1, got(other))),
    }
}

fn str_arg<'a>(args: &'a [Value], i: usize, f: &str, line: u32) -> Result<&'a str, Ctrl> {
    match args.get(i) {
        Some(Value::Str(s)) => Ok(s),
        other => fail(line, format!("`{}` expects a str as argument {}, got {}", f, i + 1, got(other))),
    }
}

fn out_of_range<T>(f: &str, line: u32) -> Result<T, Ctrl> {
    fail(line, format!("`{}`: the date is out of range (years 1 to 9999)", f))
}

fn day_value(z: Option<i64>, f: &str, line: u32) -> R {
    match z {
        Some(z) if in_range(civil(z).0) => Ok(Value::str(date_text(z))),
        _ => out_of_range(f, line),
    }
}

// ---- dates -------------------------------------------------------------

// `date.valid(s, "date")` takes only a date, and `"date-time"` only a
// date-time: text a page sends, checked with `date.valid(s)` alone, may be a
// date-time that `date.add_days` and the rest refuse with a runtime error
// (FEEDBACK13.md, Q-093; branch feedback13-fixes).
/// `date.valid_zone(z)`: a zone is what people type (FEEDBACK25.md, Q-317; branch
/// feedback26-fixes).
fn valid_zone(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(Value::Bool(matches!(args.first(), Some(Value::Str(_))) && zone(&args, 0, "date.valid_zone", line).is_ok()))
}

fn valid(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (date, stamp) = match args.get(1) {
        None | Some(Value::Nil) => (true, true),
        Some(Value::Str(k)) if &**k == "date" => (true, false),
        Some(Value::Str(k)) if &**k == "date-time" => (false, true),
        Some(other) => return fail(line, format!("`date.valid` takes \"date\" or \"date-time\" as argument 2, got {}", got(Some(other)))),
    };
    Ok(Value::Bool(matches!(args.first(), Some(Value::Str(s)) if (date && parse_date(s).is_some()) || (stamp && parse_stamp(s).is_some()))))
}

fn of(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (y, m, d) = (int_arg(&args, 0, "date.of", line)?, int_arg(&args, 1, "date.of", line)?, int_arg(&args, 2, "date.of", line)?);
    match ymd(y, m, d) {
        Some(z) => Ok(Value::str(date_text(z))),
        None => fail(line, format!("`date.of`: there is no date {}-{}-{}", y, m, d)),
    }
}

fn parts(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (z, t) = either_arg(&args, 0, "date.parts", line)?;
    let (y, m, d) = civil(z);
    let mut out = Map::new();
    out.insert("year".into(), Value::Int(y));
    out.insert("month".into(), Value::Int(m));
    out.insert("day".into(), Value::Int(d));
    out.insert("weekday".into(), Value::Int(weekday(z)));
    if let Some(t) = t {
        out.insert("hour".into(), Value::Int(t.sec / 3600));
        out.insert("minute".into(), Value::Int(t.sec % 3600 / 60));
        out.insert("second".into(), Value::Int(t.sec % 60));
        out.insert("offset".into(), Value::str(offset_text(t.off)));
    }
    Ok(Value::map(out))
}

fn weekday_fn(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(Value::Int(weekday(either_arg(&args, 0, "date.weekday", line)?.0)))
}

/// A year: an int, or the year of a date.
fn year_arg(args: &[Value], f: &str, line: u32) -> Result<i64, Ctrl> {
    match args.first() {
        Some(Value::Str(_)) => Ok(civil(either_arg(args, 0, f, line)?.0).0),
        _ => int_arg(args, 0, f, line),
    }
}

fn is_leap_year(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(Value::Bool(leap(year_arg(&args, "date.is_leap_year", line)?)))
}

/// `date.days_in_month(d)` or `date.days_in_month(year, month)`.
fn days_in_month(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let f = "date.days_in_month";
    let (y, m) = match args.first() {
        Some(Value::Str(_)) => {
            let (y, m, _) = civil(either_arg(&args, 0, f, line)?.0);
            (y, m)
        }
        _ => (int_arg(&args, 0, f, line)?, int_arg(&args, 1, f, line)?),
    };
    if !(1..=12).contains(&m) {
        return fail(line, format!("`{}`: there is no month {}", f, m));
    }
    Ok(Value::Int(month_len(y, m)))
}

fn add_days(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let f = "date.add_days";
    let (z, n) = (date_arg(&args, 0, f, line)?, int_arg(&args, 1, f, line)?);
    day_value((n.unsigned_abs() <= 4_000_000).then(|| z + n), f, line)
}

fn add_months_fn(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let f = "date.add_months";
    let (z, n) = (date_arg(&args, 0, f, line)?, int_arg(&args, 1, f, line)?);
    day_value(add_months(z, n), f, line)
}

fn add_years(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let f = "date.add_years";
    let (z, n) = (date_arg(&args, 0, f, line)?, int_arg(&args, 1, f, line)?);
    day_value((n.unsigned_abs() <= 10_000).then(|| add_months(z, n * 12)).flatten(), f, line)
}

fn days_between(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let f = "date.days_between";
    let a = date_arg(&args, 0, f, line)?;
    Ok(Value::Int(date_arg(&args, 1, f, line)? - a))
}

/// Whole months from a to b: the most n with a + n months (clamped) not
/// past b. Negative, the same count, when b is before a.
fn months_from(a: i64, b: i64) -> i64 {
    if b < a {
        return -months_from(b, a);
    }
    let ((y1, m1, _), (y2, m2, _)) = (civil(a), civil(b));
    let n = (y2 * 12 + m2) - (y1 * 12 + m1);
    if add_months(a, n).map_or(true, |x| x > b) {
        n - 1
    } else {
        n
    }
}

fn months_between(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let f = "date.months_between";
    Ok(Value::Int(months_from(date_arg(&args, 0, f, line)?, date_arg(&args, 1, f, line)?)))
}

fn start_of_month(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (y, m, _) = civil(date_arg(&args, 0, "date.start_of_month", line)?);
    Ok(Value::str(date_text(days(y, m, 1))))
}

fn end_of_month(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (y, m, _) = civil(date_arg(&args, 0, "date.end_of_month", line)?);
    Ok(Value::str(date_text(days(y, m, month_len(y, m)))))
}

/// The Gregorian Easter Sunday (the anonymous algorithm, as Meeus gives it).
// ---- the clock and time zones -----------------------------------------

enum Zone {
    Fixed(i64),
    Named(jiff::tz::TimeZone),
}

fn zone(args: &[Value], i: usize, f: &str, line: u32) -> Result<Zone, Ctrl> {
    let Some(v) = given(args, i) else { return Ok(Zone::Fixed(0)) };
    let Value::Str(name) = v else {
        return fail(line, format!("`{}` expects a time zone (like \"Europe/Paris\") as argument {}, got {}", f, i + 1, got(Some(v))));
    };
    if name.eq_ignore_ascii_case("UTC") || &**name == "Z" {
        return Ok(Zone::Fixed(0));
    }
    // An offset is one as `date.parse` reads it, `+05:30`, `+0530` or `+05`, on both sides: the
    // browser took the last two (its Intl does) and the server none (FEEDBACK55.md, Q-829; branch
    // feedback55). No other name starts with a sign.
    if name.starts_with(['+', '-']) {
        return match written_offset(name) {
            Some(o) => Ok(Zone::Fixed(o)),
            None => fail(line, format!("`{}`: unknown time zone {}", f, got(Some(v)))),
        };
    }
    match jiff::tz::TimeZone::get(name) {
        Ok(tz) if !not_a_zone(name) => Ok(Zone::Named(tz)),
        _ => fail(line, format!("`{}`: unknown time zone {}", f, got(Some(v)))),
    }
}

/// What the system's copy of the time zone database holds, or jiff knows, that is no zone the
/// browser knows: `posixrules` (a file, the rules for a POSIX TZ string), `Factory`, CLDR's
/// `Etc/Unknown`, and what a Linux tzdata has besides: `localtime` (this machine's zone) and the
/// copies under `posix/` and `right/` (with leap seconds). A zone is a name of the IANA database, a
/// zone's or a link's, in any case, on both sides; runtime.js's `NOT_ZONES` is the other half
/// (FEEDBACK55.md, Q-782; branch feedback55).
fn not_a_zone(name: &str) -> bool {
    let folded = name.to_ascii_lowercase();
    ["posixrules", "factory", "etc/unknown", "localtime"].contains(&folded.as_str()) || folded.starts_with("posix/") || folded.starts_with("right/")
}

/// The zone's offset at an instant (seconds since 1970, UTC). jiff's times end
/// about two days before the calendar does: past that, the offset where they end
/// (FEEDBACK16.md, Q-116; branch feedback20-fixes).
fn offset_at(z: &Zone, instant: i64) -> i64 {
    match z {
        Zone::Fixed(o) => *o,
        Zone::Named(tz) => {
            let last = instant.clamp(jiff::Timestamp::MIN.as_second(), jiff::Timestamp::MAX.as_second());
            jiff::Timestamp::from_second(last).map_or(0, |t| tz.to_offset(t).seconds() as i64)
        }
    }
}

/// The instant a local time names in a zone. In a gap (clocks moved
/// forward) it is the time that far after the gap, and in an overlap
/// (moved back) the earlier of the two: what jiff and Temporal call
/// "compatible". It assumes one change of offset within a day either side.
/// What a time the clocks skip becomes (`date.at`'s `skipped`).
#[derive(Clone, Copy, PartialEq)]
enum Skipped {
    Later,
    Earlier,
    Error,
}

/// `local_instant`, with the second of two times a wall-clock time names when `later`, and
/// a skipped time moved on, back, or refused (FEEDBACK30.md, Q-375; branch feedback27-35-fixes).
fn local_instant_as(z: &Zone, local: i64, later: bool, skipped: Skipped) -> Result<i64, ()> {
    let (before, after) = (offset_at(z, local - 86400), offset_at(z, local + 86400));
    let fits = |o: i64| offset_at(z, local - o) == o;
    match (fits(before), fits(after)) {
        (true, true) if later => Ok(local - before.min(after)),
        (true, true) => Ok(local - before.max(after)),
        (true, false) => Ok(local - before),
        (false, true) => Ok(local - after),
        (false, false) => match skipped {
            Skipped::Later => Ok(local - before),
            Skipped::Earlier => Ok(local - after),
            Skipped::Error => Err(()),
        },
    }
}

fn clock() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// `date.ms(t)`: a date-time (or a date, at its midnight in UTC) as
/// milliseconds since 1970, what `now()` gives; `date.from_ms(ms, tz)` the
/// other way, to the second. A program keeps times as `now()` does and shows
/// them with this module (FEEDBACK2.md, Q-027; branch feedback6-fixes).
fn ms(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (z, t) = either_arg(&args, 0, "date.ms", line)?;
    Ok(Value::Int(match t {
        Some(t) => {
            let frac = t.frac.trim_start_matches('.');
            let milli = format!("{:0<3}", &frac[..frac.len().min(3)]).parse::<i64>().unwrap_or(0);
            t.instant() * 1000 + milli
        }
        None => z * 86_400_000,
    }))
}

fn from_ms(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let n = int_arg(&args, 0, "date.from_ms", line)?;
    let z = zone(&args, 1, "date.from_ms", line)?;
    let instant = n.div_euclid(1000);
    match stamp_text(instant, "", offset_at(&z, instant)) {
        Some(s) => Ok(Value::str(s)),
        None => out_of_range("date.from_ms", line),
    }
}

fn today(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let z = zone(&args, 0, "date.today", line)?;
    let t = clock();
    Ok(Value::str(date_text((t + offset_at(&z, t)).div_euclid(86400))))
}

fn now(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let z = zone(&args, 0, "date.now", line)?;
    let t = clock();
    Ok(Value::str(stamp_text(t, "", offset_at(&z, t)).unwrap_or_default()))
}

/// A fraction of a second that is all zeros is no fraction: `.000Z`, as JavaScript's
/// `toISOString()` writes every time, becomes `Z`, so two times made alike are one text
/// (FEEDBACK30.md, Q-368; branch feedback27-35-fixes).
fn kept_frac(frac: &str) -> &str {
    if frac.bytes().skip(1).all(|b| b == b'0') {
        ""
    } else {
        frac
    }
}

/// `date.compare(a, b)`: -1, 0 or 1, by the instants they name, a fraction of a second
/// included; a date is its midnight in UTC (FEEDBACK30.md, Q-368; branch feedback27-35-fixes).
fn compare(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let key = |i: usize| -> Result<(i64, u64), Ctrl> {
        let (z, t) = either_arg(&args, i, "date.compare", line)?;
        Ok(match t {
            Some(t) => {
                let frac = t.frac.trim_start_matches('.');
                (t.instant(), format!("{:0<9}", &frac[..frac.len().min(9)]).parse::<u64>().unwrap_or(0))
            }
            None => (z * 86400, 0),
        })
    };
    let (a, b) = (key(0)?, key(1)?);
    Ok(Value::Int(match a.cmp(&b) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }))
}

fn to_zone(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let f = "date.to_zone";
    let t = stamp_arg(&args, 0, f, line)?;
    let z = zone(&args, 1, f, line)?;
    let i = t.instant();
    match stamp_text(i, kept_frac(&t.frac), offset_at(&z, i)) {
        Some(s) => Ok(Value::str(s)),
        None => out_of_range(f, line),
    }
}

fn date_in(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let f = "date.date_in";
    let t = stamp_arg(&args, 0, f, line)?;
    let z = zone(&args, 1, f, line)?;
    let i = t.instant();
    day_value(Some((i + offset_at(&z, i)).div_euclid(86400)), f, line)
}

/// `date.at(d, "14:30", tz)`: the date-time of a local time in a zone. `date.at(v, tz)` takes
/// the date and time as one text, as `<input type="datetime-local">` gives it (FEEDBACK30.md,
/// Q-376), and options last say which of two times a repeated one is, `{ repeated: "later" }`,
/// and what a skipped one does, `{ skipped: "earlier" }` or `"error"` (Q-375; branch
/// feedback27-35-fixes).
fn at(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let f = "date.at";
    let one_text = match args.first() {
        Some(Value::Str(s)) => local_date_time(s),
        _ => None,
    };
    if let Some((day, sec)) = one_text {
        let z = zone(&args, 1, f, line)?;
        return at_local(&z, day * 86400 + sec, args.get(2), f, line, 3);
    }
    let day = date_arg(&args, 0, f, line)?;
    let sec = match given(&args, 1) {
        None => 0,
        Some(Value::Str(s)) => {
            let b = s.as_bytes();
            let t = match b.len() {
                5 => hms(&[b, b":00"].concat()),
                8 => hms(b),
                _ => None,
            };
            match t {
                Some(t) => t,
                None => return fail(line, format!("`{}` expects a time (HH:MM or HH:MM:SS) as argument 2, got {}", f, got(args.get(1)))),
            }
        }
        other => return fail(line, format!("`{}` expects a time (HH:MM or HH:MM:SS) as argument 2, got {}", f, got(other))),
    };
    let z = zone(&args, 2, f, line)?;
    at_local(&z, day * 86400 + sec, args.get(3), f, line, 4)
}

/// `"2026-10-27T14:30"` or `"2026-10-27T14:30:05"`, a date and a wall-clock time with no
/// offset: the day and the second of it.
fn local_date_time(s: &str) -> Option<(i64, i64)> {
    let b = s.as_bytes();
    if !s.is_ascii() || !matches!(b.len(), 16 | 19) || !matches!(b[10], b'T' | b' ') {
        return None;
    }
    let day = parse_date(&s[..10])?;
    let sec = if b.len() == 16 { hms(&[&b[11..16], b":00"].concat())? } else { hms(&b[11..19])? };
    Some((day, sec))
}

fn at_local(z: &Zone, local: i64, opts: Option<&Value>, f: &str, line: u32, n: usize) -> R {
    let (mut later, mut skipped) = (false, Skipped::Later);
    match opts {
        None | Some(Value::Nil) => {}
        Some(Value::Map(m)) => {
            for (k, v) in m.iter() {
                match (&**k, v) {
                    ("repeated", Value::Str(w)) if &**w == "earlier" || &**w == "later" => later = &**w == "later",
                    ("skipped", Value::Str(w)) if matches!(&**w, "later" | "earlier" | "error") => {
                        skipped = match &**w {
                            "later" => Skipped::Later,
                            "earlier" => Skipped::Earlier,
                            _ => Skipped::Error,
                        }
                    }
                    ("repeated", other) => return fail(line, format!("`{}`'s repeated is \"earlier\" or \"later\", got {}", f, got(Some(other)))),
                    ("skipped", other) => return fail(line, format!("`{}`'s skipped is \"later\", \"earlier\" or \"error\", got {}", f, got(Some(other)))),
                    (k, _) => return fail(line, format!("`{}`: the options are repeated and skipped, not `{}`", f, k)),
                }
            }
        }
        Some(other) => return fail(line, format!("`{}` expects options as a map as argument {}, got {}", f, n, got(Some(other)))),
    }
    let Ok(i) = local_instant_as(z, local, later, skipped) else {
        let (day, sec) = (local.div_euclid(86400), local.rem_euclid(86400));
        return Ok(Value::err(Value::str(format!("{}: {} {:02}:{:02} is skipped in that zone, as its clocks move forward", f, date_text(day), sec / 3600, sec % 3600 / 60))));
    };
    match stamp_text(i, "", offset_at(z, i)) {
        Some(s) => Ok(Value::str(s)),
        None => out_of_range(f, line),
    }
}

// ---- text --------------------------------------------------------------

/// A language's names and its date styles (written in the pattern tokens
/// below), after CLDR. The browser has the same table.
struct Names {
    tags: &'static [&'static str],
    months: [&'static str; 12],
    mon: [&'static str; 12],
    days: [&'static str; 7],
    dy: [&'static str; 7],
    /// short, medium, long, full
    styles: [&'static str; 4],
}

// Each language's names, shared by its regions' rows.
const EN_MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const EN_MON: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const GB_MON: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sept", "Oct", "Nov", "Dec"];
const EN_DAYS: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
const EN_DY: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const DE_MONTHS: [&str; 12] = ["Januar", "Februar", "März", "April", "Mai", "Juni", "Juli", "August", "September", "Oktober", "November", "Dezember"];
const DE_MON: [&str; 12] = ["Jan.", "Feb.", "März", "Apr.", "Mai", "Juni", "Juli", "Aug.", "Sept.", "Okt.", "Nov.", "Dez."];
const DE_DAYS: [&str; 7] = ["Montag", "Dienstag", "Mittwoch", "Donnerstag", "Freitag", "Samstag", "Sonntag"];
const DE_DY: [&str; 7] = ["Mo.", "Di.", "Mi.", "Do.", "Fr.", "Sa.", "So."];
const DE_STYLES: [&str; 4] = ["DD.MM.YY", "DD.MM.YYYY", "D. MMMM YYYY", "dddd, D. MMMM YYYY"];
const FR_MONTHS: [&str; 12] = ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"];
const FR_MON: [&str; 12] = ["janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc."];
const FR_DAYS: [&str; 7] = ["lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche"];
const FR_DY: [&str; 7] = ["lun.", "mar.", "mer.", "jeu.", "ven.", "sam.", "dim."];
const ES_MONTHS: [&str; 12] = ["enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre", "octubre", "noviembre", "diciembre"];
const ES_MON: [&str; 12] = ["ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sept", "oct", "nov", "dic"];
const ES_DAYS: [&str; 7] = ["lunes", "martes", "miércoles", "jueves", "viernes", "sábado", "domingo"];
const ES_DY: [&str; 7] = ["lun", "mar", "mié", "jue", "vie", "sáb", "dom"];
const IT_MONTHS: [&str; 12] = ["gennaio", "febbraio", "marzo", "aprile", "maggio", "giugno", "luglio", "agosto", "settembre", "ottobre", "novembre", "dicembre"];
const IT_MON: [&str; 12] = ["gen", "feb", "mar", "apr", "mag", "giu", "lug", "ago", "set", "ott", "nov", "dic"];
const IT_DAYS: [&str; 7] = ["lunedì", "martedì", "mercoledì", "giovedì", "venerdì", "sabato", "domenica"];
const IT_DY: [&str; 7] = ["lun", "mar", "mer", "gio", "ven", "sab", "dom"];
const NL_MONTHS: [&str; 12] = ["januari", "februari", "maart", "april", "mei", "juni", "juli", "augustus", "september", "oktober", "november", "december"];
const NL_MON: [&str; 12] = ["jan", "feb", "mrt", "apr", "mei", "jun", "jul", "aug", "sep", "okt", "nov", "dec"];
const NL_DAYS: [&str; 7] = ["maandag", "dinsdag", "woensdag", "donderdag", "vrijdag", "zaterdag", "zondag"];
const NL_DY: [&str; 7] = ["ma", "di", "wo", "do", "vr", "za", "zo"];
const PT_MONTHS: [&str; 12] = ["janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro", "dezembro"];
const PT_MON: [&str; 12] = ["jan.", "fev.", "mar.", "abr.", "mai.", "jun.", "jul.", "ago.", "set.", "out.", "nov.", "dez."];
const PT_DAYS: [&str; 7] = ["segunda-feira", "terça-feira", "quarta-feira", "quinta-feira", "sexta-feira", "sábado", "domingo"];

/// The rows: a language's, and its regions' that CLDR writes otherwise (en-CA's short date is
/// 2026-09-05, de-AT's January Jänner, pt-PT's medium one 05/09/2026), each region's styles and
/// names as CLDR 47 has them (FEEDBACK55.md, Q-828; branch feedback55).
const LOCALES: &[Names] = &[
    Names { tags: &["en", "en-us"], months: EN_MONTHS, mon: EN_MON, days: EN_DAYS, dy: EN_DY, styles: ["M/D/YY", "MMM D, YYYY", "MMMM D, YYYY", "dddd, MMMM D, YYYY"] },
    Names { tags: &["en-gb", "en-nz"], months: EN_MONTHS, mon: GB_MON, days: EN_DAYS, dy: EN_DY, styles: ["DD/MM/YYYY", "D MMM YYYY", "D MMMM YYYY", "dddd, D MMMM YYYY"] },
    Names { tags: &["en-ie"], months: EN_MONTHS, mon: GB_MON, days: EN_DAYS, dy: EN_DY, styles: ["DD/MM/YYYY", "D MMM YYYY", "D MMMM YYYY", "dddd D MMMM YYYY"] },
    Names {
        tags: &["en-au"],
        months: EN_MONTHS,
        mon: ["Jan", "Feb", "Mar", "Apr", "May", "June", "July", "Aug", "Sept", "Oct", "Nov", "Dec"],
        days: EN_DAYS,
        dy: EN_DY,
        styles: ["D/M/YY", "D MMM YYYY", "D MMMM YYYY", "dddd, D MMMM YYYY"],
    },
    Names { tags: &["en-in"], months: EN_MONTHS, mon: GB_MON, days: EN_DAYS, dy: EN_DY, styles: ["DD/MM/YY", "D MMM YYYY", "D MMMM YYYY", "dddd, D MMMM YYYY"] },
    Names { tags: &["en-za"], months: EN_MONTHS, mon: GB_MON, days: EN_DAYS, dy: EN_DY, styles: ["YYYY/MM/DD", "DD MMM YYYY", "DD MMMM YYYY", "dddd, DD MMMM YYYY"] },
    Names { tags: &["en-ca"], months: EN_MONTHS, mon: EN_MON, days: EN_DAYS, dy: EN_DY, styles: ["YYYY-MM-DD", "MMM D, YYYY", "MMMM D, YYYY", "dddd, MMMM D, YYYY"] },
    Names { tags: &["de"], months: DE_MONTHS, mon: DE_MON, days: DE_DAYS, dy: DE_DY, styles: DE_STYLES },
    Names {
        tags: &["de-at"],
        months: ["Jänner", "Februar", "März", "April", "Mai", "Juni", "Juli", "August", "September", "Oktober", "November", "Dezember"],
        mon: ["Jän.", "Feb.", "März", "Apr.", "Mai", "Juni", "Juli", "Aug.", "Sep.", "Okt.", "Nov.", "Dez."],
        days: DE_DAYS,
        dy: DE_DY,
        styles: DE_STYLES,
    },
    Names { tags: &["fr"], months: FR_MONTHS, mon: FR_MON, days: FR_DAYS, dy: FR_DY, styles: ["DD/MM/YYYY", "D MMM YYYY", "D MMMM YYYY", "dddd D MMMM YYYY"] },
    Names {
        tags: &["fr-ca"],
        months: FR_MONTHS,
        mon: ["janv.", "févr.", "mars", "avr.", "mai", "juin", "juill.", "août", "sept.", "oct.", "nov.", "déc."],
        days: FR_DAYS,
        dy: FR_DY,
        styles: ["YYYY-MM-DD", "D MMM YYYY", "D MMMM YYYY", "dddd D MMMM YYYY"],
    },
    Names { tags: &["fr-ch"], months: FR_MONTHS, mon: FR_MON, days: FR_DAYS, dy: FR_DY, styles: ["DD.MM.YY", "D MMM YYYY", "D MMMM YYYY", "dddd, D MMMM YYYY"] },
    Names { tags: &["es"], months: ES_MONTHS, mon: ES_MON, days: ES_DAYS, dy: ES_DY, styles: ["D/M/YY", "D MMM YYYY", "D [de] MMMM [de] YYYY", "dddd, D [de] MMMM [de] YYYY"] },
    Names {
        tags: &["es-mx"],
        months: ES_MONTHS,
        mon: ["ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sep", "oct", "nov", "dic"],
        days: ES_DAYS,
        dy: ES_DY,
        styles: ["DD/MM/YY", "D MMM YYYY", "D [de] MMMM [de] YYYY", "dddd, D [de] MMMM [de] YYYY"],
    },
    Names { tags: &["it"], months: IT_MONTHS, mon: IT_MON, days: IT_DAYS, dy: IT_DY, styles: ["DD/MM/YY", "D MMM YYYY", "D MMMM YYYY", "dddd D MMMM YYYY"] },
    Names { tags: &["it-ch"], months: IT_MONTHS, mon: IT_MON, days: IT_DAYS, dy: IT_DY, styles: ["DD.MM.YY", "D MMM YYYY", "D MMMM YYYY", "dddd, D MMMM YYYY"] },
    Names { tags: &["nl"], months: NL_MONTHS, mon: NL_MON, days: NL_DAYS, dy: NL_DY, styles: ["DD-MM-YYYY", "D MMM YYYY", "D MMMM YYYY", "dddd D MMMM YYYY"] },
    Names { tags: &["nl-be"], months: NL_MONTHS, mon: NL_MON, days: NL_DAYS, dy: NL_DY, styles: ["D/MM/YYYY", "D MMM YYYY", "D MMMM YYYY", "dddd D MMMM YYYY"] },
    Names {
        tags: &["pt"],
        months: PT_MONTHS,
        mon: PT_MON,
        days: PT_DAYS,
        dy: ["seg.", "ter.", "qua.", "qui.", "sex.", "sáb.", "dom."],
        styles: ["DD/MM/YYYY", "D [de] MMM [de] YYYY", "D [de] MMMM [de] YYYY", "dddd, D [de] MMMM [de] YYYY"],
    },
    Names {
        tags: &["pt-pt"],
        months: PT_MONTHS,
        mon: PT_MON,
        days: PT_DAYS,
        dy: ["segunda", "terça", "quarta", "quinta", "sexta", "sábado", "domingo"],
        styles: ["DD/MM/YY", "DD/MM/YYYY", "D [de] MMMM [de] YYYY", "dddd, D [de] MMMM [de] YYYY"],
    },
];

/// A locale's names: its own, its language's, or English's.
fn names(locale: &str) -> &'static Names {
    let tag = locale.trim().to_ascii_lowercase().replace('_', "-");
    let lang = tag.split('-').next().unwrap_or("");
    LOCALES
        .iter()
        .find(|n| n.tags.contains(&tag.as_str()))
        .or_else(|| LOCALES.iter().find(|n| n.tags.contains(&lang)))
        .unwrap_or(&LOCALES[0])
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Tok {
    Lit(usize, usize),
    Year4,
    Year2,
    MonthName,
    MonthAbbr,
    Month2,
    Month,
    Day2,
    Day,
    DayName,
    DayAbbr,
    Hour2,
    Hour,
    Hour12x2,
    Hour12,
    Minute,
    Second,
    AmPm,
    Offset,
}

/// Longest first, so "MMMM" is not read as "MM" twice.
const TOKENS: &[(&str, Tok)] = &[
    ("YYYY", Tok::Year4),
    ("YY", Tok::Year2),
    ("MMMM", Tok::MonthName),
    ("MMM", Tok::MonthAbbr),
    ("MM", Tok::Month2),
    ("M", Tok::Month),
    ("DD", Tok::Day2),
    ("D", Tok::Day),
    ("dddd", Tok::DayName),
    ("ddd", Tok::DayAbbr),
    ("HH", Tok::Hour2),
    ("H", Tok::Hour),
    ("hh", Tok::Hour12x2),
    ("h", Tok::Hour12),
    ("mm", Tok::Minute),
    ("ss", Tok::Second),
    ("A", Tok::AmPm),
    ("Z", Tok::Offset),
];

/// A pattern's tokens; text in [brackets] and other characters as they are
/// (a literal is a range of chars in `p`).
fn tokens(p: &[char]) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut i = 0;
    'next: while i < p.len() {
        if p[i] == '[' {
            let end = p[i + 1..].iter().position(|c| *c == ']').map_or(p.len(), |k| i + 1 + k);
            out.push(Tok::Lit(i + 1, end));
            i = (end + 1).min(p.len());
            continue;
        }
        for (t, tok) in TOKENS {
            if p[i..].iter().take(t.len()).copied().eq(t.chars()) {
                out.push(*tok);
                i += t.len();
                continue 'next;
            }
        }
        out.push(Tok::Lit(i, i + 1));
        i += 1;
    }
    out
}

fn is_time(t: Tok) -> bool {
    matches!(t, Tok::Hour2 | Tok::Hour | Tok::Hour12x2 | Tok::Hour12 | Tok::Minute | Tok::Second | Tok::AmPm | Tok::Offset)
}

fn token_text(t: Tok) -> &'static str {
    TOKENS.iter().find(|(_, x)| *x == t).map_or("", |(s, _)| s)
}

fn render(p: &[char], toks: &[Tok], n: &Names, z: i64, t: Option<&Stamp>) -> String {
    let (y, m, d) = civil(z);
    let (sec, off) = t.map_or((0, 0), |t| (t.sec, t.off));
    let (h, mi, s) = (sec / 3600, sec % 3600 / 60, sec % 60);
    let mut out = String::new();
    for tok in toks {
        match *tok {
            Tok::Lit(a, b) => out.extend(&p[a..b]),
            Tok::Year4 => out.push_str(&format!("{:04}", y)),
            Tok::Year2 => out.push_str(&format!("{:02}", y % 100)),
            Tok::MonthName => out.push_str(n.months[m as usize - 1]),
            Tok::MonthAbbr => out.push_str(n.mon[m as usize - 1]),
            Tok::Month2 => out.push_str(&format!("{:02}", m)),
            Tok::Month => out.push_str(&m.to_string()),
            Tok::Day2 => out.push_str(&format!("{:02}", d)),
            Tok::Day => out.push_str(&d.to_string()),
            Tok::DayName => out.push_str(n.days[weekday(z) as usize - 1]),
            Tok::DayAbbr => out.push_str(n.dy[weekday(z) as usize - 1]),
            Tok::Hour2 => out.push_str(&format!("{:02}", h)),
            Tok::Hour => out.push_str(&h.to_string()),
            Tok::Hour12x2 => out.push_str(&format!("{:02}", (h + 11) % 12 + 1)),
            Tok::Hour12 => out.push_str(&((h + 11) % 12 + 1).to_string()),
            Tok::Minute => out.push_str(&format!("{:02}", mi)),
            Tok::Second => out.push_str(&format!("{:02}", s)),
            Tok::AmPm => out.push_str(if h < 12 { "AM" } else { "PM" }),
            Tok::Offset => out.push_str(&offset_text(off)),
        }
    }
    out
}

/// `date.format(d, "DD/MM/YYYY")`, `date.format(d, { locale: "de", style:
/// "long" })` or `{ pattern, locale }` (names in that language).
fn format(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let f = "date.format";
    let (z, t) = either_arg(&args, 0, f, line)?;
    let (pattern, n): (String, &Names) = match given(&args, 1) {
        Some(Value::Str(p)) => (p.to_string(), &LOCALES[0]),
        Some(Value::Map(o)) => {
            let n = match o.get("locale") {
                None | Some(Value::Nil) => &LOCALES[0],
                Some(Value::Str(l)) => names(l),
                Some(other) => return fail(line, format!("`{}`: locale is a str, like \"de-DE\", got {}", f, got(Some(other)))),
            };
            if let Some(k) = o.keys().find(|k| !matches!(k.as_str(), "locale" | "style" | "pattern")) {
                return fail(line, format!("`{}`: the options are locale, style and pattern, not `{}`", f, k.as_str()));
            }
            match (o.get("pattern"), o.get("style")) {
                (Some(Value::Str(p)), _) => (p.to_string(), n),
                (None | Some(Value::Nil), None | Some(Value::Nil)) => (n.styles[1].to_string(), n),
                (None | Some(Value::Nil), Some(Value::Str(s))) => match ["short", "medium", "long", "full"].iter().position(|x| **x == **s) {
                    Some(k) => (n.styles[k].to_string(), n),
                    None => return fail(line, format!("`{}`: style is short, medium, long or full, got {}", f, got(o.get("style")))),
                },
                (Some(other), _) | (_, Some(other)) => return fail(line, format!("`{}`: pattern and style are strs, got {}", f, got(Some(other)))),
            }
        }
        _ => return fail(line, format!("`{}` expects a pattern (like \"DD/MM/YYYY\") or options as argument 2, got {}", f, got(args.get(1)))),
    };
    let p: Vec<char> = pattern.chars().collect();
    let toks = tokens(&p);
    if t.is_none() {
        if let Some(x) = toks.iter().find(|x| is_time(**x)) {
            return fail(line, format!("`{}`: the pattern has a time ({}), but {} is a date", f, token_text(*x), got(args.first())));
        }
    }
    Ok(Value::str(render(&p, &toks, n, z, t.as_ref())))
}

/// Reads up to `max` digits (at least `min`).
fn digits(s: &[char], i: &mut usize, min: usize, max: usize) -> Option<i64> {
    let start = *i;
    while *i < s.len() && *i - start < max && s[*i].is_ascii_digit() {
        *i += 1;
    }
    if *i - start < min {
        return None;
    }
    Some(s[start..*i].iter().fold(0, |n, c| n * 10 + c.to_digit(10).unwrap() as i64))
}

/// One of `words` at i, ignoring case, the longest that fits; its index.
fn word(s: &[char], i: &mut usize, words: &[&str]) -> Option<usize> {
    let mut best: Option<(usize, usize)> = None;
    for (k, w) in words.iter().enumerate() {
        let n = w.chars().count();
        if *i + n <= s.len() && s[*i..*i + n].iter().map(|c| c.to_lowercase().next().unwrap_or(*c)).eq(w.chars().map(|c| c.to_lowercase().next().unwrap_or(c))) && best.map_or(true, |(_, b)| n > b) {
            best = Some((k, n));
        }
    }
    let (k, n) = best?;
    *i += n;
    Some(k)
}

/// Reads text by a pattern: a date, or a date-time when the pattern has a
/// time (at its `Z` offset, or UTC).
fn read(s: &[char], p: &[char]) -> Option<String> {
    let en = &LOCALES[0];
    let (mut y, mut mo, mut d) = (None, None, None);
    let (mut h, mut mi, mut se, mut pm, mut twelve, mut off) = (None, 0, 0, None, false, 0);
    let mut timed = false;
    let mut i = 0;
    for tok in tokens(p) {
        timed |= is_time(tok);
        match tok {
            Tok::Lit(a, b) => {
                for c in &p[a..b] {
                    if s.get(i) != Some(c) {
                        return None;
                    }
                    i += 1;
                }
            }
            Tok::Year4 => y = Some(digits(s, &mut i, 4, 4)?),
            Tok::Year2 => y = Some(2000 + digits(s, &mut i, 2, 2)?),
            Tok::MonthName => mo = Some(word(s, &mut i, &en.months)? as i64 + 1),
            Tok::MonthAbbr => mo = Some(word(s, &mut i, &en.mon)? as i64 + 1),
            Tok::Month2 => mo = Some(digits(s, &mut i, 2, 2)?),
            Tok::Month => mo = Some(digits(s, &mut i, 1, 2)?),
            Tok::Day2 => d = Some(digits(s, &mut i, 2, 2)?),
            Tok::Day => d = Some(digits(s, &mut i, 1, 2)?),
            Tok::DayName => _ = word(s, &mut i, &en.days)?,
            Tok::DayAbbr => _ = word(s, &mut i, &en.dy)?,
            Tok::Hour2 => h = Some(digits(s, &mut i, 2, 2)?),
            Tok::Hour => h = Some(digits(s, &mut i, 1, 2)?),
            Tok::Hour12x2 | Tok::Hour12 => {
                let n = if tok == Tok::Hour12 { 1 } else { 2 };
                let x = digits(s, &mut i, n, 2)?;
                if !(1..=12).contains(&x) {
                    return None;
                }
                h = Some(x);
                twelve = true;
            }
            Tok::Minute => mi = digits(s, &mut i, 2, 2)?,
            Tok::Second => se = digits(s, &mut i, 2, 2)?,
            Tok::AmPm => pm = Some(word(s, &mut i, &["AM", "PM"])? == 1),
            // "Z", "+01:00" or "+0100".
            Tok::Offset => {
                let len = match s.get(i) {
                    Some('Z' | 'z') => 1,
                    _ if s.get(i + 3) == Some(&':') => 6,
                    _ => 5,
                };
                let mut o: String = s.get(i..i + len)?.iter().collect();
                if !o.is_ascii() {
                    return None;
                }
                if len == 5 {
                    o.insert(3, ':');
                }
                off = parse_offset(o.to_ascii_uppercase().as_bytes())?;
                i += len;
            }
        }
    }
    if i != s.len() {
        return None;
    }
    let day = ymd(y?, mo?, d?)?;
    if !timed {
        return Some(date_text(day));
    }
    // "12:30 AM" is 00:30, "12:30 PM" 12:30.
    let h = match (h.unwrap_or(0), pm) {
        (h, Some(p)) if twelve => h % 12 + if p { 12 } else { 0 },
        (h, _) => h,
    };
    if h > 23 || mi > 59 || se > 59 {
        return None;
    }
    stamp_text(day * 86400 + h * 3600 + mi * 60 + se - off, "", off)
}

/// What `date.parse` takes without a pattern: a date, or a date-time with
/// "T", "t" or a space, seconds or not, and an offset ("Z", "+01:00",
/// "+0100", "+01"). It gives them back as this module writes them.
fn read_iso(s: &str) -> Option<String> {
    if let Some(z) = parse_date(s) {
        return Some(date_text(z));
    }
    let b = s.as_bytes();
    if b.len() < 16 || !s.is_ascii() || !matches!(b[10], b'T' | b't' | b' ') {
        return None;
    }
    let day = parse_date(&s[..10])?;
    let mut i = 16;
    let hm = [&b[11..16], b":00"].concat();
    let mut sec = hms(&hm)?;
    if b.get(16) == Some(&b':') {
        let s = num(b.get(17..19)?)?;
        if s > 59 {
            return None;
        }
        sec += s;
        i = 19;
    }
    let fs = i;
    if b.get(i) == Some(&b'.') || b.get(i) == Some(&b',') {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == fs + 1 || i > fs + 10 {
            return None;
        }
    }
    let frac = if i > fs { format!(".{}", &s[fs + 1..i]) } else { String::new() };
    let o = &s[i..];
    let off = match o.len() {
        1 => parse_offset(o.to_ascii_uppercase().as_bytes())?,
        _ => written_offset(o)?,
    };
    stamp_text(day * 86400 + sec - off, &frac, off)
}

/// An offset as people write one: `+01`, `+0100`, `+01:00` (and `+00:09:21`), in seconds.
fn written_offset(o: &str) -> Option<i64> {
    match o.len() {
        3 => parse_offset(format!("{}:00", o).as_bytes()),
        5 if !o.contains(':') => parse_offset(format!("{}:{}", o.get(..3)?, o.get(3..)?).as_bytes()),
        _ => parse_offset(o.as_bytes()),
    }
}

/// An HTTP date, `Tue, 27 Oct 2026 14:30:00 GMT` (RFC 9110's IMF-fixdate), as UTC.
/// Seconds since 1970 of an HTTP date in any of the three forms RFC 9110 §5.6.7 says a
/// recipient takes: `Sun, 06 Nov 1994 08:49:37 GMT`, RFC 850's `Sunday, 06-Nov-94 08:49:37
/// GMT` and asctime's `Sun Nov  6 08:49:37 1994` (FEEDBACK36.md, Q-441; branch feedback36-39).
pub(crate) fn http_seconds(s: &str) -> Option<i64> {
    const MON: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let month = |m: &str| MON.iter().position(|x| *x == m).map(|i| i as i64 + 1);
    let w: Vec<&str> = s.split_whitespace().collect();
    let (y, m, d, t) = match w.as_slice() {
        [dow, d, m, y, t, "GMT"] if dow.ends_with(',') && d.len() == 2 && y.len() == 4 => (num(y.as_bytes())?, month(m)?, num(d.as_bytes())?, *t),
        [dow, dmy, t, "GMT"] if dow.ends_with(',') => {
            let [d, m, y] = dmy.split('-').collect::<Vec<_>>()[..] else { return None };
            let y = num(y.as_bytes()).filter(|_| y.len() == 2)?;
            // Two digits: a year more than 50 years ahead is taken as the past (§5.6.7).
            let now = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() / 31_556_952) as i64 + 1970;
            let mut y = now - now % 100 + y;
            if y > now + 50 {
                y -= 100;
            }
            (y, month(m)?, num(d.as_bytes()).filter(|_| d.len() == 2)?, *t)
        }
        [_, m, d, t, y] if y.len() == 4 => (num(y.as_bytes())?, month(m)?, num(d.as_bytes())?, *t),
        _ => return None,
    };
    if t.len() != 8 {
        return None;
    }
    Some(ymd(y, m, d)? * 86400 + hms(t.as_bytes())?)
}

fn read_http(s: &str) -> Option<String> {
    const MON: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let w: Vec<&str> = s.split_whitespace().collect();
    let [dow, d, m, y, t, "GMT"] = w.as_slice() else { return None };
    if !dow.ends_with(',') || d.len() != 2 || y.len() != 4 || t.len() != 8 {
        return None;
    }
    let m = MON.iter().position(|x| x == m)? as i64 + 1;
    let day = ymd(num(y.as_bytes())?, m, num(d.as_bytes())?)?;
    let sec = hms(t.as_bytes())?;
    stamp_text(day * 86400 + sec, "", 0)
}

fn parse(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let f = "date.parse";
    let s = str_arg(&args, 0, f, line)?;
    let r = match given(&args, 1) {
        // A local date-time names no instant until a zone is given: it is refused with what to
        // write (FEEDBACK30.md, Q-376; branch feedback27-35-fixes).
        None if local_date_time(s.trim()).is_some() => Err(format!("{} has no offset, so it is a wall-clock time and names no instant: read it in its zone with date.at(v, zone)", got(args.first()))),
        None => read_iso(s.trim()).or_else(|| read_http(s.trim())).ok_or_else(|| format!("not a date or a date-time: {}", got(args.first()))),
        Some(Value::Str(p)) => {
            let (s, p): (Vec<char>, Vec<char>) = (s.trim().chars().collect(), p.chars().collect());
            read(&s, &p).ok_or_else(|| format!("{} is not a date in the form {}", got(args.first()), got(args.get(1))))
        }
        other => return fail(line, format!("`{}` expects a pattern (like \"DD/MM/YYYY\") as argument 2, got {}", f, got(other))),
    };
    Ok(match r {
        Ok(s) => Value::str(s),
        Err(e) => Value::err(Value::str(e)),
    })
}
