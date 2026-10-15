//! Tokenizer. The parser drives it one token at a time, and switches to
//! reading characters directly for HTML literals and route paths.

use crate::error::WispError;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq)]
pub enum StrPart {
    Lit(String),
    /// Source of an interpolated `{expr}` and the line it starts on.
    Expr(String, u32),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Ident(String),
    Int(i64),
    Float(f64),
    Str(Vec<StrPart>),
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Colon,
    Semi,
    Dot,
    DotDot,
    Newline,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Assign,
    PlusAssign,
    MinusAssign,
    EqEq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    Bar,
    PipeGt,
    Question,
    QQ,
    Bang,
    FatArrow,
    Arrow,
    Eof,
}

impl Tok {
    pub fn describe(&self) -> String {
        match self {
            Tok::Ident(s) => format!("`{}`", s),
            Tok::Int(n) => format!("`{}`", n),
            Tok::Float(n) => format!("`{}`", n),
            Tok::Str(_) => "a string".into(),
            Tok::Newline => "end of line".into(),
            Tok::Eof => "end of input".into(),
            other => format!("`{}`", other.symbol()),
        }
    }

    fn symbol(&self) -> &'static str {
        match self {
            Tok::LParen => "(",
            Tok::RParen => ")",
            Tok::LBrace => "{",
            Tok::RBrace => "}",
            Tok::LBracket => "[",
            Tok::RBracket => "]",
            Tok::Comma => ",",
            Tok::Colon => ":",
            Tok::Semi => ";",
            Tok::Dot => ".",
            Tok::DotDot => "..",
            Tok::Plus => "+",
            Tok::Minus => "-",
            Tok::Star => "*",
            Tok::Slash => "/",
            Tok::Percent => "%",
            Tok::Assign => "=",
            Tok::PlusAssign => "+=",
            Tok::MinusAssign => "-=",
            Tok::EqEq => "==",
            Tok::NotEq => "!=",
            Tok::Lt => "<",
            Tok::Le => "<=",
            Tok::Gt => ">",
            Tok::Ge => ">=",
            Tok::Bar => "|",
            Tok::PipeGt => "|>",
            Tok::Question => "?",
            Tok::QQ => "??",
            Tok::Bang => "!",
            Tok::FatArrow => "=>",
            Tok::Arrow => "->",
            _ => "?",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Token {
    pub tok: Tok,
    pub line: u32,
}

#[derive(Clone)]
pub struct Lexer {
    chars: Rc<Vec<char>>,
    pub pos: usize,
    pub line: u32,
    /// Open brackets. Newlines are ignored inside `(` and `[`.
    pub stack: Vec<char>,
    /// The last token produced, used to decide whether a newline ends a statement.
    pub last: Option<Tok>,
}

impl Lexer {
    pub fn new(src: &str, line: u32) -> Self {
        Lexer { chars: Rc::new(src.chars().collect()), pos: 0, line, stack: Vec::new(), last: None }
    }

    pub fn peek_char(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    pub fn peek_char_at(&self, n: usize) -> Option<char> {
        self.chars.get(self.pos + n).copied()
    }

    pub fn bump(&mut self) -> Option<char> {
        let c = self.peek_char()?;
        self.pos += 1;
        if c == '\n' {
            self.line += 1;
        }
        Some(c)
    }

    fn starts_with(&self, s: &str) -> bool {
        s.chars().enumerate().all(|(i, c)| self.peek_char_at(i) == Some(c))
    }

    /// Skips spaces, tabs and `//` comments, but not newlines.
    pub fn skip_inline_ws(&mut self) {
        loop {
            match self.peek_char() {
                Some(' ' | '\t' | '\r') => {
                    self.bump();
                }
                Some('/') if self.peek_char_at(1) == Some('/') => {
                    while let Some(c) = self.peek_char() {
                        if c == '\n' {
                            break;
                        }
                        self.bump();
                    }
                }
                _ => break,
            }
        }
    }

    pub fn skip_all_ws(&mut self) {
        loop {
            self.skip_inline_ws();
            if self.peek_char() == Some('\n') {
                self.bump();
            } else {
                break;
            }
        }
    }

    pub fn word_ahead(&self) -> String {
        let mut s = String::new();
        let mut i = 0;
        while let Some(c) = self.peek_char_at(i) {
            if c.is_alphanumeric() || c == '_' {
                s.push(c);
                i += 1;
            } else {
                break;
            }
        }
        s
    }

    /// A newline ends a statement only when the previous token could end an
    /// expression and the next line does not obviously continue it
    /// (`|>`, `??`, `.method`, `else`, ...).
    fn newline_significant(&self) -> bool {
        if matches!(self.stack.last(), Some('(') | Some('[')) {
            return false;
        }
        let value_like = match &self.last {
            None => false,
            Some(Tok::Ident(k)) => !matches!(k.as_str(), "and" | "or" | "not" | "then" | "else" | "in"),
            Some(
                Tok::Int(_)
                | Tok::Float(_)
                | Tok::Str(_)
                | Tok::RParen
                | Tok::RBrace
                | Tok::RBracket
                | Tok::Question,
            ) => true,
            _ => false,
        };
        if !value_like {
            return false;
        }
        if self.starts_with("|") || self.starts_with("??") {
            return false;
        }
        if self.starts_with(".") && !self.starts_with("..") {
            return false;
        }
        if matches!(self.word_ahead().as_str(), "else" | "then" | "and" | "or") {
            return false;
        }
        self.peek_char().is_some()
    }

    pub fn next_token(&mut self) -> Result<Token, WispError> {
        loop {
            self.skip_inline_ws();
            if self.peek_char() == Some('\n') {
                let line = self.line;
                self.skip_all_ws();
                if self.newline_significant() {
                    self.last = Some(Tok::Newline);
                    return Ok(Token { tok: Tok::Newline, line });
                }
                continue;
            }
            break;
        }
        let line = self.line;
        let c = match self.peek_char() {
            None => return Ok(Token { tok: Tok::Eof, line }),
            Some(c) => c,
        };
        let tok = if c.is_ascii_digit() {
            self.lex_number()?
        } else if c.is_alphabetic() || c == '_' {
            let w = self.word_ahead();
            for _ in 0..w.chars().count() {
                self.bump();
            }
            Tok::Ident(w)
        } else if c == '"' {
            Tok::Str(self.lex_string()?)
        } else {
            self.bump();
            let n = self.peek_char();
            let two = |lx: &mut Lexer, t: Tok| {
                lx.bump();
                t
            };
            match c {
                '(' => {
                    self.stack.push('(');
                    Tok::LParen
                }
                ')' => {
                    self.stack.pop();
                    Tok::RParen
                }
                '{' => {
                    self.stack.push('{');
                    Tok::LBrace
                }
                '}' => {
                    self.stack.pop();
                    Tok::RBrace
                }
                '[' => {
                    self.stack.push('[');
                    Tok::LBracket
                }
                ']' => {
                    self.stack.pop();
                    Tok::RBracket
                }
                ',' => Tok::Comma,
                ':' => Tok::Colon,
                ';' => Tok::Semi,
                '.' if n == Some('.') => two(self, Tok::DotDot),
                '.' => Tok::Dot,
                '+' if n == Some('=') => two(self, Tok::PlusAssign),
                '+' => Tok::Plus,
                '-' if n == Some('=') => two(self, Tok::MinusAssign),
                '-' if n == Some('>') => two(self, Tok::Arrow),
                '-' => Tok::Minus,
                '*' => Tok::Star,
                '/' => Tok::Slash,
                '%' => Tok::Percent,
                '=' if n == Some('=') => two(self, Tok::EqEq),
                '=' if n == Some('>') => two(self, Tok::FatArrow),
                '=' => Tok::Assign,
                '!' if n == Some('=') => two(self, Tok::NotEq),
                '!' => Tok::Bang,
                '<' if n == Some('=') => two(self, Tok::Le),
                '<' => Tok::Lt,
                '>' if n == Some('=') => two(self, Tok::Ge),
                '>' => Tok::Gt,
                '|' if n == Some('>') => two(self, Tok::PipeGt),
                '|' => Tok::Bar,
                '?' if n == Some('?') => two(self, Tok::QQ),
                '?' => Tok::Question,
                other => return Err(WispError::new(format!("unexpected character `{}`", other), line)),
            }
        };
        self.last = Some(tok.clone());
        Ok(Token { tok, line })
    }

    fn lex_number(&mut self) -> Result<Tok, WispError> {
        let line = self.line;
        let mut s = String::new();
        let digits = |lx: &mut Lexer, s: &mut String| {
            while let Some(c) = lx.peek_char() {
                if c.is_ascii_digit() {
                    s.push(c);
                    lx.bump();
                } else if c == '_' {
                    lx.bump();
                } else {
                    break;
                }
            }
        };
        digits(self, &mut s);
        if self.peek_char() == Some('.') && self.peek_char_at(1).map_or(false, |c| c.is_ascii_digit()) {
            s.push('.');
            self.bump();
            digits(self, &mut s);
            return s.parse::<f64>().map(Tok::Float).map_err(|_| WispError::new("bad number", line));
        }
        s.parse::<i64>()
            .map(Tok::Int)
            .map_err(|_| WispError::new(format!("number `{}` is too large", s), line))
    }

    /// Lexes a string literal starting at the opening quote. `{expr}` parts
    /// are kept as source and parsed later; `\{` is a literal brace.
    pub fn lex_string(&mut self) -> Result<Vec<StrPart>, WispError> {
        let start = self.line;
        let unterminated = || WispError::new("unexpected end of input: unterminated string", start);
        self.bump();
        let mut parts = Vec::new();
        let mut buf = String::new();
        loop {
            let c = self.bump().ok_or_else(unterminated)?;
            match c {
                '"' => break,
                '\\' => {
                    let e = self.bump().ok_or_else(unterminated)?;
                    buf.push(match e {
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        '0' => '\0',
                        '"' | '\\' | '{' | '}' => e,
                        other => {
                            return Err(WispError::new(format!("unknown escape `\\{}` in string", other), self.line))
                        }
                    });
                }
                '{' => {
                    if !buf.is_empty() {
                        parts.push(StrPart::Lit(std::mem::take(&mut buf)));
                    }
                    let line = self.line;
                    let open_brace = || {
                        WispError::new(
                            "unexpected end of input: `{` in a string starts an interpolation that is never closed \
                             (write `\\{` for a literal brace)",
                            line,
                        )
                    };
                    let mut depth = 1;
                    let mut src = String::new();
                    loop {
                        let c = self.bump().ok_or_else(open_brace)?;
                        match c {
                            '{' => depth += 1,
                            '}' => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            '"' => {
                                // A string nested inside the interpolation.
                                src.push('"');
                                loop {
                                    let d = self.bump().ok_or_else(open_brace)?;
                                    src.push(d);
                                    if d == '\\' {
                                        if let Some(x) = self.bump() {
                                            src.push(x);
                                        }
                                    } else if d == '"' {
                                        break;
                                    }
                                }
                                continue;
                            }
                            _ => {}
                        }
                        src.push(c);
                    }
                    if src.trim().is_empty() {
                        return Err(WispError::new("empty `{}` in string (write `\\{` for a literal brace)", line));
                    }
                    parts.push(StrPart::Expr(src, line));
                }
                _ => buf.push(c),
            }
        }
        if !buf.is_empty() || parts.is_empty() {
            parts.push(StrPart::Lit(buf));
        }
        Ok(parts)
    }
}
