//! Work on large lists, spread over the cores: `stats`, `vec` and `math`
//! (src/numeric.rs) and the builtin `sort` hand it chunks of a list.
//!
//! Chunks are always `CHUNK` items, however many cores there are, and
//! results come back in their order, so an answer (a compensated sum, say)
//! is the same on every machine. Short lists run in the caller alone. The
//! threads are made once and wait between jobs; while one job runs, another
//! caller does its work alone rather than wait for the pool.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};

/// Items in a chunk.
pub const CHUNK: usize = 1 << 15;

/// Lists shorter than this are not worth waking the pool for.
const PAR: usize = 1 << 17;

struct Job {
    work: *const (dyn Fn(usize) + Sync),
    next: *const AtomicUsize,
    chunks: usize,
}

// The pointers outlive every use: `run` waits until no thread has the job.
unsafe impl Send for Job {}

#[derive(Default)]
struct State {
    job: Option<Job>,
    generation: u64,
    /// Pool threads inside the current job.
    active: usize,
}

struct Pool {
    state: Mutex<State>,
    wake: Condvar,
    done: Condvar,
    busy: AtomicBool,
    threads: usize,
}

fn pool() -> &'static Pool {
    static POOL: OnceLock<&'static Pool> = OnceLock::new();
    POOL.get_or_init(|| {
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).clamp(1, 32) - 1;
        let p: &'static Pool = Box::leak(Box::new(Pool {
            state: Mutex::new(State::default()),
            wake: Condvar::new(),
            done: Condvar::new(),
            busy: AtomicBool::new(false),
            threads,
        }));
        for i in 0..threads {
            let _ = std::thread::Builder::new().name(format!("quillo-par-{}", i)).spawn(move || worker(p));
        }
        p
    })
}

fn worker(p: &'static Pool) {
    // What these threads allocate is freed on others: mimalloc's (src/alloc.rs).
    crate::alloc::waiter_thread();
    let mut seen = 0;
    let mut st = p.state.lock().unwrap();
    loop {
        // A job this thread has not been in yet (one it slept through is gone).
        while st.job.is_none() || st.generation == seen {
            st = p.wake.wait(st).unwrap();
        }
        seen = st.generation;
        let job = st.job.as_ref().unwrap();
        let (work, next, chunks) = (job.work, job.next, job.chunks);
        st.active += 1;
        drop(st);
        // SAFETY: `run` keeps both alive until `active` is back to 0.
        let (work, next) = unsafe { (&*work, &*next) };
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| claim(work, next, chunks)));
        st = p.state.lock().unwrap();
        st.active -= 1;
        if st.active == 0 {
            p.done.notify_all();
        }
    }
}

fn claim(work: &(dyn Fn(usize) + Sync), next: &AtomicUsize, chunks: usize) {
    loop {
        let i = next.fetch_add(1, Ordering::Relaxed);
        if i >= chunks {
            return;
        }
        work(i);
    }
}

/// Calls `work(i)` for every chunk i in 0..chunks, on the pool's threads
/// and this one, and returns when all are done.
fn run(chunks: usize, items: usize, work: &(dyn Fn(usize) + Sync)) {
    if chunks <= 1 || items < PAR {
        (0..chunks).for_each(work);
        return;
    }
    let p = pool();
    if p.threads == 0 || p.busy.swap(true, Ordering::Acquire) {
        (0..chunks).for_each(work);
        return;
    }
    let next = AtomicUsize::new(0);
    {
        let mut st = p.state.lock().unwrap();
        // SAFETY (lifetimes): the job is taken away below, after every thread left it.
        let work: &'static (dyn Fn(usize) + Sync) = unsafe { std::mem::transmute(work) };
        st.job = Some(Job { work, next: &next, chunks });
        st.generation += 1;
        p.wake.notify_all();
    }
    // Even if this thread's share panics, the others must be out of the job
    // (which lives on this stack) before it unwinds.
    let mine = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| claim(work, &next, chunks)));
    let mut st = p.state.lock().unwrap();
    while st.active > 0 {
        st = p.done.wait(st).unwrap();
    }
    st.job = None;
    drop(st);
    p.busy.store(false, Ordering::Release);
    if let Err(e) = mine {
        std::panic::resume_unwind(e);
    }
}

fn chunks_of(n: usize) -> usize {
    n.div_ceil(CHUNK)
}

fn range(i: usize, n: usize) -> std::ops::Range<usize> {
    i * CHUNK..((i + 1) * CHUNK).min(n)
}

struct Slots<T>(*mut T);
unsafe impl<T: Send> Sync for Slots<T> {}
unsafe impl<T: Send> Send for Slots<T> {}

impl<T> Slots<T> {
    /// SAFETY: each index is written by one chunk only.
    unsafe fn at(&self, i: usize) -> *mut T {
        self.0.add(i)
    }
}

/// `f` of each chunk's range of 0..n, in order.
pub fn map<T: Send>(n: usize, f: impl Fn(std::ops::Range<usize>) -> T + Sync) -> Vec<T> {
    let chunks = chunks_of(n);
    let mut out: Vec<Option<T>> = (0..chunks).map(|_| None).collect();
    let slots = Slots(out.as_mut_ptr());
    run(chunks, n, &|i| unsafe { *slots.at(i) = Some(f(range(i, n))) });
    out.into_iter().map(|x| x.expect("a chunk did not run")).collect()
}

/// Fills `out` chunk by chunk: `f(start, part)` writes items start.. of it.
pub fn fill<T: Send>(out: &mut [T], f: impl Fn(usize, &mut [T]) + Sync) {
    let n = out.len();
    let slots = Slots(out.as_mut_ptr());
    run(chunks_of(n), n, &|i| {
        let r = range(i, n);
        // SAFETY: chunks do not overlap.
        let part = unsafe { std::slice::from_raw_parts_mut(slots.at(r.start), r.len()) };
        f(r.start, part)
    });
}

/// Calls `f(i)` for each i in 0..n, a task each (for tasks of unequal size).
pub fn each(n: usize, items: usize, f: impl Fn(usize) + Sync) {
    run(n, items, &f);
}

#[cfg(test)]
mod tests {
    #[test]
    fn many_callers_at_once_get_their_own_answers() {
        let handles: Vec<_> = (0..8u64)
            .map(|t| {
                std::thread::spawn(move || {
                    for round in 0..20u64 {
                        let n = 200_000 + (t * 7919 + round) as usize;
                        let parts = super::map(n, |r| r.map(|i| i as u64 * (t + 1)).sum::<u64>());
                        let want = (0..n as u64).sum::<u64>() * (t + 1);
                        assert_eq!(parts.into_iter().sum::<u64>(), want);
                        let mut v = vec![0u64; n];
                        super::fill(&mut v, |start, part| {
                            for (k, x) in part.iter_mut().enumerate() {
                                *x = (start + k) as u64 + t;
                            }
                        });
                        assert!(v.iter().enumerate().all(|(i, x)| *x == i as u64 + t));
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
    }

    #[test]
    fn a_panic_in_a_job_reaches_the_caller() {
        let r = std::panic::catch_unwind(|| super::map(1 << 20, |r| if r.start == 0 { panic!("boom") } else { 1 }));
        assert!(r.is_err());
        // And the pool still works afterwards.
        assert_eq!(super::map(1 << 20, |r| r.len()).into_iter().sum::<usize>(), 1 << 20);
    }
}
