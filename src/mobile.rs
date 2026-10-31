//! A program inside a phone app (branch mobile).
//!
//! The app's launcher (Swift on iOS, Kotlin on Android: see mobile/) calls
//! `start` with the folder the program was bundled in, read-only, and a folder
//! the app may write to. The program is copied into `<data>/app` and run from
//! there, so `db.open("app.db")`, uploads and `write_file` land in the app's
//! own container, as they would beside a program on a server. It is served on
//! 127.0.0.1 at a port the system picks, to the app's web view only: the URL
//! `start` gives carries a token that the server answers once, with a cookie,
//! as `quillo app` does on a desktop (`server::launch_gate`). Other apps on the
//! phone reaching the port get 403.
//!
//! What a phone build leaves out: compiled numbers (no JIT on iOS), channels
//! over WebTransport (pages use the WebSocket), `.qwer` plugins built at run
//! time (no compiler on a phone), and everything that starts a process
//! (`--instances`, `--split`, `--with`).

use crate::interp::Interp;
use crate::server::{App, Loaded};
use std::path::{Path, PathBuf};

/// What the launcher opens: the address of the program's first page, with the
/// token that lets the web view in.
pub struct Started {
    pub port: u16,
    pub url: String,
}

/// Files that hold what the program wrote: copied from the bundle when missing
/// (a database the program ships with), never over what the phone has.
fn is_data(p: &Path) -> bool {
    let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    [".db", ".sqlite", ".sqlite3", ".db-wal", ".db-shm", "-wal", "-shm", "-journal"].iter().any(|x| name.ends_with(x))
}

