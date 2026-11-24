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
#[derive(Clone)]
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

/// Whether the file opened, `m`, is another than the one looked at (`id`, `etag`): saved anew in
/// its place. By identity; where the system gives none (Windows), by its tag, its length and
/// times, so a file written to in place is taken for another there (FEEDBACK55.md, Q-701; branch
/// feedback55).
pub fn replaced(id: Option<FileId>, etag: &str, full: &Path, m: &std::fs::Metadata) -> bool {
    match (id, id_of(m)) {
        (Some(was), Some(now)) => was != now,
        _ => info_of(full, m).etag != etag,
    }
}

/// Whether a file `m`, read from `now` (the time before the read), changed so lately that a write
/// in the same tick of its clock would not be seen (ctime is kept in ticks of about 4 ms on Linux
/// before 6.13, 10 ms on a kernel of 100 Hz, whole seconds on some file systems): no copy of it is
/// kept, as git does not trust such a file. By ctime on unix, which the kernel sets from its own
/// clock; elsewhere by mtime, which a program may set, as an archive unpacked or `rsync -t` from
/// a clock ahead does (FEEDBACK55.md, Q-701; branch feedback55).
pub fn racy(m: &std::fs::Metadata, now: std::time::SystemTime) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        UNIX_EPOCH.checked_add(Duration::new(m.ctime().max(0) as u64, m.ctime_nsec().clamp(0, 999_999_999) as u32)).is_none_or(|t| racy_time(t, now))
    }
    #[cfg(not(unix))]
    {
        m.modified().map_or(true, |t| racy_time(t, now))
    }
}

/// Whether a change at `t` is too near `now` to trust a copy read then: within 50 ms of it, or two
/// seconds where the time has whole seconds only; one ahead of now by more than that is the
/// clock's, not a change during the read (FEEDBACK55.md, Q-701; branch feedback55).
pub fn racy_time(t: std::time::SystemTime, now: std::time::SystemTime) -> bool {
    let coarse = t.duration_since(UNIX_EPOCH).is_ok_and(|d| d.subsec_nanos() == 0);
    let window = if coarse { Duration::from_secs(2) } else { Duration::from_millis(50) };
    match now.duration_since(t) {
        Ok(ago) => ago < window,
        Err(ahead) => ahead.duration() < window,
    }
}

/// At most so many files are read whole into memory at once off the interpreter's lock, each
/// holding as much as the file, up to a MiB (256 at once held 1.4 GB); waited for without a thread,
/// so the threads that wait for databases and fetches are not taken by reads waiting their turn.
/// Reads on the request's own thread take none (FEEDBACK55.md, Q-701; branch feedback55).
pub fn read_places() -> &'static tokio::sync::Semaphore {
    static PLACES: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();
    PLACES.get_or_init(|| tokio::sync::Semaphore::new(std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(2, 8)))
}

/// Memory for a file of `len` bytes, read into by `read_into`: made on the thread that asks, the
/// request's, whose memory is used again, not on a thread that reads for it off the lock and
/// keeps what is freed elsewhere (1 MiB copies made there held 550 MB) (FEEDBACK55.md, Q-701;
/// branch feedback55).
pub fn buffer(len: u64) -> Arc<[u8]> {
    // SAFETY: zeroed bytes are bytes.
    unsafe { Arc::<[u8]>::new_zeroed_slice(len as usize).assume_init() }
}

/// The first `len` bytes of the file, read into memory of that size made once: no second copy
/// (FEEDBACK55.md, Q-701; branch feedback55). Fewer if the file has fewer now.
pub fn read_whole(f: &std::fs::File, len: u64) -> std::io::Result<Arc<[u8]>> {
    read_into(f, buffer(len))
}

