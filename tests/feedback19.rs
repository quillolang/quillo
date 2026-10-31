//! FEEDBACK19.md (branch feedback19-fixes): what a JavaScript habit meets in
//! Quillo. `||` before text or a number whose type is not known (Q-150),
//! `filled` (Q-151), `break` and `continue` (Q-152), and bits and hex
//! (Q-153), with the bits checked against the browser's own answers.

use quillo::check::{check_source, Severity};
use std::path::Path;
use std::process::Command;

fn diags(src: &str) -> Vec<String> {
    check_source(src, "t.qwe", Path::new("."))
        .diagnostics
        .iter()
        .map(|d| format!("{}{} {}", d.line, if d.severity == Severity::Warning { " warning:" } else { "" }, d.message))
        .collect()
}

/// Exactly one diagnostic, containing `want`.
fn one(src: &str, want: &str) {
    let d = diags(src);
    assert!(d.len() == 1 && d[0].contains(want), "expected one diagnostic containing {:?}, got {:#?}\n--- source ---\n{}", want, d, src);
}

fn clean(src: &str) {
    let d = diags(src);
    assert!(d.is_empty(), "expected no diagnostics, got {:#?}\n--- source ---\n{}", d, src);
}

fn node_available() -> bool {
    Command::new("node").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

/// `body` run on the server, as a function's body: what it gives, or
/// "ERROR: " and the message.
fn server(body: &str) -> (quillo::Interp, String) {
    let src = format!("fn __t() {{\n{}\n}}\n", body);
    let mut it = quillo::Interp::new(".");
    it.load_source(&src, "case.qwe").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    let out = match it.call(&quillo::value::lookup(&it.globals, "__t").unwrap(), vec![], 0) {
        Ok(v) => v.repr(),
        Err(quillo::interp::Ctrl::Error(e)) => format!("ERROR: {}", e.msg),
        Err(_) => "left the function".into(),
    };
    (it, out)
}

/// The same answer on the server and in the browser's runtime, and it is `want`.
fn same(body: &str, want: &str) {
    let (it, on_server) = server(body);
    assert_eq!(on_server, want, "server: {}", body);
    if node_available() {
        let js = quillo::client::test_script(&it, "__t").unwrap_or_else(|e| panic!("{}\n{}", e, body));
        let out = Command::new("node").arg("-e").arg(&js).output().expect("node");
        assert!(out.status.success(), "node failed: {}\n{}", String::from_utf8_lossy(&out.stderr), js);
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), want, "browser: {}", body);
    }
}

/// The source does not load, with this message.
fn refused(src: &str, want: &str) {
    match quillo::eval_str(src) {
        Ok(v) => panic!("{} gave {}", src, v.repr()),
        Err(e) => assert_eq!(e.msg, want, "{}", src),
    }
}

/// The smallest and the largest int; the smallest has no literal of its own.
const MIN: &str = "(-9223372036854775807 - 1)";
const MAX: &str = "9223372036854775807";

// ---- `||` before a value of no known type (Q-150) ----------------------------------------------

#[test]
fn text_for_a_fallback_says_the_value_was_meant_as_text() {
    // Where blank values come from: nothing says these are strings.
    one("route GET / = request.query.name || \"anonymous\"", "1 warning: `request.query.name` counts as true even when it is \"\"");
    one("route POST / = request.json.title || \"untitled\"", "`request.json.title` counts as true");
    one("fn f(row) = row.name || \"none\"", "`row.name` counts as true");
    one("fn f(name) = name || \"anonymous\"", "`name` counts as true");
    one("fn f(a, b) = a || `hi ${b}`", "`a` counts as true");
    // What it says to write replaces a blank and a missing value alike.
    one("fn f(name) = name || \"anonymous\"", "unless it is null: write `filled(name) ?? ...`, or `name ?? ...` to replace only null");
}

