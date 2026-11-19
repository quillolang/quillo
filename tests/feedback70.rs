// FEEDBACK70.md: how Quillo keeps forms from other sites' pages (branch security). Q-965, a GET
// route that writes, run by a link on another site with the visitor's session: `quillo check` warns
// where the route surely writes and surely reads who the visitor is, and `route writes GET /x` says
// the write is meant. Q-966, `Origin`'s scheme against the request's, where a proxy `--trust-proxy`
// names says it. HSTS: Quillo serves no TLS itself, so it sends none, whatever a proxy says.

mod support;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn dir(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = support::temp_dir(&format!("f70-{}", name));
    for (f, text) in files {
        std::fs::write(d.join(f), text).unwrap();
    }
    d
}

/// `quillo check main.qwe` in `d`, with a deadline, its output read as it comes (a full pipe would
/// stop it): whether it passed, and its error and warning lines.
fn check(d: &Path) -> (bool, Vec<String>) {
    use std::io::Read;
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("check")
        .arg("main.qwe")
        .current_dir(d)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let read = |mut r: Box<dyn Read + Send>| std::thread::spawn(move || {
        let mut v = Vec::new();
        let _ = r.read_to_end(&mut v);
        v
    });
    let (out, err) = (read(Box::new(child.stdout.take().unwrap())), read(Box::new(child.stderr.take().unwrap())));
    // A safety net, not a measure: the 20,000-call chain under a debug build takes a while.
    let end = Instant::now() + Duration::from_secs(300);
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        if Instant::now() > end {
            let _ = child.kill();
            panic!("quillo check did not end");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let text = String::from_utf8_lossy(&out.join().unwrap()).into_owned() + &String::from_utf8_lossy(&err.join().unwrap());
    (status.success(), text.lines().filter(|l| l.contains(": error: ") || l.contains(": warning: ")).map(String::from).collect())
}

fn check_src(name: &str, src: &str) -> (bool, Vec<String>) {
    check(&dir(name, &[("main.qwe", src)]))
}

/// What `quillo check` says of a GET route that writes and reads who the visitor is.
fn warned(line: usize, route: &str, writes: &str, reads: &str) -> String {
    format!(
        "main.qwe:{}: warning: `{}` writes ({}) and reads who the visitor is ({}): a link on another site, in an email or in a chat runs it with their session (`SameSite=Lax` cookies go with a followed link). Make it a `POST`, or, if the write is meant (a confirm-your-address link, a sign-in callback, a presence ping), tag it: `route writes GET ...`",
        line, route, writes, reads
    )
}

/// The report's program: `/posts/delete-all` deletes the signed-in visitor's posts.
const REPORT: &str = r#"db.open(":memory:")
db.exec("create table posts (id integer primary key, owner int not null)")

route GET /login {
    set_session({ user: 1 })
    db.exec("insert into posts (owner) values (1), (1), (1)")
    return "in"
}
route GET /count = `${db.query("select count(*) as n from posts")[0].n}`
route GET /posts/delete-all {
    let u = session().user ?? return respond(401, "log in first")
    db.exec("delete from posts where owner = ?", u)
    return "deleted"
}
"#;

// The report's probe: the route that deletes is said, at its line, with what it writes and what
// of the visitor it reads; `/login`, which writes but reads nothing of anyone's, and `/count`,
// which only reads, are not.
#[test]
fn q965_the_reports_route_is_warned_of() {
    assert_eq!(
        check_src("report", REPORT),
        (true, vec![warned(10, "GET /posts/delete-all", "`db.exec(\"delete from posts where owner = ?\")`", "`session()`")])
    );
}

/// What each kind of write and of read is said as, one GET route each, at its line.
const SERVICE: &str = r#"db.open(":memory:")
db.exec("create table charges (who text)")

server fn charge(who: str) {
    db.exec("insert into charges (who) values (?)", who)
    return 1
}

server fn ping() {
    return "pong"
}

server fn charges_of(who: str) -> list {
    return db.query("select * from charges where who = ?", who)
}

server fn mine(who: str) -> list {
    let q = "select * from charges where who = ?"
    return db.query(q, who)
}
"#;

const PRELUDE: &str = r#"db.open(":memory:")
db.exec("create table notes (id integer primary key, owner text, title text, seen int)")
service billing = "billing.qwe"
service remote = "remote.qwe"
let bumps = 0

fn me() {
    return session().user
}

fn wipe(u) {
    db.exec("delete from notes where owner = ?", u)
}

fn signed(request) {
    return session().user == null ? redirect("/") : request
}

fn teamed(req) {
    return req.cookies.team == null ? redirect("/") : req
}
"#;

// Each write the prefetch guard refuses at run time (db.exec, SQL written that writes, piped too,
// a global assigned, the session and cookies, files, publish as a method, http with a method that
// writes, a service's function that writes or whose code is not here, a message sent, a
// transaction's function) and each read of who the visitor is (session(), cookie(), the request's
// headers and cookies, destructured too, a request pattern, a function called, a group's
// middleware, by the session or by the request it is given), in a route with unicode in its path
// and its SQL as well.
#[test]
fn q965_each_write_and_each_read_is_said() {
    let sql = |f: &str, s: &str| format!("`{}(\"{}\")`", f, s);
    let cases: Vec<(&str, String, String, String)> = vec![
        ("/c1", "    let u = session().user\n    db.exec(\"delete from notes where owner = ?\", u)\n".into(), sql("db.exec", "delete from notes where owner = ?"), "`session()`".into()),
        ("/c2", "    let sql = \"delete from notes\"\n    session()\n    db.exec(sql)\n".into(), "`db.exec`".into(), "`session()`".into()),
        ("/c3", "    db.query(\"delete from notes where owner = ? returning id\", session().user)\n".into(), format!("{}, SQL that writes", sql("db.query", "delete from notes where owner = ? returning id")), "`session()`".into()),
        ("/c4", "    \"delete from notes\" | db.query()\n    session()\n".into(), format!("{}, SQL that writes", sql("db.query", "delete from notes")), "`session()`".into()),
        ("/c5", "    if session().user != null {\n        bumps += 1\n    }\n".into(), "assigns the program's variable `bumps`".into(), "`session()`".into()),
        ("/c6", "    set_cookie(\"seen\", str(session().user))\n".into(), "`set_cookie`".into(), "`session()`".into()),
        ("/c7", "    if cookie(\"theme\") == \"x\" {\n        end_session()\n    }\n".into(), "`end_session`".into(), "`cookie()`".into()),
        ("/c8", "    write_file(\"log.txt\", request.headers[\"user-agent\"] ?? \"\")\n".into(), "`write_file`".into(), "the `user-agent` header".into()),
        ("/c9", "    let c = request.cookies\n    print(c)\n    \"notes\".publish()\n".into(), "`publish`".into(), "the request's cookies".into()),
        ("/c10", "    http.post(\"https://example.com/hook\", { who: session().user })\n".into(), "`http.post`".into(), "`session()`".into()),
        ("/c11", "    http.request(\"delete\", \"https://example.com/x/\" + str(session().user))\n".into(), "`http.request(\"delete\", ...)`".into(), "`session()`".into()),
        ("/c12", "    billing.charge(str(session().user))\n".into(), "`billing.charge`, a call of the service `billing`".into(), "`session()`".into()),
        // A service whose code is not here: its call is taken as a write, as at run time.
        ("/c12b", "    remote.charge(str(session().user))\n".into(), "`remote.charge`, a call of the service `remote`".into(), "`session()`".into()),
        ("/c13", "    wipe(me())\n".into(), format!("{}, in `wipe`", sql("db.exec", "delete from notes where owner = ?")), "`session()`, in `me`".into()),
        ("/c14", "    let { headers } = request\n    set_session({ agent: headers[\"user-agent\"] })\n".into(), "`set_session`".into(), "the request's headers".into()),
        ("/c15", "    notification.send(str(session().user), { title: \"Hi\", body: \"x\", url: \"/\" })\n".into(), "`notification.send`".into(), "`session()`".into()),
        ("/c16", "    db.exec_many(\"insert into notes (owner) values (?)\", [[session().user]])\n".into(), sql("db.exec_many", "insert into notes (owner) values (?)"), "`session()`".into()),
        ("/c17", "    let r = db.transaction(() => db.exec(\"delete from notes where owner = ?\", session().user))\n    print(r)\n".into(), sql("db.exec", "delete from notes where owner = ?"), "`session()`".into()),
        ("/café/supprimer", "    db.exec(\"delete from notes where title = 'été ✓'\")\n    print(session().user ?? \"personne\")\n".into(), sql("db.exec", "delete from notes where title = 'été ✓'"), "`session()`".into()),
    ];
    let mut src = PRELUDE.to_string();
    let mut want = vec!["main.qwe:4: warning: service remote: remote.qwe is not here, so its calls are not checked; it must run elsewhere (QUILLO_SERVICE_REMOTE or --service-url)".to_string()];
    for (path, body, w, r) in &cases {
        want.push(warned(src.lines().count() + 1, &format!("GET {}", path), w, r));
        src.push_str(&format!("route GET {} {{\n{}    return \"ok\"\n}}\n", path, body));
    }
    // A request pattern, and a group's middleware: by the session, and by the request it is given.
    want.push(warned(src.lines().count() + 1, "GET /pat", "`db.exec(\"delete from notes\")`", "its request pattern takes the request's cookies"));
    src.push_str("route GET /pat { cookies: { sid: str } } {\n    db.exec(\"delete from notes\")\n    return sid\n}\n");
    want.push(warned(src.lines().count() + 2, "GET /m/x", "`db.exec(\"delete from notes\")`", "`session()`, in `signed`"));
    src.push_str("routes /m | signed {\n    route GET /x {\n        db.exec(\"delete from notes\")\n        return \"x\"\n    }\n}\n");
    want.push(warned(src.lines().count() + 2, "GET /t/y", "`db.exec(\"delete from notes\")`", "the `team` cookie, in its group's middleware"));
    src.push_str("routes /t | teamed {\n    route GET /y {\n        db.exec(\"delete from notes\")\n        return \"y\"\n    }\n}\n");
    let d = dir("each", &[("main.qwe", &src), ("billing.qwe", SERVICE)]);
    let (ok, got) = check(&d);
    assert!(ok, "{:?}", got);
    assert_eq!(got.len(), want.len(), "{}\n{:#?}", src, got);
    for (g, w) in got.iter().zip(&want) {
        assert_eq!(g, w);
    }
}

// What must keep working, and is not said: a read written in SQL (`for update`, a `pragma`, a `with`
// that selects, `db.exec` of a select), SQL and methods built as the program runs (never guessed:
// a `let` holding a literal is read as that literal, changed with FEEDBACK75.md, Q-1084),
// a write that reads nothing of anyone's (a counter; the request's host), a write in a handler the
// page runs (a server fn's own POST), a local of a global's name, a POST and a DELETE, a service's
// functions that do not surely write (nothing, a select, SQL built as it runs), the program's own
// function named as a builtin, and `http.get` and a GET `http.request`. An `ANY` route answers a
// GET, so it is said (second review).
#[test]
fn q965_what_reads_or_is_not_sure_is_not_said() {
    let src = r#"db.open(":memory:")
db.exec("create table notes (id integer primary key, owner text, title text, seen int)")
service billing = "billing.qwe"
let bumps = 0

fn publish(x: str) {
    return x
}

server fn drop_all() {
    db.exec("delete from notes")
    return 1
}

route GET /r1 {
    let u = session().user
    return db.query("select * from notes where owner = ?", u)
}
route GET /r2 {
    let sql = "delete from notes"
    session()
    return db.query(sql)
}
route GET /r3 = db.query("select * from notes where owner = ? for update", session().user)
route GET /r4 {
    session()
    return db.query("pragma table_info(notes)")
}
route GET /r5 = db.query("with mine as (select * from notes where owner = ?) select count(*) as n from mine", session().user)
route GET /r6 {
    db.exec("select 1")
    return session().user ?? "no one"
}
route GET /r7 {
    bumps += 1
    return str(bumps)
}
route GET /r8 {
    let who = session().user ?? "no one"
    return <html><body><p>{who}</p><button @click={() => drop_all()}>Remove</button></body></html>
}
route GET /r9 {
    let bumps = 1
    bumps += 1
    return `${bumps} ${session().user}`
}
route POST /r10 {
    db.exec("delete from notes where owner = ?", session().user)
    return "ok"
}
route DELETE /r11 {
    db.exec("delete from notes where owner = ?", session().user)
    return "ok"
}
route GET /r12 {
    let p = billing.ping()
    return `${p} ${session().user}`
}
route GET /r13 = publish(str(session().user))
route GET /r14 {
    let a = http.get("https://example.com/")
    let b = http.request("GET", "https://example.com/" + str(session().user))
    return str([a, b])
}
route ANY /r15 {
    db.exec("delete from notes where owner = ?", session().user)
    return "ok"
}
route GET /r16 {
    let m = "DELETE"
    let r = http.request(m, "https://example.com/" + str(cookie("x")))
    return str(r)
}
route GET /r17 {
    db.exec("delete from notes")
    return "ok"
}
route GET /r18 {
    let h = request.headers.host ?? request.headers["Host"] ?? ""
    db.exec("delete from notes")
    return h
}
route GET /r19 {
    let a = billing.charges_of(str(session().user))
    let b = billing.mine(str(session().user))
    return str([a, b])
}
"#;
    let d = dir("not", &[("main.qwe", src), ("billing.qwe", SERVICE)]);
    assert_eq!(
        check(&d),
        (
            true,
            vec![
                "main.qwe:6: warning: `publish` hides the builtin `publish`: the program's own is used everywhere, so pick another name".to_string(),
                // `/r2` and `/r16` hold their SQL and their method in a `let` of a literal, read as
                // that literal (changed with FEEDBACK75.md, Q-1084; branch feedback75).
                warned(19, "GET /r2", "`db.query(\"delete from notes\")`, SQL that writes", "`session()`"),
                warned(65, "ANY /r15", "`db.exec(\"delete from notes where owner = ?\")`", "`session()`"),
                warned(69, "GET /r16", "`http.request(\"DELETE\", ...)`", "`cookie()`"),
            ]
        )
    );
}

// The tag says the write is meant: on a route, either order with `offline`, and on a group, at any
// depth, where only its GET routes take it. An untagged route beside them is still said; and a
// function that calls itself ends the walk (the check's deadline says so).
#[test]
fn q965_the_writes_tag_silences_it() {
    let src = r#"fn again(n: int) {
    if n > 0 {
        return again(n - 1)
    }
    return session().user
}

route writes GET /confirm/:code {
    db.exec("update notes set seen = 1 where title = ?", code)
    return str(session().user)
}
routes writes /hooks {
    route GET /seen {
        db.exec("update notes set seen = 1 where owner = ?", session().user)
        return "ok"
    }
    route POST /x = "x"
}
routes /a {
    routes writes /b {
        route GET /c {
            db.exec("delete from notes where owner = ?", again(3))
            return "ok"
        }
    }
    route GET /d {
        db.exec("delete from notes where owner = ?", again(3))
        return "ok"
    }
}
"#;
    assert_eq!(check_src("tag", src), (true, vec![warned(26, "GET /a/d", "`db.exec(\"delete from notes where owner = ?\")`", "`session()`, in `again`")]));
}

