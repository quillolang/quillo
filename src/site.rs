//! What a program sets about the site it serves, at load time, with
//! top-level calls such as `allow_cross_site("/payments/return")`. Shared,
//! unchanged, by every handle on the program (see `Interp::fork`).

#[derive(Default, Clone)]
pub struct Site {
    /// Path prefixes that take form posts from other sites (a payment
    /// provider's return, a single sign-on callback).
    pub cross_site_ok: Vec<String>,
    /// The folder `db.migrate` applies, again for each database opened later.
    pub migrations: Option<std::path::PathBuf>,
}

impl Site {
    pub fn takes_cross_site(&self, path: &str) -> bool {
        self.cross_site_ok.iter().any(|p| path == p || path.starts_with(&format!("{}/", p.trim_end_matches('/'))))
    }
}
