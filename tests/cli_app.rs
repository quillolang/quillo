//! `quillo app`: all it does before there is a window. Each run here must stop before the
//! window opens, so a program that would be served has its port taken; a test can't see a
//! window, and `no_window` kills any run that gets that far.

mod support;

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Runs `quillo app` and gives its status and what it said on stdout and stderr.
fn no_window(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> (i32, String, String) {
    let (out, err) = (dir.join("out.log"), dir.join("err.log"));
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("app")
        .args(args)
        .current_dir(dir)
        .env("HOME", dir)
        .envs(env.iter().copied())
        .stdin(Stdio::null())
        .stdout(std::fs::File::create(&out).unwrap())
        .stderr(std::fs::File::create(&err).unwrap())
        .spawn()
        .unwrap();
    let mut child = support::Guard::new(child);
    let deadline = Instant::now() + Duration::from_secs(20);
    let status = loop {
        if let Some(s) = child.0.try_wait().unwrap() {
            break s;
        }
        assert!(Instant::now() < deadline, "`quillo app {:?}` got as far as a window", args);
        std::thread::sleep(Duration::from_millis(10));
    };
    (status.code().unwrap(), std::fs::read_to_string(&out).unwrap(), std::fs::read_to_string(&err).unwrap())
}

fn folder(name: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
    let d = support::temp_dir(&format!("cli-app-{}", name));
    for (file, text) in files {
        std::fs::write(d.join(file), text).unwrap();
    }
    d
}

#[cfg(not(feature = "app"))]
#[test]
fn a_build_without_windows_says_how_to_get_one() {
    let d = folder("nowindows", &[("app.qwe", "route GET / = \"hi\"\n")]);
    let (code, _, err) = no_window(&d, &["app.qwe"], &[]);
    assert_eq!(code, 2);
    assert_eq!(err, "quillo: this quillo was built without windows; build it with `cargo build --release --features app`\n");
}

#[cfg(feature = "app")]
mod with_windows {
    use super::*;

    const APP: &str = "route GET / = \"hi\"\n";

    #[test]
    fn open_takes_a_path_of_the_program() {
        let d = folder("open", &[("app.qwe", APP)]);
        for bad in [&["app.qwe", "--open"][..], &["app.qwe", "--open", "admin"], &["app.qwe", "--open", "//evil.example/"], &["app.qwe", "--open", "https://evil.example/"]] {
            let (code, _, err) = no_window(&d, bad, &[]);
            assert_eq!(code, 2, "{:?}", bad);
            assert_eq!(err, "quillo: --open takes a path of the program, such as /admin\n", "{:?}", bad);
        }
    }

    #[test]
    fn it_takes_the_options_of_run_and_refuses_what_is_only_for_a_server() {
        let d = folder("options", &[("app.qwe", APP)]);
        let said = "quillo: `app` serves its own window only; --host, --register and --service are for `run`\n";
        for bad in [&["app.qwe", "--host", "0.0.0.0"][..], &["app.qwe", "--register", "http://127.0.0.1:9"], &["app.qwe", "--service", "pool"]] {
            let (code, _, err) = no_window(&d, bad, &[]);
            assert_eq!((code, err.as_str()), (2, said), "{:?}", bad);
        }
        let (code, _, err) = no_window(&d, &["app.qwe", "--prot", "1"], &[]);
        assert_eq!(code, 2);
        assert_eq!(err, "quillo: unknown option `--prot` (did you mean `--port`?)\n");
    }

    #[test]
    fn help_is_run_s_and_opens_nothing() {
        let d = folder("help", &[("app.qwe", APP)]);
        let (code, out, _) = no_window(&d, &["app.qwe", "--help"], &[]);
        assert_eq!(code, 0);
        assert!(out.contains("usage:\n  quillo run <file.qwe> [options]"), "{}", out);
    }

    #[test]
    fn the_program_is_found_as_for_run() {
        let d = folder("program", &[]);
        let (code, _, err) = no_window(&d, &[], &[]);
        assert_eq!((code, err.as_str()), (2, "quillo: no program: name a .qwe file, or add a main.qwe here\n"));
    }

    #[test]
    fn a_program_with_nothing_to_show_says_so() {
        let d = folder("noroutes", &[("app.qwe", "print(\"ran\")\n")]);
        let (code, out, err) = no_window(&d, &["app.qwe"], &[]);
        assert_eq!(code, 1);
        assert_eq!(out, "ran\n");
        assert_eq!(err, "quillo: app.qwe declares no routes, so there is nothing to show in a window\n");
    }

    #[test]
    fn the_env_file_beside_the_program_is_read_as_for_run() {
        let d = folder("dotenv", &[("app.qwe", "print(env(\"GREETING\"))\n"), (".env", "GREETING=hello\n")]);
        let (_, out, _) = no_window(&d, &["app.qwe"], &[]);
        assert_eq!(out, "hello\n");
        let (code, _, err) = no_window(&d, &["app.qwe", "--env-file", "nope.env"], &[]);
        assert_eq!(code, 2, "{}", err);
    }

    #[test]
    fn a_command_beside_the_program_runs_in_its_folder_and_stops_with_it() {
        let d = folder("with", &[("app.qwe", "sleep(1000)\nlet x = 1 / 0\n")]);
        let (code, _, err) = no_window(&d, &["app.qwe", "--no-check", "--with", "echo $$ > side.pid; exec sleep 30"], &[]);
        assert_eq!(code, 1, "{}", err);
        let pid = std::fs::read_to_string(d.join("side.pid")).expect("the command did not start");
        let alive = std::process::Command::new("kill").args(["-0", pid.trim()]).stderr(std::process::Stdio::null()).status().unwrap().success();
        assert!(!alive, "the command stops with the program");
    }

    #[test]
    fn a_program_with_errors_opens_nothing() {
        let d = folder("errors", &[("app.qwe", "route GET / = nope\n")]);
        let (code, _, err) = no_window(&d, &["app.qwe"], &[]);
        assert_eq!(code, 1);
        assert!(err.contains("app.qwe:1: error: unknown name `nope`"), "{}", err);
        let (code, _, err) = no_window(&d, &["gone.qwe"], &[]);
        assert_eq!(code, 1);
        assert!(err.contains("gone.qwe"), "{}", err);
    }

    #[test]
    fn a_port_already_taken_stops_it_before_the_window() {
        let d = folder("taken", &[("app.qwe", "print(\"loaded\")\nroute GET / = \"hi\"\n")]);
        let held = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = held.local_addr().unwrap().port().to_string();
        let (code, out, err) = no_window(&d, &["app.qwe", "--port", &port, "--open", "/admin?x=1", "--workers", "2"], &[("QUILLO_SECRET", "short")]);
        assert_eq!(code, 1, "{}", err);
        assert_eq!(out, "loaded\n", "the program ran first");
        assert!(err.contains("QUILLO_SECRET is 5 bytes"), "{}", err);
        assert!(err.contains(&format!("quillo: cannot listen on 127.0.0.1:{}", port)), "{}", err);
        drop(held);
    }
}
