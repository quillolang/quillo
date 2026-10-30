//! `quillo lsp`: a Language Server Protocol server over stdin/stdout.
//!
//! It checks documents as they change (with `crate::check`) and answers
//! hover, go-to-definition, completion and document-symbol requests from
//! what the checker records. Any LSP client can use it; the VS Code
//! extension (quillolang/editors) starts it automatically.

use crate::check::{self, Analysis, Severity, SymbolKind};
use serde_json::{json, Value as J};
use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const KEYWORDS: &[&str] = &[
    "let", "fn", "view", "component", "server", "route", "use", "mount", "return", "if", "else", "match", "for", "while", "in",
    "true", "false", "null", "reply", "as", "with", "state", "type", "service", "private", "public",
];
const TYPES: &[&str] = &["int", "float", "dec", "num", "str", "bool", "list", "map", "html", "fn", "any", "response", "null"];
const METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "HEAD", "ANY"];
/// HTML's elements, offered after `<` with the program's views and components.
const HTML_TAGS: &[&str] = &[
    "a", "abbr", "article", "aside", "audio", "b", "blockquote", "body", "br", "button", "canvas", "caption", "code", "col",
    "colgroup", "datalist", "dd", "details", "dialog", "div", "dl", "dt", "em", "fieldset", "figcaption", "figure", "footer",
    "form", "h1", "h2", "h3", "h4", "h5", "h6", "head", "header", "hr", "html", "i", "iframe", "img", "input", "kbd", "label",
    "legend", "li", "link", "main", "mark", "meta", "meter", "nav", "noscript", "ol", "optgroup", "option", "output", "p",
    "picture", "pre", "progress", "q", "s", "script", "section", "select", "slot", "small", "source", "span", "strong", "style",
    "sub", "summary", "sup", "svg", "table", "tbody", "td", "template", "textarea", "tfoot", "th", "thead", "time", "title",
    "tr", "track", "u", "ul", "video",
];

struct Doc {
    text: String,
    analysis: Analysis,
}

pub struct Server {
    docs: HashMap<String, Doc>,
    out: io::Stdout,
    /// From the editor's `initializationOptions` (`{ "strictNull": true }`).
    opts: check::Options,
    /// `.qwer` files' diagnostics: Quillo's (as they are typed) and the Rust
    /// compiler's (when opened and saved, worked out on a thread of their own).
    rust: Arc<Mutex<HashMap<String, RustDiags>>>,
}

#[derive(Default)]
struct RustDiags {
    quillo: Vec<J>,
    cargo: Vec<J>,
    /// A check is running; another is wanted when it ends.
    running: bool,
    again: bool,
}

/// Sends a message from any thread (stdout's lock keeps messages whole).
fn send_any(msg: &J) {
    // Private names as they were written (src/privacy.rs; branch public-private).
    let body = crate::privacy::demangle(&msg.to_string());
    let mut out = io::stdout().lock();
    let _ = write!(out, "Content-Length: {}\r\n\r\n{}", body.len(), body);
    let _ = out.flush();
}

fn publish_rust(uri: &str, d: &RustDiags) {
    let all: Vec<J> = d.quillo.iter().chain(d.cargo.iter()).cloned().collect();
    send_any(&json!({ "jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": { "uri": uri, "diagnostics": all } }));
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
            // A `.qwer` file uses no other.
            let Some(u) = use_line(line).filter(|u| !u.path.ends_with(".qwer")) else { continue };
            let Ok(full) = base.join(&u.path).canonicalize() else { continue };
            if full == target {
                return true;
            }
            todo.push(full);
        }
    }
    false
}

/// A `use` line as written: the names it takes (each with the name it has
/// here, after `as`, and its column), the file it names, as the parser
/// reads it, and that file's columns on the line, in characters (branch
/// use-editor).
#[derive(Debug, Default)]
struct UseLine {
    names: Vec<(String, String, usize)>,
    path: String,
    at: (usize, usize),
}

fn use_line(src: &str) -> Option<UseLine> {
    let chars: Vec<char> = src.chars().collect();
    let skip = |mut i: usize| {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        i
    };
    let word_end = |mut i: usize| {
        while i < chars.len() && is_word(chars[i]) {
            i += 1;
        }
        i
    };
    let word = |a: usize, b: usize| chars[a..b].iter().collect::<String>();
    let mut i = skip(0);
    if word_end(i) != i + 3 || word(i, i + 3) != "use" {
        return None;
    }
    i = skip(i + 3);
    let mut u = UseLine::default();
    // `with module, preload`, before `from` (branch library-hints).
    let skip_with = |mut j: usize| {
        if word(j, word_end(j)) == "with" {
            while j < chars.len() && word(j, word_end(j)) != "from" {
                j = skip(if chars[j] == ',' { j + 1 } else { word_end(j).max(j + 1) });
            }
        }
        j
    };
    if chars.get(i) == Some(&'*') {
        i = skip(i + 1);
        // `use * as THREE with module from ...`.
        if word(i, word_end(i)) == "as" {
            i = skip(word_end(skip(word_end(i))));
            i = skip_with(i);
        }
        let e = word_end(i);
        if word(i, e) != "from" {
            return None;
        }
        i = skip(e);
    } else if chars.get(i) != Some(&'"') {
        // Names, then `from`: told apart as the parser does, by the words before it.
        let rest: String = chars[i..].iter().collect();
        let words: Vec<&str> = rest.split(|c: char| c.is_whitespace() || c == ',').filter(|w| !w.is_empty()).collect();
        let ident = |w: &str| w.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') && w.chars().all(is_word);
        if words.iter().position(|w| *w == "from").is_some_and(|at| at > 0 && words[..at].iter().all(|w| ident(w))) {
            loop {
                let e = word_end(i);
                let name = word(i, e);
                if name == "from" {
                    i = skip(e);
                    break;
                }
                let (col, mut j) = (i, skip(e));
                let mut alias = name.clone();
                if word(j, word_end(j)) == "as" {
                    let a = skip(word_end(j));
                    j = word_end(a);
                    alias = word(a, j);
                    j = skip(j);
                }
                j = skip_with(j);
                u.names.push((name, alias, col));
                if chars.get(j) == Some(&',') {
                    j = skip(j + 1);
                }
                if j == i {
                    return None;
                }
                i = j;
            }
        }
    }
    if chars.get(i) == Some(&'"') {
        let end = chars[i + 1..].iter().position(|c| *c == '"').map_or(chars.len(), |p| i + 1 + p);
        u.path = word(i + 1, end);
        u.at = (i + 1, end);
        return Some(u);
    }
    let mut e = i;
    while e < chars.len() && (chars[e].is_alphanumeric() || "-._~%/\\".contains(chars[e])) {
        e += 1;
    }
    if e == i {
        return None;
    }
    u.path = word(i, e).replace('\\', "/");
    if !u.path.ends_with(".qwe") && !u.path.ends_with(".qwer") {
        u.path.push_str(if u.names.is_empty() { ".qwe" } else { ".qwer" });
    }
    u.at = (i, e);
    Some(u)
}