/// What `quillo check` says of a page tagged `offline` and `writes`.
fn offline_writes(line: u32, route: &str) -> String {
    format!(
        "main.qwe:{}: warning: `route offline writes {}`: Quillo's service worker keeps an offline page ahead with a prefetch, which writes nothing, so this one is refused then, and a copy kept from a visit is shown offline without the write; drop `offline`, or `writes` and the write (a `POST` the page sends)",
        line, route
    )
}

// `offline` with `writes` on one route, in either order and from a group's tags: the page is not
// kept ahead (the worker's prefetch writes nothing), so it is said.
#[test]
fn q965_offline_with_writes_is_said() {
    let src = "route offline writes GET /a = \"a\"\nroute writes offline GET /b = \"b\"\nroutes offline writes /g {\n    route GET /c = \"c\"\n}\nroutes writes /h {\n    route offline GET /d = \"d\"\n}\nroute offline GET /e = \"e\"\n";
    assert_eq!(
        check_src("offline", src),
        (true, vec![offline_writes(1, "GET /a"), offline_writes(2, "GET /b"), offline_writes(4, "GET /g/c"), offline_writes(7, "GET /h/d")])
    );
}

// The tag as written: after `route`, before the method, with `offline` in either order, and after
// `routes`, before the group's path. `writes` stays a name everywhere else. Refused, each said for
// what it is: the tag on a route that is not a GET, twice, after the method or the path, with no
// method, a group with no GET (at the top and inside another), and markup left open runs into a
// tagged route as into a plain one.
#[test]
fn q965_the_tag_is_parsed() {
    let ok = [
        "let writes = true\nfn f(writes: bool) = !writes\nroute GET /x = f(writes) ? \"y\" : \"n\"\n",
        "route writes GET /x with db = str(db.query(\"select 1\"))\n",
        "route writes GET /x/:id(int) ? { page: int = 1 } = <p>{id} {page}</p>\n",
        "route offline writes GET /x = \"x\"\nroute writes  offline\tGET /y = \"y\"\n",
        "routes writes /docs {\n    routes /inner {\n        route GET /a = \"a\"\n    }\n    route DELETE /b = \"b\"\n}\n",
        "routes writes offline /p {\n    route GET /a = \"a\"\n}\nroutes offline  writes /q {\n    route GET /a = \"a\"\n}\n",
        "let routes = 2\nlet writes = 1\nroute GET / = str(routes / writes)\n",
        "route writes ANY /x = \"x\"\nroutes writes /g {\n    route ANY /y = \"y\"\n}\n",
    ];
    for src in ok {
        let (passed, said) = check_src("parse-ok", src);
        assert!(passed && said.iter().all(|l| l.contains("route offline writes")), "{}: {:?}", src, said);
    }
    let refused = [
        ("route writes POST /x = \"x\"\n", "main.qwe:1: error: `writes` is for GET routes (and ANY, which takes GET): it says that a GET's write is meant, where a POST, a PUT, a PATCH or a DELETE is expected to write; drop it (`route POST /...`)"),
        ("route writes DELETE /x = \"x\"\n", "main.qwe:1: error: `writes` is for GET routes (and ANY, which takes GET): it says that a GET's write is meant, where a POST, a PUT, a PATCH or a DELETE is expected to write; drop it (`route DELETE /...`)"),
        ("route writes HEAD /x = \"x\"\n", "main.qwe:1: error: `writes` is for GET routes (and ANY, which takes GET): it says that a GET's write is meant, where a POST, a PUT, a PATCH or a DELETE is expected to write; drop it (`route HEAD /...`)"),
        ("route writes writes GET /x = \"x\"\n", "main.qwe:1: error: `writes` is written twice: once is enough, `route writes GET /...`"),
        ("route GET /confirm writes = \"x\"\n", "main.qwe:1: error: `writes` comes after `route`, before the method: `route writes GET /confirm ...`"),
        ("route GET writes /confirm = \"x\"\n", "main.qwe:1: error: `writes` comes after `route`, before the method: `route writes GET /...`"),
        ("route writes /confirm = \"x\"\n", "main.qwe:1: error: `writes` comes before the method, and says that a GET's write is meant: `route writes GET /confirm/:code = ...`"),
        ("route offline writes /confirm = \"x\"\n", "main.qwe:1: error: `writes` comes before the method, and says that a GET's write is meant: `route writes GET /confirm/:code = ...`"),
        ("routes writes /api {\n    route POST /a = \"a\"\n}\n", "main.qwe:1: error: this `routes writes` group has no GET route (nor an ANY one, which takes GET): `writes` says that a GET's write is meant, and its other routes are expected to write"),
        ("routes /api {\n    routes writes /in {\n        route PUT /a = \"a\"\n    }\n    route GET /b = \"b\"\n}\n", "main.qwe:2: error: this `routes writes` group has no GET route (nor an ANY one, which takes GET): `writes` says that a GET's write is meant, and its other routes are expected to write"),
        ("routes writes writes /api {\n    route GET /a = \"a\"\n}\n", "main.qwe:1: error: `writes` is written twice: once is enough, `routes writes /...`"),
        ("route GET /a = <div>\n    <p>x</p>\n\nroute writes offline GET /b = \"b\"\n", "main.qwe:1: error: `<div>` is never closed: add `</div>` before `route` on line 4"),
    ];
    for (src, said) in refused {
        assert_eq!(check_src("parse-refused", src), (false, vec![said.to_string()]), "{}", src);
    }
}

