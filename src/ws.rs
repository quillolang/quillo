//! Just enough WebSocket (RFC 6455) for the server to push messages to a
//! page: the handshake, and text and ping frames from server to client.
//!
//! Pages wait for changes (live data, and reloads in `--watch` mode) on a
//! WebSocket rather than an HTTP request, because browsers allow only six
//! HTTP/1.1 connections to a host, shared by every tab: a few tabs each
//! holding a long poll open would leave none for anything else, and calls
//! to the server would wait in the browser's queue. WebSockets do not count
//! toward that limit.

use std::io::Write;

/// Whether a request asks to become a WebSocket.
pub fn is_upgrade(rq: &tiny_http::Request) -> bool {
    rq.headers().iter().any(|h| h.field.equiv("upgrade") && h.value.as_str().eq_ignore_ascii_case("websocket"))
}

/// A WebSocket the server writes to. Each `send` reports whether the page
/// is still there.
pub struct Socket(Box<dyn tiny_http::ReadWrite + Send>);

impl Socket {
    /// Completes the handshake; `None` if the request is not a valid one.
    pub fn accept(rq: tiny_http::Request) -> Option<Socket> {
        let key = rq.headers().iter().find(|h| h.field.equiv("sec-websocket-key"))?.value.as_str().trim().to_string();
        let accept = base64(&sha1(format!("{}258EAFA5-E914-47DA-95CA-C5AB0DC85B11", key).as_bytes()));
        let resp = tiny_http::Response::empty(101)
            .with_header(tiny_http::Header::from_bytes(&b"sec-websocket-accept"[..], accept.as_bytes()).ok()?);
        Some(Socket(rq.upgrade("websocket", resp)))
    }

    pub fn send(&mut self, text: &str) -> bool {
        self.frame(0x1, text.as_bytes())
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

fn sha1(data: &[u8]) -> [u8; 20] {
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

fn base64(data: &[u8]) -> String {
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
}
