/// Quillo's own messages go to stderr, and one that cannot be written is dropped: std's
/// `eprintln!` panics then, and a server whose stderr went into a pipe that closed (a `| head`,
/// a log shipper that stopped) could no longer be stopped by a signal, its handler ended by
/// the panic (FEEDBACK37.md, Q-452; branch feedback36-39).
macro_rules! eprintln {
    () => {{
        use std::io::Write as _;
        let _ = std::io::stderr().write_all(b"\n");
    }};
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stderr(), $($arg)*);
    }};
}

use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use quillo::ast::Stmt;
use quillo::interp::Ctrl;
use quillo::check::SymbolKind;
use quillo::parser::Parser;
use quillo::server::{App, Loaded};
use quillo::{Interp, Value};

#[cfg(feature = "app")]
mod desktop;

const USAGE: &str = "\
Quillo — a small web language

usage:
  quillo new <folder> [--minimal]   a program to start from: the to-do app, or a
               [--binary]           page and a component; --binary copies this
                                    quillo into <folder>/.quillo/bin
  quillo run <file.qwe> [options]   run a program; serve it if it has routes
  quillo app <file.qwe> [options]   run a program in a native window (needs a
                                    build with `--features app`)
  quillo check <file.qwe>           check a program without running it, and
               [--strict-null]      warn where a value may be null and null
                                    does not work
  quillo fix <file.qwe>             write the `import`s a program's files need
  quillo docs <file.qwe>            the program's API as OpenAPI 3: its routes,
              [--mcp]               and its server fns' contracts; --mcp: what an
              [--policy FILE]       AI client is told of its tools instead, as
              [--service-name N]    the MCP doors answer: server/discover on
                                    stdio and over HTTP (under N), and tools/list
  quillo build <file.qwe>           build the Rust (.qwer files) a program uses,
                                    into .quillo/native/ next to them, so it
                                    runs where Rust is not installed
  quillo install <plugin>...        download Rust plugins (.qwer) from
                                    quillolang.org/plugins into this folder,
                                    their version and hash checked; --example,
                                    --update, --list (`quillo install --help`)
  quillo repl [-i]                  interactive prompt (from a pipe, the answers
                                    alone; -i keeps the prompts)
  quillo lsp                        language server for editors (stdio)
  quillo dap                        debugger for editors (the Debug Adapter
                                    Protocol, on stdio): breakpoints, stepping,
                                    variables
  quillo classes <file.qwe>         the classes the program's markup writes, one a
                                    line, for a CSS tool (Tailwind's safelist)
  quillo serve <folder> [options]   serve a folder of files as they are, as a
                                    static host does: index.html, clean URLs,
                                    404.html (the options of `run`)
  quillo fcgi <file.qwe> [options]  run it behind FastCGI (mod_fcgid starts it with
                                    its socket on file descriptor 0; or --listen
                                    unix:PATH, or --port, for nginx's fastcgi_pass)
  quillo mcp <file.qwe> [options]   serve the program's tools (`fn mcp`) to an AI
                                    client that starts it, over stdio (MCP
                                    2026-07-28): checked, its top level run, then
                                    messages on stdin and stdout; no routes
  quillo <file.qwe>                 same as `quillo run`

The program is the first argument that is not an option, wherever the options
stand. Left out, or a folder, it is that folder's main.qwe: `quillo run` in a
program's folder runs it. An option's value follows it, or `=`: `--port 8081`
or `--port=8081`.

options for `run`:
  --port N       port to listen on (default $PORT, then 8080)
  --host H       address to listen on (default 127.0.0.1)
  --listen unix:PATH  listen on a Unix socket, for a web server in front (mode 0600; --socket-mode 660)
  --policy FILE  narrow what it may do, after /etc/quillo/policy.toml and before quillo.policy
  --env-file F   the settings from F, not the .env beside the program (F must be there)
  --no-build     load the .qwer libraries .quillo holds, as plugins.lock names them; never run cargo
  --instances N  run the program N times over, each on a core of its own
  --max-compute 2s  end a request that computes this long without letting go, as a 500
  --trust-proxy A,B believe X-Forwarded-For from these addresses or networks
  -- a b         what follows is the program's, for args()
  --allow-path DIR  read_file and write_file may use DIR too, for a script
                 used as a tool (any part of the program may then)
  --allowed-hosts A,B  under --watch, host names answered besides localhost and
                 addresses (.example.test takes its subdomains too)
  --max-connections N  connections open at once (default: what the file
                 descriptors allow, less 4,608)
  --connections-per-client N  connections one client may hold open (default
                 512; this machine and --trust-proxy's proxies are not held to it)
  --live-per-client N  pages one client may have waiting for live data (default 256)
                 (separate variables, as N processes would have; the
                 database and `publish` changes reach them all)
  --max-tool-calls N  tool calls a minute each user (a token's, a session's;
                 else the session) may make at /_quillo/mcp, then 429 (default 60)
  --workers N    requests that can wait (for a database, the network) at
                 once, each on a thread of its own (default 64)
  --watch, -w    reload when the program's files change, and refresh open pages
  --reload       reload when the program's files change (in production: no
                 errors shown on pages, no page refreshes)
  --no-check     skip the static checks before running
  --split        run each `service` the program declares as a process of
                 its own, on a port of its own (one process: the default)
  --service-url NAME=URL
                 where service NAME runs, when not here (also
                 QUILLO_SERVICE_<NAME>, as QUILLO_SERVICE_BILLING); calls are signed with
                 QUILLO_SERVICE_SECRET, the same on every service
  --base PATH    the path the program is served under (/shop), as a mounted
                 program is: its links and redirects carry it
  --describe     answer GET /_quillo/describe with the program's API (as
                 `quillo docs` writes it)
  --scale NAME=N with --split: run service NAME as N copies, taking calls in
                 turn (they share its database)
  --with CMD     run CMD beside the program, in its folder, its lines marked,
                 stopped with it (`--with \"npx tailwindcss -i src/app.css -o
                 public/app.css --watch\"`); may be given more than once
  --join URL     with --as-service: join the app at URL as one more copy of
                 this service, while it runs; leaves when stopped; signed with
                 $QUILLO_SERVICE_KEY when set, which proves it is this service
  --join-as URL  with --join: the address the app calls this copy at, such as
                 https://billing-3.internal:8443 (it must lead to where the
                 copy joined from)
  --no-join      take no copies that join; --allow-join billing,risk: only those
  --as-service   serve the program as a service: its server fns answer only
                 other services' signed calls, not pages (--split sets it)
  --service-name NAME
                 the name other programs call this service by, when it is
                 not the program's file name: calls signed for another
                 service are refused
  --no-view-cache  render views every time, instead of reusing their HTML
                 while their arguments and the globals they read are unchanged
  --no-jobs      run none of the program's `schedule`d jobs on this server
  --no-prefetch  pages do not prefetch the links pointed at (see Prefetching
                 in the user guide)
  --plain-http   the site is served over plain HTTP on purpose, on a network
                 you trust: cookies are not Secure (browsers drop Secure ones
                 there, so signing in would not stick)
  --max-body N   largest request body to accept, in bytes (default 10 MiB)
  --wt-port N    the UDP port pages connect to for channels over WebTransport
                 (default: the HTTP port's number)
  --tls-cert F, --tls-key F
                 the certificate and key (PEM) WebTransport presents; without
                 them it makes its own, which pages take by its hash
  --no-wt        no WebTransport: pages' channels use a WebSocket
  --log F        the request log on stderr: text (the default), json (a JSON
                 object per line: time, method, path, status, ms, ip, tenant,
                 rid) or off
  --metrics      answer GET /_quillo/metrics: request counts and times, open
                 connections and uptime, in Prometheus's text format
  --query-stats  time database queries for GET /_quillo/health/queries: min,
                 max and average of the last 100, overall and per statement
                 (on under --watch; also QUILLO_QUERY_STATS=1, or switched on
                 while running with the health token)
  --register URL join limer's `discover` pool through its admin API (such as
                 http://127.0.0.1:2019), and share live updates with the other
                 servers there; limer's admin secret must be in
                 $LIMER_ADMIN_SECRET
  --service S    the pool's name (default: the file's name, without .qwe)

`app` takes the options of `run` except --host, --register and --service; its
port, by default one the system picks, answers only the window. `--open PATH`
is the page the window opens on (default /).

`mcp` takes --env-file, --policy, --allow-path, --no-build and `-- a b`. Its
stdout carries MCP's messages alone: `print` and the log go to stderr, and
`stdin`, `read_line` and `exit` are refused. It ends when stdin closes, or on
SIGTERM once the call in progress is answered.";

// See quillo::alloc.
#[global_allocator]
static ALLOC: quillo::alloc::Mi = quillo::alloc::Mi;

fn main() {
    let mut args: Vec<String> = std::env::args().collect();
    // `quillo run tool.qwe -- a b`: what follows is the program's, for `args()`
    // (FEEDBACK26.md, Q-202; branch feedback26-fixes).
    // `quillo mcp tool.qwe -- a b` too (FEEDBACK56.md, C.1; branch feedback56).
    if args.get(1).is_some_and(|c| c == "run" || c == "mcp" || c.ends_with(".qwe")) {
        if let Some(at) = args.iter().position(|a| a == "--") {
            let mut theirs = args.split_off(at + 1);
            args.pop();
            // `quillo run -- a.qwe x`: a `--` before the program ends the options, and the
            // program is the word after it (FEEDBACK31.md, Q-381; branch feedback27-35-fixes).
            let from = if args[1] == "run" || args[1] == "mcp" { 2 } else { 1 };
            let named = program_and_flags(&args[from..]).ok().is_some_and(|_| has_program(&args[from..]));
            if !named && !theirs.is_empty() {
                args.push(theirs.remove(0));
            }
            let _ = quillo::builtins::PROGRAM_ARGS.set(theirs);
        }
    }
    if let Some(usage) = help_asked(&args) {
        println!("{}", usage);
        std::process::exit(0);
    }
    // The window must run on the main thread (macOS requires it); the
    // program gets a thread of its own.
    if args.get(1).map(String::as_str) == Some("app") {
        std::process::exit(app(&args));
    }
    // Run on a thread with a roomy stack (see WORKER_STACK).
    let child = match std::thread::Builder::new().stack_size(quillo::server::WORKER_STACK).spawn(move || real_main(args)) {
        Ok(child) => child,
        // Said, rather than a panic: an account at its process limit, which counts every thread
        // of its processes, cannot start one more (found by CI, where the account had more than
        // the limit a test set; FEEDBACK41.md, Q-480).
        Err(e) => {
            eprintln!("quillo: cannot start: no thread can be made ({}){}", e, quillo::server::process_limit_note());
            std::process::exit(1);
        }
    };
    // A panic is a way out too: what runs beside the program stops with it (FEEDBACK31.md,
    // Q-382; branch feedback27-35-fixes).
    let code = child.join().unwrap_or(1);
    with::stop_all();
    std::process::exit(code);
}

