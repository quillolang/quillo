pub use crate::sym::Sym;
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
    /// Trusted CSS of the text its value is: what the parser puts around a component's or view's
    /// `style` written at its tag as a template or a text, whose values it escaped
    /// (`parser::style_values`). No program can write it (FEEDBACK79.md, Q-1250, the user's decision; branch feedback72-81).
    Css,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Lit(Lit),
    Template(Vec<TPart>),
    Ident(Sym, u32),
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
    /// `while cond { body }`: runs the body while the condition holds; null.
    While(Box<Expr>, Box<Expr>, u32),
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
    /// The line its pattern is on.
    pub line: u32,
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

/// A short expression as it is written, for a message that says what to write instead: a name,
/// a number, a text, `a.b`, `a[0]`; anything longer is `...` (FEEDBACK64.md, Q-934; branch
/// feedback64).
pub fn written(e: &Expr) -> String {
    match e {
        Expr::Ident(n, _) => n.to_string(),
        Expr::Lit(Lit::Int(n)) => n.to_string(),
        // `2.0` stays a float: `2` would be an int (found in review).
        Expr::Lit(Lit::Float(f)) if f.is_finite() && f.fract() == 0.0 && f.abs() < 1e15 => format!("{:.1}", f),
        Expr::Lit(Lit::Float(f)) => f.to_string(),
        Expr::Lit(Lit::Bool(b)) => b.to_string(),
        Expr::Lit(Lit::Nil) => "null".to_string(),
        Expr::Lit(Lit::Str(s)) if s.len() <= 40 && !s.contains(['"', '\\', '\n']) => format!("\"{}\"", s),
        Expr::Unary(UnOp::Neg, x, _) if matches!(&**x, Expr::Lit(Lit::Int(_) | Lit::Float(_))) => format!("-{}", written(x)),
        Expr::Field(o, f, _) if matches!(&**o, Expr::Ident(..)) => format!("{}.{}", written(o), f),
        Expr::Index(o, i, _) if matches!(&**o, Expr::Ident(..)) && !matches!(written(i).as_str(), "...") => format!("{}[{}]", written(o), written(i)),
        _ => "...".to_string(),
    }
}

/// A map key as a literal map writes it: `k` when it is a name, else in quotes; none when quotes
/// alone cannot hold it (a quote, a backslash, a line break: found in review).
pub fn key_written(k: &str) -> Option<String> {
    let name = k.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if name {
        Some(k.to_string())
    } else if k.chars().any(|c| c == '"' || c == '\\' || c.is_control()) {
        None
    } else {
        Some(format!("\"{}\"", k))
    }
}

/// Whether a map key is a name, as `m.k` and `el.value` write it.
pub fn key_is_name(k: &str) -> bool {
    key_written(k).is_some_and(|w| w == k)
}

