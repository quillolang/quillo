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
pub struct Socket(std::net::TcpStream);

impl Socket {
    /// Completes the handshake on the request's connection; `None` if the
    /// request is not a valid one.
    pub fn accept(req: &HttpRequest, mut stream: std::net::TcpStream) -> Option<Socket> {
        let key = req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("sec-websocket-key"))?.1.trim().to_string();
        let accept = base64(&sha1(format!("{}258EAFA5-E914-47DA-95CA-C5AB0DC85B11", key).as_bytes()));
        let handshake = format!(
            "HTTP/1.1 101 Switching Protocols\r\nupgrade: websocket\r\nconnection: Upgrade\r\nsec-websocket-accept: {}\r\n\r\n",
            accept
        );
        stream.write_all(handshake.as_bytes()).ok()?;
        Some(Socket(stream))
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
    pub fn reader(&self, max: usize) -> std::io::Result<Reader<std::net::TcpStream>> {
        Ok(Reader::new(self.0.try_clone()?, max))
    }

    /// An empty ping: the browser answers it, and if the page has gone the
    /// write fails, so a waiting thread finds out and ends.
    pub fn ping(&mut self) -> bool {
        self.frame(0x9, &[])
    }

    pub fn close(mut self) {
        self.frame(0x8, &[]);
    }

    fn frame(&mut self, opcode: u8, payload: &[u8]) -> bool {
        let mut f = vec![0x80 | opcode];
        match payload.len() {
            n if n < 126 => f.push(n as u8),
            n if n <= 0xffff => {
                f.push(126);
                f.extend_from_slice(&(n as u16).to_be_bytes());
            }
            n => {
                f.push(127);
                f.extend_from_slice(&(n as u64).to_be_bytes());
            }
        }
        f.extend_from_slice(payload);
        self.0.write_all(&f).and_then(|_| self.0.flush()).is_ok()
    }
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
            let so_far = self.partial.as_ref().map_or(0, |(_, d)| d.len()) as u64;
            if len + so_far > self.max as u64 {
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
