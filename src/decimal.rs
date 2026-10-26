//! Decimals: exact numbers for money, and for whatever else is counted in
//! tenths and hundredths.
//!
//! A `dec` is a whole number (up to 96 bits, 28 or 29 digits) and how many
//! of its digits come after the point (0 to 28), as C#'s `decimal` is. The
//! scale is kept, so `12.50m` reads back as "12.50"; equality and order do
//! not see it (`12.5m == 12.50m`).
//!
//! `+`, `-` and `*` are exact while the answer has room: an answer with more
//! digits than fit keeps its whole part and rounds its fraction (half to
//! even, as C# does), and a whole part too big is an overflow. `/` gives the
//! exact answer when it has 28 decimals or fewer, else 28 or 29 significant
//! digits, rounded half to even.

use std::cmp::Ordering;
use std::fmt;

/// The largest scale: digits after the point.
pub const MAX_SCALE: u32 = 28;

/// The largest whole number a decimal holds: 2^96 - 1.
const MAX: u128 = (1 << 96) - 1;

/// A decimal: `lo` and `hi` are the whole number's 96 bits, `neg` its sign
/// (never set for zero), `scale` the digits after the point. Kept in 16
/// bytes, aligned as a u64, so a `Value` holding one is no larger.
#[derive(Clone, Copy)]
pub struct Dec {
    lo: u64,
    hi: u32,
    scale: u8,
    neg: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecError {
    Overflow,
    DivByZero,
}

impl DecError {
    pub fn message(self) -> &'static str {
        match self {
            DecError::Overflow => "decimal overflow",
            DecError::DivByZero => "division by zero",
        }
    }
}

/// How a decimal is rounded to fewer digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rounding {
    /// To the nearest; a tie to the even neighbour (banker's rounding).
    HalfEven,
    /// To the nearest; a tie away from zero (as taught at school).
    HalfUp,
    /// To the nearest; a tie toward zero.
    HalfDown,
    /// Away from zero.
    Up,
    /// Toward zero (truncation).
    Down,
    /// Toward positive infinity.
    Ceiling,
    /// Toward negative infinity.
    Floor,
}

impl Rounding {
    pub const NAMES: &'static str = "half_even, half_up, half_down, up, down, ceiling or floor";

    pub fn parse(s: &str) -> Option<Rounding> {
        Some(match s {
            "half_even" => Rounding::HalfEven,
            "half_up" => Rounding::HalfUp,
            "half_down" => Rounding::HalfDown,
            "up" => Rounding::Up,
            "down" => Rounding::Down,
            "ceiling" => Rounding::Ceiling,
            "floor" => Rounding::Floor,
            _ => return None,
        })
    }

    /// Whether to add one to what is kept, given the first digit dropped,
    /// whether any after it are not zero, and whether what is kept is odd.
    fn bump(self, digit: u64, rest: bool, odd: bool, neg: bool) -> bool {
        let any = digit != 0 || rest;
        match self {
            Rounding::HalfEven => digit > 5 || (digit == 5 && (rest || odd)),
            Rounding::HalfUp => digit >= 5,
            Rounding::HalfDown => digit > 5 || (digit == 5 && rest),
            Rounding::Up => any,
            Rounding::Down => false,
            Rounding::Ceiling => any && !neg,
            Rounding::Floor => any && neg,
        }
    }
}

const fn pow10(k: u32) -> u128 {
    let mut p = 1u128;
    let mut i = 0;
    while i < k {
        p *= 10;
        i += 1;
    }
    p
}

// ---- a 256-bit whole number, for what does not fit 128 ------------------

/// Little-endian 64-bit limbs. Large enough for a 96-bit number times
/// another, or times 10^28, with room to spare.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Wide([u64; 4]);

impl Wide {
    const ZERO: Wide = Wide([0; 4]);

    fn from(x: u128) -> Wide {
        Wide([x as u64, (x >> 64) as u64, 0, 0])
    }

