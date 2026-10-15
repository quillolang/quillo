//! Wisp: a small web language where routes, payload patterns and HTML are syntax.

pub mod ast;
pub mod builtins;
pub mod error;
pub mod interp;
pub mod lexer;
pub mod parser;
pub mod server;
pub mod value;

pub use error::WispError;
pub use interp::Interp;
pub use value::Value;

/// Runs a snippet and returns the value of its last statement.
pub fn eval_str(src: &str) -> Result<Value, WispError> {
    Interp::new(".").load_source(src, "<input>")
}
