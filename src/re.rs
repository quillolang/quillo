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
use regex_automata::{hybrid, nfa::thompson, util::prefilter::Prefilter, Anchored, Input, MatchKind};
use std::cell::RefCell;
use std::rc::Rc;
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
    map: HashMap<(String, String), (Regex, usize, Room)>,
    order: std::collections::VecDeque<(String, String)>,
    bytes: usize,
}

thread_local! {
    static CACHE: RefCell<Kept> = RefCell::new(Kept::default());
}

/// The pattern (argument 2) and flags (`flags_at`), compiled.
fn regex_of(args: &[Value], flags_at: usize, f: &str, line: u32) -> Result<Regex, Ctrl> {
    pattern(args, flags_at, f, line, 0, false).map(|pat| pat.r)
}

/// A pattern as the searches take it: compiled, and how long a text the crate may search for it
/// (`Room`). Its text and flags, which its `Guard` is made from, are read again from the call's
/// arguments where one is needed (`Key`), and not carried by each call (found in review: 130
/// instructions a call). `r` is `unused()` until a call takes the crate's path (`Room::gate` shut
/// until then).
struct Pat {
    r: Regex,
    room: Room,
}

/// Where a call's pattern and flags are, for the guard.
#[derive(Clone, Copy)]
struct Key<'a> {
    args: &'a [Value],
    flags_at: usize,
}

impl Key<'_> {
    fn strings(&self) -> (String, String) {
        let text = |v: Option<&Value>| match v {
            Some(Value::Str(s)) => s.to_string(),
            _ => String::new(),
        };
        (text(self.args.get(1)), text(self.args.get(self.flags_at)))
    }
}

/// `regex_of`, with the rest of `Pat`: for a call on `len` bytes, asking for groups or not (`caps`),
/// which a pattern not yet kept is compiled for (`nfa_first`).
fn pattern(args: &[Value], flags_at: usize, f: &str, line: u32, len: usize, caps: bool) -> Result<Pat, Ctrl> {
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
    if let Some(pat) = CACHE.with(|c| c.borrow().map.get(&key).map(|(r, _, room)| Pat { r: r.clone(), room: *room })) {
        return Ok(pat);
    }
    compile_and_keep(key, f, line, len, caps)
}

/// `pattern` for one not kept yet: compiled for the first call (`nfa_first`), and kept.
#[cold]
#[inline(never)]
fn compile_and_keep(key: (String, String), f: &str, line: u32, len: usize, caps: bool) -> Result<Pat, Ctrl> {
    if len > NFA_FIRST {
        if let Some(pat) = nfa_first(&key, len, caps) {
            return Ok(pat);
        }
    }
    let (r, step) = compile_stepped(&key.0, &key.1).or_else(|e| fail(line, format!("`{}`: {}", f, e)))?;
    // From the size it compiled within, which no NFA of it passes (see `Room`).
    let room = Room::of(step as u128 / STATE_BYTES, step as u128 / RANGE_BYTES, 2 * r.captures_len() as u128, false);
    store(&key, &r, step * 2, room);
    Ok(Pat { r, room })
}

/// From how long a text a pattern not yet kept is first compiled as the guard compiles it (its NFA,
/// and its exact `Room`), and the crate's Regex only where that room holds the text: below it, the
/// crate's first, as before. A pattern that only ever went by the guard was compiled for both, the
/// UI kit's checker and a 600-word list on 1 MB 15 to 17% slower than 0.8.8 (found measuring).
const NFA_FIRST: usize = 64 << 10;

/// `pattern`'s first compile for a long text (`NFA_FIRST`), kept: the crate's Regex where the room
/// holds the text, else the NFA alone for the guard, the Regex `unused()` until a call needs it
/// (`guard_past_room`). None where it cannot be compiled so, which the crate's compile then says.
fn nfa_first(key: &(String, String), len: usize, caps: bool) -> Option<Pat> {
    // A pattern the crate searches with literals alone is the crate's, as in 0.8.8 (`Room::literal`:
    // past its room, in windows), and its NFA is never made: 5,000 hashes of 40 characters, whose NFA is past
    // the 10 MiB a guard may compile, were "too costly" on 68 KB, and word lists of 10,000 and 50,000
    // on 1 MB took 5.6 and 3.9 times 0.8.8's instructions by the guard; a list of 3,000 words or more
    // is told from its text, as parsing it took half as long as the crate's whole compile
    // (FEEDBACK75.md follow-up found by 0.8.9's coverage, Q-1125; branch feedback75).
    if let Some(longest) = many_literals(&key.0, &key.1) {
        return crate_literal(key, Room::literal(longest, 1));
    }
    let hir = regex_automata::util::syntax::parse(&full_pattern(&key.0, &key.1).ok()?).ok()?;
    if let Some((longest, factor)) = crate_searches_literals(&hir) {
        return crate_literal(key, Room::literal(longest, factor));
    }
    let nfa = thompson::Compiler::new().configure(guard_nfa(false)).build_from_hir(&hir).ok()?;
    let room = room_of(&nfa);
    if room.fits(len, caps) {
        let (r, step) = compile_stepped(&key.0, &key.1).ok()?;
        store(key, &r, step * 2, room);
        return Some(Pat { r, room });
    }
    // Kept for the guard alone only where the crate would compile it too: the crate compiles its NFA
    // back as well, within the same limit, and refuses the pattern where that one is past it, which
    // `(?:\S{1000}){5}` is (forward 5.5 MB, back past 10 MiB). Kept unchecked, such a pattern first
    // searched on a long text was searched, and then `re.valid` said true and calls on short texts
    // answered, where in a new thread each was "the pattern is too big", as in 0.8.8. The NFA back
    // goes to the guard, which compiled the same one the first time a match needed it (FEEDBACK75.md
    // follow-up found by 0.8.9's coverage, Q-1126; branch feedback75).
    let rev = thompson::Compiler::new().configure(guard_nfa(true)).build_from_hir(&hir).ok()?;
    // Counted as the crate's would be (twice the step its program fits, forward and back).
    let room = room.with_crate(false);
    let r = unused();
    store(key, &r, 2 * step_for(nfa.memory_usage()), room);
    MADE.with(|m| *m.borrow_mut() = Some((key.clone(), Made { nfa, rev: Some(rev) })));
    Some(Pat { r, room })
}

/// `key` compiled by the crate and kept with `room`, a `Room::literal` (Q-1125); None where the crate
/// refuses it, which its compile then says.
fn crate_literal(key: &(String, String), room: Room) -> Option<Pat> {
    let (r, step) = compile_stepped(&key.0, &key.1).ok()?;
    store(key, &r, step * 2, room);
    Some(Pat { r, room })
}

/// Whether the crate searches `hir` with literals alone, building no NFA: regex-automata 0.4.18's two
/// "Pre" strategies, which `meta::strategy::new` takes before any regex engine, where the pattern has
/// no group and no assertion: a prefilter of its prefixes where they are exact (`Pre::from_prefixes`:
/// `\d`, `[;{}]`, `cat|dog`, the prefixes as `build_guard` takes them), and Aho-Corasick for an
/// alternation of 3,000 literals or more (`Pre::from_alternation_literals`). Where it does, its
/// longest literal in bytes and its factor (`LITERAL_PS`) (Q-1125).
fn crate_searches_literals(hir: &regex_syntax::hir::Hir) -> Option<(usize, usize)> {
    let props = hir.properties();
    if props.explicit_captures_len() != 0 || !props.look_set().is_empty() {
        return None;
    }
    if let (true, regex_syntax::hir::HirKind::Alternation(alts)) = (props.is_alternation_literal(), hir.kind()) {
        if alts.len() >= LITERALS_FOR_AC {
            return Some((alts.iter().filter_map(|a| a.properties().maximum_len()).max().unwrap_or(0), 1));
        }
    }
    let mut extractor = regex_syntax::hir::literal::Extractor::new();
    extractor.kind(regex_syntax::hir::literal::ExtractKind::Prefix);
    let mut seq = regex_syntax::hir::literal::Seq::empty();
    seq.union(&mut extractor.extract(hir));
    seq.optimize_for_prefix_by_preference();
    let lits = seq.literals().filter(|_| seq.is_exact())?;
    Prefilter::new(MatchKind::LeftmostFirst, lits)?;
    let longest = seq.max_literal_len().unwrap_or(0);
    // Teddy, or Rabin-Karp under it, checks each literal a candidate may be, 128 at the most, and a
    // literal of one byte is checked once.
    Some((longest, if longest <= 1 { 1 } else { lits.len().min(128) }))
}

/// What the crate's search with literals alone costs at its worst, in picoseconds a byte of the text
/// for each byte of the longest literal and one, times its factor: from each place a literal may
/// start, the search may read the longest one through before it knows (a list of 3,000 with one of
/// 20,001 bytes read them after each of 2 MB of `a`, past 8 s in `re.replace`), and each literal
/// that may start there may be read, the factor (`crate_searches_literals`: 64 literals of 100 bytes
/// that share 98 took 450 ns a byte of `a`, 0.07 ns a byte for each). 5 times what was measured, as
/// a text within the room is searched with no clock (FEEDBACK75.md follow-up found by 0.8.9's
/// coverage, Q-1125, review; branch feedback75).
const LITERAL_PS: u128 = 5_000;

/// How many literals an alternation has for the crate to search it by Aho-Corasick
/// (regex-automata's `meta::literal::alternation_literals`).
const LITERALS_FOR_AC: usize = 3000;

/// Whether `p` with `flags` is, from its text alone, an alternation of `LITERALS_FOR_AC` literals or
/// more, which the crate searches by Aho-Corasick (`crate_searches_literals`), and the bytes of its
/// longest part where it is: each part plain
/// characters and the escapes `translate` writes as a character, no part empty (an empty one is no
/// literal), not every part one character (which the parser makes a class of), and no `i` (which
/// makes classes of letters). Read without parsing it, which took 31 ms for 50,000 words, more than
/// half the crate's own compile of them, and in passes the machine makes wide, as reading it a
/// character at a time took 1% of that compile (Q-1125). None for a list that escapes `|` or `\`,
/// or whose parts are 4 bytes or fewer on average, whose parse then tells.
fn many_literals(p: &str, flags: &str) -> Option<usize> {
    let bytes = p.as_bytes();
    // The parts' number and the longest's bytes, more than its literal's where it has an escape.
    let (mut bars, mut longest, mut from) = (0, 0, 0);
    for at in memchr::memchr_iter(b'|', bytes) {
        bars += 1;
        longest = longest.max(at - from);
        from = at + 1;
    }
    let longest = longest.max(bytes.len() - from);
    if flags.contains('i') || bars + 1 < LITERALS_FOR_AC {
        return None;
    }
    // No part empty: no `|` first, last or after another (no escape is one, below).
    if bytes.first() == Some(&b'|') || bytes.last() == Some(&b'|') || memchr::memmem::find(bytes, b"||").is_some() {
        return None;
    }
    // Each escape a character: a metacharacter after `\` (`\.`) as it is, the rest as `translate`
    // reads them. The metacharacters are then the escapes' alone, and each escape of `n` characters
    // (one byte each) is `n - 1` bytes more than the one character it is.
    let (mut escaped, mut fewer, mut past) = (0, 0, 0);
    for at in memchr::memchr_iter(b'\\', bytes) {
        if at < past {
            continue; // the second half of `\uD83D\uDE00`
        }
        let n = match bytes.get(at + 1) {
            Some(b'^' | b'$' | b'.' | b'*' | b'+' | b'?' | b'(' | b')' | b'[' | b']' | b'{' | b'}') => {
                escaped += 1;
                2
            }
            Some(b'/') => 2,
            Some(b'|' | b'\\') | None => return None,
            Some(_) => {
                let cs: Vec<char> = p[at..].chars().take(12).collect();
                match (cs[1], escape(&cs, 0, false)) {
                    ('d' | 'D' | 'w' | 'W' | 's' | 'S', _) | (_, Err(_) | Ok((_, _, true))) => return None,
                    (_, Ok((_, n, false))) => {
                        // `\u{1F600}`'s braces.
                        escaped += bytes[at..at + n].iter().filter(|b| b"[](){}.*+?^$".contains(b)).count();
                        n
                    }
                }
            }
        };
        fewer += n - 1;
        past = at + n;
    }
    let metas: usize = [b"[](", b"){}", b".*+", b"?^$"].iter().map(|m| memchr::memchr3_iter(m[0], m[1], m[2], bytes).count()).sum();
    // A character is 4 bytes at the most: parts of more than 4 bytes on average, each escape counted
    // as one, have one of 2 characters or more (counting the characters took a third of the time).
    (metas == escaped && bytes.len() - fewer - bars > 4 * (bars + 1)).then_some(longest)
}

