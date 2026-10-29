//! Recursive-descent parser. Precedence, lowest first:
//! `? :`, `??`, `|` (pipe), `||`, `&&`, comparisons, `..`, `+ -`, `* / %`,
//! unary `-`, then postfix calls, fields, indexing and `?`.

use crate::ast::*;
use crate::error::{Fix, QuilloError};
use crate::lexer::{Lexer, StrPart, Tok, Token};
use std::sync::Arc;

type PResult<T> = Result<T, QuilloError>;

/// `<Row key={x.id}/>` says which item of a list a row is: it is the
/// list's, so a view or component cannot take it as a parameter.
fn key_not_a_param(def: &FnDef) -> PResult<()> {
    if def.params.iter().any(|p| p.name == "key") {
        return Err(QuilloError::new(
            format!("<{}> cannot take a parameter named `key`: in a list, key={{...}} says which item an element is, and is not passed on; name it otherwise (`code`, `id`)", def.name),
            def.line,
        ));
    }
    Ok(())
}

// `date`: text that is a calendar date, YYYY-MM-DD (FEEDBACK13.md, Q-093; branch open-fixes).
pub const TYPE_NAMES: &[&str] = &["int", "float", "dec", "num", "str", "date", "bool", "list", "map", "html", "fn", "any", "response"];
pub const VOID_TAGS: &[&str] =
    &["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track", "wbr"];
const KEYWORDS: &[&str] = &[
    "let", "fn", "view", "route", "use", "return", "if", "then", "else", "match", "for", "in", "and", "or", "not",
    "true", "false", "null", "nil", "reply", "as", "component", "while",
];
const METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "ANY"];

/// How deeply expressions, patterns, types and elements may nest. Deeper
/// source is refused here, so that nothing that walks it later (the
/// checker, the interpreter, freeing it) runs out of stack.
pub const MAX_NESTING: u32 = 256;
const TOO_DEEP: &str = "the code nests too deeply";

pub struct Parser {
    lx: Lexer,
    peeked: Option<Token>,
    /// While > 0, `name => ...` is not an arrow function: in `for x in xs => body`
    /// the `=>` belongs to the loop.
    no_bare_arrow: u32,
    /// How deeply the code being parsed nests (see `MAX_NESTING`), and the
    /// deepest it went in the function being parsed.
    depth: u32,
    deepest: u32,
    /// What `ternary_ahead` found at each `?`, so that nested ternaries are
    /// looked ahead at once each, not again for every one around them.
    ternaries: std::collections::HashMap<(usize, bool), bool>,
    /// The file being parsed, stamped on its functions and routes.
    file: Option<Arc<str>>,
    /// The component or view whose body is being parsed, for `style`.
    styling: Option<Styling>,
    /// The `fn` about to be parsed is a view (`view`, or a capital name).
    view_next: bool,
    /// The elements whose children are being parsed, innermost last: what a
    /// missing end tag is told against (branch editor-end-tags).
    open: Vec<OpenTag>,
    /// The elements still open where the text ended, if it ended inside
    /// markup: what an editor closes at the cursor (see `open_tags_at_end`).
    unclosed_at_end: Option<Vec<OpenTag>>,
    /// The statement being parsed is one of the file's own, at its top
    /// level: where a `routes` group may be (branch route-groups).
    top: bool,
    /// The routes of a group after its first, which the program takes after
    /// the statement that gave them (branch route-groups).
    more: Vec<Stmt>,
    /// The file's `private` names, renamed once it is parsed (src/privacy.rs;
    /// branch public-private), and what their new names end in.
    privates: std::collections::HashSet<String>,
    privacy_tag: String,
}

/// An element whose end tag has not been read yet.
#[derive(Debug, Clone)]
pub struct OpenTag {
    pub tag: String,
    /// The line of its `<`.
    pub line: u32,
    /// Just after its start tag's `>`, as a character index, and that line.
    pub end: usize,
    pub end_line: u32,
    /// The indentation of the line it starts on.
    pub indent: String,
}

/// The elements open at the end of `src` (an editor's text up to its
/// cursor), outermost first; None when the text does not end in markup.
/// The parser's own reading, so that markup in `{...}` and text such as
/// `a < b` are told apart as they are when the file runs (branch
/// editor-end-tags).
pub fn open_tags_at_end(src: &str) -> Option<Vec<OpenTag>> {
    let mut p = Parser::new(src);
    match p.parse_program() {
        Ok(_) => None,
        Err(_) => p.unclosed_at_end.filter(|o| !o.is_empty()),
    }
}

/// The column of `pos` on its line, in characters.
fn col_at(text: &[char], pos: usize) -> u32 {
    text[..pos].iter().rev().take_while(|c| **c != '\n').count() as u32
}

/// Only spaces come before `pos` on its line.
fn starts_line(text: &[char], pos: usize) -> bool {
    text[..pos].iter().rev().take_while(|c| **c != '\n').all(|c| *c == ' ' || *c == '\t')
}

/// A line that starts a new declaration at its first column (`route GET
/// /x`, `fn f(`, `view Page`...): markup left open runs into it. Written to
/// its shape, as markup may start a line with a plain word.
fn starts_declaration(line: &[char]) -> bool {
    if line.first().map_or(true, |c| c.is_whitespace()) {
        return false;
    }
    let line: String = line.iter().take(80).collect();
    let mut words = line.split_whitespace();
    let (Some(first), Some(second)) = (words.next(), words.next()) else { return false };
    let name = |w: &str| w.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_');
    let upper = |w: &str| w.chars().next().is_some_and(char::is_uppercase);
    match first {
        "route" => second.chars().all(|c| c.is_ascii_uppercase()),
        "view" | "component" | "type" => upper(second),
        "fn" => name(second) && line.contains('('),
        "server" => second == "fn",
        "let" => name(second) && line.contains('='),
        "use" => second.starts_with('"'),
        "service" => name(second),
        _ => false,
    }
}

/// A component's or view's own CSS (FEEDBACK8.md, Q-067; branch
/// feedback8-fixes): after `style ...` in its body, each element it writes
/// carries `data-q-<id>`, on the server and in the browser alike, and its
/// rules are made to need it.
struct Styling {
    owner: String,
    may: bool,
    params: Vec<String>,
    scope: Option<String>,
    /// Elements written so far: a `style` comes before them.
    els: u32,
}

fn can_start_expr(t: &Tok) -> bool {
    match t {
        Tok::Int(_)
        | Tok::Float(_)
        | Tok::Dec(_)
        | Tok::Str(_)
        | Tok::LParen
        | Tok::LBracket
        | Tok::LBrace
        | Tok::Minus
        | Tok::Bang
        | Tok::Lt => true,
        Tok::Ident(k) => !matches!(k.as_str(), "else" | "then" | "and" | "or" | "in" | "as"),
        _ => false,
    }
}

fn lit_string(parts: Vec<StrPart>) -> Option<String> {
    let mut s = String::new();
    for p in parts {
        match p {
            StrPart::Lit(x) => s.push_str(&x),
            StrPart::Expr(..) => return None,
        }
    }
    Some(s)
}

fn is_path_char(c: char) -> bool {
    c.is_alphanumeric() || "-._~%".contains(c)
}

impl Parser {
    pub fn new(src: &str) -> Self {
        Self::with_line(src, 1)
    }

    pub fn with_line(src: &str, line: u32) -> Self {
        Parser { lx: Lexer::new(src, line), peeked: None, no_bare_arrow: 0, depth: 0, deepest: 0, ternaries: Default::default(), file: None, styling: None, view_next: false, open: Vec::new(), unclosed_at_end: None, top: false, more: Vec::new(),
                 privates: Default::default(), privacy_tag: crate::privacy::tag(None, src) }
    }

    /// Records `file` on the functions and routes parsed (see `FnDef::file`).
    pub fn in_file(mut self, file: Option<Arc<str>>) -> Self {
        if let Some(f) = &file {
            self.privacy_tag = crate::privacy::tag(Some(f), "");
        }
        self.file = file;
        self
    }

    // ---- token helpers -------------------------------------------------

    fn peek(&mut self) -> PResult<Tok> {
        if self.peeked.is_none() {
            self.peeked = Some(self.lx.next_token()?);
        }
        Ok(self.peeked.as_ref().unwrap().tok.clone())
    }

    fn line(&mut self) -> PResult<u32> {
        self.peek()?;
        Ok(self.peeked.as_ref().unwrap().line)
    }

    fn next(&mut self) -> PResult<Token> {
        match self.peeked.take() {
            Some(t) => Ok(t),
            None => self.lx.next_token(),
        }
    }

    fn at(&mut self, t: &Tok) -> PResult<bool> {
        Ok(&self.peek()? == t)
    }

