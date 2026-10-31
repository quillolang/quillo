//! Numbers for data: `math` (Python's math module), `stats` (its
//! statistics module, and the summaries a data frame gives) and `vec`
//! (numpy's operations on lists of numbers).
//!
//! ```text
//! math.sqrt(2)  math.log(8, 2)  math.comb(10, 3)  math.fsum(xs)  math.pi
//! stats.mean(xs)  stats.median(xs)  stats.stdev(xs)  stats.quantiles(xs, 4)
//! stats.correlation(xs, ys)  stats.linear_regression(xs, ys)  stats.describe(xs)
//! vec.add(xs, 1)  vec.mul(xs, ys)  vec.dot(xs, ys)  vec.cumsum(xs)  vec.argsort(xs)
//! ```
//!
//! A list is read once into f64s, and the work is done on those: sums are
//! compensated (as accurate as Python's, without its fractions), and a
//! median is a selection, not a sort. What Python raises (a math domain
//! error, statistics of no data) is an error here too.

use crate::interp::{Ctrl, Interp};
use crate::value::*;
use crate::par;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::atomic::{AtomicBool, AtomicUsize};

type R = Result<Value, Ctrl>;

// The C library's, which Rust's own float functions use too.
mod c {
    extern "C" {
        pub fn erf(x: f64) -> f64;
        pub fn erfc(x: f64) -> f64;
        pub fn tgamma(x: f64) -> f64;
        #[cfg(unix)]
        pub fn lgamma_r(x: f64, sign: *mut i32) -> f64;
        #[cfg(not(unix))]
        pub fn lgamma(x: f64) -> f64;
        pub fn remainder(x: f64, y: f64) -> f64;
        pub fn frexp(x: f64, e: *mut i32) -> f64;
        pub fn ldexp(x: f64, e: i32) -> f64;
    }
}

fn erf(x: f64) -> f64 {
    unsafe { c::erf(x) }
}

fn erfc(x: f64) -> f64 {
    unsafe { c::erfc(x) }
}

fn gamma(x: f64) -> f64 {
    unsafe { c::tgamma(x) }
}

fn lgamma(x: f64) -> f64 {
    #[cfg(unix)]
    {
        let mut sign = 0;
        unsafe { c::lgamma_r(x, &mut sign) }
    }
    #[cfg(not(unix))]
    unsafe {
        c::lgamma(x)
    }
}

pub fn install_math(m: &mut Map) {
    install(m, "math.", MATH);
    for (k, v) in [
        ("pi", std::f64::consts::PI),
        ("e", std::f64::consts::E),
        ("tau", std::f64::consts::TAU),
        ("inf", f64::INFINITY),
        ("nan", f64::NAN),
    ] {
        m.insert(k.into(), Value::Float(v));
    }
}

pub fn install_bit(m: &mut Map) {
    install(m, "bit.", BIT);
}

pub fn install_stats(m: &mut Map) {
    install(m, "stats.", STATS);
}

pub fn install_vec(m: &mut Map) {
    install(m, "vec.", VEC);
}

fn install(m: &mut Map, prefix: &str, fns: &[(&'static str, BuiltinFn)]) {
    for (name, f) in fns {
        m.insert(name.trim_start_matches(prefix).into(), Value::Builtin(name, *f));
    }
}

// ---- arguments ---------------------------------------------------------

/// An error; one about a decimal says what to do with it. These functions
/// work on floats, which a decimal becomes only when asked.
fn fail<T>(line: u32, msg: impl Into<String>) -> Result<T, Ctrl> {
    let msg = msg.into();
    if msg.ends_with(" dec") || msg.contains(" dec and ") {
        return crate::interp::fail(line, format!("{}{}", msg, crate::interp::FLOAT_ONLY));
    }
    crate::interp::fail(line, msg)
}

fn tn(v: Option<&Value>) -> &'static str {
    v.map_or("nothing", |v| v.type_name())
}

fn given(args: &[Value], i: usize) -> Option<&Value> {
    args.get(i).filter(|v| !matches!(v, Value::Nil))
}

fn num(args: &[Value], i: usize, f: &str, line: u32) -> Result<f64, Ctrl> {
    match args.get(i).and_then(Value::as_f64) {
        Some(x) => Ok(x),
        None => fail(line, format!("`{}` expects a number as argument {}, got {}", f, i + 1, tn(args.get(i)))),
    }
}

fn num_or(args: &[Value], i: usize, f: &str, line: u32, default: f64) -> Result<f64, Ctrl> {
    if given(args, i).is_none() {
        return Ok(default);
    }
    num(args, i, f, line)
}

fn int(args: &[Value], i: usize, f: &str, line: u32) -> Result<i64, Ctrl> {
    match args.get(i) {
        Some(Value::Int(n)) => Ok(*n),
        Some(Value::Float(x)) if x.fract() == 0.0 && x.abs() < 9.0e15 => Ok(*x as i64),
        other => fail(line, format!("`{}` expects an int as argument {}, got {}", f, i + 1, tn(other))),
    }
}

fn int_or(args: &[Value], i: usize, f: &str, line: u32, default: i64) -> Result<i64, Ctrl> {
    if given(args, i).is_none() {
        return Ok(default);
    }
    int(args, i, f, line)
}

fn text_or<'a>(args: &'a [Value], i: usize, f: &str, line: u32, default: &'a str) -> Result<&'a str, Ctrl> {
    match given(args, i) {
        None => Ok(default),
        Some(Value::Str(s)) => Ok(s),
        other => fail(line, format!("`{}` expects a str as argument {}, got {}", f, i + 1, tn(other))),
    }
}

fn list<'a>(args: &'a [Value], i: usize, f: &str, line: u32) -> Result<&'a [Value], Ctrl> {
    match args.get(i) {
        Some(Value::List(l)) => Ok(l),
        Some(Value::Nil) => Ok(&[]),
        other => fail(line, format!("`{}` expects a list as argument {}, got {}", f, i + 1, tn(other))),
    }
}

/// A list of numbers, as f64s, and whether all of them were ints. A long
/// list is read on all the cores.
fn nums(args: &[Value], i: usize, f: &str, line: u32) -> Result<(Vec<f64>, bool), Ctrl> {
    let l = list(args, i, f, line)?;
    let mut out = vec![0.0f64; l.len()];
    let floats = AtomicBool::new(false);
    let bad = AtomicUsize::new(usize::MAX);
    par::fill(&mut out, |start, part| {
        let mut seen = false;
        for (k, slot) in part.iter_mut().enumerate() {
            match &l[start + k] {
                Value::Float(x) => {
                    seen = true;
                    *slot = *x
                }
                Value::Int(n) => *slot = *n as f64,
                _ => {
                    bad.fetch_min(start + k, Relaxed);
                    break;
                }
            }
        }
        if seen {
            floats.store(true, Relaxed);
        }
    });
    let k = bad.into_inner();
    if k != usize::MAX {
        return fail(line, format!("`{}` expects a list of numbers, but item {} is {}", f, k, l[k].type_name()));
    }
    Ok((out, !floats.into_inner()))
}

fn xs(args: &[Value], i: usize, f: &str, line: u32) -> Result<Vec<f64>, Ctrl> {
    Ok(nums(args, i, f, line)?.0)
}

fn floats(v: impl IntoIterator<Item = f64>) -> Value {
    Value::list(v.into_iter().map(Value::Float).collect())
}

/// A list of n numbers, g(i) each, made on all the cores when long.
fn pvalues(n: usize, g: impl Fn(usize) -> Value + Sync) -> Value {
    let mut out: Vec<std::mem::MaybeUninit<Value>> = Vec::with_capacity(n);
    // SAFETY: MaybeUninit needs no initializing; `fill` writes every item.
    unsafe { out.set_len(n) };
    par::fill(&mut out, |start, part| {
        for (k, slot) in part.iter_mut().enumerate() {
            slot.write(g(start + k));
        }
    });
    let mut out = std::mem::ManuallyDrop::new(out);
    // SAFETY: all n are written, and MaybeUninit<Value> is laid out as Value.
    Value::list(unsafe { Vec::from_raw_parts(out.as_mut_ptr() as *mut Value, n, out.capacity()) })
}

/// As `pvalues`, where g may fail: the error of the first item that does.
fn ptry(n: usize, g: impl Fn(usize) -> R + Sync) -> R {
    let first: std::sync::Mutex<Option<(usize, Ctrl)>> = std::sync::Mutex::new(None);
    let v = pvalues(n, |i| match g(i) {
        Ok(v) => v,
        Err(e) => {
            let mut f = first.lock().unwrap();
            if f.as_ref().map_or(true, |(j, _)| i < *j) {
                *f = Some((i, e));
            }
            Value::Nil
        }
    });
    match first.into_inner().unwrap() {
        Some((_, e)) => Err(e),
        None => Ok(v),
    }
}

fn pfloats(n: usize, g: impl Fn(usize) -> f64 + Sync) -> Value {
    pvalues(n, |i| Value::Float(g(i)))
}

/// An int when the inputs were ints and it is whole, else a float.
fn back(x: f64, ints: bool) -> Value {
    if ints && x.fract() == 0.0 && x.abs() < 9.0e15 {
        Value::Int(x as i64)
    } else {
        Value::Float(x)
    }
}

fn whole(x: f64) -> Value {
    back(x, true)
}

fn overflow(line: u32) -> Ctrl {
    Ctrl::Error(crate::QuilloError::new("integer overflow", line))
}

// ---- sums --------------------------------------------------------------

/// A sum and what it lost to rounding, kept with Knuth's TwoSum (no
/// branches, so four of them side by side keep a core busy).
#[derive(Clone, Copy, Default)]
struct Acc {
    s: f64,
    c: f64,
}

impl Acc {
    #[inline(always)]
    fn add(&mut self, x: f64) {
        let t = self.s + x;
        let z = t - self.s;
        self.c += (self.s - (t - z)) + (x - z);
        self.s = t;
    }

    /// Adds a * b with the product's own rounding error (Ogita, Rump and
    /// Oishi's Dot2): as if computed in twice the precision.
    #[inline(always)]
    fn add_prod(&mut self, a: f64, b: f64) {
        let p = a * b;
        let e = a.mul_add(b, -p);
        self.add(p);
        self.c += e;
    }

    fn merge(&mut self, o: Acc) {
        self.add(o.s);
        self.c += o.c;
    }

    /// The sum, and what rounding it to a float lost.
    fn get(self) -> (f64, f64) {
        // Past an infinity, s (the plain sum) is the answer.
        if !self.s.is_finite() {
            return (self.s, 0.0);
        }
        let t = self.s + self.c;
        (t, self.c - (t - self.s))
    }
}

/// Merged in a fixed order: the same answer however the work was spread.
fn merged<const K: usize>(parts: Vec<[Acc; K]>) -> [Acc; K] {
    let mut out = [Acc::default(); K];
    for p in parts {
        for k in 0..K {
            out[k].merge(p[k]);
        }
    }
    out
}

/// g of each item, summed in four lanes.
#[inline(always)]
fn sum4(x: &[f64], g: impl Fn(f64) -> f64) -> Acc {
    let mut a = [Acc::default(); 4];
    let mut it = x.chunks_exact(4);
    for c in &mut it {
        a[0].add(g(c[0]));
        a[1].add(g(c[1]));
        a[2].add(g(c[2]));
        a[3].add(g(c[3]));
    }
    for v in it.remainder() {
        a[0].add(g(*v));
    }
    let [mut s, b, c, d] = a;
    s.merge(b);
    s.merge(c);
    s.merge(d);
    s
}

/// The compensated sum of g of each item of a slice, on all the cores when long.
fn psum(x: &[f64], g: impl Fn(f64) -> f64 + Sync) -> (f64, f64) {
    merged(par::map(x.len(), |r| [sum4(&x[r], &g)]))[0].get()
}

/// Neumaier's compensated sum: nearly always the correctly rounded sum, at
/// a few more additions an item.
fn csum(xs: impl IntoIterator<Item = f64>) -> f64 {
    let (s, c) = csum2(xs);
    s + c
}