#[test]
fn a_number_for_a_fallback_says_the_value_was_meant_as_a_number() {
    one("fn f(row) = row.n || 10", "`row.n` counts as true even when it is 0");
    one("fn f(row) = row.n || 10", "write `row.n != null && row.n != 0 ? row.n : ...`, or `row.n ?? ...`");
    one("fn f(row) = row.n || 1.5", "`row.n` counts as true even when it is 0");
    one("fn f(row) = row.n || 2.50m", "`row.n` counts as true even when it is 0");
}

#[test]
fn a_fallback_that_says_nothing_is_not_warned_about() {
    // `""` and `0` give what JavaScript gives, whatever the value was.
    clean("fn f(x) = x || \"\"");
    clean("fn f(x) = x || 0");
    clean("fn f(x) = x || 0.0");
    clean("fn f(x) = x || 0m");
    // Nor do these say what `x` is meant to be.
    clean("fn f(x) = x || []");
    clean("fn f(x) = x || {}");
    clean("fn f(x) = x || null");
    clean("fn f(x, y) = x || y");
    clean("fn f(x) = x || true");
    // A value of a known type is judged by its type, as before.
    clean("fn f(a: int, b: int) = a == b || \"different\"");
    clean("fn f(ok: bool) = ok || \"no\"");
    // `??` and an explicit test are what the warning asks for.
    clean("fn f(x) = x ?? \"anonymous\"");
    clean("fn f(x) = filled(x) ?? \"anonymous\"");
}

#[test]
fn only_or_is_read_this_way() {
    clean("fn f(x) = x && \"yes\"");
    clean("fn f(x) = !x");
    clean("fn f(x) = x ? \"yes\" : \"no\"");
}

#[test]
fn what_is_not_a_chain_of_names_is_called_this_value() {
    one("fn g() = { a: 1 }\nfn f(xs) = xs[0] || \"none\"", "this value counts as true");
    one("fn f(g) = g().name || \"none\"", "this value counts as true");
    one("fn f(a) = a.b.c.d || \"none\"", "`a.b.c.d` counts as true");
}

// ---- `filled` (Q-151) --------------------------------------------------------------------------

#[test]
fn filled_makes_blank_text_null() {
    same("filled(\"\")", "null");
    same("filled(\"a\")", "\"a\"");
    same("filled(null)", "null");
    same("filled(\"\") ?? \"anonymous\"", "\"anonymous\"");
    same("filled(null) ?? \"anonymous\"", "\"anonymous\"");
    same("filled(\"Ada\") ?? \"anonymous\"", "\"Ada\"");
    same("\"\" | filled ?? \"piped\"", "\"piped\"");
}

#[test]
fn filled_leaves_everything_else_as_it_is() {
    same("filled(0)", "0");
    same("filled(false)", "false");
    same("filled([])", "[]");
    same("filled({})", "{}");
    // Spaces are text: `trim` first to count them as blank.
    same("filled(\" \")", "\" \"");
    same("filled(trim(\" \")) ?? \"blank\"", "\"blank\"");
    same("filled(err(\"x\")) ?? \"replaced\"", "\"replaced\"");
}

#[test]
fn filled_is_there_for_scripts_too() {
    // A rule a program's users write is where a blank value meets a fallback.
    let (_, r) = server("run_script(\"filled(name) ?? \\\"anonymous\\\"\", { name: \"\" })");
    assert_eq!(r, "\"anonymous\"");
}

#[test]
fn filled_is_known_to_the_checker_and_the_browser() {
    clean("component C(name: str) = <p>{filled(name) ?? \"anonymous\"}</p>\nroute GET / = <C name=\"\"/>");
    one("filled()", "missing argument `arg1` in call to `filled`");
}

// ---- `break` and `continue` (Q-152) ------------------------------------------------------------

#[test]
fn break_and_continue_say_what_to_write() {
    one("for x in [1] {\n  break\n}", "2 unknown name `break`: Quillo has no `break`: `return` leaves a loop");
    one("for x in [1] {\n  continue\n}", "2 unknown name `continue`: Quillo has no `continue`: `filter` the list first");
}

