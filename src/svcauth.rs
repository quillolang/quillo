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
            Some(Value::Str(s) | Value::Css(s)) if !s.is_empty() => Ok(Some(s.to_string())),
            Some(Value::Str(_) | Value::Css(_)) => Err(format!("service {}: `{}` is empty (is its environment variable set?)", name, k)),
            Some(other) => Err(format!("service {}: `{}` is text, not {}", name, k, crate::interp::short(other))),
        }
    };
    let path = |p: Option<String>| p.map(|p| if p == "system" { p } else { base.join(p).to_string_lossy().into_owned() });
    match auth {
        None | Some(Value::Nil) => {}
        Some(Value::Str(s) | Value::Css(s)) if &**s == "quillo" => {}
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
                Some(Value::Str(t) | Value::Css(t)) => a.bearer = Some(Value::Str(t.clone())),
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
                    Value::Str(h) | Value::Css(h) if !h.is_empty() && h.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') => a.forward.push(h.to_ascii_lowercase()),
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
    /// `client_cert: { chain: true }`: several elements taken, the last (Envoy's APPEND_FORWARD);
    /// else one only (FEEDBACK71.md, Q-1000; branch security).
    pub client_cert_chain: bool,
    /// `client_cert: { subject: true }`: the header is the certificate's subject alone, one line,
    /// as nginx's `$ssl_client_s_dn` gives it; else it is Envoy's (FEEDBACK71.md, Q-1000).
    pub client_cert_subject: bool,
    /// `check: (caller) => ...`: anything else, in Quillo.
    pub check: Option<Value>,
}

