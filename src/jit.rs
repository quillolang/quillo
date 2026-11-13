//! Numeric functions compiled to machine code.
//!
//! A top-level function whose body only does arithmetic on ints, floats and
//! bools (with `let`, assignment, `if`, `for` over a range, `while`, `return`, and
//! calls of other such functions) is compiled with Cranelift once it is
//! called a second time, for the types of the arguments it was called with.
//! It then runs as the interpreter would run it, only faster. Where the
//! interpreter would fail (an int overflowing, `%` by zero, comparing NaN,
//! too deep a recursion), the machine code gives up instead, and the call is
//! run again by the interpreter, which fails as it always has: the functions
//! compiled change nothing but their result, so running one twice is the
//! same as running it once.
//!
//! What cannot be compiled (anything else: strings, lists, builtins, `/` of
//! two ints, whose answer is an int or a float depending on the numbers, a
//! variable that changes type) is interpreted as before.

use crate::ast::*;
use crate::interp::{Ctrl, MAX_DEPTH};
use crate::value::{lookup, lookup_global, Closure, Env, Value};
use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{types, AbiParam, Block, InstBuilder, MemFlagsData, Type, UserFuncName, Value as V};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{default_libcall_names, FuncId, Module};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;

/// A value's type in compiled code.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum T {
    I,
    F,
    B,
}

impl T {
    pub(crate) fn of(v: &Value) -> Option<T> {
        match v {
            Value::Int(_) => Some(T::I),
            Value::Float(_) => Some(T::F),
            Value::Bool(_) => Some(T::B),
            _ => None,
        }
    }

    pub(crate) fn ir(self) -> Type {
        match self {
            T::I => types::I64,
            T::F => types::F64,
            T::B => types::I8,
        }
    }
}

/// What compiled code shares with its caller: whether it gave up, and how
/// deep its calls may go.
#[repr(C)]
struct Ctx {
    bail: u64,
    depth: u64,
    max: u64,
}

type Entry = unsafe extern "C" fn(*const u64, *mut Ctx) -> u64;

/// A function compiled for some argument types.
struct Compiled {
    entry: Entry,
    ret: T,
    /// The global functions it calls (directly or not), by name: if one is
    /// assigned something else, it is compiled no more.
    deps: Vec<(String, usize)>,
    /// Its own code, which a function compiled after it calls. Such a caller was planned with
    /// its type and then refused, as its code was not in that compile's: a caller of a function
    /// called on its own first was never compiled (FEEDBACK52.md, Q-677 review; branch
    /// feedback52).
    id: FuncId,
    /// The stack counted for each call of it or of what it calls (`LEVEL` or more): the largest
    /// frame among the functions on a cycle of calls, the only ones a chain of calls can hold
    /// more than once.
    per: usize,
    /// Beyond `per` a call, what the functions on no cycle take more, each at most once on a
    /// chain (`compile`).
    excess: usize,
    /// A call of it was given up for the stack left: run on `BIG` straight away when the stack
    /// left would give it up again (`run`).
    deep: std::sync::atomic::AtomicBool,
}

impl Compiled {
    /// The stack kept beyond `per` a call: `excess`, and `RESERVE` where a frame is counted at its
    /// own size, with no room to spare as `LEVEL` has. A function keeping 8,000 floats, its frame
    /// counted exactly in `excess`, overflowed by its entry (Q-947 review 7).
    fn reserve(&self) -> usize {
        self.excess + if self.per > LEVEL || self.excess > 0 { RESERVE } else { 0 }
    }
}

type Key = (usize, Vec<T>);

/// The stack counted for a compiled call at least, as for all calls before Q-947. A function with
/// many float `let`s still needed after its own call takes more: with 57,
/// `parallel(() => big(9000, 1.5), () => 1)` overflowed the child's stack and stopped the process,
/// where the interpreter gives its value; with 34 a route's `parallel` child (512 KB) killed the
/// server (SIGBUS); with 31 a runaway `big(-1, 1.5)` stopped it. Each function's own frame is now
/// Cranelift's (`Compiled::per`, `Compiled::excess`; FEEDBACK52.md, Q-947; branch feedback52).
const LEVEL: usize = 512;

/// Stack kept beyond the calls' frames where they are counted at their own size: the entry, and
/// the deepest call, which gives up there. With none, 8 of 600 recursions with large frames in
/// `parallel` children overflowed by a few KB. Not for `LEVEL`, which leaves room enough: kept
/// there too, a recursion of small frames in a 512 KB child was given up for the stack 32 calls
/// sooner, 31-34 ms against main's 8.9 (Q-947 review 7).
const RESERVE: usize = 16 << 10;

/// `FnDef::hot` once a function is known not to compile.
const NEVER: u32 = u32::MAX;

struct Jit {
    module: JITModule,
    done: HashMap<Key, Option<Arc<Compiled>>>,
    /// Functions whose code is made (so their addresses stay theirs).
    keep: Vec<Arc<FnDef>>,
    /// Functions whose plan met the budget (`PLAN_BASE`), or met one that did: never planned
    /// again (`compile`), and kept in `keep`.
    over: std::collections::HashSet<usize>,
}

thread_local! {
    static JIT: RefCell<Option<Jit>> = const { RefCell::new(None) };
    static LAST: std::cell::Cell<Option<Quick>> = const { std::cell::Cell::new(None) };
    /// The depth of a call whose machine code went too deep (`run`): every call deeper than it is
    /// the interpreter's, until a call no deeper (`under_floor`).
    static FLOOR: Floor = const { Floor(std::cell::Cell::new(NO_FLOOR)) };
}

/// How many threads have a `FLOOR`, and `UNREAD` and `OFF`: while it is 0, a call reads nothing
/// more than this. A recursion past the call limit gave up in machine code at the limit, the
/// interpreter went on with that call, and each call under it went back into machine code and
/// down to the limit again: 5·10⁷ calls where the interpreter makes 10⁴, up to 47 times the
/// interpreter's time (`fn fact(n) { if n == 0 { return 1 }; return n * fact(n - 1) }` and
/// `fact(-1)`), where main interpreted a function with `return` (FEEDBACK52.md, Q-945; branch
/// feedback52).
static FLOORS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(UNREAD);

/// In `FLOORS` until the first call has read `QUILLO_JIT` (`enabled`), in `under_floor`, so that
/// `FLOORS` is the only thing a call reads first, as before Q-945 `enabled` was (the floor's own
/// test cost 4 instructions a call, 0.8% of a function mapped over a list).
const UNREAD: usize = 1 << (usize::BITS - 1);
/// In `FLOORS` for good when `QUILLO_JIT` is `0`: a call is the interpreter's at once. Read in
/// `under_floor`, as `UNREAD` was, it cost each call of an interpreted program 34 instructions,
/// 1.2% of 5 million calls (Q-945 review 5).
const OFF: usize = 1 << (usize::BITS - 2);

const NO_FLOOR: usize = usize::MAX;

/// A thread's `FLOOR`: a thread that ends with one set takes itself out of `FLOORS`.
struct Floor(std::cell::Cell<usize>);

impl Drop for Floor {
    fn drop(&mut self) {
        if self.0.get() != NO_FLOOR {
            FLOORS.fetch_sub(1, Relaxed);
        }
    }
}

/// Whether a call at `depth` is under this thread's floor, and so the interpreter's; a call no
/// deeper takes the floor away. That is only a backup: the floor ends with the call that went too
/// deep (`Floored`). `call1` comes here only for a number, and a component never does, so after
/// `d(100000) ?? "E"`, `map(["a", ...], (s) => fib(27))` stayed interpreted, 479 ms against main's
/// 8 ms, and in a server every such route until one called a function itself (FEEDBACK52.md,
/// Q-945 review 5; branch feedback52).
#[cold]
#[inline(never)]
fn under_floor(depth: usize) -> bool {
    // One load: a thread that saw `UNREAD` in `call`'s and here finds it gone has another thread's
    // `OFF` (set before `UNREAD` goes), and must not go on to machine code; reading the two in two
    // loads, it skipped `enabled` and ran one call in machine code under QUILLO_JIT=0
    // (FEEDBACK52.md, Q-945 review 6; branch feedback52).
    let gate = FLOORS.load(Relaxed);
    if gate & UNREAD != 0 && enabled() {
        FLOORS.fetch_and(!UNREAD, Relaxed);
    } else if gate & (UNREAD | OFF) != 0 {
        // Machine code is off. OFF before UNREAD goes: `FLOORS` is never 0 with it off.
        FLOORS.fetch_or(OFF, Relaxed);
        FLOORS.fetch_and(!UNREAD, Relaxed);
        return true;
    }
    FLOOR
        .try_with(|f| {
            let at = f.0.get();
            if at == NO_FLOOR {
                return false;
            }
            if depth > at {
                return true;
            }
            f.0.set(NO_FLOOR);
            FLOORS.fetch_sub(1, Relaxed);
            false
        })
        .unwrap_or(false)
}

/// For tests: this thread's floor at `depth`, as if a call there had gone too deep and its end had
/// not taken the floor away (`Floored`), which no program does now: how a test reaches the backup,
/// `under_floor`'s clearing, and a thread's end with a floor (`Floor`); read by no call
/// (FEEDBACK52.md, Q-945 review 5; branch feedback52).
pub fn leave_floor_for_tests(depth: usize) {
    FLOOR.with(|f| {
        if f.0.replace(depth) == NO_FLOOR {
            FLOORS.fetch_add(1, Relaxed);
        }
    });
}