/// Options of `run` and `app` that take a value: the word after one is its
/// value, not the program.
const VALUE_OPTIONS: &[&str] = &[
    "--host", "--port", "--listen", "--socket-mode", "--policy", "--workers", "--register", "--service", "--service-url", "--service-name", "--base",
    "--max-body", "--max-compute", "--max-tool-calls", "--allow-path", "--trust-proxy", "--live-per-client", "--max-connections", "--connections-per-client", "--allowed-hosts", "--instances", "--wt-port", "--tls-cert", "--tls-key", "--log", "--open", "--env-file",
    "--scale", "--join", "--with", "--allow-join", "--join-as",
];

/// Options of `run`, `app` and `check` that take no value.
const FLAG_OPTIONS: &[&str] = &[
    "--watch", "--reload", "--no-check", "--split", "--as-service", "--no-wt", "--no-prefetch", "--no-view-cache",
    "--no-jobs", "--plain-http", "--metrics", "--query-stats", "--strict-null", "--describe", "--no-join", "--checked", "--no-build",
];

/// What an unknown option was probably meant to be: the nearest known one
/// (FEEDBACK9.md, Q-070; branch feedback9-fixes).
fn meant(option: &str) -> String {
    if option == "-p" {
        return " (the port is `--port`)".to_string();
    }
    nearest(option, VALUE_OPTIONS.iter().chain(FLAG_OPTIONS))
}

fn nearest<'a>(option: &str, known: impl Iterator<Item = &'a &'a str>) -> String {
    // Never itself: `--strict-null` under `run` is no misspelling (FEEDBACK16.md, Q-140; branch
    // feedback20-fixes).
    let near = known
        .map(|o| (quillo::check::edit_distance(option, o), *o))
        .filter(|(d, o)| *d <= 2.max(o.len() / 4) && *o != option)
        .min();
    near.map_or_else(String::new, |(_, o)| format!(" (did you mean `{}`?)", o))
}

/// Whether `args` name a program: a word that is no option and no option's value.
fn has_program(args: &[String]) -> bool {
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a.starts_with('-') {
            if VALUE_OPTIONS.contains(&a.as_str()) {
                i += 1;
            }
        } else {
            return true;
        }
        i += 1;
    }
    false
}

/// The program a command names and the options around it, wherever they
/// stand: the first argument that is neither an option nor an option's
/// value. A folder, or no program at all, is that folder's main.qwe, and
/// nothing else (FEEDBACK6.md, Q-058 and Q-059; branch feedback6-fixes).
fn program_and_flags(args: &[String]) -> Result<(String, Vec<String>), String> {
    let (program, flags) = word_and_flags(args)?;
    let mut named = program.unwrap_or_else(|| ".".to_string());
    // `quillo check main`: the file is `main.qwe`, as a `use` path without its extension is
    // (FEEDBACK45.md, Q-528; branch feedback40-43).
    if !Path::new(&named).exists() && Path::new(&format!("{}.qwe", named)).is_file() {
        named = format!("{}.qwe", named);
    }
    if Path::new(&named).is_dir() {
        let main = Path::new(&named).join("main.qwe");
        if !main.is_file() {
            // A folder of pages: said how to serve it (FEEDBACK36.md, Q-450; branch feedback36-39).
            let pages = Path::new(&named).join("index.html").is_file();
            let serve = if pages { format!("; to serve its files as they are, `quillo serve {}`", named) } else { String::new() };
            return Err(if named == "." {
                format!("no program: name a .qwe file, or add a main.qwe here{}", serve)
            } else {
                format!("no program: {} has no main.qwe; name a .qwe file{}", named, serve)
            });
        }
        return Ok((if named == "." { "main.qwe".to_string() } else { main.to_string_lossy().into_owned() }, flags));
    }
    // A file of another kind is said to be one, with how to serve it, rather than read as Quillo
    // and refused at its first character (FEEDBACK36.md, Q-450; branch feedback36-39).
    let ext = Path::new(&named).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let folder = Path::new(&named).parent().map(|p| p.to_string_lossy().into_owned()).filter(|p| !p.is_empty()).unwrap_or_else(|| ".".into());
    match ext.as_str() {
        "html" | "htm" => return Err(format!("{} is HTML, not a Quillo program: serve its folder as it is with `quillo serve {}`, or from a program with `route GET /*path = files(\"{}\", path)`", named, folder, folder)),
        "md" | "markdown" => return Err(format!("{} is Markdown, not a Quillo program: show it as a page with `route GET / = markdown_page(read_file(\"{}\"))`, or serve its folder with `quillo serve {}`", named, named, folder)),
        "rs" | "qwer" => return Err(format!("{} is Rust, not a Quillo program: a program uses it with `use \"{}\"`, and calls its `pub fn`s", named, Path::new(&named).file_name().map_or(named.clone(), |n| n.to_string_lossy().into_owned()))),
        "css" | "js" | "mjs" | "json" | "txt" | "xml" | "csv" | "svg" => return Err(format!("{} is not a Quillo program (a .qwe file): serve it with `file(\"{}\")` in a route, or its folder with `quillo serve {}`", named, named, folder)),
        _ => {}
    }
    Ok((named, flags))
}

/// The first word that is neither an option nor an option's value, and the options around it
/// (the words after it among them, for the command to refuse): `program_and_flags`'s reading,
/// which `quillo serve` reads its folder by too (FEEDBACK55.md, Q-847; branch feedback55).
fn word_and_flags(args: &[String]) -> Result<(Option<String>, Vec<String>), String> {
    let mut program = None;
    let mut flags = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        // `--port=8081` is `--port 8081`, for each option that takes a value
        // (FEEDBACK9.md, Q-070; branch feedback9-fixes).
        if let Some((name, value)) = a.split_once('=').filter(|(n, _)| n.starts_with("--")) {
            if VALUE_OPTIONS.contains(&name) {
                flags.push(name.to_string());
                flags.push(value.to_string());
                i += 1;
                continue;
            }
            if FLAG_OPTIONS.contains(&name) {
                return Err(format!("`{}` takes no value: write `{}` alone", name, name));
            }
        }
        if a.starts_with('-') {
            flags.push(a.clone());
            if VALUE_OPTIONS.contains(&a.as_str()) {
                if let Some(v) = args.get(i + 1) {
                    // `--service-name --watch`: an option is no value, and would go unseen
                    // (FEEDBACK31.md, Q-381; branch feedback27-35-fixes).
                    if VALUE_OPTIONS.contains(&v.as_str()) || FLAG_OPTIONS.contains(&v.as_str()) || matches!(v.as_str(), "-w" | "-h" | "--help") {
                        return Err(format!("`{}` needs a value, and `{}` is an option", a, v));
                    }
                    flags.push(v.clone());
                    i += 1;
                }
            }
        } else if program.is_none() {
            program = Some(a.clone());
        } else {
            flags.push(a.clone());
        }
        i += 1;
    }
    Ok((program, flags))
}

/// `quillo serve DIR`: the folder's files as they are, by a program of one route that is no file
/// in it (FEEDBACK36.md, Q-450; branch feedback36-39).
fn serve(args: &[String]) -> i32 {
    // Its options read as `run`'s are: `--port=8080`, and no option taken as a value
    // (FEEDBACK55.md, Q-847; branch feedback55).
    let (dir, flags) = match word_and_flags(args) {
        Ok(read) => read,
        Err(e) => {
            eprintln!("quillo: {}", e);
            return 2;
        }
    };
    let mut i = 0;
    while i < flags.len() {
        if VALUE_OPTIONS.contains(&flags[i].as_str()) {
            i += 1;
        } else if !flags[i].starts_with('-') {
            eprintln!("quillo: unknown argument `{}`: `serve` takes one folder", flags[i]);
            return 2;
        }
        i += 1;
    }
    let dir = dir.unwrap_or_else(|| ".".into());
    if !Path::new(&dir).is_dir() {
        eprintln!("quillo: {} is not a folder: `quillo serve` serves a folder's files", dir);
        return 2;
    }
    let program = Path::new(&dir).join(".quillo-serve.qwe");
    quillo::error::set_virtual(program.clone(), "route GET /*path = files(\".\", path)\n".into());
    // Its URLs are its own (FEEDBACK55.md, Q-721; branch feedback55).
    quillo::builtins::SERVES_FOLDER.store(true, std::sync::atomic::Ordering::Relaxed);
    run(&program.to_string_lossy(), &flags)
}

/// `program_and_flags`, or the line that says why there is none (exit 2).
fn with_program(args: &[String], f: impl FnOnce(&str, &[String]) -> i32) -> i32 {
    match program_and_flags(args) {
        Ok((program, flags)) => f(&program, &flags),
        Err(e) => {
            eprintln!("quillo: {}", e);
            2
        }
    }
}

/// `with_program` for a command that takes one program and the options in
/// `known`. What it does not know is refused, as `run` refuses it: a misspelt
/// `--strict-null` would turn a check off, and a second program would go
/// unseen under `ok` (FEEDBACK16.md, Q-125; branch feedback20-fixes).
fn with_one_program(args: &[String], known: &[&str], f: impl FnOnce(&str, &[String]) -> i32) -> i32 {
    if args[2..].iter().any(|a| a == "-h" || a == "--help") {
        println!("{}", USAGE);
        return 0;
    }
    with_program(&args[2..], |program, flags| {
        for a in flags {
            if known.contains(&a.as_str()) {
                continue;
            }
            if a.starts_with('-') {
                eprintln!("quillo: unknown option `{}` for `{}`{}", a, args[1], nearest(a, known.iter()));
            } else {
                eprintln!("quillo: unknown argument `{}`: `{}` takes one program", a, args[1]);
            }
            return 2;
        }
        f(program, flags)
    })
}

