//! What a program's files reach, and what its developer confirmed of it (FEEDBACK83.md, Q-1300
//! to Q-1308, Part D; branch feedback82-85).
//!
//! `quillo confirm` reads every file of the program (the user's decision of 2026-11-21: "should
//! be every file"): each `.qwe` file it takes, the main file too, for the hosts its `http.*` calls
//! name, the paths its file builtins name, the scripts its pages load, the hosts its markup loads
//! from and its `js("fetch", ...)` calls, and the routes a taken file adds; and each Rust file by
//! its SHA-256, as nothing can hold Rust to a list. What the developer says yes to is written to
//! `quillo.confirmed` beside the main file, which is committed with the project:
//!
//! ```toml
//! ["lib/outside.qwe"]
//! http = ["nominatim.openstreetmap.org", "*.stripe.com"]
//! files = ["data/"]          # read only
//! writes = ["uploads/"]      # read and written
//! listens = ["udp 514"]      # a UDP port others reach
//! routes = ["GET /export", "server fn export"]   # a taken file's ways in
//!
//! ["ui/map.qwe"]
//! scripts = ["https://cdnjs.cloudflare.com/ajax/libs/leaflet/1.9.4/leaflet.min.js sha384-..."]
//! loads = ["tile.openstreetmap.org"]
//! calls = ["api.example.com"]
//!
//! ["airtable.qwer"]
//! sha256 = "..."
//! effects = "http, env, time"
//! ```
//!
//! Once the file is there, Quillo holds the program to it (the user's decision: "hold it"): an
//! `http.*` call to a host its file was not confirmed for, and a file builtin on a path it was
//! not, is an error value at the call, said once in the log; a script not confirmed is left out
//! of the page; a Rust file not confirmed at its hash does not load, and the start stops (the
//! user's decision: "refuse"). A program with no `quillo.confirmed` runs as it did, but under a
//! provider's policy (the host's layers and `QUILLO_POLICY_PATH`; not a `--policy` file, the
//! user's revision of 2026-11-23), which asks for a complete one (the user's decision: "policy must require
//! complete"); in the first release that has this, that is a warning naming the release from
//! which it stops the start (`STOPS_FROM`; the user's decision of 2026-11-22).

use crate::ast::*;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock, RwLock};

/// The file, beside the main file.
pub const FILE: &str = "quillo.confirmed";

/// The release from which a start under a provider's policy without a complete `quillo.confirmed`
/// stops, rather than warns: the one after the first that has `quillo confirm` (0.8.11), as the
/// user decided (FEEDBACK83.md, question 6, 2026-11-22; branch feedback82-85).
pub const STOPS_FROM: &str = "0.8.12";

/// Whether this Quillo, at `version`, stops such a start (see `STOPS_FROM`).
pub fn stops(version: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> { v.split(['.', '-', '+']).take(3).map(|p| p.parse().unwrap_or(0)).collect() };
    parts(version) >= parts(STOPS_FROM)
}

/// What is listed of a `.qwe` file, each with the key it has in the file and the words a
/// listing says it with.
pub const KINDS: &[(&str, &str)] = &[("http", "reaches"), ("files", "reads"), ("writes", "writes"), ("listens", "listens on"), ("scripts", "page script"), ("loads", "page loads"), ("calls", "page calls"), ("routes", "adds a route")];

/// Each item, with the lines it is written on (none for what was read from `quillo.confirmed`).
pub type Items = BTreeMap<String, Vec<u32>>;

/// One file's list: as found in it, or as confirmed.
#[derive(Debug, Default, Clone)]
pub struct Block {
    /// The hosts its `http.*` calls reach.
    pub http: Items,
    /// The paths its file builtins read: a folder ends with `/`.
    pub files: Items,
    /// The paths they write or delete (`write_file`, `delete_file`, `save:`, `save_upload`,
    /// `image.convert`'s, a database's file): a path confirmed only to be read is not written
    /// (FEEDBACK83.md, Q-1300, found in review; branch feedback82-85).
    pub writes: Items,
    /// The UDP ports it listens on (`udp 514`), held where the address is not this machine's
    /// loopback (found in review 2).
    pub listens: Items,
    /// The scripts its pages load (`client "https://..."`), by address.
    pub scripts: Items,
    /// The hosts its markup loads from (`src`, a component's default address).
    pub loads: Items,
    /// The hosts its pages call (`js("fetch", "https://...")`).
    pub calls: Items,
    /// The routes a file taken for some of its names adds (FEEDBACK83.md, Q-1301).
    pub routes: Items,
    /// A Rust file: its SHA-256, what its header says it does, and what a reading of it finds.
    pub rust: bool,
    pub sha256: Option<String>,
    pub effects: Option<String>,
    pub env: Items,
    pub crates: Items,
    /// A confirmed script's integrity hash (`sha384-...`), by its address (FEEDBACK83.md, Q-1304).
    pub integrity: BTreeMap<String, String>,
    /// What the reading says beside the list: a taken file that adds routes or works at start
    /// (Q-1301), a Rust file whose effects line leaves out what it calls (Q-1305). Found only.
    pub notes: Vec<(u32, String)>,
    /// A Rust file's written paths and addresses, and the effects a reading finds. Found only.
    pub written: Items,
    pub found_effects: crate::ast::Effects,
    /// The `server fn`s and tools of a taken file that the program took by name (`use x.qwe use
    /// load_notes`): the program's own ways in, never held (FEEDBACK83.md, Q-1301, found writing
    /// the guide: llms.py's note_list example; branch feedback82-85). Found only.
    pub named: Vec<String>,
    /// A file taken for some of its names, whose routes are held to the list. Found only.
    pub taken: bool,
    /// Its tools (`fn mcp`), whichever way it is taken. Found only.
    pub tools: Vec<String>,
}

impl Block {
    pub fn items(&self, kind: &str) -> &Items {
        match kind {
            "http" => &self.http,
            "files" => &self.files,
            "writes" => &self.writes,
            "listens" => &self.listens,
            "scripts" => &self.scripts,
            "loads" => &self.loads,
            "calls" => &self.calls,
            "routes" => &self.routes,
            "env" => &self.env,
            "crates" => &self.crates,
            _ => &self.written,
        }
    }

    fn items_mut(&mut self, kind: &str) -> Option<&mut Items> {
        Some(match kind {
            "http" => &mut self.http,
            "files" => &mut self.files,
            "writes" => &mut self.writes,
            "listens" => &mut self.listens,
            "scripts" => &mut self.scripts,
            "loads" => &mut self.loads,
            "calls" => &mut self.calls,
            "routes" => &mut self.routes,
            "env" => &mut self.env,
            "crates" => &mut self.crates,
            _ => return None,
        })
    }

    fn is_empty(&self) -> bool {
        !self.rust && KINDS.iter().all(|(k, _)| self.items(k).is_empty())
    }
}

fn add(items: &mut Items, item: String, line: u32) {
    let lines = items.entry(item).or_default();
    if line > 0 && !lines.contains(&line) {
        lines.push(line);
    }
}

// ---- the scan ----------------------------------------------------------------------------------

/// What a program's files reach, by file as `quillo.confirmed` names it: its path from the main
/// file's folder, with `/`.
#[derive(Debug, Default)]
pub struct Scan {
    pub files: BTreeMap<String, Block>,
}

/// The folder `quillo.confirmed` is in: the main file's.
pub fn dir_of(main: &Path) -> PathBuf {
    main.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new(".")).to_path_buf()
}

/// `file` as `quillo.confirmed` names it: from `dir` (both as they really are), `..` where it is
/// outside.
fn rel_name(dir: &Path, file: &Path) -> String {
    let d = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let f = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    if let Ok(r) = f.strip_prefix(&d) {
        return slashed(r);
    }
    let common = f.components().zip(d.components()).take_while(|(a, b)| a == b).count();
    let ups = d.components().count() - common;
    let mut parts: Vec<String> = std::iter::repeat_n("..".to_string(), ups).collect();
    parts.extend(f.components().skip(common).map(|c| c.as_os_str().to_string_lossy().into_owned()));
    parts.join("/")
}

/// `file` as `quillo.confirmed` in `dir` names it.
pub fn name_from(dir: &Path, file: &Path) -> String {
    rel_name(dir, file)
}

fn slashed(p: &Path) -> String {
    p.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
}

/// Reads the program from `main`: each file it takes by `use`, `mount` or `service`, once.
pub fn scan(main: &Path) -> Scan {
    scan_with(main, &HashMap::new(), true)
}

/// `scan`, with the files `quillo check` parsed already (`Analysis::parsed`) taken as they are;
/// `deep`: each file's code read through, not only its top level and its Rust files, which is
/// what `quillo check` says of a program with no `quillo.confirmed` and no policy that holds
/// hosts (a check of quillolang.org's site costs 2% more read through).
pub fn scan_with(main: &Path, parsed: &HashMap<PathBuf, std::sync::Arc<Vec<Stmt>>>, deep: bool) -> Scan {
    scan_from(main, &dir_of(main), parsed, deep)
}

/// A file of the program as read: its name in `quillo.confirmed`, its parse, and the files it takes
/// (each with the names it takes of it, or none for a file taken whole; false for a mounted program
/// or a service, which are programs of their own).
struct Read {
    name: String,
    stmts: std::sync::Arc<Vec<Stmt>>,
    takes: Vec<(PathBuf, Option<Vec<String>>, bool)>,
}

/// `scan_with`, from `main`, naming files from `dir`: a service started apart is held by its app's
/// `quillo.confirmed` (FEEDBACK83.md, Q-1300, found in review; branch feedback82-85).
pub fn scan_from(main: &Path, dir: &Path, parsed: &HashMap<PathBuf, std::sync::Arc<Vec<Stmt>>>, deep: bool) -> Scan {
    if !deep && !parsed.is_empty() {
        return scan_taken(main, parsed);
    }
    let dir = dir.to_path_buf();
    let mut out = Scan::default();
    // Each file once, a `use` in it counted from its own program's folder: a mounted program's
    // and a service's are theirs, as `native::walk_program` counts them (found in review).
    let main_key = crate::modules::key(main);
    let mut todo: Vec<(PathBuf, PathBuf)> = vec![(main.to_path_buf(), crate::modules::source_dir(main))];
    let mut read: HashMap<PathBuf, Read> = HashMap::new();
    while let Some((full, base)) = todo.pop() {
        let key = crate::modules::key(&full);
        if read.contains_key(&key) || out.files.contains_key(&rel_name(&dir, &full)) {
            continue;
        }
        let name = rel_name(&dir, &full);
        if crate::native::is_rust(&full.to_string_lossy()) {
            if let Ok(bytes) = crate::error::read_file(&full) {
                out.files.insert(name, if deep { scan_rust(&bytes) } else { scan_rust_said(&bytes) });
            }
            continue;
        }
        let stmts = match parsed.get(&key) {
            Some(s) => s.clone(),
            None => {
                let Ok(src) = crate::error::read_source(&full) else { continue };
                // A file that does not parse is `quillo check`'s to say; nothing is listed of it.
                let Some(stmts) = crate::interp::parse_kept(&src, &full) else { continue };
                stmts
            }
        };
        let mut takes = Vec::new();
        for s in stmts.iter() {
            match s {
                Stmt::Use(path, names, _, options) if !is_script(path) => {
                    let p = base.join(crate::modules::resolve_use(&base, &full, path, options.implied));
                    takes.push((crate::modules::key(&p), names.as_ref().map(|n| n.iter().map(|(n, _)| n.clone()).collect()), true));
                    todo.push((p, base.clone()));
                }
                Stmt::Mount(_, path, _) | Stmt::Service(_, _, path, _, _, _) if !is_address(path) => {
                    let p = base.join(path);
                    let own = p.parent().map_or_else(|| base.clone(), Path::to_path_buf);
                    takes.push((crate::modules::key(&p), None, false));
                    todo.push((p, own));
                }
                _ => {}
            }
        }
        read.insert(key, Read { name, stmts, takes });
    }
    // Which files are taken for some of their names: those a `use` takes names of, and those such
    // a file takes whole, through any number of files; a file taken both ways is taken, whatever
    // the order the files were read in (FEEDBACK83.md, Q-1301, found in review; branch
    // feedback82-85). The main file is the program's own.
    let mut taken: HashMap<PathBuf, Vec<String>> = HashMap::new();
    for r in read.values() {
        for (k, names, _) in &r.takes {
            if let Some(n) = names.as_ref().filter(|_| *k != main_key) {
                taken.entry(k.clone()).or_default().extend(n.iter().cloned());
            }
        }
    }
    loop {
        let mut more: Vec<(PathBuf, String)> = Vec::new();
        for (k, r) in &read {
            if !taken.contains_key(k) {
                continue;
            }
            for (t, names, used) in &r.takes {
                if *used && names.is_none() && *t != main_key && !taken.contains_key(t) && read.contains_key(t) {
                    more.push((t.clone(), format!("what {} takes", r.name)));
                }
            }
        }
        if more.is_empty() {
            break;
        }
        for (t, why) in more {
            taken.entry(t).or_default().push(why);
        }
    }
    for (key, r) in read {
        let names = taken.get(&key);
        let mut b = Block { taken: names.is_some(), ..Default::default() };
        let mut routes: Vec<(String, u32)> = Vec::new();
        let mut at_start: Vec<u32> = Vec::new();
        for s in r.stmts.iter() {
            match s {
                Stmt::Use(path, _, line, _) if is_script(path) && is_address(path) => add(&mut b.scripts, path.clone(), *line),
                Stmt::Route(rd) if b.taken => routes.push((rd.shown(), rd.line)),
                // A taken file's `server fn` is called from the browser, and its `fn mcp` by an AI
                // client: ways in, as its routes are (Q-1301, found in review).
                Stmt::Fn(d) if b.taken && (d.server || d.mcp) && names.is_some_and(|n| n.contains(&d.name)) => b.named.push(entry_name(d)),
                Stmt::Fn(d) if b.taken && (d.server || d.mcp) => routes.push((entry_name(d), d.line)),
                Stmt::Let(_, e, line) | Stmt::Expr(e @ Expr::Call(_, _, line)) if b.taken && reaches_out(e) => at_start.push(*line),
                _ => {}
            }
            if let Stmt::Fn(d) = s {
                if d.mcp {
                    b.tools.push(d.name.clone());
                }
                if d.component {
                    // A component's default address, handed on to what it shows (B.2: map tiles).
                    for p in &d.params {
                        if let Some(h) = p.default.as_ref().and_then(|e| origin_written(e)) {
                            add(&mut b.loads, h, d.line);
                        }
                    }
                }
            }
        }
        for (shown, line) in &routes {
            add(&mut b.routes, shown.clone(), *line);
        }
        if deep {
            read_calls(&r.stmts, &mut b);
        }
        // A file taken for some of its names that also adds ways in or works as the program
        // starts (FEEDBACK83.md, Q-1301; branch feedback82-85).
        if let Some(n) = names.and_then(|names| taken_note(&r.name, names, &routes, &at_start)) {
            b.notes.push(n);
        }
        out.files.insert(r.name, b);
    }
    out
}

/// How a `server fn` or a tool is listed among a file's ways in.
fn entry_name(d: &FnDef) -> String {
    if d.mcp {
        format!("fn mcp {}", d.name)
    } else {
        format!("server fn {}", d.name)
    }
}

