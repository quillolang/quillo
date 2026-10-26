use std::io::{self, BufRead, Write};
use std::path::Path;
use quillo::ast::Stmt;
use quillo::interp::Ctrl;
use quillo::check::SymbolKind;
use quillo::parser::Parser;
use quillo::server::{App, Loaded};
use quillo::{Interp, Value};

#[cfg(feature = "app")]
mod desktop;

const VERSION: &str = env!("CARGO_PKG_VERSION");

const USAGE: &str = "\
Quillo — a small web language

usage:
  quillo run <file.qwe> [options]   run a program; serve it if it has routes
  quillo app <file.qwe> [options]   run a program in a native window (needs a
                                    build with `--features app`)
  quillo check <file.qwe>           check a program without running it
               [--strict-null]      and warn where a value may be null and
                                    null does not work
  quillo build <file.qwe>           build the Rust (.qwer files) a program uses,
                                    into .quillo/native/ next to them, so it
                                    runs where Rust is not installed
  quillo repl                       interactive prompt
  quillo lsp                        language server for editors (stdio)
  quillo <file.qwe>                 same as `quillo run`

options for `run`:
  --port N       port to listen on (default $PORT, then 8080)
  --host H       address to listen on (default 127.0.0.1)
  --instances N  run the program N times over, each on a core of its own
                 (separate variables, as N processes would have; the
                 database and `publish` changes reach them all)
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
                 QUILLO_SERVICE_NAME); calls are signed with
                 QUILLO_SERVICE_SECRET, the same on every service
  --base PATH    the path the program is served under (/shop), as a mounted
                 program is: its links and redirects carry it
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
  --max-body N   largest request body to accept, in bytes (default 10 MiB)
  --wt-port N    the UDP port pages connect to for channels over WebTransport
                 (default: the HTTP port's number)
  --tls-cert F, --tls-key F
                 the certificate and key (PEM) WebTransport presents; without
                 them it makes its own, which pages take by its hash
  --no-wt        no WebTransport: pages' channels use a WebSocket
  --log F        the request log on stderr: text (the default), json (a JSON
                 object per line: time, method, path, status, ms, ip, tenant)
                 or off
  --metrics      answer GET /_quillo/metrics: request counts and times, open
                 connections and uptime, in Prometheus's text format
  --register URL join limer's `discover` pool through its admin API (such as
                 http://127.0.0.1:2019), and share live updates with the other
                 servers there; limer's admin secret must be in
                 $LIMER_ADMIN_SECRET
  --service S    the pool's name (default: the file's name, without .qwe)

`app` takes the options of `run` except --host, --register and --service; its
port, by default one the system picks, answers only the window. `--open PATH`
is the page the window opens on (default /).";

// The system's allocator on the event loops, mimalloc on the threads that
// wait for them (see quillo::alloc).
#[global_allocator]
static ALLOC: quillo::alloc::Split = quillo::alloc::Split;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // The window must run on the main thread (macOS requires it); the
    // program gets a thread of its own.
    if args.get(1).map(String::as_str) == Some("app") {
        std::process::exit(app(&args));
    }
    // Run on a thread with a roomy stack (see WORKER_STACK).
    let child = std::thread::Builder::new()
        .stack_size(quillo::server::WORKER_STACK)
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
        Some("check") => {
            let strict_null = args.iter().any(|a| a == "--strict-null");
            match args.iter().skip(2).find(|a| !a.starts_with("--")) {
                Some(f) => check(f, quillo::check::Options { strict_null, ..Default::default() }),
                None => usage(),
            }
        }
        Some("build") => match args.get(2) {
            Some(f) => match quillo::native::build_all(Path::new(f)) {
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
            },
            None => usage(),
        },
        Some("repl") | None => repl(),
        Some("lsp") => quillo::lsp::Server::new().run(),
        Some("version" | "--version" | "-V") => {
            println!("quillo {}", VERSION);
            0
        }
        Some("help" | "--help" | "-h") => {
            println!("{}", USAGE);
            0
        }
        Some(f) if f.ends_with(".qwe") => run(f, &args[2..]),
        Some(_) => usage(),
    }
}

#[cfg(feature = "app")]
fn app(args: &[String]) -> i32 {
    match args.get(2) {
        Some(f) => desktop::main(f, &args[3..]),
        None => usage(),
    }
}

#[cfg(not(feature = "app"))]
fn app(_: &[String]) -> i32 {
    eprintln!("quillo: this quillo was built without windows; build it with `cargo build --release --features app`");
    2
}

fn usage() -> i32 {
    eprintln!("{}", USAGE);
    2
}

/// The options of `run` (and of `app`, which takes some of them).
struct RunOptions {
    host: Option<String>,
    port: Option<String>,
    workers: usize,
    watch: bool,
    reload: bool,
    static_check: bool,
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
}

