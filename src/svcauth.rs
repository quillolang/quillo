//! Authentication between services (FEEDBACK38.md, Q-458 to Q-464; branch feedback36-39).
//!
//! What a call proves, and to whom. The caller says how it proves who it is, where it declares
//! the service (`service billing = "billing.qwe" { auth: ... }`):
//! - `"quillo"`, the default: signed with the shared `QUILLO_SERVICE_SECRET`, which any service
//!   holding it can sign as any other;
//! - `{ key, as }`: signed with a key of its own, which the callee knows by the caller's name
//!   (`kid`), so the callee can tell `shop` from `notify`;
//! - `{ bearer: () => token }`, `{ oauth: { token_url, client_id, client_secret, scope,
//!   audience } }`: a bearer token, made by the program or fetched by OAuth 2.0's client
//!   credentials grant (RFC 6749 §4.4) and kept until shortly before it expires;
//! - `{ mtls: { cert, key, ca } }`: a client certificate on the connection;
//!
//! and `forward: ["authorization"]` passes the visitor's own headers along.
//!
//! The callee says what it takes with `accept_calls { keys, shared, jwt, client_cert, check }`,
//! and who called shows in `request.caller`: `{ name, by }`, and for a token its `scopes` and
//! `claims`. `server fn f(...) for ["shop"]` takes calls from those callers only.
//!
//! Signatures of version 3 cover the caller's name, the deadline and the contract as well as
//! what version 2 covered, and an answer is signed back with the same key. Several secrets may
//! be given at once (`QUILLO_SERVICE_SECRET=new,old`, or `QUILLO_SERVICE_SECRET_PREVIOUS`): calls
//! are signed with the first and taken with any, so a secret can be rotated machine by machine.

use crate::value::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// The shared secrets: the first signs, any is taken (Q-461).
pub fn secrets() -> Vec<String> {
    let mut out: Vec<String> = std::env::var("QUILLO_SERVICE_SECRET").unwrap_or_default().split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    if let Ok(prev) = std::env::var("QUILLO_SERVICE_SECRET_PREVIOUS") {
        out.extend(prev.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
    }
    out
}

/// This service's own key, which it joins an app with and signs answers with when the caller
/// signed with it (`QUILLO_SERVICE_KEY`).
pub fn own_key() -> Option<String> {
    std::env::var("QUILLO_SERVICE_KEY").ok().map(|k| k.split(',').next().unwrap_or("").trim().to_string()).filter(|k| !k.is_empty())
}

/// What version 3 signs: what version 2 did, and the caller, the deadline and the contract,
/// which a call on a plain network could otherwise have changed on its way (Q-458).
#[allow(clippy::too_many_arguments)]
pub fn sign3(key: &str, t: u64, to: &str, f: &str, tenant: &str, call: &str, kid: &str, caller: &str, deadline: &str, contract: &str, body: &[u8]) -> String {
    let mut msg = format!("quillo-service-3\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n", t, to, f, tenant, call, kid, caller, deadline, contract).into_bytes();
    msg.extend_from_slice(body);
    crate::crypto::hex(&crate::crypto::hmac_sha256(key.as_bytes(), &msg))
}

/// The visitor's headers a call passes on, as they are signed with it: one `name:value` a line,
/// by name, so a call on its way cannot have its user changed (FEEDBACK38.md, Q-460).
pub fn forwarded_text<'a>(headers: impl Iterator<Item = (&'a str, &'a str)>) -> String {
    let mut lines: Vec<String> = headers
        .filter_map(|(k, v)| k.to_ascii_lowercase().strip_prefix("x-quillo-forward-").map(|n| format!("{}:{}\n", n, v)))
        .collect();
    lines.sort();
    lines.concat()
}

/// An answer's signature: by the callee, with the key the call was signed with, so that a copy
/// that only says it is the service cannot answer for it (Q-459).
pub fn sign_answer(key: &str, call: &str, status: u16, body: &[u8]) -> String {
    let mut msg = format!("quillo-answer-3\n{}\n{}\n", call, status).into_bytes();
    msg.extend_from_slice(body);
    crate::crypto::hex(&crate::crypto::hmac_sha256(key.as_bytes(), &msg))
}

