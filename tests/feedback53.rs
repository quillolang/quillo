//! FEEDBACK53.md: a long-read publishing site (quilloer.com) built on Quillo 0.6.0
//! (Q-682 to Q-684, and Q-008 and Q-029 seen again; branch main).

mod support;

use quillo::check::{check_source, Severity};
use quillo::Interp;
use std::path::{Path, PathBuf};

fn diags(src: &str) -> Vec<String> {
    check_source(src, "t.qwe", Path::new("."))
        .diagnostics
        .iter()
        .map(|d| format!("{}{} {}", d.line, if d.severity == Severity::Warning { " warning:" } else { "" }, d.message))
        .collect()
}

fn run_in(d: &Path, src: &str) -> String {
    Interp::new(d).load_source(src, "t.qwe").map(|v| v.display()).unwrap_or_else(|e| format!("error: {}", e))
}

fn run(src: &str) -> String {
    run_in(Path::new("."), src)
}

fn dir(name: &str) -> PathBuf {
    support::temp_dir(&format!("f53-{}", name))
}

// Q-683: `/@:name`, profiles' addresses, and the other characters a URL's path takes.
#[test]
fn a_path_takes_at_and_a_parameter_after_it() {
    let d = dir("at");
    std::fs::write(
        d.join("main.qwe"),
        r#"route GET /@:name = `profile ${name}`
route GET /~:user(int) = `user ${user + 1}`
route GET /u/@me = "me"
route GET /c++ = "plus"
route GET /a@b!$& = "chars"
route GET /:page = `page ${page}`
"#,
    )
    .unwrap();
    let s = support::serve(&d.join("main.qwe"), &[]);
    let get = |p: &str| support::send(s.port, "GET", p, &[], "").body;
    assert_eq!(get("/@ann"), "profile ann");
    // Sent encoded, as some clients do, it is the same address.
    assert_eq!(get("/%40ann"), "profile ann");
    assert_eq!(get("/@x@y"), "profile x@y");
    // The prefix alone has no value: the next route takes it.
    assert_eq!(get("/@"), "page @");
    assert_eq!(get("/~41"), "user 42");
    // Not of its type: the next route.
    assert_eq!(get("/~x"), "page ~x");
    assert_eq!(get("/u/@me"), "me");
    assert_eq!(get("/c++"), "plus");
    assert_eq!(get("/a@b!$&"), "chars");
    assert_eq!(get("/plain"), "page plain");
}

#[test]
fn a_parameter_after_a_prefix_is_checked_shown_and_described() {
    // One that takes every request a later one would is said.
    let d = diags("route GET /@:name = name\nroute GET /@@:x = x\nroute GET /@ann = \"ann\"\nroute GET /:page = page\nroute GET /@ = \"at\"\n");
    assert!(d.iter().any(|x| x.starts_with("2 warning: this route is never reached: `GET /@:name` (line 1)")), "{:?}", d);
    assert!(d.iter().any(|x| x.starts_with("3 warning: this route is never reached: `GET /@:name` (line 1)")), "{:?}", d);
    // `/@` alone is not one of them: `/:page` takes it, which says so in its own words.
    assert!(!d.iter().any(|x| x.starts_with("5 ") && x.contains("/@:name")), "{:?}", d);
    assert!(d.iter().any(|x| x.starts_with("5 warning: this route is never reached: `GET /:page` (line 4)")), "{:?}", d);
    assert!(!d.iter().any(|x| x.starts_with("4 ")), "{:?}", d);
    // The parameter is the route's, typed as written.
    assert_eq!(diags("route GET /~:n(int) = n + 1\n"), Vec::<String>::new());
    assert!(diags("route GET /@:n/@:n = n\n").iter().any(|x| x.contains("path parameter `n` appears twice")));
    // Shown as written, and in OpenAPI with its prefix before the template.
    let d = dir("describe");
    std::fs::write(d.join("main.qwe"), "route GET /@:name = name\n").unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_quillo")).args(["docs", "main.qwe"]).current_dir(&d).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("/@{name}"), "{}", text);
    // What a path still cannot hold says so.
    assert!(run("route GET /a=b = 1").contains("error"), "{}", run("route GET /a=b = 1"));
    let e = run("route GET /a:b = 1");
    assert!(e.contains("a path cannot hold `:` here: a parameter is a piece of its own"), "{}", e);
}

// Q-682: `<script>{x}</script>` wrote nothing for null and false, and a string written in the
// braces as the script's own text.
#[test]
fn every_value_in_a_script_is_written_as_json() {
    let src = r#"let none = null
let no = false
let empty = ""
let name = "Ann"
str(<script>const a = {null}; const g = {none}; const b = {false}; const b2 = {no}; const d = {""}; const s = {"hello"}; const d2 = {empty}; const t = {name}; const h = {true}; const c = {0}; const e = {[null, false]}; const f = {{ x: null }}; const x = {"</script><b>"};</script>)"#;
    assert_eq!(
        run(src),
        r#"<script>const a = null; const g = null; const b = false; const b2 = false; const d = ""; const s = "hello"; const d2 = ""; const t = "Ann"; const h = true; const c = 0; const e = [null,false]; const f = {"x":null}; const x = "\u003c/script>\u003cb>";</script>"#
    );
    // The script's own text, and HTML made with raw(), are as they were.
    assert_eq!(run("str(<script>let q = \"text\"; if (a < b) go()</script>)"), "<script>let q = \"text\"; if (a < b) go()</script>");
    assert_eq!(run("str(<script>{raw(\"let r = 1;\")}</script>)"), "<script>let r = 1;</script>");
    // A string beside its own text, and a template with values.
    assert_eq!(run("let n = 3\nstr(<script>go({\"a\"}, {`n${n}`});</script>)"), "<script>go(\"a\", \"n3\");</script>");
    // Outside a script, a string in braces is text, as before.
    assert_eq!(run("str(<p>{\"hello\"} {null}</p>)"), "<p>hello </p>");
}

