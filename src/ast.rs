use std::sync::Arc;

#[derive(Debug, Clone)]
pub enum Lit {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
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
    /// `a ?? b`: `b` when `a` is nil or an error.
    Coalesce(Box<Expr>, Box<Expr>),
    /// `a |> f(b)` calls `f(a, b)`.
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
/// `optional` (`str?`) also allows nil.
#[derive(Debug, Clone)]
pub struct Ty {
    pub names: Vec<String>,
    pub optional: bool,
}

impl Ty {
    pub fn text(&self) -> String {
        let mut s = self.names.join(" | ");
        if self.optional {
            s.push_str(if self.names.len() == 1 { "?" } else { " | nil" });
        }
        s
    }
}

/// Effects a function can declare with `with`, and the builtins that use them.
pub const EFFECTS: &[&str] = &["db", "http", "fs", "env", "time", "random"];
pub const ALL_EFFECTS: u8 = 0b11_1111;

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
    } else if name.starts_with("http.") {
        "http"
    } else {
        match name {
            "file" | "read_file" | "write_file" => "fs",
            "env" => "env",
            "now" | "sleep" => "time",
            "random" | "random_int" | "uuid" => "random",
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
    pub line: u32,
    pub body: Expr,
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
}

#[derive(Debug, Clone)]
pub enum AttrVal {
    Bool,
    Expr(Expr),
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
}
