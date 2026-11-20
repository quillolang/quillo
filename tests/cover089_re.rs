//! Coverage of 0.8.9's regex budget (FEEDBACK75.md, Q-1075; branch feedback75): the lines of
//! src/re.rs added for it that no test ran at 60a58d6. A search the regex crate could take too
//! long over goes by the guard (a lazy DFA held to a time, the one-pass DFA, the PikeVM or a bounded
//! backtracker for a match's groups); each test here gives the guard a text longer than its
//! pattern's room, and asserts what it answers against the regex crate's own search of the same
//! pattern (`quillo::re::compile`), or the stop it should be.

mod support;

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn dir(name: &str) -> PathBuf {
    support::temp_dir(&format!("cover089-re-{}", name))
}

/// `src`, which sets `out`, run in `d`: `out` as the interpreter writes it, or the error.
fn try_ask(d: &Path, src: &str) -> Result<String, String> {
    let mut it = quillo::Interp::new(d);
    it.load_source(src, "t.qwe").map_err(|e| e.to_string())?;
    Ok(quillo::value::lookup(&it.globals, "out").unwrap().repr())
}

fn ask(d: &Path, src: &str) -> String {
    try_ask(d, src).unwrap_or_else(|e| panic!("{}", e))
}

/// `n` characters of `alpha`, the same each time for a seed.
fn mixed_text(n: usize, alpha: &str, seed: u32) -> String {
    let alpha: Vec<char> = alpha.chars().collect();
    let mut x = seed;
    (0..n)
        .map(|_| {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            alpha[(x >> 16) as usize % alpha.len()]
        })
        .collect()
}

/// A line of words, about `n` bytes, the same each time.
fn words_line(n: usize, seed: u32) -> String {
    let words = ["lorem", "ipsum", "at", "dolor", "sit", "amet", "consectetur", "elit"];
    let mut x = seed;
    let mut text = String::with_capacity(n + 16);
    while text.len() < n {
        x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        text.push_str(words[(x >> 16) as usize % words.len()]);
        text.push(' ');
    }
    text.truncate(n);
    text
}

/// The matches `re.find_all` gives, by the regex crate: from left to right as `matches` finds
/// them (after an empty one, a character on), each `[start, end, groups]` with positions in
/// characters, as Quillo writes the list.
fn crate_matches(p: &str, flags: &str, s: &str) -> Vec<(usize, usize, Vec<Option<(usize, usize)>>)> {
    // The last pattern compiled is compiled once (a list of 5,000 hashes took a second in a debug
    // build).
    thread_local! {
        static LAST: std::cell::RefCell<Option<((String, String), regex::Regex)>> = const { std::cell::RefCell::new(None) };
    }
    let r = LAST.with(|l| {
        let mut l = l.borrow_mut();
        match &*l {
            Some((k, r)) if k.0 == p && k.1 == flags => r.clone(),
            _ => {
                let r = quillo::re::compile(p, flags).unwrap();
                *l = Some(((p.to_string(), flags.to_string()), r.clone()));
                r
            }
        }
    });
    let mut out = Vec::new();
    let mut pos = 0;
    while pos <= s.len() {
        let Some(c) = r.captures_at(s, pos) else { break };
        let m = c.get(0).unwrap();
        out.push((m.start(), m.end(), (1..c.len()).map(|i| c.get(i).map(|g| (g.start(), g.end()))).collect()));
        pos = if m.end() > m.start() { m.end() } else { m.end() + s[m.end()..].chars().next().map_or(1, char::len_utf8) };
    }
    out
}

fn written(s: &str, found: &[(usize, usize, Vec<Option<(usize, usize)>>)]) -> String {
    let mut chars = vec![0usize; s.len() + 1];
    let mut n = 0;
    for (i, c) in s.char_indices() {
        chars[i] = n;
        for k in 1..c.len_utf8() {
            chars[i + k] = n;
        }
        n += 1;
    }
    chars[s.len()] = n;
    let each: Vec<String> = found
        .iter()
        .map(|(a, b, groups)| {
            let g: Vec<String> = groups.iter().map(|g| g.map_or("null".to_string(), |(x, y)| format!("{:?}", &s[x..y]))).collect();
            format!("[{}, {}, [{}]]", chars[*a], chars[*b], g.join(", "))
        })
        .collect();
    format!("[{}]", each.join(", "))
}

/// The five calls of `p` (with `flags`) on `text` through Quillo, each the regex crate's answer:
/// `re.find_all`'s matches with their groups, `re.find`'s first, `re.test`, `re.replace` with
/// `<$0>`, and `re.split`. Returns how many matches there are.
fn same_as_crate(d: &Path, p: &str, flags: &str, text: &str) -> usize {
    std::fs::write(d.join("p.txt"), p).unwrap();
    std::fs::write(d.join("f.txt"), flags).unwrap();
    std::fs::write(d.join("t.txt"), text).unwrap();
    let found = crate_matches(p, flags, text);
    let mut replaced = String::new();
    let mut parts: Vec<&str> = Vec::new();
    let mut last = 0;
    for (a, b, _) in &found {
        replaced.push_str(&text[last..*a]);
        replaced.push('<');
        replaced.push_str(&text[*a..*b]);
        replaced.push('>');
        parts.push(&text[last..*a]);
        last = *b;
    }
    replaced.push_str(&text[last..]);
    parts.push(&text[last..]);
    std::fs::write(d.join("replaced.txt"), &replaced).unwrap();
    std::fs::write(d.join("split.txt"), parts.join("\u{1}")).unwrap();
    let head = "let t = read_file(\"t.txt\")\nlet p = read_file(\"p.txt\")\nlet f = read_file(\"f.txt\")\n";
    let all = ask(d, &format!("{}let a = re.find_all(t, p, f)\nlet out = is_err(a) ? a : map(a, (m) => [m.start, m.end, m.groups])\n", head));
    assert_eq!(all, written(text, &found), "re.find_all of {:.80} ({:?}) on {} bytes", p, flags, text.len());
    let first = ask(d, &format!("{}let m = re.find(t, p, f)\nlet out = m == null || is_err(m) ? m : [[m.start, m.end, m.groups]]\n", head));
    assert_eq!(first, if found.is_empty() { "null".to_string() } else { written(text, &found[..1]) }, "re.find of {:.80}", p);
    let test = ask(d, &format!("{}let out = re.test(t, p, f)\n", head));
    assert_eq!(test, (!found.is_empty()).to_string(), "re.test of {:.80}", p);
    let rep = ask(d, &format!("{}let r = re.replace(t, p, \"<$0>\", f)\nlet out = is_err(r) ? r : r == read_file(\"replaced.txt\")\n", head));
    assert_eq!(rep, "true", "re.replace of {:.80}", p);
    let split = ask(d, &format!("{}let s = re.split(t, p, f)\nlet out = is_err(s) ? s : join(s, \"\\u{{1}}\") == read_file(\"split.txt\")\n", head));
    assert_eq!(split, "true", "re.split of {:.80}", p);
    found.len()
}

