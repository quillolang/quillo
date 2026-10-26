//! Decimals (src/decimal.rs): exact numbers, `12.50m`. What they give on
//! the server, in the checker, in the database and, compiled, in the browser.

use quillo::eval_str;
use std::process::Command;

fn ev(src: &str) -> String {
    match eval_str(src) {
        Ok(v) => v.repr(),
        Err(e) => panic!("{}\n--- source ---\n{}", e, src),
    }
}

fn ev_err(src: &str) -> String {
    match eval_str(src) {
        Ok(v) => panic!("expected an error, got {}\n--- source ---\n{}", v.repr(), src),
        Err(e) => e.to_string(),
    }
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
        Ok(v) | Err(quillo::interp::Ctrl::Return(v)) => v.repr(),
        Err(quillo::interp::Ctrl::Error(e)) => format!("ERROR: {}", e.msg),
        Err(quillo::interp::Ctrl::Reply(_)) => "REPLY".into(),
    };
    (it, out)
}

/// `body` run on the server and, compiled, in Node: what each gives.
fn both(body: &str) -> (String, String) {
    let (it, server) = server(body);
    let js = quillo::client::test_script(&it, "__t").unwrap_or_else(|e| panic!("{}\n{}", e, body));
    let out = Command::new("node").arg("-e").arg(&js).output().expect("node");
    assert!(out.status.success(), "node failed: {}\n{}", String::from_utf8_lossy(&out.stderr), js);
    (server, String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The same answer on the server and in the browser, and it is `want`.
fn same(body: &str, want: &str) {
    assert_eq!(server(body).1, want, "server: {}", body);
    if node_available() {
        let (s, b) = both(body);
        assert_eq!(s, b, "browser differs: {}", body);
    }
}

#[test]
fn literals_keep_their_digits() {
    same("[12.50m, 5m, -0.01m, 0.0m, 1e3m, 2.5e-3m]", "[12.50m, 5m, -0.01m, 0.0m, 1000m, 0.0025m]");
    same("[str(12.50m), `${-0.05m}`, type_of(1m)]", "[\"12.50\", \"-0.05\", \"dec\"]");
    same("[0.1m + 0.2m == 0.3m, 0.1m + 0.2m, 12.5m == 12.50m, 1m == 1, 0.1m == 0.1]", "[true, 0.3m, true, true, true]");
}

#[test]
fn arithmetic_is_exact() {
    same("[1.10m + 2.205m, 10m - 0.01m, 1.10m * 3, 3 * 0.5m, -1.5m * -2m, 7.5m % 2, -7.5m % 2]", "[3.305m, 9.99m, 3.30m, 1.5m, 3.0m, 1.5m, -1.5m]");
    same("let total = 0m\nfor x in [0.1m, 0.1m, 0.1m] { total += x }\ntotal", "0.3m");
    same("[sum([0.1m, 0.2m, 0.3m]), sum([1, 2.50m]), sum([])]", "[0.6m, 3.50m, 0]");
    same("[min([3m, 1.5m, 2]), max([3m, 1.5m, 2]), abs(-2.50m), -(1.5m)]", "[1.5m, 3m, 2.50m, -1.5m]");
    same("[1.5m < 2, 2 <= 2.0m, 0.1m > 0.09m, -1m < -0.5m, sort([3m, 1, 2.5m])]", "[true, true, true, true, [1, 2.5m, 3m]]");
    same("let big = 79228162514264337593543950335m\nbig + 1", "ERROR: decimal overflow");
    same("1m / 0", "ERROR: division by zero");
    same("1.5m % 0m", "ERROR: division by zero");
}

#[test]
fn division_rounds_half_even_to_28_digits() {
    same(
        "[1m / 3, 2m / 3, 10m / 3, 100m / 3m, 10m / 4, 1.00m / 2, 100m / 0.5m, -7m / 2]",
        "[0.3333333333333333333333333333m, 0.6666666666666666666666666667m, 3.3333333333333333333333333333m, 33.333333333333333333333333333m, 2.5m, 0.50m, 200m, -3.5m]",
    );
    // One third, three times, is not one; rounded to cents it is.
    same("let third = 1m / 3\nlet x = third * 3\n[x, round(x, 2)]", "[0.9999999999999999999999999999m, 1.00m]");
}

#[test]
fn every_rounding_mode() {
    let modes = ["half_even", "half_up", "half_down", "up", "down", "ceiling", "floor"];
    let cases = [
        ("2.5m", ["2", "3", "2", "3", "2", "3", "2"]),
        ("-2.5m", ["-2", "-3", "-2", "-3", "-2", "-2", "-3"]),
        ("3.5m", ["4", "4", "3", "4", "3", "4", "3"]),
        ("-3.5m", ["-4", "-4", "-3", "-4", "-3", "-3", "-4"]),
        ("2.51m", ["3", "3", "3", "3", "2", "3", "2"]),
        ("-2.49m", ["-2", "-2", "-2", "-3", "-2", "-2", "-3"]),
        ("-0.4m", ["0", "0", "0", "-1", "0", "0", "-1"]),
    ];
    for (x, want) in cases {
        let body = format!("[{}]", modes.iter().map(|m| format!("round({}, 0, \"{}\")", x, m)).collect::<Vec<_>>().join(", "));
        let want = format!("[{}]", want.iter().map(|w| format!("{}m", w)).collect::<Vec<_>>().join(", "));
        same(&body, &want);
    }
    // To cents: ties at the third decimal, both signs.
    same(
        "[round(1.005m, 2), round(1.015m, 2), round(1.005m, 2, \"half_up\"), round(-1.005m, 2, \"half_up\"), round(-1.005m, 2, \"half_down\"), round(-1.001m, 2, \"floor\"), round(1.001m, 2, \"ceiling\")]",
        "[1.00m, 1.02m, 1.01m, -1.01m, -1.00m, -1.01m, 1.01m]",
    );
    // Fewer decimals than asked for: padded, so amounts show as cents.
    same("[round(12.5m, 2), round(12m, 2), round(1250m, -2), round(1350m, -2), round(2.5m), floor(-2.5m), ceil(-2.5m), floor(2.99m), ceil(2.01m)]",
        "[12.50m, 12.00m, 1200m, 1400m, 2m, -3m, -2m, 2m, 3m]");
    same("round(1.5m, 0, \"sideways\")", "ERROR: `round`: unknown rounding \"sideways\" (one of half_even, half_up, half_down, up, down, ceiling or floor)");
    same("round(1.5, 0, \"half_up\")", "ERROR: `round` takes a rounding mode for a dec only: convert with dec(x)");
}

#[test]
fn a_dec_and_a_float_do_not_mix() {
    let hint = "a dec is exact and a float is not, so convert one with dec(x) or float(x)";
    for body in ["1.5m + 0.5", "0.5 * 2m", "1m - 0.25", "1m / 0.5", "1m < 0.5", "max([1m, 0.5])"] {
        let (s, b) = if node_available() { both(body) } else { (server(body).1, String::new()) };
        assert!(s.contains(hint), "{}: {}", body, s);
        if node_available() {
            assert!(b.contains(hint), "browser, {}: {}", body, b);
        }
    }
    // Converted, they do.
    same("[1.5m + dec(0.5), float(1.5m) + 0.25, int(-2.75m), float(0.1m) == 0.1]", "[2.0m, 1.75, -2, true]");
    // Strings concatenate as with every value.
    same("\"total: \" + 12.50m", "\"total: 12.50\"");
    assert!(ev_err("math.sqrt(2m)").contains("convert a dec with float(x)"));
    assert!(ev_err("stats.mean([1m, 2m])").contains("convert a dec with float(x)"));
    assert!(ev_err("sqrt(2m)").contains("convert a dec with float(x)"));
}

#[test]
fn dec_converts_exactly() {
    same(
        "[dec(\"12.50\"), dec(\" -0.05 \"), dec(\"1e-3\"), dec(42), dec(0.1), dec(0.1) == 0.1m, dec(1.5m), dec(2.0)]",
        "[12.50m, -0.05m, 0.001m, 42m, 0.1m, true, 1.5m, 2m]",
    );
    same("[dec(\"abc\"), dec(\"1.2.3\"), dec(true), dec(\"0.00000000000000000000000000001\")]",
        "[err(\"not a decimal: \\\"abc\\\"\"), err(\"not a decimal: \\\"1.2.3\\\"\"), err(\"cannot convert bool to dec\"), err(\"not a decimal: \\\"0.00000000000000000000000000001\\\"\")]");
    assert_eq!(ev("dec(1e300)"), "err(\"cannot convert float to dec: it is too large, or not a number\")");
    same("[int(12.99m), int(-12.99m), float(12.5m), str(-0.50m), type_of(dec(\"1\"))]", "[12, -12, 12.5, \"-0.50\", \"dec\"]");
}

#[test]
fn as_map_keys_trailing_zeros_do_not_count() {
    same("let m = set({}, 12.50m, \"a\")\n[m, get(m, 12.5m), has(m, 12.500m)]", "[{ \"12.5\": \"a\" }, \"a\", true]");
    same("group_by([1.50m, 1.5m, 2.25m], (x) => x)", "{ \"1.5\": [1.50m, 1.5m], \"2.25\": [2.25m] }");
    same("unique([1.50m, 1.5m, 2m])", "[1.50m, 2m]");
}

#[test]
fn literals_that_do_not_fit_are_refused() {
    assert!(ev_err("0.00000000000000000000000000001m").contains("does not fit"));
    assert!(ev_err("79228162514264337593543950336m").contains("does not fit"));
    // `m` right after a number, and only there: a name can still start with m.
    assert_eq!(ev("let m = 2\nlet mm = 3\n[5 * m, 1m, mm]"), "[10, 1m, 3]");
}

#[test]
fn patterns_convert_to_dec() {
    same(
        "[\"12.50\", 3, 1.25, 2.5m, \"x\", true] | map((v) => match v { dec as d => d, _ => null })",
        "[12.50m, 3m, 1.25m, 2.5m, null, null]",
    );
    same("match 2.50m { 2.5m => \"yes\", _ => \"no\" }", "\"yes\"");
    same("match -1.5m { -1.5m => \"neg\", _ => \"no\" }", "\"neg\"");
    same("let { amount: dec } = { amount: \"9.99\" }\namount", "9.99m");
}

const SHOP: &str = r#"
type Line = { sku: str, price: dec, qty: int = 1 }
type Order = { lines: list<Line>, tip: dec? }

fn total(o: Order) -> dec = sum(o.lines | map((l) => l.price * l.qty)) + (o.tip ?? 0m)

route GET /pay/:amount(dec) = { amount, doubled: amount * 2 }
route GET /quote ? { amount: dec, rate: dec = 0.2m } = { amount, tax: round(amount * rate, 2) }
route POST /orders { json: Order } = { total: total(json), lines: len(json.lines) }
route POST /orders = respond(422, "not an order")
server fn add(a: dec, b: dec) -> dec = a + b
server fn price() = { amount: 12.50m, big: 9007199254740993, list: [0.1m, 0.2m] }
server fn order_total(o: Order) -> dec = total(o)
server fn line(l: Line) = l
"#;

fn shop() -> quillo::Interp {
    let mut it = quillo::Interp::new(env!("CARGO_MANIFEST_DIR"));
    it.load_source(SHOP, "shop.qwe").unwrap_or_else(|e| panic!("{}", e));
    it
}

fn send(it: &mut quillo::Interp, method: &str, url: &str, body: &str) -> quillo::server::HttpResponse {
    let mut headers = vec![("Host".to_string(), "localhost".to_string())];
    if !body.is_empty() {
        headers.push(("Content-Type".into(), "application/json".into()));
    }
    it.handle(&quillo::server::HttpRequest { method: method.into(), url: url.into(), headers, body: body.as_bytes().to_vec(), remote: "127.0.0.1".into() })
}

#[test]
fn records_routes_and_json() {
    assert_eq!(
        ev(&format!("{}\nLine({{ sku: \"a\", price: \"12.50\" }})", SHOP)),
        "{ sku: \"a\", price: 12.50m, qty: 1 }"
    );
    assert_eq!(ev(&format!("{}\nLine({{ sku: \"a\", price: 3 }}).price", SHOP)), "3m");
    assert_eq!(ev(&format!("{}\nLine({{ sku: \"a\", price: \"cheap\" }})", SHOP)), "err(\"Line: `price` must be dec, got str \\\"cheap\\\"\")");
    let mut it = shop();
    assert_eq!(send(&mut it, "GET", "/pay/12.50", "").text(), r#"{"amount":"12.50","doubled":"25.00"}"#);
    assert_eq!(send(&mut it, "GET", "/pay/abc", "").status, 404);
    assert_eq!(send(&mut it, "GET", "/quote?amount=19.99", "").text(), r#"{"amount":"19.99","tax":"4.00"}"#);
    assert_eq!(send(&mut it, "GET", "/quote?amount=10&rate=0.075", "").text(), r#"{"amount":"10","tax":"0.75"}"#);
    // From JSON: text exactly, numbers through their shortest text.
    let r = send(&mut it, "POST", "/orders", r#"{"lines": [{"sku": "a", "price": "0.10", "qty": 3}, {"sku": "b", "price": 0.2}], "tip": 1}"#);
    assert_eq!(r.text(), r#"{"total":"1.50","lines":2}"#);
    assert_eq!(send(&mut it, "POST", "/orders", r#"{"lines": [{"sku": "a", "price": "free"}]}"#).status, 422);
    // json.stringify writes decimals as text; json.parse makes no decimals.
    assert_eq!(ev("json.stringify({ a: 1.50m, b: [0.1m] })"), r#""{\"a\":\"1.50\",\"b\":[\"0.1\"]}""#);
    assert_eq!(ev("json.parse(\"{\\\"a\\\": 1.50}\").a"), "1.5");
}

#[test]
fn server_fns_keep_decimals_both_ways() {
    let mut it = shop();
    let rpc = |it: &mut quillo::Interp, name: &str, body: &str| send(it, "POST", &format!("/_quillo/rpc/{}", name), body).text();
    // What a page sends: tagged decimals, or plain numbers and text for a `dec` parameter.
    assert_eq!(rpc(&mut it, "add", r#"[{"$dec": "0.10"}, {"$dec": "0.20"}]"#), r#"{"ok":{"$dec":"0.30"}}"#);
    assert_eq!(rpc(&mut it, "add", r#"["0.1", 0.2]"#), r#"{"ok":{"$dec":"0.3"}}"#);
    assert!(rpc(&mut it, "add", r#"["x", 1]"#).contains("must be dec"));
    // What it answers: decimals tagged, an int past 2^53 too, so the page gets them whole.
    assert_eq!(
        rpc(&mut it, "price", "[]"),
        r#"{"ok":{"amount":{"$dec":"12.50"},"big":{"$int":"9007199254740993"},"list":[{"$dec":"0.1"},{"$dec":"0.2"}]}}"#
    );    // A record argument: its `dec` fields made decimals, tagged or not, as
    // `Line(v)` makes them (a default put in, other fields left out).
    assert_eq!(
        rpc(&mut it, "order_total", r#"[{"lines": [{"sku": "a", "price": {"$dec": "0.10"}, "qty": 3}, {"sku": "b", "price": "0.20"}], "tip": 1}]"#),
        r#"{"ok":{"$dec":"1.50"}}"#
    );
    assert_eq!(rpc(&mut it, "line", r#"[{"sku": "a", "price": "1.5", "is_admin": true}]"#), r#"{"ok":{"sku":"a","price":{"$dec":"1.5"},"qty":1}}"#);
    assert!(rpc(&mut it, "order_total", r#"[{"lines": [{"sku": "a", "price": "free"}]}]"#).contains("must be Order"));
}

#[test]
fn sqlite_keeps_decimals() {
    let src = r#"
db.open(":memory:")
db.exec("create table t (id integer, price decimal(12,2), exact decimal_text, note text, n numeric)")
db.exec("insert into t values (1, ?, ?, ?, 2.5)", 12.50m, 0.10m, 3.5m)
db.exec("insert into t values (2, ?, ?, ?, 3)", 7, "12345678901234567890.123456789", null)
let rows = db.query("select * from t order by id")
[rows[0].price, rows[0].exact, rows[0].note, rows[1].price, rows[1].exact, db.one("select sum(price) as s from t").s, rows[0].n, rows[1].n]
"#;
    // `decimal(12,2)` has NUMERIC affinity: SQLite keeps 12.50 as the real
    // 12.5. `decimal_text` keeps the text; a `text` column gives text back,
    // and a bare `numeric` (SQLite's name for any number) numbers.
    assert_eq!(ev(src), "[12.5m, 0.10m, \"3.5\", 7m, 12345678901234567890.123456789m, 19.5, 2.5, 3]");
}

#[test]
fn compiled_functions_leave_decimals_to_the_interpreter() {
    let src = "fn f(a, b) = a * b + 1\nlet x = f(2, 3)\nlet y = f(2, 3)\nlet z = f(0.1m, 3)\nlet w = f(0.1m, 3)\n[x, y, z, w]";
    assert_eq!(ev(src), "[7, 7, 1.3m, 1.3m]");
    let src = "fn g(n) { let t = 0m\nfor i in 0..n { t = t + 0.1m }\nt }\n[g(10), g(10), g(3)]";
    assert_eq!(ev(src), "[1.0m, 1.0m, 0.3m]");
}

#[test]
fn ints_past_2_53_reach_the_page_whole() {
    same("let n = 9007199254740993\n[n, n + 1, n - 2, n * 1, str(n), type_of(n), n > 9007199254740992]",
        "[9007199254740993, 9007199254740994, 9007199254740991, 9007199254740993, \"9007199254740993\", \"int\", true]");
    same("9223372036854775807 + 1", "ERROR: integer overflow");
    same("[dec(9007199254740993), 9007199254740993 + 0.5m]", "[9007199254740993m, 9007199254740993.5m]");
}

fn diags(src: &str) -> Vec<String> {
    quillo::check::check_source(src, "t.qwe", std::path::Path::new(".")).diagnostics.iter().map(|d| format!("{} {}", d.line, d.message)).collect()
}

#[test]
fn the_checker_knows_decimals() {
    let clean = [
        "let a = 1.5m + 1\nlet b = a * 2 - 0.5m\nprint(b / 3, b % 2, -b, b > 1, b == 1.5)",
        "fn fee(amount: dec) -> dec = round(amount * 0.029m + 0.30m, 2, \"half_up\")\nprint(fee(10m))",
        "let x = dec(\"12.50\") ?? 0m\nprint(x + 1m)",
        "print(sum([1.5m, 2m]) + 1, abs(-1m) + 1m, floor(2.5m) + 1, int(2.5m) + 1, float(2.5m) + 0.5)",
        "type Line = { price: dec, qty: int = 1 }\nfn total(ls: list<Line>) -> dec = sum(ls | map((l) => l.price * l.qty))\nprint(total([]))",
        "match 1 { dec as d => print(d + 1m), _ => null }",
    ];
    for src in clean {
        let d = diags(src);
        assert!(d.is_empty(), "{:?}\n{}", d, src);
    }
    let hint = "a dec is exact and a float is not, so convert one with dec(x) or float(x)";
    for (src, want) in [
        ("print(1.5m + 0.5)", "cannot add dec and float"),
        ("let x = 2.0\nprint(x * 1m)", "cannot multiply float and dec"),
        ("print(1m < 0.5)", "cannot compare dec with float"),
        ("print(abs(-1.5m) + 0.5)", "cannot add dec and float"),
        ("print(sum([1m, 2m]) - 0.5)", "cannot subtract dec and float"),
    ] {
        let d = diags(src);
        assert!(d.len() == 1 && d[0].contains(want) && d[0].contains(hint), "{:?}\n{}", d, src);
    }
    let d = diags("fn f() -> int = 1.5m");
    assert!(d.len() == 1 && d[0].contains("dec"), "{:?}", d);
    let d = diags("fn f(x: dec) = x\nprint(f(\"1\"))");
    assert!(d.len() == 1 && d[0].contains("dec"), "{:?}", d);
}
