//! Records: types with a name and fields.
//!
//! ```text
//! type Address = { city: str, zip: str? }
//! type User = {
//!     name: str,
//!     email: str,
//!     role: "admin" | "member" = "member",   // one of these; a default
//!     address: Address?,
//!     tags: list<str> = [],
//! }
//! type Id = int | str                        // another name for a type
//!
//! User({ name: "Ada", email: "ada@example.com" })   // a User, or an error value
//! fn greet(u: User) = `Hi ${u.name}`                // checked when called
//! route POST /users { json: User } = save(json)     // the body, as a User
//! match v { User as u => ..., _ => ... }
//! ```
//!
//! A record is a map: JSON, the browser and every map function see it as
//! one. What its name adds is checking. `User(v)` makes one from a map: each
//! field is checked (records in it are made the same way), missing ones get
//! their defaults, and fields the record does not have are left out, so
//! what a client sends cannot add any. It gives an error value saying what
//! is wrong, as `int("x")` does. A record in an annotation or a pattern is
//! checked field by field (in a pattern, made as `User(v)` makes it), and
//! `quillo check` holds code to the fields before it runs.

use crate::ast::{is_type_name, Ty, TypeDef};
use crate::interp::{type_name_matches, Ctrl, Interp};
use crate::value::*;
use std::collections::HashMap;
use std::sync::Arc;

pub type Types = HashMap<String, Arc<TypeDef>>;

/// Where in a value it goes wrong, and how; `what` empty when it is just
/// not the type (said where that is known).
#[derive(Debug, Default)]
pub struct Why {
    path: String,
    what: String,
}

impl Why {
    fn inside(mut self, seg: &str) -> Why {
        self.path = if self.path.is_empty() {
            seg.to_string()
        } else if self.path.starts_with('[') {
            format!("{}{}", seg, self.path)
        } else {
            format!("{}.{}", seg, self.path)
        };
        self
    }

    /// Says what was wanted here, if nothing deeper did.
    fn here(mut self, ty: &Ty, v: &Value) -> Why {
        if self.what.is_empty() {
            self.what = format!("must be {}, got {} {}", ty.text(), v.type_name(), short(v));
        }
        self
    }

    fn missing(field: &str) -> Why {
        Why { path: field.to_string(), what: "is missing".into() }
    }

    /// "`address.city` must be str, got int 5"; None for a plain mismatch.
    pub fn text(&self) -> Option<String> {
        match (self.path.is_empty(), self.what.is_empty()) {
            (_, true) => None,
            (true, false) => Some(self.what.clone()),
            (false, false) => Some(format!("`{}` {}", self.path, self.what)),
        }
    }
}

fn short(v: &Value) -> String {
    let r = v.repr();
    if r.chars().count() > 40 {
        format!("{}…", r.chars().take(40).collect::<String>())
    } else {
        r
    }
}

/// Deeper than this, a type that names itself is taken as it is.
const DEPTH: usize = 64;

/// Whether `v` is of type `ty`. At the top of an annotation (`deep` false),
/// lists and map shapes are checked as before (`list<int>` is any list);
/// records are checked field by field, and everything inside them too.
pub fn fits(types: &Types, ty: &Ty, v: &Value, deep: bool) -> Result<(), Why> {
    fits_at(types, ty, v, deep, 0)
}

fn fits_at(types: &Types, ty: &Ty, v: &Value, deep: bool, depth: usize) -> Result<(), Why> {
    if (ty.optional && matches!(v, Value::Nil)) || depth > DEPTH {
        return Ok(());
    }
    let mut best = Why::default();
    for n in &ty.names {
        match fits_name(types, n, ty, v, deep, depth) {
            Ok(()) => return Ok(()),
            Err(w) if best.what.is_empty() && !w.what.is_empty() => best = w,
            Err(_) => {}
        }
    }
    Err(best)
}

