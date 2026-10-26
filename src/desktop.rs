//! `quillo app`: the program in a native window.
//!
//! The program is served as by `quillo run`, but on 127.0.0.1 at a port the
//! system picks, and only to the window (see `server::launch_gate`). The
//! window is the platform's own web view: WebKit on macOS and Linux,
//! WebView2 on Windows. Closing it ends the program.

use std::net::SocketAddr;
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
}

pub fn main(file: &str, flags: &[String]) -> i32 {
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
    let token = match quillo::server::launch_token() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("quillo: {}", e);
            return 1;
        }
    };
    let path = Path::new(file).to_path_buf();
    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Quillo".into());
    let watch = o.watch;
    let addr = match start_program(&path, o, token.clone()) {
        Ok(a) => a,
        Err(code) => return code,
    };
    let origin = format!("http://{}", addr);
    match open_window(&name, &origin, &open, &token, watch) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("quillo: cannot open the window: {}", e);
            1
        }
    }
}

/// Loads and serves the program on a thread of its own; returns once it
/// listens, with its address.
fn start_program(path: &Path, o: crate::RunOptions, token: String) -> Result<SocketAddr, i32> {
    let (tx, rx) = std::sync::mpsc::channel::<Result<SocketAddr, i32>>();
    let path = path.to_path_buf();
    let addr = format!("127.0.0.1:{}", o.port.as_deref().unwrap_or("0"));
    let spawned = std::thread::Builder::new()
        .name("quillo-app".into())
        .stack_size(quillo::server::WORKER_STACK)
        .spawn(move || {
            let mut app = match crate::load(&path, &o) {
                Ok(Some(app)) => app,
                Ok(None) => {
                    eprintln!("quillo: {} declares no routes, so there is nothing to show in a window", path.display());
                    let _ = tx.send(Err(1));
                    return;
                }
                Err(code) => {
                    let _ = tx.send(Err(code));
                    return;
                }
            };
            app.launch_token = Some(token);
            let ready = tx.clone();
            if let Err(e) = quillo::server::serve_with(app, &addr, o.workers, move |a| {
                let _ = ready.send(Ok(a));
            }) {
                eprintln!("quillo: {}", e);
                let _ = tx.send(Err(1));
            }
        });
    if let Err(e) = spawned {
        eprintln!("quillo: cannot start the program's thread: {}", e);
        return Err(1);
    }
    // Disconnected: the thread ended without saying why (it panicked).
    rx.recv().unwrap_or(Err(1))
}

/// Runs the window until it is closed; does not return when it opens.
fn open_window(name: &str, origin: &str, start: &str, token: &str, devtools: bool) -> Result<(), String> {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let window = WindowBuilder::new()
        .with_title(name)
        .with_inner_size(LogicalSize::new(1100.0, 760.0))
        .build(&event_loop)
        .map_err(|e| e.to_string())?;

    let titles = event_loop.create_proxy();
    let opens = event_loop.create_proxy();
    let (own, own_too) = (origin.to_string(), origin.to_string());
    let builder = WebViewBuilder::new()
        .with_url(format!("{}{}{}_quillo_launch={}", origin, start, if start.contains('?') { '&' } else { '?' }, token))
        .with_devtools(devtools)
        .with_document_title_changed_handler(move |t| {
            let _ = titles.send_event(UserEvent::Title(t));
        })
        .with_navigation_handler(move |url| {
            if is_own(&url, &own) || url == "about:blank" {
                return true;
            }
            open_outside(&url);
            false
        })
        .with_new_window_req_handler(move |url, _| {
            if is_own(&url, &own_too) {
                let _ = opens.send_event(UserEvent::Open(url));
            } else {
                open_outside(&url);
            }
            NewWindowResponse::Deny
        });

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
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => *control_flow = ControlFlow::Exit,
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