/// The least of `STEPS` that `bytes` of compiled program fit.
fn step_for(bytes: usize) -> usize {
    STEPS.iter().copied().find(|s| bytes <= *s).unwrap_or(MOST_COMPILED)
}

/// `key` kept in this thread's cache as `r`, counted at `counted`, with `room`, in place of what was
/// kept for it; the oldest go first where it would not fit (see `CACHE_BYTES`).
fn store(key: &(String, String), r: &Regex, counted: usize, room: Room) {
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if let Some((_, n, _)) = c.map.remove(key) {
            c.bytes -= n;
            c.order.retain(|k| k != key);
        }
        while !c.order.is_empty() && (c.bytes + counted > CACHE_BYTES || c.map.len() >= CACHE_MOST) {
            let old = c.order.pop_front().expect("not empty");
            if let Some((_, n, _)) = c.map.remove(&old) {
                c.bytes -= n;
            }
        }
        if counted <= CACHE_BYTES {
            c.bytes += counted;
            c.order.push_back(key.clone());
            c.map.insert(key.clone(), (r.clone(), counted, room));
        }
    });
}

thread_local! {
    /// What a kept pattern holds in place of the crate's Regex until a call takes the crate's path:
    /// never searched with, nor its names read, as its `Room`'s gate is shut until then.
    static UNUSED: Regex = Regex::new("").expect("the empty pattern compiles");
    /// The NFA `nfa_first` compiled for a pattern it kept for the guard alone, until the guard is
    /// made of it in the same call (`exact_room`), out of `Pat`, which each call makes.
    static MADE: RefCell<Option<((String, String), Made)>> = const { RefCell::new(None) };
}

fn unused() -> Regex {
    UNUSED.with(Regex::clone)
}

/// Whether this thread's cache keeps `p` with `flags`, and what it is counted at (for tests).
pub fn kept(p: &str, flags: &str) -> Option<usize> {
    CACHE.with(|c| c.borrow().map.get(&(p.to_string(), flags.to_string())).map(|(_, n, _)| *n))
}

/// What `many_literals` and `crate_searches_literals` say of `p` with `flags` (for tests): the longest
/// part's bytes where its text shows an alternation the crate searches by Aho-Corasick, and the
/// longest literal and its factor where the crate searches it with literals alone (None where it
/// does not parse) (Q-1125).
pub fn literal_verdicts(p: &str, flags: &str) -> (Option<usize>, Option<(usize, usize)>) {
    let hir = full_pattern(p, flags).ok().and_then(|full| regex_automata::util::syntax::parse(&full).ok());
    (many_literals(p, flags), hir.as_ref().and_then(crate_searches_literals))
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
    let full = full_pattern(p, flags)?;
    RegexBuilder::new(&full).size_limit(limit).dfa_size_limit(DFA_CACHE).build().map_err(|e| match e {
        regex::Error::CompiledTooBig(_) => (true, format!("the pattern is too big: it compiles to more than the size limit of {} MB a pattern has (a repeat inside a repeat, such as (?:\\w{{1000}}){{100}}, multiplies its size)", MOST_COMPILED >> 20)),
        e => (false, format!("the pattern is too big: {}", e.to_string().lines().last().unwrap_or(""))),
    })
}

/// The flags checked and the pattern written for the crates, the flags in front.
fn full_pattern(p: &str, flags: &str) -> Result<String, (bool, String)> {
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
    Ok(if pre.is_empty() { body } else { format!("(?{}){}", pre, body) })
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
    /// A guarded search gave up (`Guard`).
    GaveUp,
}

/// What the searches of a call over `len` bytes may take, of their thread's time computing, once
/// they have read `SCANS` times the text: a quarter of a second and 4 µs a byte, at least 5 times
/// what ordinary tokenizers take a byte in a debug build with ten loops beside it on the cores
/// (Q-976, found in review: wall-clock time, and 0.64 µs a byte, refused them there).
fn scan_time(len: usize) -> Duration {
    Duration::from_millis(250) + Duration::from_nanos((len as u64).saturating_mul(4_000))
}

/// What one search by the crate costs a byte at its worst, the PikeVM following every state of the
/// NFA, in picoseconds: each state (`STATE_PS`), each range a state's transitions are scanned
/// through (`RANGE_PS`: a class of 64 ranges is scanned whole), and, where the groups are asked
/// for, each slot each state carries (`SLOT_PS`: the PikeVM copies them all for every thread).
/// Measured on the server, under load, as upper bounds with room: 4 ns a state (`[ab]*a[ab]{900}c`,
/// 909 states, 0.74 s on 200 KB), 13 ns a state for 63 ranges each (0.81 s on 69 KB), 0.04 ns a
/// state and slot (`(?:(a)|(a)|...)*` of 1,000, 3,007 states and 2,002 slots, 5.4 s on 21 KB), and
/// no family of 25 adversarial ones, nor 1,600 fuzzed patterns, past 0.78 of the bound
/// (FEEDBACK75.md, Q-1075, third review; branch feedback75).
const STATE_PS: u128 = 6_000;
const RANGE_PS: u128 = 250;
const SLOT_PS: u128 = 80;

/// How much slower a debug build's guard steps and backtracks: its clock (`Clock`) and its
/// backtracker's cost (`TRACK_PS`) are both this many times longer, so that it decides as a
/// release build does (found running the tests there).
const SLOWER: u32 = if cfg!(debug_assertions) { 20 } else { 1 };

/// What the bounded backtracker costs a state a byte of a match, at the most, in picoseconds (see
/// `Guarding::track_long`): measured 0.055 to 0.38 ns, the most with 2,002 and 6,002 slots alike
/// (`(?:(a)|...)*` of 1,000 and 3,000), with room (FEEDBACK75.md, Q-1075).
const TRACK_PS: u128 = 1_000 * SLOWER as u128;

/// What a guarded call may compute a byte of its text, past its tenth of a second (see `Clock`):
/// 3.9 times the slowest real pattern measured, every pattern of the website, its plugins, the UI
/// kit and the starter, the kit's translation checker and a 600-word alternation with `i`, on 10 MB
/// of prose, JSON, log lines and mixed text (`[^ims]` on log lines, 38.5 ns a byte for the whole
/// `re.replace`) (FEEDBACK75.md, Q-1075; branch feedback75). A debug build's (`SLOWER`) is longer.
const GUARD_PER_BYTE: Duration = Duration::from_nanos(150 * SLOWER as u64);

/// What one search by the crate may compute at its worst: Q-976's quarter of a second.
const GUARD_BUDGET_PS: u128 = 250_000_000_000;

/// What a state and a range take, at least, of the size an NFA is compiled within (regex-automata's
/// builder counts each state at 32 bytes and each range of a state's transitions at 8): a pattern
/// compiled within `step` has no more than `step / 32` states nor `step / 8` ranges.
const STATE_BYTES: u128 = 32;
const RANGE_BYTES: u128 = 8;

/// How long a text the crate may search for a pattern, its worst (see `STATE_PS`) within
/// `GUARD_BUDGET_PS`: `test` where no group is asked for (`re.test`), `caps` where they are, and
/// what a byte costs with them, for the guard's own search of a match's groups. From the size the
/// pattern compiled within until a text is longer than that allows (`exact` false), then from its
/// NFA's own states, ranges and slots, counted once (`exact_room`). The crate guarantees no more
/// than that for any pattern, whatever its shape; a search past it goes by the pattern's `Guard`.
/// Each call compares its text's length with one of them: one `re.test` of `[ab]*a[ab]{900}c` on
/// 200 KB held an instance 5 s, past `--max-compute`, and the shapes looked for before missed
/// patterns written out, cases, escapes, groups and wide classes (FEEDBACK75.md, Q-1075; branch
/// feedback75).
#[derive(Clone, Copy)]
struct Room {
    /// What a call's length is compared with, the one compare a call makes (`holds`): one past the
    /// room (`test`, `caps`) where the crate's Regex is compiled, nothing where it is not.
    gate: (usize, usize),
    /// What a byte costs at the crate's worst without groups and with them, which the room is.
    test_ps: u64,
    caps_ps: u64,
    exact: bool,
    /// For a pattern the crate searches with literals alone (`Room::literal`), the longest of them,
    /// in bytes, which a call past the room reads the text in windows of (`windowed_matches`); 0 for
    /// any other.
    longest: usize,
}

impl Room {
    fn of(states: u128, ranges: u128, slots: u128, exact: bool) -> Room {
        let test_ps = STATE_PS.saturating_mul(states).saturating_add(RANGE_PS.saturating_mul(ranges)).max(1);
        let caps_ps = test_ps.saturating_add(SLOT_PS.saturating_mul(states).saturating_mul(slots));
        let ps = |ps: u128| u64::try_from(ps).unwrap_or(u64::MAX);
        Room { gate: (0, 0), test_ps: ps(test_ps), caps_ps: ps(caps_ps), exact, longest: 0 }.with_crate(true)
    }

    /// No room at all, for a pattern its NFA cannot be compiled of again.
    fn none() -> Room {
        Room { gate: (0, 0), test_ps: u64::MAX, caps_ps: u64::MAX, exact: true, longest: 0 }.with_crate(true)
    }

    /// The room of a pattern the crate searches with literals alone (`crate_searches_literals`),
    /// building no NFA, whose `longest` literal is that many bytes, and of `factor` (see
    /// `LITERAL_PS`); the crate's on any text within it, as 0.8.8 searched it, and in windows past it,
    /// with the guard's clock (FEEDBACK75.md follow-up found by 0.8.9's coverage, Q-1125; branch
    /// feedback75).
    fn literal(longest: usize, factor: usize) -> Room {
        let ps = LITERAL_PS.saturating_mul(longest as u128 + 1).saturating_mul(factor.max(1) as u128);
        let ps = u64::try_from(ps).unwrap_or(u64::MAX);
        Room { gate: (0, 0), test_ps: ps, caps_ps: ps, exact: true, longest: longest.max(1) }.with_crate(true)
    }

    /// How long a text the crate may search, without groups and with them.
    fn test(&self) -> usize {
        usize::try_from(GUARD_BUDGET_PS / u128::from(self.test_ps.max(1))).unwrap_or(usize::MAX)
    }

    fn caps(&self) -> usize {
        usize::try_from(GUARD_BUDGET_PS / u128::from(self.caps_ps.max(1))).unwrap_or(usize::MAX)
    }

    /// Whether the crate's Regex is compiled (see `nfa_first`): the gate is open, at 1 at least.
    fn compiled(&self) -> bool {
        self.gate.0 != 0
    }

    /// It, the crate's Regex compiled or not.
    fn with_crate(self, compiled: bool) -> Room {
        let gate = if compiled { (self.test().saturating_add(1), self.caps().saturating_add(1)) } else { (0, 0) };
        Room { gate, ..self }
    }

    /// Whether a call on `len` bytes goes to the crate at once: within the room, and compiled.
    #[inline]
    fn holds(&self, len: usize, caps: bool) -> bool {
        len < if caps { self.gate.1 } else { self.gate.0 }
    }

