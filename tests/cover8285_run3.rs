//! Coverage of the lines of src/head.rs and src/db.rs that the full run at 8b84078 left unrun, or
//! that the speed round 2 of 3117dea added (coverage of feedback82-85; branch feedback82-85):
//! `page_head`'s reading of a head as the browser's tokenizer reads it (FEEDBACK84.md, Q-1320): the
//! four named references the standard reads without their `;` in an attribute's value, and a tag
//! whose attribute's name is followed by spaces and then its `>` or a `/`; and `conn_of`, which
//! tells a connection's map, looked for its "$db" first (FEEDBACK83.md, Q-1300). Each page's head
//! as parse5 7.3.0 reads it (scripting on and off) was checked to hold what the test says.

mod support;

use quillo::head::{apply, Head, KEYS};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

// ---- page_head, as tests/cover8285_head5.rs reads a head ------------------------------------------

/// A head that says `pairs`, each at depth 0 in no component, in their order.
fn said_head(pairs: &[(&str, &str)]) -> Head {
    let mut h = Head::default();
    for (k, v) in pairs {
        h.say(KEYS.iter().position(|x| x == k).unwrap(), 0, 0, v.to_string());
        h.called();
    }
    h
}

/// `page` with `h`'s tags in its head.
fn with_head(page: &str, h: &Head) -> String {
    let mut b = page.as_bytes().to_vec();
    apply(&mut b, h);
    String::from_utf8(b).unwrap()
}

/// A document whose head is `head`.
fn doc(head: &str) -> String {
    format!("<!doctype html><html><head>{head}</head><body>b</body></html>")
}

/// What `page_head({ title: "T" })` writes.
const TT: &str = "<title>T</title><meta property=\"og:title\" content=\"T\">";

/// What `page_head({ description: "D" })` writes.
const DD: &str = "<meta name=\"description\" content=\"D\"><meta property=\"og:description\" content=\"D\">";

/// What `page_head({ canonical: "https://x.example/a" })` writes.
const CC: &str = "<link rel=\"canonical\" href=\"https://x.example/a\"><meta property=\"og:url\" content=\"https://x.example/a\">";

/// `&amp`, `&lt`, `&gt` and `&quot` (and their capitals) without their `;`, in a meta's name or
/// property, are the character they name where no letter, digit or `=` follows, as the browser
/// reads them: `description&amp` is `description&`, `description&lt` (at the value's end)
/// `description<`, `og:description&QUOT` `og:description"`, none a description, so each is left
/// beside `page_head`'s. Where a letter or `=` follows they are as written (`&ampdescription`,
/// `&amp=description`, `descr&GTiption`), no description either. A reference read as nothing
/// would have cut the first three (src/head.rs char_ref, FEEDBACK84.md Q-1320; coverage of
/// feedback82-85; branch feedback82-85).
#[test]
fn a_named_reference_without_its_semicolon_is_read_as_the_browser_reads_it_in_a_metas_name() {
    let h = said_head(&[("description", "D")]);
    for head in [
        "<meta name=\"description&amp\" content=\"K\">",
        "<meta name=\"description&lt\" content=\"K\">",
        "<meta property=\"og:description&QUOT\" content=\"K\">",
        "<meta name=\"description&GT \" content=\"K\">",
        "<meta name=\"&ampdescription\" content=\"K\">",
        "<meta name=\"&amp=description\" content=\"K\">",
        "<meta name=\"descr&GTiption\" content=\"K\">",
    ] {
        assert_eq!(with_head(&doc(head), &h), doc(&format!("{head}{DD}")), "{head}");
    }
    // The same references with their `;` are what they were (`ASCII_REFS`): no description either.
    let head = "<meta name=\"description&amp;\" content=\"K\">";
    assert_eq!(with_head(&doc(head), &h), doc(&format!("{head}{DD}")));
}

/// In a link's `rel`, such a reference is a token of its own where a space or a tab follows it,
/// by reference or written (`&amp canonical`, `&LT&#9;canonical`, `&quot&#32;canonical`, `canonical
/// &lt`): the link is a canonical, and is cut. Followed by a letter or `=` (`&ampcanonical`,
/// `&AMP=canonical`), or ending the token (`canonical&gt`), it is part of a token that is no
/// `canonical`, and the link is left (src/head.rs char_ref and has_token, FEEDBACK84.md Q-1320;
/// coverage of feedback82-85; branch feedback82-85).
#[test]
fn a_named_reference_without_its_semicolon_splits_a_rel_only_where_a_space_follows() {
    let h = said_head(&[("canonical", "https://x.example/a")]);
    for head in [
        "<link rel=\"&amp canonical\" href=\"/old\">",
        "<link rel=\"&LT&#9;canonical\" href=\"/old\">",
        "<link rel=\"&quot&#32;canonical\" href=\"/old\">",
        "<link rel=\"canonical &lt\" href=\"/old\">",
    ] {
        assert_eq!(with_head(&doc(head), &h), doc(CC), "{head}");
    }
    for head in ["<link rel=\"&ampcanonical\" href=\"/k\">", "<link rel=\"&AMP=canonical\" href=\"/k\">", "<link rel=\"canonical&gt\" href=\"/k\">"] {
        assert_eq!(with_head(&doc(head), &h), doc(&format!("{head}{CC}")), "{head}");
    }
}

/// A tag whose last attribute's name is followed by spaces and then its `>` (`async >`) or a `/`
/// (`async />`) ends there, in a start tag and in an end tag (`</title lang >`): the meta and the
/// title are cut whole, and nothing after them is (src/head.rs tag_close, FEEDBACK84.md Q-1320;
/// coverage of feedback82-85; branch feedback82-85).
#[test]
fn a_tag_whose_attributes_name_is_followed_by_spaces_ends_at_its_own_close() {
    let h = said_head(&[("title", "T"), ("description", "D")]);
    for head in [
        "<meta name=description content=OLD async >",
        "<meta name=description content=OLD async \t/>",
        "<meta name=description content=OLD async / >",
        "<title lang >Old</title lang >",
        "<title lang >Old</title><meta name=description content=OLD async >",
    ] {
        assert_eq!(with_head(&doc(&format!("{head}<link rel=icon href=/i.png>")), &h), doc(&format!("<link rel=icon href=/i.png>{TT}{DD}")), "{head}");
    }
}

/// In a template of the head, `<svg x />` (its attribute's name, a space, then `/>`) is a whole
/// svg, as the browser closes it, so the `<title>` after it is the page's own, whose text holds
/// the `</template>` in it, and the template ends at the second: the head's title after it is cut.
/// `<svg x >` and `<svg x / >` (a space between `/` and `>`) leave the svg open: the `<title>` is
/// the svg's, the first `</template>` ends the template, the `<p>` after it ends the head, and the
/// tags go before that `<p>`, the title after it left as the body's (src/head.rs tag_close and walk,
/// FEEDBACK84.md Q-1320; coverage of feedback82-85; branch feedback82-85).
#[test]
fn an_svgs_self_closing_tag_after_an_attributes_name_is_read_as_the_browser_reads_it() {
    let h = said_head(&[("title", "T")]);
    let rest = "<title></template><p>t</p></title></template><title>Old</title>";
    let closed = format!("<template><svg x />{rest}");
    assert_eq!(with_head(&doc(&closed), &h), doc(&format!("<template><svg x /><title></template><p>t</p></title></template>{TT}")));
    for open in ["<svg x >", "<svg x / >"] {
        let head = format!("<template>{open}{rest}");
        assert_eq!(
            with_head(&doc(&head), &h),
            format!("<!doctype html><html><head><template>{open}<title></template>{TT}<p>t</p></title></template><title>Old</title></head><body>b</body></html>"),
            "{open}"
        );
    }
}

// ---- conn_of: a map called as a connection -----------------------------------------------------

/// A test's folder, removed when the test ends.
struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `quillo run main.qwe` in `d`, ended after a minute: its exit code and what it wrote.
fn run(d: &Path) -> (Option<i32>, String) {
    let log = d.join("cmd.log");
    let file = std::fs::File::create(&log).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", "main.qwe"])
        .current_dir(d)
        .env_remove("QUILLO_POLICY_PATH")
        .env_remove("QUILLO_DB")
        .env_remove("CARGO_TARGET_DIR")
        .stdin(Stdio::null())
        .stdout(file.try_clone().unwrap())
        .stderr(file)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let code = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s.code();
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    (code, std::fs::read_to_string(&log).unwrap_or_default())
}

/// A method called on a map is a connection's where the map has a "$db" that is an int and the
/// name is one of `db`'s builtins: it runs on that connection, the one the number says (`a` or
/// `b`), whatever map it is in; one the program has not opened (-1, 2, the last int) is refused.
/// A map with no "$db", or one that is no int (`"1"`, `1.0`), is no connection: `db.one` runs on
/// the program's own database, which has no `t`. A function of the map's own, or a builtin not of
/// `db`'s, is called as it is (src/db.rs conn_of, the speed round 2 of 0.8.11, FEEDBACK83.md
/// Q-1300; coverage of feedback82-85; branch feedback82-85).
#[test]
fn a_map_is_called_as_a_connection_only_where_its_db_is_an_int_and_the_name_is_dbs() {
    let d = Dir(support::temp_dir("cov8285r3-conn"));
    std::fs::create_dir_all(d.0.join("data")).unwrap();
    std::fs::write(
        d.0.join("main.qwe"),
        r#"let a = db.connect("data/a.db")
let b = db.connect("data/b.db")
a.exec("create table t (x int)")
a.exec("insert into t values (7)")
b.exec("create table t (x int)")
b.exec("insert into t values (8)")
print(a["$db"], b["$db"])
print(a.one("select x from t"), b.one("select x from t"))
let on_b = { "$db": b["$db"], one: db.one, query: db.query }
print(on_b.one("select x from t"), on_b.query("select x + 1 as y from t"))
let on_a = { "$db": a["$db"], one: db.one }
print(on_a.one("select x from t"))
print({ "$db": "1", one: db.one }.one("select x from t"))
print({ "$db": 1.0, one: db.one }.one("select x from t"))
print({ one: db.one }.one("select x from t"))
print({ "$db": -1, one: db.one }.one("select x from t"))
print({ "$db": 2, one: db.one }.one("select x from t"))
print({ "$db": 9223372036854775807, one: db.one }.one("select x from t"))
print({ "$db": b["$db"], one: (sql) => "its own: " + sql }.one("select x from t"))
print({ "$db": b["$db"], one: upper }.one("select x from t"))
"#,
    )
    .unwrap();
    let (code, out) = run(&d.0);
    assert_eq!(code, Some(0), "{out}");
    assert_eq!(
        out,
        "0 1\n{ x: 7 } { x: 8 }\n{ x: 8 } [{ y: 9 }]\n{ x: 7 }\n\
         err(\"db: no such table: t\")\nerr(\"db: no such table: t\")\nerr(\"db: no such table: t\")\n\
         err(\"db: this connection is not one of this program's\")\nerr(\"db: this connection is not one of this program's\")\nerr(\"db: this connection is not one of this program's\")\n\
         its own: select x from t\nSELECT X FROM T\n"
    );
}

// Fix round 6

// ---- one folder, however it is named ------------------------------------------------------------

use support::{port_in_log, send, Guard};

/// `d`, a folder as the system names it, by its other name through the data volume's firmlink on
/// macOS (`/System/Volumes/Data/private/var/...` for `/private/var/...`), looked at to be the same
/// folder; None where it has none (Linux, as on CI).
#[cfg(unix)]
fn other_name(d: &Path) -> Option<PathBuf> {
    use std::os::unix::fs::MetadataExt;
    let long = Path::new("/System/Volumes/Data").join(d.strip_prefix("/").ok()?);
    let (a, b) = (std::fs::metadata(d).ok()?, std::fs::metadata(&long).ok()?);
    (a.dev() == b.dev() && a.ino() == b.ino()).then_some(long)
}

/// A test's folder (removed at its end) as the system names it, and the same folder by its other
/// name; None, said, where it has none, and the test has nothing to look at.
#[cfg(unix)]
fn named_twice(name: &str) -> Option<(Dir, PathBuf)> {
    let d = Dir(support::temp_dir(name).canonicalize().unwrap());
    let Some(long) = other_name(&d.0) else {
        eprintln!("skipped: {} has no other name here (no /System/Volumes/Data firmlink, as on Linux)", d.0.display());
        return None;
    };
    Some((d, long))
}

/// `quillo args` in `cwd`, with `vars`, its output in `log`, ended after `secs`: its exit code and
/// what it wrote. No policy, database or cargo of the test's own reaches it.
fn quillo_at(cwd: &Path, args: &[&str], vars: &[(&str, &str)], log: &Path, secs: u64) -> (Option<i32>, String) {
    let file = std::fs::File::create(log).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(args)
        .current_dir(cwd)
        .env_remove("QUILLO_POLICY_PATH")
        .env_remove("QUILLO_DB")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTFLAGS")
        .envs(vars.iter().copied())
        .stdin(Stdio::null())
        .stdout(file.try_clone().unwrap())
        .stderr(file)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(secs);
    let code = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s.code();
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    (code, std::fs::read_to_string(log).unwrap_or_default())
}

/// `quillo run args --port 0` in `cwd`, its log in `log`, stopped when the guard is dropped, and
/// its port.
fn serve_at(cwd: &Path, args: &[&str], log: &Path) -> (Guard, u16) {
    let file = std::fs::File::create(log).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .args(args)
        .args(["--port", "0"])
        .current_dir(cwd)
        .env_remove("QUILLO_POLICY_PATH")
        .env_remove("QUILLO_DB")
        .env("QUILLO_SECRET", "test secret for the tests, 32 bytes or more")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(file)
        .spawn()
        .unwrap();
    let mut g = Guard::new(child);
    let port = port_in_log(&mut g.0, log, Duration::from_secs(60));
    (g, port)
}

/// A pinned build is refused a temporary folder the program can reach, whatever name either is
/// given by. On macOS `/System/Volumes/Data/private/...` is the folder `/private/...`, and an
/// `--allow-path` or `[fs] paths` in the one form with `TMPDIR` in the other (given so, or through a
/// link) built in a folder the program could write: refused now, as the same form is, and so the
/// folder itself (the limit), the data volume's own root (which only the long name names), the
/// program's folder named long, a folder of a unicode name given decomposed, and `--allow-path /`.
/// A temporary folder beside the one named, whose name starts with its name, is not in it: there,
/// started from `/` (a unit's working folder, which holds every place and is let pass), the
/// plugin builds (src/native.rs private_dir, FEEDBACK82.md Q-1285, found by the coverage tests;
/// branch feedback82-85)
#[cfg(unix)]
#[test]
fn a_pinned_build_is_refused_a_temporary_folder_the_program_reaches_by_its_other_name() {
    use sha2::Digest;
    let Some((d, long)) = named_twice("cov8285r3-pin-names") else { return };
    let root = d.0.clone();
    let lg = |p: &Path| long.join(p.strip_prefix(&root).unwrap());
    let (app, o, s, u, beside) = (root.join("app"), root.join("other"), root.join("shared"), root.join("\u{f6}rt"), root.join("other-2"));
    for f in [&app, &o, &s, &u, &beside, &root.join("cwd")] {
        std::fs::create_dir_all(f.join("tmp")).unwrap();
    }
    std::os::unix::fs::symlink(lg(&o.join("tmp")), root.join("via")).unwrap();
    let src = "//! version: 1.0.0\n//! effects: none\npub fn hi() -> String { \"hi from the pinned source\".into() }\n";
    std::fs::write(app.join("hi.qwer"), src).unwrap();
    std::fs::write(app.join("main.qwe"), "use \"hi.qwer\"\nprint(hi())\n").unwrap();
    let hash: String = sha2::Sha256::digest(src.as_bytes()).iter().map(|b| format!("{:02x}", b)).collect();
    let pin = format!("[plugins]\nallow = [\"hi@sha256:{}\"]\n", hash);
    std::fs::write(app.join("pin.toml"), &pin).unwrap();
    std::fs::write(app.join("pin-fs.toml"), format!("{}[fs]\npaths = [\"{}\"]\n", pin, lg(&s).display())).unwrap();
    let record = root.join("record").display().to_string();
    let log = root.join("run.log");
    let run = |cwd: &Path, args: &[&str], tmp: &Path, secs: u64| quillo_at(cwd, args, &[("TMPDIR", &tmp.display().to_string()), ("QUILLO_BUILT_RECORD_DIR", &record)], &log, secs);
    let none_left = |tmp: &Path| std::fs::read_dir(tmp).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().starts_with("quillo-pinned-"));
    let (main, policy) = (app.join("main.qwe").display().to_string(), app.join("pin.toml").display().to_string());
    let pinned = |extra: &[&str]| -> Vec<String> { ["run", &main, "--no-check", "--policy", &policy].iter().chain(extra).map(|a| a.to_string()).collect() };
    let refused = |cwd: &Path, args: Vec<String>, tmp: &Path, shown: &Path, inside: &Path| {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let (code, out) = run(cwd, &args, tmp, 120);
        assert_eq!(code, Some(1), "{}: {}", tmp.display(), out);
        let said = format!("a pinned plugin is built apart, and the temporary folder {} is inside {}, which the program can reach", shown.display(), inside.display());
        assert!(out.contains(&said), "{} {:?}: {}", tmp.display(), args, out);
        assert!(!out.contains("building ") && none_left(tmp), "{}", out);
    };
    let (lo, o_s) = (lg(&o).display().to_string(), o.display().to_string());
    // `--allow-path` long, TMPDIR short; the other way round; the folder itself; through a link
    // that leads to the long name.
    refused(&app, pinned(&["--allow-path", &lo]), &o.join("tmp"), &o.join("tmp"), &lg(&o));
    refused(&app, pinned(&["--allow-path", &o_s]), &lg(&o.join("tmp")), &lg(&o.join("tmp")), &o);
    refused(&app, pinned(&["--allow-path", &lo]), &o, &o, &lg(&o));
    refused(&app, pinned(&["--allow-path", &o_s]), &root.join("via"), &lg(&o.join("tmp")), &o);
    // The data volume's root, which holds `/private` by its long name only; and `/`. Each holds the
    // cargo home too, which is judged first (native.rs toolchain_reached, fix round 7): refused
    // naming the root either way.
    for root_named in ["/System/Volumes/Data", "/"] {
        let args = pinned(&["--allow-path", root_named]);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let (code, out) = run(&app, &args, &o.join("tmp"), 120);
        assert_eq!(code, Some(1), "{}: {}", root_named, out);
        assert!(out.contains(&format!(" is inside {}, which the program can reach", root_named)), "{}: {}", root_named, out);
        assert!(!out.contains("building ") && none_left(&o.join("tmp")), "{}", out);
    }
    // A unicode name, given decomposed in the long form: the folder as the system keeps it.
    let decomposed = long.join("o\u{308}rt").display().to_string();
    refused(&app, pinned(&["--allow-path", &decomposed]), &u.join("tmp"), &u.join("tmp"), &lg(&u));
    // `[fs] paths` in the long form.
    let fs_pin = app.join("pin-fs.toml").display().to_string();
    refused(&app, ["run", &main, "--no-check", "--policy", &fs_pin].map(String::from).to_vec(), &s.join("tmp"), &s.join("tmp"), &lg(&s));
    // The program's folder named long, started from elsewhere.
    let (lmain, lpin) = (lg(&app.join("main.qwe")).display().to_string(), lg(&app.join("pin.toml")).display().to_string());
    refused(&root.join("cwd"), ["run", &lmain, "--no-check", "--policy", &lpin].map(String::from).to_vec(), &app.join("tmp"), &app.join("tmp"), &lg(&app));
    // Beside it: built, and its private folder gone once loaded.
    let (code, out) = run(Path::new("/"), &["run", &main, "--no-check", "--policy", &policy, "--allow-path", &lo], &beside.join("tmp"), 600);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains("hi from the pinned source") && none_left(&beside.join("tmp")), "{}", out);
}

