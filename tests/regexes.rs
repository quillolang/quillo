//! `re`: the escapes and classes a pattern may have, what it says of those it
//! may not, and the arguments each function takes.

use quillo::eval_str;
use quillo::re::compile;

fn ev(src: &str) -> String {
    match eval_str(src) {
        Ok(v) => v.repr(),
        Err(e) => panic!("{}\n--- source ---\n{}", e, src),
    }
}

#[track_caller]
fn fails(src: &str, wanted: &str) {
    match eval_str(src) {
        Ok(v) => panic!("expected an error, got {}\n--- source ---\n{}", v.repr(), src),
        Err(e) => assert!(e.to_string().contains(wanted), "{}\n  wanted: {}\n  got: {}", src, wanted, e),
    }
}

#[track_caller]
fn hits(pattern: &str, text: &str) {
    assert!(compile(pattern, "").unwrap_or_else(|e| panic!("{}: {}", pattern, e)).is_match(text), "{:?} should match {:?}", pattern, text);
}

#[track_caller]
fn misses(pattern: &str, text: &str) {
    assert!(!compile(pattern, "").unwrap_or_else(|e| panic!("{}: {}", pattern, e)).is_match(text), "{:?} should not match {:?}", pattern, text);
}

#[track_caller]
fn refused(pattern: &str, wanted: &str) {
    match compile(pattern, "") {
        Ok(_) => panic!("{:?} should be refused", pattern),
        Err(e) => assert!(e.contains(wanted), "{:?}\n  wanted: {}\n  got: {}", pattern, wanted, e),
    }
}

#[test]
fn control_characters_have_their_escapes() {
    hits(r"^a\nb$", "a\nb");
    hits(r"^a\tb$", "a\tb");
    hits(r"^a\rb$", "a\rb");
    hits(r"^a\fb$", "a\x0Cb");
    hits(r"^a\vb$", "a\x0Bb");
    hits(r"^a\0b$", "a\0b");
    misses(r"^a\nb$", "anb");
    refused(r"\01", "`\\0` is not one of the escapes patterns have");
}

#[test]
fn hex_and_unicode_escapes_name_a_character() {
    hits(r"^\x41$", "A");
    hits(r"^\xe9$", "é");
    hits(r"^\u0041$", "A");
    hits(r"^\u00e9$", "é");
    hits(r"^\u{1F600}$", "😀");
    hits(r"^\uD83D\uDE00$", "😀");
    hits(r"^\u{41}+$", "AAA");
    // A character regex syntax treats specially stays that character.
    hits(r"^\x2E$", ".");
    misses(r"^\x2E$", "a");
    hits(r"^\u002A$", "*");
}

#[test]
fn a_bad_hex_or_unicode_escape_is_refused() {
    let hex = "`\\x` takes two hex digits, as in \\x41";
    refused(r"\xZZ", hex);
    refused(r"\x4", hex);
    refused(r"\x", hex);
    refused(r"\xé", hex);
    let uni = "`\\u` takes four hex digits or {hex}";
    refused(r"\u12", uni);
    refused(r"\u", uni);
    refused(r"\uZZZZ", uni);
    refused(r"\u{}", uni);
    refused(r"\u{1F600", uni);
    refused(r"\u{1234567}", uni);
    refused(r"\u{110000}", uni);
    refused(r"\u{D800}", uni);
    let half = "half of a surrogate pair on its own";
    refused(r"\uD83D", half);
    refused(r"\uD83Dx", half);
    refused(r"\uD83D\u0041", half);
    refused(r"\uD83D\n", half);
    refused(r"\uDE00", half);
}

#[test]
fn word_boundaries_and_their_opposite() {
    hits(r"\bcat\b", "a cat sat");
    misses(r"\bcat\b", "concat");
    hits(r"con\Bcat", "concat");
    misses(r"a\Bb", "a b");
    // ASCII, as in JavaScript.
    misses(r"\bé", "é");
    refused(r"\b*", "nothing to repeat before `*`");
    refused(r"\B+", "nothing to repeat before `+`");
}

#[test]
fn escapes_in_a_class_are_single_characters() {
    hits(r"^[\n]$", "\n");
    hits(r"^[\t]$", "\t");
    hits(r"^[\r]$", "\r");
    hits(r"^[\f]$", "\x0C");
    hits(r"^[\v]$", "\x0B");
    hits(r"^[\0]$", "\0");
    hits(r"^[\x41]$", "A");
    hits(r"^[\u0041]$", "A");
    hits(r"^[\u{1F600}]$", "😀");
    hits(r"^[\.]$", ".");
    misses(r"^[\.]$", "a");
    hits(r"^[\]]$", "]");
    hits(r"^[a\-z]$", "-");
    misses(r"^[a\-z]$", "b");
}