/// The effect a builtin needs, by its name; 0 for pure builtins.
pub fn builtin_effect(name: &str) -> u8 {
    // Reads an error value it is given; asks the database nothing.
    if name == "db.error_kind" || name == "db.sql_name" {
        return 0;
    }
    // A `.qwer` function: what its file declares (FEEDBACK43.md, Q-489; branch feedback40-43).
    if name.starts_with("native:") {
        return crate::native::effects_of(name);
    }
    let e = if name.starts_with("db.") || name == "idempotent" || name == "later.status" {
        "db"
    // A site's function is a request to it (FEEDBACK51.md, Q-640; branch feedback51-fixes).
    } else if name.starts_with("http.") || name == "__service" || name == "__service_later" || name == "__site_call" {
        "http"
    } else {
        match name {
            // `save_upload` writes a file (FEEDBACK41.md, Q-476; branch feedback40-43).
            "file" | "read_file" | "write_file" | "delete_file" | "check_program" | "image.info" | "image.convert" | "save_upload" | "files" => "fs",
            "env" | "args" | "stdin" | "read_line" | "exit" => "env",
            "now" | "sleep" | "date.today" | "date.now" => "time",
            "random" | "random_int" | "uuid" | "crypto.token" | "crypto.hash_password" => "random",
            // Listening is net too: a port opened (FEEDBACK41.md, Q-478; branch feedback40-43).
            "udp.send" | "udp.reply" | "udp.listen" | "media.rtp" | "channel.send" | "channel.broadcast" => "net",
            _ => return 0,
        }
    };
    effect_bit(e)
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: Sym,
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
    /// `fn mcp name(...)`, `server fn mcp name(...)`: a tool, for AI clients; to the program a
    /// function as any other (FEEDBACK56.md, Q-853; branch feedback56).
    pub mcp: bool,
    /// `component`: a view with `state` that also runs in the browser.
    pub component: bool,
    pub line: u32,
    pub body: Expr,
    /// How deeply its body nests (at most `parser::MAX_NESTING`): a call
    /// makes sure there is stack for that much.
    pub nesting: u32,
    /// The file it was written in (an absolute path), when the parser was
    /// told: for the debugger's stack frames and breakpoints.
    pub file: Option<Arc<str>>,
    /// The `///` lines right above it, one line of text each, which `quillo docs` writes as
    /// what it does; a `//` line is not one (FEEDBACK56.md, Q-854; branch feedback56).
    pub doc: Option<String>,
    /// How often it was called, for compiling it (src/jit.rs); `u32::MAX`
    /// once it is known that it cannot be.
    pub hot: std::sync::atomic::AtomicU32,
    /// What its code does with `request`, once asked (see `FnDef::request_use`).
    pub request_use: std::sync::atomic::AtomicU8,
    /// Whether a component runs by itself, once asked (see `FnDef::runs_by_itself`).
    pub runs_by_itself: std::sync::atomic::AtomicU8,
    /// The sandboxed script's run whose source it was read from (`run_script`;
    /// `script::parsing_run`), 0 for the program's: a function made of it runs only while that run
    /// goes on, as the script's (`Interp::script_mask`, `Interp::script_runs`; Q-1274, found reviewing the trusted CSS of
    /// FEEDBACK79.md's Q-1250; branch feedback72-81).
    pub run: u64,
}

impl FnDef {
    /// Whether its parameters' defaults or its body name `request`, and whether they can read the
    /// visitor from it (see `parser::request_use`). Worked out once: it was walked on each call
    /// of a server fn in a `live` run (FEEDBACK55.md, Q-880; branch feedback55).
    pub fn request_use(&self) -> (bool, bool) {
        use std::sync::atomic::Ordering::Relaxed;
        const KNOWN: u8 = 1;
        const NAMES: u8 = 2;
        const VISITOR: u8 = 4;
        let mut known = self.request_use.load(Relaxed);
        if known == 0 {
            let (names, visitor) = crate::parser::request_use(|f| crate::check::walk_fn(self, f));
            known = KNOWN | if names { NAMES } else { 0 } | if visitor { VISITOR } else { 0 };
            self.request_use.store(known, Relaxed);
        }
        (known & NAMES != 0, known & VISITOR != 0)
    }

    /// Whether its body starts timers, live data or a channel, or runs JavaScript, without anyone
    /// touching it (`island::runs_by_itself`). Worked out once, not on each render: the body was
    /// walked on each, and `channel.open` made the walk longer (FEEDBACK66.md, Q-953; branch
    /// feedback66).
    pub fn runs_by_itself(&self) -> bool {
        use std::sync::atomic::Ordering::Relaxed;
        const KNOWN: u8 = 1;
        const YES: u8 = 2;
        let mut known = self.runs_by_itself.load(Relaxed);
        if known & KNOWN == 0 {
            // Or'ed in: the same byte keeps `listens_to_scroll`'s answer (Q-1010).
            known = KNOWN | if crate::island::runs_by_itself(&self.body) { YES } else { 0 };
            self.runs_by_itself.fetch_or(known, Relaxed);
        }
        known & YES != 0
    }