/// A folder `--allow-path` names is reached by either of its names: named long
/// (`/System/Volumes/Data/...`) for a program named short, `read_file("../shared/a.txt")` was
/// refused with "`--allow-path ../shared` lets the program read it"; named short for a program
/// named long, the same. Read and written by `..` and by each absolute name, in a folder of a
/// unicode name given decomposed; a folder beside it whose name starts with its name, and one not
/// named, stay out, with the way out said (src/builtins.rs allowed_path, FEEDBACK85.md Q-1340, found
/// by the coverage tests; branch feedback82-85)
#[cfg(unix)]
#[test]
fn a_folder_allow_path_names_is_reached_by_either_of_its_names() {
    let Some((d, long)) = named_twice("cov8285r3-allow-names") else { return };
    let root = d.0.clone();
    let lg = |p: &Path| long.join(p.strip_prefix(&root).unwrap());
    let (app, shared, u) = (root.join("app"), root.join("shared"), root.join("\u{f6}rt"));
    for f in [&app, &shared, &u, &root.join("shared-2"), &root.join("secret"), &root.join("cwd")] {
        std::fs::create_dir_all(f).unwrap();
    }
    std::fs::write(shared.join("a.txt"), "from shared").unwrap();
    std::fs::write(u.join("\u{fc}.txt"), "unicode").unwrap();
    std::fs::write(root.join("shared-2/c.txt"), "beside").unwrap();
    std::fs::write(root.join("secret/b.txt"), "secret").unwrap();
    let main = format!(
        "print(read_file(\"../shared/a.txt\"))\nprint(read_file(\"{s}/a.txt\"))\nprint(read_file(\"{ls}/a.txt\"))\nprint(read_file(\"../\u{f6}rt/\u{fc}.txt\"))\n\
         print(write_file(\"../shared/made.txt\", \"made\") | is_err)\nprint(read_file(\"{ls}/made.txt\"))\nprint(read_file(\"../shared-2/c.txt\"))\nprint(read_file(\"../secret/b.txt\"))\n",
        s = shared.display(),
        ls = lg(&shared).display()
    );
    std::fs::write(app.join("main.qwe"), main).unwrap();
    let want = "from shared\nfrom shared\nfrom shared\nunicode\nfalse\nmade\n\
                err(\"read_file: path must stay inside the program's folder: ../shared-2/c.txt; `--allow-path ../shared-2` (on `quillo run` or `quillo mcp`) lets the program read it\")\n\
                err(\"read_file: path must stay inside the program's folder: ../secret/b.txt; `--allow-path ../secret` (on `quillo run` or `quillo mcp`) lets the program read it\")\n";
    let log = root.join("run.log");
    let (ls, us) = (lg(&shared).display().to_string(), long.join("o\u{308}rt").display().to_string());
    let (code, out) = quillo_at(&app, &["run", "main.qwe", "--allow-path", &ls, "--allow-path", &us], &[], &log, 60);
    assert_eq!((code, out.as_str()), (Some(0), want), "allowed by the long name");
    std::fs::remove_file(shared.join("made.txt")).unwrap();
    let lmain = lg(&app.join("main.qwe")).display().to_string();
    let (ss, su) = (shared.display().to_string(), u.display().to_string());
    let (code, out) = quillo_at(&root.join("cwd"), &["run", &lmain, "--allow-path", &ss, "--allow-path", &su], &[], &log, 60);
    assert_eq!((code, out.as_str()), (Some(0), want), "the program named long");
    std::fs::remove_file(shared.join("made.txt")).unwrap();
    // Under a policy whose `[fs] paths` names the folder above by the one name, `--allow-path` by
    // the other is inside it (src/policy.rs allows_path); a folder outside it is still refused.
    for (ceiling, given) in [(lg(&root), ss.as_str()), (root.clone(), ls.as_str())] {
        std::fs::write(app.join("p.toml"), format!("[fs]\npaths = [\"{}\"]\n", ceiling.display())).unwrap();
        let (code, out) = quillo_at(&app, &["run", "main.qwe", "--policy", "p.toml", "--allow-path", given, "--allow-path", &su], &[], &log, 60);
        assert_eq!(code, Some(0), "{}", out);
        assert!(out.ends_with(want), "[fs] paths {}: {}", ceiling.display(), out);
        std::fs::remove_file(shared.join("made.txt")).unwrap();
    }
    std::fs::write(app.join("p.toml"), format!("[fs]\npaths = [\"{}\"]\n", lg(&shared).display())).unwrap();
    let (code, out) = quillo_at(&app, &["run", "main.qwe", "--policy", "p.toml", "--allow-path", &su], &[], &log, 60);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains(&format!("--allow-path {} is not inside the folders", u.display())), "{}", out);
}

/// The program's own files are its own by either name: with `--allow-path` naming the folder above
/// the program's by the one name, and the program's folder named by the other, writing its main
/// file, its `.env` or the database it has open by either name is refused, and so is deleting the
/// folder that holds the database (which was deleted by the name the database was not opened by),
/// or the program's folder itself, refused as by the name it was started by (it was deleted by
/// the other: its loaded files and its database were named by the one). A file of its own folder
/// that is none of these is written (src/builtins.rs own_file and full_is_base, FEEDBACK85.md
/// Q-1340, found by the coverage tests; branch feedback82-85)
#[cfg(unix)]
#[test]
fn the_programs_own_files_are_its_own_by_either_name() {
    let Some((d, long)) = named_twice("cov8285r3-own-names") else { return };
    let root = d.0.clone();
    let lg = |p: &Path| long.join(p.strip_prefix(&root).unwrap());
    let app = root.join("app");
    std::fs::create_dir_all(app.join("data")).unwrap();
    std::fs::create_dir_all(root.join("cwd")).unwrap();
    std::fs::write(app.join(".env"), "X=1\n").unwrap();
    let (a, la) = (app.display().to_string(), lg(&app).display().to_string());
    let mut main = String::from("db.open(\"data/app.db\")\ndb.exec(\"create table if not exists t (x int)\")\n");
    let writes = [format!("{a}/main.qwe"), format!("{la}/main.qwe"), format!("{a}/.env"), format!("{la}/.env"), format!("{a}/data/app.db"), format!("{la}/data/app.db"), format!("{la}/data/app.db-journal")];
    let deletes = [format!("{a}/data"), format!("{la}/data"), a.clone(), la.clone()];
    for p in &writes {
        main.push_str(&format!("print(write_file(\"{}\", \"x\"))\n", p));
    }
    for p in &deletes {
        main.push_str(&format!("print(delete_file(\"{}\"))\n", p));
    }
    main.push_str(&format!("print(write_file(\"{la}/notes.txt\", \"kept\") | is_err)\nprint(read_file(\"notes.txt\"))\n"));
    std::fs::write(app.join("main.qwe"), &main).unwrap();
    let whys = ["it is one of the program's own files", "it is one of the program's own files", "it is the program's settings (.env)", "it is the program's settings (.env)", "it is the database the program has open", "it is the database the program has open", "it is the database the program has open"];
    let mut want = String::new();
    for (p, why) in writes.iter().zip(whys) {
        want.push_str(&format!("err(\"write_file: will not write {}: {}\")\n", p, why));
    }
    for p in &deletes[..2] {
        want.push_str(&format!("err(\"delete_file: will not delete {}: it holds the database the program has open\")\n", p));
    }
    // The program's folder itself, by either name, as by the name it was started by.
    for p in &deletes[2..] {
        want.push_str(&format!("err(\"delete_file: path must be inside the program's folder: {}\")\n", p));
    }
    want.push_str("false\nkept\n");
    let log = root.join("run.log");
    let (lr, r) = (long.display().to_string(), root.display().to_string());
    // The program named short, the folder above it long; then the program long, the folder short.
    let (code, out) = quillo_at(&app, &["run", "main.qwe", "--allow-path", &lr], &[], &log, 60);
    assert_eq!((code, out.as_str()), (Some(0), want.as_str()), "the program named short");
    assert_eq!(std::fs::read_to_string(app.join(".env")).unwrap(), "X=1\n");
    std::fs::remove_file(app.join("notes.txt")).unwrap();
    let lmain = lg(&app.join("main.qwe")).display().to_string();
    let (code, out) = quillo_at(&root.join("cwd"), &["run", &lmain, "--allow-path", &r], &[], &log, 60);
    assert_eq!((code, out.as_str()), (Some(0), want.as_str()), "the program named long");
    assert!(app.join("main.qwe").exists() && app.join("data/app.db").exists());
}

/// `db.migrate()` applies the program's migrations whichever name its folder is given by: `quillo
/// run /System/Volumes/Data/.../main.qwe` refused each file as "not the one judged", the system
/// naming the file it opened `/private/...` (a regression from 0.8.10, held or not); and so from the
/// folder entered by its long name, and through a `migrations` link to the long name of a folder
/// named short. Each applied once (src/migrate.rs read_judged, FEEDBACK83.md Q-1300, found by the
/// coverage tests; branch feedback82-85)
#[cfg(unix)]
#[test]
fn db_migrate_applies_a_program_named_by_either_name() {
    let Some((d, long)) = named_twice("cov8285r3-migrate-names") else { return };
    let root = d.0.clone();
    let lg = |p: &Path| long.join(p.strip_prefix(&root).unwrap());
    let app = root.join("app");
    for f in [app.join("migrations"), app.join("data"), root.join("real-migrations"), root.join("cwd")] {
        std::fs::create_dir_all(f).unwrap();
    }
    std::fs::write(app.join("migrations/001_t.sql"), "create table t (x int);\n").unwrap();
    std::fs::write(app.join("main.qwe"), "db.open(\"data/app.db\")\nprint(db.migrate())\nprint(db.migrate())\n").unwrap();
    let want = "quillo: applied migration 001_t.sql\n[\"001_t.sql\"]\n[]\n";
    let log = root.join("run.log");
    let fresh = || {
        for f in ["data/app.db", "data/app.db-wal", "data/app.db-shm", "data/app.db-journal"] {
            let _ = std::fs::remove_file(app.join(f));
        }
    };
    let lmain = lg(&app.join("main.qwe")).display().to_string();
    let (code, out) = quillo_at(&root.join("cwd"), &["run", &lmain], &[], &log, 60);
    assert_eq!((code, out.as_str()), (Some(0), want), "the program named long");
    fresh();
    let (code, out) = quillo_at(&lg(&app), &["run", "main.qwe"], &[], &log, 60);
    assert_eq!((code, out.as_str()), (Some(0), want), "its folder entered by its long name");
    fresh();
    // Its migrations through a link to their folder's long name.
    std::fs::rename(app.join("migrations/001_t.sql"), root.join("real-migrations/001_t.sql")).unwrap();
    std::fs::remove_dir(app.join("migrations")).unwrap();
    std::os::unix::fs::symlink(lg(&root.join("real-migrations")), app.join("migrations")).unwrap();
    let (code, out) = quillo_at(&app, &["run", "main.qwe", "--allow-path", &lg(&root.join("real-migrations")).display().to_string()], &[], &log, 60);
    assert_eq!((code, out.as_str()), (Some(0), want), "through a link to the long name");
}

/// `file()` and `files()` serve through a link into a folder `--allow-path` names by its other
/// name: `public/s -> ../../shared/web` with `--allow-path /System/Volumes/Data/.../shared` was a
/// 404, the log saying the link led "outside"; and a link written to the long name, a program
/// named long, a file of a unicode name, a large file (judged as it is sent). What `files()` keeps
/// to itself (dot files, `.qwe`) stays a 404 there, and a folder beside it is not let out
/// (src/builtins.rs led_out, allowed_dir, hidden_target and open_hosted, FEEDBACK85.md Q-1340,
/// found by the coverage tests; branch feedback82-85)
#[cfg(unix)]
#[test]
fn files_through_a_link_reach_a_folder_allow_path_names_by_either_name() {
    let Some((d, long)) = named_twice("cov8285r3-files-names") else { return };
    let root = d.0.clone();
    let lg = |p: &Path| long.join(p.strip_prefix(&root).unwrap());
    let (app, web) = (root.join("app"), root.join("shared/web"));
    for f in [app.join("public"), web.clone(), root.join("shared-2"), root.join("cwd")] {
        std::fs::create_dir_all(f).unwrap();
    }
    std::fs::write(web.join("x.js"), "x").unwrap();
    std::fs::write(web.join("\u{f1}.js"), "unicode").unwrap();
    std::fs::write(web.join(".env"), "SECRET=1").unwrap();
    std::fs::write(web.join("lib.qwe"), "let a = 1").unwrap();
    std::fs::write(web.join("big.bin"), vec![b'b'; 1_500_000]).unwrap();
    std::fs::write(root.join("shared-2/y.js"), "y").unwrap();
    std::os::unix::fs::symlink("../../shared/web", app.join("public/s")).unwrap();
    std::os::unix::fs::symlink(lg(&web), app.join("public/l")).unwrap();
    std::os::unix::fs::symlink("../../shared-2", app.join("public/b")).unwrap();
    std::fs::write(
        app.join("main.qwe"),
        "route GET /s/*path = files(\"public/s\", path)\nroute GET /l/*path = files(\"public/l\", path)\nroute GET /b/*path = files(\"public/b\", path)\nroute GET /one = file(\"public/s/x.js\")\nroute GET /lone = file(\"public/l/x.js\")\n",
    )
    .unwrap();
    let look = |port: u16, how: &str| {
        for (path, body) in [("/s/x.js", "x"), ("/l/x.js", "x"), ("/one", "x"), ("/lone", "x"), ("/s/%C3%B1.js", "unicode"), ("/l/%C3%B1.js", "unicode")] {
            let a = send(port, "GET", path, &[], "");
            assert_eq!((a.status, a.body.as_str()), (200, body), "{} {}", how, path);
        }
        let big = send(port, "GET", "/s/big.bin", &[], "");
        assert_eq!((big.status, big.body.len()), (200, 1_500_000), "{} big", how);
        for kept in ["/s/.env", "/l/.env", "/s/lib.qwe", "/l/lib.qwe", "/b/y.js"] {
            assert_eq!(send(port, "GET", kept, &[], "").status, 404, "{} {}", how, kept);
        }
    };
    let (ls, ss) = (lg(&root.join("shared")).display().to_string(), root.join("shared").display().to_string());
    let (g, port) = serve_at(&app, &["main.qwe", "--allow-path", &ls], &root.join("a.log"));
    look(port, "allowed by the long name");
    drop(g);
    let lmain = lg(&app.join("main.qwe")).display().to_string();
    let (g, port) = serve_at(&root.join("cwd"), &[&lmain, "--allow-path", &ss], &root.join("b.log"));
    look(port, "the program named long");
    drop(g);
    // Only the folder beside it was said to lead out.
    for log in ["a.log", "b.log"] {
        let said = std::fs::read_to_string(root.join(log)).unwrap();
        assert!(said.contains("leads out of the program's folder through a link") && said.contains("shared-2"), "{}: {}", log, said);
        assert!(!said.contains("shared/web"), "{}: {}", log, said);
    }
}

/// What a firmlink tests above, where there is none (Linux too): `--allow-path /` lets the program
/// reach every folder, the temporary one too, so a pinned build is refused, naming `/` (it was let
/// pass, as the folder Quillo runs in is); and the program's folder is its folder by any path, so
/// `delete_file("../app")` under `--allow-path ..` is refused as `delete_file(".")` is (src/native.rs
/// private_dir, FEEDBACK82.md Q-1285; src/builtins.rs full_is_base, FEEDBACK85.md Q-1340; found by
/// the coverage tests; branch feedback82-85)
#[cfg(unix)]
#[test]
fn the_whole_disk_named_and_the_programs_folder_by_another_path_are_judged_by_the_folder() {
    use sha2::Digest;
    let d = Dir(support::temp_dir("cov8285r3-root-reach").canonicalize().unwrap());
    let (app, tmp) = (d.0.join("app"), d.0.join("tmp"));
    std::fs::create_dir_all(&app).unwrap();
    std::fs::create_dir_all(&tmp).unwrap();
    let src = "//! version: 1.0.0\n//! effects: none\npub fn hi() -> String { \"hi\".into() }\n";
    std::fs::write(app.join("hi.qwer"), src).unwrap();
    std::fs::write(app.join("main.qwe"), "use \"hi.qwer\"\nprint(hi())\n").unwrap();
    let hash: String = sha2::Sha256::digest(src.as_bytes()).iter().map(|b| format!("{:02x}", b)).collect();
    std::fs::write(app.join("pin.toml"), format!("[plugins]\nallow = [\"hi@sha256:{}\"]\n", hash)).unwrap();
    let t = tmp.display().to_string();
    let (code, out) = quillo_at(&app, &["run", "main.qwe", "--no-check", "--policy", "pin.toml", "--allow-path", "/"], &[("TMPDIR", &t)], &d.0.join("a.log"), 120);
    assert_eq!(code, Some(1), "{}", out);
    // The temporary folder's, or (where `/` holds the cargo home too, judged first: native.rs
    // toolchain_reached, fix round 7) the cargo home's.
    assert!(out.contains(" is inside /, which the program can reach") && !out.contains("building "), "{}", out);
    assert_eq!(std::fs::read_dir(&tmp).unwrap().count(), 0, "a private folder was made");
    std::fs::write(app.join("del.qwe"), "print(delete_file(\"../app\"))\nprint(delete_file(\".\"))\nprint(delete_file(\"../app/del.qwe\"))\n").unwrap();
    let (code, out) = quillo_at(&app, &["run", "del.qwe", "--allow-path", ".."], &[], &d.0.join("b.log"), 60);
    assert_eq!(code, Some(0), "{}", out);
    assert_eq!(
        out,
        "err(\"delete_file: path must be inside the program's folder: ../app\")\nerr(\"delete_file: path must be inside the program's folder: .\")\nerr(\"delete_file: will not delete ../app/del.qwe: it is one of the program's own files\")\n"
    );
    assert!(app.join("main.qwe").exists());
}

/// A plugin a provider's policy pins by its source's SHA-256, with no quillo.confirmed, that builds
/// another file in is refused before anything is built: `global_asm!(".include \"/x/evil.s\"")`
/// was built, the assembler reading evil.s, which neither the hash nor rustc's list of what it read
/// covers, and its code ran (42); so `include!` by its other spelling, `std::include!`. The private
/// folder is never made (src/native.rs build_private, FEEDBACK82.md Q-1285 and FEEDBACK83.md Q-1305,
/// found by the coverage tests of 3117dea; branch feedback82-85)
#[cfg(unix)]
#[test]
fn a_pinned_plugin_that_builds_another_file_in_is_refused_without_a_quillo_confirmed() {
    use sha2::Digest;
    let d = Dir(support::temp_dir("cov8285r3-pin-asm").canonicalize().unwrap());
    let (app, tmp) = (d.0.join("app"), d.0.join("tmp"));
    std::fs::create_dir_all(&app).unwrap();
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(d.0.join("evil.s"), ".globl _quillo_evil\n.globl quillo_evil\n.p2align 2\n_quillo_evil:\nquillo_evil:\n    mov x0, #42\n    ret\n").unwrap();
    std::fs::write(d.0.join("other.rs"), "pub fn other() -> i64 { 42 }\n").unwrap();
    let evil = d.0.join("evil.s").display().to_string();
    let other = d.0.join("other.rs").display().to_string();
    for (src, how) in [
        (format!("//! version: 1.0.0\n//! effects: none\nstd::arch::global_asm!(\".include \\\"{}\\\"\");\nextern \"C\" {{ fn quillo_evil() -> i64; }}\npub fn hi() -> i64 {{ unsafe {{ quillo_evil() }} }}\n", evil), "global_asm"),
        (format!("//! version: 1.0.0\n//! effects: none\nstd::include!(\"{}\");\npub fn hi() -> i64 {{ other() }}\n", other), "include"),
    ] {
        std::fs::write(app.join("hi.qwer"), &src).unwrap();
        std::fs::write(app.join("main.qwe"), "use \"hi.qwer\"\nprint(hi())\n").unwrap();
        let hash: String = sha2::Sha256::digest(src.as_bytes()).iter().map(|b| format!("{:02x}", b)).collect();
        std::fs::write(app.join("pin.toml"), format!("[plugins]\nallow = [\"hi@sha256:{}\"]\n", hash)).unwrap();
        let (t, record) = (tmp.display().to_string(), d.0.join("record").display().to_string());
        let (code, out) = quillo_at(&app, &["run", "main.qwe", "--no-check", "--policy", "pin.toml"], &[("TMPDIR", &t), ("QUILLO_BUILT_RECORD_DIR", &record)], &d.0.join("run.log"), 120);
        assert_eq!(code, Some(1), "{}: {}", how, out);
        assert!(out.contains("builds another file into it") && out.contains("which the SHA-256 its source is held to would not cover, so it does not load"), "{}: {}", how, out);
        assert!(out.contains(how), "{}: {}", how, out);
        assert!(!out.contains("building ") && !out.lines().any(|l| l.trim() == "42"), "{}: {}", how, out);
        assert_eq!(std::fs::read_dir(&tmp).unwrap().count(), 0, "{}: a private folder was made", how);
        assert!(!app.join(".quillo/native").exists(), "{}", how);
    }
}

// Fix round 7

/// A sandbox profile (written in `dir`) under which nothing can be deleted but a SQLite database's
/// journal, and nothing written in the home folders: a `delete_file` that is not refused fails
/// there, and deletes nothing. None, said, where `sandbox-exec` is not there or does not hold.
#[cfg(target_os = "macos")]
fn no_unlink(dir: &Path) -> Option<PathBuf> {
    let profile = dir.join("nounlink.sb");
    std::fs::write(
        &profile,
        "(version 1)\n(allow default)\n(deny file-write-unlink)\n(allow file-write-unlink (regex #\"\\.db-(journal|wal|shm)$\"))\n\
         (deny file-write* (subpath \"/Users\") (subpath \"/System/Volumes/Data/Users\") (subpath \"/Applications\") (subpath \"/System/Volumes/Data/Applications\") (subpath \"/Library\") (subpath \"/System/Volumes/Data/Library\"))\n",
    )
    .unwrap();
    let probe = dir.join("sandbox-probe");
    std::fs::create_dir_all(&probe).unwrap();
    let held = Command::new("/usr/bin/sandbox-exec").arg("-f").arg(&profile).arg("/bin/rmdir").arg(&probe).stderr(Stdio::null()).status().is_ok_and(|s| !s.success()) && probe.exists();
    if !held {
        eprintln!("skipped: sandbox-exec is not there or lets a folder be deleted");
        return None;
    }
    Some(profile)
}

/// `quillo args` under the sandbox `profile`, as `quillo_at` runs it.
#[cfg(target_os = "macos")]
fn sandboxed_at(cwd: &Path, profile: &Path, args: &[&str], vars: &[(&str, &str)], log: &Path, secs: u64) -> (Option<i32>, String) {
    let file = std::fs::File::create(log).unwrap();
    let mut child = Command::new("/usr/bin/sandbox-exec")
        .arg("-f")
        .arg(profile)
        .arg(env!("CARGO_BIN_EXE_quillo"))
        .args(args)
        .current_dir(cwd)
        .env_remove("QUILLO_POLICY_PATH")
        .env_remove("QUILLO_DB")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTFLAGS")
        .envs(vars.iter().copied())
        .stdin(Stdio::null())
        .stdout(file.try_clone().unwrap())
        .stderr(file)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(secs);
    let code = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s.code();
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    (code, std::fs::read_to_string(log).unwrap_or_default())
}

