//! `math`, `stats` and `vec` (src/numeric.rs): the answers Python gives
//! (computed with python3's math and statistics modules), and the same
//! answers in the browser.

use quillo::Interp;
use std::process::Command;

const DATA: &str = "let d = [2.75, 1.75, 1.25, 0.25, 0.5, 1.25, 3.5]\nlet x = [1, 2, 3, 4, 5, 6, 7, 8, 9]\nlet y = [1, 2, 3, 1, 2, 3, 1, 2, 3]";

fn run(body: &str) -> Result<String, String> {
    let src = format!("{}\nfn __t() {{\n{}\n}}\n", DATA, body);
    let mut it = Interp::new(".");
    it.load_source(&src, "case.qwe").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    match it.call(&quillo::value::lookup(&it.globals, "__t").unwrap(), vec![], 0) {
        Ok(v) => Ok(v.repr()),
        Err(quillo::interp::Ctrl::Error(e)) => Err(e.msg),
        Err(_) => Err("returned".into()),
    }
}

fn eval(body: &str) -> String {
    run(body).unwrap_or_else(|e| panic!("{}: {}", body, e))
}

fn float(body: &str) -> f64 {
    let r = eval(body);
    r.parse().unwrap_or_else(|_| panic!("{} gave {}", body, r))
}

/// How many floats apart.
fn ulps(a: f64, b: f64) -> u64 {
    let key = |x: f64| {
        let b = x.to_bits() as i64;
        if b < 0 { i64::MIN - b } else { b }
    };
    (key(a) - key(b)).unsigned_abs()
}

#[test]
fn floats_python_gives() {
    // (expression, Python's answer, floats apart allowed)
    let cases: &[(&str, f64, u64)] = &[
        ("math.sqrt(2)", 1.4142135623730951, 0),
        ("math.log(8, 2)", 3.0, 0),
        ("math.log10(1000)", 3.0, 0),
        ("math.exp(1)", 2.718281828459045, 0),
        ("math.expm1(1e-10)", 1.00000000005e-10, 0),
        ("math.log1p(1e-10)", 9.999999999500001e-11, 0),
        ("math.erf(0.5)", 0.5204998778130465, 1),
        ("math.erfc(3)", 2.2090496998585438e-05, 1),
        ("math.gamma(4.5)", 11.631728396567452, 2),
        ("math.gamma(-1.5)", 2.363271801207355, 2),
        ("math.lgamma(100)", 359.1342053695754, 2),
        ("math.hypot(3, 4, 12)", 13.0, 0),
        ("math.dist([1, 2], [4, 6])", 5.0, 0),
        ("math.atan2(1, -1)", 2.356194490192345, 0),
        ("math.remainder(10, 3)", 1.0, 0),
        ("math.remainder(7.5, 5)", -2.5, 0),
        ("math.fmod(-7, 3)", -1.0, 0),
        ("math.degrees(1)", 57.29577951308232, 0),
        ("math.fsum([0.1, 0.2, 0.3, 1e100, -1e100])", 0.6, 0),
        ("math.fsum([0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1])", 1.0, 0),
        ("stats.mean(d)", 1.6071428571428572, 0),
        ("stats.stdev(d)", 1.171334200612195, 0),
        ("stats.variance(d)", 1.3720238095238095, 0),
        ("stats.pvariance(d)", 1.1760204081632653, 0),
        ("stats.pstdev(d)", 1.084444746477784, 0),
        ("stats.harmonic_mean([40, 60])", 48.0, 0),
        ("stats.geometric_mean([54, 24, 36])", 36.000000000000014, 2),
        ("stats.fmean([1, 2, 3], [3, 2, 1])", 1.6666666666666667, 0),
        ("stats.normal_cdf(1.96)", 0.9750021048517796, 1),
        // Python's C build fuses multiplies and adds here; this does not.
        ("stats.normal_inv(0.975)", 1.9599639845400534, 1),
        ("stats.normal_inv(0.3, 10, 2)", 8.951198974583919, 1),
        ("stats.normal_inv(1e-10)", -6.361340902404057, 1),
        ("stats.normal_pdf(1, 0, 2)", 0.17603266338214976, 1),
        ("stats.correlation(x, y)", 0.31622776601683794, 1),
    ];
    for (e, want, tol) in cases {
        let got = float(e);
        assert!(ulps(got, *want) <= *tol, "{}: got {:?}, Python {:?} ({} floats apart)", e, got, want, ulps(got, *want));
    }
}