/// The same, as a sum and what it lost to rounding.
fn csum2(xs: impl IntoIterator<Item = f64>) -> (f64, f64) {
    let mut a = Acc::default();
    for x in xs {
        a.add(x);
    }
    a.get()
}

fn cdot(a: &[f64], b: &[f64]) -> f64 {
    let (s, c) = cdot2(a, b);
    s + c
}

/// A dot product as if in twice the precision, on all the cores when long.
fn cdot2(a: &[f64], b: &[f64]) -> (f64, f64) {
    let n = a.len().min(b.len());
    let parts = par::map(n, |r| {
        let (x, y) = (&a[r.clone()], &b[r]);
        let mut l = [Acc::default(); 4];
        let mut i = 0;
        while i + 4 <= x.len() {
            l[0].add_prod(x[i], y[i]);
            l[1].add_prod(x[i + 1], y[i + 1]);
            l[2].add_prod(x[i + 2], y[i + 2]);
            l[3].add_prod(x[i + 3], y[i + 3]);
            i += 4;
        }
        while i < x.len() {
            l[0].add_prod(x[i], y[i]);
            i += 1;
        }
        let [mut s, b, c, d] = l;
        s.merge(b);
        s.merge(c);
        s.merge(d);
        [s]
    });
    merged(parts)[0].get()
}

/// hi + lo divided by n, rounded once.
fn div2((hi, lo): (f64, f64), n: f64) -> f64 {
    let q = hi / n;
    if !q.is_finite() {
        return q;
    }
    q + ((-q).mul_add(n, hi) + lo) / n
}

/// The exactly rounded sum, as Python's `math.fsum` (Shewchuk's algorithm),
/// in one pass: what `pfsum` must equal.
#[cfg(test)]
fn fsum(xs: impl IntoIterator<Item = f64>) -> f64 {
    let (partials, special) = exact(xs);
    special.unwrap_or_else(|| rounded_sum(&partials))
}

/// The exactly rounded sum of a slice, as Python's `math.fsum` (Shewchuk's
/// algorithm). An exact sum is the same in any
/// order, so each chunk's is found on its own core, and theirs added last.
fn pfsum(x: &[f64]) -> f64 {
    let parts = par::map(x.len(), |r| exact(x[r].iter().copied()));
    if let Some(sp) = parts.iter().filter_map(|p| p.1).reduce(|a, b| a + b) {
        return sp;
    }
    let (all, special) = exact(parts.into_iter().flat_map(|p| p.0));
    special.unwrap_or_else(|| rounded_sum(&all))
}

/// The sum as floats that do not overlap (whose exact sum it is), and the
/// sum of the infinities and NaNs, if any.
fn exact(xs: impl IntoIterator<Item = f64>) -> (Vec<f64>, Option<f64>) {
    let mut partials: Vec<f64> = Vec::new();
    let mut special: Option<f64> = None;
    for mut x in xs {
        if !x.is_finite() {
            special = Some(special.unwrap_or(0.0) + x);
            continue;
        }
        let mut i = 0;
        for j in 0..partials.len() {
            let mut y = partials[j];
            if x.abs() < y.abs() {
                std::mem::swap(&mut x, &mut y);
            }
            let hi = x + y;
            let lo = y - (hi - x);
            if lo != 0.0 {
                partials[i] = lo;
                i += 1;
            }
            x = hi;
        }
        partials.truncate(i);
        // A sum past the floats leaves an infinity and its own rounding error,
        // which would cancel to 0 (FEEDBACK16.md, Q-113; branch feedback20-fixes).
        if !x.is_finite() {
            special = Some(special.unwrap_or(0.0) + x);
            partials.clear();
            continue;
        }
        partials.push(x);
    }
    (partials, special)
}

/// Partials (increasing in size, as `exact` leaves them) to the nearest float.
fn rounded_sum(partials: &[f64]) -> f64 {
    let mut n = partials.len();
    if n == 0 {
        return 0.0;
    }
    n -= 1;
    let mut hi = partials[n];
    let mut lo = 0.0;
    while n > 0 {
        let x = hi;
        n -= 1;
        let y = partials[n];
        hi = x + y;
        lo = y - (hi - x);
        if lo != 0.0 {
            break;
        }
    }
    // Halfway between two floats: round as the remaining partials say.
    if n > 0 && ((lo < 0.0 && partials[n - 1] < 0.0) || (lo > 0.0 && partials[n - 1] > 0.0)) {
        let y = lo * 2.0;
        let x = hi + y;
        if y == x - hi {
            hi = x;
        }
    }
    hi
}

fn mean_of(x: &[f64]) -> f64 {
    let n = x.len() as f64;
    let m = div2(psum(x, |v| v), n);
    // The sum may pass the floats where the mean does not, as in Python
    // (FEEDBACK16.md, Q-114; branch feedback20-fixes).
    if !m.is_finite() && x.iter().all(|v| v.is_finite()) {
        return csum(x.iter().map(|v| v / n));
    }
    m
}

/// The sum of squared deviations from `m`, corrected for `m`'s own
/// rounding, as a sum and what it lost to rounding. One pass, no copy.
fn ss(x: &[f64], m: f64) -> (f64, f64) {
    let parts = par::map(x.len(), |r| {
        let mut a = [Acc::default(); 4];
        let mut it = x[r].chunks_exact(2);
        for c in &mut it {
            let (d0, d1) = (c[0] - m, c[1] - m);
            a[0].add(d0);
            a[1].add(d1);
            a[2].add_prod(d0, d0);
            a[3].add_prod(d1, d1);
        }
        for v in it.remainder() {
            let d = v - m;
            a[0].add(d);
            a[2].add_prod(d, d);
        }
        a[0].merge(a[1]);
        a[2].merge(a[3]);
        [a[0], a[2]]
    });
    let [d, q] = merged(parts);
    let c = d.get().0;
    let (hi, lo) = q.get();
    let fix = c * c / x.len() as f64;
    let t = hi - fix;
    if t <= 0.0 {
        return (0.0, 0.0);
    }
    (t, lo - ((t - hi) + fix))
}

/// The least and the greatest, NaNs aside.
fn bounds(x: &[f64]) -> (f64, f64) {
    par::map(x.len(), |r| x[r].iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(*v), b.max(*v))))
        .into_iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), (c, d)| (a.min(c), b.max(d)))
}

// ---- order -------------------------------------------------------------
//
// Floats ordered as their bits are, once flipped (total_cmp's order). A
// sample of the keys, sorted, gives cut points that split the list into
// ranges of about equal size whatever its distribution; each item goes to
// its range (in its order), on all the cores. Sorting then sorts each range;
// medians, percentiles and quantiles sort only the ranges that hold the
// positions asked for.

fn key(x: f64) -> u64 {
    let b = x.to_bits();
    if b >> 63 == 1 {
        !b
    } else {
        b | 1 << 63
    }
}

fn unkey(k: u64) -> f64 {
    f64::from_bits(if k >> 63 == 1 { k & !(1 << 63) } else { !k })
}

/// Below this, a plain sort is quicker than ranges.
const SMALL: usize = 1 << 14;

/// Cut points between ranges: a key goes to the range after the last cut
/// at or below it.
struct Cuts(Vec<u64>);

impl Cuts {
    /// From a sample of the keys (the same for the same list), about a
    /// thousand items a range.
    fn sample(n: usize, k: impl Fn(usize) -> u64) -> Cuts {
        let ranges = (n / 1024).next_power_of_two().clamp(16, 4096);
        let per = 8;
        let mut r = 0x9E37_79B9_7F4A_7C15u64 ^ n as u64;
        let mut s: Vec<u64> = (0..ranges * per)
            .map(|_| {
                r ^= r << 13;
                r ^= r >> 7;
                r ^= r << 17;
                k((r % n as u64) as usize)
            })
            .collect();
        s.sort_unstable();
        let mut cuts: Vec<u64> = (1..ranges).map(|j| s[j * per]).collect();
        cuts.dedup();
        Cuts(cuts)
    }

    fn len(&self) -> usize {
        self.0.len() + 1
    }

    #[inline(always)]
    fn of(&self, k: u64) -> usize {
        self.0.partition_point(|c| *c <= k)
    }
}

/// The items put in their ranges, each range's in their order, and where
/// each range starts (and the end).
fn distribute<T: Copy + Send + Sync + Default>(v: &[T], k: &(impl Fn(&T) -> u64 + Sync), cuts: &Cuts) -> (Vec<T>, Vec<usize>) {
    let (which, counts) = assign(v, k, cuts);
    scatter(v, &which, &counts)
}

/// Each item's range, and how many of each range every chunk holds.
fn assign<T: Sync>(v: &[T], k: &(impl Fn(&T) -> u64 + Sync), cuts: &Cuts) -> (Vec<u16>, Vec<Vec<u32>>) {
    let n = v.len();
    let m = cuts.len();
    let mut which = vec![0u16; n];
    par::fill(&mut which, |start, part| {
        for (j, w) in part.iter_mut().enumerate() {
            *w = cuts.of(k(&v[start + j])) as u16;
        }
    });
    let counts = par::map(n, |r| {
        let mut h = vec![0u32; m];
        for w in &which[r] {
            h[*w as usize] += 1;
        }
        h
    });
    (which, counts)
}

fn scatter<T: Copy + Send + Sync + Default>(v: &[T], which: &[u16], counts: &[Vec<u32>]) -> (Vec<T>, Vec<usize>) {
    let n = v.len();
    let m = counts.first().map_or(1, |h| h.len());
    // Each chunk's place in each range: after the earlier ranges, and after
    // the earlier chunks' items in this one.
    let mut at: Vec<Vec<usize>> = counts.iter().map(|_| vec![0; m]).collect();
    let mut bounds = Vec::with_capacity(m + 1);
    let mut acc = 0;
    for b in 0..m {
        bounds.push(acc);
        for (c, h) in counts.iter().enumerate() {
            at[c][b] = acc;
            acc += h[b] as usize;
        }
    }
    bounds.push(acc);
    let mut out = vec![T::default(); n];
    let dst = SendPtr(out.as_mut_ptr());
    let at: Vec<std::sync::Mutex<Vec<usize>>> = at.into_iter().map(std::sync::Mutex::new).collect();
    par::map(n, |r| {
        let mut mine = at[r.start / par::CHUNK].lock().unwrap();
        for i in r {
            let b = which[i] as usize;
            // SAFETY: every position is written once, by the chunk it was given to.
            unsafe { *dst.at(mine[b]) = v[i] };
            mine[b] += 1;
        }
    });
    (out, bounds)
}

/// Sorts these ranges of `out` (by their start), on all the cores, in
/// tasks of about a chunk.
fn sort_ranges<T: Send>(out: &mut [T], bounds: &[usize], ranges: &[usize], sort: &(impl Fn(&mut [T]) + Sync)) {
    let mut tasks: Vec<Vec<usize>> = Vec::new();
    let mut size = usize::MAX;
    for &b in ranges {
        if size >= par::CHUNK {
            tasks.push(Vec::new());
            size = 0;
        }
        tasks.last_mut().unwrap().push(b);
        size += bounds[b + 1] - bounds[b];
    }
    let n = out.len();
    let dst = SendPtr(out.as_mut_ptr());
    par::each(tasks.len(), n, |t| {
        for &b in &tasks[t] {
            let (p, q) = (bounds[b], bounds[b + 1]);
            if q - p > 1 {
                // SAFETY: ranges do not overlap, and each is in one task.
                sort(unsafe { std::slice::from_raw_parts_mut(dst.at(p), q - p) });
            }
        }
    });
}

