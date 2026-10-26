//! Sensitive values kept out of what Quillo writes down: error messages and
//! pages, `print`, the request log and the audit log's details.
//!
//! ```text
//! redact(["iban", "tax_id"])     // more map keys whose values are never shown
//! ```
//!
//! A value under a sensitive key (`password`, `card_number`, `iban`, ... and
//! those the program adds) shows as `[redacted]`: in maps printed or put in
//! an error message (`{ password: [redacted] }`), in text that reads
//! `password=...` or `"password": "..."`, and in query strings in the
//! request log. Card numbers (13 to 19 digits that pass the Luhn check, in
//! the ranges cards use) and IBANs (whose check digits work out) are masked
//! in that text too, wherever they appear. The program's own values are
//! not changed: a route still reads `form.password`, and a page still
//! shows what it is given.
//!
//! The keys are the process's: a program and the programs it mounts share
//! them.

use crate::interp::{fail, Ctrl, Interp};
use crate::value::*;
use std::borrow::Cow;
use std::sync::RwLock;

type R = Result<Value, Ctrl>;

pub const MASK: &str = "[redacted]";

/// Keys masked with no `redact` call.
const DEFAULT_KEYS: &[&str] = &[
    "password", "password_hash", "passwd", "secret", "client_secret", "api_key", "access_token", "refresh_token", "authorization",
    "cookie", "card_number", "cvv", "cvc", "iban", "ssn",
];

/// Keys the program added, lowercased, `-` read as `_`.
static KEYS: RwLock<Vec<String>> = RwLock::new(Vec::new());

fn norm(k: &str) -> String {
    k.trim().to_ascii_lowercase().replace('-', "_")
}

/// Whether values under `key` are kept out of logs and messages.
pub fn sensitive(key: &str) -> bool {
    let k = norm(key);
    DEFAULT_KEYS.contains(&k.as_str()) || KEYS.read().unwrap_or_else(|e| e.into_inner()).iter().any(|x| *x == k)
}

/// `redact(keys)`: more map keys (a name or a list of them) whose values
/// are masked in error messages, `print` and the logs.
pub fn redact(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let names: Vec<String> = match args.first() {
        Some(Value::Str(s)) => vec![s.to_string()],
        Some(Value::List(l)) => {
            let mut out = Vec::new();
            for v in l.iter() {
                match v {
                    Value::Str(s) => out.push(s.to_string()),
                    v => return fail(line, format!("`redact` expects key names, got {}", v.type_name())),
                }
            }
            out
        }
        other => return fail(line, format!("`redact` expects a key name or a list of them, got {}", other.map_or("nothing", |v| v.type_name()))),
    };
    let mut keys = KEYS.write().unwrap_or_else(|e| e.into_inner());
    for n in names {
        let n = norm(&n);
        if !n.is_empty() && !keys.contains(&n) {
            keys.push(n);
        }
    }
    Ok(Value::Nil)
}

/// `v` with the values under sensitive keys replaced by `[redacted]`, at
/// any depth. Unchanged parts are shared, not copied.
pub fn value(v: &Value) -> Value {
    masked(v).unwrap_or_else(|| v.clone())
}

/// `value`, or None when nothing in `v` is sensitive.
fn masked(v: &Value) -> Option<Value> {
    match v {
        Value::Map(m) => {
            let mut out: Option<Map> = None;
            for (i, (k, x)) in m.iter().enumerate() {
                let nx = if !matches!(x, Value::Nil) && sensitive(k) { Some(Value::str(MASK)) } else { masked(x) };
                if nx.is_some() && out.is_none() {
                    out = Some(m.iter().take(i).map(|(k, v)| (k.clone(), v.clone())).collect());
                }
                if let Some(o) = &mut out {
                    o.insert(k.clone(), nx.unwrap_or_else(|| x.clone()));
                }
            }
            out.map(Value::map)
        }
        Value::List(l) => {
            let items: Vec<Option<Value>> = l.iter().map(masked).collect();
            if items.iter().all(Option::is_none) {
                return None;
            }
            Some(Value::list(items.into_iter().zip(l.iter()).map(|(n, x)| n.unwrap_or_else(|| x.clone())).collect()))
        }
        Value::Err(e) => masked(e).map(Value::err),
        _ => None,
    }
}

fn word_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'-'
}

