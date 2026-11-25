//! WebTransport sessions whose HTTP/3 control frames come in pieces (FEEDBACK75.md, Q-1124; branch
//! feedback86-89). wtransport 0.7.2 read a peer's SETTINGS frame and its CONNECT stream's capsules
//! inside a `select!` that drops the read whenever a stream or a datagram arrives, losing what the
//! read had taken: a frame split across packets with a stream between was read again from its
//! middle, and the connection closed (0x105, 0x107) or the session never started. Quillo uses a
//! patched copy (vendor/wtransport-0.7.2). These tests speak HTTP/3 by hand over raw QUIC
//! (wtransport's own quinn), so that they can cut a frame where they like and open a stream or send
//! a datagram between the pieces.
#![cfg(feature = "realtime")]

mod support;

use std::borrow::Cow;
use std::time::Duration;
use wtransport::proto::frame::Frame;
use wtransport::proto::headers::Headers;
use wtransport::proto::session::SessionRequest;
use wtransport::quinn;
use wtransport::ClientConfig;

/// A page's channel, and what it heard.
const PROGRAM: &str = r#"let heard = []
channel.on("chat", (m, p) => {
    heard = heard + [m]
})
route GET /heard = heard
"#;

/// How long a test may take before it fails: a session that never starts would otherwise wait for
/// good, as on 0.8.11 (where the server's keep-alive held the connection open).
const LIMIT: Duration = Duration::from_secs(60);

/// What the tests wait between two pieces, so that the server has read the first before the next
/// thing comes.
const GAP: Duration = Duration::from_millis(40);

/// A server of `PROGRAM`, stopped and its folder removed when dropped.
struct Served(support::Server, std::path::PathBuf);

impl Drop for Served {
    fn drop(&mut self) {
        self.0.kill();
        let _ = std::fs::remove_dir_all(&self.1);
    }
}

impl std::ops::Deref for Served {
    type Target = support::Server;
    fn deref(&self) -> &support::Server {
        &self.0
    }
}

fn server(name: &str) -> Served {
    let d = support::temp_dir(&format!("f812-wt-{}", name));
    let f = d.join("main.qwe");
    std::fs::write(&f, PROGRAM).unwrap();
    Served(support::serve(&f, &[]), d)
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap()
}

/// Runs a test's steps, failing after `LIMIT`.
fn within<F: std::future::Future<Output = ()>>(what: &str, f: F) {
    rt().block_on(async {
        if tokio::time::timeout(LIMIT, f).await.is_err() {
            panic!("{}: not done within {:?}", what, LIMIT);
        }
    });
}

// ---- HTTP/3 by hand -------------------------------------------------------------------------

/// A QUIC variable-length integer (RFC 9000 §16).
fn varint(v: u64) -> Vec<u8> {
    match v {
        0..=0x3f => vec![v as u8],
        0x40..=0x3fff => ((v as u16) | 0x4000).to_be_bytes().to_vec(),
        0x4000..=0x3fff_ffff => ((v as u32) | 0x8000_0000).to_be_bytes().to_vec(),
        _ => (v | 0xc000_0000_0000_0000).to_be_bytes().to_vec(),
    }
}

/// An HTTP/3 frame: its type, its length, its payload.
fn frame(kind: u64, payload: &[u8]) -> Vec<u8> {
    let mut f = varint(kind);
    f.extend(varint(payload.len() as u64));
    f.extend_from_slice(payload);
    f
}

/// A browser's SETTINGS, in a fixed order, padded with settings no one knows (which a receiver
/// passes over) to 76 bytes, so that the frame's length takes two bytes and the payload holds
/// one-, four- and eight-byte integers to cut inside.
fn settings_payload() -> Vec<u8> {
    let mut p = Vec::new();
    for (id, value) in [
        (0x01, 0),          // QPACK_MAX_TABLE_CAPACITY
        (0x07, 0),          // QPACK_BLOCKED_STREAMS
        (0x08, 1),          // ENABLE_CONNECT_PROTOCOL
        (0x33, 1),          // H3_DATAGRAM
        (0x2b60_3742, 1),   // ENABLE_WEBTRANSPORT (draft 02)
        (0xc671_706a, 1),   // WEBTRANSPORT_MAX_SESSIONS
        (0x0a, 1u64 << 40), // unknown, from here on
        (0x0b, 1 << 40),
        (0x0c, 1 << 40),
        (0x0d, 1 << 40),
        (0x0e, 1 << 40),
        (0x0f, 1 << 40),
    ] {
        p.extend(varint(id));
        p.extend(varint(value));
    }
    assert_eq!(p.len(), 76);
    p
}