fn real_main(args: Vec<String>) -> i32 {
    match args.get(1).map(String::as_str) {
        Some("run") => with_program(&args[2..], run),
        // FastCGI (FEEDBACK41.md, Q-479; branch feedback40-43).
        Some("fcgi") => with_program(&args[2..], |f, flags| {
            FCGI.store(true, std::sync::atomic::Ordering::Relaxed);
            run(f, flags)
        }),
        // A debug adapter for editors, on stdin and stdout (src/debug.rs).
        Some("dap") => quillo::debug::serve(|l| run(&l.program, &l.args)),
        // The program's tools for an AI client, over stdio (FEEDBACK56.md, C.1; branch feedback56).
        Some("mcp") => with_program(&args[2..], mcp),
        Some("check") => {
            // `--policy FILE`: what the program would be refused under it, said before a deploy
            // (FEEDBACK41.md, Q-478; branch feedback40-43).
            let mut rest = Vec::new();
            let mut policies = Vec::new();
            let mut i = 0;
            while i < args.len() {
                match args[i].strip_prefix("--policy=") {
                    Some(v) => policies.push(PathBuf::from(v)),
                    None if args[i] == "--policy" && i + 1 < args.len() && !args[i + 1].starts_with('-') => {
                        policies.push(PathBuf::from(&args[i + 1]));
                        i += 1;
                    }
                    // Its file left out (FEEDBACK45.md, Q-528; branch feedback40-43).
                    None if args[i] == "--policy" => {
                        eprintln!("quillo: `--policy` needs the policy's file: `quillo check main.qwe --policy policy.toml`");
                        return 2;
                    }
                    None => rest.push(args[i].clone()),
                }
                i += 1;
            }
            with_one_program(&rest, &["--strict-null"], |f, flags| {
                if let Err(code) = with_policy(Path::new(f), &policies, !policies.is_empty()) {
                    return code;
                }
                let strict_null = flags.iter().any(|a| a == "--strict-null");
                check(f, quillo::check::Options { strict_null, css: true, ..Default::default() })
            })
        }
        // The program's API as OpenAPI, from its source (FEEDBACK10.md, Q-077;
        // branch feedback10-fixes).
        // `--mcp`: what an AI client is told of the program's tools, as its doors answer
        // (FEEDBACK56.md, Part D; branch feedback56).
        Some("docs") => docs(&args),
        Some("build") => with_one_program(&args, &[], |f, _| match quillo::native::build_all(Path::new(f)) {
                Ok(libs) => {
                    for l in &libs {
                        println!("built {}", l.display());
                    }
                    if libs.is_empty() {
                        println!("{} uses no .qwer files: nothing to build", f);
                    }
                    0
                }
                Err(e) => {
                    eprintln!("quillo: {}", e);
                    1
                }
        }),
        // The `use`s a program's files need (src/modules.rs; branch feedback19-fixes).
        Some("fix") => with_one_program(&args, &[], |f, _| match quillo::modules::fix(Path::new(f)) {
            Ok(said) => {
                for s in &said {
                    println!("{}", s);
                }
                if said.is_empty() {
                    println!("{}: nothing to fix", f);
                }
                0
            }
            Err(e) => {
                eprintln!("quillo: {}", e);
                1
            }
        }),
        // Plugins from quillolang.org's catalog, into the program's folder.
        Some("install") => quillo::install::main(&args[2..]),
        // A program to start from (FEEDBACK34.md, Q-422; branch feedback27-35-fixes).
        Some("new") => quillo::new::main(&args[2..]),
        Some("serve") => serve(&args[2..]),
        // The classes the program writes, one a line, for a CSS tool beside it (FEEDBACK37.md,
        // Q-456; branch feedback36-39).
        Some("classes") => with_one_program(&args, &[], |f, _| {
            let mut files = Vec::new();
            match quillo::native::walk_program(Path::new(f), &mut |_, stmts| files.push(stmts.to_vec())) {
                Ok(_) => {
                    let refs: Vec<&[Stmt]> = files.iter().map(|s| s.as_slice()).collect();
                    for c in quillo::styles::written_classes(&refs) {
                        println!("{}", c);
                    }
                    0
                }
                Err(e) => {
                    eprintln!("quillo: {}", e);
                    1
                }
            }
        }),
        // `-i`: the prompts and the greeting, from a pipe too, as Python's `-i` (FEEDBACK31.md,
        // Q-388; branch feedback27-35-fixes).
        // Its one option: anything else is said, not passed over (FEEDBACK55.md, Q-852; branch
        // feedback55).
        Some("repl") => {
            if let Some(a) = args[2..].iter().find(|a| *a != "-i") {
                if a.starts_with('-') {
                    eprintln!("quillo: unknown option `{}` for `repl` (it takes -i)", a);
                } else if a.to_ascii_lowercase().ends_with(".qwe") {
                    eprintln!("quillo: unknown argument `{}`: `repl` takes no file; to run it: quillo run {}", a, a);
                } else {
                    eprintln!("quillo: unknown argument `{}`: `repl` takes no file", a);
                }
                return 2;
            }
            repl(args.len() > 2)
        }
        None => repl(false),
        Some("lsp") => quillo::lsp::Server::new().run(),
        Some("version" | "--version" | "-V") => {
            println!("quillo {}", quillo::version());
            0
        }
        Some("help" | "--help" | "-h") => {
            println!("{}", USAGE);
            0
        }
        // The same reading as `run`: `--log=off` is `--log off` here too
        // (FEEDBACK16.md, Q-139; branch feedback20-fixes).
        Some(f) if f.to_ascii_lowercase().ends_with(".qwe") => with_program(&args[1..], run),
        // A misspelt command is told the nearest, and a file given without `run` how to run it
        // (FEEDBACK31.md, Q-389; branch feedback27-35-fixes).
        Some(c) if !c.starts_with('-') => {
            let near = nearest(c, COMMANDS.iter());
            if Path::new(c).is_file() {
                eprintln!("quillo: unknown command `{}`{}; to run the file: quillo run {}", c, near, c);
            } else {
                eprintln!("quillo: unknown command `{}`{}; `quillo help` lists them", c, near);
            }
            2
        }
        // `quillo --watch main.qwe`: one line that says what to write, not the whole usage
        // (FEEDBACK55.md, Q-851; branch feedback55).
        Some(o) => {
            let name = o.split('=').next().unwrap_or(o);
            if VALUE_OPTIONS.contains(&name) || FLAG_OPTIONS.contains(&name) || name == "-w" {
                // A command written after the options is the one put first: `quillo --watch run
                // tool.qwe` is `quillo run --watch tool.qwe`, not `run` of a file named run.
                let rest = &args[1..];
                let mut i = 0;
                let mut named = None;
                while i < rest.len() {
                    let a = rest[i].as_str();
                    if VALUE_OPTIONS.contains(&a) {
                        i += 1;
                    } else if !a.starts_with('-') {
                        named = COMMANDS.contains(&a).then_some(i);
                        break;
                    }
                    i += 1;
                }
                let command = match named {
                    Some(at) => rest[at].as_str(),
                    None if name == "--strict-null" => "check",
                    None => "run",
                };
                let rest: Vec<String> = rest.iter().enumerate().filter(|(i, _)| Some(*i) != named).map(|(_, a)| shell_word(a)).collect();
                eprintln!("quillo: put the command first: `quillo {} {}`", command, rest.join(" "));
            } else {
                eprintln!("quillo: unknown option `{}`{}; `quillo help` lists the commands and their options", o, meant(name));
            }
            2
        }
    }
}

/// The options of `quillo docs`: `--mcp`, and with it the policy and the name the doors run under.
const DOCS_OPTIONS: &[&str] = &["--mcp", "--policy", "--service-name"];

/// `quillo docs main.qwe`: the program's API as OpenAPI, from its source. With `--mcp`, what an AI
/// client is told of its tools, as its doors answer (`mcp::served`): under the policy they run
/// under, the files beside it and `--policy`'s, as `quillo check` reads them, so that it prints
/// nothing for a program they would refuse; and over HTTP under `--service-name`'s name, as
/// `quillo run --service-name` serves it (FEEDBACK56.md, Part D, found in review; branch
/// feedback56).
fn docs(args: &[String]) -> i32 {
    if args[2..].iter().any(|a| a == "-h" || a == "--help") {
        println!("{}", USAGE);
        return 0;
    }
    with_program(&args[2..], |f, flags| {
        let (mut mcp, mut policies, mut name) = (false, Vec::new(), None);
        let mut i = 0;
        while i < flags.len() {
            let a = flags[i].as_str();
            match (a, flags.get(i + 1)) {
                ("--mcp", _) => mcp = true,
                ("--policy", Some(v)) => policies.push(PathBuf::from(v)),
                ("--service-name", Some(v)) => name = Some(v.clone()),
                ("--policy" | "--service-name", None) => {
                    eprintln!("quillo: `{}` needs a value", a);
                    return 2;
                }
                _ if a.starts_with('-') => {
                    eprintln!("quillo: unknown option `{}` for `docs`{}", a, nearest(a, DOCS_OPTIONS.iter()));
                    return 2;
                }
                _ => {
                    eprintln!("quillo: unknown argument `{}`: `docs` takes one program", a);
                    return 2;
                }
            }
            i += if a == "--mcp" { 1 } else { 2 };
        }
        if !mcp && (!policies.is_empty() || name.is_some()) {
            eprintln!("quillo: `--policy` and `--service-name` are for `quillo docs --mcp`: the OpenAPI document is the program's as written");
            return 2;
        }
        let doc = if mcp {
            if let Err(code) = with_policy(Path::new(f), &policies, !policies.is_empty()) {
                return code;
            }
            if let Some(n) = &name {
                quillo::service::serve_as(n);
            }
            quillo::mcp::served(Path::new(f))
        } else {
            quillo::describe::of_source(Path::new(f)).map_err(|e| e.to_string())
        };
        match doc {
            Ok(doc) => {
                println!("{}", serde_json::to_string_pretty(&doc).unwrap_or_default());
                0
            }
            Err(e) => {
                eprintln!("quillo: {}", e);
                1
            }
        }
    })
}

