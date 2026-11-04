//! Media: RTP from cameras and encoders, played in pages.
//!
//! ```text
//! media.rtp("cam", { video: 5004, audio: 5006 })    // H.264 and Opus
//! route GET / = <html><body><quillo-media stream="cam" controls/></body></html>
//! ```
//!
//! The server takes RTP on the ports given, puts each track's packets back
//! in order (a packet may wait up to `REORDER` for one sent before it), and
//! makes frames of them: for H.264 (RFC 6184), an access unit of NAL units,
//! from single packets, STAP-A aggregates and FU-A fragments; for Opus (RFC
//! 7587), each packet. Video frames lost or broken make the stream wait for
//! the next keyframe, as later frames cannot be decoded without them.
//!
//! Pages get the frames over their channels' connection (see `wt`): a
//! stream per track over WebTransport, binary messages over a WebSocket.
//! The frames since the last keyframe are kept, so a page that starts
//! watching starts at once; one that falls behind skips to the next
//! keyframe. In the page, `<quillo-media>` decodes them with WebCodecs.
//! No packet reaches Quillo code.

use crate::interp::{fail, Ctrl, Interp};
use crate::value::*;
use std::collections::{BTreeMap, HashMap};
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

type R = Result<Value, Ctrl>;

/// How long a packet waits for one sent before it that has not come.
const REORDER: Duration = Duration::from_millis(50);
/// Video frames kept for viewers who start watching (from the last keyframe).
const MAX_GOP: usize = 600;
/// At most this many bytes of them: 600 frames of 1 MiB from a page were 600 MiB, never freed
/// (FEEDBACK51.md, Q-643; branch feedback51-fixes).
const MAX_GOP_BYTES: usize = 32 << 20;
/// Streams pages may make in all, sending or watching (each a `media.on` let them).
const MAX_PAGE_STREAMS: usize = 1024;
/// Of those, the most one tenant's pages (or those of no tenant) may make, so that one tenant
/// cannot take them all (found in review of FEEDBACK51.md, Q-643; branch feedback51-fixes).
const MAX_TENANT_STREAMS: usize = 256;
/// Most streams one page watches at once.
const MAX_WATCHED: usize = 32;
/// Most bytes of video kept for viewers to come, in all streams: 1,024 streams of 32 MiB each
/// were 32 GiB (found in review of FEEDBACK51.md, Q-643).
const MAX_GOP_TOTAL: usize = 256 << 20;
/// What all streams keep now (see `MAX_GOP_TOTAL`).
static GOP_TOTAL: AtomicUsize = AtomicUsize::new(0);

pub const VIDEO: u8 = 1;
pub const AUDIO: u8 = 2;
/// A stream has ended (`media.close`): no data.
pub const END: u8 = 0;