#[test]
fn what_python_gives() {
    for (e, want) in [
        ("[1e3, 2.5e-1, 6E+2, 1_000.5e1]", "[1000.0, 0.25, 600.0, 10005.0]"),
        ("[sum([1, 2.5, 3]), sum([1, 2]), sort([3, 1.5, 2]), sort([0.5, -1.0]), sort([3, 1, 2])]", "[6.5, 3, [1.5, 2, 3], [-1.0, 0.5], [1, 2, 3]]"),
        ("math.comb(52, 5)", "2598960"),
        ("math.perm(5, 2)", "20"),
        ("math.perm(4)", "24"),
        ("math.factorial(20)", "2432902008176640000"),
        ("math.gcd(12, 18, 27)", "3"),
        ("math.gcd()", "0"),
        ("math.lcm(4, 6, 10)", "60"),
        ("math.lcm([3, 5])", "15"),
        ("math.isqrt(99)", "9"),
        ("math.isqrt(9007199254740993)", "94906265"),
        ("math.floor(-2.5)", "-3"),
        ("math.ceil(2.1)", "3"),
        ("math.trunc(-2.7)", "-2"),
        ("math.prod([1, 2, 3, 4])", "24"),
        ("math.prod([2, 0.5])", "1.0"),
        ("math.sumprod([1, 2, 3], [4, 5, 6])", "32"),
        ("math.modf(-3.5)", "[-0.5, -3.0]"),
        ("math.frexp(8)", "[0.5, 4]"),
        ("math.isclose(1.0, 1.0 + 1e-10)", "true"),
        ("math.isclose(1.0, 1.1)", "false"),
        ("math.isclose(0, 1e-12, 1e-9, 1e-10)", "true"),
        ("[math.isnan(math.nan), math.isinf(-math.inf), math.isfinite(1)]", "[true, true, true]"),
        ("math.tau == 2 * math.pi", "true"),
        ("stats.median(d)", "1.25"),
        ("stats.median([1, 3, 5, 7])", "4.0"),
        ("stats.median([5, 1, 3])", "3"),
        ("stats.median_low([1, 3, 5, 7])", "3"),
        ("stats.median_high([1, 3, 5, 7])", "5"),
        ("stats.mode([\"red\", \"blue\", \"blue\", \"red\", \"green\", \"red\"])", "\"red\""),
        ("stats.mode([1, 1.0, 2])", "1"),
        ("stats.multimode(split(\"aabbbbccddddeeffffgg\", \"\"))", "[\"b\", \"d\", \"f\"]"),
        ("stats.multimode([])", "[]"),
        ("stats.quantiles(d)", "[0.5, 1.25, 2.75]"),
        ("stats.quantiles(d, 10, \"inclusive\")", "[0.4, 0.65, 1.1, 1.25, 1.25, 1.55, 1.95, 2.55, 3.05]"),
        ("stats.quantiles(range(1, 101))", "[25.25, 50.5, 75.75]"),
        ("stats.quantiles([7], 3)", "[7.0, 7.0]"),
        ("stats.linear_regression([1, 2, 3, 4, 5], [3, 5, 7, 9, 11])", "{ slope: 2.0, intercept: 1.0 }"),
        ("stats.linear_regression([1, 2, 3], [2, 4, 6], true)", "{ slope: 2.0, intercept: 0.0 }"),
        ("stats.covariance([1, 2, 3, 4, 5, 6, 7, 8, 9], [1, 2, 3, 1, 2, 3, 1, 2, 3])", "0.75"),
        ("stats.correlation(x, [9, 8, 7, 6, 5, 4, 3, 2, 1])", "-1.0"),
        ("stats.correlation([1, 2, 3, 4, 5], [1, 4, 9, 16, 1000], \"ranked\")", "1.0"),
    ] {
        assert_eq!(eval(e), want, "{}", e);
    }
}

