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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

type R = Result<Value, Ctrl>;

/// How long a packet waits for one sent before it that has not come.
const REORDER: Duration = Duration::from_millis(50);
/// Video frames kept for viewers who start watching (from the last keyframe).
const MAX_GOP: usize = 600;

pub const VIDEO: u8 = 1;
pub const AUDIO: u8 = 2;

pub fn install(m: &mut Map) {
    let fns: &[(&'static str, BuiltinFn)] = &[("media.rtp", rtp), ("media.streams", streams)];
    for (name, f) in fns {
        m.insert(name.trim_start_matches("media.").into(), Value::Builtin(name, *f));
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
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
    /// Video since the last keyframe.
    gop: Mutex<Vec<Arc<Frame>>>,
    video: Stats,
    audio: Stats,
    ports: Mutex<Vec<u16>>,
}

fn streams_map() -> &'static Mutex<HashMap<String, Arc<Stream>>> {
    static S: OnceLock<Mutex<HashMap<String, Arc<Stream>>>> = OnceLock::new();
    S.get_or_init(Default::default)
}

fn stream(name: &str) -> Arc<Stream> {
    lock(streams_map())
        .entry(name.to_string())
        .or_insert_with(|| {
            Arc::new(Stream {
                name: name.to_string(),
                viewers: Mutex::new(Vec::new()),
                gop: Mutex::new(Vec::new()),
                video: Stats::default(),
                audio: Stats::default(),
                ports: Mutex::new(Vec::new()),
            })
        })
        .clone()
}

impl Stream {
    fn push(&self, f: Frame) {
        let f = Arc::new(f);
        if f.kind == VIDEO {
            self.video.count(f.wire.len());
            let mut gop = lock(&self.gop);
            if f.key || gop.len() >= MAX_GOP {
                gop.clear();
            }
            if f.key || !gop.is_empty() {
                gop.push(f.clone());
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

/// A page starts watching `name`: it gets the video since the last
/// keyframe at once, then each frame.
pub fn watch(name: &str, sink: Arc<dyn Sink>) {
    let s = stream(name);
    let gop = lock(&s.gop).clone();
    let behind = !gop.iter().all(|f| sink.take(f)) || gop.is_empty();
    let mut viewers = lock(&s.viewers);
    viewers.retain(|v| v.sink.id() != sink.id());
    viewers.push(Arc::new(Viewer { sink, behind: AtomicBool::new(behind) }));
}

pub fn unwatch(name: &str, id: u64) {
    if let Some(s) = lock(streams_map()).get(name) {
        lock(&s.viewers).retain(|v| v.sink.id() != id);
    }
}

/// A page has gone: it watches nothing.
pub fn unwatch_all(id: u64) {
    for s in lock(streams_map()).values() {
        lock(&s.viewers).retain(|v| v.sink.id() != id);
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
}

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
fn rtp(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
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
    let s = stream(&name);
    for (port, codec) in tracks {
        if let Err(e) = listen(&host, port, Track { stream: s.clone(), codec }) {
            return fail(line, format!("`media.rtp`: cannot listen on UDP port {}: {}", port, e));
        }
        let mut ports = lock(&s.ports);
        if !ports.contains(&port) {
            ports.push(port);
        }
    }
    Ok(Value::Nil)
}

/// `media.streams()`: each stream, with its viewers and its tracks' rates.
fn streams(_: &mut Interp, _: Vec<Value>, _: u32) -> R {
    let mut all: Vec<Arc<Stream>> = lock(streams_map()).values().filter(|s| !lock(&s.ports).is_empty()).cloned().collect();
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
