//! A phone app pointed at a domain (FEEDBACK33.md, Q-406 to Q-414; branch feedback27-35-fixes):
//! `env.NAME` in app.toml read on the phone, what of a program's folder goes into an app, and
//! `site shop = url`, a deployed site's server fns called as the program's own, its cookies kept
//! where the program is one person's. The shells' frames and `site =` are Swift and Kotlin,
//! checked by their compilers; the page's message for another site's fetch is in the runtime.

mod support;

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

fn dir(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = support::temp_dir(&format!("feedback33-{}", name));
    for (f, text) in files {
        let p = d.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    d
}

// ---- Q-406: env.NAME in app.toml --------------------------------------------------------------

#[test]
fn app_toml_env_lines_are_what_env_gives_on_the_phone() {
    let d = dir("app-env", &[(
        "app.toml",
        "name = \"Probe\"\nenv.API_URL = \"https://app.example.com\"   # the domain\nenv.PLAIN = staging\nenv.QUOTED = 'single # kept'\nenv.ESC = \"a\\\"b\\nc\"\nenv.bad-name = \"x\"\nenvironment = \"no\"\n  env.SPACED  =  \"yes\"\n",
    )]);
    let m = quillo::mobile::app_env(&d.join("app.toml"));
    assert_eq!(m.get("API_URL").map(String::as_str), Some("https://app.example.com"));
    assert_eq!(m.get("PLAIN").map(String::as_str), Some("staging"));
    assert_eq!(m.get("QUOTED").map(String::as_str), Some("single # kept"));
    assert_eq!(m.get("ESC").map(String::as_str), Some("a\"b\nc"));
    assert_eq!(m.get("SPACED").map(String::as_str), Some("yes"));
    assert_eq!(m.len(), 5, "{:?}", m);
    assert!(quillo::mobile::app_env(&d.join("missing.toml")).is_empty());
}

// ---- Q-407, Q-408: what goes into an app ------------------------------------------------------

#[cfg(unix)]
#[test]
fn an_app_carries_no_dotenv_no_git_and_keeps_underscore_folders() {
    let d = dir("copy", &[
        ("prog/main.qwe", "route GET / = \"x\"\n"),
        ("prog/.env", "SECRET=1\n"),
        ("prog/.env.production", "SECRET=2\n"),
        ("prog/.git/HEAD", "ref\n"),
        ("prog/.quillo/bin/quillo", "bin"),
        ("prog/_lib/x.qwe", "fn x() = 1\n"),
        ("prog/public/.well-known/assetlinks.json", "[]"),
        ("prog/sub/main.qwe", "route GET / = \"other\"\n"),
        ("prog/app.db", "db"),
        ("prog/uploads/a.png", "a"),
        ("prog/app.toml", "name = \"T\"\n"),
    ]);
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("mobile/app-config.sh");
    let fn_only = format!("eval \"$(sed -n '/^app_copy_program()/,/^}}/p' '{}')\"; app_copy_program \"$1\" \"$2\"", script.display());
    let o = Command::new("sh").args(["-c", &fn_only, "sh"]).arg(d.join("prog")).arg(d.join("out")).output().unwrap();
    let said = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let mut got: Vec<String> = walk(&d.join("out")).into_iter().map(|p| p.strip_prefix(d.join("out")).unwrap().display().to_string()).collect();
    got.sort();
    assert_eq!(got, ["_lib/x.qwe", "app.toml", "main.qwe", "public/.well-known/assetlinks.json"]);
    assert!(said.contains("leaving out: .env .env.production .git .quillo"), "{}", said);
    assert!(said.contains("a .env is the server's: put public values in app.toml as env.NAME"), "{}", said);
    // Android keeps what the script put there.
    let gradle = std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("mobile/android/app/build.gradle.kts")).unwrap();
    assert!(gradle.contains("ignoreAssetsPattern = \"!.svn:!.git:!.ds_store:!*.scc:!CVS:!thumbs.db:!picasa.ini:!*~\""));
}

fn walk(d: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(d).unwrap().flatten() {
        if e.file_type().unwrap().is_dir() {
            out.extend(walk(&e.path()));
        } else {
            out.push(e.path());
        }
    }
    out
}

// ---- Q-412: site ------------------------------------------------------------------------------

struct Running(Child, u16);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn domain() -> (PathBuf, Running) {
    let d = dir("domain", &[(
        "main.qwe",
        r#"server fn notes() = [{ id: 1, text: "from the domain" }]
server fn sign_in(name: str) { set_session({ user: name }); name }
server fn sign_out() { end_session(); "out" }
server fn who() = session()?.user ?? "nobody"
server fn fails() = err("no such note")
server fn price(n: int) = 12.50m * n
route GET / = "the domain"
"#,
    )]);
    let port = support::free_port();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(d.join("main.qwe"))
        .args(["--port", &port.to_string(), "--plain-http"])
        .env("QUILLO_SECRET", "a test secret for the domain, long enough")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    support::stop_with_tests(&child);
    let deadline = Instant::now() + Duration::from_secs(20);
    while !support::listening(port) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(30));
    }
    (d, Running(child, port))
}

