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
use std::time::UNIX_EPOCH;

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
    m.lock().unwrap_or_else(|e| e.into_inner())
}

static FILES: Mutex<Option<Lru<(PathBuf, u64, u128)>>> = Mutex::new(None);
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
    mtime: u128,
}

pub fn info(full: &Path) -> std::io::Result<Info> {
    let m = std::fs::metadata(full)?;
    if !m.is_file() {
        return Err(std::io::ErrorKind::NotFound.into());
    }
    let mtime = m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
    // Weak: the same file gzipped or not is the same content.
    let path_hash = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        full.hash(&mut h);
        h.finish()
    };
    let etag = format!("W/\"{:x}-{:x}-{:x}\"", m.len(), mtime, path_hash & 0xffff_ffff);
    let secs = (mtime / 1_000_000_000) as u64;
    Ok(Info { len: m.len(), etag, last_modified: crate::httpd::http_date(secs), mtime })
}

/// The whole file, from memory when it is small and unchanged.
pub fn read(full: &Path, info: &Info) -> std::io::Result<Arc<[u8]>> {
    let key = (full.to_path_buf(), info.len, info.mtime);
    if info.len <= SMALL {
        if let Some(b) = get(&mut lock(&FILES), FILES_BUDGET, &key) {
            return Ok(b);
        }
    }
    let b: Arc<[u8]> = Arc::from(std::fs::read(full)?);
    if info.len <= SMALL {
        put(&mut lock(&FILES), FILES_BUDGET, key, b.clone());
    }
    Ok(b)
}

/// Bytes `start..=end` of the file.
pub fn read_range(full: &Path, start: u64, end: u64) -> std::io::Result<Vec<u8>> {
    let mut f = std::fs::File::open(full)?;
    f.seek(SeekFrom::Start(start))?;
    let mut out = vec![0; (end - start + 1) as usize];
    f.read_exact(&mut out)?;
    Ok(out)
}

/// A single `Range: bytes=...` of a file `len` long: `Some(Ok((start,
/// end)))`, `Some(Err(()))` when it cannot be served (416), or `None` to
/// send the whole file (no range, several ranges, or not bytes).
pub fn range(header: &str, len: u64) -> Option<Result<(u64, u64), ()>> {
    let spec = header.trim().strip_prefix("bytes=")?;
    if spec.contains(',') || len == 0 {
        return None;
    }
    let (a, b) = spec.split_once('-')?;
    let (a, b) = (a.trim(), b.trim());
    let r = if a.is_empty() {
        // The last N bytes.
        let n: u64 = b.parse().ok()?;
        if n == 0 {
            return Some(Err(()));
        }
        (len.saturating_sub(n), len - 1)
    } else {
        let start: u64 = a.parse().ok()?;
        let end = if b.is_empty() { len - 1 } else { b.parse::<u64>().ok()?.min(len - 1) };
        if start > end || start >= len {
            return Some(Err(()));
        }
        (start, end)
    };
    Some(Ok(r))
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
}
