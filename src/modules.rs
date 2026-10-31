//! Modules: what a file sees (FEEDBACK19.md, Q-154; branch feedback19-fixes).
//!
//! A file sees its own names, the names it takes, and builtins. `use lib/auth.qwe`
//! takes every public name of the file, `import lib/auth.qwe use login, logout as bye`
//! the ones it says, and neither passes them on to whoever takes from this file. Two
//! files may then have one public name.
//!
//! How: the program stays one table of globals when it runs, and this pass, once every
//! file is parsed, says what each file's names are called there. A name only one file
//! defines keeps its name, so a program without a clash runs as it always has. Of a name
//! several define, the main file's stays as it is and each other's is renamed as a
//! private one is (src/privacy.rs): the same on every server, and shown as it was
//! written. Each file's text is then renamed: its own names, and those it takes.
//!
//! A `server fn`'s name is its address, and types and services are the program's: those
//! are not renamed, so one name in two files stays an error (`quillo check`'s "defined
//! twice"). Types and services are seen everywhere, too. A name used but not taken is
//! found by the checker, which knows what is a local (`Links::sees`), and `quillo fix`
//! writes the `import` for it.

use crate::ast::*;
use crate::check::Severity;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// What a `use` could not take.
#[derive(Debug, Clone)]
pub struct Problem {
    pub file: PathBuf,
    pub line: u32,
    pub message: String,
}

#[derive(Default)]
struct Imports {
    /// Files whose every public name this file takes.
    files: HashSet<PathBuf>,
    /// Names it takes one by one, as the program calls them.
    names: HashSet<String>,
    /// Names two of its `use`s give, each from those files.
    ambiguous: HashMap<String, Vec<PathBuf>>,
}

#[derive(Default)]
pub struct Links {
    renames: HashMap<PathBuf, HashMap<String, String>>,
    imports: HashMap<PathBuf, Imports>,
    /// Names more than one file defines, and which.
    shared: HashMap<String, Vec<PathBuf>>,
    /// How the program writes the path of each file it `use`s.
    written: HashMap<PathBuf, String>,
    pub problems: Vec<Problem>,
    /// Each file as it was parsed, before any renaming.
    pub parsed: HashMap<PathBuf, Arc<Vec<Stmt>>>,
}

