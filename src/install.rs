//! `quillo install <plugin>...`: Rust plugins (`.qwer` files) from quillolang.org's
//! catalog, into a program's folder.
//!
//! The catalog is `<registry>/plugins/index.json` (the registry is
//! https://quillolang.org, or `--registry` / `QUILLO_REGISTRY`): each plugin's name,
//! version, the SHA-256 of its file, and where its files are. A plugin is downloaded,
//! its hash and the version in its header checked against the catalog, and written
//! beside the program (`--dir` for another folder) as `<name>.qwer`. A file already
//! there is left alone when it is the same, replaced when the catalog's is newer, and
//! kept when it is newer or was changed by hand (unless `--force`).
//!
//!   quillo install csv qr jwt          several at once
//!   quillo install csv --example       and its example program, csv.qwe
//!   quillo install --update            every installed plugin the catalog has newer
//!   quillo install --list [word]       what the catalog has

use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

const REGISTRY: &str = "https://quillolang.org";
/// No plugin, example or catalog is bigger than this.
const MAX_DOWNLOAD: u64 = 8 << 20;

const USAGE: &str = "\
usage:
  quillo install <name>[@version]...   download plugins into this folder
  quillo install --update              update the plugins installed here
  quillo install --list [word]         the plugins there are

options:
  --example         also download each plugin's example program (<name>.qwe)
  --dir DIR         install into DIR instead of this folder
  --force           replace a file that was changed by hand, or is newer
  --registry URL    another catalog (default $QUILLO_REGISTRY, then https://quillolang.org)";

#[derive(Debug, Clone)]
struct Entry {
    name: String,
    title: String,
    version: String,
    summary: String,
    group: String,
    sha256: String,
    file: String,
    example: String,
    files: Vec<String>,
}

#[derive(Debug, Default)]
struct Options {
    names: Vec<String>,
    example: bool,
    force: bool,
    update: bool,
    list: bool,
    dir: Option<PathBuf>,
    registry: Option<String>,
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut o = Options::default();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let (flag, inline) = match a.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_string())),
            _ => (a, None),
        };
        let mut value = |what: &str| -> Result<String, String> {
            if let Some(v) = inline.clone() {
                return Ok(v);
            }
            i += 1;
            args.get(i).cloned().ok_or(format!("{} takes {}", flag, what))
        };
        match flag {
            "--example" | "--examples" => o.example = true,
            "--force" | "-f" => o.force = true,
            "--update" | "-u" => o.update = true,
            "--list" | "-l" => o.list = true,
            "--dir" => o.dir = Some(PathBuf::from(value("a folder")?)),
            "--registry" => o.registry = Some(value("a URL")?),
            "--help" | "-h" => return Err(String::new()),
            f if f.starts_with('-') => return Err(format!("unknown option `{}`", f)),
            name => o.names.push(name.to_string()),
        }
        i += 1;
    }
    Ok(o)
}

/// `csv` or `csv@1.2.0`.
fn split_name(arg: &str) -> Result<(String, Option<String>), String> {
    let (name, version) = match arg.split_once('@') {
        Some((n, v)) => (n, Some(v.trim_start_matches('v').to_string())),
        None => (arg, None),
    };
    let name = name.trim_end_matches(".qwer").to_ascii_lowercase().replace('-', "_");
    if name.is_empty() || !name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_') {
        return Err(format!("`{}` is not a plugin name (letters, digits and _)", arg));
    }
    Ok((name, version))
}

/// The `//! version: X.Y.Z` line of a plugin's header.
fn header_version(text: &str) -> Option<String> {
    text.lines()
        .take_while(|l| l.trim_start().starts_with("//!"))
        .find_map(|l| l.trim_start().trim_start_matches("//!").trim().strip_prefix("version:").map(|v| v.trim().to_string()))
}

