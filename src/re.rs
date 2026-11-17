//! `re`: regular expressions that match the same on the server and in the
//! browser.
//!
//! ```text
//! re.test(s, p)  re.find(s, p)  re.find_all(s, p)  re.replace(s, p, with)  re.split(s, p)
//! ```
//!
//! A pattern is written in the part of the syntax that JavaScript's (with
//! the `u` flag) and the `regex` crate's mean the same by: characters,
//! `.`, `[...]`, `\d \w \s \b` and their capitals, `^ $`, groups (`(...)`,
//! `(?:...)`, `(?<name>...)`), `|`, and `* + ? {n} {n,} {n,m}` with a lazy
//! `?`. The rest (lookaround, backreferences, inline flags, `\p{...}`) is an
//! error on both sides. `\d`, `\w`, `\b`, `\s` and `.` mean what they do in
//! JavaScript (ASCII digits and word characters, its whitespace, any
//! character but a line break), which `translate` writes out for the crate.
//! Flags are the last argument: `i`, `m`, `s`. Positions count characters,
//! as `slice` does; matches are found from left to right as JavaScript's
//! `matchAll` does, an empty one included after a longer one ends.
//! The browser's half is in runtime.js (`reCheck`), and must agree.

use crate::interp::{fail, Ctrl, Interp};
use crate::value::*;
use regex::{Regex, RegexBuilder};
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Duration;

type R = Result<Value, Ctrl>;

pub fn install(m: &mut Map) {
    m.insert("test".into(), Value::Builtin("re.test", test));
    m.insert("valid".into(), Value::Builtin("re.valid", valid));
    m.insert("find".into(), Value::Builtin("re.find", find));
    m.insert("find_all".into(), Value::Builtin("re.find_all", find_all));
    m.insert("replace".into(), Value::Builtin("re.replace", replace));
    m.insert("split".into(), Value::Builtin("re.split", split));
}

/// JavaScript's `\s`.
const SPACE: &str = r"\t\n\x0B\x0C\r \u{A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}";
const DIGIT: &str = "0-9";
const WORD: &str = "0-9A-Za-z_";
/// What `{n}` may ask for at most.
const MOST: u32 = 1000;
/// What a pattern may compile to, the crate's program of it: the crate's own default, 10 MiB.
/// A visitor's 21 bytes of pattern held 11 MB a request, and 255 of them 2.8 GB: what held it
/// was the cache, which keeps each now within `CACHE_BYTES` (FEEDBACK71.md, Q-975; branch
/// security). A limit of 4 MB refused a pattern of the UI kit's translation checker
/// (`[^{}]{0,200}(?:\{[^{}]{0,200}\}[^{}]{0,200}){0,20}`), and the website no longer started
/// (found in review). And what a search's lazy DFA may keep, which the crate gave 2 MB.
const MOST_COMPILED: usize = 10 << 20;
const DFA_CACHE: usize = 1 << 20;
/// The sizes a pattern is compiled within, in turn, the first that holds it what it is counted
/// at: every pattern in Quillo's repositories, its website and its 628 plugins is under the
/// first (9 KB at most), and a bigger one, the most each step four times the last, is compiled
/// at most six times once, the larger tries cut short at their limit.
const STEPS: [usize; 6] = [16 << 10, 64 << 10, 256 << 10, 1 << 20, 4 << 20, MOST_COMPILED];
/// What a thread's cache of compiled patterns may hold: each counted at twice its step (its
/// program, forward and backward), 256 at most. Kept only within it: 255 patterns of 11 MB each
/// held 2.8 GB (Q-975), and only those under 64 KB kept made the validators a program writes
/// (`^.{1,255}$`, 352 KB) compile again at every call, 75 times slower (found in review). The
/// oldest go first when one more would not fit: a hit changes nothing, so it costs what it did.
const CACHE_BYTES: usize = 32 << 20;
const CACHE_MOST: usize = 256;
/// What the searches of one call may read in all, as a multiple of the text, before the time
/// they take is looked at (see `matches`; FEEDBACK71.md, Q-976; branch security).
const SCANS: u64 = 64;