    /// Whether an element of its body listens to `scroll` (or `scrollend`), which comes with no
    /// pointer, touch, focus or key when a script or a link moves the box: its island says so
    /// (`data-sc`), and the page's loader takes it up on a scroll (`island_loader` in server.rs).
    /// Worked out once, in `runs_by_itself`'s byte (FEEDBACK72.md, Q-1010; branch feedback72-81).
    pub fn listens_to_scroll(&self) -> bool {
        use std::sync::atomic::Ordering::Relaxed;
        const KNOWN: u8 = 4;
        const YES: u8 = 8;
        let mut known = self.runs_by_itself.load(Relaxed);
        if known & KNOWN == 0 {
            // The elements of each literal, through their children, looked at in a loop (the
            // expressions by `walk_expr`, which goes into the markup in them).
            let mut hit = false;
            crate::check::walk_expr(&self.body, &mut |x| {
                let Expr::Html(root, _) = x else { return };
                let mut todo = vec![&**root];
                while let Some(el) = todo.pop().filter(|_| !hit) {
                    hit = el.attrs.iter().any(|(n, _)| n == "@scroll" || n == "@scrollend");
                    todo.extend(el.children.iter().filter_map(|c| if let HtmlChild::El(e) = c { Some(e) } else { None }));
                }
            });
            known = KNOWN | if hit { YES } else { 0 };
            self.runs_by_itself.fetch_or(known, Relaxed);
        }
        known & YES != 0
    }
}

#[derive(Debug, Clone)]
pub enum Pattern {
    Wild,
    Bind(Sym),
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
            // `{ tag: [str] }`: every value of a query's or a form's name.
            Pattern::List(items, None) => items.len() == 1 && items[0].is_type_test(),
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
    /// `:name`, `:name(int)`; the last is what the piece starts with before the parameter,
    /// `@` in `/@:name`, a profile's address (FEEDBACK53.md, Q-683; branch main), or "".
    Param(String, Option<String>, String),
    Rest(String),
}

/// The method of a guarded group's hidden route (`routes /admin | admins { ... }`): its
/// middleware, run for a request under the group's path that no route of it answers, before
/// the server's 400, 404, 405, 501 or `OPTIONS` say what is there (FEEDBACK47.md, Q-539; branch
/// feedback40-43). A program cannot write this method.
pub const GUARD_METHOD: &str = "__GUARD";

#[derive(Debug)]
pub struct RouteDef {
    pub method: String,
    pub path: Vec<Seg>,
    pub query: Option<Pattern>,
    pub body_pat: Option<Pattern>,
    /// The request patterns of the `routes` groups around it, outermost
    /// first: each must match as `body_pat` does (branch route-groups).
    pub guards: Vec<Pattern>,
    pub body: Expr,
    pub line: u32,
    /// The file it was written in, as for `FnDef::file`.
    pub file: Option<Arc<str>>,
    /// Whether the route can see `request`: its body (or a default in its
    /// patterns) names it, or it has a body pattern, which is matched
    /// against it. Otherwise the request map is made only if something asks
    /// for it (`url()`, a server fn).
    pub uses_request: bool,
    /// Whether its body or its patterns can read the visitor from the request: the page is
    /// theirs (see `parser::request_use`; FEEDBACK55.md, Q-886; branch feedback55).
    pub reads_visitor: bool,
    /// Whether its groups' middleware can, once known (see `Interp::middleware_reads`).
    pub middleware_reads: std::sync::OnceLock<bool>,
    /// Whether it reads more of the request it is handed than the session, once known, for what
    /// Quillo's service worker keeps (see `Interp::middleware_beyond`; FEEDBACK67.md, B.2, review;
    /// branch pwa).
    pub middleware_beyond: std::sync::OnceLock<bool>,
    /// `route GET /report/:day with db { ... }`: what its body, and the middleware of its
    /// groups, may do; the group's `with` narrowed by its own (branch feedback40-43).
    pub effects: Option<Vec<String>>,
    /// The same as a function the runtime holds while the route runs, for its messages
    /// (`route GET /report/:day` is declared `with db`); made by `limited`.
    pub limit: Option<Arc<FnDef>>,
    /// `route offline GET /menu`, or a GET route in `routes offline /docs { ... }`: its page is
    /// kept by Quillo's service worker, for when there is no network (FEEDBACK67.md, B.2; branch
    /// pwa).
    pub offline: bool,
    /// `route writes GET /confirm/:code`, or a GET route in `routes writes /p { ... }`: the GET
    /// writes on purpose, so `quillo check` does not warn that a link from another site runs its
    /// write with the visitor's session (FEEDBACK70.md, Q-965; branch security).
    pub writes: bool,
}

