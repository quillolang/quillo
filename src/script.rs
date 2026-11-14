//! `run_script(source, vars)`: code a program's users write (a rule on a
//! form's field, who may see a record) run on the server, safely. The
//! script sees only the variables it is given and the builtins that compute
//! (no database, files, network, clock, sessions or replies); it cannot
//! define functions, routes or state; and it stops after a budget of steps.
//! What it gives back is its last expression's value, or an error value
//! saying what went wrong (a parse error, a name it may not use, too many
//! steps), so the program can show it as a message.

use crate::ast::*;
use crate::interp::{Ctrl, Interp};
use crate::value::*;

/// Builtins a script may call: they compute from their arguments and
/// nothing else (and cannot make huge values from small ones).
const SAFE: &[&str] = &[
    "str", "int", "float", "len", "type_of", "upper", "lower", "normalize", "local_path", "trim", "filled", "split", "join", "contains", "starts_with", "ends_with",
    "replace", "slice", "sum", "min", "max", "sort", "reverse", "first", "last", "push", "enumerate", "keys", "values", "get", "has",
    "set", "remove", "merge", "err", "is_err", "unwrap", "escape", "url_encode", "abs", "round", "floor", "ceil", "sqrt", "pow", "unique", "flatten",
    "zip", "take", "drop", "index_of", "map", "filter", "find", "any", "all", "reduce", "sort_by", "count", "group_by",
];

/// Steps (calls and loop rounds) a script may take.
pub const SCRIPT_STEPS: u64 = 20_000;

/// The most a value a script makes may hold, in bytes, roughly: a string's
/// bytes, and eight for each number, item or entry, through nested lists
/// and maps. Each 1 KB made also costs a step.
pub const SCRIPT_BYTES: usize = 1 << 20;

/// How deep a script's lists and maps may nest.
const SCRIPT_NESTING: usize = 256;

/// The size of `v`, made by a script, or why it is too big. It counts no
/// further than the limit, so a check costs at most that.
pub fn too_big(v: &Value) -> Result<usize, &'static str> {
    let mut n = 0usize;
    let mut todo = vec![(v, 0usize)];
    while let Some((v, depth)) = todo.pop() {
        if depth > SCRIPT_NESTING {
            return Err("the script nested lists or maps too deeply");
        }
        match v {
            Value::Str(s) | Value::Html(s) => n += s.len(),
            Value::List(l) => {
                n = n.saturating_add(8 * l.len());
                if n <= SCRIPT_BYTES {
                    todo.extend(l.iter().map(|x| (x, depth + 1)));
                }
            }
            Value::Map(m) => {
                for (k, x) in m.iter() {
                    n += 8 + k.len();
                    if n > SCRIPT_BYTES {
                        break;
                    }
                    todo.push((x, depth + 1));
                }
            }
            Value::Err(x, _, _) => todo.push((x, depth + 1)),
            _ => n += 8,
        }
        if n > SCRIPT_BYTES {
            return Err("the script made a value larger than 1 MB");
        }
    }
    Ok(n)
}

/// Whether builtin `name` would make a value over the limit from `args`,
/// checked before it runs: `join`, `replace` and `split` can make far more
/// than they are given.
pub fn would_be_too_big(name: &str, args: &[Value]) -> bool {
    let text = |i: usize| match args.get(i) {
        Some(Value::Str(s)) => Some(&**s),
        _ => None,
    };
    let times = |s: &str, part: &str| if part.is_empty() { s.chars().count() + 1 } else { s.matches(part).count() + 1 };
    let made = match name {
        "join" => match args.first() {
            Some(Value::List(l)) => {
                let sep = text(1).map_or(0, str::len);
                too_big(&args[0]).unwrap_or(usize::MAX).saturating_add(sep.saturating_mul(l.len()))
            }
            _ => 0,
        },
        "replace" => match (text(0), text(1), text(2)) {
            (Some(s), Some(from), Some(to)) => s.len().saturating_add(times(s, from).saturating_mul(to.len())),
            _ => 0,
        },
        "split" => match text(0) {
            Some(s) => s.len().saturating_add(8 * times(s, text(1).unwrap_or(" "))),
            _ => 0,
        },
        _ => 0,
    };
    made > SCRIPT_BYTES
}

