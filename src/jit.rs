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
use crate::value::{lookup, Closure, Env, Value};
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
enum T {
    I,
    F,
    B,
}

impl T {
    fn of(v: &Value) -> Option<T> {
        match v {
            Value::Int(_) => Some(T::I),
            Value::Float(_) => Some(T::F),
            Value::Bool(_) => Some(T::B),
            _ => None,
        }
    }

    fn ir(self) -> Type {
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
}

type Key = (usize, Vec<T>);

/// `FnDef::hot` once a function is known not to compile.
const NEVER: u32 = u32::MAX;

struct Jit {
    module: JITModule,
    done: HashMap<Key, Option<Arc<Compiled>>>,
    /// Functions whose code is made (so their addresses stay theirs).
    keep: Vec<Arc<FnDef>>,
}

thread_local! {
    static JIT: RefCell<Option<Jit>> = const { RefCell::new(None) };
}

/// How many functions this thread has compiled (for tests).
pub fn compiled() -> usize {
    JIT.with(|j| j.borrow().as_ref().map_or(0, |j| j.done.values().filter(|c| c.is_some()).count()))
}

fn new_jit() -> Option<Jit> {
    let mut flags = settings::builder();
    flags.set("use_colocated_libcalls", "false").ok()?;
    flags.set("is_pic", "false").ok()?;
    flags.set("opt_level", "speed").ok()?;
    let isa = cranelift_native::builder().ok()?.finish(settings::Flags::new(flags)).ok()?;
    Some(Jit { module: JITModule::new(JITBuilder::with_isa(isa, default_libcall_names())), done: HashMap::new(), keep: Vec::new() })
}

/// `c(args)` in machine code, when it can be: None to interpret it.
pub fn call(c: &Closure, args: &[Option<Value>], depth: usize) -> Option<Result<Value, Ctrl>> {
    let def = &c.def;
    let hot = def.hot.load(Relaxed);
    if hot == NEVER {
        return None;
    }
    // Compiled on the second call: one call alone is not worth it.
    if hot < 1 {
        def.hot.store(hot + 1, Relaxed);
        return None;
    }
    if args.len() != def.params.len() || args.len() > 16 {
        def.hot.store(NEVER, Relaxed);
        return None;
    }
    let mut sig = Vec::with_capacity(args.len());
    let mut raw = [0u64; 16];
    for (i, a) in args.iter().enumerate() {
        let a = a.as_ref()?;
        sig.push(T::of(a)?);
        raw[i] = match a {
            Value::Int(n) => *n as u64,
            Value::Float(f) => f.to_bits(),
            Value::Bool(b) => *b as u64,
            _ => return None,
        };
    }
    let compiled = JIT.with(|j| {
        let mut j = j.borrow_mut();
        if j.is_none() {
            *j = new_jit();
        }
        let jit = j.as_mut()?;
        let key = (Arc::as_ptr(def) as usize, sig.clone());
        if let Some(done) = jit.done.get(&key) {
            return done.clone();
        }
        let made = compile(jit, c, &sig).map(Arc::new);
        if made.is_none() && !shape_ok(def) {
            def.hot.store(NEVER, Relaxed);
        }
        jit.done.insert(key, made.clone());
        made
    })?;
    // What it calls is still what it was compiled with.
    for (name, at) in &compiled.deps {
        match lookup(&c.env, name) {
            Some(Value::Func(f)) if Arc::as_ptr(&f.def) as usize == *at => {}
            _ => return None,
        }
    }
    // Calls as deep as the stack left allows (a recursion deeper than that
    // gives up, and the interpreter, which grows the stack, goes on).
    let left = crate::stack::remaining().unwrap_or(64 << 10);
    let mut ctx = Ctx { bail: 0, depth: 0, max: (MAX_DEPTH.saturating_sub(depth)).min(left / 512) as u64 };
    // SAFETY: `entry` was made for exactly these argument types, read from
    // `raw` in order; `ctx` outlives the call.
    let r = unsafe { (compiled.entry)(raw.as_ptr(), &mut ctx) };
    if ctx.bail != 0 {
        return None;
    }
    Some(Ok(match compiled.ret {
        T::I => Value::Int(r as i64),
        T::F => Value::Float(f64::from_bits(r)),
        T::B => Value::Bool(r != 0),
    }))
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
    /// Being planned, with the result type assumed for calls back into it.
    open: HashMap<Key, (T, bool)>,
    deps: Vec<(String, usize)>,
    done: &'a HashMap<Key, Option<Arc<Compiled>>>,
}

impl<'a> Plan<'a> {
    fn func(&mut self, def: &Arc<FnDef>, args: &[T]) -> Option<T> {
        let key = (Arc::as_ptr(def) as usize, args.to_vec());
        if let Some(t) = self.types.get(&key) {
            return Some(*t);
        }
        if let Some(Some(c)) = self.done.get(&key) {
            return Some(c.ret);
        }
        if let Some((t, used)) = self.open.get_mut(&key) {
            *used = true;
            return Some(*t);
        }
        if !shape_ok(def) || def.params.len() != args.len() || !def.params.iter().zip(args).all(|(p, t)| ty_takes(&p.ty, *t)) {
            return None;
        }
        // A recursive function's calls of itself: its result type is
        // guessed, and the guess must come out.
        for guess in [T::I, T::F, T::B] {
            self.open.insert(key.clone(), (guess, false));
            let mut sc = Scopes { stack: vec![def.params.iter().map(|p| p.name.clone()).zip(args.iter().copied()).collect()] };
            let mut ret = None;
            let got = self.body(&def.body, &mut sc, &mut ret);
            let (_, used) = self.open.remove(&key).unwrap();
            let t = match (got, ret) {
                (Some(a), Some(b)) if a != b => None,
                (Some(a), _) => Some(a),
                (None, _) => None,
            };
            match t {
                Some(t) if (!used || t == guess) && ty_takes(&def.ret, t) => {
                    self.types.insert(key, t);
                    self.order.push((def.clone(), args.to_vec()));
                    return Some(t);
                }
                _ if !used => return None,
                _ => {}
            }
        }
        None
    }