/// Copies the bundle into `to`: the program's files follow the bundle (an app
/// update brings new code), what the program wrote stays. Nothing in `to` is
/// deleted. Returns how many files were copied.
pub fn sync(from: &Path, to: &Path) -> Result<usize, String> {
    let mut copied = 0;
    let mut todo: Vec<PathBuf> = vec![PathBuf::new()];
    while let Some(rel) = todo.pop() {
        let dir = from.join(&rel);
        std::fs::create_dir_all(to.join(&rel)).map_err(|e| format!("{}: {}", to.join(&rel).display(), e))?;
        let entries = std::fs::read_dir(&dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
        for entry in entries.flatten() {
            let name = entry.file_name();
            let r = rel.join(&name);
            let src = from.join(&r);
            let dst = to.join(&r);
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            if kind.is_dir() {
                todo.push(r);
                continue;
            }
            if is_data(&src) && dst.exists() {
                continue;
            }
            let same = match (std::fs::metadata(&src), std::fs::metadata(&dst)) {
                (Ok(a), Ok(b)) if a.len() == b.len() => std::fs::read(&src).ok() == std::fs::read(&dst).ok(),
                _ => false,
            };
            if !same {
                std::fs::copy(&src, &dst).map_err(|e| format!("{}: {}", dst.display(), e))?;
                copied += 1;
            }
        }
    }
    Ok(copied)
}

/// A key for signing sessions, made once and kept in the app's container, so
/// a visitor stays signed in when the app is opened again.
fn keep_secret(data: &Path) -> Result<(), String> {
    if std::env::var("QUILLO_SECRET").map_or(false, |s| !s.is_empty()) {
        return Ok(());
    }
    let file = data.join(".quillo-secret");
    let secret = match std::fs::read_to_string(&file) {
        Ok(s) if s.trim().len() >= 32 => s.trim().to_string(),
        _ => {
            let s = crate::server::launch_token()?;
            std::fs::write(&file, &s).map_err(|e| format!("{}: {}", file.display(), e))?;
            s
        }
    };
    std::env::set_var("QUILLO_SECRET", secret);
    Ok(())
}

/// Copies the program from `bundle` into `<data>/app`, loads `main` (a file of
/// it, "main.qwe" when empty) and serves it on a thread of its own; returns
/// once it listens. `device` is what the launcher knows of the device, as JSON,
/// for `platform()` ("" when nothing). Called once per process.
pub fn start(bundle: &Path, data: &Path, main: &str, device: &str) -> Result<Started, String> {
    crate::platform::set_runtime("mobile");
    crate::platform::set_launcher(device);
    let app_dir = data.join("app");
    std::fs::create_dir_all(&app_dir).map_err(|e| format!("{}: {}", app_dir.display(), e))?;
    sync(bundle, &app_dir)?;
    keep_secret(data)?;
    let main = if main.is_empty() { "main.qwe" } else { main };
    let path = app_dir.join(main);
    if !path.exists() {
        return Err(format!("no {} in the app's program", main));
    }
    let token = crate::server::launch_token()?;
    let (tx, rx) = std::sync::mpsc::channel::<Result<u16, String>>();
    let thread_token = token.clone();
    std::thread::Builder::new()
        .name("quillo-mobile".into())
        .stack_size(crate::server::WORKER_STACK)
        .spawn(move || {
            // Beside the file, as `quillo run` has it: `main` may be in a subfolder
            // (FEEDBACK20.md, Q-169; branch feedback20-fixes).
            let base = path.parent().map_or_else(|| app_dir.clone(), Path::to_path_buf);
            let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            crate::service::serve_as(&stem);
            let mut interp = Interp::new(&base);
            let shared = interp.shared.clone();
            if let Err(e) = interp.load_file(&path) {
                let _ = tx.send(Err(e.to_string()));
                return;
            }
            if interp.routes.is_empty() && interp.mounts.is_empty() {
                let _ = tx.send(Err(format!("{} declares no routes, so there is nothing to show", path.display())));
                return;
            }
            let mut app = App::new(Loaded::Ready(interp), shared, &path, &base, false);
            app.launch_token = Some(thread_token);
            let ready = tx.clone();
            if let Err(e) = crate::server::serve_with(app, "127.0.0.1:0", 8, move |a| {
                let _ = ready.send(Ok(a.port()));
            }) {
                let _ = tx.send(Err(e));
            }
        })
        .map_err(|e| format!("cannot start the program's thread: {}", e))?;
    let port = rx.recv().map_err(|_| "the program stopped before it listened".to_string())??;
    Ok(Started { port, url: format!("http://127.0.0.1:{}/?{}={}", port, crate::server::LAUNCH_PARAM, token) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bundle_is_copied_and_what_the_program_wrote_stays() {
        let root = std::env::temp_dir().join(format!("quillo-mobile-{}", std::process::id()));
        let (from, to) = (root.join("bundle"), root.join("data"));
        std::fs::create_dir_all(from.join("lib")).unwrap();
        std::fs::write(from.join("main.qwe"), "route GET / = \"one\"\n").unwrap();
        std::fs::write(from.join("lib/x.qwe"), "fn x() = 1\n").unwrap();
        std::fs::write(from.join("seed.db"), "seed").unwrap();
        assert_eq!(sync(&from, &to).unwrap(), 3);
        assert_eq!(sync(&from, &to).unwrap(), 0, "nothing changed, nothing copied");
        // The program wrote its database; an app update changes the code.
        std::fs::write(to.join("seed.db"), "the user's rows").unwrap();
        std::fs::write(from.join("main.qwe"), "route GET / = \"two\"\n").unwrap();
        assert_eq!(sync(&from, &to).unwrap(), 1);
        assert_eq!(std::fs::read_to_string(to.join("main.qwe")).unwrap(), "route GET / = \"two\"\n");
        assert_eq!(std::fs::read_to_string(to.join("seed.db")).unwrap(), "the user's rows");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_program_is_served_to_the_web_view_and_no_one_else() {
        use std::io::{Read, Write};
        let root = std::env::temp_dir().join(format!("quillo-mobile-start-{}", std::process::id()));
        let (from, to) = (root.join("bundle"), root.join("data"));
        std::fs::create_dir_all(&from).unwrap();
        std::fs::write(from.join("main.qwe"), "db.open(\"app.db\")\ndb.exec(\"create table if not exists t (n int)\")\nroute GET / = \"hello from the phone\"\n").unwrap();
        let s = start(&from, &to, "", r#"{ "model": "Test1,1" }"#).unwrap();
        let get = |path: &str| {
            let mut c = std::net::TcpStream::connect(("127.0.0.1", s.port)).unwrap();
            write!(c, "GET {} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n", path).unwrap();
            let mut out = String::new();
            c.read_to_string(&mut out).unwrap();
            out
        };
        assert!(get("/").starts_with("HTTP/1.1 403"), "no token, no page");
        let first = get(&s.url[s.url.find("/?").unwrap()..]);
        assert!(first.starts_with("HTTP/1.1 30") || first.contains("hello from the phone"), "{}", first);
        assert!(to.join("app/app.db").exists(), "the database is in the app's container");
        assert!(std::fs::read_to_string(to.join(".quillo-secret")).unwrap().len() >= 32);
        let _ = std::fs::remove_dir_all(&root);
    }
}