/// The commands, for the nearest to a misspelt one (FEEDBACK31.md, Q-389; `serve`, `classes` and
/// `fcgi` too: FEEDBACK55.md, Q-852; branch feedback55; `mcp`: FEEDBACK56.md, C.1; branch
/// feedback56).
const COMMANDS: &[&str] = &["run", "check", "docs", "build", "fix", "install", "repl", "lsp", "dap", "app", "new", "serve", "classes", "fcgi", "mcp", "version", "help"];

/// The options of `run` that `quillo mcp` takes: those about the program, not about serving it.
const MCP_OPTIONS: &[&str] = &["--env-file", "--policy", "--allow-path", "--no-build"];

/// `quillo mcp main.qwe`: the program's tools (`fn mcp`, `server fn mcp`) for an AI client on this
/// machine, which starts it, over stdio (FEEDBACK56.md, C.1; branch feedback56). Checked first and
/// its top level run, as `quillo run` does it; then MCP's messages are read on stdin and answered
/// on stdout, which carries nothing else. No routes are served, and there are no tokens: the user
/// who starts it is the caller.
fn mcp(file: &str, flags: &[String]) -> i32 {
    let mut i = 0;
    while i < flags.len() {
        let f = flags[i].as_str();
        if !MCP_OPTIONS.contains(&f) {
            if VALUE_OPTIONS.contains(&f) || FLAG_OPTIONS.contains(&f) || f == "-w" {
                eprintln!("quillo: `{}` is not for `quillo mcp`, which serves no routes: it takes --env-file, --policy, --allow-path and --no-build", f);
            } else if f.starts_with('-') {
                eprintln!("quillo: unknown option `{}` for `mcp`{}", f, nearest(f, MCP_OPTIONS.iter()));
            } else {
                eprintln!("quillo: unknown argument `{}`: `mcp` takes one program; its own arguments go after `--`", f);
            }
            return 2;
        }
        i += if VALUE_OPTIONS.contains(&f) { 2 } else { 1 };
    }
    // First: from here on nothing but MCP's messages reaches the client's stdout.
    let mut out = quillo::mcp::take_stdout();
    let _ = ctrlc::set_handler(quillo::mcp::stop_asked);
    let o = match run_options(flags) {
        Ok(o) => o,
        Err(code) => return code,
    };
    if let Err(code) = with_policy(Path::new(file), &o.policies, true) {
        return code;
    }
    // The policy's limits, held as `run_program` holds them: how long a call may compute, and
    // how long a message may be (found in review: they were not read here).
    let policy = quillo::policy::get();
    if let Some(c) = policy.max_compute {
        let (now, c) = (quillo::httpd::compute_limit_ms(), c.as_millis().max(1) as u64);
        if now == 0 || now > c {
            quillo::httpd::limit_compute(c);
            if let Some(f) = &policy.max_compute_from {
                quillo::httpd::compute_limit_from(f);
            }
        }
    }
    if let Err(code) = before_the_program(file, &o) {
        return code;
    }
    if let Ok(files) = quillo::native::rust_files(Path::new(file)) {
        quillo::native::prebuild(&files);
    }
    let path = Path::new(file);
    // Checked as `quillo run` checks it, and a program with errors does not start the door. The
    // tools are the checker's reading of it (src/mcp.rs).
    let analysis = quillo::check::check_file(path);
    for d in &analysis.diagnostics {
        eprintln!("{}", d);
    }
    if analysis.errors().next().is_some() {
        if !analysis.syntax_errors.is_empty() {
            eprintln!("note: {}", quillo::error::which_quillo());
        }
        return 1;
    }
    let tools = quillo::mcp::tools(&analysis);
    if tools.is_empty() {
        eprintln!("quillo: {} has no tools: a function is one when it is declared `fn mcp name(...)`", file);
    }
    let base = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    quillo::service::serve_as(&stem);
    // stdin is the client's from the start: `stdin`, `read_line` and `exit` are refused as in any
    // program that serves, at the top level too, where one would take the client's first message.
    quillo::value::SERVING.store(true, std::sync::atomic::Ordering::Relaxed);
    let mut interp = Interp::new(base);
    if let Err(e) = interp.load_file(path) {
        eprintln!("quillo: {:#}", e);
        return 1;
    }
    // What a builtin's error says is hidden from the client, as from a visitor, and logged.
    quillo::value::HIDING.store(true, std::sync::atomic::Ordering::Relaxed);
    // Its `//!` block too, told to the client after Quillo's lines (FEEDBACK56.md, Part D; branch
    // feedback56).
    let mut door = quillo::mcp::Door::of_program(path, stem, tools, false);
    if let Some(b) = policy.max_body {
        door.most = door.most.min(b);
    }
    let stdin = io::stdin();
    let mut input = stdin.lock();
    quillo::mcp::serve_stdio(&door, &mut interp, &mut input, &mut *out)
}

/// `-h` or `--help` after a command (or a program's file), wherever it stands: the command's usage,
/// on stdout, read before anything else, so that `run -h` needs no main.qwe and `lsp --help` starts
/// no server (FEEDBACK55.md, Q-851; branch feedback55).
fn help_asked(args: &[String]) -> Option<&'static str> {
    let c = args.get(1)?.as_str();
    if !COMMANDS.contains(&c) && !c.to_ascii_lowercase().ends_with(".qwe") {
        return None;
    }
    args[2..].iter().any(|a| a == "-h" || a == "--help").then(|| match c {
        "install" => quillo::install::USAGE,
        "new" => quillo::new::USAGE,
        _ => USAGE,
    })
}