    fn body(&mut self, e: &Expr, sc: &mut Scopes<T>, ret: &mut Option<T>) -> Option<T> {
        self.value(e, sc, ret)
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
            Expr::If(c, a, Some(b)) => {
                if self.value(c, sc, ret)? != B {
                    return None;
                }
                let x = self.value(a, sc, ret)?;
                let y = self.value(b, sc, ret)?;
                (x == y).then_some(x)
            }
            Expr::Block(ss) => {
                sc.stack.push(Vec::new());
                let t = self.block(ss, sc, ret, true);
                sc.stack.pop();
                t
            }
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
                let dep = (name.clone(), Arc::as_ptr(&callee.def) as usize);
                if !self.deps.contains(&dep) {
                    self.deps.push(dep);
                }
                Some(t)
            }
            _ => None,
        }
    }

    /// A block's statements; with `want`, the type of its value.
    fn block(&mut self, ss: &[Stmt], sc: &mut Scopes<T>, ret: &mut Option<T>, want: bool) -> Option<T> {
        let mut last = None;
        for (i, s) in ss.iter().enumerate() {
            let is_last = i + 1 == ss.len();
            last = None;
            match s {
                Stmt::Expr(e) if is_last && want => last = Some(self.value(e, sc, ret)?),
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
                }
                _ => return None,
            }
        }
        if want {
            last
        } else {
            Some(T::B)
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
            if !matches!(lookup(env, "range"), Some(Value::Builtin("range", _))) {
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
fn arith_type(op: BinOp, x: T, y: T) -> Option<T> {
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

// ---- making the code ---------------------------------------------------

fn compile(jit: &mut Jit, c: &Closure, sig: &[T]) -> Option<Compiled> {
    let mut plan = Plan { env: &c.env, types: HashMap::new(), order: Vec::new(), open: HashMap::new(), deps: Vec::new(), done: &jit.done };
    let ret = plan.func(&c.def, sig)?;
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
    for (def, args) in &order {
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
            };
            g.function(def, args)?;
            g.b.seal_all_blocks();
            g.b.finalize(tc);
        }
        jit.module.define_function(ids[&key], &mut ctx).ok()?;
        jit.module.clear_context(&mut ctx);
    }
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
    Some(Compiled { entry, ret, deps })
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
            self.b.def_var(var, params[i]);
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
                for (blk, arm) in [(yes, a), (no, b)] {
                    self.b.switch_to_block(blk);
                    let v = self.value(arm, ret)?;
                    if !self.ended {
                        self.b.ins().jump(join, &[v.into()]);
                    }
                    self.ended = false;
                }
                self.b.switch_to_block(join);
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
                let id = match (self.ids.get(&key), self.done.get(&key)) {
                    (Some(id), _) => *id,
                    _ => return None,
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
            Expr::If(_, a, _) => self.type_of(a),
            Expr::Block(ss) => {
                // Its last expression's type, with the block's own `let`s seen.
                let mut sc = Scopes { stack: self.sc.stack.iter().map(|s| s.iter().map(|(n, (_, t))| (n.clone(), *t)).collect()).collect() };
                sc.stack.push(Vec::new());
                for s in ss {
                    match s {
                        Stmt::Let(Pattern::Bind(n), e, _) => {
                            let t = type_in(e, &sc, self)?;
                            sc.set(n, t);
                        }
                        Stmt::Expr(e) if std::ptr::eq(s, ss.last()?) => return type_in(e, &sc, self),
                        _ => {}
                    }
                }
                None
            }
            Expr::Call(f, args, _) => {
                let Expr::Ident(name, _) = &**f else { return None };
                let Some(Value::Func(callee)) = lookup(self.env, name) else { return None };
                let ts: Option<Vec<T>> = args.iter().map(|a| self.type_of(a)).collect();
                let key = (Arc::as_ptr(&callee.def) as usize, ts?);
                self.types.get(&key).copied().or_else(|| self.done.get(&key).and_then(|c| c.as_ref().map(|c| c.ret)))
            }
            _ => None,
        }
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
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge if floats => {
                // NaN cannot be compared: the interpreter fails there.
                let nan = self.b.ins().fcmp(FloatCC::Unordered, x, y);
                self.check(nan);
                let cc = match op {
                    BinOp::Lt => FloatCC::LessThan,
                    BinOp::Le => FloatCC::LessThanOrEqual,
                    BinOp::Gt => FloatCC::GreaterThan,
                    _ => FloatCC::GreaterThanOrEqual,
                };
                self.b.ins().fcmp(cc, x, y)
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
                    self.b.def_var(var, v);
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
                    self.b.def_var(var, v);
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
                // As the interpreter: at most ten million steps.
                let span = self.b.ins().isub(end, start);
                let (p, q) = (self.b.ins().bxor(end, start), self.b.ins().bxor(end, span));
                let both = self.b.ins().band(p, q);
                let wrapped = self.b.ins().icmp_imm_s(IntCC::SignedLessThan, both, 0);
                self.check(wrapped);
                let big = self.b.ins().icmp_imm_s(IntCC::SignedGreaterThan, span, 10_000_000);
                self.check(big);
                let k = self.b.declare_var(types::I64);
                self.b.def_var(k, start);
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
                    self.b.def_var(var, kv);
                    self.sc.set(n, (var, T::I));
                }
                let r = self.stmt_expr(body, ret);
                self.sc.stack.pop();
                r?;
                if !self.ended {
                    let next = self.b.ins().iadd_imm_s(kv, 1);
                    self.b.def_var(k, next);
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
        Expr::If(_, a, _) => type_in(a, sc, g),
        _ => g.type_of(e),
    }
}