/// A folder that holds the program's files or a database it has open is its own by the folder
/// itself, whatever name it is given. Started by the long name under `--allow-path
/// /System/Volumes/Data`, `delete_file("/System/Volumes/Data")` went on to `remove_dir_all` (0.8.10
/// refused it); under `--allow-path /`, the folder above the program's and its database's folder
/// named `/.nofollow/...`, and a folder holding a database opened by `/.nofollow/...` named
/// plainly (a unicode name) or long. A folder holding none of them is let through, to the
/// deletion, which the sandbox refuses: run under it, nothing is deleted whatever the binary does
/// (src/builtins.rs own_file and own_inside, src/statics.rs holds_any and sys_form, FEEDBACK85.md
/// Q-1340, found by the review of 3e793e3; branch feedback82-85)
#[cfg(target_os = "macos")]
#[test]
fn a_folder_that_holds_the_programs_files_or_database_is_its_own_by_the_folder_itself() {
    let Some((d, long)) = named_twice("cov8285r3-hold-names") else { return };
    let Some(profile) = no_unlink(&d.0) else { return };
    let root = d.0.clone();
    let lg = |p: &Path| long.join(p.strip_prefix(&root).unwrap());
    let (app, other, ord) = (root.join("app"), root.join("other"), root.join("\u{f6}rdner"));
    for f in [app.join("data"), other.clone(), ord.clone(), root.join("cwd")] {
        std::fs::create_dir_all(f).unwrap();
    }
    std::fs::write(other.join("x.txt"), "x").unwrap();
    std::fs::write(app.join("a.qwe"), "print(delete_file(\"/System/Volumes/Data\"))\n").unwrap();
    let la = lg(&app.join("a.qwe")).display().to_string();
    let (code, out) = sandboxed_at(&root.join("cwd"), &profile, &["run", &la, "--allow-path", "/System/Volumes/Data"], &[], &root.join("a.log"), 60);
    assert_eq!((code, out.as_str()), (Some(0), "err(\"delete_file: will not delete /System/Volumes/Data: it is one of the program's own files\")\n"));
    let nf = |p: &Path| format!("/.nofollow{}", p.display());
    let deletes = [nf(&root), nf(&app.join("data")), ord.display().to_string(), lg(&ord).display().to_string(), other.display().to_string()];
    let mut main = format!(
        "db.open(\"data/app.db\")\ndb.exec(\"create table if not exists t (x int)\")\nlet c = db.connect(\"{}\")\nc.exec(\"create table if not exists t (x int)\")\n",
        nf(&ord.join("x.db"))
    );
    for p in &deletes {
        main.push_str(&format!("print(delete_file(\"{}\"))\n", p));
    }
    std::fs::write(app.join("b.qwe"), main).unwrap();
    let want = format!(
        "err(\"delete_file: will not delete {}: it is one of the program's own files\")\n\
         err(\"delete_file: will not delete {}: it holds the database the program has open\")\n\
         err(\"delete_file: will not delete {}: it holds the database the program has open\")\n\
         err(\"delete_file: will not delete {}: it holds the database the program has open\")\n\
         err(\"delete_file: {}: Operation not permitted (os error 1)\")\n",
        deletes[0], deletes[1], deletes[2], deletes[3], deletes[4]
    );
    let (code, out) = sandboxed_at(&app, &profile, &["run", "b.qwe", "--allow-path", "/"], &[], &root.join("b.log"), 60);
    assert_eq!((code, out.as_str()), (Some(0), want.as_str()));
    assert!(app.join("data/app.db").exists() && ord.join("x.db").exists() && other.join("x.txt").exists());
}

/// A program started as `/.nofollow/.../main.qwe`, or given `--allow-path /.nofollow/...`, reaches
/// what the plain name reaches: realpath keeps the prefix, which the system drops, and every path
/// out of the folder was refused, a link into the folder allowed was a 404, and its migrations
/// were "not the one judged". A folder not named stays out, and under `/.nofollow` a link is not
/// followed, by the system's own rule (src/statics.rs sys_form_of,
/// src/migrate.rs read_judged, FEEDBACK85.md Q-1340 and FEEDBACK83.md Q-1300, found by the review
/// of 3e793e3; branch feedback82-85)
#[cfg(target_os = "macos")]
#[test]
fn a_program_or_allow_path_named_under_nofollow_reaches_what_its_plain_name_reaches() {
    let d = Dir(support::temp_dir("cov8285r3-nofollow").canonicalize().unwrap());
    let root = d.0.clone();
    if !Path::new("/.nofollow").join(root.strip_prefix("/").unwrap()).is_dir() {
        eprintln!("skipped: no /.nofollow here");
        return;
    }
    let nf = |p: &Path| format!("/.nofollow{}", p.display());
    let (app, shared) = (root.join("app"), root.join("shared"));
    for f in [app.join("migrations"), app.join("data"), app.join("public"), shared.clone(), root.join("secret"), root.join("cwd")] {
        std::fs::create_dir_all(f).unwrap();
    }
    std::fs::write(app.join("a.txt"), "mine").unwrap();
    std::fs::write(shared.join("a.txt"), "from shared").unwrap();
    std::fs::write(root.join("secret/b.txt"), "secret").unwrap();
    std::fs::write(app.join("migrations/001_t.sql"), "create table t (x int);\n").unwrap();
    std::os::unix::fs::symlink("../../shared", app.join("public/s")).unwrap();
    std::fs::write(
        app.join("main.qwe"),
        "print(read_file(\"a.txt\"))\nprint(read_file(\"../shared/a.txt\"))\nprint(files(\"public/s\", \"a.txt\"))\nprint(is_err(read_file(\"../secret/b.txt\")))\ndb.open(\"data/app.db\")\nprint(db.migrate())\n",
    )
    .unwrap();
    let want = "mine\nfrom shared\n<response 200>\ntrue\nquillo: applied migration 001_t.sql\n[\"001_t.sql\"]\n";
    let fresh = || {
        for f in ["data/app.db", "data/app.db-wal", "data/app.db-shm", "data/app.db-journal"] {
            let _ = std::fs::remove_file(app.join(f));
        }
    };
    let (ns, ss) = (nf(&shared), shared.display().to_string());
    let (code, out) = quillo_at(&app, &["run", "main.qwe", "--allow-path", &ns], &[], &root.join("a.log"), 60);
    assert_eq!((code, out.as_str()), (Some(0), want), "--allow-path under /.nofollow");
    fresh();
    let nmain = nf(&app.join("main.qwe"));
    // Under `/.nofollow` the system follows no link: `public/s` is not followed, by the system
    // itself (ELOOP), a 404, as `cat` of it fails.
    let (code, out) = quillo_at(&root.join("cwd"), &["run", &nmain, "--allow-path", &ss], &[], &root.join("b.log"), 60);
    assert_eq!((code, out.as_str()), (Some(0), want.replace("<response 200>", "<response 404>").as_str()), "the program under /.nofollow");
}

/// Where /usr/share/firmlinks cannot be read (a sandbox), the firmlinks are still known: each of
/// the known list is taken where a look finds its two names one folder. Without them, a pinned
/// build ran in a temporary folder under `--allow-path /System/Volumes/Data` (which holds it only by
/// the long name), and `--allow-path` by the long name refused `../shared/a.txt` (src/statics.rs
/// firmlinks, FEEDBACK82.md Q-1285 and FEEDBACK85.md Q-1340, found by the review of 3e793e3; branch
/// feedback82-85)
#[cfg(target_os = "macos")]
#[test]
fn the_firmlinks_are_known_where_the_systems_list_cannot_be_read() {
    use sha2::Digest;
    let Some((d, long)) = named_twice("cov8285r3-firm-unread") else { return };
    let root = d.0.clone();
    let profile = root.join("nofirm.sb");
    std::fs::write(&profile, "(version 1)\n(allow default)\n(deny file-read* (literal \"/usr/share/firmlinks\"))\n").unwrap();
    let denied = Command::new("/usr/bin/sandbox-exec").arg("-f").arg(&profile).args(["/bin/cat", "/usr/share/firmlinks"]).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| !s.success());
    if !denied {
        eprintln!("skipped: sandbox-exec is not there or lets /usr/share/firmlinks be read");
        return;
    }
    let (app, tmp, shared) = (root.join("app"), root.join("tmp"), root.join("shared"));
    for f in [&app, &tmp, &shared] {
        std::fs::create_dir_all(f).unwrap();
    }
    std::fs::write(shared.join("a.txt"), "from shared").unwrap();
    let src = "//! version: 1.0.0\n//! effects: none\npub fn hi() -> String { \"hi\".into() }\n";
    std::fs::write(app.join("hi.qwer"), src).unwrap();
    std::fs::write(app.join("main.qwe"), "use \"hi.qwer\"\nprint(hi())\n").unwrap();
    std::fs::write(app.join("r.qwe"), "print(read_file(\"../shared/a.txt\"))\n").unwrap();
    let hash: String = sha2::Sha256::digest(src.as_bytes()).iter().map(|b| format!("{:02x}", b)).collect();
    std::fs::write(app.join("pin.toml"), format!("[plugins]\nallow = [\"hi@sha256:{}\"]\n", hash)).unwrap();
    let (t, record) = (tmp.display().to_string(), root.join("record").display().to_string());
    let (code, out) = sandboxed_at(&app, &profile, &["run", "main.qwe", "--no-check", "--policy", "pin.toml", "--allow-path", "/System/Volumes/Data"], &[("TMPDIR", &t), ("QUILLO_BUILT_RECORD_DIR", &record)], &root.join("a.log"), 120);
    // The temporary folder's, or the cargo home's (judged first: native.rs toolchain_reached, fix
    // round 7); each by the same firmlinks.
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains(" is inside /System/Volumes/Data, which the program can reach") && !out.contains("building "), "{}", out);
    assert_eq!(std::fs::read_dir(&tmp).unwrap().count(), 0, "a private folder was made");
    let ls = long.join("shared").display().to_string();
    let (code, out) = sandboxed_at(&app, &profile, &["run", "r.qwe", "--allow-path", &ls], &[], &root.join("b.log"), 60);
    assert_eq!((code, out.as_str()), (Some(0), "from shared\n"));
}

// Fix round 7 (F3)

/// A pinned build is refused where the program can reach what the build runs or reads besides
/// its own folder: its cargo home (`CARGO_HOME`, made, or not made yet, which the program could
/// make), rustup's home (`RUSTUP_HOME`), the `cargo` on `PATH`, and a folder of `PATH` with no
/// cargo in it (whose `cc` the link runs), each in a folder `--allow-path` gives the program. The start says which, and where, and to pin the library instead; nothing is
/// built and no private folder is made. With that folder allowed and none of them in it, the plugin
/// builds and loads (src/native.rs toolchain_reached, FEEDBACK82.md Q-1285, found by the coverage
/// tests; branch feedback82-85).
#[cfg(unix)]
#[test]
fn a_pinned_build_is_refused_a_cargo_home_toolchain_or_cargo_the_program_reaches() {
    use sha2::Digest;
    use std::os::unix::fs::PermissionsExt;
    let d = Dir(support::temp_dir("cov8285r3-pin-toolchain").canonicalize().unwrap());
    let (app, tmp, open) = (d.0.join("app"), d.0.join("tmp"), d.0.join("open"));
    for f in [&app, &tmp, &open.join("bin"), &open.join("cargo-home"), &open.join("tools")] {
        std::fs::create_dir_all(f).unwrap();
    }
    let cargo = open.join("bin/cargo");
    std::fs::write(&cargo, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(&cargo, std::fs::Permissions::from_mode(0o755)).unwrap();
    let src = "//! version: 1.0.0\n//! effects: none\npub fn hi() -> String { \"from the pin\".into() }\n";
    std::fs::write(app.join("hi.qwer"), src).unwrap();
    std::fs::write(app.join("main.qwe"), "use \"hi.qwer\"\nprint(hi())\n").unwrap();
    let hash: String = sha2::Sha256::digest(src.as_bytes()).iter().map(|b| format!("{:02x}", b)).collect();
    std::fs::write(app.join("pin.toml"), format!("[plugins]\nallow = [\"hi@sha256:{}\"]\n", hash)).unwrap();
    let (t, record, o) = (tmp.display().to_string(), d.0.join("record").display().to_string(), open.display().to_string());
    let path = format!("{}:{}", open.join("bin").display(), std::env::var("PATH").unwrap_or_default());
    let tools = format!("{}:{}", std::env::var("PATH").unwrap_or_default(), open.join("tools").display());
    let args = ["run", "main.qwe", "--no-check", "--policy", "pin.toml", "--allow-path", &o];
    let cases = [
        ("its cargo home (CARGO_HOME)", "CARGO_HOME", open.join("cargo-home").display().to_string()),
        ("its cargo home (CARGO_HOME)", "CARGO_HOME", open.join("not-made/yet").display().to_string()),
        ("rustup's home (RUSTUP_HOME)", "RUSTUP_HOME", open.join("rustup").display().to_string()),
        ("the cargo it runs", "PATH", path.clone()),
        ("a folder of its PATH, whose programs it runs", "PATH", tools.clone()),
    ];
    for (what, var, value) in &cases {
        let (code, out) = quillo_at(&app, &args, &[("TMPDIR", &t), ("QUILLO_BUILT_RECORD_DIR", &record), (var, value)], &d.0.join("run.log"), 120);
        assert_eq!(code, Some(1), "{}: {}", what, out);
        let shown = match (*var, what.starts_with("a folder")) {
            ("PATH", true) => open.join("tools").display().to_string(),
            ("PATH", false) => cargo.display().to_string(),
            _ => value.clone(),
        };
        assert!(
            out.contains(&format!("is held to its source's SHA-256 and built here, and the build would use {}, {}, which is inside {}, which the program can reach and so could steer what is built: it does not load (have the policy pin its library by its own SHA-256 instead, as .quillo/plugins.lock has it)", what, shown, o)),
            "{}: {}",
            what,
            out
        );
        assert!(!out.contains("building ") && !out.contains("from the pin"), "{}: {}", what, out);
        assert_eq!(std::fs::read_dir(&tmp).unwrap().count(), 0, "{}: a private folder was made", what);
    }
    let (code, out) = quillo_at(&app, &args, &[("TMPDIR", &t), ("QUILLO_BUILT_RECORD_DIR", &record)], &d.0.join("run.log"), 300);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.ends_with("from the pin\n"), "{}", out);
}

// Fix round 8

/// A folder that is the program's (its main file and a used one under `lib/`), in a test's
/// folder that is removed at its end, and the no-unlink sandbox profile; None, said, where the
/// sandbox is not there.
#[cfg(target_os = "macos")]
fn sandboxed_app(name: &str) -> Option<(Dir, PathBuf, PathBuf)> {
    let d = Dir(support::temp_dir(name).canonicalize().unwrap());
    let profile = no_unlink(&d.0)?;
    let app = d.0.join("app");
    std::fs::create_dir_all(app.join("lib")).unwrap();
    std::fs::write(app.join("lib/x.qwe"), "let lib_value = 1\n").unwrap();
    Some((d, app, profile))
}

/// Judging a journal's name looks at its database without opening it: `spool/q` a FIFO (or a link
/// to one outside), `write_file("spool/q-wal")` and `delete_file("spool")` opened `q` to read its
/// first bytes, and waited for a writer, under the interpreter's lock, for ever: a FIFO is no
/// database. Run with a deadline, under the no-unlink sandbox (src/builtins.rs own_file and
/// own_inside, FEEDBACK85.md Q-1340, found by the review of b7f7c26; branch feedback82-85)
#[cfg(target_os = "macos")]
#[test]
fn a_fifo_beside_a_journals_name_is_no_database_and_is_never_opened() {
    let Some((d, app, profile)) = sandboxed_app("cov8285r3-fifo") else { return };
    for f in [app.join("spool"), app.join("unpacked"), d.0.join("outside")] {
        std::fs::create_dir_all(f).unwrap();
    }
    for fifo in [app.join("spool/q"), d.0.join("outside/fifo")] {
        assert!(Command::new("mkfifo").arg(&fifo).status().unwrap().success());
    }
    std::os::unix::fs::symlink(d.0.join("outside/fifo"), app.join("unpacked/q")).unwrap();
    for j in ["spool/q-wal", "unpacked/q-wal"] {
        std::fs::write(app.join(j), "j").unwrap();
    }
    std::fs::write(
        app.join("main.qwe"),
        "print(write_file(\"spool/q-wal\", \"x\") | is_err)\nprint(write_file(\"unpacked/q-shm\", \"x\") | is_err)\nprint(delete_file(\"spool\"))\nprint(delete_file(\"unpacked\"))\n",
    )
    .unwrap();
    let (code, out) = sandboxed_at(&app, &profile, &["run", "main.qwe"], &[], &d.0.join("a.log"), 30);
    assert_eq!(
        (code, out.as_str()),
        (Some(0), "false\nfalse\nerr(\"delete_file: spool: Operation not permitted (os error 1)\")\nerr(\"delete_file: unpacked: Operation not permitted (os error 1)\")\n"),
        "it waited on the FIFO"
    );
}

/// The program's `.quillo` folders (beside its main file and beside each file it loaded: built
/// plugins, their lock, the session key) are its own: written, by any name (case, a link, a
/// hard link, not made yet), or deleted, they were ordinary files, and a program or an upload's
/// name could put another library in for the next start. Another project's `.quillo`, and a name
/// that only begins so, are files like others (src/builtins.rs own_file, FEEDBACK85.md Q-1340 as
/// FEEDBACK30.md Q-361 closed it for main.qwe, found by the review of b7f7c26; branch feedback82-85)
#[cfg(target_os = "macos")]
#[test]
fn the_programs_quillo_folders_are_its_own_by_any_name() {
    let Some((d, app, profile)) = sandboxed_app("cov8285r3-dotquillo") else { return };
    for f in [app.join(".quillo/native"), app.join("lib/.quillo/native"), app.join("other/.quillo")] {
        std::fs::create_dir_all(f).unwrap();
    }
    for (f, text) in [(".quillo/native/libhi-0123.dylib", "lib"), (".quillo/plugins.lock", "lock"), (".quillo/.quillo-secret", "key"), ("lib/.quillo/native/liby.dylib", "lib")] {
        std::fs::write(app.join(f), text).unwrap();
    }
    std::os::unix::fs::symlink(".quillo", app.join("ql")).unwrap();
    std::fs::hard_link(app.join(".quillo/plugins.lock"), app.join("hl.bin")).unwrap();
    // Fix round 10 decided: a `.quillo` anywhere is refused by its name (`other/.quillo`, which
    // was a file like others), and a second hard link is judged where Quillo reads it (a lock of
    // two names is not read: `a_library_or_lock_planted_in_quillo_gives_nothing`), not here.
    let writes = [".quillo/plugins.lock", ".QUILLO/native/libhi-0123.dylib", ".quillo/native/new.dylib", "lib/.quillo/native/liby.dylib", "ql/plugins.lock", ".quillo", "other/.quillo/x.txt"];
    let mut main = String::from("use \"lib/x.qwe\"\n");
    for w in writes {
        main.push_str(&format!("print(write_file(\"{}\", \"x\"))\n", w));
    }
    main.push_str("print(delete_file(\".quillo\"))\nprint(delete_file(\"lib/.quillo/native\"))\nprint(delete_file(\"ql\"))\n");
    main.push_str("print(write_file(\".quillo-notes.txt\", \"ok\") | is_err)\nprint(write_file(\"hl.bin\", \"x\") | is_err)\n");
    std::fs::write(app.join("main.qwe"), &main).unwrap();
    let why = "it is in the program's .quillo folder, which holds its built plugins, their lock and its keys";
    let mut want = String::new();
    for w in writes {
        want.push_str(&format!("err(\"write_file: will not write {}: {}\")\n", w, why));
    }
    for p in [".quillo", "lib/.quillo/native", "ql"] {
        want.push_str(&format!("err(\"delete_file: will not delete {}: {}\")\n", p, why));
    }
    want.push_str("false\nfalse\n");
    let (code, out) = sandboxed_at(&app, &profile, &["run", "main.qwe"], &[], &d.0.join("a.log"), 60);
    assert_eq!((code, out.as_str()), (Some(0), want.as_str()));
    assert_eq!(std::fs::read_to_string(app.join(".quillo/native/libhi-0123.dylib")).unwrap(), "lib");
}

/// Deleting a folder judges a link in it by what it leads to, as naming the link does: a link
/// the program's code was used through (`lib/util.qwe -> ../vendor/util.qwe`), the file
/// `--env-file` names (`config/app.env` a link), and the database it opened through a link
/// (`data/app.db -> ../store/app.db`) went with their folders, and the next start would have read
/// what was put in their place. A folder whose links lead to nothing of the program's is let
/// through to the deletion, which the sandbox refuses (src/builtins.rs own_inside, FEEDBACK85.md
/// Q-1340, found by the review of b7f7c26; branch feedback82-85)
#[cfg(target_os = "macos")]
#[test]
fn deleting_a_folder_judges_its_links_by_what_they_lead_to() {
    let Some((d, app, profile)) = sandboxed_app("cov8285r3-links-in") else { return };
    for f in ["vendor", "uses", "secrets", "config", "store", "data", "plain", "elsewhere"] {
        std::fs::create_dir_all(app.join(f)).unwrap();
    }
    std::fs::write(app.join("vendor/util.qwe"), "let util_value = 2\n").unwrap();
    std::os::unix::fs::symlink("../vendor/util.qwe", app.join("uses/util.qwe")).unwrap();
    std::fs::write(app.join("secrets/app.env"), "X=1\n").unwrap();
    std::os::unix::fs::symlink("../secrets/app.env", app.join("config/app.env")).unwrap();
    std::os::unix::fs::symlink("../store/app.db", app.join("data/app.db")).unwrap();
    std::fs::write(app.join("elsewhere/n.txt"), "n").unwrap();
    std::os::unix::fs::symlink("../elsewhere/n.txt", app.join("plain/n.txt")).unwrap();
    std::fs::write(
        app.join("main.qwe"),
        "use \"uses/util.qwe\"\ndb.open(\"data/app.db\")\ndb.exec(\"create table if not exists t (x int)\")\nfor f in [\"uses\", \"config\", \"data\", \"plain\"] {\n  print(delete_file(f))\n}\n",
    )
    .unwrap();
    let (code, out) = sandboxed_at(&app, &profile, &["run", "main.qwe", "--env-file", "config/app.env"], &[], &d.0.join("a.log"), 60);
    assert_eq!(
        (code, out.as_str()),
        (
            Some(0),
            "err(\"delete_file: will not delete uses: it is one of the program's own files\")\nerr(\"delete_file: will not delete config: it is the program's settings (.env)\")\n\
             err(\"delete_file: will not delete data: it holds the database the program has open\")\nerr(\"delete_file: plain: Operation not permitted (os error 1)\")\n"
        )
    );
}