    fn eat(&mut self, t: &Tok) -> PResult<bool> {
        if self.at(t)? {
            self.next()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn expect(&mut self, t: &Tok, what: &str) -> PResult<Token> {
        let tok = self.next()?;
        if &tok.tok == t {
            Ok(tok)
        } else {
            Err(QuilloError::new(format!("expected {}, found {}", what, tok.tok.describe()), tok.line))
        }
    }

    fn at_kw(&mut self, kw: &str) -> PResult<bool> {
        Ok(matches!(self.peek()?, Tok::Ident(ref k) if k == kw))
    }

    fn eat_kw(&mut self, kw: &str) -> PResult<bool> {
        if self.at_kw(kw)? {
            self.next()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn expect_ident(&mut self, what: &str) -> PResult<String> {
        let t = self.next()?;
        match t.tok {
            Tok::Ident(s) if !KEYWORDS.contains(&s.as_str()) => Ok(s),
            Tok::Ident(s) => Err(QuilloError::new(format!("expected {}, found `{}`, which is a keyword: pick another name", what, s), t.line)),
            other => Err(QuilloError::new(format!("expected {}, found {}", what, other.describe()), t.line)),
        }
    }

    fn skip_seps(&mut self) -> PResult<()> {
        while matches!(self.peek()?, Tok::Newline | Tok::Semi) {
            self.next()?;
        }
        Ok(())
    }

    fn skip_list_seps(&mut self) -> PResult<()> {
        while matches!(self.peek()?, Tok::Newline | Tok::Semi | Tok::Comma) {
            self.next()?;
        }
        Ok(())
    }

    fn save(&self) -> (Lexer, Option<Token>, u32) {
        (self.lx.clone(), self.peeked.clone(), self.depth)
    }

    fn restore(&mut self, s: (Lexer, Option<Token>, u32)) {
        self.lx = s.0;
        self.peeked = s.1;
        self.depth = s.2;
    }

    /// One level deeper, unless that is too deep. A chain such as
    /// `a + b + c` nests as `(a + b) + c`, so each link counts too.
    fn deeper(&mut self) -> PResult<()> {
        if self.depth >= MAX_NESTING {
            return self.err_at(format!("{} (more than {} levels)", TOO_DEEP, MAX_NESTING));
        }
        self.depth += 1;
        self.deepest = self.deepest.max(self.depth);
        Ok(())
    }

    /// `f`, and how deeply what it parsed nests: a function's body.
    fn measured<T>(&mut self, f: impl FnOnce(&mut Self) -> PResult<T>) -> PResult<(T, u32)> {
        let outer = std::mem::replace(&mut self.deepest, self.depth);
        let r = f(self)?;
        let n = self.deepest - self.depth;
        self.deepest = self.deepest.max(outer);
        Ok((r, n))
    }

    /// `f`, one level deeper (on more stack, when this one runs low).
    fn nested<T>(&mut self, f: impl FnOnce(&mut Self) -> PResult<T>) -> PResult<T> {
        let depth = self.depth;
        self.deeper()?;
        let r = crate::stack::maybe_grow(256 << 10, 2 << 20, || f(self));
        self.depth = depth;
        r
    }

    fn err_at<T>(&mut self, msg: impl Into<String>) -> PResult<T> {
        let line = self.line()?;
        Err(QuilloError::new(msg, line))
    }

    // ---- statements ----------------------------------------------------

    pub fn parse_program(&mut self) -> PResult<Vec<Stmt>> {
        let mut out = Vec::new();
        loop {
            self.skip_seps()?;
            if self.at(&Tok::Eof)? {
                // `private` names: renamed in the whole file (src/privacy.rs).
                if !self.privates.is_empty() {
                    crate::privacy::rename(&mut out, &self.privates, &self.privacy_tag)?;
                }
                return Ok(out);
            }
            self.top = true;
            out.push(self.parse_stmt()?);
            // A `routes` group's other routes, in order (branch route-groups).
            out.append(&mut self.more);
            self.end_stmt()?;
        }
    }

    fn end_stmt(&mut self) -> PResult<()> {
        match self.peek()? {
            Tok::Newline | Tok::Semi => {
                self.next()?;
                Ok(())
            }
            Tok::Eof | Tok::RBrace => Ok(()),
            other => {
                if let Some(h) = old_syntax_hint(&other) {
                    return self.err_at(h);
                }
                self.err_at(format!("expected end of statement, found {}", other.describe()))
            }
        }
    }

    fn parse_stmt(&mut self) -> PResult<Stmt> {
        // Only the outermost statement is at the top level (route-groups).
        let top = std::mem::take(&mut self.top);
        let line = self.line()?;
        if let Tok::Ident(k) = self.peek()? {
            match k.as_str() {
                // `private fn helper()`, `public view Card`: who sees the name. Only
                // before a definition at a file's top level; otherwise a name
                // (branch public-private).
                "private" | "public" => {
                    let private = k == "private";
                    let saved = self.save();
                    self.next()?;
                    let what = match self.peek()? {
                        Tok::Ident(w) if self.line()? == line && ["fn", "view", "component", "let", "server", "type"].contains(&w.as_str()) => Some(w),
                        _ => None,
                    };
                    if let Some(what) = what {
                        if !top {
                            return Err(QuilloError::new(format!("`{}` is written at the top level of a file, before what it names", k), line));
                        }
                        if private && what == "server" {
                            return Err(QuilloError::new("a `server fn` cannot be private: it is an address on the server that anyone can call; keep the work in a private `fn` it calls", line));
                        }
                        if private && what == "type" {
                            return Err(QuilloError::new("`private type` is not there yet: a type's name is the program's", line));
                        }
                        self.top = true;
                        let stmt = self.parse_stmt()?;
                        if private {
                            match &stmt {
                                Stmt::Fn(def) => {
                                    self.privates.insert(def.name.clone());
                                }
                                Stmt::Let(pat, _, _) => {
                                    let mut names = Vec::new();
                                    pattern_binds(pat, &mut names);
                                    self.privates.extend(names);
                                }
                                _ => {}
                            }
                        }
                        return Ok(stmt);
                    }
                    self.restore(saved);
                }
                // `style css`...`` (or a name): the CSS of the component or
                // view this is the body of. Otherwise `style` is a name.
                "style" => {
                    let saved = self.save();
                    self.next()?;
                    if self.line()? == line && matches!(self.peek()?, Tok::Ident(_) | Tok::Str(_)) {
                        return self.style_stmt(line);
                    }
                    self.restore(saved);
                }
                "let" => {
                    self.next()?;
                    let pat = self.parse_binding()?;
                    self.expect(&Tok::Assign, "`=`")?;
                    let e = self.parse_expr()?;
                    return Ok(Stmt::Let(pat, e, line));
                }
                "fn" | "view" => {
                    let view = k == "view";
                    let saved = self.save();
                    self.next()?;
                    if let Tok::Ident(_) = self.peek()? {
                        let name = self.expect_ident("a function name")?;
                        self.view_next = view || name.starts_with(|c: char| c.is_uppercase());
                        let def = self.parse_fn_rest(name, line)?;
                        if view || def.name.starts_with(|c: char| c.is_uppercase()) {
                            key_not_a_param(&def)?;
                        }
                        return Ok(Stmt::Fn(Arc::new(def)));
                    }
                    self.restore(saved);
                }
                "server" => {
                    // `server fn name(...)`; otherwise `server` is an ordinary name.
                    let saved = self.save();
                    self.next()?;
                    if self.eat_kw("fn")? {
                        let name = self.expect_ident("a function name after `server fn`")?;
                        let mut def = self.parse_fn_rest(name, line)?;
                        def.server = true;
                        return Ok(Stmt::Fn(Arc::new(def)));
                    }
                    self.restore(saved);
                }
                "route" => {
                    self.next()?;
                    return Ok(Stmt::Route(Arc::new(self.parse_route(line)?)));
                }
                // `routes /admin | auth.required { route GET /users = ... }`: a
                // group, whose prefix, middleware and request pattern each of
                // its routes takes; otherwise `routes` is a name (branch route-groups).
                "routes" => {
                    let saved = self.save();
                    self.next()?;
                    self.lx.skip_spaces();
                    if self.peeked.is_none() && self.group_ahead() {
                        if !top {
                            return Err(QuilloError::new("a `routes` group is written at the top level of a file, as `route` is", line));
                        }
                        let mut routes = self.parse_group(line, &[], &[], &[])?.into_iter();
                        let Some(first) = routes.next() else {
                            return Err(QuilloError::new("this `routes` group has no routes in it", line));
                        };
                        self.more.extend(routes.map(|r| Stmt::Route(Arc::new(r))));
                        return Ok(Stmt::Route(Arc::new(first)));
                    }
                    self.restore(saved);
                }
                "component" => {
                    self.next()?;
                    return Ok(Stmt::Fn(Arc::new(self.parse_component(line)?)));
                }
                "type" => {
                    // `type Name = ...`; otherwise `type` is a name.
                    let saved = self.save();
                    self.next()?;
                    if let Tok::Ident(n) = self.peek()? {
                        let capital = n.chars().next().map_or(false, |c| c.is_ascii_uppercase());
                        self.next()?;
                        if capital && self.at(&Tok::Assign)? {
                            if !crate::ast::is_type_name(&n) {
                                return Err(QuilloError::new(format!("a type is named like `User` or `OrderLine`, with small letters: `{}` reads as a constant", n), line));
                            }
                            self.next()?;
                            return Ok(Stmt::Type(Arc::new(self.parse_typedef(n, line)?)));
                        }
                    }
                    self.restore(saved);
                }
                "service" => {
                    // `service name = "file.qwe"` or `service /prefix = "file.qwe"`;
                    // otherwise `service` is a name.
                    let saved = self.save();
                    self.next()?;
                    if self.peeked.is_none() {
                        self.lx.skip_spaces();
                        let prefix = if self.lx.peek_char() == Some('/') {
                            let mut p = String::new();
                            while let Some(c) = self.lx.peek_char() {
                                if c == '/' || is_path_char(c) {
                                    p.push(c);
                                    self.lx.bump();
                                } else {
                                    break;
                                }
                            }
                            let p = p.trim_end_matches('/').to_string();
                            if p.is_empty() || p.contains("//") {
                                return Err(QuilloError::new("`service` needs a name or a path: service billing = \"billing.qwe\"", line));
                            }
                            self.lx.last = Some(Tok::RParen);
                            Some(p)
                        } else {
                            None
                        };
                        let name = match &prefix {
                            Some(_) => Some(String::new()),
                            None => match self.peek()? {
                                Tok::Ident(n) if !KEYWORDS.contains(&n.as_str()) => {
                                    self.next()?;
                                    Some(n)
                                }
                                _ => None,
                            },
                        };
                        if let (Some(name), true) = (name, prefix.is_some() || self.at(&Tok::Assign)?) {
                            self.expect(&Tok::Assign, "`=` after the service")?;
                            let t = self.next()?;
                            if let Tok::Str(parts) = t.tok {
                                if let Some(file) = lit_string(parts) {
                                    let name = match &prefix {
                                        Some(_) => std::path::Path::new(&file).file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string(),
                                        None => name,
                                    };
                                    if !name.chars().next().map_or(false, |c| c.is_ascii_alphabetic() || c == '_')
                                        || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                                    {
                                        return Err(QuilloError::new(format!("a service is named like a variable: `{}` is not one (name the file so)", name), line));
                                    }
                                    // How it is called, on the same line: `{ timeout: "2s" }`.
                                    self.lx.skip_inline_ws();
                                    let options = if self.peeked.is_none() && self.lx.peek_char() == Some('{') {
                                        match self.parse_expr()? {
                                            e @ Expr::Map(_) => Some(e),
                                            _ => return Err(QuilloError::new("after a service's file, a map of how it is called: { timeout: \"2s\", retries: 2 }", line)),
                                        }
                                    } else {
                                        None
                                    };
                                    return Ok(Stmt::Service(name, prefix, file, options, line));
                                }
                            }
                            return Err(QuilloError::new("`service` expects a file: service billing = \"billing.qwe\"", line));
                        }
                    }
                    self.restore(saved);
                }
                "mount" => {
                    // `mount /prefix = "file.qwe"`; otherwise `mount` is a name.
                    let saved = self.save();
                    self.next()?;
                    if self.peeked.is_none() {
                        self.lx.skip_spaces();
                        if self.lx.peek_char() == Some('/') {
                            let mut prefix = String::new();
                            while let Some(c) = self.lx.peek_char() {
                                if c == '/' || is_path_char(c) {
                                    prefix.push(c);
                                    self.lx.bump();
                                } else {
                                    break;
                                }
                            }
                            let prefix = prefix.trim_end_matches('/').to_string();
                            if prefix.is_empty() || prefix.contains("//") {
                                return Err(QuilloError::new("`mount` needs a path such as /notes", line));
                            }
                            self.lx.last = Some(Tok::RParen);
                            self.expect(&Tok::Assign, "`=` after the mount path")?;
                            let t = self.next()?;
                            if let Tok::Str(parts) = t.tok {
                                if let Some(file) = lit_string(parts) {
                                    return Ok(Stmt::Mount(prefix, file, line));
                                }
                            }
                            return Err(QuilloError::new("`mount` expects a file: mount /notes = \"notes.qwe\"", line));
                        }
                    }
                    self.restore(saved);
                }
                "use" => {
                    self.next()?;
                    let t = self.next()?;
                    if let Tok::Str(parts) = t.tok {
                        if let Some(path) = lit_string(parts) {
                            return Ok(Stmt::Use(path, line));
                        }
                    }
                    return Err(QuilloError::new("`use` expects a file path string, e.g. use \"lib.qwe\"", line));
                }
                "return" => {
                    self.next()?;
                    let e = if can_start_expr(&self.peek()?) { Some(self.parse_expr()?) } else { None };
                    return Ok(Stmt::Return(e));
                }
                _ => {}
            }
        }
        let e = self.parse_expr()?;
        let op = match self.peek()? {
            Tok::Assign => Some(AssignOp::Set),
            Tok::PlusAssign => Some(AssignOp::Add),
            Tok::MinusAssign => Some(AssignOp::Sub),
            _ => None,
        };
        if let Some(op) = op {
            self.next()?;
            let name = match e {
                Expr::Ident(n, _) => n,
                Expr::Field(obj, name, _) => {
                    let rhs = self.parse_expr()?;
                    return Ok(Stmt::SetField(*obj, name, op, rhs, line));
                }
                _ => return Err(QuilloError::new("only variables, and the fields of the page's objects (`el.value = x`), can be assigned to", line)),
            };
            let rhs = self.parse_expr()?;
            return Ok(Stmt::Assign(name, op, rhs, line));
        }
        Ok(Stmt::Expr(e))
    }

    fn parse_fn_rest(&mut self, name: String, line: u32) -> PResult<FnDef> {
        self.expect(&Tok::LParen, "`(`")?;
        let mut params = Vec::new();
        while !self.at(&Tok::RParen)? {
            let pname = self.expect_ident("a parameter name")?;
            let ty = if self.eat(&Tok::Colon)? { Some(self.parse_type()?) } else { None };
            let default = if self.eat(&Tok::Assign)? { Some(self.parse_expr()?) } else { None };
            params.push(Param { name: pname, ty, default });
            if !self.eat(&Tok::Comma)? {
                break;
            }
        }
        self.expect(&Tok::RParen, "`)`")?;
        let ret = if self.eat(&Tok::Arrow)? { Some(self.parse_type()?) } else { None };
        let err = if self.eat(&Tok::Bang)? { Some(self.parse_type()?) } else { None };
        let effects = if self.eat_kw("with")? {
            let mut effs = Vec::new();
            loop {
                let t = self.next()?;
                match t.tok {
                    Tok::Ident(e) if EFFECTS.contains(&e.as_str()) => effs.push(e),
                    other => {
                        return Err(QuilloError::new(
                            format!("expected an effect after `with` ({}), found {}", EFFECTS.join(", "), other.describe()),
                            t.line,
                        ))
                    }
                }
                if !self.eat(&Tok::Comma)? {
                    break;
                }
            }
            Some(effs)
        } else {
            None
        };
        let may = std::mem::take(&mut self.view_next);
        // A helper written inside a styled component or view is part of it.
        let scope = if may { None } else { self.styling.as_ref().and_then(|s| s.scope.clone()) };
        let styling = Styling { owner: name.clone(), may, params: params.iter().map(|p| p.name.clone()).collect(), scope, els: 0 };
        let outer = self.styling.replace(styling);
        let body = if self.eat(&Tok::Assign)? {
            self.measured(Self::parse_arrow_body)
        } else if self.at(&Tok::LBrace)? {
            self.measured(Self::parse_block)
        } else {
            self.styling = outer;
            return self.err_at(format!("expected `=` or `{{` to start the body of `{}`", name));
        };
        self.styling = outer;
        let (body, nesting) = body?;
        Ok(FnDef { name, params, ret, err, effects, server: false, component: false, body, line, nesting, file: self.file.clone(), hot: Default::default() })
    }

    /// `style <css>` in a component's or view's body: the call of `__style`,
    /// which does nothing as the code runs; the CSS is taken when the
    /// program loads (`Interp::styles`).
    fn style_stmt(&mut self, line: u32) -> PResult<Stmt> {
        let (owner, params) = match &self.styling {
            Some(s) if s.may => {
                if s.els > 0 {
                    return Err(QuilloError::new(format!("`style` comes before the markup of `{}`", s.owner), line));
                }
                (s.owner.clone(), s.params.clone())
            }
            _ => return Err(QuilloError::new("`style` belongs in the body of a component or a view", line)),
        };
        // `style global ...`: sent with the component, its selectors as written
        // (a library whose classes are its own already, such as the UI kit's
        // `ui-`). `global` alone is a name like any other (FEEDBACK8.md, Q-068;
        // branch style-global).
        let mut global = false;
        if matches!(self.peek()?, Tok::Ident(w) if w == "global") {
            let saved = self.save();
            self.next()?;
            if self.line()? == line && matches!(self.peek()?, Tok::Ident(_) | Tok::Str(_)) {
                global = true;
            } else {
                self.restore(saved);
            }
        }
        let e = self.parse_expr()?;
        let mut read = None;
        crate::check::walk_expr(&e, &mut |x| {
            if let Expr::Ident(n, _) = x {
                if read.is_none() && params.contains(n) {
                    read = Some(n.clone());
                }
            }
        });
        if let Some(n) = read {
            return Err(QuilloError::new(
                format!("a style is the same for every `{}`, so it cannot read its parameter `{}`; set a class or `style=` on an element instead", owner, n),
                line,
            ));
        }
        let id = style_id(self.file.as_deref(), &owner);
        if let Some(s) = &mut self.styling {
            s.scope = Some(id);
        }
        let mut args = vec![e];
        if global {
            args.push(Expr::Lit(Lit::Bool(true)));
        }
        Ok(Stmt::Expr(Expr::Call(Box::new(Expr::Ident("__style".into(), line)), args, line)))
    }

    /// `int`, `str?`, or a union: `int | str | null`.
    /// `component Name(params) { state x = init ... <html/> }`. `state` statements
    /// may only appear directly in the body.
    fn parse_component(&mut self, line: u32) -> PResult<FnDef> {
        let name = self.expect_ident("a component name")?;
        if !name.chars().next().map_or(false, |c| c.is_uppercase()) {
            return Err(QuilloError::new(format!("component names start with a capital letter: `{}`", name), line));
        }
        self.expect(&Tok::LParen, "`(`")?;
        let mut params = Vec::new();
        while !self.at(&Tok::RParen)? {
            let pname = self.expect_ident("a parameter name")?;
            let ty = if self.eat(&Tok::Colon)? { Some(self.parse_type()?) } else { None };
            let default = if self.eat(&Tok::Assign)? { Some(self.parse_expr()?) } else { None };
            params.push(Param { name: pname, ty, default });
            if !self.eat(&Tok::Comma)? {
                break;
            }
        }
        self.expect(&Tok::RParen, "`)`")?;
        let styling = Styling { owner: name.clone(), may: true, params: params.iter().map(|p| p.name.clone()).collect(), scope: None, els: 0 };
        let outer = self.styling.replace(styling);
        let body = self.measured(|p| p.component_body(&name));
        self.styling = outer;
        let (body, nesting) = body?;
        let def = FnDef { name, params, ret: None, err: None, effects: None, server: false, component: true, body, line, nesting, file: self.file.clone(), hot: Default::default() };
        key_not_a_param(&def)?;
        Ok(def)
    }

    fn component_body(&mut self, name: &str) -> PResult<Expr> {
        Ok(if self.eat(&Tok::Assign)? {
            self.parse_expr()?
        } else if self.eat(&Tok::LBrace)? {
            let mut stmts = Vec::new();
            loop {
                self.skip_seps()?;
                if self.eat(&Tok::RBrace)? {
                    break;
                }
                if self.at(&Tok::Eof)? {
                    return self.err_at("unexpected end of input: missing `}`");
                }
                let l = self.line()?;
                if let Some(n) = self.state_decl()? {
                    self.expect(&Tok::Assign, "`=` after the state name")?;
                    let e = self.parse_expr()?;
                    stmts.push(Stmt::State(n, e, l));
                } else {
                    stmts.push(self.parse_stmt()?);
                }
                self.end_stmt()?;
            }
            Expr::Block(stmts)
        } else {
            return self.err_at(format!("expected `=` or `{{` to start the body of component `{}`", name));
        })
    }

    /// `state count = ...` (or `~count = ...`) in a component: the state's
    /// name, with what comes after it next. Otherwise nothing is taken, and
    /// `state` is a name like any other (`let state = ...`, `state.label`).
    fn state_decl(&mut self) -> PResult<Option<String>> {
        if self.eat(&Tok::Tilde)? {
            return self.expect_ident("a state name after `~`").map(Some);
        }
        if !matches!(self.peek()?, Tok::Ident(w) if w == "state") {
            return Ok(None);
        }
        let saved = self.save();
        self.next()?;
        if let Tok::Ident(n) = self.peek()? {
            let line = self.line()?;
            self.next()?;
            if self.at(&Tok::Assign)? {
                // `state reply = ...`: a state meant, with a keyword for its name.
                if KEYWORDS.contains(&n.as_str()) {
                    return Err(QuilloError::new(
                        format!("expected a state name, found `{}`, which is a keyword: pick another name", n),
                        line,
                    ));
                }
                return Ok(Some(n));
            }
        }
        self.restore(saved);
        Ok(None)
    }

    fn parse_type(&mut self) -> PResult<Ty> {
        self.nested(Self::parse_type_union)
    }

    fn parse_type_union(&mut self) -> PResult<Ty> {
        let mut names: Vec<String> = Vec::new();
        let mut optional = false;
        let mut elem = None;
        let mut fields = None;
        loop {
            let t = self.next()?;
            let name = match t.tok {
                Tok::Ident(s) => s,
                // `{ title: str, tags: list<str> }`: a map and its fields.
                Tok::LBrace => {
                    fields = Some(self.parse_shape(None)?);
                    "map".to_string()
                }
                // `"admin" | "member"`: one of these strings.
                Tok::Str(parts) => match lit_string(parts) {
                    Some(s) => serde_json::to_string(&s).unwrap_or_default(),
                    None => return Err(QuilloError::new("a string in a type cannot contain `${…}`", t.line)),
                },
                other => return Err(QuilloError::new(format!("expected a type, found {}", other.describe()), t.line)),
            };
            // `list<int>`: what the list holds.
            if name == "list" && self.eat(&Tok::Lt)? {
                elem = Some(Box::new(self.parse_type()?));
                self.expect(&Tok::Gt, "`>` after the list's type")?;
            }
            if name == "nil" {
                return Err(QuilloError::new("use `null` instead of `nil`", t.line));
            }
            if name == "null" {
                optional = true;
            } else if TYPE_NAMES.contains(&name.as_str()) || name.starts_with('"') || crate::ast::is_type_name(&name) {
                if !names.contains(&name) {
                    names.push(name);
                }
            } else {
                return Err(QuilloError::new(
                    format!("unknown type `{}` (known types: {}, null, and the program's own, which start with a capital letter)", name, TYPE_NAMES.join(", ")),
                    t.line,
                ));
            }
            if self.eat(&Tok::Question)? {
                optional = true;
            }
            if !self.eat(&Tok::Bar)? {
                break;
            }
        }
        if names.is_empty() {
            names.push("any".into());
        }
        Ok(Ty { names, optional, elem, fields })
    }

    /// `type Name = ...` after the `=`: a record's fields, each perhaps with
    /// a default (`role: str = "member"`), or another type.
    fn parse_typedef(&mut self, name: String, line: u32) -> PResult<TypeDef> {
        self.skip_seps()?;
        if self.eat(&Tok::LBrace)? {
            let mut defaults = Vec::new();
            let fields = self.parse_shape(Some(&mut defaults))?;
            return Ok(TypeDef { name, ty: Ty { names: vec!["map".into()], optional: false, elem: None, fields: Some(fields) }, defaults, line });
        }
        let ty = self.parse_type()?;
        Ok(TypeDef { name, ty, defaults: Vec::new(), line })
    }

    /// The fields of a map type, after its `{`; with `defaults`, a record's,
    /// which may have them.
    fn parse_shape(&mut self, mut defaults: Option<&mut Vec<(String, Expr)>>) -> PResult<Vec<(String, Ty)>> {
        let mut fields: Vec<(String, Ty)> = Vec::new();
        loop {
            self.skip_list_seps()?;
            if self.eat(&Tok::RBrace)? {
                return Ok(fields);
            }
            let t = self.next()?;
            let key = match t.tok {
                Tok::Ident(s) => s,
                Tok::Str(parts) => match lit_string(parts) {
                    Some(s) => s,
                    None => return Err(QuilloError::new("map keys must be plain strings", t.line)),
                },
                other => return Err(QuilloError::new(format!("expected a field name, found {}", other.describe()), t.line)),
            };
            self.expect(&Tok::Colon, &format!("`:` and a type after `{}`", key))?;
            let ty = self.parse_type()?;
            if let Some(ds) = defaults.as_deref_mut() {
                if self.eat(&Tok::Assign)? {
                    let e = self.parse_expr()?;
                    ds.retain(|(k, _)| *k != key);
                    ds.push((key.clone(), e));
                }
            }
            fields.retain(|(k, _)| *k != key);
            fields.push((key, ty));
            match self.peek()? {
                Tok::Comma | Tok::Newline | Tok::Semi | Tok::RBrace => {}
                other => return self.err_at(format!("expected `,` or `}}` in a map type, found {}", other.describe())),
            }
        }
    }

    fn read_word(&mut self) -> String {
        let mut s = String::new();
        while let Some(c) = self.lx.peek_char() {
            if c.is_alphanumeric() || c == '_' {
                s.push(c);
                self.lx.bump();
            } else {
                break;
            }
        }
        s
    }

    fn parse_route(&mut self, line: u32) -> PResult<RouteDef> {
        let method = self.expect_ident("an HTTP method")?;
        if !METHODS.contains(&method.as_str()) {
            return Err(QuilloError::new(
                format!("unknown HTTP method `{}` (use one of {})", method, METHODS.join(", ")),
                line,
            ));
        }
        let path = self.parse_path("after the method")?;
        self.parse_route_rest(method, path, line)
    }

    /// A route's or a group's path, read character by character:
    /// `/users/:id(int)/*rest`.
    fn parse_path(&mut self, after: &str) -> PResult<Vec<Seg>> {
        self.lx.skip_spaces();
        if self.lx.peek_char() != Some('/') {
            return Err(QuilloError::new(format!("expected a path starting with `/` {}", after), self.lx.line));
        }
        let mut path = Vec::new();
        while self.lx.peek_char() == Some('/') {
            self.lx.bump();
            match self.lx.peek_char() {
                Some(':') => {
                    self.lx.bump();
                    let name = self.read_word();
                    if name.is_empty() {
                        return Err(QuilloError::new("expected a parameter name after `:` in the path", self.lx.line));
                    }
                    let ty = if self.lx.peek_char() == Some('(') {
                        self.lx.bump();
                        let t = self.read_word();
                        if self.lx.bump() != Some(')') || !matches!(t.as_str(), "int" | "float" | "dec" | "num" | "str" | "date") {
                            return Err(QuilloError::new(
                                "path parameter types are written :name(int), :name(float), :name(dec), :name(num), :name(str) or :name(date)",
                                self.lx.line,
                            ));
                        }
                        Some(t)
                    } else {
                        None
                    };
                    path.push(Seg::Param(name, ty));
                }
                Some('*') => {
                    self.lx.bump();
                    let name = self.read_word();
                    path.push(Seg::Rest(if name.is_empty() { "rest".into() } else { name }));
                }
                Some(c) if is_path_char(c) => {
                    let mut s = String::new();
                    while let Some(c) = self.lx.peek_char() {
                        if !is_path_char(c) {
                            break;
                        }
                        s.push(c);
                        self.lx.bump();
                    }
                    path.push(Seg::Lit(s));
                }
                _ => {}
            }
        }
        self.lx.last = Some(Tok::RParen);
        Ok(path)
    }

    fn parse_route_rest(&mut self, method: String, path: Vec<Seg>, line: u32) -> PResult<RouteDef> {
        let query = if self.eat(&Tok::Question)? {
            if !self.at(&Tok::LBrace)? {
                return self.err_at("expected `{` with query parameters after `?`");
            }
            Some(self.parse_pattern()?)
        } else {
            None
        };

        // `{ json: {...} } { body }` or `{ ... } = expr`: a pattern, then the body.
        let mut body_pat = None;
        if self.at(&Tok::LBrace)? {
            let saved = self.save();
            let res = self.parse_pattern();
            let is_pat = match &res {
                Ok(_) => matches!(self.peek(), Ok(Tok::LBrace) | Ok(Tok::Assign)),
                Err(_) => false,
            };
            if is_pat {
                body_pat = res.ok();
            } else {
                self.restore(saved);
            }
        }

        let body = if self.eat(&Tok::Assign)? {
            self.parse_expr()?
        } else if self.at(&Tok::LBrace)? {
            self.parse_block()?
        } else {
            return self.err_at("expected `=` or `{` to start the route body");
        };
        let uses_request = body_pat.is_some()
            || names_request(&body)
            || query.as_ref().map_or(false, pattern_names_request);
        Ok(RouteDef { method, path, query, body_pat, guards: Vec::new(), body, line, file: self.file.clone(), uses_request })
    }

    /// Whether what follows `routes` is a group's path: `/` and a path
    /// (`/admin`, `/:team`), or `/` alone before `|` or `{` (a group at the
    /// root). `routes / 2` is a division (branch route-groups).
    fn group_ahead(&self) -> bool {
        if self.lx.peek_char() != Some('/') {
            return false;
        }
        match self.lx.peek_char_at(1) {
            Some(c) if c.is_alphanumeric() || matches!(c, ':' | '_' | '-' | '.' | '*') => true,
            _ => {
                let mut n = 1;
                while matches!(self.lx.peek_char_at(n), Some(' ' | '\t')) {
                    n += 1;
                }
                matches!(self.lx.peek_char_at(n), Some('|' | '{'))
            }
        }
    }

    /// `routes /prefix | middleware ... { request pattern } { routes }`, the
    /// prefix and the rest added to those of the groups around it: its routes,
    /// each with the prefix before its path, the middleware called before its
    /// body (`__answer(m(request))`: a `reply` in it, or a response it gives,
    /// answers instead), and the pattern matched with its own (branch
    /// route-groups).
    fn parse_group(&mut self, line: u32, prefix: &[Seg], before: &[(Expr, u32)], guards: &[Pattern]) -> PResult<Vec<RouteDef>> {
        let mut path = prefix.to_vec();
        path.extend(self.parse_path("after `routes`")?);
        let mut before = before.to_vec();
        while self.eat(&Tok::Bar)? {
            // Its call is told of at the line it is written on.
            let at = self.line()?;
            before.push((self.parse_or()?, at));
        }
        let mut guards = guards.to_vec();
        if self.at(&Tok::LBrace)? {
            let saved = self.save();
            match self.parse_pattern() {
                Ok(p) if self.at(&Tok::LBrace)? => guards.push(p),
                _ => self.restore(saved),
            }
        }
        if !self.eat(&Tok::LBrace)? {
            return self.err_at("expected `{` and the group's routes");
        }
        let mut out = Vec::new();
        loop {
            self.skip_seps()?;
            if self.eat(&Tok::RBrace)? {
                break;
            }
            let at = self.line()?;
            match self.peek()? {
                Tok::Ident(k) if k == "route" => {
                    self.next()?;
                    let mut r = self.parse_route(at)?;
                    let mut full = path.clone();
                    full.append(&mut r.path);
                    r.path = full;
                    let mut gs = guards.clone();
                    gs.append(&mut r.guards);
                    r.guards = gs;
                    if !before.is_empty() {
                        let mut stmts: Vec<Stmt> = before
                            .iter()
                            .map(|(m, ml)| {
                                let called = Expr::Call(Box::new(m.clone()), vec![Expr::Ident("request".into(), *ml)], *ml);
                                Stmt::Expr(Expr::Call(Box::new(Expr::Ident("__answer".into(), *ml)), vec![called], *ml))
                            })
                            .collect();
                        stmts.push(Stmt::Expr(std::mem::replace(&mut r.body, Expr::Lit(Lit::Nil))));
                        r.body = Expr::Block(stmts);
                    }
                    r.uses_request = r.uses_request || !before.is_empty() || !r.guards.is_empty();
                    out.push(r);
                }
                Tok::Ident(k) if k == "routes" => {
                    self.next()?;
                    out.extend(self.parse_group(at, &path, &before, &guards)?);
                }
                other => return self.err_at(format!("a `routes` group holds `route` and `routes`, found {}", other.describe())),
            }
            self.end_stmt()?;
        }
        let _ = line;
        Ok(out)
    }

    /// Runs `f` with `name => ...` arrows allowed again (inside brackets).
    fn no_bare_arrow_off<T>(&mut self, f: impl FnOnce(&mut Self) -> PResult<T>) -> PResult<T> {
        let saved = std::mem::replace(&mut self.no_bare_arrow, 0);
        let r = f(self);
        self.no_bare_arrow = saved;
        r
    }

    /// After `(`: `(a, b: int = 1): T => body` if that is what follows,
    /// otherwise nothing is consumed.
    fn try_arrow(&mut self, line: u32) -> PResult<Option<Expr>> {
        let saved = self.save();
        let head = (|| -> PResult<Option<(Vec<Param>, Option<Ty>)>> {
            let mut params = Vec::new();
            while !self.at(&Tok::RParen)? {
                let Tok::Ident(name) = self.peek()? else { return Ok(None) };
                if KEYWORDS.contains(&name.as_str()) {
                    return Ok(None);
                }
                self.next()?;
                let ty = if self.eat(&Tok::Colon)? { Some(self.parse_type()?) } else { None };
                let default = if self.eat(&Tok::Assign)? { Some(self.parse_expr()?) } else { None };
                params.push(Param { name, ty, default });
                if !self.eat(&Tok::Comma)? {
                    break;
                }
            }
            if !self.eat(&Tok::RParen)? {
                return Ok(None);
            }
            let ret = if self.eat(&Tok::Colon)? { Some(self.parse_type()?) } else { None };
            if !self.eat(&Tok::FatArrow)? {
                return Ok(None);
            }
            Ok(Some((params, ret)))
        })();
        match head {
            Ok(Some((params, ret))) => {
                let body = self.no_bare_arrow_off(|p| p.measured(Self::parse_arrow_body))?;
                Ok(Some(arrow(params, ret, body, line, self.file.clone())))
            }
            Err(e) if e.msg.starts_with(TOO_DEEP) => Err(e),
            _ => {
                self.restore(saved);
                Ok(None)
            }
        }
    }

    /// `=> expr` or `=> { statements }`, also used for match arms and
    /// `fn f() = ...`. Here `{ x }` is a block giving `x`, as in
    /// JavaScript, not the map `{ x: x }`; `{ x, y }` and `{ x: 1 }` are maps.
    fn parse_arrow_body(&mut self) -> PResult<Expr> {
        if self.at(&Tok::LBrace)? {
            let s = self.save();
            self.next()?;
            let lone_name = matches!(self.peek()?, Tok::Ident(_)) && {
                self.next()?;
                matches!(self.peek()?, Tok::RBrace)
            };
            self.restore(s);
            if lone_name {
                return self.no_bare_arrow_off(|p| p.parse_block());
            }
        }
        self.parse_expr()
    }

    fn parse_block(&mut self) -> PResult<Expr> {
        self.expect(&Tok::LBrace, "`{`")?;
        self.parse_block_rest()
    }

    fn parse_block_rest(&mut self) -> PResult<Expr> {
        let mut stmts = Vec::new();
        loop {
            self.skip_seps()?;
            if self.eat(&Tok::RBrace)? {
                return Ok(Expr::Block(stmts));
            }
            if self.at(&Tok::Eof)? {
                return self.err_at("unexpected end of input: missing `}`");
            }
            stmts.push(self.parse_stmt()?);
            self.end_stmt()?;
        }
    }

    // ---- expressions ---------------------------------------------------

    pub fn parse_expr(&mut self) -> PResult<Expr> {
        self.nested(Self::parse_ternary)
    }

    fn parse_ternary(&mut self) -> PResult<Expr> {
        let cond = self.parse_coalesce()?;
        // `cond ? a : b`
        if self.at(&Tok::Question)? {
            self.next()?;
            let a = self.parse_expr()?;
            self.expect(&Tok::Colon, "`:` in `cond ? a : b`")?;
            let b = self.parse_expr()?;
            return Ok(Expr::If(Box::new(cond), Box::new(a), Some(Box::new(b))));
        }
        Ok(cond)
    }

    fn parse_coalesce(&mut self) -> PResult<Expr> {
        let depth = self.depth;
        let mut lhs = self.parse_pipe()?;
        while self.eat(&Tok::QQ)? {
            self.deeper()?;
            let rhs = self.parse_pipe()?;
            lhs = Expr::Coalesce(Box::new(lhs), Box::new(rhs));
        }
        self.depth = depth;
        Ok(lhs)
    }

    fn parse_pipe(&mut self) -> PResult<Expr> {
        let depth = self.depth;
        let mut lhs = self.parse_or()?;
        loop {
            let line = self.line()?;
            if self.at(&Tok::PipeGt)? {
                return self.err_at("the pipe is now a single `|`: `xs | map(f)`");
            }
            if !self.eat(&Tok::Bar)? {
                self.depth = depth;
                return Ok(lhs);
            }
            self.deeper()?;
            let rhs = self.parse_or()?;
            lhs = Expr::Pipe(Box::new(lhs), Box::new(rhs), line);
        }
    }

    fn parse_or(&mut self) -> PResult<Expr> {
        let depth = self.depth;
        let mut lhs = self.parse_and()?;
        loop {
            let line = self.line()?;
            if !self.eat(&Tok::OrOr)? {
                self.depth = depth;
                return Ok(lhs);
            }
            self.deeper()?;
            let rhs = self.parse_and()?;
            lhs = Expr::Binary(BinOp::Or, Box::new(lhs), Box::new(rhs), line);
        }
    }

    fn parse_and(&mut self) -> PResult<Expr> {
        let depth = self.depth;
        let mut lhs = self.parse_cmp()?;
        loop {
            let line = self.line()?;
            if !self.eat(&Tok::AndAnd)? {
                self.depth = depth;
                return Ok(lhs);
            }
            self.deeper()?;
            let rhs = self.parse_cmp()?;
            lhs = Expr::Binary(BinOp::And, Box::new(lhs), Box::new(rhs), line);
        }
    }

    fn parse_cmp(&mut self) -> PResult<Expr> {
        let lhs = self.parse_range()?;
        let line = self.line()?;
        let op = match self.peek()? {
            Tok::EqEq => BinOp::Eq,
            Tok::NotEq => BinOp::Ne,
            Tok::Lt => BinOp::Lt,
            Tok::Le => BinOp::Le,
            Tok::Gt => BinOp::Gt,
            Tok::Ge => BinOp::Ge,
            _ => return Ok(lhs),
        };
        self.next()?;
        let rhs = self.parse_range()?;
        Ok(Expr::Binary(op, Box::new(lhs), Box::new(rhs), line))
    }

    fn parse_range(&mut self) -> PResult<Expr> {
        let lhs = self.parse_add()?;
        let line = self.line()?;
        if self.eat(&Tok::DotDot)? {
            let rhs = self.parse_add()?;
            return Ok(Expr::Binary(BinOp::Range, Box::new(lhs), Box::new(rhs), line));
        }
        Ok(lhs)
    }

    fn parse_add(&mut self) -> PResult<Expr> {
        let depth = self.depth;
        let mut lhs = self.parse_mul()?;
        loop {
            let line = self.line()?;
            let op = match self.peek()? {
                Tok::Plus => BinOp::Add,
                Tok::Minus => BinOp::Sub,
                _ => {
                    self.depth = depth;
                    return Ok(lhs);
                }
            };
            self.next()?;
            self.deeper()?;
            let rhs = self.parse_mul()?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs), line);
        }
    }

    fn parse_mul(&mut self) -> PResult<Expr> {
        let depth = self.depth;
        let mut lhs = self.parse_unary()?;
        loop {
            let line = self.line()?;
            let op = match self.peek()? {
                Tok::Star => BinOp::Mul,
                Tok::Slash => BinOp::Div,
                Tok::Percent => BinOp::Mod,
                _ => {
                    self.depth = depth;
                    return Ok(lhs);
                }
            };
            self.next()?;
            self.deeper()?;
            let rhs = self.parse_unary()?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs), line);
        }
    }

