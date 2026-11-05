//! A program inside an app, with no HTTP server (FEEDBACK39.md, Q-465 to Q-470; branch
//! feedback36-39).
//!
//! A phone app and `quillo app` served the program on 127.0.0.1 and pointed a web view at it:
//! the window's origin changed with the port (and the page's storage with it), iOS may take a
//! suspended app's socket back, and any other app on the phone could reach the port. Here the
//! program runs in the app's process and nothing listens: the app's web view asks for its pages
//! through a scheme of its own (`quillo://app/`), and the app hands each request to `request`.
//! What a scheme cannot carry goes through the app's message bridge: a connection that stays
//! (live data, channels) is `connect`, a numbered stream of messages each way.
//!
//! The program sees nothing new: its requests are answered by the same code that answers a
//! server's (`server::Worker::answer`), on threads of the program's own. A long-lived answer
//! (a long poll, a WebSocket, a large file sent from disk) that a server hands its socket to is
//! handed one end of a pair of local sockets, and the other end is read here. The web view keeps
//! no cookies for a custom scheme, so the cookie jar is kept here, in the app's folder, and a
//! session lasts from one launch to the next. Jobs, ticks and calls made `later` reach the
//! program without a connection to itself.

use crate::server::{Answer, App, HttpRequest, HttpResponse, Loaded, Worker};
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::SeqCst};
use std::sync::{Arc, Condvar, Mutex};

/// Where an answer goes: its head, then its body (in parts when it is made as it goes), then
/// its end. `body` gives false when the page has gone, and nothing more is made.
pub trait Reply: Send {
    fn head(&mut self, status: u16, headers: &[(String, String)]);
    fn body(&mut self, part: &[u8]) -> bool;
    fn end(&mut self);
}

/// A message on a connection: text, bytes, or its end.
#[derive(Debug, Clone, PartialEq)]
pub enum Frame {
    Text(String),
    Binary(Vec<u8>),
    Close,
}

/// A program running in this process, answering an app's requests.
#[derive(Clone)]
pub struct Program {
    inner: Arc<Inner>,
}

struct Inner {
    app: Arc<App>,
    queue: Mutex<VecDeque<(HttpRequest, Box<dyn Reply>)>>,
    ready: Condvar,
    jar: Mutex<Jar>,
    /// Where the jar is kept.
    jar_file: Option<PathBuf>,
    conns: Mutex<HashMap<u64, Pipe>>,
    next: AtomicU64,
    stopping: AtomicBool,
    /// Answers kept for `/_quillo/posted/<id>`: a form's answer, shown once.
    posted: Mutex<HashMap<String, HttpResponse>>,
}

/// How many threads answer requests: Quillo code runs one at a time, and these let one
/// request wait (on the network, a timer, the database) while another runs.
const WORKERS: usize = 4;

impl Program {
    /// Copies the program bundled at `bundle` into `<data>/app` (see `mobile::sync`), loads
    /// `main` ("main.qwe" when empty) and starts it, with no socket. `device` is what the app
    /// knows of the device, as JSON, for `platform()`; `runtime` is `"mobile"` or `"app"`.
    pub fn open(bundle: &Path, data: &Path, main: &str, device: &str, runtime: &'static str) -> Result<Program, String> {
        crate::platform::set_runtime(runtime);
        crate::platform::set_launcher(device);
        let app_dir = data.join("app");
        std::fs::create_dir_all(&app_dir).map_err(|e| format!("{}: {}", app_dir.display(), e))?;
        crate::mobile::sync(bundle, &app_dir)?;
        crate::mobile::keep_secret(data)?;
        let main = if main.is_empty() { "main.qwe" } else { main };
        let path = app_dir.join(main);
        if !path.exists() {
            return Err(format!("no {} in the app's program", main));
        }
        let (tx, rx) = std::sync::mpsc::channel::<Result<App, String>>();
        // Loaded on a thread with a server's stack, as `mobile::start` loads it.
        let loader = std::thread::Builder::new().name("quillo-load".into()).stack_size(crate::server::WORKER_STACK).spawn(move || {
            let base = path.parent().map_or_else(|| app_dir.clone(), Path::to_path_buf);
            let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            crate::service::serve_as(&stem);
            let mut interp = crate::Interp::new(&base);
            interp.env_file = Arc::new(crate::mobile::app_env(&app_dir.join("app.toml")));
            let shared = interp.shared.clone();
            if let Err(e) = interp.load_file(&path) {
                let _ = tx.send(Err(e.to_string()));
                return;
            }
            if interp.routes.is_empty() && interp.mounts.is_empty() {
                let _ = tx.send(Err(format!("{} declares no routes, so there is nothing to show", path.display())));
                return;
            }
            let app = App::new(Loaded::Ready(interp), shared, &path, &base, false);
            // Loaded: the lock goes with it to the threads that answer.
            app.shared().gil.release();
            let _ = tx.send(Ok(app));
        });
        loader.map_err(|e| format!("cannot start the program's thread: {}", e))?;
        let app = rx.recv().map_err(|_| "the program stopped as it loaded".to_string())??;
        Ok(Program::of(app, Some(data.join(".quillo").join("cookies.json"))))
    }

