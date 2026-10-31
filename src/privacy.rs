//! `private` and `public` at the top level of a file (branch public-private).
//!
//! Every file a program `use`s puts its names in the program's one namespace, and a
//! name is defined once. `private fn helper()` (or `view`, `component`, `let`) keeps a
//! name to its file: two files may each have a private `helper`, and other files do
//! not see it. `public` says what is the default.
//!
//! How: once a file is parsed, each of its private names is renamed, everywhere in
//! that file, to `name__pv` and eight hex digits of a hash of the file's path (a name
//! no one writes). Definitions and uses alike, locals of the same name too, so what
//! each name means in the file does not change; map keys, fields, attribute names and
//! types are left as they are. Everything after the parser (the checker, the
//! interpreter, the component compiler, jobs) then sees names that cannot collide.
//! `demangle` turns such a name back into what was written, for every message.

use crate::ast::*;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// What ends every private name: `helper` is `helper__pv1a2b3c4d`.
pub const MARK: &str = "__pv";

/// The eight hex digits of a file: from its path as seen from the folder the program
/// runs in, so they are the same on every server and after every deploy.
pub fn tag(file: Option<&str>, src: &str) -> String {
    let key = match file {
        Some(f) => {
            let p = std::path::Path::new(f);
            let rel = std::env::current_dir().ok().and_then(|d| p.strip_prefix(&d).ok().map(|r| r.to_path_buf()));
            rel.map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_else(|| f.to_string())
        }
        None => src.to_string(),
    };
    // FNV-1a: the same on every machine and Rust version.
    let mut h: u32 = 0x811c9dc5;
    for b in key.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    format!("{:08x}", h)
}

pub fn mangle(name: &str, tag: &str) -> String {
    format!("{}{}{}", name, MARK, tag)
}

/// Whether a name is a private one, and what was written.
pub fn private_name(name: &str) -> Option<&str> {
    let at = name.rfind(MARK)?;
    let hex = &name[at + MARK.len()..];
    (hex.len() == 8 && hex.bytes().all(|b| b.is_ascii_hexdigit())).then(|| &name[..at])
}

/// `text` with every private name as it was written.
pub fn demangle(text: &str) -> String {
    if !text.contains(MARK) {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(MARK) {
        let hex = &rest[at + MARK.len()..];
        let ok = hex.len() >= 8 && hex.as_bytes()[..8].iter().all(|b| b.is_ascii_hexdigit())
            && !hex.as_bytes().get(8).map_or(false, |b| b.is_ascii_alphanumeric() || *b == b'_');
        if ok {
            out.push_str(&rest[..at]);
            rest = &hex[8..];
        } else {
            out.push_str(&rest[..at + MARK.len()]);
            rest = hex;
        }
    }
    out.push_str(rest);
    out
}

/// Renames `names` in a parsed file, as the module's comment says. Errs when a view's
/// or component's parameter has a private name: props are matched by name, and the
/// renamed parameter would no longer be found.
pub fn rename(stmts: &mut [Stmt], names: &HashSet<String>, tag: &str) -> Result<(), crate::error::QuilloError> {
    let map = names.iter().map(|n| (n.clone(), mangle(n, tag))).collect();
    rename_as(stmts, &map, "a private name in this file")
}

/// Renames names in a parsed file as `map` says, which is how a file's names get their
/// names in the program (src/modules.rs). `what` is how an error calls such a name.
pub fn rename_as(stmts: &mut [Stmt], map: &HashMap<String, String>, what: &str) -> Result<(), crate::error::QuilloError> {
    let r = Renamer { map, what };
    for s in stmts.iter_mut() {
        r.stmt(s)?;
    }
    Ok(())
}

struct Renamer<'a> {
    map: &'a HashMap<String, String>,
    what: &'a str,
}

type R = Result<(), crate::error::QuilloError>;