    fn parse_unary(&mut self) -> PResult<Expr> {
        let line = self.line()?;
        if self.eat(&Tok::Minus)? {
            let e = self.nested(Self::parse_unary)?;
            return Ok(Expr::Unary(UnOp::Neg, Box::new(e), line));
        }
        if self.eat(&Tok::Bang)? {
            let e = self.nested(Self::parse_unary)?;
            return Ok(Expr::Unary(UnOp::Not, Box::new(e), line));
        }
        self.parse_postfix()
    }

    /// After `x`, is `? a : b` coming (a ternary), or is `?` the postfix
    /// that returns an error? A ternary needs an expression and then `:`.
    fn ternary_ahead(&mut self) -> PResult<bool> {
        let at = (self.lx.pos, self.no_bare_arrow > 0);
        if let Some(&known) = self.ternaries.get(&at) {
            return Ok(known);
        }
        let saved = self.save();
        let ok = (|| -> PResult<bool> {
            self.expect(&Tok::Question, "`?`")?;
            if !can_start_expr(&self.peek()?) {
                return Ok(false);
            }
            self.parse_expr()?;
            Ok(self.at(&Tok::Colon)?)
        })();
        self.restore(saved);
        let ok = match ok {
            // Too deep either way.
            Err(e) if e.msg.starts_with(TOO_DEEP) => return Err(e),
            r => r.unwrap_or(false),
        };
        self.ternaries.insert(at, ok);
        Ok(ok)
    }

