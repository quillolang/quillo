//! Loops in machine code (FEEDBACK29.md; branch feedback29-fixes).
//!
//! A numeric function goes to machine code whole (src/jit.rs). Most loops are in functions
//! that do other things too, or at a script's top: those run their first `WARM` rounds in
//! the interpreter, and a loop that has gone that far is compiled, for the types its
//! variables have then, and goes on in machine code from the round it is at.
//!
//! What it takes: ints, floats and bools; lists of them, and lists of such lists, held in
//! variables, read with `xs[i]` and `g[y][x]`, measured with `len`, and changed as the
//! interpreter changes them in place, `xs = xs | set(i, v)`, `xs = set(xs, i, v)`,
//! `xs.set(i, v)`, `g = set(g, y, set(g[y], x, v))`, `xs = xs | push(v)` and their other
//! forms; `let`, assignment, `if`, `for` over a range, and `while`. Nothing it does can be
//! told from the interpreter's doing it:
//!
//! - An error is the interpreter's own, made from the same values (`binop`, `set_of`): an
//!   int that overflows, a `%` by zero, NaN compared, a `set` past the end, a step of 0.
//! - Reading an item of another type than the code was made for, or past the end, gives
//!   the round back to the interpreter as it was when the round began: numbers are put
//!   back as they were then. A list written cannot be put back, so a read must not follow
//!   a write in a round; a loop where one could is not compiled.
//! - A list it changes is taken out of its variable while it runs, so nothing else holds it
//!   and it is changed where it is; one it only reads is held. Both are put back after.
//! - It runs only where nothing else watches: not in a script with a budget, under the
//!   debugger, with `--max-compute`, or while a view's reads are followed.

use crate::ast::*;
use crate::interp::{binop, call_on, same_expr, Ctrl, Interp};
use crate::jit::{arith_type, with_module, T};
use crate::sym::Sym;
use crate::value::{self, Env, Value};
use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{types, AbiParam, Block, InstBuilder, MemFlagsData, Signature, Type, UserFuncName, Value as V};
use cranelift_codegen::isa::CallConv;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::Module;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

/// Rounds a loop runs in the interpreter before it is compiled: a short loop never is.
pub const WARM: u64 = 256;

/// A variable from around the loop, as its code takes it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Kind {
    Num(T),
    /// A list, and what its first item is, if it has one.
    List(Option<Item>),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Item {
    Num(T),
    /// A list of numbers, by its first.
    List(T),
}

fn kind_of(v: &Value) -> Option<Kind> {
    Some(match v {
        Value::List(l) => Kind::List(l.first().and_then(|x| match x {
            Value::List(inner) => inner.first().and_then(T::of).map(Item::List),
            x => T::of(x).map(Item::Num),
        })),
        v => Kind::Num(T::of(v)?),
    })
}

/// How a run of a loop's code ended.
pub(crate) enum Ran {
    Done,
    /// Given back at this round (a range's count; a `while` asks its condition again).
    Resume(i64),
    Failed(Ctrl),
}

/// What compiled code and its helpers share. The numbers first, at fixed places.
#[repr(C)]
struct Cx {
    k: i64,
    end: i64,
    step: i64,
    miss: u64,
    err: Option<Ctrl>,
}

const K: i32 = 0;
const END: i32 = 8;
const STEP: i32 = 16;
const MISS: i32 = 24;

type Entry = unsafe extern "C" fn(*mut u64, *mut Value, *mut Cx) -> u64;

/// What a loop uses from around it, worked out once.
struct Shape {
    /// The names it reads or assigns and does not bind, and whether it assigns each.
    free: Vec<(Sym, bool)>,
    /// Builtins it calls by name: each must be the builtin where it runs.
    builtins: Vec<Sym>,
}

struct Code {
    entry: Entry,
}

type ShapeKey = (u64, usize);
type CodeKey = (u64, usize, Vec<Kind>);

thread_local! {
    static SHAPES: RefCell<HashMap<ShapeKey, Option<Arc<Shape>>>> = RefCell::new(HashMap::new());
    static CODE: RefCell<HashMap<CodeKey, Option<Arc<Code>>>> = RefCell::new(HashMap::new());
}

/// How many loops this thread has compiled (for tests).
pub fn compiled() -> usize {
    CODE.with(|c| c.borrow().values().filter(|c| c.is_some()).count())
}

/// Runs `node`, a `for` over a range at round `range.0` or a `while`, in machine code from
/// here, if it can be: None to go on in the interpreter.
pub(crate) fn run(it: &mut Interp, node: &Expr, env: &Env, range: Option<(i64, i64, i64)>) -> Option<Ran> {
    let id = it.code_id;
    let skey = (id, node as *const Expr as usize);
    let shape = SHAPES.with(|s| s.borrow_mut().entry(skey).or_insert_with(|| shape_of(node).map(Arc::new)).clone())?;
    for b in &shape.builtins {
        if !matches!(value::builtin_named(env, *b), Some((n, _)) if n == b.as_str()) {
            return None;
        }
    }
    let mut kinds = Vec::with_capacity(shape.free.len());
    for (n, written) in &shape.free {
        let k = value::with_var(env, *n, kind_of)??;
        // What it assigns must be its to assign: a local, or a global while the program loads.
        if *written && !value::takable(env, *n) {
            return None;
        }
        kinds.push(k);
    }
    let ckey = (id, skey.1, kinds.clone());
    let code = match CODE.with(|c| c.borrow().get(&ckey).cloned()) {
        Some(c) => c?,
        None => {
            let made = compile(node, &shape, &kinds).map(Arc::new);
            CODE.with(|c| c.borrow_mut().insert(ckey, made.clone()));
            made?
        }
    };
    let mut scalars = vec![0u64; shape.free.len()];
    let mut lists: Vec<Value> = Vec::new();
    for (i, ((n, written), k)) in shape.free.iter().zip(&kinds).enumerate() {
        match k {
            Kind::Num(_) => scalars[i] = value::with_var(env, *n, bits_of)??,
            // Taken, so that it is changed where it is.
            Kind::List(_) if *written => lists.push(value::take_local(env, *n)?),
            Kind::List(_) => lists.push(value::lookup(env, *n)?),
        }
    }
    let (k, end, step) = range.unwrap_or((0, 0, 1));
    let mut cx = Cx { k, end, step, miss: 0, err: None };
    // SAFETY: the code was made for exactly these kinds: a number for each in `scalars`, a
    // list for each in `lists`, in order, which outlive the call, as `cx` does.
    let status = unsafe { (code.entry)(scalars.as_mut_ptr(), lists.as_mut_ptr(), &mut cx) };
    let mut lists = lists.into_iter();
    for (i, ((n, written), k)) in shape.free.iter().zip(&kinds).enumerate() {
        match k {
            Kind::Num(t) if *written => {
                value::assign(env, *n, value_of(*t, scalars[i]));
            }
            Kind::Num(_) => {}
            Kind::List(_) => {
                let v = lists.next().unwrap_or(Value::Nil);
                if *written {
                    value::assign(env, *n, v);
                }
            }
        }
    }
    Some(match status {
        0 => Ran::Done,
        1 => Ran::Resume(cx.k),
        _ => Ran::Failed(cx.err.take().unwrap_or_else(|| Ctrl::Error(Box::new(crate::error::QuilloError::new("a compiled loop failed unexpectedly", 0))))),
    })
}

