//! Requests framed from a connection's bytes (src/httpd.rs): the same
//! requests whether the bytes come at once or in pieces, and no panic.
#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

#[derive(Arbitrary, Debug)]
struct Input {
    /// Each byte the length (less one) of a piece the bytes come in.
    cuts: Vec<u8>,
    /// A small body limit, to reach the "too large" answers.
    small: bool,
    data: Vec<u8>,
}

fuzz_target!(|i: Input| {
    quillo::httpd::fuzzing::framing(&i.data, &i.cuts, if i.small { 16 } else { 1 << 20 });
});