/// The machine code of a call at `depth` gave up (`run`). Too deep, when no frame gave the depth
/// back and the one that could go no deeper left it at `max` (one that gave up otherwise at
/// exactly `max` is taken as too deep too, which only costs time): the floor is that depth,
/// fixed, as the interpreter runs the call again, and every call under it is the interpreter's.
/// Following the interpreter down instead, a call of another function at the same depth
/// (`r(d - 1, h(x))`) or of the same one (`r(d - 1, r(0, x) + 1)`) took it back each time (Q-677
/// reviews 2 and 3).
#[cold]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn gave_up(ctx: &Ctx, entry: Entry, ret: T, raw: &[u64; 16], depth: usize, frame: usize, reserve: usize, deep: Option<&std::sync::atomic::AtomicBool>) -> Ran {
    if ctx.depth != ctx.max {
        return Ran::Interpret;
    }
    // The stack left, not the call limit, made `max` (`parallel`'s 512 KB children, or under a
    // deep interpreted recursion): taken as too deep, `r(2000)` in `parallel` was interpreted
    // whole, 268-332 ms against main's 5-7 ms; and left to the interpreter with no floor, as
    // main, a runaway `fact(-1)` there went down in machine code again at each level, 120 ms
    // against the interpreter's 10-23 ms (Q-945 review 5). So the call runs once more, on a stack
    // with room for the call limit: it ends there, or gives up at the limit, and has its floor.
    // Only where the stack left is known, as it is on the new stack, so that this runs once. The
    // stack it needs is `frame` a call (Q-947).
    let limit = MAX_DEPTH.saturating_sub(depth);
    if (ctx.max as usize) < limit && crate::stack::remaining().is_some() {
        if let Some(deep) = deep {
            deep.store(true, Relaxed);
        }
        return floor_unless_done(on_big(entry, ret, raw, depth, frame, reserve), depth);
    }
    floor(depth)
}

/// `again`, a call at `depth` run on `BIG`; given up there other than at the limit (`3 * p(n - 1)`
/// overflowing at the bottom), a floor for it. With none, the interpreter took one level, and the
/// next was given up for the stack and run again on a new stack, a whole rerun and an mmap a
/// level: `p(9000)` in `parallel` 372 ms, main 155, the interpreter 4.4 (Q-945 review 6). As
/// c10c0f1 kept for any give-up at `max`.
fn floor_unless_done(again: Ran, depth: usize) -> Ran {
    match again {
        Ran::Interpret => floor(depth),
        again => again,
    }
}

/// The floor at `depth`, for the interpreter running that call (`Ran::Floored`).
fn floor(depth: usize) -> Ran {
    LAST.set(None);
    // A call that reached machine code was at the floor or above it, and took it away
    // (`under_floor`): there is none to keep lower.
    let _ = FLOOR.try_with(|f| {
        if f.0.replace(depth) == NO_FLOOR {
            FLOORS.fetch_add(1, Relaxed);
        }
    });
    Ran::Floored
}

/// `run` on a stack with room for the call limit at `frame` a call: this thread's `BIG`, made once
/// and kept, so that a call given up for the stack at each call of a loop maps and touches no new
/// stack each time. A new one each time, 5 MB for `f(9000)` in a `parallel` child, faulted 18
/// pages a call: 90,895 page reclaims against main's 899, 2.5 times its instructions (Q-947 review
/// 7). In use (a call under it, which no machine code makes), a new one.
#[cold]
#[inline(never)]
fn on_big(entry: Entry, ret: T, raw: &[u64; 16], depth: usize, frame: usize, reserve: usize) -> Ran {
    let need = MAX_DEPTH.saturating_sub(depth) * frame + reserve + crate::stack::MARGIN;
    // The new stack's `max` is the limit's, so this does not come back here.
    let again = || run(entry, ret, raw, depth, frame, reserve, None);
    #[cfg(not(windows))]
    {
        use corosensei::stack::{DefaultStack, Stack};
        let kept = BIG.with(|b| b.borrow_mut().take()).filter(|s| s.base().get() - crate::stack::limit_of(s) >= need);
        // As `crate::stack` makes one.
        let stack = kept.unwrap_or_else(|| DefaultStack::new(need + (256 << 10) + crate::stack::MARGIN).expect("cannot allocate a stack"));
        let (low, high) = (crate::stack::limit_of(&stack), stack.base().get());
        let outer = crate::stack::switch(low);
        let mut out = None;
        // SAFETY: `low..high` is memory of `stack`, which outlives the call.
        unsafe {
            psm::on_stack(low as *mut u8, high - low, || out = Some(std::panic::catch_unwind(std::panic::AssertUnwindSafe(again))));
        }
        crate::stack::switch(outer);
        BIG.with(|b| *b.borrow_mut() = Some(stack));
        out.expect("ran").unwrap_or_else(|p| std::panic::resume_unwind(p))
    }
    // Windows grows a stack with fibers (`crate::stack`): a new one each time.
    #[cfg(windows)]
    crate::stack::maybe_grow(need, need + (256 << 10), again)
}

#[cfg(not(windows))]
thread_local! {
    /// The stack `on_big` runs on, kept for the next.
    static BIG: RefCell<Option<corosensei::stack::DefaultStack>> = const { RefCell::new(None) };
}

/// What `call` did with a call.
pub enum Ran {
    /// Machine code gave this.
    Done(Result<Value, Ctrl>),
    /// The interpreter's.
    Interpret,
    /// The interpreter's, and this call's machine code went too deep and set the floor, which ends
    /// with the interpreted call (`Floored`). Said by `call` itself: a guard kept on each
    /// interpreted call cost it 10 to 12 instructions, 1% of a function mapped over 3 million
    /// numbers given a text (Q-945 review 6).
    Floored,
}

/// The call at `depth` the interpreter runs after its machine code went too deep (`Ran::Floored`),
/// while it lasts: the floor it set ends with it, on every way out (its value, an error passed on,
/// `?`, an unwind), not at the next call `call` sees (`under_floor`), which may be long after or
/// never (Q-945 review 5).
pub struct Floored(usize);

impl Floored {
    pub fn new(depth: usize) -> Floored {
        Floored(depth)
    }
}

impl Drop for Floored {
    fn drop(&mut self) {
        let _ = FLOOR.try_with(|f| {
            if f.0.get() == self.0 {
                f.0.set(NO_FLOOR);
                FLOORS.fetch_sub(1, Relaxed);
            }
        });
    }
}

/// The function called last, if what it calls cannot change under it.
#[derive(Clone, Copy)]
struct Quick {
    def: usize,
    code: u64,
    entry: Entry,
    ret: T,
}

/// How many functions this thread has compiled (for tests).
pub fn compiled() -> usize {
    JIT.with(|j| j.borrow().as_ref().map_or(0, |j| j.done.values().filter(|c| c.is_some()).count()))
}

thread_local! {
    /// The function `refuse_for_tests` names, and whether Cranelift is the one to refuse it.
    static REFUSE: RefCell<(String, bool)> = const { RefCell::new((String::new(), false)) };
}

/// For tests: this thread's compiles from now on refuse the function named `name` once it is
/// planned, in Gen or, with `define`, in Cranelift (its returns made unlike its signature's);
/// `""` for none. No program reaches what a compile refused part way then does (`give_up`), so
/// this is how a test does; it is read once a compile, never by a call (FEEDBACK52.md, Q-677
/// review; branch feedback52).
pub fn refuse_for_tests(name: &str, define: bool) {
    REFUSE.with(|r| *r.borrow_mut() = (name.to_string(), define));
}

/// `f` with this thread's module, made if it is the first: for loops' code (src/jitloop.rs).
pub(crate) fn with_module<R>(f: impl FnOnce(&mut JITModule) -> Option<R>) -> Option<R> {
    JIT.with(|j| {
        let mut j = j.borrow_mut();
        if j.is_none() {
            *j = new_jit();
        }
        f(&mut j.as_mut()?.module)
    })
}

fn new_jit() -> Option<Jit> {
    let mut flags = settings::builder();
    flags.set("use_colocated_libcalls", "false").ok()?;
    flags.set("is_pic", "false").ok()?;
    flags.set("opt_level", "speed").ok()?;
    // A frame over the 4 KB guard page touches each page as it is made, so that it cannot step
    // past a thread's guard: inline, and none in a frame under 4 KB (Q-947 review 7).
    flags.set("enable_probestack", "true").ok()?;
    flags.set("probestack_strategy", "inline").ok()?;
    let isa = cranelift_native::builder().ok()?.finish(settings::Flags::new(flags)).ok()?;
    Some(Jit { module: JITModule::new(JITBuilder::with_isa(isa, default_libcall_names())), done: HashMap::new(), keep: Vec::new(), over: Default::default() })
}

/// `c(args)` in machine code, when it can be: None to interpret it.
/// An argument as a call has it: given, or left to its default.
pub trait Arg {
    fn given(&self) -> Option<&Value>;
}

impl Arg for Option<Value> {
    #[inline(always)]
    fn given(&self) -> Option<&Value> {
        self.as_ref()
    }
}

impl Arg for Value {
    #[inline(always)]
    fn given(&self) -> Option<&Value> {
        Some(self)
    }
}

/// Whether machine code is made at all: `QUILLO_JIT=0` runs everything in the
/// interpreter, to tell the two apart (FEEDBACK29.md; branch feedback29-fixes).
pub fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("QUILLO_JIT").map_or(true, |v| v != "0"))
}