/// `quillo run` of `src` as a script in `d` with `args`, given `secs` seconds: its exit code,
/// stdout and stderr.
fn script(d: &Path, src: &str, args: &[&str], secs: u64) -> (Option<i32>, String, String) {
    std::fs::write(d.join("s.qwe"), src).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", "s.qwe"])
        .args(args)
        .current_dir(d)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    support::stop_with_tests(&child);
    let (mut out, mut err) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
    let o = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = out.read_to_string(&mut s);
        s
    });
    let e = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = err.read_to_string(&mut s);
        s
    });
    let deadline = Instant::now() + Duration::from_secs(secs);
    let code = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status.code();
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    (code, o.join().unwrap(), e.join().unwrap())
}

/// A `quillo run` server this test started, on the port the system picked, its stderr in a file;
/// killed when dropped.
struct Run {
    child: std::process::Child,
    port: u16,
    log: PathBuf,
}

impl Drop for Run {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Run {
    fn start(d: &Path, src: &str, args: &[&str]) -> Run {
        std::fs::write(d.join("app.qwe"), src).unwrap();
        let log = d.join("app.log");
        let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
            .args(["run", "app.qwe", "--port", "0"])
            .args(args)
            .current_dir(d)
            .env("QUILLO_SECRET", "a secret for the cover089_re tests, long enough")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(&log).unwrap())
            .spawn()
            .unwrap();
        support::stop_with_tests(&child);
        let mut r = Run { child, port: 0, log };
        r.port = support::port_in_log(&mut r.child, &r.log, Duration::from_secs(60));
        r
    }

    fn log(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
}

/// `head` and then `body`, raw, on a connection of its own: the status and the answer's body.
fn raw(port: u16, head: &str, body: &[u8]) -> (u16, String) {
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(if cfg!(debug_assertions) { 600 } else { 120 }))).unwrap();
    s.write_all(head.as_bytes()).unwrap();
    let _ = s.write_all(body);
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let out = String::from_utf8_lossy(&out).into_owned();
    let (h, b) = out.split_once("\r\n\r\n").unwrap_or((&out, ""));
    let status = h.lines().next().and_then(|l| l.split(' ').nth(1)).and_then(|c| c.parse().ok()).unwrap_or(0);
    (status, b.to_string())
}

fn post(port: u16, path: &str, body: &str) -> (u16, String) {
    let head = format!("POST {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n", path, body.len());
    raw(port, &head, body.as_bytes())
}

const COSTLY: &str = "the pattern is too costly for this text";

// ---- Choosing the search ------------------------------------------------------------------------

