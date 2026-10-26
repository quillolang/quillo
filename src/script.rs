//! `run_script(source, vars)`: code a program's users write (a rule on a
//! form's field, who may see a record) run on the server, safely. The
//! script sees only the variables it is given and the builtins that compute
//! (`date` among them, without `date.today` and `date.now`: no database,
//! files, network, clock, sessions or replies); it cannot
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
    "str", "int", "float", "len", "type_of", "upper", "lower", "trim", "split", "join", "contains", "starts_with", "ends_with",
    "replace", "slice", "sum", "min", "max", "sort", "reverse", "first", "last", "push", "enumerate", "keys", "values", "get", "has",
    "set", "remove", "merge", "err", "is_err", "unwrap", "escape", "abs", "round", "floor", "ceil", "sqrt", "pow", "unique", "flatten",
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
            Value::Err(x) => todo.push((x, depth + 1)),
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
        Stmt::Let(_, e, _) | Stmt::Assign(_, _, e, _) | Stmt::Expr(e) | Stmt::Return(Some(e)) => forbidden(e),
        Stmt::Return(None) => None,
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
        Expr::For(_, it, body, _) => forbidden(it).or_else(|| forbidden(body)),
        Expr::Block(stmts) => stmts.iter().find_map(forbidden_stmt),
        Expr::Lambda(def) => def.params.iter().find_map(|p| p.default.as_ref().and_then(forbidden)).or_else(|| forbidden(&def.body)),
        Expr::Reply(..) => Some("a script cannot reply"),
        Expr::Html(..) => Some("a script gives values, not markup"),
    }
}

pub fn run_script(it: &mut Interp, src: &str, vars: Option<&Map>) -> Value {
    let stmts = match crate::parser::Parser::new(src).parse_program() {
        Ok(s) => s,
        Err(e) => return bad(&format!("line {}: {}", e.line, e.msg)),
    };
    // Its world: the safe builtins, then its variables.
    let root = Scope::root();
    for name in SAFE {
        if let Some(v) = lookup(&it.globals, name) {
            define(&root, name, v);
        }
    }
    // Dates, without the clock: a script gives the same answer each time.
    let mut date = Map::new();
    crate::dates::install_pure(&mut date);
    define(&root, "date", Value::map(date));
    let env = Scope::child(&root);
    if let Some(m) = vars {
        for (k, v) in m.iter() {
            define(&env, k, v.clone());
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
    it.fuel = saved;
    it.allowed = saved_allowed;
    it.restrictor = saved_restrictor;
    it.script = saved_script;
    out
}