/// The fields of `x-quillo-service`: `v=3,kid=shop,t=...,sig=...` (`v` and `kid` absent in
/// version 2).
pub struct Signed<'a> {
    pub v: u32,
    pub kid: &'a str,
    pub t: u64,
    pub sig: &'a str,
}

pub fn signed(header: &str) -> Signed<'_> {
    let field = |name: &str| header.split(',').find_map(|p| p.trim().strip_prefix(name));
    Signed {
        v: field("v=").and_then(|v| v.parse().ok()).unwrap_or(2),
        kid: field("kid=").unwrap_or(""),
        t: field("t=").and_then(|v| v.parse().ok()).unwrap_or(0),
        sig: field("sig=").unwrap_or(""),
    }
}

// ---- the caller's side ------------------------------------------------------------------------

/// How a program calls one service (its declaration's `auth` and `forward`).
#[derive(Clone, Default)]
pub struct Auth {
    /// `{ key, as }`: the key, and the name it is known by at the callee.
    pub key: Option<(String, String)>,
    /// `{ bearer: f }`: a function giving a token for each call.
    pub bearer: Option<Value>,
    pub oauth: Option<OAuth>,
    /// `{ mtls: { cert, key, ca } }`.
    pub mtls: Option<(Option<String>, Option<String>, Option<String>)>,
    /// The visitor's headers that go along.
    pub forward: Vec<String>,
}

#[derive(Clone, Default, PartialEq)]
pub struct OAuth {
    pub token_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub scope: String,
    pub audience: String,
    /// How the client proves itself to the token endpoint: `post` (in the form) or `basic`.
    pub basic: bool,
}

fn auths() -> &'static Mutex<HashMap<String, Auth>> {
    static A: OnceLock<Mutex<HashMap<String, Auth>>> = OnceLock::new();
    A.get_or_init(Default::default)
}

/// How service `name` is called.
pub fn auth_of(name: &str) -> Auth {
    auths().lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(name).cloned().unwrap_or_default()
}

