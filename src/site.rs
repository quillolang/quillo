//! What a program sets about the site it serves, at load time, with
//! top-level calls such as `allow_cross_site("/payments/return")`. Shared,
//! unchanged, by every handle on the program (see `Interp::fork`).

use crate::value::Value;

/// How a request's tenant is found (`tenant_from`).
#[derive(Clone)]
pub enum TenantRule {
    /// The part of the host before this domain: `acme.example.com` is `acme`.
    Subdomain(String),
    /// What this function gives for the request: a name, or null.
    Function(Value),
}

/// Whether `name` can be a tenant's: lowercase letters, digits, `-` and `_`,
/// starting with a letter or digit, at most 63 characters, so it is safe in
/// a file name, a database name or a URL.
pub fn tenant_name_ok(name: &str) -> bool {
    let b = name.as_bytes();
    !b.is_empty()
        && b.len() <= 63
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-' || *c == b'_')
}

#[derive(Default, Clone)]
pub struct Site {
    /// How each request's tenant is found; none for a single-tenant program.
    pub tenant_rule: Option<TenantRule>,
    /// Path prefixes that take form posts from other sites (a payment
    /// provider's return, a single sign-on callback).
    pub cross_site_ok: Vec<String>,
    /// The folder `db.migrate` applies, again for each database opened later.
    pub migrations: Option<std::path::PathBuf>,
    /// What `translations` loaded.
    pub i18n: Option<std::sync::Arc<crate::i18n::I18n>>,
    /// `locale_from`'s function of the request.
    pub locale_rule: Option<Value>,
    /// What `schedule` set to run (see `jobs`).
    pub jobs: Vec<std::sync::Arc<crate::jobs::Job>>,
    /// What `every` at the top set to run on the server (see `jobs`;
    /// FEEDBACK12.md, Q-080; branch feedback12-fixes).
    pub ticks: Vec<std::sync::Arc<crate::jobs::Tick>>,
    /// What `udp.listen` listens for (see `udp`).
    pub udp: Vec<std::sync::Arc<crate::udp::Listener>>,
    /// What `channel.on` handles (see `wt`).
    pub channels: Vec<std::sync::Arc<crate::wt::Handler>>,
}

impl Site {
    pub fn takes_cross_site(&self, path: &str) -> bool {
        self.cross_site_ok.iter().any(|p| path == p || path.starts_with(&format!("{}/", p.trim_end_matches('/'))))
    }
}