/// `read_whole`, into memory made before (`buffer`) (FEEDBACK55.md, Q-701; branch feedback55).
pub fn read_into(f: &std::fs::File, mut a: Arc<[u8]>) -> std::io::Result<Arc<[u8]>> {
    let buf = Arc::get_mut(&mut a).ok_or(std::io::ErrorKind::Other)?;
    let mut f = f;
    f.seek(SeekFrom::Start(0))?;
    let mut got = 0;
    while got < buf.len() {
        match f.read(&mut buf[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    if got < buf.len() {
        return Ok(Arc::from(&buf[..got]));
    }
    Ok(a)
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
        // A file unlinked since it was opened is named `... (deleted)`: no name to judge it by
        // (FEEDBACK55.md, Q-701; branch feedback55).
        std::fs::read_link(format!("/proc/self/fd/{}", f.as_raw_fd())).ok().filter(|p| !p.to_string_lossy().ends_with(" (deleted)"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "android")))]
    {
        let _ = f;
        None
    }
}

/// The folder `dir` as the system names the files in it (`path_of` of it opened), so a file's
/// place and the folder's are compared in one form: realpath keeps `/System/Volumes/Data/...`
/// where the system names a file `/private/tmp/...`. Kept by the folder's identity, so asking
/// again costs a look at it, and a link to it pointed elsewhere (a release switched) is resolved
/// anew (FEEDBACK55.md, Q-701; branch feedback55).
pub fn real_dir(dir: &Path) -> Option<PathBuf> {
    static DIRS: Mutex<Option<HashMap<PathBuf, (FileId, PathBuf)>>> = Mutex::new(None);
    let id = std::fs::metadata(dir).ok().and_then(|m| id_of(&m));
    if let Some(id) = id {
        if let Some((was, form)) = lock(&DIRS).as_ref().and_then(|m| m.get(dir)) {
            if *was == id {
                return Some(form.clone());
            }
        }
    }
    // A folder that cannot be opened (no read permission) is resolved, in the same form
    // (FEEDBACK85.md, Q-1340, found by the coverage tests; branch feedback82-85).
    let form = std::fs::File::open(dir).ok().and_then(|f| path_of(&f)).or_else(|| dir.canonicalize().ok().map(sys_form))?;
    if let Some(id) = id {
        let mut dirs = lock(&DIRS);
        let m = dirs.get_or_insert_with(HashMap::new);
        if m.len() >= 1024 {
            m.clear();
        }
        m.insert(dir.to_path_buf(), (id, form.clone()));
    }
    Some(form)
}

// ---- one folder, however it is named ------------------------------------------------------------

/// Where macOS mounts the data volume, whose folders the system volume shows through firmlinks.
#[cfg(target_os = "macos")]
const DATA: &str = "/System/Volumes/Data";

/// The firmlinks of macOS 15 and 26, as /usr/share/firmlinks lists them: for a process that may
/// not read that file (a sandbox), each still taken only where a look finds it one folder.
#[cfg(target_os = "macos")]
const KNOWN_FIRMLINKS: &str = "/AppleInternal\tAppleInternal\n/Applications\tApplications\n/Library\tLibrary\n/System/Library/Caches\tSystem/Library/Caches\n\
/System/Library/Assets\tSystem/Library/Assets\n/System/Library/PreinstalledAssets\tSystem/Library/PreinstalledAssets\n/System/Library/AssetsV2\tSystem/Library/AssetsV2\n\
/System/Library/PreinstalledAssetsV2\tSystem/Library/PreinstalledAssetsV2\n/System/Library/CoreServices/CoreTypes.bundle/Contents/Library\tSystem/Library/CoreServices/CoreTypes.bundle/Contents/Library\n\
/System/Library/Speech\tSystem/Library/Speech\n/Users\tUsers\n/Volumes\tVolumes\n/cores\tcores\n/opt\topt\n/pkg\tpkg\n/private\tprivate\n/usr/local\tusr/local\n\
/usr/libexec/cups\tusr/libexec/cups\n/usr/share/snmp\tusr/share/snmp\n";

/// The data volume's folders that have a second name on macOS, as (long, short):
/// `/System/Volumes/Data/private` is `/private`, `/System/Volumes/Data/Users` is `/Users` (the
/// system's list, /usr/share/firmlinks), each taken only once a look finds the two one folder (its
/// device and inode, no link on either side). Longest first. Read once. Where the list cannot be
/// read (a sandbox), the known one is looked at in its place: with none, a folder's two names
/// were two folders, which refused a path an `--allow-path` by the other name allows (closed),
/// but let a pinned build into a temporary folder under `--allow-path /System/Volumes/Data`, a
/// root only the long name names (open) (FEEDBACK82.md, Q-1285, found by the review of 3e793e3;
/// branch feedback82-85). Where no look finds them one folder either, the names are not the
/// system's, and stay as they are.
#[cfg(target_os = "macos")]
fn firmlinks() -> &'static [(PathBuf, PathBuf)] {
    static LINKS: std::sync::OnceLock<Vec<(PathBuf, PathBuf)>> = std::sync::OnceLock::new();
    LINKS.get_or_init(|| {
        let text = std::fs::read_to_string("/usr/share/firmlinks").unwrap_or_else(|_| KNOWN_FIRMLINKS.to_string());
        let dir = |p: &Path| std::fs::symlink_metadata(p).ok().filter(|m| m.file_type().is_dir()).and_then(|m| id_of(&m));
        let mut out: Vec<(PathBuf, PathBuf)> = text
            .lines()
            .filter_map(|l| l.split_once('\t'))
            .filter(|(short, rel)| short.starts_with('/') && !rel.is_empty() && !rel.starts_with('/'))
            .map(|(short, rel)| (Path::new(DATA).join(rel.trim_end()), PathBuf::from(short)))
            .filter(|(long, short)| dir(long).is_some() && dir(long) == dir(short))
            .collect();
        out.sort_by_key(|(long, _)| std::cmp::Reverse(long.as_os_str().len()));
        out
    })
}