/// Versions compared as numbers, part by part: 1.10.0 after 1.9.0.
fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let parts = |v: &str| v.split(|c| c == '.' || c == '-').map(|p| p.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
    parts(a).cmp(&parts(b))
}

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{:02x}", b)).collect()
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .user_agent(concat!("quillo/", env!("CARGO_PKG_VERSION"), " (install)"))
        .build()
}

fn download(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>, String> {
    let resp = agent.get(url).call().map_err(|e| match e {
        ureq::Error::Status(code, _) => format!("{} answered {}", url, code),
        e => format!("{}: {}", url, e),
    })?;
    let mut bytes = Vec::new();
    resp.into_reader().take(MAX_DOWNLOAD + 1).read_to_end(&mut bytes).map_err(|e| format!("{}: {}", url, e))?;
    if bytes.len() as u64 > MAX_DOWNLOAD {
        return Err(format!("{} is larger than {} MB", url, MAX_DOWNLOAD >> 20));
    }
    Ok(bytes)
}

fn catalog(agent: &ureq::Agent, registry: &str) -> Result<Vec<Entry>, String> {
    let url = format!("{}/plugins/index.json", registry);
    let bytes = download(agent, &url)?;
    let j: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| format!("{}: not a catalog ({})", url, e))?;
    if j["format"].as_u64() != Some(1) {
        return Err(format!("{}: a catalog in a format this quillo does not read; a newer quillo may", url));
    }
    let text = |v: &serde_json::Value, k: &str| v[k].as_str().unwrap_or("").to_string();
    Ok(j["plugins"]
        .as_array()
        .map(|ps| {
            ps.iter()
                .map(|p| Entry {
                    name: text(p, "name"),
                    title: text(p, "title"),
                    version: text(p, "version"),
                    summary: text(p, "summary"),
                    group: text(p, "group"),
                    sha256: text(p, "sha256"),
                    file: text(p, "file"),
                    example: text(p, "example"),
                    files: p["files"].as_array().map(|f| f.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default(),
                })
                .filter(|e| !e.name.is_empty() && !e.file.is_empty())
                .collect()
        })
        .unwrap_or_default())
}

/// A URL in the catalog: absolute, or a path on the registry.
fn absolute(registry: &str, u: &str) -> String {
    if u.starts_with("https://") || u.starts_with("http://") {
        u.to_string()
    } else {
        format!("{}/{}", registry, u.trim_start_matches('/'))
    }
}

/// Written whole or not at all: to a temporary file beside it, then renamed.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension(format!("{}.part", path.extension().and_then(|e| e.to_str()).unwrap_or("")));
    std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {}", tmp.display(), e))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {}", path.display(), e)
    })
}

/// The names nearest a misspelt one: those sharing its start, else containing it.
fn suggest(name: &str, all: &[Entry]) -> String {
    let start: String = name.chars().take(3).collect();
    let mut near: Vec<&str> = all.iter().filter(|e| e.name.starts_with(&start) || e.name.contains(name) || name.contains(&e.name)).map(|e| e.name.as_str()).take(5).collect();
    near.sort();
    if near.is_empty() {
        " (`quillo install --list` shows them all)".into()
    } else {
        format!(" (did you mean {}?)", near.iter().map(|n| format!("`{}`", n)).collect::<Vec<_>>().join(", "))
    }
}

/// What happened to one plugin.
enum Done {
    Installed,
    Updated(String),
    UpToDate,
    Kept(String),
}

