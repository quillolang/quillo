//! `quillo lsp`: a Language Server Protocol server over stdin/stdout.
//!
//! It checks documents as they change (with `crate::check`) and answers
//! hover, go-to-definition, completion and document-symbol requests from
//! what the checker records. Any LSP client can use it; the VS Code
//! extension in `editors/vscode` starts it automatically.

use crate::check::{self, Analysis, Severity, SymbolKind};
use serde_json::{json, Value as J};
use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

const KEYWORDS: &[&str] = &[
    "let", "fn", "view", "component", "server", "route", "use", "mount", "return", "if", "else", "match", "for", "in",
    "true", "false", "null", "reply", "as", "with", "state", "type", "service",
];
const TYPES: &[&str] = &["int", "float", "num", "str", "bool", "list", "map", "html", "fn", "any", "response", "null"];
const METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "HEAD", "ANY"];
const EFFECTS: &[&str] = &["db", "http", "fs", "env", "time", "random"];

struct Doc {
    text: String,
    analysis: Analysis,
}

pub struct Server {
    docs: HashMap<String, Doc>,
    out: io::Stdout,
    /// From the editor's `initializationOptions` (`{ "strictNull": true }`).
    opts: check::Options,
}

fn read_message(input: &mut impl BufRead) -> io::Result<Option<J>> {
    let mut len = None;
    loop {
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(v) = line.strip_prefix("Content-Length:") {
            len = v.trim().parse::<usize>().ok();
        }
    }
    let Some(len) = len else { return Ok(Some(J::Null)) };
    let mut buf = vec![0; len];
    input.read_exact(&mut buf)?;
    Ok(Some(serde_json::from_slice(&buf).unwrap_or(J::Null)))
}

/// The main file of the program `file` belongs to, when another file
/// `use`s it (directly, or through files it uses): looked for in the file's
/// folder and the folders above it, nearest first. `use` paths are from
/// the main file's folder.
pub fn program_entry(file: &Path) -> Option<PathBuf> {
    let target = file.canonicalize().ok()?;
    let mut dir = target.parent()?.to_path_buf();
    for _ in 0..4 {
        let mut candidates: Vec<PathBuf> = std::fs::read_dir(&dir)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "qwe") && p.as_path() != target.as_path())
            .collect();
        candidates.sort();
        for main in candidates {
            if uses_transitively(&main, &dir, &target) {
                return Some(main);
            }
        }
        // Not above the project.
        if dir.join("Cargo.toml").exists() || dir.join(".git").exists() || !dir.pop() {
            break;
        }
    }
    None
}

/// Whether `main` (in `base`) reaches `target` through its `use`s.
fn uses_transitively(main: &Path, base: &Path, target: &Path) -> bool {
    let mut todo = vec![main.to_path_buf()];
    let mut seen = std::collections::HashSet::new();
    while let Some(f) = todo.pop() {
        if !seen.insert(f.clone()) || seen.len() > 500 {
            continue;
        }
        let Ok(src) = std::fs::read_to_string(&f) else { continue };
        for line in src.lines() {
            let Some(rest) = line.trim_start().strip_prefix("use ") else { continue };
            let Some(path) = rest.trim().strip_prefix('"').and_then(|r| r.split('"').next()) else { continue };
            let Ok(full) = base.join(path).canonicalize() else { continue };
            if full == target {
                return true;
            }
            todo.push(full);
        }
    }
    false
}

pub fn uri_to_path(uri: &str) -> PathBuf {
    let p = uri.strip_prefix("file://").unwrap_or(uri);
    PathBuf::from(crate::server::percent_decode(p, false))
}