fn fits_name(types: &Types, n: &str, ty: &Ty, v: &Value, deep: bool, depth: usize) -> Result<(), Why> {
    if n.starts_with('"') {
        let want: String = serde_json::from_str(n).unwrap_or_default();
        return match v {
            Value::Str(s) if **s == *want => Ok(()),
            _ => Err(Why::default()),
        };
    }
    if let Some(td) = types.get(n) {
        if !td.is_record() {
            return fits_at(types, &td.ty, v, deep, depth + 1);
        }
        let Value::Map(m) = v else { return Err(Why::default()) };
        for (k, fty) in td.ty.fields.as_deref().unwrap_or_default() {
            match m.get(k.as_str()).filter(|x| !matches!(x, Value::Nil)) {
                None if fty.optional => {}
                None => return Err(Why::missing(k)),
                Some(x) => fits_at(types, fty, x, true, depth + 1).map_err(|w| w.here(fty, x).inside(k))?,
            }
        }
        return Ok(());
    }
    if is_type_name(n) || !type_name_matches(n, v) {
        return Err(Why::default());
    }
    if deep {
        match (n, v) {
            ("list", Value::List(items)) => {
                if let Some(e) = &ty.elem {
                    for (i, x) in items.iter().enumerate() {
                        fits_at(types, e, x, true, depth + 1).map_err(|w| w.here(e, x).inside(&format!("[{}]", i)))?;
                    }
                }
            }
            ("map", Value::Map(m)) => {
                for (k, fty) in ty.fields.as_deref().unwrap_or_default() {
                    match m.get(k.as_str()).filter(|x| !matches!(x, Value::Nil)) {
                        None if fty.optional => {}
                        None => return Err(Why::missing(k)),
                        Some(x) => fits_at(types, fty, x, true, depth + 1).map_err(|w| w.here(fty, x).inside(k))?,
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Whether a type has a record in it (then a pattern makes the value).
fn has_record(types: &Types, ty: &Ty, depth: usize) -> bool {
    depth < 8
        && (ty.names.iter().any(|n| types.get(n).map_or(false, |td| td.is_record() || has_record(types, &td.ty, depth + 1)))
            || ty.elem.as_deref().map_or(false, |e| has_record(types, e, depth + 1)))
}

/// `v` made into a value of type `ty`: records made from maps (defaults
/// put in, other fields left out), lists of them item by item, the rest
/// checked as it is. `lenient`: text converts to the scalar asked for, as
/// form fields and query values do in patterns.
pub fn make(it: &mut Interp, ty: &Ty, v: &Value, lenient: bool) -> Result<Result<Value, Why>, Ctrl> {
    make_at(it, ty, v, lenient, 0)
}

fn make_at(it: &mut Interp, ty: &Ty, v: &Value, lenient: bool, depth: usize) -> Result<Result<Value, Why>, Ctrl> {
    if (ty.optional && matches!(v, Value::Nil)) || depth > DEPTH {
        return Ok(Ok(v.clone()));
    }
    let mut best = Why::default();
    for n in &ty.names {
        let r = make_name(it, n, ty, v, lenient, depth)?;
        match r {
            Ok(x) => return Ok(Ok(x)),
            Err(w) if best.what.is_empty() && !w.what.is_empty() => best = w,
            Err(_) => {}
        }
    }
    Ok(Err(best))
}

fn make_name(it: &mut Interp, n: &str, ty: &Ty, v: &Value, lenient: bool, depth: usize) -> Result<Result<Value, Why>, Ctrl> {
    if let Some(td) = it.types.get(n).cloned() {
        if !td.is_record() {
            return make_at(it, &td.ty, v, lenient, depth + 1);
        }
        let Value::Map(m) = v else { return Ok(Err(Why::default())) };
        return make_record(it, &td, m, lenient, depth);
    }
    match (n, v) {
        ("list", Value::List(items)) if ty.elem.as_deref().map_or(false, |e| has_record(&it.types, e, 0)) => {
            let e = ty.elem.as_deref().unwrap();
            let mut out = Vec::with_capacity(items.len());
            for (i, x) in items.iter().enumerate() {
                match make_at(it, e, x, lenient, depth + 1)? {
                    Ok(y) => out.push(y),
                    Err(w) => return Ok(Err(w.here(e, x).inside(&format!("[{}]", i)))),
                }
            }
            Ok(Ok(Value::list(out)))
        }
        (t, Value::Str(s)) if lenient && !type_name_matches(t, v) => match crate::interp::coerce(t, s) {
            Some(x) => Ok(Ok(x)),
            None => Ok(Err(Why::default())),
        },
        _ => {
            let one = Ty { names: vec![n.to_string()], optional: false, elem: ty.elem.clone(), fields: ty.fields.clone() };
            Ok(fits(&it.types, &one, v, true).map(|_| v.clone()))
        }
    }
}

fn make_record(it: &mut Interp, td: &TypeDef, m: &Map, lenient: bool, depth: usize) -> Result<Result<Value, Why>, Ctrl> {
    let mut out = Map::with_capacity(m.len());
    for (k, fty) in td.ty.fields.as_deref().unwrap_or_default() {
        let given = m.get(k.as_str()).filter(|x| !matches!(x, Value::Nil)).cloned();
        let x = match given {
            Some(x) => x,
            None => match td.defaults.iter().find(|(d, _)| d == k) {
                // Made again for each record: a default list is each one's own.
                Some((_, e)) => {
                    let globals = it.globals.clone();
                    it.eval(e, &globals)?
                }
                None if fty.optional => continue,
                None => return Ok(Err(Why::missing(k))),
            },
        };
        match make_at(it, fty, &x, lenient, depth + 1)? {
            Ok(y) => {
                out.insert(k.as_str().into(), y);
            }
            Err(w) => return Ok(Err(w.here(fty, &x).inside(k))),
        }
    }
    Ok(Ok(Value::map(out)))
}

/// `__record(name, fields)`: what `User(fields)` calls.
pub fn construct(it: &mut Interp, args: Vec<Value>, line: u32) -> Result<Value, Ctrl> {
    let name = match args.first() {
        Some(Value::Str(s)) => s.to_string(),
        _ => return crate::interp::fail(line, "`__record` is called by a record's name, as in User({ ... })"),
    };
    let Some(td) = it.types.get(&name).cloned() else {
        return crate::interp::fail(line, format!("there is no type `{}`", name));
    };
    let v = match args.get(1) {
        None | Some(Value::Nil) if td.is_record() => Value::map(Map::new()),
        Some(v) => v.clone(),
        None => Value::Nil,
    };
    if td.is_record() && !matches!(v, Value::Map(_)) {
        return Ok(Value::err(Value::str(format!("{}: expects a map of its fields, got {} {}", name, v.type_name(), short(&v)))));
    }
    let ty = Ty { names: vec![name.clone()], optional: false, elem: None, fields: None };
    Ok(match make(it, &ty, &v, false)? {
        Ok(x) => x,
        Err(w) => Value::err(Value::str(match w.here(&td.ty, &v).text() {
            Some(t) => format!("{}: {}", name, t),
            None => format!("{}: must be {}, got {} {}", name, td.ty.text(), v.type_name(), short(&v)),
        })),
    })
}