impl Accept {
    /// Whether it takes another way of proving which caller calls than the shared secret, which
    /// every service of the system holds: a key, a certificate or a token. Then the secret proves
    /// only that a call is one of the system's services, not which (the user's decision,
    /// 2026-11-17; FEEDBACK71.md, Q-970; branch security).
    pub fn names_callers(&self) -> bool {
        !self.keys.is_empty() || self.client_cert.is_some() || self.jwt.is_some()
    }
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
                    Value::Str(s) | Value::Css(s) => s.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
                    Value::List(l) => l.iter().filter_map(|v| if let Value::Str(s) | Value::Css(s) = v { Some(s.to_string()) } else { None }).filter(|s| !s.is_empty()).collect(),
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
                Some(Value::Str(s) | Value::Css(s)) if !s.is_empty() => Some(s.to_string()),
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
        Some(Value::Map(c)) => {
            if let Some(k) = c.keys().find(|k| !["header", "chain", "subject"].contains(&&***k)) {
                return Err(format!("`accept_calls`: `client_cert` has no `{}` (header, chain, subject)", k));
            }
            match c.get("header") {
                Some(Value::Str(h) | Value::Css(h)) if !h.is_empty() => a.client_cert = Some(h.to_ascii_lowercase()),
                _ => return Err("`accept_calls`: `client_cert` needs `header`, the one the proxy that ended mTLS sets (x-forwarded-client-cert)".into()),
            }
            a.client_cert_chain = match c.get("chain") {
                None | Some(Value::Nil) => false,
                Some(Value::Bool(b)) => *b,
                Some(other) => return Err(format!("`accept_calls`: `client_cert`'s `chain` is true or false, not {}", crate::interp::short(other))),
            };
            a.client_cert_subject = match c.get("subject") {
                None | Some(Value::Nil) => false,
                Some(Value::Bool(b)) => *b,
                Some(other) => return Err(format!("`accept_calls`: `client_cert`'s `subject` is true or false, not {}", crate::interp::short(other))),
            };
            if a.client_cert_subject && a.client_cert_chain {
                return Err("`accept_calls`: `client_cert` takes `chain` or `subject`, not both: `chain` is for Envoy's elements, and with `subject: true` the header is one subject".into());
            }
        }
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
/// the SPIFFE id, whole, or the CN (see `cert_caller`), the last of several elements taken, as
/// `client_cert: { chain: true }` takes it.
pub fn cert_identity(header: &str) -> Option<(String, Vec<(&'static str, Value)>)> {
    cert_caller(header, true).ok()
}

/// The keys Envoy writes in an element, in the order it writes them: `By` for each URI of the
/// proxy's own certificate, then the client certificate's `Hash`, `Cert`, `Chain`, `Subject`, and
/// a `URI` and a `DNS` for each of its names.
const ENVOY_KEYS: [&str; 7] = ["By", "Hash", "Cert", "Chain", "Subject", "URI", "DNS"];

/// `cert_identity`, or why the header names no caller. `chain`: several elements are taken,
/// the last being the client's (Envoy's `APPEND_FORWARD`); else the header is one proxy's
/// element alone (`SANITIZE_SET`).
///
/// A SPIFFE id is the caller's name whole, `spiffe://cluster.local/ns/shop/sa/shop`, as Istio's
/// policies name principals: its last part alone made any workload that could have a service
/// account called `shop`, in any namespace or trust domain, `shop` (the user's decision,
/// 2026-11-17; FEEDBACK71.md, Q-971; branch security). It is compared as written. A certificate
/// with two URIs is no SPIFFE id (an X.509-SVID has exactly one), and neither is taken: refused.
///
/// Envoy writes a URI and a DNS name bare, unquoted, whatever is in it, so a name a CA issued
/// with a `,` in it ended the proxy's element and began another, the last, which its holder
/// wrote: `.../sa/x,URI=spiffe://.../sa/shop` named the caller shop. So by default a header is
/// one element, a `spiffe://` URI is a SPIFFE id as its spec writes one (`spiffe_id`), another
/// URI holds nothing that could end it, and an element holds only Envoy's keys, in Envoy's
/// order, quoted where Envoy quotes them, so that a name written into it cannot add a pair the
/// proxy did not write (the user's decision, 2026-11-17; FEEDBACK71.md, Q-1000; branch security).
pub fn cert_caller(header: &str, chain: bool) -> Result<(String, Vec<(&'static str, Value)>), String> {
    let mut elements = xfcc(header)?;
    if elements.len() > 1 && !chain {
        return Err(format!(
            "has {} elements: one proxy's is expected, in place of any the client sent (Envoy's SANITIZE_SET); `client_cert: {{ chain: true }}` takes the last of several (APPEND_FORWARD)",
            elements.len()
        ));
    }
    for (_, pairs) in &elements {
        envoy_element(pairs)?;
    }
    let Some((_, pairs)) = elements.pop() else { return Err("names no certificate's subject".into()) };
    let values = |k: &str| pairs.iter().filter(|(a, _, _)| a.eq_ignore_ascii_case(k)).map(|(_, v, _)| v.clone()).collect::<Vec<String>>();
    let subjects = values("Subject");
    if subjects.len() > 1 {
        return Err(format!("names {} subjects for the client's certificate, so who calls is not certain", subjects.len()));
    }
    let subject = subjects.into_iter().next().unwrap_or_default();
    let uris: Vec<String> = values("URI").into_iter().filter(|u| !u.is_empty()).collect();
    if uris.len() > 1 {
        return Err(format!("names {} URIs for the client's certificate: a SPIFFE id is one (an X.509-SVID has exactly one), so who calls is not certain", uris.len()));
    }
    let uri = uris.into_iter().next();
    // Only a SPIFFE id names the caller; another URI is shown, and the CN names it, read clearly.
    // `URI=https://victim.example/;DNS=x` is what Envoy writes for a URI with `;DNS=x` in it, and
    // for a URI and a DNS name: a name of the victim's, had any URI named the caller
    // (FEEDBACK71.md, Q-1000; branch security).
    let name = match &uri {
        Some(u) if u.starts_with("spiffe://") => u.clone(),
        _ => cn_of(&subject_attrs(&subject)?)?,
    };
    let mut extra: Vec<(&'static str, Value)> = vec![("subject", Value::str(subject.as_str()))];
    if let Some(u) = uri {
        extra.push(("uri", Value::str(u.as_str())));
    }
    Ok((name, extra))
}

/// `client_cert: { header, subject: true }`: the header is the certificate's subject alone, in
/// RFC 2253's form, as nginx's `proxy_set_header X-Client-DN $ssl_client_s_dn` sets it. Read
/// whole: `CN=evil,OU=x,CN=shop` is two CNs. Which form a header is in is the program's to say,
/// never the header's: guessed from its first key, a client's own `/CN=shop/O=` before Envoy's
/// appended `,Hash=...` made the whole header a subject that named shop (FEEDBACK71.md, Q-1000;
/// branch security).
pub fn subject_caller(subject: &str) -> Result<(String, Vec<(&'static str, Value)>), String> {
    let attrs = subject_attrs(subject)?;
    if let Some((t, _, _)) = attrs.iter().find(|(t, _, _)| ENVOY_KEYS.iter().any(|k| k.eq_ignore_ascii_case(t))) {
        return Err(format!("is a subject with `{}` in it, one of Envoy's keys: is it Envoy's header (no `subject: true`)?", t));
    }
    Ok((cn_of(&attrs)?, vec![("subject", Value::str(subject))]))
}

/// An element as Envoy writes one: its keys only, in its order, quoted where it quotes (`Cert`,
/// `Chain`, `Subject`) and nowhere else, and each URI one that cannot have ended early (Q-1000).
fn envoy_element(pairs: &[(String, String, bool)]) -> Result<(), String> {
    let mut last = 0;
    for (k, v, quoted) in pairs {
        let Some(rank) = ENVOY_KEYS.iter().position(|e| e.eq_ignore_ascii_case(k)) else {
            return Err(format!("has `{}`, a key Envoy does not write (By, Hash, Cert, Chain, Subject, URI, DNS)", k.chars().take(40).collect::<String>()));
        };
        if rank < last {
            return Err("has its keys out of Envoy's order (By, Hash, Cert, Chain, Subject, URI, DNS): a pair the proxy did not write?".into());
        }
        last = rank;
        if *quoted && matches!(rank, 0 | 1 | 5 | 6) {
            return Err(format!("quotes `{}`, which Envoy writes bare", ENVOY_KEYS[rank]));
        }
        // Envoy quotes these always: a `Subject=CN=ops` after a comma in a name was no proxy's.
        if !*quoted && matches!(rank, 2..=4) {
            return Err(format!("writes `{}` bare, which Envoy quotes", ENVOY_KEYS[rank]));
        }
        if rank == 5 && !v.is_empty() {
            let spiffe = v.get(..9).is_some_and(|s| s.eq_ignore_ascii_case("spiffe://"));
            if spiffe {
                spiffe_id(v)?;
            } else if let Some(c) = v.chars().find(|c| matches!(c, ',' | ';' | '=' | '"' | '\\') || c.is_whitespace()) {
                return Err(format!("gives a URI with `{}` in it: Envoy writes it bare, so where it ends is not certain", c.escape_debug()));
            }
        }
    }
    Ok(())
}

/// A SPIFFE id as the SPIFFE ID spec writes one: `spiffe://` in lower case, a trust domain of
/// `[a-z0-9.-_]`, path segments of `[a-zA-Z0-9.-_]`, none empty, `.` or `..`, no query, fragment,
/// port or user, and 2,048 bytes at most (Q-1000).
fn spiffe_id(u: &str) -> Result<(), String> {
    let bad = |why: &str| Err(format!("gives `{}`, which is not a SPIFFE id: {}", u.chars().take(80).collect::<String>(), why));
    let Some(rest) = u.strip_prefix("spiffe://") else { return bad("its scheme is `spiffe`, in lower case") };
    if u.len() > 2048 {
        return bad("it is longer than 2,048 bytes");
    }
    let (domain, path) = match rest.split_once('/') {
        Some((d, p)) => (d, Some(p)),
        None => (rest, None),
    };
    if domain.is_empty() || !domain.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'_')) {
        return bad("its trust domain is lower-case letters, digits, `.`, `-` and `_`");
    }
    for seg in path.map(|p| p.split('/').collect::<Vec<&str>>()).unwrap_or_default() {
        if seg.is_empty() || seg == "." || seg == ".." {
            return bad("a part of its path is empty, `.` or `..`");
        }
        if !seg.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_')) {
            return bad("its path is letters, digits, `.`, `-` and `_`, between slashes");
        }
    }
    Ok(())
}

/// A certificate's subject's attributes, each its type, its value with its escapes decoded
/// (`Acme\, Inc.` is `Acme, Inc.`) and whether it was in hex, read in the form it is written in:
/// RFC 4514's (`CN=shop,O=Acme\, Inc.`, what Envoy sends, RFC 2253's spaces around `,` `+` `=`
/// allowed). `cn_of` takes the CN attribute itself: one, not empty, and with no `:`.
///
/// The CN was the first piece after a `,` or a `/` that began `CN=`, wherever it was: an OU of
/// `a/CN=shop`, written ahead of the CN, named the caller shop (FEEDBACK71.md, Q-989; branch
/// security).
fn subject_attrs(subject: &str) -> Result<Vec<(String, String, bool)>, String> {
    // OpenSSL's slash form is not read: its printers escape `/` and `+` in a value or not, and
    // join an RDN's attributes with `+`, so `/O=x\/CN=admin` and `/CN=shop+OU=x` have no one
    // reading (FEEDBACK71.md, Q-989; branch security).
    if subject.starts_with('/') {
        return Err("gives the subject in OpenSSL's slash form (`/O=.../CN=...`), which has no one reading: send it as RFC 2253 writes it (`CN=...,O=...`; nginx's `$ssl_client_s_dn`, not `$ssl_client_s_dn_legacy`)".into());
    }
    rfc4514_subject(subject)
}

/// The one CN among a subject's attributes (`subject_attrs`).
fn cn_of(attrs: &[(String, String, bool)]) -> Result<String, String> {
    let cns: Vec<&(String, String, bool)> = attrs.iter().filter(|(t, _, _)| t.eq_ignore_ascii_case("CN") || t.eq_ignore_ascii_case("commonName") || t == "2.5.4.3").collect();
    match cns.as_slice() {
        [] => Err("names no certificate's subject".into()),
        [(_, _, true)] => Err("gives the subject's CN in hex (`#...`), which is not read".into()),
        [(_, v, _)] if v.is_empty() => Err("gives an empty CN".into()),
        // `CN=spiffe://td/ns/prod/sa/admin` passed a `for` that names that id (Q-989).
        [(_, v, _)] if v.contains(':') => Err("gives a CN with `:` in it, which could pass for a URI's name: it names no caller".into()),
        [(_, v, _)] => Ok(v.clone()),
        more => Err(format!("gives {} CNs in the certificate's subject, so who calls is not certain", more.len())),
    }
}

/// An attribute's type, as RFC 4514 writes one: a name (`CN`, `emailAddress`) or an OID.
fn attribute_type(t: &str) -> Result<String, String> {
    let name = t.starts_with(|c: char| c.is_ascii_alphabetic()) && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    let oid = !t.is_empty() && t.chars().all(|c| c.is_ascii_digit() || c == '.');
    if name || oid {
        Ok(t.to_string())
    } else {
        Err(format!("gives a subject that does not parse: `{}` is no attribute's type", t.chars().take(40).collect::<String>()))
    }
}

/// RFC 4514's form: each attribute's type, its value decoded, and whether it was in hex (`#...`).
fn rfc4514_subject(s: &str) -> Result<Vec<(String, String, bool)>, String> {
    let bad = |why: &str| Err(format!("gives a subject that does not parse: {}", why));
    let b = s.as_bytes();
    let mut out = Vec::new();
    if s.trim().is_empty() {
        return Ok(out);
    }
    let mut i = 0;
    loop {
        let from = i;
        while i < b.len() && !matches!(b[i], b'=' | b',' | b'+' | b'\\') {
            i += 1;
        }
        if i == b.len() || b[i] != b'=' {
            return bad("an attribute is not type=value");
        }
        let ty = attribute_type(s[from..i].trim())?;
        i += 1;
        while i < b.len() && b[i] == b' ' {
            i += 1;
        }
        let hex = b.get(i) == Some(&b'#');
        // The value's bytes, and how many are kept: unescaped spaces at its end are not its.
        let (mut v, mut keep) = (Vec::new(), 0);
        while i < b.len() && !matches!(b[i], b',' | b'+') {
            match b[i] {
                b'\\' => {
                    let hexval = |c: u8| (c as char).to_digit(16);
                    match (b.get(i + 1).copied(), b.get(i + 2).copied()) {
                        (Some(h), Some(l)) if hexval(h).is_some() && hexval(l).is_some() => {
                            v.push((hexval(h).unwrap_or(0) * 16 + hexval(l).unwrap_or(0)) as u8);
                            i += 3;
                        }
                        (Some(c), _) if b"\\\"+,;<> #=".contains(&c) => {
                            v.push(c);
                            i += 2;
                        }
                        _ => return bad("a backslash escapes nothing RFC 4514 escapes"),
                    }
                    keep = v.len();
                }
                c @ (b'"' | b';' | b'<' | b'>') => return bad(&format!("`{}` in a value, not escaped", c as char)),
                c => {
                    v.push(c);
                    i += 1;
                    if c != b' ' {
                        keep = v.len();
                    }
                }
            }
        }
        v.truncate(keep);
        let Ok(v) = String::from_utf8(v) else { return bad("a value that is not UTF-8") };
        out.push((ty, v, hex));
        if i == b.len() {
            return Ok(out);
        }
        i += 1;
    }
}

/// Envoy's `x-forwarded-client-cert`, read as it is written: elements split by `,`, an element's
/// pairs by `;`, outside a value's quotes; a pair is `key=value`, a value with `,`, `;` or `=` in
/// it in quotes, where a backslash takes the next character as it is (`\"` and `\\`, and the
/// subject's own RFC 2253 escapes, as `\,` in `O=Acme\, Inc.`). Each element's text and its pairs,
/// a value as written between its quotes, and whether it was in quotes.
///
/// A quote was open or shut at each `"`, escaped or not, so an element of the client's (Envoy's
/// `APPEND_FORWARD` keeps those it was sent, before its own) with a quote it never closed made
/// the proxy's own element part of one of its values: `URI=spiffe://.../sa/shop;X="` before it
/// named the caller shop (FEEDBACK71.md, Q-988; branch security). Now a quote not closed, a
/// backslash outside one or a part that is not `key=value` refuses the header: the proxy's
/// element is read only as it would be alone.
fn xfcc(header: &str) -> Result<Vec<(&str, Vec<(String, String, bool)>)>, String> {
    let mut elements = Vec::new();
    let (mut quoted, mut escaped) = (false, false);
    let (mut from, mut pair_from) = (0, 0);
    let mut pairs: Vec<&str> = Vec::new();
    for (i, c) in header.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' if quoted => escaped = true,
            '\\' => return Err("has a backslash outside a quoted value".into()),
            '"' => quoted = !quoted,
            ';' | ',' if !quoted => {
                pairs.push(&header[pair_from..i]);
                pair_from = i + 1;
                if c == ',' {
                    elements.push((&header[from..i], std::mem::take(&mut pairs)));
                    from = i + 1;
                }
            }
            _ => {}
        }
    }
    if quoted || escaped {
        return Err("has a quote that is not closed, so where the proxy's own part begins is not certain".into());
    }
    pairs.push(&header[pair_from..]);
    elements.push((&header[from..], pairs));
    let mut out = Vec::new();
    if header.is_empty() {
        return Ok(out);
    }
    for (text, parts) in elements {
        let mut kv = Vec::new();
        // Nothing trimmed and nothing passed over: Envoy writes no space around `;`, `,` or `=`,
        // and no empty part or element, so `URI=spiffe://.../admin;DNS=x`, `...admin;`,
        // `...admin, ,` and a space after one were a URI cut short and read as whole
        // (FEEDBACK71.md, Q-1000; branch security).
        for p in parts {
            if p.is_empty() {
                return Err("has an empty part or element, which Envoy never writes".into());
            }
            let Some((k, v)) = p.split_once('=').filter(|(k, _)| !k.contains('"') && !k.is_empty()) else {
                return Err("has a part that is not key=value".into());
            };
            if [k.chars().next(), k.chars().last(), v.chars().next(), v.chars().last()].into_iter().flatten().any(char::is_whitespace) {
                return Err("has a space around a `;`, `,` or `=`, which Envoy never writes".into());
            }
            // Quotes around the whole value only, and none unescaped inside.
            let (inner, quoted) = match v.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
                Some(inner) => (inner, true),
                None if v.contains('"') => return Err("has a quote inside a value that is not quoted whole".into()),
                None => (v, false),
            };
            let mut esc = false;
            for c in inner.chars() {
                match c {
                    _ if esc => esc = false,
                    '\\' => esc = true,
                    '"' => return Err("has a quote inside a value that is not quoted whole".into()),
                    _ => {}
                }
            }
            kv.push((k.to_string(), inner.to_string(), quoted));
        }
        out.push((text, kv));
    }
    Ok(out)
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

