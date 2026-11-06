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
    /// The name the import line takes that this is about, so its uses are not said again as
    /// unknown (FEEDBACK45.md, Q-523).
    pub name: Option<String>,
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
    /// What a `use` takes again: said as a warning, the program runs (FEEDBACK45.md, Q-523).
    pub repeats: Vec<Problem>,
    /// By file, the names it takes only `as` another name, with that name: the one the other
    /// file calls it, written here, is not one it took (Q-523).
    pub old_names: HashMap<PathBuf, HashMap<String, String>>,
    /// Each file as it was parsed, before any renaming.
    pub parsed: HashMap<PathBuf, Arc<Vec<Stmt>>>,
    /// By file, what the names it takes from Rust files are defined as, where not as written:
    /// another name, or (`None`) none, as another of that name wins (FEEDBACK55.md, Q-816).
    rust: HashMap<PathBuf, HashMap<String, Option<String>>>,
}

/// How a file is known here, however a `use` writes its path.
pub fn key(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// A `use` that takes names from a path with no extension says `.qwer`, as the parser
/// cannot look: it is a `.qwe` file when that is the one there.
/// A `use` path as `resolve` reads it, but one that starts `./` or `../` counts from the folder of
/// the file that writes it, `from`, so a library's file can name its neighbour and be checked
/// alone (FEEDBACK25.md, Q-325; branch feedback27-35-fixes). Given back as the program's folder
/// counts it, so every file names one file the same way.
pub fn resolve_from(base: &Path, from: &Path, path: &str) -> String {
    resolve_use(base, from, path, crate::native::ends_with_any_case(path, ".qwer"))
}

/// The folder a program's `use`, `import`, `mount` and `service` paths count from: its main
/// file's, and for a main file that is a link, the folder of the file it leads to, so a tool
/// linked into the PATH finds its files, which were looked for beside the link (FEEDBACK55.md,
/// Q-817; branch feedback55). Its policy, `.env`, data and messages stay the folder given, and
/// a folder that is a link (`current -> releases/7`) is the folder given. Said with as few
/// `..` as the way allows, for messages.
pub fn source_dir(main: &Path) -> PathBuf {
    let given = main.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new(".")).to_path_buf();
    let mut at = main.to_path_buf();
    let mut hops = 0;
    while hops < 40 && std::fs::symlink_metadata(&at).is_ok_and(|m| m.file_type().is_symlink()) {
        let Ok(to) = std::fs::read_link(&at) else { break };
        at = if to.is_absolute() { to } else { at.parent().unwrap_or(Path::new("")).join(to) };
        hops += 1;
    }
    if hops == 0 {
        return given;
    }
    let short = tidy(&at);
    // A link on the way that `..` crosses: the file's own path.
    let at = if key(&short) == key(main) { short } else { key(main) };
    at.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new(".")).to_path_buf()
}

/// `source_dir` for a main file that is a link, else `given`, the folder as the caller has it.
pub fn source_dir_or(main: &Path, given: &Path) -> PathBuf {
    if std::fs::symlink_metadata(main).is_ok_and(|m| m.file_type().is_symlink()) {
        source_dir(main)
    } else {
        given.to_path_buf()
    }
}

/// `a/./b/../c` as `a/c`, a `..` that has nothing before it kept.
fn tidy(p: &Path) -> PathBuf {
    let mut out: Vec<std::path::Component> = Vec::new();
    for c in p.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir if matches!(out.last(), Some(std::path::Component::Normal(_))) => {
                out.pop();
            }
            c => out.push(c),
        }
    }
    out.iter().collect()
}

/// `resolve_from` for a `use` whose path the parser gave its extension, `implied`
/// (`LibOptions::implied`), or that was written with one.
pub fn resolve_use(base: &Path, from: &Path, path: &str, implied: bool) -> String {
    let (base, rel) = from_program(base, from, path);
    resolve_as(&base, &rel, implied)
}

/// A path without its extension that more than one file could be (`use helpers` beside
/// `helpers.qwe` and `helpers.rs`): those files, as the program's folder counts them, to say
/// so; it took the `.qwe` one without a word (FEEDBACK55.md, Q-819; branch feedback55).
pub fn implied_files(base: &Path, from: &Path, path: &str) -> Vec<String> {
    let (base, rel) = from_program(base, from, path);
    let stem = [".qwer", ".qwe", ".rs"].iter().find(|e| crate::native::ends_with_any_case(&rel, e)).map_or(rel.as_str(), |e| &rel[..rel.len() - e.len()]);
    [".qwe", ".qwer", ".rs"].iter().map(|e| format!("{}{}", stem, e)).filter(|p| base.join(p).is_file()).collect()
}

