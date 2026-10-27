//! `date`: calendar dates, date-times, time zones, business days, and the
//! day counts that interest is computed with.
//!
//! ```text
//! date.today()  date.today("Europe/Paris")  date.now()    // "2026-10-27", "2026-10-27T14:30:00Z"
//! date.add_months("2026-01-31", 1)                         // "2026-02-28": clamped, not March
//! date.to_zone("2026-10-27T14:30:00Z", "America/New_York") // "2026-10-27T10:30:00-04:00"
//! date.adjust("2026-12-25", "modified_following", { holidays: ["2026-12-25"] })
//! date.year_fraction("2026-01-15", "2026-07-15", "ACT/360") // 0.5027777777777778
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
    ("date.is_business_day", is_business_day),
    ("date.add_business_days", add_business_days),
    ("date.business_days_between", business_days_between),
    ("date.adjust", adjust_fn),
    ("date.easter", easter),
    ("date.day_count", day_count),
    ("date.year_fraction", year_fraction),
    ("date.format", format),
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
    if n.abs() > 120_000 {
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

fn valid(_: &mut Interp, args: Vec<Value>, _: u32) -> R {
    Ok(Value::Bool(matches!(args.first(), Some(Value::Str(s)) if parse_date(s).is_some() || parse_stamp(s).is_some())))
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
    day_value((n.abs() <= 4_000_000).then(|| z + n), f, line)
}

fn add_months_fn(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let f = "date.add_months";
    let (z, n) = (date_arg(&args, 0, f, line)?, int_arg(&args, 1, f, line)?);
    day_value(add_months(z, n), f, line)
}

fn add_years(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let f = "date.add_years";
    let (z, n) = (date_arg(&args, 0, f, line)?, int_arg(&args, 1, f, line)?);
    day_value((n.abs() <= 10_000).then(|| add_months(z, n * 12)).flatten(), f, line)
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
fn easter_day(y: i64) -> i64 {
    let (a, b, c) = (y % 19, y / 100, y % 100);
    let (d, e) = (b / 4, b % 4);
    let g = (b - (b + 8) / 25 + 1) / 3;
    let h = (19 * a + b - d - g + 15) % 30;
    let (i, k) = (c / 4, c % 4);
    let l = (32 + 2 * e + 2 * i - h - k) % 7;
    let m = (a + 11 * h + 22 * l) / 451;
    let n = h + l - 7 * m + 114;
    days(y, n / 31, n % 31 + 1)
}

fn easter(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let y = int_arg(&args, 0, "date.easter", line)?;
    if !(1583..=9999).contains(&y) {
        return fail(line, format!("`date.easter` is for Gregorian years (1583 to 9999), got {}", y));
    }
    Ok(Value::str(date_text(easter_day(y))))
}

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
    if name.starts_with(['+', '-']) {
        if let Some(o) = parse_offset(name.as_bytes()) {
            return Ok(Zone::Fixed(o));
        }
    }
    match jiff::tz::TimeZone::get(name) {
        Ok(tz) => Ok(Zone::Named(tz)),
        Err(_) => fail(line, format!("`{}`: unknown time zone {}", f, got(Some(v)))),
    }
}

/// The zone's offset at an instant (seconds since 1970, UTC).
fn offset_at(z: &Zone, instant: i64) -> i64 {
    match z {
        Zone::Fixed(o) => *o,
        Zone::Named(tz) => jiff::Timestamp::from_second(instant).map_or(0, |t| tz.to_offset(t).seconds() as i64),
    }
}

/// The instant a local time names in a zone. In a gap (clocks moved
/// forward) it is the time that far after the gap, and in an overlap
/// (moved back) the earlier of the two: what jiff and Temporal call
/// "compatible". It assumes one change of offset within a day either side.
fn local_instant(z: &Zone, local: i64) -> i64 {
    let (before, after) = (offset_at(z, local - 86400), offset_at(z, local + 86400));
    let fits = |o: i64| offset_at(z, local - o) == o;
    match (fits(before), fits(after)) {
        (true, true) => local - before.max(after),
        (true, false) => local - before,
        (false, true) => local - after,
        (false, false) => local - before,
    }
}

fn clock() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
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

fn to_zone(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let f = "date.to_zone";
    let t = stamp_arg(&args, 0, f, line)?;
    let z = zone(&args, 1, f, line)?;
    let i = t.instant();
    match stamp_text(i, &t.frac, offset_at(&z, i)) {
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

/// `date.at(d, "14:30", tz)`: the date-time of a local time in a zone.
fn at(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let f = "date.at";
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
    let i = local_instant(&z, day * 86400 + sec);
    match stamp_text(i, "", offset_at(&z, i)) {
        Some(s) => Ok(Value::str(s)),
        None => out_of_range(f, line),
    }
}

// ---- business days -----------------------------------------------------

/// `{ weekend: [6, 7], holidays: ["2026-12-25", ...] }`.
struct Calendar {
    weekend: [bool; 8],
    holidays: Vec<i64>,
}

impl Calendar {
    fn open(&self, z: i64) -> bool {
        !self.weekend[weekday(z) as usize] && self.holidays.binary_search(&z).is_err()
    }
}

fn calendar(args: &[Value], i: usize, f: &str, line: u32) -> Result<Calendar, Ctrl> {
    let mut cal = Calendar { weekend: [false, false, false, false, false, false, true, true], holidays: Vec::new() };
    let Some(v) = given(args, i) else { return Ok(cal) };
    let Value::Map(m) = v else {
        return fail(line, format!("`{}` expects a calendar ({{ weekend, holidays }}) as argument {}, got {}", f, i + 1, got(Some(v))));
    };
    for (k, v) in m.iter() {
        match (k.as_str(), v) {
            ("weekend", Value::List(days)) => {
                cal.weekend = [false; 8];
                for d in days.iter() {
                    match d {
                        Value::Int(n @ 1..=7) => cal.weekend[*n as usize] = true,
                        Value::Float(x) if x.fract() == 0.0 && (1.0..=7.0).contains(x) => cal.weekend[*x as usize] = true,
                        _ => return fail(line, format!("`{}`: a calendar's weekend is days 1 (Monday) to 7 (Sunday), got {}", f, got(Some(d)))),
                    }
                }
                if cal.weekend[1..].iter().all(|w| *w) {
                    return fail(line, format!("`{}`: a calendar's weekend cannot be the whole week", f));
                }
            }
            ("holidays", Value::List(hs)) => {
                for h in hs.iter() {
                    match h {
                        Value::Str(s) if parse_date(s).is_some() => cal.holidays.extend(parse_date(s)),
                        _ => return fail(line, format!("`{}`: a calendar's holidays are dates (YYYY-MM-DD), got {}", f, got(Some(h)))),
                    }
                }
            }
            ("weekend" | "holidays", Value::Nil) => {}
            ("weekend" | "holidays", other) => return fail(line, format!("`{}`: a calendar's {} is a list, got {}", f, k.as_str(), got(Some(other)))),
            (other, _) => return fail(line, format!("`{}`: a calendar has `weekend` and `holidays`, not `{}`", f, other)),
        }
    }
    cal.holidays.sort_unstable();
    Ok(cal)
}

fn step(z: i64, by: i64) -> Option<i64> {
    let z = z + by;
    in_range(civil(z).0).then_some(z)
}

/// The first business day from z going by `by` days (z itself if open).
fn roll(cal: &Calendar, mut z: i64, by: i64) -> Option<i64> {
    while !cal.open(z) {
        z = step(z, by)?;
    }
    Some(z)
}

/// The business day conventions (ISDA 2006, section 4.12).
fn adjust(cal: &Calendar, z: i64, how: &str) -> Option<Option<i64>> {
    let month = |x: i64| civil(x).1;
    Some(match how {
        "unadjusted" => Some(z),
        // The next business day.
        "following" => roll(cal, z, 1),
        // The next, unless that is in the next month: then the one before.
        "modified_following" => roll(cal, z, 1).filter(|x| month(*x) == month(z)).or_else(|| roll(cal, z, -1)),
        // The business day before.
        "preceding" => roll(cal, z, -1),
        // The one before, unless that is in the month before: then the next.
        "modified_preceding" => roll(cal, z, -1).filter(|x| month(*x) == month(z)).or_else(|| roll(cal, z, 1)),
        _ => return None,
    })
}

fn is_business_day(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let f = "date.is_business_day";
    let z = date_arg(&args, 0, f, line)?;
    Ok(Value::Bool(calendar(&args, 1, f, line)?.open(z)))
}

/// n business days on (back, for n < 0). With n = 0, the day itself, or
/// the next business day if it is not one.
fn add_business_days(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let f = "date.add_business_days";
    let (z, n) = (date_arg(&args, 0, f, line)?, int_arg(&args, 1, f, line)?);
    let cal = calendar(&args, 2, f, line)?;
    if n.abs() > 4_000_000 {
        return out_of_range(f, line);
    }
    let by = if n < 0 { -1 } else { 1 };
    let mut z = Some(z);
    for _ in 0..n.abs() {
        z = z.and_then(|z| step(z, by)).and_then(|z| roll(&cal, z, by));
    }
    day_value(z.and_then(|z| roll(&cal, z, 1)), f, line)
}

/// Business days from a (counted) to b (not counted); negative when b is
/// before a.
fn business_days_between(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let f = "date.business_days_between";
    let (a, b) = (date_arg(&args, 0, f, line)?, date_arg(&args, 1, f, line)?);
    let cal = calendar(&args, 2, f, line)?;
    let n = (a.min(b)..a.max(b)).filter(|z| cal.open(*z)).count() as i64;
    Ok(Value::Int(if b < a { -n } else { n }))
}

fn adjust_fn(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let f = "date.adjust";
    let z = date_arg(&args, 0, f, line)?;
    let how = match given(&args, 1) {
        None => "following",
        Some(Value::Str(s)) => s,
        other => return fail(line, format!("`{}` expects a convention as argument 2, got {}", f, got(other))),
    };
    let cal = calendar(&args, 2, f, line)?;
    match adjust(&cal, z, how) {
        Some(z) => day_value(z, f, line),
        None => fail(line, format!(
            "`{}`: unknown convention {} (following, modified_following, preceding, modified_preceding or unadjusted)",
            f,
            got(args.get(1))
        )),
    }
}

// ---- day counts --------------------------------------------------------

#[derive(Clone, Copy)]
enum Basis {
    Act360,
    Act365F,
    ActActIsda,
    Bond,
    Us,
    Euro,
}

fn basis(args: &[Value], f: &str, line: u32) -> Result<Basis, Ctrl> {
    let name = str_arg(args, 2, f, line)?;
    let n = name.trim().to_ascii_uppercase().replace("ACTUAL", "ACT");
    Ok(match n.as_str() {
        "ACT/360" => Basis::Act360,
        "ACT/365" | "ACT/365F" | "ACT/365 FIXED" => Basis::Act365F,
        "ACT/ACT" | "ACT/ACT ISDA" => Basis::ActActIsda,
        "30/360" | "30/360 ISDA" | "360/360" | "BOND BASIS" => Basis::Bond,
        "30U/360" | "30/360 US" => Basis::Us,
        "30E/360" | "EUROBOND BASIS" => Basis::Euro,
        _ => {
            return fail(line, format!(
                "`{}`: unknown day count convention {} (ACT/360, ACT/365, ACT/ACT, 30/360, 30U/360 or 30E/360)",
                f,
                got(args.get(2))
            ))
        }
    })
}

fn thirty(y1: i64, m1: i64, d1: i64, y2: i64, m2: i64, d2: i64) -> i64 {
    360 * (y2 - y1) + 30 * (m2 - m1) + (d2 - d1)
}

/// Days as the convention counts them from a to b (a <= b), and the
/// fraction of a year they make.
fn count(c: Basis, a: i64, b: i64) -> (i64, f64) {
    let ((y1, m1, d1), (y2, m2, d2)) = (civil(a), civil(b));
    let act = b - a;
    match c {
        // Actual days over 360 (money markets).
        Basis::Act360 => (act, act as f64 / 360.0),
        // Actual days over 365, leap years or not (ACT/365 Fixed).
        Basis::Act365F => (act, act as f64 / 365.0),
        // ISDA 2006 4.16(b): the days in a leap year over 366, plus the
        // days in other years over 365.
        Basis::ActActIsda => {
            let base = |y: i64| if leap(y) { 366.0 } else { 365.0 };
            if y1 == y2 {
                return (act, act as f64 / base(y1));
            }
            let first = (days(y1 + 1, 1, 1) - a) as f64;
            let last = (b - days(y2, 1, 1)) as f64;
            (act, first / base(y1) + (y2 - y1 - 1) as f64 + last / base(y2))
        }
        // ISDA 2006 4.16(f), "30/360" or bond basis: a 31st first day is
        // the 30th; a 31st last day is the 30th when the first day is then
        // the 30th.
        Basis::Bond => {
            let d1 = if d1 == 31 { 30 } else { d1 };
            let d2 = if d2 == 31 && d1 == 30 { 30 } else { d2 };
            let n = thirty(y1, m1, d1, y2, m2, d2);
            (n, n as f64 / 360.0)
        }
        // 30/360 US (SIA): as bond basis, and the last day of February
        // counts as the 30th when it begins the period, and when it ends a
        // period that began on one.
        Basis::Us => {
            let feb_end = |y: i64, m: i64, d: i64| m == 2 && d == month_len(y, 2);
            let (e1, e2) = (feb_end(y1, m1, d1), feb_end(y2, m2, d2));
            let mut d2 = if e1 && e2 { 30 } else { d2 };
            let d1 = if e1 || d1 == 31 { 30 } else { d1 };
            if d2 == 31 && d1 == 30 {
                d2 = 30;
            }
            let n = thirty(y1, m1, d1, y2, m2, d2);
            (n, n as f64 / 360.0)
        }
        // ISDA 2006 4.16(g), 30E/360 or Eurobond basis: every 31st is the 30th.
        Basis::Euro => {
            let n = thirty(y1, m1, d1.min(30), y2, m2, d2.min(30));
            (n, n as f64 / 360.0)
        }
    }
}

fn counted(args: &[Value], f: &str, line: u32) -> Result<(i64, f64), Ctrl> {
    let (a, b) = (date_arg(args, 0, f, line)?, date_arg(args, 1, f, line)?);
    let c = basis(args, f, line)?;
    if b < a {
        let (n, x) = count(c, b, a);
        return Ok((-n, -x));
    }
    Ok(count(c, a, b))
}

fn day_count(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(Value::Int(counted(&args, "date.day_count", line)?.0))
}

fn year_fraction(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(Value::Float(counted(&args, "date.year_fraction", line)?.1))
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

const LOCALES: &[Names] = &[
    Names {
        tags: &["en", "en-us"],
        months: ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"],
        mon: ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"],
        days: ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"],
        dy: ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"],
        styles: ["M/D/YY", "MMM D, YYYY", "MMMM D, YYYY", "dddd, MMMM D, YYYY"],
    },
    Names {
        tags: &["en-gb", "en-ie", "en-au", "en-nz", "en-in", "en-za"],
        months: ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"],
        mon: ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sept", "Oct", "Nov", "Dec"],
        days: ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"],
        dy: ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"],
        styles: ["DD/MM/YYYY", "D MMM YYYY", "D MMMM YYYY", "dddd, D MMMM YYYY"],
    },
    Names {
        tags: &["de"],
        months: ["Januar", "Februar", "März", "April", "Mai", "Juni", "Juli", "August", "September", "Oktober", "November", "Dezember"],
        mon: ["Jan.", "Feb.", "März", "Apr.", "Mai", "Juni", "Juli", "Aug.", "Sept.", "Okt.", "Nov.", "Dez."],
        days: ["Montag", "Dienstag", "Mittwoch", "Donnerstag", "Freitag", "Samstag", "Sonntag"],
        dy: ["Mo.", "Di.", "Mi.", "Do.", "Fr.", "Sa.", "So."],
        styles: ["DD.MM.YY", "DD.MM.YYYY", "D. MMMM YYYY", "dddd, D. MMMM YYYY"],
    },
    Names {
        tags: &["fr"],
        months: ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"],
        mon: ["janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc."],
        days: ["lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche"],
        dy: ["lun.", "mar.", "mer.", "jeu.", "ven.", "sam.", "dim."],
        styles: ["DD/MM/YYYY", "D MMM YYYY", "D MMMM YYYY", "dddd D MMMM YYYY"],
    },
    Names {
        tags: &["es"],
        months: ["enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre", "octubre", "noviembre", "diciembre"],
        mon: ["ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sept", "oct", "nov", "dic"],
        days: ["lunes", "martes", "miércoles", "jueves", "viernes", "sábado", "domingo"],
        dy: ["lun", "mar", "mié", "jue", "vie", "sáb", "dom"],
        styles: ["D/M/YY", "D MMM YYYY", "D [de] MMMM [de] YYYY", "dddd, D [de] MMMM [de] YYYY"],
    },
    Names {
        tags: &["it"],
        months: ["gennaio", "febbraio", "marzo", "aprile", "maggio", "giugno", "luglio", "agosto", "settembre", "ottobre", "novembre", "dicembre"],
        mon: ["gen", "feb", "mar", "apr", "mag", "giu", "lug", "ago", "set", "ott", "nov", "dic"],
        days: ["lunedì", "martedì", "mercoledì", "giovedì", "venerdì", "sabato", "domenica"],
        dy: ["lun", "mar", "mer", "gio", "ven", "sab", "dom"],
        styles: ["DD/MM/YY", "D MMM YYYY", "D MMMM YYYY", "dddd D MMMM YYYY"],
    },
    Names {
        tags: &["nl"],
        months: ["januari", "februari", "maart", "april", "mei", "juni", "juli", "augustus", "september", "oktober", "november", "december"],
        mon: ["jan", "feb", "mrt", "apr", "mei", "jun", "jul", "aug", "sep", "okt", "nov", "dec"],
        days: ["maandag", "dinsdag", "woensdag", "donderdag", "vrijdag", "zaterdag", "zondag"],
        dy: ["ma", "di", "wo", "do", "vr", "za", "zo"],
        styles: ["DD-MM-YYYY", "D MMM YYYY", "D MMMM YYYY", "dddd D MMMM YYYY"],
    },
    Names {
        tags: &["pt"],
        months: ["janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro", "dezembro"],
        mon: ["jan.", "fev.", "mar.", "abr.", "mai.", "jun.", "jul.", "ago.", "set.", "out.", "nov.", "dez."],
        days: ["segunda-feira", "terça-feira", "quarta-feira", "quinta-feira", "sexta-feira", "sábado", "domingo"],
        dy: ["seg.", "ter.", "qua.", "qui.", "sex.", "sáb.", "dom."],
        styles: ["DD/MM/YYYY", "D [de] MMM [de] YYYY", "D [de] MMMM [de] YYYY", "dddd, D [de] MMMM [de] YYYY"],
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
        3 => parse_offset(format!("{}:00", o).as_bytes())?,
        5 if !o.contains(':') => parse_offset(format!("{}:{}", &o[..3], &o[3..]).as_bytes())?,
        _ => parse_offset(o.as_bytes())?,
    };
    stamp_text(day * 86400 + sec - off, &frac, off)
}

fn parse(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let f = "date.parse";
    let s = str_arg(&args, 0, f, line)?;
    let r = match given(&args, 1) {
        None => read_iso(s.trim()).ok_or_else(|| format!("not a date or a date-time: {}", got(args.first()))),
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