/// The items at these positions of x sorted (each < x.len()).
fn values_at(x: &[f64], at: &[usize]) -> Vec<f64> {
    let n = x.len();
    if n <= SMALL {
        let mut d = x.to_vec();
        d.sort_unstable_by(f64::total_cmp);
        return at.iter().map(|i| d[*i]).collect();
    }
    let k = |v: &f64| key(*v);
    let cuts = Cuts::sample(n, |i| key(x[i]));
    let (which, counts) = assign(x, &k, &cuts);
    let mut start = vec![0usize; cuts.len() + 1];
    for h in &counts {
        for (b, c) in h.iter().enumerate() {
            start[b + 1] += *c as usize;
        }
    }
    for b in 0..cuts.len() {
        start[b + 1] += start[b];
    }
    let range_of = |i: usize| start.partition_point(|s| *s <= i) - 1;
    let mut wanted: Vec<usize> = at.iter().map(|i| range_of(*i)).collect();
    wanted.sort_unstable();
    wanted.dedup();
    let sort = |s: &mut [f64]| s.sort_unstable_by(f64::total_cmp);
    if wanted.len() > 8 {
        let (mut d, bounds) = scatter(x, &which, &counts);
        sort_ranges(&mut d, &bounds, &wanted, &sort);
        return at.iter().map(|i| d[*i]).collect();
    }
    // A few ranges: only their items, gathered and sorted.
    let mut is = vec![false; cuts.len()];
    for b in &wanted {
        is[*b] = true;
    }
    let mut cand: Vec<f64> = par::map(n, |r| r.filter(|i| is[which[*i] as usize]).map(|i| x[i]).collect::<Vec<_>>()).concat();
    sort(&mut cand);
    let mut before = vec![0usize; cuts.len()];
    let mut acc = 0;
    for b in 0..cuts.len() {
        before[b] = acc;
        if is[b] {
            acc += start[b + 1] - start[b];
        }
    }
    at.iter()
        .map(|i| {
            let b = range_of(*i);
            cand[before[b] + (i - start[b])]
        })
        .collect()
}

/// Sorts by key, keeping the order of equal keys (`sort` sorts a range:
/// it must order equal keys as they were, or by something that does).
fn sort_by_key<T: Copy + Send + Sync + Default>(v: &[T], k: impl Fn(&T) -> u64 + Sync, sort: impl Fn(&mut [T]) + Sync) -> Vec<T> {
    let n = v.len();
    if n <= SMALL {
        let mut out = v.to_vec();
        sort(&mut out);
        return out;
    }
    let cuts = Cuts::sample(n, |i| k(&v[i]));
    let (mut out, bounds) = distribute(v, &k, &cuts);
    let all: Vec<usize> = (0..cuts.len()).collect();
    sort_ranges(&mut out, &bounds, &all, &sort);
    out
}

struct SendPtr<T>(*mut T);
unsafe impl<T: Send> Sync for SendPtr<T> {}
unsafe impl<T: Send> Send for SendPtr<T> {}

impl<T> SendPtr<T> {
    /// SAFETY: as for `pointer::add`; callers write disjoint positions.
    unsafe fn at(&self, i: usize) -> *mut T {
        self.0.add(i)
    }
}

/// x sorted (total_cmp's order).
fn sorted(x: Vec<f64>) -> Vec<f64> {
    let keys: Vec<u64> = x.iter().map(|v| key(*v)).collect();
    sort_by_key(&keys, |k| *k, |s| s.sort_unstable()).into_iter().map(unkey).collect()
}

/// The positions of x in sorted order, equal items in theirs.
fn order(x: &[f64], descending: bool) -> Vec<u32> {
    let pairs: Vec<(u64, u32)> = x.iter().enumerate().map(|(i, v)| (if descending { !key(*v + 0.0) } else { key(*v + 0.0) }, i as u32)).collect();
    sort_by_key(&pairs, |p| p.0, |s| s.sort_unstable()).into_iter().map(|p| p.1).collect()
}

/// For the builtin `sort`: a list of only ints, or only floats (no NaN),
/// sorted, keeping equal items (0.0 and -0.0) in their order.
pub fn sort_numbers(l: &[Value]) -> Option<Value> {
    if l.len() < 64 {
        return None;
    }
    if l.iter().all(|v| matches!(v, Value::Int(_))) {
        let keys: Vec<u64> = l.iter().map(|v| if let Value::Int(n) = v { (*n as u64) ^ 1 << 63 } else { 0 }).collect();
        let s = sort_by_key(&keys, |k| *k, |s| s.sort_unstable());
        return Some(Value::list(s.into_iter().map(|k| Value::Int((k ^ 1 << 63) as i64)).collect()));
    }
    let mut x = Vec::with_capacity(l.len());
    for v in l {
        match v {
            Value::Float(f) if !f.is_nan() => x.push(*f),
            _ => return None,
        }
    }
    if x.iter().any(|v| v.to_bits() == 1 << 63) {
        return Some(Value::list(order(&x, false).into_iter().map(|i| Value::Float(x[i as usize])).collect()));
    }
    Some(floats(sorted(x)))
}

// ---- math --------------------------------------------------------------

// ---- bit ---------------------------------------------------------------

// `| ` is the pipe, so these are functions, not operators, and each works on
// the 64 bits of an int, in the browser too (FEEDBACK19.md, Q-153; branch
// feedback19-fixes).

/// How far to shift: past 63 every bit would be gone, which is a mistake.
fn shift(args: &[Value], f: &str, line: u32) -> Result<u32, Ctrl> {
    let n = int(args, 1, f, line)?;
    match u32::try_from(n) {
        Ok(k) if k < 64 => Ok(k),
        _ => fail(line, format!("`{}`: a shift is 0 to 63, got {}", f, n)),
    }
}

fn b_and(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(Value::Int(int(&args, 0, "bit.and", line)? & int(&args, 1, "bit.and", line)?))
}

fn b_or(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(Value::Int(int(&args, 0, "bit.or", line)? | int(&args, 1, "bit.or", line)?))
}

fn b_xor(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(Value::Int(int(&args, 0, "bit.xor", line)? ^ int(&args, 1, "bit.xor", line)?))
}

fn b_not(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(Value::Int(!int(&args, 0, "bit.not", line)?))
}

/// Bits past the top are dropped: this is shifting, not `*`, which refuses.
fn b_shl(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(Value::Int(int(&args, 0, "bit.shl", line)? << shift(&args, "bit.shl", line)?))
}

/// The sign is kept, as in JavaScript's `>>`; `bit.and` masks it off.
fn b_shr(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(Value::Int(int(&args, 0, "bit.shr", line)? >> shift(&args, "bit.shr", line)?))
}

const BIT: &[(&str, BuiltinFn)] = &[
    ("bit.and", b_and),
    ("bit.or", b_or),
    ("bit.xor", b_xor),
    ("bit.not", b_not),
    ("bit.shl", b_shl),
    ("bit.shr", b_shr),
];

const MATH: &[(&str, BuiltinFn)] = &[
    ("math.sqrt", m_sqrt),
    ("math.cbrt", m_cbrt),
    ("math.exp", m_exp),
    ("math.exp2", m_exp2),
    ("math.expm1", m_expm1),
    ("math.log", m_log),
    ("math.log2", m_log2),
    ("math.log10", m_log10),
    ("math.log1p", m_log1p),
    ("math.pow", m_pow),
    ("math.sin", m_sin),
    ("math.cos", m_cos),
    ("math.tan", m_tan),
    ("math.asin", m_asin),
    ("math.acos", m_acos),
    ("math.atan", m_atan),
    ("math.atan2", m_atan2),
    ("math.sinh", m_sinh),
    ("math.cosh", m_cosh),
    ("math.tanh", m_tanh),
    ("math.asinh", m_asinh),
    ("math.acosh", m_acosh),
    ("math.atanh", m_atanh),
    ("math.degrees", m_degrees),
    ("math.radians", m_radians),
    ("math.hypot", m_hypot),
    ("math.dist", m_dist),
    ("math.floor", m_floor),
    ("math.ceil", m_ceil),
    ("math.trunc", m_trunc),
    ("math.fabs", m_fabs),
    ("math.copysign", m_copysign),
    ("math.fmod", m_fmod),
    ("math.remainder", m_remainder),
    ("math.modf", m_modf),
    ("math.frexp", m_frexp),
    ("math.ldexp", m_ldexp),
    ("math.factorial", m_factorial),
    ("math.gcd", m_gcd),
    ("math.lcm", m_lcm),
    ("math.comb", m_comb),
    ("math.perm", m_perm),
    ("math.isqrt", m_isqrt),
    ("math.isclose", m_isclose),
    ("math.isfinite", m_isfinite),
    ("math.isinf", m_isinf),
    ("math.isnan", m_isnan),
    ("math.prod", m_prod),
    ("math.fsum", m_fsum),
    ("math.sumprod", m_sumprod),
    ("math.erf", m_erf),
    ("math.erfc", m_erfc),
    ("math.gamma", m_gamma),
    ("math.lgamma", m_lgamma),
];

/// A float result, refused as Python refuses it: outside the function's
/// domain (or NaN from a number), or too large for a float.
fn checked(f: &str, x: f64, y: f64, in_domain: bool, line: u32) -> R {
    if !in_domain || (y.is_nan() && !x.is_nan()) {
        return fail(line, format!("`math.{}`: math domain error ({})", f, x));
    }
    if y.is_infinite() && x.is_finite() {
        return fail(line, format!("`math.{}`: math range error ({})", f, x));
    }
    Ok(Value::Float(y))
}

macro_rules! unary {
    ($($id:ident $name:literal $f:expr, $dom:expr;)*) => {$(
        fn $id(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
            let x = num(&args, 0, concat!("math.", $name), line)?;
            let f: fn(f64) -> f64 = $f;
            let dom: fn(f64) -> bool = $dom;
            checked($name, x, f(x), x.is_nan() || dom(x), line)
        }
    )*};
}

fn any(_: f64) -> bool {
    true
}

fn not_pole(x: f64) -> bool {
    !(x <= 0.0 && x.fract() == 0.0) && x != f64::NEG_INFINITY
}

unary! {
    m_sqrt "sqrt" f64::sqrt, |x| x >= 0.0;
    m_cbrt "cbrt" f64::cbrt, any;
    m_exp "exp" f64::exp, any;
    m_exp2 "exp2" f64::exp2, any;
    m_expm1 "expm1" f64::exp_m1, any;
    m_log2 "log2" f64::log2, |x| x > 0.0;
    m_log10 "log10" f64::log10, |x| x > 0.0;
    m_log1p "log1p" f64::ln_1p, |x| x > -1.0;
    m_sin "sin" f64::sin, |x| x.is_finite();
    m_cos "cos" f64::cos, |x| x.is_finite();
    m_tan "tan" f64::tan, |x| x.is_finite();
    m_asin "asin" f64::asin, |x| x.abs() <= 1.0;
    m_acos "acos" f64::acos, |x| x.abs() <= 1.0;
    m_atan "atan" f64::atan, any;
    m_sinh "sinh" f64::sinh, any;
    m_cosh "cosh" f64::cosh, any;
    m_tanh "tanh" f64::tanh, any;
    m_asinh "asinh" f64::asinh, any;
    m_acosh "acosh" f64::acosh, |x| x >= 1.0;
    m_atanh "atanh" f64::atanh, |x| x.abs() < 1.0;
    m_degrees "degrees" f64::to_degrees, any;
    m_radians "radians" f64::to_radians, any;
    m_fabs "fabs" f64::abs, any;
    m_erf "erf" erf, any;
    m_erfc "erfc" erfc, any;
    m_gamma "gamma" gamma, not_pole;
    m_lgamma "lgamma" lgamma, not_pole;
}

/// `math.log(x)`, or `math.log(x, base)`.
fn m_log(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let x = num(&args, 0, "math.log", line)?;
    let y = match given(&args, 1) {
        None => x.ln(),
        Some(_) => {
            let b = num(&args, 1, "math.log", line)?;
            if b <= 0.0 || b == 1.0 {
                return fail(line, format!("`math.log`: math domain error (base {})", b));
            }
            x.ln() / b.ln()
        }
    };
    checked("log", x, y, x.is_nan() || x > 0.0, line)
}

fn two(args: &[Value], f: &str, line: u32) -> Result<(f64, f64), Ctrl> {
    Ok((num(args, 0, f, line)?, num(args, 1, f, line)?))
}