/// The control stream as a client writes it: its type (0x00), then SETTINGS.
fn control(settings: &[u8]) -> Vec<u8> {
    let mut c = vec![0x00];
    c.extend(frame(0x04, settings));
    c
}

/// A capsule (RFC 9297) in a DATA frame, as on a session's CONNECT stream.
fn capsule(kind: u64, payload: &[u8]) -> Vec<u8> {
    let mut c = varint(kind);
    c.extend(varint(payload.len() as u64));
    c.extend_from_slice(payload);
    frame(0x00, &c)
}

/// CLOSE_WEBTRANSPORT_SESSION (0x2843): an error code and a reason.
fn close_capsule(code: u32, reason: &str) -> Vec<u8> {
    let mut p = code.to_be_bytes().to_vec();
    p.extend_from_slice(reason.as_bytes());
    capsule(0x2843, &p)
}

/// A message on a page's stream, after its length.
fn framed(t: &str) -> Vec<u8> {
    let mut f = (t.len() as u32).to_be_bytes().to_vec();
    f.extend_from_slice(t.as_bytes());
    f
}

/// What comes between two pieces of a frame.
#[derive(Clone, Copy)]
enum Between {
    /// A uni stream of this type: 0x02 and 0x03 are QPACK's, which a browser opens beside its
    /// control stream and keeps open (closing one closes the connection); 0x21 is a reserved type
    /// the server must pass over, finished at once.
    Stream(u8),
    /// A datagram of the session's (quarter stream id 0) carrying this page message.
    Datagram(&'static str),
}

/// A started session's streams: the CONNECT stream's sending side, and the others held open.
struct Live {
    connect: quinn::SendStream,
    _streams: (quinn::RecvStream, quinn::SendStream, quinn::RecvStream),
}

struct Raw {
    _endpoint: quinn::Endpoint,
    conn: quinn::Connection,
    url: String,
    /// QPACK's streams, held open.
    qpack: std::sync::Mutex<Vec<quinn::SendStream>>,
}

fn e(x: impl std::fmt::Display) -> String {
    x.to_string()
}

impl Raw {
    /// A QUIC connection to the server's WebTransport port, with HTTP/3's ALPN and the server's
    /// certificate taken by its hash, as a page's.
    async fn connect(s: &support::Server) -> Raw {
        let a = support::send(s.port, "POST", "/_quillo/wt-ticket", &[], "");
        assert_eq!(a.status, 200, "{}", a.body);
        let t: serde_json::Value = serde_json::from_str(&a.body).unwrap();
        let port = t["port"].as_u64().expect("no port") as u16;
        let hex = t["hash"].as_str().expect("no hash");
        let bytes: Vec<u8> = (0..32).map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap()).collect();
        let hash = wtransport::tls::Sha256Digest::new(bytes.try_into().unwrap());
        let c = ClientConfig::builder().with_bind_default().with_server_certificate_hashes([hash]).build();
        let mut endpoint = quinn::Endpoint::client(([127, 0, 0, 1], 0).into()).unwrap();
        endpoint.set_default_client_config(c.quic_config().clone());
        let conn = endpoint.connect(([127, 0, 0, 1], port).into(), "localhost").unwrap().await.expect("no QUIC connection");
        let url = format!("https://127.0.0.1:{}/_quillo/wt?t={}", port, t["ticket"].as_str().unwrap());
        Raw { _endpoint: endpoint, conn, url, qpack: Default::default() }
    }

    async fn between(&self, b: Between) {
        match b {
            Between::Stream(kind) => {
                let mut s = self.conn.open_uni().await.unwrap();
                s.write_all(&[kind]).await.unwrap();
                if kind == 0x21 {
                    let _ = s.finish();
                } else {
                    self.qpack.lock().unwrap().push(s);
                }
            }
            Between::Datagram(text) => {
                let mut d = varint(0);
                d.extend_from_slice(text.as_bytes());
                self.conn.send_datagram(d.into()).unwrap();
            }
        }
        tokio::time::sleep(GAP).await;
    }

    /// Writes `pieces` on `s`, each after `between` (none before the first), waiting `GAP` after
    /// each so that the server reads it alone.
    async fn pieces(&self, s: &mut quinn::SendStream, pieces: &[&[u8]], between: &[Between]) {
        for (i, piece) in pieces.iter().enumerate() {
            if i > 0 {
                self.between(between[(i - 1) % between.len()]).await;
            }
            s.write_all(piece).await.unwrap();
            tokio::time::sleep(GAP).await;
        }
    }