fn bits_of(v: &Value) -> Option<u64> {
    Some(match v {
        Value::Int(n) => *n as u64,
        Value::Float(f) => f.to_bits(),
        Value::Bool(b) => *b as u64,
        _ => return None,
    })
}

fn value_of(t: T, bits: u64) -> Value {
    match t {
        T::I => Value::Int(bits as i64),
        T::F => Value::Float(f64::from_bits(bits)),
        T::B => Value::Bool(bits != 0),
    }
}

fn tag(t: T) -> i64 {
    match t {
        T::I => 0,
        T::F => 1,
        T::B => 2,
    }
}

fn of_tag(tag: u64) -> T {
    match tag {
        0 => T::I,
        1 => T::F,
        _ => T::B,
    }
}

// ---- what a loop is made of ----------------------------------------------------------------

/// An update of a list, as the interpreter does one in place.
enum Update<'a> {
    Set1 { i: &'a Expr, v: &'a Expr, line: u32 },
    Set2 { y: &'a Expr, x: &'a Expr, v: &'a Expr, line: u32, inner: u32 },
    Push { v: &'a Expr },
}

fn update<'a>(name: Sym, op: &AssignOp, e: &'a Expr) -> Option<Update<'a>> {
    if !matches!(op, AssignOp::Set) {
        return None;
    }
    let (f, recv, rest, line) = call_on(e)?;
    if !matches!(recv, Expr::Ident(n, _) if *n == name) {
        return None;
    }
    match (f.as_str(), rest) {
        ("push", [v]) => Some(Update::Push { v }),
        ("set", [y, inner]) => {
            if let Some((g, Expr::Index(o, y2, _), [x, v], inner_line)) = call_on(inner) {
                if g.as_str() == "set" && matches!(&**o, Expr::Ident(n, _) if *n == name) && same_expr(y, y2) {
                    return Some(Update::Set2 { y, x, v, line, inner: inner_line });
                }
            }
            Some(Update::Set1 { i: y, v: inner, line })
        }
        _ => None,
    }
}

/// `a..b`, `range(b)`, `range(a, b)` or `range(a, b, step)`: its bounds and step.
fn range_parts(iter: &Expr) -> Option<(Option<&Expr>, &Expr, Option<&Expr>)> {
    match iter {
        Expr::Binary(BinOp::Range, a, b, _) => Some((Some(a), b, None)),
        Expr::Call(f, args, _) if matches!(&**f, Expr::Ident(n, _) if n == "range") => match args.as_slice() {
            [b] => Some((None, b, None)),
            [a, b] => Some((Some(a), b, None)),
            [a, b, s] => Some((Some(a), b, Some(s))),
            _ => None,
        },
        _ => None,
    }
}

fn is_range_call(iter: &Expr) -> bool {
    matches!(iter, Expr::Call(..))
}

/// Whether running `e` may write into a list.
fn writes(e: &Expr) -> bool {
    let mut found = false;
    crate::check::walk_expr(e, &mut |x| {
        if let Expr::Block(ss) = x {
            for s in ss {
                if let Stmt::Assign(n, op, e, _) = s {
                    found |= update(*n, op, e).is_some();
                }
            }
        }
    });
    found
}

struct Walk {
    bound: Vec<Vec<Sym>>,
    free: Vec<(Sym, bool)>,
    builtins: Vec<Sym>,
}

impl Walk {
    fn use_name(&mut self, n: Sym, write: bool) {
        if self.bound.iter().any(|s| s.contains(&n)) {
            return;
        }
        match self.free.iter_mut().find(|(m, _)| *m == n) {
            Some(f) => f.1 |= write,
            None => self.free.push((n, write)),
        }
    }

    fn builtin(&mut self, n: &str) {
        let n = Sym::new(n);
        if !self.builtins.contains(&n) {
            self.builtins.push(n);
        }
    }

    fn bind(&mut self, p: &Pattern) -> Option<()> {
        match p {
            Pattern::Bind(n) => self.bound.last_mut()?.push(*n),
            Pattern::Wild => {}
            _ => return None,
        }
        Some(())
    }

    /// A list where one is wanted: a variable, or `g[y]`.
    fn list(&mut self, e: &Expr) -> Option<()> {
        match e {
            Expr::Ident(n, _) => self.use_name(*n, false),
            Expr::Index(o, y, _) => {
                let Expr::Ident(n, _) = &**o else { return None };
                self.use_name(*n, false);
                self.expr(y)?;
            }
            _ => return None,
        }
        Some(())
    }

