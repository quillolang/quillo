//! `quillo app` before its window (src/desktop.rs): a folder for its secret that cannot be made,
//! and an interrupt while the program loads. No run here gets as far as a window: each is
//! stopped, or stops, before one opens.

mod support;

#[cfg(feature = "app")]
mod with_windows {
    use super::support;
    use std::path::Path;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    fn folder(name: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
        let d = support::temp_dir(&format!("cover-desktop-{}", name));
        for (file, text) in files {
            std::fs::write(d.join(file), text).unwrap();
        }
        d
    }

    fn start(dir: &Path, args: &[&str]) -> support::Guard {
        let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
            .arg("app")
            .args(args)
            .current_dir(dir)
            .env("HOME", dir)
            .stdin(Stdio::null())
            .stdout(std::fs::File::create(dir.join("out.log")).unwrap())
            .stderr(std::fs::File::create(dir.join("err.log")).unwrap())
            .spawn()
            .unwrap();
        support::Guard::new(child)
    }

    /// Its exit code, waiting at most 20 s: a run that gets further is at a window, and fails.
    fn ended(g: &mut support::Guard) -> i32 {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(s) = g.0.try_wait().unwrap() {
                return s.code().unwrap_or(-1);
            }
            assert!(Instant::now() < deadline, "`quillo app` got as far as a window");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn a_folder_for_the_apps_secret_that_cannot_be_made_is_said_before_the_program_runs() {
        // `.quillo` beside the program is a file: the folder for the secret and the cookies
        // cannot be made there.
        let d = folder("secret", &[("app.qwe", "print(\"loaded\")\nroute GET / = \"hi\"\n"), (".quillo", "a file\n")]);
        let mut g = start(&d, &["app.qwe"]);
        assert_eq!(ended(&mut g), 1);
        let err = std::fs::read_to_string(d.join("err.log")).unwrap();
        assert!(err.starts_with("quillo: .quillo: "), "{}", err);
        assert_eq!(std::fs::read_to_string(d.join("out.log")).unwrap(), "", "the program did not run");
    }

    #[test]
    fn an_interrupt_while_the_program_loads_stops_it_and_the_commands_beside_it() {
        let d = folder("interrupt", &[("app.qwe", "sleep(20000)\nroute GET / = \"hi\"\n")]);
        let mut g = start(&d, &["app.qwe", "--no-check", "--with", "echo $$ > side.pid; exec sleep 30"]);
        let deadline = Instant::now() + Duration::from_secs(20);
        while !d.join("side.pid").exists() || std::fs::read_to_string(d.join("side.pid")).unwrap().trim().is_empty() {
            assert!(Instant::now() < deadline, "the command did not start");
            assert!(g.0.try_wait().unwrap().is_none());
            std::thread::sleep(Duration::from_millis(20));
        }
        // Loading, sleeping: no window yet.
        std::thread::sleep(Duration::from_millis(300));
        let _ = Command::new("kill").args(["-INT", &g.0.id().to_string()]).status();
        assert_eq!(ended(&mut g), 130);
        let pid = std::fs::read_to_string(d.join("side.pid")).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while Command::new("kill").args(["-0", pid.trim()]).stderr(Stdio::null()).status().unwrap().success() {
            assert!(Instant::now() < deadline, "the command stops with the program");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// Links the library, as every test binary must for the coverage run.
#[test]
fn the_windows_shell_script_says_how_bodies_go() {
    assert!(quillo::embed::shell_js("window.ipc.postMessage", "bytes").contains("const bodies = \"bytes\""));
}