/// What `quillo check` says of a program nothing holds, from the files it parsed: each file taken
/// for some of its names that adds routes or reaches out at start (Q-1301), and each Rust file
/// whose effects line leaves out what it calls (Q-1305). Only those `use` lines are looked up,
/// as each lookup asks the system where the file is (a check of quillolang.org's site has 150).
fn scan_taken(main: &Path, parsed: &HashMap<PathBuf, std::sync::Arc<Vec<Stmt>>>) -> Scan {
    let dir = dir_of(main);
    let base = crate::modules::source_dir(main);
    let mut out = Scan::default();
    let mut taken: HashMap<PathBuf, Vec<String>> = HashMap::new();
    for (file, stmts) in parsed {
        for s in stmts.iter() {
            let Stmt::Use(path, names, _, options) = s else { continue };
            if is_script(path) || (names.is_none() && !crate::native::is_rust(path) && !options.implied) {
                continue;
            }
            let full = base.join(crate::modules::resolve_use(&base, file, path, options.implied));
            if crate::native::is_rust(&full.to_string_lossy()) {
                let name = rel_name(&dir, &full);
                if !out.files.contains_key(&name) {
                    if let Ok(bytes) = crate::error::read_file(&full) {
                        out.files.insert(name, scan_rust_said(&bytes));
                    }
                }
            } else if let Some(names) = names {
                taken.entry(crate::modules::key(&full)).or_default().extend(names.iter().map(|(n, _)| n.clone()));
            }
        }
    }
    for (key, names) in taken {
        let Some(stmts) = parsed.get(&key) else { continue };
        let name = rel_name(&dir, &key);
        let mut routes: Vec<(String, u32)> = Vec::new();
        let mut at_start: Vec<u32> = Vec::new();
        for s in stmts.iter() {
            match s {
                Stmt::Route(r) => routes.push((r.shown(), r.line)),
                Stmt::Fn(d) if (d.server || d.mcp) && !names.contains(&d.name) => routes.push((entry_name(d), d.line)),
                Stmt::Let(_, e, line) | Stmt::Expr(e @ Expr::Call(_, _, line)) if reaches_out(e) => at_start.push(*line),
                _ => {}
            }
        }
        let mut b = Block { taken: true, ..Default::default() };
        if let Some(n) = taken_note(&name, &names, &routes, &at_start) {
            b.notes.push(n);
        }
        out.files.insert(name, b);
    }
    out
}

/// What is said of a file taken for `names` that adds `routes` or reaches out at start.
fn taken_note(name: &str, names: &[String], routes: &[(String, u32)], at_start: &[u32]) -> Option<(u32, String)> {
    if routes.is_empty() && at_start.is_empty() {
        return None;
    }
    let mut also = Vec::new();
    if !routes.is_empty() {
        also.push(format!("adds {}", routes.iter().map(|(r, _)| r.as_str()).collect::<Vec<_>>().join(", ")));
    }
    if !at_start.is_empty() {
        also.push(format!("reaches out as the program starts (line {})", at_start.iter().map(u32::to_string).collect::<Vec<_>>().join(", ")));
    }
    let line = routes.first().map(|r| r.1).into_iter().chain(at_start.first().copied()).min().unwrap_or(0);
    let mut names = names.to_vec();
    names.dedup();
    Some((line, format!("{} is taken for {}, and also {}", name, names.join(", "), also.join(" and "))))
}

/// Whether `e` calls something that reaches outside the program: `http.*`, a file builtin, the
/// database, `env`.
fn reaches_out(e: &Expr) -> bool {
    let mut hit = false;
    crate::check::walk_expr(e, &mut |x| {
        if let Expr::Call(c, _, _) = x {
            let n = callee(c);
            hit |= n.starts_with("http.") || n.starts_with("db.") || n.starts_with("udp.") || matches!(n.as_str(), "read_file" | "write_file" | "delete_file" | "env" | "__site" | "file" | "files" | "save_upload");
        }
    });
    hit
}

/// A call's name as written: `read_file`, `http.get`.
fn callee(c: &Expr) -> String {
    match c {
        Expr::Ident(n, _) => n.to_string(),
        Expr::Field(o, f, _) => match &**o {
            Expr::Ident(m, _) => format!("{}.{}", m, f),
            _ => String::new(),
        },
        _ => String::new(),
    }
}

/// The calls and markup of a file's code, into its list.
fn read_calls(stmts: &[Stmt], b: &mut Block) {
    // A call given its first argument by a pipe (`url | http.get()`): read with it.
    let mut piped: HashSet<*const Expr> = HashSet::new();
    crate::check::each_expr_of(stmts, &mut |e| match e {
        Expr::Pipe(lhs, rhs, _) => {
            if let Expr::Call(c, args, line) = &**rhs {
                piped.insert(&**rhs as *const Expr);
                let mut all: Vec<&Expr> = vec![&**lhs];
                all.extend(args.iter());
                read_call(&callee(c), &all, *line, b);
            }
        }
        Expr::Call(c, args, line) if !piped.contains(&(e as *const Expr)) => {
            let all: Vec<&Expr> = args.iter().collect();
            read_call(&callee(c), &all, *line, b);
        }
        Expr::Html(root, _) => {
            // Its elements, through their children: the expressions among them come here by the
            // walk, with any markup they hold.
            let mut els = vec![&**root];
            while let Some(el) = els.pop() {
                for (name, v) in &el.attrs {
                    let AttrVal::Expr(v) = v else { continue };
                    let loads = match name.as_str() {
                        "src" | "poster" => true,
                        "href" => el.tag == "link",
                        "data" => el.tag == "object",
                        _ => false,
                    };
                    if let Some(h) = origin_written(v).filter(|_| loads) {
                        add(&mut b.loads, h, el.line);
                    }
                }
                els.extend(el.children.iter().filter_map(|c| if let HtmlChild::El(e) = c { Some(e) } else { None }));
            }
        }
        _ => {}
    });
}

fn read_call(name: &str, args: &[&Expr], line: u32, b: &mut Block) {
    // A path written: what is only read apart from what is written, which a read confirmed does not
    // let a file do (FEEDBACK83.md, Q-1300, found in review; branch feedback82-85).
    let path = |at: usize, write: bool, b: &mut Block| {
        if let Some(p) = args.get(at).and_then(|e| path_written(e)) {
            add(if write { &mut b.writes } else { &mut b.files }, p, line);
        }
    };
    match name {
        "http.get" | "http.post" | "http.put" | "http.patch" | "http.delete" | "http.stream" | "http.request" => {
            let at = if name == "http.request" { 1 } else { 0 };
            if let Some(h) = args.get(at).and_then(|e| host_written(e)) {
                add(&mut b.http, h, line);
            }
            // `save:`, a file written (FEEDBACK85.md, Q-1342).
            for a in args {
                if let Expr::Map(fields) = a {
                    if let Some(p) = fields.iter().find(|(k, _)| k == "save").and_then(|(_, v)| path_written(v)) {
                        add(&mut b.writes, p, line);
                    }
                }
            }
        }
        // `site shop = "https://..."` (`__site`), and a UDP packet's host.
        "__site" => {
            if let Some(h) = args.get(1).and_then(|e| host_written(e)) {
                add(&mut b.http, h, line);
            }
        }
        "udp.send" => {
            if let Some(Expr::Lit(Lit::Str(h))) = args.first() {
                add(&mut b.http, h.trim_start_matches('[').trim_end_matches(']').to_ascii_lowercase(), line);
            }
        }
        // A database: each host its driver connects to, a replica's too, or a file it writes; a
        // target worked out is not listed, and refused where it is used (found in review 2).
        "db.open" | "db.connect" | "db.system" => {
            match args.first() {
                Some(e) if written_start(e).is_some_and(|(s, _)| crate::db::server_hosts(&s).is_some() || s.contains("://")) => {
                    if let Some(Ok(hosts)) = written_start(e).filter(|(_, whole)| *whole).map(|(s, _)| db_hosts(&s)) {
                        for h in hosts {
                            add(&mut b.http, h, line);
                        }
                    }
                }
                Some(Expr::Lit(Lit::Str(f))) if &**f == ":memory:" => {}
                _ => path(0, true, b),
            }
            if let Some(Expr::Map(fields)) = args.get(1) {
                if let Some((_, Expr::List(urls))) = fields.iter().find(|(k, _)| k == "replicas") {
                    for u in urls {
                        if let Some(Ok(hosts)) = written_start(u).filter(|(_, whole)| *whole).map(|(s, _)| db_hosts(&s)) {
                            for h in hosts {
                                add(&mut b.http, h, line);
                            }
                        }
                    }
                }
            }
        }
        // RTP ports, on this machine's loopback unless `host` says another address.
        "media.rtp" => {
            if let Some(Expr::Map(fields)) = args.get(1) {
                let wide = fields.iter().any(|(k, v)| k == "host" && !matches!(v, Expr::Lit(Lit::Str(h)) if h.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback()) || &**h == "localhost"));
                for (k, v) in fields {
                    if let (true, "video" | "audio", Expr::Lit(Lit::Int(port))) = (wide, k.as_str(), v) {
                        add(&mut b.listens, format!("udp {}", port), line);
                    }
                }
            }
        }
        "udp.listen" => {
            let loopback = matches!(args.get(2), Some(Expr::Map(f)) if f.iter().any(|(k, v)| k == "host" && matches!(v, Expr::Lit(Lit::Str(h)) if h.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback()) || &**h == "localhost")));
            if let (Some(Expr::Lit(Lit::Int(port))), false) = (args.first(), loopback) {
                add(&mut b.listens, format!("udp {}", port), line);
            }
        }
        "read_file" | "file" | "image.info" | "check_program" => path(0, false, b),
        // `files("public", path)`: the folder.
        "files" => {
            if let Some(p) = args.first().and_then(|e| written_start(e)).filter(|(_, whole)| *whole).map(|(s, _)| normal(&format!("{}/", s.trim_end_matches('/')))) {
                add(&mut b.files, p, line);
            }
        }
        "write_file" | "delete_file" => path(0, true, b),
        "save_upload" => path(1, true, b),
        "image.convert" => {
            path(0, false, b);
            path(1, true, b);
        }
        "js" => {
            if matches!(args.first(), Some(Expr::Lit(Lit::Str(f))) if &**f == "fetch") {
                if let Some(h) = args.get(1).and_then(|e| origin_written(e)) {
                    add(&mut b.calls, h, line);
                }
            }
        }
        _ => {}
    }
}

/// The text a value starts with as written, and whether that is all of it: a text, a template's
/// first part, `"..." + x`.
fn written_start(e: &Expr) -> Option<(String, bool)> {
    match e {
        Expr::Lit(Lit::Str(s)) => Some((s.to_string(), true)),
        Expr::Template(parts) => match parts.first() {
            Some(TPart::Lit(s)) => Some((s.clone(), parts.len() == 1)),
            _ => None,
        },
        Expr::Binary(BinOp::Add, a, _, _) => written_start(a).map(|(s, _)| (s, false)),
        _ => None,
    }
}

/// The host of the address `e` writes, where its host is written out: `"https://api.x.com/" +
/// id` has one, `"https://" + host` none.
pub fn host_written(e: &Expr) -> Option<String> {
    let (s, whole) = written_start(e)?;
    host_of(&s, whole)
}

/// `url` as the HTTP client reads it, an `http(s)://` address (or `//host`, as a page reads it);
/// `whole`: all of it is written, else only where its host ends within it. Read by the `url` crate,
/// as the call reads it: `http://127.0.0.1:9\\@good.example/` goes to 127.0.0.1, where reading it
/// by hand took `good.example` (FEEDBACK83.md, Q-1300, found in review; branch feedback82-85).
fn url_of(url: &str, whole: bool) -> Option<url::Url> {
    let url = if url.starts_with("//") { format!("https:{}", url) } else { url.to_string() };
    let lower = url.get(..8).unwrap_or(&url).to_ascii_lowercase();
    let at = if lower.starts_with("https://") {
        8
    } else if lower.starts_with("http://") {
        7
    } else {
        return None;
    };
    let written = if whole {
        url.as_str()
    } else {
        // The host ends at the first of these, `\\` among them, as the parser takes it.
        let end = url[at..].find(['/', '?', '#', '\\'])?;
        &url[..at + end]
    };
    url::Url::parse(written).ok()
}

/// The host of `url` (see `url_of`), lower-case, an IPv6 address without its brackets.
pub fn host_of(url: &str, whole: bool) -> Option<String> {
    let u = url_of(url, whole)?;
    let host = u.host_str()?.trim_start_matches('[').trim_end_matches(']').trim_end_matches('.').to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

/// The host of `url` with its port where one is written: what a page's policy names
/// (`localhost:5173`), as a host alone does not take another port.
fn origin_host(url: &str, whole: bool) -> Option<String> {
    let u = url_of(url, whole)?;
    let host = u.host_str()?.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        return None;
    }
    Some(match u.port() {
        Some(p) => format!("{}:{}", host, p),
        None => host,
    })
}

/// `origin_host` of the address `e` writes, for what a page loads or calls.
fn origin_written(e: &Expr) -> Option<String> {
    let (s, whole) = written_start(e)?;
    origin_host(&s, whole)
}

/// The path `e` names, where it is written out: a text, or the folder a worked-out one starts
/// with (`"uploads/" + name` is `uploads/`).
fn path_written(e: &Expr) -> Option<String> {
    let (s, whole) = written_start(e)?;
    if whole {
        let n = normal(&s);
        return (!n.is_empty()).then_some(n);
    }
    let folder = &s[..s.rfind('/')? + 1];
    let n = normal(folder);
    (!n.is_empty()).then(|| if n.ends_with('/') { n } else { format!("{}/", n) })
}

/// A path as it is compared: `./` and `x/..` taken away, `/` between its parts, a folder's `/`
/// kept at its end.
pub fn normal(p: &str) -> String {
    let folder = p.ends_with('/') && p.len() > 1;
    let mut out: Vec<String> = Vec::new();
    let mut root = false;
    for c in Path::new(p).components() {
        match c {
            Component::RootDir => root = true,
            Component::Prefix(x) => out.push(x.as_os_str().to_string_lossy().into_owned()),
            Component::CurDir => {}
            Component::ParentDir => {
                if out.last().is_some_and(|l| l != "..") {
                    out.pop();
                } else if !root {
                    out.push("..".into());
                }
            }
            Component::Normal(n) => out.push(n.to_string_lossy().into_owned()),
        }
    }
    let mut s = out.join("/");
    if root {
        s.insert(0, '/');
    }
    if folder && !s.is_empty() && !s.ends_with('/') {
        s.push('/');
    }
    s
}

// ---- Rust files --------------------------------------------------------------------------------