fn checked2(f: &str, x: f64, y: f64, r: f64, ok: bool, line: u32) -> R {
    let nan_in = x.is_nan() || y.is_nan();
    if !ok || (r.is_nan() && !nan_in) {
        return fail(line, format!("`math.{}`: math domain error ({}, {})", f, x, y));
    }
    if r.is_infinite() && x.is_finite() && y.is_finite() {
        return fail(line, format!("`math.{}`: math range error ({}, {})", f, x, y));
    }
    Ok(Value::Float(r))
}

fn m_pow(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (x, y) = two(&args, "math.pow", line)?;
    // 0 to a negative power is a domain error in Python, not infinity.
    checked2("pow", x, y, x.powf(y), !(x == 0.0 && y < 0.0), line)
}

fn m_atan2(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (y, x) = two(&args, "math.atan2", line)?;
    Ok(Value::Float(y.atan2(x)))
}

fn m_copysign(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (x, y) = two(&args, "math.copysign", line)?;
    Ok(Value::Float(x.copysign(y)))
}

fn m_fmod(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (x, y) = two(&args, "math.fmod", line)?;
    checked2("fmod", x, y, x % y, !(y == 0.0 || x.is_infinite()) || x.is_nan() || y.is_nan(), line)
}

fn m_remainder(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (x, y) = two(&args, "math.remainder", line)?;
    checked2("remainder", x, y, unsafe { c::remainder(x, y) }, !(y == 0.0 || x.is_infinite()) || x.is_nan() || y.is_nan(), line)
}

fn m_hypot(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let mut v = Vec::with_capacity(args.len());
    for i in 0..args.len() {
        v.push(num(&args, i, "math.hypot", line)?);
    }
    Ok(Value::Float(norm2(&v)))
}

/// The largest size in `v`, or NaN with one in it: `f64::max` passes over NaN
/// (FEEDBACK16.md, Q-115; branch feedback20-fixes).
fn largest(v: &[f64]) -> f64 {
    v.iter().fold(0.0f64, |m, x| if m.is_nan() || x.is_nan() { f64::NAN } else { m.max(x.abs()) })
}

/// The Euclidean length, scaled so that squaring neither overflows nor underflows.
fn norm2(v: &[f64]) -> f64 {
    if v.iter().any(|x| x.is_infinite()) {
        return f64::INFINITY;
    }
    let big = largest(v);
    if big == 0.0 || big.is_nan() {
        return big;
    }
    big * csum(v.iter().map(|x| (x / big) * (x / big))).sqrt()
}

fn m_dist(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let p = xs(&args, 0, "math.dist", line)?;
    let q = xs(&args, 1, "math.dist", line)?;
    if p.len() != q.len() {
        return fail(line, format!("`math.dist`: both points must have the same number of dimensions ({} and {})", p.len(), q.len()));
    }
    let d: Vec<f64> = p.iter().zip(&q).map(|(a, b)| a - b).collect();
    Ok(Value::Float(norm2(&d)))
}

fn rounded(args: &[Value], f: &str, line: u32, r: fn(f64) -> f64) -> R {
    if let Some(Value::Int(n)) = args.first() {
        return Ok(Value::Int(*n));
    }
    let x = num(args, 0, f, line)?;
    if !x.is_finite() {
        return fail(line, format!("`{}`: cannot convert {} to an int", f, x));
    }
    let y = r(x);
    if y.abs() >= 9.2e18 {
        return Err(overflow(line));
    }
    Ok(Value::Int(y as i64))
}

fn m_floor(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    rounded(&args, "math.floor", line, f64::floor)
}

fn m_ceil(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    rounded(&args, "math.ceil", line, f64::ceil)
}

fn m_trunc(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    rounded(&args, "math.trunc", line, f64::trunc)
}

/// `[fractional part, integer part]`, both with x's sign.
fn m_modf(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let x = num(&args, 0, "math.modf", line)?;
    if x.is_infinite() {
        return Ok(floats([0.0f64.copysign(x), x]));
    }
    Ok(floats([x.fract(), x.trunc()]))
}

/// `[m, e]` with x = m * 2^e and 0.5 <= |m| < 1.
fn m_frexp(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let mut e = 0;
    let m = unsafe { c::frexp(num(&args, 0, "math.frexp", line)?, &mut e) };
    Ok(Value::list(vec![Value::Float(m), Value::Int(e as i64)]))
}

fn m_ldexp(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let x = num(&args, 0, "math.ldexp", line)?;
    let e = int(&args, 1, "math.ldexp", line)?.clamp(-10_000, 10_000) as i32;
    checked("ldexp", x, unsafe { c::ldexp(x, e) }, true, line)
}

fn natural(args: &[Value], i: usize, f: &str, line: u32) -> Result<i64, Ctrl> {
    let n = int(args, i, f, line)?;
    if n < 0 {
        return fail(line, format!("`{}` is not defined for negative numbers ({})", f, n));
    }
    Ok(n)
}

fn m_factorial(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let n = natural(&args, 0, "math.factorial", line)?;
    (1..=n).try_fold(1i64, |a, k| a.checked_mul(k)).map(Value::Int).ok_or_else(|| overflow(line))
}

fn gcd(mut a: i64, mut b: i64) -> i64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a.abs()
}

fn ints(args: &[Value], f: &str, line: u32) -> Result<Vec<i64>, Ctrl> {
    // `gcd(a, b, c)` or `gcd([a, b, c])`.
    if let [Value::List(l)] = args {
        return (0..l.len()).map(|i| int(l, i, f, line)).collect();
    }
    (0..args.len()).map(|i| int(args, i, f, line)).collect()
}

fn m_gcd(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let v = ints(&args, "math.gcd", line)?;
    if v.contains(&i64::MIN) {
        return Err(overflow(line));
    }
    Ok(Value::Int(v.into_iter().fold(0, gcd)))
}

fn m_lcm(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let v = ints(&args, "math.lcm", line)?;
    // A zero anywhere makes it 0, before any absolute value can overflow
    // (FEEDBACK16.md, Q-147; branch feedback20-fixes).
    if v.contains(&0) {
        return Ok(Value::Int(0));
    }
    let mut acc = 1i64;
    for n in v {
        let n = n.checked_abs().ok_or_else(|| overflow(line))?;
        acc = (acc / gcd(acc, n)).checked_mul(n).ok_or_else(|| overflow(line))?;
    }
    Ok(Value::Int(acc))
}

fn m_comb(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let n = natural(&args, 0, "math.comb", line)?;
    let k = natural(&args, 1, "math.comb", line)?;
    if k > n {
        return Ok(Value::Int(0));
    }
    let k = k.min(n - k);
    let mut r: u128 = 1;
    for i in 0..k as u128 {
        r = r * (n as u128 - i) / (i + 1);
        if r > i64::MAX as u128 {
            return Err(overflow(line));
        }
    }
    Ok(Value::Int(r as i64))
}

fn m_perm(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let n = natural(&args, 0, "math.perm", line)?;
    let k = if given(&args, 1).is_some() { natural(&args, 1, "math.perm", line)? } else { n };
    if k > n {
        return Ok(Value::Int(0));
    }
    ((n - k + 1)..=n).try_fold(1i64, |a, x| a.checked_mul(x)).map(Value::Int).ok_or_else(|| overflow(line))
}

fn m_isqrt(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let n = natural(&args, 0, "math.isqrt", line)?;
    let mut s = (n as f64).sqrt() as i64;
    while s.checked_mul(s).map_or(true, |q| q > n) {
        s -= 1;
    }
    while (s + 1).checked_mul(s + 1).map_or(false, |q| q <= n) {
        s += 1;
    }
    Ok(Value::Int(s))
}

/// `math.isclose(a, b, rel_tol = 1e-9, abs_tol = 0)`.
fn m_isclose(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (a, b) = two(&args, "math.isclose", line)?;
    let rel = num_or(&args, 2, "math.isclose", line, 1e-9)?;
    let abs = num_or(&args, 3, "math.isclose", line, 0.0)?;
    if rel < 0.0 || abs < 0.0 {
        return fail(line, "`math.isclose`: tolerances must be non-negative");
    }
    if a == b {
        return Ok(Value::Bool(true));
    }
    if a.is_infinite() || b.is_infinite() {
        return Ok(Value::Bool(false));
    }
    let d = (a - b).abs();
    Ok(Value::Bool(d <= (rel * b).abs() || d <= (rel * a).abs() || d <= abs))
}

fn m_isfinite(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(Value::Bool(num(&args, 0, "math.isfinite", line)?.is_finite()))
}

fn m_isinf(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(Value::Bool(num(&args, 0, "math.isinf", line)?.is_infinite()))
}

fn m_isnan(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(Value::Bool(num(&args, 0, "math.isnan", line)?.is_nan()))
}

/// `math.prod(list, start = 1)`: an int while everything is one.
fn m_prod(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let l = list(&args, 0, "math.prod", line)?;
    let mut acc = given(&args, 1).cloned().unwrap_or(Value::Int(1));
    for x in l {
        acc = match (&acc, x) {
            (Value::Int(a), Value::Int(b)) => Value::Int(a.checked_mul(*b).ok_or_else(|| overflow(line))?),
            _ => match (acc.as_f64(), x.as_f64()) {
                (Some(a), Some(b)) => Value::Float(a * b),
                _ => return fail(line, format!("`math.prod` expects numbers, got {}", x.type_name())),
            },
        };
    }
    Ok(acc)
}

fn m_fsum(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let x = xs(&args, 0, "math.fsum", line)?;
    let s = pfsum(&x);
    // What Python raises (FEEDBACK16.md, Q-113; branch feedback20-fixes).
    if s.is_nan() && x.contains(&f64::INFINITY) && x.contains(&f64::NEG_INFINITY) {
        return fail(line, "`math.fsum`: inf and -inf have no sum");
    }
    if !s.is_finite() && x.iter().all(|v| v.is_finite()) {
        return fail(line, "`math.fsum`: the sum is too large for a float");
    }
    Ok(Value::Float(s))
}

fn pair(args: &[Value], f: &str, line: u32) -> Result<(Vec<f64>, Vec<f64>, bool), Ctrl> {
    let (a, ia) = nums(args, 0, f, line)?;
    let (b, ib) = nums(args, 1, f, line)?;
    if a.len() != b.len() {
        return fail(line, format!("`{}` needs lists of the same length ({} and {})", f, a.len(), b.len()));
    }
    Ok((a, b, ia && ib))
}

/// Ints, multiplied and added exactly; else a float, as if in twice the precision.
fn dot(args: &[Value], f: &str, line: u32) -> R {
    let (a, b, ints) = pair(args, f, line)?;
    if ints {
        let (p, q) = (list(args, 0, f, line)?, list(args, 1, f, line)?);
        let mut acc = 0i64;
        for (x, y) in p.iter().zip(q) {
            if let (Value::Int(x), Value::Int(y)) = (x, y) {
                acc = x.checked_mul(*y).and_then(|m| acc.checked_add(m)).ok_or_else(|| overflow(line))?;
            }
        }
        return Ok(Value::Int(acc));
    }
    Ok(Value::Float(cdot(&a, &b)))
}

fn m_sumprod(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    dot(&args, "math.sumprod", line)
}

// ---- stats -------------------------------------------------------------

const STATS: &[(&str, BuiltinFn)] = &[
    ("stats.mean", s_mean),
    ("stats.fmean", s_fmean),
    ("stats.geometric_mean", s_geometric_mean),
    ("stats.harmonic_mean", s_harmonic_mean),
    ("stats.median", s_median),
    ("stats.median_low", s_median_low),
    ("stats.median_high", s_median_high),
    ("stats.mode", s_mode),
    ("stats.multimode", s_multimode),
    ("stats.pvariance", s_pvariance),
    ("stats.variance", s_variance),
    ("stats.pstdev", s_pstdev),
    ("stats.stdev", s_stdev),
    ("stats.quantiles", s_quantiles),
    ("stats.percentile", s_percentile),
    ("stats.covariance", s_covariance),
    ("stats.correlation", s_correlation),
    ("stats.linear_regression", s_linear_regression),
    ("stats.describe", s_describe),
    ("stats.histogram", s_histogram),
    ("stats.zscore", s_zscore),
    ("stats.normalize", s_normalize),
    ("stats.rank", s_rank),
    ("stats.normal_pdf", s_normal_pdf),
    ("stats.normal_cdf", s_normal_cdf),
    ("stats.normal_inv", s_normal_inv),
];