    fn parse_postfix(&mut self) -> PResult<Expr> {
        let depth = self.depth;
        let mut e = self.parse_primary()?;
        loop {
            let line = self.line()?;
            let t = self.peek()?;
            if matches!(t, Tok::LParen | Tok::Dot | Tok::LBracket | Tok::Question) {
                self.deeper()?;
            }
            match t {
                Tok::LParen => {
                    self.next()?;
                    let args = self.no_bare_arrow_off(|p| {
                        let mut args = Vec::new();
                        while !p.at(&Tok::RParen)? {
                            args.push(p.parse_expr()?);
                            if !p.eat(&Tok::Comma)? {
                                break;
                            }
                        }
                        p.expect(&Tok::RParen, "`)` after arguments")?;
                        Ok(args)
                    })?;
                    e = Expr::Call(Box::new(e), args, line);
                }
                Tok::Dot => {
                    self.next()?;
                    let t = self.next()?;
                    let name = match t.tok {
                        Tok::Ident(s) => s,
                        Tok::Int(n) => n.to_string(),
                        other => {
                            return Err(QuilloError::new(
                                format!("expected a field name after `.`, found {}", other.describe()),
                                t.line,
                            ))
                        }
                    };
                    e = Expr::Field(Box::new(e), name, line);
                }
                Tok::LBracket => {
                    self.next()?;
                    let i = self.no_bare_arrow_off(|p| p.parse_expr())?;
                    self.expect(&Tok::RBracket, "`]`")?;
                    e = Expr::Index(Box::new(e), Box::new(i), line);
                }
                Tok::Question => {
                    if self.ternary_ahead()? {
                        self.depth = depth;
                        return Ok(e);
                    }
                    self.next()?;
                    e = Expr::Try(Box::new(e));
                }
                _ => {
                    self.depth = depth;
                    return Ok(e);
                }
            }
        }
    }