#[test]
fn beyond_python_s_modules() {
    for (e, want) in [
        // numpy's percentile (linear), and pandas' describe.
        ("stats.percentile([1, 2, 3, 4], 50)", "2.5"),
        ("stats.percentile([1, 2, 3, 4], [0, 25, 100])", "[1.0, 1.75, 4.0]"),
        ("stats.percentile([15, 20, 35, 40, 50], 40)", "29.0"),
        (
            "stats.describe(d)",
            "{ count: 7, sum: 11.25, mean: 1.6071428571428572, std: 1.171334200612195, min: 0.25, q1: 0.875, median: 1.25, q3: 2.25, max: 3.5 }",
        ),
        ("stats.describe([])", "{ count: 0, sum: 0, mean: null, std: null, min: null, q1: null, median: null, q3: null, max: null }"),
        ("stats.histogram([1, 2, 2, 3, 3, 3, 4], 3)", "{ counts: [1, 2, 4], edges: [1.0, 2.0, 3.0, 4.0] }"),
        ("stats.histogram([0.5, 1, 7, 10, 11], [0, 1, 5, 10])", "{ counts: [1, 1, 2], edges: [0.0, 1.0, 5.0, 10.0] }"),
        ("stats.histogram([5, 5], 2)", "{ counts: [0, 2], edges: [4.5, 5.0, 5.5] }"),
        ("stats.zscore([2, 4, 4, 4, 5, 5, 7, 9])", "[-1.5, -0.5, -0.5, -0.5, 0.0, 0.0, 1.0, 2.0]"),
        ("stats.normalize([10, 15, 20])", "[0.0, 0.5, 1.0]"),
        ("stats.rank([10, 20, 20, 30])", "[1, 2.5, 2.5, 4]"),
        // numpy's elementwise operations, on lists.
        ("vec.add([1, 2, 3], 10)", "[11, 12, 13]"),
        ("vec.sub(10, [1, 2])", "[9, 8]"),
        ("vec.mul([1, 2, 3], [4, 5, 6])", "[4, 10, 18]"),
        ("vec.div([1, 2, 3], 2)", "[0.5, 1, 1.5]"),
        ("vec.mod([7, -7, 7.5], 3)", "[1, -1, 1.5]"),
        ("vec.pow([1, 2, 3], 2)", "[1.0, 4.0, 9.0]"),
        ("vec.dot([1, 2, 3], [4, 5, 6])", "32"),
        ("vec.norm([3, 4])", "5.0"),
        ("vec.norm([3, -4], 1)", "7.0"),
        ("vec.cumsum([1, 2, 3, 4])", "[1, 3, 6, 10]"),
        ("vec.cumprod([1, 2, 3, 4])", "[1, 2, 6, 24]"),
        ("vec.diff([1, 4, 9, 16])", "[3, 5, 7]"),
        ("[vec.argmin([3, 1, 2, 1]), vec.argmax([3, 1, 3]), vec.argmin([])]", "[1, 0, null]"),
        ("vec.argsort([3, 1, 2])", "[1, 2, 0]"),
        ("vec.argsort([3, 1, 2], true)", "[0, 2, 1]"),
        ("vec.clip([1, 5, 10], 2, 8)", "[2, 5, 8]"),
        ("vec.clip([1, 5, 10], null, 8)", "[1, 5, 8]"),
        ("vec.abs([-1, 2, -3])", "[1, 2, 3]"),
        ("vec.round([1.25, 2.5, -2.5])", "[1, 3, -3]"),
        ("vec.round([1.234, 5.678], 1)", "[1.2, 5.7]"),
        ("vec.linspace(0, 1, 5)", "[0.0, 0.25, 0.5, 0.75, 1.0]"),
        ("vec.linspace(0, 1, 4, false)", "[0.0, 0.25, 0.5, 0.75]"),
        ("vec.arange(5)", "[0, 1, 2, 3, 4]"),
        ("vec.arange(1, 10, 3)", "[1, 4, 7]"),
        ("vec.arange(0, 1, 0.25)", "[0.0, 0.25, 0.5, 0.75]"),
        ("vec.arange(5, 0, -2)", "[5, 3, 1]"),
        ("[vec.zeros(2), vec.ones(2), vec.full(2, \"x\")]", "[[0, 0], [1, 1], [\"x\", \"x\"]]"),
        ("vec.rolling_mean([1, 2, 3, 4, 5], 2)", "[1.5, 2.5, 3.5, 4.5]"),
        ("vec.rolling_sum([1, 2, 3, 4, 5], 3)", "[6.0, 9.0, 12.0]"),
        ("vec.rolling_mean([1, 2], 3)", "[]"),
        ("vec.matmul([[1, 2], [3, 4]], [[5, 6], [7, 8]])", "[[19.0, 22.0], [43.0, 50.0]]"),
        ("vec.matmul([[1, 2], [3, 4]], [1, 1])", "[3.0, 7.0]"),
        ("vec.transpose([[1, 2, 3], [4, 5, 6]])", "[[1, 4], [2, 5], [3, 6]]"),
        ("vec.column([{ a: 1 }, { a: 2, b: 3 }, {}], \"a\")", "[1, 2, null]"),
        ("vec.column([[1, 2], [3, 4]], 1)", "[2, 4]"),
        // Data from a table: the column, then its numbers.
        ("let rows = [{ total: 12.5 }, { total: 30 }, { total: 7.5 }]\nstats.mean(vec.column(rows, \"total\"))", "16.666666666666668"),
    ] {
        assert_eq!(eval(e), want, "{}", e);
    }
}

