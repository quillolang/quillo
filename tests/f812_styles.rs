//! FEEDBACK71.md's Q-991 (Quillo 0.8.12; branch feedback86-89): a large `public/*.css` made
//! `quillo check` and the language server take tens of seconds, each class's line counted from the
//! sheet's start (1 MiB: 29 s). The classes a sheet styles and their lines are found in one pass
//! now, and nested at-rules are read without the stack: 120,000 `@media x{` (1 MiB) overflowed it.
//! Times are compared as ratios of one size to another, never against fixed seconds.

mod support;

use quillo::styles::classes_in;
use std::time::{Duration, Instant};

/// The classes with their lines, counted from 0 as `classes_in` gives them.
fn at(css: &str) -> Vec<(String, u32)> {
    classes_in(css)
}

fn c(name: &str, line: u32) -> (String, u32) {
    (name.to_string(), line)
}

/// The best of `n` runs of `f`.
fn best(n: usize, f: impl Fn()) -> Duration {
    (0..n)
        .map(|_| {
            let t = Instant::now();
            f();
            t.elapsed()
        })
        .min()
        .unwrap()
}

/// How much longer a sheet eight times as large takes: about 8 when linear, 64 when quadratic.
fn growth(small: &str, large: &str) -> f64 {
    // Once each first, so neither pays for the allocator's first growth.
    classes_in(small);
    classes_in(large);
    let s = best(5, || {
        classes_in(small);
    });
    let l = best(3, || {
        classes_in(large);
    });
    l.as_secs_f64() / s.as_secs_f64().max(1e-9)
}

#[test]
fn an_empty_sheet_styles_nothing() {
    assert_eq!(at(""), vec![]);
    assert_eq!(at("\n\n\r\n"), vec![]);
    assert_eq!(at("/* .a { } */"), vec![]);
}

#[test]
fn the_first_and_the_last_line() {
    assert_eq!(at(".first { }"), vec![c("first", 0)]);
    assert_eq!(at(".first { }\n.mid { }\n.last { }\n"), vec![c("first", 0), c("mid", 1), c("last", 2)]);
    // A sheet with no newline at its end: the last line is still counted.
    assert_eq!(at(".first { }\n.mid { }\n.last { }"), vec![c("first", 0), c("mid", 1), c("last", 2)]);
    // A class right after a newline is on the next line; one right before it, on its own.
    assert_eq!(at("\n.a{}\n\n.b{}"), vec![c("a", 1), c("b", 3)]);
    assert_eq!(at("x,.a\n{}"), vec![c("a", 0)]);
}

#[test]
fn crlf_lines_count_once_and_a_lone_cr_not_at_all() {
    assert_eq!(at(".a { }\r\n.b { }\r\n\r\n.c { }\r\n"), vec![c("a", 0), c("b", 1), c("c", 3)]);
    assert_eq!(at(".a { }\r\n.b { }\r\n\r\n.c { }"), vec![c("a", 0), c("b", 1), c("c", 3)]);
    // As 0.8.11: a newline is `\n`.
    assert_eq!(at(".a { }\r.b { }\n.c { }"), vec![c("a", 0), c("b", 0), c("c", 1)]);
    assert_eq!(at("/* x\r\ny */\r\n.a { }"), vec![c("a", 2)]);
}

#[test]
fn multibyte_characters_before_a_class_keep_its_line() {
    // In a comment (blanked to one space a character), in a string, and in a name.
    assert_eq!(at("/* é日本🙂 */ .a { }\n/* 日\n本 */\n.b { }"), vec![c("a", 0), c("b", 3)]);
    assert_eq!(at("a[title=\"日本語 .no\"] .x { }\n.ü { }\n🙂 .y { }"), vec![c("x", 0), c("ü", 1), c("y", 2)]);
    assert_eq!(at(".日本 { content: \"🙂\" }\n.é-x { }"), vec![c("日本", 0), c("é-x", 1)]);
    // Many of them, so a byte offset taken for a character would land lines away.
    let css = format!("/* {} */\n{}\n.after {{ }}\n", "日本語\n".repeat(500), "🙂".repeat(5000));
    assert_eq!(at(&css), vec![c("after", 502)]);
}

#[test]
fn a_very_long_line() {
    let line: String = (0..40_000).map(|i| format!(".c{} {{ x: 1 }} ", i)).collect();
    assert!(line.len() > 500_000);
    let css = format!("{}\n.next {{ }}\n{}", line, line);
    let got = at(&css);
    assert_eq!(got.len(), 80_001);
    assert!(got[..40_000].iter().all(|(_, l)| *l == 0));
    assert_eq!(got[40_000], c("next", 1));
    assert!(got[40_001..].iter().all(|(_, l)| *l == 2));
    assert_eq!((got[0].0.as_str(), got[80_000].0.as_str()), ("c0", "c39999"));
}