/// A word as a shell takes it back.
fn shell_word(s: &str) -> String {
    if !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_./=:,@".contains(&b)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

#[cfg(feature = "app")]
fn app(args: &[String]) -> i32 {
    with_program(&args[2..], desktop::main)
}

#[cfg(not(feature = "app"))]
fn app(_: &[String]) -> i32 {
    eprintln!("quillo: this quillo was built without windows; build it with `cargo build --release --features app`");
    2
}

/// The options of `run` (and of `app`, which takes some of them).
struct RunOptions {
    host: Option<String>,
    port: Option<String>,
    /// `--policy FILE`s, layers between the host's and the program's (FEEDBACK41.md, Q-478).
    policies: Vec<PathBuf>,
    /// `--listen unix:PATH` (FEEDBACK41.md, Q-479; branch feedback40-43).
    listen: Option<PathBuf>,
    socket_mode: u32,
    workers: usize,
    watch: bool,
    reload: bool,
    static_check: bool,
    /// The whole program, services and all, was checked before `--split` started them: the
    /// first load does not check it again (FEEDBACK40.md, Q-473; branch feedback40-43).
    checked: bool,
    register: Option<String>,
    service: Option<String>,
    max_body: usize,
    instances: usize,
    /// Where services run that do not run here (`--service-url name=url`).
    service_urls: Vec<(String, String)>,
    /// The path the program is served under (`--base /shop`), as a mounted one is.
    base: Option<String>,
    /// Each service its own process (`--split`).
    split: bool,
    /// Run as a service (`--as-service`): server fns only for other services.
    as_service: bool,
    /// The service this is, when not its file's name (`--service-name`).
    service_name: Option<String>,
    /// The `.env` to read, when not the one beside the program (`--env-file`).
    env_file: Option<String>,
    /// Copies of a service under `--split` (`--scale risk=3`).
    scale: Vec<(String, usize)>,
    /// The app this copy of a service joins (`--join http://app:8080`).
    join: Option<String>,
    /// Commands run beside the program and stopped with it (`--with`).
    with: Vec<String>,
    /// The program's tools, when `--split` checked it before its services started (FEEDBACK56.md,
    /// C.2; branch feedback56).
    tools: Option<Vec<quillo::mcp::Tool>>,
}

fn run_options(flags: &[String]) -> Result<RunOptions, i32> {
    let mut o = RunOptions {
        host: None,
        port: None,
        listen: None,
        socket_mode: 0o600,
        policies: Vec::new(),
        workers: 64,
        watch: false,
        reload: false,
        static_check: true,
        checked: false,
        register: None,
        service: None,
        max_body: quillo::server::MAX_BODY,
        instances: 1,
        service_urls: Vec::new(),
        base: None,
        split: false,
        as_service: false,
        service_name: None,
        env_file: None,
        scale: Vec::new(),
        join: None,
        with: Vec::new(),
        tools: None,
    };
    let mut tls: (Option<PathBuf>, Option<PathBuf>) = (None, None);
    let mut i = 0;
    while i < flags.len() {
        let value = flags.get(i + 1);
        match (flags[i].as_str(), value) {
            ("--watch" | "-w", _) => {
                o.watch = true;
                i += 1;
                continue;
            }
            ("--reload", _) => {
                o.reload = true;
                i += 1;
                continue;
            }
            ("--no-check", _) => {
                o.static_check = false;
                i += 1;
                continue;
            }
            ("--split", _) => {
                o.split = true;
                i += 1;
                continue;
            }
            ("--as-service", _) => {
                o.as_service = true;
                i += 1;
                continue;
            }
            ("--no-build", _) => {
                quillo::native::NO_BUILD.store(true, std::sync::atomic::Ordering::Relaxed);
                i += 1;
                continue;
            }
            // Given by `--split` to the services it starts, once it has checked them itself.
            ("--checked", _) => {
                o.checked = true;
                i += 1;
                continue;
            }
            ("--service-url", Some(v)) => match v.split_once('=') {
                Some((name, url)) if !name.is_empty() && (url.starts_with("http://") || url.starts_with("https://")) => {
                    o.service_urls.push((name.to_string(), url.to_string()))
                }
                _ => {
                    eprintln!("quillo: --service-url is name=http://host:port (billing=http://10.0.0.5:8081)");
                    return Err(2);
                }
            },
            ("--service-name", Some(v)) => o.service_name = Some(v.clone()),
            ("--env-file", Some(v)) => o.env_file = Some(v.clone()),
            ("--with", Some(v)) if !v.trim().is_empty() => o.with.push(v.clone()),
            ("--scale", Some(v)) => match v.split_once('=').map(|(n, c)| (n, c.parse::<usize>())) {
                Some((name, Ok(n))) if !name.is_empty() && (1..=64).contains(&n) => o.scale.push((name.to_string(), n)),
                _ => {
                    eprintln!("quillo: --scale is a service and how many copies of it, 1 to 64: --scale risk=3");
                    return Err(2);
                }
            },
            ("--join", Some(v)) if v.starts_with("http://") || v.starts_with("https://") => o.join = Some(v.trim_end_matches('/').to_string()),
            ("--join", Some(_)) => {
                eprintln!("quillo: --join is the address of the app to join: --join http://app:8080");
                return Err(2);
            }
            ("--base", Some(v)) => {
                let b = v.trim_end_matches('/');
                if !b.starts_with('/') || b.contains("//") || b.contains('?') {
                    eprintln!("quillo: --base is a path such as /shop");
                    return Err(2);
                }
                o.base = Some(b.to_string());
            }
            ("--no-wt", _) => {
                quillo::wt::DISABLED.store(true, std::sync::atomic::Ordering::Relaxed);
                i += 1;
                continue;
            }
            ("--wt-port", Some(v)) => match v.parse::<u16>() {
                Ok(n) if n > 0 => {
                    quillo::wt::PORT.store(n, std::sync::atomic::Ordering::Relaxed);
                }
                _ => {
                    eprintln!("quillo: --wt-port needs a port number");
                    return Err(2);
                }
            },
            // The last given, as for every option, and the one checked (FEEDBACK55.md, Q-852;
            // branch feedback55).
            ("--tls-cert", Some(v)) => tls.0 = Some(PathBuf::from(v)),
            ("--tls-key", Some(v)) => tls.1 = Some(PathBuf::from(v)),
            ("--no-prefetch", _) => {
                quillo::server::PREFETCH.store(false, std::sync::atomic::Ordering::Relaxed);
                i += 1;
                continue;
            }
            // FEEDBACK13.md, Q-089; branch feedback13-fixes.
            ("--plain-http", _) => {
                quillo::session::PLAIN_HTTP_ASKED.store(true, std::sync::atomic::Ordering::Relaxed);
                i += 1;
                continue;
            }
            ("--no-view-cache", _) => {
                quillo::viewcache::DISABLED.store(true, std::sync::atomic::Ordering::Relaxed);
                i += 1;
                continue;
            }
            ("--no-jobs", _) => {
                quillo::jobs::DISABLED.store(true, std::sync::atomic::Ordering::Relaxed);
                i += 1;
                continue;
            }
            ("--describe", _) => {
                quillo::describe::ENABLED.store(true, std::sync::atomic::Ordering::Relaxed);
                i += 1;
                continue;
            }
            ("--query-stats", _) => {
                quillo::querystats::ENABLED.store(true, std::sync::atomic::Ordering::Relaxed);
                i += 1;
                continue;
            }
            ("--metrics", _) => {
                quillo::metrics::ENABLED.store(true, std::sync::atomic::Ordering::Relaxed);
                quillo::metrics::start();
                i += 1;
                continue;
            }
            ("--log", Some(v)) => {
                let format = match v.as_str() {
                    "text" => quillo::metrics::TEXT,
                    "json" => quillo::metrics::JSON,
                    "off" => quillo::metrics::OFF,
                    _ => {
                        eprintln!("quillo: --log is text, json or off");
                        return Err(2);
                    }
                };
                quillo::metrics::LOG.store(format, std::sync::atomic::Ordering::Relaxed);
            }
            // `localhost` is both loopbacks to whoever calls it, and its first address is `::1` on
            // macOS: the one the guide and the tools call, 127.0.0.1, is listened on
            // (FEEDBACK31.md, Q-384; branch feedback27-35-fixes).
            ("--host", Some(v)) if v.eq_ignore_ascii_case("localhost") => o.host = Some("127.0.0.1".into()),
            ("--host", Some(v)) => o.host = Some(v.clone()),
            ("--listen", Some(v)) => match v.strip_prefix("unix:").filter(|p| !p.is_empty()) {
                Some(p) => o.listen = Some(PathBuf::from(p)),
                None => {
                    eprintln!("quillo: --listen takes unix:PATH, a socket to make (for a TCP port, --host and --port), got {:?}", v);
                    return Err(2);
                }
            },
            ("--policy", Some(v)) => o.policies.push(PathBuf::from(v)),
            ("--socket-mode", Some(v)) => match u32::from_str_radix(v, 8) {
                Ok(m) if m <= 0o777 && m & 0o600 == 0o600 => o.socket_mode = m,
                _ => {
                    eprintln!("quillo: --socket-mode is octal permissions the owner can read and write, such as 600 or 660, got {:?}", v);
                    return Err(2);
                }
            },
            // A port is checked with the options, a usage mistake (FEEDBACK31.md, Q-381).
            ("--port", Some(v)) => match v.parse::<u16>() {
                Ok(_) => o.port = Some(v.clone()),
                Err(_) => {
                    eprintln!("quillo: --port is a number from 1 to 65535 (or 0 for any free one), got {:?}", v);
                    return Err(2);
                }
            },
            ("--register", Some(v)) => o.register = Some(v.clone()),
            ("--service", Some(v)) => o.service = Some(v.clone()),
            // `--max-body 50MB`, as the policy writes a size, or plain bytes.
            ("--max-body", Some(v)) => match quillo::policy::size(v) {
                Some(n) if n > 0 => o.max_body = n,
                _ => {
                    eprintln!("quillo: --max-body is a size such as 50MB, 512KB or 2GB (or a number of bytes), got {:?}", v);
                    return Err(2);
                }
            },
            ("--allow-path", Some(v)) => {
                if let Err(e) = quillo::builtins::allow_path(v) {
                    eprintln!("quillo: --allow-path needs a folder that is there: {}", e);
                    return Err(2);
                }
            }
            ("--max-compute", Some(v)) => match compute_limit(v) {
                Some(ms) => quillo::httpd::limit_compute(ms),
                None => {
                    eprintln!("quillo: --max-compute needs a length of time: 500ms, 2s or 1m");
                    return Err(2);
                }
            },
            // Which copies may join, and where a copy is called (FEEDBACK38.md, Q-459; branch
            // feedback36-39).
            ("--no-join", _) => {
                quillo::service::set_join_policy(Some(Vec::new()));
                i += 1;
                continue;
            }
            ("--allow-join", Some(v)) => {
                let names: Vec<String> = v.split(',').map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).collect();
                if names.is_empty() {
                    eprintln!("quillo: --allow-join names the services whose copies may join: --allow-join billing,risk");
                    return Err(2);
                }
                quillo::service::set_join_policy(Some(names));
            }
            ("--join-as", Some(v)) if v.starts_with("http://") || v.starts_with("https://") => quillo::service::join_as(v),
            ("--join-as", Some(_)) => {
                eprintln!("quillo: --join-as is the address this copy is called at: --join-as https://billing-3.internal:8443");
                return Err(2);
            }
            ("--trust-proxy", Some(v)) => {
                if !quillo::httpd::trust_proxies(v) {
                    eprintln!("quillo: --trust-proxy needs addresses or networks, as 127.0.0.1 or 10.0.0.0/8,192.168.0.0/16");
                    return Err(2);
                }
            }
            // Tool calls a minute each caller may make at the MCP door (FEEDBACK56.md, C.4, Q-860; branch
            // feedback56).
            ("--max-tool-calls", Some(v)) => match v.parse::<usize>() {
                Ok(n) if n > 0 => quillo::mcp::MAX_TOOL_CALLS.store(n, std::sync::atomic::Ordering::Relaxed),
                _ => {
                    eprintln!("quillo: --max-tool-calls needs a positive number: the tool calls a minute each caller may make at /_quillo/mcp");
                    return Err(2);
                }
            },
            ("--live-per-client", Some(v)) => match v.parse::<usize>() {
                Ok(n) if n > 0 => quillo::server::LIVE_PER_CLIENT.store(n, std::sync::atomic::Ordering::Relaxed),
                _ => {
                    eprintln!("quillo: --live-per-client needs a positive number");
                    return Err(2);
                }
            },
            ("--allowed-hosts", Some(v)) => {
                let names: Vec<String> = v.split(',').map(|h| h.trim().trim_end_matches('.').to_ascii_lowercase()).filter(|h| !h.is_empty()).collect();
                if names.is_empty() || quillo::server::ALLOWED_HOSTS.set(names).is_err() {
                    eprintln!("quillo: --allowed-hosts needs host names, as myapp.test or .myapp.test, once");
                    return Err(2);
                }
            }
            ("--max-connections", Some(v)) => match v.parse::<usize>() {
                Ok(n) if n > 0 => quillo::httpd::MAX_CONNECTIONS.store(n, std::sync::atomic::Ordering::SeqCst),
                _ => {
                    eprintln!("quillo: --max-connections needs a positive number");
                    return Err(2);
                }
            },
            ("--connections-per-client", Some(v)) => match v.parse::<usize>() {
                Ok(n) if n > 0 => quillo::httpd::CONNECTIONS_PER_CLIENT.store(n, std::sync::atomic::Ordering::SeqCst),
                _ => {
                    eprintln!("quillo: --connections-per-client needs a positive number");
                    return Err(2);
                }
            },
            ("--instances", Some(v)) => match v.parse::<usize>() {
                Ok(n) if n > 0 => {
                    // Each is a copy of the program and a thread: past the cores they only cost
                    // (FEEDBACK31.md, Q-381; branch feedback27-35-fixes).
                    let cores = std::thread::available_parallelism().map_or(1, |c| c.get());
                    if n > cores {
                        eprintln!("quillo: --instances {} is more than the {} cores here: each is a copy of the program, and they take turns", n, cores);
                    }
                    o.instances = n
                }
                _ => {
                    eprintln!("quillo: --instances needs a positive number");
                    return Err(2);
                }
            },
            ("--workers", Some(v)) => match v.parse::<usize>() {
                Ok(n) if n > 0 => o.workers = n,
                _ => {
                    eprintln!("quillo: --workers needs a positive number");
                    return Err(2);
                }
            },
            ("-h" | "--help", _) => {
                println!("{}", USAGE);
                return Err(0);
            }
            // Known, and with nothing after it (FEEDBACK16.md, Q-140; branch feedback20-fixes).
            (other, None) if VALUE_OPTIONS.contains(&other) => {
                eprintln!("quillo: `{}` needs a value", other);
                return Err(2);
            }
            ("--with", Some(_)) => {
                eprintln!("quillo: --with needs a command to run");
                return Err(2);
            }
            ("--open", Some(_)) => {
                eprintln!("quillo: --open is for `quillo app`, which opens a window on that page");
                return Err(2);
            }
            (other, _) if !other.starts_with('-') => {
                eprintln!("quillo: unknown argument `{}`: a program's own arguments go after `--`, as `quillo run tool.qwe -- {}`", other, other);
                return Err(2);
            }
            (other, _) => {
                eprintln!("quillo: unknown option `{}`{}", other, meant(other.split('=').next().unwrap_or(other)));
                return Err(2);
            }
        }
        i += 2;
    }
    if let Some(c) = tls.0 {
        let _ = quillo::wt::TLS_CERT.set(c);
    }
    if let Some(k) = tls.1 {
        let _ = quillo::wt::TLS_KEY.set(k);
    }
    if quillo::wt::TLS_CERT.get().is_some() != quillo::wt::TLS_KEY.get().is_some() {
        eprintln!("quillo: --tls-cert and --tls-key go together");
        return Err(2);
    }
    // The files are there, said with the other options (FEEDBACK31.md, Q-381; branch
    // feedback27-35-fixes).
    for (flag, f) in [("--tls-cert", quillo::wt::TLS_CERT.get()), ("--tls-key", quillo::wt::TLS_KEY.get())] {
        if let Some(f) = f.filter(|f| !f.is_file()) {
            eprintln!("quillo: {} {}: there is no such file", flag, f.display());
            return Err(2);
        }
    }
    if let Some(why) = quillo::wt::TLS_CERT.get().zip(quillo::wt::TLS_KEY.get()).and_then(|(c, k)| quillo::wt::tls_pair_problem(c, k)) {
        eprintln!("quillo: {}", why);
        return Err(2);
    }
    // The key is not served under any name, nor written over; the certificate is public
    // (FEEDBACK55.md, Q-701; branch feedback55).
    if let Some(key) = quillo::wt::TLS_KEY.get() {
        quillo::builtins::protect_named(key, quillo::builtins::OWN_TLS_KEY);
    }
    // Query stats: on in development, else when asked (branch query-stats).
    if o.watch || std::env::var("QUILLO_QUERY_STATS").is_ok_and(|v| v == "1" || v == "true") {
        quillo::querystats::ENABLED.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    Ok(o)
}

/// Checks and loads the program, ready to serve. `Ok(None)`: it ran and
/// declared no routes, so there is nothing to serve. `Err` is the exit code.
fn load(path: &Path, o: &RunOptions) -> Result<Option<App>, i32> {
    // Components in files of their own, for the browser's debugger.
    quillo::client::DEV.store(o.watch || quillo::debug::on(), std::sync::atomic::Ordering::Relaxed);
    let base = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    // Signed service calls name the service they are for (see `service::answer`).
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    quillo::service::serve_as(o.service_name.as_deref().unwrap_or(&stem));
    // The files read or tried, watched under --watch while the program does not load
    // (FEEDBACK55.md, Q-836; branch feedback55).
    let (checked, mut tried) = quillo::error::noting_reads(|| if o.static_check && !o.checked { static_errors(path).map(Some) } else { Ok(o.tools.clone()) });
    if let (Err(_), false) = (&checked, o.watch) {
        return Err(1);
    }
    // A service the app started apart: the app's folder (Q-807).
    if o.as_service {
        if let Some(dir) = std::env::var_os(quillo::privacy::ROOT_ENV).filter(|d| !d.is_empty()) {
            quillo::privacy::set_root(Path::new(&dir));
        }
    }
    let mut interp = Interp::new(base);
    let shared = interp.shared.clone();
    shared.service_urls.lock().unwrap_or_else(std::sync::PoisonError::into_inner).extend(o.service_urls.iter().cloned());
    interp.base = o.base.clone().unwrap_or_default();
    interp.is_service = o.as_service;
    let loaded = match checked.and_then(|tools| {
        let (r, read) = quillo::error::noting_reads(|| interp.load_file(path).map_err(|e| format!("{:#}", e)));
        tried.extend(read);
        r.map(|_| tools)
    }) {
        Ok(tools) => {
            tried.clear();
            // Its MCP door, when it has tools (FEEDBACK56.md, C.2; branch feedback56).
            quillo::mcp::open_door(&mut interp, path, tools);
            Loaded::Ready(interp)
        }
        Err(e) => {
            if !e.is_empty() {
                eprintln!("quillo: {}", e);
            }
            if !o.watch {
                return Err(1);
            }
            Loaded::Failed(if e.is_empty() { "the program has errors (see the terminal)".into() } else { e })
        }
    };
    if let Loaded::Ready(it) = &loaded {
        // A program with server fns and no routes may be a service: served.
        // So is one whose routes are its services' (`service /shop = ...`), or
        // that only listens, ticks or runs jobs (FEEDBACK12.md, Q-088; branch
        // feedback12-fixes).
        let site = &it.site;
        // And a service whose only offer is its tools, which its app's door calls (FEEDBACK56.md,
        // Part F, second review; branch feedback56).
        let serves = !it.routes.is_empty() || !it.server_fns.is_empty() || !it.mounts.is_empty() || !it.remote_mounts.is_empty()
            || !site.channels.is_empty() || !site.udp.is_empty() || !site.jobs.is_empty() || !site.ticks.is_empty() || it.offers_tools();
        if !serves && !o.watch {
            return Ok(None);
        }
    }
    let mut app = App::new(loaded, shared, path, base, o.watch);
    app.tried(tried);
    app.static_check = o.static_check;
    app.max_body = o.max_body;
    app.reload = o.reload;
    app.base = o.base.clone().unwrap_or_default();
    app.is_service = o.as_service;
    Ok(Some(app))
}

/// `--instances N`: the program N times over, one event loop each, so a
/// server uses N cores. Each instance has its own variables, as N
/// processes would; changes to the database and `publish` reach them all.
/// The first Ctrl-C or SIGTERM stops the server gracefully: it leaves the
/// pool it joined, finishes the requests in progress and exits; a second
/// one exits at once.
/// Commands run beside the program (`quillo run main.qwe --watch --with
/// "npx tailwindcss -i src/app.css -o public/app.css --watch"`): each in the
/// program's folder, in a process group of its own, its lines marked with
/// its first word; stopped, group and all, when the program stops
/// (FEEDBACK11.md, Q-079; branch feedback11-fixes).
mod with {
    use std::io::{BufRead, Read};
    use std::path::Path;
    use std::sync::Mutex;

    static RUNNING: Mutex<Vec<std::process::Child>> = Mutex::new(Vec::new());
    /// The pipe to each command's watcher, and the watcher, kept open while `quillo` runs.
    static WATCHED: Mutex<Vec<(std::process::ChildStdin, std::process::Child)>> = Mutex::new(Vec::new());

    pub fn start(cmd: &str, dir: &Path) -> Result<(), String> {
        let mut c = std::process::Command::new("sh");
        c.arg("-c").arg(cmd).current_dir(dir).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            c.process_group(0);
        }
        let mut child = c.spawn().map_err(|e| e.to_string())?;
        // Whatever way `quillo` ends, SIGKILL included, the pipe a watcher reads closes, and the
        // watcher ends the command's group (FEEDBACK31.md, Q-382; branch feedback27-35-fixes).
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            let watcher = std::process::Command::new("sh")
                .arg("-c")
                .arg(format!("read _; kill -TERM -{} 2>/dev/null", child.id()))
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .process_group(0)
                .spawn();
            if let Some((pipe, w)) = watcher.ok().and_then(|mut w| w.stdin.take().map(|p| (p, w))) {
                WATCHED.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push((pipe, w));
            }
        }
        let tag = cmd.split_whitespace().find(|w| !w.contains('=') && *w != "npx" && *w != "bunx").unwrap_or("with").rsplit('/').next().unwrap_or("with").to_string();
        eprintln!("quillo: started `{}` beside the program", cmd);
        for out in [child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>), child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>)].into_iter().flatten() {
            let tag = tag.clone();
            std::thread::spawn(move || {
                for l in std::io::BufReader::new(out).lines().map_while(Result::ok) {
                    eprintln!("[{}] {}", tag, l);
                }
            });
        }
        RUNNING.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(child);
        Ok(())
    }

    /// Stops each command and what it started.
    pub fn stop_all() {
        let mut running = RUNNING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        for child in running.iter_mut() {
            #[cfg(unix)]
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGTERM);
            }
            #[cfg(not(unix))]
            let _ = child.kill();
            let _ = child.wait();
        }
        running.clear();
        for (pipe, mut w) in WATCHED.lock().unwrap_or_else(std::sync::PoisonError::into_inner).drain(..) {
            drop(pipe);
            let _ = w.wait();
        }
    }
}

