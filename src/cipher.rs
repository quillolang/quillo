//! Encrypted fields: `crypto.encrypt`, `crypto.decrypt` and
//! `crypto.reencrypt`, AES-256-GCM with keys from the environment.
//!
//! ```text
//! QUILLO_ENCRYPTION_KEYS="k2026:<base64 key>,k2025:<base64 key>"   // the first is current
//! let token = crypto.encrypt(iban)      // "qe1.k2026.<nonce, text and tag, base64url>"
//! crypto.decrypt(token)                 // the text, or an error value
//! crypto.reencrypt(token)               // the same text under the current key
//! ```
//!
//! A token names the key it was made with, so keys can be rotated: put a
//! new key first, and older ones after it. Decrypting finds the token's
//! key by its id; `crypto.reencrypt` moves a value to the current key (a
//! job can go through a table with it), after which the old key can go.
//! Each token has a random 96-bit nonce, and its version and key id are
//! authenticated with it, so a token cannot be passed off as another key's.
//! A token that was changed, or whose key is not there, is an error value.
//!
//! The keys are read from the environment where they are used, in Rust:
//! no Quillo value holds them, so they cannot reach a page.
//! `quillo keygen` makes a key.

use crate::crypto::{b64url, unb64url};
use crate::interp::{fail, Ctrl, Interp};
use crate::value::*;
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM, NONCE_LEN};

type R = Result<Value, Ctrl>;

pub const ENV: &str = "QUILLO_ENCRYPTION_KEYS";
const VERSION: &str = "qe1";