    fn parse_primary(&mut self) -> PResult<Expr> {
        let t = self.next()?;
        let line = t.line;
        match t.tok {
            Tok::Int(n) => Ok(Expr::Lit(Lit::Int(n))),
            Tok::Float(f) => Ok(Expr::Lit(Lit::Float(f))),
            Tok::Dec(d) => Ok(Expr::Lit(Lit::Dec(d))),
            Tok::Str(parts) => self.template(parts),
            Tok::LParen => {
                if let Some(f) = self.try_arrow(line)? {
                    return Ok(f);
                }
                self.no_bare_arrow_off(|p| {
                    let e = p.parse_expr()?;
                    p.expect(&Tok::RParen, "`)`")?;
                    Ok(e)
                })
            }
            Tok::LBracket => self.no_bare_arrow_off(|p| {
                // `[...xs, 3]` is `__concat(xs, [3])`: lists between the spreads.
                let mut parts: Vec<Expr> = Vec::new();
                let mut items = Vec::new();
                while !p.at(&Tok::RBracket)? {
                    if p.eat_spread()? {
                        if !items.is_empty() {
                            parts.push(Expr::List(std::mem::take(&mut items)));
                        }
                        parts.push(p.parse_expr()?);
                    } else {
                        items.push(p.parse_expr()?);
                    }
                    if !p.eat(&Tok::Comma)? {
                        break;
                    }
                }
                p.expect(&Tok::RBracket, "`]` to close the list")?;
                if parts.is_empty() {
                    return Ok(Expr::List(items));
                }
                if !items.is_empty() {
                    parts.push(Expr::List(items));
                }
                Ok(Expr::Call(Box::new(Expr::Ident("__concat".into(), line)), parts, line))
            }),
            Tok::LBrace => self.no_bare_arrow_off(|p| {
                if p.brace_is_map()? {
                    p.parse_map_rest()
                } else {
                    p.parse_block_rest()
                }
            }),
            Tok::Lt => {
                if self.peeked.is_none() && self.lx.peek_char().map_or(false, |c| c.is_alphabetic()) {
                    self.parse_html(line)
                } else {
                    Err(QuilloError::new("expected an expression, found `<`", line))
                }
            }
            Tok::Ident(k) => match k.as_str() {
                "true" => Ok(Expr::Lit(Lit::Bool(true))),
                "false" => Ok(Expr::Lit(Lit::Bool(false))),
                "null" => Ok(Expr::Lit(Lit::Nil)),
                "nil" => Err(QuilloError::new("use `null` instead of `nil`", line)),
                "not" => Err(QuilloError::new("use `!x` instead of `not x`", line)),
                "if" => self.parse_if(),
                "match" => self.parse_match(line),
                "for" => self.parse_for(line),
                "while" => self.parse_while(line),
                "fn" => Err(QuilloError::new(
                    "anonymous functions are written `(x) => x * 2` (or `x => x * 2`); `fn` is for named functions",
                    line,
                )),
                "reply" => self.parse_reply(line),
                // `?? return null`, `pat => return x`: leaves the function, as
                // the statement does.
                "return" => {
                    let e = if can_start_expr(&self.peek()?) { Some(self.parse_expr()?) } else { None };
                    Ok(Expr::Block(vec![Stmt::Return(e)]))
                }
                kw if KEYWORDS.contains(&kw) => Err(QuilloError::new(format!("unexpected keyword `{}`", kw), line)),
                _ if self.peeked.is_none() && self.lx.peek_char() == Some('`') => {
                    // A tagged template: css`...` is css(`...`).
                    let parts = self.lx.lex_template()?;
                    self.lx.last = Some(Tok::RParen);
                    let t = self.template(parts)?;
                    Ok(Expr::Call(Box::new(Expr::Ident(k.clone(), line)), vec![t], line))
                }
                _ => {
                    // `x => body`: an arrow function with one parameter.
                    if self.no_bare_arrow == 0 && self.at(&Tok::FatArrow)? {
                        self.next()?;
                        let body = self.measured(Self::parse_arrow_body)?;
                        let params = vec![Param { name: k.clone(), ty: None, default: None }];
                        return Ok(arrow(params, None, body, line, self.file.clone()));
                    }
                    Ok(Expr::Ident(k.clone(), line))
                }
            },
            other => Err(QuilloError::new(format!("expected an expression, found {}", other.describe()), line)),
        }
    }