    fn small(self) -> Option<u128> {
        (self.0[2] == 0 && self.0[3] == 0).then(|| self.0[0] as u128 | (self.0[1] as u128) << 64)
    }

    fn is_zero(self) -> bool {
        self.0 == [0; 4]
    }

    fn over_max(self) -> bool {
        self.small().map_or(true, |x| x > MAX)
    }

    fn odd(self) -> bool {
        self.0[0] & 1 == 1
    }

    fn mul_small(self, m: u64) -> Option<Wide> {
        let mut out = [0u64; 4];
        let mut carry = 0u128;
        for i in 0..4 {
            let t = self.0[i] as u128 * m as u128 + carry;
            out[i] = t as u64;
            carry = t >> 64;
        }
        (carry == 0).then_some(Wide(out))
    }

    fn mul_pow10(mut self, mut k: u32) -> Option<Wide> {
        while k > 0 {
            let step = k.min(19);
            self = self.mul_small(pow10(step) as u64)?;
            k -= step;
        }
        Some(self)
    }

    fn add(self, o: Wide) -> Option<Wide> {
        let mut out = [0u64; 4];
        let mut carry = 0u128;
        for i in 0..4 {
            let t = self.0[i] as u128 + o.0[i] as u128 + carry;
            out[i] = t as u64;
            carry = t >> 64;
        }
        (carry == 0).then_some(Wide(out))
    }

    /// self - o, for self >= o.
    fn sub(self, o: Wide) -> Wide {
        let mut out = [0u64; 4];
        let mut borrow = 0i128;
        for i in 0..4 {
            let mut t = self.0[i] as i128 - o.0[i] as i128 - borrow;
            borrow = 0;
            if t < 0 {
                t += 1 << 64;
                borrow = 1;
            }
            out[i] = t as u64;
        }
        Wide(out)
    }

    fn cmp(self, o: Wide) -> Ordering {
        for i in (0..4).rev() {
            match self.0[i].cmp(&o.0[i]) {
                Ordering::Equal => {}
                other => return other,
            }
        }
        Ordering::Equal
    }

    fn divrem_small(self, d: u64) -> (Wide, u64) {
        let mut out = [0u64; 4];
        let mut rem = 0u128;
        for i in (0..4).rev() {
            let cur = rem << 64 | self.0[i] as u128;
            out[i] = (cur / d as u128) as u64;
            rem = cur % d as u128;
        }
        (Wide(out), rem as u64)
    }

    fn bit(self, i: usize) -> bool {
        self.0[i / 64] >> (i % 64) & 1 == 1
    }

    fn shl1(self) -> Wide {
        let mut out = [0u64; 4];
        for i in 0..4 {
            out[i] = self.0[i] << 1 | if i > 0 { self.0[i - 1] >> 63 } else { 0 };
        }
        Wide(out)
    }

    /// The remainder of self / d (d not zero), by shifting and subtracting.
    fn rem(self, d: Wide) -> Wide {
        let mut r = Wide::ZERO;
        for i in (0..256).rev() {
            r = r.shl1();
            if self.bit(i) {
                r.0[0] |= 1;
            }
            if r.cmp(d) != Ordering::Less {
                r = r.sub(d);
            }
        }
        r
    }

    fn mul(a: u128, b: u128) -> Wide {
        let (a0, a1) = (a as u64 as u128, a >> 64);
        let (b0, b1) = (b as u64 as u128, b >> 64);
        let lo = Wide::from(a0 * b0);
        let mid1 = Wide::from(a0 * b1);
        let mid2 = Wide::from(a1 * b0);
        let hi = Wide::from(a1 * b1);
        let shift = |w: Wide, limbs: usize| {
            let mut out = [0u64; 4];
            for i in 0..4 - limbs {
                out[i + limbs] = w.0[i];
            }
            Wide(out)
        };
        lo.add(shift(mid1, 1)).and_then(|x| x.add(shift(mid2, 1))).and_then(|x| x.add(shift(hi, 2))).expect("a product of two 128-bit numbers fits 256 bits")
    }
}

