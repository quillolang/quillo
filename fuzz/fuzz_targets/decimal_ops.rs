//! Decimal `+`, `-`, `*` and comparison (src/decimal.rs) against exact
//! arithmetic on integers of any size: the same answer, rounded where the
//! guide says (as many decimals as fit, half to even), or an overflow
//! exactly where the whole part does not fit.
#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use num_bigint::BigInt;
use quillo::decimal::Dec;

#[path = "common_dec.rs"]
mod common_dec;
use common_dec::*;

#[derive(Arbitrary, Debug)]
struct Input {
    a: (i128, u8),
    b: (i128, u8),
}

fn dec((m, s): (i128, u8)) -> Dec {
    // Any 96-bit whole number and scale 0 to 28.
    let m = m % (1i128 << 96);
    Dec::new(m, s as u32 % 29).unwrap()
}

fn check(what: &str, a: Dec, b: Dec, got: Result<Dec, quillo::decimal::DecError>, n: BigInt, s: u32) {
    let want = fitted(&n, s);
    match (got, want) {
        (Ok(d), Some(w)) => {
            let g = exact(d);
            // The same number, at the scale that fits.
            assert!(g.0 == w.0 && g.1 == w.1, "{} {} {}: got {} ({:?}), want {:?}", a, what, b, d, g, w);
        }
        (Err(_), None) => {}
        (got, want) => panic!("{} {} {}: got {:?}, want {:?}", a, what, b, got.map(|d| d.to_string()), want),
    }
}

fuzz_target!(|i: Input| {
    let (a, b) = (dec(i.a), dec(i.b));
    let (x, y) = (exact(a), exact(b));
    let s = x.1.max(y.1);
    let (xs, ys) = (&x.0 * pow10(s - x.1), &y.0 * pow10(s - y.1));
    check("+", a, b, a.add(b), &xs + &ys, s);
    check("-", a, b, a.sub(b), &xs - &ys, s);
    check("*", a, b, a.mul(b), &x.0 * &y.0, x.1 + y.1);
    assert_eq!(a.cmp(b), cmp(&x, &y), "{} <=> {}", a, b);
    assert_eq!(a == b, cmp(&x, &y).is_eq());
});
