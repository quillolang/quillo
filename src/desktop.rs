//! `quillo app`: the program in a native window.
//!
//! The program runs in this process with no socket (src/embed.rs): the window
//! asks for its pages through the `quillo` scheme, at an origin that stays the
//! same from one launch to the next, and nothing else can reach the program.
//! The window is the platform's own web view: WebKit on macOS and Linux,
//! WebView2 on Windows. Closing it ends the program.

use std::path::Path;
use tao::dpi::LogicalSize;
#[cfg(target_os = "macos")]
use tao::event::StartCause;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use wry::{NewWindowResponse, WebViewBuilder};

enum UserEvent {
    /// The page's `<title>` changed.
    Title(String),
    /// The page asked for a new window onto one of its own pages: it opens
    /// in this one instead, since only this window may reach the program.
    Open(String),
    /// What the program sends the page through the bridge (src/embed.rs).
    Eval(String),
}

pub fn main(file: &str, flags: &[String]) -> i32 {
    let code = window(file, flags);
    crate::with::stop_all();
    code
}

fn window(file: &str, flags: &[String]) -> i32 {
    // `--open /admin`: the page the window opens on (the program's own path).
    let mut open = "/".to_string();
    let mut rest = Vec::new();
    let mut it = flags.iter();
    while let Some(f) = it.next() {
        if f == "--open" {
            match it.next() {
                Some(p) if p.starts_with('/') && !p.starts_with("//") => open = p.clone(),
                _ => {
                    eprintln!("quillo: --open takes a path of the program, such as /admin");
                    return 2;
                }
            }
        } else {
            rest.push(f.clone());
        }
    }
    let flags = &rest[..];
    let o = match crate::run_options(flags) {
        Ok(o) => o,
        Err(code) => return code,
    };
    if o.host.is_some() || o.register.is_some() || o.service.is_some() {
        eprintln!("quillo: `app` serves its own window only; --host, --register and --service are for `run`");
        return 2;
    }
    // The window asks the program directly, with no socket (FEEDBACK39.md, Q-465): a port was
    // taken and ignored without a word (found measuring coverage; branch feedback40-43).
    if o.port.is_some() || o.listen.is_some() {
        eprintln!("quillo: `app` has no port: its window asks the program directly, with no socket; --port and --listen are for `run`");
        return 2;
    }
    if let Err(code) = crate::before_the_program(file, &o) {
        return code;
    }
    // Commands beside the program are in process groups of their own, so a
    // signal reaches them only from here (FEEDBACK16.md, Q-124; branch
    // feedback20-fixes).
    if !o.with.is_empty() {
        let _ = ctrlc::set_handler(|| {
            crate::with::stop_all();
            std::process::exit(130);
        });
    }
    quillo::platform::set_runtime("app");
    let path = Path::new(file).to_path_buf();
    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Quillo".into());
    let watch = o.watch;
    let program = match start_program(&path, o) {
        Ok(p) => p,
        Err(code) => return code,
    };
    if watch {
        program.watch();
    }
    match open_window(&name, program, &open, watch) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("quillo: cannot open the window: {}", e);
            1
        }
    }
}

/// The window's origin, the same at every launch, so the page's `localStorage` and IndexedDB stay
/// with it (FEEDBACK39.md, Q-466; branch feedback36-39): wry serves the scheme as
/// `https://quillo.localhost` on Windows.
#[cfg(target_os = "windows")]
const ORIGIN: &str = "https://quillo.localhost";
#[cfg(not(target_os = "windows"))]
const ORIGIN: &str = "quillo://app";