fn data(args: &[Value], f: &str, least: usize, line: u32) -> Result<(Vec<f64>, bool), Ctrl> {
    let (x, ints) = nums(args, 0, f, line)?;
    if x.len() < least {
        let need = if least == 1 { "at least one data point".to_string() } else { format!("at least {} data points", least) };
        return fail(line, format!("`{}` requires {}", f, need));
    }
    Ok((x, ints))
}

fn s_mean(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (x, _) = data(&args, "stats.mean", 1, line)?;
    Ok(Value::Float(mean_of(&x)))
}

/// `stats.fmean(data, weights = null)`.
fn s_fmean(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (x, _) = data(&args, "stats.fmean", 1, line)?;
    if given(&args, 1).is_none() {
        return Ok(Value::Float(mean_of(&x)));
    }
    let w = xs(&args, 1, "stats.fmean", line)?;
    if w.len() != x.len() {
        return fail(line, "`stats.fmean`: data and weights must be the same length");
    }
    let total = csum(w.iter().copied());
    if total == 0.0 {
        return fail(line, "`stats.fmean`: the sum of the weights must be non-zero");
    }
    Ok(Value::Float(cdot(&x, &w) / total))
}

fn s_geometric_mean(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (x, _) = data(&args, "stats.geometric_mean", 1, line)?;
    if x.iter().any(|v| *v <= 0.0) {
        return fail(line, "`stats.geometric_mean` requires positive numbers");
    }
    Ok(Value::Float((csum(x.iter().map(|v| v.ln())) / x.len() as f64).exp()))
}

fn s_harmonic_mean(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (x, _) = data(&args, "stats.harmonic_mean", 1, line)?;
    if x.iter().any(|v| *v < 0.0) {
        return fail(line, "`stats.harmonic_mean` does not support negative values");
    }
    if x.iter().any(|v| *v == 0.0) {
        return Ok(Value::Float(0.0));
    }
    // Each reciprocal with its rounding error, so that the sum is of the exact ones.
    let r: Vec<f64> = x.iter().map(|v| 1.0 / v).collect();
    let e = x.iter().zip(&r).map(|(v, q)| (-q).mul_add(*v, 1.0) / v);
    Ok(Value::Float(x.len() as f64 / csum(r.iter().copied().chain(e))))
}

fn s_median(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (x, ints) = data(&args, "stats.median", 1, line)?;
    let n = x.len();
    if n % 2 == 1 {
        return Ok(back(values_at(&x, &[n / 2])[0], ints));
    }
    let v = values_at(&x, &[n / 2 - 1, n / 2]);
    Ok(Value::Float((v[0] + v[1]) / 2.0))
}

fn s_median_low(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (x, ints) = data(&args, "stats.median_low", 1, line)?;
    let n = x.len();
    Ok(back(values_at(&x, &[if n % 2 == 1 { n / 2 } else { n / 2 - 1 }])[0], ints))
}

fn s_median_high(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (x, ints) = data(&args, "stats.median_high", 1, line)?;
    let n = x.len();
    Ok(back(values_at(&x, &[n / 2])[0], ints))
}

/// A hash for numbers' bits: a multiply, not SipHash (nothing here is an
/// attacker's choice of keys that would matter: the worst is a slow mode).
#[derive(Default)]
struct Bits(u64);

impl std::hash::Hasher for Bits {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, b: &[u8]) {
        for x in b {
            self.0 = (self.0.rotate_left(5) ^ *x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        }
    }
    fn write_u64(&mut self, x: u64) {
        self.0 = (x ^ (x >> 29)).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (x >> 32);
    }
}

type Fast<K> = HashMap<K, usize, std::hash::BuildHasherDefault<Bits>>;

/// How often each value occurs, in the order first seen.
fn tally(l: &[Value]) -> Vec<(usize, usize)> {
    let mut nums: Fast<u64> = Fast::default();
    let mut others: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (i, v) in l.iter().enumerate() {
        let at = match v {
            // 1 and 1.0 are the same value, and so are 0.0 and -0.0.
            Value::Int(_) | Value::Float(_) => nums.entry((v.as_f64().unwrap() + 0.0).to_bits()),
            Value::Str(s) => {
                let n = out.len();
                let j = *others.entry(format!("s{}", s)).or_insert(n);
                if j == n {
                    out.push((i, 0));
                }
                out[j].1 += 1;
                continue;
            }
            other => {
                let n = out.len();
                let j = *others.entry(format!("v{}", other.repr())).or_insert(n);
                if j == n {
                    out.push((i, 0));
                }
                out[j].1 += 1;
                continue;
            }
        };
        let n = out.len();
        let j = *at.or_insert(n);
        if j == n {
            out.push((i, 0));
        }
        out[j].1 += 1;
    }
    out
}

/// Where the most common values first occur, in that order.
fn most_common(l: &[Value]) -> Vec<usize> {
    if l.len() > SMALL && l.iter().all(|v| matches!(v, Value::Int(_) | Value::Float(_))) {
        // Numbers: sorted (equal ones in their order), counted by runs.
        let x: Vec<f64> = l.iter().map(|v| v.as_f64().unwrap()).collect();
        let idx = order(&x, false);
        let (mut most, mut firsts) = (0, Vec::new());
        let mut i = 0;
        while i < idx.len() {
            let mut j = i + 1;
            while j < idx.len() && x[idx[j] as usize] == x[idx[i] as usize] {
                j += 1;
            }
            if j - i > most {
                most = j - i;
                firsts.clear();
            }
            if j - i == most {
                firsts.push(idx[i] as usize);
            }
            i = j;
        }
        firsts.sort_unstable();
        return firsts;
    }
    let t = tally(l);
    let most = t.iter().map(|x| x.1).max().unwrap_or(0);
    t.iter().filter(|x| x.1 == most).map(|x| x.0).collect()
}

fn s_mode(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let l = list(&args, 0, "stats.mode", line)?;
    match most_common(l).first() {
        Some(i) => Ok(l[*i].clone()),
        None => fail(line, "`stats.mode`: no mode for empty data"),
    }
}

fn s_multimode(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let l = list(&args, 0, "stats.multimode", line)?;
    Ok(Value::list(most_common(l).into_iter().map(|i| l[i].clone()).collect()))
}

fn variance(args: &[Value], f: &str, sample: bool, line: u32) -> Result<f64, Ctrl> {
    let (x, _) = data(args, f, if sample { 2 } else { 1 }, line)?;
    let m = match given(args, 1) {
        Some(_) => num(args, 1, f, line)?,
        None => mean_of(&x),
    };
    Ok(div2(ss(&x, m), (x.len() - sample as usize) as f64))
}

fn s_pvariance(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(Value::Float(variance(&args, "stats.pvariance", false, line)?))
}

fn s_variance(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(Value::Float(variance(&args, "stats.variance", true, line)?))
}

fn s_pstdev(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(Value::Float(variance(&args, "stats.pstdev", false, line)?.sqrt()))
}

fn s_stdev(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(Value::Float(variance(&args, "stats.stdev", true, line)?.sqrt()))
}

/// Most parts `stats.quantiles` cuts data into: n - 1 numbers are made,
/// and a request's n must not take the server's memory.
pub const MOST_QUANTILES: i64 = 10_000;

/// `stats.quantiles(data, n = 4, method = "exclusive")`: the n - 1 cut
/// points, as Python computes them.
fn s_quantiles(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let f = "stats.quantiles";
    let (x, _) = data(&args, f, 1, line)?;
    let n = int_or(&args, 1, f, line, 4)?;
    if !(1..=MOST_QUANTILES).contains(&n) {
        return fail(line, format!("`{}`: n must be from 1 to {}, got {}", f, MOST_QUANTILES, n));
    }
    let method = text_or(&args, 2, f, line, "exclusive")?;
    let ld = x.len() as i64;
    if ld == 1 {
        return Ok(floats(std::iter::repeat(x[0]).take(n as usize - 1)));
    }
    // Each cut point is between two neighbours: (position of the lower, delta).
    let cuts: Vec<(usize, i64)> = match method {
        "inclusive" => {
            let m = ld - 1;
            (1..n).map(|i| ((i * m / n) as usize, i * m - (i * m / n) * n)).collect()
        }
        "exclusive" => {
            let m = ld + 1;
            (1..n)
                .map(|i| {
                    let j = (i * m / n).clamp(1, ld - 1);
                    (j as usize - 1, i * m - j * n)
                })
                .collect()
        }
        other => return fail(line, format!("`{}`: unknown method {:?} (exclusive or inclusive)", f, other)),
    };
    let at: Vec<usize> = cuts.iter().flat_map(|(j, _)| [*j, j + 1]).collect();
    let v = values_at(&x, &at);
    let nf = n as f64;
    Ok(floats(cuts.iter().enumerate().map(|(k, (_, delta))| (v[2 * k] * (nf - *delta as f64) + v[2 * k + 1] * *delta as f64) / nf)))
}

/// Linear between the closest ranks, as numpy's default.
fn percentiles(x: &[f64], ps: &[f64]) -> Vec<f64> {
    let n = x.len();
    let place = |p: f64| {
        let h = p / 100.0 * (n - 1) as f64;
        (h.floor() as usize, h - h.floor())
    };
    let at: Vec<usize> = ps.iter().flat_map(|p| {
        let lo = place(*p).0;
        [lo, (lo + 1).min(n - 1)]
    }).collect();
    let v = values_at(x, &at);
    ps.iter().enumerate().map(|(k, p)| v[2 * k] + (v[2 * k + 1] - v[2 * k]) * place(*p).1).collect()
}

/// `stats.percentile(data, p)`, p from 0 to 100, or a list of them.
fn s_percentile(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let f = "stats.percentile";
    let (x, _) = data(&args, f, 1, line)?;
    let check = |p: f64| if (0.0..=100.0).contains(&p) { Ok(p) } else { fail(line, format!("`{}`: percentiles are from 0 to 100, got {}", f, p)) };
    if let Some(Value::List(_)) = args.get(1) {
        let ps = xs(&args, 1, f, line)?;
        for p in &ps {
            check(*p)?;
        }
        return Ok(floats(percentiles(&x, &ps)));
    }
    let p = check(num(&args, 1, f, line)?)?;
    Ok(Value::Float(percentiles(&x, &[p])[0]))
}

fn paired(args: &[Value], f: &str, line: u32) -> Result<(Vec<f64>, Vec<f64>), Ctrl> {
    let (x, y, _) = pair(args, f, line)?;
    if x.len() < 2 {
        return fail(line, format!("`{}` requires at least two data points", f));
    }
    Ok((x, y))
}

/// The means, and the sums of squares and of products about them
/// (corrected for the means' own rounding): one pass, no copies.
fn moments(x: &[f64], y: &[f64]) -> (f64, f64, f64, f64, f64) {
    let (mx, my) = (mean_of(x), mean_of(y));
    let parts = par::map(x.len(), |r| {
        let mut a = [Acc::default(); 5];
        for (u, v) in x[r.clone()].iter().zip(&y[r]) {
            let (dx, dy) = (u - mx, v - my);
            a[0].add(dx);
            a[1].add(dy);
            a[2].add_prod(dx, dx);
            a[3].add_prod(dy, dy);
            a[4].add_prod(dx, dy);
        }
        a
    });
    let [dx, dy, xx, yy, xy] = merged(parts);
    let n = x.len() as f64;
    let (dx, dy) = (dx.get().0, dy.get().0);
    let sum = |a: Acc| {
        let (s, c) = a.get();
        s + c
    };
    (mx, my, (sum(xx) - dx * dx / n).max(0.0), (sum(yy) - dy * dy / n).max(0.0), sum(xy) - dx * dy / n)
}

fn s_covariance(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (x, y) = paired(&args, "stats.covariance", line)?;
    let (_, _, _, _, sxy) = moments(&x, &y);
    Ok(Value::Float(sxy / (x.len() - 1) as f64))
}