/// `mag`, at `scale`, made to fit: digits past the 28th decimal, or past
/// what 96 bits hold, are dropped and the rest rounded by `mode`. `sticky`
/// says digits already dropped were not all zero. An overflow when the
/// whole part alone does not fit.
fn fit(neg: bool, mut mag: Wide, mut scale: u32, sticky: bool, mode: Rounding) -> Result<Dec, DecError> {
    let (mut digit, mut rest, mut dropped) = (0u64, sticky, sticky);
    loop {
        while scale > MAX_SCALE || mag.over_max() {
            if scale == 0 {
                return Err(DecError::Overflow);
            }
            let (q, r) = mag.divrem_small(10);
            rest |= digit != 0;
            digit = r;
            dropped = true;
            mag = q;
            scale -= 1;
        }
        if dropped && mode.bump(digit, rest, mag.odd(), neg) {
            mag = mag.add(Wide::from(1)).ok_or(DecError::Overflow)?;
            (digit, rest, dropped) = (0, false, false);
            // 9.99 may have become 10.00, one digit too many.
            continue;
        }
        break;
    }
    Ok(Dec::from_parts(neg, mag.small().unwrap(), scale))
}

impl Dec {
    pub const ZERO: Dec = Dec { lo: 0, hi: 0, scale: 0, neg: false };

    /// From a sign, a whole number (at most 2^96 - 1) and a scale (at most 28).
    fn from_parts(neg: bool, mag: u128, scale: u32) -> Dec {
        debug_assert!(mag <= MAX && scale <= MAX_SCALE);
        Dec { lo: mag as u64, hi: (mag >> 64) as u32, scale: scale as u8, neg: neg && mag != 0 }
    }

    /// `m` × 10^-`scale`, if it fits as it is.
    pub fn new(m: i128, scale: u32) -> Option<Dec> {
        (m.unsigned_abs() <= MAX && scale <= MAX_SCALE).then(|| Dec::from_parts(m < 0, m.unsigned_abs(), scale))
    }

    pub fn from_i64(n: i64) -> Dec {
        Dec::from_parts(n < 0, n.unsigned_abs() as u128, 0)
    }

    fn mag(self) -> u128 {
        self.lo as u128 | (self.hi as u128) << 64
    }

    /// The whole number: the decimal is this × 10^-scale.
    pub fn mantissa(self) -> i128 {
        if self.neg {
            -(self.mag() as i128)
        } else {
            self.mag() as i128
        }
    }

    pub fn scale(self) -> u32 {
        self.scale as u32
    }

    pub fn is_zero(self) -> bool {
        self.mag() == 0
    }

    pub fn is_negative(self) -> bool {
        self.neg
    }

    /// Text, as a program writes a number: "12.50", "-0.05", "3", "2.5e-3".
    /// Exactly, or None: digits past what a decimal holds must be zeros.
    pub fn parse(s: &str) -> Option<Dec> {
        Dec::parse_with(s, None)
    }

    /// `parse`, rounding (half to even) digits that do not fit.
    pub fn parse_rounded(s: &str) -> Option<Dec> {
        Dec::parse_with(s, Some(Rounding::HalfEven))
    }