/// Reads `auth` and `forward` of a service's options; an error says what is wrong.
pub fn set_auth(name: &str, auth: Option<&Value>, forward: Option<&Value>, base: &std::path::Path) -> Result<(), String> {
    let mut a = Auth::default();
    let text = |m: &Map, k: &str| -> Result<Option<String>, String> {
        match m.get(k) {
            None | Some(Value::Nil) => Ok(None),
            Some(Value::Str(s)) if !s.is_empty() => Ok(Some(s.to_string())),
            Some(Value::Str(_)) => Err(format!("service {}: `{}` is empty (is its environment variable set?)", name, k)),
            Some(other) => Err(format!("service {}: `{}` is text, not {}", name, k, crate::interp::short(other))),
        }
    };
    let path = |p: Option<String>| p.map(|p| if p == "system" { p } else { base.join(p).to_string_lossy().into_owned() });
    match auth {
        None | Some(Value::Nil) => {}
        Some(Value::Str(s)) if &**s == "quillo" => {}
        Some(Value::Map(m)) => {
            let given: Vec<&str> = ["key", "bearer", "oauth", "mtls"].into_iter().filter(|k| m.get(*k).is_some_and(|v| !matches!(v, Value::Nil))).collect();
            if let Some(k) = m.keys().find(|k| !["key", "as", "bearer", "oauth", "mtls"].contains(&&***k)) {
                return Err(format!("service {}: `auth` has no `{}` (key, as, bearer, oauth, mtls)", name, k));
            }
            if given.len() > 1 && !(given.len() == 2 && given.contains(&"mtls")) {
                return Err(format!("service {}: `auth` takes one way to prove who calls, and mtls beside it: got {}", name, given.join(" and ")));
            }
            if let Some(key) = text(m, "key")? {
                let me = text(m, "as")?.or_else(|| crate::service::served_as().map(str::to_string)).unwrap_or_else(|| "app".into());
                a.key = Some((me, key));
            } else if m.get("key").is_some_and(|v| !matches!(v, Value::Nil)) {
                return Err(format!("service {}: `auth: {{ key }}` is empty (is its environment variable set?)", name));
            }
            match m.get("bearer") {
                None | Some(Value::Nil) => {}
                Some(f @ (Value::Func(_) | Value::Builtin(..))) => a.bearer = Some(f.clone()),
                Some(Value::Str(t)) => a.bearer = Some(Value::Str(t.clone())),
                Some(other) => return Err(format!("service {}: `bearer` is a function giving a token, not {}", name, crate::interp::short(other))),
            }
            match m.get("oauth") {
                None | Some(Value::Nil) => {}
                Some(Value::Map(o)) => {
                    if let Some(k) = o.keys().find(|k| !["token_url", "client_id", "client_secret", "scope", "audience", "method"].contains(&&***k)) {
                        return Err(format!("service {}: `oauth` has no `{}` (token_url, client_id, client_secret, scope, audience, method)", name, k));
                    }
                    let need = |k: &str| text(o, k)?.ok_or_else(|| format!("service {}: `oauth` needs `{}`", name, k));
                    let method = text(o, "method")?.unwrap_or_else(|| "post".into());
                    if method != "post" && method != "basic" {
                        return Err(format!("service {}: `oauth`'s method is \"post\" or \"basic\", not {:?}", name, method));
                    }
                    a.oauth = Some(OAuth {
                        token_url: need("token_url")?,
                        client_id: need("client_id")?,
                        client_secret: need("client_secret")?,
                        scope: text(o, "scope")?.unwrap_or_default(),
                        audience: text(o, "audience")?.unwrap_or_default(),
                        basic: method == "basic",
                    });
                }
                Some(other) => return Err(format!("service {}: `oauth` is a map, not {}", name, crate::interp::short(other))),
            }
            match m.get("mtls") {
                None | Some(Value::Nil) => {}
                Some(Value::Map(t)) => {
                    if let Some(k) = t.keys().find(|k| !["cert", "key", "ca"].contains(&&***k)) {
                        return Err(format!("service {}: `mtls` has no `{}` (cert, key, ca)", name, k));
                    }
                    let (cert, key, ca) = (path(text(t, "cert")?), path(text(t, "key")?), path(text(t, "ca")?));
                    if cert.is_none() || key.is_none() {
                        return Err(format!("service {}: `mtls` needs `cert` and `key`, the certificate this program presents and its key", name));
                    }
                    a.mtls = Some((ca, cert, key));
                }
                Some(other) => return Err(format!("service {}: `mtls` is a map, not {}", name, crate::interp::short(other))),
            }
        }
        Some(other) => return Err(format!("service {}: `auth` is \"quillo\" or a map ({{ key }}, {{ bearer }}, {{ oauth }}, {{ mtls }}), not {}", name, crate::interp::short(other))),
    }
    match forward {
        None | Some(Value::Nil) => {}
        Some(Value::List(l)) => {
            for v in l.iter() {
                match v {
                    Value::Str(h) if !h.is_empty() && h.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') => a.forward.push(h.to_ascii_lowercase()),
                    other => return Err(format!("service {}: `forward` is a list of header names, not {}", name, crate::interp::short(other))),
                }
            }
        }
        Some(other) => return Err(format!("service {}: `forward` is a list of header names, like [\"authorization\"], not {}", name, crate::interp::short(other))),
    }
    auths().lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(name.to_string(), a);
    Ok(())
}

/// Tokens fetched by OAuth, by their client: kept until 30 s before they expire.
fn tokens() -> &'static Mutex<HashMap<String, (String, Instant)>> {
    static T: OnceLock<Mutex<HashMap<String, (String, Instant)>>> = OnceLock::new();
    T.get_or_init(Default::default)
}

fn token_key(o: &OAuth) -> String {
    format!("{}\n{}\n{}\n{}", o.token_url, o.client_id, o.scope, o.audience)
}

