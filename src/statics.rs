//! Files served with `file()`. Small ones are kept in memory while they are
//! unchanged on disk; each answer carries validators (ETag, Last-Modified)
//! so a browser can ask whether its copy is current and get a bodiless 304;
//! a byte range (for audio and video) is read on its own, not the whole
//! file; and the gzipped copy of a text file is kept too, instead of being
//! compressed again for every request.

use indexmap::IndexMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::time::{Duration, Instant, UNIX_EPOCH};

/// Files up to this size are kept in memory.
const SMALL: u64 = 1 << 20;
/// Memory for kept files, and for gzipped copies.
const FILES_BUDGET: usize = 64 << 20;
const GZIP_BUDGET: usize = 32 << 20;

/// A cache of byte strings, least recently used dropped first past a budget.
struct Lru<K> {
    map: IndexMap<K, Arc<[u8]>>,
    bytes: usize,
    budget: usize,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Under `--watch`, the files the program has served with `file()`, and the
/// path each was asked for at: a change to one refreshes the pages, and a
/// stylesheet is swapped in them as they are (FEEDBACK11.md, Q-078; branch
/// feedback11-fixes).
pub static WATCH_SERVED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static SERVED: Mutex<Option<HashMap<PathBuf, String>>> = Mutex::new(None);

/// Notes that `file` went out at `path`, while `--watch` looks.
pub fn served(file: &Path, path: &str) {
    if !WATCH_SERVED.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let mut s = SERVED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let m = s.get_or_insert_with(HashMap::new);
    if m.len() < 10_000 {
        m.insert(file.to_path_buf(), path.to_string());
    }
}

/// The files served so far, with the path each was asked for at.
pub fn served_files() -> Vec<(PathBuf, String)> {
    SERVED.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref().map_or(Vec::new(), |m| m.iter().map(|(f, p)| (f.clone(), p.clone())).collect())
}

/// By path, length, time and identity, and whether `files` judged it: what `files` reads from here
/// is a copy of the file it judged, and none that `file` read (FEEDBACK55.md, Q-701; branch
/// feedback55).
static FILES: Mutex<Option<Lru<(PathBuf, u64, u128, bool)>>> = Mutex::new(None);
static GZIP: Mutex<Option<Lru<(String, u64)>>> = Mutex::new(None);

fn get<K: std::hash::Hash + Eq>(c: &mut Option<Lru<K>>, budget: usize, k: &K) -> Option<Arc<[u8]>> {
    let c = c.get_or_insert_with(|| Lru { map: IndexMap::new(), bytes: 0, budget });
    let i = c.map.get_index_of(k)?;
    let last = c.map.len() - 1;
    c.map.move_index(i, last);
    Some(c.map[last].clone())
}

fn put<K: std::hash::Hash + Eq>(c: &mut Option<Lru<K>>, budget: usize, k: K, v: Arc<[u8]>) {
    let c = c.get_or_insert_with(|| Lru { map: IndexMap::new(), bytes: 0, budget });
    c.bytes += v.len();
    if let Some(old) = c.map.insert(k, v) {
        c.bytes -= old.len();
    }
    while c.bytes > c.budget && c.map.len() > 1 {
        if let Some((_, gone)) = c.map.shift_remove_index(0) {
            c.bytes -= gone.len();
        }
    }
}

/// What a file is now: its size, when it changed, and its validators.
pub struct Info {
    pub len: u64,
    pub etag: String,
    pub last_modified: String,
    /// `last_modified` as seconds since 1970.
    pub modified: i64,
    mtime: u128,
    /// Which file it is, and how many names it has: one with more may be another name of one of
    /// the program's own files (FEEDBACK55.md, Q-701; branch feedback55).
    pub id: Option<FileId>,
    pub links: u64,
    /// When it was made, where the system says: a number used again is another file's
    /// (FEEDBACK55.md, Q-701; branch feedback55).
    pub born: Option<std::time::SystemTime>,
}

/// A file as the system tells it from others, by its device and inode, whatever its name or
/// the links to it: what `files` holds against the program's own files (FEEDBACK55.md, Q-701
/// and Q-702; branch feedback55).
pub type FileId = (u64, u64);

/// The file a look at it found (FEEDBACK55.md, Q-701; branch feedback55). None on Windows:
/// stable Rust gives no file index there, so it has only the checks by name and by content.
pub fn id_of(m: &std::fs::Metadata) -> Option<FileId> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some((m.dev(), m.ino()))
    }
    #[cfg(not(unix))]
    {
        let _ = m;
        None
    }
}