fn bad(what: &str) -> Value {
    Value::err(Value::str(what))
}

/// What a script may not contain, if anything.
fn forbidden_stmt(s: &Stmt) -> Option<&'static str> {
    match s {
        Stmt::Let(_, e, _) | Stmt::Assign(_, _, e, _) | Stmt::Expr(e) | Stmt::Return(Some(e), _) => forbidden(e),
        Stmt::SetField(o, _, _, e, _) => forbidden(o).or_else(|| forbidden(e)),
        Stmt::Return(None, _) => None,
        Stmt::Fn(_) => Some("a script cannot define functions (use `(x) => ...`)"),
        Stmt::Route(_) | Stmt::Mount(..) => Some("a script cannot define routes"),
        Stmt::Service(..) => Some("a script cannot call services"),
        Stmt::Type(td) => td.defaults.iter().find_map(|(_, e)| forbidden(e)),
        Stmt::Use(..) => Some("a script cannot `use` files"),
        Stmt::State(..) => Some("a script cannot keep state"),
    }
}

fn forbidden(e: &Expr) -> Option<&'static str> {
    let all = |es: &[Expr]| es.iter().find_map(forbidden);
    match e {
        Expr::Lit(_) | Expr::Ident(..) => None,
        Expr::Template(parts) => parts.iter().find_map(|p| if let TPart::Expr(e) = p { forbidden(e) } else { None }),
        Expr::List(es) => all(es),
        Expr::Map(kvs) => kvs.iter().find_map(|(_, e)| forbidden(e)),
        Expr::Unary(_, a, _) | Expr::Try(a) | Expr::Field(a, _, _) => forbidden(a),
        Expr::Binary(_, a, b, _) | Expr::Coalesce(a, b) | Expr::Pipe(a, b, _) | Expr::Index(a, b, _) => forbidden(a).or_else(|| forbidden(b)),
        Expr::Call(f, args, _) => forbidden(f).or_else(|| all(args)),
        Expr::If(c, a, b) => forbidden(c).or_else(|| forbidden(a)).or_else(|| b.as_deref().and_then(forbidden)),
        Expr::Match(x, arms, _) => forbidden(x).or_else(|| arms.iter().find_map(|a| a.guard.as_ref().and_then(forbidden).or_else(|| forbidden(&a.body)))),
        Expr::For(_, it, body, _) | Expr::While(it, body, _) => forbidden(it).or_else(|| forbidden(body)),
        Expr::Block(stmts) => stmts.iter().find_map(forbidden_stmt),
        Expr::Lambda(def) => def.params.iter().find_map(|p| p.default.as_ref().and_then(forbidden)).or_else(|| forbidden(&def.body)),
        Expr::Reply(..) => Some("a script cannot reply"),
        Expr::Html(..) => Some("a script gives values, not markup"),
    }
}

pub fn run_script(it: &mut Interp, src: &str, vars: Option<&Map>) -> Value {
    // Names are kept for good, and this source may be a visitor's: its own are its own
    // (src/sym.rs), and messages say them as it wrote them.
    let (parsed, names) = crate::sym::Sym::in_script(|| crate::parser::Parser::new(src).parse_program());
    let out = match parsed {
        Ok(stmts) => run_parsed(it, &stmts, &names, vars),
        Err(e) => bad(&format!("line {}: {}", e.line, e.msg)),
    };
    if names.is_empty() {
        return out;
    }
    match &out {
        Value::Err(e, _, _) if matches!(&**e, Value::Str(_)) => bad(&said_as_written(&e.display(), &names)),
        _ => out,
    }
}