/// A length of time for `--max-compute`, in ms.
fn compute_limit(v: &str) -> Option<u64> {
    let (n, unit) = v.find(|c: char| !c.is_ascii_digit()).map_or((v, ""), |at| v.split_at(at));
    let n: u64 = n.parse().ok().filter(|n| *n > 0)?;
    n.checked_mul(match unit {
        "ms" => 1,
        "s" => 1000,
        "m" => 60_000,
        _ => return None,
    })
}

fn stop_on_signal(member: Option<quillo::cluster::Membership>) {
    stop_on_signal_with(member, None)
}

fn stop_on_signal_with(member: Option<quillo::cluster::Membership>, joining: Option<std::sync::Arc<quillo::service::Joining>>) {
    install_stop();
    let mut stop = STOP.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    stop.serving = true;
    stop.member = member;
    stop.joining = joining;
}

fn run_instances(path: &Path, mut o: RunOptions, addr: &str) -> i32 {
    if o.watch || o.register.is_some() {
        eprintln!("quillo: --instances does not go with --watch or --register");
        return 2;
    }
    let n = o.instances;
    quillo::pg::RUNTIME_THREADS.store((n / 2).max(2), std::sync::atomic::Ordering::Relaxed);
    let mut apps = Vec::with_capacity(n);
    for i in 0..n {
        match load(path, &o) {
            Ok(Some(app)) => apps.push(app),
            Ok(None) => return 0,
            Err(code) => return code,
        }
        // Checked once is enough.
        if i == 0 {
            o.static_check = false;
        }
    }
    let shared: Vec<_> = apps.iter().map(|a| a.shared()).collect();
    quillo::shared::Shared::link(&shared);
    stop_on_signal(None);
    match quillo::server::serve_many(apps, addr, o.workers) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("quillo: {}", e);
            1
        }
    }
}