    /// A program already loaded (`quillo app`), its lock given up; the jar kept in `jar_file`.
    pub fn of(app: App, jar_file: Option<PathBuf>) -> Program {
        crate::value::SERVING.store(true, SeqCst);
        crate::value::HIDING.store(!app.watch, SeqCst);
        crate::server::SHOW_ERRORS.store(app.watch, SeqCst);
        let jar = jar_file.as_deref().map(Jar::read).unwrap_or_default();
        let inner = Arc::new(Inner {
            app: Arc::new(app),
            queue: Mutex::new(VecDeque::new()),
            ready: Condvar::new(),
            jar: Mutex::new(jar),
            jar_file,
            conns: Mutex::new(HashMap::new()),
            next: AtomicU64::new(1),
            stopping: AtomicBool::new(false),
            posted: Mutex::new(HashMap::new()),
        });
        for i in 0..WORKERS {
            let inner = inner.clone();
            let _ = std::thread::Builder::new().name(format!("quillo-embed-{}", i)).stack_size(crate::server::WORKER_STACK).spawn(move || work(inner));
        }
        // Jobs, ticks and calls made later, with no connection to the program (Q-468).
        let local = inner.clone();
        crate::jobs::answer_here(Box::new(move |req| answer_now(&local, req)));
        crate::server::start_embedded(&inner.app);
        Program { inner }
    }

    /// One request: its answer comes on `reply`, on one of the program's threads.
    pub fn request(&self, req: HttpRequest, reply: Box<dyn Reply>) {
        let mut req = req;
        // A form's answer, kept for the page to show (see `post`).
        if let Some(id) = req.url.strip_prefix("/_quillo/posted/") {
            let kept = self.inner.posted.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(id);
            let mut reply = reply;
            let resp = kept.unwrap_or_else(|| crate::server::plain_response(404, "this answer was shown already\n"));
            send_whole(&mut *reply, &resp);
            return;
        }
        self.inner.jar.lock().unwrap_or_else(std::sync::PoisonError::into_inner).add_to(&mut req);
        self.inner.queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push_back((req, reply));
        self.inner.ready.notify_one();
    }

    /// `request`, waiting for the whole answer.
    pub fn request_whole(&self, req: HttpRequest) -> HttpResponse {
        let (tx, rx) = std::sync::mpsc::channel();
        self.request(req, Box::new(Whole { status: 0, headers: Vec::new(), body: Vec::new(), done: Some(tx) }));
        rx.recv().unwrap_or_else(|_| crate::server::plain_response(500, "the program stopped\n"))
    }

    /// A form the scheme cannot carry, sent through the bridge: answered now and kept, so that
    /// the page can go to `/_quillo/posted/<id>` to show it, as after an ordinary post. The id,
    /// or the answer itself when it is a redirect (the page follows it).
    pub fn post(&self, req: HttpRequest) -> (Option<String>, HttpResponse) {
        let resp = self.request_whole(req);
        if (300..400).contains(&resp.status) {
            return (None, resp);
        }
        let id = crate::service::new_id(16);
        self.inner.posted.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(id.clone(), resp.clone());
        (Some(id), resp)
    }

    /// A connection the page keeps (live data, channels), through the bridge: `req` is the
    /// request a page would upgrade to a WebSocket. What the program sends comes on `to_page`
    /// until it gives false or the connection ends. Its number, for `send` and `close`.
    pub fn connect(&self, req: HttpRequest, to_page: Box<dyn FnMut(Frame) -> bool + Send>) -> Result<u64, String> {
        let mut req = req;
        self.inner.jar.lock().unwrap_or_else(std::sync::PoisonError::into_inner).add_to(&mut req);
        let key = crate::service::new_id(16);
        for (k, v) in [("upgrade", "websocket"), ("connection", "Upgrade"), ("sec-websocket-version", "13")] {
            req.headers.push((k.into(), v.into()));
        }
        req.headers.push(("sec-websocket-key".into(), key));
        let (ours, theirs) = pair().map_err(|e| e.to_string())?;
        let answered = answer_now_with(&self.inner, req, Some(theirs));
        if answered.status != 101 {
            return Err(format!("the program answered {} where a connection was asked for", answered.status));
        }
        let id = self.inner.next.fetch_add(1, SeqCst);
        let reader = ours.try_clone().map_err(|e| e.to_string())?;
        self.inner.conns.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(id, ours);
        let inner = self.inner.clone();
        let _ = std::thread::Builder::new().name("quillo-embed-conn".into()).spawn(move || {
            read_frames(reader, to_page);
            inner.conns.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&id);
        });
        Ok(id)
    }

    /// Sends a message from the page on connection `id`.
    pub fn send(&self, id: u64, frame: Frame) -> bool {
        let mut conns = self.inner.conns.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(s) = conns.get_mut(&id) else { return false };
        let (op, data): (u8, &[u8]) = match &frame {
            Frame::Text(t) => (0x1, t.as_bytes()),
            Frame::Binary(b) => (0x2, b),
            Frame::Close => (0x8, &[]),
        };
        s.write_all(&client_frame(op, data)).is_ok()
    }

    /// Ends connection `id`, as the page leaving does.
    pub fn close(&self, id: u64) {
        if let Some(s) = self.inner.conns.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&id) {
            let mut s = s;
            let _ = s.write_all(&client_frame(0x8, &[]));
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
    }

    /// Loads the program again when its files change (`quillo app --watch`), as a server does.
    pub fn watch(&self) {
        crate::statics::WATCH_SERVED.store(true, SeqCst);
        let _ = std::thread::Builder::new().name("quillo-watch".into()).stack_size(crate::server::WORKER_STACK).spawn(crate::server::watcher(self.inner.app.clone()));
    }

    /// The app goes behind the home screen: its cookies are kept now, as it may not come back.
    pub fn suspend(&self) {
        self.inner.jar.lock().unwrap_or_else(std::sync::PoisonError::into_inner).keep(self.inner.jar_file.as_deref());
    }

    pub fn resume(&self) {}

    /// Stops answering: the threads end once what they answer is answered.
    pub fn stop(&self) {
        self.suspend();
        self.inner.stopping.store(true, SeqCst);
        self.inner.ready.notify_all();
        for (_, s) in self.inner.conns.lock().unwrap_or_else(std::sync::PoisonError::into_inner).drain() {
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
    }
}