    /// Whether a text of `len` bytes is within it, compiled or not.
    fn fits(&self, len: usize, caps: bool) -> bool {
        len <= if caps { self.caps() } else { self.test() }
    }
}

/// `pat`'s room from its NFA as the crate compiles it, its states, the ranges of their transitions
/// and its slots: counted the first time a text is too long for its room from size, and kept with
/// the pattern; none at all where it cannot be compiled again.
/// Also the NFA, which the guard is made from where the room is too small: compiled once for both, where twice made the UI kit's translation checker on 1 MB slower than
/// 0.8.8 (found measuring: 6 ms of 11).
fn exact_room(pat: &mut Pat, key: &(String, String)) -> (Room, Option<Made>) {
    if pat.room.exact {
        let made = MADE.with(|m| {
            let mut m = m.borrow_mut();
            if m.as_ref().is_some_and(|(k, _)| k == key) {
                m.take().map(|(_, made)| made)
            } else {
                None
            }
        });
        return (pat.room, made);
    }
    let (room, made) = 'room: {
        // A pattern the crate searches with literals alone (a room from size is only ever the
        // crate's, compiled) has its own room, its NFA never made (FEEDBACK75.md follow-up found by
        // 0.8.9's coverage, Q-1125; branch feedback75; see `nfa_first`).
        if let Some(longest) = many_literals(&key.0, &key.1) {
            break 'room (Room::literal(longest, 1), None);
        }
        let hir = full_pattern(&key.0, &key.1).ok().and_then(|full| regex_automata::util::syntax::parse(&full).ok());
        if let Some((longest, factor)) = hir.as_ref().and_then(crate_searches_literals) {
            break 'room (Room::literal(longest, factor), None);
        }
        let made = hir.and_then(|hir| thompson::Compiler::new().configure(guard_nfa(false)).build_from_hir(&hir).ok().map(|nfa| Made { nfa, rev: None }));
        (made.as_ref().map_or_else(Room::none, |m| room_of(&m.nfa)).with_crate(pat.room.compiled()), made)
    };
    pat.room = room;
    CACHE.with(|c| {
        if let Some(e) = c.borrow_mut().map.get_mut(key) {
            e.2 = room;
        }
    });
    (room, made)
}

/// `nfa`'s room, from its states, the ranges of their transitions and its slots (see `Room`).
fn room_of(nfa: &thompson::NFA) -> Room {
    let ranges: u128 = nfa
        .states()
        .iter()
        .map(|st| match st {
            thompson::State::Sparse(t) => t.transitions.len() as u128,
            thompson::State::ByteRange { .. } | thompson::State::Dense(_) => 1,
            _ => 0,
        })
        .sum();
    Room::of(nfa.states().len() as u128, ranges, nfa.group_info().slot_len() as u128, true)
}

/// A pattern's NFA forward, as `guard_nfa(false)` compiles it, and back where `nfa_first` compiled
/// it to know that the crate compiles the pattern (Q-1126), which the guard then takes.
struct Made {
    nfa: thompson::NFA,
    rev: Option<thompson::NFA>,
}

/// How a guard's NFAs are compiled, forward or back (back without groups, which it never finds).
fn guard_nfa(reverse: bool) -> thompson::Config {
    let c = thompson::Config::new().nfa_size_limit(Some(MOST_COMPILED)).reverse(reverse);
    if reverse {
        c.which_captures(thompson::WhichCaptures::None)
    } else {
        c
    }
}

/// A pattern's search whose work is held to a time (FEEDBACK75.md, Q-1075; branch feedback75):
/// regex-automata's lazy DFA, forward to where a match ends and back to where it starts, stepped
/// here (`forward`, `backward`) as the crate steps it, with the prefilter the crate uses, and a look
/// at the time every 4,096 bytes and every 16 states it has to make or look up: past what the call
/// may compute (`Clock`), the error value. The crate's own rule for giving up (three clearings of
/// its cache with under 10 bytes read a state) let a text that made a state every 21 bytes run a
/// 4-branch `(?:x*[ab]){900}` pattern 6.5 s on 1 MB; a rule of the NFA's states a state refused the
/// UI kit's translation checker on 1 MB of text with many braces (found in reviews). A match's
/// groups are found within the match alone, by the bounded backtracker where it fits, as the
/// crate does, else the PikeVM, held to the same budget as the crate's own searches. What only a
/// match needs is made when one first does (the DFA back, the one-pass DFA), and a pattern every
/// match of which ends where the text ends (`x$`) is searched back from there, as the crate does:
/// made and stepped through first, the UI kit's checker, the 600-word list, and `(ch|sh|x)$` on
/// 1 MB with no match took up to 19% longer than 0.8.8 (found measuring).
struct Guard {
    forward: hybrid::dfa::DFA,
    /// The pattern as the crates take it, which the DFA back is compiled from.
    full: String,
    /// Whether each match ends where the text ends, and none must start where it starts (the
    /// crate's "reverse anchored" search).
    end_anchored: bool,
    reverse: std::cell::OnceCell<Option<hybrid::dfa::DFA>>,
    /// Where the pattern has a literal inside that every match holds, and no fast one in front:
    /// the crate's "reverse inner" search (`Guarding::find_inner`).
    inner: Option<Inner>,
    groups: thompson::pikevm::PikeVM,
    /// For a match short enough for it, as the crate does: quicker than the PikeVM there.
    track: Option<thompson::backtrack::BoundedBacktracker>,
    /// Where the pattern is one-pass, as the crate does: a match's groups in time linear in it,
    /// however long, where the PikeVM's bound refused `^([^:]*)(?::([^:]*))?$` on a 2 MB line
    /// that 0.8.8 read in 5 ms (found measuring).
    onepass: std::cell::OnceCell<Option<regex_automata::dfa::onepass::DFA>>,
}

impl Guard {
    /// The DFA back from where a match ends, made the first time one is needed; None where it
    /// cannot be (the call then gives up).
    fn reverse(&self) -> Option<&hybrid::dfa::DFA> {
        self.reverse
            .get_or_init(|| {
                let hir = regex_automata::util::syntax::parse(&self.full).ok()?;
                reverse_dfa(thompson::Compiler::new().configure(guard_nfa(true)).build_from_hir(&hir).ok()?)
            })
            .as_ref()
    }

    /// The one-pass DFA, made the first time a match's groups are looked for; only for groups,
    /// which a pattern without any never looks for.
    fn onepass(&self) -> Option<&regex_automata::dfa::onepass::DFA> {
        self.onepass
            .get_or_init(|| {
                let nfa = self.groups.get_nfa();
                if nfa.group_info().group_len(regex_automata::PatternID::ZERO) > 1 {
                    regex_automata::dfa::onepass::DFA::new_from_nfa(nfa.clone()).ok()
                } else {
                    None
                }
            })
            .as_ref()
    }
}

/// The DFA back of a pattern's NFA back (`Guard::reverse`).
fn reverse_dfa(rev: thompson::NFA) -> Option<hybrid::dfa::DFA> {
    let dfa = hybrid::dfa::Config::new().cache_capacity(GUARD_CACHE).skip_cache_capacity_check(true);
    hybrid::dfa::Builder::new().configure(dfa.prefilter(None).specialize_start_states(false).match_kind(MatchKind::All)).build_from_nfa(rev).ok()
}

/// What each lazy DFA of a guarded call may keep: twice the crate's, as states of large patterns
/// are large (at least what one state needs, whatever this says).
const GUARD_CACHE: usize = 2 << 20;

/// `p` with `flags` as a `Guard`, from what `exact_room` made where it is given, or None where it
/// cannot be made.
fn build_guard(p: &str, flags: &str, made: Option<Made>) -> Option<Guard> {
    // Parsed again (a small part of compiling).
    let full = full_pattern(p, flags).ok()?;
    let hir = regex_automata::util::syntax::parse(&full).ok()?;
    let (fwd, rev) = match made {
        Some(Made { nfa, rev }) => (nfa, rev),
        None => (thompson::Compiler::new().configure(guard_nfa(false)).build_from_hir(&hir).ok()?, None),
    };
    let props = hir.properties();
    let start_anchored = props.look_set_prefix().contains(regex_syntax::hir::Look::Start);
    let dfa = hybrid::dfa::Config::new().cache_capacity(GUARD_CACHE).skip_cache_capacity_check(true);
    // The crate's prefixes and its prefilter of them (regex-automata's `util::prefilter::prefixes`
    // and `Prefilter::from_hir_prefix`), none where every match starts at the start, as the crate
    // has none there.
    let prefixes = (!start_anchored).then(|| {
        let mut extractor = regex_syntax::hir::literal::Extractor::new();
        extractor.kind(regex_syntax::hir::literal::ExtractKind::Prefix);
        let mut seq = regex_syntax::hir::literal::Seq::empty();
        seq.union(&mut extractor.extract(&hir));
        seq.optimize_for_prefix_by_preference();
        seq
    });
    let prefilter = prefixes.as_ref().and_then(|seq| seq.literals()).and_then(|lits| Prefilter::new(MatchKind::LeftmostFirst, lits));
    let fast_prefix = prefilter.as_ref().is_some_and(Prefilter::is_fast);
    // A pattern the crate searches with its prefilter alone (exact prefixes, no group, no
    // assertion: `Pre::from_prefixes`) has no guard, being the crate's on any text (Q-1125, see
    // `crate_searches_literals`); the guard's own search by that prefilter was taken out with it.
    let forward = hybrid::dfa::Builder::new().configure(dfa.prefilter(prefilter)).build_from_nfa(fwd.clone()).ok()?;
    // As the crate chooses: back from the end where every match ends there, else from a literal
    // inside where there is no fast one in front, else forward.
    let end_anchored = props.look_set_suffix().contains(regex_syntax::hir::Look::End) && !start_anchored;
    let inner = if end_anchored || start_anchored || fast_prefix { None } else { inner_search(&hir) };
    let track = thompson::backtrack::BoundedBacktracker::new_from_nfa(fwd.clone()).ok();
    let groups = thompson::pikevm::PikeVM::new_from_nfa(fwd).ok()?;
    let reverse = std::cell::OnceCell::new();
    if let Some(rev) = rev {
        let _ = reverse.set(reverse_dfa(rev));
    }
    Some(Guard { forward, full, end_anchored, reverse, inner, groups, track, onepass: std::cell::OnceCell::new() })
}

/// The crate's "reverse inner" search for a pattern: the literal inside it, found by a prefilter,
/// and the part in front of it, compiled back, which the search reads back from the literal to
/// where the match starts (FEEDBACK75.md, Q-1075; branch feedback75). Without it, the guard read
/// every byte of a text the crate skipped to each `@` in: `[^@\s]+@[^@\s]+\.[^@\s]+` on 990 KB
/// 22% slower than 0.8.8 (found measuring).
struct Inner {
    pre: Prefilter,
    rev: hybrid::dfa::DFA,
}

/// `hir`'s `Inner`, where the crate would search it so: a literal inside a top-level concatenation
/// that a fast prefilter finds, and a part in front of it that no earlier match can come from
/// (`inner::has_no_earlier_match`).
fn inner_search(hir: &regex_syntax::hir::Hir) -> Option<Inner> {
    let found = inner::extract(hir)?;
    if !inner::has_no_earlier_match(&found.prefix, &found.literals) {
        return None;
    }
    let nfa = thompson::Compiler::new().configure(guard_nfa(true)).build_from_hir(&found.prefix).ok()?;
    let dfa = hybrid::dfa::Config::new().cache_capacity(GUARD_CACHE).skip_cache_capacity_check(true).prefilter(None).specialize_start_states(false).match_kind(MatchKind::All);
    let rev = hybrid::dfa::Builder::new().configure(dfa).build_from_nfa(nfa).ok()?;
    Some(Inner { pre: found.pre, rev })
}