pub fn path_to_uri(p: &str) -> String {
    let abs = Path::new(p).canonicalize().map(|x| x.display().to_string()).unwrap_or_else(|_| p.to_string());
    let mut out = String::from("file://");
    for b in abs.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// UTF-16 length of a prefix of `line` that is `chars` characters long.
fn utf16_col(line: &str, chars: usize) -> usize {
    line.chars().take(chars).map(char::len_utf16).sum()
}

/// Character index for a UTF-16 column.
fn char_col(line: &str, utf16: usize) -> usize {
    let mut n = 0;
    for (i, c) in line.chars().enumerate() {
        if n >= utf16 {
            return i;
        }
        n += c.len_utf16();
    }
    line.chars().count()
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Where `word` appears as a whole word in `line`, as character columns.
fn find_word(line: &str, word: &str) -> Option<(usize, usize)> {
    let chars: Vec<char> = line.chars().collect();
    let w: Vec<char> = word.chars().collect();
    if w.is_empty() || w.len() > chars.len() {
        return None;
    }
    for i in 0..=chars.len() - w.len() {
        if chars[i..i + w.len()] == w[..]
            && (i == 0 || !is_word(chars[i - 1]))
            && (i + w.len() == chars.len() || !is_word(chars[i + w.len()]))
        {
            return Some((i, i + w.len()));
        }
    }
    None
}

/// The range a diagnostic underlines: the first `name` in backticks that
/// appears on the line, else the line's text.
fn diag_range(text: &str, line: u32, msg: &str) -> J {
    let l = line.saturating_sub(1) as usize;
    let src = text.lines().nth(l).unwrap_or("");
    let mut span = None;
    for part in msg.split('`').skip(1).step_by(2) {
        let word = part.trim_start_matches('~').trim_start_matches('<').trim_end_matches('>');
        if let Some(s) = find_word(src, word) {
            span = Some(s);
            break;
        }
    }
    let (a, b) = span.unwrap_or_else(|| {
        let start = src.chars().take_while(|c| c.is_whitespace()).count();
        (start, src.chars().count().max(start + 1))
    });
    json!({
        "start": { "line": l, "character": utf16_col(src, a) },
        "end": { "line": l, "character": utf16_col(src, b) }
    })
}

fn word_at(text: &str, line: usize, col16: usize) -> Option<(String, usize)> {
    let src = text.lines().nth(line)?;
    let chars: Vec<char> = src.chars().collect();
    let col = char_col(src, col16).min(chars.len());
    let mut a = col;
    while a > 0 && is_word(chars[a - 1]) {
        a -= 1;
    }
    let mut b = col;
    while b < chars.len() && is_word(chars[b]) {
        b += 1;
    }
    (a < b).then(|| (chars[a..b].iter().collect(), a))
}

fn symbol_kind(k: SymbolKind) -> u32 {
    match k {
        SymbolKind::Function | SymbolKind::ServerFn => 12,
        SymbolKind::View | SymbolKind::Component => 5,
        SymbolKind::Variable => 13,
        SymbolKind::Builtin => 12,
        SymbolKind::Route => 6,
    }
}

impl Server {
    pub fn new() -> Self {
        Server { docs: HashMap::new(), out: io::stdout(), opts: check::Options::default() }
    }

    fn send(&mut self, msg: J) {
        let body = msg.to_string();
        let mut out = self.out.lock();
        let _ = write!(out, "Content-Length: {}\r\n\r\n{}", body.len(), body);
        let _ = out.flush();
    }

    fn reply(&mut self, id: J, result: J) {
        self.send(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
    }

    pub fn run(&mut self) -> i32 {
        let stdin = io::stdin();
        let mut input = stdin.lock();
        let mut shutdown = false;
        loop {
            let msg = match read_message(&mut input) {
                Ok(Some(m)) => m,
                Ok(None) | Err(_) => return if shutdown { 0 } else { 1 },
            };
            let method = msg["method"].as_str().unwrap_or("").to_string();
            let id = msg.get("id").cloned();
            let params = &msg["params"];
            match method.as_str() {
                "initialize" => {
                    self.opts.strict_null = params["initializationOptions"]["strictNull"].as_bool().unwrap_or(false);
                    let caps = json!({
                        "capabilities": {
                            "textDocumentSync": { "openClose": true, "change": 1, "save": { "includeText": false } },
                            "hoverProvider": true,
                            "definitionProvider": true,
                            "documentSymbolProvider": true,
                            "completionProvider": { "triggerCharacters": [".", "<", "@"] }
                        },
                        "serverInfo": { "name": "quillo", "version": env!("CARGO_PKG_VERSION") }
                    });
                    self.reply(id.unwrap_or(J::Null), caps);
                }
                "shutdown" => {
                    shutdown = true;
                    self.reply(id.unwrap_or(J::Null), J::Null);
                }
                "exit" => return if shutdown { 0 } else { 1 },
                "textDocument/didOpen" => {
                    let uri = params["textDocument"]["uri"].as_str().unwrap_or("").to_string();
                    let text = params["textDocument"]["text"].as_str().unwrap_or("").to_string();
                    self.update(uri, text);
                }
                "textDocument/didChange" => {
                    let uri = params["textDocument"]["uri"].as_str().unwrap_or("").to_string();
                    if let Some(text) = params["contentChanges"].as_array().and_then(|c| c.last()).and_then(|c| c["text"].as_str()) {
                        self.update(uri, text.to_string());
                    }
                }
                "textDocument/didSave" => {
                    // Other open files may `use` the saved one.
                    let open: Vec<(String, String)> = self.docs.iter().map(|(u, d)| (u.clone(), d.text.clone())).collect();
                    for (u, t) in open {
                        self.update(u, t);
                    }
                }
                "textDocument/didClose" => {
                    let uri = params["textDocument"]["uri"].as_str().unwrap_or("").to_string();
                    self.docs.remove(&uri);
                    self.send(json!({ "jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": { "uri": uri, "diagnostics": [] } }));
                }
                "textDocument/hover" => {
                    let r = self.hover(params);
                    self.reply(id.unwrap_or(J::Null), r);
                }
                "textDocument/definition" => {
                    let r = self.definition(params);
                    self.reply(id.unwrap_or(J::Null), r);
                }
                "textDocument/completion" => {
                    let r = self.completion(params);
                    self.reply(id.unwrap_or(J::Null), r);
                }
                "textDocument/documentSymbol" => {
                    let r = self.document_symbols(params);
                    self.reply(id.unwrap_or(J::Null), r);
                }
                _ => {
                    if let Some(id) = id {
                        self.send(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("unsupported: {}", method) } }));
                    }
                }
            }
        }
    }

    fn update(&mut self, uri: String, text: String) {
        let path = uri_to_path(&uri);
        let file = path.display().to_string();
        let base = path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
        // A file another one `use`s is part of that program: checked as the
        // program, from its main file, with what is open in the editor.
        let analysis = match program_entry(&path) {
            Some(main) => {
                let mut open: HashMap<PathBuf, String> = self.docs.iter().map(|(u, d)| (uri_to_path(u), d.text.clone())).collect();
                open.insert(path.clone(), text.clone());
                check::check_file_opts(&main, open, self.opts)
            }
            None => check::check_source_opts(&text, &file, &base, self.opts),
        };
        let mut by_file: HashMap<String, Vec<J>> = HashMap::new();
        by_file.insert(uri.clone(), Vec::new());
        for d in &analysis.diagnostics {
            let (duri, range) = if d.file == file {
                (uri.clone(), diag_range(&text, d.line, &d.message))
            } else {
                let other = std::fs::read_to_string(&d.file).unwrap_or_default();
                (path_to_uri(&d.file), diag_range(&other, d.line, &d.message))
            };
            // Problems in a `use`d file are shown there only if it is not
            // open (it is checked on its own when open).
            if duri != uri && self.docs.contains_key(&duri) {
                continue;
            }
            by_file.entry(duri).or_default().push(json!({
                "range": range,
                "severity": if d.severity == Severity::Error { 1 } else { 2 },
                "source": "quillo",
                "message": d.message,
            }));
        }
        for (u, diags) in by_file {
            self.send(json!({ "jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": { "uri": u, "diagnostics": diags } }));
        }
        // While the text does not parse (mid-edit), keep what was known.
        let mut analysis = analysis;
        if analysis.symbols.is_empty() && analysis.references.is_empty() {
            if let Some(old) = self.docs.remove(&uri) {
                analysis.symbols = old.analysis.symbols;
                analysis.references = old.analysis.references;
            }
        }
        self.docs.insert(uri, Doc { text, analysis });
    }

    fn position(params: &J) -> (String, usize, usize) {
        (
            params["textDocument"]["uri"].as_str().unwrap_or("").to_string(),
            params["position"]["line"].as_u64().unwrap_or(0) as usize,
            params["position"]["character"].as_u64().unwrap_or(0) as usize,
        )
    }

    fn reference_at(&self, params: &J) -> Option<(&Doc, check::Reference)> {
        let (uri, line, col) = Self::position(params);
        let doc = self.docs.get(&uri)?;
        let (word, _) = word_at(&doc.text, line, col)?;
        let file = uri_to_path(&uri).display().to_string();
        let r = doc
            .analysis
            .references
            .iter()
            .find(|r| r.name == word && r.line as usize == line + 1 && r.file == file)
            .or_else(|| doc.analysis.references.iter().find(|r| r.name == word && r.file == file))
            .cloned()
            .or_else(|| {
                doc.analysis.symbols.iter().find(|s| s.name == word).map(|s| check::Reference {
                    file: s.file.clone(),
                    line: s.line,
                    name: s.name.clone(),
                    def: Some((s.file.clone(), s.line)),
                    detail: s.detail.clone(),
                })
            })?;
        Some((doc, r))
    }

    fn hover(&self, params: &J) -> J {
        if let Some((_, r)) = self.reference_at(params) {
            return json!({ "contents": { "kind": "markdown", "value": format!("```quillo\n{}\n```", r.detail) } });
        }
        let (uri, line, col) = Self::position(params);
        if let Some((w, _)) = self.docs.get(&uri).and_then(|d| word_at(&d.text, line, col)) {
            if let Some(text) = keyword_doc(&w) {
                return json!({ "contents": { "kind": "markdown", "value": text } });
            }
        }
        J::Null
    }

    fn definition(&self, params: &J) -> J {
        let Some((_, r)) = self.reference_at(params) else { return J::Null };
        let Some((file, line)) = r.def else { return J::Null };
        let (uri, _, _) = Self::position(params);
        let text = if uri_to_path(&uri).display().to_string() == file {
            self.docs.get(&uri).map(|d| d.text.clone()).unwrap_or_default()
        } else {
            std::fs::read_to_string(&file).unwrap_or_default()
        };
        let l = line.saturating_sub(1) as usize;
        let src = text.lines().nth(l).unwrap_or("");
        let (a, b) = find_word(src, &r.name).unwrap_or((0, 0));
        json!({
            "uri": path_to_uri(&file),
            "range": {
                "start": { "line": l, "character": utf16_col(src, a) },
                "end": { "line": l, "character": utf16_col(src, b) }
            }
        })
    }

    fn completion(&self, params: &J) -> J {
        let (uri, line, col) = Self::position(params);
        let Some(doc) = self.docs.get(&uri) else { return json!([]) };
        let src = doc.text.lines().nth(line).unwrap_or("");
        let before: String = src.chars().take(char_col(src, col)).collect();
        let mut items = Vec::new();
        // After `db.`, `http.` or `json.`: that module's functions.
        let trimmed = before.trim_end_matches(is_word);
        if let Some(prefix) = trimmed.strip_suffix('.') {
            let module: String = prefix.chars().rev().take_while(|c| is_word(*c)).collect::<Vec<_>>().into_iter().rev().collect();
            for (name, sig) in check::builtin_signatures() {
                if let Some(f) = name.strip_prefix(&format!("{}.", module)) {
                    items.push(json!({ "label": f, "kind": 3, "detail": sig }));
                }
            }
            if !items.is_empty() {
                return json!(items);
            }
        }
        let file = uri_to_path(&uri).display().to_string();
        let mut seen = std::collections::HashSet::new();
        // Names used or defined near the cursor (locals, parameters, state).
        for r in &doc.analysis.references {
            if r.file == file && r.def.as_ref().map_or(false, |(f, _)| *f == file) && seen.insert(r.name.clone()) {
                items.push(json!({ "label": r.name, "kind": 6, "detail": r.detail }));
            }
        }
        for s in &doc.analysis.symbols {
            if s.kind != SymbolKind::Route && seen.insert(s.name.clone()) {
                let kind = match s.kind {
                    SymbolKind::Variable => 6,
                    SymbolKind::View | SymbolKind::Component => 7,
                    _ => 3,
                };
                items.push(json!({ "label": s.name, "kind": kind, "detail": s.detail }));
            }
        }
        for (name, sig) in check::builtin_signatures() {
            if !name.contains('.') && seen.insert(name.clone()) {
                items.push(json!({ "label": name, "kind": 3, "detail": sig }));
            }
        }
        for m in ["json", "db", "http", "udp", "channel", "media"] {
            if seen.insert(m.to_string()) {
                items.push(json!({ "label": m, "kind": 9, "detail": format!("module {}", m) }));
            }
        }
        for k in KEYWORDS {
            items.push(json!({ "label": k, "kind": 14 }));
        }
        for t in TYPES {
            if seen.insert(t.to_string()) {
                items.push(json!({ "label": t, "kind": 25, "detail": "type" }));
            }
        }
        if before.trim_start().starts_with("route") {
            for m in METHODS {
                items.push(json!({ "label": m, "kind": 14, "detail": "HTTP method" }));
            }
        }
        if before.contains(" with ") {
            for e in EFFECTS {
                items.push(json!({ "label": e, "kind": 14, "detail": "effect" }));
            }
        }
        json!(items)
    }

    fn document_symbols(&self, params: &J) -> J {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or("");
        let Some(doc) = self.docs.get(uri) else { return json!([]) };
        let file = uri_to_path(uri).display().to_string();
        let lines: Vec<&str> = doc.text.lines().collect();
        let out: Vec<J> = doc
            .analysis
            .symbols
            .iter()
            .filter(|s| s.file == file)
            .map(|s| {
                let l = s.line.saturating_sub(1) as usize;
                let src = lines.get(l).copied().unwrap_or("");
                let (a, b) = find_word(src, &s.name).unwrap_or((0, src.chars().count()));
                let full = json!({ "start": { "line": l, "character": 0 }, "end": { "line": l, "character": utf16_col(src, src.chars().count()) } });
                let sel = json!({ "start": { "line": l, "character": utf16_col(src, a) }, "end": { "line": l, "character": utf16_col(src, b) } });
                json!({ "name": s.name, "detail": s.detail, "kind": symbol_kind(s.kind), "range": full, "selectionRange": sel })
            })
            .collect();
        json!(out)
    }
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

fn keyword_doc(w: &str) -> Option<String> {
    let text = match w {
        "route" => "`route METHOD /path/:param ? { query } { request pattern } = expr`\n\nDeclares an HTTP route. Routes are tried in order; the first whose method, path and patterns match handles the request.",
        "component" => "`component Name(props) { state count = 0 ... <html/> }`\n\nA view that also runs in the browser: rendered on the server, then interactive. Assigning to its state re-renders it. Handlers: `@click={() => { count += 1 }}`.",
        "server" => "`server fn name(params) = ...`\n\nA function the browser can call. Component code awaits it automatically.",
        "view" => "`view Name(params, children) = <html/>`\n\nA function returning HTML, used as `<Name .../>`.",
        "reply" => "`reply status [body]`\n\nEnds the current request immediately with that response, from anywhere.",
        "with" => "`fn f() with db, http = ...`\n\nDeclares the effects a function may use: db, http, fs, env, time, random.",
        "match" => "`match value { pattern [if guard] => result ... }`",
        "use" => "`use \"lib.qwe\"` runs another file once, into the global scope.",
        "mount" => "`mount /prefix = \"app.qwe\"` serves another program under a path prefix, with its own globals and database.",
        "state" => "`state count = 0` in a component: state, set on the first render and kept between renders (`~count = 0` is the short form). Assigning to `count` re-renders the component.",
        "type" => "`type User = { name: str, role: \"admin\" | \"member\" = \"member\" }`\n\nA record (or another name for a type). `User({ ... })` makes one, or gives an error value; in annotations and patterns it is checked field by field.",
        "service" => "`service billing = \"billing.qwe\"` (or `service /shop = \"shop.qwe\"`): a part of the app that runs here or as a server of its own; its `server fn`s are called as `billing.invoice(...)`.",
        _ => return None,
    };
    Some(text.to_string())
}
