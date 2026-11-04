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