/// An access token for `o`, kept or fetched now (`fresh`: not the kept one, which was refused).
pub fn oauth_token(o: &OAuth, fresh: bool) -> Result<String, String> {
    let key = token_key(o);
    if !fresh {
        if let Some((t, until)) = tokens().lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(&key) {
            if *until > Instant::now() {
                return Ok(t.clone());
            }
        }
    }
    let mut form = String::from("grant_type=client_credentials");
    if !o.basic {
        form.push_str(&format!("&client_id={}&client_secret={}", enc(&o.client_id), enc(&o.client_secret)));
    }
    if !o.scope.is_empty() {
        form.push_str(&format!("&scope={}", enc(&o.scope)));
    }
    if !o.audience.is_empty() {
        form.push_str(&format!("&audience={}", enc(&o.audience)));
    }
    let mut rq = agent().post(&o.token_url).timeout(Duration::from_secs(10)).set("content-type", "application/x-www-form-urlencoded").set("accept", "application/json");
    if o.basic {
        let both = format!("{}:{}", enc(&o.client_id), enc(&o.client_secret));
        rq = rq.set("authorization", &format!("Basic {}", b64(both.as_bytes())));
    }
    let text = match rq.send_string(&form) {
        Ok(r) => r.into_string().map_err(|e| e.to_string())?,
        Err(ureq::Error::Status(code, r)) => return Err(format!("the token endpoint {} answered {}: {}", o.token_url, code, r.into_string().unwrap_or_default().trim())),
        Err(e) => return Err(format!("the token endpoint {}: {}", o.token_url, e)),
    };
    let j: serde_json::Value = serde_json::from_str(&text).map_err(|_| format!("the token endpoint {} gave no JSON", o.token_url))?;
    let token = j["access_token"].as_str().ok_or_else(|| format!("the token endpoint {} gave no access_token", o.token_url))?.to_string();
    let secs = j["expires_in"].as_u64().unwrap_or(300);
    let until = Instant::now() + Duration::from_secs(secs.saturating_sub(30).max(1));
    tokens().lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(key, (token.clone(), until));
    Ok(token)
}

/// The agent for calls to services, plain or with a client certificate.
pub fn agent_for(mtls: Option<&(Option<String>, Option<String>, Option<String>)>) -> Result<ureq::Agent, String> {
    static AGENTS: OnceLock<Mutex<HashMap<(Option<String>, Option<String>, Option<String>), ureq::Agent>>> = OnceLock::new();
    let key = mtls.cloned().unwrap_or_default();
    let mut agents = AGENTS.get_or_init(Default::default).lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(a) = agents.get(&key) {
        return Ok(a.clone());
    }
    let b = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(5)).timeout(Duration::from_secs(60)).redirects(0).user_agent(concat!("quillo/", env!("CARGO_PKG_VERSION")));
    let b = match mtls {
        Some((ca, cert, k)) => b.tls_config(crate::fetch::client_tls(ca.as_deref(), cert.as_deref(), k.as_deref())?),
        None => b,
    };
    let a = b.build();
    agents.insert(key, a.clone());
    Ok(a)
}

fn agent() -> ureq::Agent {
    agent_for(None).expect("a plain agent")
}

// ---- the callee's side ------------------------------------------------------------------------

/// What a program takes calls with (`accept_calls { ... }`).
#[derive(Clone, Default)]
pub struct Accept {
    /// Keys by caller: the first signs answers; any is taken.
    pub keys: HashMap<String, Vec<String>>,
    /// Calls signed with the shared secret are taken (unless `shared: false`).
    pub shared: bool,
    pub jwt: Option<JwtRules>,
    /// `client_cert: { header }`: the identity a proxy that ended mTLS gives in this header,
    /// believed only from an address `--trust-proxy` names.
    pub client_cert: Option<String>,
    /// `check: (caller) => ...`: anything else, in Quillo.
    pub check: Option<Value>,
}

#[derive(Clone, Default)]
pub struct JwtRules {
    pub issuer: Option<String>,
    pub audience: Option<String>,
    pub jwks: Option<String>,
    /// HS256's secret, for tokens a program signs itself.
    pub secret: Option<String>,
}

