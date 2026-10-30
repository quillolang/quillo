//! `parallel` when a function fails in a way no Quillo code can: a panic in a
//! builtin. The request or program gets an error, and goes on. The panic here
//! comes from a builtin of the test's own, as Quillo's own have none to provoke.

mod support;

use quillo::interp::Ctrl;
use quillo::server::{serve_with, App, Loaded};
use quillo::value::define;
use quillo::{Interp, Value};
use std::path::Path;
use std::sync::mpsc::channel;
use std::sync::Once;
use std::time::Duration;

const FAILED: &str = "a function given to `parallel` failed unexpectedly";

/// These panics are expected; any other is shown as usual.
fn quietly() {
    static SET: Once = Once::new();
    SET.call_once(|| {
        let shown = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |p| {
            if p.payload().downcast_ref::<&str>() != Some(&"boom") {
                shown(p);
            }
        }));
    });
}

fn boom(_: &mut Interp, _: Vec<Value>, _: u32) -> Result<Value, Ctrl> {
    panic!("boom");
}

/// `caught(f)`: what `f` failed with, as text.
fn caught(it: &mut Interp, args: Vec<Value>, line: u32) -> Result<Value, Ctrl> {
    match it.call(&args[0], Vec::new(), line) {
        Err(Ctrl::Error(e)) => Ok(Value::str(e.msg.as_str())),
        other => other,
    }
}

fn program(dir: &Path) -> Interp {
    let it = Interp::new(dir);
    define(&it.globals, "boom", Value::Builtin("boom", boom));
    define(&it.globals, "caught", Value::Builtin("caught", caught));
    it
}

#[test]
fn a_panic_in_a_thread_of_parallel_is_an_error_of_the_program() {
    quietly();
    let mut it = program(&std::env::temp_dir());
    let src = "let told = caught(() => parallel(() => 1, () => boom()))\nlet after = parallel(() => 2, () => 3)\n[told, after]\n";
    assert_eq!(it.load_source(src, "main.qwe").unwrap().repr(), format!("[\"{}\", [2, 3]]", FAILED));
}

#[test]
fn a_panic_in_a_request_of_parallel_is_an_error_of_that_request() {
    quietly();
    let d = support::temp_dir("parallel-panics");
    let (tx, rx) = channel();
    let dir = d.clone();
    std::thread::spawn(move || {
        let main = dir.join("main.qwe");
        let mut it = program(&dir);
        let src = "route GET /boom = caught(() => parallel(() => 1, () => boom()))\nroute GET /fine = parallel(() => 2, () => 3)\n";
        it.load_source(src, main.to_str().unwrap()).unwrap();
        let shared = it.shared.clone();
        let app = App::new(Loaded::Ready(it), shared, &main, &dir, false);
        serve_with(app, "127.0.0.1:0", 1, move |a| tx.send(a.port()).unwrap()).unwrap();
    });
    let port = rx.recv_timeout(Duration::from_secs(20)).expect("the server did not start");
    assert_eq!(support::send(port, "GET", "/boom", &[], "").body, FAILED);
    // The server took no harm from it.
    assert_eq!(support::send(port, "GET", "/fine", &[], "").body, "[2,3]");
}
