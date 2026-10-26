use std::sync::Arc;

#[derive(Debug, Clone)]
pub enum Lit {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    /// `12.50m`
    Dec(crate::decimal::Dec),
    Str(Arc<str>),
}

#[derive(Debug, Clone)]
pub enum TPart {
    Lit(String),
    Expr(Expr),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    Range,
}

#[derive(Debug, Clone, Copy)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Lit(Lit),
    Template(Vec<TPart>),
    Ident(String, u32),
    List(Vec<Expr>),
    Map(Vec<(String, Expr)>),
    Unary(UnOp, Box<Expr>, u32),
    Binary(BinOp, Box<Expr>, Box<Expr>, u32),
    /// `a ?? b`: `b` when `a` is null or an error.
    Coalesce(Box<Expr>, Box<Expr>),
    /// `a | f(b)` calls `f(a, b)`.
    Pipe(Box<Expr>, Box<Expr>, u32),
    Call(Box<Expr>, Vec<Expr>, u32),
    Field(Box<Expr>, String, u32),
    Index(Box<Expr>, Box<Expr>, u32),
    /// Postfix `?`: return an error value from the enclosing function.
    Try(Box<Expr>),
    If(Box<Expr>, Box<Expr>, Option<Box<Expr>>),
    Match(Box<Expr>, Vec<Arm>, u32),
    For(Pattern, Box<Expr>, Box<Expr>, u32),
    Block(Vec<Stmt>),
    Lambda(Arc<FnDef>),
    /// `reply status [body]`: finish the current request immediately.
    Reply(Box<Expr>, Option<Box<Expr>>, u32),
    Html(Box<HtmlEl>, u32),
}

#[derive(Debug, Clone)]
pub struct Arm {
    pub pat: Pattern,
    pub guard: Option<Expr>,
    pub body: Expr,
}

/// A type annotation: one type name or a union such as `int | str`.
/// `optional` (`str?`) also allows null. `list<int>` says what a list
/// holds (`elem`) and `{ title: str, tags: list<str> }` a map's fields;
/// only the checker reads these: at run time they are `list` and `map`.
#[derive(Debug, Clone, Default)]
pub struct Ty {
    pub names: Vec<String>,
    pub optional: bool,
    pub elem: Option<Box<Ty>>,
    pub fields: Option<Vec<(String, Ty)>>,
}

impl Ty {
    pub fn text(&self) -> String {
        let mut s = self
            .names
            .iter()
            .map(|n| match (n.as_str(), &self.elem, &self.fields) {
                ("list", Some(e), _) => format!("list<{}>", e.text()),
                ("map", _, Some(fs)) => {
                    let fs: Vec<String> = fs.iter().map(|(k, t)| format!("{}: {}", k, t.text())).collect();
                    format!("{{ {} }}", fs.join(", "))
                }
                _ => n.clone(),
            })
            .collect::<Vec<_>>()
            .join(" | ");
        if self.optional {
            s.push_str(if self.names.len() == 1 { "?" } else { " | null" });
        }
        s
    }
}

/// Effects a function can declare with `with`, and the builtins that use them.
pub const EFFECTS: &[&str] = &["db", "http", "fs", "env", "time", "random", "net"];
pub const ALL_EFFECTS: u8 = 0b111_1111;

pub fn effect_bit(name: &str) -> u8 {
    EFFECTS.iter().position(|e| *e == name).map_or(0, |i| 1 << i)
}

pub fn effect_mask(names: &[String]) -> u8 {
    names.iter().fold(0, |m, n| m | effect_bit(n))
}

pub fn effect_names(mask: u8) -> Vec<&'static str> {
    EFFECTS.iter().enumerate().filter(|(i, _)| mask & (1 << i) != 0).map(|(_, e)| *e).collect()
}

/// The effect a builtin needs, by its name; 0 for pure builtins.
pub fn builtin_effect(name: &str) -> u8 {
    let e = if name.starts_with("db.") {
        "db"
    } else if name.starts_with("http.") || name == "__service" || name == "__service_later" {
        "http"
    } else {
        match name {
            "file" | "read_file" | "write_file" | "delete_file" | "check_program" | "image.info" | "image.convert" => "fs",
            "env" => "env",
            "now" | "sleep" => "time",
            "random" | "random_int" | "uuid" | "crypto.token" | "crypto.hash_password" => "random",
            "udp.send" | "udp.reply" | "channel.send" | "channel.broadcast" => "net",
            _ => return 0,
        }
    };
    effect_bit(e)
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub ty: Option<Ty>,
    pub default: Option<Expr>,
}