/// `p`, a path resolved (`canonicalize`), in the form the system names files in, which `path_of`
/// and the temporary folder give: on macOS a folder of the data volume has two names,
/// `/System/Volumes/Data/private/tmp/x` and `/private/tmp/x`, and realpath keeps the one it was
/// given, so one folder compared by its two names was two (FEEDBACK82.md Q-1285, FEEDBACK85.md
/// Q-1340, FEEDBACK83.md Q-1300, found by the coverage tests; branch feedback82-85). Anything else
/// is as it is: a path that is not in the long form costs a look at its first bytes.
pub fn sys_form(p: PathBuf) -> PathBuf {
    match sys_form_of(&p) {
        std::borrow::Cow::Owned(o) => o,
        std::borrow::Cow::Borrowed(_) => p,
    }
}

/// `sys_form`, borrowed where it is already in that form. On macOS `/.nofollow/x` is `/x` too: the
/// system resolves a path under it from `/` with no link followed, and realpath keeps the prefix,
/// so for a path it resolved (no link left in it) it names the same file, which `path_of` names
/// without it; a program started as `/.nofollow/.../main.qwe`, or given `--allow-path
/// /.nofollow/...`, was refused every path, and its migrations as "not the one judged"
/// (FEEDBACK85.md Q-1340 and FEEDBACK83.md Q-1300, found by the review of 3e793e3; branch
/// feedback82-85). `/.nofollow` itself is a folder of its own, and stays.
pub fn sys_form_of(p: &Path) -> std::borrow::Cow<'_, Path> {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        let b = p.as_os_str().as_bytes();
        if b.starts_with(b"/.nofollow/") {
            let plain = Path::new(std::ffi::OsStr::from_bytes(&b["/.nofollow".len()..]));
            return std::borrow::Cow::Owned(sys_form_of(plain).into_owned());
        }
        if b.starts_with(b"/System/Volumes/Data/") {
            if let Some((long, short)) = firmlinks().iter().find(|(long, _)| p.starts_with(long)) {
                let rest = p.strip_prefix(long).unwrap_or(Path::new(""));
                return std::borrow::Cow::Owned(if rest.as_os_str().is_empty() { short.clone() } else { short.join(rest) });
            }
        }
    }
    std::borrow::Cow::Borrowed(p)
}

/// Whether `p` is the folder `root` or in it, both paths resolved, by whichever name each was given
/// (`sys_form`): `/System/Volumes/Data/private/x` is in `/private`, and so is `/private/x/a`
/// in `/System/Volumes/Data/private/x`. A root only the long form names (the data volume's own
/// root `/System/Volumes/Data`, or `/System/Volumes/Data/usr`, which holds `/usr/local`) holds
/// what its firmlinks show (FEEDBACK85.md, Q-1340, found by the coverage tests; branch
/// feedback82-85). Where both are named alike, what `starts_with` said.
pub fn within(p: &Path, root: &Path) -> bool {
    rel_in(p, root).is_some()
}