/// A tree deeper than a path can be long (60 folders of 30 letters, 1,900 bytes) is walked by its
/// folders, as `remove_dir_all` deletes it: judged by a path, it was "File name too long" and
/// stayed. It is deleted; one with a second name of the main file at its bottom is refused
/// (src/builtins.rs own_inside, FEEDBACK85.md Q-1340, found by the review of b7f7c26; branch
/// feedback82-85)
#[cfg(unix)]
#[test]
fn a_tree_deeper_than_a_path_is_long_is_judged_and_deleted() {
    let d = Dir(support::temp_dir("cov8285r3-deep").canonicalize().unwrap());
    let app = d.0.join("app");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::write(app.join("main.qwe"), "print(delete_file(\"deep\") | is_err)\nprint(delete_file(\"deep2\"))\n").unwrap();
    let level = "f".repeat(30);
    let main = app.join("main.qwe").display().to_string();
    let script = format!(
        "set -e; for t in deep deep2; do mkdir $t; cd $t; i=0; while [ $i -lt 60 ]; do mkdir {l}; cd {l}; i=$((i+1)); done; echo bottom > b.txt; cd {app}; done; \
         cd deep2; i=0; while [ $i -lt 60 ]; do cd {l}; i=$((i+1)); done; ln {main} again.qwe",
        l = level,
        app = app.display(),
        main = main
    );
    assert!(Command::new("sh").arg("-c").arg(&script).current_dir(&app).status().unwrap().success());
    let (code, out) = quillo_at(&app, &["run", "main.qwe"], &[], &d.0.join("a.log"), 60);
    assert_eq!((code, out.as_str()), (Some(0), "false\nerr(\"delete_file: will not delete deep2: it is one of the program's own files\")\n"));
    assert!(!app.join("deep").exists() && app.join("deep2").exists() && app.join("main.qwe").exists());
}

// Fix round 8 (F3)

/// A pinned build is refused where a folder the program can reach is inside what the build runs
/// or reads, not only where one holds it: `--allow-path` of the cargo home's `registry` (the
/// sources the build compiles), of rustup's home's `toolchains`, and of a folder inside one of its
/// PATH's. The start says which, the folder inside it, and to pin the library instead; nothing is
/// built (src/native.rs toolchain_reached, FEEDBACK82.md Q-1285, found by the coverage tests;
/// branch feedback82-85).
#[cfg(unix)]
#[test]
fn a_pinned_build_is_refused_a_folder_the_program_reaches_inside_its_toolchain() {
    use sha2::Digest;
    let d = Dir(support::temp_dir("cov8285r3-pin-inside").canonicalize().unwrap());
    let (app, tmp, ch, rh, bin) = (d.0.join("app"), d.0.join("tmp"), d.0.join("cargo-home"), d.0.join("rustup-home"), d.0.join("bin"));
    for f in [&app, &tmp, &ch.join("registry/src"), &rh.join("toolchains"), &bin.join("sub")] {
        std::fs::create_dir_all(f).unwrap();
    }
    let src = "//! version: 1.0.0\n//! effects: none\npub fn hi() -> String { \"from the pin\".into() }\n";
    std::fs::write(app.join("hi.qwer"), src).unwrap();
    std::fs::write(app.join("main.qwe"), "use \"hi.qwer\"\nprint(hi())\n").unwrap();
    let hash: String = sha2::Sha256::digest(src.as_bytes()).iter().map(|b| format!("{:02x}", b)).collect();
    std::fs::write(app.join("pin.toml"), format!("[plugins]\nallow = [\"hi@sha256:{}\"]\n", hash)).unwrap();
    let (t, record) = (tmp.display().to_string(), d.0.join("record").display().to_string());
    let path = format!("{}:{}", std::env::var("PATH").unwrap_or_default(), bin.display());
    let cases = [
        ("its cargo home (CARGO_HOME)", ch.join("registry"), vec![("CARGO_HOME", ch.display().to_string())]),
        ("rustup's home (RUSTUP_HOME)", rh.join("toolchains"), vec![("RUSTUP_HOME", rh.display().to_string())]),
        ("a folder of its PATH, whose programs it runs", bin.join("sub"), vec![("PATH", path.clone())]),
    ];
    for (what, inside, vars) in &cases {
        let i = inside.display().to_string();
        let mut all = vec![("TMPDIR", t.clone()), ("QUILLO_BUILT_RECORD_DIR", record.clone())];
        all.extend(vars.iter().map(|(k, v)| (*k, v.clone())));
        let all: Vec<(&str, &str)> = all.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let (code, out) = quillo_at(&app, &["run", "main.qwe", "--no-check", "--policy", "pin.toml", "--allow-path", &i], &all, &d.0.join("run.log"), 120);
        assert_eq!(code, Some(1), "{}: {}", what, out);
        assert!(
            out.contains(&format!("and the build would use {}, ", what)) && out.contains(&format!(", which holds {}, which the program can reach and so could steer what is built: it does not load (have the policy pin its library by its own SHA-256 instead", i)),
            "{}: {}",
            what,
            out
        );
        assert!(!out.contains("building ") && !out.contains("from the pin"), "{}: {}", what, out);
        assert_eq!(std::fs::read_dir(&tmp).unwrap().count(), 0, "{}: a private folder was made", what);
    }
}

/// `files()` serving a journal's name (`q-wal`) whose database's name is a FIFO answers at once:
/// judged a SQLite journal by `q`'s first bytes, it opened the FIFO and waited for a writer, and
/// the request with it, for ever; the next request was answered (src/builtins.rs sqlite_journal,
/// FEEDBACK85.md Q-1340, found by the review of b7f7c26; branch feedback82-85)
#[cfg(unix)]
#[test]
fn files_never_waits_on_a_fifo_beside_a_journals_name() {
    let d = Dir(support::temp_dir("cov8285r3-fifo-serve").canonicalize().unwrap());
    let app = d.0.join("app");
    std::fs::create_dir_all(app.join("public")).unwrap();
    assert!(Command::new("mkfifo").arg(app.join("public/q")).status().unwrap().success());
    std::fs::write(app.join("public/q-wal"), "j").unwrap();
    std::fs::write(app.join("main.qwe"), "route GET /s/*path = files(\"public\", path)\nroute GET /ping = \"pong\"\n").unwrap();
    let (g, port) = serve_at(&app, &["main.qwe"], &d.0.join("a.log"));
    let started = Instant::now();
    let a = send(port, "GET", "/s/q-wal", &[], "");
    assert_eq!((a.status, a.body.as_str()), (200, "j"), "after {:?}", started.elapsed());
    assert_eq!(send(port, "GET", "/ping", &[], "").body, "pong");
    drop(g);
}

// Fix round 9

/// What `own_file` says of a file in the program's `.quillo` folders.
const OWN_QUILLO: &str = "it is in the program's .quillo folder, which holds its built plugins, their lock and its keys";

/// A `.quillo` that is a link, or a link in one, is the program's by where it leads: with
/// `.quillo -> qstore` and `lib/.quillo/native -> ../../build-native`, writing the lock, a library,
/// the `--watch` session key by either name (case too, or not made yet), or deleting them or their
/// folders, went through, as only the name `.quillo` was looked for in the path as it really is.
/// A name beside it that only begins so is a file like others. Under the no-unlink sandbox
/// (src/builtins.rs own_file and quillo_guard, FEEDBACK85.md Q-1340, found by the review of
/// 0284f7c; branch feedback82-85)
#[cfg(target_os = "macos")]
#[test]
fn a_quillo_folder_that_is_a_link_is_the_programs_where_it_leads() {
    let Some((d, app, profile)) = sandboxed_app("cov8285r3-quillo-link") else { return };
    for f in ["qstore/native", "build-native", "lib/.quillo"] {
        std::fs::create_dir_all(app.join(f)).unwrap();
    }
    for (f, text) in [("qstore/native/libhi-1.dylib", "lib"), ("qstore/plugins.lock", "lock"), ("qstore/.quillo-secret", "key"), ("build-native/liby-1.dylib", "lib")] {
        std::fs::write(app.join(f), text).unwrap();
    }
    std::os::unix::fs::symlink("qstore", app.join(".quillo")).unwrap();
    std::os::unix::fs::symlink("../../build-native", app.join("lib/.quillo/native")).unwrap();
    // Fix round 10 decided: where a link inside a `.quillo` leads (`build-native`) is a folder
    // like others to the program, and nothing there is Quillo's: it reads no link in `.quillo`
    // (`a_library_or_lock_planted_in_quillo_gives_nothing`). Its name through the link stays refused.
    let writes = [".quillo/plugins.lock", ".QUILLO/native/libhi-1.dylib", "qstore/plugins.lock", "qstore/native/libhi-1.dylib", "qstore/native/new.dylib", ".quillo/.quillo-secret", "lib/.quillo/native/liby-1.dylib"];
    let deletes = [".quillo/plugins.lock", ".quillo/native", "qstore/native/libhi-1.dylib", "qstore"];
    let mut main = String::from("use \"lib/x.qwe\"\n");
    for w in writes {
        main.push_str(&format!("print(write_file(\"{}\", \"EVIL\"))\n", w));
    }
    for p in deletes {
        main.push_str(&format!("print(delete_file(\"{}\"))\n", p));
    }
    main.push_str("print(write_file(\"qstore-notes.txt\", \"ok\") | is_err)\nprint(write_file(\"build-native/new.dylib\", \"ok\") | is_err)\n");
    std::fs::write(app.join("main.qwe"), &main).unwrap();
    let mut want = String::new();
    for w in writes {
        want.push_str(&format!("err(\"write_file: will not write {}: {}\")\n", w, OWN_QUILLO));
    }
    for p in deletes {
        want.push_str(&format!("err(\"delete_file: will not delete {}: {}\")\n", p, OWN_QUILLO));
    }
    want.push_str("false\nfalse\n");
    let (code, out) = sandboxed_at(&app, &profile, &["run", "main.qwe"], &[], &d.0.join("a.log"), 60);
    assert_eq!((code, out.as_str()), (Some(0), want.as_str()));
    for (f, text) in [("qstore/plugins.lock", "lock"), ("qstore/.quillo-secret", "key"), ("build-native/liby-1.dylib", "lib")] {
        assert_eq!(std::fs::read_to_string(app.join(f)).unwrap(), text, "{}", f);
    }
    assert!(!app.join("qstore/native/new.dylib").exists());
}

/// An upload saved by the name a visitor gave cannot reach a `.quillo` that is a link out of the
/// folder into one `--allow-path` names: `.quillo/plugins.lock` replaced the lock and
/// `.quillo/native/...` planted a library (src/builtins.rs own_file, FEEDBACK85.md Q-1340, found
/// by the review of 0284f7c; branch feedback82-85)
#[cfg(unix)]
#[test]
fn an_upload_cannot_reach_a_quillo_folder_linked_out_of_the_program() {
    let d = Dir(support::temp_dir("cov8285r3-quillo-upload").canonicalize().unwrap());
    let (app, store) = (d.0.join("app"), d.0.join("data/qstore"));
    std::fs::create_dir_all(store.join("native")).unwrap();
    std::fs::create_dir_all(&app).unwrap();
    std::fs::write(store.join("plugins.lock"), "lock").unwrap();
    std::os::unix::fs::symlink("../data/qstore", app.join(".quillo")).unwrap();
    std::fs::write(app.join("main.qwe"), "route POST /u {\n  let r = save_upload(request.files.doc, request.form.to)\n  return is_err(r) ? r.error : \"saved\"\n}\n").unwrap();
    let (g, port) = serve_at(&app, &["main.qwe", "--allow-path", "../data"], &d.0.join("a.log"));
    let origin = format!("http://localhost:{}", port);
    // `.QUILLO` is `.quillo` where names are compared in any case (macOS, Windows); on Linux it is
    // another folder, and an upload there is a file like others (found by CI on 18de67d).
    let mut tos = vec![".quillo/plugins.lock", ".quillo/native/libhi-1.dylib"];
    if cfg!(any(target_os = "macos", windows)) {
        tos.push(".QUILLO/native/libhi-2.dylib");
    }
    for to in tos {
        let body = format!("--B\r\nContent-Disposition: form-data; name=\"to\"\r\n\r\n{}\r\n--B\r\nContent-Disposition: form-data; name=\"doc\"; filename=\"e.txt\"\r\nContent-Type: text/plain\r\n\r\nEVIL\r\n--B--\r\n", to);
        let a = send(port, "POST", "/u", &[("Content-Type", "multipart/form-data; boundary=B"), ("Origin", &origin), ("Sec-Fetch-Site", "same-origin")], &body);
        assert_eq!(a.body, format!("save_upload: will not write {}: {}; save uploads under a folder of their own, such as uploads/", to, OWN_QUILLO), "{}", to);
    }
    drop(g);
    assert_eq!(std::fs::read_to_string(store.join("plugins.lock")).unwrap(), "lock");
    assert_eq!(std::fs::read_dir(store.join("native")).unwrap().count(), 0);
}

/// Deleting a folder that holds a link to a folder above the program's (`/`, the temporary folder,
/// `..`) is refused, as naming the link is, saying so: it said "it is one of the program's own
/// files", of a link to `/`. A link to a folder that holds nothing of the program's is let
/// through, to the deletion the sandbox refuses (src/builtins.rs own_inside, FEEDBACK85.md Q-1340,
/// found by the review of 0284f7c; branch feedback82-85)
#[cfg(target_os = "macos")]
#[test]
fn deleting_a_folder_with_a_link_above_the_program_says_why() {
    let Some((d, app, profile)) = sandboxed_app("cov8285r3-link-above") else { return };
    for f in ["j_root", "j_up", "j_tmp", "j_none", "none"] {
        std::fs::create_dir_all(app.join(f)).unwrap();
    }
    std::os::unix::fs::symlink("/", app.join("j_root/r")).unwrap();
    std::os::unix::fs::symlink("..", app.join("j_up/up")).unwrap();
    std::os::unix::fs::symlink(&d.0, app.join("j_tmp/t")).unwrap();
    std::os::unix::fs::symlink("../none", app.join("j_none/n")).unwrap();
    std::fs::write(app.join("main.qwe"), "for f in [\"j_root\", \"j_up\", \"j_tmp\", \"j_none\"] {\n  print(delete_file(f))\n}\n").unwrap();
    let held = "it holds a link to a folder that holds the program's own files";
    let want = format!(
        "err(\"delete_file: will not delete j_root: {held}\")\nerr(\"delete_file: will not delete j_up: {held}\")\nerr(\"delete_file: will not delete j_tmp: {held}\")\nerr(\"delete_file: j_none: Operation not permitted (os error 1)\")\n"
    );
    let (code, out) = sandboxed_at(&app, &profile, &["run", "main.qwe"], &[], &d.0.join("a.log"), 60);
    assert_eq!((code, out.as_str()), (Some(0), want.as_str()));
}

/// On macOS `.quıllo` (a dotless ı) is another name than `.quillo`, as APFS compares names: it was
/// refused as the program's `.quillo`, the name folded by upper then lower case, which makes ı an
/// i. `.QUILLO` still is the program's (src/builtins.rs fold_name, FEEDBACK85.md Q-1340, found by
/// the review of 0284f7c; branch feedback82-85)
#[cfg(target_os = "macos")]
#[test]
fn a_dotless_i_is_another_name_on_macos() {
    let d = Dir(support::temp_dir("cov8285r3-dotless").canonicalize().unwrap());
    let app = d.0.join("app");
    std::fs::create_dir_all(app.join(".quillo")).unwrap();
    std::fs::create_dir_all(app.join(".qu\u{131}llo")).unwrap();
    if !app.join(".qu\u{131}llo").is_dir() || std::fs::read_dir(&app).unwrap().count() != 2 {
        eprintln!("skipped: this file system takes .qu\u{131}llo for .quillo");
        return;
    }
    std::fs::write(app.join("main.qwe"), "print(write_file(\".qu\u{131}llo/notes.txt\", \"ok\") | is_err)\nprint(write_file(\".QUILLO/plugins.lock\", \"x\"))\n").unwrap();
    let (code, out) = quillo_at(&app, &["run", "main.qwe"], &[], &d.0.join("a.log"), 60);
    assert_eq!((code, out.as_str()), (Some(0), format!("false\nerr(\"write_file: will not write .QUILLO/plugins.lock: {}\")\n", OWN_QUILLO).as_str()));
    assert!(app.join(".qu\u{131}llo/notes.txt").exists() && !app.join(".quillo/plugins.lock").exists());
}

// Fix round 10 (F3)

/// A plugin's native build cache of its own, in cargo's temporary folder for tests.
fn r10_cache(name: &str) -> String {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("run3-r10-{}", name)).display().to_string()
}

/// A plugin no layer pins by hash, which links a library of the program's own folder
/// (`#[link(name = "qzs")]` with `-L`), loads whether or not another plugin is pinned by hash:
/// `allow = ["zs"]` and `allow = ["zs", "other@sha256:.."]` alike. Since fcac7a3 the second was
/// refused by what its library loads, judged wherever any layer pinned any plugin by hash
/// (src/native.rs use_file, load_copy, pinned_by_hash, FEEDBACK83.md Q-1305, found by the coverage
/// tests; branch feedback82-85).
#[cfg(target_os = "macos")]
#[test]
fn an_unpinned_plugin_is_not_judged_where_another_is_pinned_by_hash() {
    let d = Dir(support::temp_dir("cov8285r3-unpinned").canonicalize().unwrap());
    let (app, lib, tmp) = (d.0.join("app"), d.0.join("lib"), d.0.join("tmp"));
    for f in [&app, &lib, &tmp] {
        std::fs::create_dir_all(f).unwrap();
    }
    std::fs::write(lib.join("q.c"), "int qzs(void) { return 7; }\n").unwrap();
    let dylib = lib.join("libqzs.dylib");
    let ok = Command::new("cc").args(["-dynamiclib", "-o"]).arg(&dylib).arg(lib.join("q.c")).arg("-install_name").arg(&dylib).stdin(Stdio::null()).status().unwrap();
    assert!(ok.success());
    std::fs::write(app.join("zs.qwer"), "//! version: 1.0.0\n//! effects: none\n#[link(name = \"qzs\")]\nextern \"C\" { fn qzs() -> i32; }\npub fn hi() -> String { unsafe { qzs() }.to_string() }\n").unwrap();
    std::fs::write(app.join("main.qwe"), "use \"zs.qwer\"\nprint(hi())\n").unwrap();
    let (flags, cache, t) = (format!("-L {}", lib.display()), r10_cache("unpinned"), tmp.display().to_string());
    for (policy, allow) in [("nopin.toml", "[\"zs\"]".to_string()), ("pin.toml", format!("[\"zs\", \"other@sha256:{}\"]", "2d".repeat(32)))] {
        std::fs::write(app.join(policy), format!("[plugins]\nallow = {}\n", allow)).unwrap();
        let (code, out) = quillo_at(&app, &["run", "main.qwe", "--no-check", "--policy", policy], &[("RUSTFLAGS", &flags), ("QUILLO_NATIVE_CACHE", &cache), ("TMPDIR", &t)], &d.0.join("run.log"), 600);
        assert_eq!((code, out.ends_with("7\n")), (Some(0), true), "{}: {}", policy, out);
    }
}

