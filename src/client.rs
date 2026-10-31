//! Compiles components, and what they use, to JavaScript for the browser.
//!
//! The bundle served at `/_quillo/quillo.js` is the runtime (`client/runtime.js`)
//! followed by the compiled program: every `component`, the functions,
//! views and constants they reference, and a stub for each `server fn`.
//! Code that cannot run in a browser (`request`, `db`, a global the server
//! changes, ...) is a load-time error that names the component.
//!
//! A page gets a bundle of its own: the components it was rendered with,
//! those they show, and what they use (`bundle_for`), at
//! `/_quillo/b/<hash>.js`, kept by the browser for good since its address
//! changes with its content. The runtime is a file of its own beside it,
//! `/_quillo/r/<hash>.js`, the same for every page (`runtime_file`). A page with
//! no components loads no script, and one with components loads it when it
//! is needed (`island_loader` in server.rs, `island.rs`). The runtime resumes
//! each component on the server's elements (`Inst.adopt` in runtime.js):
//! it renders once from the state the server sent and takes the DOM as it
//! is, making it again only where it differs.

use crate::ast::*;
use crate::error::QuilloError;
use crate::interp::{not_sendable, Interp};
use crate::value::*;
use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::sync::Arc;

const RUNTIME: &str = include_str!("client/runtime.js");

/// Development (`--watch`, the debugger): a page's bundle is a loader for
/// the runtime and each component, view and function in a file of its own,
/// named after the `.qwe` file it is in, with a source map to its lines.
pub static DEV: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Builtins that also exist in the browser.
const CLIENT_BUILTINS: &[&str] = &[
    "print", "str", "int", "float", "dec", "len", "type_of", "upper", "lower", "trim", "filled", "split", "join", "contains",
    "starts_with", "ends_with", "replace", "slice", "map", "filter", "find", "any", "all", "reduce", "sum", "min",
    "max", "sort", "sort_by", "reverse", "first", "last", "push", "range", "enumerate", "keys", "values", "get", "has",
    "set", "remove", "merge", "err", "is_err", "unwrap", "raw", "escape", "now", "random", "random_int", "uuid", "abs",
    "round", "floor", "ceil", "sqrt", "pow", "unique", "flatten", "zip", "take", "drop", "count", "group_by",
    "index_of", "repeat", "format_number", "url", "url_encode", "navigate", "markdown", "css", "sql", "parallel", "every", "listen", "receive", "__record", "__merge", "__concat", "t", "locale", "js", "js_new", "highlight", "ask", "notify",
    // FEEDBACK13.md, Q-091; branch feedback13-fixes.
    "waiting",
    // In the browser, `defer(x)` is `x` (src/defer.rs; branch defer).
    "defer",
];

pub fn bundle(it: &Interp) -> Result<String, QuilloError> {
    let (c, boots) = compiled(it, None)?;
    Ok(bundle_from(it, &c, boots))
}

/// The whole program's bundle and its runtime file from one compile. Made
/// apart, each compiled the program again: two thirds of a program's load
/// (FEEDBACK15.md, Q-098; branch dev-maps-once).
pub fn bundle_and_runtime(it: &Interp) -> Result<(String, Option<RuntimeFile>), QuilloError> {
    let (c, boots) = compiled(it, None)?;
    let js = bundle_from(it, &c, boots);
    Ok((js, (!c.dev).then(|| runtime_from(it, &c, boots))))
}

fn bundle_from(it: &Interp, c: &Compiler, boots: bool) -> String {
    if c.dev {
        return dev::bundle(it, c, boots);
    }
    assemble("(function () {", &program_text(it, c, boots, true), "})();")
}

/// The program compiled for the browser, from `roots` (all its components
/// when none), and whether it has any.
fn compiled<'a>(it: &'a Interp, roots: Option<&[String]>) -> Result<(Compiler<'a>, bool), QuilloError> {
    let mut c = Compiler::new(it);
    let mut names: Vec<(String, Arc<FnDef>)> = Vec::new();
    for (name, v) in scope_vars(&it.globals) {
        if let Value::Func(f) = v {
            if f.def.component && roots.map_or(true, |r| r.contains(&name)) {
                names.push((name, f.def.clone()));
            }
        }
    }
    // In an order that depends only on which they are (FEEDBACK7.md, Q-062;
    // branch feedback7-fixes).
    names.sort_by(|(a, x), (b, y)| (&x.file, x.line, a).cmp(&(&y.file, y.line, b)));
    for (name, _) in &names {
        c.global(name, 0)?;
    }
    Ok((c, !names.is_empty()))
}

/// The components a compile made: the roots and every one they show.
fn components_of(it: &Interp, c: &Compiler) -> Vec<String> {
    let mut out: Vec<String> = c
        .globals
        .keys()
        .filter(|k| matches!(lookup(&it.globals, k), Some(Value::Func(f)) if f.def.component))
        .cloned()
        .collect();
    out.sort();
    out
}

/// The styles of the components and views a compile made (FEEDBACK8.md,
/// Q-068; branch feedback8-fixes): a page can come to show them.
fn styles_of(it: &Interp, c: &Compiler) -> Vec<String> {
    let mut out: Vec<String> = c
        .globals
        .keys()
        .filter_map(|k| match lookup(&it.globals, k) {
            Some(Value::Func(f)) if matches!(&f.def.body, Expr::Block(ss) if ss.iter().any(|s| style_of(s).is_some())) => {
                Some(style_id(f.def.file.as_deref(), &f.def.name))
            }
            _ => None,
        })
        .collect();
    out.sort();
    out
}

/// A browser library a program uses: `client "...js" use Chart` (branch
/// js-libraries). `global` is what the script defines on the page, `src` where
/// it is, `order` its place among the program's (a script after the one it
/// builds on), `hint` what a page does ahead of it, `export` what it takes from an
/// ES module (a name, `default` or `*`), which then goes by `global` in the runtime
/// and not on the page (branch library-hints).
#[derive(Clone, Debug)]
pub struct JsLib {
    pub global: String,
    pub src: String,
    pub order: usize,
    pub hint: Option<LibHint>,
    pub export: Option<String>,
}

/// The scripts of the libraries a compile's code uses, in the order the program
/// names them: a page loads them before its bundle (branch js-libraries).
fn libs_of(it: &Interp, c: &Compiler) -> Vec<String> {
    let mut libs: Vec<&JsLib> = c.globals.keys().filter_map(|k| it.js_libs.get(k)).filter(|l| l.export.is_none()).collect();
    libs.sort_by_key(|l| l.order);
    let mut out: Vec<String> = Vec::new();
    for l in libs {
        let src = script_src(it, &l.src);
        if !out.contains(&src) {
            out.push(src);
        }
    }
    out
}

/// Every library's script, for the whole program's bundle.
pub fn all_libs(it: &Interp) -> Vec<String> {
    let mut libs: Vec<&JsLib> = it.js_libs.values().filter(|l| l.export.is_none()).collect();
    libs.sort_by_key(|l| l.order);
    let mut out: Vec<String> = Vec::new();
    for l in libs {
        let src = script_src(it, &l.src);
        if !out.contains(&src) {
            out.push(src);
        }
    }
    out
}

/// The `<link>`s for a page's `<head>`, for those of these libraries named `with
/// preload` or `with preconnect`. No `crossorigin` for a script: the page's loader
/// makes a plain `<script>`, and a preload that differs from it is fetched twice. A
/// module is fetched with CORS: `modulepreload` is, and a connection needs saying so
/// (branch library-hints).
fn hint_links<'a>(it: &Interp, libs: impl Iterator<Item = &'a JsLib>) -> String {
    let mut libs: Vec<&JsLib> = libs.collect();
    libs.sort_by_key(|l| l.order);
    let mut out = String::new();
    for l in libs {
        let module = l.export.is_some();
        let link = match (l.hint, crate::ast::script_origin(&l.src)) {
            (Some(LibHint::Preload), _) if module => format!("<link rel=\"modulepreload\" href=\"{}\">", crate::interp::escape(&script_src(it, &l.src))),
            (Some(LibHint::Preload), _) => format!("<link rel=\"preload\" as=\"script\" href=\"{}\">", crate::interp::escape(&script_src(it, &l.src))),
            (Some(LibHint::Preconnect), Some(origin)) => {
                format!("<link rel=\"preconnect\" href=\"{}\"{}>", crate::interp::escape(origin), if module { " crossorigin" } else { "" })
            }
            _ => continue,
        };
        if !out.contains(&link) {
            out.push_str(&link);
        }
    }
    out
}

/// Every library's `<link>`s, for the whole program's bundle.
pub fn all_hints(it: &Interp) -> String {
    hint_links(it, it.js_libs.values())
}

/// A script's address as the page loads it: an address as written, a path from
/// the program's own (`/assets/leaflet.js`, under its base).
fn script_src(it: &Interp, src: &str) -> String {
    if crate::ast::is_address(src) {
        src.to_string()
    } else {
        format!("{}/{}", it.base, src.trim_start_matches("./").trim_start_matches('/'))
    }
}

/// What starts a page: `boot`, once the modules its code uses are in, so that a
/// component never runs ahead of one (branch library-hints).
fn boot_line(it: &Interp, c: &Compiler) -> String {
    let start = "if (document.readyState === \"loading\") document.addEventListener(\"DOMContentLoaded\", boot); else boot();";
    let mut mods: Vec<(&String, &JsLib)> = c.globals.keys().filter_map(|k| it.js_libs.get_key_value(k)).collect();
    mods.retain(|(_, l)| l.export.is_some());
    mods.sort_by_key(|(_, l)| l.order);
    if mods.is_empty() {
        return format!("{}\n", start);
    }
    let list: Vec<String> = mods
        .iter()
        .map(|(name, l)| format!("[{}, {}, {}]", js_str(name), js_str(l.export.as_deref().unwrap_or_default()), js_str(&script_src(it, &l.src))))
        .collect();
    format!("W.imports([{}]).then(() => {{ {} }});\n", list.join(", "), start)
}

/// The compiled program as one script: `BASE` (when `with_base`), the
/// markup blocks, the definitions, the `server fn` stubs, and the boot.
fn program_text(it: &Interp, c: &Compiler, boots: bool, with_base: bool) -> String {
    let mut program = String::new();
    if with_base && !it.base.is_empty() {
        let _ = writeln!(program, "BASE = {};", js_str(&it.base));
    }
    for b in &c.blocks {
        program.push_str(b);
        program.push('\n');
    }
    for d in c.defs.iter().chain(c.view_defs.iter()) {
        program.push_str(d);
        program.push('\n');
    }
    if !it.server_fns.is_empty() {
        program.push_str("const server = {};\n");
        for name in it.server_fns.iter() {
            let idem = matches!(lookup(&it.globals, name), Some(Value::Func(c)) if crate::idempotency::used_by(&c.def));
            let _ = writeln!(program, "server[{}] = W.server({}{});", js_str(name), js_str(name), if idem { ", true" } else { "" });
        }
        program.push_str("window.quillo = { server, components: C, rpc };\n");
    }
    if boots {
        program.push_str(&boot_line(it, c));
    }
    program
}

/// A page's bundle: the components it was rendered with, every component
/// they show (all of which it can come to show without loading more), and
/// what they use. Two pages with the same components get the same bundle,
/// whichever of them each rendered (FEEDBACK7.md, Q-062; branch
/// feedback7-fixes). With the program's runtime file, the bundle is the
/// program's code alone, which takes the runtime from it (Q-064).
pub fn bundle_for(it: &Interp, roots: &[String], rt: Option<&RuntimeFile>) -> Result<PageBundle, QuilloError> {
    let mut c = compiled(it, Some(roots))?;
    let set = components_of(it, &c.0);
    let mut sorted_roots = roots.to_vec();
    sorted_roots.sort();
    sorted_roots.dedup();
    if set != sorted_roots {
        c = compiled(it, Some(&set))?;
    }
    let (c, boots) = c;
    let styles = styles_of(it, &c);
    let libs = libs_of(it, &c);
    let hints = hint_links(it, c.globals.keys().filter_map(|k| it.js_libs.get(k)));
    if c.dev {
        return Ok(PageBundle { js: dev::bundle(it, &c, boots), set, split: false, styles, libs, hints });
    }
    if let Some(rt) = rt {
        if let Some(js) = rt.wrap(&program_text(it, &c, boots, false)) {
            return Ok(PageBundle { js, set, split: true, styles, libs, hints });
        }
    }
    Ok(PageBundle { js: assemble("(function () {", &program_text(it, &c, boots, true), "})();"), set, split: false, styles, libs, hints })
}

pub struct PageBundle {
    pub js: String,
    /// The components it holds, sorted: its key.
    pub set: Vec<String>,
    /// It takes the runtime from the runtime file.
    pub split: bool,
    /// The styles of the components and views it holds.
    pub styles: Vec<String>,
    /// The scripts of the libraries its code uses, loaded before it.
    pub libs: Vec<String>,
    /// The `<link>`s of those named `with preload` or `with preconnect`.
    pub hints: String,
}

/// A page's bundle as the page names it.
#[derive(Clone)]
pub struct PageRef {
    pub hash: String,
    /// It takes the runtime from the runtime file.
    pub split: bool,
    pub styles: Vec<String>,
    pub libs: Vec<String>,
    pub hints: String,
}

/// The runtime as a file of its own, `/_quillo/r/<hash>.js`, which every
/// page's bundle shares, so the browser downloads it once rather than once
/// for each set of components (FEEDBACK7.md, Q-064; branch feedback7-fixes).
/// It holds what the whole program uses, and hands the bundles the names
/// they take from it as `window.__quillo_rt`; inside its function, they do
/// not meet the page's own scripts' names.
pub struct RuntimeFile {
    pub hash: String,
    pub js: Arc<String>,
    keep: Vec<bool>,
    exported: HashSet<String>,
}

/// The program's runtime file, or none in development (whose bundles load
/// the runtime as it is written).
pub fn runtime_file(it: &Interp) -> Result<Option<RuntimeFile>, QuilloError> {
    let (c, boots) = compiled(it, None)?;
    Ok((!c.dev).then(|| runtime_from(it, &c, boots)))
}

fn runtime_from(it: &Interp, c: &Compiler, boots: bool) -> RuntimeFile {
    let program = program_text(it, c, boots, false);
    let rt = runtime_units();
    let keep = shake(rt, &program);
    let wanted: HashSet<String> = names_in(&program).into_iter().collect();
    let mut exported: Vec<String> = rt
        .by_name
        .iter()
        .filter(|(n, &i)| keep[i] && !n.contains('.') && wanted.contains(*n))
        .map(|(n, _)| n.clone())
        .collect();
    exported.sort();
    let mut js = String::from("(function () {\n");
    minify_into(&rt.preamble, &mut js);
    for (i, (_, text)) in rt.units.iter().enumerate() {
        if keep[i] {
            minify_into(text, &mut js);
        }
    }
    if !it.base.is_empty() {
        let _ = writeln!(js, "BASE = {};", js_str(&it.base));
    }
    let _ = writeln!(js, "window.__quillo_rt = {{ {} }};", exported.join(", "));
    js.push_str("})();\n");
    let hash = hash_text(&js);
    RuntimeFile { hash, js: Arc::new(js), keep, exported: exported.into_iter().collect() }
}

