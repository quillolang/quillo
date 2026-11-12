//! Just enough WebSocket (RFC 6455) for the server and a page to talk: the
//! handshake, frames from the server, and messages from the page (which
//! channels use when WebTransport cannot be had; see `wt`).
//!
//! Pages wait for changes (live data, and reloads in `--watch` mode) on a
//! WebSocket rather than an HTTP request, because browsers allow only six
//! HTTP/1.1 connections to a host, shared by every tab: a few tabs each
//! holding a long poll open would leave none for anything else, and calls
//! to the server would wait in the browser's queue. WebSockets do not count
//! toward that limit.

use std::io::{Read, Write};

use crate::server::HttpRequest;

/// Whether a request asks to become a WebSocket.
pub fn is_upgrade(req: &HttpRequest) -> bool {
    req.headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("upgrade") && v.eq_ignore_ascii_case("websocket"))
}

/// A WebSocket the server writes to. Each `send` reports whether the page
/// is still there.
/// The connection, how long one frame may take to write (none: as long as it takes), and its
/// watch (see `watch`).
pub struct Socket(std::net::TcpStream, Option<std::time::Duration>, Option<watch::Watching>);

impl Drop for Socket {
    /// The watch ends before the connection closes: the descriptor's number, free once it is
    /// closed, may be the next connection's, which the watch's end would have taken from the
    /// watcher.
    fn drop(&mut self) {
        self.2 = None;
    }
}

/// What a page sent on a socket the server writes to (`Socket::heard`).
#[derive(Debug, PartialEq)]
pub enum Heard {
    /// Nothing more has come.
    Nothing,
    /// A text message, its parts given as they were read; false when it was passed over (not masked
    /// as a page masks it, or a new message began before it ended), and what was given of it is to
    /// be dropped.
    Text(bool),
    /// The page has gone: it closed (answered with a close of its code, and the connection shut),
    /// hung up, or stopped in the middle of a frame for longer than it was given; or it was let go,
    /// with a close of 1009 for a text message longer than asked for, or of 1002 for a control frame
    /// longer than one may be.
    Gone,
}