/// A library pinned by its own SHA-256 (a provider's `QUILLO_POLICY_PATH` layer) is opened as a
/// private copy of the bytes whose hash is pinned, read once: while another process swaps it in
/// `.quillo/native` over and over with a library whose constructor writes a file, every start
/// loads the pinned one or is refused, and the other never runs. Before, the library was hashed,
/// read again to be judged and opened by its path a third time (src/native.rs load_copy,
/// private_library, FEEDBACK83.md Q-1305, found by the coverage tests; branch feedback82-85).
#[cfg(target_os = "macos")]
#[test]
fn a_library_pinned_by_its_hash_is_opened_as_the_bytes_whose_hash_is_pinned() {
    use sha2::Digest;
    use std::sync::atomic::{AtomicBool, Ordering};
    let d = Dir(support::temp_dir("cov8285r3-libswap").canonicalize().unwrap());
    let (app, tmp) = (d.0.join("app"), d.0.join("tmp"));
    for f in [&app, &tmp] {
        std::fs::create_dir_all(f).unwrap();
    }
    std::fs::write(app.join("hi.qwer"), "//! version: 1.0.0\n//! effects: none\npub fn hi() -> String { \"hi from A\".into() }\n").unwrap();
    std::fs::write(app.join("main.qwe"), "use \"hi.qwer\"\nprint(hi())\n").unwrap();
    let (cache, t) = (r10_cache("libswap"), tmp.display().to_string());
    let (code, out) = quillo_at(&app, &["build", "main.qwe"], &[("QUILLO_NATIVE_CACHE", &cache)], &d.0.join("build.log"), 600);
    assert_eq!(code, Some(0), "{}", out);
    let lib = quillo::native::library_path(&app.join("hi.qwer")).unwrap();
    let a = std::fs::read(&lib).unwrap();
    std::fs::write(d.0.join("a.lib"), &a).unwrap();
    let ran = d.0.join("RAN");
    std::fs::write(d.0.join("b.c"), format!("#include <stdio.h>\n__attribute__((constructor)) static void b(void) {{ FILE *f = fopen({:?}, \"w\"); if (f) fclose(f); }}\n", ran.display().to_string())).unwrap();
    let ok = Command::new("cc").args(["-dynamiclib", "-o"]).arg(d.0.join("b.lib")).arg(d.0.join("b.c")).stdin(Stdio::null()).status().unwrap();
    assert!(ok.success());
    let hash: String = sha2::Sha256::digest(&a).iter().map(|b| format!("{:02x}", b)).collect();
    let pin = d.0.join("pin.toml");
    std::fs::write(&pin, format!("[plugins]\nallow = [\"hi@sha256:{}\"]\n", hash)).unwrap();
    let p = pin.display().to_string();
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let swapper = {
        let (stop, d0, lib) = (stop.clone(), d.0.clone(), lib.clone());
        std::thread::spawn(move || {
            let next = lib.with_extension("next");
            while !stop.load(Ordering::Relaxed) {
                for src in ["a.lib", "b.lib"] {
                    let _ = std::fs::remove_file(&next);
                    if std::fs::hard_link(d0.join(src), &next).is_ok() {
                        let _ = std::fs::rename(&next, &lib);
                    }
                }
            }
        })
    };
    let mut loaded = 0;
    for _ in 0..150 {
        let (code, out) = quillo_at(&app, &["run", "main.qwe", "--no-check", "--no-build"], &[("QUILLO_POLICY_PATH", &p), ("TMPDIR", &t)], &d.0.join("run.log"), 60);
        assert!(!ran.exists(), "the other library ran: {}", out);
        if code == Some(0) {
            assert!(out.ends_with("hi from A\n"), "{}", out);
            loaded += 1;
        }
    }
    stop.store(true, Ordering::Relaxed);
    swapper.join().unwrap();
    // The swapper left the library a second name of a.lib or b.lib: one of two names is not read
    // (fix round 10, src/statics.rs open_below), so it is put back as a file of its own.
    let _ = std::fs::remove_file(&lib);
    std::fs::write(&lib, &a).unwrap();
    let (code, out) = quillo_at(&app, &["run", "main.qwe", "--no-check", "--no-build"], &[("QUILLO_POLICY_PATH", &p), ("TMPDIR", &t)], &d.0.join("run.log"), 60);
    assert_eq!((code, out.ends_with("hi from A\n")), (Some(0), true), "{} (loaded {} of 150 while swapped)", out, loaded);
    assert!(!ran.exists());
    assert_eq!(std::fs::read_dir(&tmp).unwrap().count(), 0, "a private copy was left");
}

/// A link in a `.quillo` that leads to the program's folder or one above it (`native -> /`,
/// `sites -> ..`) is no folder of Quillo's own: taken as one, it had every write of the program
/// refused, and the whole disk looked at once a second. An ordinary file is written; what is in
/// `.quillo` itself stays the program's (src/builtins.rs quillo_guard, FEEDBACK85.md Q-1340,
/// found fixing the review of 0284f7c; branch feedback82-85)
#[cfg(unix)]
#[test]
fn a_link_in_quillo_to_a_folder_above_the_program_is_no_folder_of_quillos() {
    let d = Dir(support::temp_dir("cov8285r3-quillo-up").canonicalize().unwrap());
    let app = d.0.join("app");
    std::fs::create_dir_all(app.join(".quillo")).unwrap();
    std::fs::write(app.join(".quillo/plugins.lock"), "lock").unwrap();
    std::os::unix::fs::symlink("/", app.join(".quillo/native")).unwrap();
    std::os::unix::fs::symlink("..", app.join(".quillo/sites")).unwrap();
    std::fs::write(app.join("main.qwe"), "print(write_file(\"notes.txt\", \"ok\") | is_err)\nprint(write_file(\"data/n.txt\", \"ok\") | is_err)\nprint(write_file(\".quillo/plugins.lock\", \"x\"))\n").unwrap();
    let started = Instant::now();
    let (code, out) = quillo_at(&app, &["run", "main.qwe"], &[], &d.0.join("a.log"), 60);
    assert_eq!((code, out.as_str()), (Some(0), format!("false\nfalse\nerr(\"write_file: will not write .quillo/plugins.lock: {}\")\n", OWN_QUILLO).as_str()));
    assert!(started.elapsed() < Duration::from_secs(20), "{:?}", started.elapsed());
}

// Fix round 10

/// A program's `.quillo` folder made a link to `qstore`, with more than 4,096 files in another
/// `.quillo` of the program (`lib/.quillo/cache`): the lock, the session key and a library were
/// written, the walk of round 9 having stopped at 4,096 entries before it came to `qstore`. Judged
/// by its name and by where it resolves, nothing in it looked at (src/builtins.rs own_target and
/// quillo_roots, FEEDBACK85.md Q-1340, found by the review of 9da2632; branch feedback82-85)
#[cfg(target_os = "macos")]
#[test]
fn a_quillo_is_judged_by_its_name_and_where_it_resolves_however_much_is_in_one() {
    let Some((d, app, profile)) = sandboxed_app("cov8285r3-quillo-many") else { return };
    for f in ["qstore/native", "lib/.quillo/cache"] {
        std::fs::create_dir_all(app.join(f)).unwrap();
    }
    for i in 0..4100 {
        std::fs::write(app.join(format!("lib/.quillo/cache/c{}", i)), "c").unwrap();
    }
    for (f, text) in [("qstore/native/libhi-1.dylib", "lib"), ("qstore/plugins.lock", "lock"), ("qstore/.quillo-secret", "key")] {
        std::fs::write(app.join(f), text).unwrap();
    }
    std::os::unix::fs::symlink("qstore", app.join(".quillo")).unwrap();
    let writes = [".quillo/plugins.lock", "qstore/plugins.lock", "qstore/native/new.dylib", ".quillo/native/libhi-1.dylib", ".quillo/.quillo-secret"];
    let deletes = ["qstore/native/libhi-1.dylib", "qstore", ".quillo/native"];
    let mut main = String::from("use \"lib/x.qwe\"\n");
    for w in writes {
        main.push_str(&format!("print(write_file(\"{}\", \"EVIL\"))\n", w));
    }
    for p in deletes {
        main.push_str(&format!("print(delete_file(\"{}\"))\n", p));
    }
    std::fs::write(app.join("main.qwe"), &main).unwrap();
    let mut want = String::new();
    for w in writes {
        want.push_str(&format!("err(\"write_file: will not write {}: {}\")\n", w, OWN_QUILLO));
    }
    for p in deletes {
        want.push_str(&format!("err(\"delete_file: will not delete {}: {}\")\n", p, OWN_QUILLO));
    }
    let (code, out) = sandboxed_at(&app, &profile, &["run", "main.qwe"], &[], &d.0.join("a.log"), 60);
    assert_eq!((code, out.as_str()), (Some(0), want.as_str()));
    for (f, text) in [("qstore/native/libhi-1.dylib", "lib"), ("qstore/plugins.lock", "lock"), ("qstore/.quillo-secret", "key")] {
        assert_eq!(std::fs::read_to_string(app.join(f)).unwrap(), text, "{}", f);
    }
}

/// A write does not wait on what a `.quillo` holds: with 100,000 files in `.quillo/sites`, a write
/// stopped for a quarter of a second once a second while round 9's guard read the folder to its
/// end. 20,000 writes, the longest compared with the longest where `.quillo` is empty, on the same
/// machine: a fixed bound failed on CI's Linux runner in debug, whose writes are slow enough on
/// their own (found by CI on 18de67d) (src/builtins.rs own_target, FEEDBACK85.md Q-1340, found by
/// the review of 9da2632; branch feedback82-85)
#[cfg(unix)]
#[test]
fn a_write_does_not_wait_on_what_a_quillo_holds() {
    let d = Dir(support::temp_dir("cov8285r3-quillo-flat").canonicalize().unwrap());
    let app = d.0.join("app");
    std::fs::create_dir_all(app.join(".quillo/sites")).unwrap();
    std::fs::create_dir_all(app.join("out")).unwrap();
    std::fs::write(
        app.join("main.qwe"),
        "let worst = 0\nlet last = now()\nfor i in range(0, 20000) {\n  let _w = write_file(\"out/a.txt\", \"x\")\n  let t = now()\n  if t - last > worst {\n    worst = t - last\n  }\n  last = t\n}\nprint(worst)\n",
    )
    .unwrap();
    let worst = || -> u64 {
        let (code, out) = quillo_at(&app, &["run", "main.qwe"], &[], &d.0.join("a.log"), 600);
        assert_eq!(code, Some(0), "{}", out);
        out.trim().parse().unwrap()
    };
    let empty = worst();
    for i in 0..100_000 {
        std::fs::write(app.join(format!(".quillo/sites/s{}.json", i)), "{}").unwrap();
    }
    let full = worst();
    assert!(full <= empty * 5 + 100, "the longest write {} ms with a full .quillo, {} ms with an empty one", full, empty);
}

/// A `.quillo` that is a link to what is not there yet (`qstore`, `../data/qstore`) leads there
/// all the same: made by the program, the lock, the key and a library were its, as nothing was
/// protected while the link led nowhere (src/builtins.rs link_end, FEEDBACK85.md Q-1340, found by
/// the review of 9da2632; branch feedback82-85)
#[cfg(target_os = "macos")]
#[test]
fn a_quillo_that_leads_to_what_is_not_there_yet_leads_there() {
    let Some((d, app, profile)) = sandboxed_app("cov8285r3-quillo-dangling") else { return };
    std::fs::create_dir_all(d.0.join("data")).unwrap();
    std::os::unix::fs::symlink("qstore", app.join(".quillo")).unwrap();
    std::os::unix::fs::symlink("../../data/qstore", app.join("lib/.quillo")).unwrap();
    // (`.quillo/plugins.lock` through the dangling link is refused before: it leads nowhere.)
    let writes = ["qstore/plugins.lock", "qstore/.quillo-secret", "qstore/native/libhi-1.dylib", "../data/qstore/plugins.lock", "../data/qstore/native/libhi-1.dylib"];
    let mut main = String::from("use \"lib/x.qwe\"\n");
    for w in writes {
        main.push_str(&format!("print(write_file(\"{}\", \"EVIL\"))\n", w));
    }
    main.push_str("print(write_file(\"qstore-notes/a.txt\", \"ok\") | is_err)\n");
    std::fs::write(app.join("main.qwe"), &main).unwrap();
    let mut want = String::new();
    for w in writes {
        want.push_str(&format!("err(\"write_file: will not write {}: {}\")\n", w, OWN_QUILLO));
    }
    want.push_str("false\n");
    let (code, out) = sandboxed_at(&app, &profile, &["run", "main.qwe", "--allow-path", "../data"], &[], &d.0.join("a.log"), 60);
    assert_eq!((code, out.as_str()), (Some(0), want.as_str()));
    assert!(!app.join("qstore").exists() && !d.0.join("data/qstore").exists());
}

/// A `.quillo` that leads to the program's folder or above it (`-> .`, `-> ..`, `-> /`), beside
/// its main file or a file it uses, stops the start, naming it: with `.quillo -> .` the lock and a
/// library were the program's files to write (`plugins.lock`, `native/...`) (src/builtins.rs
/// quillo_layout, src/main.rs, FEEDBACK85.md Q-1340, found by the review of 9da2632; branch
/// feedback82-85)
#[cfg(unix)]
#[test]
fn a_quillo_that_leads_to_the_programs_folder_or_above_stops_the_start() {
    for (at, to) in [(".quillo", "."), (".quillo", ".."), (".quillo", "/"), ("lib/.quillo", "..")] {
        let d = Dir(support::temp_dir("cov8285r3-quillo-self").canonicalize().unwrap());
        let app = d.0.join("app");
        std::fs::create_dir_all(app.join("lib")).unwrap();
        std::fs::write(app.join("lib/x.qwe"), "let lib_value = 1\n").unwrap();
        std::fs::write(app.join("main.qwe"), "use \"lib/x.qwe\"\nprint(write_file(\"plugins.lock\", \"EVIL\"))\n").unwrap();
        std::os::unix::fs::symlink(to, app.join(at)).unwrap();
        let (code, out) = quillo_at(&app, &["run", "main.qwe"], &[], &d.0.join("a.log"), 60);
        assert_eq!(code, Some(1), "{} -> {}: {}", at, to, out);
        assert!(out.contains(&format!("{} leads to ", app.join(at).display())) && out.contains(", where the program writes") && out.contains("so it must be a folder of its own"), "{} -> {}: {}", at, to, out);
        assert!(!app.join("plugins.lock").exists());
    }
}

/// A `.quillo` anywhere is refused by its name, as the file system compares names: another tool's
/// or project's (`tools/.quillo`, `vendor/newplug/.quillo`), which were files like others, in any
/// case on macOS. A name that only begins so is not (src/statics.rs quillo_part, FEEDBACK85.md
/// Q-1340, found by the review of 9da2632; branch feedback82-85)
#[cfg(unix)]
#[test]
fn a_quillo_anywhere_is_refused_by_its_name() {
    let d = Dir(support::temp_dir("cov8285r3-quillo-anywhere").canonicalize().unwrap());
    let app = d.0.join("app");
    std::fs::create_dir_all(app.join("tools")).unwrap();
    let mut writes = vec!["tools/.quillo/plugins.lock", "tools/.quillo/native/libp-0123.dylib", "vendor/newplug/.quillo/native/libnewplug-abcd.dylib"];
    if cfg!(target_os = "macos") {
        writes.push("tools/.QuIlLo/plugins.lock");
    }
    let mut main = String::new();
    for w in &writes {
        main.push_str(&format!("print(write_file(\"{}\", \"EVIL\"))\n", w));
    }
    main.push_str("print(write_file(\"tools/.quillo-old/a.txt\", \"ok\") | is_err)\nprint(write_file(\"tools/x.quillo/a.txt\", \"ok\") | is_err)\n");
    std::fs::write(app.join("main.qwe"), &main).unwrap();
    let mut want = String::new();
    for w in &writes {
        want.push_str(&format!("err(\"write_file: will not write {}: {}\")\n", w, OWN_QUILLO));
    }
    want.push_str("false\nfalse\n");
    let (code, out) = quillo_at(&app, &["run", "main.qwe"], &[], &d.0.join("a.log"), 60);
    assert_eq!((code, out.as_str()), (Some(0), want.as_str()));
    assert!(!app.join("tools/.quillo").exists());
}

/// A database a program names in a `.quillo`, by its name or where it resolves, is refused as a
/// write there is: `db.open(".quillo/.quillo-secret")` made a database of the session key's file
/// (src/builtins.rs quillo_refused, which db.rs asks; FEEDBACK85.md Q-1340, found by the review of
/// 9da2632; branch feedback82-85)
#[cfg(unix)]
#[test]
fn a_database_in_a_quillo_is_refused_by_the_helper_db_asks() {
    let d = Dir(support::temp_dir("cov8285r3-quillo-db").canonicalize().unwrap());
    let app = d.0.join("app");
    std::fs::create_dir_all(app.join("qstore")).unwrap();
    std::os::unix::fs::symlink("qstore", app.join(".quillo")).unwrap();
    let it = quillo::Interp::new(&app);
    for p in [".quillo/.quillo-secret", ".quillo/plugins.lock", "qstore/app.db", "lib/.quillo/x.db"] {
        let why = quillo::builtins::quillo_refused(&it, &app.join(p));
        assert_eq!(why, Some(format!("{}: {}", app.join(p).display(), OWN_QUILLO)), "{}", p);
    }
    assert_eq!(quillo::builtins::quillo_refused(&it, &app.join("data/app.db")), None);
}

/// `db.open` of a file in a `.quillo` is refused (needs db.rs to ask `builtins::quillo_refused`,
/// F2's file this round: un-ignore once it does) (src/db.rs, FEEDBACK85.md Q-1340, found by the
/// review of 9da2632; branch feedback82-85)
#[cfg(unix)]
#[test]
fn db_open_of_a_file_in_a_quillo_is_refused() {
    let d = Dir(support::temp_dir("cov8285r3-quillo-dbopen").canonicalize().unwrap());
    let app = d.0.join("app");
    std::fs::create_dir_all(app.join(".quillo")).unwrap();
    std::fs::write(app.join("main.qwe"), "let r = db.connect(\".quillo/.quillo-secret\")\nprint(is_err(r))\n").unwrap();
    let (code, out) = quillo_at(&app, &["run", "main.qwe"], &[], &d.0.join("a.log"), 60);
    assert_eq!((code, out.as_str()), (Some(0), "true\n"));
    assert!(!app.join(".quillo/.quillo-secret").exists());
}

/// Quillo reads what it keeps in `.quillo` as its own: no link below it is followed and no file of
/// two names is read. With `.quillo/native` a link to a folder the program writes, a library
/// hard-linked from one, or `plugins.lock` a link to one, the plugin loaded what the program put
/// there; now the start says which and why, and nothing is built through the link (src/native.rs
/// quillo_read and lib_there, src/statics.rs open_below, FEEDBACK85.md Q-1340, found by the
/// review of 9da2632; branch feedback82-85)
#[cfg(unix)]
#[test]
fn a_library_or_lock_planted_in_quillo_gives_nothing() {
    let d = Dir(support::temp_dir("cov8285r3-quillo-planted").canonicalize().unwrap());
    let app = d.0.join("app");
    std::fs::create_dir_all(app.join("uploads")).unwrap();
    std::fs::write(app.join("hi.qwer"), "//! version: 1.0.0\n//! effects: none\npub fn hi() -> String { \"built here\".into() }\n").unwrap();
    std::fs::write(app.join("main.qwe"), "use \"hi.qwer\"\nprint(hi())\n").unwrap();
    let log = d.0.join("a.log");
    let record = d.0.join("record").display().to_string();
    let run = || quillo_at(&app, &["run", "main.qwe", "--no-check"], &[("QUILLO_BUILT_RECORD_DIR", &record)], &log, 600);
    let (code, out) = run();
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.ends_with("built here\n"), "{}", out);
    let native = app.join(".quillo/native");
    let lib = std::fs::read_dir(&native).unwrap().flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|x| x == std::env::consts::DLL_EXTENSION)).unwrap();
    let name = lib.file_name().unwrap().to_owned();
    // `native` a link to a folder the program writes.
    std::fs::rename(&native, app.join("uploads/native")).unwrap();
    std::os::unix::fs::symlink("../uploads/native", &native).unwrap();
    let (code, out) = run();
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("will not load the library of ./hi.qwer: native is a link in") && out.contains("which Quillo does not follow there"), "{}", out);
    assert!(!out.contains("building ") && !out.contains("built here\n"), "{}", out);
    std::fs::remove_file(&native).unwrap();
    std::fs::rename(app.join("uploads/native"), &native).unwrap();
    // The library hard-linked from a file the program writes.
    std::fs::hard_link(native.join(&name), app.join("uploads/lib.bin")).unwrap();
    let (code, out) = run();
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("has another name too (a hard link), which Quillo does not read there"), "{}", out);
    std::fs::remove_file(app.join("uploads/lib.bin")).unwrap();
    // The lock a link to a file the program writes.
    let lock = app.join(".quillo/plugins.lock");
    std::fs::rename(&lock, app.join("uploads/plugins.lock")).unwrap();
    std::os::unix::fs::symlink("../uploads/plugins.lock", &lock).unwrap();
    let (code, out) = run();
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("will not load the library of ./hi.qwer: plugins.lock is a link in"), "{}", out);
    std::fs::remove_file(&lock).unwrap();
    std::fs::rename(app.join("uploads/plugins.lock"), &lock).unwrap();
    let (code, out) = run();
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.ends_with("built here\n") && !out.contains("building "), "{}", out);
}

/// The `--watch` session key is read as Quillo's own: one hard-linked from a file the program
/// writes was the program's to choose, and signed its sessions. It is said, and not used
/// (src/mobile.rs keep_secret, FEEDBACK85.md Q-1340, found by the review of 9da2632; branch
/// feedback82-85)
#[cfg(unix)]
#[test]
fn a_session_key_of_two_names_is_not_used() {
    let d = Dir(support::temp_dir("cov8285r3-quillo-key").canonicalize().unwrap());
    let app = d.0.join("app");
    std::fs::create_dir_all(app.join(".quillo")).unwrap();
    std::fs::create_dir_all(app.join("uploads")).unwrap();
    std::fs::write(app.join("uploads/key"), "k".repeat(64)).unwrap();
    std::fs::hard_link(app.join("uploads/key"), app.join(".quillo/.quillo-secret")).unwrap();
    std::fs::write(app.join("main.qwe"), "route GET / = \"ok\"\n").unwrap();
    let log = d.0.join("a.log");
    let (g, port) = support_serve_watch(&app, &log);
    assert_eq!(send(port, "GET", "/", &[], "").body, "ok");
    drop(g);
    let said = std::fs::read_to_string(&log).unwrap();
    assert!(said.contains("cannot keep a session key in .quillo") && said.contains("has another name too (a hard link)"), "{}", said);
}

/// `quillo run main.qwe --watch --port 0` in `app`, with no QUILLO_SECRET: its guard and port.
fn support_serve_watch(app: &Path, log: &Path) -> (Guard, u16) {
    let file = std::fs::File::create(log).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", "main.qwe", "--watch", "--port", "0"])
        .current_dir(app)
        .env_remove("QUILLO_POLICY_PATH")
        .env_remove("QUILLO_SECRET")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(file)
        .spawn()
        .unwrap();
    let mut g = Guard::new(child);
    let port = port_in_log(&mut g.0, log, Duration::from_secs(60));
    (g, port)
}

