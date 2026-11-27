//! Small generated programs of ints, floats and bools (lets, assignment, `if`, `for` over a range,
//! `while`, `return`, and calls of a second such function), run three times as a program's
//! functions, where the JIT (src/jit.rs) compiles them, and once as a script, where the
//! interpreter runs them under its steps: the same answers, or a failure in both
//! (FEEDBACK49.md, Q-608; branch feedback86-89).
//!
//! The JIT is never used under `--max-compute` or a script's steps, so each program is made to
//! end by itself instead: loops count to at most 12, nested at most 3 deep, and no function calls
//! itself. With `QUILLO_FUZZ_STATS=1`, every 10,000 programs it says how many it compared and how
//! many were compiled.
#![no_main]

use arbitrary::{Arbitrary, Unstructured};
use libfuzzer_sys::fuzz_target;
use quillo::interp::Ctrl;
use quillo::value::{Map, Value};
use quillo::Interp;
use std::cell::{Cell, RefCell};

#[derive(Clone, Copy, PartialEq, Debug)]
enum Ty {
    I,
    F,
    B,
}

const INTS: &[&str] = &["0", "1", "2", "3", "7", "10", "100", "65536", "2147483647", "4611686018427387904", "9223372036854775807", "(-1)", "(-9223372036854775807 - 1)"];
const FLOATS: &[&str] = &["0.0", "0.5", "1.0", "1.5", "2.0", "3.0", "10.0", "0.1", "1000000.0", "123456789.25", "(-0.0)", "(-2.5)"];

struct Var {
    name: String,
    ty: Ty,
    /// Lets only: not a parameter, a loop's counter or a `while`'s guard.
    assignable: bool,
}

struct Gen<'a, 'b> {
    u: &'b mut Unstructured<'a>,
    scopes: Vec<Vec<Var>>,
    next: usize,
    /// Statements left to write, for both functions.
    budget: usize,
    /// Loops the statement being written is in.
    loops: usize,
    /// `g`'s parameters and answer, once written: `f` may call it.
    helper: Option<(Vec<Ty>, Ty)>,
    ret: Ty,
}

impl Gen<'_, '_> {
    fn pick(&mut self, n: usize) -> usize {
        self.u.choose_index(n).unwrap_or(0)
    }

    fn ty(&mut self) -> Ty {
        [Ty::I, Ty::F, Ty::B][self.pick(3)]
    }

    fn fresh(&mut self, p: &str) -> String {
        self.next += 1;
        format!("{}{}", p, self.next)
    }

    fn vars(&self, ty: Ty, assignable: bool) -> Vec<String> {
        self.scopes.iter().flatten().filter(|v| v.ty == ty && (v.assignable || !assignable)).map(|v| v.name.clone()).collect()
    }

    fn leaf(&mut self, ty: Ty) -> String {
        let vars = self.vars(ty, false);
        if !vars.is_empty() && self.pick(3) > 0 {
            return vars[self.pick(vars.len())].clone();
        }
        match ty {
            Ty::I => INTS[self.pick(INTS.len())].into(),
            Ty::F => FLOATS[self.pick(FLOATS.len())].into(),
            Ty::B => ["true", "false"][self.pick(2)].into(),
        }
    }