    fn expr(&mut self, e: &Expr) -> Option<()> {
        match e {
            Expr::Lit(Lit::Int(_) | Lit::Float(_) | Lit::Bool(_)) => {}
            Expr::Ident(n, _) => self.use_name(*n, false),
            Expr::Unary(_, x, _) => self.expr(x)?,
            Expr::Binary(op, a, b, _) if *op != BinOp::Range => {
                self.expr(a)?;
                self.expr(b)?;
            }
            Expr::If(c, a, b) => {
                self.expr(c)?;
                self.expr(a)?;
                if let Some(b) = b {
                    self.expr(b)?;
                }
            }
            Expr::Block(ss) => {
                self.bound.push(Vec::new());
                for s in ss {
                    self.stmt(s)?;
                }
                self.bound.pop();
            }
            Expr::Index(o, i, _) => {
                self.list(o)?;
                self.expr(i)?;
            }
            Expr::Call(f, args, _) => {
                let (Expr::Ident(n, _), [x]) = (&**f, args.as_slice()) else { return None };
                if n != "len" {
                    return None;
                }
                self.builtin("len");
                self.list(x)?;
            }
            Expr::For(pat, iter, body, _) => {
                let (a, b, s) = range_parts(iter)?;
                if is_range_call(iter) {
                    self.builtin("range");
                }
                for x in [a, Some(b), s].into_iter().flatten() {
                    self.expr(x)?;
                }
                self.bound.push(Vec::new());
                self.bind(pat)?;
                self.expr(body)?;
                self.bound.pop();
            }
            Expr::While(c, body, _) => {
                self.expr(c)?;
                self.bound.push(Vec::new());
                self.expr(body)?;
                self.bound.pop();
            }
            _ => return None,
        }
        Some(())
    }

    fn stmt(&mut self, s: &Stmt) -> Option<()> {
        match s {
            Stmt::Expr(e) => self.expr(e)?,
            Stmt::Let(Pattern::Bind(n), e, _) => {
                self.expr(e)?;
                self.bound.last_mut()?.push(*n);
            }
            Stmt::Assign(n, op, e, _) => {
                match update(*n, op, e) {
                    Some(u) => {
                        let parts: Vec<&Expr> = match u {
                            Update::Set1 { i, v, .. } => {
                                self.builtin("set");
                                vec![i, v]
                            }
                            Update::Set2 { y, x, v, .. } => {
                                self.builtin("set");
                                vec![y, x, v]
                            }
                            Update::Push { v } => {
                                self.builtin("push");
                                vec![v]
                            }
                        };
                        for x in parts {
                            self.expr(x)?;
                        }
                    }
                    None => self.expr(e)?,
                }
                self.use_name(*n, true);
            }
            _ => return None,
        }
        Some(())
    }
}

/// What a loop takes from around it, if it is made only of what compiled code does.
fn shape_of(node: &Expr) -> Option<Shape> {
    let mut w = Walk { bound: vec![Vec::new()], free: Vec::new(), builtins: Vec::new() };
    match node {
        Expr::For(pat, iter, body, _) => {
            range_parts(iter)?;
            w.bound.push(Vec::new());
            w.bind(pat)?;
            w.expr(body)?;
        }
        Expr::While(c, body, _) => {
            w.expr(c)?;
            w.bound.push(Vec::new());
            w.expr(body)?;
        }
        _ => return None,
    }
    (w.free.len() <= 64).then_some(Shape { free: w.free, builtins: w.builtins })
}

// ---- the helpers compiled code calls ------------------------------------------------------

fn at(n: usize, i: i64) -> Option<usize> {
    let n = n as i64;
    let i = if i < 0 { n + i } else { i };
    (i >= 0 && i < n).then_some(i as usize)
}

fn want(v: &Value, t: u64) -> Option<u64> {
    match (v, t) {
        (Value::Int(n), 0) => Some(*n as u64),
        (Value::Float(f), 1) => Some(f.to_bits()),
        (Value::Bool(b), 2) => Some(*b as u64),
        _ => None,
    }
}

fn miss(cx: *mut Cx) -> u64 {
    // SAFETY: `cx` is the run's, alive for its call.
    unsafe { (*cx).miss = 1 };
    0
}

/// `xs[i]`, of the type the code wants, or a miss.
unsafe extern "C" fn get1(list: *const Value, i: i64, t: u64, cx: *mut Cx) -> u64 {
    // SAFETY: `list` is one of the run's lists.
    let found = match unsafe { &*list } {
        Value::List(l) => at(l.len(), i).and_then(|i| want(&l[i], t)),
        _ => None,
    };
    found.unwrap_or_else(|| miss(cx))
}

/// `g[y][x]`, as `get1`.
unsafe extern "C" fn get2(list: *const Value, y: i64, x: i64, t: u64, cx: *mut Cx) -> u64 {
    // SAFETY: as `get1`.
    let found = match unsafe { &*list } {
        Value::List(l) => at(l.len(), y).and_then(|y| match &l[y] {
            Value::List(row) => at(row.len(), x).and_then(|x| want(&row[x], t)),
            _ => None,
        }),
        _ => None,
    };
    found.unwrap_or_else(|| miss(cx))
}

unsafe extern "C" fn len1(list: *const Value) -> i64 {
    // SAFETY: as `get1`.
    match unsafe { &*list } {
        Value::List(l) => l.len() as i64,
        _ => 0,
    }
}

/// `len(g[y])`, or a miss when `g[y]` is not a list.
unsafe extern "C" fn len2(list: *const Value, y: i64, cx: *mut Cx) -> u64 {
    // SAFETY: as `get1`.
    let found = match unsafe { &*list } {
        Value::List(l) => at(l.len(), y).and_then(|y| match &l[y] {
            Value::List(row) => Some(row.len() as u64),
            _ => None,
        }),
        _ => None,
    };
    found.unwrap_or_else(|| miss(cx))
}

fn failed(cx: *mut Cx, e: Ctrl) -> u64 {
    // SAFETY: as `miss`.
    unsafe { (*cx).err = Some(e) };
    2
}

/// `xs = set(xs, i, v)`: where it is, or the error `set` gives.
unsafe extern "C" fn set1(list: *mut Value, i: i64, t: u64, bits: u64, line: u64, cx: *mut Cx) -> u64 {
    let v = value_of(of_tag(t), bits);
    // SAFETY: `list` is one of the run's lists, written by nothing else while it runs.
    let slot = unsafe { &mut *list };
    if let Value::List(l) = slot {
        if let Some(i) = at(l.len(), i) {
            Arc::make_mut(l)[i] = v;
            return 0;
        }
    }
    match crate::builtins::set_of(vec![slot.clone(), Value::Int(i), v], line as u32) {
        Ok(now) => {
            *slot = now;
            0
        }
        Err(e) => failed(cx, e),
    }
}