/// Checks `p` and writes it for the regex crate (`dot_all`: the `s` flag).
pub fn translate(p: &str, dot_all: bool) -> Result<String, String> {
    let cs: Vec<char> = p.chars().collect();
    let mut out = String::new();
    let mut names: Vec<String> = Vec::new();
    let mut i = 0;
    // Whether what came last can be repeated.
    let mut atom = false;
    let mut depth = 0usize;
    while i < cs.len() {
        let c = cs[i];
        match c {
            '\\' => {
                let (s, n, assertion) = escape(&cs, i, false)?;
                out.push_str(&s);
                i += n;
                atom = !assertion;
            }
            '[' => {
                let (s, n) = class(&cs, i)?;
                out.push_str(&s);
                i += n;
                atom = true;
            }
            '(' => {
                if cs.get(i + 1) == Some(&'?') {
                    match (cs.get(i + 2), cs.get(i + 3)) {
                        (Some(':'), _) => {
                            out.push_str("(?:");
                            i += 3;
                        }
                        (Some('<'), Some(d)) if *d != '=' && *d != '!' => {
                            let start = i + 3;
                            let mut j = start;
                            while j < cs.len() && (cs[j].is_ascii_alphanumeric() || cs[j] == '_') {
                                j += 1;
                            }
                            if j == start || cs[start].is_ascii_digit() || cs.get(j) != Some(&'>') {
                                return Err("a group's name is letters, digits and `_`, as in (?<name>...)".into());
                            }
                            let name: String = cs[start..j].iter().collect();
                            if names.contains(&name) {
                                return Err(format!("two groups are named `{}`", name));
                            }
                            out.push_str(&format!("(?<{}>", name));
                            names.push(name);
                            i = j + 1;
                        }
                        (Some('=' | '!' | '<'), _) => return Err("lookahead and lookbehind ((?= (?! (?<= (?<!) are not supported".into()),
                        _ => return Err("flags go in the last argument (\"i\", \"m\", \"s\"), not in the pattern".into()),
                    }
                } else {
                    out.push('(');
                    i += 1;
                }
                depth += 1;
                atom = false;
            }
            ')' => {
                if depth == 0 {
                    return Err("`)` without its `(`".into());
                }
                depth -= 1;
                out.push(')');
                i += 1;
                atom = true;
            }
            '|' | '^' | '$' => {
                out.push(c);
                i += 1;
                atom = false;
            }
            '.' => {
                out.push_str(if dot_all { "(?s:.)" } else { r"[^\n\r\u{2028}\u{2029}]" });
                i += 1;
                atom = true;
            }
            '*' | '+' | '?' | '{' => {
                let n = quantifier(&cs, i)?;
                if !atom {
                    return Err(format!("nothing to repeat before `{}`", c));
                }
                out.extend(&cs[i..i + n]);
                i += n;
                if cs.get(i) == Some(&'?') {
                    out.push('?');
                    i += 1;
                }
                atom = false;
            }
            ']' | '}' => return Err(format!("`{}` on its own: write `\\{}`", c, c)),
            c => {
                out.push_str(&regex::escape(c.encode_utf8(&mut [0; 4])));
                i += 1;
                atom = true;
            }
        }
    }
    if depth > 0 {
        return Err("`(` without its `)`".into());
    }
    Ok(out)
}

/// The length of the quantifier at `i`.
fn quantifier(cs: &[char], i: usize) -> Result<usize, String> {
    if cs[i] != '{' {
        return Ok(1);
    }
    let lone = || "`{` on its own: write `\\{`, or repeat with {n}, {n,} or {n,m}".to_string();
    let mut j = i + 1;
    let num = |j: &mut usize| -> Option<u32> {
        let start = *j;
        while *j < cs.len() && cs[*j].is_ascii_digit() {
            *j += 1;
        }
        if *j == start {
            return None;
        }
        Some(cs[start..*j].iter().collect::<String>().parse().unwrap_or(u32::MAX))
    };
    let lo = num(&mut j).ok_or_else(lone)?;
    let mut hi = Some(lo);
    if cs.get(j) == Some(&',') {
        j += 1;
        hi = num(&mut j);
    }
    if cs.get(j) != Some(&'}') {
        return Err(lone());
    }
    if lo > MOST || hi.is_some_and(|h| h > MOST) {
        return Err(format!("repeating more than {} times is not supported", MOST));
    }
    if hi.is_some_and(|h| h < lo) {
        return Err("in {n,m}, n is more than m".into());
    }
    Ok(j + 1 - i)
}