fn install_one(agent: &ureq::Agent, registry: &str, e: &Entry, o: &Options, dir: &Path) -> Result<Done, String> {
    let path = dir.join(format!("{}.qwer", e.name));
    let had = std::fs::read(&path).ok();
    if let Some(old) = &had {
        if hex_sha256(old) == e.sha256 {
            return Ok(Done::UpToDate);
        }
        let mine = header_version(&String::from_utf8_lossy(old)).unwrap_or_else(|| "0".into());
        if !o.force {
            match version_cmp(&mine, &e.version) {
                std::cmp::Ordering::Less => {}
                std::cmp::Ordering::Equal => return Ok(Done::Kept(format!("{} {} here differs from the catalog's: changed by hand? --force replaces it", e.name, mine))),
                std::cmp::Ordering::Greater => return Ok(Done::Kept(format!("{} {} here is newer than the catalog's {}; --force replaces it", e.name, mine, e.version))),
            }
        }
    }
    let bytes = download(agent, &absolute(registry, &e.file))?;
    let got = hex_sha256(&bytes);
    if got != e.sha256 {
        return Err(format!("{}: the download's SHA-256 is {}, the catalog says {}; nothing written", e.name, got, e.sha256));
    }
    match header_version(&String::from_utf8_lossy(&bytes)) {
        Some(v) if v == e.version => {}
        v => return Err(format!("{}: the file says version {}, the catalog {}; nothing written", e.name, v.unwrap_or_else(|| "none".into()), e.version)),
    }
    std::fs::create_dir_all(dir).map_err(|err| format!("{}: {}", dir.display(), err))?;
    write_atomic(&path, &bytes)?;
    Ok(match had {
        Some(old) => Done::Updated(header_version(&String::from_utf8_lossy(&old)).unwrap_or_else(|| "?".into())),
        None => Done::Installed,
    })
}

/// With --example: the plugin's example program and the files it reads, beside it.
/// One already there is kept, unless --force.
fn install_extras(agent: &ureq::Agent, registry: &str, e: &Entry, o: &Options, dir: &Path) -> Result<(), String> {
    let mut extra = vec![e.example.clone()];
    extra.extend(e.files.iter().cloned());
    for u in extra.iter().filter(|u| !u.is_empty()) {
        let file = u.rsplit('/').next().unwrap_or("");
        if file.is_empty() || file.starts_with('.') {
            continue;
        }
        let target = dir.join(file);
        if target.exists() && !o.force {
            println!("  {} is already here: kept (--force replaces it)", target.display());
            continue;
        }
        write_atomic(&target, &download(agent, &absolute(registry, u))?)?;
        println!("  and {}", target.display());
    }
    Ok(())
}

