//! Recursive-descent parser. Precedence, lowest first:
//! `??`, `|>`, `or`, `and`, `not`, comparisons, `..`, `+ -`, `* / %`,
//! unary `-`, then postfix calls, fields, indexing and `?`.

use crate::ast::*;
use crate::error::WispError;
use crate::lexer::{Lexer, StrPart, Tok, Token};
use std::rc::Rc;

type PResult<T> = Result<T, WispError>;

pub const TYPE_NAMES: &[&str] = &["int", "float", "num", "str", "bool", "list", "map", "html", "fn", "any", "response"];
pub const VOID_TAGS: &[&str] =
    &["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track", "wbr"];
const KEYWORDS: &[&str] = &[
    "let", "fn", "view", "route", "use", "return", "if", "then", "else", "match", "for", "in", "and", "or", "not",
    "true", "false", "nil", "reply", "as",
];
const METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "ANY"];

pub struct Parser {
    lx: Lexer,
    peeked: Option<Token>,
}

fn can_start_expr(t: &Tok) -> bool {
    match t {
        Tok::Int(_)
        | Tok::Float(_)
        | Tok::Str(_)
        | Tok::LParen
        | Tok::LBracket
        | Tok::LBrace
        | Tok::Minus
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
        Parser { lx: Lexer::new(src, line), peeked: None }
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
            Err(WispError::new(format!("expected {}, found {}", what, tok.tok.describe()), tok.line))
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
            other => Err(WispError::new(format!("expected {}, found {}", what, other.describe()), t.line)),
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

    fn save(&self) -> (Lexer, Option<Token>) {
        (self.lx.clone(), self.peeked.clone())
    }

    fn restore(&mut self, s: (Lexer, Option<Token>)) {
        self.lx = s.0;
        self.peeked = s.1;
    }

    fn err_at<T>(&mut self, msg: impl Into<String>) -> PResult<T> {
        let line = self.line()?;
        Err(WispError::new(msg, line))
    }

    // ---- statements ----------------------------------------------------

    pub fn parse_program(&mut self) -> PResult<Vec<Stmt>> {
        let mut out = Vec::new();
        loop {
            self.skip_seps()?;
            if self.at(&Tok::Eof)? {
                return Ok(out);
            }
            out.push(self.parse_stmt()?);
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
            other => self.err_at(format!("expected end of statement, found {}", other.describe())),
        }
    }

    fn parse_stmt(&mut self) -> PResult<Stmt> {
        let line = self.line()?;
        if let Tok::Ident(k) = self.peek()? {
            match k.as_str() {
                "let" => {
                    self.next()?;
                    let pat = self.parse_pattern()?;
                    self.expect(&Tok::Assign, "`=`")?;
                    let e = self.parse_expr()?;
                    return Ok(Stmt::Let(pat, e, line));
                }
                "fn" | "view" => {
                    let saved = self.save();
                    self.next()?;
                    if let Tok::Ident(_) = self.peek()? {
                        let name = self.expect_ident("a function name")?;
                        let def = self.parse_fn_rest(name, line)?;
                        return Ok(Stmt::Fn(Rc::new(def)));
                    }
                    self.restore(saved);
                }
                "route" => {
                    self.next()?;
                    return Ok(Stmt::Route(Rc::new(self.parse_route(line)?)));
                }
                "use" => {
                    self.next()?;
                    let t = self.next()?;
                    if let Tok::Str(parts) = t.tok {
                        if let Some(path) = lit_string(parts) {
                            return Ok(Stmt::Use(path, line));
                        }
                    }
                    return Err(WispError::new("`use` expects a file path string, e.g. use \"lib.wisp\"", line));
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
                _ => return Err(WispError::new("only variables can be assigned to", line)),
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
        if self.eat(&Tok::Bang)? {
            // The declared error type is documentation for now.
            self.expect_ident("an error type after `!`")?;
        }
        let body = if self.eat(&Tok::Assign)? {
            self.parse_expr()?
        } else if self.at(&Tok::LBrace)? {
            self.parse_block()?
        } else {
            return self.err_at(format!("expected `=` or `{{` to start the body of `{}`", name));
        };
        Ok(FnDef { name, params, ret, body, line })
    }

    fn parse_type(&mut self) -> PResult<Ty> {
        let t = self.next()?;
        let name = match t.tok {
            Tok::Ident(s) => s,
            other => return Err(WispError::new(format!("expected a type, found {}", other.describe()), t.line)),
        };
        if !TYPE_NAMES.contains(&name.as_str()) {
            return Err(WispError::new(
                format!("unknown type `{}` (known types: {})", name, TYPE_NAMES.join(", ")),
                t.line,
            ));
        }
        let optional = self.eat(&Tok::Question)?;
        Ok(Ty { name, optional })
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
            return Err(WispError::new(
                format!("unknown HTTP method `{}` (use one of {})", method, METHODS.join(", ")),
                line,
            ));
        }
        // The path is read character by character: `/users/:id(int)/*rest`.
        self.lx.skip_inline_ws();
        if self.lx.peek_char() != Some('/') {
            return Err(WispError::new("expected a path starting with `/` after the method", self.lx.line));
        }
        let mut path = Vec::new();
        while self.lx.peek_char() == Some('/') {
            self.lx.bump();
            match self.lx.peek_char() {
                Some(':') => {
                    self.lx.bump();
                    let name = self.read_word();
                    if name.is_empty() {
                        return Err(WispError::new("expected a parameter name after `:` in the path", self.lx.line));
                    }
                    let ty = if self.lx.peek_char() == Some('(') {
                        self.lx.bump();
                        let t = self.read_word();
                        if self.lx.bump() != Some(')') || !matches!(t.as_str(), "int" | "float" | "num" | "str") {
                            return Err(WispError::new(
                                "path parameter types are written :name(int), :name(float), :name(num) or :name(str)",
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
        Ok(RouteDef { method, path, query, body_pat, body, line })
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
        let mut lhs = self.parse_pipe()?;
        while self.eat(&Tok::QQ)? {
            let rhs = self.parse_pipe()?;
            lhs = Expr::Coalesce(Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_pipe(&mut self) -> PResult<Expr> {
        let mut lhs = self.parse_or()?;
        loop {
            let line = self.line()?;
            if !self.eat(&Tok::PipeGt)? {
                return Ok(lhs);
            }
            let rhs = self.parse_or()?;
            lhs = Expr::Pipe(Box::new(lhs), Box::new(rhs), line);
        }
    }

    fn parse_or(&mut self) -> PResult<Expr> {
        let mut lhs = self.parse_and()?;
        loop {
            let line = self.line()?;
            if !self.eat_kw("or")? {
                return Ok(lhs);
            }
            let rhs = self.parse_and()?;
            lhs = Expr::Binary(BinOp::Or, Box::new(lhs), Box::new(rhs), line);
        }
    }

    fn parse_and(&mut self) -> PResult<Expr> {
        let mut lhs = self.parse_not()?;
        loop {
            let line = self.line()?;
            if !self.eat_kw("and")? {
                return Ok(lhs);
            }
            let rhs = self.parse_not()?;
            lhs = Expr::Binary(BinOp::And, Box::new(lhs), Box::new(rhs), line);
        }
    }

    fn parse_not(&mut self) -> PResult<Expr> {
        let line = self.line()?;
        if self.eat_kw("not")? {
            let e = self.parse_not()?;
            return Ok(Expr::Unary(UnOp::Not, Box::new(e), line));
        }
        self.parse_cmp()
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
        let mut lhs = self.parse_mul()?;
        loop {
            let line = self.line()?;
            let op = match self.peek()? {
                Tok::Plus => BinOp::Add,
                Tok::Minus => BinOp::Sub,
                _ => return Ok(lhs),
            };
            self.next()?;
            let rhs = self.parse_mul()?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs), line);
        }
    }

    fn parse_mul(&mut self) -> PResult<Expr> {
        let mut lhs = self.parse_unary()?;
        loop {
            let line = self.line()?;
            let op = match self.peek()? {
                Tok::Star => BinOp::Mul,
                Tok::Slash => BinOp::Div,
                Tok::Percent => BinOp::Mod,
                _ => return Ok(lhs),
            };
            self.next()?;
            let rhs = self.parse_unary()?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs), line);
        }
    }

    fn parse_unary(&mut self) -> PResult<Expr> {
        let line = self.line()?;
        if self.eat(&Tok::Minus)? {
            let e = self.parse_unary()?;
            return Ok(Expr::Unary(UnOp::Neg, Box::new(e), line));
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> PResult<Expr> {
        let mut e = self.parse_primary()?;
        loop {
            let line = self.line()?;
            match self.peek()? {
                Tok::LParen => {
                    self.next()?;
                    let mut args = Vec::new();
                    while !self.at(&Tok::RParen)? {
                        args.push(self.parse_expr()?);
                        if !self.eat(&Tok::Comma)? {
                            break;
                        }
                    }
                    self.expect(&Tok::RParen, "`)` after arguments")?;
                    e = Expr::Call(Box::new(e), args, line);
                }
                Tok::Dot => {
                    self.next()?;
                    let t = self.next()?;
                    let name = match t.tok {
                        Tok::Ident(s) => s,
                        Tok::Int(n) => n.to_string(),
                        other => {
                            return Err(WispError::new(
                                format!("expected a field name after `.`, found {}", other.describe()),
                                t.line,
                            ))
                        }
                    };
                    e = Expr::Field(Box::new(e), name, line);
                }
                Tok::LBracket => {
                    self.next()?;
                    let i = self.parse_expr()?;
                    self.expect(&Tok::RBracket, "`]`")?;
                    e = Expr::Index(Box::new(e), Box::new(i), line);
                }
                Tok::Question => {
                    self.next()?;
                    e = Expr::Try(Box::new(e));
                }
                _ => return Ok(e),
            }
        }
    }

    fn parse_primary(&mut self) -> PResult<Expr> {
        let t = self.next()?;
        let line = t.line;
        match t.tok {
            Tok::Int(n) => Ok(Expr::Lit(Lit::Int(n))),
            Tok::Float(f) => Ok(Expr::Lit(Lit::Float(f))),
            Tok::Str(parts) => self.template(parts),
            Tok::LParen => {
                let e = self.parse_expr()?;
                self.expect(&Tok::RParen, "`)`")?;
                Ok(e)
            }
            Tok::LBracket => {
                let mut items = Vec::new();
                while !self.at(&Tok::RBracket)? {
                    items.push(self.parse_expr()?);
                    if !self.eat(&Tok::Comma)? {
                        break;
                    }
                }
                self.expect(&Tok::RBracket, "`]` to close the list")?;
                Ok(Expr::List(items))
            }
            Tok::LBrace => {
                if self.brace_is_map()? {
                    self.parse_map_rest()
                } else {
                    self.parse_block_rest()
                }
            }
            Tok::Lt => {
                if self.peeked.is_none() && self.lx.peek_char().map_or(false, |c| c.is_alphabetic()) {
                    self.parse_html(line)
                } else {
                    Err(WispError::new("expected an expression, found `<`", line))
                }
            }
            Tok::Ident(k) => match k.as_str() {
                "true" => Ok(Expr::Lit(Lit::Bool(true))),
                "false" => Ok(Expr::Lit(Lit::Bool(false))),
                "nil" => Ok(Expr::Lit(Lit::Nil)),
                "if" => self.parse_if(),
                "match" => self.parse_match(line),
                "for" => self.parse_for(line),
                "fn" => {
                    let def = self.parse_fn_rest("<fn>".into(), line)?;
                    Ok(Expr::Lambda(Rc::new(def)))
                }
                "reply" => self.parse_reply(line),
                kw if KEYWORDS.contains(&kw) => Err(WispError::new(format!("unexpected keyword `{}`", kw), line)),
                _ => Ok(Expr::Ident(k.clone(), line)),
            },
            other => Err(WispError::new(format!("expected an expression, found {}", other.describe()), line)),
        }
    }

    /// After `{` in expression position: a map if it is empty or starts with
    /// `key:`, `key,` or `{ key }`; otherwise a block.
    fn brace_is_map(&mut self) -> PResult<bool> {
        match self.peek()? {
            Tok::RBrace => Ok(true),
            Tok::Ident(k) if !KEYWORDS.contains(&k.as_str()) => {
                let s = self.save();
                self.next()?;
                let r = matches!(self.peek()?, Tok::Colon | Tok::Comma | Tok::RBrace);
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

    fn parse_map_rest(&mut self) -> PResult<Expr> {
        let mut entries = Vec::new();
        loop {
            self.skip_list_seps()?;
            if self.eat(&Tok::RBrace)? {
                return Ok(Expr::Map(entries));
            }
            let t = self.next()?;
            let (key, shorthand_ok) = match t.tok {
                Tok::Ident(s) => (s, true),
                Tok::Str(parts) => match lit_string(parts) {
                    Some(s) => (s, false),
                    None => return Err(WispError::new("map keys must be plain strings", t.line)),
                },
                other => return Err(WispError::new(format!("expected a map key, found {}", other.describe()), t.line)),
            };
            if self.eat(&Tok::Colon)? {
                let v = self.parse_expr()?;
                entries.push((key, v));
            } else if shorthand_ok {
                entries.push((key.clone(), Expr::Ident(key, t.line)));
            } else {
                return Err(WispError::new(format!("expected `:` after map key \"{}\"", key), t.line));
            }
            match self.peek()? {
                Tok::Comma | Tok::Newline | Tok::Semi | Tok::RBrace => {}
                other => return self.err_at(format!("expected `,` or `}}` in map, found {}", other.describe())),
            }
        }
    }

    fn template(&self, parts: Vec<StrPart>) -> PResult<Expr> {
        let mut out = Vec::new();
        for p in parts {
            match p {
                StrPart::Lit(s) => out.push(TPart::Lit(s)),
                StrPart::Expr(src, line) => {
                    let mut sub = Parser::with_line(&src, line);
                    let e = sub.parse_expr()?;
                    sub.skip_seps()?;
                    if !sub.at(&Tok::Eof)? {
                        return Err(WispError::new("unexpected text inside `{…}` in string", line));
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
        if self.eat_kw("then")? {
            let a = self.parse_expr()?;
            let b = if self.eat_kw("else")? { Some(Box::new(self.parse_expr()?)) } else { None };
            return Ok(Expr::If(Box::new(cond), Box::new(a), b));
        }
        if !self.at(&Tok::LBrace)? {
            return self.err_at("expected `then` or `{` after the `if` condition");
        }
        let a = self.parse_block()?;
        let b = if self.eat_kw("else")? {
            if self.eat_kw("if")? {
                Some(Box::new(self.parse_if()?))
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
            let guard = if self.eat_kw("if")? { Some(self.parse_expr()?) } else { None };
            self.expect(&Tok::FatArrow, "`=>` after the match pattern")?;
            let body = self.parse_expr()?;
            arms.push(Arm { pat, guard, body });
        }
        Ok(Expr::Match(Box::new(subj), arms, line))
    }

    fn parse_for(&mut self, line: u32) -> PResult<Expr> {
        let pat = self.parse_pattern()?;
        if !self.eat_kw("in")? {
            return self.err_at("expected `in` after the `for` pattern");
        }
        let iter = self.parse_expr()?;
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
        let mut alts = vec![self.parse_pattern_atom()?];
        while self.eat(&Tok::Bar)? {
            alts.push(self.parse_pattern_atom()?);
        }
        let mut p = if alts.len() == 1 { alts.pop().unwrap() } else { Pattern::Alt(alts) };
        while self.eat_kw("as")? {
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
                lit_string(parts).ok_or_else(|| WispError::new("pattern keys must be plain strings", t.line))
            }
            other => Err(WispError::new(format!("expected a field name, found {}", other.describe()), t.line)),
        }
    }

    fn parse_pattern_atom(&mut self) -> PResult<Pattern> {
        let t = self.next()?;
        let line = t.line;
        match t.tok {
            Tok::Int(n) => Ok(Pattern::Lit(Lit::Int(n))),
            Tok::Float(f) => Ok(Pattern::Lit(Lit::Float(f))),
            Tok::Minus => {
                let t = self.next()?;
                match t.tok {
                    Tok::Int(n) => Ok(Pattern::Lit(Lit::Int(-n))),
                    Tok::Float(f) => Ok(Pattern::Lit(Lit::Float(-f))),
                    other => Err(WispError::new(format!("expected a number after `-`, found {}", other.describe()), line)),
                }
            }
            Tok::Str(parts) => match lit_string(parts) {
                Some(s) => Ok(Pattern::Lit(Lit::Str(s.into()))),
                None => Err(WispError::new("string patterns cannot contain `{…}`", line)),
            },
            Tok::Ident(k) => Ok(match k.as_str() {
                "_" => Pattern::Wild,
                "true" => Pattern::Lit(Lit::Bool(true)),
                "false" => Pattern::Lit(Lit::Bool(false)),
                "nil" => Pattern::Lit(Lit::Nil),
                t if TYPE_NAMES.contains(&t) => Pattern::Type(k.clone()),
                t if KEYWORDS.contains(&t) => {
                    return Err(WispError::new(format!("unexpected keyword `{}` in pattern", t), line))
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
            other => Err(WispError::new(format!("expected a pattern, found {}", other.describe()), line)),
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
        Err(WispError::new(msg, self.lx.line))
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
                None => return self.html_err(format!("unexpected end of input: unclosed <{}>", tag)),
                Some(c) => return self.html_err(format!("unexpected `{}` in <{}> tag", c, tag)),
            }
        };
        let mut children = Vec::new();
        if !self_closing {
            loop {
                self.lx.skip_all_ws();
                match (self.lx.peek_char(), self.lx.peek_char_at(1)) {
                    (Some('<'), Some('/')) => {
                        self.lx.bump();
                        self.lx.bump();
                        let close = self.read_html_name();
                        if close != tag {
                            return self.html_err(format!(
                                "expected </{}> (opened on line {}), found </{}>",
                                tag, line, close
                            ));
                        }
                        self.lx.skip_inline_ws();
                        if self.lx.peek_char() != Some('>') {
                            return self.html_err(format!("expected `>` to close </{}", tag));
                        }
                        self.lx.bump();
                        break;
                    }
                    (Some('<'), Some(c)) if c.is_alphabetic() => {
                        let l = self.lx.line;
                        self.lx.bump();
                        children.push(HtmlChild::El(self.parse_html_el(l)?));
                    }
                    (Some('"'), _) => {
                        let parts = self.lx.lex_string()?;
                        children.push(HtmlChild::Expr(self.template(parts)?));
                    }
                    (Some('{'), _) => children.push(HtmlChild::Expr(self.parse_braced_expr()?)),
                    (None, _) => {
                        return self.html_err(format!("unexpected end of input: unclosed <{}> from line {}", tag, line))
                    }
                    (Some(c), _) => {
                        return self.html_err(format!(
                            "unexpected `{}` inside <{}>: children are <elements>, \"text\" or {{expressions}}",
                            c, tag
                        ))
                    }
                }
            }
        }
        Ok(HtmlEl { tag, id, classes, attrs, children, line })
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
        if parts.is_empty() {
            return self.html_err("expected a name after `.` or `#`");
        }
        Ok(Expr::Template(parts))
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