    /// The CONNECT request of a WebTransport session, and the server's answer to it: the
    /// session's stream when it is 200, or what went wrong.
    async fn session(&self) -> Result<(quinn::SendStream, quinn::RecvStream), String> {
        let (mut out, mut inn) = self.conn.open_bi().await.map_err(e)?;
        let request = SessionRequest::new(&self.url).unwrap().headers().generate_frame();
        out.write_all(&frame(0x01, request.payload())).await.map_err(e)?;
        let kind = read_varint(&mut inn).await?;
        let len = read_varint(&mut inn).await?;
        let mut payload = vec![0; len as usize];
        inn.read_exact(&mut payload).await.map_err(e)?;
        assert_eq!(kind, 0x01, "the answer is not HEADERS");
        let headers = Headers::with_frame(&Frame::new_headers(Cow::Owned(payload))).unwrap();
        match headers.get(":status") {
            Some("200") => Ok((out, inn)),
            other => Err(format!("status {:?}", other)),
        }
    }

    /// The page's first stream (WEBTRANSPORT_STREAM 0x41 of session 0), on which it joins `chat`
    /// and is told so.
    async fn join(&self) -> Result<(quinn::SendStream, quinn::RecvStream), String> {
        let (mut out, mut inn) = self.conn.open_bi().await.map_err(e)?;
        let mut w = varint(0x41);
        w.extend(varint(0));
        w.extend(framed("[\"j\",\"chat\"]"));
        out.write_all(&w).await.map_err(e)?;
        let mut len = [0u8; 4];
        inn.read_exact(&mut len).await.map_err(e)?;
        let mut b = vec![0u8; u32::from_be_bytes(len) as usize];
        inn.read_exact(&mut b).await.map_err(e)?;
        assert_eq!(String::from_utf8(b).unwrap(), "[\"j\",\"chat\",true]");
        Ok((out, inn))
    }

    /// The session working end to end: started, then joined. Its streams are kept open: a
    /// stream let go would tell the server to stop sending on it.
    async fn works(&self) -> Result<Live, String> {
        let (connect, answer) = self.session().await?;
        let (page, page_in) = self.join().await?;
        Ok(Live { connect, _streams: (answer, page, page_in) })
    }

    /// The code the server closed the connection with.
    async fn closed_with(&self) -> String {
        match self.conn.closed().await {
            quinn::ConnectionError::ApplicationClosed(c) => format!("{:#x}", u64::from(c.error_code)),
            other => other.to_string(),
        }
    }
}

async fn read_varint(s: &mut quinn::RecvStream) -> Result<u64, String> {
    let mut b = [0u8; 8];
    s.read_exact(&mut b[..1]).await.map_err(e)?;
    let n = 1usize << (b[0] >> 6);
    s.read_exact(&mut b[1..n]).await.map_err(e)?;
    Ok(b[1..n].iter().fold((b[0] & 0x3f) as u64, |v, x| (v << 8) | *x as u64))
}

/// The pages' messages the program heard.
fn heard(s: &support::Server) -> String {
    support::send(s.port, "GET", "/heard", &[], "").body
}

// ---- SETTINGS in pieces ---------------------------------------------------------------------

/// A SETTINGS frame cut in its type, inside its two-byte length, after a byte of its payload and
/// inside a four-byte setting id, with QPACK's two streams and a reserved one opened between the
/// pieces, as a browser's packets can come after a loss: the session starts and the page joins.
/// On 0.8.11 the server read the frame again from each cut and closed the connection with
/// H3_FRAME_UNEXPECTED (0x105).
#[test]
fn a_settings_frame_in_pieces_with_streams_between_starts_the_session() {
    let s = server("pieces");
    within("settings in pieces", async {
        let raw = Raw::connect(&s).await;
        let c = control(&settings_payload());
        // [type 0x00, SETTINGS 0x04] [length's first byte] [its second, a payload byte] [6 bytes,
        // into 0x2b603742] [the rest]
        let cuts: [&[u8]; 5] = [&c[..2], &c[2..3], &c[3..5], &c[5..15], &c[15..]];
        let mut ctl = raw.conn.open_uni().await.unwrap();
        // A second QPACK stream of a kind would be refused (H3_STREAM_CREATION_ERROR): one each.
        let between = [Between::Stream(0x02), Between::Stream(0x03), Between::Stream(0x21), Between::Stream(0x21)];
        raw.pieces(&mut ctl, &cuts, &between).await;
        raw.works().await.unwrap_or_else(|why| panic!("the session did not start: {}", why));
    });
}