    fn parse_with(s: &str, round: Option<Rounding>) -> Option<Dec> {
        let b = s.as_bytes();
        if b.is_empty() || b.len() > 400 {
            return None;
        }
        let mut i = 0;
        let neg = match b[0] {
            b'-' => {
                i = 1;
                true
            }
            b'+' => {
                i = 1;
                false
            }
            _ => false,
        };
        let mut mag = Wide::ZERO;
        let (mut digits, mut frac) = (0u32, 0i64);
        // Digits past what 256 bits hold: only their being non-zero counts.
        let (mut extra, mut sticky) = (0i64, false);
        let mut seen_point = false;
        while i < b.len() {
            match b[i] {
                c @ b'0'..=b'9' => {
                    digits += 1;
                    match mag.mul_small(10).and_then(|m| m.add(Wide::from((c - b'0') as u128))) {
                        Some(m) if extra == 0 => mag = m,
                        _ => {
                            extra += 1;
                            sticky |= c != b'0';
                        }
                    }
                    if seen_point {
                        frac += 1;
                    }
                }
                b'.' if !seen_point => seen_point = true,
                _ => break,
            }
            i += 1;
        }
        if digits == 0 {
            return None;
        }
        let mut exp = 0i64;
        if i < b.len() {
            if !matches!(b[i], b'e' | b'E') {
                return None;
            }
            let e = std::str::from_utf8(&b[i + 1..]).ok()?;
            let e = e.strip_prefix('+').unwrap_or(e);
            if e.is_empty() || e.len() > 6 || !e.trim_start_matches('-').bytes().all(|c| c.is_ascii_digit()) {
                return None;
            }
            exp = e.parse().ok()?;
        }
        // The value is mag × 10^(extra + exp - frac).
        let mut scale = frac - exp - extra;
        if scale < 0 {
            // A whole number past 77 digits does not fit, whatever they are.
            if extra > 0 {
                return None;
            }
            if !mag.is_zero() {
                mag = mag.mul_pow10(u32::try_from(-scale).ok()?)?;
            }
            scale = 0;
        }
        if scale > 400 {
            return None;
        }
        match round {
            Some(mode) => fit(neg, mag, scale as u32, sticky, mode).ok(),
            None => {
                if sticky {
                    return None;
                }
                // Only zeros may go.
                let mut scale = scale as u32;
                while scale > MAX_SCALE || mag.over_max() {
                    let (q, r) = mag.divrem_small(10);
                    if r != 0 || scale == 0 {
                        return None;
                    }
                    mag = q;
                    scale -= 1;
                }
                Some(Dec::from_parts(neg, mag.small()?, scale))
            }
        }
    }

    /// A float, through its shortest text (so 0.1 is 0.1, not
    /// 0.1000000000000000055511151231257827). None for NaN, the infinities
    /// and what is too large; digits past the 28th decimal are rounded.
    pub fn from_f64(f: f64) -> Option<Dec> {
        if !f.is_finite() {
            return None;
        }
        Dec::parse_rounded(&format!("{:e}", f))
    }

    /// The nearest float (from the text, so correctly rounded).
    pub fn to_f64(self) -> f64 {
        self.to_string().parse().unwrap_or(f64::NAN)
    }

    /// The whole part, as an int (None when it does not fit 64 bits).
    pub fn trunc_i64(self) -> Option<i64> {
        let whole = self.mag() / pow10(self.scale as u32);
        if self.neg {
            i64::try_from(-(whole as i128)).ok()
        } else {
            i64::try_from(whole).ok()
        }
    }

    pub fn neg(self) -> Dec {
        Dec { neg: !self.neg && !self.is_zero(), ..self }
    }

    pub fn abs(self) -> Dec {
        Dec { neg: false, ..self }
    }

    /// The same number with no zeros at the end of its fraction: what is
    /// written for it as a map key, so 12.5 and 12.50 are one key.
    pub fn normalized(self) -> Dec {
        let (mut m, mut s) = (self.mag(), self.scale as u32);
        while s > 0 && m % 10 == 0 {
            m /= 10;
            s -= 1;
        }
        Dec::from_parts(self.neg, m, s)
    }

    pub fn add(self, o: Dec) -> Result<Dec, DecError> {
        // The usual case: the same scale (cents and cents).
        if self.scale == o.scale {
            let m = self.mantissa() + o.mantissa();
            if m.unsigned_abs() <= MAX {
                return Ok(Dec::from_parts(m < 0, m.unsigned_abs(), self.scale as u32));
            }
        }
        let scale = self.scale.max(o.scale) as u32;
        let a = Wide::from(self.mag()).mul_pow10(scale - self.scale as u32).ok_or(DecError::Overflow)?;
        let b = Wide::from(o.mag()).mul_pow10(scale - o.scale as u32).ok_or(DecError::Overflow)?;
        if self.neg == o.neg {
            fit(self.neg, a.add(b).ok_or(DecError::Overflow)?, scale, false, Rounding::HalfEven)
        } else if a.cmp(b) != Ordering::Less {
            fit(self.neg, a.sub(b), scale, false, Rounding::HalfEven)
        } else {
            fit(o.neg, b.sub(a), scale, false, Rounding::HalfEven)
        }
    }

