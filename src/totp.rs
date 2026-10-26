//! A second factor: one-time codes from an authenticator app (TOTP, RFC
//! 6238), and recovery codes for when the phone is lost.
//!
//! ```text
//! let secret = crypto.totp_secret()                         // base32: keep it with the user
//! crypto.totp_uri(secret, "ada@acme.test", "Acme")          // otpauth://… for the app
//! let step = crypto.totp_verify(secret, code, { after: user.totp_step })   // its time step, or null
//! let codes = crypto.recovery_codes(10)                     // [{ code, hash }]: show codes, keep hashes
//! crypto.verify_recovery_code(entered, hashes)              // which one it is, or null
//! ```
//!
//! Codes are HMAC-SHA1 of the 30-second time step, 6 digits, as every
//! authenticator app makes them. `totp_verify` takes the code of the
//! current step and of `window` steps either side (1 unless given: clocks
//! drift), comparing each in constant time. A code works once: the program
//! keeps the step `totp_verify` gave back, and passes it as `after` the
//! next time, so a code seen over someone's shoulder, or used already, is
//! refused.
//!
//! Recovery codes are 12 characters of Crockford's base32 (60 bits), shown
//! as `7k2m-x9qp-4hzt`; what the program keeps is their Argon2id hashes,
//! as for passwords. Typed with or without dashes, in either case, with
//! `o` for `0` or `l` for `1`, they are the same code.

use crate::interp::{fail, Ctrl, Interp};
use crate::value::*;

type R = Result<Value, Ctrl>;

const PERIOD: i64 = 30;
const DIGITS: u32 = 6;
const B32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
/// Crockford's: no I, L, O or U, which are read as others.
const CROCKFORD: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