/// Average ranks from 1, ties sharing theirs.
fn ranks(x: &[f64]) -> Vec<f64> {
    let idx: Vec<usize> = order(x, false).into_iter().map(|i| i as usize).collect();
    let mut r = vec![0.0; x.len()];
    let mut i = 0;
    while i < idx.len() {
        let mut j = i;
        while j + 1 < idx.len() && x[idx[j + 1]] == x[idx[i]] {
            j += 1;
        }
        let avg = (i + j) as f64 / 2.0 + 1.0;
        for k in &idx[i..=j] {
            r[*k] = avg;
        }
        i = j + 1;
    }
    r
}

/// `stats.correlation(x, y, method = "linear")`: Pearson's, or Spearman's with "ranked".
fn s_correlation(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let f = "stats.correlation";
    let (mut x, mut y) = paired(&args, f, line)?;
    match text_or(&args, 2, f, line, "linear")? {
        "linear" => {}
        "ranked" => {
            x = ranks(&x);
            y = ranks(&y);
        }
        other => return fail(line, format!("`{}`: unknown method {:?} (linear or ranked)", f, other)),
    }
    let (_, _, sxx, syy, sxy) = moments(&x, &y);
    if sxx == 0.0 || syy == 0.0 {
        return fail(line, format!("`{}`: at least one of the inputs is constant", f));
    }
    Ok(Value::Float((sxy / (sxx * syy).sqrt()).clamp(-1.0, 1.0)))
}

/// `stats.linear_regression(x, y, proportional = false)`: `{ slope, intercept }`.
fn s_linear_regression(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let f = "stats.linear_regression";
    let (x, y) = paired(&args, f, line)?;
    let proportional = given(&args, 2).map_or(false, Value::truthy);
    let (slope, intercept) = if proportional {
        let sxx = cdot(&x, &x);
        if sxx == 0.0 {
            return fail(line, format!("`{}`: x is all zeros", f));
        }
        (cdot(&x, &y) / sxx, 0.0)
    } else {
        let (mx, my, sxx, _, sxy) = moments(&x, &y);
        if sxx == 0.0 {
            return fail(line, format!("`{}`: x is constant", f));
        }
        let slope = sxy / sxx;
        (slope, my - slope * mx)
    };
    let mut m = Map::new();
    m.insert("slope".into(), Value::Float(slope));
    m.insert("intercept".into(), Value::Float(intercept));
    Ok(Value::map(m))
}

/// `{ count, sum, mean, std, min, q1, median, q3, max }`, as a data frame's
/// describe (std of a sample, quartiles by numpy's rule). Nulls for no data.
fn s_describe(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (x, ints) = nums(&args, 0, "stats.describe", line)?;
    let n = x.len();
    let mut m = Map::new();
    m.insert("count".into(), Value::Int(n as i64));
    let total = {
        let (s, c) = psum(&x, |v| v);
        s + c
    };
    m.insert("sum".into(), back(total, ints));
    let put = |m: &mut Map, k: &str, v: Option<Value>| {
        m.insert(k.into(), v.unwrap_or(Value::Nil));
    };
    let mean = (n > 0).then(|| total / n as f64);
    put(&mut m, "mean", mean.map(Value::Float));
    put(&mut m, "std", (n > 1).then(|| Value::Float(div2(ss(&x, mean.unwrap()), (n - 1) as f64).sqrt())));
    let (lo, hi) = bounds(&x);
    let q = if n > 0 { percentiles(&x, &[25.0, 50.0, 75.0]) } else { Vec::new() };
    put(&mut m, "min", (n > 0).then(|| back(lo, ints)));
    put(&mut m, "q1", q.first().map(|v| Value::Float(*v)));
    put(&mut m, "median", q.get(1).map(|v| Value::Float(*v)));
    put(&mut m, "q3", q.get(2).map(|v| Value::Float(*v)));
    put(&mut m, "max", (n > 0).then(|| back(hi, ints)));
    Ok(Value::map(m))
}

/// `stats.histogram(data, bins = 10)`: `{ counts, edges }`, as numpy's: equal
/// bins from the least to the greatest value, or the edges given as a list.
/// Each bin holds its left edge; the last holds its right one too.
fn s_histogram(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let f = "stats.histogram";
    let x = xs(&args, 0, f, line)?;
    let edges = if let Some(Value::List(_)) = args.get(1) {
        let e = xs(&args, 1, f, line)?;
        if e.len() < 2 || e.windows(2).any(|w| w[1] < w[0]) {
            return fail(line, format!("`{}`: edges must be at least two, increasing", f));
        }
        e
    } else {
        let bins = int_or(&args, 1, f, line, 10)?;
        if !(1..=1_000_000).contains(&bins) {
            return fail(line, format!("`{}`: bins must be from 1 to 1000000, got {}", f, bins));
        }
        let (mut lo, mut hi) = bounds(&x);
        if x.is_empty() {
            (lo, hi) = (0.0, 1.0);
        }
        if lo == hi {
            (lo, hi) = (lo - 0.5, hi + 0.5);
        }
        linspace(lo, hi, bins as usize + 1, true)
    };
    let nb = edges.len() - 1;
    let (lo, hi) = (edges[0], edges[nb]);
    let even = edges.len() > 2 && {
        let w = (hi - lo) / nb as f64;
        edges.iter().enumerate().all(|(i, e)| (e - (lo + w * i as f64)).abs() <= w * 1e-9)
    };
    let scale = nb as f64 / (hi - lo);
    let parts = par::map(x.len(), |r| {
        let mut counts = vec![0u32; nb];
        for &v in &x[r] {
            if !(v >= lo && v <= hi) {
                continue;
            }
            let mut b = if even { ((v - lo) * scale) as usize } else { edges.partition_point(|e| *e <= v).saturating_sub(1) };
            b = b.min(nb - 1);
            // Where rounding put it on the wrong side of an edge.
            if v < edges[b] {
                b -= 1;
            } else if b + 1 < nb && v >= edges[b + 1] {
                b += 1;
            }
            counts[b] += 1;
        }
        counts
    });
    let mut counts = vec![0i64; nb];
    for p in parts {
        for (c, k) in counts.iter_mut().zip(p) {
            *c += k as i64;
        }
    }
    let mut m = Map::new();
    m.insert("counts".into(), Value::list(counts.into_iter().map(Value::Int).collect()));
    m.insert("edges".into(), floats(edges));
    Ok(Value::map(m))
}

/// `stats.zscore(data)`: how many (population) deviations from the mean. A
/// constant list gives zeros.
fn s_zscore(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (x, _) = nums(&args, 0, "stats.zscore", line)?;
    if x.is_empty() {
        return Ok(Value::list(Vec::new()));
    }
    let m = mean_of(&x);
    let sd = div2(ss(&x, m), x.len() as f64).sqrt();
    Ok(pfloats(x.len(), |i| if sd == 0.0 { 0.0 } else { (x[i] - m) / sd }))
}

/// `stats.normalize(data)`: scaled to 0..1, the least value 0 and the
/// greatest 1. A constant list gives zeros.
fn s_normalize(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (x, _) = nums(&args, 0, "stats.normalize", line)?;
    let (lo, hi) = bounds(&x);
    let w = hi - lo;
    Ok(pfloats(x.len(), |i| if w > 0.0 { (x[i] - lo) / w } else { 0.0 }))
}

fn s_rank(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (x, _) = nums(&args, 0, "stats.rank", line)?;
    Ok(Value::list(ranks(&x).into_iter().map(whole).collect()))
}

fn normal(args: &[Value], f: &str, line: u32) -> Result<(f64, f64, f64), Ctrl> {
    let x = num(args, 0, f, line)?;
    let mu = num_or(args, 1, f, line, 0.0)?;
    let sigma = num_or(args, 2, f, line, 1.0)?;
    if !(sigma > 0.0) {
        return fail(line, format!("`{}`: sigma must be positive, got {}", f, sigma));
    }
    Ok((x, mu, sigma))
}

/// `stats.normal_pdf(x, mu = 0, sigma = 1)`.
fn s_normal_pdf(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (x, mu, sigma) = normal(&args, "stats.normal_pdf", line)?;
    let z = (x - mu) / sigma;
    Ok(Value::Float((-0.5 * z * z).exp() / (sigma * (std::f64::consts::TAU).sqrt())))
}

fn s_normal_cdf(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (x, mu, sigma) = normal(&args, "stats.normal_cdf", line)?;
    Ok(Value::Float(0.5 * (1.0 + erf((x - mu) / (sigma * std::f64::consts::SQRT_2)))))
}

/// `stats.normal_inv(p, mu = 0, sigma = 1)`: the x below which p of the
/// distribution lies (Wichura's AS241, as Python's NormalDist.inv_cdf).
fn s_normal_inv(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (p, mu, sigma) = normal(&args, "stats.normal_inv", line)?;
    if !(p > 0.0 && p < 1.0) {
        return fail(line, format!("`stats.normal_inv`: p must be between 0 and 1 (exclusive), got {}", p));
    }
    Ok(Value::Float(mu + inv_cdf(p) * sigma))
}

fn poly(c: &[f64], r: f64) -> f64 {
    c.iter().fold(0.0, |acc, k| acc * r + k)
}

fn inv_cdf(p: f64) -> f64 {
    let q = p - 0.5;
    if q.abs() <= 0.425 {
        let r = 0.180625 - q * q;
        let num = poly(
            &[2509.0809287301226727, 33430.575583588128105, 67265.770927008700853, 45921.953931549871457, 13731.693765509461125, 1971.5909503065514427, 133.14166789178437745, 3.387132872796366608],
            r,
        ) * q;
        let den = poly(&[5226.495278852545561, 28729.085735721942674, 39307.89580009271061, 21213.794301586595867, 5394.1960214247511077, 687.1870074920579083, 42.313330701600911252, 1.0], r);
        return num / den;
    }
    let mut r = if q <= 0.0 { p } else { 1.0 - p };
    r = (-r.ln()).sqrt();
    let (num, den) = if r <= 5.0 {
        r -= 1.6;
        (
            poly(&[7.7454501427834140764e-4, 0.0227238449892691845833, 0.24178072517745061177, 1.27045825245236838258, 3.64784832476320460504, 5.7694972214606914055, 4.6303378461565452959, 1.42343711074968357734], r),
            poly(&[1.05075007164441684324e-9, 5.475938084995344946e-4, 0.0151986665636164571966, 0.14810397642748007459, 0.68976733498510000455, 1.6763848301838038494, 2.05319162663775882187, 1.0], r),
        )
    } else {
        r -= 5.0;
        (
            poly(&[2.01033439929228813265e-7, 2.71155556874348757815e-5, 0.0012426609473880784386, 0.026532189526576123093, 0.29656057182850489123, 1.7848265399172913358, 5.4637849111641143699, 6.6579046435011037772], r),
            poly(&[2.04426310338993978564e-15, 1.4215117583164458887e-7, 1.8463183175100546818e-5, 7.868691311456132591e-4, 0.0148753612908506148525, 0.13692988092273580531, 0.59983220655588793769, 1.0], r),
        )
    };
    let x = num / den;
    if q < 0.0 {
        -x
    } else {
        x
    }
}

// ---- vec ---------------------------------------------------------------

const VEC: &[(&str, BuiltinFn)] = &[
    ("vec.add", v_add),
    ("vec.sub", v_sub),
    ("vec.mul", v_mul),
    ("vec.div", v_div),
    ("vec.mod", v_mod),
    ("vec.pow", v_pow),
    ("vec.dot", v_dot),
    ("vec.norm", v_norm),
    ("vec.cumsum", v_cumsum),
    ("vec.cumprod", v_cumprod),
    ("vec.diff", v_diff),
    ("vec.argmin", v_argmin),
    ("vec.argmax", v_argmax),
    ("vec.argsort", v_argsort),
    ("vec.clip", v_clip),
    ("vec.abs", v_abs),
    ("vec.sqrt", v_sqrt),
    ("vec.exp", v_exp),
    ("vec.log", v_log),
    ("vec.round", v_round),
    ("vec.linspace", v_linspace),
    ("vec.arange", v_arange),
    ("vec.zeros", v_zeros),
    ("vec.ones", v_ones),
    ("vec.full", v_full),
    ("vec.rolling_mean", v_rolling_mean),
    ("vec.rolling_sum", v_rolling_sum),
    ("vec.matmul", v_matmul),
    ("vec.transpose", v_transpose),
    ("vec.column", v_column),
];