impl RouteDef {
    /// `GET /report/:day`, as written.
    pub fn shown(&self) -> String {
        let mut s = String::new();
        for seg in &self.path {
            s.push('/');
            match seg {
                Seg::Lit(l) => s.push_str(l),
                Seg::Param(n, Some(t), p) => s.push_str(&format!("{}:{}({})", p, n, t)),
                Seg::Param(n, None, p) => s.push_str(&format!("{}:{}", p, n)),
                Seg::Rest(n) => s.push_str(&format!("*{}", n)),
            }
        }
        if s.is_empty() {
            s.push('/');
        }
        // A guarded group's hidden route is its group, in what a message says of it.
        if self.method == GUARD_METHOD {
            let group = s.strip_suffix("/*__rest").unwrap_or(&s);
            return format!("routes {}", if group.is_empty() { "/" } else { group });
        }
        format!("{} {}", self.method, s)
    }

    /// Sets `limit` from `effects` and the route's path.
    pub fn limited(mut self) -> Self {
        self.limit = self.effects.as_ref().map(|e| {
            Arc::new(FnDef {
                name: format!("route {}", self.shown()),
                params: Vec::new(),
                ret: None,
                err: None,
                effects: Some(e.clone()),
                server: false,
                mcp: false,
                component: false,
                body: Expr::Lit(Lit::Nil),
                line: self.line,
                nesting: 1,
                file: self.file.clone(),
                doc: None,
                hot: Default::default(),
                request_use: Default::default(), runs_by_itself: Default::default(), run: 0,
            })
        });
        self
    }
}

#[derive(Debug, Clone)]
pub enum AttrVal {
    Bool,
    Expr(Expr),
    /// `{...map}`: each key of the map becomes an attribute (or a prop).
    /// Stored under the name `...`.
    Spread(Expr),
}

/// What a person can change of an element in the page, by its attribute: the event that says
/// it changed, and the property that holds it. `bind:x={state}` keeps the two in step, and so
/// does `x={state}` when nothing else on the element sets it (branch feedback40-43).
pub fn bind_event(tag: &str, attr: &str) -> Option<(&'static str, &'static str)> {
    Some(match (tag, attr) {
        ("input" | "textarea" | "select", "value") => ("input", "value"),
        ("input", "checked") => ("change", "checked"),
        ("input", "indeterminate") => ("change", "indeterminate"),
        ("details", "open") => ("toggle", "open"),
        ("dialog", "open") => ("close", "open"),
        ("video" | "audio", "muted") => ("volumechange", "muted"),
        ("video" | "audio", "volume") => ("volumechange", "volume"),
        ("video" | "audio", "currentTime") => ("timeupdate", "currentTime"),
        ("video" | "audio", "playbackRate") => ("ratechange", "playbackRate"),
        // On another element (a custom one), `bind:value` and `bind:checked` as on an input.
        (_, "value") if tag.contains('-') => ("input", "value"),
        (_, "checked") if tag.contains('-') => ("change", "checked"),
        _ => return None,
    })
}

/// Attributes whose value the browser runs as code (`onclick`) or shows as
/// a page of the site (`srcdoc`). Escaping does not help there: the browser
/// decodes the value first.
pub fn code_attr(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    let name = name.strip_prefix("bind:").unwrap_or(&name);
    name.starts_with("on") || name == "srcdoc"
}