    fn expr(&mut self, ty: Ty, depth: usize) -> String {
        if depth == 0 || self.pick(3) == 0 {
            return self.leaf(ty);
        }
        let d = depth - 1;
        // `g` only outside loops, so that a loop of calls of a function with loops stays small.
        let call = self.helper.clone().filter(|(_, r)| *r == ty && self.loops == 0);
        match ty {
            Ty::I => match self.pick(7) {
                0 => format!("({} + {})", self.expr(Ty::I, d), self.expr(Ty::I, d)),
                1 => format!("({} - {})", self.expr(Ty::I, d), self.expr(Ty::I, d)),
                2 => format!("({} * {})", self.expr(Ty::I, d), self.expr(Ty::I, d)),
                3 => format!("({} % {})", self.expr(Ty::I, d), self.expr(Ty::I, d)),
                4 => format!("(-{})", self.expr(Ty::I, d)),
                5 if call.is_some() => self.call(call.unwrap().0, d),
                _ => self.leaf(ty),
            },
            Ty::F => match self.pick(9) {
                0 => format!("({} + {})", self.expr(Ty::F, d), self.expr(Ty::F, d)),
                1 => format!("({} - {})", self.expr(Ty::F, d), self.expr(Ty::F, d)),
                2 => format!("({} * {})", self.expr(Ty::F, d), self.expr(Ty::F, d)),
                3 => format!("({} / {})", self.expr(Ty::F, d), self.expr(Ty::F, d)),
                4 => format!("(-{})", self.expr(Ty::F, d)),
                // An int and a float: a float.
                5 => format!("({} {} {})", self.expr(Ty::I, d), ["+", "-", "*", "/"][self.pick(4)], self.expr(Ty::F, d)),
                6 => format!("({} {} {})", self.expr(Ty::F, d), ["+", "-", "*", "/"][self.pick(4)], self.expr(Ty::I, d)),
                7 if call.is_some() => self.call(call.unwrap().0, d),
                _ => self.leaf(ty),
            },
            Ty::B => match self.pick(7) {
                0 | 1 => {
                    let t = if self.pick(2) == 0 { Ty::I } else { Ty::F };
                    let op = ["<", "<=", ">", ">=", "==", "!="][self.pick(6)];
                    format!("({} {} {})", self.expr(t, d), op, self.expr(t, d))
                }
                2 => format!("({} && {})", self.expr(Ty::B, d), self.expr(Ty::B, d)),
                3 => format!("({} || {})", self.expr(Ty::B, d), self.expr(Ty::B, d)),
                4 => format!("(!{})", self.expr(Ty::B, d)),
                5 => format!("({} == {})", self.expr(Ty::B, d), self.expr(Ty::B, d)),
                _ => self.leaf(ty),
            },
        }
    }

    fn call(&mut self, params: Vec<Ty>, d: usize) -> String {
        let args: Vec<String> = params.iter().map(|t| self.expr(*t, d)).collect();
        format!("g({})", args.join(", "))
    }

    fn block(&mut self, indent: usize, out: &mut String) {
        self.scopes.push(Vec::new());
        let n = 1 + self.pick(3);
        for _ in 0..n {
            self.stmt(indent, out);
        }
        self.scopes.pop();
    }

    fn line(&mut self, indent: usize, out: &mut String, text: &str) {
        out.push_str(&"    ".repeat(indent));
        out.push_str(text);
        out.push('\n');
    }

    fn stmt(&mut self, indent: usize, out: &mut String) {
        if self.budget == 0 {
            return;
        }
        self.budget -= 1;
        let nested = indent < 4;
        match self.pick(8) {
            2 if nested => {
                let c = self.expr(Ty::B, 3);
                self.line(indent, out, &format!("if {} {{", c));
                self.block(indent + 1, out);
                if self.pick(2) == 0 {
                    self.line(indent, out, "} else {");
                    self.block(indent + 1, out);
                }
                self.line(indent, out, "}");
            }
            3 if nested && self.loops < 3 => {
                let i = self.fresh("i");
                let k = self.pick(13);
                let range = if self.pick(2) == 0 { format!("0..{}", k) } else { format!("range({})", k) };
                self.line(indent, out, &format!("for {} in {} {{", i, range));
                self.loops += 1;
                self.scopes.push(vec![Var { name: i, ty: Ty::I, assignable: false }]);
                self.block(indent + 1, out);
                self.scopes.pop();
                self.loops -= 1;
                self.line(indent, out, "}");
            }
            4 if nested && self.loops < 3 => {
                // A guard that counts to at most 12, whatever else the condition says.
                let w = self.fresh("w");
                let k = self.pick(13);
                self.line(indent, out, &format!("let {} = 0", w));
                let c = self.expr(Ty::B, 2);
                self.line(indent, out, &format!("while {} < {} && {} {{", w, k, c));
                self.line(indent + 1, out, &format!("{} = {} + 1", w, w));
                self.loops += 1;
                self.block(indent + 1, out);
                self.loops -= 1;
                self.line(indent, out, "}");
                self.scopes.last_mut().unwrap().push(Var { name: w, ty: Ty::I, assignable: false });
            }
            5 if indent > 1 => {
                let e = self.expr(self.ret, 3);
                self.line(indent, out, &format!("return {}", e));
            }
            1 | 6 => {
                let ty = self.ty();
                let targets = self.vars(ty, true);
                if targets.is_empty() {
                    return self.let_(indent, out);
                }
                let v = targets[self.pick(targets.len())].clone();
                let e = self.expr(ty, 3);
                self.line(indent, out, &format!("{} = {}", v, e));
            }
            _ => self.let_(indent, out),
        }
    }