/// A key for signing sessions kept in `dir`, made once, unless `QUILLO_SECRET` is set: an app is
/// one person's, and their session lasts from one launch to the next.
pub fn keep_secret(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
    crate::mobile::keep_secret(dir)
}

/// A request in the form an app gives it: the method, the URL (`quillo://app/notes?x=1`,
/// `https://quillo.localhost/notes`, or a path), headers and body. Its host is the URL's (`app`
/// for a path), so the page's own origin is the program's.
pub fn request_of(method: &str, url: &str, headers: Vec<(String, String)>, body: Vec<u8>) -> HttpRequest {
    let (host, path) = match url.find("://") {
        Some(i) => {
            let rest = &url[i + 3..];
            let at = rest.find(['/', '?']).unwrap_or(rest.len());
            let path = &rest[at..];
            (rest[..at].to_string(), if path.starts_with('/') { path.to_string() } else { format!("/{}", path) })
        }
        None if url.starts_with('/') => ("app".to_string(), url.to_string()),
        None => ("app".to_string(), format!("/{}", url)),
    };
    let path = path.split('#').next().unwrap_or("/").to_string();
    let mut headers: Vec<(String, String)> = headers.into_iter().filter(|(k, _)| !k.eq_ignore_ascii_case("host") && !k.eq_ignore_ascii_case("cookie")).collect();
    headers.push(("host".into(), host));
    HttpRequest { method: method.to_ascii_uppercase(), url: path, headers, body, remote: "app".into() }
}

/// What the threads do: take a request, answer it, and wait for the next.
fn work(inner: Arc<Inner>) {
    let mut worker = Worker::new(&inner.app);
    loop {
        let job = {
            let mut q = inner.queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            loop {
                if inner.stopping.load(SeqCst) {
                    return;
                }
                if let Some(j) = q.pop_front() {
                    break j;
                }
                q = inner.ready.wait(q).unwrap_or_else(std::sync::PoisonError::into_inner);
            }
        };
        let (req, mut reply) = job;
        if worker.outdated(&inner.app) {
            worker = Worker::new(&inner.app);
        }
        answer(&inner, &mut worker, req, &mut *reply);
    }
}

/// Where a streamed answer goes: the reply's head when it starts, its parts as they come.
struct ReplySink<'a> {
    reply: &'a mut dyn Reply,
    inner: &'a Inner,
    gone: bool,
    /// The request's path, whose folder a cookie with no `Path` is for.
    asked: &'a str,
}

impl crate::stream::Sink for ReplySink<'_> {
    fn chunked(&self) -> bool {
        false
    }

    fn start(&mut self, resp: &HttpResponse, _: bool) -> bool {
        self.inner.jar.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take_from(resp, self.asked, self.inner.jar_file.as_deref());
        // Not those the program sent none of (FEEDBACK55.md, Q-710; branch feedback55).
        let headers: Vec<(String, String)> = resp.headers.iter().filter(|(_, v)| v != crate::server::DROPPED).cloned().collect();
        self.reply.head(resp.status, &headers);
        true
    }

    fn write(&mut self, data: &[u8]) -> bool {
        if self.gone {
            return false;
        }
        self.gone = !self.reply.body(data);
        !self.gone
    }
}

fn answer(inner: &Arc<Inner>, worker: &mut Worker, req: HttpRequest, reply: &mut dyn Reply) {
    inner.app.shared().gil.acquire();
    let asked = req.url.clone();
    let mut sink = ReplySink { reply, inner, gone: false, asked: &asked };
    let got = worker.answer(&inner.app, req, Some(&mut sink));
    inner.app.shared().gil.release();
    match got {
        Answer::Respond(resp) => {
            inner.jar.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take_from(&resp, &asked, inner.jar_file.as_deref());
            send_whole(reply, &resp);
        }
        // A HEAD of a large file says the length a GET would send (FEEDBACK55.md, Q-725; branch
        // feedback55).
        Answer::Head(mut resp, len) => {
            inner.jar.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take_from(&resp, &asked, inner.jar_file.as_deref());
            resp.headers.push(("content-length".into(), len.to_string()));
            send_whole(reply, &resp);
        }
        Answer::Streamed { .. } => reply.end(),
        // A long poll, a stream of events, a file sent from disk: it writes to a socket as it
        // would a client's, and what it writes is read back here.
        Answer::Detach(f) => {
            let Ok((ours, theirs)) = pair() else {
                send_whole(reply, &crate::server::plain_response(500, "no pipe for the answer\n"));
                return;
            };
            f(as_tcp(theirs), Vec::new());
            relay(ours, reply, inner, &asked);
        }
    }
}