/// The folder a file's `use` paths are read from: its program's main
/// file's (branch use-editor).
fn use_base(file: &Path) -> PathBuf {
    program_entry(file).unwrap_or_else(|| file.to_path_buf()).parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."))
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

/// Where a position (line, UTF-16 column) is in `text`, as a byte index.
fn byte_offset(text: &str, line: usize, col16: usize) -> usize {
    let mut at = 0;
    for (i, l) in text.split_inclusive('\n').enumerate() {
        if i == line {
            let mut n = 0;
            for (b, c) in l.char_indices() {
                if n >= col16 || c == '\n' {
                    return at + b;
                }
                n += c.len_utf16();
            }
            return at + l.len();
        }
        at += l.len();
    }
    text.len()
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
        let word = part.trim_start_matches('~').trim_start_matches('<').trim_start_matches('.').trim_end_matches('>');
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

fn is_class_char(c: char) -> bool {
    c.is_alphanumeric() || c == '-' || c == '_'
}

/// Text before a class name that makes it one: `.` in a tag (`<span.chip`,
/// `<a.btn.primary`), or an open `class="...` (or `class={...` with a
/// string open in it).
fn before_class(pre: &[char]) -> bool {
    if pre.last() == Some(&'.') {
        let mut i = pre.len() - 1;
        while i > 0 && (is_class_char(pre[i - 1]) || pre[i - 1] == '.' || pre[i - 1] == '#') {
            i -= 1;
        }
        return i > 0 && pre[i - 1] == '<' && pre[i].is_alphabetic();
    }
    let pre: String = pre.iter().collect();
    let Some(at) = pre.rfind("class=") else { return false };
    let rest = &pre[at + "class=".len()..];
    if let Some(r) = rest.strip_prefix('"') {
        return !r.contains('"');
    }
    rest.starts_with('{') && (rest.matches('"').count() % 2 == 1 || rest.matches('`').count() % 2 == 1)
}

/// The class name at a position (UTF-16 column) and its first column, in
/// characters, when it is in a place that names a class.
fn class_at(src: &str, col16: usize) -> Option<(String, usize)> {
    let chars: Vec<char> = src.chars().collect();
    let col = char_col(src, col16).min(chars.len());
    let mut a = col;
    while a > 0 && is_class_char(chars[a - 1]) {
        a -= 1;
    }
    let mut b = col;
    while b < chars.len() && is_class_char(chars[b]) {
        b += 1;
    }
    (a < b && before_class(&chars[..a])).then(|| (chars[a..b].iter().collect(), a))
}

/// Where `.class` is on a line of CSS, as character columns of the name.
fn find_class(line: &str, class: &str) -> Option<(usize, usize)> {
    let chars: Vec<char> = line.chars().collect();
    let w: Vec<char> = class.chars().collect();
    (0..chars.len()).find_map(|i| {
        let end = i + 1 + w.len();
        (chars[i] == '.' && end <= chars.len() && chars[i + 1..end] == w[..] && chars.get(end).map_or(true, |c| !is_class_char(*c)))
            .then(|| (i + 1, end))
    })
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
        Server { docs: HashMap::new(), out: io::stdout(), opts: check::Options { classes: true, ..Default::default() }, rust: Default::default() }
    }

    fn send(&mut self, msg: J) {
        // Private names as they were written (src/privacy.rs; branch public-private).
        let body = crate::privacy::demangle(&msg.to_string());
        let mut out = self.out.lock();
        let _ = write!(out, "Content-Length: {}\r\n\r\n{}", body.len(), body);
        let _ = out.flush();
    }

    fn reply(&mut self, id: J, result: J) {
        self.send(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
    }

    pub fn run(&mut self) -> i32 {
        crate::error::note_start();
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
                            "completionProvider": { "triggerCharacters": [".", "<", "@", "/"] },
                            "codeActionProvider": { "codeActionKinds": ["quickfix"] },
                            "referencesProvider": true,
                            "renameProvider": { "prepareProvider": true },
                            // CSS files are welcome: their classes (an
                            // older server would read them as Quillo).
                            "experimental": { "cssClasses": true }
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
                    let rust = uri.ends_with(".qwer");
                    self.update(uri.clone(), text);
                    if rust {
                        self.check_rust(uri);
                    }
                }
                "textDocument/didChange" => {
                    let uri = params["textDocument"]["uri"].as_str().unwrap_or("").to_string();
                    if let Some(text) = params["contentChanges"].as_array().and_then(|c| c.last()).and_then(|c| c["text"].as_str()) {
                        self.update(uri, text.to_string());
                    }
                }
                "textDocument/didSave" => {
                    let uri = params["textDocument"]["uri"].as_str().unwrap_or("").to_string();
                    if uri.ends_with(".qwer") {
                        self.check_rust(uri);
                    }
                    // Other open files may `use` the saved one.
                    let open: Vec<(String, String)> = self.docs.iter().map(|(u, d)| (u.clone(), d.text.clone())).collect();
                    for (u, t) in open {
                        self.update(u, t);
                    }
                }
                "textDocument/didClose" => {
                    let uri = params["textDocument"]["uri"].as_str().unwrap_or("").to_string();
                    self.docs.remove(&uri);
                    self.rust.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&uri);
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
                "textDocument/codeAction" => {
                    let r = self.code_actions(params);
                    self.reply(id.unwrap_or(J::Null), r);
                }
                "textDocument/references" => {
                    let r = self.class_references(params);
                    self.reply(id.unwrap_or(J::Null), r);
                }
                "textDocument/prepareRename" => {
                    let r = self.prepare_rename(params);
                    self.reply(id.unwrap_or(J::Null), r);
                }
                "textDocument/rename" => match self.rename(params) {
                    Ok(r) => self.reply(id.unwrap_or(J::Null), r),
                    Err(message) => self.send(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32602, "message": message } })),
                },
                "quillo/closeTag" => {
                    let r = self.close_tag(params);
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
        if uri.ends_with(".qwer") {
            return self.update_rust(uri, text);
        }
        // An app's style sheet: kept for its classes' rules, as typed.
        if uri.ends_with(".css") {
            self.docs.insert(uri, Doc { text, analysis: Analysis::default() });
            return;
        }
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
            // A file that does not parse may be for a newer Quillo than
            // this server: say which it is.
            let message = if d.severity == Severity::Error && analysis.syntax_errors.contains(&(d.file.clone(), d.line)) {
                format!("{}\n({})", d.message, crate::error::which_quillo())
            } else {
                d.message.clone()
            };
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
                "severity": match d.severity { Severity::Error => 1, Severity::Warning => 2, Severity::Hint => 4 },
                "source": "quillo",
                "message": message,
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
                analysis.classes = old.analysis.classes;
                analysis.files = old.analysis.files;
            }
        }
        self.docs.insert(uri, Doc { text, analysis });
    }

    /// A `.qwer` file: what Quillo makes of it (its exports, and what it
    /// cannot export), as it is typed.
    fn update_rust(&mut self, uri: String, text: String) {
        let path = uri_to_path(&uri);
        let file = path.display().to_string();
        let parsed = crate::native::parse(&text);
        let quillo: Vec<J> = parsed
            .problems
            .iter()
            .map(|(line, msg)| json!({ "range": diag_range(&text, *line, msg), "severity": 2, "source": "quillo", "message": msg }))
            .collect();
        let symbols = parsed
            .exports
            .iter()
            .map(|e| {
                let mut detail = check::native_signature(e);
                if !e.doc.is_empty() {
                    detail = format!("{}\n\n{}", detail, e.doc);
                }
                check::Symbol { name: e.name.clone(), kind: SymbolKind::Function, file: file.clone(), line: e.line, detail }
            })
            .collect();
        {
            let mut all = self.rust.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let d = all.entry(uri.clone()).or_default();
            d.quillo = quillo;
            publish_rust(&uri, d);
        }
        self.docs.insert(uri, Doc { text, analysis: check::Analysis { symbols, ..Default::default() } });
    }

    /// The Rust compiler's view of a `.qwer` file as saved, on a thread of
    /// its own (`cargo check` takes a moment); one at a time per file.
    fn check_rust(&mut self, uri: String) {
        {
            let mut all = self.rust.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let d = all.entry(uri.clone()).or_default();
            if d.running {
                d.again = true;
                return;
            }
            d.running = true;
        }
        let rust = self.rust.clone();
        std::thread::spawn(move || loop {
            let path = uri_to_path(&uri);
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let cargo: Vec<J> = match crate::native::check(&path) {
                Ok(notes) => notes
                    .iter()
                    .map(|n| {
                        let col = |line: u32, c: u32| {
                            let src = text.lines().nth(line.saturating_sub(1) as usize).unwrap_or("");
                            utf16_col(src, c as usize)
                        };
                        let range = if n.end_line == n.line && n.end_col == n.col {
                            diag_range(&text, n.line, "")
                        } else {
                            json!({
                                "start": { "line": n.line.saturating_sub(1), "character": col(n.line, n.col) },
                                "end": { "line": n.end_line.saturating_sub(1), "character": col(n.end_line, n.end_col) }
                            })
                        };
                        json!({ "range": range, "severity": if n.error { 1 } else { 2 }, "source": "rustc", "message": n.message })
                    })
                    .collect(),
                Err(e) => vec![json!({ "range": diag_range(&text, 1, ""), "severity": 3, "source": "quillo", "message": e })],
            };
            let mut all = rust.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(d) = all.get_mut(&uri) else { return };
            d.cargo = cargo;
            publish_rust(&uri, d);
            if !std::mem::take(&mut d.again) {
                d.running = false;
                return;
            }
        });
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
        // The name as used on this line, or defined on it: the same word
        // elsewhere (text in markup, `<h1>Notes</h1>`, beside `<Notes/>`)
        // is not it.
        let here = |l: u32| l as usize == line + 1;
        let r = doc
            .analysis
            .references
            .iter()
            // A private name is found by what was written (branch public-private).
            .find(|r| (r.name == word || crate::privacy::private_name(&r.name) == Some(word.as_str())) && here(r.line) && r.file == file)
            .cloned()
            .or_else(|| {
                doc.analysis.symbols.iter().find(|s| (s.name == word || crate::privacy::private_name(&s.name) == Some(word.as_str())) && here(s.line) && s.file == file).map(|s| check::Reference {
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
        // In a .qwer file: what Quillo sees of an exported function.
        let (uri, _, _) = Self::position(params);
        if uri.ends_with(".qwer") {
            let Some((_, r)) = self.reference_at(params) else { return J::Null };
            let (sig, doc) = r.detail.split_once("\n\n").unwrap_or((&r.detail, ""));
            return json!({ "contents": { "kind": "markdown", "value": format!("```quillo\n{}\n```\n{}\n\n*Exported to the programs that `use` this file.*", sig, doc) } });
        }
        if let Some(h) = self.use_hover(params) {
            return h;
        }
        if let Some(h) = self.class_hover(params) {
            return h;
        }
        if let Some((_, r)) = self.reference_at(params) {
            return json!({ "contents": { "kind": "markdown", "value": format!("```quillo\n{}\n```", private_detail(&r.name, &r.detail)) } });
        }
        let (uri, line, col) = Self::position(params);
        if let Some((w, _)) = self.docs.get(&uri).and_then(|d| word_at(&d.text, line, col)) {
            if let Some(text) = keyword_doc(&w) {
                return json!({ "contents": { "kind": "markdown", "value": text } });
            }
        }
        J::Null
    }

    /// What the cursor is on in a `use` line: the file it names (no name),
    /// or a name it takes and the name it has here (branch use-editor).
    fn use_here(&self, params: &J) -> Option<(UseLine, PathBuf, Option<(String, String)>)> {
        let (uri, line, col) = Self::position(params);
        if !uri.ends_with(".qwe") {
            return None;
        }
        let doc = self.docs.get(&uri)?;
        let src = doc.text.lines().nth(line)?;
        let u = use_line(src)?;
        let full = use_base(&uri_to_path(&uri)).join(&u.path);
        let at = char_col(src, col);
        if at >= u.at.0 && at <= u.at.1 {
            return Some((u, full, None));
        }
        let (w, _) = word_at(&doc.text, line, col)?;
        let taken = u.names.iter().find(|(n, a, _)| *n == w || *a == w).map(|(n, a, _)| (n.clone(), a.clone()))?;
        Some((u, full, Some(taken)))
    }

    /// A `.qwer` file's exported functions, as open in the editor or saved.
    fn exports_of(&self, full: &Path) -> Vec<crate::native::Export> {
        crate::native::parse(&self.text_of(&full.display().to_string())).exports
    }

    /// On a `use` line: the functions a `.qwer` file gives (those the line
    /// takes), or one of them with the name it has here; a `.qwe` file's
    /// path (branch use-editor).
    fn use_hover(&self, params: &J) -> Option<J> {
        let (u, full, taken) = self.use_here(params)?;
        let short = full.file_name().map_or(String::new(), |f| f.to_string_lossy().to_string());
        let text = if !full.exists() {
            format!("`{}`: no such file", u.path)
        } else if let Some((name, alias)) = taken {
            let e = self.exports_of(&full).into_iter().find(|e| e.name == name)?;
            let called = if alias != name { format!(", called `{}` here", alias) } else { String::new() };
            let doc = if e.doc.is_empty() { String::new() } else { format!("{}\n\n", e.doc) };
            format!("```quillo\n{}\n```\n{}*From {}{}.*", check::native_signature(&crate::native::Export { name: alias, ..e }), doc, short, called)
        } else if u.path.ends_with(".qwer") {
            let sigs: Vec<String> = self
                .exports_of(&full)
                .iter()
                .filter(|e| u.names.is_empty() || u.names.iter().any(|(n, _, _)| *n == e.name))
                .map(|e| match u.names.iter().find(|(n, a, _)| *n == e.name && a != n) {
                    Some((_, a, _)) => check::native_signature(&crate::native::Export { name: a.clone(), ..e.clone() }),
                    None => check::native_signature(e),
                })
                .collect();
            format!("`{}`: Rust, {}\n```quillo\n{}\n```", u.path, if u.names.is_empty() { "every function it exports" } else { "the functions taken" }, sigs.join("\n"))
        } else {
            format!("`{}`: run once, its public names this file's to use", u.path)
        };
        Some(json!({ "contents": { "kind": "markdown", "value": text } }))
    }

    /// On a `use` line: the file it names, or where a `.qwer` file defines
    /// a name the line takes (branch use-editor).
    fn use_definition(&self, params: &J) -> Option<J> {
        let (_, full, taken) = self.use_here(params)?;
        if !full.exists() {
            return None;
        }
        let file = full.display().to_string();
        let Some((name, _)) = taken else { return Some(self.location(&file, 0, 0, 0)) };
        let e = self.exports_of(&full).into_iter().find(|e| e.name == name)?;
        let l = e.line.saturating_sub(1) as usize;
        let text = self.text_of(&file);
        let (a, b) = find_word(text.lines().nth(l).unwrap_or(""), &name).unwrap_or((0, 0));
        Some(self.location(&file, l, a, b))
    }

    /// On a `use` line: the program's files where a path goes, and a
    /// `.qwer` file's functions where names go (branch use-editor).
    fn use_completion(&self, doc: &Doc, uri: &str, line: usize, col16: usize) -> Option<J> {
        let src = doc.text.lines().nth(line)?;
        let chars: Vec<char> = src.chars().collect();
        let col = char_col(src, col16).min(chars.len());
        let before: String = chars[..col].iter().collect();
        let words: Vec<&str> = before.split(|c: char| c.is_whitespace() || c == ',').filter(|w| !w.is_empty()).collect();
        if words.first() != Some(&"use") || !before.trim_start()[3..].starts_with(char::is_whitespace) {
            return None;
        }
        let mut a = col;
        while a > 0 && (chars[a - 1].is_alphanumeric() || "-._~%/\\".contains(chars[a - 1])) {
            a -= 1;
        }
        let typed = a < col;
        // The word before the one being typed.
        let prev = words.get(words.len() - 1 - typed as usize).copied().unwrap_or("");
        let quoted = a > 0 && chars[a - 1] == '"';
        let from_at = words.iter().position(|w| *w == "from");
        let names = from_at.is_some_and(|f| f > 1 && words[1] != "*");
        if prev == "*" {
            return Some(json!([{ "label": "as", "kind": 14 }, { "label": "from", "kind": 14 }]));
        }
        if prev == "as" {
            return Some(json!([]));
        }
        // What `with` takes for a browser library, then `from` (branch library-hints).
        if words.contains(&"with") && from_at.is_none() {
            let head: String = chars[..a].iter().collect();
            let head = head.trim_end();
            if head.ends_with("with") || head.ends_with(',') {
                return Some(json!([{ "label": "module", "kind": 14 }, { "label": "preload", "kind": 14 }, { "label": "preconnect", "kind": 14 }]));
            }
            if matches!(prev, "module" | "preload" | "preconnect") {
                return Some(json!([{ "label": "from", "kind": 14 }]));
            }
        }
        let path_here = quoted || prev == "from" || (prev == "use" && !src[before.len()..].contains(" from "));
        if !path_here {
            // A name: the functions of the file after `from`, not yet taken.
            // With no name yet, the line is not a `use` of names: its file is read alone.
            let from = &src[src.find(" from ")?..];
            let qwer = |u: &UseLine| u.path.ends_with(".qwer");
            let u = use_line(src).filter(qwer).or_else(|| use_line(&format!("use x{}", from)).filter(qwer))?;
            let full = use_base(&uri_to_path(uri)).join(&u.path);
            let items: Vec<J> = self
                .exports_of(&full)
                .iter()
                .filter(|e| !u.names.iter().any(|(n, _, c)| *n == e.name && *c != a))
                .map(|e| json!({ "label": e.name, "kind": 3, "detail": check::native_signature(e), "documentation": e.doc }))
                .collect();
            return Some(json!(items));
        }
        let base = use_base(&uri_to_path(uri));
        let here = uri_to_path(uri).canonicalize().ok();
        let mut files = Vec::new();
        let mut todo = vec![(base.clone(), 0)];
        while let Some((dir, depth)) = todo.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for e in entries.flatten() {
                let p = e.path();
                let name = e.file_name().to_string_lossy().to_string();
                if p.is_dir() {
                    if depth < 4 && !name.starts_with('.') && name != "target" && name != "node_modules" {
                        todo.push((p, depth + 1));
                    }
                } else if (name.ends_with(".qwer") || !names && name.ends_with(".qwe")) && p.canonicalize().ok() != here && files.len() < 500 {
                    files.push(p.strip_prefix(&base).unwrap_or(&p).to_string_lossy().replace('\\', "/"));
                }
            }
        }
        files.sort();
        let range = json!({ "start": { "line": line, "character": utf16_col(src, a) }, "end": { "line": line, "character": col16 } });
        let items: Vec<J> = files
            .iter()
            .map(|f| json!({ "label": f, "kind": 17, "filterText": f, "textEdit": { "range": range, "newText": f } }))
            .collect();
        Some(json!(items))
    }

    fn definition(&self, params: &J) -> J {
        if let Some(found) = self.use_definition(params) {
            return found;
        }
        if let Some(found) = self.class_definition(params) {
            return found;
        }
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
        // A `.qwer` function taken under another name (`use brightness as bright`)
        // is found by its own (branch use-editor).
        let own = || {
            let at = src.find("fn ")? + 3;
            find_word(src, &src[at..].chars().take_while(|c| is_word(*c)).collect::<String>())
        };
        let (a, b) = find_word(src, crate::privacy::private_name(&r.name).unwrap_or(&r.name)).or_else(own).unwrap_or((0, 0));
        json!({
            "uri": path_to_uri(&file),
            "range": {
                "start": { "line": l, "character": utf16_col(src, a) },
                "end": { "line": l, "character": utf16_col(src, b) }
            }
        })
    }

    /// A class in markup (`<span.chip>`, `class="chip"`): the rules the
    /// program's style sheets have for it (branch class-links).
    fn class_definition(&self, params: &J) -> Option<J> {
        let (uri, line, col) = Self::position(params);
        if !uri.ends_with(".qwe") {
            return None;
        }
        let doc = self.docs.get(&uri)?;
        let (class, _) = class_at(doc.text.lines().nth(line)?, col)?;
        let found: Vec<J> = self.rule_spans(&doc.analysis, &class).into_iter().map(|(f, l, a, b)| self.location(&f, l, a, b)).collect();
        (!found.is_empty()).then(|| json!(found))
    }

    /// The text of a file: as open in the editor, else as saved.
    fn text_of(&self, file: &str) -> String {
        match self.docs.iter().find(|(u, _)| uri_to_path(u).display().to_string() == file) {
            Some((_, d)) => d.text.clone(),
            None => std::fs::read_to_string(file).unwrap_or_default(),
        }
    }

    /// A location from character columns on a line (from 0).
    fn location(&self, file: &str, line: usize, a: usize, b: usize) -> J {
        let text = self.text_of(file);
        let src = text.lines().nth(line).unwrap_or("");
        json!({ "uri": path_to_uri(file), "range": {
            "start": { "line": line, "character": utf16_col(src, a) },
            "end": { "line": line, "character": utf16_col(src, b) }
        } })
    }

    /// The class at a position, in markup or in a rule (in a `.css` file
    /// or in a sheet in a `.qwe` file), and the characters it spans.
    fn class_here(&self, params: &J) -> Option<(String, usize, usize)> {
        let (uri, line, col) = Self::position(params);
        let doc = self.docs.get(&uri)?;
        let src = doc.text.lines().nth(line)?;
        let chars: Vec<char> = src.chars().collect();
        let col = char_col(src, col).min(chars.len());
        let (mut a, mut b) = (col, col);
        while a > 0 && is_class_char(chars[a - 1]) {
            a -= 1;
        }
        while b < chars.len() && is_class_char(chars[b]) {
            b += 1;
        }
        if a == b {
            return None;
        }
        let class: String = chars[a..b].iter().collect();
        let dot = a > 0 && chars[a - 1] == '.';
        let here = uri_to_path(&uri).display().to_string();
        let found = if uri.ends_with(".css") {
            dot
        } else if uri.ends_with(".qwe") {
            before_class(&chars[..a])
                || (dot && doc.analysis.classes.iter().any(|(n, f, l)| *n == class && *f == here && *l as usize == line + 1))
        } else {
            false
        };
        found.then_some((class, a, b))
    }

    /// The program a document is part of: a `.qwe` file's own; for a CSS
    /// file, that of an open `.qwe` file whose sheets include it, else the
    /// `main.qwe` in the folder above its `public/`.
    fn with_program<R>(&self, uri: &str, f: impl FnOnce(&Analysis) -> R) -> Option<R> {
        if !uri.ends_with(".css") {
            return self.docs.get(uri).map(|d| f(&d.analysis));
        }
        let path = uri_to_path(uri);
        let file = path.display().to_string();
        if let Some(d) = self.docs.values().find(|d| d.analysis.classes.iter().any(|(_, f, _)| *f == file)) {
            return Some(f(&d.analysis));
        }
        let base = path.ancestors().find(|a| a.file_name().is_some_and(|n| n == "public"))?.parent()?;
        let main = base.join("main.qwe");
        if !main.exists() {
            return None;
        }
        let open: HashMap<PathBuf, String> = self.docs.iter().map(|(u, d)| (uri_to_path(u), d.text.clone())).collect();
        Some(f(&check::check_file_opts(&main, open, self.opts)))
    }

    /// Every `.class` in the rules for it: file, line (from 0) and the
    /// name's characters. A CSS file is read as it is now, open or saved.
    fn rule_spans(&self, a: &Analysis, class: &str) -> Vec<(String, usize, usize, usize)> {
        let mut lines: Vec<(String, usize)> = Vec::new();
        let mut sheets: Vec<&String> = Vec::new();
        for (name, file, line) in &a.classes {
            if file.ends_with(".css") {
                if !sheets.contains(&file) {
                    sheets.push(file);
                }
            } else if name == class {
                lines.push((file.clone(), line.saturating_sub(1) as usize));
            }
        }
        for file in sheets {
            for (name, k) in crate::styles::classes_in(&self.text_of(file)) {
                if name == class {
                    lines.push((file.clone(), k as usize));
                }
            }
        }
        lines.dedup();
        let mut out = Vec::new();
        let mut texts: HashMap<String, String> = HashMap::new();
        for (file, l) in lines {
            let text = texts.entry(file.clone()).or_insert_with(|| self.text_of(&file));
            let src = text.lines().nth(l).unwrap_or("");
            let mut from = 0;
            let chars: Vec<char> = src.chars().collect();
            while let Some((x, y)) = find_class(&chars[from..].iter().collect::<String>(), class) {
                if !out.contains(&(file.clone(), l, from + x, from + y)) {
                    out.push((file.clone(), l, from + x, from + y));
                }
                from += y;
            }
        }
        out
    }

    /// Where the program's markup writes `class`, as written.
    fn use_spans(&self, a: &Analysis, class: &str) -> Vec<(String, usize, usize, usize)> {
        let mut out = Vec::new();
        for file in &a.files {
            for (l, src) in self.text_of(file).lines().enumerate() {
                let chars: Vec<char> = src.chars().collect();
                let mut i = 0;
                while i < chars.len() {
                    if !is_class_char(chars[i]) || (i > 0 && is_class_char(chars[i - 1])) {
                        i += 1;
                        continue;
                    }
                    let mut j = i;
                    while j < chars.len() && is_class_char(chars[j]) {
                        j += 1;
                    }
                    if chars[i..j].iter().copied().eq(class.chars()) && before_class(&chars[..i]) {
                        out.push((file.clone(), l, i, j));
                    }
                    i = j;
                }
            }
        }
        out
    }

    /// A class: its rules, as CSS (branch class-tools).
    fn class_hover(&self, params: &J) -> Option<J> {
        let (uri, line, _) = Self::position(params);
        let (class, a, b) = self.class_here(params)?;
        let rules = self.with_program(&uri, |p| self.rule_spans(p, &class))?;
        let mut shown: Vec<(String, usize)> = rules.iter().map(|(f, l, _, _)| (f.clone(), *l)).collect();
        shown.dedup();
        let mut value = if shown.is_empty() {
            format!("`.{}`: no style sheet of the program has a rule for it", class)
        } else {
            format!("`.{}`: {} rule{}", class, shown.len(), if shown.len() == 1 { "" } else { "s" })
        };
        for (file, l) in shown.iter().take(5) {
            let text = self.text_of(file);
            // The rule, to the `}` that ends it.
            let mut rule = Vec::new();
            let mut depth = 0i32;
            for src in text.lines().skip(*l).take(12) {
                rule.push(src);
                depth += src.matches('{').count() as i32 - src.matches('}').count() as i32;
                if depth <= 0 && src.contains('}') {
                    break;
                }
            }
            let short = Path::new(file).file_name().map_or(file.clone(), |f| f.to_string_lossy().into_owned());
            value.push_str(&format!("\n\n{}:{}\n```css\n{}\n```", short, l + 1, rule.join("\n")));
        }
        if shown.len() > 5 {
            value.push_str(&format!("\n\n… and {} more", shown.len() - 5));
        }
        let src = self.docs.get(&uri)?.text.lines().nth(line).unwrap_or("").to_string();
        Some(json!({
            "contents": { "kind": "markdown", "value": value },
            "range": { "start": { "line": line, "character": utf16_col(&src, a) }, "end": { "line": line, "character": utf16_col(&src, b) } }
        }))
    }

    /// A class's rules (when asked for) and everywhere markup writes it
    /// (branch class-tools).
    fn class_references(&self, params: &J) -> J {
        let (uri, _, _) = Self::position(params);
        let Some((class, _, _)) = self.class_here(params) else { return json!([]) };
        let rules = params["context"]["includeDeclaration"].as_bool().unwrap_or(true);
        let spans = self.with_program(&uri, |p| {
            let mut all = if rules { self.rule_spans(p, &class) } else { Vec::new() };
            all.extend(self.use_spans(p, &class));
            all
        });
        json!(spans.unwrap_or_default().into_iter().map(|(f, l, a, b)| self.location(&f, l, a, b)).collect::<Vec<_>>())
    }

    fn prepare_rename(&self, params: &J) -> J {
        let (uri, line, _) = Self::position(params);
        let Some((class, a, b)) = self.class_here(params) else { return J::Null };
        let src = self.docs.get(&uri).and_then(|d| d.text.lines().nth(line)).unwrap_or("").to_string();
        json!({
            "range": { "start": { "line": line, "character": utf16_col(&src, a) }, "end": { "line": line, "character": utf16_col(&src, b) } },
            "placeholder": class
        })
    }

    /// A class renamed in its rules and in the markup that writes it as
    /// written; one put together (`card-${size}`) is left as it is (branch
    /// class-tools).
    fn rename(&self, params: &J) -> Result<J, String> {
        let (uri, _, _) = Self::position(params);
        let Some((class, _, _)) = self.class_here(params) else { return Err("only a class can be renamed here".into()) };
        let name = params["newName"].as_str().unwrap_or("").trim().trim_start_matches('.');
        if !name.chars().all(is_class_char) || !name.trim_start_matches('-').starts_with(|c: char| c.is_alphabetic() || c == '_') {
            return Err(format!("`{}` is not a class name", name));
        }
        let spans = self
            .with_program(&uri, |p| {
                let mut all = self.rule_spans(p, &class);
                all.extend(self.use_spans(p, &class));
                all
            })
            .unwrap_or_default();
        let mut changes: serde_json::Map<String, J> = serde_json::Map::new();
        for (f, l, a, b) in spans {
            let mut loc = self.location(&f, l, a, b);
            let edits = changes.entry(loc["uri"].as_str().unwrap_or("").to_string()).or_insert_with(|| json!([]));
            edits.as_array_mut().unwrap().push(json!({ "range": loc["range"].take(), "newText": name }));
        }
        Ok(json!({ "changes": changes }))
    }

    fn completion(&self, params: &J) -> J {
        let (uri, line, col) = Self::position(params);
        // Rust or CSS: Quillo's names would only be in the way.
        if !uri.ends_with(".qwe") {
            return json!([]);
        }
        let Some(doc) = self.docs.get(&uri) else { return json!([]) };
        if let Some(items) = self.use_completion(doc, &uri, line, col) {
            return items;
        }
        if let Some(items) = Self::class_completion(doc, line, col) {
            return items;
        }
        if let Some(items) = Self::markup_completion(doc, params, line, col) {
            return items;
        }
        let src = doc.text.lines().nth(line).unwrap_or("");
        let before: String = src.chars().take(char_col(src, col)).collect();
        let mut items = Vec::new();
        // After `db.`, `udp.` or any module's name: its functions.
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
            // Another file's private names are not this file's to use (branch public-private).
            if crate::privacy::private_name(&s.name).is_some() && s.file != file {
                continue;
            }
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
        for m in check::MODULES {
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
            for e in crate::ast::EFFECTS {
                items.push(json!({ "label": e, "kind": 14, "detail": "effect" }));
            }
        }
        json!(items)
    }

    /// After `<span.` or in `class="...`: the classes the program's style
    /// sheets have rules for (branch class-links).
    fn class_completion(doc: &Doc, line: usize, col: usize) -> Option<J> {
        let src = doc.text.lines().nth(line)?;
        let chars: Vec<char> = src.chars().collect();
        let col = char_col(src, col).min(chars.len());
        let mut a = col;
        while a > 0 && is_class_char(chars[a - 1]) {
            a -= 1;
        }
        if !before_class(&chars[..a]) {
            return None;
        }
        let mut seen = std::collections::HashSet::new();
        let items: Vec<J> = doc
            .analysis
            .classes
            .iter()
            .filter(|(name, _, _)| seen.insert(name.clone()))
            .map(|(name, file, l)| {
                let short = Path::new(file).file_name().map_or(file.clone(), |f| f.to_string_lossy().into_owned());
                json!({ "label": name, "kind": 7, "detail": format!("{}:{}", short, l) })
            })
            .collect();
        Some(json!(items))
    }

    /// After `</`: the end tag of the element open there. After a `<` that
    /// starts markup: the program's views and components, and HTML's
    /// elements (branch editor-end-tags).
    fn markup_completion(doc: &Doc, params: &J, line: usize, col: usize) -> Option<J> {
        let at = byte_offset(&doc.text, line, col);
        let head = &doc.text[..at];
        let partial = head.len() - head.trim_end_matches(|c: char| c.is_alphanumeric() || c == '-' || c == '_').len();
        let head = &head[..head.len() - partial];
        let typed = doc.text[at - partial..at].encode_utf16().count();
        let range = json!({ "start": { "line": line, "character": col - typed }, "end": { "line": line, "character": col } });
        if let Some(rest) = head.strip_suffix("</") {
            let Some(open) = crate::parser::open_tags_at_end(rest) else { return Some(json!([])) };
            let o = open.last()?;
            let text = if doc.text[at..].starts_with('>') { o.tag.clone() } else { format!("{}>", o.tag) };
            return Some(json!([{
                "label": format!("</{}>", o.tag),
                "kind": 10,
                "detail": format!("ends <{}> from line {}", o.tag, o.line),
                "filterText": o.tag,
                "preselect": true,
                "textEdit": { "range": range, "newText": text },
            }]));
        }
        // `/` is division or a comment anywhere else.
        if params["context"]["triggerCharacter"].as_str() == Some("/") {
            return Some(json!([]));
        }
        let rest = head.strip_suffix('<')?;
        // `a < b`, `xs[i] <`: a comparison.
        if rest.trim_end().ends_with(|c: char| is_word(c) || c == ')' || c == ']' || c == '<') {
            return None;
        }
        let mut items = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let file = uri_to_path(params["textDocument"]["uri"].as_str().unwrap_or("")).display().to_string();
        for s in &doc.analysis.symbols {
            // Another file's private views are not this file's (branch public-private-editor).
            let theirs = crate::privacy::private_name(&s.name).is_some() && s.file != file;
            if matches!(s.kind, SymbolKind::View | SymbolKind::Component) && !theirs && seen.insert(s.name.clone()) {
                items.push(json!({ "label": s.name, "kind": 7, "detail": s.detail, "sortText": format!("0{}", s.name) }));
            }
        }
        for t in HTML_TAGS {
            items.push(json!({ "label": t, "kind": 10, "detail": "HTML element", "sortText": format!("1{}", t) }));
        }
        Some(json!(items))
    }

    /// `quillo/closeTag`, asked by the editor as `>` or `</` is typed: what
    /// to put at the cursor. After the `>` of a start tag whose element has
    /// no end tag yet, that end tag after the cursor (a snippet,
    /// `$0</li>`); after `</`, the name of the element open there and `>`;
    /// else null (branch editor-end-tags).
    fn close_tag(&self, params: &J) -> J {
        let (uri, line, col) = Self::position(params);
        let Some(doc) = self.docs.get(&uri) else { return J::Null };
        if !uri.ends_with(".qwe") {
            return J::Null;
        }
        let at = byte_offset(&doc.text, line, col);
        let (head, after) = doc.text.split_at(at);
        if let Some(rest) = head.strip_suffix("</") {
            let Some(o) = crate::parser::open_tags_at_end(rest).and_then(|mut o| o.pop()) else { return J::Null };
            return json!(if after.starts_with('>') { o.tag } else { format!("{}>", o.tag) });
        }
        if !head.ends_with('>') {
            return J::Null;
        }
        // A `>` typed again in a start tag that has its end tag already.
        if crate::parser::Parser::new(&doc.text).parse_program().is_ok() {
            return J::Null;
        }
        let Some(o) = crate::parser::open_tags_at_end(head).and_then(|mut o| o.pop()) else { return J::Null };
        if o.end != head.chars().count() || after.trim_start().starts_with(&format!("</{}>", o.tag)) {
            return J::Null;
        }
        json!(format!("$0</{}>", o.tag))
    }

    /// Quick fixes: the change a syntax error comes with, such as the end
    /// tag an element is missing (branch editor-end-tags).
    fn code_actions(&self, params: &J) -> J {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or("");
        let Some(doc) = self.docs.get(uri) else { return json!([]) };
        if !uri.ends_with(".qwe") {
            return json!([]);
        }
        let Err(e) = crate::parser::Parser::new(&doc.text).parse_program() else { return json!([]) };
        let Some(fix) = e.fix else { return json!([]) };
        let l = e.line.saturating_sub(1) as u64;
        let (first, last) = (params["range"]["start"]["line"].as_u64().unwrap_or(0), params["range"]["end"]["line"].as_u64().unwrap_or(0));
        let diags: Vec<J> = params["context"]["diagnostics"]
            .as_array()
            .map(|d| d.iter().filter(|d| d["source"] == "quillo" && d["range"]["start"]["line"].as_u64() == Some(l)).cloned().collect())
            .unwrap_or_default();
        if diags.is_empty() && !(first..=last).contains(&l) {
            return json!([]);
        }
        let fl = fix.line.saturating_sub(1) as usize;
        let src = doc.text.lines().nth(fl).unwrap_or("");
        let range = json!({
            "start": { "line": fl, "character": utf16_col(src, fix.col as usize) },
            "end": { "line": fl, "character": utf16_col(src, (fix.col + fix.len) as usize) }
        });
        json!([{
            "title": fix.title,
            "kind": "quickfix",
            "isPreferred": true,
            "diagnostics": diags,
            "edit": { "changes": { uri: [{ "range": range, "newText": fix.text }] } },
        }])
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
                let (a, b) = find_word(src, crate::privacy::private_name(&s.name).unwrap_or(&s.name)).unwrap_or((0, src.chars().count()));
                let full = json!({ "start": { "line": l, "character": 0 }, "end": { "line": l, "character": utf16_col(src, src.chars().count()) } });
                let sel = json!({ "start": { "line": l, "character": utf16_col(src, a) }, "end": { "line": l, "character": utf16_col(src, b) } });
                json!({ "name": s.name, "detail": private_detail(&s.name, &s.detail), "kind": symbol_kind(s.kind), "range": full, "selectionRange": sel })
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

/// A private name's signature says so: `private fn page_of(...)` (branch
/// public-private-editor).
fn private_detail(name: &str, detail: &str) -> String {
    match crate::privacy::private_name(name) {
        Some(_) if !detail.starts_with("param ") => format!("private {}", detail),
        _ => detail.to_string(),
    }
}

fn keyword_doc(w: &str) -> Option<String> {
    let text = match w {
        "route" => "`route METHOD /path/:param ? { query } { request pattern } = expr`\n\nDeclares an HTTP route. Routes are tried in order; the first whose method, path and patterns match handles the request.",
        "component" => "`component Name(props) { state count = 0 ... <html/> }`\n\nA view that also runs in the browser: rendered on the server, then interactive. Assigning to its state re-renders it. Handlers: `@click={() => { count += 1 }}`.",
        "server" => "`server fn name(params) = ...`\n\nA function the browser can call. Component code awaits it automatically.",
        "view" => "`view Name(params, children) = <html/>`\n\nA function returning HTML, used as `<Name .../>`.",
        "reply" => "`reply status [body]`\n\nEnds the current request immediately with that response, from anywhere.",
        "with" => "`fn f() with db, http = ...`\n\nDeclares the effects a function may use: db, http, fs, env, time, random.\n\n`use Chart with module, preload from \"...\"`\n\nHow a page takes a browser library: `module` for an ES module, kept in the runtime and not on `window`; `preload` fetches it ahead; `preconnect` only connects to its host.",
        "match" => "`match value { pattern [if guard] => result ... }`",
        "use" => "`use lib/auth.qwe` (or `use lib/auth`, or `use \"lib/auth.qwe\"`) runs another file once: its public names are this file's to use.\n\n`use cache_get as redis_get, cache_set from redis.qwer` takes only those functions of a Rust file, each under the name after `as`; `use * from redis.qwer` takes them all.",
        "mount" => "`mount /prefix = \"app.qwe\"` serves another program under a path prefix, with its own globals and database.",
        "state" => "`state count = 0` in a component: state, set on the first render and kept between renders (`~count = 0` is the short form). Assigning to `count` re-renders the component.",
        "private" => "`private fn helper() = ...` (also `view`, `component`, a top-level `let`)\n\nA name for this file only: other files cannot use it, and each file may have its own `helper`. Not for a `server fn` or a `type`.",
        "public" => "`public fn name() = ...`\n\nThe default, said out loud: every file of the program may use the name.",
        "type" => "`type User = { name: str, role: \"admin\" | \"member\" = \"member\" }`\n\nA record (or another name for a type). `User({ ... })` makes one, or gives an error value; in annotations and patterns it is checked field by field.",
        "service" => "`service billing = \"billing.qwe\"` (or `service /shop = \"shop.qwe\"`): a part of the app that runs here or as a server of its own; its `server fn`s are called as `billing.invoice(...)`.",
        _ => return None,
    };
    Some(text.to_string())
}
