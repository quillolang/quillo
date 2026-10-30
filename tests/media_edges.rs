//! Media at its edges (src/media.rs): RTP as cameras send it, packets lost and
//! out of order, what `media.rtp` and `media.on` refuse, and a stream's end.
//! Frames are watched here by a sink of the test's own, as a page's connection
//! watches them.

mod support;

use quillo::media::{self, Frame, Out, Packet, Reorder, Sink, Unit, H264};
use quillo::Interp;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SPS: &[u8] = &[0x67, 0x42, 0xc0, 0x1e, 0xaa];
const PPS: &[u8] = &[0x68, 0xce, 0x3c, 0x80];

fn fails(src: &str) -> String {
    match quillo::eval_str(src) {
        Ok(v) => panic!("expected an error, got {}\n--- source ---\n{}", v.repr(), src),
        Err(e) => e.to_string(),
    }
}

fn ev(src: &str) -> String {
    match quillo::eval_str(src) {
        Ok(v) => v.repr(),
        Err(e) => panic!("{}\n--- source ---\n{}", e, src),
    }
}

/// A UDP port below those the system hands out, so that no other socket of
/// these tests can be given it.
fn free_udp_port() -> u16 {
    loop {
        let p = support::free_port();
        if UdpSocket::bind(("0.0.0.0", p)).is_ok() {
            return p;
        }
    }
}

fn eventually<T>(secs: u64, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < deadline, "{}", what);
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn rtp(seq: u16, ts: u32, marker: bool, payload: &[u8]) -> Vec<u8> {
    let mut b = vec![0x80, if marker { 0x80 | 96 } else { 96 }];
    b.extend_from_slice(&seq.to_be_bytes());
    b.extend_from_slice(&ts.to_be_bytes());
    b.extend_from_slice(&[0, 0, 0, 1]);
    b.extend_from_slice(payload);
    b
}

fn packet(seq: u16, ts: u32, marker: bool, payload: &[u8]) -> Packet {
    Packet { seq, ts, marker, payload: payload.to_vec() }
}

fn stap(nals: &[&[u8]]) -> Vec<u8> {
    let mut b = vec![24];
    for n in nals {
        b.extend_from_slice(&(n.len() as u16).to_be_bytes());
        b.extend_from_slice(n);
    }
    b
}

fn pushed(h: &mut H264, ps: &[Packet]) -> Vec<Unit> {
    let mut out = Vec::new();
    for p in ps {
        h.push(p, &mut out);
    }
    out
}

fn keyframe(ts: u32) -> Packet {
    packet(0, ts, true, &stap(&[SPS, PPS, &[0x65, 1]]))
}

#[test]
fn rtp_headers_are_read_and_bad_ones_refused() {
    // The smallest there is.
    let p = media::parse(&rtp(1, 2, false, b"")).unwrap();
    assert_eq!((p.seq, p.ts, p.marker, p.payload.len()), (1, 2, false, 0));
    // Too short, or not version 2.
    assert!(media::parse(&[0x80; 11]).is_none());
    assert!(media::parse(&[0x40; 12]).is_none());
    // Two contributing sources.
    let mut b = rtp(1, 2, true, &[0; 8]);
    b[0] |= 2;
    b.extend_from_slice(b"data");
    assert_eq!(media::parse(&b).unwrap().payload, b"data");
    // An extension, skipped; one cut short; one longer than the packet.
    let mut b = rtp(1, 2, true, &[0xbe, 0xde, 0, 1, 9, 9, 9, 9]);
    b[0] |= 0x10;
    b.extend_from_slice(b"data");
    assert_eq!(media::parse(&b).unwrap().payload, b"data");
    let mut b = rtp(1, 2, true, &[0xbe]);
    b[0] |= 0x10;
    assert!(media::parse(&b).is_none());
    let mut b = rtp(1, 2, true, &[0xbe, 0xde, 0, 9]);
    b[0] |= 0x10;
    assert!(media::parse(&b).is_none());
    // Padding: as the last byte says; more than there is.
    let mut b = rtp(7, 9000, true, b"x");
    b[0] |= 0x20;
    b.extend_from_slice(&[0, 0, 3]);
    assert_eq!(media::parse(&b).unwrap().payload, b"x");
    let mut b = rtp(7, 9000, true, b"x");
    b[0] |= 0x20;
    b.push(200);
    assert!(media::parse(&b).is_none());
    let mut b = rtp(7, 9000, true, b"x");
    b[0] |= 0x20;
    b.push(3);
    assert!(media::parse(&b).is_none());
}

