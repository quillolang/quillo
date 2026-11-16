// FEEDBACK58.md, Q-874: a field limited to a set of strings is checked by a form's or a
// query's pattern as by JSON's: named, it takes the values of the set (it refused every one);
// written out, it says where its name is used that it binds none; and a miss names the field
// (branch cache-and-migrate).

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;

fn dir(name: &str, src: &str) -> PathBuf {
    let d = support::temp_dir(&format!("f58-{}", name));
    std::fs::write(d.join("main.qwe"), src).unwrap();
    d
}

fn check(d: &Path) -> (bool, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("check").arg(d.join("main.qwe")).output().unwrap();
    (o.status.success(), String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr))
}

const BOOKING: &str = r#"type Service = "oil_change" | "tire_rotation"
type Size = "S" | "M" | "L" | null
type Booking = { appointment_date: date, service: Service, size: Size = null }

route GET /q ? { service: Service } = `q ok ${service}`
route GET /o ? { size: Size = null } = `o ok ${size}`
route GET /many ? { service: [Service] } = `many ${len(service)}`
route POST /r { form: Booking } = `r ok ${form.service} ${form.size}`
route POST /b { form: { appointment_date: str, service: Service } } = `b ok ${service}`
route POST /d { json: { appointment_date: date, service: Service } } = `d ok ${service}`
route POST /s { form: { service: "oil_change" | "tire_rotation" } } = `s ok ${request.form.service}`
"#;

const FORM: (&str, &str) = ("content-type", "application/x-www-form-urlencoded");