/// The same frame a byte at a time, a stream between each byte (the limit: every cut there is).
#[test]
fn a_settings_frame_a_byte_at_a_time_starts_the_session() {
    let s = server("bytes");
    within("settings a byte at a time", async {
        let raw = Raw::connect(&s).await;
        let c = control(&settings_payload());
        let cuts: Vec<&[u8]> = c.chunks(1).collect();
        let mut ctl = raw.conn.open_uni().await.unwrap();
        raw.pieces(&mut ctl, &cuts, &[Between::Stream(0x21)]).await;
        raw.works().await.unwrap_or_else(|why| panic!("the session did not start: {}", why));
    });
}

/// An empty SETTINGS frame (no setting at all is allowed) cut after its type, a stream between:
/// the session starts. On 0.8.11 the server lost the type, took the length 0 for a DATA frame's
/// type and waited for good.
#[test]
fn an_empty_settings_frame_cut_after_its_type_starts_the_session() {
    let s = server("empty");
    within("empty settings in two", async {
        let raw = Raw::connect(&s).await;
        let c = control(&[]);
        assert_eq!(c, [0x00, 0x04, 0x00]);
        let mut ctl = raw.conn.open_uni().await.unwrap();
        raw.pieces(&mut ctl, &[&c[..2], &c[2..]], &[Between::Stream(0x21)]).await;
        raw.works().await.unwrap_or_else(|why| panic!("the session did not start: {}", why));
    });
}

/// Datagrams also end the driver's wait: a SETTINGS frame cut once with a datagram between starts
/// the session too (the datagram, sent before the session, is the server's to drop or keep).
#[test]
fn a_settings_frame_cut_with_a_datagram_between_starts_the_session() {
    let s = server("dgram-settings");
    within("settings around a datagram", async {
        let raw = Raw::connect(&s).await;
        let c = control(&settings_payload());
        let mut ctl = raw.conn.open_uni().await.unwrap();
        raw.pieces(&mut ctl, &[&c[..5], &c[5..]], &[Between::Datagram("[\"m\",\"chat\",\"early\"]")]).await;
        raw.works().await.unwrap_or_else(|why| panic!("the session did not start: {}", why));
    });
}

// ---- capsules in pieces ---------------------------------------------------------------------

/// On a working session, a capsule no one knows and then CLOSE_WEBTRANSPORT_SESSION, each cut in
/// its frame's type, its length and its capsule's, with the page's datagrams between the pieces:
/// every datagram is heard, the unknown capsule is passed over, and the close closes the session
/// with H3_NO_ERROR (0x100). On 0.8.11 the cuts were read from their middles: the close was never
/// seen (the session stayed open) or the connection closed with an HTTP/3 error.
#[test]
fn capsules_in_pieces_with_datagrams_between_are_read_whole() {
    let s = server("capsules");
    within("capsules in pieces", async {
        let raw = Raw::connect(&s).await;
        let mut ctl = raw.conn.open_uni().await.unwrap();
        ctl.write_all(&control(&settings_payload())).await.unwrap();
        let mut live = raw.works().await.unwrap();
        let datagrams = [
            Between::Datagram("[\"m\",\"chat\",\"between 1\"]"),
            Between::Datagram("[\"m\",\"chat\",\"between 2\"]"),
            Between::Datagram("[\"m\",\"chat\",\"between 3\"]"),
        ];
        // A capsule of a reserved type (0x29 * N + 0x17, RFC 9297 §5.4): passed over.
        let unknown = capsule(0x17, "ignored ünïcode".as_bytes());
        raw.pieces(&mut live.connect, &[&unknown[..1], &unknown[1..2], &unknown[2..4], &unknown[4..]], &datagrams).await;
        let close = close_capsule(0, "done");
        raw.pieces(&mut live.connect, &[&close[..1], &close[1..2], &close[2..4], &close[4..]], &datagrams).await;
        assert_eq!(raw.closed_with().await, "0x100", "the close was not read whole");
        let h = heard(&s);
        for n in 1..=3 {
            assert!(h.contains(&format!("between {}", n)), "datagram {} not heard: {}", n, h);
        }
    });
}

// ---- whole frames -------------------------------------------------------------------------

/// What must keep working: SETTINGS, the session's request and the close each in one write, as a
/// client on a good network sends them (this one passes on 0.8.11 too).
#[test]
fn whole_frames_still_start_and_close_a_session() {
    let s = server("whole");
    within("whole frames", async {
        let raw = Raw::connect(&s).await;
        let mut ctl = raw.conn.open_uni().await.unwrap();
        ctl.write_all(&control(&settings_payload())).await.unwrap();
        let mut live = raw.works().await.unwrap();
        raw.between(Between::Datagram("[\"m\",\"chat\",\"whole\"]")).await;
        live.connect.write_all(&close_capsule(7, "")).await.unwrap();
        assert_eq!(raw.closed_with().await, "0x100");
        assert!(heard(&s).contains("whole"), "{}", heard(&s));
    });
}