/// What a reading of a Rust file's code takes for an effect: each a call's start, and its effect
/// (FEEDBACK83.md, Q-1305 and Q-1308; branch feedback82-85).
const RUST_CALLS: &[(&str, &str)] = &[
    ("std::fs::", "fs"),
    ("fs::read", "fs"),
    ("fs::write", "fs"),
    ("fs::remove", "fs"),
    ("fs::create_dir", "fs"),
    ("fs::copy", "fs"),
    ("fs::rename", "fs"),
    ("fs::metadata", "fs"),
    ("File::open", "fs"),
    ("File::create", "fs"),
    ("OpenOptions::new", "fs"),
    // A crate's own way to a file by its path: `image::open(path)`, `WavReader::open(path)`,
    // `Reader::from_path(path)`, `Document::load(path)`, `img.save(path)`.
    ("::open(", "fs"),
    ("::create(", "fs"),
    ("::from_path(", "fs"),
    ("::from_file(", "fs"),
    ("::load(", "fs"),
    (".save(", "fs"),
    // A builder's own way to its file: `csv::ReaderBuilder::new().from_path(path)`, and `.open(path)`
    // where the statement starts at a builder; and what asks about a path: `Path::new(p).exists()`,
    // `dir.read_dir()` (FEEDBACK83.md, Q-1305; branch feedback82-85: the catalog's csv.qwer and
    // cli_run.qwer said fs, the reading did not).
    (".from_path(", "fs"),
    (".open(", "fs"),
    (".exists()", "fs"),
    (".try_exists()", "fs"),
    (".is_dir()", "fs"),
    (".is_file()", "fs"),
    (".is_symlink()", "fs"),
    (".metadata()", "fs"),
    (".symlink_metadata()", "fs"),
    (".canonicalize()", "fs"),
    ("read_dir(", "fs"),
    // A file read as the plugin is built, into it: `include_str!("/etc/hosts")`, `include_bytes!`,
    // and `include!`, whose code the reading never sees (FEEDBACK83.md, Q-1305, found by the
    // coverage tests; branch feedback82-85).
    ("include_str!", "fs"),
    ("include_bytes!", "fs"),
    ("include!", "fs"),
    ("TcpStream::", "net"),
    ("TcpListener::", "net"),
    ("UdpSocket::", "net"),
    ("UnixStream::", "net"),
    // A name looked up: DNS (FEEDBACK83.md, Q-1305; branch feedback82-85: ssrf_guard.qwer).
    ("to_socket_addrs", "net"),
    ("ToSocketAddrs", "net"),
    ("lookup_host", "net"),
    ("ureq::", "http"),
    ("reqwest::", "http"),
    ("hyper::", "http"),
    ("attohttpc::", "http"),
    ("isahc::", "http"),
    ("env::var", "env"),
    ("SystemTime::now", "time"),
    ("Instant::now", "time"),
    ("Utc::now", "time"),
    ("Local::now", "time"),
    ("OffsetDateTime::now", "time"),
    // A wait reads the clock: `std::thread::sleep(d)`, tokio's `time::sleep(d)` (FEEDBACK83.md,
    // Q-1305; branch feedback82-85: 29 catalog plugins retry after a wait).
    ("sleep(", "time"),
    ("rand::", "random"),
    ("thread_rng", "random"),
    ("OsRng", "random"),
    ("getrandom", "random"),
    ("fastrand::", "random"),
    // std's own seed for a hash map, taken as a random number (FEEDBACK83.md, Q-1305; branch
    // feedback82-85: spotify.qwer, square.qwer).
    ("RandomState::new", "random"),
    ("RandomState::default", "random"),
    ("process::Command", "run"),
    ("Command::new", "run"),
];

/// Crates that do what an effect names themselves, whatever the file's own code calls: a file
/// that takes one is read as doing it. Read wide, as an `//! effects:` line that says too much
/// refuses a call, and one that says too little lets it through.
const RUST_CRATES: &[(&str, &str)] = &[
    ("ureq", "http"),
    ("reqwest", "http"),
    ("hyper", "http"),
    ("isahc", "http"),
    ("attohttpc", "http"),
    ("tungstenite", "net"),
    ("tokio-rustls", "net"),
    ("h2", "net"),
    ("socket2", "net"),
    ("russh", "net"),
    ("russh-sftp", "net"),
    ("redis", "net"),
    ("suppaftp", "net"),
    ("rumqttc", "net"),
    ("rskafka", "net"),
    ("mongodb", "net"),
    ("lettre", "net"),
    ("ldap3", "net"),
    ("lapin", "net"),
    ("async-nats", "net"),
    ("zeromq", "net"),
    ("tiberius", "net"),
    ("hickory-resolver", "net"),
    ("dns-lookup", "net"),
    ("mdns-sd", "net"),
    ("rusqlite", "fs"),
    ("duckdb", "fs"),
    ("redb", "fs"),
    ("polodb_core", "fs"),
    ("cacache", "fs"),
    ("walkdir", "fs"),
    ("fs4", "fs"),
    ("gix", "fs"),
    ("serialport", "fs"),
    ("nusb", "fs"),
    ("sysinfo", "fs"),
    ("tantivy", "fs"),
    ("rand", "random"),
    ("getrandom", "random"),
    ("nanoid", "random"),
    ("cuid2", "random"),
    ("petname", "random"),
    ("fake", "random"),
    ("ulid", "random"),
    ("ulid", "time"),
    ("svix-ksuid", "random"),
    ("svix-ksuid", "time"),
    // What these do unseen (FEEDBACK83.md, Q-1305; branch feedback82-85): a CUID2 takes the time
    // in; a PASETO token its time and nonce; a captcha, a Bloom filter's seeds (bloomfilter's
    // default `random` feature) and feed-rs's ids for entries without one are random; an
    // icalendar event gets a DTSTAMP and a UID where it has none.
    ("cuid2", "time"),
    ("pasetors", "time"),
    ("pasetors", "random"),
    ("captcha", "random"),
    ("bloomfilter", "random"),
    ("feed-rs", "random"),
    ("icalendar", "time"),
    ("icalendar", "random"),
];

/// Crates that do what an effect names only where the file takes a feature of theirs
/// (`feature:v4` in its dependency line) or calls one of theirs (any text of the code): `uuid`'s
/// v4 and v7, `bcrypt::hash`'s salt (FEEDBACK83.md, Q-1305; branch feedback82-85).
const RUST_CRATE_USES: &[(&str, &[&str], &str)] = &[
    ("uuid", &["feature:v4", "feature:v7", "new_v4", "now_v7"], "random"),
    ("uuid", &["feature:v7", "now_v7"], "time"),
    ("totp-rs", &["generate_secret", "gen_secret("], "random"),
    ("bcrypt", &["hash(", "hash_with_result("], "random"),
    ("biscuit-auth", &["KeyPair::new"], "random"),
    ("biscuit-auth", &[".time()"], "time"),
    ("age", &["Identity::generate", "encrypt", "Encryptor"], "random"),
    ("sharks", &["dealer("], "random"),
    ("bson", &["ObjectId::new"], "time"),
    ("bson", &["ObjectId::new"], "random"),
    ("epub-builder", &["generate("], "time"),
    ("epub-builder", &["generate("], "random"),
    ("printpdf", &["PdfDocument::new"], "time"),
    ("rusty-s3", &["sign("], "time"),
    ("pgp", &["sign"], "time"),
    ("lettre", &["Message::builder"], "time"),
    ("lettre", &["Message::builder"], "random"),
];

/// Calls of a wide pattern (`::open(`, `::create(`, `::from_path(`) that take no file: redis's
/// `Client::open(url)`, rust-stemmers' `Stemmer::create(language)`, tantivy's
/// `SnippetGenerator::create(..)`, `mime_guess::from_path(name)` (a name's extension) (FEEDBACK83.md,
/// Q-1305; branch feedback82-85).
const RUST_NOT_FILES: &[&str] = &["Client::open", "Stemmer::create", "SnippetGenerator::create", "mime_guess::from_path"];

/// A Rust file's code with its comments taken out and each string's text out of it (`""` in its
/// place), lines kept; and the strings, each with its line and where it stood in the code.
fn rust_code(src: &str) -> (String, Vec<(u32, usize, String)>) {
    let b = src.as_bytes();
    let mut code = String::with_capacity(src.len());
    let mut strings = Vec::new();
    let (mut i, mut line) = (0usize, 1u32);
    let mut start = 0;
    let flush = |code: &mut String, from: usize, to: usize| code.push_str(&src[from..to]);
    while i < b.len() {
        let c = b[i];
        if c == b'\n' {
            line += 1;
            i += 1;
            continue;
        }
        if c == b'/' && b.get(i + 1) == Some(&b'/') {
            flush(&mut code, start, i);
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            start = i;
            continue;
        }
        if c == b'/' && b.get(i + 1) == Some(&b'*') {
            flush(&mut code, start, i);
            let mut depth = 0;
            while i < b.len() {
                if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
                    depth += 1;
                    i += 2;
                } else if b[i] == b'*' && b.get(i + 1) == Some(&b'/') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    if b[i] == b'\n' {
                        line += 1;
                        code.push('\n');
                    }
                    i += 1;
                }
            }
            start = i;
            continue;
        }
        // A raw string: `r"..."`, `r#"..."#`, `br"..."`.
        let ident_before = i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_');
        if c == b'r' && !ident_before || (c == b'b' && b.get(i + 1) == Some(&b'r') && !ident_before) {
            let mut j = i + if c == b'b' { 2 } else { 1 };
            let mut hashes = 0;
            while b.get(j) == Some(&b'#') {
                hashes += 1;
                j += 1;
            }
            if b.get(j) == Some(&b'"') {
                flush(&mut code, start, j);
                let close: String = std::iter::once('"').chain(std::iter::repeat_n('#', hashes)).collect();
                let body_start = j + 1;
                let end = src[body_start..].find(&close).map_or(b.len(), |n| body_start + n);
                let text = &src[body_start..end];
                strings.push((line, code.len(), text.to_string()));
                line += text.matches('\n').count() as u32;
                code.push_str("\"\"");
                code.extend(std::iter::repeat_n('\n', text.matches('\n').count()));
                i = (end + close.len()).min(b.len());
                start = i;
                continue;
            }
        }
        if c == b'"' {
            flush(&mut code, start, i);
            let mut j = i + 1;
            while j < b.len() && b[j] != b'"' {
                j += if b[j] == b'\\' { 2 } else { 1 };
            }
            let j = j.min(b.len());
            // As written, its escapes kept: what is looked for in it is an address or a name.
            let text = src.get(i + 1..j).unwrap_or("").to_string();
            strings.push((line, code.len(), text.clone()));
            let n = text.matches('\n').count() as u32;
            line += n;
            code.push_str("\"\"");
            code.extend(std::iter::repeat_n('\n', n as usize));
            i = (j + 1).min(b.len());
            start = i;
            continue;
        }
        if c == b'\'' {
            // A character, not a lifetime: `'x'`, `'\n'`, `'"'`.
            let next = src[i + 1..].chars().next();
            let after = next.map(|ch| i + 1 + ch.len_utf8());
            if next == Some('\\') {
                let end = src.get(i + 3..).and_then(|r| r.find('\'')).map_or(b.len(), |n| i + 3 + n + 1);
                flush(&mut code, start, i);
                code.push_str("' '");
                i = end;
                start = i;
                continue;
            }
            if let Some(a) = after.filter(|a| b.get(*a) == Some(&b'\'')) {
                flush(&mut code, start, i);
                code.push_str("' '");
                i = a + 1;
                start = i;
                continue;
            }
        }
        i += 1;
    }
    flush(&mut code, start.min(b.len()), b.len());
    (code, strings)
}

/// A byte of a Rust name: ASCII's, and any byte of a character past it (a name may be Unicode).
fn rust_name_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80
}

/// `code` (`rust_code`'s) with each path written as one, `std :: fs :: read` as `std::fs::read`,
/// and no space between a name (or a macro's `!`) and its `(`, `read (p)` as `read(p)`; a line
/// break taken out is put back at the end of its line, so each call keeps its line.
fn rust_joined(code: &str) -> String {
    let b = code.as_bytes();
    let space = |c: u8| matches!(c, b' ' | b'\t' | b'\n' | b'\r');
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut owed = 0usize;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b':' && b.get(i + 1) == Some(&b':') {
            while out.last().is_some_and(|l| space(*l)) {
                owed += usize::from(out.pop() == Some(b'\n'));
            }
            out.extend_from_slice(b"::");
            i += 2;
            while i < b.len() && space(b[i]) {
                owed += usize::from(b[i] == b'\n');
                i += 1;
            }
            continue;
        }
        if space(c) {
            let j = i + b[i..].iter().take_while(|x| space(**x)).count();
            if b.get(j) == Some(&b'(') && out.last().is_some_and(|l| rust_name_byte(*l) || *l == b'!') {
                owed += b[i..j].iter().filter(|x| **x == b'\n').count();
                i = j;
                continue;
            }
        }
        if c == b'\n' {
            out.extend(std::iter::repeat_n(b'\n', owed));
            owed = 0;
        }
        out.push(c);
        i += 1;
    }
    out.extend(std::iter::repeat_n(b'\n', owed));
    // Only ASCII spaces were taken out, and line breaks put in between characters.
    String::from_utf8(out).unwrap_or_else(|_| code.to_string())
}

/// The names `use` declarations (and `extern crate x as y`) in `code` give: each name, the path
/// it stands for, and whether it is another name for it (`as`); and where each declaration is.
/// A name of the file's own (`crate::`, `self::`, `super::`) is read where it is written.
fn rust_use_names(code: &str) -> (Vec<(String, String, bool)>, Vec<(usize, usize)>) {
    let b = code.as_bytes();
    let mut names = Vec::new();
    let mut spans = Vec::new();
    for kw in ["use", "extern crate"] {
        let mut from = 0;
        while let Some(at) = code[from..].find(kw).map(|a| a + from) {
            from = at + kw.len();
            let after = b.get(from).copied().unwrap_or(b';');
            if (at > 0 && (rust_name_byte(b[at - 1]) || b[at - 1] == b':')) || !(matches!(after, b' ' | b'\t' | b'\n' | b'\r' | b'{') || (kw == "use" && after == b':')) {
                continue;
            }
            let Some(end) = code[from..].find(';').map(|e| e + from) else { break };
            spans.push((at, end));
            let toks = rust_use_tokens(&code[from..end]);
            let mut i = 0;
            if kw == "use" {
                rust_use_tree(&toks, &mut i, &[], &mut names);
            } else if let [name, rest @ ..] = toks.as_slice() {
                if let ["as", alias] = rest {
                    names.push((alias.to_string(), name.to_string(), true));
                }
            }
        }
    }
    names.retain(|(n, full, _)| n != "_" && *n != *full && !["crate", "self", "super"].iter().any(|s| full.split("::").next() == Some(*s)));
    (names, spans)
}

fn rust_use_tokens(s: &str) -> Vec<&str> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if rust_name_byte(c) {
            let n = b[i..].iter().take_while(|x| rust_name_byte(**x)).count();
            out.push(&s[i..i + n]);
            i += n;
        } else if c == b':' && b.get(i + 1) == Some(&b':') {
            out.push("::");
            i += 2;
        } else {
            if matches!(c, b'{' | b'}' | b',' | b'*') {
                out.push(&s[i..i + 1]);
            }
            i += 1;
        }
    }
    out
}

/// One tree of a `use` (`std::{fs as f, net::TcpStream}`) under `prefix`, its names into `out`.
fn rust_use_tree(toks: &[&str], i: &mut usize, prefix: &[String], out: &mut Vec<(String, String, bool)>) {
    let mut path = prefix.to_vec();
    while let Some(t) = toks.get(*i).copied() {
        match t {
            "::" => *i += 1,
            "*" | "}" | "," => {
                *i += usize::from(t == "*");
                return;
            }
            "{" => {
                *i += 1;
                while toks.get(*i).is_some_and(|t| *t != "}") {
                    let at = *i;
                    rust_use_tree(toks, i, &path, out);
                    if toks.get(*i) == Some(&",") {
                        *i += 1;
                    } else if *i == at {
                        *i += 1;
                    }
                }
                *i += 1;
                return;
            }
            name => {
                *i += 1;
                if toks.get(*i) == Some(&"::") {
                    path.push(name.to_string());
                    continue;
                }
                if name != "self" {
                    path.push(name.to_string());
                }
                let Some(last) = path.last().cloned() else { return };
                let (given, other) = match (toks.get(*i), toks.get(*i + 1)) {
                    (Some(&"as"), Some(alias)) => {
                        *i += 2;
                        (alias.to_string(), true)
                    }
                    _ => (last, false),
                };
                out.push((given, path.join("::"), other));
                return;
            }
        }
    }
}