/// An answer at once, with no reply to stream to: for jobs and ticks, a form kept for the page,
/// and the handshake of a connection (`stream`: the socket end a detached answer gets).
fn answer_now_with(inner: &Arc<Inner>, req: HttpRequest, stream: Option<Pipe>) -> HttpResponse {
    let mut worker = Worker::new(&inner.app);
    let asked = req.url.clone();
    inner.app.shared().gil.acquire();
    let got = worker.answer(&inner.app, req, None);
    inner.app.shared().gil.release();
    match got {
        Answer::Respond(resp) | Answer::Head(resp, _) => resp,
        Answer::Streamed { .. } => crate::server::plain_response(200, ""),
        Answer::Detach(f) => match stream {
            Some(s) => {
                f(as_tcp(s), Vec::new());
                HttpResponse { status: 101, headers: Vec::new(), body: Vec::new() }
            }
            None => {
                let Ok((ours, theirs)) = pair() else { return crate::server::plain_response(500, "no pipe\n") };
                f(as_tcp(theirs), Vec::new());
                let mut w = Whole { status: 0, headers: Vec::new(), body: Vec::new(), done: None };
                relay(ours, &mut w, inner, &asked);
                HttpResponse { status: w.status, headers: w.headers, body: w.body }
            }
        },
    }
}

fn answer_now(inner: &Arc<Inner>, req: HttpRequest) -> HttpResponse {
    answer_now_with(inner, req, None)
}

/// Two connected ends: what a detached answer writes to, and where it is read from here. On
/// Unix a pair of local sockets, which nothing else can reach.
#[cfg(unix)]
type Pipe = std::os::unix::net::UnixStream;
#[cfg(not(unix))]
type Pipe = std::net::TcpStream;

#[cfg(unix)]
fn pair() -> std::io::Result<(Pipe, Pipe)> {
    std::os::unix::net::UnixStream::pair()
}

/// Elsewhere (Windows), two ends of a loopback connection, the listener taking only the
/// connection made from here, and gone once it has.
#[cfg(not(unix))]
fn pair() -> std::io::Result<(Pipe, Pipe)> {
    let l = std::net::TcpListener::bind("127.0.0.1:0")?;
    let ours = std::net::TcpStream::connect(l.local_addr()?)?;
    loop {
        let (theirs, from) = l.accept()?;
        if from == ours.local_addr()? {
            return Ok((ours, theirs));
        }
    }
}

/// A local socket as the `TcpStream` a detached answer takes: it reads and writes as one.
#[cfg(unix)]
fn as_tcp(s: Pipe) -> std::net::TcpStream {
    use std::os::fd::{FromRawFd, IntoRawFd};
    // SAFETY: the descriptor is given up by `s`; reads, writes, shutting down and timeouts are
    // the same calls on a local socket as on a TCP one.
    unsafe { std::net::TcpStream::from_raw_fd(s.into_raw_fd()) }
}

#[cfg(not(unix))]
fn as_tcp(s: Pipe) -> std::net::TcpStream {
    s
}

fn send_whole(reply: &mut dyn Reply, resp: &HttpResponse) {
    let mut headers = resp.headers.clone();
    // Nor those the program sent none of (FEEDBACK55.md, Q-710; branch feedback55).
    headers.retain(|(k, v)| !k.eq_ignore_ascii_case(crate::statics::SEND_HEADER) && v != crate::server::DROPPED);
    reply.head(resp.status, &headers);
    if !resp.body.is_empty() {
        reply.body(&resp.body);
    }
    reply.end();
}

/// An HTTP answer read from a socket, to `reply`: its head, then its body by its length, in
/// chunks, or until the end.
fn relay(mut from: Pipe, reply: &mut dyn Reply, inner: &Inner, asked: &str) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 16 << 10];
    let (head_len, status, headers) = loop {
        let mut hs = [httparse::EMPTY_HEADER; 64];
        let mut r = httparse::Response::new(&mut hs);
        if let Ok(httparse::Status::Complete(n)) = r.parse(&buf) {
            let headers: Vec<(String, String)> = r.headers.iter().map(|h| (h.name.to_ascii_lowercase(), String::from_utf8_lossy(h.value).into_owned())).collect();
            break (n, r.code.unwrap_or(500), headers);
        }
        match from.read(&mut chunk) {
            Ok(0) | Err(_) => {
                reply.head(502, &[]);
                reply.end();
                return;
            }
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    };
    let resp = HttpResponse { status, headers: headers.clone(), body: Vec::new() };
    inner.jar.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take_from(&resp, asked, inner.jar_file.as_deref());
    let shown: Vec<(String, String)> = headers.iter().filter(|(k, _)| k != "transfer-encoding" && k != "connection").cloned().collect();
    reply.head(status, &shown);
    let header = |n: &str| headers.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone());
    let mut rest = buf.split_off(head_len);
    let chunked = header("transfer-encoding").is_some_and(|t| t.to_ascii_lowercase().contains("chunked"));
    let mut left = header("content-length").and_then(|l| l.trim().parse::<usize>().ok());
    if matches!(status, 101 | 204 | 304) {
        reply.end();
        return;
    }
    loop {
        if chunked {
            // Whole chunks out of what came; the last is empty.
            loop {
                let Some(eol) = rest.windows(2).position(|w| w == b"\r\n") else { break };
                let size = usize::from_str_radix(String::from_utf8_lossy(&rest[..eol]).split(';').next().unwrap_or("").trim(), 16).unwrap_or(0);
                if size == 0 {
                    reply.end();
                    return;
                }
                if rest.len() < eol + 2 + size + 2 {
                    break;
                }
                if !reply.body(&rest[eol + 2..eol + 2 + size]) {
                    let _ = from.shutdown(std::net::Shutdown::Both);
                    return;
                }
                rest.drain(..eol + 2 + size + 2);
            }
        } else if !rest.is_empty() {
            let take = left.map_or(rest.len(), |l| l.min(rest.len()));
            if !reply.body(&rest[..take]) {
                let _ = from.shutdown(std::net::Shutdown::Both);
                return;
            }
            left = left.map(|l| l - take);
            rest.clear();
        }
        if left == Some(0) {
            break;
        }
        match from.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => rest.extend_from_slice(&chunk[..n]),
        }
    }
    reply.end();
}