/// `quillo install ...`; the exit status.
pub fn main(args: &[String]) -> i32 {
    let o = match parse_args(args) {
        Ok(o) => o,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("quillo install: {}", e);
            }
            eprintln!("{}", USAGE);
            return if e.is_empty() { 0 } else { 2 };
        }
    };
    let registry = o.registry.clone().or_else(|| std::env::var("QUILLO_REGISTRY").ok().filter(|r| !r.is_empty())).unwrap_or_else(|| REGISTRY.into());
    let registry = registry.trim_end_matches('/').to_string();
    let dir = o.dir.clone().unwrap_or_else(|| PathBuf::from("."));
    if !o.list && !o.update && o.names.is_empty() {
        eprintln!("quillo install: which plugins? `quillo install csv qr`, or `quillo install --list`");
        eprintln!("{}", USAGE);
        return 2;
    }
    let agent = agent();
    let all = match catalog(&agent, &registry) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("quillo install: {}", e);
            return 1;
        }
    };

    if o.list {
        let words: Vec<String> = o.names.iter().map(|w| w.to_lowercase()).collect();
        let mut shown: Vec<&Entry> = all
            .iter()
            .filter(|e| {
                let hay = format!("{} {} {} {}", e.name, e.title, e.summary, e.group).to_lowercase();
                words.iter().all(|w| hay.contains(w.as_str()))
            })
            .collect();
        shown.sort_by(|a, b| a.group.cmp(&b.group).then(a.name.cmp(&b.name)));
        let width = shown.iter().map(|e| e.name.len()).max().unwrap_or(4);
        // None at the start, not "", or those with no group get no heading
        // (FEEDBACK16.md, Q-131; branch feedback20-fixes).
        let mut group = None;
        for e in &shown {
            if group != Some(&e.group) {
                group = Some(&e.group);
                println!("\n{}", if e.group.is_empty() { "Other" } else { &e.group });
            }
            println!("  {:width$}  {:8}  {}", e.name, e.version, e.summary, width = width);
        }
        println!("\n{} plugin{}{}; `quillo install <name>` to install one", shown.len(), if shown.len() == 1 { "" } else { "s" },
            if words.is_empty() { String::new() } else { format!(" matching {}", words.join(" ")) });
        return 0;
    }

    // The plugins to install: those named, or with --update those already here.
    let mut wanted: Vec<(String, Option<String>)> = Vec::new();
    for n in &o.names {
        match split_name(n) {
            Ok(w) => wanted.push(w),
            Err(e) => {
                eprintln!("quillo install: {}", e);
                return 2;
            }
        }
    }
    if o.update {
        let here: Vec<String> = std::fs::read_dir(&dir)
            .map(|rd| rd.filter_map(|f| f.ok()).filter_map(|f| f.file_name().to_str().and_then(|n| n.strip_suffix(".qwer")).map(str::to_string)).collect())
            .unwrap_or_default();
        for n in here {
            if all.iter().any(|e| e.name == n) && !wanted.iter().any(|(w, _)| *w == n) {
                wanted.push((n, None));
            }
        }
        if wanted.is_empty() {
            println!("no plugins from the catalog in {}", dir.display());
            return 0;
        }
    }

    let mut failed = 0;
    let mut fresh = Vec::new();
    for (name, version) in &wanted {
        let Some(e) = all.iter().find(|e| e.name == *name) else {
            eprintln!("quillo install: no plugin `{}`{}", name, suggest(name, &all));
            failed += 1;
            continue;
        };
        if let Some(v) = version {
            if *v != e.version {
                eprintln!("quillo install: {}@{} is not in the catalog: it has {} {}", name, v, name, e.version);
                failed += 1;
                continue;
            }
        }
        match install_one(&agent, &registry, e, &o, &dir) {
            Ok(done) => {
                match done {
                    Done::Installed => {
                        println!("installed {} {} into {}", e.name, e.version, dir.join(format!("{}.qwer", e.name)).display());
                        fresh.push(e.name.clone());
                    }
                    Done::Updated(from) => println!("updated {} {} → {}", e.name, from, e.version),
                    Done::UpToDate => println!("{} {} is up to date", e.name, e.version),
                    Done::Kept(why) => println!("kept {}", why),
                }
                if o.example {
                    if let Err(err) = install_extras(&agent, &registry, e, &o, &dir) {
                        eprintln!("quillo install: {}", err);
                        failed += 1;
                    }
                }
            }
            Err(err) => {
                eprintln!("quillo install: {}", err);
                failed += 1;
            }
        }
    }
    if !fresh.is_empty() {
        println!("\nUse {} in a program:", if fresh.len() == 1 { "it" } else { "them" });
        for n in &fresh {
            println!("  use \"{}.qwer\"", n);
        }
        if which_cargo().is_none() {
            println!("\nThe first run builds each plugin with cargo, which is not on this PATH: install Rust from https://rustup.rs");
        }
    }
    if failed > 0 {
        1
    } else {
        0
    }
}