impl RuntimeFile {
    /// A page's program as a bundle that takes the runtime from this file,
    /// or none if it needs something the file does not have (it then
    /// carries its own runtime).
    fn wrap(&self, program: &str) -> Option<String> {
        let rt = runtime_units();
        let need = shake(rt, program);
        if need.iter().zip(&self.keep).any(|(n, k)| *n && !*k) {
            return None;
        }
        // The program's own top-level names are its own.
        let declared: HashSet<String> = program
            .lines()
            .filter_map(|l| ["const ", "let ", "function ", "class ", "async function "].iter().find_map(|p| l.strip_prefix(p)))
            .map(|r| r.chars().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$').collect())
            .collect();
        let mut used: Vec<String> = names_in(program)
            .into_iter()
            .filter(|n| !n.contains('.') && rt.by_name.contains_key(n) && !declared.contains(n))
            .collect();
        used.sort();
        used.dedup();
        // A name it does not have, or one the program assigns (which a copy
        // would not pass on), and the bundle carries its own runtime.
        if used.iter().any(|n| !self.exported.contains(n) || assigns(program, n)) {
            return None;
        }
        let mut out = format!("(function ($rt) {{\n\"use strict\";\nconst {{ {} }} = $rt;\n", used.join(", "));
        minify_into(program, &mut out);
        out.push_str("})(window.__quillo_rt);\n");
        Some(out)
    }
}

/// Whether `js` assigns to the variable `name` (`name = `, `name += `...).
fn assigns(js: &str, name: &str) -> bool {
    let ident = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    js.match_indices(name).any(|(i, _)| {
        let before = js[..i].chars().next_back();
        if before.map_or(false, |c| ident(c) || c == '.') {
            return false;
        }
        let rest = &js[i + name.len()..];
        if rest.chars().next().map_or(false, ident) {
            return false;
        }
        // `=`, `+=`, `??=`..., not `==`, `=>`, `<=`, `!=`.
        let op = rest.trim_start().trim_start_matches(|c| "+-*/%&|^?".contains(c));
        op.starts_with('=') && !op.starts_with("==") && !op.starts_with("=>")
    })
}

pub fn hash_text(js: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    js.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// The bundles made for pages, by the set of components they hold, and the
/// runtime file they share. At most `KEPT` sets are kept, the one used
/// longest ago going first: a page that names a bundle no longer kept is
/// sent the whole program's, and one asked for again is made again
/// (FEEDBACK7.md, Q-063; branch feedback7-fixes).
#[derive(Default)]
pub struct Bundles {
    pub runtime: Option<Arc<RuntimeFile>>,
    /// The components a page rendered, sorted and joined, to its set's key.
    by_roots: HashMap<String, String>,
    by_set: HashMap<String, Made>,
    tick: u64,
}

struct Made {
    page: PageRef,
    js: Arc<String>,
    used: u64,
}

pub const KEPT: usize = 256;

impl Bundles {
    pub fn hash_for_roots(&mut self, roots: &str) -> Option<PageRef> {
        let set = self.by_roots.get(roots)?.clone();
        self.tick += 1;
        let m = self.by_set.get_mut(&set)?;
        m.used = self.tick;
        Some(m.page.clone())
    }

    pub fn keep(&mut self, roots: String, b: PageBundle) -> PageRef {
        let set = b.set.join(",");
        self.tick += 1;
        if self.by_roots.len() > 4 * KEPT {
            self.by_roots.clear();
        }
        self.by_roots.insert(roots, set.clone());
        if let Some(m) = self.by_set.get_mut(&set) {
            m.used = self.tick;
            return m.page.clone();
        }
        let page = PageRef { hash: hash_text(&b.js), split: b.split, styles: b.styles, libs: b.libs, hints: b.hints };
        self.by_set.insert(set, Made { page: page.clone(), js: Arc::new(b.js), used: self.tick });
        crate::metrics::BUNDLES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        while self.by_set.len() > KEPT {
            let Some(old) = self.by_set.iter().min_by_key(|(_, m)| m.used).map(|(k, _)| k.clone()) else { break };
            self.by_set.remove(&old);
            crate::metrics::BUNDLES.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        }
        page
    }

    pub fn by_hash(&self, hash: &str) -> Option<Arc<String>> {
        self.by_set.values().find(|m| m.page.hash == hash).map(|m| m.js.clone())
    }

    pub fn len(&self) -> usize {
        self.by_set.len()
    }
}

impl Drop for Bundles {
    fn drop(&mut self) {
        crate::metrics::BUNDLES.fetch_sub(self.by_set.len() as i64, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Where a statement's line is written into compiled code in development:
/// `\u{1}12\u{1}`. Strings in compiled code are JSON, so the character is
/// never in them as it is.
const MARK: char = '\u{1}';

/// Development bundles (see `DEV`): the runtime as it is written, the
/// program's pieces in files of their own, and a loader that adds them to
/// the page in order. They are plain scripts, not modules or closures, so
/// the names the runtime declares are seen by every file after it.
mod dev {
    use super::*;
    use std::sync::Mutex;

    /// The files of the bundles made, by their hash: path → script.
    static FILES: Mutex<Option<HashMap<String, Arc<HashMap<String, String>>>>> = Mutex::new(None);

    /// A file of a development bundle (`/_quillo/src/<hash>/<path>`).
    pub fn file(hash: &str, path: &str) -> Option<String> {
        FILES.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref()?.get(hash)?.get(path).cloned()
    }

    /// A file being made: its path and code (with line marks), and where
    /// the code comes from.
    struct Piece {
        path: String,
        code: String,
        source: Option<Arc<str>>,
    }

    pub fn bundle(it: &Interp, c: &Compiler, boots: bool) -> String {
        let mut pieces: Vec<Piece> = Vec::new();
        let mut prelude = String::from("DEV = true;\n");
        if !it.base.is_empty() {
            let _ = writeln!(prelude, "BASE = {};", js_str(&it.base));
        }
        for b in &c.blocks {
            prelude.push_str(b);
            prelude.push('\n');
        }
        pieces.push(Piece { path: "program.js".into(), code: prelude, source: None });
        let base = std::fs::canonicalize(&*it.base_dir).unwrap_or_else(|_| it.base_dir.to_path_buf());
        // Each function the program wrote, in a file of its own (in the order
        // they were compiled, which is the order they need); what else the
        // program defines goes in files between them.
        let mut by_name: HashMap<String, usize> = HashMap::new();
        for d in &c.defs {
            let def = defined(d).and_then(|n| match lookup(&it.globals, &n) {
                Some(Value::Func(f)) if f.def.file.is_some() && !f.def.server => Some((n, f.def.clone())),
                _ => None,
            });
            match def {
                Some((name, def)) => {
                    let file = def.file.clone().unwrap();
                    let rel = std::path::Path::new(&*file).strip_prefix(&base).map(|p| p.to_path_buf()).unwrap_or_else(|_| {
                        std::path::Path::new(&*file).file_name().map(std::path::PathBuf::from).unwrap_or_default()
                    });
                    let path = format!("{}/{}.js", url_path(&rel.to_string_lossy()), url_path(&name));
                    by_name.insert(name, pieces.len());
                    pieces.push(Piece { path, code: format!("{}\n", d), source: Some(file) });
                }
                None => match pieces.last_mut() {
                    Some(p) if p.source.is_none() => {
                        p.code.push_str(d);
                        p.code.push('\n');
                    }
                    _ => {
                        let n = pieces.len();
                        pieces.push(Piece { path: format!("program-{}.js", n), code: format!("{}\n", d), source: None });
                    }
                },
            }
        }
        // A view's `<Name/>` form goes with it.
        for v in &c.view_defs {
            let at = v.strip_prefix("W.view(").and_then(defined).and_then(|n| by_name.get(&n).copied());
            match at {
                Some(i) => {
                    pieces[i].code.push_str(v);
                    pieces[i].code.push('\n');
                }
                None => {
                    let n = pieces.len();
                    pieces.push(Piece { path: format!("program-{}.js", n), code: format!("{}\n", v), source: None });
                }
            }
        }
        let mut boot = String::new();
        if !it.server_fns.is_empty() {
            boot.push_str("const server = {};\n");
            for name in it.server_fns.iter() {
                let _ = writeln!(boot, "server[{}] = W.server({});", js_str(name), js_str(name));
            }
            boot.push_str("window.quillo = { server, components: C, rpc };\n");
        }
        if boots {
            boot.push_str(&super::boot_line(it, c));
        }
        pieces.push(Piece { path: "boot.js".into(), code: boot, source: None });

        let mut files: HashMap<String, String> = HashMap::new();
        let mut order = vec!["quillo.js".to_string()];
        files.insert("quillo.js".into(), RUNTIME.to_string());
        let mut sources: HashMap<Arc<str>, Option<String>> = HashMap::new();
        // A file's text goes into the map of the first piece made from it only;
        // the other pieces name the file and carry no copy. With it in every
        // piece, a program using a 4 MB file of 1,600 definitions (a component
        // kit) took 7 s to load under --watch, nearly all of it writing and
        // base64-encoding that file hundreds of times over (FEEDBACK15.md,
        // Q-097; branch dev-maps-once).
        let mut embedded: HashSet<Arc<str>> = HashSet::new();
        for p in pieces {
            if p.code.trim().is_empty() {
                continue;
            }
            let text = match &p.source {
                Some(src) => {
                    let content = sources.entry(src.clone()).or_insert_with(|| std::fs::read_to_string(&**src).ok());
                    let once = if embedded.insert(src.clone()) { content.as_deref() } else { None };
                    with_map(&p.code, src, once)
                }
                None => format!("\"use strict\";\n{}", unmarked(&p.code)),
            };
            order.push(p.path.clone());
            files.insert(p.path, text);
        }
        let hash = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            for f in &order {
                f.hash(&mut h);
                files[f].hash(&mut h);
            }
            format!("{:016x}", h.finish())
        };
        {
            let mut all = FILES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let all = all.get_or_insert_with(HashMap::new);
            // Bundles from long-gone versions of the program.
            if all.len() > 256 {
                all.clear();
            }
            all.insert(hash.clone(), Arc::new(files));
        }
        let list: Vec<String> = order.iter().map(|f| js_str(f)).collect();
        format!(
            "// Quillo in development: the runtime, and each component, view and function in a file\n\
             // of its own, with a source map to the .qwe file it is in.\n\
             (function () {{\n\
             const at = {};\n\
             for (const f of [{}]) {{\n\
             const s = document.createElement(\"script\");\n\
             s.src = at + f;\n\
             s.async = false;\n\
             document.head.appendChild(s);\n\
             }}\n\
             }})();\n",
            js_str(&format!("{}/_quillo/src/{}/", it.base, hash)),
            list.join(", ")
        )
    }

    /// `code` without its line marks, each with the line it names: a piece
    /// with no source file to map them to (a constant made from its
    /// definition, `let star = <svg>...</svg>`). Taking out only the marks
    /// left their numbers in the code: `59return W.t(...)` (FEEDBACK5.md,
    /// Q-047; branch stays-fixes).
    fn unmarked(code: &str) -> String {
        code.split(MARK).enumerate().filter(|(i, _)| i % 2 == 0).map(|(_, part)| part).collect()
    }

    /// The global `G["name"] = ...` defines, if it is one.
    fn defined(code: &str) -> Option<String> {
        let rest = code.strip_prefix("G[")?;
        let end = rest.find("]")?;
        serde_json::from_str::<String>(&rest[..end]).ok()
    }

    /// A path as it goes in a URL: what is not plain is %-escaped.
    fn url_path(p: &str) -> String {
        let mut out = String::new();
        for b in p.replace('\\', "/").bytes() {
            if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
                out.push(b as char);
            } else {
                out.push_str(&format!("%{:02X}", b));
            }
        }
        out
    }

    /// `code` without its line marks, and a source map (inline) from each
    /// mark's place to the line of `source` it names.
    fn with_map(code: &str, source: &str, content: Option<&str>) -> String {
        let (mut out, map) = mapped(code, source, content);
        let _ = write!(out, "//# sourceMappingURL=data:application/json;charset=utf-8;base64,{}\n", crate::ws::base64(map.to_string().as_bytes()));
        out
    }

    fn mapped(code: &str, source: &str, content: Option<&str>) -> (String, serde_json::Value) {
        let mut out = String::from("\"use strict\";\n");
        // One line of the map per line of the file: (column, source line, name).
        let mut lines: Vec<Vec<(usize, u32, Option<usize>)>> = vec![Vec::new()];
        let mut names: Vec<String> = Vec::new();
        for line in code.lines() {
            let mut marks: Vec<(usize, u32)> = Vec::new();
            let mut text = String::new();
            let mut parts = line.split(MARK);
            text.push_str(parts.next().unwrap_or(""));
            while let (Some(n), Some(after)) = (parts.next(), parts.next()) {
                if let Ok(n) = n.parse::<u32>() {
                    let col = text.encode_utf16().count();
                    if marks.last().map_or(true, |&(c, _)| c != col) {
                        marks.push((col, n));
                    }
                }
                text.push_str(after);
            }
            // The program's own names (`$count_1` is `count`), for debuggers that
            // show variables by the names the source gave them; on lines that
            // come from a statement (others would gain a line they are not on).
            let mut segs: Vec<(usize, u32, Option<usize>)> = marks.iter().map(|&(c, l)| (c, l, None)).collect();
            for (col, name) in own_names(&text) {
                let at = marks.iter().rev().find(|&&(c, _)| c <= col).map(|&(_, l)| l);
                if let Some(l) = at {
                    let i = names.iter().position(|n| *n == name).unwrap_or_else(|| {
                        names.push(name);
                        names.len() - 1
                    });
                    segs.push((col, l, Some(i)));
                }
            }
            segs.sort_by_key(|s| s.0);
            segs.dedup_by_key(|s| s.0);
            out.push_str(&text);
            out.push('\n');
            lines.push(segs);
        }
        let mut mappings = String::new();
        let (mut src_line, mut src_col, mut name) = (0i64, 0i64, 0i64);
        for (i, segs) in lines.iter().enumerate() {
            if i > 0 {
                mappings.push(';');
            }
            let mut gen_col = 0i64;
            for (j, &(col, line, n)) in segs.iter().enumerate() {
                if j > 0 {
                    mappings.push(',');
                }
                vlq(col as i64 - gen_col, &mut mappings);
                vlq(0, &mut mappings);
                vlq(line as i64 - 1 - src_line, &mut mappings);
                vlq(0 - src_col, &mut mappings);
                if let Some(n) = n {
                    vlq(n as i64 - name, &mut mappings);
                    name = n as i64;
                }
                gen_col = col as i64;
                src_line = line as i64 - 1;
                src_col = 0;
            }
        }
        let url = format!("file://{}", url_path(source));
        let map = serde_json::json!({
            "version": 3,
            "sources": [url],
            "sourcesContent": [content],
            "names": names,
            "mappings": mappings,
        });
        (out, map)
    }

    /// Where compiled code declares the program's variables: `const $count_1`
    /// (named by `fresh`), as (UTF-16 column, `count`). Only declarations: a
    /// breakpoint on a name elsewhere in a statement could be moved by the
    /// browser to the next one.
    fn own_names(line: &str) -> Vec<(usize, String)> {
        let b = line.as_bytes();
        let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'$';
        let mut out = Vec::new();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'$' && (i == 0 || !word(b[i - 1])) {
                let start = i;
                i += 1;
                while i < b.len() && word(b[i]) && b[i] != b'$' {
                    i += 1;
                }
                let w = &line[start + 1..i];
                let declared = ["const ", "let "].iter().any(|k| line[..start].ends_with(k));
                if let (Some((name, n)), true) = (w.rsplit_once('_'), declared) {
                    if !name.is_empty() && !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()) {
                        out.push((line[..start].encode_utf16().count(), name.to_string()));
                    }
                }
            } else {
                i += 1;
            }
        }
        out
    }

    /// A source map number: base64 digits of 5 bits each, the sign last.
    fn vlq(n: i64, out: &mut String) {
        const DIGITS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut v = if n < 0 { ((-n) << 1) | 1 } else { n << 1 } as u64;
        loop {
            let mut d = (v & 31) as usize;
            v >>= 5;
            if v > 0 {
                d |= 32;
            }
            out.push(DIGITS[d] as char);
            if v == 0 {
                break;
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn numbers_in_source_maps() {
            let mut s = String::new();
            for n in [0, 1, -1, 15, 16, -16, 1000] {
                vlq(n, &mut s);
                s.push(' ');
            }
            assert_eq!(s, "A C D e gB hB w+B ");
        }

        #[test]
        fn marks_become_a_map() {
            let code = format!("{m}3{m}const a = 1;\nfoo();\nif (x) {m}5{m}b(); {m}6{m}c();\n", m = MARK);
            let (out, map) = mapped(&code, "/app/main.qwe", None);
            assert_eq!(out, "\"use strict\";\nconst a = 1;\nfoo();\nif (x) b(); c();\n");
            // Line 2 (after "use strict") col 0 → source line 3 (index 2); line 4: col 7 → 5, col 12 → 6.
            assert_eq!(map["mappings"], ";AAEA;;OAEA,KACA");
            assert_eq!(map["sources"][0], "file:///app/main.qwe");
        }

        // A piece with no file to map to loses its marks with their lines
        // (FEEDBACK5.md, Q-047; branch stays-fixes).
        #[test]
        fn marks_go_with_their_lines() {
            let code = format!("G[\"star\"] = (function () {{\n{m}59{m}return W.t($b1, []);\n}})();\n", m = MARK);
            assert_eq!(unmarked(&code), "G[\"star\"] = (function () {\nreturn W.t($b1, []);\n})();\n");
        }

        #[test]
        fn variables_keep_their_names() {
            assert_eq!(own_names("const $count_1 = $i.sig(\"count\", () => $p.start + $my_total_12.v);"), vec![(6, "count".to_string())]);
            assert_eq!(own_names("let $my_total_12 = 0;"), vec![(4, "my_total".to_string())]);
            let code = format!("{m}2{m}const $count_1 = 0;\n{m}3{m}f($count_1);\n", m = MARK);
            let (_, map) = mapped(&code, "/a.qwe", None);
            assert_eq!(map["names"], serde_json::json!(["count"]));
            assert_eq!(map["mappings"], ";AACA,MAAAA;AACA");
        }
    }
}

/// A file of a development bundle (see `DEV`).
pub fn dev_file(hash: &str, path: &str) -> Option<String> {
    dev::file(hash, path)
}

/// The script pages with `<quillo-media>` load: the player and the
/// connection it uses, and nothing else of the runtime.
pub fn media_script(base: &str) -> String {
    let program = format!("BASE = {};\nmediaDefine();\n", js_str(base));
    assemble("(function () {", &program, "})();")
}

/// A script that runs the runtime, compiles the global function `name`,
/// calls it and prints the result's repr: for testing against the server.
pub fn test_script(it: &Interp, name: &str) -> Result<String, QuilloError> {
    let mut c = Compiler::new(it);
    let (js, is_async) = c.global(name, 0)?;
    let mut program = String::new();
    for b in &c.blocks {
        program.push_str(b);
        program.push('\n');
    }
    for d in c.defs.iter().chain(c.view_defs.iter()) {
        program.push_str(d);
        program.push('\n');
    }
    let _ = writeln!(
        program,
        "try {{ console.log(W.repr({}{}())); }} catch (e) {{ console.log(\"ERROR: \" + (e instanceof Ret ? W.repr(e.v) : e.message)); }}",
        if is_async { "await " } else { "" },
        js
    );
    Ok(assemble("(async function () {", &program, "})();"))
}

/// The parts of the runtime `program` needs, then the program, minified.
fn assemble(open: &str, program: &str, close: &str) -> String {
    let rt = runtime_units();
    let keep = shake(rt, program);
    let mut out = String::with_capacity(program.len() + 16 * 1024);
    out.push_str(open);
    out.push('\n');
    minify_into(&rt.preamble, &mut out);
    for (i, (_, text)) in rt.units.iter().enumerate() {
        if keep[i] {
            minify_into(text, &mut out);
        }
    }
    minify_into(program, &mut out);
    out.push_str(close);
    out.push('\n');
    out
}

/// Which of the runtime's declarations `program` needs, with what they need.
fn shake(rt: &Runtime, program: &str) -> Vec<bool> {
    let mut keep = vec![false; rt.units.len()];
    let mut todo: Vec<String> = names_in(program);
    while let Some(n) = todo.pop() {
        if let Some(&i) = rt.by_name.get(&n) {
            if !keep[i] {
                keep[i] = true;
                todo.extend(names_in(&rt.units[i].1));
            }
        }
    }
    keep
}

/// The runtime cut into its top-level declarations (`function f`, `const x`,
/// `class C`, `B.name = ...`, `W.name = ...`), in order, without the lines
/// for development.
struct Runtime {
    preamble: String,
    units: Vec<(String, String)>,
    by_name: HashMap<String, usize>,
}

fn runtime_units() -> &'static Runtime {
    static RT: std::sync::OnceLock<Runtime> = std::sync::OnceLock::new();
    RT.get_or_init(|| {
        let mut preamble = String::new();
        let mut units: Vec<(String, String)> = Vec::new();
        // Lines ending `// dev` are for development bundles only, which
        // take the runtime as it is written.
        for line in RUNTIME.lines().filter(|l| !l.trim_end().ends_with("// dev")) {
            let decl = ["async function ", "function ", "class ", "const ", "let "]
                .iter()
                .find_map(|p| line.strip_prefix(p))
                .map(|rest| rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$').collect::<String>())
                .or_else(|| {
                    ["B.", "W."].iter().find_map(|p| {
                        line.strip_prefix(p).map(|rest| {
                            let n: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                            format!("{}{}", p, n)
                        })
                    })
                });
            match decl {
                Some(name) if !name.is_empty() && !name.ends_with('.') => units.push((name, format!("{}\n", line))),
                _ => match units.last_mut() {
                    Some(u) => {
                        u.1.push_str(line);
                        u.1.push('\n');
                    }
                    None => {
                        preamble.push_str(line);
                        preamble.push('\n');
                    }
                },
            }
        }
        let mut by_name: HashMap<String, usize> = units.iter().enumerate().map(|(i, (n, _))| (n.clone(), i)).collect();
        // `let a = null, b = 0`: each name it declares needs it (the commas
        // between declarations, not those inside their values).
        for (i, (_, text)) in units.iter().enumerate() {
            let first = text.lines().next().unwrap_or("");
            if !(first.starts_with("let ") || first.starts_with("const ")) {
                continue;
            }
            let (mut depth, mut quote) = (0i32, None);
            for (k, c) in first.char_indices() {
                match (quote, c) {
                    (Some(q), _) if c == q => quote = None,
                    (Some(_), _) => {}
                    (None, '"' | '\'' | '`') => quote = Some(c),
                    (None, '(' | '[' | '{') => depth += 1,
                    (None, ')' | ']' | '}') => depth -= 1,
                    (None, ';') if depth == 0 => break,
                    (None, ',') if depth == 0 => {
                        let rest = first[k + 1..].trim_start();
                        let n: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$').collect();
                        if !n.is_empty() && rest[n.len()..].trim_start().starts_with('=') {
                            by_name.entry(n).or_insert(i);
                        }
                    }
                    _ => {}
                }
            }
        }
        Runtime { preamble, units, by_name }
    })
}

/// The identifiers in some JavaScript, with `B.x` and `W.x` kept whole
/// (and also giving `B` or `W`). Strings are scanned too: that can only
/// keep more of the runtime than needed, never less.
fn names_in(js: &str) -> Vec<String> {
    let chars: Vec<char> = js.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let ident = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    let mut line_start = true;
    while i < chars.len() {
        let c = chars[i];
        // A whole-line comment names nothing the code needs.
        if line_start && c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '\n' {
            line_start = true;
        } else if !c.is_whitespace() {
            line_start = false;
        }
        // (After `.` a property, except after `...`, a spread.)
        let spread = i >= 3 && chars[i - 3..i] == ['.', '.', '.'];
        if ident(c) && (i == 0 || !ident(chars[i - 1]) && (chars[i - 1] != '.' || spread)) {
            let start = i;
            while i < chars.len() && ident(chars[i]) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            if (word == "B" || word == "W") && chars.get(i) == Some(&'.') {
                let s2 = i + 1;
                let mut j = s2;
                while j < chars.len() && ident(chars[j]) {
                    j += 1;
                }
                out.push(format!("{}.{}", word, chars[s2..j].iter().collect::<String>()));
            }
            out.push(word);
        } else {
            i += 1;
        }
    }
    out
}

/// `@mount` is no DOM event: the runtime's `$mount` handles it, and naming
/// it so in the program brings that part of the runtime into the bundle.
fn event_name(ev: &str) -> &str {
    if ev == "mount" {
        "$mount"
    } else {
        ev
    }
}

/// Drops comment lines, blank lines and indentation.
fn minify_into(js: &str, out: &mut String) {
    let mut scan = Scan::default();
    for line in js.lines() {
        let t = line.trim();
        if scan.code() && (t.is_empty() || t.starts_with("//")) {
            continue;
        }
        out.push_str(if scan.code() { t } else { line });
        // A newline after `;`, `{` or `,` in code changes nothing: the
        // statement there is ended, or goes on, either way.
        if !(scan.line(t) && t.ends_with([';', '{', ','])) {
            out.push('\n');
        }
    }
}

/// Whether code ends with a keyword after which `/` starts a regular
/// expression (`return /x/`), not a division.
fn after_keyword(code: &[u8]) -> bool {
    let t = std::str::from_utf8(code).unwrap_or("").trim_end();
    let word = t.rsplit(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$')).next().unwrap_or("");
    matches!(word, "return" | "typeof" | "case" | "in" | "of" | "delete" | "void" | "throw" | "new" | "else" | "do" | "yield" | "await")
}

/// Where a scan of JavaScript is at the end of a line: in code (and how
/// many `${` of templates deep), or inside a template that goes on.
#[derive(Default)]
struct Scan {
    /// Template literals open, innermost last: the brace depth inside each
    /// one's current `${...}`, or None while in its text.
    templates: Vec<Option<u32>>,
}

impl Scan {
    fn code(&self) -> bool {
        self.templates.is_empty()
    }

    /// Scans a line; whether it ends in code, outside any string, template,
    /// regular expression or comment, and the scan is sure of it. A
    /// comment, or a `/` it cannot place, makes it unsure.
    fn line(&mut self, l: &str) -> bool {
        let b = l.as_bytes();
        let mut i = 0;
        // The last character of code before here, to tell a regular
        // expression's `/` from a division's.
        let mut prev = b' ';
        while i < b.len() {
            let c = b[i];
            match self.templates.last_mut() {
                // A template's text.
                Some(None) => {
                    match c {
                        b'\\' => i += 1,
                        b'`' => {
                            self.templates.pop();
                            prev = b'`';
                        }
                        b'$' if b.get(i + 1) == Some(&b'{') => {
                            *self.templates.last_mut().unwrap() = Some(0);
                            i += 1;
                            prev = b'{';
                        }
                        _ => {}
                    }
                    i += 1;
                    continue;
                }
                Some(Some(depth)) if c == b'}' => {
                    if *depth == 0 {
                        *self.templates.last_mut().unwrap() = None;
                    } else {
                        *depth -= 1;
                    }
                    prev = b'}';
                    i += 1;
                    continue;
                }
                Some(Some(depth)) if c == b'{' => {
                    *depth += 1;
                    prev = b'{';
                    i += 1;
                    continue;
                }
                _ => {}
            }
            match c {
                b'"' | b'\'' => {
                    i += 1;
                    while i < b.len() && b[i] != c {
                        i += if b[i] == b'\\' { 2 } else { 1 };
                    }
                    if i >= b.len() {
                        return false;
                    }
                    prev = c;
                }
                b'`' => self.templates.push(None),
                b'/' if matches!(b.get(i + 1), Some(b'/' | b'*')) => return false,
                b'/' if b"(,=:[!&|?{};+-*%<>~^".contains(&prev) || prev == b' ' && i == 0 || after_keyword(&b[..i]) => {
                    // A regular expression, to its closing `/`.
                    let mut class = false;
                    i += 1;
                    while i < b.len() && (class || b[i] != b'/') {
                        match b[i] {
                            b'\\' => i += 1,
                            b'[' => class = true,
                            b']' => class = false,
                            _ => {}
                        }
                        i += 1;
                    }
                    if i >= b.len() {
                        return false;
                    }
                    prev = b'/';
                }
                b'/' if prev.is_ascii_alphanumeric() || b")]}_$\"'`".contains(&prev) => prev = b'/',
                b'/' => return false,
                b' ' | b'\t' => {}
                _ => prev = c,
            }
            i += 1;
        }
        self.code()
    }
}

/// What a name refers to inside compiled code.
#[derive(Clone)]
enum Local {
    /// A variable, with its JS name; `Some(async)` when it holds a known function.
    Var(String, Option<bool>),
    /// Component state: a signal, read `x.v` and set with `x.set(v)`.
    State(String),
    /// What follows state (a memo, a row's item): read `x.v`.
    Reactive(String),
    /// A component's or view's parameter, read from its props: `$p.name`.
    Prop(String),
}

struct FnCtx {
    scopes: Vec<HashMap<String, Local>>,
    temps: usize,
    is_async: bool,
    needs_async: bool,
    /// The component or view being compiled, for error messages and `state` rules.
    component: Option<String>,
    /// A view's code (props, no state).
    view: bool,
    /// `state` was declared ahead of the code (a body made again whole).
    hoisted: bool,
    /// A `state` initializer's code, which the server runs as it renders.
    state_init: bool,
    /// The function written in an `@event` or `@mount` attribute itself, not
    /// one inside it: a Promise its `js(...)` gives is waited for here.
    waits_js: bool,
}

enum Resolved {
    Local(String, Option<bool>),
    State(String),
    Reactive(String),
    Prop(String),
    /// JS expression and whether calling it waits on the server.
    Global(String, bool),
}

struct Compiler<'a> {
    it: &'a Interp,
    /// The lambda an `@event` or `@mount` attribute is, while it compiles (by
    /// its definition, which both of `function`'s passes see).
    handler: Option<*const FnDef>,
    /// Compiling what `.then`, `.catch` or `.finally` is called on: a Promise
    /// is wanted there, not what it gives.
    keep_promise: bool,
    /// `let` functions that wait for what `js(...)` gives, as a handler
    /// does: a component's own (unless its markup calls them, which cannot
    /// wait) and those written inside a handler (FEEDBACK18.md, Q-102;
    /// branch feedback18-fixes).
    waiting: HashSet<*const FnDef>,
    /// For each component being compiled, the names its code calls where it
    /// cannot wait: in markup, outside any function.
    sync_calls: Vec<HashSet<String>>,
    assigned: HashSet<String>,
    /// Globals read from the server's environment, files or database.
    private: HashSet<String>,
    defs: Vec<String>,
    /// Templates (see `block` in the runtime), emitted before everything else.
    blocks: Vec<String>,
    /// Names the component being compiled assigns somewhere: its `let`s of
    /// them stay variables.
    mutable: HashSet<String>,
    /// Whether what was just compiled reads something that can change
    /// (state, a memo, a prop, a row's item, `url()`, the component's own
    /// functions): then it is made a memo, an effect or a hole that follows.
    reactive: bool,
    /// The row signal of the `for` row whose template is being compiled
    /// (`row` until that template takes it, `cur_row` while it is
    /// compiled), what the hole being compiled read (`read_log`: signals'
    /// JS, "*" for what may read anything), and which of the template's
    /// values read nothing but the row (`row_mask`): those the row updates
    /// itself, without an effect each (see `direct` in the runtime).
    row: Option<String>,
    cur_row: Option<String>,
    read_log: Option<Vec<String>>,
    row_mask: u32,
    /// Whether the name just resolved belongs to a component's (or view's) code.
    last_here: bool,
    /// Views whose `<Name/>` form (code once, props as getters) is made.
    views: HashSet<String>,
    view_defs: Vec<String>,
    /// Globals compiled (or being compiled): JS name and whether they are async.
    globals: HashMap<String, (String, bool)>,
    ctxs: Vec<FnCtx>,
    counter: usize,
    line: u32,
    /// The component or function being compiled, for error messages.
    what: String,
    /// The file it is written in, so an error names that file and not the
    /// program's main one (FEEDBACK18.md, Q-101; branch feedback18-fixes).
    file: Option<std::sync::Arc<str>>,
    /// The program's own types whose shapes are in the bundle (`W.type`).
    types: HashSet<String>,
    /// Whether statements are marked with the line they come from (see `DEV`).
    dev: bool,
}

fn js_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

fn ty_js(ty: &Option<Ty>) -> String {
    match ty {
        Some(t) => format!("[{}], {}", t.names.iter().map(|n| js_str(n)).collect::<Vec<_>>().join(", "), t.optional),
        None => "null, false".into(),
    }
}

fn collect_assigned(stmts: &[Stmt], out: &mut HashSet<String>) {
    for s in stmts {
        match s {
            Stmt::Assign(n, _, e, _) => {
                out.insert(n.clone());
                expr_assigned(e, out);
            }
            Stmt::Let(_, e, _) | Stmt::Expr(e) | Stmt::State(_, e, _) => expr_assigned(e, out),
            Stmt::SetField(o, _, _, e, _) => {
                expr_assigned(o, out);
                expr_assigned(e, out)
            }
            Stmt::Return(Some(e)) => expr_assigned(e, out),
            Stmt::Fn(d) => expr_assigned(&d.body, out),
            Stmt::Route(r) => expr_assigned(&r.body, out),
            Stmt::Use(..) | Stmt::Mount(..) | Stmt::Service(..) | Stmt::Type(_) | Stmt::Return(None) => {}
        }
    }
}

fn expr_assigned(e: &Expr, out: &mut HashSet<String>) {
    let mut f = |x: &Expr| expr_assigned(x, out);
    match e {
        Expr::Block(stmts) => collect_assigned(stmts, out),
        Expr::Lambda(d) => expr_assigned(&d.body, out),
        Expr::Lit(_) | Expr::Ident(..) => {}
        Expr::Template(parts) => {
            for p in parts {
                if let TPart::Expr(x) = p {
                    f(x)
                }
            }
        }
        Expr::List(xs) => xs.iter().for_each(f),
        Expr::Map(es) => es.iter().for_each(|(_, x)| f(x)),
        Expr::Unary(_, x, _) | Expr::Try(x) | Expr::Field(x, _, _) => f(x),
        Expr::Binary(_, a, b, _) | Expr::Coalesce(a, b) | Expr::Pipe(a, b, _) | Expr::Index(a, b, _) => {
            f(a);
            f(b)
        }
        Expr::Call(c, args, _) => {
            f(c);
            args.iter().for_each(f)
        }
        Expr::If(c, a, b) => {
            f(c);
            f(a);
            if let Some(b) = b {
                f(b)
            }
        }
        Expr::Match(s, arms, _) => {
            f(s);
            for a in arms {
                if let Some(g) = &a.guard {
                    expr_assigned(g, out)
                }
                expr_assigned(&a.body, out)
            }
        }
        Expr::For(_, i, b, _) | Expr::While(i, b, _) => {
            f(i);
            f(b)
        }
        Expr::Reply(s, b, _) => {
            f(s);
            if let Some(b) = b {
                f(b)
            }
        }
        Expr::Html(el, _) => html_assigned(el, out),
    }
}

/// The names a component's code calls where it cannot wait (outside any
/// function: its markup, its state's initializers), and what the `let`
/// functions among them call in turn (FEEDBACK18.md, Q-102; branch
/// feedback18-fixes).
fn sync_called(body: &Expr) -> HashSet<String> {
    fn calls(e: &Expr, out: &mut HashSet<String>) {
        each_expr_in(e, &mut |x| match x {
            Expr::Call(c, _, _) => {
                if let Expr::Ident(n, _) = &**c {
                    out.insert(n.clone());
                }
            }
            Expr::Pipe(_, b, _) => {
                if let Expr::Ident(n, _) = &**b {
                    out.insert(n.clone());
                }
            }
            _ => {}
        }, false);
    }
    let mut lets: HashMap<&str, &FnDef> = HashMap::new();
    if let Expr::Block(ss) = body {
        for st in ss {
            if let Stmt::Let(Pattern::Bind(n), Expr::Lambda(d), _) = st {
                lets.insert(n.as_str(), &**d);
            }
        }
    }
    let mut out = HashSet::new();
    calls(body, &mut out);
    let mut done: HashSet<String> = HashSet::new();
    loop {
        let todo: Vec<String> = out.iter().filter(|n| lets.contains_key(n.as_str()) && !done.contains(*n)).cloned().collect();
        if todo.is_empty() {
            break;
        }
        for n in todo {
            calls(&lets[n.as_str()].body, &mut out);
            done.insert(n);
        }
    }
    out
}

/// Calls `f` on `e` and every expression inside it.
pub(crate) fn each_expr(e: &Expr, f: &mut dyn FnMut(&Expr)) {
    each_expr_in(e, f, true)
}

/// The same; with `fns` false, not inside the functions in it.
fn each_expr_in(e: &Expr, f: &mut dyn FnMut(&Expr), fns: bool) {
    f(e);
    let stmts = |ss: &[Stmt], f: &mut dyn FnMut(&Expr)| {
        for st in ss {
            match st {
                Stmt::Let(_, x, _) | Stmt::Assign(_, _, x, _) | Stmt::Expr(x) | Stmt::State(_, x, _) | Stmt::Return(Some(x)) => {
                    each_expr_in(x, f, fns)
                }
                Stmt::SetField(o, _, _, x, _) => {
                    each_expr_in(o, f, fns);
                    each_expr_in(x, f, fns)
                }
                Stmt::Fn(d) if fns => each_expr_in(&d.body, f, fns),
                Stmt::Fn(_) => {}
                Stmt::Route(r) => each_expr_in(&r.body, f, fns),
                Stmt::Use(..) | Stmt::Mount(..) | Stmt::Service(..) | Stmt::Type(_) | Stmt::Return(None) => {}
            }
        }
    };
    match e {
        Expr::Block(ss) => stmts(ss, f),
        Expr::Lambda(d) if fns => each_expr_in(&d.body, f, fns),
        Expr::Lambda(_) => {}
        Expr::Lit(_) | Expr::Ident(..) => {}
        Expr::Template(parts) => parts.iter().for_each(|p| if let TPart::Expr(x) = p { each_expr_in(x, f, fns) }),
        Expr::List(xs) => xs.iter().for_each(|x| each_expr_in(x, f, fns)),
        Expr::Map(es) => es.iter().for_each(|(_, x)| each_expr_in(x, f, fns)),
        Expr::Unary(_, x, _) | Expr::Try(x) | Expr::Field(x, _, _) => each_expr_in(x, f, fns),
        Expr::Binary(_, a, b, _) | Expr::Coalesce(a, b) | Expr::Pipe(a, b, _) | Expr::Index(a, b, _) => {
            each_expr_in(a, f, fns);
            each_expr_in(b, f, fns)
        }
        Expr::Call(c, args, _) => {
            each_expr_in(c, f, fns);
            args.iter().for_each(|x| each_expr_in(x, f, fns))
        }
        Expr::If(c, a, b) => {
            each_expr_in(c, f, fns);
            each_expr_in(a, f, fns);
            if let Some(b) = b {
                each_expr_in(b, f, fns)
            }
        }
        Expr::Match(x, arms, _) => {
            each_expr_in(x, f, fns);
            for a in arms {
                if let Some(g) = &a.guard {
                    each_expr_in(g, f, fns)
                }
                each_expr_in(&a.body, f, fns)
            }
        }
        Expr::For(_, i, b, _) | Expr::While(i, b, _) => {
            each_expr_in(i, f, fns);
            each_expr_in(b, f, fns)
        }
        Expr::Reply(x, b, _) => {
            each_expr_in(x, f, fns);
            if let Some(b) = b {
                each_expr_in(b, f, fns)
            }
        }
        Expr::Html(el, _) => each_html_expr(el, f, fns),
    }
}

/// Whether `body` can leave its function early (`return`, or `?` on an
/// error), which compiled code does by throwing `W.Ret`; a function whose
/// body cannot needs no catch. (Functions inside count too: it errs on the
/// side of a catch.)
fn leaves_early(body: &Expr) -> bool {
    let mut found = false;
    each_expr(body, &mut |e| match e {
        Expr::Try(_) => found = true,
        Expr::Block(ss) if ss.iter().any(|s| matches!(s, Stmt::Return(_))) => found = true,
        _ => {}
    });
    found
}

fn each_html_expr(el: &HtmlEl, f: &mut dyn FnMut(&Expr), fns: bool) {
    for x in el.id.iter().chain(el.classes.iter()) {
        each_expr_in(x, f, fns)
    }
    for (_, v) in &el.attrs {
        if let AttrVal::Expr(x) | AttrVal::Spread(x) = v {
            each_expr_in(x, f, fns)
        }
    }
    for c in &el.children {
        match c {
            HtmlChild::El(e) => each_html_expr(e, f, fns),
            HtmlChild::Expr(x) => each_expr_in(x, f, fns),
        }
    }
}

/// Whether `e` is markup, or may give markup (a loop, a branch).
fn has_markup(e: &Expr) -> bool {
    match e {
        Expr::Html(..) | Expr::For(..) => true,
        Expr::If(_, a, b) => has_markup(a) || b.as_deref().map_or(false, has_markup),
        Expr::Match(_, arms, _) => arms.iter().any(|a| has_markup(&a.body)),
        Expr::Block(ss) => matches!(ss.last(), Some(Stmt::Expr(x)) if has_markup(x)),
        _ => false,
    }
}

/// What variables named `PUBLIC_...` hold may be sent to the browser:
/// `env("PUBLIC_MAP_TOKEN")`, with the name written out, gives that name.
pub fn public_env<'a>(callee: &Expr, args: &'a [Expr]) -> Option<&'a str> {
    match (callee, args.first()) {
        (Expr::Ident(f, _), Some(Expr::Lit(Lit::Str(n)))) if f == "env" && args.len() <= 2 && n.starts_with("PUBLIC_") => Some(n),
        _ => None,
    }
}

/// Builtins whose results may be private to the server: its environment,
/// its files, its database, other services.
const PRIVATE_SOURCES: &[&str] = &["env", "read_file", "file", "db", "http"];

/// Top-level variables whose value comes, directly or through other such
/// variables, from `PRIVATE_SOURCES`, a service, or a function that uses
/// any of them (`let CONFIG = load_config()`). Components get other
/// globals' values written into the browser bundle, which anyone can
/// download; these they must ask a `server fn` for.
fn collect_private(program: &[Arc<Vec<Stmt>>]) -> HashSet<String> {
    let mut services: HashSet<String> = HashSet::new();
    for stmts in program {
        for st in stmts.iter() {
            if let Stmt::Service(name, ..) = st {
                services.insert(name.clone());
            }
        }
    }
    let sources: HashSet<&str> = PRIVATE_SOURCES.iter().copied().collect();
    let mut vars: HashSet<String> = HashSet::new();
    // Functions that use what is private: calling one gives a private value.
    // They can be called as methods too (`"KEY".env`).
    let mut fns: HashSet<String> = ["env", "read_file"].iter().map(|s| s.to_string()).collect();
    // Whether `e` uses a private name (other than the `shadowed` ones), its
    // functions' bodies included when `inside` is set.
    // `env("PUBLIC_...")` is not: each such call is one use of `env` that
    // does not count.
    let uses = |e: &Expr, inside: bool, shadowed: &[String], vars: &HashSet<String>, fns: &HashSet<String>| {
        let mut found = false;
        let (mut envs, mut public) = (0, 0);
        each_expr_in(
            e,
            &mut |x| match x {
                Expr::Ident(n, _) if n == "env" && !shadowed.contains(n) => envs += 1,
                Expr::Ident(n, _) if !shadowed.contains(n) => {
                    found |= sources.contains(n.as_str()) || services.contains(n) || vars.contains(n) || fns.contains(n)
                }
                Expr::Field(_, n, _) => found |= fns.contains(n),
                Expr::Call(f, args, _) if public_env(f, args).is_some() => public += 1,
                _ => {}
            },
            inside,
        );
        found || envs > public
    };
    // What a variable's functions use, as before: a function value is made
    // in the browser from its definition, and the server fns it calls stay
    // on the server.
    let uses_in_fns = |e: &Expr, vars: &HashSet<String>| {
        let mut found = false;
        let (mut envs, mut public) = (0, 0);
        each_expr(e, &mut |x| match x {
            Expr::Ident(n, _) if n == "env" => envs += 1,
            Expr::Ident(n, _) => found |= sources.contains(n.as_str()) || vars.contains(n),
            Expr::Call(f, args, _) if public_env(f, args).is_some() => public += 1,
            _ => {}
        });
        found || envs > public
    };
    loop {
        let before = vars.len() + fns.len();
        for stmts in program {
            for st in stmts.iter() {
                match st {
                    Stmt::Let(p, e, _) if uses(e, false, &[], &vars, &fns) || uses_in_fns(e, &vars) => {
                        let mut names = Vec::new();
                        pattern_names(p, &mut names);
                        vars.extend(names);
                    }
                    Stmt::Fn(d) if !fns.contains(&d.name) => {
                        // Its parameters are its own, whatever their names.
                        let params: Vec<String> = d.params.iter().map(|p| p.name.clone()).collect();
                        if uses(&d.body, true, &params, &vars, &fns) {
                            fns.insert(d.name.clone());
                        }
                    }
                    _ => {}
                }
            }
        }
        if vars.len() + fns.len() == before {
            return vars;
        }
    }
}

fn html_assigned(el: &HtmlEl, out: &mut HashSet<String>) {
    for (_, v) in &el.attrs {
        if let AttrVal::Expr(e) | AttrVal::Spread(e) = v {
            expr_assigned(e, out)
        }
    }
    for c in &el.children {
        match c {
            HtmlChild::El(e) => html_assigned(e, out),
            HtmlChild::Expr(e) => expr_assigned(e, out),
        }
    }
}

/// A pattern as JS testing `v` (a JS variable or path) directly, for the
/// simple kinds: names, `_`, literals and lists of them. The test ("" for
/// none) and each name's JS; None for the others, which `W.match` does.
fn simple_pattern(p: &Pattern, v: &str, out: &mut Vec<(String, String)>) -> Option<String> {
    match p {
        Pattern::Wild => Some(String::new()),
        Pattern::Bind(n) => {
            if out.iter().any(|(m, _)| m == n) {
                return None;
            }
            out.push((n.clone(), v.to_string()));
            Some(String::new())
        }
        Pattern::Lit(l) => Some(format!("W.eq({}, {})", lit_js(l), v)),
        Pattern::List(ps, rest) => {
            let k = ps.len();
            let mut conds = vec![format!("Array.isArray({})", v), format!("{}.length {} {}", v, if rest.is_none() { "===" } else { ">=" }, k)];
            for (i, q) in ps.iter().enumerate() {
                let c = simple_pattern(q, &format!("{}[{}]", v, i), out)?;
                if !c.is_empty() {
                    conds.push(c);
                }
            }
            if let Some(Some(n)) = rest {
                if out.iter().any(|(m, _)| m == n) {
                    return None;
                }
                out.push((n.clone(), format!("{}.slice({})", v, k)));
            }
            Some(conds.join(" && "))
        }
        _ => None,
    }
}

/// Names a pattern binds, in order.
fn pattern_names(p: &Pattern, out: &mut Vec<String>) {
    match p {
        Pattern::Bind(n) => out.push(n.clone()),
        Pattern::Alt(ps) => ps.iter().for_each(|q| pattern_names(q, out)),
        Pattern::As(q, n) => {
            pattern_names(q, out);
            out.push(n.clone())
        }
        Pattern::List(ps, rest) => {
            ps.iter().for_each(|q| pattern_names(q, out));
            if let Some(Some(n)) = rest {
                out.push(n.clone())
            }
        }
        Pattern::Record(fs) => {
            for f in fs {
                let before = out.len();
                pattern_names(&f.pat, out);
                if out.len() == before && f.pat.is_type_test() {
                    out.push(f.path.last().unwrap().clone())
                }
            }
        }
        Pattern::Wild | Pattern::Lit(_) | Pattern::Type(_) => {}
    }
    let mut seen = HashSet::new();
    out.retain(|n| seen.insert(n.clone()));
}

fn lit_js(l: &Lit) -> String {
    match l {
        Lit::Nil => "null".into(),
        Lit::Bool(b) => b.to_string(),
        // Past 2^53 a JS number is not whole: a BigInt, as the runtime has them.
        Lit::Int(n) if n.unsigned_abs() > (1 << 53) - 1 => format!("{}n", n),
        Lit::Int(n) => n.to_string(),
        Lit::Float(f) if f.is_finite() => format!("{:?}", f),
        Lit::Dec(d) => format!("new W.Dec({}n, {})", d.mantissa(), d.scale()),
        Lit::Float(_) => "null".into(),
        Lit::Str(s) => js_str(s),
    }
}

/// A top-level constant's value as JavaScript, for the bundle. JSON would
/// do but for decimals (text in JSON) and ints past 2^53 (rounded): those
/// are written as the literals `lit_js` gives, wherever they are inside
/// lists and maps (FEEDBACK5.md, Q-045; branch stays-fixes).
fn value_js(v: &Value) -> String {
    fn exact(v: &Value) -> bool {
        match v {
            Value::Dec(_) => false,
            Value::Int(n) => n.unsigned_abs() <= (1 << 53) - 1,
            Value::List(l) => l.iter().all(exact),
            Value::Map(m) => m.values().all(exact),
            _ => true,
        }
    }
    if exact(v) {
        return v.to_json().to_string();
    }
    match v {
        Value::Dec(d) => format!("new W.Dec({}n, {})", d.mantissa(), d.scale()),
        Value::Int(n) => format!("{}n", n),
        Value::List(l) => format!("[{}]", l.iter().map(value_js).collect::<Vec<_>>().join(",")),
        // `"__proto__": x` in a literal would set the object's prototype.
        Value::Map(m) => format!(
            "{{{}}}",
            m.iter()
                .map(|(k, x)| if &**k == "__proto__" { format!("[{}]:{}", js_str(k), value_js(x)) } else { format!("{}:{}", js_str(k), value_js(x)) })
                .collect::<Vec<_>>()
                .join(",")
        ),
        other => other.to_json().to_string(),
    }
}

impl<'a> Compiler<'a> {
    fn new(it: &'a Interp) -> Self {
        let mut assigned = HashSet::new();
        for stmts in it.program.iter() {
            collect_assigned(stmts, &mut assigned);
        }
        Compiler {
            it,
            assigned,
            private: collect_private(&it.program),
            defs: Vec::new(),
            blocks: Vec::new(),
            mutable: HashSet::new(),
            reactive: false,
            row: None,
            cur_row: None,
            read_log: None,
            row_mask: 0,
            last_here: false,
            views: HashSet::new(),
            view_defs: Vec::new(),
            globals: HashMap::new(),
            handler: None,
            keep_promise: false,
            waiting: HashSet::new(),
            sync_calls: Vec::new(),
            ctxs: Vec::new(),
            counter: 0,
            line: 0,
            what: String::new(),
            file: None,
            types: HashSet::new(),
            dev: DEV.load(std::sync::atomic::Ordering::Relaxed),
        }
    }