pub fn call<A: Arg>(c: &Closure, args: &[A], depth: usize) -> Ran {
    // Before anything else, for every function, those never compiled too (`FLOORS`); and
    // whether machine code is made at all.
    let gate = FLOORS.load(Relaxed);
    if gate != 0 && (gate & OFF != 0 || under_floor(depth)) {
        return Ran::Interpret;
    }
    let def = &c.def;
    let hot = def.hot.load(Relaxed);
    if hot == NEVER {
        return Ran::Interpret;
    }
    // Compiled on the second call, as one call alone is not worth it, unless that one
    // call has a loop to spend its time in (FEEDBACK29.md; branch feedback29-fixes).
    if hot < 1 {
        if !shape_ok(def) {
            def.hot.store(NEVER, Relaxed);
            return Ran::Interpret;
        }
        def.hot.store(hot + 1, Relaxed);
        if !has_loop(&def.body) {
            return Ran::Interpret;
        }
    }
    if args.len() != def.params.len() || args.len() > 16 {
        def.hot.store(NEVER, Relaxed);
        return Ran::Interpret;
    }
    let mut code = args.len() as u64;
    let mut raw = [0u64; 16];
    for (i, a) in args.iter().enumerate() {
        let Some(given) = a.given() else { return Ran::Interpret };
        let (t, bits) = match given {
            Value::Int(n) => (0, *n as u64),
            Value::Float(f) => (1, f.to_bits()),
            Value::Bool(b) => (2, *b as u64),
            _ => return Ran::Interpret,
        };
        code = code << 2 | t;
        raw[i] = bits;
    }
    let at = Arc::as_ptr(def) as usize;
    // A function given to `map` or `filter` is called for each item: the last one
    // called is at hand, with no table to look in.
    if let Some(q) = LAST.get().filter(|q| q.def == at && q.code == code) {
        // It calls nothing, so one frame is all it takes.
        return run(q.entry, q.ret, &raw, depth, LEVEL, 0, None);
    }
    let compiled = JIT.with(|j| {
        let mut j = j.borrow_mut();
        if j.is_none() {
            *j = new_jit();
        }
        let jit = j.as_mut()?;
        let sig: Vec<T> = args.iter().filter_map(|a| T::of(a.given()?)).collect();
        let key = (at, sig.clone());
        if let Some(done) = jit.done.get(&key) {
            return done.clone();
        }
        let made = compile(jit, c, &sig).map(Arc::new);
        if made.is_none() && !shape_ok(def) {
            def.hot.store(NEVER, Relaxed);
        }
        jit.done.insert(key, made.clone());
        made
    });
    let Some(compiled) = compiled else { return Ran::Interpret };
    // The last function's quick call counts `LEVEL` and keeps nothing: only for one whose frame
    // is no larger. One keeping 10,000 floats, called at each level of a 9,000-deep interpreted
    // recursion, overflowed the stack, where the interpreter gives its value (Q-947 review 7).
    if compiled.deps.is_empty() && compiled.excess == 0 {
        LAST.set(Some(Quick { def: at, code, entry: compiled.entry, ret: compiled.ret }));
    }
    // What it calls is still what it was compiled with.
    for (name, at) in &compiled.deps {
        match lookup(&c.env, name) {
            Some(Value::Func(f)) if Arc::as_ptr(&f.def) as usize == *at => {}
            _ => return Ran::Interpret,
        }
    }
    run(compiled.entry, compiled.ret, &raw, depth, compiled.per, compiled.reserve(), Some(&compiled.deep))
}

fn run(entry: Entry, ret: T, raw: &[u64; 16], depth: usize, frame: usize, reserve: usize, deep: Option<&std::sync::atomic::AtomicBool>) -> Ran {
    // Calls as deep as the stack left allows (a recursion deeper than that
    // gives up, and the interpreter, which grows the stack, goes on), at
    // `frame` bytes a call, with `reserve` kept.
    let left = crate::stack::remaining().unwrap_or(64 << 10).saturating_sub(reserve);
    let max = (MAX_DEPTH.saturating_sub(depth)).min(left / frame);
    // A frame is made before the code counts the call: with no room for the first, and a frame
    // over `LEVEL` (`reserve`), no machine code (Q-947 review 7).
    if reserve != 0 && max == 0 {
        return Ran::Interpret;
    }
    // Given up for the stack before, and it would be again: on `BIG` at once, not after a descent
    // to `max` and back (Q-947 review 7).
    if let Some(deep) = deep {
        if max < MAX_DEPTH.saturating_sub(depth) && deep.load(Relaxed) && crate::stack::remaining().is_some() {
            return floor_unless_done(on_big(entry, ret, raw, depth, frame, reserve), depth);
        }
    }
    let mut ctx = Ctx { bail: 0, depth: 0, max: max as u64 };
    // SAFETY: `entry` was made for exactly these argument types, read from
    // `raw` in order; `ctx` outlives the call.
    let r = unsafe { entry(raw.as_ptr(), &mut ctx) };
    if ctx.bail != 0 {
        return gave_up(&ctx, entry, ret, raw, depth, frame, reserve, deep);
    }
    Ran::Done(Ok(match ret {
        T::I => Value::Int(r as i64),
        T::F => Value::Float(f64::from_bits(r)),
        T::B => Value::Bool(r != 0),
    }))
}

/// Whether a function that compiles has a loop of its own.
fn has_loop(e: &Expr) -> bool {
    match e {
        Expr::For(..) | Expr::While(..) => true,
        Expr::Block(ss) => ss.iter().any(|s| matches!(s, Stmt::Let(_, e, _) | Stmt::Assign(_, _, e, _) | Stmt::Expr(e) | Stmt::Return(Some(e), _) if has_loop(e))),
        Expr::If(c, a, b) => has_loop(c) || has_loop(a) || b.as_deref().is_some_and(has_loop),
        _ => false,
    }
}

// ---- which functions, and their types ----------------------------------

/// Whether a parameter's or result's declared type takes `t`.
fn ty_takes(ty: &Option<Ty>, t: T) -> bool {
    let Some(ty) = ty else { return true };
    !ty.optional
        && ty.elem.is_none()
        && ty.fields.is_none()
        && ty.names.len() == 1
        && match (ty.names[0].as_str(), t) {
            ("any", _) | ("int", T::I) | ("float", T::F) | ("num", T::I | T::F) | ("bool", T::B) => true,
            _ => false,
        }
}

/// Whether a function could ever be compiled, whatever its arguments: a
/// plain top-level function whose body has only what compiled code does.
fn shape_ok(def: &FnDef) -> bool {
    fn expr(e: &Expr) -> bool {
        match e {
            Expr::Lit(Lit::Int(_) | Lit::Float(_) | Lit::Bool(_)) | Expr::Ident(..) => true,
            Expr::Unary(_, x, _) => expr(x),
            Expr::Binary(op, a, b, _) => !matches!(op, BinOp::Range) && expr(a) && expr(b),
            Expr::If(c, a, b) => expr(c) && expr(a) && b.as_deref().map_or(true, expr),
            Expr::Block(ss) => ss.iter().all(stmt),
            Expr::For(Pattern::Bind(_) | Pattern::Wild, iter, body, _) => {
                let range = match &**iter {
                    Expr::Binary(BinOp::Range, a, b, _) => expr(a) && expr(b),
                    Expr::Call(f, args, _) => matches!(&**f, Expr::Ident(n, _) if n == "range") && (1..=2).contains(&args.len()) && args.iter().all(expr),
                    _ => false,
                };
                range && expr(body)
            }
            Expr::While(c, body, _) => expr(c) && expr(body),
            Expr::Call(f, args, _) => matches!(&**f, Expr::Ident(..)) && args.iter().all(expr),
            _ => false,
        }
    }
    fn stmt(s: &Stmt) -> bool {
        match s {
            Stmt::Let(Pattern::Bind(_), e, _) | Stmt::Assign(_, _, e, _) | Stmt::Expr(e) | Stmt::Return(Some(e), _) => expr(e),
            _ => false,
        }
    }
    !def.server && !def.component && def.effects.is_none() && def.err.is_none() && def.params.iter().all(|p| p.default.is_none()) && expr(&def.body)
}

/// Variables in scope while a function is looked at: name and type (and,
/// when compiling, its Cranelift variable).
struct Scopes<X> {
    stack: Vec<Vec<(String, X)>>,
}

impl<X: Clone> Scopes<X> {
    fn get(&self, n: &str) -> Option<X> {
        self.stack.iter().rev().flat_map(|s| s.iter().rev()).find(|(m, _)| m == n).map(|(_, x)| x.clone())
    }
    fn set(&mut self, n: &str, x: X) {
        self.stack.last_mut().unwrap().push((n.to_string(), x));
    }
}

/// Works out the types in a call graph: each function for given argument
/// types, its result's type.
struct Plan<'a> {
    env: &'a Env,
    /// Functions planned (and to compile): result type.
    types: HashMap<Key, T>,
    order: Vec<(Arc<FnDef>, Vec<T>)>,
    /// Being planned, with the result type assumed for calls back into it, whether one read it,
    /// and its place in `lows`.
    open: HashMap<Key, (T, bool, usize)>,
    deps: Vec<(String, usize)>,
    done: &'a HashMap<Key, Option<Arc<Compiled>>>,
    /// Planned functions that read guesses still being made: the places in `reads` of the
    /// functions guessed, in order. Only these are taken back when one of those guesses does not
    /// come out (FEEDBACK52.md, Q-677 review 2; branch feedback52), and only for that one: the
    /// outermost alone took back, at each guess of a function, the plans that read only the
    /// first function's, and a chain each calling the first was as slow as before (Q-677 review 3).
    stands_on: HashMap<Key, Vec<usize>>,
    /// For each function being planned, the places of the guesses its plan read, in order.
    reads: Vec<Vec<usize>>,
    /// Guesses tried, against `PLAN_BASE` and `PLAN_STEPS` for each function planned (`seen`): a
    /// plan whose guesses each stand on the last (a chain each calling its caller) is still
    /// exponential, and past that its compile is refused and the interpreter runs it (Q-677
    /// reviews 3 and 4).
    steps: usize,
    seen: std::collections::HashSet<usize>,
    /// Functions whose plan met the budget before (`Jit::over`), and whether this one met it or
    /// one of them.
    over: &'a std::collections::HashSet<usize>,
    met: bool,
}