/// `accept_calls { ... }` read; an error says what is wrong.
pub fn accept_of(v: &Value) -> Result<Accept, String> {
    let Value::Map(m) = v else { return Err(format!("`accept_calls` takes a map, like accept_calls {{ keys: {{ shop: env(\"SHOP_KEY\") }} }}, not {}", crate::interp::short(v))) };
    if let Some(k) = m.keys().find(|k| !["keys", "shared", "jwt", "client_cert", "check"].contains(&&***k)) {
        return Err(format!("`accept_calls` has no `{}` (keys, shared, jwt, client_cert, check)", k));
    }
    let mut a = Accept { shared: true, ..Default::default() };
    match m.get("keys") {
        None | Some(Value::Nil) => {}
        Some(Value::Map(keys)) => {
            for (who, k) in keys.iter() {
                let list: Vec<String> = match k {
                    Value::Str(s) => s.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
                    Value::List(l) => l.iter().filter_map(|v| if let Value::Str(s) = v { Some(s.to_string()) } else { None }).filter(|s| !s.is_empty()).collect(),
                    Value::Nil => Vec::new(),
                    other => return Err(format!("`accept_calls`: the key of `{}` is text or a list of text, not {}", who, crate::interp::short(other))),
                };
                if list.is_empty() {
                    return Err(format!("`accept_calls`: the key of `{}` is empty (is its environment variable set?)", who));
                }
                a.keys.insert(who.to_string(), list);
            }
        }
        Some(other) => return Err(format!("`accept_calls`: `keys` is a map of callers' names to their keys, not {}", crate::interp::short(other))),
    }
    match m.get("shared") {
        None | Some(Value::Nil) => {}
        Some(Value::Bool(b)) => a.shared = *b,
        Some(other) => return Err(format!("`accept_calls`: `shared` is true or false, not {}", crate::interp::short(other))),
    }
    match m.get("jwt") {
        None | Some(Value::Nil) => {}
        Some(Value::Map(j)) => {
            if let Some(k) = j.keys().find(|k| !["issuer", "audience", "jwks", "secret"].contains(&&***k)) {
                return Err(format!("`accept_calls`: `jwt` has no `{}` (issuer, audience, jwks, secret)", k));
            }
            let text = |k: &str| match j.get(k) {
                Some(Value::Str(s)) if !s.is_empty() => Some(s.to_string()),
                _ => None,
            };
            let rules = JwtRules { issuer: text("issuer"), audience: text("audience"), jwks: text("jwks"), secret: text("secret") };
            if rules.jwks.is_none() && rules.secret.is_none() {
                return Err("`accept_calls`: `jwt` needs `jwks` (the identity provider's keys, a URL) or `secret` (HS256)".into());
            }
            if rules.audience.is_none() {
                return Err("`accept_calls`: `jwt` needs `audience`: a token for another service must not be taken here".into());
            }
            a.jwt = Some(rules);
        }
        Some(other) => return Err(format!("`accept_calls`: `jwt` is a map, not {}", crate::interp::short(other))),
    }
    match m.get("client_cert") {
        None | Some(Value::Nil) => {}
        Some(Value::Map(c)) => match c.get("header") {
            Some(Value::Str(h)) if !h.is_empty() => a.client_cert = Some(h.to_ascii_lowercase()),
            _ => return Err("`accept_calls`: `client_cert` needs `header`, the one the proxy that ended mTLS sets (x-forwarded-client-cert)".into()),
        },
        Some(other) => return Err(format!("`accept_calls`: `client_cert` is a map, like {{ header: \"x-forwarded-client-cert\" }}, not {}", crate::interp::short(other))),
    }
    match m.get("check") {
        None | Some(Value::Nil) => {}
        Some(f @ (Value::Func(_) | Value::Builtin(..))) => a.check = Some(f.clone()),
        Some(other) => return Err(format!("`accept_calls`: `check` is a function of the caller, not {}", crate::interp::short(other))),
    }
    if a.keys.is_empty() && a.jwt.is_none() && a.client_cert.is_none() && a.check.is_none() && a.shared {
        return Err("`accept_calls` with nothing in it takes what it took before: give keys, jwt, client_cert, check or shared: false".into());
    }
    Ok(a)
}