/// A pattern that does not compile is its error on a long text too, where the first compile is the
/// guard's (`nfa_first`), which cannot make it and leaves it to the crate's compile: the same
/// message as on a short text, an unclosed group, bad flags, and a list of 3,000 words that ends
/// with `\` or has an escape there is none of alike (src/re.rs `compile_and_keep`, `many_literals`,
/// FEEDBACK75.md Q-1075 and its follow-up Q-1125; coverage of 0.8.9; branch feedback75).
#[test]
fn a_pattern_that_does_not_compile_is_its_error_on_a_long_text() {
    let d = dir("bad");
    std::fs::write(d.join("long.txt"), "a".repeat(70_000)).unwrap();
    std::fs::write(d.join("short.txt"), "aaa").unwrap();
    let list = words(3_000, 9).join("|");
    let (ends, odd) = (list.clone() + "\\", list.replacen('|', "\\q|", 1));
    // The pattern and flags from data, as `quillo check` would refuse them written out.
    for (f, p, flags, want) in [
        ("re.test", "(ab", "", "`re.test`: `(` without its `)`"),
        ("re.find", "a{2,1}", "", "`re.find`: in {n,m}, n is more than m"),
        ("re.find_all", "a", "x", "`re.find_all`: the flags are `i` (any case), `m` (^ and $ at each line) and `s` (. matches line breaks), each once; got \"x\""),
        ("re.find_all", ends.as_str(), "", "`re.find_all`: the pattern ends with `\\`"),
        ("re.test", odd.as_str(), "", "`re.test`: `\\q` is not one of the escapes patterns have"),
    ] {
        std::fs::write(d.join("p.txt"), p).unwrap();
        std::fs::write(d.join("f.txt"), flags).unwrap();
        for file in ["long.txt", "short.txt"] {
            let e = try_ask(&d, &format!("let t = read_file({:?})\nlet out = {}(t, read_file(\"p.txt\"), read_file(\"f.txt\"))\n", file, f)).expect_err(f);
            assert!(e.contains(want), "{} {:.40} on {}: {}", f, p, file, e);
        }
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// Where the literal inside a pattern is fast to find alone but not with what follows it
/// (`[xyz]`, then `q`), the inner search looks for it alone, as the crate's does, and every match,
/// first and last, with the five calls, is the crate's, on 150 KB with no match too
/// (src/re.rs `inner::extract`, FEEDBACK75.md Q-1075; coverage of 0.8.9; branch feedback75).
#[test]
fn an_inner_literal_slow_with_what_follows_is_looked_for_alone() {
    let d = dir("inner-alone");
    let p = r"[^@\s]{3}[xyz]q[a-z]*!{0,300}";
    let n = same_as_crate(&d, p, "", &mixed_text(150_000, "abcxyzq! @\n", 3));
    assert!(n > 1000, "{}", n);
    assert_eq!(same_as_crate(&d, p, "", &mixed_text(150_000, "abcxyz! @\n", 4)), 0);
    let _ = std::fs::remove_dir_all(&d);
}

/// A concatenation that is one part once its groups are taken away (`()` and a repeat) has no
/// inner literal, and the guard searches it forward: its matches, their empty groups, the empty
/// match at the end, as the crate gives them (src/re.rs `inner::top_concat`, `inner::flatten`,
/// FEEDBACK75.md Q-1075; coverage of 0.8.9; branch feedback75).
#[test]
fn a_concatenation_of_one_part_has_no_inner_literal() {
    let d = dir("one-part");
    let n = same_as_crate(&d, "()[ab]{0,900}", "", &mixed_text(100_000, "ab", 7));
    // 900 at a time, and the empty match at the end.
    assert_eq!(n, 100_000 / 900 + 2);
    // A `c` every 5,000 bytes: an empty match there, and the runs between.
    let with_c: String = mixed_text(100_000, "ab", 8).char_indices().map(|(i, c)| if i % 5_000 == 4_999 { 'c' } else { c }).collect();
    assert_eq!(same_as_crate(&d, "()[ab]{0,900}", "", &with_c), 20 * 7 + 1);
    let _ = std::fs::remove_dir_all(&d);
}

/// The proofs that the first match found from an inner literal is the crate's: a class that every
/// match has before the literal, disjoint from it and from the parts in front (`\b`, a repeat of
/// `@@|-#`, digits, then `[a-z]+` before `@`), lets the guard search from each `@`, and a repeat of
/// more than a class (`(?:[a-z]0)+`) or a class holding the literal is no proof, and the guard
/// searches forward; every match and its groups are the crate's, on texts full of `@`, `-#`
/// and digits (src/re.rs `inner::has_no_earlier_match` and the functions it calls, FEEDBACK75.md
/// Q-1075; coverage of 0.8.9; branch feedback75).
#[test]
fn a_class_before_the_inner_literal_proves_the_first_match() {
    let d = dir("separator");
    let p = r"\b((?:@@|-#)*)[0-9]*([a-z]+)@[a-z]+x{0,300}";
    let n = same_as_crate(&d, p, "", &mixed_text(80_000, "ab@-#01 x\n", 11));
    assert!(n > 500, "{}", n);
    assert!(same_as_crate(&d, p, "", &mixed_text(70_000, "abcdefgh@@-#0x ", 12)) > 500);
    let q = r"[!-@]+(?:[a-z]0)+@x(?:y{0,300})";
    let n = same_as_crate(&d, q, "", &mixed_text(120_000, "!#@a0b0x y", 13));
    assert!(n > 20, "{}", n);
    let _ = std::fs::remove_dir_all(&d);
}

/// A thread keeps the guards of 64 patterns: the 65th sends the oldest away, and that one is made
/// again when it is next used, with the same answer; each of the 65 finds its match on 60 KB
/// (src/re.rs `guard_of`, FEEDBACK75.md Q-1075; coverage of 0.8.9; branch feedback75).
#[test]
fn more_guards_than_a_thread_keeps_are_made_again() {
    let d = dir("guards");
    let text = "a".repeat(59_000) + "zq" + &mixed_text(1_000, "ab", 5);
    std::fs::write(d.join("t.txt"), &text).unwrap();
    let mut it = quillo::Interp::new(&d);
    let mut find = |n: usize| {
        it.load_source(&format!("let m = re.find(read_file(\"t.txt\"), \"zq[ab]{{{}}}\")\nlet out = [m.start, m.end, m.text == slice(read_file(\"t.txt\"), m.start, m.end)]\n", n), "t.qwe").unwrap_or_else(|e| panic!("{}", e));
        quillo::value::lookup(&it.globals, "out").unwrap().repr()
    };
    for n in 900..=964 {
        assert_eq!(find(n), format!("[59000, {}, true]", 59_002 + n), "zq[ab]{{{}}}", n);
    }
    // The first, sent away by the 65th, and one kept.
    assert_eq!(find(900), "[59000, 59902, true]");
    assert_eq!(find(964), "[59000, 59966, true]");
    let _ = std::fs::remove_dir_all(&d);
}

// ---- The searches -------------------------------------------------------------------------------

/// A pattern that begins with `\b` and a literal is searched from each place its prefilter finds,
/// the DFA started again there for what comes before it (a word character or not): `foo` after
/// `x`, after `_`, at the start of a line and after a space, its matches the crate's
/// (src/re.rs `forward`, FEEDBACK75.md Q-1075; coverage of 0.8.9; branch feedback75).
#[test]
fn a_prefilter_before_a_word_boundary_starts_the_dfa_again() {
    let d = dir("restart");
    let unit = "xfoo foo afoo_foo foox9 foofoo _foo\nfoo-foo lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor ";
    let text = unit.repeat(30_000 / unit.len() + 1);
    let n = same_as_crate(&d, r"\bfoo(?:x{0,900})", "", &text);
    assert_eq!(n, 5 * (30_000 / unit.len() + 1));
    let n = same_as_crate(&d, r"\bfoo(?:x{0,900})", "", &("xfoo".to_string() + &"y".repeat(60_000) + " foox"));
    assert_eq!(n, 1);
    let _ = std::fs::remove_dir_all(&d);
}

/// A pattern whose every match ends where the text does is read back from the end, as the crate
/// reads it: `x[ab]{900}$` and its groups, a repeat that may be empty (its match at the end, then
/// the empty one), and `(?:[ab]*b|...)$` back to the text's start; with no match too (src/re.rs
/// `Guarding::find_at_end`, `backward`, FEEDBACK75.md Q-1075; coverage of 0.8.9; branch
/// feedback75).
#[test]
fn a_pattern_ending_at_the_end_is_read_back_from_there() {
    let d = dir("at-end");
    let tail = mixed_text(900, "ab", 21);
    let text = mixed_text(100_000, "ab", 22) + "x" + &tail;
    assert_eq!(same_as_crate(&d, "x[ab]{900}$", "", &text), 1);
    assert_eq!(same_as_crate(&d, "(x)([ab]{900})$", "", &text), 1);
    assert_eq!(same_as_crate(&d, "x[ab]{900}$", "", &(text.clone() + "c")), 0);
    assert_eq!(same_as_crate(&d, "[ab]{0,900}$", "", &text), 2);
    assert_eq!(same_as_crate(&d, "(?:[ab]*b|c{0,300}d)$", "", &(mixed_text(100_000, "ab", 23) + "b")), 1);
    assert_eq!(same_as_crate(&d, "(?:[ab]*b|c{0,300}d)$", "", &(mixed_text(100_000, "ab", 23) + "a")), 0);
    let _ = std::fs::remove_dir_all(&d);
}

/// A pattern that is its literals and nothing else (200 words) is found by the crate's own search,
/// its prefilter alone, on 100 KB, where its room from its NFA is 32 KB: every word, first and last,
/// through the five calls, and none in a text without them (changed with Q-1125, which gives such a
/// pattern to the crate on any text, as 0.8.8 did, where the guard searched it by the same
/// prefilter; src/re.rs `crate_searches_literals`, FEEDBACK75.md Q-1075 and its follow-up Q-1125;
/// coverage of 0.8.9; branch feedback75).
#[test]
fn a_pattern_of_literals_is_found_by_its_prefilter() {
    let d = dir("literal");
    let mut x = 3u32;
    let mut next = |n: usize| {
        x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        (x >> 16) as usize % n
    };
    let mut words = std::collections::BTreeSet::new();
    while words.len() < 200 {
        let n = 6 + next(5);
        words.insert((0..n).map(|_| char::from(b'a' + next(26) as u8)).collect::<String>());
    }
    let words: Vec<String> = words.into_iter().collect();
    let mut text = String::new();
    while text.len() < 100_000 {
        if next(10) == 0 {
            text.push_str(&words[next(words.len())]);
        } else {
            let n = 2 + next(8);
            text.extend((0..n).map(|_| char::from(b'a' + next(26) as u8)));
        }
        text.push(' ');
    }
    let p = words.join("|");
    let n = same_as_crate(&d, &p, "", &text);
    assert!(n > 500, "{}", n);
    assert_eq!(same_as_crate(&d, &p, "", &"0123 456 ".repeat(12_000)), 0);
    let _ = std::fs::remove_dir_all(&d);
}

/// The inner search's readings forward from where a match may start: one that reads past the next
/// `@` before it fails (`a@b@c d@e!`) sends the search forward from the start, as the crate's does;
/// one that reaches the end of the text with no match, and an `@` that is the text's last byte,
/// end it with none, and one whose match is known only at the end of the text ends with it; a part
/// in front that comes back to where it began (`[ab@]*?` before `-#`) is read through; each answer
/// the crate's (src/re.rs `forward_stopat`, `Guarding::find_inner`, FEEDBACK75.md Q-1075; coverage
/// of 0.8.9; branch feedback75).
#[test]
fn the_inner_search_reads_past_the_next_literal_and_to_the_end() {
    let d = dir("inner-ends");
    let p = r"[^@\s]+@[^\s]+!x{0,300}";
    let filler = words_line(100_000, 31);
    assert_eq!(same_as_crate(&d, p, "", &(filler.clone() + " a@b@c d@e! f")), 1);
    assert_eq!(same_as_crate(&d, p, "", &(filler.clone() + " abc@def")), 0);
    assert_eq!(same_as_crate(&d, p, "", &(filler.clone() + " abc@")), 0);
    assert_eq!(same_as_crate(&d, p, "", &(filler.clone() + " d@e!")), 1);
    assert_eq!(same_as_crate(&d, p, "", &(filler.clone() + " a@b@c d@e!xx g@h@i! j@k")), 2);
    assert!(same_as_crate(&d, p, "", &mixed_text(150_000, "ab@ !\n", 32)) > 1000);
    let lazy = "[ab@]*?-#{1,3}(?:Q{0,300})";
    assert!(same_as_crate(&d, lazy, "", &mixed_text(100_000, "ab@@..-# x\n", 33)) > 300);
    assert_eq!(same_as_crate(&d, lazy, "", &mixed_text(100_000, "ab@@..- x\n", 34)), 0);
    let _ = std::fs::remove_dir_all(&d);
}

/// An empty match the guard finds inside a character (`\B` between the bytes of `é`, `ö` and `☕`)
/// is passed over a byte at a time, as the crate does: `re.replace` and `re.split` by the guard
/// (the pattern with a branch that never matches, which makes its room small) give what the crate
/// gives for `\B` alone, on 48 KB (src/re.rs `Guarding::find_forward`, FEEDBACK75.md Q-1075;
/// coverage of 0.8.9; branch feedback75).
#[test]
fn an_empty_match_inside_a_character_is_passed_over() {
    let d = dir("split-char");
    // Few places between two word characters or two others, and many inside a character.
    std::fs::write(d.join("t.txt"), "aébécö☕d☕e xy, ü9".repeat(2_000)).unwrap();
    for (call, rest) in [("re.replace", ", \"<$0>\""), ("re.split", ""), ("re.find_all", "")] {
        let src = |p: &str| format!("let r = {}(read_file(\"t.txt\"), {:?}{})\nlet out = is_err(r) ? r : crypto.sha256(str(r))\n", call, p, rest);
        let guarded = ask(&d, &src(r"\B|[ab]{1000}\u0001"));
        assert!(!guarded.starts_with("err("), "{}: {}", call, guarded);
        assert_eq!(guarded, ask(&d, &src(r"\B")), "{}", call);
    }
    assert_eq!(ask(&d, "let out = len(re.find_all(read_file(\"t.txt\"), \"\\\\B|[ab]{1000}\\\\u0001\"))\n"), ask(&d, "let out = len(re.find_all(read_file(\"t.txt\"), \"\\\\B\"))\n"));
    let _ = std::fs::remove_dir_all(&d);
}

// ---- A match's groups ---------------------------------------------------------------------------

/// A match too long for the bounded backtracker's visits but within the PikeVM's budget gets its
/// groups from the PikeVM: `^(\S+)\s*(.*)$` with `m` on a text whose first line is 100 KB and whose
/// 700 KB of lines after it begin with a space, which it does not match, as the crate gives them (src/re.rs `Guarding::groups_of`, FEEDBACK75.md Q-1075;
/// coverage of 0.8.9; branch feedback75).
#[test]
fn a_long_match_within_the_pikevms_budget_gets_its_groups() {
    let d = dir("pikevm");
    let mut text = words_line(100_000, 41);
    let mut x = 5u32;
    while text.len() < 800_000 {
        text.push_str("\n ");
        x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        text.push_str(&words_line(10 + (x >> 16) as usize % 70, x));
    }
    assert_eq!(same_as_crate(&d, r"^(\S+)\s*(.*)$", "m", &text), 1);
    let _ = std::fs::remove_dir_all(&d);
}

const TRACKED: &str = r"^(?:(a)b|[^\s\S]|a)(?:a|c{0,300}d)*?(?:aa|a)$";

/// The groups of a long match that is not one-pass, by the guard's bounded backtracker, which backs
/// out of a group whose branch fails (`(a)b` before `a`), of a class that matches nothing, of `$`
/// before the end and of a byte that is not there, and does not visit a state at a place twice: the
/// crate's match and group on 100 KB beginning with `ab`, and on 100 KB of `a`, where the group is
/// null (src/re.rs `Guarding::track_long`, `Track::step`, FEEDBACK75.md Q-1075; coverage of 0.8.9;
/// branch feedback75).
#[test]
fn the_backtracker_backs_out_of_groups_and_dead_ends() {
    let d = dir("track");
    let find = |text: &str| {
        assert_eq!(same_as_crate(&d, TRACKED, "", text), 1);
        ask(&d, &format!("let m = re.find(read_file(\"t.txt\"), {:?})\nlet out = [m.start, m.end, m.groups]\n", TRACKED))
    };
    assert_eq!(find(&("ab".to_string() + &"a".repeat(100_000))), "[0, 100002, [\"a\"]]");
    assert_eq!(find(&"a".repeat(100_001)), "[0, 100001, [null]]");
    let _ = std::fs::remove_dir_all(&d);
}

/// The backtracker's set of visits is asked for before it is made: under `--max-memory 4MB`, the
/// 7.7 MB it needs for the match above (100 KB, 619 states) is refused, and the call is the error
/// value; without the limit the same script gives the match and its group (src/re.rs `Guarding::track_long`,
/// FEEDBACK75.md Q-1075; coverage of 0.8.9; branch feedback75).
#[test]
fn the_backtrackers_visits_past_max_memory_are_refused() {
    let d = dir("visits");
    std::fs::write(d.join("t.txt"), "ab".to_string() + &"a".repeat(100_000)).unwrap();
    let src = format!("let m = re.find(read_file(\"t.txt\"), {:?})\nprint(is_err(m) ? str(m) : str([m.start, m.end, m.groups]))\nprint(\"done\")\n", TRACKED);
    let secs = if cfg!(debug_assertions) { 600 } else { 120 };
    let (code, out, err) = script(&d, &src, &["--max-memory", "4MB"], secs);
    assert_eq!(code, Some(0), "{}\n{}", out, err);
    let first = out.lines().next().unwrap_or_default();
    assert!(first.starts_with("err(") && first.contains(&format!("`re.find`: {}", COSTLY)) && first.contains("these 100002 bytes"), "{}", out);
    let (code, out, err) = script(&d, &src, &[], secs);
    assert_eq!(code, Some(0), "{}\n{}", out, err);
    assert_eq!(out, "[0, 100002, [\"a\"]]\ndone\n");
    let _ = std::fs::remove_dir_all(&d);
}

/// Under `--max-compute`, the groups of a long match are not begun where they cannot be found in
/// the time the request has left: the PikeVM's on a 600 KB first line and the backtracker's on a
/// 4 MB line each end the request with the compute stop under `--max-compute 100ms`, as a search
/// past it does (`re.find`); and the server goes on (src/re.rs `Guarding::groups_of`,
/// `Clock::room_for`, `guarded_matches`, FEEDBACK75.md Q-1075; coverage of 0.8.9; branch
/// feedback75).
#[test]
fn groups_past_max_compute_are_not_begun() {
    let d = dir("compute");
    let src = "route POST /pike {\n    let m = re.find(request.body, \"^(\\\\S+)\\\\s*(.*)$\", \"m\")\n    return str(m.end)\n}\nroute POST /track {\n    let m = re.find(request.body, \"^(\\\\S+)\\\\s*(.*)$\")\n    return str(m.end)\n}\nroute GET /ok {\n    return str(re.find(\"say hello\", \"(h)(e)llo\").groups)\n}\n";
    let srv = Run::start(&d, src, &["--max-compute", "100ms"]);
    let mut pike = words_line(600_000, 51);
    while pike.len() < 1_000_000 {
        pike.push('\n');
        pike.push_str(&words_line(60, pike.len() as u32));
    }
    for (path, body) in [("/pike", pike), ("/track", words_line(4_000_000, 52))] {
        let (status, answer) = post(srv.port, path, &body);
        assert_eq!(status, 500, "{}: {}", path, answer);
    }
    let log = srv.log();
    assert_eq!(log.matches("this request computed for longer than --max-compute allows").count(), 2, "{}", log);
    assert_eq!(raw(srv.port, "GET /ok HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n", b""), (200, "[\"h\", \"e\"]".to_string()));
    let _ = std::fs::remove_dir_all(&d);
}


// ---- Q-1125 and Q-1126: what the crate searches alone, and what it refuses ---------------------------

/// `n` strings of 40 hex digits, the same each time for a seed.
fn hashes(n: usize, seed: u64) -> Vec<String> {
    let mut x = seed;
    (0..n)
        .map(|_| {
            (0..5)
                .map(|_| {
                    x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                    format!("{:08x}", x >> 32)
                })
                .collect()
        })
        .collect()
}

/// `n` different words of 5 to 10 letters, the same each time for a seed.
fn words(n: usize, seed: u32) -> Vec<String> {
    let mut x = seed;
    let mut set = std::collections::BTreeSet::new();
    while set.len() < n {
        let mut next = |m: u32| {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            (x >> 16) % m
        };
        let len = 5 + next(6);
        set.insert((0..len).map(|_| char::from(b'a' + next(26) as u8)).collect::<String>());
    }
    set.into_iter().collect()
}

/// About `len` bytes of lines, every 7th holding one of `members` (as written in the text), the
/// rest words of no list.
fn text_with(members: &[String], len: usize) -> String {
    let mut t = String::with_capacity(len + 64);
    let mut i = 0;
    while t.len() < len {
        if i % 7 == 0 {
            t.push_str(&members[(i * 31) % members.len()]);
        } else {
            t.push_str("lorem ipsum dolor sit amet");
        }
        t.push_str(if i % 3 == 0 { "\n" } else { " " });
        i += 1;
    }
    t
}

/// `f` on a thread of its own, whose caches of compiled patterns and guards are new.
fn fresh<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::spawn(f).join().unwrap_or_else(|e| std::panic::resume_unwind(e))
}

/// The repro: 5,000 hashes of 40 hex digits, whose NFA is past the 10 MiB a guard may compile and
/// which the crate searches by Aho-Corasick, were "too costly" on 68 KB and more; now every call
/// answers as the crate does (as 0.8.8 did) on texts of each length around where it changed (64 KB,
/// where a pattern's first compile is the guard's, and the 68,196 and 69,755 bytes the crate's room
/// from size gave it), whether the first call is on 1 MB or on a short text, and the repro's own
/// numbers (src/re.rs `nfa_first`, `exact_room`, `many_literals`, FEEDBACK75.md follow-up found by
/// 0.8.9's coverage, Q-1125; branch feedback75).
#[test]
fn a_list_of_5000_hashes_is_searched_on_any_text() {
    let d = dir("hashes");
    let hs = hashes(5_000, 1);
    let lines: Vec<String> = hs.iter().map(|h| format!("GET /file/{} 200", h)).collect();
    for first in [1_000_000, 10] {
        let (d, hs, lines) = (d.clone(), hs.clone(), lines.clone());
        fresh(move || {
            let p = hs.join("|");
            for len in [first, 65_535, 65_536, 65_537, 68_195, 68_196, 68_197, 69_754, 69_755, 69_756, 100_000] {
                let text = text_with(&lines, len);
                assert_eq!(same_as_crate(&d, &p, "", &text), text.matches("GET").count(), "{} bytes after {}", len, first);
            }
            std::fs::write(d.join("t.txt"), format!("GET /file/{} 200\n", hs[17]).repeat(2_000)).unwrap();
            assert_eq!(ask(&d, "let t = read_file(\"t.txt\")\nlet p = read_file(\"p.txt\")\nlet out = [len(t), re.test(t, p), len(re.find_all(t, p))]\n"), "[110000, true, 2000]");
        });
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// Lists written every way are told apart as the crate tells them, and each answers as the crate
/// does: by Aho-Corasick, on 100 KB, 3,000 and 3,001 words, domains with `\.`, words with `\/`,
/// `\x41`, `é`, `\u{1F600}` and `😀`, a list that escapes `|` or `\` and one of short
/// words (both read by parsing them), and `cat|dog|bird`; by the regex engines, past their room (5 KB),
/// 2,999 words, a list with `i`, an empty part (between two `|`, first or last), a class, a
/// repeat, a group, `\b`, or one-character parts (a class to the crate); and an alternation of
/// literals with a costly branch is still the error value on 200 KB (src/re.rs `many_literals`,
/// `crate_searches_literals`, FEEDBACK75.md follow-up found by 0.8.9's coverage, Q-1125; branch
/// feedback75).
#[test]
fn literal_lists_written_every_way_answer_as_the_crate() {
    let d = dir("lists");
    let ws = words(3_001, 9);
    let list = |n: usize, f: &dyn Fn(usize, &str) -> String| -> Vec<String> { ws[..n].iter().enumerate().map(|(i, w)| f(i, w)).collect() };
    let plain = |_: usize, w: &str| w.to_string();
    let escapes = ["\\x41", "\\u00e9", "\\u{1F600}", "\\uD83D\\uDE00", "\\/", "\\.", "\\(", "\\}"];
    let shown = ["A", "é", "😀", "😀", "/", ".", "(", "}"];
    // (pattern parts, the same as the text holds them, flags, text length)
    let cases: Vec<(Vec<String>, Vec<String>, &str, usize)> = vec![
        (list(3_000, &plain), list(3_000, &plain), "", 100_000),
        (list(3_001, &plain), list(3_001, &plain), "", 100_000),
        (list(3_000, &|_, w| format!("{}\\.com", w)), list(3_000, &|_, w| format!("{}.com", w)), "", 100_000),
        (list(3_000, &|i, w| format!("{}{}", w, escapes[i % 8])), list(3_000, &|i, w| format!("{}{}", w, shown[i % 8])), "", 100_000),
        (list(3_000, &|i, w| if i == 5 { format!("{}\\|x", w) } else { w.to_string() }), list(3_000, &|i, w| if i == 5 { format!("{}|x", w) } else { w.to_string() }), "", 100_000),
        (list(3_000, &|i, w| if i == 5 { format!("{}\\\\x", w) } else { w.to_string() }), list(3_000, &|i, w| if i == 5 { format!("{}\\x", w) } else { w.to_string() }), "", 100_000),
        (list(3_000, &|_, w| w[..3].to_string() + &w[w.len() - 1..]), list(3_000, &|_, w| w[..3].to_string() + &w[w.len() - 1..]), "", 100_000),
        (vec!["cat".into(), "dog".into(), "bird".into()], vec!["cat".into(), "dog".into(), "bird".into()], "", 100_000),
        (list(2_999, &plain), list(2_999, &plain), "", 5_000),
        (list(3_000, &plain), list(3_000, &|_, w| w.to_uppercase()), "i", 5_000),
        (list(3_000, &|i, w| if i == 5 { String::new() } else { w.to_string() }), list(3_000, &plain), "", 5_000),
        (list(3_000, &|i, w| if i == 0 { format!("|{}", w) } else { w.to_string() }), list(3_000, &plain), "", 5_000),
        (list(3_000, &|i, w| if i == 2_999 { format!("{}|", w) } else { w.to_string() }), list(3_000, &plain), "", 5_000),
        (list(3_000, &|i, w| if i == 5 { format!("{}[xy]", w) } else { w.to_string() }), list(3_000, &|i, w| if i == 5 { format!("{}y", w) } else { w.to_string() }), "", 5_000),
        (list(3_000, &|i, w| if i == 5 { format!("{}x*", w) } else { w.to_string() }), list(3_000, &plain), "", 5_000),
        (list(3_000, &|i, w| if i == 0 { format!("({}", w) } else if i == 2_999 { format!("{})", w) } else { w.to_string() }), list(3_000, &plain), "", 5_000),
        (list(3_000, &|i, w| if i == 0 { format!("\\b(?:{}", w) } else if i == 2_999 { format!("{})\\b", w) } else { w.to_string() }), list(3_000, &plain), "", 5_000),
        (list(3_000, &|i, w| if i == 5 { format!("{}\\d", w) } else { w.to_string() }), list(3_000, &|i, w| if i == 5 { format!("{}7", w) } else { w.to_string() }), "", 5_000),
        ((0..3_000).map(|i| char::from_u32(0x4E00 + 2 * i).unwrap().to_string()).collect(), (0..3_000).map(|i| char::from_u32(0x4E00 + 2 * i).unwrap().to_string()).collect(), "", 100_000),
    ];
    for (parts, members, flags, len) in cases {
        let d = d.clone();
        fresh(move || {
            let p = parts.join("|");
            let n = same_as_crate(&d, &p, flags, &text_with(&members, len));
            assert!(n > 0, "{:.60}: no match", p);
        });
    }
    // Read by parsing it, where its first call is on a short text too: the list that escapes `|`.
    let (d2, parts) = (d.clone(), list(3_000, &|i, w| if i == 5 { format!("{}\\|x", w) } else { w.to_string() }));
    fresh(move || {
        let p = parts.join("|");
        same_as_crate(&d2, &p, "", &text_with(&parts, 10));
        assert!(same_as_crate(&d2, &p, "", &text_with(&parts, 100_000)) > 100);
    });
    std::fs::write(d.join("t.txt"), mixed_text(200_000, "ab", 41)).unwrap();
    let got = ask(&d, "let out = re.find(read_file(\"t.txt\"), \"cat|dog|[ab]*a[ab]{900}c\")\n");
    assert!(got.starts_with("err(") && got.contains(&format!("`re.find`: {}", COSTLY)), "{}", got);
    let _ = std::fs::remove_dir_all(&d);
}

/// A pattern the crate refuses is "the pattern is too big" whatever came first, as in 0.8.8:
/// `(?:\S{1000}){5}`, whose NFA forward (5.5 MB) a guard could compile but whose NFA back is past
/// the 10 MiB the crate allows, is that error on 70 KB and on 3 bytes, the long text first or the
/// short, and `re.valid` says false after either; `(?:\S{1000}){4}`, whose NFA back is 6.4 MB, is
/// the crate's and the guard's, and answers on both (src/re.rs `nfa_first`, FEEDBACK75.md follow-up
/// found by 0.8.9's coverage, Q-1126; branch feedback75).
#[test]
fn a_pattern_the_crate_refuses_is_too_big_whatever_came_first() {
    let d = dir("refused");
    std::fs::write(d.join("long.txt"), " ".repeat(70_000)).unwrap();
    std::fs::write(d.join("short.txt"), "abc").unwrap();
    for (p, refused) in [(r"(?:\S{1000}){5}", true), (r"(?:\S{1000}){4}", false)] {
        for order in [["long.txt", "short.txt"], ["short.txt", "long.txt"]] {
            let d = d.clone();
            fresh(move || {
                // From data: `quillo check` refuses the first written out.
                std::fs::write(d.join("p.txt"), p).unwrap();
                for (k, file) in order.into_iter().enumerate() {
                    for call in if k == 0 { &["re.test", "re.find"][..] } else { &["re.test"][..] } {
                        let got = try_ask(&d, &format!("let out = {}(read_file({:?}), read_file(\"p.txt\"))\n", call, file));
                        if refused {
                            let e = got.expect_err(call);
                            assert!(e.contains(&format!("`{}`: the pattern is too big: it compiles to more than the size limit of 10 MB", call)), "{} {:?} on {}: {}", p, order, file, e);
                        } else {
                            assert_eq!(got.unwrap(), if *call == "re.test" { "false" } else { "null" }, "{} {:?} on {}", p, order, file);
                        }
                    }
                    assert_eq!(ask(&d, "let out = re.valid(read_file(\"p.txt\"))\n"), (!refused).to_string(), "{} {:?} after {}", p, order, file);
                }
            });
        }
    }
    let _ = std::fs::remove_dir_all(&d);
}

// ---- Q-1125, review: a literal search past its room, and the verdicts ------------------------------

/// The test thread's own CPU time.
fn thread_cpu() -> Duration {
    let mut t = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: `t` lives through the call, which only writes it.
    unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut t) };
    Duration::new(t.tv_sec as u64, t.tv_nsec as u32)
}

/// The literal lists of the review: one of 3,000 with a literal of 20,001 bytes and `a`, and one of
/// 64 literals of 100 bytes that share 98 (with `a`, or not).
fn review_lists() -> (String, String, String) {
    let mut parts = vec!["a".repeat(20_000) + "b", "a".to_string()];
    parts.extend(words(2_998, 7));
    let cs: Vec<char> = "bcdefghijklmnopqrstuvwxyzBCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-".chars().collect();
    let long: Vec<String> = cs.iter().map(|c| format!("{}{}b", "a".repeat(98), c)).collect();
    (parts.join("|"), long.join("|") + "|a|zz", long.join("|") + "|zz")
}

/// What `many_literals` (from the text) and `crate_searches_literals` (from the parsed pattern) say,
/// through `quillo::re::literal_verdicts`: lists of 3,000 literals or more, plain or with the
/// escapes the text check reads, by both (the text's longest part at least the literal's); lists
/// that escape `|` or `\`, of short words, exact prefixes (`cat|dog|bird`, `\d`, `[;{}]`, one literal
/// of 100 bytes, 64 of 100) by the parse alone, each with its longest literal and factor; and none
/// at 2,999 literals, with `i`, an empty part, a class, a repeat, a group, `\b`, `\d` in a part, a
/// costly branch, one-character parts or one literal of 101 bytes; and wherever the text says yes,
/// the parse says yes (src/re.rs `many_literals`, `crate_searches_literals`, FEEDBACK75.md follow-up
/// found by 0.8.9's coverage, Q-1125, review; coverage of 0.8.9; branch feedback75).
#[test]
fn the_literal_verdicts_are_the_crates() {
    let ws = words(3_001, 9);
    let longest = |n: usize| ws[..n].iter().map(String::len).max().unwrap();
    let list = |n: usize, f: &dyn Fn(usize, &str) -> String| ws[..n].iter().enumerate().map(|(i, w)| f(i, w)).collect::<Vec<String>>().join("|");
    let (ac, teddy, _) = review_lists();
    let cjk: String = (0..3_000).map(|i| char::from_u32(0x4E00 + 2 * i).unwrap().to_string()).collect::<Vec<_>>().join("|");
    // (pattern, flags, by its text, by its parse: the longest literal and the factor)
    let cases: Vec<(String, &str, bool, Option<(usize, usize)>)> = vec![
        (list(3_000, &|_, w| w.to_string()), "", true, Some((longest(3_000), 1))),
        (list(3_001, &|_, w| w.to_string()), "m", true, Some((longest(3_001), 1))),
        (list(3_000, &|_, w| format!("{}\\.com", w)), "s", true, Some((longest(3_000) + 4, 1))),
        (list(3_000, &|i, w| if i == 0 { format!("{}\\u{{1F600}}\\x41\\uD83D\\uDE00\\/", w) } else { w.to_string() }), "", true, Some((longest(3_000).max(ws[0].len() + 10), 1))),
        (ac.clone(), "", true, Some((20_001, 1))),
        (list(3_000, &|i, w| if i == 5 { format!("{}\\|x", w) } else { w.to_string() }), "", false, Some((longest(3_000).max(ws[5].len() + 2), 1))),
        (list(3_000, &|i, w| if i == 5 { format!("{}\\\\x", w) } else { w.to_string() }), "", false, Some((longest(3_000).max(ws[5].len() + 2), 1))),
        (list(3_000, &|_, w| w[..3].to_string() + &w[w.len() - 1..]), "", false, Some((4, 1))),
        ("cat|dog|bird".to_string(), "", false, Some((4, 3))),
        ("\\d".to_string(), "", false, Some((1, 1))),
        ("[;{}]".to_string(), "", false, Some((1, 1))),
        ("x".repeat(100), "", false, Some((100, 1))),
        (teddy.clone(), "", false, Some((100, 64))),
        ("x".repeat(101), "", false, None),
        (list(2_999, &|_, w| w.to_string()), "", false, None),
        (list(3_000, &|_, w| w.to_string()), "i", false, None),
        (list(3_000, &|i, w| if i == 5 { String::new() } else { w.to_string() }), "", false, None),
        (list(3_000, &|i, w| if i == 5 { format!("{}[xy]", w) } else { w.to_string() }), "", false, None),
        (list(3_000, &|i, w| if i == 5 { format!("{}x*", w) } else { w.to_string() }), "", false, None),
        (format!("({})", list(3_000, &|_, w| w.to_string())), "", false, None),
        (format!("\\b(?:{})\\b", list(3_000, &|_, w| w.to_string())), "", false, None),
        (list(3_000, &|i, w| if i == 5 { format!("{}\\d", w) } else { w.to_string() }), "", false, None),
        (list(2_999, &|_, w| w.to_string()) + "|[ab]*a[ab]{900}c", "", false, None),
        (cjk, "", false, None),
    ];
    for (p, flags, by_text, by_parse) in cases {
        let (text, parse) = quillo::re::literal_verdicts(&p, flags);
        assert_eq!(text.is_some(), by_text, "{:.60} ({:?}): {:?}", p, flags, text);
        assert_eq!(parse, by_parse, "{:.60} ({:?})", p, flags);
        if let Some(t) = text {
            assert!(parse.is_some_and(|(l, _)| l <= t), "{:.60}: {} {:?}", p, t, parse);
        }
    }
}

/// The review's two cases, past their room, give the error value, where the crate unclocked took 8
/// and 4.5 s (the second's searches, in a debug build, within its 20 times longer clock, may answer): a list of 3,000 with a literal of 20,001 bytes and `a`, on 200 KB of `a`, through
/// `re.replace`, `re.find_all` and `re.split` (each search reading the long one through after each
/// `a`); 64 literals of 100 bytes that share 98, with `a`, through `re.replace` on 1 MB, and without
/// it through `re.test` and `re.find`; and within their room (1 KB) each answers (src/re.rs
/// `windowed_matches`, `windowed_test`, `Room::literal`, FEEDBACK75.md follow-up found by 0.8.9's
/// coverage, Q-1125, review; coverage of 0.8.9; branch feedback75).
#[test]
fn a_costly_literal_search_past_its_room_gives_its_error() {
    let d = dir("lit-costly");
    let (ac, teddy_a, teddy) = review_lists();
    std::fs::write(d.join("a200k.txt"), "a".repeat(200_000)).unwrap();
    std::fs::write(d.join("a1m.txt"), "a".repeat(1_000_000)).unwrap();
    std::fs::write(d.join("a1k.txt"), "a".repeat(1_000)).unwrap();
    // (pattern, text, calls with what each answers where its search is within the time, which the
    // second list's is in a debug build: the clock there gives 20 times as long, and the crate's own
    // search of these literals is not that much slower there)
    for (p, file, calls, slow) in [
        (&ac, "a200k.txt", &[("re.replace", ", \"\"", "\"\""), ("re.find_all", "", ""), ("re.split", "", "")][..], true),
        (&teddy_a, "a1m.txt", &[("re.replace", ", \"\"", "\"\"")][..], false),
        (&teddy, "a1m.txt", &[("re.test", "", "false"), ("re.find", "", "null")][..], false),
    ] {
        std::fs::write(d.join("p.txt"), p).unwrap();
        for (f, rest, answer) in calls {
            let got = ask(&d, &format!("let out = {}(read_file({:?}), read_file(\"p.txt\"){})\n", f, file, rest));
            let costly = got.starts_with("err(") && got.contains(&format!("`{}`: {}", f, COSTLY));
            assert!(costly || (cfg!(debug_assertions) && !slow && got == *answer), "{} {}: {:.200}", f, file, got);
        }
    }
    // Within their room, the crate's answers.
    for (p, want) in [(&ac, "[\"\", 1000, 0]"), (&teddy_a, "[\"\", 1000, 0]"), (&teddy, "[\"aaaaaaaaaa\", 0, 0]")] {
        std::fs::write(d.join("p.txt"), p).unwrap();
        let got = ask(&d, "let t = read_file(\"a1k.txt\")\nlet p = read_file(\"p.txt\")\nlet out = [slice(re.replace(t, p, \"\"), 0, 10), len(re.find_all(t, p)), len(re.split(t, p)) - 1 - len(re.find_all(t, p))]\n");
        assert_eq!(got, want, "{:.40}", p);
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// A literal pattern past its room is searched in windows and finds what the crate finds: 5,000
/// hashes (room about 1.2 MB) on 1.5 MB with a hash across each 64 KB boundary between characters
/// of 2 and 3 bytes, and one found only in the last window by `re.test`, or none; `cat|dog|bird`
/// (room about 3.3 MB) on 4 MB; a list whose literal of 5,000 bytes starts just before a window's
/// first 64 KB end; and one where it starts just past them, after 64 KB with no match, cut off by
/// the window, which `qq` after it in the list would answer (src/re.rs `windowed_matches`, `windowed_test`, `window_end`, `next_window`, FEEDBACK75.md
/// follow-up found by 0.8.9's coverage, Q-1125, review; coverage of 0.8.9; branch feedback75).
#[test]
fn a_literal_search_in_windows_finds_what_the_crate_finds() {
    let d = dir("windows");
    let hs = hashes(5_000, 3);
    let p = hs.join("|");
    let filler = |n: usize| "é☕x".repeat(n / 6 + 1);
    // A hash across each 64 KB boundary, the rest characters of 2 and 3 bytes.
    let mut text = String::new();
    let mut k: usize = 1;
    while text.len() < 1_500_000 {
        let at = k * 65_536 - 20;
        let mut f = filler(at.saturating_sub(text.len()));
        while text.len() + f.len() > at {
            f.pop();
        }
        text.push_str(&f);
        text.push_str(&hs[k * 7 % hs.len()]);
        k += 1;
    }
    assert_eq!(same_as_crate(&d, &p, "", &text), k - 1);
    let last = filler(1_500_000) + &hs[42];
    assert_eq!(same_as_crate(&d, &p, "", &last), 1);
    assert_eq!(same_as_crate(&d, &p, "", &filler(1_500_000)), 0);
    let mut cats = String::new();
    while cats.len() < 4_000_000 {
        cats.push_str(&"lorem ipsum é ".repeat(70));
        cats.push_str(["cat ", "dog ", "bird "][cats.len() % 3]);
    }
    assert!(same_as_crate(&d, "cat|dog|bird", "", &cats) > 3_000);
    let mut parts = vec!["q".repeat(5_000)];
    parts.extend(words(2_999, 5));
    let long = "x".repeat(65_000) + &"q".repeat(5_000) + &"x".repeat(200_000);
    assert_eq!(same_as_crate(&d, &parts.join("|"), "", &long), 1);
    // After 64 KB and 10 bytes with no match, `q`×5,000, which the list has before `qq`: the first
    // window ends 4,999 bytes past its first 64 KB, where the long one is cut off and `qq` would be the
    // answer; it starts past those 64 KB, so the next window is searched, where it is whole.
    let mut seam = vec!["q".repeat(5_000), "qq".to_string()];
    seam.extend(words(2_998, 11));
    let text = "x".repeat(65_546) + &"q".repeat(5_000) + &"x".repeat(30_000);
    let found = crate_matches(&seam.join("|"), "", &text);
    assert_eq!(found.iter().map(|(a, b, _)| (*a, *b)).collect::<Vec<_>>(), [(65_546, 70_546)]);
    assert_eq!(same_as_crate(&d, &seam.join("|"), "", &text), 1);
    let _ = std::fs::remove_dir_all(&d);
}

/// A list of 10,000 words on 10 MB is searched at the crate's own speed: `re.find` of the one word
/// at the end takes at most 3 times the thread time of the regex crate's own search of the same
/// pattern (by the guard, before Q-1125, many times that), the best of 3 each, after a first call
/// that compiles it (src/re.rs `nfa_first`, `Room::literal`, `windowed_matches`, FEEDBACK75.md
/// follow-up found by 0.8.9's coverage, Q-1125, review; coverage of 0.8.9; branch feedback75).
#[test]
fn a_word_list_is_searched_at_the_crates_speed() {
    let d = dir("speed");
    let ws = words(10_000, 3);
    let p = ws.join("|");
    let text = "lorem ipsum dolor sit amet ".repeat(10_000_000 / 27) + &ws[123];
    std::fs::write(d.join("p.txt"), &p).unwrap();
    std::fs::write(d.join("t.txt"), &text).unwrap();
    let src = "let out = re.find(read_file(\"t.txt\"), read_file(\"p.txt\")).text\n";
    assert_eq!(ask(&d, src), format!("{:?}", ws[123]));
    let best = |f: &dyn Fn()| {
        (0..3)
            .map(|_| {
                let t = thread_cpu();
                f();
                thread_cpu() - t
            })
            .min()
            .unwrap()
    };
    let quillo = best(&|| assert_eq!(ask(&d, src), format!("{:?}", ws[123])));
    let r = quillo::re::compile(&p, "").unwrap();
    let crate_own = best(&|| assert_eq!(r.find(&text).map(|m| m.as_str()), Some(ws[123].as_str())));
    assert!(quillo < crate_own * 3, "Quillo {:?}, the crate {:?}", quillo, crate_own);
    let _ = std::fs::remove_dir_all(&d);
}