/// Signing secrets too short to be safe are warned about.
fn warn_of_secrets() {
    for w in quillo::crypto::secret_problems(|k| std::env::var(k).ok()) {
        eprintln!("quillo: {}", w);
    }
}

/// What `run` and `app` do before the program: `app` used to skip it
/// (FEEDBACK16.md, Q-124; branch feedback20-fixes).
fn before_the_program(file: &str, o: &RunOptions) -> Result<(), i32> {
    // The program's `.env`, before anything reads the environment
    // (FEEDBACK9.md, Q-071; branch feedback9-fixes).
    let env_file = match &o.env_file {
        Some(f) => (std::path::PathBuf::from(f), true),
        None => (Path::new(file).parent().unwrap_or(Path::new(".")).join(".env"), false),
    };
    if let Err(e) = quillo::dotenv::load_into_process(&env_file.0, env_file.1) {
        eprintln!("quillo: {}", e);
        return Err(2);
    }
    // Known by identity, so a second hard link to one is not served (FEEDBACK55.md, Q-701; branch
    // feedback55).
    quillo::builtins::protect_settings(Path::new(file).parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new(".")), Some(&env_file.0));
    // What runs beside the program (FEEDBACK11.md, Q-079; branch
    // feedback11-fixes): started in its folder, stopped with it.
    let dir = Path::new(file).parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    for cmd in &o.with {
        if let Err(e) = with::start(cmd, dir) {
            eprintln!("quillo: --with {:?}: {}", cmd, e);
            return Err(1);
        }
    }
    warn_of_secrets();
    Ok(())
}

/// The way out of a stop: during the load, at once and with what runs beside the program;
/// while serving, once the requests are answered (FEEDBACK31.md, Q-382; branch
/// feedback27-35-fixes). Set once, before anything starts, as a handler can be set only once.
struct Stop {
    serving: bool,
    member: Option<quillo::cluster::Membership>,
    joining: Option<std::sync::Arc<quillo::service::Joining>>,
}

static STOP: std::sync::Mutex<Stop> = std::sync::Mutex::new(Stop { serving: false, member: None, joining: None });

fn install_stop() {
    let _ = ctrlc::set_handler(stop_asked);
}

/// Whether a stop was asked for already: the next one exits at once.
static STOP_AGAIN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// A stop asked for: by Ctrl-C or SIGTERM, or for a service `--split` started, by its app
/// ending (`stop_with_app`).
fn stop_asked() {
    let stop = STOP.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if STOP_AGAIN.swap(true, std::sync::atomic::Ordering::SeqCst) || !stop.serving {
        with::stop_all();
        std::process::exit(130);
    }
    // A message that cannot be written no longer panics (the `eprintln!` above), so the
    // handler goes on to stop the server (FEEDBACK37.md, Q-452; branch feedback36-39).
    eprintln!("quillo: stopping once the requests in progress are answered (again to stop now)");
    if let Some(m) = &stop.member {
        m.leave();
    }
    if let Some(j) = &stop.joining {
        j.leave();
    }
    with::stop_all();
    quillo::httpd::stop();
    // A request computing never comes back to its loop, so the time is kept from here
    // (FEEDBACK20.md, Q-164; branch feedback20-fixes).
    std::thread::spawn(|| {
        std::thread::sleep(quillo::httpd::DRAIN + std::time::Duration::from_millis(500));
        eprintln!("quillo: stopping with requests in progress: {}", quillo::httpd::in_progress().join(", "));
        std::process::exit(0);
    });
}

/// A service `--split` started stops when its app ends, whatever way it ends (SIGKILL and a
/// crash included): the app holds the other end of the service's stdin, which closes then. A
/// stop under way already goes on as it was asked (found in review of FEEDBACK55.md's Q-807;
/// branch feedback55).
fn stop_with_app() {
    if std::env::var_os(quillo::service::STOP_WITH_APP).is_none() {
        return;
    }
    std::env::remove_var(quillo::service::STOP_WITH_APP);
    std::thread::spawn(|| {
        use std::io::Read;
        let mut stdin = std::io::stdin().lock();
        let mut buf = [0u8; 64];
        loop {
            match stdin.read(&mut buf) {
                Ok(0) => break,
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        if !quillo::httpd::stopping() {
            stop_asked();
        }
    });
}

/// Runs the program. Whichever way it ends, a program that fails to load, has
/// nothing to serve or cannot listen included, what runs beside it stops
/// with it (FEEDBACK16.md, Q-123; branch feedback20-fixes).
/// `quillo fcgi`: the program answers FastCGI, not HTTP.
static FCGI: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn run(file: &str, flags: &[String]) -> i32 {
    let code = run_program(file, flags);
    with::stop_all();
    code
}

fn run_program(file: &str, flags: &[String]) -> i32 {
    let mut o = match run_options(flags) {
        Ok(o) => o,
        Err(code) => return code,
    };
    install_stop();
    stop_with_app();
    // What whoever runs it allows (FEEDBACK41.md, Q-478; branch feedback40-43).
    if let Err(code) = with_policy(Path::new(file), &o.policies, true) {
        return code;
    }
    {
        let p = quillo::policy::get();
        if let Some(w) = p.workers {
            o.workers = o.workers.min(w);
        }
        if let Some(b) = p.max_body {
            o.max_body = o.max_body.min(b);
        }
        // The policy narrows the option, as for the others (FEEDBACK56.md, C.4; branch feedback56).
        if let Some(n) = p.max_tool_calls {
            quillo::mcp::MAX_TOOL_CALLS.fetch_min(n, std::sync::atomic::Ordering::Relaxed);
        }
        if let Some(c) = p.max_compute {
            let (now, c) = (quillo::httpd::compute_limit_ms(), c.as_millis().max(1) as u64);
            if now == 0 || now > c {
                quillo::httpd::limit_compute(c);
                // Its message names the file (FEEDBACK55.md, Q-849; branch feedback55).
                if let Some(f) = &p.max_compute_from {
                    quillo::httpd::compute_limit_from(f);
                }
            }
        }
    }
    if let Err(code) = before_the_program(file, &o) {
        return code;
    }
    // The Rust files not built yet, in one cargo call before the program runs, rather than one
    // `use` after another (FEEDBACK37.md, Q-454; branch feedback36-39).
    if let Ok(files) = quillo::native::rust_files(Path::new(file)) {
        quillo::native::prebuild(&files);
    }
    let host = o.host.clone().unwrap_or_else(|| "127.0.0.1".into());
    let port = o.port.clone().unwrap_or_else(|| std::env::var("PORT").unwrap_or_else(|_| "8080".into()));
    // A process manager may pass a socket's path in $PORT.
    let listen = o.listen.clone().or_else(|| {
        let p = port.strip_prefix("unix:");
        (o.port.is_none() && (p.is_some() || port.starts_with('/'))).then(|| PathBuf::from(p.unwrap_or(&port)))
    });
    let fcgi = FCGI.load(std::sync::atomic::Ordering::Relaxed);
    // Nor one that tells others its port, which it does not listen on (`--join`).
    if (listen.is_some() || fcgi) && (o.split || o.instances > 1 || o.register.is_some() || o.join.is_some() || cfg!(not(unix))) {
        eprintln!("quillo: {} is for one process (not with --split, --instances, --register or --join), on Unix", if fcgi { "quillo fcgi" } else { "--listen unix:" });
        return 2;
    }
    if listen.as_ref().is_some_and(|l| l.as_os_str().len() <= 100) && !fcgi && !quillo::httpd::trusts_proxies() {
        eprintln!("quillo: every visitor through unix:{} is 127.0.0.1 to the program (one rate limit, one IP for all): give --trust-proxy 127.0.0.1 so that the web server's X-Forwarded-For is believed", listen.as_ref().unwrap().display());
    }
    let path = Path::new(file);
    // Announcements are signed with a key made from limer's secret: without
    // one, anyone who can reach a server could make its pages reload.
    let limer_secret = std::env::var("LIMER_ADMIN_SECRET").ok().filter(|s| !s.is_empty());
    if o.register.is_some() && limer_secret.is_none() {
        eprintln!("quillo: --register needs limer's admin secret in $LIMER_ADMIN_SECRET (and limer configured with it)");
        return 2;
    }
    if !o.scale.is_empty() && !o.split {
        eprintln!("quillo: --scale goes with --split: quillo run app.qwe --split --scale risk=3");
        return 2;
    }
    // Each service its own process: started first, told where the others are.
    let mut bound = None;
    let _split = if o.split {
        if o.instances > 1 || o.register.is_some() {
            eprintln!("quillo: --split does not go with --instances or --register");
            return 2;
        }
        // Where the app listens, for its services' channels (FEEDBACK12.md,
        // Q-087; branch open-fixes). `--port 0`: bound before they start, so that they are told
        // the port the system gave, not 0 (FEEDBACK55.md, Q-843; branch feedback55).
        let port = if port.parse::<u16>() == Ok(0) {
            let listener = match std::net::TcpListener::bind(format!("{}:{}", host, port)) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("quillo: cannot listen on {}:{}: {}", host, port, e);
                    return 1;
                }
            };
            let given = listener.local_addr().map_or_else(|_| port.clone(), |a| a.port().to_string());
            bound = Some(listener);
            given
        } else {
            port.clone()
        };
        let reach = if host == "0.0.0.0" || host == "::" { "127.0.0.1" } else { host.as_str() };
        let app_url = if reach.contains(':') { format!("http://[{}]:{}", reach, port) } else { format!("http://{}:{}", reach, port) };
        // The whole tree checked once, here, before any service starts: an error in a service is
        // found before anything waits for it, and each warning is said once, not once a level.
        // The services started skip their own check (FEEDBACK40.md, Q-473; branch feedback40-43).
        // A file that is not there is said as it was, by `split` (`cannot read ...`).
        if o.static_check && path.exists() {
            // Under --watch an error does not stop it: the app then checks again as it loads, to
            // show the error on its pages.
            match static_errors(path) {
                Ok(tools) => {
                    o.checked = true;
                    o.tools = Some(tools);
                }
                Err(_) => o.checked = false,
            }
            if !o.checked && !o.watch {
                return 1;
            }
        }
        // The services' processes count their private names' tags from the app's folder, as
        // they do in its process (FEEDBACK55.md, Q-807; branch feedback55).
        if let Some(dir) = path.canonicalize().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
            std::env::set_var(quillo::privacy::ROOT_ENV, dir);
        }
        match quillo::service::split(path, o.watch, o.reload, o.static_check, o.checked, &o.scale, &app_url) {
            Ok(s) => Some(s),
            Err(e) => {
                eprintln!("quillo: {}", e);
                return 1;
            }
        }
    } else {
        None
    };
    if o.instances > 1 {
        return run_instances(path, o, &format!("{}:{}", host, port));
    }
    let mut app = match load(path, &o) {
        Ok(Some(app)) => app,
        Ok(None) => return 0,
        Err(code) => return code,
    };
    let mut member = None;
    if let Some(admin) = o.register {
        let Ok(port) = port.parse::<u16>() else {
            eprintln!("quillo: --register needs a numeric --port");
            return 2;
        };
        let service = o.service.unwrap_or_else(|| {
            path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "app".into())
        });
        let Some(secret) = limer_secret else { return 2 };
        let m = quillo::cluster::Membership { admin, service, port, secret };
        member = Some(m.clone());
        app.cluster_key = Some(m.key());
        quillo::cluster::join(app.shared(), m);
    }
    // One more copy of a service, joining an app as it runs (FEEDBACK10.md,
    // Q-074; branch feedback10-fixes).
    let joining = match &o.join {
        Some(url) if o.as_service => {
            let Ok(port) = port.parse::<u16>() else {
                eprintln!("quillo: --join needs a numeric --port");
                return 2;
            };
            let service = o.service_name.clone().unwrap_or_else(|| path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
            Some(quillo::service::Joining::start(url.clone(), service, port))
        }
        Some(_) => {
            eprintln!("quillo: --join is for a copy of a service: quillo run risk.qwe --as-service --join http://app:8080");
            return 2;
        }
        None => None,
    };
    stop_on_signal_with(member, joining);
    #[cfg(unix)]
    if fcgi {
        let source = match (&listen, &o.port) {
            (Some(sock), _) => quillo::server::fcgi_unix(sock, o.socket_mode),
            (None, Some(_)) => std::net::TcpListener::bind(format!("{}:{}", host, port)).map(quillo::fcgi::Source::Tcp).map_err(|e| format!("cannot listen on {}:{}: {}", host, port, e)),
            (None, None) => quillo::fcgi::from_fd0(),
        };
        return match source.and_then(|s| quillo::server::serve_fcgi(app, s, listen.as_deref(), o.workers)) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("quillo: {}", e);
                1
            }
        };
    }
    #[cfg(unix)]
    if let Some(sock) = &listen {
        return match quillo::server::serve_unix(app, sock, o.socket_mode, o.workers) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("quillo: {}", e);
                1
            }
        };
    }
    let served = match bound {
        Some(listener) => quillo::server::serve_bound(app, listener, o.workers),
        None => quillo::server::serve(app, &format!("{}:{}", host, port), o.workers),
    };
    match served {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("quillo: {}", e);
            1
        }
    }
}