    pub fn sub(self, o: Dec) -> Result<Dec, DecError> {
        self.add(o.neg())
    }

    pub fn mul(self, o: Dec) -> Result<Dec, DecError> {
        let neg = self.neg != o.neg;
        let scale = self.scale as u32 + o.scale as u32;
        if let Some(m) = self.mag().checked_mul(o.mag()) {
            if m <= MAX && scale <= MAX_SCALE {
                return Ok(Dec::from_parts(neg, m, scale));
            }
        }
        fit(neg, Wide::mul(self.mag(), o.mag()), scale, false, Rounding::HalfEven)
    }

    pub fn div(self, o: Dec) -> Result<Dec, DecError> {
        if o.is_zero() {
            return Err(DecError::DivByZero);
        }
        let neg = self.neg != o.neg;
        let (a, b) = (self.mag(), o.mag());
        let mut scale = self.scale as i32 - o.scale as i32;
        let mut q = Wide::from(a / b);
        let mut r = a % b;
        // Long division, a digit at a time, while there is a remainder and
        // room for the digit (one past it, to round by).
        while r != 0 && (scale < 0 || (scale <= MAX_SCALE as i32 && !q.over_max())) {
            r *= 10;
            q = q.mul_small(10).and_then(|x| x.add(Wide::from(r / b))).ok_or(DecError::Overflow)?;
            r %= b;
            scale += 1;
        }
        if scale < 0 {
            q = q.mul_pow10((-scale) as u32).ok_or(DecError::Overflow)?;
            scale = 0;
        }
        fit(neg, q, scale as u32, r != 0, Rounding::HalfEven)
    }

    /// The remainder of truncated division, as `%` on ints: its sign is
    /// the dividend's.
    pub fn rem(self, o: Dec) -> Result<Dec, DecError> {
        if o.is_zero() {
            return Err(DecError::DivByZero);
        }
        let scale = self.scale.max(o.scale) as u32;
        let a = Wide::from(self.mag()).mul_pow10(scale - self.scale as u32).ok_or(DecError::Overflow)?;
        let b = Wide::from(o.mag()).mul_pow10(scale - o.scale as u32).ok_or(DecError::Overflow)?;
        let r = match (a.small(), b.small()) {
            (Some(x), Some(y)) => Wide::from(x % y),
            _ => a.rem(b),
        };
        fit(self.neg, r, scale, false, Rounding::HalfEven)
    }

    /// Rounded to `digits` after the point (a negative number rounds to
    /// tens, hundreds, ...). With fewer digits than that already, zeros
    /// are added, so `round(12.5m, 2)` is 12.50: what amounts are shown as.
    pub fn round(self, digits: i32, mode: Rounding) -> Result<Dec, DecError> {
        let digits = digits.clamp(-(MAX_SCALE as i32), MAX_SCALE as i32);
        let scale = self.scale as i32;
        if digits >= scale {
            // Padded, when there is room for the zeros.
            return Ok(Wide::from(self.mag())
                .mul_pow10((digits - scale) as u32)
                .and_then(|w| w.small())
                .filter(|&m| m <= MAX)
                .map_or(self, |m| Dec::from_parts(self.neg, m, digits as u32)));
        }
        // Drop `scale - digits` digits, rounding by the first one dropped.
        let drop = (scale - digits) as u32;
        let mag = self.mag();
        let p = pow10(drop.min(38));
        let (kept, gone) = if drop > 38 { (0, mag) } else { (mag / p, mag % p) };
        let (digit, rest) = if drop > 38 {
            (0, gone != 0)
        } else {
            let unit = p / 10;
            ((gone / unit) as u64, gone % unit != 0)
        };
        let kept = if mode.bump(digit, rest, kept & 1 == 1, self.neg) { kept + 1 } else { kept };
        if digits >= 0 {
            fit(self.neg, Wide::from(kept), digits as u32, false, mode)
        } else {
            let back = Wide::from(kept).mul_pow10((-digits) as u32).ok_or(DecError::Overflow)?;
            fit(self.neg, back, 0, false, mode)
        }
    }