/// `g = set(g, y, set(g[y], x, v))`: where it is, or as the interpreter does it the long way.
#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn set2(list: *mut Value, y: i64, x: i64, t: u64, bits: u64, line: u64, inner_line: u64, cx: *mut Cx) -> u64 {
    let v = value_of(of_tag(t), bits);
    // SAFETY: as `set1`.
    let slot = unsafe { &mut *list };
    if value::update_inside(slot, &Value::Int(y), None, vec![Value::Int(x), v.clone()]) {
        return 0;
    }
    let row = match &*slot {
        Value::List(l) => at(l.len(), y).map_or(Value::Nil, |y| l[y].clone()),
        _ => Value::Nil,
    };
    let row = match crate::builtins::set_of(vec![row, Value::Int(x), v], inner_line as u32) {
        Ok(r) => r,
        Err(e) => return failed(cx, e),
    };
    match crate::builtins::set_of(vec![slot.clone(), Value::Int(y), row], line as u32) {
        Ok(now) => {
            *slot = now;
            0
        }
        Err(e) => failed(cx, e),
    }
}

unsafe extern "C" fn push1(list: *mut Value, t: u64, bits: u64) {
    // SAFETY: as `set1`.
    if let Value::List(l) = unsafe { &mut *list } {
        Arc::make_mut(l).push(value_of(of_tag(t), bits));
    }
}

const OPS: [BinOp; 10] = [BinOp::Add, BinOp::Sub, BinOp::Mul, BinOp::Div, BinOp::Mod, BinOp::Lt, BinOp::Le, BinOp::Gt, BinOp::Ge, BinOp::Eq];

/// The interpreter's error for `a op b`, which the code found it would give.
unsafe extern "C" fn arith_failed(op: u64, ta: u64, a: u64, tb: u64, b: u64, line: u64, cx: *mut Cx) {
    let e = match binop(OPS[op as usize], value_of(of_tag(ta), a), value_of(of_tag(tb), b), line as u32) {
        Err(e) => e,
        Ok(_) => Ctrl::Error(Box::new(crate::error::QuilloError::new("a compiled loop failed where the interpreter would not", line as u32))),
    };
    failed(cx, e);
}

/// An error of the interpreter's own words: an int's overflow (0), a step of 0 (1).
unsafe extern "C" fn plain_failed(what: u64, line: u64, cx: *mut Cx) {
    let msg = if what == 0 { "integer overflow" } else { "`range`: a step of 0 never gets anywhere" };
    failed(cx, Ctrl::Error(Box::new(crate::error::QuilloError::new(msg, line as u32))));
}

// ---- making the code ------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Var {
    Num(Variable, T),
    /// A list: its place among the run's lists.
    List(u32),
}

struct Gen<'f> {
    b: FunctionBuilder<'f>,
    cc: CallConv,
    ptr: Type,
    lists: V,
    cx: V,
    sc: Vec<Vec<(Sym, Var)>>,
    /// Each list's kind, by its place.
    items: Vec<Option<Item>>,
    /// A list may have been written in this round: a read cannot give the round back.
    written: bool,
    bail: Block,
    fail: Block,
}

fn compile(node: &Expr, shape: &Shape, kinds: &[Kind]) -> Option<Code> {
    with_module(|module| {
        let ptr = module.target_config().pointer_type();
        let cc = module.target_config().default_call_conv;
        let mut sig = module.make_signature();
        for _ in 0..3 {
            sig.params.push(AbiParam::new(ptr));
        }
        sig.returns.push(AbiParam::new(types::I64));
        let id = module.declare_anonymous_function(&sig).ok()?;
        let mut ctx = module.make_context();
        ctx.func.signature = sig;
        ctx.func.name = UserFuncName::user(1, id.as_u32());
        let mut fctx = FunctionBuilderContext::new();
        {
            let b = FunctionBuilder::new(&mut ctx.func, &mut fctx);
            let mut g = Gen { b, cc, ptr, lists: V::from_u32(0), cx: V::from_u32(0), sc: vec![Vec::new()], items: Vec::new(), written: false, bail: Block::from_u32(0), fail: Block::from_u32(0) };
            g.function(node, shape, kinds)?;
            g.b.seal_all_blocks();
            g.b.finalize(module.target_config());
        }
        module.define_function(id, &mut ctx).ok()?;
        module.clear_context(&mut ctx);
        module.finalize_definitions().ok()?;
        let code = module.get_finalized_function(id);
        // SAFETY: the function was made with exactly this signature.
        Some(Code { entry: unsafe { std::mem::transmute::<*const u8, Entry>(code) } })
    })
}