    /// Puts the shape of the program's own type `name` in the bundle (and
    /// of the types it names), for `User(...)`, annotations and patterns.
    fn need_type(&mut self, name: &str) -> Result<(), QuilloError> {
        let Some(td) = self.it.types.get(name).cloned() else { return Ok(()) };
        if !self.types.insert(name.to_string()) {
            return Ok(());
        }
        let shape = if td.is_record() {
            let mut fs = Vec::new();
            for (k, ft) in td.ty.fields.as_deref().unwrap_or_default() {
                let t = self.ty_shape(ft)?;
                let d = match td.defaults.iter().find(|(d, _)| d == k) {
                    // Made for each record, in the program's top-level scope.
                    Some((_, e)) => {
                        let def = FnDef {
                            name: format!("{}.{}", td.name, k),
                            params: Vec::new(),
                            ret: None,
                            err: None,
                            effects: None,
                            server: false,
                            component: false,
                            line: td.line,
                            body: e.clone(),
                            nesting: 0,
                            file: None,
                            hot: Default::default(),
                        };
                        let saved = std::mem::take(&mut self.ctxs);
                        let r = self.function(&def);
                        self.ctxs = saved;
                        let (code, is_async) = r?;
                        if is_async {
                            self.line = td.line;
                            return self.err(format!("the default of `{}` in `{}` waits on the server, which a record made in the browser cannot", k, td.name));
                        }
                        code
                    }
                    None => "null".into(),
                };
                fs.push(format!("[{}, {}, {}]", js_str(k), t, d));
            }
            format!("{{r: [{}]}}", fs.join(", "))
        } else {
            format!("{{a: {}}}", self.ty_shape(&td.ty)?)
        };
        self.defs.push(format!("W.type({}, {});", js_str(name), shape));
        Ok(())
    }