/// A plugin's `.quillo` that leads to the folder it is in or above (`lib/.quillo -> ..`), laid out
/// so after its library and lock were put where the program writes (`app/native`,
/// `app/plugins.lock`), loaded what was there before the start's judge of the layout came; now the
/// `use` refuses it, naming the `.quillo` (src/native.rs quillo_read and lib_there,
/// src/builtins.rs quillo_not_its_own, FEEDBACK85.md Q-1340, found fixing the review of 9da2632;
/// branch feedback82-85)
#[cfg(unix)]
#[test]
fn a_plugins_quillo_that_leads_above_its_folder_is_not_read() {
    let d = Dir(support::temp_dir("cov8285r3-quillo-plugup").canonicalize().unwrap());
    let app = d.0.join("app");
    std::fs::create_dir_all(app.join("lib")).unwrap();
    std::fs::write(app.join("lib/hi.qwer"), "//! version: 1.0.0\n//! effects: none\npub fn hi() -> String { \"built here\".into() }\n").unwrap();
    std::fs::write(app.join("main.qwe"), "use \"lib/hi.qwer\"\nprint(hi())\n").unwrap();
    let log = d.0.join("a.log");
    let (code, out) = quillo_at(&app, &["run", "main.qwe", "--no-check"], &[], &log, 600);
    assert_eq!((code, out.ends_with("built here\n")), (Some(0), true), "{}", out);
    std::fs::rename(app.join("lib/.quillo/native"), app.join("native")).unwrap();
    std::fs::rename(app.join("lib/.quillo/plugins.lock"), app.join("plugins.lock")).unwrap();
    std::fs::remove_dir_all(app.join("lib/.quillo")).unwrap();
    std::os::unix::fs::symlink("..", app.join("lib/.quillo")).unwrap();
    let (code, out) = quillo_at(&app, &["run", "main.qwe", "--no-check"], &[], &log, 600);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains(&format!("{} leads to {}, which holds {}, where the program writes", app.join("lib/.quillo").display(), app.display(), app.join("lib").display())), "{}", out);
    assert!(!out.contains("built here\n") && !out.contains("building "), "{}", out);
}

// Fix round 11 (F3)

/// `quillo run args --port 0` in `cwd` with `vars`, its log in `log`, stopped when the guard is
/// dropped, and its port.
fn serve_vars(cwd: &Path, args: &[&str], vars: &[(&str, &str)], log: &Path) -> (Guard, u16) {
    let file = std::fs::File::create(log).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .args(args)
        .args(["--port", "0"])
        .current_dir(cwd)
        .env_remove("QUILLO_POLICY_PATH")
        .env_remove("QUILLO_DB")
        .env("QUILLO_SECRET", "test secret for the tests, 32 bytes or more")
        .envs(vars.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(file)
        .spawn()
        .unwrap();
    let mut g = Guard::new(child);
    let port = port_in_log(&mut g.0, log, Duration::from_secs(600));
    (g, port)
}

/// What `p` holds, or nothing.
fn log_of(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

/// A plugin whose function answers the address of one static and a count kept in it.
const COUNTER: &str = "//! version: 1.0.0\n//! effects: none\nstatic N: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);\npub fn addr() -> String { format!(\"{:p} {}\", &N, N.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1) }\n";

/// Each answer's address and count.
fn counted(body: &str) -> (String, i64) {
    let (a, n) = body.trim().split_once(' ').unwrap_or_else(|| panic!("{}", body));
    (a.to_string(), n.parse().unwrap_or_else(|_| panic!("{}", body)))
}

/// A plugin a quillo.confirmed holds, and one pinned by its library's own SHA-256, are each loaded
/// once in a process, from one private copy: two instances answer with one static, its count goes
/// on from one to the next, and four reloads (`--reload`, the main file edited) keep it, where each
/// instance and each reload loaded a new copy, its count started again and memory grew
/// (src/native.rs load_copy, COPIES, FEEDBACK83.md Q-1305, found by the coverage tests; branch
/// feedback82-85).
#[cfg(unix)]
#[test]
fn a_held_plugin_is_loaded_once_a_process_by_each_instance_and_reload() {
    use sha2::Digest;
    let d = Dir(support::temp_dir("cov8285r3-once").canonicalize().unwrap());
    let (confirmed, pinned, tmp) = (d.0.join("confirmed"), d.0.join("pinned"), d.0.join("tmp"));
    std::fs::create_dir_all(&tmp).unwrap();
    for f in [&confirmed, &pinned] {
        std::fs::create_dir_all(f).unwrap();
        std::fs::write(f.join("cnt.qwer"), COUNTER).unwrap();
        std::fs::write(f.join("main.qwe"), "import cnt.qwer use addr\nroute GET / = addr()\n").unwrap();
    }
    let (t, record, cache) = (tmp.display().to_string(), d.0.join("record").display().to_string(), r10_cache("once"));
    let (code, out) = quillo_at(&confirmed, &["confirm", "--yes"], &[("QUILLO_BUILT_RECORD_DIR", &record)], &d.0.join("c.log"), 120);
    assert_eq!(code, Some(0), "{}", out);
    let (code, out) = quillo_at(&pinned, &["build", "main.qwe"], &[("QUILLO_NATIVE_CACHE", &cache)], &d.0.join("b.log"), 600);
    assert_eq!(code, Some(0), "{}", out);
    let lib = quillo::native::library_path(&pinned.join("cnt.qwer")).unwrap();
    let hash: String = sha2::Sha256::digest(std::fs::read(&lib).unwrap()).iter().map(|b| format!("{:02x}", b)).collect();
    let pin = d.0.join("pin.toml");
    std::fs::write(&pin, format!("[plugins]\nallow = [\"cnt@sha256:{}\"]\n", hash)).unwrap();
    let p = pin.display().to_string();
    for (name, app, vars) in [("confirmed", &confirmed, vec![("TMPDIR", t.as_str()), ("QUILLO_BUILT_RECORD_DIR", record.as_str())]), ("pinned", &pinned, vec![("TMPDIR", t.as_str()), ("QUILLO_POLICY_PATH", p.as_str())])] {
        // Two instances, one library.
        let log = d.0.join(format!("{}-2.log", name));
        let (g, port) = serve_vars(app, &["main.qwe", "--no-check", "--instances", "2"], &vars, &log);
        let seen: Vec<(String, i64)> = (0..8).map(|_| counted(&send(port, "GET", "/", &[], "").body)).collect();
        drop(g);
        assert!(seen.iter().all(|(a, _)| *a == seen[0].0), "{}: two copies: {:?} {}", name, seen, log_of(&log));
        assert_eq!(seen.iter().map(|(_, n)| *n).collect::<Vec<_>>(), (1..=8).collect::<Vec<_>>(), "{}", name);
        // Four reloads, one library, its count kept.
        let log = d.0.join(format!("{}-r.log", name));
        let (g, port) = serve_vars(app, &["main.qwe", "--no-check", "--reload"], &vars, &log);
        let (first, mut last) = counted(&send(port, "GET", "/", &[], "").body);
        for i in 1..=4 {
            std::fs::write(app.join("main.qwe"), format!("import cnt.qwer use addr\nroute GET / = addr()\n// edit {}\n", i)).unwrap();
            let until = Instant::now() + Duration::from_secs(60);
            while log_of(&log).matches("reloaded main.qwe").count() < i && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(50));
            }
            let (a, n) = counted(&send(port, "GET", "/", &[], "").body);
            assert_eq!((a.as_str(), n), (first.as_str(), last + 1), "{}: reload {}: {}", name, i, log_of(&log));
            last = n;
        }
        drop(g);
        assert_eq!(log_of(&log).matches("reloaded main.qwe").count(), 4, "{}", log_of(&log));
    }
    assert_eq!(std::fs::read_dir(&tmp).unwrap().count(), 0, "a private copy was left");
}

/// A library pinned by its own SHA-256 loads where the temporary folder is inside what
/// `--allow-path` gives the program: it is not built, and its private copy is made in the user's
/// own temporary folder (macOS's `DARWIN_USER_TEMP_DIR`), which it cannot reach, where it was
/// refused as a pinned build's folder would be. Where the program can reach that folder too, it is
/// refused, saying it is the private copy's folder (src/native.rs copy_dir, user_temp_dir,
/// FEEDBACK83.md Q-1305, found by the coverage tests; branch feedback82-85).
#[cfg(target_os = "macos")]
#[test]
fn a_library_pinned_by_its_hash_loads_where_the_temporary_folder_is_the_programs() {
    use sha2::Digest;
    let d = Dir(support::temp_dir("cov8285r3-copydir").canonicalize().unwrap());
    let (app, tmp) = (d.0.join("app"), d.0.join("tmp"));
    for f in [&app, &tmp] {
        std::fs::create_dir_all(f).unwrap();
    }
    std::fs::write(app.join("hi.qwer"), "//! version: 1.0.0\n//! effects: none\npub fn hi() -> String { \"hi from the pinned library\".into() }\n").unwrap();
    std::fs::write(app.join("main.qwe"), "use \"hi.qwer\"\nprint(hi())\n").unwrap();
    let cache = r10_cache("copydir");
    let (code, out) = quillo_at(&app, &["build", "main.qwe"], &[("QUILLO_NATIVE_CACHE", &cache)], &d.0.join("b.log"), 600);
    assert_eq!(code, Some(0), "{}", out);
    let lib = quillo::native::library_path(&app.join("hi.qwer")).unwrap();
    let hash: String = sha2::Sha256::digest(std::fs::read(&lib).unwrap()).iter().map(|b| format!("{:02x}", b)).collect();
    let pin = d.0.join("pin.toml");
    std::fs::write(&pin, format!("[plugins]\nallow = [\"hi@sha256:{}\"]\n", hash)).unwrap();
    let (t, p, open) = (tmp.display().to_string(), pin.display().to_string(), d.0.display().to_string());
    let (code, out) = quillo_at(&app, &["run", "main.qwe", "--no-check", "--no-build", "--allow-path", &open], &[("TMPDIR", &t), ("QUILLO_POLICY_PATH", &p)], &d.0.join("r.log"), 60);
    assert_eq!((code, out.ends_with("hi from the pinned library\n")), (Some(0), true), "{}", out);
    assert_eq!(std::fs::read_dir(&tmp).unwrap().count(), 0, "a copy was made in the folder the program reaches");
    // The user's own temporary folder too.
    let user = String::from_utf8(Command::new("/usr/bin/getconf").arg("DARWIN_USER_TEMP_DIR").output().unwrap().stdout).unwrap().trim().to_string();
    let user = PathBuf::from(&user).canonicalize().unwrap().display().to_string();
    let (code, out) = quillo_at(&app, &["run", "main.qwe", "--no-check", "--no-build", "--allow-path", &open, "--allow-path", &user], &[("TMPDIR", &t), ("QUILLO_POLICY_PATH", &p)], &d.0.join("r.log"), 60);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("a held or pinned library is opened as a private copy, in a folder the program cannot reach, and it can reach ") && out.contains(": it does not load") && !out.contains("hi from"), "{}", out);
}

/// An ELF image (64 bits, little-endian) of these loaded segments (`(p_offset, p_vaddr, size)`),
/// a dynamic table of `dynamic` at `dyn_at` (in the file and in memory), and `bytes` put at file
/// offsets, `len` bytes long.
fn elf_of(loads: &[(u64, u64, u64)], dyn_at: u64, dynamic: &[(u64, u64)], bytes: &[(usize, &[u8])], len: usize) -> Vec<u8> {
    let mut b = vec![0u8; len];
    b[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    b[16..18].copy_from_slice(&3u16.to_le_bytes());
    b[32..40].copy_from_slice(&64u64.to_le_bytes());
    b[54..56].copy_from_slice(&56u16.to_le_bytes());
    b[56..58].copy_from_slice(&((loads.len() + 1) as u16).to_le_bytes());
    let dyn_size = (dynamic.len() as u64 + 1) * 16;
    for (i, (kind, off, va, size)) in loads.iter().map(|(o, v, s)| (1u32, *o, *v, *s)).chain([(2u32, dyn_at, dyn_at, dyn_size)]).enumerate() {
        let h = 64 + i * 56;
        b[h..h + 4].copy_from_slice(&kind.to_le_bytes());
        b[h + 8..h + 16].copy_from_slice(&off.to_le_bytes());
        b[h + 16..h + 24].copy_from_slice(&va.to_le_bytes());
        b[h + 32..h + 40].copy_from_slice(&size.to_le_bytes());
        b[h + 40..h + 48].copy_from_slice(&size.to_le_bytes());
    }
    for (i, (t, v)) in dynamic.iter().enumerate() {
        let at = dyn_at as usize + i * 16;
        b[at..at + 8].copy_from_slice(&t.to_le_bytes());
        b[at + 8..at + 16].copy_from_slice(&v.to_le_bytes());
    }
    for (at, x) in bytes {
        b[*at..*at + x.len()].copy_from_slice(x);
    }
    b
}

/// What an ELF library needs is read as its loader reads it: musl's `libc.musl-x86_64.so.1` and
/// `ld-musl-x86_64.so.1` are the C runtime's; two loaded segments on one page, which ld.so maps
/// one over the other, are refused; and a needed name is read through the segment that holds its
/// address, `libssl.so.3` there, not the `libc.so.6` the file has at the string table's place plus
/// its offset (src/native.rs elf_loads, runtime_soname, FEEDBACK83.md Q-1305, found by the coverage
/// tests; branch feedback82-85).
#[test]
fn an_elf_librarys_strings_are_read_where_its_loader_reads_them() {
    let musl = b"\0libc.musl-x86_64.so.1\0ld-musl-x86_64.so.1\0";
    let one = elf_of(&[(0, 0, 0x400)], 0x200, &[(1, 1), (1, 23), (5, 0x300)], &[(0x300, musl)], 0x400);
    assert_eq!(quillo::native::loads_refused(&one), None);
    let overlap = elf_of(&[(0, 0, 0x800), (0x800, 0x800, 0x800)], 0x200, &[(1, 1), (5, 0x300)], &[(0x300, b"\0libc.so.6\0")], 0x1000);
    assert_eq!(quillo::native::loads_refused(&overlap), Some("has loaded segments (PT_LOAD) on the same pages, which its loader maps one over the other".to_string()));
    // DT_STRTAB at 0x300 of the first segment; the name at 0x300 + 0xfd00 = 0x10000, an address of
    // the second segment (file 0x1000), where the file itself has another name at 0x10000.
    let far = elf_of(&[(0, 0, 0x1000), (0x1000, 0x10000, 0x100)], 0x200, &[(1, 0xfd00), (5, 0x300)], &[(0x300, b"\0"), (0x1000, b"libssl.so.3\0"), (0x10000, b"libc.so.6\0")], 0x10010);
    assert_eq!(quillo::native::loads_refused(&far), Some("needs libssl.so.3, which is not the C runtime's".to_string()));
}

// Fix round 12 (F3)

/// A crate of the test's folder, `evil2`, whose build script writes `MARKER` beside it when cargo
/// builds it, and its path.
fn marker_crate(d: &Path) -> PathBuf {
    let dep = d.join("evil2");
    std::fs::create_dir_all(dep.join("src")).unwrap();
    std::fs::write(dep.join("Cargo.toml"), "[package]\nname = \"evil2\"\nversion = \"0.1.0\"\nedition = \"2021\"\nbuild = \"build.rs\"\n\n[lib]\npath = \"src/lib.rs\"\n").unwrap();
    std::fs::write(dep.join("build.rs"), format!("fn main() {{ std::fs::write({:?}, \"ran\").unwrap(); }}\n", d.join("MARKER").display().to_string())).unwrap();
    std::fs::write(dep.join("src/lib.rs"), "pub fn x() -> i64 { 4242 }\n").unwrap();
    dep
}

/// A plugin a host's layer pins by its library's SHA-256 is never built, building on or not: with
/// the pinned library in `.quillo/native` (bytes that are no library) and a lock line of another
/// hash, the start does not build the repository's plugin, so its dependency's build script never
/// runs, and the library pinned is refused as what it is. Before, the start said it was building it
/// again and ran cargo (src/native.rs build_with, prebuild, FEEDBACK83.md Q-1305, found by the
/// coverage tests; branch feedback82-85).
#[cfg(unix)]
#[test]
fn a_plugin_pinned_by_its_library_is_never_built() {
    use sha2::Digest;
    let d = Dir(support::temp_dir("cov8285r3-never").canonicalize().unwrap());
    let (app, tmp) = (d.0.join("app"), d.0.join("tmp"));
    for f in [&app, &tmp] {
        std::fs::create_dir_all(f).unwrap();
    }
    let dep = marker_crate(&d.0);
    std::fs::write(app.join("qr.qwer"), format!("//! version: 1.0.0\n//! effects: none\n//! [dependencies]\n//! evil2 = {{ path = {:?} }}\npub fn hi() -> i64 {{ evil2::x() }}\n", dep.display().to_string())).unwrap();
    std::fs::write(app.join("main.qwe"), "use \"qr.qwer\"\nprint(hi())\n").unwrap();
    let lib = quillo::native::library_path(&app.join("qr.qwer")).unwrap();
    std::fs::create_dir_all(lib.parent().unwrap()).unwrap();
    let fake = b"not the library built from qr.qwer";
    std::fs::write(&lib, fake).unwrap();
    std::fs::write(app.join(".quillo/plugins.lock"), format!("{} {}\n", lib.file_name().unwrap().to_string_lossy(), "0".repeat(64))).unwrap();
    let hash: String = sha2::Sha256::digest(fake).iter().map(|b| format!("{:02x}", b)).collect();
    let host = d.0.join("host.toml");
    std::fs::write(&host, format!("[plugins]\nallow = [\"qr@sha256:{}\"]\n", hash)).unwrap();
    let (h, t, cache) = (host.display().to_string(), tmp.display().to_string(), r10_cache("never"));
    let (code, out) = quillo_at(&app, &["run", "main.qwe", "--no-check"], &[("QUILLO_POLICY_PATH", &h), ("TMPDIR", &t), ("QUILLO_NATIVE_CACHE", &cache)], &d.0.join("run.log"), 600);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("is not a library this system loads: it does not load") && !out.contains("building"), "{}", out);
    assert!(!d.0.join("MARKER").exists(), "the repository's dependency was built: {}", out);
}

/// A plugin that starts with a byte order mark, held to its source's SHA-256, builds no dependency
/// its header names from a folder: refused before anything is built, at the dependency's line,
/// where the header was read on the text with the mark (no header) and the build on the text
/// without it, which built `evil2` and ran its build script (src/native.rs rust_text, build_private;
/// FEEDBACK83.md Q-1305, found by the coverage tests; branch feedback82-85).
#[cfg(unix)]
#[test]
fn a_plugin_with_a_byte_order_mark_under_a_source_pin_builds_no_dependency_from_a_folder() {
    use sha2::Digest;
    let d = Dir(support::temp_dir("cov8285r3-bom").canonicalize().unwrap());
    let (app, tmp) = (d.0.join("app"), d.0.join("tmp"));
    for f in [&app, &tmp] {
        std::fs::create_dir_all(f).unwrap();
    }
    let dep = marker_crate(&d.0);
    let src = format!("\u{FEFF}//! version: 1.0.0\n//! effects: none\n//! [dependencies]\n//! evil2 = {{ path = {:?} }}\npub fn hi() -> i64 {{ evil2::x() }}\n", dep.display().to_string());
    std::fs::write(app.join("hi.qwer"), &src).unwrap();
    std::fs::write(app.join("main.qwe"), "use \"hi.qwer\"\nprint(hi())\n").unwrap();
    let hash: String = sha2::Sha256::digest(src.as_bytes()).iter().map(|b| format!("{:02x}", b)).collect();
    std::fs::write(app.join("pin.toml"), format!("[plugins]\nallow = [\"hi@sha256:{}\"]\n", hash)).unwrap();
    let (t, record) = (tmp.display().to_string(), d.0.join("record").display().to_string());
    let (code, out) = quillo_at(&app, &["run", "main.qwe", "--no-check", "--policy", "pin.toml"], &[("TMPDIR", &t), ("QUILLO_BUILT_RECORD_DIR", &record)], &d.0.join("run.log"), 600);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("builds another file into it (the dependency `evil2` from a folder (`path`) at line 4)") && !out.contains("building ") && !out.contains("4242"), "{}", out);
    assert!(!d.0.join("MARKER").exists(), "the dependency was built: {}", out);
}

// Fix round 11

/// A mounted program's or a service's `.quillo` that is a link is the process's, kept from the
/// main program's writes as the main program's own is: with `shop/.quillo -> qs` (and
/// `svc/.quillo -> qs2`), the main program wrote `shop/qs/plugins.lock` and a library into
/// `shop/qs/native`, which loaded at the next start, and deleted `shop/qs` (src/builtins.rs
/// note_quillo_folder and quillo_roots, FEEDBACK85.md Q-1340, found by the final review of
/// a6cee54; branch feedback82-85)
#[cfg(target_os = "macos")]
#[test]
fn a_mounted_programs_or_services_quillo_is_the_processs() {
    let Some((d, app, profile)) = sandboxed_app("cov8285r3-quillo-mount") else { return };
    for f in ["shop/qs/native", "svc/qs2/native"] {
        std::fs::create_dir_all(app.join(f)).unwrap();
    }
    for f in ["shop/qs/plugins.lock", "svc/qs2/plugins.lock"] {
        std::fs::write(app.join(f), "lock").unwrap();
    }
    std::os::unix::fs::symlink("qs", app.join("shop/.quillo")).unwrap();
    std::os::unix::fs::symlink("qs2", app.join("svc/.quillo")).unwrap();
    std::fs::write(app.join("shop/app.qwe"), "route GET / = \"shop\"\n").unwrap();
    std::fs::write(app.join("svc/b.qwe"), "server fn hello() {\n  return \"b\"\n}\n").unwrap();
    let writes = ["shop/qs/plugins.lock", "shop/qs/native/libhi-1.dylib", "svc/qs2/plugins.lock", "svc/qs2/native/libb-1.dylib"];
    let deletes = ["shop/qs", "svc/qs2/native"];
    let mut main = String::from("mount /shop = \"shop/app.qwe\"\nservice /b = \"svc/b.qwe\"\nroute GET / = \"main\"\n");
    for w in writes {
        main.push_str(&format!("print(write_file(\"{}\", \"EVIL\"))\n", w));
    }
    for p in deletes {
        main.push_str(&format!("print(delete_file(\"{}\"))\n", p));
    }
    main.push_str("print(write_file(\"shop/notes.txt\", \"ok\") | is_err)\nexit(0)\n");
    std::fs::write(app.join("main.qwe"), &main).unwrap();
    let mut want = String::new();
    for w in writes {
        want.push_str(&format!("err(\"write_file: will not write {}: {}\")\n", w, OWN_QUILLO));
    }
    for p in deletes {
        want.push_str(&format!("err(\"delete_file: will not delete {}: {}\")\n", p, OWN_QUILLO));
    }
    want.push_str("false\n");
    let (code, out) = sandboxed_at(&app, &profile, &["run", "main.qwe", "--port", "0"], &[], &d.0.join("a.log"), 60);
    assert!(out.contains(&want), "{:?}: {}", code, out);
    for f in ["shop/qs/plugins.lock", "svc/qs2/plugins.lock"] {
        assert_eq!(std::fs::read_to_string(app.join(f)).unwrap(), "lock", "{}", f);
    }
    assert!(!app.join("shop/qs/native/libhi-1.dylib").exists() && !app.join("svc/qs2/native/libb-1.dylib").exists());
}