/// `code` as the reading looks through it: `rust_joined`, and each name a `use` gives another
/// written as what it names, so `use std::fs as g; g::write(p)` is read as `std::fs::write(p)`
/// and `use std::net::UdpSocket as U; U::bind(a)` as `std::net::UdpSocket::bind(a)`; a name
/// taken as it is (`use std::fs::write;`) where it is called, `write(p)`. `deps`: the names a
/// dependency line gives a crate (`h = { package = "reqwest" }`). Before, each of these spellings
/// read as `effects: none` (FEEDBACK83.md, Q-1305, found by the coverage tests; branch
/// feedback82-85).
fn rust_spelled(code: &str, deps: &[(String, String)]) -> String {
    let joined = rust_joined(code);
    let (mut names, spans) = rust_use_names(&joined);
    names.extend(deps.iter().map(|(n, p)| (n.clone(), p.clone(), true)));
    if names.is_empty() {
        return joined;
    }
    let b = joined.as_bytes();
    let mut out = String::with_capacity(joined.len() + 64);
    let (mut i, mut last) = (0, 0);
    while i < b.len() {
        if !rust_name_byte(b[i]) || b[i].is_ascii_digit() || (i > 0 && (rust_name_byte(b[i - 1]) || matches!(b[i - 1], b'.' | b':' | b'\''))) {
            i += 1;
            continue;
        }
        let end = i + b[i..].iter().take_while(|x| rust_name_byte(**x)).count();
        let word = &joined[i..end];
        let path = b.get(end) == Some(&b':') && b.get(end + 1) == Some(&b':');
        let called = b.get(end) == Some(&b'(');
        let mac = b.get(end) == Some(&b'!') && b.get(end + 1) != Some(&b'=');
        let taken = names.iter().find(|(n, _, other)| n == word && (called || (*other && (path || mac))));
        if let Some((_, full, _)) = taken.filter(|_| !spans.iter().any(|(s, e)| (*s..*e).contains(&i)) && !joined[..i].ends_with("fn ")) {
            out.push_str(&joined[last..i]);
            out.push_str(full);
            last = end;
        }
        i = end;
    }
    out.push_str(&joined[last..]);
    out
}

/// What a reading of a Rust file finds it does: the effects, each with the calls that mean it and
/// their lines. A reading, not a proof: a crate can do what it does unseen.
pub fn rust_effects(src: &str) -> (Effects, Vec<(&'static str, String, u32)>) {
    let (code, _) = rust_code(src);
    let code = rust_spelled(&code, &[]);
    let starts_programs = code.contains("std::process") || code.contains("process::Command");
    effects_in(&code, starts_programs)
}

fn effects_in(code: &str, starts_programs: bool) -> (Effects, Vec<(&'static str, String, u32)>) {
    let mut found = Vec::new();
    let mut mask: Effects = 0;
    let tb = code.as_bytes();
    // Each pattern looked for once through the whole text, its line counted only where it is
    // found: looked for line by line, a 53 KB plugin cost `quillo check` 0.6% of the site's check
    // (found measuring review 1's fixes).
    for (pat, effect) in RUST_CALLS {
        let mut from = 0;
        while let Some(at) = code[from..].find(pat).map(|a| a + from) {
            from = at + pat.len();
            // A name of its own, not the end of another (`my_fs::read`); `Command::new` is a
            // program started only where the file takes it from `std::process` (clap's reads
            // arguments: B.4).
            let word = pat.as_bytes()[0].is_ascii_alphanumeric();
            if word && at > 0 && (tb[at - 1].is_ascii_alphanumeric() || tb[at - 1] == b'_') {
                continue;
            }
            if *pat == "Command::new" && !starts_programs {
                continue;
            }
            // A function of the file's own by that name (`fn sleep(`) is not the call; `.open(x)`
            // is a file only where the statement starts at a builder (`ReaderBuilder::new()`),
            // not hpke's `ctx.open(ct, aad)` (FEEDBACK83.md, Q-1305; branch feedback82-85).
            if word && code[..at].ends_with("fn ") {
                continue;
            }
            if *pat == ".open(" {
                let stmt = code[..at].rfind([';', '{', '}']).map_or(0, |i| i + 1);
                if tb.get(from) == Some(&b')') || !code[stmt..at].contains("Builder") {
                    continue;
                }
            }
            let ident = |c: &u8| c.is_ascii_alphanumeric() || *c == b'_' || *c == b':';
            let s = at - code[..at].bytes().rev().take_while(ident).count();
            // A pattern that ends at its `(` names the call without what it is given
            // (`redis::Client::open`, not `redis::Client::open(url`).
            let e = if pat.ends_with(['(', ')']) { from } else { from + code[from..].bytes().take_while(ident).count() };
            let call = code[s..e].trim_end_matches(['(', ':']).to_string();
            if pat.starts_with("::") && RUST_NOT_FILES.iter().any(|n| call == *n || call.strip_suffix(n).is_some_and(|p| p.ends_with("::"))) {
                continue;
            }
            mask |= effect_bit(effect);
            if !found.iter().any(|(_, c, _): &(&str, String, u32)| *c == call) {
                let line = code[..at].bytes().filter(|b| *b == b'\n').count() as u32 + 1;
                found.push((*effect, call, line));
            }
        }
    }
    found.sort_by_key(|f| f.2);
    (mask, found)
}

/// `scan_rust` where only a disagreement with the file's own effects line is wanted: a file with
/// no line is not read further (`quillo check` of a program nothing holds).
fn scan_rust_said(bytes: &[u8]) -> Block {
    let src = String::from_utf8_lossy(bytes);
    let said = crate::native::header_lines(&src, true).iter().any(|(_, l)| l.trim().starts_with("effects:"));
    if said {
        return scan_rust(bytes);
    }
    Block { rust: true, ..Default::default() }
}

fn scan_rust(bytes: &[u8]) -> Block {
    let src = String::from_utf8_lossy(bytes);
    let mut b = Block { rust: true, sha256: Some(crate::crypto::hex(&<sha2::Sha256 as sha2::Digest>::digest(bytes))), ..Default::default() };
    let parsed = crate::native::parse(&src);
    b.effects = parsed.effects.map(|m| if m == 0 { "none".to_string() } else { effect_names(m).join(", ") });
    for l in parsed.deps.lines() {
        if let Some((name, v)) = l.split_once('=') {
            let version = v.split('"').nth(1).unwrap_or("").to_string();
            let name = name.trim();
            if !name.is_empty() && !name.starts_with('#') {
                add(&mut b.crates, format!("{} {}", name, version).trim().to_string(), 0);
            }
        }
    }
    let (code, strings) = rust_code(&src);
    // A crate taken under another name, `h = { package = "reqwest" }`: read as the crate it is, by
    // its line and in the code (FEEDBACK83.md, Q-1305, found by the coverage tests; branch
    // feedback82-85).
    let crate_of = |name: &str, spec: &str| -> String {
        let package = spec.split_once("package").and_then(|(_, r)| r.trim_start().strip_prefix('=')).and_then(|r| r.split('"').nth(1));
        package.unwrap_or(name).to_string()
    };
    let renamed: Vec<(String, String)> = parsed
        .deps
        .lines()
        .filter_map(|l| l.split_once('='))
        .map(|(n, spec)| (n.trim().replace('-', "_"), crate_of(n.trim(), spec).replace('-', "_")))
        .filter(|(n, c)| n != c && !n.is_empty())
        .collect();
    let spelled = rust_spelled(&code, &renamed);
    let starts_programs = spelled.contains("std::process") || spelled.contains("process::Command");
    for (line, at, text) in &strings {
        let address = ["https://", "http://", "wss://", "ws://"].iter().find_map(|s| text.find(s).map(|i| format!("https://{}", &text[i + s.len()..])));
        if let Some(h) = address.and_then(|a| host_of(&a, true)) {
            add(&mut b.written, h, *line);
        } else if text.starts_with('/') && text.len() > 1 && !text.contains(' ') && !text.contains('{') {
            add(&mut b.written, text.clone(), *line);
        }
        // `env::var("NAME")`: the variable it reads.
        let before = code[..*at].trim_end();
        if before.ends_with("var(") || before.ends_with("var_os(") {
            add(&mut b.env, text.clone(), *line);
        }
    }
    let (mut mask, mut calls) = effects_in(&spelled, starts_programs);
    let code = spelled;
    for c in b.crates.keys() {
        let name = c.split(' ').next().unwrap_or("");
        let package = parsed.deps.lines().filter_map(|l| l.split_once('=')).find(|(n, _)| n.trim() == name).map(|(n, spec)| crate_of(n.trim(), spec));
        let name = package.as_deref().unwrap_or(name);
        for (_, effect) in RUST_CRATES.iter().filter(|(k, _)| *k == name) {
            mask |= effect_bit(effect);
            calls.push((*effect, format!("the crate {}", name), 0));
        }
    }
    // A crate's feature or call that does it (FEEDBACK83.md, Q-1305; branch feedback82-85).
    for l in parsed.deps.lines() {
        let Some((name, spec)) = l.split_once('=') else { continue };
        let name = crate_of(name.trim(), spec);
        let name = name.as_str();
        for (_, uses, effect) in RUST_CRATE_USES.iter().filter(|(k, _, _)| *k == name) {
            let by = uses.iter().find(|u| match u.strip_prefix("feature:") {
                Some(f) => spec.contains(&format!("\"{}\"", f)),
                // `hash(`, not `legacy_django_hash(`.
                None => code.match_indices(*u).any(|(at, _)| !u.as_bytes()[0].is_ascii_alphanumeric() || at == 0 || !(code.as_bytes()[at - 1].is_ascii_alphanumeric() || code.as_bytes()[at - 1] == b'_')),
            });
            if let Some(u) = by {
                mask |= effect_bit(effect);
                calls.push((*effect, format!("the crate {} ({})", name, u.trim_end_matches('(')), 0));
            }
        }
    }
    b.found_effects = mask;
    // Its line leaves out what it plainly calls (FEEDBACK83.md, Q-1305; branch feedback82-85).
    if let Some(said) = parsed.effects {
        let left: Vec<&(&str, String, u32)> = calls.iter().filter(|(e, _, _)| said & effect_bit(e) == 0).collect();
        if let Some(first) = left.iter().find(|c| c.2 > 0).or(left.first()) {
            let shown: Vec<String> = left.iter().map(|(_, c, l)| if *l > 0 { format!("{} (line {})", c, l) } else { c.clone() }).collect();
            b.notes.push((first.2, format!("says `effects: {}`, and calls {}: a reading of its text, not a proof (a crate can do what it does unseen)", b.effects.clone().unwrap_or_default(), shown.join(", "))));
        }
    }
    b
}

// ---- quillo.confirmed --------------------------------------------------------------------------

/// What `quillo.confirmed` says, by file.
pub type Confirmed = BTreeMap<String, Block>;

fn unquote(s: &str) -> Option<(String, &str)> {
    let s = s.trim_start();
    let rest = s.strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = rest.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return Some((out, &rest[i + 1..])),
            '\\' => match chars.next()?.1 {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                // `\u{1b}`, as `quote` writes a control character (`unseen`).
                'u' => match rest[i + 2..].strip_prefix('{').and_then(|r| r.split_once('}')).and_then(|(h, _)| u32::from_str_radix(h, 16).ok().and_then(char::from_u32).map(|c| (c, h.len()))) {
                    Some((c, n)) => {
                        out.push(c);
                        for _ in 0..n + 2 {
                            chars.next();
                        }
                    }
                    None => out.push('u'),
                },
                c => out.push(c),
            },
            c => out.push(c),
        }
    }
    None
}

/// Whether a terminal does something with `c` other than show it: a control character (C0, DEL,
/// C1: `\u{1b}[1A` moves up a line, `\u{9b}` is a CSI too), a line or paragraph separator, or one
/// that turns the direction of the text around it.
fn unseen(c: char) -> bool {
    c.is_control() || matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{2028}' | '\u{2029}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

/// The program's text `s` as it is shown at a terminal: each character a terminal acts on
/// (`unseen`) written as a Quillo text writes it (`\n`, `\r`, `\t`, else `\u{1b}`), the rest as it
/// is. The listing a developer reads before answering `y`, and the hold's messages, printed a
/// path's `\u{1b}[1A\u{1b}[2K` as it was: a taken file could erase the line that said what else
/// it reaches (FEEDBACK83.md, Q-1300 D.2, found by the coverage tests; branch feedback82-85).
pub fn escaped(s: &str) -> std::borrow::Cow<'_, str> {
    if !s.chars().any(unseen) {
        return std::borrow::Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if unseen(c) => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
            c => out.push(c),
        }
    }
    std::borrow::Cow::Owned(out)
}

/// `line` up to a `#` that is not in a text.
fn uncommented(line: &str) -> &str {
    let mut in_text = false;
    let mut escaped = false;
    for (i, c) in line.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' if in_text => escaped = true,
            '"' => in_text = !in_text,
            '#' if !in_text => return &line[..i],
            _ => {}
        }
    }
    line
}

fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            // Any other a terminal acts on, so the file shown (`cat`, a diff) is what it says
            // (FEEDBACK83.md, Q-1300 D.2, found by the coverage tests; branch feedback82-85).
            c if unseen(c) => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Reads `quillo.confirmed`'s text; why not, with the line.
pub fn parse(text: &str) -> Result<Confirmed, String> {
    let mut out = Confirmed::new();
    let mut at: Option<String> = None;
    let mut lines = text.lines().enumerate();
    while let Some((n, raw)) = lines.next() {
        // A `#` outside a text ends the line, after a list's `]` too, where the list was read as
        // going on and took the next block in (found in review).
        let line = uncommented(raw).trim();
        let err = |why: &str| format!("line {}: {}", n + 1, why);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(inner) = line.strip_prefix('[') {
            let inner = inner.strip_suffix(']').ok_or_else(|| err("a file's block is `[\"path\"]`"))?;
            let name = match unquote(inner) {
                Some((name, rest)) if rest.trim().is_empty() => name,
                _ => inner.trim().to_string(),
            };
            if name.is_empty() {
                return Err(err("a file's block names its file"));
            }
            out.entry(name.clone()).or_default();
            at = Some(name);
            continue;
        }
        let (key, value) = line.split_once('=').ok_or_else(|| err("each line is `key = value`"))?;
        let key = key.trim();
        let Some(file) = at.clone() else { return Err(err("a key before any file's block")) };
        let mut value = value.trim().to_string();
        // A list over several lines.
        if value.starts_with('[') {
            while !value.trim_end().ends_with(']') {
                let Some((_, more)) = lines.next() else { return Err(err("a list that does not end")) };
                value.push(' ');
                value.push_str(uncommented(more).trim());
            }
        }
        let b = out.entry(file).or_default();
        match key {
            "sha256" | "effects" => {
                let (v, rest) = unquote(&value).ok_or_else(|| err(&format!("`{}` is a text: \"...\"", key)))?;
                if !rest.trim().is_empty() && !rest.trim().starts_with('#') {
                    return Err(err("one text after `=`"));
                }
                b.rust = true;
                if key == "sha256" {
                    if v.len() != 64 || !v.bytes().all(|c| c.is_ascii_hexdigit()) {
                        return Err(err("a SHA-256 is 64 hexadecimal digits"));
                    }
                    b.sha256 = Some(v.to_ascii_lowercase());
                } else {
                    b.effects = Some(v);
                }
            }
            k => {
                let mut hashes: Vec<(String, String)> = Vec::new();
                let Some(items) = b.items_mut(k) else {
                    return Err(err(&format!("`{}` is not something quillo.confirmed says (it says http, files, writes, listens, scripts, loads, calls, routes, sha256, effects, env, crates)", k)));
                };
                let mut rest = value.strip_prefix('[').ok_or_else(|| err(&format!("`{}` is a list: [\"...\", ...]", k)))?.trim_start();
                loop {
                    rest = rest.trim_start().trim_start_matches(',').trim_start();
                    if let Some(after) = rest.strip_prefix(']') {
                        if !after.trim().is_empty() && !after.trim().starts_with('#') {
                            return Err(err("nothing after a list but a comment"));
                        }
                        break;
                    }
                    let (item, after) = unquote(rest).ok_or_else(|| err(&format!("`{}` is a list of texts", k)))?;
                    rest = after;
                    let (item, hash) = match item.split_once(' ') {
                        Some((u, h)) if k == "scripts" && h.starts_with("sha") => (u.to_string(), Some(h.trim().to_string())),
                        _ => (item, None),
                    };
                    let item = if matches!(k, "files" | "writes") { normal(&item) } else if matches!(k, "http" | "loads" | "calls") { item.to_ascii_lowercase() } else { item };
                    items.entry(item.clone()).or_default();
                    if let Some(h) = hash {
                        hashes.push((item, h));
                    }
                }
                b.integrity.extend(hashes);
            }
        }
    }
    Ok(out)
}