/// regex-automata 0.4.18's own choice of an inner literal (`src/meta/reverse_inner.rs`) and its
/// proofs that the first match found from one is the match its engines report
/// (`src/meta/prefix.rs`), as they are there (MIT or Apache-2.0): private to the crate, and the
/// guard must choose as it does to answer as it does.
mod inner {
    use regex_automata::util::prefilter::Prefilter;
    use regex_automata::MatchKind;
    use regex_syntax::hir::{
        self,
        literal::{self, Literal},
        Class, Hir, HirKind,
    };

    pub(super) struct Found {
        pub(super) prefix: Hir,
        pub(super) pre: Prefilter,
        pub(super) literals: Vec<Literal>,
    }

    /// `InnerPrefilter::new` for one pattern.
    pub(super) fn extract(hir: &Hir) -> Option<Found> {
        let mut concat = top_concat(hir)?;
        for i in 1..concat.len() {
            let Some((pre, lits)) = prefilter_with_literals(&concat[i]) else { continue };
            if !pre.is_fast() {
                continue;
            }
            let concat_suffix = Hir::concat(concat.split_off(i));
            let concat_prefix = Hir::concat(concat);
            let (pre, literals) = match prefilter_with_literals(&concat_suffix) {
                Some((pre2, lits2)) if pre2.is_fast() => (pre2, lits2),
                _ => (pre, lits),
            };
            return Some(Found { prefix: concat_prefix, pre, literals });
        }
        None
    }

    fn prefilter_with_literals(hir: &Hir) -> Option<(Prefilter, Vec<Literal>)> {
        let mut extractor = literal::Extractor::new();
        extractor.kind(literal::ExtractKind::Prefix);
        let mut prefixes = extractor.extract(hir);
        prefixes.make_inexact();
        prefixes.optimize_for_prefix_by_preference();
        let lits = prefixes.literals()?;
        let pre = Prefilter::new(MatchKind::LeftmostFirst, lits)?;
        Some((pre, lits.to_vec()))
    }

    fn top_concat(mut hir: &Hir) -> Option<Vec<Hir>> {
        loop {
            hir = match hir.kind() {
                HirKind::Empty | HirKind::Literal(_) | HirKind::Class(_) | HirKind::Look(_) | HirKind::Repetition(_) | HirKind::Alternation(_) => return None,
                HirKind::Capture(hir::Capture { ref sub, .. }) => sub,
                HirKind::Concat(ref subs) => {
                    let concat = Hir::concat(subs.iter().map(flatten).collect());
                    return match concat.into_kind() {
                        HirKind::Concat(xs) => Some(xs),
                        _ => None,
                    };
                }
            };
        }
    }

    fn flatten(hir: &Hir) -> Hir {
        match hir.kind() {
            HirKind::Empty => Hir::empty(),
            HirKind::Literal(hir::Literal(ref x)) => Hir::literal(x.clone()),
            HirKind::Class(ref x) => Hir::class(x.clone()),
            HirKind::Look(ref x) => Hir::look(*x),
            HirKind::Repetition(ref x) => Hir::repetition(x.with(flatten(&x.sub))),
            HirKind::Capture(hir::Capture { ref sub, .. }) => flatten(sub),
            HirKind::Alternation(ref xs) => Hir::alternation(xs.iter().map(flatten).collect()),
            HirKind::Concat(ref xs) => Hir::concat(xs.iter().map(flatten).collect()),
        }
    }

    /// Whether the first literal confirmed is the match the crate's engines report.
    pub(super) fn has_no_earlier_match(concat_prefix: &Hir, literals: &[Literal]) -> bool {
        if literals.is_empty() || literals.iter().any(|lit| lit.is_empty()) {
            return false;
        }
        if literals.len() == 1 && !hir_can_contain_literal(concat_prefix, literals[0].as_bytes()) {
            return true;
        }
        hir_has_fixed_length(concat_prefix) || has_disjoint_class_separator(concat_prefix, literals)
    }

    fn hir_can_contain_literal(hir: &Hir, lit: &[u8]) -> bool {
        lit.is_empty() || lit.iter().all(|&byte| hir_can_consume_byte(hir, byte))
    }

    fn hir_has_fixed_length(hir: &Hir) -> bool {
        let props = hir.properties();
        props.minimum_len().and_then(|min| props.maximum_len().map(|max| min == max)).unwrap_or(false)
    }

    fn has_disjoint_class_separator(prefix: &Hir, literals: &[Literal]) -> bool {
        let hirs = match uncapture(prefix).kind() {
            HirKind::Concat(hirs) if hirs.len() >= 2 => hirs,
            _ => return false,
        };
        let Some(first) = hirs.iter().position(|hir| !hir_matches_empty_only(hir)) else { return false };
        let last = hirs.iter().rposition(|hir| !hir_matches_empty_only(hir)).expect("one is");
        has_disjoint_class_separator_at(hirs, last, literals) || (first != last && has_disjoint_class_separator_at(hirs, first, literals))
    }

    fn has_disjoint_class_separator_at(hirs: &[Hir], separator: usize, literals: &[Literal]) -> bool {
        let Some(separator_class) = required_class(&hirs[separator]) else { return false };
        class_is_disjoint_from_literals(separator_class, literals) && hirs.iter().enumerate().all(|(i, hir)| i == separator || hir_is_disjoint_from_class(hir, separator_class))
    }

    fn hir_matches_empty_only(hir: &Hir) -> bool {
        hir.properties().maximum_len() == Some(0)
    }

    fn required_class(hir: &Hir) -> Option<&Class> {
        match uncapture(hir).kind() {
            HirKind::Class(cls) => Some(cls),
            HirKind::Repetition(rep) if rep.min > 0 => match uncapture(&rep.sub).kind() {
                HirKind::Class(cls) => Some(cls),
                _ => None,
            },
            _ => None,
        }
    }

    fn hir_is_disjoint_from_class(hir: &Hir, separator: &Class) -> bool {
        match hir.kind() {
            HirKind::Empty | HirKind::Look(_) => true,
            HirKind::Literal(lit) => class_is_disjoint_from_literal(separator, &lit.0),
            HirKind::Class(cls) => classes_are_disjoint(cls, separator),
            HirKind::Repetition(rep) => hir_is_disjoint_from_class(&rep.sub, separator),
            HirKind::Capture(capture) => hir_is_disjoint_from_class(&capture.sub, separator),
            HirKind::Concat(hirs) | HirKind::Alternation(hirs) => hirs.iter().all(|hir| hir_is_disjoint_from_class(hir, separator)),
        }
    }

    fn uncapture(mut hir: &Hir) -> &Hir {
        while let HirKind::Capture(capture) = hir.kind() {
            hir = &capture.sub;
        }
        hir
    }

    fn classes_are_disjoint(left: &Class, right: &Class) -> bool {
        match (left, right) {
            (Class::Bytes(left), Class::Bytes(right)) => {
                let (mut intersection, other) = if left.ranges().len() <= right.ranges().len() { (left.clone(), right) } else { (right.clone(), left) };
                intersection.intersect(other);
                intersection.ranges().is_empty()
            }
            (Class::Unicode(left), Class::Unicode(right)) => {
                let (mut intersection, other) = if left.ranges().len() <= right.ranges().len() { (left.clone(), right) } else { (right.clone(), left) };
                intersection.intersect(other);
                intersection.ranges().is_empty()
            }
            _ => false,
        }
    }

    fn class_is_disjoint_from_literals(cls: &Class, literals: &[Literal]) -> bool {
        literals.iter().all(|lit| class_is_disjoint_from_literal(cls, lit.as_bytes()))
    }

    fn class_is_disjoint_from_literal(cls: &Class, lit: &[u8]) -> bool {
        match cls {
            Class::Bytes(cls) => lit.iter().all(|&byte| !byte_class_contains(cls, byte)),
            Class::Unicode(cls) => core::str::from_utf8(lit).is_ok_and(|lit| lit.chars().all(|ch| !unicode_class_contains(cls, ch))),
        }
    }

    fn hir_can_consume_byte(hir: &Hir, byte: u8) -> bool {
        match hir.kind() {
            HirKind::Empty | HirKind::Look(_) => false,
            HirKind::Literal(lit) => lit.0.contains(&byte),
            HirKind::Class(Class::Bytes(cls)) => byte_class_contains(cls, byte),
            HirKind::Class(Class::Unicode(cls)) => byte > 0x7F || unicode_class_contains(cls, char::from(byte)),
            HirKind::Repetition(rep) => hir_can_consume_byte(&rep.sub, byte),
            HirKind::Capture(capture) => hir_can_consume_byte(&capture.sub, byte),
            HirKind::Concat(hirs) | HirKind::Alternation(hirs) => hirs.iter().any(|hir| hir_can_consume_byte(hir, byte)),
        }
    }

    fn byte_class_contains(cls: &hir::ClassBytes, byte: u8) -> bool {
        cls.ranges().iter().any(|range| range.start() <= byte && byte <= range.end())
    }

    fn unicode_class_contains(cls: &hir::ClassUnicode, ch: char) -> bool {
        cls.ranges().iter().any(|range| range.start() <= ch && ch <= range.end())
    }
}

/// A thread's guards, kept as its compiled patterns are (each counted as its pattern is), at most
/// `GUARDS_MOST`; None for a pattern none could be made of.
#[derive(Default)]
struct Guards {
    map: HashMap<(String, String), (Option<Rc<Guard>>, usize)>,
    order: std::collections::VecDeque<(String, String)>,
    bytes: usize,
}

const GUARDS_MOST: usize = 64;

thread_local! {
    static GUARDS: RefCell<Guards> = RefCell::new(Guards::default());
}

/// The pattern's `Guard`, made the first time a search needs it; None where it cannot be made,
/// and the search is then made by the crate, as before.
fn guard_of(key: (String, String), made: Option<Made>) -> Option<Rc<Guard>> {
    if let Some(g) = GUARDS.with(|c| c.borrow().map.get(&key).map(|(g, _)| g.clone())) {
        return g;
    }
    let g = build_guard(&key.0, &key.1, made).map(Rc::new);
    // Counted as its pattern is in the cache, as most as anything kept there where it is not.
    let counted = CACHE.with(|c| c.borrow().map.get(&key).map_or(CACHE_BYTES, |(_, n, _)| *n));
    GUARDS.with(|c| {
        let mut c = c.borrow_mut();
        while !c.order.is_empty() && (c.bytes + counted > CACHE_BYTES || c.map.len() >= GUARDS_MOST) {
            let old = c.order.pop_front().expect("not empty");
            if let Some((_, n)) = c.map.remove(&old) {
                c.bytes -= n;
            }
        }
        if counted <= CACHE_BYTES {
            c.bytes += counted;
            c.order.push_back(key.clone());
            c.map.insert(key, (g.clone(), counted));
        }
    });
    g
}

/// How a call past its pattern's room searches (`guard_for`): by the pattern's guard, with what a
/// byte of a match's groups costs, or, for a pattern the crate searches with literals alone
/// (`Room::literal`), by the crate in windows of the text, the longest literal's bytes being given
/// (`windowed_matches`).
enum Way {
    Guard(Rc<Guard>, u64),
    Windows(usize),
}

/// The guard for a call of `pat` over `len` bytes (`caps`: one that asks for groups), where the
/// crate's worst would not fit (`Room`): Ok(None) for the crate, Err where none can be made, which is
/// the call's error value, never the crate.
#[inline]
fn guard_for(pat: &mut Pat, key: Key, len: usize, caps: bool) -> Result<Option<Way>, ()> {
    if pat.room.holds(len, caps) {
        return Ok(None);
    }
    guard_past_room(pat, key, len, caps)
}