#[test]
fn packets_are_put_back_in_order_with_gaps_given_up_on() {
    let t = Instant::now();
    let seqs = |o: &[Out]| o.iter().map(|o| match o { Out::Packet(p) => format!("{}", p.seq), Out::Lost(n) => format!("lost {}", n) }).collect::<Vec<_>>();
    let mut r = Reorder::default();
    let mut out = Vec::new();
    // Nothing yet.
    r.drain(t, &mut out);
    assert!(out.is_empty());
    for seq in [65534, 0, 65535, 1] {
        r.push(packet(seq, 0, false, &[]), t, &mut out);
    }
    assert_eq!(seqs(&out), ["65534", "65535", "0", "1"]);
    out.clear();
    // 2 never comes: 3 waits for it, then goes without.
    r.push(packet(3, 0, false, &[]), t, &mut out);
    assert!(out.is_empty());
    r.drain(t + Duration::from_millis(49), &mut out);
    assert!(out.is_empty());
    r.drain(t + Duration::from_millis(50), &mut out);
    assert_eq!(seqs(&out), ["lost 1", "3"]);
    out.clear();
    // Too late: already given up on. A copy: already passed on.
    r.push(packet(2, 0, false, &[]), t, &mut out);
    r.push(packet(3, 0, false, &[]), t, &mut out);
    assert!(out.is_empty());
    // Far ahead: the sender started again.
    r.push(packet(10_000, 0, false, &[]), t, &mut out);
    r.push(packet(10_001, 0, false, &[]), t, &mut out);
    assert_eq!(seqs(&out), ["10000", "10001"]);
    out.clear();
    // Many waiting: no longer waited for.
    let mut r = Reorder::default();
    r.push(packet(100, 0, false, &[]), t, &mut out);
    out.clear();
    for seq in 102..=357 {
        r.push(packet(seq, 0, false, &[]), t, &mut out);
    }
    assert_eq!(seqs(&out)[..2], ["lost 1".to_string(), "102".to_string()]);
    assert_eq!(out.len(), 257);
}

#[test]
fn h264_units_are_made_of_what_arrives() {
    let mut h = H264::new();
    // Nothing in the packet; a type it does not know (STAP-B); either way no frame.
    let none = pushed(&mut h, &[packet(1, 100, true, b""), packet(2, 100, true, &[25, 1, 2])]);
    assert!(none.is_empty());
    // A new time ends the frame before it, without a marker.
    let u = pushed(&mut h, &[packet(3, 200, false, &stap(&[SPS, PPS, &[0x65, 1]])), packet(4, 300, true, &[0x41, 2])]);
    assert_eq!(u.iter().map(|u| (u.ts, u.key)).collect::<Vec<_>>(), [(200, true), (300, false)]);
    // A marker on a packet of nothing: no frame.
    assert!(pushed(&mut h, &[packet(5, 400, true, &[0x7c])]).is_empty());
}

