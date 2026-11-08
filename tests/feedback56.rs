//! FEEDBACK56.md, slice 1 of the built-in MCP work (branch feedback56): what a function says
//! about itself in the document Quillo writes (`quillo docs`, `/_quillo/describe`). Its `///`
//! comment (Q-854), the program's records under `components/schemas` (Q-855), the fields a map
//! type requires (Q-856), its `with` (Q-857), and `list[str]` in a map or record type (Q-861).
//! Each test makes the case, and the edges either side of it.

mod support;

use quillo::ast::Stmt;
use quillo::describe::{contract, of_interp, of_source};
use quillo::Interp;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

fn dir(name: &str, src: &str) -> PathBuf {
    let d = support::temp_dir(&format!("feedback56-{}", name));
    std::fs::write(d.join("main.qwe"), src).unwrap();
    d
}

/// The document of a program, read from its source.
fn doc_of(name: &str, src: &str) -> Value {
    let d = dir(name, src);
    let doc = of_source(&d.join("main.qwe"));
    let _ = std::fs::remove_dir_all(&d);
    doc.unwrap_or_else(|e| panic!("{}", e.msg))
}

/// Each function of a program as parsed: its name and its doc comment.
fn docs(src: &str) -> Vec<(String, Option<String>)> {
    let stmts = quillo::parser::Parser::new(src).parse_program().unwrap_or_else(|e| panic!("{}: {}", e.line, e.msg));
    stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::Fn(d) => Some((d.name.clone(), d.doc.clone())),
            _ => None,
        })
        .collect()
}

fn doc_of_fn(src: &str, name: &str) -> Option<String> {
    docs(src).into_iter().find(|(n, _)| n == name).unwrap_or_else(|| panic!("no function {} in {}", name, src)).1
}

/// What the parser says of a program it refuses, and on which line.
fn refused(src: &str) -> (u32, String) {
    match quillo::parser::Parser::new(src).parse_program() {
        Ok(_) => panic!("parsed: {}", src),
        Err(e) => (e.line, e.msg),
    }
}

/// `quillo <args>` in `d`, under a deadline: whether it passed, its output and its errors.
fn quillo(d: &Path, args: &[&str]) -> (bool, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(args)
        .current_dir(d)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("quillo {:?} did not end", args);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let o = child.wait_with_output().unwrap();
    (o.status.success(), String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned())
}

/// Every `$ref` in a document, in order.
fn refs(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(m) => {
            if let Some(Value::String(r)) = m.get("$ref") {
                out.push(r.clone());
            }
            m.values().for_each(|x| refs(x, out));
        }
        Value::Array(a) => a.iter().for_each(|x| refs(x, out)),
        _ => {}
    }
}

/// Every `$ref` names a schema of the document's own, every schema is named by one, and every
/// key is one OpenAPI takes (`^[a-zA-Z0-9.\-_]+$`). Nothing beside a `$ref`, which OpenAPI 3.0
/// would not read.
fn refs_resolve(doc: &Value) {
    let mut all = Vec::new();
    refs(doc, &mut all);
    let schemas = doc["components"]["schemas"].as_object().cloned().unwrap_or_default();
    for r in &all {
        let key = r.strip_prefix("#/components/schemas/").unwrap_or_else(|| panic!("not a local $ref: {}", r));
        assert!(schemas.contains_key(key), "{} names no schema: {}", r, doc);
    }
    for k in schemas.keys() {
        assert!(!k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || ".-_".contains(c)), "key {:?}", k);
        assert!(all.iter().any(|r| r.ends_with(&format!("/{}", k))), "{} is written but not named", k);
    }
    fn alone(v: &Value) {
        match v {
            Value::Object(m) => {
                assert!(!m.contains_key("$ref") || m.len() == 1, "something beside a $ref: {}", v);
                m.values().for_each(alone);
            }
            Value::Array(a) => a.iter().for_each(alone),
            _ => {}
        }
    }
    alone(doc);
}

fn schema<'a>(doc: &'a Value, key: &str) -> &'a Value {
    let s = &doc["components"]["schemas"][key];
    assert!(s.is_object(), "no schema {}: {}", key, doc);
    s
}

fn r(key: &str) -> Value {
    json!({ "$ref": format!("#/components/schemas/{}", key) })
}

// ---- Q-854: the `///` comment ------------------------------------------------------------------

