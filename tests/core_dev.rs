//! A program whose whole bundle is made when first asked for, in development.

use quillo::Interp;
use std::sync::atomic::Ordering;

#[test]
fn a_component_that_cannot_run_in_the_browser_says_so_in_the_console() {
    quillo::client::DEV.store(true, Ordering::Relaxed);
    let mut it = Interp::new(".");
    let src = "component A() {\n  state x = 0\n  <button @click={() => { x = read_file(\"a\") }}>{x}</button>\n}\n";
    if let Err(e) = it.load_source(src, "main.qwe") {
        panic!("{}", e);
    }
    let js = it.client_js();
    assert!(js.starts_with("console.error("), "{}", js);
    assert!(js.contains("is not available in the browser"), "{}", js);
}