impl Socket {
    /// Completes the handshake on the request's connection; `None` if the
    /// request is not a valid one.
    pub fn accept(req: &HttpRequest, mut stream: std::net::TcpStream) -> Option<Socket> {
        // A GET, asking for an upgrade, in version 13, as RFC 6455 §4.2.1 asks; told so with a
        // 426 or a 400 otherwise (an HTTP/1.0 request's `Upgrade` is dropped as it is read)
        // (FEEDBACK36.md, Q-445; branch feedback36-39).
        let header = |name: &str| req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.trim());
        let upgrading = header("connection").is_some_and(|v| v.split(',').any(|t| t.trim().eq_ignore_ascii_case("upgrade")));
        let refusal = if req.method != "GET" || !upgrading || header("sec-websocket-key").is_none() {
            Some("HTTP/1.1 400 Bad Request\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
        } else if header("sec-websocket-version") != Some("13") {
            Some("HTTP/1.1 426 Upgrade Required\r\nsec-websocket-version: 13\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
        } else {
            None
        };
        if let Some(r) = refusal {
            let _ = stream.write_all(r.as_bytes());
            return None;
        }
        let key = header("sec-websocket-key")?.to_string();
        let accept = base64(&sha1(format!("{}258EAFA5-E914-47DA-95CA-C5AB0DC85B11", key).as_bytes()));
        let handshake = format!(
            "HTTP/1.1 101 Switching Protocols\r\nupgrade: websocket\r\nconnection: Upgrade\r\nsec-websocket-accept: {}\r\n\r\n",
            accept
        );
        stream.write_all(handshake.as_bytes()).ok()?;
        Some(Socket(stream, None, None))
    }

    /// Calls `heard`, on the watcher's thread, each time the page sends something or hangs up, so
    /// that a thread that only writes to the socket hears of it at once (`heard` reads it). False
    /// where a socket cannot be watched (Windows), where it is looked at now and then instead
    /// (FEEDBACK61.md, Q-900; branch feedback61).
    pub fn watch(&mut self, heard: std::sync::Arc<dyn Fn() + Send + Sync>) -> bool {
        self.2 = watch::Watching::new(&self.0, heard);
        self.2.is_some()
    }

    /// What the page sent, read now: a ping is answered, a pong dropped, a close answered with a
    /// close of its code (RFC 6455 §5.5.1) and the connection shut, and a text message's parts are
    /// given to `f` as they are read, never held whole, up to `max` bytes (a longer one, or a control
    /// frame longer than one may be, fails the connection); a binary message, and a frame of no
    /// message, are passed over, as a waiting page's frames always were. Once a frame
    /// has begun, the rest of it may take `wait`. A page waiting for changes sends only pongs, its
    /// close and its topics, and a close the server took only at its next look, every 5 s, ended as
    /// 1006, a failed connection, after which Firefox held back the page's next socket for 10 to 45
    /// s and more (FEEDBACK61.md, Q-900; branch feedback61).
    ///
    /// Not inlined, nor what it calls: their buffers would be in the frame of every waiting thread,
    /// which mostly never reads.
    #[inline(never)]
    pub fn heard(&mut self, max: usize, wait: std::time::Duration, mut f: impl FnMut(&[u8])) -> Heard {
        let mut timed = false;
        let got = loop {
            match self.pending() {
                None => break Heard::Gone,
                Some(false) => break Heard::Nothing,
                Some(true) => {}
            }
            if !timed {
                if self.0.set_read_timeout(Some(wait)).is_err() {
                    break Heard::Gone;
                }
                timed = true;
            }
            match self.frame_heard(max, &mut f) {
                Ok(Some(h)) => break h,
                Ok(None) => {}
                Err(_) => break Heard::Gone,
            }
        };
        if got == Heard::Gone {
            let _ = self.0.shutdown(std::net::Shutdown::Both);
        } else if timed {
            let _ = self.0.set_read_timeout(None);
        }
        got
    }

    /// Whether something has come from the page (`None`: it hung up), without waiting.
    fn pending(&mut self) -> Option<bool> {
        if self.0.set_nonblocking(true).is_err() {
            return None;
        }
        let mut b = [0u8; 1];
        let got = loop {
            match self.0.peek(&mut b) {
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                r => break r,
            }
        };
        let _ = self.0.set_nonblocking(false);
        match got {
            Ok(0) => None,
            Ok(_) => Some(true),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Some(false),
            Err(_) => None,
        }
    }

    /// One frame of the page's, and for a text message's first, the rest of the message; what
    /// `heard` says of it, or `None` to read on.
    #[inline(never)]
    fn frame_heard(&mut self, max: usize, f: &mut impl FnMut(&[u8])) -> std::io::Result<Option<Heard>> {
        let (fin, op, mask, len) = self.frame_head()?;
        if op & 0x8 != 0 {
            return self.control(op, mask, len).map(|gone| gone.then_some(Heard::Gone));
        }
        // A message's first frame, text and masked, is read; the rest is passed over.
        if op != 0x1 || mask.is_none() {
            self.skip(len)?;
            return Ok((op == 0x1).then_some(Heard::Text(false)));
        }
        let (mut taken, mut total, mut fin, mut mask, mut len) = (true, 0u64, fin, mask, len);
        loop {
            total = total.saturating_add(len);
            // Longer than it may be: the connection is failed with 1009 (message too big), unread,
            // as a first message too long is, where it was passed over and its page, which took it
            // for followed, never heard of its topics (found in review of FEEDBACK61.md, Q-900;
            // branch feedback61).
            if taken && total > max as u64 {
                self.ending(Some(1009));
                return Ok(Some(Heard::Gone));
            }
            match (taken, mask) {
                (true, Some(m)) => self.unmasked(len, m, f)?,
                _ => self.skip(len)?,
            }
            if fin {
                return Ok(Some(Heard::Text(taken)));
            }
            // Its next part; control frames may come between the parts.
            loop {
                let (next_fin, op, next_mask, next_len) = self.frame_head()?;
                if op & 0x8 != 0 {
                    if self.control(op, next_mask, next_len)? {
                        return Ok(Some(Heard::Gone));
                    }
                    continue;
                }
                if op != 0x0 {
                    // A new message before this one ended: this one is passed over, and so is that.
                    self.skip(next_len)?;
                    return Ok(Some(Heard::Text(false)));
                }
                (fin, mask, len) = (next_fin, next_mask, next_len);
                if mask.is_none() {
                    taken = false;
                }
                break;
            }
        }
    }

    /// A frame's head: whether it ends its message, its opcode, its mask (none if not masked), and
    /// its length. A frame with reserved bits set counts as not masked: it is passed over.
    fn frame_head(&mut self) -> std::io::Result<(bool, u8, Option<[u8; 4]>, u64)> {
        let mut r = &self.0;
        let mut head = [0u8; 2];
        r.read_exact(&mut head)?;
        let len = match head[1] & 0x7f {
            126 => {
                let mut b = [0u8; 2];
                r.read_exact(&mut b)?;
                u16::from_be_bytes(b) as u64
            }
            127 => {
                let mut b = [0u8; 8];
                r.read_exact(&mut b)?;
                u64::from_be_bytes(b)
            }
            n => n as u64,
        };
        let mut mask = None;
        if head[1] & 0x80 != 0 {
            let mut m = [0u8; 4];
            r.read_exact(&mut m)?;
            mask = Some(m);
        }
        Ok((head[0] & 0x80 != 0, head[0] & 0x0f, mask.filter(|_| head[0] & 0x70 == 0), len))
    }

    /// A control frame: a ping answered, a pong (or one the page may not send) dropped, a close
    /// answered. True if the page has gone.
    fn control(&mut self, op: u8, mask: Option<[u8; 4]>, len: u64) -> std::io::Result<bool> {
        // Longer than a control frame may be (RFC 6455 §5.5): the connection is failed with 1002,
        // where it was passed over and a close of 126 bytes went unanswered (found in review of
        // FEEDBACK61.md, Q-900; branch feedback61).
        if len > 125 {
            self.ending(Some(1002));
            return Ok(true);
        }
        let mut data = [0u8; 125];
        let data = &mut data[..len as usize];
        (&self.0).read_exact(data)?;
        for (i, b) in data.iter_mut().enumerate() {
            *b ^= mask.map_or(0, |m| m[i % 4]);
        }
        match op {
            0x8 => {
                self.closing(data);
                Ok(true)
            }
            0x9 => Ok(!self.pong(data)),
            _ => Ok(false),
        }
    }

    /// The answer to the page's close: its code (none if it gave none), or 1002 for one a page may
    /// not send; then the connection is shut.
    fn closing(&mut self, theirs: &[u8]) {
        let code = match theirs {
            [] => None,
            [a, b, ..] => Some(u16::from_be_bytes([*a, *b])).filter(|c| matches!(c, 1000..=1003 | 1007..=1014 | 3000..=4999)).or(Some(1002)),
            [_] => Some(1002),
        };
        self.ending(code);
    }

    /// A close of `code` (none: a close with no code), then the connection shut.
    fn ending(&mut self, code: Option<u16>) {
        self.write_timeout(std::time::Duration::from_secs(2));
        match code {
            Some(c) => self.frame(0x8, &c.to_be_bytes()),
            None => self.frame(0x8, &[]),
        };
        let _ = self.0.shutdown(std::net::Shutdown::Both);
    }

    /// `len` bytes of a frame, unmasked and given to `f` a part at a time.
    fn unmasked(&mut self, len: u64, mask: [u8; 4], f: &mut impl FnMut(&[u8])) -> std::io::Result<()> {
        let mut buf = [0u8; 4096];
        let (mut left, mut at) = (len, 0usize);
        while left > 0 {
            let n = (left as usize).min(buf.len());
            (&self.0).read_exact(&mut buf[..n])?;
            for (i, b) in buf[..n].iter_mut().enumerate() {
                *b ^= mask[(at + i) % 4];
            }
            f(&buf[..n]);
            left -= n as u64;
            at += n;
        }
        Ok(())
    }

    /// `len` bytes of a frame, read and dropped.
    fn skip(&mut self, len: u64) -> std::io::Result<()> {
        let mut buf = [0u8; 1024];
        let mut left = len;
        while left > 0 {
            let n = (left as usize).min(buf.len());
            (&self.0).read_exact(&mut buf[..n])?;
            left -= n as u64;
        }
        Ok(())
    }

    pub fn send(&mut self, text: &str) -> bool {
        self.frame(0x1, text.as_bytes())
    }

    pub fn send_binary(&mut self, data: &[u8]) -> bool {
        self.frame(0x2, data)
    }

    pub fn pong(&mut self, data: &[u8]) -> bool {
        self.frame(0xA, data)
    }

    /// The same connection, to read from on another thread while this one
    /// writes.
    /// How long one frame may take to write to a page that does not read, after which it
    /// fails and the connection ends: for the whole frame, as a page that takes a few bytes
    /// now and then would hold each write's own timeout off (FEEDBACK49.md, Q-594; branch main).
    pub fn write_timeout(&mut self, d: std::time::Duration) {
        self.1 = Some(d);
        let _ = self.0.set_write_timeout(Some(d));
    }

    /// The first message the page sends, when it is text of at most `max` bytes that comes
    /// within `wait`: what a live page follows, when that is too long for its address
    /// (FEEDBACK55.md, Q-801; branch feedback55). Handed to `f` a part at a time as it is read,
    /// never held whole: a message of 4 MiB held for each socket took 8 MiB a socket, 2 GiB for
    /// one visitor's 256 (found in review). False if it is not such a message (or none came).
    pub fn first_text_parts(&mut self, max: usize, wait: std::time::Duration, mut f: impl FnMut(&[u8])) -> bool {
        if self.0.set_read_timeout(Some(wait)).is_err() {
            return false;
        }
        let ok = (|| -> std::io::Result<bool> {
            let mut r = &self.0;
            let (mut total, mut first) = (0u64, true);
            loop {
                let mut head = [0u8; 2];
                r.read_exact(&mut head)?;
                let (fin, opcode) = (head[0] & 0x80 != 0, head[0] & 0x0f);
                // Text, then its continuations, masked, as a page sends them; nothing else first.
                if head[0] & 0x70 != 0 || head[1] & 0x80 == 0 || opcode != if first { 0x1 } else { 0x0 } {
                    return Ok(false);
                }
                first = false;
                let len = match head[1] & 0x7f {
                    126 => {
                        let mut b = [0u8; 2];
                        r.read_exact(&mut b)?;
                        u16::from_be_bytes(b) as u64
                    }
                    127 => {
                        let mut b = [0u8; 8];
                        r.read_exact(&mut b)?;
                        u64::from_be_bytes(b)
                    }
                    n => n as u64,
                };
                total = total.saturating_add(len);
                if total > max as u64 {
                    return Ok(false);
                }
                let mut mask = [0u8; 4];
                r.read_exact(&mut mask)?;
                let mut buf = [0u8; 4096];
                let (mut left, mut at) = (len as usize, 0usize);
                while left > 0 {
                    let n = left.min(buf.len());
                    r.read_exact(&mut buf[..n])?;
                    for (i, b) in buf[..n].iter_mut().enumerate() {
                        *b ^= mask[(at + i) % 4];
                    }
                    f(&buf[..n]);
                    (left, at) = (left - n, at + n);
                }
                if fin {
                    return Ok(true);
                }
            }
        })()
        .unwrap_or(false);
        let _ = self.0.set_read_timeout(None);
        ok
    }

    pub fn reader(&self, max: usize) -> std::io::Result<Reader<std::net::TcpStream>> {
        Ok(Reader::new(self.0.try_clone()?, max))
    }

    /// An empty ping: the browser answers it, and if the page has gone the
    /// write fails, so a waiting thread finds out and ends.
    pub fn ping(&mut self) -> bool {
        self.frame(0x9, &[])
    }

    /// Whether the page has gone, for a thread that only writes: what the
    /// page sent is read here, pongs to drop, or a close. A write would learn of
    /// it up to two pings later, and a place would stay taken till then
    /// (FEEDBACK24.md, Q-194; branch feedback24-fixes).
    pub fn gone(&mut self) -> bool {
        if self.0.set_nonblocking(true).is_err() {
            return false;
        }
        let mut buf = [0u8; 512];
        let mut gone = false;
        for _ in 0..16 {
            match self.0.read(&mut buf) {
                Ok(0) => gone = true,
                Ok(n) => gone = closes(&buf[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => gone = true,
            }
            if gone {
                break;
            }
        }
        let _ = self.0.set_nonblocking(false);
        gone
    }

    pub fn close(mut self) {
        self.frame(0x8, &[]);
    }

    /// The end of a channel's connection: a close frame when the last write went through (not
    /// in the middle of one that did not), then the socket shut both ways, which wakes its
    /// reader too. Only dropping it left the reader's copy open, and a page that neither read
    /// nor closed kept a thread and a socket for good (FEEDBACK49.md, Q-594; found in review).
    pub fn end(mut self, clean: bool) {
        if clean {
            self.write_timeout(std::time::Duration::from_secs(2));
            self.frame(0x8, &[]);
        }
        let _ = self.0.shutdown(std::net::Shutdown::Both);
    }

    fn frame(&mut self, opcode: u8, payload: &[u8]) -> bool {
        let mut f = Vec::with_capacity(payload.len() + 10);
        frame_into(&mut f, opcode, payload);
        self.write_frames(&f)
    }

    /// Frames, each an opcode (0x1 text, 0x2 binary, 0x9 ping, 0xA pong) and its payload, in one
    /// write: what a page has waiting, not a system call each, with each frame's head made here
    /// and its payload written from where it is, nothing copied (see `wt::over_websocket`;
    /// FEEDBACK55.md, Q-806; branch feedback55). At most 64.
    pub fn send_all(&mut self, frames: &[(u8, &[u8])]) -> bool {
        const MOST: usize = 64;
        let frames = &frames[..frames.len().min(MOST)];
        let mut heads = [[0u8; 10]; MOST];
        let mut lens = [0usize; MOST];
        for (i, (op, payload)) in frames.iter().enumerate() {
            lens[i] = head_into(&mut heads[i], *op, payload.len());
        }
        let mut slices = [std::io::IoSlice::new(&[]); 2 * MOST];
        for (i, (_, payload)) in frames.iter().enumerate() {
            slices[2 * i] = std::io::IoSlice::new(&heads[i][..lens[i]]);
            slices[2 * i + 1] = std::io::IoSlice::new(payload);
        }
        let mut rest = &mut slices[..2 * frames.len()];
        let wait = self.1;
        let until = wait.map(|w| std::time::Instant::now() + w);
        let (mut part, mut shortened) = (false, false);
        let ok = loop {
            while rest.first().is_some_and(|s| s.is_empty()) {
                rest = &mut rest[1..];
            }
            if rest.is_empty() {
                break true;
            }
            // As `write_frames`: the deadline is for the whole write, and set again only after a
            // part of it was written.
            if let (true, Some(until)) = (part, until) {
                let left = until.saturating_duration_since(std::time::Instant::now());
                if left.is_zero() || self.0.set_write_timeout(Some(left)).is_err() {
                    break false;
                }
                shortened = true;
            }
            match self.0.write_vectored(rest) {
                Ok(0) => break false,
                Ok(n) => {
                    std::io::IoSlice::advance_slices(&mut rest, n);
                    part = true;
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break false,
            }
        };
        if shortened {
            let _ = self.0.set_write_timeout(wait);
        }
        ok
    }

    /// Frames made with `frame_into`, written at once.
    pub fn write_frames(&mut self, f: &[u8]) -> bool {
        let Some(wait) = self.1 else {
            return self.0.write_all(f).and_then(|_| self.0.flush()).is_ok();
        };
        // The socket's own timeout is `wait` already: set again only after a part was written,
        // to what is left, and put back after, so a frame written whole costs no system call
        // more (found in review).
        let until = std::time::Instant::now() + wait;
        let mut rest = f;
        let mut shortened = false;
        let ok = loop {
            if rest.is_empty() {
                break true;
            }
            if shortened || rest.len() < f.len() {
                let left = until.saturating_duration_since(std::time::Instant::now());
                if left.is_zero() || self.0.set_write_timeout(Some(left)).is_err() {
                    break false;
                }
                shortened = true;
            }
            match self.0.write(rest) {
                Ok(0) => break false,
                Ok(n) => rest = &rest[n..],
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break false,
            }
        };
        if shortened {
            let _ = self.0.set_write_timeout(Some(wait));
        }
        ok
    }
}

/// Sockets whose threads only write, watched on one thread of their own for what their pages send
/// (`Socket::watch`): with a thread a socket, a page's close was taken only when its thread next
/// looked, every 5 s (FEEDBACK61.md, Q-900; branch feedback61).
mod watch {
    pub(super) type Told = std::sync::Arc<dyn Fn() + Send + Sync>;

    #[cfg(unix)]
    pub(super) use unix::Watching;

    #[cfg(not(unix))]
    pub(super) struct Watching;

    #[cfg(not(unix))]
    impl Watching {
        pub(super) fn new(_: &std::net::TcpStream, _: Told) -> Option<Watching> {
            None
        }
    }

    #[cfg(unix)]
    mod unix {
        use super::Told;
        use std::collections::HashMap;
        use std::mem::ManuallyDrop;
        use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd};
        use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
        use std::sync::{Mutex, OnceLock};

        struct Watcher {
            registry: mio::Registry,
            /// Whom to tell, by the token their socket has.
            told: Mutex<HashMap<usize, Told>>,
            next: AtomicUsize,
        }

        static WATCHER: OnceLock<Option<Watcher>> = OnceLock::new();

        fn watcher() -> Option<&'static Watcher> {
            WATCHER
                .get_or_init(|| {
                    let poll = mio::Poll::new().ok()?;
                    let registry = poll.registry().try_clone().ok()?;
                    std::thread::Builder::new().name("quillo-ws-watch".into()).stack_size(256 << 10).spawn(move || watching(poll)).ok()?;
                    Some(Watcher { registry, told: Mutex::new(HashMap::new()), next: AtomicUsize::new(1) })
                })
                .as_ref()
        }

        fn watching(mut poll: mio::Poll) {
            let mut events = mio::Events::with_capacity(256);
            loop {
                if let Err(e) = poll.poll(&mut events, None) {
                    if e.kind() != std::io::ErrorKind::Interrupted {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                    continue;
                }
                let Some(w) = WATCHER.get().and_then(Option::as_ref) else { continue };
                let told = w.told.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                for e in events.iter() {
                    if let Some(f) = told.get(&e.token().0) {
                        f();
                    }
                }
            }
        }

        /// A socket watched. Its descriptor is the socket's own, not a copy, which would take one
        /// more of those a server may have for each page: the watch never closes it.
        pub(in super::super) struct Watching {
            fd: ManuallyDrop<mio::net::TcpStream>,
            token: usize,
        }

        impl Watching {
            pub(in super::super) fn new(s: &std::net::TcpStream, told: Told) -> Option<Watching> {
                let w = watcher()?;
                let token = w.next.fetch_add(1, Relaxed);
                // Before it is watched: what has come already is told at once.
                w.told.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(token, told);
                // SAFETY: the socket's open descriptor, which the watch gives back unclosed when it
                // ends (see `drop`), before the socket closes it (`Socket`'s own drop).
                let mut fd = ManuallyDrop::new(unsafe { mio::net::TcpStream::from_raw_fd(s.as_raw_fd()) });
                if w.registry.register(&mut *fd, mio::Token(token), mio::Interest::READABLE).is_err() {
                    w.told.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&token);
                    let _ = ManuallyDrop::into_inner(fd).into_raw_fd();
                    return None;
                }
                Some(Watching { fd, token })
            }
        }

        impl Drop for Watching {
            fn drop(&mut self) {
                let Some(w) = WATCHER.get().and_then(Option::as_ref) else { return };
                let _ = w.registry.deregister(&mut *self.fd);
                w.told.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&self.token);
                // SAFETY: taken once, here; the descriptor is given back, not closed.
                let _ = unsafe { ManuallyDrop::take(&mut self.fd) }.into_raw_fd();
            }
        }
    }
}

/// A frame of the server's (not masked) put at the end of `f`: an opcode (0x1 text, 0x2 binary,
/// 0x9 ping, 0xA pong) and its payload.
pub fn frame_into(f: &mut Vec<u8>, opcode: u8, payload: &[u8]) {
    let mut head = [0u8; 10];
    let n = head_into(&mut head, opcode, payload.len());
    f.extend_from_slice(&head[..n]);
    f.extend_from_slice(payload);
}

/// A frame's head, for a payload of `len` bytes, in `h`; how many bytes it takes.
fn head_into(h: &mut [u8; 10], opcode: u8, len: usize) -> usize {
    h[0] = 0x80 | opcode;
    match len {
        n if n < 126 => {
            h[1] = n as u8;
            2
        }
        n if n <= 0xffff => {
            h[1] = 126;
            h[2..4].copy_from_slice(&(n as u16).to_be_bytes());
            4
        }
        n => {
            h[1] = 127;
            h[2..10].copy_from_slice(&(n as u64).to_be_bytes());
            10
        }
    }
}

/// Whether short frames from a page hold a close (a waiting page sends only
/// those and pongs, each a few bytes).
fn closes(mut wire: &[u8]) -> bool {
    while wire.len() >= 2 {
        if wire[0] & 0x0f == 0x8 {
            return true;
        }
        let len = (wire[1] & 0x7f) as usize;
        if len >= 126 {
            return false;
        }
        wire = wire.get(6 + len..).unwrap_or(&[]);
    }
    false
}

/// A message from the page.
#[derive(Debug, PartialEq)]
pub enum Message {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong,
    Close,
}

/// Reads the page's messages: frames unmasked (a page's always are masked)
/// and put back together when sent in parts, with control frames, which may
/// come between the parts, given as they come.
pub struct Reader<R> {
    r: R,
    max: usize,
    /// A message sent in parts: its kind and what has come of it.
    partial: Option<(u8, Vec<u8>)>,
}

impl<R: Read> Reader<R> {
    pub fn new(r: R, max: usize) -> Reader<R> {
        Reader { r, max, partial: None }
    }

