use std::rc::Rc;

#[derive(Debug, Clone)]
pub enum Lit {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Rc<str>),
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
    Lambda(Rc<FnDef>),
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

#[derive(Debug, Clone)]
pub struct Ty {
    pub name: String,
    pub optional: bool,
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
    pub body: Expr,
    pub line: u32,
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
    Fn(Rc<FnDef>),
    Route(Rc<RouteDef>),
    Use(String, u32),
    Return(Option<Expr>),
}