/// Whether attribute `name` of element `tag` is given a value worked out
/// when the page is made, where only text written in the program may go:
/// `onclick={...}`. A component's attributes are its parameters, so they
/// may be anything; and an `<iframe>` whose written `sandbox` keeps its
/// page away from the site (no allow-same-origin) may show any `srcdoc`.
pub fn code_from_data(tag: &str, attrs: &[(String, AttrVal)], name: &str, v: &AttrVal) -> bool {
    if tag.starts_with(|c: char| c.is_uppercase()) || !code_attr(name) || matches!(v, AttrVal::Bool | AttrVal::Expr(Expr::Lit(_))) {
        return false;
    }
    let sandboxed = || {
        attrs.iter().any(|(n, v)| {
            n.eq_ignore_ascii_case("sandbox")
                && match v {
                    AttrVal::Bool => true,
                    AttrVal::Expr(Expr::Lit(Lit::Str(s))) => !s.to_ascii_lowercase().contains("allow-same-origin"),
                    _ => false,
                }
        })
    };
    !(name.eq_ignore_ascii_case("srcdoc") && sandboxed())
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
    /// The lines where its text has a `$` right before a `{`, as a template's
    /// `${name}` is written, for `quillo check` to ask about: once parsed, `$` is
    /// text like `{"$"}`, which means it (FEEDBACK20.md, Q-170; branch feedback20-fixes).
    pub dollars: Vec<u32>,
    /// Text and an inline element on lines of their own, which JSX's rule joins with no space
    /// ("Hello,<b>Ann</b>"): the line, and the two as `quillo check` names them (FEEDBACK48.md,
    /// Q-573; branch main).
    pub joined: Vec<(u32, String, String)>,
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
    /// One of HTML's void names with nothing in it in an XML answer (an Atom feed's `<link>`):
    /// written `<link .../>`, as XML has it (FEEDBACK79.md, Q-1259; branch feedback72-81).
    pub xml_empty: bool,
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
    /// A `<select value={...}>`: its value is shown as `selected` on its options, rendered each
    /// time (FEEDBACK55.md, Q-758; branch feedback55).
    pub select_value: bool,
    /// `<svg>` (true) and `<foreignObject>` (false): what is in it is SVG's or HTML's, which the
    /// components in it are made for (FEEDBACK55.md, Q-763; branch feedback55).
    pub svg: Option<bool>,
    /// What its tag is to the browser's parser where HTML, SVG and MathML meet, and whether it is
    /// `<svg>` or `<math>`, where foreign content starts: the renderer follows where the parser is
    /// only inside them, so that an `html` value in a script the parser reads as markup goes in a
    /// CDATA section (see `crate::foreign`; FEEDBACK71.md, Q-969; branch security).
    pub ns: crate::foreign::Kind,
    pub ns_opens: bool,
    /// It, or an element in it, may end foreign content where it stands (`<p>`, `<div>`): inside
    /// `<svg>` its HTML, made once, is written as made, and what follows is where it leaves the
    /// parser.
    pub breaks: bool,
    /// It is written from pieces of its own (`tags_fixed`, but not a `<pre>`, whose text may need
    /// a newline first), and its tags go into its parent's pieces (`flat`: not an `<svg>`, a
    /// `<foreignObject>` or a `<math>` either, which change what is in them). Worked out when it
    /// is parsed, where its tag was read for it on each render (see `Interp::pieces_into`;
    /// FEEDBACK71.md, Q-969; branch security).
    pub pieces_root: bool,
    pub flat: bool,
    /// The tags its pieces hold of the elements in it change nothing of where the parser is
    /// (`foreign::Kind::Other`): its pieces are written as they are inside `<svg>` too.
    pub pieces_plain: bool,
    /// What it writes depends on where the parser is: a `<script>` with a value in it, a view or a
    /// component (whose body may have one), a value code makes (`{chart(x)}`); and whether its
    /// children do, which a view's call is rendered before the view with.
    pub sensitive: bool,
    pub kids_sensitive: bool,
    /// Nothing it writes depends on where the parser is, nor can hold a script: no view or
    /// component, no `<script>` with a value, no value but literal markup (`{for p in ps =>
    /// <circle cx={p.x}/>}`). An `<svg>` like it, in HTML, is written as HTML pages are, where
    /// the parser is not followed (FEEDBACK71.md, Q-969, found in review; branch security).
    pub ns_free: bool,
    /// Its tag, and those of the elements in it, change nothing of where the parser is inside
    /// `<svg>` or `<math>` (`foreign::Kind::Other`: no integration point, no tag that ends foreign
    /// content), with no spread and no `<script>` with a value: there it is written as in HTML,
    /// its values looked at (FEEDBACK71.md, Q-969, found in review; branch security).
    pub ns_still: bool,
    /// Where the view it calls puts its children, for that view called where it was
    /// (`foreign::kept`): a view is walked once for each place it is called from.
    pub kids_at: std::sync::atomic::AtomicU64,
}

/// Whether markup written in `e` (in a branch, a loop, a block, a lambda) may end foreign content
/// (`ElFacts::breaks`).
fn expr_breaks(e: &Expr) -> bool {
    let mut hit = false;
    crate::check::walk_expr(e, &mut |x| {
        if let Expr::Html(el, _) = x {
            hit |= el.facts.breaks;
        }
    });
    hit
}