    /// A type for the runtime's record checks: names, null allowed, what a
    /// list holds and a map's fields.
    fn ty_shape(&mut self, t: &Ty) -> Result<String, QuilloError> {
        for n in &t.names {
            self.need_type(n)?;
        }
        let e = match &t.elem {
            Some(e) => self.ty_shape(e)?,
            None => "null".into(),
        };
        let f = match &t.fields {
            Some(fs) => {
                let mut out = Vec::new();
                for (k, ft) in fs {
                    out.push(format!("[{}, {}]", js_str(k), self.ty_shape(ft)?));
                }
                format!("[{}]", out.join(", "))
            }
            None => "null".into(),
        };
        Ok(format!("{{n: [{}], o: {}, e: {}, f: {}, t: {}}}", t.names.iter().map(|n| js_str(n)).collect::<Vec<_>>().join(", "), t.optional, e, f, js_str(&t.text())))
    }

    /// The shapes of the types an annotation names.
    fn need_types_in(&mut self, t: &Ty) -> Result<(), QuilloError> {
        if t.names.iter().any(|n| self.it.types.contains_key(n)) || t.elem.is_some() || t.fields.is_some() {
            self.ty_shape(t)?;
        }
        Ok(())
    }

    fn err<T>(&self, msg: impl Into<String>) -> Result<T, QuilloError> {
        let msg = msg.into();
        let where_ = if self.what.is_empty() { String::new() } else { format!("{} (compiled for the browser): ", self.what) };
        let e = QuilloError::new(format!("{}{}", where_, msg), self.line);
        Err(match &self.file {
            Some(f) => e.in_file(&crate::error::shown_path(f)),
            None => e,
        })
    }

    fn fresh(&mut self, name: &str) -> String {
        self.counter += 1;
        let clean: String = name.chars().map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' }).collect();
        format!("${}_{}", clean, self.counter)
    }

    fn ctx(&mut self) -> &mut FnCtx {
        self.ctxs.last_mut().expect("inside a function")
    }

    fn temp(&mut self) -> String {
        let c = self.ctx();
        c.temps += 1;
        format!("$t{}", c.temps - 1)
    }

    fn push_scope(&mut self) {
        self.ctx().scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.ctx().scopes.pop();
    }

    /// Declares a name in the innermost scope; returns its JS name. A second
    /// `let` of the same name in one scope reuses the variable, as on the server.
    fn declare(&mut self, name: &str, func: Option<bool>) -> String {
        if let Some(Local::Var(js, _)) = self.ctx().scopes.last().and_then(|s| s.get(name)).cloned() {
            self.ctx().scopes.last_mut().unwrap().insert(name.to_string(), Local::Var(js.clone(), func));
            return js;
        }
        let js = self.fresh(name);
        self.ctx().scopes.last_mut().unwrap().insert(name.to_string(), Local::Var(js.clone(), func));
        js
    }

    /// Declares `name` as `local` in the innermost scope.
    fn declare_as(&mut self, name: &str, local: Local) {
        self.ctx().scopes.last_mut().unwrap().insert(name.to_string(), local);
    }

    fn resolve(&mut self, name: &str) -> Result<Resolved, QuilloError> {
        let mut found = None;
        'outer: for (ci, ctx) in self.ctxs.iter().enumerate().rev() {
            for scope in ctx.scopes.iter().rev() {
                match scope.get(name) {
                    Some(Local::Var(js, f)) => found = Some((ci, Resolved::Local(js.clone(), *f))),
                    Some(Local::State(js)) => found = Some((ci, Resolved::State(js.clone()))),
                    Some(Local::Reactive(js)) => found = Some((ci, Resolved::Reactive(js.clone()))),
                    Some(Local::Prop(n)) => found = Some((ci, Resolved::Prop(n.clone()))),
                    None => continue,
                }
                break 'outer;
            }
        }
        if let Some((ci, r)) = found {
            // What it gives can change, and so can what a function reading
            // it gives (`find(xs, (x) => x.id == wanted)`), or a function of
            // the component's own.
            self.last_here = self.ctxs[ci].component.is_some();
            if matches!(r, Resolved::State(_) | Resolved::Reactive(_) | Resolved::Prop(_)) {
                self.reactive = true;
            }
            if let Some(log) = &mut self.read_log {
                match &r {
                    Resolved::State(js) | Resolved::Reactive(js) => log.push(js.clone()),
                    Resolved::Prop(_) => log.push("*".into()),
                    _ => {}
                }
            }
            return Ok(r);
        }
        self.last_here = false;
        let (js, is_async) = self.global(name, self.line)?;
        Ok(Resolved::Global(js, is_async))
    }

    /// A top-level name used by browser code: compiled once, on first use.
    fn global(&mut self, name: &str, line: u32) -> Result<(String, bool), QuilloError> {
        if let Some(g) = self.globals.get(name) {
            return Ok(g.clone());
        }
        // What a global reads is its own, not the code that first used it.
        let saved = std::mem::replace(&mut self.reactive, false);
        let log = self.read_log.take();
        let rows = (self.row.take(), self.cur_row.take(), std::mem::replace(&mut self.row_mask, 0));
        let r = self.global_inner(name, line);
        self.reactive = saved;
        self.read_log = log;
        (self.row, self.cur_row, self.row_mask) = rows;
        r
    }

    fn global_inner(&mut self, name: &str, line: u32) -> Result<(String, bool), QuilloError> {
        if name == "request" {
            return self.err("`request` is only available on the server; pass what you need as a prop");
        }
        let Some(v) = lookup(&self.it.globals, name) else {
            return self.err(format!("unknown name `{}`{}", name, crate::error::removed_note(name)));
        };
        let js = format!("G[{}]", js_str(name));
        // A library's name: what its script defined on the page (branch js-libraries).
        if let (Value::Builtin("__jslib", _), Some(lib)) = (&v, self.it.js_libs.get(name)) {
            let r = (format!("B.js_value({})", js_str(&lib.global)), false);
            self.globals.insert(name.to_string(), r.clone());
            return Ok(r);
        }
        match &v {
            Value::Builtin(b, _) => {
                if CLIENT_BUILTINS.contains(b) {
                    // `parallel` waits for what it calls, and `ask` for an
                    // answer: they are awaited.
                    let r = (format!("B.{}", b), *b == "parallel" || *b == "ask" || *b == "receive" || *b == "markdown");
                    self.globals.insert(name.to_string(), r.clone());
                    Ok(r)
                } else if b.starts_with("native:") {
                    self.err(format!("`{}` is Rust, which runs on the server, not in the browser; call it from a `server fn`", crate::native::display_name(b)))
                } else if *b == "env" {
                    self.err("`env` is not available in the browser; call it from a `server fn`, or read a variable named PUBLIC_... with its name written out: env(\"PUBLIC_MAP_TOKEN\")")
                } else {
                    self.err(format!("`{}` is not available in the browser; call it from a `server fn`", b))
                }
            }
            // The module, not a map of the program's own by its name.
            Value::Map(m)
                if matches!(name, "json" | "math" | "bit" | "stats" | "vec" | "re" | "money" | "channel" | "http" | "date")
                    && m.values().any(|x| matches!(x, Value::Builtin(b, _) if b.strip_prefix(name).map_or(false, |r| r.starts_with('.')))) =>
            {
                let r = (format!("B.{}", name), false);
                self.globals.insert(name.to_string(), r.clone());
                Ok(r)
            }
            Value::Map(m) if matches!(name, "db" | "udp" | "media") && m.values().all(|x| matches!(x, Value::Builtin(..))) => {
                self.err(format!("`{}` is not available in the browser; use it from a `server fn`", name))
            }
            Value::Func(f) if f.def.server => {
                let r = (js.clone(), true);
                self.globals.insert(name.to_string(), r.clone());
                let idem = if crate::idempotency::used_by(&f.def) { ", true" } else { "" };
                self.defs.push(format!("{} = W.server({}{});", js, js_str(name), idem));
                Ok(r)
            }
            Value::Func(f) => {
                let def = f.def.clone();
                // `User(...)`: the record's shape goes with it.
                if self.it.types.contains_key(name) {
                    self.need_type(name)?;
                }
                // Recursive references see the function as not async until
                // known. If it turns out async, it is compiled again with
                // them awaited, and so is what it compiled meanwhile, which
                // may call it back.
                let (defs_before, known): (usize, HashSet<String>) = (self.defs.len(), self.globals.keys().cloned().collect());
                let (views_before, view_defs_before) = (self.views.clone(), self.view_defs.len());
                let mut assume_async = false;
                let (code, is_async) = loop {
                    self.globals.insert(name.to_string(), (js.clone(), assume_async));
                    let saved_ctxs = std::mem::take(&mut self.ctxs);
                    let saved_what = std::mem::replace(
                        &mut self.what,
                        if def.component { format!("<{}>", def.name) } else { format!("`{}`", def.name) },
                    );
                    let saved_line = self.line;
                    let saved_file = self.file.clone();
        if def.file.is_some() {
            self.file = def.file.clone();
        }
                    self.line = def.line.max(line);
                    let res = if def.component { self.component(&def) } else { self.function(&def) };
                    self.ctxs = saved_ctxs;
                    self.what = saved_what;
                    self.line = saved_line;
                    self.file = saved_file;
                    let (code, is_async) = res?;
                    if is_async && !assume_async {
                        self.defs.truncate(defs_before);
                        self.globals.retain(|k, _| known.contains(k));
                        self.views = views_before.clone();
                        self.view_defs.truncate(view_defs_before);
                        assume_async = true;
                        continue;
                    }
                    break (code, is_async);
                };
                let code = if def.component {
                    format!("{} = W.component({}, {});", js, js_str(&def.name), code)
                } else {
                    format!("{} = {};", js, code)
                };
                self.defs.push(code);
                self.globals.insert(name.to_string(), (js.clone(), is_async));
                Ok((js, is_async))
            }
            other => {
                if self.assigned.contains(name) {
                    return self.err(format!(
                        "`{}` is a server variable that the program changes; pass it as a prop or read it with a `server fn`",
                        name
                    ));
                }
                if self.private.contains(name) {
                    return self.err(format!(
                        "`{}` is read from the server's environment, files, database or a service, and would be \
                         sent to every browser; pass what the page needs as a prop or read it with a `server fn`",
                        name
                    ));
                }
                if let Some(bad) = not_sendable(other) {
                    // A constant holding functions (a table of rules, say)
                    // is made in the browser too, from its definition.
                    if let Some(e) = self.definition(name) {
                        return self.global_made(name, &js, e, line);
                    }
                    return self.err(format!("`{}` is {}, which cannot be sent to the browser", name, bad));
                }
                self.defs.push(format!("{} = {};", js, value_js(other)));
                let r = (js, false);
                self.globals.insert(name.to_string(), r.clone());
                Ok(r)
            }
        }
    }