const MOST: usize = 10_000_000;

#[derive(Clone, Copy, PartialEq)]
enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}

/// As the operators do: ints stay ints (`/` when it divides exactly), else floats.
fn arith(op: Op, a: &Value, b: &Value, f: &str, line: u32) -> R {
    match (a, b) {
        (Value::Float(x), Value::Float(y)) => Ok(Value::Float(float_op(op, *x, *y))),
        (Value::Int(x), Value::Int(y)) => {
            let (x, y) = (*x, *y);
            let r = match op {
                Op::Add => x.checked_add(y),
                Op::Sub => x.checked_sub(y),
                Op::Mul => x.checked_mul(y),
                Op::Mod => {
                    if y == 0 {
                        return fail(line, format!("`{}`: division by zero", f));
                    }
                    // (FEEDBACK16.md, Q-146; branch feedback20-fixes)
                    Some(x.wrapping_rem(y))
                }
                Op::Div => {
                    if y == 0 {
                        return fail(line, format!("`{}`: division by zero", f));
                    }
                    if x.checked_rem(y) != Some(0) {
                        return Ok(Value::Float(x as f64 / y as f64));
                    }
                    x.checked_div(y)
                }
            };
            r.map(Value::Int).ok_or_else(|| overflow(line))
        }
        _ => match (a.as_f64(), b.as_f64()) {
            (Some(x), Some(y)) => Ok(Value::Float(float_op(op, x, y))),
            _ => fail(line, format!("`{}` expects numbers, got {} and {}", f, a.type_name(), b.type_name())),
        },
    }
}

fn float_op(op: Op, x: f64, y: f64) -> f64 {
    match op {
        Op::Add => x + y,
        Op::Sub => x - y,
        Op::Mul => x * y,
        Op::Div => x / y,
        Op::Mod => x % y,
    }
}

/// Item by item: two lists of one length, or a list and a number.
fn elementwise(args: &[Value], op: Op, f: &str, line: u32) -> R {
    let a = args.get(0).unwrap_or(&Value::Nil);
    let b = args.get(1).unwrap_or(&Value::Nil);
    match (a, b) {
        (Value::List(p), Value::List(q)) => {
            if p.len() != q.len() {
                return fail(line, format!("`{}` needs lists of the same length ({} and {})", f, p.len(), q.len()));
            }
            each_pair(p.len(), |i| (&p[i], &q[i]), op, f, line)
        }
        (Value::List(p), y) => each_pair(p.len(), |i| (&p[i], y), op, f, line),
        (x, Value::List(q)) => each_pair(q.len(), |i| (x, &q[i]), op, f, line),
        (x, y) => arith(op, x, y, f, line),
    }
}

fn each_pair<'a>(n: usize, pair: impl Fn(usize) -> (&'a Value, &'a Value) + Sync, op: Op, f: &str, line: u32) -> R {
    ptry(n, |i| {
        let (x, y) = pair(i);
        Ok(match (x, y) {
            (Value::Float(a), Value::Float(b)) => Value::Float(float_op(op, *a, *b)),
            (Value::Float(a), Value::Int(b)) => Value::Float(float_op(op, *a, *b as f64)),
            _ => arith(op, x, y, f, line)?,
        })
    })
}

fn v_add(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    elementwise(&args, Op::Add, "vec.add", line)
}

fn v_sub(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    elementwise(&args, Op::Sub, "vec.sub", line)
}

fn v_mul(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    elementwise(&args, Op::Mul, "vec.mul", line)
}

fn v_div(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    elementwise(&args, Op::Div, "vec.div", line)
}

/// The remainder, as `%`: with the sign of the dividend.
fn v_mod(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    elementwise(&args, Op::Mod, "vec.mod", line)
}

fn v_pow(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let x = xs(&args, 0, "vec.pow", line)?;
    let p = num(&args, 1, "vec.pow", line)?;
    if p == 2.0 {
        return Ok(pfloats(x.len(), |i| x[i] * x[i]));
    }
    Ok(pfloats(x.len(), |i| x[i].powf(p)))
}

fn v_dot(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    dot(&args, "vec.dot", line)
}

/// `vec.norm(xs, p = 2)`: the length, or the sum of |x|^p to the 1/p.
fn v_norm(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let x = xs(&args, 0, "vec.norm", line)?;
    let p = num_or(&args, 1, "vec.norm", line, 2.0)?;
    Ok(Value::Float(if p == 2.0 {
        norm2(&x)
    } else if p == 1.0 {
        csum(x.iter().map(|v| v.abs()))
    } else if p.is_infinite() {
        largest(&x)
    } else {
        csum(x.iter().map(|v| v.abs().powf(p))).powf(1.0 / p)
    }))
}

fn accumulate(args: &[Value], op: Op, f: &str, line: u32) -> R {
    let l = list(args, 0, f, line)?;
    // All floats, or all ints: a running total in a register, the list made after.
    if l.iter().all(|v| matches!(v, Value::Float(_))) {
        let x = xs(args, 0, f, line)?;
        let mut t = Vec::with_capacity(x.len());
        let mut acc = x.first().copied().unwrap_or(0.0);
        for (i, v) in x.iter().enumerate() {
            if i > 0 {
                acc = float_op(op, acc, *v);
            }
            t.push(acc);
        }
        return Ok(pfloats(t.len(), |i| t[i]));
    }
    if l.iter().all(|v| matches!(v, Value::Int(_))) {
        let mut t = Vec::with_capacity(l.len());
        let mut acc = 0i64;
        for (i, v) in l.iter().enumerate() {
            let Value::Int(n) = v else { unreachable!() };
            acc = if i == 0 {
                *n
            } else {
                match op {
                    Op::Mul => acc.checked_mul(*n),
                    _ => acc.checked_add(*n),
                }
                .ok_or_else(|| overflow(line))?
            };
            t.push(acc);
        }
        return Ok(pvalues(t.len(), |i| Value::Int(t[i])));
    }
    let mut out: Vec<Value> = Vec::with_capacity(l.len());
    for x in l {
        let next = match (out.last(), x) {
            (None, Value::Int(_) | Value::Float(_)) => x.clone(),
            (None, other) => return fail(line, format!("`{}` expects numbers, got {}", f, other.type_name())),
            (Some(Value::Float(a)), Value::Float(b)) => Value::Float(float_op(op, *a, *b)),
            (Some(a), _) => arith(op, a, x, f, line)?,
        };
        out.push(next);
    }
    Ok(Value::list(out))
}

fn v_cumsum(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    accumulate(&args, Op::Add, "vec.cumsum", line)
}

fn v_cumprod(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    accumulate(&args, Op::Mul, "vec.cumprod", line)
}

fn v_diff(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let l = list(&args, 0, "vec.diff", line)?;
    l.windows(2).map(|w| arith(Op::Sub, &w[1], &w[0], "vec.diff", line)).collect::<Result<Vec<_>, _>>().map(Value::list)
}

fn arg_extreme(args: &[Value], f: &str, want: Ordering, line: u32) -> R {
    let x = xs(args, 0, f, line)?;
    let mut best: Option<usize> = None;
    for (i, v) in x.iter().enumerate() {
        if v.is_nan() {
            continue;
        }
        if best.map_or(true, |b| v.partial_cmp(&x[b]) == Some(want)) {
            best = Some(i);
        }
    }
    Ok(best.map_or(Value::Nil, |i| Value::Int(i as i64)))
}

fn v_argmin(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    arg_extreme(&args, "vec.argmin", Ordering::Less, line)
}

fn v_argmax(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    arg_extreme(&args, "vec.argmax", Ordering::Greater, line)
}

/// `vec.argsort(xs, descending = false)`: the indexes that would sort xs
/// (ties in their order).
fn v_argsort(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let x = xs(&args, 0, "vec.argsort", line)?;
    let desc = given(&args, 1).map_or(false, Value::truthy);
    let idx = order(&x, desc);
    Ok(pvalues(idx.len(), |i| Value::Int(idx[i] as i64)))
}

/// `vec.clip(xs, lo, hi)`: each kept within lo..hi (either may be null).
fn v_clip(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let l = list(&args, 0, "vec.clip", line)?;
    let lo = given(&args, 1).map(|_| num(&args, 1, "vec.clip", line)).transpose()?;
    let hi = given(&args, 2).map(|_| num(&args, 2, "vec.clip", line)).transpose()?;
    let mut out = Vec::with_capacity(l.len());
    for (k, v) in l.iter().enumerate() {
        let Some(x) = v.as_f64() else {
            return fail(line, format!("`vec.clip` expects a list of numbers, but item {} is {}", k, v.type_name()));
        };
        out.push(match (lo, hi) {
            (Some(a), _) if x < a => args[1].clone(),
            (_, Some(b)) if x > b => args[2].clone(),
            _ => v.clone(),
        });
    }
    Ok(Value::list(out))
}

fn each(args: &[Value], f: &str, line: u32, g: fn(f64) -> f64) -> R {
    let x = xs(args, 0, f, line)?;
    Ok(pfloats(x.len(), |i| g(x[i])))
}

fn v_abs(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (x, ints) = nums(&args, 0, "vec.abs", line)?;
    Ok(pvalues(x.len(), |i| back(x[i].abs(), ints)))
}

fn v_sqrt(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    each(&args, "vec.sqrt", line, f64::sqrt)
}

fn v_exp(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    each(&args, "vec.exp", line, f64::exp)
}

fn v_log(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    each(&args, "vec.log", line, f64::ln)
}

/// `vec.round(xs)` to ints, or `vec.round(xs, digits)` (halves away from zero, as `round`).
fn v_round(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let x = xs(&args, 0, "vec.round", line)?;
    if given(&args, 1).is_none() {
        return Ok(pvalues(x.len(), |i| whole(x[i].round())));
    }
    let p = 10f64.powi(int(&args, 1, "vec.round", line)?.clamp(0, 15) as i32);
    Ok(pfloats(x.len(), |i| (x[i] * p).round() / p))
}

fn linspace(a: f64, b: f64, n: usize, endpoint: bool) -> Vec<f64> {
    if n == 0 {
        return Vec::new();
    }
    let div = if endpoint { n.saturating_sub(1).max(1) } else { n } as f64;
    let step = (b - a) / div;
    let mut out: Vec<f64> = (0..n).map(|i| a + i as f64 * step).collect();
    if endpoint && n > 1 {
        out[n - 1] = b;
    }
    out
}

fn count(n: i64, f: &str, line: u32) -> Result<usize, Ctrl> {
    if n < 0 || n as usize > MOST {
        return fail(line, format!("`{}`: {} is not a count from 0 to {}", f, n, MOST));
    }
    Ok(n as usize)
}

/// `vec.linspace(start, stop, n = 50, endpoint = true)`: n evenly spaced numbers.
fn v_linspace(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let f = "vec.linspace";
    let (a, b) = (num(&args, 0, f, line)?, num(&args, 1, f, line)?);
    let n = count(int_or(&args, 2, f, line, 50)?, f, line)?;
    let endpoint = given(&args, 3).map_or(true, Value::truthy);
    Ok(floats(linspace(a, b, n, endpoint)))
}

