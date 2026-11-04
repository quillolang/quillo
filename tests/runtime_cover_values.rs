//! The browser runtime's values and standard library, run in Node against the
//! interpreter: dates, plurals, numbers, decimals, money, vectors, ranges and
//! the small builtins must give in the browser what they give on the server,
//! for the ordinary case and the edge ones (wrong types, boundaries, unicode).
//! What only the page has (its objects, `fetch`, `crypto`) is checked against
//! the answer the browser must give.

use std::process::Command;
use quillo::Interp;

fn node_available() -> bool {
    let ok = Command::new("node").arg("--version").output().map(|o| o.status.success()).unwrap_or(false);
    if !ok {
        eprintln!("skipping: needs Node");
    }
    ok
}

/// The server's answer for a loaded program's `__t`, as `repr` writes it, or "ERROR: ...".
fn server_of(it: &mut Interp) -> String {
    match it.call(&quillo::value::lookup(&it.globals, "__t").unwrap(), vec![], 0) {
        Ok(v) => v.repr(),
        Err(quillo::interp::Ctrl::Error(e)) => format!("ERROR: {}", e.msg),
        Err(quillo::interp::Ctrl::Return(v)) => v.repr(),
        Err(quillo::interp::Ctrl::Reply(_)) => "REPLY".into(),
    }
}

