//! `quillo new <folder>`: a program to start from (FEEDBACK34.md, Q-422; branch
//! feedback27-35-fixes). The to-do app is `app/`'s own files, embedded, so the sample and what
//! `new` writes cannot differ; `--minimal` is a page, a component and a function both sides use.
//! It writes into a folder beside the target and moves it in place at the end, so a failed `new`
//! leaves nothing half made, and it never writes an empty folder (Q-423).

use std::path::{Path, PathBuf};

/// The to-do app, as `app/` has it.
const TODO: &[(&str, &str)] = &[
    ("main.qwe", include_str!("../app/main.qwe")),
    ("lib/todos.qwe", include_str!("../app/lib/todos.qwe")),
    ("ui/page.qwe", include_str!("../app/ui/page.qwe")),
    ("ui/todos.qwe", include_str!("../app/ui/todos.qwe")),
    ("migrations/001_todos.sql", include_str!("../app/migrations/001_todos.sql")),
    ("public/css/style.css", include_str!("../app/public/css/style.css")),
    ("public/js/app.js", include_str!("../app/public/js/app.js")),
    ("public/icons/favicon.svg", include_str!("../app/public/icons/favicon.svg")),
];

const MINIMAL_MAIN: &str = r#"// A Quillo program. This file is the program: the files it uses and its routes.
//
//   quillo run --watch      then open http://127.0.0.1:8080
//   quillo check            checks every file it uses, without running
//
//   lib/greet.qwe     functions, for the server and the browser
//   ui/page.qwe       the frame every page uses, and a component

import lib/greet.qwe use greeting
import ui/page.qwe use Page, Counter

route GET / =
    <Page title="{TITLE}">
        <h1>{greeting("world")}</h1>
        <Counter start={0}/>
    </Page>

route GET /hello/:name = greeting(name)
"#;

const MINIMAL_GREET: &str = r#"// Plain functions: the server and the browser both use them.

fn greeting(name: str) -> str = `Hello, ${name}!`
"#;

const MINIMAL_PAGE: &str = r#"// The frame every page uses, and a component that runs in the browser.

view Page(title: str, children) =
    <html>
        <head>
            <meta charset="utf-8"/>
            <meta name="viewport" content="width=device-width, initial-scale=1"/>
            <title>{title}</title>
        </head>
        <body>{children}</body>
    </html>

component Counter(start: int) {
    state count = start
    <button @click={() => { count += 1 }}>Clicked {count} times</button>
}
"#;

const GITIGNORE: &str = "*.db\n*.db-wal\n*.db-shm\n.env\n.env.*\n.quillo/\n";

pub const USAGE: &str = "usage: quillo new <folder> [--minimal] [--binary]
  a new program in <folder>: the to-do app (a database, a component, files
  served), or with --minimal a page, a component and a function; --binary
  puts a copy of this quillo in <folder>/.quillo/bin";

/// Why a folder's name cannot be used, if it cannot: what Windows refuses is refused on every
/// system, so a program made on a Mac can be cloned on Windows.
pub fn name_problem(name: &str) -> Option<String> {
    if name.is_empty() {
        return Some("the folder needs a name".into());
    }
    if name.chars().any(|c| c.is_control()) {
        return Some(format!("{:?} has a line break or a control character in it", name));
    }
    if let Some(c) = name.chars().find(|c| "<>:\"|?*\\".contains(*c)) {
        return Some(format!("{:?} has `{}` in it, which Windows does not take in a name", name, c));
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Some(format!("{:?} ends with a dot or a space, which Windows does not keep", name));
    }
    let stem = name.split('.').next().unwrap_or("").to_ascii_lowercase();
    let reserved = matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
        || (stem.len() == 4 && (stem.starts_with("com") || stem.starts_with("lpt")) && stem.as_bytes()[3].is_ascii_digit() && stem.as_bytes()[3] != b'0');
    reserved.then(|| format!("{:?} is a name Windows keeps for a device", name))
}

/// `quillo new` with its arguments (after `new`): the exit status.
pub fn main(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{}", USAGE);
        return 0;
    }
    let mut minimal = false;
    let mut binary = false;
    let mut folder = None;
    for a in args {
        match a.as_str() {
            "--minimal" => minimal = true,
            "--binary" => binary = true,
            o if o.starts_with('-') => {
                let near = ["--minimal", "--binary"].iter().find(|k| crate::check::edit_distance(o, k) <= 2);
                eprintln!("quillo: unknown option `{}` for `new`{}", o, near.map_or(String::new(), |k| format!(" (did you mean `{}`?)", k)));
                return 2;
            }
            f if folder.is_none() => folder = Some(f.to_string()),
            f => {
                eprintln!("quillo: unknown argument `{}`: `new` takes one folder", f);
                return 2;
            }
        }
    }
    let Some(folder) = folder else {
        eprintln!("{}", USAGE);
        return 2;
    };
    match make(Path::new(&folder), minimal, binary) {
        Ok(said) => {
            for s in said {
                eprintln!("quillo: {}", s);
            }
            let cd = if folder == "." { String::new() } else { format!("cd {} && ", shell_word(&folder)) };
            let run = if binary { ".quillo/bin/quillo run --watch" } else { "quillo run --watch" };
            println!("made {}: {}{}, then open http://127.0.0.1:8080", folder, cd, run);
            0
        }
        Err(e) => {
            eprintln!("quillo: new: {}", e);
            1
        }
    }
}