#[test]
fn q854_a_doc_comment_is_kept_with_its_function() {
    // The report's: one line, above a server fn with a `with`.
    let src = "/// Adds a note and gives back all notes.\nserver fn add_note(text: str) -> list ! str with db { [] }\n";
    assert_eq!(doc_of_fn(src, "add_note").as_deref(), Some("Adds a note and gives back all notes."));
    // On the file's first line, and several lines: each its own, an empty one between two
    // paragraphs kept, the empty ones at either end left out.
    let src = "///\n/// First.\n///\n/// Second, `code`.\n///\nfn f() = 1\n";
    assert_eq!(doc_of_fn(src, "f").as_deref(), Some("First.\n\nSecond, `code`."));
    // One space after `///` is the comment's; more is its text, as for a block of code.
    assert_eq!(doc_of_fn("///x\nfn f() = 1\n", "f").as_deref(), Some("x"));
    assert_eq!(doc_of_fn("///   indented  \nfn f() = 1\n", "f").as_deref(), Some("  indented"));
    // Unicode, and a file from Windows.
    assert_eq!(doc_of_fn("/// Café ☕, 日本語.\nfn f() = 1\n", "f").as_deref(), Some("Café ☕, 日本語."));
    assert_eq!(doc_of_fn("/// One.\r\n/// Two.\r\nfn f() = 1\r\n", "f").as_deref(), Some("One.\nTwo."));
    // Indented, as in a block; every kind of function.
    assert_eq!(doc_of_fn("  /// Inside.\n  fn f() = 1\n", "f").as_deref(), Some("Inside."));
    let src = "/// A card.\nview Card(t) = <p>{t}</p>\n/// A counter.\ncomponent Counter() {\n  state n = 0\n  <b>{n}</b>\n}\n/// Plain.\nfn plain() = 1\n";
    assert_eq!(docs(src), [("Card".to_string(), Some("A card.".to_string())), ("Counter".into(), Some("A counter.".into())), ("plain".into(), Some("Plain.".into()))]);
    let src = "/// Mine only.\nprivate fn helper() = 1\nfn other() = helper()\n";
    assert!(docs(src).iter().any(|(_, d)| d.as_deref() == Some("Mine only.")), "{:?}", docs(src));
    // None at all, and an empty one.
    assert_eq!(doc_of_fn("fn f() = 1\n", "f"), None);
    assert_eq!(doc_of_fn("///\n///\nfn f() = 1\n", "f"), None);
}

#[test]
fn q854_only_the_lines_right_above_and_only_three_slashes() {
    // A `//` line among them is passed over: the hover shows it, the program does not.
    let src = "/// Kept.\n// a line put aside\n/// Also kept.\nfn f() = 1\n";
    assert_eq!(doc_of_fn(src, "f").as_deref(), Some("Kept.\nAlso kept."));
    assert_eq!(doc_of_fn("// a plain comment\nfn f() = 1\n", "f"), None);
    // `////` is a line of slashes, as in Rust.
    assert_eq!(doc_of_fn("////////////\nfn f() = 1\n", "f"), None);
    assert_eq!(doc_of_fn("//// not a doc\n/// A doc.\nfn f() = 1\n", "f").as_deref(), Some("A doc."));
    // A blank line, or code, ends it.
    assert_eq!(doc_of_fn("/// Detached.\n\nfn f() = 1\n", "f"), None);
    let src = "/// For a.\nfn a() = 1\nfn b() = 2\n";
    assert_eq!(docs(src), [("a".to_string(), Some("For a.".to_string())), ("b".into(), None)]);
    let src = "/// For a.\nfn a() = 1\n/// For b.\nfn b() = 2\n";
    assert_eq!(docs(src), [("a".to_string(), Some("For a.".to_string())), ("b".into(), Some("For b.".into()))]);
}

#[test]
fn q854_a_doc_in_a_string_or_after_code_is_not_one() {
    // After code on its line, a `///` is that line's comment.
    assert_eq!(doc_of_fn("let x = 1 /// about x\nfn f() = x\n", "f"), None);
    // Text in a string that looks like one is text.
    assert_eq!(doc_of_fn("let s = `\n/// text`\nfn f() = s\n", "f"), None);
    assert_eq!(doc_of_fn("let s = \"\n/// text\n\"\nfn f() = s\n", "f"), None);
    // In a block comment.
    assert_eq!(doc_of_fn("/*\n/// inside\n*/\nfn f() = 1\n", "f"), None);
}

#[test]
fn q854_the_document_says_what_a_function_does() {
    let src = "/// Adds a note.\n///\n/// Gives back all of them.\nserver fn add_note(text: str) -> list = [text]\nserver fn plain() = 1\n";
    let doc = doc_of("q854-doc", src);
    let fns = &doc["x-quillo-server-fns"];
    assert_eq!(fns["add_note"]["description"], "Adds a note.\n\nGives back all of them.");
    // Without one, no description: the entry is as it was.
    assert_eq!(fns["plain"], json!({ "contract": "plain()", "params": [], "call": "POST /_quillo/rpc/plain" }));
    // The running program's document says the same, and `quillo docs` prints it.
    let d = dir("q854-cli", src);
    let mut it = Interp::new(d.to_str().unwrap());
    it.load_source(src, "main.qwe").unwrap();
    assert_eq!(of_interp(&it, "main")["x-quillo-server-fns"], doc["x-quillo-server-fns"]);
    let (ok, out, err) = quillo(&d, &["docs", "main.qwe"]);
    assert!(ok, "{}", err);
    let printed: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(printed["x-quillo-server-fns"]["add_note"]["description"], "Adds a note.\n\nGives back all of them.");
    let _ = std::fs::remove_dir_all(&d);
}

// ---- Q-855: records under components/schemas ----------------------------------------------------