/// `msg` with a script's own names as it wrote them: a message quotes a name in backticks,
/// `` `$2` ``, and that is put back as `` `total` ``. Other text is the script's own.
fn said_as_written(msg: &str, names: &std::collections::HashMap<String, Sym>) -> String {
    let back: std::collections::HashMap<&str, &str> = names.iter().map(|(k, v)| (v.as_str(), k.as_str())).collect();
    let mut out = String::with_capacity(msg.len());
    let mut rest = msg;
    while let Some(at) = rest.find("`$") {
        out.push_str(&rest[..at]);
        let digits = rest[at + 2..].bytes().take_while(u8::is_ascii_digit).count();
        let token = &rest[at + 1..at + 2 + digits];
        match back.get(token) {
            Some(name) if rest[at + 2 + digits..].starts_with('`') => {
                out.push('`');
                out.push_str(name);
            }
            _ => out.push_str(&rest[at..at + 2 + digits]),
        }
        rest = &rest[at + 2 + digits..];
    }
    out.push_str(rest);
    out
}

fn run_parsed(it: &mut Interp, stmts: &[Stmt], names: &std::collections::HashMap<String, Sym>, vars: Option<&Map>) -> Value {
    // Its world: the safe builtins, then its variables. Quillo's own, not what the program's
    // globals have by those names: a program's `fn upper` was the script's `upper`, run with the
    // program's effects (FEEDBACK51.md, Q-648; branch feedback51-fixes).
    let root = Scope::root();
    BUILTINS.with(|builtins| {
        for name in SAFE {
            if let Some(v) = lookup(builtins, name) {
                define(&root, name, v);
            }
        }
    });
    // Dates, without the clock: a script gives the same answer each time.
    let mut date = Map::new();
    crate::dates::install_pure(&mut date);
    define(&root, "date", Value::map(date));
    let env = Scope::child(&root);
    // A variable the script never names is none of its business, and makes no name.
    if let Some(m) = vars {
        for (k, v) in m.iter() {
            if let Some(n) = names.get(k.as_str()).copied().or_else(|| Sym::known(k.as_str())) {
                define(&env, n, v.clone());
            }
        }
    }
    // A budget of steps, and no effects for what the script reaches by
    // itself; helpers the program passes in keep what the program may do
    // (`Interp::script_mask`).
    let saved = std::mem::replace(&mut it.fuel, SCRIPT_STEPS);
    let saved_allowed = std::mem::replace(&mut it.allowed, 0);
    let saved_restrictor = it.restrictor.take();
    let saved_script = it.script.replace((root.clone(), saved_allowed));
    // Room on the stack for code nested as deeply as the parser allows.
    let out = crate::stack::for_nesting(crate::parser::MAX_NESTING, || {
        if let Some(why) = stmts.iter().find_map(forbidden_stmt) {
            return bad(why);
        }
        let mut last = Value::Nil;
        for s in stmts.iter() {
            match it.exec_stmt(s, &env) {
                Ok(v) => last = v,
                Err(Ctrl::Return(v)) => return v,
                Err(Ctrl::Error(e)) => return bad(&if e.line > 0 { format!("line {}: {}", e.line, e.msg) } else { e.msg.clone() }),
                Err(Ctrl::Reply(_)) => return bad("a script cannot reply"),
            }
        }
        last
    });
    // Its own variables let go of, a lambda it kept in one too, which held them for ever
    // (FEEDBACK65.md, Q-941; branch feedback65).
    Scope::done(env);
    it.fuel = saved;
    it.allowed = saved_allowed;
    it.restrictor = saved_restrictor;
    it.script = saved_script;
    // A function it made would run after it, with no budget of steps: `(n) => ...` doubling a
    // text made 16 MB, and a `while true` ran for good (FEEDBACK51.md, Q-670; branch
    // feedback51-fixes).
    if holds_function(&out) {
        return bad("a script gives back no function (it would run without the script's limits): give back what it computes");
    }
    out
}

thread_local! {
    /// The builtins as Quillo installs them, for scripts' safe names.
    static BUILTINS: Env = {
        let g = Scope::root();
        crate::builtins::install(&g);
        g
    };
}

/// Whether a function is in `v`, however deep.
fn holds_function(v: &Value) -> bool {
    let mut todo = vec![v];
    while let Some(v) = todo.pop() {
        match v {
            Value::Func(_) => return true,
            Value::List(l) => todo.extend(l.iter()),
            Value::Map(m) => todo.extend(m.values()),
            Value::Err(e, ..) => todo.push(e),
            _ => {}
        }
    }
    false
}