/// `request.caller` of a token's claims; an error for a token a user signed in for. Such a token
/// names the client the user came through (`azp: "shop"`) and the user (`sub: "user-42"`), and
/// it passed `for ["shop"]`: a page forwarding its visitor's token called as the shop
/// (FEEDBACK51.md, Q-665; branch feedback51-fixes). A client's own token has the client as
/// `sub` (RFC 9068 §2.2, Okta), or is one of Auth0's client-credentials tokens, a Keycloak
/// service account's, or an Entra ID app's.
/// Okta names the client in `cid`, with the user's login in `sub` and no `client_id` or `azp`:
/// the name fell back to `sub`, which equals itself, so every Okta user's token for the audience
/// was a service's own, named by the login. `cid` names the client too, after `client_id` and
/// `azp`, and so does Entra ID's v1 `appid` (FEEDBACK75.md, Q-1079 and its review; branch
/// feedback75).
pub fn jwt_caller(claims: &serde_json::Value) -> Result<Value, String> {
    let name = ["client_id", "azp", "cid", "appid", "sub"].iter().find_map(|k| claims[*k].as_str()).unwrap_or("").to_string();
    let own = match claims["sub"].as_str() {
        None => true,
        Some(sub) => {
            sub == name
                || sub == format!("{}@clients", name)
                || claims["gty"] == "client-credentials"
                // Keycloak's service account: its name is one a user may be given, so the
                // client's own claim too (found in review).
                || claims["client_id"].as_str() == Some(name.as_str()) && claims["preferred_username"].as_str() == Some(format!("service-account-{}", name).as_str())
                // Entra ID: an app's token says so (`idtyp`), or is its service principal's,
                // with no delegated scopes (found in review).
                || claims["idtyp"] == "app"
                || claims["oid"].as_str() == Some(sub) && claims["scp"].is_null()
        }
    };
    if !own {
        return Err(format!("the token is one a user signed in for (sub {:?}), not client {:?}'s own: a service calls with its own (the client-credentials grant)", claims["sub"].as_str().unwrap_or(""), name));
    }
    let scopes: Vec<Value> = match &claims["scope"] {
        serde_json::Value::String(s) => s.split_whitespace().map(Value::str).collect(),
        serde_json::Value::Array(l) => l.iter().filter_map(|v| v.as_str().map(Value::str)).collect(),
        _ => match &claims["scp"] {
            serde_json::Value::Array(l) => l.iter().filter_map(|v| v.as_str().map(Value::str)).collect(),
            _ => Vec::new(),
        },
    };
    Ok(caller_value(&name, "jwt", vec![("scopes", Value::list(scopes)), ("claims", Value::from_json(claims))]))
}

pub type Shared = Arc<Accept>;
