//! The gzipped copies of files, kept by their ETag and body (src/statics.rs).
//! The first thing this process does with them is to keep one.

use quillo::statics::{gzipped, keep_gzipped};
use std::sync::Arc;

#[test]
fn a_kept_copy_is_found_by_its_etag_and_body_alone() {
    let gz: Arc<[u8]> = Arc::from(&b"gz of a"[..]);
    keep_gzipped("W/\"1\"", b"body a", gz.clone());
    assert_eq!(gzipped("W/\"1\"", b"body a"), Some(gz));
    assert_eq!(gzipped("W/\"1\"", b"body b"), None, "another body under the same ETag");
    assert_eq!(gzipped("W/\"2\"", b"body a"), None);
    // Kept again, the newer copy is the one found.
    let newer: Arc<[u8]> = Arc::from(&b"gz again"[..]);
    keep_gzipped("W/\"1\"", b"body a", newer.clone());
    assert_eq!(gzipped("W/\"1\"", b"body a"), Some(newer));
}
