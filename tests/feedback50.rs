//! FEEDBACK50.md: money and the other finance types (branch feedback50-fixes). Q-620, `+` and
//! `-` on two money values; Q-622 and Q-623, one reading of a money map for every function;
//! Q-630, withdrawn currencies; Q-631, allocation by the largest remainders. Each case runs on
//! the server and, compiled, in Node: the browser must give the same answer.

mod support;

use std::path::PathBuf;
use std::process::Command;

fn node_available() -> bool {
    Command::new("node").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

/// Runs `js` in Node, given on its standard input.
fn node(js: &str) -> std::process::Output {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new("node").arg("-").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().expect("node");
    child.stdin.take().unwrap().write_all(js.as_bytes()).expect("node's input");
    child.wait_with_output().expect("node")
}

/// `body` run on the server as a function's body: what it gives, or "ERROR: " and the message.
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

/// The same answer on the server and in Node, and it is `want`.
#[track_caller]
fn same(body: &str, want: &str) {
    let (it, s) = server(body);
    assert_eq!(s, want, "server: {}", body);
    if node_available() {
        let js = quillo::client::test_script(&it, "__t").unwrap_or_else(|e| panic!("{}\n{}", e, body));
        let out = node(&js);
        assert!(out.status.success(), "node failed: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), s, "browser differs: {}", body);
    }
}

fn dir(name: &str, src: &str) -> PathBuf {
    let d = support::temp_dir(&format!("feedback50-{}", name));
    std::fs::write(d.join("main.qwe"), src).unwrap();
    d
}

/// What `quillo check` says of a program.
fn check(src: &str) -> String {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let d = dir(&format!("check{}", N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)), src);
    let o = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("check").arg(d.join("main.qwe")).output().unwrap();
    String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)
}

const USD10: &str = "money.of(10, \"USD\")";
const USD5: &str = "money.of(5, \"USD\")";

// Q-620: `+` merged the two maps, so `total + line` was the line, and dollars plus euros
// were the euros.
#[test]
fn money_plus_money_adds_the_amounts() {
    same(&format!("{USD10} + {USD5}"), "{ amount: 15.00m, currency: \"USD\" }");
    same(&format!("{USD10} - {USD5}"), "{ amount: 5.00m, currency: \"USD\" }");
    same(&format!("{USD5} - {USD10}"), "{ amount: -5.00m, currency: \"USD\" }");
    same("money.zero(\"JPY\") + money.of(1235, \"JPY\")", "{ amount: 1235m, currency: \"JPY\" }");
    same("money.of(\"0.001\", \"BHD\") + money.of(\"0.009\", \"BHD\")", "{ amount: 0.010m, currency: \"BHD\" }");
    same("money.zero(\"USD\") - money.zero(\"USD\")", "{ amount: 0.00m, currency: \"USD\" }");
    // The cart from the report: a running total.
    same(
        "let total = money.zero(\"USD\")\nfor l in [{ price: money.of(10, \"USD\"), qty: 2 }, { price: money.of(5, \"USD\"), qty: 1 }] {\n  total = total + money.mul(l.price, l.qty)\n}\ntotal",
        "{ amount: 25.00m, currency: \"USD\" }",
    );
    same("let t = money.of(1, \"EUR\")\nt += money.of(2, \"EUR\")\nt -= money.of(0.5m, \"EUR\")\nt", "{ amount: 2.50m, currency: \"EUR\" }");
    // What JSON brings back, the amount as text, adds too.
    same("let m = money.of(19.90m, \"USD\")\njson.parse(json.stringify(m)) + m", "{ amount: 39.80m, currency: \"USD\" }");
    // A code written in lower case, or with spaces, is the currency.
    same("{ amount: 1, currency: \" usd \" } + money.of(1, \"USD\")", "{ amount: 2.00m, currency: \"USD\" }");
}

#[test]
fn money_of_two_currencies_does_not_add() {
    same(&format!("{USD10} + money.of(2, \"EUR\")"), "err(\"cannot add USD and EUR: convert one currency to the other first\")");
    same(&format!("money.of(2, \"EUR\") - {USD10}"), "err(\"cannot subtract EUR and USD: convert one currency to the other first\")");
    // Past what money.of takes: an error value, as from money.add.
    same("money.of(\"92233720368547758.07\", \"USD\") + money.of(\"0.01\", \"USD\")", "err(\"92233720368547758.08 USD is too large an amount\")");
    same("money.of(\"-92233720368547758.08\", \"USD\") - money.of(\"0.01\", \"USD\")", "err(\"-92233720368547758.09 USD is too large an amount\")");
    same("money.of(\"92233720368547758.07\", \"USD\") + money.zero(\"USD\")", "{ amount: 92233720368547758.07m, currency: \"USD\" }");
}

#[test]
fn other_maps_still_merge() {
    same("{ a: 1 } + { b: 2 }", "{ a: 1, b: 2 }");
    same("{ amount: 1, currency: \"USD\", note: \"x\" } + money.of(10, \"USD\")", "{ amount: 10.00m, currency: \"USD\", note: \"x\" }");
    same("{ amount: 1m } + { amount: 2m }", "{ amount: 2m }");
    same("money.of(10, \"USD\") + { currency: \"EUR\" }", "{ amount: 10.00m, currency: \"EUR\" }");
    same("{} + {}", "{}");
    same("{ a: 1 } - { a: 1 }", "ERROR: cannot subtract map and map");
}

#[test]
fn money_shaped_maps_are_held_to_money() {
    same("{ amount: 1, currency: \"XYZ\" } + { amount: 1, currency: \"XYZ\" }",
        "ERROR: `+` expects money ({ amount, currency }) as argument 1, got map { amount: 1, currency: \"XYZ\" }: unknown currency");
    same("money.of(1, \"USD\") - { amount: 1.005m, currency: \"USD\" }",
        "ERROR: `-`: 1.005 has more decimals than USD has (2): round it first with money.round(m, mode)");
    same("money.of(1, \"USD\") + { amount: true, currency: \"USD\" }",
        "ERROR: `+` expects money ({ amount, currency }) as argument 2, got map { amount: true, currency: \"USD\" }: its amount must be a dec");
}