pub fn info(full: &Path) -> std::io::Result<Info> {
    let m = std::fs::metadata(full)?;
    if !m.is_file() {
        return Err(std::io::ErrorKind::NotFound.into());
    }
    Ok(info_of(full, &m))
}

/// `info`, of what a look at `full` found, or a file opened there (FEEDBACK55.md, Q-701; branch
/// feedback55).
pub fn info_of(full: &Path, m: &std::fs::Metadata) -> Info {
    let mtime = m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
    // Weak: the same file gzipped or not is the same content. Its inode and change time are in
    // it too, which a rename over it or a write keeps from staying the same, though the size
    // and the modified time can (`cp -p`, `touch -t`, a build that sets every time to one): the
    // memory's copy and the browser's are then seen to be old (FEEDBACK31.md, Q-392; branch
    // feedback27-35-fixes).
    let path_hash = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        full.hash(&mut h);
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            (m.ino(), m.dev(), m.ctime(), m.ctime_nsec()).hash(&mut h);
        }
        h.finish()
    };
    let etag = format!("W/\"{:x}-{:x}-{:x}\"", m.len(), mtime, path_hash & 0xffff_ffff);
    let secs = (mtime / 1_000_000_000) as u64;
    // The key of the copy in memory: the time, and the file's identity above it.
    let key = (mtime & u64::MAX as u128) | ((path_hash as u128) << 64);
    #[cfg(unix)]
    let links = std::os::unix::fs::MetadataExt::nlink(m);
    #[cfg(not(unix))]
    let links = 1;
    Info { len: m.len(), etag, last_modified: crate::httpd::http_date(secs), modified: secs as i64, mtime: key, id: id_of(m), links, born: m.created().ok() }
}

/// Whether the file opened, `m`, is the file `info` is of, as it was then (FEEDBACK55.md, Q-701;
/// branch feedback55).
pub fn same(info: &Info, full: &Path, m: &std::fs::Metadata) -> bool {
    m.len() == info.len && id_of(m) == info.id && info_of(full, m).mtime == info.mtime
}

/// The error of a file that is not what it was when it was looked at: looked at again
/// (FEEDBACK55.md, Q-701; branch feedback55).
pub fn changed() -> std::io::Error {
    std::io::Error::other("the file changed since it was looked at")
}

pub fn is_changed(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::Other && e.to_string() == "the file changed since it was looked at"
}

/// `full` opened as a file of bytes: one that is not (a FIFO, a device, a folder, through a link
/// swapped in) is not found, and opening it does not wait (FEEDBACK55.md, Q-701; branch
/// feedback55).
pub fn open_regular(full: &Path) -> std::io::Result<(std::fs::File, std::fs::Metadata)> {
    let mut o = std::fs::OpenOptions::new();
    o.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_NONBLOCK);
    }
    let f = o.open(full)?;
    let m = f.metadata()?;
    if !m.is_file() {
        return Err(std::io::ErrorKind::NotFound.into());
    }
    Ok((f, m))
}

/// Where the file opened is, as the system has it, whatever links led there: what `files`
/// judges, so that no link swapped in after the look takes it elsewhere (FEEDBACK55.md, Q-701;
/// branch feedback55). None where the system does not say (then the path is resolved again).
pub fn path_of(f: &std::fs::File) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::io::AsRawFd;
        let mut buf = vec![0u8; libc::PATH_MAX as usize + 1];
        // SAFETY: F_GETPATH writes at most PATH_MAX bytes and a NUL into the buffer given.
        if unsafe { libc::fcntl(f.as_raw_fd(), libc::F_GETPATH, buf.as_mut_ptr()) } == -1 {
            return None;
        }
        let n = buf.iter().position(|&b| b == 0)?;
        Some(PathBuf::from(std::ffi::OsStr::from_bytes(&buf[..n])))
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        use std::os::unix::io::AsRawFd;
        std::fs::read_link(format!("/proc/self/fd/{}", f.as_raw_fd())).ok()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "android")))]
    {
        let _ = f;
        None
    }
}

