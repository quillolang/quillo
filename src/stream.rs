//! Answers sent as they are made: `stream` and `events`.
//!
//! ```text
//! route GET /export = stream((send) => {
//!     for row in db.query("select * from orders") { send(csv_line(row)) }
//! }, { type: "text/csv" })
//!
//! route GET /progress = events((send) => {
//!     for step in range(10) { work(step); send({ done: step + 1, of: 10 }) }
//! })
//! ```
//!
//! The route gives back the answer's status and headers at once, and its
//! function then runs with `send`: each call goes to the client as it is
//! made (a chunk of a chunked answer), and waits only when the client reads
//! slower than it is written. When the client goes away (or the server
//! stops), the next `send` ends the function. `events` sends Server-Sent
//! Events: each value as JSON in an event's data, so pages (`listen`,
//! `receive`) get back the same value.
//!
//! Outside the server (a program's own `Interp::handle`, as in tests) the
//! whole answer is made first and sent as one.

use crate::interp::{fail, Ctrl, Interp};
use crate::value::*;
use std::cell::RefCell;
use std::sync::Arc;

type R = Result<Value, Ctrl>;

pub const BUILTINS: &[(&str, BuiltinFn)] = &[("stream", stream), ("events", events), ("listen", listen), ("receive", receive)];

/// The header that marks a response as one to stream (taken off before it is sent).
pub const MARK: &str = "x-quillo-stream";

/// What `send`'s writes go to: the connection, or a buffer.
pub trait Sink {
    /// Writes and sends `data`; false when the client is gone.
    fn write(&mut self, data: &[u8]) -> bool;
}

/// The stream a function is sending on.
pub(crate) struct Out {
    sink: SinkPtr,
    events: bool,
    chunked: bool,
    gone: bool,
}

/// Set only while the request's function runs, on the thread running it
/// (a fork for `parallel` starts without one).
struct SinkPtr(*mut dyn Sink);
unsafe impl Send for SinkPtr {}
unsafe impl Sync for SinkPtr {}

/// A response's function, made when the route's answer was (`render_response`).
pub(crate) struct Pending {
    pub f: Value,
    pub events: bool,
    /// A page whose waiting components follow it, instead of a function
    /// (src/defer.rs; branch defer).
    pub page: Option<crate::defer::Page>,
}

thread_local! {
    /// Set by `render_response` and taken at once by `Interp::handle`, with
    /// nothing in between that could let another request run on the thread.
    static MADE: RefCell<Option<Pending>> = const { RefCell::new(None) };
}

pub(crate) fn made(f: Value, events: bool) {
    MADE.with(|m| *m.borrow_mut() = Some(Pending { f, events, page: None }));
}

pub(crate) fn take_made() -> Option<Pending> {
    MADE.with(|m| m.borrow_mut().take())
}

/// What `send` says when the client is gone, ending the function.
const GONE: &str = "the client went away";

fn response(args: &[Value], name: &str, events: bool, line: u32) -> R {
    let f = match args.first() {
        Some(f @ (Value::Func(_) | Value::Builtin(..))) => f.clone(),
        other => {
            return fail(
                line,
                format!("`{}` takes a function that sends, as in {}((send) => send(\"...\")), got {}", name, name, other.map_or("nothing", |v| v.type_name())),
            )
        }
    };
    let opts = match args.get(1) {
        None | Some(Value::Nil) => Map::new(),
        Some(Value::Map(m)) => (**m).clone(),
        Some(other) => return fail(line, format!("`{}` takes options as a map, got {}", name, other.type_name())),
    };
    let status = match opts.get("status") {
        None | Some(Value::Nil) => 200,
        Some(Value::Int(n)) if (200..600).contains(n) => *n as u16,
        Some(other) => return fail(line, format!("`{}`: status must be an int from 200 to 599, got {}", name, other.repr())),
    };
    let ctype = match opts.get("type") {
        Some(Value::Str(t)) if !events => t.to_string(),
        _ if events => "text/event-stream".into(),
        _ => "text/plain; charset=utf-8".into(),
    };
    let mut headers = vec![("content-type".to_string(), ctype)];
    if events {
        headers.push(("cache-control".into(), "no-cache".into()));
    }
    // Proxies such as nginx would otherwise hold the answer back until it ends.
    headers.push(("x-accel-buffering".into(), "no".into()));
    if let Some(Value::Map(h)) = opts.get("headers") {
        for (k, v) in h.iter() {
            headers.retain(|(n, _)| !n.eq_ignore_ascii_case(k.as_str()));
            headers.push((k.to_string(), v.display()));
        }
    }
    Ok(Value::Response(Arc::new(Response { status, headers, body: Body::Stream { f, events } })))
}

/// `stream(f, { type, status, headers })`: an answer sent as `f` makes it.
fn stream(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    response(&args, "stream", false, line)
}

/// `events(f, { status, headers })`: Server-Sent Events, as `f` sends them.
fn events(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    response(&args, "events", true, line)
}

/// `listen(url, f)` and `receive(url, f, opts)` read streams in the
/// browser (src/client/runtime.js); rendering on the server, they do nothing.
fn listen(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    match (args.first(), args.get(1)) {
        (Some(Value::Str(_)), Some(Value::Func(_) | Value::Builtin(..))) => Ok(Value::Nil),
        _ => fail(line, "`listen` takes a URL and a function: listen(\"/progress\", (p) => { done = p.done })"),
    }
}

