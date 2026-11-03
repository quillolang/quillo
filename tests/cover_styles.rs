//! `style_with` given a function: a builtin, one that gives an error value, one that fails,
//! and one that replies; and the classes markup writes beside its other attributes.

use quillo::Interp;

fn loads(src: &str) -> Result<String, String> {
    match Interp::new(".").load_source(src, "case.qwe") {
        Ok(v) => Ok(v.repr()),
        Err(e) => Err(format!("{} {}", e.line, e.msg)),
    }
}

const CARD: &str = "component C() {\n    style css`.c { x: 1 }`\n    <p.c/>\n}\n";

#[test]
fn a_builtin_can_make_the_styles() {
    assert_eq!(loads(&format!("style_with(upper)\n{}1", CARD)), Ok("1".into()));
}

#[test]
fn a_style_function_that_fails_stops_the_load_with_why() {
    assert_eq!(loads(&format!("fn engine(css) = err(\"no engine\")\nstyle_with(engine)\n{}", CARD)), Err("0 style_with `engine` failed: no engine".into()));
    assert_eq!(loads(&format!("fn engine(css) = 1 / 0\nstyle_with(engine)\n{}", CARD)), Err("0 style_with `engine` failed: division by zero".into()));
    assert_eq!(
        loads(&format!("fn engine(css) {{\n    reply 404\n}}\nstyle_with(engine)\n{}", CARD)),
        Err("0 style_with `engine` failed: it did not give CSS text".into())
    );
}

#[test]
fn the_classes_written_skip_other_attributes() {
    assert_eq!(loads("component C() {\n    <p id=\"x\" class=\"a\" title=\"t\" class:on={true}/>\n}\nclasses()"), Ok("[\"a\", \"on\"]".into()));
}