// The tag in Quillo's highlighter: a keyword after `route` and `routes`, in either order with
// `offline`, the path after it still a path; a name `writes` is none.
#[test]
fn q965_the_highlighter_knows_the_tag() {
    let v = quillo::eval_str("highlight(\"route writes GET /c/*rest = 1\\nroutes offline writes /api {\\n}\\nlet writes = f(writes)\\nroute GET /x = writes\", \"quillo\")").unwrap();
    assert_eq!(
        v.display(),
        "<span class=\"hl-k\">route</span> <span class=\"hl-k\">writes</span> <span class=\"hl-t\">GET</span> /c/*rest = <span class=\"hl-n\">1</span>\n<span class=\"hl-k\">routes</span> <span class=\"hl-k\">offline</span> <span class=\"hl-k\">writes</span> /api {\n}\n<span class=\"hl-k\">let</span> writes = <span class=\"hl-f\">f</span>(writes)\n<span class=\"hl-k\">route</span> <span class=\"hl-t\">GET</span> /x = writes"
    );
}

/// `quillo lsp` in `d`, its messages read on a thread of their own, so that every wait has a
/// deadline: a language server that stops answering fails the test, not hangs it.
struct Lsp {
    _g: support::Guard,
    stdin: std::process::ChildStdin,
    got: std::sync::mpsc::Receiver<serde_json::Value>,
    next: u64,
}