/// What a `use` is told when its path, written without an extension, is more than one file.
pub fn two_files(could: &[String]) -> String {
    let shown: Vec<&str> = could.iter().map(|c| c.trim_start_matches("./")).collect();
    let written = shown[0].rsplit_once('.').map_or(shown[0], |(s, _)| s);
    format!("`{}` could be {}: write the one you mean with its extension", written, shown.join(" or "))
}

/// The folder a `use` path counts from, and the path from there: one that starts `./` or `../`
/// from the folder of the file that writes it, said as a way from the program's folder.
fn from_program(base: &Path, from: &Path, path: &str) -> (PathBuf, String) {
    if !(path.starts_with("./") || path.starts_with("../")) {
        return (base.to_path_buf(), path.to_string());
    }
    let real = |p: &Path| -> std::path::PathBuf {
        let abs = if p.is_absolute() { p.to_path_buf() } else { std::env::current_dir().map(|d| d.join(p)).unwrap_or_else(|_| p.to_path_buf()) };
        abs.canonicalize().unwrap_or_else(|_| lexical(&abs))
    };
    // The folder of the file itself, where a main file is a link (Q-817).
    let dir = from.canonicalize().ok().and_then(|f| f.parent().map(Path::to_path_buf)).unwrap_or_else(|| real(from.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."))));
    let target = lexical(&dir.join(path));
    let base = real(base);
    // Outside the program's folder it is said as a way from it, `../secret.qwe`, never as the
    // machine's own path.
    let common = target.components().zip(base.components()).take_while(|(a, b)| a == b).count();
    let ups = base.components().count() - common;
    let rel = std::iter::repeat_n("..".to_string(), ups)
        .chain(target.components().skip(common).map(|c| c.as_os_str().to_string_lossy().into_owned()))
        .collect::<Vec<_>>()
        .join("/");
    (base, rel)
}

/// `a/./b/../c` as `a/c`, without asking the system.
fn lexical(p: &Path) -> std::path::PathBuf {
    let mut out = std::path::PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            c => out.push(c.as_os_str()),
        }
    }
    out
}

pub fn resolve(base: &Path, path: &str) -> String {
    resolve_as(base, path, crate::native::ends_with_any_case(path, ".qwer"))
}