#[test]
fn the_checker_takes_money_minus_money() {
    let ok = check(
        "fn cart(lines: list) = {\n  let total = money.zero(\"USD\") ?? money.of(0, \"USD\")\n  for l in lines {\n    total = total + money.mul(l.price, l.qty)\n  }\n  total\n}\nlet a = money.of(10, \"USD\")\nlet b = money.of(5, \"USD\")\nprint(a - b, a + b, cart([]))\n",
    );
    assert!(ok.contains("ok"), "{}", ok);
    assert!(!ok.contains("cannot subtract"), "{}", ok);
    // The answer is money, or an error value for two currencies.
    let typed = check("let a = money.of(10, \"USD\") ?? reply 400\nlet b = money.of(5, \"USD\") ?? reply 400\nfn want_int(n: int) = n\nwant_int(a - b)\n");
    assert!(typed.contains("map | error"), "{}", typed);
    // Maps of other shapes: `-` is still refused, `+` still merges.
    let other = check("let x = { a: 1 } - { a: 2 }\n");
    assert!(other.contains("cannot subtract"), "{}", other);
    assert!(check("let x = { a: 1 } + { b: 2 }\nprint(x.b)\n").contains("ok"));
}

// Q-622: some functions rounded $1.005, others took it as it was. Now none take it.
#[test]
fn an_amount_with_more_decimals_than_its_currency_is_refused_by_every_function() {
    let raw = "{ amount: 1.005m, currency: \"USD\" }";
    for (call, f) in [
        (format!("money.compare({raw}, {USD5})"), "money.compare"),
        (format!("money.is_zero({raw})"), "money.is_zero"),
        (format!("money.neg({raw})"), "money.neg"),
        (format!("money.add({raw}, {USD5})"), "money.add"),
        (format!("money.sub({USD5}, {raw})"), "money.sub"),
        (format!("money.mul({raw}, 2)"), "money.mul"),
        (format!("money.to_minor({raw})"), "money.to_minor"),
        (format!("money.format({raw}, \"en-US\")"), "money.format"),
        (format!("money.allocate({raw}, 2)"), "money.allocate"),
    ] {
        same(&call, &format!("ERROR: `{f}`: 1.005 has more decimals than USD has (2): round it first with money.round(m, mode)"));
    }
    same("money.is_zero({ amount: 0.001m, currency: \"USD\" })", "ERROR: `money.is_zero`: 0.001 has more decimals than USD has (2): round it first with money.round(m, mode)");
    same("money.is_zero({ amount: 1m, currency: \"JPY\" })", "false");
    same("money.neg({ amount: 0.5m, currency: \"JPY\" })", "ERROR: `money.neg`: 0.5 has more decimals than JPY has (0): round it first with money.round(m, mode)");
    // money.round is how such an amount becomes money.
    same(&format!("money.round({raw}, \"half_up\")"), "{ amount: 1.01m, currency: \"USD\" }");
    same(&format!("money.round({raw})"), "{ amount: 1.00m, currency: \"USD\" }");
    same("money.round({ amount: \"2.675\", currency: \"usd\" }, \"half_up\")", "{ amount: 2.68m, currency: \"USD\" }");
    same("money.round({ amount: 92233720368547758.075m, currency: \"USD\" }, \"up\")", "err(\"92233720368547758.08 USD is too large an amount\")");
    // Zeros past the currency's decimals are not more decimals; the amount is the currency's.
    same("money.compare({ amount: 1.500m, currency: \"USD\" }, money.of(1.5m, \"USD\"))", "0");
    same("money.neg({ amount: 1.500m, currency: \"USD\" })", "{ amount: -1.50m, currency: \"USD\" }");
    same("money.neg({ amount: \"7\", currency: \"BHD\" })", "{ amount: -7.000m, currency: \"BHD\" }");
    same("money.to_minor({ amount: -3, currency: \"USD\" })", "-300");
}

// Q-623: money.of refused "1e3" and amounts past an int's minor units; a map did not.
#[test]
fn a_money_map_is_read_as_money_of_reads_an_amount() {
    for text in ["1e3", "1E3", "12.5e1", "abc", "", " ", "1,000.00", "$5"] {
        same(
            &format!("money.add({{ amount: {:?}, currency: \"USD\" }}, money.zero(\"USD\"))", text),
            &format!("ERROR: `money.add` expects money ({{ amount, currency }}) as argument 1, got map {{ amount: {:?}, currency: \"USD\" }}: its amount {:?} is not an amount", text, text),
        );
    }
    same("money.add({ amount: \" 5 \", currency: \"USD\" }, money.zero(\"USD\"))", "{ amount: 5.00m, currency: \"USD\" }");
    same("money.add({ amount: \"+5\", currency: \"USD\" }, money.zero(\"USD\"))", "{ amount: 5.00m, currency: \"USD\" }");
    same("money.add({ amount: \"-.5\", currency: \"USD\" }, money.zero(\"USD\"))", "{ amount: -0.50m, currency: \"USD\" }");
    same("money.add({ amount: 19.99, currency: \"USD\" }, money.zero(\"USD\"))",
        "ERROR: `money.add` expects money ({ amount, currency }) as argument 1, got map { amount: 19.99, currency: \"USD\" }: its amount is a float: make it with money.of(dec(x), currency)");
    // The minor units: the last that fits, and the first that does not.
    same("money.to_minor({ amount: \"92233720368547758.07\", currency: \"USD\" })", "9223372036854775807");
    same("money.to_minor({ amount: \"-92233720368547758.08\", currency: \"USD\" })", "-9223372036854775808");
    same("money.to_minor({ amount: \"92233720368547758.08\", currency: \"USD\" })", "ERROR: `money.to_minor`: 92233720368547758.08 USD is too large an amount");
    same("money.compare({ amount: 9223372036854775808m, currency: \"JPY\" }, money.zero(\"JPY\"))", "ERROR: `money.compare`: 9223372036854775808 JPY is too large an amount");
    same("money.neg(money.of(\"-92233720368547758.08\", \"USD\"))", "err(\"92233720368547758.08 USD is too large an amount\")");
    same("money.mul(money.of(\"92233720368547758.07\", \"USD\"), 2)", "err(\"184467440737095516.14 USD is too large an amount\")");
    same("money.sub(money.of(\"-92233720368547758.08\", \"USD\"), money.of(1, \"USD\"))", "err(\"-92233720368547759.08 USD is too large an amount\")");
}

// Q-630: an invoice kept in leva was an error in every money function once BGN left the table.
#[test]
fn withdrawn_currencies_still_read() {
    same("[money.minor_units(\"BGN\"), money.minor_units(\"hrk\"), money.minor_units(\"BYR\"), money.minor_units(\"ZWL\"), money.minor_units(\"XYZ\")]",
        "[2, 2, 0, 2, err(\"unknown currency \\\"XYZ\\\"\")]");
    same("money.of(\"120.50\", \" bgn \")", "{ amount: 120.50m, currency: \"BGN\" }");
    same("money.of(\"1.5\", \"BYR\")", "err(\"1.5 has more decimals than BYR has (0); give a rounding mode to round it\")");
    same("money.format({ amount: \"1234.5\", currency: \"BGN\" }, \"en-US\")", "\"BGN\u{a0}1,234.50\"");
    same("money.add({ amount: 120, currency: \"HRK\" }, money.from_minor(50, \"HRK\"))", "{ amount: 120.50m, currency: \"HRK\" }");
    same("money.of(1, \"BTC\")", "err(\"unknown currency \\\"BTC\\\"\")");
}

