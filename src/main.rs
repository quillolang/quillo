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

const USAGE: &str = "\
Quillo — a small web language

usage:
  quillo run <file.qwe> [options]   run a program; serve it if it has routes
  quillo app <file.qwe> [options]   run a program in a native window (needs a
                                    build with `--features app`)
  quillo check <file.qwe>           check a program without running it
               [--strict-null]      and warn where a value may be null and
                                    null does not work
  quillo docs <file.qwe>            the program's API as OpenAPI 3: its routes,
                                    and its server fns' contracts
  quillo build <file.qwe>           build the Rust (.qwer files) a program uses,
                                    into .quillo/native/ next to them, so it
                                    runs where Rust is not installed
  quillo repl                       interactive prompt
  quillo lsp                        language server for editors (stdio)
  quillo dap                        debugger for editors (the Debug Adapter
                                    Protocol, on stdio): breakpoints, stepping,
                                    variables
  quillo <file.qwe>                 same as `quillo run`

The program is the first argument that is not an option, wherever the options
stand. Left out, or a folder, it is that folder's main.qwe: `quillo run` in a
program's folder runs it. An option's value follows it, or `=`: `--port 8081`
or `--port=8081`.

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
  --describe     answer GET /_quillo/describe with the program's API (as
                 `quillo docs` writes it)
  --scale NAME=N with --split: run service NAME as N copies, taking calls in
                 turn (they share its database)
  --with CMD     run CMD beside the program, in its folder, its lines marked,
                 stopped with it (`--with \"npx tailwindcss -i src/app.css -o
                 public/app.css --watch\"`); may be given more than once
  --join URL     with --as-service: join the app at URL as one more copy of
                 this service, while it runs; leaves when stopped
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
                 object per line: time, method, path, status, ms, ip, tenant,
                 rid) or off
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

/// Options of `run`, `app` and `build` that take a value: the word after one
/// is its value, not the program.
const VALUE_OPTIONS: &[&str] = &[
    "--host", "--port", "--workers", "--register", "--service", "--service-url", "--service-name", "--base",
    "--max-body", "--instances", "--wt-port", "--tls-cert", "--tls-key", "--log", "--open", "--env-file",
    "--scale", "--join", "--with",
];

/// Options of `run`, `app`, `build` and `check` that take no value.
const FLAG_OPTIONS: &[&str] = &[
    "--watch", "--reload", "--no-check", "--split", "--as-service", "--no-wt", "--no-prefetch", "--no-view-cache",
    "--no-jobs", "--metrics", "--strict-null", "--describe",
];

/// What an unknown option was probably meant to be: the nearest known one
/// (FEEDBACK9.md, Q-070; branch feedback9-fixes).
fn meant(option: &str) -> String {
    if option == "-p" {
        return " (the port is `--port`)".to_string();
    }
    let near = VALUE_OPTIONS
        .iter()
        .chain(FLAG_OPTIONS)
        .map(|o| (quillo::check::edit_distance(option, o), *o))
        .filter(|(d, o)| *d <= 2.max(o.len() / 4))
        .min();
    near.map_or_else(String::new, |(_, o)| format!(" (did you mean `{}`?)", o))
}