/// Prints the checker's findings; `Err` when there are errors. The error
/// text is empty because everything has been printed already.
/// Its tools as the checker read them, for the MCP door (FEEDBACK56.md, C.2; branch feedback56).
fn static_errors(path: &Path) -> Result<Vec<quillo::mcp::Tool>, String> {
    let analysis = quillo::check::check_file(path);
    for d in &analysis.diagnostics {
        eprintln!("{}", d);
    }
    if analysis.errors().next().is_some() {
        if !analysis.syntax_errors.is_empty() {
            eprintln!("note: {}", quillo::error::which_quillo());
        }
        return Err(String::new());
    }
    Ok(quillo::mcp::tools(&analysis))
}


/// Reads the policy that applies to `main` and puts it in force; `say` prints what it is.
fn with_policy(main: &Path, extra: &[PathBuf], say: bool) -> Result<(), i32> {
    match quillo::policy::load(main, extra) {
        Ok(p) => {
            if say && !p.files.is_empty() {
                eprintln!("quillo: {}", p.summary());
            }
            if !p.build {
                quillo::native::NO_BUILD.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            quillo::policy::set(p);
            Ok(())
        }
        Err(e) => {
            eprintln!("quillo: {}", e);
            Err(2)
        }
    }
}

fn check(file: &str, opts: quillo::check::Options) -> i32 {
    let analysis = quillo::check::check_file_opts(Path::new(file), Default::default(), opts);
    for d in &analysis.diagnostics {
        eprintln!("{}", d);
    }
    let errors = analysis.errors().count();
    let warnings = analysis.diagnostics.len() - errors;
    if errors > 0 {
        if !analysis.syntax_errors.is_empty() {
            eprintln!("note: {}", quillo::error::which_quillo());
        }
        eprintln!("quillo: {} error{}{}", errors, if errors == 1 { "" } else { "s" }, plural_warnings(warnings));
        return 1;
    }
    // The whole program: the file and those it `use`s.
    let count = |k: SymbolKind| analysis.symbols.iter().filter(|s| s.kind == k).count();
    let files = analysis.symbols.iter().map(|s| s.file.as_str()).chain([file]).collect::<std::collections::HashSet<_>>().len();
    // "1 route", not "1 routes" (FEEDBACK45.md, Q-528; branch feedback40-43).
    let some = |n: usize, what: &str| format!("{} {}{}", n, what, if n == 1 { "" } else { "s" });
    println!(
        "{}: ok ({}, {}, {}{}){}",
        file,
        some(count(SymbolKind::Route), "route"),
        some(count(SymbolKind::Function) + count(SymbolKind::ServerFn) + count(SymbolKind::View), "function"),
        some(count(SymbolKind::Component), "component"),
        if files > 1 { format!(", in {} files", files) } else { String::new() },
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

fn repl(prompts: bool) -> i32 {
    let mut interp = Interp::new(".");
    let stdin = io::stdin();
    let mut buf = String::new();
    let mut kept: Vec<Vec<Stmt>> = Vec::new();
    // From a pipe, the output is the answers alone, as Python's REPL does, and input that ends
    // with a statement open is an error (FEEDBACK31.md, Q-388; branch feedback27-35-fixes).
    #[cfg(unix)]
    let terminal = prompts || unsafe { libc::isatty(0) } == 1;
    #[cfg(not(unix))]
    let terminal = { let _ = prompts; true };
    if terminal {
        println!("Quillo {} — enter expressions; Ctrl-D exits", quillo::version());
        // The program here, which `quillo run` serves (FEEDBACK6.md, Q-059; branch feedback6-fixes).
        if Path::new("main.qwe").is_file() {
            println!("main.qwe is here: `quillo run` serves it");
        }
    }
    let mut line_no = 0usize;
    let mut began = 0usize;
    let mut failed = false;
    loop {
        if terminal {
            print!("{}", if buf.is_empty() { "quillo> " } else { "....> " });
            let _ = io::stdout().flush();
        }
        // Read as bytes: a line that is not UTF-8 is said and skipped, and the input goes on, where
        // it was taken for the end, with exit 0 (FEEDBACK55.md, Q-848; branch feedback55).
        let mut bytes = Vec::new();
        let read = stdin.lock().read_until(b'\n', &mut bytes).unwrap_or(0);
        let line = match String::from_utf8(bytes) {
            Ok(l) => l,
            Err(e) => {
                line_no += 1;
                let ok = &e.as_bytes()[..e.utf8_error().valid_up_to()];
                let col = String::from_utf8_lossy(ok).chars().count() + 1;
                eprintln!("error: line {} is not UTF-8 text (byte 0x{:02X} at column {}): skipped", line_no, e.as_bytes()[ok.len()], col);
                failed = true;
                continue;
            }
        };
        if read == 0 {
            if !buf.trim().is_empty() {
                let what = Parser::new(&buf).parse_program().err().map_or_else(String::new, |e| format!(": {}", e.msg));
                eprintln!("error: the input ended inside the statement that began on line {}{}", began, what);
                return 1;
            }
            if terminal {
                println!();
            }
            return if failed && !terminal { 1 } else { 0 };
        }
        line_no += 1;
        if buf.is_empty() {
            began = line_no;
        }
        buf.push_str(&line);
        let stmts = match Parser::new(&buf).parse_program() {
            Ok(s) => s,
            Err(e) if e.msg.contains("end of input") => continue,
            Err(e) => {
                eprintln!("error: {}", e);
                failed = true;
                buf.clear();
                continue;
            }
        };
        buf.clear();
        let globals = interp.globals.clone();
        // Kept for the session: a compiled loop's code goes by where its loop is (src/jitloop.rs).
        kept.push(stmts);
        let stmts = kept.last().expect("just pushed");
        for s in stmts {
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
                    failed = true;
                    break;
                }
            }
        }
    }
}