pub fn base32(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(5) * 8);
    let (mut acc, mut bits) = (0u32, 0);
    for &b in bytes {
        acc = (acc << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(B32[((acc >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(B32[((acc << (5 - bits)) & 31) as usize] as char);
    }
    out
}

/// Base32 as authenticator apps show it: either case, spaces and `=` ignored.
pub fn unbase32(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes().filter(|c| !matches!(c, b' ' | b'-' | b'=')) {
        let v = B32.iter().position(|&x| x == c.to_ascii_uppercase())? as u32;
        acc = (acc << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

pub fn hmac_sha1(key: &[u8], msg: &[u8]) -> [u8; 20] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..20].copy_from_slice(&crate::ws::sha1(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = k.map(|b| b ^ 0x36).to_vec();
    inner.extend_from_slice(msg);
    let mut outer = k.map(|b| b ^ 0x5c).to_vec();
    outer.extend_from_slice(&crate::ws::sha1(&inner));
    crate::ws::sha1(&outer)
}

/// The HOTP code (RFC 4226) for `counter`, as a number of `digits` digits.
pub fn hotp(key: &[u8], counter: u64, digits: u32) -> u32 {
    let h = hmac_sha1(key, &counter.to_be_bytes());
    let at = (h[19] & 15) as usize;
    let n = u32::from_be_bytes([h[at] & 0x7f, h[at + 1], h[at + 2], h[at + 3]]);
    n % 10u32.pow(digits)
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

fn text(args: &[Value], i: usize, f: &str, what: &str, line: u32) -> Result<std::sync::Arc<str>, Ctrl> {
    match args.get(i) {
        Some(Value::Str(s)) => Ok(s.clone()),
        other => fail(line, format!("`crypto.{}` expects {} (a str), got {}", f, what, other.map_or("nothing", |v| v.type_name()))),
    }
}

/// `crypto.totp_secret(bytes = 20)`: a new secret, in base32.
pub fn totp_secret(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let n = match args.first() {
        None | Some(Value::Nil) => 20,
        Some(Value::Int(n)) if (10..=64).contains(n) => *n as usize,
        Some(v) => return fail(line, format!("`crypto.totp_secret` takes a number of bytes from 10 to 64, got {}", v.repr())),
    };
    match crate::crypto::random_bytes(n) {
        Some(b) => Ok(Value::str(base32(&b))),
        None => fail(line, "`crypto.totp_secret`: the operating system gave no random bytes"),
    }
}

/// Percent-encodes all but letters, digits and `-._~`.
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{:02X}", b) })
        .collect()
}

/// `crypto.totp_uri(secret, account, issuer)`: the `otpauth://` address an
/// authenticator app takes (as a link, or a QR code of it).
pub fn totp_uri(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let secret = text(&args, 0, "totp_uri", "the secret", line)?;
    let account = text(&args, 1, "totp_uri", "the account (an email, a name)", line)?;
    let issuer = match args.get(2) {
        None | Some(Value::Nil) => None,
        Some(Value::Str(s)) => Some(s.clone()),
        Some(v) => return fail(line, format!("`crypto.totp_uri`'s issuer is a str, got {}", v.type_name())),
    };
    if unbase32(&secret).map_or(true, |k| k.is_empty()) {
        return fail(line, "`crypto.totp_uri` needs a base32 secret, as `crypto.totp_secret` makes");
    }
    let clean: String = secret.chars().filter(|c| !matches!(c, ' ' | '-' | '=')).collect::<String>().to_ascii_uppercase();
    let label = match &issuer {
        Some(i) => format!("{}:{}", encode(i), encode(&account)),
        None => encode(&account),
    };
    let mut uri = format!("otpauth://totp/{}?secret={}", label, clean);
    if let Some(i) = &issuer {
        uri.push_str(&format!("&issuer={}", encode(i)));
    }
    uri.push_str("&algorithm=SHA1&digits=6&period=30");
    Ok(Value::str(uri))
}

/// The time step a code is right for, among `step - window ..= step +
/// window` and after `after`. Every one is compared, the same way, so the
/// time taken tells nothing.
pub fn check(key: &[u8], code: &str, step: i64, window: i64, after: Option<i64>) -> Option<i64> {
    let code: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    if code.len() != DIGITS as usize || !code.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut found = None;
    for s in step - window..=step + window {
        let want = format!("{:06}", hotp(key, s.max(0) as u64, DIGITS));
        let fresh = after.map_or(true, |a| s > a);
        if crate::crypto::same(want.as_bytes(), code.as_bytes()) && fresh && found.is_none() {
            found = Some(s);
        }
    }
    found
}

/// `crypto.totp_verify(secret, code, { window: 1, after, at })`: the time
/// step the code is right for, or null. `after`: the step of the code used
/// last (refused, as are those before it). `at`: the time, in seconds since
/// 1970, if not now.
pub fn totp_verify(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let secret = text(&args, 0, "totp_verify", "the secret", line)?;
    let code = match args.get(1) {
        Some(Value::Str(s)) => s.to_string(),
        Some(Value::Int(n)) => format!("{:06}", n),
        Some(Value::Nil) | None => return Ok(Value::Nil),
        Some(v) => return fail(line, format!("`crypto.totp_verify`'s code is a str, got {}", v.type_name())),
    };
    let Some(key) = unbase32(&secret).filter(|k| !k.is_empty()) else {
        return fail(line, "`crypto.totp_verify` needs a base32 secret, as `crypto.totp_secret` makes");
    };
    let (mut window, mut after, mut at) = (1, None, now());
    match args.get(2) {
        None | Some(Value::Nil) => {}
        Some(Value::Map(m)) => {
            for (k, v) in m.iter() {
                match (&**k, v) {
                    ("window", Value::Int(n)) if (0..=10).contains(n) => window = *n,
                    ("after", Value::Int(n)) => after = Some(*n),
                    ("after", Value::Nil) => {}
                    ("at", Value::Int(n)) => at = *n,
                    ("at", Value::Float(f)) => at = *f as i64,
                    _ => return fail(line, format!("`crypto.totp_verify` takes window (0 to 10), after (a step) and at (seconds), got {}: {}", k, v.repr())),
                }
            }
        }
        Some(v) => return fail(line, format!("`crypto.totp_verify` takes options as a map, got {}", v.type_name())),
    }
    Ok(check(&key, &code, at.div_euclid(PERIOD), window, after).map_or(Value::Nil, Value::Int))
}

/// A recovery code as typed, the way it was hashed: 12 characters of
/// Crockford's base32, lowercase.
fn canonical(code: &str) -> String {
    code.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| match c.to_ascii_lowercase() {
            'o' => '0',
            'i' | 'l' => '1',
            c => c,
        })
        .collect()
}

/// `crypto.recovery_codes(n = 10)`: n codes to show the user once, each
/// with the hash to keep: `[{ code, hash }]`.
pub fn recovery_codes(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let n = match args.first() {
        None | Some(Value::Nil) => 10,
        Some(Value::Int(n)) if (1..=50).contains(n) => *n as usize,
        Some(v) => return fail(line, format!("`crypto.recovery_codes` makes 1 to 50 codes, got {}", v.repr())),
    };
    let Some(bytes) = crate::crypto::random_bytes(n * 12) else {
        return fail(line, "`crypto.recovery_codes`: the operating system gave no random bytes");
    };
    let codes: Vec<String> = bytes.chunks(12).map(|c| c.iter().map(|b| CROCKFORD[(b & 31) as usize] as char).collect()).collect();
    let plain = codes.clone();
    let hashed = crate::crypto::hashing(it, move || plain.iter().map(|c| crate::crypto::argon_hash(c.as_bytes())).collect::<Result<Vec<_>, _>>());
    let hashes = match hashed.and_then(|r| r) {
        Ok(h) => h,
        Err(e) => return fail(line, format!("`crypto.recovery_codes`: {}", e)),
    };
    let list = codes
        .iter()
        .zip(hashes)
        .map(|(c, h)| {
            let mut m = Map::new();
            m.insert("code".into(), Value::str(format!("{}-{}-{}", &c[..4], &c[4..8], &c[8..])));
            m.insert("hash".into(), Value::str(h));
            Value::map(m)
        })
        .collect();
    Ok(Value::list(list))
}

/// `crypto.verify_recovery_code(code, hashes)`: the index of the hash the
/// code was made with, or null. The program then removes that hash: each
/// code works once.
pub fn verify_recovery_code(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let code = canonical(&text(&args, 0, "verify_recovery_code", "the code typed", line)?);
    let hashes: Vec<Option<String>> = match args.get(1) {
        Some(Value::List(l)) => l.iter().map(|v| if let Value::Str(s) = v { Some(s.to_string()) } else { None }).collect(),
        Some(Value::Nil) | None => Vec::new(),
        Some(v) => return fail(line, format!("`crypto.verify_recovery_code` expects the list of hashes kept, got {}", v.type_name())),
    };
    let well_formed = code.len() == 12;
    let found = crate::crypto::hashing(it, move || {
        if hashes.is_empty() || !well_formed {
            // As long as a check would take.
            let _ = crate::crypto::argon_check(code.as_bytes(), None);
            return None;
        }
        hashes.iter().position(|h| crate::crypto::argon_check(code.as_bytes(), h.as_deref()))
    });
    match found {
        Ok(i) => Ok(i.map_or(Value::Nil, |i| Value::Int(i as i64))),
        Err(e) => fail(line, format!("`crypto.verify_recovery_code`: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc_vectors() {
        // RFC 4226, appendix D.
        let key = b"12345678901234567890";
        let want = [755224, 287082, 359152, 969429, 338314, 254676, 287922, 162583, 399871, 520489];
        for (i, w) in want.iter().enumerate() {
            assert_eq!(hotp(key, i as u64, 6), *w);
        }
        // RFC 6238, appendix B (SHA-1), as 8 digits and as the 6 apps show.
        for (t, code) in [(59, 94287082), (1111111109, 7081804), (1111111111, 14050471), (1234567890, 89005924), (2000000000, 69279037), (20000000000i64, 65353130)] {
            assert_eq!(hotp(key, (t / 30) as u64, 8), code);
            assert_eq!(check(key, &format!("{:06}", code % 1_000_000), t / 30, 0, None), Some(t / 30));
        }
        // A step later or earlier is in a window of 1, not of 0; a step used is not again.
        let c = format!("{:06}", hotp(key, 100, 6));
        assert_eq!(check(key, &c, 101, 1, None), Some(100));
        assert_eq!(check(key, &c, 101, 0, None), None);
        assert_eq!(check(key, &c, 100, 1, Some(100)), None);
        assert_eq!(check(key, "12345", 100, 1, None), None);
        assert_eq!(base32(b"12345678901234567890"), "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ");
        assert_eq!(unbase32("gezd gnbv gy3t qojq gezd gnbv gy3t qojq").unwrap(), b"12345678901234567890");
        assert_eq!(unbase32("GE1"), None);
        // RFC 2202, test case 2.
        assert_eq!(crate::crypto::hex(&hmac_sha1(b"Jefe", b"what do ya want for nothing?")), "effcdf6ae5eb2fa2d27416d5f184df9c259a7c79");
        assert_eq!(canonical("7K2M-X9QP-4HZO"), "7k2mx9qp4hz0");
    }
}