/// Loads the program on a thread of its own, to run in this process with no socket: the window
/// asks for its pages through the `quillo` scheme (src/embed.rs; FEEDBACK39.md, Q-465 and Q-469).
fn start_program(path: &Path, o: crate::RunOptions) -> Result<quillo::embed::Program, i32> {
    let dir = path.parent().unwrap_or(Path::new(".")).join(".quillo");
    if let Err(e) = quillo::embed::keep_secret(&dir) {
        eprintln!("quillo: {}", e);
        return Err(1);
    }
    let (tx, rx) = std::sync::mpsc::channel::<Result<quillo::server::App, i32>>();
    let path_buf = path.to_path_buf();
    let spawned = std::thread::Builder::new()
        .name("quillo-app".into())
        .stack_size(quillo::server::WORKER_STACK)
        .spawn(move || {
            let app = match crate::load(&path_buf, &o) {
                Ok(Some(app)) => app,
                Ok(None) => {
                    eprintln!("quillo: {} declares no routes, so there is nothing to show in a window", path_buf.display());
                    let _ = tx.send(Err(1));
                    return;
                }
                Err(code) => {
                    let _ = tx.send(Err(code));
                    return;
                }
            };
            app.shared().gil.release();
            let _ = tx.send(Ok(app));
        });
    if let Err(e) = spawned {
        eprintln!("quillo: cannot start the program's thread: {}", e);
        return Err(1);
    }
    // Disconnected: the thread ended without saying why (it panicked).
    let app = rx.recv().unwrap_or(Err(1))?;
    let jar = path.parent().unwrap_or(Path::new(".")).join(".quillo").join("app-cookies.json");
    Ok(quillo::embed::Program::of(app, Some(jar)))
}

/// Runs the window until it is closed; does not return when it opens.
fn open_window(name: &str, program: quillo::embed::Program, start: &str, devtools: bool) -> Result<(), String> {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let window = WindowBuilder::new()
        .with_title(name)
        .with_inner_size(LogicalSize::new(1100.0, 760.0))
        .build(&event_loop)
        .map_err(|e| e.to_string())?;

    let titles = event_loop.create_proxy();
    let opens = event_loop.create_proxy();
    let evals = std::sync::Mutex::new(event_loop.create_proxy());
    let to_page: std::sync::Arc<dyn Fn(String) + Send + Sync> = std::sync::Arc::new(move |json: String| {
        let _ = evals.lock().unwrap_or_else(std::sync::PoisonError::into_inner).send_event(UserEvent::Eval(quillo::embed::deliver_js(&json)));
    });
    let (pages, bridged) = (program.clone(), program.clone());
    let builder = WebViewBuilder::new()
        .with_url(format!("{}{}", ORIGIN, start))
        .with_devtools(devtools)
        // WebKitGTK carries no body through a scheme unless built so: on Linux they go through
        // the bridge, as on Android.
        .with_initialization_script(quillo::embed::shell_js("window.ipc.postMessage", if cfg!(target_os = "linux") { "bridge" } else { "bytes" }))
        // Each request of the page, answered by the program in this process.
        .with_asynchronous_custom_protocol("quillo".into(), move |_, request, responder| {
            let (parts, body) = request.into_parts();
            let headers = parts.headers.iter().map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string())).collect();
            let req = quillo::embed::request_of(parts.method.as_str(), &parts.uri.to_string(), headers, body);
            let p = pages.clone();
            std::thread::spawn(move || {
                let r = p.request_whole(req);
                let mut b = wry::http::Response::builder().status(r.status);
                for (k, v) in r.headers.iter().filter(|(k, _)| !k.eq_ignore_ascii_case("set-cookie")) {
                    b = b.header(k.as_str(), v.as_str());
                }
                let resp = b.body(r.body).unwrap_or_else(|_| wry::http::Response::builder().status(500).body(Vec::new()).expect("a plain answer"));
                responder.respond(resp);
            });
        })
        // What a scheme cannot carry: connections, for live data and channels.
        .with_ipc_handler(move |req| quillo::embed::bridge_message(&bridged, req.body(), to_page.clone()))
        .with_document_title_changed_handler(move |t| {
            let _ = titles.send_event(UserEvent::Title(t));
        })
        .with_navigation_handler(move |url| {
            if is_own(&url, ORIGIN) || url == "about:blank" {
                return true;
            }
            open_outside(&url);
            false
        })
        .with_new_window_req_handler(move |url, _| {
            if is_own(&url, ORIGIN) {
                let _ = opens.send_event(UserEvent::Open(url));
            } else {
                open_outside(&url);
            }
            NewWindowResponse::Deny
        });

    #[cfg(target_os = "windows")]
    let builder = {
        use wry::WebViewBuilderExtWindows;
        builder.with_https_scheme(true)
    };
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    let webview = builder.build(&window).map_err(|e| e.to_string())?;
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        let vbox = window.default_vbox().ok_or("the window has no GTK box for the web view")?;
        builder.build_gtk(vbox).map_err(|e| e.to_string())?
    };

    let name = name.to_string();
    #[cfg(target_os = "macos")]
    let menu = menu_bar(&name);
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            #[cfg(target_os = "macos")]
            Event::NewEvents(StartCause::Init) => menu.init_for_nsapp(),
            Event::UserEvent(UserEvent::Title(t)) => window.set_title(if t.trim().is_empty() { &name } else { &t }),
            Event::UserEvent(UserEvent::Open(url)) => {
                let _ = webview.load_url(&url);
            }
            Event::UserEvent(UserEvent::Eval(js)) => {
                let _ = webview.evaluate_script(&js);
            }
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                program.stop();
                *control_flow = ControlFlow::Exit
            }
            // `run` does not return: the last thing before it ends.
            Event::LoopDestroyed => crate::with::stop_all(),
            _ => {}
        }
    })
}