/// `quillo.confirmed`'s text for `c`.
pub fn render(c: &Confirmed) -> String {
    let mut s = String::from("# quillo.confirmed: what this program's files reach, as confirmed.\n# Written by `quillo confirm`. Commit it: the answer is the project's.\n");
    for (file, b) in c {
        if b.is_empty() {
            continue;
        }
        s.push('\n');
        s.push_str(&format!("[{}]\n", quote(file)));
        if b.rust {
            if let Some(h) = &b.sha256 {
                s.push_str(&format!("sha256 = {}\n", quote(h)));
            }
            if let Some(e) = &b.effects {
                s.push_str(&format!("effects = {}\n", quote(e)));
            }
        }
        for k in ["http", "files", "writes", "listens", "scripts", "loads", "calls", "routes", "env", "crates"] {
            let items = b.items(k);
            if items.is_empty() {
                continue;
            }
            let list: Vec<String> = items
                .keys()
                .map(|i| match b.integrity.get(i).filter(|_| k == "scripts") {
                    Some(h) => quote(&format!("{} {}", i, h)),
                    None => quote(i),
                })
                .collect();
            s.push_str(&format!("{} = [{}]\n", k, list.join(", ")));
        }
    }
    s
}

/// Reads the file at `path`: None when there is none.
pub fn read(path: &Path) -> Result<Option<Confirmed>, String> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {}", path.display(), e)),
        Ok(_) => {}
    }
    let text = crate::error::read_text(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    parse(&text).map(Some).map_err(|e| format!("{}: {}", path.display(), e))
}

// ---- what is not confirmed ---------------------------------------------------------------------

/// One thing a file reaches that `quillo.confirmed` does not say it may.
#[derive(Debug, Clone)]
pub struct Missing {
    pub file: String,
    /// The key it has in the file (`http`, ...), or `sha256` for a Rust file.
    pub kind: &'static str,
    pub item: String,
    pub lines: Vec<u32>,
}

fn has(items: &Items, kind: &str, item: &str) -> bool {
    match kind {
        "http" | "loads" | "calls" => items.keys().any(|h| host_matches(h, item)),
        "files" | "writes" => items.keys().any(|p| path_within(p, item)),
        _ => items.contains_key(item),
    }
}

/// What `scan` finds that `conf` does not confirm.
pub fn missing(scan: &Scan, conf: &Confirmed) -> Vec<Missing> {
    let mut out = Vec::new();
    let empty = Block::default();
    for (file, found) in &scan.files {
        let c = conf.get(file).unwrap_or(&empty);
        if found.rust {
            if c.sha256 != found.sha256 {
                out.push(Missing { file: file.clone(), kind: "sha256", item: found.sha256.clone().unwrap_or_default(), lines: Vec::new() });
            }
            continue;
        }
        for (k, _) in KINDS {
            for (item, lines) in found.items(k) {
                // A path confirmed under `writes` is read too, as the hold reads it at run time
                // (`path_in`): `quillo check` and the start's count asked for a `files` entry too
                // (FEEDBACK83.md, Q-1300, found by the coverage tests; branch feedback82-85).
                if !has(c.items(k), k, item) && !(*k == "files" && has(&c.writes, "writes", item)) {
                    out.push(Missing { file: file.clone(), kind: k, item: item.clone(), lines: lines.clone() });
                }
            }
        }
    }
    out
}

/// How many confirmed items the files `chosen` takes no longer have: they leave the list at the
/// next `quillo confirm` (D.3). A block of a file no longer taken leaves whole, each of its items
/// counted (its patterns too, and a Rust file's SHA-256 as one), where it was said as 1 item
/// however many it had; a file not chosen keeps its block, and is not counted (FEEDBACK83.md,
/// Q-1300 D.3, found by the coverage tests; branch feedback82-85).
pub fn stale(scan: &Scan, conf: &Confirmed, chosen: &dyn Fn(&str) -> bool) -> usize {
    let mut n = 0;
    for (file, c) in conf.iter().filter(|(f, _)| chosen(f)) {
        let Some(found) = scan.files.get(file) else {
            n += KINDS.iter().map(|(k, _)| c.items(k).len()).sum::<usize>() + usize::from(c.rust);
            continue;
        };
        for (k, _) in KINDS {
            n += c.items(k).keys().filter(|i| !i.contains('*') && !found.items(k).contains_key(*i)).count();
        }
    }
    n
}

/// `quillo.confirmed` with the files `chosen` takes confirmed as they are now: each one's list is
/// what it reaches today (an item no longer in it leaves), a confirmed script keeps its integrity
/// hash; the other files' blocks stay as they were.
pub fn merged(scan: &Scan, conf: &Confirmed, chosen: &dyn Fn(&str) -> bool) -> Confirmed {
    let mut out = conf.clone();
    for (file, found) in &scan.files {
        if !chosen(file) {
            continue;
        }
        let mut b = found.clone();
        b.notes.clear();
        b.written.clear();
        for k in ["http", "files", "writes", "listens", "scripts", "loads", "calls", "routes", "env", "crates"] {
            if let Some(items) = b.items_mut(k) {
                items.values_mut().for_each(Vec::clear);
            }
        }
        if let Some(old) = conf.get(file) {
            for (u, h) in &old.integrity {
                if b.scripts.contains_key(u) {
                    b.integrity.insert(u.clone(), h.clone());
                }
            }
            // A pattern written by hand (`*.stripe.com`) stays, and what it covers is not written
            // beside it: only a written item the files no longer have leaves (FEEDBACK83.md,
            // Q-1300, found in review; branch feedback82-85).
            for (k, _) in KINDS {
                let kept: Vec<String> = old.items(k).keys().filter(|i| i.contains('*')).cloned().collect();
                if kept.is_empty() {
                    continue;
                }
                if let Some(items) = b.items_mut(k) {
                    let pats: Items = kept.iter().map(|p| (p.clone(), Vec::new())).collect();
                    items.retain(|i, _| !has(&pats, k, i));
                    items.extend(pats);
                }
            }
        }
        out.insert(file.clone(), b);
    }
    // A file the program no longer takes leaves, where it was chosen.
    out.retain(|f, _| scan.files.contains_key(f) || !chosen(f));
    out
}

/// What is not confirmed, by file, as `quillo confirm` and a start list it (D.2); the program's
/// texts in it `escaped`, as a terminal shows them (FEEDBACK83.md, Q-1300 D.2, found by the
/// coverage tests; branch feedback82-85).
pub fn listing(scan: &Scan, missing: &[Missing]) -> String {
    let mut s = String::new();
    let mut files: Vec<&str> = missing.iter().map(|m| m.file.as_str()).collect();
    files.dedup();
    for f in files {
        let found = scan.files.get(f);
        let rust = found.is_some_and(|b| b.rust);
        s.push_str(&format!("\n  {}{}\n", escaped(f), if rust { "  (Rust: Quillo cannot hold it to a list, only to this file)" } else { "" }));
        for m in missing.iter().filter(|m| m.file == f) {
            if m.kind == "sha256" {
                s.push_str(&format!("    {:<13} sha256 {}\n", "file", m.item));
                continue;
            }
            let word = KINDS.iter().find(|(k, _)| *k == m.kind).map_or(m.kind, |(_, w)| w);
            let lines = match m.lines.len() {
                0 => String::new(),
                1 => format!("line {}", m.lines[0]),
                _ => format!("lines {}", m.lines.iter().map(u32::to_string).collect::<Vec<_>>().join(", ")),
            };
            s.push_str(&format!("    {:<13} {:<40} {}\n", word, escaped(&m.item), lines));
        }
        if let Some(b) = found.filter(|b| b.rust) {
            s.push_str(&format!("    {:<13} effects: {}\n", "says", b.effects.as_deref().unwrap_or("nothing (every effect)")));
            let reads = effect_names(b.found_effects);
            if !reads.is_empty() {
                s.push_str(&format!("    {:<13} {}\n", "reads as", reads.join(", ")));
            }
            if !b.written.is_empty() {
                s.push_str(&format!("    {:<13} {}\n", "written", escaped(&b.written.keys().cloned().collect::<Vec<_>>().join(", "))));
            }
            for (_, n) in &b.notes {
                s.push_str(&format!("    {:<13} {}\n", "but", escaped(n.trim_start_matches("says "))));
            }
        }
    }
    s
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{} {}", n, if n == 1 { one } else { many })
}

/// One line for what is not confirmed: `3 things in 2 files are not confirmed`.
fn summary(missing: &[Missing]) -> String {
    let mut files: Vec<&str> = missing.iter().map(|m| m.file.as_str()).collect();
    files.sort();
    files.dedup();
    format!("{} in {} {} not confirmed", plural(missing.len(), "thing", "things"), plural(files.len(), "file", "files"), if missing.len() == 1 { "is" } else { "are" })
}

// ---- holding a run to it -----------------------------------------------------------------------

/// What a run is held to: each file's confirmed list, by its name and by the names the running
/// program has for it (`cur_file`).
struct Hold {
    dir: PathBuf,
    /// The main file's name, for code that names no file.
    main: String,
    /// The blocks whose routes are held: of files taken by `use` (Q-1301).
    routes_held: HashSet<u32>,
    /// The ways in of taken files that the program took by name: its own (`Block::named`).
    named: HashSet<(u32, String)>,
    /// Block 0 is an empty one: a file with no block is confirmed for nothing.
    blocks: Vec<Block>,
    by_name: HashMap<String, u32>,
    seen: RwLock<HashMap<String, u32>>,
    said: Mutex<HashSet<String>>,
    /// A confirmed script's integrity hash, by the block that confirmed it and its address: two
    /// files may confirm one address with two hashes (found in review).
    integrity: HashMap<(u32, String), String>,
    /// The tools (`fn mcp`) of taken files that were not confirmed: not offered (Q-1301); and
    /// every tool there was at the start, a new one being of a file not read then.
    refused_tools: HashSet<String>,
    known_tools: HashSet<String>,
    /// Each confirmed path where it really is, by the folder it was read from (`real_item`).
    reals: RwLock<HashMap<(PathBuf, String), Option<PathBuf>>>,
    page_policy: Option<String>,
}

static HOLD: OnceLock<Hold> = OnceLock::new();

/// Holds the run to `conf`, from here on. Once.
fn hold(dir: &Path, main: &Path, conf: &Confirmed, found: &Scan) {
    let dir = dir.to_path_buf();
    let mut blocks = vec![Block::default()];
    let mut by_name = HashMap::new();
    let mut integrity = HashMap::new();
    for (f, b) in conf {
        let i = blocks.len() as u32;
        by_name.insert(f.clone(), i);
        blocks.push(b.clone());
        integrity.extend(b.integrity.iter().map(|(u, h)| ((i, u.clone()), h.clone())));
    }
    // Each file of the program its own block, confirmed for nothing where it has none.
    for f in found.files.keys() {
        if !by_name.contains_key(f) {
            by_name.insert(f.clone(), blocks.len() as u32);
            blocks.push(Block::default());
        }
    }
    let routes_held: HashSet<u32> = found.files.iter().filter(|(_, b)| b.taken).filter_map(|(f, _)| by_name.get(f).copied()).collect();
    let named: HashSet<(u32, String)> = found.files.iter().filter(|(_, b)| b.taken).filter_map(|(f, b)| by_name.get(f).map(|i| (*i, b))).flat_map(|(i, b)| b.named.iter().map(move |n| (i, n.clone()))).collect();
    let mut refused_tools = HashSet::new();
    for (f, b) in found.files.iter().filter(|(_, b)| b.taken) {
        let confirmed = conf.get(f).map(|c| &c.routes);
        for r in b.routes.keys().filter(|r| r.starts_with("fn mcp ")) {
            if !confirmed.is_some_and(|c| c.contains_key(r)) {
                refused_tools.insert(r["fn mcp ".len()..].to_string());
            }
        }
    }
    let known_tools: HashSet<String> = found.files.values().flat_map(|b| b.tools.iter().cloned()).collect();
    // The page's policy from the files the program has now, not from blocks a file it no longer
    // takes left behind (found in review 2).
    let current: Confirmed = conf.iter().filter(|(f, _)| found.files.contains_key(*f)).map(|(f, b)| (f.clone(), b.clone())).collect();
    let page_policy = Some(page_policy_of(&current));
    let main = rel_name(&dir, main);
    let _ = HOLD.set(Hold { dir: dir.canonicalize().unwrap_or(dir), main, routes_held, named, blocks, by_name, seen: RwLock::new(HashMap::new()), said: Mutex::new(HashSet::new()), integrity, refused_tools, known_tools, reals: RwLock::new(HashMap::new()), page_policy });
}

/// The `content-security-policy-report-only` of a held program's pages: what its files' scripts,
/// loads and calls were confirmed for (FEEDBACK83.md, Q-1303, D.4; branch feedback82-85).
/// Report-only, as the report proposes, until a developer can turn it on: a page with inline
/// scripts of its own is not broken by it. Quillo's own inline scripts are let through.
fn page_policy_of(conf: &Confirmed) -> String {
    let mut scripts: Vec<String> = Vec::new();
    let mut loads: Vec<String> = Vec::new();
    let mut calls: Vec<String> = Vec::new();
    let push = |v: &mut Vec<String>, h: String| {
        if !v.contains(&h) {
            v.push(h);
        }
    };
    for b in conf.values() {
        for u in b.scripts.keys() {
            if let Some(h) = origin_host(u, true) {
                push(&mut scripts, h.clone());
                // A library's stylesheet and fonts come from its host too.
                push(&mut loads, h);
            }
        }
        for h in b.loads.keys() {
            push(&mut loads, h.clone());
        }
        for h in b.calls.keys() {
            push(&mut calls, h.clone());
        }
    }
    let with = |base: &str, hosts: &[String]| if hosts.is_empty() { base.to_string() } else { format!("{} {}", base, hosts.join(" ")) };
    [
        "default-src 'self'".to_string(),
        with("script-src 'self' 'unsafe-inline'", &scripts),
        with("style-src 'self' 'unsafe-inline'", &loads),
        with("img-src 'self' data: blob:", &loads),
        with("media-src 'self' blob:", &loads),
        with("font-src 'self' data:", &loads),
        with("frame-src 'self'", &loads),
        with("connect-src 'self'", &calls),
    ]
    .join("; ")
}

