//! `date` (src/dates.rs): text read and written back the same, and every
//! function given dates, date-times, numbers, zones, calendars, patterns
//! and nonsense, near the ends of the calendar too: an answer or an error,
//! never a panic.
#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use quillo::value::{Map, Value};
use quillo::Interp;
use std::cell::RefCell;

#[derive(Arbitrary, Debug)]
enum Arg {
    Text(String),
    /// A date: days from 1970-01-01, kept within years 0 to 10000.
    Day(i32),
    /// A date-time: the day, the seconds, a fraction and an offset.
    Stamp(i32, u32, Option<u16>, i16),
    Int(i64),
    Float(f64),
    Zone(u8),
    Calendar(Vec<i32>, Vec<i8>),
    Options(Option<String>, Option<String>, Option<String>),
    Null,
}

const ZONES: &[&str] = &[
    "UTC", "Europe/Paris", "America/New_York", "Asia/Kolkata", "Australia/Lord_Howe", "Pacific/Chatham",
    "America/St_Johns", "Pacific/Kiritimati", "Etc/GMT+12", "Antarctica/Troll", "Africa/Monrovia", "Nowhere/Else", "",
];

fn day_text(z: i32) -> String {
    let z = (z as i64).rem_euclid(3_652_500) - 719_528;
    let (y, m, d) = quillo::dates::civil(z);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

fn value(a: &Arg) -> Value {
    match a {
        Arg::Text(s) => Value::str(s),
        Arg::Day(z) => Value::str(day_text(*z)),
        Arg::Stamp(z, sec, frac, off) => {
            let sec = sec % 86400;
            let frac = frac.map_or(String::new(), |f| format!(".{}", f));
            let off = *off as i64 % (24 * 60);
            let zone = if off == 0 { "Z".to_string() } else { format!("{}{:02}:{:02}", if off < 0 { '-' } else { '+' }, off.abs() / 60, off.abs() % 60) };
            Value::str(format!("{}T{:02}:{:02}:{:02}{}{}", day_text(*z), sec / 3600, sec / 60 % 60, sec % 60, frac, zone))
        }
        Arg::Int(n) => Value::Int(*n),
        Arg::Float(f) => Value::Float(*f),
        Arg::Zone(z) => Value::str(ZONES[*z as usize % ZONES.len()]),
        Arg::Calendar(holidays, weekend) => {
            let mut m = Map::new();
            m.insert("holidays".into(), Value::list(holidays.iter().map(|z| Value::str(day_text(*z))).collect()));
            m.insert("weekend".into(), Value::list(weekend.iter().map(|d| Value::Int(*d as i64)).collect()));
            Value::map(m)
        }
        Arg::Options(locale, style, pattern) => {
            let mut m = Map::new();
            for (k, v) in [("locale", locale), ("style", style), ("pattern", pattern)] {
                if let Some(v) = v {
                    m.insert(k.into(), Value::str(v));
                }
            }
            Value::map(m)
        }
        Arg::Null => Value::Nil,
    }
}

#[derive(Arbitrary, Debug)]
struct Input {
    text: String,
    f: u8,
    args: Vec<Arg>,
}

thread_local! {
    static IT: RefCell<(Interp, Vec<Value>)> = RefCell::new({
        let it = Interp::new(std::env::temp_dir());
        let Some(Value::Map(date)) = quillo::value::lookup(&it.globals, "date") else { panic!("no date module") };
        // Not the clock's: their answers change, and are not read from input.
        let fns = date.iter().filter(|(k, _)| !matches!(k.as_str(), "today" | "now")).map(|(_, f)| f.clone()).collect();
        (it, fns)
    });
}

fuzz_target!(|i: Input| {
    quillo::dates::fuzzing::round_trip(&i.text);
    if i.args.len() > 6 {
        return;
    }
    IT.with(|cell| {
        let (it, fns) = &mut *cell.borrow_mut();
        let f = fns[i.f as usize % fns.len()].clone();
        let _ = it.call(&f, i.args.iter().map(value).collect(), 0);
    });
});