/// Whether `e`, a child of an element, writes only markup the program wrote: a literal, an element
/// (`ElFacts::ns_free`), or a branch or a loop of them; no value, which may be html made apart.
fn markup_only(e: &Expr) -> bool {
    match e {
        // A text (`{"Points"}`, `"${n} items"`), which the page escapes.
        Expr::Lit(_) | Expr::Template(_) => true,
        Expr::Html(el, _) => el.facts.ns_free,
        Expr::If(_, a, b) => markup_only(a) && b.as_deref().is_none_or(markup_only),
        Expr::For(_, _, body, _) => markup_only(body),
        Expr::Match(_, arms, _) => arms.iter().all(|a| markup_only(&a.body)),
        _ => false,
    }
}

/// Whether what `e` writes in a page is the same wherever the parser is: its value, made by no
/// code that may write markup (a variable's html was made already). See `ElFacts::sensitive`.
fn writes_alike(e: &Expr) -> bool {
    match e {
        Expr::Lit(_) | Expr::Ident(..) => true,
        Expr::Field(x, _, _) | Expr::Unary(_, x, _) => writes_alike(x),
        Expr::Index(a, b, _) | Expr::Binary(_, a, b, _) | Expr::Coalesce(a, b) => writes_alike(a) && writes_alike(b),
        Expr::Template(parts) => parts.iter().all(|p| match p {
            TPart::Lit(_) => true,
            TPart::Expr(x) => writes_alike(x),
        }),
        _ => false,
    }
}

impl ElFacts {
    pub fn of(tag: &str, id: &Option<Expr>, classes: &[Expr], attrs: &[(String, AttrVal)], children: &[HtmlChild], void: bool) -> ElFacts {
        let lit = |e: &Expr| matches!(e, Expr::Lit(_));
        let component = tag.starts_with(|c: char| c.is_uppercase());
        let select_value = tag == "select" && attrs.iter().any(|(n, _)| n == "value" || n == "bind:value");
        let plain_attrs = tag != "textarea" && attrs.iter().all(|(n, a)| !matches!(a, AttrVal::Spread(_)) && n != "class");
        let tags_fixed = !component
            && !select_value
            && plain_attrs
            && tag != "html"
            && tag != "style"
            && tag != "script"
            && id.as_ref().map_or(true, lit)
            && classes.iter().all(lit)
            && attrs.iter().all(|(n, v)| match v {
                AttrVal::Bool => true,
                AttrVal::Expr(e) => lit(e) && !crate::interp::url_attr(n),
                AttrVal::Spread(_) => false,
            });
        let fixed = !component
            && !select_value
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
        let ns = crate::foreign::kind_of(tag, attrs, component);
        let ns_opens = matches!(ns, crate::foreign::Kind::Svg | crate::foreign::Kind::Math);
        let svg = match tag {
            "svg" => Some(true),
            "foreignObject" => Some(false),
            _ => None,
        };
        let pieces_root = tags_fixed && !matches!(tag, "pre" | "textarea" | "listing");
        let flat = pieces_root && svg.is_none() && !ns_opens;
        let els = || children.iter().filter_map(|c| if let HtmlChild::El(e) = c { Some(e) } else { None });
        // Markup an expression in it writes counts too: `{c ? <p>x</p> : ""}` ends foreign
        // content as `<p>x</p>` does (found in review).
        let breaks = matches!(ns, crate::foreign::Kind::Breakout | crate::foreign::Kind::FontAsk)
            || children.iter().any(|c| match c {
                HtmlChild::El(e) => e.facts.breaks,
                HtmlChild::Expr(x) => expr_breaks(x),
            });
        let pieces_plain = els().all(|e| !e.facts.flat || (e.facts.ns == crate::foreign::Kind::Other && e.facts.pieces_plain));
        let kids_sensitive = children.iter().any(|c| match c {
            HtmlChild::El(e) => e.facts.sensitive,
            HtmlChild::Expr(x) => !writes_alike(x),
        });
        let dyn_script = tag.eq_ignore_ascii_case("script") && children.iter().any(|c| !matches!(c, HtmlChild::Expr(Expr::Lit(Lit::Str(_)))));
        ElFacts {
            component,
            void,
            xml_empty: false,
            html: tag == "html",
            style: tag == "style",
            plain_attrs,
            fixed,
            html_once: std::sync::OnceLock::new(),
            tags_fixed,
            pieces: std::sync::OnceLock::new(),
            last_len: std::sync::atomic::AtomicUsize::new(0),
            select_value,
            svg,
            ns,
            ns_opens,
            breaks,
            pieces_root,
            flat,
            pieces_plain,
            sensitive: component || dyn_script || kids_sensitive,
            kids_sensitive,
            ns_still: !component
                && ns == crate::foreign::Kind::Other
                && !attrs.iter().any(|(_, a)| matches!(a, AttrVal::Spread(_)))
                && !dyn_script
                && els().all(|e| e.facts.ns_still),
            ns_free: !component
                && !dyn_script
                && children.iter().all(|c| match c {
                    HtmlChild::El(e) => e.facts.ns_free,
                    HtmlChild::Expr(x) => markup_only(x),
                }),
            kids_at: std::sync::atomic::AtomicU64::new(0),
        }
    }
}

