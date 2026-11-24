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
    for to in [".quillo/plugins.lock", ".quillo/native/libhi-1.dylib", ".QUILLO/native/libhi-2.dylib"] {
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
/// end. 20,000 writes, none held (src/builtins.rs own_target, FEEDBACK85.md Q-1340, found by the
/// review of 9da2632; branch feedback82-85)
#[cfg(unix)]
#[test]
fn a_write_does_not_wait_on_what_a_quillo_holds() {
    let d = Dir(support::temp_dir("cov8285r3-quillo-flat").canonicalize().unwrap());
    let app = d.0.join("app");
    std::fs::create_dir_all(app.join(".quillo/sites")).unwrap();
    std::fs::create_dir_all(app.join("out")).unwrap();
    for i in 0..100_000 {
        std::fs::write(app.join(format!(".quillo/sites/s{}.json", i)), "{}").unwrap();
    }
    std::fs::write(
        app.join("main.qwe"),
        "let worst = 0\nlet last = now()\nfor i in range(0, 20000) {\n  let _w = write_file(\"out/a.txt\", \"x\")\n  let t = now()\n  if t - last > worst {\n    worst = t - last\n  }\n  last = t\n}\nprint(worst < 150)\n",
    )
    .unwrap();
    let (code, out) = quillo_at(&app, &["run", "main.qwe"], &[], &d.0.join("a.log"), 120);
    assert_eq!((code, out.as_str()), (Some(0), "true\n"));
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