/// Who called, as `request.caller` gives it.
pub fn caller_value(name: &str, by: &str, extra: Vec<(&str, Value)>) -> Value {
    let mut m = Map::new();
    m.insert("name".into(), Value::str(name));
    m.insert("by".into(), Value::str(by));
    for (k, v) in extra {
        m.insert(k.into(), v);
    }
    Value::map(m)
}

/// The identity in a proxy's client-certificate header: Envoy's `x-forwarded-client-cert`
/// (`By=...;Hash=...;Subject="CN=shop,O=x";URI=spiffe://...`), or a subject alone. The name is
/// the SPIFFE id's last part, or the CN.
pub fn cert_identity(header: &str) -> Option<(String, Vec<(&'static str, Value)>)> {
    // Split where no quote is open: a subject has commas of its own.
    fn split_outside(s: &str, sep: char) -> Vec<&str> {
        let (mut out, mut from, mut quoted) = (Vec::new(), 0, false);
        for (i, c) in s.char_indices() {
            match c {
                '"' => quoted = !quoted,
                c if c == sep && !quoted => {
                    out.push(&s[from..i]);
                    from = i + 1;
                }
                _ => {}
            }
        }
        out.push(&s[from..]);
        out
    }
    // The last element is the client's (each proxy adds one, comma between).
    let elements = split_outside(header, ',');
    let last = elements.iter().rev().find(|e| e.contains('=')).copied().unwrap_or(header).trim();
    let field = |k: &str| {
        split_outside(last, ';').into_iter().find_map(|p| {
            let (a, b) = p.split_once('=')?;
            a.trim().eq_ignore_ascii_case(k).then(|| b.trim().trim_matches('"').to_string())
        })
    };
    let subject = field("Subject").unwrap_or_else(|| if last.contains(';') { String::new() } else { last.to_string() });
    let uri = field("URI");
    let cn = subject.split([',', '/']).find_map(|p| p.trim().strip_prefix("CN=").map(str::to_string));
    let name = uri.as_deref().and_then(|u| u.rsplit('/').next()).map(str::to_string).filter(|n| !n.is_empty()).or(cn)?;
    let mut extra: Vec<(&'static str, Value)> = vec![("subject", Value::str(subject.as_str()))];
    if let Some(u) = uri {
        extra.push(("uri", Value::str(u.as_str())));
    }
    Some((name, extra))
}

// ---- JWT access tokens (RFC 9068), checked against a JWKS (RFC 7517) or a secret ---------------

fn b64url_decode(s: &str) -> Option<Vec<u8>> {
    crate::crypto::unb64url(s.trim_end_matches('='))
}

/// Text for a form or a URL: every byte but letters, digits and `-_.~` written `%XX`.
fn enc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

/// Base64 as `Authorization: Basic` has it: `+` and `/`, padded.
fn b64(bytes: &[u8]) -> String {
    let mut s: String = crate::crypto::b64url(bytes).chars().map(|c| match c { '-' => '+', '_' => '/', c => c }).collect();
    while s.len() % 4 != 0 {
        s.push('=');
    }
    s
}

/// The claims of a token that verifies under `rules`: its signature, `exp` (required), `nbf`,
/// `iss` and `aud` (Q-463's checks).
pub fn verify_jwt(token: &str, rules: &JwtRules) -> Result<serde_json::Value, String> {
    let mut parts = token.split('.');
    let (Some(h), Some(p), Some(s), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return Err("not a JWT (three parts, joined by dots)".into());
    };
    let header: serde_json::Value = b64url_decode(h).and_then(|b| serde_json::from_slice(&b).ok()).ok_or("the token's header is not JSON")?;
    let claims: serde_json::Value = b64url_decode(p).and_then(|b| serde_json::from_slice(&b).ok()).ok_or("the token's claims are not JSON")?;
    let sig = b64url_decode(s).ok_or("the token's signature is not base64url")?;
    let signed = format!("{}.{}", h, p);
    let alg = header["alg"].as_str().unwrap_or("");
    match alg {
        "HS256" => {
            let secret = rules.secret.as_deref().ok_or("an HS256 token, and no secret to check it with")?;
            let want = crate::crypto::hmac_sha256(secret.as_bytes(), signed.as_bytes());
            if !crate::crypto::same(&want, &sig) {
                return Err("the token's signature is wrong".into());
            }
        }
        "RS256" | "ES256" | "EdDSA" => {
            let url = rules.jwks.as_deref().ok_or_else(|| format!("an {} token, and no jwks to check it with", alg))?;
            let kid = header["kid"].as_str().unwrap_or("");
            let key = jwk(url, kid, alg)?;
            let ok = match alg {
                "RS256" => {
                    let n = key["n"].as_str().and_then(b64url_decode).ok_or("the key has no n")?;
                    let e = key["e"].as_str().and_then(b64url_decode).ok_or("the key has no e")?;
                    ring::signature::RsaPublicKeyComponents { n, e }.verify(&ring::signature::RSA_PKCS1_2048_8192_SHA256, signed.as_bytes(), &sig).is_ok()
                }
                "ES256" => {
                    let x = key["x"].as_str().and_then(b64url_decode).ok_or("the key has no x")?;
                    let y = key["y"].as_str().and_then(b64url_decode).ok_or("the key has no y")?;
                    let mut point = vec![4u8];
                    point.extend(x);
                    point.extend(y);
                    ring::signature::UnparsedPublicKey::new(&ring::signature::ECDSA_P256_SHA256_FIXED, point).verify(signed.as_bytes(), &sig).is_ok()
                }
                _ => {
                    let x = key["x"].as_str().and_then(b64url_decode).ok_or("the key has no x")?;
                    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, x).verify(signed.as_bytes(), &sig).is_ok()
                }
            };
            if !ok {
                return Err("the token's signature is wrong".into());
            }
        }
        "" | "none" => return Err("an unsigned token".into()),
        other => return Err(format!("a token signed with {}, which is not taken (HS256, RS256, ES256, EdDSA)", other)),
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    const LEEWAY: i64 = 60;
    match claims["exp"].as_i64() {
        None => return Err("the token has no exp: it would be good for ever".into()),
        Some(exp) if exp + LEEWAY < now => return Err("the token has expired".into()),
        _ => {}
    }
    if claims["nbf"].as_i64().is_some_and(|nbf| nbf - LEEWAY > now) {
        return Err("the token is not good yet (nbf)".into());
    }
    if let Some(iss) = &rules.issuer {
        if claims["iss"].as_str() != Some(iss.as_str()) {
            return Err(format!("the token is from another issuer than {}", iss));
        }
    }
    if let Some(aud) = &rules.audience {
        let ok = match &claims["aud"] {
            serde_json::Value::String(a) => a == aud,
            serde_json::Value::Array(l) => l.iter().any(|a| a.as_str() == Some(aud.as_str())),
            _ => false,
        };
        if !ok {
            return Err(format!("the token is not for {} (aud)", aud));
        }
    }
    Ok(claims)
}

/// The key `kid` of the JWKS at `url`, fetched and kept for ten minutes, and fetched again (at
/// most once a minute) for a key it does not have, as a provider's keys rotate.
///
/// A fetch that failed is not tried again for a minute, and the keys it had are kept and used
/// meanwhile; one fetch at a time, for three seconds at most; and the URL and what went wrong
/// are told in the log, not to the caller, whose token may be anyone's. Each junk token made a
/// fetch, a provider that did not answer held each call ten seconds, and one that was down
/// refused every caller once the keys were ten minutes old (FEEDBACK49.md, Q-597; branch main).
fn jwk(url: &str, kid: &str, alg: &str) -> Result<serde_json::Value, String> {
    let find = |keys: &[serde_json::Value]| {
        let kty = match alg {
            "RS256" => "RSA",
            "ES256" => "EC",
            _ => "OKP",
        };
        keys.iter().find(|k| k["kty"] == kty && (kid.is_empty() || k["kid"] == kid) && k["use"].as_str().map_or(true, |u| u == "sig")).cloned()
    };
    let missing = || format!("no key `{}` in the identity provider's keys", kid);
    let cache = jwks_cache();
    // What is kept answers, unless it is old, or has no such key and was fetched over a
    // minute ago.
    let kept = |c: &HashMap<String, KeptKeys>| -> Option<Result<serde_json::Value, String>> {
        let k = c.get(url)?;
        let fresh = k.at.is_some_and(|at| at.elapsed() < JWKS_FRESH);
        let tried = k.tried.elapsed() < JWKS_RETRY;
        match find(&k.keys) {
            Some(key) if fresh || tried => Some(Ok(key)),
            None if tried => Some(Err(if k.at.is_some() { missing() } else { "the identity provider's keys could not be fetched".to_string() })),
            _ => None,
        }
    };
    if let Some(r) = kept(&cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner)) {
        return r;
    }
    // One fetch at a time: the others wait for it, and take what it got.
    let one = {
        static FETCHING: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
        FETCHING.get_or_init(Default::default).lock().unwrap_or_else(std::sync::PoisonError::into_inner).entry(url.to_string()).or_default().clone()
    };
    let _turn = one.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(r) = kept(&cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner)) {
        return r;
    }
    let fetched = agent()
        .get(url)
        .timeout(Duration::from_secs(3))
        .call()
        .map_err(|e| e.to_string())
        .and_then(|r| r.into_string().map_err(|e| e.to_string()))
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).map_err(|_| "not JSON".to_string()))
        .and_then(|j| j["keys"].as_array().cloned().ok_or_else(|| "no `keys`".to_string()));
    let mut c = cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let entry = c.entry(url.to_string()).or_insert_with(|| KeptKeys { keys: Vec::new(), at: None, tried: Instant::now() });
    entry.tried = Instant::now();
    match fetched {
        Ok(keys) => {
            entry.keys = keys;
            entry.at = Some(Instant::now());
            find(&entry.keys).ok_or_else(missing)
        }
        Err(e) => {
            let kept = !entry.keys.is_empty();
            eprintln!("quillo: warning: the jwks at {} could not be fetched ({}){}", url, e, if kept { "; the keys fetched before are used until it can be" } else { "" });
            find(&entry.keys).ok_or_else(|| if kept { missing() } else { "the identity provider's keys could not be fetched".to_string() })
        }
    }
}