// Q-684: `image.convert` wrote JPEG whatever the target's name.
#[test]
fn image_convert_writes_the_format_its_target_names() {
    let d = dir("image");
    // A 30x20 PNG, red and half see-through.
    let png = {
        let raw: Vec<u8> = (0..20).flat_map(|_| std::iter::once(0u8).chain((0..30).flat_map(|_| [255u8, 0, 0, 128]))).collect();
        let chunk = |t: &[u8], data: &[u8]| {
            let mut c = (data.len() as u32).to_be_bytes().to_vec();
            c.extend_from_slice(t);
            c.extend_from_slice(data);
            let mut h = flate2::Crc::new();
            h.update(t);
            h.update(data);
            c.extend_from_slice(&h.sum().to_be_bytes());
            c
        };
        let mut ihdr = 30u32.to_be_bytes().to_vec();
        ihdr.extend_from_slice(&20u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut z, &raw).unwrap();
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        out.extend(chunk(b"IHDR", &ihdr));
        out.extend(chunk(b"IDAT", &z.finish().unwrap()));
        out.extend(chunk(b"IEND", b""));
        out
    };
    std::fs::write(d.join("in.png"), png).unwrap();
    let ev = |src: &str| run_in(&d, src);
    assert_eq!(ev("image.convert(\"in.png\", \"out.png\", { width: 20 })\nimage.info(\"out.png\")"), "{ width: 20, height: 13, format: \"png\", alpha: true }");
    assert_eq!(ev("image.convert(\"in.png\", \"out.WEBP\", { width: 20 })\nimage.info(\"out.WEBP\").format"), "webp");
    assert_eq!(ev("image.convert(\"in.png\", \"out.jpeg\")\nimage.info(\"out.jpeg\")"), "{ width: 30, height: 20, format: \"jpeg\", alpha: false }");
    // A format said wins over the name.
    assert_eq!(ev("image.convert(\"in.png\", \"photo.png\", { format: \"jpeg\" })\nimage.info(\"photo.png\").format"), "jpeg");
    assert_eq!(ev("image.convert(\"in.png\", \"blob.bin\", { format: \"png\" })\nimage.info(\"blob.bin\").format"), "png");
    // A name that says no format it writes, and none said: an error value, nothing written.
    for name in ["out.gif", "noext", "out.png.bak"] {
        let r = ev(&format!("image.convert(\"in.png\", \"{}\")", name));
        assert_eq!(r, format!("err(\"image: {}: its name says no format to write: end it in .jpg, .png or .webp, or give format: \\\"jpeg\\\", \\\"png\\\" or \\\"webp\\\"\")", name));
        assert!(!d.join(name).exists(), "{}", name);
    }
}

// Q-008, seen again: `\s` in a string, as a regular expression has it, was told that a newer
// Quillo might know it.
#[test]
fn a_regular_expressions_escape_says_what_to_write() {
    for (c, src) in [('s', "let r = `a\\sb`"), ('d', "let r = \"\\d+\""), ('w', "let r = \"\\w\""), ('.', "let r = \"\\.\""), ('b', "let r = \"\\b\"")] {
        let e = run(src);
        assert!(e.contains(&format!("unknown escape `\\{0}` in string: a regular expression's `\\{0}` is written `\\\\{0}` in a Quillo string", c)), "{}", e);
    }
    assert!(!quillo::error::may_be_newer_syntax("unknown escape `\\s` in string: a regular expression's `\\s` is written `\\\\s` in a Quillo string"));
    // Another unknown escape may still be a newer Quillo's.
    assert!(quillo::error::may_be_newer_syntax("unknown escape `\\q` in string"));
    // Written so, it is the regular expression's.
    assert_eq!(run("re.replace(\"a  b\", \"\\\\s+\", \"-\")"), "a-b");
}

// Q-029, seen again: a `` sql`...` `` constant, then `+ "where ..."`, warned for good.
#[test]
fn sql_written_in_a_tag_is_sql_the_program_wrote() {
    let src = "let BASE = sql`select * from t `\nfn f(id) = db.query(BASE + \"where id = ?\", id)\nfn h(id) {\n    let b = sql`select 1 `\n    db.query(b + \"where id = ?\", id)\n}\nfn j() = db.query(sql`select 1` + \" limit 1\")\n";
    assert_eq!(diags(src), Vec::<String>::new());
    // A value in it, or added to it, is still SQL built from values.
    let w = "`db.query` with SQL built from values";
    assert_eq!(diags("fn bad(name) {\n    let b = sql`select * from t where name = '${name}' `\n    db.query(b + \"and id = 1\")\n}\n").iter().filter(|d| d.contains(w)).count(), 1);
    assert_eq!(diags("let BASE = sql`select * from t `\nfn bad2(name) = db.query(BASE + \"where name = '\" + name + \"'\")\n").iter().filter(|d| d.contains(w)).count(), 1);
    // The program's own function named `sql` is not the tag.
    assert_eq!(diags("fn g(x) {\n    let sql = (s) => s + x\n    db.query(sql(\"select \") + \"1\")\n}\n").iter().filter(|d| d.contains(w)).count(), 1);
}