// Q-631: the units left went to the first shares, so a 1% share of $0.10 got 10%.
#[test]
fn allocation_gives_the_units_left_to_the_largest_remainders() {
    let amounts = |m: &str, r: &str| format!("money.allocate({m}, {r}) | map((x) => x.amount)");
    same(&amounts("money.of(0.10m, \"USD\")", "[1, 99]"), "[0.00m, 0.10m]");
    same(&amounts("money.of(0.10m, \"USD\")", "[99, 1]"), "[0.10m, 0.00m]");
    same(&amounts("money.of(1, \"USD\")", "[1, 1, 98]"), "[0.01m, 0.01m, 0.98m]");
    same(&amounts("money.of(0.01m, \"USD\")", "[1, 2]"), "[0.00m, 0.01m]");
    same(&amounts("money.of(0.01m, \"USD\")", "[2, 1]"), "[0.01m, 0.00m]");
    // Equal remainders: the first shares, as before.
    same(&amounts("money.of(100, \"USD\")", "3"), "[33.34m, 33.33m, 33.33m]");
    same(&amounts("money.of(0.05m, \"USD\")", "[3, 7]"), "[0.02m, 0.03m]");
    same(&amounts("money.of(-100, \"USD\")", "[1, 1, 1]"), "[-33.34m, -33.33m, -33.33m]");
    same(&amounts("money.of(-0.10m, \"USD\")", "[1, 99]"), "[0.00m, -0.10m]");
    // A share with no ratio gets nothing, however many units are left.
    same(&amounts("money.of(10, \"JPY\")", "[0, 1, 1, 1]"), "[0m, 4m, 3m, 3m]");
    same(&amounts("money.of(0.02m, \"USD\")", "[0, 1, 0, 1, 1]"), "[0.00m, 0.01m, 0.00m, 0.01m, 0.00m]");
    same(&amounts("money.of(0.07m, \"USD\")", "[0.333m, 0.333m, 0.334m]"), "[0.02m, 0.02m, 0.03m]");
    same(&amounts("money.of(5, \"USD\")", "[1]"), "[5.00m]");
    same(&amounts("money.zero(\"USD\")", "[1, 2]"), "[0.00m, 0.00m]");
    same("let parts = money.allocate(money.of(1234.57m, \"USD\"), [17, 31, 52])\nsum(parts | map((m) => m.amount))", "1234.57m");
    same("len(money.allocate(money.of(1, \"USD\"), 100000) | filter((m) => m.amount > 0))", "100");
}

/// What `quillo run` prints for a script, under a deadline.
fn run_script(d: &std::path::Path) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(d.join("main.qwe"))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            panic!("the script did not end");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let o = child.wait_with_output().unwrap();
    String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)
}

// Q-625: PostgreSQL's money is a count of its lc_monetary currency's unit, which was read as
// cents whatever the currency: ¥1,234 came back as 12.34, and 500 written read back as 5.00.
#[test]
fn postgres_money_reads_with_its_currency_s_decimals() {
    let Some(pg) = support::postgres("f50-money") else { return };
    let psql = |url: &str, sql: &str| {
        Command::new(pg.bin.join("psql")).args([url, "-q", "-c", sql]).output().map(|o| o.status.success()).unwrap_or(false)
    };
    let cents = pg.database("cents");
    let yen = pg.database("yen");
    // A locale with no decimals in its money: as the system names it, if it has one.
    let has_yen = ["ja_JP.UTF-8", "ja_JP.utf8", "ja_JP.eucJP"].iter().any(|l| psql(&pg.url, &format!("alter database yen set lc_monetary = '{}'", l)));
    let program = |url: &str| {
        format!(
            "db.open({url:?})\ndb.exec(\"create table t (id serial primary key, m money, n numeric(12, 2))\")\ndb.exec(\"insert into t (m, n) values ($1, $2), ($3, $4)\", 500m, 1.25m, -1234m, 0m)\nprint(db.query(\"select m, n from t order by id\"))\nprint(db.query(\"select n from t order by id\"))\nprint(db.query(\"select m from t order by id\"))\n\nroute GET /m = db.query(\"select m, m::text as t from t order by id\")\n"
        )
    };
    let d = dir("pg-cents", &program(&cents));
    let out = run_script(&d);
    assert!(out.contains("[{ m: 500.00m, n: 1.25m }, { m: -1234.00m, n: 0.00m }]"), "{}", out);
    assert!(out.contains("[{ n: 1.25m }, { n: 0.00m }]"), "{}", out);
    // The second query with money on the connection uses what the first found.
    assert!(out.contains("[{ m: 500.00m }, { m: -1234.00m }]"), "{}", out);
    if !has_yen {
        eprintln!("skipping the yen half: no Japanese locale on this system");
        return;
    }
    let d = dir("pg-yen", &program(&yen));
    let out = run_script(&d);
    assert!(out.contains("[{ m: 500m, n: 1.25m }, { m: -1234m, n: 0.00m }]"), "{}", out);
    assert!(out.contains("[{ m: 500m }, { m: -1234m }]"), "{}", out);
    // In a server, on the connections requests share.
    let s = support::serve(&d.join("main.qwe"), &[]);
    let body = support::send(s.port, "GET", "/m", &[], "").body;
    assert!(body.contains("\"m\":\"500\""), "{}", body);
    assert!(body.contains("\"m\":\"-1234\""), "{}", body);
}