#[test]
fn q855_a_record_is_written_once_and_referred_to() {
    let src = "type Note = { id: int, text: str, role: \"admin\" | \"member\" = \"member\", tags: list<str> = [] }\n\
               server fn save(note: Note) -> Note = note\nserver fn all() -> list<Note> = []\nroute POST /notes { json: Note } = json\n";
    let doc = doc_of("q855-once", src);
    refs_resolve(&doc);
    let fns = &doc["x-quillo-server-fns"];
    assert_eq!(fns["save"]["params"][0]["schema"], r("Note"));
    assert_eq!(fns["save"]["returns"], r("Note"));
    assert_eq!(fns["all"]["returns"], json!({ "type": "array", "items": r("Note") }));
    assert_eq!(doc["paths"]["/notes"]["post"]["requestBody"]["content"]["application/json"]["schema"], r("Note"));
    assert_eq!(doc["components"]["schemas"].as_object().unwrap().len(), 1);
    assert_eq!(
        schema(&doc, "Note"),
        &json!({
            "type": "object",
            "properties": {
                "id": { "type": "integer" },
                "text": { "type": "string" },
                "role": { "type": "string", "enum": ["admin", "member"], "default": "member" },
                "tags": { "type": "array", "items": { "type": "string" }, "default": [] }
            },
            "required": ["id", "text"]
        })
    );
}

#[test]
fn q855_a_record_in_a_list_in_a_record() {
    let src = "type Tag = { name: str, color: str? }\ntype Note = { text: str, labels: list<Tag> = [], pinned: list<list<Tag>>? }\n\
               type Shelf = { notes: list<Note> }\nserver fn put(s: Shelf) = 1\n";
    let doc = doc_of("q855-nested", src);
    refs_resolve(&doc);
    assert_eq!(doc["x-quillo-server-fns"]["put"]["params"][0]["schema"], r("Shelf"));
    assert_eq!(schema(&doc, "Shelf")["properties"]["notes"], json!({ "type": "array", "items": r("Note") }));
    let note = schema(&doc, "Note");
    assert_eq!(note["properties"]["labels"], json!({ "type": "array", "items": r("Tag"), "default": [] }));
    assert_eq!(note["properties"]["pinned"], json!({ "type": "array", "items": { "type": "array", "items": r("Tag") }, "nullable": true }));
    assert_eq!(note["required"], json!(["text"]));
    assert_eq!(schema(&doc, "Tag"), &json!({ "type": "object", "properties": { "name": { "type": "string" }, "color": { "type": "string", "nullable": true } }, "required": ["name"] }));
}