/// An upload by the name a visitor gave cannot reach a mounted program's `.quillo` that is a link:
/// `shop/qs/plugins.lock` and a library in `shop/qs/native` were "saved", and the library's
/// constructor ran at the next start (src/builtins.rs own_target, FEEDBACK85.md Q-1340, found by
/// the final review of a6cee54; branch feedback82-85)
#[cfg(unix)]
#[test]
fn an_upload_cannot_reach_a_mounted_programs_quillo() {
    let d = Dir(support::temp_dir("cov8285r3-quillo-mount-up").canonicalize().unwrap());
    let app = d.0.join("app");
    std::fs::create_dir_all(app.join("shop/qs/native")).unwrap();
    std::fs::write(app.join("shop/qs/plugins.lock"), "lock").unwrap();
    std::os::unix::fs::symlink("qs", app.join("shop/.quillo")).unwrap();
    std::fs::write(app.join("shop/app.qwe"), "route GET / = \"shop\"\n").unwrap();
    std::fs::write(app.join("main.qwe"), "mount /shop = \"shop/app.qwe\"\nroute POST /u {\n  let r = save_upload(request.files.doc, request.form.to)\n  return is_err(r) ? r.error : \"saved\"\n}\n").unwrap();
    let (g, port) = serve_at(&app, &["main.qwe"], &d.0.join("a.log"));
    let origin = format!("http://localhost:{}", port);
    for to in ["shop/qs/plugins.lock", "shop/qs/native/libhi-1.dylib"] {
        let body = format!("--B\r\nContent-Disposition: form-data; name=\"to\"\r\n\r\n{}\r\n--B\r\nContent-Disposition: form-data; name=\"doc\"; filename=\"e.txt\"\r\nContent-Type: text/plain\r\n\r\nEVIL\r\n--B--\r\n", to);
        let a = send(port, "POST", "/u", &[("Content-Type", "multipart/form-data; boundary=B"), ("Origin", &origin), ("Sec-Fetch-Site", "same-origin")], &body);
        assert_eq!(a.body, format!("save_upload: will not write {}: {}; save uploads under a folder of their own, such as uploads/", to, OWN_QUILLO), "{}", to);
    }
    assert_eq!(send(port, "GET", "/shop/", &[], "").body, "shop");
    drop(g);
    assert_eq!(std::fs::read_to_string(app.join("shop/qs/plugins.lock")).unwrap(), "lock");
    assert_eq!(std::fs::read_dir(app.join("shop/qs/native")).unwrap().count(), 0);
}

/// `files()` and `file()` keep to themselves what is under where a `.quillo` leads, as dot names
/// are kept: with `.quillo -> qstore`, `files(".", p)` served `/qstore/plugins.lock`, a site's
/// cookie jar and a library's name, and so through `public/q -> ../qstore`; `file()` of a
/// visitor's path too. `read_file` of the program's own choosing reads it, and another file is
/// served (src/builtins.rs quillo_hidden and hidden_target, FEEDBACK85.md Q-1340, found by the
/// final review of a6cee54; branch feedback82-85)
#[cfg(unix)]
#[test]
fn files_keeps_what_is_under_a_quillo_to_itself() {
    let d = Dir(support::temp_dir("cov8285r3-quillo-serve").canonicalize().unwrap());
    let app = d.0.join("app");
    for f in ["qstore/sites", "qstore/native", "public"] {
        std::fs::create_dir_all(app.join(f)).unwrap();
    }
    for (f, text) in [("qstore/plugins.lock", "lock"), ("qstore/sites/shop.json", "{}"), ("qstore/app-cookies.json", "{}"), ("qstore/native/libhi-1.dylib", "lib"), ("x.txt", "x")] {
        std::fs::write(app.join(f), text).unwrap();
    }
    std::os::unix::fs::symlink("qstore", app.join(".quillo")).unwrap();
    std::os::unix::fs::symlink("../qstore", app.join("public/q")).unwrap();
    std::fs::write(
        app.join("main.qwe"),
        "route GET /all/*p = files(\".\", p)\nroute GET /pub/*p = files(\"public\", p)\nroute GET /f/*p = file(p)\nroute GET /r = read_file(\"qstore/plugins.lock\")\n",
    )
    .unwrap();
    let (g, port) = serve_at(&app, &["main.qwe"], &d.0.join("a.log"));
    for path in ["/all/qstore/plugins.lock", "/all/qstore/sites/shop.json", "/all/qstore/app-cookies.json", "/all/qstore/native/libhi-1.dylib", "/all/QSTORE/plugins.lock", "/pub/q/plugins.lock", "/f/qstore/plugins.lock", "/f/.quillo/plugins.lock", "/f/public/q/plugins.lock"] {
        assert_eq!(send(port, "GET", path, &[], "").status, 404, "{}", path);
    }
    assert_eq!(send(port, "GET", "/all/x.txt", &[], "").body, "x");
    assert_eq!(send(port, "GET", "/f/x.txt", &[], "").body, "x");
    assert_eq!(send(port, "GET", "/r", &[], "").body, "lock");
    drop(g);
}

// Fix round 13 (F3)

/// A pinned build whose dependency a cargo home's `[patch.crates-io]` takes from a folder (the
/// glue's serde_json, from a crate whose build script writes `MARKER`) is refused as its
/// dependencies are resolved, before anything is built: the build script never runs, and the start
/// says which package and from where. Before, the lock was judged after the build, when that build
/// script had run (src/native.rs build_private, packages_refused, FEEDBACK83.md Q-1305, found by
/// the coverage tests; branch feedback82-85).
#[cfg(unix)]
#[test]
fn a_dependency_patched_to_a_folder_is_refused_before_anything_is_built() {
    use sha2::Digest;
    let d = Dir(support::temp_dir("cov8285r3-patch").canonicalize().unwrap());
    let (app, tmp, ch, sj) = (d.0.join("app"), d.0.join("tmp"), d.0.join("cargo-home"), d.0.join("sj"));
    for f in [&app, &tmp, &ch, &sj.join("src")] {
        std::fs::create_dir_all(f).unwrap();
    }
    std::fs::write(sj.join("Cargo.toml"), "[package]\nname = \"serde_json\"\nversion = \"1.0.999\"\nedition = \"2021\"\nbuild = \"build.rs\"\n\n[lib]\npath = \"src/lib.rs\"\n").unwrap();
    std::fs::write(sj.join("build.rs"), format!("fn main() {{ std::fs::write({:?}, \"ran\").unwrap(); }}\n", d.0.join("MARKER").display().to_string())).unwrap();
    std::fs::write(sj.join("src/lib.rs"), "pub fn x() {}\n").unwrap();
    std::fs::write(ch.join("config.toml"), format!("[patch.crates-io]\nserde_json = {{ path = {:?} }}\n", sj.display().to_string())).unwrap();
    let src = "//! version: 1.0.0\n//! effects: none\npub fn hi() -> i64 { 7 }\n";
    std::fs::write(app.join("hi.qwer"), src).unwrap();
    std::fs::write(app.join("main.qwe"), "use \"hi.qwer\"\nprint(hi())\n").unwrap();
    let hash: String = sha2::Sha256::digest(src.as_bytes()).iter().map(|b| format!("{:02x}", b)).collect();
    std::fs::write(app.join("pin.toml"), format!("[plugins]\nallow = [\"hi@sha256:{}\"]\n", hash)).unwrap();
    let (t, c, record) = (tmp.display().to_string(), ch.display().to_string(), d.0.join("record").display().to_string());
    let (code, out) = quillo_at(&app, &["run", "main.qwe", "--no-check", "--policy", "pin.toml"], &[("TMPDIR", &t), ("CARGO_HOME", &c), ("QUILLO_BUILT_RECORD_DIR", &record)], &d.0.join("run.log"), 600);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains(&format!("and its dependencies resolve to the package serde_json from a folder ({}), whose code that hash does not cover: it does not load, and nothing was built", sj.join("Cargo.toml").display())), "{}", out);
    assert!(!d.0.join("MARKER").exists(), "the patched crate's build script ran: {}", out);
    assert_eq!(std::fs::read_dir(&tmp).unwrap().count(), 0, "a private folder was left");
}

/// On macOS a pinned build is refused where the program can reach the user's own temporary folder,
/// where xcrun keeps the cache that picks the clang and ld `cc` runs, whatever TMPDIR says, and
/// where it can reach the developer folder (`xcode-select -p`) its linker is taken from; the start
/// says which, and nothing is built (src/native.rs toolchain_reached, FEEDBACK82.md Q-1285, found
/// by the coverage tests; branch feedback82-85).
#[cfg(target_os = "macos")]
#[test]
fn a_pinned_build_is_refused_where_the_program_reaches_xcruns_cache_or_the_developer_folder() {
    use sha2::Digest;
    let d = Dir(support::temp_dir("cov8285r3-xcrun").canonicalize().unwrap());
    let (app, tmp) = (d.0.join("app"), d.0.join("tmp"));
    for f in [&app, &tmp] {
        std::fs::create_dir_all(f).unwrap();
    }
    let src = "//! version: 1.0.0\n//! effects: none\npub fn hi() -> i64 { 7 }\n";
    std::fs::write(app.join("hi.qwer"), src).unwrap();
    std::fs::write(app.join("main.qwe"), "use \"hi.qwer\"\nprint(hi())\n").unwrap();
    let hash: String = sha2::Sha256::digest(src.as_bytes()).iter().map(|b| format!("{:02x}", b)).collect();
    std::fs::write(app.join("pin.toml"), format!("[plugins]\nallow = [\"hi@sha256:{}\"]\n", hash)).unwrap();
    let ask = |cmd: &str, args: &[&str]| PathBuf::from(String::from_utf8(Command::new(cmd).args(args).output().unwrap().stdout).unwrap().trim()).canonicalize().unwrap();
    let (user, developer) = (ask("/usr/bin/getconf", &["DARWIN_USER_TEMP_DIR"]), ask("/usr/bin/xcode-select", &["-p"]));
    let (t, record) = (tmp.display().to_string(), d.0.join("record").display().to_string());
    for (allowed, said) in [
        (user.clone(), format!("and the build would use the user's temporary folder, {}, where xcrun keeps the cache that picks the linker, which is inside {}", user.display(), user.display())),
        (developer.clone(), format!("and the build would use the developer folder its linker is taken from (xcode-select -p), {}, which is inside {}", developer.display(), developer.display())),
    ] {
        let a = allowed.display().to_string();
        let (code, out) = quillo_at(&app, &["run", "main.qwe", "--no-check", "--policy", "pin.toml", "--allow-path", &a], &[("TMPDIR", &t), ("QUILLO_BUILT_RECORD_DIR", &record)], &d.0.join("run.log"), 120);
        assert_eq!(code, Some(1), "{}: {}", a, out);
        assert!(out.contains(&said) && !out.contains("building "), "{}: {}", a, out);
    }
}

// Fix round 14 (F3)

/// A pinned build where cargo, offline, has only the host's packages (its cargo home filled by
/// `cargo fetch --target <host>`) builds and loads: its dependencies are resolved for this host
/// alone (`--filter-platform`), and the lock it built with is judged whole. Before, cargo metadata
/// wanted every platform's packages (getrandom's r-efi, wasip2), failed where the build works, and
/// the start was refused, "cannot resolve the dependencies" (src/native.rs build_private,
/// FEEDBACK82.md Q-1285, found by the coverage tests; branch feedback82-85).
#[cfg(unix)]
#[test]
fn a_pinned_build_offline_with_only_the_hosts_packages_builds() {
    use sha2::Digest;
    let d = Dir(support::temp_dir("cov8285r3-offline").canonicalize().unwrap());
    let (app, tmp, ch, fetch) = (d.0.join("app"), d.0.join("tmp"), d.0.join("cargo-home"), d.0.join("fetch"));
    for f in [&app, &tmp, &ch, &fetch] {
        std::fs::create_dir_all(f).unwrap();
    }
    let run = |cmd: &mut Command, secs: u64| {
        let mut child = cmd.env("CARGO_HOME", &ch).env_remove("CARGO_TARGET_DIR").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().unwrap();
        let mut err = child.stderr.take().unwrap();
        let reader = std::thread::spawn(move || {
            let mut s = String::new();
            let _ = std::io::Read::read_to_string(&mut err, &mut s);
            s
        });
        let deadline = Instant::now() + Duration::from_secs(secs);
        let status = loop {
            if let Some(s) = child.try_wait().unwrap() {
                break Some(s);
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        (status.is_some_and(|s| s.success()), reader.join().unwrap())
    };
    let vv = String::from_utf8(Command::new("rustc").arg("-vV").output().unwrap().stdout).unwrap();
    let host = vv.lines().find_map(|l| l.strip_prefix("host: ")).unwrap().trim().to_string();
    // The private crate's manifest, so what is fetched is what it resolves to.
    std::fs::write(fetch.join("Cargo.toml"), "[package]\nname = \"quillo_hi_pinned\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\nbuild = false\n\n[lib]\npath = \"lib.rs\"\n\n[dependencies]\ngetrandom = \"0.3\"\nserde_json = \"1\"\n\n[workspace]\n").unwrap();
    std::fs::write(fetch.join("lib.rs"), "").unwrap();
    let manifest = fetch.join("Cargo.toml");
    let (fetched, said) = run(Command::new("cargo").args(["fetch", "--target", &host, "--manifest-path"]).arg(&manifest), 300);
    assert!(fetched, "cargo fetch: {}", said);
    std::fs::write(ch.join("config.toml"), "[net]\noffline = true\n").unwrap();
    // What the review saw: offline, cargo metadata of every platform fails here.
    let (all, said) = run(Command::new("cargo").args(["metadata", "--format-version", "1", "--manifest-path"]).arg(&manifest), 120);
    assert!(!all && said.contains("--offline"), "cargo metadata of every platform did not fail offline: {}", said);
    let src = "//! version: 1.0.0\n//! effects: none\n//! [dependencies]\n//! getrandom = \"0.3\"\npub fn hi() -> i64 { 7 }\n";
    std::fs::write(app.join("hi.qwer"), src).unwrap();
    std::fs::write(app.join("main.qwe"), "use \"hi.qwer\"\nprint(hi())\n").unwrap();
    let hash: String = sha2::Sha256::digest(src.as_bytes()).iter().map(|b| format!("{:02x}", b)).collect();
    std::fs::write(app.join("pin.toml"), format!("[plugins]\nallow = [\"hi@sha256:{}\"]\n", hash)).unwrap();
    let (t, c, record) = (tmp.display().to_string(), ch.display().to_string(), d.0.join("record").display().to_string());
    let (code, out) = quillo_at(&app, &["run", "main.qwe", "--no-check", "--policy", "pin.toml"], &[("TMPDIR", &t), ("CARGO_HOME", &c), ("QUILLO_BUILT_RECORD_DIR", &record)], &d.0.join("run.log"), 600);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.lines().any(|l| l.trim() == "7") && !out.contains("cannot resolve"), "{}", out);
}

/// A pinned build is refused where the program can reach a folder its C compiler searches on its
/// own, `/usr/local/include`, from which `cc`, given no `-isysroot`, takes `<string.h>` before the
/// system's: the start says which folder, and nothing is built. Before, it was built, and a
/// crate's C (ring's, for jwt) could take a header the program wrote (src/native.rs
/// toolchain_reached, compiler_folders, FEEDBACK82.md Q-1285, found by the coverage tests; branch
/// feedback82-85).
#[cfg(unix)]
#[test]
fn a_pinned_build_is_refused_where_the_program_reaches_a_folder_its_c_compiler_searches() {
    use sha2::Digest;
    let inc = Path::new("/usr/local/include");
    if !inc.is_dir() {
        eprintln!("no /usr/local/include on this machine: nothing to allow");
        return;
    }
    let inc = inc.canonicalize().unwrap();
    let d = Dir(support::temp_dir("cov8285r3-include").canonicalize().unwrap());
    let (app, tmp) = (d.0.join("app"), d.0.join("tmp"));
    for f in [&app, &tmp] {
        std::fs::create_dir_all(f).unwrap();
    }
    let src = "//! version: 1.0.0\n//! effects: none\npub fn hi() -> i64 { 7 }\n";
    std::fs::write(app.join("hi.qwer"), src).unwrap();
    std::fs::write(app.join("main.qwe"), "use \"hi.qwer\"\nprint(hi())\n").unwrap();
    let hash: String = sha2::Sha256::digest(src.as_bytes()).iter().map(|b| format!("{:02x}", b)).collect();
    std::fs::write(app.join("pin.toml"), format!("[plugins]\nallow = [\"hi@sha256:{}\"]\n", hash)).unwrap();
    let (t, record, a) = (tmp.display().to_string(), d.0.join("record").display().to_string(), inc.display().to_string());
    let (code, out) = quillo_at(&app, &["run", "main.qwe", "--no-check", "--policy", "pin.toml", "--allow-path", &a], &[("TMPDIR", &t), ("QUILLO_BUILT_RECORD_DIR", &record)], &d.0.join("run.log"), 120);
    assert_eq!(code, Some(1), "{}", out);
    let said = format!("and the build would use a folder its C compiler searches on its own (for headers, libraries or the programs it runs), /usr/local/include, which is inside {}", inc.display());
    assert!(out.contains(&said) && !out.contains("building "), "{}", out);
}

// Fix round 12

/// An app and the services `--split` starts each keep the other's `.quillo` folders: the app knew a
/// service only by its main file's folder, so `svc/lib/.quillo -> ../q3` (a file the service uses)
/// took the app's write of the service's lock, and `files()` served it; a service at the app's
/// root wrote where the app's `lib/.quillo -> ../q4` leads. The app reads its services' files, a
/// service its app's (src/builtins.rs note_program_folders, src/service.rs split, src/main.rs;
/// FEEDBACK85.md Q-1340, found by the re-review of 00e83c3; branch feedback82-85)
#[cfg(unix)]
#[test]
fn an_app_and_its_split_services_keep_each_others_quillo_folders() {
    let d = Dir(support::temp_dir("cov8285r3-quillo-split").canonicalize().unwrap());
    let app = d.0.join("app");
    for f in ["svc/lib", "svc/q3/native", "lib", "q4/native"] {
        std::fs::create_dir_all(app.join(f)).unwrap();
    }
    for f in ["svc/q3/plugins.lock", "q4/plugins.lock"] {
        std::fs::write(app.join(f), "lock").unwrap();
    }
    std::os::unix::fs::symlink("../q3", app.join("svc/lib/.quillo")).unwrap();
    std::os::unix::fs::symlink("../q4", app.join("lib/.quillo")).unwrap();
    std::fs::write(app.join("svc/lib/h.qwe"), "let h = 1\n").unwrap();
    std::fs::write(app.join("lib/m.qwe"), "let m = 2\n").unwrap();
    let w = "server fn w(p) {\n  let r = write_file(p, \"EVIL\")\n  return is_err(r) ? r.error : \"saved\"\n}\n";
    std::fs::write(app.join("svc/b.qwe"), format!("use \"lib/h.qwe\"\n{}", w)).unwrap();
    std::fs::write(app.join("c.qwe"), w).unwrap();
    std::fs::write(
        app.join("main.qwe"),
        "use \"lib/m.qwe\"\nservice /b = \"svc/b.qwe\"\nservice /c = \"c.qwe\"\nroute writes GET /w/*p {\n  let r = write_file(p, \"EVIL\")\n  return is_err(r) ? r.error : \"saved\"\n}\nroute writes GET /cw/*p {\n  return c.w(p)\n}\nroute GET /all/*p = files(\".\", p)\n",
    )
    .unwrap();
    let (g, port) = serve_at(&app, &["main.qwe", "--split"], &d.0.join("a.log"));
    let refused = |p: &str| format!("write_file: will not write {}: {}", p, OWN_QUILLO);
    assert_eq!(send(port, "GET", "/w/svc/q3/plugins.lock", &[], "").body, refused("svc/q3/plugins.lock"));
    assert_eq!(send(port, "GET", "/w/svc/q3/native/libhi-1.dylib", &[], "").body, refused("svc/q3/native/libhi-1.dylib"));
    assert_eq!(send(port, "GET", "/all/svc/q3/plugins.lock", &[], "").status, 404);
    assert_eq!(send(port, "GET", "/cw/q4/plugins.lock", &[], "").body, refused("q4/plugins.lock"));
    assert_eq!(send(port, "GET", "/w/notes.txt", &[], "").body, "saved");
    drop(g);
    for f in ["svc/q3/plugins.lock", "q4/plugins.lock"] {
        assert_eq!(std::fs::read_to_string(app.join(f)).unwrap(), "lock", "{}", f);
    }
}

/// A service another process serves (`--service-url`) is read where its source is here: the app
/// wrote `svc/qs2/plugins.lock` (`svc/.quillo -> qs2`) and served it (src/service.rs,
/// src/builtins.rs note_program_folders; FEEDBACK85.md Q-1340, found by the re-review of 00e83c3;
/// branch feedback82-85)
#[cfg(unix)]
#[test]
fn a_service_served_elsewhere_keeps_its_quillo_where_its_source_is_here() {
    let d = Dir(support::temp_dir("cov8285r3-quillo-svcurl").canonicalize().unwrap());
    let app = d.0.join("app");
    for f in ["svc/qs2/native", "svc/lib"] {
        std::fs::create_dir_all(app.join(f)).unwrap();
    }
    std::fs::write(app.join("svc/qs2/plugins.lock"), "lock").unwrap();
    std::os::unix::fs::symlink("qs2", app.join("svc/.quillo")).unwrap();
    std::fs::write(app.join("svc/b.qwe"), "server fn hello() {\n  return \"b\"\n}\n").unwrap();
    std::fs::write(app.join("main.qwe"), "service /b = \"svc/b.qwe\"\nroute writes GET /w/*p {\n  let r = write_file(p, \"EVIL\")\n  return is_err(r) ? r.error : \"saved\"\n}\nroute GET /all/*p = files(\".\", p)\n").unwrap();
    // The service itself, served apart, for the app to reach.
    let (gs, sport) = serve_at(&app.join("svc"), &["b.qwe", "--as-service", "--service-name", "b"], &d.0.join("s.log"));
    let url = format!("b=http://127.0.0.1:{}", sport);
    let (g, port) = serve_at(&app, &["main.qwe", "--service-url", &url], &d.0.join("a.log"));
    assert_eq!(send(port, "GET", "/w/svc/qs2/plugins.lock", &[], "").body, format!("write_file: will not write svc/qs2/plugins.lock: {}", OWN_QUILLO));
    assert_eq!(send(port, "GET", "/all/svc/qs2/plugins.lock", &[], "").status, 404);
    drop(g);
    drop(gs);
    assert_eq!(std::fs::read_to_string(app.join("svc/qs2/plugins.lock")).unwrap(), "lock");
}

/// A `.quillo` link is followed part by part: `.quillo -> sub/qstore` with `sub -> missing` leads
/// to `missing/qstore` (where it leads once `missing` is made), which was left as `sub/qstore` and
/// written; `.quillo -> qstore/../q2` with no `qstore` leads where the system cannot tell yet, and
/// stops the start (src/builtins.rs link_end, FEEDBACK85.md Q-1340, found by the re-review of
/// 00e83c3; branch feedback82-85)
#[cfg(unix)]
#[test]
fn a_quillo_link_is_followed_part_by_part() {
    let d = Dir(support::temp_dir("cov8285r3-quillo-chain").canonicalize().unwrap());
    let app = d.0.join("app");
    std::fs::create_dir_all(&app).unwrap();
    std::os::unix::fs::symlink("sub/qstore", app.join(".quillo")).unwrap();
    std::os::unix::fs::symlink("missing", app.join("sub")).unwrap();
    std::fs::write(app.join("main.qwe"), "print(write_file(\"missing/qstore/plugins.lock\", \"EVIL\"))\nprint(write_file(\"missing/other.txt\", \"ok\") | is_err)\n").unwrap();
    let (code, out) = quillo_at(&app, &["run", "main.qwe"], &[], &d.0.join("a.log"), 60);
    assert_eq!((code, out.as_str()), (Some(0), format!("err(\"write_file: will not write missing/qstore/plugins.lock: {}\")\nfalse\n", OWN_QUILLO).as_str()));
    assert!(!app.join("missing/qstore").exists());
    let app2 = d.0.join("app2");
    std::fs::create_dir_all(&app2).unwrap();
    std::os::unix::fs::symlink("qstore/../q2", app2.join(".quillo")).unwrap();
    std::fs::write(app2.join("main.qwe"), "print(write_file(\"q2/plugins.lock\", \"EVIL\"))\n").unwrap();
    let (code, out) = quillo_at(&app2, &["run", "main.qwe"], &[], &d.0.join("b.log"), 60);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("leads where the system cannot tell yet (a `..` after a folder that is not there)"), "{}", out);
    assert!(!app2.join("q2").exists());
}