/// Up to `len` bytes of the file from `start`, read where they are: never more, whatever the file
/// has grown to (FEEDBACK55.md, Q-701; branch feedback55).
pub fn read_at(f: &std::fs::File, start: u64, len: u64) -> std::io::Result<Vec<u8>> {
    let mut f = f;
    f.seek(SeekFrom::Start(start))?;
    let mut out = Vec::with_capacity(len.min(SMALL) as usize);
    f.take(len).read_to_end(&mut out)?;
    Ok(out)
}

/// The whole file, from memory when it is small and unchanged; one that is not what `info`
/// says any more (renamed over, written since) is `changed()`, and is read no further than its
/// length was (FEEDBACK55.md, Q-701; branch feedback55).
pub fn read(full: &Path, info: &Info) -> std::io::Result<Arc<[u8]>> {
    let key = (full.to_path_buf(), info.len, info.mtime, false);
    if info.len <= SMALL {
        if let Some(b) = get(&mut lock(&FILES), FILES_BUDGET, &key) {
            return Ok(b);
        }
    }
    let (f, m) = open_regular(full)?;
    if !same(info, full, &m) {
        return Err(changed());
    }
    let b = read_at(&f, 0, info.len)?;
    if b.len() as u64 != info.len {
        return Err(changed());
    }
    let b: Arc<[u8]> = Arc::from(b);
    if info.len <= SMALL {
        put(&mut lock(&FILES), FILES_BUDGET, key, b.clone());
    }
    Ok(b)
}

/// Bytes `start..=end` of the file, read from a file of bytes (FEEDBACK55.md, Q-701; branch
/// feedback55).
pub fn read_range(full: &Path, start: u64, end: u64) -> std::io::Result<Vec<u8>> {
    let (f, _) = open_regular(full)?;
    let out = read_at(&f, start, end - start + 1)?;
    if out.len() as u64 != end - start + 1 {
        return Err(std::io::ErrorKind::UnexpectedEof.into());
    }
    Ok(out)
}

/// A small file's copy `files` judged, if memory has it (FEEDBACK55.md, Q-701; branch feedback55).
pub fn judged_copy(full: &Path, info: &Info) -> Option<Arc<[u8]>> {
    get(&mut lock(&FILES), FILES_BUDGET, &(full.to_path_buf(), info.len, info.mtime, true))
}

/// Keeps a small file's copy `files` judged, under what the file opened was (FEEDBACK55.md,
/// Q-701; branch feedback55).
pub fn keep_judged(full: &Path, info: &Info, b: Arc<[u8]>) {
    if info.len <= SMALL {
        put(&mut lock(&FILES), FILES_BUDGET, (full.to_path_buf(), info.len, info.mtime, true), b);
    }
}

/// Larger files go out from disk a piece at a time, on a thread of their
/// own, so they take no more memory than a buffer (see `Worker::answer`).
/// `file()` marks the response with what to send, signed with a key only
/// this process has, so that a program's own headers cannot name a file.
pub const SEND_HEADER: &str = "x-quillo-send";

fn send_key() -> &'static [u8] {
    static KEY: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    KEY.get_or_init(|| crate::crypto::secret_bytes(32))
}

/// Whether the file is small enough to be kept in memory.
pub fn small(info: &Info) -> bool {
    info.len <= SMALL
}

/// What `files` sends a large file for: the folder it serves, the program's, and the file's tag
/// as it judged it, so the file opened to send is judged again, and one put in its place since is
/// sent with its own head (FEEDBACK55.md, Q-701; branch feedback55).
#[derive(Clone, Debug, PartialEq)]
pub struct Hosted {
    pub served: PathBuf,
    pub program: PathBuf,
    pub etag: String,
}