impl Renamer<'_> {
    fn name(&self, n: &mut String) {
        if let Some(to) = self.map.get(n.as_str()) {
            *n = to.clone();
        }
    }

    fn stmt(&self, s: &mut Stmt) -> R {
        match s {
            Stmt::Let(p, e, _) => {
                self.pat(p)?;
                self.expr(e)
            }
            Stmt::Assign(n, _, e, _) => {
                self.name(n);
                self.expr(e)
            }
            Stmt::SetField(o, _, _, e, _) => {
                self.expr(o)?;
                self.expr(e)
            }
            Stmt::Expr(e) | Stmt::Return(Some(e), _) => self.expr(e),
            Stmt::Fn(def) => self.func(def),
            Stmt::Route(r) => {
                let r = Arc::get_mut(r).expect("a route just parsed has one owner");
                for seg in r.path.iter_mut() {
                    match seg {
                        Seg::Param(n, _) | Seg::Rest(n) => self.name(n),
                        Seg::Lit(_) => {}
                    }
                }
                for p in r.query.iter_mut().chain(r.body_pat.iter_mut()).chain(r.guards.iter_mut()) {
                    self.pat(p)?;
                }
                self.expr(&mut r.body)
            }
            Stmt::State(n, e, _) => {
                self.name(n);
                self.expr(e)
            }
            Stmt::Service(_, _, _, Some(e), _) => self.expr(e),
            Stmt::Type(_) | Stmt::Use(..) | Stmt::Mount(..) | Stmt::Service(..) | Stmt::Return(None, _) => Ok(()),
        }
    }

    fn func(&self, def: &mut Arc<FnDef>) -> R {
        let def = Arc::get_mut(def).expect("a function just parsed has one owner");
        let props = def.component || def.name.starts_with(|c: char| c.is_uppercase());
        self.name(&mut def.name);
        for p in def.params.iter_mut() {
            if props && self.map.contains_key(&p.name) {
                return Err(crate::error::QuilloError::new(
                    format!("`{}` is {}, and a prop of `{}` too: props are passed by name, so rename one", p.name, self.what, demangle(&def.name)),
                    def.line,
                ));
            }
            self.name(&mut p.name);
            if let Some(d) = p.default.as_mut() {
                self.expr(d)?;
            }
        }
        self.expr(&mut def.body)
    }

    fn pat(&self, p: &mut Pattern) -> R {
        match p {
            Pattern::Bind(n) => self.name(n),
            Pattern::As(inner, n) => {
                self.pat(inner)?;
                self.name(n);
            }
            Pattern::Alt(ps) => {
                for p in ps {
                    self.pat(p)?;
                }
            }
            Pattern::List(ps, rest) => {
                for p in ps {
                    self.pat(p)?;
                }
                if let Some(Some(n)) = rest {
                    self.name(n);
                }
            }
            Pattern::Record(fields) => {
                for f in fields {
                    // `{ q: str }` binds its key: under a new name, what it binds
                    // is told apart from the key by `as`, or the body's renamed `q`
                    // reads the variable (FEEDBACK16.md, Q-112; branch feedback20-fixes).
                    if let (true, Some(to)) = (f.pat.is_type_test(), f.path.last().and_then(|k| self.map.get(k))) {
                        f.pat = Pattern::As(Box::new(std::mem::replace(&mut f.pat, Pattern::Wild)), to.clone());
                    }
                    self.pat(&mut f.pat)?;
                    if let Some(d) = f.default.as_mut() {
                        self.expr(d)?;
                    }
                }
            }
            Pattern::Wild | Pattern::Lit(_) | Pattern::Type(_) => {}
        }
        Ok(())
    }

    fn expr(&self, e: &mut Expr) -> R {
        match e {
            Expr::Ident(n, _) => self.name(n),
            Expr::Lit(_) => {}
            Expr::Template(parts) => {
                for p in parts {
                    if let TPart::Expr(e) = p {
                        self.expr(e)?;
                    }
                }
            }
            Expr::List(xs) => {
                for x in xs {
                    self.expr(x)?;
                }
            }
            Expr::Map(kvs) => {
                for (_, v) in kvs {
                    self.expr(v)?;
                }
            }
            Expr::Unary(_, a, _) | Expr::Try(a) | Expr::Field(a, _, _) => self.expr(a)?,
            Expr::Binary(_, a, b, _) | Expr::Coalesce(a, b) | Expr::Pipe(a, b, _) | Expr::Index(a, b, _) | Expr::While(a, b, _) => {
                self.expr(a)?;
                self.expr(b)?;
            }
            Expr::Call(f, args, _) => {
                self.expr(f)?;
                for a in args {
                    self.expr(a)?;
                }
            }
            Expr::If(c, t, f) => {
                self.expr(c)?;
                self.expr(t)?;
                if let Some(f) = f {
                    self.expr(f)?;
                }
            }
            Expr::Match(x, arms, _) => {
                self.expr(x)?;
                for a in arms {
                    self.pat(&mut a.pat)?;
                    if let Some(g) = a.guard.as_mut() {
                        self.expr(g)?;
                    }
                    self.expr(&mut a.body)?;
                }
            }
            Expr::For(p, xs, body, _) => {
                self.pat(p)?;
                self.expr(xs)?;
                self.expr(body)?;
            }
            Expr::Block(stmts) => {
                for s in stmts {
                    self.stmt(s)?;
                }
            }
            Expr::Lambda(def) => self.func(def)?,
            Expr::Reply(a, b, _) => {
                self.expr(a)?;
                if let Some(b) = b {
                    self.expr(b)?;
                }
            }
            Expr::Html(el, _) => self.el(el)?,
        }
        Ok(())
    }

    fn el(&self, el: &mut HtmlEl) -> R {
        // A view or component of this file's own, private: its tag is its name.
        if el.facts.component {
            self.name(&mut el.tag);
        }
        if let Some(id) = el.id.as_mut() {
            self.expr(id)?;
        }
        for c in el.classes.iter_mut() {
            self.expr(c)?;
        }
        for (_, v) in el.attrs.iter_mut() {
            match v {
                AttrVal::Expr(e) | AttrVal::Spread(e) => self.expr(e)?,
                AttrVal::Bool => {}
            }
        }
        for c in el.children.iter_mut() {
            match c {
                HtmlChild::El(e) => self.el(e)?,
                HtmlChild::Expr(e) => self.expr(e)?,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demangles() {
        let t = tag(Some("/x/lib/a.qwe"), "");
        let m = mangle("helper", &t);
        assert_eq!(private_name(&m), Some("helper"));
        assert_eq!(demangle(&format!("unknown name `{}` in {}()", m, m)), "unknown name `helper` in helper()");
        assert_eq!(demangle("a__pvnothex"), "a__pvnothex");
        assert_eq!(private_name("plain"), None);
    }
}