/// The program a command names and the options around it, wherever they
/// stand: the first argument that is neither an option nor an option's
/// value. A folder, or no program at all, is that folder's main.qwe, and
/// nothing else (FEEDBACK6.md, Q-058 and Q-059; branch feedback6-fixes).
fn program_and_flags(args: &[String]) -> Result<(String, Vec<String>), String> {
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
    let named = program.unwrap_or_else(|| ".".to_string());
    if Path::new(&named).is_dir() {
        let main = Path::new(&named).join("main.qwe");
        if !main.is_file() {
            return Err(if named == "." {
                "no program: name a .qwe file, or add a main.qwe here".to_string()
            } else {
                format!("no program: {} has no main.qwe; name a .qwe file", named)
            });
        }
        return Ok((if named == "." { "main.qwe".to_string() } else { main.to_string_lossy().into_owned() }, flags));
    }
    Ok((named, flags))
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

fn real_main(args: Vec<String>) -> i32 {
    match args.get(1).map(String::as_str) {
        Some("run") => with_program(&args[2..], run),
        // A debug adapter for editors, on stdin and stdout (src/debug.rs).
        Some("dap") => quillo::debug::serve(|l| run(&l.program, &l.args)),
        Some("check") => {
            let strict_null = args.iter().any(|a| a == "--strict-null");
            with_program(&args[2..], |f, _| check(f, quillo::check::Options { strict_null, css: true, ..Default::default() }))
        }
        // The program's API as OpenAPI, from its source (FEEDBACK10.md, Q-077;
        // branch feedback10-fixes).
        Some("docs") => with_program(&args[2..], |f, _| match quillo::describe::of_source(Path::new(f)) {
            Ok(doc) => {
                println!("{}", serde_json::to_string_pretty(&doc).unwrap_or_default());
                0
            }
            Err(e) => {
                eprintln!("quillo: {}", e);
                1
            }
        }),
        Some("build") => with_program(&args[2..], |f, _| match quillo::native::build_all(Path::new(f)) {
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
        Some("repl") | None => repl(),
        Some("lsp") => quillo::lsp::Server::new().run(),
        Some("version" | "--version" | "-V") => {
            println!("quillo {}", quillo::version());
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
    with_program(&args[2..], desktop::main)
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
    /// The `.env` to read, when not the one beside the program (`--env-file`).
    env_file: Option<String>,
    /// Copies of a service under `--split` (`--scale risk=3`).
    scale: Vec<(String, usize)>,
    /// The app this copy of a service joins (`--join http://app:8080`).
    join: Option<String>,
    /// Commands run beside the program and stopped with it (`--with`).
    with: Vec<String>,
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
        env_file: None,
        scale: Vec::new(),
        join: None,
        with: Vec::new(),
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
            ("--describe", _) => {
                quillo::describe::ENABLED.store(true, std::sync::atomic::Ordering::Relaxed);
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
            ("-h" | "--help", _) => {
                println!("{}", USAGE);
                return Err(0);
            }
            (other, _) => {
                eprintln!("quillo: unknown option `{}`{}", other, meant(other.split('=').next().unwrap_or(other)));
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
    // Components in files of their own, for the browser's debugger.
    quillo::client::DEV.store(o.watch || quillo::debug::on(), std::sync::atomic::Ordering::Relaxed);
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
        // So is one whose routes are its services' (`service /shop = ...`), or
        // that only listens, ticks or runs jobs (FEEDBACK12.md, Q-088; branch
        // feedback12-fixes).
        let site = &it.site;
        let serves = !it.routes.is_empty() || !it.server_fns.is_empty() || !it.mounts.is_empty() || !it.remote_mounts.is_empty()
            || !site.channels.is_empty() || !site.udp.is_empty() || !site.jobs.is_empty() || !site.ticks.is_empty();
        if !serves && !o.watch {
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

    pub fn start(cmd: &str, dir: &Path) -> Result<(), String> {
        let mut c = std::process::Command::new("sh");
        c.arg("-c").arg(cmd).current_dir(dir).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            c.process_group(0);
        }
        let mut child = c.spawn().map_err(|e| e.to_string())?;
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
        RUNNING.lock().unwrap_or_else(|e| e.into_inner()).push(child);
        Ok(())
    }

    /// Stops each command and what it started.
    pub fn stop_all() {
        let mut running = RUNNING.lock().unwrap_or_else(|e| e.into_inner());
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
    }
}

fn stop_on_signal(member: Option<quillo::cluster::Membership>) {
    stop_on_signal_with(member, None)
}

fn stop_on_signal_with(member: Option<quillo::cluster::Membership>, joining: Option<std::sync::Arc<quillo::service::Joining>>) {
    let again = std::sync::atomic::AtomicBool::new(false);
    let _ = ctrlc::set_handler(move || {
        if again.swap(true, std::sync::atomic::Ordering::SeqCst) {
            std::process::exit(130);
        }
        eprintln!("quillo: stopping once the requests in progress are answered (again to stop now)");
        if let Some(m) = &member {
            m.leave();
        }
        if let Some(j) = &joining {
            j.leave();
        }
        with::stop_all();
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

/// Signing secrets too short to be safe are warned about.
fn warn_of_secrets() {
    for w in quillo::crypto::secret_problems(|k| std::env::var(k).ok()) {
        eprintln!("quillo: {}", w);
    }
}

fn run(file: &str, flags: &[String]) -> i32 {
    let o = match run_options(flags) {
        Ok(o) => o,
        Err(code) => return code,
    };
    // The program's `.env`, before anything reads the environment
    // (FEEDBACK9.md, Q-071; branch feedback9-fixes).
    let env_file = match &o.env_file {
        Some(f) => (std::path::PathBuf::from(f), true),
        None => (Path::new(file).parent().unwrap_or(Path::new(".")).join(".env"), false),
    };
    if let Err(e) = quillo::dotenv::load_into_process(&env_file.0, env_file.1) {
        eprintln!("quillo: {}", e);
        return 2;
    }
    // What runs beside the program (FEEDBACK11.md, Q-079; branch
    // feedback11-fixes): started in its folder, stopped with it.
    let dir = Path::new(file).parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    for cmd in &o.with {
        if let Err(e) = with::start(cmd, dir) {
            eprintln!("quillo: --with {:?}: {}", cmd, e);
            with::stop_all();
            return 1;
        }
    }
    warn_of_secrets();
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
    if !o.scale.is_empty() && !o.split {
        eprintln!("quillo: --scale goes with --split: quillo run app.qwe --split --scale risk=3");
        return 2;
    }
    // Each service its own process: started first, told where the others are.
    let _split = if o.split {
        if o.instances > 1 || o.register.is_some() {
            eprintln!("quillo: --split does not go with --instances or --register");
            return 2;
        }
        match quillo::service::split(path, o.watch, o.static_check, &o.scale) {
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
        if !analysis.syntax_errors.is_empty() {
            eprintln!("note: {}", quillo::error::which_quillo());
        }
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
        if !analysis.syntax_errors.is_empty() {
            eprintln!("note: {}", quillo::error::which_quillo());
        }
        eprintln!("quillo: {} error{}{}", errors, if errors == 1 { "" } else { "s" }, plural_warnings(warnings));
        return 1;
    }
    // The whole program: the file and those it `use`s.
    let count = |k: SymbolKind| analysis.symbols.iter().filter(|s| s.kind == k).count();
    let files = analysis.symbols.iter().map(|s| s.file.as_str()).chain([file]).collect::<std::collections::HashSet<_>>().len();
    println!(
        "{}: ok ({} routes, {} functions, {} components{}){}",
        file,
        count(SymbolKind::Route),
        count(SymbolKind::Function) + count(SymbolKind::ServerFn) + count(SymbolKind::View),
        count(SymbolKind::Component),
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

fn repl() -> i32 {
    let mut interp = Interp::new(".");
    let stdin = io::stdin();
    let mut buf = String::new();
    println!("Quillo {} — enter expressions; Ctrl-D exits", quillo::version());
    // The program here, which `quillo run` serves (FEEDBACK6.md, Q-059; branch feedback6-fixes).
    if Path::new("main.qwe").is_file() {
        println!("main.qwe is here: `quillo run` serves it");
    }
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