/// The escape at `i`: the crate's text for it, its length, and whether it
/// is an assertion (`\b`, `\B`), which cannot be repeated. In a class,
/// `\d` and its kind give what goes between the brackets.
fn escape(cs: &[char], i: usize, in_class: bool) -> Result<(String, usize, bool), String> {
    let Some(&e) = cs.get(i + 1) else { return Err("the pattern ends with `\\`".into()) };
    let lit = |c: char| regex::escape(c.encode_utf8(&mut [0; 4]));
    let set = |inner: &str, neg: bool| match (in_class, neg) {
        (true, false) => inner.to_string(),
        (_, true) => format!("[^{}]", inner),
        (false, false) => format!("[{}]", inner),
    };
    Ok(match e {
        'd' | 'D' => (set(DIGIT, e == 'D'), 2, false),
        'w' | 'W' => (set(WORD, e == 'W'), 2, false),
        's' | 'S' => (set(SPACE, e == 'S'), 2, false),
        'b' | 'B' if in_class => return Err("`\\b` cannot go in [...]".into()),
        'b' => (r"(?-u:\b)".into(), 2, true),
        'B' => (r"(?-u:\B)".into(), 2, true),
        'n' => (r"\n".into(), 2, false),
        't' => (r"\t".into(), 2, false),
        'r' => (r"\r".into(), 2, false),
        'f' => (r"\x0C".into(), 2, false),
        'v' => (r"\x0B".into(), 2, false),
        '0' if !cs.get(i + 2).is_some_and(|c| c.is_ascii_digit()) => (r"\x00".into(), 2, false),
        'x' => {
            let h: String = cs.iter().skip(i + 2).take(2).collect();
            if h.len() != 2 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err("`\\x` takes two hex digits, as in \\x41".into());
            }
            (lit(char::from(u8::from_str_radix(&h, 16).unwrap_or(0))), 4, false)
        }
        'u' => {
            let (c, n) = unicode(cs, i)?;
            (lit(c), n, false)
        }
        '1'..='9' | 'k' => return Err("backreferences (\\1, \\k<name>) are not supported".into()),
        'p' | 'P' => return Err("Unicode classes (\\p{...}) are not supported".into()),
        '^' | '$' | '\\' | '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '/' => (lit(e), 2, false),
        '-' if in_class => (lit(e), 2, false),
        other => return Err(format!("`\\{}` is not one of the escapes patterns have", other)),
    })
}

/// `\uXXXX` (a pair of them for a character beyond the first 65,536) or
/// `\u{X...}`: the character, and the escape's length.
fn unicode(cs: &[char], i: usize) -> Result<(char, usize), String> {
    let bad = || "`\\u` takes four hex digits or {hex}, as in \\u00e9 or \\u{1F600}".to_string();
    let hex = |from: usize, n: usize| -> Option<u32> {
        let h: String = cs.iter().skip(from).take(n).collect();
        (h.len() == n && h.chars().all(|c| c.is_ascii_hexdigit())).then(|| u32::from_str_radix(&h, 16).ok()).flatten()
    };
    if cs.get(i + 2) == Some(&'{') {
        let end = (i + 3..cs.len()).find(|&j| cs[j] == '}').ok_or_else(bad)?;
        let n = if (1..=6).contains(&(end - i - 3)) { hex(i + 3, end - i - 3) } else { None };
        let c = n.and_then(char::from_u32).ok_or_else(bad)?;
        return Ok((c, end + 1 - i));
    }
    let hi = hex(i + 2, 4).ok_or_else(bad)?;
    if (0xD800..0xDC00).contains(&hi) {
        if cs.get(i + 6) == Some(&'\\') && cs.get(i + 7) == Some(&'u') {
            if let Some(lo) = hex(i + 8, 4).filter(|lo| (0xDC00..0xE000).contains(lo)) {
                let c = char::from_u32(0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)).ok_or_else(bad)?;
                return Ok((c, 12));
            }
        }
        return Err("half of a surrogate pair on its own".into());
    }
    Ok((char::from_u32(hi).ok_or_else(|| "half of a surrogate pair on its own".to_string())?, 6))
}