/// The mark for bytes `start..start + len` of a file, for `files` with what it judged
/// (FEEDBACK55.md, Q-701; branch feedback55).
pub fn mark(full: &Path, start: u64, len: u64, hosted: Option<&Hosted>) -> String {
    let b64 = |p: &Path| crate::crypto::b64url(p.to_string_lossy().as_bytes());
    let h = hosted.map_or(String::new(), |h| format!("{},{},{}", b64(&h.served), b64(&h.program), crate::crypto::b64url(h.etag.as_bytes())));
    let what = format!("{}:{}:{}:{}", start, len, h, full.display());
    let sig = crate::crypto::hmac_sha256(send_key(), what.as_bytes());
    format!("{}.{}", crate::crypto::b64url(what.as_bytes()), crate::crypto::b64url(&sig))
}

/// What a mark names, if this process made it.
pub fn marked(v: &str) -> Option<(PathBuf, u64, u64, Option<Hosted>)> {
    let (what, sig) = v.split_once('.')?;
    let what = crate::crypto::unb64url(what)?;
    if !crate::crypto::same(&crate::crypto::hmac_sha256(send_key(), &what), &crate::crypto::unb64url(sig)?) {
        return None;
    }
    let what = String::from_utf8(what).ok()?;
    let mut parts = what.splitn(4, ':');
    let start = parts.next()?.parse().ok()?;
    let len = parts.next()?.parse().ok()?;
    let text = |s: &str| crate::crypto::unb64url(s).and_then(|b| String::from_utf8(b).ok());
    let hosted = match parts.next()? {
        "" => None,
        h => {
            let mut h = h.split(',');
            Some(Hosted { served: PathBuf::from(text(h.next()?)?), program: PathBuf::from(text(h.next()?)?), etag: text(h.next()?)? })
        }
    };
    Some((PathBuf::from(parts.next()?), start, len, hosted))
}

/// Large files being sent at once, in all and to one address: each holds a
/// thread and a connection for as long as its reader takes.
const MOST_SENDS: usize = 256;
const MOST_SENDS_PER_IP: usize = 8;

static SENDING: Mutex<Option<HashMap<String, usize>>> = Mutex::new(None);

/// Who a limit per address counts a client as: its IPv4 address, or the
/// /64 network of its IPv6 one, since one client is given a whole /64 and
/// can take a new address in it for each connection.
pub(crate) fn client_of(ip: &str) -> String {
    match ip.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V6(a)) => match a.to_ipv4_mapped() {
            Some(v4) => v4.to_string(),
            None => {
                let s = a.segments();
                format!("{:x}:{:x}:{:x}:{:x}::/64", s[0], s[1], s[2], s[3])
            }
        },
        Ok(a) => a.to_string(),
        Err(_) => ip.to_string(),
    }
}

/// A place among the large-file sends, given back when dropped.
pub struct SendPlace(String);

impl SendPlace {
    pub fn take(ip: &str) -> Option<SendPlace> {
        let who = client_of(ip);
        let mut g = lock(&SENDING);
        let m = g.get_or_insert_with(HashMap::new);
        let total: usize = m.values().sum();
        let mine = m.get(&who).copied().unwrap_or(0);
        if total >= MOST_SENDS || mine >= MOST_SENDS_PER_IP {
            return None;
        }
        m.insert(who.clone(), mine + 1);
        Some(SendPlace(who))
    }
}

impl Drop for SendPlace {
    fn drop(&mut self) {
        if let Some(m) = lock(&SENDING).as_mut() {
            if let Some(n) = m.get_mut(&self.0) {
                *n -= 1;
                if *n == 0 {
                    m.remove(&self.0);
                }
            }
        }
    }
}

/// A reader slower than this (after a grace period) is let go, so that
/// readers taking a byte now and then cannot hold the places forever.
const SLOWEST: u64 = 16 << 10;
const GRACE: Duration = Duration::from_secs(20);
/// Nor can one just fast enough hold a place for a whole large file: a
/// send stops after this long (a browser or player goes on with a range).
const LONGEST: Duration = Duration::from_secs(3600);