// Fix round 13

const OWN_SOURCE: &str = "it is one of the program's own files";
const APART_DATABASE: &str = "it is a database of a service or app of the program, which opens it in a process of its own";
const APART_MIGRATIONS: &str = "it is in the migrations of a service or app of the program, which runs them in a process of its own";

/// A write route and an upload route, answering the refusal or "saved".
const WRITES: &str = "route writes GET /w/*p {\n  let r = write_file(p, \"EVIL\")\n  return is_err(r) ? r.error : \"saved\"\n}\nroute POST /u {\n  let r = save_upload(request.files.doc, request.form.to)\n  return is_err(r) ? r.error : \"saved\"\n}\n";

/// A service's code that opens `data/s.db` and runs `migrations` at its start, and names
/// `data/p.db` (by a pipe) and the folder `m2` in a function it never runs.
const SERVICE_DB: &str = "let d = db.open(\"data/s.db\")\ndb.migrate()\nserver fn other() {\n  let x = \"data/p.db\" | db.open()\n  return db.migrate(\"m2\")\n}\nserver fn hello() {\n  return \"b\"\n}\n";

fn upload(port: u16, to: &str) -> String {
    let origin = format!("http://localhost:{}", port);
    let body = format!("--B\r\nContent-Disposition: form-data; name=\"to\"\r\n\r\n{}\r\n--B\r\nContent-Disposition: form-data; name=\"doc\"; filename=\"e.txt\"\r\nContent-Type: text/plain\r\n\r\nEVIL\r\n--B--\r\n", to);
    send(port, "POST", "/u", &[("Content-Type", "multipart/form-data; boundary=B"), ("Origin", &origin), ("Sec-Fetch-Site", "same-origin")], &body).body
}

/// The service's files laid out under `app/svc`: its main file, a file it uses, its migrations.
fn service_files(app: &Path) {
    for f in ["svc/lib", "svc/data", "svc/migrations", "svc/m2"] {
        std::fs::create_dir_all(app.join(f)).unwrap();
    }
    std::fs::write(app.join("svc/lib/h.qwe"), "let h = 1\n").unwrap();
    std::fs::write(app.join("svc/b.qwe"), format!("use \"lib/h.qwe\"\n{}", SERVICE_DB)).unwrap();
    std::fs::write(app.join("svc/migrations/001_a.sql"), "create table t (x int);\n").unwrap();
    std::fs::write(app.join("svc/m2/001_a.sql"), "create table u (x int);\n").unwrap();
}

/// What the app may not write of its service's: each refused as in one process, the files as
/// they were.
fn app_keeps_the_services_files(app: &Path, port: u16) {
    let refused = |p: &str, why: &str| format!("write_file: will not write {}: {}", p, why);
    assert_eq!(upload(port, "svc/b.qwe"), format!("save_upload: will not write svc/b.qwe: {}; save uploads under a folder of their own, such as uploads/", OWN_SOURCE));
    assert_eq!(send(port, "GET", "/w/svc/b.qwe", &[], "").body, refused("svc/b.qwe", OWN_SOURCE));
    assert_eq!(send(port, "GET", "/w/svc/lib/h.qwe", &[], "").body, refused("svc/lib/h.qwe", OWN_SOURCE));
    for p in ["svc/data/s.db", "svc/data/s.db-wal", "svc/data/p.db", "svc/quillo-system.db"] {
        assert_eq!(send(port, "GET", &format!("/w/{}", p), &[], "").body, refused(p, APART_DATABASE), "{}", p);
    }
    for p in ["svc/migrations/002_b.sql", "svc/m2/002_b.sql"] {
        assert_eq!(send(port, "GET", &format!("/w/{}", p), &[], "").body, refused(p, APART_MIGRATIONS), "{}", p);
    }
    assert_eq!(send(port, "GET", "/w/svc/notes.txt", &[], "").body, "saved");
    assert_eq!(send(port, "GET", "/w/notes.txt", &[], "").body, "saved");
    assert_eq!(std::fs::read_to_string(app.join("svc/b.qwe")).unwrap(), format!("use \"lib/h.qwe\"\n{}", SERVICE_DB));
    assert_eq!(std::fs::read_to_string(app.join("svc/lib/h.qwe")).unwrap(), "let h = 1\n");
    assert!(!app.join("svc/migrations/002_b.sql").exists() && !app.join("svc/data/p.db").exists());
}

/// Under `--split` the app keeps its service's own files as one process keeps them: an upload
/// replaced the service's `svc/b.qwe` (which the next start ran), and `write_file` its
/// `svc/lib/h.qwe`, its open `svc/data/s.db` and its migrations; the databases and migrations
/// are those a read of the service's code finds (src/builtins.rs note_program_folders and
/// apart_refused, src/service.rs split; FEEDBACK85.md Q-1340, found by the review of 8989870;
/// branch feedback82-85)
#[cfg(unix)]
#[test]
fn an_app_keeps_its_split_services_own_files() {
    let d = Dir(support::temp_dir("cov8285r3-split-own").canonicalize().unwrap());
    let app = d.0.join("app");
    service_files(&app);
    std::fs::write(app.join("main.qwe"), format!("service /b = \"svc/b.qwe\"\n{}", WRITES)).unwrap();
    let (g, port) = serve_at(&app, &["main.qwe", "--split"], &d.0.join("a.log"));
    app_keeps_the_services_files(&app, port);
    drop(g);
}

/// A service another process serves (`--service-url`) whose source is here: the app keeps its
/// files as it keeps a split service's (src/service.rs declare, src/builtins.rs
/// note_program_folders; FEEDBACK85.md Q-1340, found by the review of 8989870; branch
/// feedback82-85)
#[cfg(unix)]
#[test]
fn an_app_keeps_the_own_files_of_a_service_served_elsewhere() {
    let d = Dir(support::temp_dir("cov8285r3-svcurl-own").canonicalize().unwrap());
    let app = d.0.join("app");
    service_files(&app);
    std::fs::write(app.join("main.qwe"), format!("service /b = \"svc/b.qwe\"\n{}", WRITES)).unwrap();
    let (gs, sport) = serve_at(&app.join("svc"), &["b.qwe", "--as-service", "--service-name", "b"], &d.0.join("s.log"));
    let url = format!("b=http://127.0.0.1:{}", sport);
    let (g, port) = serve_at(&app, &["main.qwe", "--service-url", &url], &d.0.join("a.log"));
    app_keeps_the_services_files(&app, port);
    drop(g);
    drop(gs);
}

/// A service `--split` starts at the app's root keeps the app's files as the app keeps its: it
/// overwrote the app's `main.qwe` and `lib/m.qwe`; the app's database is kept too, and the
/// service's own (`data/c.db`, which its code names and has not opened) is its own to write
/// (src/main.rs, src/builtins.rs note_program_folders; FEEDBACK85.md Q-1340, found by the review
/// of 8989870; branch feedback82-85)
#[cfg(unix)]
#[test]
fn a_split_service_keeps_its_apps_own_files() {
    let d = Dir(support::temp_dir("cov8285r3-split-app-own").canonicalize().unwrap());
    let app = d.0.join("app");
    std::fs::create_dir_all(app.join("lib")).unwrap();
    std::fs::create_dir_all(app.join("data")).unwrap();
    std::fs::write(app.join("lib/m.qwe"), "let m = 2\n").unwrap();
    std::fs::write(app.join("c.qwe"), "server fn w(p) {\n  let r = write_file(p, \"EVIL\")\n  return is_err(r) ? r.error : \"saved\"\n}\nserver fn later() {\n  return db.open(\"data/c.db\")\n}\n").unwrap();
    let main = "use \"lib/m.qwe\"\nlet d = db.open(\"data/app.db\")\nservice /c = \"c.qwe\"\nroute writes GET /cw/*p {\n  return c.w(p)\n}\n";
    std::fs::write(app.join("main.qwe"), main).unwrap();
    let (g, port) = serve_at(&app, &["main.qwe", "--split"], &d.0.join("a.log"));
    let refused = |p: &str, why: &str| format!("write_file: will not write {}: {}", p, why);
    assert_eq!(send(port, "GET", "/cw/main.qwe", &[], "").body, refused("main.qwe", OWN_SOURCE));
    assert_eq!(send(port, "GET", "/cw/lib/m.qwe", &[], "").body, refused("lib/m.qwe", OWN_SOURCE));
    assert_eq!(send(port, "GET", "/cw/data/app.db", &[], "").body, refused("data/app.db", APART_DATABASE));
    assert_eq!(send(port, "GET", "/cw/data/c.db", &[], "").body, "saved");
    assert_eq!(send(port, "GET", "/cw/notes.txt", &[], "").body, "saved");
    drop(g);
    assert_eq!(std::fs::read_to_string(app.join("main.qwe")).unwrap(), main);
    assert_eq!(std::fs::read_to_string(app.join("lib/m.qwe")).unwrap(), "let m = 2\n");
}

// Fix round 15

/// A 1x1 PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, 0xde,
    0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0xc9, 0xfe, 0x92, 0xef, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
    0x42, 0x60, 0x82,
];

/// A path named through a `.quillo` in a folder `--allow-path` names keeps that name for every
/// writer: resolved, `../other/.quillo/plugins.lock` (`other/.quillo -> store`) was
/// `store/plugins.lock`, which `http.get`'s `save` and `image.convert` wrote, as only `write_file`
/// looked at the name given (`save_upload` takes no `..`); reading it, and writing `store/` by its
/// own name, are as they were (src/builtins.rs allowed_path; FEEDBACK85.md Q-1340, found by the verdict review of
/// 0cc8547; branch feedback82-85)
#[cfg(unix)]
#[test]
fn a_path_named_through_a_quillo_in_an_allowed_folder_is_refused_to_every_writer() {
    let d = Dir(support::temp_dir("cov8285r3-allow-quillo").canonicalize().unwrap());
    let (app, other, srv) = (d.0.join("app"), d.0.join("other"), d.0.join("srv"));
    for f in [&app, &other.join("store/native"), &srv] {
        std::fs::create_dir_all(f).unwrap();
    }
    std::fs::write(other.join("store/plugins.lock"), "lock").unwrap();
    std::os::unix::fs::symlink("store", other.join(".quillo")).unwrap();
    std::fs::write(app.join("in.png"), PNG).unwrap();
    std::fs::write(srv.join("main.qwe"), "route GET /x = \"EVIL\"\n").unwrap();
    let (gs, sport) = serve_at(&srv, &["main.qwe"], &d.0.join("s.log"));
    let u = format!("http://127.0.0.1:{}/x", sport);
    let main = format!(
        "print(http.get(\"{u}\", {{}}, {{ save: \"../other/.quillo/plugins.lock\", public_only: false }}))\nprint(image.convert(\"in.png\", \"../other/.quillo/native/libp-1.dylib\", {{ format: \"png\" }}))\nprint(image.convert(\"in.png\", \"../other/.quillo/p.png\"))\nprint(write_file(\"../other/.quillo/plugins.lock\", \"EVIL\"))\nprint(read_file(\"../other/.quillo/plugins.lock\"))\nprint(http.get(\"{u}\", {{}}, {{ save: \"../other/store/ok.bin\", public_only: false }}).bytes)\n"
    );
    std::fs::write(app.join("main.qwe"), main).unwrap();
    let (code, out) = quillo_at(&app, &["run", "main.qwe", "--allow-path", "../other"], &[], &d.0.join("a.log"), 60);
    let refused = |what: &str, p: &str| format!("err(\"{}will not write {}: {}\")", what, p, OWN_QUILLO);
    assert_eq!(
        (code, out.lines().collect::<Vec<_>>()),
        (
            Some(0),
            vec![
                refused("http.get: save: ", "../other/.quillo/plugins.lock").as_str(),
                refused("image: ", "../other/.quillo/native/libp-1.dylib").as_str(),
                refused("image: ", "../other/.quillo/p.png").as_str(),
                refused("write_file: ", "../other/.quillo/plugins.lock").as_str(),
                "lock",
                "4",
            ]
        ),
        "{}",
        out
    );
    drop(gs);
    assert_eq!(std::fs::read_to_string(other.join("store/plugins.lock")).unwrap(), "lock");
    assert_eq!(std::fs::read_dir(other.join("store/native")).unwrap().count(), 0);
    assert!(!other.join("store/p.png").exists());
    assert_eq!(std::fs::read_to_string(other.join("store/ok.bin")).unwrap(), "EVIL");
}

/// A `.quillo` made while the program runs is seen within a second: with none at the start,
/// `ln -s qstore .quillo` while it served let `write_file` into `qstore/` and `files()` serve it,
/// as the roots were kept by the files loaded alone (src/builtins.rs roots_seen and look_again;
/// FEEDBACK85.md Q-1340, found by the verdict review of 0cc8547; branch feedback82-85)
#[cfg(unix)]
#[test]
fn a_quillo_made_while_the_program_runs_is_seen() {
    let d = Dir(support::temp_dir("cov8285r3-quillo-later").canonicalize().unwrap());
    let app = d.0.join("app");
    std::fs::create_dir_all(app.join("qstore")).unwrap();
    std::fs::write(app.join("qstore/plugins.lock"), "lock").unwrap();
    std::fs::write(app.join("main.qwe"), format!("{}route GET /all/*p = files(\".\", p)\n", WRITES)).unwrap();
    let (g, port) = serve_at(&app, &["main.qwe"], &d.0.join("a.log"));
    assert_eq!(send(port, "GET", "/w/notes.txt", &[], "").body, "saved");
    assert_eq!(send(port, "GET", "/all/qstore/plugins.lock", &[], "").body, "lock");
    std::os::unix::fs::symlink("qstore", app.join(".quillo")).unwrap();
    // Past the second the noted folders are looked at again within (at least, as a slower machine
    // only waits longer).
    std::thread::sleep(std::time::Duration::from_millis(1500));
    assert_eq!(send(port, "GET", "/w/qstore/plugins.lock", &[], "").body, format!("write_file: will not write qstore/plugins.lock: {}", OWN_QUILLO));
    assert_eq!(send(port, "GET", "/all/qstore/plugins.lock", &[], "").status, 404);
    assert_eq!(send(port, "GET", "/w/notes.txt", &[], "").body, "saved");
    drop(g);
    assert_eq!(std::fs::read_to_string(app.join("qstore/plugins.lock")).unwrap(), "lock");
}

/// The layout is judged again as the program loads where a build (or a prompt) ran after the
/// start judged it: a `.quillo -> .` made while the Rust files were built stopped the start only
/// after the top level ran (src/main.rs run_program and load; FEEDBACK85.md Q-1340, found by the
/// verdict review of 0cc8547; branch feedback82-85)
#[cfg(unix)]
#[test]
fn a_quillo_made_while_the_rust_files_build_stops_the_start_before_the_top_level() {
    use std::os::unix::fs::PermissionsExt;
    let d = Dir(support::temp_dir("cov8285r3-quillo-prebuild").canonicalize().unwrap());
    let app = d.0.join("app");
    std::fs::create_dir_all(&app).unwrap();
    // A cargo that makes the link and builds nothing.
    let cargo = d.0.join("cargo");
    std::fs::write(&cargo, format!("#!/bin/sh\nln -s . \"{}/.quillo\"\nexit 1\n", app.display())).unwrap();
    std::fs::set_permissions(&cargo, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(app.join("a.qwer"), "pub fn a() -> i64 { 1 }\n").unwrap();
    std::fs::write(app.join("b.qwer"), "pub fn b() -> i64 { 2 }\n").unwrap();
    std::fs::write(app.join("main.qwe"), "print(\"TOP\")\nuse \"a.qwer\"\nuse \"b.qwer\"\nprint(a())\n").unwrap();
    let (code, out) = quillo_at(&app, &["run", "--no-check", "main.qwe"], &[("CARGO", cargo.to_str().unwrap())], &d.0.join("a.log"), 120);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("building b.qwer, a.qwer together") || out.contains("building a.qwer, b.qwer together"), "{}", out);
    assert!(!out.lines().any(|l| l == "TOP"), "{}", out);
    assert!(out.contains(".quillo leads to") && out.contains("so it must be a folder of its own"), "{}", out);
}

/// A `--watch` reload judges the layout as a start does: with `.quillo -> ..` made while the
/// program served, a save reloaded it and ran its top level, where a start stops (src/server.rs
/// reload_saying; FEEDBACK85.md Q-1340, found by the verdict review of 0cc8547; branch
/// feedback82-85)
#[cfg(unix)]
#[test]
fn a_reload_judges_the_layout_as_a_start_does() {
    let d = Dir(support::temp_dir("cov8285r3-quillo-reload").canonicalize().unwrap());
    let app = d.0.join("app");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::write(app.join("main.qwe"), "route GET /v = \"1\"\n").unwrap();
    let log = d.0.join("a.log");
    let (g, port) = serve_vars(&app, &["main.qwe", "--watch"], &[], &log);
    assert_eq!(send(port, "GET", "/v", &[], "").body, "1");
    std::os::unix::fs::symlink("..", app.join(".quillo")).unwrap();
    std::fs::write(app.join("main.qwe"), "route GET /v = \"2\"\n").unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(120);
    while !log_of(&log).contains("reloaded") && !log_of(&log).contains(".quillo leads to") {
        assert!(std::time::Instant::now() < deadline, "{}", log_of(&log));
        std::thread::sleep(Duration::from_millis(50));
    }
    let said = log_of(&log);
    assert!(said.contains(".quillo leads to") && said.contains("so it must be a folder of its own") && !said.contains("reloaded"), "{}", said);
    assert_eq!(send(port, "GET", "/v", &[], "").body, "1");
    drop(g);
}
