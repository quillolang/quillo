use std::io::{self, BufRead, Write};
use std::path::Path;
use wisp::ast::Stmt;
use wisp::interp::Ctrl;
use wisp::parser::Parser;
use wisp::{Interp, Value};

const VERSION: &str = env!("CARGO_PKG_VERSION");

const USAGE: &str = "\
Wisp — a small web language

usage:
  wisp run <file.wisp> [--host H] [--port N]   run a program; serve it if it has routes
  wisp check <file.wisp>                       parse only, report errors
  wisp repl                                    interactive prompt
  wisp <file.wisp>                             same as `wisp run`

The port defaults to $PORT, then 8080. The host defaults to 127.0.0.1.";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // Deeply recursive programs need more stack than the main thread has.
    let child = std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(move || real_main(args))
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
    let mut i = 0;
    while i < flags.len() {
        match (flags[i].as_str(), flags.get(i + 1)) {
            ("--host", Some(v)) => host = v.clone(),
            ("--port", Some(v)) => port = v.clone(),
            (other, _) => {
                eprintln!("wisp: unknown option `{}`", other);
                return 2;
            }
        }
        i += 2;
    }

    let path = Path::new(file);
    let base = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let mut interp = Interp::new(base);
    if let Err(e) = interp.load_file(path) {
        eprintln!("wisp: {}", e);
        return 1;
    }
    if interp.routes.is_empty() {
        return 0;
    }
    match wisp::server::serve(&mut interp, &format!("{}:{}", host, port)) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("wisp: {}", e);
            1
        }
    }
}

fn check(file: &str) -> i32 {
    let src = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("wisp: cannot read {}: {}", file, e);
            return 1;
        }
    };
    match Parser::new(&src).parse_program() {
        Ok(stmts) => {
            let routes = stmts.iter().filter(|s| matches!(s, Stmt::Route(_))).count();
            let fns = stmts.iter().filter(|s| matches!(s, Stmt::Fn(_))).count();
            println!("{}: ok ({} routes, {} functions)", file, routes, fns);
            0
        }
        Err(e) => {
            eprintln!("wisp: {}", e.in_file(file));
            1
        }
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