    /// Ordered as numbers, whatever the scales.
    pub fn cmp(self, o: Dec) -> Ordering {
        match (self.neg, o.neg) {
            (false, true) => return Ordering::Greater,
            (true, false) => return Ordering::Less,
            _ => {}
        }
        let mags = if self.scale == o.scale {
            self.mag().cmp(&o.mag())
        } else {
            let scale = self.scale.max(o.scale) as u32;
            let a = Wide::from(self.mag()).mul_pow10(scale - self.scale as u32).unwrap();
            let b = Wide::from(o.mag()).mul_pow10(scale - o.scale as u32).unwrap();
            a.cmp(b)
        };
        if self.neg {
            mags.reverse()
        } else {
            mags
        }
    }
}

impl PartialEq for Dec {
    fn eq(&self, o: &Dec) -> bool {
        self.cmp(*o) == Ordering::Equal
    }
}

impl PartialOrd for Dec {
    fn partial_cmp(&self, o: &Dec) -> Option<Ordering> {
        Some(Dec::cmp(*self, *o))
    }
}

impl std::hash::Hash for Dec {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        let n = self.normalized();
        (n.mantissa(), n.scale).hash(h)
    }
}

impl fmt::Display for Dec {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let digits = self.mag().to_string();
        let scale = self.scale as usize;
        let mut out = String::with_capacity(digits.len() + 3);
        if self.neg {
            out.push('-');
        }
        if scale == 0 {
            out.push_str(&digits);
        } else if digits.len() > scale {
            out.push_str(&digits[..digits.len() - scale]);
            out.push('.');
            out.push_str(&digits[digits.len() - scale..]);
        } else {
            out.push_str("0.");
            out.extend(std::iter::repeat('0').take(scale - digits.len()));
            out.push_str(&digits);
        }
        f.write_str(&out)
    }
}

