//! The lexer and parser on any text: an error is fine, a panic, a hang or
//! a stack overflow is not.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|src: &str| {
    let _ = quillo::parser::Parser::new(src).parse_program();
});