#[derive(Debug)]
pub struct FnDef {
    pub name: String,
    pub params: Vec<Param>,
    pub ret: Option<Ty>,
    /// `! T`: what an error returned by this function must contain.
    pub err: Option<Ty>,
    /// `with db, http`: `None` when the function declares nothing.
    pub effects: Option<Vec<String>>,
    /// `server fn`: callable from the browser.
    pub server: bool,
    /// `component`: a view with `state` that also runs in the browser.
    pub component: bool,
    pub line: u32,
    pub body: Expr,
    /// How often it was called, for compiling it (src/jit.rs); `u32::MAX`
    /// once it is known that it cannot be.
    pub hot: std::sync::atomic::AtomicU32,
}

#[derive(Debug, Clone)]
pub enum Pattern {
    Wild,
    Bind(String),
    Lit(Lit),
    /// A type name such as `int`; matches any value of that type.
    Type(String),
    Alt(Vec<Pattern>),
    As(Box<Pattern>, String),
    /// Items, then the rest: `None` means exact length, `Some(None)` is `..`,
    /// `Some(Some(name))` is `..name`.
    List(Vec<Pattern>, Option<Option<String>>),
    Record(Vec<RecField>),
}

impl Pattern {
    /// A pattern that only tests the type: `int`, or `int | null`. As a
    /// record field it binds the field's name: `{ page: int | null }`.
    pub fn is_type_test(&self) -> bool {
        match self {
            Pattern::Type(_) => true,
            Pattern::Alt(ps) => {
                ps.iter().all(|p| matches!(p, Pattern::Type(_) | Pattern::Lit(Lit::Nil)))
                    && ps.iter().any(|p| matches!(p, Pattern::Type(_)))
            }
            _ => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RecField {
    /// `customer.id` is `["customer", "id"]`.
    pub path: Vec<String>,
    pub pat: Pattern,
    pub default: Option<Expr>,
}

#[derive(Debug, Clone)]
pub enum Seg {
    Lit(String),
    Param(String, Option<String>),
    Rest(String),
}

#[derive(Debug)]
pub struct RouteDef {
    pub method: String,
    pub path: Vec<Seg>,
    pub query: Option<Pattern>,
    pub body_pat: Option<Pattern>,
    pub body: Expr,
    pub line: u32,
    /// Whether the route can see `request`: its body (or a default in its
    /// patterns) names it, or it has a body pattern, which is matched
    /// against it. Otherwise the request map is made only if something asks
    /// for it (`url()`, a server fn).
    pub uses_request: bool,
}

#[derive(Debug, Clone)]
pub enum AttrVal {
    Bool,
    Expr(Expr),
    /// `{...map}`: each key of the map becomes an attribute (or a prop).
    /// Stored under the name `...`.
    Spread(Expr),
}

#[derive(Debug, Clone)]
pub enum HtmlChild {
    El(HtmlEl),
    /// A "text" literal or an `{expression}`; both are escaped when rendered.
    Expr(Expr),
}

#[derive(Debug, Clone)]
pub struct HtmlEl {
    pub tag: String,
    pub id: Option<Expr>,
    pub classes: Vec<Expr>,
    pub attrs: Vec<(String, AttrVal)>,
    pub children: Vec<HtmlChild>,
    pub line: u32,
    /// What rendering needs to know of it, worked out when it is parsed.
    pub facts: ElFacts,
}

/// A part of an element whose tags never change (see `ElFacts::pieces`).
#[derive(Debug, Clone)]
pub enum Piece {
    Html(Arc<str>),
    Expr(Expr),
    El(HtmlEl),
}

/// An element's facts for the server's renderer (src/interp.rs), so that
/// rendering one does not look at its tag's text again.
#[derive(Default)]
pub struct ElFacts {
    /// A capitalized tag: a view or component.
    pub component: bool,
    /// A void element (`<br>`): no children, no closing tag.
    pub void: bool,
    /// `<html>`, which the doctype goes before.
    pub html: bool,
    /// `<style>`, whose text is raw.
    pub style: bool,
    /// No `{...spread}`, no `class=`, no <textarea>: attributes are written
    /// as they are evaluated.
    pub plain_attrs: bool,
    /// Nothing in it changes (literal attributes and text, elements likewise,
    /// no URLs a base path changes): its HTML is made once, kept in `html_once`.
    pub fixed: bool,
    pub html_once: std::sync::OnceLock<Arc<str>>,
    /// Its tags and attributes, and those of the elements in it like it,
    /// never change (only what is shown in them does): it is rendered as
    /// `pieces`, fixed HTML between what is evaluated, made on first use.
    pub tags_fixed: bool,
    pub pieces: std::sync::OnceLock<Vec<Piece>>,
    /// The length of its HTML last time, to make room for it at once.
    pub last_len: std::sync::atomic::AtomicUsize,
}

impl ElFacts {
    pub fn of(tag: &str, id: &Option<Expr>, classes: &[Expr], attrs: &[(String, AttrVal)], children: &[HtmlChild], void: bool) -> ElFacts {
        let lit = |e: &Expr| matches!(e, Expr::Lit(_));
        let component = tag.starts_with(|c: char| c.is_uppercase());
        let plain_attrs = tag != "textarea" && attrs.iter().all(|(n, a)| !matches!(a, AttrVal::Spread(_)) && n != "class");
        let tags_fixed = !component
            && plain_attrs
            && tag != "html"
            && tag != "style"
            && id.as_ref().map_or(true, lit)
            && classes.iter().all(lit)
            && attrs.iter().all(|(n, v)| match v {
                AttrVal::Bool => true,
                AttrVal::Expr(e) => lit(e) && !crate::interp::url_attr(n),
                AttrVal::Spread(_) => false,
            });
        let fixed = !component
            && id.as_ref().map_or(true, lit)
            && classes.iter().all(lit)
            && attrs.iter().all(|(n, v)| match v {
                AttrVal::Bool => true,
                AttrVal::Expr(e) => {
                    lit(e) && !["href", "src", "action", "formaction", "poster"].iter().any(|u| u.eq_ignore_ascii_case(n))
                }
                AttrVal::Spread(_) => false,
            })
            && children.iter().all(|c| match c {
                HtmlChild::El(e) => e.facts.fixed,
                HtmlChild::Expr(e) => matches!(e, Expr::Lit(Lit::Str(_))),
            });
        ElFacts {
            component,
            void,
            html: tag == "html",
            style: tag == "style",
            plain_attrs,
            fixed,
            html_once: std::sync::OnceLock::new(),
            tags_fixed,
            pieces: std::sync::OnceLock::new(),
            last_len: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

impl Clone for ElFacts {
    fn clone(&self) -> ElFacts {
        ElFacts {
            html_once: self.html_once.clone(),
            pieces: self.pieces.clone(),
            last_len: std::sync::atomic::AtomicUsize::new(self.last_len.load(std::sync::atomic::Ordering::Relaxed)),
            ..*self
        }
    }
}

impl std::fmt::Debug for ElFacts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ElFacts")
    }
}

#[derive(Debug, Clone, Copy)]
pub enum AssignOp {
    Set,
    Add,
    Sub,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Let(Pattern, Expr, u32),
    Assign(String, AssignOp, Expr, u32),
    Expr(Expr),
    Fn(Arc<FnDef>),
    Route(Arc<RouteDef>),
    Use(String, u32),
    Return(Option<Expr>),
    /// `state name = expr` (or `~name = expr`) in a component: set once, kept between renders.
    State(String, Expr, u32),
    /// `mount /prefix = "app.qwe"`: another program, served under the prefix.
    Mount(String, String, u32),
    /// `service billing = "billing.qwe"` (its `server fn`s called as
    /// `billing.invoice(...)`), or `service /shop = "shop.qwe"` (the same,
    /// named after the file, and its routes served under /shop): a part
    /// that runs in this process or as a server of its own (src/service.rs).
    /// After the file, a map of how it is called: `{ timeout: "2s", retries: 2 }`.
    Service(String, Option<String>, String, Option<Expr>, u32),
    /// `type User = { name: str, role: "admin" | "member" = "member" }`: a
    /// record (src/record.rs), or another name for a type.
    Type(Arc<TypeDef>),
}

/// A type with a name: a record's fields (and their defaults), or a type
/// the name stands for.
#[derive(Debug)]
pub struct TypeDef {
    pub name: String,
    /// A record is a `map` with `fields`.
    pub ty: Ty,
    /// The fields that have a default, and its expression.
    pub defaults: Vec<(String, Expr)>,
    pub line: u32,
}

impl TypeDef {
    pub fn is_record(&self) -> bool {
        self.ty.fields.is_some() && self.ty.names == ["map"]
    }
}

/// Whether a name is one of the program's own types (a record, or another
/// named type): written like `User` or `OrderLine`, a capital letter first
/// and a small one in it, unlike constants such as `MAX` or `NODE_W`.
pub fn is_type_name(n: &str) -> bool {
    n.chars().next().map_or(false, |c| c.is_ascii_uppercase()) && n.chars().any(|c| c.is_ascii_lowercase())
}