#[inline(never)]
fn guard_past_room(pat: &mut Pat, key: Key, len: usize, caps: bool) -> Result<Option<Way>, ()> {
    let key = key.strings();
    let (room, made) = exact_room(pat, &key);
    if room.fits(len, caps) {
        if room.compiled() {
            return Ok(None);
        }
        // A pattern the guard took first, on a text the crate may search: its Regex compiled now,
        // once, and kept with it.
        if let Ok((r, step)) = compile_stepped(&key.0, &key.1) {
            let room = room.with_crate(true);
            store(&key, &r, step * 2, room);
            pat.r = r;
            pat.room = room;
            return Ok(None);
        }
    }
    // Past a literal pattern's room, its own search in windows, with the guard's clock: unclocked, a
    // list with one literal of 20,001 bytes took 8 s in `re.replace` on 2 MB, and 64 literals of 100
    // bytes 4.5 s on 10 MB, where the guard gives 0.4 and 1.6 s (FEEDBACK75.md follow-up found by
    // 0.8.9's coverage, Q-1125, review; branch feedback75).
    if room.longest > 0 {
        return Ok(Some(Way::Windows(room.longest)));
    }
    guard_of(key, made).map(|g| Some(Way::Guard(g, room.caps_ps))).ok_or(())
}

/// A guarded call's caches, made once for all its searches.
struct Guarding<'g> {
    g: &'g Guard,
    fwd: hybrid::dfa::Cache,
    /// Made with the DFA back, where a match first needs it.
    rev: Option<hybrid::dfa::Cache>,
    /// The cache of `Inner`'s DFA back, where the search first needs it.
    inner: Option<hybrid::dfa::Cache>,
    /// Made where the PikeVM runs, its size charged first (see `room_for_bytes`): it holds each
    /// state's slots twice, and `(?:(a)|…×3000)*` made 785 MB for a `re.test` that never used it
    /// (found in review).
    groups: Option<thompson::pikevm::Cache>,
    track: Option<thompson::backtrack::Cache>,
    onepass: Option<regex_automata::dfa::onepass::Cache>,
    caps: regex_automata::util::captures::Captures,
    /// The pattern's groups, the whole match among them.
    count: usize,
    /// What a byte of a match's groups costs, at the most (`Room`).
    caps_ps: u64,
    clock: Clock,
}

/// Why a guarded search stopped short.
#[derive(Clone, Copy)]
enum Halt {
    /// Past what the call may compute: the call's error value.
    Time,
    /// Past `--max-compute`: the request's stop, as Q-976's searches stop it (the watchdog's flag
    /// is taken as it is read, so an error value would let the request go on).
    Compute,
}

/// The most steps that may make a state between two looks at the clock, and the NFA states they may
/// follow in all between them (see `Clock::weigh`): a debug build's (`SLOWER`) fewer.
const STEPS_MOST: u32 = 4096;
const STATE_WORK: usize = 40_000 / SLOWER as usize;

/// What a guarded call may compute, in all: a tenth of a second and `GUARD_PER_BYTE` a byte of its
/// text, of its thread's own time (see there), and never past what the request may still compute
/// under `--max-compute`; looked at by the wall clock, which is cheap, and then by the thread's,
/// which a loaded machine does not run ahead.
struct Clock {
    cpu_from: Duration,
    allowed: Duration,
    until: std::time::Instant,
    /// When `--max-compute` ends the request, by its own (wall) clock.
    compute_until: Option<std::time::Instant>,
    bytes: u32,
    steps: u32,
    /// How many steps that may make a state go between looks (`weigh`).
    every: u32,
}

impl Clock {
    fn new(len: usize) -> Clock {
        let now = std::time::Instant::now();
        let allowed = Duration::from_millis(100) * SLOWER + GUARD_PER_BYTE.saturating_mul(u32::try_from(len).unwrap_or(u32::MAX));
        Clock { cpu_from: thread_time(), allowed, until: now + allowed, compute_until: crate::httpd::compute_left().map(|d| now + d), bytes: 0, steps: 0, every: STEPS_MOST }
    }

    /// The steps between looks for a DFA of an NFA of `states` states, the fewest of those it steps:
    /// making a state follows at most each state of the NFA, so that `STATE_WORK` over the states
    /// is about 2 ms of making at 50 ns a state, and never more than now for a large one (16). Each
    /// match state is a step that may make a state, and a look every 16 of them was a fifth of a
    /// tokenizer's search (`Instant::now`; found profiling).
    fn weigh(&mut self, states: usize) {
        let every = (STATE_WORK / states.max(1)).clamp(16, STEPS_MOST as usize);
        self.every = self.every.min(every as u32);
    }

    /// Whether `cost` more fits before `--max-compute` ends the request.
    fn compute_fits(&self, cost: Duration) -> bool {
        self.compute_until.is_none_or(|c| std::time::Instant::now() + cost < c)
    }

    /// Whether `cost` more fits what the call may still compute and what `--max-compute` leaves.
    fn room_for(&self, cost: Duration) -> Result<(), Halt> {
        if !self.compute_fits(cost) {
            return Err(Halt::Compute);
        }
        if thread_time().saturating_sub(self.cpu_from) + cost > self.allowed {
            return Err(Halt::Time);
        }
        Ok(())
    }

    /// A byte stepped over.
    #[inline]
    fn byte(&mut self) -> Result<(), Halt> {
        self.bytes(1)
    }

    /// `n` bytes stepped over.
    #[inline]
    fn bytes(&mut self, n: usize) -> Result<(), Halt> {
        self.bytes = self.bytes.saturating_add(u32::try_from(n).unwrap_or(u32::MAX));
        if self.bytes < 4096 {
            return Ok(());
        }
        self.bytes = 0;
        self.look()
    }

    /// A step that may have made a state.
    #[inline]
    fn slow(&mut self) -> Result<(), Halt> {
        self.steps += 1;
        if self.steps < self.every {
            return Ok(());
        }
        self.steps = 0;
        self.look()
    }

    #[inline(never)]
    fn look(&mut self) -> Result<(), Halt> {
        let now = std::time::Instant::now();
        if self.compute_until.is_some_and(|c| now >= c) || crate::httpd::OVER_TIME.load(std::sync::atomic::Ordering::Relaxed) && crate::httpd::over_time() {
            return Err(Halt::Compute);
        }
        if now < self.until {
            return Ok(());
        }
        let used = thread_time().saturating_sub(self.cpu_from);
        if used > self.allowed {
            return Err(Halt::Time);
        }
        self.until = now + (self.allowed - used);
        Ok(())
    }
}

/// The next state from `sid` on `b`: from the table where it is known, else made (and looked at
/// the clock for).
#[inline(always)]
fn step(dfa: &hybrid::dfa::DFA, cache: &mut hybrid::dfa::Cache, clock: &mut Clock, sid: hybrid::LazyStateID, b: u8) -> Result<hybrid::LazyStateID, Halt> {
    if !sid.is_tagged() {
        let next = dfa.next_state_untagged(cache, sid, b);
        if !next.is_unknown() {
            return Ok(next);
        }
    }
    clock.slow()?;
    dfa.next_state(cache, sid, b).map_err(|_| Halt::Time)
}

/// Where the leftmost-first match from `input`'s start ends, stepped as regex-automata's
/// `find_fwd` steps it (its start, match, dead and quit states, its prefilter from a start state,
/// the transition past the end), with the clock looked at.
fn forward(dfa: &hybrid::dfa::DFA, cache: &mut hybrid::dfa::Cache, clock: &mut Clock, input: &Input<'_>) -> Result<Option<usize>, Halt> {
    if input.is_done() {
        return Ok(None);
    }
    let hay = input.haystack();
    let pre = if input.get_anchored().is_anchored() { None } else { dfa.get_config().get_prefilter() };
    let universal = dfa.get_nfa().look_set_prefix_any().is_empty();
    let restart = |cache: &mut hybrid::dfa::Cache, at: usize| {
        let mut i = input.clone();
        i.set_start(at);
        dfa.start_state_forward(cache, &i).map_err(|_| Halt::Time)
    };
    let mut end = None;
    let mut sid = dfa.start_state_forward(cache, input).map_err(|_| Halt::Time)?;
    let mut at = input.start();
    if let Some(pre) = pre {
        match pre.find(hay, regex_automata::Span::from(at..input.end())) {
            None => return Ok(None),
            Some(sp) => {
                at = sp.start;
                if !universal {
                    sid = restart(cache, at)?;
                }
            }
        }
    }
    while at < input.end() {
        // Through untagged states from the table, as the crate's loop goes, and the clock looked at
        // once for each 4,096 bytes (found in review: a step and a count a byte made ordinary
        // patterns slower than the crate).
        if !sid.is_tagged() {
            let stop = input.end().min(at + 4096);
            let from = at;
            // SAFETY (each step): `sid` is a state of this DFA, not tagged (looked at before it is
            // stepped from), and the byte's index is below `stop`, at most the haystack's end.
            while at + 4 <= stop {
                let a = unsafe { dfa.next_state_untagged_unchecked(cache, sid, *hay.get_unchecked(at)) };
                if a.is_tagged() {
                    break;
                }
                let b = unsafe { dfa.next_state_untagged_unchecked(cache, a, *hay.get_unchecked(at + 1)) };
                if b.is_tagged() {
                    sid = a;
                    at += 1;
                    break;
                }
                let c = unsafe { dfa.next_state_untagged_unchecked(cache, b, *hay.get_unchecked(at + 2)) };
                if c.is_tagged() {
                    sid = b;
                    at += 2;
                    break;
                }
                let d = unsafe { dfa.next_state_untagged_unchecked(cache, c, *hay.get_unchecked(at + 3)) };
                if d.is_tagged() {
                    sid = c;
                    at += 3;
                    break;
                }
                sid = d;
                at += 4;
            }
            while at < stop && !sid.is_tagged() {
                let next = unsafe { dfa.next_state_untagged_unchecked(cache, sid, *hay.get_unchecked(at)) };
                if next.is_tagged() {
                    break;
                }
                sid = next;
                at += 1;
            }
            clock.bytes(at - from)?;
            if at == stop {
                continue;
            }
        }
        sid = step(dfa, cache, clock, sid, hay[at])?;
        if sid.is_tagged() {
            if sid.is_start() {
                if let (Some(pre), None) = (pre, end) {
                    match pre.find(hay, regex_automata::Span::from(at..input.end())) {
                        None => return Ok(None),
                        Some(sp) if sp.start > at => {
                            at = sp.start;
                            if !universal {
                                sid = restart(cache, at)?;
                            }
                            continue;
                        }
                        Some(_) => {}
                    }
                }
            } else if sid.is_match() {
                end = Some(at);
                if input.get_earliest() {
                    return Ok(end);
                }
            } else if sid.is_dead() {
                return Ok(end);
            } else if sid.is_quit() {
                return Err(Halt::Time);
            }
        }
        // Counted by `step` where it may make a state or leaves a tagged one, and the steps of the
        // loop above by their run: each step here from an untagged state is one the loop above
        // stopped before, followed by a counted one (a tokenizer's match states made two counts
        // of each byte, found measuring).
        at += 1;
    }
    sid = match hay.get(input.end()) {
        Some(&b) => dfa.next_state(cache, sid, b),
        None => dfa.next_eoi_state(cache, sid),
    }
    .map_err(|_| Halt::Time)?;
    if sid.is_match() {
        end = Some(input.end());
    }
    Ok(end)
}