#[test]
fn what_python_raises_is_an_error() {
    for (e, needle) in [
        ("math.sqrt(-1)", "math domain error"),
        ("math.log(0)", "math domain error"),
        ("math.log(8, 1)", "math domain error"),
        ("math.acos(2)", "math domain error"),
        ("math.atanh(1)", "math domain error"),
        ("math.gamma(0)", "math domain error"),
        ("math.lgamma(-2)", "math domain error"),
        ("math.pow(0, -1)", "math domain error"),
        ("math.fmod(1, 0)", "math domain error"),
        ("math.exp(1000)", "math range error"),
        ("sum([9223372036854775807, 1])", "integer overflow"),
        ("math.factorial(-1)", "negative"),
        ("math.factorial(21)", "integer overflow"),
        ("math.comb(100, 50)", "integer overflow"),
        ("stats.mean([])", "at least one data point"),
        ("stats.stdev([1])", "at least 2 data points"),
        ("stats.mode([])", "no mode"),
        ("stats.mean([1, \"2\"])", "item 1 is str"),
        ("stats.correlation([1, 2, 3], [1, 1, 1])", "constant"),
        ("stats.correlation([1, 2], [1, 2, 3])", "same length"),
        ("stats.quantiles(d, 4, \"other\")", "unknown method"),
        ("stats.percentile(d, 101)", "from 0 to 100"),
        ("stats.normal_inv(1)", "between 0 and 1"),
        ("vec.add([1, 2], [1])", "same length"),
        ("vec.div([1], 0)", "division by zero"),
        ("vec.matmul([[1, 2]], [[1, 2]])", "a has 2 columns, b has 1 rows"),
        ("vec.arange(0, 1, 0)", "non-zero"),
    ] {
        match run(e) {
            Ok(v) => panic!("{} gave {}, not an error", e, v),
            Err(m) => assert!(m.contains(needle), "{}: {}", e, m),
        }
    }
}

