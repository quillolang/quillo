//! `crypto.verify_webhook`: whether a webhook came from who it says.
//!
//! ```text
//! route POST /hooks/stripe {
//!     let ok = crypto.verify_webhook(request.body, request.headers["stripe-signature"], env("STRIPE_SECRET"))
//!     if !ok { reply 400 "bad signature" }
//!     ...
//! }
//! crypto.verify_webhook(request.body, request.headers["x-hub-signature-256"], secret)    // GitHub: sha256=<hex>
//! crypto.verify_webhook(request.body, request.headers["x-shopify-hmac-sha256"], secret)  // Shopify: base64
//! ```
//!
//! Two schemes, told apart by the header unless `scheme` says:
//!
//! - `stripe`: `t=<unix time>,v1=<hex>,...`, an HMAC-SHA256 of `<t>.<body>`.
//!   Any of the `v1` signatures may match (Stripe sends two while a secret
//!   is being rolled), and `t` must be within `tolerance` (5 minutes unless
//!   given) of now, so a webhook replayed later is refused.
//! - `hmac-sha256`: an HMAC-SHA256 of the body, in hex (`sha256=` before it
//!   or not, as GitHub sends it) or base64 (as Shopify does).
//!
//! Signatures are compared in constant time. The answer is `true`, or an
//! error value saying why not (falsy, so `if !...` works). The body must
//! be the request's as sent: `request.body` is that text, not JSON parsed
//! and written again.

use crate::interp::{fail, Ctrl, Interp};
use crate::value::*;

type R = Result<Value, Ctrl>;

fn no(why: impl Into<String>) -> Value {
    Value::err(Value::str(format!("crypto.verify_webhook: {}", why.into())))
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

fn unb64(s: &str) -> Option<Vec<u8>> {
    let s: String = s.trim().trim_end_matches('=').chars().map(|c| match c {
        '+' => '-',
        '/' => '_',
        c => c,
    }).collect();
    crate::crypto::unb64url(&s)
}

/// The Stripe scheme's answer for `header`, at time `now` (seconds).
pub fn stripe(body: &[u8], header: &str, secret: &[u8], tolerance: u64, now: u64) -> Result<(), String> {
    let mut t = None;
    let mut sigs = Vec::new();
    for part in header.split(',') {
        match part.trim().split_once('=') {
            Some(("t", v)) => t = v.parse::<u64>().ok(),
            Some(("v1", v)) => sigs.push(v.to_ascii_lowercase()),
            _ => {}
        }
    }
    let Some(t) = t else { return Err("the header has no timestamp (t=...)".into()) };
    if sigs.is_empty() {
        return Err("the header has no v1 signature".into());
    }
    let mut signed = Vec::with_capacity(body.len() + 12);
    signed.extend_from_slice(t.to_string().as_bytes());
    signed.push(b'.');
    signed.extend_from_slice(body);
    let mac = crate::crypto::hmac_sha256(secret, &signed);
    // Every signature is compared, whichever matches.
    let mut ok = false;
    for s in &sigs {
        ok |= unhex(s).map_or(false, |b| crate::crypto::same(&b, &mac));
    }
    if !ok {
        return Err("no signature matches".into());
    }
    if now.abs_diff(t) > tolerance {
        return Err(format!("its timestamp is {}s from now, more than the tolerance of {}s", now.abs_diff(t), tolerance));
    }
    Ok(())
}

/// The plain scheme: the body's HMAC-SHA256, in hex (`sha256=` before it or
/// not) or base64.
pub fn plain(body: &[u8], header: &str, secret: &[u8]) -> Result<(), String> {
    let h = header.trim();
    let h = h.strip_prefix("sha256=").unwrap_or(h);
    let given = if h.len() == 64 { unhex(&h.to_ascii_lowercase()) } else { None }.or_else(|| unb64(h));
    let mac = crate::crypto::hmac_sha256(secret, body);
    match given {
        Some(g) if crate::crypto::same(&g, &mac) => Ok(()),
        Some(_) => Err("the signature does not match".into()),
        None => Err("the header is not a signature in hex or base64".into()),
    }
}

/// `crypto.verify_webhook(body, header, secret, { tolerance: "5m", scheme })`.
pub fn verify_webhook(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let body = match args.first() {
        Some(Value::Str(s)) => s.clone(),
        other => return fail(line, format!("`crypto.verify_webhook` expects the request's body (request.body), got {}", other.map_or("nothing", |v| v.type_name()))),
    };
    let header = match args.get(1) {
        Some(Value::Str(s)) => s.clone(),
        // A request without the header: not signed.
        None | Some(Value::Nil) => return Ok(no("there is no signature header")),
        Some(v) => return fail(line, format!("`crypto.verify_webhook` expects the signature header's text, got {}", v.type_name())),
    };
    let secret = match args.get(2) {
        Some(Value::Str(s)) if !s.is_empty() => s.clone(),
        Some(Value::Nil) | None => return Ok(no("the secret is missing (is its environment variable set?)")),
        Some(v) => return fail(line, format!("`crypto.verify_webhook` expects the secret as a str, got {}", v.type_name())),
    };
    let opts = match args.get(3) {
        None | Some(Value::Nil) => Map::new(),
        Some(Value::Map(m)) => (**m).clone(),
        Some(v) => return fail(line, format!("`crypto.verify_webhook` expects options as a map, got {}", v.type_name())),
    };
    let tolerance = match opts.get("tolerance") {
        None | Some(Value::Nil) => 300,
        Some(v) => match crate::builtins::duration_seconds(v) {
            Some(n) => n,
            None => return fail(line, format!("`crypto.verify_webhook`'s tolerance is seconds or a duration like \"5m\", got {}", v.repr())),
        },
    };
    let stripe_like = header.contains("t=") && header.contains("v1=");
    let scheme = match opts.get("scheme") {
        None | Some(Value::Nil) => if stripe_like { "stripe" } else { "hmac-sha256" }.to_string(),
        Some(Value::Str(s)) if matches!(&**s, "stripe" | "hmac-sha256") => s.to_string(),
        Some(v) => return fail(line, format!("`crypto.verify_webhook`'s scheme is \"stripe\" or \"hmac-sha256\", got {}", v.repr())),
    };
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let r = if scheme == "stripe" {
        stripe(body.as_bytes(), &header, secret.as_bytes(), tolerance, now)
    } else {
        plain(body.as_bytes(), &header, secret.as_bytes())
    };
    Ok(match r {
        Ok(()) => Value::Bool(true),
        Err(e) => no(e),
    })
}
