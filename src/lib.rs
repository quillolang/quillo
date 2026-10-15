//! Lite: a small web language where routes, payload patterns and HTML are syntax.

pub mod ast;
pub mod builtins;
pub mod check;
pub mod client;
pub mod db;
pub mod error;
pub mod fetch;
pub mod interp;
pub mod lexer;
pub mod lsp;
pub mod parser;
pub mod server;
pub mod shared;
pub mod value;

pub use error::LiteError;
pub use interp::Interp;
pub use value::Value;

/// Runs a snippet and returns the value of its last statement.
pub fn eval_str(src: &str) -> Result<Value, LiteError> {
    Interp::new(".").load_source(src, "<input>")
}