/// Guesses `Plan` tries for each function it plans, at most, past `PLAN_BASE`: a chain of 40
/// functions each calling the first, bool results, takes 9 a function.
const PLAN_STEPS: usize = 64;
/// Guesses `Plan` tries for a compile beyond `PLAN_STEPS` a function. A chain of n functions each
/// calling its caller and the next takes 2^(n+1) - 2 guesses with float results and
/// (3^(n+1) - 3) / 2 with bools, all compiled on main however long that took; with 64 a function
/// alone, 5 bools (363) and 10 floats (2,046) were refused and interpreted (FEEDBACK52.md, Q-677
/// review 4; branch feedback52). With this, up to 8 bools (9,840) and 13 floats (16,382) are
/// compiled, as on main (8,192 refused both: Q-945 review 5), and a chain that meets it is
/// refused after about 8 ms of planning at 24 functions, 11 ms at 40 and 17 ms at 100 (half a
/// microsecond a guess, more in a longer chain).
const PLAN_BASE: usize = 16384;

/// Adds the places `more` to `to`, both in order.
fn stand_on(to: &mut Vec<usize>, more: &[usize]) {
    for &m in more {
        if let Err(i) = to.binary_search(&m) {
            to.insert(i, m);
        }
    }
}

impl<'a> Plan<'a> {
    fn func(&mut self, def: &Arc<FnDef>, args: &[T]) -> Option<T> {
        let key = (Arc::as_ptr(def) as usize, args.to_vec());
        if let Some(t) = self.types.get(&key) {
            let t = *t;
            if let Some(on) = self.stands_on.get(&key) {
                let on = on.clone();
                self.read(&on);
            }
            return Some(t);
        }
        if let Some(Some(c)) = self.done.get(&key) {
            // Its code is called as it is (`Compiled::id`), so what it calls is what this plan
            // calls too.
            for d in &c.deps {
                if !self.deps.contains(d) {
                    self.deps.push(d.clone());
                }
            }
            return Some(c.ret);
        }
        // Its plan met the budget before (`compile`).
        if self.over.contains(&key.0) {
            self.met = true;
            return None;
        }
        if let Some((t, used, at)) = self.open.get_mut(&key) {
            *used = true;
            let (t, at) = (*t, *at);
            self.read(&[at]);
            return Some(t);
        }
        if !shape_ok(def) || def.params.len() != args.len() || !def.params.iter().zip(args).all(|(p, t)| ty_takes(&p.ty, *t)) {
            return None;
        }
        // A recursive function's calls of itself: its result type is
        // guessed, and the guess must come out.
        self.seen.insert(key.0);
        let me = self.reads.len();
        for guess in [T::I, T::F, T::B] {
            self.steps += 1;
            if self.steps > PLAN_STEPS * self.seen.len() + PLAN_BASE {
                self.met = true;
                return None;
            }
            self.open.insert(key.clone(), (guess, false, me));
            self.reads.push(Vec::new());
            let planned = self.order.len();
            let mut sc = Scopes { stack: vec![def.params.iter().map(|p| p.name.to_string()).zip(args.iter().copied()).collect()] };
            let mut ret = None;
            let got = self.tail(&def.body, &mut sc, &mut ret);
            let (_, used, _) = self.open.remove(&key).unwrap();
            // The guesses this plan read but its own.
            let mut on = self.reads.pop().unwrap();
            on.retain(|&at| at != me);
            let t = match (got, ret) {
                (Some(Some(a)), Some(b)) if a != b => None,
                (Some(Some(a)), _) => Some(a),
                // A body that always leaves by `return` (`{ return x % 7 + 1 }`, or an `if` whose
                // arms both do) gives what `return` gives. It was taken as one that does not
                // compile, and so was every function calling it (FEEDBACK52.md, Q-677; branch
                // feedback52).
                (Some(None), r) => r,
                (None, _) => None,
            };
            match t {
                Some(t) if (!used || t == guess) && ty_takes(&def.ret, t) => {
                    // What read this guess now stands on what this plan reads: outer guesses,
                    // or none.
                    if !self.stands_on.is_empty() {
                        for (d, a) in &self.order[planned..] {
                            let k = (Arc::as_ptr(d) as usize, a.clone());
                            let Some(read) = self.stands_on.get_mut(&k) else { continue };
                            let Ok(i) = read.binary_search(&me) else { continue };
                            read.remove(i);
                            stand_on(read, &on);
                            if read.is_empty() {
                                self.stands_on.remove(&k);
                            }
                        }
                    }
                    if !on.is_empty() {
                        self.read(&on);
                        self.stands_on.insert(key.clone(), on);
                    }
                    self.types.insert(key, t);
                    self.order.push((def.clone(), args.to_vec()));
                    return Some(t);
                }
                _ => {}
            }
            // A guess that did not come out takes back what was planned on it. A function
            // planned to call this one as `guess` stayed in `types`, the next guess found it there
            // without calling back into this one (so without `used`), and it was made with the
            // wrong type: Cranelift's def_var stopped the process (FEEDBACK52.md, Q-677 review;
            // branch feedback52). Only those: taking back every function planned under the guess
            // planned the ones that never read it again at each guess, and a chain of recursive
            // functions each calling the next took 2 to 3 times as long for each function, 7 s
            // for 24 (Q-677 review 2). `deps` keeps what they added: one too many is only looked
            // at.
            if !self.stands_on.is_empty() {
                let tail: Vec<_> = self.order.drain(planned..).collect();
                for (d, a) in tail {
                    let k = (Arc::as_ptr(&d) as usize, a);
                    if self.stands_on.get(&k).is_some_and(|read| read.binary_search(&me).is_ok()) {
                        self.stands_on.remove(&k);
                        self.types.remove(&k);
                    } else {
                        self.order.push((d, k.1));
                    }
                }
            }
            if !used {
                return None;
            }
        }
        None
    }

    /// The function being planned read a plan, or a guess, standing on the guesses at `on` in
    /// `reads`.
    fn read(&mut self, on: &[usize]) {
        if let Some(read) = self.reads.last_mut() {
            stand_on(read, on);
        }
    }

    /// The type of `e` where its value is the function's, a block's last or an `if`'s arm's,
    /// where a `return` may end it: None inside when it always leaves by `return`, and so has no
    /// value. An arm that leaves gives the `if` the other arm's type (FEEDBACK52.md, Q-677;
    /// branch feedback52).
    fn tail(&mut self, e: &Expr, sc: &mut Scopes<T>, ret: &mut Option<T>) -> Option<Option<T>> {
        match e {
            Expr::If(c, a, Some(b)) => {
                if self.value(c, sc, ret)? != T::B {
                    return None;
                }
                let x = self.tail(a, sc, ret)?;
                let y = self.tail(b, sc, ret)?;
                match (x, y) {
                    (Some(x), Some(y)) => (x == y).then_some(Some(x)),
                    (x, None) | (None, x) => Some(x),
                }
            }
            Expr::Block(ss) => {
                sc.stack.push(Vec::new());
                let t = self.block(ss, sc, ret, true);
                sc.stack.pop();
                t
            }
            e => self.value(e, sc, ret).map(Some),
        }
    }

    /// The type of `e` where its value is used.
    fn value(&mut self, e: &Expr, sc: &mut Scopes<T>, ret: &mut Option<T>) -> Option<T> {
        use T::*;
        match e {
            Expr::Lit(Lit::Int(_)) => Some(I),
            Expr::Lit(Lit::Float(_)) => Some(F),
            Expr::Lit(Lit::Bool(_)) => Some(B),
            Expr::Ident(n, _) => sc.get(n),
            Expr::Unary(UnOp::Neg, x, _) => match self.value(x, sc, ret)? {
                B => None,
                t => Some(t),
            },
            Expr::Unary(UnOp::Not, x, _) => (self.value(x, sc, ret)? == B).then_some(B),
            Expr::Binary(op, a, b, _) => {
                let (x, y) = (self.value(a, sc, ret)?, self.value(b, sc, ret)?);
                arith_type(*op, x, y)
            }
            // One that always leaves by `return` has no value to use here: `let y = { return 1 }`
            // is the interpreter's (Q-677).
            Expr::If(_, _, Some(_)) | Expr::Block(_) => self.tail(e, sc, ret)?,
            Expr::Call(f, args, _) => {
                let Expr::Ident(name, _) = &**f else { return None };
                if sc.get(name).is_some() {
                    return None;
                }
                let Some(Value::Func(callee)) = lookup(self.env, name) else { return None };
                // Only top-level functions: they see the program's names, not a caller's.
                if !Arc::ptr_eq(&callee.env, self.env) {
                    return None;
                }
                let mut ts = Vec::new();
                for a in args {
                    ts.push(self.value(a, sc, ret)?);
                }
                let t = self.func(&callee.def, &ts)?;
                let dep = (name.to_string(), Arc::as_ptr(&callee.def) as usize);
                if !self.deps.contains(&dep) {
                    self.deps.push(dep);
                }
                Some(t)
            }
            _ => None,
        }
    }

    /// A block's statements; with `want`, the type of its value, as `tail` gives it.
    fn block(&mut self, ss: &[Stmt], sc: &mut Scopes<T>, ret: &mut Option<T>, want: bool) -> Option<Option<T>> {
        let mut last = None;
        for (i, s) in ss.iter().enumerate() {
            let is_last = i + 1 == ss.len();
            last = None;
            match s {
                Stmt::Expr(e) if is_last && want => last = Some(self.tail(e, sc, ret)?),
                Stmt::Expr(e) => self.stmt_expr(e, sc, ret)?,
                Stmt::Let(Pattern::Bind(n), e, _) => {
                    let t = self.value(e, sc, ret)?;
                    sc.set(n, t);
                }
                Stmt::Assign(n, op, e, _) => {
                    let v = sc.get(n)?;
                    let t = self.value(e, sc, ret)?;
                    let t = match op {
                        AssignOp::Set => t,
                        AssignOp::Add => arith_type(BinOp::Add, v, t)?,
                        AssignOp::Sub => arith_type(BinOp::Sub, v, t)?,
                    };
                    if t != v {
                        return None;
                    }
                }
                Stmt::Return(Some(e), _) => {
                    let t = self.value(e, sc, ret)?;
                    if ret.map_or(false, |r| r != t) {
                        return None;
                    }
                    *ret = Some(t);
                    // Last, the block leaves here and has no value (Q-677).
                    last = Some(None);
                }
                _ => return None,
            }
        }
        if want {
            last
        } else {
            Some(None)
        }
    }