    /// After `{` in expression position: a map if it is empty or starts with
    /// `key:`, `key,` or `{ key }`; otherwise a block. A keyword may be a key
    /// (`{ view: "grid" }`), but not a shorthand one.
    fn brace_is_map(&mut self) -> PResult<bool> {
        match self.peek()? {
            Tok::RBrace => Ok(true),
            Tok::DotDot => {
                let s = self.save();
                let r = self.eat_spread()?;
                self.restore(s);
                Ok(r)
            }
            Tok::Ident(k) => {
                let keyword = KEYWORDS.contains(&k.as_str());
                let s = self.save();
                self.next()?;
                let r = match self.peek()? {
                    Tok::Colon => true,
                    Tok::Comma | Tok::RBrace => !keyword,
                    _ => false,
                };
                self.restore(s);
                Ok(r)
            }
            Tok::Str(_) => {
                let s = self.save();
                self.next()?;
                let r = matches!(self.peek()?, Tok::Colon);
                self.restore(s);
                Ok(r)
            }
            _ => Ok(false),
        }
    }

    /// `...` before a value in a list or a map, which spreads it.
    fn eat_spread(&mut self) -> PResult<bool> {
        if !self.at(&Tok::DotDot)? {
            return Ok(false);
        }
        let s = self.save();
        self.next()?;
        if self.eat(&Tok::Dot)? {
            return Ok(true);
        }
        self.restore(s);
        Ok(false)
    }

    fn parse_map_rest(&mut self) -> PResult<Expr> {
        // `{ ...m, name: v }` is `merge(m, { name: v })`: later keys win.
        let mut parts: Vec<Expr> = Vec::new();
        let mut entries = Vec::new();
        loop {
            self.skip_list_seps()?;
            if self.eat(&Tok::RBrace)? {
                if parts.is_empty() {
                    return Ok(Expr::Map(entries));
                }
                if !entries.is_empty() {
                    parts.push(Expr::Map(entries));
                }
                let line = self.lx.line;
                return Ok(Expr::Call(Box::new(Expr::Ident("__merge".into(), line)), parts, line));
            }
            if self.eat_spread()? {
                if !entries.is_empty() {
                    parts.push(Expr::Map(std::mem::take(&mut entries)));
                }
                parts.push(self.parse_expr()?);
                match self.peek()? {
                    Tok::Comma | Tok::Newline | Tok::Semi | Tok::RBrace => {}
                    other => return self.err_at(format!("expected `,` or `}}` in map, found {}", other.describe())),
                }
                continue;
            }
            let t = self.next()?;
            let (key, shorthand_ok) = match t.tok {
                Tok::Ident(s) => (s, true),
                Tok::Str(parts) => match lit_string(parts) {
                    Some(s) => (s, false),
                    None => return Err(QuilloError::new("map keys must be plain strings", t.line)),
                },
                other => return Err(QuilloError::new(format!("expected a map key, found {}", other.describe()), t.line)),
            };
            if self.eat(&Tok::Colon)? {
                let v = self.parse_expr()?;
                entries.push((key, v));
            } else if shorthand_ok {
                entries.push((key.clone(), Expr::Ident(key, t.line)));
            } else {
                return Err(QuilloError::new(format!("expected `:` after map key \"{}\"", key), t.line));
            }
            match self.peek()? {
                Tok::Comma | Tok::Newline | Tok::Semi | Tok::RBrace => {}
                other => return self.err_at(format!("expected `,` or `}}` in map, found {}", other.describe())),
            }
        }
    }

    fn template(&mut self, parts: Vec<StrPart>) -> PResult<Expr> {
        let mut out = Vec::new();
        for p in parts {
            match p {
                StrPart::Lit(s) => out.push(TPart::Lit(s)),
                StrPart::Expr(src, line) => {
                    let mut sub = Parser::with_line(&src, line);
                    (sub.depth, sub.deepest) = (self.depth, self.depth);
                    let e = sub.parse_expr()?;
                    self.deepest = self.deepest.max(sub.deepest);
                    sub.skip_seps()?;
                    if !sub.at(&Tok::Eof)? {
                        return Err(QuilloError::new("unexpected text inside `{…}` in string", line));
                    }
                    out.push(TPart::Expr(e));
                }
            }
        }
        if out.iter().all(|p| matches!(p, TPart::Lit(_))) {
            let s: String = out
                .into_iter()
                .map(|p| match p {
                    TPart::Lit(s) => s,
                    TPart::Expr(_) => unreachable!(),
                })
                .collect();
            Ok(Expr::Lit(Lit::Str(s.into())))
        } else {
            Ok(Expr::Template(out))
        }
    }

    fn parse_if(&mut self) -> PResult<Expr> {
        let cond = self.parse_expr()?;
        if self.at_kw("then")? {
            return self.err_at("`if c then a else b` is now written `c ? a : b`");
        }
        if !self.at(&Tok::LBrace)? {
            return self.err_at("expected `{` after the `if` condition (or write `cond ? a : b`)");
        }
        let a = self.parse_block()?;
        let b = if self.eat_kw("else")? {
            if self.eat_kw("if")? {
                Some(Box::new(self.nested(Self::parse_if)?))
            } else {
                Some(Box::new(self.parse_block()?))
            }
        } else {
            None
        };
        Ok(Expr::If(Box::new(cond), Box::new(a), b))
    }

    fn parse_match(&mut self, line: u32) -> PResult<Expr> {
        let subj = self.parse_expr()?;
        self.expect(&Tok::LBrace, "`{` after the match subject")?;
        let mut arms = Vec::new();
        loop {
            self.skip_list_seps()?;
            if self.eat(&Tok::RBrace)? {
                break;
            }
            let pat = self.parse_pattern()?;
            let guard = if self.eat_kw("if")? {
                self.no_bare_arrow += 1;
                let g = self.parse_expr();
                self.no_bare_arrow -= 1;
                Some(g?)
            } else {
                None
            };
            self.expect(&Tok::FatArrow, "`=>` after the match pattern")?;
            let body = self.parse_arrow_body()?;
            arms.push(Arm { pat, guard, body });
        }
        Ok(Expr::Match(Box::new(subj), arms, line))
    }

    fn parse_while(&mut self, line: u32) -> PResult<Expr> {
        self.no_bare_arrow += 1;
        let cond = self.parse_expr();
        self.no_bare_arrow -= 1;
        let cond = cond?;
        if !self.at(&Tok::LBrace)? {
            return self.err_at("expected `{` after the `while` condition");
        }
        let body = self.parse_block()?;
        Ok(Expr::While(Box::new(cond), Box::new(body), line))
    }

    /// What `let` and `for` bind: a pattern, in which a bare name binds
    /// that name, even one that names a type (`let num = 5`), as a
    /// parameter does; `match` tests types.
    fn parse_binding(&mut self) -> PResult<Pattern> {
        Ok(match self.parse_pattern()? {
            Pattern::Type(n) if TYPE_NAMES.contains(&n.as_str()) => Pattern::Bind(n),
            p => p,
        })
    }

    fn parse_for(&mut self, line: u32) -> PResult<Expr> {
        let pat = self.parse_binding()?;
        if !self.eat_kw("in")? {
            return self.err_at("expected `in` after the `for` pattern");
        }
        self.no_bare_arrow += 1;
        let iter = self.parse_expr();
        self.no_bare_arrow -= 1;
        let iter = iter?;
        let body = if self.eat(&Tok::FatArrow)? {
            self.parse_expr()?
        } else if self.at(&Tok::LBrace)? {
            self.parse_block()?
        } else {
            return self.err_at("expected `=>` or `{` after the `for` list");
        };
        Ok(Expr::For(pat, Box::new(iter), Box::new(body), line))
    }

    fn parse_reply(&mut self, line: u32) -> PResult<Expr> {
        let status = self.parse_postfix()?;
        let body = if can_start_expr(&self.peek()?) { Some(Box::new(self.parse_expr()?)) } else { None };
        Ok(Expr::Reply(Box::new(status), body, line))
    }

    // ---- patterns ------------------------------------------------------

    pub fn parse_pattern(&mut self) -> PResult<Pattern> {
        self.nested(Self::parse_pattern_alts)
    }

    fn parse_pattern_alts(&mut self) -> PResult<Pattern> {
        let mut alts = vec![self.parse_pattern_atom()?];
        while self.eat(&Tok::Bar)? {
            alts.push(self.parse_pattern_atom()?);
        }
        let mut p = if alts.len() == 1 { alts.pop().unwrap() } else { Pattern::Alt(alts) };
        while self.eat_kw("as")? {
            self.deeper()?;
            let n = self.expect_ident("a name after `as`")?;
            p = Pattern::As(Box::new(p), n);
        }
        Ok(p)
    }

    fn pattern_key(&mut self) -> PResult<String> {
        let t = self.next()?;
        match t.tok {
            Tok::Ident(s) => Ok(s),
            Tok::Str(parts) => {
                lit_string(parts).ok_or_else(|| QuilloError::new("pattern keys must be plain strings", t.line))
            }
            other => Err(QuilloError::new(format!("expected a field name, found {}", other.describe()), t.line)),
        }
    }

    fn parse_pattern_atom(&mut self) -> PResult<Pattern> {
        let t = self.next()?;
        let line = t.line;
        match t.tok {
            Tok::Int(n) => Ok(Pattern::Lit(Lit::Int(n))),
            Tok::Float(f) => Ok(Pattern::Lit(Lit::Float(f))),
            Tok::Dec(d) => Ok(Pattern::Lit(Lit::Dec(d))),
            Tok::Minus => {
                let t = self.next()?;
                match t.tok {
                    Tok::Int(n) => Ok(Pattern::Lit(Lit::Int(-n))),
                    Tok::Float(f) => Ok(Pattern::Lit(Lit::Float(-f))),
                    Tok::Dec(d) => Ok(Pattern::Lit(Lit::Dec(d.neg()))),
                    other => Err(QuilloError::new(format!("expected a number after `-`, found {}", other.describe()), line)),
                }
            }
            Tok::Str(parts) => match lit_string(parts) {
                Some(s) => Ok(Pattern::Lit(Lit::Str(s.into()))),
                None => Err(QuilloError::new("string patterns cannot contain `{…}`", line)),
            },
            Tok::Ident(k) => Ok(match k.as_str() {
                "_" => Pattern::Wild,
                "true" => Pattern::Lit(Lit::Bool(true)),
                "false" => Pattern::Lit(Lit::Bool(false)),
                "null" => Pattern::Lit(Lit::Nil),
                "nil" => return Err(QuilloError::new("use `null` instead of `nil`", line)),
                t if TYPE_NAMES.contains(&t) => Pattern::Type(k.clone()),
                // A record, or another type of the program's own.
                t if crate::ast::is_type_name(t) => Pattern::Type(k.clone()),
                t if KEYWORDS.contains(&t) => {
                    return Err(QuilloError::new(format!("`{}` is a keyword, so it cannot be a name here: pick another name", t), line))
                }
                _ => Pattern::Bind(k.clone()),
            }),
            Tok::LBracket => {
                let mut items = Vec::new();
                let mut rest = None;
                while !self.at(&Tok::RBracket)? {
                    if self.eat(&Tok::DotDot)? {
                        let name =
                            if let Tok::Ident(_) = self.peek()? { Some(self.expect_ident("a name")?) } else { None };
                        rest = Some(name);
                        break;
                    }
                    items.push(self.parse_pattern()?);
                    if !self.eat(&Tok::Comma)? {
                        break;
                    }
                }
                self.expect(&Tok::RBracket, "`]` to close the list pattern")?;
                Ok(Pattern::List(items, rest))
            }
            Tok::LBrace => {
                let mut fields = Vec::new();
                loop {
                    self.skip_list_seps()?;
                    if self.eat(&Tok::RBrace)? {
                        break;
                    }
                    let mut path = vec![self.pattern_key()?];
                    while self.eat(&Tok::Dot)? {
                        path.push(self.pattern_key()?);
                    }
                    let pat = if self.eat(&Tok::Colon)? {
                        self.parse_pattern()?
                    } else {
                        Pattern::Bind(path.last().unwrap().clone())
                    };
                    let default = if self.eat(&Tok::Assign)? { Some(self.parse_expr()?) } else { None };
                    fields.push(RecField { path, pat, default });
                    match self.peek()? {
                        Tok::Comma | Tok::Newline | Tok::Semi | Tok::RBrace => {}
                        other => {
                            return self.err_at(format!("expected `,` or `}}` in record pattern, found {}", other.describe()))
                        }
                    }
                }
                Ok(Pattern::Record(fields))
            }
            other => Err(QuilloError::new(format!("expected a pattern, found {}", other.describe()), line)),
        }
    }