pub fn install(m: &mut Map) {
    let fns: &[(&'static str, BuiltinFn)] = &[("media.rtp", rtp), ("media.streams", streams), ("media.close", close), ("media.on", on)];
    for (name, f) in fns {
        m.insert(name.trim_start_matches("media.").into(), Value::Builtin(name, *f));
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

// ---- frames and viewers ------------------------------------------------------

/// A frame as it goes to pages: `[name length][name][kind][flags][timestamp]`
/// then the frame's bytes (H.264 in Annex B form, or an Opus packet).
pub struct Frame {
    pub kind: u8,
    pub key: bool,
    pub wire: Vec<u8>,
}

impl Frame {
    fn new(stream: &str, kind: u8, key: bool, ts: u32, data: &[u8]) -> Frame {
        let mut wire = Vec::with_capacity(data.len() + stream.len() + 7);
        wire.push(stream.len() as u8);
        wire.extend_from_slice(stream.as_bytes());
        wire.push(kind);
        wire.push(key as u8);
        wire.extend_from_slice(&ts.to_be_bytes());
        wire.extend_from_slice(data);
        Frame { kind, key, wire }
    }
}

/// Where a page's frames go.
pub trait Sink: Send + Sync {
    /// False when the frame could not be taken (the page is behind or gone).
    fn take(&self, f: &Arc<Frame>) -> bool;
    fn id(&self) -> u64;
}

struct Viewer {
    sink: Arc<dyn Sink>,
    /// Behind: skips video until a keyframe.
    behind: AtomicBool,
}

#[derive(Default)]
struct Stats {
    frames: AtomicU64,
    bytes: AtomicU64,
    lost: AtomicU64,
    /// Frames and bytes in the second so far, and the last second's.
    window: Mutex<(Option<Instant>, u64, u64, f64, f64)>,
}

impl Stats {
    fn count(&self, bytes: usize) {
        self.frames.fetch_add(1, Relaxed);
        self.bytes.fetch_add(bytes as u64, Relaxed);
        let mut w = lock(&self.window);
        let now = Instant::now();
        let start = *w.0.get_or_insert(now);
        w.1 += 1;
        w.2 += bytes as u64;
        let secs = now.duration_since(start).as_secs_f64();
        if secs >= 1.0 {
            *w = (Some(now), 0, 0, w.1 as f64 / secs, w.2 as f64 * 8.0 / secs / 1000.0);
        }
    }

    fn value(&self) -> Value {
        let w = lock(&self.window);
        // Nothing for a while: no rate.
        let stale = w.0.map_or(true, |t| t.elapsed() > Duration::from_secs(3));
        let mut m = Map::new();
        m.insert("frames".into(), Value::Int(self.frames.load(Relaxed) as i64));
        m.insert("fps".into(), Value::Int(if stale { 0 } else { w.3.round() as i64 }));
        m.insert("kbps".into(), Value::Int(if stale { 0 } else { w.4.round() as i64 }));
        m.insert("lost".into(), Value::Int(self.lost.load(Relaxed) as i64));
        Value::map(m)
    }
}

pub struct Stream {
    name: String,
    viewers: Mutex<Vec<Arc<Viewer>>>,
    /// Video since the last keyframe, and its bytes.
    gop: Mutex<Vec<Arc<Frame>>>,
    gop_bytes: AtomicUsize,
    video: Stats,
    audio: Stats,
    ports: Mutex<Vec<u16>>,
    /// Pages send it (`<quillo-media publish>`), rather than RTP to ports.
    from_pages: AtomicBool,
}

fn streams_map() -> &'static Mutex<HashMap<String, Arc<Stream>>> {
    static S: OnceLock<Mutex<HashMap<String, Arc<Stream>>>> = OnceLock::new();
    S.get_or_init(Default::default)
}

/// A stream's key: its room's, `media:<name>` for the tenant (see `wt::room_key`), so each
/// tenant has its own streams of a name (FEEDBACK51.md, Q-642; branch feedback51-fixes).
pub fn key(name: &str, tenant: Option<&str>) -> String {
    crate::wt::room_key(&format!("media:{}", name), tenant)
}

fn made(name: &str) -> Arc<Stream> {
    Arc::new(Stream {
        name: name.to_string(),
        viewers: Mutex::new(Vec::new()),
        gop: Mutex::new(Vec::new()),
        gop_bytes: AtomicUsize::new(0),
        video: Stats::default(),
        audio: Stats::default(),
        ports: Mutex::new(Vec::new()),
        from_pages: AtomicBool::new(false),
    })
}

/// The stream `key`, made if need be: by the program (RTP), always.
fn stream(key: &str, name: &str) -> Arc<Stream> {
    lock(streams_map()).entry(key.to_string()).or_insert_with(|| made(name)).clone()
}

/// Whether no RTP comes to the stream `key`, and no page sends or watches it.
fn idle(key: &str, s: &Stream) -> bool {
    lock(&s.ports).is_empty() && lock(&s.viewers).is_empty() && !crate::wt::in_room(key)
}

/// The stream `key` for a page a `media.on` let send or watch: made only while pages have made
/// fewer than `MAX_PAGE_STREAMS`, and its tenant fewer than `MAX_TENANT_STREAMS`.
fn page_stream(key: &str, name: &str) -> Option<Arc<Stream>> {
    let mut all = lock(streams_map());
    if let Some(s) = all.get(key) {
        return Some(s.clone());
    }
    let tenant = crate::wt::key_tenant(key);
    let full = |all: &HashMap<String, Arc<Stream>>| all.len() >= MAX_PAGE_STREAMS || all.keys().filter(|k| crate::wt::key_tenant(k) == tenant).count() >= MAX_TENANT_STREAMS;
    if full(&all) {
        // Those no page sends or watches any more go first: names pass, and a stream stayed
        // made for each, so the 1,025th room of `rooms:*` would never have one.
        all.retain(|k, s| !idle(k, s));
        if full(&all) {
            return None;
        }
    }
    Some(all.entry(key.to_string()).or_insert_with(|| made(name)).clone())
}

/// The stream `key` goes, if no RTP comes to it and no page sends or watches it any more: a page
/// that gone leaves nothing behind (found in review of FEEDBACK51.md, Q-643).
pub fn drop_if_idle(key: &str) {
    if !crate::wt::key_channel(key).starts_with("media:") {
        return;
    }
    let mut all = lock(streams_map());
    if all.get(key).is_some_and(|s| idle(key, s)) {
        all.remove(key);
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        GOP_TOTAL.fetch_sub(self.gop_bytes.load(Relaxed), Relaxed);
    }
}

impl Stream {
    /// What it kept for viewers to come goes.
    fn forget(&self, gop: &mut Vec<Arc<Frame>>) {
        gop.clear();
        GOP_TOTAL.fetch_sub(self.gop_bytes.swap(0, Relaxed), Relaxed);
    }

    fn push(&self, f: Frame) {
        let f = Arc::new(f);
        if f.kind == VIDEO {
            self.video.count(f.wire.len());
            let mut gop = lock(&self.gop);
            let n = f.wire.len();
            if f.key || gop.len() >= MAX_GOP || self.gop_bytes.load(Relaxed) + n > MAX_GOP_BYTES {
                self.forget(&mut gop);
            }
            if f.key || !gop.is_empty() {
                // Past what all streams may keep, this one keeps none: those who start watching
                // wait for its next keyframe.
                if GOP_TOTAL.load(Relaxed) + n > MAX_GOP_TOTAL {
                    self.forget(&mut gop);
                } else {
                    self.gop_bytes.fetch_add(n, Relaxed);
                    GOP_TOTAL.fetch_add(n, Relaxed);
                    gop.push(f.clone());
                }
            }
        } else {
            self.audio.count(f.wire.len());
        }
        let viewers = lock(&self.viewers).clone();
        for v in viewers {
            if f.kind == VIDEO {
                if v.behind.load(Relaxed) && !f.key {
                    continue;
                }
                v.behind.store(!v.sink.take(&f), Relaxed);
            } else {
                v.sink.take(&f);
            }
        }
    }
}

/// A page its stream's `watch` let in starts watching `name` (the stream `key`, or with none of
/// the tenant's the program's own of that name, an RTP camera): it gets the video since the last
/// keyframe at once, then each frame.
pub fn watch(key: &str, name: &str, sink: Arc<dyn Sink>) {
    let found = {
        let all = lock(streams_map());
        // One page watches so many at most.
        if all.values().filter(|s| lock(&s.viewers).iter().any(|v| v.sink.id() == sink.id())).count() >= MAX_WATCHED {
            return;
        }
        all.get(key).or_else(|| program_stream(&all, name)).cloned()
    };
    let Some(s) = found.or_else(|| page_stream(key, name)) else { return };
    let gop = lock(&s.gop).clone();
    let behind = !gop.iter().all(|f| sink.take(f)) || gop.is_empty();
    let mut viewers = lock(&s.viewers);
    viewers.retain(|v| v.sink.id() != sink.id());
    viewers.push(Arc::new(Viewer { sink, behind: AtomicBool::new(behind) }));
}

/// The program's own stream of `name`, one RTP comes to, which a tenant's pages watch when it
/// has none of its own. Not one pages made: a page with no tenant watching a name first made a
/// stream that tenants' viewers then joined, and its own frames reached them (found in review of
/// FEEDBACK51.md, Q-642).
fn program_stream<'a>(all: &'a HashMap<String, Arc<Stream>>, name: &str) -> Option<&'a Arc<Stream>> {
    all.get(&format!("media:{}", name)).filter(|s| !lock(&s.ports).is_empty())
}

pub fn unwatch(key: &str, id: u64) {
    let name = crate::wt::key_channel(key).strip_prefix("media:").unwrap_or_default().to_string();
    {
        let all = lock(streams_map());
        // Its tenant's, and the program's it may watch for want of one.
        for s in all.get(key).into_iter().chain(program_stream(&all, &name)) {
            lock(&s.viewers).retain(|v| v.sink.id() != id);
        }
    }
    drop_if_idle(key);
}

/// A page has gone: it watches nothing.
pub fn unwatch_all(id: u64) {
    let watched: Vec<String> = lock(streams_map())
        .iter()
        .filter(|(_, s)| {
            let mut v = lock(&s.viewers);
            let before = v.len();
            v.retain(|v| v.sink.id() != id);
            v.len() != before
        })
        .map(|(k, _)| k.clone())
        .collect();
    for k in watched {
        drop_if_idle(&k);
    }
}

// ---- RTP ---------------------------------------------------------------------

pub struct Packet {
    pub seq: u16,
    pub ts: u32,
    pub marker: bool,
    pub payload: Vec<u8>,
}

/// An RTP packet's header and payload (RFC 3550), without its padding.
pub fn parse(b: &[u8]) -> Option<Packet> {
    if b.len() < 12 || b[0] >> 6 != 2 {
        return None;
    }
    let cc = (b[0] & 0x0f) as usize;
    let mut at = 12 + 4 * cc;
    if b[0] & 0x10 != 0 {
        if b.len() < at + 4 {
            return None;
        }
        let words = u16::from_be_bytes([b[at + 2], b[at + 3]]) as usize;
        at += 4 + 4 * words;
    }
    let mut end = b.len();
    if b[0] & 0x20 != 0 {
        end = end.checked_sub(*b.last()? as usize)?;
    }
    if at > end {
        return None;
    }
    Some(Packet {
        seq: u16::from_be_bytes([b[2], b[3]]),
        ts: u32::from_be_bytes([b[4], b[5], b[6], b[7]]),
        marker: b[1] & 0x80 != 0,
        payload: b[at..end].to_vec(),
    })
}

/// Puts packets back in the order they were sent: a packet is passed on
/// once those before it have been, or have been waited for long enough.
#[derive(Default)]
pub struct Reorder {
    /// The next sequence number to pass on, extended past 16 bits.
    next: Option<u64>,
    waiting: BTreeMap<u64, (Instant, Packet)>,
}

pub enum Out {
    Packet(Packet),
    /// Packets that never came.
    Lost(u64),
}

impl Reorder {
    fn extend(&self, seq: u16) -> u64 {
        let Some(next) = self.next else { return seq as u64 + (1 << 16) };
        // The extended number nearest the next expected one.
        let base = next & !0xffff;
        [base.wrapping_sub(1 << 16), base, base + (1 << 16)]
            .into_iter()
            .map(|b| b + seq as u64)
            .min_by_key(|c| c.abs_diff(next))
            .unwrap_or(base + seq as u64)
    }

    pub fn push(&mut self, p: Packet, now: Instant, out: &mut Vec<Out>) {
        let ext = self.extend(p.seq);
        match self.next {
            None => self.next = Some(ext),
            // Late (or a copy): already passed on, or given up on.
            Some(n) if ext < n => return,
            // Far ahead: the sender started again.
            Some(n) if ext - n > 3000 => {
                self.waiting.clear();
                self.next = Some(ext);
            }
            _ => {}
        }
        self.waiting.insert(ext, (now, p));
        self.drain(now, out);
    }

    /// Passes on what can be, giving up on packets waited for too long.
    pub fn drain(&mut self, now: Instant, out: &mut Vec<Out>) {
        loop {
            let Some(next) = self.next else { return };
            if let Some((_, p)) = self.waiting.remove(&next) {
                out.push(Out::Packet(p));
                self.next = Some(next + 1);
                continue;
            }
            let Some((&first, (at, _))) = self.waiting.iter().next() else { return };
            if now.duration_since(*at) < REORDER && self.waiting.len() < 256 {
                return;
            }
            out.push(Out::Lost(first - next));
            self.next = Some(first);
        }
    }
}

/// Makes H.264 access units of RTP payloads, in order.
#[derive(Default)]
pub struct H264 {
    ts: Option<u32>,
    nals: Vec<Vec<u8>>,
    /// A fragmented NAL unit being put together.
    fu: Option<Vec<u8>>,
    broken: bool,
    sps: Option<Vec<u8>>,
    pps: Option<Vec<u8>>,
    /// After a loss: only a keyframe will do.
    want_key: bool,
    /// The bytes of the frame being put together (Q-604).
    held: usize,
}

/// Larger than any frame a camera sends: a 4K keyframe is a few MB.
const MAX_FRAME: usize = 16 << 20;

/// An access unit: Annex B bytes, and whether it is a keyframe.
pub struct Unit {
    pub ts: u32,
    pub key: bool,
    pub data: Vec<u8>,
}

impl H264 {
    pub fn new() -> H264 {
        H264 { want_key: true, ..Default::default() }
    }

    pub fn lost(&mut self) {
        self.broken = true;
        self.fu = None;
    }

    pub fn push(&mut self, p: &Packet, out: &mut Vec<Unit>) {
        if self.ts.is_some_and(|t| t != p.ts) {
            self.flush(out);
        }
        self.ts = Some(p.ts);
        let b = &p.payload;
        if b.is_empty() {
            return;
        }
        // A frame that never ends (no marker, the same time) is let go past a size no frame
        // has, where it grew for as long as packets came (FEEDBACK49.md, Q-604; branch main).
        self.held += b.len();
        if self.held > MAX_FRAME {
            self.nals.clear();
            self.lost();
            self.held = b.len();
        }
        match b[0] & 0x1f {
            1..=23 => self.nals.push(b.to_vec()),
            // STAP-A: NAL units, each after its 16-bit size.
            24 => {
                let mut at = 1;
                while at + 2 <= b.len() {
                    let n = u16::from_be_bytes([b[at], b[at + 1]]) as usize;
                    at += 2;
                    if n == 0 || at + n > b.len() {
                        self.broken = true;
                        break;
                    }
                    self.nals.push(b[at..at + n].to_vec());
                    at += n;
                }
            }
            // FU-A: a NAL unit in pieces; its header made of the two bytes'.
            28 if b.len() >= 2 => {
                let (start, end) = (b[1] & 0x80 != 0, b[1] & 0x40 != 0);
                if start {
                    let mut nal = vec![(b[0] & 0xe0) | (b[1] & 0x1f)];
                    nal.extend_from_slice(&b[2..]);
                    self.fu = Some(nal);
                } else {
                    match self.fu.as_mut() {
                        Some(nal) => nal.extend_from_slice(&b[2..]),
                        // The start was lost.
                        None => self.broken = true,
                    }
                }
                if end {
                    if let Some(nal) = self.fu.take() {
                        self.nals.push(nal);
                    }
                }
            }
            _ => {}
        }
        if p.marker {
            self.flush(out);
        }
    }

    fn flush(&mut self, out: &mut Vec<Unit>) {
        self.held = 0;
        let nals = std::mem::take(&mut self.nals);
        let broken = std::mem::take(&mut self.broken) || self.fu.take().is_some();
        let Some(ts) = self.ts.take() else { return };
        if broken {
            self.want_key = true;
            return;
        }
        if nals.is_empty() {
            return;
        }
        let mut key = false;
        let (mut has_sps, mut has_pps) = (false, false);
        for n in &nals {
            match n[0] & 0x1f {
                5 => key = true,
                7 => {
                    has_sps = true;
                    self.sps = Some(n.clone());
                }
                8 => {
                    has_pps = true;
                    self.pps = Some(n.clone());
                }
                _ => {}
            }
        }
        if self.want_key && !key {
            return;
        }
        let mut data = Vec::with_capacity(nals.iter().map(|n| n.len() + 4).sum::<usize>() + 64);
        if key {
            // A keyframe carries what decoding it needs.
            let (Some(sps), Some(pps)) = (self.sps.clone(), self.pps.clone()) else {
                self.want_key = true;
                return;
            };
            for (have, nal) in [(has_sps, sps), (has_pps, pps)] {
                if !have {
                    data.extend_from_slice(&[0, 0, 0, 1]);
                    data.extend_from_slice(&nal);
                }
            }
            self.want_key = false;
        }
        for n in nals {
            data.extend_from_slice(&[0, 0, 0, 1]);
            data.extend_from_slice(&n);
        }
        out.push(Unit { ts, key, data });
    }
}

// ---- receiving -----------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Codec {
    H264,
    Opus,
}

