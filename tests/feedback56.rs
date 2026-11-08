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

// ---- Q-853, slice 2: the words `fn mcp`, `server fn mcp`, `service mcp` (Part A) ------------------

/// Each top-level function of a program as parsed: its name, whether it is a server fn, and
/// whether it is a tool.
fn fns(src: &str) -> Vec<(String, bool, bool)> {
    let stmts = quillo::parser::Parser::new(src).parse_program().unwrap_or_else(|e| panic!("{}: {}\n{}", e.line, e.msg, src));
    stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::Fn(d) => Some((d.name.clone(), d.server, d.mcp)),
            _ => None,
        })
        .collect()
}

fn tool(name: &str) -> (String, bool, bool) {
    (name.to_string(), false, true)
}

/// Each service a program declares, as parsed: its name, its path, its file, and its tag.
fn services(src: &str) -> Vec<(String, Option<String>, String, bool)> {
    let stmts = quillo::parser::Parser::new(src).parse_program().unwrap_or_else(|e| panic!("{}: {}\n{}", e.line, e.msg, src));
    stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::Service(n, p, f, _, _, mcp) => Some((n.clone(), p.clone(), f.clone(), *mcp)),
            _ => None,
        })
        .collect()
}

/// What `quillo check` says of a program, a line each: `3 error: ...`, `3 warning: ...`.
fn said(src: &str) -> Vec<String> {
    quillo::check::check_source(src, "t.qwe", Path::new("."))
        .diagnostics
        .iter()
        .map(|d| format!("{} {}: {}", d.line, if d.severity == quillo::check::Severity::Error { "error" } else { "warning" }, d.message))
        .collect()
}

#[track_caller]
fn says(src: &str, want: &[&str]) {
    assert_eq!(said(src), want, "\n--- source ---\n{}", src);
}

/// A tool with nothing to say of it, for the rules below: its `///`, a type, a `with`.
const CLEAN: &str = "/// The time now.\nfn mcp clock() with time = now()\n";

const NAME_RULE: &str = "a tool's name is 1 to 128 of A-Z, a-z, 0-9, `_`, `-` and `.`, as MCP allows";

#[test]
fn q853_the_tag_is_read_after_fn_and_server_fn() {
    assert_eq!(fns("fn mcp total() = 1\n"), [tool("total")]);
    assert_eq!(fns("server fn mcp add_note(text: str) -> list ! str with db = []\n"), [("add_note".to_string(), true, true)]);
    // `public` before it, as before any declaration; a tab; a body in braces; its `///` kept.
    assert_eq!(fns("public fn mcp a() = 1\npublic server fn mcp b() = 1\n"), [tool("a"), ("b".to_string(), true, true)]);
    assert_eq!(fns("fn mcp\tt() = 1\nfn  mcp  u() {\n    1\n}\n"), [tool("t"), tool("u")]);
    assert_eq!(doc_of_fn("/// What it does.\nfn mcp t() = 1\n", "t").as_deref(), Some("What it does."));
    // A tool named `mcp`, in both kinds.
    assert_eq!(fns("fn mcp mcp() = 1\n"), [tool("mcp")]);
    assert_eq!(fns("server fn mcp mcp(x: int) = x\n"), [("mcp".to_string(), true, true)]);
    // Any name the parser takes, which `quillo check` holds to MCP's (below).
    assert_eq!(fns("fn mcp café() = 1\nfn mcp _x() = 1\n"), [tool("café"), tool("_x")]);
    // `server fn for` stays a server fn's: `for [...]` after the tag's name.
    assert_eq!(fns("server fn mcp t() for [\"shop\"] = 1\n"), [("t".to_string(), true, true)]);
    // Not a keyword after the tag: said as a name is.
    assert_eq!(refused("fn mcp if() = 1\n"), (1, "expected a function name, found `if`, which is a keyword: pick another name".to_string()));
}

#[test]
fn q853_mcp_stays_a_name_everywhere_else() {
    // A function named `mcp`: `(` after it, with or without a space, and with no parameters.
    let plain = |n: &str, server: bool| (n.to_string(), server, false);
    assert_eq!(fns("fn mcp(x) = x + 1\n"), [plain("mcp", false)]);
    assert_eq!(fns("fn mcp (x) = x\nfn mcp() = 1\n"), [plain("mcp", false), plain("mcp", false)]);
    assert_eq!(fns("server fn mcp(x: int) = x\n"), [plain("mcp", true)]);
    // A private one, renamed as private names are.
    let private = fns("private fn mcp(x) = x\n");
    assert!(private.len() == 1 && private[0].0.starts_with("mcp__") && !private[0].1 && !private[0].2, "{:?}", private);
    // The report's probe runs as it did: `mcp(1)` is 2.
    let mut it = Interp::new(".");
    it.load_source("fn mcp(x) = x + 1\nlet mcp_of = { mcp: 3 }\nfn f(mcp) = mcp * 2\nlet mcp2 = mcp(1) + mcp_of.mcp + f(5)\n", "case.qwe").unwrap();
    assert_eq!(quillo::value::lookup(&it.globals, "mcp2").unwrap().repr(), "15");
    // A variable, a parameter, a key, a service named `mcp`, a name on the next line.
    assert!(fns("let mcp = 1\nfn f(mcp) = mcp\nlet m = { mcp: 1 }\nprint(m.mcp + mcp)\n").iter().all(|f| !f.2));
    assert_eq!(services("service mcp = \"mcp.qwe\"\n"), [("mcp".to_string(), None, "mcp.qwe".to_string(), false)]);
    assert!(refused("fn mcp\ntotal() = 1\n").1.contains("expected `(`"), "{:?}", refused("fn mcp\ntotal() = 1\n"));
    // A tool's own code calls it as any function.
    let mut it = Interp::new(".");
    it.load_source("/// Doubles.\nfn mcp double(n: int) -> int = n * 2\nlet four = double(2)\n", "case.qwe").unwrap();
    assert_eq!(quillo::value::lookup(&it.globals, "four").unwrap().repr(), "4");
}

#[test]
fn q853_service_mcp_is_read_with_a_path_or_a_name() {
    let s = |n: &str, p: Option<&str>, f: &str, mcp: bool| (n.to_string(), p.map(String::from), f.to_string(), mcp);
    assert_eq!(services("service mcp /billing = \"billing.qwe\"\n"), [s("billing", Some("/billing"), "billing.qwe", true)]);
    assert_eq!(services("service mcp crm = \"https://crm.example.com/mcp\"\n"), [s("crm", None, "https://crm.example.com/mcp", true)]);
    // A service named `mcp`, with the tag and without; the forms of today.
    assert_eq!(services("service mcp mcp = \"tools.qwe\"\n"), [s("mcp", None, "tools.qwe", true)]);
    assert_eq!(services("service /billing = \"billing.qwe\"\nservice billing2 = \"b.qwe\"\n"), [s("billing", Some("/billing"), "billing.qwe", false), s("billing2", None, "b.qwe", false)]);
    assert_eq!(services("service mcp\t/a/b = \"sub/b.qwe\"\n"), [s("b", Some("/a/b"), "sub/b.qwe", true)]);
    // How it is called, kept after the file, as for any service.
    let stmts = quillo::parser::Parser::new("service mcp /billing = \"billing.qwe\" { timeout: \"2s\" }\n").parse_program().unwrap();
    assert!(matches!(&stmts[0], Stmt::Service(_, _, _, Some(_), 1, true)));
    // A comment after it starts no path: a service named `mcp`, as before (found in review).
    assert_eq!(services("service mcp /* note */ = \"mcp.qwe\"\n"), [s("mcp", None, "mcp.qwe", false)]);
    assert_eq!(services("service mcp /b = \"b.qwe\"\nservice mcp /x/y = \"y.qwe\"\n"), [s("b", Some("/b"), "b.qwe", true), s("y", Some("/x/y"), "y.qwe", true)]);
    // `service` and `mcp` as names.
    assert!(services("let service = 1\nlet mcp = service + 1\n").is_empty());
}

#[test]
fn q853_a_tool_cannot_be_private() {
    let msg = "a `fn mcp` cannot be private: it is a tool, an address that anyone the door lets in can call; keep the work in a private `fn` it calls";
    assert_eq!(refused("private fn mcp t() = 1\n"), (1, msg.to_string()));
    assert_eq!(refused("let a = 1\n\nprivate fn mcp t() {\n    a\n}\n"), (3, msg.to_string()));
    // `private server fn mcp`: a server fn's own words, as before.
    assert_eq!(refused("private server fn mcp t() = 1\n").1, "a `server fn` cannot be private: it is an address on the server that anyone can call; keep the work in a private `fn` it calls");
    // A private function named `mcp`, and a public tool, are fine.
    assert!(quillo::parser::Parser::new("private fn mcp(x) = x\npublic fn mcp t() = 1\n").parse_program().is_ok());
}

#[test]
fn q853_a_tool_is_declared_at_the_top_level() {
    let msg = "error: `fn mcp` must be declared at the top level, as `server fn` is: a tool is an address of the program's";
    says("fn outer() {\n    fn mcp inner() = 1\n    inner()\n}\nprint(outer())\n", &[&format!("2 {}", msg)]);
    // In a component, a branch, a loop; two levels down.
    says("component C() {\n    fn mcp inner() = 1\n    <p>{inner()}</p>\n}\n", &[&format!("2 {}", msg)]);
    says("if true {\n    fn mcp t() = 1\n}\n", &[&format!("2 {}", msg)]);
    says("fn a() {\n    fn b() {\n        fn mcp c() = 1\n        c()\n    }\n    b()\n}\nprint(a())\n", &[&format!("3 {}", msg)]);
    // A nested `server fn mcp` is a nested server fn, said once.
    says("fn outer() {\n    server fn mcp inner() = 1\n    inner()\n}\nprint(outer())\n", &["2 error: `server fn` and `component` must be declared at the top level"]);
    // A function named `mcp` inside one is fine; so is the tool at the top level.
    says("fn outer() {\n    fn mcp(x) = x\n    mcp(1)\n}\nprint(outer())\n", &[]);
    says(CLEAN, &[]);
}

#[test]
fn q853_a_view_or_a_component_is_no_tool() {
    let not = |what: &str| format!("error: a {} is not a tool: `fn mcp` is for a function that answers with data, not a page", what);
    says("view mcp Card() = <p>x</p>\n", &[&format!("1 {}", not("view"))]);
    // A view whatever its name: `view mcp card()` passed (found in review); `view mcp(x)` is a
    // view named `mcp`.
    let view = "a view is not a tool: `fn mcp` is for a function that answers with data, not a page".to_string();
    assert_eq!(refused("view mcp card() = \"x\"\n"), (1, view.clone()));
    assert_eq!(refused("let a = 1\nview mcp card_2(n: int) {\n    \"x\"\n}\n"), (2, view));
    says("view mcp card() = \"x\"\n", &[&format!("1 {}", not("view"))]);
    assert!(quillo::parser::Parser::new("view mcp(x) = <p>{x}</p>\nview card() = \"x\"\n").parse_program().is_ok());
    says("/// A card.\nfn mcp Card() with time = <p>x</p>\n", &[&format!("2 {}", not("view"))]);
    says("component mcp Box() {\n    <p>x</p>\n}\n", &[&format!("1 {}", not("component"))]);
    // As before: a view and a component named so, and `component mcp(...)`'s own error.
    says("view Card() = <p>x</p>\ncomponent Box() {\n    <p>x</p>\n}\n", &[]);
    assert_eq!(refused("component mcp(x) {\n    <p/>\n}\n").1, "component names start with a capital letter: `mcp`");
}

#[test]
fn q853_a_tools_name_is_one_mcp_allows() {
    let t = |name: &str| format!("/// It.\nfn mcp {}() with time = now()\n", name);
    says(&t("café"), &[&format!("2 error: {}: `café` has `é`", NAME_RULE)]);
    says(&t("日本"), &[&format!("2 error: {}: `日本` has `日`", NAME_RULE)]);
    says(&t("naïve_ok"), &[&format!("2 error: {}: `naïve_ok` has `ï`", NAME_RULE)]);
    // 128 characters is the most; one more is refused; one is the least, and the shortest name.
    let long = |n: usize| format!("a{}", "b".repeat(n - 1));
    says(&t(&long(128)), &[]);
    says(&t(&long(127)), &[]);
    says(&t(&long(129)), &[&format!("2 error: {}: this one has 129 characters", NAME_RULE)]);
    says(&t("a"), &[]);
    says(&t("_"), &[]);
    says(&t("Get_2_items"), &[&format!("2 error: a view is not a tool: `fn mcp` is for a function that answers with data, not a page")]);
    says(&t("get_2_items"), &[]);
    // A plain function keeps any name Quillo takes.
    says("fn café() = 1\nprint(café())\n", &[]);
}

#[test]
fn q853_each_parameter_comes_as_json() {
    let json = |p: &str, t: &str| format!("2 error: parameter `{}` of tool `probe` is {}, which cannot come as JSON", p, t);
    says("/// It.\nfn mcp probe(f: fn) with time = now()\n", &[&json("f", "fn")]);
    says("/// It.\nfn mcp probe(a: int, page: html, r: response) with time = now()\n", &[&json("page", "html"), &json("r", "response")]);
    // A `server fn mcp`'s is said of the tool, once.
    says("/// It.\nserver fn mcp probe(f: fn) with time = now()\n", &[&json("f", "fn")]);
    // As before for a server fn; and what JSON carries.
    says("server fn s(f: fn) = 1\n", &["1 error: parameter `f` of server fn `s` is fn, which the browser cannot send"]);
    says("type Note = { text: str }\n/// It.\nfn mcp probe(a: int, b: str?, c: list<int>, d: { x: int }, e: Note, f: \"a\" | \"b\", g: any = 1) with time = now()\n", &[]);
}

#[test]
fn q853_check_warns_of_what_the_client_would_not_be_told() {
    let w = |s: &str| format!("2 warning: {}", s);
    // No `///`: a `//` line is not one.
    says("// Not a doc.\nfn mcp probe() with time = now()\n", &[&w("tool `probe` has no `///` comment: an AI client sees only its name and its parameters; say what it does on a `///` line above it")]);
    // A parameter with no type, a default too; one with a type.
    says("/// It.\nfn mcp probe(a, b = 2, c: int) with time = now()\n", &[
        &w("parameter `a` of tool `probe` has no type: its schema is `{}`, and the model guesses what to send"),
        &w("parameter `b` of tool `probe` has no type: its schema is `{}`, and the model guesses what to send"),
    ]);
    // No `with`: what it is seen to use, or none.
    says("/// It.\nfn mcp probe() = now()\n", &[&w("tool `probe` has no `with`, so an AI client is told it may change anything and reach anywhere; it uses time: `with time`")]);
    says("/// It.\nfn mcp probe() = [random(), now()]\n", &[&w("tool `probe` has no `with`, so an AI client is told it may change anything and reach anywhere; it uses time and random: `with time, random`")]);
    // Seen to do nothing, it is fine as written: `with` takes one effect at least, and a hint of
    // one it does not use would tell the client it may (decided in review: it warned, and said
    // `with db`). Through a helper, seen as well; a tool that uses nothing passes untold.
    says("/// Adds.\nfn mcp add(a: int, b: int) -> int = a + b\n", &[]);
    says("fn twice(n) = n * 2\n/// It.\nfn mcp probe(a: int) -> int = twice(a) + len(str(a))\n", &[]);
    says("fn stamp() = now()\n/// It.\nfn mcp probe(a: int) = [a, stamp()]\n", &["3 warning: tool `probe` has no `with`, so an AI client is told it may change anything and reach anywhere; it uses time: `with time`"]);
    // A server fn mcp the same; a plain fn and a plain server fn are told nothing.
    says("/// It.\nserver fn mcp probe(a) with time = now()\n", &[&w("parameter `a` of tool `probe` has no type: its schema is `{}`, and the model guesses what to send")]);
    says("fn f(a) = a\nserver fn s(a) = a\nprint(f(1))\n", &[]);
}

#[test]
fn q853_a_tool_answers_with_data_not_a_page() {
    let page = |what: &str| format!("2 error: a tool answers with data, not a page: `probe` answers with {}", what);
    says("/// It.\nfn mcp probe() with time = <p>{now()}</p>\n", &[&page("html")]);
    says("/// It.\nfn mcp probe() with time = redirect(\"/login\")\n", &[&page("a response, as `redirect(...)` makes")]);
    says("/// It.\nfn mcp probe() -> html with time = markdown(str(now()))\n", &[&page("html")]);
    says("/// It.\nserver fn mcp probe() with time = respond(200, str(now()))\n", &[&page("a response, as `redirect(...)` makes")]);
    // An early `return` of one, and an answer that may be one.
    says("/// It.\nfn mcp probe(x: bool) with time {\n    if x {\n        return redirect(\"/\")\n    }\n    <p>{now()}</p>\n}\n", &[&page("html")]);
    says("/// It.\nfn mcp probe(x: bool) with time = if x { <p/> } else { str(now()) }\n", &["2 warning: a tool answers with data, not a page: `probe` may answer with html"]);
    // Data, null, an error, a `reply`: none is a page; nor what the checker cannot see.
    says("/// It.\nfn mcp probe(x: int) -> map ! str with time {\n    if x < 0 {\n        reply 400 \"no\"\n    }\n    if x == 0 {\n        return err(\"zero\")\n    }\n    { at: now(), html: \"<p/>\" }\n}\n", &[]);
    says("/// It.\nfn mcp probe(f: any) with time = now()\n", &[]);
    // A view that answers html stays a view; a plain fn may answer with a page.
    says("fn page() = <p>x</p>\nprint(page())\n", &[]);
}

#[test]
fn q853_service_mcp_with_a_name_is_for_later() {
    let later = "`service mcp crm` would call another MCP server, which Quillo does not do yet: `service mcp /crm = \"crm.qwe\"` offers a Quillo file's tools here, and `service crm = \"crm.qwe\"` calls its server fns";
    // Said once, with its line; its calls not said again as unknown.
    says("let a = 1\nservice mcp crm = \"https://crm.example.com/mcp\"\nfn find_one() with http = crm.find_customer(\"x\")\nprint(a)\n", &[&format!("2 error: {}", later)]);
    // Run anyway, the program says it at its line.
    let mut it = Interp::new(".");
    let Err(e) = it.load_source("let a = 1\nservice mcp crm = \"https://crm.example.com/mcp\"\n", "case.qwe") else { panic!("it ran") };
    assert_eq!((e.line, e.msg.as_str()), (2, later));
}