    // ---- HTML literals -------------------------------------------------
    //
    // <tag.class#id-{expr} attr="text {x}" flag other={expr}>
    //     "text" {expr} <child/>
    // </tag>

    fn parse_html(&mut self, line: u32) -> PResult<Expr> {
        let el = self.parse_html_el(line)?;
        self.lx.last = Some(Tok::RParen);
        Ok(Expr::Html(Box::new(el), line))
    }

    fn html_err<T>(&self, msg: impl Into<String>) -> PResult<T> {
        Err(QuilloError::new(msg, self.lx.line))
    }

    fn read_html_name(&mut self) -> String {
        let mut s = String::new();
        while let Some(c) = self.lx.peek_char() {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                s.push(c);
                self.lx.bump();
            } else {
                break;
            }
        }
        s
    }

    fn parse_html_el(&mut self, line: u32) -> PResult<HtmlEl> {
        // Where its `<` is (just read).
        let start = self.lx.pos.saturating_sub(1);
        let tag = self.read_html_name();
        let mut id = None;
        let mut classes = Vec::new();
        loop {
            match self.lx.peek_char() {
                Some('.') => {
                    self.lx.bump();
                    classes.push(self.read_interp_name()?);
                }
                Some('#') => {
                    self.lx.bump();
                    id = Some(self.read_interp_name()?);
                }
                _ => break,
            }
        }
        let mut attrs = Vec::new();
        let self_closing = loop {
            self.lx.skip_all_ws();
            match self.lx.peek_char() {
                Some('/') => {
                    self.lx.bump();
                    if self.lx.peek_char() != Some('>') {
                        return self.html_err(format!("expected `>` after `/` in <{}>", tag));
                    }
                    self.lx.bump();
                    break true;
                }
                Some('>') => {
                    self.lx.bump();
                    break VOID_TAGS.contains(&tag.as_str());
                }
                Some('.') => {
                    self.lx.bump();
                    classes.push(self.read_interp_name()?);
                }
                Some('#') => {
                    self.lx.bump();
                    id = Some(self.read_interp_name()?);
                }
                Some(c) if c.is_alphabetic() || c == '_' || c == '@' || c == ':' => {
                    let mut name = String::new();
                    while let Some(c) = self.lx.peek_char() {
                        if c.is_alphanumeric() || "-_:@.".contains(c) {
                            name.push(c);
                            self.lx.bump();
                        } else {
                            break;
                        }
                    }
                    self.lx.skip_inline_ws();
                    if self.lx.peek_char() == Some('=') {
                        self.lx.bump();
                        self.lx.skip_inline_ws();
                        let v = match self.lx.peek_char() {
                            Some('"') => {
                                let parts = self.lx.lex_string()?;
                                self.template(parts)?
                            }
                            Some('{') => self.parse_braced_expr()?,
                            _ => {
                                return self.html_err(format!(
                                    "attribute `{}` needs a \"string\" or {{expression}} value",
                                    name
                                ))
                            }
                        };
                        attrs.push((name, AttrVal::Expr(v)));
                    } else {
                        attrs.push((name, AttrVal::Bool));
                    }
                }
                // `{...map}` spreads a map; `{name}` is short for name={name}.
                Some('{') => {
                    let line = self.lx.line;
                    if self.lx.peek_char_at(1) == Some('.')
                        && self.lx.peek_char_at(2) == Some('.')
                        && self.lx.peek_char_at(3) == Some('.')
                    {
                        self.lx.bump();
                        self.lx.bump();
                        self.lx.bump();
                        let e = self.parse_braced_expr()?;
                        attrs.push(("...".to_string(), AttrVal::Spread(e)));
                    } else {
                        match self.parse_braced_expr()? {
                            Expr::Ident(name, l) => attrs.push((name.clone(), AttrVal::Expr(Expr::Ident(name, l)))),
                            _ => {
                                return Err(QuilloError::new(
                                    format!("in <{}>, write {{name}} (short for name={{name}}) or {{...map}}", tag),
                                    line,
                                ))
                            }
                        }
                    }
                }
                None => return self.html_err(format!("unexpected end of input: unclosed <{}>", tag)),
                Some(c) => return self.html_err(format!("unexpected `{}` in <{}> tag", c, tag)),
            }
        };
        // One of the two would be lost: in a list, `key` and a view's own
        // `key` was such a pair.
        if let Some((_, (name, _))) = attrs.iter().enumerate().find(|(i, (n, _))| n != "..." && attrs[..*i].iter().any(|(m, _)| m == n)) {
            return self.html_err(format!("attribute `{}` is written twice in <{}>", name, tag));
        }
        // The browser decodes these attributes before it runs them, so no
        // escaping keeps a value from data from running as code.
        if let Some((name, _)) = attrs.iter().find(|(n, v)| code_from_data(&tag, &attrs, n, v)) {
            let lower = name.to_ascii_lowercase();
            let msg = match lower.strip_prefix("bind:").unwrap_or(&lower).strip_prefix("on") {
                Some(ev) => format!("`{}` in <{}> takes only text written here, as it runs as code: handle the event with `@{}={{...}}`", name, tag, ev),
                None => format!("`{}` in <{}> takes only text written here, unless the element also has a `sandbox` attribute without allow-same-origin", name, tag),
            };
            return Err(QuilloError::new(msg, line));
        }
        let mut children = Vec::new();
        if !self_closing {
            let text = self.lx.text();
            let opened = OpenTag {
                tag: tag.clone(),
                line,
                end: self.lx.pos,
                end_line: self.lx.line,
                indent: text[start - col_at(text, start) as usize..].iter().take_while(|c| **c == ' ' || **c == '\t').collect(),
            };
            self.open.push(opened);
            let r = self.parse_html_children(&tag, line);
            self.open.pop();
            children = r?;
        }
        // `<textarea>{text}</textarea>` follows `text` in the browser (see
        // `textareaFollows` in the runtime); with `value={...}` as well, one
        // of the two would be lost.
        if tag == "textarea" && !children.is_empty() && attrs.iter().any(|(n, _)| n == "value" || n == "bind:value") {
            return Err(QuilloError::new(
                "a <textarea> takes its text from value={...} or from what is between its tags, not both",
                line,
            ));
        }
        // An element of a component or view with a style of its own.
        if let Some(s) = &mut self.styling {
            s.els += 1;
            if let Some(scope) = &s.scope {
                if !tag.starts_with(|c: char| c.is_uppercase()) {
                    attrs.push((format!("data-q-{}", scope), AttrVal::Bool));
                }
            }
        }
        let facts = ElFacts::of(&tag, &id, &classes, &attrs, &children, VOID_TAGS.contains(&tag.as_str()));
        Ok(HtmlEl { tag, id, classes, attrs, children, line, facts })
    }

    /// What is between an element's tags, up to its end tag. A missing end
    /// tag is told at the element that lacks it, with where it should go
    /// as the fix (branch editor-end-tags): before an end tag that closes
    /// an element around it, before the `}` that ends the code around it,
    /// or before the declaration that the file goes on with.
    fn parse_html_children(&mut self, tag: &str, line: u32) -> PResult<Vec<HtmlChild>> {
        let mut children = Vec::new();
        let mut text = String::new();
        loop {
            match (self.lx.peek_char(), self.lx.peek_char_at(1)) {
                (Some('<'), Some('/')) => {
                    push_text(&mut children, &mut text);
                    let (at, at_line) = (self.lx.pos, self.lx.line);
                    self.lx.bump();
                    self.lx.bump();
                    let close = self.read_html_name();
                    if close != tag {
                        let around = self.open.iter().rev().skip(1).any(|o| o.tag == close);
                        if around {
                            return Err(self.never_closed(format!("before `</{}>` on line {}", close, at_line), at, at_line));
                        }
                        let col = col_at(self.lx.text(), at) + 2;
                        return Err(QuilloError::new(
                            format!("`</{}>` does not close `<{}>` from line {}: expected `</{}>`", close, tag, line, tag),
                            at_line,
                        )
                        .with_fix(Fix { title: format!("Change to </{}>", tag), line: at_line, col, len: close.chars().count() as u32, text: tag.to_string() }));
                    }
                    self.lx.skip_inline_ws();
                    if self.lx.peek_char() != Some('>') {
                        return self.html_err(format!("expected `>` to close </{}", tag));
                    }
                    self.lx.bump();
                    return Ok(children);
                }
                (Some('<'), Some(c)) if c.is_alphabetic() => {
                    push_text(&mut children, &mut text);
                    let l = self.lx.line;
                    self.lx.bump();
                    children.push(HtmlChild::El(self.nested(|p| p.parse_html_el(l))?));
                }
                // `{/* a note */}`: a comment, as in JSX.
                (Some('{'), _) if self.braced_comment_len() > 0 => {
                    for _ in 0..self.braced_comment_len() {
                        self.lx.bump();
                    }
                }
                (Some('{'), _) => {
                    push_text(&mut children, &mut text);
                    children.push(HtmlChild::Expr(and_as_if(self.parse_braced_expr()?)));
                }
                // A line of markup starting with `//` is a comment, not
                // text for the page (`{"//"}` writes the characters).
                (Some('/'), Some('/')) if text.contains('\n') && text.rsplit('\n').next().map_or(false, |l| l.trim().is_empty()) => {
                    while !matches!(self.lx.peek_char(), Some('\n') | None) {
                        self.lx.bump();
                    }
                }
                (Some('}'), _) => {
                    let (at, at_line) = (self.lx.pos, self.lx.line);
                    // `<p>a } b</p>`: the end tag follows, so the `}` is
                    // meant as text.
                    let rest: String = self.lx.text()[at..].iter().take_while(|c| **c != '\n').collect();
                    if rest.contains(&format!("</{}>", tag)) {
                        return self.html_err(format!("unexpected `}}` inside <{}> (write {{\"}}\"}} for the character)", tag));
                    }
                    let mut e = self.never_closed(format!("before the `}}` on line {}", at_line), at, at_line);
                    e.msg.push_str(" (write {\"}\"} for the character)");
                    return Err(e);
                }
                (None, _) => {
                    self.unclosed_at_end = Some(self.open.clone());
                    // The file goes on with a declaration of its own: the
                    // end tag goes after the last line of markup before it.
                    let chars = self.lx.text();
                    let from = self.open.last().map_or(self.lx.pos, |o| o.end);
                    let mut l = self.open.last().map_or(line, |o| o.end_line);
                    let mut next = None;
                    for i in from..chars.len() {
                        if chars[i] == '\n' {
                            l += 1;
                            let rest = &chars[i + 1..];
                            let eol = rest.iter().position(|c| *c == '\n').unwrap_or(rest.len());
                            if starts_declaration(&rest[..eol]) {
                                next = Some((i + 1, l));
                                break;
                            }
                        }
                    }
                    return Err(match next {
                        Some((at, at_line)) => {
                            let word: String = chars[at..].iter().take_while(|c| c.is_alphabetic()).collect();
                            // Back over the blank lines before it.
                            let mut at = at;
                            let mut at_line = at_line;
                            while at > from && chars[at - 1] == '\n' && chars[..at - 1].iter().rev().take_while(|c| **c != '\n').all(|c| c.is_whitespace()) {
                                at -= chars[..at - 1].iter().rev().take_while(|c| **c != '\n').count() + 1;
                                at_line -= 1;
                            }
                            self.never_closed(format!("before `{}` on line {}", word, l), at, at_line)
                        }
                        None => {
                            let (at, at_line) = (chars.len(), self.lx.line);
                            self.never_closed("at the end of the file".to_string(), at, at_line)
                        }
                    });
                }
                (Some(c), _) => {
                    // Text, as in JSX.
                    text.push(c);
                    self.lx.bump();
                }
            }
        }
    }

    /// The innermost open element has no end tag; it goes at `at`, on its
    /// own line when `at` starts one.
    fn never_closed(&self, place: String, at: usize, at_line: u32) -> QuilloError {
        let o = self.open.last().expect("an element is open");
        let chars = self.lx.text();
        let fix = if starts_line(chars, at) {
            Fix { title: format!("Add </{}>", o.tag), line: at_line, col: 0, len: 0, text: format!("{}</{}>\n", o.indent, o.tag) }
        } else if at == chars.len() {
            Fix { title: format!("Add </{}>", o.tag), line: at_line, col: col_at(chars, at), len: 0, text: format!("\n{}</{}>", o.indent, o.tag) }
        } else {
            Fix { title: format!("Add </{}>", o.tag), line: at_line, col: col_at(chars, at), len: 0, text: format!("</{}>", o.tag) }
        };
        QuilloError::new(format!("`<{}>` is never closed: add `</{}>` {}", o.tag, o.tag, place), o.line).with_fix(fix)
    }

    /// How many characters `{/* ... */}` takes, starting at its `{`, or 0
    /// when what is ahead is not one (only spaces may stand around the
    /// comment; the comment may hold `}`).
    fn braced_comment_len(&self) -> usize {
        let at = |i: usize| self.lx.peek_char_at(i);
        let mut i = 1;
        while matches!(at(i), Some(' ' | '\t' | '\r' | '\n')) {
            i += 1;
        }
        if !(at(i) == Some('/') && at(i + 1) == Some('*')) {
            return 0;
        }
        i += 2;
        loop {
            match (at(i), at(i + 1)) {
                (Some('*'), Some('/')) => break,
                (Some(_), _) => i += 1,
                (None, _) => return 0,
            }
        }
        i += 2;
        while matches!(at(i), Some(' ' | '\t' | '\r' | '\n')) {
            i += 1;
        }
        if at(i) == Some('}') { i + 1 } else { 0 }
    }

    fn read_interp_name(&mut self) -> PResult<Expr> {
        let mut parts = Vec::new();
        let mut buf = String::new();
        loop {
            match self.lx.peek_char() {
                Some(c) if c.is_alphanumeric() || c == '-' || c == '_' => {
                    buf.push(c);
                    self.lx.bump();
                }
                // Tailwind's variants and fractions: `<em.hover:underline>`,
                // `<div.md:flex>`, `<div.w-1/2>`. A `:` was taken for the start
                // of an attribute, silently (FEEDBACK11.md, Q-079; branch
                // shorthand-colon).
                Some(':') if !buf.is_empty() && self.lx.peek_char_at(1).is_some_and(|c| c.is_alphanumeric()) => {
                    buf.push(':');
                    self.lx.bump();
                }
                Some('/') if !buf.is_empty() && self.lx.peek_char_at(1).is_some_and(|c| c.is_ascii_digit()) => {
                    buf.push('/');
                    self.lx.bump();
                }
                Some('{') => {
                    if !buf.is_empty() {
                        parts.push(TPart::Lit(std::mem::take(&mut buf)));
                    }
                    parts.push(TPart::Expr(self.parse_braced_expr()?));
                }
                _ => break,
            }
        }
        if !buf.is_empty() {
            parts.push(TPart::Lit(buf));
        }
        match parts.len() {
            0 => self.html_err("expected a name after `.` or `#`"),
            // `.price`: a plain name, not a template to build each time.
            1 if matches!(parts[0], TPart::Lit(_)) => match parts.pop() {
                Some(TPart::Lit(s)) => Ok(Expr::Lit(Lit::Str(s.into()))),
                _ => unreachable!(),
            },
            _ => Ok(Expr::Template(parts)),
        }
    }

    /// Parses `{expr}` starting at the `{` character, inside an HTML literal.
    fn parse_braced_expr(&mut self) -> PResult<Expr> {
        debug_assert!(self.peeked.is_none());
        self.lx.bump();
        self.lx.stack.push('(');
        self.lx.last = Some(Tok::LParen);
        let e = self.parse_expr()?;
        self.expect(&Tok::RBrace, "`}`")?;
        Ok(e)
    }
}

