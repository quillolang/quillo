//! `crypto`: hashes, signatures, tokens and passwords.
//!
//! ```text
//! crypto.sha256("text")                    // hex
//! crypto.hmac(env("SECRET"), "message")    // hex, HMAC-SHA256
//! crypto.token()                           // 32 random bytes, base64url (43 characters)
//! let h = crypto.hash_password(password)   // Argon2id, salted: store this
//! crypto.verify_password(password, h)      // true or false
//! crypto.equal(a, b)                       // compares in constant time
//! crypto.totp_secret()                     // a second factor (see totp.rs)
//! ```
//!
//! Password hashing takes tens of milliseconds and some memory on purpose
//! (that is what makes guessing slow), so it runs off the event loop.

use crate::interp::{fail, Ctrl, Interp};
use crate::value::*;
use sha2::{Digest, Sha256};

type R = Result<Value, Ctrl>;

pub fn install(m: &mut Map) {
    let fns: &[(&'static str, BuiltinFn)] = &[
        ("crypto.sha256", sha256),
        ("crypto.hmac", hmac),
        ("crypto.token", token),
        ("crypto.hash_password", hash_password),
        ("crypto.verify_password", verify_password),
        ("crypto.equal", equal),
        ("crypto.totp_secret", crate::totp::totp_secret),
        ("crypto.totp_uri", crate::totp::totp_uri),
        ("crypto.totp_verify", crate::totp::totp_verify),
        ("crypto.recovery_codes", crate::totp::recovery_codes),
        ("crypto.verify_recovery_code", crate::totp::verify_recovery_code),
    ];
    for (name, f) in fns {
        m.insert(name.trim_start_matches("crypto.").into(), Value::Builtin(name, *f));
    }
}

fn text(args: &[Value], i: usize, f: &str, line: u32) -> Result<std::sync::Arc<str>, Ctrl> {
    match args.get(i) {
        Some(Value::Str(s)) => Ok(s.clone()),
        other => fail(line, format!("`crypto.{}` expects a str as argument {}, got {}", f, i + 1, other.map_or("nothing", |v| v.type_name()))),
    }
}

pub fn hex(bytes: &[u8]) -> String {
    const D: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(D[(b >> 4) as usize] as char);
        s.push(D[(b & 15) as usize] as char);
    }
    s
}

/// Base64 with `-` and `_`, unpadded: safe in URLs and cookies.
pub fn b64url(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut s = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..=c.len() {
            s.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    s
}

pub fn unb64url(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'-' => Some(62),
        b'_' => Some(63),
        _ => None,
    };
    if s.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    for c in s.as_bytes().chunks(4) {
        let mut n = 0u32;
        for (i, &b) in c.iter().enumerate() {
            n |= (val(b)? as u32) << (18 - 6 * i);
        }
        for i in 0..c.len() - 1 {
            out.push((n >> (16 - 8 * i)) as u8);
        }
    }
    Some(out)
}

pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let pad = |x: u8| k.map(|b| b ^ x);
    let inner = Sha256::new().chain_update(pad(0x36)).chain_update(msg).finalize();
    Sha256::new().chain_update(pad(0x5c)).chain_update(inner).finalize().into()
}

/// Equal bytes, taking the same time wherever they differ.
pub fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

/// Random bytes for a secret of this process's (a signing key, a token).
/// Without them nothing it signs could be trusted, so it stops.
pub fn secret_bytes(n: usize) -> Vec<u8> {
    random_bytes(n).unwrap_or_else(|| {
        eprintln!("quillo: the operating system gave no random bytes; stopping");
        std::process::exit(1)
    })
}

pub fn random_bytes(n: usize) -> Option<Vec<u8>> {
    let mut b = vec![0u8; n];
    getrandom::getrandom(&mut b).ok()?;
    Some(b)
}

fn sha256(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let t = text(&args, 0, "sha256", line)?;
    Ok(Value::str(hex(&Sha256::digest(t.as_bytes()))))
}

fn hmac(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let key = text(&args, 0, "hmac", line)?;
    let msg = text(&args, 1, "hmac", line)?;
    Ok(Value::str(hex(&hmac_sha256(key.as_bytes(), msg.as_bytes()))))
}

/// `crypto.token(bytes = 32)`: random from the operating system, for session
/// ids, reset links and API keys.
fn token(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let n = match args.first() {
        None | Some(Value::Nil) => 32,
        Some(Value::Int(n)) if (1..=1024).contains(n) => *n as usize,
        Some(v) => return fail(line, format!("`crypto.token` takes a number of bytes from 1 to 1024, got {}", v.repr())),
    };
    match random_bytes(n) {
        Some(b) => Ok(Value::str(b64url(&b))),
        None => fail(line, "`crypto.token`: the operating system gave no random bytes"),
    }
}

/// Hashes run at most one per core at a time (each takes 19 MiB), so a
/// flood of logins cannot take the server's memory; past `MOST_WAITING`
/// waiting their turn, more are refused. In the server they wait without
/// holding a thread (queries and `http` calls need those), then run on
/// threads of their own.
const MOST_WAITING: usize = 1024;

fn cores() -> usize {
    std::thread::available_parallelism().map_or(4, |n| n.get())
}

