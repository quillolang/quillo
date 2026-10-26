//! Security headers and the Content-Security-Policy.
//!
//! ```text
//! security_headers({ hsts: "max-age=63072000; includeSubDomains", frame: "DENY" })
//! security_headers(false)                  // none; in a route, for its answer only
//! csp()                                    // a strict policy, with a nonce per answer
//! csp({ script_src: ["https://cdn.example.com"], report_only: true })
//! <script nonce={csp_nonce()}>...</script>
//! ```
//!
//! Every answer tells the browser how to treat it: `Referrer-Policy:
//! strict-origin-when-cross-origin`, `X-Frame-Options: SAMEORIGIN` with
//! `frame-ancestors 'self'` (no other site may put the page in a frame),
//! `X-Content-Type-Options: nosniff` (see server.rs), and
//! `Strict-Transport-Security` when the request came over HTTPS. Quillo
//! serves plain HTTP behind a proxy that ends TLS, so HTTPS is what the
//! proxy says (`X-Forwarded-Proto: https`). Browsers ignore the header on
//! plain HTTP, so forging it gains nothing.
//!
//! `csp()` adds a policy under which a page runs only its site's scripts
//! and the inline ones carrying the answer's nonce: `script-src 'self'
//! 'nonce-…'; object-src 'none'; base-uri 'self'; frame-ancestors 'self'`.
//! The nonce is new for each answer. The scripts Quillo writes into pages
//! (the component loader, translations, prefetching, `--watch`'s reload)
//! carry it, and so must the program's own: `nonce={csp_nonce()}`. The
//! browser runtime needs no `eval`.
//!
//! At the top of a program these set every answer's headers; in a route,
//! only that answer's. A header the route sets itself (`respond(200, body,
//! { "x-frame-options": "DENY" })`) is kept as it is.

use crate::interp::{fail, Ctrl, Interp};
use crate::server::{HttpRequest, HttpResponse};
use crate::value::*;

type R = Result<Value, Ctrl>;

pub const BUILTINS: &[(&str, BuiltinFn)] = &[("security_headers", security_headers), ("csp", csp), ("csp_nonce", csp_nonce)];

/// The headers every answer carries (`security_headers`). None: not sent.
#[derive(Clone, Debug)]
pub struct Policy {
    pub hsts: Option<String>,
    pub referrer: Option<String>,
    /// `X-Frame-Options`: SAMEORIGIN or DENY.
    pub frame: Option<String>,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            hsts: Some("max-age=31536000".into()),
            referrer: Some("strict-origin-when-cross-origin".into()),
            frame: Some("SAMEORIGIN".into()),
        }
    }
}

/// A Content-Security-Policy (`csp`): its directives, besides `script-src`,
/// which always starts with `'self' 'nonce-…'`.
#[derive(Clone, Debug, Default)]
pub struct Csp {
    /// Sources added to `script-src`.
    pub scripts: Vec<String>,
    /// Directives the program set; an empty value leaves one out.
    pub directives: Vec<(String, String)>,
    /// Sent as `Content-Security-Policy-Report-Only`: violations are
    /// reported, not blocked.
    pub report_only: bool,
}

/// What a request set about its own answer, and the session it found
/// (see `session_store`). Cleared for each request.
#[derive(Default)]
pub struct PerRequest {
    /// The answer's nonce, made when first needed.
    pub nonce: Option<String>,
    /// `security_headers` in the route: Some(None) is none at all.
    pub policy: Option<Option<Policy>>,
    /// `csp` in the route: Some(None) is no policy.
    pub csp: Option<Option<Csp>>,
    /// The id (as stored) of the request's session in the database.
    pub session_id: Option<String>,
    /// Whether `session_id` was looked for already.
    pub session_read: bool,
}

/// Whether code runs for a request (rather than when the program loads).
fn in_request(it: &mut Interp) -> bool {
    it.request_value().is_some()
}

/// The answer's nonce, made if there is none yet.
pub fn nonce(it: &mut Interp) -> String {
    if let Some(n) = &it.access.nonce {
        return n.clone();
    }
    let n = crate::crypto::b64url(&crate::crypto::secret_bytes(16));
    it.access.nonce = Some(n.clone());
    n
}

/// ` nonce="…"` for a script or style Quillo writes, when the answer has a nonce.
pub fn nonce_attr(it: &Interp) -> String {
    it.access.nonce.as_ref().map_or_else(String::new, |n| format!(" nonce=\"{}\"", n))
}