struct Track {
    stream: Arc<Stream>,
    codec: Codec,
    /// Set by `media.close`: the socket's thread stops and lets the port go,
    /// then says so (`released`).
    closed: bool,
    released: bool,
}

fn sockets() -> &'static Mutex<HashMap<u16, Arc<Mutex<Track>>>> {
    static S: OnceLock<Mutex<HashMap<u16, Arc<Mutex<Track>>>>> = OnceLock::new();
    S.get_or_init(Default::default)
}

/// Takes RTP for `track` on `port`; the same port again (a reloaded program)
/// keeps its socket and takes the track as it is now.
fn listen(host: &str, port: u16, track: Track) -> std::io::Result<()> {
    let mut all = lock(sockets());
    if let Some(t) = all.get(&port) {
        *lock(t) = track;
        return Ok(());
    }
    let socket = UdpSocket::bind((host, port))?;
    socket.set_read_timeout(Some(REORDER / 2))?;
    let t = Arc::new(Mutex::new(track));
    all.insert(port, t.clone());
    std::thread::Builder::new().name(format!("quillo-rtp-{}", port)).spawn(move || receive(socket, t))?;
    Ok(())
}

fn receive(socket: UdpSocket, track: Arc<Mutex<Track>>) {
    let mut buf = vec![0u8; 65_536];
    let mut order = Reorder::default();
    let mut h264 = H264::new();
    let (mut packets, mut units) = (Vec::new(), Vec::new());
    loop {
        if crate::httpd::stopping() {
            return;
        }
        if lock(&track).closed {
            drop(socket);
            lock(&track).released = true;
            return;
        }
        let now = Instant::now();
        match socket.recv_from(&mut buf) {
            Ok((n, _)) => {
                if let Some(p) = parse(&buf[..n]) {
                    order.push(p, now, &mut packets);
                }
            }
            Err(_) => order.drain(now, &mut packets),
        }
        if packets.is_empty() {
            continue;
        }
        let (stream, codec) = {
            let t = lock(&track);
            (t.stream.clone(), t.codec)
        };
        for p in packets.drain(..) {
            match (p, codec) {
                (Out::Lost(n), _) => {
                    let stats = if codec == Codec::H264 { &stream.video } else { &stream.audio };
                    stats.lost.fetch_add(n, Relaxed);
                    if codec == Codec::H264 {
                        h264.lost();
                    }
                }
                (Out::Packet(p), Codec::H264) => h264.push(&p, &mut units),
                (Out::Packet(p), Codec::Opus) => {
                    if !p.payload.is_empty() {
                        stream.push(Frame::new(&stream.name, AUDIO, true, p.ts, &p.payload));
                    }
                }
            }
        }
        for u in units.drain(..) {
            stream.push(Frame::new(&stream.name, VIDEO, u.key, u.ts, &u.data));
        }
    }
}