    /// An expression whose value is not used.
    fn stmt_expr(&mut self, e: &Expr, sc: &mut Scopes<T>, ret: &mut Option<T>) -> Option<()> {
        match e {
            Expr::If(c, a, b) => {
                if self.value(c, sc, ret)? != T::B {
                    return None;
                }
                self.stmt_expr(a, sc, ret)?;
                if let Some(b) = b {
                    self.stmt_expr(b, sc, ret)?;
                }
                Some(())
            }
            Expr::Block(ss) => {
                sc.stack.push(Vec::new());
                let t = self.block(ss, sc, ret, false);
                sc.stack.pop();
                t.map(|_| ())
            }
            Expr::For(pat, iter, body, _) => {
                let (a, b) = range_of(iter, self.env)?;
                // `range(n)` is the builtin's only where no parameter or variable of the function
                // is named `range` (`fn g(range, n)`, `let range = n`): there it gave the
                // builtin's sum, where the interpreter says "cannot call int" (FEEDBACK52.md, Q-677
                // review 3; branch feedback52). That the program's own `range` is the builtin is
                // decided here, for the code made (`range_of`), and only here: a `let range` or
                // `range = ...` after the code is made is not followed, as on main (filed as
                // Q-944).
                if matches!(&**iter, Expr::Call(..)) && sc.get("range").is_some() {
                    return None;
                }
                if let Some(a) = a {
                    if self.value(a, sc, ret)? != T::I {
                        return None;
                    }
                }
                if self.value(b, sc, ret)? != T::I {
                    return None;
                }
                sc.stack.push(Vec::new());
                if let Pattern::Bind(n) = pat {
                    sc.set(n, T::I);
                }
                let t = self.stmt_expr(body, sc, ret);
                sc.stack.pop();
                t
            }
            Expr::While(c, body, _) => {
                if self.value(c, sc, ret)? != T::B {
                    return None;
                }
                sc.stack.push(Vec::new());
                let t = self.stmt_expr(body, sc, ret);
                sc.stack.pop();
                t
            }
            e => self.value(e, sc, ret).map(|_| ()),
        }
    }
}