/// The nonce in an answer's policy, for what is added to it later (`--watch`'s script).
pub fn nonce_in(resp: &HttpResponse) -> Option<String> {
    let csp = resp.header("content-security-policy").or_else(|| resp.header("content-security-policy-report-only"))?;
    let at = csp.find("'nonce-")? + 7;
    let rest = &csp[at..];
    Some(rest[..rest.find('\'')?].to_string())
}

fn sources(v: &Value, what: &str, line: u32) -> Result<Vec<String>, Ctrl> {
    let one = |s: &str| -> Result<String, Ctrl> {
        // A `;` or `,` would start another directive or policy.
        if s.is_empty() || s.bytes().any(|b| b == b';' || b == b',' || b < 0x20 || b == 0x7f) {
            return fail(line, format!("`csp`'s {} has sources such as \"https://cdn.example.com\" or \"'self'\", got {:?}", what, s));
        }
        Ok(s.to_string())
    };
    match v {
        Value::Str(s) => s.split_whitespace().map(one).collect(),
        Value::List(l) => l.iter().map(|x| one(&x.display())).collect(),
        Value::Nil | Value::Bool(false) => Ok(Vec::new()),
        v => fail(line, format!("`csp`'s {} is a str or a list of them, got {}", what, v.type_name())),
    }
}

fn csp_from(args: &[Value], line: u32) -> Result<Option<Csp>, Ctrl> {
    let m = match args.first() {
        None | Some(Value::Nil) | Some(Value::Bool(true)) => return Ok(Some(Csp::default())),
        Some(Value::Bool(false)) => return Ok(None),
        Some(Value::Map(m)) => m.clone(),
        Some(v) => return fail(line, format!("`csp` takes a map of directives, or false, got {}", v.type_name())),
    };
    let mut c = Csp::default();
    for (k, v) in m.iter() {
        let name = k.replace('_', "-").to_ascii_lowercase();
        match name.as_str() {
            "report-only" => c.report_only = v.truthy(),
            "script-src" => c.scripts = sources(v, k, line)?,
            _ if name.bytes().all(|b| b.is_ascii_lowercase() || b == b'-') && !name.is_empty() => {
                c.directives.push((name, sources(v, k, line)?.join(" ")));
            }
            _ => return fail(line, format!("`csp` takes directives such as img_src or connect_src, got {:?}", &**k)),
        }
    }
    Ok(Some(c))
}

/// `csp()`, `csp({ img_src: [...], report_only: true })` or `csp(false)`:
/// for every answer at the top of the program, for this one in a route.
fn csp(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let c = csp_from(&args, line)?;
    if in_request(it) {
        it.access.csp = Some(c);
    } else {
        std::sync::Arc::make_mut(&mut it.site).csp = c;
    }
    Ok(Value::Nil)
}

/// `csp_nonce()`: this answer's nonce, for `<script nonce={csp_nonce()}>`.
fn csp_nonce(it: &mut Interp, _: Vec<Value>, _: u32) -> R {
    Ok(Value::str(nonce(it)))
}

fn header_value(v: &Value, what: &str, line: u32) -> Result<Option<String>, Ctrl> {
    match v {
        Value::Nil | Value::Bool(false) => Ok(None),
        Value::Str(s) if !s.is_empty() && crate::server::valid_header("x", s) => Ok(Some(s.to_string())),
        v => fail(line, format!("`security_headers`' {} is a header's value or false, got {}", what, v.repr())),
    }
}

/// `security_headers({ hsts, referrer_policy, frame })`, or false for none:
/// for every answer at the top of the program, for this one in a route.
fn security_headers(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let base = if in_request(it) { it.access.policy.clone().unwrap_or_else(|| Some(it.site.headers.clone())) } else { Some(it.site.headers.clone()) };
    let p = match args.first() {
        Some(Value::Bool(false)) => None,
        None | Some(Value::Nil) | Some(Value::Bool(true)) => Some(Policy::default()),
        Some(Value::Map(m)) => {
            let mut p = base.unwrap_or_default();
            for (k, v) in m.iter() {
                match &**k {
                    "hsts" => p.hsts = header_value(v, k, line)?,
                    "referrer_policy" => p.referrer = header_value(v, k, line)?,
                    "frame" => {
                        p.frame = header_value(v, k, line)?.map(|f| f.to_ascii_uppercase());
                        if p.frame.as_deref().map_or(false, |f| f != "SAMEORIGIN" && f != "DENY") {
                            return fail(line, "`security_headers`' frame is \"SAMEORIGIN\", \"DENY\" or false");
                        }
                    }
                    _ => return fail(line, format!("`security_headers` takes hsts, referrer_policy and frame, got {:?}", &**k)),
                }
            }
            Some(p)
        }
        Some(v) => return fail(line, format!("`security_headers` takes a map, or false, got {}", v.type_name())),
    };
    if in_request(it) {
        it.access.policy = Some(p);
    } else {
        std::sync::Arc::make_mut(&mut it.site).headers = p.unwrap_or(Policy { hsts: None, referrer: None, frame: None });
    }
    Ok(Value::Nil)
}