/// The class `[...]` at `i`, and its length.
fn class(cs: &[char], i: usize) -> Result<(String, usize), String> {
    let mut out = String::from("[");
    let mut j = i + 1;
    if cs.get(j) == Some(&'^') {
        out.push('^');
        j += 1;
    }
    if cs.get(j) == Some(&']') {
        return Err("an empty class `[]` matches nothing: write `\\[\\]` for the brackets".into());
    }
    // One item: a character (and its text), or a set such as \d.
    let item = |j: &mut usize| -> Result<(Option<char>, String), String> {
        let c = cs[*j];
        if c == '\\' {
            let e = cs.get(*j + 1).copied();
            let (s, n, _) = escape(cs, *j, true)?;
            let single = match e {
                Some('d' | 'D' | 'w' | 'W' | 's' | 'S') => None,
                Some('n') => Some('\n'),
                Some('t') => Some('\t'),
                Some('r') => Some('\r'),
                Some('f') => Some('\x0C'),
                Some('v') => Some('\x0B'),
                Some('0') => Some('\0'),
                Some('x') => u32::from_str_radix(&cs[*j + 2..*j + 4].iter().collect::<String>(), 16).ok().and_then(char::from_u32),
                Some('u') => Some(unicode(cs, *j)?.0),
                Some(other) => Some(other),
                None => None,
            };
            *j += n;
            return Ok((single, s));
        }
        if c == '[' {
            return Err("write `\\[` for a `[` in [...]".into());
        }
        let next = cs.get(*j + 1).copied();
        if matches!((c, next), ('&', Some('&')) | ('~', Some('~')) | ('-', Some('-'))) {
            return Err(format!("write `{}\\{}` for two `{}` in [...]", c, c, c));
        }
        *j += 1;
        Ok((Some(c), regex::escape(c.encode_utf8(&mut [0; 4]))))
    };
    loop {
        match cs.get(j) {
            None => return Err("`[` without its `]`".into()),
            Some(']') => {
                out.push(']');
                return Ok((out, j + 1 - i));
            }
            _ => {}
        }
        let (lo, text) = item(&mut j)?;
        if cs.get(j) == Some(&'-') && cs.get(j + 1).is_some_and(|c| *c != ']') {
            j += 1;
            let (hi, hi_text) = item(&mut j)?;
            match (lo, hi) {
                (Some(a), Some(b)) if a <= b => out.push_str(&format!("{}-{}", text, hi_text)),
                (Some(_), Some(_)) => return Err("a range in [...] goes from the lower character to the higher".into()),
                _ => return Err("a range in [...] is between two characters, not \\d, \\w or \\s".into()),
            }
        } else {
            out.push_str(&text);
        }
    }
}

/// A thread's compiled patterns (see `CACHE_BYTES`): each with what it is counted at, and the
/// order they came in.
#[derive(Default)]
struct Kept {
    map: HashMap<(String, String), (Regex, usize)>,
    order: std::collections::VecDeque<(String, String)>,
    bytes: usize,
}

thread_local! {
    static CACHE: RefCell<Kept> = RefCell::new(Kept::default());
}

/// The pattern (argument 2) and flags (`flags_at`), compiled.
fn regex_of(args: &[Value], flags_at: usize, f: &str, line: u32) -> Result<Regex, Ctrl> {
    let p = match args.get(1) {
        Some(Value::Str(s)) => s.to_string(),
        other => return fail(line, format!("`{}` expects a str as argument 2, got {}", f, other.map_or("nothing", |v| v.type_name()))),
    };
    let flags = match args.get(flags_at) {
        None | Some(Value::Nil) => String::new(),
        Some(Value::Str(s)) => s.to_string(),
        Some(other) => return fail(line, format!("`{}` expects a str as argument {}, got {}", f, flags_at + 1, other.type_name())),
    };
    let key = (p, flags);
    if let Some(r) = CACHE.with(|c| c.borrow().map.get(&key).map(|(r, _)| r.clone())) {
        return Ok(r);
    }
    let (r, step) = compile_stepped(&key.0, &key.1).or_else(|e| fail(line, format!("`{}`: {}", f, e)))?;
    let counted = step * 2;
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        while !c.order.is_empty() && (c.bytes + counted > CACHE_BYTES || c.map.len() >= CACHE_MOST) {
            let old = c.order.pop_front().expect("not empty");
            if let Some((_, n)) = c.map.remove(&old) {
                c.bytes -= n;
            }
        }
        if counted <= CACHE_BYTES {
            c.bytes += counted;
            c.order.push_back(key.clone());
            c.map.insert(key, (r.clone(), counted));
        }
    });
    Ok(r)
}