    /// The next message; an error when the connection ends, a frame is not
    /// one a page may send, or a message is larger than `max`.
    pub fn next(&mut self) -> std::io::Result<Message> {
        let bad = |why: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, why.to_string());
        loop {
            let mut head = [0u8; 2];
            self.r.read_exact(&mut head)?;
            let (fin, opcode, masked) = (head[0] & 0x80 != 0, head[0] & 0x0f, head[1] & 0x80 != 0);
            if head[0] & 0x70 != 0 {
                return Err(bad("reserved bits set"));
            }
            if !masked {
                return Err(bad("a page's frames are masked"));
            }
            let len = match head[1] & 0x7f {
                126 => {
                    let mut b = [0u8; 2];
                    self.r.read_exact(&mut b)?;
                    u16::from_be_bytes(b) as u64
                }
                127 => {
                    let mut b = [0u8; 8];
                    self.r.read_exact(&mut b)?;
                    u64::from_be_bytes(b)
                }
                n => n as u64,
            };
            // A control frame is short and whole (RFC 6455 5.5); a message's parts are counted
            // without wrapping, where a length of u64::MAX after a first part wrapped to 0 and
            // passed, and the allocation panicked the channel's thread (FEEDBACK49.md, Q-593;
            // branch main).
            if opcode & 0x8 != 0 && (len > 125 || !fin) {
                return Err(bad("a control frame longer than 125 bytes, or in parts"));
            }
            let so_far = if opcode == 0x0 { self.partial.as_ref().map_or(0, |(_, d)| d.len()) as u64 } else { 0 };
            if len.checked_add(so_far).map_or(true, |n| n > self.max as u64) {
                return Err(bad("message too large"));
            }
            let mut mask = [0u8; 4];
            self.r.read_exact(&mut mask)?;
            let mut data = vec![0u8; len as usize];
            self.r.read_exact(&mut data)?;
            for (i, b) in data.iter_mut().enumerate() {
                *b ^= mask[i % 4];
            }
            match opcode {
                0x8 => return Ok(Message::Close),
                0x9 => return Ok(Message::Ping(data)),
                0xA => return Ok(Message::Pong),
                0x0 => {
                    let Some((_, sofar)) = self.partial.as_mut() else { return Err(bad("a continuation of nothing")) };
                    sofar.extend_from_slice(&data);
                    if fin {
                        let (kind, all) = self.partial.take().unwrap_or_default();
                        return whole(kind, all);
                    }
                }
                0x1 | 0x2 => {
                    if self.partial.is_some() {
                        return Err(bad("a new message before the last one ended"));
                    }
                    if fin {
                        return whole(opcode, data);
                    }
                    self.partial = Some((opcode, data));
                }
                _ => return Err(bad("unknown opcode")),
            }
        }
    }
}