/// `same`, with top-level definitions (types, functions) before the case.
#[track_caller]
fn same_with(defs: &str, body: &str, want: &str) {
    let src = format!("{}\nfn __t() {{\n{}\n}}\n", defs, body);
    let mut it = quillo::Interp::new(".");
    it.load_source(&src, "case.qwe").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    let s = match it.call(&quillo::value::lookup(&it.globals, "__t").unwrap(), vec![], 0) {
        Ok(v) | Err(quillo::interp::Ctrl::Return(v)) => v.repr(),
        Err(quillo::interp::Ctrl::Error(e)) => format!("ERROR: {}", e.msg),
        Err(quillo::interp::Ctrl::Reply(_)) => "REPLY".into(),
    };
    assert_eq!(s, want, "server: {}", body);
    if node_available() {
        let js = quillo::client::test_script(&it, "__t").unwrap_or_else(|e| panic!("{}\n{}", e, body));
        let out = node(&js);
        assert!(out.status.success(), "node failed: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), s, "browser differs: {}", body);
    }
}

// Q-621: `type Money = { amount: dec, currency: str }`, the guide's way to check money from a
// client, checked the shape only: $10.004 was taken and dropped, "1e3" was 1,000, BTC a 500.
#[test]
fn the_money_type_holds_data_to_money_of_s_rules() {
    let defs = "type Line = { item: str, price: money }";
    let line = |price: &str| format!("Line({{ item: \"a\", price: json.parse({price:?}) }})");
    for (price, want) in [
        (r#"{"amount": "10.00", "currency": "USD"}"#, "{ item: \"a\", price: { amount: 10.00m, currency: \"USD\" } }"),
        (r#"{"amount": 19.99, "currency": " usd "}"#, "{ item: \"a\", price: { amount: 19.99m, currency: \"USD\" } }"),
        (r#"{"amount": 1500, "currency": "JPY"}"#, "{ item: \"a\", price: { amount: 1500m, currency: \"JPY\" } }"),
        (r#"{"amount": "-0.5", "currency": "EUR", "note": "x"}"#, "{ item: \"a\", price: { amount: -0.50m, currency: \"EUR\" } }"),
        (r#"{"amount": "120", "currency": "BGN"}"#, "{ item: \"a\", price: { amount: 120.00m, currency: \"BGN\" } }"),
        (r#"{"amount": "92233720368547758.07", "currency": "USD"}"#, "{ item: \"a\", price: { amount: 92233720368547758.07m, currency: \"USD\" } }"),
        (r#"{"amount": "10.004", "currency": "USD"}"#, "err(\"Line: `price` has more decimals than USD has (2)\")"),
        (r#"{"amount": 1.5, "currency": "JPY"}"#, "err(\"Line: `price` has more decimals than JPY has (0)\")"),
        (r#"{"amount": "1e3", "currency": "USD"}"#, "err(\"Line: `price` has an amount that is not a number\")"),
        (r#"{"amount": true, "currency": "USD"}"#, "err(\"Line: `price` has an amount that is not a number\")"),
        (r#"{"amount": null, "currency": "USD"}"#, "err(\"Line: `price` has an amount that is not a number\")"),
        (r#"{"currency": "USD"}"#, "err(\"Line: `price` has no amount\")"),
        (r#"{"amount": "5", "currency": "BTC"}"#, "err(\"Line: `price` has a currency that is not an ISO 4217 code\")"),
        (r#"{"amount": "5", "currency": 5}"#, "err(\"Line: `price` has a currency that is not text\")"),
        (r#"{"amount": "5"}"#, "err(\"Line: `price` has no currency\")"),
        (r#"{"amount": "92233720368547758.08", "currency": "USD"}"#, "err(\"Line: `price` is too large an amount of USD\")"),
        (r#"{"amount": "79228162514264337593543950335", "currency": "USD"}"#, "err(\"Line: `price` is too large an amount of USD\")"),
        (r#""5""#, "err(\"Line: `price` must be money, { amount, currency }\")"),
    ] {
        same_with(defs, &line(price), want);
    }
    // In a pattern, and an annotation, which takes money as the money functions take it.
    let defs = "fn kind(v) = match v { money as m => `money ${money.format(m, \"en-US\")}`, _ => \"other\" }\nfn cents(m: money) = money.to_minor(m)";
    same_with(defs, "[kind(json.parse(`{\"amount\": \"2\", \"currency\": \"eur\"}`)), kind(json.parse(`{\"amount\": \"2.001\", \"currency\": \"eur\"}`)), kind(5)]",
        "[\"money €2.00\", \"other\", \"other\"]");
    same_with(defs, "cents(money.of(\"12.5\", \"USD\") ?? null)", "1250");
    same_with(defs, "cents(json.parse(`{\"amount\": \"1.005\", \"currency\": \"USD\"}`))",
        "ERROR: argument `m` of `cents` must be money, got map { amount: \"1.005\", currency: \"USD\" }");
    same_with(defs, "cents(json.parse(`{\"amount\": 1.5, \"currency\": \"USD\"}`))",
        "ERROR: argument `m` of `cents` must be money, got map { amount: 1.5, currency: \"USD\" }");
}

#[test]
fn a_route_refuses_what_is_not_money_and_says_why() {
    let d = dir(
        "route",
        "type Transfer = { to: str, amount: money }\nroute POST /transfer { json: Transfer } = {\n  let fee = money.of(0.25m, \"USD\") ?? reply 500\n  let total = json.amount + fee\n  is_err(total) ? reply 400 total.error : { total, cents: money.to_minor(total) }\n}\nroute POST /pay { json: { price: money } } = { price, cents: money.to_minor(price) }\n",
    );
    assert!(check(&std::fs::read_to_string(d.join("main.qwe")).unwrap()).contains("ok"));
    let s = support::serve(&d.join("main.qwe"), &[]);
    let post = |path: &str, body: &str| support::send(s.port, "POST", path, &[("content-type", "application/json")], body);
    let ok = post("/transfer", r#"{"to": "a", "amount": {"amount": "10.00", "currency": "usd"}}"#);
    assert_eq!(ok.status, 200, "{}", ok.body);
    assert!(ok.body.contains(r#""cents":1025"#), "{}", ok.body);
    for bad in [r#"{"amount": "10.004", "currency": "USD"}"#, r#"{"amount": "1e3", "currency": "USD"}"#, r#"{"amount": "10", "currency": "BTC"}"#, r#"{"amount": "92233720368547758.08", "currency": "USD"}"#] {
        let r = post("/transfer", &format!(r#"{{"to": "a", "amount": {bad}}}"#));
        assert_eq!(r.status, 400, "{}: {}", bad, r.body);
    }
    // Two currencies: an error value the route answers with.
    let eur = post("/transfer", r#"{"to": "a", "amount": {"amount": "5", "currency": "EUR"}}"#);
    assert_eq!((eur.status, eur.body.contains("cannot add EUR and USD")), (400, true), "{}", eur.body);
    let pay = post("/pay", r#"{"price": {"amount": 1500, "currency": "JPY"}}"#);
    assert!(pay.body.contains(r#""cents":1500"#), "{}", pay.body);
    assert_eq!(post("/pay", r#"{"price": {"amount": "1.5", "currency": "JPY"}}"#).status, 400);
}

#[test]
fn a_money_pattern_says_which_rule_it_fails_under_watch() {
    let d = dir("why", "route POST /pay { json: { price: money } } = money.to_minor(price)\n");
    let s = support::serve_with(&d.join("main.qwe"), &["--watch"]);
    let r = support::send(s.port, "POST", "/pay", &[("content-type", "application/json")], r#"{"price": {"amount": "1.5", "currency": "JPY"}}"#);
    assert_eq!(r.status, 400);
    assert!(r.body.contains("`json.price` has more decimals than JPY has (0)"), "{}", r.body);
    // What was sent is not repeated.
    assert!(!r.body.contains("1.5"), "{}", r.body);
    let r = support::send(s.port, "POST", "/pay", &[("content-type", "application/json")], r#"{"price": "5"}"#);
    assert!(r.body.contains("`json.price` must be money, { amount, currency }"), "{}", r.body);
}

#[test]
fn the_api_document_describes_money() {
    let d = dir("describe", "route POST /pay { json: { price: money } } = 1\n");
    let doc = quillo::describe::of_source(&d.join("main.qwe")).unwrap();
    let schema = &doc["paths"]["/pay"]["post"]["requestBody"]["content"]["application/json"]["schema"]["properties"]["price"];
    assert_eq!(schema["type"], "object", "{}", doc);
    assert_eq!(schema["properties"]["amount"]["format"], "decimal");
    assert_eq!(schema["required"], serde_json::json!(["amount", "currency"]));
}

// Found in review: `-=`, a variable assigned again, a `map` parameter and JSON were refused by
// the checker ("cannot subtract map and map"), so `quillo run` would not start the program.
#[test]
fn the_checker_takes_money_minus_money_however_it_is_written() {
    for src in [
        "let t = money.of(1, \"EUR\") ?? reply 400\nt -= money.of(2, \"EUR\") ?? reply 400\nt += money.of(2, \"EUR\") ?? reply 400\nprint(t)\n",
        "let t = money.of(9, \"USD\") ?? reply 400\nfor x in [1, 2] {\n  t = t - (money.of(x, \"USD\") ?? reply 400)\n}\nprint(t)\n",
        "fn d(a: map, b: map) = a - b\nprint(d(money.of(2, \"USD\") ?? null, money.of(1, \"USD\") ?? null))\n",
        "print(json.parse(`[{\"amount\": \"5\", \"currency\": \"USD\"}]`)[0] - money.of(1, \"USD\"))\n",
        "fn d(a, b) = a - b\nprint(d(money.of(2, \"USD\"), money.of(1, \"USD\")))\n",
        "component C() {\n  state left = money.of(10, \"USD\")\n  <button @click={() => { left -= money.of(1, \"USD\") ?? left }}>{money.format(left)}</button>\n}\nroute GET / = <C/>\n",
    ] {
        let out = check(src);
        assert!(out.contains(": ok") && !out.contains("cannot subtract"), "{}\n{}", src, out);
    }
    // What can never be money is still refused.
    for (src, said) in [("print([1] - { a: 1 })\n", "cannot subtract"), ("print(\"a\" - money.of(1, \"USD\"))\n", "cannot subtract"), ("print({ a: 1 } - 5)\n", "cannot subtract")] {
        assert!(check(src).contains(said), "{}", src);
    }
}

#[test]
fn too_large_says_the_amount_as_it_was_given_on_both_sides() {
    same("money.to_minor({ amount: 92233720368547759, currency: \"USD\" })", "ERROR: `money.to_minor`: 92233720368547759 USD is too large an amount");
    same("money.to_minor({ amount: \"92233720368547758.080\", currency: \"USD\" })", "ERROR: `money.to_minor`: 92233720368547758.080 USD is too large an amount");
    same("money.to_minor({ amount: 9223372036854775808m, currency: \"USD\" })", "ERROR: `money.to_minor`: 9223372036854775808 USD is too large an amount");
    // Four decimals: the last amount that fits, and the first that does not.
    same("money.to_minor({ amount: \"922337203685477.5807\", currency: \"CLF\" })", "9223372036854775807");
    same("money.to_minor({ amount: \"922337203685477.5808\", currency: \"CLF\" })", "ERROR: `money.to_minor`: 922337203685477.5808 CLF is too large an amount");
    same("money.add({ amount: 1, currency: \"BGN\" }, money.zero(\"BGN\")) + { amount: \"0.50\", currency: \"bgn\" }", "{ amount: 1.50m, currency: \"BGN\" }");
    same("money.minor_units(\"ZMK\")", "2");
    same("money.of(1, \"XYZ\") + money.of(1, \"USD\")", "ERROR: cannot add error and map");
}

#[test]
fn codes_and_amounts_are_trimmed_and_upper_cased_alike() {
    // Rust trims White_Space: U+0085 but not U+FEFF; a code is upper-cased in ASCII only.
    same("money.of(\"\\u{0085}5\\u{0085}\", \"USD\")", "{ amount: 5.00m, currency: \"USD\" }");
    same("money.of(\"\\u{feff}5\", \"USD\")", "err(\"not an amount: \\\"\u{feff}5\\\"\")");
    same("money.of(1, \"u\\u{17f}d\")", "err(\"unknown currency \\\"u\u{17f}d\\\"\")");
    same("{ amount: \"\\u{3000}5\", currency: \"\\tusd\\n\" } + money.of(1, \"USD\")", "{ amount: 6.00m, currency: \"USD\" }");
}

// Q-627: money.format was not "the same as Intl.NumberFormat": XOF had no symbol anywhere, the
// French locales none for $CL, 13 currencies showed decimals Intl does not, and a locale not in
// the table was another country's (es-CO as Spain's, zh-Hant-TW as mainland China's).
#[test]
fn money_formats_as_cldr_writes_it() {
    let f = |amount: &str, code: &str, loc: &str| format!("money.format(money.of({amount:?}, {code:?}), {loc:?})");
    for (call, want) in [
        (f("1234", "XOF", "en-US"), "F\u{202f}CFA\u{a0}1,234"),
        (f("1234", "XOF", "fr-FR"), "1\u{202f}234\u{a0}F\u{202f}CFA"),
        (f("1234", "XAF", "fr-FR"), "1\u{202f}234\u{a0}FCFA"),
        (f("1234", "XCD", "en-US"), "EC$1,234.00"),
        (f("1234", "XCG", "en-US"), "Cg.\u{a0}1,234.00"),
        (f("1234", "CLP", "fr-FR"), "1\u{202f}234\u{a0}$CL"),
        (f("1234.50", "IRR", "en-US"), "IRR\u{a0}1,235"),
        (f("1234.49", "IRR", "en-US"), "IRR\u{a0}1,234"),
        (f("-2.50", "IRR", "en-US"), "-IRR\u{a0}3"),
        (f("-0.40", "IRR", "en-US"), "IRR\u{a0}0"),
        (f("1234.567", "IQD", "en-US"), "IQD\u{a0}1,235"),
        (f("1234.5", "USD", "es-CO"), "US$\u{a0}1.234,50"),
        (f("1234.5", "USD", "es-HN"), "USD\u{a0}1,234.50"),
        (f("1234.5", "TWD", "zh-Hant-TW"), "$1,234.50"),
        (f("1234.5", "TWD", "zh-Hant"), "$1,234.50"),
        (f("1234.5", "TWD", "zh-Hant-CN"), "$1,234.50"),
        (f("1234.5", "CNY", "zh-Hans"), "¥1,234.50"),
        (f("1234.5", "CNY", "zh-Hans-TW"), "¥1,234.50"),
        (f("1234.5", "EUR", "pt-GW"), "1234,50\u{a0}€"),
        (f("1234.5", "EUR", "pt-US"), "€\u{a0}1.234,50"),
        (f("1234.5", "EUR", "es-GQ"), "1234,50\u{a0}€"),
        (f("1234.5", "CAD", "en-u-ca-gregory"), "CA$1,234.50"),
        (f("1234.5", "USD", "sw"), "US$\u{a0}1,234.50"),
        (f("1234.5", "EUR", "sr-Latn-RS"), "1.234,50\u{a0}€"),
        (f("-1234.5", "INR", "en-IN"), "-₹1,234.50"),
        (f("1234567", "INR", "hi-IN"), "₹12,34,567.00"),
    ] {
        same(&call, &format!("{:?}", want).replace("\\u{a0}", "\u{a0}").replace("\\u{202f}", "\u{202f}"));
    }
    // Numbers: Indian grouping where only numbers have it.
    same("[format_number(1234567, \"ml-IN\", 2), money.format(money.of(1234567, \"INR\"), \"ml-IN\")]", "[\"12,34,567.00\", \"₹1,234,567.00\"]");
    // An amount keeps its minor units; only what is shown is rounded, a tie away from zero, as
    // Intl rounds it (Intl writes -0.40 rials as "-IRR 0"; Quillo writes no -0).
    same("money.to_minor(money.of(\"1234.50\", \"IRR\"))", "123450");
    // Every name a fallback rule takes, in the browser too.
    same("[\"es-HN\", \"zh-Hans-SG\", \"zh-Hant-MO\", \"pt-CV\", \"sr-Latn\", \"en-Latn\", \"es-\\u{e9}\", \"en--US\", \"de-CH-u-nu-latn\"] | map((l) => format_number(-1234.5, l, 1))",
        "[\"-1,234.5\", \"-1,234.5\", \"-1,234.5\", \"-1234,5\", \"-1.234,5\", \"-1,234.5\", \"-1234,5\", \"-1,234.5\", \"-1’234.5\"]");
}

#[test]
fn a_locale_written_as_another_is_said_once_in_the_log() {
    let d = dir(
        "locale-log",
        "print(money.format(money.of(1, \"USD\") ?? null, \"xx-YY\"))\nprint(money.format(money.of(2, \"USD\") ?? null, \"xx-YY\"))\nprint(format_number(3, \"de-XX\"))\nprint(money.format(money.of(4, \"EUR\") ?? null, \"fr\"))\n",
    );
    let out = run_script(&d);
    assert_eq!(out.matches("no money and number data for locale \"xx-YY\": written as en-US").count(), 1, "{}", out);
    assert_eq!(out.matches("no money and number data for locale \"de-XX\": written as de-DE").count(), 1, "{}", out);
    // A language alone names its first locale, and is not said.
    assert!(!out.contains("\"fr\""), "{}", out);
    assert!(out.contains("$1.00") && out.contains("$2.00") && out.contains("4,00\u{a0}€"), "{}", out);
}

// Q-628: `money` and `decimal(12, 2)` columns in SQLite lose cents past 15 digits and the
// scale; only the guide said so.
#[test]
fn sqlite_says_once_which_columns_lose_digits() {
    let d = dir(
        "sqlite-lossy",
        "db.exec(\"create table t (id integer primary key, price money, p2 DECIMAL(12, 2), p3 decimal_text, n numeric, d dec(5,1), \\\"q\\\" numeric ( 10, 2), dx decimal, \\\"unit price\\\" money, c money_cents)\")\ndb.exec(\"create table if not exists t (price money)\")\ndb.exec(\"alter table t add column fee decimal\")\ndb.exec(\"alter table t add tax money\")\ndb.exec(\"CREATE\\n  TEMP TABLE u (price money) -- , note decimal\\n; create table v (a text default ', x money', /* , y money */ b int)\")\ndb.exec(\"insert into t (price, p3) values (?, ?)\", 12345678901234567.89m, 12345678901234567.89m)\nprint(db.query(\"select price, p3 from t\"))\n",
    );
    std::fs::create_dir_all(d.join("migrations")).unwrap();
    std::fs::write(d.join("migrations/001_ledger.sql"), "create table ledger (id integer primary key, total decimal(12, 2));\n").unwrap();
    std::fs::write(d.join("main.qwe"), format!("db.migrate()\n{}", std::fs::read_to_string(d.join("main.qwe")).unwrap())).unwrap();
    let out = run_script(&d);
    for col in ["t.price", "t.p2", "t.d", "t.q", "t.dx", "t.unit price", "t.c", "t.fee", "t.tax", "u.price", "ledger.total"] {
        assert_eq!(out.matches(&format!("SQLite keeps `{}` (", col)).count(), 1, "{}: {}", col, out);
    }
    for col in ["t.p3", "t.n", "t.id", "u.note", "v.x", "v.y", "v.a", "v.b"] {
        assert!(!out.contains(&format!("`{}` (", col)), "{}: {}", col, out);
    }
    assert!(out.contains("price: 12345678901234568m, p3: 12345678901234567.89m"), "{}", out);
}

// Q-629: Python's sqlite3 takes the parameters as a list; Quillo stored the list as JSON text.
#[test]
fn a_list_given_as_the_parameters_is_warned_about() {
    let out = check(
        "db.exec(\"insert into t (p) values (?)\", [9.99m])\ndb.query(\"select * from t where a = ? and b = ?\", [1, 2])\ndb.query(\"select * from t where a = ? and b = ? and c = ?\", [1, 2, 3])\nlet c = db.connect(\"o.db\")\nc.exec(`insert into t (p) values (?)`, [1])\n",
    );
    assert!(out.contains("main.qwe:1: warning: `db.exec` takes its parameters one by one, as in `db.exec(sql, a, b)`: this list is one parameter, stored as its JSON text. Pass the values after the SQL, or json.stringify(list) to store a list"), "{}", out);
    assert!(out.contains("main.qwe:2: warning: `db.query` takes its parameters one by one") && out.contains("the SQL's 1 other `?` gets nothing"), "{}", out);
    assert!(out.contains("main.qwe:3:") && out.contains("the SQL's 2 other `?` get nothing"), "{}", out);
    assert!(out.contains("main.qwe:5: warning: `c.exec` takes its parameters one by one"), "{}", out);
    // Not for: values given one by one, a list stored on purpose, a `?` in quotes, no `?`,
    // SQL that is not written out, exec_many's list of rows, and a list after other values.
    let quiet = check(
        "db.exec(\"insert into t (p) values (?)\", 9.99m)\ndb.exec(\"insert into t (p) values (?)\", json.stringify([1]))\ndb.query(\"select * from t where a = '?'\", [1])\ndb.exec(\"delete from t\", [1])\nlet sql = \"insert into t (p) values (?)\"\ndb.exec(sql, [1])\ndb.exec_many(\"insert into t (p) values (?)\", [[1], [2]])\ndb.exec(\"insert into t (a, b) values (?, ?)\", 1, [2])\n[1] | db.exec(\"insert into t (p) values (?)\")\n",
    );
    assert!(!quiet.contains("one by one"), "{}", quiet);
}

// Q-632: `(1 + r)^n` for interest and loan payments went through floats: pow refused a dec.
#[test]
fn a_decimal_to_an_int_s_power_stays_a_decimal() {
    same("[pow(1.1m, 2), pow(2m, -2), pow(0m, 0), pow(-1.5m, 3), pow(10m, 28), pow(1.05m, 1), pow(7m, 0), pow(-2m, -3)]",
        "[1.21m, 0.25m, 1m, -3.375m, 10000000000000000000000000000m, 1.05m, 1m, -0.125m]");
    // A 30-year loan of $200,000 at 5% a year: the factor to the 25th digit, the payment exact.
    same("let r = 0.05m / 12\nlet f = pow(1m + r, 360)\n[f, round(200000m * r * f / (f - 1), 2)]", "[4.4677443140061322124280701433m, 1073.64m]");
    same("pow(0.5m, 200)", "0.0000000000000000000000000000m");
    same("pow(1m, 9223372036854775807)", "1m");
    same("pow(10m, 29)", "ERROR: decimal overflow");
    same("pow(10m, 64)", "ERROR: decimal overflow");
    // A negative power: below one, from one over it; above one, 0 past what fits.
    same("[pow(0.5m, -90), pow(-0.5m, -3), pow(2m, -100), pow(1.05m, -2000), pow(10m, -29)]",
        "[1237940039285380274899124224m, -8m, 0.0000000000000000000000000000m, 0.0000000000000000000000000000m, 0.0000000000000000000000000000m]");
    same("pow(0.5m, -100)", "ERROR: decimal overflow");
    same("pow(0m, -1)", "ERROR: division by zero");
    same("pow(1.5m, 0.5)", "ERROR: `pow` takes a dec to an int's power, got float as the power: for a fraction of a power, float(x) and math.pow");
    same("pow(1.5m, 2m)", "ERROR: `pow` takes a dec to an int's power, got dec as the power: for a fraction of a power, float(x) and math.pow");
    // The checker keeps the type it is given.
    let out = check("fn want_int(n: int) = n\nwant_int(pow(1.05m, 12))\n");
    assert!(out.contains("must be int, got dec"), "{}", out);
    let ok = check("print(pow(2, 10) + 1, pow(1.05m, 12) + 1m, round(pow(1.05m, 12), 2))\n");
    assert!(ok.contains(": ok"), "{}", ok);
}

// Q-626: round(1.005, 2) was 1.0 while format_number wrote 1.01, and format_number could not
// round as money does.
#[test]
fn round_and_format_number_round_alike_and_format_number_takes_a_mode() {
    same("[round(1.005, 2), round(-1.005, 2), round(2.675, 2), round(0.125, 2), round(1.115, 2), round(123.456, 1), round(-1.25, 1), round(0.1 + 0.2, 15), round(1.23456789, 15)]",
        "[1.01, -1.01, 2.68, 0.13, 1.12, 123.5, -1.3, 0.3, 1.23456789]");
    // Whole results (a page writes 0.0 as 0, as it cannot tell them apart): away from a tie the
    // quick way, with no -0; past 2^53; past what a decimal holds; not a number.
    let (_, s) = server("[round(0.0049999, 2), round(-0.001, 2), round(-0.4, 0), round(2.5, 0), round(12345678901234567.0, 2) == 12345678901234567.0, round(1e300, 2) == 1e300, round(1.0 / 0, 2)]");
    assert_eq!(s, "[0.0, 0.0, 0.0, 3.0, true, true, inf]");
    same("[round(1e300, 2) == 1e300, round(1e306, 15) == 1e306, round(663152832.685, 2) == 663152832.69, round(133743.506045, 5) == 133743.50605]", "[true, true, true, true]");
    same("[format_number(0.125m, \"en-US\", 2), format_number(0.125m, \"en-US\", 2, \"half_even\"), format_number(0.135m, \"en-US\", 2, \"half_even\"), format_number(2.5, \"en-US\", 0, \"floor\"), format_number(-2.5, \"de-DE\", 0, \"ceiling\")]",
        "[\"0.13\", \"0.12\", \"0.14\", \"2\", \"-2\"]");
    // An invoice line shows what money works out.
    same("let line = money.mul(money.of(2.50m, \"USD\"), 0.05m)\n[money.format(line, \"en-US\"), format_number(2.50m * 0.05m, \"en-US\", 2, \"half_even\")]", "[\"$0.12\", \"0.12\"]");
    same("format_number(1, \"en-US\", 2, \"bad\")", "ERROR: `format_number`: unknown rounding \"bad\" (one of half_even, half_up, half_down, up, down, ceiling or floor)");
    same("format_number(1, \"en-US\", 2, 5)", "ERROR: `format_number` expects a rounding mode (a str) as argument 4, got int 5");
    same("format_number(1, \"en-US\", null, \"half_even\")", "\"1\"");
}

// Q-624: money.of reads text as a program writes it, so a German visitor's "1.000", a
// thousand, was one euro; nothing read money.format's own output back.
#[test]
fn money_parse_reads_an_amount_as_the_locale_writes_it() {
    let p = |text: &str, code: &str, loc: &str| format!("money.parse({text:?}, {code:?}, {loc:?})");
    let m = |amount: &str, code: &str| format!("{{ amount: {amount}m, currency: \"{code}\" }}");
    for (text, code, loc, want) in [
        ("1.234,50 €", "EUR", "de-DE", m("1234.50", "EUR")),
        ("1.000", "EUR", "de-DE", m("1000.00", "EUR")),
        ("1,5", "EUR", "de-DE", m("1.50", "EUR")),
        ("1234,5", "USD", "de-DE", m("1234.50", "USD")),
        ("-1.234,50 €", "EUR", "de-DE", m("-1234.50", "EUR")),
        ("$1,234.50", "USD", "en-US", m("1234.50", "USD")),
        ("-$1,234.50", "USD", "en-US", m("-1234.50", "USD")),
        ("$-5", "USD", "en-US", m("-5.00", "USD")),
        ("  USD 5 ", "USD", "en-US", m("5.00", "USD")),
        ("US$5", "USD", "en-US", m("5.00", "USD")),
        ("1,234", "USD", "en-US", m("1234.00", "USD")),
        ("1.000", "USD", "en-US", m("1.00", "USD")),
        ("₹12,34,567.00", "INR", "en-IN", m("1234567.00", "INR")),
        ("1,00,000", "INR", "en-IN", m("100000.00", "INR")),
        ("1,000,000", "INR", "en-IN", m("1000000.00", "INR")),
        ("1\u{202f}234,50\u{a0}€", "EUR", "fr-FR", m("1234.50", "EUR")),
        ("1 234,50", "EUR", "fr-FR", m("1234.50", "EUR")),
        ("5 $", "USD", "fr-FR", m("5.00", "USD")),
        ("CHF 1'234.50", "CHF", "de-CH", m("1234.50", "CHF")),
        ("CHF-1’234.50", "CHF", "de-CH", m("-1234.50", "CHF")),
        ("¥1,235", "JPY", "ja-JP", m("1235", "JPY")),
        ("\u{2212}5\u{a0}kr", "SEK", "sv-SE", m("-5.00", "SEK")),
        ("\u{200f}1,234.50\u{a0}EGP", "EGP", "en-US", m("1234.50", "EGP")),
        ("0,001", "BHD", "de-DE", m("0.001", "BHD")),
    ] {
        same(&p(text, code, loc), &want);
    }
    for (text, code, loc) in [
        ("1.00", "EUR", "de-DE"), ("1234.5", "USD", "de-DE"), ("1,5", "USD", "en-US"), ("1,2345", "USD", "en-US"), ("12,34,567", "USD", "en-US"),
        ("", "USD", "en-US"), ("abc", "USD", "en-US"), ("$", "USD", "en-US"), ("1,", "USD", "en-US"), (",5", "USD", "en-US"), ("1,,000", "USD", "en-US"),
        ("1e3", "USD", "en-US"), ("--5", "USD", "en-US"), ("5 £", "USD", "en-US"), ("1,23,4567", "INR", "en-IN"), ("١٢", "USD", "en-US"),
    ] {
        same(&p(text, code, loc), &format!("err(\"not an amount: {}\")", format!("{:?}", text).replace('"', "\\\"")));
    }
    same(&p("12.345", "USD", "en-US"), "err(\"12.345 has more decimals than USD has (2); give a rounding mode to round it\")");
    same("money.parse(\"12.345\", \"USD\", \"en-US\", \"half_up\")", &m("12.35", "USD"));
    same("money.parse(\"1\", \"XYZ\", \"en-US\")", "err(\"unknown currency \\\"XYZ\\\"\")");
    same("money.parse(\"92.233.720.368.547.758,08\", \"EUR\", \"de-DE\")", "err(\"92233720368547758.08 EUR is too large an amount\")");
    same("money.parse(5, \"USD\")", "ERROR: `money.parse` expects text as argument 1, got int 5");
    same("money.parse(\"1\", \"USD\", \"en-US\", 3)", "ERROR: `money.parse` expects a rounding mode (a str) as argument 4, got int 3");
    // What money.format writes, money.parse reads, in every locale.
    same(
        "let m = money.of(\"-1234567.89\", \"EUR\")\n[\"en-US\", \"de-DE\", \"fr-FR\", \"de-CH\", \"en-IN\", \"sv-SE\", \"nl-NL\", \"es-CO\", \"pt-BR\", \"ja-JP\"] | map((l) => money.parse(money.format(m, l), \"EUR\", l) == m)",
        "[true, true, true, true, true, true, true, true, true, true]",
    );
    // Without a locale, the program's, else en-US; the checker takes it as money or an error.
    same("money.parse(\"1,234.50\", \"USD\")", &m("1234.50", "USD"));
    let out = check("fn want_int(n: int) = n\nwant_int(money.parse(\"5\", \"USD\"))\n");
    assert!(out.contains("got map | error"), "{}", out);
}

// Q-633: the guide said money.mul gives error values for bad arguments (they are runtime
// errors; too large an amount is the error value), and `check` let literals through that can
// only fail, and typed `money.of(...).amount` as anything.
#[test]
fn the_checker_knows_money_s_literals_fields_and_error_values() {
    let out = check(
        "let a = money.of(1, \"USD\", \"half-up\")\nlet b = money.of(1, \"XYZ\")\nlet c = money.parse(\"1\", \"usd\", \"en-US\", \"bankers\")\nlet d = money.zero(\"BTC\")\nlet e = money.round(money.of(1, \"USD\") ?? null, \"HALF_EVEN\")\nprint(money.of(1, \"USD\").amount + 0.5)\n",
    );
    for line in [
        "main.qwe:1: error: `money.of`: unknown rounding \"half-up\"",
        "main.qwe:2: error: `money.of`: \"XYZ\" is not an ISO 4217 currency code",
        "main.qwe:3: error: `money.parse`: unknown rounding \"bankers\"",
        "main.qwe:4: error: `money.zero`: \"BTC\" is not an ISO 4217 currency code",
        "main.qwe:5: error: `money.round`: unknown rounding \"HALF_EVEN\"",
        "main.qwe:6: error: cannot add dec and float",
    ] {
        assert!(out.contains(line), "{}\n{}", line, out);
    }
    // What is right passes: codes in any case and with spaces, a withdrawn code, every mode,
    // modes and codes held in variables, and testing what money.mul gives.
    let ok = check(
        "let mode = \"half_up\"\nlet m = money.of(1, \" usd \", mode) ?? reply 400\nlet r = money.mul(m, 3, \"floor\")\nprint(is_err(r) ? r.error : money.format(r), money.of(1, \"bgn\", \"ceiling\"), money.minor_units(\"HRK\"))\nprint(money.from_minor(1, \"JPY\"), money.round(m, \"half_down\"), money.parse(\"1\", \"EUR\", null, \"up\"))\n",
    );
    assert!(ok.contains(": ok"), "{}", ok);
}

#[test]
fn placeholders_in_comments_and_quoted_names_are_not_counted() {
    let quiet = check("db.query(\"select * from t -- why?\\n\", [1])\ndb.query(\"select \\\"col?\\\" from t /* ? */\", [1])\n");
    assert!(!quiet.contains("one by one"), "{}", quiet);
    let said = check("db.query(\"select * from t where a = ? -- the id\", [1])\n");
    assert!(said.contains("one by one"), "{}", said);
    assert!(check("print(format_number(1, \"en-US\", 2, null))\n").contains(": ok"));
}