#[test]
fn large_lists_are_exact_where_python_is() {
    // A million numbers whose mean is known: a running float sum drifts, these do not.
    assert_eq!(eval("stats.mean(vec.full(1000000, 0.1))"), "0.1");
    assert_eq!(eval("math.fsum(vec.full(1000000, 0.1))"), "100000.0");
    assert_eq!(eval("stats.pvariance(vec.full(1000, 1e9 + 0.1))"), "0.0");
    assert_eq!(eval("stats.median(vec.arange(1000001))"), "500000");
    assert_eq!(eval("stats.median(vec.arange(1000000))"), "499999.5");
    assert_eq!(eval("stats.histogram(vec.arange(1000), 4).counts"), "[250, 250, 250, 250]");
    assert_eq!(eval("let r = vec.rolling_mean(vec.full(100000, 0.1), 10)\n[len(r), r[0], r[99990]]"), "[99991, 0.1, 0.1]");
}

#[test]
fn checked_like_other_builtins() {
    let check = |src: &str| -> Vec<String> {
        quillo::check::check_source(src, "main.qwe", std::path::Path::new(".")).errors().map(|d| d.message.clone()).collect()
    };
    assert!(check("let m = stats.mean([1, 2])\nfn n() -> float = math.sqrt(m) + math.pi\nprint(vec.add([1], 2), n(), 1e-3)\n").is_empty());
    let e = check("print(math.sqrt(\"x\"))\nprint(math.nope(1))\nprint(stats.mean(1))\n");
    assert!(e.iter().any(|m| m.contains("math.sqrt")), "{:?}", e);
    assert!(e.iter().any(|m| m.contains("`math` has no function `nope`")), "{:?}", e);
    assert!(e.iter().any(|m| m.contains("stats.mean")), "{:?}", e);
    // The browser has them too.
    assert!(check("component Avg(xs: list) = <p>{stats.mean(xs)} {math.floor(math.pi)} {vec.add(xs, 1)}</p>\nroute GET / = <Avg xs={[1, 2]} />\n").iter().all(|m| !m.contains("not available in the browser")));
}

// ---- the browser ------------------------------------------------------------------------------

fn node_available() -> bool {
    Command::new("node").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

/// The server's and the browser's answers. A float that is whole shows as
/// 2.0 on the server and 2 in the browser, which cannot tell them apart.
fn both(body: &str) -> (String, String) {
    let src = format!("{}\nfn __t() {{\n{}\n}}\n", DATA, body);
    let mut it = Interp::new(".");
    it.load_source(&src, "case.qwe").unwrap();
    let server = match it.call(&quillo::value::lookup(&it.globals, "__t").unwrap(), vec![], 0) {
        Ok(v) => v.repr(),
        Err(quillo::interp::Ctrl::Error(e)) => format!("ERROR: {}", e.msg),
        Err(_) => "returned".into(),
    };
    let js = quillo::client::test_script(&it, "__t").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    let out = Command::new("node").arg("-e").arg(&js).output().expect("node");
    assert!(out.status.success(), "node failed: {}", String::from_utf8_lossy(&out.stderr));
    let browser = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (whole_floats(&server), browser)
}

fn whole_floats(s: &str) -> String {
    let mut out = String::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'.' && b.get(i + 1) == Some(&b'0') && i > 0 && b[i - 1].is_ascii_digit() && !b.get(i + 2).map_or(false, |c| c.is_ascii_digit()) {
            i += 2;
            continue;
        }
        out.push(b[i] as char);
        i += 1;
    }
    out
}