pub fn install(m: &mut Map) {
    let fns: &[(&'static str, BuiltinFn)] = &[
        ("crypto.encrypt", encrypt),
        ("crypto.decrypt", decrypt),
        ("crypto.reencrypt", reencrypt),
        ("crypto.verify_webhook", crate::webhook::verify_webhook),
    ];
    for (name, f) in fns {
        m.insert(name.trim_start_matches("crypto.").into(), Value::Builtin(name, *f));
    }
}

/// Base64 in either alphabet, padded or not.
fn unb64(s: &str) -> Option<Vec<u8>> {
    let s: String = s.trim().trim_end_matches('=').chars().map(|c| match c {
        '+' => '-',
        '/' => '_',
        c => c,
    }).collect();
    unb64url(&s)
}

fn id_ok(id: &str) -> bool {
    !id.is_empty() && id.len() <= 32 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// A key ring, `id:base64key` pairs separated by commas or spaces, the
/// current key first.
pub fn keyring(text: &str) -> Result<Vec<(String, [u8; 32])>, String> {
    let mut out = Vec::new();
    for entry in text.split(|c: char| c == ',' || c.is_whitespace()).filter(|e| !e.is_empty()) {
        let Some((id, k)) = entry.split_once(':') else {
            return Err("a key is id:base64key (quillo keygen makes one)".into());
        };
        if !id_ok(id) {
            return Err(format!("a key's id is letters, digits, - and _ (up to 32), got {:?}", id));
        }
        let key: [u8; 32] = match unb64(k) {
            Some(b) => b.try_into().map_err(|b: Vec<u8>| format!("key {} is {} bytes; AES-256 needs 32", id, b.len()))?,
            None => return Err(format!("key {} is not base64", id)),
        };
        if out.iter().any(|(i, _)| i == id) {
            return Err(format!("key id {} is there twice", id));
        }
        out.push((id.to_string(), key));
    }
    if out.is_empty() {
        return Err("no keys".into());
    }
    Ok(out)
}

/// The key ring given as an argument, or else the environment's.
fn ring_from(args: &[Value], i: usize, f: &str, line: u32) -> Result<Result<Vec<(String, [u8; 32])>, String>, Ctrl> {
    match args.get(i) {
        None | Some(Value::Nil) => Ok(match std::env::var(ENV) {
            Ok(t) if !t.trim().is_empty() => keyring(&t).map_err(|e| format!("{}: {}", ENV, e)),
            _ => Err(format!("{} is not set: it holds the keys, id:base64key (quillo keygen makes one)", ENV)),
        }),
        Some(Value::Str(t)) => Ok(keyring(t)),
        Some(v) => fail(line, format!("`crypto.{}` takes keys as id:base64key text, got {}", f, v.type_name())),
    }
}

fn sealing_key(k: &[u8; 32]) -> LessSafeKey {
    LessSafeKey::new(UnboundKey::new(&AES_256_GCM, k).expect("a 32-byte key"))
}

/// `text` encrypted with key `id`: `qe1.<id>.<base64url(nonce, ciphertext, tag)>`.
pub fn seal(id: &str, key: &[u8; 32], text: &[u8]) -> Result<String, String> {
    let nonce: [u8; NONCE_LEN] = crate::crypto::random_bytes(NONCE_LEN)
        .ok_or("the operating system gave no random bytes")?
        .try_into()
        .map_err(|_| "no nonce")?;
    let mut buf = text.to_vec();
    let aad = format!("{}.{}", VERSION, id);
    sealing_key(key)
        .seal_in_place_append_tag(Nonce::assume_unique_for_key(nonce), Aad::from(aad.as_bytes()), &mut buf)
        .map_err(|_| "encryption failed")?;
    let mut all = nonce.to_vec();
    all.extend_from_slice(&buf);
    Ok(format!("{}.{}", aad, b64url(&all)))
}

/// The text in a token, with the key ring: (text, key id).
pub fn open(ring: &[(String, [u8; 32])], token: &str) -> Result<(Vec<u8>, String), String> {
    let mut parts = token.trim().splitn(3, '.');
    let (Some(v), Some(id), Some(body)) = (parts.next(), parts.next(), parts.next()) else {
        return Err("not an encrypted value".into());
    };
    if v != VERSION {
        return Err(format!("not an encrypted value of a version this Quillo reads ({})", if v.len() > 8 { "?" } else { v }));
    }
    let Some((_, key)) = ring.iter().find(|(i, _)| i == id) else {
        return Err(format!("no key {} (its id is not among the keys)", if id_ok(id) { id } else { "?" }));
    };
    let Some(mut all) = unb64url(body).filter(|b| b.len() >= NONCE_LEN + 16) else {
        return Err("the encrypted value is damaged".into());
    };
    let nonce: [u8; NONCE_LEN] = all[..NONCE_LEN].try_into().map_err(|_| "no nonce")?;
    let aad = format!("{}.{}", VERSION, id);
    let n = sealing_key(key)
        .open_in_place(Nonce::assume_unique_for_key(nonce), Aad::from(aad.as_bytes()), &mut all[NONCE_LEN..])
        .map_err(|_| "the encrypted value was changed, or the key is not the one it was made with")?
        .len();
    Ok((all[NONCE_LEN..NONCE_LEN + n].to_vec(), id.to_string()))
}

fn err(f: &str, e: impl std::fmt::Display) -> Value {
    Value::err(Value::str(format!("crypto.{}: {}", f, e)))
}

/// `crypto.encrypt(text, keys = $QUILLO_ENCRYPTION_KEYS)`: a token for the
/// text, made with the current (first) key.
fn encrypt(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let text = match args.first() {
        Some(Value::Str(s)) => s.clone(),
        other => return fail(line, format!("`crypto.encrypt` expects a str (json.stringify a map first), got {}", other.map_or("nothing", |v| v.type_name()))),
    };
    let ring = match ring_from(&args, 1, "encrypt", line)? {
        Ok(r) => r,
        Err(e) => return Ok(err("encrypt", e)),
    };
    let (id, key) = &ring[0];
    Ok(seal(id, key, text.as_bytes()).map(Value::str).unwrap_or_else(|e| err("encrypt", e)))
}

fn token_arg(args: &[Value], f: &str, line: u32) -> Result<std::sync::Arc<str>, Ctrl> {
    match args.first() {
        Some(Value::Str(s)) => Ok(s.clone()),
        other => fail(line, format!("`crypto.{}` expects what crypto.encrypt made, got {}", f, other.map_or("nothing", |v| v.type_name()))),
    }
}

/// `crypto.decrypt(token, keys = $QUILLO_ENCRYPTION_KEYS)`: the text, or an
/// error value (a changed token, an unknown key).
fn decrypt(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let token = token_arg(&args, "decrypt", line)?;
    let ring = match ring_from(&args, 1, "decrypt", line)? {
        Ok(r) => r,
        Err(e) => return Ok(err("decrypt", e)),
    };
    Ok(match open(&ring, &token) {
        Ok((b, _)) => match String::from_utf8(b) {
            Ok(s) => Value::str(s),
            Err(_) => err("decrypt", "the value is not text"),
        },
        Err(e) => err("decrypt", e),
    })
}

/// `crypto.reencrypt(token, keys = $QUILLO_ENCRYPTION_KEYS)`: the value
/// under the current key; a token made with it already is given back as it
/// is, so going through a table twice changes nothing more.
fn reencrypt(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let token = token_arg(&args, "reencrypt", line)?;
    let ring = match ring_from(&args, 1, "reencrypt", line)? {
        Ok(r) => r,
        Err(e) => return Ok(err("reencrypt", e)),
    };
    Ok(match open(&ring, &token) {
        Ok((_, id)) if id == ring[0].0 => Value::Str(token),
        Ok((b, _)) => seal(&ring[0].0, &ring[0].1, &b).map(Value::str).unwrap_or_else(|e| err("reencrypt", e)),
        Err(e) => err("reencrypt", e),
    })
}

/// `quillo keygen [id]`: a new key, as a `QUILLO_ENCRYPTION_KEYS` entry.
pub fn keygen(id: Option<&str>) -> Result<String, String> {
    let id = match id {
        Some(i) if id_ok(i) => i.to_string(),
        Some(i) => return Err(format!("a key's id is letters, digits, - and _ (up to 32), got {:?}", i)),
        None => {
            let days = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() / 86400);
            let (y, m, d) = crate::httpd::civil(days as i64);
            format!("k{}{:02}{:02}", y, m, d)
        }
    };
    let key = crate::crypto::random_bytes(32).ok_or("the operating system gave no random bytes")?;
    Ok(format!("{}:{}", id, b64url(&key)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sealed_and_opened() {
        let ring = keyring("new:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=, old:HxwdHhsYGRoXFBUWExAREg8MDQ4LCAkKBwQFBgMAAQI").unwrap();
        let t = seal("new", &ring[0].1, b"DE89 3704").unwrap();
        assert!(t.starts_with("qe1.new."), "{}", t);
        assert_eq!(open(&ring, &t).unwrap(), (b"DE89 3704".to_vec(), "new".to_string()));
        // Another nonce each time.
        assert_ne!(t, seal("new", &ring[0].1, b"DE89 3704").unwrap());
        // Passed off as the other key's: refused.
        let swapped = t.replacen("qe1.new.", "qe1.old.", 1);
        assert!(open(&ring, &swapped).is_err());
        // A changed byte: refused.
        let mut b = t.into_bytes();
        let last = b.len() - 2;
        b[last] = if b[last] == b'A' { b'B' } else { b'A' };
        assert!(open(&ring, &String::from_utf8(b).unwrap()).is_err());
        assert!(keyring("x:AAEC").is_err());
        assert!(keyring("bad id:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=").is_err());
        assert!(keygen(None).unwrap().starts_with('k'));
    }
}