    /// What `let name = ...` at the top of the program gave `name`, for a
    /// name bound once, alone.
    fn definition(&self, name: &str) -> Option<Expr> {
        let mut found = None;
        for stmts in self.it.program.iter() {
            for st in stmts.iter() {
                if let Stmt::Let(Pattern::Bind(n), e, _) = st {
                    if n == name {
                        if found.is_some() {
                            return None;
                        }
                        found = Some(e.clone());
                    }
                }
            }
        }
        found
    }

    /// A constant compiled from its definition: a function of nothing,
    /// called once when the bundle loads.
    fn global_made(&mut self, name: &str, js: &str, e: Expr, line: u32) -> Result<(String, bool), QuilloError> {
        let def = FnDef {
            name: name.to_string(),
            params: Vec::new(),
            ret: None,
            err: None,
            effects: None,
            server: false,
            component: false,
            line,
            body: e,
            nesting: 0,
            file: None,
            hot: Default::default(),
        };
        // Until made, references to it (from functions it holds) find it.
        self.globals.insert(name.to_string(), (js.to_string(), false));
        let saved_ctxs = std::mem::take(&mut self.ctxs);
        let saved_what = std::mem::replace(&mut self.what, format!("`{}`", name));
        let res = self.function(&def);
        self.ctxs = saved_ctxs;
        self.what = saved_what;
        let (code, is_async) = res?;
        if is_async {
            return self.err(format!("`{}` waits on the server to be made, so the browser cannot make it", name));
        }
        self.defs.push(format!("{} = ({})();", js, code));
        Ok((js.to_string(), false))
    }

    fn new_ctx(&mut self, component: Option<String>, is_async: bool) {
        self.ctxs.push(FnCtx { scopes: vec![HashMap::new()], temps: 0, is_async, needs_async: false, component, view: false, hoisted: false, state_init: false, waits_js: false });
    }

    fn temps_decl(n: usize) -> String {
        if n == 0 {
            return String::new();
        }
        format!("let {};\n", (0..n).map(|i| format!("$t{}", i)).collect::<Vec<_>>().join(", "))
    }

    /// Parameter defaults and checks; returns the JS parameter names.
    fn params(&mut self, def: &FnDef, out: &mut String, check_arity: bool) -> Result<Vec<String>, QuilloError> {
        let fname = js_str(&def.name);
        if check_arity {
            let n = def.params.len();
            let _ = writeln!(out, "if (arguments.length > {}) W.arity(arguments.length, {}, {});", n, n, fname);
        }
        let mut names = Vec::new();
        for p in &def.params {
            let js = self.declare(&p.name, None);
            match &p.default {
                Some(d) => {
                    let dv = self.expr(d)?;
                    let _ = writeln!(out, "if ({} === undefined) {} = {};", js, js, dv);
                }
                None => {
                    let _ = writeln!(out, "if ({} === undefined) W.missing({}, {});", js, js_str(&p.name), fname);
                }
            }
            if let Some(ty) = &p.ty {
                self.need_types_in(ty)?;
                let call = format!("W.argty({}, {}, {}, {});", js, ty_js(&Some(ty.clone())), js_str(&p.name), fname);
                // The common types checked in place; `argty` says what is wrong.
                let checks: Option<Vec<String>> = ty
                    .names
                    .iter()
                    .map(|t| match t.as_str() {
                        "int" => Some(format!("Number.isInteger({})", js)),
                        "float" | "num" => Some(format!("typeof {} === \"number\"", js)),
                        "str" => Some(format!("typeof {} === \"string\"", js)),
                        "date" => Some(format!("B.date.valid({}, \"date\")", js)),
                        "bool" => Some(format!("typeof {} === \"boolean\"", js)),
                        "list" => Some(format!("Array.isArray({})", js)),
                        _ => None,
                    })
                    .collect();
                match checks {
                    Some(mut cs) if ty.names.iter().all(|t| t != "any") => {
                        if ty.optional {
                            cs.push(format!("{} == null", js));
                        }
                        let _ = writeln!(out, "if (!({})) {}", cs.join(" || "), call);
                    }
                    _ => {
                        let _ = writeln!(out, "{}", call);
                    }
                }
            }
            names.push(js);
        }
        Ok(names)
    }

    /// A function or view. Compiled once synchronously; if it turns out to
    /// call the server, again as an `async function`.
    fn function(&mut self, def: &FnDef) -> Result<(String, bool), QuilloError> {
        if def.effects.as_ref().map_or(false, |e| e.iter().any(|x| x != "time" && x != "random" && x != "http")) {
            return self.err(format!(
                "`{}` declares `with {}`, which the browser cannot do; make it a `server fn`",
                def.name,
                def.effects.as_ref().unwrap().join(", ")
            ));
        }
        let first = self.function_with(def, false)?;
        if !first.1 {
            return Ok((first.0, false));
        }
        let (code, _) = self.function_with(def, true)?;
        Ok((code, true))
    }

    fn function_with(&mut self, def: &FnDef, is_async: bool) -> Result<(String, bool), QuilloError> {
        let saved_line = self.line;
        self.line = def.line;
        self.new_ctx(None, is_async);
        if self.handler == Some(def as *const FnDef) || self.waiting.contains(&(def as *const FnDef)) {
            self.ctx().waits_js = true;
        }
        let mut pre = String::new();
        let res = (|| -> Result<(Vec<String>, String), QuilloError> {
            let names = self.params(def, &mut pre, true)?;
            let body = self.body(&def.body)?;
            Ok((names, body))
        })();
        let ctx = self.ctxs.pop().unwrap();
        self.line = saved_line;
        let (names, body) = res?;
        let checked = def.ret.is_some() || def.err.is_some();
        for t in def.ret.iter().chain(def.err.iter()) {
            self.need_types_in(t)?;
        }
        let run = if leaves_early(&def.body) {
            format!(
                "{}try {{\n{}}} catch ($e) {{ if ($e instanceof W.Ret) return $e.v; throw $e; }}\n",
                Self::temps_decl(ctx.temps),
                body
            )
        } else {
            format!("{}{}", Self::temps_decl(ctx.temps), body)
        };
        let inner = if checked {
            let call = if is_async { format!("await (async () => {{\n{}}})()", run) } else { format!("(() => {{\n{}}})()", run) };
            format!(
                "return W.retty({}, {}, {}, {});\n",
                call,
                ty_js(&def.ret),
                js_str(&def.name),
                match &def.err {
                    Some(t) => ty_js(&Some(t.clone())),
                    None => "null, false".into(),
                }
            )
        } else {
            run
        };
        let func = format!("{}function ({}) {{\n{}{}}}", if is_async { "async " } else { "" }, names.join(", "), pre, inner);
        // Only named functions need their name and parameters at hand (views
        // are called with props by name); a lambda, made again on each
        // render, goes without.
        let code = if def.name == "<fn>" {
            func
        } else {
            format!(
                "W.meta({}, {}, [{}])",
                func,
                js_str(&def.name),
                def.params.iter().map(|p| js_str(&p.name)).collect::<Vec<_>>().join(", ")
            )
        };
        Ok((code, ctx.needs_async))
    }

    /// A component: its parameters' names and its code.
    fn component(&mut self, def: &FnDef) -> Result<(String, bool), QuilloError> {
        let code = self.setup(def, false)?;
        Ok((format!("[{}], {}", def.params.iter().map(|p| js_str(&p.name)).collect::<Vec<_>>().join(", "), code), false))
    }

    /// A component's code, or (with `view`) a view's for `<Name .../>`: it
    /// runs once, with its props as getters: `function ($i, $p)`, a view's
    /// `function ($p)`.
    fn setup(&mut self, def: &FnDef, view: bool) -> Result<String, QuilloError> {
        self.new_ctx(Some(def.name.clone()), false);
        self.ctx().view = view;
        let saved = (std::mem::take(&mut self.mutable), std::mem::replace(&mut self.reactive, false));
        self.sync_calls.push(sync_called(&def.body));
        let res = self.setup_inner(def);
        self.sync_calls.pop();
        let ctx = self.ctxs.pop().unwrap();
        self.mutable = saved.0;
        self.reactive = saved.1;
        let body = res?;
        Ok(format!("function ({}) {{\n{}{}}}", if view { "$p" } else { "$i, $p" }, Self::temps_decl(ctx.temps), body))
    }