#[test]
fn every_class_is_on_the_line_it_names() {
    // CRLF and LF lines, comments over lines, multibyte text, nested at-rules, globals, and a
    // last line with no newline: each class found is written on the line it is said to be on.
    let (mut css, mut want) = (String::new(), 1);
    for i in 0..3_000 {
        // The classes each kind of rule styles.
        want += [1, 1, 1, 2, 1, 0, 1][i % 7];
        let nl = if i % 3 == 0 { "\r\n" } else { "\n" };
        match i % 7 {
            0 => css.push_str(&format!(".k{} {{ a: \"日本\" }}{}", i, nl)),
            1 => css.push_str(&format!("/* {}\n🙂 */ .k{} {{ }}{}", i, i, nl)),
            2 => css.push_str(&format!("@media (x) {{{} .k{} {{ }}{}}}{}", nl, i, nl, nl)),
            3 => css.push_str(&format!(":global(.g{}) .k{}, é .k{}x {{ }}{}", i, i, i, nl)),
            4 => css.push_str(&format!("@supports (y) {{ @media (z) {{{}.k{} {{ }} }} }}{}", nl, i, nl)),
            5 => css.push_str(&format!("@keyframes k{} {{ from {{ a: 0 }} }}{}", i, nl)),
            _ => css.push_str(&format!("a[title=\"{{\"] .k{} {{ }}{}", i, nl)),
        }
    }
    css.push_str(".end { }");
    let lines: Vec<&str> = css.lines().collect();
    let got = at(&css);
    assert_eq!(got.len(), want, "{:?}", &got[..10]);
    for (name, line) in &got {
        let text = lines[*line as usize];
        assert!(text.contains(&format!(".{} ", name)) || text.contains(&format!(".{},", name)), ".{} said on line {}: {:?}", name, line, text);
    }
    assert_eq!(got.last().unwrap(), &c("end", lines.len() as u32 - 1));
}

#[test]
fn globals_are_left_out_wherever_they_stand() {
    assert_eq!(at(":global(.a) .b :global(.c .d) .e, .f:global(.g) { }"), vec![c("b", 0), c("e", 0), c("f", 0)]);
    // A selector with no block styles nothing.
    assert_eq!(at(":global(.a) .b"), vec![]);
    assert_eq!(at(":global(.a).b { }"), vec![c("b", 0)]);
    assert_eq!(at(":global(.a:not(.b)) .c { }"), vec![c("c", 0)]);
    // Not closed: its `(` takes the rest of the sheet, so no rule is found, as in 0.8.11.
    assert_eq!(at(".a :global(.b .c { }\n.d { }"), vec![]);
    let wide: String = (0..2_000).map(|i| format!(":global(.g{}) .l{} ", i, i)).collect::<String>() + "{ }";
    let got = at(&wide);
    assert_eq!(got.len(), 2_000);
    assert!(got.iter().enumerate().all(|(i, (n, l))| *n == format!("l{}", i) && *l == 0));
}

#[test]
fn nested_at_rules_are_read_at_any_depth() {
    assert_eq!(at("@media a {\n@supports b {\n.x { }\n}\n}\n.y { }"), vec![c("x", 2), c("y", 5)]);
    // 120,000 levels (1 MiB) overflowed the stack, ending check and the language server.
    let n = 120_000;
    let css = "@media x{\n".repeat(n) + ".deep{}" + &"}".repeat(n) + "\n.out{}";
    assert_eq!(at(&css), vec![c("deep", n as u32), c("out", n as u32 + 1)]);
    // Not closed at all.
    let css = "@media x{\n".repeat(n) + ".open{";
    assert_eq!(at(&css), vec![c("open", n as u32)]);
}

#[test]
fn a_large_sheets_classes_take_linear_time() {
    // Eight times the rules: about 8 times the time (0.8.11: about 64 times).
    let sheet = |n: usize| (0..n).map(|i| format!(".c{} {{ x: 1 }}\n/* note */\n", i)).collect::<String>();
    let (small, large) = (sheet(6_000), sheet(48_000));
    let g = growth(&small, &large);
    assert!(g < 24.0, "eight times the rules took {:.1} times as long", g);
}

#[test]
fn a_selector_of_many_globals_takes_linear_time() {
    let wide = |n: usize| (0..n).map(|i| format!(":global(.g{}) .l{} ", i, i)).collect::<String>() + "{ }";
    let (small, large) = (wide(3_000), wide(24_000));
    let g = growth(&small, &large);
    assert!(g < 24.0, "eight times the globals took {:.1} times as long", g);
}