#[test]
fn the_browser_gives_the_same_answers() {
    if !node_available() {
        return;
    }
    for e in [
        "[math.sqrt(2), math.log(8, 2), math.log10(1000), math.exp(1), math.cbrt(27), math.hypot(3, 4), math.dist([1, 2], [4, 6])]",
        "[math.floor(-2.5), math.ceil(2.1), math.trunc(-2.7), math.fmod(-7, 3), math.remainder(7.5, 5), math.copysign(3, -0.0), math.modf(-3.5), math.frexp(8), math.ldexp(0.5, 4)]",
        "[math.comb(52, 5), math.perm(5, 2), math.factorial(18), math.gcd(12, 18, 27), math.lcm(4, 6, 10), math.isqrt(99), math.prod([1, 2, 3, 4]), math.sumprod([1, 2, 3], [4, 5, 6])]",
        "[math.fsum([0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1]), math.fsum([0.1, 0.2, 0.3, 1e100, -1e100]), math.isclose(1.0, 1.0 + 1e-10), math.pi, math.tau]",
        "[stats.mean(d), stats.stdev(d), stats.variance(d), stats.pvariance(d), stats.pstdev(d), stats.harmonic_mean([40, 60]), stats.fmean([1, 2, 3], [3, 2, 1])]",
        "[stats.median(d), stats.median([1, 3, 5, 7]), stats.median_low([1, 3, 5, 7]), stats.median_high([1, 3, 5, 7]), stats.mode([\"a\", \"b\", \"b\"]), stats.multimode([1, 1, 2, 2, 3])]",
        "[stats.quantiles(d), stats.quantiles(d, 10, \"inclusive\"), stats.quantiles(range(1, 101))]",
        "[stats.covariance(x, y), stats.correlation(x, y), stats.correlation([1, 2, 3, 4, 5], [1, 4, 9, 16, 1000], \"ranked\"), stats.linear_regression([1, 2, 3, 4, 5], [3, 5, 7, 9, 11])]",
        "[stats.normal_inv(0.975), stats.normal_inv(0.3, 10, 2), stats.normal_inv(1e-10), stats.normal_pdf(1, 0, 2)]",
        "[stats.percentile([1, 2, 3, 4], 50), stats.percentile([1, 2, 3, 4], [0, 25, 100]), stats.describe(d), stats.histogram([1, 2, 2, 3, 3, 3, 4], 3)]",
        "[stats.histogram([0.5, 1, 7, 10, 11], [0, 1, 5, 10]), stats.zscore([2, 4, 4, 4, 5, 5, 7, 9]), stats.normalize([10, 15, 20]), stats.rank([10, 20, 20, 30])]",
        "[vec.mod([7, -7, 7.5], 3), vec.add([1, 2, 3], 10), vec.sub(10, [1, 2]), vec.mul([1, 2, 3], [4, 5, 6]), vec.div([1, 2, 3], 2), vec.pow([1, 2, 3], 2), vec.dot([1, 2, 3], [4, 5, 6]), vec.norm([3, 4])]",
        "[vec.cumsum([1, 2, 3, 4]), vec.cumprod([1, 2, 3, 4]), vec.diff([1, 4, 9, 16]), vec.argmin([3, 1, 2, 1]), vec.argmax([3, 1, 3]), vec.argsort([3, 1, 2]), vec.argsort([3, 1, 2], true)]",
        "[vec.clip([1, 5, 10], 2, 8), vec.abs([-1, 2, -3]), vec.round([1.25, 2.5, -2.5]), vec.round([1.234, 5.678], 1), vec.linspace(0, 1, 5), vec.arange(1, 10, 3), vec.arange(0, 1, 0.25)]",
        "[vec.rolling_mean([1, 2, 3, 4, 5], 2), vec.rolling_sum([1, 2, 3, 4, 5], 3), vec.matmul([[1, 2], [3, 4]], [[5, 6], [7, 8]]), vec.matmul([[1, 2], [3, 4]], [1, 1]), vec.transpose([[1, 2, 3], [4, 5, 6]]), vec.column([{ a: 1 }, {}], \"a\")]",
        "math.sqrt(-1)",
        "stats.mean([])",
        "vec.add([1, 2], [1])",
    ] {
        let (s, b) = both(e);
        assert_eq!(s, b, "{}", e);
    }
    // Transcendental functions: the same to the last digit or nearly.
    for e in ["math.erf(0.5)", "math.erfc(3)", "math.erf(-1.5)", "math.gamma(4.5)", "math.gamma(-1.5)", "math.lgamma(100)", "math.lgamma(0.5)", "stats.normal_cdf(1.96)"] {
        let (s, b) = both(e);
        let (s, b): (f64, f64) = (s.parse().unwrap(), b.parse().unwrap());
        assert!(((s - b) / s).abs() < 1e-13, "{}: server {}, browser {}", e, s, b);
    }
}