/// The header a held program's pages carry, when the program sets no policy of its own.
pub fn page_policy() -> Option<&'static str> {
    HOLD.get().and_then(|h| h.page_policy.as_deref())
}

/// Whether a run is held at all.
#[inline]
pub fn held() -> bool {
    HOLD.get().is_some()
}

/// The block the code of `cur_file` is held to (0: it is not held, the program has no
/// `quillo.confirmed`); a file whose block is not there is held to block 1, an empty one.
#[inline]
pub fn held_id(cur_file: Option<&str>) -> u32 {
    match HOLD.get() {
        None => 0,
        Some(h) => h.id(cur_file) + 1,
    }
}

impl Hold {
    fn id(&self, cur_file: Option<&str>) -> u32 {
        let Some(name) = cur_file else { return self.by_name.get(&self.main).copied().unwrap_or(0) };
        if let Some(i) = self.seen.read().unwrap_or_else(std::sync::PoisonError::into_inner).get(name) {
            return *i;
        }
        let path = Path::new(name);
        let full = if path.is_absolute() { path.to_path_buf() } else { std::env::current_dir().map(|d| d.join(path)).unwrap_or_else(|_| path.to_path_buf()) };
        let rel = rel_name(&self.dir, &full);
        let i = self.by_name.get(&rel).copied().unwrap_or(0);
        self.seen.write().unwrap_or_else(std::sync::PoisonError::into_inner).insert(name.to_string(), i);
        i
    }

    fn name_of(&self, i: u32) -> &str {
        self.by_name.iter().find(|(_, v)| **v == i).map_or("this file", |(k, _)| k.as_str())
    }

    /// Says `what` once in the log, the program's texts in it `escaped` (FEEDBACK83.md, Q-1300,
    /// found by the coverage tests; branch feedback82-85).
    fn say(&self, what: String) {
        if self.said.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(what.clone()) {
            eprintln!("quillo: {}", escaped(&what));
        }
    }
}

/// Why the code held as `id` (`held_id`) may not reach `url`, if it may not: its host is not
/// among those its file was confirmed for. Each address of a call, its redirects too.
pub fn http_ok(id: u32, url: &str) -> Result<(), String> {
    let Some(h) = HOLD.get().filter(|_| id > 0) else { return Ok(()) };
    let i = id - 1;
    let host = host_of(url, true).unwrap_or_default();
    if h.blocks[i as usize].http.keys().any(|c| host_matches(c, &host)) {
        return Ok(());
    }
    let file = if i == 0 { "this file".to_string() } else { h.name_of(i).to_string() };
    h.say(format!("{}: a call to {} was refused: it is not confirmed for this file (`quillo confirm` lists it)", file, host));
    Err(format!("{} is not confirmed for {} (quillo confirm)", host, file))
}

/// Why the code of `cur_file` may not reach `host` (a UDP packet's, a database server's), if it
/// may not: as `http_ok` says it of an address (FEEDBACK83.md, Q-1300, found in review; branch
/// feedback82-85).
pub fn host_ok(cur_file: Option<&str>, host: &str) -> Result<(), String> {
    let Some(h) = HOLD.get() else { return Ok(()) };
    host_in(h, h.id(cur_file), host)
}

fn host_in(h: &Hold, i: u32, host: &str) -> Result<(), String> {
    let host = host.trim_start_matches('[').trim_end_matches(']').to_ascii_lowercase();
    if h.blocks[i as usize].http.keys().any(|c| host_matches(c, &host)) {
        return Ok(());
    }
    let file = if i == 0 { "this file".to_string() } else { h.name_of(i).to_string() };
    h.say(format!("{}: {} was refused: it is not confirmed for this file (`quillo confirm` lists it)", file, host));
    Err(format!("{} is not confirmed for {} (quillo confirm)", host, file))
}

/// The hosts a database server's address `url` has its driver connect to, as the driver itself
/// reads it (`db::server_hosts`): read by hand, `postgres://db.example/app@127.0.0.1` was listed as
/// db.example and connected to 127.0.0.1 (found in review 3). A Unix socket is refused where held.
pub fn db_hosts(url: &str) -> Result<Vec<String>, String> {
    crate::db::server_hosts(url).unwrap_or_else(|| Err(format!("{} is no server's address", url)))
}

/// Why a database may not be opened at `target` by the code held as `id` (`held_id`), if it may
/// not: each host its driver would connect to, or the file it writes, as that code's file was
/// confirmed. Asked where each connection is made, a tenant's and a replica's too, with the file
/// that declared the database (FEEDBACK83.md, Q-1300, found in review 2; branch feedback82-85).
pub fn db_ok(id: u32, base: &Path, target: &str) -> Result<(), String> {
    let Some(h) = HOLD.get().filter(|_| id > 0) else { return Ok(()) };
    let i = id - 1;
    if let Some(hosts) = crate::db::server_hosts(target) {
        for host in hosts? {
            host_in(h, i, &host)?;
        }
        return Ok(());
    }
    if target == ":memory:" {
        return Ok(());
    }
    path_in(h, i, None, base, target, true)
}

/// A packet's mark, where the run is held: what a UDP packet the program was handed says it came
/// from, signed with a key of this process's, so a reply goes back to its sender and to no address
/// the program wrote into a map of its own (`udp.reply`; found in review 2). None when nothing is
/// held: the packet is as it was.
pub fn udp_mark(port: u16, from: &std::net::SocketAddr) -> Option<String> {
    HOLD.get()?;
    Some(crate::crypto::hex(&crate::crypto::hmac_sha256(udp_key(), format!("{} {}", port, from).as_bytes())))
}

/// Whether `mark` is the one a packet to `port` from `from` was handed with.
pub fn udp_marked(port: u16, from: &std::net::SocketAddr, mark: &str) -> bool {
    udp_mark(port, from).is_some_and(|m| crate::crypto::same(m.as_bytes(), mark.as_bytes()))
}

fn udp_key() -> &'static [u8] {
    static KEY: OnceLock<Vec<u8>> = OnceLock::new();
    KEY.get_or_init(|| crate::crypto::secret_bytes(32))
}

/// Why the code of `cur_file` may not listen on UDP `port` at `host`, if it may not: an address
/// other than this machine's own loopback, which others reach, not confirmed as `udp PORT`.
pub fn listen_ok(cur_file: Option<&str>, host: &str, port: u16) -> Result<(), String> {
    let Some(h) = HOLD.get() else { return Ok(()) };
    if host.trim_start_matches('[').trim_end_matches(']').parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback()) || host == "localhost" {
        return Ok(());
    }
    let i = h.id(cur_file);
    let item = format!("udp {}", port);
    if h.blocks[i as usize].listens.contains_key(&item) {
        return Ok(());
    }
    let file = if i == 0 { "this file".to_string() } else { h.name_of(i).to_string() };
    h.say(format!("{}: listening on {} was refused: it is not confirmed for this file (`quillo confirm` lists it)", file, item));
    Err(format!("listening on {} is not confirmed for {} (quillo confirm)", item, file))
}

/// Where `p` (under `base`) really is, as the system goes through it: the longest start of it
/// that is there, resolved by the system (its links followed, and each `..` in it taken from
/// where the link before it leads), then the names after it, a `..` among them taking away the
/// name before it, which is not there (as `create_dir_all` makes it, a plain folder). None where
/// a part of it is a link that leads nowhere: what is written through it is made wherever that
/// is. Before, `..` was taken away from the text first: `data/media/../y.bin`, with `data/media`
/// a link to `../../shared/media`, was judged as `data/y.bin` and written in `shared/`
/// (FEEDBACK83.md, Q-1300, found by the coverage tests; branch feedback82-85). A path that is
/// there costs one `canonicalize`, as before.
fn real_under(base: &Path, p: &str) -> Option<PathBuf> {
    let full = if Path::new(p).is_absolute() { PathBuf::from(p) } else { base.join(p) };
    let mut at = full.as_path();
    let mut after: Vec<Component> = Vec::new();
    loop {
        if let Ok(mut r) = at.canonicalize() {
            for c in after.iter().rev() {
                match c {
                    Component::ParentDir => {
                        r.pop();
                    }
                    c => r.push(c.as_os_str()),
                }
            }
            return Some(r);
        }
        if std::fs::symlink_metadata(at).is_ok_and(|m| m.file_type().is_symlink()) {
            return None;
        }
        match at.components().next_back()? {
            c @ (Component::Normal(_) | Component::ParentDir) => after.push(c),
            Component::CurDir => {}
            _ => return None,
        }
        at = at.parent()?;
    }
}

/// Why the code of `cur_file` may not use the path `p` (as the program wrote it, under `base`),
/// if it may not: it is not among, nor in a folder among, those its file was confirmed for; to
/// `write` (or delete), among those confirmed to be written. Both are judged where they really
/// are, links followed: `uploads/x` leading to `.env` is `.env` (found in review). One look at
/// `HOLD` where nothing is held.
pub fn path_ok(cur_file: Option<&str>, base: &Path, p: &str, write: bool) -> Result<(), String> {
    let Some(h) = HOLD.get() else { return Ok(()) };
    path_in(h, h.id(cur_file), cur_file, base, p, write)
}

/// A confirmed path where it really is, worked out once for each folder it is read from: a
/// request held to it asks the system only for its own path (found measuring review 2).
fn real_item(h: &Hold, base: &Path, c: &str) -> Option<PathBuf> {
    let key = (base.to_path_buf(), c.to_string());
    if let Some(r) = h.reals.read().unwrap_or_else(std::sync::PoisonError::into_inner).get(&key) {
        return r.clone();
    }
    let r = real_under(base, c);
    h.reals.write().unwrap_or_else(std::sync::PoisonError::into_inner).insert(key, r.clone());
    r
}

fn path_in(h: &Hold, i: u32, cur_file: Option<&str>, base: &Path, p: &str, write: bool) -> Result<(), String> {
    let b = &h.blocks[i as usize];
    // A path through a link that leads nowhere is covered by nothing (`real_under`).
    let real = real_under(base, p);
    let covers = |items: &Items| {
        let Some(real) = &real else { return false };
        items.keys().any(|c| real_item(h, base, c).is_some_and(|cr| *real == cr || (c.ends_with('/') && real.starts_with(&cr))))
    };
    if covers(&b.writes) || (!write && covers(&b.files)) {
        return Ok(());
    }
    let shown = match &real {
        Some(real) => real.strip_prefix(&h.dir).map_or_else(|_| real.display().to_string(), slashed),
        None => p.to_string(),
    };
    let file = if i == 0 { cur_file.map_or_else(|| h.main.clone(), |f| rel_name(&h.dir, Path::new(f))) } else { h.name_of(i).to_string() };
    let how = if write && covers(&b.files) { "confirmed to be read, not written" } else { "not confirmed" };
    h.say(format!("{}: the path {} was refused: it is {} for this file (`quillo confirm` lists it)", file, shown, how));
    Err(format!("{} is {} for {} (quillo confirm)", shown, how, file))
}

/// Whether a script the code of `cur_file` takes is left out of the page, and its integrity hash
/// where it was confirmed with one (Q-1304).
pub fn script(cur_file: Option<&str>, src: &str) -> (bool, Option<String>) {
    let Some(h) = HOLD.get() else { return (false, None) };
    let i = h.id(cur_file);
    if !h.blocks[i as usize].scripts.contains_key(src) {
        h.say(format!("{}: the script {} is left out of the pages: it is not confirmed for this file (`quillo confirm` lists it)", if i == 0 { "a file" } else { h.name_of(i) }, src));
        return (true, None);
    }
    (false, h.integrity.get(&(i, src.to_string())).cloned())
}

/// Whether the tool `name` (`fn mcp`) is offered: not one of a taken file's that was not
/// confirmed (Q-1301, found in review).
pub fn tool_ok(name: &str) -> bool {
    let Some(h) = HOLD.get() else { return true };
    if !h.known_tools.contains(name) {
        h.say(format!("the tool {} is not offered: its file was not read as the program started (restart it once it is confirmed)", name));
        return false;
    }
    !h.refused_tools.contains(name)
}

/// Whether the Rust file `qwer` is held to a confirmed SHA-256: then only a library built here
/// from it loads, never one a cloned `.quillo/native` brought (found in review); with building
/// off, only one this machine's build record knows.
pub fn pinned(qwer: &Path) -> bool {
    HOLD.get().is_some_and(|h| h.by_name.get(&rel_name(&h.dir, qwer)).is_some_and(|i| h.blocks[*i as usize].sha256.is_some()))
}

/// Whether the route `shown` (`GET /x`) that the code of `cur_file` declares is served: a file
/// taken for some of its names serves only the routes confirmed for it (FEEDBACK83.md, Q-1301; branch
/// feedback82-85). The main file's, and a mounted program's, are the program's own.
pub fn route_ok(cur_file: Option<&str>, shown: &str) -> bool {
    let Some(h) = HOLD.get() else { return true };
    let i = h.id(cur_file);
    // A file no start read (one `--watch` took since): none of its ways in until a restart with
    // it confirmed (found in review 2).
    if i == 0 {
        h.say(format!("{}: {} is not served: its file was not read as the program started (restart it once it is confirmed)", cur_file.map_or_else(|| "a file".to_string(), |f| rel_name(&h.dir, Path::new(f))), shown));
        return false;
    }
    if !h.routes_held.contains(&i) || h.blocks[i as usize].routes.contains_key(shown) || h.named.contains(&(i, shown.to_string())) {
        return true;
    }
    let what = if shown.starts_with("server fn ") || shown.starts_with("fn mcp ") { format!("{} is not offered", shown) } else { format!("the route {} is not served", shown) };
    h.say(format!("{}: {}: it is not confirmed for this file (`quillo confirm` lists it)", if i == 0 { "a taken file" } else { h.name_of(i) }, what));
    false
}

/// Why the Rust file `qwer` may not load, if it may not: it is not the file confirmed, by its
/// SHA-256 (D.3: a Rust file is asked about again whenever it changes).
pub fn plugin_ok(qwer: &Path) -> Result<(), String> {
    let Some(h) = HOLD.get() else { return Ok(()) };
    let rel = rel_name(&h.dir, qwer);
    let bytes = crate::error::read_file(qwer).map_err(|e| format!("{}: {}", qwer.display(), e))?;
    let have = crate::crypto::hex(&<sha2::Sha256 as sha2::Digest>::digest(&bytes));
    let want = h.by_name.get(&rel).and_then(|i| h.blocks[*i as usize].sha256.as_deref());
    let shown = escaped(&rel);
    match want {
        Some(w) if w == have => Ok(()),
        Some(_) => Err(format!("{} is not the file confirmed: its SHA-256 is {} now; Rust cannot be held to a list, so it does not load (`quillo confirm` shows it)", shown, have)),
        None => Err(format!("{} is not confirmed (its SHA-256 is {}); Rust cannot be held to a list, so it does not load (`quillo confirm` asks about it)", shown, have)),
    }
}

/// Whether `pattern` (a confirmed host, `*.example.com` for its subdomains) covers `host`.
fn host_matches(pattern: &str, host: &str) -> bool {
    let (pattern, host) = (pattern.to_ascii_lowercase(), host.to_ascii_lowercase());
    match pattern.strip_prefix("*.") {
        Some(d) => host.ends_with(&format!(".{}", d)),
        None => pattern == "*" || pattern == host,
    }
}

/// Whether the confirmed path `c` covers `p`: the same, or `p` inside the folder `c` (`c` ends
/// with `/`).
fn path_within(c: &str, p: &str) -> bool {
    let p = normal(p);
    c == p || c.trim_end_matches('/') == p.trim_end_matches('/') || (c.ends_with('/') && p.starts_with(c))
}

