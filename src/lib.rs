//! Quillo: a small web language where routes, payload patterns and HTML are syntax.

pub mod alloc;
pub mod ast;
pub mod builtins;
pub mod check;
pub mod client;
pub mod cluster;
pub mod crypto;
pub mod dates;
pub mod db;
pub mod txn;
pub mod decimal;
pub mod describe;
pub mod dotenv;
pub mod debug;
pub mod error;
pub mod fetch;
pub mod highlight;
pub mod httpd;
pub mod i18n;
pub mod idempotency;
pub mod install;
pub mod imaging;
pub mod island;
pub mod jobs;
pub mod udp;
pub mod wt;
pub mod media;
pub mod native;
pub mod interp;
#[cfg(feature = "jit")]
pub mod jit;
pub mod lexer;
pub mod livecache;
pub mod metrics;
pub mod migrate;
pub mod money;
#[cfg(feature = "mysql")]
pub mod my;
pub mod lsp;
pub mod parser;
pub mod script;
pub mod pg;
pub mod server;
pub mod numeric;
pub mod re;
pub mod ready;
pub mod record;
pub mod par;
pub mod service;
pub mod stream;
pub mod styles;
pub mod shared;
pub mod session;
pub mod site;
pub mod statics;
pub mod stack;
pub mod sqlinfo;
pub mod upload;
pub mod value;
pub mod viewcache;
pub mod ws;

pub use error::QuilloError;
pub use interp::Interp;
pub use value::Value;

/// This build's version, and the commit it was built from when known
/// (build.rs): "0.5.0 (d2c2aa4, 2026-10-27)".
pub fn version() -> String {
    match env!("QUILLO_COMMIT") {
        "" => env!("CARGO_PKG_VERSION").to_string(),
        commit => format!("{} ({})", env!("CARGO_PKG_VERSION"), commit),
    }
}

/// Runs a snippet and returns the value of its last statement.
pub fn eval_str(src: &str) -> Result<Value, QuilloError> {
    Interp::new(".").load_source(src, "<input>")
}