/// Whether the request reached the proxy in front over HTTPS.
fn over_https(req: &HttpRequest) -> bool {
    if crate::session::PLAIN_HTTP.load(std::sync::atomic::Ordering::Relaxed) {
        return false;
    }
    req.headers.iter().any(|(k, v)| {
        (k.eq_ignore_ascii_case("x-forwarded-proto") && v.split(',').next().map_or(false, |p| p.trim().eq_ignore_ascii_case("https")))
            || (k.eq_ignore_ascii_case("forwarded") && v.to_ascii_lowercase().replace(' ', "").contains("proto=https"))
    })
}

/// The policy's text, with the answer's nonce.
fn render(c: &Csp, nonce: &str, frame: Option<&str>) -> String {
    let mut out = vec![(
        "script-src".to_string(),
        std::iter::once("'self'".to_string()).chain(std::iter::once(format!("'nonce-{}'", nonce))).chain(c.scripts.iter().cloned()).collect::<Vec<_>>().join(" "),
    )];
    out.push(("object-src".into(), "'none'".into()));
    out.push(("base-uri".into(), "'self'".into()));
    if let Some(f) = frame {
        out.push(("frame-ancestors".into(), f.into()));
    }
    for (k, v) in &c.directives {
        match out.iter_mut().find(|(n, _)| n == k) {
            Some(d) => d.1 = v.clone(),
            None => out.push((k.clone(), v.clone())),
        }
    }
    out.iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| format!("{} {}", k, v)).collect::<Vec<_>>().join("; ")
}

/// Adds the security headers to an answer, leaving those it has.
pub fn apply(it: &mut Interp, req: &HttpRequest, resp: &mut HttpResponse) {
    let policy = match &it.access.policy {
        Some(p) => p.clone(),
        None => Some(it.site.headers.clone()),
    };
    let csp = match &it.access.csp {
        Some(c) => c.clone(),
        None => it.site.csp.clone(),
    };
    let add = |resp: &mut HttpResponse, k: &str, v: String| {
        if resp.header(k).is_none() {
            resp.headers.push((k.to_string(), v));
        }
    };
    let frame = policy.as_ref().and_then(|p| p.frame.as_deref()).map(|f| if f == "DENY" { "'none'" } else { "'self'" });
    if let Some(p) = &policy {
        if let (Some(h), true) = (&p.hsts, over_https(req)) {
            add(resp, "strict-transport-security", h.clone());
        }
        if let Some(r) = &p.referrer {
            add(resp, "referrer-policy", r.clone());
        }
        if let Some(f) = &p.frame {
            add(resp, "x-frame-options", f.clone());
        }
    }
    let has_csp = resp.header("content-security-policy").is_some() || resp.header("content-security-policy-report-only").is_some();
    match csp {
        Some(c) if !has_csp => {
            let n = nonce(it);
            let name = if c.report_only { "content-security-policy-report-only" } else { "content-security-policy" };
            resp.headers.push((name.into(), render(&c, &n, frame)));
            // Report-only blocks nothing, frames included.
            if c.report_only {
                if let Some(f) = frame {
                    resp.headers.push(("content-security-policy".into(), format!("frame-ancestors {}", f)));
                }
            }
        }
        None if !has_csp => {
            if let Some(f) = frame {
                resp.headers.push(("content-security-policy".into(), format!("frame-ancestors {}", f)));
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policies_read_as_written() {
        let c = Csp { scripts: vec!["https://cdn.example.com".into()], directives: vec![("img-src".into(), "'self' data:".into()), ("base-uri".into(), "'none'".into())], report_only: false };
        assert_eq!(
            render(&c, "abc", Some("'self'")),
            "script-src 'self' 'nonce-abc' https://cdn.example.com; object-src 'none'; base-uri 'none'; frame-ancestors 'self'; img-src 'self' data:"
        );
        let r = HttpResponse { status: 200, headers: vec![("content-security-policy".into(), render(&Csp::default(), "n0nce", None))], body: Vec::new() };
        assert_eq!(nonce_in(&r).as_deref(), Some("n0nce"));
    }
}