/// Whether a send that has taken `took` and sent `sent` bytes goes on.
fn keeps_place(took: Duration, sent: u64) -> bool {
    took <= LONGEST && (took <= GRACE || sent >= took.as_secs() * SLOWEST)
}

/// Answers on a connection of its own: `resp`'s head, then `len` bytes of
/// the file from `start` (none for `HEAD`), and closes it.
pub fn send(mut stream: std::net::TcpStream, resp: &crate::server::HttpResponse, (path, start, len, hosted): (PathBuf, u64, u64, Option<Hosted>), head: bool) {
    use std::io::Write;
    // A reader that stopped reading lets the thread go.
    let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(30)));
    let mut out = Vec::with_capacity(512);
    // For `files`, the file opened is judged as it is, and one saved in the place of the one
    // judged, if it is served, goes whole with its own head: the head made for the other said
    // another length (FEEDBACK55.md, Q-701; branch feedback55).
    let opened = match &hosted {
        Some(h) => crate::builtins::open_hosted(&h.served, &h.program, &path).map(|(f, m)| (f, Some(m))).ok_or(()),
        None => open_regular(&path).map(|(f, _)| (f, None)).map_err(|_| ()),
    };
    let mut resp = std::borrow::Cow::Borrowed(resp);
    let (mut start, mut len) = (start, len);
    if let (Ok((_, Some(m))), Some(h)) = (&opened, &hosted) {
        let now = info_of(&path, m);
        if now.etag != h.etag {
            let r = resp.to_mut();
            r.status = 200;
            r.headers.retain(|(k, _)| !["etag", "last-modified", "content-range"].contains(&k.as_str()));
            r.headers.push(("etag".into(), now.etag.clone()));
            r.headers.push(("last-modified".into(), now.last_modified.clone()));
            (start, len) = (0, now.len);
        }
    }
    let file = opened.and_then(|(mut f, _)| f.seek(SeekFrom::Start(start)).map(|_| f).map_err(|_| ()));
    let Ok(f) = file else {
        crate::httpd::write_response(&mut out, &crate::server::plain_response(404, "not found\n"), head, true);
        let _ = stream.write_all(&out);
        return;
    };
    crate::httpd::write_head(&mut out, &resp, len, true);
    if stream.write_all(&out).is_err() || head {
        return;
    }
    let mut f = f.take(len);
    let mut buf = vec![0u8; 64 << 10];
    let began = Instant::now();
    let mut sent = 0u64;
    loop {
        let n = match f.read(&mut buf) {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        let mut at = 0;
        while at < n {
            match stream.write(&buf[at..n]) {
                Ok(0) | Err(_) => return,
                Ok(w) => {
                    at += w;
                    sent += w as u64;
                }
            }
            if !keeps_place(began.elapsed(), sent) {
                return;
            }
        }
    }
}

/// A single `Range: bytes=...` of a file `len` long: `Some(Ok((start,
/// end)))`, `Some(Err(()))` when it cannot be served (416), or `None` to
/// send the whole file (no range, several ranges, or not bytes).
pub fn range(header: &str, len: u64) -> Option<Result<(u64, u64), ()>> {
    // The unit in any case, and positions of digits only, however many: `Bytes=0-1` was let go,
    // `bytes=+2-4` taken, and an end past 2^64 sent the whole file as a 200, where it is the
    // file's end (RFC 9110 §14.1; FEEDBACK55.md, Q-723; branch feedback55).
    let header = header.trim();
    let spec = header.get(6..).filter(|_| header[..6].eq_ignore_ascii_case("bytes="))?;
    if spec.contains(',') || len == 0 {
        return None;
    }
    let (a, b) = spec.split_once('-')?;
    let (a, b) = (a.trim(), b.trim());
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit());
    let pos = |s: &str| s.parse::<u64>().unwrap_or(u64::MAX);
    if !(digits(a) || a.is_empty()) || !(digits(b) || b.is_empty()) {
        return None;
    }
    let r = if a.is_empty() {
        // The last N bytes.
        if b.is_empty() {
            return None;
        }
        let n = pos(b);
        if n == 0 {
            return Some(Err(()));
        }
        (len.saturating_sub(n), len - 1)
    } else {
        let start = pos(a);
        let end = if b.is_empty() { len - 1 } else { pos(b).min(len - 1) };
        if start > end || start >= len {
            return Some(Err(()));
        }
        (start, end)
    };
    Some(Ok(r))
}

