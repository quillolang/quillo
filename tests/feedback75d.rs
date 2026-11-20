//! FEEDBACK75.md, the security audit of 0.8.7, slice D (branch feedback75): outbound HTTP, images,
//! regular expressions, numbers and request bodies. A PNG's colour profile inflated within a
//! budget of its own (Q-1051); `http.*` answers read into a buffer that never passes their limit,
//! with all those being read held to a budget (Q-1072); a program's `content-length` and the other
//! framing headers no longer sent (Q-1089); IPv4-translated addresses judged as the IPv4 address
//! they lead to (Q-1093); one search on a long text bounded as Q-976 bounds many (Q-1075);
//! `stats.mode` counted with a keyed hash (Q-1092); a gzip body's ratio limited (Q-1090); and a
//! body still arriving when a stop comes given a deadline (Q-1091). Every server is one this test
//! starts, on this machine.

mod support;

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn dir(name: &str) -> PathBuf {
    support::temp_dir(&format!("feedback75d-{}", name))
}

/// `src`'s last value, as the interpreter writes it, run in `d`.
fn run(d: &Path, src: &str) -> String {
    let mut it = quillo::Interp::new(d);
    it.load_source(&format!("let out = {}\n", src), "t.qwe").unwrap_or_else(|e| panic!("{}", e));
    quillo::value::lookup(&it.globals, "out").unwrap().repr()
}

/// `quillo run` of `src` as a script in `d`, with `env`, given `secs` seconds: its exit code,
/// stdout and stderr.
fn script(d: &Path, src: &str, env: &[(&str, &str)], secs: u64) -> (Option<i32>, String, String) {
    std::fs::write(d.join("s.qwe"), src).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", "s.qwe"])
        .current_dir(d)
        .envs(env.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    support::stop_with_tests(&child);
    let (mut out, mut err) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
    let o = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = out.read_to_string(&mut s);
        s
    });
    let e = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = err.read_to_string(&mut s);
        s
    });
    let deadline = Instant::now() + Duration::from_secs(secs);
    let code = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status.code();
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    (code, o.join().unwrap(), e.join().unwrap())
}

/// Held by the tests that read hundreds of megabytes, one at a time: beside each other, in a debug
/// build, one's server was starved past its client's 60 s (found running them there).
static HEAVY: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn heavy() -> std::sync::MutexGuard<'static, ()> {
    HEAVY.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The test thread's own CPU time: what a call on this thread computed, which a loaded machine
/// does not stretch as it does a wall clock (CI's runner is many times slower than a local build,
/// and shared). A slow call computes, so it still counts here.
fn thread_cpu() -> Duration {
    let mut t = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: `t` lives through the call, which only writes it.
    unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut t) };
    Duration::new(t.tv_sec as u64, t.tv_nsec as u32)
}

/// The mimalloc caps the audit's memory repros ran under: the process may take 256 MiB in all, and
/// an allocation past that is refused (which aborts it).
const CAPPED: &[(&str, &str)] = &[("MIMALLOC_RESERVE_OS_MEMORY", "256MiB"), ("MIMALLOC_ARENA_RESERVE", "256MiB"), ("MIMALLOC_DISALLOW_OS_ALLOC", "1")];

// ---- Q-1051: a PNG's colour profile ------------------------------------------------------------

/// A PNG chunk: its length, kind, data and CRC.
fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut crc = flate2::Crc::new();
    crc.update(kind);
    crc.update(data);
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    out.extend_from_slice(&crc.sum().to_be_bytes());
    out
}

/// zlib's format of `bytes`.
fn zlib(bytes: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    e.write_all(bytes).unwrap();
    e.finish().unwrap()
}

/// zlib's format of `mib` MiB of zeros, made fast: 1 MiB of them deflated once, with a full flush
/// (byte-aligned, nothing looked back to), written `mib` times, then a last empty block and the
/// Adler-32 of that many zeros (1 low, the count high).
fn zeros_zlib(mib: usize) -> Vec<u8> {
    let mut c = flate2::Compress::new(flate2::Compression::best(), false);
    let input = vec![0u8; 1 << 20];
    let mut seg = Vec::with_capacity(8 << 10);
    while (c.total_in() as usize) < input.len() {
        seg.reserve(8 << 10);
        c.compress_vec(&input[c.total_in() as usize..], &mut seg, flate2::FlushCompress::None).unwrap();
    }
    loop {
        seg.reserve(8 << 10);
        let before = seg.len();
        c.compress_vec(&[], &mut seg, flate2::FlushCompress::Full).unwrap();
        if seg.len() == before || seg.ends_with(&[0, 0, 0xff, 0xff]) {
            break;
        }
    }
    let mut out = vec![0x78, 0xda];
    for _ in 0..mib {
        out.extend_from_slice(&seg);
    }
    out.extend_from_slice(&[0x03, 0x00]);
    let n = (mib as u64) << 20;
    out.extend_from_slice(&((((n % 65521) as u32) << 16) | 1).to_be_bytes());
    out
}

/// A 1×1 RGB PNG with an `iCCP` chunk of `name` and `profile` (zlib's format, as written), before
/// its pixels.
fn png_with_profile(name: &[u8], method: u8, profile: &[u8]) -> Vec<u8> {
    let mut ihdr = 1u32.to_be_bytes().to_vec();
    ihdr.extend_from_slice(&1u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut iccp = name.to_vec();
    iccp.push(0);
    iccp.push(method);
    iccp.extend_from_slice(profile);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend(chunk(b"IHDR", &ihdr));
    out.extend(chunk(b"iCCP", &iccp));
    out.extend(chunk(b"IDAT", &zlib(&[0, 255, 0, 0])));
    out.extend(chunk(b"IEND", &[]));
    out
}

const MOST_PROFILE: usize = 4 << 20;

/// The repro: a 1×1 PNG of 0.6 MB whose profile inflates to 600 MB made `image.info` take 512 MiB
/// and more (the process aborted under the audit's 256 MiB cap); now it is refused, as an error
/// value, under that cap, at once, and so is `image.convert` of it, which writes nothing
/// (FEEDBACK75.md, Q-1051; branch feedback75).
#[test]
fn a_png_profile_bomb_is_refused_in_little_memory() {
    let d = dir("iccp-bomb");
    let png = png_with_profile(b"x", 0, &zeros_zlib(600));
    assert!(png.len() < 700_000, "{} bytes", png.len());
    std::fs::write(d.join("bomb.png"), &png).unwrap();
    let t = Instant::now();
    let (code, out, err) = script(&d, "print(image.info(\"bomb.png\"))\nprint(image.convert(\"bomb.png\", \"out.jpg\", { width: 1 }))\n", CAPPED, 120);
    assert_eq!(code, Some(0), "stdout: {}\nstderr: {}", out, err);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 2, "{}", out);
    for l in &lines {
        assert!(l.starts_with("err(") && l.contains("colour profile (iCCP) is larger than the 4 MiB read"), "{}", out);
    }
    assert!(!d.join("out.jpg").exists());
    assert!(t.elapsed() < Duration::from_secs(60), "{:?}", t.elapsed());
    let _ = std::fs::remove_dir_all(&d);
}