fn caller(name: &str, src: &str, keep: bool) -> Output {
    let d = dir(name, &[("main.qwe", src)]);
    let mut c = Command::new(env!("CARGO_BIN_EXE_quillo"));
    c.arg("run").arg(d.join("main.qwe")).current_dir(&d).stdin(Stdio::null());
    if keep {
        c.env("QUILLO_SITE_COOKIES", "1");
    }
    c.output().unwrap()
}

#[test]
fn a_sites_server_fns_are_called_as_the_programs_own() {
    let (_d, dom) = domain();
    let src = format!(
        "site shop = env(\"SHOP_URL\", \"http://127.0.0.1:{}/\")\nlet site = \"still a name\"\nprint(shop.notes())\nprint(shop.sign_in(\"ann\"), shop.who(), shop.price(2) + 1m, site)\nprint(shop.fails(), shop.nope())\nprint(type_of(shop.price(\"x\")))\n",
        dom.1
    );
    let o = caller("site-server", &src, false);
    let out = String::from_utf8_lossy(&o.stdout);
    let err = String::from_utf8_lossy(&o.stderr);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], r#"[{ id: 1, text: "from the domain" }]"#, "{}", err);
    // On a server the site's cookies are not kept: no one is signed in at the next call.
    assert_eq!(lines[1], "ann nobody 26.00 still a name", "{}", err);
    assert!(err.contains("the cookies it set are not kept on a server"), "{}", err);
    assert_eq!(lines[2], r#"err("no such note") err("site shop: `nope` answered 404: no server fn `nope`")"#);
    assert_eq!(lines[3], "error");
}

#[test]
fn a_site_keeps_its_cookies_where_the_program_is_one_persons() {
    let (_d, dom) = domain();
    let first = caller("site-keep", &format!("site shop = \"http://127.0.0.1:{}\"\nprint(shop.sign_in(\"ann\"), shop.who())\n", dom.1), true);
    assert_eq!(String::from_utf8_lossy(&first.stdout), "ann ann\n", "{}", String::from_utf8_lossy(&first.stderr));
    // Another run of the same program in the same folder: still signed in, then signed out.
    let dir_of = std::env::temp_dir().join(format!("quillo-feedback33-site-keep-{}", std::process::id()));
    std::fs::write(dir_of.join("main.qwe"), format!("site shop = \"http://127.0.0.1:{}\"\nprint(shop.who(), shop.sign_out(), shop.who())\n", dom.1)).unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("run").arg(dir_of.join("main.qwe")).current_dir(&dir_of).env("QUILLO_SITE_COOKIES", "1").output().unwrap();
    assert_eq!(String::from_utf8_lossy(&o.stdout), "ann out nobody\n", "{}", String::from_utf8_lossy(&o.stderr));
    assert!(dir_of.join(".quillo/sites/shop.json").exists());
}

#[test]
fn a_site_is_checked_and_refused_in_the_browser() {
    let d = dir("site-check", &[(
        "main.qwe",
        "site shop = \"https://shop.example.com\"\nserver fn n() = shop.notes()\ncomponent C() {\n    state x = null\n    <button @click={() => { x = shop.notes() }}>{x}</button>\n}\nroute GET / = <html><C/></html>\n",
    )]);
    let o = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("check").arg(d.join("main.qwe")).output().unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("main.qwe:5: error: `shop.notes()` calls the site from the server, and <C> also runs in the browser"), "{}", err);
    for (bad, said) in [("site shop = 3", "needs an address, as text"), ("site shop = \"ftp://x\"", "needs an address like https://"), ("site shop = \"https://x/?a\"", "needs an address like")] {
        let d = dir("site-bad", &[("main.qwe", &format!("{}\n", bad))]);
        let o = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("run").arg(d.join("main.qwe")).arg("--no-check").output().unwrap();
        assert!(String::from_utf8_lossy(&o.stderr).contains(said), "{}: {}", bad, String::from_utf8_lossy(&o.stderr));
    }
    // A site nobody answers at: an error value with the reason.
    let d = dir("site-down", &[("main.qwe", &format!("site shop = \"http://127.0.0.1:{}\"\nprint(is_err(shop.notes()), shop.notes().error)\n", support::free_port()))]);
    let o = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("run").arg(d.join("main.qwe")).output().unwrap();
    assert!(String::from_utf8_lossy(&o.stdout).starts_with("true site shop: no answer from http://127.0.0.1:"), "{}", String::from_utf8_lossy(&o.stdout));
}