/// Whether this thread's cache keeps `p` with `flags`, and what it is counted at (for tests).
pub fn kept(p: &str, flags: &str) -> Option<usize> {
    CACHE.with(|c| c.borrow().map.get(&(p.to_string(), flags.to_string())).map(|(_, n)| *n))
}

/// `compile` at each of `STEPS` in turn: the pattern, and the step it compiled within.
fn compile_stepped(p: &str, flags: &str) -> Result<(Regex, usize), String> {
    for (i, step) in STEPS.iter().enumerate() {
        match compile_within(p, flags, *step) {
            Ok(r) => return Ok((r, *step)),
            Err((true, e)) if i + 1 == STEPS.len() => return Err(e),
            Err((true, _)) => {}
            Err((false, e)) => return Err(e),
        }
    }
    unreachable!("the last step gives the pattern or its error")
}

/// Checks the flags and compiles the pattern; also for `quillo check`.
pub fn compile(p: &str, flags: &str) -> Result<Regex, String> {
    compile_within(p, flags, MOST_COMPILED).map_err(|(_, e)| e)
}

/// `compile`, the pattern held to `limit` bytes compiled: Err((true, ..)) when it is bigger.
fn compile_within(p: &str, flags: &str, limit: usize) -> Result<Regex, (bool, String)> {
    for (k, f) in flags.char_indices() {
        if !matches!(f, 'i' | 'm' | 's') || flags[..k].contains(f) {
            return Err((false, format!("the flags are `i` (any case), `m` (^ and $ at each line) and `s` (. matches line breaks), each once; got \"{}\"", flags)));
        }
    }
    let mut pre = String::new();
    if flags.contains('i') {
        pre.push('i');
    }
    if flags.contains('m') {
        // Lines end at \n or \r, as in JavaScript.
        pre.push_str("mR");
    }
    let body = translate(p, flags.contains('s')).map_err(|e| (false, e))?;
    let full = if pre.is_empty() { body } else { format!("(?{}){}", pre, body) };
    RegexBuilder::new(&full).size_limit(limit).dfa_size_limit(DFA_CACHE).build().map_err(|e| match e {
        regex::Error::CompiledTooBig(_) => (true, format!("the pattern is too big: it compiles to more than the size limit of {} MB a pattern has (a repeat inside a repeat, such as (?:\\w{{1000}}){{100}}, multiplies its size)", MOST_COMPILED >> 20)),
        e => (false, format!("the pattern is too big: {}", e.to_string().lines().last().unwrap_or(""))),
    })
}

/// A match: where it is in bytes, and its groups' (byte) spans.
struct Found {
    start: usize,
    end: usize,
    groups: Vec<Option<(usize, usize)>>,
}

/// Why not every match was found.
enum Stop {
    /// The request computed past `--max-compute`.
    Compute,
    /// The searches read the text over and over for longer than `scan_time` gives them.
    Scans(Duration),
    /// `--max-memory`'s stop.
    Memory(crate::httpd::OverMemory),
}

/// What the searches of a call over `len` bytes may take, of their thread's time computing, once
/// they have read `SCANS` times the text: a quarter of a second and 4 µs a byte, at least 5 times
/// what ordinary tokenizers take a byte in a debug build with ten loops beside it on the cores
/// (Q-976, found in review: wall-clock time, and 0.64 µs a byte, refused them there).
fn scan_time(len: usize) -> Duration {
    Duration::from_millis(250) + Duration::from_nanos((len as u64).saturating_mul(4_000))
}

/// The time this thread has spent computing: what it waited, or another thread's work on its core,
/// does not count. Read with `CLOCK_THREAD_CPUTIME_ID`, which macOS, Linux and the BSDs have.
#[cfg(unix)]
fn thread_time() -> Duration {
    let mut t = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: `t` lives through the call, which only writes it.
    if unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut t) } != 0 {
        return Duration::ZERO;
    }
    Duration::new(t.tv_sec as u64, t.tv_nsec as u32)
}