#[test]
fn a_class_can_range_between_escapes() {
    hits(r"^[\x41-\x43]+$", "ABC");
    misses(r"^[\x41-\x43]+$", "ABD");
    hits(r"^[\u0041-\u{43}]+$", "CBA");
    hits(r"^[\t-\r]+$", "\t\n\r");
    hits(r"^[\0-\x1F]+$", "\0\x01\x1F");
    refused(r"[\x43-\x41]", "a range in [...] goes from the lower character to the higher");
    refused(r"[a-\d]", "a range in [...] is between two characters, not \\d, \\w or \\s");
    refused(r"[\d-a]", "a range in [...] is between two characters");
}

#[test]
fn a_class_says_what_it_cannot_hold() {
    refused(r"[\b]", "`\\b` cannot go in [...]");
    refused(r"[\B]", "`\\b` cannot go in [...]");
    refused(r"[abc", "`[` without its `]`");
    refused(r"[", "`[` without its `]`");
    refused(r"[a-", "`[` without its `]`");
    refused(r"[^", "`[` without its `]`");
    refused(r"\-", "`\\-` is not one of the escapes patterns have");
}

#[test]
fn a_groups_name_is_a_plain_word() {
    let name = "a group's name is letters, digits and `_`, as in (?<name>...)";
    refused(r"(?<>x)", name);
    refused(r"(?<1a>x)", name);
    refused(r"(?<a-b>x)", name);
    refused(r"(?<a", name);
    hits(r"^(?<first_1>a)$", "a");
}

#[test]
fn a_brace_is_a_repeat_or_refused() {
    let lone = "`{` on its own: write `\\{`, or repeat with {n}, {n,} or {n,m}";
    refused(r"a{2", lone);
    refused(r"a{2,3", lone);
    refused(r"a{2,3x}", lone);
    refused(r"a{", lone);
    hits(r"^a{2,}$", "aaa");
    misses(r"^a{2,3}$", "aaaa");
}

#[test]
fn each_function_says_what_it_takes() {
    fails("re.test(5, \"a\")", "`re.test` expects a str as argument 1, got int");
    fails("re.test()", "`re.test` expects a str as argument 1, got nothing");
    fails("re.find(null, \"a\")", "`re.find` expects a str as argument 1, got null");
    fails("re.test(\"a\", 5)", "`re.test` expects a str as argument 2, got int");
    fails("re.test(\"a\", \"a\", 5)", "`re.test` expects a str as argument 3, got int");
    fails("re.replace(\"a\", \"a\", \"b\", 5)", "`re.replace` expects a str as argument 4, got int");
    fails("re.replace(\"a\", \"a\", 5)", "`re.replace` expects what to put in (a str or a function) as argument 3, got int");
    fails("re.replace(\"a\", \"a\")", "as argument 3, got null");
    assert_eq!(ev("re.test(\"A\", \"a\", null)"), "false");
    assert_eq!(ev("re.replace(\"A\", \"a\", \"b\", \"i\")"), "\"b\"");
}

#[test]
fn a_dollar_that_names_nothing_is_a_dollar() {
    assert_eq!(ev("re.replace(\"cost\", \"cost\", \"$ 5\")"), "\"$ 5\"");
    assert_eq!(ev("re.replace(\"cost\", \"cost\", \"5$\")"), "\"5$\"");
    assert_eq!(ev("re.replace(\"cost\", \"cost\", \"$x$\")"), "\"$x$\"");
    assert_eq!(ev("re.replace(\"cost\", \"(c)ost\", \"$$1 and $1\")"), "\"$1 and c\"");
}

#[test]
fn more_patterns_than_are_kept_all_still_work() {
    // Past 256 at once, what was compiled is let go, not wrong.
    let all = ev("let hits = (0..300) | map((i) => re.test(`x${i}y`, `^x${i}y$`))\nlet again = (0..300) | map((i) => re.test(`x${i}y`, `^x${i}y$`))\n[len(hits), all(hits, (x) => x), all(again, (x) => x)]");
    assert_eq!(all, "[300, true, true]");
}
