//! Tokenizer. The parser drives it one token at a time, and switches to
//! reading characters directly for HTML literals and route paths.

use crate::error::QuilloError;
use std::sync::Arc;

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
    /// `12.50m`: an exact decimal.
    Dec(crate::decimal::Dec),
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
    OrOr,
    AndAnd,
    PipeGt,
    Question,
    QQ,
    Bang,
    FatArrow,
    Arrow,
    Tilde,
    Eof,
}

impl Tok {
    pub fn describe(&self) -> String {
        match self {
            Tok::Ident(s) => format!("`{}`", s),
            Tok::Int(n) => format!("`{}`", n),
            Tok::Float(n) => format!("`{}`", n),
            Tok::Dec(d) => format!("`{}m`", d),
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
            Tok::OrOr => "||",
            Tok::AndAnd => "&&",
            Tok::PipeGt => "|>",
            Tok::Question => "?",
            Tok::QQ => "??",
            Tok::Bang => "!",
            Tok::FatArrow => "=>",
            Tok::Arrow => "->",
            Tok::Tilde => "~",
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
    chars: Arc<Vec<char>>,
    pub pos: usize,
    pub line: u32,
    /// Open brackets. Newlines are ignored inside `(` and `[`.
    pub stack: Vec<char>,
    /// The last token produced, used to decide whether a newline ends a statement.
    pub last: Option<Tok>,
    /// A `/* ... */` comment just skipped held a line break (on this line):
    /// it ends a statement as that line break would have.
    comment_newline: Option<u32>,
}

impl Lexer {
    pub fn new(src: &str, line: u32) -> Self {
        Lexer { chars: Arc::new(src.chars().collect()), pos: 0, line, stack: Vec::new(), last: None, comment_newline: None }
    }

    pub fn peek_char(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    /// The text being read, for messages that point back into it.
    pub fn text(&self) -> &[char] {
        &self.chars
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

    /// Skips spaces, tabs, `//` comments and `/* ... */` comments, but not
    /// newlines (a `/* */` comment over several lines is noted in
    /// `comment_newline`).
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
                Some('/') if self.peek_char_at(1) == Some('*') => {
                    let line = self.line;
                    self.bump();
                    self.bump();
                    // Up to `*/`, or the end of the text (then what was
                    // being parsed is reported unfinished).
                    while self.peek_char().is_some() && !(self.peek_char() == Some('*') && self.peek_char_at(1) == Some('/')) {
                        self.bump();
                    }
                    if self.peek_char().is_some() {
                        self.bump();
                        self.bump();
                    }
                    if self.line != line && self.comment_newline.is_none() {
                        self.comment_newline = Some(line);
                    }
                }
                _ => break,
            }
        }
    }

    /// Skips spaces and tabs only: before a path, where `/*` starts a
    /// wildcard (`route GET /*rest`), not a comment.
    pub fn skip_spaces(&mut self) {
        while matches!(self.peek_char(), Some(' ' | '\t' | '\r')) {
            self.bump();
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
        self.comment_newline = None;
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
    /// (`|`, `??`, `.method`, `else`, `- x`, ...).
    fn newline_significant(&self) -> bool {
        if matches!(self.stack.last(), Some('(') | Some('[')) {
            return false;
        }
        let value_like = match &self.last {
            None => false,
            Some(Tok::Ident(k)) => !matches!(k.as_str(), "else" | "in"),
            Some(
                Tok::Int(_)
                | Tok::Float(_)
                | Tok::Dec(_)
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
        // `|` pipes, `||`, `&&`, `??`, and `? a : b` split over lines.
        if self.starts_with("|") || self.starts_with("&&") || self.starts_with("?") || self.starts_with(":") {
            return false;
        }
        if self.starts_with(".") && !self.starts_with("..") {
            return false;
        }
        if self.word_ahead() == "else" {
            return false;
        }
        // Operators that cannot start a statement, and `- b` or `+ b` with a
        // space (`-b` alone starts a new statement, as in `{ x\n -y }`).
        if ["*", "%", "==", "!="].iter().any(|op| self.starts_with(op)) {
            return false;
        }
        if matches!(self.peek_char(), Some('-' | '+')) && matches!(self.peek_char_at(1), Some(' ' | '\t')) {
            return false;
        }
        self.peek_char().is_some()
    }

    pub fn next_token(&mut self) -> Result<Token, QuilloError> {
        loop {
            self.skip_inline_ws();
            let crossed = self.comment_newline.take();
            if crossed.is_some() || self.peek_char() == Some('\n') {
                let line = crossed.unwrap_or(self.line);
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
        } else if c == '"' || c == '\'' {
            Tok::Str(self.lex_string()?)
        } else if c == '`' {
            Tok::Str(self.lex_template()?)
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
                '|' if n == Some('|') => two(self, Tok::OrOr),
                '|' => Tok::Bar,
                '&' if n == Some('&') => two(self, Tok::AndAnd),
                '?' if n == Some('?') => two(self, Tok::QQ),
                '?' => Tok::Question,
                '~' => Tok::Tilde,
                other => return Err(QuilloError::new(format!("unexpected character `{}`", other), line)),
            }
        };
        self.last = Some(tok.clone());
        Ok(Token { tok, line })
    }

    fn lex_number(&mut self) -> Result<Tok, QuilloError> {
        let line = self.line;
        let mut s = String::new();
        // `0xFF`, `0xFFFF_0000`: masks and flags, for the `bit` module. As with
        // any int, one past 64 bits is an error, not a wrap (FEEDBACK19.md,
        // Q-153; branch feedback19-fixes).
        if self.peek_char() == Some('0') && matches!(self.peek_char_at(1), Some('x' | 'X')) {
            self.bump();
            self.bump();
            while let Some(c) = self.peek_char().filter(|c| c.is_ascii_hexdigit() || *c == '_') {
                self.bump();
                if c != '_' {
                    s.push(c);
                }
            }
            if s.is_empty() {
                return Err(QuilloError::new("expected hex digits after `0x`", line));
            }
            return i64::from_str_radix(&s, 16).map(Tok::Int).map_err(|_| QuilloError::new(format!("number `0x{}` is too large", s), line));
        }
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
        let mut float = false;
        if self.peek_char() == Some('.') && self.peek_char_at(1).map_or(false, |c| c.is_ascii_digit()) {
            s.push('.');
            self.bump();
            digits(self, &mut s);
            float = true;
        }
        // An exponent: 1e9, 2.5e-3, 6E+23.
        if matches!(self.peek_char(), Some('e' | 'E')) {
            let signed = matches!(self.peek_char_at(1), Some('+' | '-'));
            if self.peek_char_at(if signed { 2 } else { 1 }).map_or(false, |c| c.is_ascii_digit()) {
                s.push('e');
                self.bump();
                if signed {
                    s.push(self.bump().unwrap());
                }
                digits(self, &mut s);
                float = true;
            }
        }
        // `12.50m`: a decimal, kept as written (C#'s suffix).
        if self.peek_char() == Some('m') && !self.peek_char_at(1).map_or(false, |c| c.is_alphanumeric() || c == '_') {
            self.bump();
            return crate::decimal::Dec::parse(&s).map(Tok::Dec).ok_or_else(|| {
                QuilloError::new(format!("decimal `{}m` does not fit: at most 28 digits after the point, 29 in all", s), line)
            });
        }
        if float {
            return s.parse::<f64>().map(Tok::Float).map_err(|_| QuilloError::new("bad number", line));
        }
        s.parse::<i64>()
            .map(Tok::Int)
            .map_err(|_| QuilloError::new(format!("number `{}` is too large", s), line))
    }

    fn escape(&mut self, unterminated: &dyn Fn() -> QuilloError) -> Result<char, QuilloError> {
        let e = self.bump().ok_or_else(unterminated)?;
        Ok(match e {
            'n' => '\n',
            't' => '\t',
            'r' => '\r',
            '0' => '\0',
            '"' | '\'' | '`' | '\\' | '{' | '}' | '$' => e,
            'u' => return self.unicode_escape(unterminated),
            other => return Err(QuilloError::new(format!("unknown escape `\\{}` in string", other), self.line)),
        })
    }

    /// `\u{1F600}` (1 to 6 hex digits) or `\u00e9` (exactly 4), after the `u`.
    fn unicode_escape(&mut self, unterminated: &dyn Fn() -> QuilloError) -> Result<char, QuilloError> {
        let line = self.line;
        let bad = || QuilloError::new("bad escape: write `\\u{1F600}` or `\\u00e9`, in hex", line);
        let mut hex = String::new();
        if self.peek_char() == Some('{') {
            self.bump();
            loop {
                match self.bump().ok_or_else(unterminated)? {
                    '}' => break,
                    c if c.is_ascii_hexdigit() && hex.len() < 6 => hex.push(c),
                    _ => return Err(bad()),
                }
            }
        } else {
            for _ in 0..4 {
                match self.bump().ok_or_else(unterminated)? {
                    c if c.is_ascii_hexdigit() => hex.push(c),
                    _ => return Err(bad()),
                }
            }
        }
        let n = u32::from_str_radix(&hex, 16).map_err(|_| bad())?;
        char::from_u32(n).ok_or_else(|| QuilloError::new(format!("`\\u{{{}}}` is not a Unicode character", hex), line))
    }

    /// Lexes a "..." or '...' string, starting at the opening quote. Its
    /// text is taken as it is; `${}` interpolation needs a `template`.
    pub fn lex_string(&mut self) -> Result<Vec<StrPart>, QuilloError> {
        let start = self.line;
        let unterminated = || QuilloError::new("unexpected end of input: unterminated string", start);
        let quote = self.bump().unwrap_or('"');
        let mut buf = String::new();
        loop {
            let c = self.bump().ok_or_else(unterminated)?;
            match c {
                c if c == quote => break,
                '\\' => buf.push(self.escape(&unterminated)?),
                _ => buf.push(c),
            }
        }
        Ok(vec![StrPart::Lit(buf)])
    }

    /// Lexes a `template ${with} interpolation`, starting at the backtick.
    /// `${...}` parts are kept as source and parsed later.
    pub fn lex_template(&mut self) -> Result<Vec<StrPart>, QuilloError> {
        let start = self.line;
        let unterminated = || QuilloError::new("unexpected end of input: unterminated template string", start);
        self.bump();
        let mut parts = Vec::new();
        let mut buf = String::new();
        loop {
            let c = self.bump().ok_or_else(unterminated)?;
            match c {
                '`' => break,
                '\\' => buf.push(self.escape(&unterminated)?),
                '$' if self.peek_char() == Some('{') => {
                    self.bump();
                    if !buf.is_empty() {
                        parts.push(StrPart::Lit(std::mem::take(&mut buf)));
                    }
                    let line = self.line;
                    let open = || QuilloError::new("unexpected end of input: `${` in a template is never closed", line);
                    let mut depth = 1;
                    let mut src = String::new();
                    loop {
                        let c = self.bump().ok_or_else(open)?;
                        match c {
                            '{' => depth += 1,
                            '}' => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            '"' | '\'' | '`' => {
                                // A string nested inside the interpolation.
                                src.push(c);
                                loop {
                                    let d = self.bump().ok_or_else(open)?;
                                    src.push(d);
                                    if d == '\\' {
                                        if let Some(x) = self.bump() {
                                            src.push(x);
                                        }
                                    } else if d == c {
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
                        return Err(QuilloError::new("empty `${}` in template", line));
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