/// Elsewhere (Windows, which libc gives no thread clock), the time since the first look: a stall
/// counts there too (found in review: the Windows build did not compile).
#[cfg(not(unix))]
fn thread_time() -> Duration {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed()
}

/// The matches from left to right, as JavaScript's `matchAll` gives them:
/// after an empty match, the search goes on a character later.
///
/// Each search may read to the end of the text, and one with an alternation does where an
/// earlier branch must look that far before a later one's match is the answer
/// (`[a-z]+X|[a-z]` on a run of letters): finding every match then reads the text once for each,
/// and 300 KB held a worker 11 s, past `--max-compute`. So every 64th search looks (`look`)
/// whether the request is past `--max-compute`, and once what the searches may have read comes
/// to `SCANS` times the text, at the time they have taken: past `scan_time`, the call stops
/// (FEEDBACK71.md, Q-976; branch security). What they may read cannot tell an ordinary tokenizer
/// from such a pattern (a word at a time, 100 KB is 10,000 times its text), and what they do read
/// the crate does not say, so the time is what decides; up to `SCANS` times the text nothing is
/// timed. The searches go 64 at a time, with nothing looked at between: looked at every search,
/// it cost a search 20 instructions (2% of a 100 KB call). One search (`re.find`) is linear, and
/// looks at nothing.
fn matches(r: &Regex, s: &str, most: usize) -> Result<Vec<Found>, Stop> {
    let mut out = Vec::new();
    let mut pos = 0;
    let mut locs = r.capture_locations();
    let mut may_read = 0u64;
    let mut timed: Option<(Duration, Duration)> = None;
    // What a match takes: itself, and its groups' spans beside it, each match's own (Q-977, found
    // in review: `(a)?` a hundred times over took 2.4 KB a match beside the 40 counted, and 400 KB
    // of text 683 MB under a 16 MB limit).
    let each = std::mem::size_of::<Found>() + locs.len().saturating_sub(1) * std::mem::size_of::<Option<(usize, usize)>>();
    'all: while pos <= s.len() && out.len() < most {
        let until = if most == 1 {
            1
        } else {
            look(&mut out, s, pos, most, each, &mut may_read, &mut timed)?;
            (out.len() + 64).min(most)
        };
        while pos <= s.len() && out.len() < until {
            let Some((start, end)) = r.captures_read_at(&mut locs, s, pos).map(|m| (m.start(), m.end())) else { break 'all };
            let groups = (1..locs.len()).map(|g| locs.get(g)).collect();
            out.push(Found { start, end, groups });
            pos = if end > start { end } else { end + s[end..].chars().next().map_or(1, char::len_utf8) };
        }
    }
    Ok(out)
}

/// What `matches` looks at before each 64 searches: the compute limit, the time the searches
/// take once they may have read `SCANS` times the text (each of the 64 counted as reading from
/// here to the end), and room for 64 more, made as the list would grow and asked about first
/// under `--max-memory` once it is large, `each` bytes a match with its groups (FEEDBACK71.md,
/// Q-976, Q-977; branch security).
#[inline(never)]
fn look(out: &mut Vec<Found>, s: &str, pos: usize, most: usize, each: usize, may_read: &mut u64, timed: &mut Option<(Duration, Duration)>) -> Result<(), Stop> {
    if crate::httpd::OVER_TIME.load(std::sync::atomic::Ordering::Relaxed) && crate::httpd::over_time() {
        return Err(Stop::Compute);
    }
    *may_read = may_read.saturating_add(((s.len() - pos) as u64 + 1) * 64);
    if *may_read > (s.len() as u64 + 1).saturating_mul(SCANS) {
        let (since, allowed) = *timed.get_or_insert_with(|| (thread_time(), scan_time(s.len())));
        if thread_time().saturating_sub(since) > allowed {
            return Err(Stop::Scans(allowed));
        }
    }
    if out.capacity() - out.len() < 64 {
        let want = (out.capacity() * 2).max(64).min(most.max(1));
        crate::httpd::making(want.saturating_mul(each)).map_err(Stop::Memory)?;
        out.reserve_exact(want.saturating_sub(out.len()));
    }
    Ok(())
}