/// Where the match that ends at `input`'s end begins, back from there, anchored, as regex-automata's
/// `find_rev` steps it, with the clock looked at.
fn backward(dfa: &hybrid::dfa::DFA, cache: &mut hybrid::dfa::Cache, clock: &mut Clock, input: &Input<'_>) -> Result<Option<usize>, Halt> {
    let hay = input.haystack();
    let mut start = None;
    let mut sid = dfa.start_state_reverse(cache, input).map_err(|_| Halt::Time)?;
    if input.start() < input.end() {
        let mut at = input.end() - 1;
        loop {
            // Through untagged states from the table, as `forward` does.
            if !sid.is_tagged() {
                let from = at;
                loop {
                    // SAFETY: `sid` is a state of this DFA and not tagged, and `at` is within the
                    // haystack (at most `input.end() - 1`, at least its start).
                    let next = unsafe { dfa.next_state_untagged_unchecked(cache, sid, *hay.get_unchecked(at)) };
                    if next.is_tagged() || at == input.start() {
                        break;
                    }
                    sid = next;
                    at -= 1;
                }
                clock.bytes(from - at)?;
            }
            sid = step(dfa, cache, clock, sid, hay[at])?;
            if sid.is_tagged() {
                if sid.is_match() {
                    start = Some(at + 1);
                } else if sid.is_dead() {
                    return Ok(start);
                } else if sid.is_quit() {
                    return Err(Halt::Time);
                }
            }
            if at == input.start() {
                break;
            }
            // Counted as in `forward`: a step here leaves a tagged state, which `step` counts, or
            // is the one the loop above stopped before (a match state at each byte, as reading back
            // a run of `[^;\n{}]+` has, made two counts of each).
            at -= 1;
        }
    }
    sid = match input.start().checked_sub(1) {
        Some(i) => dfa.next_state(cache, sid, hay[i]),
        None => dfa.next_eoi_state(cache, sid),
    }
    .map_err(|_| Halt::Time)?;
    if sid.is_match() {
        start = Some(input.start());
    }
    Ok(start)
}

/// Where the match from `input`'s start (anchored) ends, or Err where the reading stopped with
/// none, as regex-automata's `stopat::hybrid_try_search_half_fwd` reads it, with the clock looked
/// at.
fn forward_stopat(dfa: &hybrid::dfa::DFA, cache: &mut hybrid::dfa::Cache, clock: &mut Clock, input: &Input<'_>) -> Result<Result<usize, usize>, Halt> {
    let hay = input.haystack();
    let mut end = None;
    let mut sid = dfa.start_state_forward(cache, input).map_err(|_| Halt::Time)?;
    let mut at = input.start();
    while at < input.end() {
        sid = step(dfa, cache, clock, sid, hay[at])?;
        if sid.is_tagged() {
            if sid.is_match() {
                end = Some(at);
                if input.get_earliest() {
                    return Ok(end.ok_or(at));
                }
            } else if sid.is_dead() {
                return Ok(end.ok_or(at));
            } else if sid.is_quit() {
                return Err(Halt::Time);
            }
        }
        at += 1;
        clock.byte()?;
    }
    sid = match hay.get(input.end()) {
        Some(&b) => dfa.next_state(cache, sid, b),
        None => dfa.next_eoi_state(cache, sid),
    }
    .map_err(|_| Halt::Time)?;
    if sid.is_quit() {
        return Err(Halt::Time);
    }
    if sid.is_match() {
        end = Some(input.end());
    }
    Ok(end.ok_or(at))
}

/// Where the match that ends at `input`'s end begins, back from there, anchored, as regex-automata's
/// `limited::hybrid_try_search_half_rev` reads it, with the clock looked at: None where it would
/// read before `min_start`, or reached the start without its DFA dying (which could be a match
/// that starts earlier), the crate's signs that the search should go forward instead.
fn back_limited(dfa: &hybrid::dfa::DFA, cache: &mut hybrid::dfa::Cache, clock: &mut Clock, input: &Input<'_>, min_start: usize) -> Result<Option<Option<usize>>, Halt> {
    let hay = input.haystack();
    let mut start = None;
    let mut sid = dfa.start_state_reverse(cache, input).map_err(|_| Halt::Time)?;
    let eoi = |sid: &mut hybrid::LazyStateID, start: &mut Option<usize>, cache: &mut hybrid::dfa::Cache| -> Result<(), Halt> {
        *sid = match input.start().checked_sub(1) {
            Some(i) => dfa.next_state(cache, *sid, hay[i]),
            None => dfa.next_eoi_state(cache, *sid),
        }
        .map_err(|_| Halt::Time)?;
        if sid.is_quit() {
            return Err(Halt::Time);
        }
        if sid.is_match() {
            *start = Some(input.start());
        }
        Ok(())
    };
    if input.start() == input.end() {
        eoi(&mut sid, &mut start, cache)?;
        return Ok(Some(start));
    }
    let mut at = input.end() - 1;
    loop {
        sid = step(dfa, cache, clock, sid, hay[at])?;
        clock.byte()?;
        if sid.is_tagged() {
            if sid.is_match() {
                start = Some(at + 1);
            } else if sid.is_dead() {
                return Ok(Some(start));
            } else if sid.is_quit() {
                return Err(Halt::Time);
            }
        }
        if at == input.start() {
            break;
        }
        at -= 1;
        if at < min_start {
            return Ok(None);
        }
    }
    let was_dead = sid.is_dead();
    eoi(&mut sid, &mut start, cache)?;
    if at == input.start() && start.is_some_and(|s| s > input.start()) && !was_dead {
        return Ok(None);
    }
    Ok(Some(start))
}

impl<'g> Guarding<'g> {
    fn new(g: &'g Guard, caps_ps: u64, len: usize) -> Guarding<'g> {
        let count = g.groups.get_nfa().group_info().group_len(regex_automata::PatternID::ZERO);
        Guarding {
            g,
            fwd: g.forward.create_cache(),
            rev: None,
            inner: None,
            groups: None,
            track: None,
            onepass: None,
            caps: g.groups.create_captures(),
            count,
            caps_ps,
            clock: {
                let mut clock = Clock::new(len);
                clock.weigh(g.forward.get_nfa().states().len());
                clock
            },
        }
    }

    /// The first match from `from` on (`earliest`: whether there is one, ended where it is first
    /// known, its groups not looked for), or Err past the call's time, or where the match's groups
    /// could take longer to find than `GUARD_BUDGET_PS`.
    #[inline]
    fn find(&mut self, s: &str, from: usize, earliest: bool) -> Result<Option<Found>, Halt> {
        if self.g.end_anchored {
            return self.find_at_end(s, from, earliest);
        }
        if self.g.inner.is_some() {
            return self.find_inner(s, from, earliest);
        }
        self.find_forward(s, from, earliest)
    }

    /// `find` from the pattern's inner literal (`Inner`), as regex-automata's "reverse inner"
    /// search goes (`ReverseInner::try_search_full`): to each literal by the prefilter, back from
    /// it to where a match starts (no further back than the last literal, `min_match_start`), and
    /// forward from there to where it ends; where a literal is before where the last forward
    /// reading stopped, or the reading back would pass the last literal, the search goes forward
    /// from the start instead, as the crate's does, so that no byte is read over and over. Each
    /// step is a step of the guard's, counted by its clock.
    #[inline(never)]
    fn find_inner(&mut self, s: &str, from: usize, earliest: bool) -> Result<Option<Found>, Halt> {
        let inner = self.g.inner.as_ref().expect("looked at by `find`");
        let mut span = regex_automata::Span::from(from..s.len());
        let (mut min_match_start, mut min_pre_start) = (0usize, 0usize);
        loop {
            let Some(lit) = inner.pre.find(s.as_bytes(), span) else { break };
            self.clock.bytes(lit.start - span.start)?;
            if lit.start < min_pre_start {
                return self.find_forward(s, from, earliest);
            }
            let back = Input::new(s).anchored(Anchored::Yes).span(from..lit.start);
            if self.inner.is_none() {
                self.clock.weigh(inner.rev.get_nfa().states().len());
                self.inner = Some(inner.rev.create_cache());
            }
            let cache = self.inner.as_mut().expect("made above");
            match back_limited(&inner.rev, cache, &mut self.clock, &back, min_match_start)? {
                None => return self.find_forward(s, from, earliest),
                Some(Some(start)) => {
                    let ahead = Input::new(s).anchored(Anchored::Yes).span(start..s.len()).earliest(earliest);
                    match forward_stopat(&self.g.forward, &mut self.fwd, &mut self.clock, &ahead)? {
                        Err(stopat) => {
                            min_pre_start = stopat;
                            span.start = lit.start + 1;
                        }
                        Ok(end) if earliest => return Ok(Some(Found { start: end, end, groups: Vec::new() })),
                        Ok(end) => return self.groups_of(s, start, end),
                    }
                }
                Some(None) => {}
            }
            if span.start >= span.end {
                break;
            }
            span.start = lit.start + 1;
            min_match_start = lit.end;
        }
        Ok(None)
    }

    /// `find`, forward to where the match ends, and back from there to where it starts.
    #[inline(always)]
    fn find_forward(&mut self, s: &str, from: usize, earliest: bool) -> Result<Option<Found>, Halt> {
        let fdfa = &self.g.forward;
        let mut input = Input::new(s).range(from..).earliest(earliest);
        let mut end = forward(fdfa, &mut self.fwd, &mut self.clock, &input)?;
        // An empty match inside a character, which `\B` can find between the bytes of `é`: passed
        // over a byte at a time, as the crate does (UTF-8 mode).
        if fdfa.get_nfa().has_empty() && fdfa.get_nfa().is_utf8() {
            while let Some(e) = end {
                if s.is_char_boundary(e) {
                    break;
                }
                input.set_start(input.start() + 1);
                end = forward(fdfa, &mut self.fwd, &mut self.clock, &input)?;
            }
        }
        let Some(end) = end else { return Ok(None) };
        if earliest {
            return Ok(Some(Found { start: end, end, groups: Vec::new() }));
        }
        let start = if end == from || fdfa.get_nfa().is_always_start_anchored() {
            from
        } else {
            let back = Input::new(s).span(from..end).anchored(Anchored::Yes).earliest(false);
            self.back(&back)?.ok_or(Halt::Time)?
        };
        self.groups_of(s, start, end)
    }

    /// Where the match that ends at `input`'s end begins, by the DFA back (made where it is first
    /// needed).
    #[inline(always)]
    fn back(&mut self, input: &Input<'_>) -> Result<Option<usize>, Halt> {
        let rdfa = self.g.reverse().ok_or(Halt::Time)?;
        if self.rev.is_none() {
            self.clock.weigh(rdfa.get_nfa().states().len());
            self.rev = Some(rdfa.create_cache());
        }
        let cache = self.rev.as_mut().expect("made above");
        backward(rdfa, cache, &mut self.clock, input)
    }

    /// `find` for a pattern whose every match ends where the text does (`Guard::end_anchored`):
    /// back from there, anchored, for the match that starts first (the crate's "reverse anchored"
    /// search, and its answer), where forward would read the whole text.
    #[inline(never)]
    fn find_at_end(&mut self, s: &str, from: usize, earliest: bool) -> Result<Option<Found>, Halt> {
        if from > s.len() {
            return Ok(None);
        }
        let back = Input::new(s).span(from..s.len()).anchored(Anchored::Yes).earliest(false);
        let Some(start) = self.back(&back)? else { return Ok(None) };
        // Never inside a character, which the DFA's UTF-8 classes cannot end a match in; were one
        // found there, the search goes forward instead, as any other (never taken in tests).
        if !s.is_char_boundary(start) {
            return self.find_forward(s, from, earliest);
        }
        if earliest {
            return Ok(Some(Found { start: s.len(), end: s.len(), groups: Vec::new() }));
        }
        self.groups_of(s, start, s.len())
    }

    /// The match from `start` to `end`, with its groups where the pattern has any.
    #[inline(always)]
    fn groups_of(&mut self, s: &str, start: usize, end: usize) -> Result<Option<Found>, Halt> {
        // No groups to find: the PikeVM went over every match, and `(?:a*){900}` on 1 MB of `a`
        // took 7.7 s where the crate took 2 ms (found in review).
        if self.count <= 1 {
            return Ok(Some(Found { start, end, groups: Vec::new() }));
        }
        // Finding them follows each state, its ranges and its slots, a byte of the match at the most
        // (`Room`): past the budget, given up as a search is (`((?:a*){900})` took 1.3 s on 200 KB;
        // found in review).
        let within = Input::new(s).span(start..end).anchored(Anchored::Yes);
        // One-pass: linear in the match, whatever its length.
        if let Some(o) = self.g.onepass() {
            let cache = self.onepass.get_or_insert_with(|| o.create_cache());
            if o.try_search(cache, &within, &mut self.caps).is_ok() {
                let groups = (1..self.count).map(|i| self.caps.get_group(i).map(|g| (g.start, g.end))).collect();
                return Ok(Some(Found { start, end, groups }));
            }
        }
        // Within the match, from its start: what lies around it is still looked at by `\b` and `$`.
        // The bounded backtracker where the match fits its own small set of visits, as the crate
        // does: each state at each byte once, a few milliseconds at most there.
        let tracked = match &self.g.track {
            Some(t) if end - start <= t.max_haystack_len() => {
                let cache = self.track.get_or_insert_with(|| t.create_cache());
                t.try_search(cache, &within, &mut self.caps).is_ok()
            }
            _ => false,
        };
        if !tracked {
            let cost = (self.caps_ps as u128).saturating_mul((end - start) as u128);
            let nfa = self.g.groups.get_nfa();
            let pikevm_bytes = (nfa.states().len() as u128).saturating_mul(nfa.group_info().slot_len() as u128).saturating_mul(16);
            if cost <= GUARD_BUDGET_PS && (self.groups.is_some() || room_for_bytes(pikevm_bytes)) {
                // Nor past `--max-compute`: it cannot be stopped once it starts.
                if !self.clock.compute_fits(Duration::from_nanos(u64::try_from(cost / 1000).unwrap_or(u64::MAX))) {
                    return Err(Halt::Compute);
                }
                let cache = self.groups.get_or_insert_with(|| self.g.groups.create_cache());
                self.g.groups.search(cache, &within, &mut self.caps);
            } else {
                self.track_long(&within, end - start)?;
            }
        }
        let groups = (1..self.count).map(|i| self.caps.get_group(i).map(|g| (g.start, g.end))).collect();
        Ok(Some(Found { start, end, groups }))
    }

    /// A long match's groups where the PikeVM's bound does not fit: regex-automata's bounded
    /// backtracker (`nfa::thompson::backtrack`), as it is there, run here so that its stack can be
    /// charged as it grows. It visits each state at each byte once, and keeps a group's slots in the
    /// frame it saves them in rather than copying them all for each thread as the PikeVM does, so
    /// its time is the states times the bytes, with no factor of slots (`TRACK_PS`), held to what the
    /// call may still compute and to `--max-compute`, and the clock looked at as it goes. Its set of
    /// visits (a bit for each state at each byte) is asked for first; its stack, which nothing else
    /// bounds, is asked for as it grows, each time with the visits and the buffer it grows from
    /// (`room_for_bytes`), and past that room the call gives up: charged at its worst up front (a
    /// frame of 16 bytes for each visit), `^(\S+)\s*(.*)$` was refused from 340 KB, where 0.8.8
    /// read a 1 MB line in 20 ms and its stack takes about 50 bytes a byte; uncharged, a thousand
    /// groups on 20 KB took 974 MB (found in reviews; FEEDBACK75.md, Q-1075; branch feedback75).
    #[inline(never)]
    fn track_long(&mut self, within: &Input<'_>, span: usize) -> Result<(), Halt> {
        let nfa = self.g.groups.get_nfa();
        let states = nfa.states().len() as u128;
        let ps = states.saturating_mul(span as u128).saturating_mul(TRACK_PS);
        self.clock.room_for(Duration::from_nanos(u64::try_from(ps / 1000).unwrap_or(u64::MAX)))?;
        let visits = states.saturating_mul(span as u128 + 1);
        let blocks = visits.div_ceil(64);
        let visited_bytes = blocks.saturating_mul(8);
        if !room_for_bytes(visited_bytes) {
            return Err(Halt::Time);
        }
        let mut visited: Vec<u64> = Vec::new();
        visited.try_reserve_exact(usize::try_from(blocks).map_err(|_| Halt::Time)?).map_err(|_| Halt::Time)?;
        visited.resize(blocks as usize, 0);
        let mut track = Track { nfa, hay: within.haystack(), start: within.start(), end: within.end(), stride: span + 1, visited, stack: Vec::new(), visited_bytes, steps: 0 };
        let caps = &mut self.caps;
        caps.set_pattern(None);
        let slots = caps.slots_mut();
        slots.iter_mut().for_each(|slot| *slot = None);
        track.push(Frame::Step { sid: nfa.start_anchored(), at: within.start() })?;
        while let Some(frame) = track.stack.pop() {
            match frame {
                Frame::Step { sid, at } => {
                    if track.step(sid, at, slots, &mut self.clock)? {
                        caps.set_pattern(Some(regex_automata::PatternID::ZERO));
                        return Ok(());
                    }
                }
                Frame::Restore { slot, offset } => slots[slot as usize] = offset,
            }
        }
        Ok(())
    }
}

/// A frame of `Track`'s stack: a state to try at a place, or a slot to put back (regex-automata's
/// `backtrack::Frame`), 16 bytes.
enum Frame {
    Step { sid: regex_automata::util::primitives::StateID, at: usize },
    Restore { slot: u32, offset: Option<regex_automata::util::primitives::NonMaxUsize> },
}

/// regex-automata's bounded backtracker over one anchored span (`Guarding::track_long`).
struct Track<'a> {
    nfa: &'a thompson::NFA,
    hay: &'a [u8],
    start: usize,
    end: usize,
    stride: usize,
    visited: Vec<u64>,
    stack: Vec<Frame>,
    /// What the visits take, charged with the stack each time it grows.
    visited_bytes: u128,
    steps: u32,
}