/// How long a JWKS is taken as it is, and how soon one is fetched again after a try.
const JWKS_FRESH: Duration = Duration::from_secs(600);
const JWKS_RETRY: Duration = Duration::from_secs(60);

struct KeptKeys {
    keys: Vec<serde_json::Value>,
    /// When they were fetched; none if no fetch has worked yet.
    at: Option<Instant>,
    /// When a fetch was last tried, whether it worked or not.
    tried: Instant,
}

fn jwks_cache() -> &'static Mutex<HashMap<String, KeptKeys>> {
    static JWKS: OnceLock<Mutex<HashMap<String, KeptKeys>>> = OnceLock::new();
    JWKS.get_or_init(Default::default)
}

/// For tests: the JWKS at `url` as if fetched, and tried, `by` earlier.
#[doc(hidden)]
pub fn age_jwks(url: &str, by: Duration) {
    if let Some(k) = jwks_cache().lock().unwrap_or_else(std::sync::PoisonError::into_inner).get_mut(url) {
        k.at = k.at.and_then(|at| at.checked_sub(by));
        k.tried = k.tried.checked_sub(by).unwrap_or(k.tried);
    }
}

/// `request.caller` of a token's claims.
pub fn jwt_caller(claims: &serde_json::Value) -> Value {
    let name = ["client_id", "azp", "sub"].iter().find_map(|k| claims[*k].as_str()).unwrap_or("").to_string();
    let scopes: Vec<Value> = match &claims["scope"] {
        serde_json::Value::String(s) => s.split_whitespace().map(Value::str).collect(),
        serde_json::Value::Array(l) => l.iter().filter_map(|v| v.as_str().map(Value::str)).collect(),
        _ => match &claims["scp"] {
            serde_json::Value::Array(l) => l.iter().filter_map(|v| v.as_str().map(Value::str)).collect(),
            _ => Vec::new(),
        },
    };
    caller_value(&name, "jwt", vec![("scopes", Value::list(scopes)), ("claims", Value::from_json(claims))])
}

pub type Shared = Arc<Accept>;