// ---- the provider's policy ---------------------------------------------------------------------

/// The first layer of the policy in force that is a provider's: it asks for a complete
/// `quillo.confirmed` (the user's decision, question 5). Only the host's layers ask; a `--policy`
/// file no longer does, as the program's own `quillo.policy` never did, the rule library pins
/// follow (`native::library_pin_met`; FEEDBACK83.md, question 5 as the user revised it on
/// 2026-11-23; branch feedback82-85).
pub fn asked_by() -> Option<PathBuf> {
    let p = crate::policy::get();
    p.files.iter().zip(p.kinds.iter()).find(|(_, k)| k.is_providers()).map(|(f, _)| f.clone())
}

/// What a provider's policy says of a program that is not completely confirmed: `what` is missing.
fn provider_line(layer: &Path, what: &str) -> (bool, String) {
    let stop = stops(env!("CARGO_PKG_VERSION"));
    let then = if stop { "so it does not start".to_string() } else { format!("from Quillo {} it will not start so", STOPS_FROM) };
    (stop, format!("the policy {} asks for a complete {}, and {}: {} (`quillo confirm` writes it)", layer.display(), FILE, what, then))
}

// ---- a start -----------------------------------------------------------------------------------

/// Before a program starts: reads its `quillo.confirmed` and holds the run to it, saying once what
/// is not confirmed; on a terminal (`ask`), asks about that first. A Rust file not confirmed stops
/// the start; so does, under a provider's policy, a program not completely confirmed, from
/// `STOPS_FROM` on. With no `quillo.confirmed` and no provider's policy, nothing is read: one look
/// for the file.
pub fn at_start(main: &Path, ask: bool) -> Result<(), i32> {
    at_start_in(main, &dir_of(main), ask)
}

/// `at_start`, with the `quillo.confirmed` of `dir`: a service its app started apart is held by
/// the app's, never by one beside the service's own file (FEEDBACK83.md, Q-1300, found in review;
/// branch feedback82-85).
pub fn at_start_in(main: &Path, dir: &Path, ask: bool) -> Result<(), i32> {
    let dir = dir.to_path_buf();
    let path = dir.join(FILE);
    let layer = asked_by();
    let conf = match read(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("quillo: {}", e);
            return Err(2);
        }
    };
    let Some(mut conf) = conf else {
        if let Some(layer) = layer {
            let (stop, line) = provider_line(&layer, &format!("{} has none", dir.join(FILE).display()));
            eprintln!("quillo: {}", line);
            if stop {
                return Err(2);
            }
        }
        return Ok(());
    };
    // A link is read and trusted as the file is, and said once, with where it leads: what holds
    // the program is then a file elsewhere, which a provider may want to know (FEEDBACK83.md,
    // Q-1300, found by the coverage tests; branch feedback82-85). A checkout has a plain file.
    if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        let to = path.canonicalize().map_or_else(|_| "nowhere".to_string(), |r| r.display().to_string());
        eprintln!("quillo: {} is a symbolic link, to {}: the program is held to what that file says", escaped(&path.display().to_string()), escaped(&to));
    }
    // The program's own, served by no name and written by no builtin, as its policy is.
    crate::builtins::protect_named(&path, crate::builtins::OWN_POLICY);
    let found = scan_from(main, &dir, &HashMap::new(), true);
    let mut left = missing(&found, &conf);
    if !left.is_empty() && ask {
        eprintln!("quillo: {} ({}):{}", summary(&left), FILE, listing(&found, &left));
        eprint!("  [y] confirm them   [n] not now: ");
        let mut answer = String::new();
        let _ = std::io::stdin().read_line(&mut answer);
        if answer.trim().eq_ignore_ascii_case("y") {
            conf = merged(&found, &conf, &|f| left.iter().any(|m| m.file == f));
            fetch_integrity(&mut conf);
            if let Err(e) = write(&path, &conf) {
                eprintln!("quillo: {}", e);
                return Err(1);
            }
            left = missing(&found, &conf);
        }
    }
    if !left.is_empty() {
        eprintln!("quillo: {} ({}): each is refused as it is reached; `quillo confirm` lists them", summary(&left), FILE);
        if let Some(layer) = &layer {
            let (stop, line) = provider_line(layer, &format!("{} leaves {} out", FILE, plural(left.len(), "thing", "things")));
            eprintln!("quillo: {}", line);
            if stop {
                return Err(2);
            }
        }
    }
    // Rust does not load unconfirmed, and the start stops (the user's decision, question 3).
    if let Some(m) = left.iter().find(|m| m.kind == "sha256") {
        let full = dir.join(&m.file);
        eprintln!("quillo: {}", plugin_message(&conf, m, &full));
        return Err(1);
    }
    hold(&dir, main, &conf, &found);
    Ok(())
}

/// Why `quillo build` may not build `files`, the Rust files of `main`, if it may not: the program
/// has a `quillo.confirmed`, and one of them is not in it at its SHA-256; else whether it has one.
pub fn rust_confirmed(main: &Path, files: &[PathBuf]) -> Result<bool, String> {
    let dir = dir_of(main);
    let Some(conf) = read(&dir.join(FILE))? else { return Ok(false) };
    for f in files {
        let name = rel_name(&dir, f);
        let bytes = crate::error::read_file(f).map_err(|e| format!("{}: {}", f.display(), e))?;
        let have = crate::crypto::hex(&<sha2::Sha256 as sha2::Digest>::digest(&bytes));
        if conf.get(&name).and_then(|b| b.sha256.as_deref()) != Some(have.as_str()) {
            let m = Missing { file: name, kind: "sha256", item: have, lines: Vec::new() };
            return Err(format!("{}: nothing built", plugin_message(&conf, &m, f)));
        }
    }
    Ok(true)
}

fn plugin_message(conf: &Confirmed, m: &Missing, _full: &Path) -> String {
    let file = escaped(&m.file);
    match conf.get(&m.file).and_then(|b| b.sha256.as_ref()) {
        Some(_) => format!("{} is not the file confirmed: its SHA-256 is {} now; Rust cannot be held to a list, so it does not load (`quillo confirm` shows it)", file, m.item),
        None => format!("{} is not confirmed (its SHA-256 is {}); Rust cannot be held to a list, so it does not load (`quillo confirm` asks about it)", file, m.item),
    }
}

/// Writes `quillo.confirmed` whole or not at all.
pub fn write(path: &Path, conf: &Confirmed) -> Result<(), String> {
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(format!("{} is a symbolic link: nothing written", path.display()));
    }
    // Made new, never opened through what is at its name: `fs::write` followed a link left there
    // and wrote the list wherever it led. One a run of this process's number left is taken away
    // first, a link at that name is not, and nothing is written (FEEDBACK83.md, Q-1300, found by
    // the coverage tests; branch feedback82-85).
    let tmp = path.with_file_name(format!(".{}.{}.part", FILE, std::process::id()));
    match std::fs::symlink_metadata(&tmp) {
        Ok(m) if m.file_type().is_symlink() => return Err(format!("{} is a symbolic link: nothing written", tmp.display())),
        Ok(_) => {
            let _ = std::fs::remove_file(&tmp);
        }
        Err(_) => {}
    }
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .and_then(|mut f| std::io::Write::write_all(&mut f, render(conf).as_bytes()))
        .map_err(|e| {
            if e.kind() != std::io::ErrorKind::AlreadyExists {
                let _ = std::fs::remove_file(&tmp);
            }
            format!("{}: {}", tmp.display(), e)
        })?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {}", path.display(), e)
    })
}

/// Each confirmed script with no integrity hash yet, fetched once for its SHA-384 (FEEDBACK83.md,
/// Q-1304; branch feedback82-85). One that cannot be fetched is confirmed without, and said.
pub fn fetch_integrity(conf: &mut Confirmed) {
    let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(20)).build();
    for b in conf.values_mut() {
        let urls: Vec<String> = b.scripts.keys().filter(|u| !b.integrity.contains_key(*u) && is_address(u)).cloned().collect();
        for u in urls {
            let url = if u.starts_with("//") { format!("https:{}", u) } else { u.clone() };
            let got = agent.get(&url).call().map_err(|e| e.to_string()).and_then(|r| {
                let mut bytes = Vec::new();
                std::io::Read::read_to_end(&mut std::io::Read::take(r.into_reader(), 16 << 20), &mut bytes).map_err(|e| e.to_string())?;
                Ok(bytes)
            });
            match got {
                Ok(bytes) => {
                    let h = <sha2::Sha384 as sha2::Digest>::digest(&bytes);
                    b.integrity.insert(u, format!("sha384-{}", crate::crypto_b64(&h)));
                }
                Err(e) => eprintln!("quillo: {} could not be fetched for its integrity hash ({}): confirmed without one", escaped(&u), escaped(&e)),
            }
        }
    }
}

// ---- quillo check ------------------------------------------------------------------------------

/// What `quillo check` says of a program's reach, beside its own findings: a taken file that adds
/// routes or works at start (Q-1301), a Rust file whose effects line leaves out what it calls
/// (Q-1305), and warnings; a written address the policy's hosts refuse (Q-1302), what is not in
/// the program's `quillo.confirmed`, and errors.
pub fn for_check(main: &Path, parsed: &HashMap<PathBuf, std::sync::Arc<Vec<Stmt>>>) -> Vec<crate::check::Diagnostic> {
    use crate::check::{Diagnostic, Severity};
    let dir = dir_of(main);
    let path = dir.join(FILE);
    let shown = |f: &str| if dir == Path::new(".") { f.to_string() } else { dir.join(f).display().to_string() };
    let mut out = Vec::new();
    let conf = match read(&path) {
        Ok(c) => c,
        Err(e) => {
            out.push(Diagnostic { file: path.display().to_string(), line: 0, severity: Severity::Error, message: e });
            return out;
        }
    };
    let layer = asked_by();
    let p = crate::policy::get();
    let found = scan_with(main, parsed, conf.is_some() || layer.is_some() || !p.hosts.is_empty());
    for (file, b) in &found.files {
        for (line, note) in &b.notes {
            // The program's texts `escaped`, as the listing has them (FEEDBACK83.md, Q-1300 D.2,
            // found by the coverage tests; branch feedback82-85).
            let message = if b.rust { format!("{} {}", escaped(file), escaped(note)) } else { escaped(note).into_owned() };
            out.push(Diagnostic { file: shown(file), line: *line, severity: Severity::Warning, message });
        }
        // A written address the policy refuses (FEEDBACK83.md, Q-1302; branch feedback82-85).
        for (host, lines) in &b.http {
            if let Some(why) = p.refuses_host(host) {
                for l in lines {
                    out.push(Diagnostic { file: shown(file), line: *l, severity: Severity::Error, message: format!("http: {}", why) });
                }
            }
        }
    }
    match &conf {
        Some(conf) => {
            let left = missing(&found, conf);
            for m in &left {
                let message = if m.kind == "sha256" {
                    plugin_message(conf, m, &dir.join(&m.file))
                } else {
                    let word = KINDS.iter().find(|(k, _)| *k == m.kind).map_or(m.kind, |(_, w)| w);
                    format!("{} {}: not confirmed for this file ({}; `quillo confirm` lists it)", word, escaped(&m.item), FILE)
                };
                let line = m.lines.first().copied().unwrap_or(0);
                out.push(Diagnostic { file: shown(&m.file), line, severity: Severity::Error, message });
            }
            if let (Some(layer), false) = (&layer, left.is_empty()) {
                let (stop, line) = provider_line(layer, &format!("{} leaves {} out", FILE, plural(left.len(), "thing", "things")));
                out.push(Diagnostic { file: path.display().to_string(), line: 0, severity: if stop { Severity::Error } else { Severity::Warning }, message: line });
            }
        }
        None => {
            if let Some(layer) = layer {
                let (stop, line) = provider_line(&layer, &format!("{} has none", path.display()));
                out.push(Diagnostic { file: main.display().to_string(), line: 0, severity: if stop { Severity::Error } else { Severity::Warning }, message: line });
            }
        }
    }
    out
}

// ---- quillo confirm ----------------------------------------------------------------------------

pub const USAGE: &str = "\
usage:
  quillo confirm [<file.qwe>] [PATH...]   list what the program's files reach that is not
                                          confirmed yet, ask, and write quillo.confirmed
options:
  PATH...           only the files in these folders, or these files
  --yes             confirm them without asking, as you would at a terminal; in
                    CI run `quillo check` instead, which fails on what is not
                    confirmed (--yes there would confirm whatever a change brought)
  --list            list everything each file reaches; write nothing
  --effects FILE... what a reading of each Rust file finds it does, as an
                    `//! effects:` line, and where its own line leaves that out";

/// `quillo confirm ...`; the exit status.
pub fn main(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{}", USAGE);
        return 0;
    }
    if args.first().is_some_and(|a| a == "--effects") {
        return effects_command(&args[1..]);
    }
    let (mut yes, mut list) = (false, false);
    let mut program: Option<String> = None;
    let mut paths: Vec<String> = Vec::new();
    for a in args {
        match a.as_str() {
            "--yes" | "-y" => yes = true,
            "--list" => list = true,
            o if o.starts_with('-') => {
                eprintln!("quillo confirm: unknown option `{}`\n{}", o, USAGE);
                return 2;
            }
            p if program.is_none() && paths.is_empty() && p.to_ascii_lowercase().ends_with(".qwe") => program = Some(p.to_string()),
            p => paths.push(p.to_string()),
        }
    }
    let main = PathBuf::from(program.unwrap_or_else(|| "main.qwe".into()));
    if !main.is_file() {
        eprintln!("quillo confirm: no program: {} is not a file (name the main .qwe file first)", main.display());
        return 2;
    }
    let dir = dir_of(&main);
    // The policy, to say what it refuses of what is confirmed: a list only narrows (D.1).
    if let Ok(p) = crate::policy::load(&main, &[]) {
        crate::policy::set(p);
    }
    let found = scan(&main);
    if list {
        print!("{}", whole_listing(&found));
        return 0;
    }
    let path = dir.join(FILE);
    let conf = match read(&path) {
        Ok(c) => c.unwrap_or_default(),
        Err(e) => {
            eprintln!("quillo confirm: {}", e);
            return 2;
        }
    };
    let chosen = |f: &str| paths.is_empty() || paths.iter().any(|p| path_within(&normal(&format!("{}/", p.trim_end_matches('/'))), f) || normal(p) == f);
    let left: Vec<Missing> = missing(&found, &conf).into_iter().filter(|m| chosen(&m.file)).collect();
    let stale_n = stale(&found, &conf, &chosen);
    if left.is_empty() {
        if stale_n > 0 || !path.exists() {
            let next = merged(&found, &conf, &chosen);
            if let Err(e) = write(&path, &next) {
                eprintln!("quillo confirm: {}", e);
                return 1;
            }
            if stale_n > 0 {
                println!("quillo: {} no longer in the files left {}", plural(stale_n, "item", "items"), FILE);
            }
        }
        println!("quillo: everything the program's files reach is confirmed ({})", path.display());
        return 0;
    }
    let n = left.iter().map(|m| &m.file).collect::<HashSet<_>>().len();
    println!("quillo: {} something not confirmed yet{}", plural(n, "file reaches", "files reach"), listing(&found, &left));
    let mut take: HashSet<String> = HashSet::new();
    if yes {
        take.extend(left.iter().map(|m| m.file.clone()));
    } else if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        print!("  [y] confirm all   [f] file by file   [n] none: ");
        let _ = std::io::Write::flush(&mut std::io::stdout());
        match read_answer().as_str() {
            "y" => take.extend(left.iter().map(|m| m.file.clone())),
            "f" => {
                let mut files: Vec<&str> = left.iter().map(|m| m.file.as_str()).collect();
                files.dedup();
                for f in files {
                    print!("  {}? [y/n]: ", escaped(f));
                    let _ = std::io::Write::flush(&mut std::io::stdout());
                    if read_answer() == "y" {
                        take.insert(f.to_string());
                    }
                }
            }
            _ => {}
        }
    } else {
        eprintln!("quillo confirm: no one to ask here (not a terminal): run it at a terminal, or `quillo confirm --yes` once you have read the list; in CI, `quillo check` fails on what is not confirmed");
        return 1;
    }
    if take.is_empty() {
        println!("quillo: nothing confirmed");
        return 1;
    }
    let mut next = merged(&found, &conf, &|f| take.contains(f));
    fetch_integrity(&mut next);
    if let Err(e) = write(&path, &next) {
        eprintln!("quillo confirm: {}", e);
        return 1;
    }
    println!("quillo: wrote {} ({} confirmed)", path.display(), plural(take.len(), "file", "files"));
    // A list only narrows: what the policy refuses stays refused (D.1).
    let p = crate::policy::get();
    for (file, b) in next.iter().filter(|(f, _)| take.contains(*f)) {
        for host in b.http.keys() {
            if let Some(why) = p.refuses_host(host) {
                println!("  note: {}: {} stays refused: {}", escaped(file), escaped(host), why);
            }
        }
    }
    let rest = missing(&found, &next).len();
    i32::from(rest > 0)
}

