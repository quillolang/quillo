use std::io::{self, BufRead, Write};
use std::path::Path;
use wisp::ast::Stmt;
use wisp::interp::Ctrl;
use wisp::check::SymbolKind;
use wisp::parser::Parser;
use wisp::server::{App, Loaded};
use wisp::{Interp, Value};

const VERSION: &str = env!("CARGO_PKG_VERSION");

const USAGE: &str = "\
Wisp — a small web language

usage:
  wisp run <file.wisp> [options]               run a program; serve it if it has routes
  wisp check <file.wisp>                       check a program without running it
  wisp repl                                    interactive prompt
  wisp <file.wisp>                             same as `wisp run`

options for `run`:
  --port N       port to listen on (default $PORT, then 8080)
  --host H       address to listen on (default 127.0.0.1)
  --workers N    request threads (default 8)
  --watch, -w    reload when the program's files change, and refresh open pages
  --no-check     skip the static checks before running";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // Run on a thread with a roomy stack (see WORKER_STACK).
    let child = std::thread::Builder::new()
        .stack_size(wisp::server::WORKER_STACK)
        .spawn(move || {
            real_main(args)
        })
        .expect("cannot start interpreter thread");
    std::process::exit(child.join().unwrap_or(1));
}

fn real_main(args: Vec<String>) -> i32 {
    match args.get(1).map(String::as_str) {
        Some("run") => match args.get(2) {
            Some(f) => run(f, &args[3..]),
            None => usage(),
        },
        Some("check") => match args.get(2) {
            Some(f) => check(f),
            None => usage(),
        },
        Some("repl") | None => repl(),
        Some("version" | "--version" | "-V") => {
            println!("wisp {}", VERSION);
            0
        }
        Some("help" | "--help" | "-h") => {
            println!("{}", USAGE);
            0
        }
        Some(f) if f.ends_with(".wisp") => run(f, &args[2..]),
        Some(_) => usage(),
    }
}

fn usage() -> i32 {
    eprintln!("{}", USAGE);
    2
}

fn run(file: &str, flags: &[String]) -> i32 {
    let mut host = "127.0.0.1".to_string();
    let mut port = std::env::var("PORT").unwrap_or_else(|_| "8080".into());
    let mut workers = 8;
    let mut watch = false;
    let mut static_check = true;
    let mut i = 0;
    while i < flags.len() {
        let value = flags.get(i + 1);
        match (flags[i].as_str(), value) {
            ("--watch" | "-w", _) => {
                watch = true;
                i += 1;
                continue;
            }
            ("--no-check", _) => {
                static_check = false;
                i += 1;
                continue;
            }
            ("--host", Some(v)) => host = v.clone(),
            ("--port", Some(v)) => port = v.clone(),
            ("--workers", Some(v)) => match v.parse::<usize>() {
                Ok(n) if n > 0 => workers = n,
                _ => {
                    eprintln!("wisp: --workers needs a positive number");
                    return 2;
                }
            },
            (other, _) => {
                eprintln!("wisp: unknown option `{}`", other);
                return 2;
            }
        }
        i += 2;
    }

    let path = Path::new(file);
    let base = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let checked = if static_check { static_errors(path) } else { Ok(()) };
    if let (Err(_), false) = (&checked, watch) {
        return 1;
    }
    let mut interp = Interp::new(base);
    let shared = interp.shared.clone();
    let loaded = match checked.and_then(|()| interp.load_file(path).map_err(|e| e.to_string())) {
        Ok(_) => Loaded::Ready(interp),
        Err(e) => {
            if !e.is_empty() {
                eprintln!("wisp: {}", e);
            }
            if !watch {
                return 1;
            }
            Loaded::Failed(if e.is_empty() { "the program has errors (see the terminal)".into() } else { e })
        }
    };
    if let Loaded::Ready(it) = &loaded {
        if it.routes.is_empty() && !watch {
            return 0;
        }
    }
    let mut app = App::new(loaded, shared, path, base, watch);
    app.static_check = static_check;
    match wisp::server::serve(app, &format!("{}:{}", host, port), workers) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("wisp: {}", e);
            1
        }
    }
}

/// Prints the checker's findings; `Err` when there are errors. The error
/// text is empty because everything has been printed already.
fn static_errors(path: &Path) -> Result<(), String> {
    let analysis = wisp::check::check_file(path);
    for d in &analysis.diagnostics {
        eprintln!("{}", d);
    }
    if analysis.errors().next().is_some() {
        return Err(String::new());
    }
    Ok(())
}

fn check(file: &str) -> i32 {
    let analysis = wisp::check::check_file(Path::new(file));
    for d in &analysis.diagnostics {
        eprintln!("{}", d);
    }
    let errors = analysis.errors().count();
    let warnings = analysis.diagnostics.len() - errors;
    if errors > 0 {
        eprintln!("wisp: {} error{}{}", errors, if errors == 1 { "" } else { "s" }, plural_warnings(warnings));
        return 1;
    }
    let count = |k: SymbolKind| analysis.symbols.iter().filter(|s| s.kind == k && s.file == file).count();
    println!(
        "{}: ok ({} routes, {} functions, {} components){}",
        file,
        count(SymbolKind::Route),
        count(SymbolKind::Function) + count(SymbolKind::ServerFn) + count(SymbolKind::View),
        count(SymbolKind::Component),
        plural_warnings(warnings)
    );
    0
}

fn plural_warnings(n: usize) -> String {
    match n {
        0 => String::new(),
        1 => ", 1 warning".into(),
        n => format!(", {} warnings", n),
    }
}

fn repl() -> i32 {
    let mut interp = Interp::new(".");
    let stdin = io::stdin();
    let mut buf = String::new();
    println!("Wisp {} — enter expressions; Ctrl-D exits", VERSION);
    loop {
        print!("{}", if buf.is_empty() { "wisp> " } else { "....> " });
        let _ = io::stdout().flush();
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).unwrap_or(0) == 0 {
            println!();
            return 0;
        }
        buf.push_str(&line);
        let stmts = match Parser::new(&buf).parse_program() {
            Ok(s) => s,
            Err(e) if e.msg.contains("end of input") => continue,
            Err(e) => {
                eprintln!("error: {}", e);
                buf.clear();
                continue;
            }
        };
        buf.clear();
        let globals = interp.globals.clone();
        for s in &stmts {
            match interp.exec_stmt(s, &globals) {
                Ok(v) => {
                    if matches!(s, Stmt::Expr(_)) && !matches!(v, Value::Nil) {
                        println!("{}", v.repr());
                    }
                }
                Err(Ctrl::Return(v)) => println!("{}", v.repr()),
                Err(Ctrl::Reply(r)) => println!("<reply {}>", r.status),
                Err(Ctrl::Error(e)) => {
                    eprintln!("error: {}", e);
                    break;
                }
            }
        }
    }
}
