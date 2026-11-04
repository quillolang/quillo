//! What pages send is capped (FEEDBACK51.md, Q-643; branch feedback51-fixes): a stream keeps at
//! most 32 MiB of video since its keyframe, and all of them 256 MiB; pages make at most 1,024
//! streams, 256 a tenant, and a page watches at most 32. In a process of its own, as it fills
//! the process's table of streams.

use quillo::media;
use quillo::wt::{self, Out, Peer};
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;

fn wire(stream: &str, key: bool, data: &[u8]) -> Vec<u8> {
    let mut w = vec![stream.len() as u8];
    w.extend_from_slice(stream.as_bytes());
    w.extend_from_slice(&[media::VIDEO, key as u8, 0, 0, 0, 7]);
    w.extend_from_slice(data);
    w
}

fn peer() -> (Arc<Peer>, Receiver<Out>) {
    peer_in(None)
}

/// A page on `tenant`'s host.
fn peer_in(tenant: Option<&str>) -> (Arc<Peer>, Receiver<Out>) {
    let exp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() + 100;
    let sealed = quillo::session::seal("quillo channel ticket", &serde_json::json!({ "s": "{}", "t": tenant, "e": exp }).to_string());
    wt::connect(wt::ticket_of(&format!("t={}", sealed)).unwrap(), None)
}

/// The frames that came, and whether each is a keyframe.
fn came(rx: &mut Receiver<Out>) -> Vec<bool> {
    let mut out = Vec::new();
    while let Ok(o) = rx.try_recv() {
        match o {
            Out::Media(f) => out.push(f.key),
            _ => panic!("not a frame"),
        }
    }
    out
}

/// How many streams `media.streams()` lists, of those `filter` keeps.
fn streams(filter: &str) -> String {
    quillo::eval_str(&format!("len(media.streams(){})", filter)).unwrap().repr()
}