// ---- builtins ------------------------------------------------------------------

/// A track's port and codec, from `5004` or `{ port: 5004, codec: "h264" }`.
fn track_opt(v: &Value, kind: &str, line: u32) -> Result<(u16, Codec), Ctrl> {
    let (want, name) = if kind == "video" { (Codec::H264, "h264") } else { (Codec::Opus, "opus") };
    let port = |v: &Value| match v {
        Value::Int(n) if (1..=65535).contains(n) => Some(*n as u16),
        _ => None,
    };
    let bad = || fail(line, format!("`media.rtp`: {} is a port, or {{ port, codec: \"{}\" }}", kind, name)).map(|_: Value| (0, want));
    match v {
        Value::Map(m) => {
            let Some(p) = m.get("port").and_then(port) else { return bad() };
            match m.get("codec") {
                None | Some(Value::Nil) => Ok((p, want)),
                Some(Value::Str(c)) if c.eq_ignore_ascii_case(name) => Ok((p, want)),
                Some(c) => fail(line, format!("`media.rtp`: {} in {} is not supported ({} is)", c.display(), kind, name)).map(|_: Value| (0, want)),
            }
        }
        v => match port(v) {
            Some(p) => Ok((p, want)),
            None => bad(),
        },
    }
}

/// `media.rtp(name, { video, audio, host })`, at the top of a program.
fn rtp(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let name = match args.first() {
        Some(Value::Str(s)) if crate::wt::name_ok(s) => s.to_string(),
        other => return fail(line, format!("`media.rtp` takes a stream's name first (letters, digits, `_`, `-`, `.` and `:`), got {}", other.map_or("nothing".into(), |v| v.repr()))),
    };
    let Some(Value::Map(o)) = args.get(1) else {
        return fail(line, "`media.rtp` takes the ports second: { video: 5004, audio: 5006 }".to_string());
    };
    let mut host = "0.0.0.0".to_string();
    let mut tracks = Vec::new();
    for (k, v) in o.iter() {
        match k.as_str() {
            "video" | "audio" => tracks.push(track_opt(v, k.as_str(), line)?),
            "host" => host = v.display(),
            other => return fail(line, format!("`media.rtp` has no option `{}` (it takes video, audio and host)", other)),
        }
    }
    if tracks.is_empty() {
        return fail(line, "`media.rtp` needs a video port, an audio port, or both".to_string());
    }
    let s = stream(&key(&name, it.tenant().as_deref()), &name);
    for (port, codec) in tracks {
        if let Err(e) = listen(&host, port, Track { stream: s.clone(), codec, closed: false, released: false }) {
            return fail(line, format!("`media.rtp`: cannot listen on UDP port {}: {}", port, e));
        }
        let mut ports = lock(&s.ports);
        if !ports.contains(&port) {
            ports.push(port);
        }
    }
    Ok(Value::Nil)
}