#[test]
fn a_named_set_of_strings_takes_its_values_from_a_form_and_a_query() {
    let d = dir("named", BOOKING);
    assert!(check(&d).0, "{}", check(&d).1);
    let s = support::serve_with(&d.join("main.qwe"), &["--watch"]);
    let get = |path: &str| {
        let a = support::send(s.port, "GET", path, &[], "");
        (a.status, a.body)
    };
    let post = |path: &str, headers: &[(&str, &str)], body: &str| {
        let a = support::send(s.port, "POST", path, headers, body);
        (a.status, a.body)
    };
    assert_eq!(get("/q?service=tire_rotation"), (200, "q ok tire_rotation".into()));
    assert_eq!(get("/q?service=oil_change"), (200, "q ok oil_change".into()));
    assert_eq!(post("/r", &[FORM], "appointment_date=2026-11-12&service=tire_rotation"), (200, "r ok tire_rotation null".into()));
    assert_eq!(post("/r", &[FORM], "appointment_date=2026-11-12&service=oil_change&size=M"), (200, "r ok oil_change M".into()));
    assert_eq!(post("/b", &[FORM], "appointment_date=2026-11-12&service=tire_rotation"), (200, "b ok tire_rotation".into()));
    // JSON, as before; and the set written out, which checks the value.
    assert_eq!(post("/d", &[], r#"{"appointment_date":"2026-11-12","service":"tire_rotation"}"#), (200, "d ok tire_rotation".into()));
    assert_eq!(post("/s", &[FORM], "service=tire_rotation"), (200, "s ok tire_rotation".into()));
    // A set with null in it, and a default: absent is the default, a value of it is taken.
    assert_eq!(get("/o"), (200, "o ok null".into()));
    assert_eq!(get("/o?size=L"), (200, "o ok L".into()));
    // Every value of a name, each of the set.
    assert_eq!(get("/many?service=oil_change&service=tire_rotation"), (200, "many 2".into()));
    let _ = std::fs::remove_dir_all(&d);
}

// Edges: a value outside the set, its case changed, padded, empty, or of another script; a value
// of the set in a field of another type; all refused, and the miss names the field and the type,
// never the value sent.
#[test]
fn a_value_outside_the_set_is_refused_naming_the_field() {
    let d = dir("outside", BOOKING);
    let s = support::serve_with(&d.join("main.qwe"), &["--watch"]);
    for bad in ["brake_job", "Tire_Rotation", "tire_rotation%20", "", "%C3%B6il_change"] {
        let a = support::send(s.port, "GET", &format!("/q?service={}", bad), &[], "");
        assert_eq!(a.status, 400, "{}: {}", bad, a.body);
        assert!(a.body.contains("`query.service` is text, not a Service"), "{}: {}", bad, a.body);
        if !bad.is_empty() {
            assert!(!a.body.contains(bad), "{}", a.body);
        }
        let a = support::send(s.port, "POST", "/b", &[FORM], &format!("appointment_date=2026-11-12&service={}", bad));
        assert_eq!(a.status, 400, "{}: {}", bad, a.body);
        assert!(a.body.contains("`form.service` is text, not a Service"), "{}: {}", bad, a.body);
    }
    let a = support::send(s.port, "GET", "/many?service=oil_change&service=brake_job", &[], "");
    assert_eq!(a.status, 400, "{}", a.body);
    let a = support::send(s.port, "POST", "/r", &[FORM], "appointment_date=2026-11-12&service=brake_job");
    assert_eq!(a.status, 400, "{}", a.body);
    let a = support::send(s.port, "POST", "/r", &[FORM], "appointment_date=not-a-date&service=oil_change");
    assert_eq!(a.status, 400, "{}", a.body);
    let _ = std::fs::remove_dir_all(&d);
}

// A set written out in the pattern checks the field and binds no name: where the name is used,
// `quillo check` says so and what to write, a query's and a form's alike, and a single value
// written there (a webhook's `kind: "push"`) too.
#[test]
fn a_set_written_out_says_it_binds_no_name() {
    for (route, at) in [
        ("route POST /s { form: { service: \"oil_change\" | \"tire_rotation\" } } = `s ${service}`", "form.service"),
        ("route GET /q ? { service: \"oil_change\" | \"tire_rotation\" } = `q ${service}`", "query.service"),
        ("route POST /hook { json: { kind: \"push\" } } = `k ${kind}`", "json.kind"),
    ] {
        let d = dir("written", &format!("{}\n", route));
        let (ok, out) = check(&d);
        assert!(!ok, "{}", out);
        let name = at.rsplit('.').next().unwrap();
        assert!(
            out.contains(&format!("unknown name `{}`: the route's pattern checks `{}` against the values written there, which binds no name", name, at))
                && out.contains(&format!("write `{}: Choice` there to have `{}`, or read `request.{}`", name, name, at)),
            "{}",
            out
        );
        let _ = std::fs::remove_dir_all(&d);
    }
    // A name the pattern does bind, beside one it does not: no hint for the first.
    let d = dir("beside", "route POST /s { form: { service: \"a\" | \"b\", note: str } } = `${note} ${request.form.service}`\n");
    assert!(check(&d).0, "{}", check(&d).1);
    let _ = std::fs::remove_dir_all(&d);
}

// Lines 0.8.6's coverage run did not reach: a set of strings named inside another named type
// (`Outer = Inner | null`), and a type mixing a scalar with a string (`int | "auto"`), each
// refusing a value outside it by the field's name and taking those inside.
#[test]
fn a_set_named_inside_another_type_and_a_scalar_beside_a_string() {
    let d = dir("nested", "type Inner = \"a\" | \"b\"\ntype Outer = Inner | null\ntype Width = int | \"auto\"\nroute GET /o ? { x: Outer = null } = `o ${x}`\nroute GET /w ? { w: Width } = `w ${w}`\n");
    assert!(check(&d).0, "{}", check(&d).1);
    let s = support::serve_with(&d.join("main.qwe"), &["--watch"]);
    let get = |p: &str| {
        let a = support::send(s.port, "GET", p, &[], "");
        (a.status, a.body)
    };
    assert_eq!(get("/o?x=a"), (200, "o a".into()));
    assert_eq!(get("/o"), (200, "o null".into()));
    let (st, body) = get("/o?x=c");
    assert!(st == 400 && body.contains("`query.x` is text, not an Outer"), "{} {}", st, body);
    assert_eq!(get("/w?w=12"), (200, "w 12".into()));
    assert_eq!(get("/w?w=auto"), (200, "w auto".into()));
    let (st, body) = get("/w?w=wide");
    assert!(st == 400 && body.contains("`query.w` is text, not a Width"), "{} {}", st, body);
    let _ = std::fs::remove_dir_all(&d);
}