/// How a file is known here, however a `use` writes its path.
pub fn key(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// A `use` that takes names from a path with no extension says `.qwer`, as the parser
/// cannot look: it is a `.qwe` file when that is the one there.
pub fn resolve(base: &Path, path: &str) -> String {
    match path.strip_suffix(".qwer").map(|p| format!("{}.qwe", p)) {
        Some(qwe) if !base.join(path).exists() && base.join(&qwe).exists() => qwe,
        _ => path.to_string(),
    }
}

/// The names a file defines at its top level: the name, and whether it is the
/// program's (see the module's comment). Private ones are left out, already renamed.
fn defined(stmts: &[Stmt]) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    for s in stmts {
        match s {
            Stmt::Fn(d) => out.push((d.name.clone(), d.server)),
            Stmt::Type(td) => out.push((td.name.clone(), true)),
            Stmt::Service(name, ..) => out.push((name.clone(), true)),
            Stmt::Let(p, ..) => {
                let mut names = Vec::new();
                crate::check::pattern_names(p, &mut names);
                out.extend(names.into_iter().map(|n| (n, false)));
            }
            _ => {}
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Reads the program from `main`. `read` gives a file's parse from the path as written
/// and in full, or nothing when it cannot be had, which the `use` itself goes on to report.
pub fn link(main: &Path, stmts: Arc<Vec<Stmt>>, base: &Path, read: &mut dyn FnMut(&str, &Path) -> Option<Arc<Vec<Stmt>>>) -> Links {
    let mut links = Links::default();
    let main = key(main);
    let mut files = vec![(main.clone(), stmts)];
    let mut at = 0;
    while at < files.len() {
        let stmts = files[at].1.clone();
        for s in stmts.iter() {
            let Stmt::Use(path, _, _, _) = s else { continue };
            if crate::ast::is_script(path) {
                continue;
            }
            let path = resolve(base, path);
            let full = base.join(&path);
            let k = key(&full);
            links.written.entry(k.clone()).or_insert(path.clone());
            if !path.ends_with(".qwer") && !files.iter().any(|(f, _)| *f == k) {
                if let Some(parsed) = read(&path, &full) {
                    links.parsed.insert(k.clone(), parsed.clone());
                    files.push((k, parsed));
                }
            }
        }
        at += 1;
    }

    let tops: Vec<Vec<(String, bool)>> = files.iter().map(|(_, s)| defined(s)).collect();
    let mut by_name: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, names) in tops.iter().enumerate() {
        for (n, _) in names {
            by_name.entry(n).or_default().push(i);
        }
    }
    // Each file's name for its own: the one written, but for a name several define.
    let mut finals: HashMap<(usize, &str), String> = HashMap::new();
    for (n, in_files) in by_name.iter().filter(|(_, f)| f.len() > 1) {
        links.shared.insert(n.to_string(), in_files.iter().map(|i| files[*i].0.clone()).collect());
        for i in in_files.iter().filter(|i| **i != 0) {
            if !tops[*i].iter().any(|(m, wide)| m == n && *wide) {
                let tag = crate::privacy::tag(Some(&files[*i].0.display().to_string()), "");
                finals.insert((*i, n), crate::privacy::mangle(n, &tag));
            }
        }
    }
    let final_of = |i: usize, n: &str| finals.get(&(i, n)).cloned().unwrap_or_else(|| n.to_string());

    for (i, (file, stmts)) in files.iter().enumerate() {
        let own: HashSet<&str> = tops[i].iter().map(|(n, _)| n.as_str()).collect();
        let mut map: HashMap<String, String> = HashMap::new();
        for n in &own {
            if finals.contains_key(&(i, *n)) {
                map.insert(n.to_string(), final_of(i, n));
            }
        }
        let mut imports = Imports::default();
        let mut whole: BTreeMap<&str, Vec<(String, usize)>> = BTreeMap::new();
        let mut named: HashMap<String, String> = HashMap::new();
        for s in stmts.iter() {
            let Stmt::Use(path, names, line, _) = s else { continue };
            if crate::ast::is_script(path) {
                imports.names.extend(names.iter().flatten().map(|(_, alias)| alias.clone()));
                continue;
            }
            let path = resolve(base, path);
            let k = key(&base.join(&path));
            let target = files.iter().position(|(f, _)| *f == k);
            let mut problem = |message: String| links.problems.push(Problem { file: file.clone(), line: *line, message });
            match (names, target) {
                (None, Some(t)) => {
                    imports.files.insert(k);
                    for (n, _) in tops[t].iter().filter(|(n, _)| !own.contains(n.as_str())) {
                        whole.entry(n).or_default().push((final_of(t, n), t));
                    }
                }
                (None, None) => {
                    imports.files.insert(k);
                }
                (Some(list), Some(t)) => {
                    for (n, alias) in list {
                        if !tops[t].iter().any(|(m, _)| m == n) {
                            let private = defined_privately(&files[t].1, n);
                            problem(if private {
                                format!("`{}` is private to {}: make it `public` there, or take something public", n, path)
                            } else {
                                format!("{} has no public name `{}`", path, n)
                            });
                            continue;
                        }
                        if own.contains(alias.as_str()) {
                            problem(format!("`{}` is defined in this file, and taken from {}: take it `as` another name", alias, path));
                            continue;
                        }
                        let f = final_of(t, n);
                        if named.get(alias).is_some_and(|other| *other != f) {
                            problem(format!("`{}` is taken twice, the second time from {}: take one `as` another name", alias, path));
                            continue;
                        }
                        named.insert(alias.clone(), f.clone());
                        imports.names.insert(f.clone());
                        if f != *alias {
                            map.insert(alias.clone(), f);
                        }
                    }
                }
                // A plugin's names, or a file that is reported where it is used.
                (Some(list), None) => imports.names.extend(list.iter().map(|(_, alias)| alias.clone())),
            }
        }
        for (n, from) in whole {
            if named.contains_key(n) {
                continue;
            }
            let f = &from[0].0;
            if from.iter().all(|(other, _)| other == f) {
                if f != n {
                    map.insert(n.to_string(), f.clone());
                }
            } else {
                imports.ambiguous.insert(n.to_string(), from.iter().map(|(_, t)| files[*t].0.clone()).collect());
            }
        }
        if !map.is_empty() {
            links.renames.insert(file.clone(), map);
        }
        links.imports.insert(file.clone(), imports);
    }
    links
}

/// Whether a file has `name` as a private name.
fn defined_privately(stmts: &[Stmt], name: &str) -> bool {
    defined(stmts).iter().any(|(n, _)| crate::privacy::private_name(n) == Some(name))
}

impl Links {
    /// What `file`'s names are called in the program, when any differ.
    pub fn renames(&self, file: &Path) -> Option<&HashMap<String, String>> {
        self.renames.get(file)
    }

    /// Whether `file` took `name`, which `from` defines.
    pub fn sees(&self, file: &Path, from: &Path, name: &str) -> bool {
        self.imports.get(file).map_or(true, |i| i.files.contains(from) || i.names.contains(name))
    }

    /// The files two of `file`'s `use`s give `name` from.
    pub fn choices(&self, file: &Path, name: &str) -> Option<&Vec<PathBuf>> {
        self.imports.get(file).and_then(|i| i.ambiguous.get(name))
    }

    /// The files that define a name several do.
    pub fn shared(&self, name: &str) -> Option<&Vec<PathBuf>> {
        self.shared.get(name)
    }

    /// A file's path as the program writes it: from the main file's folder.
    pub fn written(&self, file: &Path) -> String {
        match self.written.get(file) {
            Some(p) => p.clone(),
            None => file.file_name().map_or(String::new(), |f| f.to_string_lossy().to_string()),
        }
    }
}

/// A `use` written as 0.6 did, and how it is written now.
#[derive(Debug, Clone)]
pub struct Respell {
    pub file: String,
    pub line: u32,
    /// As far as the file: `import lib/auth.qwe`, `preload module "https://..."`.
    pub head: String,
    pub names: String,
}

impl Respell {
    pub fn new(file: &str, line: u32, path: &str, names: Option<&[(String, String)]>, options: &LibOptions) -> Respell {
        let script = is_script(path);
        let word = match options.hint {
            Some(LibHint::Preload) => "preload",
            Some(LibHint::Preconnect) => "preconnect",
            None if script => "client",
            None => "import",
        };
        let module = if options.module && !is_module_file(path) { " module" } else { "" };
        let plain = !is_address(path) && path.chars().enumerate().all(|(i, c)| crate::parser::is_use_path_char(c, i == 0));
        let shown = if plain { path.to_string() } else { format!("\"{}\"", path) };
        let taken = names.map_or("*".to_string(), |list| {
            list.iter().map(|(n, a)| if n == a { n.clone() } else { format!("{} as {}", n, a) }).collect::<Vec<_>>().join(", ")
        });
        Respell { file: file.to_string(), line, head: format!("{}{} {}", word, module, shown), names: taken }
    }

    fn written(&self) -> String {
        statement(&self.head, &self.names)
    }
}

/// A line that takes names, its `use` on a line of its own when one line would be long.
fn statement(head: &str, names: &str) -> String {
    let at = if head.len() + " use ".len() + names.len() > 90 { "\n    " } else { " " };
    format!("{}{}use {}", head, at, names)
}

/// What a line has after the path of its `use`: a comment, or nothing.
fn after_path(line: &str) -> &str {
    let rest = line.split_once(" from ").map_or("", |(_, r)| r.trim_start());
    let end = match rest.strip_prefix('"') {
        Some(q) => q.find('"').map_or(rest.len(), |e| e + 2),
        None => rest.find(|c: char| c.is_whitespace() || c == ';').unwrap_or(rest.len()),
    };
    &rest[end..]
}

/// `text` with the `use` on `r`'s line written anew. A line that has more than the `use`
/// before it stays as it was: the warning stays too.
fn respelled(text: &str, r: &Respell) -> String {
    let mut lines: Vec<String> = text.split('\n').map(String::from).collect();
    let at = r.line as usize - 1;
    let l = lines[at].clone();
    let body = l.trim_start();
    if body.starts_with("use ") {
        let indent = &l[..l.len() - body.len()];
        lines[at] = format!("{}{}{}", indent, r.written().replace('\n', &format!("\n{}", indent)), after_path(&l));
    }
    lines.join("\n")
}

/// The statement that takes a script, for another file that needs it too.
fn script_line(all: &[Respell], file: &str, line: u32) -> Result<String, String> {
    if let Some(r) = all.iter().find(|r| r.file == file && r.line == line) {
        return Ok(r.written());
    }
    let text = std::fs::read_to_string(file).map_err(|e| format!("cannot read {}: {}", file, e))?;
    let mut lines = text.lines().skip(line as usize - 1);
    let first = lines.next().unwrap_or("").trim();
    Ok(match lines.next().map(str::trim).filter(|n| n.starts_with("use ") && !first.contains(" use ")) {
        Some(next) => format!("{}\n    {}", first, next),
        None => first.to_string(),
    })
}

/// `quillo fix`: writes the `import` each file needs for the names it uses and does not
/// take, and turns a `use` written as 0.6 did around. Says what it did, a line each.
pub fn fix(main: &Path) -> Result<Vec<String>, String> {
    let a = crate::check::check_file(main);
    if let Some(d) = a.diagnostics.iter().find(|d| d.severity == Severity::Error) {
        return Err(format!("{}:{}: {}", d.file, d.line, d.message));
    }
    let mut files: BTreeMap<String, String> = BTreeMap::new();
    let mut said = Vec::new();
    // A file as it stands: as this has left it, or as it is.
    let read = |files: &BTreeMap<String, String>, f: &str| files.get(f).cloned().map_or_else(|| std::fs::read_to_string(f).map_err(|e| format!("cannot read {}: {}", f, e)), Ok);
    let mut respell = a.respell.clone();
    respell.sort_by(|x, y| (&x.file, x.line).cmp(&(&y.file, y.line)));
    respell.dedup_by(|x, y| (&x.file, x.line) == (&y.file, y.line));
    // From the end of each file: one that takes two lines moves those after it.
    for r in respell.iter().rev() {
        let text = read(&files, &r.file)?;
        files.insert(r.file.clone(), respelled(&text, r));
    }
    said.extend(respell.iter().map(|r| format!("{}:{}: {}", r.file, r.line, r.written().replace("\n   ", ""))));
    // By file, then by what it comes from.
    let mut wanted: BTreeMap<&str, BTreeMap<String, Vec<&str>>> = BTreeMap::new();
    for i in &a.imports {
        let at = match &i.script {
            Some((f, l)) => script_line(&a.respell, f, *l)?,
            None => format!("import {}", i.from),
        };
        wanted.entry(&i.file).or_default().entry(at).or_default().push(&i.name);
    }
    for (f, froms) in wanted {
        let mut text = read(&files, f)?;
        for (from, mut names) in froms {
            let line = if from.starts_with("import ") {
                names.sort();
                names.dedup();
                statement(&from, &names.join(", "))
            } else {
                from
            };
            text = with_use(&text, &line);
            said.push(format!("{}: {}", f, line.replace("\n   ", "")));
        }
        files.insert(f.to_string(), text);
    }
    for (f, text) in files {
        std::fs::write(&f, text).map_err(|e| format!("cannot write {}: {}", f, e))?;
    }
    Ok(said)
}

/// `text` with a line added below the `use`s and `import`s at its start, or, with none,
/// above its code.
fn with_use(text: &str, line: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut last_use = None;
    let mut code = lines.len();
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim();
        if t.starts_with("use ") || crate::parser::import_line(t) {
            last_use = Some(i);
        } else if !t.is_empty() && !t.starts_with("//") {
            code = i;
            break;
        }
    }
    let mut out: Vec<&str> = lines.clone();
    match last_use {
        Some(i) => out.insert(i + 1, line),
        None => {
            out.insert(code, line);
            out.insert(code + 1, "");
        }
    }
    out.join("\n")
}
