//! The lines Quillo 0.8.7 added that no test ran, the rest of them (slice S4; branch coverage-087):
//! `quillo check`'s GET-that-writes warning (FEEDBACK70.md, Q-965) and what it reads of a group's
//! middleware, an offline group's middleware at run time and in the check (FEEDBACK67.md, B.2), a
//! handler's waits before `notification.ask` (B.3), the group tags' parsing, a tool's schema key
//! with a hidden character (FEEDBACK71.md, Q-980), a service's scheme word from IPv6 and IPv4 in
//! IPv6's clothes (Q-986), a remote service's marks for the worker, `--max-memory` in an events
//! stream, a compiled loop and `read_file` (Q-977), the native plugin cache's folders (Q-983), and
//! `quillo install` into a folder it cannot write (Q-981). Each test says what it must give; every
//! process it starts has a deadline and is stopped by it, and its folders are removed.

mod support;

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

// ---- helpers ----------------------------------------------------------------------------

/// A fresh folder for one test, removed when dropped (its permissions given back first, for a
/// test that took them away).
struct Dir(PathBuf);

impl Dir {
    fn new(name: &str, files: &[(&str, &str)]) -> Dir {
        let d = Dir(support::temp_dir(&format!("cover087-{}", name)));
        for (f, text) in files {
            d.write(f, text);
        }
        d
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn join(&self, p: &str) -> PathBuf {
        self.0.join(p)
    }

    fn write(&self, f: &str, text: &str) {
        let p = self.0.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        #[cfg(unix)]
        open_up(&self.0);
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Gives the user back every folder under `p` (links not followed), so that it can be removed.
#[cfg(unix)]
fn open_up(p: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let Ok(m) = std::fs::symlink_metadata(p) else { return };
    if m.is_dir() {
        let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700));
        if let Ok(rd) = std::fs::read_dir(p) {
            for e in rd.flatten() {
                open_up(&e.path());
            }
        }
    }
}

#[cfg(unix)]
fn chmod(p: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[cfg(unix)]
fn me() -> u32 {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc::geteuid() }
}

/// A finished process: its exit code (None: stopped at its deadline), stdout and stderr.
struct Ran {
    code: Option<i32>,
    out: String,
    err: String,
}

/// Runs `c` to its end, `secs` seconds at most, its output read as it comes (a full pipe would
/// stop it), and kills it at the deadline.
fn run_capped(mut c: Command, secs: u64) -> Ran {
    let mut child = support::Guard::new(c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap());
    let read = |mut r: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut v = Vec::new();
            let _ = r.read_to_end(&mut v);
            String::from_utf8_lossy(&v).into_owned()
        })
    };
    let (out, err) = (read(Box::new(child.0.stdout.take().unwrap())), read(Box::new(child.0.stderr.take().unwrap())));
    let end = Instant::now() + Duration::from_secs(secs);
    let code = loop {
        if let Some(s) = child.0.try_wait().unwrap() {
            break s.code();
        }
        if Instant::now() > end {
            let _ = child.0.kill();
            let _ = child.0.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    Ran { code, out: out.join().unwrap(), err: err.join().unwrap() }
}

fn quillo(d: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_quillo"));
    c.current_dir(d);
    c
}

/// `quillo check <file>` in `d`: whether it passed, and its error and warning lines.
fn check_file(d: &Path, file: &str) -> (bool, Vec<String>) {
    let mut c = quillo(d);
    c.args(["check", file]);
    let r = run_capped(c, 300);
    assert!(r.code.is_some(), "quillo check {} did not end", file);
    let text = r.out + &r.err;
    (r.code == Some(0), text.lines().filter(|l| l.contains(": error: ") || l.contains(": warning: ")).map(String::from).collect())
}

/// `quillo check main.qwe` of `src` (and `more` files beside it).
fn check_src(name: &str, src: &str, more: &[(&str, &str)]) -> (bool, Vec<String>) {
    let d = Dir::new(name, more);
    d.write("main.qwe", src);
    check_file(d.path(), "main.qwe")
}

/// `check_src`'s lines without the warning of a GET that writes and is not declared `route writes`
/// (FEEDBACK70.md, Q-965, decided 2026-11-23: warned by `quillo check`). The tests that use it are
/// about the warning of a GET that writes and reads who the visitor is, written before Q-965's.
fn check_src_who(name: &str, src: &str, more: &[(&str, &str)]) -> (bool, Vec<String>) {
    let (ok, lines) = check_src(name, src, more);
    (ok, lines.into_iter().filter(|l| !l.contains("but is not declared `route writes`")).collect())
}

/// What `quillo check` says of a GET route that writes and reads who the visitor is.
fn get_warned(line: usize, route: &str, writes: &str, reads: &str) -> String {
    format!(
        "main.qwe:{}: warning: `{}` writes ({}) and reads who the visitor is ({}): a link on another site, in an email or in a chat runs it with their session (`SameSite=Lax` cookies go with a followed link). Make it a `POST`, or, if the write is meant (a confirm-your-address link, a sign-in callback, a presence ping), tag it: `route writes GET ...`",
        line, route, writes, reads
    )
}

fn exec(sql: &str) -> String {
    format!("`db.exec(\"{}\")`", sql)
}

/// `quillo run <file> --port 0` in `d` with `args`, its output in `log`, once it says where it
/// listens (60 s at most): the process and its port.
fn serve(d: &Path, file: &str, args: &[&str], log: &str) -> (support::Guard, u16) {
    let out = std::fs::File::create(d.join(log)).unwrap();
    let child = quillo(d)
        .args(["run", file, "--port", "0"])
        .args(args)
        .env("QUILLO_SECRET", "a secret for the cover_misc087 tests, long enough")
        .stdin(Stdio::null())
        .stdout(out.try_clone().unwrap())
        .stderr(out)
        .spawn()
        .unwrap();
    let mut g = support::Guard::new(child);
    let port = support::port_in_log(&mut g.0, &d.join(log), Duration::from_secs(60));
    (g, port)
}

/// The log at `path` once it holds `want`, 10 seconds at most.
fn log_holding(path: &Path, want: &str) -> String {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let log = std::fs::read_to_string(path).unwrap_or_default();
        if log.contains(want) || Instant::now() >= end {
            return log;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// One request to `host`:`port`, read for 30 seconds at most.
fn ask(host: &str, port: u16, path: &str, headers: &[(&str, &str)]) -> support::Answer {
    let mut s = std::net::TcpStream::connect((host, port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    let mut head = format!("GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n", path);
    for (k, v) in headers {
        head.push_str(&format!("{}: {}\r\n", k, v));
    }
    head.push_str("\r\n");
    s.write_all(head.as_bytes()).unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let out = String::from_utf8_lossy(&out).into_owned();
    let (head, body) = out.split_once("\r\n\r\n").unwrap_or((&out, ""));
    let mut lines = head.lines();
    let status = lines.next().and_then(|l| l.split(' ').nth(1)).and_then(|c| c.parse().ok()).unwrap_or(0);
    let headers = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string())).collect();
    support::Answer { status, headers, body: body.to_string() }
}

// ---- quillo check: a GET route that writes (FEEDBACK70.md, Q-965) ------------------------

const NOTES: &str = "db.open(\":memory:\")\ndb.exec(\"create table notes (id integer primary key, owner text)\")\n";

/// A route's writes are listed three at a time, and how many more: three are said whole, four
/// end with "and 1 more", five with "and 2 more"; and the grouped warning of three routes that
/// share the same four writes says so too (src/check.rs `get_writes_said`'s listing,
/// FEEDBACK70.md, Q-965; coverage of 0.8.7; branch coverage-087).
#[test]
fn a_get_routes_writes_past_three_are_counted() {
    let w = ["delete from notes where owner = ?", "insert into notes (owner) values (?)", "update notes set owner = ? where id = 1", "delete from notes where id = 2", "delete from notes where id = 3"];
    let mut src = NOTES.to_string();
    src.push_str("fn wipe_all(u) {\n");
    for s in &w[..4] {
        src.push_str(&format!("    db.exec(\"{}\", u)\n", s));
    }
    src.push_str("}\n");
    let mut want = Vec::new();
    let mut routes = Vec::new();
    for (path, n) in [("/three", 3), ("/four", 4), ("/five", 5)] {
        let line = src.lines().count() + 1;
        src.push_str(&format!("route GET {} {{\n    let u = session().user\n", path));
        for s in &w[..n] {
            src.push_str(&format!("    db.exec(\"{}\", u)\n", s));
        }
        src.push_str("    return \"ok\"\n}\n");
        let mut listed: Vec<String> = w[..3].iter().map(|s| exec(s)).collect();
        if n > 3 {
            listed.push(format!("and {} more", n - 3));
        }
        routes.push(get_warned(line, &format!("GET {}", path), &listed.join("; "), "`session()`"));
    }
    let mut lines = Vec::new();
    for g in ["/g1", "/g2", "/g3"] {
        lines.push(src.lines().count() + 1);
        src.push_str(&format!("route GET {} {{\n    wipe_all(session().user)\n    return \"ok\"\n}}\n", g));
    }
    let listed: Vec<String> = w[..3].iter().map(|s| format!("{}, in `wipe_all`", exec(s))).collect();
    want.push(format!(
        "main.qwe:4: warning: these writes ({}; and 1 more) are done by 3 GET routes that read who the visitor is: `GET /g1` (line {}), `GET /g2` (line {}) and `GET /g3` (line {}). A link on another site, in an email or in a chat runs them with the visitor's session (`SameSite=Lax` cookies go with a followed link). Make them `POST`s, or, if the write is meant (a presence ping, a page's count of views), tag them: `route writes GET ...`, `routes writes /...`",
        listed.join("; "),
        lines[0],
        lines[1],
        lines[2]
    ));
    want.extend(routes);
    assert_eq!(check_src("get-more", &src, &[]), (true, want), "{}", src);
}

/// A request pattern of alternatives is looked through: one alternative that takes the headers
/// or the cookies is a read, as a record that takes them is, the first, the last of three, and
/// under `as` too; alternatives that take neither, and a route tagged `writes`, say nothing
/// (src/check.rs `pattern_takes`; coverage of 0.8.7, Q-1104; branch coverage-087).
#[test]
fn a_get_routes_alternative_request_patterns_are_looked_through() {
    let route = |path: &str, pat: &str| format!("route GET {} {} {{\n    db.exec(\"delete from notes\")\n    return \"x\"\n}}\n", path, pat);
    let cases = [
        ("/one", "{ headers: h }", Some("headers")),
        ("/alt", "{ query: q } | { headers: h }", Some("headers")),
        ("/both", "{ cookies: c } | { headers: h }", Some("cookies")),
        ("/third", "{ query: q } | { json: j } | { cookies: { sid: str } }", Some("cookies")),
        ("/as", "{ headers: h } as r", Some("headers")),
        ("/neither", "{ query: q } | { json: j }", None),
        ("/plain", "{ query: q }", None),
    ];
    let mut src = NOTES.to_string();
    let mut want = Vec::new();
    for (path, pat, read) in cases {
        if let Some(f) = read {
            want.push(get_warned(src.lines().count() + 1, &format!("GET {}", path), &exec("delete from notes"), &format!("its request pattern takes the request's {}", f)));
        }
        src.push_str(&route(path, pat));
    }
    src.push_str("route writes GET /meant { cookies: c } | { headers: h } {\n    db.exec(\"delete from notes\")\n    return \"x\"\n}\n");
    assert_eq!(check_src_who("get-alt", &src, &[]), (true, want), "{}", src);
}

/// A field of the request taken by `let { ... } = request` is named as `request.<field>`, one of
/// nobody's (`path`) is no read; and a function that returns early with a bare `return` is read
/// on past it (src/check.rs `GetWalk::block`, 10446 and 10475; coverage of
/// 0.8.7; branch coverage-087).
#[test]
fn a_get_routes_field_of_the_request_and_an_early_return_are_read() {
    let src = format!(
        "{}fn wipe(u) {{\n    if u == null {{\n        return\n    }}\n    db.exec(\"delete from notes where owner = ?\", u)\n}}\nroute GET /mcp {{\n    let {{ mcp }} = request\n    db.exec(\"delete from notes\")\n    return str(mcp)\n}}\nroute GET /early {{\n    wipe(session().user)\n    return \"ok\"\n}}\nroute GET /path {{\n    let {{ path }} = request\n    db.exec(\"delete from notes\")\n    return path\n}}\n",
        NOTES
    );
    assert_eq!(
        check_src_who("get-field", &src, &[]),
        (
            true,
            vec![
                get_warned(9, "GET /mcp", &exec("delete from notes"), "`request.mcp`"),
                get_warned(14, "GET /early", &format!("{}, in `wipe`", exec("delete from notes where owner = ?")), "`session()`"),
            ]
        )
    );
}

/// A group's middleware the check cannot read, a field of a map (`gates.team`) or a call that is
/// no function of the program's (`gates.make("admin")`), is no sure read, though each reads a
/// header; one made by a function of the program's is read in that function; and `__answer`
/// written by hand, given no call, is refused (src/check.rs `GetWalk::middleware` 10654-10656
/// and `request_at` 10550; coverage of 0.8.7; branch coverage-087).
#[test]
fn a_groups_middleware_the_check_cannot_read_reads_nothing() {
    let src = format!(
        r#"{}let gates = {{ team: (req) => req.headers["x-team"] == null ? redirect("/") : req, make: (role) => (req) => req.headers["x-role"] == role ? req : redirect("/") }}
fn make(role: str) = (request) => request.headers["x-role"] == role ? request : redirect("/")
routes /f | gates.team {{
    route GET /a {{
        db.exec("delete from notes")
        return "a"
    }}
}}
routes /m | gates.make("admin") {{
    route GET /b {{
        db.exec("delete from notes")
        return "b"
    }}
}}
routes /g | make("admin") {{
    route GET /c {{
        db.exec("delete from notes")
        return "c"
    }}
}}
route GET /d {{
    __answer(1, "x")
    db.exec("delete from notes where owner = ?", session().user)
    return "d"
}}
"#,
        NOTES
    );
    let (ok, lines) = check_src_who("get-middleware", &src, &[]);
    assert!(!ok);
    // The hand-written `__answer`'s error, whose words are not this test's (Q-1108 is open).
    assert_eq!(lines.len(), 3, "{:?}", lines);
    assert_eq!(
        lines[..2],
        [
            get_warned(18, "GET /g/c", &exec("delete from notes"), "the `x-role` header, in its group's middleware"),
            get_warned(23, "GET /d", &exec("delete from notes where owner = ?"), "`session()`"),
        ]
    );
    assert!(lines[2].starts_with("main.qwe:24: error: "), "{}", lines[2]);
}

/// A service's server fn that writes in its own code and reads who the visitor is in a function
/// it calls is one that surely writes: a GET route of the app calling it is said; one that only
/// reads is not (src/check.rs `get_facts` with the first write found, 5752; coverage of 0.8.7;
/// branch coverage-087).
#[test]
fn a_services_server_fn_that_writes_and_then_reads_is_a_write() {
    let billing = r#"db.open(":memory:")
db.exec("create table charges (who text)")
fn who() = session().user
server fn charge() {
    db.exec("insert into charges (who) values ('x')")
    return who()
}
server fn peek() {
    return who()
}
"#;
    let src = format!(
        "{}service billing = \"billing.qwe\"\nroute GET /pay {{\n    let c = billing.charge()\n    return `${{c}} ${{session().user}}`\n}}\nroute GET /look {{\n    let p = billing.peek()\n    return `${{p}} ${{session().user}}`\n}}\n",
        NOTES
    );
    assert_eq!(
        check_src("get-service", &src, &[("billing.qwe", billing)]),
        (true, vec![get_warned(4, "GET /pay", "`billing.charge`, a call of the service `billing`", "`session()`")])
    );
}

// ---- quillo check: offline groups' middleware, and handlers' waits (FEEDBACK67.md) --------

/// An offline group's middleware, read as the server reads it: a lambda written in place, a
/// function of the program's, or a lambda a top-level `let` holds, that reads a header makes its
/// pages not kept, said; one that reads the session alone says nothing. Middleware the server
/// cannot follow, whose pages it never keeps, is said too: a field of a map (`gates.team`), a
/// builtin (`unwrap`), a call that makes the function (`make("admin")`); a `let` that holds what a
/// call gave may hold a function the server reads, and is not said. `__answer` written by hand,
/// given no call, is refused (src/check.rs `beyond_in`, 726-730, and `unkept_by_middleware`;
/// coverage of 0.8.7, Q-1102; branch coverage-087).
#[test]
fn an_offline_groups_middleware_is_read_by_the_check_as_the_server_reads_it() {
    let src = r#"fn gate(request) {
    return request.headers["x-team"] == null ? redirect("/") : request
}
fn make(role: str) = (request) => request.headers["x-role"] == role ? request : redirect("/")
let gates = { team: (request) => request.headers["x-team"] == null ? redirect("/") : request }
let held = (request) => request.headers["x-team"] == null ? redirect("/") : request
let held_s = (request) => session().user == null ? redirect("/") : request
let made = make("admin")
routes offline /l | (request) => request.headers["x-team"] == null ? redirect("/") : request {
    route GET /a = <html><body>a</body></html>
}
routes offline /s | (request) => session().user == null ? redirect("/") : request {
    route GET /b = <html><body>{session().user}</body></html>
}
routes offline /n | gate {
    route GET /c = <html><body>c</body></html>
}
routes offline /f | gates.team {
    route GET /d = <html><body>d</body></html>
}
routes offline /u | unwrap {
    route GET /e = <html><body>e</body></html>
}
routes offline /m | make("admin") {
    route GET /g = <html><body>g</body></html>
}
routes offline /h | held {
    route GET /i = <html><body>i</body></html>
}
routes offline /hs | held_s {
    route GET /j = <html><body>{session().user}</body></html>
}
routes offline /md | made {
    route GET /k = <html><body>k</body></html>
}
"#;
    let not_kept = |line: u32, route: &str, why: &str| format!("main.qwe:{}: warning: `route offline {}` reads who the visitor is other than through `session()` ({}): its page is not kept for offline, and the offline page is shown in its place; read who they are with `session()`", line, route, why);
    let reads = "a cookie, a header, the request's own fields of theirs";
    let unfollowed = |name: &str| format!("its group's middleware, `{}`, is no function of the program's or lambda written in place, which Quillo follows, so it counts as such a read", name);
    assert_eq!(
        check_src("offline-lambda", src, &[]),
        (
            true,
            vec![
                not_kept(10, "GET /l/a", reads),
                not_kept(16, "GET /n/c", reads),
                not_kept(19, "GET /f/d", &unfollowed("gates.team")),
                not_kept(22, "GET /u/e", &unfollowed("unwrap")),
                not_kept(25, "GET /m/g", &unfollowed("make(...)")),
                not_kept(28, "GET /h/i", reads),
            ]
        )
    );
    // The same middleware on a group that is not offline says nothing of it.
    let (ok, lines) = check_src("online-unfollowed", "routes /u | unwrap {\n    route GET /e = <html><body>e</body></html>\n}\nroutes /f | [unwrap][0] {\n    route GET /x = <html><body>x</body></html>\n}\n", &[]);
    assert!(ok && lines.is_empty(), "{:?}", lines);
    // An index, unnamed: said without a name.
    let (_, lines) = check_src("offline-index", "routes offline /f | [unwrap][0] {\n    route GET /x = <html><body>x</body></html>\n}\n", &[]);
    assert_eq!(lines, [not_kept(2, "GET /f/x", "its group's middleware is no function of the program's or lambda written in place, which Quillo follows, so it counts as such a read")]);
    // `__answer` written by hand: refused, in words that are not this test's (Q-1108 is open).
    let (ok, lines) = check_src("offline-answer", "route offline GET /e {\n    __answer(1, \"x\")\n    return <html><body>e</body></html>\n}\n", &[]);
    assert!(!ok && lines.len() == 1 && lines[0].starts_with("main.qwe:2: error: "), "{:?}", lines);
}

/// A mounted program that is not UTF-8 is said by its own check, and its routes are not read
/// for offline tags; one that is, is (src/check.rs, the mount's `read_text`, 2650; coverage of
/// 0.8.7; branch coverage-087).
#[test]
fn a_mounted_program_that_is_not_utf8_is_said_once() {
    let d = Dir::new("mount-bytes", &[("main.qwe", "mount /blog = \"blog.qwe\"\nroute GET / = \"home\"\n")]);
    std::fs::write(d.join("blog.qwe"), b"route offline GET / = \"\xff\xfe\"\n").unwrap();
    assert_eq!(check_file(d.path(), "main.qwe"), (false, vec!["./blog.qwe: error: cannot read file: it is not UTF-8 text: byte 0xFF at line 1, column 24 (save it as UTF-8)".to_string()]));
    d.write("blog.qwe", "route offline GET / = <html><body>é</body></html>\n");
    assert_eq!(
        check_file(d.path(), "main.qwe"),
        (true, vec!["./blog.qwe:1: warning: `route offline GET /` is in a program mounted at `/blog`, whose pages Quillo's service worker does not keep: put the route in the main program".to_string()])
    );
}

/// In a handler, a bare `return`, a function written there (not called before the ask) and a
/// `reply` are walked in order: the ask after a wait is said, the one before none is not
/// (src/check.rs `Waits::stmts` 11213 and `Waits::expr` 11280; coverage of 0.8.7; branch
/// coverage-087).
#[test]
fn a_handlers_return_local_fn_and_reply_are_walked_for_waits() {
    let src = r#"server fn save(n: int) -> int = n
component Alerts() {
    state on = notification.state()
    <div>
        <button @click={() => {
            let n = save(1)
            if n > 5 {
                return
            }
            on = notification.ask()
        }}>one</button>
        <button @click={() => {
            fn later() {
                return save(2)
            }
            on = notification.ask()
            on = str(later())
        }}>two</button>
        <button @click={() => {
            on = str(save(3))
            reply 200 notification.ask()
        }}>three</button>
    </div>
}
route GET / = <html><body><Alerts/></body></html>
"#;
    let late = |line: u32| format!("main.qwe:{}: warning: `notification.ask` comes after `save` waits in this handler: browsers show the permission prompt only close to the click, so ask first, then wait", line);
    assert_eq!(
        check_src("waits", src, &[]),
        (false, vec![late(10), "main.qwe:21: error: `reply` only works on the server, but <Alerts> also runs in the browser".to_string(), late(21)])
    );
}

// ---- the group tags (src/parser.rs `group_tags`) -------------------------------------------

/// A group's tag written against its path is said for what it is: the space it needs, at the top
/// level and inside a group, after another tag too; a group's path without its `/` says where
/// the `/` goes, with the path as written; a tab after a tag is a space, and `route offline/x`
/// says the method it lacks (src/parser.rs `group_tags` and `group_slip`; coverage of 0.8.7,
/// Q-1103; branch coverage-087).
#[test]
fn a_group_tag_without_a_space_after_it_says_so() {
    let tag = |line: u32, t: &str, written: &str| format!("main.qwe:{}: error: `{}` is a tag, and a space goes between it and the group's path: `routes {} {{ ... }}`", line, t, written);
    for (src, want) in [
        ("routes offline/x {\n    route GET /a = <html><body>a</body></html>\n}\n".to_string(), tag(1, "offline", "offline /x")),
        ("routes writes/api/v1|g {\n    route GET /a = \"a\"\n}\n".to_string(), tag(1, "writes", "writes /api/v1")),
        ("routes offline writes/x {\n    route GET /a = \"a\"\n}\n".to_string(), tag(1, "writes", "offline writes /x")),
        ("routes /a {\n    routes offline/x {\n        route GET /b = \"b\"\n    }\n}\n".to_string(), tag(2, "offline", "offline /x")),
        ("routes writes-x {\n    route GET /a = \"a\"\n}\n".to_string(), "main.qwe:1: error: a group's path starts with `/`: `routes /writes-x { ... }`".to_string()),
        ("routes café.v2 | g {\n    route GET /a = \"a\"\n}\n".to_string(), "main.qwe:1: error: a group's path starts with `/`: `routes /café.v2 { ... }`".to_string()),
        ("routes admin { route GET /x = 1 }\n".to_string(), "main.qwe:1: error: a group's path starts with `/`: `routes /admin { ... }`".to_string()),
        ("route offline/x = \"a\"\n".to_string(), "main.qwe:1: error: `offline` comes before the method, and a page kept for offline is a GET: `route offline GET /menu = ...`".to_string()),
    ] {
        assert_eq!(check_src("tags", &src, &[]), (false, vec![want]), "{}", src);
    }
    for src in ["routes writes\t/x {\n    route GET /a = \"a\"\n}\n", "routes offline  /x {\n    route GET /a = <html><body>a</body></html>\n}\n", "let routes = [1]\nprint(routes)\n"] {
        let (ok, lines) = check_src("tags-ok", src, &[]);
        assert!(ok && lines.is_empty(), "{}: {:?}", src, lines);
    }
}

// ---- a tool's schema (FEEDBACK71.md, Q-980) -------------------------------------------------

/// A key of an object in a tool's schema (a map or a list of maps given as a default) with a
/// character a reader does not see is said with where it is (src/mcp.rs `unseen_in_json`,
/// 1034-1035; coverage of 0.8.7; branch coverage-087).
#[test]
fn a_hidden_character_in_a_schemas_key_is_said() {
    let said = |at: &str, c: &str| format!("main.qwe:3: error: the schema of tool `pick` is what AI clients are told of its arguments and result, and a key in `{}` holds {}, which a reader of the file does not see: take it out", at, c);
    for (param, at, c) in [
        ("o: map = { \"a\u{200B}b\": 1 }", "inputSchema/properties/o/default", "U+200B (zero width space)"),
        ("o: list = [{ \"k\": \"v\" }, { \"z\u{2066}\": 2 }]", "inputSchema/properties/o/default/1", "U+2066 (left-to-right isolate)"),
        ("o: map = { \"tag\u{E0041}\": { \"in\u{200B}ner\": 1 } }", "inputSchema/properties/o/default", "U+E0041 (a tag character)"),
    ] {
        let src = format!("//! Notes.\n/// Picks one.\nfn mcp pick({}) -> str with time = \"x\"\n", param);
        assert_eq!(check_src("schema-key", &src, &[]), (false, vec![said(at, c)]), "{}", param);
    }
    // A key a reader sees, unicode too, is fine.
    let (ok, lines) = check_src("schema-key-ok", "//! Notes.\n/// Picks one.\nfn mcp pick(o: map = { \"café 日本\": [1, { \"ü\": 2 }] }) -> str with time = \"x\"\n", &[]);
    assert!(ok && lines.is_empty(), "{:?}", lines);
}

// ---- servers: offline groups' middleware, a remote service's marks (FEEDBACK67.md, B.2) ----

/// The cookies a sign-in at `/in/<who>` sets, as a request sends them back.
fn signed_in(port: u16, who: &str) -> String {
    let a = support::send(port, "GET", &format!("/in/{}", who), &[], "");
    assert_eq!(a.status, 200, "{}", a.body);
    let jar: Vec<String> = a.headers.iter().filter(|(k, _)| k == "set-cookie").map(|(_, v)| v.split(';').next().unwrap().to_string()).collect();
    assert!(jar.iter().any(|c| c.contains("quillo_session=")) && jar.iter().any(|c| c.starts_with("quillo_tag=")), "{:?}", jar);
    jar.join("; ")
}

/// An offline group's pages are kept by what its middleware reads, as the server reads the
/// middleware: a lambda that reads a header, a field of a map and a builtin (which it cannot
/// read, a call that makes the function too) keep nothing; a lambda that reads the session alone,
/// written in place or held by a `let`, and a named one that does before a body that is a block or
/// a call (`respond(...)`), keep the page for that session: as `quillo check` says (src/server.rs
/// `middleware_beyond`, 1304-1315; coverage of 0.8.7, Q-1102; branch coverage-087).
#[test]
fn an_offline_groups_middleware_decides_what_is_kept() {
    let src = r#"let gates = { team: (request) => request.headers["x-team"] == null ? redirect("/") : request }
fn signed(request) = session().user == null ? redirect("/") : request
fn make(role: str) = (request) => request.headers["x-role"] == role ? request : redirect("/")
let held = (request) => request.headers["x-team"] == null ? redirect("/") : request
let held_s = (request) => session().user == null ? redirect("/") : request
routes offline /m | make("admin") {
    route GET /a = <html><body>m</body></html>
}
routes offline /k | held {
    route GET /a = <html><body>k</body></html>
}
routes offline /ks | held_s {
    route GET /a = <html><body>{session().user}</body></html>
}
route GET /in/:who {
    set_session({ user: who })
    return "in"
}
routes offline /h | (request) => request.headers["x-team"] == null ? redirect("/") : request {
    route GET /a = <html><body>h</body></html>
}
routes offline /s | (request) => session().user == null ? redirect("/") : request {
    route GET /b = <html><body>{session().user}</body></html>
}
routes offline /f | gates.team {
    route GET /c = <html><body>f</body></html>
}
routes offline /u | unwrap {
    route GET /d = <html><body>u</body></html>
}
routes offline /p | signed {
    route GET /e {
        print("e")
        return <html><body>{session().user}</body></html>
    }
    route GET /g = respond(200, <html><body>{session().user}</body></html>)
}
"#;
    let d = Dir::new("offline-mw", &[("main.qwe", src)]);
    let (_g, port) = serve(d.path(), "main.qwe", &[], "server.log");
    let jar = signed_in(port, "ana");
    let kept = |path: &str| {
        let a = support::send(port, "GET", path, &[("cookie", &jar), ("x-team", "t"), ("x-role", "admin")], "");
        assert_eq!(a.status, 200, "{}: {}", path, a.body);
        assert!(a.body.contains("/_quillo/sw.js"), "{}: the page registers the worker: {}", path, a.body);
        a.header("x-quillo-offline").map(str::to_string)
    };
    let private = Some("private".to_string());
    assert_eq!([kept("/h/a"), kept("/s/b"), kept("/f/c"), kept("/u/d"), kept("/p/e"), kept("/p/g")], [None, private.clone(), None, None, private.clone(), private.clone()]);
    // As the check now says (Q-1102): a call that makes the function and a `let`'s lambda that
    // reads a header keep nothing, a `let`'s lambda that reads the session alone keeps the page.
    assert_eq!([kept("/m/a"), kept("/k/a"), kept("/ks/a")], [None, None, private]);
    // Without the session, the guard sends the visitor away.
    assert_eq!(support::send(port, "GET", "/s/b", &[], "").status, 303);
    assert!(log_holding(&d.join("server.log"), "GET /p/e").contains("GET /p/e -> 200"));
}

/// A service with a prefix that runs elsewhere (`--service-url`) may mark its answers for the
/// worker as it likes: the app passes them on without those marks once it has offline routes, and
/// marks its own (src/server.rs `handle`, the remote mount's `pwa::unmarked`, 619; coverage of
/// 0.8.7; branch coverage-087).
#[test]
fn a_remote_services_marks_for_the_worker_are_taken_off() {
    let billing = "route GET /forged = respond(200, <html><body><h1>Billing</h1></body></html>, { \"x-quillo-offline\": \"1\", \"x-quillo-session\": \"abc\" })\n";
    let d = Dir::new("remote-marks", &[("billing.qwe", billing), ("main.qwe", "service /billing = \"billing.qwe\"\nroute offline GET / = <html><body><h1>Home</h1></body></html>\n")]);
    let (_b, bp) = serve(d.path(), "billing.qwe", &[], "billing.log");
    let own = support::send(bp, "GET", "/forged", &[], "");
    assert_eq!((own.header("x-quillo-offline"), own.header("x-quillo-session")), (Some("1"), Some("abc")));
    let (_m, port) = serve(d.path(), "main.qwe", &["--service-url", &format!("billing=http://127.0.0.1:{}", bp)], "main.log");
    let a = support::send(port, "GET", "/billing/forged", &[], "");
    assert!(a.status == 200 && a.body.contains("<h1>Billing</h1>"), "{} {}", a.status, a.body);
    assert_eq!((a.header("x-quillo-offline"), a.header("x-quillo-session")), (None, None), "{:?}", a.headers);
    let home = support::send(port, "GET", "/", &[], "");
    assert_eq!((home.status, home.header("x-quillo-offline")), (200, Some("1")));
}

/// `__answer` written by hand in an offline route, given no call: the check refuses it; run
/// anyway, the server takes it as middleware it cannot read, which keeps none of those pages,
/// while a page of no middleware is kept. What such a call answers is not this test's (Q-1108 is
/// open) (src/server.rs `middleware_beyond`, 1315; coverage of 0.8.7; branch coverage-087).
#[test]
fn a_hand_written_answer_without_a_call_is_middleware_the_server_cannot_read() {
    let src = "route offline GET /x {\n    __answer(1, \"x\")\n    return <html><body>x</body></html>\n}\nroute offline GET /y {\n    __answer(null, \"y\")\n    return <html><body>y</body></html>\n}\nroute offline GET /z = <html><body>z</body></html>\n";
    let d = Dir::new("hand-answer", &[("main.qwe", src)]);
    let (ok, lines) = check_file(d.path(), "main.qwe");
    assert!(!ok && lines.len() == 2 && lines[0].starts_with("main.qwe:2: error: ") && lines[1].starts_with("main.qwe:6: error: "), "{:?}", lines);
    let (_g, port) = serve(d.path(), "main.qwe", &["--no-check"], "server.log");
    for path in ["/x", "/y"] {
        let a = support::send(port, "GET", path, &[], "");
        assert_eq!(a.header("x-quillo-offline"), None, "{}: {} {:?}", path, a.status, a.headers);
    }
    let z = support::send(port, "GET", "/z", &[], "");
    assert_eq!((z.status, z.header("x-quillo-offline")), (200, Some("1")));
}

// ---- a service's scheme word over IPv6 (FEEDBACK70.md, Q-986) -----------------------------

const SERVICE_SECRET: &str = "0123456789abcdef0123456789abcdef";

/// The app's word on a scheme as `quillo run --split` signs it, with the tests' service secret.
fn word(scheme: &str) -> String {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let sig = quillo::crypto::hex(&quillo::crypto::hmac_sha256(SERVICE_SECRET.as_bytes(), format!("quillo-scheme-1\n{}\n{}", scheme, t).as_bytes()));
    format!("{};t={};sig={}", scheme, t, sig)
}

fn secure(a: &support::Answer) -> bool {
    a.header("set-cookie").unwrap_or("").split(';').any(|p| p.trim() == "Secure")
}

/// A service as `--split` starts it (`--as-service`, its stdin its app's), listening on `::`: the
/// word its app signs is believed from this machine over IPv6 (`::1`) and over IPv4, which a
/// listener on `::` sees in IPv6's clothes (`::ffff:127.0.0.1`); an `http` word, or none, is not
/// https (src/server.rs `local_peer`, 391-392; coverage of 0.8.7; branch coverage-087).
#[test]
fn a_split_services_word_is_believed_from_this_machine_over_ipv6() {
    if std::net::TcpListener::bind(("::1", 0)).is_err() {
        eprintln!("no IPv6 here: passed over");
        return;
    }
    let d = Dir::new("ipv6-word", &[("billing.qwe", "route GET /ip = request.ip\nroute GET /del {\n    delete_cookie(\"theme\")\n    return \"deleted\"\n}\n")]);
    let log = d.join("service.log");
    let child = quillo(d.path())
        .args(["run", "billing.qwe", "--port", "0", "--host", "::", "--as-service"])
        .env("QUILLO_SECRET", "a secret for the cover_misc087 tests, long enough")
        .env("QUILLO_SERVICE_SECRET", SERVICE_SECRET)
        .env("QUILLO_STOP_WITH_APP", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    let mut g = support::Guard::new(child);
    let end = Instant::now() + Duration::from_secs(60);
    let port = loop {
        if let Some(p) = support::port_said_in(&log) {
            break p;
        }
        if let Ok(Some(st)) = g.0.try_wait() {
            panic!("the service stopped: {}: {}", st, std::fs::read_to_string(&log).unwrap_or_default());
        }
        assert!(Instant::now() < end, "the service did not start: {}", std::fs::read_to_string(&log).unwrap_or_default());
        std::thread::sleep(Duration::from_millis(20));
    };
    while std::net::TcpStream::connect(("::1", port)).is_err() {
        assert!(Instant::now() < end, "the service did not listen");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(ask("::1", port, "/ip", &[]).body, "::1");
    let (https, http) = (word("https"), word("http"));
    for _ in 0..2 {
        assert!(secure(&ask("::1", port, "/del", &[("x-quillo-scheme", &https)])));
        assert!(!secure(&ask("::1", port, "/del", &[("x-quillo-scheme", &http)])));
        assert!(!secure(&ask("::1", port, "/del", &[])));
    }
    // IPv4 to a listener on `::`, where the system maps it.
    if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
        assert_eq!(ask("127.0.0.1", port, "/ip", &[]).body, "::ffff:127.0.0.1");
        assert!(secure(&ask("127.0.0.1", port, "/del", &[("x-quillo-scheme", &https)])));
        assert!(!secure(&ask("127.0.0.1", port, "/del", &[("x-quillo-scheme", &http)])));
    }
}

/// Many forms with `X-Forwarded-Proto` from a peer `--trust-proxy` does not name, all at once on
/// a fresh server of four instances (four loops reading requests at once), the header first and
/// 98 others after it (which the gate reads past, from the last, to find it): each is taken, and
/// the line that says which address to trust is said once, whichever request read the header
/// first. The assertions hold whatever the order; only whether two requests read the header at
/// the same time, and so run the early return for the one that lost (src/httpd.rs
/// `forwarded_proto_not_believed`, 2263), is chance: it did in each run here (coverage of 0.8.7;
/// branch coverage-087).
#[test]
fn the_forwarded_proto_line_is_said_once_when_many_requests_come_at_once() {
    use std::sync::{Arc, Barrier};
    let d = Dir::new("proto-race", &[("main.qwe", "route POST /notes { form: { text: str } } = \"saved\"\n")]);
    let said = "quillo: a request came with X-Forwarded-Proto from 127.0.0.1, which --trust-proxy does not name: if that address is your proxy, --trust-proxy 127.0.0.1; if not, a visitor sent it and it is ignored";
    let mut head = "POST /notes HTTP/1.1\r\nX-Forwarded-Proto: https\r\nHost: localhost\r\nOrigin: http://localhost\r\nConnection: close\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: 7\r\n".to_string();
    for i in 0..93 {
        head.push_str(&format!("x-forwarded-p{:04}: {}\r\n", i, i));
    }
    head.push_str("\r\ntext=h");
    let head = Arc::new(head);
    for round in 0..3 {
        let log = format!("race-{}.log", round);
        let (_g, port) = serve(d.path(), "main.qwe", &["--instances", "4"], &log);
        let n = 64;
        let at_once = Arc::new(Barrier::new(n));
        let all: Vec<_> = (0..n)
            .map(|_| {
                let (at_once, head) = (at_once.clone(), head.clone());
                std::thread::spawn(move || {
                    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
                    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
                    s.write_all(head.as_bytes()).unwrap();
                    at_once.wait();
                    s.write_all(b"i").unwrap();
                    let mut out = String::new();
                    let _ = s.read_to_string(&mut out);
                    out
                })
            })
            .collect();
        for t in all {
            let out = t.join().unwrap();
            assert!(out.starts_with("HTTP/1.1 200") && out.ends_with("\r\n\r\nsaved"), "{}", out);
        }
        let text = log_holding(&d.join(&log), said);
        assert_eq!(text.matches(said).count(), 1, "{}", text);
    }
}

// ---- --max-memory (FEEDBACK71.md, Q-977) ----------------------------------------------------

/// A chunked body's data, and whether it ended with its last chunk.
fn dechunked(body: &str) -> (String, bool) {
    let (mut out, mut rest) = (String::new(), body);
    while let Some((size, after)) = rest.split_once("\r\n") {
        let Ok(n) = usize::from_str_radix(size.trim(), 16) else { break };
        if n == 0 {
            return (out, true);
        }
        if after.len() < n {
            out.push_str(after);
            break;
        }
        out.push_str(&after[..n]);
        rest = after[n..].strip_prefix("\r\n").unwrap_or(&after[n..]);
    }
    (out, false)
}

const MEM: &str = r#"fn doubled_once(n: int) -> int {
    let s = "a"
    for i in range(0, n) {
        if i == n - 2 {
            s += s
        } else {
            s += "b"
        }
    }
    return len(s)
}
fn many(n: int) -> list {
    let one = "q".repeat(100000)
    let xs = []
    for i in range(0, n) {
        xs = xs | push(one)
    }
    return xs
}
route GET /twice ? { n: int } = `${doubled_once(n)}`
route GET /ev ? { n: int } = events((send) => {
    send("start")
    send(many(n))
    send("end")
})
route GET /dir = is_err(read_file("sub")) ? "not read" : "read"
route GET /why = str(read_file("sub"))
"#;

/// Under `--max-memory`: a compiled loop that adds a text to itself (which then cannot grow in
/// place) gives what the interpreter gives; an events stream whose value's JSON would pass the
/// limit ends there, what was sent staying sent; `read_file` of a folder is an error value, its
/// reason in the log, as without the limit (src/jitloop.rs `text_add`, 781-782; src/stream.rs
/// `encode`, 256; src/builtins.rs `read_file`, 3114; coverage of 0.8.7; branch coverage-087).
#[test]
fn max_memory_in_a_compiled_loop_an_events_stream_and_a_folder_read() {
    let d = Dir::new("mem", &[("mem.qwe", MEM)]);
    std::fs::create_dir_all(d.join("sub")).unwrap();
    let (_g, port) = serve(d.path(), "mem.qwe", &["--max-memory", "1MB"], "mem.log");
    let (_p, plain) = serve(d.path(), "mem.qwe", &[], "plain.log");
    let get = |port: u16, path: &str| {
        let a = support::send(port, "GET", path, &[], "");
        (a.status, a.body)
    };
    // 1 + 598 "b"s, doubled, and one more: past the 256 rounds after which the loop is compiled.
    for (n, want) in [(600, "1199"), (300, "599"), (10, "19"), (2, "3")] {
        assert_eq!(get(port, &format!("/twice?n={}", n)), (200, want.to_string()), "{}", n);
        assert_eq!(get(plain, &format!("/twice?n={}", n)), (200, want.to_string()), "{}", n);
    }
    // 20 texts of 100 KB are 2 MB of JSON: the stream ends after its first event, its chunks
    // with no last one.
    let (status, body) = get(port, "/ev?n=20");
    assert_eq!((status, dechunked(&body)), (200, ("data: \"start\"\n\n".to_string(), false)));
    let log = log_holding(&d.join("mem.log"), "error while streaming an answer");
    assert!(log.contains("quillo: error while streaming an answer: mem.qwe:23: this request's value would be bigger than --max-memory allows (1048576 bytes)"), "{}", log);
    // 5 of them are 500 KB: all sent; without the limit, the 20 too.
    for (p, n) in [(port, 5), (plain, 20)] {
        let (status, body) = get(p, &format!("/ev?n={}", n));
        let (data, whole) = dechunked(&body);
        assert!(status == 200 && whole && data.starts_with("data: \"start\"\n\ndata: [\"qqq") && data.ends_with("\"]\n\ndata: \"end\"\n\n"), "{} {}", status, data.len());
        assert_eq!(data.matches(&"q".repeat(100000)).count(), n);
    }
    // A folder is no file, with the limit or without it.
    for (p, log) in [(port, "mem.log"), (plain, "plain.log")] {
        assert_eq!(get(p, "/dir"), (200, "not read".to_string()));
        // Its reason is the log's, not the visitor's.
        assert_eq!(get(p, "/why"), (200, "err(\"internal error\")".to_string()));
        let text = log_holding(&d.join(log), "read_file: sub:");
        assert!(text.contains("mem.qwe:27: read_file: sub: Is a directory"), "{}: {}", log, text);
    }
}

// ---- the native plugin cache (FEEDBACK71.md, Q-983) ---------------------------------------

/// A program that uses a native plugin in `app/`, and in `bin/` a cargo that only writes where it
/// ran and what it was given to `cargo-said.txt`, and fails: the folder and that file.
#[cfg(unix)]
fn native_app(name: &str) -> (Dir, PathBuf) {
    let d = Dir::new(name, &[("app/main.qwe", "use \"hi.qwer\"\nprint(twice(21))\n"), ("app/hi.qwer", "pub fn twice(n: i64) -> i64 { n * 2 }\n"), ("tmp/.keep", "")]);
    let said = d.join("cargo-said.txt");
    d.write("bin/cargo", &format!("#!/bin/sh\necho \"cwd=$(pwd)\" > '{0}'\nprintf '%s\\n' \"$@\" >> '{0}'\nexit 1\n", said.display()));
    chmod(&d.join("bin/cargo"), 0o755);
    (d, said)
}

/// `quillo run main.qwe` in `d`'s `app/` with only PATH (`path`) and TMPDIR set, the coverage
/// run's own settings kept, and `env` besides.
#[cfg(unix)]
fn run_native(d: &Dir, path: &str, env: &[(&str, &Path)]) -> Ran {
    let mut c = quillo(&d.join("app"));
    c.env_clear().env("PATH", path).env("TMPDIR", d.join("tmp")).args(["run", "main.qwe"]);
    for k in ["LLVM_PROFILE_FILE", "QUILLO_RUNTIME_COVERAGE"] {
        if let Some(v) = std::env::var_os(k) {
            c.env(k, v);
        }
    }
    for (k, v) in env {
        c.env(k, v);
    }
    run_capped(c, 60)
}

/// A folder another user (not root) owns, with no link on its way, if this machine has one in
/// the usual places: it, and its owner.
#[cfg(unix)]
fn another_users_folder() -> Option<(PathBuf, u32)> {
    use std::os::unix::fs::MetadataExt;
    for root in ["/private/var", "/private/var/db", "/var", "/var/lib", "/var/cache", "/var/spool", "/var/log", "/usr/local", "/opt", "/srv", "/home"] {
        let Ok(rd) = std::fs::read_dir(root) else { continue };
        let mut found: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        found.sort();
        for p in found {
            let Ok(m) = std::fs::symlink_metadata(&p) else { continue };
            if m.is_dir() && m.uid() != 0 && m.uid() != me() && p.canonicalize().is_ok_and(|c| c == p) {
                return Some((p, m.uid()));
            }
        }
    }
    None
}

/// `QUILLO_NATIVE_CACHE` under a folder another user owns: refused before anything is made, and
/// cargo is not run (src/native.rs `safe_above`, 1149; coverage of 0.8.7; branch coverage-087).
#[cfg(unix)]
#[test]
fn a_cache_under_another_users_folder_is_refused() {
    let Some((theirs, uid)) = another_users_folder() else {
        eprintln!("no folder of another user's here: passed over");
        return;
    };
    let (d, said) = native_app("cache-theirs");
    let cache = theirs.join("quillo-cover087-cache");
    let r = run_native(&d, "/usr/bin:/bin", &[("CARGO", &d.join("bin/cargo")), ("QUILLO_NATIVE_CACHE", &cache)]);
    assert_ne!(r.code, Some(0));
    assert!(
        r.err.contains(&format!("{}, above the plugin cache, belongs to another user (uid {}): nothing is built there; set QUILLO_NATIVE_CACHE to a folder of your own", theirs.display(), uid)),
        "{}",
        r.err
    );
    assert!(!said.exists(), "cargo was run");
}

/// `QUILLO_NATIVE_CACHE` that is a link leading nowhere, and one that is a file: neither is a
/// folder to build in, each said so, nothing is made where the link leads and the file is as it
/// was; cargo is not run (src/native.rs `cache_folder`, 1209 and 1221; coverage of 0.8.7; branch
/// coverage-087).
#[cfg(unix)]
#[test]
fn a_cache_that_is_a_dangling_link_or_a_file_is_no_folder() {
    let (d, said) = native_app("cache-not-folder");
    let link = d.join("cache-link");
    std::os::unix::fs::symlink(d.join("nowhere"), &link).unwrap();
    let r = run_native(&d, "/usr/bin:/bin", &[("CARGO", &d.join("bin/cargo")), ("QUILLO_NATIVE_CACHE", &link)]);
    assert_ne!(r.code, Some(0));
    assert!(r.err.contains(&format!("cannot make {}/crates/quillo_hi_", link.display())) && r.err.contains("No such file or directory"), "{}", r.err);
    assert!(!d.join("nowhere").exists() && !said.exists());
    d.write("cache-file", "mine\n");
    let r = run_native(&d, "/usr/bin:/bin", &[("CARGO", &d.join("bin/cargo")), ("QUILLO_NATIVE_CACHE", &d.join("cache-file"))]);
    assert_ne!(r.code, Some(0));
    assert!(r.err.contains(&format!("the plugin cache {} is not a folder: nothing is built there", d.join("cache-file").display())), "{}", r.err);
    assert_eq!(std::fs::read_to_string(d.join("cache-file")).unwrap(), "mine\n");
    assert!(!said.exists());
}

/// A cache in HOME that others could write in, whose `crates` is a file and whose `target` is a
/// link to a folder of the user's: both taken away as they are (the link, not what it leads
/// to), made folders of the user's again, and cargo run (src/native.rs `empty_builds`, 1266;
/// coverage of 0.8.7; branch coverage-087).
#[cfg(unix)]
#[test]
fn an_opened_caches_file_and_link_are_taken_away_as_they_are() {
    let (d, said) = native_app("cache-opened");
    let home = d.join("home");
    let native = home.join(".cache/quillo/native");
    d.write("home/.cache/quillo/native/crates", "theirs\n");
    d.write("victim/keep.txt", "mine\n");
    std::os::unix::fs::symlink(d.join("victim"), native.join("target")).unwrap();
    chmod(&home.join(".cache/quillo"), 0o775);
    let r = run_native(&d, "/usr/bin:/bin", &[("CARGO", &d.join("bin/cargo")), ("HOME", &home)]);
    assert!(said.exists(), "cargo was not run: {}", r.err);
    for f in ["crates", "target"] {
        let m = std::fs::symlink_metadata(native.join(f)).unwrap();
        assert!(m.is_dir() && !m.file_type().is_symlink(), "{}", f);
    }
    let left: Vec<_> = std::fs::read_dir(d.join("victim")).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(left, ["keep.txt"]);
    assert_eq!(std::fs::read_to_string(d.join("victim/keep.txt")).unwrap(), "mine\n");
    use std::os::unix::fs::MetadataExt;
    assert_eq!(std::fs::metadata(home.join(".cache/quillo")).unwrap().mode() & 0o777, 0o700);
}

/// With no `$CARGO`, the `cargo` on the PATH is the one run, by its whole path: an entry named
/// in full, and one relative to the program's folder (src/native.rs `cargo_program`, 1300;
/// coverage of 0.8.7; branch coverage-087).
#[cfg(unix)]
#[test]
fn cargo_is_found_on_the_path_by_its_whole_path() {
    let (d, said) = native_app("cargo-path");
    std::fs::create_dir_all(d.join("app/tools")).unwrap();
    std::fs::copy(d.join("bin/cargo"), d.join("app/tools/cargo")).unwrap();
    for path in [format!("{}:/usr/bin:/bin", d.join("bin").display()), "tools:/usr/bin:/bin".to_string()] {
        let _ = std::fs::remove_file(&said);
        let r = run_native(&d, &path, &[("QUILLO_NATIVE_CACHE", &d.join("cache"))]);
        assert_ne!(r.code, Some(0));
        let asked = std::fs::read_to_string(&said).unwrap_or_else(|_| panic!("{}: cargo was not run: {}", path, r.err));
        // Cargo runs in `/`, where the relative entry would find nothing.
        assert!(asked.starts_with("cwd=/\n") && asked.contains(&format!("{}/crates/quillo_hi_", d.join("cache").display())), "{}", asked);
    }
}

/// The first 8 bytes of the SHA-256 of each part and a zero after it, in hex, as
/// `native::hex_hash` names a plugin's crate.
fn hex_hash(parts: &[&str]) -> String {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    for p in parts {
        h.update(p.as_bytes());
        h.update([0]);
    }
    h.finalize()[..8].iter().map(|b| format!("{:02x}", b)).collect()
}

/// A plugin's crate folder its user made so that it cannot be listed (write and enter only): it is
/// not emptied, as nothing in it can be seen, a file of the user's there stays, and the crate is
/// still written and built. Root lists any folder, so there it is passed over (src/native.rs
/// `prepare`, 1388; coverage of 0.8.7; branch coverage-087).
#[cfg(unix)]
#[test]
fn a_crate_folder_that_cannot_be_listed_is_still_written() {
    if me() == 0 {
        eprintln!("root lists any folder: passed over");
        return;
    }
    let (d, said) = native_app("crate-unlisted");
    let abs = d.join("app/hi.qwer").canonicalize().unwrap();
    let crate_dir = d.join("cache/crates").join(format!("quillo_hi_{}", &hex_hash(&[&abs.display().to_string()])[..8]));
    std::fs::create_dir_all(&crate_dir).unwrap();
    std::fs::write(crate_dir.join("notes.txt"), "mine\n").unwrap();
    chmod(&crate_dir, 0o300);
    let r = run_native(&d, "/usr/bin:/bin", &[("CARGO", &d.join("bin/cargo")), ("QUILLO_NATIVE_CACHE", &d.join("cache"))]);
    let asked = std::fs::read_to_string(&said).unwrap_or_else(|_| panic!("cargo was not run: {}", r.err));
    assert!(asked.contains(&crate_dir.join("Cargo.toml").display().to_string()), "{}", asked);
    let manifest = std::fs::read_to_string(crate_dir.join("Cargo.toml")).unwrap();
    assert!(manifest.contains("publish = false\nbuild = false\n"), "{}", manifest);
    assert!(std::fs::read_to_string(crate_dir.join("lib.rs")).unwrap().contains("twice"));
    assert_eq!(std::fs::read_to_string(crate_dir.join("notes.txt")).unwrap(), "mine\n");
}

// ---- quillo install into a folder it cannot write (FEEDBACK71.md, Q-981) -----------------

/// A catalog with plugin `hi`, served on a port of the test's own until the test ends: its URL.
fn registry(file: &'static [u8]) -> String {
    use sha2::Digest;
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let sha: String = sha2::Sha256::digest(file).iter().map(|b| format!("{:02x}", b)).collect();
    let index = format!(
        r#"{{"format": 1, "plugins": [{{"name": "hi", "title": "hi", "version": "1.0.0", "summary": "Says hi.", "group": "Text", "sha256": "{}", "file": "/plugins/files/hi.qwer", "example": "/plugins/files/hi.qwe", "files": []}}]}}"#,
        sha
    );
    std::thread::spawn(move || {
        for mut s in l.incoming().flatten() {
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") {
                match s.read(&mut byte) {
                    Ok(1) => head.push(byte[0]),
                    _ => break,
                }
            }
            let path = String::from_utf8_lossy(&head).split_whitespace().nth(1).unwrap_or("/").to_string();
            let body: &[u8] = match path.as_str() {
                "/plugins/index.json" => index.as_bytes(),
                "/plugins/files/hi.qwer" => file,
                _ => b"no",
            };
            let _ = write!(s, "HTTP/1.1 {}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", if body == b"no" { "404 Not Found" } else { "200 OK" }, body.len());
            let _ = s.write_all(body);
        }
    });
    url
}

/// `quillo install hi` into a folder its user may not write in: refused at the file it would
/// make beside the plugin, nothing left there (src/install.rs `write_atomic`, 207; coverage of
/// 0.8.7; branch coverage-087).
#[cfg(unix)]
#[test]
fn install_into_a_folder_it_cannot_write_says_so() {
    if me() == 0 {
        eprintln!("root writes anywhere: passed over");
        return;
    }
    let d = Dir::new("install-ro", &[("repo/.keep", "")]);
    let repo = d.join("repo");
    chmod(&repo, 0o555);
    let url = registry(b"//! Says hi.\n//! version: 1.0.0\n//!\npub fn hi() -> String { \"hi\".into() }\n");
    let mut c = quillo(&repo);
    c.args(["install", "hi", "--registry", &url]).env("HOME", d.path());
    let r = run_capped(c, 60);
    chmod(&repo, 0o755);
    assert_ne!(r.code, Some(0), "{}{}", r.out, r.err);
    let text = r.out + &r.err;
    // `./hi.qwer.<pid>-<8 hex digits>.part`, the file of its own it would have made first.
    let line = text.lines().find(|l| l.starts_with("quillo install: ./hi.qwer.")).unwrap_or_else(|| panic!("{}", text));
    let part = line.trim_start_matches("quillo install: ./hi.qwer.").split(".part: ").collect::<Vec<_>>();
    assert!(part.len() == 2 && part[1].starts_with("Permission denied"), "{}", line);
    let (pid, hex) = part[0].split_once('-').unwrap();
    assert!(pid.parse::<u32>().is_ok() && hex.len() == 8 && hex.bytes().all(|b| b.is_ascii_hexdigit()), "{}", line);
    let left: Vec<_> = std::fs::read_dir(&repo).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(left, [".keep"]);
}