/// A whole answer, for `request_whole` and jobs.
struct Whole {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    done: Option<std::sync::mpsc::Sender<HttpResponse>>,
}

impl Reply for Whole {
    fn head(&mut self, status: u16, headers: &[(String, String)]) {
        self.status = status;
        self.headers = headers.to_vec();
    }

    fn body(&mut self, part: &[u8]) -> bool {
        self.body.extend_from_slice(part);
        true
    }

    fn end(&mut self) {
        if let Some(tx) = self.done.take() {
            let _ = tx.send(HttpResponse { status: self.status, headers: std::mem::take(&mut self.headers), body: std::mem::take(&mut self.body) });
        }
    }
}

// ---- the WebSocket, as a page's end of it --------------------------------------------------

/// A frame as a client sends it: masked (RFC 6455 §5.3).
fn client_frame(op: u8, data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x80 | op];
    match data.len() {
        n if n < 126 => out.push(0x80 | n as u8),
        n if n <= 0xffff => {
            out.push(0x80 | 126);
            out.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            out.push(0x80 | 127);
            out.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    let mask: [u8; 4] = fastrand::u32(..).to_be_bytes();
    out.extend_from_slice(&mask);
    out.extend(data.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    out
}

/// What the program sends on a connection, after its handshake, as messages to the page.
fn read_frames(mut from: Pipe, mut to_page: Box<dyn FnMut(Frame) -> bool + Send>) {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 16 << 10];
    let mut handshake = true;
    let mut message: Vec<u8> = Vec::new();
    let mut message_op = 0u8;
    loop {
        if handshake {
            if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                buf.drain(..end + 4);
                handshake = false;
            }
        }
        while !handshake && buf.len() >= 2 {
            let (fin, op) = (buf[0] & 0x80 != 0, buf[0] & 0x0f);
            let mut len = (buf[1] & 0x7f) as usize;
            let mut at = 2;
            if len == 126 {
                if buf.len() < 4 {
                    break;
                }
                len = u16::from_be_bytes([buf[2], buf[3]]) as usize;
                at = 4;
            } else if len == 127 {
                if buf.len() < 10 {
                    break;
                }
                len = u64::from_be_bytes(buf[2..10].try_into().unwrap_or([0; 8])) as usize;
                at = 10;
            }
            if buf.len() < at + len {
                break;
            }
            let payload: Vec<u8> = buf[at..at + len].to_vec();
            buf.drain(..at + len);
            match op {
                0x8 => {
                    to_page(Frame::Close);
                    return;
                }
                0x9 => {
                    let _ = from.write_all(&client_frame(0xA, &payload));
                }
                0xA => {}
                _ => {
                    if op != 0 {
                        message_op = op;
                    }
                    message.extend_from_slice(&payload);
                    if fin {
                        let frame = if message_op == 0x1 { Frame::Text(String::from_utf8_lossy(&message).into_owned()) } else { Frame::Binary(std::mem::take(&mut message)) };
                        message.clear();
                        if !to_page(frame) {
                            let _ = from.shutdown(std::net::Shutdown::Both);
                            return;
                        }
                    }
                }
            }
        }
        match from.read(&mut chunk) {
            Ok(0) | Err(_) => {
                to_page(Frame::Close);
                return;
            }
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
}

// ---- the cookie jar ------------------------------------------------------------------------

/// The program's cookies, for an app of one person: by name, path and domain, as a browser keeps
/// them, where a second path's or a deletion at another path replaced the first (FEEDBACK55.md,
/// Q-714; branch feedback55); each with its value and when it ends.
#[derive(Default)]
pub struct Jar {
    cookies: HashMap<(String, String, String), (String, Option<i64>)>,
}

/// RFC 6265's default-path of a request's path (§5.1.4): its folder, `/` for one at the root.
fn default_path(asked: &str) -> String {
    let path = asked.split(['?', '#']).next().unwrap_or("");
    match path.rfind('/') {
        Some(i) if i > 0 && path.starts_with('/') => path[..i].to_string(),
        _ => "/".to_string(),
    }
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

impl Jar {
    /// The jar kept on disk: a map of records by a name of their own (the cookie's name for one at
    /// `/` of no domain, as jars before were), each with its `name` (else the record's), `value`,
    /// `path`, `domain` and `until`.
    fn read(file: &Path) -> Jar {
        let mut jar = Jar::default();
        let Ok(text) = std::fs::read_to_string(file) else { return jar };
        let Ok(serde_json::Value::Object(m)) = serde_json::from_str::<serde_json::Value>(&text) else { return jar };
        for (key, c) in m {
            let (Some(v), Some(p)) = (c["value"].as_str(), c["path"].as_str()) else { continue };
            let name = c["name"].as_str().map_or(key, str::to_string);
            let domain = c["domain"].as_str().unwrap_or("").to_string();
            let until = c["until"].as_i64();
            if until.map_or(true, |u| u > now()) {
                jar.cookies.insert((name, p.to_string(), domain), (v.to_string(), until));
            }
        }
        jar
    }

    fn keep(&self, file: Option<&Path>) {
        let Some(file) = file else { return };
        let m: serde_json::Map<String, serde_json::Value> = self
            .cookies
            .iter()
            .map(|((n, p, d), (v, u))| {
                let key = if p == "/" && d.is_empty() { n.clone() } else { format!("{}; Path={}; Domain={}", n, p, d) };
                (key, serde_json::json!({ "name": n, "value": v, "path": p, "domain": d, "until": u }))
            })
            .collect();
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = file.with_extension("tmp");
        if std::fs::write(&tmp, serde_json::Value::Object(m).to_string()).is_ok() {
            let _ = std::fs::rename(&tmp, file);
        }
    }

    /// The `Set-Cookie`s of the answer to a request for `asked`, taken in (and kept, when they
    /// changed). One with no `Path` is for the folder of the path asked for, RFC 6265's
    /// default-path (§5.1.4), where it was kept at `/` (FEEDBACK55.md, Q-714; branch feedback55;
    /// found in review).
    pub fn take_from(&mut self, resp: &HttpResponse, asked: &str, file: Option<&Path>) {
        let mut changed = false;
        for (_, v) in resp.headers.iter().filter(|(k, _)| k.eq_ignore_ascii_case("set-cookie")) {
            let mut parts = v.split(';');
            let Some((name, value)) = parts.next().and_then(|p| p.split_once('=')) else { continue };
            let (name, value) = (name.trim().to_string(), value.trim().to_string());
            let mut path = default_path(asked);
            let mut domain = String::new();
            let mut until = None;
            for a in parts {
                let (k, val) = a.split_once('=').map_or((a.trim(), ""), |(k, v)| (k.trim(), v.trim()));
                match k.to_ascii_lowercase().as_str() {
                    "path" if val.starts_with('/') => path = val.to_string(),
                    "domain" => domain = val.trim_start_matches('.').to_ascii_lowercase(),
                    "max-age" => until = val.parse::<i64>().ok().map(|s| now() + s),
                    "expires" if until.is_none() => until = crate::dates::http_seconds(val),
                    _ => {}
                }
            }
            let key = (name, path, domain);
            if until.is_some_and(|u| u <= now()) || value.is_empty() && until.is_some() {
                changed |= self.cookies.remove(&key).is_some();
            } else {
                self.cookies.insert(key, (value, until));
                changed = true;
            }
        }
        if changed {
            self.keep(file);
        }
    }

    /// `Cookie` on a request, for its path: a longer path's first, as browsers send them, so that
    /// `request.cookies` keeps it (FEEDBACK55.md, Q-714 and Q-718; branch feedback55).
    pub fn add_to(&self, req: &mut HttpRequest) {
        let path = req.url.split('?').next().unwrap_or("/");
        let t = now();
        let mut pairs: Vec<(&str, &str, &str)> = self
            .cookies
            .iter()
            .filter(|((_, p, _), (_, u))| u.map_or(true, |u| u > t) && (path == p || path.starts_with(&format!("{}/", p.trim_end_matches('/'))) || p == "/"))
            .map(|((n, p, _), (v, _))| (n.as_str(), p.as_str(), v.as_str()))
            .collect();
        if pairs.is_empty() {
            return;
        }
        pairs.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(b.0)).then(a.2.cmp(b.2)));
        let line: Vec<String> = pairs.iter().map(|(n, _, v)| format!("{}={}", n, v)).collect();
        req.headers.retain(|(k, _)| !k.eq_ignore_ascii_case("cookie"));
        req.headers.push(("cookie".into(), line.join("; ")));
    }
}

// ---- the bridge: what the page sends that the scheme cannot carry -------------------------

/// The script an app's shell runs before every page: the runtime's way out, over the app's
/// bridge, handed to the runtime when it asks with a `quillo-wire` event (no name on `window`). `__QUILLO_POST__` is the shell's own way to send the native side
/// a string, a function's name (each system has one: `window.ipc.postMessage`, ...), called with
/// the message; the native side answers with `deliver_js`'s event.
/// `__QUILLO_BODIES__` is `"bridge"` where the scheme carries no body (Android), else
/// `"bytes"`, where it carries bytes but not a `Blob` or a `FormData` (WebKit).
pub const SHELL_JS: &str = r#"(() => {
  const post = (m) => { __QUILLO_POST__(JSON.stringify(m)); };
  const bodies = "__QUILLO_BODIES__";
  const waiting = new Map(), sockets = new Map();
  let next = 1;
  const b64 = (buf) => { let s = ""; const u = new Uint8Array(buf); for (let i = 0; i < u.length; i += 0x8000) s += String.fromCharCode.apply(null, u.subarray(i, i + 0x8000)); return btoa(s); };
  const unb64 = (t) => Uint8Array.from(atob(t || ""), (c) => c.charCodeAt(0));
  const ours = (u) => { try { return new URL(u, location.href).origin === location.origin; } catch (_) { return false; } };
  // A body as bytes, and its type: a FormData's multipart, with its boundary.
  const bytes = async (body, headers) => {
    if (body === undefined || body === null) return null;
    const r = new Response(body);
    const t = r.headers.get("content-type");
    if (t && !Object.keys(headers).some((k) => k.toLowerCase() === "content-type")) headers["content-type"] = t;
    return r.arrayBuffer();
  };
  const plain = (h) => { const o = {}; if (h instanceof Headers) h.forEach((v, k) => { o[k] = v; }); else Object.assign(o, h || {}); return o; };
  const realFetch = window.fetch.bind(window);
  const wire = {
    async fetch(u, init) {
      init = init || {};
      if (typeof Request !== "undefined" && u instanceof Request) return realFetch(u, init);
      const method = (init.method || "GET").toUpperCase();
      const has = init.body !== undefined && init.body !== null;
      if (!ours(u) || !has) return realFetch(u, init);
      const headers = plain(init.headers);
      const buf = await bytes(init.body, headers);
      if (bodies === "bytes") return realFetch(u, Object.assign({}, init, { headers, body: buf }));
      const id = next++;
      return new Promise((ok, no) => {
        waiting.set(id, { ok, no });
        post({ op: "fetch", id, method, url: new URL(u, location.href).href, headers, body: b64(buf) });
        if (init.signal) init.signal.addEventListener("abort", () => { waiting.delete(id); no(new DOMException("aborted", "AbortError")); });
      });
    },
    socket(path) {
      const id = next++;
      const s = { readyState: 0, binaryType: "blob", onopen: null, onmessage: null, onclose: null, onerror: null,
        send(d) { if (s.readyState !== 1) return; if (typeof d === "string") post({ op: "send", id, text: d }); else post({ op: "send", id, bin: b64(d instanceof ArrayBuffer ? d : d.buffer) }); },
        close() { if (s.readyState > 1) return; s.readyState = 2; post({ op: "close", id }); },
        addEventListener(k, f) { const o = s["on" + k]; s["on" + k] = (e) => { if (o) o(e); f(e); }; } };
      sockets.set(id, s);
      post({ op: "open", id, url: new URL(path, location.href).href });
      return s;
    },
    receive(m) {
      if (typeof m === "string") m = JSON.parse(m);
      if (m.op === "answer") {
        const w = waiting.get(m.id); if (!w) return; waiting.delete(m.id);
        w.ok(new Response(m.status === 204 || m.status === 304 ? null : unb64(m.body), { status: m.status, headers: m.headers }));
      } else if (m.op === "failed") {
        const w = waiting.get(m.id); if (w) { waiting.delete(m.id); w.no(new TypeError(m.error)); }
        const s = sockets.get(m.id); if (s) { sockets.delete(m.id); s.readyState = 3; if (s.onerror) s.onerror({}); if (s.onclose) s.onclose({ code: 1006 }); }
      } else if (m.op === "opened") {
        const s = sockets.get(m.id); if (s) { s.readyState = 1; if (s.onopen) s.onopen({}); }
      } else if (m.op === "frame") {
        const s = sockets.get(m.id); if (!s || !s.onmessage) return;
        const data = m.text !== undefined ? m.text : (s.binaryType === "arraybuffer" ? unb64(m.bin).buffer : new Blob([unb64(m.bin)]));
        s.onmessage({ data });
      } else if (m.op === "closed") {
        const s = sockets.get(m.id); if (s) { sockets.delete(m.id); s.readyState = 3; if (s.onclose) s.onclose({ code: 1000 }); }
      } else if (m.op === "posted") {
        // Only to the web or the app's own pages (FEEDBACK55.md, Q-719; branch feedback55).
        const goes = (u) => { try { const p = new URL(u, location.href).protocol; return p === "http:" || p === "https:" || p === location.protocol; } catch (_) { return false; } };
        if (m.location && goes(m.location)) location.href = m.location;
        else if (m.page) location.href = "/_quillo/posted/" + m.page;
      }
    },
  };
  // No name on `window`: the runtime asks for the wire with an event, and the app's native side
  // answers with one.
  document.addEventListener("quillo-wire", (e) => { if (typeof e.detail === "function") e.detail(wire); });
  document.addEventListener("quillo-wire-receive", (e) => wire.receive(e.detail));
  // Where the scheme carries no body (Android), the page's own scripts' `fetch` goes the same
  // way, or its bodies would arrive empty: the one thing this changes on `window`.
  if (bodies === "bridge") window.fetch = (u, init) => wire.fetch(u, init);
  // A form the scheme cannot carry (every post on Android, one with a file on WebKit): sent
  // through the bridge, its answer kept for the page to go to, as after a post and redirect.
  document.addEventListener("submit", (e) => {
    const f = e.target;
    if (e.defaultPrevented || !(f instanceof HTMLFormElement) || (f.method || "get").toLowerCase() !== "post") return;
    const action = f.action || location.href;
    if (!ours(action)) return;
    const file = !!f.querySelector("input[type=file]");
    if (bodies === "bytes" && !file) return;
    e.preventDefault();
    const fd = new FormData(f, e.submitter || undefined);
    const plainForm = (f.enctype || "").toLowerCase() !== "multipart/form-data";
    const body = plainForm ? new URLSearchParams(fd) : fd;
    const headers = {};
    bytes(body, headers).then((buf) => post({ op: "post", id: next++, method: "POST", url: new URL(action, location.href).href, headers, body: b64(buf) }));
  }, true);
})();"#;