#[test]
fn a_keyframe_needs_its_parameter_sets() {
    let mut h = H264::new();
    // No SPS or PPS yet: nothing to decode it with.
    assert!(pushed(&mut h, &[packet(1, 100, true, &[0x65, 1])]).is_empty());
    assert!(pushed(&mut h, &[packet(2, 200, true, &[0x41, 2])]).is_empty(), "a delta waits for a keyframe");
    let u = pushed(&mut h, &[keyframe(300)]);
    assert_eq!(u.len(), 1);
    // A later keyframe without them gets them.
    let u = pushed(&mut h, &[packet(4, 400, true, &[0x65, 3])]);
    assert!(u[0].key);
    let mut want = vec![0, 0, 0, 1];
    want.extend_from_slice(SPS);
    want.extend_from_slice(&[0, 0, 0, 1]);
    want.extend_from_slice(PPS);
    want.extend_from_slice(&[0, 0, 0, 1, 0x65, 3]);
    assert_eq!(u[0].data, want);
}

#[test]
fn a_broken_frame_waits_for_the_next_keyframe() {
    // A STAP-A whose size runs past the packet; or a size of nothing.
    for bad in [&[24, 0, 9, 0x41][..], &[24, 0, 0, 0x41][..]] {
        let mut h = H264::new();
        assert_eq!(pushed(&mut h, &[keyframe(100)]).len(), 1);
        assert!(pushed(&mut h, &[packet(2, 200, true, bad)]).is_empty());
        assert!(pushed(&mut h, &[packet(3, 300, true, &[0x41, 1])]).is_empty(), "a delta after a broken frame");
        assert_eq!(pushed(&mut h, &[keyframe(400)]).len(), 1);
    }
    // A fragment whose start never came.
    let mut h = H264::new();
    pushed(&mut h, &[keyframe(100)]);
    assert!(pushed(&mut h, &[packet(2, 200, true, &[0x7c, 0x40 | 1, 5])]).is_empty());
    assert!(pushed(&mut h, &[packet(3, 300, true, &[0x41, 1])]).is_empty());
    // A packet lost in a frame: the rest of it is not used.
    let mut h = H264::new();
    pushed(&mut h, &[keyframe(100)]);
    pushed(&mut h, &[packet(2, 200, false, &[0x7c, 0x80 | 1, 5])]);
    h.lost();
    assert!(pushed(&mut h, &[packet(4, 200, true, &[0x41, 1])]).is_empty());
    assert_eq!(pushed(&mut h, &[keyframe(300)]).len(), 1);
}

#[test]
fn the_builtins_say_what_they_take() {
    let v = |o: &str| fails(&format!("media.rtp(\"cam\", {{ video: {} }})", o));
    assert!(fails("media.rtp(\"a b\", { video: 1 })").contains("takes a stream's name first"));
    assert!(fails("media.rtp()").contains("got nothing"));
    assert!(fails("media.rtp(\"cam\")").contains("takes the ports second"));
    assert!(fails("media.rtp(\"cam\", { port: 1 })").contains("no option `port` (it takes video, audio and host)"));
    assert!(fails("media.rtp(\"cam\", {})").contains("needs a video port, an audio port, or both"));
    assert!(fails("media.rtp(\"cam\", { host: \"127.0.0.1\" })").contains("needs a video port"));
    assert!(v("0").contains("video is a port, or { port, codec: \"h264\" }"));
    assert!(v("\"x\"").contains("video is a port"));
    assert!(v("{ port: 0 }").contains("video is a port"));
    assert!(v("{ port: 1, codec: \"vp9\" }").contains("vp9 in video is not supported (h264 is)"));
    assert!(fails("media.rtp(\"cam\", { audio: { port: 1, codec: 5 } })").contains("5 in audio is not supported (opus is)"));

    assert!(fails("media.on()").contains("takes a stream's name first (or one ending in `*`), got nothing"));
    assert!(fails("media.on(\"a b\")").contains("got \"a b\""));
    assert!(fails(&format!("media.on(\"{}\")", "a".repeat(59))).contains("takes a stream's name first"));
    assert!(fails("media.on(\"cam\", { publish: 1 })").contains("`publish` is a function of the page that wants to send"));
    assert!(fails("media.on(\"cam\", { watch: 1 })").contains("no option `watch` (it takes publish)"));
    assert!(fails("media.on(\"cam\", 5)").contains("takes options as a map, got int"));
    assert!(fails("media.on(\"cam\")\nmedia.on(\"cam\")").contains("already takes stream \"cam\" from pages"));
    assert!(fails("media.close()").contains("takes a stream's name, got nothing"));
    assert!(fails("media.close(1)").contains("got 1"));

    // Taken: a port someone else has.
    let taken = UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port();
    assert!(fails(&format!("media.rtp(\"taken\", {{ audio: {}, host: \"127.0.0.1\" }})", port)).contains(&format!("cannot listen on UDP port {}", port)));

    let mut it = Interp::new(std::env::temp_dir());
    it.load_source("media.on(\"room:*\", { publish: (p) => true })\nmedia.on(\"open\", null)\nmedia.on(\"any\", {})", "m.qwe").unwrap();
    let names: Vec<&str> = it.site.channels.iter().map(|h| h.name.as_str()).collect();
    assert_eq!(names, ["media:room:*", "media:open", "media:any"]);
    assert!(it.site.channels[0].join.is_some() && it.site.channels[1].join.is_none());
}