/// `matches` for `f`, its stops as its errors: the compute limit's and the memory limit's end the
/// request; too many readings of the text is an error value (Q-976).
fn all_matches(r: &Regex, s: &str, most: usize, f: &str, line: u32) -> Result<Result<Vec<Found>, Value>, Ctrl> {
    match matches(r, s, most) {
        Ok(found) => Ok(Ok(found)),
        Err(Stop::Compute) => fail(line, crate::httpd::over_compute()),
        Err(Stop::Memory(stop)) => Err(stop.at(line)),
        Err(Stop::Scans(allowed)) => Ok(Err(Value::err(Value::str(format!(
            "`{}`: the pattern scans the text too many times: each search reads on past its match to look for a longer one (as `[a-z]+X|[a-z]` does on a run of letters), and the searches over these {} bytes computed for more than the {:.1} s they may; write it so that a search can stop sooner",
            f,
            s.len(),
            allowed.as_secs_f64()
        ))))),
    }
}

/// A match as a map: `{ text, start, end, groups, named }`, with positions
/// in characters (`chars` counts them up to `at`, from where it got last).
fn match_value(r: &Regex, s: &str, m: &Found, chars: &mut (usize, usize)) -> Value {
    let mut count = |to: usize| {
        if to < chars.0 {
            *chars = (0, 0);
        }
        chars.1 += s[chars.0..to].chars().count();
        chars.0 = to;
        chars.1
    };
    let start = count(m.start);
    let end = count(m.end);
    let text = |g: Option<(usize, usize)>| g.map_or(Value::Nil, |(a, b)| Value::str(&s[a..b]));
    let mut named = Map::new();
    for (i, name) in r.capture_names().enumerate().skip(1) {
        if let Some(n) = name {
            named.insert(n.into(), text(m.groups[i - 1]));
        }
    }
    let mut v = Map::new();
    v.insert("text".into(), Value::str(&s[m.start..m.end]));
    v.insert("start".into(), Value::Int(start as i64));
    v.insert("end".into(), Value::Int(end as i64));
    v.insert("groups".into(), Value::list(m.groups.iter().map(|g| text(*g)).collect()));
    v.insert("named".into(), Value::map(named));
    Value::map(v)
}

fn text_arg(args: &[Value], f: &str, line: u32) -> Result<std::sync::Arc<str>, Ctrl> {
    match args.first() {
        Some(Value::Str(s)) => Ok(s.clone()),
        other => fail(line, format!("`{}` expects a str as argument 1, got {}", f, other.map_or("nothing", |v| v.type_name()))),
    }
}

/// `re.test(s, pattern, flags)`: whether it matches anywhere in `s`.
/// `re.valid(pattern, flags)`: a pattern from data can be asked about first
/// (FEEDBACK25.md, Q-318; branch feedback26-fixes).
fn valid(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let Some(p @ Value::Str(_)) = args.first() else { return Ok(Value::Bool(false)) };
    let asked = [Value::Nil, p.clone(), args.get(1).cloned().unwrap_or(Value::Nil)];
    Ok(Value::Bool(regex_of(&asked, 2, "re.valid", line).is_ok()))
}

fn test(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let s = text_arg(&args, "re.test", line)?;
    Ok(Value::Bool(regex_of(&args, 2, "re.test", line)?.is_match(&s)))
}

/// `re.find(s, pattern, flags)`: the first match, or null.
fn find(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let s = text_arg(&args, "re.find", line)?;
    let r = regex_of(&args, 2, "re.find", line)?;
    let found = match all_matches(&r, &s, 1, "re.find", line)? {
        Ok(found) => found,
        Err(e) => return Ok(e),
    };
    Ok(found.first().map_or(Value::Nil, |m| match_value(&r, &s, m, &mut (0, 0))))
}

/// `re.find_all(s, pattern, flags)`: every match, from left to right.
fn find_all(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let s = text_arg(&args, "re.find_all", line)?;
    let r = regex_of(&args, 2, "re.find_all", line)?;
    let found = match all_matches(&r, &s, usize::MAX, "re.find_all", line)? {
        Ok(found) => found,
        Err(e) => return Ok(e),
    };
    // A map for each, asked about first under `--max-memory` (FEEDBACK71.md, Q-977; branch
    // security).
    crate::httpd::making(found.len().saturating_mul(ITEM_BYTES)).map_err(|s| s.at(line))?;
    let mut chars = (0, 0);
    Ok(Value::list(found.iter().map(|m| match_value(&r, &s, m, &mut chars)).collect()))
}