fn receive(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    match (args.first(), args.get(1)) {
        (Some(Value::Str(_)), Some(Value::Func(_) | Value::Builtin(..))) => Ok(Value::Nil),
        _ => fail(line, "`receive` takes a URL and a function: receive(\"/reply\", (t) => { text = text + t })"),
    }
}

/// The bytes `send(v, event)` puts on the wire. A stream: text as it is,
/// other values as a line of JSON. Events: the value as JSON, as an event's
/// data (named `event` if given); `send(null)` is a comment, which keeps a
/// quiet connection open.
fn encode(v: &Value, event: Option<&Value>, events: bool) -> Vec<u8> {
    if !events {
        return match v {
            Value::Str(s) => s.as_bytes().to_vec(),
            Value::Html(h) => h.as_bytes().to_vec(),
            Value::Nil => Vec::new(),
            other => {
                let mut out = Vec::new();
                other.json_into(&mut out);
                out.push(b'\n');
                out
            }
        };
    }
    let mut out = Vec::new();
    if matches!(v, Value::Nil) && event.is_none() {
        out.extend_from_slice(b":\n\n");
        return out;
    }
    if let Some(e) = event.filter(|e| !matches!(e, Value::Nil)) {
        // One line: a name with a line break in it would end the field.
        let name: String = e.display().chars().filter(|c| *c != '\n' && *c != '\r').collect();
        out.extend_from_slice(b"event: ");
        out.extend_from_slice(name.as_bytes());
        out.push(b'\n');
    }
    // JSON has no raw line breaks, so the data is one line.
    out.extend_from_slice(b"data: ");
    v.json_into(&mut out);
    out.extend_from_slice(b"\n\n");
    out
}

/// `send(value, event = null)`, given to a stream's function.
fn send(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let Some(out) = it.stream_out.as_mut() else {
        return fail(line, "`send` works while a `stream` or `events` answer is being sent");
    };
    if out.gone {
        return fail(line, GONE);
    }
    let data = encode(args.first().unwrap_or(&Value::Nil), args.get(1), out.events);
    if data.is_empty() {
        return Ok(Value::Bool(true));
    }
    let framed = if out.chunked {
        let mut f = format!("{:x}\r\n", data.len()).into_bytes();
        f.extend_from_slice(&data);
        f.extend_from_slice(b"\r\n");
        f
    } else {
        data
    };
    // SAFETY: the sink outlives the function's run (`run`), which is when `out` is set.
    let ok = unsafe { (*out.sink.0).write(&framed) };
    if !ok || crate::httpd::stopping() {
        out.gone = true;
        return fail(line, GONE);
    }
    Ok(Value::Bool(true))
}

/// How a stream's function ended.
pub(crate) enum Ended {
    Done,
    /// The client went away: nothing more can be sent.
    Gone,
    /// The function failed, after the answer had begun.
    Failed,
}

/// Runs a stream's function with `send`, writing to `sink` (in chunks, for
/// a connection), and ends the answer.
/// One piece of an answer, framed as a chunk when the answer is chunked; false when the
/// client is gone.
fn piece(sink: &mut dyn Sink, data: &[u8], chunked: bool) -> bool {
    if data.is_empty() {
        return true;
    }
    if !chunked {
        return sink.write(data);
    }
    let mut f = format!("{:x}\r\n", data.len()).into_bytes();
    f.extend_from_slice(data);
    f.extend_from_slice(b"\r\n");
    sink.write(&f) && !crate::httpd::stopping()
}

/// A page with waiting components: the page, then each component's part as it is
/// rendered, then the end of the page (src/defer.rs; branch defer).
fn run_page(it: &mut Interp, page: crate::defer::Page, sink: &mut dyn Sink, chunked: bool) -> Ended {
    if !piece(sink, &page.first, chunked) {
        return Ended::Gone;
    }
    for d in &page.jobs {
        let part = crate::defer::resolve(it, d);
        if !piece(sink, part.as_bytes(), chunked) {
            return Ended::Gone;
        }
    }
    if !piece(sink, &page.tail, chunked) || (chunked && !sink.write(b"0\r\n\r\n")) {
        return Ended::Gone;
    }
    Ended::Done
}

pub(crate) fn run(it: &mut Interp, mut p: Pending, sink: &mut dyn Sink, chunked: bool) -> Ended {
    if let Some(page) = p.page.take() {
        return run_page(it, page, sink, chunked);
    }
    // SAFETY (lifetime): `stream_out` is cleared below, before `sink` goes.
    let sink: *mut (dyn Sink + '_) = sink;
    let sink: *mut dyn Sink = unsafe { std::mem::transmute(sink) };
    let saved = it.stream_out.replace(Out { sink: SinkPtr(sink), events: p.events, chunked, gone: false });
    let r = it.call(&p.f, vec![Value::Builtin("send", send)], 0);
    let out = std::mem::replace(&mut it.stream_out, saved).expect("set above");
    match r {
        _ if out.gone => Ended::Gone,
        Ok(_) | Err(Ctrl::Return(_)) | Err(Ctrl::Reply(_)) => {
            // SAFETY: as in `send`.
            if chunked && !unsafe { (*out.sink.0).write(b"0\r\n\r\n") } {
                return Ended::Gone;
            }
            Ended::Done
        }
        Err(Ctrl::Error(e)) => {
            eprintln!("quillo: error while streaming an answer: {}", e);
            Ended::Failed
        }
    }
}

/// A sink that keeps what is sent: the answer made whole, outside the server.
pub(crate) struct Buffer(pub Vec<u8>);

impl Sink for Buffer {
    fn write(&mut self, data: &[u8]) -> bool {
        self.0.extend_from_slice(data);
        true
    }
}