impl Track<'_> {
    /// `frame` onto the stack, which grows only where its room, with the visits and the buffer it
    /// grows from, fits (`room_for_bytes`).
    #[inline(always)]
    fn push(&mut self, frame: Frame) -> Result<(), Halt> {
        if self.stack.len() == self.stack.capacity() {
            self.grow()?;
        }
        self.stack.push(frame);
        Ok(())
    }

    #[cold]
    #[inline(never)]
    fn grow(&mut self) -> Result<(), Halt> {
        let old = self.stack.capacity();
        let new = old.saturating_mul(2).max(1024);
        let frame = std::mem::size_of::<Frame>() as u128;
        if !room_for_bytes(self.visited_bytes.saturating_add(frame.saturating_mul(old as u128 + new as u128))) {
            return Err(Halt::Time);
        }
        self.stack.try_reserve_exact(new - self.stack.len()).map_err(|_| Halt::Time)
    }

    /// From `sid` at `at`, as regex-automata's `BoundedBacktracker::step` goes: true at a match.
    #[inline(always)]
    fn step(&mut self, mut sid: regex_automata::util::primitives::StateID, mut at: usize, slots: &mut [Option<regex_automata::util::primitives::NonMaxUsize>], clock: &mut Clock) -> Result<bool, Halt> {
        loop {
            let index = sid.as_usize() * self.stride + (at - self.start);
            let (block, bit) = (index / 64, 1u64 << (index % 64));
            if self.visited[block] & bit != 0 {
                return Ok(false);
            }
            self.visited[block] |= bit;
            // The clock, every 4,096 visits.
            self.steps = self.steps.wrapping_add(1);
            if self.steps % 4096 == 0 {
                clock.look()?;
            }
            match self.nfa.state(sid) {
                thompson::State::ByteRange { trans } => {
                    if at >= self.end || !trans.matches(self.hay, at) {
                        return Ok(false);
                    }
                    sid = trans.next;
                    at += 1;
                }
                thompson::State::Sparse(sparse) => {
                    if at >= self.end {
                        return Ok(false);
                    }
                    let Some(next) = sparse.matches(self.hay, at) else { return Ok(false) };
                    sid = next;
                    at += 1;
                }
                thompson::State::Dense(dense) => {
                    if at >= self.end {
                        return Ok(false);
                    }
                    let Some(next) = dense.matches(self.hay, at) else { return Ok(false) };
                    sid = next;
                    at += 1;
                }
                thompson::State::Look { look, next } => {
                    if !self.nfa.look_matcher().matches(*look, self.hay, at) {
                        return Ok(false);
                    }
                    sid = *next;
                }
                thompson::State::Union { alternates } => {
                    let Some(&first) = alternates.first() else { return Ok(false) };
                    sid = first;
                    for &alt in alternates[1..].iter().rev() {
                        self.push(Frame::Step { sid: alt, at })?;
                    }
                }
                thompson::State::BinaryUnion { alt1, alt2 } => {
                    sid = *alt1;
                    self.push(Frame::Step { sid: *alt2, at })?;
                }
                thompson::State::Capture { next, slot, .. } => {
                    if slot.as_usize() < slots.len() {
                        self.push(Frame::Restore { slot: slot.as_u32(), offset: slots[slot.as_usize()] })?;
                        slots[slot.as_usize()] = regex_automata::util::primitives::NonMaxUsize::new(at);
                    }
                    sid = *next;
                }
                thompson::State::Fail => return Ok(false),
                thompson::State::Match { .. } => return Ok(true),
            }
        }
    }
}

/// The most a guarded search may make for its own work, where nothing else holds it, as an
/// `http.*` answer's reading may (FEEDBACK75.md, Q-1072).
const WORK_MOST: u128 = 256 << 20;

/// Whether `bytes` more of a guarded search's own work fit: within `WORK_MOST`, and within
/// `--max-memory` where it is set. Where not, the search goes another way, or gives up (found in
/// review: the PikeVM's cache and the backtracker's stack were neither).
fn room_for_bytes(bytes: u128) -> bool {
    bytes <= WORK_MOST && usize::try_from(bytes).is_ok_and(|b| crate::httpd::making(b).is_ok())
}