// ---- pages that send ------------------------------------------------------------

/// A frame a page sent (`<quillo-media publish="name">`), as the server sends
/// frames to pages: `[name length][name][kind][key][timestamp]`, then H.264
/// (Annex B) or Opus. The page has joined the stream's channel, `media:<name>`
/// (see `on`), which src/wt.rs checks before this. The frame goes to every
/// page watching, as one from RTP does (FEEDBACK6.md, Q-060; branch
/// feedback6-fixes).
pub fn from_page(key: &str, wire: Vec<u8>) {
    let Some(&n) = wire.first() else { return };
    let n = n as usize;
    if wire.len() < 7 + n || !matches!(wire[1 + n], VIDEO | AUDIO) {
        return;
    }
    let Ok(name) = std::str::from_utf8(&wire[1..1 + n]) else { return };
    let Some(s) = page_stream(key, name) else { return };
    s.from_pages.store(true, Relaxed);
    let (kind, key) = (wire[1 + n], wire[2 + n] == 1);
    s.push(Frame { kind, key, wire });
}

/// The stream a frame from a page is for, to check the page may send it.
pub fn frame_stream(wire: &[u8]) -> Option<&str> {
    let n = *wire.first()? as usize;
    std::str::from_utf8(wire.get(1..1 + n)?).ok()
}