/// Whether `If-Range`'s tag is the file's `etag`, with or without `W/` on either (FEEDBACK55.md,
/// Q-723; branch feedback55).
pub fn same_tag(tag: &str, etag: &str) -> bool {
    let bare = |t: &str| t.trim().strip_prefix("W/").unwrap_or(t.trim()).to_string();
    bare(tag) == bare(etag)
}

/// Whether bytes this many are sent from memory, not from disk on a thread of their own (a range
/// of a large file: FEEDBACK55.md, Q-725; branch feedback55).
pub fn small_len(n: u64) -> bool {
    n <= SMALL
}

/// Whether an `If-None-Match` header names `etag` (weakly) or `*`.
pub fn matches(if_none_match: &str, etag: &str) -> bool {
    let bare = |t: &str| t.trim().trim_start_matches("W/").to_string();
    let want = bare(etag);
    if_none_match.split(',').any(|t| t.trim() == "*" || bare(t) == want)
}

/// The gzipped copy of `body`, if one was kept for it under this ETag.
/// The body is part of the key too (hashed with a per-process secret): a
/// program may well give different bodies the same ETag.
pub fn gzipped(etag: &str, body: &[u8]) -> Option<Arc<[u8]>> {
    get(&mut lock(&GZIP), GZIP_BUDGET, &(etag.to_string(), body_hash(body)))
}

pub fn keep_gzipped(etag: &str, body: &[u8], gz: Arc<[u8]>) {
    if gz.len() as u64 <= SMALL {
        put(&mut lock(&GZIP), GZIP_BUDGET, (etag.to_string(), body_hash(body)), gz);
    }
}

fn body_hash(body: &[u8]) -> u64 {
    use std::hash::BuildHasher;
    static SEED: std::sync::OnceLock<std::collections::hash_map::RandomState> = std::sync::OnceLock::new();
    SEED.get_or_init(Default::default).hash_one(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        assert_eq!(range("bytes=0-9", 100), Some(Ok((0, 9))));
        assert_eq!(range("bytes=90-", 100), Some(Ok((90, 99))));
        assert_eq!(range("bytes=-10", 100), Some(Ok((90, 99))));
        assert_eq!(range("bytes=50-500", 100), Some(Ok((50, 99))));
        assert_eq!(range("bytes=100-", 100), Some(Err(())));
        assert_eq!(range("bytes=0-1,5-6", 100), None);
        assert_eq!(range("items=0-1", 100), None);
        assert!(matches("W/\"a\", \"b\"", "W/\"b\"") && matches("*", "W/\"x\"") && !matches("\"c\"", "W/\"b\""));
    }

    #[test]
    fn large_file_places_count_an_ipv6_network_as_one_client() {
        // What the review saw: each address of a /64 counted on its own.
        let mut held = Vec::new();
        for i in 0..MOST_SENDS_PER_IP {
            held.push(SendPlace::take(&format!("2001:db8:1:2::{:x}", i + 1)).expect("a place"));
        }
        assert!(SendPlace::take("2001:db8:1:2:ffff::9").is_none());
        assert!(SendPlace::take("2001:db8:1:3::1").is_some());
        assert_eq!(client_of("::ffff:10.0.0.1"), "10.0.0.1");
        assert_eq!(client_of("10.0.0.1"), "10.0.0.1");
        drop(held);
        assert!(SendPlace::take("2001:db8:1:2::1").is_some());
    }

    #[test]
    fn a_send_ends_when_too_slow_or_too_long() {
        assert!(keeps_place(Duration::from_secs(5), 0));
        assert!(!keeps_place(Duration::from_secs(60), 60 * SLOWEST - 1));
        assert!(keeps_place(Duration::from_secs(60), 60 * SLOWEST));
        // A reader just fast enough is still let go in the end.
        let long = LONGEST + Duration::from_secs(1);
        assert!(!keeps_place(long, long.as_secs() * SLOWEST * 2));
    }
}