#[test]
fn break_says_so_when_the_program_runs_too() {
    refused("break", "unknown name `break`: Quillo has no `break`: `return` leaves a loop and the function around it, and `find` stops at the first match");
    refused("continue", "unknown name `continue`: Quillo has no `continue`: `filter` the list first, or put the rest of the loop in an `if`");
}

#[test]
fn they_are_not_reserved() {
    clean("let break = 1\nlet continue = 2\nprint(break + continue)");
}

#[test]
fn other_unknown_names_are_as_they_were() {
    one("let count = 1\nprint(cuont)", "did you mean `count`?");
    one("rate_limit(1)", "Quillo removed it in 0.5.0");
}

// ---- hex (Q-153) -------------------------------------------------------------------------------

#[test]
fn hex_literals_are_ints() {
    same("[0x0, 0xF, 0xff, 0xFF, 0Xff]", "[0, 15, 255, 255, 255]");
    same("0xDEAD_BEEF", "3735928559");
    same("0x7FFF_FFFF_FFFF_FFFF", MAX);
    same("-0x10", "-16");
    same("type_of(0xFF)", "\"int\"");
    // `e` is a digit here, not an exponent.
    same("0xe+1", "15");
    same("0x1_0 + 0x_1", "17");
    same("match 255 { 0xFF => \"ff\", _ => \"other\" }", "\"ff\"");
}

#[test]
fn hex_past_an_int_or_without_digits_is_refused() {
    refused("0x8000000000000000", "number `0x8000000000000000` is too large");
    refused("0xFFFF_FFFF_FFFF_FFFF", "number `0xFFFFFFFFFFFFFFFF` is too large");
    refused("0x", "expected hex digits after `0x`");
    refused("0x_", "expected hex digits after `0x`");
}

#[test]
fn a_zero_before_other_letters_is_as_it_was() {
    same("[0, 0.5, 0e3 + 1.5]", "[0, 0.5, 1.5]");
}

// ---- `bit` (Q-153) -----------------------------------------------------------------------------

#[test]
fn and_or_xor_not() {
    same("bit.and(12, 10)", "8");
    same("bit.or(12, 10)", "14");
    same("bit.xor(12, 10)", "6");
    same("bit.not(0)", "-1");
    same("bit.not(-1)", "0");
    same("bit.not(5)", "-6");
    same("bit.and(-1, 255)", "255");
    same("bit.or(-256, 255)", "-1");
    same("bit.xor(-1, 1)", "-2");
    same("0xCAFE | bit.and(0xFF)", "254");
}

#[test]
fn shifts() {
    same("bit.shl(1, 0)", "1");
    same("bit.shl(1, 10)", "1024");
    same("bit.shr(1024, 10)", "1");
    same("bit.shr(1, 63)", "0");
    // The sign stays, as in JavaScript's `>>`, and a mask takes it off.
    same("bit.shr(-8, 1)", "-4");
    same("bit.shr(-1, 63)", "-1");
    same("bit.and(bit.shr(-256, 4), 0xFF)", "240");
}

/// Where JavaScript's own operators, on 32 bits, would give something else.
#[test]
fn the_browser_works_on_all_64_bits() {
    same("bit.and(0x1_0000_00FF, 0x1_0000_000F)", "4294967311");
    same("bit.or(0x1_0000_0000, 1)", "4294967297");
    same("bit.xor(0x1_0000_0000, 0x1_0000_0001)", "1");
    same("bit.shl(1, 40)", "1099511627776");
    same("bit.shr(0x100_0000_0000, 40)", "1");
    same("bit.shl(1, 32)", "4294967296");
    same("bit.shr(-0x1_0000_0000, 1)", "-2147483648");
    same("bit.not(0xFFFF_FFFF)", "-4294967296");
}