/// A page, as far as a stream can tell.
struct Watcher {
    id: u64,
    frames: Mutex<Vec<(u8, bool, Vec<u8>)>>,
    refuse: AtomicBool,
}

impl Sink for Watcher {
    fn take(&self, f: &Arc<Frame>) -> bool {
        if self.refuse.load(Ordering::SeqCst) {
            return false;
        }
        self.frames.lock().unwrap().push((f.kind, f.key, f.wire.clone()));
        true
    }

    fn id(&self) -> u64 {
        self.id
    }
}

fn watcher() -> Arc<Watcher> {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    Arc::new(Watcher { id: NEXT.fetch_add(1, Ordering::SeqCst), frames: Mutex::new(Vec::new()), refuse: AtomicBool::new(false) })
}

/// A frame as a page would send it.
fn wire(stream: &str, kind: u8, key: bool, data: &[u8]) -> Vec<u8> {
    let mut w = vec![stream.len() as u8];
    w.extend_from_slice(stream.as_bytes());
    w.extend_from_slice(&[kind, key as u8, 0, 0, 0, 7]);
    w.extend_from_slice(data);
    w
}

fn kinds(w: &Watcher) -> Vec<(u8, bool)> {
    w.frames.lock().unwrap().iter().map(|f| (f.0, f.1)).collect()
}

/// The figures `media.streams()` gives of a stream, as text.
fn figures(name: &str) -> String {
    ev(&format!("media.streams().filter((s) => s.name == {:?})", name))
}