/// Where the value after `key` (and its `:` or `=`) ends, if `at` is
/// such a value: `"..."`, `'...'`, a bracketed one, or a run up to the
/// next separator.
fn value_end(b: &[u8], at: usize) -> usize {
    match b.get(at) {
        Some(&q @ (b'"' | b'\'')) => {
            let mut i = at + 1;
            while i < b.len() && b[i] != q {
                if b[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            (i + 1).min(b.len())
        }
        Some(&open @ (b'{' | b'[')) => {
            let close = if open == b'{' { b'}' } else { b']' };
            let mut depth = 0;
            let mut i = at;
            while i < b.len() {
                if b[i] == open {
                    depth += 1;
                } else if b[i] == close {
                    depth -= 1;
                    if depth == 0 {
                        return i + 1;
                    }
                }
                i += 1;
            }
            b.len()
        }
        _ => {
            let mut i = at;
            while i < b.len() && !matches!(b[i], b',' | b'}' | b']' | b')' | b'&' | b';' | b' ' | b'\n' | b'\r' | b'\t') {
                i += 1;
            }
            i
        }
    }
}

/// `text` with the values after sensitive keys (`password: "..."`,
/// `"iban":"..."`, `card_number=...`), card numbers and IBANs masked.
pub fn text(text: &str) -> Cow<'_, str> {
    let keyed = keyed(text);
    let numbers = numbers(&keyed);
    match numbers {
        Cow::Borrowed(_) => keyed,
        Cow::Owned(s) => Cow::Owned(s),
    }
}

fn keyed(text: &str) -> Cow<'_, str> {
    let b = text.as_bytes();
    let mut out: Option<String> = None;
    let mut copied = 0;
    let mut i = 0;
    while i < b.len() {
        if !word_char(b[i]) || (i > 0 && word_char(b[i - 1])) {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.len() && word_char(b[i]) {
            i += 1;
        }
        if !sensitive(&text[start..i]) {
            continue;
        }
        let mut j = i;
        if b.get(j) == Some(&b'"') || b.get(j) == Some(&b'\'') {
            j += 1;
        }
        while b.get(j) == Some(&b' ') {
            j += 1;
        }
        if !matches!(b.get(j), Some(b':' | b'=')) {
            continue;
        }
        j += 1;
        while b.get(j) == Some(&b' ') {
            j += 1;
        }
        let end = value_end(b, j);
        if end <= j || &text[j..end] == MASK || &text[j..end] == "null" {
            continue;
        }
        let o = out.get_or_insert_with(|| String::with_capacity(text.len()));
        o.push_str(&text[copied..j]);
        o.push_str(MASK);
        copied = end;
        i = end;
    }
    match out {
        None => Cow::Borrowed(text),
        Some(mut o) => {
            o.push_str(&text[copied..]);
            Cow::Owned(o)
        }
    }
}

fn luhn(digits: &[u8]) -> bool {
    let mut sum = 0u32;
    for (i, d) in digits.iter().rev().enumerate() {
        let mut x = (d - b'0') as u32;
        if i % 2 == 1 {
            x *= 2;
            if x > 9 {
                x -= 9;
            }
        }
        sum += x;
    }
    sum % 10 == 0
}

/// Whether digits start and run as a card's do: Visa, Mastercard, American
/// Express, Discover, Diners, JCB and UnionPay numbers.
fn card_shape(d: &[u8]) -> bool {
    let n = d.len();
    let lead = |k: usize| d[..k].iter().fold(0u32, |a, c| a * 10 + (c - b'0') as u32);
    if n < 13 {
        return false;
    }
    match d[0] {
        b'4' => matches!(n, 13 | 16 | 19),
        b'5' => (51..=55).contains(&lead(2)) && n == 16,
        b'2' => (2221..=2720).contains(&lead(4)) && n == 16,
        b'3' => match lead(2) {
            34 | 37 => n == 15,
            36 | 38 | 39 => (14..=19).contains(&n),
            30 => (300..=305).contains(&lead(3)) && (14..=19).contains(&n),
            35 => (3528..=3589).contains(&lead(4)) && (16..=19).contains(&n),
            _ => false,
        },
        b'6' => (lead(4) == 6011 || (644..=649).contains(&lead(3)) || lead(2) == 65 || lead(2) == 62) && (16..=19).contains(&n),
        _ => false,
    }
}

/// An IBAN's check: its first four characters moved to the end, letters as
/// numbers (A is 10), leaves 1 when divided by 97.
fn iban_ok(s: &[u8]) -> bool {
    if s.len() < 15 || s.len() > 34 || !s[..2].iter().all(u8::is_ascii_uppercase) || !s[2..4].iter().all(u8::is_ascii_digit) {
        return false;
    }
    let mut r = 0u32;
    for &c in s[4..].iter().chain(&s[..4]) {
        let n = match c {
            b'0'..=b'9' => (c - b'0') as u32,
            b'A'..=b'Z' => (c - b'A') as u32 + 10,
            _ => return false,
        };
        r = if n >= 10 { (r * 100 + n) % 97 } else { (r * 10 + n) % 97 };
    }
    r == 1
}

/// The card number or IBAN starting at `i` (at a word's start), as the
/// length of text it covers.
fn number_at(b: &[u8], i: usize) -> Option<usize> {
    // A card: digits, single spaces or dashes between them.
    if (b'2'..=b'6').contains(&b[i]) {
        let mut digits = Vec::new();
        let mut j = i;
        let mut end = i;
        while j < b.len() && digits.len() < 19 {
            if b[j].is_ascii_digit() {
                digits.push(b[j]);
                j += 1;
                end = j;
            } else if (b[j] == b' ' || b[j] == b'-') && b.get(j + 1).map_or(false, u8::is_ascii_digit) && j > i {
                j += 1;
            } else {
                break;
            }
        }
        let alone = end >= b.len() || !b[end].is_ascii_alphanumeric();
        if alone && card_shape(&digits) && luhn(&digits) {
            return Some(end - i);
        }
    }
    // An IBAN: two letters, two digits, then letters and digits, in
    // groups of four or not.
    if b[i].is_ascii_uppercase() {
        let mut chars = Vec::new();
        let mut ends = Vec::new();
        let mut j = i;
        while j < b.len() && chars.len() < 34 {
            if b[j].is_ascii_uppercase() || b[j].is_ascii_digit() {
                chars.push(b[j]);
                j += 1;
                if j >= b.len() || !b[j].is_ascii_alphanumeric() {
                    ends.push((chars.len(), j));
                }
            } else if b[j] == b' ' && b.get(j + 1).map_or(false, |c| c.is_ascii_uppercase() || c.is_ascii_digit()) && j > i {
                j += 1;
            } else {
                break;
            }
        }
        for (n, end) in ends.into_iter().rev() {
            if iban_ok(&chars[..n]) {
                return Some(end - i);
            }
        }
    }
    None
}

fn numbers(text: &str) -> Cow<'_, str> {
    let b = text.as_bytes();
    let mut out: Option<String> = None;
    let mut copied = 0;
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_alphanumeric() && (i == 0 || !b[i - 1].is_ascii_alphanumeric()) {
            if let Some(n) = number_at(b, i) {
                let o = out.get_or_insert_with(|| String::with_capacity(text.len()));
                o.push_str(&text[copied..i]);
                o.push_str(MASK);
                i += n;
                copied = i;
                continue;
            }
        }
        i += 1;
    }
    match out {
        None => Cow::Borrowed(text),
        Some(mut o) => {
            o.push_str(&text[copied..]);
            Cow::Owned(o)
        }
    }
}