fn which_cargo() -> Option<PathBuf> {
    if let Ok(c) = std::env::var("CARGO") {
        return Some(PathBuf::from(c));
    }
    std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join("cargo")).find(|c| c.is_file()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;

    #[test]
    fn names_and_versions() {
        assert_eq!(split_name("csv").unwrap(), ("csv".into(), None));
        assert_eq!(split_name("csv@1.2.0").unwrap(), ("csv".into(), Some("1.2.0".into())));
        assert_eq!(split_name("File-Hash.qwer").unwrap(), ("file_hash".into(), None));
        assert!(split_name("../etc").is_err());
        assert_eq!(version_cmp("1.10.0", "1.9.0"), std::cmp::Ordering::Greater);
        assert_eq!(header_version("//! A thing.\n//! version: 1.4.2\n//!\n"), Some("1.4.2".into()));
        assert_eq!(header_version("fn x() {}\n//! version: 1.0.0"), None);
    }

    /// A registry on a port of its own: the catalog and one plugin, `files` by path.
    fn registry(files: Vec<(&'static str, Vec<u8>)>) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            for s in l.incoming().flatten() {
                let mut s = s;
                let mut buf = [0u8; 4096];
                let n = s.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                let (status, body) = files.iter().find(|(p, _)| *p == path).map(|(_, b)| ("200 OK", b.clone())).unwrap_or(("404 Not Found", b"no".to_vec()));
                let _ = write!(s, "HTTP/1.1 {}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", status, body.len());
                let _ = s.write_all(&body);
            }
        });
        format!("http://{}", addr)
    }

    #[test]
    fn installs_checks_and_updates() {
        let v1 = b"//! Says hi.\n//! version: 1.0.0\n//!\npub fn hi() -> String { \"hi\".into() }\n".to_vec();
        let index = |file: &[u8], version: &str| {
            serde_json::json!({ "format": 1, "plugins": [{ "name": "hi", "title": "Hi", "version": version, "summary": "Says hi.",
                "group": "Text", "sha256": hex_sha256(file), "file": "/plugins/files/hi.qwer", "example": "/plugins/files/hi.qwe", "files": [] }] })
                .to_string()
                .into_bytes()
        };
        let reg = registry(vec![("/plugins/index.json", index(&v1, "1.0.0")), ("/plugins/files/hi.qwer", v1.clone())]);
        let dir = std::env::temp_dir().join(format!("quillo-install-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let args = |extra: &[&str]| -> Vec<String> {
            let mut a: Vec<String> = vec!["--registry".into(), reg.clone(), "--dir".into(), dir.display().to_string()];
            a.extend(extra.iter().map(|s| s.to_string()));
            a
        };
        assert_eq!(main(&args(&["hi"])), 0);
        assert_eq!(std::fs::read(dir.join("hi.qwer")).unwrap(), v1);
        // Again: up to date. A missing one fails, the rest still goes.
        assert_eq!(main(&args(&["hi"])), 0);
        assert_eq!(main(&args(&["hi", "nope"])), 1);
        assert_eq!(main(&args(&["hi@2.0.0"])), 1);
        // Changed by hand at the same version: kept, unless --force.
        std::fs::write(dir.join("hi.qwer"), b"//! Says hi.\n//! version: 1.0.0\n// mine\n").unwrap();
        assert_eq!(main(&args(&["hi"])), 0);
        assert!(String::from_utf8_lossy(&std::fs::read(dir.join("hi.qwer")).unwrap()).contains("// mine"));
        assert_eq!(main(&args(&["hi", "--force"])), 0);
        assert_eq!(std::fs::read(dir.join("hi.qwer")).unwrap(), v1);

        // A catalog whose hash does not match the file: nothing written.
        let bad = registry(vec![("/plugins/index.json", index(b"other", "1.0.0")), ("/plugins/files/hi.qwer", v1.clone())]);
        let dir2 = dir.join("bad");
        assert_eq!(main(&["--registry".into(), bad, "--dir".into(), dir2.display().to_string(), "hi".into()]), 1);
        assert!(!dir2.join("hi.qwer").exists());

        // A newer version in the catalog: --update takes it.
        let v2 = String::from_utf8(v1.clone()).unwrap().replace("1.0.0", "1.1.0").into_bytes();
        let newer = registry(vec![("/plugins/index.json", index(&v2, "1.1.0")), ("/plugins/files/hi.qwer", v2.clone())]);
        assert_eq!(main(&["--registry".into(), newer, "--dir".into(), dir.display().to_string(), "--update".into()]), 0);
        assert_eq!(std::fs::read(dir.join("hi.qwer")).unwrap(), v2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