pub(crate) fn hashing<T: Send + 'static>(it: &mut Interp, work: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
    static WAITING: AtomicUsize = AtomicUsize::new(0);
    if WAITING.fetch_add(1, SeqCst) >= MOST_WAITING {
        WAITING.fetch_sub(1, SeqCst);
        return Err("too many passwords are being checked at once; try again shortly".into());
    }
    struct Left;
    impl Drop for Left {
        fn drop(&mut self) {
            WAITING.fetch_sub(1, SeqCst);
        }
    }
    let _left = Left;
    // Outside the server, or inside a SQLite transaction (which no other
    // request may run in meanwhile): here, in turn.
    if !crate::httpd::in_task() || it.sqlite_tx > 0 {
        return Ok(in_turn(work));
    }
    static TURNS: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();
    let fut = Box::pin(async move {
        let _turn = TURNS.get_or_init(|| tokio::sync::Semaphore::new(cores())).acquire().await;
        tokio::task::spawn_blocking(work).await.map_err(|e| e.to_string())
    });
    match crate::httpd::await_elsewhere(fut) {
        Some(Ok(r)) => r,
        Some(Err(e)) => Err(e),
        None => Err("password hashing failed unexpectedly".into()),
    }
}

/// `f` once fewer than one hash per core is running.
fn in_turn<T>(f: impl FnOnce() -> T) -> T {
    use std::sync::{Condvar, Mutex};
    static RUNNING: Mutex<usize> = Mutex::new(0);
    static FREED: Condvar = Condvar::new();
    let mut n = RUNNING.lock().unwrap_or_else(|e| e.into_inner());
    while *n >= cores() {
        n = FREED.wait(n).unwrap_or_else(|e| e.into_inner());
    }
    *n += 1;
    drop(n);
    struct Done;
    impl Drop for Done {
        fn drop(&mut self) {
            *RUNNING.lock().unwrap_or_else(|e| e.into_inner()) -= 1;
            FREED.notify_one();
        }
    }
    let _done = Done;
    f()
}

/// An Argon2id hash of `pw`, salted.
pub(crate) fn argon_hash(pw: &[u8]) -> Result<String, String> {
    use argon2::password_hash::{PasswordHasher, SaltString};
    let salt = random_bytes(16).ok_or("the operating system gave no random bytes")?;
    let salt = SaltString::encode_b64(&salt).map_err(|e| e.to_string())?;
    argon2::Argon2::default().hash_password(pw, &salt).map(|h| h.to_string()).map_err(|e| e.to_string())
}

/// Whether `pw` is what `hash` was made from. Anything that is not such a
/// hash is checked against a stand-in, taking as long, and is false.
pub(crate) fn argon_check(pw: &[u8], hash: Option<&str>) -> bool {
    use argon2::password_hash::{PasswordHash, PasswordVerifier};
    let check = |h: &PasswordHash| argon2::Argon2::default().verify_password(pw, h).is_ok();
    match hash.map(PasswordHash::new) {
        Some(Ok(h)) if hash != Some(STAND_IN) => check(&h),
        _ => {
            let _ = check(&PasswordHash::new(STAND_IN).expect("the stand-in hash parses"));
            false
        }
    }
}

fn hash_password(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let pw = text(&args, 0, "hash_password", line)?;
    match hashing(it, move || argon_hash(pw.as_bytes())).and_then(|r| r) {
        Ok(h) => Ok(Value::str(h)),
        Err(e) => fail(line, format!("`crypto.hash_password`: {}", e)),
    }
}

/// A hash of nothing in particular, checked against when there is none.
const STAND_IN: &str = "$argon2id$v=19$m=19456,t=2,p=1$c3RhbmQtaW4tc2FsdA$DhLSbtFH5pnvXR6WmcnVCvnVK8wTrW0J5FPGMXBhEMk";

/// False for a wrong password, and for anything that is not a hash made by
/// `hash_password` (so a missing hash never lets anyone in). Taking as long
/// either way: an unknown user's login answers no sooner than a known one's,
/// which would tell who has an account.
fn verify_password(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let pw = text(&args, 0, "verify_password", line)?;
    let hash = match args.get(1) {
        Some(Value::Str(h)) => Some(h.clone()),
        _ => None,
    };
    match hashing(it, move || argon_check(pw.as_bytes(), hash.as_deref())) {
        Ok(ok) => Ok(Value::Bool(ok)),
        Err(e) => fail(line, format!("`crypto.verify_password`: {}", e)),
    }
}

fn equal(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let a = text(&args, 0, "equal", line)?;
    let b = text(&args, 1, "equal", line)?;
    Ok(Value::Bool(same(a.as_bytes(), b.as_bytes())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_answers() {
        // RFC 4231, test case 2.
        assert_eq!(
            hex(&hmac_sha256(b"Jefe", b"what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        // A key longer than a block is hashed first (RFC 4231, test case 6).
        assert_eq!(
            hex(&hmac_sha256(&[0xaa; 131], b"Test Using Larger Than Block-Size Key - Hash Key First")),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
        for s in ["", "f", "fo", "foo", "foob", "fooba", "foobar"] {
            assert_eq!(unb64url(&b64url(s.as_bytes())).unwrap(), s.as_bytes());
        }
        assert_eq!(b64url(b"\xfb\xff"), "-_8");
        assert_eq!(unb64url("a"), None);
        assert_eq!(unb64url("a=bc"), None);
        assert!(argon2::password_hash::PasswordHash::new(STAND_IN).is_ok());
    }
}