/// What the page sent through the bridge (`SHELL_JS`'s messages, JSON), carried out; what goes
/// back to the page goes to `to_page` as JSON, for the shell to hand the page with `deliver_js`.
pub fn bridge_message(p: &Program, message: &str, to_page: Arc<dyn Fn(String) + Send + Sync>) {
    let Ok(m) = serde_json::from_str::<serde_json::Value>(message) else { return };
    let id = m["id"].as_u64().unwrap_or(0);
    let headers = |m: &serde_json::Value| -> Vec<(String, String)> {
        m["headers"].as_object().map(|o| o.iter().map(|(k, v)| (k.to_ascii_lowercase(), v.as_str().unwrap_or("").to_string())).collect()).unwrap_or_default()
    };
    let body = |m: &serde_json::Value| m["body"].as_str().and_then(|b| b64_decode(b)).unwrap_or_default();
    match m["op"].as_str().unwrap_or("") {
        "fetch" | "post" => {
            let mut hs = headers(&m);
            hs.retain(|(k, _)| k != "origin");
            hs.push(("origin".into(), origin_of(m["url"].as_str().unwrap_or(""))));
            let req = request_of(m["method"].as_str().unwrap_or("POST"), m["url"].as_str().unwrap_or("/"), hs, body(&m));
            let (p, posting) = (p.clone(), m["op"] == "post");
            std::thread::spawn(move || {
                if posting {
                    let (page, resp) = p.post(req);
                    // Not one that runs code or carries a page, which the page would go to
                    // (FEEDBACK55.md, Q-719; branch feedback55; found in review).
                    let location = resp.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("location")).map(|(_, v)| v.clone()).filter(|l| !crate::builtins::runs_or_carries(l));
                    to_page(serde_json::json!({ "op": "posted", "id": id, "page": page, "location": location }).to_string());
                    return;
                }
                let r = p.request_whole(req);
                let hs: serde_json::Map<String, serde_json::Value> = r.headers.iter().filter(|(k, _)| !k.eq_ignore_ascii_case("set-cookie")).map(|(k, v)| (k.clone(), v.clone().into())).collect();
                to_page(serde_json::json!({ "op": "answer", "id": id, "status": r.status, "headers": hs, "body": crate::crypto_b64(&r.body) }).to_string());
            });
        }
        "open" => {
            let url = m["url"].as_str().unwrap_or("/").to_string();
            let (p, page) = (p.clone(), to_page.clone());
            std::thread::spawn(move || {
                let req = request_of("GET", &url, vec![("origin".into(), origin_of(&url))], Vec::new());
                let out = page.clone();
                let connected = p.connect(
                    req,
                    Box::new(move |f| {
                        let msg = match f {
                            Frame::Text(t) => serde_json::json!({ "op": "frame", "id": id, "text": t }),
                            Frame::Binary(b) => serde_json::json!({ "op": "frame", "id": id, "bin": crate::crypto_b64(&b) }),
                            Frame::Close => serde_json::json!({ "op": "closed", "id": id }),
                        };
                        out(msg.to_string());
                        true
                    }),
                );
                match connected {
                    Ok(conn) => {
                        BRIDGED.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(id, conn);
                        page(serde_json::json!({ "op": "opened", "id": id }).to_string());
                    }
                    Err(e) => page(serde_json::json!({ "op": "failed", "id": id, "error": e }).to_string()),
                }
            });
        }
        "send" => {
            let Some(conn) = BRIDGED.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(&id).copied() else { return };
            let frame = match (m["text"].as_str(), m["bin"].as_str()) {
                (Some(t), _) => Frame::Text(t.to_string()),
                (None, Some(b)) => Frame::Binary(b64_decode(b).unwrap_or_default()),
                _ => return,
            };
            p.send(conn, frame);
        }
        "close" => {
            if let Some(conn) = BRIDGED.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&id) {
                p.close(conn);
            }
        }
        _ => {}
    }
}

