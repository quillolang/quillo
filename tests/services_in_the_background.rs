//! What runs with a service but outside any request: its jobs and ticks in
//! the app's process, a call made later in a tenant, and copies under `--scale`.

mod support;

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn dir(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = support::temp_dir(&format!("background-{}", name));
    for (f, text) in files {
        std::fs::write(d.join(f), text).unwrap();
    }
    d
}

fn eventually<T>(secs: u64, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < deadline, "{}", what);
        std::thread::sleep(Duration::from_millis(30));
    }
}

/// A running server, stopped as Ctrl-C does: under `--split` that stops its services too.
struct Run {
    child: Child,
    port: u16,
    log: PathBuf,
}

impl Drop for Run {
    fn drop(&mut self) {
        let _ = Command::new("kill").args(["-INT", &self.child.id().to_string()]).status();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Run {
    fn get(&self, path: &str) -> String {
        support::send(self.port, "GET", path, &[], "").body
    }

    fn log(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
}

fn run(file: &Path, extra: &[&str]) -> Run {
    let port = support::free_port();
    let log = file.with_extension("log");
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(file)
        .args(["--port", &port.to_string()])
        .args(extra)
        .env("QUILLO_SECRET", "a test secret for the background, long enough")
        .env_remove("QUILLO_SERVICE_SECRET")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    support::stop_with_tests(&child);
    let mut r = Run { child, port, log };
    let deadline = Instant::now() + Duration::from_secs(25);
    while !support::listening(port) {
        if let Ok(Some(status)) = r.child.try_wait() {
            panic!("quillo stopped ({}):\n{}", status, r.log());
        }
        assert!(Instant::now() < deadline, "the server did not start:\n{}", r.log());
        std::thread::sleep(Duration::from_millis(20));
    }
    // A server may drop a first connection as it starts.
    eventually(10, "the server did not answer", || (support::send(port, "GET", "/", &[], "").status != 0).then_some(()));
    r
}

#[test]
fn a_services_jobs_and_ticks_run_in_the_apps_process() {
    let work = "let beats = 0\nlet sweeps = 0\nfn beat() { beats = beats + 1 }\nfn sweep() { sweeps = sweeps + 1 }\nevery(50, beat)\nschedule(1, sweep)\nserver fn counts() = { beats, sweeps }\n";
    let d = dir("jobs", &[("work.qwe", work), ("front.qwe", "service work = \"work.qwe\"\nroute GET /counts = work.counts()\n")]);
    let r = run(&d.join("front.qwe"), &[]);
    eventually(20, "the service's tick and job did not run", || {
        let got: serde_json::Value = serde_json::from_str(&r.get("/counts")).ok()?;
        (got["beats"].as_i64()? >= 2 && got["sweeps"].as_i64()? >= 1).then_some(())
    });
}

#[test]
fn a_call_made_later_in_a_tenant_is_made_in_that_tenant() {
    let mailer = "let sent = []\nserver fn send(to: str) -> int {\n    sent = sent + [tenant() + \":\" + to]\n    len(sent)\n}\nserver fn outbox() = sent | join(\",\")\n";
    let front = "service mailer = \"mailer.qwe\"\nroute GET /send/:to = with_tenant(\"acme\", () => mailer.later.send(to))\nroute GET /outbox = mailer.outbox()\n";
    let d = dir("later", &[("mailer.qwe", mailer), ("front.qwe", front)]);
    let r = run(&d.join("front.qwe"), &[]);
    let id = r.get("/send/ana");
    assert!(id.len() == 24 && id.bytes().all(|b| b.is_ascii_hexdigit()), "{}\n{}", id, r.log());
    eventually(15, "the call made later was not made", || (r.get("/outbox") == "acme:ana").then_some(()));
}

#[test]
fn scale_runs_a_service_as_that_many_copies_taking_calls_in_turn() {
    let work = "let me = crypto.token(8)\nserver fn who() = me\n";
    let d = dir("scale", &[("work.qwe", work), ("front.qwe", "service work = \"work.qwe\"\nroute GET /who = work.who()\n")]);
    let declared = quillo::service::declared(&d.join("front.qwe")).unwrap();
    assert_eq!(declared.iter().map(|(name, ..)| name.as_str()).collect::<Vec<_>>(), ["work"]);
    let r = run(&d.join("front.qwe"), &["--split", "--scale", "work=2"]);
    let mut seen = std::collections::HashSet::new();
    eventually(25, "the two copies did not both answer", || {
        let who = support::send(r.port, "GET", "/who", &[], "");
        if who.status == 200 {
            seen.insert(who.body);
        }
        (seen.len() == 2).then_some(())
    });
}