/// What follows the folder `root` in `p`, both resolved, as `within` reads them: None where `p` is
/// not in it. `strip_prefix`, by whichever name each was given (FEEDBACK85.md, Q-1340, found by the
/// coverage tests; branch feedback82-85).
pub fn rel_in<'a>(p: &'a Path, root: &Path) -> Option<std::borrow::Cow<'a, Path>> {
    if let Ok(rest) = p.strip_prefix(root) {
        return Some(std::borrow::Cow::Borrowed(rest));
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        let (sp, sr) = (sys_form_of(p), sys_form_of(root));
        if !matches!((&sp, &sr), (std::borrow::Cow::Borrowed(_), std::borrow::Cow::Borrowed(_))) {
            if let Ok(rest) = sp.strip_prefix(&*sr) {
                return Some(std::borrow::Cow::Owned(rest.to_path_buf()));
            }
        }
        // A root of the data volume that no short name shows: what a firmlink in it shows is in it.
        if sr.as_os_str().as_bytes().starts_with(b"/System") {
            for (long, short) in firmlinks() {
                if let (Ok(above), Ok(rest)) = (long.strip_prefix(&*sr), sp.strip_prefix(short)) {
                    return Some(std::borrow::Cow::Owned(if rest.as_os_str().is_empty() { above.to_path_buf() } else { above.join(rest) }));
                }
            }
        }
    }
    None
}

/// `p`, resolved, by its long name on macOS where it has one (`/System/Volumes/Data/private/x`
/// for `/private/x`): the name under which the most folders hold it, the data volume's root among
/// them. As it is elsewhere.
pub fn long_form(p: &Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        let p = sys_form_of(p);
        if let Some((long, short)) = firmlinks().iter().find(|(_, short)| p.starts_with(short)) {
            let rest = p.strip_prefix(short).unwrap_or(Path::new(""));
            return if rest.as_os_str().is_empty() { long.clone() } else { long.join(rest) };
        }
        p.into_owned()
    }
    #[cfg(not(target_os = "macos"))]
    p.to_path_buf()
}

/// Whether the folder `dir`, resolved, is `root` or inside it, by the folders themselves: by their
/// names in one form (`within`), and by identity, each folder above `dir` (by its long name, which
/// has the most above it) looked at and compared with `root` by its device and inode, so a folder
/// with yet another name (a bind mount on Linux) is the one it is. A look at each folder above
/// `dir`: for what must stay out of the program's reach, once, never on a path a request takes
/// (FEEDBACK82.md, Q-1285, found by the coverage tests; branch feedback82-85).
pub fn holds(root: &Path, dir: &Path) -> bool {
    holds_any(root, [dir.to_path_buf()])
}

/// Whether the folder `root` holds any of `paths` (each resolved), as `holds` judges one: `root`
/// looked at once, and each folder above them once, however many share it. For a folder the
/// program would delete whole, whatever name it is given: `delete_file("/System/Volumes/Data")`
/// or `"/.nofollow/..."`, which no name of the program's files began with (FEEDBACK85.md, Q-1340,
/// found by the review of 3e793e3; branch feedback82-85).
pub fn holds_any(root: &Path, paths: impl IntoIterator<Item = PathBuf>) -> bool {
    let mut want = None;
    let mut seen = std::collections::HashSet::new();
    for p in paths {
        if within(&p, root) {
            return true;
        }
        if want.is_none() {
            want = std::fs::metadata(root).ok().and_then(|m| id_of(&m));
            if want.is_none() {
                return false;
            }
        }
        for chain in [long_form(&p), p] {
            for a in chain.ancestors() {
                // Looked at already, and so every folder above it.
                if !seen.insert(a.to_path_buf()) {
                    break;
                }
                if std::fs::metadata(a).ok().and_then(|m| id_of(&m)) == want {
                    return true;
                }
            }
        }
    }
    false
}

// ---- what Quillo keeps beside a program: `.quillo` ----------------------------------------------

/// Whether a part of `p` is named `.quillo` as the file system compares names: in any ASCII case
/// on macOS and Windows (where `.quillo.` and `.quillo ` are it too), exactly elsewhere. Read by its
/// bytes, with no allocation: a write asks it of each path it judges (FEEDBACK85.md, Q-1340, found
/// by the review of 9da2632; branch feedback82-85).
pub fn quillo_part(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        p.as_os_str().as_bytes().split(|&b| b == b'/').any(|part| part.len() == 7 && if cfg!(target_os = "macos") { part.eq_ignore_ascii_case(b".quillo") } else { part == b".quillo" })
    }
    #[cfg(not(unix))]
    {
        p.components().any(|c| {
            let n = c.as_os_str().to_string_lossy();
            n.trim_end_matches(['.', ' ']).eq_ignore_ascii_case(".quillo")
        })
    }
}