#[test]
fn what_pages_send_is_capped() {
    // 4 MiB a frame: seven are kept for a page that starts watching.
    let (name, chunk) = ("big", vec![0u8; 4 << 20]);
    let key = media::key(name, None);
    media::from_page(&key, wire(name, true, &chunk));
    for _ in 0..6 {
        media::from_page(&key, wire(name, false, &chunk));
    }
    let (p, mut p_rx) = peer();
    wt::let_watch(&p, name);
    assert_eq!(came(&mut p_rx), [true, false, false, false, false, false, false]);
    // The eighth is past 32 MiB: what was kept goes, and the page watching gets each still.
    for _ in 0..3 {
        media::from_page(&key, wire(name, false, &chunk));
    }
    assert_eq!(came(&mut p_rx), [false; 3]);
    // One that starts now gets nothing kept, and waits for the next keyframe.
    let (q, mut q_rx) = peer();
    wt::let_watch(&q, name);
    media::from_page(&key, wire(name, false, b"d"));
    assert!(came(&mut q_rx).is_empty());
    assert_eq!(came(&mut p_rx), [false]);
    media::from_page(&key, wire(name, true, b"k"));
    assert_eq!(came(&mut q_rx), [true]);

    // A page watches 32 at most.
    let (w, mut w_rx) = peer();
    for i in 0..40 {
        wt::let_watch(&w, &format!("w{}", i));
    }
    for i in 0..40 {
        let s = format!("w{}", i);
        media::from_page(&media::key(&s, None), wire(&s, true, b"k"));
    }
    assert_eq!(came(&mut w_rx).len(), 32);
    // Frames to the eight it could not watch made streams of their own, which no page sends
    // from a room here: those go when the table is full.
    assert_eq!(streams(""), "41");
    // It goes: the streams no one else sends or watches go with it.
    wt::disconnect(&w);
    assert_eq!(streams(""), "9");

    // Pages with no tenant make 256 streams, `big` among them; each tenant 256 more; 1,024 in all.
    let fill = |tenant: Option<&str>, prefix: &str, n: usize| -> (Vec<Arc<Peer>>, usize) {
        let mut pages = Vec::new();
        let mut got = 0;
        for chunk in 0..n.div_ceil(32) {
            let (p, mut rx) = peer_in(tenant);
            for i in chunk * 32..(chunk * 32 + 32).min(n) {
                let s = format!("{}{}", prefix, i);
                wt::let_watch(&p, &s);
                media::from_page(&media::key(&s, tenant), wire(&s, true, b"k"));
            }
            got += came(&mut rx).len();
            pages.push(p);
        }
        (pages, got)
    };
    let (none, got) = fill(None, "n", 300);
    assert_eq!(got, 255);
    let mut tenants = Vec::new();
    for t in ["t1", "t2", "t3"] {
        let (pages, got) = fill(Some(t), "s", 300);
        assert_eq!(got, 256, "{}", t);
        tenants.extend(pages);
    }
    assert_eq!(streams(""), "1024");
    let (t4, got) = fill(Some("t4"), "s", 10);
    assert_eq!(got, 0, "the table is full");
    // Asking to watch one past them makes none either; one there is still watched.
    let (r, mut r_rx) = peer();
    wt::let_watch(&r, "n299");
    media::from_page(&media::key("n299", None), wire("n299", true, b"k"));
    assert!(came(&mut r_rx).is_empty());
    wt::let_watch(&r, "n5");
    assert_eq!(came(&mut r_rx), [true]);
    // Those watching go: their streams give way; `n5`, still watched, and `big` stay.
    for p in none.iter().chain(tenants.iter()).chain(t4.iter()) {
        wt::disconnect(p);
    }
    assert_eq!(streams(""), "2");
    media::from_page(&media::key("fresh", None), wire("fresh", true, b"k"));
    assert_eq!(streams(".filter((s) => s.name == \"n5\" || s.name == \"big\")"), "2");

    // 256 MiB kept in all: a stream past it keeps nothing for those who come later.
    let big_frame = vec![0u8; 30 << 20];
    for i in 0..9 {
        let s = format!("g{}", i);
        media::from_page(&media::key(&s, None), wire(&s, true, &big_frame));
    }
    // `big` keeps 4 MiB (its last keyframe), the first eight 30 MiB each: 244 MiB; the ninth
    // would be 274.
    let (g, mut g_rx) = peer();
    wt::let_watch(&g, "g7");
    assert_eq!(came(&mut g_rx), [true]);
    wt::let_watch(&g, "g8");
    assert!(came(&mut g_rx).is_empty());
    media::from_page(&media::key("g8", None), wire("g8", true, b"small"));
    assert_eq!(came(&mut g_rx), [true]);
    // A stream ended gives back what it kept: the ninth keeps its next keyframe.
    assert_eq!(quillo::eval_str("media.close(\"g0\")").unwrap().repr(), "true");
    media::from_page(&media::key("g8", None), wire("g8", true, &big_frame));
    let (h, mut h_rx) = peer();
    wt::let_watch(&h, "g8");
    assert_eq!(came(&mut h_rx), [true]);
    for p in [&g, &h] {
        wt::disconnect(p);
    }
    for p in [&p, &q, &r] {
        wt::disconnect(p);
    }
}

// Found in review of FEEDBACK51.md: each channel name a page sent made an inbox, with a thread
// once the server listens, whether the program had that channel or not. A page asks of 256
// names at most, and pages of 4,096 in all; past that a page asking is told no at once.
#[test]
fn pages_make_so_many_inboxes_at_most() {
    let told_no = |rx: &mut Receiver<Out>| {
        let mut no = Vec::new();
        while let Ok(o) = rx.try_recv() {
            if let Out::Text { json, .. } = o {
                no.push(json);
            }
        }
        no
    };
    // One page: its 257th name is refused, though there is room.
    let (p, mut rx) = peer();
    for i in 0..257 {
        wt::from_page(&p, &format!("[\"j\",\"one-{}\"]", i));
    }
    assert_eq!(told_no(&mut rx), ["[\"j\",\"one-256\",false]"]);
    // Twenty pages of 256 each: past 4,096 inboxes in all, refused.
    let mut refused = 0;
    let mut pages = vec![p];
    for n in 0..20 {
        let (q, mut q_rx) = peer();
        for i in 0..256 {
            wt::from_page(&q, &format!("[\"j\",\"junk-{}-{}\"]", n, i));
        }
        refused += told_no(&mut q_rx).len();
        pages.push(q);
    }
    assert!((20 * 256 + 256 - 4096..20 * 256 + 256 - 4000).contains(&refused), "{}", refused);
    for p in &pages {
        wt::disconnect(p);
    }
}