#[test]
fn deeply_nested_at_rules_take_linear_time() {
    let nest = |n: usize| "@media x{".repeat(n) + ".a{}" + &"}".repeat(n);
    let (small, large) = (nest(2_000), nest(16_000));
    let g = growth(&small, &large);
    assert!(g < 24.0, "eight times the depth took {:.1} times as long", g);
}

// ---- through `quillo check` ----------------------------------------------------------

mod check {
    use super::support::temp_dir;
    use quillo::check::{check_file_opts, Options, Severity};
    use std::path::PathBuf;
    use std::time::Instant;

    struct App(PathBuf);

    impl App {
        fn new(name: &str, sheet: &str) -> App {
            let d = temp_dir(&format!("f812-styles-{}", name));
            std::fs::create_dir_all(d.join("public")).unwrap();
            // A route that answers a page: the sheets under public/ are then the program's.
            std::fs::write(d.join("main.qwe"), "route GET / = <p.used>x</p>\n").unwrap();
            std::fs::write(d.join("public/big.css"), sheet).unwrap();
            App(d.canonicalize().unwrap())
        }

        /// The css warnings, as `public/big.css:LINE .class` (a class nothing writes) or
        /// `public/big.css:LINE outline: none` (a focus ring taken away).
        fn css(&self) -> Vec<String> {
            let dir = format!("{}/", self.0.display());
            check_file_opts(&self.0.join("main.qwe"), Default::default(), Options { css: true, ..Default::default() })
                .diagnostics
                .iter()
                .filter(|d| d.severity == Severity::Warning && (d.message.ends_with("is styled here, but nothing in the program writes it") || d.message.starts_with("`outline: none` on `:focus`")))
                .map(|d| format!("{}:{} {}", d.file.replace(&dir, ""), d.line, d.message.split('`').nth(1).unwrap_or("")))
                .collect()
        }
    }

    impl Drop for App {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn sheet(n: usize) -> String {
        (0..n).map(|i| format!(".c{} {{ color: red }}\r\n/* é */\n", i)).collect()
    }

    /// The issue's case: a public sheet of 1 MiB and more, each class warned about on its line,
    /// and eight times the sheet about eight times the check (0.8.11: 1 MiB 29 s, 2 MiB 50 s).
    #[test]
    fn a_large_public_sheet_is_checked_in_linear_time() {
        let small = App::new("small", &sheet(5_000));
        let large = App::new("large", &sheet(40_000));
        assert!(std::fs::metadata(large.0.join("public/big.css")).unwrap().len() > 1 << 20);
        let got = large.css();
        assert_eq!(got.len(), 40_000);
        assert_eq!(got[0], "public/big.css:1 .c0");
        assert_eq!(got[1], "public/big.css:3 .c1");
        assert_eq!(got[39_999], "public/big.css:79999 .c39999");
        let time = |a: &App| {
            (0..3)
                .map(|_| {
                    let t = Instant::now();
                    a.css();
                    t.elapsed()
                })
                .min()
                .unwrap()
                .as_secs_f64()
        };
        let (s, l) = (time(&small), time(&large));
        assert!(l / s.max(1e-9) < 24.0, "eight times the sheet took {:.1} times as long ({:.3} s, {:.3} s)", l / s, s, l);
    }

    /// A sheet of 120,000 nested `@media` (1 MiB) is checked, not a stack overflow.
    #[test]
    fn a_public_sheet_nested_too_deep_for_the_stack_is_checked() {
        let n = 120_000;
        let app = App::new("deep", &("@media x{\n".repeat(n) + ".deep{}" + &"}".repeat(n) + "\n.last{}"));
        assert_eq!(app.css(), [format!("public/big.css:{} .deep", n + 1), format!("public/big.css:{} .last", n + 2)]);
    }

    /// A sheet that takes the focus ring away is read for its rules' lines in one pass too: each
    /// rule's line was counted from the sheet's start, 2.9 s for a sheet of 2 MiB.
    #[test]
    fn a_large_public_sheet_that_takes_the_focus_ring_away_is_checked_in_linear_time() {
        let with_focus = |n: usize| sheet(n) + "a:focus { outline: none }\n";
        let small = App::new("focus-small", &with_focus(5_000));
        let large = App::new("focus-large", &with_focus(40_000));
        let got = large.css();
        assert_eq!(got.len(), 40_001);
        assert_eq!(got.iter().filter(|w| w.ends_with(" outline: none")).collect::<Vec<_>>(), ["public/big.css:80001 outline: none"]);
        let time = |a: &App| {
            (0..3)
                .map(|_| {
                    let t = Instant::now();
                    a.css();
                    t.elapsed()
                })
                .min()
                .unwrap()
                .as_secs_f64()
        };
        let (s, l) = (time(&small), time(&large));
        assert!(l / s.max(1e-9) < 24.0, "eight times the sheet took {:.1} times as long ({:.3} s, {:.3} s)", l / s, s, l);
    }
}
