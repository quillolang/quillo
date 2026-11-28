//! FEEDBACK82.md (slice P of FEEDBACK82-85; branch feedback82-85): what a provider's policy can
//! hold a program to. Q-1280: `[run]`, `[fs] paths` and `[env] set`, each only narrowed by later
//! layers; Q-1281/Q-1282: the anchor's `include` and `require_path`, `policy.d/`,
//! `QUILLO_POLICY_PATH`; Q-1283: a layer there but broken stops the program; Q-1284: phone apps read
//! the program's policy; Q-1285: plugins held to their SHA-256; Q-1286: `quillo policy`.

mod support;

use quillo::policy::{self, Kind, Policy};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use support::{send, temp_dir};

/// `quillo` with `args` in `dir`, ended after 60 s: its exit code and what it wrote.
fn quillo(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> (Option<i32>, String) {
    quillo_without(dir, args, env, &[])
}

fn confirm(dir: &Path, file: &str) {
    let (code, out) = quillo(dir, &["confirm", "--yes", file], &[]);
    assert_eq!(code, Some(0), "{}", out);
}

/// `quillo`, with the variables `unset` not in its environment.
fn quillo_without(dir: &Path, args: &[&str], env: &[(&str, &str)], unset: &[&str]) -> (Option<i32>, String) {
    let log = dir.join("run.log");
    let file = std::fs::File::create(&log).unwrap();
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_quillo"));
    for k in unset {
        cmd.env_remove(k);
    }
    let mut child = cmd
        .args(args)
        .current_dir(dir)
        .env_remove(policy::PATH_VAR)
        // `cargo test` gives its tests these; a program's run has them only where a test says so.
        .env_remove("CARGO")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTFLAGS")
        // A test sets it where it means to: one the run itself has would beat a .env's.
        .env_remove("CARGO_TARGET_DIR")
        .envs(env.iter().copied())
        .stdin(std::process::Stdio::null())
        .stdout(file.try_clone().unwrap())
        .stderr(file)
        .spawn()
        .unwrap();
    // A pinned plugin is built from nothing, its dependencies too.
    let deadline = Instant::now() + Duration::from_secs(if args.contains(&"slow") { 600 } else { 60 });
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

/// Layers read in order, each named `layer0`, `layer1`, ..., as the host's layers are (those
/// `QUILLO_POLICY_PATH` names).
fn layers(texts: &[&str]) -> Result<Policy, String> {
    let mut p = Policy::default();
    for (i, t) in texts.iter().enumerate() {
        p.add_layer(Path::new(&format!("layer{}", i)), t, Kind::PathVar)?;
    }
    Ok(p)
}

fn net(s: &str) -> policy::Net {
    policy::nets(s).unwrap()[0]
}

// ---- Q-1280: [run], [fs] paths, [env] set ----------------------------------------------------

#[test]
fn trust_proxy_only_narrows() {
    // A later layer inside the first narrows it; one wider keeps the first's.
    let p = layers(&["[run]\ntrust_proxy = [\"10.0.0.0/8\"]\n", "[run]\ntrust_proxy = [\"10.1.0.0/16\"]\n"]).unwrap();
    assert_eq!(p.run.trust_proxy, Some(vec![net("10.1.0.0/16")]));
    assert_eq!(p.origin_of("run.trust_proxy"), Some(Path::new("layer1")));
    let p = layers(&["[run]\ntrust_proxy = [\"10.0.0.0/8\"]\n", "[run]\ntrust_proxy = [\"0.0.0.0/0\"]\n"]).unwrap();
    assert_eq!(p.run.trust_proxy, Some(vec![net("10.0.0.0/8")]), "0.0.0.0/0 widens nothing");
    // Apart: none trusted; and [] trusts none.
    let p = layers(&["[run]\ntrust_proxy = [\"10.0.0.0/8\"]\n", "[run]\ntrust_proxy = [\"192.168.0.0/16\"]\n"]).unwrap();
    assert_eq!(p.run.trust_proxy, Some(vec![]));
    let p = layers(&["[run]\ntrust_proxy = []\n"]).unwrap();
    assert_eq!(p.run.trust_proxy, Some(vec![]));
    assert!(p.wider_proxy(&[net("127.0.0.1")]).is_some(), "[] lets --trust-proxy name nothing");
    // The limit and one either side: a /8's first and last address are in it, the next is not.
    let p = layers(&["[run]\ntrust_proxy = [\"10.0.0.0/8\", \"::1\"]\n"]).unwrap();
    assert!(p.wider_proxy(&[net("10.0.0.0"), net("10.255.255.255"), net("10.0.0.0/8"), net("::1")]).is_none());
    assert_eq!(p.wider_proxy(&[net("11.0.0.0")]), Some(&net("11.0.0.0")));
    assert_eq!(p.wider_proxy(&[net("9.255.255.255")]), Some(&net("9.255.255.255")));
    assert_eq!(p.wider_proxy(&[net("10.0.0.0/7")]), Some(&net("10.0.0.0/7")), "a wider network is refused");
    // IPv4 written as IPv6 is IPv4.
    assert!(p.wider_proxy(&[net("::ffff:10.1.2.3")]).is_none());
    assert!(layers(&["[run]\ntrust_proxy = [\"10.0.0.0/33\"]\n"]).unwrap_err().contains("is not an address or a network"));
    assert!(layers(&["[run]\ntrust_proxy = [\"a,b\"]\n"]).is_err());
    assert!(layers(&["[run]\ntrust_proxy = \"10.0.0.0/8\"\n"]).unwrap_err().contains("is a list"));
    // No layer says it: nothing held.
    assert!(Policy::default().wider_proxy(&[net("0.0.0.0/0")]).is_none());
}

#[test]
fn a_switch_turned_off_stays_off() {
    for k in policy::SWITCHES {
        let p = layers(&[&format!("[run]\n{} = false\n", k), &format!("[run]\n{} = true\n", k)]).unwrap();
        assert_eq!(p.switched_off(k), Some(Path::new("layer0")), "{}", k);
        let p = layers(&[&format!("[run]\n{} = true\n", k)]).unwrap();
        assert_eq!(p.switched_off(k), None, "true alone turns nothing off: {}", k);
        assert!(layers(&[&format!("[run]\n{} = \"no\"\n", k)]).unwrap_err().contains("wrong kind of value"));
    }
    let p = layers(&["[run]\ncheck = true\n", "[run]\ncheck = false\n"]).unwrap();
    assert!(p.run.check);
    assert!(!layers(&["[run]\ncheck = false\n"]).unwrap().run.check);
}

#[test]
fn a_ceiling_takes_the_least() {
    for k in ["instances", "max_connections", "connections_per_client", "live_per_client"] {
        let p = layers(&[&format!("[run]\n{} = 4\n", k), &format!("[run]\n{} = 8\n", k)]).unwrap();
        assert_eq!((p.run_ceiling(k), p.origin_of(&format!("run.{}", k))), (Some(4), Some(Path::new("layer0"))), "{}", k);
        let p = layers(&[&format!("[run]\n{} = 8\n", k), &format!("[run]\n{} = 4\n", k), &format!("[run]\n{} = 4\n", k)]).unwrap();
        assert_eq!((p.run_ceiling(k), p.origin_of(&format!("run.{}", k))), (Some(4), Some(Path::new("layer1"))), "{}", k);
        assert_eq!(layers(&[&format!("[run]\n{} = 1\n", k)]).unwrap().run_ceiling(k), Some(1));
        assert!(layers(&[&format!("[run]\n{} = 0\n", k)]).unwrap_err().contains("is a positive number"), "{}", k);
        assert!(layers(&[&format!("[run]\n{} = -1\n", k)]).is_err());
    }
    let p = layers(&["[run]\ndrain = \"30s\"\n", "[run]\ndrain = \"1m\"\n", "[run]\ndrain = \"500ms\"\n"]).unwrap();
    assert_eq!(p.run_ceiling("drain"), Some(500));
    assert!(layers(&["[run]\ndrain = \"0s\"\n"]).unwrap_err().contains("longer than 0"));
    assert!(layers(&["[run]\ndrain = \"soon\"\n"]).is_err());
    // Permission bits: each layer's at once; the owner's read and write kept.
    let p = layers(&["[run]\nsocket_mode = \"660\"\n", "[run]\nsocket_mode = \"640\"\n"]).unwrap();
    assert_eq!(p.run.socket_mode, Some(0o640));
    let p = layers(&["[run]\nsocket_mode = \"644\"\n", "[run]\nsocket_mode = \"660\"\n"]).unwrap();
    assert_eq!(p.run.socket_mode, Some(0o640));
    assert_eq!(layers(&["[run]\nsocket_mode = \"777\"\n"]).unwrap().run.socket_mode, Some(0o777));
    for bad in ["400", "1777", "x", ""] {
        assert!(layers(&[&format!("[run]\nsocket_mode = \"{}\"\n", bad)]).is_err(), "{}", bad);
    }
}

#[test]
fn a_forced_value_cannot_be_changed() {
    let p = layers(&["[run]\nlog = \"json\"\nbase = \"/shop/\"\nlisten = \"unix\"\n", "[run]\nlog = \"json\"\nbase = \"/shop\"\n"]).unwrap();
    assert_eq!((p.run_forced("log"), p.run_forced("base"), p.run_forced("listen")), (Some("json"), Some("/shop"), Some("unix")));
    assert_eq!(p.origin_of("run.base"), Some(Path::new("layer0")));
    let e = layers(&["[run]\nlog = \"json\"\n", "[run]\nlog = \"text\"\n"]).unwrap_err();
    assert!(e.contains("layer1:2: `run.log` is \"json\" in layer0, which a later layer cannot change"), "{}", e);
    assert!(layers(&["[run]\nbase = \"/a\"\n", "[run]\nbase = \"/b\"\n"]).is_err());
    assert!(layers(&["[run]\nlog = \"xml\"\n"]).unwrap_err().contains("text, json or off"));
    assert!(layers(&["[run]\nlisten = \"tcp\"\n"]).unwrap_err().contains("a Unix socket only"));
    for bad in ["shop", "/a//b", "/a?b"] {
        assert!(layers(&[&format!("[run]\nbase = \"{}\"\n", bad)]).is_err(), "{}", bad);
    }
    // Unicode in a path is a path.
    assert_eq!(layers(&["[run]\nbase = \"/café\"\n"]).unwrap().run_forced("base"), Some("/café"));
}

#[test]
fn fs_paths_hold_allow_path() {
    let d = temp_dir("f82-fs-paths");
    let shared = d.join("shared");
    let inner = shared.join("ünïcode");
    let other = d.join("other");
    for f in [&inner, &other] {
        std::fs::create_dir_all(f).unwrap();
    }
    let real = |p: &Path| p.canonicalize().unwrap();
    let p = layers(&[&format!("[fs]\npaths = [\"{}\"]\n", shared.display())]).unwrap();
    assert!(p.allows_path(&real(&shared)).is_ok(), "the folder itself");
    assert!(p.allows_path(&real(&inner)).is_ok(), "one inside it");
    assert!(p.allows_path(&real(&other)).unwrap_err().contains("is not inside the folders layer0 allows"));
    assert!(p.allows_path(Path::new("/")).is_err());
    // A name that begins as the folder's is not inside it.
    std::fs::create_dir_all(d.join("shared2")).unwrap();
    assert!(p.allows_path(&real(&d.join("shared2"))).is_err());
    // Each layer's must hold: a second narrows to the inner folder.
    let p = layers(&[&format!("[fs]\npaths = [\"{}\"]\n", shared.display()), &format!("[fs]\npaths = [\"{}\"]\n", inner.display())]).unwrap();
    assert!(p.allows_path(&real(&inner)).is_ok());
    assert!(p.allows_path(&real(&shared)).unwrap_err().contains("layer1"));
    // A later layer wider than the first does not widen it.
    let p = layers(&[&format!("[fs]\npaths = [\"{}\"]\n", inner.display()), "[fs]\npaths = [\"/\"]\n"]).unwrap();
    assert!(p.allows_path(&real(&shared)).unwrap_err().contains("layer0"));
    // [] allows none; relative paths are refused.
    assert!(layers(&["[fs]\npaths = []\n"]).unwrap().allows_path(&real(&inner)).unwrap_err().contains("allows no folder outside the program's"));
    assert!(layers(&["[fs]\npaths = [\"shared\"]\n"]).unwrap_err().contains("by their whole path"));
    assert!(Policy::default().allows_path(Path::new("/")).is_ok());
}

#[test]
fn env_set_is_forced_and_only_added_to() {
    let p = layers(&["[env]\nset = { APP_ENV = \"production\", REGION = \"eu, west = 1\" }\n"]).unwrap();
    assert_eq!(p.env_set, [("APP_ENV".to_string(), "production".to_string()), ("REGION".to_string(), "eu, west = 1".to_string())]);
    // A later layer may say the same, or set another name; never change one.
    let p = layers(&["[env]\nset = { APP_ENV = \"production\" }\n", "[env.set]\nAPP_ENV = \"production\"\nTZ = \"UTC\"\n"]).unwrap();
    assert_eq!(p.env_set.len(), 2);
    let e = layers(&["[env]\nset = { APP_ENV = \"production\" }\n", "[env]\nset = { APP_ENV = \"dev\" }\n"]).unwrap_err();
    // Its name, never its value, which may be a secret (found in review).
    assert!(e.contains("`env.set.APP_ENV` is set in layer0, which a later layer cannot change") && !e.contains("production"), "{}", e);
    // One name, none, unicode in a value; what is not a table of texts.
    assert_eq!(layers(&["[env]\nset = { A = \"\" }\n"]).unwrap().env_set, [("A".to_string(), String::new())]);
    assert!(layers(&["[env]\nset = {}\n"]).unwrap().env_set.is_empty());
    assert_eq!(layers(&["[env]\nset = { A = \"ünï\" }\n"]).unwrap().env_set[0].1, "ünï");
    for bad in ["{ A = 1 }", "{ A = \"x\", A = \"y\" }", "{ = \"x\" }", "{ A-B = \"x\" }", "{ A = \"x\" B = \"y\" }"] {
        assert!(layers(&[&format!("[env]\nset = {}\n", bad)]).is_err(), "{}", bad);
    }
    // What a layer sets, its own names let env() read; an earlier layer's names still hold.
    let p = layers(&["[env]\nnames = [\"DATABASE_URL\"]\nset = { APP_ENV = \"production\" }\n"]).unwrap();
    assert!(p.env_name("APP_ENV").is_ok() && p.env_name("DATABASE_URL").is_ok());
    let p = layers(&["[env]\nnames = [\"DATABASE_URL\"]\n", "[env]\nnames = [\"DATABASE_URL\"]\nset = { APP_ENV = \"x\" }\n"]).unwrap();
    assert!(p.env_name("APP_ENV").unwrap_err().contains("layer0"));
}

#[test]
fn what_a_run_section_does_not_say_is_refused() {
    let e = layers(&["[run]\nport = 80\n"]).unwrap_err();
    assert!(e.contains("`run.port` is not something a policy says") && e.contains("run.trust_proxy"), "{}", e);
    assert!(layers(&["[fs]\nread = true\n"]).is_err());
    // include and require_path are the host's file's alone.
    assert!(layers(&["include = [\"/x.toml\"]\n"]).unwrap_err().contains("is said only by the host's file"));
    assert!(layers(&["require_path = true\n"]).is_err());
    // The eleven settings there were read as before.
    let p = layers(&["[limits]\nworkers = 8\n", "[limits]\nworkers = 16\n"]).unwrap();
    assert_eq!((p.workers, p.origin_of("limits.workers")), (Some(8), Some(Path::new("layer0"))));
}

/// A program that prints what it gets, and a provider's policy for it.
fn provider(name: &str, policy: &str, main: &str) -> PathBuf {
    let d = temp_dir(name);
    std::fs::write(d.join("p.toml"), policy).unwrap();
    std::fs::write(d.join("main.qwe"), main).unwrap();
    d
}

#[test]
fn run_refuses_an_option_a_switch_turned_off() {
    let d = provider(
        "f82-switches",
        "[run]\nwith = false\nwatch = false\ndescribe = false\nplain_http = false\nsplit = false\njoin = false\ncheck = true\n",
        "print(\"ran\")\n",
    );
    for (args, said) in [
        (&["--with", "echo hi"][..], "--with is turned off by the policy p.toml (run.with = false)"),
        (&["--watch"], "--watch is turned off by the policy p.toml (run.watch = false)"),
        (&["-w"], "-w is turned off"),
        (&["--watch=reload"], "--watch=reload is turned off"),
        (&["--reload"], "--reload is turned off"),
        (&["--describe"], "--describe is turned off"),
        (&["--plain-http"], "--plain-http is turned off"),
        (&["--split"], "--split is turned off"),
        (&["--scale", "a=2"], "--scale is turned off"),
        (&["--join", "http://127.0.0.1:9"], "--join is turned off"),
        (&["--register", "http://127.0.0.1:9"], "--register is turned off"),
        (&["--allow-join", "billing"], "--allow-join is turned off by the policy p.toml (run.join = false)"),
        (&["--join-as", "http://127.0.0.1:9"], "--join-as is turned off"),
        (&["--service", "pool"], "--service is turned off"),
        (&["--as-service"], "--as-service is turned off by the policy p.toml (run.split = false)"),
        (&["--checked"], "--checked is refused by the policy p.toml (run.check = true)"),
        (&["--no-check"], "--no-check is refused by the policy p.toml (run.check = true)"),
    ] {
        let mut a = vec!["run", "main.qwe", "--policy", "p.toml"];
        a.extend_from_slice(args);
        let (code, out) = quillo(&d, &a, &[]);
        assert_eq!(code, Some(2), "{:?}: {}", args, out);
        assert!(out.contains(said) && !out.contains("ran"), "{:?}: {}", args, out);
    }
    // Without them it runs; a value that is an option's name is no option.
    let (code, out) = quillo(&d, &["run", "main.qwe", "--policy", "p.toml", "--service-name", "x"], &[]);
    assert_eq!((code, out.contains("ran")), (Some(0), true), "{}", out);
    // Under no policy, the same options are the developer's.
    let (code, out) = quillo(&d, &["run", "main.qwe", "--with", "true"], &[]);
    assert_eq!((code, out.contains("ran")), (Some(0), true), "{}", out);
}

#[test]
fn run_holds_forced_values() {
    let d = provider("f82-forced", "[run]\nlog = \"json\"\nbase = \"/shop\"\n", "print(\"ran\")\n");
    let (code, out) = quillo(&d, &["run", "main.qwe", "--policy", "p.toml", "--log", "text"], &[]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("--log text is not the request log the policy p.toml sets (run.log = \"json\")"), "{}", out);
    let (code, out) = quillo(&d, &["run", "main.qwe", "--policy", "p.toml", "--base", "/other"], &[]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("--base /other is not the path the policy p.toml serves the program under"), "{}", out);
    for same in [&["--log", "json"][..], &["--base", "/shop/"], &[]] {
        let mut a = vec!["run", "main.qwe", "--policy", "p.toml"];
        a.extend_from_slice(same);
        let (code, out) = quillo(&d, &a, &[]);
        assert_eq!((code, out.contains("ran")), (Some(0), true), "{:?}: {}", same, out);
    }
    // A Unix socket only: refused on a port, taken on a socket.
    std::fs::write(d.join("u.toml"), "[run]\nlisten = \"unix\"\n").unwrap();
    let (code, out) = quillo(&d, &["run", "main.qwe", "--policy", "u.toml", "--port", "0"], &[]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("has the program listen on a Unix socket only (run.listen = \"unix\")"), "{}", out);
    let (code, out) = quillo(&d, &["run", "main.qwe", "--policy", "u.toml", "--listen", "unix:s.sock", "--trust-proxy", "127.0.0.1"], &[]);
    assert_eq!((code, out.contains("ran")), (Some(0), true), "{}", out);
}

#[test]
fn trust_proxy_from_the_policy_is_believed_and_a_wider_one_refused() {
    let d = provider("f82-proxy", "[run]\ntrust_proxy = [\"127.0.0.1\"]\n", "route GET / = request.ip\n");
    let p = d.join("p.toml").display().to_string();
    // Measured in FEEDBACK82 Part A: --trust-proxy 0.0.0.0/0 made request.ip what a visitor wrote.
    let (code, out) = quillo(&d, &["run", "main.qwe", "--policy", "p.toml", "--trust-proxy", "0.0.0.0/0"], &[]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("--trust-proxy 0.0.0.0/0 is not among the proxies the policy p.toml trusts: 127.0.0.1"), "{}", out);
    // Not given: the policy's proxy is believed, as the provider's proxy is the only one.
    let s = support::serve_with(&d.join("main.qwe"), &["--policy", &p]);
    assert_eq!(send(s.port, "GET", "/", &[("X-Forwarded-For", "6.6.6.6")], "").body, "6.6.6.6");
    drop(s);
    // [] trusts none: a visitor's header is not believed, and the option is refused.
    std::fs::write(d.join("none.toml"), "[run]\ntrust_proxy = []\n").unwrap();
    let s = support::serve_with(&d.join("main.qwe"), &["--policy", &d.join("none.toml").display().to_string()]);
    assert_eq!(send(s.port, "GET", "/", &[("X-Forwarded-For", "6.6.6.6")], "").body, "127.0.0.1");
    drop(s);
    let (code, out) = quillo(&d, &["run", "main.qwe", "--policy", "none.toml", "--trust-proxy", "127.0.0.1"], &[]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("trusts no proxy (run.trust_proxy = [])"), "{}", out);
    // An identity header needs a trusted proxy, the policy's counting.
    let (code, out) = quillo(&d, &["run", "main.qwe", "--policy", "none.toml", "--user-header", "x-remote-user"], &[]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("--user-header read a proxy's header"), "{}", out);
}

#[test]
fn allow_path_and_env_set_under_a_policy() {
    let d = temp_dir("f82-run-fs");
    let shared = d.join("shared");
    let other = d.join("other");
    std::fs::create_dir_all(&shared).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(shared.join("x.txt"), "in shared").unwrap();
    std::fs::write(other.join("x.txt"), "in other").unwrap();
    let app = d.join("app");
    std::fs::create_dir_all(&app).unwrap();
    let main = format!("print(read_file(\"{}\") ?? \"refused\")\nprint(env(\"F82_APP_ENV\") ?? \"none\")\n", shared.join("x.txt").display());
    std::fs::write(app.join("main.qwe"), main).unwrap();
    confirm(&d, "app/main.qwe");
    std::fs::write(app.join(".env"), "F82_APP_ENV=from-dotenv\n").unwrap();
    std::fs::write(d.join("p.toml"), format!("[fs]\npaths = [\"{}\"]\n", shared.display())).unwrap();
    std::fs::write(d.join("host.toml"), "[env]\nset = { F82_APP_ENV = \"production\" }\n").unwrap();
    let host = d.join("host.toml").display().to_string();
    let shared_s = shared.display().to_string();
    let other_s = other.display().to_string();
    // Inside the ceiling: read; the forced value beats .env and the shell's.
    let (code, out) = quillo(&d, &["run", "app/main.qwe", "--policy", "p.toml", "--allow-path", &shared_s], &[("F82_APP_ENV", "from-shell"), (policy::PATH_VAR, &host)]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains("in shared\nproduction\n"), "{}", out);
    // Outside: the start stops, naming the file.
    let (code, out) = quillo(&d, &["run", "app/main.qwe", "--policy", "p.toml", "--allow-path", &other_s], &[]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("is not inside the folders p.toml allows"), "{}", out);
    // `quillo mcp` takes --allow-path: held the same.
    let (code, out) = quillo(&d, &["mcp", "app/main.qwe", "--policy", "p.toml", "--allow-path", &other_s], &[]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("is not inside the folders p.toml allows"), "{}", out);
    // Without a policy, .env gives its value as before, and without --allow-path the file is refused.
    let (code, out) = quillo(&d, &["run", "app/main.qwe"], &[]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains("refused\nfrom-dotenv\n"), "{}", out);
}

// ---- Q-1281, Q-1282: the anchor, include, policy.d, QUILLO_POLICY_PATH ---------------------------

#[test]
fn the_anchor_its_includes_and_policy_d_come_in_order() {
    let d = temp_dir("f82-anchor");
    let etc = d.join("etc");
    std::fs::create_dir_all(etc.join("policy.d")).unwrap();
    let plan = d.join("plan.toml");
    std::fs::write(&plan, "[limits]\nworkers = 6\n").unwrap();
    std::fs::write(etc.join("policy.toml"), format!("include = [\"{}\"]\n[limits]\nworkers = 8\n", plan.display())).unwrap();
    std::fs::write(etc.join("policy.d/20-b.toml"), "[limits]\nworkers = 2\n").unwrap();
    std::fs::write(etc.join("policy.d/10-a.toml"), "[limits]\nworkers = 4\n").unwrap();
    std::fs::write(etc.join("policy.d/notes.txt"), "not a policy").unwrap();
    let main = d.join("app/main.qwe");
    let p = policy::load_from(&etc.join("policy.toml"), &main, &[], None).unwrap();
    let kinds: Vec<(String, Kind)> = p.files.iter().map(|f| f.file_name().unwrap().to_string_lossy().into_owned()).zip(p.kinds.iter().copied()).collect();
    assert_eq!(
        kinds,
        [("policy.toml".into(), Kind::Host), ("plan.toml".into(), Kind::Included), ("10-a.toml".into(), Kind::Dropin), ("20-b.toml".into(), Kind::Dropin)]
    );
    assert_eq!((p.workers, p.origin_of("limits.workers").map(|f| f.ends_with("20-b.toml"))), (Some(2), Some(true)));
    // An include that is not there stops it, as a layer asked for.
    std::fs::remove_file(&plan).unwrap();
    let e = policy::load_from(&etc.join("policy.toml"), &main, &[], None).unwrap_err();
    assert!(e.missing && e.message.contains("includes") && e.message.contains("which is not there"), "{:?}", e);
    // An include written as a relative path is refused.
    std::fs::write(etc.join("policy.toml"), "include = [\"plan.toml\"]\n").unwrap();
    assert!(!policy::load_from(&etc.join("policy.toml"), &main, &[], None).unwrap_err().missing);
    // A folder in policy.d named as a policy is a broken layer.
    std::fs::write(etc.join("policy.toml"), "").unwrap();
    std::fs::create_dir_all(etc.join("policy.d/30-dir.toml")).unwrap();
    let e = policy::load_from(&etc.join("policy.toml"), &main, &[], None).unwrap_err();
    assert!(!e.missing && e.message.contains("30-dir.toml: it is a folder, not a policy file"), "{:?}", e);
    // No anchor: nothing from it, and no policy.d beside it.
    let p = policy::load_from(&d.join("nowhere/policy.toml"), &main, &[], None).unwrap();
    assert!(p.files.is_empty() && p.absent.len() == 4, "{:?}", p.absent);
}

#[test]
fn require_path_and_the_variable() {
    let d = temp_dir("f82-require");
    let anchor = d.join("policy.toml");
    std::fs::write(&anchor, "require_path = true\n").unwrap();
    let plan = d.join("plan ü.toml");
    std::fs::write(&plan, "[http]\nhosts = [\"api.example.com\"]\n").unwrap();
    let main = d.join("main.qwe");
    let e = policy::load_from(&anchor, &main, &[], None).unwrap_err();
    assert!(e.missing && e.message.contains("says require_path = true, and QUILLO_POLICY_PATH is not set"), "{:?}", e);
    let e = policy::load_from(&anchor, &main, &[], Some("".into())).unwrap_err();
    assert!(e.missing && e.message.contains("names no file"), "{:?}", e);
    let p = policy::load_from(&anchor, &main, &[], Some(plan.clone().into_os_string())).unwrap();
    assert_eq!(p.kinds, [Kind::Host, Kind::PathVar]);
    assert!(p.http("https://example.com/").is_err());
    // Two files, in order, before --policy; an empty entry is skipped.
    let second = d.join("second.toml");
    std::fs::write(&second, "[limits]\nworkers = 3\n").unwrap();
    let joined = std::env::join_paths([plan.as_path(), Path::new(""), second.as_path()]).unwrap();
    let named = d.join("named.toml");
    std::fs::write(&named, "").unwrap();
    let p = policy::load_from(&anchor, &main, &[named.clone()], Some(joined)).unwrap();
    assert_eq!(p.kinds, [Kind::Host, Kind::PathVar, Kind::PathVar, Kind::Named]);
    assert_eq!(p.files[3], named);
    // A file it names must be there; without require_path, an unset variable is no layer.
    let e = policy::load_from(&anchor, &main, &[], Some(d.join("gone.toml").into_os_string())).unwrap_err();
    assert!(e.missing && e.message.contains("names the policy") && e.message.contains("gone.toml, which is not there"), "{:?}", e);
    std::fs::write(&anchor, "require_path = false\n").unwrap();
    assert_eq!(policy::load_from(&anchor, &main, &[], None).unwrap().kinds, [Kind::Host]);
    // require_path is the anchor's alone: said by a file the variable names, it is refused.
    std::fs::write(&second, "require_path = true\n").unwrap();
    assert!(policy::load_from(&anchor, &main, &[], Some(second.into_os_string())).unwrap_err().message.contains("is said only by the host's file"));
}

#[test]
fn quillo_policy_path_is_read_by_run_and_check() {
    let d = provider("f82-path-var", "[limits]\nworkers = 3\n", "print(\"ran\")\n");
    confirm(&d, "main.qwe");
    std::fs::write(d.join("q.toml"), "[limits]\nmax_body = \"1KB\"\n").unwrap();
    let both = std::env::join_paths([d.join("p.toml"), d.join("q.toml")]).unwrap().into_string().unwrap();
    let (code, out) = quillo(&d, &["run", "main.qwe"], &[(policy::PATH_VAR, &both)]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains("p.toml + ") && out.contains("q.toml (workers 3; max_body 1024 bytes)"), "{}", out);
    // Before --policy.
    std::fs::write(d.join("r.toml"), "").unwrap();
    let (_, out) = quillo(&d, &["run", "main.qwe", "--policy", "r.toml"], &[(policy::PATH_VAR, &both)]);
    assert!(out.contains("q.toml + r.toml"), "{}", out);
    // A file it names that is not there, or a folder: the start stops, exit 2, naming it.
    let gone = d.join("gone.toml").display().to_string();
    for (cmd, f) in [("run", gone.as_str()), ("check", gone.as_str())] {
        let (code, out) = quillo(&d, &[cmd, "main.qwe"], &[(policy::PATH_VAR, f)]);
        assert_eq!(code, Some(2), "{}", out);
        assert!(out.contains("QUILLO_POLICY_PATH names the policy") && out.contains("gone.toml, which is not there") && !out.contains("ran"), "{}", out);
    }
    std::fs::create_dir_all(d.join("dir.toml")).unwrap();
    let (code, out) = quillo(&d, &["run", "main.qwe"], &[(policy::PATH_VAR, &d.join("dir.toml").display().to_string())]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("dir.toml: it is a folder, not a policy file"), "{}", out);
    // Empty: no layer.
    let (code, out) = quillo(&d, &["run", "main.qwe"], &[(policy::PATH_VAR, "")]);
    assert_eq!((code, out.contains("policy")), (Some(0), false), "{}", out);
}

#[test]
fn the_anchor_is_the_systems_place() {
    #[cfg(not(windows))]
    assert_eq!(policy::anchor().unwrap(), Path::new(policy::HOST_FILE));
    #[cfg(windows)]
    assert!(policy::anchor().unwrap().ends_with("quillo\\policy.toml") && !policy::anchor().unwrap().starts_with("\\etc"));
}

// ---- Q-1283: a layer there but broken stops the program ------------------------------------------

#[test]
fn a_broken_program_policy_stops_the_program() {
    let top = temp_dir("f82-broken");
    let d = top.join("release");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("main.qwe"), "print(\"ran\")\n").unwrap();
    // The deploy of the report: a link into a shared folder that is not there.
    std::os::unix::fs::symlink("../shared/quillo.policy", d.join("quillo.policy")).unwrap();
    for cmd in ["run", "check"] {
        let (code, out) = quillo(&d, &[cmd, "main.qwe"], &[]);
        assert_eq!(code, Some(2), "{}: {}", cmd, out);
        assert!(out.contains("quillo.policy: it is a link to ../shared/quillo.policy, which is not there") && !out.contains("ran") && !out.contains(": ok"), "{}: {}", cmd, out);
    }
    // A link to a file there is read.
    let shared = top.join("shared");
    std::fs::create_dir_all(&shared).unwrap();
    std::fs::write(shared.join("quillo.policy"), "[limits]\nworkers = 2\n").unwrap();
    let (code, out) = quillo(&d, &["run", "main.qwe"], &[]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains("quillo.policy (workers 2)") && out.contains("ran"), "{}", out);
    std::fs::remove_file(shared.join("quillo.policy")).unwrap();
    // A folder by that name.
    std::fs::remove_file(d.join("quillo.policy")).unwrap();
    std::fs::create_dir_all(d.join("quillo.policy")).unwrap();
    let (code, out) = quillo(&d, &["run", "main.qwe"], &[]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("quillo.policy: it is a folder, not a policy file"), "{}", out);
    std::fs::remove_dir(d.join("quillo.policy")).unwrap();
    // An empty file is a policy that narrows nothing; none at all is none.
    std::fs::write(d.join("quillo.policy"), "").unwrap();
    let (code, out) = quillo(&d, &["run", "main.qwe"], &[]);
    assert_eq!((code, out.contains("policy quillo.policy\n"), out.contains("ran")), (Some(0), true, true), "{}", out);
    std::fs::remove_file(d.join("quillo.policy")).unwrap();
    let (code, out) = quillo(&d, &["run", "main.qwe"], &[]);
    assert_eq!((code, out.contains("policy")), (Some(0), false), "{}", out);
    // Unreadable (where the tests are not root, who reads anything).
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(d.join("quillo.policy"), "[limits]\nworkers = 2\n").unwrap();
    std::fs::set_permissions(d.join("quillo.policy"), std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read(d.join("quillo.policy")).is_err() {
        let (code, out) = quillo(&d, &["run", "main.qwe"], &[]);
        assert_eq!(code, Some(2), "{}", out);
        assert!(out.contains("cannot read the policy") && out.contains("quillo.policy"), "{}", out);
    }
    std::fs::set_permissions(d.join("quillo.policy"), std::fs::Permissions::from_mode(0o644)).unwrap();
}

// ---- Q-1284: the phone app reads the program's policy --------------------------------------------

#[test]
fn a_phone_app_reads_the_programs_policy() {
    let d = temp_dir("f82-mobile");
    let bundle = d.join("bundle");
    std::fs::create_dir_all(&bundle).unwrap();
    std::fs::write(bundle.join("main.qwe"), "route GET / = \"hi\"\n").unwrap();
    // Broken: the start says so, and nothing is served.
    std::fs::create_dir_all(bundle.join("quillo.policy")).unwrap();
    let e = quillo::mobile::start(&bundle, &d.join("data-broken"), "", "").err().unwrap();
    assert!(e.contains("quillo.policy: it is a folder, not a policy file"), "{}", e);
    std::fs::remove_dir(bundle.join("quillo.policy")).unwrap();
    // `[env] set` is the host's to say, and a phone has no host's layer.
    std::fs::write(bundle.join("quillo.policy"), "[env]\nset = { F82_MOBILE_SET = \"x\" }\n").unwrap();
    let e = quillo::mobile::start(&bundle, &d.join("data-env"), "", "").err().unwrap();
    assert!(e.contains("`env.set` is the host's to say"), "{}", e);
    std::fs::write(bundle.join("quillo.policy"), "[http]\nhosts = [\"api.example.com\"]\n[limits]\nmax_body = \"1KB\"\n").unwrap();
    let s = quillo::mobile::start(&bundle, &d.join("data"), "", "").unwrap();
    assert!(s.port > 0);
    let p = policy::get();
    assert_eq!(p.kinds, [Kind::Program], "the program's alone: a phone has no host's file");
    assert!(p.http("https://example.com/").unwrap_err().contains("not among the hosts"));
    assert_eq!(p.max_body, Some(1024));
}

// ---- Q-1285: a plugin held to its SHA-256 --------------------------------------------------------

const HASH_A: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

#[test]
fn a_plugin_pinned_by_its_hash() {
    let p = layers(&[&format!("[plugins]\nallow = [\"qr@sha256:{}\", \"csv@1\"]\n", HASH_A)]).unwrap();
    assert!(p.pins_by_hash());
    assert!(p.plugin("qr", None, &[HASH_A.to_string()]).is_ok());
    assert!(p.plugin("qr", Some("2.0"), &["other".into(), HASH_A.to_uppercase()]).is_ok(), "the library's, in any case");
    let e = p.plugin("qr", None, &["0".repeat(64)]).unwrap_err();
    assert!(e.contains("is not in the plugins layer0 allows") && e.contains(&format!("(its SHA-256 is {})", "0".repeat(64))), "{}", e);
    assert!(p.plugin("qr", None, &[]).is_err(), "no hash known: refused");
    assert!(p.plugin("evil", None, &[HASH_A.to_string()]).is_err(), "the hash is the named plugin's alone");
    assert!(p.plugin("csv", Some("1.2"), &[]).is_ok(), "a version pin as before");
    assert!(!layers(&["[plugins]\nallow = [\"qr\"]\n"]).unwrap().pins_by_hash());
    for bad in [&HASH_A[1..], &format!("{}0", HASH_A), &format!("{}g", &HASH_A[1..]), ""] {
        assert!(layers(&[&format!("[plugins]\nallow = [\"qr@sha256:{}\"]\n", bad)]).unwrap_err().contains("64 hexadecimal digits"), "{}", bad);
    }
}

#[test]
fn check_holds_a_plugin_to_its_hash() {
    use sha2::Digest;
    let d = temp_dir("f82-plugin-hash");
    let src = "//! version: 1.0.0\n//! effects: none\npub fn hi() -> String { \"hi\".into() }\n";
    std::fs::write(d.join("hi.qwer"), src).unwrap();
    std::fs::write(d.join("main.qwe"), "use \"hi.qwer\"\nprint(hi())\n").unwrap();
    let hash: String = sha2::Sha256::digest(src.as_bytes()).iter().map(|b| format!("{:02x}", b)).collect();
    std::fs::write(d.join("ok.toml"), format!("[plugins]\nallow = [\"hi@sha256:{}\"]\nbuild = false\n", hash)).unwrap();
    let (code, out) = quillo(&d, &["check", "main.qwe", "--policy", "ok.toml"], &[]);
    assert_eq!(code, Some(0), "{}", out);
    // Changed by a byte, or another file renamed to its name: refused before any build.
    std::fs::write(d.join("hi.qwer"), src.replace("\"hi\"", "\"ho\"")).unwrap();
    let (code, out) = quillo(&d, &["check", "main.qwe", "--policy", "ok.toml"], &[]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("the plugin `hi` 1.0.0 is not in the plugins ok.toml allows") && out.contains("(its SHA-256 is "), "{}", out);
    let (code, out) = quillo(&d, &["run", "main.qwe", "--no-check", "--policy", "ok.toml"], &[]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("is not in the plugins ok.toml allows") && !out.contains("quillo: building"), "{}", out);
}

// ---- Q-1286: quillo policy -----------------------------------------------------------------------

#[test]
fn quillo_policy_shows_each_value_and_its_file() {
    let d = provider("f82-policy-cmd", "[http]\nhosts = [\"api.example.com\"]\n[limits]\nworkers = 4\n[run]\ntrust_proxy = [\"10.0.0.0/8\"]\nwith = false\n", "print(\"ran\")\n");
    std::fs::write(d.join("quillo.policy"), "[http]\nhosts = [\"*\"]\n[limits]\nworkers = 2\n").unwrap();
    let (code, out) = quillo(&d, &["policy", "main.qwe", "--policy", "p.toml"], &[]);
    assert_eq!(code, Some(0), "{}", out);
    for line in [
        "the policy for main.qwe",
        "  p.toml (--policy)",
        "  quillo.policy (the program's)",
        "  /etc/quillo/policy.toml (the host's file)",
        "  QUILLO_POLICY_PATH (not set)",
        "  http.hosts: in every one of [api.example.com] (p.toml), [*] (quillo.policy)",
        "  limits.workers = 2 (quillo.policy)",
        "  run.trust_proxy = [10.0.0.0/8] (p.toml)",
        "  run.with = false (--with) (p.toml)",
    ] {
        assert!(out.contains(line), "{:?} in:\n{}", line, out);
    }
    assert!(!out.contains("ran"), "the program is not run: {}", out);
    // A folder, or none: its main.qwe's, which need not be there.
    let (code, out) = quillo(&d, &["policy"], &[]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains("./main.qwe") && out.contains("limits.workers = 2"), "{}", out);
    let empty = temp_dir("f82-policy-none");
    let (code, out) = quillo(&empty, &["policy"], &[]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains("no policy: nothing narrows what the program may do"), "{}", out);
    // A layer asked for and not there: 1; one there but broken: 2.
    let (code, out) = quillo(&d, &["policy", "main.qwe"], &[(policy::PATH_VAR, "/nowhere/ü.toml")]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("QUILLO_POLICY_PATH names the policy /nowhere/ü.toml, which is not there"), "{}", out);
    std::fs::write(d.join("bad.toml"), "[run]\nport = 1\n").unwrap();
    let (code, out) = quillo(&d, &["policy", "--policy", "bad.toml"], &[]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("bad.toml:2: `run.port` is not something a policy says"), "{}", out);
    // Its own options.
    let (code, out) = quillo(&d, &["policy", "--polcy", "p.toml"], &[]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("unknown option `--polcy` for `policy` (did you mean `--policy`?)"), "{}", out);
    let (code, _) = quillo(&d, &["policy", "--policy"], &[]);
    assert_eq!(code, Some(2));
    let (code, out) = quillo(&d, &["policy", "a.qwe", "b.qwe"], &[]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("`policy` takes one program"), "{}", out);
}

// ---- Found in review of slice P ------------------------------------------------------------------

#[test]
fn env_set_is_the_hosts_alone_and_never_the_policys_own_names() {
    // A developer's layer (`--policy`, quillo.policy) may not say it: it narrows nothing.
    for kind in [Kind::Named, Kind::Program] {
        let mut p = Policy::default();
        let e = p.add_layer(Path::new("dev.toml"), "[env]\nset = { PATH = \"/evil\" }\n", kind).unwrap_err();
        assert!(e.contains("dev.toml:2: `env.set` is the host's to say"), "{}", e);
        assert!(Policy::default().add_layer(Path::new("dev.toml"), "[env.set]\nA = \"x\"\n", kind).is_err());
    }
    for kind in [Kind::Host, Kind::Included, Kind::Dropin, Kind::PathVar] {
        assert!(Policy::default().add_layer(Path::new("h.toml"), "[env]\nset = { APP_ENV = \"x\" }\n", kind).is_ok(), "{:?}", kind);
    }
    // Not even the host's may set what chooses the policy or what a process loads.
    for name in ["QUILLO_POLICY_PATH", "quillo_policy_path", "LD_PRELOAD", "LD_LIBRARY_PATH", "DYLD_INSERT_LIBRARIES"] {
        let e = layers(&[&format!("[env]\nset = {{ {} = \"/app/empty.toml\" }}\n", name)]).unwrap_err();
        assert!(e.contains("may not set"), "{}: {}", name, e);
    }
    assert!(layers(&["[env]\nset = { QUILLO_SECRET_X = \"x\" }\n"]).is_ok(), "the one before the limit");
    // Run: a program's own quillo.policy saying it stops the start; the variable it would set is
    // not changed.
    let d = provider("f82-env-dev", "", "print(env(\"QUILLO_POLICY_PATH\") ?? \"unset\")\n");
    std::fs::write(d.join("quillo.policy"), "[env]\nset = { QUILLO_POLICY_PATH = \"/app/empty.toml\" }\n").unwrap();
    let (code, out) = quillo(&d, &["run", "main.qwe"], &[]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("`env.set` is the host's to say"), "{}", out);
    std::fs::write(d.join("quillo.policy"), "").unwrap();
    std::fs::write(d.join("p.toml"), "[env]\nset = { F82_X = \"y\" }\n").unwrap();
    let (code, out) = quillo(&d, &["run", "main.qwe", "--policy", "p.toml"], &[]);
    assert_eq!(code, Some(2), "{}", out);
}

#[test]
fn quillo_policy_names_what_env_set_sets_and_not_its_value() {
    let d = provider("f82-env-shown", "[env]\nset = { F82_TOKEN = \"s3cret\" }\n", "print(\"ran\")\n");
    let host = d.join("p.toml").display().to_string();
    let (code, out) = quillo(&d, &["policy"], &[(policy::PATH_VAR, &host)]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains("env.set.F82_TOKEN (set) (") && !out.contains("s3cret"), "{}", out);
}

/// A plugin with a library planted where `build` would look for it, and a lock line to match.
fn planted(name: &str) -> (PathBuf, String, String) {
    use sha2::Digest;
    let hex = |b: &[u8]| sha2::Sha256::digest(b).iter().map(|b| format!("{:02x}", b)).collect::<String>();
    let d = temp_dir(name);
    let src = "//! version: 1.0.0\n//! effects: none\npub fn hi() -> String { \"hi\".into() }\n";
    std::fs::write(d.join("hi.qwer"), src).unwrap();
    std::fs::write(d.join("main.qwe"), "use \"hi.qwer\"\nprint(hi())\n").unwrap();
    let lib = quillo::native::library_path(&d.join("hi.qwer")).unwrap();
    std::fs::create_dir_all(lib.parent().unwrap()).unwrap();
    let fake = b"not the library built from hi.qwer";
    std::fs::write(&lib, fake).unwrap();
    std::fs::write(lib.parent().unwrap().parent().unwrap().join("plugins.lock"), format!("{} {}\n", lib.file_name().unwrap().to_string_lossy(), hex(fake))).unwrap();
    (d, hex(src.as_bytes()), hex(fake))
}

#[test]
fn a_source_pin_never_loads_a_prebuilt_library() {
    let (d, source, library) = planted("f82-planted");
    std::fs::write(d.join("src.toml"), format!("[plugins]\nallow = [\"hi@sha256:{}\"]\nbuild = false\n", source)).unwrap();
    let (code, out) = quillo(&d, &["run", "main.qwe", "--no-check", "--policy", "src.toml"], &[]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("so only a library built here from that source may load") && !out.contains("cannot load"), "{}", out);
    // Pinned by the library's own hash, the prebuilt one is the one allowed: it is the one opened
    // (and, being no library, is refused as one before the system's loader is given it: since
    // fcac7a3 what a held library loads is judged first, `native::loads_refused`).
    std::fs::write(d.join("lib.toml"), format!("[plugins]\nallow = [\"hi@sha256:{}\"]\nbuild = false\n", library)).unwrap();
    let (code, out) = quillo(&d, &["run", "main.qwe", "--no-check", "--policy", "lib.toml"], &[]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("is not a library this system loads: it does not load") && !out.contains("not in the plugins"), "{}", out);
    // One layer by the source, another by the library: refused, nothing loaded.
    std::fs::write(d.join("only-src.toml"), format!("[plugins]\nallow = [\"hi@sha256:{}\"]\n", source)).unwrap();
    let (code, out) = quillo(&d, &["run", "main.qwe", "--no-check", "--policy", "only-src.toml", "--policy", "lib.toml"], &[]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("pins the plugin by its source's SHA-256 and another by its library's") && !out.contains("cannot load"), "{}", out);
}

#[test]
fn check_true_holds_under_watch_and_split() {
    let d = provider("f82-check-watch", "[run]\ncheck = true\n", "route GET / = nope()\n");
    // Before, --watch served the error page and kept running.
    let (code, out) = quillo(&d, &["run", "main.qwe", "--policy", "p.toml", "--watch", "--port", "0"], &[]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("unknown name `nope`"), "{}", out);
    // A forced base and --split: refused before any service starts.
    std::fs::write(d.join("b.toml"), "[run]\nbase = \"/shop\"\n").unwrap();
    std::fs::write(d.join("ok.qwe"), "print(\"ran\")\n").unwrap();
    let (code, out) = quillo(&d, &["run", "ok.qwe", "--policy", "b.toml", "--split"], &[]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("--split does not go with the base the policy b.toml forces"), "{}", out);
}

// ---- Found in the second review of slice P -------------------------------------------------------

#[test]
fn a_dotenv_never_names_the_policy() {
    let d = provider("f82-dotenv-policy", "[env]\nset = { FOO = \"host\" }\n", "print(env(\"QUILLO_POLICY_PATH\") ?? \"unset\")\nprint(env(\"CARGO_TARGET_DIR\") ?? \"no cargo dir\")\n");
    std::fs::write(d.join(".env"), format!("QUILLO_POLICY_PATH={}\nCARGO_TARGET_DIR=t\n", d.join("p.toml").display())).unwrap();
    // No host's policy: the name is left out, said; the rest of .env as before.
    let (code, out) = quillo(&d, &["run", "main.qwe"], &[]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains(".env sets QUILLO_POLICY_PATH, which only whoever starts Quillo may set: not read") && out.contains("unset\nt\n"), "{}", out);
    // A process whose environment says a file gave it reads it all the same: no .env sets it, and
    // the variable that says so is anyone's to set, which dropped the provider's layer
    // (FEEDBACK82.md, Q-1281, found by the coverage tests; branch feedback82-85).
    let (code, out) = quillo(&d, &["run", "main.qwe"], &[(policy::PATH_VAR, "/nowhere/x.toml"), ("QUILLO_ENV_FROM_FILE", "QUILLO_POLICY_PATH")]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("QUILLO_POLICY_PATH names the policy /nowhere/x.toml, which is not there") && !out.contains("came from a .env file"), "{}", out);
}

#[test]
fn under_a_hosts_policy_a_dotenv_steers_nothing() {
    let d = provider("f82-dotenv-steer", "[limits]\nworkers = 4\n", "print(\"ran\")\n");
    confirm(&d, "main.qwe");
    let host = d.join("p.toml").display().to_string();
    for name in ["LD_PRELOAD", "DYLD_INSERT_LIBRARIES", "CARGO", "RUSTC_WRAPPER", "RUSTFLAGS", "QUILLO_NATIVE_CACHE", "XDG_CACHE_HOME", "QUILLO_STOP_WITH_APP", "QUILLO_HOST_X", "RUSTUP_DIST_SERVER", "QUILLO_ENV_FROM_FILE"] {
        std::fs::write(d.join(".env"), format!("{}=/tmp/x\n", name)).unwrap();
        let (code, out) = quillo(&d, &["run", "main.qwe"], &[(policy::PATH_VAR, &host)]);
        assert_eq!(code, Some(2), "{}: {}", name, out);
        assert!(out.contains(&format!(".env sets {}, which steers Quillo", name)) && !out.contains("ran"), "{}: {}", name, out);
    }
    // What the program reads is its own, as before; and one the environment set already is left.
    std::fs::write(d.join(".env"), "APP_NAME=shop\nCARGO=/tmp/x\n").unwrap();
    let (code, out) = quillo(&d, &["run", "main.qwe"], &[(policy::PATH_VAR, &host), ("CARGO", "/usr/bin/false")]);
    assert_eq!((code, out.contains("ran")), (Some(0), true), "{}", out);
    assert!(quillo::dotenv::steers("ld_preload") && !quillo::dotenv::steers("APP_NAME") && !quillo::dotenv::steers("QUILLO_SECRET"));
}

#[test]
fn quillo_build_and_the_editors_check_honour_build_false() {
    let d = temp_dir("f82-build-false");
    std::fs::write(d.join("hi.qwer"), "//! effects: none\npub fn hi() -> String { \"hi\".into() }\n").unwrap();
    std::fs::write(d.join("main.qwe"), "use \"hi.qwer\"\nprint(hi())\n").unwrap();
    std::fs::write(d.join("quillo.policy"), "[plugins]\nbuild = false\n").unwrap();
    let (code, out) = quillo(&d, &["build", "main.qwe"], &[]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("no build here: the policy quillo.policy says build = false") && !out.contains("building"), "{}", out);
    let e = quillo::native::check(&d.join("hi.qwer")).unwrap_err();
    assert!(e.contains("no cargo check here") && e.contains("build = false"), "{}", e);
}

#[test]
fn a_pinned_source_is_built_apart_with_no_cargo_of_the_developers() {
    use sha2::Digest;
    let d = temp_dir("f82-pinned-build");
    let src = "//! version: 1.0.0\n//! effects: none\npub fn hi() -> String { \"hi from the pinned source\".into() }\n";
    std::fs::write(d.join("hi.qwer"), src).unwrap();
    std::fs::write(d.join("main.qwe"), "use \"hi.qwer\"\nprint(hi())\n").unwrap();
    let hash: String = sha2::Sha256::digest(src.as_bytes()).iter().map(|b| format!("{:02x}", b)).collect();
    std::fs::write(d.join("pin.toml"), format!("[plugins]\nallow = [\"hi@sha256:{}\"]\n", hash)).unwrap();
    // A cargo and a rustc wrapper of the developer's, which leave a mark if they run.
    let mark = d.join("ran-by-the-developers-cargo");
    let fake = d.join("fake.sh");
    std::fs::write(&fake, format!("#!/bin/sh\ntouch {}\nexit 1\n", mark.display())).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let f = fake.display().to_string();
    let cache = d.join("cache").display().to_string();
    let (code, out) = quillo(&d, &["run", "main.qwe", "--no-check", "--policy", "pin.toml", "--", "slow"], &[("CARGO", &f), ("RUSTC_WRAPPER", &f), ("RUSTC", &f), ("QUILLO_NATIVE_CACHE", &cache)]);
    // `slow` is the program's own argument; only the deadline reads it.
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains("hi from the pinned source"), "{}", out);
    assert!(out.contains("held to its SHA-256: from its source, apart"), "{}", out);
    assert!(!mark.exists(), "the developer's cargo ran: {}", out);
    assert!(!d.join(".quillo/native").exists() && !d.join("cache/target").exists(), "built where the developer's folders are: {}", out);
}

// ---- Found in the third review of slice P --------------------------------------------------------

/// A plugin pinned by its source's SHA-256, a program that uses it, and the pin's policy.
fn pinned(name: &str) -> PathBuf {
    use sha2::Digest;
    let d = temp_dir(name);
    let src = "//! version: 1.0.0\n//! effects: none\npub fn hi() -> String { \"hi from the pinned source\".into() }\n";
    std::fs::write(d.join("hi.qwer"), src).unwrap();
    std::fs::write(d.join("main.qwe"), "use \"hi.qwer\"\nprint(hi())\n").unwrap();
    let hash: String = sha2::Sha256::digest(src.as_bytes()).iter().map(|b| format!("{:02x}", b)).collect();
    std::fs::write(d.join("pin.toml"), format!("[plugins]\nallow = [\"hi@sha256:{}\"]\n", hash)).unwrap();
    d
}

fn pinned_dirs(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir).map(|r| r.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with("quillo-pinned-")).collect()).unwrap_or_default()
}