/// A URL of the program's own pages.
fn is_own(url: &str, origin: &str) -> bool {
    url.strip_prefix(origin).map_or(false, |rest| rest.is_empty() || rest.starts_with(['/', '?', '#']))
}

/// Links to other sites open in the system's browser.
fn open_outside(url: &str) {
    if !["https://", "http://", "mailto:"].iter().any(|p| url.starts_with(p)) {
        return;
    }
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).spawn();
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(url).spawn();
    if let Err(e) = r {
        eprintln!("quillo: cannot open {} in the browser: {}", url, e);
    }
}

/// The menu bar. Besides the usual items, its Edit menu is what makes copy,
/// paste and select-all work in the window's text fields.
#[cfg(target_os = "macos")]
fn menu_bar(name: &str) -> muda::Menu {
    use muda::{Menu, PredefinedMenuItem as P, Submenu};
    let menu = Menu::new();
    let app = Submenu::with_items(
        name,
        true,
        &[
            &P::about(None, None),
            &P::separator(),
            &P::services(None),
            &P::separator(),
            &P::hide(None),
            &P::hide_others(None),
            &P::show_all(None),
            &P::separator(),
            &P::quit(None),
        ],
    );
    let edit = Submenu::with_items(
        "Edit",
        true,
        &[&P::undo(None), &P::redo(None), &P::separator(), &P::cut(None), &P::copy(None), &P::paste(None), &P::select_all(None)],
    );
    let window = Submenu::with_items(
        "Window",
        true,
        &[&P::minimize(None), &P::maximize(None), &P::fullscreen(None), &P::separator(), &P::close_window(None)],
    );
    for m in [app, edit, window].into_iter().flatten() {
        let _ = menu.append(&m);
    }
    menu
}

#[cfg(test)]
mod tests {
    use super::is_own;

    #[test]
    fn own_pages() {
        let o = "http://127.0.0.1:5000";
        assert!(is_own("http://127.0.0.1:5000", o));
        assert!(is_own("http://127.0.0.1:5000/notes?x=1", o));
        assert!(is_own("http://127.0.0.1:5000?x", o));
        assert!(!is_own("http://127.0.0.1:50001/", o));
        assert!(!is_own("http://127.0.0.1:5000.evil.example/", o));
        assert!(!is_own("https://example.com/", o));
    }
}