fn read_answer() -> String {
    let mut a = String::new();
    let _ = std::io::stdin().read_line(&mut a);
    a.trim().to_ascii_lowercase()
}

/// Everything each file reaches, for `--list`, its texts `escaped` as `listing`'s are.
fn whole_listing(found: &Scan) -> String {
    let mut s = String::new();
    for (file, b) in &found.files {
        if b.is_empty() && b.notes.is_empty() {
            continue;
        }
        s.push_str(&format!("{}\n", escaped(file)));
        if b.rust {
            s.push_str(&format!("  {:<13} {}\n", "sha256", b.sha256.as_deref().unwrap_or("")));
            s.push_str(&format!("  {:<13} {}\n", "says effects", b.effects.as_deref().unwrap_or("nothing (every effect)")));
            s.push_str(&format!("  {:<13} {}\n", "reads as", if b.found_effects == 0 { "none".to_string() } else { effect_names(b.found_effects).join(", ") }));
            for k in ["written", "env", "crates"] {
                if !b.items(k).is_empty() {
                    s.push_str(&format!("  {:<13} {}\n", k, escaped(&b.items(k).keys().cloned().collect::<Vec<_>>().join(", "))));
                }
            }
        }
        for (k, word) in KINDS {
            for (item, lines) in b.items(k) {
                s.push_str(&format!("  {:<13} {:<40} {}\n", word, escaped(item), lines.iter().map(u32::to_string).collect::<Vec<_>>().join(", ")));
            }
        }
        for (_, n) in &b.notes {
            s.push_str(&format!("  note: {}\n", escaped(n)));
        }
    }
    s
}

/// `quillo confirm --effects FILE...`: the line a reading proposes for each Rust file, and where
/// its own leaves that out (FEEDBACK83.md, Q-1305 and Q-1306; branch feedback82-85). Exit 1 where
/// one does.
fn effects_command(files: &[String]) -> i32 {
    if files.is_empty() {
        eprintln!("quillo confirm: --effects takes Rust files: quillo confirm --effects plugin.qwer");
        return 2;
    }
    let mut code = 0;
    for f in files {
        let Ok(bytes) = crate::error::read_file(Path::new(f)) else {
            eprintln!("quillo confirm: cannot read {}", f);
            code = 2;
            continue;
        };
        let b = scan_rust(&bytes);
        let proposed = if b.found_effects == 0 { "none".to_string() } else { effect_names(b.found_effects).join(", ") };
        println!("{}: effects: {}", f, proposed);
        for (_, n) in &b.notes {
            println!("  but it {}", escaped(n));
            code = code.max(1);
        }
    }
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_written_out() {
        assert_eq!(host_of("https://API.example.com/x", true).as_deref(), Some("api.example.com"));
        assert_eq!(host_of("http://u:p@127.0.0.1:47831/boot", true).as_deref(), Some("127.0.0.1"));
        assert_eq!(host_of("https://[::1]:8080/", true).as_deref(), Some("::1"));
        assert_eq!(host_of("https://api.", false), None);
        assert_eq!(host_of("https://api.x.com", false), None);
        assert_eq!(host_of("https://api.x.com?", false).as_deref(), Some("api.x.com"));
        assert_eq!(host_of("ftp://x", true), None);
        assert_eq!(host_of("https://", true), None);
    }

    #[test]
    fn paths_compared() {
        assert_eq!(normal("./a/../b/"), "b/");
        assert_eq!(normal("a/../../.env"), "../.env");
        assert_eq!(normal("/srv/x/../y"), "/srv/y");
        assert!(path_within("uploads/", "uploads/a.png"));
        assert!(!path_within("uploads/", "uploads/../.env"));
        assert!(path_within("data/notes.txt", "./data/notes.txt"));
        assert!(!path_within("data/notes.txt", "data/notes.txt2"));
        assert!(path_within("data/", "data"));
    }

    #[test]
    fn the_stopping_release() {
        assert!(!stops("0.8.10"));
        assert!(!stops("0.8.11"));
        assert!(stops("0.8.12"));
        assert!(stops("0.9.0"));
        assert!(stops("1.0.0-rc1"));
    }

    #[test]
    fn the_file_read_and_written() {
        let text = "# c\n[\"a b.qwe\"]\nhttp = [\"API.x.com\", \"y.org\"]\nscripts = [\"https://c.dev/l.js sha384-abc\"]\nfiles = [\n  \"./up/\",\n]\n\n[\"p.qwer\"]\nsha256 = \"".to_string() + &"a".repeat(64) + "\"\neffects = \"none\"\n";
        let c = parse(&text).unwrap();
        assert!(c["a b.qwe"].http.contains_key("api.x.com"));
        assert_eq!(c["a b.qwe"].integrity["https://c.dev/l.js"], "sha384-abc");
        assert!(c["a b.qwe"].files.contains_key("up/"));
        assert_eq!(c["p.qwer"].sha256.as_deref(), Some(&*"a".repeat(64)));
        let again = parse(&render(&c)).unwrap();
        assert_eq!(render(&again), render(&c));
        assert!(parse("[\"x\"]\nnope = []\n").unwrap_err().contains("line 2"));
        assert!(parse("http = []\n").unwrap_err().contains("before any file"));
        assert!(parse("[\"x\"]\nsha256 = \"12\"\n").unwrap_err().contains("64"));
        assert!(parse("[\"x\"]\nhttp = [\"a\"\n").unwrap_err().contains("does not end"));
        assert!(parse("").unwrap().is_empty());
    }

    #[test]
    fn rust_read_for_what_it_calls() {
        let src = "//! effects: none\nuse std::io::Write;\n/// reads `std::fs` in a comment\npub fn greet() -> String {\n    let h = std::fs::read_to_string(\"/etc/hosts\");\n    let s = std::net::TcpStream::connect(\"127.0.0.1:1\");\n    let k = std::env::var(\"API_KEY\");\n    let c = '\"';\n    \"x\".into()\n}\n";
        let b = scan_rust(src.as_bytes());
        assert_eq!(effect_names(b.found_effects), vec!["fs", "env", "net"]);
        assert!(b.written.contains_key("/etc/hosts"));
        assert!(b.env.contains_key("API_KEY"));
        assert!(b.notes[0].1.contains("std::fs::read_to_string (line 5)"), "{:?}", b.notes);
        // clap's Command reads arguments; std's starts a program.
        assert_eq!(rust_effects("use clap::Command;\nfn a() { Command::new(\"x\"); }").0, 0);
        assert_eq!(rust_effects("use std::process::Command;\nfn a() { Command::new(\"sh\"); }").0, effect_bit("run"));
        // A name that ends with another's is not it; raw strings and lifetimes pass.
        assert_eq!(rust_effects("fn a<'a>(x: &'a str) { my_fs::read(r#\"std::fs::\"#); }").0, 0);
    }

    // FEEDBACK83.md, Q-1305 (branch feedback82-85): what the review of the 632 catalog plugins
    // found the reading missed or overstated, one test for each kind.

    fn reads(code: &str) -> Vec<&'static str> {
        effect_names(rust_effects(code).0)
    }

    /// A file with these dependency lines and this code, as `quillo confirm --effects` reads it.
    fn reads_with(deps: &str, code: &str) -> Vec<&'static str> {
        let header: String = deps.lines().map(|l| format!("//! {}\n", l)).collect();
        let src = format!("//! version: 1.0.0\n//!\n//! [dependencies]\n{}\n{}", header, code);
        effect_names(scan_rust(src.as_bytes()).found_effects)
    }

    #[test]
    fn rust_read_a_wait_as_time() {
        assert_eq!(reads("fn a() { std::thread::sleep(std::time::Duration::from_millis(5)); }"), vec!["time"]);
        assert_eq!(reads("use std::thread::sleep;\nfn a() { sleep(D); }"), vec!["time"]);
        assert_eq!(reads("async fn a() { tokio::time::sleep(D).await; }"), vec!["time"]);
        // Its own function by that name, or one that ends with it, is not a wait.
        assert!(reads("fn sleep(n: u8) -> u8 { n }\nfn a() { my_sleep(1); }").is_empty());
    }

    #[test]
    fn rust_read_crates_that_do_it_unseen() {
        assert_eq!(reads_with("uuid = { version = \"1\", features = [\"v4\"] }", "fn a() {}"), vec!["random"]);
        assert_eq!(reads_with("uuid = { version = \"1\", features = [\"v7\"] }", "fn a() {}"), vec!["time", "random"]);
        assert_eq!(reads_with("uuid = \"1\"", "fn a() -> String { uuid::Uuid::now_v7().to_string() }"), vec!["time", "random"]);
        assert_eq!(reads_with("uuid = \"1\"", "fn a() -> String { Uuid::new_v4().to_string() }"), vec!["random"]);
        // v5 is the same id for the same name: nothing.
        assert!(reads_with("uuid = { version = \"1\", features = [\"v5\"] }", "fn a() { Uuid::new_v5(&N, b\"x\"); }").is_empty());
        assert_eq!(reads_with("totp-rs = \"5\"", "fn a() { Secret::generate_secret(); }"), vec!["random"]);
        assert_eq!(reads_with("bcrypt = \"0.15\"", "fn a() { bcrypt::hash(p, 12); }"), vec!["random"]);
        assert!(reads_with("bcrypt = \"0.15\"", "fn legacy_django_hash() { bcrypt::verify(p, h); }").is_empty());
        assert_eq!(reads_with("pasetors = \"0.7\"", "fn a() {}"), vec!["time", "random"]);
        assert_eq!(reads_with("biscuit-auth = \"6\"", "fn a() { KeyPair::new(); }"), vec!["random"]);
        assert_eq!(reads_with("biscuit-auth = \"6\"", "fn a() { AuthorizerBuilder::new().time().build(&t); }"), vec!["time"]);
        assert_eq!(reads_with("age = \"0.11\"", "fn a() { age::x25519::Identity::generate(); }"), vec!["random"]);
        assert_eq!(reads_with("captcha = \"0.0.9\"", "fn a() {}"), vec!["random"]);
        assert_eq!(reads_with("sharks = \"0.5\"", "fn a() { Sharks(2).dealer(&d); }"), vec!["random"]);
        assert_eq!(reads_with("bloomfilter = \"3\"", "fn a() {}"), vec!["random"]);
        assert_eq!(reads_with("bson = \"2\"", "fn a() { ObjectId::new(); }"), vec!["time", "random"]);
        assert_eq!(reads_with("icalendar = \"0.16\"", "fn a() {}"), vec!["time", "random"]);
        assert_eq!(reads_with("epub-builder = \"0.8\"", "fn a() { b.generate(&mut out); }"), vec!["time", "random"]);
        assert_eq!(reads_with("printpdf = \"0.7\"", "fn a() { PdfDocument::new(t, w, h, l); }"), vec!["time"]);
        assert_eq!(reads_with("rusty-s3 = \"0.5\"", "fn a() { b.get_object(Some(&c), k).sign(d); }"), vec!["time"]);
        assert_eq!(reads_with("pgp = \"0.21\"", "fn a() { m.sign(&k, p); }"), vec!["time"]);
        assert_eq!(reads_with("feed-rs = \"2\"", "fn a() {}"), vec!["random"]);
        assert_eq!(reads_with("lettre = \"0.11\"", "fn a() { Message::builder(); }"), vec!["time", "random", "net"]);
        assert_eq!(reads_with("lettre = \"0.11\"", "fn a() {}"), vec!["net"]);
        assert_eq!(reads_with("cuid2 = \"0.1\"", "fn a() {}"), vec!["time", "random"]);
    }

    #[test]
    fn rust_read_a_hash_seed_as_random() {
        assert_eq!(reads("fn a() { let s = std::collections::hash_map::RandomState::new(); }"), vec!["random"]);
        assert_eq!(reads("fn a() { let s = RandomState::default(); }"), vec!["random"]);
    }

    #[test]
    fn rust_read_a_builders_file() {
        assert_eq!(reads("fn a(p: &str) { csv::ReaderBuilder::new().flexible(true).from_path(p); }"), vec!["fs"]);
        assert_eq!(reads("fn a(p: &str) { let r = DbBuilder::new()\n    .cache(9)\n    .open(p); }"), vec!["fs"]);
        // Not a builder's: hpke opens a message, a maze a cell.
        assert!(reads("fn a() { let m = ctx.open(ct, aad); for n in g.open(c) {} }").is_empty());
    }

    #[test]
    fn rust_read_a_name_looked_up_as_net() {
        assert_eq!(reads("fn a(h: &str) { (h, 80).to_socket_addrs(); }"), vec!["net"]);
        assert_eq!(reads("use std::net::ToSocketAddrs;\nfn a() {}"), vec!["net"]);
    }

    #[test]
    fn rust_read_what_asks_about_a_path_as_fs() {
        for code in ["Path::new(p).exists()", "Path::new(p).is_dir()", "p.is_file()", "p.metadata()", "p.read_dir()", "std::fs::metadata(p)", "fs::read_dir(p)", "p.canonicalize()", "p.try_exists()"] {
            assert_eq!(reads(&format!("fn a() {{ {}; }}", code)), vec!["fs"], "{}", code);
        }
    }

    #[test]
    fn rust_read_constructors_that_take_no_file() {
        assert!(reads("fn a(u: &str) { redis::Client::open(u); Client::open(u); }").is_empty());
        assert!(reads("fn a() { Stemmer::create(Algorithm::English); SnippetGenerator::create(&s, &*q, f); mime_guess::from_path(n); }").is_empty());
        // What does open or make a file still does.
        for code in ["image::open(p)", "std::fs::File::open(p)", "hound::WavWriter::create(p, s)", "Database::create(p)", "shapefile::Writer::from_path(p, t)", "Tiff::open(p)"] {
            assert_eq!(reads(&format!("fn a() {{ {}; }}", code)), vec!["fs"], "{}", code);
        }
        // A call is named without what it is given.
        let b = scan_rust(b"//! effects: none\nfn a(p: &str) { image::open(p); }\n");
        assert!(b.notes[0].1.contains("calls image::open (line 2)"), "{:?}", b.notes);
    }
}
