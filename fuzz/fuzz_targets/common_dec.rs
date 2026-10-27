//! The decimal targets' reference: a decimal as an exact fraction, n / 10^s,
//! with integers of any size.
#![allow(dead_code)]

use num_bigint::BigInt;
use num_traits::{Signed, Zero};
use quillo::decimal::Dec;

/// The largest whole number a decimal holds, and its largest scale.
pub fn max() -> BigInt {
    (BigInt::from(1) << 96) - 1
}
pub const MAX_SCALE: u32 = 28;

pub fn pow10(n: u32) -> BigInt {
    num_traits::pow(BigInt::from(10), n as usize)
}

/// `d` as (n, s): d = n / 10^s.
pub fn exact(d: Dec) -> (BigInt, u32) {
    (BigInt::from(d.mantissa()), d.scale())
}

/// a/10^sa compared with b/10^sb.
pub fn cmp(a: &(BigInt, u32), b: &(BigInt, u32)) -> std::cmp::Ordering {
    let s = a.1.max(b.1);
    (&a.0 * pow10(s - a.1)).cmp(&(&b.0 * pow10(s - b.1)))
}

/// n / 10^k, rounded half to even.
pub fn round_div(n: &BigInt, k: u32) -> BigInt {
    let d = pow10(k);
    let (q, r) = (n.abs() / &d, n.abs() % &d);
    let twice = &r * 2;
    let up = twice > d || (twice == d && (&q % 2u32) == BigInt::from(1));
    let q = if up { q + 1 } else { q };
    if n.is_negative() {
        -q
    } else {
        q
    }
}

/// What a decimal answer of n / 10^s should be: as many digits after the
/// point as fit (at most 28), the last rounded half to even; None when its
/// whole part does not fit.
pub fn fitted(n: &BigInt, s: u32) -> Option<(BigInt, u32)> {
    let mut t = s.min(MAX_SCALE);
    loop {
        let q = round_div(n, s - t);
        if q.abs() <= max() {
            // The same number, as the decimal keeps it: its scale as it is
            // (trailing zeros included).
            return Some((if q.is_zero() { BigInt::zero() } else { q }, t));
        }
        if t == 0 {
            return None;
        }
        t -= 1;
    }
}