impl Gen<'_> {
    fn function(&mut self, node: &Expr, shape: &Shape, kinds: &[Kind]) -> Option<()> {
        let entry = self.b.create_block();
        self.b.append_block_params_for_function_params(entry);
        self.b.switch_to_block(entry);
        let params = self.b.block_params(entry).to_vec();
        let (scalars, lists, cx) = (params[0], params[1], params[2]);
        self.lists = lists;
        self.cx = cx;
        // The variables from around, and for those it assigns, their values as a round began.
        let mut outs = Vec::new();
        for (i, ((n, written), k)) in shape.free.iter().zip(kinds).enumerate() {
            match k {
                Kind::Num(t) => {
                    let raw = self.b.ins().load(types::I64, MemFlagsData::trusted(), scalars, (i * 8) as i32);
                    let v = self.from_bits(*t, raw);
                    let var = self.b.declare_var(t.ir());
                    self.b.def_var(var, v);
                    self.sc[0].push((*n, Var::Num(var, *t)));
                    if *written {
                        let snap = self.b.declare_var(t.ir());
                        self.b.def_var(snap, v);
                        outs.push((var, snap, *t, i));
                    }
                }
                Kind::List(item) => {
                    self.sc[0].push((*n, Var::List(self.items.len() as u32)));
                    self.items.push(*item);
                }
            }
        }
        self.bail = self.b.create_block();
        self.fail = self.b.create_block();
        let done = self.b.create_block();
        let head = self.b.create_block();
        let body = self.b.create_block();
        let snapshot = |g: &mut Self| {
            for (var, snap, _, _) in &outs {
                let v = g.b.use_var(*var);
                g.b.def_var(*snap, v);
            }
        };
        let k = self.b.declare_var(types::I64);
        match node {
            Expr::For(pat, _, inner, _) => {
                let start = self.b.ins().load(types::I64, MemFlagsData::trusted(), cx, K);
                let end = self.b.ins().load(types::I64, MemFlagsData::trusted(), cx, END);
                let step = self.b.ins().load(types::I64, MemFlagsData::trusted(), cx, STEP);
                self.b.def_var(k, start);
                self.b.ins().jump(head, &[]);
                self.b.switch_to_block(head);
                let kv = self.b.use_var(k);
                let more = self.counting(kv, end, step);
                self.b.ins().brif(more, body, &[], done, &[]);
                self.b.switch_to_block(body);
                snapshot(self);
                self.written = false;
                self.sc.push(Vec::new());
                if let Pattern::Bind(n) = pat {
                    let var = self.b.declare_var(types::I64);
                    self.b.def_var(var, kv);
                    self.sc.last_mut()?.push((*n, Var::Num(var, T::I)));
                }
                self.stmt_expr(inner)?;
                self.sc.pop();
                self.next(k, kv, step, head, done);
            }
            Expr::While(c, inner, _) => {
                self.b.ins().jump(head, &[]);
                self.b.switch_to_block(head);
                snapshot(self);
                self.written = false;
                let (cv, t) = self.value(c)?;
                if t != T::B {
                    return None;
                }
                self.b.ins().brif(cv, body, &[], done, &[]);
                self.b.switch_to_block(body);
                self.sc.push(Vec::new());
                self.stmt_expr(inner)?;
                self.sc.pop();
                self.b.ins().jump(head, &[]);
            }
            _ => return None,
        }
        // Done: what it assigned, as it is now.
        self.b.switch_to_block(done);
        for (var, _, t, i) in &outs {
            let v = self.b.use_var(*var);
            let raw = self.to_bits(*t, v);
            self.b.ins().store(MemFlagsData::trusted(), raw, scalars, (*i * 8) as i32);
        }
        let zero = self.b.ins().iconst(types::I64, 0);
        self.b.ins().return_(&[zero]);
        // Failed, the error in `cx`: as it is now, as the interpreter would have it.
        self.b.switch_to_block(self.fail);
        for (var, _, t, i) in &outs {
            let v = self.b.use_var(*var);
            let raw = self.to_bits(*t, v);
            self.b.ins().store(MemFlagsData::trusted(), raw, scalars, (*i * 8) as i32);
        }
        let two = self.b.ins().iconst(types::I64, 2);
        self.b.ins().return_(&[two]);
        // Given back: as the round began, at that round.
        self.b.switch_to_block(self.bail);
        for (_, snap, t, i) in &outs {
            let v = self.b.use_var(*snap);
            let raw = self.to_bits(*t, v);
            self.b.ins().store(MemFlagsData::trusted(), raw, scalars, (*i * 8) as i32);
        }
        let kv = self.b.use_var(k);
        self.b.ins().store(MemFlagsData::trusted(), kv, cx, K);
        let one = self.b.ins().iconst(types::I64, 1);
        self.b.ins().return_(&[one]);
        Some(())
    }

    fn from_bits(&mut self, t: T, raw: V) -> V {
        match t {
            T::I => raw,
            T::F => self.b.ins().bitcast(types::F64, MemFlagsData::new(), raw),
            T::B => self.b.ins().ireduce(types::I8, raw),
        }
    }

    fn to_bits(&mut self, t: T, v: V) -> V {
        match t {
            T::I => v,
            T::F => self.b.ins().bitcast(types::I64, MemFlagsData::new(), v),
            T::B => self.b.ins().uextend(types::I64, v),
        }
    }

    /// Whether a count of `step` from `k` has not reached `end`.
    fn counting(&mut self, k: V, end: V, step: V) -> V {
        let up = self.b.ins().icmp_imm_s(IntCC::SignedGreaterThan, step, 0);
        let below = self.b.ins().icmp(IntCC::SignedLessThan, k, end);
        let above = self.b.ins().icmp(IntCC::SignedGreaterThan, k, end);
        self.b.ins().select(up, below, above)
    }

    /// The next count, or the end when it would pass the last int.
    fn next(&mut self, k: Variable, kv: V, step: V, head: Block, done: Block) {
        let r = self.b.ins().iadd(kv, step);
        let (a, b) = (self.b.ins().bxor(kv, r), self.b.ins().bxor(step, r));
        let both = self.b.ins().band(a, b);
        let over = self.b.ins().icmp_imm_s(IntCC::SignedLessThan, both, 0);
        self.b.def_var(k, r);
        self.b.ins().brif(over, done, &[], head, &[]);
    }

    fn lookup(&self, n: Sym) -> Option<Var> {
        self.sc.iter().rev().flat_map(|s| s.iter().rev()).find(|(m, _)| *m == n).map(|(_, v)| *v)
    }

    fn list_addr(&mut self, slot: u32) -> V {
        self.b.ins().iadd_imm_s(self.lists, slot as i64 * std::mem::size_of::<Value>() as i64)
    }

    /// Calls `f`, a helper, with `args`.
    fn helper(&mut self, f: usize, args: &[V], ret: bool) -> Option<V> {
        let mut sig = Signature::new(self.cc);
        for a in args {
            sig.params.push(AbiParam::new(self.b.func.dfg.value_type(*a)));
        }
        if ret {
            sig.returns.push(AbiParam::new(types::I64));
        }
        let sref = self.b.import_signature(sig);
        let callee = self.b.ins().iconst(self.ptr, f as i64);
        let call = self.b.ins().call_indirect(sref, callee, args);
        self.b.inst_results(call).first().copied()
    }

    /// Goes to `to` when `cond` holds.
    fn leave_if(&mut self, cond: V, to: Block) {
        let go = self.b.create_block();
        self.b.ins().brif(cond, to, &[], go, &[]);
        self.b.switch_to_block(go);
    }

    /// After a helper that may have missed: the round back to the interpreter, if it did.
    fn missed(&mut self) -> Option<()> {
        if self.written {
            return None;
        }
        let m = self.b.ins().load(types::I64, MemFlagsData::trusted(), self.cx, MISS);
        self.leave_if(m, self.bail);
        Some(())
    }

    /// Fails with the interpreter's error for `x op y`.
    fn arith_fail(&mut self, op: BinOp, ta: T, tb: T, x: V, y: V, line: u32) {
        let i = OPS.iter().position(|o| *o == op).unwrap_or(0);
        let (xa, yb) = (self.to_bits(ta, x), self.to_bits(tb, y));
        let args = [
            self.b.ins().iconst(types::I64, i as i64),
            self.b.ins().iconst(types::I64, tag(ta)),
            xa,
            self.b.ins().iconst(types::I64, tag(tb)),
            yb,
            self.b.ins().iconst(types::I64, line as i64),
            self.cx,
        ];
        self.helper(arith_failed as *const () as usize, &args, false);
        self.b.ins().jump(self.fail, &[]);
    }

    /// When `cond` holds, fails as `arith_fail` says.
    fn arith_check(&mut self, cond: V, op: BinOp, ta: T, tb: T, x: V, y: V, line: u32) {
        let go = self.b.create_block();
        self.arith_check_to(cond, go, op, ta, tb, x, y, line);
    }

    /// `arith_check`, going on in `go`.
    #[allow(clippy::too_many_arguments)]
    fn arith_check_to(&mut self, cond: V, go: Block, op: BinOp, ta: T, tb: T, x: V, y: V, line: u32) {
        let bad = self.b.create_block();
        self.b.ins().brif(cond, bad, &[], go, &[]);
        self.b.switch_to_block(bad);
        self.arith_fail(op, ta, tb, x, y, line);
        self.b.switch_to_block(go);
    }

    fn plain_check(&mut self, cond: V, what: i64, line: u32) {
        let (bad, go) = (self.b.create_block(), self.b.create_block());
        self.b.ins().brif(cond, bad, &[], go, &[]);
        self.b.switch_to_block(bad);
        let args = [self.b.ins().iconst(types::I64, what), self.b.ins().iconst(types::I64, line as i64), self.cx];
        self.helper(plain_failed as *const () as usize, &args, false);
        self.b.ins().jump(self.fail, &[]);
        self.b.switch_to_block(go);
    }

    /// A list where one is wanted: a variable's place, and `g[y]`'s `y` when it is that.
    fn list(&mut self, e: &Expr) -> Option<(u32, Option<V>)> {
        match e {
            Expr::Ident(n, _) => match self.lookup(*n)? {
                Var::List(slot) => Some((slot, None)),
                Var::Num(..) => None,
            },
            Expr::Index(o, y, _) => {
                let Expr::Ident(n, _) = &**o else { return None };
                let Var::List(slot) = self.lookup(*n)? else { return None };
                let (yv, t) = self.value(y)?;
                (t == T::I).then_some((slot, Some(yv)))
            }
            _ => None,
        }
    }

    fn value(&mut self, e: &Expr) -> Option<(V, T)> {
        use T::*;
        Some(match e {
            Expr::Lit(Lit::Int(n)) => (self.b.ins().iconst(types::I64, *n), I),
            Expr::Lit(Lit::Float(f)) => (self.b.ins().f64const(*f), F),
            Expr::Lit(Lit::Bool(x)) => (self.b.ins().iconst(types::I8, *x as i64), B),
            Expr::Ident(n, _) => match self.lookup(*n)? {
                Var::Num(var, t) => (self.b.use_var(var), t),
                Var::List(_) => return None,
            },
            Expr::Unary(UnOp::Neg, x, line) => {
                let (v, t) = self.value(x)?;
                match t {
                    I => {
                        let over = self.b.ins().icmp_imm_s(IntCC::Equal, v, i64::MIN);
                        self.plain_check(over, 0, *line);
                        (self.b.ins().ineg(v), I)
                    }
                    F => (self.b.ins().fneg(v), F),
                    B => return None,
                }
            }
            Expr::Unary(UnOp::Not, x, _) => {
                let (v, t) = self.value(x)?;
                if t != B {
                    return None;
                }
                (self.b.ins().bxor_imm_s(v, 1), B)
            }
            Expr::Binary(op @ (BinOp::And | BinOp::Or), a, b, _) => {
                let (x, ta) = self.value(a)?;
                if ta != B {
                    return None;
                }
                let rhs = self.b.create_block();
                let join = self.b.create_block();
                let out = self.b.append_block_param(join, types::I8);
                if *op == BinOp::And {
                    self.b.ins().brif(x, rhs, &[], join, &[x.into()]);
                } else {
                    self.b.ins().brif(x, join, &[x.into()], rhs, &[]);
                }
                self.b.switch_to_block(rhs);
                let (y, tb) = self.value(b)?;
                if tb != B {
                    return None;
                }
                self.b.ins().jump(join, &[y.into()]);
                self.b.switch_to_block(join);
                (out, B)
            }
            Expr::Binary(op, a, b, line) => {
                let (x, ta) = self.value(a)?;
                let (y, tb) = self.value(b)?;
                self.binop(*op, ta, tb, x, y, *line)?
            }
            Expr::If(c, a, Some(b)) => {
                let (cv, tc) = self.value(c)?;
                if tc != B {
                    return None;
                }
                let (yes, no, join) = (self.b.create_block(), self.b.create_block(), self.b.create_block());
                self.b.ins().brif(cv, yes, &[], no, &[]);
                let w0 = self.written;
                self.b.switch_to_block(yes);
                let (va, ta) = self.value(a)?;
                let wa = std::mem::replace(&mut self.written, w0);
                let out = self.b.append_block_param(join, ta.ir());
                self.b.ins().jump(join, &[va.into()]);
                self.b.switch_to_block(no);
                let (vb, tb) = self.value(b)?;
                if tb != ta {
                    return None;
                }
                self.b.ins().jump(join, &[vb.into()]);
                self.written |= wa;
                self.b.switch_to_block(join);
                (out, ta)
            }
            Expr::Block(ss) => {
                let (last, init) = ss.split_last()?;
                let Stmt::Expr(last) = last else { return None };
                self.sc.push(Vec::new());
                for s in init {
                    self.stmt(s)?;
                }
                let v = self.value(last);
                self.sc.pop();
                v?
            }
            Expr::Index(o, i, _) => {
                let (slot, row) = self.list(o)?;
                let (iv, ti) = self.value(i)?;
                if ti != I {
                    return None;
                }
                let t = match (self.items.get(slot as usize).copied()??, row) {
                    (Item::Num(t), None) | (Item::List(t), Some(_)) => t,
                    _ => return None,
                };
                let addr = self.list_addr(slot);
                let tv = self.b.ins().iconst(types::I64, tag(t));
                let raw = match row {
                    None => self.helper(get1 as *const () as usize, &[addr, iv, tv, self.cx], true)?,
                    Some(y) => self.helper(get2 as *const () as usize, &[addr, y, iv, tv, self.cx], true)?,
                };
                self.missed()?;
                (self.from_bits(t, raw), t)
            }
            Expr::Call(f, args, _) => {
                let (Expr::Ident(n, _), [x]) = (&**f, args.as_slice()) else { return None };
                if n != "len" {
                    return None;
                }
                let (slot, row) = self.list(x)?;
                let addr = self.list_addr(slot);
                match row {
                    None => (self.helper(len1 as *const () as usize, &[addr], true)?, I),
                    Some(y) => {
                        let r = self.helper(len2 as *const () as usize, &[addr, y, self.cx], true)?;
                        self.missed()?;
                        (r, I)
                    }
                }
            }
            _ => return None,
        })
    }

    fn binop(&mut self, op: BinOp, ta: T, tb: T, x0: V, y0: V, line: u32) -> Option<(V, T)> {
        use T::*;
        let t = arith_type(op, ta, tb)?;
        let (x, y) = match (ta, tb) {
            (I, F) => (self.b.ins().fcvt_from_sint(types::F64, x0), y0),
            (F, I) => (x0, self.b.ins().fcvt_from_sint(types::F64, y0)),
            _ => (x0, y0),
        };
        let floats = ta == F || tb == F;
        let v = match op {
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
                self.arith_check(over, op, ta, tb, x0, y0, line);
                r
            }
            BinOp::Sub => {
                let r = self.b.ins().isub(x, y);
                let (a, b) = (self.b.ins().bxor(x, y), self.b.ins().bxor(x, r));
                let both = self.b.ins().band(a, b);
                let over = self.b.ins().icmp_imm_s(IntCC::SignedLessThan, both, 0);
                self.arith_check(over, op, ta, tb, x0, y0, line);
                r
            }
            BinOp::Mul => {
                let lo = self.b.ins().imul(x, y);
                let hi = self.b.ins().smulhi(x, y);
                let sign = self.b.ins().sshr_imm_s(lo, 63);
                let over = self.b.ins().icmp(IntCC::NotEqual, hi, sign);
                self.arith_check(over, op, ta, tb, x0, y0, line);
                lo
            }
            BinOp::Div => self.b.ins().fdiv(x, y),
            BinOp::Mod => {
                let zero = self.b.ins().icmp_imm_s(IntCC::Equal, y, 0);
                self.arith_check(zero, op, ta, tb, x0, y0, line);
                // Nothing is left over from dividing by -1, the smallest int too, as the
                // interpreter has it; and no trap.
                let neg1 = self.b.ins().icmp_imm_s(IntCC::Equal, y, -1);
                let one = self.b.ins().iconst(types::I64, 1);
                let safe = self.b.ins().select(neg1, one, y);
                let r = self.b.ins().srem(x, safe);
                let nothing = self.b.ins().iconst(types::I64, 0);
                self.b.ins().select(neg1, nothing, r)
            }
            // An int with a float, exactly, in ints (FEEDBACK55.md, Q-787; branch feedback55).
            // As floats, as before, unless they could be wrong: the same float past 2^53 (rare).
            // Then the round is the interpreter's, which compares exactly, or, once a list was
            // written in it, worked out here; NaN is refused, as before.
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge if ta != tb => {
                let (i, fi, f, side) = if ta == I { (x0, x, y, op) } else { (y0, y, x, crate::jit::swapped(op)) };
                let (_, go) = crate::jit::float_needs_look(&mut self.b, f);
                let nan = self.b.ins().fcmp(FloatCC::Unordered, fi, f);
                let checked = self.b.create_block();
                self.b.set_cold_block(checked);
                self.arith_check_to(nan, checked, op, ta, tb, x0, y0, line);
                let same = self.b.ins().fcmp(FloatCC::Equal, fi, f);
                if !self.written {
                    self.b.ins().brif(same, self.bail, &[], go, &[]);
                    self.b.switch_to_block(go);
                    self.b.ins().fcmp(crate::jit::float_cc(side), fi, f)
                } else {
                    let join = self.b.create_block();
                    let out = self.b.append_block_param(join, types::I8);
                    let exact = crate::jit::int_float_same_ir(&mut self.b, side, i, f);
                    let by_float = self.b.ins().fcmp(crate::jit::float_cc(side), fi, f);
                    let r = self.b.ins().select(same, exact, by_float);
                    self.b.ins().jump(join, &[r.into()]);
                    self.b.switch_to_block(go);
                    let r = self.b.ins().fcmp(crate::jit::float_cc(side), fi, f);
                    self.b.ins().jump(join, &[r.into()]);
                    self.b.switch_to_block(join);
                    out
                }
            }
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge if floats => {
                let nan = self.b.ins().fcmp(FloatCC::Unordered, x, y);
                self.arith_check(nan, op, ta, tb, x0, y0, line);
                self.b.ins().fcmp(crate::jit::float_cc(op), x, y)
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
        };
        Some((v, t))
    }

    fn stmt(&mut self, s: &Stmt) -> Option<()> {
        match s {
            Stmt::Expr(e) => self.stmt_expr(e),
            Stmt::Let(Pattern::Bind(n), e, _) => {
                let (v, t) = self.value(e)?;
                let var = self.b.declare_var(t.ir());
                self.b.def_var(var, v);
                self.sc.last_mut()?.push((*n, Var::Num(var, t)));
                Some(())
            }
            Stmt::Assign(n, op, e, line) => {
                if let Some(u) = update(*n, op, e) {
                    return self.update(*n, u);
                }
                let Var::Num(var, t) = self.lookup(*n)? else { return None };
                let (v, te) = self.value(e)?;
                let v = match op {
                    AssignOp::Set => v,
                    AssignOp::Add | AssignOp::Sub => {
                        let cur = self.b.use_var(var);
                        let bop = if matches!(op, AssignOp::Add) { BinOp::Add } else { BinOp::Sub };
                        let (r, tr) = self.binop(bop, t, te, cur, v, *line)?;
                        if tr != t {
                            return None;
                        }
                        r
                    }
                };
                if matches!(op, AssignOp::Set) && te != t {
                    return None;
                }
                self.b.def_var(var, v);
                Some(())
            }
            _ => None,
        }
    }

    fn update(&mut self, n: Sym, u: Update) -> Option<()> {
        let Var::List(slot) = self.lookup(n)? else { return None };
        match u {
            Update::Set1 { i, v, line } => {
                let (iv, ti) = self.value(i)?;
                let (vv, tv) = self.value(v)?;
                if ti != T::I {
                    return None;
                }
                let addr = self.list_addr(slot);
                let bits = self.to_bits(tv, vv);
                let args = [addr, iv, self.b.ins().iconst(types::I64, tag(tv)), bits, self.b.ins().iconst(types::I64, line as i64), self.cx];
                let r = self.helper(set1 as *const () as usize, &args, true)?;
                self.leave_if(r, self.fail);
            }
            Update::Set2 { y, x, v, line, inner } => {
                let (yv, ty) = self.value(y)?;
                let (xv, tx) = self.value(x)?;
                let (vv, tv) = self.value(v)?;
                if ty != T::I || tx != T::I {
                    return None;
                }
                let addr = self.list_addr(slot);
                let bits = self.to_bits(tv, vv);
                let args = [
                    addr,
                    yv,
                    xv,
                    self.b.ins().iconst(types::I64, tag(tv)),
                    bits,
                    self.b.ins().iconst(types::I64, line as i64),
                    self.b.ins().iconst(types::I64, inner as i64),
                    self.cx,
                ];
                let r = self.helper(set2 as *const () as usize, &args, true)?;
                self.leave_if(r, self.fail);
            }
            Update::Push { v } => {
                let (vv, tv) = self.value(v)?;
                let addr = self.list_addr(slot);
                let bits = self.to_bits(tv, vv);
                let tg = self.b.ins().iconst(types::I64, tag(tv));
                self.helper(push1 as *const () as usize, &[addr, tg, bits], false);
            }
        }
        self.written = true;
        Some(())
    }

    fn stmt_expr(&mut self, e: &Expr) -> Option<()> {
        match e {
            Expr::If(c, a, b) => {
                let (cv, tc) = self.value(c)?;
                if tc != T::B {
                    return None;
                }
                let (yes, no, join) = (self.b.create_block(), self.b.create_block(), self.b.create_block());
                self.b.ins().brif(cv, yes, &[], no, &[]);
                let w0 = self.written;
                self.b.switch_to_block(yes);
                self.stmt_expr(a)?;
                self.b.ins().jump(join, &[]);
                let wa = std::mem::replace(&mut self.written, w0);
                self.b.switch_to_block(no);
                if let Some(b) = b {
                    self.stmt_expr(b)?;
                }
                self.b.ins().jump(join, &[]);
                self.written |= wa;
                self.b.switch_to_block(join);
                Some(())
            }
            Expr::Block(ss) => {
                self.sc.push(Vec::new());
                for s in ss {
                    self.stmt(s)?;
                }
                self.sc.pop();
                Some(())
            }
            Expr::For(pat, iter, body, line) => {
                let (a, b, s) = range_parts(iter)?;
                let start = match a {
                    Some(a) => self.int(a)?,
                    None => self.b.ins().iconst(types::I64, 0),
                };
                let end = self.int(b)?;
                let step = match s {
                    Some(s) => {
                        let sv = self.int(s)?;
                        let zero = self.b.ins().icmp_imm_s(IntCC::Equal, sv, 0);
                        self.plain_check(zero, 1, *line);
                        sv
                    }
                    None => self.b.ins().iconst(types::I64, 1),
                };
                // A body that writes is run again after its writes.
                let after = self.written || writes(body);
                self.written = after;
                let k = self.b.declare_var(types::I64);
                self.b.def_var(k, start);
                let (head, step_blk, exit) = (self.b.create_block(), self.b.create_block(), self.b.create_block());
                self.b.ins().jump(head, &[]);
                self.b.switch_to_block(head);
                let kv = self.b.use_var(k);
                let more = self.counting(kv, end, step);
                self.b.ins().brif(more, step_blk, &[], exit, &[]);
                self.b.switch_to_block(step_blk);
                self.sc.push(Vec::new());
                if let Pattern::Bind(n) = pat {
                    let var = self.b.declare_var(types::I64);
                    self.b.def_var(var, kv);
                    self.sc.last_mut()?.push((*n, Var::Num(var, T::I)));
                }
                self.stmt_expr(body)?;
                self.sc.pop();
                self.next(k, kv, step, head, exit);
                self.b.switch_to_block(exit);
                self.written = after;
                Some(())
            }
            Expr::While(c, body, _) => {
                let after = self.written || writes(body);
                self.written = after;
                let (head, step_blk, exit) = (self.b.create_block(), self.b.create_block(), self.b.create_block());
                self.b.ins().jump(head, &[]);
                self.b.switch_to_block(head);
                let (cv, tc) = self.value(c)?;
                if tc != T::B {
                    return None;
                }
                self.b.ins().brif(cv, step_blk, &[], exit, &[]);
                self.b.switch_to_block(step_blk);
                self.sc.push(Vec::new());
                self.stmt_expr(body)?;
                self.sc.pop();
                self.b.ins().jump(head, &[]);
                self.b.switch_to_block(exit);
                self.written = after;
                Some(())
            }
            e => self.value(e).map(|_| ()),
        }
    }

    fn int(&mut self, e: &Expr) -> Option<V> {
        let (v, t) = self.value(e)?;
        (t == T::I).then_some(v)
    }
}