/// The error value of a guarded search that gave up (out of line: in it, each call's frame was
/// larger, found measuring short calls).
#[cold]
#[inline(never)]
fn gave_up(f: &str, len: usize) -> Value {
    Value::err(Value::str(format!(
        "`{}`: the pattern is too costly for this text: from each place a match may start, the search follows more ways at once than it can keep (as [ab]*a[ab]{{900}}c does on a text of a and b), and following them one by one over these {} bytes could take longer than the quarter of a second a search may; repeat less, or search a shorter text",
        f, len
    )))
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

/// `matches`, each search made by the guard (FEEDBACK75.md, Q-1075; branch feedback75), its caches
/// made here: a loop of its own, so that the crate's is 0.8.8's, and the guard's state is out of the
/// frames of calls by the crate (found in review: 130 instructions a call).
#[inline(never)]
fn guarded_matches(s: &str, most: usize, g: &Guard, caps_ps: u64) -> Result<Vec<Found>, Stop> {
    let mut guarding = Guarding::new(g, caps_ps, s.len());
    let mut out = Vec::new();
    let mut pos = 0;
    let mut may_read = 0u64;
    let mut timed: Option<(Duration, Duration)> = None;
    let each = std::mem::size_of::<Found>() + guarding.count.saturating_sub(1) * std::mem::size_of::<Option<(usize, usize)>>();
    'all: while pos <= s.len() && out.len() < most {
        let until = if most == 1 {
            1
        } else {
            look(&mut out, s, pos, most, each, &mut may_read, &mut timed)?;
            (out.len() + 64).min(most)
        };
        while pos <= s.len() && out.len() < until {
            let found = guarding.find(s, pos, false).map_err(|h| match h {
                Halt::Time => Stop::GaveUp,
                Halt::Compute => Stop::Compute,
            })?;
            let Some(found) = found else { break 'all };
            let (start, end) = (found.start, found.end);
            out.push(found);
            pos = if end > start { end } else { end + s[end..].chars().next().map_or(1, char::len_utf8) };
        }
    }
    Ok(out)
}

/// What a windowed search reads at once past where it starts (`windowed_matches`).
const WINDOW: usize = 64 << 10;

/// The end of the window a search from `pos` reads: `WINDOW` bytes and the longest literal less one
/// past `pos`, so that each literal that starts in the first `WINDOW` bytes is whole in it; the
/// text's end at the most, and never inside a character.
fn window_end(s: &str, pos: usize, longest: usize) -> usize {
    let mut end = pos.saturating_add(WINDOW).saturating_add(longest.saturating_sub(1)).min(s.len());
    while !s.is_char_boundary(end) {
        end += 1;
    }
    end
}

/// Where the window after the one from `pos` starts: `WINDOW` bytes on, at a character.
fn next_window(s: &str, pos: usize) -> usize {
    let mut at = pos + WINDOW;
    while !s.is_char_boundary(at) {
        at += 1;
    }
    at
}

/// `matches` for a pattern the crate searches with literals alone, past its room (`Room::literal`):
/// each search the crate's, on a window of the text (`window_end`), its match kept where it starts in
/// the window's first `WINDOW` bytes, where every literal is whole and it is the match the whole text
/// gives, else the next window searched; and what each search may have read, its match and the
/// longest literal past it within the window, counted on the guard's clock (`Clock`), past which the
/// call is the error value, as a guarded call's is (FEEDBACK75.md follow-up found by 0.8.9's
/// coverage, Q-1125, review; branch feedback75).
#[inline(never)]
fn windowed_matches(r: &Regex, longest: usize, s: &str, most: usize) -> Result<Vec<Found>, Stop> {
    let mut clock = Clock::new(s.len());
    let mut out = Vec::new();
    let mut pos = 0;
    let mut may_read: u64;
    let mut timed: Option<(Duration, Duration)> = None;
    let halt = |h: Halt| match h {
        Halt::Time => Stop::GaveUp,
        Halt::Compute => Stop::Compute,
    };
    'all: while pos <= s.len() && out.len() < most {
        let until = if most == 1 {
            1
        } else {
            // A literal pattern has no group of its own: a match takes its `Found` alone. The time is
            // the clock's: what `look` counts of readings (each to the text's end) is cleared, its
            // limit of them, which a debug build does not lengthen as the clock is, otherwise first
            // there (found running the tests).
            may_read = 0;
            look(&mut out, s, pos, most, std::mem::size_of::<Found>(), &mut may_read, &mut timed)?;
            (out.len() + 64).min(most)
        };
        while pos <= s.len() && out.len() < until {
            let end = window_end(s, pos, longest);
            // `find_at`, not `captures_read_at`: a literal pattern has no group, and a second caller of
            // `captures_read_at` left it out of line in `matches` too, 30 instructions a match on every
            // pattern (the 0.8.9 benchmark after Q-1125; branch feedback75).
            match r.find_at(&s[..end], pos).map(|m| (m.start(), m.end())) {
                Some((start, stop)) if start < pos + WINDOW || end == s.len() => {
                    clock.bytes((stop + longest).min(end) - pos).map_err(halt)?;
                    out.push(Found { start, end: stop, groups: Vec::new() });
                    pos = if stop > start { stop } else { stop + s[stop..].chars().next().map_or(1, char::len_utf8) };
                }
                _ if end == s.len() => break 'all,
                _ => {
                    clock.bytes(end - pos).map_err(halt)?;
                    pos = next_window(s, pos);
                }
            }
        }
    }
    Ok(out)
}

/// `re.test` in windows (see `windowed_matches`): a match anywhere in a window is one in the text.
#[inline(never)]
fn windowed_test(r: &Regex, longest: usize, s: &str, line: u32) -> R {
    let mut clock = Clock::new(s.len());
    let mut pos = 0;
    loop {
        let end = window_end(s, pos, longest);
        if r.is_match_at(&s[..end], pos) {
            return Ok(Value::Bool(true));
        }
        if end == s.len() {
            return Ok(Value::Bool(false));
        }
        match clock.bytes(end - pos) {
            Ok(()) => pos = next_window(s, pos),
            Err(Halt::Time) => return Ok(gave_up("re.test", s.len())),
            Err(Halt::Compute) => return fail(line, crate::httpd::over_compute()),
        }
    }
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
/// The guard's searches where one is given, out of line, so that the crate's path is 0.8.8's (found
/// measuring: in line, 77 instructions more a `re.find` of a short text).
fn all_matches(r: &Regex, s: &str, most: usize, f: &str, line: u32, guard: Option<&Way>) -> Result<Result<Vec<Found>, Value>, Ctrl> {
    match guard {
        None => stopped(matches(r, s, most), s, f, line),
        Some(way) => all_guarded(r, way, s, most, f, line),
    }
}

#[inline(never)]
fn all_guarded(r: &Regex, way: &Way, s: &str, most: usize, f: &str, line: u32) -> Result<Result<Vec<Found>, Value>, Ctrl> {
    let found = match way {
        Way::Guard(g, caps_ps) => guarded_matches(s, most, g, *caps_ps),
        Way::Windows(longest) => windowed_matches(r, *longest, s, most),
    };
    stopped(found, s, f, line)
}

/// What `all_matches` gives for what the searches found or why they stopped.
#[inline(always)]
fn stopped(found: Result<Vec<Found>, Stop>, s: &str, f: &str, line: u32) -> Result<Result<Vec<Found>, Value>, Ctrl> {
    match found {
        Ok(found) => Ok(Ok(found)),
        Err(Stop::GaveUp) => Ok(Err(gave_up(f, s.len()))),
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

/// `re.test` by the guard (see `guarded_matches`).
#[inline(never)]
fn guarded_test(g: &Guard, caps_ps: u64, s: &str, line: u32) -> R {
    match Guarding::new(g, caps_ps, s.len()).find(s, 0, true) {
        Ok(m) => Ok(Value::Bool(m.is_some())),
        Err(Halt::Time) => Ok(gave_up("re.test", s.len())),
        Err(Halt::Compute) => fail(line, crate::httpd::over_compute()),
    }
}

/// Where a call's group names are read: the crate's Regex, or the guard's NFA where the call went
/// by the guard (whose Regex may be `unused()`). The same names, in the same order: both are the
/// group info of one pattern compiled the same way.
#[derive(Clone, Copy)]
enum Names<'a> {
    Crate(&'a Regex),
    Guard(&'a Guard),
}

impl<'a> Names<'a> {
    fn of(r: &'a Regex, guard: Option<&'a Way>) -> Names<'a> {
        match guard {
            Some(Way::Guard(g, _)) => Names::Guard(g),
            _ => Names::Crate(r),
        }
    }

    /// Each group's name, the whole match's (none) first.
    fn list(self) -> Vec<Option<&'a str>> {
        match self {
            Names::Crate(r) => r.capture_names().collect(),
            Names::Guard(g) => g.groups.get_nfa().group_info().pattern_names(regex_automata::PatternID::ZERO).collect(),
        }
    }

    /// `f` of each named group's index and name.
    fn each(self, mut f: impl FnMut(usize, &'a str)) {
        match self {
            Names::Crate(r) => r.capture_names().enumerate().for_each(|(i, n)| n.into_iter().for_each(|n| f(i, n))),
            Names::Guard(g) => g.groups.get_nfa().group_info().pattern_names(regex_automata::PatternID::ZERO).enumerate().for_each(|(i, n)| n.into_iter().for_each(|n| f(i, n))),
        }
    }
}

/// A match as a map: `{ text, start, end, groups, named }`, with positions
/// in characters (`chars` counts them up to `at`, from where it got last).
fn match_value(names: Names, s: &str, m: &Found, chars: &mut (usize, usize)) -> Value {
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
    names.each(|i, n| {
        if i > 0 {
            named.insert(n.into(), text(m.groups[i - 1]));
        }
    });
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
    let mut pat = pattern(&args, 2, "re.test", line, s.len(), false)?;
    // By its guard where the crate could take too long (FEEDBACK75.md, Q-1075; branch feedback75).
    let Ok(guard) = guard_for(&mut pat, Key { args: &args, flags_at: 2 }, s.len(), false) else { return Ok(gave_up("re.test", s.len())) };
    match guard {
        Some(Way::Guard(g, caps_ps)) => return guarded_test(&g, caps_ps, &s, line),
        Some(Way::Windows(longest)) => return windowed_test(&pat.r, longest, &s, line),
        None => {}
    }
    Ok(Value::Bool(pat.r.is_match(&s)))
}

/// `re.find(s, pattern, flags)`: the first match, or null.
fn find(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let s = text_arg(&args, "re.find", line)?;
    let mut pat = pattern(&args, 2, "re.find", line, s.len(), true)?;
    let Ok(guard) = guard_for(&mut pat, Key { args: &args, flags_at: 2 }, s.len(), true) else { return Ok(gave_up("re.find", s.len())) };
    let r = &pat.r;
    let found = match all_matches(r, &s, 1, "re.find", line, guard.as_ref())? {
        Ok(found) => found,
        Err(e) => return Ok(e),
    };
    Ok(found.first().map_or(Value::Nil, |m| match_value(Names::of(r, guard.as_ref()), &s, m, &mut (0, 0))))
}

/// `re.find_all(s, pattern, flags)`: every match, from left to right.
fn find_all(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let s = text_arg(&args, "re.find_all", line)?;
    let mut pat = pattern(&args, 2, "re.find_all", line, s.len(), true)?;
    // Its searches by the guard where one could take too long (FEEDBACK75.md, Q-1075; branch
    // feedback75).
    let Ok(guard) = guard_for(&mut pat, Key { args: &args, flags_at: 2 }, s.len(), true) else { return Ok(gave_up("re.find_all", s.len())) };
    let r = &pat.r;
    let found = match all_matches(r, &s, usize::MAX, "re.find_all", line, guard.as_ref())? {
        Ok(found) => found,
        Err(e) => return Ok(e),
    };
    // A map for each, asked about first under `--max-memory` (FEEDBACK71.md, Q-977; branch
    // security).
    crate::httpd::making(found.len().saturating_mul(ITEM_BYTES)).map_err(|s| s.at(line))?;
    let mut chars = (0, 0);
    let names = Names::of(r, guard.as_ref());
    Ok(Value::list(found.iter().map(|m| match_value(names, &s, m, &mut chars)).collect()))
}

/// `re.replace(s, pattern, with, flags)`: every match replaced by `with`,
/// text in which `$1`, `$<name>` and `$0` are the match's and `$$` is `$`,
/// or a function given the match (as `re.find` gives it).
fn replace(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let s = text_arg(&args, "re.replace", line)?;
    let mut pat = pattern(&args, 3, "re.replace", line, s.len(), true)?;
    // Its searches by the guard where one could take too long (FEEDBACK75.md, Q-1075; branch
    // feedback75).
    let Ok(guard) = guard_for(&mut pat, Key { args: &args, flags_at: 3 }, s.len(), true) else { return Ok(gave_up("re.replace", s.len())) };
    let r = &pat.r;
    let with = args.get(2).cloned().unwrap_or(Value::Nil);
    if !matches!(with, Value::Str(_) | Value::Func(_) | Value::Builtin(..)) {
        return fail(line, format!("`re.replace` expects what to put in (a str or a function) as argument 3, got {}", with.type_name()));
    }
    let names: Vec<Option<&str>> = Names::of(r, guard.as_ref()).list();
    let found = match all_matches(r, &s, usize::MAX, "re.replace", line, guard.as_ref())? {
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
                let v = match_value(Names::of(r, guard.as_ref()), &s, &m, &mut chars);
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
    let mut pat = pattern(&args, 2, "re.split", line, s.len(), true)?;
    // Its searches by the guard where one could take too long (FEEDBACK75.md, Q-1075; branch
    // feedback75).
    let Ok(guard) = guard_for(&mut pat, Key { args: &args, flags_at: 2 }, s.len(), true) else { return Ok(gave_up("re.split", s.len())) };
    let r = &pat.r;
    let mut out = Vec::new();
    let mut last = 0;
    let found = match all_matches(r, &s, usize::MAX, "re.split", line, guard.as_ref())? {
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
