//! `quillo install`: plugins from a catalog, here one on a port of the test's own.

mod support;

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};

/// A plugin as a file, with the version in its header.
fn plugin(version: &str) -> Vec<u8> {
    format!("//! Says hi.\n//! version: {}\n//!\npub fn hi() -> String {{ \"hi\".into() }}\n", version).into_bytes()
}

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{:02x}", b)).collect()
}

/// A catalog entry for `name`, its file at /plugins/files/<name>.qwer.
fn entry(name: &str, version: &str, file: &[u8]) -> serde_json::Value {
    serde_json::json!({
        "name": name, "title": name, "version": version, "summary": format!("The {} plugin.", name), "group": "Text",
        "sha256": sha(file), "file": format!("/plugins/files/{}.qwer", name), "example": "", "files": [],
    })
}

#[derive(Clone)]
struct Page {
    status: &'static str,
    /// What content-length says, when it is not the body's.
    declared: Option<usize>,
    body: Vec<u8>,
}

/// The catalog's server. It serves what it is given, and answers 404 to anything else.
struct Registry {
    url: String,
    pages: Arc<Mutex<HashMap<String, Page>>>,
}

fn answer(mut s: TcpStream, pages: &Mutex<HashMap<String, Page>>) {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        match s.read(&mut byte) {
            Ok(1) => head.push(byte[0]),
            _ => return,
        }
    }
    let head = String::from_utf8_lossy(&head).to_string();
    let path = head.split_whitespace().nth(1).unwrap_or("/");
    let missing = Page { status: "404 Not Found", declared: None, body: b"no".to_vec() };
    let p = pages.lock().unwrap().get(path).cloned().unwrap_or(missing);
    let _ = write!(s, "HTTP/1.1 {}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", p.status, p.declared.unwrap_or(p.body.len()));
    let _ = s.write_all(&p.body);
}

impl Registry {
    fn new() -> Registry {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        let pages: Arc<Mutex<HashMap<String, Page>>> = Default::default();
        let served = pages.clone();
        std::thread::spawn(move || {
            for s in l.incoming().flatten() {
                let served = served.clone();
                std::thread::spawn(move || answer(s, &served));
            }
        });
        Registry { url, pages }
    }

    fn put(&self, path: &str, body: impl Into<Vec<u8>>) {
        self.pages.lock().unwrap().insert(path.to_string(), Page { status: "200 OK", declared: None, body: body.into() });
    }

    /// A catalog of these entries, and each `(name, file)`'s file where its entry says.
    fn with(entries: &[serde_json::Value], files: &[(&str, &[u8])]) -> Registry {
        let r = Registry::new();
        r.catalog(entries);
        for (name, file) in files {
            r.put(&format!("/plugins/files/{}.qwer", name), file.to_vec());
        }
        r
    }

    fn catalog(&self, entries: &[serde_json::Value]) {
        self.put("/plugins/index.json", serde_json::json!({ "format": 1, "plugins": entries }).to_string());
    }

    /// One plugin, `hi` at 1.0.0.
    fn hi() -> Registry {
        let file = plugin("1.0.0");
        Registry::with(&[entry("hi", "1.0.0", &file)], &[("hi", &file)])
    }
}

/// `quillo install` here, this folder as its home, with a catalog that answers nothing unless
/// the test names one: it never reaches the real one.
fn command(dir: &Path, args: &[&str]) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_quillo"));
    c.arg("install").args(args).current_dir(dir).env("HOME", dir).env("QUILLO_REGISTRY", "http://127.0.0.1:9").stdin(Stdio::null());
    c
}

fn install(dir: &Path, args: &[&str]) -> Output {
    command(dir, args).output().unwrap()
}