#[test]
fn a_pinned_build_takes_no_home_nor_temp_folder_from_dotenv() {
    let d = pinned("f82-pinned-home");
    std::fs::create_dir_all(d.join("h")).unwrap();
    std::fs::write(d.join(".env"), format!("HOME={}\n", d.join("h").display())).unwrap();
    // HOME left unset by whoever started it, and given by .env: the pinned build is refused.
    let (code, out) = quillo_without(&d, &["run", "main.qwe", "--no-check", "--policy", "pin.toml", "--", "slow"], &[], &["HOME", "CARGO_HOME"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("needs HOME (or CARGO_HOME) from whoever starts Quillo, not from .env"), "{}", out);
    // Under a host's layer, .env may not set it at all.
    std::fs::write(d.join("host.toml"), "").unwrap();
    let host = d.join("host.toml").display().to_string();
    for name in ["HOME", "TMPDIR", "PATH", "USER"] {
        std::fs::write(d.join(".env"), format!("{}=/tmp/x\n", name)).unwrap();
        let (code, out) = quillo_without(&d, &["run", "main.qwe", "--no-check"], &[(policy::PATH_VAR, &host)], &[name]);
        assert_eq!(code, Some(2), "{}: {}", name, out);
        assert!(out.contains(&format!(".env sets {}, which steers Quillo", name)), "{}: {}", name, out);
    }
    // TMPDIR from .env, inside the program's folder: the build is not made there.
    std::fs::create_dir_all(d.join("tmp")).unwrap();
    std::fs::write(d.join(".env"), format!("TMPDIR={}\n", d.join("tmp").display())).unwrap();
    let (code, out) = quillo_without(&d, &["run", "main.qwe", "--no-check", "--policy", "pin.toml", "--", "slow"], &[], &["TMPDIR"]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains("hi from the pinned source") && pinned_dirs(&d.join("tmp")).is_empty(), "{}", out);
}

#[test]
fn a_pinned_builds_folder_goes_once_it_is_loaded() {
    let d = pinned("f82-pinned-gone");
    let t = temp_dir("f82-pinned-gone-tmp");
    let (code, out) = quillo(&d, &["run", "main.qwe", "--no-check", "--policy", "pin.toml", "--", "slow"], &[("TMPDIR", &t.display().to_string())]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains("hi from the pinned source"), "{}", out);
    assert!(pinned_dirs(&t).is_empty(), "left in the temporary folder: {:?}", pinned_dirs(&t));
}

#[test]
fn a_dotenv_policy_path_does_not_reach_a_split_service() {
    // The attack of the second review: .env names a file of the developer's, which a --split service
    // read as a host's layer, and its [env] set replaced the provider's FOO.
    let d = temp_dir("f82-split-attack");
    std::fs::write(d.join("evil.toml"), "[env]\nset = { FOO = \"planted\" }\n").unwrap();
    std::fs::write(d.join(".env"), format!("QUILLO_POLICY_PATH={}\n", d.join("evil.toml").display())).unwrap();
    std::fs::write(d.join("work.qwe"), "server fn foo() -> str = env(\"FOO\") ?? \"none\"\n").unwrap();
    std::fs::write(d.join("main.qwe"), "service work = \"work.qwe\"\nroute GET /foo = work.foo()\n").unwrap();
    let port = support::free_port();
    let log = std::fs::File::create(d.join("split.log")).unwrap();
    let child = std::process::Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", "main.qwe", "--port", &port.to_string(), "--split"])
        .current_dir(&d)
        .env_remove(policy::PATH_VAR)
        .env("FOO", "provider")
        .env("QUILLO_SECRET", "a secret for the slice P tests, long enough")
        .env("QUILLO_SERVICE_SECRET", "another secret for the slice P tests, long enough")
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap();
    let mut g = support::Guard::new(child);
    let deadline = Instant::now() + Duration::from_secs(30);
    let body = loop {
        if support::listening(port) {
            let b = send(port, "GET", "/foo", &[], "").body;
            if b == "provider" || b == "planted" {
                break b;
            }
        }
        assert!(Instant::now() < deadline, "{}", std::fs::read_to_string(d.join("split.log")).unwrap_or_default());
        std::thread::sleep(Duration::from_millis(100));
    };
    // Stopped as a person stops it: the app stops its service too.
    let _ = std::process::Command::new("kill").args(["-INT", &g.0.id().to_string()]).status();
    let until = Instant::now() + Duration::from_secs(10);
    while g.0.try_wait().ok().flatten().is_none() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
    let said = std::fs::read_to_string(d.join("split.log")).unwrap_or_default();
    assert_eq!(body, "provider", "{}", said);
    assert!(said.contains(".env sets QUILLO_POLICY_PATH, which only whoever starts Quillo may set: not read"), "{}", said);
}