    fn let_(&mut self, indent: usize, out: &mut String) {
        let ty = self.ty();
        let v = self.fresh("v");
        let e = self.expr(ty, 3);
        self.line(indent, out, &format!("let {} = {}", v, e));
        self.scopes.last_mut().unwrap().push(Var { name: v, ty, assignable: true });
    }

    /// A function's parameters and body (each line ending with a newline), its last line a return.
    fn function(&mut self, prefix: &str, params: &[Ty]) -> (Vec<String>, String) {
        let names: Vec<String> = (0..params.len()).map(|k| format!("{}{}", prefix, k)).collect();
        self.scopes = vec![names.iter().zip(params).map(|(n, t)| Var { name: n.clone(), ty: *t, assignable: false }).collect()];
        let mut body = String::new();
        let n = 1 + self.pick(5);
        for _ in 0..n {
            self.stmt(1, &mut body);
        }
        let e = self.expr(self.ret, 4);
        self.line(1, &mut body, &format!("return {}", e));
        (names, body)
    }
}

#[derive(Arbitrary, Debug)]
enum Arg {
    Int(i64),
    Small(i8),
    Float(f64),
    Bool(bool),
}

struct Case {
    program: String,
    script: String,
    args: Vec<Value>,
}

fn case(u: &mut Unstructured) -> arbitrary::Result<Case> {
    let args: Vec<Arg> = (0..1 + u.choose_index(3)?).map(|_| Arg::arbitrary(u)).collect::<arbitrary::Result<_>>()?;
    let args: Vec<Value> = args
        .iter()
        .map(|a| match a {
            Arg::Int(n) => Value::Int(*n),
            Arg::Small(n) => Value::Int(*n as i64),
            Arg::Float(f) => Value::Float(*f),
            Arg::Bool(b) => Value::Bool(*b),
        })
        .collect();
    let types: Vec<Ty> = args.iter().map(|a| match a {
        Value::Int(_) => Ty::I,
        Value::Float(_) => Ty::F,
        _ => Ty::B,
    }).collect();
    let mut g = Gen { u, scopes: Vec::new(), next: 0, budget: 24, loops: 0, helper: None, ret: Ty::I };
    let (mut program, mut script) = (String::new(), String::new());
    if g.pick(2) == 0 {
        let params: Vec<Ty> = (0..1 + g.pick(2)).map(|_| if g.pick(2) == 0 { Ty::I } else { Ty::F }).collect();
        g.ret = if g.pick(2) == 0 { Ty::I } else { Ty::F };
        let (names, body) = g.function("a", &params);
        program.push_str(&format!("fn g({}) {{\n{}}}\n", names.join(", "), body));
        script.push_str(&format!("let g = ({}) => {{\n{}}}\n", names.join(", "), in_script(&body)));
        g.helper = Some((params, g.ret));
    }
    g.ret = g.ty();
    let (names, body) = g.function("p", &types);
    program.push_str(&format!("fn f({}) {{\n{}}}\n", names.join(", "), body));
    script.push_str(&format!("let f = ({}) => {{\n{}}}\n", names.join(", "), in_script(&body)));
    let given: Vec<String> = (0..args.len()).map(|k| format!("x{}", k)).collect();
    script.push_str(&format!("f({})\n", given.join(", ")));
    Ok(Case { program, script, args })
}

/// A body as a script writes it: `range` is not a script's, so `range(k)` is `0..k` there.
fn in_script(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(at) = rest.find("range(") {
        let k = &rest[at + 6..];
        let end = k.find(')').expect("range(k)");
        out.push_str(&rest[..at]);
        out.push_str("0..");
        out.push_str(&k[..end]);
        rest = &k[end + 1..];
    }
    out.push_str(rest);
    out
}

enum Got {
    Value(Value),
    Error(String),
}

impl std::fmt::Debug for Got {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Got::Value(v) => write!(f, "{}", v.repr()),
            Got::Error(m) => write!(f, "error {:?}", m),
        }
    }
}