impl fmt::Debug for Dec {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}m", self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Dec {
        Dec::parse(s).unwrap_or_else(|| panic!("{} parses", s))
    }

    #[test]
    fn parses_and_shows_its_digits() {
        for s in ["0", "12.50", "-0.05", "79228162514264337593543950335", "0.0000000000000000000000000001", "-7.9228162514264337593543950335"] {
            assert_eq!(d(s).to_string(), s);
        }
        assert_eq!(d("+.5").to_string(), "0.5");
        assert_eq!(d("5.").to_string(), "5");
        assert_eq!(d("2.5e-3").to_string(), "0.0025");
        assert_eq!(d("1.2E3").to_string(), "1200");
        assert_eq!(d("-0.00").to_string(), "0.00");
        assert_eq!(d("1.500000000000000000000000000000000").to_string(), "1.5000000000000000000000000000");
        for bad in ["", "-", ".", "1.2.3", "12a", "1e", "e5", "0x10", "1_000", "79228162514264337593543950336", "0.00000000000000000000000000001", "1e29"] {
            assert!(Dec::parse(bad).is_none(), "{:?} is refused", bad);
        }
    }

    #[test]
    fn floats_go_through_their_shortest_text() {
        assert_eq!(Dec::from_f64(0.1).unwrap().to_string(), "0.1");
        assert_eq!(Dec::from_f64(0.1 + 0.2).unwrap().to_string(), "0.30000000000000004");
        assert_eq!(Dec::from_f64(3.0).unwrap().to_string(), "3");
        assert_eq!(Dec::from_f64(-1234.5).unwrap().to_string(), "-1234.5");
        assert_eq!(Dec::from_f64(1e20).unwrap().to_string(), "100000000000000000000");
        assert!(Dec::from_f64(1e30).is_none() && Dec::from_f64(f64::NAN).is_none());
        assert_eq!(d("0.1").to_f64(), 0.1);
    }

    #[test]
    fn adds_subtracts_and_multiplies_exactly() {
        assert_eq!(d("0.1").add(d("0.2")).unwrap().to_string(), "0.3");
        assert_eq!(d("12.50").add(d("0.5")).unwrap().to_string(), "13.00");
        assert_eq!(d("1").sub(d("0.01")).unwrap().to_string(), "0.99");
        assert_eq!(d("1.10").mul(d("3")).unwrap().to_string(), "3.30");
        assert_eq!(d("-0.5").mul(d("0.5")).unwrap().to_string(), "-0.25");
        assert_eq!(d("79228162514264337593543950335").add(d("1")), Err(DecError::Overflow));
        assert_eq!(d("79228162514264337593543950335").mul(d("2")), Err(DecError::Overflow));
        // Too many decimals: the fraction is rounded, the whole part kept.
        assert_eq!(d("10000000000000000000000000000").add(d("0.5")).unwrap().to_string(), "10000000000000000000000000000");
        assert_eq!(
            d("1.2345678901234567890123456789").mul(d("1.2345678901234567890123456789")).unwrap().to_string(),
            "1.5241578753238836750495351563"
        );
        assert_eq!(d("12.5").cmp(d("12.50")), Ordering::Equal);
        assert!(d("-1.5") < d("-1.25") && d("0.001") > d("-1000"));
    }

    #[test]
    fn divides_to_28_digits_half_even() {
        let div = |a: &str, b: &str| d(a).div(d(b)).unwrap().to_string();
        assert_eq!(div("1", "3"), "0.3333333333333333333333333333");
        assert_eq!(div("2", "3"), "0.6666666666666666666666666667");
        assert_eq!(div("10", "3"), "3.3333333333333333333333333333");
        assert_eq!(div("100", "3"), "33.333333333333333333333333333");
        assert_eq!(div("10", "4"), "2.5");
        assert_eq!(div("1.00", "2"), "0.50");
        assert_eq!(div("100", "0.5"), "200");
        assert_eq!(div("-7", "2"), "-3.5");
        assert_eq!(d("1").div(Dec::ZERO), Err(DecError::DivByZero));
        assert_eq!(d("7.5").rem(d("2")).unwrap().to_string(), "1.5");
        assert_eq!(d("-7.5").rem(d("2")).unwrap().to_string(), "-1.5");
    }

    #[test]
    fn rounds_every_way() {
        let r = |s: &str, n: i32, m: Rounding| d(s).round(n, m).unwrap().to_string();
        use Rounding::*;
        let cases = [
            ("2.5", ["2", "3", "2", "3", "2", "3", "2"]),
            ("-2.5", ["-2", "-3", "-2", "-3", "-2", "-2", "-3"]),
            ("3.5", ["4", "4", "3", "4", "3", "4", "3"]),
            ("2.51", ["3", "3", "3", "3", "2", "3", "2"]),
            ("-2.49", ["-2", "-2", "-2", "-3", "-2", "-2", "-3"]),
            ("2.0", ["2", "2", "2", "2", "2", "2", "2"]),
        ];
        for (x, want) in cases {
            for (m, w) in [HalfEven, HalfUp, HalfDown, Up, Down, Ceiling, Floor].into_iter().zip(want) {
                assert_eq!(r(x, 0, m), w, "{} {:?}", x, m);
            }
        }
        assert_eq!(r("1.005", 2, HalfEven), "1.00");
        assert_eq!(r("1.015", 2, HalfEven), "1.02");
        assert_eq!(r("12.5", 2, HalfEven), "12.50");
        assert_eq!(r("1250", -2, HalfEven), "1200");
        assert_eq!(r("1350", -2, HalfEven), "1400");
        assert_eq!(r("-0.004", 2, HalfUp), "0.00");
        assert_eq!(r("9.999", 2, HalfUp), "10.00");
    }
}