/// `scheme://host` of the page's URL: the origin its requests come from.
fn origin_of(url: &str) -> String {
    match url.find("://") {
        Some(i) => {
            let rest = &url[i + 3..];
            format!("{}://{}", &url[..i], &rest[..rest.find(['/', '?', '#']).unwrap_or(rest.len())])
        }
        None => "quillo://app".into(),
    }
}

/// The page's connection numbers, to the program's.
static BRIDGED: std::sync::LazyLock<Mutex<HashMap<u64, u64>>> = std::sync::LazyLock::new(Default::default);

/// Standard base64, padded, as `atob` reads it.
fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let url: String = s.trim_end_matches('=').chars().map(|c| match c { '+' => '-', '/' => '_', c => c }).collect();
    crate::crypto::unb64url(&url)
}

/// The script that hands the page a message from the program (JSON), for a shell that runs
/// script in the page: an event, as `SHELL_JS` listens for.
pub fn deliver_js(json: &str) -> String {
    format!("document.dispatchEvent(new CustomEvent(\"quillo-wire-receive\", {{ detail: {} }}))", json)
}

/// The shell script for a system: `post` is its way to send a string, `bodies` "bridge" or
/// "bytes" (see `SHELL_JS`).
pub fn shell_js(post: &str, bodies: &str) -> String {
    SHELL_JS.replace("__QUILLO_POST__", post).replace("__QUILLO_BODIES__", bodies)
}