/// A profile of exactly 4 MiB inflated is read, one byte more is refused; an empty one, one of a
/// few bytes, one named in Latin-1 (`é`, one byte) and a PNG with none are read as before
/// (FEEDBACK75.md, Q-1051; branch feedback75).
#[test]
fn a_png_profile_is_read_up_to_its_limit() {
    let d = dir("iccp-limit");
    let info = r#"{ width: 1, height: 1, format: "png", alpha: false }"#;
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("at.png", png_with_profile(b"ICC Profile", 0, &zlib(&vec![0u8; MOST_PROFILE]))),
        ("over.png", png_with_profile(b"ICC Profile", 0, &zlib(&vec![0u8; MOST_PROFILE + 1]))),
        ("under.png", png_with_profile(b"ICC Profile", 0, &zlib(&vec![7u8; MOST_PROFILE - 1]))),
        ("empty.png", png_with_profile(b"e", 0, &zlib(&[]))),
        ("one.png", png_with_profile(b"one", 0, &zlib(b"a profile"))),
        ("latin1.png", png_with_profile(b"Caf\xe9", 0, &zlib(&vec![1u8; 2 << 20]))),
    ];
    for (name, bytes) in &cases {
        std::fs::write(d.join(name), bytes).unwrap();
    }
    let img = image::RgbImage::from_fn(3, 2, |x, y| image::Rgb([x as u8, y as u8, 9]));
    img.save(d.join("plain.png")).unwrap();
    assert_eq!(run(&d, r#"image.info("at.png")"#), info);
    let over = run(&d, r#"image.info("over.png")"#);
    assert!(over.starts_with("err(") && over.contains("larger than the 4 MiB read"), "{}", over);
    assert_eq!(run(&d, r#"image.info("under.png")"#), info);
    assert_eq!(run(&d, r#"image.info("empty.png")"#), info);
    assert_eq!(run(&d, r#"image.info("one.png")"#), info);
    assert_eq!(run(&d, r#"image.info("latin1.png")"#), info);
    assert_eq!(run(&d, r#"image.info("plain.png")"#), r#"{ width: 3, height: 2, format: "png", alpha: false }"#);
    // Converted as before, a profile at the limit and none.
    assert_eq!(run(&d, r#"image.convert("at.png", "at.webp", {})"#), "{ width: 1, height: 1 }");
    assert_eq!(run(&d, r#"image.convert("plain.png", "p.jpg", { width: 2 })"#), "{ width: 2, height: 1 }");
    assert!(run(&d, r#"image.convert("over.png", "o.png", {})"#).contains("larger than the 4 MiB read"));
    let _ = std::fs::remove_dir_all(&d);
}

/// A profile the decoder passes over without inflating it (a name of 81 bytes, no name, a method
/// that is not deflate, a header that is not zlib's) is passed over here too, however large, and
/// so is a small damaged one and one wrong only in its checksum; a large damaged one, which might
/// inflate past the limit for all that can be told, is refused (FEEDBACK75.md, Q-1051; branch
/// feedback75).
#[test]
fn a_png_profile_the_decoder_skips_is_skipped() {
    let d = dir("iccp-skip");
    let info = r#"{ width: 1, height: 1, format: "png", alpha: false }"#;
    let big = zeros_zlib(64);
    // A block of the kind kept for later (3), then what is never read: too long to be passed over
    // by its size.
    let mut damaged = vec![0x78, 0xda, 0x07];
    damaged.extend(vec![0u8; 8000]);
    // 1 MiB that does not compress, inflated here for its size, its Adler-32 wrong.
    let mut x = 1u32;
    let noise: Vec<u8> = (0..1 << 20)
        .map(|_| {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            (x >> 16) as u8
        })
        .collect();
    let mut checksum = zlib(&noise);
    let n = checksum.len();
    checksum[n - 1] ^= 0x55;
    let mut not_zlib = big.clone();
    not_zlib[0] = 0x79;
    let cases: Vec<(&str, Vec<u8>, bool)> = vec![
        ("long-name.png", png_with_profile(&[b'n'; 81], 0, &big), true),
        ("no-name.png", png_with_profile(b"", 0, &big), true),
        ("method.png", png_with_profile(b"m", 1, &big), true),
        ("not-zlib.png", png_with_profile(b"z", 0, &not_zlib), true),
        ("small-damaged.png", png_with_profile(b"s", 0, &[0x78, 0xda, 0xff, 0xff, 0xff]), true),
        ("checksum.png", png_with_profile(b"c", 0, &checksum), true),
        ("damaged.png", png_with_profile(b"d", 0, &damaged), false),
    ];
    for (name, bytes, read) in &cases {
        std::fs::write(d.join(name), bytes).unwrap();
        let got = run(&d, &format!("image.info({:?})", name));
        if *read {
            assert_eq!(got, info, "{}", name);
        } else {
            assert!(got.starts_with("err(") && got.contains("colour profile (iCCP) cannot be read"), "{}: {}", name, got);
        }
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// Many `image.info` calls at once, more than are read at once, are each answered: the header
/// reads take turns, they do not lock up (FEEDBACK75.md, Q-1051; branch feedback75).
#[test]
fn many_image_headers_at_once_are_each_read() {
    let d = dir("iccp-many");
    std::fs::write(d.join("p.png"), png_with_profile(b"p", 0, &zlib(&vec![0u8; 3 << 20]))).unwrap();
    let threads: Vec<_> = (0..24)
        .map(|_| {
            let d = d.clone();
            std::thread::spawn(move || run(&d, r#"image.info("p.png")"#))
        })
        .collect();
    for t in threads {
        assert_eq!(t.join().unwrap(), r#"{ width: 1, height: 1, format: "png", alpha: false }"#);
    }
    let _ = std::fs::remove_dir_all(&d);
}

// ---- Q-1072: answers read whole ----------------------------------------------------------------

/// gzip's format of `mib` MiB of zeros, made as `zeros_zlib` makes them, split after `first` MiB:
/// the two parts, with the zeros' CRC-32 and size at the end.
fn zeros_gzip(mib: usize, first: usize) -> (Vec<u8>, Vec<u8>) {
    let z = zeros_zlib(mib);
    let seg = (z.len() - 2 - 2 - 4) / mib;
    let mut a = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0xff];
    a.extend_from_slice(&z[2..2 + seg * first]);
    let mut b = z[2 + seg * first..z.len() - 4].to_vec();
    let mut crc = flate2::Crc::new();
    let zeros = vec![0u8; 1 << 20];
    for _ in 0..mib {
        crc.update(&zeros);
    }
    b.extend_from_slice(&crc.sum().to_le_bytes());
    b.extend_from_slice(&(((mib as u64) << 20) as u32).to_le_bytes());
    (a, b)
}

/// A server on a port of its own that answers each of `n` connections with `head`, then `a`, then
/// waits until `go` says, then `b`, and closes: its port, and how many have had `a`.
fn held_upstream(n: usize, head: &'static str, a: Vec<u8>, b: Vec<u8>, go: std::sync::Arc<std::sync::atomic::AtomicBool>) -> (u16, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use std::sync::atomic::Ordering::SeqCst;
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let had = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let had2 = had.clone();
    let (a, b) = (std::sync::Arc::new(a), std::sync::Arc::new(b));
    std::thread::spawn(move || {
        for _ in 0..n {
            let Ok((mut s, _)) = l.accept() else { return };
            let (a, b, go, had) = (a.clone(), b.clone(), go.clone(), had2.clone());
            std::thread::spawn(move || {
                let mut req = [0u8; 4096];
                let _ = s.read(&mut req);
                let _ = s.write_all(head.as_bytes());
                let _ = s.write_all(&a);
                had.fetch_add(1, SeqCst);
                let until = Instant::now() + Duration::from_secs(120);
                while !go.load(SeqCst) && Instant::now() < until {
                    std::thread::sleep(Duration::from_millis(10));
                }
                let _ = s.write_all(&b);
            });
        }
    });
    (port, had)
}

const GZIP_HEAD: &str = "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Encoding: gzip\r\nConnection: close\r\n\r\n";

/// The most memory a process has taken since it started, in KB, read while it runs: Linux's
/// `VmHWM`, macOS's lifetime peak footprint.
fn peak_kb(pid: u32) -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let s = std::fs::read_to_string(format!("/proc/{}/status", pid)).ok()?;
        s.lines().find_map(|l| l.strip_prefix("VmHWM:")).and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
    }
    #[cfg(target_os = "macos")]
    {
        // SAFETY: `ri` lives through the call, which only writes it, as large as the flavour says.
        let mut ri: libc::rusage_info_v4 = unsafe { std::mem::zeroed() };
        let r = unsafe { libc::proc_pid_rusage(pid as i32, libc::RUSAGE_INFO_V4, &mut ri as *mut _ as *mut libc::rusage_info_t) };
        (r == 0).then_some(ri.ri_lifetime_max_phys_footprint / 1024)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = pid;
        None
    }
}

/// What a process takes now, in KB, as `peak_kb` counts its height: Linux's VmRSS, macOS's
/// physical footprint.
fn now_kb(pid: u32) -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let s = std::fs::read_to_string(format!("/proc/{}/status", pid)).ok()?;
        s.lines().find_map(|l| l.strip_prefix("VmRSS:")).and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
    }
    #[cfg(target_os = "macos")]
    {
        // SAFETY: as in `peak_kb`.
        let mut ri: libc::rusage_info_v4 = unsafe { std::mem::zeroed() };
        let r = unsafe { libc::proc_pid_rusage(pid as i32, libc::RUSAGE_INFO_V4, &mut ri as *mut _ as *mut libc::rusage_info_t) };
        (r == 0).then_some(ri.ri_phys_footprint / 1024)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = pid;
        None
    }
}

/// What a script waits with, after reading what it needs and saying so: until the test, having
/// taken the process's own size, writes the file "go" (`Measured`).
const READY: &str = "print(\"ready\")\nwhile is_err(read_file(\"go\")) {\n    sleep(10)\n}\n";

/// `quillo run s.qwe` (with `args`) in `d`, whose output is read as it comes, so that what its work
/// adds to what the process takes can be measured: the size once it says "ready" (`READY`) is its
/// own, from the binary, the interpreter and what it read, which an instrumented Linux build has
/// much more of than a local one (CI's 201 MB at the height of a test this file once held to an
/// absolute 192 MiB). mimalloc gives back what is freed at once (`MIMALLOC_PURGE_DELAY=0`), so the
/// height is what the work held, not what the allocator had yet to give back (the backtracker's
/// stack, as it grows, frees each buffer it outgrew: 370 MiB at the height with mimalloc's 10 ms,
/// 225 MiB without). Killed when dropped.
struct Measured {
    child: std::process::Child,
    rx: std::sync::mpsc::Receiver<String>,
    out: String,
    base: Option<u64>,
}

impl Measured {
    fn start(d: &Path, args: &[&str]) -> Measured {
        let _ = std::fs::remove_file(d.join("go"));
        let mut child = Command::new(env!("CARGO_BIN_EXE_quillo")).args(["run", "s.qwe"]).args(args).current_dir(d).env("MIMALLOC_PURGE_DELAY", "0").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        support::stop_with_tests(&child);
        let mut stdout = child.stdout.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = stdout.read(&mut buf) {
                if n == 0 || tx.send(String::from_utf8_lossy(&buf[..n]).into_owned()).is_err() {
                    break;
                }
            }
        });
        Measured { child, rx, out: String::new(), base: None }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    /// The output once it holds `marker`, or what there was after `secs`.
    fn wait_for(&mut self, marker: &str, secs: u64) -> &str {
        let until = Instant::now() + Duration::from_secs(secs);
        while !self.out.contains(marker) && Instant::now() < until {
            if let Ok(more) = self.rx.recv_timeout(Duration::from_millis(50)) {
                self.out.push_str(&more);
            }
        }
        &self.out
    }

    /// Once it is ready: its own size taken, then told to go.
    fn go(&mut self, d: &Path) {
        let out = self.wait_for("ready\n", 600).to_string();
        assert!(out.contains("ready\n"), "{}", out);
        // Twice, a moment apart, the larger kept: what it read may still be settling.
        let first = now_kb(self.pid());
        std::thread::sleep(Duration::from_millis(100));
        self.base = first.max(now_kb(self.pid()));
        std::fs::write(d.join("go"), "").unwrap();
    }

    /// Its exit code (None past `secs`, when it is killed), stdout and stderr.
    fn finish(mut self, secs: u64) -> (Option<i32>, String, String) {
        let until = Instant::now() + Duration::from_secs(secs);
        let code = loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                break status.code();
            }
            if Instant::now() > until {
                let _ = self.child.kill();
                let _ = self.child.wait();
                break None;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        while let Ok(more) = self.rx.recv_timeout(Duration::from_millis(200)) {
            self.out.push_str(&more);
        }
        let mut err = String::new();
        if let Some(mut e) = self.child.stderr.take() {
            let _ = e.read_to_string(&mut err);
        }
        (code, std::mem::take(&mut self.out), err)
    }

    /// What it takes now over its own size, in KB (all of it where its own size could not be read).
    fn grown_now_kb(&self) -> u64 {
        now_kb(self.pid()).unwrap_or(0).saturating_sub(self.base.unwrap_or(0))
    }

    /// What its height came to over its own size, in KB, where both can be read.
    fn grew_kb(&self) -> Option<u64> {
        Some(peak_kb(self.pid())?.saturating_sub(self.base?))
    }
}

impl Drop for Measured {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The repro: two calls at once to an answer of 200 KB of gzip that inflates to 200 MiB, each
/// held after 33 MiB, took about 470 MB between them (each buffer doubled to 128 MiB and the old one
/// was copied in), 230 MB for one; now each holds no more than its 64 MiB, and the error is said as
/// before, the growth over the process's own size measured (changed for CI's instrumented build,
/// whose own size passed the absolute bound; FEEDBACK75.md, Q-1072; branch feedback75).
#[test]
fn a_gzip_answer_bomb_takes_no_more_than_its_limit() {
    let _heavy = heavy();
    use std::sync::atomic::Ordering::SeqCst;
    let d = dir("gzip-bomb");
    let (a, b) = zeros_gzip(200, 33);
    assert!(a.len() + b.len() < 300_000, "{} bytes", a.len() + b.len());
    let go = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (port, had) = held_upstream(2, GZIP_HEAD, a, b, go.clone());
    let src = format!("{}let u = \"http://127.0.0.1:{}/bomb\"\nfor r in parallel(() => http.get(u), () => http.get(u)) {{ print(r) }}\nprint(\"done\")\nsleep(5000)\n", READY, port);
    std::fs::write(d.join("s.qwe"), &src).unwrap();
    let mut run = Measured::start(&d, &[]);
    run.go(&d);
    let pid = run.pid();
    // Released once both hold 33 MiB each, or after a while.
    let until = Instant::now() + Duration::from_secs(if cfg!(debug_assertions) { 120 } else { 30 });
    let held = |pid| now_kb(pid).map_or(true, |kb| kb.saturating_sub(run.base.unwrap_or(0)) >= 2 * 33 * 1024);
    while (had.load(SeqCst) < 2 || !held(pid)) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
    go.store(true, SeqCst);
    let out = run.wait_for("done\n", if cfg!(debug_assertions) { 300 } else { 60 }).to_string();
    assert_eq!(out.matches("the answer is longer than 64 MiB, the most that is read whole").count(), 2, "{}", out);
    // Two of 64 MiB over the process's own size, and 48 MiB for the allocator (the pieces' slack)
    // and the code first run after it was measured; each was 192 MiB at its height, 470 MB in all.
    if let Some(kb) = run.grew_kb() {
        assert!(kb < 2 * 64 * 1024 + 48 * 1024, "{} KB over its own {:?} KB", kb, run.base);
    }
    drop(run);
    let _ = std::fs::remove_dir_all(&d);
}

/// A server on a port of its own that answers one connection with 4 MiB of text once `ready` says.
fn ready_upstream(ready: std::sync::Arc<std::sync::atomic::AtomicBool>) -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let Ok((mut s, _)) = l.accept() else { return };
        let mut req = [0u8; 4096];
        let _ = s.read(&mut req);
        let until = Instant::now() + Duration::from_secs(120);
        while !ready.load(std::sync::atomic::Ordering::SeqCst) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = s.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", 4 << 20).as_bytes());
        let _ = s.write_all(&vec![b's'; 4 << 20]);
    });
    port
}

/// Answers that stall hold only what has arrived: five held after 32 MiB each (31 MiB of each
/// counted, the first MiB not) leave room, and a sixth call reads its 4 MiB meanwhile, where room
/// asked for ahead (64 MiB each) refused it, and refused the fifth held one; then all five end
/// whole (FEEDBACK75.md, Q-1072, found in review; branch feedback75).
#[test]
fn stalled_answers_hold_only_what_has_arrived() {
    let _heavy = heavy();
    use std::sync::atomic::Ordering::SeqCst;
    let d = dir("gzip-stalled");
    let (a, b) = zeros_gzip(32, 32);
    let go = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let ready = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (port, had) = held_upstream(5, GZIP_HEAD, a, b, go.clone());
    let small = ready_upstream(ready.clone());
    let src = format!(
        "{}let f = (u) => {{ let r = http.get(u)\n return is_err(r) ? str(r) : `${{r.status}} ${{len(r.body)}}` }}\nlet big = () => f(\"http://127.0.0.1:{}/big\")\nfor r in parallel(big, big, big, big, big, () => f(\"http://127.0.0.1:{}/small\")) {{ print(r) }}\n",
        READY, port, small
    );
    std::fs::write(d.join("s.qwe"), src).unwrap();
    let mut run = Measured::start(&d, &[]);
    run.go(&d);
    // Held once the five hold what they were sent, over the process's own size (an absolute size
    // is reached sooner by a build larger of its own).
    let until = Instant::now() + Duration::from_secs(if cfg!(debug_assertions) { 120 } else { 30 });
    while (had.load(SeqCst) < 5 || run.grown_now_kb() < 5 * 31 * 1024) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
    ready.store(true, SeqCst);
    std::thread::sleep(Duration::from_millis(if cfg!(debug_assertions) { 5000 } else { 1000 }));
    go.store(true, SeqCst);
    let (code, out, err) = run.finish(if cfg!(debug_assertions) { 300 } else { 60 });
    assert_eq!(code, Some(0), "stdout: {}\nstderr: {}", out, err);
    let lines: Vec<&str> = out.lines().filter(|l| *l != "ready").collect();
    let mut want = vec![format!("200 {}", 32 << 20); 5];
    want.push(format!("200 {}", 4 << 20));
    assert_eq!(lines, want, "{}", out);
    let _ = std::fs::remove_dir_all(&d);
}

/// What all calls read at once is still held to 256 MiB: nine held after 32 MiB each would hold
/// 279 MiB, so at least one is the error value that says so, and each of the others ends whole
/// (FEEDBACK75.md, Q-1072; branch feedback75).
#[test]
fn answers_read_at_once_are_held_to_a_budget() {
    let _heavy = heavy();
    use std::sync::atomic::Ordering::SeqCst;
    let d = dir("gzip-budget");
    // Plain, not gzip: a debug build inflated nine of 32 MiB slower than the client's 60 s.
    let (a, b) = (vec![b'z'; (32 << 20) - 1], vec![b'z'; 1]);
    let go = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (port, had) = held_upstream(9, "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 33554432\r\nConnection: close\r\n\r\n", a, b, go.clone());
    let src = format!("{}let u = \"http://127.0.0.1:{}/big\"\nlet f = () => {{ let r = http.get(u)\n return is_err(r) ? str(r) : `${{r.status}} ${{len(r.body)}}` }}\nfor r in parallel(f, f, f, f, f, f, f, f, f) {{ print(r) }}\n", READY, port);
    std::fs::write(d.join("s.qwe"), src).unwrap();
    let mut run = Measured::start(&d, &[]);
    run.go(&d);
    // Released once each call holds all it was sent (or was refused): the memory has stopped
    // growing, past what six such answers take (one or two are refused, as they cross the budget
    // at once), over the process's own size (an absolute one is reached sooner by a build larger
    // of its own). A debug build reads slower (found running it there). Released before the
    // client's own 60 s at the latest.
    let until = Instant::now() + Duration::from_secs(40);
    let mut last = (0u64, Instant::now());
    while Instant::now() < until {
        let now = run.grown_now_kb();
        if now > last.0 + 1024 {
            last = (now, Instant::now());
        } else if had.load(SeqCst) == 9 && now >= 6 * 31 * 1024 && last.1.elapsed() > Duration::from_secs(if cfg!(debug_assertions) { 5 } else { 1 }) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    go.store(true, SeqCst);
    let (code, out, err) = run.finish(if cfg!(debug_assertions) { 300 } else { 60 });
    assert_eq!(code, Some(0), "stdout: {}\nstderr: {}", out, err);
    let lines: Vec<&str> = out.lines().filter(|l| *l != "ready").collect();
    assert_eq!(lines.len(), 9, "{}", out);
    let whole = format!("200 {}", 32 << 20);
    let busy = lines.iter().filter(|l| l.contains("the answers being read at once would take more than the 256 MiB read at once")).count();
    assert!(busy >= 1, "{}", out);
    assert_eq!(lines.iter().filter(|l| **l == whole).count(), 9 - busy, "{}", out);
    let _ = std::fs::remove_dir_all(&d);
}

/// A plain answer, gzip and not, empty, of one byte, and of text in other scripts, is read whole
/// as before; one exactly at `--max-memory` is read, one byte past it is the request's stop
/// (FEEDBACK75.md, Q-1072; branch feedback75).
#[test]
fn ordinary_answers_are_read_as_before() {
    let d = dir("answers");
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { return };
            std::thread::spawn(move || {
                let mut req = [0u8; 4096];
                let n = s.read(&mut req).unwrap_or(0);
                let path = String::from_utf8_lossy(&req[..n]).split(' ').nth(1).unwrap_or("").to_string();
                let (coding, body): (&str, Vec<u8>) = match path.as_str() {
                    "/empty" => ("", Vec::new()),
                    "/one" => ("", b"a".to_vec()),
                    "/unicode" => ("", "héllo wörld, 世界 🌍".repeat(1000).into_bytes()),
                    "/gzip" => ("Content-Encoding: gzip\r\n", {
                        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
                        e.write_all(&"line\n".repeat(50_000).into_bytes()).unwrap();
                        e.finish().unwrap()
                    }),
                    "/at" => ("", vec![b'x'; 2_000_000]),
                    "/past" => ("", vec![b'x'; 2_000_001]),
                    _ => ("", b"?".to_vec()),
                };
                let head = format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n{}Content-Length: {}\r\nConnection: close\r\n\r\n", coding, body.len());
                let _ = s.write_all(head.as_bytes());
                let _ = s.write_all(&body);
            });
        }
    });
    let src = format!(
        "let u = \"http://127.0.0.1:{}\"\nfor p in [\"/empty\", \"/one\", \"/unicode\", \"/gzip\", \"/at\"] {{ let r = http.get(u + p, {{}}, {{ public_only: false }})\n print(`${{p}} ${{r.status}} ${{len(r.body)}} ${{slice(r.body, 0, 4)}}`) }}\nprint(len(http.get(u + \"/past\", {{}}, {{ public_only: false }}).body))\n",
        port
    );
    let (code, out, err) = script(&d, &src, &[], 60);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 6, "{:?} stdout: {}\nstderr: {}", code, out, err);
    assert_eq!(lines[0], "/empty 200 0 ");
    assert_eq!(lines[1], "/one 200 1 a");
    assert_eq!(lines[2], format!("/unicode 200 {} héll", "héllo wörld, 世界 🌍".chars().count() * 1000));
    assert_eq!(lines[3], "/gzip 200 250000 line");
    assert_eq!(lines[4], "/at 200 2000000 xxxx");
    assert_eq!(lines[5], "2000001");
    // Under --max-memory 2000000: one byte past it is the request's stop.
    let (code, out, err) = {
        std::fs::write(d.join("s.qwe"), &src).unwrap();
        let o = Command::new(env!("CARGO_BIN_EXE_quillo")).args(["run", "s.qwe", "--max-memory", "2000000"]).current_dir(&d).stdin(Stdio::null()).output().unwrap();
        (o.status.code(), String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned())
    };
    assert!(out.contains("/at 200 2000000 xxxx"), "stdout: {}\nstderr: {}", out, err);
    assert!(err.contains("this request's value would be bigger than --max-memory allows (2000000 bytes)"), "{:?} stdout: {}\nstderr: {}", code, out, err);
    let _ = std::fs::remove_dir_all(&d);
}

// ---- Q-1089: framing headers -------------------------------------------------------------------

/// A keep-alive HTTP/1.1 server on a port of its own that reads each request by its
/// `Content-Length`, answers it, and keeps what it read: (request line, headers in lower case,
/// body).
fn recording_upstream() -> (u16, std::sync::Arc<std::sync::Mutex<Vec<(String, Vec<(String, String)>, Vec<u8>)>>>) {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen2 = seen.clone();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { return };
            let seen = seen2.clone();
            std::thread::spawn(move || {
                let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
                let mut buf: Vec<u8> = Vec::new();
                loop {
                    let end = loop {
                        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            break Some(i);
                        }
                        let mut piece = [0u8; 4096];
                        match s.read(&mut piece) {
                            Ok(0) | Err(_) => break None,
                            Ok(n) => buf.extend_from_slice(&piece[..n]),
                        }
                    };
                    let Some(end) = end else { return };
                    let head = String::from_utf8_lossy(&buf[..end]).into_owned();
                    let mut lines = head.split("\r\n");
                    let line = lines.next().unwrap_or("").to_string();
                    let headers: Vec<(String, String)> = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string())).collect();
                    let len: usize = headers.iter().find(|(k, _)| k == "content-length").and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
                    buf.drain(..end + 4);
                    while buf.len() < len {
                        let mut piece = [0u8; 4096];
                        match s.read(&mut piece) {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buf.extend_from_slice(&piece[..n]),
                        }
                    }
                    let body: Vec<u8> = buf.drain(..len).collect();
                    let answer = format!("answer to {}", line);
                    seen.lock().unwrap().push((line, headers, body));
                    let _ = s.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}", answer.len(), answer).as_bytes());
                }
            });
        }
    });
    (port, seen)
}

/// The repro: `content-length: 3` before a longer body made the upstream read the rest as a second
/// request (`GET /smuggled`, `Host: x`); now the client writes the length of what it sends, and
/// the program's framing and connection headers (any case: `Content-Length`, `TRANSFER-ENCODING`,
/// `connection`, `keep-alive`, `te`, `trailer`, `upgrade`) are left out, on `http.request`,
/// `http.post` and `http.stream`; others (`x-api-key`, `content-type`) go as before, a program's
/// `Host` among them (the user's decision, 2026-11-19: a virtual host by its address), and so does
/// a call with no headers (FEEDBACK75.md, Q-1089; branch feedback75).
#[test]
fn a_programs_framing_headers_are_not_sent() {
    let d = dir("smuggle");
    let (port, seen) = recording_upstream();
    let src = format!(
        r#"let u = "http://127.0.0.1:{port}"
let body = "abcGET /smuggled HTTP/1.1\r\nHost: x\r\n\r\n"
let a = http.request("POST", u + "/first", body, {{ "content-length": "3", "content-type": "text/plain" }})
print(a.body)
let b = http.post(u + "/second", "hello", {{ "Content-Length": "1", "TRANSFER-ENCODING": "chunked", "connection": "close", "keep-alive": "timeout=5", "te": "trailers", "trailer": "x-t", "upgrade": "h2c", "host": "evil.example", "x-api-key": "k1", "accept": "text/plain" }})
print(b.body)
let c = http.get(u + "/third")
print(c.body)
let e = http.stream(u + "/fourth", (piece) => print(piece), {{ method: "POST", body: "12345", headers: {{ "content-length": "2", "x-one": "1" }} }})
print(e.status)
"#
    );
    let (code, out, err) = script(&d, &src, &[], 60);
    assert_eq!(code, Some(0), "stdout: {}\nstderr: {}", out, err);
    let seen = seen.lock().unwrap().clone();
    let lines: Vec<&str> = seen.iter().map(|(l, _, _)| l.as_str()).collect();
    assert_eq!(lines, ["POST /first HTTP/1.1", "POST /second HTTP/1.1", "GET /third HTTP/1.1", "POST /fourth HTTP/1.1"], "{}", out);
    assert_eq!(seen[0].2, b"abcGET /smuggled HTTP/1.1\r\nHost: x\r\n\r\n");
    assert_eq!(seen[1].2, b"hello");
    assert_eq!(seen[3].2, b"12345");
    let header = |i: usize, k: &str| seen[i].1.iter().filter(|(n, _)| n == k).map(|(_, v)| v.clone()).collect::<Vec<_>>();
    assert_eq!(header(0, "content-length"), ["38"]);
    assert_eq!(header(0, "content-type"), ["text/plain"]);
    assert_eq!(header(1, "content-length"), ["5"]);
    assert_eq!(header(1, "host"), ["evil.example"]);
    assert_eq!(header(0, "host"), [format!("127.0.0.1:{}", port)]);
    for k in ["transfer-encoding", "connection", "keep-alive", "te", "trailer", "upgrade"] {
        assert!(header(1, k).is_empty(), "{}: {:?}", k, seen[1].1);
    }
    assert_eq!(header(1, "x-api-key"), ["k1"]);
    assert_eq!(header(1, "accept"), ["text/plain"]);
    assert_eq!(header(3, "content-length"), ["5"]);
    assert_eq!(header(3, "x-one"), ["1"]);
    assert!(out.contains("answer to POST /first HTTP/1.1") && out.contains("answer to GET /third HTTP/1.1"), "{}", out);
    let _ = std::fs::remove_dir_all(&d);
}

// ---- Q-1093: IPv4 inside IPv6 ------------------------------------------------------------------

/// `::ffff:0:a.b.c.d` (IPv4-translated) is judged as the IPv4 address in it, as `::ffff:a.b.c.d`,
/// NAT64 and 6to4 are, and the rest of 0::/8 is not public; so is ISATAP's `::5efe:a.b.c.d` (and
/// `::200:5efe:`) where its IPv4 address is not public; ordinary public addresses still are,
/// and `[http] private = false` refuses what `public_only` refuses (FEEDBACK75.md, Q-1093; branch
/// feedback75).
#[test]
fn ipv4_translated_addresses_are_judged_as_ipv4() {
    use quillo::fetch::public_ip;
    use quillo::policy::private_ip;
    let ip = |s: &str| s.parse::<std::net::IpAddr>().unwrap();
    for a in [
        "::ffff:0:127.0.0.1",
        "::ffff:0:10.0.0.1",
        "::ffff:0:169.254.169.254",
        "::ffff:0:0.0.0.0",
        "::ffff:0:192.168.1.1",
        "::ffff:1:7f00:1",
        "0:1::7f00:1",
        "ff::1",
        "::1",
        "::",
        "::127.0.0.1",
        "::ffff:127.0.0.1",
        "64:ff9b::a00:1",
        "2002:7f00:1::",
        "2a00:1450:4001::5efe:7f00:1",
        "2a00:1450:4001::200:5efe:a00:1",
        "2a00:1450:4001::5efe:a9fe:a9fe",
    ] {
        assert!(!public_ip(ip(a)), "{} taken for public", a);
        assert!(private_ip(ip(a)), "{} not refused under private = false", a);
    }
    for a in ["::ffff:0:8.8.8.8", "::ffff:8.8.8.8", "8.8.8.8", "2a00:1450:4001:80b::200e", "2606:4700::1111", "2a00:1450:4001::5efe:808:808", "64:ff9b::808:808", "100:0:0:1::1"] {
        assert!(public_ip(ip(a)), "{} taken for not public", a);
        assert!(!private_ip(ip(a)), "{} refused under private = false", a);
    }
}

/// The repro: `http.get` of `http://[::ffff:0:127.0.0.1]:port/` with `public_only` was connected
/// to; now it is refused before anything connects, as `[::ffff:127.0.0.1]` was, and a public
/// name still is not refused for its form (FEEDBACK75.md, Q-1093; branch feedback75).
#[test]
fn public_only_refuses_an_ipv4_translated_address() {
    let d = dir("translated");
    let src = "for h in [\"[::ffff:0:127.0.0.1]\", \"[::ffff:0:7f00:1]\", \"[::ffff:127.0.0.1]\", \"[0:1::7f00:1]\"] {\n  print(http.get(`http://${h}:9/x`, {}, { public_only: true }))\n}\n";
    let (code, out, err) = script(&d, src, &[], 60);
    assert_eq!(code, Some(0), "stdout: {}\nstderr: {}", out, err);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 4, "{}", out);
    for l in lines {
        assert!(l.contains("refused by public_only") && l.contains("is not on the public internet"), "{}", out);
    }
    let _ = std::fs::remove_dir_all(&d);
}

// ---- Q-1075: one search on a long text ---------------------------------------------------------

/// `n` bytes of `a` and `b`, as random as the audit's.
fn ab_text(n: usize) -> String {
    let mut x = 7u32;
    (0..n)
        .map(|_| {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            if (x >> 16) & 1 == 0 {
                'a'
            } else {
                'b'
            }
        })
        .collect()
}

/// The repro: `re.test` and `re.find` of `[ab]*a[ab]{900}c` (and four such joined by `|`) on
/// 200 KB of `a` and `b` held an instance 2 to 5 s, past `--max-compute`, and so did the first
/// search of `re.find_all`, `re.replace` and `re.split`; now each gives an error value that says
/// why, quickly: the search that gives up does, where the crate went on slowly (FEEDBACK75.md,
/// Q-1075; branch feedback75).
#[test]
fn a_costly_search_gives_its_error_quickly() {
    let d = dir("re-costly");
    std::fs::write(d.join("t.txt"), ab_text(200_000)).unwrap();
    let p4 = "[ab]*a[ab]{900}c|[ab]*b[ab]{900}d|[ab]*a[ab]{900}e|[ab]*b[ab]{900}f";
    for p in ["[ab]*a[ab]{900}c", p4] {
        for (f, rest) in [("re.test", ""), ("re.find", ""), ("re.find_all", ""), ("re.replace", ", \"-\""), ("re.split", "")] {
            let t = thread_cpu();
            let got = run(&d, &format!("{}(read_file(\"t.txt\"), {:?}{})", f, p, rest));
            // The guard gives up at its time, a tenth of a second and half a microsecond a byte of
            // its thread's own: 0.2 s here.
            assert!((thread_cpu() - t) < Duration::from_millis(if cfg!(debug_assertions) { 20_000 } else { 2_000 }), "{} {}: {:?}", f, p, (thread_cpu() - t));
            let want = format!("`{}`: the pattern is too costly for this text", f);
            assert!(got.starts_with("err(") && got.contains(&want) && got.contains("these 200000 bytes"), "{}", got);
        }
    }
    let _ = std::fs::remove_dir_all(&d);
}

const KIT: &str = r#"%%|\{\{\s{0,9}[\w.-]{1,100}\s{0,9}\}\}|\{[\w.-]{1,100}(?:,[^{}]{0,200}(?:\{[^{}]{0,200}\}[^{}]{0,200}){0,20})?\}|\$\{\w{1,100}\}|%\([\w.-]{1,100}\)[sdif]|%(?:[0-9]{1,3}\$)?[-+0#]{0,4}[0-9]{0,4}(?:\.[0-9]{1,4})?[sdifgxXcu@]"#;

/// A long pattern on a long ordinary text still works: the UI kit's translation checker (about
/// 8,500 places) finds every placeholder in 585 KB of messages, and a pattern of 1,500 places
/// with groups finds each of its 15,000 matches with them, through `re.find_all`, `re.find`,
/// `re.test`, `re.replace` and `re.split`; an empty text and one character too (FEEDBACK75.md,
/// Q-1075; branch feedback75).
#[test]
fn a_long_ordinary_pattern_on_a_long_text_still_works() {
    let d = dir("re-long");
    std::fs::write(d.join("kit.txt"), KIT).unwrap();
    std::fs::write(d.join("msgs.txt"), "Hello {name}, you have {count, plural, one {# message} other {# messages}} and %d items; see ${link} or {{ title }}. ".repeat(5000)).unwrap();
    std::fs::write(d.join("words.txt"), "Lorem ipsum dolor sit amet, consectetur adipiscing elit, 12345 sed do. ".repeat(15_000)).unwrap();
    assert_eq!(run(&d, r#"len(re.find_all(read_file("msgs.txt"), read_file("kit.txt")))"#), "25000");
    assert_eq!(run(&d, r#"map(take(re.find_all(read_file("msgs.txt"), read_file("kit.txt")), 5), (m) => m.text)"#), r#"["{name}", "{count, plural, one {# message} other {# messages}}", "%d", "${link}", "{{ title }}"]"#);
    assert_eq!(run(&d, r#"re.test(read_file("msgs.txt"), read_file("kit.txt"))"#), "true");
    let p = r#""(Lorem) (ipsum)[a-z ,.0-9]{0,600}?(sed)|x[ab]{900}""#;
    assert_eq!(run(&d, &format!(r#"len(re.find_all(read_file("words.txt"), {}))"#, p)), "15000");
    assert_eq!(run(&d, &format!(r#"re.find_all(read_file("words.txt"), {})[14999].groups"#, p)), r#"["Lorem", "ipsum", "sed"]"#);
    assert_eq!(run(&d, &format!(r#"re.find(read_file("words.txt"), {}).end"#, p)), "66");
    // Each 71 characters: the 66 matched become the 3 of `sed`.
    assert_eq!(run(&d, &format!(r#"len(re.replace(read_file("words.txt"), {}, "$3"))"#, p)), (15_000 * (71 - 66 + 3)).to_string());
    assert_eq!(run(&d, &format!(r#"len(re.split(read_file("words.txt"), {}))"#, p)), "15001");
    assert_eq!(run(&d, &format!(r#"re.test("", {})"#, p)), "false");
    assert_eq!(run(&d, &format!(r#"re.find("L", {})"#, p)), "null");
    let _ = std::fs::remove_dir_all(&d);
}

/// What a search through the pattern that gives up finds is what the crate finds: each case run
/// on 80 KB with a branch that never matches (`|[ab]{1000}\u0001`, open to a match begun inside it,
/// which sends it that way) and
/// without, matches, groups, names, `\b`, `^` and `$` at lines, other scripts and empty matches
/// alike (FEEDBACK75.md, Q-1075; branch feedback75).
#[test]
fn the_search_that_gives_up_finds_what_the_crate_finds() {
    let d = dir("re-same");
    std::fs::write(d.join("t.txt"), "Hello wörld 12 foo_bar Baz\nnext line: 3.5 times, café ☕ end. x=7;y=88\r\nZZ  tail ".repeat(1000)).unwrap();
    for (p, flags) in [(r"(\w+)\s(\d+)", ""), (r"(?<word>[a-z]+)\b", "i"), (r"^(\w+)", "m"), (r"(\d+)$", "m"), ("(caf.)|(end)", ""), (r"(x|y)=(\d+)", ""), ("☕|é", ""), (r"(L)(i)(n)(e)?", "i")] {
        let call = |pad: &str| run(&d, &format!("re.find_all(read_file(\"t.txt\"), {:?}, {:?})", format!("{}{}", p, pad), flags));
        let plain = call("");
        assert!(plain.len() > 100, "{}: {}", p, plain);
        assert_eq!(call(r"|[ab]{1000}\u0001"), plain, "{} {}", p, flags);
    }
    for (p, flags) in [(r"\b", ""), ("a*", ""), ("", ""), (r"\s*", "")] {
        let call = |f: &str, pad: &str| run(&d, &format!("{}(read_file(\"t.txt\"), {:?}, \"<$0>\", {:?})", f, format!("{}{}", p, pad), flags));
        assert_eq!(call("re.replace", r"|[ab]{1000}\u0001"), call("re.replace", ""), "{:?}", p);
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// What the review found: without groups, a long match is not searched again for them
/// (`(?:a*){900}` on 1 MB of `a` took 7.7 s where 0.8.8 took 2 ms); with groups, a match too long
/// to find them within a quarter of a second is the error value (`((?:a*){900})` took 1.3 s on
/// 200 KB in 0.8.8 too); and a pattern of 497 places, under the flat 500 that sent searches to the
/// guard, is guarded by what it is, on 1 MB (2.3 s in 0.8.8) (FEEDBACK75.md, Q-1075; branch
/// feedback75).
#[test]
fn the_guard_finds_groups_only_where_they_are_and_in_time() {
    let d = dir("re-review");
    std::fs::write(d.join("a1m.txt"), "a".repeat(1_000_000)).unwrap();
    std::fs::write(d.join("a200k.txt"), "a".repeat(200_000)).unwrap();
    std::fs::write(d.join("ab1m.txt"), ab_text(1_000_000)).unwrap();
    let quick = Duration::from_millis(if cfg!(debug_assertions) { 30_000 } else { 3_000 });
    let timed = |src: &str| {
        let t = thread_cpu();
        let got = run(&d, src);
        assert!((thread_cpu() - t) < quick, "{}: {:?}", src, (thread_cpu() - t));
        got
    };
    assert_eq!(timed(r#"re.find(read_file("a1m.txt"), "(?:a*){900}").end"#), "1000000");
    assert_eq!(timed(r#"len(re.find_all(read_file("a200k.txt"), "(?:a*){900}"))"#), "2");
    assert_eq!(timed(r#"re.test(read_file("a1m.txt"), "(?:a*){900}")"#), "true");
    let g = timed(r#"re.find(read_file("a200k.txt"), "((?:a*){900})")"#);
    assert!(g.starts_with("err(") && g.contains("`re.find`: the pattern is too costly for this text"), "{}", g);
    // A short one still has its groups.
    assert_eq!(timed(r#"re.find(repeat("a", 100) + "b", "((?:a*){900})b").groups[0]"#), format!("{:?}", "a".repeat(100)));
    let e = timed(r#"re.test(read_file("ab1m.txt"), "[ab]*a[ab]{494}c")"#);
    assert!(e.starts_with("err(") && e.contains("`re.test`: the pattern is too costly for this text"), "{}", e);
    let _ = std::fs::remove_dir_all(&d);
}

/// An alternation of 600 words between `\b`s, with `i`, on 600 KB, finds each word, any case: a
/// literal alternation is no danger, and goes by the crate as before (the review saw it guarded,
/// 5 ms became 21) (FEEDBACK75.md, Q-1075; branch feedback75).
#[test]
fn an_alternation_of_words_goes_as_before() {
    let d = dir("re-words");
    fn next(x: &mut u32) -> usize {
        *x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        (*x >> 16) as usize
    }
    let syll = ["ka", "lo", "mi", "ne", "pu", "ra", "si", "to", "vu", "ze", "bri", "dan", "fel", "gor", "hu"];
    let word = |x: &mut u32, n: usize| (0..n).map(|_| syll[next(x) % syll.len()]).collect::<String>();
    let mut x = 11u32;
    let mut set = std::collections::BTreeSet::new();
    while set.len() < 600 {
        let n = 2 + set.len() % 3;
        set.insert(word(&mut x, n));
    }
    let list: Vec<String> = set.iter().cloned().collect();
    let mut text = String::new();
    let mut want = 0;
    while text.len() < 600_000 {
        let w = if next(&mut x) % 20 == 0 { list[next(&mut x) % list.len()].clone() } else { let n = 2 + next(&mut x) % 4; word(&mut x, n) };
        if set.contains(&w) {
            want += 1;
        }
        let w = if next(&mut x) % 7 == 0 { w.to_uppercase() } else { w };
        text.push_str(&w);
        text.push(' ');
    }
    std::fs::write(d.join("t.txt"), &text).unwrap();
    std::fs::write(d.join("p.txt"), format!(r"\b(?:{})\b", list.join("|"))).unwrap();
    assert_eq!(run(&d, r#"len(re.find_all(read_file("t.txt"), read_file("p.txt"), "i"))"#), want.to_string());
    assert_eq!(run(&d, r#"re.test(read_file("t.txt"), read_file("p.txt"), "i")"#), "true");
    let _ = std::fs::remove_dir_all(&d);
}

/// What the second review found past shape scoring, each bounded now that the crate is used only
/// where its own worst (its NFA's states times the bytes) fits a quarter of a second, and the guard
/// elsewhere holds its work to the text: written-out repeats (`a` + `x[ab]`×450 + `c` on `ax`/`bx`,
/// 1.2 s on 1 MB; `ab`×2000 + `[cd]` on `abab`, 9 s), the fuzzer's `a\s`×450, `k[0-9a]0`×300 and
/// `[kK]a[ax]`×300 with `i`, a text that makes a state every 21 bytes for a 4-branch
/// `(?:x*[ab]){900}` (5.3 s), escapes and cases under `i` (`\x41`, 5.3 s; `ϑ`/`ϴ`, 3.5 s), and
/// `re.valid` of a pattern nested 124 deep (2.4 s while shapes were scored): each is its answer or
/// the error value, within a second (FEEDBACK75.md, Q-1075, second review; branch feedback75).
#[test]
fn every_shape_the_review_found_is_bounded() {
    let d = dir("re-shapes");
    let mut x = 5u32;
    let mut pick = |n: usize| {
        x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        (x >> 16) as usize % n
    };
    let mut gen = |parts: &[&str], len: usize| {
        let mut t = String::with_capacity(len + 16);
        while t.len() < len {
            t.push_str(parts[pick(parts.len())]);
        }
        t
    };
    let x20 = "x".repeat(20);
    let (ax20, bx20) = (format!("a{}", x20), format!("b{}", x20));
    let files = [
        ("abx.txt", gen(&["ax", "bx"], 1_000_000)),
        ("abab.txt", "ab".repeat(500_000)),
        ("as.txt", gen(&["a ", "a\t", "a\n"], 1_000_000)),
        ("k0.txt", gen(&["k00", "k10", "k20", "k30", "k40", "k50", "k60", "k70", "k80", "k90", "ka0"], 1_000_000)),
        ("ka.txt", gen(&["kaa", "kax", "Kaa", "Kax"], 1_000_000)),
        ("abx20.txt", gen(&[&ax20, &bx20], 1_000_000)),
        ("aab.txt", gen(&["a", "A", "b"], 1_000_000)),
        ("thb.txt", gen(&["ϑ", "θ", "b"], 750_000)),
    ];
    for (name, text) in &files {
        std::fs::write(d.join(name), text).unwrap();
    }
    let g4 = "[abx]*a(?:x*[ab]){900}c|[abx]*b(?:x*[ab]){900}d|[abx]*a(?:x*[ab]){900}e|[abx]*b(?:x*[ab]){900}f".to_string();
    let cases: Vec<(String, &str, &str)> = vec![
        (format!("a{}c", "x[ab]".repeat(450)), "abx.txt", ""),
        (format!("a{}c", "x[ab]".repeat(900)), "abx.txt", ""),
        (format!("{}[cd]", "ab".repeat(2000)), "abab.txt", ""),
        (format!(r"{}c", r"a\s".repeat(450)), "as.txt", ""),
        (format!("{}z", "k[0-9a]0".repeat(300)), "k0.txt", ""),
        (format!("{}z", "[kK]a[ax]".repeat(300)), "ka.txt", "i"),
        (g4, "abx20.txt", ""),
        (r"(?:\x41|b)*\x41(?:\x41|b){900}c".to_string(), "aab.txt", "i"),
        ("(?:ϑ|b)*ϴ(?:ϑ|b){900}c".to_string(), "thb.txt", "i"),
    ];
    // At their time: 0.6 s of the thread's own on 1 MB.
    let quick = Duration::from_millis(if cfg!(debug_assertions) { 30_000 } else { 3_000 });
    for (p, file, flags) in &cases {
        std::fs::write(d.join("p.txt"), p).unwrap();
        for f in ["re.test", "re.find"] {
            let t = thread_cpu();
            let got = run(&d, &format!("{}(read_file({:?}), read_file(\"p.txt\"), {:?})", f, file, flags));
            assert!((thread_cpu() - t) < quick, "{} {:.60} on {}: {:?}", f, p, file, (thread_cpu() - t));
            assert!(got == "false" || got == "null" || got.contains("the pattern is too costly for this text"), "{} {:.60}: {}", f, p, got);
        }
    }
    // Nested 124 deep, 100 KB: read, not scored.
    std::fs::write(d.join("n.txt"), format!("{}{}{}", "(?:".repeat(124), "ab".repeat(50_000), ")?".repeat(124))).unwrap();
    let t = thread_cpu();
    assert_eq!(run(&d, r#"re.valid(read_file("n.txt"))"#), "true");
    assert!((thread_cpu() - t) < quick * 2, "{:?}", (thread_cpu() - t));
    let _ = std::fs::remove_dir_all(&d);
}

/// What the third review found: the crate's worst is more than its states, where groups are
/// asked for (the PikeVM copies every slot of every thread at each byte: `(?:(a)|(a)|…×1000)*` on
/// the 20,784 `a` it allowed took 5 s in `re.find`, ×3000 on 6,939 took 14 s, 11 and 27 s in 0.8.8)
/// and where a state's transitions are many ranges (a class of 63 ranges, 0.9 s on its allowed
/// 68,756 bytes); a match of 20 KB with a thousand groups passed the group bound and took 5.4 s.
/// Each is now held to the crate's worst counted with them, or to the guard's time and room: the
/// groups of a match the PikeVM's bound refuses would be found by the bounded backtracker, whose
/// stack at its worst (16 bytes a visit, a thousand alternatives a byte here) is past the 256 MiB a
/// guarded search may make for itself, so these are the error value, quickly (changed with the
/// final check, which found that stack unbounded: the 20 KB ones were answered at 974 MB); `re.test`
/// of the same, which asks for no group, is its answer (FEEDBACK75.md, Q-1075, third review, last
/// and final checks; branch feedback75).
#[test]
fn groups_and_wide_classes_count_in_the_crates_worst() {
    let d = dir("re-slots");
    let alts = |k: usize| format!("(?:{})*", vec!["(a)"; k].join("|"));
    std::fs::write(d.join("p1000.txt"), alts(1000)).unwrap();
    std::fs::write(d.join("p3000.txt"), alts(3000)).unwrap();
    std::fs::write(d.join("p1000b.txt"), alts(1000) + "b").unwrap();
    let odd: String = (1..0x7e).step_by(2).map(|c| format!("\\x{:02x}", c)).collect::<String>() + "\\x7e";
    std::fs::write(d.join("px.txt"), format!("[\\x7d\\x7e]*\\x7d[{}]{{900}}c", odd)).unwrap();
    std::fs::write(d.join("a20784.txt"), "a".repeat(20_784)).unwrap();
    std::fs::write(d.join("a6939.txt"), "a".repeat(6_939)).unwrap();
    std::fs::write(d.join("a20kb.txt"), "a".repeat(20_000) + "b").unwrap();
    std::fs::write(d.join("a100k.txt"), "a".repeat(100_000)).unwrap();
    std::fs::write(d.join("a50k.txt"), "a".repeat(50_000)).unwrap();
    let mut x = 2u32;
    let tilde: String = (0..68_756)
        .map(|_| {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            if (x >> 16) & 1 == 0 {
                '}'
            } else {
                '~'
            }
        })
        .collect();
    std::fs::write(d.join("tilde.txt"), tilde).unwrap();
    let quick = Duration::from_millis(if cfg!(debug_assertions) { 20_000 } else { 2_000 });
    for (call, want) in [
        (r#"re.find(read_file("a20784.txt"), read_file("p1000.txt"))"#, None),
        (r#"re.find(read_file("a6939.txt"), read_file("p3000.txt"))"#, None),
        (r#"re.find(read_file("a20kb.txt"), read_file("p1000b.txt"))"#, None),
        (r#"re.find(read_file("a100k.txt"), read_file("p1000.txt"))"#, None),
        (r#"re.find(read_file("a50k.txt"), read_file("p3000.txt"))"#, None),
        (r#"re.test(read_file("tilde.txt"), read_file("px.txt"))"#, None),
        (r#"re.find(read_file("tilde.txt"), read_file("px.txt"))"#, None),
        (r#"re.test(read_file("a20784.txt"), read_file("p1000.txt"))"#, Some("true")),
        (r#"re.find("aab", read_file("p1000b.txt")).end"#, Some("3")),
    ] {
        let t = thread_cpu();
        let got = run(&d, call);
        assert!((thread_cpu() - t) < quick, "{}: {:?}", call, (thread_cpu() - t));
        match want {
            Some(w) => assert_eq!(got, w, "{}", call),
            None => assert!(got.starts_with("err(") && got.contains("the pattern is too costly for this text"), "{}: {}", call, got),
        }
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// The UI kit's translation checker reads text with many braces, up to 10 MB, as 0.8.8 did: the
/// rule that gave up where fewer bytes than the NFA's 74,971 states were read for each state made
/// refused it on 1 MB (found in review); the guard now gives up only at its time. The counts are
/// 0.8.8's on the same texts. A debug build reads 1 and 2 MB (its guard is slower, its time not).
#[test]
fn the_kit_checker_reads_text_full_of_braces() {
    let _heavy = heavy();
    let d = dir("re-kitmix");
    std::fs::write(d.join("kit.txt"), KIT).unwrap();
    let alpha: Vec<char> = " #$%,.abcdefghijklmnop{}".chars().collect();
    let gen = |n: usize| {
        let mut x = 7u32;
        (0..n)
            .map(|_| {
                x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                alpha[(x >> 16) as usize % alpha.len()]
            })
            .collect::<String>()
    };
    let sizes: &[(usize, &str, &str)] = &[(1, "16641", "888838"), (2, "33322", "1774434"), (5, "83224", "4444111"), (10, "166508", "8881642")];
    for (mb, count, replaced) in sizes.iter().take(if cfg!(debug_assertions) { 2 } else { 4 }) {
        std::fs::write(d.join("t.txt"), gen(mb * 1_000_000)).unwrap();
        assert_eq!(&run(&d, r#"len(re.find_all(read_file("t.txt"), read_file("kit.txt")))"#), count, "{} MB", mb);
        assert_eq!(&run(&d, r#"len(re.replace(read_file("t.txt"), read_file("kit.txt"), ""))"#), replaced, "{} MB", mb);
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// Under `--max-compute`, a guarded search stops at the request's limit, not at its own time:
/// `[ab]*a[ab]{900}c` on 10 MB of `a` and `b` under `--max-compute 1s`, where the guard alone would
/// give 1.6 s, ends the request near 1 s with the compute stop, as Q-976's searches end it; and the
/// server goes on (FEEDBACK75.md, Q-1075; branch feedback75).
#[test]
fn a_guarded_search_stops_at_max_compute() {
    let _heavy = heavy();
    let d = dir("re-compute");
    let src = "route POST /re {\n    return str(re.test(request.body, \"[ab]*a[ab]{900}c\"))\n}\nroute GET /ok {\n    return str(re.test(\"xabcx\", \"a.c\"))\n}\n";
    let srv = Run::start(&d, src, &["--max-compute", "1s"]);
    let body = ab_text(10_000_000);
    let head = format!("POST /re HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n", body.len());
    let t = Instant::now();
    let (status, answer) = raw(srv.port, &head, body.as_bytes());
    let took = t.elapsed();
    assert_eq!(status, 500, "{}", answer);
    assert!(took >= Duration::from_millis(900) && took < Duration::from_millis(if cfg!(debug_assertions) { 20_000 } else { 2_500 }), "{:?}", took);
    let log = srv.log();
    assert!(log.contains("this request computed for longer than --max-compute allows"), "{}", log);
    assert_eq!(raw(srv.port, "GET /ok HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n", b""), (200, "true".to_string()));
    let _ = std::fs::remove_dir_all(&d);
}

/// `n` bytes of `alpha`, the same each time.
fn mixed_text(n: usize, alpha: &str, seed: u32) -> String {
    let alpha: Vec<char> = alpha.chars().collect();
    let mut x = seed;
    (0..n)
        .map(|_| {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            alpha[(x >> 16) as usize % alpha.len()]
        })
        .collect()
}

/// A pattern with a literal inside that every match holds and none in front (`[^@\s]+@...`) is
/// searched by the guard from that literal, as the crate searches it ("reverse inner"), and finds
/// what the crate finds: every match, the first and the last, with their groups and names, on
/// 2 MB where the guard takes them, also on texts made to send the search forward instead (a
/// literal on every byte, and long runs before each one), and in time. Without it the guard read
/// every byte: an email pattern on 990 KB 22% slower than 0.8.8 (FEEDBACK75.md, Q-1075; branch
/// feedback75).
#[test]
fn the_inner_literal_search_finds_what_the_crate_finds() {
    let _heavy = heavy();
    let d = dir("re-inner");
    let texts = [
        ("mixed", mixed_text(2_000_000, "abcdefghij klm.@-_\n0123", 3)),
        ("ats", mixed_text(2_000_000, "ab@ .", 5)),
        ("runs", ("a".repeat(5_000) + "@").repeat(300) + "b.c"),
        ("only-at", "@".repeat(600_000) + "x@y.z" + &"@".repeat(600_000)),
    ];
    let pats = [r"[^@\s]+@[^@\s]+\.[^@\s]+", r"[^\s@]+@[^\s@]+", r"(\w+)@(\w+)\.(\w+)", r"[ab]+@[ab]*", r"\b\w+@", r".@."];
    let quick = Duration::from_millis(if cfg!(debug_assertions) { 60_000 } else { 3_000 });
    for (name, text) in &texts {
        std::fs::write(d.join("t.txt"), text).unwrap();
        for p in pats {
            let re = regex::Regex::new(p).unwrap();
            let all: Vec<regex::Match> = re.find_iter(text).collect();
            let want = match (all.first(), all.last()) {
                (Some(a), Some(b)) => format!("[true, {}, {}, {}, {}, {}]", all.len(), a.start(), a.end(), b.start(), b.end()),
                _ => "[false, 0]".to_string(),
            };
            let t = thread_cpu();
            let got = run(&d, &format!("map([re.find_all(read_file(\"t.txt\"), {p:?})], (a) => len(a) > 0 ? [re.test(read_file(\"t.txt\"), {p:?}), len(a), a[0].start, a[0].end, a[len(a) - 1].start, a[len(a) - 1].end] : [re.test(read_file(\"t.txt\"), {p:?}), 0])[0]"));
            assert_eq!(got, want, "{} {}", name, p);
            assert!((thread_cpu() - t) < quick, "{} {}: {:?}", name, p, (thread_cpu() - t));
        }
    }
    // Groups and their names, by the guard: `named` and `$<name>` read the guard's NFA, the crate's
    // Regex not being compiled for a pattern only the guard has searched.
    let text = &texts[0].1;
    std::fs::write(d.join("t.txt"), text).unwrap();
    let p = r"(?<user>[^@\s]+)@(?<host>[^@\s]+)\.(?<top>[^@\s]+)";
    let re = regex::Regex::new(p).unwrap();
    let c = re.captures(text).unwrap();
    let want = format!("[{:?}, {:?}, {:?}]", &c["user"], &c["host"], &c["top"]);
    assert_eq!(run(&d, &format!("map([re.find(read_file(\"t.txt\"), {p:?})], (m) => [m.named.user, m.named.host, m.named.top])[0]")), want);
    let replaced = re.replace_all(text, "${top}.${host}");
    assert_eq!(run(&d, &format!("len(re.replace(read_file(\"t.txt\"), {p:?}, \"$<top>.$<host>\"))")), replaced.len().to_string());
    assert_eq!(run(&d, &format!("crypto.sha256(re.replace(read_file(\"t.txt\"), {p:?}, \"$<top>.$<host>\"))")), format!("{:?}", sha256_hex(replaced.as_bytes())));
    let _ = std::fs::remove_dir_all(&d);
}

/// SHA-256 of `bytes` in hex, by the interpreter's own `crypto.sha256` of the same text read back.
fn sha256_hex(bytes: &[u8]) -> String {
    let d = dir("re-sha");
    std::fs::write(d.join("x.txt"), bytes).unwrap();
    let out = run(&d, r#"crypto.sha256(read_file("x.txt"))"#);
    let _ = std::fs::remove_dir_all(&d);
    out.trim_matches('"').to_string()
}

/// A pattern first searched on a long text the crate may not search (the UI kit's checker on 1 MB)
/// is compiled for the guard alone; the crate's Regex is compiled the first time a call may take
/// it (a short text), once, and then each call takes its own way: the same answers as 0.8.8's
/// (FEEDBACK75.md, Q-1075; branch feedback75).
#[test]
fn a_pattern_the_guard_took_first_is_compiled_for_the_crate_when_needed() {
    let _heavy = heavy();
    let d = dir("re-lazy");
    std::fs::write(d.join("kit.txt"), KIT).unwrap();
    std::fs::write(d.join("t.txt"), mixed_text(1_000_000, " #$%,.abcdefghijklmnop{}", 7)).unwrap();
    let mut it = quillo::Interp::new(&d);
    let mut ask = |src: &str| {
        it.load_source(&format!("let out = {}\n", src), "t.qwe").unwrap_or_else(|e| panic!("{}", e));
        quillo::value::lookup(&it.globals, "out").unwrap().repr()
    };
    assert_eq!(ask(r#"len(re.find_all(read_file("t.txt"), read_file("kit.txt")))"#), "16641");
    assert!(quillo::re::kept(KIT, "").is_some());
    assert_eq!(ask(r#"re.find("a {name} and %s", read_file("kit.txt")).text"#), r#""{name}""#);
    assert_eq!(ask(r#"len(re.find_all("a {name} and %s and ${x}", read_file("kit.txt")))"#), "3");
    assert_eq!(ask(r#"len(re.replace(read_file("t.txt"), read_file("kit.txt"), ""))"#), "888838");
    assert_eq!(ask(r#"re.test("no braces here", read_file("kit.txt"))"#), "false");
    let _ = std::fs::remove_dir_all(&d);
}

/// A line of words, `n` bytes, the same each time.
fn words_line(n: usize) -> String {
    let words = ["lorem", "ipsum", "at", "dolor", "sit", "amet", "@", "consectetur", "elit"];
    let mut x = 9u32;
    let mut text = String::with_capacity(n + 16);
    while text.len() < n {
        x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        text.push_str(words[(x >> 16) as usize % words.len()]);
        text.push(' ');
    }
    text.truncate(n);
    text
}

/// A pattern with groups that is not one-pass, whose one match is a whole line, gives the groups
/// 0.8.8 gave (the regex crate's own captures here), in time, on 300 KB: `^(\S+)\s*(.*)$` and
/// `^(.*\S)\s+(?:at|@)\s+(\S.*)$`. On a 10 MB line, where 0.8.8 took 187 and 492 ms, each is now
/// the error value, quickly: the PikeVM's bound is past the budget there, and the bounded
/// backtracker that found them in about 50 ms made 521 MB for its stack, which the final check
/// found unbounded; it now has 256 MiB at the most (changed with the final check; FEEDBACK75.md,
/// Q-1075, last and final checks; branch feedback75).
#[test]
fn a_long_match_finds_its_groups() {
    let _heavy = heavy();
    let d = dir("re-longline");
    let pats = [r"^(\S+)\s*(.*)$", r"^(.*\S)\s+(?:at|@)\s+(\S.*)$"];
    let text = words_line(300_000);
    std::fs::write(d.join("t.txt"), &text).unwrap();
    for p in pats {
        let c = regex::Regex::new(p).unwrap().captures(&text).unwrap();
        let (m, g1, g2) = (c.get(0).unwrap(), c.get(1).unwrap(), c.get(2).unwrap());
        let want = format!("[{}, {}, {}, {}]", m.start(), m.end(), g1.len(), g2.len());
        let t = thread_cpu();
        let got = run(&d, &format!("map([re.find(read_file(\"t.txt\"), {:?})], (m) => [m.start, m.end, len(m.groups[0]), len(m.groups[1])])[0]", p));
        assert_eq!(got, want, "{}", p);
        assert!((thread_cpu() - t) < Duration::from_millis(if cfg!(debug_assertions) { 30_000 } else { 3_000 }), "{}: {:?}", p, (thread_cpu() - t));
    }
    std::fs::write(d.join("t.txt"), words_line(10_000_000)).unwrap();
    for p in pats {
        let t = thread_cpu();
        let got = run(&d, &format!("re.find(read_file(\"t.txt\"), {:?})", p));
        assert!(got.starts_with("err(") && got.contains("the pattern is too costly for this text"), "{}: {}", p, got);
        assert!((thread_cpu() - t) < Duration::from_millis(if cfg!(debug_assertions) { 30_000 } else { 3_000 }), "{}: {:?}", p, (thread_cpu() - t));
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// `n` bytes of `alpha` after `head`, the same each time (the reviewer's lines).
fn line_of(head: &str, alpha: &str, n: usize, seed: u32) -> String {
    let mut out = head.to_string();
    out.push_str(&mixed_text(n - head.len(), alpha, seed));
    out
}

/// The reviewer's lines: `^(\S+)\s*(.*)$` on a 1 MB line of `abcdefgh-0123 ` (spaces in it, no line
/// break) and on a 2 MB line (a word, a space, then `abcdefgh{}:,"0123`), and on 4 MB of the
/// first, give the groups 0.8.8 gave (the regex crate's own here), in time: the bounded
/// backtracker's stack was charged at its worst before it ran (a frame of 16 bytes for each of the
/// 49 states at each byte), which refused them from 340 KB, where 0.8.8 took 20 and 50 ms; it is
/// now charged as it grows (FEEDBACK75.md, Q-1075, last check; branch feedback75).
#[test]
fn a_long_line_with_groups_answers_as_before() {
    let _heavy = heavy();
    let d = dir("re-lines");
    let p = r"^(\S+)\s*(.*)$";
    let re = regex::Regex::new(p).unwrap();
    let lines = [line_of("x", "abcdefgh-0123 ", 1_000_000, 11), line_of("word ", "abcdefgh{}:,\"0123", 2_000_000, 12), line_of("x", "abcdefgh-0123 ", 4_000_000, 13)];
    for text in &lines {
        std::fs::write(d.join("t.txt"), text).unwrap();
        let c = re.captures(text).unwrap();
        let (m, g1, g2) = (c.get(0).unwrap(), c.get(1).unwrap(), c.get(2).unwrap());
        let want = format!("[{}, {}, {}, {}]", m.start(), m.end(), g1.len(), g2.len());
        let t = thread_cpu();
        let got = run(&d, &format!("map([re.find(read_file(\"t.txt\"), {:?})], (m) => [m.start, m.end, len(m.groups[0]), len(m.groups[1])])[0]", p));
        assert_eq!(got, want, "{} bytes", text.len());
        assert!((thread_cpu() - t) < Duration::from_millis(if cfg!(debug_assertions) { 60_000 } else { 3_000 }), "{} bytes: {:?}", text.len(), (thread_cpu() - t));
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// Where the backtracker's stack does grow past its room, it is refused within it: the 4 MB line
/// above, whose visits (25 MB) and time fit `--max-memory 64MB` but whose stack does not, is the
/// error value under that limit, the process growing by no more than that and the allocator's
/// margin over its own size; without the limit it is the groups (FEEDBACK75.md, Q-1075, last check;
/// branch feedback75).
#[test]
fn a_stack_past_its_room_is_refused_within_it() {
    let _heavy = heavy();
    let d = dir("re-stack");
    let text = line_of("x", "abcdefgh-0123 ", 4_000_000, 13);
    let c = regex::Regex::new(r"^(\S+)\s*(.*)$").unwrap().captures(&text).unwrap();
    let want = format!("[{}, {}, {}]", c.get(0).unwrap().start(), c.get(0).unwrap().end(), c.get(1).unwrap().len());
    std::fs::write(d.join("t.txt"), &text).unwrap();
    let src = format!("let t = read_file(\"t.txt\")\n{}let m = re.find(t, \"^(\\\\S+)\\\\s*(.*)$\")\nprint(is_err(m) ? str(m) : str([m.start, m.end, len(m.groups[0])]))\nprint(\"done\")\nsleep(60000)\n", READY);
    std::fs::write(d.join("s.qwe"), src).unwrap();
    for (args, limited) in [(&["--max-memory", "64MB"][..], true), (&[][..], false)] {
        let mut run = Measured::start(&d, args);
        run.go(&d);
        let out = run.wait_for("done\n", if cfg!(debug_assertions) { 300 } else { 60 }).to_string();
        let first = out.lines().find(|l| *l != "ready").unwrap_or_default();
        if limited {
            assert!(first.starts_with("err(") && first.contains("the pattern is too costly for this text"), "{}", out);
            // Its 64 MiB, and 32 MiB for the allocator and the call's own; its stack alone would be
            // over 100 MB.
            if let Some(kb) = run.grew_kb() {
                assert!(kb < (64 + 32) * 1024, "{} KB over its own {:?} KB", kb, run.base);
            }
        } else {
            assert_eq!(first, want, "{}", out);
        }
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// A guarded search makes for its own work no more than `--max-memory` allows, nor than 256 MiB
/// where nothing is set, and where its work would need more, it goes another way or gives up: the
/// PikeVM's cache is made only where the PikeVM runs (a `re.test` of `(?:(a)|…×3000)*` on 10 KB made
/// 785 MB for it and never used it), and the bounded backtracker's set of visits and its stack at
/// their worst are asked for first (a thousand groups on 20 KB took 974 MB, the 10 MB line 521 MB).
/// Each answer is the same with the limit and without, and the process grows over its own size by
/// no more than it may make: 64 MiB under the limit; without it, the 256 MiB room, which the
/// backtracker's stack grows to before it is refused (changed with the reviewer's last check, which
/// found a 1 MB line refused on the stack's worst), each with the allocator's margin; measured as
/// growth since CI's instrumented build is much larger of its own (FEEDBACK75.md, Q-1075, final
/// and last checks; branch feedback75).
#[test]
fn a_guarded_search_makes_no_more_than_it_may() {
    let _heavy = heavy();
    let d = dir("re-room");
    let alts = |k: usize| format!("(?:{})*", vec!["(a)"; k].join("|"));
    std::fs::write(d.join("p3000.txt"), alts(3000)).unwrap();
    std::fs::write(d.join("p1000b.txt"), alts(1000) + "b").unwrap();
    std::fs::write(d.join("b10k.txt"), "b".repeat(10_000)).unwrap();
    std::fs::write(d.join("a20kb.txt"), "a".repeat(20_000) + "b" + &"c".repeat(40_000)).unwrap();
    std::fs::write(d.join("line.txt"), words_line(10_000_000)).unwrap();
    let read = "let b10k = read_file(\"b10k.txt\")\nlet p3000 = read_file(\"p3000.txt\")\nlet a20kb = read_file(\"a20kb.txt\")\nlet p1000b = read_file(\"p1000b.txt\")\nlet line = read_file(\"line.txt\")\n";
    let work = "print(re.test(b10k, p3000))\nlet m = re.find(b10k, p3000)\nprint(str([m.start, m.end, len(m.groups)]))\nprint(re.find(a20kb, p1000b))\nprint(re.find(line, \"^(\\\\S+)\\\\s*(.*)$\"))\nprint(\"done\")\nsleep(60000)\n";
    std::fs::write(d.join("s.qwe"), format!("{}{}{}", read, READY, work)).unwrap();
    for args in [&[][..], &["--max-memory", "64MB"][..]] {
        let mut run = Measured::start(&d, args);
        run.go(&d);
        let out = run.wait_for("done\n", if cfg!(debug_assertions) { 300 } else { 60 }).to_string();
        let lines: Vec<&str> = out.lines().filter(|l| *l != "ready").collect();
        assert_eq!(lines.len(), 5, "{:?}: {}", args, out);
        assert_eq!(lines[0], "true", "{:?}", args);
        assert_eq!(lines[1], "[0, 0, 3000]", "{:?}", args);
        for l in &lines[2..4] {
            assert!(l.starts_with("err(") && l.contains("the pattern is too costly for this text"), "{:?}: {}", args, l);
        }
        // Over the process's own size (the texts read): the room, and a margin for the allocator
        // and the call's own; each was 521 MB or more at its height.
        if let Some(kb) = run.grew_kb() {
            let most = if args.is_empty() { 256 + 64 } else { 64 + 32 };
            assert!(kb < most * 1024, "{:?}: {} KB over its own {:?} KB", args, kb, run.base);
        }
    }
    let _ = std::fs::remove_dir_all(&d);
}

// ---- Q-1092: stats.mode ------------------------------------------------------------------------

/// `n` floats in [1, 2) whose bits all hashed to the same low 24 bits under the old fixed hash
/// (`((x ^ x>>29) * K) ^ (x>>32)`), as the audit's hashflood.py made them.
fn colliding_floats(n: usize) -> Vec<f64> {
    const K: u64 = 0x9E37_79B9_7F4A_7C15;
    const M24: u64 = (1 << 24) - 1;
    let mut inv = K;
    for _ in 0..6 {
        inv = inv.wrapping_mul(2u64.wrapping_sub(K.wrapping_mul(inv)));
    }
    let mut x = 12_345u64;
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(n);
    while out.len() < n {
        x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        let hi = (0x3FFu64 << 52) | (((x >> 20) & ((1 << 28) - 1)) << 24);
        let want = ((hi >> 32) & M24).wrapping_mul(inv) & M24;
        let bits = hi | (want ^ ((hi >> 29) & M24));
        let h = ((bits ^ (bits >> 29)).wrapping_mul(K)) ^ (bits >> 32);
        assert_eq!(h & M24, 0);
        if seen.insert(bits) {
            out.push(f64::from_bits(bits));
        }
    }
    out
}

/// The repro: 160,000 crafted numbers and a text (which sends any length to the hash) held an
/// instance 3.5 s in `stats.mode`; now as long as any 160,000 numbers take, and `stats.multimode`
/// too (FEEDBACK75.md, Q-1092; branch feedback75).
#[test]
fn crafted_numbers_do_not_slow_stats_mode() {
    let d = dir("mode");
    let floats = colliding_floats(160_000);
    let list: Vec<String> = floats.iter().map(|f| format!("{:?}", f)).collect();
    std::fs::write(d.join("hf.json"), format!("[{},\"x\"]", list.join(","))).unwrap();
    let mut it = quillo::Interp::new(&d);
    it.load_source("let l = json.parse(read_file(\"hf.json\"))\n", "t.qwe").unwrap();
    let t = thread_cpu();
    it.load_source("let m = stats.mode(l)\nlet mm = len(stats.multimode(l))\n", "u.qwe").unwrap();
    let took = thread_cpu() - t;
    assert_eq!(quillo::value::lookup(&it.globals, "m").unwrap().repr(), format!("{:?}", floats[0]));
    assert_eq!(quillo::value::lookup(&it.globals, "mm").unwrap().repr(), "160001");
    assert!(took < Duration::from_millis(if cfg!(debug_assertions) { 10_000 } else { 1_000 }), "{:?}", took);
    let _ = std::fs::remove_dir_all(&d);
}

/// What `stats.mode` and `stats.multimode` give is as before: the first most common value, 1 and
/// 1.0 one value, 0.0 and -0.0 one, texts in other scripts, one item, lists, and no mode of
/// nothing (FEEDBACK75.md, Q-1092; branch feedback75).
#[test]
fn stats_mode_gives_what_it_gave() {
    let d = dir("mode-same");
    assert_eq!(run(&d, r#"stats.mode([1, 2, 2, "x"])"#), "2");
    assert_eq!(run(&d, r#"stats.mode([1.0, 1, 2])"#), "1.0");
    assert_eq!(run(&d, r#"stats.mode([0.0, -0.0, 1])"#), "0.0");
    assert_eq!(run(&d, r#"stats.mode(["é", "é", 1, "x"])"#), r#""é""#);
    assert_eq!(run(&d, r#"stats.mode([7])"#), "7");
    assert_eq!(run(&d, r#"stats.mode([3, 1, 3, 1, 2])"#), "3");
    assert_eq!(run(&d, r#"stats.multimode([1, 2, 2, 1, "x", 3.5])"#), "[1, 2]");
    assert_eq!(run(&d, r#"stats.multimode([])"#), "[]");
    assert_eq!(run(&d, r#"stats.multimode([1, 1.0, 2, 2.0, "日本", "日本", [1], [1]])"#), r#"[1, 2, "日本", [1]]"#);
    let mut it = quillo::Interp::new(&d);
    let e = match it.load_source("let m = stats.mode([])\n", "t.qwe") {
        Ok(_) => String::new(),
        Err(e) => e.to_string(),
    };
    assert!(e.contains("no mode for empty data"), "{}", e);
    let _ = std::fs::remove_dir_all(&d);
}

// ---- Servers -----------------------------------------------------------------------------------

/// A `quillo run` this test started, on the port the system picked, its stderr in a file; killed
/// when dropped.
struct Run {
    child: std::process::Child,
    port: u16,
    log: PathBuf,
}

impl Drop for Run {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Run {
    fn start(d: &Path, src: &str, args: &[&str]) -> Run {
        std::fs::write(d.join("app.qwe"), src).unwrap();
        let log = d.join("app.log");
        let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
            .args(["run", "app.qwe", "--port", "0"])
            .args(args)
            .current_dir(d)
            .env("QUILLO_SECRET", "a secret for the feedback75d tests, long enough")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(&log).unwrap())
            .spawn()
            .unwrap();
        support::stop_with_tests(&child);
        let mut r = Run { child, port: 0, log };
        r.port = support::port_in_log(&mut r.child, &r.log, Duration::from_secs(30));
        r
    }

    fn log(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
}

/// `head` and then `body`, raw, on a connection of its own: the status and the answer's body.
fn raw(port: u16, head: &str, body: &[u8]) -> (u16, String) {
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
    s.write_all(head.as_bytes()).unwrap();
    let _ = s.write_all(body);
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let out = String::from_utf8_lossy(&out).into_owned();
    let (h, b) = out.split_once("\r\n\r\n").unwrap_or((&out, ""));
    let status = h.lines().next().and_then(|l| l.split(' ').nth(1)).and_then(|c| c.parse().ok()).unwrap_or(0);
    (status, b.to_string())
}

/// A POST of `body` as gzip says, to `path`.
fn post_gzip(port: u16, path: &str, body: &[u8]) -> (u16, String) {
    let head = format!("POST {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/octet-stream\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\n\r\n", path, body.len());
    raw(port, &head, body)
}

/// gzip's format of `data`, `pad` bytes of extra field in its header (FEXTRA), which change its
/// size and nothing it decodes to.
fn gzip_padded(data: &[u8], pad: usize) -> Vec<u8> {
    let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
    e.write_all(data).unwrap();
    let deflated = e.finish().unwrap();
    let mut out = vec![0x1f, 0x8b, 8, if pad > 0 { 4 } else { 0 }, 0, 0, 0, 0, 0, 0xff];
    if pad > 0 {
        let xlen = pad - 2;
        out.extend_from_slice(&(xlen as u16).to_le_bytes());
        out.extend(std::iter::repeat_n(b'p', xlen));
    }
    out.extend_from_slice(&deflated);
    let mut crc = flate2::Crc::new();
    crc.update(data);
    out.extend_from_slice(&crc.sum().to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out
}

/// gzip's format of `data`, padded to be `size` bytes in all.
fn gzip_of_size(data: &[u8], size: usize) -> Vec<u8> {
    let bare = gzip_padded(data, 0).len();
    assert!(size >= bare + 2, "{} bytes compress to {}", data.len(), bare);
    let out = gzip_padded(data, size - bare);
    assert_eq!(out.len(), size);
    out
}

// ---- Q-1090: a gzip body's ratio ---------------------------------------------------------------

const LEN_APP: &str = "route POST /len {\n    return str(len(request.body))\n}\n";

/// The repro: 10 KB of gzip carried 10 MB of zeros to the route, at a thousandth of the cost to
/// send; now a body that decodes to more than 100 times its size and 64 KB is refused (413) and
/// says so, and the server goes on (FEEDBACK75.md, Q-1090; branch feedback75).
#[test]
fn a_gzip_body_bomb_is_refused() {
    let d = dir("gzip-body");
    let srv = Run::start(&d, LEN_APP, &[]);
    let bomb = gzip_padded(&vec![0u8; 10 * 1024 * 1024 - 16], 0);
    assert!(bomb.len() < 20_000, "{}", bomb.len());
    let (status, body) = post_gzip(srv.port, "/len", &bomb);
    assert_eq!(status, 413, "{}", body);
    // The line is written after the answer: waited for (CI read the log before it came).
    let line = "request body too large for what was sent (once decompressed, more than 100 times its size)";
    let until = Instant::now() + Duration::from_secs(60);
    while !srv.log().contains(line) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
    let log = srv.log();
    assert!(log.contains(line), "{}", log);
    assert_eq!(post_gzip(srv.port, "/len", &gzip_padded(b"still here", 0)), (200, "10".into()));
    let _ = std::fs::remove_dir_all(&d);
}

/// The limit: 10,000 bytes sent may decode to 100 × 10,000 + 65,536 bytes, and not one more; an
/// empty body, one character, text in other scripts and real JSON (about 5 times smaller
/// compressed) are read as before, and so is a body that is not gzip-encoded; a body that is not
/// gzip is still a 400 (FEEDBACK75.md, Q-1090; branch feedback75).
#[test]
fn a_gzip_body_decodes_to_a_hundred_times_its_size() {
    let d = dir("gzip-ratio");
    let srv = Run::start(&d, LEN_APP, &[]);
    let at = 100 * 10_000 + 65_536;
    assert_eq!(post_gzip(srv.port, "/len", &gzip_of_size(&vec![0u8; at], 10_000)), (200, at.to_string()));
    assert_eq!(post_gzip(srv.port, "/len", &gzip_of_size(&vec![0u8; at + 1], 10_000)).0, 413);
    assert_eq!(post_gzip(srv.port, "/len", &gzip_padded(b"", 0)), (200, "0".into()));
    assert_eq!(post_gzip(srv.port, "/len", &gzip_padded("é".as_bytes(), 0)), (200, "1".into()));
    let text = "日本語のテキスト, größer, текст. ".repeat(500);
    assert_eq!(post_gzip(srv.port, "/len", &gzip_padded(text.as_bytes(), 0)), (200, text.chars().count().to_string()));
    let json: String = (0..2000).map(|i| format!("{{\"id\":{},\"name\":\"user {}\",\"score\":{}}}", i, i * 7919 % 10007, i * 31 % 97)).collect::<Vec<_>>().join(",");
    let json = format!("[{}]", json);
    let gz = gzip_padded(json.as_bytes(), 0);
    assert!(json.len() > 3 * gz.len() && json.len() < 100 * gz.len(), "{} / {}", json.len(), gz.len());
    assert_eq!(post_gzip(srv.port, "/len", &gz), (200, json.len().to_string()));
    let plain = format!("POST /len HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n", 300_000);
    assert_eq!(raw(srv.port, &plain, &vec![b'z'; 300_000]), (200, "300000".into()));
    assert_eq!(post_gzip(srv.port, "/len", b"not gzip at all").0, 400);
    let _ = std::fs::remove_dir_all(&d);
}

// ---- Q-1091: a body still arriving at a stop ---------------------------------------------------

/// The repro: a body trickled at about 2 KiB/s held a stop for as long as it kept coming (78 s seen,
/// hours possible); now, 30 s after the stop, it is told 503 and its connection closed. A request
/// being answered when the stop came is still waited for, longer than that (the user's graceful
/// stop, 2026-11-09), and a body that arrives within the time is answered (FEEDBACK75.md, Q-1091;
/// branch feedback75).
#[test]
fn a_body_trickled_at_a_stop_is_refused_after_a_while() {
    use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
    use std::sync::Arc;
    let d = dir("stop-body");
    let src = "route POST /up {\n    return str(len(request.body))\n}\nroute GET /slow {\n    sleep(33000)\n    return \"done\"\n}\n";
    let mut srv = Run::start(&d, src, &[]);
    let port = srv.port;
    // A: 10 MB said, 205 bytes every 0.1 s.
    let mut a = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    a.write_all(b"POST /up HTTP/1.1\r\nHost: localhost\r\nContent-Type: text/plain\r\nContent-Length: 10000000\r\n\r\n").unwrap();
    let told = Arc::new(AtomicBool::new(false));
    let mut a_read = a.try_clone().unwrap();
    let told2 = told.clone();
    let reader = std::thread::spawn(move || {
        a_read.set_read_timeout(Some(Duration::from_secs(80))).unwrap();
        let mut first = [0u8; 64];
        let n = a_read.read(&mut first).unwrap_or(0);
        told2.store(true, SeqCst);
        (Instant::now(), String::from_utf8_lossy(&first[..n]).into_owned())
    });
    let told3 = told.clone();
    let trickle = std::thread::spawn(move || {
        let until = Instant::now() + Duration::from_secs(80);
        while !told3.load(SeqCst) && Instant::now() < until {
            if a.write_all(&[b'x'; 205]).is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    });
    // C: a request that takes 33 s, being answered when the stop comes.
    let slow = std::thread::spawn(move || {
        let r = raw(port, "GET /slow HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n", b"");
        (Instant::now(), r)
    });
    // B: half its body before the stop, the rest a second after.
    let mut b = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    b.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
    b.write_all(b"POST /up HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: text/plain\r\nContent-Length: 1000\r\n\r\n").unwrap();
    b.write_all(&[b'y'; 500]).unwrap();
    std::thread::sleep(Duration::from_millis(1000));
    let stopped = Instant::now();
    unsafe { libc::kill(srv.child.id() as i32, libc::SIGTERM) };
    std::thread::sleep(Duration::from_millis(1000));
    b.write_all(&[b'y'; 500]).unwrap();
    let mut out = String::new();
    let _ = b.read_to_string(&mut out);
    assert!(out.starts_with("HTTP/1.1 200") && out.ends_with("1000"), "{}", out);
    // The server ends once the slow request is answered, A refused at 30 s.
    let deadline = stopped + Duration::from_secs(60);
    let ended = loop {
        if let Ok(Some(_)) = srv.child.try_wait() {
            break Instant::now();
        }
        assert!(Instant::now() < deadline, "the server did not stop: {}", srv.log());
        std::thread::sleep(Duration::from_millis(50));
    };
    let (told_at, first) = reader.join().unwrap();
    trickle.join().unwrap();
    let (slow_at, (status, body)) = slow.join().unwrap();
    assert!(first.starts_with("HTTP/1.1 503"), "{:?}\n{}", first, srv.log());
    let after = told_at.duration_since(stopped);
    assert!(after >= Duration::from_secs(29) && after < Duration::from_secs(36), "{:?}", after);
    assert_eq!((status, body.as_str()), (200, "done"));
    assert!(slow_at.duration_since(stopped) >= Duration::from_secs(30), "{:?}", slow_at.duration_since(stopped));
    assert!(ended.duration_since(stopped) < Duration::from_secs(45), "{:?}", ended.duration_since(stopped));
    let log = srv.log();
    assert!(log.contains("POST /up HTTP/1.1 -> 503 (the server is stopping, and the body had not come 30 s after)"), "{}", log);
    let _ = std::fs::remove_dir_all(&d);
}