/// `re.replace(s, pattern, with, flags)`: every match replaced by `with`,
/// text in which `$1`, `$<name>` and `$0` are the match's and `$$` is `$`,
/// or a function given the match (as `re.find` gives it).
fn replace(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let s = text_arg(&args, "re.replace", line)?;
    let r = regex_of(&args, 3, "re.replace", line)?;
    let with = args.get(2).cloned().unwrap_or(Value::Nil);
    if !matches!(with, Value::Str(_) | Value::Func(_) | Value::Builtin(..)) {
        return fail(line, format!("`re.replace` expects what to put in (a str or a function) as argument 3, got {}", with.type_name()));
    }
    let names: Vec<Option<&str>> = r.capture_names().collect();
    let found = match all_matches(&r, &s, usize::MAX, "re.replace", line)? {
        Ok(found) => found,
        Err(e) => return Ok(e),
    };
    // The text it makes, written through a `Held` text, which stops at `--max-memory` as it grows:
    // each match given a long text (FEEDBACK71.md, Q-977; branch security).
    let mut out = Held::with_capacity(s.len());
    let mut last = 0;
    let mut chars = (0, 0);
    for m in found {
        out.push_str(&s[last..m.start]);
        match &with {
            Value::Str(t) => expand(t, &s, &m, &names, &mut out).or_else(|e| fail(line, format!("`re.replace`: {}", e)))?,
            f => {
                let v = match_value(&r, &s, &m, &mut chars);
                // The function's error is given back: its text went into the text, and a `?` in
                // it wrote "err(...)" (FEEDBACK55.md, Q-784; branch feedback55).
                match it.call(f, vec![v], line)? {
                    e @ Value::Err(..) => return Ok(e),
                    v => v.display_into(&mut out),
                }
            }
        }
        if out.is_over() {
            return Err(crate::httpd::OverMemory.at(line));
        }
        last = m.end;
    }
    out.push_str(&s[last..]);
    Ok(Value::str(out.into_text().map_err(|o| o.at(line))?))
}

/// `$0`..`$9`, `$<name>` and `$$` in replacement text.
fn expand(t: &str, s: &str, m: &Found, names: &[Option<&str>], out: &mut Held) -> Result<(), String> {
    let group = |g: usize| if g == 0 { Some((m.start, m.end)) } else { m.groups.get(g - 1).copied().flatten() };
    let mut rest = t;
    while let Some(k) = rest.find('$') {
        out.push_str(&rest[..k]);
        let after = &rest[k + 1..];
        match after.chars().next() {
            Some('$') => {
                out.push('$');
                rest = &after[1..];
            }
            Some(d @ '0'..='9') => {
                let g = d as usize - '0' as usize;
                if g > m.groups.len() {
                    return Err(format!("`${}`, but the pattern has {} group{}", g, m.groups.len(), if m.groups.len() == 1 { "" } else { "s" }));
                }
                if let Some((a, b)) = group(g) {
                    out.push_str(&s[a..b]);
                }
                rest = &after[1..];
            }
            Some('<') => {
                let end = after.find('>').ok_or("`$<` without its `>`")?;
                let name = &after[1..end];
                let g = names.iter().position(|n| *n == Some(name)).ok_or_else(|| format!("no group is named `{}`", name))?;
                if let Some((a, b)) = group(g) {
                    out.push_str(&s[a..b]);
                }
                rest = &after[end + 1..];
            }
            _ => {
                out.push('$');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    Ok(())
}

/// `re.split(s, pattern, flags)`: the text between the matches.
fn split(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let s = text_arg(&args, "re.split", line)?;
    let r = regex_of(&args, 2, "re.split", line)?;
    let mut out = Vec::new();
    let mut last = 0;
    let found = match all_matches(&r, &s, usize::MAX, "re.split", line)? {
        Ok(found) => found,
        Err(e) => return Ok(e),
    };
    // The list of parts, asked about first under `--max-memory` (Q-977).
    crate::httpd::making((found.len() + 1).saturating_mul(ITEM_BYTES)).map_err(|s| s.at(line))?;
    for m in found {
        out.push(Value::str(&s[last..m.start]));
        last = m.end;
    }
    out.push(Value::str(&s[last..]));
    Ok(Value::list(out))
}
