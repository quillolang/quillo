//! Decimal text (`dec("...")`, `12.50m`, a `dec` field in a form): what
//! reads is exactly the number written, it writes back as it reads, and a
//! number a decimal can hold is not refused.
#![no_main]

use libfuzzer_sys::fuzz_target;
use num_bigint::BigInt;
use num_traits::Zero;
use quillo::decimal::Dec;

#[path = "common_dec.rs"]
mod common_dec;
use common_dec::*;

/// The text as a number, n × 10^e, if it is written as one: a sign, digits
/// with perhaps a point, perhaps an exponent.
fn read(s: &str) -> Option<(BigInt, i64)> {
    let b = s.strip_prefix(['+', '-']).unwrap_or(s);
    let neg = s.starts_with('-');
    let (num, exp) = match b.find(['e', 'E']) {
        Some(i) => (&b[..i], &b[i + 1..]),
        None => (b, "0"),
    };
    // An exponent: one sign at most, then digits.
    let digits = exp.strip_prefix(['+', '-']).unwrap_or(exp);
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let (whole, frac) = num.split_once('.').unwrap_or((num, ""));
    if whole.len() + frac.len() == 0 || !(whole.bytes().chain(frac.bytes())).all(|c| c.is_ascii_digit()) {
        return None;
    }
    let e: i64 = exp.parse().ok()?;
    let n: BigInt = format!("{}{}", whole, frac).parse().ok()?;
    Some((if neg { -n } else { n }, e - frac.len() as i64))
}

fuzz_target!(|s: &str| {
    let d = Dec::parse(s);
    let r = Dec::parse_rounded(s);
    if let Some(d) = d {
        // Exactly the number written.
        let (n, e) = read(s).unwrap_or_else(|| panic!("{:?} read as {} but is not a number", s, d));
        let x = exact(d);
        let written = if e >= 0 { (n * pow10(e as u32), 0) } else { (n, (-e) as u32) };
        assert!(cmp(&x, &written).is_eq(), "{:?} read as {}", s, d);
        // Written back, it reads the same, scale and all.
        let again = Dec::parse(&d.to_string()).unwrap_or_else(|| panic!("{} (from {:?}) does not read back", d, s));
        assert_eq!((again.mantissa(), again.scale()), (d.mantissa(), d.scale()), "{:?}", s);
        // Rounding changes nothing that fits.
        assert_eq!(r.map(|r| (r.mantissa(), r.scale())), Some((d.mantissa(), d.scale())), "{:?}", s);
    } else if s.len() <= 400 {
        // Refused: not a number, or not one a decimal holds exactly.
        if let Some((n, e)) = read(s) {
            // Exponents are at most 6 digits, 5 after a `-`.
            let exp_text = s.find(['e', 'E']).map_or("", |i| &s[i + 1..]);
            if exp_text.trim_start_matches('+').len() <= 6 {
                // The fewest decimals it can be written with.
                let (mut n, mut e) = (n, e);
                while e < 0 && !n.is_zero() && (&n % 10u32).is_zero() {
                    n /= 10u32;
                    e += 1;
                }
                if n.is_zero() {
                    e = e.max(0);
                }
                let fits = if e >= 0 {
                    e < 40 && (&n * pow10(e as u32)).magnitude() <= max().magnitude()
                } else {
                    -e <= MAX_SCALE as i64 && n.magnitude() <= max().magnitude()
                };
                assert!(!fits, "{:?} was refused, but a decimal holds it", s);
            }
        }
    }
    if let Some(r) = r {
        // Rounded: within half a unit of its last digit.
        if let Some((n, e)) = read(s) {
            let x = exact(r);
            let scale = x.1 + 1 + if e < 0 { (-e) as u32 } else { 0 };
            let want = if e >= 0 { n * pow10(e as u32) * pow10(scale) } else { n * pow10(scale - (-e) as u32) };
            let got = &x.0 * pow10(scale - x.1);
            let half = pow10(scale - x.1) / 2u32;
            assert!((got - want).magnitude() <= half.magnitude(), "{:?} rounded to {}", s, r);
        }
    }
});