/// A path whose extension the parser gave (`import imaging use img`, `use lib/b`) is whichever of
/// the `.qwe`, `.qwer` and `.rs` file is there; one written with its extension is that file:
/// `use helpers.rs` loaded `helpers.qwe`, and `import imaging use img` looked only for
/// `imaging.qwer` beside `imaging.rs` (FEEDBACK55.md, Q-819; branch feedback55).
pub fn resolve_as(base: &Path, path: &str, implied: bool) -> String {
    if !implied || base.join(path).exists() {
        return path.to_string();
    }
    let stem = [".qwer", ".qwe", ".rs"].iter().find(|e| crate::native::ends_with_any_case(path, e)).map_or(path, |e| &path[..path.len() - e.len()]);
    [".qwe", ".qwer", ".rs"].iter().map(|e| format!("{}{}", stem, e)).find(|p| base.join(p).exists()).unwrap_or_else(|| path.to_string())
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
            let Stmt::Use(path, _, _, options) = s else { continue };
            if crate::ast::is_script(path) {
                continue;
            }
            let path = resolve_use(base, &files[at].0, path, options.implied);
            let full = base.join(&path);
            let k = key(&full);
            links.written.entry(k.clone()).or_insert(path.clone());
            if !crate::native::is_rust(&path) && !files.iter().any(|(f, _)| *f == k) {
                if let Some(parsed) = read(&path, &full) {
                    links.parsed.insert(k.clone(), parsed.clone());
                    files.push((k, parsed));
                }
            }
        }
        at += 1;
    }

    let tops: Vec<Vec<(String, bool)>> = files.iter().map(|(_, s)| defined(s)).collect();
    // The names each file takes from Rust files: its own, as its `fn`s are, so another file's of
    // the same spelling is another function, where the last file's `use` was the whole
    // program's (FEEDBACK55.md, Q-816; branch feedback55). And those of a file taken whole
    // that the file has, or takes by name, from elsewhere: not defined, as that one wins.
    let rust: Vec<(Vec<RustName>, Vec<String>)> = files.iter().enumerate().map(|(i, (f, s))| rust_names(base, f, s, &tops[i])).collect();
    // The program's types, whose names `as` cannot take (Q-821).
    let types: HashSet<&str> = files.iter().flat_map(|(_, s)| s.iter()).filter_map(|s| if let Stmt::Type(td) = s { Some(td.name.as_str()) } else { None }).collect();
    let mut by_name: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, names) in tops.iter().enumerate() {
        for (n, _) in names {
            by_name.entry(n).or_default().push(i);
        }
    }
    for (i, (names, _)) in rust.iter().enumerate() {
        for r in names.iter().filter(|r| !tops[i].iter().any(|(m, _)| *m == r.alias)) {
            let at = by_name.entry(&r.alias).or_default();
            if !at.contains(&i) {
                at.push(i);
            }
        }
    }
    // What a name is in each file that has it: the file's own, or a Rust file's function. One
    // function of one Rust file, in several files, is one thing, as at HEAD (Q-816).
    let meanings = |n: &str, in_files: &[usize]| -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = Vec::new();
        for i in in_files {
            let found: Vec<PathBuf> = if tops[*i].iter().any(|(m, _)| m == n) {
                vec![files[*i].0.clone()]
            } else {
                rust[*i].0.iter().filter(|r| r.alias == n).map(|r| r.from.join(&r.export)).collect()
            };
            for f in found {
                if !out.contains(&f) {
                    out.push(f);
                }
            }
        }
        out
    };
    // Each file's name for its own: the one written, but for a name several define.
    let mut finals: HashMap<(usize, &str), String> = HashMap::new();
    for (n, in_files) in by_name.iter().filter(|(_, f)| f.len() > 1) {
        let what = meanings(n, in_files);
        if what.len() < 2 {
            continue;
        }
        // Where each comes from: a Rust function by its file, which is where to take it.
        let mut from: Vec<PathBuf> = Vec::new();
        for f in what {
            let f = if files.iter().any(|(k, _)| *k == f) { f } else { f.parent().map_or(f.clone(), Path::to_path_buf) };
            if !from.contains(&f) {
                from.push(f);
            }
        }
        links.shared.insert(n.to_string(), from);
        for i in in_files.iter().filter(|i| **i != 0) {
            if !tops[*i].iter().any(|(m, wide)| m == n && *wide) {
                let tag = crate::privacy::tag(Some(&files[*i].0.display().to_string()), "");
                finals.insert((*i, n), crate::privacy::mangle(n, &tag));
            }
        }
    }
    let final_of = |i: usize, n: &str| finals.get(&(i, n)).cloned().unwrap_or_else(|| n.to_string());

    for (i, (file, stmts)) in files.iter().enumerate() {
        // A file's own names, and those it takes from a Rust file by name (Q-816); one it takes
        // with a whole Rust file gives way to a name it takes from elsewhere.
        let own: HashSet<&str> = tops[i].iter().map(|(n, _)| n.as_str()).chain(rust[i].0.iter().filter(|r| !r.whole).map(|r| r.alias.as_str())).collect();
        let mut map: HashMap<String, String> = HashMap::new();
        for n in own.iter().copied().chain(rust[i].0.iter().map(|r| r.alias.as_str())) {
            if finals.contains_key(&(i, n)) {
                map.insert(n.to_string(), final_of(i, n));
            }
        }
        // What the file's Rust names are defined as: another name, or none.
        let mut as_defined: HashMap<String, Option<String>> = HashMap::new();
        for r in &rust[i].0 {
            if finals.contains_key(&(i, r.alias.as_str())) {
                as_defined.insert(r.alias.clone(), Some(final_of(i, &r.alias)));
            }
        }
        for n in &rust[i].1 {
            as_defined.insert(n.clone(), None);
        }
        if !as_defined.is_empty() {
            links.rust.insert(file.clone(), as_defined);
        }
        let mut imports = Imports::default();
        let mut whole: BTreeMap<&str, Vec<(String, usize)>> = BTreeMap::new();
        let mut named: HashMap<String, String> = HashMap::new();
        let mut renamed: Vec<(String, String)> = Vec::new();
        for s in stmts.iter() {
            let Stmt::Use(path, names, line, options) = s else { continue };
            if crate::ast::is_script(path) {
                imports.names.extend(names.iter().flatten().map(|(_, alias)| alias.clone()));
                continue;
            }
            let path = resolve_use(base, file, path, options.implied);
            let k = key(&base.join(&path));
            let target = files.iter().position(|(f, _)| *f == k);
            let mut problem = |message: String, name: Option<String>| links.problems.push(Problem { file: file.clone(), line: *line, message, name });
            let mut repeat = |message: String| links.repeats.push(Problem { file: file.clone(), line: *line, message, name: None });
            match (names, target) {
                (None, Some(t)) => {
                    if !imports.files.insert(k) {
                        repeat(format!("{} is taken a second time in this file: once is enough", path));
                        continue;
                    }
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
                            let public: Vec<&str> = tops[t].iter().map(|(m, _)| m.as_str()).collect();
                            let near = public.iter().copied().filter(|m| crate::check::edit_distance(n, m) <= 2).min_by_key(|m| (crate::check::edit_distance(n, m), *m));
                            problem(
                                if private {
                                    format!("`{}` is private to {}: make it `public` there, or take something public", n, path)
                                } else {
                                    format!("{} has no public name `{}`{}", path, n, near.map_or(String::new(), |m| format!(" (did you mean `{}`?)", m)))
                                },
                                Some(alias.clone()),
                            );
                            continue;
                        }
                        if own.contains(alias.as_str()) {
                            problem(format!("`{}` is defined in this file, and taken from {}: take it `as` another name", alias, path), None);
                            continue;
                        }
                        // `as` cannot give a name of the program's types, whose constructor it
                        // replaced in this file (FEEDBACK55.md, Q-821; branch feedback55).
                        if alias != n && types.contains(alias.as_str()) {
                            problem(format!("`{}` is a type of the program: take `{}` from {} under another name", alias, n, path), None);
                            continue;
                        }
                        let f = final_of(t, n);
                        if named.get(alias).is_some_and(|other| *other != f) {
                            problem(format!("`{}` is taken twice, the second time from {}: take one `as` another name", alias, path), None);
                            continue;
                        }
                        if named.get(alias).is_some() {
                            repeat(format!("`{}` is taken a second time from {}: drop this one", alias, path));
                            continue;
                        }
                        if n != alias {
                            renamed.push((n.clone(), alias.clone()));
                        }
                        named.insert(alias.clone(), f.clone());
                        imports.names.insert(f.clone());
                        if f != *alias {
                            map.insert(alias.clone(), f);
                        }
                        // A type taken `as` another name is a type by that name too, in what the
                        // file's annotations say (Q-821).
                        if n != alias && types.contains(n.as_str()) {
                            map.insert(format!("{}{}", crate::privacy::TYPE_ALIAS, alias), n.clone());
                            if !crate::ast::is_type_name(alias) {
                                repeat(format!("`{}` is `{}` here, but no type is written `{}` (a type's name is a capital letter and a small one, at least), so `p: {}` cannot say it: take it as `{}` or another such name", alias, n, alias, alias, crate::check::type_like(alias)));
                            }
                        }
                    }
                }
                // A plugin's names, or a file that is reported where it is used: by what the
                // program calls them (Q-816).
                (Some(list), None) => imports.names.extend(list.iter().map(|(_, alias)| final_of(i, alias))),
            }
        }
        // `use price as p`, then `price(1)`: still the other file's, though not taken by that name.
        let old: HashMap<String, String> = renamed
            .into_iter()
            .filter(|(n, _)| n.starts_with(|c: char| c.is_lowercase()) && !own.contains(n.as_str()) && !named.contains_key(n) && !whole.contains_key(n.as_str()))
            .collect();
        if !old.is_empty() {
            links.old_names.insert(file.clone(), old);
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

/// A name a file takes from a Rust file (Q-816).
struct RustName {
    alias: String,
    /// The Rust file, as `key` knows it, and its function.
    from: PathBuf,
    export: String,
    /// Taken with the whole file, not by name.
    whole: bool,
}

/// The names a file takes from the Rust files it uses (Q-816): those it names, and every function
/// of a file it takes whole; and, apart, those of a whole file that the file defines, or takes by
/// name from another file, which are not defined, as the file's own or named one wins.
fn rust_names(base: &Path, file: &Path, stmts: &[Stmt], own: &[(String, bool)]) -> (Vec<RustName>, Vec<String>) {
    let mut out = Vec::new();
    let mut wholes = Vec::new();
    let mut named: HashSet<&str> = own.iter().map(|(n, _)| n.as_str()).collect();
    for s in stmts {
        let Stmt::Use(path, names, _, options) = s else { continue };
        if crate::ast::is_script(path) {
            continue;
        }
        let path = resolve_use(base, file, path, options.implied);
        let from = key(&base.join(&path));
        match (names, crate::native::is_rust(&path)) {
            (Some(list), rust) => {
                named.extend(list.iter().map(|(_, alias)| alias.as_str()));
                if rust {
                    out.extend(list.iter().map(|(n, alias)| RustName { alias: alias.clone(), from: from.clone(), export: n.clone(), whole: false }));
                }
            }
            (None, true) => {
                if let Ok(src) = crate::error::read_source(base.join(&path)) {
                    wholes.extend(crate::native::parse(&src).exports.into_iter().map(|e| RustName { alias: e.name.clone(), from: from.clone(), export: e.name, whole: true }));
                }
            }
            (None, false) => {}
        }
    }
    let mut given_way = Vec::new();
    for r in wholes {
        if named.contains(r.alias.as_str()) {
            given_way.push(r.alias);
        } else {
            out.push(r);
        }
    }
    given_way.sort();
    given_way.dedup();
    (out, given_way)
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

    /// What `file`'s names from Rust files are defined as, where any is not as written.
    pub fn rust_names(&self, file: &Path) -> Option<&HashMap<String, Option<String>>> {
        self.rust.get(file)
    }

    /// Whether `file` took `name` by name: a browser library's, which only its own line gives.
    pub fn took(&self, file: &Path, name: &str) -> bool {
        self.imports.get(file).map_or(true, |i| i.names.contains(name))
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
    let (bom, text) = without_bom(text);
    let mut lines: Vec<String> = text.split('\n').map(String::from).collect();
    let at = r.line as usize - 1;
    let l = lines[at].clone();
    let body = l.trim_start();
    if body.starts_with("use ") {
        let indent = &l[..l.len() - body.len()];
        let eol = if l.ends_with('\r') { "\r\n" } else { "\n" };
        lines[at] = format!("{}{}{}", indent, r.written().replace('\n', &format!("{}{}", eol, indent)), after_path(&l));
    }
    format!("{}{}", bom, lines.join("\n"))
}

/// A file's byte-order mark, and the rest: `fix` writes after it, the mark kept first, where an
/// import written before it made the file fail to parse (FEEDBACK55.md, Q-837; branch feedback55).
fn without_bom(text: &str) -> (&str, &str) {
    match text.strip_prefix('\u{FEFF}') {
        Some(rest) => ("\u{FEFF}", rest),
        None => ("", text),
    }
}

/// The statement that takes a script, for another file that needs it too.
fn script_line(all: &[Respell], file: &str, line: u32) -> Result<String, String> {
    if let Some(r) = all.iter().find(|r| r.file == file && r.line == line) {
        return Ok(r.written());
    }
    let text = crate::error::read_source(file).map_err(|e| format!("cannot read {}: {}", file, e))?;
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
    let read = |files: &BTreeMap<String, String>, f: &str| files.get(f).cloned().map_or_else(|| crate::error::read_source(f).map_err(|e| format!("cannot read {}: {}", f, e)), Ok);
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
/// The line and the blank after it end as the file's lines do, CRLF in a CRLF file (FEEDBACK55.md,
/// Q-837; branch feedback55).
fn with_use(text: &str, line: &str) -> String {
    let (bom, text) = without_bom(text);
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let line = line.replace('\n', eol);
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
    // Where line `n` starts; past the end, a last line with no line end of its own.
    let start = |n: usize| lines[..n].iter().map(|l| l.len() + 1).sum::<usize>();
    let (at, added) = match last_use {
        Some(i) => (i + 1, format!("{}{}", line, eol)),
        None => (code, format!("{}{}{}", line, eol, eol)),
    };
    if at < lines.len() {
        let n = start(at);
        format!("{}{}{}{}", bom, &text[..n], added, &text[n..])
    } else if last_use.is_some() {
        format!("{}{}{}{}", bom, text, eol, line)
    } else {
        format!("{}{}{}{}{}", bom, text, eol, line, eol)
    }
}
