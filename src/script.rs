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
    "str", "int", "float", "len", "type_of", "upper", "lower", "trim", "split", "join", "contains", "starts_with", "ends_with",
    "replace", "slice", "sum", "min", "max", "sort", "reverse", "first", "last", "push", "enumerate", "keys", "values", "get", "has",
    "set", "remove", "merge", "err", "is_err", "unwrap", "escape", "abs", "round", "floor", "ceil", "sqrt", "pow", "unique", "flatten",
    "zip", "take", "drop", "index_of", "map", "filter", "find", "any", "all", "reduce", "sort_by", "count", "group_by",
];

/// Steps (calls and loop rounds) a script may take.
pub const SCRIPT_STEPS: u64 = 20_000;

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
    if let Some(why) = stmts.iter().find_map(forbidden_stmt) {
        return bad(why);
    }
    // Its world: the safe builtins, then its variables.
    let root = Scope::root();
    for name in SAFE {
        if let Some(v) = lookup(&it.globals, name) {
            define(&root, name, v);
        }
    }
    let env = Scope::child(&root);
    if let Some(m) = vars {
        for (k, v) in m.iter() {
            define(&env, k, v.clone());
        }
    }
    let saved = std::mem::replace(&mut it.fuel, SCRIPT_STEPS);
    let mut last = Value::Nil;
    let mut out = None;
    for s in stmts.iter() {
        match it.exec_stmt(s, &env) {
            Ok(v) => last = v,
            Err(Ctrl::Return(v)) => {
                out = Some(v);
                break;
            }
            Err(Ctrl::Error(e)) => {
                out = Some(bad(&if e.line > 0 { format!("line {}: {}", e.line, e.msg) } else { e.msg.clone() }));
                break;
            }
            Err(Ctrl::Reply(_)) => {
                out = Some(bad("a script cannot reply"));
                break;
            }
        }
    }
    it.fuel = saved;
    out.unwrap_or(last)
}