fn shell_word(s: &str) -> String {
    if s.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_./".contains(&b)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// The files of a program for `folder`, by path.
pub fn files(name: &str, minimal: bool) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = if minimal {
        // The name in `title="..."` alone, never in a comment, where a line break in it would
        // be code; `\` and `"` escaped as a string needs.
        let title = name.replace('\\', "\\\\").replace('"', "\\\"");
        vec![
            ("main.qwe".into(), MINIMAL_MAIN.replace("{TITLE}", &title)),
            ("lib/greet.qwe".into(), MINIMAL_GREET.into()),
            ("ui/page.qwe".into(), MINIMAL_PAGE.into()),
        ]
    } else {
        TODO.iter()
            .map(|(p, t)| {
                // Its comments name it from the repository's root: here it is run in its own folder.
                let t = if *p == "main.qwe" {
                    t.replace("quillo run app/main.qwe --watch     then open http://localhost:8080", "quillo run --watch     then open http://127.0.0.1:8080")
                        .replace("quillo check app/main.qwe           checks", "quillo check           checks")
                } else {
                    t.to_string()
                };
                (p.to_string(), t)
            })
            .collect()
    };
    out.push((".gitignore".into(), GITIGNORE.into()));
    out
}

fn make(target: &Path, minimal: bool, binary: bool) -> Result<Vec<String>, String> {
    let mut said = Vec::new();
    let abs = if target.is_absolute() { target.to_path_buf() } else { std::env::current_dir().map_err(|e| e.to_string())?.join(target) };
    let name = abs.file_name().map(|n| n.to_string_lossy().into_owned()).or_else(|| abs.canonicalize().ok().and_then(|c| c.file_name().map(|n| n.to_string_lossy().into_owned()))).unwrap_or_default();
    if let Some(why) = name_problem(&name) {
        return Err(why);
    }
    let exists = abs.exists();
    if exists {
        if !abs.is_dir() {
            return Err(format!("{} is a file, not a folder", target.display()));
        }
        let mut there: Vec<String> = std::fs::read_dir(&abs).map_err(|e| e.to_string())?.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n != ".git" && n != ".DS_Store").collect();
        there.sort();
        if !there.is_empty() {
            let shown: Vec<&str> = there.iter().take(5).map(String::as_str).collect();
            return Err(format!("{} is not empty: it has {}{}", target.display(), shown.join(", "), if there.len() > 5 { ", ..." } else { "" }));
        }
    }
    let parent = abs.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    if parent.ancestors().any(|a| a.join("main.qwe").is_file()) {
        said.push(format!("warning: {} is inside another program (a folder above has a main.qwe): a phone build of that one leaves this out", target.display()));
    }
    std::fs::create_dir_all(&parent).map_err(|e| format!("cannot make {}: {}", parent.display(), e))?;
    let temp = parent.join(format!(".{}.quillo-new-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&temp);
    let mut write = || -> Result<(), String> {
        for (p, text) in files(&name, minimal) {
            let f = temp.join(&p);
            let d = f.parent().unwrap_or(&temp);
            std::fs::create_dir_all(d).map_err(|e| format!("cannot make {}: {}", d.display(), e))?;
            std::fs::write(&f, text).map_err(|e| format!("cannot write {}: {}", p, e))?;
        }
        if binary {
            let exe = std::env::current_exe().and_then(|e| e.canonicalize()).map_err(|e| format!("cannot find this quillo: {}", e))?;
            let to = temp.join(".quillo/bin").join(if cfg!(windows) { "quillo.exe" } else { "quillo" });
            std::fs::create_dir_all(to.parent().unwrap_or(&temp)).map_err(|e| e.to_string())?;
            std::fs::copy(&exe, &to).map_err(|e| format!("cannot copy {}: {}", exe.display(), e))?;
            if exe.components().any(|c| c.as_os_str() == "debug") {
                let mb = std::fs::metadata(&exe).map_or(0, |m| m.len()) / 1_000_000;
                said.push(format!("warning: the copy is a debug build ({} MB, and slower): a release build is smaller", mb));
            }
        }
        Ok(())
    };
    if let Err(e) = write() {
        let _ = std::fs::remove_dir_all(&temp);
        return Err(e);
    }
    let moved = if exists {
        // Into the folder there (empty, or holding only `.git`), entry by entry.
        std::fs::read_dir(&temp).map_err(|e| e.to_string()).and_then(|entries| {
            for e in entries.flatten() {
                std::fs::rename(e.path(), abs.join(e.file_name())).map_err(|e| e.to_string())?;
            }
            Ok(())
        })
    } else {
        std::fs::rename(&temp, &abs).map_err(|e| e.to_string())
    };
    let _ = std::fs::remove_dir_all(&temp);
    moved.map_err(|e| format!("cannot move the program into {}: {}", target.display(), e))?;
    Ok(said)
}