fn run_options(flags: &[String]) -> Result<RunOptions, i32> {
    let mut o = RunOptions {
        host: None,
        port: None,
        workers: 64,
        watch: false,
        reload: false,
        static_check: true,
        register: None,
        service: None,
        max_body: quillo::server::MAX_BODY,
        instances: 1,
        service_urls: Vec::new(),
        base: None,
        split: false,
        as_service: false,
        service_name: None,
    };
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
            ("--tls-cert", Some(v)) => {
                let _ = quillo::wt::TLS_CERT.set(v.into());
            }
            ("--tls-key", Some(v)) => {
                let _ = quillo::wt::TLS_KEY.set(v.into());
            }
            ("--no-prefetch", _) => {
                quillo::server::PREFETCH.store(false, std::sync::atomic::Ordering::Relaxed);
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
            ("--host", Some(v)) => o.host = Some(v.clone()),
            ("--port", Some(v)) => o.port = Some(v.clone()),
            ("--register", Some(v)) => o.register = Some(v.clone()),
            ("--service", Some(v)) => o.service = Some(v.clone()),
            ("--max-body", Some(v)) => match v.parse::<usize>() {
                Ok(n) if n > 0 => o.max_body = n,
                _ => {
                    eprintln!("quillo: --max-body needs a positive number of bytes");
                    return Err(2);
                }
            },
            ("--instances", Some(v)) => match v.parse::<usize>() {
                Ok(n) if n > 0 => o.instances = n,
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
            (other, _) => {
                eprintln!("quillo: unknown option `{}`", other);
                return Err(2);
            }
        }
        i += 2;
    }
    if quillo::wt::TLS_CERT.get().is_some() != quillo::wt::TLS_KEY.get().is_some() {
        eprintln!("quillo: --tls-cert and --tls-key go together");
        return Err(2);
    }
    Ok(o)
}

/// Checks and loads the program, ready to serve. `Ok(None)`: it ran and
/// declared no routes, so there is nothing to serve. `Err` is the exit code.
fn load(path: &Path, o: &RunOptions) -> Result<Option<App>, i32> {
    let base = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    // Signed service calls name the service they are for (see `service::answer`).
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    quillo::service::serve_as(o.service_name.as_deref().unwrap_or(&stem));
    let checked = if o.static_check { static_errors(path) } else { Ok(()) };
    if let (Err(_), false) = (&checked, o.watch) {
        return Err(1);
    }
    let mut interp = Interp::new(base);
    let shared = interp.shared.clone();
    shared.service_urls.lock().unwrap_or_else(|e| e.into_inner()).extend(o.service_urls.iter().cloned());
    interp.base = o.base.clone().unwrap_or_default();
    interp.is_service = o.as_service;
    let loaded = match checked.and_then(|()| interp.load_file(path).map_err(|e| e.to_string())) {
        Ok(_) => Loaded::Ready(interp),
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
        if it.routes.is_empty() && it.server_fns.is_empty() && !o.watch {
            return Ok(None);
        }
    }
    let mut app = App::new(loaded, shared, path, base, o.watch);
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
fn stop_on_signal(member: Option<quillo::cluster::Membership>) {
    let again = std::sync::atomic::AtomicBool::new(false);
    let _ = ctrlc::set_handler(move || {
        if again.swap(true, std::sync::atomic::Ordering::SeqCst) {
            std::process::exit(130);
        }
        eprintln!("quillo: stopping once the requests in progress are answered (again to stop now)");
        if let Some(m) = &member {
            m.leave();
        }
        quillo::httpd::stop();
    });
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

fn run(file: &str, flags: &[String]) -> i32 {
    let o = match run_options(flags) {
        Ok(o) => o,
        Err(code) => return code,
    };
    let host = o.host.clone().unwrap_or_else(|| "127.0.0.1".into());
    let port = o.port.clone().unwrap_or_else(|| std::env::var("PORT").unwrap_or_else(|_| "8080".into()));
    let path = Path::new(file);
    // Announcements are signed with a key made from limer's secret: without
    // one, anyone who can reach a server could make its pages reload.
    let limer_secret = std::env::var("LIMER_ADMIN_SECRET").ok().filter(|s| !s.is_empty());
    if o.register.is_some() && limer_secret.is_none() {
        eprintln!("quillo: --register needs limer's admin secret in $LIMER_ADMIN_SECRET (and limer configured with it)");
        return 2;
    }
    // Each service its own process: started first, told where the others are.
    let _split = if o.split {
        if o.instances > 1 || o.register.is_some() {
            eprintln!("quillo: --split does not go with --instances or --register");
            return 2;
        }
        match quillo::service::split(path, o.watch, o.static_check) {
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
    stop_on_signal(member);
    match quillo::server::serve(app, &format!("{}:{}", host, port), o.workers) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("quillo: {}", e);
            1
        }
    }
}

/// Prints the checker's findings; `Err` when there are errors. The error
/// text is empty because everything has been printed already.
fn static_errors(path: &Path) -> Result<(), String> {
    let analysis = quillo::check::check_file(path);
    for d in &analysis.diagnostics {
        eprintln!("{}", d);
    }
    if analysis.errors().next().is_some() {
        return Err(String::new());
    }
    Ok(())
}

fn check(file: &str, opts: quillo::check::Options) -> i32 {
    let analysis = quillo::check::check_file_opts(Path::new(file), Default::default(), opts);
    for d in &analysis.diagnostics {
        eprintln!("{}", d);
    }
    let errors = analysis.errors().count();
    let warnings = analysis.diagnostics.len() - errors;
    if errors > 0 {
        eprintln!("quillo: {} error{}{}", errors, if errors == 1 { "" } else { "s" }, plural_warnings(warnings));
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
    println!("Quillo {} — enter expressions; Ctrl-D exits", VERSION);
    loop {
        print!("{}", if buf.is_empty() { "quillo> " } else { "....> " });
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