/// `quillo install` from `r`.
fn from(r: &Registry, dir: &Path, args: &[&str]) -> Output {
    command(dir, args).env("QUILLO_REGISTRY", &r.url).output().unwrap()
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn dir(name: &str) -> std::path::PathBuf {
    support::temp_dir(&format!("cli-install-{}", name))
}

#[test]
fn help_is_not_an_error_and_a_wrong_option_is() {
    let d = dir("help");
    for flag in ["--help", "-h"] {
        let o = install(&d, &[flag]);
        assert!(o.status.success());
        assert!(format!("{}{}", out(&o), err(&o)).starts_with("usage:\n  quillo install <name>[@version]..."), "{}", err(&o));
    }
    let o = install(&d, &["--wat"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o).starts_with("quillo install: unknown option `--wat`\nusage:"), "{}", err(&o));
    let o = install(&d, &["--dir"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o).starts_with("quillo install: --dir takes a folder\n"), "{}", err(&o));
    let o = install(&d, &["--registry"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o).starts_with("quillo install: --registry takes a URL\n"), "{}", err(&o));
}

#[test]
fn nothing_named_asks_which() {
    let d = dir("which");
    let o = install(&d, &[]);
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o).starts_with("quillo install: which plugins? `quillo install csv qr`, or `quillo install --list`\nusage:"), "{}", err(&o));
}