/// What Node prints running `__t` compiled, with `prelude` run first.
fn browser_of(it: &Interp, prelude: &str) -> String {
    let js = format!("{}\n{}", prelude, quillo::client::test_script(it, "__t").unwrap_or_else(|e| panic!("{}", e)));
    let out = Command::new("node").arg("-e").arg(&js).output().expect("node");
    assert!(out.status.success(), "node failed: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn loaded(src: &str) -> Interp {
    let mut it = Interp::new(".");
    it.load_source(src, "case.qwe").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    it
}

/// Runs `body` as the body of `__t` on the server and in Node: (server, browser).
fn both_with(prelude: &str, defs: &str, body: &str) -> (String, String) {
    let mut it = loaded(&format!("{}\nfn __t() {{\n{}\n}}\n", defs, body));
    let server = server_of(&mut it);
    (server, browser_of(&it, prelude))
}

/// The browser gives what the server gives, and that is `want`.
#[track_caller]
fn same_is(defs: &str, body: &str, want: &str) {
    let (s, b) = both_with("", defs, body);
    assert_eq!(s, want, "server\n--- defs ---\n{}\n--- body ---\n{}", defs, body);
    assert_eq!(b, want, "browser\n--- defs ---\n{}\n--- body ---\n{}", defs, body);
}

/// Both stop, with `want` as the message.
#[track_caller]
fn same_error(body: &str, want: &str) {
    same_is("", body, &format!("ERROR: {}", want));
}

/// Only the browser runs it (the page's objects): its answer, with `prelude` run first.
fn browser(prelude: &str, body: &str) -> String {
    browser_of(&loaded(&format!("fn __t() {{\n{}\n}}\n", body)), prelude)
}

#[test]
fn decimals_past_28_places_round_as_on_the_server() {
    if !node_available() {
        return;
    }
    // A product with more than 28 decimals keeps 28, the last one rounded half to even.
    same_is("", "dec(\"0.000000000000001\") * dec(\"0.000000000000001\")", "0.0000000000000000000000000000m");
    same_is("", "dec(\"0.00000000000001\") * dec(\"0.000000000000015\")", "0.0000000000000000000000000002m");
    same_is("", "dec(\"-0.00000000000001\") * dec(\"0.000000000000025\")", "-0.0000000000000000000000000002m");
    same_is("", "dec(\"0.00000000000001\") * dec(\"0.000000000000035\")", "0.0000000000000000000000000004m");
    // Past 96 bits, too: what is kept is rounded, or there is no room at all.
    same_is("", "dec(\"7922816251426433759354395033.5\") * dec(\"10\")", "79228162514264337593543950335m");
    same_error("dec(\"79228162514264337593543950335\") * dec(\"2\")", "decimal overflow");
    // Written with zeros past what a decimal holds, the zeros go and the value stays.
    same_is("", "dec(\"1.00000000000000000000000000000\")", "1.0000000000000000000000000000m");
    same_is("", "dec(\"-2.500000000000000000000000000000000\")", "-2.5000000000000000000000000000m");
    same_is("", "dec(\"79228162514264337593543950335.0\")", "79228162514264337593543950335m");
    same_is("", "dec(\"79228162514264337593543950335.000\") == dec(\"79228162514264337593543950335\")", "true");
    // Digits that are not zeros there, and a whole part past 96 bits, are not a decimal.
    same_is("", "dec(\"79228162514264337593543950336.0\")", "err(\"not a decimal: \\\"79228162514264337593543950336.0\\\"\")");
    same_is("", "dec(\"1.00000000000000000000000000001\")", "err(\"not a decimal: \\\"1.00000000000000000000000000001\\\"\")");
}

#[test]
fn ints_past_2_53_meet_floats_and_wrong_types() {
    if !node_available() {
        return;
    }
    // Past 2^53 an int with a float is a float, as on the server.
    same_is(
        "",
        "[9007199254740993 % 1.5, 0.5 / 9007199254740993, (9007199254740993 + 0.5) == 9007199254740992.0, (0.25 - 9007199254740993) == -9007199254740992.0, 9007199254740993 * 0.5 > 4503599627370000.0]",
        "[0.5, 0.00000000000000005551115123125783, true, true, true]",
    );
    same_is("", "-9223372036854775807 / 0.5 < -18446744073709000000.0", "true");
    same_error("9007199254740993 - \"x\"", "cannot subtract int and str");
    same_error("9007199254740993 * [1]", "cannot multiply int and list");
    same_error("\"2\" / 9007199254740993", "cannot divide str and int");
    same_error("9007199254740993 % null", "cannot take the remainder of int and null");
}

#[test]
fn float_division_by_zero_is_infinite_and_int_division_stops() {
    if !node_available() {
        return;
    }
    same_is("", "[1.5 / 0, -1.5 / 0, 1.5 % 0, 0.5 / 0 > 1000000]", "[inf, -inf, NaN, true]");
    same_error("7 / 0", "division by zero");
    same_error("-7 % 0", "division by zero");
}

#[test]
fn what_cannot_be_negated_indexed_or_converted_is_named_by_its_type() {
    if !node_available() {
        return;
    }
    same_error("1 + ((x) => x)", "cannot add int and fn");
    same_error("let s = \"x\"\n-s", "cannot negate str");
    same_error("let l = [1]\n-l", "cannot negate list");
    same_error("let f = (x) => x\n-f", "cannot negate fn");
    same_is("", "let d = dec(\"1.5\")\nlet b = 9007199254740993\n[-d, -b]", "[-1.5m, -9007199254740993]");
    same_error("let x = 5\nx[0]", "cannot index int with int");
    same_error("let x = true\nx[\"a\"]", "cannot index bool with str");
    same_error("let x = [1]\nx[\"a\"]", "cannot index list with str");
    same_error("let x = \"héllo\"\nx[0.5]", "cannot index str with float");
    same_is("", "let x = null\n[x[0], x[\"a\"]]", "[null, null]");
    same_is(
        "",
        "[int([1]), int({}), int(null), float([1]), float(null), float({ a: 1 }), int(true), float(\"2.5\")]",
        "[err(\"cannot convert list to int\"), err(\"cannot convert map to int\"), err(\"cannot convert null to int\"), err(\"cannot convert list to float\"), err(\"cannot convert null to float\"), err(\"cannot convert map to float\"), 1, 2.5]",
    );
}

#[test]
fn contains_slice_reverse_index_of_and_pow_take_what_they_take() {
    if !node_available() {
        return;
    }
    // A map holds a key, by its text (an int is its digits); null holds nothing.
    same_is(
        "",
        "[contains({ a: 1 }, \"a\"), contains({ a: 1 }, \"b\"), contains({}, \"\"), contains(null, 1), contains({ \"1\": 2 }, 1), contains({ \"é\": 1 }, \"é\")]",
        "[true, false, false, false, true, true]",
    );
    same_error("contains(5, 1)", "`contains` expects a str, list or map, got int");
    same_error("contains(true, 1)", "`contains` expects a str, list or map, got bool");
    same_error("slice(5, 0)", "`slice` expects a list or str, got int");
    same_error("slice({ a: 1 }, 0, 1)", "`slice` expects a list or str, got map");
    same_error("reverse(5)", "`reverse` expects a list or str, got int");
    same_error("reverse(null)", "`reverse` expects a list or str, got null");
    same_is("", "[reverse(\"añ😀\"), reverse([]), reverse(\"\")]", "[\"😀ña\", [], \"\"]");
    same_error("index_of(5, 1)", "`index_of` expects a list or str, got int");
    same_error("index_of(null, 1)", "`index_of` expects a list or str, got null");
    same_error("index_of({ a: 1 }, \"a\")", "`index_of` expects a list or str, got map");
    // A float or a negative power is a float's power.
    same_is("", "[pow(2.0, 0.5), pow(2, -1), pow(4, -2), pow(0.5, 3), pow(-8, 1.0 / 3), pow(2.5, 2)]", "[1.4142135623730951, 0.5, 0.0625, 0.125, NaN, 6.25]");
    // A decimal to an int's power is a decimal (FEEDBACK50.md, Q-632; branch feedback50-fixes).
    same_is("", "pow(dec(\"1.5\"), 2)", "2.25m");
    same_error("pow(2, dec(\"1.5\"))", "`pow` expects a number as argument 2, got dec (this works on floats: convert a dec with float(x))");
    same_error("pow(\"a\", 2)", "`pow` expects a number as argument 1, got str");
    same_error("pow(2, \"a\")", "`pow` expects a number as argument 2, got str");
    same_error("pow(2.5, null)", "`pow` expects a number as argument 2, got null");
}

#[test]
fn range_with_a_step_counts_either_way() {
    if !node_available() {
        return;
    }
    same_is(
        "",
        "[range(0, 10, 3), range(10, 0, -3), range(5, 5, 1), range(5, 5, -1), range(0, 1, 5), range(-3, 3, 2), range(0, 5, -1), range(5, 0, 1)]",
        "[[0, 3, 6, 9], [10, 7, 4, 1], [], [], [0], [-3, -1, 1], [], []]",
    );
    same_error("range(0, 10, 0)", "`range`: a step of 0 never gets anywhere");
    same_error("range(0, 100000000, 1)", "range is too large");
    same_error("range(100000000, 0, -1)", "range is too large");
    same_error("range(0, 1.5, 1)", "`range` expects an int as argument 2, got float");
    same_error("range(0, 10, \"2\")", "`range` expects an int as argument 3, got str");
}

#[test]
fn a_date_parameter_takes_a_real_day_only() {
    if !node_available() {
        return;
    }
    let f = "fn f(d: date) = d\nfn g(d: date?) = d";
    same_is(
        f,
        "[f(\"2024-02-29\"), f(\"2000-02-29\"), f(\"0001-01-01\"), f(\"9999-12-31\"), g(null), g(\"2026-04-30\")]",
        "[\"2024-02-29\", \"2000-02-29\", \"0001-01-01\", \"9999-12-31\", null, \"2026-04-30\"]",
    );
    for bad in [
        "\"2026-02-29\"",
        "\"1900-02-29\"",
        "\"2026-04-31\"",
        "\"2026-13-01\"",
        "\"2026-00-10\"",
        "\"2026-10-00\"",
        "\"0000-01-01\"",
        "\"2026-1-01\"",
        "\"2026-10-27T00:00:00Z\"",
        "\"２０２６-10-27\"",
        "\"\"",
    ] {
        same_is(f, &format!("f({})", bad), &format!("ERROR: argument `d` of `f` must be date, got str {}", bad));
    }
    same_is(f, "f(20261027)", "ERROR: argument `d` of `f` must be date, got int 20261027");
    same_is(f, "f(null)", "ERROR: argument `d` of `f` must be date, got null null");
}

#[test]
fn pattern_escapes_are_checked_as_on_the_server() {
    if !node_available() {
        return;
    }
    // \xHH, a surrogate pair written as two \u escapes, and \u with four digits.
    same_is(
        "",
        "[re.test(\"A\", \"\\\\x41\"), re.test(\"😀\", \"^\\\\ud83d\\\\ude00$\"), re.test(\"é\", \"\\\\u00e9\"), re.find(\"a😀b\", \"\\\\ud83d\\\\ude00\"), re.test(\"a\", \"\\\\x62\")]",
        "[true, true, true, { text: \"😀\", start: 1, end: 2, groups: [], named: {} }, false]",
    );
    for bad in ["\\\\xZ1", "\\\\x4", "\\\\x"] {
        same_error(&format!("re.test(\"a\", \"{}\")", bad), "`re.test`: `\\x` takes two hex digits, as in \\x41");
    }
    // Half a pair on its own: a low half, a high half at the end or before something else.
    for bad in ["\\\\udc00", "\\\\udfff", "\\\\ud83d", "\\\\ud83dx", "\\\\ud83d\\\\u0041"] {
        same_error(&format!("re.test(\"a\", \"{}\")", bad), "`re.test`: half of a surrogate pair on its own");
    }
    same_is("", "[re.valid(\"\\\\xZZ\"), re.valid(\"\\\\udc00\"), re.valid(\"\\\\x41\")]", "[false, false, true]");
}

#[test]
fn money_says_what_it_was_given_that_is_not_an_amount() {
    if !node_available() {
        return;
    }
    let usd = "let m = money.of(dec(\"1.00\"), \"USD\")\n";
    same_error("money.of(true, \"USD\")", "`money.of` expects an amount (a dec, int or str) as argument 1, got bool true");
    same_error("money.of([1], \"USD\")", "`money.of` expects an amount (a dec, int or str) as argument 1, got list [1]");
    same_error("money.of(null, \"USD\")", "`money.of` expects an amount (a dec, int or str) as argument 1, got null null");
    same_error(&format!("{}money.mul(m, \"2\")", usd), "`money.mul` expects a dec or an int as argument 2, got str \"2\"");
    same_error(&format!("{}money.mul(m, null)", usd), "`money.mul` expects a dec or an int as argument 2, got null null");
    same_error(&format!("{}money.mul(m, 1.5)", usd), "`money.mul` takes a dec or an int, not a float: convert it with dec(x)");
    for (ratio, got) in [("-1", "int -1"), ("\"x\"", "str \"x\""), ("2.5", "float 2.5"), ("dec(\"-0.5\")", "dec -0.5m"), ("null", "null null")] {
        same_error(
            &format!("{}money.allocate(m, [1, {}])", usd, ratio),
            &format!("`money.allocate`: ratio 1 must be an int or a dec, 0 or more, got {}", got),
        );
    }
    // A ratio of 0 takes nothing; decimals as ratios split by their size, the cent left to
    // the largest remainder, 0.666... (FEEDBACK50.md, Q-631; branch feedback50-fixes).
    same_is(
        "",
        &format!("{}money.allocate(m, [0, dec(\"0.5\"), 1])", usd),
        "[{ amount: 0.00m, currency: \"USD\" }, { amount: 0.33m, currency: \"USD\" }, { amount: 0.67m, currency: \"USD\" }]",
    );
    same_error("format_number(\"12\")", "`format_number` expects a number as argument 1, got str \"12\"");
    same_error("format_number(null)", "`format_number` expects a number as argument 1, got null null");
    same_error("format_number([1])", "`format_number` expects a number as argument 1, got list [1]");
}

#[test]
fn vectors_of_floats_scalars_and_norms() {
    if !node_available() {
        return;
    }
    // A dot product with a float in it is a float's.
    same_is(
        "",
        "[vec.dot([1.5, 2], [2, 4.25]), vec.dot([0.1, 0.2], [0.3, 0.4]), vec.dot([], []), vec.dot([1, 2], [3, 4])]",
        "[11.5, 0.11000000000000001, 0, 11]",
    );
    same_error("vec.dot([1.5], [1, 2])", "`vec.dot` needs lists of the same length (1 and 2)");
    // Two numbers are added as numbers; one list goes with the other number.
    same_is(
        "",
        "[vec.add(1, 2), vec.mul(2.5, 1.5), vec.sub(1, [1, 2]), vec.add([1, 2], 1), vec.div(1, 4), vec.mod(7, 4)]",
        "[3, 3.75, [0, -1], [2, 3], 0.25, 3]",
    );
    same_error("vec.add(\"a\", 1)", "`vec.add` expects numbers, got str and int");
    same_error("vec.div(1, 0)", "`vec.div`: division by zero");
    // The norms: 1, the largest (p = inf), and any other p.
    same_is(
        "",
        "[vec.norm([0.25, -1.5], 1), vec.norm([0.25, -1.5], 1.5 / 0), vec.norm([3, -4], 3), vec.norm([3, -4], 0.5), vec.norm([], 1) == 0, vec.norm([], 1.5 / 0) == 0, vec.norm([], 3) == 0, vec.norm([0.3, 0.4])]",
        "[1.75, 1.5, 4.497941445275415, 13.928203230275509, true, true, true, 0.5]",
    );
    same_error("vec.norm([1], \"x\")", "`vec.norm` expects a number as argument 2, got str");
    // A column by position: rows too short, or not lists, have none.
    same_is(
        "",
        "[vec.column([[1, 2], [3], \"x\", [5, 6]], 1), vec.column([[1, 2]], -1), vec.column([[1, 2]], 0), vec.column([], 0), vec.column([{ a: 1 }, [2]], \"a\")]",
        "[[2, null, null, 6], [null], [1], [], [1, null]]",
    );
    same_error("vec.column([[1, 2]], 1.5)", "`vec.column` expects a str or int key as argument 2, got float");
    same_error("vec.column([[1, 2]], null)", "`vec.column` expects a str or int key as argument 2, got null");
    same_error("vec.column([[1, 2]], [0])", "`vec.column` expects a str or int key as argument 2, got list");
}

#[test]
fn the_inverse_normal_reaches_its_far_tails() {
    if !node_available() {
        return;
    }
    // Past about 1e-11 from 0 or 1 the far-tail series is used.
    same_is(
        "",
        "[stats.normal_inv(0.000000000001), stats.normal_inv(0.999999999999), stats.normal_inv(1e-300), stats.normal_inv(0.01), stats.normal_inv(0.975, 10, 2)]",
        "[-7.034483825301132, 7.0344869100478356, -37.0470962993612, -2.3263478740408408, 13.919927969080106]",
    );
    same_is("", "stats.normal_inv(0.5) == 0", "true");
    same_error("stats.normal_inv(1)", "`stats.normal_inv`: p must be between 0 and 1 (exclusive), got 1");
    same_error("stats.normal_inv(0)", "`stats.normal_inv`: p must be between 0 and 1 (exclusive), got 0");
}

/// `t(key, { count })` in `locale`, on the server and in the browser, for counts that
/// fall in each plural form.
fn plurals_in(locale: &str) -> (String, String) {
    // Each form the language has, and no other (the server warns of those).
    let forms: Vec<String> = quillo::i18n::categories(locale).iter().map(|c| format!("{}: \"{} {{count}}\"", c, c)).collect();
    let src = format!(
        r#"translations({{ "{}": {{ k: {{ {} }} }} }})
fn __t() = [0, 1, 2, 3, 4, 5, 6, 7, 10, 11, 12, 14, 19, 20, 21, 22, 25, 100, 101, 102, 103, 111, 1000000, -1, -2, -21, 1.5, "3", dec("1.0"), dec("2"), "2.5"] | map((n) => t("k", {{ count: n }}))
"#,
        locale,
        forms.join(", ")
    );
    let mut it = loaded(&src);
    let server = server_of(&mut it);
    let words = it.site.i18n.as_ref().unwrap().browser_json(locale, None);
    (server, browser_of(&it, &format!("globalThis.window = {{ [Symbol.for(\"quillo\")]: {{ i18n: {} }} }};", words)))
}

#[test]
fn plurals_follow_each_languages_rules_in_the_browser_too() {
    if !node_available() {
        return;
    }
    for locale in [
        "en", "de", "ja", "zh-TW", "ko", "pt-PT", "pt_pt", "pt-BR", "fr", "es", "it", "hi", "fa", "ro", "lt", "lv", "sl", "is", "mk", "ga", "cy", "mt",
        "ru", "uk", "sr", "hr", "pl", "cs", "sk", "ar", "he", "xx",
    ] {
        let (s, b) = plurals_in(locale);
        assert_eq!(s, b, "{}", locale);
    }
    // What some forms are, that the server and the browser agree on.
    let (s, _) = plurals_in("cy");
    assert!(s.starts_with("[\"zero 0\", \"one 1\", \"two 2\", \"few 3\", \"other 4\", \"other 5\", \"many 6\""), "{}", s);
    let (s, _) = plurals_in("ar");
    assert!(s.contains("\"few 103\"") && s.contains("\"many 111\"") && s.contains("\"other 100\""), "{}", s);
    let (s, _) = plurals_in("pt-PT");
    assert!(s.contains("\"other 0\"") && s.contains("\"many 1000000\""), "{}", s);
    let (s, _) = plurals_in("lv");
    assert!(s.contains("\"zero 10\"") && s.contains("\"one 21\"") && s.contains("\"zero 11\""), "{}", s);
}

#[test]
fn dates_are_read_by_a_pattern() {
    if !node_available() {
        return;
    }
    same_is(
        "",
        r#"[date.parse("27/10/2026", "DD/MM/YYYY"), date.parse(" 7/3/26 ", "D/M/YY"), date.parse("October 27, 2026", "MMMM D, YYYY"), date.parse("oct 27 2026", "MMM D YYYY"), date.parse("Tuesday 27 Oct 2026", "dddd D MMM YYYY"), date.parse("TUE 27 Oct 2026", "ddd D MMM YYYY"), date.parse("le 27 de 10 2026", "[le] DD [de] MM YYYY")]"#,
        r#"["2026-10-27", "2026-03-07", "2026-10-27", "2026-10-27", "2026-10-27", "2026-10-27", "2026-10-27"]"#,
    );
    // With a time it is a date-time: 12-hour clocks, and offsets as Z, +01:00 or -0530.
    same_is(
        "",
        r#"[date.parse("27/10/2026 14:30", "DD/MM/YYYY HH:mm"), date.parse("27/10/2026 2:05:09 PM", "DD/MM/YYYY h:mm:ss A"), date.parse("27/10/2026 12:30 AM", "DD/MM/YYYY hh:mm A"), date.parse("27/10/2026 12:30 pm", "DD/MM/YYYY hh:mm A"), date.parse("2026-10-27 9 +01:00", "YYYY-MM-DD H Z"), date.parse("2026-10-27 09 -0530", "YYYY-MM-DD HH Z"), date.parse("2026-10-27 09 z", "YYYY-MM-DD HH Z"), date.parse("10/27/2026 07", "MM/DD/YYYY hh")]"#,
        r#"["2026-10-27T14:30:00Z", "2026-10-27T14:05:09Z", "2026-10-27T00:30:00Z", "2026-10-27T12:30:00Z", "2026-10-27T09:00:00+01:00", "2026-10-27T09:00:00-05:30", "2026-10-27T09:00:00Z", "2026-10-27T07:00:00Z"]"#,
    );
    // Text that does not fit: each is an error value naming the text and the pattern.
    for (text, pattern) in [
        ("27 [10 2026", "DD [[] MM YYYY"),
        ("2026 x", "YYYY [x"),
        ("2026-02-29", "YYYY-MM-DD"),
        ("2026-1-1", "YYYY-MM-DD"),
        ("2026-10-27x", "YYYY-MM-DD"),
        ("2026/10/27", "YYYY-MM-DD"),
        ("13/10/2026 1 PM", "MM/DD/YYYY h A"),
        ("10/27/2026 13:00 PM", "MM/DD/YYYY hh:mm A"),
        ("10/27/2026 00:00", "MM/DD/YYYY hh:mm"),
        ("10/27/2026 24:00", "MM/DD/YYYY HH:mm"),
        ("10/27/2026 23:60", "MM/DD/YYYY HH:mm"),
        ("10/27/2026 23:59:60", "MM/DD/YYYY HH:mm:ss"),
        ("10/27/2026 X", "MM/DD/YYYY A"),
        ("2026-10-27 +01", "YYYY-MM-DD Z"),
        ("2026-10-27 +0é:00", "YYYY-MM-DD Z"),
        ("2026-10-27 +99:00", "YYYY-MM-DD Z"),
        ("Foo 27 2026", "MMMM D YYYY"),
        ("27 2026", "dddd D YYYY"),
        ("27 ÉÉ 2026", "DD [ÉÉ] YYYY"),
        ("27 ÉÉ 2026", "DD [éé] YYYY"),
        ("2026", "YYYY"),
        ("10/2026", "MM/YYYY"),
        ("", "YYYY"),
        ("２０２６-10-27", "YYYY-MM-DD"),
    ] {
        same_is(
            "",
            &format!("date.parse({:?}, {:?})", text, pattern),
            &format!("err({:?})", format!("{:?} is not a date in the form {:?}", text, pattern)),
        );
    }
    same_error(r#"date.parse("x", 5)"#, "`date.parse` expects a pattern (like \"DD/MM/YYYY\") as argument 2, got int");
    same_error(r#"date.parse("x", ["YYYY"])"#, "`date.parse` expects a pattern (like \"DD/MM/YYYY\") as argument 2, got list");
    same_error(r#"date.parse(5, "YYYY")"#, "`date.parse` expects a str as argument 1, got int");
}

#[test]
fn iso_date_times_are_read_with_seconds_fractions_and_offsets() {
    if !node_available() {
        return;
    }
    same_is(
        "",
        r#"[date.parse("2026-10-27T14:30:15Z"), date.parse("2026-10-27t14:30+01:00"), date.parse("2026-10-27 14:30:15.250-04:00"), date.parse("2026-10-27T14:30:15,5Z"), date.parse("2026-10-27T14:30:15.123456789+0100"), date.parse("2026-10-27T14:30:15+01"), date.parse("2026-10-27T14:30:15z"), date.parse(" 2026-10-27 "), date.parse("9999-12-31T23:30:00-05:00"), date.parse("0001-01-01T00:30:00+01:00")]"#,
        r#"["2026-10-27T14:30:15Z", "2026-10-27T14:30:00+01:00", "2026-10-27T14:30:15.250-04:00", "2026-10-27T14:30:15.5Z", "2026-10-27T14:30:15.123456789+01:00", "2026-10-27T14:30:15+01:00", "2026-10-27T14:30:15Z", "2026-10-27", "9999-12-31T23:30:00-05:00", "0001-01-01T00:30:00+01:00"]"#,
    );
    for bad in [
        "2026-10-27T14:30:1Z",
        "2026-10-27T14:30:61Z",
        "2026-10-27T14:30:15.Z",
        "2026-10-27T14:30:15.1234567890Z",
        "2026-10-27T14:30:15+1:00",
        "2026-10-27T25:30:15Z",
        "2026-13-27T14:30:15Z",
        "2026-10-27X14:30:15Z",
        "2026-10-27T14:30é15Z",
        "2026-10-27T14:30:15+01:0",
        "2026-10-27T14:3",
        "2026-10-27T14:30:",
        "2026-10-27T14:30:15Q",
    ] {
        same_is("", &format!("date.parse({:?})", bad), &format!("err({:?})", format!("not a date or a date-time: {:?}", bad)));
    }
    same_is(
        "",
        r#"date.parse("2026-10-27T14:30:15")"#,
        r#"err("\"2026-10-27T14:30:15\" has no offset, so it is a wall-clock time and names no instant: read it in its zone with date.at(v, zone)")"#,
    );
}

#[test]
fn the_calendar_functions_agree_with_the_server() {
    if !node_available() {
        return;
    }
    same_is(
        "",
        r#"[date.of(2024, 2, 29), date.parts("2026-10-27"), date.parts("2026-10-27T14:30:15+05:30"), date.weekday("2026-11-01"), date.weekday("2026-10-27T23:30:00-05:00")]"#,
        r#"["2024-02-29", { year: 2026, month: 10, day: 27, weekday: 2 }, { year: 2026, month: 10, day: 27, weekday: 2, hour: 14, minute: 30, second: 15, offset: "+05:30" }, 7, 2]"#,
    );
    same_error("date.of(2026, 2, 29)", "`date.of`: there is no date 2026-2-29");
    same_error("date.parts(\"2026-10-32\")", "`date.parts` expects a date or a date-time as argument 1, got \"2026-10-32\"");
    same_is(
        "",
        r#"[date.is_leap_year(2024), date.is_leap_year(1900), date.is_leap_year(2000), date.is_leap_year("2023-05-01"), date.is_leap_year("2024-05-01T00:00:00Z")]"#,
        "[true, false, true, false, true]",
    );
    same_is(
        "",
        r#"[date.days_in_month(2024, 2), date.days_in_month(2023, 2), date.days_in_month("2026-04-10"), date.days_in_month(2026, 12), date.days_in_month("2026-12-10T00:00:00Z"), date.days_in_month(1900, 2)]"#,
        "[29, 28, 30, 31, 31, 28]",
    );
    same_error("date.days_in_month(2026, 13)", "`date.days_in_month`: there is no month 13");
    same_error("date.days_in_month(2026, 0)", "`date.days_in_month`: there is no month 0");
    same_error("date.days_in_month(2026)", "`date.days_in_month` expects an int as argument 2, got nothing");
    same_is(
        "",
        r#"[date.start_of_month("2026-10-27"), date.end_of_month("2024-02-10"), date.end_of_month("2026-12-01"), date.days_between("2026-10-27", "2026-01-01"), date.days_between("0001-01-01", "9999-12-31")]"#,
        r#"["2026-10-01", "2024-02-29", "2026-12-31", -299, 3652058]"#,
    );
    same_is(
        "",
        r#"[date.valid("2026-10-27"), date.valid("2026-10-27", "date-time"), date.valid("2026-10-27T00:00:00Z", "date"), date.valid("2026-10-27T00:00:00Z", "date-time"), date.valid(5), date.valid_zone("UTC"), date.valid_zone("Mars/Base"), date.valid_zone(5)]"#,
        "[true, false, false, true, false, true, false, false]",
    );
    same_error(r#"date.valid("x", "time")"#, "`date.valid` takes \"date\" or \"date-time\" as argument 2, got \"time\"");
}

#[test]
fn months_are_added_and_counted_to_the_end_of_shorter_months() {
    if !node_available() {
        return;
    }
    same_is(
        "",
        r#"[date.add_months("2026-01-31", 1), date.add_months("2024-01-31", 1), date.add_months("2026-03-31", -1), date.add_months("2026-10-27", 0), date.add_months("2026-10-27", -22), date.add_months("2026-10-27", 120), date.add_months("9999-11-30", 1), date.add_months("0001-02-28", -1), date.add_years("2024-02-29", 1), date.add_years("2024-02-29", 4), date.add_years("2026-10-27", -2025)]"#,
        r#"["2026-02-28", "2024-02-29", "2026-02-28", "2026-10-27", "2024-12-27", "2036-10-27", "9999-12-30", "0001-01-28", "2025-02-28", "2028-02-29", "0001-10-27"]"#,
    );
    let out = "the date is out of range (years 1 to 9999)";
    for body in [
        r#"date.add_months("2026-10-27", 120001)"#,
        r#"date.add_months("2026-10-27", -120001)"#,
        r#"date.add_months("9999-12-01", 1)"#,
        r#"date.add_months("0001-01-01", -1)"#,
    ] {
        same_error(body, &format!("`date.add_months`: {}", out));
    }
    for body in [r#"date.add_years("2026-10-27", 10001)"#, r#"date.add_years("2026-10-27", 8000)"#, r#"date.add_years("2026-10-27", -2026)"#] {
        same_error(body, &format!("`date.add_years`: {}", out));
    }
    same_error(
        r#"date.add_months("2026-10-27T14:30:00Z", 1)"#,
        "`date.add_months` expects a date (YYYY-MM-DD) as argument 1, got the date-time \"2026-10-27T14:30:00Z\" (`date.date_in` gives its date)",
    );
    same_error("date.add_days(5, 1)", "`date.add_days` expects a date (YYYY-MM-DD) as argument 1, got int");
    same_error("date.add_days(null, 1)", "`date.add_days` expects a date (YYYY-MM-DD) as argument 1, got null");
    same_error("date.add_days()", "`date.add_days` expects a date (YYYY-MM-DD) as argument 1, got nothing");
    same_error(r#"date.add_days("2026-10-27", 1.5)"#, "`date.add_days` expects an int as argument 2, got float");
    same_is(
        "",
        r#"[date.months_between("2026-01-31", "2026-02-28"), date.months_between("2026-01-31", "2026-03-31"), date.months_between("2026-03-31", "2026-01-31"), date.months_between("2026-10-27", "2026-10-27"), date.months_between("2020-02-29", "2021-02-28"), date.months_between("0001-01-01", "9999-12-31"), date.months_between("2026-05-15", "2025-05-16"), date.months_between("9999-12-31", "9999-11-30")]"#,
        "[1, 2, -2, 0, 12, 119987, -11, -1]",
    );
    same_error(r#"date.months_between("2026-01-01", "x")"#, "`date.months_between` expects a date (YYYY-MM-DD) as argument 2, got \"x\"");
    same_error(r#"date.months_between(1, "2026-01-01")"#, "`date.months_between` expects a date (YYYY-MM-DD) as argument 1, got int");
}

#[test]
fn instants_go_to_and_from_milliseconds_and_zones() {
    if !node_available() {
        return;
    }
    same_is(
        "",
        r#"[date.date_in("2026-10-27T23:30:00-05:00", "UTC"), date.date_in("2026-10-27T01:30:00Z", "America/New_York"), date.date_in("2026-10-27T12:00:00Z", "Pacific/Kiritimati")]"#,
        r#"["2026-10-28", "2026-10-26", "2026-10-28"]"#,
    );
    same_error(r#"date.date_in("2026-10-27", "UTC")"#, "`date.date_in` expects a date-time (like 2026-10-27T14:30:00Z) as argument 1, got \"2026-10-27\"");
    same_is(
        "",
        r#"[date.ms("2026-10-27"), date.ms("2026-10-27T14:30:15.250Z"), date.ms("2026-10-27T14:30:15Z"), date.ms("1969-12-31T23:59:59.5Z"), date.ms("2026-10-27T14:30:15.1+01:00"), date.ms("0001-01-01")]"#,
        "[1793059200000, 1793111415250, 1793111415000, -500, 1793107815100, -62135596800000]",
    );
    same_error("date.ms(0)", "`date.ms` expects a date or a date-time as argument 1, got int");
    same_is(
        "",
        r#"[date.from_ms(0, "UTC"), date.from_ms(1793197815250, "UTC"), date.from_ms(-1, "UTC"), date.from_ms(1793197815250, "Asia/Kolkata"), date.from_ms(-62135596800000, "UTC")]"#,
        r#"["1970-01-01T00:00:00Z", "2026-10-28T14:30:15Z", "1969-12-31T23:59:59Z", "2026-10-28T20:00:15+05:30", "0001-01-01T00:00:00Z"]"#,
    );
    same_error(r#"date.from_ms(1.5, "UTC")"#, "`date.from_ms` expects an int as argument 1, got float");
    same_error(r#"date.from_ms(253402300800000, "UTC")"#, "`date.from_ms`: the date is out of range (years 1 to 9999)");
    same_error(r#"date.from_ms(0, "Nowhere/City")"#, "`date.from_ms`: unknown time zone \"Nowhere/City\"");
    same_is(
        "",
        r#"[date.compare("2026-10-27", "2026-10-27T00:00:00Z"), date.compare("2026-10-27T00:00:00.5Z", "2026-10-27T00:00:00Z"), date.compare("2026-10-27T01:00:00+01:00", "2026-10-27T00:00:00Z"), date.compare("2026-10-26", "2026-10-27"), date.compare("2026-10-27T00:00:00.123456789Z", "2026-10-27T00:00:00.12345679Z")]"#,
        "[0, 1, 0, -1, -1]",
    );
    same_is(
        "",
        r#"[date.to_zone("2026-10-27T14:30:00Z", "Asia/Tokyo"), date.to_zone("2026-10-27T14:30:00.500+01:00", "UTC"), date.at("2026-10-27", "14:30", "Europe/Paris"), date.at("2026-10-27", "14:30:15", "UTC"), date.at("2026-10-27", null, "UTC"), date.at("2026-10-27T14:30", "America/New_York")]"#,
        r#"["2026-10-27T23:30:00+09:00", "2026-10-27T13:30:00.500Z", "2026-10-27T14:30:00+01:00", "2026-10-27T14:30:15Z", "2026-10-27T00:00:00Z", "2026-10-27T14:30:00-04:00"]"#,
    );
    same_error(r#"date.at("2026-10-27", "1430", "UTC")"#, "`date.at` expects a time (HH:MM or HH:MM:SS) as argument 2, got \"1430\"");
    same_error(r#"date.to_zone("9999-12-31T23:30:00Z", "Asia/Tokyo")"#, "`date.to_zone`: the date is out of range (years 1 to 9999)");
}

#[test]
fn dates_are_written_in_a_locale_by_style_or_pattern() {
    if !node_available() {
        return;
    }
    same_is(
        "",
        r#"[date.format("2026-10-27", { locale: "de-DE" }), date.format("2026-10-27", { locale: "fr_FR", style: "full" }), date.format("2026-10-27", { locale: "pt-BR", style: "long" }), date.format("2026-10-27", { locale: "nl", style: "short" }), date.format("2026-10-27", { style: "medium" }), date.format("2026-10-27", {}), date.format("2026-10-27", { locale: "xx-YY" }), date.format("2026-10-27", { locale: " ES ", pattern: "dddd D MMMM" }), date.format("2026-10-27", { locale: null }), date.format("2026-10-27", { pattern: null, style: null })]"#,
        r#"["27.10.2026", "mardi 27 octobre 2026", "27 de outubro de 2026", "27-10-2026", "Oct 27, 2026", "Oct 27, 2026", "Oct 27, 2026", "martes 27 octubre", "Oct 27, 2026", "Oct 27, 2026"]"#,
    );
    // [brackets] are text as written, to the end when one is not closed.
    same_is(
        "",
        r#"[date.format("2026-10-27T14:05:09-04:00", { pattern: "[at] h:mm A Z [on] ddd" }), date.format("2026-10-27", { pattern: "[YYYY] YYYY [unclosed" }), date.format("2026-10-27", "[]D[é😀]"), date.format("2026-10-27T00:30:00Z", "hh h A HH H YY M D"), date.format("2026-10-27T12:30:00Z", "hh h A"), date.format("0005-01-02", "YYYY YY")]"#,
        r#"["at 2:05 PM -04:00 on Tue", "YYYY 2026 unclosed", "27é😀", "12 12 AM 00 0 26 10 27", "12 12 PM", "0005 05"]"#,
    );
    same_error(r#"date.format("2026-10-27", { locale: 5 })"#, "`date.format`: locale is a str, like \"de-DE\", got int");
    same_error(r#"date.format("2026-10-27", { zone: "UTC" })"#, "`date.format`: the options are locale, style and pattern, not `zone`");
    same_error(r#"date.format("2026-10-27", { style: "huge" })"#, "`date.format`: style is short, medium, long or full, got \"huge\"");
    same_error(r#"date.format("2026-10-27", { style: 5 })"#, "`date.format`: pattern and style are strs, got int");
    same_error(r#"date.format("2026-10-27", { pattern: 5 })"#, "`date.format`: pattern and style are strs, got int");
    same_error(r#"date.format("2026-10-27", 5)"#, "`date.format` expects a pattern (like \"DD/MM/YYYY\") or options as argument 2, got int");
    same_error(r#"date.format("2026-10-27", "HH:mm")"#, "`date.format`: the pattern has a time (HH), but \"2026-10-27\" is a date");
    same_error(r#"date.format("x", "YYYY")"#, "`date.format` expects a date or a date-time as argument 1, got \"x\"");
}

#[test]
fn the_pages_functions_have_fields_and_no_handler_is_set_from_quillo() {
    if !node_available() {
        return;
    }
    // A function of the page's object keeps its own fields: `window.Array.from` is called as it is.
    let w = "let w = js(\"globalThis\")\n";
    assert_eq!(
        browser("", &format!("{}[w.Array.length, w.Array.nope, w.Array.from([1, 2]), w.Math.max(1, 5), w.Array.isArray.name]", w)),
        "[1, null, [1, 2], 5, \"isArray\"]"
    );
    assert_eq!(browser("", &format!("{}w.Array.isArray([1])", w)), "true");
    for name in ["prototype", "constructor"] {
        assert_eq!(
            browser("", &format!("{}w.Array.{}", w, name)),
            format!("ERROR: `{}` is not used from Quillo code: it runs text as code or HTML", name)
        );
    }
    // A field of the page's object is set; a handler is not, given a function or text.
    let lib = "globalThis.lib = new (class L { constructor() { this.n = 1; } })();";
    assert_eq!(browser(lib, "let o = js(\"lib\")\no.n = 2\no.n"), "2");
    assert_eq!(
        browser(lib, "let o = js(\"lib\")\no.onclick = (e) => 1\no.n"),
        "ERROR: `onclick` is not set from Quillo code, as it would take text to run as code too: write x.addEventListener(\"click\", f), or @click on an element"
    );
    assert_eq!(
        browser(lib, "let o = js(\"lib\")\no.ONLOAD = (e) => 1\no.n"),
        "ERROR: `ONLOAD` is not set from Quillo code, as it would take text to run as code too: write x.addEventListener(\"LOAD\", f), or @LOAD on an element"
    );
    assert_eq!(browser(lib, "let o = js(\"lib\")\no.onclick = \"x\"\no.n"), "ERROR: `onclick` is not set from Quillo code: it runs text as code or HTML");
}

#[test]
fn uuid_without_the_browsers_own_is_still_a_version_4_uuid() {
    if !node_available() {
        return;
    }
    let body = "let u = uuid()\n[len(u), re.test(u, \"^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$\"), u == uuid()]";
    // With crypto.randomUUID, and on a page without it (an old browser, or not https).
    same_is("", body, "[36, true, false]");
    // (`node -e` has its own `crypto` module in scope, so the page's is put in its place.)
    let gone = "var crypto = undefined;";
    assert_eq!(browser(gone, body), "[36, true, false]");
    let no_fn = "var crypto = {};";
    assert_eq!(browser(no_fn, body), "[36, true, false]");
    // Many of them, all different, the variant digit always 8, 9, a or b.
    let many = "let us = range(0, 200) | map((i) => uuid())\n[len(unique(us)), all(us, (u) => contains(\"89ab\", slice(u, 19, 20)) && slice(u, 14, 15) == \"4\")]";
    assert_eq!(browser(gone, many), "[200, true]");
}

#[test]
fn markdown_that_cannot_reach_the_server_is_an_error_value() {
    if !node_available() {
        return;
    }
    // The server makes it; the browser asks the server, and says why when it cannot.
    let (s, _) = both_with("", "", "markdown(\"# Hi\")");
    assert_eq!(s, "<h1>Hi</h1>\n");
    let offline = "globalThis.fetch = async () => { throw new Error(\"offline\"); };";
    assert_eq!(browser(offline, "markdown(\"# Hi\")"), "err(\"markdown could not be made: offline\")");
    assert_eq!(browser(offline, "is_err(markdown(\"\", { safe: true }))"), "true");
    same_error("markdown(\"# Hi\", 5)", "`markdown` expects options as a map, got int");
    same_error("markdown(5)", "`markdown` expects a str as argument 1, got int");
}
