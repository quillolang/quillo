//! Islands load their code when it is needed. Most components only react
//! to someone — a click, typing — so the browser downloads the page's
//! bundle when someone reaches for one (hovers, touches, focuses it). A
//! component that runs by itself must start with the page: this says which.

use crate::ast::*;

/// Builtins that, called while a component renders, make it run without
/// anyone touching it: timers, live data, the page's own JavaScript.
const BY_ITSELF: &[&str] = &["every", "live", "js", "js_new"];

/// Whether a component's body (not its event handlers, which only run when
/// someone acts) starts timers or live data, runs JavaScript, or has an
/// element with `@mount`.
pub fn runs_by_itself(e: &Expr) -> bool {
    match e {
        Expr::Lit(_) | Expr::Ident(..) => false,
        // Handlers and callbacks run when called, from something that woke
        // the component up already.
        Expr::Lambda(_) => false,
        Expr::Call(f, args, _) => {
            matches!(&**f, Expr::Ident(n, _) if BY_ITSELF.contains(&n.as_str())) || runs_by_itself(f) || args.iter().any(runs_by_itself)
        }
        Expr::Template(parts) => parts.iter().any(|p| matches!(p, TPart::Expr(e) if runs_by_itself(e))),
        Expr::List(es) => es.iter().any(runs_by_itself),
        Expr::Map(kvs) => kvs.iter().any(|(_, e)| runs_by_itself(e)),
        Expr::Unary(_, a, _) | Expr::Try(a) | Expr::Field(a, _, _) => runs_by_itself(a),
        Expr::Binary(_, a, b, _) | Expr::Coalesce(a, b) | Expr::Pipe(a, b, _) | Expr::Index(a, b, _) => runs_by_itself(a) || runs_by_itself(b),
        Expr::If(c, a, b) => runs_by_itself(c) || runs_by_itself(a) || b.as_deref().is_some_and(runs_by_itself),
        Expr::Match(x, arms, _) => runs_by_itself(x) || arms.iter().any(|a| a.guard.as_ref().is_some_and(runs_by_itself) || runs_by_itself(&a.body)),
        Expr::For(_, it, body, _) => runs_by_itself(it) || runs_by_itself(body),
        Expr::Block(stmts) => stmts.iter().any(stmt_runs_by_itself),
        Expr::Reply(..) => false,
        Expr::Html(el, _) => el_runs_by_itself(el),
    }
}

fn stmt_runs_by_itself(s: &Stmt) -> bool {
    match s {
        Stmt::Let(_, e, _) | Stmt::Assign(_, _, e, _) | Stmt::Expr(e) | Stmt::State(_, e, _) | Stmt::Return(Some(e)) => runs_by_itself(e),
        _ => false,
    }
}

fn el_runs_by_itself(el: &HtmlEl) -> bool {
    el.attrs.iter().any(|(name, v)| {
        name == "@mount" || (!name.starts_with('@') && matches!(v, AttrVal::Expr(e) | AttrVal::Spread(e) if runs_by_itself(e)))
    }) || el.id.as_ref().is_some_and(runs_by_itself)
        || el.classes.iter().any(runs_by_itself)
        || el.children.iter().any(|c| match c {
            HtmlChild::El(e) => el_runs_by_itself(e),
            HtmlChild::Expr(e) => runs_by_itself(e),
        })
}