#[test]
fn a_catalog_that_cannot_be_read_stops_it() {
    let d = dir("catalog");
    // No one there.
    let closed = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let url = format!("http://127.0.0.1:{}", closed);
    let o = install(&d, &["--registry", &url, "hi"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).starts_with(&format!("quillo install: {}/plugins/index.json: ", url)), "{}", err(&o));

    // Someone there, with no catalog.
    let r = Registry::new();
    let o = from(&r, &d, &["hi"]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(err(&o), format!("quillo install: {}/plugins/index.json answered 404\n", r.url));

    r.put("/plugins/index.json", "<html>not json</html>");
    let o = from(&r, &d, &["hi"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).starts_with(&format!("quillo install: {}/plugins/index.json: not a catalog (", r.url)), "{}", err(&o));

    r.put("/plugins/index.json", "{\"format\": 2, \"plugins\": []}");
    let o = from(&r, &d, &["hi"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("a catalog in a format this quillo does not read; a newer quillo may"), "{}", err(&o));

    r.put("/plugins/index.json", vec![b' '; (8 << 20) + 1]);
    let o = from(&r, &d, &["hi"]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(err(&o), format!("quillo install: {}/plugins/index.json is larger than 8 MB\n", r.url));
    assert!(std::fs::read_dir(&d).unwrap().next().is_none(), "nothing written");
}

#[test]
fn the_registry_may_be_named_each_way_there_is() {
    let d = dir("registry");
    let r = Registry::hi();
    let inline = format!("--registry={}/", r.url);
    let o = install(&d, &[&inline, "--list"]);
    assert!(o.status.success(), "{}", err(&o));
    assert!(out(&o).contains("hi"), "{}", out(&o));
    let o = from(&r, &d, &["--list"]);
    assert!(out(&o).contains("hi"), "{}", out(&o));
    let o = install(&d, &["--registry", &r.url, "-l"]);
    assert!(out(&o).contains("hi"), "{}", out(&o));
}

fn listed() -> Registry {
    let file = plugin("1.0.0");
    let mut img = entry("qr_code", "2.10.0", &file);
    img["group"] = "Images".into();
    img["summary"] = "Squares for phones.".into();
    let mut csv = entry("csv", "1.0.0", &file);
    csv["title"] = "Comma Separated".into();
    // Not one a program could install: skipped.
    let no_name = entry("", "1.0.0", &file);
    let mut no_file = entry("nofile", "1.0.0", &file);
    no_file["file"] = "".into();
    Registry::with(&[csv, entry("hi", "1.0.0", &file), img, no_name, no_file], &[])
}

#[test]
fn list_shows_each_group_with_its_plugins_in_order() {
    let d = dir("list");
    let o = from(&listed(), &d, &["--list"]);
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(
        out(&o),
        "\nImages\n  qr_code  2.10.0    Squares for phones.\n\nText\n  csv      1.0.0     The csv plugin.\n  hi       1.0.0     The hi plugin.\n\n3 plugins; `quillo install <name>` to install one\n"
    );
}

#[test]
fn list_narrows_to_the_words_given_in_any_field() {
    let d = dir("list-words");
    let r = listed();
    let o = from(&r, &d, &["--list", "QR"]);
    assert_eq!(out(&o), "\nImages\n  qr_code  2.10.0    Squares for phones.\n\n1 plugin matching qr; `quillo install <name>` to install one\n");
    // In the title, the summary, the group; every word must match.
    assert!(out(&from(&r, &d, &["--list", "comma"])).contains("csv"));
    assert!(out(&from(&r, &d, &["--list", "phones"])).contains("qr_code"));
    assert!(out(&from(&r, &d, &["--list", "text", "csv"])).ends_with("1 plugin matching text csv; `quillo install <name>` to install one\n"));
    let o = from(&r, &d, &["--list", "text", "squares"]);
    assert!(o.status.success());
    assert_eq!(out(&o), "\n0 plugins matching text squares; `quillo install <name>` to install one\n");
}

#[test]
fn list_of_a_catalog_with_no_plugins_says_none() {
    let d = dir("list-none");
    let r = Registry::new();
    r.put("/plugins/index.json", "{\"format\": 1}");
    assert_eq!(out(&from(&r, &d, &["--list"])), "\n0 plugins; `quillo install <name>` to install one\n");
}

#[test]
fn plugins_with_no_group_appear_with_the_rest() {
    let file = plugin("1.0.0");
    let mut loose = entry("loose", "1.0.0", &file);
    loose["group"] = "".into();
    let r = Registry::with(&[loose, entry("hi", "1.0.0", &file)], &[]);
    let d = dir("list-loose");
    let text = out(&from(&r, &d, &["--list"]));
    assert!(text.contains("  loose  1.0.0     The loose plugin.\n"), "{}", text);
    assert!(text.contains("\nText\n  hi     1.0.0"), "{}", text);
}

#[test]
fn plugins_with_no_group_are_listed_under_other() {
    let file = plugin("1.0.0");
    let mut loose = entry("loose", "1.0.0", &file);
    loose["group"] = "".into();
    let r = Registry::with(&[loose, entry("hi", "1.0.0", &file)], &[]);
    let d = dir("list-other");
    let text = out(&from(&r, &d, &["--list"]));
    assert!(text.contains("\nOther\n  loose"), "{}", text);
}

#[test]
fn a_plugin_is_installed_beside_the_program_and_said_how_to_use() {
    let d = dir("one");
    let o = from(&Registry::hi(), &d, &["hi"]);
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(std::fs::read(d.join("hi.qwer")).unwrap(), plugin("1.0.0"));
    assert!(out(&o).starts_with("installed hi 1.0.0 into ./hi.qwer\n\nUse it in a program:\n  use \"hi.qwer\"\n"), "{}", out(&o));
    assert!(!d.join("hi.qwer.part").exists());
}

#[test]
fn several_go_at_once_and_into_a_folder_made_for_them() {
    let d = dir("several");
    let (a, b) = (plugin("1.0.0"), plugin("2.0.0"));
    let r = Registry::with(&[entry("a_one", "1.0.0", &a), entry("b_two", "2.0.0", &b)], &[("a_one", &a), ("b_two", &b)]);
    let to = d.join("deep/down");
    let inline = format!("--dir={}", to.display());
    let o = from(&r, &d, &[&inline, "A-One.qwer", "b_two@v2.0.0"]);
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(std::fs::read(to.join("a_one.qwer")).unwrap(), a);
    assert_eq!(std::fs::read(to.join("b_two.qwer")).unwrap(), b);
    let text = out(&o);
    assert!(text.contains("Use them in a program:\n  use \"a_one.qwer\"\n  use \"b_two.qwer\"\n"), "{}", text);
}

#[test]
fn a_file_may_be_anywhere_the_catalog_says() {
    let d = dir("absolute");
    let file = plugin("1.0.0");
    let r = Registry::new();
    let mut e = entry("hi", "1.0.0", &file);
    e["file"] = format!("{}/elsewhere/hi.qwer", r.url).into();
    r.catalog(&[e]);
    r.put("/elsewhere/hi.qwer", file.clone());
    let o = from(&r, &d, &["hi"]);
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(std::fs::read(d.join("hi.qwer")).unwrap(), file);
}

#[test]
fn a_name_that_is_no_name_is_refused() {
    let d = dir("badname");
    let o = from(&Registry::hi(), &d, &["../etc"]);
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(err(&o), "quillo install: `../etc` is not a plugin name (letters, digits and _)\n");
    let o = from(&Registry::hi(), &d, &["@1.0.0"]);
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn a_misspelt_name_gets_the_nearest_ones_and_the_rest_still_go() {
    let d = dir("near");
    let file = plugin("1.0.0");
    let entries = [entry("hi", "1.0.0", &file), entry("csv_read", "1.0.0", &file), entry("csv_write", "1.0.0", &file)];
    let r = Registry::with(&entries, &[("hi", &file)]);
    let o = from(&r, &d, &["csv", "zzz", "hi"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("quillo install: no plugin `csv` (did you mean `csv_read`, `csv_write`?)\n"), "{}", err(&o));
    assert!(err(&o).contains("quillo install: no plugin `zzz` (`quillo install --list` shows them all)\n"), "{}", err(&o));
    assert!(d.join("hi.qwer").exists());
}

#[test]
fn a_version_the_catalog_does_not_have_is_refused() {
    let d = dir("version");
    let o = from(&Registry::hi(), &d, &["hi@2.0.0"]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(err(&o), "quillo install: hi@2.0.0 is not in the catalog: it has hi 1.0.0\n");
    assert!(!d.join("hi.qwer").exists());
}

#[test]
fn a_file_that_is_not_what_the_catalog_says_is_not_written() {
    let d = dir("mismatch");
    // Another hash than the file's.
    let r = Registry::with(&[entry("hi", "1.0.0", b"something else")], &[("hi", &plugin("1.0.0"))]);
    let o = from(&r, &d, &["hi"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).starts_with("quillo install: hi: the download's SHA-256 is "), "{}", err(&o));
    assert!(err(&o).trim_end().ends_with("; nothing written"), "{}", err(&o));

    // The right hash, another version in its header.
    let newer = plugin("1.1.0");
    let r = Registry::with(&[entry("hi", "1.0.0", &newer)], &[("hi", &newer)]);
    let o = from(&r, &d, &["hi"]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(err(&o), "quillo install: hi: the file says version 1.1.0, the catalog 1.0.0; nothing written\n");

    // No version at all.
    let bare = b"pub fn hi() {}\n".to_vec();
    let r = Registry::with(&[entry("hi", "1.0.0", &bare)], &[("hi", &bare)]);
    let o = from(&r, &d, &["hi"]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(err(&o), "quillo install: hi: the file says version none, the catalog 1.0.0; nothing written\n");
    assert!(!d.join("hi.qwer").exists());
}

#[test]
fn a_download_that_fails_is_reported_and_writes_nothing() {
    let d = dir("download");
    let file = plugin("1.0.0");
    let r = Registry::with(&[entry("hi", "1.0.0", &file)], &[]);
    let o = from(&r, &d, &["hi"]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(err(&o), format!("quillo install: {}/plugins/files/hi.qwer answered 404\n", r.url));

    // It stops halfway.
    r.pages.lock().unwrap().insert("/plugins/files/hi.qwer".into(), Page { status: "200 OK", declared: Some(file.len()), body: file[..10].to_vec() });
    let o = from(&r, &d, &["hi"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).starts_with(&format!("quillo install: {}/plugins/files/hi.qwer: ", r.url)), "{}", err(&o));
    assert!(!d.join("hi.qwer").exists());
}

#[test]
fn a_file_that_cannot_be_written_is_reported() {
    let d = dir("write");
    let r = Registry::hi();
    // Something in the way of the folder.
    std::fs::write(d.join("taken"), "a file").unwrap();
    let o = from(&r, &d, &["--dir", "taken/sub", "hi"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).starts_with("quillo install: taken/sub: "), "{}", err(&o));

    // Something in the way of the temporary file.
    std::fs::create_dir_all(d.join("a/hi.qwer.part")).unwrap();
    let o = from(&r, &d, &["--dir", "a", "hi"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).starts_with("quillo install: a/hi.qwer.part: "), "{}", err(&o));

    // Something in the way of the file itself: the temporary file is cleared up.
    std::fs::create_dir_all(d.join("b/hi.qwer/in-it")).unwrap();
    let o = from(&r, &d, &["--dir", "b", "hi"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).starts_with("quillo install: b/hi.qwer: "), "{}", err(&o));
    assert!(!d.join("b/hi.qwer.part").exists());
}

#[test]
fn what_is_here_already_is_kept_unless_older() {
    let d = dir("kept");
    let r = Registry::hi();
    assert!(from(&r, &d, &["hi"]).status.success());
    let again = from(&r, &d, &["hi"]);
    assert_eq!(out(&again), "hi 1.0.0 is up to date\n");

    // Changed by hand.
    let mine = "//! version: 1.0.0\n// mine\n";
    std::fs::write(d.join("hi.qwer"), mine).unwrap();
    let o = from(&r, &d, &["hi"]);
    assert!(o.status.success());
    assert_eq!(out(&o), "kept hi 1.0.0 here differs from the catalog's: changed by hand? --force replaces it\n");
    assert_eq!(std::fs::read_to_string(d.join("hi.qwer")).unwrap(), mine);

    // Newer than the catalog's.
    let newer = "//! version: 3.0.0\n";
    std::fs::write(d.join("hi.qwer"), newer).unwrap();
    let o = from(&r, &d, &["hi"]);
    assert!(o.status.success());
    assert_eq!(out(&o), "kept hi 3.0.0 here is newer than the catalog's 1.0.0; --force replaces it\n");
    assert_eq!(std::fs::read_to_string(d.join("hi.qwer")).unwrap(), newer);

    // --force takes the catalog's, and says what it was.
    let o = from(&r, &d, &["hi", "-f"]);
    assert_eq!(out(&o), "updated hi 3.0.0 → 1.0.0\n");
    assert_eq!(std::fs::read(d.join("hi.qwer")).unwrap(), plugin("1.0.0"));

    // A file with no version counts as older than any, and has none to tell.
    std::fs::write(d.join("hi.qwer"), "pub fn hi() {}\n").unwrap();
    assert_eq!(out(&from(&r, &d, &["hi"])), "updated hi ? → 1.0.0\n");
}

#[test]
fn update_takes_what_is_newer_for_the_plugins_here_only() {
    let d = dir("update");
    let (v2, other) = (plugin("1.10.0"), plugin("1.0.0"));
    let r = Registry::with(&[entry("hi", "1.10.0", &v2), entry("other", "1.0.0", &other)], &[("hi", &v2), ("other", &other)]);
    std::fs::write(d.join("hi.qwer"), plugin("1.9.0")).unwrap();
    std::fs::write(d.join("mine.qwer"), "pub fn mine() {}\n").unwrap();
    let o = from(&r, &d, &["--update"]);
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(out(&o), "updated hi 1.9.0 → 1.10.0\n");
    assert_eq!(std::fs::read(d.join("hi.qwer")).unwrap(), v2);
    assert_eq!(std::fs::read_to_string(d.join("mine.qwer")).unwrap(), "pub fn mine() {}\n", "not the catalog's: left alone");
    assert!(!d.join("other.qwer").exists(), "not here: not installed");
    assert_eq!(out(&from(&r, &d, &["-u"])), "hi 1.10.0 is up to date\n");
    // A plugin named along with it goes once.
    assert_eq!(out(&from(&r, &d, &["-u", "hi"])), "hi 1.10.0 is up to date\n");
}

#[test]
fn update_with_no_plugins_here_says_so() {
    let d = dir("update-none");
    let o = from(&Registry::hi(), &d, &["--update"]);
    assert!(o.status.success());
    assert_eq!(out(&o), "no plugins from the catalog in .\n");
    let o = from(&Registry::hi(), &d, &["--update", "--dir", "not-there"]);
    assert!(o.status.success());
    assert_eq!(out(&o), "no plugins from the catalog in not-there\n");
}

/// `hi`, with an example and files that it reads.
fn with_example() -> (Registry, Vec<u8>) {
    let file = plugin("1.0.0");
    let mut e = entry("hi", "1.0.0", &file);
    e["example"] = "/plugins/files/hi.qwe".into();
    e["files"] = serde_json::json!(["/plugins/files/words.txt", "", "/plugins/files/.hidden", "/plugins/files/"]);
    let r = Registry::with(&[e], &[("hi", &file)]);
    r.put("/plugins/files/hi.qwe", "use \"hi.qwer\"\nprint(hi())\n");
    r.put("/plugins/files/words.txt", "hello\n");
    (r, file)
}

#[test]
fn example_brings_the_example_and_the_files_it_reads() {
    let d = dir("example");
    let (r, _) = with_example();
    let o = from(&r, &d, &["hi", "--example"]);
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(std::fs::read_to_string(d.join("hi.qwe")).unwrap(), "use \"hi.qwer\"\nprint(hi())\n");
    assert_eq!(std::fs::read_to_string(d.join("words.txt")).unwrap(), "hello\n");
    assert!(!d.join(".hidden").exists(), "a name starting with a dot is not taken");
    assert!(out(&o).contains("  and ./hi.qwe\n  and ./words.txt\n"), "{}", out(&o));
}

#[test]
fn an_example_here_already_is_kept_unless_forced() {
    let d = dir("example-kept");
    let (r, _) = with_example();
    std::fs::write(d.join("hi.qwe"), "my own\n").unwrap();
    let o = from(&r, &d, &["hi", "--examples"]);
    assert!(o.status.success());
    assert!(out(&o).contains("  ./hi.qwe is already here: kept (--force replaces it)\n"), "{}", out(&o));
    assert_eq!(std::fs::read_to_string(d.join("hi.qwe")).unwrap(), "my own\n");
    // Up to date itself, the plugin's example still comes when forced.
    let o = from(&r, &d, &["hi", "--example", "--force"]);
    assert!(out(&o).starts_with("hi 1.0.0 is up to date\n  and ./hi.qwe\n"), "{}", out(&o));
    assert_eq!(std::fs::read_to_string(d.join("hi.qwe")).unwrap(), "use \"hi.qwer\"\nprint(hi())\n");
}

#[test]
fn an_example_that_cannot_be_had_fails_but_keeps_the_plugin() {
    let d = dir("example-fails");
    let (r, file) = with_example();
    r.pages.lock().unwrap().remove("/plugins/files/hi.qwe");
    let o = from(&r, &d, &["hi", "--example"]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(err(&o), format!("quillo install: {}/plugins/files/hi.qwe answered 404\n", r.url));
    assert_eq!(std::fs::read(d.join("hi.qwer")).unwrap(), file);
}

#[test]
fn it_says_when_cargo_is_needed_and_not_to_be_found() {
    let d = dir("cargo");
    let (empty, tools) = (d.join("empty"), d.join("tools"));
    std::fs::create_dir_all(&empty).unwrap();
    std::fs::create_dir_all(&tools).unwrap();
    std::fs::write(tools.join("cargo"), "").unwrap();
    let r = Registry::hi();
    let said = |path: &Path, cargo: Option<&str>| {
        let _ = std::fs::remove_file(d.join("hi.qwer"));
        let mut c = command(&d, &["hi"]);
        c.env("QUILLO_REGISTRY", &r.url).env("PATH", path).env_remove("CARGO");
        if let Some(v) = cargo {
            c.env("CARGO", v);
        }
        let o = c.output().unwrap();
        assert!(o.status.success(), "{}", err(&o));
        out(&o)
    };
    let hint = "The first run builds each plugin with cargo, which is not on this PATH: install Rust from https://rustup.rs";
    assert!(said(&empty, None).contains(hint));
    assert!(!said(&tools, None).contains(hint));
    assert!(!said(&empty, Some("/somewhere/cargo")).contains(hint));
}
