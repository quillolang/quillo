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