#[test]
fn frames_from_a_page_reach_those_watching_and_a_stream_can_be_closed() {
    let name = "page-sent";
    assert_eq!(media::frame_stream(&wire(name, media::VIDEO, true, b"x")), Some(name));
    assert_eq!(media::frame_stream(&[]), None);
    assert_eq!(media::frame_stream(&[9, b'a']), None);
    assert_eq!(media::frame_stream(&[1, 0xff]), None);

    // A frame cut short, of no kind, or with no name in UTF-8 is not taken.
    let w = watcher();
    media::watch(name, w.clone());
    media::from_page(Vec::new());
    media::from_page(wire(name, media::VIDEO, true, b"")[..5].to_vec());
    media::from_page(wire(name, 9, true, b"x"));
    media::from_page(vec![1, 0xff, media::VIDEO, 1, 0, 0, 0, 0]);
    assert!(kinds(&w).is_empty());
    assert!(!figures(name).contains(name), "not one yet: {}", figures(name));

    // Video before any keyframe is skipped; then all of it goes.
    media::from_page(wire(name, media::VIDEO, false, b"d0"));
    media::from_page(wire(name, media::VIDEO, true, b"k1"));
    media::from_page(wire(name, media::VIDEO, false, b"d1"));
    media::from_page(wire(name, media::AUDIO, true, b"a1"));
    assert_eq!(kinds(&w), [(media::VIDEO, true), (media::VIDEO, false), (media::AUDIO, true)]);
    assert_eq!(w.frames.lock().unwrap()[0].2, wire(name, media::VIDEO, true, b"k1"));

    // A page that comes now starts from the keyframe.
    let late = watcher();
    media::watch(name, late.clone());
    assert_eq!(kinds(&late), [(media::VIDEO, true), (media::VIDEO, false)]);
    let s = figures(name);
    assert!(s.contains("viewers: 2") && s.contains("video: { frames: 3") && s.contains("audio: { frames: 1"), "{}", s);

    // One that cannot take a frame skips until the next keyframe, audio aside.
    late.refuse.store(true, Ordering::SeqCst);
    media::from_page(wire(name, media::VIDEO, false, b"d2"));
    late.refuse.store(false, Ordering::SeqCst);
    media::from_page(wire(name, media::VIDEO, false, b"d3"));
    media::from_page(wire(name, media::AUDIO, true, b"a2"));
    assert_eq!(kinds(&late)[2..], [(media::AUDIO, true)]);
    media::from_page(wire(name, media::VIDEO, true, b"k2"));
    assert_eq!(kinds(&late)[3..], [(media::VIDEO, true)]);

    // A page that stops watching gets no more; one watching twice, once.
    media::unwatch(name, late.id);
    media::unwatch("no-such-stream", late.id);
    media::from_page(wire(name, media::AUDIO, true, b"a3"));
    assert_eq!(kinds(&late).len(), 4);
    media::watch(name, w.clone());
    media::from_page(wire(name, media::VIDEO, false, b"d4"));
    let n = w.frames.lock().unwrap().iter().filter(|f| f.2.ends_with(b"d4")).count();
    assert_eq!(n, 1);
    assert!(figures(name).contains("viewers: 1"), "{}", figures(name));
    media::unwatch_all(w.id);
    assert!(figures(name).contains("viewers: 0"), "{}", figures(name));

    // Those watching at the end hear of it, and no stream is left.
    let last = watcher();
    media::watch(name, last.clone());
    let before = last.frames.lock().unwrap().len();
    assert_eq!(ev(&format!("media.close({:?})", name)), "true");
    let f = last.frames.lock().unwrap();
    assert_eq!(f[before..].iter().map(|f| f.0).collect::<Vec<_>>(), [media::END]);
    drop(f);
    assert_eq!(ev(&format!("media.close({:?})", name)), "false");
    assert!(!figures(name).contains(name));
}

#[test]
fn a_keyframe_that_never_comes_lets_go_of_what_is_kept() {
    let name = "long-gop";
    media::from_page(wire(name, media::VIDEO, true, b"k"));
    for _ in 0..600 {
        media::from_page(wire(name, media::VIDEO, false, b"d"));
    }
    // 600 kept is the most: no keyframe to start from, so a new page waits.
    let w = watcher();
    media::watch(name, w.clone());
    assert!(kinds(&w).is_empty(), "{}", kinds(&w).len());
    media::from_page(wire(name, media::VIDEO, false, b"d"));
    assert!(kinds(&w).is_empty());
    media::from_page(wire(name, media::VIDEO, true, b"k"));
    assert_eq!(kinds(&w), [(media::VIDEO, true)]);
    assert_eq!(ev(&format!("media.close({:?})", name)), "true");
}