/// `vec.arange(stop)`, `vec.arange(start, stop, step = 1)`: from start, by
/// step, up to (not including) stop. Ints when all three are.
fn v_arange(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let f = "vec.arange";
    let (start, stop) = if given(&args, 1).is_some() { (arg_num(&args, 0, f, line)?, arg_num(&args, 1, f, line)?) } else { (Value::Int(0), arg_num(&args, 0, f, line)?) };
    let step = if given(&args, 2).is_some() { arg_num(&args, 2, f, line)? } else { Value::Int(1) };
    let (a, b, s) = (start.as_f64().unwrap(), stop.as_f64().unwrap(), step.as_f64().unwrap());
    if s == 0.0 || !s.is_finite() {
        return fail(line, format!("`{}`: step must be a non-zero number", f));
    }
    let n = ((b - a) / s).ceil().max(0.0);
    let n = count(if n.is_finite() { n as i64 } else { -1 }, f, line)?;
    if let (Value::Int(a), Value::Int(s)) = (&start, &step) {
        return Ok(Value::list((0..n as i64).map(|i| Value::Int(a + i * s)).collect()));
    }
    Ok(floats((0..n).map(|i| a + i as f64 * s)))
}

fn arg_num(args: &[Value], i: usize, f: &str, line: u32) -> R {
    match args.get(i) {
        Some(v @ (Value::Int(_) | Value::Float(_))) => Ok(v.clone()),
        other => fail(line, format!("`{}` expects a number as argument {}, got {}", f, i + 1, tn(other))),
    }
}

fn v_zeros(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let n = count(int(&args, 0, "vec.zeros", line)?, "vec.zeros", line)?;
    Ok(Value::list(vec![Value::Int(0); n]))
}

fn v_ones(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let n = count(int(&args, 0, "vec.ones", line)?, "vec.ones", line)?;
    Ok(Value::list(vec![Value::Int(1); n]))
}

fn v_full(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let n = count(int(&args, 0, "vec.full", line)?, "vec.full", line)?;
    Ok(Value::list(vec![args.get(1).cloned().unwrap_or(Value::Nil); n]))
}

/// Sums of each run of `window` neighbours: n - window + 1 of them.
fn rolling(args: &[Value], f: &str, line: u32) -> Result<(Vec<f64>, usize), Ctrl> {
    let x = xs(args, 0, f, line)?;
    let w = int(args, 1, f, line)?;
    if w < 1 {
        return fail(line, format!("`{}`: the window must be at least 1, got {}", f, w));
    }
    let w = w as usize;
    if w > x.len() {
        return Ok((Vec::new(), w));
    }
    // Window j is x[j..j + w]. A running sum, compensated, started again at
    // every 4096th window (and each chunk's first) so that what was
    // subtracted leaves no error behind; the chunks run on all the cores.
    let mut out = vec![0.0f64; x.len() - w + 1];
    par::fill(&mut out, |start, part| {
        let mut a = Acc::default();
        for (k, slot) in part.iter_mut().enumerate() {
            let j = start + k;
            if k == 0 || j % 4096 == 0 {
                a = Acc::default();
                for u in &x[j..j + w] {
                    a.add(*u);
                }
            } else {
                a.add(-x[j - 1]);
                a.add(x[j + w - 1]);
            }
            *slot = a.s + a.c;
        }
    });
    Ok((out, w))
}

/// `vec.rolling_mean(xs, window)`: the mean of each window of neighbours.
fn v_rolling_mean(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (s, w) = rolling(&args, "vec.rolling_mean", line)?;
    Ok(pfloats(s.len(), |i| s[i] / w as f64))
}

fn v_rolling_sum(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (s, _) = rolling(&args, "vec.rolling_sum", line)?;
    Ok(pfloats(s.len(), |i| s[i]))
}

/// Rows of numbers, all of one length.
fn matrix(args: &[Value], i: usize, f: &str, line: u32) -> Result<(Vec<f64>, usize, usize), Ctrl> {
    let rows = list(args, i, f, line)?;
    let cols = match rows.first() {
        Some(Value::List(r)) => r.len(),
        Some(other) => return fail(line, format!("`{}` expects a list of rows, but row 0 is {}", f, other.type_name())),
        None => 0,
    };
    let mut out = Vec::with_capacity(rows.len() * cols);
    for (k, r) in rows.iter().enumerate() {
        let Value::List(r) = r else {
            return fail(line, format!("`{}` expects a list of rows, but row {} is {}", f, k, r.type_name()));
        };
        if r.len() != cols {
            return fail(line, format!("`{}`: row {} has {} numbers, row 0 has {}", f, k, r.len(), cols));
        }
        for v in r.iter() {
            match v.as_f64() {
                Some(x) => out.push(x),
                None => return fail(line, format!("`{}` expects numbers, but row {} holds {}", f, k, v.type_name())),
            }
        }
    }
    Ok((out, rows.len(), cols))
}

/// `vec.matmul(a, b)`: a matrix (a list of rows) times a matrix, or times a
/// list of numbers (a column), which gives a list.
fn v_matmul(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let f = "vec.matmul";
    let (a, n, k) = matrix(&args, 0, f, line)?;
    let column = matches!(args.get(1), Some(Value::List(l)) if l.first().map_or(false, |v| !matches!(v, Value::List(_))));
    if column {
        let b = xs(&args, 1, f, line)?;
        if b.len() != k {
            return fail(line, format!("`{}`: a has {} columns, b has {} numbers", f, k, b.len()));
        }
        return Ok(floats((0..n).map(|i| cdot(&a[i * k..(i + 1) * k], &b))));
    }
    let (b, k2, m) = matrix(&args, 1, f, line)?;
    if k != k2 {
        return fail(line, format!("`{}`: a has {} columns, b has {} rows", f, k, k2));
    }
    let mut c = vec![0.0f64; n * m];
    for i in 0..n {
        let row = &mut c[i * m..(i + 1) * m];
        for p in 0..k {
            let x = a[i * k + p];
            for (out, y) in row.iter_mut().zip(&b[p * m..(p + 1) * m]) {
                *out += x * y;
            }
        }
    }
    Ok(Value::list(c.chunks(m.max(1)).take(n).map(|r| floats(r.iter().copied())).collect()))
}

fn v_transpose(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let rows = list(&args, 0, "vec.transpose", line)?;
    let mut cols: Vec<Vec<Value>> = Vec::new();
    for (k, r) in rows.iter().enumerate() {
        let Value::List(r) = r else {
            return fail(line, format!("`vec.transpose` expects a list of rows, but row {} is {}", k, r.type_name()));
        };
        if k == 0 {
            cols = (0..r.len()).map(|_| Vec::with_capacity(rows.len())).collect();
        } else if r.len() != cols.len() {
            return fail(line, format!("`vec.transpose`: row {} has {} items, row 0 has {}", k, r.len(), cols.len()));
        }
        for (c, v) in cols.iter_mut().zip(r.iter()) {
            c.push(v.clone());
        }
    }
    Ok(Value::list(cols.into_iter().map(Value::list).collect()))
}

/// `vec.column(rows, key)`: each row's field (rows are maps) or item (rows
/// are lists, the key an int); null where a row has none.
fn v_column(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let rows = list(&args, 0, "vec.column", line)?;
    let out = match args.get(1) {
        Some(Value::Str(k)) => {
            let key = Key::new(k);
            rows.iter()
                .map(|r| match r {
                    Value::Map(m) => m.get(&key).cloned().unwrap_or(Value::Nil),
                    _ => Value::Nil,
                })
                .collect()
        }
        Some(Value::Int(i)) => rows
            .iter()
            .map(|r| match r {
                Value::List(l) => usize::try_from(*i).ok().and_then(|i| l.get(i)).cloned().unwrap_or(Value::Nil),
                _ => Value::Nil,
            })
            .collect(),
        other => return fail(line, format!("`vec.column` expects a str or int key as argument 2, got {}", tn(other))),
    };
    Ok(Value::list(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lists long enough for the pools and the ranges: random, with many
    /// repeats, negative and positive zeros, infinities, and clustered values.
    fn lists() -> Vec<Vec<f64>> {
        let mut r = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move || {
            r ^= r << 13;
            r ^= r >> 7;
            r ^= r << 17;
            r
        };
        let n = 300_001;
        vec![
            (0..n).map(|_| (next() % 1_000_000) as f64 / 7.0 - 50_000.0).collect(),
            (0..n).map(|_| (next() % 10) as f64).collect(),
            (0..n).map(|i| match i % 7 { 0 => 0.0, 1 => -0.0, 2 => f64::INFINITY, 3 => f64::NEG_INFINITY, _ => (next() % 100) as f64 - 50.0 }).collect(),
            (0..n).map(|_| 10.0 + (next() % 1000) as f64 * 1e-12).collect(),
            (0..n).map(|_| f64::from_bits(next() >> 2)).collect(),
            vec![3.5; n],
        ]
    }

    #[test]
    fn ranks_are_the_sorted_list_s() {
        for x in lists() {
            let mut d = x.clone();
            d.sort_by(f64::total_cmp);
            let n = x.len();
            let at = [0, 1, n / 4, n / 2 - 1, n / 2, n / 2 + 1, 3 * n / 4, n - 2, n - 1];
            let got = values_at(&x, &at);
            for (i, v) in at.iter().zip(got) {
                assert_eq!(d[*i].to_bits(), v.to_bits(), "position {}", i);
            }
            let s = sorted(x.clone());
            assert!(s.iter().zip(&d).all(|(a, b)| a.to_bits() == b.to_bits()));
        }
    }

    #[test]
    fn orders_keep_equal_items_in_theirs() {
        for x in lists() {
            let mut want: Vec<u32> = (0..x.len() as u32).collect();
            want.sort_by(|a, b| (x[*a as usize] + 0.0).partial_cmp(&(x[*b as usize] + 0.0)).unwrap_or_else(|| (x[*a as usize] + 0.0).total_cmp(&(x[*b as usize] + 0.0))));
            assert_eq!(order(&x, false), want);
            let mut down: Vec<u32> = (0..x.len() as u32).collect();
            down.sort_by(|a, b| (x[*b as usize] + 0.0).total_cmp(&(x[*a as usize] + 0.0)));
            assert_eq!(order(&x, true), down);
        }
    }

    #[test]
    fn sums_are_as_exact_as_alone() {
        for x in lists().into_iter().filter(|x| x.iter().all(|v| v.is_finite() && v.abs() < 1e300)) {
            let (s, c) = psum(&x, |v| v);
            assert_eq!(s + c, fsum(x.iter().copied()));
            assert_eq!(pfsum(&x).to_bits(), fsum(x.iter().copied()).to_bits());
            let (d, e) = cdot2(&x, &x);
            let exact = fsum(x.iter().flat_map(|v| {
                let p = v * v;
                [p, v.mul_add(*v, -p)]
            }));
            assert!(((d + e) - exact).abs() <= exact.abs() * 1e-15, "{} {}", d + e, exact);
        }
    }

    #[test]
    fn exact_sums_of_infinities_are_python_s() {
        let mut x = vec![1.0; 300_000];
        x[5] = f64::INFINITY;
        assert_eq!(pfsum(&x), f64::INFINITY);
        x[250_000] = f64::NEG_INFINITY;
        assert!(pfsum(&x).is_nan());
        let y: Vec<f64> = (0..300_000).map(|i| if i % 2 == 0 { 1e100 } else { -1e100 }).chain([0.1, 0.2, 0.3]).collect();
        assert_eq!(pfsum(&y), 0.6);
    }

    #[test]
    fn builtin_sort_of_numbers_is_the_general_one() {
        for x in lists() {
            let l: Vec<Value> = x.iter().map(|v| Value::Float(*v)).collect();
            let got = sort_numbers(&l).unwrap();
            let mut want = l.clone();
            want.sort_by(|a, b| compare(a, b).unwrap());
            let Value::List(got) = got else { panic!() };
            assert!(got.iter().zip(&want).all(|(a, b)| a.as_f64().unwrap().to_bits() == b.as_f64().unwrap().to_bits()));
        }
        let ints: Vec<Value> = (0..200_000i64).map(|i| Value::Int((i * 7919) % 100_003 - 50_000)).collect();
        let Value::List(got) = sort_numbers(&ints).unwrap() else { panic!() };
        let mut want: Vec<i64> = ints.iter().map(|v| if let Value::Int(n) = v { *n } else { 0 }).collect();
        want.sort();
        assert!(got.iter().zip(&want).all(|(a, b)| matches!(a, Value::Int(n) if n == b)));
    }
}