/// Past 2^53, where the page keeps an int as a BigInt.
#[test]
fn past_what_a_javascript_number_holds() {
    same("bit.shl(1, 53)", "9007199254740992");
    same("bit.shl(1, 62)", "4611686018427387904");
    same(&format!("bit.and({}, 0xFF)", MAX), "255");
    same(&format!("bit.and({}, 0x7FFF_FFFF_FFFF_FFF0)", MAX), "9223372036854775792");
    same(&format!("bit.or({}, 0)", MAX), MAX);
    same(&format!("bit.xor({}, {})", MAX, MAX), "0");
    same(&format!("bit.not({})", MAX), "-9223372036854775808");
    same(&format!("bit.not({})", MIN), MAX);
    same(&format!("bit.shr({}, 62)", MAX), "1");
    same(&format!("bit.shr({}, 63)", MIN), "-1");
    same("bit.and(9007199254740993, 9007199254740993)", "9007199254740993");
}

#[test]
fn bits_past_the_top_are_dropped_not_an_overflow() {
    same("bit.shl(1, 63)", "-9223372036854775808");
    same("bit.shl(3, 63)", "-9223372036854775808");
    same("bit.shl(-1, 1)", "-2");
    same(&format!("bit.shl({}, 1)", MAX), "-2");
    same(&format!("bit.shl({}, 1)", MIN), "0");
}

#[test]
fn a_whole_float_counts_as_an_int_as_in_math() {
    same("bit.and(3.0, 1)", "1");
    same("bit.shl(1, 2.0)", "4");
}

#[test]
fn a_shift_is_0_to_63() {
    same("bit.shl(1, 64)", "ERROR: `bit.shl`: a shift is 0 to 63, got 64");
    same("bit.shl(1, -1)", "ERROR: `bit.shl`: a shift is 0 to 63, got -1");
    same("bit.shr(1, 64)", "ERROR: `bit.shr`: a shift is 0 to 63, got 64");
    same("bit.shr(1, -1)", "ERROR: `bit.shr`: a shift is 0 to 63, got -1");
    same(&format!("bit.shl(1, {})", MAX), &format!("ERROR: `bit.shl`: a shift is 0 to 63, got {}", MAX));
    same("let far = bit.shl(1, 40)\nbit.shr(1, far)", "ERROR: `bit.shr`: a shift is 0 to 63, got 1099511627776");
}

#[test]
fn what_is_not_an_int_is_refused_at_each_argument() {
    for f in ["and", "or", "xor", "shl", "shr"] {
        same(&format!("bit.{}(\"a\", 1)", f), &format!("ERROR: `bit.{}` expects an int as argument 1, got str", f));
        same(&format!("bit.{}(1, \"a\")", f), &format!("ERROR: `bit.{}` expects an int as argument 2, got str", f));
        same(&format!("bit.{}(1.5, 1)", f), &format!("ERROR: `bit.{}` expects an int as argument 1, got float", f));
    }
    same("bit.not(\"a\")", "ERROR: `bit.not` expects an int as argument 1, got str");
    same("bit.not(null)", "ERROR: `bit.not` expects an int as argument 1, got null");
}

#[test]
fn the_checker_knows_the_module() {
    clean("print(bit.and(1, 2), bit.or(1, 2), bit.xor(1, 2), bit.not(1), bit.shl(1, 2), bit.shr(1, 2))");
    one("bit.nope(1)", "`bit` has no function `nope`");
    one("bit.and(1)", "missing argument `arg2` in call to `bit.and`");
    one("bit.and(\"a\", 1)", "argument 1 of `bit.and` must be int, got str");
    clean("component C(n: int) = <p>{bit.and(n, 0xFF)} {bit.shl(1, n)}</p>\nroute GET / = <C n={3}/>");
}

#[test]
fn a_variable_called_bit_is_still_allowed() {
    clean("fn f(bit: int) = bit + 1\nprint(f(1))");
    same("let bit = 5\nbit + 1", "6");
}

// ---- beside these ------------------------------------------------------------------------------

#[test]
fn an_empty_template_is_empty_text_on_both_sides() {
    same("``", "\"\"");
}