fn shown(args: &[Value]) -> String {
    args.iter().map(|a| a.repr()).collect::<Vec<_>>().join(", ")
}

fn same(a: &Got, b: &Got) -> bool {
    match (a, b) {
        (Got::Value(Value::Float(x)), Got::Value(Value::Float(y))) => x.to_bits() == y.to_bits() || (x.is_nan() && y.is_nan()),
        (Got::Value(Value::Int(x)), Got::Value(Value::Int(y))) => x == y,
        (Got::Value(Value::Bool(x)), Got::Value(Value::Bool(y))) => x == y,
        (Got::Value(x), Got::Value(y)) => x.repr() == y.repr(),
        (Got::Error(x), Got::Error(y)) => x == y,
        _ => false,
    }
}

thread_local! {
    static IT: RefCell<Interp> = RefCell::new(Interp::new(std::env::temp_dir()));
    /// Programs run, compared, compared as errors, left uncompared, functions compiled.
    static STATS: Cell<[u64; 5]> = const { Cell::new([0; 5]) };
}

fn count(k: usize, n: u64) {
    STATS.with(|s| {
        let mut v = s.get();
        v[k] += n;
        s.set(v);
        if k == 0 && v[0] % 10_000 == 0 && std::env::var_os("QUILLO_FUZZ_STATS").is_some() {
            eprintln!("run: {} programs, {} compared ({} as errors), {} not (steps), {} functions compiled", v[0], v[1], v[2], v[3], v[4]);
        }
    });
}

fuzz_target!(|data: &[u8]| {
    let Ok(c) = case(&mut Unstructured::new(data)) else { return };
    IT.with(|cell| {
        let it = &mut *cell.borrow_mut();
        let before = quillo::jit::compiled();
        if let Err(e) = it.load_source(&c.program, "fuzz.qwe") {
            panic!("a generated program did not load: {}\n{}", e, c.program);
        }
        let f = quillo::value::lookup(&it.globals, "f").expect("f");
        // Interpreted first, or compiled at once when it has a loop; then compiled.
        let runs: Vec<Got> = (0..3)
            .map(|_| match it.call(&f, c.args.clone(), 0) {
                Ok(v) => Got::Value(v),
                Err(Ctrl::Error(e)) => Got::Error(format!("line {}: {}", e.line, e.msg)),
                Err(_) => Got::Error("a return or reply out of the function".into()),
            })
            .collect();
        for (k, r) in runs.iter().enumerate().skip(1) {
            assert!(same(r, &runs[0]), "call {} gave {:?}, the first {:?}\n{}args: {}", k + 1, r, runs[0], c.program, shown(&c.args));
        }
        count(4, (quillo::jit::compiled() - before) as u64);
        // The interpreter alone: a script never runs machine code.
        let mut vars = Map::new();
        for (k, a) in c.args.iter().enumerate() {
            vars.insert(format!("x{}", k).into(), a.clone());
        }
        let script = match quillo::script::run_script_for(it, &c.script, Some(&vars), 60_000) {
            Ok(Value::Err(e, ..)) => Got::Error(e.display()),
            Ok(v) => Got::Value(v),
            Err(_) => Got::Error("stopped".into()),
        };
        drop(f);
        count(0, 1);
        match (&script, &runs[0]) {
            (Got::Error(m), _) if m.contains("too many steps") || m.contains("took longer") => count(3, 1),
            (Got::Error(m), _) if m.contains("script") => panic!("the script was refused: {}\n{}", m, c.script),
            // The script says the line in its own text, where `g` is a `let`: the message without it.
            (Got::Error(s), Got::Error(p)) => {
                let tail = |m: &str| m.splitn(2, ": ").nth(1).unwrap_or(m).to_string();
                assert_eq!(tail(s), tail(p), "the interpreter and the program fail differently\n{}args: {}", c.program, shown(&c.args));
                count(1, 1);
                count(2, 1);
            }
            _ => {
                assert!(same(&script, &runs[0]), "the interpreter gave {:?}, the program {:?}\n{}args: {}", script, runs[0], c.program, shown(&c.args));
                count(1, 1);
            }
        }
    });
    // Its machine code given back now and then: a run of millions of programs kept each one's.
    if STATS.with(|s| s.get()[0]) % 512 == 0 {
        quillo::jit::fuzzing::forget();
    }
});