impl Clone for ElFacts {
    fn clone(&self) -> ElFacts {
        ElFacts {
            html_once: self.html_once.clone(),
            pieces: self.pieces.clone(),
            last_len: std::sync::atomic::AtomicUsize::new(self.last_len.load(std::sync::atomic::Ordering::Relaxed)),
            kids_at: std::sync::atomic::AtomicU64::new(self.kids_at.load(std::sync::atomic::Ordering::Relaxed)),
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
    Assign(Sym, AssignOp, Expr, u32),
    /// `obj.name = expr` (or `+=`, `-=`): a field of one of the page's
    /// objects (an element, an event), set in a component.
    SetField(Expr, String, AssignOp, Expr, u32),
    Expr(Expr),
    Fn(Arc<FnDef>),
    Route(Arc<RouteDef>),
    /// `use "lib.qwe"`: every public name. With names, `import redis.qwer use cache_get
    /// as redis_get, cache_set`: those only, each under the name after `as`. From a
    /// script, `client "https://.../chart.umd.min.js" use Chart`: the names it gives the
    /// page, for the browser (see `is_script`; branch js-libraries), and `preload module
    /// ...` says how the page takes it (see `LibOptions`; branch library-hints); a
    /// module's `use * as THREE` is the name `"*"`.
    Use(String, Option<Vec<(String, String)>>, u32, LibOptions),
    /// With its line: a bare `return` has no expression to tell it
    /// (FEEDBACK16.md, Q-138; branch feedback20-fixes).
    Return(Option<Expr>, u32),
    /// `state name = expr` (or `~name = expr`) in a component: set once, kept between renders.
    State(String, Expr, u32),
    /// `mount /prefix = "app.qwe"`: another program, served under the prefix.
    Mount(String, String, u32),
    /// `service billing = "billing.qwe"` (its `server fn`s called as
    /// `billing.invoice(...)`), or `service /shop = "shop.qwe"` (the same,
    /// named after the file, and its routes served under /shop): a part
    /// that runs in this process or as a server of its own (src/service.rs).
    /// After the file, a map of how it is called: `{ timeout: "2s", retries: 2 }`.
    /// Last, whether it is `service mcp /billing = ...` (its tools offered here) or `service mcp
    /// crm = "https://..."` (another MCP server, for later) (FEEDBACK56.md, Q-853; branch
    /// feedback56).
    Service(String, Option<String>, String, Option<Expr>, u32, bool),
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

/// Whether a `use` names a browser library rather than a Quillo or Rust file: a
/// `.js` file or an address (`https://...`, `//...`). It is loaded by the pages
/// whose components use it, and the names it takes are what it defines on the
/// page (branch js-libraries).
pub fn is_script(path: &str) -> bool {
    // Before a `?v=2` as well, in any case (branch library-hints).
    let file = path.split(['?', '#']).next().unwrap_or(path).to_ascii_lowercase();
    file.ends_with(".js") || file.ends_with(".mjs") || is_address(path)
}

/// What a page does ahead of a browser library it can load (branch library-hints):
/// `preload` fetches its script from the `<head>`, `preconnect` only opens the
/// connection to its host.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LibHint {
    Preload,
    Preconnect,
}

/// How a browser library is taken (branch library-hints): `module` for an ES module,
/// whose exports are kept in the runtime rather than on the page, and a hint.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LibOptions {
    pub module: bool,
    pub hint: Option<LibHint>,
    /// Written as 0.6 did, `use names from file`, which `quillo fix` turns around
    /// (FEEDBACK19.md, Q-156; branch feedback19-fixes).
    pub old: bool,
    /// Its path was written without an extension, which the parser gave it: the file is the
    /// `.qwe`, `.qwer` or `.rs` one there (FEEDBACK55.md, Q-819; branch feedback55).
    pub implied: bool,
}

/// Whether a script says by its name that it is an ES module (branch library-hints).
pub fn is_module_file(path: &str) -> bool {
    path.split(['?', '#']).next().is_some_and(|p| p.to_ascii_lowercase().ends_with(".mjs"))
}

/// Whether a path is an address on the web. A scheme is the same in any case:
/// `HTTP://...` is no path on this site (branch library-hints).
pub fn is_address(path: &str) -> bool {
    let start = path.get(..8).unwrap_or(path).to_ascii_lowercase();
    start.starts_with("https://") || start.starts_with("http://") || path.starts_with("//")
}

/// Why a `use` cannot name this: nothing at all, a kind of address a page cannot
/// load a script from, or one with a password in it, which browsers refuse and the
/// page would show to everyone (branch library-hints).
pub fn bad_use_path(path: &str) -> Option<String> {
    if path.trim().is_empty() {
        return Some("`use` expects a file: use lib/auth.qwe".into());
    }
    let scheme = path.split_once(':').map(|(s, _)| s).filter(|s| {
        // One letter is a Windows drive.
        s.len() > 1 && s.starts_with(|c: char| c.is_ascii_alphabetic()) && s.chars().all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
    });
    if let Some(s) = scheme.filter(|_| !is_address(path)) {
        return Some(format!("`use` takes a file, or a script at https://, http:// or //, not a `{}:` address", s));
    }
    if script_origin(path).is_some_and(|o| o.contains('@')) {
        return Some(format!("`{}` has a user name in it, which browsers refuse to load and everyone could read: put the script where no password is needed", path));
    }
    None
}

/// Where a script at an address is served from (`https://cdn.example`,
/// `//cdn.example`), which `with preconnect` connects to; a path on the program's
/// own address has none (branch library-hints).
pub fn script_origin(path: &str) -> Option<&str> {
    if !is_address(path) {
        return None;
    }
    let host = path.find("//")? + 2;
    let end = path[host..].find(['/', '?', '#']).map_or(path.len(), |e| host + e);
    Some(&path[..end])
}

/// Whether a name is one of the program's own types (a record, or another
/// named type): written like `User` or `OrderLine`, a capital letter first
/// and a small one in it, unlike constants such as `MAX` or `NODE_W`.
pub fn is_type_name(n: &str) -> bool {
    n.chars().next().map_or(false, |c| c.is_ascii_uppercase()) && n.chars().any(|c| c.is_ascii_lowercase())
}

/// The attribute a component's or view's elements carry when it has a
/// style of its own (`data-q-<id>`), from its file and name (FEEDBACK8.md,
/// Q-067; branch feedback8-fixes).
pub fn style_id(file: Option<&str>, name: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (file.unwrap_or(""), name).hash(&mut h);
    format!("{:08x}", h.finish() as u32)
}

/// `style ...` in a body, as the parser writes it: a call of `__style`.
pub fn style_of(s: &Stmt) -> Option<&Expr> {
    style_parts(s).map(|(e, _)| e)
}

/// `style_of`, and whether it is `style global` (its selectors left as
/// written; FEEDBACK8.md, Q-068; branch style-global).
pub fn style_parts(s: &Stmt) -> Option<(&Expr, bool)> {
    match s {
        Stmt::Expr(Expr::Call(f, args, _)) if matches!(&**f, Expr::Ident(n, _) if n == "__style") => {
            args.first().map(|e| (e, matches!(args.get(1), Some(Expr::Lit(Lit::Bool(true))))))
        }
        _ => None,
    }
}