impl Lsp {
    fn start(d: &Path) -> Lsp {
        use std::io::{BufRead, Read};
        let child = std::process::Command::new(env!("CARGO_BIN_EXE_quillo")).arg("lsp").current_dir(d).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).spawn().unwrap();
        let mut g = support::Guard::new(child);
        let mut out = std::io::BufReader::new(g.0.stdout.take().unwrap());
        let stdin = g.0.stdin.take().unwrap();
        let (tx, got) = std::sync::mpsc::channel();
        std::thread::spawn(move || loop {
            let mut len = 0;
            loop {
                let mut line = String::new();
                if out.read_line(&mut line).unwrap_or(0) == 0 {
                    return;
                }
                if line.trim_end().is_empty() {
                    break;
                }
                if let Some(v) = line.trim_end().strip_prefix("Content-Length:") {
                    len = v.trim().parse().unwrap_or(0);
                }
            }
            let mut buf = vec![0; len];
            if out.read_exact(&mut buf).is_err() || tx.send(serde_json::from_slice(&buf).unwrap_or_default()).is_err() {
                return;
            }
        });
        Lsp { _g: g, stdin, got, next: 0 }
    }

    fn send(&mut self, msg: serde_json::Value) {
        use std::io::Write;
        let body = msg.to_string();
        write!(self.stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
        self.stdin.flush().unwrap();
    }

    fn notify(&mut self, method: &str, params: serde_json::Value) {
        self.send(serde_json::json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    /// The first message `pick` takes, within 60 seconds.
    fn wait(&mut self, what: &str, mut pick: impl FnMut(&serde_json::Value) -> bool) -> serde_json::Value {
        let end = Instant::now() + Duration::from_secs(60);
        loop {
            let left = end.saturating_duration_since(Instant::now());
            match self.got.recv_timeout(left) {
                Ok(m) if pick(&m) => return m,
                Ok(_) => {}
                Err(_) => panic!("the language server did not send {}", what),
            }
        }
    }

    fn ask(&mut self, method: &str, params: serde_json::Value) -> serde_json::Value {
        self.next += 1;
        let id = self.next;
        self.send(serde_json::json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        self.wait(method, |m| m.get("id") == Some(&serde_json::json!(id)))["result"].clone()
    }

    fn open(d: &Path, text: &str) -> (Lsp, String) {
        std::fs::write(d.join("main.qwe"), text).unwrap();
        let uri = format!("file://{}", d.join("main.qwe").display());
        let mut l = Lsp::start(d);
        l.ask("initialize", serde_json::json!({ "capabilities": {} }));
        l.notify("initialized", serde_json::json!({}));
        l.notify("textDocument/didOpen", serde_json::json!({ "textDocument": { "uri": uri, "languageId": "quillo", "version": 1, "text": text } }));
        (l, uri)
    }
}

// The tag in the editor (`quillo lsp`): offered after `route ` and `routes `, and after the other
// tag; hovering it says what it is for, and a name `writes` is no tag.
#[test]
fn q965_the_editor_offers_and_explains_the_tag() {
    use serde_json::{json, Value};
    let d = support::temp_dir("f70-lsp").canonicalize().unwrap();
    let text = "route writes GET /c = 1\nroutes offline writes /p {\n}\nlet writes = 2\nroute \nroutes \nroute offline \nroute writes \nroute GET \n";
    let (mut l, uri) = Lsp::open(&d, text);
    let mut at = |method: &str, line: usize, character: usize| l.ask(method, json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": character } }));
    let tag = at("textDocument/hover", 0, 8)["contents"]["value"].as_str().unwrap_or("").to_string();
    assert!(tag.starts_with("`route writes GET /confirm/:code = ...`") && tag.contains("Only a GET takes it, and an ANY route"), "{}", tag);
    assert_eq!(at("textDocument/hover", 1, 17)["contents"]["value"].as_str().unwrap_or(""), tag);
    assert!(at("textDocument/hover", 1, 9)["contents"]["value"].as_str().unwrap_or("").contains("Only a GET can be kept."));
    assert!(!at("textDocument/hover", 3, 6).to_string().contains("Only a GET takes it"));
    let offered = |items: &Value, label: &str| items.as_array().unwrap().iter().any(|i| i["label"] == label && i["kind"] == 14);
    for (line, col, writes, offline) in [(4, 6, true, true), (5, 7, true, true), (6, 14, true, false), (7, 13, false, true), (8, 10, false, false)] {
        let items = at("textDocument/completion", line, col);
        assert_eq!((offered(&items, "writes"), offered(&items, "offline")), (writes, offline), "line {}: {}", line, items);
    }
}

// ---- Q-966: Origin's scheme ------------------------------------------------------------------

const FORM: &str = "route POST /notes { form: { text: str } } = \"saved\"\nserver fn save(text: str) -> str {\n    return text\n}\n";

/// A form post to `/notes` with these headers, its status.
fn post(port: u16, headers: &[(&str, &str)]) -> u16 {
    let host = format!("127.0.0.1:{}", port);
    let mut hs: Vec<(&str, &str)> = vec![("host", &host), ("content-type", "application/x-www-form-urlencoded")];
    hs.extend_from_slice(headers);
    support::send(port, "POST", "/notes", &hs, "text=hi").status
}

// The report's table, with `--trust-proxy 127.0.0.1` and `X-Forwarded-Proto: https`: an Origin of
// the other scheme is another site now; the request's own, another host, and Sec-Fetch-Site as
// before. The scheme known by the proxy's word, either way (any case), and an Origin of `null`.
#[test]
fn q966_with_the_scheme_known_the_other_one_is_another_site() {
    let d = dir("scheme", &[("main.qwe", FORM)]);
    let s = support::serve_with(&d.join("main.qwe"), &["--trust-proxy", "127.0.0.1"]);
    let port = s.port;
    let http = format!("http://127.0.0.1:{}", port);
    let https = format!("https://127.0.0.1:{}", port);
    for proto in ["https", "HTTPS"] {
        assert_eq!(post(port, &[("x-forwarded-proto", proto), ("origin", &http)]), 403, "{}", proto);
        assert_eq!(post(port, &[("x-forwarded-proto", proto), ("origin", &https)]), 200, "{}", proto);
        assert_eq!(post(port, &[("x-forwarded-proto", proto), ("origin", "https://evil.example")]), 403);
        assert_eq!(post(port, &[("x-forwarded-proto", proto), ("origin", &http), ("sec-fetch-site", "cross-site")]), 403);
        assert_eq!(post(port, &[("x-forwarded-proto", proto), ("origin", "null")]), 403);
        // A browser that says the site: what it says wins, as a page cannot set it.
        assert_eq!(post(port, &[("x-forwarded-proto", proto), ("origin", &http), ("sec-fetch-site", "same-origin")]), 200);
    }
    assert_eq!(post(port, &[("x-forwarded-proto", "http"), ("origin", &https)]), 403);
    assert_eq!(post(port, &[("x-forwarded-proto", "http"), ("origin", &http)]), 200);
    // Two proxies' words: the nearest's, the last.
    assert_eq!(post(port, &[("x-forwarded-proto", "http"), ("x-forwarded-proto", "https"), ("origin", &http)]), 403);
    // Neither header (a webhook) passes as before.
    assert_eq!(post(port, &[("x-forwarded-proto", "https")]), 200);
    // A server fn's door asks the same.
    let rpc = |origin: &str| {
        let host = format!("127.0.0.1:{}", port);
        support::send(port, "POST", "/_quillo/rpc/save", &[("host", &host), ("x-forwarded-proto", "https"), ("origin", origin)], "[\"a\"]").status
    };
    assert_eq!((rpc(&http), rpc(&https)), (403, 200));
}

// Where the scheme is not known, the hosts are compared alone, as before: no proxy named (its word
// believed from no one), a proxy named that says nothing, a word that is neither scheme (a list),
// and under --watch on localhost.
#[test]
fn q966_with_the_scheme_unknown_the_hosts_are_compared() {
    let d = dir("unknown", &[("main.qwe", FORM)]);
    let s = support::serve_with(&d.join("main.qwe"), &[]);
    let port = s.port;
    let http = format!("http://127.0.0.1:{}", port);
    let https = format!("https://127.0.0.1:{}", port);
    assert_eq!(post(port, &[("x-forwarded-proto", "https"), ("origin", &http)]), 200);
    assert_eq!(post(port, &[("x-forwarded-proto", "http"), ("origin", &https)]), 200);
    assert_eq!(post(port, &[("origin", "https://evil.example")]), 403);
    assert_eq!(post(port, &[("origin", "null")]), 403);
    drop(s);
    let s = support::serve_with(&d.join("main.qwe"), &["--trust-proxy", "127.0.0.1"]);
    let port = s.port;
    let http = format!("http://127.0.0.1:{}", port);
    let https = format!("https://127.0.0.1:{}", port);
    assert_eq!((post(port, &[("origin", &http)]), post(port, &[("origin", &https)])), (200, 200));
    assert_eq!(post(port, &[("x-forwarded-proto", "https, http"), ("origin", &http)]), 200);
    assert_eq!(post(port, &[("x-forwarded-proto", "wss"), ("origin", &http)]), 200);
    assert_eq!(post(port, &[("origin", "null")]), 403);
    drop(s);
    let s = support::serve_with(&d.join("main.qwe"), &["--watch"]);
    let port = s.port;
    let local = format!("localhost:{}", port);
    let origin = format!("http://localhost:{}", port);
    let a = support::send(port, "POST", "/notes", &[("host", &local), ("content-type", "application/x-www-form-urlencoded"), ("origin", &origin)], "text=hi");
    assert_eq!(a.status, 200);
}

// ---- HSTS: Quillo serves no TLS of its own ---------------------------------------------------

// Every answer is one Quillo sent over plain HTTP, so none carries `Strict-Transport-Security`, a
// proxy's `X-Forwarded-Proto: https` believed or not, under --base too: behind a proxy, the proxy
// decides. A program sends it with `site_headers`, and a route's own value wins.
#[test]
fn hsts_is_not_sent_over_plain_http_and_site_headers_sets_it() {
    let src = "route GET / = \"home\"\nroute GET /own = respond(200, \"own\", { \"strict-transport-security\": \"max-age=60\" })\n";
    let d = dir("hsts", &[("main.qwe", src)]);
    for opts in [&["--trust-proxy", "127.0.0.1"][..], &["--trust-proxy", "127.0.0.1", "--base", "/shop"][..]] {
        let s = support::serve_with(&d.join("main.qwe"), opts);
        let a = support::send(s.port, "GET", "/", &[("x-forwarded-proto", "https")], "");
        assert_eq!((a.status, a.header("strict-transport-security")), (200, None), "{:?}", opts);
    }
    let set = "site_headers({ \"strict-transport-security\": \"max-age=31536000; includeSubDomains\" })\nroute GET / = \"home\"\nroute GET /own = respond(200, \"own\", { \"strict-transport-security\": \"max-age=60\" })\n";
    let d = dir("hsts-set", &[("main.qwe", set)]);
    let s = support::serve_with(&d.join("main.qwe"), &[]);
    assert_eq!(support::send(s.port, "GET", "/", &[], "").header("strict-transport-security"), Some("max-age=31536000; includeSubDomains"));
    assert_eq!(support::send(s.port, "GET", "/own", &[], "").header("strict-transport-security"), Some("max-age=60"));
}

// ---- the review of slice 1 -----------------------------------------------------------------

/// `quillo check` of `src` in a folder of its own: the lines that say a GET writes (one route, or
/// one write for several), each cut to its start, past the file and line.
fn get_lines(name: &str, src: &str) -> Vec<String> {
    let (ok, said) = check_src(name, src);
    assert!(ok, "{:?}", said);
    said.into_iter().filter(|l| l.contains("reads who the visitor is") || l.contains("GET routes that read who")).map(|l| l.split(": warning: ").nth(1).unwrap_or("").split(": a link").next().unwrap_or("").split(". A link").next().unwrap_or("").to_string()).collect()
}

// Review 1: the request going anywhere reads who the visitor is, as Q-886 has it: handed to a
// function, bound to a name, indexed by name; and middleware that destructures it, hands it to a
// helper, or is made by a function (`| require("admin")`). Its address, `request.ip`, and its host
// are nobody's. A header or cookie is named where the text names it.
#[test]
fn review_the_request_going_anywhere_is_a_read() {
    let src = r#"fn who(req) {
    return req.cookies.sid
}
fn guard(req) {
    let { cookies } = req
    return cookies.sid == null ? null : req
}
fn ok(req) {
    return req.headers["x-key"] != null
}
fn handed(req) {
    return ok(req) ? req : null
}
fn require(role: str) {
    return (req) => req.cookies.role == role ? req : null
}
fn signed(request) {
    return request
}
route GET /p1 {
    db.exec("delete from t")
    return who(request)
}
route GET /p2 {
    let r = request
    db.exec("delete from t")
    return r.cookies.sid
}
route GET /p3 {
    db.exec("delete from t")
    return request["cookies"]["sid"]
}
routes /g1 | guard {
    route GET /x {
        db.exec("delete from t")
        return "x"
    }
}
routes /g2 | handed {
    route GET /x {
        db.exec("delete from t")
        return "x"
    }
}
routes /g3 | require("admin") {
    route GET /x {
        db.exec("delete from t")
        return "x"
    }
}
routes /g4 | signed {
    route GET /x {
        db.exec("delete from t")
        return "x"
    }
}
route GET /ip {
    db.exec("delete from t")
    return request.ip ?? ""
}
route GET /ua {
    db.exec("delete from t")
    return `${request.headers["user-agent"] ?? ""} ${request.headers.host ?? ""} ${request.path}`
}
"#;
    let w = "writes (`db.exec(\"delete from t\")`) and reads who the visitor is";
    assert_eq!(
        get_lines("r1", src),
        [
            format!("`GET /p1` {} (the request, given to `who`)", w),
            format!("`GET /p2` {} (the request)", w),
            format!("`GET /p3` {} (the `sid` cookie)", w),
            format!("`GET /g1/x` {} (the request's cookies, in its group's middleware)", w),
            format!("`GET /g2/x` {} (the request, given to `ok`, in its group's middleware)", w),
            format!("`GET /g3/x` {} (the `role` cookie, in its group's middleware)", w),
            format!("`GET /ua` {} (the `user-agent` header)", w),
        ]
    );
}

// Review 2: a connection's functions are `db`'s: one a top-level `let` holds, and one the route
// binds. A parameter or a map of the program's is not known to be one.
#[test]
fn review_a_connections_write_is_one() {
    let src = r#"let pg = db.connect(":memory:")?
fn clear(m) {
    m.exec("delete from t")
}
route GET /c1 {
    pg.exec("delete from t")
    return str(session().user)
}
route GET /c2 {
    let c = db.connect(":memory:")?
    c.exec_many("insert into t (a) values (?)", [[1]])
    return str(session().user)
}
route GET /c3 {
    let m = { exec: (s) => s }
    m.exec("delete from t")
    clear(m)
    return str(session().user)
}
route GET /c4 {
    let pg = { exec: (s) => s }
    pg.exec("delete from t")
    return str(session().user)
}
"#;
    assert_eq!(
        get_lines("r2", src),
        [
            "`GET /c1` writes (`pg.exec(\"delete from t\")`) and reads who the visitor is (`session()`)".to_string(),
            "`GET /c2` writes (`c.exec_many(\"insert into t (a) values (?)\")`) and reads who the visitor is (`session()`)".to_string(),
        ]
    );
}

// Review 3: a component's props are worked out on the server as the page is made: a write or a read
// of who the visitor is there counts, its handlers and lambdas do not.
#[test]
fn review_a_components_props_run_on_the_server() {
    let src = r#"server fn drop_all() {
    db.exec("delete from t")
    return 1
}
component Card(n: int) = <p>card {n}</p>
component Nav(user: any, go: any) = <p>{user}</p>
route GET /k1 = <html><body><Card n={db.exec("insert into t default values").changes}/><p>{session().user}</p></body></html>
route GET /k2 {
    db.exec("delete from t")
    return <html><body><Nav user={session().user} go={() => 1}/></body></html>
}
route GET /k3 = <html><body><Nav user={session().user} go={() => drop_all()}/></body></html>
"#;
    assert_eq!(
        get_lines("r3", src),
        [
            "`GET /k1` writes (`db.exec(\"insert into t default values\")`) and reads who the visitor is (`session()`)".to_string(),
            "`GET /k2` writes (`db.exec(\"delete from t\")`) and reads who the visitor is (`session()`)".to_string(),
        ]
    );
}

// Review 4: what a route is said to do does not hang on the order of the routes or the functions:
// `a` reads and calls `b`, `b` writes and calls `a`.
#[test]
fn review_the_order_of_the_routes_changes_nothing() {
    let fns = "fn a(n: int) {\n    session()\n    return n == 0 ? 0 : b(n - 1)\n}\nfn b(n: int) {\n    db.exec(\"delete from t\")\n    return n == 0 ? 0 : a(n - 1)\n}\n";
    let said = |route: &str| format!("`GET /{}` writes (`db.exec(\"delete from t\")`, in `b`) and reads who the visitor is (`session()`, in `a`)", route);
    for routes in ["route GET /two = str(b(2))\nroute GET /one = str(a(2))\n", "route GET /one = str(a(2))\nroute GET /two = str(b(2))\n"] {
        let mut got = get_lines("r4", &format!("{}{}", fns, routes));
        got.sort();
        assert_eq!(got, [said("one"), said("two")], "{}", routes);
    }
    assert_eq!(get_lines("r4b", &format!("route GET /two = str(b(2))\n{}", fns)), [said("two")]);
}

// Review 5 and 6: a function of the program's handed to a call runs as part of it; a lambda a `let`
// holds runs where it is called or handed on, not where it is bound (a handler of the page).
#[test]
fn review_functions_handed_on_and_lambdas_held() {
    let src = r#"fn tx() {
    db.exec("delete from t")
}
fn save_visit() {
    db.exec("insert into visits default values")
}
fn run(f) {
    return f()
}
server fn drop_all() {
    db.exec("delete from t")
    return 1
}
route GET /f1 {
    let r = db.transaction(tx)
    return `${r} ${session().user}`
}
route GET /f2 {
    run(save_visit)
    return str(session().user)
}
route GET /h1 {
    let remove = () => drop_all()
    let who = session().user ?? "no one"
    return <html><body><p>{who}</p><button @click={remove}>Remove</button><button onclick={drop_all}>All</button></body></html>
}
route GET /h2 {
    let wipe = () => db.exec("delete from t")
    wipe()
    return str(session().user)
}
route GET /h3 {
    let later = () => db.exec("delete from t")
    let r = db.transaction(later)
    return `${r} ${session().user}`
}
"#;
    assert_eq!(
        get_lines("r5", src),
        [
            "`GET /f1` writes (`db.exec(\"delete from t\")`, in `tx`) and reads who the visitor is (`session()`)".to_string(),
            "`GET /f2` writes (`db.exec(\"insert into visits default values\")`, in `save_visit`) and reads who the visitor is (`session()`)".to_string(),
            "`GET /h2` writes (`db.exec(\"delete from t\")`) and reads who the visitor is (`session()`)".to_string(),
            "`GET /h3` writes (`db.exec(\"delete from t\")`) and reads who the visitor is (`session()`)".to_string(),
        ]
    );
}

// Review 7: a chain of 20,000 calls is read without the stack (0.8.6 checks it; the walk had
// overflowed), to its end.
#[test]
fn review_a_chain_of_20000_calls_is_read_to_its_end() {
    let mut src = String::new();
    for i in 0..20_000 {
        src.push_str(&format!("fn f{}(x) {{\n    return f{}(x)\n}}\n", i, i + 1));
    }
    src.push_str("fn f20000(x) {\n    return session().user\n}\nroute GET /r {\n    db.exec(\"delete from t\")\n    return str(f0(1))\n}\n");
    assert_eq!(get_lines("r7", &src), ["`GET /r` writes (`db.exec(\"delete from t\")`) and reads who the visitor is (`session()`, in `f20000`)"]);
}

// Review 9: three GET routes or more through one write (a helper's, a group's middleware's) are said
// once, at the write, with the first three and how many more; two are said each.
#[test]
fn review_one_write_for_many_routes_is_said_once() {
    let helper = "fn seen() {\n    db.exec(\"update users set seen = 1 where id = ?\", session().user)\n}\n";
    let routes = |n: usize| (0..n).map(|i| format!("route GET /r{} {{\n    seen()\n    return \"ok\"\n}}\n", i)).collect::<String>();
    let (ok, said) = check_src("r9", &format!("{}{}", helper, routes(5)));
    assert_eq!(
        (ok, said),
        (true, vec!["main.qwe:2: warning: this write (`db.exec(\"update users set seen = 1 where id = ?\")`, in `seen`) is done by 5 GET routes that read who the visitor is: `GET /r0` (line 4), `GET /r1` (line 8), `GET /r2` (line 12) and 2 more. A link on another site, in an email or in a chat runs them with the visitor's session (`SameSite=Lax` cookies go with a followed link). Make them `POST`s, or, if the write is meant (a presence ping, a page's count of views), tag them: `route writes GET ...`, `routes writes /...`".to_string()])
    );
    let (_, three) = check_src("r9c", &format!("{}{}", helper, routes(3)));
    assert!(three.len() == 1 && three[0].contains("by 3 GET routes") && three[0].contains("`GET /r2` (line 12). A link"), "{:?}", three);
    let two = get_lines("r9b", &format!("{}{}", helper, routes(2)));
    assert_eq!(two.len(), 2, "{:?}", two);
    // A group's middleware that writes, for its routes, in a used file: said at the write there.
    let d = dir("r9d", &[
        ("main.qwe", "use \"lib.qwe\"\nroutes /m | noted {\n    route GET /a = \"a\"\n    route GET /b = \"b\"\n    route GET /c = \"c\"\n}\n"),
        ("lib.qwe", "public fn noted(req) {\n    db.exec(\"insert into visits (who) values (?)\", session().user)\n    return req\n}\n"),
    ]);
    let (ok, said) = check(&d);
    assert!(ok && said.len() == 1 && said[0].contains("lib.qwe:2: warning: this write (`db.exec(\"insert into visits (who) values (?)\")`, in `noted`) is done by 3 GET routes") && said[0].contains("`GET /m/a` (main.qwe:3)"), "{:?}", said);
}

// `sql_shown`: SQL on one line, cut after 60 characters: 59 and 60 whole, 61 cut.
#[test]
fn review_sql_is_shown_to_60_characters() {
    let sql = |n: usize| {
        let head = "delete from t where a = '";
        format!("{}{}'", head, "x".repeat(n - head.len() - 1))
    };
    for (n, cut) in [(59, false), (60, false), (61, true)] {
        let s = sql(n);
        assert_eq!(s.chars().count(), n);
        let shown = if cut { format!("{}...", &s[..60]) } else { s.clone() };
        let got = get_lines("sql", &format!("route GET /x {{\n    db.exec(\"{}\")\n    return str(session().user)\n}}\n", s));
        assert_eq!(got, [format!("`GET /x` writes (`db.exec(\"{}\")`) and reads who the visitor is (`session()`)", shown)], "{}", n);
    }
    // On one line: a statement written over three is shown on one.
    let got = get_lines("sql-lines", "route GET /x {\n    db.exec(`delete\n        from t\n        where a = 1`)\n    return str(session().user)\n}\n");
    assert_eq!(got, ["`GET /x` writes (`db.exec(\"delete from t where a = 1\")`) and reads who the visitor is (`session()`)"]);
}

// ---- the review: Q-966's edges -------------------------------------------------------------

/// `quillo run` of `d`'s main.qwe, its log in a file: the system picks the port (`--port 0`) and
/// the log says which, as `support::serve_opts` has it (a port picked here could be taken in
/// between), with a deadline.
struct Logged(std::process::Child, PathBuf, u16);

impl Drop for Logged {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn logged(d: &Path, opts: &[&str], host: &str) -> Logged {
    let log = d.join("server.log");
    let child = std::process::Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(d.join("main.qwe"))
        .args(["--port", "0"])
        .args(opts)
        .env("QUILLO_SECRET", "test secret for the tests")
        .current_dir(d)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    support::stop_with_tests(&child);
    let mut s = Logged(child, log, 0);
    let end = Instant::now() + Duration::from_secs(60);
    while s.2 == 0 {
        let text = std::fs::read_to_string(&s.1).unwrap_or_default();
        if let Some(url) = text.split("listening on ").nth(1) {
            let hostport = url.split("://").nth(1).unwrap_or(url).split(|c: char| c == '/' || c.is_whitespace()).next().unwrap_or("");
            s.2 = hostport.rsplit(':').next().and_then(|p| p.parse().ok()).unwrap_or(0);
        }
        if let Ok(Some(st)) = s.0.try_wait() {
            panic!("quillo run stopped: {}: {}", st, text);
        }
        assert!(Instant::now() < end, "the server did not start: {}", text);
        std::thread::sleep(Duration::from_millis(20));
    }
    while std::net::TcpStream::connect((host, s.2)).is_err() {
        assert!(Instant::now() < end, "the server did not listen");
        std::thread::sleep(Duration::from_millis(20));
    }
    s
}

/// A request to `host`'s `port`: its status and body, read for at most 30 seconds, as
/// `support::send` does. An answer with no length (a live connection let in, which stays open) is
/// its head alone; nothing read is status 0, which no test expects.
fn ask(host: &str, port: u16, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> (u16, String) {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect((host, port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    let mut head = format!("{} {} HTTP/1.1\r\nConnection: close\r\nContent-Length: {}\r\n", method, path, body.len());
    for (k, v) in headers {
        head.push_str(&format!("{}: {}\r\n", k, v));
    }
    head.push_str("\r\n");
    s.write_all(head.as_bytes()).unwrap();
    s.write_all(body.as_bytes()).unwrap();
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    while let Ok(n) = s.read(&mut buf) {
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
        let text = String::from_utf8_lossy(&out);
        if let Some((h, b)) = text.split_once("\r\n\r\n") {
            let length = h.lines().find_map(|l| l.split_once(':').filter(|(k, _)| k.eq_ignore_ascii_case("content-length")).and_then(|(_, v)| v.trim().parse::<usize>().ok()));
            let chunked = h.to_ascii_lowercase().contains("transfer-encoding: chunked");
            if length.map_or(!chunked, |n| b.len() >= n) {
                break;
            }
        }
    }
    let text = String::from_utf8_lossy(&out).into_owned();
    let status = text.split(' ').nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    (status, text.split_once("\r\n\r\n").map_or(String::new(), |(_, b)| b.to_string()))
}

fn form(host: &str, port: u16, hs: &[(&str, &str)]) -> u16 {
    let h = if host.contains(':') { format!("[{}]:{}", host, port) } else { format!("{}:{}", host, port) };
    let mut all: Vec<(&str, &str)> = vec![("host", &h), ("content-type", "application/x-www-form-urlencoded")];
    all.extend_from_slice(hs);
    ask(host, port, "POST", "/notes", &all, "text=hi").0
}

// Review 8: `X-Forwarded-Proto` as one header with a list and as two headers says one scheme, the
// last (a proxy appends its own); the mark of a trusted proxy a client writes itself is not
// believed; the scheme compared under --instances and --base, and over IPv6; and the first refusal
// for the scheme alone is said once in the log, with each side's scheme.
#[test]
fn review_q966_lists_forged_marks_instances_base_ipv6_and_the_log() {
    let d = dir("q966-edges", &[("main.qwe", FORM)]);
    let s = logged(&d, &["--trust-proxy", "127.0.0.1"], "127.0.0.1");
    let (port, http, https) = (s.2, format!("http://127.0.0.1:{}", s.2), format!("https://127.0.0.1:{}", s.2));
    for list in [&[("x-forwarded-proto", "http, https")][..], &[("x-forwarded-proto", "http"), ("x-forwarded-proto", "https")][..], &[("x-forwarded-proto", " HTTP ,HTTPS ")][..]] {
        let mut hs = list.to_vec();
        hs.push(("origin", &http));
        assert_eq!(form("127.0.0.1", port, &hs), 403, "{:?}", list);
        let mut hs = list.to_vec();
        hs.push(("origin", &https));
        assert_eq!(form("127.0.0.1", port, &hs), 200, "{:?}", list);
    }
    assert_eq!(form("127.0.0.1", port, &[("x-forwarded-proto", "https, http"), ("origin", &http)]), 200);
    let log = |s: &Logged| std::fs::read_to_string(&s.1).unwrap_or_default();
    let end = Instant::now() + Duration::from_secs(10);
    while !log(&s).contains("taken as another site's") && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }
    let text = log(&s);
    assert_eq!(text.matches("quillo: warning: a request was taken as another site's: its Origin is http:// and the proxy (--trust-proxy) says it came over https. Browsers send no Sec-Fetch-Site to an http:// page, so if this site is served over http:// too and the proxy says https there (a fixed X-Forwarded-Proto on its port-80 site), every browser's write from an http:// page is refused").count(), 1, "{}", text);
    drop(s);
    // A trusted proxy elsewhere: this client's own mark is taken away, its scheme unknown.
    let s = logged(&d, &["--trust-proxy", "10.9.9.9"], "127.0.0.1");
    let http = format!("http://127.0.0.1:{}", s.2);
    assert_eq!(form("127.0.0.1", s.2, &[("x-quillo-from-trusted-proxy", "1"), ("x-forwarded-proto", "https"), ("origin", &http)]), 200);
    drop(s);
    for opts in [&["--trust-proxy", "127.0.0.1", "--instances", "2"][..], &["--trust-proxy", "127.0.0.1", "--base", "/shop"][..]] {
        let s = logged(&d, opts, "127.0.0.1");
        let (http, https) = (format!("http://127.0.0.1:{}", s.2), format!("https://127.0.0.1:{}", s.2));
        for _ in 0..4 {
            assert_eq!(form("127.0.0.1", s.2, &[("x-forwarded-proto", "https"), ("origin", &http)]), 403, "{:?}", opts);
            assert_eq!(form("127.0.0.1", s.2, &[("x-forwarded-proto", "https"), ("origin", &https)]), 200, "{:?}", opts);
        }
    }
    if std::net::TcpListener::bind(("::1", 0)).is_ok() {
        let s = logged(&d, &["--host", "::1", "--trust-proxy", "::1"], "::1");
        let (http, https) = (format!("http://[::1]:{}", s.2), format!("https://[::1]:{}", s.2));
        assert_eq!(form("::1", s.2, &[("x-forwarded-proto", "https"), ("origin", &http)]), 403);
        assert_eq!(form("::1", s.2, &[("x-forwarded-proto", "https"), ("origin", &https)]), 200);
    }
}

// Review 8: the other doors that call `cross_site` take the scheme too: the live connection, the
// WebSocket, and the push subscription's.
#[test]
fn review_q966_the_live_websocket_and_push_doors() {
    let src = "route GET / = <html><body><button @click={() => notification.ask()}>x</button></body></html>\n";
    let d = dir("q966-doors", &[("main.qwe", src)]);
    let s = logged(&d, &["--trust-proxy", "127.0.0.1"], "127.0.0.1");
    let host = format!("127.0.0.1:{}", s.2);
    let (http, https) = (format!("http://{}", host), format!("https://{}", host));
    let door = |method: &str, path: &str, origin: &str, extra: &[(&str, &str)], body: &str| {
        let mut hs: Vec<(&str, &str)> = vec![("host", &host), ("x-forwarded-proto", "https"), ("origin", origin)];
        hs.extend_from_slice(extra);
        ask("127.0.0.1", s.2, method, path, &hs, body)
    };
    assert_eq!(door("GET", "/_quillo/live", &http, &[], "").0, 403);
    assert_eq!(door("GET", "/_quillo/live", &https, &[], "").0, 200);
    let ws: &[(&str, &str)] = &[("upgrade", "websocket"), ("connection", "Upgrade"), ("sec-websocket-version", "13"), ("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")];
    assert_eq!(door("GET", "/_quillo/wt", &http, ws, ""), (403, "only the program's own pages connect\n".to_string()));
    // Past the gate: refused for the ticket it does not have.
    assert_eq!(door("GET", "/_quillo/wt", &https, ws, ""), (403, "no ticket, or an old one\n".to_string()));
    let json: &[(&str, &str)] = &[("content-type", "application/json")];
    let (status, body) = door("POST", "/_quillo/push", &http, json, "{}");
    assert!(status == 403 && body.contains("only the program's own pages subscribe here"), "{} {}", status, body);
    let (status, body) = door("POST", "/_quillo/push", &https, json, "{}");
    assert!(status == 400 && !body.contains("only the program's own pages"), "{} {}", status, body);
}

// ---- the second review ---------------------------------------------------------------------

/// The least of three runs of `f`, in seconds.
fn least(mut f: impl FnMut()) -> f64 {
    (0..3)
        .map(|_| {
            let t = Instant::now();
            f();
            t.elapsed().as_secs_f64()
        })
        .fold(f64::MAX, f64::min)
}

// Second review 1: the highlighter reads a tag from the word, not from its line's start: a line of
// N and of 4N `writes` (and `offline`, `route writes`) take time in proportion, where each word had
// walked back to the line's start (80,000 took 6.2 s). The same through `/_quillo/markdown`, which
// every program has. Loose bounds: CI runs a debug build under coverage.
#[test]
fn second_review_the_highlighter_is_linear_on_a_line_of_tags() {
    let n = 20_000;
    for word in ["writes ", "x offline", "route writes "] {
        let run = |k: usize| quillo::eval_str(&format!("highlight(repeat({:?}, {}), \"quillo\")", word, k)).unwrap();
        let (one, four) = (least(|| drop(run(n))), least(|| drop(run(4 * n))));
        assert!(four < 10.0 * one + 0.05, "{:?}: {} words {:.3}s, {} words {:.3}s", word, n, one, 4 * n, four);
    }
    let d = dir("md-door", &[("main.qwe", "route GET / = \"home\"\n")]);
    let s = logged(&d, &[], "127.0.0.1");
    let body = |k: usize| serde_json::json!([format!("```quillo\n{}\n```\n", "writes ".repeat(k))]).to_string();
    let host = format!("127.0.0.1:{}", s.2);
    let post = |b: &str| {
        let (status, html) = ask("127.0.0.1", s.2, "POST", "/_quillo/markdown", &[("host", &host), ("content-type", "application/json")], b);
        assert!(status == 200 && html.contains("language-quillo"), "{} {}", status, &html[..html.len().min(300)]);
    };
    let (b1, b4) = (body(n), body(4 * n));
    let (one, four) = (least(|| post(&b1)), least(|| post(&b4)));
    assert!(four < 10.0 * one + 0.2, "markdown: {:.3}s and {:.3}s", one, four);
}

// Second review 2: what the server runs to make a component the page shows is the route's: its
// state's initializer calling a server fn that writes, or reads the session; not its handlers.
#[test]
fn second_review_a_shown_components_code_is_followed() {
    let src = r#"server fn mark_sv() {
    db.exec("insert into t (a) values (1)")
    return 1
}
server fn who_sv() {
    return session().user
}
server fn sv() {
    db.exec("delete from t")
    return 1
}
component Mark() {
    state n = mark_sv()
    <p>marked {n} <button @click={() => sv()}>x</button></p>
}
component Who() {
    state u = who_sv()
    <p>{u}</p>
}
component Button() = <button @click={sv}>x</button>
route GET /comp1 {
    db.exec("delete from t")
    return <html><body><Who/></body></html>
}
route GET /comp2 = <html><body><p>{session().user}</p><Mark/></body></html>
route GET /comp3 = <html><body><p>{session().user}</p><Button/></body></html>
"#;
    assert_eq!(
        get_lines("sr2", src),
        [
            "`GET /comp1` writes (`db.exec(\"delete from t\")`) and reads who the visitor is (`session()`, in `who_sv`)".to_string(),
            "`GET /comp2` writes (`db.exec(\"insert into t (a) values (1)\")`, in `mark_sv`) and reads who the visitor is (`session()`)".to_string(),
        ]
    );
}

// Second review 3: what each function can reach is worked out once a check, so 2,000 routes over a
// shared tree of 3,000 functions that reach nothing cost about what the routes alone do (each had
// walked the whole tree); and a tree with a cycle gives one answer whatever the order.
#[test]
fn second_review_reaching_is_worked_out_once() {
    let mut tree = String::new();
    for i in 0..3_000 {
        let kids: Vec<String> = [2 * i + 1, 2 * i + 2].iter().filter(|k| **k < 3_000).map(|k| format!("f{}(x)", k)).collect();
        tree.push_str(&format!("fn f{}(x) {{\n    return [x{}]\n}}\n", i, kids.iter().map(|k| format!(", {}", k)).collect::<String>()));
    }
    let routes = |call: bool| (0..2_000).map(|i| format!("route GET /r{} = str({})\n", i, if call { "f0(1)" } else { "1" })).collect::<String>();
    let (with, without) = (dir("sr3a", &[("main.qwe", &format!("{}{}", tree, routes(true)))]), dir("sr3b", &[("main.qwe", &format!("{}{}", tree, routes(false)))]));
    let (a, b) = (least(|| assert!(check(&with).0)), least(|| assert!(check(&without).0)));
    assert!(a < 3.0 * b + 0.5, "with the calls {:.3}s, without {:.3}s", a, b);
    // A cycle between a reader and a writer, reached from many routes, in both orders.
    let cycle = "fn a(n: int) {\n    session()\n    return n == 0 ? 0 : b(n - 1)\n}\nfn b(n: int) {\n    db.exec(\"delete from t\")\n    return n == 0 ? 0 : c(n - 1)\n}\nfn c(n: int) {\n    return n == 0 ? 0 : a(n - 1)\n}\n";
    let rs = "route GET /x = str(c(3))\nroute GET /y = str(b(3))\nroute GET /z = str(a(3))\n";
    let (_, one) = check_src("sr3c", &format!("{}{}", cycle, rs));
    let (_, two) = check_src("sr3d", &format!("{}{}", rs, cycle));
    assert_eq!(one.len(), 1, "{:?}", one);
    assert!(one[0].contains("this write (`db.exec(\"delete from t\")`, in `b`) is done by 3 GET routes"), "{:?}", one);
    assert_eq!(two.len(), 1, "{:?}", two);
    assert!(two[0].contains("this write (`db.exec(\"delete from t\")`, in `b`) is done by 3 GET routes"), "{:?}", two);
}

// Second review 4: a name the code binds hides the program's only where it is in scope: after its
// `let` (`let user = user()` calls the function), inside its lambda or its loop; a local `db` is
// no module.
#[test]
fn second_review_shadowing_is_scoped() {
    let src = r#"fn user() {
    return session().user
}
fn wipe(u) {
    db.exec("delete from t where who = ?", u)
}
fn forget(id) {
    db.exec("delete from t where a = ?", id)
}
route GET /n1 {
    let user = user()
    db.exec("delete from t where who = ?", user)
    return "ok"
}
route GET /n2 {
    let u = session().user
    let xs = [1, 2] | map((wipe) => wipe + 1)
    wipe(u)
    return str(xs)
}
route GET /n3 {
    forget(1)
    for forget in [1] {
        print(forget)
    }
    return str(session().user)
}
route GET /n4 {
    let db = { exec: (s) => s }
    db.exec("delete from t")
    return str(session().user)
}
"#;
    assert_eq!(
        get_lines("sr4", src),
        [
            "`GET /n1` writes (`db.exec(\"delete from t where who = ?\")`) and reads who the visitor is (`session()`, in `user`)".to_string(),
            "`GET /n2` writes (`db.exec(\"delete from t where who = ?\")`, in `wipe`) and reads who the visitor is (`session()`)".to_string(),
            "`GET /n3` writes (`db.exec(\"delete from t where a = ?\")`, in `forget`) and reads who the visitor is (`session()`)".to_string(),
        ]
    );
}

// Second review 5: an ANY route answers a GET, so it is said, and takes `writes`.
#[test]
fn second_review_an_any_route_is_a_get_too() {
    let src = "route ANY /wipe {\n    if session().user == null {\n        return \"no\"\n    }\n    db.exec(\"delete from t\")\n    return \"wiped\"\n}\nroute writes ANY /ok {\n    db.exec(\"delete from t\")\n    return str(session().user)\n}\nroutes writes /g {\n    route ANY /x {\n        db.exec(\"delete from t\")\n        return str(session().user)\n    }\n}\n";
    assert_eq!(get_lines("sr5", src), ["`ANY /wipe` writes (`db.exec(\"delete from t\")`) and reads who the visitor is (`session()`)"]);
}

// Second review 6: a route is put with others under one write only where that is its one write; one
// that does another says them all, nearest first. An editor is told where the grouped routes are.
#[test]
fn second_review_a_group_hides_no_other_write() {
    let src = "fn seen() {\n    db.exec(\"update t set a = 1 where who = ?\", session().user)\n}\nfn wipe() {\n    db.exec(\"delete from t where who = ?\", session().user)\n}\nroute GET /r0 {\n    seen()\n    return \"0\"\n}\nroute GET /r1 {\n    seen()\n    return \"1\"\n}\nroute GET /r2 {\n    seen()\n    return \"2\"\n}\nroute GET /r3 {\n    seen()\n    wipe()\n    return \"3\"\n}\n";
    assert_eq!(
        get_lines("sr6", src),
        [
            "this write (`db.exec(\"update t set a = 1 where who = ?\")`, in `seen`) is done by 3 GET routes that read who the visitor is: `GET /r0` (line 7), `GET /r1` (line 11) and `GET /r2` (line 15)".to_string(),
            "`GET /r3` writes (`db.exec(\"update t set a = 1 where who = ?\")`, in `seen`; `db.exec(\"delete from t where who = ?\")`, in `wipe`) and reads who the visitor is (`session()`, in `seen`)".to_string(),
        ]
    );
    let d = support::temp_dir("f70-lsp-related").canonicalize().unwrap();
    let (mut l, uri) = Lsp::open(&d, src);
    let m = l.wait("the diagnostics", |m| m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == uri.as_str());
    let grouped = m["params"]["diagnostics"].as_array().unwrap().iter().find(|d| d["message"].as_str().unwrap_or("").starts_with("this write (")).cloned().unwrap_or_default();
    let lines: Vec<(String, u64, String)> = grouped["relatedInformation"]
        .as_array()
        .map(|a| a.iter().map(|r| (r["location"]["uri"].as_str().unwrap_or("").to_string(), r["location"]["range"]["start"]["line"].as_u64().unwrap_or(0), r["message"].as_str().unwrap_or("").to_string())).collect())
        .unwrap_or_default();
    assert_eq!(lines, [(uri.clone(), 6, "`GET /r0`".to_string()), (uri.clone(), 10, "`GET /r1`".to_string()), (uri.clone(), 14, "`GET /r2`".to_string())], "{}", grouped);
}

// Second review 7 and 8: `sql\`...\`` is its text (one with values put in is built as it runs, not
// guessed); `?.` is `.`, for nobody's fields as for the visitor's.
#[test]
fn second_review_sql_tags_and_optional_fields() {
    let src = r#"route GET /s1 {
    let r = db.one(sql`insert into t (a, who) values (1, ?) returning id`, str(session().user))
    return str(r)
}
route GET /s2 {
    let r = db.one(sql`
        insert into t (a, who) values (1, ?)
        returning id`, str(session().user))
    return str(r)
}
route GET /s3 {
    let r = db.query(sql`select * from t where who = ?`, str(session().user))
    return str(r)
}
route GET /q6 {
    let r = request?.cookies?.sid
    db.exec("delete from t")
    return str(r)
}
route GET /q7 {
    db.exec("delete from t")
    return request?.path ?? ""
}
"#;
    assert_eq!(
        get_lines("sr7", src),
        [
            "`GET /s1` writes (`db.one(\"insert into t (a, who) values (1, ?) returning id\")`, SQL that writes) and reads who the visitor is (`session()`)".to_string(),
            "`GET /s2` writes (`db.one(\"insert into t (a, who) values (1, ?) returning id\")`, SQL that writes) and reads who the visitor is (`session()`)".to_string(),
            "`GET /q6` writes (`db.exec(\"delete from t\")`) and reads who the visitor is (the `sid` cookie)".to_string(),
        ]
    );
}

// Second review 9: the line for a scheme refused says who is refused, either way: a proxy saying
// https on an http:// site refuses every browser there; one saying http behind a TLS hop, older
// browsers on https://.
#[test]
fn second_review_q966_the_log_says_who_is_refused() {
    let d = dir("q966-log", &[("main.qwe", FORM)]);
    let s = logged(&d, &["--trust-proxy", "127.0.0.1"], "127.0.0.1");
    let https = format!("https://127.0.0.1:{}", s.2);
    assert_eq!(form("127.0.0.1", s.2, &[("x-forwarded-proto", "http"), ("origin", &https)]), 403);
    let end = Instant::now() + Duration::from_secs(30);
    let text = loop {
        let t = std::fs::read_to_string(&s.1).unwrap_or_default();
        if t.contains("taken as another site's") || Instant::now() > end {
            break t;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(text.contains("quillo: warning: a request was taken as another site's: its Origin is https:// and the proxy (--trust-proxy) says it came over http. If browsers reach this site over https://, the proxy is saying its own hop's scheme"), "{}", text);
}

// Second review 6, as the sweep found it: a helper that does two writes (a page's count of views) is
// one set of writes, said once for the routes whose writes are just those; what `every` is given
// runs later (in a component, in the browser: the page's clock), so it is no write of the GET.
#[test]
fn second_review_a_set_of_writes_and_later_code() {
    let helper = "fn hit(k: str) {\n    db.exec(\"insert or ignore into seen (k, who) values (?, ?)\", k, session().user)\n    db.exec(\"update hits set n = n + 1 where k = ?\", k)\n}\n";
    let routes = (0..4).map(|i| format!("route GET /p{} {{\n    hit(\"p{}\")\n    return \"ok\"\n}}\n", i, i)).collect::<String>();
    let got = get_lines("sr6b", &format!("{}{}", helper, routes));
    assert_eq!(got, ["these writes (`db.exec(\"insert or ignore into seen (k, who) values (?, ?)\")`, in `hit`; `db.exec(\"update hits set n = n + 1 where k = ?\")`, in `hit`) are done by 4 GET routes that read who the visitor is: `GET /p0` (line 5), `GET /p1` (line 9), `GET /p2` (line 13) and 1 more"]);
    let later = r#"server fn save(t: str) {
    db.exec("update notes set t = ?", t)
    return 1
}
component Editor() {
    state note = ""
    if note != "" {
        every(500, () => save(note))
    }
    <textarea bind:value={note}></textarea>
}
route GET /e = <html><body><p>{session().user}</p><Editor/></body></html>
"#;
    assert_eq!(get_lines("sr6c", later), Vec::<String>::new());
}