/// A URL with the values of its sensitive query parameters masked, for
/// the request log.
pub fn url(u: &str) -> Cow<'_, str> {
    let Some((path, q)) = u.split_once('?') else { return Cow::Borrowed(u) };
    let mut changed = false;
    let parts: Vec<String> = q
        .split('&')
        .map(|p| match p.split_once('=') {
            Some((k, v)) if !v.is_empty() && sensitive(&crate::server::percent_decode(k, true)) => {
                changed = true;
                format!("{}={}", k, MASK)
            }
            _ => p.to_string(),
        })
        .collect();
    if !changed {
        return Cow::Borrowed(u);
    }
    Cow::Owned(format!("{}?{}", path, parts.join("&")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_and_numbers_are_masked() {
        assert_eq!(text(r#"{ password: "hunter2", name: "Ada" }"#), r#"{ password: [redacted], name: "Ada" }"#);
        assert_eq!(text(r#"{"iban":"DE89 3704","n":1}"#), r#"{"iban":[redacted],"n":1}"#);
        assert_eq!(text("card_number=4111111111111111&x=1"), "card_number=[redacted]&x=1");
        assert_eq!(text("paid with 4111 1111 1111 1111 today"), "paid with [redacted] today");
        assert_eq!(text("paid with 4111-1111-1111-1111."), "paid with [redacted].");
        // Not a card: fails the Luhn check, or is a timestamp, or i64's largest.
        assert_eq!(text("4111111111111112"), "4111111111111112");
        assert_eq!(text("at 1792716244123"), "at 1792716244123");
        assert_eq!(text("9223372036854775807"), "9223372036854775807");
        assert_eq!(text("to DE89 3704 0044 0532 0130 00 now"), "to [redacted] now");
        assert_eq!(text("to GB82WEST12345698765432"), "to [redacted]");
        assert_eq!(text("DE89 3704 0044 0532 0130 01"), "DE89 3704 0044 0532 0130 01");
        assert_eq!(text("password_reset is a route"), "password_reset is a route");
        assert_eq!(url("/pay?card_number=4111&ok=1"), "/pay?card_number=[redacted]&ok=1");
        assert_eq!(url("/a?b=c"), "/a?b=c");
    }
}