/// Why `rel` under the folder `root` cannot be read as Quillo's own, or the file: `root` as it
/// leads (a `.quillo` that is a link is followed), each part below it opened without following a
/// link (`openat` with `O_NOFOLLOW`), and the file a file of bytes with one name (`st_nlink` 1),
/// opened without waiting. A link or a second hard link planted in `.quillo` gives nothing then,
/// though the program could write where it leads (FEEDBACK85.md, Q-1340, found by the review of
/// 9da2632; branch feedback82-85).
pub enum Below {
    /// Not there.
    Missing,
    /// There, and not Quillo's to read: why.
    Refused(String),
}

/// See `Below`.
pub fn open_below(root: &Path, rel: &Path) -> Result<std::fs::File, Below> {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        use std::os::fd::FromRawFd;
        use std::os::unix::ffi::OsStrExt;
        let shown = || root.join(rel).display().to_string();
        let fail = |e: std::io::Error, part: &Path| match e.raw_os_error() {
            Some(libc::ENOENT) => Below::Missing,
            Some(libc::ELOOP) | Some(libc::ENOTDIR) => Below::Refused(format!("{} is a link in {}, which Quillo does not follow there", part.display(), root.display())),
            _ => Below::Refused(format!("{}: {}", shown(), e)),
        };
        let c = |p: &std::ffi::OsStr| CString::new(p.as_bytes()).map_err(|_| Below::Refused(format!("{}: a NUL in its name", shown())));
        // SAFETY: a NUL-ended path; the descriptor, if any, is closed below or owned by the File.
        let mut fd = unsafe { libc::open(c(root.as_os_str())?.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC) };
        if fd < 0 {
            return Err(fail(std::io::Error::last_os_error(), root));
        }
        let parts: Vec<&std::ffi::OsStr> = rel.iter().collect();
        let mut at = PathBuf::new();
        for (i, part) in parts.iter().enumerate() {
            at.push(part);
            let last = i + 1 == parts.len();
            let flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | if last { libc::O_NONBLOCK } else { libc::O_DIRECTORY };
            let name = match c(part) {
                Ok(n) => n,
                Err(e) => {
                    // SAFETY: the descriptor opened above, closed once.
                    unsafe { libc::close(fd) };
                    return Err(e);
                }
            };
            // SAFETY: a descriptor of a folder open here and a NUL-ended name.
            let next = unsafe { libc::openat(fd, name.as_ptr(), flags) };
            let e = std::io::Error::last_os_error();
            // SAFETY: as above.
            unsafe { libc::close(fd) };
            if next < 0 {
                return Err(fail(e, &at));
            }
            fd = next;
        }
        // SAFETY: the descriptor opened last, owned by the File from here.
        let f = unsafe { std::fs::File::from_raw_fd(fd) };
        let m = f.metadata().map_err(|e| Below::Refused(format!("{}: {}", shown(), e)))?;
        if !m.is_file() {
            return Err(Below::Refused(format!("{} is no file of bytes", shown())));
        }
        if std::os::unix::fs::MetadataExt::nlink(&m) > 1 {
            return Err(Below::Refused(format!("{} has another name too (a hard link), which Quillo does not read there", shown())));
        }
        Ok(f)
    }
    #[cfg(not(unix))]
    {
        std::fs::File::open(root.join(rel)).map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { Below::Missing } else { Below::Refused(e.to_string()) })
    }
}

/// The whole of `rel` under `root`, read as `open_below` opens it.
pub fn read_below(root: &Path, rel: &Path) -> Result<Vec<u8>, Below> {
    let mut f = open_below(root, rel)?;
    let mut out = Vec::new();
    std::io::Read::read_to_end(&mut f, &mut out).map_err(|e| Below::Refused(format!("{}: {}", root.join(rel).display(), e)))?;
    Ok(out)
}

/// Where a path Quillo keeps in a `.quillo` folder (`a/.quillo/native/lib.dylib`) is read from:
/// the `.quillo` folder (its last part of that name) and what follows it, for `open_below`. None
/// for a path with no such part.
pub fn quillo_split(p: &Path) -> Option<(PathBuf, PathBuf)> {
    let parts: Vec<std::path::Component> = p.components().collect();
    let i = parts.iter().rposition(|c| c.as_os_str() == ".quillo")?;
    let root: PathBuf = parts[..=i].iter().collect();
    let rel: PathBuf = parts[i + 1..].iter().collect();
    (!rel.as_os_str().is_empty()).then_some((root, rel))
}