fn arrow(params: Vec<Param>, ret: Option<Ty>, (body, nesting): (Expr, u32), line: u32, file: Option<Arc<str>>) -> Expr {
    Expr::Lambda(Arc::new(FnDef {
        name: "<fn>".into(),
        params,
        ret,
        err: None,
        effects: None,
        server: false,
        component: false,
        line,
        body,
        nesting,
        file,
        hot: Default::default(),
    }))
}

/// What to write instead of an old Wisp-era spelling.
fn old_syntax_hint(t: &Tok) -> Option<&'static str> {
    match t {
        Tok::Ident(k) => match k.as_str() {
            "and" => Some("use `&&` instead of `and`"),
            "or" => Some("use `||` instead of `or`"),
            "then" => Some("`if c then a else b` is now written `c ? a : b`"),
            _ => None,
        },
        _ => None,
    }
}

/// Text inside an element, with JSX's whitespace rules: lines are trimmed
/// (except where they touch an element or `{}` on the same line), blank
/// lines dropped, and the rest joined with single spaces. HTML entities
/// such as `&amp;` are decoded.
fn jsx_text(raw: &str) -> String {
    let lines: Vec<&str> = raw.split('\n').collect();
    let n = lines.len();
    let mut kept: Vec<String> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let l = l.trim_end_matches('\r');
        let mut t: &str = l;
        if i > 0 {
            t = t.trim_start();
        }
        if i + 1 < n {
            t = t.trim_end();
        }
        if !t.is_empty() {
            kept.push(t.to_string());
        }
    }
    decode_entities(&kept.join(" "))
}

fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest.find(';').filter(|&e| e <= 10);
        let decoded = end.and_then(|e| {
            let name = &rest[1..e];
            let c = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" | "#39" => Some('\''),
                "nbsp" => Some('\u{a0}'),
                "copy" => Some('©'),
                "mdash" => Some('—'),
                "ndash" => Some('–'),
                "hellip" => Some('…'),
                _ => name
                    .strip_prefix("#x")
                    .and_then(|h| u32::from_str_radix(h, 16).ok())
                    .or_else(|| name.strip_prefix('#').and_then(|d| d.parse().ok()))
                    .and_then(char::from_u32),
            };
            c.map(|c| (c, e))
        });
        match decoded {
            Some((c, e)) => {
                out.push(c);
                rest = &rest[e + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// `cond && <x>` where markup goes is `if cond { <x> }`: the same value
/// (only `false` and `null` are false, and neither shows), but a branch,
/// which keeps its elements while `cond` stays true, as `if` and `? :` do.
/// As an `&&` it was a value made anew each time, so inputs in it lost
/// their focus and text on every change
/// (FEEDBACK4.md, Q-041; branch bank-fixes).
/// Looks into what puts markup where it stands: `for` bodies, branches, a
/// block's end.
fn and_as_if(e: Expr) -> Expr {
    match e {
        // The condition stays an `&&` (`cond && true` holds when `cond` does),
        // so `quillo check` still says what it says of `&&` (`count && <x>`
        // with a number) in the words of `&&`.
        Expr::Binary(BinOp::And, a, b, line) if is_markup(&b) => Expr::If(
            Box::new(Expr::Binary(BinOp::And, a, Box::new(Expr::Lit(Lit::Bool(true))), line)),
            Box::new(and_as_if(*b)),
            None,
        ),
        Expr::For(p, it, body, line) => Expr::For(p, it, Box::new(and_as_if(*body)), line),
        Expr::If(c, t, f) => Expr::If(c, Box::new(and_as_if(*t)), f.map(|f| Box::new(and_as_if(*f)))),
        Expr::Match(s, arms, line) => {
            Expr::Match(s, arms.into_iter().map(|a| Arm { body: and_as_if(a.body), ..a }).collect(), line)
        }
        Expr::Block(mut stmts) => {
            match stmts.pop() {
                Some(Stmt::Expr(last)) => stmts.push(Stmt::Expr(and_as_if(last))),
                Some(other) => stmts.push(other),
                None => {}
            }
            Expr::Block(stmts)
        }
        e => e,
    }
}

/// Whether `e` puts markup on the page: an element, or a choice of them.
fn is_markup(e: &Expr) -> bool {
    match e {
        Expr::Html(..) => true,
        Expr::Binary(BinOp::And, _, b, _) => is_markup(b),
        Expr::If(_, t, f) => is_markup(t) || f.as_deref().map_or(false, is_markup),
        _ => false,
    }
}

fn push_text(children: &mut Vec<HtmlChild>, text: &mut String) {
    if text.is_empty() {
        return;
    }
    let t = jsx_text(text);
    text.clear();
    if !t.is_empty() {
        children.push(HtmlChild::Expr(Expr::Lit(Lit::Str(t.into()))));
    }
}

/// Whether `e` mentions `request` anywhere (closures included).
pub(crate) fn names_request(e: &Expr) -> bool {
    let mut found = false;
    crate::check::walk_expr(e, &mut |x| {
        if let Expr::Ident(n, _) = x {
            found |= n == "request";
        }
    });
    found
}

fn pattern_names_request(p: &Pattern) -> bool {
    match p {
        Pattern::Record(fields) => {
            fields.iter().any(|f| f.default.as_ref().map_or(false, names_request) || pattern_names_request(&f.pat))
        }
        Pattern::List(items, _) | Pattern::Alt(items) => items.iter().any(pattern_names_request),
        Pattern::As(q, _) => pattern_names_request(q),
        _ => false,
    }
}

/// The names a `let` pattern binds.
fn pattern_binds(p: &Pattern, out: &mut Vec<String>) {
    match p {
        Pattern::Bind(n) => out.push(n.clone()),
        Pattern::As(inner, n) => {
            pattern_binds(inner, out);
            out.push(n.clone());
        }
        Pattern::Alt(ps) => ps.iter().for_each(|p| pattern_binds(p, out)),
        Pattern::List(ps, rest) => {
            ps.iter().for_each(|p| pattern_binds(p, out));
            if let Some(Some(n)) = rest {
                out.push(n.clone());
            }
        }
        Pattern::Record(fs) => fs.iter().for_each(|f| pattern_binds(&f.pat, out)),
        Pattern::Wild | Pattern::Lit(_) | Pattern::Type(_) => {}
    }
}