    fn setup_inner(&mut self, def: &FnDef) -> Result<String, QuilloError> {
        let fname = js_str(&def.name);
        let mut out = String::new();
        let mut defaults = Vec::new();
        for p in &def.params {
            if let Some(d) = &p.default {
                let (v, _) = self.reactive_expr(d)?;
                defaults.push(format!("{}: () => ({})", js_str(&p.name), v));
            }
            self.declare_as(&p.name, Local::Prop(p.name.clone()));
        }
        if !defaults.is_empty() {
            let _ = writeln!(out, "$p = W.props($p, {{{}}});", defaults.join(", "));
        }
        for p in &def.params {
            if p.default.is_none() {
                let _ = writeln!(out, "if ($p.{} === undefined) W.missing({}, {});", p.name, js_str(&p.name), fname);
            }
            if let Some(ty) = &p.ty {
                self.need_types_in(ty)?;
                let _ = writeln!(out, "W.argty($p.{}, {}, {}, {});", p.name, ty_js(&Some(ty.clone())), js_str(&p.name), fname);
            }
        }
        // A body that can return early, or changes its own variables as it
        // runs, is made again whole when what it read changes (its state
        // stays).
        let mut exits = false;
        let mut changed = HashSet::new();
        each_expr_in(
            &def.body,
            &mut |e| match e {
                Expr::Try(_) => exits = true,
                Expr::Block(ss) => {
                    for st in ss {
                        match st {
                            Stmt::Return(_) => exits = true,
                            Stmt::Assign(n, ..) => {
                                changed.insert(n.clone());
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            },
            false,
        );
        let states: HashSet<&String> = match &def.body {
            Expr::Block(ss) => ss.iter().filter_map(|st| if let Stmt::State(n, ..) = st { Some(n) } else { None }).collect(),
            _ => HashSet::new(),
        };
        expr_assigned(&def.body, &mut self.mutable);
        if exits || changed.iter().any(|n| !states.contains(n)) {
            self.ctx().hoisted = true;
            let mut copy = String::new();
            for p in &def.params {
                let js = self.declare(&p.name, None);
                let _ = writeln!(copy, "let {} = $p.{};", js, p.name);
            }
            let body = self.body(&def.body)?;
            let _ = writeln!(
                out,
                "return W.frag([() => {{\n{}try {{\n{}}} catch ($e) {{ if ($e instanceof W.Ret) return $e.v; throw $e; }}\n}}]);",
                copy, body
            );
            return Ok(out);
        }
        match &def.body {
            Expr::Block(ss) => {
                self.push_scope();
                let r = self.setup_stmts(ss);
                self.pop_scope();
                out.push_str(&r?);
            }
            e => {
                let v = self.markup(e)?;
                let _ = writeln!(out, "return {};", v);
            }
        }
        Ok(out)
    }

    /// Statements of code that runs once: a `let` that reads what changes
    /// is a memo, a statement that does an effect, and the last value is
    /// markup.
    fn setup_stmts(&mut self, stmts: &[Stmt]) -> Result<String, QuilloError> {
        let mut body = String::new();
        let mut decls: Vec<String> = Vec::new();
        let n = stmts.len();
        let mut pending: Option<(usize, Option<u32>)> = None;
        for (i, st) in stmts.iter().enumerate() {
            if let Some((at, line)) = pending.take() {
                self.mark(&mut body, at, line);
            }
            pending = Some((body.len(), crate::debug::stmt_line(st)));
            let last = i + 1 == n;
            match st {
                // The style is sent as the page's sheet, not in the code.
                Stmt::Expr(_) if style_of(st).is_some() => {}
                Stmt::Expr(e) if last => {
                    let v = self.markup(e)?;
                    let _ = writeln!(body, "return {};", v);
                }
                Stmt::Expr(e) => {
                    let (v, r) = self.reactive_expr(e)?;
                    if r {
                        let _ = writeln!(body, "W.effect(() => {{ {}; }});", v);
                    } else {
                        let _ = writeln!(body, "{};", v);
                    }
                }
                Stmt::Let(p, e, line) if !matches!(e, Expr::Lambda(_)) && !matches!(p, Pattern::Bind(n) if self.mutable.contains(n)) => {
                    self.line = *line;
                    let (v, r) = self.reactive_expr(e)?;
                    if !r {
                        pending = None;
                        self.stmt(st, last, &mut body, &mut decls)?;
                        continue;
                    }
                    if let Pattern::Bind(name) = p {
                        let m = self.fresh(name);
                        let _ = writeln!(body, "const {} = W.memo(() => ({}));", m, v);
                        self.declare_as(name, Local::Reactive(m));
                    } else {
                        let pat = self.pattern(p)?;
                        let b = self.fresh("b");
                        let _ = writeln!(
                            body,
                            "const {} = W.memo(() => {{ const $v = {}, $b = W.match({}, $v); if ($b === null) W.fail(\"`let` pattern does not match \" + W.short($v)); return $b; }});",
                            b, v, pat
                        );
                        let mut names = Vec::new();
                        pattern_names(p, &mut names);
                        for n in names {
                            let m = self.fresh(&n);
                            let _ = writeln!(body, "const {} = W.memo(() => {}.v[{}]);", m, b, js_str(&n));
                            self.declare_as(&n, Local::Reactive(m));
                        }
                    }
                    if last {
                        body.push_str("return null;\n");
                    }
                }
                _ => {
                    pending = None;
                    self.stmt(st, last, &mut body, &mut decls)?
                }
            }
        }
        if let Some((at, line)) = pending {
            self.mark(&mut body, at, line);
        }
        if stmts.is_empty() {
            body.push_str("return null;\n");
        }
        decls.sort();
        decls.dedup();
        let head = if decls.is_empty() { String::new() } else { format!("let {};\n", decls.join(", ")) };
        Ok(head + &body)
    }

    /// A function body as statements ending in `return`.
    fn body(&mut self, e: &Expr) -> Result<String, QuilloError> {
        match e {
            Expr::Block(stmts) => self.stmts(stmts),
            other => {
                let mut out = format!("return {};\n", self.expr(other)?);
                self.mark(&mut out, 0, Some(crate::debug::expr_line(other)).filter(|&l| l > 0));
                Ok(out)
            }
        }
    }

    /// Statements of a block, in a new scope, returning the last value.
    fn stmts(&mut self, stmts: &[Stmt]) -> Result<String, QuilloError> {
        self.push_scope();
        let res = self.stmts_inner(stmts);
        self.pop_scope();
        res
    }

    fn stmts_inner(&mut self, stmts: &[Stmt]) -> Result<String, QuilloError> {
        let mut body = String::new();
        let mut decls: Vec<String> = Vec::new();
        let n = stmts.len();
        for (i, s) in stmts.iter().enumerate() {
            self.stmt(s, i + 1 == n, &mut body, &mut decls)?;
        }
        if stmts.is_empty() {
            body.push_str("return null;\n");
        }
        decls.sort();
        decls.dedup();
        let head = if decls.is_empty() { String::new() } else { format!("let {};\n", decls.join(", ")) };
        Ok(head + &body)
    }

    /// One statement into `body` (the last returns its value); variables it
    /// declares into `decls`.
    fn stmt(&mut self, s: &Stmt, last: bool, body: &mut String, decls: &mut Vec<String>) -> Result<(), QuilloError> {
        let start = body.len();
        self.stmt_code(s, last, body, decls)?;
        self.mark(body, start, crate::debug::stmt_line(s));
        Ok(())
    }

    /// In development, the line a statement's code comes from, where it starts.
    fn mark(&self, body: &mut String, at: usize, line: Option<u32>) {
        if let (true, Some(line)) = (self.dev, line) {
            body.insert_str(at, &format!("{}{}{}", MARK, line, MARK));
        }
    }

    fn stmt_code(&mut self, s: &Stmt, last: bool, body: &mut String, decls: &mut Vec<String>) -> Result<(), QuilloError> {
        {
            match s {
                Stmt::Expr(_) if style_of(s).is_some() => {}
                Stmt::Expr(e) => {
                    let v = self.expr(e)?;
                    if last {
                        let _ = writeln!(body, "return {};", v);
                    } else {
                        let _ = writeln!(body, "{};", v);
                    }
                }
                Stmt::Let(p, e, line) => {
                    self.line = *line;
                    if let (Pattern::Bind(n), Expr::Lambda(d)) = (p, e) {
                        let not_sync = self.sync_calls.last().map_or(false, |s| !s.contains(n));
                        let c = self.ctx();
                        let own = c.component.is_some() && !c.view && not_sync;
                        if own || c.waits_js {
                            self.waiting.insert(&**d as *const FnDef);
                        }
                    }
                    let func = if let (Pattern::Bind(_), Expr::Lambda(d)) = (p, e) { Some(self.lambda_is_async(d)?) } else { None };
                    let v = self.expr(e)?;
                    if let Pattern::Bind(n) = p {
                        let js = self.declare(n, func);
                        decls.push(js.clone());
                        let _ = writeln!(body, "{} = {};", js, v);
                    } else {
                        let t = self.temp();
                        let mut names = Vec::new();
                        if let Some(cond) = simple_pattern(p, &t, &mut names) {
                            let _ = writeln!(body, "{} = {};", t, v);
                            if !cond.is_empty() {
                                let _ = writeln!(body, "if (!({})) W.fail(\"`let` pattern does not match \" + W.short({}));", cond, t);
                            }
                            for (n, at) in names {
                                let js = self.declare(&n, None);
                                decls.push(js.clone());
                                let _ = writeln!(body, "{} = {};", js, at);
                            }
                        } else {
                            let pat = self.pattern(p)?;
                            let _ = writeln!(body, "{} = W.match({}, {});", t, pat, v);
                            let _ = writeln!(body, "if ({} === null) W.fail(\"`let` pattern does not match \" + W.short({}));", t, v);
                            let mut all = Vec::new();
                            pattern_names(p, &mut all);
                            for n in all {
                                let js = self.declare(&n, None);
                                decls.push(js.clone());
                                let _ = writeln!(body, "{} = {}[{}];", js, t, js_str(&n));
                            }
                        }
                    }
                    if last {
                        body.push_str("return null;\n");
                    }
                }
                Stmt::SetField(o, name, op, e, line) => {
                    self.line = *line;
                    let o = self.expr(o)?;
                    let rhs = self.expr(e)?;
                    let op = match op {
                        AssignOp::Set => "",
                        AssignOp::Add => ", \"+\"",
                        AssignOp::Sub => ", \"-\"",
                    };
                    let _ = writeln!(body, "W.setf({}, {}, {}{});", o, js_str(name), rhs, op);
                    if last {
                        body.push_str("return null;\n");
                    }
                }
                Stmt::Assign(name, op, e, line) => {
                    self.line = *line;
                    let rhs = self.expr(e)?;
                    let target = self.resolve(name)?;
                    let cur = match &target {
                        Resolved::Local(js, _) => js.clone(),
                        Resolved::State(js) => format!("{}.v", js),
                        Resolved::Reactive(_) | Resolved::Prop(_) => {
                            return self.err(format!(
                                "cannot assign to `{}`: it follows other values (a prop, a loop's item, a `let` of state); keep it in state: state {} = ...",
                                name, name
                            ))
                        }
                        Resolved::Global(..) => {
                            return self.err(format!(
                                "cannot assign to `{}` in the browser: it is a server variable (use `state {}` in the component, or a `server fn`)",
                                name, name
                            ))
                        }
                    };
                    let v = match op {
                        AssignOp::Set => rhs,
                        AssignOp::Add => format!("W.add({}, {})", cur, rhs),
                        AssignOp::Sub => format!("W.sub({}, {})", cur, rhs),
                    };
                    match target {
                        Resolved::State(js) => {
                            let _ = writeln!(body, "{}.set({});", js, v);
                        }
                        _ => {
                            let _ = writeln!(body, "{} = {};", cur, v);
                        }
                    }
                    if last {
                        body.push_str("return null;\n");
                    }
                }
                Stmt::Fn(def) => {
                    self.line = def.line;
                    if def.server || def.component {
                        return self.err("`server fn` and `component` must be declared at the top level");
                    }
                    let is_async = self.lambda_is_async(def)?;
                    let js = self.declare(&def.name, Some(is_async));
                    decls.push(js.clone());
                    let (code, _) = self.function(def)?;
                    let _ = writeln!(body, "{} = {};", js, code);
                    if last {
                        body.push_str("return null;\n");
                    }
                }
                Stmt::State(name, e, line) => {
                    self.line = *line;
                    if self.ctxs.len() != 1 || self.ctx().scopes.len() != 2 || self.ctx().component.is_none() || self.ctx().view {
                        return self.err("`state` can only be declared directly in a component's body");
                    }
                    let sig = self.fresh(name);
                    // `state x = live(load, ...args)`: a subscription.
                    if let Some((fname, args)) = self.live_call(e)? {
                        let mut argv = Vec::new();
                        for a in args {
                            argv.push(self.reactive_expr(a)?.0);
                        }
                        let _ = writeln!(body, "const {} = W.live($i, {}, {}, () => [{}]);", sig, js_str(name), js_str(&fname), argv.join(", "));
                        self.declare_as(name, Local::State(sig));
                        if last {
                            body.push_str("return null;\n");
                        }
                        return Ok(());
                    }
                    let saved_reactive = self.reactive;
                    // The initializer may wait on the server; it runs once.
                    self.new_ctx(None, false);
                    self.ctx().state_init = true;
                    // Let the initializer see the component's names.
                    let outer = std::mem::take(&mut self.ctxs[0].scopes);
                    self.ctxs.last_mut().unwrap().scopes = outer.clone();
                    self.ctxs.last_mut().unwrap().scopes.push(HashMap::new());
                    let first = self.expr(e);
                    let mut init_ctx = self.ctxs.pop().unwrap();
                    let mut init = first?;
                    if init_ctx.needs_async {
                        self.ctxs.push(FnCtx { scopes: outer.clone(), temps: 0, is_async: true, needs_async: false, component: None, view: false, hoisted: false, state_init: true, waits_js: false });
                        self.ctxs.last_mut().unwrap().scopes.push(HashMap::new());
                        let again = self.expr(e);
                        init_ctx = self.ctxs.pop().unwrap();
                        init = again?;
                    }
                    self.ctxs[0].scopes = outer;
                    self.reactive = saved_reactive;
                    // An initializer that reads the page runs again in the
                    // browser when the component is resumed (FEEDBACK18.md,
                    // Q-105; branch feedback18-fixes).
                    let page = ["B.js(", "B.js_new(", "B.js_value("].iter().any(|f| init.contains(f));
                    let _ = writeln!(
                        body,
                        "const {} = $i.sig({}, {}() => {{ {}return {}; }}{});",
                        sig,
                        js_str(name),
                        if init_ctx.is_async { "async " } else { "" },
                        Self::temps_decl(init_ctx.temps),
                        init,
                        if page { ", true" } else { "" }
                    );
                    self.declare_as(name, Local::State(sig));
                    if last {
                        body.push_str("return null;\n");
                    }
                }
                Stmt::Return(e) => {
                    let v = match e {
                        Some(e) => self.expr(e)?,
                        None => "null".into(),
                    };
                    let _ = writeln!(body, "throw new W.Ret({});", v);
                }
                Stmt::Route(r) => {
                    self.line = r.line;
                    return self.err("routes cannot be declared in browser code");
                }
                Stmt::Use(_, _, line, _) => {
                    self.line = *line;
                    return self.err("`use` must be at the top level");
                }
                Stmt::Mount(_, _, line) => {
                    self.line = *line;
                    return self.err("`mount` must be at the top level");
                }
                Stmt::Service(_, _, _, _, line) => {
                    self.line = *line;
                    return self.err("`service` must be at the top level");
                }
                Stmt::Type(td) => {
                    self.line = td.line;
                    return self.err("`type` must be at the top level");
                }
            }
        }
        Ok(())
    }

    /// `live(server_fn, ...args)`, if `e` is such a call: the function's
    /// name and the arguments.
    fn live_call<'e>(&mut self, e: &'e Expr) -> Result<Option<(String, &'e [Expr])>, QuilloError> {
        let Expr::Call(callee, args, line) = e else { return Ok(None) };
        let Expr::Ident(n, _) = &**callee else { return Ok(None) };
        if n != "live" || self.ctxs.iter().any(|c| c.scopes.iter().any(|s| s.contains_key("live"))) {
            return Ok(None);
        }
        if !matches!(lookup(&self.it.globals, "live"), Some(Value::Builtin("live", _))) {
            return Ok(None);
        }
        self.line = *line;
        let fname = match args.first() {
            Some(Expr::Ident(f, _)) => match lookup(&self.it.globals, f) {
                Some(Value::Func(c)) if c.def.server => f.clone(),
                _ => return self.err(format!("`live` needs a server fn: `{}` is not one", f)),
            },
            _ => return self.err("`live` needs a server fn by name: state items = live(load_items, ...)"),
        };
        Ok(Some((fname, &args[1..])))
    }

    /// Whether a function waits on the server (it calls a `server fn`, or a
    /// function that does). Compiles it in a scratch context to find out.
    fn lambda_is_async(&mut self, def: &FnDef) -> Result<bool, QuilloError> {
        let saved_counter = self.counter;
        let saved_defs = self.defs.len();
        let r = self.function_with(def, false).map(|(_, a)| a);
        self.counter = saved_counter.max(self.counter);
        let _ = saved_defs;
        r
    }

    /// `{ ... }` in expression position.
    fn block_expr(&mut self, stmts: &[Stmt]) -> Result<String, QuilloError> {
        let inner = self.stmts(stmts)?;
        Ok(self.iife(&inner))
    }

    fn iife(&mut self, body: &str) -> String {
        if self.ctx().is_async {
            format!("(await (async () => {{\n{}}})())", body)
        } else {
            format!("(() => {{\n{}}})()", body)
        }
    }

    fn expr(&mut self, e: &Expr) -> Result<String, QuilloError> {
        Ok(match e {
            Expr::Lit(l) => lit_js(l),
            Expr::Template(parts) => {
                let mut out = Vec::new();
                for p in parts {
                    match p {
                        TPart::Lit(s) => out.push(js_str(s)),
                        TPart::Expr(x) => out.push(format!("W.display({})", self.expr(x)?)),
                    }
                }
                if out.is_empty() {
                    "\"\"".into()
                } else {
                    format!("({})", out.join(" + "))
                }
            }
            Expr::Ident(n, line) => {
                self.line = *line;
                match self.resolve(n)? {
                    Resolved::Local(js, _) => js,
                    Resolved::State(js) | Resolved::Reactive(js) => format!("{}.v", js),
                    Resolved::Prop(n) => format!("$p.{}", n),
                    // Passed around, it could be called with any name.
                    Resolved::Global(js, _) if js == "B.js" || js == "B.js_new" => {
                        return self.err(format!("`{}` can only be called, with the name of what it calls written out", n));
                    }
                    Resolved::Global(js, _) => js,
                }
            }
            Expr::List(items) => {
                let mut out = Vec::new();
                for i in items {
                    out.push(self.expr(i)?);
                }
                format!("[{}]", out.join(", "))
            }
            Expr::Map(entries) => {
                let mut out = Vec::new();
                for (k, v) in entries {
                    // `"__proto__": v` would set the prototype; `["__proto__"]: v` is a key.
                    if k == "__proto__" {
                        out.push(format!("[{}]: {}", js_str(k), self.expr(v)?));
                    } else {
                        out.push(format!("{}: {}", js_str(k), self.expr(v)?));
                    }
                }
                format!("{{{}}}", out.join(", "))
            }
            Expr::Unary(op, x, line) => {
                self.line = *line;
                let v = self.expr(x)?;
                match op {
                    UnOp::Not => format!("!W.truthy({})", v),
                    UnOp::Neg => format!("W.neg({})", v),
                }
            }
            Expr::Binary(op @ (BinOp::Eq | BinOp::Ne), a, b, line) if self.state_of(a).is_some() || self.state_of(b).is_some() => {
                self.line = *line;
                // `x == state`: what reads it hears only when the state
                // becomes x or stops being it (see `is` in the runtime).
                let (s, other) = match self.state_of(a) {
                    Some(s) => {
                        let _ = self.resolve(match &**a { Expr::Ident(n, _) => n, _ => unreachable!() })?;
                        (s, self.expr(b)?)
                    }
                    None => {
                        let x = self.expr(a)?;
                        let _ = self.resolve(match &**b { Expr::Ident(n, _) => n, _ => unreachable!() })?;
                        (self.state_of(b).unwrap(), x)
                    }
                };
                format!("{}W.is({}, {})", if matches!(op, BinOp::Ne) { "!" } else { "" }, s, other)
            }
            Expr::Binary(op, a, b, line) => {
                self.line = *line;
                let x = self.expr(a)?;
                let y = self.expr(b)?;
                match op {
                    BinOp::And => {
                        let t = self.temp();
                        format!("(W.truthy({t} = {x}) ? {y} : {t})")
                    }
                    BinOp::Or => {
                        let t = self.temp();
                        format!("(W.truthy({t} = {x}) ? {t} : {y})")
                    }
                    BinOp::Eq => format!("W.eq({}, {})", x, y),
                    BinOp::Ne => format!("!W.eq({}, {})", x, y),
                    BinOp::Add => format!("W.add({}, {})", x, y),
                    BinOp::Sub => format!("W.sub({}, {})", x, y),
                    BinOp::Mul => format!("W.mul({}, {})", x, y),
                    BinOp::Div => format!("W.div({}, {})", x, y),
                    BinOp::Mod => format!("W.mod({}, {})", x, y),
                    BinOp::Lt => format!("W.lt({}, {})", x, y),
                    BinOp::Le => format!("W.le({}, {})", x, y),
                    BinOp::Gt => format!("W.gt({}, {})", x, y),
                    BinOp::Ge => format!("W.ge({}, {})", x, y),
                    BinOp::Range => format!("W.range({}, {})", x, y),
                }
            }
            Expr::Coalesce(a, b) => {
                let t = self.temp();
                let x = self.expr(a)?;
                let y = self.expr(b)?;
                format!("((({t} = {x}) === null || {t} === undefined || {t} instanceof W.Err) ? {y} : {t})")
            }
            Expr::Pipe(lhs, rhs, line) => {
                self.line = *line;
                let v = self.expr(lhs)?;
                match &**rhs {
                    Expr::Call(callee, args, l) => {
                        self.line = *l;
                        self.call(callee, args, Some(v))?
                    }
                    other => {
                        let f = self.callee_value(other)?;
                        self.await_if(f.1, format!("W.call({}, [{}])", f.0, v))
                    }
                }
            }
            Expr::Call(callee, args, line) => {
                self.line = *line;
                // `raw(t(key, vars))`: the values put in are escaped (as on the server).
                let is = |c: &mut Self, e: &Expr, name: &str| -> Result<bool, QuilloError> {
                    Ok(matches!(e, Expr::Ident(n, _) if n == name) && matches!(c.resolve(name)?, Resolved::Global(js, _) if js == format!("B.{}", name)))
                };
                match args.as_slice() {
                    [Expr::Call(inner, targs, _)] if is(self, callee, "raw")? && is(self, inner, "t")? => {
                        let vs = targs.iter().map(|a| self.expr(a)).collect::<Result<Vec<_>, _>>()?;
                        format!("B.t_html({})", vs.join(", "))
                    }
                    _ => self.call(callee, args, None)?,
                }
            }
            Expr::Field(obj, name, line) => {
                self.line = *line;
                // `Chart.defaults`: read from the library (branch js-libraries).
                if let Some(path) = self.lib_path(e)? {
                    self.js_name("js", Some(&Expr::Lit(Lit::Str(path.as_str().into()))))?;
                    return Ok(format!("B.js_value({})", js_str(&path)));
                }
                let o = self.expr(obj)?;
                let mut fb = self.fallback(name);
                // `x.js` is no call of `js` with the name x holds.
                if fb == "B.js" || fb == "B.js_new" {
                    fb = "null".into();
                }
                // Names Object.prototype has need the careful check for a
                // map's own key; any other is read directly when present.
                const PROTO: &[&str] = &[
                    "constructor", "hasOwnProperty", "isPrototypeOf", "propertyIsEnumerable", "toLocaleString", "toString",
                    "valueOf", "__proto__", "__defineGetter__", "__defineSetter__", "__lookupGetter__", "__lookupSetter__",
                ];
                if PROTO.contains(&name.as_str()) {
                    format!("W.field({}, {}, {})", o, js_str(name), fb)
                } else {
                    format!("W.fieldp({}, {}, {})", o, js_str(name), fb)
                }
            }
            Expr::Index(obj, idx, line) => {
                self.line = *line;
                format!("W.index({}, {})", self.expr(obj)?, self.expr(idx)?)
            }
            Expr::Try(x) => format!("W.tryv({})", self.expr(x)?),
            Expr::If(c, a, b) => {
                let cv = self.expr(c)?;
                let av = self.branch(a)?;
                let bv = match b {
                    Some(b) => self.branch(b)?,
                    None => "null".into(),
                };
                format!("(W.truthy({}) ? {} : {})", cv, av, bv)
            }
            Expr::Match(subj, arms, line) => {
                self.line = *line;
                let s = self.expr(subj)?;
                let mut body = format!("const $v = {};\nlet $b;\n", s);
                for arm in arms {
                    let pat = self.pattern(&arm.pat)?;
                    self.push_scope();
                    let mut names = Vec::new();
                    pattern_names(&arm.pat, &mut names);
                    let mut binds = String::new();
                    for n in &names {
                        let js = self.declare(n, None);
                        let _ = write!(binds, "const {} = $b[{}]; ", js, js_str(n));
                    }
                    let guard = match &arm.guard {
                        Some(g) => Some(self.expr(g)?),
                        None => None,
                    };
                    let b = self.expr(&arm.body);
                    self.pop_scope();
                    let b = b?;
                    match guard {
                        Some(g) => {
                            let _ = writeln!(body, "if (($b = W.match({}, $v)) !== null) {{ {}if (W.truthy({})) return {}; }}", pat, binds, g, b);
                        }
                        None => {
                            let _ = writeln!(body, "if (($b = W.match({}, $v)) !== null) {{ {}return {}; }}", pat, binds, b);
                        }
                    }
                }
                body.push_str("W.fail(\"no `match` arm matches \" + W.short($v));\n");
                self.iife(&body)
            }
            Expr::For(pat, iter, body, line) => {
                self.line = *line;
                self.push_scope();
                let r = self.loop_head(pat, iter).and_then(|(head, pre, pat, elem)| {
                    let binds = self.bind_pattern(pat, &elem, "`for` pattern does not match ")?;
                    Ok((head, pre + &binds, self.expr(body)?))
                });
                self.pop_scope();
                let (head, binds, b) = r?;
                let inner = format!("const $out = [];\n{} {{ {}$out.push({}); }}\nreturn $out;\n", head, binds, b);
                self.iife(&inner)
            }
            Expr::While(c, body, line) => {
                self.line = *line;
                let cv = self.expr(c)?;
                let b = self.expr(body)?;
                self.iife(&format!("while (W.truthy({})) {{ {}; }}\nreturn null;\n", cv, b))
            }
            Expr::Block(stmts) => self.block_expr(stmts)?,
            Expr::Lambda(def) => {
                let (code, _) = self.function(def)?;
                code
            }
            Expr::Reply(_, _, line) => {
                self.line = *line;
                return self.err("`reply` only works on the server");
            }
            Expr::Html(el, _) => self.html(el)?,
        })
    }

    /// The signal of the state `e` names, if it is a component's state.
    fn state_of(&self, e: &Expr) -> Option<String> {
        let Expr::Ident(n, _) = e else { return None };
        for ctx in self.ctxs.iter().rev() {
            for scope in ctx.scopes.iter().rev() {
                if let Some(l) = scope.get(n) {
                    return if let Local::State(js) = l { Some(js.clone()) } else { None };
                }
            }
        }
        None
    }

    /// The head of a loop over `iter` (in the loop's scope), what its body
    /// starts with, the pattern each item is matched with, and the item's
    /// JS. `enumerate(xs)` and `range(a, b)` (or `a..b`) are looped over by
    /// number, without making the list.
    fn loop_head<'p>(&mut self, pat: &'p Pattern, iter: &Expr) -> Result<(String, String, &'p Pattern, String), QuilloError> {
        let builtin = |c: &mut Self, callee: &Expr, name: &str| -> Result<bool, QuilloError> {
            Ok(matches!(callee, Expr::Ident(n, _) if n == name) && matches!(c.resolve(name)?, Resolved::Global(js, _) if js == format!("B.{}", name)))
        };
        let plain = |c: &mut Self| -> Result<(String, String, &'p Pattern, String), QuilloError> {
            let l = c.expr(iter)?;
            Ok((format!("for (const $x of W.iter({}))", l), String::new(), pat, "$x".into()))
        };
        match iter {
            Expr::Call(callee, args, _) if args.len() == 1 && builtin(self, callee, "enumerate")? => {
                let Pattern::List(ps, None) = pat else { return plain(self) };
                let [i, x] = ps.as_slice() else { return plain(self) };
                let mut names = Vec::new();
                if simple_pattern(i, "$k", &mut names).map_or(true, |c| !c.is_empty()) {
                    return plain(self);
                }
                let l = self.expr(&args[0])?;
                let mut pre = String::new();
                for (n, at) in names {
                    let js = self.declare(&n, None);
                    let _ = write!(pre, "const {} = {}; ", js, at);
                }
                let _ = write!(pre, "const $x = $l[$k]; ");
                Ok((format!("const $l = W.enumlist({});\nfor (let $k = 0; $k < $l.length; $k++)", l), pre, x, "$x".into()))
            }
            Expr::Call(callee, args, _) if (args.len() == 1 || args.len() == 2) && builtin(self, callee, "range")? => {
                let a = self.expr(&args[0])?;
                let b = if args.len() == 2 { self.expr(&args[1])? } else { "undefined".into() };
                Ok((format!("const $r = W.span({}, {}, false);\nfor (let $x = $r[0]; $x < $r[1]; $x++)", a, b), String::new(), pat, "$x".into()))
            }
            Expr::Binary(BinOp::Range, a, b, _) => {
                let a = self.expr(a)?;
                let b = self.expr(b)?;
                Ok((format!("const $r = W.span({}, {}, true);\nfor (let $x = $r[0]; $x < $r[1]; $x++)", a, b), String::new(), pat, "$x".into()))
            }
            _ => plain(self),
        }
    }

    /// Binds `p`'s names to what `v` (a JS variable) holds, failing with
    /// `what` when it does not match.
    fn bind_pattern(&mut self, p: &Pattern, v: &str, what: &str) -> Result<String, QuilloError> {
        let mut names = Vec::new();
        let mut out = String::new();
        if let Some(cond) = simple_pattern(p, v, &mut names) {
            if !cond.is_empty() {
                let _ = write!(out, "if (!({})) W.fail({} + W.short({})); ", cond, js_str(what), v);
            }
            for (n, at) in names {
                let js = self.declare(&n, None);
                let _ = write!(out, "const {} = {}; ", js, at);
            }
            return Ok(out);
        }
        let pj = self.pattern(p)?;
        let _ = write!(out, "const $b = W.match({}, {}); if ($b === null) W.fail({} + W.short({})); ", pj, v, js_str(what), v);
        let mut all = Vec::new();
        pattern_names(p, &mut all);
        for n in all {
            let js = self.declare(&n, None);
            let _ = write!(out, "const {} = $b[{}]; ", js, js_str(&n));
        }
        Ok(out)
    }

    /// A branch of `if`: blocks become IIFEs, other expressions stay inline.
    fn branch(&mut self, e: &Expr) -> Result<String, QuilloError> {
        self.expr(e)
    }

    fn await_if(&mut self, is_async: bool, call: String) -> String {
        if !is_async {
            return call;
        }
        if self.ctx().component.is_some() {
            // Reported by `call`, which knows the name.
            return call;
        }
        let c = self.ctx();
        c.needs_async = true;
        if c.is_async {
            format!("(await {})", call)
        } else {
            call
        }
    }

    /// What `name` means as a function for `x.name` and `x.name()`, or "null".
    fn fallback(&mut self, name: &str) -> String {
        for ctx in self.ctxs.iter().rev() {
            for scope in ctx.scopes.iter().rev() {
                match scope.get(name) {
                    Some(Local::Var(js, _)) => return js.clone(),
                    Some(Local::State(js) | Local::Reactive(js)) => return format!("{}.v", js),
                    Some(Local::Prop(n)) => return format!("$p.{}", n),
                    None => {}
                }
            }
        }
        match lookup(&self.it.globals, name) {
            Some(Value::Builtin(b, _)) if CLIENT_BUILTINS.contains(&b) => format!("B.{}", b),
            Some(Value::Func(f)) if !f.def.component => {
                let line = self.line;
                match self.global(name, line) {
                    Ok((js, _)) => js,
                    Err(_) => "null".into(),
                }
            }
            _ => "null".into(),
        }
    }

    /// The function value an expression names, and whether calling it waits.
    fn callee_value(&mut self, e: &Expr) -> Result<(String, bool), QuilloError> {
        if let Expr::Ident(n, line) = e {
            self.line = *line;
            return Ok(match self.resolve(n)? {
                Resolved::Local(js, f) => (js, f.unwrap_or(false)),
                Resolved::State(js) | Resolved::Reactive(js) => (format!("{}.v", js), false),
                Resolved::Prop(n) => (format!("$p.{}", n), false),
                Resolved::Global(js, a) => (js, a),
            });
        }
        Ok((self.expr(e)?, false))
    }

    fn call(&mut self, callee: &Expr, args: &[Expr], pre: Option<String>) -> Result<String, QuilloError> {
        if let Some(path) = self.lib_path(callee)? {
            return self.lib_call(&path, args, pre);
        }
        let mut argv: Vec<String> = pre.into_iter().collect();
        if let Expr::Field(obj, name, _) = callee {
            let saved = std::mem::replace(&mut self.keep_promise, matches!(name.as_str(), "then" | "catch" | "finally"));
            let recv = self.expr(obj);
            self.keep_promise = saved;
            let recv = recv?;
            let fb = self.fallback(name);
            if fb == "B.js" || fb == "B.js_new" {
                return self.err(format!("`{}` needs the name of what it calls written out, like {}(\"Chart\", ...)", name, name));
            }
            for a in args {
                argv.push(self.expr(a)?);
            }
            if recv == "B.http" {
                return self.http_call(name, argv);
            }
            let is_async = self.is_async_name(name);
            let call = format!("W.mcall({}, {}, {}, [{}])", recv, js_str(name), fb, argv.join(", "));
            return self.finish_call(name, is_async, call);
        }
        if let Some(name) = public_env(callee, args).filter(|_| !self.is_local("env")) {
            // Written into the bundle as it is now: the name says it is for
            // everyone to see.
            return Ok(match std::env::var(name) {
                Ok(v) => js_str(&v),
                Err(_) => match args.get(1) {
                    Some(d) => self.expr(d)?,
                    None => "null".into(),
                },
            });
        }
        // `waiting(grid)`: whether the live state `grid` waits for the answer
        // to new arguments (or its first, in the browser): read from its
        // signal, and followed (FEEDBACK13.md, Q-091; branch feedback13-fixes).
        if let (Expr::Ident(n, _), [Expr::Ident(var, _)]) = (callee, args) {
            if n == "waiting" && argv.is_empty() && matches!(self.resolve(n)?, Resolved::Global(js, _) if js == "B.waiting") {
                if let Resolved::State(js) = self.resolve(var)? {
                    self.reactive = true;
                    if let Some(log) = &mut self.read_log {
                        log.push("*".into());
                    }
                    return Ok(format!("W.waiting({})", js));
                }
                return self.err(format!("`waiting` takes a component's live state by its name (`state {} = live(...)`)", var));
            }
        }
        let (f, is_async) = self.callee_value(callee)?;
        // The component's own functions, the program's and url() may read
        // what changes: what calls them follows it.
        if matches!(callee, Expr::Ident(..)) && (self.last_here || f.starts_with("G[") || f == "B.url") {
            self.reactive = true;
            if let Some(log) = &mut self.read_log {
                log.push("*".into());
            }
        }
        if f == "B.js" || f == "B.js_new" {
            // `name | js(...)` would take the name from the pipeline.
            let name = if argv.is_empty() { args.first() } else { None };
            self.js_name(&f[2..], name)?;
        }
        for a in args {
            argv.push(self.expr(a)?);
        }
        let name = match callee {
            Expr::Ident(n, _) => n.clone(),
            _ => "this function".into(),
        };
        let call = if f.starts_with("B.") || f.starts_with("G[") {
            format!("{}({})", f, argv.join(", "))
        } else {
            format!("W.call({}, [{}])", f, argv.join(", "))
        };
        // A Promise from the page's function is waited for where code can
        // wait (an event handler, @mount, a state's initializer), as a
        // `server fn` is: its rejection an error value (FEEDBACK6.md, Q-057;
        // branch feedback6-fixes). Elsewhere, and before `.then`, it stays a
        // Promise.
        if (f == "B.js" || f == "B.js_new") && !self.keep_promise && (self.ctx().waits_js || self.ctx().state_init) {
            return self.finish_call(&name, true, format!("W.settle({})", call));
        }
        self.finish_call(&name, is_async, call)
    }

    /// `Chart`, `L.map`, `Chart.new`: a name a browser library gives, and the
    /// fields after it, as the dotted path on the page; none for anything else
    /// (branch js-libraries).
    fn lib_path(&mut self, e: &Expr) -> Result<Option<String>, QuilloError> {
        match e {
            Expr::Ident(n, _) => {
                if !self.it.js_libs.contains_key(n) {
                    return Ok(None);
                }
                Ok(match self.resolve(n)? {
                    Resolved::Global(js, _) if js.starts_with("B.js_value(") => self.it.js_libs.get(n).map(|l| l.global.clone()),
                    _ => None,
                })
            }
            Expr::Field(obj, name, _) => Ok(self.lib_path(obj)?.map(|p| format!("{}.{}", p, name))),
            _ => Ok(None),
        }
    }

    /// `Chart.new(el, config)` is `js_new("Chart", el, config)`, and `L.map(el)`
    /// is `js("L.map", el)`: the same checks, written as JavaScript reads
    /// (branch js-libraries).
    fn lib_call(&mut self, path: &str, args: &[Expr], pre: Option<String>) -> Result<String, QuilloError> {
        let (f, path) = match path.strip_suffix(".new") {
            Some(p) => ("js_new", p),
            None => ("js", path),
        };
        self.js_name(f, Some(&Expr::Lit(Lit::Str(path.into()))))?;
        let mut argv = vec![js_str(path)];
        argv.extend(pre);
        for a in args {
            argv.push(self.expr(a)?);
        }
        let call = format!("B.{}({})", f, argv.join(", "));
        // Its Promise is waited for where code can wait, as `js(...)`'s is.
        if !self.keep_promise && (self.ctx().waits_js || self.ctx().state_init) {
            return self.finish_call(path, true, format!("W.settle({})", call));
        }
        self.finish_call(path, false, call)
    }

    /// `http.get(...)` and the rest, which the browser makes once the page
    /// has arrived: in handlers and the functions they call, not while the
    /// server renders (see src/fetch.rs).
    fn http_call(&mut self, name: &str, argv: Vec<String>) -> Result<String, QuilloError> {
        if !matches!(name, "get" | "delete" | "post" | "put" | "patch" | "request") {
            return self.err(match name {
                "stream" => "`http.stream` is the server's; in the browser, `receive(url, f, ...)` in a handler and `listen(url, f)` read a stream".to_string(),
                _ => format!("`http` has no function `{}`", name),
            });
        }
        let c = self.ctx();
        if c.component.is_some() || c.state_init {
            return self.err(format!(
                "`http.{}` in a component runs in the browser, once the page has arrived, and the server renders \
                 the component first: call it from an event handler or @mount, or from a `server fn`",
                name
            ));
        }
        let call = format!("B.http.{}({})", name, argv.join(", "));
        Ok(self.await_if(true, call))
    }

    /// `js` and `js_new` name what they call in the code itself: a name
    /// that came from data could be anything on the page, `eval` included.
    fn js_name(&mut self, f: &str, name: Option<&Expr>) -> Result<(), QuilloError> {
        let Some(Expr::Lit(Lit::Str(path))) = name else {
            return self.err(format!("`{}` needs the name of what it calls written out, like {}(\"Chart\", ...)", f, f));
        };
        let ok = !path.is_empty() && path.split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$'));
        if !ok {
            return self.err(format!("`{}`: {:?} is not a dotted name such as \"L.map\"", f, path));
        }
        let banned = [
            "eval",
            "Function",
            "setTimeout",
            "setInterval",
            "execScript",
            "insertAdjacentHTML",
            "setHTMLUnsafe",
            "parseHTMLUnsafe",
            "createContextualFragment",
            "write",
            "writeln",
            "constructor",
            "__proto__",
        ];
        let last = path.rsplit('.').next().unwrap_or("");
        if banned.contains(&last) {
            return self.err(format!("`{}` does not call {}: it runs text as code", f, path));
        }
        // Ways around the checks the runtime makes on the page's functions:
        // setting any property, calling with any `this`, parsing markup.
        let around = [
            "Reflect.apply",
            "Reflect.construct",
            "Reflect.get",
            "Reflect.set",
            "Reflect.defineProperty",
            "Reflect.setPrototypeOf",
            "Reflect.getOwnPropertyDescriptor",
            "Object.assign",
            "Object.defineProperty",
            "Object.defineProperties",
            "Object.setPrototypeOf",
            "Object.getOwnPropertyDescriptor",
            "Object.getOwnPropertyDescriptors",
            "DOMParser",
        ];
        let mut bare: &str = path;
        while let Some(rest) = ["window.", "globalThis.", "self."].iter().find_map(|p| bare.strip_prefix(p)) {
            bare = rest;
        }
        if around.contains(&bare) {
            return self.err(format!("`{}` does not call {}: it steps around the checks on the page's functions", f, path));
        }
        Ok(())
    }

    fn is_local(&self, name: &str) -> bool {
        self.ctxs.iter().any(|c| c.scopes.iter().any(|s| s.contains_key(name)))
    }

    fn is_async_name(&mut self, name: &str) -> bool {
        for ctx in self.ctxs.iter().rev() {
            for scope in ctx.scopes.iter().rev() {
                match scope.get(name) {
                    Some(Local::Var(_, f)) => return f.unwrap_or(false),
                    Some(_) => return false,
                    None => {}
                }
            }
        }
        match lookup(&self.it.globals, name) {
            Some(Value::Func(f)) if !f.def.component => {
                let line = self.line;
                self.global(name, line).map(|(_, a)| a).unwrap_or(false)
            }
            _ => false,
        }
    }

    fn finish_call(&mut self, name: &str, is_async: bool, call: String) -> Result<String, QuilloError> {
        if is_async && self.ctx().component.is_some() {
            return self.err(format!(
                "`{}` waits on the server, so a component can only call it from an event handler \
                 (@click={{() => {{ ... }}}}) or to set state: state x = {}(...)",
                name, name
            ));
        }
        Ok(self.await_if(is_async, call))
    }

    fn pattern(&mut self, p: &Pattern) -> Result<String, QuilloError> {
        Ok(match p {
            Pattern::Wild => "{k: \"w\"}".into(),
            Pattern::Bind(n) => format!("{{k: \"b\", n: {}}}", js_str(n)),
            Pattern::Lit(l) => format!("{{k: \"l\", v: {}}}", lit_js(l)),
            Pattern::Type(t) => {
                self.need_type(t)?;
                format!("{{k: \"t\", t: {}}}", js_str(t))
            }
            Pattern::Alt(ps) => {
                let mut out = Vec::new();
                for q in ps {
                    out.push(self.pattern(q)?);
                }
                format!("{{k: \"a\", ps: [{}]}}", out.join(", "))
            }
            Pattern::As(q, n) => format!("{{k: \"as\", p: {}, n: {}}}", self.pattern(q)?, js_str(n)),
            Pattern::List(ps, rest) => {
                let mut out = Vec::new();
                for q in ps {
                    out.push(self.pattern(q)?);
                }
                let rest = match rest {
                    None => "undefined".to_string(),
                    Some(None) => "\"\"".to_string(),
                    Some(Some(n)) => js_str(n),
                };
                format!("{{k: \"ls\", ps: [{}], rest: {}}}", out.join(", "), rest)
            }
            Pattern::Record(fs) => {
                let mut out = Vec::new();
                for f in fs {
                    let d = match &f.default {
                        Some(d) => {
                            let saved = self.ctx().needs_async;
                            let v = self.expr(d)?;
                            if self.ctx().needs_async && !saved {
                                return self.err("a pattern default cannot wait on the server");
                            }
                            format!("() => {}", v)
                        }
                        None => "null".into(),
                    };
                    out.push(format!(
                        "{{path: [{}], p: {}, d: {}, ty: {}}}",
                        f.path.iter().map(|k| js_str(k)).collect::<Vec<_>>().join(", "),
                        self.pattern(&f.pat)?,
                        d,
                        f.pat.is_type_test()
                    ));
                }
                format!("{{k: \"r\", fs: [{}]}}", out.join(", "))
            }
        })
    }

    /// What `e` gives, and whether it reads what changes (see `reactive`).
    fn reactive_expr(&mut self, e: &Expr) -> Result<(String, bool), QuilloError> {
        let saved = std::mem::replace(&mut self.reactive, false);
        let v = self.expr(e);
        let r = std::mem::replace(&mut self.reactive, saved);
        Ok((v?, r))
    }

    /// A value for markup: as it is when fixed, else a function giving it,
    /// which the part of the page showing it follows.
    fn dyn_value(&mut self, e: &Expr) -> Result<String, QuilloError> {
        let (v, r) = self.reactive_expr(e)?;
        Ok(if r { format!("() => ({})", v) } else { v })
    }

    /// What is shown where `e` is in markup. `for` is a list of rows that
    /// each follow their item; `if` and `match` choose a branch, made again
    /// only when another is chosen; anything else is a value (`dyn_value`).
    fn markup(&mut self, e: &Expr) -> Result<String, QuilloError> {
        let saved = self.reactive;
        let r = self.markup_inner(e);
        self.reactive = saved;
        r
    }

    fn markup_inner(&mut self, e: &Expr) -> Result<String, QuilloError> {
        match e {
            Expr::Html(el, _) => self.html(el),
            Expr::For(pat, iter, body, line) => {
                self.line = *line;
                self.each(pat, iter, body)
            }
            Expr::If(c, a, b) if has_markup(e) => {
                let (cv, r) = self.reactive_expr(c)?;
                let av = self.markup(a)?;
                let bv = match b {
                    Some(b) => self.markup(b)?,
                    None => "null".into(),
                };
                Ok(if r {
                    format!("W.choose(() => (W.truthy({}) ? 0 : 1), [() => {}, () => {}])", cv, av, bv)
                } else {
                    format!("W.frag([W.truthy({}) ? {} : {}])", cv, av, bv)
                })
            }
            Expr::Match(subj, arms, line) if has_markup(e) => self.choose_match(subj, arms, *line),
            Expr::Block(ss) if has_markup(e) => {
                if let [Stmt::Expr(x)] = ss.as_slice() {
                    return self.markup(x);
                }
                self.push_scope();
                let r = self.setup_stmts(ss);
                self.pop_scope();
                Ok(format!("(() => {{\n{}}})()", r?))
            }
            _ => self.dyn_value(e),
        }
    }

    /// `for` in markup: `W.each(list, key, row)`. A row is made once for
    /// its key (the `key` of its element, else its place) and gets its item
    /// as a signal, so a row whose item changed changes only where it shows
    /// what changed.
    fn each(&mut self, pat: &Pattern, iter: &Expr, body: &Expr) -> Result<String, QuilloError> {
        let (list, _) = self.reactive_expr(iter)?;
        let p = if matches!(pat, Pattern::Bind(_)) { None } else { Some(self.pattern(pat)?) };
        let mut names = Vec::new();
        pattern_names(pat, &mut names);
        let key_of = |el: &HtmlEl| {
            el.attrs.iter().find_map(|(n, v)| match (n.as_str(), v) {
                ("key", AttrVal::Expr(k)) => Some(k.clone()),
                _ => None,
            })
        };
        // A block whose value is one element is keyed by it too, as `=>` is:
        // `for p in xs { let n = p.name; <li key={n}/> }` (FEEDBACK6.md, Q-055;
        // branch feedback6-fixes). Its statements before the element run
        // in the key's function as well, so the key may use them.
        let (root, key, before) = match body {
            Expr::Html(el, _) => (Some(&**el), key_of(el), &[][..]),
            Expr::Block(stmts) => match stmts.split_last() {
                Some((Stmt::Expr(Expr::Html(el, _)), before)) => (None, key_of(el), before),
                _ => (None, None, &[][..]),
            },
            _ => (None, None, &[][..]),
        };
        let key_js = match &key {
            Some(k) => {
                self.push_scope();
                // With statements before it: the key is that block's value.
                let k = if before.is_empty() {
                    k.clone()
                } else {
                    Expr::Block(before.iter().cloned().chain(std::iter::once(Stmt::Expr(k.clone()))).collect())
                };
                let k = self.bind_pattern(pat, "$x", "`for` pattern does not match ").and_then(|binds| Ok((binds, self.reactive_expr(&k)?.0)));
                self.pop_scope();
                let (binds, k) = k?;
                format!("($x) => {{ {}return {}; }}", binds, k)
            }
            None => "null".into(),
        };
        let x = self.fresh("row");
        self.push_scope();
        let mut binds = String::new();
        match &p {
            None => self.declare_as(&names[0], Local::Reactive(x.clone())),
            Some(pj) => {
                let b = self.fresh("b");
                let _ = write!(
                    binds,
                    "const {} = W.memo(() => {{ const $v = {}.v, $b = W.match({}, $v); if ($b === null) W.fail(\"`for` pattern does not match \" + W.short($v)); return $b; }}); ",
                    b, x, pj
                );
                for n in &names {
                    let m = self.fresh(n);
                    let _ = write!(binds, "const {} = W.memo(() => {}.v[{}]); ", m, b, js_str(n));
                    self.declare_as(n, Local::Reactive(m));
                }
            }
        }
        // The row's own template: what in it reads only the row, the row
        // updates itself.
        if root.is_some() {
            self.row = Some(x.clone());
        }
        let row = match root {
            Some(el) if key.is_some() => self.html_unkeyed(el),
            _ => self.markup(body),
        };
        self.row = None;
        self.pop_scope();
        Ok(format!("W.each(() => ({}), {}, ({}) => {{ {}return {}; }})", list, key_js, x, binds, row?))
    }

    /// `match` in markup: the arm is chosen by the subject (a memo), and
    /// an arm's names follow the subject while it stays chosen.
    fn choose_match(&mut self, subj: &Expr, arms: &[Arm], line: u32) -> Result<String, QuilloError> {
        self.line = line;
        let (s, _) = self.reactive_expr(subj)?;
        let v = self.fresh("v");
        let mut which = format!("const $v = {}.v;\nlet $b;\n", v);
        let mut branches = Vec::new();
        for (i, arm) in arms.iter().enumerate() {
            let pat = self.pattern(&arm.pat)?;
            let mut names = Vec::new();
            pattern_names(&arm.pat, &mut names);
            self.push_scope();
            let mut binds = String::new();
            for n in &names {
                let js = self.declare(n, None);
                let _ = write!(binds, "const {} = $b[{}]; ", js, js_str(n));
            }
            let guard = arm.guard.as_ref().map(|g| self.reactive_expr(g));
            self.pop_scope();
            match guard {
                Some(g) => {
                    let _ = writeln!(which, "if (($b = W.match({}, $v)) !== null) {{ {}if (W.truthy({})) return {}; }}", pat, binds, g?.0, i);
                }
                None => {
                    let _ = writeln!(which, "if (W.match({}, $v) !== null) return {};", pat, i);
                }
            }
            self.push_scope();
            let mut pre = String::new();
            if !names.is_empty() {
                let b = self.fresh("b");
                let _ = write!(pre, "const {} = W.memo(() => W.match({}, {}.v) || {{}}); ", b, pat, v);
                for n in &names {
                    let m = self.fresh(n);
                    let _ = write!(pre, "const {} = W.memo(() => {}.v[{}]); ", m, b, js_str(n));
                    self.declare_as(n, Local::Reactive(m));
                }
            }
            let body = self.markup(&arm.body);
            self.pop_scope();
            branches.push(format!("() => {{ {}return {}; }}", pre, body?));
        }
        which.push_str("W.fail(\"no `match` arm matches \" + W.short($v));\n");
        Ok(format!(
            "(() => {{ const {} = W.memo(() => ({})); return W.choose(() => {{\n{}}}, [{}]); }})()",
            v,
            s,
            which,
            branches.join(", ")
        ))
    }

    /// An element or component; with a `key` that changes, made afresh.
    fn html(&mut self, el: &HtmlEl) -> Result<String, QuilloError> {
        self.line = el.line;
        let key = el.attrs.iter().find_map(|(n, v)| match (n.as_str(), v) {
            ("key", AttrVal::Expr(k)) => Some(k),
            _ => None,
        });
        let Some(key) = key else { return self.html_unkeyed(el) };
        let (k, r) = self.reactive_expr(key)?;
        let made = self.html_unkeyed(el)?;
        Ok(if r { format!("W.keyed(() => ({}), () => {})", k, made) } else { made })
    }

    fn html_unkeyed(&mut self, el: &HtmlEl) -> Result<String, QuilloError> {
        let saved = self.reactive;
        let r = if el.tag.starts_with(|c: char| c.is_uppercase()) { self.comp_call(el) } else { self.template(el) };
        self.reactive = saved;
        r
    }

    /// Markup of a fixed shape: its template is defined once (see `tmpl`
    /// in the runtime), and each use gives the values of its holes.
    fn template(&mut self, el: &HtmlEl) -> Result<String, QuilloError> {
        let row = self.row.take();
        let saved = (std::mem::replace(&mut self.cur_row, row.clone()), std::mem::replace(&mut self.row_mask, 0));
        let mut vals = Vec::new();
        let tpl = self.tpl_el(el, &mut vals);
        let mask = self.row_mask;
        (self.cur_row, self.row_mask) = saved;
        let tpl = tpl?;
        let name = format!("$b{}", self.blocks.len());
        self.blocks.push(format!("const {} = W.block({});", name, tpl));
        Ok(match row {
            Some(row) if mask != 0 => format!("W.t({}, [{}], {}, {})", name, vals.join(", "), row, mask),
            _ => format!("W.t({}, [{}])", name, vals.join(", ")),
        })
    }

    /// A hole's value in the template being compiled (see `dyn_value`),
    /// noting in `row_mask` whether it reads nothing but its row.
    fn hole_value(&mut self, e: &Expr, at: usize) -> Result<String, QuilloError> {
        let Some(row) = self.cur_row.clone() else { return self.dyn_value(e) };
        let saved = self.read_log.replace(Vec::new());
        let r = self.reactive_expr(e);
        let log = std::mem::replace(&mut self.read_log, saved).unwrap_or_default();
        if let Some(outer) = &mut self.read_log {
            outer.extend(log.iter().cloned());
        }
        let (v, reactive) = r?;
        if !reactive {
            return Ok(v);
        }
        if !log.is_empty() && log.iter().all(|s| *s == row) && at < 31 {
            self.row_mask |= 1 << at;
        }
        Ok(format!("() => ({})", v))
    }

    /// `<Name .../>`: a component, a view or a function, with its props as
    /// getters where they read what changes, and its children as markup.
    fn comp_call(&mut self, el: &HtmlEl) -> Result<String, QuilloError> {
        self.line = el.line;
        let (f, _) = self.callee_value(&Expr::Ident(el.tag.clone(), el.line))?;
        if f.starts_with("G[") && matches!(lookup(&self.it.globals, &el.tag), Some(Value::Func(c)) if !c.def.component && !c.def.server) {
            self.need_view(&el.tag)?;
        }
        let mut props = Vec::new();
        let mut spreads = Vec::new();
        for (name, v) in &el.attrs {
            if name == "key" {
                continue;
            }
            match v {
                AttrVal::Bool => props.push(format!("{}: true", js_str(name))),
                AttrVal::Expr(e) => {
                    let (v, r) = self.reactive_expr(e)?;
                    if r {
                        props.push(format!("get {}() {{ return {}; }}", js_str(name), v));
                    } else {
                        props.push(format!("{}: {}", js_str(name), v));
                    }
                }
                AttrVal::Spread(e) => spreads.push(self.reactive_expr(e)?.0),
            }
        }
        let kids = if el.children.is_empty() {
            "null".to_string()
        } else {
            let mut ks = Vec::new();
            for c in &el.children {
                ks.push(match c {
                    HtmlChild::El(e) => self.html(e)?,
                    HtmlChild::Expr(x) => self.markup(x)?,
                });
            }
            format!("W.frag([{}])", ks.join(", "))
        };
        let spreads = if spreads.is_empty() { String::new() } else { format!(", () => [{}]", spreads.join(", ")) };
        Ok(format!("W.c({}, {{{}}}, {}{})", f, props.join(", "), kids, spreads))
    }

    /// The form of view `name` for `<Name .../>`: its code once, with its
    /// props as getters (see `W.view` in the runtime), after the bundle's
    /// definitions.
    fn need_view(&mut self, name: &str) -> Result<(), QuilloError> {
        if !self.views.insert(name.to_string()) {
            return Ok(());
        }
        let Some(Value::Func(f)) = lookup(&self.it.globals, name) else { return Ok(()) };
        let def = f.def.clone();
        let (js, _) = self.global(name, def.line)?;
        let saved_ctxs = std::mem::take(&mut self.ctxs);
        let saved_what = std::mem::replace(&mut self.what, format!("<{}>", def.name));
        let saved_line = self.line;
        let saved_file = self.file.clone();
        if def.file.is_some() {
            self.file = def.file.clone();
        }
        self.line = def.line;
        let res = self.setup(&def, true);
        self.ctxs = saved_ctxs;
        self.what = saved_what;
        self.line = saved_line;
        self.file = saved_file;
        let code = res?;
        self.view_defs.push(format!("W.view({}, {});", js, code));
        Ok(())
    }

    /// `el`'s template, `[tag, [[name, value], ...], [kid, ...], [[kind,
    /// name, i], ...]]` (see `tmpl` in the runtime), adding to `vals` the
    /// values of its holes.
    fn tpl_el(&mut self, el: &HtmlEl, vals: &mut Vec<String>) -> Result<String, QuilloError> {
        self.line = el.line;
        let mut fixed_attrs: Vec<String> = Vec::new();
        let mut holes: Vec<String> = Vec::new();
        let hole = |holes: &mut Vec<String>, vals: &mut Vec<String>, kind: u8, name: &str, v: String| {
            holes.push(format!("[{}, {}, {}]", kind, js_str(name), vals.len()));
            vals.push(v);
        };
        let mut spread_js = Vec::new();
        let mut spread_r = false;
        for (_, v) in &el.attrs {
            if let AttrVal::Spread(e) = v {
                let (v, r) = self.reactive_expr(e)?;
                spread_js.push(v);
                spread_r |= r;
            }
        }
        if let Some(id) = &el.id {
            match fixed(id) {
                Some(l) => fixed_attrs.push(format!("[\"id\", {}]", lit_js(l))),
                None => {
                    let v = self.hole_value(id, vals.len())?;
                    hole(&mut holes, vals, 0, "id", v);
                }
            }
        }
        let mut classes: Vec<&Expr> = el.classes.iter().collect();
        for (name, v) in &el.attrs {
            if let ("class", AttrVal::Expr(e)) = (name.as_str(), v) {
                classes.push(e);
            }
        }
        if spread_js.is_empty() && classes.iter().all(|e| matches!(fixed(e), Some(Lit::Str(_)))) {
            let names: Vec<&str> = classes
                .iter()
                .filter_map(|e| match fixed(e) {
                    Some(Lit::Str(s)) if !s.is_empty() => Some(&**s),
                    _ => None,
                })
                .collect();
            if !names.is_empty() {
                fixed_attrs.push(format!("[\"class\", {}]", js_str(&names.join(" "))));
            }
        } else {
            let mut js = Vec::new();
            let mut r = spread_r;
            // One hole for all of them: only the row read, the row's.
            let row = if spread_js.is_empty() { self.cur_row.clone() } else { None };
            let saved = row.as_ref().map(|_| self.read_log.replace(Vec::new()));
            let got: Result<Vec<(String, bool)>, QuilloError> = classes.iter().map(|e| self.reactive_expr(e)).collect();
            let log = saved.map(|saved| std::mem::replace(&mut self.read_log, saved).unwrap_or_default());
            for (v, x) in got? {
                js.push(v);
                r |= x;
            }
            if let (Some(row), Some(log)) = (&row, log) {
                if r && !log.is_empty() && log.iter().all(|s| s == row) && vals.len() < 31 {
                    self.row_mask |= 1 << vals.len();
                }
                if let Some(outer) = &mut self.read_log {
                    outer.extend(log);
                }
            }
            let v = if !spread_js.is_empty() {
                js.push(format!("...W.spreadcls([{}])", spread_js.join(", ")));
                format!("W.cls([{}])", js.join(", "))
            } else if js.len() == 1 {
                format!("W.cls1({})", js[0])
            } else {
                format!("W.cls([{}])", js.join(", "))
            };
            hole(&mut holes, vals, 1, "", if r { format!("() => {}", v) } else { v });
        }
        let mut written = Vec::new();
        // Handlers (`@input`) and bindings' own (`bind:value`), joined after
        // the attributes when they listen to the same event.
        let mut ons: Vec<(String, String)> = Vec::new();
        let mut binds: Vec<(&str, String)> = Vec::new();
        for (name, v) in &el.attrs {
            if matches!(v, AttrVal::Spread(_)) || name == "key" || name == "class" {
                continue;
            }
            written.push(js_str(name));
            if let Some(ev) = name.strip_prefix('@') {
                let f = match v {
                    AttrVal::Expr(e @ Expr::Lambda(d)) => {
                        let saved = self.handler.replace(&**d as *const FnDef);
                        let code = self.expr(e);
                        self.handler = saved;
                        code?
                    }
                    AttrVal::Expr(e) => {
                        let (f, r) = self.reactive_expr(e)?;
                        if r {
                            // The handler it reads when the event comes.
                            format!("($e) => {{ const $f = {}; if (typeof $f === \"function\") return $f.length > 0 ? $f($e) : $f(); }}", f)
                        } else {
                            f
                        }
                    }
                    _ => "true".to_string(),
                };
                ons.push((event_name(ev).to_string(), f));
                continue;
            }
            if let Some(prop) = name.strip_prefix("bind:") {
                let AttrVal::Expr(Expr::Ident(var, _)) = v else {
                    return self.err(format!("`bind:{}` needs a variable: bind:{}={{name}}", prop, prop));
                };
                let (event, read) = match prop {
                    "value" => ("input", "e.target.value"),
                    "checked" => ("change", "e.target.checked"),
                    other => return self.err(format!("`bind:{}` is not supported (use bind:value or bind:checked)", other)),
                };
                let saved = self.reactive;
                let (get, set) = match self.resolve(var)? {
                    Resolved::State(js) => (format!("() => {}.v", js), format!("{}.set({})", js, read)),
                    Resolved::Local(js, _) => (js.clone(), format!("({} = {})", js, read)),
                    _ => return self.err(format!("`bind:{}` needs component state, like `state {}`", prop, var)),
                };
                self.reactive = saved;
                hole(&mut holes, vals, 0, prop, get);
                binds.push((event, set));
                continue;
            }
            // value, checked and selected are the element's live state,
            // set on the element itself rather than as attributes.
            let live = matches!(name.as_str(), "value" | "checked" | "selected");
            match v {
                AttrVal::Bool if !live => fixed_attrs.push(format!("[{}, true]", js_str(name))),
                AttrVal::Expr(e) if !live && fixed(e).is_some() => match fixed(e) {
                    Some(Lit::Nil | Lit::Bool(false)) => {}
                    Some(l) => fixed_attrs.push(format!("[{}, {}]", js_str(name), lit_js(l))),
                    None => unreachable!(),
                },
                AttrVal::Expr(e) if !live => {
                    let v = self.hole_value(e, vals.len())?;
                    hole(&mut holes, vals, 0, name, v);
                }
                AttrVal::Expr(e) => {
                    let v = self.dyn_value(e)?;
                    hole(&mut holes, vals, 0, name, v);
                }
                _ => hole(&mut holes, vals, 0, name, "true".into()),
            }
        }
        // An element listens once to an event, so `bind:value` with `@input`
        // (or `bind:checked` with `@change`) is one handler: the binding sets
        // the state, then the handler runs and reads it. Before, the second
        // took the first one's place (FEEDBACK14.md, Q-094; branch
        // feedback14-fixes).
        for (ev, f) in ons {
            let f = match binds.iter().position(|(b, _)| *b == ev) {
                Some(i) => {
                    let (_, set) = binds.remove(i);
                    format!("(e) => {{ {}; const $h = {}; if (typeof $h === \"function\") return $h.length > 0 ? $h(e) : $h(); }}", set, f)
                }
                None => f,
            };
            hole(&mut holes, vals, 2, &ev, f);
        }
        for (ev, set) in binds {
            hole(&mut holes, vals, 2, ev, format!("(e) => {{ {}; }}", set));
        }
        if !spread_js.is_empty() {
            let v = format!("W.spread([{}], [{}])", spread_js.join(", "), written.join(", "));
            hole(&mut holes, vals, 3, "", if spread_r { format!("() => {}", v) } else { v });
        }
        // Children: fixed text and elements are in the template; anything
        // else is a value shown in its place.
        let mut kids = Vec::new();
        for c in &el.children {
            // The browser runs a script the page puts in: what fills one
            // would be code. (A view's, made on the server, takes values as
            // JSON.)
            if el.tag == "script" && !matches!(c, HtmlChild::Expr(Expr::Lit(Lit::Str(_)))) {
                return self.err("a component's <script> takes only text written in the program; pass data in a `data-` attribute, or use a view");
            }
            match c {
                HtmlChild::Expr(Expr::Lit(Lit::Str(s))) => kids.push(js_str(s)),
                HtmlChild::El(e) if inlineable(e) => kids.push(self.tpl_el(e, vals)?),
                HtmlChild::El(e) => {
                    let v = self.html(e)?;
                    kids.push(vals.len().to_string());
                    vals.push(v);
                }
                HtmlChild::Expr(e) => {
                    let v = if has_markup(e) { self.markup(e)? } else { self.hole_value(e, vals.len())? };
                    kids.push(vals.len().to_string());
                    vals.push(v);
                }
            }
        }
        self.line = el.line;
        Ok(format!("[{}, [{}], [{}], [{}]]", js_str(&el.tag), fixed_attrs.join(", "), kids.join(", "), holes.join(", ")))
    }
}

/// Whether `el` can be part of its parent's template: an element (not a
/// component) without a `key`, which makes it start afresh when it changes.
fn inlineable(el: &HtmlEl) -> bool {
    !el.tag.starts_with(|c: char| c.is_uppercase()) && el.attrs.iter().all(|(name, _)| name != "key")
}

/// A value fixed when the program is written: text, a number or a bool.
fn fixed(e: &Expr) -> Option<&Lit> {
    match e {
        Expr::Lit(l) => Some(l),
        _ => None,
    }
}

#[cfg(test)]
mod minify_tests {
    fn min(js: &str) -> String {
        let mut out = String::new();
        super::minify_into(js, &mut out);
        out
    }

    /// The whole runtime, as production bundles take it (without the lines
    /// for development), minified: still JavaScript.
    #[test]
    fn the_minified_runtime_is_still_javascript() {
        if std::process::Command::new("node").arg("--version").output().is_err() {
            return;
        }
        let rt = super::runtime_units();
        let mut js = String::new();
        super::minify_into(&rt.preamble, &mut js);
        for (_, text) in &rt.units {
            super::minify_into(text, &mut js);
        }
        assert!(!js.contains("// dev"));
        let f = std::env::temp_dir().join(format!("quillo-runtime-{}.js", std::process::id()));
        std::fs::write(&f, &js).unwrap();
        let out = std::process::Command::new("node").arg("--check").arg(&f).output().unwrap();
        let _ = std::fs::remove_file(&f);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }

    #[test]
    fn newlines_go_only_where_they_change_nothing() {
        // After `;`, `{` and `,` in code: joined, indentation and comment
        // lines gone.
        assert_eq!(min("function f(a,\n    b) {\n  // why\n  let x = 1;\n  return x\n}\n"), "function f(a,b) {let x = 1;return x\n}\n");
        // A newline that may end a statement stays.
        assert_eq!(min("let a = b\n(c)\n"), "let a = b\n(c)\n");
        // A template that goes on over lines keeps its text, indentation and
        // all, and what looks like a comment in it.
        assert_eq!(min("const t = `a;\n  // b,\n  ${x};\n  c`;\nf();\n"), "const t = `a;\n  // b,\n  ${x};\n  c`;f();");
        assert_eq!(min("const t = `${ { a: 1 }.a },\n`;\ng();\n"), "const t = `${ { a: 1 }.a },\n`;g();");
        // Strings, regular expressions and comments with quotes, backticks
        // and slashes in them; a line ending in a comment keeps its newline.
        assert_eq!(min("const q = \"`\";\nconst r = /[`/]x/g;\nreturn /`;/.test(s);\nx = a / b;\nf(); // a,\ng();\n"), "const q = \"`\";const r = /[`/]x/g;return /`;/.test(s);x = a / b;f(); // a,\ng();");
    }
}