/// `media.on(name, { publish, watch, tenants })`: pages may send stream `name` (a name ending
/// in `*`: every stream it starts), with `<quillo-media publish="name">`, and watch it.
/// `publish(peer)` says which may send, as a channel's `join` does: the page joins the channel
/// `media:<name>`, and only a page that did is taken frames from. `watch(peer)` says which may
/// watch. Without one, no page may: anyone could watch a private call, and send into any
/// stream, an RTP camera too (FEEDBACK51.md, Q-642 and Q-643; branch feedback51-fixes). Each
/// tenant has its own streams of a name, unless `tenants: "shared"`.
fn on(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let name = match args.first() {
        Some(Value::Str(s)) if crate::wt::name_ok(s.trim_end_matches('*')) && s.len() <= 58 => s.to_string(),
        other => return fail(line, format!("`media.on` takes a stream's name first (or one ending in `*`), got {}", other.map_or("nothing".into(), |v| v.repr()))),
    };
    let (mut publish, mut watch, mut everyone) = (None, None, false);
    match args.get(1) {
        None | Some(Value::Nil) => {}
        Some(Value::Map(o)) => {
            for (k, v) in o.iter() {
                match (k.as_str(), v) {
                    ("publish", f @ (Value::Func(_) | Value::Builtin(..))) => publish = Some(f.clone()),
                    ("watch", f @ (Value::Func(_) | Value::Builtin(..))) => watch = Some(f.clone()),
                    ("publish", _) => return fail(line, "`media.on`: `publish` is a function of the page that wants to send"),
                    ("watch", _) => return fail(line, "`media.on`: `watch` is a function of the page that wants to watch"),
                    ("tenants", v) => everyone = crate::wt::shared_opt(v, "media.on", line)?,
                    (other, _) => return fail(line, format!("`media.on` has no option `{}` (it takes publish, watch and tenants)", other)),
                }
            }
        }
        Some(v) => return fail(line, format!("`media.on` takes options as a map, got {}", v.type_name())),
    }
    fn nothing(_: &mut Interp, _: Vec<Value>, _: u32) -> R {
        Ok(Value::Nil)
    }
    fn nobody(_: &mut Interp, _: Vec<Value>, _: u32) -> R {
        Ok(Value::Bool(false))
    }
    let channel = format!("media:{}", name);
    let site = std::sync::Arc::make_mut(&mut it.site);
    if site.channels.iter().any(|h| h.name == channel) {
        return fail(line, format!("`media.on`: the program already takes stream {:?} from pages", name));
    }
    if everyone {
        crate::wt::share(&channel);
    }
    let join = Some(publish.unwrap_or(Value::Builtin("media.on.nobody", nobody)));
    site.channels.push(Arc::new(crate::wt::Handler { name: channel, f: Value::Builtin("media.on.frames", nothing), join, leave: None, watch }));
    Ok(Value::Nil)
}

/// `media.close(name)`: the stream stops taking RTP, its ports are let go,
/// and the pages watching it are told it ended; true if there was one. A
/// stream made while the program runs (one a room, a call) need not hold its
/// ports until the program stops (FEEDBACK6.md, Q-061; branch feedback6-fixes).
fn close(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let name = match args.first() {
        Some(Value::Str(s)) => s.to_string(),
        other => return fail(line, format!("`media.close` takes a stream's name, got {}", other.map_or("nothing".into(), |v| v.repr()))),
    };
    // Its tenant's; from a job, a tick or the top level, every tenant's of that name, as
    // `channel.broadcast` reaches (found in review of FEEDBACK51.md, Q-642).
    let keys: Vec<String> = match crate::wt::reach(it) {
        Some(t) => vec![key(&name, t.as_deref())],
        None => {
            let ch = format!("media:{}", name);
            lock(streams_map()).keys().filter(|k| crate::wt::key_channel(k) == ch).cloned().collect()
        }
    };
    let mut any = false;
    for k in keys {
        any |= close_key(&k);
    }
    Ok(Value::Bool(any))
}