#[test]
fn q853_service_mcp_with_a_path_is_a_service_as_today() {
    let d = dir("q853-service", "service mcp /billing = \"billing.qwe\"\n\nroute GET / = str(billing.total())\n");
    std::fs::write(d.join("billing.qwe"), "/// The total.\nfn mcp invoice() with time = 1\n\nserver fn total() = 41 + 1\n\nroute GET /status = \"up\"\n").unwrap();
    let (ok, out, err) = quillo(&d, &["check", "main.qwe"]);
    assert!(ok, "{}{}", out, err);
    assert!(out.contains("main.qwe: ok") && !err.contains("warning"), "{}{}", out, err);
    {
        let s = support::serve(&d.join("main.qwe"), &[]);
        assert_eq!(support::send(s.port, "GET", "/", &[], "").body, "42");
        assert_eq!(support::send(s.port, "GET", "/billing/status", &[], "").body, "up");
    }
    // Not under --split: the named form is no service to start, so it starts none.
    assert_eq!(quillo::service::declared(&d.join("main.qwe")).unwrap().len(), 1);
    std::fs::write(d.join("main.qwe"), "service mcp /billing = \"billing.qwe\"\nservice mcp crm = \"https://crm.example.com/mcp\"\n").unwrap();
    assert_eq!(quillo::service::declared(&d.join("main.qwe")).unwrap().iter().map(|(n, _, _)| n.as_str()).collect::<Vec<_>>(), ["billing"]);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn q853_a_tools_name_is_the_programs_in_every_file() {
    // Two files' tools of one name are one address: "defined twice", as two server fns' are.
    let d = dir("q853-files", "use lib.qwe\n\n/// Mine.\nfn mcp total() with time = 2\n\nroute GET / = str(total())\n");
    std::fs::write(d.join("lib.qwe"), "/// Theirs.\nfn mcp total() with time = 1\n").unwrap();
    let (ok, out, err) = quillo(&d, &["check", "main.qwe"]);
    assert!(!ok && err.contains("main.qwe:4: error: `total` is defined twice, at lib.qwe:2 and main.qwe:4"), "{}{}", out, err);
    // Two plain functions of one name are each their file's, as before.
    std::fs::write(d.join("main.qwe"), "use lib.qwe\n\nfn total() = 2\n\nroute GET / = str(total())\n").unwrap();
    std::fs::write(d.join("lib.qwe"), "fn total() = 1\n").unwrap();
    let (ok, out, err) = quillo(&d, &["check", "main.qwe"]);
    assert!(ok, "{}{}", out, err);
    // A tool in a file the program uses is taken as any function, by `use` and by `import`.
    std::fs::write(d.join("lib.qwe"), "/// Theirs.\nfn mcp total() with time = 1\n").unwrap();
    std::fs::write(d.join("main.qwe"), "import lib.qwe use total\n\nroute GET / = str(total())\n").unwrap();
    let (ok, out, err) = quillo(&d, &["check", "main.qwe"]);
    assert!(ok, "{}{}", out, err);
    // `quillo fix` writes the import, and leaves the tags as they are.
    std::fs::write(d.join("main.qwe"), "use total from \"lib.qwe\"\n\n/// Mine.\nfn mcp mine() with time = total()\n\nroute GET / = str(mine())\n").unwrap();
    let (ok, out, err) = quillo(&d, &["fix", "main.qwe"]);
    assert!(ok, "{}{}", out, err);
    assert_eq!(std::fs::read_to_string(d.join("main.qwe")).unwrap(), "import lib.qwe use total\n\n/// Mine.\nfn mcp mine() with time = total()\n\nroute GET / = str(mine())\n");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn q853_quillo_check_says_each_with_its_line() {
    let d = dir(
        "q853-check",
        "/// Fine.\nserver fn mcp add(a: int, b: int) -> int with time = a + b\n\nprivate fn helper() = now()\n\nfn mcp untold(x) = [helper(), x]\n\nfn outer() {\n    fn mcp inner() = 1\n    inner()\n}\n\nroute GET / = str(outer())\n",
    );
    let (ok, out, err) = quillo(&d, &["check", "main.qwe"]);
    assert!(!ok, "{}{}", out, err);
    let lines: Vec<&str> = err.lines().collect();
    assert_eq!(lines, [
        "main.qwe:6: warning: parameter `x` of tool `untold` has no type: its schema is `{}`, and the model guesses what to send",
        "main.qwe:6: warning: tool `untold` has no `///` comment: an AI client sees only its name and its parameters; say what it does on a `///` line above it",
        "main.qwe:6: warning: tool `untold` has no `with`, so an AI client is told it may change anything and reach anywhere; it uses time: `with time`",
        "main.qwe:9: error: `fn mcp` must be declared at the top level, as `server fn` is: a tool is an address of the program's",
        "quillo: 1 error, 3 warnings",
    ], "{}", err);
    // A private tool stops the parse, as a private server fn does; `quillo run` refuses both.
    std::fs::write(d.join("main.qwe"), "private fn mcp t() = 1\n").unwrap();
    let (ok, _, err) = quillo(&d, &["check", "main.qwe"]);
    assert!(!ok && err.contains("main.qwe:1: error: a `fn mcp` cannot be private"), "{}", err);
    // Clean, a tool counts as a function.
    std::fs::write(d.join("main.qwe"), format!("{}\n/// Adds.\nserver fn mcp add(a: int, b: int) -> int with time = a + b\n", CLEAN)).unwrap();
    let (ok, out, err) = quillo(&d, &["check", "main.qwe"]);
    assert!(ok && out.trim() == "main.qwe: ok (0 routes, 2 functions, 0 components)", "{}{}", out, err);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn q853_a_server_fn_mcp_answers_at_rpc_and_a_plain_fn_mcp_does_not() {
    let d = dir("q853-rpc", "/// Adds.\nserver fn mcp add(a: int, b: int) -> int with time = a + b\n\n/// Seven.\nfn mcp seven() -> int with time = 7\n\nserver fn mcp(x: int) -> int = x + 1\n\nroute GET / = str(seven() + add(1, 2))\n");
    let s = support::serve(&d.join("main.qwe"), &[]);
    let a = support::send(s.port, "POST", "/_quillo/rpc/add", &[], "[2, 3]");
    assert_eq!((a.status, a.body.as_str()), (200, "{\"ok\":5}"));
    // Its checks, as a server fn's.
    let a = support::send(s.port, "POST", "/_quillo/rpc/add", &[], "[\"2\", 3]");
    assert_eq!(a.status, 400, "{}", a.body);
    // A plain `fn mcp` is no page's to call, as any plain function: 404.
    let b = support::send(s.port, "POST", "/_quillo/rpc/seven", &[], "[]");
    assert_eq!(b.status, 404, "{}", b.body);
    // A server fn named `mcp` answers as before; the program calls the tool as a function.
    assert_eq!(support::send(s.port, "POST", "/_quillo/rpc/mcp", &[], "[1]").body, "{\"ok\":2}");
    assert_eq!(support::send(s.port, "GET", "/", &[], "").body, "10");
    drop(s);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn q853_the_document_marks_a_server_fn_mcp_and_only_it() {
    let doc = doc_of("q853-doc", "/// Adds.\nserver fn mcp add(a: int) -> int with time = a\n\n/// Seven.\nfn mcp seven() with time = 7\n\nserver fn plain() = 1\n");
    let fns = doc["x-quillo-server-fns"].as_object().unwrap();
    assert_eq!(fns.keys().collect::<Vec<_>>(), ["add", "plain"]);
    assert_eq!(fns["add"]["x-quillo-mcp"], json!(true));
    assert_eq!(fns["add"]["description"], json!("Adds."));
    assert!(fns["plain"].get("x-quillo-mcp").is_none());
    // `/_quillo/describe`, from the running program, says the same.
    let mut it = Interp::new(".");
    it.load_source("/// Adds.\nserver fn mcp add(a: int) -> int with time = a\n\n/// Seven.\nfn mcp seven() with time = 7\n\nserver fn plain() = 1\n", "main.qwe").unwrap();
    assert_eq!(of_interp(&it, "main")["x-quillo-server-fns"], doc["x-quillo-server-fns"]);
    // Its contract is a server fn's, so a service's copy of it is not refused.
    assert_eq!(fns["add"]["contract"], json!("add(a: int) -> int with time"));
    let stmts = quillo::parser::Parser::new("server fn mcp add(a: int) -> int with time = a\n").parse_program().unwrap();
    let Stmt::Fn(d) = &stmts[0] else { panic!() };
    assert_eq!(contract(d), "add(a: int) -> int");
}

// The highlighter, on the server (highlight.rs) and in the browser (runtime.js), token for token.

/// A Quillo string literal of `s`.
fn lit(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' | '"' | '$' | '{' | '}' => {
                o.push('\\');
                o.push(c);
            }
            '\n' => o.push_str("\\n"),
            '\t' => o.push_str("\\t"),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// `highlight(code, "quillo")` of each, as the server makes it, once the browser's runtime (in
/// Node) has made the same.
fn highlighted(codes: &[&str]) -> Vec<String> {
    let calls: Vec<String> = codes.iter().map(|c| format!("str(highlight({}, \"quillo\"))", lit(c))).collect();
    let src = format!("fn __t() = [{}]\n", calls.join(", "));
    let mut it = Interp::new(".");
    it.load_source(&src, "case.qwe").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    let v = match it.call(&quillo::value::lookup(&it.globals, "__t").unwrap(), vec![], 0) {
        Ok(v) => v,
        Err(_) => panic!("no value"),
    };
    if Command::new("node").arg("--version").output().map(|o| o.status.success()).unwrap_or(false) {
        let js = quillo::client::test_script(&it, "__t").unwrap_or_else(|e| panic!("{}", e));
        let out = support::node(&js).expect("node");
        assert!(out.status.success(), "node failed: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), v.repr(), "the browser differs");
    } else {
        eprintln!("skipping the browser half: no Node");
    }
    let quillo::value::Value::List(l) = v else { panic!("not a list") };
    l.iter().map(|x| x.display()).collect()
}

#[test]
fn q853_the_tag_is_coloured_only_where_it_is_one() {
    let k = |w: &str| format!("<span class=\"hl-k\">{}</span>", w);
    let f = |w: &str| format!("<span class=\"hl-f\">{}</span>", w);
    let cases: Vec<(&str, String)> = vec![
        ("fn mcp total() = 1", format!("{} {} {}() = <span class=\"hl-n\">1</span>", k("fn"), k("mcp"), f("total"))),
        ("server fn mcp add()", format!("{} {} {} {}()", k("server"), k("fn"), k("mcp"), f("add"))),
        ("fn mcp mcp()", format!("{} {} {}()", k("fn"), k("mcp"), f("mcp"))),
        ("fn\tmcp\tcafé()", format!("{}\t{}\t{}()", k("fn"), k("mcp"), f("café"))),
        ("view mcp Card()", format!("{} {} {}()", k("view"), k("mcp"), f("Card"))),
        ("component mcp Box()", format!("{} {} {}()", k("component"), k("mcp"), f("Box"))),
        ("service mcp /billing = 1", "service <span class=\"hl-k\">mcp</span> /billing = <span class=\"hl-n\">1</span>".to_string()),
        ("service mcp crm = 1", format!("service {} crm = <span class=\"hl-n\">1</span>", k("mcp"))),
        // A name: a function named `mcp`, a service named `mcp`, a variable, a key, a call.
        ("fn mcp(x) = x", format!("{} {}(x) = x", k("fn"), f("mcp"))),
        ("fn mcp (x)", format!("{} {} (x)", k("fn"), f("mcp"))),
        ("service mcp = 1", "service mcp = <span class=\"hl-n\">1</span>".to_string()),
        ("let mcp = mcp + m.mcp", format!("{} mcp = mcp + m.mcp", k("let"))),
        ("xfn mcp total", "xfn mcp total".to_string()),
        ("fn mcp", format!("{} mcp", k("fn"))),
        ("fn mcp\ntotal", format!("{} mcp\ntotal", k("fn"))),
        ("fn mcp 1", format!("{} mcp <span class=\"hl-n\">1</span>", k("fn"))),
        ("fnmcp total", "fnmcp total".to_string()),
        ("service mcp\n/a", "service mcp\n/a".to_string()),
        ("service mcp /* note */ = 1", "service mcp <span class=\"hl-c\">/* note */</span> = <span class=\"hl-n\">1</span>".to_string()),
        ("service mcp // note", "service mcp <span class=\"hl-c\">// note</span>".to_string()),
        ("service mcp /", format!("service {} /", k("mcp"))),
        ("// fn mcp t()", "<span class=\"hl-c\">// fn mcp t()</span>".to_string()),
        ("let s = \"fn mcp t\"", format!("{} s = <span class=\"hl-s\">&quot;fn mcp t&quot;</span>", k("let"))),
    ];
    let got = highlighted(&cases.iter().map(|(c, _)| *c).collect::<Vec<_>>());
    for ((code, want), g) in cases.iter().zip(got) {
        assert_eq!(&g, want, "{:?}", code);
    }
    assert_eq!(quillo::highlight::highlight("fn mcp t()", "rust"), "<span class=\"hl-k\">fn</span> mcp <span class=\"hl-f\">t</span>()");
}

// The language server.

/// An editor talking to `quillo lsp`, killed when dropped or after a minute.
struct Editor {
    child: std::process::Child,
    out: std::io::BufReader<std::process::ChildStdout>,
    next: u64,
    notes: Vec<Value>,
    done: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Editor {
    fn start(cwd: &Path) -> Editor {
        use std::process::Stdio;
        let mut child = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("lsp").current_dir(cwd).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
        let out = std::io::BufReader::new(child.stdout.take().unwrap());
        // The deadline: this test's own process, stopped if it has not ended.
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (pid, flag) = (child.id() as i32, done.clone());
        std::thread::spawn(move || {
            let until = Instant::now() + Duration::from_secs(60);
            while Instant::now() < until {
                if flag.load(std::sync::atomic::Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            if !flag.load(std::sync::atomic::Ordering::SeqCst) {
                // SAFETY: a signal to the process this test started, which has not been reaped.
                unsafe { libc::kill(pid, libc::SIGKILL) };
            }
        });
        let mut e = Editor { child, out, next: 1, notes: Vec::new(), done };
        e.request("initialize", json!({ "capabilities": {} }));
        e.notify("initialized", json!({}));
        e
    }

    fn send(&mut self, msg: Value) {
        use std::io::Write;
        let body = msg.to_string();
        let stdin = self.child.stdin.as_mut().unwrap();
        write!(stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
        stdin.flush().unwrap();
    }

    fn recv(&mut self) -> Value {
        use std::io::{BufRead, Read};
        let mut len = 0;
        loop {
            let mut line = String::new();
            assert!(self.out.read_line(&mut line).unwrap() > 0, "the language server ended");
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some(v) = line.strip_prefix("Content-Length:") {
                len = v.trim().parse().unwrap();
            }
        }
        let mut buf = vec![0; len];
        self.out.read_exact(&mut buf).unwrap();
        serde_json::from_slice(&buf).unwrap()
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            let m = self.recv();
            if m.get("id") == Some(&json!(id)) {
                return m["result"].clone();
            }
            self.notes.push(m);
        }
    }

    /// Opens `text` and waits for its diagnostics: it is read once they come.
    fn open(&mut self, uri: &str, text: &str) {
        self.notify("textDocument/didOpen", json!({ "textDocument": { "uri": uri, "languageId": "quillo", "version": 1, "text": text } }));
        while !self.notes.iter().any(|m| m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == uri) {
            let m = self.recv();
            self.notes.push(m);
        }
    }

    fn at(&mut self, method: &str, uri: &str, line: usize, character: usize) -> Value {
        self.request(method, json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": character } }))
    }

    fn hover(&mut self, uri: &str, line: usize, character: usize) -> String {
        self.at("textDocument/hover", uri, line, character)["contents"]["value"].as_str().unwrap_or("").to_string()
    }

    /// Whether completion here offers the tag, and first.
    fn offers_tag(&mut self, uri: &str, line: usize, character: usize) -> bool {
        let items = self.at("textDocument/completion", uri, line, character);
        let tag = items.as_array().unwrap().iter().position(|i| i["label"] == "mcp" && i["kind"] == 14);
        assert!(tag.map_or(true, |t| t == 0), "the tag is not first: {}", items);
        tag.is_some()
    }
}

impl Drop for Editor {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.done.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

#[test]
fn q853_the_editor_hovers_completes_and_outlines_tools() {
    let d = support::temp_dir("feedback56-q853-lsp").canonicalize().unwrap();
    let main = "/// Today's total.\nfn mcp total() with time = now()\nfn mcp(x) = x\nservice mcp /billing = \"billing.qwe\"\nlet y = mcp(1) + total()\n";
    let named = "/// Named so.\nfn mcp mcp() with time = 1\n";
    let typing = "fn \nserver fn m\nservice \nlet \nfn mcpx\npublic fn \nprivate fn \nfn x\n";
    for (f, t) in [("main.qwe", main), ("named.qwe", named), ("typing.qwe", typing), ("billing.qwe", "server fn t() = 1\n")] {
        std::fs::write(d.join(f), t).unwrap();
    }
    let uri = |f: &str| format!("file://{}", d.join(f).display());
    let (a, b, c) = (uri("main.qwe"), uri("named.qwe"), uri("typing.qwe"));
    let mut e = Editor::start(&d);
    e.open(&a, main);
    e.open(&b, named);
    e.open(&c, typing);
    // On the tag: what a tool is, after `fn` and after `service`.
    let tag = e.hover(&a, 1, 4);
    assert!(tag.contains("A tool, for AI clients that speak MCP"), "{}", tag);
    assert_eq!(e.hover(&a, 3, 9), tag);
    assert_eq!(e.hover(&b, 1, 3), tag);
    // On the name: the tool, as written, a tool named `mcp` too.
    assert!(e.hover(&a, 1, 9).contains("fn mcp total() with time"), "{}", e.hover(&a, 1, 9));
    assert!(e.hover(&b, 1, 8).contains("fn mcp mcp() with time"), "{}", e.hover(&b, 1, 8));
    // `mcp` as a name: the function's, not the tag's.
    for (line, col) in [(2, 4), (4, 9)] {
        let h = e.hover(&a, line, col);
        assert!(h.contains("fn mcp(x)") && !h.contains("A tool"), "{}:{}: {}", line, col, h);
    }
    // Completion: after `fn `, `server fn m`, `service `, `public fn `; not after `let `, in a
    // name, after `private fn ` (refused), nor after a name's first letter.
    for (line, col, want) in [(0, 3, true), (1, 11, true), (2, 8, true), (5, 10, true), (3, 4, false), (4, 7, false), (6, 11, false), (0, 2, false), (7, 4, false)] {
        assert_eq!(e.offers_tag(&c, line, col), want, "line {} column {}", line, col);
    }
    // The outline: each tool says it is one, its name where it is written, after the tag.
    let symbols = |e: &mut Editor, u: &str| e.request("textDocument/documentSymbol", json!({ "textDocument": { "uri": u } }));
    let (sa, sb) = (symbols(&mut e, &a), symbols(&mut e, &b));
    let of = |all: &Value, name: &str, line: u64| all.as_array().unwrap().iter().find(|s| s["name"] == name && s["range"]["start"]["line"] == line).cloned().unwrap_or_else(|| panic!("no {} in {}", name, all));
    let at = |line: u64, from: u64, to: u64| json!({ "start": { "line": line, "character": from }, "end": { "line": line, "character": to } });
    assert_eq!(of(&sa, "total", 1)["detail"], "fn mcp total() with time");
    assert_eq!(of(&sa, "total", 1)["selectionRange"], at(1, 7, 12));
    assert_eq!(of(&sa, "mcp", 2)["detail"], "fn mcp(x)");
    assert_eq!(of(&sa, "mcp", 2)["selectionRange"], at(2, 3, 6));
    assert_eq!(of(&sb, "mcp", 1)["detail"], "fn mcp mcp() with time");
    assert_eq!(of(&sb, "mcp", 1)["selectionRange"], at(1, 7, 10));
    // Go to the definition: of `mcp(1)`, the function named so; of the tool named `mcp`, its name.
    assert_eq!(e.at("textDocument/definition", &a, 4, 9)["range"], at(2, 3, 6));
    assert_eq!(e.at("textDocument/definition", &b, 1, 8)["range"], at(1, 7, 10));
    drop(e);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn q853_review_a_file_that_fails_to_parse_defines_its_tools_names() {
    // Its names are read from its text, so their uses are not said again as unknown: the tag was
    // read as the name, and `total` was unknown (found in review).
    let d = dir("q853-unparsed", "use lib.qwe\n\nroute GET / = str(total() + mcp(1) + plain())\n");
    std::fs::write(d.join("lib.qwe"), "/// T.\nfn mcp total() with time = 1\nfn mcp(x) = x\nserver fn plain() = 1\nlet broken = (\n").unwrap();
    let a = quillo::check::check_file(&d.join("main.qwe"));
    let said: Vec<String> = a.diagnostics.iter().map(|x| format!("{}:{} {}", x.file.rsplit('/').next().unwrap_or(""), x.line, x.message)).collect();
    assert_eq!(said.len(), 1, "{:#?}", said);
    assert!(said[0].starts_with("lib.qwe:"), "{:#?}", said);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn q853_review_a_tools_parameters_are_no_call_of_it() {
    let d = support::temp_dir("feedback56-q853-signature").canonicalize().unwrap();
    let text = "/// Adds.\nfn mcp total(a: int, b: int) with time = a + b\nserver fn mcp twice(n: int) with time = n * 2\nlet y = total(1, 2)\n";
    std::fs::write(d.join("main.qwe"), text).unwrap();
    let uri = format!("file://{}", d.join("main.qwe").display());
    let mut e = Editor::start(&d);
    e.open(&uri, text);
    // Typing a tool's parameters: no signature of the tool itself, as for any definition.
    assert_eq!(e.at("textDocument/signatureHelp", &uri, 1, 21), Value::Null);
    assert_eq!(e.at("textDocument/signatureHelp", &uri, 2, 24), Value::Null);
    // A call of it has one.
    let call = e.at("textDocument/signatureHelp", &uri, 3, 17);
    assert_eq!(call["activeParameter"], 1, "{}", call);
    assert!(call["signatures"][0]["label"].as_str().unwrap_or("").contains("total(a: int, b: int)"), "{}", call);
    drop(e);
    let _ = std::fs::remove_dir_all(&d);
}

// ---- slice 3: a tool's schemas in JSON Schema 2020-12 (Part B.2), its hints (B.3), and what
// only reads (Q-858) --------------------------------------------------------------------------

use quillo::mcp::{self, Tool};
use quillo::value::Value as Q;

/// The tools of a program of several files, as the door lists them.
fn tools_in(name: &str, files: &[(&str, &str)]) -> Vec<Tool> {
    let d = support::temp_dir(&format!("feedback56-{}", name));
    for (f, src) in files {
        std::fs::write(d.join(f), src).unwrap();
    }
    let t = mcp::of_source(&d.join("main.qwe"));
    let _ = std::fs::remove_dir_all(&d);
    t.unwrap_or_else(|e| panic!("{}", e))
}

fn tools_of(name: &str, src: &str) -> Vec<Tool> {
    tools_in(name, &[("main.qwe", src)])
}

fn the<'a>(tools: &'a [Tool], name: &str) -> &'a Tool {
    tools.iter().find(|t| t.name == name).unwrap_or_else(|| panic!("no tool {} in {:?}", name, tools.iter().map(|t| &t.name).collect::<Vec<_>>()))
}

/// What the checker found each tool's code to do.
fn facts_in(name: &str, files: &[(&str, &str)]) -> Vec<quillo::check::ToolFacts> {
    let d = support::temp_dir(&format!("feedback56-{}", name));
    for (f, src) in files {
        std::fs::write(d.join(f), src).unwrap();
    }
    let a = quillo::check::check_file(&d.join("main.qwe"));
    let _ = std::fs::remove_dir_all(&d);
    let errors: Vec<String> = a.errors().map(|x| x.to_string()).collect();
    assert!(errors.is_empty(), "{:#?}", errors);
    a.tools
}

fn fact<'a>(facts: &'a [quillo::check::ToolFacts], name: &str) -> &'a quillo::check::ToolFacts {
    facts.iter().find(|f| f.def.name == name).unwrap_or_else(|| panic!("no tool {}", name))
}

/// Every `$ref` of a schema points into its own `$defs`, and each of those is there.
fn refs_local(schema: &Value) {
    let mut found = Vec::new();
    refs(schema, &mut found);
    for r in found {
        let key = r.strip_prefix("#/$defs/").unwrap_or_else(|| panic!("a $ref not to $defs: {} in {}", r, schema));
        assert!(schema["$defs"].get(key).is_some(), "{} is not under $defs: {}", key, schema);
    }
}

/// Ajv's JSON Schema 2020-12 validator, run by npx (no global install): each case's schema
/// compiled in strict mode with the standard formats checked (ajv-formats), as a client may, then
/// its `valid` values each passed and its `invalid` ones each refused. None when npx or the package cannot be had here: the callers check the documents'
/// shapes themselves then, and say so.
fn ajv(cases: &[Value]) -> Option<Vec<Value>> {
    // A folder of each call's own: tests run at once.
    static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let d = support::temp_dir(&format!("feedback56-ajv-{}", CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    let script = r#"const Ajv = require("ajv/dist/2020");
const formats = require("ajv-formats");
const cases = JSON.parse(require("fs").readFileSync(process.argv[2], "utf8"));
console.log(JSON.stringify(cases.map((c) => {
  const ajv = new Ajv({ strict: true, allowUnionTypes: true, validateFormats: true });
  formats(ajv);
  let v;
  try { v = ajv.compile(c.schema); } catch (e) { return { compile: String(e.message) }; }
  return { valid: c.valid.map((x) => (v(x) ? null : ajv.errorsText(v.errors))), invalid: c.invalid.map((x) => !v(x)) };
})));
"#;
    std::fs::write(d.join("check.js"), script).unwrap();
    std::fs::write(d.join("cases.json"), Value::Array(cases.to_vec()).to_string()).unwrap();
    let run = Command::new("sh")
        .arg("-c")
        .arg(r#"npx --yes -p ajv-cli@5.0.0 -p ajv-formats@3.0.1 -c 'NODE_PATH="$(dirname "$(dirname "$(command -v ajv)")")" node check.js cases.json'"#)
        .current_dir(&d)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn();
    let Ok(mut child) = run else {
        let _ = std::fs::remove_dir_all(&d);
        return None;
    };
    let deadline = Instant::now() + Duration::from_secs(240);
    while child.try_wait().ok().flatten().is_none() {
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_dir_all(&d);
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let out = child.wait_with_output().ok()?;
    let _ = std::fs::remove_dir_all(&d);
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().last()?;
    serde_json::from_str::<Vec<Value>>(line).ok()
}

/// Each case against the validator: `(what, schema, valid, invalid)`.
fn validated(cases: &[(String, Value, Vec<Value>, Vec<Value>)]) {
    let mut docs: Vec<Value> = cases.iter().map(|(_, s, v, i)| json!({ "schema": s, "valid": v, "invalid": i })).collect();
    // The validator is a strict one: a type 2020-12 does not have does not compile, nor a format
    // it does not know.
    docs.push(json!({ "schema": { "type": "strin" }, "valid": [], "invalid": [] }));
    docs.push(json!({ "schema": { "type": "string", "format": "decimal" }, "valid": [], "invalid": [] }));
    let Some(mut results) = ajv(&docs) else {
        eprintln!("skipped: the 2020-12 validator (npx could not run ajv-cli@5.0.0 with ajv-formats@3.0.1 here: no npx, or no network); the schemas' shapes are checked alone");
        return;
    };
    assert!(results.pop().is_some_and(|r| r.get("compile").is_some()), "the validator took a format it does not know");
    assert!(results.pop().is_some_and(|r| r.get("compile").is_some()), "the validator took a type 2020-12 does not have");
    assert_eq!(results.len(), cases.len());
    for ((what, schema, valid, invalid), r) in cases.iter().zip(&results) {
        assert!(r.get("compile").is_none(), "{}: the schema does not compile as 2020-12: {} in {}", what, r["compile"], schema);
        for (v, why) in valid.iter().zip(r["valid"].as_array().unwrap()) {
            assert!(why.is_null(), "{}: {} should pass {}: {}", what, v, schema, why);
        }
        for (v, refused) in invalid.iter().zip(r["invalid"].as_array().unwrap()) {
            assert_eq!(refused, &json!(true), "{}: {} should be refused by {}", what, v, schema);
        }
    }
}

/// The guide's types (Variables, functions, views; Records), each as a parameter and as a result,
/// with what they are written as, what fits and what does not.
fn guide_types() -> Vec<(&'static str, Value, Vec<Value>, Vec<Value>)> {
    let dec = json!({ "type": "string", "pattern": "^[+-]?([0-9]+[.]?[0-9]*|[.][0-9]+)([eE][+-]?[0-9]{1,6})?$", "description": "a decimal number, as text: \"12.50\"" });
    let money = json!({
        "type": "object",
        "required": ["amount", "currency"],
        "properties": { "amount": dec, "currency": { "type": "string", "pattern": "^[A-Za-z]{3}$" } }
    });
    vec![
        ("int", json!({ "type": "integer" }), vec![json!(1), json!(-5), json!(0)], vec![json!(1.5), json!("1"), json!(null)]),
        ("float", json!({ "type": "number" }), vec![json!(1.5), json!(2)], vec![json!("x"), json!(null)]),
        ("num", json!({ "type": "number" }), vec![json!(1), json!(2.5)], vec![json!("1")]),
        // Text `dec` reads: a sign, a point, an exponent; not a comma, nor words.
        ("dec", dec.clone(), vec![json!("12.50"), json!("-3"), json!("+.5"), json!("1e-6"), json!("7.")], vec![json!(true), json!("12,5"), json!("abc"), json!(""), json!("1e1234567")]),
        ("str", json!({ "type": "string" }), vec![json!(""), json!("é ✓")], vec![json!(1), json!(null)]),
        ("date", json!({ "type": "string", "format": "date" }), vec![json!("2026-11-08")], vec![json!(5), json!("2026-13-40")]),
        ("bool", json!({ "type": "boolean" }), vec![json!(true), json!(false)], vec![json!(0), json!("true")]),
        ("list", json!({ "type": "array" }), vec![json!([]), json!([1, "a"])], vec![json!({})]),
        ("map", json!({ "type": "object" }), vec![json!({}), json!({ "a": 1 })], vec![json!([])]),
        ("any", json!({}), vec![json!(1), json!(null), json!("x"), json!([]), json!({})], vec![]),
        ("money", money.clone(), vec![json!({ "amount": "1.00", "currency": "USD" })], vec![json!({ "amount": "1.00" }), json!({ "amount": "1", "currency": "US" }), json!({ "amount": "one", "currency": "USD" })]),
        ("str?", json!({ "type": ["string", "null"] }), vec![json!("a"), json!(null)], vec![json!(1)]),
        ("int | str", json!({ "type": ["integer", "string"] }), vec![json!(1), json!("a")], vec![json!(1.5), json!(null)]),
        ("int | null", json!({ "type": ["integer", "null"] }), vec![json!(1), json!(null)], vec![json!("a")]),
        ("int | str | null", json!({ "type": ["integer", "string", "null"] }), vec![json!(1), json!("a"), json!(null)], vec![json!(true)]),
        ("int | int", json!({ "type": "integer" }), vec![json!(1)], vec![json!("a")]),
        ("list<int>", json!({ "type": "array", "items": { "type": "integer" } }), vec![json!([]), json!([1, 2])], vec![json!(["a"]), json!([1.5])]),
        ("list<str>?", json!({ "type": ["array", "null"], "items": { "type": "string" } }), vec![json!(null), json!(["a"])], vec![json!([1])]),
        ("list<list<int>>", json!({ "type": "array", "items": { "type": "array", "items": { "type": "integer" } } }), vec![json!([[1], []])], vec![json!([1])]),
        ("money?", json!({ "type": ["object", "null"], "required": ["amount", "currency"], "properties": money["properties"].clone() }), vec![json!(null)], vec![json!({})]),
        ("\"admin\" | \"member\"", json!({ "type": "string", "enum": ["admin", "member"] }), vec![json!("admin")], vec![json!("owner"), json!(null)]),
        ("\"admin\" | \"member\" | null", json!({ "type": ["string", "null"], "enum": ["admin", "member", null] }), vec![json!("member"), json!(null)], vec![json!("x")]),
        ("\"auto\" | int", json!({ "anyOf": [{ "type": "string", "enum": ["auto"] }, { "type": "integer" }] }), vec![json!("auto"), json!(3)], vec![json!("x"), json!(1.5)]),
        ("\"ü\" | \"日本\"", json!({ "type": "string", "enum": ["ü", "日本"] }), vec![json!("日本")], vec![json!("u")]),
        // `null` alone is read as `any?`, any value.
        ("null", json!({}), vec![json!(null), json!(0)], vec![]),
        ("any?", json!({}), vec![json!(null), json!(1)], vec![]),
        // What cannot come as JSON is any value in a result, in 2020-12's words alone.
        ("fn", json!({}), vec![json!(1)], vec![]),
    ]
}

#[test]
fn b2_each_type_of_the_guide_as_a_parameter_and_a_result() {
    let mut src = String::new();
    for (i, (ty, _, _, _)) in guide_types().iter().enumerate() {
        // A function is no parameter of a tool's (`quillo check` says so): a result only.
        let param = if *ty == "fn" { "any" } else { ty };
        src.push_str(&format!("/// {}.\nfn mcp t{}(x: {}) -> {} with time = x\n", ty, i, param, ty));
    }
    let tools = tools_of("b2-types", &src);
    let mut cases = Vec::new();
    for (i, (ty, want, good, bad)) in guide_types().into_iter().enumerate() {
        let t = the(&tools, &format!("t{}", i));
        let input = &t.input_schema;
        assert_eq!(input["properties"]["x"], if ty == "fn" { json!({}) } else { want.clone() }, "{} as a parameter", ty);
        assert_eq!(input["required"], json!(["x"]), "{}", ty);
        assert_eq!(input["additionalProperties"], json!(false), "{}", ty);
        assert!(input.get("$defs").is_none(), "{}: {}", ty, input);
        // A result's root is an object (MCP 2025-11-25, and the SDKs): one that is not is held as
        // `result`, in `structuredContent` too.
        let object = want.get("type") == Some(&json!("object"));
        let out = if object { want.clone() } else { json!({ "type": "object", "properties": { "result": want }, "required": ["result"] }) };
        assert_eq!(t.output_schema.as_ref(), Some(&out), "{} as a result", ty);
        assert_eq!(t.wrapped, !object, "{}", ty);
        // Never a bare value that is not an object.
        assert_eq!(t.structured(json!(1)), if object { None } else { Some(json!({ "result": 1 })) });
        assert_eq!(t.structured(json!({ "a": 1 })), Some(if object { json!({ "a": 1 }) } else { json!({ "result": { "a": 1 } }) }));
        assert_eq!(t.description.as_deref(), Some(&*format!("{}.", ty)));
        // Each value that fits is an argument object the door takes; one more key, or none, it
        // does not.
        let mut refused: Vec<Value> = bad.iter().map(|b| json!({ "x": b })).collect();
        refused.push(json!({}));
        refused.push(json!({ "x": good[0], "y": 1 }));
        cases.push((format!("{} in", ty), input.clone(), good.iter().map(|g| json!({ "x": g })).collect(), refused));
        let (good, bad) = if object { (good, bad) } else { (good.into_iter().map(|g| json!({ "result": g })).collect(), bad.into_iter().map(|b| json!({ "result": b })).chain([json!(1), json!({})]).collect()) };
        cases.push((format!("{} out", ty), out, good, bad));
    }
    validated(&cases);
}

#[test]
fn b2_parameters_their_defaults_and_none_at_all() {
    let src = "/// Now.\nfn mcp now_ms() -> int with time = now()\n\
               fn mcp nothing() with time = now()\n\
               /// Notes.\nfn mcp page(q, limit: int = 20, tags: list<str> = [\"a\"], order: \"new\" | \"old\" = \"new\", from: int = -1, at = now()) with time = q\n";
    let tools = tools_of("b2-params", src);
    // No parameters: an empty object, as MCP recommends; no result type, no output schema; no
    // `///`, no description.
    let now = the(&tools, "now_ms");
    assert_eq!(now.input_schema, json!({ "type": "object", "additionalProperties": false }));
    assert_eq!(now.output_schema, Some(json!({ "type": "object", "properties": { "result": { "type": "integer" } }, "required": ["result"] })));
    let nothing = the(&tools, "nothing");
    assert_eq!(nothing.output_schema, None);
    assert_eq!(nothing.description, None);
    assert!(nothing.to_json().get("description").is_none() && nothing.to_json().get("outputSchema").is_none(), "{}", nothing.to_json());
    // No result type: its answer goes as text alone, no structured content, an object or not.
    assert_eq!((nothing.structured(json!(1)), nothing.structured(json!({ "a": 1 })), nothing.structured(json!(null))), (None, None, None));
    // A parameter without a type is any value; one with a default is not required, and its
    // default is said when it is written as a value, not when it is worked out (`now()`).
    let page = the(&tools, "page");
    assert_eq!(
        page.input_schema,
        json!({
            "type": "object",
            "properties": {
                "q": {},
                "limit": { "type": "integer", "default": 20 },
                "tags": { "type": "array", "items": { "type": "string" }, "default": ["a"] },
                "order": { "type": "string", "enum": ["new", "old"], "default": "new" },
                "from": { "type": "integer", "default": -1 },
                "at": {}
            },
            "required": ["q"],
            "additionalProperties": false
        })
    );
    // Source order, which the door lists them in.
    assert_eq!(tools.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["now_ms", "nothing", "page"]);
    validated(&[
        ("none".into(), now.input_schema.clone(), vec![json!({})], vec![json!({ "a": 1 }), json!([])]),
        ("defaults".into(), page.input_schema.clone(), vec![json!({ "q": 1 }), json!({ "q": null, "order": "old", "at": 5 })], vec![json!({ "limit": 1 }), json!({ "q": 1, "order": "mid" })]),
    ]);
}

#[test]
fn b2_records_nested_and_recursive_once_under_defs() {
    let src = "type Tag = { name: str }\n\
               type Note = { id: int, text: str, role: \"admin\" | \"member\" = \"member\", tags: list<Tag> = [], note: str? }\n\
               type Tree = { value: int, children: list<Tree> = [] }\n\
               type Left = { right: Right? }\n\
               type Right = { left: Left? }\n\
               type Id = int | str\n\
               type Person = { name: str, age: int? }\n\
               /// Saves.\nfn mcp save(note: Note, also: list<Note>) -> Note with time = note\n\
               /// Grows.\nfn mcp grow(t: Tree) -> Tree with time = t\n\
               /// Pairs.\nfn mcp pair(a: Left) -> Right? with time = null\n\
               /// Ids.\nfn mcp ids(id: Id, more: Id?) -> list<Id> with time = [id]\n\
               /// People.\nfn mcp people(p: Person, q: { name: str, age: int? }) -> Person with time = p\n";
    let tools = tools_of("b2-records", src);
    let note = json!({
        "type": "object",
        "properties": {
            "id": { "type": "integer" },
            "text": { "type": "string" },
            "role": { "type": "string", "enum": ["admin", "member"], "default": "member" },
            "tags": { "type": "array", "items": { "$ref": "#/$defs/Tag" }, "default": [] },
            "note": { "type": ["string", "null"] }
        },
        "required": ["id", "text"],
        "additionalProperties": false
    });
    let tag = json!({ "type": "object", "properties": { "name": { "type": "string" } }, "required": ["name"], "additionalProperties": false });
    let save = the(&tools, "save");
    assert_eq!(save.input_schema["properties"]["note"], json!({ "$ref": "#/$defs/Note" }));
    assert_eq!(save.input_schema["properties"]["also"], json!({ "type": "array", "items": { "$ref": "#/$defs/Note" } }));
    // Once each, the one a record names too; none the tool does not name.
    assert_eq!(save.input_schema["$defs"], json!({ "Note": note, "Tag": tag }));
    refs_local(&save.input_schema);
    // A result's record is said as it is, an object at the root: a record is made field by field,
    // so its fields are required, but nothing checks or drops other keys, so none are refused.
    let out = save.output_schema.clone().unwrap();
    let open = |mut r: Value| {
        r.as_object_mut().unwrap().remove("additionalProperties");
        r
    };
    assert_eq!(out, json!({ "$ref": "#/$defs/Note", "type": "object", "$defs": { "Note": open(note.clone()), "Tag": open(tag.clone()) } }));
    assert!(!save.wrapped);
    assert_eq!(save.structured(json!({ "id": 1 })), Some(json!({ "id": 1 })));
    assert_eq!(save.structured(json!([1])), None);
    refs_local(&out);
    // A record that names itself is a `$ref` to itself, and the walk ends.
    let grow = the(&tools, "grow");
    let tree = json!({
        "type": "object",
        "properties": { "value": { "type": "integer" }, "children": { "type": "array", "items": { "$ref": "#/$defs/Tree" }, "default": [] } },
        "required": ["value"],
        "additionalProperties": false
    });
    assert_eq!(grow.input_schema["$defs"], json!({ "Tree": tree }));
    refs_local(&grow.input_schema);
    // Two that name each other, each nullable by `?`.
    let lp = the(&tools, "pair");
    assert_eq!(lp.input_schema["$defs"]["Left"]["properties"]["right"], json!({ "anyOf": [{ "$ref": "#/$defs/Right" }, { "type": "null" }] }));
    assert_eq!(lp.input_schema["$defs"]["Right"]["properties"]["left"], json!({ "anyOf": [{ "$ref": "#/$defs/Left" }, { "type": "null" }] }));
    assert_eq!(lp.output_schema.as_ref().unwrap()["properties"]["result"]["anyOf"], json!([{ "$ref": "#/$defs/Right" }, { "type": "null" }]));
    assert!(lp.wrapped);
    refs_local(&lp.input_schema);
    refs_local(lp.output_schema.as_ref().unwrap());
    // Another name for a type is under `$defs` too, a nullable one by `anyOf`.
    let ids = the(&tools, "ids");
    assert_eq!(ids.input_schema["$defs"], json!({ "Id": { "type": ["integer", "string"] } }));
    assert_eq!(ids.input_schema["properties"]["more"], json!({ "anyOf": [{ "$ref": "#/$defs/Id" }, { "type": "null" }] }));
    assert_eq!(
        ids.output_schema,
        Some(json!({
            "type": "object",
            "properties": { "result": { "type": "array", "items": { "$ref": "#/$defs/Id" } } },
            "required": ["result"],
            "$defs": { "Id": { "type": ["integer", "string"] } }
        }))
    );
    let deep = json!({ "value": 1, "children": [{ "value": 2, "children": [{ "value": 3 }] }] });
    validated(&[
        (
            "a record".into(),
            save.input_schema.clone(),
            vec![json!({ "note": { "id": 1, "text": "é" }, "also": [] }), json!({ "note": { "id": 1, "text": "a", "role": "admin", "tags": [{ "name": "x" }], "note": null }, "also": [{ "id": 2, "text": "b" }] })],
            vec![
                json!({ "note": { "id": 1 }, "also": [] }),
                json!({ "note": { "id": 1, "text": "a", "role": "owner" }, "also": [] }),
                json!({ "note": { "id": 1, "text": "a", "tags": [{}] }, "also": [] }),
                json!({ "note": { "id": 1, "text": "a", "extra": 1 }, "also": [] }),
                json!({ "note": null, "also": [] }),
            ],
        ),
        ("a record's result".into(), out, vec![json!({ "id": 1, "text": "a" }), json!({ "id": 1, "text": "a", "kept": true })], vec![json!({ "id": 1 }), json!(null)]),
        ("a recursive record".into(), grow.input_schema.clone(), vec![json!({ "t": deep })], vec![json!({ "t": { "value": 1, "children": [{ "children": [] }] } }), json!({ "t": { "value": 1, "children": [{ "value": "2" }] } })]),
        ("two that name each other".into(), lp.input_schema.clone(), vec![json!({ "a": {} }), json!({ "a": { "right": { "left": { "right": null } } } })], vec![json!({ "a": { "right": { "left": 1 } } })]),
        ("an alias".into(), ids.input_schema.clone(), vec![json!({ "id": 1, "more": null }), json!({ "id": "a", "more": 2 })], vec![json!({ "id": 1.5, "more": 1 }), json!({ "id": 1 })]),
        ("a result that names a record that names itself".into(), lp.output_schema.clone().unwrap(), vec![json!({ "result": null }), json!({ "result": { "left": { "right": null } } })], vec![json!(null), json!({}), json!({ "result": 1 })]),
    ]);
    // A record says it has no other fields, as one drops them from what a client sends (B.2,
    // Smaller notes); a map type says which it requires, and nothing of others.
    let people = the(&tools, "people");
    assert_eq!(people.input_schema["$defs"]["Person"]["additionalProperties"], json!(false));
    assert_eq!(
        people.input_schema["properties"]["q"],
        json!({ "type": "object", "properties": { "name": { "type": "string" }, "age": { "type": ["integer", "null"] } }, "required": ["name"] })
    );
}

#[test]
fn b2_nullable_refs_and_unions_of_records() {
    let src = "type Cat = { kind: \"cat\", lives: int }\ntype Dog = { kind: \"dog\" }\n\
               /// Pets.\nfn mcp pets(one: Cat?, two: Cat | Dog, three: Cat | Dog | null, four: list<Cat?>, five: Cat | null) -> Cat | Dog with time = two\n";
    let tools = tools_of("b2-nullable", src);
    let p = &the(&tools, "pets").input_schema;
    let (cat, dog, nul) = (json!({ "$ref": "#/$defs/Cat" }), json!({ "$ref": "#/$defs/Dog" }), json!({ "type": "null" }));
    assert_eq!(p["properties"]["one"], json!({ "anyOf": [cat, nul] }));
    assert_eq!(p["properties"]["two"], json!({ "anyOf": [cat, dog] }));
    assert_eq!(p["properties"]["three"], json!({ "anyOf": [cat, dog, nul] }));
    assert_eq!(p["properties"]["four"], json!({ "type": "array", "items": { "anyOf": [cat, nul] } }));
    assert_eq!(p["properties"]["five"], p["properties"]["one"]);
    refs_local(p);
    // Nothing of OpenAPI's: no `nullable`, no `allOf` around a `$ref`, no `oneOf`.
    let text = p.to_string();
    assert!(!text.contains("nullable") && !text.contains("allOf") && !text.contains("oneOf") && !text.contains("x-quillo"), "{}", text);
    validated(&[(
        "nullable refs".into(),
        p.clone(),
        vec![json!({ "one": null, "two": { "kind": "dog" }, "three": null, "four": [null, { "kind": "cat", "lives": 9 }], "five": { "kind": "cat", "lives": 1 } })],
        vec![
            json!({ "one": { "kind": "cat" }, "two": { "kind": "dog" }, "three": null, "four": [], "five": null }),
            json!({ "one": null, "two": null, "three": null, "four": [], "five": null }),
            json!({ "one": null, "two": { "kind": "fish" }, "three": null, "four": [], "five": null }),
        ],
    )]);
}

#[test]
fn b2_the_openapi_document_is_as_before() {
    // The 2020-12 writer is the tools' own: `quillo docs` still writes OpenAPI 3.0's words.
    let doc = doc_of("b2-openapi", "type Note = { id: int, note: str? }\nserver fn mcp keep(n: Note?, s: str?) -> int | str with time = 1\n");
    let f = &doc["x-quillo-server-fns"]["keep"];
    assert_eq!(f["params"][1]["schema"], json!({ "type": "string", "nullable": true }));
    assert_eq!(f["params"][0]["schema"], json!({ "allOf": [{ "$ref": "#/components/schemas/Note" }], "type": "object", "nullable": true }));
    assert_eq!(f["returns"], json!({ "oneOf": [{ "type": "integer" }, { "type": "string" }] }));
    assert_eq!(doc["components"]["schemas"]["Note"]["properties"]["note"], json!({ "type": "string", "nullable": true }));
}

/// The hints of a tool, as the door gives them: `(readOnlyHint, openWorldHint)` where set.
fn hints_of(t: &Tool) -> (Option<bool>, Option<bool>) {
    (t.annotations.get("readOnlyHint").and_then(Value::as_bool), t.annotations.get("openWorldHint").and_then(Value::as_bool))
}

const HINTS: &str = "service billing = \"billing.qwe\"\n\
service pay = \"pay.qwe\"\n\
service far = \"far.qwe\"\n\
use \"pure.qwer\"\n\
site shop = \"https://shop.example.com\"\n\
let LIMIT = 20\n\
let handlers = { go: () => 1 }\n\
/// r\nfn mcp reads() -> list with db = db.query(\"select * from notes limit ?\", LIMIT)\n\
/// w\nfn mcp writes(t: str) with db = db.exec(\"insert into notes (text) values (?)\", t)\n\
/// f\nfn mcp files() with fs = read_file(\"a.txt\")\n\
/// c\nfn mcp clock() -> int with time = now()\n\
/// d\nfn mcp dice() -> int with time, random = random_int(1, 6)\n\
/// e\nfn mcp setting() with env = env(\"HOME\")\n\
/// h\nfn mcp fetches() with http = http.get(\"https://example.com\")\n\
/// b\nfn mcp bills(n: int) with http = billing.charge(n)\n\
/// l\nfn mcp bills_later(n: int) with http = billing.later.charge(n)\n\
/// s\nfn mcp shops() with http = shop.notes()\n\
/// n\nfn mcp sends() with net = udp.send(\"127.0.0.1\", 9, \"x\")\n\
/// bt\nfn mcp both() -> list with db, time = db.query(\"select * from notes\")\n\
/// m\nfn mcp mixed() with db, fs = db.query(\"select 1\")\n\
/// a\nfn mcp all_of() with db, http, net, fs, env, time, random = 1\n\
/// p\nfn mcp pure(a: int, b: int) -> int = a + b\n\
/// i\nfn mcp impure() -> int = now()\n\
/// u\nfn mcp blind(xs: list) = xs | map(handlers.go)\n\
/// ut\nfn mcp blind_time(xs: list) with time = xs | map(handlers.go)\n\
/// uh\nfn mcp blind_http(xs: list) with http = xs | map(handlers.go)\n\
/// ss\nfn mcp logout() with time = end_session()\n\
/// sp\nfn mcp logout_pure() = end_session()\n\
/// id\nfn mcp idle() with db = 1\n\
/// py\nfn mcp pays(n: int) with http = pay.charge(n)\n\
/// pl\nfn mcp pays_later(n: int) with http = pay.later.charge(n)\n\
/// fr\nfn mcp fars(n: int) with http = far.charge(n)\n\
/// nt\nfn mcp native_time() with time = double(2)\n\
/// np\nfn mcp native_pure() = double(2)\n\
/// ko\nfn mcp kinds_only() = db.error_kind(err(\"x\"))\n\
/// cn\nfn mcp connects() with db = db.connect(\"other.db\")\n\
/// sy\nfn mcp syncs() -> list with db = db.query(\"select sync()\")\n";

/// A service whose function reaches outside: the app's tool calling it does too.
const PAY: &str = "server fn charge(n: int) -> int with http {\n    http.get(\"https://payments.example.com\")\n    n\n}\n";
const PURE_QWER: &str = "//! effects: none\npub fn double(n: i64) -> i64 {\n    n * 2\n}\n";

const BILLING: &str = "server fn charge(n: int) -> int = n\n";

#[test]
fn b3_the_hints_of_each_with() {
    let tools = tools_in("b3-hints", &[("main.qwe", HINTS), ("billing.qwe", BILLING), ("pay.qwe", PAY), ("pure.qwer", PURE_QWER)]);
    let want: &[(&str, Option<bool>, Option<bool>, bool)] = &[
        // name, readOnlyHint, openWorldHint, run in a read-only transaction
        ("reads", Some(true), Some(false), true),
        ("writes", None, Some(false), false),
        ("files", None, Some(false), false),
        // Said to only read, and its `with` holds the database away: no transaction.
        ("clock", Some(true), Some(false), false),
        ("dice", Some(true), Some(false), false),
        ("setting", Some(true), Some(false), false),
        ("fetches", None, None, false),
        // The program's own service, whose code is read and stays inside: inside the program, and
        // it may write.
        ("bills", None, Some(false), false),
        ("bills_later", None, Some(false), false),
        // One whose code reaches outside, now or later, or is not here to read: outside.
        ("pays", None, None, false),
        ("pays_later", None, None, false),
        ("fars", None, None, false),
        // A site is another program on the web.
        ("shops", None, None, false),
        ("sends", None, None, false),
        ("both", Some(true), Some(false), true),
        ("mixed", None, Some(false), false),
        ("all_of", None, None, false),
        // No `with`, and nothing it does: certain. It reaches no database, so it runs with no
        // transaction, and works where none can be had.
        ("pure", Some(true), Some(false), false),
        ("kinds_only", Some(true), Some(false), false),
        // Another database is another place.
        ("connects", None, None, false),
        // A function its database does not ship, called in a `select`: it may write and reach
        // anywhere.
        ("syncs", None, None, false),
        // No `with`, and an effect, or code the checker cannot see: MCP's defaults.
        ("impure", None, None, false),
        ("blind", None, None, false),
        // A `with` holds what it names at run time, but not what code the checker cannot see
        // writes that no effect names, nor what a native plugin's Rust reaches, whatever its
        // file says.
        ("blind_time", None, None, false),
        ("blind_http", None, None, false),
        ("native_time", None, None, false),
        ("native_pure", None, None, false),
        // A session ended is a change, with a `with` or without.
        ("logout", None, Some(false), false),
        ("logout_pure", None, None, false),
        // `with db` and no database call seen: it only reads, and is still run in the
        // transaction, which is for the call the checker did not see.
        ("idle", Some(true), Some(false), true),
    ];
    for (name, ro, open, tx) in want {
        let t = the(&tools, name);
        assert_eq!(hints_of(t), (*ro, *open), "{}: {}", name, t.to_json());
        assert_eq!(t.reads_only, *tx, "{}", name);
        // Never the other two, which stay MCP's defaults, and never a default said.
        assert!(t.annotations.keys().all(|k| k == "readOnlyHint" || k == "openWorldHint"), "{:?}", t.annotations);
        assert!(t.annotations.get("readOnlyHint").map_or(true, |v| *v == json!(true)) && t.annotations.get("openWorldHint").map_or(true, |v| *v == json!(false)), "{:?}", t.annotations);
        assert_eq!(t.to_json().get("annotations").is_some(), ro.is_some() || open.is_some(), "{}", name);
    }
    assert_eq!(tools.len(), want.len());
}

/// What one tool `t`'s code was found to do, among other functions.
fn facts_of_t(name: &str, src: &str) -> quillo::check::ToolFacts {
    let facts = facts_in(name, &[("main.qwe", src), ("billing.qwe", BILLING)]);
    fact(&facts, "t").clone()
}

#[test]
fn q858_a_select_written_in_the_program_reads() {
    let reads = [
        "db.query(\"select * from notes\")",
        "db.one(\"select text from notes where id = ?\", id)",
        "db.one(\"SELECT text FROM notes WHERE id = :id\", { id })",
        // A template with nothing put in it is written in the program too.
        "db.query(`select * from notes`)",
        "\"select 1 as one\" | db.query()",
        "db.query(\"-- the newest first\\nselect * from notes order by id desc\")",
        "db.query(\"select * from notes where text = 'insert into x'\")",
        "db.query(\"select 'é ✓ 日本' as t\")",
        // What each database ships, which only computes.
        "db.one(\"select count(*) as n, coalesce(max(id), 0) as m from notes\")",
        "db.query(\"select lower(text), date('now'), now(), json_extract(meta, '$.a') from notes\")",
        "db.query(\"select cast(id as varchar(10)), round(avg(id), 2), strftime('%Y', at) from notes group by at having count(*) > 1\")",
        "db.query(\"select * from notes where id in (select id from notes) and exists (select 1)\")",
        // Reads through helpers, one that calls itself, and one given as a value.
        "first(id)",
        "deep(3)",
        "[1, 2] | map(one_of)",
        // What does not touch the database at all.
        "db.error_kind(err(\"x\"))",
    ];
    for body in reads {
        let src = format!(
            "fn first(id) = db.one(\"select * from notes where id = ?\", id)\nfn deep(n) = if n == 0 {{ db.query(\"select 1\") }} else {{ deep(n - 1) }}\nfn one_of(i) = db.one(\"select ? as i\", i)\n/// T.\nfn mcp t(id: int) with db = {}\n",
            body
        );
        let f = facts_of_t("q858-reads", &src);
        assert_eq!(f.writes, None, "{}", body);
        assert_eq!(f.uncertain, None, "{}", body);
    }
}

#[test]
fn q858_anything_else_counts_as_a_write() {
    let writes = [
        ("db.exec(\"insert into notes (text) values ('a')\")", "`db.exec`"),
        ("db.exec(\"select 1\")", "`db.exec`"),
        ("db.exec_many(\"insert into notes (text) values (?)\", [[\"a\"]])", "`db.exec_many`"),
        // The guide's own `billing.invoice`: a `db.one` that writes.
        ("db.one(\"insert into notes (text) values (?) returning id\", \"a\")", "is not a `select`"),
        ("db.query(\"update notes set text = 'b' returning id\")", "is not a `select`"),
        ("db.query(\"delete from notes\")", "is not a `select`"),
        // A `select` that writes or locks, as each database reads it.
        ("db.query(\"select * from notes for update\")", "is not a `select`"),
        ("db.query(\"select nextval('s')\")", "is not a `select`"),
        ("db.query(\"select * into copy from notes\")", "is not a `select`"),
        ("db.query(\"select 1; delete from notes\")", "is not a `select`"),
        // A `with` before it: counted as a write, though this one only reads.
        ("db.query(\"with x as (select 1) select * from x\")", "is not a `select`"),
        ("db.query(\"with x as (delete from notes returning *) select * from x\")", "is not a `select`"),
        ("db.query(\"\")", "is not a `select`"),
        // A statement built as the program runs, or kept in a variable.
        ("db.query(\"select * from \" + name)", "not written in the program"),
        ("db.query(`select * from ${name}`)", "not written in the program"),
        ("db.query(SQL)", "not written in the program"),
        ("db.query(sql_name(name))", "not written in the program"),
        ("[\"select 1\"] | map(db.query)", "not written in the program"),
        // Writes through a helper, two deep, and one given as a value.
        ("save(name)", "`db.exec`"),
        ("outer(name)", "`db.exec`"),
        ("[name] | map(save)", "`db.exec`"),
        ("db.transaction(() => db.query(\"select 1\"))", "`db.transaction`"),
        ("db.migrate()", "`db.migrate`"),
        ("db.connect(\"other.db\")", "`db.connect`"),
        ("idempotent(() => 1)", "`idempotent`"),
        ("with_tenant(\"a\", () => db.query(\"select 1\"))", "`with_tenant`"),
        // What the server keeps without the database.
        ("set_session({ user: 1 })", "`set_session`"),
        ("publish(\"notes\")", "`publish`"),
        ("bump()", "assigns the program's variable `hits`"),
        // A view a tool's code shows runs too.
        ("len(str(<Leak/>))", "`db.exec`"),
        // A local `fn` of the name in another block is no reason to pass over the program's.
        ("{\n    let f = () => {\n        fn wipe() = 0\n        wipe()\n    }\n    wipe()\n}", "`db.exec`"),
        // What the view cache says changes something, a builtin no effect names.
        ("media.close(\"cam\")", "`media.close` changes something"),
        // PostgreSQL's dblink writes over a connection of its own, outside the transaction.
        ("db.query(\"select * from dblink('dbname=x', 'delete from notes') as t(a int)\")", "calls `dblink`"),
        // A function its database does not ship: it may write, on a connection of its own too.
        ("db.query(\"select sync()\")", "calls `sync`"),
        // One the reader knows writes is said so.
        ("db.one(\"select pg_notify('notes', 'x') as n\")", "is not a `select`"),
        ("db.query(\"select count(*), lower(text), my_audit(id) from notes\")", "calls `my_audit`"),
        ("db.query(\"select public.archive_old()\")", "calls `"),
        ("db.query(\"select dblink_exec('dbname=x', 'delete from notes')\")", "is not a `select`"),
    ];
    for (body, why) in writes {
        let src = format!(
            "let SQL = \"select 1\"\nlet hits = 0\nfn wipe() = db.exec(\"delete from notes\")\nview Leak() = <b>{{db.exec(\"delete from notes\").changes}}</b>\nfn save(t) = db.exec(\"insert into notes (text) values (?)\", t)\nfn outer(t) = save(t)\nfn bump() {{\n    hits += 1\n    hits\n}}\n/// T.\nfn mcp t(name: str) with db = {}\n",
            body
        );
        let f = facts_of_t("q858-writes", &src);
        let w = f.writes.as_deref().unwrap_or_else(|| panic!("{} was not found to write", body));
        assert!(w.contains(why), "{}: {}", body, w);
    }
}

#[test]
fn q858_review_a_records_defaults_run_as_one_is_made() {
    // Taken, given back, in a list, inside another record, made by name, by a helper's parameter,
    // by a pattern, by a lambda's parameter: each runs the defaults, the door's arguments
    // too, before the tool's code.
    let src = "let n = 0\nfn next_no() {\n    n += 1\n    n\n}\n\
               type Ticket = { title: str, no: int = next_no() }\ntype Holder = { t: Ticket? }\n\
               type Row = { id: int = db.one(\"insert into ids default values returning id\").id }\n\
               type Plain = { title: str, at: int = 1 }\n\
               fn h(t: Ticket) = t.title\n\
               /// T.\nfn mcp takes(t: Ticket) with time = t.title\n\
               /// T.\nfn mcp many(ts: list<Ticket>) with time = len(ts)\n\
               /// T.\nfn mcp nested(x: Holder) with time = 1\n\
               /// T.\nfn mcp gives() -> Ticket? with time = null\n\
               /// T.\nfn mcp makes() with time = Ticket({ title: \"a\" }).title\n\
               /// T.\nfn mcp via(x: any) with time = h(x)\n\
               /// T.\nfn mcp matches(x: map) with time = match x {\n    Ticket as t => 1\n    _ => 0\n}\n\
               /// T.\nfn mcp lambda(xs: list) with time = xs | map((t: Ticket) => t.no)\n\
               /// T.\nfn mcp row(r: Row) with db = r.id\n\
               /// T.\nfn mcp plain(p: Plain) with time = p.title\n";
    let facts = facts_in("q858-defaults", &[("main.qwe", src)]);
    for name in ["takes", "many", "nested", "gives", "makes", "via", "matches", "lambda"] {
        assert_eq!(fact(&facts, name).writes.as_deref(), Some("assigns the program's variable `n`"), "{}", name);
    }
    assert!(fact(&facts, "row").writes.as_deref().is_some_and(|w| w.contains("is not a `select`")), "{:?}", fact(&facts, "row").writes);
    assert_eq!(fact(&facts, "plain").writes, None);
    let tools = tools_of("q858-defaults-hints", src);
    // `n`, assigned elsewhere, may hold anything by then: the rest is unseen too.
    assert_eq!(hints_of(the(&tools, "takes")), (None, None));
    assert!(!the(&tools, "takes").reads_only);
    assert_eq!(hints_of(the(&tools, "plain")), (Some(true), Some(false)));
}

#[test]
fn q858_what_the_checker_cannot_see() {
    let unseen = [
        ("f()", "calls a parameter or a variable"),
        ("{\n    let g = () => 1\n    g()\n}", "calls a parameter or a variable"),
        ("apply(() => 1)", "calls a parameter or a variable"),
        ("js(\"Chart\")", "`js` runs code"),
        ("run_script(\"1 + 1\")", "`run_script` runs code"),
        ("handlers.go()", "`.go(...)` calls a function a value holds"),
        ("[1] | map(handlers.go)", "may hold a function"),
        ("{\n    let d = db\n    d.exec(\"delete from notes\")\n}", "the module `db` is passed as a value"),
        ("pass(billing)", "`billing` is passed as a value"),
        ("[1] | map(billing.charge)", "`billing.charge` is passed as a value"),
        ("{ go: 1 }.go()", "`.go(...)` calls a function a value holds"),
        ("make()()", "calls a function an expression gives"),
        ("double(2)", "native plugin"),
    ];
    for (body, why) in unseen {
        let src = format!(
            "use \"pure.qwer\"\nservice billing = \"billing.qwe\"\nlet handlers = {{ go: () => 1 }}\nfn apply(h) = h()\nfn pass(x) = x\nfn make() = () => 1\n/// T.\nfn mcp t(f: any) with time = {}\n",
            body
        );
        let facts = facts_in("q858-unseen", &[("main.qwe", &src), ("billing.qwe", BILLING), ("pure.qwer", "//! effects: none\npub fn double(n: i64) -> i64 {\n    n * 2\n}\n")]);
        let f = fact(&facts, "t");
        let u = f.uncertain.as_deref().unwrap_or_else(|| panic!("{}: seen as certain", body));
        assert!(u.contains(why), "{}: {}", body, u);
    }
    // What it sees: its own `fn`s, lambdas, functions given by name, the program's values
    // written as data, a record made, a method that is a builtin's.
    let seen = [
        "{\n    fn inner(x) = x + 1\n    inner(1)\n}",
        "[1, 2] | map((x) => x * LIMIT)",
        "[1, 2] | map(twice)",
        "Note({ id: 1 })",
        "\"a\".upper()",
        "NAMES | map((n) => n.len())",
        "((x) => x)(1)",
        "CONFIG.size + len(f)",
        "math.pi * 2",
        "len(str(<Badge n={1}/>))",
    ];
    for body in seen {
        let src = format!(
            "type Note = {{ id: int }}\nview Badge(n) = <b>{{n}}</b>\nlet LIMIT = 20\nlet NAMES = [\"a\", \"b\"]\nlet CONFIG = {{ size: 3, name: \"x\" }}\nfn twice(x) = x * 2\n/// T.\nfn mcp t(f: any) with time = {}\n",
            body
        );
        let f = fact(&facts_in("q858-seen", &[("main.qwe", &src)]), "t").clone();
        assert_eq!(f.uncertain, None, "{}", body);
        assert_eq!(f.writes, None, "{}", body);
    }
}

#[test]
fn q858_the_tools_list_in_order_across_files() {
    let tools = tools_in(
        "q858-files",
        &[("main.qwe", "/// A.\nfn mcp a() -> int = 1\nuse \"lib.qwe\"\n/// C.\nserver fn mcp c() -> int = 3\nfn mcp(x) = x\n"), ("lib.qwe", "/// B.\nfn mcp b() -> int = 2\n")],
    );
    let list = mcp::list(&tools);
    let names: Vec<&str> = list.as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["a", "b", "c"]);
    assert_eq!(
        list[0],
        json!({
            "name": "a",
            "description": "A.",
            "inputSchema": { "type": "object", "additionalProperties": false },
            "outputSchema": { "type": "object", "properties": { "result": { "type": "integer" } }, "required": ["result"] },
            "annotations": { "readOnlyHint": true, "openWorldHint": false }
        })
    );
    // A program with errors gives them, as `quillo check` says them.
    let d = dir("q858-errors", "/// X.\nfn mcp x(f: fn) = 1\n");
    let e = mcp::of_source(&d.join("main.qwe")).unwrap_err();
    let _ = std::fs::remove_dir_all(&d);
    assert!(e.contains("cannot come as JSON"), "{}", e);
    // None at all.
    assert!(tools_of("q858-none", "fn plain() = 1\n").is_empty());
}

// ---- the read-only transaction ---------------------------------------------------------------

/// A program on a database, run, and its tools.
fn run_on(d: &Path, db: &str, setup: &[&str], tools: &str) -> (Interp, Vec<Tool>) {
    let mut src = format!("db.open({:?})\n", db);
    for s in setup {
        src.push_str(&format!("db.exec({:?})\n", s));
    }
    src.push_str(tools);
    std::fs::write(d.join("main.qwe"), &src).unwrap();
    let list = mcp::of_source(&d.join("main.qwe")).unwrap_or_else(|e| panic!("{}\n{}", e, src));
    let mut it = Interp::new(d);
    it.load_file(&d.join("main.qwe")).unwrap_or_else(|e| panic!("{}\n{}", e, src));
    (it, list)
}

fn global(it: &Interp, name: &str) -> Q {
    quillo::value::lookup(&it.globals, name).unwrap_or_else(|| panic!("no {}", name))
}

/// A call's value, or its failure said.
fn shown(r: Result<Q, quillo::interp::Ctrl>) -> Result<Q, String> {
    match r {
        Ok(v) => Ok(v),
        Err(quillo::interp::Ctrl::Error(e)) => Err(e.msg.clone()),
        Err(_) => Err("a return or a reply".into()),
    }
}

fn ok(r: Result<Q, quillo::interp::Ctrl>) -> Q {
    shown(r).unwrap_or_else(|e| panic!("{}", e))
}

/// A tool's answer through `mcp::call`, its arguments taken.
fn answer(r: Result<Result<Q, String>, quillo::interp::Ctrl>) -> Q {
    match r {
        Ok(Ok(v)) => v,
        Ok(Err(why)) => panic!("refused: {}", why),
        Err(e) => panic!("{}", shown(Err(e)).err().unwrap_or_default()),
    }
}

fn count(it: &mut Interp) -> i64 {
    let f = global(it, "count");
    match shown(it.call(&f, vec![], 0)) {
        Ok(Q::Int(n)) => n,
        other => panic!("count: {:?}", other.map(|v| v.display())),
    }
}

/// The tools every database is tried with: one that reads, one the checker misjudges (a
/// function of the database's own that writes, called in a `select`), and the program's
/// helpers.
const RO_TOOLS: &str = "/// The notes.\nfn mcp notes() -> list with db = db.query(\"select text from notes order by id\")\n\
/// Looks.\nfn mcp sneaky() with db = db.query(\"select add_note() as id\")\n\
fn count() -> int = db.one(\"select count(*) as n from notes\").n\n\
fn kind(e) = [db.error_kind(e), error(e)]\n\
fn write(t: str) {\n    let r = db.exec(\"insert into notes (text) values (?)\", t)\n    db.error_kind(r) ?? \"wrote\"\n}\n\
fn fails() = unwrap(err(\"no\"))\n\
/// Adds.\nfn mcp adds(t: str) with db = write(t)\n\
type Ticket = { title: str, no: int? = db.one(\"select add_note() as id\").id }\n\
/// Tickets.\nfn mcp ticket(t: Ticket) with db = t.title\n\
fn arg() = { title: \"x\" }\n\
type Stamp = { n: int = db.exec(\"insert into notes (text) values ('stamp')\").changes }\n\
/// Recent.\nfn mcp recent(q: Stamp) -> int with time = q.n\n\
type Archive = { n: int = db.one(\"select add_note() as n\").n }\n\
/// Recent.\nfn mcp recent_db(q: Archive) -> int with time = q.n\n\
fn empty() = {}\n\
fn make() = Ticket({ title: \"y\" }).title\n\
fn kinds() = [\n    db.error_kind(err(\"db: Server error: `ERROR 1062 (23000): Duplicate entry 'x(1792)' for key 'v''\")),\n    db.error_kind(err(\"db: Server error: `ERROR 1792 (25006): Cannot execute statement in a READ ONLY transaction'\")),\n    db.error_kind(err(\"db: no such table: error 1792 (x\")),\n    db.error_kind(err(\"db: attempt to write a readonly database\")),\n]\n";

/// What each database is checked for: the tool that reads gives its rows in the transaction;
/// the one misjudged is refused by the database, as `read_only`, and writes nothing, where run
/// as a plain call it writes; a write in the transaction is refused, and one after it is not, a
/// transaction that failed too; and a read-only transaction inside another is an error.
fn read_only_on(it: &mut Interp, tools: &[Tool], misjudged: bool) {
    let notes = the(tools, "notes");
    assert!(notes.reads_only);
    let rows = answer(mcp::call(it, notes, vec![], 0));
    assert_eq!(rows.display(), "[{ text: \"a\" }]");
    // A record's default that writes, in a tool whose `with` has no `db`: made under that
    // `with`, it fails as the body would, and writes nothing (found in review).
    let recent = the(tools, "recent");
    assert_eq!(hints_of(recent), (None, Some(false)));
    let empty = ok(it.call(&global(it, "empty"), vec![], 0));
    let n = count(it);
    let failed = mcp::call(it, recent, vec![empty.clone()], 0);
    assert!(matches!(&failed, Err(quillo::interp::Ctrl::Error(e)) if e.msg.contains("needs the db effect, but `recent` is declared `with time`")), "{:?}", shown(failed.map(|_| Q::Nil)).map(|v| v.display()));
    assert_eq!(count(it), n);
    if misjudged {
        // A function its database does not ship, in a `select`: no longer taken for a read, nor
        // for staying inside.
        let sneaky = the(tools, "sneaky");
        assert!(!sneaky.reads_only);
        assert_eq!(hints_of(sneaky), (None, None));
        // Were it misjudged, the transaction refuses its write, as `read_only`.
        let f = global(it, "sneaky");
        let r = ok(mcp::reading_only(it, 0, &mut |it| it.call(&f, vec![], 0)));
        assert!(matches!(r, Q::Err(..)), "{}", r.display());
        let kind = global(it, "kind");
        let k = ok(it.call(&kind, vec![r], 0)).display();
        assert!(k.starts_with("[\"read_only\", \"db: "), "{}", k);
        assert_eq!(count(it), n);
        // As a plain call, it writes: the transaction is what refused it.
        let wrote = ok(it.call(&f, vec![], 0));
        assert!(!matches!(wrote, Q::Err(..)), "{}", wrote.display());
        assert_eq!(count(it), n + 1);
        // A record's default runs as the door makes the argument: inside the transaction, so
        // the write is refused there too, the call with it.
        let ticket = the(tools, "ticket");
        assert!(!ticket.reads_only);
        let mut misjudged_ticket = ticket.clone();
        misjudged_ticket.reads_only = true;
        let arg = ok(it.call(&global(it, "arg"), vec![], 0));
        let made = mcp::call(it, &misjudged_ticket, vec![arg], 0);
        assert!(!matches!(made, Ok(Ok(_))), "{:?}", made.ok().map(|r| r.map(|v| v.display())));
        assert_eq!(count(it), n + 1);
        // Made outside it, the record writes.
        let make = global(it, "make");
        assert_eq!(ok(it.call(&make, vec![], 0)).display(), "y");
        assert_eq!(count(it), n + 2);
        // The review's own case: a `select` of a function that writes, in a default of a tool
        // `with time`: neither hint, and the call fails before the write.
        let recent_db = the(tools, "recent_db");
        assert_eq!(hints_of(recent_db), (None, None));
        assert!(mcp::call(it, recent_db, vec![empty], 0).is_err());
        assert_eq!(count(it), n + 2);
    }
    let before = count(it);
    let write = global(it, "write");
    let refused = ok(mcp::reading_only(it, 0, &mut |it| it.call(&write, vec![Q::str("é ✓")], 0)));
    assert_eq!(refused.display(), "read_only");
    assert_eq!(count(it), before);
    // A transaction that failed ends as one that worked: read-only no more.
    let fails = global(it, "fails");
    let failed = shown(mcp::reading_only(it, 0, &mut |it| it.call(&fails, vec![], 0)));
    assert!(failed.is_err() || matches!(failed, Ok(Q::Err(..))), "{:?}", failed.map(|v| v.display()));
    assert_eq!(ok(it.call(&write, vec![Q::str("b")], 0)).display(), "wrote");
    assert_eq!(count(it), before + 1);
    // One inside another, read-only or not, is refused, and the outer one goes on.
    let inner = mcp::reading_only(it, 0, &mut |it| match shown(mcp::reading_only(it, 0, &mut |_| Ok(Q::Nil))) {
        Err(e) => Ok(Q::str(e)),
        Ok(v) => panic!("{}", v.display()),
    });
    assert_eq!(ok(inner).display(), "a read-only transaction cannot be inside another");
    let begin = global(it, "begin_then");
    let nested = ok(it.call(&begin, vec![], 0));
    assert!(!matches!(nested, Q::Err(..)), "{}", nested.display());
    // With an isolation level too: still refused, and a read still answers.
    let serializable = quillo::txn::Options { isolation: Some(quillo::txn::Isolation::Serializable), read_only: true, ..Default::default() };
    let n = count(it);
    assert_eq!(ok(quillo::txn::run(it, None, &serializable, 0, &mut |it| it.call(&write, vec![Q::str("c")], 0))).display(), "read_only");
    let notes_fn = global(it, "notes");
    assert!(matches!(ok(quillo::txn::run(it, None, &serializable, 0, &mut |it| it.call(&notes_fn, vec![], 0))), Q::List(_)));
    assert_eq!(count(it), n);
    // A tool that writes is called as it is, with no transaction; one the program does not have
    // is said.
    let adds = the(tools, "adds");
    assert!(!adds.reads_only);
    assert_eq!(answer(mcp::call(it, adds, vec![Q::str("d")], 0)).display(), "wrote");
    assert_eq!(count(it), n + 1);
    let mut gone = adds.clone();
    gone.name = "gone".into();
    assert_eq!(shown(mcp::call(it, &gone, vec![], 0).map(|_| Q::Nil)).err().as_deref(), Some("the program has no tool `gone`"));
    // An argument the door refuses is said, the tool not run.
    assert!(matches!(mcp::call(it, adds, vec![Q::Int(1)], 0), Ok(Err(why)) if why.contains("argument `t` of `adds` must be str")));
    assert_eq!(count(it), n + 1);
    // A tool said to only read that has no `with`, in a program with a database: run in the
    // transaction, as nothing else holds the database away from it.
    let Q::Func(c) = global(it, "write") else { panic!("write") };
    let mut bare = adds.clone();
    bare.name = "write".into();
    bare.def = c.def.clone();
    bare.reads_only = true;
    assert_eq!(answer(mcp::call(it, &bare, vec![Q::str("e")], 0)).display(), "read_only");
    bare.reads_only = false;
    assert_eq!(answer(mcp::call(it, &bare, vec![Q::str("e")], 0)).display(), "wrote");
    assert_eq!(count(it), n + 2);
}

#[test]
fn q858_the_read_only_transaction_on_sqlite() {
    let d = support::temp_dir("feedback56-q858-sqlite");
    let tools = format!("{}{}", RO_TOOLS, SQLITE_EXTRA);
    let (mut it, list) = run_on(&d, &d.join("app.db").display().to_string(), &["create table notes (id integer primary key, text text)", "insert into notes (text) values ('a')"], &tools);
    // SQLite has no functions of the program's to misjudge: the write is the program's own.
    read_only_on(&mut it, &list, false);
    // `query_only` is put back as it was, on as well as off.
    ok(it.call(&global(&it, "lock"), vec![], 0));
    let write = global(&it, "write");
    assert_eq!(ok(mcp::reading_only(&mut it, 0, &mut |it| it.call(&write, vec![Q::str("z")], 0))).display(), "read_only");
    assert_eq!(ok(it.call(&global(&it, "query_only"), vec![], 0)).display(), "1");
    ok(it.call(&global(&it, "unlock"), vec![], 0));
    assert_eq!(ok(it.call(&global(&it, "query_only"), vec![], 0)).display(), "0");
    // The kind is told by a database's code or its own words at the start, not by a value in
    // its message.
    assert_eq!(ok(it.call(&global(&it, "kinds"), vec![], 0)).display(), "[\"unique\", \"read_only\", null, \"read_only\"]");
    drop(it);
    let _ = std::fs::remove_dir_all(&d);
}

const SQLITE_EXTRA: &str = "fn begin_then() = db.transaction(() => db.query(\"select 1\"))\n\
fn query_only() = db.one(\"pragma query_only\").query_only\n\
fn lock() = db.exec(\"pragma query_only = 1\")\n\
fn unlock() = db.exec(\"pragma query_only = 0\")\n";

/// A panic in a transaction's body, a read-only one and one of `db.transaction`'s, a nested one
/// too: what it raised is lowered and what it held rolled back, so a write after it is committed,
/// as another connection to the database sees (found in review).
fn panics_on(it: &mut Interp, d: &Path, db: &str) {
    let seen_elsewhere = |d: &Path| {
        let mut other = Interp::new(d);
        other.load_source(&format!("db.open({:?})\nfn count() -> int = db.one(\"select count(*) as n from notes\").n\n", db), "other.qwe").unwrap();
        count(&mut other)
    };
    let write = global(it, "write");
    let plain = quillo::txn::Options::default();
    for (what, outer) in [("read-only", true), ("db.transaction's", false), ("nested", false)] {
        let before = count(it);
        let free_before = free(it);
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let body = &mut |_: &mut Interp| -> Result<Q, quillo::interp::Ctrl> { panic!("a panic in the body, on purpose") };
            let _ = match (outer, what) {
                (true, _) => mcp::reading_only(it, 0, body),
                (false, "nested") => quillo::txn::run(it, None, &plain, 0, &mut |it| quillo::txn::run(it, None, &plain, 0, &mut |_| panic!("a panic in the body, on purpose"))),
                _ => quillo::txn::run(it, None, &plain, 0, body),
            };
        }));
        assert!(caught.is_err(), "{}", what);
        assert_eq!(it.sqlite_tx, 0, "{}", what);
        assert!(!quillo::httpd::held(), "{}", what);
        // Nothing pinned: the nested case passed when the inner transaction never lowered its
        // depth, the outer leaving the connection pinned and the write after it autocommitted
        // on it (found in review).
        nothing_left(it, free_before, what);
        assert_eq!(ok(it.call(&write, vec![Q::str("after")], 0)).display(), "wrote", "{}", what);
        assert_eq!(count(it), before + 1, "{}", what);
        assert_eq!(seen_elsewhere(d), before + 1, "{}: the write after it is not committed", what);
    }
}

#[test]
fn q858_review_a_tool_that_reaches_no_database_runs_with_none() {
    // A database per tenant, and no tenant (as over stdio): a tool that reaches no database is
    // not stopped by one it cannot have; one `with db` is run in the transaction and says why.
    let d = support::temp_dir("feedback56-q858-tenantless");
    let src = format!(
        "db.open({:?})\n/// Adds.\nfn mcp add(a: int, b: int) -> int = a + b\n/// Idle.\nfn mcp idle() with db = 1\n",
        d.join("t_{tenant}.db").display().to_string()
    );
    std::fs::write(d.join("main.qwe"), &src).unwrap();
    let tools = mcp::of_source(&d.join("main.qwe")).unwrap();
    let mut it = Interp::new(&d);
    it.load_file(&d.join("main.qwe")).unwrap();
    let add = the(&tools, "add");
    assert!(!add.reads_only);
    assert_eq!(answer(mcp::call(&mut it, add, vec![Q::Int(1), Q::Int(2)], 0)).display(), "3");
    let idle = the(&tools, "idle");
    assert!(idle.reads_only);
    let r = answer(mcp::call(&mut it, idle, vec![], 0));
    assert!(matches!(r, Q::Err(..)), "{}", r.display());
    drop(it);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn q858_review_each_function_is_read_once_and_services_only_for_a_tool() {
    // Thirty tools, each calling the first of thirty helpers that call the next: each function's
    // code is read once, not once for each tool that reaches it.
    let mut src = String::new();
    for i in 0..30 {
        src.push_str(&format!("fn h{}(x) = {}\n", i, if i == 29 { "x".to_string() } else { format!("h{}(x) + 1", i + 1) }));
    }
    for i in 0..30 {
        src.push_str(&format!("/// T.\nfn mcp t{}(x: int) with time = h0(x)\n", i));
    }
    let d = dir("q858-once", &src);
    let a = quillo::check::check_file(&d.join("main.qwe"));
    let _ = std::fs::remove_dir_all(&d);
    assert_eq!(a.tools.len(), 30);
    assert_eq!(a.walked, 60);
    // A service is walked only when a tool calls it: not for a program without tools, nor for a
    // tool that does not call it.
    let walked = |main: &str| {
        let d = dir("q858-services", main);
        std::fs::write(d.join("billing.qwe"), "server fn charge(n: int) -> int = n\nserver fn refund(n: int) -> int = n\n").unwrap();
        let a = quillo::check::check_file(&d.join("main.qwe"));
        let _ = std::fs::remove_dir_all(&d);
        assert!(a.errors().next().is_none(), "{:?}", a.diagnostics);
        a.walked
    };
    assert_eq!(walked("service billing = \"billing.qwe\"\nserver fn pay() with http = billing.charge(1)\n"), 0);
    assert_eq!(walked("service billing = \"billing.qwe\"\n/// T.\nfn mcp t() with time = 1\n"), 1);
    assert_eq!(walked("service billing = \"billing.qwe\"\n/// T.\nfn mcp t() with http = billing.charge(1)\n/// U.\nfn mcp u() with http = billing.refund(1)\n"), 4);
}

#[test]
fn q858_review_a_panic_puts_sqlite_back_as_it_was() {
    let d = support::temp_dir("feedback56-q858-panic");
    let tools = format!("{}{}", RO_TOOLS, SQLITE_EXTRA);
    let db = d.join("app.db").display().to_string();
    let (mut it, _) = run_on(&d, &db, &["create table notes (id integer primary key, text text)"], &tools);
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = mcp::reading_only(&mut it, 0, &mut |_| panic!("a panic in the body, on purpose"));
    }));
    assert!(caught.is_err());
    // The shared connection is not left refusing every write, nor in a savepoint.
    assert_eq!(ok(it.call(&global(&it, "query_only"), vec![], 0)).display(), "0");
    panics_on(&mut it, &d, &db);
    drop(it);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn q858_the_read_only_transaction_on_postgres() {
    let Some(pg) = support::postgres("feedback56-q858") else {
        eprintln!("skipped: q858_the_read_only_transaction_on_postgres, as PostgreSQL's initdb and pg_ctl were not found");
        return;
    };
    let url = pg.database("ro");
    let d = support::temp_dir("feedback56-q858-pg");
    let tools = format!("{}fn begin_then() = db.transaction(() => db.query(\"select 1\"))\n", RO_TOOLS);
    let (mut it, list) = run_on(
        &d,
        &url,
        &[
            "create table notes (id serial primary key, text text)",
            "insert into notes (text) values ('a')",
            "create function add_note() returns int language sql as $$ insert into notes (text) values ('x') returning id $$",
        ],
        &tools,
    );
    read_only_on(&mut it, &list, true);
    panics_on(&mut it, &d, &url);
    drop(it);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn q858_the_read_only_transaction_on_mariadb() {
    let Some(my) = support::mariadb("feedback56-q858") else {
        eprintln!("skipped: q858_the_read_only_transaction_on_mariadb, as MariaDB's mariadbd and mariadb-install-db were not found");
        return;
    };
    let url = my.url("ro");
    let d = support::temp_dir("feedback56-q858-my");
    let tools = format!("{}fn begin_then() = db.transaction(() => db.query(\"select 1\"))\n", RO_TOOLS);
    let (mut it, list) = run_on(
        &d,
        &url,
        &[
            "create table notes (id int auto_increment primary key, text text)",
            "insert into notes (text) values ('a')",
            "create function add_note() returns int modifies sql data begin insert into notes (text) values ('x'); return 1; end",
            "create table u (v varchar(20) unique)",
        ],
        &format!("{}fn dup() = db.error_kind(db.exec(\"insert into u (v) values ('x(1792)')\"))\n", tools),
    );
    read_only_on(&mut it, &list, true);
    // A duplicate whose value holds `(1792)` is a duplicate.
    let dup = global(&it, "dup");
    assert_eq!(ok(it.call(&dup, vec![], 0)).display(), "null");
    assert_eq!(ok(it.call(&dup, vec![], 0)).display(), "unique");
    panics_on(&mut it, &d, &url);
    drop(it);
    let _ = std::fs::remove_dir_all(&d);
}

// ---- Slice 4: the stdio door, `quillo mcp file.qwe` (C.1) ------------------------------------
//
// Driven as an AI client drives it: `quillo mcp` a child process, newline-delimited JSON-RPC
// written to its stdin and read from its stdout, every read and every wait under a deadline, and
// the process killed when the test is done with it (`support::Guard`). The messages are those of
// MCP's revision 2026-07-28 (schema/2026-07-28/schema.ts).

use std::io::{BufRead as _, Write as _};
use std::sync::{Arc, Mutex};

/// How long a test waits for a line, or for the door to end.
const WAIT: Duration = Duration::from_secs(30);

/// The `_meta` every request of revision 2026-07-28 carries.
fn meta() -> Value {
    json!({ "io.modelcontextprotocol/protocolVersion": "2026-07-28", "io.modelcontextprotocol/clientCapabilities": {} })
}

/// A request of 2026-07-28, its `_meta` put in its `params`.
fn request(id: Value, method: &str, mut params: Value) -> String {
    params["_meta"] = meta();
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string()
}

/// A tool's call, its id the tool's name.
fn call_line(name: &str, args: Value) -> String {
    request(json!(name), "tools/call", json!({ "name": name, "arguments": args }))
}

/// `quillo mcp` started as a client starts it, its pipes held.
struct Stdio {
    guard: support::Guard,
    stdin: Option<std::process::ChildStdin>,
    out: std::sync::mpsc::Receiver<String>,
    err: Arc<Mutex<String>>,
    err_reader: Option<std::thread::JoinHandle<()>>,
}

impl Stdio {
    fn start(d: &Path, args: &[&str]) -> Stdio {
        Stdio::start_env(d, args, &[])
    }

    fn start_env(d: &Path, args: &[&str], env: &[(&str, &str)]) -> Stdio {
        let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
            .arg("mcp")
            .args(args)
            .envs(env.iter().copied())
            .current_dir(d)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let (tx, out) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for l in std::io::BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(l).is_err() {
                    break;
                }
            }
        });
        let err = Arc::new(Mutex::new(String::new()));
        let e = err.clone();
        let err_reader = std::thread::spawn(move || {
            let mut r = std::io::BufReader::new(stderr);
            let mut line = String::new();
            while r.read_line(&mut line).is_ok_and(|n| n > 0) {
                e.lock().unwrap().push_str(&line);
                line.clear();
            }
        });
        Stdio { guard: support::Guard::new(child), stdin, out, err, err_reader: Some(err_reader) }
    }

    /// Writes `text` as it is: a message's line ends with `\n`. A door that has ended already (one
    /// that refuses to start may, before the test writes) closed its stdin: what it would have
    /// answered is missed by `next`, which says so (found running the file at once).
    fn write(&mut self, text: &[u8]) {
        let s = self.stdin.as_mut().expect("stdin was closed");
        if let Err(e) = s.write_all(text).and_then(|_| s.flush()) {
            assert_eq!(e.kind(), std::io::ErrorKind::BrokenPipe, "{}", e);
        }
    }

    fn send(&mut self, line: &str) {
        self.write(format!("{}\n", line).as_bytes());
    }

    /// The next line of stdout, which must be JSON (read as deep as it goes: an answer may be
    /// deeper than serde's own limit): None once stdout ends, or when nothing came in time.
    fn next_within(&self, wait: Duration) -> Option<Value> {
        self.out.recv_timeout(wait).ok().map(|l| {
            let mut de = serde_json::Deserializer::from_str(&l);
            de.disable_recursion_limit();
            <Value as serde::Deserialize>::deserialize(&mut de).unwrap_or_else(|e| panic!("not JSON on stdout ({}): {:?}", e, l.get(..300).unwrap_or(&l)))
        })
    }

    fn next(&self) -> Option<Value> {
        self.next_within(WAIT)
    }

    /// The answer to `line`.
    fn ask(&mut self, line: &str) -> Value {
        self.send(line);
        self.next().unwrap_or_else(|| panic!("no answer to {}\nstderr: {}", line, self.stderr()))
    }

    /// A tool's result: its `result`, the call's id checked and `_meta` taken out.
    fn call(&mut self, name: &str, args: Value) -> Value {
        let a = self.ask(&call_line(name, args));
        assert_eq!(a["id"], json!(name), "{}", a);
        let mut r = a["result"].clone();
        assert!(r.is_object(), "not a result: {}", a);
        assert_eq!(r["resultType"], json!("complete"), "{}", a);
        assert_eq!(r["_meta"]["io.modelcontextprotocol/serverInfo"]["name"], json!("main"), "{}", a);
        r.as_object_mut().unwrap().remove("_meta");
        r.as_object_mut().unwrap().remove("resultType");
        r
    }

    /// A protocol error's id (None when it has none), code and message.
    fn refusal(&mut self, line: &str) -> (Option<Value>, i64, String) {
        let a = self.ask(line);
        assert!(a.get("result").is_none(), "answered {} with a result: {}", line, a);
        assert_eq!(a["jsonrpc"], json!("2.0"));
        (a.get("id").cloned(), a["error"]["code"].as_i64().unwrap_or_else(|| panic!("{}", a)), a["error"]["message"].as_str().unwrap_or_default().to_string())
    }

    fn stderr(&self) -> String {
        self.err.lock().unwrap().clone()
    }

    /// Waits until stderr says `what`.
    fn says(&self, what: &str) {
        let deadline = Instant::now() + WAIT;
        while !self.stderr().contains(what) {
            assert!(Instant::now() < deadline, "stderr never said {:?}: {}", what, self.stderr());
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn close(&mut self) {
        self.stdin = None;
    }

    fn pid(&self) -> u32 {
        self.guard.0.id()
    }

    /// Waits for it to end, under the deadline: its status, None when it did not end (it is
    /// killed when dropped). Its stderr is all read then.
    fn ended(&mut self) -> Option<i32> {
        let deadline = Instant::now() + WAIT;
        loop {
            if let Some(s) = self.guard.0.try_wait().unwrap() {
                if let Some(r) = self.err_reader.take() {
                    let _ = r.join();
                }
                return s.code();
            }
            if Instant::now() > deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// `quillo mcp <args>` in `d` with its stdin closed: its status, stdout and stderr.
fn mcp_exits(d: &Path, args: &[&str]) -> (Option<i32>, Vec<Value>, String) {
    let mut s = Stdio::start(d, args);
    s.close();
    let code = s.ended();
    let mut out = Vec::new();
    while let Some(v) = s.next_within(Duration::from_millis(200)) {
        out.push(v);
    }
    (code, out, s.stderr())
}

/// The program the door is driven with: a tool of each kind of answer.
const DOOR_SRC: &str = r#"db.open(":memory:")
db.exec("create table notes (id integer primary key, text text not null)")
print("top level, to stderr")
print(`{"jsonrpc":"2.0","id":99,"result":{}}`)
print("two\nlines")

/// Adds a note and gives back all notes.
fn mcp add_note(text: str) -> list ! str with db {
    print(`adding ${text}`)
    if trim(text) == "" { return err("empty note") }
    db.exec("insert into notes (text) values (?)", text)?
    db.query("select id, text from notes order by id")
}

/// A greeting.
fn mcp hello(name: str, greeting: str = "hi", end: str = "!") with time = `${greeting} ${name}${end}`

/// A note by its number.
fn mcp lookup(id: int) -> str ! { code: int, why: str } with time = err({ code: 3, why: `no note ${id}` })

/// Needs a login.
fn mcp mine() -> str with time {
    session().user ?? reply 401 "Log in first"
}

/// Gone.
fn mcp gone() with time {
    reply 404
}

/// Taken.
fn mcp taken() with time {
    reply 409 { why: "taken" }
}

let pages = [(u) => redirect(u), () => <p>hi</p>]

/// A page, which the checker cannot see.
fn mcp page(which: int) with time = if which == 0 { pages[0]("/x") } else { pages[1]() }

/// Breaks in a builtin.
fn mcp boom(n: int) -> int with time = int("x" + str(n))?

/// Fails.
fn mcp crash() with time = unwrap(err("on purpose"))

/// A script's.
fn mcp leave() with env = exit(3)

/// A script's.
fn mcp piped() with env = stdin()

/// A script's.
fn mcp line() with env = read_line()

/// Nothing to take.
fn mcp clock() -> int with time = 7

type Point = { x: int, y: int = 0 }

/// Moves.
fn mcp moved(p: Point, by: int = 1) -> Point with time = Point({ x: p.x + by, y: p.y + by })

/// Echo, untyped.
fn mcp echo(v: any) with time = v

/// Its arguments.
fn mcp given() with env = args()

/// Whether writes are refused now.
fn mcp locked() -> int with db = db.one("select query_only as q from pragma_query_only").q

/// Whether writes are refused while it writes.
fn mcp unlocked(t: str) -> int with db {
    db.exec("insert into notes (text) values (?)", t)
    db.one("select query_only as q from pragma_query_only").q
}

/// Waits.
fn mcp wait(ms: int) -> int with time {
    print("waiting")
    sleep(ms)
    ms
}
"#;

fn door_dir(name: &str) -> PathBuf {
    dir(&format!("c1-{}", name), DOOR_SRC)
}

#[test]
fn c1_discover_list_and_call() {
    let d = door_dir("door");
    let mut door = Stdio::start(&d, &["main.qwe"]);
    // `server/discover`: the one version, the one capability, how long both hold, who answers.
    let found = door.ask(&request(json!(1), "server/discover", json!({})));
    assert_eq!(found["id"], json!(1));
    let r = &found["result"];
    assert_eq!(r["resultType"], json!("complete"));
    assert_eq!(r["supportedVersions"], json!(["2026-07-28"]));
    assert_eq!(r["capabilities"], json!({ "tools": {} }));
    assert_eq!((r["ttlMs"].clone(), r["cacheScope"].clone()), (json!(3_600_000), json!("public")));
    let who = &r["_meta"]["io.modelcontextprotocol/serverInfo"];
    assert_eq!(who["name"], json!("main"));
    assert!(who["version"].as_str().is_some_and(|v| v.starts_with(env!("CARGO_PKG_VERSION"))), "{}", who);
    // Part D's instructions are slice 6's.
    assert!(r.get("instructions").is_none(), "{}", r);
    // `tools/list`: slice 3's tools as `mcp::list` writes them, in source order, in one page.
    let listed = door.ask(&request(json!("list"), "tools/list", json!({})));
    let r = &listed["result"];
    assert_eq!(r["tools"], mcp::list(&mcp::of_source(&d.join("main.qwe")).unwrap()));
    let names: Vec<&str> = r["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["add_note", "hello", "lookup", "mine", "gone", "taken", "page", "boom", "crash", "leave", "piped", "line", "clock", "moved", "echo", "given", "locked", "unlocked", "wait"]);
    assert_eq!((r["resultType"].clone(), r["ttlMs"].clone(), r["cacheScope"].clone()), (json!("complete"), json!(3_600_000), json!("public")));
    assert!(r.get("nextCursor").is_none());
    // A typed result: `structuredContent`, and the same JSON as its text. Unicode both ways.
    let r = door.call("add_note", json!({ "text": "é ✓ 日本" }));
    assert_eq!(r["structuredContent"], json!({ "result": [{ "id": 1, "text": "é ✓ 日本" }] }));
    assert_eq!(r["content"], json!([{ "type": "text", "text": r["structuredContent"].to_string() }]));
    assert!(r.get("isError").is_none(), "{}", r);
    let r = door.call("add_note", json!({ "text": "b" }));
    assert_eq!(r["structuredContent"]["result"].as_array().map(Vec::len), Some(2));
    // A record is the object itself; a field it does not have is dropped, as records drop it.
    assert_eq!(door.call("moved", json!({ "p": { "x": 1 } }))["structuredContent"], json!({ "x": 2, "y": 1 }));
    assert_eq!(door.call("moved", json!({ "p": { "x": 1, "z": 2 }, "by": 3 }))["structuredContent"], json!({ "x": 4, "y": 3 }));
    // No parameters: no `arguments`, or an empty object.
    assert_eq!(door.call("clock", json!({}))["structuredContent"], json!({ "result": 7 }));
    let a = door.ask(&request(json!("bare"), "tools/call", json!({ "name": "clock" })));
    assert_eq!(a["result"]["structuredContent"], json!({ "result": 7 }), "{}", a);
    // An untyped result: text alone, a text as it is. Arguments by name, one before another left
    // out for its default, all left out, an empty one given.
    let text = |r: &Value| {
        assert!(r.get("structuredContent").is_none() && r.get("isError").is_none(), "{}", r);
        r["content"][0]["text"].as_str().unwrap().to_string()
    };
    assert_eq!(text(&door.call("hello", json!({ "name": "Ana", "end": "?" }))), "hi Ana?");
    assert_eq!(text(&door.call("hello", json!({ "name": "Ana" }))), "hi Ana!");
    assert_eq!(text(&door.call("hello", json!({ "end": "", "greeting": "oi", "name": "Zoë" }))), "oi Zoë");
    // Any other value its JSON, as sent: Quillo's own wire (`$dec`) is not read into it.
    assert_eq!(text(&door.call("echo", json!({ "v": { "$dec": "1.5" } }))), r#"{"$dec":"1.5"}"#);
    assert_eq!(text(&door.call("echo", json!({ "v": [1, [2, { "a": "é" }], null] }))), r#"[1,[2,{"a":"é"}],null]"#);
    assert_eq!(text(&door.call("echo", json!({ "v": null }))), "null");
    assert_eq!(text(&door.call("echo", json!({ "v": "" }))), "");
    assert_eq!(text(&door.call("given", json!({}))), "[]");
    door.close();
    assert_eq!(door.ended(), Some(0));
    assert!(door.stderr().contains("adding é ✓ 日本"), "{}", door.stderr());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn c1_what_a_tool_says_is_a_result() {
    let d = door_dir("results");
    let mut door = Stdio::start(&d, &["main.qwe"]);
    let error = |r: Value| {
        assert_eq!(r["isError"], json!(true), "{}", r);
        assert!(r.get("structuredContent").is_none(), "{}", r);
        assert_eq!(r["content"].as_array().map(Vec::len), Some(1), "{}", r);
        assert_eq!(r["content"][0]["type"], json!("text"));
        r["content"][0]["text"].as_str().unwrap().to_string()
    };
    // The tool's own `err(v)`: a text as it is, any other value as its JSON.
    assert_eq!(error(door.call("add_note", json!({ "text": " " }))), "empty note");
    assert_eq!(error(door.call("lookup", json!({ "id": 5 }))), r#"{"code":3,"why":"no note 5"}"#);
    // An argument the door refuses, with the words `/_quillo/rpc` says it in.
    assert_eq!(error(door.call("add_note", json!({ "text": 5 }))), "argument `text` of `add_note` must be str, got int");
    assert_eq!(error(door.call("add_note", json!({}))), "missing argument `text` for `add_note`");
    assert_eq!(error(door.call("hello", json!({ "greeting": "yo" }))), "missing argument `name` for `hello`");
    assert_eq!(error(door.call("hello", json!({ "name": null }))), "argument `name` of `hello` must be str, got null");
    assert_eq!(error(door.call("add_note", json!({ "txt": "x" }))), "`add_note` has no argument `txt`: it takes `text`");
    assert_eq!(error(door.call("hello", json!({ "name": "a", "Name": "b" }))), "`hello` has no argument `Name`: it takes `name`, `greeting`, `end`");
    assert_eq!(error(door.call("clock", json!({ "x": 1 }))), "`clock` takes no arguments, got `x`");
    assert!(error(door.call("moved", json!({ "p": { "y": 1 } }))).starts_with("argument `p` of `moved` must be Point, got map"));
    // A `reply`: its text; with no body, its status's words.
    assert_eq!(error(door.call("mine", json!({}))), "Log in first");
    assert_eq!(error(door.call("gone", json!({}))), "404 Not Found");
    assert_eq!(error(door.call("taken", json!({}))), r#"{"why":"taken"}"#);
    // A page, a redirect or markup, is none of a tool's answers.
    assert_eq!(error(door.call("page", json!({ "which": 0 }))), "a tool answers with data, not a page");
    assert_eq!(error(door.call("page", json!({ "which": 1 }))), "a tool answers with data, not a page");
    // A builtin's error and a failure: `internal error`, the real one on stderr.
    assert_eq!(error(door.call("boom", json!({ "n": 1 }))), "internal error");
    door.says("not an int: \"x1\"");
    assert_eq!(error(door.call("crash", json!({}))), "internal error");
    door.says("error in tool `crash`");
    door.says("unwrap of an error: on purpose");
    // A script's builtins, refused as in any program that serves: stdin is the client's.
    for (tool, builtin) in [("leave", "exit"), ("piped", "stdin"), ("line", "read_line")] {
        assert_eq!(error(door.call(tool, json!({}))), "internal error");
        door.says(&format!("`{}` is for a script", builtin));
    }
    // And the door goes on.
    assert_eq!(door.call("clock", json!({}))["structuredContent"], json!({ "result": 7 }));
    door.close();
    assert_eq!(door.ended(), Some(0));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn c1_protocol_errors_have_their_codes() {
    let d = door_dir("protocol");
    let mut door = Stdio::start(&d, &["main.qwe"]);
    let (parse, invalid_request, no_method, invalid_params, version) = (-32700, -32600, -32601, -32602, -32022);
    // `tools/call` of no tool, or not of its shape: invalid params, the id said.
    for (params, said) in [
        (json!({ "name": "nope" }), "Unknown tool: nope"),
        (json!({ "name": "café ✓" }), "Unknown tool: café ✓"),
        (json!({ "name": "" }), "Unknown tool: "),
        (json!({}), "Invalid params: `name` is the tool's name, a string"),
        (json!({ "name": 5 }), "Invalid params: `name` is the tool's name, a string"),
        (json!({ "name": "clock", "arguments": [1] }), "Invalid params: `arguments` is an object, each argument by its name"),
        (json!({ "name": "clock", "arguments": "x" }), "Invalid params: `arguments` is an object, each argument by its name"),
        (json!({ "name": "clock", "arguments": null }), "Invalid params: `arguments` is an object, each argument by its name"),
    ] {
        assert_eq!(door.refusal(&request(json!(7), "tools/call", params.clone())), (Some(json!(7)), invalid_params, said.to_string()), "{}", params);
    }
    // `tools/list` has one page: a cursor is one the door never gave.
    for cursor in [json!("next"), json!("")] {
        assert_eq!(door.refusal(&request(json!(8), "tools/list", json!({ "cursor": cursor }))), (Some(json!(8)), invalid_params, "Invalid cursor".into()));
    }
    // `_meta`'s version and capabilities, on every request.
    let meta_said = "Invalid params: `_meta` carries io.modelcontextprotocol/protocolVersion and io.modelcontextprotocol/clientCapabilities";
    for line in [
        r#"{"jsonrpc":"2.0","id":9,"method":"tools/list"}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":9,"method":"tools/list","params":{}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":9,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":9,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":[]}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":9,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":5,"io.modelcontextprotocol/clientCapabilities":{}}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":9,"method":"server/discover","params":{"_meta":{"io.modelcontextprotocol/clientCapabilities":{}}}}"#.to_string(),
    ] {
        assert_eq!(door.refusal(&line), (Some(json!(9)), invalid_params, meta_said.to_string()), "{}", line);
    }
    assert_eq!(door.refusal(r#"{"jsonrpc":"2.0","id":9,"method":"tools/list","params":[]}"#), (Some(json!(9)), invalid_params, "Invalid params: `params` is an object".into()));
    // A version it does not speak: the version error, with what it speaks; before what else is
    // missing.
    for (v, caps) in [("2025-06-18", true), ("2025-11-25", false), ("2026-07-29", true), ("", true)] {
        let mut m = json!({ "io.modelcontextprotocol/protocolVersion": v });
        if caps {
            m["io.modelcontextprotocol/clientCapabilities"] = json!({});
        }
        let a = door.ask(&json!({ "jsonrpc": "2.0", "id": 10, "method": "tools/list", "params": { "_meta": m } }).to_string());
        assert_eq!(a["error"]["code"], json!(version), "{}", a);
        assert_eq!(a["error"]["data"], json!({ "supported": ["2026-07-28"], "requested": v }), "{}", a);
        assert_eq!(a["id"], json!(10));
    }
    // A method it does not have, those of capabilities it does not say among them.
    for m in ["ping", "resources/list", "prompts/list", "subscriptions/listen", "completion/complete", "Tools/List", "notifications/initialized"] {
        assert_eq!(door.refusal(&request(json!(11), m, json!({}))), (Some(json!(11)), no_method, format!("Method not found: {}", m)));
    }
    // Not a request: no id where it cannot be read, MCP's ids never being null.
    for (line, code, said) in [
        ("not json", parse, "Parse error: expected ident at line 1 column 2"),
        ("{", parse, "Parse error: EOF while parsing an object at line 1 column 1"),
        (r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"} {}"#, parse, "Parse error: trailing characters at line 1 column 48"),
        ("[]", invalid_request, "Invalid request: one message a line, not a batch"),
        (r#"[{"jsonrpc":"2.0","id":1,"method":"tools/list"}]"#, invalid_request, "Invalid request: one message a line, not a batch"),
        ("5", invalid_request, "Invalid request: a message is a JSON object"),
        ("\"tools/list\"", invalid_request, "Invalid request: a message is a JSON object"),
        ("null", invalid_request, "Invalid request: a message is a JSON object"),
        (r#"{"jsonrpc":"2.0","id":null,"method":"tools/list"}"#, invalid_request, "Invalid request: an id is a string or a number"),
        (r#"{"jsonrpc":"2.0","id":{},"method":"tools/list"}"#, invalid_request, "Invalid request: an id is a string or a number"),
        (r#"{"jsonrpc":"2.0","id":true,"method":"tools/list"}"#, invalid_request, "Invalid request: an id is a string or a number"),
        (r#"{"jsonrpc":"2.0","method":5}"#, invalid_request, "Invalid request: `method` is a string"),
        (r#"{"jsonrpc":"2.0"}"#, invalid_request, "Invalid request: `method` is a string"),
    ] {
        assert_eq!(door.refusal(line), (None, code, said.to_string()), "{}", line);
    }
    // Bytes that are not UTF-8: not JSON.
    door.write(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/\xff\"}\n");
    let a = door.next().unwrap();
    assert!(a.get("id").is_none() && a["error"]["code"] == json!(parse), "{}", a);
    assert!(a["error"]["message"].as_str().is_some_and(|m| m.starts_with("Parse error: ")), "{}", a);
    // With an id that can be read, it is said.
    assert_eq!(door.refusal(&request(json!(12), "tools/list", json!({})).replace("\"2.0\"", "\"1.0\"")), (Some(json!(12)), invalid_request, "Invalid request: `jsonrpc` is \"2.0\"".into()));
    assert_eq!(door.refusal(r#"{"id":12,"method":"tools/list"}"#), (Some(json!(12)), invalid_request, "Invalid request: `jsonrpc` is \"2.0\"".into()));
    assert_eq!(door.refusal(r#"{"jsonrpc":"2.0","id":12,"method":["tools/list"]}"#), (Some(json!(12)), invalid_request, "Invalid request: `method` is a string".into()));
    assert_eq!(door.refusal(r#"{"jsonrpc":"2.0","id":12}"#), (Some(json!(12)), invalid_request, "Invalid request: `method` is a string".into()));
    // Ids come back as they were sent.
    // Any number, as `RequestId` is `string | number` (found in review: 1.5 was refused).
    for id in [json!("s é"), json!(""), json!(0), json!(-1), json!(i64::MIN), json!(u64::MAX), json!(1.5), json!(-0.25), json!(1e300)] {
        assert_eq!(door.ask(&request(id.clone(), "server/discover", json!({})))["id"], id);
    }
    // Never answered: a notification, known or not; a client's response; a line of nothing.
    // Each is followed by a request, whose answer is the next line.
    for (n, quiet) in [
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":"x","reason":"late"}}"#,
        r#"{"jsonrpc":"2.0","method":"tools/list","params":{}}"#,
        r#"{"jsonrpc":"2.0","method":"anything"}"#,
        r#"{"jsonrpc":"2.0","id":5,"result":{}}"#,
        r#"{"jsonrpc":"2.0","id":5,"error":{"code":-1,"message":"no"}}"#,
        r#"{"jsonrpc":"2.0","result":{}}"#,
        "",
        "   \t",
    ]
    .iter()
    .enumerate()
    {
        door.send(quiet);
        assert_eq!(door.ask(&request(json!(100 + n), "server/discover", json!({})))["id"], json!(100 + n), "{:?} was answered", quiet);
    }
    // A line that ends `\r\n`.
    door.write(format!("{}\r\n", request(json!(13), "server/discover", json!({}))).as_bytes());
    assert_eq!(door.next().unwrap()["id"], json!(13));
    door.close();
    assert_eq!(door.ended(), Some(0));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn c1_initialize_is_told_the_version_to_speak() {
    let d = door_dir("initialize");
    let mut door = Stdio::start(&d, &["main.qwe"]);
    // As a client of 2025-11-25 opens (Claude Code 2.1.270's own message): the version error,
    // which names the version the door speaks, and no fallback (decided 2026-11-08).
    let a = door.ask(r#"{"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{"roots":{"listChanged":true},"elicitation":{}},"clientInfo":{"name":"claude-code","version":"2.1.270"}},"jsonrpc":"2.0","id":0}"#);
    assert_eq!(a, json!({ "jsonrpc": "2.0", "id": 0, "error": { "code": -32022, "message": "Unsupported protocol version: this server speaks MCP 2026-07-28 only", "data": { "supported": ["2026-07-28"], "requested": "2025-11-25" } } }));
    // Whatever it says of its version, or nothing.
    for (params, requested) in [(json!({ "protocolVersion": "2024-11-05" }), "2024-11-05"), (json!({}), ""), (json!({ "protocolVersion": 3 }), "")] {
        let a = door.ask(&json!({ "jsonrpc": "2.0", "id": "i", "method": "initialize", "params": params }).to_string());
        assert_eq!((a["error"]["code"].clone(), a["error"]["data"]["requested"].clone()), (json!(-32022), json!(requested)), "{}", a);
    }
    assert_eq!(door.ask(r#"{"jsonrpc":"2.0","id":"i","method":"initialize"}"#)["error"]["code"], json!(-32022));
    // Sent with 2026-07-28's `_meta`, `initialize` is a method it does not have, not a version
    // to try again with.
    assert_eq!(door.refusal(&request(json!(1), "initialize", json!({}))), (Some(json!(1)), -32601, "Method not found: initialize".into()));
    // Nothing was kept: `notifications/initialized` is ignored, and a request of 2026-07-28 is
    // answered as the first would be.
    door.send(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
    assert_eq!(door.call("clock", json!({}))["structuredContent"], json!({ "result": 7 }));
    door.close();
    assert_eq!(door.ended(), Some(0));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn c1_stdout_carries_only_messages_while_the_program_prints() {
    let d = door_dir("stdout");
    let mut door = Stdio::start(&d, &["main.qwe"]);
    let sent = ["a", "b\nc", "{\"jsonrpc\":\"2.0\",\"id\":98}"];
    for (i, t) in sent.iter().enumerate() {
        door.send(&request(json!(i), "tools/call", json!({ "name": "add_note", "arguments": { "text": t } })));
    }
    door.send(&call_line("wait", json!({ "ms": 1 })));
    door.close();
    assert_eq!(door.ended(), Some(0));
    // Each line of stdout is the answer to a request sent, in order, and nothing else is there.
    let mut lines = Vec::new();
    while let Some(v) = door.next_within(Duration::from_millis(200)) {
        lines.push(v);
    }
    let ids: Vec<Value> = lines.iter().map(|l| l["id"].clone()).collect();
    assert_eq!(ids, [json!(0), json!(1), json!(2), json!("wait")], "{:?}", lines);
    assert!(lines.iter().all(|l| l["jsonrpc"] == json!("2.0") && l["result"]["resultType"] == json!("complete")), "{:?}", lines);
    // What the program printed, at the top level and in its tools, went to stderr.
    let err = door.stderr();
    for p in ["top level, to stderr\n", "{\"jsonrpc\":\"2.0\",\"id\":99,\"result\":{}}\n", "two\nlines\n", "adding a\n", "adding b\nc\n", "adding {\"jsonrpc\":\"2.0\",\"id\":98}\n", "waiting\n"] {
        assert!(err.contains(p), "{:?} not on stderr: {}", p, err);
    }
    // A script's `print` is on stdout, as before.
    std::fs::write(d.join("script.qwe"), "print(\"to stdout\")\n").unwrap();
    assert_eq!(quillo(&d, &["run", "script.qwe"]), (true, "to stdout\n".to_string(), String::new()));
    let _ = std::fs::remove_dir_all(&d);
}

#[cfg(unix)]
#[test]
fn c1_the_end_of_stdin_and_sigterm_end_it() {
    let d = door_dir("end");
    // Stdin closed at once, and after a last line with no `\n`, which is answered.
    assert_eq!(mcp_exits(&d, &["main.qwe"]).0, Some(0));
    let mut door = Stdio::start(&d, &["main.qwe"]);
    door.write(request(json!("last"), "server/discover", json!({})).as_bytes());
    door.close();
    assert_eq!(door.next().map(|a| a["id"].clone()), Some(json!("last")));
    assert_eq!(door.ended(), Some(0));
    let term = |door: &Stdio| unsafe { libc::kill(door.pid() as i32, libc::SIGTERM) };
    // SIGTERM while it waits for a message: it ends.
    let mut door = Stdio::start(&d, &["main.qwe"]);
    assert_eq!(door.call("clock", json!({}))["structuredContent"], json!({ "result": 7 }));
    term(&door);
    assert_eq!(door.ended(), Some(0));
    // While a call is answered: once its answer is out.
    let mut door = Stdio::start(&d, &["main.qwe"]);
    door.send(&call_line("wait", json!({ "ms": 1500 })));
    door.says("waiting");
    let at = Instant::now();
    term(&door);
    let a = door.next().expect("the call in progress was not answered");
    assert_eq!(a["result"]["structuredContent"], json!({ "result": 1500 }), "{}", a);
    assert_eq!(door.ended(), Some(0));
    assert!(at.elapsed() < Duration::from_secs(10), "{:?}", at.elapsed());
    assert!(door.stderr().contains("quillo: stopping once the call in progress is answered (again to stop now)"), "{}", door.stderr());
    // A second SIGTERM: at once, the call left unanswered.
    let mut door = Stdio::start(&d, &["main.qwe"]);
    door.send(&call_line("wait", json!({ "ms": 20000 })));
    door.says("waiting");
    term(&door);
    door.says("stopping once");
    let at = Instant::now();
    term(&door);
    assert_eq!(door.ended(), Some(130));
    assert!(at.elapsed() < Duration::from_secs(10), "{:?}", at.elapsed());
    assert!(door.next_within(Duration::from_millis(200)).is_none());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn c1_a_program_that_does_not_check_does_not_start() {
    // An error `quillo check` finds: the door does not start, and stdout is empty.
    let d = dir("c1-unchecked", "/// T.\nfn mcp café() = 1\n");
    let (code, out, err) = mcp_exits(&d, &["main.qwe"]);
    assert_eq!((code, out.len()), (Some(1), 0), "{}", err);
    assert!(err.contains(NAME_RULE), "{}", err);
    // A syntax error, said as `quillo run` says it.
    std::fs::write(d.join("main.qwe"), "/// T.\nfn mcp tick( = 1\n").unwrap();
    let (code, out, err) = mcp_exits(&d, &["main.qwe"]);
    assert_eq!((code, out, err), (Some(1), vec![], "main.qwe:2: error: expected a parameter name, found `=`\n".to_string()));
    // A top level that fails, and one that reads stdin, which is the client's.
    std::fs::write(d.join("main.qwe"), "let x = unwrap(err(\"at the top\"))\n/// T.\nfn mcp tick() = 1\n").unwrap();
    let (code, out, err) = mcp_exits(&d, &["main.qwe"]);
    assert_eq!((code, out.len()), (Some(1), 0), "{}", err);
    assert!(err.contains("at the top"), "{}", err);
    std::fs::write(d.join("main.qwe"), "let first = read_line()\n/// T.\nfn mcp tick() = 1\n").unwrap();
    let mut door = Stdio::start(&d, &["main.qwe"]);
    door.send(&request(json!(1), "server/discover", json!({})));
    assert_eq!(door.ended(), Some(1));
    assert!(door.stderr().contains("`read_line` is for a script"), "{}", door.stderr());
    assert!(door.next_within(Duration::from_millis(200)).is_none());
    // No program there.
    let (code, out, err) = mcp_exits(&d, &["gone.qwe"]);
    assert_eq!((code, out.len()), (Some(1), 0), "{}", err);
    // A program with no tools starts, and says so; its list is empty.
    std::fs::write(d.join("main.qwe"), "fn plain() = 1\n").unwrap();
    let mut door = Stdio::start(&d, &["main.qwe"]);
    assert_eq!(door.ask(&request(json!(1), "tools/list", json!({})))["result"]["tools"], json!([]));
    assert_eq!(door.ask(&request(json!(2), "server/discover", json!({})))["result"]["capabilities"], json!({ "tools": {} }));
    door.close();
    assert_eq!(door.ended(), Some(0));
    assert!(door.stderr().contains("quillo: main.qwe has no tools: a function is one when it is declared `fn mcp name(...)`"), "{}", door.stderr());
    // `run`'s options about serving are not `mcp`'s.
    for (args, said) in [
        (&["main.qwe", "--port", "1"][..], "quillo: `--port` is not for `quillo mcp`, which serves no routes: it takes --env-file, --policy, --allow-path and --no-build\n"),
        (&["--watch", "main.qwe"], "quillo: `--watch` is not for `quillo mcp`, which serves no routes: it takes --env-file, --policy, --allow-path and --no-build\n"),
        (&["main.qwe", "--bogus"], "quillo: unknown option `--bogus` for `mcp`\n"),
        (&["main.qwe", "--env-fil", "x"], "quillo: unknown option `--env-fil` for `mcp` (did you mean `--env-file`?)\n"),
        (&["main.qwe", "other.qwe"], "quillo: unknown argument `other.qwe`: `mcp` takes one program; its own arguments go after `--`\n"),
    ] {
        assert_eq!(mcp_exits(&d, args), (Some(2), vec![], said.to_string()), "{:?}", args);
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn c1_the_settings_and_the_programs_arguments() {
    let d = dir("c1-settings", "/// A setting.\nfn mcp setting(k: str) with env = env(k)\n/// Its arguments.\nfn mcp given() with env = args()\n");
    std::fs::write(d.join(".env"), "COLOUR=teal\n").unwrap();
    std::fs::write(d.join("other.env"), "COLOUR=ochre\n").unwrap();
    let text = |door: &mut Stdio, name: &str, args: Value| door.call(name, args)["content"][0]["text"].as_str().unwrap().to_string();
    // The `.env` beside the program, as `quillo run` reads it, or `--env-file`'s.
    let mut door = Stdio::start(&d, &["main.qwe"]);
    assert_eq!(text(&mut door, "setting", json!({ "k": "COLOUR" })), "teal");
    assert_eq!(text(&mut door, "given", json!({})), "[]");
    drop(door);
    // A program's own arguments after `--`, `--help` among them.
    let mut door = Stdio::start(&d, &["main.qwe", "--env-file", "other.env", "--", "a", "b c", "--help"]);
    assert_eq!(text(&mut door, "setting", json!({ "k": "COLOUR" })), "ochre");
    assert_eq!(text(&mut door, "given", json!({})), r#"["a","b c","--help"]"#);
    drop(door);
    // Its folder, as `quillo run .` takes it.
    let mut door = Stdio::start(&d, &[]);
    assert_eq!(text(&mut door, "setting", json!({ "k": "COLOUR" })), "teal");
    drop(door);
    // `help` lists it; a misspelling is told it; `-h` is the usage, on stdout.
    let (ok, out, _) = quillo(&d, &["help"]);
    assert!(ok && out.contains("\n  quillo mcp <file.qwe> [options]   serve the program's tools (`fn mcp`) to an AI\n"), "{}", out);
    assert!(out.contains("\n`mcp` takes --env-file, --policy, --allow-path, --no-build and `-- a b`."), "{}", out);
    let (ok, _, err) = quillo(&d, &["mcps", "main.qwe"]);
    assert!(!ok && err.contains("unknown command `mcps` (did you mean `mcp`?)"), "{}", err);
    let (ok, out, err) = quillo(&d, &["mcp", "-h"]);
    assert!(ok && out.starts_with("Quillo — a small web language\n") && err.is_empty(), "{}{}", out, err);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn c1_a_tool_that_only_reads_runs_in_the_read_only_transaction() {
    let d = door_dir("read-only");
    let mut door = Stdio::start(&d, &["main.qwe"]);
    let listed = door.ask(&request(json!(1), "tools/list", json!({})));
    let tools = listed["result"]["tools"].as_array().unwrap().clone();
    let hints = |name: &str| tools.iter().find(|t| t["name"] == json!(name)).unwrap()["annotations"].clone();
    assert_eq!(hints("locked"), json!({ "readOnlyHint": true, "openWorldHint": false }));
    assert_eq!(hints("unlocked"), json!({ "openWorldHint": false }));
    // Through the door, SQLite refuses writes while the tool that only reads runs, and not while
    // the one that writes does.
    assert_eq!(door.call("locked", json!({}))["structuredContent"], json!({ "result": 1 }));
    assert_eq!(door.call("unlocked", json!({ "t": "a" }))["structuredContent"], json!({ "result": 0 }));
    assert_eq!(door.call("locked", json!({}))["structuredContent"], json!({ "result": 1 }));
    assert_eq!(door.call("add_note", json!({ "text": "b" }))["structuredContent"]["result"].as_array().map(Vec::len), Some(2));
    door.close();
    assert_eq!(door.ended(), Some(0));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn c1_on_postgres_a_write_the_checker_could_not_see_is_refused() {
    let Some(pg) = support::postgres("feedback56-c1") else {
        eprintln!("skipped: c1_on_postgres_a_write_the_checker_could_not_see_is_refused, as PostgreSQL's initdb and pg_ctl were not found");
        return;
    };
    let url = pg.database("cdoor");
    // A view whose `select` calls a function of the program's that writes: to the checker, a
    // `select` of a table, so the tool is said to only read; the database refuses the write.
    let src = format!(
        "db.open({:?})\n\
         db.exec(\"create table if not exists notes (id serial primary key, text text)\")\n\
         db.exec(\"create or replace function add_note() returns int language sql as $$ insert into notes (text) values ('sneaked') returning id $$\")\n\
         db.exec(\"create or replace view sneaky as select add_note() as id\")\n\
         /// Peeks.\nfn mcp peek() -> list with db = db.query(\"select id from sneaky\")\n\
         /// Counts.\nfn mcp total() -> int with db = db.one(\"select count(*) as n from notes\").n\n\
         /// Adds.\nfn mcp add(t: str) -> int with db = db.exec(\"insert into notes (text) values (?)\", t).changes\n",
        url
    );
    let d = dir("c1-pg", &src);
    let mut door = Stdio::start(&d, &["main.qwe"]);
    let listed = door.ask(&request(json!(1), "tools/list", json!({})));
    assert_eq!(listed["result"]["tools"][0]["annotations"], json!({ "readOnlyHint": true, "openWorldHint": false }), "{}", listed);
    let r = door.call("peek", json!({}));
    assert_eq!((r["isError"].clone(), r["content"][0]["text"].clone()), (json!(true), json!("internal error")), "{}", r);
    door.says("read-only transaction");
    assert_eq!(door.call("total", json!({}))["structuredContent"], json!({ "result": 0 }));
    // A tool that writes is not held to it.
    assert_eq!(door.call("add", json!({ "t": "é" }))["structuredContent"], json!({ "result": 1 }));
    assert_eq!(door.call("total", json!({}))["structuredContent"], json!({ "result": 1 }));
    door.close();
    assert_eq!(door.ended(), Some(0));
    drop(pg);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn c1_a_message_at_the_limit_and_past_it() {
    let d = door_dir("limit");
    let mut door = Stdio::start(&d, &["main.qwe"]);
    let most = 10 << 20;
    // A message of exactly the limit, one under and one over, its JSON padded with spaces.
    let padded = |id: i64, len: usize| {
        let m = request(json!(id), "tools/list", json!({}));
        format!("{}{}", m, " ".repeat(len - m.len()))
    };
    assert_eq!(door.ask(&padded(1, most))["id"], json!(1));
    assert_eq!(door.ask(&padded(2, most - 1))["id"], json!(2));
    assert_eq!(door.refusal(&padded(3, most + 1)), (None, -32600, "Invalid request: a message is at most 10 MiB".into()));
    // And the next line is read whole.
    assert_eq!(door.ask(&request(json!(4), "tools/list", json!({})))["id"], json!(4));
    // As deep as every reader takes JSON, 128 levels, and one past.
    let nested = |levels: usize| format!("{}{}", "[".repeat(levels), "]".repeat(levels));
    let deep = |levels: usize| {
        let m = call_line("echo", json!({ "v": "HOLE" }));
        m.replace("\"HOLE\"", &nested(levels))
    };
    let r = door.ask(&deep(125));
    assert_eq!(r["result"]["content"][0]["text"], json!(nested(125)), "{}", r);
    assert_eq!(door.refusal(&deep(126)), (None, -32700, "Parse error: it nests more than 128 deep".into()));
    door.close();
    assert_eq!(door.ended(), Some(0));
    let _ = std::fs::remove_dir_all(&d);
}

/// A command in `d` under a deadline, its stdout and stderr read while it runs, so that one that
/// says much cannot fill a pipe and wait for ever: Err when it did not end in time (killed then).
fn run_for(d: &Path, cmd: &str, secs: u64) -> Result<std::process::Output, String> {
    use std::io::Read as _;
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(d)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let read = |mut r: Box<dyn std::io::Read + Send>| {
        std::thread::spawn(move || {
            let mut b = Vec::new();
            let _ = r.read_to_end(&mut b);
            b
        })
    };
    let out = read(Box::new(child.stdout.take().unwrap()));
    let err = read(Box::new(child.stderr.take().unwrap()));
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return Ok(std::process::Output { status, stdout: out.join().unwrap_or_default(), stderr: err.join().unwrap_or_default() });
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("`{}` did not end in {} s", cmd, secs));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// MCP's own TypeScript client (`@modelcontextprotocol/client` 2.4.0, the SDK's v2, which speaks
/// 2026-07-28), installed with npm into the test's folder, as a client would drive the door:
/// pinned to 2026-07-28, and as a client of 2025-11-25 that opens with `initialize`. Skipped only
/// where npm is not found or the package cannot be installed; a client that does not end in time,
/// or a call of it that fails, fails the test (found in review: any of them was a skip).
#[test]
fn c1_the_official_typescript_client_drives_it() {
    let d = door_dir("sdk");
    let script = r#"const { Client } = require("@modelcontextprotocol/client");
const { StdioClientTransport } = require("@modelcontextprotocol/client/stdio");
(async () => {
  const [quillo, dir] = process.argv.slice(2);
  const out = {};
  for (const mode of ["pin", "legacy"]) {
    const client = new Client({ name: "quillo-test", version: "1.0.0" }, mode === "pin" ? { versionNegotiation: { mode: { pin: "2026-07-28" } } } : {});
    const transport = new StdioClientTransport({ command: quillo, args: ["mcp", "main.qwe"], cwd: dir, stderr: "pipe" });
    const r = {};
    try {
      await client.connect(transport);
      r.protocol = client.getNegotiatedProtocolVersion();
      r.tools = (await client.listTools()).tools.map((t) => t.name);
      r.add = await client.callTool({ name: "add_note", arguments: { text: "é ✓" } });
      r.moved = await client.callTool({ name: "moved", arguments: { p: { x: 1 } } });
      r.hello = await client.callTool({ name: "hello", arguments: { name: "Ana" } });
      r.empty = await client.callTool({ name: "add_note", arguments: { text: " " } });
      try { await client.callTool({ name: "nope", arguments: {} }); } catch (e) { r.unknown = e.code; }
    } catch (e) {
      r.failed = { code: e.code, message: String(e.message) };
    }
    try { await client.close(); } catch (e) {}
    out[mode] = r;
  }
  console.log(JSON.stringify(out));
})();
"#;
    std::fs::write(d.join("client.cjs"), script).unwrap();
    std::fs::write(d.join("package.json"), r#"{"name":"quillo-c1-sdk","private":true}"#).unwrap();
    if Command::new("npm").arg("--version").stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().is_err() {
        let _ = std::fs::remove_dir_all(&d);
        eprintln!("skipped: c1_the_official_typescript_client_drives_it, as npm was not found");
        return;
    }
    match run_for(&d, "npm install --no-audit --no-fund --ignore-scripts @modelcontextprotocol/client@2.4.0", 240) {
        Ok(o) if o.status.success() => {}
        failed => {
            let _ = std::fs::remove_dir_all(&d);
            let why = failed.map_or_else(|e| e, |o| String::from_utf8_lossy(&o.stderr).into_owned());
            eprintln!("skipped: c1_the_official_typescript_client_drives_it, as npm could not install @modelcontextprotocol/client@2.4.0: {}", why);
            return;
        }
    }
    let ran = run_for(&d, &format!("node client.cjs {:?} {:?}", env!("CARGO_BIN_EXE_quillo"), d.display().to_string()), 120);
    let _ = std::fs::remove_dir_all(&d);
    let o = ran.unwrap_or_else(|e| panic!("the client: {}", e));
    let (stdout, stderr) = (String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned());
    assert!(o.status.success(), "the client failed: {}\n{}", stdout, stderr);
    let said: Value = stdout.lines().last().and_then(|l| serde_json::from_str(l).ok()).unwrap_or_else(|| panic!("the client said no JSON: {}\n{}", stdout, stderr));
    let pin = &said["pin"];
    assert!(pin.get("failed").is_none(), "{}", said);
    assert_eq!(pin["protocol"], json!("2026-07-28"));
    assert_eq!(pin["tools"].as_array().map(Vec::len), Some(19), "{}", pin);
    // The client checks each `structuredContent` against the tool's `outputSchema`.
    assert_eq!(pin["add"]["structuredContent"], json!({ "result": [{ "id": 1, "text": "é ✓" }] }), "{}", pin);
    assert_eq!(pin["moved"]["structuredContent"], json!({ "x": 2, "y": 1 }), "{}", pin);
    assert_eq!(pin["hello"]["content"], json!([{ "type": "text", "text": "hi Ana!" }]), "{}", pin);
    assert_eq!((pin["empty"]["isError"].clone(), pin["empty"]["content"][0]["text"].clone()), (json!(true), json!("empty note")), "{}", pin);
    assert_eq!(pin["unknown"], json!(-32602));
    // A client of 2025-11-25, the SDK's default: refused at `initialize` with the version error.
    assert_eq!(said["legacy"]["failed"]["code"], json!(-32022), "{}", said);
}

// ---- Slice 3, review: a panic in any transaction, db.atomic's too ----------------------------
//
// A panic is a bug of Quillo's, which the server answers 500 and lives on: what a transaction
// raised is lowered, what it held let go, and nothing of a database is left for the requests
// after it (found in review). `quillo::db::panic_at_db_call(n)` makes the `n`th call holding a
// server database's connection panic, as no query a program can make does.

/// How many of the program's PostgreSQL connections can be had now (0 on another database): as
/// many after a panic as before it, when it lost none.
fn free(it: &Interp) -> usize {
    let slot = it.db.lock().unwrap().clone();
    let Some(quillo::db::DbSlot::One(b)) = slot else { return 0 };
    let quillo::db::Backend::Postgres(p) = &*b else { return 0 };
    let mut got = Vec::new();
    while let Some(c) = p.try_checkout().unwrap() {
        got.push(c);
    }
    let n = got.len();
    for c in got {
        p.give_back(c);
    }
    n
}

/// Whether the program's database is outside any transaction, as SQLite says.
fn autocommit(it: &Interp) -> bool {
    let slot = it.db.lock().unwrap().clone();
    let Some(quillo::db::DbSlot::One(b)) = slot else { return true };
    let quillo::db::Backend::Sqlite(m) = &*b else { return true };
    let open = !m.lock().unwrap().is_autocommit();
    !open
}

/// Nothing a panic left raised or held: no transaction pinned, every connection to be had, the
/// thread let go, SQLite outside a transaction, the interpreter's counts down.
fn nothing_left(it: &Interp, free_before: usize, what: &str) {
    assert!(it.pg_tx.lock().unwrap().is_empty(), "{}: a transaction's connection is left pinned", what);
    assert_eq!(free(it), free_before, "{}: a connection's place in the pool is lost", what);
    assert_eq!(it.sqlite_tx, 0, "{}", what);
    assert!(!quillo::httpd::held(), "{}: the thread is left held", what);
    assert!(autocommit(it), "{}: SQLite is left in a transaction", what);
    assert_eq!(it.tx_depth, 0, "{}", what);
    assert_eq!(it.catching_drops, 0, "{}", what);
}

/// `f`, which panics: the panic goes on up to here.
fn panics(it: &mut Interp, what: &str, f: impl FnOnce(&mut Interp)) {
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(it)));
    quillo::db::panic_at_db_call(0);
    assert!(caught.is_err(), "{}: it did not panic", what);
}

/// A function given to a transaction that panics, as a bug of Quillo's in it would.
fn boom(_: &mut Interp, _: Vec<Q>, _: u32) -> Result<Q, quillo::interp::Ctrl> {
    panic!("a panic in the function, on purpose")
}

/// One that fails, as a function does.
fn fails(_: &mut Interp, _: Vec<Q>, _: u32) -> Result<Q, quillo::interp::Ctrl> {
    Ok(Q::err(Q::str("no")))
}

/// One whose statement panics in its call, the connection held.
fn boom_in_a_call(it: &mut Interp, _: Vec<Q>, _: u32) -> Result<Q, quillo::interp::Ctrl> {
    quillo::db::panic_at_db_call(1);
    let write = global(it, "write");
    it.call(&write, vec![Q::str("lost")], 0)
}

const PANIC_HELPERS: &str = "fn write(t: str) {\n    let r = db.exec(\"insert into notes (text) values (?)\", t)\n    db.error_kind(r) ?? \"wrote\"\n}\n\
fn count() -> int = db.one(\"select count(*) as n from notes\").n\n\
fn run_atomic(dbs, f) = db.atomic(dbs, f)\n";

/// A transaction of `db.transaction`'s around `body`, and one inside it.
fn tx(it: &mut Interp, body: &mut dyn FnMut(&mut Interp) -> Result<Q, quillo::interp::Ctrl>) -> Result<Q, quillo::interp::Ctrl> {
    quillo::txn::run(it, None, &quillo::txn::Options::default(), 0, body)
}

fn nested(it: &mut Interp, body: &mut dyn FnMut(&mut Interp) -> Result<Q, quillo::interp::Ctrl>) -> Result<Q, quillo::interp::Ctrl> {
    tx(it, &mut |it| tx(it, &mut *body))
}

/// A PostgreSQL program with a function of its own, its pool of two.
fn pg_program(d: &Path, url: &str) -> Interp {
    let src = format!(
        "db.open(\"{}?pool_size=2\")\ndb.exec(\"create table notes (id serial primary key, text text)\")\n\
         db.exec(\"create function add_note() returns int language sql as $$ insert into notes (text) values ('x') returning id $$\")\n\
         fn own() = db.query(\"select add_note() as id\")\n{}",
        url, PANIC_HELPERS
    );
    let mut it = Interp::new(d);
    it.load_source(&src, "main.qwe").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    it
}

/// What another program sees in `url`'s notes: what was committed.
fn committed_in(d: &Path, url: &str) -> String {
    let mut other = Interp::new(d);
    other.load_source(&format!("db.open({:?})\ndb.one(\"select count(*) as n from notes\").n\n", url), "other.qwe").unwrap().display()
}

#[test]
fn q858_review_a_panic_in_a_call_on_postgres_loses_no_connection() {
    let Some(pg) = support::postgres("feedback56-call-panic") else {
        eprintln!("skipped: q858_review_a_panic_in_a_call_on_postgres_loses_no_connection, as PostgreSQL's initdb and pg_ctl were not found");
        return;
    };
    let url = pg.database("calls");
    let d = support::temp_dir("feedback56-call-panic");
    let mut it = pg_program(&d, &url);
    let write = global(&it, "write");
    let own = global(&it, "own");
    let all = free(&it);
    assert_eq!(all, 2);
    // Outside a transaction, a statement and one calling a function of the program's (on a
    // connection of its own); inside one, outermost and nested: each panicking call's connection
    // is closed and its place freed, where one was lost each time.
    for (what, f, args) in [("a statement", &write, vec![Q::str("lost")]), ("a function of its own", &own, vec![])] {
        panics(&mut it, what, |it| {
            quillo::db::panic_at_db_call(1);
            let _ = it.call(f, args, 0);
        });
        nothing_left(&it, all, what);
    }
    for (what, deep) in [("in a transaction", false), ("in a nested one", true)] {
        panics(&mut it, what, |it| {
            let body = &mut |it: &mut Interp| {
                quillo::db::panic_at_db_call(1);
                it.call(&write, vec![Q::str("lost")], 0)
            };
            let _ = if deep { nested(it, body) } else { tx(it, body) };
        });
        nothing_left(&it, all, what);
    }
    // More panics than the pool holds, then a transaction: it gets a connection at once.
    for _ in 0..3 {
        panics(&mut it, "again", |it| {
            let _ = tx(it, &mut |it| {
                quillo::db::panic_at_db_call(1);
                it.call(&write, vec![Q::str("lost")], 0)
            });
        });
    }
    let started = Instant::now();
    assert_eq!(ok(tx(&mut it, &mut |it| it.call(&write, vec![Q::str("kept")], 0))).display(), "wrote");
    assert!(started.elapsed() < Duration::from_secs(5), "waited {:?} for a connection", started.elapsed());
    nothing_left(&it, all, "after");
    assert_eq!(committed_in(&d, &url), "1");
    drop(it);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn q858_review_after_a_call_panicked_nothing_is_rolled_back_on_another_connection() {
    let Some(pg) = support::postgres("feedback56-no-rollback") else {
        eprintln!("skipped: q858_review_after_a_call_panicked_nothing_is_rolled_back_on_another_connection, as PostgreSQL's initdb and pg_ctl were not found");
        return;
    };
    let url = pg.database("norollback");
    let d = support::temp_dir("feedback56-no-rollback");
    let mut it = pg_program(&d, &url);
    let write = global(&it, "write");
    let all = free(&it);
    // The call that panicked closed its connection, and its transaction with it: no other call is
    // made, where a connection was checked out (waiting up to 30 s for one) to roll back nothing,
    // in each transaction it was in.
    for (what, deep) in [("outermost", false), ("nested", true)] {
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let body = &mut |it: &mut Interp| {
                quillo::db::panic_at_db_call(1);
                it.call(&write, vec![Q::str("lost")], 0)
            };
            let _ = if deep { nested(&mut it, body) } else { tx(&mut it, body) };
        }));
        assert!(caught.is_err(), "{}", what);
        // The calls since the body armed it: the one that panicked.
        assert_eq!(quillo::db::panic_at_db_call(0), 1, "{}: a call after the one that panicked", what);
        nothing_left(&it, all, what);
    }
    assert_eq!(committed_in(&d, &url), "0");
    drop(it);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn q858_review_a_panic_in_the_rollback_leaves_nothing_raised_on_postgres() {
    let Some(pg) = support::postgres("feedback56-rb-panic") else {
        eprintln!("skipped: q858_review_a_panic_in_the_rollback_leaves_nothing_raised_on_postgres, as PostgreSQL's initdb and pg_ctl were not found");
        return;
    };
    let url = pg.database("rollbackpanic");
    let d = support::temp_dir("feedback56-rollback-panic");
    let mut it = pg_program(&d, &url);
    let write = global(&it, "write");
    let all = free(&it);
    // The body panics, then so does the rollback (of the transaction, or of the nested one's
    // savepoint): the depth and the announcements were lowered after the rollback, so they were
    // left raised, the connection pinned.
    for (what, deep) in [("outermost", false), ("nested", true)] {
        panics(&mut it, what, |it| {
            let body = &mut |it: &mut Interp| -> Result<Q, quillo::interp::Ctrl> {
                assert_eq!(ok(it.call(&write, vec![Q::str("lost")], 0)).display(), "wrote");
                quillo::db::panic_at_db_call(1);
                panic!("a panic in the body, on purpose")
            };
            let _ = if deep { nested(it, body) } else { tx(it, body) };
        });
        nothing_left(&it, all, what);
        assert_eq!(committed_in(&d, &url), "0", "{}", what);
    }
    assert_eq!(ok(it.call(&write, vec![Q::str("kept")], 0)).display(), "wrote");
    assert_eq!(committed_in(&d, &url), "1");
    drop(it);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn q858_review_a_panic_puts_back_what_a_transaction_catches() {
    // What a statement dropped in the body is caught for the transaction, the outer one's kept
    // aside: both were left as the body had them after a panic, the catching raised for good.
    let d = support::temp_dir("feedback56-caught");
    let mut it = Interp::new(&d);
    it.load_source(&format!("db.open(\"app.db\")\ndb.exec(\"create table notes (id integer primary key, text text)\")\n{}", PANIC_HELPERS), "main.qwe").unwrap();
    it.dropped = Some(Q::str("the outer one's"));
    for (what, deep) in [("outermost", false), ("nested", true)] {
        panics(&mut it, what, |it| {
            let _ = if deep { nested(it, &mut |_| panic!("a panic in the body, on purpose")) } else { tx(it, &mut |_| panic!("a panic in the body, on purpose")) };
        });
        assert_eq!(it.dropped.as_ref().map(|v| v.display()).as_deref(), Some("the outer one's"), "{}", what);
        nothing_left(&it, 0, what);
    }
    it.dropped = None;
    assert_eq!(ok(it.call(&global(&it, "write"), vec![Q::str("kept")], 0)).display(), "wrote");
    drop(it);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn q858_review_a_panic_in_db_atomic_on_sqlite() {
    let d = support::temp_dir("feedback56-atomic-sqlite");
    let src = format!(
        "db.open(\"app.db\")\ndb.exec(\"create table notes (id integer primary key, text text)\")\n{}\
         fn moved() {{\n    db.exec(\"insert into notes (text) values ('in')\")\n    1\n}}\n\
         fn mine() = [db]\n\
         fn decisions() -> int = db.one(\"select count(*) as n from _quillo_atomic\").n\n\
         fn serializable() = db.transaction({{ isolation: \"serializable\" }}, () => db.exec(\"insert into notes (text) values ('s')\").changes)\n",
        PANIC_HELPERS
    );
    let mut it = Interp::new(&d);
    it.load_source(&src, "main.qwe").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    let (run, mine, moved) = (global(&it, "run_atomic"), global(&it, "mine"), global(&it, "moved"));
    let db = ok(it.call(&mine, vec![], 0));
    // The function panics; then SQLite's part commits, its decision kept with it (the one call
    // holding a connection here, so the first): the savepoint was left open on the shared
    // connection, the thread held, and each later write landed in it, uncommitted.
    for (what, f, at) in [("its function", Q::Builtin("boom", boom), 0), ("its commit", moved.clone(), 1)] {
        panics(&mut it, what, |it| {
            quillo::db::panic_at_db_call(at);
            let _ = it.call(&run, vec![db.clone(), f], 0);
        });
        nothing_left(&it, 0, what);
        assert_eq!(committed_in(&d, "app.db"), "0", "{}", what);
    }
    assert_eq!(ok(it.call(&global(&it, "decisions"), vec![], 0)).display(), "0");
    // Not inside another: the isolation is taken; and a write after it is committed.
    assert_eq!(ok(it.call(&global(&it, "serializable"), vec![], 0)).display(), "1");
    assert_eq!(ok(it.call(&global(&it, "write"), vec![Q::str("kept")], 0)).display(), "wrote");
    assert_eq!(committed_in(&d, "app.db"), "2");
    // And `db.atomic` as ever.
    assert_eq!(ok(it.call(&run, vec![db, moved], 0)).display(), "1");
    assert_eq!(committed_in(&d, "app.db"), "3");
    nothing_left(&it, 0, "after");
    drop(it);
    let _ = std::fs::remove_dir_all(&d);
}

/// A program on server database `a` with connections to `b` and to SQLite, for `db.atomic`;
/// `serial` its kind of key, `prepared` what lists its prepared transactions.
fn atomic_program(d: &Path, a: &str, b: &str, serial: &str, prepared: &str) -> Interp {
    let src = format!(
        "db.open({a:?})\nlet other = db.connect({b:?})\nlet local = db.connect(\"local.db\")\n\
         db.exec(\"create table notes (id {serial} primary key, text text)\")\n\
         other.exec(\"create table notes (id {serial} primary key, text text)\")\n\
         local.exec(\"create table notes (id integer primary key, text text)\")\n{helpers}\
         fn moved() {{\n    db.exec(\"insert into notes (text) values ('in')\")\n    other.exec(\"insert into notes (text) values ('in')\")\n    local.exec(\"insert into notes (text) values ('in')\")\n    1\n}}\n\
         fn three() = [db, other, local]\n\
         fn local_first() = [local, db, other]\n\
         fn prepared() -> int = len(db.query({prepared:?}))\n\
         fn decisions() -> int = db.one(\"select count(*) as n from _quillo_atomic\").n\n",
        helpers = PANIC_HELPERS
    );
    let mut it = Interp::new(d);
    it.load_source(&src, "main.qwe").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    it
}

/// What another program sees in each of `db.atomic`'s three databases: what was committed.
fn committed_in_three(d: &Path, a: &str, b: &str) -> String {
    let mut other = Interp::new(d);
    let src = format!(
        "db.open({:?})\nlet other = db.connect({:?})\nlet local = db.connect(\"local.db\")\n\
         [db.one(\"select count(*) as n from notes\").n, other.one(\"select count(*) as n from notes\").n, local.one(\"select count(*) as n from notes\").n]\n",
        a, b
    );
    other.load_source(&src, "other.qwe").unwrap().display()
}

/// `db.atomic` on a server database `a`, another `b` and SQLite, panicking at each step. The
/// calls holding a connection, for `moved` over the three: 1 and 2 begin the server parts, 3 and
/// 4 are the function's statements, 5 and 6 prepare, 7 commits SQLite's part, 8 and 9 commit the
/// prepared ones.
fn atomic_panics_on(it: &mut Interp, d: &Path, a: &str, b: &str) {
    let (run, moved) = (global(it, "run_atomic"), global(it, "moved"));
    let all = free(it);
    let call = |it: &mut Interp, name: &str| ok(it.call(&global(it, name), vec![], 0));
    let (mut seen, mut first) = (0, true);
    for (what, dbs, f, at, stands) in [
        ("its function", "three", Q::Builtin("boom", boom), 0, false),
        ("a call in its function", "three", Q::Builtin("boom_in_a_call", boom_in_a_call), 0, false),
        ("a part's begin, SQLite's listed first (begun last)", "local_first", moved.clone(), 2, false),
        ("the second part's prepare", "three", moved.clone(), 6, false),
        ("SQLite's commit, the decision recorded", "three", moved.clone(), 7, false),
        ("the last part's commit, the decision standing", "three", moved.clone(), 9, true),
    ] {
        let dbs = call(it, dbs);
        // Its table is made by the first `db.atomic`.
        let decided = if first { "0".into() } else { call(it, "decisions").display() };
        first = false;
        panics(it, what, |it| {
            quillo::db::panic_at_db_call(at);
            let _ = it.call(&run, vec![dbs, f], 0);
        });
        nothing_left(it, all, what);
        seen += stands as i64;
        assert_eq!(committed_in_three(d, a, b), format!("[{s}, {s}, {s}]", s = seen), "{}", what);
        assert_eq!(call(it, "prepared").display(), "0", "{}: a part is left prepared", what);
        if !stands {
            assert_eq!(call(it, "decisions").display(), decided, "{}: a decision to commit is left", what);
        }
    }
    // And `db.atomic` as ever, all of it or none of it.
    let three = call(it, "three");
    assert_eq!(ok(it.call(&run, vec![three.clone(), moved], 0)).display(), "1");
    assert_eq!(committed_in_three(d, a, b), format!("[{s}, {s}, {s}]", s = seen + 1));
    assert!(matches!(ok(it.call(&run, vec![three, Q::Builtin("fails", fails)], 0)), Q::Err(..)));
    assert_eq!(committed_in_three(d, a, b), format!("[{s}, {s}, {s}]", s = seen + 1));
    nothing_left(it, all, "after");
}

#[test]
fn q858_review_a_panic_in_db_atomic_on_postgres() {
    let Some(pg) = support::postgres("feedback56-atomic-pg") else {
        eprintln!("skipped: q858_review_a_panic_in_db_atomic_on_postgres, as PostgreSQL's initdb and pg_ctl were not found");
        return;
    };
    // Prepared transactions, as `db.atomic` needs.
    let out = Command::new(pg.bin.join("psql")).args([pg.url.as_str(), "-q", "-c", "alter system set max_prepared_transactions = 10"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let data = std::env::temp_dir().join(format!("quillo-pg-feedback56-atomic-pg-{}", std::process::id()));
    let out = Command::new(pg.bin.join("pg_ctl")).arg("-D").arg(&data).arg("-l").arg(data.join("log")).args(["-w", "restart"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let (a, b) = (format!("{}?pool_size=2", pg.database("atomic_a")), pg.database("atomic_b"));
    let d = support::temp_dir("feedback56-atomic-pg");
    let mut it = atomic_program(&d, &a, &b, "serial", "select gid from pg_prepared_xacts");
    atomic_panics_on(&mut it, &d, &a, &b);
    drop(it);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn q858_review_a_panic_in_db_atomic_and_a_call_on_mariadb() {
    let Some(my) = support::mariadb("feedback56-atomic-my") else {
        eprintln!("skipped: q858_review_a_panic_in_db_atomic_and_a_call_on_mariadb, as MariaDB's mariadbd and mariadb-install-db were not found");
        return;
    };
    let (a, b) = (my.url("atomic_a"), my.url("atomic_b"));
    let d = support::temp_dir("feedback56-atomic-my");
    let mut it = atomic_program(&d, &a, &b, "int auto_increment", "xa recover");
    atomic_panics_on(&mut it, &d, &a, &b);
    // A call that panics in a transaction: its connection goes back to MariaDB's pool, which
    // resets it, and nothing stays pinned.
    let write = global(&it, "write");
    let before = committed_in_three(&d, &a, &b);
    for (what, deep) in [("in a transaction", false), ("in a nested one", true)] {
        panics(&mut it, what, |it| {
            let body = &mut |it: &mut Interp| {
                quillo::db::panic_at_db_call(1);
                it.call(&write, vec![Q::str("lost")], 0)
            };
            let _ = if deep { nested(it, body) } else { tx(it, body) };
        });
        nothing_left(&it, 0, what);
        assert_eq!(committed_in_three(&d, &a, &b), before, "{}", what);
    }
    assert_eq!(ok(it.call(&write, vec![Q::str("kept")], 0)).display(), "wrote");
    assert_ne!(committed_in_three(&d, &a, &b), before);
    drop(it);
    let _ = std::fs::remove_dir_all(&d);
}

// ---- Slice 4, review ----------------------------------------------------------------------------

/// The tools whose answers reach the door's limits: 10 MiB of JSON, and 128 levels.
const LIMITS_SRC: &str = r#"fn nest(n: int) -> list = if n <= 1 { [] } else { [nest(n - 1)] }

type Box = { inside: any }

/// A list `n` deep.
fn mcp deep(n: int) -> list with time = nest(n)

/// A record whose field is a list `n` deep.
fn mcp boxed(n: int) -> Box with time = Box({ inside: nest(n) })

/// The list, untyped.
fn mcp loose(n: int) with time = nest(n)

/// A text of `n` characters.
fn mcp long(n: int) -> str with time = repeat("x", n)

/// The text, untyped.
fn mcp text(n: int) with time = repeat("x", n)

/// An error of `n` characters.
fn mcp fails(n: int) -> int ! str with time = err(repeat("x", n))

/// An error that is a map.
fn mcp fails_map(n: int) -> int ! { why: str } with time = err({ why: repeat("x", n) })

/// A reply of `n` characters.
fn mcp replies(n: int) with time {
    reply 400 repeat("x", n)
}
"#;

/// `n` lists, one in another.
fn nest_json(n: usize) -> Value {
    let mut v = json!([]);
    for _ in 1..n {
        v = json!([v]);
    }
    v
}

#[test]
fn c1_review_an_answer_past_the_doors_limits_is_said_never_cut() {
    let d = dir("c1-limits", LIMITS_SRC);
    let mut door = Stdio::start(&d, &["main.qwe"]);
    let most = 10 << 20;
    let too_deep = "the result is too deep for the client: it nests more than 128 deep";
    let too_large = "the result is too large for the client: more than 10 MiB of JSON";
    let error = |r: &Value| {
        assert_eq!(r["isError"], json!(true), "{}", r.to_string().get(..300).unwrap_or_default());
        assert!(r.get("structuredContent").is_none());
        r["content"][0]["text"].as_str().unwrap().to_string()
    };
    // 128 levels, the most the door takes in, and one past: it was a silent `{"result": null}`.
    assert_eq!(door.call("deep", json!({ "n": 128 }))["structuredContent"], json!({ "result": nest_json(128) }));
    assert_eq!(door.call("deep", json!({ "n": 127 }))["structuredContent"], json!({ "result": nest_json(127) }));
    assert_eq!(error(&door.call("deep", json!({ "n": 129 }))), too_deep);
    door.says("quillo: error in tool `deep`: its result nests more than 128 deep");
    // A record is not held as `result`: its own 128 levels, where it lost its structuredContent.
    let r = door.call("boxed", json!({ "n": 127 }));
    assert_eq!(r["structuredContent"], json!({ "inside": nest_json(127) }));
    assert_eq!(r["content"][0]["text"], json!(r["structuredContent"].to_string()));
    assert_eq!(error(&door.call("boxed", json!({ "n": 128 }))), too_deep);
    // Untyped, held the same.
    assert_eq!(door.call("loose", json!({ "n": 128 }))["content"][0]["text"], json!(nest_json(128).to_string()));
    assert_eq!(error(&door.call("loose", json!({ "n": 129 }))), too_deep);
    // 10 MiB of JSON, the most a message is, and one past: a text's JSON has its quotes.
    let r = door.call("long", json!({ "n": most - 2 }));
    assert_eq!(r["structuredContent"]["result"].as_str().map(str::len), Some(most - 2));
    assert_eq!(error(&door.call("long", json!({ "n": most - 1 }))), too_large);
    door.says("quillo: error in tool `long`: its result is more than 10 MiB of JSON");
    // Untyped text goes as it is: 10 MiB of it.
    assert_eq!(door.call("text", json!({ "n": most }))["content"][0]["text"].as_str().map(str::len), Some(most));
    assert_eq!(error(&door.call("text", json!({ "n": most + 1 }))), too_large);
    // An error's payload and a reply's body, as text: held to the same size.
    assert_eq!(error(&door.call("fails", json!({ "n": most }))).len(), most);
    assert_eq!(error(&door.call("fails", json!({ "n": most + 1 }))), too_large);
    assert_eq!(error(&door.call("fails_map", json!({ "n": most - 10 }))).len(), most);
    assert_eq!(error(&door.call("fails_map", json!({ "n": most - 9 }))), too_large);
    assert_eq!(error(&door.call("replies", json!({ "n": most }))).len(), most);
    assert_eq!(error(&door.call("replies", json!({ "n": most + 1 }))), too_large);
    door.close();
    assert_eq!(door.ended(), Some(0));
    let _ = std::fs::remove_dir_all(&d);
}

/// `lines` through `mcp::serve_stdio` in this process, as stdin and stdout would carry them:
/// what it answered, a line each.
fn door_here(it: &mut Interp, door: &mcp::Door, lines: &[String]) -> Vec<Value> {
    let mut input = std::io::Cursor::new(lines.iter().map(|l| format!("{}\n", l)).collect::<String>().into_bytes());
    let mut out = Vec::new();
    assert_eq!(mcp::serve_stdio(door, it, &mut input, &mut out), 0);
    String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

/// An answer's result as `Stdio::call` gives it: an error's text, or its structured content.
fn said_in(a: &Value) -> Value {
    let r = &a["result"];
    match r["isError"] == json!(true) {
        true => r["content"][0]["text"].clone(),
        false => r["structuredContent"].clone(),
    }
}

#[test]
fn c1_review_a_panic_in_a_tool_is_that_calls_internal_error() {
    // A panic, a bug of Quillo's, in a tool that runs in the read-only transaction: the
    // transaction lets go of what it held and raises it again; the door answers that call
    // `internal error` and goes on, a tool `with time` after a `with db` one as it would.
    let d = support::temp_dir("feedback56-c1-panic");
    let db = d.join("app.db").display().to_string();
    let src = "let hook = () => 1\n/// Peeks.\nfn mcp peek() with db = hook()\n\
/// Counts.\nfn mcp total() -> int with db = db.one(\"select count(*) as n from notes\").n\n\
/// Adds.\nfn mcp add(t: str) -> int with db = db.exec(\"insert into notes (text) values (?)\", t).changes\n\
/// A time.\nfn mcp stamp() -> int with time = 7\n";
    let (mut it, mut list) = run_on(&d, &db, &["create table notes (id integer primary key, text text)"], src);
    quillo::value::define(&it.globals, "hook", Q::Builtin("boom", boom));
    list.iter_mut().find(|t| t.name == "peek").unwrap().reads_only = true;
    let door = mcp::Door::new("main", list);
    let calls: Vec<String> = [("peek", json!({})), ("stamp", json!({})), ("add", json!({ "t": "a" })), ("total", json!({})), ("peek", json!({})), ("peek", json!({})), ("stamp", json!({})), ("total", json!({}))]
        .iter()
        .map(|(n, a)| call_line(n, a.clone()))
        .collect();
    let answers = door_here(&mut it, &door, &calls);
    let said: Vec<Value> = answers.iter().map(said_in).collect();
    assert_eq!(said, [json!("internal error"), json!({ "result": 7 }), json!({ "result": 1 }), json!({ "result": 1 }), json!("internal error"), json!("internal error"), json!({ "result": 7 }), json!({ "result": 1 })], "{:?}", answers);
    nothing_left(&it, 0, "the door after a panic");
    drop(it);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn c1_review_on_postgres_a_panic_in_a_database_call_is_that_calls_internal_error() {
    let Some(pg) = support::postgres("feedback56-c1-panic") else {
        eprintln!("skipped: c1_review_on_postgres_a_panic_in_a_database_call_is_that_calls_internal_error, as PostgreSQL's initdb and pg_ctl were not found");
        return;
    };
    let url = pg.database("cpanic");
    let d = support::temp_dir("feedback56-c1-pgpanic");
    let src = "/// Counts.\nfn mcp total() -> int with db = db.one(\"select count(*) as n from notes\").n\n\
/// Adds.\nfn mcp add(t: str) -> int with db = db.exec(\"insert into notes (text) values (?)\", t).changes\n\
/// A time.\nfn mcp stamp() -> int with time = 7\n";
    let (mut it, list) = run_on(&d, &url, &["create table notes (id serial primary key, text text)"], src);
    assert!(the(&list, "total").reads_only);
    let door = mcp::Door::new("main", list);
    let before = free(&it);
    // The second call that holds a connection panics in it: the statement inside the read-only
    // transaction (the first is its `begin`), which rolls back and raises the panic again.
    quillo::db::panic_at_db_call(2);
    let calls: Vec<String> = [("total", json!({})), ("stamp", json!({})), ("add", json!({ "t": "é" })), ("total", json!({}))].iter().map(|(n, a)| call_line(n, a.clone())).collect();
    let answers = door_here(&mut it, &door, &calls);
    quillo::db::panic_at_db_call(0);
    let said: Vec<Value> = answers.iter().map(said_in).collect();
    assert_eq!(said, [json!("internal error"), json!({ "result": 7 }), json!({ "result": 1 }), json!({ "result": 1 })], "{:?}", answers);
    nothing_left(&it, before, "the door after a panic on PostgreSQL");
    drop(it);
    drop(pg);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn c1_review_what_a_plugin_writes_to_stdout_goes_to_stderr() {
    // Rust in a `.qwer` writes to descriptor 1 itself, past `print`: under `quillo mcp` that is
    // stderr, and stdout carries the answer alone. Built as tests/native.rs builds plugins.
    let d = dir("c1-plugin", "use \"loud.qwer\"\n\n/// Writes to stdout past `print`.\nfn mcp loud(text: str) -> int with time = shout(text)\n");
    std::fs::write(
        d.join("loud.qwer"),
        "//! effects: none\n\n/// Writes `text` to stdout as Rust code does, in two writes.\npub fn shout(text: &str) -> i64 {\n    use std::io::Write;\n    let mut out = std::io::stdout();\n    let _ = write!(out, \"{} \", text);\n    let _ = out.flush();\n    let _ = writeln!(out, \"(from Rust)\");\n    let _ = out.flush();\n    1\n}\n",
    )
    .unwrap();
    let cache = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("native-cache");
    let mut door = Stdio::start_env(&d, &["main.qwe"], &[("QUILLO_NATIVE_CACHE", cache.to_str().unwrap())]);
    door.send(&call_line("loud", json!({ "text": "on descriptor 1" })));
    // The first build of the cache's dependencies takes a while.
    let a = door.next_within(Duration::from_secs(300)).unwrap_or_else(|| panic!("no answer: {}", door.stderr()));
    assert_eq!(a["result"]["structuredContent"], json!({ "result": 1 }), "{}", a);
    door.close();
    assert_eq!(door.ended(), Some(0));
    assert!(door.next_within(Duration::from_millis(200)).is_none());
    assert!(door.stderr().contains("on descriptor 1 (from Rust)\n"), "{}", door.stderr());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn c1_review_the_policys_limits_hold_the_door() {
    let d = dir("c1-policy", "/// Spins.\nfn mcp spin() -> int with time {\n    let i = 0\n    while i >= 0 { i = i + 1 }\n    i\n}\n\n/// Seven.\nfn mcp clock() -> int with time = 7\n");
    std::fs::write(d.join("limits.toml"), "[limits]\nmax_body = \"2KB\"\nmax_compute = \"300ms\"\n").unwrap();
    let mut door = Stdio::start(&d, &["main.qwe", "--policy", "limits.toml"]);
    // `max_compute`: a call computing past it ends, as a request does, and the door goes on.
    let began = Instant::now();
    let r = door.call("spin", json!({}));
    assert_eq!((r["isError"].clone(), r["content"][0]["text"].clone()), (json!(true), json!("internal error")), "{}", r);
    assert!(began.elapsed() < Duration::from_secs(10), "{:?}", began.elapsed());
    door.says("this request computed for longer than the policy limits.toml allows (max_compute)");
    assert_eq!(door.call("clock", json!({}))["structuredContent"], json!({ "result": 7 }));
    // `max_body`: the longest message, the limit and one either side.
    let padded = |id: i64, len: usize| {
        let m = request(json!(id), "tools/list", json!({}));
        format!("{}{}", m, " ".repeat(len - m.len()))
    };
    assert_eq!(door.ask(&padded(1, 2047))["id"], json!(1));
    assert_eq!(door.ask(&padded(2, 2048))["id"], json!(2));
    assert_eq!(door.refusal(&padded(3, 2049)), (None, -32600, "Invalid request: a message is at most 2 KiB".into()));
    assert_eq!(door.ask(&request(json!(4), "tools/list", json!({})))["id"], json!(4));
    door.close();
    assert_eq!(door.ended(), Some(0));
    let _ = std::fs::remove_dir_all(&d);
}

#[cfg(unix)]
#[test]
fn c1_review_a_message_begun_before_sigterm_is_answered() {
    // The door is busy from a message's first byte: a stop while the rest is on its way waits for
    // its answer, where it ended the process with the message read and unanswered.
    let d = door_dir("half");
    let mut door = Stdio::start(&d, &["main.qwe"]);
    assert_eq!(door.call("clock", json!({}))["structuredContent"], json!({ "result": 7 }));
    let m = call_line("hello", json!({ "name": "Ana" }));
    let (first, rest) = m.split_at(m.len() / 2);
    door.write(first.as_bytes());
    std::thread::sleep(Duration::from_millis(300));
    unsafe { libc::kill(door.pid() as i32, libc::SIGTERM) };
    door.says("stopping once the call in progress is answered");
    door.write(format!("{}\n", rest).as_bytes());
    let a = door.next().expect("the message begun was not answered");
    assert_eq!(a["result"]["content"][0]["text"], json!("hi Ana!"), "{}", a);
    assert_eq!(door.ended(), Some(0));
    let _ = std::fs::remove_dir_all(&d);
}

// ---- Slice 3, second panic review: a transaction's begin, db.atomic's wait, a request's state --

/// A panic as a transaction begins, just after its begin (the first call after arming), before
/// its body: an outermost read-only one, an outermost one, a nested one. The begin was outside
/// the catch around the body: the connection stayed pinned (SQLite's savepoint open), and every
/// later transaction nested in it, the next read-only one refused (found in review).
fn begin_panics_on(it: &mut Interp, what: &str) {
    let (count_fn, write) = (global(it, "count"), global(it, "write"));
    let all = free(it);
    for (case, deep, read_only) in [("read-only", false, true), ("db.transaction's", false, false), ("nested", true, false)] {
        let what = format!("{}, {}", what, case);
        let before = count(it);
        panics(it, &what, |it| {
            let opts = quillo::txn::Options { read_only, ..Default::default() };
            let _ = if deep {
                tx(it, &mut |it| {
                    quillo::db::panic_at_db_call(1);
                    tx(it, &mut |it| it.call(&write, vec![Q::str("lost")], 0))
                })
            } else {
                quillo::db::panic_at_db_call(1);
                quillo::txn::run(it, None, &opts, 0, &mut |it| it.call(&write, vec![Q::str("lost")], 0))
            };
        });
        nothing_left(it, all, &what);
        // Outermost again: a read-only one runs, and an isolation level is taken.
        assert!(matches!(ok(mcp::reading_only(it, 0, &mut |it| it.call(&count_fn, vec![], 0))), Q::Int(_)), "{}: the next read-only one", what);
        let serializable = quillo::txn::Options { isolation: Some(quillo::txn::Isolation::Serializable), ..Default::default() };
        assert_eq!(ok(quillo::txn::run(it, None, &serializable, 0, &mut |it| it.call(&write, vec![Q::str("kept")], 0))).display(), "wrote", "{}", what);
        assert_eq!(count(it), before + 1, "{}", what);
    }
}

#[test]
fn q858_review_a_panic_as_a_transaction_begins_leaves_nothing_open() {
    let d = support::temp_dir("feedback56-begin-panic");
    let (mut it, _) = run_on(&d, &d.join("app.db").display().to_string(), &["create table notes (id integer primary key, text text)"], RO_TOOLS);
    begin_panics_on(&mut it, "SQLite");
    drop(it);
    match support::postgres("feedback56-begin-pg") {
        Some(pg) => {
            let (mut it, _) = run_on(&d, &pg.database("beginpanic"), &["create table notes (id serial primary key, text text)"], RO_TOOLS);
            begin_panics_on(&mut it, "PostgreSQL");
        }
        None => eprintln!("skipped on PostgreSQL: q858_review_a_panic_as_a_transaction_begins_leaves_nothing_open, as initdb and pg_ctl were not found"),
    }
    match support::mariadb("feedback56-begin-my") {
        Some(my) => {
            let (mut it, _) = run_on(&d, &my.url("beginpanic"), &["create table notes (id int auto_increment primary key, text text)"], RO_TOOLS);
            begin_panics_on(&mut it, "MariaDB");
        }
        None => eprintln!("skipped on MariaDB: q858_review_a_panic_as_a_transaction_begins_leaves_nothing_open, as mariadbd was not found"),
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn q858_review_db_atomic_lets_no_other_request_write_into_its_sqlite_part() {
    let Some(pg) = support::postgres("feedback56-atomic-wait") else {
        eprintln!("skipped: q858_review_db_atomic_lets_no_other_request_write_into_its_sqlite_part, as PostgreSQL's initdb and pg_ctl were not found");
        return;
    };
    let url = pg.database("atomicwait");
    let d = support::temp_dir("feedback56-atomic-wait");
    let src = format!(
        "db.open(\"app.db\")\ndb.exec(\"create table if not exists notes (id integer primary key, text text)\")\n\
         let other = db.connect(\"{url}?pool_size=1\")\nother.exec(\"create table if not exists t (n int)\")\n\
         route POST /hold = other.transaction(() => other.exec(\"select pg_sleep(1.5)\").changes)\n\
         fn moved(ms) = error(db.atomic([db, other], () => {{\n    db.exec(\"insert into notes (text) values ('atomic')\")\n    other.exec(\"insert into t values (1)\")\n    sleep(ms)\n    err(\"undone\")\n}}))\n\
         route POST /atomic = moved(0)\n\
         route POST /slow = moved(800)\n\
         route POST /write {{\n    db.exec(\"insert into notes (text) values ('other')\")\n    \"wrote\"\n}}\n\
         route GET /notes = json.stringify(db.query(\"select text from notes order by id\") | map((r) => r.text))\n"
    );
    std::fs::write(d.join("main.qwe"), &src).unwrap();
    let s = support::serve(&d.join("main.qwe"), &[]);
    let port = s.port;
    let started = Instant::now();
    let at = move |path: &'static str, after: u64| {
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(after));
            let a = support::send(port, "POST", path, &[], "");
            (a.status, a.body, started.elapsed())
        })
    };
    // The only connection to the other database held for 1.5 s: `db.atomic` waits for one, then
    // begins. Its SQLite part begins after the server parts have theirs, so a write made while it
    // waits is the other request's own, committed, where it went into the savepoint and was
    // rolled back with it.
    let (hold, atomic, write) = (at("/hold", 0), at("/atomic", 200), at("/write", 500));
    let write = write.join().unwrap();
    let atomic = atomic.join().unwrap();
    assert_eq!(hold.join().unwrap().0, 200);
    assert_eq!((write.0, write.1.as_str()), (200, "wrote"));
    assert!(atomic.1.contains("undone"), "it waited for its connection and ran: {}", atomic.1);
    assert!(write.2 < atomic.2, "the write ran while the atomic waited: {:?} {:?}", write.2, atomic.2);
    assert_eq!(support::send(port, "GET", "/notes", &[], "").body, r#"["other"]"#);
    // Once SQLite's part has begun no other request runs until it ends, as in `db.transaction`:
    // the write waits, then is committed on its own.
    let (slow, write) = (at("/slow", 0), at("/write", 300));
    let (slow, write) = (slow.join().unwrap(), write.join().unwrap());
    assert!(slow.1.contains("undone"), "{}", slow.1);
    assert_eq!(write.0, 200);
    assert!(write.2 >= Duration::from_millis(750), "the write ran inside the atomic: {:?} {:?}", write.2, slow.2);
    assert_eq!(support::send(port, "GET", "/notes", &[], "").body, r#"["other","other"]"#);
    drop(s);
    let _ = std::fs::remove_dir_all(&d);
}

/// A function of the program's whose call panics, as a bug of Quillo's in it would.
fn boom_in_a_request(_: &mut Interp, _: Vec<Q>, _: u32) -> Result<Q, quillo::interp::Ctrl> {
    panic!("a panic in a request, on purpose")
}

#[test]
fn q858_review_after_a_request_panics_the_next_has_its_effects() {
    // A request whose `with db` function panics answers 500, and the next, needing `time`, is
    // not run under that `with`: the server goes on with a fresh interpreter, not one left as
    // the call that panicked had narrowed it (found in review).
    let d = support::temp_dir("feedback56-request-panic");
    let f = d.join("app.qwe");
    std::fs::write(&f, "fn boomy() with db = boom()\nroute GET /boom = boomy()\nfn stamp() with time = type_of(now())\nroute GET /now = stamp()\n").unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let (file, dir) = (f.clone(), d.clone());
    std::thread::Builder::new()
        .stack_size(quillo::server::WORKER_STACK)
        .spawn(move || {
            let mut it = Interp::new(&dir);
            quillo::value::define(&it.globals, "boom", Q::Builtin("boom", boom_in_a_request));
            let shared = it.shared.clone();
            it.load_file(&file).unwrap();
            let app = quillo::server::App::new(quillo::server::Loaded::Ready(it), shared, &file, &dir, false);
            quillo::server::serve_with(app, "127.0.0.1:0", 2, move |a| tx.send(a.port()).unwrap()).unwrap();
        })
        .unwrap();
    let port = rx.recv_timeout(Duration::from_secs(10)).expect("the server did not start");
    assert_eq!(support::send(port, "GET", "/now", &[], "").body, "int");
    for _ in 0..3 {
        assert_eq!(support::send(port, "GET", "/boom", &[], "").status, 500);
        let now = support::send(port, "GET", "/now", &[], "");
        assert_eq!((now.status, now.body.as_str()), (200, "int"));
    }
    let _ = std::fs::remove_dir_all(&d);
}

// ---- Slice 5: the HTTP door, `/_quillo/mcp` (C.2 to C.4, Q-859, Q-860) --------------------------
//
// Driven against a real `quillo run` on a port the system picks (`support::serve_with`), each
// request under `support::send`'s 30 s read deadline, the server killed when the test is done
// with it (`support::Server`, `support::Guard`). The messages and headers are MCP 2026-07-28's
// (basic/transports/streamable-http, basic/authorization, server/tools).
mod http_door {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// The program the door is driven with. `tenant_from` names a tenant only for a request that
    /// says one (`x-tenant`), so the others have none.
    pub(super) const SRC: &str = r#"db.open(":memory:")
db.exec("create table notes (id integer primary key, text text not null)")
tenant_from((request) => request.headers["x-tenant"])

let bad = [{ colour: 1 }, { max_age: "0s" }, { max_age: "soon" }, 5]

route POST /login ? { user: str } = set_session({ user: user })
route POST /anon = set_session({ cart: [1] })
route POST /token = mcp_token()
route POST /token_for ? { user: str, age: str = "30d" } = mcp_token({ user: user, max_age: age })
route POST /token_bad/:how(int) = mcp_token(bad[how])
route GET /tokens = mcp_tokens()
route GET /tokens_of ? { user: str } = mcp_tokens({ user: user })
route POST /revoke ? { id: str } = str(revoke_mcp_token(id))
route POST /revoke_of ? { id: str, user: str } = str(revoke_mcp_token(id, { user: user }))
route POST /revoke_bad = revoke_mcp_token(json.parse("5"))
route POST /logout = end_session()
route GET /here = str(request.mcp)
route GET /count = str(db.one("select count(*) as n from notes").n)

/// Who is calling: the session's user, the client, the tenant.
server fn mcp whoami() -> map with time = { user: session().user, mcp: request.mcp, tenant: tenant() }

/// Adds a note: how many there are.
fn mcp add(text: str) -> int with db {
    db.exec("insert into notes (text) values (?)", text)
    db.one("select count(*) as n from notes").n
}

/// Needs a login.
fn mcp mine() -> str with time {
    session().user ?? reply 401 "Log in first"
}

/// Sets a cookie.
fn mcp sets() -> str with time {
    set_cookie("seen", "1")
    "set"
}

/// Gives back what it is given.
server fn mcp echo(v: any) with time = v

/// Spins.
fn mcp spin() -> int with time {
    let i = 0
    while i >= 0 { i = i + 1 }
    i
}

/// A plain server fn.
server fn plain_one() -> int = 1
"#;

    pub(super) const V: &str = "2026-07-28";

    /// A request of 2026-07-28 as a client sends one: `_meta` with the version, the client's
    /// capabilities and who the client is.
    pub(super) fn msg(id: Value, method: &str, mut params: Value) -> String {
        params["_meta"] = json!({
            "io.modelcontextprotocol/protocolVersion": V,
            "io.modelcontextprotocol/clientCapabilities": {},
            "io.modelcontextprotocol/clientInfo": { "name": "quillo-test", "version": "1.2.3" },
        });
        json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string()
    }

    /// Who a test calls as.
    #[derive(Clone, Debug)]
    pub(super) enum As {
        Nobody,
        Cookie(String),
        Token(String),
    }

    /// The headers MCP's transport mirrors from a message: the version, the method, and on
    /// `tools/call` the tool's name.
    pub(super) fn mirrored(method: &str, name: Option<&str>) -> Vec<(String, String)> {
        let mut h = vec![("MCP-Protocol-Version".to_string(), V.to_string()), ("Mcp-Method".to_string(), method.to_string())];
        if let Some(n) = name {
            h.push(("Mcp-Name".into(), n.into()));
        }
        h
    }

    /// A POST to the door as `who`, with `headers` as given (none added but who it is).
    pub(super) fn post(port: u16, who: &As, headers: &[(String, String)], body: &str) -> support::Answer {
        let mut h: Vec<(String, String)> = headers.to_vec();
        match who {
            As::Cookie(c) => h.push(("Cookie".into(), c.clone())),
            As::Token(t) => h.push(("Authorization".into(), format!("Bearer {}", t))),
            As::Nobody => {}
        }
        let refs: Vec<(&str, &str)> = h.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        support::send(port, "POST", "/_quillo/mcp", &refs, body)
    }

    pub(super) fn json_of(a: &support::Answer) -> Value {
        serde_json::from_str(&a.body).unwrap_or_else(|e| panic!("not JSON ({}): {} {:?}", e, a.status, a.body))
    }

    /// A message sent as a client sends it, its headers mirrored: the answer.
    pub(super) fn ask(port: u16, who: &As, id: Value, method: &str, params: Value) -> support::Answer {
        let name = params.get("name").and_then(Value::as_str).map(str::to_string);
        post(port, who, &mirrored(method, name.as_deref()), &msg(id, method, params))
    }

    /// A tool's call, answered 200 with a result: the result, `_meta` and `resultType` checked
    /// and taken out.
    pub(super) fn call(port: u16, who: &As, name: &str, args: Value) -> Value {
        let a = ask(port, who, json!(name), "tools/call", json!({ "name": name, "arguments": args }));
        assert_eq!(a.status, 200, "{} {}", name, a.body);
        assert_eq!(a.header("content-type"), Some("application/json"));
        assert_eq!(a.header("cache-control"), Some("no-store"));
        let v = json_of(&a);
        assert_eq!(v["id"], json!(name), "{}", v);
        let mut r = v["result"].clone();
        assert_eq!(r["resultType"], json!("complete"), "{}", v);
        assert_eq!(r["_meta"]["io.modelcontextprotocol/serverInfo"]["name"], json!("main"), "{}", v);
        r.as_object_mut().unwrap().remove("_meta");
        r.as_object_mut().unwrap().remove("resultType");
        r
    }

    /// The session cookie an answer sets, as `name=value`.
    pub(super) fn session_cookie(a: &support::Answer) -> String {
        a.headers
            .iter()
            .filter(|(k, _)| k == "set-cookie")
            .map(|(_, v)| v.split(';').next().unwrap_or("").to_string())
            .find(|c| c.contains("quillo_session="))
            .unwrap_or_else(|| panic!("no session cookie: {:?}", a.headers))
    }

    pub(super) fn login(port: u16, user: &str, extra: &[(&str, &str)]) -> String {
        let a = support::send(port, "POST", &format!("/login?user={}", user), extra, "");
        assert_eq!(a.status, 204, "{}", a.body);
        session_cookie(&a)
    }

    /// A route's answer as `who` (a cookie), with `extra` headers.
    pub(super) fn route(port: u16, method: &str, path: &str, cookie: &str, extra: &[(&str, &str)]) -> support::Answer {
        let mut h = vec![("Cookie", cookie)];
        h.extend_from_slice(extra);
        support::send(port, method, path, &h, "")
    }

    pub(super) fn token(port: u16, cookie: &str, extra: &[(&str, &str)]) -> String {
        let a = route(port, "POST", "/token", cookie, extra);
        assert_eq!(a.status, 200, "{}", a.body);
        assert!(a.body.starts_with("qmcp_") && a.body.len() == 48, "{}", a.body);
        a.body
    }

    /// The JSON-RPC error of a refusal: its status, id, code and message.
    pub(super) fn refusal(a: &support::Answer) -> (u16, Value, i64, String) {
        let v = json_of(a);
        assert!(v.get("result").is_none(), "{}", v);
        assert_eq!(v["jsonrpc"], json!("2.0"));
        (a.status, v.get("id").cloned().unwrap_or(Value::Null), v["error"]["code"].as_i64().unwrap_or_else(|| panic!("{}", v)), v["error"]["message"].as_str().unwrap_or_default().to_string())
    }

    pub(super) fn door(name: &str, opts: &[&str]) -> (PathBuf, support::Server) {
        let d = dir(&format!("c2-{}", name), SRC);
        let s = support::serve_with(&d.join("main.qwe"), opts);
        (d, s)
    }

    /// Nothing in an answer lets another site's page read it or send a call.
    pub(super) fn grants_nothing(a: &support::Answer) {
        assert!(!a.headers.iter().any(|(k, _)| k.starts_with("access-control-")), "{:?}", a.headers);
    }

    #[test]
    fn c2_every_refusal_has_its_code_and_status() {
        let (d, s) = door("refusals", &[]);
        let p = s.port;
        let ana = As::Cookie(login(p, "ana", &[]));
        let list = msg(json!(1), "tools/list", json!({}));
        let h = |pairs: &[(&str, &str)]| pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<Vec<_>>();
        let said = |a: &support::Answer| refusal(a);
        // The form that must keep working: the headers in any case, a request answered.
        let ok = post(p, &ana, &h(&[("mcp-protocol-version", V), ("MCP-METHOD", "tools/list")]), &list);
        assert_eq!(ok.status, 200, "{}", ok.body);
        assert_eq!(json_of(&ok)["result"]["tools"].as_array().map(Vec::len), Some(6));
        // HeaderMismatch, -32020 and 400: the version header missing, other than the body's, given
        // twice with two values, holding what no header may; the method's the same; the values in
        // their case.
        for (headers, why) in [
            (h(&[("Mcp-Method", "tools/list")]), "the MCP-Protocol-Version header is missing"),
            (h(&[("MCP-Protocol-Version", "2026-07-29"), ("Mcp-Method", "tools/list")]), "MCP-Protocol-Version header value '2026-07-29' does not match body value '2026-07-28'"),
            (h(&[("MCP-Protocol-Version", V), ("MCP-Protocol-Version", "2025-11-25"), ("Mcp-Method", "tools/list")]), "the MCP-Protocol-Version header is given twice, with two values"),
            (h(&[("MCP-Protocol-Version", V)]), "the Mcp-Method header is missing"),
            (h(&[("MCP-Protocol-Version", V), ("Mcp-Method", "tools/call")]), "Mcp-Method header value 'tools/call' does not match body value 'tools/list'"),
            (h(&[("MCP-Protocol-Version", V), ("Mcp-Method", "Tools/List")]), "Mcp-Method header value 'Tools/List' does not match body value 'tools/list'"),
            (h(&[("MCP-Protocol-Version", V), ("Mcp-Method", "tools/listé")]), "the Mcp-Method header holds a character a header may not"),
        ] {
            let (status, id, code, message) = said(&post(p, &ana, &headers, &list));
            assert_eq!((status, id, code), (400, json!(1), -32020), "{:?}", headers);
            assert!(message.starts_with("Header mismatch: ") && message.contains(why), "{:?}: {}", headers, message);
        }
        // The same header twice with one value is one value.
        let twice = h(&[("MCP-Protocol-Version", V), ("MCP-Protocol-Version", V), ("Mcp-Method", "tools/list")]);
        assert_eq!(post(p, &ana, &twice, &list).status, 200);
        // `Mcp-Name` on `tools/call`: missing, another tool's, base64 of the name (padded and not),
        // base64 of something else, not base64, another alphabet's.
        let call = msg(json!("w"), "tools/call", json!({ "name": "whoami" }));
        let named = |n: Option<&str>| mirrored("tools/call", n);
        for (name, why) in [
            (None, "the Mcp-Name header is missing".to_string()),
            (Some("echo"), "Mcp-Name header value 'echo' does not match body value 'whoami'".into()),
            (Some("=?base64?ZWNobw==?="), "Mcp-Name header value '=?base64?ZWNobw==?=' does not match body value 'whoami'".into()),
            (Some("=?base64?not base64!?="), "the Mcp-Name header's =?base64? value is not base64 of UTF-8 text".into()),
            (Some("=?base64?_-8?="), "the Mcp-Name header's =?base64? value is not base64 of UTF-8 text".into()),
            (Some("=?base64?/w==?="), "the Mcp-Name header's =?base64? value is not base64 of UTF-8 text".into()),
            (Some("WhoAmI"), "Mcp-Name header value 'WhoAmI' does not match body value 'whoami'".into()),
        ] {
            let (status, id, code, message) = said(&post(p, &ana, &named(name), &call));
            assert_eq!((status, id, code), (400, json!("w"), -32020), "{:?}", name);
            assert!(message.contains(&why), "{:?}: {}", name, message);
        }
        for name in ["=?base64?d2hvYW1p?=", "=?base64?d2hvYW1p==?=", "whoami"] {
            let a = post(p, &ana, &named(Some(name)), &call);
            assert_eq!(a.status, 200, "{}: {}", name, a.body);
            assert_eq!(json_of(&a)["result"]["structuredContent"]["user"], json!("ana"), "{}", name);
        }
        // `_meta` without what every request carries: -32602, 400 (basic/index).
        let bare = |meta: Value| json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": { "_meta": meta } }).to_string();
        for meta in [json!({ "io.modelcontextprotocol/protocolVersion": V }), json!({ "io.modelcontextprotocol/protocolVersion": V, "io.modelcontextprotocol/clientCapabilities": 5 })] {
            let (status, id, code, _) = said(&post(p, &ana, &mirrored("tools/list", None), &bare(meta)));
            assert_eq!((status, id, code), (400, json!(2), -32602));
        }
        // A version the door does not speak, in the body and the header alike, or (a client of an
        // earlier revision) in the header alone: -32022, 400, with the versions it does.
        let old = json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list", "params": { "_meta": { "io.modelcontextprotocol/protocolVersion": "2025-11-25", "io.modelcontextprotocol/clientCapabilities": {} } } }).to_string();
        let olds = h(&[("MCP-Protocol-Version", "2025-11-25"), ("Mcp-Method", "tools/list")]);
        let bare_old = json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list" }).to_string();
        for body in [old, bare_old] {
            let a = post(p, &ana, &olds, &body);
            let (status, id, code, _) = said(&a);
            assert_eq!((status, id, code), (400, json!(3), -32022), "{}", a.body);
            assert_eq!(json_of(&a)["error"]["data"], json!({ "supported": [V], "requested": "2025-11-25" }));
        }
        // `initialize`, which a client of 2025-11-25 sends with none of the headers: the version
        // error, not a missing header (decided 2026-11-08).
        let init = json!({ "jsonrpc": "2.0", "id": 0, "method": "initialize", "params": { "protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": { "name": "old", "version": "1" } } }).to_string();
        let a = post(p, &ana, &[], &init);
        let (status, id, code, _) = said(&a);
        assert_eq!((status, id, code), (400, json!(0), -32022), "{}", a.body);
        assert_eq!(json_of(&a)["error"]["data"]["requested"], json!("2025-11-25"));
        // A method the door does not have: -32601, 404; a tool it does not have: -32602, 400.
        let (status, _, code, message) = said(&ask(p, &ana, json!(4), "prompts/list", json!({})));
        assert_eq!((status, code, message.as_str()), (404, -32601, "Method not found: prompts/list"));
        let (status, _, code, message) = said(&ask(p, &ana, json!(5), "tools/call", json!({ "name": "nope" })));
        assert_eq!((status, code, message.as_str()), (400, -32602, "Unknown tool: nope"));
        // What is no request: not JSON (-32700), a batch, not an object, an id that is null (all
        // -32600), a response a client may not send; each 400, with no id where none was read.
        let mut list_h = mirrored("tools/list", None);
        list_h.push(("Content-Type".into(), "application/json".into()));
        for (body, code) in [("{", -32700), ("", -32700), ("[1]", -32600), ("\"x\"", -32600), (r#"{"jsonrpc":"2.0","id":null,"method":"tools/list"}"#, -32600), (r#"{"jsonrpc":"2.0","id":7,"result":{}}"#, -32600), (r#"{"jsonrpc":"2.0","error":{"code":1,"message":"x"}}"#, -32600)] {
            let a = post(p, &ana, &list_h, body);
            let (status, id, c, _) = said(&a);
            assert_eq!((status, c), (400, code), "{}: {}", body, a.body);
            assert!(id.is_null() || id == json!(7), "{}: {}", body, a.body);
        }
        // A notification is taken and not answered: 202, no body, whatever its headers.
        for headers in [list_h.clone(), Vec::new()] {
            let a = post(p, &ana, &headers, r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}"#);
            assert_eq!((a.status, a.body.as_str()), (202, ""), "{:?}", headers);
        }
        // POST only: GET (2025-11-25's stream), DELETE (its end of a session), PUT, HEAD 405 with
        // `Allow`.
        for method in ["GET", "DELETE", "PUT", "HEAD", "PATCH"] {
            let a = support::send(p, method, "/_quillo/mcp", &[("Cookie", match &ana { As::Cookie(c) => c, _ => unreachable!() })], "");
            assert_eq!((a.status, a.header("allow")), (405, Some("POST")), "{}", method);
        }
        // JSON only: plain text, a form, none said; 415 before anything is read.
        let list_h = mirrored("tools/list", None);
        for ctype in ["text/plain", "application/x-www-form-urlencoded", "application/jsonx", ""] {
            let mut headers = list_h.clone();
            headers.push(("Content-Type".into(), ctype.into()));
            let a = post(p, &ana, &headers, if ctype.is_empty() { "" } else { &list });
            assert_eq!(a.status, 415, "{:?}: {}", ctype, a.body);
        }
        // With its parameters, in any case, JSON is JSON.
        let mut headers = list_h.clone();
        headers.push(("Content-Type".into(), "Application/JSON; charset=utf-8".into()));
        assert_eq!(post(p, &ana, &headers, &list).status, 200);
        // 2025-11-25's session header is not read, and none is made.
        let mut headers = list_h.clone();
        headers.push(("Mcp-Session-Id".into(), "abc".into()));
        let a = post(p, &ana, &headers, &list);
        assert_eq!(a.status, 200);
        assert!(a.header("mcp-session-id").is_none(), "{:?}", a.headers);
        drop(s);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn c3_with_no_caller_the_door_is_closed() {
        let (d, s) = door("nobody", &[]);
        let p = s.port;
        let list = msg(json!(1), "tools/list", json!({}));
        let mut h = mirrored("tools/list", None);
        h.push(("Content-Type".into(), "application/json".into()));
        // No cookie, no token: 401 with the challenge, even for what the door would refuse anyway:
        // a stranger learns nothing of it.
        for body in [list.as_str(), "{", ""] {
            let a = post(p, &As::Nobody, &h, body);
            assert_eq!((a.status, a.header("www-authenticate")), (401, Some("Bearer")), "{}", body);
            assert!(a.body.contains("mcp_token()"), "{}", a.body);
        }
        // A session no one is signed in to (a visitor's cart), a cookie changed, an unknown name: 401.
        let anon = session_cookie(&support::send(p, "POST", "/anon", &[], ""));
        let ana = login(p, "ana", &[]);
        let (name, value) = ana.split_once('=').unwrap();
        let changed = format!("{}={}x", name, value);
        for c in [anon.clone(), changed, format!("other_session={}", value)] {
            let a = post(p, &As::Cookie(c.clone()), &h, &list);
            assert_eq!((a.status, a.header("www-authenticate")), (401, Some("Bearer")), "{}", c);
        }
        // Another scheme is no token: refused alone, and with a signed-in cookie the cookie is read.
        let mut basic = h.clone();
        basic.push(("Authorization".into(), "Basic YTpi".into()));
        assert_eq!(post(p, &As::Nobody, &basic, &list).status, 401);
        assert_eq!(post(p, &As::Cookie(ana.clone()), &basic, &list).status, 200);
        // A token the door does not have, an empty one, one in any case of `Bearer`: 401 with
        // `invalid_token`; a good one, with `bearer`, is let in.
        let good = token(p, &ana, &[]);
        for t in ["qmcp_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "", "x", "é"] {
            let mut with = h.clone();
            with.push(("Authorization".into(), format!("Bearer {}", t)));
            let a = post(p, &As::Nobody, &with, &list);
            assert_eq!((a.status, a.header("www-authenticate")), (401, Some("Bearer error=\"invalid_token\"")), "{:?}", t);
            // A bad token is no session's even with a good cookie beside it.
            assert_eq!(post(p, &As::Cookie(ana.clone()), &with, &list).status, 401, "{:?}", t);
        }
        let mut lower = h.clone();
        lower.push(("Authorization".into(), format!("bearer {}", good)));
        assert_eq!(post(p, &As::Nobody, &lower, &list).status, 200);
        drop(s);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A token's life on `db_line`'s store: made, used, listed, revoked, ended with its session.
    fn tokens_on(name: &str, db_line: &str) {
        let src = format!("{}\n{}", db_line, LIFE_SRC);
        let d = dir(&format!("c3-life-{}", name), &src);
        let s = support::serve_with(&d.join("main.qwe"), &["--watch"]);
        let p = s.port;
        let ana = login(p, "ana", &[]);
        let cookie = As::Cookie(ana.clone());
        // The cookie: `session()` is the visitor's, and a cookie a tool sets goes back.
        assert_eq!(call(p, &cookie, "whoami", json!({}))["structuredContent"]["user"], json!("ana"));
        let a = ask(p, &cookie, json!(1), "tools/call", json!({ "name": "sets" }));
        assert!(a.headers.iter().any(|(k, v)| k == "set-cookie" && v.starts_with("seen=1")), "{:?}", a.headers);
        // A token: given once, and the session is its own in a tool; a cookie set there has no
        // browser to go back to.
        let one = token(p, &ana, &[]);
        let two = token(p, &ana, &[]);
        assert_ne!(one, two);
        let t1 = As::Token(one.clone());
        assert_eq!(call(p, &t1, "whoami", json!({}))["structuredContent"]["user"], json!("ana"));
        assert_eq!(call(p, &t1, "mine", json!({}))["structuredContent"], json!({ "result": "ana" }));
        let a = ask(p, &t1, json!(1), "tools/call", json!({ "name": "sets" }));
        assert_eq!(a.status, 200);
        assert!(!a.headers.iter().any(|(k, _)| k == "set-cookie"), "{:?}", a.headers);
        // Listed oldest first, by id and never by token; for the user too.
        let listed: Value = serde_json::from_str(&route(p, "GET", "/tokens", &ana, &[]).body).unwrap();
        let ids: Vec<String> = listed.as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap().to_string()).collect();
        assert_eq!(ids.len(), 2, "{}", listed);
        for t in listed.as_array().unwrap() {
            let keys: Vec<&String> = t.as_object().unwrap().keys().collect();
            assert_eq!(keys, ["id", "user", "made", "expires"], "{}", t);
            assert_eq!(t["user"], json!("ana"));
            let (made, ends) = (t["made"].as_i64().unwrap(), t["expires"].as_i64().unwrap());
            assert_eq!(ends - made, 30 * 86_400 * 1000);
        }
        assert!(!listed.to_string().contains(&one[5..]) && !listed.to_string().contains(&two[5..]));
        // Another session of Ana's finds them by her user, as a sign-in again does after a cookie
        // has gone (decided in review); by the user, all of them; another user's, none.
        let ana2 = login(p, "ana", &[]);
        let again: Value = serde_json::from_str(&route(p, "GET", "/tokens", &ana2, &[]).body).unwrap();
        assert_eq!(again, listed);
        let bob0 = login(p, "bob", &[]);
        assert_eq!(route(p, "GET", "/tokens", &bob0, &[]).body, "[]");
        assert_eq!(route(p, "POST", &format!("/revoke?id={}", ids[0]), &bob0, &[]).body, "false");
        let three = token(p, &ana2, &[]);
        let of_ana: Value = serde_json::from_str(&route(p, "GET", "/tokens_of?user=ana", &ana2, &[]).body).unwrap();
        assert_eq!(of_ana.as_array().map(Vec::len), Some(3), "{}", of_ana);
        assert_eq!(route(p, "GET", "/tokens_of?user=bob", &ana2, &[]).body, "[]");
        // Revoked: that one is no good, the other is; revoking it again says there was none. (Two
        // made in the same millisecond are listed by their ids: which of the two it was is seen.)
        assert_eq!(route(p, "POST", &format!("/revoke?id={}", ids[0]), &ana, &[]).body, "true");
        let status = |t: &str| post(p, &As::Token(t.to_string()), &mirrored("tools/list", None), &msg(json!(1), "tools/list", json!({}))).status;
        let (gone, kept) = match (status(&one), status(&two)) {
            (401, 200) => (one.clone(), two.clone()),
            (200, 401) => (two.clone(), one.clone()),
            other => panic!("one revoked, the other good: {:?}", other),
        };
        assert_eq!(status(&gone), 401);
        assert_eq!(call(p, &As::Token(kept.clone()), "whoami", json!({}))["structuredContent"]["user"], json!("ana"));
        assert_eq!(route(p, "POST", &format!("/revoke?id={}", ids[0]), &ana, &[]).body, "false");
        assert_eq!(route(p, "POST", "/revoke?id=", &ana, &[]).body, "false");
        // By the user, from another session.
        let third: Value = serde_json::from_str(&route(p, "GET", "/tokens", &ana2, &[]).body).unwrap();
        let third_id = third.as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap().to_string()).find(|id| !ids.contains(id)).unwrap();
        assert_eq!(route(p, "POST", &format!("/revoke_of?id={}&user=bob", third_id), &ana, &[]).body, "false");
        assert_eq!(route(p, "POST", &format!("/revoke_of?id={}&user=ana", third_id), &ana, &[]).body, "true");
        assert_eq!(post(p, &As::Token(three), &mirrored("tools/list", None), &msg(json!(1), "tools/list", json!({}))).status, 401);
        // `end_session()`: its tokens end with it, and the list is empty.
        let four = token(p, &ana, &[]);
        assert_eq!(route(p, "POST", "/logout", &ana, &[]).status, 204);
        for t in [kept, four] {
            let a = post(p, &As::Token(t), &mirrored("tools/list", None), &msg(json!(1), "tools/list", json!({})));
            assert_eq!((a.status, a.header("www-authenticate")), (401, Some("Bearer error=\"invalid_token\"")));
        }
        assert_eq!(route(p, "GET", "/tokens_of?user=ana", &ana2, &[]).body, "[]");
        // A token for someone the program names otherwise (`{ user }`), on a session with no
        // user, and its `max_age`: one second, and it ends.
        let anon = session_cookie(&support::send(p, "POST", "/anon", &[], ""));
        let a = route(p, "POST", "/token_for?user=zo%C3%AB&age=1s", &anon, &[]);
        assert_eq!(a.status, 200, "{}", a.body);
        let short = As::Token(a.body.clone());
        let r = call(p, &short, "whoami", json!({}));
        assert_eq!(r["structuredContent"]["user"], Value::Null, "{}", r);
        assert_eq!(call(p, &short, "mine", json!({}))["content"][0]["text"], json!("Log in first"));
        let listed: Value = serde_json::from_str(&route(p, "GET", "/tokens_of?user=zo%C3%AB", &anon, &[]).body).unwrap();
        assert_eq!((listed[0]["user"].clone(), listed[0]["expires"].as_i64().unwrap() - listed[0]["made"].as_i64().unwrap()), (json!("zoë"), 1000));
        std::thread::sleep(Duration::from_millis(1300));
        assert_eq!(post(p, &short, &mirrored("tools/list", None), &msg(json!(1), "tools/list", json!({}))).status, 401);
        assert_eq!(route(p, "GET", "/tokens_of?user=zo%C3%AB", &anon, &[]).body, "[]");
        // What `mcp_token` refuses, said (under --watch, on the page).
        let said = |path: &str, cookie: &str| {
            let a = route(p, "POST", path, cookie, &[]);
            assert_eq!(a.status, 500, "{}: {}", path, a.body);
            a.body
        };
        assert!(said("/token", "").contains("`mcp_token`: no one is signed in (`session().user` is null)"));
        assert!(said("/token", &anon).contains("no one is signed in"));
        let bob = login(p, "bob", &[]);
        assert!(said("/token_bad/0", &bob).contains("`mcp_token` has no option `colour`: it takes `user` and `max_age`"));
        assert!(said("/token_bad/1", &bob).contains("`mcp_token`'s max_age is seconds or a duration like \"30d\", got \"0s\""));
        assert!(said("/token_bad/2", &bob).contains("got \"soon\""));
        assert!(said("/token_bad/3", &bob).contains("`mcp_token` expects options as a map, got int"));
        assert!(said("/revoke_bad", &bob).contains("`revoke_mcp_token` takes a token's id, as `mcp_tokens()` gives it, got int"));
        // A tool cannot make one: a token is made where the user signs in.
        let r = call(p, &As::Cookie(bob.clone()), "minting", json!({}));
        assert_eq!(r["isError"], json!(true), "{}", r);
        // Outside a session there are none to list.
        assert_eq!(route(p, "GET", "/tokens", "", &[]).body, "[]");
        drop(s);
        let _ = std::fs::remove_dir_all(&d);
    }

    const LIFE_SRC: &str = r#"
let bad = [{ colour: 1 }, { max_age: "0s" }, { max_age: "soon" }, 5]

route POST /login ? { user: str } = set_session({ user: user })
route POST /anon = set_session({ cart: [1] })
route POST /token = mcp_token()
route POST /token_for ? { user: str, age: str = "30d" } = mcp_token({ user: user, max_age: age })
route POST /token_bad/:how(int) = mcp_token(bad[how])
route GET /tokens = mcp_tokens()
route GET /tokens_of ? { user: str } = mcp_tokens({ user: user })
route POST /revoke ? { id: str } = str(revoke_mcp_token(id))
route POST /revoke_of ? { id: str, user: str } = str(revoke_mcp_token(id, { user: user }))
route POST /revoke_bad = revoke_mcp_token(json.parse("5"))
route POST /logout = end_session()

/// Who is calling.
server fn mcp whoami() -> map with time = { user: session().user, mcp: request.mcp }

/// Needs a login.
fn mcp mine() -> str with time {
    session().user ?? reply 401 "Log in first"
}

/// Sets a cookie.
fn mcp sets() -> str with time {
    set_cookie("seen", "1")
    "set"
}

/// Tries to make a token.
fn mcp minting() -> str with time = mcp_token()
"#;

    #[test]
    fn c3_a_token_kept_in_the_programs_database() {
        tokens_on("sqlite", "db.open(\":memory:\")");
    }

    #[test]
    fn c3_a_token_kept_in_memory_without_a_database() {
        tokens_on("memory", "");
    }

    #[test]
    fn c3_a_token_kept_in_a_sqlite_file_lasts_a_restart_with_quillo_secret() {
        // As a session does: the same key, the same store, the same token.
        let d = dir("c3-restart", &format!("db.open(\"data.db\")\n{}", LIFE_SRC));
        let mut s = support::serve_with(&d.join("main.qwe"), &[]);
        let ana = login(s.port, "ana", &[]);
        let t = As::Token(token(s.port, &ana, &[]));
        assert_eq!(call(s.port, &t, "whoami", json!({}))["structuredContent"]["user"], json!("ana"));
        s.kill();
        let s = support::serve_with(&d.join("main.qwe"), &[]);
        assert_eq!(call(s.port, &t, "whoami", json!({}))["structuredContent"]["user"], json!("ana"));
        // What is kept is not the token: it is in no row.
        drop(s);
        let As::Token(text) = &t else { unreachable!() };
        let files: Vec<PathBuf> = std::fs::read_dir(&d).unwrap().filter_map(|e| e.ok().map(|e| e.path())).filter(|f| f.file_name().is_some_and(|n| n.to_string_lossy().starts_with("data.db"))).collect();
        assert!(!files.is_empty(), "no data.db in {}", d.display());
        for f in files {
            let kept = std::fs::read(&f).unwrap();
            assert!(!kept.windows(20).any(|w| w == &text.as_bytes()[5..25]), "{} holds the token", f.display());
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn c3_a_token_kept_on_postgres() {
        let Some(pg) = support::postgres("c3tokens") else { return };
        tokens_on("postgres", &format!("db.open({:?})", pg.database("c3tokens")));
    }

    #[test]
    fn c3_a_token_kept_on_mariadb() {
        let Some(m) = support::mariadb("c3tokens") else { return };
        tokens_on("mariadb", &format!("db.open({:?})", m.url("c3tokens")));
    }

    #[test]
    fn c3_session_and_tenant_inside_a_tool_and_a_token_only_where_it_was_made() {
        let (d, s) = door("tenant", &[]);
        let p = s.port;
        let acme = [("x-tenant", "acme")];
        let ana = login(p, "ana", &acme);
        let t = As::Token(token(p, &ana, &acme));
        // In its tenant, by cookie and by token: the session's user and the tenant are the tool's.
        let with = |who: &As, tenant: Option<&str>| {
            let mut h = mirrored("tools/call", Some("whoami"));
            if let Some(t) = tenant {
                h.push(("x-tenant".into(), t.into()));
            }
            post(p, who, &h, &msg(json!(1), "tools/call", json!({ "name": "whoami" })))
        };
        for who in [As::Cookie(ana.clone()), t.clone()] {
            let a = with(&who, Some("acme"));
            assert_eq!(a.status, 200, "{}", a.body);
            assert_eq!(json_of(&a)["result"]["structuredContent"], json!({ "user": "ana", "mcp": { "client": "quillo-test", "version": "1.2.3" }, "tenant": "acme" }));
        }
        // Another tenant, or none: neither the cookie nor the token is good there.
        for tenant in [Some("globex"), None, Some("Acme")] {
            assert_eq!(with(&As::Cookie(ana.clone()), tenant).status, 401, "{:?}", tenant);
            let a = with(&t, tenant);
            assert_eq!((a.status, a.header("www-authenticate")), (401, Some("Bearer error=\"invalid_token\"")), "{:?}", tenant);
        }
        // Nor are its tokens listed there.
        let listed = |tenant: &[(&str, &str)]| route(p, "GET", "/tokens_of?user=ana", &ana, tenant).body;
        assert_eq!(serde_json::from_str::<Value>(&listed(&acme)).unwrap().as_array().map(Vec::len), Some(1));
        assert_eq!(listed(&[("x-tenant", "globex")]), "[]");
        drop(s);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn c3_request_mcp_is_the_clients_and_null_elsewhere() {
        let (d, s) = door("request-mcp", &[]);
        let p = s.port;
        let ana = login(p, "ana", &[]);
        let who = As::Cookie(ana.clone());
        let as_said = |info: Option<Value>| {
            let mut m = json!({ "io.modelcontextprotocol/protocolVersion": V, "io.modelcontextprotocol/clientCapabilities": {} });
            if let Some(i) = info {
                m["io.modelcontextprotocol/clientInfo"] = i;
            }
            let body = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "whoami", "_meta": m } }).to_string();
            let a = post(p, &who, &mirrored("tools/call", Some("whoami")), &body);
            assert_eq!(a.status, 200, "{}", a.body);
            json_of(&a)["result"]["structuredContent"]["mcp"].clone()
        };
        assert_eq!(as_said(Some(json!({ "name": "Claude Code", "version": "2.1.319" }))), json!({ "client": "Claude Code", "version": "2.1.319" }));
        assert_eq!(as_said(Some(json!({ "name": "日本 ✓", "version": "" }))), json!({ "client": "日本 ✓", "version": "" }));
        // Said as it was or not at all: null where it is not text.
        assert_eq!(as_said(None), json!({ "client": null, "version": null }));
        assert_eq!(as_said(Some(json!({ "name": 5, "version": ["1"] }))), json!({ "client": null, "version": null }));
        assert_eq!(as_said(Some(json!("x"))), json!({ "client": null, "version": null }));
        // The same function at /_quillo/rpc, and a route: null.
        let rpc = support::send(p, "POST", "/_quillo/rpc/whoami", &[("Cookie", &ana)], "[]");
        assert_eq!(rpc.status, 200, "{}", rpc.body);
        assert_eq!(serde_json::from_str::<Value>(&rpc.body).unwrap()["ok"], json!({ "user": "ana", "mcp": null, "tenant": null }));
        assert_eq!(support::send(p, "GET", "/here", &[], "").body, "null");
        drop(s);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a2_a_server_fn_mcp_is_called_at_both_doors() {
        let (d, s) = door("both", &[]);
        let p = s.port;
        let ana = login(p, "ana", &[]);
        let who = As::Cookie(ana.clone());
        for v in [json!(1), json!("é ✓"), json!([1, { "a": null }]), json!(null)] {
            let rpc = support::send(p, "POST", "/_quillo/rpc/echo", &[("Cookie", &ana)], &json!([v]).to_string());
            assert_eq!(serde_json::from_str::<Value>(&rpc.body).unwrap()["ok"], v, "{}", rpc.body);
            let r = call(p, &who, "echo", json!({ "v": v }));
            assert_eq!(r["content"][0]["text"].as_str().map(str::to_string), Some(if v.is_string() { v.as_str().unwrap().to_string() } else { v.to_string() }), "{}", r);
        }
        // A plain `fn mcp` is the door's alone; a plain `server fn` the page's alone.
        assert_eq!(support::send(p, "POST", "/_quillo/rpc/add", &[("Cookie", &ana)], "[\"x\"]").status, 404);
        let (status, _, code, _) = refusal(&ask(p, &who, json!(1), "tools/call", json!({ "name": "plain_one" })));
        assert_eq!((status, code), (400, -32602));
        assert_eq!(support::send(p, "POST", "/_quillo/rpc/plain_one", &[], "[]").body, r#"{"ok":1}"#);
        drop(s);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn c2_another_sites_page_cannot_drive_it() {
        let (d, s) = door("cross-site", &[]);
        let p = s.port;
        let ana = login(p, "ana", &[]);
        let who = As::Cookie(ana.clone());
        let add = msg(json!(1), "tools/call", json!({ "name": "add", "arguments": { "text": "x" } }));
        let count = || support::send(p, "GET", "/count", &[], "").body;
        assert_eq!(count(), "0");
        let with = |extra: &[(&str, &str)]| {
            let mut h = mirrored("tools/call", Some("add"));
            h.extend(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())));
            post(p, &who, &h, &add)
        };
        // Another site's `Origin`, or the browser saying so: 403, and nothing ran.
        for extra in [&[("Origin", "https://evil.example")][..], &[("Origin", "http://localhost.evil.example")], &[("Origin", "null")], &[("Sec-Fetch-Site", "cross-site")], &[("Sec-Fetch-Site", "same-site")], &[("Sec-Fetch-Site", "cross-site"), ("Origin", "http://localhost")]] {
            let a = with(extra);
            assert_eq!(a.status, 403, "{:?}: {}", extra, a.body);
            grants_nothing(&a);
        }
        assert_eq!(count(), "0");
        // The preflight a page on another site has to make for these headers: refused, and nothing
        // in it lets the browser go on. The same without an Origin (a client that is no browser):
        // what the path takes, and still no grant.
        let preflight = [("Origin", "https://evil.example"), ("Access-Control-Request-Method", "POST"), ("Access-Control-Request-Headers", "content-type, mcp-protocol-version, mcp-method, mcp-name, authorization")];
        let a = support::send(p, "OPTIONS", "/_quillo/mcp", &preflight, "");
        assert_eq!(a.status, 403, "{}", a.body);
        grants_nothing(&a);
        let a = support::send(p, "OPTIONS", "/_quillo/mcp", &preflight[1..], "");
        assert_eq!((a.status, a.header("allow")), (204, Some("POST")));
        grants_nothing(&a);
        // What such a page can send without a preflight, a form or plain text with the cookie and
        // none of the headers, runs nothing: 415; JSON without the headers, 400 HeaderMismatch.
        for ctype in ["text/plain", "application/x-www-form-urlencoded", "multipart/form-data; boundary=x"] {
            let a = support::send(p, "POST", "/_quillo/mcp", &[("Cookie", &ana), ("Content-Type", ctype)], &add);
            assert_eq!(a.status, 415, "{}", ctype);
            grants_nothing(&a);
        }
        let a = support::send(p, "POST", "/_quillo/mcp", &[("Cookie", &ana)], &add);
        assert_eq!(refusal(&a).2, -32020);
        assert_eq!(count(), "0");
        // The program's own page, and a client that is no browser: let in.
        for extra in [&[("Origin", "http://localhost")][..], &[("Sec-Fetch-Site", "same-origin")], &[("Sec-Fetch-Site", "none")], &[]] {
            let a = with(extra);
            assert_eq!(a.status, 200, "{:?}: {}", extra, a.body);
            grants_nothing(&a);
        }
        assert_eq!(count(), "4");
        drop(s);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn c2_under_base_and_not_as_a_service_nor_without_tools() {
        // `--base /shop`: the proxy in front takes `/shop` off, so the door is at `/_quillo/mcp`
        // here; the session's cookie is the base's (`quillo_session`, `Path=/shop`), and a token is
        // tied to it.
        let (d, s) = door("base", &["--base", "/shop"]);
        let p = s.port;
        let a = support::send(p, "POST", "/login?user=ana", &[], "");
        let line = a.headers.iter().find(|(k, v)| k == "set-cookie" && v.starts_with("quillo_session=")).map(|(_, v)| v.clone()).unwrap_or_else(|| panic!("{:?}", a.headers));
        assert!(line.contains("Path=/shop"), "{}", line);
        let ana = session_cookie(&a);
        assert_eq!(call(p, &As::Cookie(ana.clone()), "whoami", json!({}))["structuredContent"]["user"], json!("ana"));
        let t = As::Token(token(p, &ana, &[]));
        assert_eq!(call(p, &t, "whoami", json!({}))["structuredContent"]["user"], json!("ana"));
        // The prefixed path is the proxy's, as for every path under --base.
        assert_eq!(support::send(p, "POST", "/shop/_quillo/mcp", &[("Cookie", &ana)], "{}").status, 404);
        // A root server's cookie, signed with the same key, is no session under the base.
        let (d2, root) = door("base-root", &[]);
        let root_cookie = login(root.port, "ana", &[]);
        let value = root_cookie.split_once('=').unwrap().1;
        let a = post(p, &As::Cookie(format!("quillo_session={}", value)), &mirrored("tools/list", None), &msg(json!(1), "tools/list", json!({})));
        assert_eq!(a.status, 401);
        drop((s, root));
        // `--as-service`: a service's tools are its app's; there is no door, as there is no rpc.
        let s = support::serve_with(&d.join("main.qwe"), &["--as-service"]);
        let a = post(s.port, &As::Nobody, &mirrored("tools/list", None), &msg(json!(1), "tools/list", json!({})));
        assert_eq!(a.status, 404, "{}", a.body);
        assert_eq!(support::send(s.port, "GET", "/_quillo/mcp", &[], "").status, 404);
        drop(s);
        // A program with no tools has no door either; its server fns are as they were.
        let plain = dir("c2-no-tools", "server fn one() -> int = 1\nroute GET / = \"hi\"\n");
        let s = support::serve_with(&plain.join("main.qwe"), &[]);
        assert_eq!(post(s.port, &As::Nobody, &mirrored("tools/list", None), &msg(json!(1), "tools/list", json!({}))).status, 404);
        assert_eq!(support::send(s.port, "POST", "/_quillo/rpc/one", &[], "[]").body, r#"{"ok":1}"#);
        drop(s);
        // Under --no-check the tools are read for the door all the same.
        let s = support::serve_unchecked(&d.join("main.qwe"));
        let ana = login(s.port, "ana", &[]);
        let a = ask(s.port, &As::Cookie(ana), json!(1), "tools/list", json!({}));
        assert_eq!(json_of(&a)["result"]["tools"].as_array().map(Vec::len), Some(6), "{}", a.body);
        drop(s);
        for x in [d, d2, plain] {
            let _ = std::fs::remove_dir_all(&x);
        }
    }

    /// `n` tool calls by `who`: how many were answered, and the last refusal's `Retry-After`.
    fn calls(p: u16, who: &As, n: usize) -> (usize, Option<u64>) {
        let (mut ok, mut wait) = (0, None);
        for i in 0..n {
            let a = ask(p, who, json!(i), "tools/call", json!({ "name": "echo", "arguments": { "v": i } }));
            match a.status {
                200 => ok += 1,
                429 => {
                    assert!(a.body.contains("too many tool calls: at most"), "{}", a.body);
                    wait = a.header("retry-after").and_then(|w| w.parse().ok());
                }
                other => panic!("{}: {}", other, a.body),
            }
        }
        (ok, wait)
    }

    #[test]
    fn c4_tool_calls_are_limited_per_caller_with_retry_after() {
        // The default: 60 a minute, the 61st refused, `Retry-After` the seconds until one leaves
        // the minute.
        let (d, s) = door("limit", &[]);
        let p = s.port;
        let ana = login(p, "ana", &[]);
        let cookie = As::Cookie(ana.clone());
        let (ok, wait) = calls(p, &cookie, 61);
        assert_eq!(ok, 60);
        assert!(wait.is_some_and(|w| (1..=60).contains(&w)), "{:?}", wait);
        // Lists are no calls. A token of the same user, or a new sign-in, shares her minute (decided
        // in review: it counted by the token and the session, each a fresh 60); another user has
        // her own.
        for i in 0..5 {
            assert_eq!(ask(p, &cookie, json!(i), "tools/list", json!({})).status, 200);
        }
        let t = As::Token(token(p, &ana, &[]));
        assert_eq!(calls(p, &t, 3).0, 0);
        assert_eq!(calls(p, &As::Cookie(login(p, "ana", &[])), 3).0, 0);
        let bob = login(p, "bob", &[]);
        assert_eq!(calls(p, &As::Cookie(bob.clone()), 3), (3, None));
        assert_eq!(calls(p, &As::Token(token(p, &bob, &[])), 3), (3, None));
        // And the same user in another tenant is another user.
        let acme = login(p, "ana", &[("x-tenant", "acme")]);
        let mut h = mirrored("tools/call", Some("echo"));
        h.push(("x-tenant".into(), "acme".into()));
        let a = post(p, &As::Cookie(acme), &h, &msg(json!(1), "tools/call", json!({ "name": "echo", "arguments": { "v": 1 } })));
        assert_eq!(a.status, 200, "{}", a.body);
        drop(s);
        // `--max-tool-calls`: the limit and one either side.
        let s = support::serve_with(&d.join("main.qwe"), &["--max-tool-calls", "3"]);
        let cookie = As::Cookie(login(s.port, "ana", &[]));
        let (ok, wait) = calls(s.port, &cookie, 4);
        assert_eq!(ok, 3);
        assert!(wait.is_some_and(|w| (59..=60).contains(&w)), "{:?}", wait);
        // A call refused is not counted, and the next is refused too.
        let (ok, again) = calls(s.port, &cookie, 1);
        assert_eq!(ok, 0);
        assert!(again.is_some_and(|w| w <= wait.unwrap()), "{:?} after {:?}", again, wait);
        drop(s);
        let s = support::serve_with(&d.join("main.qwe"), &["--max-tool-calls", "1"]);
        let cookie = As::Cookie(login(s.port, "ana", &[]));
        assert_eq!(calls(s.port, &cookie, 2).0, 1);
        drop(s);
        // A policy narrows it, never widens.
        let limits = d.join("limits.toml");
        let limits = limits.to_str().unwrap();
        std::fs::write(limits, "[limits]\nmax_tool_calls = 2\n").unwrap();
        let s = support::serve_with(&d.join("main.qwe"), &["--max-tool-calls", "5", "--policy", limits]);
        let cookie = As::Cookie(login(s.port, "ana", &[]));
        assert_eq!(calls(s.port, &cookie, 4).0, 2);
        drop(s);
        std::fs::write(limits, "[limits]\nmax_tool_calls = 200\n").unwrap();
        let s = support::serve_with(&d.join("main.qwe"), &["--max-tool-calls", "2", "--policy", limits]);
        let cookie = As::Cookie(login(s.port, "ana", &[]));
        assert_eq!(calls(s.port, &cookie, 3).0, 2);
        drop(s);
        // What is no limit is said.
        for v in ["0", "-1", "x", ""] {
            let (ok, _, err) = quillo(&d, &["run", "main.qwe", "--max-tool-calls", v]);
            assert!(!ok && err.contains("--max-tool-calls needs a positive number"), "{:?}: {}", v, err);
        }
        for v in ["0", "-3", "\"60\""] {
            std::fs::write(d.join("bad.toml"), format!("[limits]\nmax_tool_calls = {}\n", v)).unwrap();
            let (ok, _, err) = quillo(&d, &["run", "main.qwe", "--policy", "bad.toml"]);
            assert!(!ok && err.contains("max_tool_calls"), "{}: {}", v, err);
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn c4_max_compute_holds_a_tool_call() {
        let (d, s) = door("compute", &["--max-compute", "300ms"]);
        let p = s.port;
        let who = As::Cookie(login(p, "ana", &[]));
        let began = Instant::now();
        let r = call(p, &who, "spin", json!({}));
        assert_eq!((r["isError"].clone(), r["content"][0]["text"].clone()), (json!(true), json!("internal error")), "{}", r);
        assert!(began.elapsed() < Duration::from_secs(10), "{:?}", began.elapsed());
        // The door goes on.
        assert_eq!(call(p, &who, "whoami", json!({}))["structuredContent"]["user"], json!("ana"));
        drop(s);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// `quillo run` with `args` in `d`, its stderr read as it comes: the server, its port, and
    /// what it has said so far.
    fn watched(d: &Path, args: &[&str]) -> (support::Guard, u16, Arc<Mutex<String>>) {
        let (guard, said) = logged(d, &[&["--port", "0"][..], args].concat());
        let deadline = Instant::now() + WAIT;
        let port = loop {
            let text = said.lock().unwrap().clone();
            if let Some(p) = text.split("listening on http://127.0.0.1:").nth(1).and_then(|r| r.split(|c: char| !c.is_ascii_digit()).next()).and_then(|p| p.parse().ok()) {
                break p;
            }
            assert!(Instant::now() < deadline, "did not start: {}", text);
            std::thread::sleep(Duration::from_millis(20));
        };
        while !support::listening(port) {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        (guard, port, said)
    }

    /// `quillo run main.qwe` with `args` in `d`, its stderr read as it comes.
    fn logged(d: &Path, args: &[&str]) -> (support::Guard, Arc<Mutex<String>>) {
        logged_env(d, args, &[])
    }

    fn logged_env(d: &Path, args: &[&str], env: &[(&str, &str)]) -> (support::Guard, Arc<Mutex<String>>) {
        let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
            .args(["run", "main.qwe", "--workers", "8"])
            .args(args)
            .envs(env.iter().copied())
            .current_dir(d)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let err = child.stderr.take().unwrap();
        let said = Arc::new(Mutex::new(String::new()));
        let s = said.clone();
        std::thread::spawn(move || {
            for l in std::io::BufReader::new(err).lines().map_while(Result::ok) {
                let mut s = s.lock().unwrap();
                s.push_str(&l);
                s.push('\n');
            }
        });
        (support::Guard::new(child), said)
    }

    /// The token the door's line names, once it is said.
    fn printed_token(said: &Arc<Mutex<String>>) -> (String, String) {
        let deadline = Instant::now() + WAIT;
        loop {
            let text = said.lock().unwrap().clone();
            if let Some(line) = text.lines().find(|l| l.starts_with("quillo: MCP door for AI clients at ")) {
                let url = line.split(" at ").nth(1).unwrap().split(',').next().unwrap().to_string();
                let token = line.split("Bearer ").nth(1).unwrap().split(' ').next().unwrap().to_string();
                assert!(text.contains(&format!("claude mcp add --transport http main {} --header \"Authorization: Bearer {}\"", url, token)), "{}", text);
                return (url, token);
            }
            assert!(Instant::now() < deadline, "no token said: {}", text);
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn c3_under_watch_on_127_0_0_1_a_token_is_printed() {
        let d = dir("c3-watch", SRC);
        let (g, port, said) = watched(&d, &["--watch"]);
        let (url, t) = printed_token(&said);
        assert_eq!(url, format!("http://127.0.0.1:{}/_quillo/mcp", port));
        // It is a caller with no session: the program's own checks hold for it.
        let dev = As::Token(t.clone());
        assert_eq!(call(port, &dev, "whoami", json!({}))["structuredContent"]["user"], Value::Null);
        assert_eq!(call(port, &dev, "mine", json!({}))["content"][0]["text"], json!("Log in first"));
        assert_eq!(said.lock().unwrap().matches("quillo: MCP door for AI clients").count(), 1);
        // Good while this runs, here alone: another server does not take it.
        let d2 = dir("c3-watch-other", SRC);
        let (g2, port2, said2) = watched(&d2, &["--watch", "--base", "/shop"]);
        let (url2, t2) = printed_token(&said2);
        // Under --base, the address this server answers at (its proxy takes /shop off), called.
        assert_eq!(url2, format!("http://127.0.0.1:{}/_quillo/mcp", port2));
        let path = url2.split_once(&format!("{}", port2)).unwrap().1;
        let refs = mirrored("tools/call", Some("whoami"));
        let mut h: Vec<(&str, &str)> = refs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        let bearer = format!("Bearer {}", t2);
        h.push(("Authorization", &bearer));
        let a = support::send(port2, "POST", path, &h, &msg(json!(1), "tools/call", json!({ "name": "whoami" })));
        assert_eq!(a.status, 200, "{}: {}", path, a.body);
        assert_ne!(t, t2);
        assert_eq!(post(port2, &dev, &mirrored("tools/list", None), &msg(json!(1), "tools/list", json!({}))).status, 401);
        drop((g, g2));
        // Not without --watch; nor with --watch for a program with no tools, until it has one.
        let (g, _, said) = watched(&d, &[]);
        std::thread::sleep(Duration::from_millis(300));
        assert!(!said.lock().unwrap().contains("MCP door"), "{}", said.lock().unwrap());
        drop(g);
        let routes = "route POST /login ? { user: str } = set_session({ user: user })\nroute POST /token = mcp_token()\n";
        let none = dir("c3-watch-none", routes);
        let (g, port, said) = watched(&none, &["--watch"]);
        std::thread::sleep(Duration::from_millis(300));
        assert!(!said.lock().unwrap().contains("MCP door"));
        // No tools, no door to make a token for.
        let ana = login(port, "ana", &[]);
        let a = route(port, "POST", "/token", &ana, &[]);
        assert!(a.status == 500 && a.body.contains("`mcp_token`: this program has no tools (`fn mcp name(...)`)"), "{}", a.body);
        std::fs::write(none.join("main.qwe"), format!("{}\n/// Seven.\nfn mcp seven() -> int with time = 7\n", routes)).unwrap();
        let (url, t) = printed_token(&said);
        assert_eq!(url, format!("http://127.0.0.1:{}/_quillo/mcp", port));
        assert_eq!(call(port, &As::Token(t), "seven", json!({}))["structuredContent"], json!({ "result": 7 }));
        // The reloaded program has its door: a token is made, and is good.
        let made = token(port, &ana, &[]);
        assert_eq!(call(port, &As::Token(made), "seven", json!({}))["structuredContent"], json!({ "result": 7 }));
        drop(g);
        // Outside a request (the top level), there is no session to stand for.
        std::fs::write(none.join("top.qwe"), "/// Seven.\nfn mcp seven() -> int with time = 7\nmcp_token()\n").unwrap();
        let (ok, _, err) = quillo(&none, &["run", "top.qwe", "--port", "0"]);
        assert!(!ok && err.contains("`mcp_token` is made while a request is answered"), "{}", err);
        for x in [d, d2, none] {
            let _ = std::fs::remove_dir_all(&x);
        }
    }

    /// The median of `f`'s time over `n` runs, in microseconds.
    fn median_us(n: usize, mut f: impl FnMut()) -> f64 {
        let mut t: Vec<f64> = (0..n)
            .map(|_| {
                let at = Instant::now();
                f();
                at.elapsed().as_secs_f64() * 1e6
            })
            .collect();
        t.sort_by(|a, b| a.partial_cmp(b).unwrap());
        t[n / 2]
    }

    #[test]
    fn c2_a_tool_call_costs_about_what_an_rpc_call_costs() {
        let (d, s) = door("cost", &["--max-tool-calls", "100000"]);
        let p = s.port;
        let ana = login(p, "ana", &[]);
        let (cookie, t) = (As::Cookie(ana.clone()), As::Token(token(p, &ana, &[])));
        let body = json!([{ "a": [1, 2, 3], "b": "é ✓" }]).to_string();
        let rpc = || assert_eq!(support::send(p, "POST", "/_quillo/rpc/echo", &[("Cookie", &ana)], &body).status, 200);
        let tool = |who: &As| assert_eq!(ask(p, who, json!(1), "tools/call", json!({ "name": "echo", "arguments": { "v": { "a": [1, 2, 3], "b": "é ✓" } } })).status, 200);
        // Warm, then in turns, so that the machine's load falls on each alike.
        for _ in 0..20 {
            rpc();
            tool(&cookie);
            tool(&t);
        }
        let (mut r, mut c, mut k) = (Vec::new(), Vec::new(), Vec::new());
        for _ in 0..5 {
            r.push(median_us(60, rpc));
            c.push(median_us(60, || tool(&cookie)));
            k.push(median_us(60, || tool(&t)));
        }
        let mid = |v: &mut Vec<f64>| {
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            v[v.len() / 2]
        };
        let (r, c, k) = (mid(&mut r), mid(&mut c), mid(&mut k));
        eprintln!("a call of `echo`, each a new connection, median of medians: /_quillo/rpc {:.0} µs, /_quillo/mcp with the cookie {:.0} µs, with a token {:.0} µs", r, c, k);
        assert!(c < r * 2.0 + 1000.0 && k < r * 2.0 + 1000.0, "rpc {:.0} µs, door {:.0} µs (cookie) and {:.0} µs (token)", r, c, k);
        drop(s);
        let _ = std::fs::remove_dir_all(&d);
    }

    // ---- Slice 5, review (FEEDBACK56.md, Q-859 and Q-860, found in review; branch feedback56) ----

    /// A program whose own routes can write the table of tokens, as SQL injection elsewhere, a
    /// shared database role or a restored backup could; and say whether it is there.
    const REVIEW_SRC: &str = r#"
route POST /login ? { user: str } = set_session({ user: user })
route POST /login_for ? { user: str, age: str } = set_session({ user: user }, { max_age: age })
route POST /login_and_token ? { user: str, age: str } {
    set_session({ user: user }, { max_age: age })
    mcp_token()
}
route POST /token = mcp_token()
route POST /token_for ? { user: str, age: str = "30d" } = mcp_token({ user: user, max_age: age })
route GET /tokens = mcp_tokens()
route GET /tokens_of ? { user: str } = mcp_tokens({ user: user })
route POST /revoke ? { id: str } = str(revoke_mcp_token(id))
route POST /revoke_of ? { id: str, user: str } = str(revoke_mcp_token(id, { user: user }))
route POST /logout = end_session()
route POST /forget = set_session(null)
route POST /tamper ? { id: str, column: str, value: str } = str(db.exec(`update _quillo_mcp_tokens set ${column} = ? where id = ?`, value, id).changes)

/// Who is calling.
server fn mcp whoami() -> map with time = { user: session().user, tenant: tenant() }
"#;

    fn review_dir(name: &str, db_line: &str, more: &str) -> PathBuf {
        dir(&format!("c3-review-{}", name), &format!("{}\ntenant_from((request) => request.headers[\"x-tenant\"])\n{}{}", db_line, REVIEW_SRC, more))
    }

    /// `s` percent-encoded for a query string.
    fn enc(s: &str) -> String {
        s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{:02X}", b) }).collect()
    }

    fn listed(p: u16, path: &str, cookie: &str) -> Vec<Value> {
        let a = route(p, "GET", path, cookie, &[]);
        assert_eq!(a.status, 200, "{}", a.body);
        serde_json::from_str::<Value>(&a.body).unwrap().as_array().unwrap().clone()
    }

    fn list_status(p: u16, who: &As) -> u16 {
        post(p, who, &mirrored("tools/list", None), &msg(json!(1), "tools/list", json!({}))).status
    }

    #[test]
    fn c3_review_a_tokens_row_is_sealed_as_a_cookie_is() {
        let d = review_dir("sealed", "db.open(\":memory:\")", "");
        let s = support::serve_with(&d.join("main.qwe"), &[]);
        let p = s.port;
        let ana = login(p, "ana", &[]);
        let bob = login(p, "bob", &[]);
        let bob_t = token(p, &bob, &[]);
        let bob_row = listed(p, "/tokens", &bob)[0].clone();
        // Each thing a row decides, changed by whoever can write the table: the token is no good,
        // and is listed nowhere. Without the seal, the first made Ana's token Bob's admin.
        for (column, value) in [
            ("session", r#"{"user":"admin"}"#.to_string()),
            ("who", "\"admin\"".to_string()),
            ("expires", "99999999999999".to_string()),
            ("made", "1".to_string()),
            ("place", "[\"\",\"acme\"]".to_string()),
            ("owner", "id 00000000000000000000000000000000".to_string()),
            ("id", bob_row["id"].as_str().unwrap().to_string()),
            ("seal", "x".to_string()),
        ] {
            let t = token(p, &ana, &[]);
            let id = listed(p, "/tokens", &ana).last().unwrap()["id"].as_str().unwrap().to_string();
            assert_eq!(call(p, &As::Token(t.clone()), "whoami", json!({}))["structuredContent"]["user"], json!("ana"));
            let changed = route(p, "POST", &format!("/tamper?id={}&column={}&value={}", id, column, enc(&value)), &ana, &[]);
            assert_eq!(changed.body, "1", "{}: {}", column, changed.body);
            // Called where the row now says it is good, so that the seal alone refuses it (found
            // in the second review: from no tenant, the place refused it first).
            let mut h = mirrored("tools/call", Some("whoami"));
            if column == "place" {
                h.push(("x-tenant".into(), "acme".into()));
            }
            let a = post(p, &As::Token(t), &h, &msg(json!(1), "tools/call", json!({ "name": "whoami" })));
            assert_eq!((a.status, a.header("www-authenticate")), (401, Some("Bearer error=\"invalid_token\"")), "{}: {}", column, a.body);
            assert!(!route(p, "GET", "/tokens_of?user=admin", &ana, &[]).body.contains(&id), "{}", column);
            assert!(!route(p, "GET", "/tokens", &ana, &[]).body.contains(&id), "{}", column);
        }
        // A row left as it was is good, Bob's among them.
        assert_eq!(call(p, &As::Token(bob_t), "whoami", json!({}))["structuredContent"]["user"], json!("bob"));
        drop(s);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Whether the table of tokens is there, by a route that asks SQLite.
    const TABLE_ROUTE: &str = "route GET /table = str(len(db.query(\"select name from sqlite_master where name = '_quillo_mcp_tokens'\")))\n";

    #[test]
    fn c3_review_logout_and_strangers_never_make_the_table() {
        let d = review_dir("no-table", "db.open(\"data.db\")", TABLE_ROUTE);
        let (g, p, said) = watched(&d, &[]);
        let table = || support::send(p, "GET", "/table", &[], "").body;
        let ana = login(p, "ana", &[]);
        // A stranger's token, again and again: 401, nothing made, nothing logged but the request.
        for _ in 0..3 {
            assert_eq!(list_status(p, &As::Token("qmcp_strangerstrangerstrangerstrangerstrange".into())), 401);
        }
        assert_eq!(table(), "0");
        // Listing, revoking, logging out, a session set to null: no tokens, nothing made.
        assert_eq!(route(p, "GET", "/tokens", &ana, &[]).body, "[]");
        assert_eq!(route(p, "GET", "/tokens_of?user=ana", &ana, &[]).body, "[]");
        assert_eq!(route(p, "POST", "/revoke?id=x", &ana, &[]).body, "false");
        assert_eq!(route(p, "POST", "/logout", &ana, &[]).status, 204);
        let bob = login(p, "bob", &[]);
        assert_eq!(route(p, "POST", "/forget", &bob, &[]).status, 204);
        assert_eq!(table(), "0");
        // `mcp_token()` makes it, once.
        let ana = login(p, "ana", &[]);
        let t = As::Token(token(p, &ana, &[]));
        assert_eq!(table(), "1");
        assert_eq!(list_status(p, &t), 200);
        let text = said.lock().unwrap().clone();
        assert!(!text.contains("cannot read its tokens") && !text.contains("error"), "{}", text);
        drop(g);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn c3_review_a_database_that_cannot_make_the_table_fails_mcp_token_alone() {
        // A SQLite database that takes no writes: `mcp_token()` says why, and a logout and a
        // stranger's token are as without the table.
        let d = review_dir("read-only", "db.open(\"ro.db\")\ndb.exec(\"pragma query_only = 1\")", "");
        let (g, p, said) = watched(&d, &["--watch"]);
        let ana = login(p, "ana", &[]);
        let a = route(p, "POST", "/token", &ana, &[]);
        assert_eq!(a.status, 500, "{}", a.body);
        assert!(a.body.contains("`mcp_token`: the token could not be kept: its table, _quillo_mcp_tokens, could not be made") && a.body.contains("needs the right to create a table there"), "{}", a.body);
        assert_eq!(list_status(p, &As::Token("qmcp_x".into())), 401);
        assert_eq!(route(p, "POST", "/logout", &ana, &[]).status, 204);
        assert!(!said.lock().unwrap().contains("cannot read its tokens"));
        drop(g);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn c3_review_on_postgres_a_logout_in_a_transaction_and_a_role_that_cannot_create() {
        let Some(pg) = support::postgres("c3review") else { return };
        let url = pg.database("c3review");
        // A logout inside the program's transaction, with no table of tokens: the transaction goes
        // on and commits (an error would have aborted it on PostgreSQL).
        let more = "db.exec(\"create table if not exists notes (text text)\")\n\
                    route POST /logout_in_tx = db.transaction(() => {\n    end_session()\n    db.exec(\"insert into notes (text) values ('kept')\")\n})\n\
                    route GET /notes = str(db.one(\"select count(*) as n from notes\").n)\n\
                    route GET /table = str(db.one(\"select count(*) as n from information_schema.tables where table_name = '_quillo_mcp_tokens'\").n)\n\
                    route POST /role = db.exec(\"create role reader login\")\n";
        let d = review_dir("pg", &format!("db.open({:?})", url), more);
        let s = support::serve_with(&d.join("main.qwe"), &[]);
        let p = s.port;
        let ana = login(p, "ana", &[]);
        let a = route(p, "POST", "/logout_in_tx", &ana, &[]);
        assert_eq!(a.status, 200, "{}", a.body);
        assert_eq!(support::send(p, "GET", "/notes", &[], "").body, "1");
        assert_eq!(support::send(p, "GET", "/table", &[], "").body, "0");
        // With the table there, inside a transaction too: the tokens end.
        let ana = login(p, "ana", &[]);
        let t = As::Token(token(p, &ana, &[]));
        assert_eq!(route(p, "POST", "/logout_in_tx", &ana, &[]).status, 200);
        assert_eq!(list_status(p, &t), 401);
        assert_eq!(support::send(p, "GET", "/notes", &[], "").body, "2");
        assert_eq!(route(p, "POST", "/role", "", &[]).status, 200);
        drop(s);
        // A role that may not create a table in the schema: `mcp_token()` fails, saying so; a
        // stranger's token is 401, with no line in the log, and a logout works.
        let reader = pg.database("c3reader").replace("quillo@", "reader@");
        let d2 = review_dir("pg-reader", &format!("db.open({:?})", reader), "");
        let (g, p, said) = watched(&d2, &["--watch"]);
        let ana = login(p, "ana", &[]);
        let a = route(p, "POST", "/token", &ana, &[]);
        assert_eq!(a.status, 500, "{}", a.body);
        assert!(a.body.contains("its table, _quillo_mcp_tokens, could not be made") && a.body.contains("permission denied"), "{}", a.body);
        for _ in 0..3 {
            assert_eq!(list_status(p, &As::Token("qmcp_x".into())), 401);
        }
        assert_eq!(route(p, "POST", "/logout", &ana, &[]).status, 204);
        assert!(!said.lock().unwrap().contains("cannot read its tokens"), "{}", said.lock().unwrap());
        drop(g);
        for x in [d, d2] {
            let _ = std::fs::remove_dir_all(&x);
        }
    }

    #[test]
    fn c3_review_end_session_ends_tokens_while_the_door_is_closed() {
        // Under --watch a version without tools has no door; a logout there still ends the
        // session's tokens, and they stay ended when the tools come back.
        let with_tools = format!("db.open(\"data.db\")\n{}", LIFE_SRC);
        let d = dir("c3-review-closed", &with_tools);
        let (g, p, said) = watched(&d, &["--watch"]);
        let reloads = |n: usize| {
            let deadline = Instant::now() + WAIT;
            while said.lock().unwrap().matches("quillo: reloaded").count() < n {
                assert!(Instant::now() < deadline, "no reload: {}", said.lock().unwrap());
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        let ana = login(p, "ana", &[]);
        let t = As::Token(token(p, &ana, &[]));
        assert_eq!(list_status(p, &t), 200);
        let without: String = with_tools.lines().filter(|l| !l.contains("fn mcp") && !l.starts_with("    ") && !l.starts_with('}') && !l.starts_with("///")).map(|l| format!("{}\n", l)).collect();
        std::fs::write(d.join("main.qwe"), &without).unwrap();
        reloads(1);
        assert_eq!(list_status(p, &t), 404);
        assert_eq!(route(p, "POST", "/logout", &ana, &[]).status, 204);
        std::fs::write(d.join("main.qwe"), &with_tools).unwrap();
        reloads(2);
        assert_eq!(list_status(p, &t), 401);
        drop(g);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn c3_review_a_token_lasts_as_its_session_and_is_found_after_the_cookie() {
        let d = review_dir("life", "db.open(\":memory:\")", "");
        let s = support::serve_with(&d.join("main.qwe"), &[]);
        let p = s.port;
        let life = |cookie: &str| {
            let t = listed(p, "/tokens", cookie);
            let last = t.last().unwrap();
            last["expires"].as_i64().unwrap() - last["made"].as_i64().unwrap()
        };
        let near = |got: i64, want: i64| assert!((want - 5_000..=want).contains(&got), "{} ms, not about {} ms", got, want);
        // A session of the default 30 days: a token of 30 days from now.
        let ana = login(p, "ana", &[]);
        token(p, &ana, &[]);
        assert_eq!(life(&ana), 30 * 86_400 * 1000);
        // A session the program gave a `max_age`: as long as it has left, from its cookie or set
        // in the same request; `mcp_token`'s own `max_age` wins; at most 366 days.
        let bob = session_cookie(&support::send(p, "POST", "/login_for?user=bob&age=1h", &[], ""));
        token(p, &bob, &[]);
        near(life(&bob), 3_600_000);
        let a = support::send(p, "POST", "/login_and_token?user=cy&age=10m", &[], "");
        assert_eq!(a.status, 200, "{}", a.body);
        near(life(&session_cookie(&a)), 600_000);
        assert_eq!(route(p, "POST", "/token_for?user=bob&age=2h", &bob, &[]).status, 200);
        // (Two made in the same millisecond are listed by their ids: the new one is looked for.)
        let lives: Vec<i64> = listed(p, "/tokens", &bob).iter().map(|t| t["expires"].as_i64().unwrap() - t["made"].as_i64().unwrap()).collect();
        assert_eq!(lives.len(), 2);
        assert!(lives.contains(&7_200_000), "{:?}", lives);
        let dee = session_cookie(&support::send(p, "POST", "/login_for?user=dee&age=800d", &[], ""));
        token(p, &dee, &[]);
        assert_eq!(life(&dee), 366 * 86_400 * 1000);
        // Its cookie gone (a second), the user signs in again: her token is listed and revoked
        // from the new session, and its id is no other user's to take.
        let eve = session_cookie(&support::send(p, "POST", "/login_for?user=eve&age=1s", &[], ""));
        let a = route(p, "POST", "/token_for?user=eve&age=1h", &eve, &[]);
        let t = As::Token(a.body.clone());
        let id = listed(p, "/tokens", &eve)[0]["id"].as_str().unwrap().to_string();
        std::thread::sleep(Duration::from_millis(1300));
        assert_eq!(route(p, "GET", "/tokens", &eve, &[]).body, "[]");
        let eve2 = login(p, "eve", &[]);
        assert_eq!(listed(p, "/tokens", &eve2).len(), 1);
        assert_eq!(route(p, "POST", &format!("/revoke?id={}", id), &ana, &[]).body, "false");
        assert_eq!(list_status(p, &t), 200);
        assert_eq!(route(p, "POST", &format!("/revoke?id={}", id), &eve2, &[]).body, "true");
        assert_eq!(list_status(p, &t), 401);
        drop(s);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Users told apart exactly on `db_line`'s database, whatever its collation takes for the same.
    fn exact_on(name: &str, db_line: &str) {
        let d = review_dir(name, db_line, "");
        let s = support::serve_with(&d.join("main.qwe"), &[]);
        let p = s.port;
        let ana = login(p, "ana", &[]);
        let upper = login(p, "ANA", &[]);
        token(p, &ana, &[]);
        token(p, &upper, &[]);
        let zoe = route(p, "POST", "/token_for?user=zo%C3%AB", &ana, &[]);
        assert_eq!(zoe.status, 200, "{}", zoe.body);
        let users = |path: &str, cookie: &str| listed(p, path, cookie).iter().map(|t| t["user"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        assert_eq!(users("/tokens_of?user=ana", &ana), ["ana"]);
        assert_eq!(users("/tokens_of?user=ANA", &ana), ["ANA"]);
        assert_eq!(users("/tokens_of?user=zoe", &ana), Vec::<String>::new());
        assert_eq!(users("/tokens_of?user=Ana", &ana), Vec::<String>::new());
        // The session's own: made in it for its user, or for its user; not zoë's, made in it for
        // another (decided in the second review), nor `ANA`'s.
        assert_eq!(users("/tokens", &ana), ["ana"]);
        assert_eq!(users("/tokens", &upper), ["ANA"]);
        let zoe_id = listed(p, "/tokens_of?user=zo%C3%AB", &ana)[0]["id"].as_str().unwrap().to_string();
        assert_eq!(route(p, "POST", &format!("/revoke_of?id={}&user=zoe", zoe_id), &ana, &[]).body, "false");
        assert_eq!(route(p, "POST", &format!("/revoke?id={}", zoe_id), &upper, &[]).body, "false");
        assert_eq!(route(p, "POST", &format!("/revoke_of?id={}&user=zo%C3%AB", zoe_id), &ana, &[]).body, "true");
        // A logout ends its own session's alone.
        assert_eq!(route(p, "POST", "/logout", &ana, &[]).status, 204);
        assert_eq!(users("/tokens_of?user=ANA", &upper), ["ANA"]);
        drop(s);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn c3_review_users_are_told_apart_exactly_on_sqlite() {
        exact_on("exact-sqlite", "db.open(\":memory:\")");
    }

    #[test]
    fn c3_review_users_are_told_apart_exactly_on_mariadb() {
        let Some(m) = support::mariadb("c3exact") else { return };
        exact_on("exact-mariadb", &format!("db.open({:?})", m.url("c3exact")));
    }

    #[test]
    fn c3_review_under_watch_the_token_is_said_once_a_failed_first_load_is_fixed() {
        // A first load that fails says no address (as before this slice): the port is named here.
        let d = dir("c3-review-late", "route GET / = \"hi\"\n/// Seven.\nfn mcp seven() -> int with time = 7 +\n");
        let p = support::free_port();
        let (g, said) = logged(&d, &["--watch", "--port", &p.to_string()]);
        let deadline = Instant::now() + WAIT;
        while !support::listening(p) {
            assert!(Instant::now() < deadline, "did not start: {}", said.lock().unwrap());
            std::thread::sleep(Duration::from_millis(20));
        }
        std::thread::sleep(Duration::from_millis(300));
        assert!(!said.lock().unwrap().contains("MCP door"));
        std::fs::write(d.join("main.qwe"), "route GET / = \"hi\"\n/// Seven.\nfn mcp seven() -> int with time = 7\n").unwrap();
        let (url, t) = printed_token(&said);
        assert_eq!(url, format!("http://127.0.0.1:{}/_quillo/mcp", p));
        assert_eq!(call(p, &As::Token(t), "seven", json!({}))["structuredContent"], json!({ "result": 7 }));
        drop(g);
        let _ = std::fs::remove_dir_all(&d);
    }

    // ---- Slice 5, second review (FEEDBACK56.md, Q-859, Q-887; branch feedback56) ----------------

    const SWAP_ROUTES: &str = "route POST /anon = set_session({ cart: [1] })\n\
        route POST /signin ? { user: str } = set_session({ cart: session().cart, user: user })\n\
        route POST /signout = set_session({ cart: session().cart })\n";

    /// The session cookie an answer sets, else the one sent.
    fn next_cookie(a: &support::Answer, was: &str) -> String {
        a.headers.iter().filter(|(k, _)| k == "set-cookie").map(|(_, v)| v.split(';').next().unwrap_or("").to_string()).find(|c| c.contains("quillo_session=")).unwrap_or_else(|| was.to_string())
    }

    #[test]
    fn c3_second_review_a_session_id_kept_from_no_user_is_not_who_made_a_token() {
        // Alice signs in on a visitor's session (its id stays, Q-887), makes a token, and is signed
        // out without `end_session`; Bob signs in on that browser, with the same id. Her token is
        // not his to list, revoke or end.
        let d = review_dir("same-id", "db.open(\":memory:\")", SWAP_ROUTES);
        let s = support::serve_with(&d.join("main.qwe"), &[]);
        let p = s.port;
        let mut c = session_cookie(&support::send(p, "POST", "/anon", &[], ""));
        c = next_cookie(&route(p, "POST", "/signin?user=alice", &c, &[]), &c);
        let alice = As::Token(token(p, &c, &[]));
        let alice_id = listed(p, "/tokens", &c)[0]["id"].as_str().unwrap().to_string();
        c = next_cookie(&route(p, "POST", "/signout", &c, &[]), &c);
        c = next_cookie(&route(p, "POST", "/signin?user=bob", &c, &[]), &c);
        assert_eq!(route(p, "GET", "/tokens", &c, &[]).body, "[]");
        assert_eq!(route(p, "POST", &format!("/revoke?id={}", alice_id), &c, &[]).body, "false");
        let bob = As::Token(token(p, &c, &[]));
        assert_eq!(listed(p, "/tokens", &c).len(), 1);
        assert_eq!(route(p, "POST", "/logout", &c, &[]).status, 204);
        assert_eq!(list_status(p, &bob), 401);
        assert_eq!(call(p, &alice, "whoami", json!({}))["structuredContent"]["user"], json!("alice"));
        assert_eq!(listed(p, "/tokens_of?user=alice", "").len(), 1);
        drop(s);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn c3_second_review_a_database_that_will_not_open_is_no_memory() {
        // The program's database ($QUILLO_DB) and a system database that will not open: each is an
        // error, never tokens kept in memory as if there were no database.
        let d = review_dir("unopened", "", "");
        let bad = review_dir("unopened-system", "db.open(\"data/{tenant}.db\")", "");
        for (dir, env) in [(&d, ("QUILLO_DB", "postgres://nobody@127.0.0.1:9/none")), (&bad, ("QUILLO_SYSTEM_DB", "postgres://nobody@127.0.0.1:9/none"))] {
            let (g, said) = logged_env(dir, &["--watch", "--port", &support::free_port().to_string()][..], &[env]);
            let port: u16 = {
                let deadline = Instant::now() + WAIT;
                loop {
                    let text = said.lock().unwrap().clone();
                    if let Some(p) = text.split("listening on http://127.0.0.1:").nth(1).and_then(|r| r.split(|c: char| !c.is_ascii_digit()).next()).and_then(|p| p.parse().ok()) {
                        break p;
                    }
                    assert!(Instant::now() < deadline, "did not start: {}", text);
                    std::thread::sleep(Duration::from_millis(20));
                }
            };
            let ana = login(port, "ana", &[]);
            let a = route(port, "POST", "/token", &ana, &[]);
            assert_eq!(a.status, 500, "{:?}: {}", env, a.body);
            assert!(a.body.contains("cannot be opened"), "{:?}: {}", env, a.body);
            assert_eq!(list_status(port, &As::Token("qmcp_x".into())), 503, "{:?}", env);
            for path in ["/revoke?id=x", "/logout"] {
                let a = route(port, "POST", path, &ana, &[]);
                assert!(a.status == 500 && a.body.contains("cannot be opened"), "{:?} {}: {}", env, path, a.body);
            }
            assert!(route(port, "GET", "/tokens", &ana, &[]).body.contains("cannot be opened"));
            assert!(!said.lock().unwrap().contains("kept in memory"), "{}", said.lock().unwrap());
            drop(g);
        }
        for x in [d, bad] {
            let _ = std::fs::remove_dir_all(&x);
        }
    }

    #[test]
    fn c3_second_review_memory_holds_100_a_user_and_10000_in_all() {
        let d = review_dir("memory-bounds", "", "route POST /many/:n(int) = len(for i in 0..n => mcp_token())\n");
        let (g, p, _) = watched(&d, &["--watch"]);
        // A stranger's token, before and after 10,000 are kept: one look-up each, not a copy of
        // them all.
        let stranger = || list_status(p, &As::Token("qmcp_strangerstrangerstrangerstrangerstrange".into()));
        let before = median_us(40, || assert_eq!(stranger(), 401));
        let many = |cookie: &str, n: usize| route(p, "POST", &format!("/many/{}", n), cookie, &[]);
        // A user's 100, the limit and one past it; one revoked makes room.
        let ana = login(p, "ana", &[]);
        assert_eq!(many(&ana, 99).body, "99");
        assert_eq!(many(&ana, 1).body, "1");
        let a = many(&ana, 1);
        assert!(a.status == 500 && a.body.contains("this user has 100 already, the most kept in memory for one user"), "{}", a.body);
        let first = listed(p, "/tokens", &ana)[0]["id"].as_str().unwrap().to_string();
        assert_eq!(route(p, "POST", &format!("/revoke?id={}", first), &ana, &[]).body, "true");
        assert_eq!(many(&ana, 1).body, "1");
        // 10,000 in all, then none more, for anyone.
        for u in 1..100 {
            let c = login(p, &format!("u{}", u), &[]);
            assert_eq!(many(&c, 100).body, "100", "u{}", u);
        }
        let late = login(p, "late", &[]);
        let a = many(&late, 1);
        assert!(a.status == 500 && a.body.contains("10000 are kept already, the most kept in memory"), "{}", a.body);
        let after = median_us(40, || assert_eq!(stranger(), 401));
        eprintln!("a stranger's token with no tokens kept: {:.0} µs; with 10,000 in memory: {:.0} µs", before, after);
        assert!(after < before * 3.0 + 300.0, "{:.0} µs, then {:.0} µs", before, after);
        drop(g);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn c3_second_review_a_revoke_under_another_key_stays_when_the_key_comes_back() {
        // QUILLO_SECRET from A to B: A's tokens are no good under B, and one revoked there stays
        // revoked when A comes back.
        let d = review_dir("rotated", "db.open(\"data.db\")", "");
        let start = |key: &str| {
            let port = support::free_port();
            let (g, said) = logged_env(&d, &["--port", &port.to_string()][..], &[("QUILLO_SECRET", key)]);
            let deadline = Instant::now() + WAIT;
            while !support::listening(port) {
                assert!(Instant::now() < deadline, "did not start: {}", said.lock().unwrap());
                std::thread::sleep(Duration::from_millis(20));
            }
            (g, port)
        };
        let (a, b) = ("key A for the tests, long enough to sign with", "key B for the tests, long enough to sign with");
        let (g, p) = start(a);
        let ana = login(p, "ana", &[]);
        let kept = As::Token(token(p, &ana, &[]));
        let first: Vec<Value> = listed(p, "/tokens", &ana);
        let gone = As::Token(token(p, &ana, &[]));
        let gone_id = listed(p, "/tokens", &ana).iter().map(|t| t["id"].as_str().unwrap().to_string()).find(|id| !first.iter().any(|f| f["id"] == json!(id))).unwrap();
        drop(g);
        // Under B, neither is good nor listed; the one revoked there is taken away all the same.
        let (g, p) = start(b);
        assert_eq!(list_status(p, &gone), 401);
        assert_eq!(list_status(p, &kept), 401);
        let ana = login(p, "ana", &[]);
        assert_eq!(route(p, "GET", "/tokens", &ana, &[]).body, "[]");
        assert_eq!(route(p, "POST", &format!("/revoke?id={}", gone_id), &ana, &[]).body, "true");
        drop(g);
        // Back to A: the one left is good again, as its key is; the revoked one stays revoked.
        let (g, p) = start(a);
        assert_eq!(list_status(p, &kept), 200);
        assert_eq!(list_status(p, &gone), 401);
        drop(g);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn c3_second_review_end_session_fails_while_the_door_is_open_and_tokens_cannot_go() {
        let more = "route POST /lock = db.exec(\"pragma query_only = 1\")\nroute POST /unlock = db.exec(\"pragma query_only = 0\")\n";
        let d = review_dir("cannot-delete", "db.open(\"data.db\")", more);
        let (g, p, _) = watched(&d, &["--watch"]);
        let ana = login(p, "ana", &[]);
        let t = As::Token(token(p, &ana, &[]));
        assert_eq!(route(p, "POST", "/lock", "", &[]).status, 200);
        let a = route(p, "POST", "/logout", &ana, &[]);
        assert!(a.status == 500 && a.body.contains("`end_session`: the session's MCP tokens could not be ended, so it was not"), "{}", a.body);
        assert!(!a.headers.iter().any(|(k, v)| k == "set-cookie" && v.contains("quillo_session=")), "{:?}", a.headers);
        assert_eq!(list_status(p, &t), 200);
        assert_eq!(route(p, "POST", "/unlock", "", &[]).status, 200);
        assert_eq!(route(p, "POST", "/logout", &ana, &[]).status, 204);
        assert_eq!(list_status(p, &t), 401);
        drop(g);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn c3_second_review_on_postgres_the_first_tokens_at_once_in_transactions() {
        // Two first `mcp_token()`s inside the program's transactions at once: the table is made
        // apart, the second takes the first's, and neither transaction is given up.
        let Some(pg) = support::postgres("c3race") else { return };
        let more = "route POST /token_slow = db.transaction(() => {\n    let t = mcp_token()\n    sleep(500)\n    t\n})\n";
        let d = review_dir("pg-race", &format!("db.open({:?})", pg.database("c3race")), more);
        let s = support::serve_with(&d.join("main.qwe"), &[]);
        let p = s.port;
        let cookies = [login(p, "ana", &[]), login(p, "bob", &[])];
        let made: Vec<_> = cookies
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let c = c.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(150 * i as u64));
                    route(p, "POST", "/token_slow", &c, &[])
                })
            })
            .collect();
        for (i, h) in made.into_iter().enumerate() {
            let a = h.join().unwrap();
            assert_eq!(a.status, 200, "{}: {}", i, a.body);
            assert_eq!(list_status(p, &As::Token(a.body.clone())), 200);
        }
        drop(s);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// MCP's own TypeScript client (`@modelcontextprotocol/client` 2.4.0, the SDK's v2) over
    /// HTTP, installed into the test's folder: pinned to 2026-07-28 and negotiating, with a
    /// token; as a client of 2025-11-25; and with no token. Skipped only where npm is not found
    /// or the package cannot be installed; a client that does not end in time fails the test.
    #[test]
    fn c2_the_official_typescript_client_drives_it_over_http() {
        let (d, s) = door("sdk", &[]);
        let p = s.port;
        let ana = login(p, "ana", &[]);
        let t = token(p, &ana, &[]);
        let script = r#"const { Client, StreamableHTTPClientTransport } = require("@modelcontextprotocol/client");
(async () => {
  const [url, token] = process.argv.slice(2);
  const out = {};
  for (const mode of ["pin", "auto", "legacy", "none"]) {
    const opts = mode === "legacy" ? {} : { versionNegotiation: { mode: mode === "auto" ? "auto" : { pin: "2026-07-28" } } };
    const client = new Client({ name: "quillo-test", version: "1.0.0" }, opts);
    const headers = mode === "none" ? {} : { Authorization: `Bearer ${token}` };
    const transport = new StreamableHTTPClientTransport(new URL(url), { requestInit: { headers } });
    const r = {};
    try {
      await client.connect(transport);
      r.protocol = client.getNegotiatedProtocolVersion();
      r.tools = (await client.listTools()).tools.map((t) => t.name);
      r.who = await client.callTool({ name: "whoami", arguments: {} });
      r.add = await client.callTool({ name: "add", arguments: { text: "é ✓" } });
      r.mine = await client.callTool({ name: "mine", arguments: {} });
      try { await client.callTool({ name: "nope", arguments: {} }); } catch (e) { r.unknown = e.code; }
    } catch (e) {
      r.failed = { code: e.code, message: String(e.message) };
    }
    try { await client.close(); } catch (e) {}
    out[mode] = r;
  }
  console.log(JSON.stringify(out));
})();
"#;
        std::fs::write(d.join("client.cjs"), script).unwrap();
        std::fs::write(d.join("package.json"), r#"{"name":"quillo-c2-sdk","private":true}"#).unwrap();
        if Command::new("npm").arg("--version").stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().is_err() {
            let _ = std::fs::remove_dir_all(&d);
            eprintln!("skipped: c2_the_official_typescript_client_drives_it_over_http, as npm was not found");
            return;
        }
        match run_for(&d, "npm install --no-audit --no-fund --ignore-scripts @modelcontextprotocol/client@2.4.0", 240) {
            Ok(o) if o.status.success() => {}
            failed => {
                let _ = std::fs::remove_dir_all(&d);
                let why = failed.map_or_else(|e| e, |o| String::from_utf8_lossy(&o.stderr).into_owned());
                eprintln!("skipped: c2_the_official_typescript_client_drives_it_over_http, as npm could not install @modelcontextprotocol/client@2.4.0: {}", why);
                return;
            }
        }
        let ran = run_for(&d, &format!("node client.cjs http://127.0.0.1:{}/_quillo/mcp {}", p, t), 120);
        drop(s);
        let _ = std::fs::remove_dir_all(&d);
        let o = ran.unwrap_or_else(|e| panic!("the client: {}", e));
        let (stdout, stderr) = (String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned());
        assert!(o.status.success(), "the client failed: {}\n{}", stdout, stderr);
        let said: Value = stdout.lines().last().and_then(|l| serde_json::from_str(l).ok()).unwrap_or_else(|| panic!("the client said no JSON: {}\n{}", stdout, stderr));
        for (mode, count) in [("pin", 1), ("auto", 2)] {
            let r = &said[mode];
            assert!(r.get("failed").is_none(), "{}: {}", mode, said);
            assert_eq!(r["protocol"], json!(V));
            assert_eq!(r["tools"], json!(["whoami", "add", "mine", "sets", "echo", "spin"]));
            // The client checks each `structuredContent` against the tool's `outputSchema`.
            assert_eq!(r["who"]["structuredContent"], json!({ "user": "ana", "mcp": { "client": "quillo-test", "version": "1.0.0" }, "tenant": null }), "{}", r);
            assert_eq!(r["add"]["structuredContent"], json!({ "result": count }), "{}", r);
            assert_eq!(r["mine"]["structuredContent"], json!({ "result": "ana" }));
            assert_eq!(r["unknown"], json!(-32602));
        }
        // A client of 2025-11-25, the SDK's default: refused at `initialize` with the version
        // error; with no token, told the door needs one.
        assert!(said["legacy"]["failed"]["message"].as_str().is_some_and(|m| m.contains("-32022")), "{}", said);
        assert!(said["none"]["failed"]["message"].as_str().is_some_and(|m| m.contains("401")), "{}", said);
    }
}