fn whole(kind: u8, data: Vec<u8>) -> std::io::Result<Message> {
    if kind == 0x2 {
        return Ok(Message::Binary(data));
    }
    String::from_utf8(data).map(Message::Text).map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "text that is not UTF-8"))
}

pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for block in msg.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([block[4 * i], block[4 * i + 1], block[4 * i + 2], block[4 * i + 3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 20];
    for (i, x) in h.iter().enumerate() {
        out[4 * i..4 * i + 4].copy_from_slice(&x.to_be_bytes());
    }
    out
}

pub fn hex(data: &[u8]) -> String {
    data.iter().map(|b| format!("{:02x}", b)).collect()
}

pub(crate) fn base64(data: &[u8]) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            s.push(if i <= c.len() { A[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    s
}

#[cfg(test)]
mod tests {
    #[test]
    fn handshake_key() {
        // The example in RFC 6455, section 1.3.
        let key = "dGhlIHNhbXBsZSBub25jZQ==258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
        assert_eq!(super::base64(&super::sha1(key.as_bytes())), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
        assert_eq!(super::base64(b"ab"), "YWI=");
        assert_eq!(super::base64(b"a"), "YQ==");
    }

    /// A frame as a page sends it: masked.
    fn frame(fin: bool, opcode: u8, data: &[u8]) -> Vec<u8> {
        let mask = [0x37, 0xfa, 0x21, 0x3d];
        let mut f = vec![if fin { 0x80 } else { 0 } | opcode];
        match data.len() {
            n if n < 126 => f.push(0x80 | n as u8),
            n if n <= 0xffff => {
                f.push(0x80 | 126);
                f.extend_from_slice(&(n as u16).to_be_bytes());
            }
            n => {
                f.push(0x80 | 127);
                f.extend_from_slice(&(n as u64).to_be_bytes());
            }
        }
        f.extend_from_slice(&mask);
        f.extend(data.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        f
    }

    #[test]
    fn messages_from_a_page() {
        use super::{Message, Reader};
        let big = vec![7u8; 70_000];
        let mut wire = Vec::new();
        wire.extend(frame(true, 0x1, b"Hello"));
        // In parts, with a ping between them.
        wire.extend(frame(false, 0x1, b"Hel"));
        wire.extend(frame(true, 0x9, b"p"));
        wire.extend(frame(true, 0x0, b"lo again"));
        wire.extend(frame(true, 0x2, &[0, 1, 2]));
        wire.extend(frame(true, 0x2, &big));
        wire.extend(frame(true, 0xA, b""));
        wire.extend(frame(true, 0x8, b""));
        let mut r = Reader::new(&wire[..], 1 << 20);
        assert_eq!(r.next().unwrap(), Message::Text("Hello".into()));
        assert_eq!(r.next().unwrap(), Message::Ping(b"p".to_vec()));
        assert_eq!(r.next().unwrap(), Message::Text("Hello again".into()));
        assert_eq!(r.next().unwrap(), Message::Binary(vec![0, 1, 2]));
        assert_eq!(r.next().unwrap(), Message::Binary(big));
        assert_eq!(r.next().unwrap(), Message::Pong);
        assert_eq!(r.next().unwrap(), Message::Close);
        assert!(r.next().is_err());
        // Unmasked, too large, or a continuation of nothing: refused.
        assert!(Reader::new(&[0x81u8, 0x01, b'x'][..], 100).next().is_err());
        assert!(Reader::new(&frame(true, 0x1, &[b'x'; 200])[..], 100).next().is_err());
        assert!(Reader::new(&frame(true, 0x0, b"x")[..], 100).next().is_err());
        assert!(Reader::new(&frame(true, 0x1, &[0xff, 0xfe])[..], 100).next().is_err());
    }
}