/// Up to `len` bytes of the file from `start`, read where they are: never more, whatever the file
/// has grown to (FEEDBACK55.md, Q-701; branch feedback55).
/// Whether the first bytes of `f` are still `seen`'s, its place in the file kept: a file written
/// in place while it was read can give them torn (a database's header begun, what was there
/// after it), which no check of the whole header sees, so the start of what is sent is read
/// twice alike (2026-11-09; branch feedback56, found by CI's
/// a_database_header_written_in_place_is_never_sent).
pub(crate) fn steady_head(f: &std::fs::File, seen: &[u8]) -> bool {
    let mut g = f;
    let Ok(at) = g.stream_position() else { return false };
    let k = seen.len().min(16);
    let same = read_at(f, 0, k as u64).is_ok_and(|again| again[..] == seen[..k]);
    g.seek(SeekFrom::Start(at)).is_ok() && same
}

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
    let now = std::time::SystemTime::now();
    let (f, m) = open_regular(full)?;
    // Saved anew (another file in its place): looked at again. Written to in place: what is
    // there, as many bytes as were looked at, kept only as the file was, and not changed so lately
    // that a write could hide in its clock's tick (FEEDBACK55.md, Q-701; branch feedback55).
    if replaced(info.id, &info.etag, full, &m) {
        return Err(changed());
    }
    let b = if info.len <= SMALL { read_whole(&f, info.len)? } else { Arc::from(read_at(&f, 0, info.len)?) };
    if info.len <= SMALL && b.len() as u64 == info.len && f.metadata().is_ok_and(|m| same(info, full, &m) && !racy(&m, now)) {
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

/// The large files `files` judged, by path, length, time and identity, so a 304 or a HEAD of one
/// not changed since costs a look at it (FEEDBACK55.md, Q-701; branch feedback55).
static JUDGED_LARGE: Mutex<Option<std::collections::HashSet<(PathBuf, u64, u128)>>> = Mutex::new(None);

pub fn judged_large(full: &Path, info: &Info) -> bool {
    lock(&JUDGED_LARGE).as_ref().is_some_and(|s| s.contains(&(full.to_path_buf(), info.len, info.mtime)))
}

pub fn keep_judged_large(full: &Path, info: &Info) {
    let mut g = lock(&JUDGED_LARGE);
    let s = g.get_or_insert_with(Default::default);
    if s.len() >= 4096 {
        s.clear();
    }
    s.insert((full.to_path_buf(), info.len, info.mtime));
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
    pub id: Option<FileId>,
}

/// The mark for bytes `start..start + len` of a file, for `files` with what it judged
/// (FEEDBACK55.md, Q-701; branch feedback55).
pub fn mark(full: &Path, start: u64, len: u64, hosted: Option<&Hosted>) -> String {
    let b64 = |p: &Path| crate::crypto::b64url(p.to_string_lossy().as_bytes());
    let id = |h: &Hosted| h.id.map_or(String::new(), |(d, i)| format!("{}-{}", d, i));
    let h = hosted.map_or(String::new(), |h| format!("{},{},{},{}", b64(&h.served), b64(&h.program), crate::crypto::b64url(h.etag.as_bytes()), id(h)));
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
            let (served, program, etag) = (PathBuf::from(text(h.next()?)?), PathBuf::from(text(h.next()?)?), text(h.next()?)?);
            let id = match h.next()? {
                "" => None,
                id => Some(id.split_once('-').and_then(|(d, i)| Some((d.parse().ok()?, i.parse().ok()?)))?),
            };
            Some(Hosted { served, program, etag, id })
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

/// Large files being sent from disk now, on threads of their own: a stop waits for them, as for
/// any request in progress (the user's graceful stop, 2026-11-09; branch feedback56, found in
/// review).
pub fn sending() -> usize {
    lock(&SENDING).as_ref().map_or(0, |m| m.values().sum())
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
    // For `files`, the file opened is judged as it is. One written to in place since (the same
    // file) goes as it was marked, as many bytes as were looked at; one saved in the place of the
    // one judged, if it is served, goes whole with its own length and tags, a part asked for (a
    // 206) as the whole (a 200), any other answer (the page of a miss) as it was
    // (FEEDBACK55.md, Q-701; branch feedback55).
    // One saved anew as it was opened is opened again, a moment later each time (on macOS an
    // open right after a rename can find the file it replaced); one replaced each time is a 503
    // to come back to, as `file` gives (FEEDBACK55.md, Q-701; branch feedback55).
    let busy = |stream: &mut std::net::TcpStream, out: &mut Vec<u8>| {
        let mut r = crate::server::plain_response(503, "the file is being replaced; try again\n");
        r.headers.push(("retry-after".into(), "1".into()));
        crate::httpd::write_response(out, &r, head, true);
        let _ = stream.write_all(out);
    };
    let opened = match &hosted {
        Some(h) => {
            let mut got = Err(crate::builtins::Judged::Replaced);
            for pause in [0, 2, 8] {
                std::thread::sleep(Duration::from_millis(pause));
                got = crate::builtins::open_hosted(&h.served, &h.program, &path);
                if got.as_ref().err() != Some(&crate::builtins::Judged::Replaced) {
                    break;
                }
            }
            match got {
                Ok((f, m)) => Ok((f, Some(m))),
                Err(crate::builtins::Judged::Replaced) => return busy(&mut stream, &mut out),
                Err(_) => Err(()),
            }
        }
        None => open_regular(&path).map(|(f, _)| (f, None)).map_err(|_| ()),
    };
    let mut resp = std::borrow::Cow::Borrowed(resp);
    let (mut start, mut len) = (start, len);
    if let (Ok((_, Some(m))), Some(h)) = (&opened, &hosted) {
        if replaced(h.id, &h.etag, &path, m) {
            let now = info_of(&path, m);
            let r = resp.to_mut();
            if r.status == 206 {
                r.status = 200;
            }
            r.headers.retain(|(k, _)| k != "content-range");
            for (k, v) in r.headers.iter_mut() {
                match k.as_str() {
                    "etag" => *v = now.etag.clone(),
                    "last-modified" => *v = now.last_modified.clone(),
                    _ => {}
                }
            }
            (start, len) = (0, now.len);
        }
    }
    let not_found = |stream: &mut std::net::TcpStream, out: &mut Vec<u8>| {
        crate::httpd::write_response(out, &crate::server::plain_response(404, "not found\n"), head, true);
        let _ = stream.write_all(out);
    };
    // A HEAD, or a part from further in, by the file's own first bytes (FEEDBACK55.md, Q-702;
    // branch feedback55).
    if let (Ok((f, _)), Some(_)) = (&opened, &hosted) {
        if (head || start > 0) && crate::builtins::sqlite_head(f) {
            return not_found(&mut stream, &mut out);
        }
    }
    let file = opened.and_then(|(mut f, _)| f.seek(SeekFrom::Start(start)).map(|_| f).map_err(|_| ()));
    let Ok(mut f) = file else { return not_found(&mut stream, &mut out) };
    let mut buf = vec![0u8; 64 << 10];
    // For `files`, the first bytes sent are judged as they are read, before the head goes, in one
    // read of 16 at least, though fewer are sent: those of a SQLite database are not sent; a part
    // from further in by the file's own first bytes, above (FEEDBACK55.md, Q-702; branch
    // feedback55).
    let mut first = 0;
    if hosted.is_some() && !head && start == 0 {
        while first < 16 {
            match f.read(&mut buf[first..]) {
                Ok(0) => break,
                Ok(n) => first += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        if crate::db::sqlite_bytes(&buf[..first]) {
            return not_found(&mut stream, &mut out);
        }
        // Begun as a database's header could be, and changed as it was read: come back (see
        // `steady_head`).
        if crate::db::sqlite_lead(&buf[..first]) && !steady_head(&f, &buf[..first]) {
            return busy(&mut stream, &mut out);
        }
    }
    // In u64: a length of 4 GiB or more is no `usize` on 32 bits (FEEDBACK55.md, Q-701; branch
    // feedback55).
    let first = if first as u64 > len { len as usize } else { first };
    crate::httpd::write_head(&mut out, &resp, len, true);
    if stream.write_all(&out).is_err() || head {
        return;
    }
    if first > 0 && stream.write_all(&buf[..first]).is_err() {
        return;
    }
    let mut f = f.take(len - first as u64);
    let began = Instant::now();
    let mut sent = first as u64;
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