#[test]
fn rtp_on_its_ports_makes_frames_with_their_figures() {
    let name = "cam-rtp";
    let (video, audio) = (free_udp_port(), free_udp_port());
    let src = format!("media.rtp({:?}, {{ video: {{ port: {}, codec: \"H264\" }}, audio: {{ port: {} }}, host: \"127.0.0.1\" }})", name, video, audio);
    assert_eq!(ev(&src), "null");
    let w = watcher();
    media::watch(name, w.clone());
    let s = UdpSocket::bind("127.0.0.1:0").unwrap();
    let sent = |p: Vec<u8>, port: u16| s.send_to(&p, ("127.0.0.1", port)).unwrap();

    // What is not RTP is passed over.
    sent(vec![1, 2, 3], video);
    sent(rtp(1, 100, true, &stap(&[SPS, PPS, &[0x65, 1]])), video);
    eventually(5, "the keyframe did not arrive", || (kinds(&w) == [(media::VIDEO, true)]).then_some(()));
    assert!(w.frames.lock().unwrap()[0].2.windows(SPS.len()).any(|x| x == SPS));

    // Out of order: put back. An empty audio packet is nothing.
    sent(rtp(1, 960, true, b""), audio);
    sent(rtp(3, 1920, true, b"o2"), audio);
    sent(rtp(2, 960, true, b"o1"), audio);
    eventually(5, "the audio did not arrive", || (w.frames.lock().unwrap().len() == 3).then_some(()));
    let f = w.frames.lock().unwrap();
    assert!(f[1].2.ends_with(b"o1") && f[2].2.ends_with(b"o2"), "{:?}", f[1..].to_vec());
    assert!(f[1].0 == media::AUDIO && f[1].1);
    drop(f);

    // A delta, then a packet lost: counted, and what follows waits for a keyframe.
    sent(rtp(2, 200, true, &[0x41, 2]), video);
    sent(rtp(4, 400, true, &[0x41, 4]), video);
    sent(rtp(5, 500, true, &[0x41, 5]), video);
    // Audio too, which goes on.
    sent(rtp(5, 3840, true, b"o4"), audio);
    eventually(5, "the losses were not counted", || (figures(name).matches("lost: 1").count() == 2).then_some(()));
    assert_eq!(w.frames.lock().unwrap().iter().filter(|f| f.2.ends_with(b"o4")).count(), 1);
    sent(rtp(6, 600, true, &stap(&[SPS, PPS, &[0x65, 6]])), video);
    eventually(5, "the next keyframe did not arrive", || (kinds(&w).iter().filter(|k| k.0 == media::VIDEO && k.1).count() == 2).then_some(()));
    assert_eq!(kinds(&w).iter().filter(|k| **k == (media::VIDEO, false)).count(), 1);

    // The same ports again (a reloaded program): the socket stays, for the new track.
    assert_eq!(ev(&src), "null");
    sent(rtp(7, 700, true, &[0x41, 7]), video);
    eventually(5, "the socket did not stay", || (kinds(&w).iter().filter(|k| **k == (media::VIDEO, false)).count() == 2).then_some(()));

    // Closed: the ports are free at once.
    assert_eq!(ev(&format!("media.close({:?})", name)), "true");
    assert_eq!(kinds(&w).last(), Some(&(media::END, false)));
    UdpSocket::bind(("127.0.0.1", video)).unwrap();
    UdpSocket::bind(("127.0.0.1", audio)).unwrap();
}

#[test]
fn a_rate_is_shown_while_frames_come() {
    let name = "rates";
    let start = Instant::now();
    let mut n = 0;
    while start.elapsed() < Duration::from_millis(1200) {
        media::from_page(wire(name, media::AUDIO, true, &[0; 100]));
        n += 1;
        std::thread::sleep(Duration::from_millis(20));
    }
    let s = figures(name);
    assert!(s.contains(&format!("frames: {}", n)), "{}", s);
    let audio = s.split("audio: ").nth(1).unwrap();
    assert!(!audio.contains("fps: 0"), "{}", s);
    assert!(!audio.contains("kbps: 0"), "{}", s);
    assert_eq!(ev(&format!("media.close({:?})", name)), "true");
}
