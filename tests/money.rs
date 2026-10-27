//! Money (src/money.rs): amounts of a currency, kept to its minor units,
//! never mixed, split without losing a cent, and written as each locale
//! writes them. The same in the browser, and as `Intl.NumberFormat` writes
//! them there.

use std::process::Command;

fn node_available() -> bool {
    Command::new("node").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

/// Runs `js` in Node, given on its standard input: as one argument, the
/// thousands of cases below are past what Linux allows (128 KiB).
fn node(js: &str) -> std::process::Output {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new("node").arg("-").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().expect("node");
    child.stdin.take().unwrap().write_all(js.as_bytes()).expect("node's input");
    child.wait_with_output().expect("node")
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

/// The same answer on the server and, compiled, in Node; and it is `want`.
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

#[test]
fn amounts_have_the_currency_s_minor_units() {
    same("money.of(19.99m, \"USD\")", "{ amount: 19.99m, currency: \"USD\" }");
    same("[money.of(12.5m, \"USD\").amount, money.of(5, \"EUR\").amount, money.of(\"7\", \"bhd\").amount, money.of(\"1235\", \"JPY\").amount]",
        "[12.50m, 5.00m, 7.000m, 1235m]");
    same("[money.minor_units(\"JPY\"), money.minor_units(\"USD\"), money.minor_units(\"BHD\"), money.minor_units(\"CLF\"), money.minor_units(\"XAU\")]",
        "[0, 2, 3, 4, err(\"unknown currency \\\"XAU\\\"\")]");
    // More decimals than the currency has: refused, unless told how to round.
    same("money.of(\"12.345\", \"USD\")", "err(\"12.345 has more decimals than USD has (2); give a rounding mode to round it\")");
    same("[money.of(\"12.345\", \"USD\", \"half_up\").amount, money.of(12.345m, \"USD\", \"half_even\").amount, money.of(12.3400m, \"USD\").amount, money.of(1234.5m, \"JPY\", \"floor\").amount]",
        "[12.35m, 12.34m, 12.34m, 1234m]");
    same("[money.of(\"abc\", \"USD\"), money.of(1, \"ZZZ\")]", "[err(\"not an amount: \\\"abc\\\"\"), err(\"unknown currency \\\"ZZZ\\\"\")]");
    same("money.of(1.5, \"USD\")", "ERROR: `money.of` takes a dec, an int or text, not a float: convert it with dec(x)");
    same("[money.from_minor(1250, \"USD\"), money.from_minor(-5, \"KWD\").amount, money.to_minor(money.of(12.5m, \"USD\")), money.to_minor(money.of(1235, \"JPY\"))]",
        "[{ amount: 12.50m, currency: \"USD\" }, -0.005m, 1250, 1235]");
    same("[money.zero(\"EUR\"), money.is_zero(money.zero(\"JPY\")), money.is_zero(money.of(1, \"JPY\"))]",
        "[{ amount: 0.00m, currency: \"EUR\" }, true, false]");
}

#[test]
fn arithmetic_stays_in_one_currency() {
    same("let a = money.of(10.10m, \"USD\")\nlet b = money.of(0.20m, \"USD\")\n[money.add(a, b).amount, money.sub(b, a).amount, money.neg(a).amount, money.compare(a, b), money.compare(b, a), money.compare(a, a)]",
        "[10.30m, -9.90m, -10.10m, 1, -1, 0]");
    same("let p = money.of(19.99m, \"USD\")\n[money.mul(p, 3).amount, money.mul(p, 0.075m).amount, money.mul(p, 0.075m, \"half_up\").amount, money.mul(p, 0.075m, \"ceiling\").amount, money.mul(p, 0.075m, \"floor\").amount]",
        "[59.97m, 1.50m, 1.50m, 1.50m, 1.49m]");
    same("money.mul(money.of(0.25m, \"USD\"), 0.5m).amount", "0.12m");
    same("money.mul(money.of(0.25m, \"USD\"), 0.5m, \"half_up\").amount", "0.13m");
    same("money.add(money.of(1, \"USD\"), money.of(1, \"EUR\"))", "ERROR: cannot add USD and EUR: convert one currency to the other first");
    same("money.mul(money.of(1, \"USD\"), 1.5)", "ERROR: `money.mul` takes a dec or an int, not a float: convert it with dec(x)");
    same("money.add({ amount: 1m }, money.of(1, \"USD\"))", "ERROR: `money.add` expects money ({ amount, currency }) as argument 1, got map { amount: 1m }: no currency");
    // What JSON brings back (the amount as text) is money too.
    same("money.add(json.parse(json.stringify(money.of(1.25m, \"EUR\"))), money.of(1, \"EUR\")).amount", "2.25m");
    same("money.round({ amount: 2.675m, currency: \"USD\" }, \"half_up\").amount", "2.68m");
}

#[test]
fn allocation_loses_no_cent() {
    same("money.allocate(money.of(100, \"USD\"), [1, 1, 1]) | map((m) => m.amount)", "[33.34m, 33.33m, 33.33m]");
    same("money.allocate(money.of(0.05m, \"USD\"), [3, 7]) | map((m) => m.amount)", "[0.02m, 0.03m]");
    same("money.allocate(money.of(-100, \"USD\"), 3) | map((m) => m.amount)", "[-33.34m, -33.33m, -33.33m]");
    same("money.allocate(money.of(10, \"JPY\"), [0, 1, 1, 1]) | map((m) => m.amount)", "[0m, 4m, 3m, 3m]");
    same("money.allocate(money.of(1000, \"EUR\"), [0.5m, 0.3m, 0.2m]) | map((m) => m.amount)", "[500.00m, 300.00m, 200.00m]");
    same("let parts = money.allocate(money.of(1234.57m, \"USD\"), [17, 31, 52])\nsum(parts | map((m) => m.amount))", "1234.57m");
    same("money.allocate(money.of(1, \"USD\"), [0, 0])", "ERROR: `money.allocate`: the ratios must add up to more than 0");
}

#[test]
fn formats_as_each_locale_writes() {
    let cases = [
        ("money.of(1234.5m, \"USD\"), \"en-US\"", "$1,234.50"),
        ("money.of(1234.5m, \"EUR\"), \"de-DE\"", "1.234,50\u{a0}€"),
        ("money.of(1234.5m, \"EUR\"), \"fr-FR\"", "1\u{202f}234,50\u{a0}€"),
        ("money.of(1234.5m, \"JPY\", \"half_even\"), \"ja-JP\"", "￥1,234"),
        ("money.of(1235, \"JPY\"), \"en-US\"", "¥1,235"),
        ("money.of(1234567, \"INR\"), \"en-IN\"", "₹12,34,567.00"),
        ("money.of(-1234.5m, \"EUR\"), \"nl-NL\"", "€\u{a0}-1.234,50"),
        ("money.of(-1234.5m, \"SEK\"), \"sv-SE\"", "\u{2212}1\u{a0}234,50\u{a0}kr"),
        ("money.of(1234.5m, \"CHF\"), \"en-US\"", "CHF\u{a0}1,234.50"),
        ("money.of(1234.5m, \"EUR\"), \"es-ES\"", "1234,50\u{a0}€"),
        ("money.of(12345.5m, \"EUR\"), \"es-ES\"", "12.345,50\u{a0}€"),
        ("money.of(0.5m, \"BHD\"), \"en-US\"", "BHD\u{a0}0.500"),
        ("money.of(1234.5m, \"EUR\"), \"de-LU\"", "1.234,50\u{a0}€"),
        ("money.of(1234.5m, \"USD\"), \"xx\"", "$1,234.50"),
    ];
    for (args, want) in cases {
        same(&format!("money.format({})", args), &serde_json::to_string(want).unwrap());
    }
    same("[format_number(1234567.891m, \"en-IN\"), format_number(-1234.5, \"de-DE\", 2), format_number(1234, \"fr-FR\"), format_number(2.5m, \"en-US\", 0)]",
        "[\"12,34,567.891\", \"-1.234,50\", \"1\u{202f}234\", \"2\"]");
}

/// Every covered locale and common currency, as the browser's own
/// `Intl.NumberFormat` writes them (with ISO 4217's minor units), when
/// Node's CLDR is the one the locale table was made from: CLDR changes
/// marks between versions (de-CH's grouping became `’` in 47).
#[test]
fn formats_as_intl_does() {
    const CLDR: &str = "47";
    if !node_available() {
        return;
    }
    let cldr = node("console.log(process.versions.cldr ?? '')");
    let cldr = String::from_utf8_lossy(&cldr.stdout).trim().to_string();
    if cldr.split('.').next() != Some(CLDR) {
        eprintln!("skipping: Node's CLDR is {}, the locale table's {}", cldr, CLDR);
        return;
    }
    let locales: Vec<&str> = quillo::money::LOCALES.lines().map(|l| l.split('\t').next().unwrap()).collect();
    let currencies = ["USD", "EUR", "GBP", "JPY", "CNY", "INR", "CAD", "AUD", "CHF", "SEK", "PLN", "CZK", "HUF", "BRL", "MXN", "KRW", "HKD", "ZAR", "ILS", "IDR", "THB", "TRY", "BHD", "CLF"];
    let amounts = ["1234567.891", "-1234.5", "0", "12.345", "-0.5"];
    let mut cases = Vec::new();
    for l in &locales {
        for c in currencies {
            for a in amounts {
                let units = quillo::money::units_of(c).unwrap();
                let d = quillo::decimal::Dec::parse(a).unwrap();
                cases.push((l.to_string(), c, a, units, quillo::money::format_amount(d, c, units, l)));
            }
        }
    }
    let js = format!(
        "const cases = {};\nconst bad = [];\nfor (const [l, c, a, u, got] of cases) {{\n  const want = new Intl.NumberFormat(l, {{ style: 'currency', currency: c, minimumFractionDigits: u, maximumFractionDigits: u, roundingMode: 'halfEven', signDisplay: 'negative' }}).format(a);\n  if (want !== got) bad.push([l, c, a, want, got]);\n}}\nconsole.log(JSON.stringify(bad.slice(0, 20)) + ' ' + bad.length);",
        serde_json::to_string(&cases.iter().map(|(l, c, a, u, g)| serde_json::json!([l, c, a, u, g])).collect::<Vec<_>>()).unwrap()
    );
    let out = node(&js);
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert!(text.ends_with(" 0"), "{}\nof {} cases", text, cases.len());
    // And plain numbers, with two decimals.
    let mut nums = Vec::new();
    for l in &locales {
        for a in amounts {
            let d = quillo::decimal::Dec::parse(a).unwrap();
            nums.push(serde_json::json!([l, a, quillo::money::format_number(d, Some(2), l)]));
        }
    }
    let js = format!(
        "const bad = [];\nfor (const [l, a, got] of {}) {{\n  const want = new Intl.NumberFormat(l, {{ minimumFractionDigits: 2, maximumFractionDigits: 2, roundingMode: 'halfEven', signDisplay: 'negative' }}).format(a);\n  if (want !== got) bad.push([l, a, want, got]);\n}}\nconsole.log(JSON.stringify(bad.slice(0, 20)) + ' ' + bad.length);",
        serde_json::to_string(&nums).unwrap()
    );
    let out = node(&js);
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert!(text.ends_with(" 0"), "{}", text);
}

#[test]
fn the_checker_knows_money() {
    let diags = |src: &str| -> Vec<String> {
        quillo::check::check_source(src, "t.qwe", std::path::Path::new(".")).diagnostics.iter().map(|d| format!("{} {}", d.line, d.message)).collect()
    };
    let d = diags("let p = money.of(19.99m, \"USD\")?\nlet t = money.mul(p, 3)\nprint(t.amount + 1m, t.currency, money.format(t, \"en-US\"), format_number(1.5m, \"de-DE\", 2))\nfor m in money.allocate(t, [1, 2]) { print(m.amount * 2) }");
    assert!(d.is_empty(), "{:?}", d);
    let d = diags("let t = money.add(money.zero(\"USD\")?, money.zero(\"USD\")?)\nprint(t.amount + 0.5)");
    assert!(d.len() == 1 && d[0].contains("cannot add dec and float"), "{:?}", d);
    let d = diags("print(money.add(money.zero(\"USD\")?, 1).amonut)");
    assert!(!d.is_empty(), "{:?}", d);
}