/// Ends the stream `k`; whether there was one.
fn close_key(k: &str) -> bool {
    let k = k.to_string();
    let Some(s) = lock(streams_map()).remove(&k) else { return false };
    let ports: Vec<u16> = lock(&s.ports).drain(..).collect();
    let closing: Vec<Arc<Mutex<Track>>> = {
        let mut all = lock(sockets());
        ports.iter().filter_map(|p| all.remove(p)).collect()
    };
    for t in &closing {
        lock(t).closed = true;
    }
    // Until the ports are free again, for another stream to take at once.
    let until = Instant::now() + Duration::from_secs(1);
    while Instant::now() < until && !closing.iter().all(|t| lock(t).released) {
        std::thread::sleep(Duration::from_millis(5));
    }
    // Pages sending it stop being taken from (they join again to send).
    crate::wt::leave_all(&k);
    let end = Arc::new(Frame::new(&s.name, END, false, 0, &[]));
    for v in lock(&s.viewers).drain(..) {
        v.sink.take(&end);
    }
    s.forget(&mut lock(&s.gop));
    true
}

/// `media.streams()`: each stream, with its viewers and its tracks' rates; for a page or a
/// request, its tenant's and the program's RTP ones (FEEDBACK51.md, Q-642); for a job, all.
fn streams(it: &mut Interp, _: Vec<Value>, _: u32) -> R {
    let reach = crate::wt::reach(it);
    // Its own, and the program's RTP ones: those pages with no tenant made are no tenant's.
    let mine = |k: &str, s: &Stream| match &reach {
        Some(t) => crate::wt::key_tenant(k) == t.as_deref() || crate::wt::key_tenant(k).is_none() && !lock(&s.ports).is_empty(),
        None => true,
    };
    let mut all: Vec<Arc<Stream>> =
        lock(streams_map()).iter().filter(|(k, s)| mine(k, s) && (!lock(&s.ports).is_empty() || s.from_pages.load(Relaxed))).map(|(_, s)| s.clone()).collect();
    all.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Value::list(
        all.iter()
            .map(|s| {
                let mut m = Map::new();
                m.insert("name".into(), Value::str(&s.name));
                m.insert("viewers".into(), Value::Int(lock(&s.viewers).len() as i64));
                m.insert("video".into(), s.video.value());
                m.insert("audio".into(), s.audio.value());
                Value::map(m)
            })
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rtp_packet(seq: u16, ts: u32, marker: bool, payload: &[u8]) -> Vec<u8> {
        let mut b = vec![0x80, if marker { 0x80 | 96 } else { 96 }];
        b.extend_from_slice(&seq.to_be_bytes());
        b.extend_from_slice(&ts.to_be_bytes());
        b.extend_from_slice(&[0, 0, 0, 1]);
        b.extend_from_slice(payload);
        b
    }

    fn units(packets: &[Vec<u8>]) -> Vec<Unit> {
        let mut h = H264::new();
        let mut out = Vec::new();
        for p in packets {
            h.push(&parse(p).unwrap(), &mut out);
        }
        out
    }

    const SPS: &[u8] = &[0x67, 0x42, 0xc0, 0x1e, 0xaa];
    const PPS: &[u8] = &[0x68, 0xce, 0x3c, 0x80];

    // FEEDBACK49.md, Q-604: packets that never set the marker, at one time, grew a frame for
    // as long as they came.
    #[test]
    fn a_frame_that_never_ends_is_let_go_past_its_size() {
        let mut h = H264::new();
        let mut out = Vec::new();
        let piece = vec![0x41u8; 60_000];
        for seq in 0..400u16 {
            h.push(&parse(&rtp_packet(seq, 9, false, &piece)).unwrap(), &mut out);
            assert!(h.held <= MAX_FRAME, "{} bytes held", h.held);
            assert!(h.nals.iter().map(Vec::len).sum::<usize>() <= MAX_FRAME);
        }
        assert!(out.is_empty());
        // FU-A pieces of one NAL unit with no end, the same.
        let mut h = H264::new();
        let mut first = vec![0x7c, 0x85];
        first.extend_from_slice(&piece);
        h.push(&parse(&rtp_packet(0, 9, false, &first)).unwrap(), &mut out);
        let mut more = vec![0x7c, 0x05];
        more.extend_from_slice(&piece);
        for seq in 1..400u16 {
            h.push(&parse(&rtp_packet(seq, 9, false, &more)).unwrap(), &mut out);
            assert!(h.fu.as_ref().map_or(0, Vec::len) <= MAX_FRAME + 60_002);
        }
        // A frame after it, whole, is taken: a keyframe with its SPS and PPS.
        let key = stap(&[SPS, PPS, &[0x65, 1, 2, 3]]);
        h.push(&parse(&rtp_packet(500, 10, true, &key)).unwrap(), &mut out);
        assert_eq!(out.len(), 1, "the whole frame after it");
        assert_eq!(h.held, 0);
    }

    fn stap(nals: &[&[u8]]) -> Vec<u8> {
        let mut b = vec![24];
        for n in nals {
            b.extend_from_slice(&(n.len() as u16).to_be_bytes());
            b.extend_from_slice(n);
        }
        b
    }

    #[test]
    fn rtp_headers() {
        let mut b = rtp_packet(7, 9000, true, b"x");
        b[0] |= 0x20; // padding: the last byte says how much
        b.extend_from_slice(&[0, 0, 3]);
        let p = parse(&b).unwrap();
        assert_eq!((p.seq, p.ts, p.marker, &p.payload[..]), (7, 9000, true, &b"x"[..]));
        assert!(parse(&[0x40, 0, 0]).is_none());
    }

    #[test]
    fn access_units_from_stap_a_single_and_fu_a() {
        let idr: Vec<u8> = std::iter::once(0x65).chain((0..3000).map(|i| i as u8)).collect();
        // The IDR in three fragments.
        let fu = |start: bool, end: bool, part: &[u8]| {
            let mut b = vec![0x60 | 28, (if start { 0x80 } else { 0 }) | (if end { 0x40 } else { 0 }) | 5];
            b.extend_from_slice(part);
            b
        };
        let packets = vec![
            rtp_packet(1, 100, false, &stap(&[SPS, PPS])),
            rtp_packet(2, 100, false, &fu(true, false, &idr[1..1000])),
            rtp_packet(3, 100, false, &fu(false, false, &idr[1000..2000])),
            rtp_packet(4, 100, true, &fu(false, true, &idr[2000..])),
            rtp_packet(5, 200, true, &[0x41, 1, 2, 3]),
        ];
        let u = units(&packets);
        assert_eq!(u.len(), 2);
        assert!(u[0].key && !u[1].key);
        let mut want = vec![0, 0, 0, 1];
        want.extend_from_slice(SPS);
        want.extend_from_slice(&[0, 0, 0, 1]);
        want.extend_from_slice(PPS);
        want.extend_from_slice(&[0, 0, 0, 1]);
        want.extend_from_slice(&idr);
        assert_eq!(u[0].data, want);
        assert_eq!(u[1].data, vec![0, 0, 0, 1, 0x41, 1, 2, 3]);
    }

    #[test]
    fn a_keyframe_gets_the_parameter_sets_and_deltas_wait_for_one() {
        let packets = vec![
            // A delta before any keyframe: nothing to decode it with.
            rtp_packet(1, 50, true, &[0x41, 9]),
            rtp_packet(2, 100, true, &stap(&[SPS, PPS, &[0x65, 1]])),
            rtp_packet(3, 200, true, &[0x41, 2]),
            // A later keyframe without them: they are added.
            rtp_packet(4, 300, true, &[0x65, 3]),
        ];
        let u = units(&packets);
        assert_eq!(u.len(), 3);
        assert!(u[2].key && u[2].data.windows(SPS.len()).any(|w| w == SPS));
    }

    #[test]
    fn a_lost_fragment_drops_frames_until_the_next_keyframe() {
        let mut h = H264::new();
        let mut out = Vec::new();
        h.push(&parse(&rtp_packet(1, 100, true, &stap(&[SPS, PPS, &[0x65, 1]]))).unwrap(), &mut out);
        // The start of a fragmented frame never came.
        h.lost();
        h.push(&parse(&rtp_packet(3, 200, true, &[0x7c, 0x40 | 1, 5])).unwrap(), &mut out);
        h.push(&parse(&rtp_packet(4, 300, true, &[0x41, 2])).unwrap(), &mut out);
        h.push(&parse(&rtp_packet(5, 400, true, &[0x65, 3])).unwrap(), &mut out);
        h.push(&parse(&rtp_packet(6, 500, true, &[0x41, 4])).unwrap(), &mut out);
        let ts: Vec<u32> = out.iter().map(|u| u.ts).collect();
        assert_eq!(ts, vec![100, 400, 500]);
    }

    #[test]
    fn packets_are_put_back_in_order_and_gaps_given_up_on() {
        let t0 = Instant::now();
        let mut r = Reorder::default();
        let mut out = Vec::new();
        let p = |seq: u16| Packet { seq, ts: 0, marker: false, payload: vec![] };
        for seq in [65534, 0, 65535, 1] {
            r.push(p(seq), t0, &mut out);
        }
        let seqs: Vec<u16> = out.iter().map(|o| if let Out::Packet(p) = o { p.seq } else { 9 }).collect();
        assert_eq!(seqs, vec![65534, 65535, 0, 1]);
        out.clear();
        // 2 never comes: 3 waits, then goes once it has waited long enough.
        r.push(p(3), t0, &mut out);
        assert!(out.is_empty());
        r.drain(t0 + REORDER, &mut out);
        assert!(matches!(out[..], [Out::Lost(1), Out::Packet(Packet { seq: 3, .. })]));
        // A packet older than what was passed on is dropped.
        out.clear();
        r.push(p(2), t0, &mut out);
        assert!(out.is_empty());
    }
}