#[test]
fn q855_a_record_that_names_itself_ends() {
    // Itself, through a list; two that name each other; an alias that names itself.
    let src = "type Tree = { value: int, kids: list<Tree> = [] }\ntype Ping = { pong: Pong? }\ntype Pong = { ping: Ping? }\n\
               type Json = int | str | list<Json>\nserver fn grow(t: Tree, a: Ping, j: Json) -> Tree? = t\n";
    let doc = doc_of("q855-self", src);
    refs_resolve(&doc);
    assert_eq!(schema(&doc, "Tree")["properties"]["kids"], json!({ "type": "array", "items": r("Tree"), "default": [] }));
    assert_eq!(schema(&doc, "Ping")["properties"]["pong"], json!({ "allOf": [r("Pong")], "type": "object", "nullable": true }));
    assert_eq!(schema(&doc, "Pong")["properties"]["ping"], json!({ "allOf": [r("Ping")], "type": "object", "nullable": true }));
    // Three JSON types: a value is one of them only.
    assert_eq!(schema(&doc, "Json"), &json!({ "oneOf": [{ "type": "integer" }, { "type": "string" }, { "type": "array", "items": r("Json") }] }));
    // A null-or-record result says it is an object or null.
    assert_eq!(doc["x-quillo-server-fns"]["grow"]["returns"], json!({ "allOf": [r("Tree")], "type": "object", "nullable": true }));
    // The same from the running program.
    let d = dir("q855-self-live", src);
    let mut it = Interp::new(d.to_str().unwrap());
    it.load_source(src, "main.qwe").unwrap();
    assert_eq!(of_interp(&it, "main")["components"], doc["components"]);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn q855_a_union_of_records_and_the_unions_around_it() {
    let src = "type Circle = { r: float }\ntype Square = { side: float }\ntype Id = int | str\n\
               server fn area(s: Circle | Square) = 1\nserver fn either(s: Circle | str) = 1\nserver fn maybe(s: Circle | Square | null) = 1\n\
               server fn many(s: list<Circle | Square>) = 1\nserver fn plain(n: int | str, x: int | float, i: Id, a: any | int) = 1\n";
    let doc = doc_of("q855-union", src);
    refs_resolve(&doc);
    let p = |f: &str, i: usize| doc["x-quillo-server-fns"][f]["params"][i]["schema"].clone();
    // A value may fit two records (neither says no to the other's fields): `anyOf`, as the call
    // takes the first it fits.
    assert_eq!(p("area", 0), json!({ "anyOf": [r("Circle"), r("Square")] }));
    assert_eq!(p("maybe", 0), json!({ "anyOf": [r("Circle"), r("Square")], "nullable": true }));
    assert_eq!(p("many", 0), json!({ "type": "array", "items": { "anyOf": [r("Circle"), r("Square")] } }));
    // A record or a text cannot be both: `oneOf`, as `int | str` always was.
    assert_eq!(p("either", 0), json!({ "oneOf": [r("Circle"), { "type": "string" }] }));
    assert_eq!(p("plain", 0), json!({ "oneOf": [{ "type": "integer" }, { "type": "string" }] }));
    // A whole number is a float too, an alias may be anything, `any` is everything.
    assert_eq!(p("plain", 1), json!({ "anyOf": [{ "type": "integer" }, { "type": "number" }] }));
    assert_eq!(p("plain", 2), r("Id"));
    assert_eq!(schema(&doc, "Id"), &json!({ "oneOf": [{ "type": "integer" }, { "type": "string" }] }));
    assert_eq!(p("plain", 3), json!({ "anyOf": [{}, { "type": "integer" }] }));
}

#[test]
fn q855_texts_are_an_enum_and_defaults_are_said() {
    let src = "type Address = { city: str, zip: str? }\n\
               type Pet = { kind: \"cat\" | \"dog\" | int, one: \"only\", name: \"Café\" | \"日本\" = \"Café\", n: int = -3, w: float = 2.5, d: dec = 1.50m,\n\
               ok: bool = false, t: str = `plain`, at: list<int> = [1, -2], m: map = { a: 1, b: [true] }, home: Address = { city: \"Paris\" },\n\
               made: str = uuid(), from: str = `x${1}`, big: int = -9223372036854775807 }\n\
               server fn adopt(p: Pet, role: \"admin\" | \"member\") = 1\n";
    let doc = doc_of("q855-enum", src);
    refs_resolve(&doc);
    assert_eq!(doc["x-quillo-server-fns"]["adopt"]["params"][1]["schema"], json!({ "type": "string", "enum": ["admin", "member"] }));
    let pet = schema(&doc, "Pet");
    let f = |k: &str| pet["properties"][k].clone();
    assert_eq!(f("kind"), json!({ "oneOf": [{ "type": "string", "enum": ["cat", "dog"] }, { "type": "integer" }] }));
    assert_eq!(f("one"), json!({ "type": "string", "enum": ["only"] }));
    assert_eq!(f("name"), json!({ "type": "string", "enum": ["Café", "日本"], "default": "Café" }));
    assert_eq!(f("n")["default"], -3);
    assert_eq!(f("big")["default"], -9223372036854775807i64);
    assert_eq!(f("w")["default"], 2.5);
    assert_eq!(f("d"), json!({ "type": "string", "format": "decimal", "default": "1.50" }));
    assert_eq!(f("ok")["default"], false);
    assert_eq!(f("t")["default"], "plain");
    assert_eq!(f("at")["default"], json!([1, -2]));
    assert_eq!(f("m")["default"], json!({ "a": 1, "b": [true] }));
    // Beside a `$ref`, in an `allOf`.
    assert_eq!(f("home"), json!({ "allOf": [r("Address")], "type": "object", "default": { "city": "Paris" } }));
    // Worked out when the record is made: not known here, and still not required.
    assert_eq!(f("made"), json!({ "type": "string" }));
    assert_eq!(f("from"), json!({ "type": "string" }));
    assert_eq!(pet["required"], json!(["kind", "one"]));
}

#[test]
fn q855_each_programs_types_are_its_own_and_live_is_the_same() {
    let d = support::temp_dir("feedback56-q855-mount");
    std::fs::create_dir_all(d.join("shop")).unwrap();
    std::fs::write(d.join("shop/app.qwe"), "type Item = { sku: str, qty: int = 1 }\nserver fn add(i: Item) -> list<Item> = [i]\n").unwrap();
    std::fs::write(d.join("billing.qwe"), "type Item = { cents: int }\nroute POST /bill { json: Item } = 1\n").unwrap();
    std::fs::write(d.join("types.qwe"), "type Item = { name: str }\n").unwrap();
    let src = "use types.qwe\nmount /shop = \"shop/app.qwe\"\nservice /billing = \"billing.qwe\"\nserver fn take(i: Item) = 1\n";
    std::fs::write(d.join("main.qwe"), src).unwrap();
    let doc = of_source(&d.join("main.qwe")).unwrap();
    refs_resolve(&doc);
    // The program's own from the file it uses; a mount's and a service's after their prefix,
    // with the name they have there.
    assert_eq!(doc["x-quillo-server-fns"]["take"]["params"][0]["schema"], r("Item"));
    assert_eq!(schema(&doc, "Item"), &json!({ "type": "object", "properties": { "name": { "type": "string" } }, "required": ["name"] }));
    assert_eq!(doc["x-quillo-server-fns"]["/shop add"]["params"][0]["schema"], r("shop.Item"));
    assert_eq!(schema(&doc, "shop.Item")["title"], "Item");
    assert_eq!(schema(&doc, "shop.Item")["required"], json!(["sku"]));
    assert_eq!(doc["paths"]["/billing/bill"]["post"]["requestBody"]["content"]["application/json"]["schema"], r("billing.Item"));
    assert_eq!(schema(&doc, "billing.Item")["properties"], json!({ "cents": { "type": "integer" } }));
    // The running program's document is the same.
    let mut it = Interp::new(d.to_str().unwrap());
    it.load_source(src, "main.qwe").unwrap();
    let live = of_interp(&it, "main");
    assert_eq!(live["components"], doc["components"]);
    assert_eq!(live["paths"], doc["paths"]);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn q855_review_a_file_the_program_and_a_mount_use_is_read_for_each() {
    // The usual layout: one models file, used by the program and by what it mounts. It was read
    // once, for whichever came first, and the other's functions named a bare `Item`.
    for mount_first in [false, true] {
        let d = support::temp_dir(&format!("feedback56-q855-shared-{}", mount_first));
        std::fs::write(d.join("models.qwe"), "type Item = { name: str, qty: int = 1 }\nroute GET /models = 1\nserver fn count() -> int = 0\n").unwrap();
        std::fs::write(d.join("shop.qwe"), "use models.qwe\nserver fn add(i: Item) -> list<Item> = [i]\n").unwrap();
        std::fs::write(d.join("other.qwe"), "use models.qwe\nserver fn keep(i: Item) = 1\n").unwrap();
        let lines = ["use models.qwe", "mount /shop = \"shop.qwe\"\nmount /other = \"other.qwe\""];
        let (a, b) = if mount_first { (lines[1], lines[0]) } else { (lines[0], lines[1]) };
        let src = format!("{}\n{}\nserver fn take(i: Item) = 1\n", a, b);
        std::fs::write(d.join("main.qwe"), &src).unwrap();
        let doc = of_source(&d.join("main.qwe")).unwrap();
        refs_resolve(&doc);
        let fns = &doc["x-quillo-server-fns"];
        assert_eq!(fns["take"]["params"][0]["schema"], r("Item"), "mount first: {}", mount_first);
        assert_eq!(fns["/shop add"]["params"][0]["schema"], r("shop.Item"), "mount first: {}", mount_first);
        assert_eq!(fns["/other keep"]["params"][0]["schema"], r("other.Item"), "mount first: {}", mount_first);
        assert_eq!(schema(&doc, "shop.Item")["required"], json!(["name"]));
        // Its routes and functions are each program's too, as the running server serves them.
        assert!(doc["paths"]["/shop/models"]["get"].is_object() && doc["paths"]["/models"]["get"].is_object(), "{}", doc["paths"]);
        assert!(fns["/shop count"].is_object() && fns["count"].is_object());
        let mut it = Interp::new(d.to_str().unwrap());
        it.load_source(&src, "main.qwe").unwrap();
        let live = of_interp(&it, "main");
        assert_eq!(live["components"], doc["components"], "mount first: {}", mount_first);
        assert_eq!(live["x-quillo-server-fns"], doc["x-quillo-server-fns"], "mount first: {}", mount_first);
        assert_eq!(live["paths"], doc["paths"], "mount first: {}", mount_first);
        let _ = std::fs::remove_dir_all(&d);
    }
    // A program that mounts itself, or what mounts it, is not read again: the document ends.
    let d = support::temp_dir("feedback56-q855-self-mount");
    std::fs::write(d.join("main.qwe"), "type Item = { a: int }\nmount /again = \"main.qwe\"\nmount /shop = \"shop.qwe\"\nserver fn f(i: Item) = 1\n").unwrap();
    std::fs::write(d.join("shop.qwe"), "mount /back = \"main.qwe\"\nroute GET /s = 1\n").unwrap();
    let doc = of_source(&d.join("main.qwe")).unwrap();
    let mut paths: Vec<&String> = doc["paths"].as_object().unwrap().keys().collect();
    paths.sort();
    assert_eq!(paths, ["/shop/s"]);
    assert_eq!(doc["x-quillo-server-fns"].as_object().unwrap().keys().collect::<Vec<_>>(), ["f"]);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn q855_a_name_a_key_cannot_hold() {
    // A key takes `A-Z a-z 0-9 . - _`: another letter becomes `_`, its name is the title, and two
    // that come out the same are told apart in the order of their names (`Caf_` < `Cafè` <
    // `Café`), not of the parameters that name them (found in review).
    let src = "type Café = { crème: bool = true }\ntype Cafè = { n: int }\ntype Caf_ = { s: str }\nserver fn f(a: Café, b: Cafè, c: Caf_) = 1\n";
    let doc = doc_of("q855-unicode", src);
    refs_resolve(&doc);
    let p = |i: usize| doc["x-quillo-server-fns"]["f"]["params"][i]["schema"].clone();
    assert_eq!(p(2), r("Caf_"));
    assert!(schema(&doc, "Caf_").get("title").is_none(), "its own name: {}", schema(&doc, "Caf_"));
    assert_eq!(p(1), r("Caf_-2"));
    assert_eq!(schema(&doc, "Caf_-2")["title"], "Cafè");
    assert_eq!(p(0), r("Caf_-3"));
    assert_eq!(schema(&doc, "Caf_-3"), &json!({ "type": "object", "properties": { "crème": { "type": "boolean", "default": true } }, "title": "Café" }));
}

#[test]
fn q855_review_keys_do_not_depend_on_which_function_names_a_type_first() {
    // Listed in the file's order by `quillo docs` and by name by the running program: the keys
    // were given in each order, so `Café` was `Caf_` in one and `Caf_-2` in the other.
    let src = "type Café = { a: int }\ntype Cafè = { b: int }\nserver fn zeta(x: Cafè) = 1\nserver fn alpha(y: Café) = 1\n";
    let doc = doc_of("q855-order", src);
    refs_resolve(&doc);
    let d = dir("q855-order-live", src);
    let mut it = Interp::new(d.to_str().unwrap());
    it.load_source(src, "main.qwe").unwrap();
    let live = of_interp(&it, "main");
    let _ = std::fs::remove_dir_all(&d);
    assert_eq!(live["components"], doc["components"]);
    assert_eq!(live["x-quillo-server-fns"], doc["x-quillo-server-fns"]);
    assert_eq!(doc["x-quillo-server-fns"]["zeta"]["params"][0]["schema"], r("Caf_"));
    assert_eq!(doc["x-quillo-server-fns"]["alpha"]["params"][0]["schema"], r("Caf_-2"));
    // A function added that names one first changes no key; one not named leaves its key unused.
    let more = doc_of("q855-order-more", &format!("server fn first(z: Café) = 1\n{}", src));
    assert_eq!(more["components"], doc["components"]);
    let one = doc_of("q855-order-one", "type Café = { a: int }\ntype Cafè = { b: int }\nserver fn alpha(y: Café) = 1\n");
    assert_eq!(one["x-quillo-server-fns"]["alpha"]["params"][0]["schema"], r("Caf_-2"));
    assert_eq!(one["components"]["schemas"].as_object().unwrap().len(), 1);
}

#[test]
fn q855_no_named_type_no_components_and_an_unknown_one_as_before() {
    let doc = doc_of("q855-none", "type Unused = { a: int }\nserver fn f(n: int) -> str = \"\"\nroute GET /x = 1\n");
    assert!(doc.get("components").is_none(), "{}", doc);
    // A name the program does not define is said as it is written.
    let doc = doc_of("q855-unknown", "server fn f(g: Ghost) -> Ghost? = g\n");
    assert!(doc.get("components").is_none(), "{}", doc);
    assert_eq!(doc["x-quillo-server-fns"]["f"]["params"][0]["schema"], json!({ "x-quillo-type": "Ghost" }));
    assert_eq!(doc["x-quillo-server-fns"]["f"]["returns"], json!({ "x-quillo-type": "Ghost", "nullable": true }));
}

#[test]
fn q855_review_a_routes_unions_are_any_of_where_a_body_could_fit_two() {
    // `{ r, side }` passes the door as a Circle, and fitted both alternatives of a `oneOf`.
    let src = "type Circle = { r: float }\ntype Square = { side: float }\n\
               route POST /s { json: Circle | Square } = 1\nroute POST /n { json: Circle | Square | null } = 1\n\
               route POST /t { json: Circle | str } = 1\nroute POST /k { json: { kind: \"cat\" | \"dog\", n: 1 | 2 } } = 1\n\
               route POST /m { json: { a: int } } = 1\nroute POST /m { json: { a: int, b: str } } = 2\n\
               route POST /o { json: { kind: \"refund\", id: int } } = 1\nroute POST /o { json: { kind: \"new\", item: str } } = 2\n\
               route POST /p { json: { kind: \"refund\" | \"new\" } } = 1\nroute POST /p { json: { kind: \"new\", item: str } } = 2\n";
    let doc = doc_of("q855-route-unions", src);
    refs_resolve(&doc);
    let body = |p: &str| doc["paths"][p]["post"]["requestBody"]["content"]["application/json"]["schema"].clone();
    assert_eq!(body("/s"), json!({ "anyOf": [r("Circle"), r("Square")] }));
    assert_eq!(body("/n"), json!({ "anyOf": [r("Circle"), r("Square")], "nullable": true }));
    // Several routes' bodies: the second `/m` body is the first's and more.
    assert!(body("/m")["anyOf"].as_array().is_some_and(|a| a.len() == 2), "{}", body("/m"));
    assert!(body("/p")["anyOf"].is_array(), "{}", body("/p"));
    // What no value can be twice stays `oneOf`: a record or a text, `"cat"` or `"dog"`, and
    // bodies told apart by a field each requires (`kind`).
    assert_eq!(body("/t"), json!({ "oneOf": [r("Circle"), { "type": "string" }] }));
    assert_eq!(body("/k")["properties"]["kind"], json!({ "oneOf": [{ "type": "string", "enum": ["cat"] }, { "type": "string", "enum": ["dog"] }] }));
    assert_eq!(body("/k")["properties"]["n"], json!({ "oneOf": [{ "type": "integer", "enum": [1] }, { "type": "integer", "enum": [2] }] }));
    assert!(body("/o")["oneOf"].as_array().is_some_and(|a| a.len() == 2), "{}", body("/o"));
}

// ---- Q-856: a map type's required fields ----------------------------------------------------------

#[test]
fn q856_a_map_type_says_which_fields_are_required() {
    let src = "server fn person(p: { name: str, age: int? }) -> { ok: bool } = { ok: true }\n\
               server fn edges(none: {}, one: { a: int }, nulls: { a: int | null, b: str? }, all: { a: int, b: any, c: list<str> }) = 1\n\
               server fn deep(p: { who: { first: str, middle: str? }, tags: list<{ k: str, v: str? }>, é: { ü: int } }) = 1\n";
    let doc = doc_of("q856", src);
    let p = |f: &str, i: usize| doc["x-quillo-server-fns"][f]["params"][i]["schema"].clone();
    assert_eq!(p("person", 0), json!({ "type": "object", "properties": { "name": { "type": "string" }, "age": { "type": "integer", "nullable": true } }, "required": ["name"] }));
    // A result's map type is not held to its fields: none is promised (found in review; see
    // q856_review_a_result_or_an_error_promises_no_field).
    assert!(doc["x-quillo-server-fns"]["person"]["returns"].get("required").is_none());
    // None, one, none required (OpenAPI 3.0 takes no empty `required`), all of them.
    assert_eq!(p("edges", 0), json!({ "type": "object", "properties": {} }));
    assert_eq!(p("edges", 1)["required"], json!(["a"]));
    assert!(p("edges", 2).get("required").is_none(), "{}", p("edges", 2));
    assert_eq!(p("edges", 3)["required"], json!(["a", "b", "c"]));
    // Inside, at each level, and in other letters.
    let deep = p("deep", 0);
    assert_eq!(deep["required"], json!(["who", "tags", "é"]));
    assert_eq!(deep["properties"]["who"]["required"], json!(["first"]));
    assert_eq!(deep["properties"]["tags"]["items"]["required"], json!(["k"]));
    assert_eq!(deep["properties"]["é"]["required"], json!(["ü"]));
}

#[test]
fn q856_the_door_refuses_exactly_what_is_required() {
    // For every field the document lists, a call without it is refused; for every other, taken.
    let src = "type Tag = { name: str, color: str? }\n\
               type Note = { id: int, text: str, role: \"admin\" | \"member\" = \"member\", tags: list<str> = [], labels: list<Tag> = [], note: str? }\n\
               server fn person(p: { name: str, age: int? }) = true\nserver fn save(n: Note) = true\nserver fn tag(t: Tag) = true\n";
    let d = dir("q856-door", src);
    let doc = of_source(&d.join("main.qwe")).unwrap();
    let s = support::serve(&d.join("main.qwe"), &[]);
    let json = [("content-type", "application/json")];
    let full = [
        ("person", doc["x-quillo-server-fns"]["person"]["params"][0]["schema"].clone(), json!({ "name": "Ada", "age": 36 })),
        ("save", schema(&doc, "Note").clone(), json!({ "id": 1, "text": "t", "role": "admin", "tags": ["x"], "labels": [], "note": "n" })),
        ("tag", schema(&doc, "Tag").clone(), json!({ "name": "n", "color": "c" })),
    ];
    for (f, schema, value) in full {
        let required: Vec<String> = schema["required"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
        let a = support::send(s.port, "POST", &format!("/_quillo/rpc/{}", f), &json, &json!([value]).to_string());
        assert_eq!(a.status, 200, "{}: {}", f, a.body);
        for k in schema["properties"].as_object().unwrap().keys() {
            let mut v = value.clone();
            v.as_object_mut().unwrap().remove(k);
            let a = support::send(s.port, "POST", &format!("/_quillo/rpc/{}", f), &json, &json!([v]).to_string());
            if required.contains(k) {
                assert_eq!(a.status, 400, "{} without {}: {}", f, k, a.body);
                assert!(a.body.contains(&format!("`{}` is missing", k)), "{} without {}: {}", f, k, a.body);
            } else {
                assert_eq!(a.status, 200, "{} without {}: {}", f, k, a.body);
            }
        }
    }
    drop(s);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn q856_review_a_result_or_an_error_promises_no_field() {
    // `-> { ok: bool }` is checked as a map, not field by field, so `{}` is returned: the
    // document said `ok` was required.
    let src = "type Note = { id: int, text: str? }\ntype Rows = list<{ x: int }>\n\
               server fn person() -> { ok: bool } = {}\nserver fn rows() -> list<{ x: int, y: int? }> ! { code: int } = [{}]\n\
               server fn note() -> Note = Note({ id: 1 })\nserver fn both(p: { ok: bool }, r: Rows) -> { ok: bool } = p\n";
    let doc = doc_of("q856-results", src);
    refs_resolve(&doc);
    let fns = &doc["x-quillo-server-fns"];
    assert_eq!(fns["person"]["returns"], json!({ "type": "object", "properties": { "ok": { "type": "boolean" } } }));
    assert_eq!(fns["rows"]["returns"], json!({ "type": "array", "items": { "type": "object", "properties": { "x": { "type": "integer" }, "y": { "type": "integer", "nullable": true } } } }));
    assert_eq!(fns["rows"]["error"], json!({ "type": "object", "properties": { "code": { "type": "integer" } } }));
    // A record is checked wherever it is named, and keeps its own; a parameter keeps its.
    assert_eq!(fns["note"]["returns"], r("Note"));
    assert_eq!(schema(&doc, "Note")["required"], json!(["id"]));
    assert_eq!(fns["both"]["params"][0]["schema"]["required"], json!(["ok"]));
    assert!(fns["both"]["returns"].get("required").is_none());
    // An alias is written once for every place that names it, a result too: its map types none.
    assert_eq!(fns["both"]["params"][1]["schema"], r("Rows"));
    assert_eq!(schema(&doc, "Rows"), &json!({ "type": "array", "items": { "type": "object", "properties": { "x": { "type": "integer" } } } }));
    // What the server does: the empty results are answered.
    let d = dir("q856-results-live", src);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let json = [("content-type", "application/json")];
    let a = support::send(s.port, "POST", "/_quillo/rpc/person", &json, "[]");
    assert_eq!((a.status, a.body.as_str()), (200, r#"{"ok":{}}"#));
    let a = support::send(s.port, "POST", "/_quillo/rpc/rows", &json, "[]");
    assert_eq!((a.status, a.body.as_str()), (200, r#"{"ok":[{}]}"#));
    drop(s);
    let _ = std::fs::remove_dir_all(&d);
}

// ---- Q-857: the `with` -------------------------------------------------------------------------------

#[test]
fn q857_the_with_ends_the_contract_and_is_listed() {
    let src = "server fn add_note(text: str) -> list ! str with db = []\nserver fn two() with time, db = 1\nserver fn twice() with db, db = 1\n\
               server fn all() with random, net, env, fs, http, time, db = 1\nserver fn free() = 1\nfn helper() with db = 1\n";
    let doc = doc_of("q857", src);
    let fns = &doc["x-quillo-server-fns"];
    assert_eq!(fns["add_note"]["contract"], "add_note(text: str) -> list ! str with db");
    assert_eq!(fns["add_note"]["x-quillo-effects"], json!(["db"]));
    // In one order, each once, whatever the order written.
    assert_eq!(fns["two"]["contract"], "two() with db, time");
    assert_eq!(fns["two"]["x-quillo-effects"], json!(["db", "time"]));
    assert_eq!(fns["twice"]["x-quillo-effects"], json!(["db"]));
    assert_eq!(fns["all"]["x-quillo-effects"], json!(["db", "http", "fs", "env", "time", "random", "net"]));
    assert_eq!(fns["all"]["contract"], "all() with db, http, fs, env, time, random, net");
    // Nothing declared: nothing said, as before; a plain fn is not there.
    assert_eq!(fns["free"]["contract"], "free()");
    assert!(fns["free"].get("x-quillo-effects").is_none());
    assert!(fns.get("helper").is_none());
    // The contract services compare stays as it was, so a caller's copy made before still agrees.
    let stmts = quillo::parser::Parser::new(src).parse_program().unwrap();
    let Stmt::Fn(d) = &stmts[0] else { panic!("not a function") };
    assert_eq!(contract(d), "add_note(text: str) -> list ! str");
}

// ---- Q-861: `list[str]` in a map or record type ------------------------------------------------------

#[test]
fn q861_brackets_in_a_map_or_record_type_name_the_fix() {
    let fix = |s: &str| format!("a list's item type goes in `<>`: `{}`", s);
    // The report's: a record's field, with the item type written and the `?` after it.
    assert_eq!(refused("type Note = { id: int, text: str, tags: list[str]? }\n"), (1, fix("tags: list<str>?")));
    assert_eq!(refused("type Note = { tags: list[str] = [] }\n"), (1, fix("tags: list<str>")));
    // A map type's, TypeScript's `str[]`, and nothing in the brackets.
    assert_eq!(refused("server fn p(m: { tags: list[str] }) = m\n"), (1, fix("tags: list<str>")));
    assert_eq!(refused("server fn p(m: { tags: str[] }) = m\n"), (1, fix("tags: list<str>")));
    assert_eq!(refused("server fn p(m: { tags: str[]? }) = m\n"), (1, fix("tags: list<str>?")));
    assert_eq!(refused("server fn p(m: { tags: list[] }) = m\n"), (1, fix("tags: list<int>")));
    // Inside, of the program's own, of a map, of a list, in other letters, on its line.
    assert_eq!(refused("server fn p(m: { a: { b: list[int] } }) = m\n"), (1, fix("b: list<int>")));
    assert_eq!(refused("type Tag = { n: str }\ntype Note = { labels: list[Tag] }\n"), (2, fix("labels: list<Tag>")));
    assert_eq!(refused("type Table = { rows: list[{ a: int }] }\n"), (1, fix("rows: list<{ a: int }>")));
    assert_eq!(refused("type Grid = { grid: list<int>[] }\n"), (1, fix("grid: list<list<int>>")));
    assert_eq!(refused("type Note = { étiquettes: list[str] }\n"), (1, fix("étiquettes: list<str>")));
    assert_eq!(refused("type Note = {\n  id: int,\n  tags: list[str]?,\n}\n"), (3, fix("tags: list<str>?")));
    // What must keep working: `<>`, and another slip's message.
    let ok = "type Note = { id: int, tags: list<str>? = null, rows: list<{ a: int }> = [] }\nserver fn p(m: { tags: list<str> }) = m\n";
    assert!(quillo::parser::Parser::new(ok).parse_program().is_ok());
    assert_eq!(refused("type Abc = { a: int b: str }\n"), (1, "expected `,` or `}` in a map type, found `b`".to_string()));
}

#[test]
fn q861_the_parameter_names_the_item_type_written() {
    let fix = |s: &str| format!("a list's item type goes in `<>`: `{}`", s);
    // It said `list<int>` whatever was written.
    assert_eq!(refused("server fn tags(t: list[str]) = t\n"), (1, fix("t: list<str>")));
    assert_eq!(refused("fn f(t: list[str]?) = t\n"), (1, fix("t: list<str>?")));
    assert_eq!(refused("fn f(t: list[Note]) = t\n"), (1, fix("t: list<Note>")));
    // As before.
    assert_eq!(refused("fn f(a: int[]) = a\n"), (1, fix("a: list<int>")));
    assert_eq!(refused("fn f(a: list[int]) = a\n"), (1, fix("a: list<int>")));
}

#[test]
fn q861_quillo_check_says_it() {
    let d = dir("q861-check", "type Note = { id: int, text: str, tags: list[str]? }\n");
    let (ok, out, err) = quillo(&d, &["check", "main.qwe"]);
    assert!(!ok);
    assert!(format!("{}{}", out, err).contains("main.qwe:1: error: a list's item type goes in `<>`: `tags: list<str>?`"), "{}{}", out, err);
    let _ = std::fs::remove_dir_all(&d);
}