/// `a..b` or `range(a, b)` / `range(b)` (the builtin): its bounds' expressions.
fn range_of<'e>(iter: &'e Expr, env: &Env) -> Option<(Option<&'e Expr>, &'e Expr)> {
    match iter {
        Expr::Binary(BinOp::Range, a, b, _) => Some((Some(a), b)),
        Expr::Call(f, args, _) if matches!(&**f, Expr::Ident(n, _) if n == "range") => {
            // The builtin itself, the global: a scope around the function that names something
            // `range` (one function in two scopes, the builtin in one) is the interpreter's. Its
            // code was reused in the other, and checking the name at each call instead kept the
            // function from the last function's quick call: 3 times the instructions mapping it
            // over a list (FEEDBACK52.md, Q-677 review 3; branch feedback52).
            if !matches!(lookup_global(env, "range"), Some((Value::Builtin("range", _), true))) {
                return None;
            }
            match args.as_slice() {
                [b] => Some((None, b)),
                [a, b] => Some((Some(a), b)),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The type of `x op y`, as the interpreter's `binop` gives it, when it is
/// one compiled code can have.
pub(crate) fn arith_type(op: BinOp, x: T, y: T) -> Option<T> {
    use T::*;
    match op {
        BinOp::Add | BinOp::Sub | BinOp::Mul => match (x, y) {
            (I, I) => Some(I),
            (I | F, I | F) => Some(F),
            _ => None,
        },
        // Ints divide to an int or a float, depending on the numbers.
        BinOp::Div => match (x, y) {
            (I, I) => None,
            (I | F, I | F) => Some(F),
            _ => None,
        },
        BinOp::Mod => (x == I && y == I).then_some(I),
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => matches!((x, y), (I | F, I | F)).then_some(B),
        BinOp::Eq | BinOp::Ne => (x == y).then_some(B),
        BinOp::And | BinOp::Or => (x == B && y == B).then_some(B),
        BinOp::Range => None,
    }
}

/// `i op f` of an int `i` and a float `f` that are the same float (`i as f64 == f`), exactly, the
/// rest of the interpreter's `value::int_float_cmp`: the float is whole then, and the int is
/// compared with it as an int, 2^63 above every int. Compiled code went through the float alone,
/// and `9007199254740993 > 9007199254740992.0` was false (FEEDBACK55.md, Q-787; branch
/// feedback55). Where the two are different floats those decide, and where the int is within
/// 2^53 its float is itself: only past 2^53 does this case need it (`float_needs_look`).
pub(crate) fn int_float_same_ir(b: &mut FunctionBuilder, op: BinOp, i: V, f: V) -> V {
    let icc = match op {
        BinOp::Lt => IntCC::SignedLessThan,
        BinOp::Le => IntCC::SignedLessThanOrEqual,
        BinOp::Gt => IntCC::SignedGreaterThan,
        _ => IntCC::SignedGreaterThanOrEqual,
    };
    let whole = b.ins().fcvt_to_sint_sat(types::I64, f);
    let by_int = b.ins().icmp(icc, i, whole);
    let two63 = b.ins().f64const(9223372036854775808.0);
    let top = b.ins().fcmp(FloatCC::GreaterThanOrEqual, f, two63);
    let below = b.ins().iconst(types::I8, matches!(op, BinOp::Lt | BinOp::Le) as i64);
    b.ins().select(top, below, by_int)
}

/// Whether the float compare of an int and the float `f` needs a closer look: `f` is NaN, which
/// cannot be compared, or 2^53 or more either way, where floats skip ints and an int's float can
/// be `f` though the int is not. One compare (`uge`, a single branch on every machine), which
/// stands for the NaN test the usual path had, and is taken out of a loop when `f` does not
/// change in it; the closer look in a block at the end (Q-787). Gives the block (switched to)
/// and the usual path's, to go on in.
pub(crate) fn float_needs_look(b: &mut FunctionBuilder, f: V) -> (Block, Block) {
    let a = b.ins().fabs(f);
    let edge = b.ins().f64const(9007199254740992.0);
    let odd = b.ins().fcmp(FloatCC::UnorderedOrGreaterThanOrEqual, a, edge);
    let (cold, go) = (b.create_block(), b.create_block());
    b.set_cold_block(cold);
    b.ins().brif(odd, cold, &[], go, &[]);
    b.switch_to_block(cold);
    (cold, go)
}

/// The float compare of `op`.
pub(crate) fn float_cc(op: BinOp) -> FloatCC {
    match op {
        BinOp::Lt => FloatCC::LessThan,
        BinOp::Le => FloatCC::LessThanOrEqual,
        BinOp::Gt => FloatCC::GreaterThan,
        _ => FloatCC::GreaterThanOrEqual,
    }
}

/// `op` with its sides swapped: `x < y` is `y > x`.
pub(crate) fn swapped(op: BinOp) -> BinOp {
    match op {
        BinOp::Lt => BinOp::Gt,
        BinOp::Le => BinOp::Ge,
        BinOp::Gt => BinOp::Lt,
        BinOp::Ge => BinOp::Le,
        other => other,
    }
}

// ---- making the code ---------------------------------------------------

fn compile(jit: &mut Jit, c: &Closure, sig: &[T]) -> Option<Compiled> {
    let mut plan = Plan { env: &c.env, types: HashMap::new(), order: Vec::new(), open: HashMap::new(), deps: Vec::new(), done: &jit.done, stands_on: HashMap::new(), reads: Vec::new(), steps: 0, seen: Default::default(), over: &jit.over, met: false };
    let Some(ret) = plan.func(&c.def, sig) else {
        // Its plan met the budget, or a function whose plan did: it is not planned again, as
        // the function called or as one calling it, and it is the interpreter's (`NEVER`), so
        // that its calls cost no more than the interpreter's. Each function of a chain past the
        // budget was planned again to the budget, as the first of its own compile, and every
        // call of one looked for its code: 5 bool functions each calling its caller and the
        // next took 30,409 M instructions, the interpreter 22,891 M (FEEDBACK52.md, Q-677 review
        // 4; branch feedback52). Kept, so that its address is its own.
        if plan.met {
            let at = Arc::as_ptr(&c.def) as usize;
            jit.over.insert(at);
            jit.keep.push(c.def.clone());
            c.def.hot.store(NEVER, Relaxed);
        }
        return None;
    };
    let (types, order, deps) = (plan.types, plan.order, plan.deps);
    // Each function to make, declared first (they may call each other).
    let mut ids: HashMap<Key, FuncId> = HashMap::new();
    for (def, args) in &order {
        let key = (Arc::as_ptr(def) as usize, args.clone());
        let id = jit.module.declare_anonymous_function(&signature(&jit.module, args, types[&key])).ok()?;
        ids.insert(key, id);
    }
    let mut fctx = FunctionBuilderContext::new();
    let mut ctx = jit.module.make_context();
    let tc = jit.module.target_config();
    let mut made = Vec::new();
    let (mut calls, mut earlier) = (Vec::new(), (LEVEL, 0));
    let (refuse, in_cranelift) = REFUSE.with(|r| r.borrow().clone());
    let all = 'all: {
        for (def, args) in &order {
            let refused = !refuse.is_empty() && def.name == refuse;
            let key = (Arc::as_ptr(def) as usize, args.clone());
            ctx.func.signature = signature(&jit.module, args, types[&key]);
            ctx.func.name = UserFuncName::user(0, ids[&key].as_u32());
            {
                let mut g = Gen {
                    b: FunctionBuilder::new(&mut ctx.func, &mut fctx),
                    module: &mut jit.module,
                    env: &c.env,
                    types: &types,
                    ids: &ids,
                    done: &jit.done,
                    ctx: V::from_u32(0),
                    depth0: V::from_u32(0),
                    bail: Block::from_u32(0),
                    sc: Scopes { stack: vec![Vec::new()] },
                    ret: types[&key],
                    ended: false,
                    calls: Vec::new(),
                    earlier: (LEVEL, 0),
                };
                if (refused && !in_cranelift) || g.function(def, args).is_none() {
                    break 'all false;
                }
                g.b.seal_all_blocks();
                g.b.finalize(tc);
                earlier = (earlier.0.max(g.earlier.0), earlier.1.max(g.earlier.1));
                calls.push((ids[&key].as_u32(), std::mem::take(&mut g.calls), LEVEL));
            }
            if refused {
                ctx.func.signature.returns.clear();
            }
            if jit.module.define_function(ids[&key], &mut ctx).is_err() {
                break 'all false;
            }
            // Its frame below the frame pointer, as Cranelift laid it out (spills, saved
            // registers, outgoing arguments), and above it the frame pointer and the return
            // address (Q-947). 16 bytes more a call took 9,000 calls with 56 such lets in
            // `parallel` for the stack, and ran them again on a new one, 8.6% over main; what is
            // not a call's (the entry, the call that gives up) is `RESERVE`.
            if let (Some(f), Some(made)) = (ctx.compiled_code().and_then(|code| code.buffer.frame_layout()), calls.last_mut()) {
                made.2 = f.frame_to_fp_offset as usize + 16;
            }
            jit.module.clear_context(&mut ctx);
            made.push(ids[&key]);
        }
        true
    };
    if !all {
        // A function made here calls the others, and the next finalize_definitions (another
        // compile's, or a loop's) looks for each of them: one declared and not made stopped the
        // process ("anonymous symbol must be defined locally"). Each not made is made to give up;
        // none of this code runs, as its entry is not made (FEEDBACK52.md, Q-677 review; branch
        // feedback52).
        for (key, id) in &ids {
            if !made.contains(id) {
                give_up(&mut jit.module, *id, &key.1, types[key]);
            }
        }
        return None;
    }
    let (per, excess) = stack_counted(&calls, earlier);
    // The entry: arguments from memory, the result as bits.
    let key = (Arc::as_ptr(&c.def) as usize, sig.to_vec());
    let target = match ids.get(&key) {
        Some(id) => *id,
        None => return None,
    };
    let ptr = jit.module.target_config().pointer_type();
    let mut esig = jit.module.make_signature();
    esig.params.push(AbiParam::new(ptr));
    esig.params.push(AbiParam::new(ptr));
    esig.returns.push(AbiParam::new(types::I64));
    let entry_id = jit.module.declare_anonymous_function(&esig).ok()?;
    ctx.func.signature = esig;
    ctx.func.name = UserFuncName::user(0, entry_id.as_u32());
    {
        let mut b = FunctionBuilder::new(&mut ctx.func, &mut fctx);
        let blk = b.create_block();
        b.append_block_params_for_function_params(blk);
        b.switch_to_block(blk);
        let (argp, cx) = (b.block_params(blk)[0], b.block_params(blk)[1]);
        let mut vals = Vec::new();
        for (i, t) in sig.iter().enumerate() {
            let raw = b.ins().load(types::I64, MemFlagsData::trusted(), argp, (i * 8) as i32);
            vals.push(match t {
                T::I => raw,
                T::F => b.ins().bitcast(types::F64, MemFlagsData::new(), raw),
                T::B => b.ins().ireduce(types::I8, raw),
            });
        }
        vals.push(cx);
        let f = jit.module.declare_func_in_func(target, b.func);
        let call = b.ins().call(f, &vals);
        let r = b.inst_results(call)[0];
        let bits = match ret {
            T::I => r,
            T::F => b.ins().bitcast(types::I64, MemFlagsData::new(), r),
            T::B => b.ins().uextend(types::I64, r),
        };
        b.ins().return_(&[bits]);
        b.seal_all_blocks();
        b.finalize(tc);
    }
    jit.module.define_function(entry_id, &mut ctx).ok()?;
    jit.module.clear_context(&mut ctx);
    jit.module.finalize_definitions().ok()?;
    for (def, args) in order {
        let key = (Arc::as_ptr(&def) as usize, args);
        jit.keep.push(def);
        let _ = key;
    }
    jit.keep.push(c.def.clone());
    let code = jit.module.get_finalized_function(entry_id);
    // SAFETY: the entry was made with exactly this signature.
    let entry = unsafe { std::mem::transmute::<*const u8, Entry>(code) };
    Some(Compiled { entry, ret, deps, id: target, per, excess, deep: Default::default() })
}

/// The stack counted for each call of a compile's code (`Compiled::per`) and kept beyond that
/// (`Compiled::excess`), from the functions made (`calls`: each with those it calls and its frame)
/// and those compiled before that they call (`earlier`). A function on a cycle of calls can be on
/// a chain any number of times, so each call is counted at the largest of their frames; one on no
/// cycle is there at most once, and what its frame has over that is kept once. Each call counted
/// at the largest frame of the whole compile, a deep recursion of small frames below one big
/// caller, or a big callee it never called, was given up for the stack and run again on a new one
/// at each call: 3,976 M instructions against main's 1,574 M (Q-947 review 7).
fn stack_counted(calls: &[(u32, Vec<u32>, usize)], earlier: (usize, usize)) -> (usize, usize) {
    // On a cycle: it reaches itself. A compile makes few functions: plain lists, no tables.
    let on_cycle = |start: usize| {
        let mut seen = vec![false; calls.len()];
        let mut todo: Vec<u32> = calls[start].1.clone();
        while let Some(f) = todo.pop() {
            if f == calls[start].0 {
                return true;
            }
            if let Some(at) = calls.iter().position(|c| c.0 == f).filter(|&at| !seen[at]) {
                seen[at] = true;
                todo.extend_from_slice(&calls[at].1);
            }
        }
        false
    };
    let cyclic: Vec<bool> = (0..calls.len()).map(on_cycle).collect();
    let per = calls.iter().zip(&cyclic).filter(|(_, c)| **c).map(|(f, _)| f.2).fold(earlier.0, usize::max);
    let excess = calls.iter().zip(&cyclic).filter(|(_, c)| !**c).map(|(f, _)| f.2.saturating_sub(per)).sum::<usize>() + earlier.1;
    (per, excess)
}

/// Code for `id` that only gives up, as the function would have at its first step.
fn give_up(m: &mut JITModule, id: FuncId, args: &[T], ret: T) {
    let mut ctx = m.make_context();
    ctx.func.signature = signature(m, args, ret);
    ctx.func.name = UserFuncName::user(0, id.as_u32());
    let mut fctx = FunctionBuilderContext::new();
    let mut b = FunctionBuilder::new(&mut ctx.func, &mut fctx);
    let blk = b.create_block();
    b.append_block_params_for_function_params(blk);
    b.switch_to_block(blk);
    let cx = b.block_params(blk)[args.len()];
    let one = b.ins().iconst(types::I64, 1);
    b.ins().store(MemFlagsData::trusted(), one, cx, BAIL);
    let zero = match ret {
        T::I => b.ins().iconst(types::I64, 0),
        T::F => b.ins().f64const(0.0),
        T::B => b.ins().iconst(types::I8, 0),
    };
    b.ins().return_(&[zero]);
    b.seal_all_blocks();
    b.finalize(m.target_config());
    let _ = m.define_function(id, &mut ctx);
}

fn signature(m: &JITModule, args: &[T], ret: T) -> cranelift_codegen::ir::Signature {
    let mut s = m.make_signature();
    for t in args {
        s.params.push(AbiParam::new(t.ir()));
    }
    s.params.push(AbiParam::new(m.target_config().pointer_type()));
    s.returns.push(AbiParam::new(ret.ir()));
    s
}

/// Makes one function's code.
struct Gen<'a, 'f> {
    b: FunctionBuilder<'f>,
    module: &'a mut JITModule,
    env: &'a Env,
    types: &'a HashMap<Key, T>,
    ids: &'a HashMap<Key, FuncId>,
    done: &'a HashMap<Key, Option<Arc<Compiled>>>,
    /// The `Ctx` pointer, and the call depth on entry.
    ctx: V,
    depth0: V,
    /// Where code goes to give up.
    bail: Block,
    sc: Scopes<(Variable, T)>,
    ret: T,
    /// The current block has ended (a `return`): code after it is unreachable.
    ended: bool,
    /// The functions of this compile it calls, and the largest `per` and `excess` of those
    /// compiled before it calls (Q-947).
    calls: Vec<u32>,
    earlier: (usize, usize),
}

const BAIL: i32 = 0;
const DEPTH: i32 = 8;
const MAX: i32 = 16;

impl Gen<'_, '_> {
    fn function(&mut self, def: &FnDef, args: &[T]) -> Option<()> {
        let entry = self.b.create_block();
        self.b.append_block_params_for_function_params(entry);
        self.b.switch_to_block(entry);
        let params = self.b.block_params(entry).to_vec();
        self.ctx = params[args.len()];
        for (i, (p, t)) in def.params.iter().zip(args).enumerate() {
            let var = self.b.declare_var(t.ir());
            self.b.try_def_var(var, params[i]).ok()?;
            self.sc.set(&p.name, (var, *t));
        }
        // The bail-out: tell the caller, return anything.
        self.bail = self.b.create_block();
        // Deeper by one, within the limit.
        let d = self.b.ins().load(types::I64, MemFlagsData::trusted(), self.ctx, DEPTH);
        self.depth0 = d;
        let d1 = self.b.ins().iadd_imm_s(d, 1);
        let max = self.b.ins().load(types::I64, MemFlagsData::trusted(), self.ctx, MAX);
        let over = self.b.ins().icmp(IntCC::UnsignedGreaterThan, d1, max);
        self.check(over);
        self.b.ins().store(MemFlagsData::trusted(), d1, self.ctx, DEPTH);
        let mut ret = None;
        let v = self.value(&def.body, &mut ret)?;
        if !self.ended {
            self.leave(v);
        }
        // Giving up.
        self.b.switch_to_block(self.bail);
        let one = self.b.ins().iconst(types::I64, 1);
        self.b.ins().store(MemFlagsData::trusted(), one, self.ctx, BAIL);
        let zero = self.zero(self.ret);
        self.b.ins().return_(&[zero]);
        Some(())
    }

    fn zero(&mut self, t: T) -> V {
        match t {
            T::I => self.b.ins().iconst(types::I64, 0),
            T::F => self.b.ins().f64const(0.0),
            T::B => self.b.ins().iconst(types::I8, 0),
        }
    }

    /// Returns `v`, the call depth as it was.
    fn leave(&mut self, v: V) {
        self.b.ins().store(MemFlagsData::trusted(), self.depth0, self.ctx, DEPTH);
        self.b.ins().return_(&[v]);
        self.ended = true;
    }

    /// Gives up when `cond` holds.
    fn check(&mut self, cond: V) {
        let go = self.b.create_block();
        self.b.ins().brif(cond, self.bail, &[], go, &[]);
        self.b.switch_to_block(go);
    }

    /// After a `return`, code goes on in a block nothing reaches.
    fn reachable(&mut self) {
        if self.ended {
            let b = self.b.create_block();
            self.b.switch_to_block(b);
            self.ended = false;
        }
    }

    fn value(&mut self, e: &Expr, ret: &mut Option<T>) -> Option<V> {
        self.reachable();
        Some(match e {
            Expr::Lit(Lit::Int(n)) => self.b.ins().iconst(types::I64, *n),
            Expr::Lit(Lit::Float(f)) => self.b.ins().f64const(*f),
            Expr::Lit(Lit::Bool(x)) => self.b.ins().iconst(types::I8, *x as i64),
            Expr::Ident(n, _) => {
                let (var, _) = self.sc.get(n)?;
                self.b.use_var(var)
            }
            Expr::Unary(UnOp::Neg, x, _) => {
                let t = self.type_of(x)?;
                let v = self.value(x, ret)?;
                match t {
                    T::I => {
                        let over = self.b.ins().icmp_imm_s(IntCC::Equal, v, i64::MIN);
                        self.check(over);
                        self.b.ins().ineg(v)
                    }
                    _ => self.b.ins().fneg(v),
                }
            }
            Expr::Unary(UnOp::Not, x, _) => {
                let v = self.value(x, ret)?;
                self.b.ins().bxor_imm_s(v, 1)
            }
            Expr::Binary(op @ (BinOp::And | BinOp::Or), a, b, _) => {
                let x = self.value(a, ret)?;
                let rhs = self.b.create_block();
                let join = self.b.create_block();
                let out = self.b.append_block_param(join, types::I8);
                // `a and b`: b when a holds; `a or b`: b when it does not.
                if matches!(op, BinOp::And) {
                    self.b.ins().brif(x, rhs, &[], join, &[x.into()]);
                } else {
                    self.b.ins().brif(x, join, &[x.into()], rhs, &[]);
                }
                self.b.switch_to_block(rhs);
                let y = self.value(b, ret)?;
                self.b.ins().jump(join, &[y.into()]);
                self.b.switch_to_block(join);
                out
            }
            Expr::Binary(op, a, b, _) => {
                let (ta, tb) = (self.type_of(a)?, self.type_of(b)?);
                let x = self.value(a, ret)?;
                let y = self.value(b, ret)?;
                self.binop(*op, ta, tb, x, y)?
            }
            Expr::If(c, a, Some(b)) => {
                let t = self.type_of(e)?;
                let cv = self.value(c, ret)?;
                let (yes, no, join) = (self.b.create_block(), self.b.create_block(), self.b.create_block());
                let out = self.b.append_block_param(join, t.ir());
                self.b.ins().brif(cv, yes, &[], no, &[]);
                let mut ends = true;
                for (blk, arm) in [(yes, a), (no, b)] {
                    self.b.switch_to_block(blk);
                    let v = self.value(arm, ret)?;
                    if !self.ended {
                        self.b.ins().jump(join, &[v.into()]);
                        ends = false;
                    }
                    self.ended = false;
                }
                self.b.switch_to_block(join);
                // Both arms left by `return`: nothing reaches the join, and the `if` has no value
                // to give, as the block ending in `return` has none (FEEDBACK52.md, Q-677; branch
                // feedback52).
                self.ended = ends;
                out
            }
            Expr::Block(ss) => {
                self.sc.stack.push(Vec::new());
                let v = self.block(ss, ret, true);
                self.sc.stack.pop();
                v?
            }
            Expr::Call(f, args, _) => {
                let Expr::Ident(name, _) = &**f else { return None };
                let Some(Value::Func(callee)) = lookup(self.env, name) else { return None };
                let mut ts = Vec::new();
                let mut vs = Vec::new();
                for a in args {
                    ts.push(self.type_of(a)?);
                    vs.push(self.value(a, ret)?);
                }
                let key = (Arc::as_ptr(&callee.def) as usize, ts.clone());
                // Made in this compile, or compiled on its own before (`Compiled::id`), its own
                // chains of calls counted (Q-947).
                let id = match self.ids.get(&key) {
                    Some(id) => {
                        self.calls.push(id.as_u32());
                        *id
                    }
                    None => {
                        let c = self.done.get(&key)?.as_ref()?;
                        self.earlier = (self.earlier.0.max(c.per), self.earlier.1.max(c.excess));
                        c.id
                    }
                };
                let _ = self.types;
                vs.push(self.ctx);
                let fr = self.module.declare_func_in_func(id, self.b.func);
                let call = self.b.ins().call(fr, &vs);
                let r = self.b.inst_results(call)[0];
                // Gave up inside: give up here too.
                let bailed = self.b.ins().load(types::I64, MemFlagsData::trusted(), self.ctx, BAIL);
                self.check(bailed);
                r
            }
            _ => return None,
        })
    }

    /// The type of an expression, as planned (for operands and branches).
    fn type_of(&self, e: &Expr) -> Option<T> {
        use T::*;
        match e {
            Expr::Lit(Lit::Int(_)) => Some(I),
            Expr::Lit(Lit::Float(_)) => Some(F),
            Expr::Lit(Lit::Bool(_)) => Some(B),
            Expr::Ident(n, _) => self.sc.get(n).map(|(_, t)| t),
            Expr::Unary(UnOp::Neg, x, _) => self.type_of(x),
            Expr::Unary(UnOp::Not, _, _) => Some(B),
            Expr::Binary(op, a, b, _) => arith_type(*op, self.type_of(a)?, self.type_of(b)?),
            // An arm that always leaves by `return` gives no value: the other arm's type is the
            // `if`'s, and when both leave any will do, the function's own (Q-677).
            Expr::If(_, a, Some(b)) if leaves(a) => if leaves(b) { Some(self.ret) } else { self.type_of(b) },
            Expr::If(_, a, _) => self.type_of(a),
            Expr::Block(ss) => block_type(ss, Scopes { stack: self.sc.stack.iter().map(|s| s.iter().map(|(n, (_, t))| (n.clone(), *t)).collect()).collect() }, self),
            Expr::Call(f, args, _) => {
                let ts: Option<Vec<T>> = args.iter().map(|a| self.type_of(a)).collect();
                self.call_type(f, ts?)
            }
            _ => None,
        }
    }

    /// The type `f(args)` gives, as planned, for arguments of the types `ts`.
    fn call_type(&self, f: &Expr, ts: Vec<T>) -> Option<T> {
        let Expr::Ident(name, _) = f else { return None };
        let Some(Value::Func(callee)) = lookup(self.env, name) else { return None };
        let key = (Arc::as_ptr(&callee.def) as usize, ts);
        self.types.get(&key).copied().or_else(|| self.done.get(&key).and_then(|c| c.as_ref().map(|c| c.ret)))
    }

    fn binop(&mut self, op: BinOp, ta: T, tb: T, x: V, y: V) -> Option<V> {
        use T::*;
        let t = arith_type(op, ta, tb)?;
        // Ints with floats are floats.
        let (x, y) = match (ta, tb) {
            (I, F) => (self.b.ins().fcvt_from_sint(types::F64, x), y),
            (F, I) => (x, self.b.ins().fcvt_from_sint(types::F64, y)),
            _ => (x, y),
        };
        let floats = ta == F || tb == F;
        Some(match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul if t == F => match op {
                BinOp::Add => self.b.ins().fadd(x, y),
                BinOp::Sub => self.b.ins().fsub(x, y),
                _ => self.b.ins().fmul(x, y),
            },
            BinOp::Add => {
                let r = self.b.ins().iadd(x, y);
                let (a, b) = (self.b.ins().bxor(x, r), self.b.ins().bxor(y, r));
                let both = self.b.ins().band(a, b);
                let over = self.b.ins().icmp_imm_s(IntCC::SignedLessThan, both, 0);
                self.check(over);
                r
            }
            BinOp::Sub => {
                let r = self.b.ins().isub(x, y);
                let (a, b) = (self.b.ins().bxor(x, y), self.b.ins().bxor(x, r));
                let both = self.b.ins().band(a, b);
                let over = self.b.ins().icmp_imm_s(IntCC::SignedLessThan, both, 0);
                self.check(over);
                r
            }
            BinOp::Mul => {
                let lo = self.b.ins().imul(x, y);
                let hi = self.b.ins().smulhi(x, y);
                let sign = self.b.ins().sshr_imm_s(lo, 63);
                let over = self.b.ins().icmp(IntCC::NotEqual, hi, sign);
                self.check(over);
                lo
            }
            BinOp::Div => self.b.ins().fdiv(x, y),
            BinOp::Mod => {
                let zero = self.b.ins().icmp_imm_s(IntCC::Equal, y, 0);
                self.check(zero);
                let min = self.b.ins().icmp_imm_s(IntCC::Equal, x, i64::MIN);
                let neg1 = self.b.ins().icmp_imm_s(IntCC::Equal, y, -1);
                let over = self.b.ins().band(min, neg1);
                self.check(over);
                self.b.ins().srem(x, y)
            }
            // An int with a float, exactly, in ints (FEEDBACK55.md, Q-787; branch feedback55).
            // As floats, as before, unless they could be wrong: the same float past 2^53 (rare),
            // where the call is the interpreter's, which compares exactly, as for NaN.
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge if ta != tb => {
                let (fi, f, op) = if ta == I { (x, y, op) } else { (y, x, swapped(op)) };
                let (_, go) = float_needs_look(&mut self.b, f);
                // NaN cannot be compared: the interpreter fails there.
                let same = self.b.ins().fcmp(FloatCC::UnorderedOrEqual, fi, f);
                self.b.ins().brif(same, self.bail, &[], go, &[]);
                self.b.switch_to_block(go);
                self.b.ins().fcmp(float_cc(op), fi, f)
            }
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge if floats => {
                // NaN cannot be compared: the interpreter fails there.
                let nan = self.b.ins().fcmp(FloatCC::Unordered, x, y);
                self.check(nan);
                self.b.ins().fcmp(float_cc(op), x, y)
            }
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                let cc = match op {
                    BinOp::Lt => IntCC::SignedLessThan,
                    BinOp::Le => IntCC::SignedLessThanOrEqual,
                    BinOp::Gt => IntCC::SignedGreaterThan,
                    _ => IntCC::SignedGreaterThanOrEqual,
                };
                self.b.ins().icmp(cc, x, y)
            }
            BinOp::Eq | BinOp::Ne if ta == F => self.b.ins().fcmp(if op == BinOp::Eq { FloatCC::Equal } else { FloatCC::NotEqual }, x, y),
            BinOp::Eq | BinOp::Ne => self.b.ins().icmp(if op == BinOp::Eq { IntCC::Equal } else { IntCC::NotEqual }, x, y),
            _ => return None,
        })
    }

    /// A block's statements; with `want`, its last value.
    fn block(&mut self, ss: &[Stmt], ret: &mut Option<T>, want: bool) -> Option<V> {
        let mut last = None;
        for (i, s) in ss.iter().enumerate() {
            let is_last = i + 1 == ss.len();
            self.reachable();
            match s {
                Stmt::Expr(e) if is_last && want => last = Some(self.value(e, ret)?),
                Stmt::Expr(e) => self.stmt_expr(e, ret)?,
                Stmt::Let(Pattern::Bind(n), e, _) => {
                    let t = self.type_of(e)?;
                    let v = self.value(e, ret)?;
                    let var = self.b.declare_var(t.ir());
                    // A value of another type than its variable's refuses the function, where
                    // def_var stopped the process (here and below; FEEDBACK52.md, Q-677 review;
                    // branch feedback52).
                    self.b.try_def_var(var, v).ok()?;
                    self.sc.set(n, (var, t));
                }
                Stmt::Assign(n, op, e, _) => {
                    let (var, t) = self.sc.get(n)?;
                    let te = self.type_of(e)?;
                    let v = self.value(e, ret)?;
                    let v = match op {
                        AssignOp::Set => v,
                        AssignOp::Add | AssignOp::Sub => {
                            let cur = self.b.use_var(var);
                            let bop = if matches!(op, AssignOp::Add) { BinOp::Add } else { BinOp::Sub };
                            self.binop(bop, t, te, cur, v)?
                        }
                    };
                    self.b.try_def_var(var, v).ok()?;
                }
                Stmt::Return(Some(e), _) => {
                    let v = self.value(e, ret)?;
                    self.leave(v);
                }
                _ => return None,
            }
        }
        if want {
            // A block that ended in `return` has no value here: any will do.
            match last {
                Some(v) => Some(v),
                None if self.ended => Some(V::from_u32(0)),
                None => None,
            }
        } else {
            Some(V::from_u32(0))
        }
    }

    fn stmt_expr(&mut self, e: &Expr, ret: &mut Option<T>) -> Option<()> {
        self.reachable();
        match e {
            Expr::If(c, a, b) => {
                let cv = self.value(c, ret)?;
                let (yes, no, join) = (self.b.create_block(), self.b.create_block(), self.b.create_block());
                self.b.ins().brif(cv, yes, &[], no, &[]);
                self.b.switch_to_block(yes);
                self.stmt_expr(a, ret)?;
                if !self.ended {
                    self.b.ins().jump(join, &[]);
                }
                self.ended = false;
                self.b.switch_to_block(no);
                if let Some(b) = b {
                    self.stmt_expr(b, ret)?;
                }
                if !self.ended {
                    self.b.ins().jump(join, &[]);
                }
                self.ended = false;
                self.b.switch_to_block(join);
                Some(())
            }
            Expr::Block(ss) => {
                self.sc.stack.push(Vec::new());
                let r = self.block(ss, ret, false);
                self.sc.stack.pop();
                r.map(|_| ())
            }
            Expr::For(pat, iter, body, _) => {
                let (a, b) = range_of(iter, self.env)?;
                let start = match a {
                    Some(a) => self.value(a, ret)?,
                    None => self.b.ins().iconst(types::I64, 0),
                };
                let end = self.value(b, ret)?;
                let k = self.b.declare_var(types::I64);
                self.b.try_def_var(k, start).ok()?;
                let (head, step, exit) = (self.b.create_block(), self.b.create_block(), self.b.create_block());
                self.b.ins().jump(head, &[]);
                self.b.switch_to_block(head);
                let kv = self.b.use_var(k);
                let more = self.b.ins().icmp(IntCC::SignedLessThan, kv, end);
                self.b.ins().brif(more, step, &[], exit, &[]);
                self.b.switch_to_block(step);
                self.sc.stack.push(Vec::new());
                if let Pattern::Bind(n) = pat {
                    // Each step's own variable, from the count.
                    let var = self.b.declare_var(types::I64);
                    self.b.try_def_var(var, kv).ok()?;
                    self.sc.set(n, (var, T::I));
                }
                let r = self.stmt_expr(body, ret);
                self.sc.stack.pop();
                r?;
                if !self.ended {
                    let next = self.b.ins().iadd_imm_s(kv, 1);
                    self.b.try_def_var(k, next).ok()?;
                    self.b.ins().jump(head, &[]);
                }
                self.ended = false;
                self.b.switch_to_block(exit);
                Some(())
            }
            Expr::While(c, body, _) => {
                let (head, step, exit) = (self.b.create_block(), self.b.create_block(), self.b.create_block());
                self.b.ins().jump(head, &[]);
                self.b.switch_to_block(head);
                let cv = self.value(c, ret)?;
                self.b.ins().brif(cv, step, &[], exit, &[]);
                self.b.switch_to_block(step);
                self.sc.stack.push(Vec::new());
                let r = self.stmt_expr(body, ret);
                self.sc.stack.pop();
                r?;
                if !self.ended {
                    self.b.ins().jump(head, &[]);
                }
                self.ended = false;
                self.b.switch_to_block(exit);
                Some(())
            }
            e => self.value(e, ret).map(|_| ()),
        }
    }
}

/// The type of `e` with the variables of `sc` (for a block's last value).
fn type_in(e: &Expr, sc: &Scopes<T>, g: &Gen) -> Option<T> {
    use T::*;
    match e {
        Expr::Lit(Lit::Int(_)) => Some(I),
        Expr::Lit(Lit::Float(_)) => Some(F),
        Expr::Lit(Lit::Bool(_)) => Some(B),
        Expr::Ident(n, _) => sc.get(n),
        Expr::Unary(UnOp::Neg, x, _) => type_in(x, sc, g),
        Expr::Unary(UnOp::Not, _, _) => Some(B),
        Expr::Binary(op, a, b, _) => arith_type(*op, type_in(a, sc, g)?, type_in(b, sc, g)?),
        Expr::If(_, a, Some(b)) if leaves(a) => type_in(b, sc, g),
        Expr::If(_, a, _) => type_in(a, sc, g),
        // A block or a call in a block is typed with that block's names, not the function's:
        // `let y = { let x = 2.5; h(x) }`, where the function's `x` is an int, had `h(x)` taken
        // as `h` of an int, the variable made for `y` did not take the float, and Cranelift
        // stopped the process (found with FEEDBACK52.md, Q-677; branch feedback52).
        Expr::Block(ss) => block_type(ss, Scopes { stack: sc.stack.clone() }, g),
        _ => {
            let Expr::Call(f, args, _) = e else { return None };
            let ts: Option<Vec<T>> = args.iter().map(|a| type_in(a, sc, g)).collect();
            g.call_type(f, ts?)
        }
    }
}

/// The type of a block's last expression, with the variables of `sc` and the block's own `let`s.
fn block_type(ss: &[Stmt], mut sc: Scopes<T>, g: &Gen) -> Option<T> {
    sc.stack.push(Vec::new());
    let (last, before) = ss.split_last()?;
    for s in before {
        if let Stmt::Let(Pattern::Bind(n), e, _) = s {
            let t = type_in(e, &sc, g)?;
            sc.set(n, t);
        }
    }
    let Stmt::Expr(e) = last else { return None };
    type_in(e, &sc, g)
}

/// Whether `e`, where its value is wanted, always leaves its function by `return` instead: a
/// block ending in `return` or in such an `if`, or an `if` whose arms both are. Planned, such an
/// arm gives the `if` no type of its own (`Plan::tail`; FEEDBACK52.md, Q-677; branch feedback52).
fn leaves(e: &Expr) -> bool {
    match e {
        Expr::Block(ss) => matches!(ss.last(), Some(Stmt::Return(..))) || matches!(ss.last(), Some(Stmt::Expr(e)) if leaves(e)),
        Expr::If(_, a, Some(b)) => leaves(a) && leaves(b),
        _ => false,
    }
}
