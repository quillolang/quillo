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
/// What starts a line that takes names from a file: the file comes first, so an editor
/// knows them while they are typed (FEEDBACK19.md, Q-156; branch feedback19-fixes).
pub(crate) const IMPORT_WORDS: &[&str] = &["import", "client", "preload", "preconnect"];

/// How deeply expressions, patterns, types and elements may nest. Deeper
/// source is refused here, so that nothing that walks it later (the
/// checker, the interpreter, freeing it) runs out of stack.
pub const MAX_NESTING: u32 = 256;
const TOO_DEEP: &str = "the code nests too deeply";

pub struct Parser {
    lx: Lexer,
    peeked: Option<Token>,
    /// Inside a `type` declaration.
    in_typedef: bool,
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
    /// The id of the file's own `style`, which its components' elements carry, and how many of
    /// its `style`s and components there are so far (FEEDBACK37.md, Q-457; branch feedback36-39).
    file_style: Option<String>,
    file_styles: u32,
    components: u32,
    /// A `server fn` is being parsed, which may name its callers; and the names it gave.
    callers_ok: bool,
    callers: Option<Vec<String>>,
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
pub(crate) fn starts_declaration(line: &[char]) -> bool {
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
        // `use "lib.qwe"`, `use lib/auth`, `use * from ...`, `use a, b from x.qwer` (branch use-editor).
        "use" => second.starts_with('"') || second == "*" || second.contains(['/', '\\', '.']) || line.contains(" from ") || words.next().is_none(),
        "service" => name(second),
        "import" | "client" | "preload" | "preconnect" => import_line(&line),
        _ => false,
    }
}

/// Whether a line, as written, starts an `import` or one of its browser kin: told by its
/// shape, as these words are names too (FEEDBACK19.md, Q-156; branch feedback19-fixes).
pub(crate) fn import_line(line: &str) -> bool {
    let mut words = line.split_whitespace();
    let (Some(first), Some(second)) = (words.next(), words.next()) else { return false };
    let path = second.starts_with('"') || (second.contains(['/', '\\', '.']) && second.chars().any(char::is_alphanumeric));
    IMPORT_WORDS.contains(&first) && (path || second == "module" || words.any(|w| w == "use"))
}

/// The start of the name of the function a file's `style` is kept in.
pub const FILE_STYLE: &str = "__file_style_";

/// The mark of a file's style: from its path, so the same on the server and in the browser.
fn file_style_id(file: Option<&str>) -> String {
    crate::ast::style_id(file, FILE_STYLE)
}

/// A style in a body that writes no element of its own, only components and views, as
/// `view V() { style global css; <Inner/> }`: nothing on the page carries its mark, which is
/// how a page's sheet is chosen, so its `__style` names it, to be noted as it runs
/// (FEEDBACK28.md, Q-345; branch feedback29-fixes).
fn name_unmarked_styles(body: &mut Expr, styling: Option<Styling>) {
    let Some(Styling { scope: Some(id), marked: 0, .. }) = styling else { return };
    let Expr::Block(stmts) = body else { return };
    for s in stmts {
        if let Stmt::Expr(Expr::Call(f, args, _)) = s {
            if matches!(&**f, Expr::Ident(n, _) if n == "__style") {
                if args.len() == 1 {
                    args.push(Expr::Lit(Lit::Nil));
                }
                args.push(Expr::Lit(Lit::Str(id.as_str().into())));
            }
        }
    }
}

/// A component's or view's own CSS (FEEDBACK8.md, Q-067; branch
/// feedback8-fixes): after `style ...` in its body, each element it writes
/// carries `data-q-<id>`, on the server and in the browser alike, and its
/// rules are made to need it.
struct Styling {
    owner: String,
    may: bool,
    /// A component's body, or a helper's inside one: its elements carry the file's style's mark.
    component: bool,
    params: Vec<String>,
    scope: Option<String>,
    /// Elements written so far: a `style` comes before them.
    els: u32,
    /// Those that carry its mark: not a component's or view's tag.
    marked: u32,
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

/// What a fixed piece of a route's path may hold: letters, digits, `-._~%`, and the others a
/// URL's path takes as they are, `@+!$&` (`/a@b`, `/c++`), where `@` was refused
/// (FEEDBACK53.md, Q-683; branch main). `=`, `,`, `;`, `'`, `(`, `)` and `*` stay out, as the
/// route line gives them meanings of its own.
fn is_path_char(c: char) -> bool {
    c.is_alphanumeric() || "-._~%@+!$&".contains(c)
}

/// What a path written without quotes may have: past its first character, what an
/// address has as well. `:`, `@` and `+`, as in `https://cdn.example/chart@4/chart.js`
/// (branch js-libraries); `?bundle&v=2#x` and `[::1]` (branch library-hints).
pub(crate) fn is_use_path_char(c: char, first: bool) -> bool {
    is_path_char(c) || c == '/' || c == '\\' || (!first && matches!(c, ':' | '@' | '+' | '?' | '=' | '&' | '#' | '[' | ']'))
}

impl Parser {
    pub fn new(src: &str) -> Self {
        Self::with_line(src, 1)
    }

    pub fn with_line(src: &str, line: u32) -> Self {
        Parser { lx: Lexer::new(src, line), peeked: None, in_typedef: false, no_bare_arrow: 0, depth: 0, deepest: 0, ternaries: Default::default(), file: None, styling: None, file_style: None, file_styles: 0, components: 0, callers_ok: false, callers: None, view_next: false, open: Vec::new(), unclosed_at_end: None, top: false, more: Vec::new(),
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

    /// The first of `n` and on that is not a space, on this line.
    fn past_spaces(&self, mut n: usize) -> usize {
        while self.lx.peek_char_at(n).is_some_and(|c| c.is_whitespace() && c != '\n') {
            n += 1;
        }
        n
    }

    /// Whether `word`, whole, is `n` characters on.
    fn word_at(&self, n: usize, word: &str) -> bool {
        word.chars().enumerate().all(|(i, c)| self.lx.peek_char_at(n + i) == Some(c))
            && !self.lx.peek_char_at(n + word.chars().count()).is_some_and(|c| c.is_alphanumeric() || c == '_')
    }

    /// How far on the file starts when a `module` comes first. A file may be called that,
    /// so one `use` follows is the file.
    fn past_module(&self) -> Option<usize> {
        let at = self.past_spaces(0);
        let next = self.past_spaces(at + "module".len());
        (self.word_at(at, "module") && !self.word_at(next, "use")).then_some(next)
    }

    /// Past the first word of a line that takes names: whether a file and then `use` come
    /// next, here or on the next line (FEEDBACK19.md, Q-156; branch feedback19-fixes).
    /// Read as text, as a path is, and the `use` is what tells it from an expression:
    /// `client / 2` and `import(x)` are the names they always were.
    fn at_import(&self) -> bool {
        let at = |n: usize| self.lx.peek_char_at(n);
        let mut n = self.past_module().unwrap_or_else(|| self.past_spaces(0));
        if at(n) == Some('"') {
            n += 1;
            while !matches!(at(n), None | Some('"' | '\n')) {
                n += 1;
            }
            n += 1;
        } else {
            let start = n;
            while at(n).is_some_and(|c| is_use_path_char(c, n == start)) {
                n += 1;
            }
            if n == start {
                return false;
            }
        }
        n = self.past_spaces(n);
        if at(n) == Some('/') && at(n + 1) == Some('/') {
            while !matches!(at(n), None | Some('\n')) {
                n += 1;
            }
        }
        if at(n) == Some('\n') {
            n = self.past_spaces(n + 1);
        }
        self.word_at(n, "use")
    }

    /// What is left of this statement's line, as written.
    fn rest_of_line(&self) -> String {
        (0..).map_while(|i| self.lx.peek_char_at(i).filter(|c| *c != '\n' && *c != ';')).collect()
    }

    /// Whether the rest of the line is names (and `as`), then `from`: read as text, so a
    /// path (`lib\\auth`) is never taken for code (branch imports).
    fn names_then_from(&self) -> bool {
        let line = self.rest_of_line();
        let words: Vec<&str> = line.split(|c: char| c.is_whitespace() || c == ',').filter(|w| !w.is_empty()).collect();
        let Some(at) = words.iter().position(|w| *w == "from") else { return false };
        let ident = |w: &str| w.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') && w.chars().all(|c| c.is_alphanumeric() || c == '_');
        at > 0 && words[..at].iter().all(|w| ident(w))
    }

    /// The file a `use` names: a string, or a path written as it is (`lib/auth.qwe`,
    /// `../shared/auth`, `lib\\auth`, a backslash read as `/`). Without an extension it
    /// is a `.qwe` file, or a `.qwer` one when the `use` takes names (branch imports).
    fn use_path(&mut self, names: bool, line: u32) -> PResult<String> {
        let quoted = self.peeked.is_some() || {
            self.lx.skip_spaces();
            self.lx.peek_char() == Some('"')
        };
        if quoted {
            if let Tok::Str(parts) = self.next()?.tok {
                return lit_string(parts).ok_or_else(|| QuilloError::new("`use` takes a plain path, not a template", line));
            }
            return Err(QuilloError::new("`use` expects a file: use lib/auth.qwe", line));
        }
        let mut p = String::new();
        while let Some(c) = self.lx.peek_char() {
            if is_use_path_char(c, p.is_empty()) {
                p.push(if c == '\\' { '/' } else { c });
                self.lx.bump();
            } else {
                break;
            }
        }
        if p.is_empty() || p.ends_with('/') {
            return Err(QuilloError::new("`use` expects a file: use lib/auth.qwe", line));
        }
        self.lx.last = Some(Tok::RParen);
        if !p.ends_with(".qwe") && !crate::native::is_rust(&p) && !crate::ast::is_script(&p) {
            p.push_str(if names { ".qwer" } else { ".qwe" });
        }
        Ok(p)
    }

    /// What follows `with` in a `use`: `module`, a hint, or both (branch library-hints).
    fn use_with(&mut self, line: u32) -> PResult<LibOptions> {
        let mut options = LibOptions::default();
        loop {
            let word = self.expect_ident("`module`, `preload` or `preconnect` after `with`")?;
            let hint = match word.as_str() {
                "module" => {
                    options.module = true;
                    None
                }
                "preload" => Some(LibHint::Preload),
                "preconnect" => Some(LibHint::Preconnect),
                other => return Err(QuilloError::new(format!("`use ... with` takes `module`, `preload` or `preconnect`, not `{}`", other), line)),
            };
            if hint.is_some() && options.hint.is_some() {
                return Err(QuilloError::new("`use ... with` takes `preload` or `preconnect`, not both: a preload opens the connection itself", line));
            }
            options.hint = options.hint.or(hint);
            if !self.eat(&Tok::Comma)? {
                return Ok(options);
            }
        }
    }

    /// What comes after `use` on a line that starts with the file: `*`, or names.
    fn import_names(&mut self, line: u32) -> PResult<Option<Vec<(String, String)>>> {
        let as_name = "the name to call it after `as`";
        if self.eat(&Tok::Star)? {
            // Not an operator: the line may end here.
            self.lx.last = Some(Tok::RParen);
            // `use *, price` (FEEDBACK45.md, Q-523).
            if self.at(&Tok::Comma)? {
                return Err(QuilloError::new("`*` takes every public name: write `use *` alone, or name each one", line));
            }
            return if self.eat_kw("as")? { Ok(Some(vec![("*".to_string(), self.expect_ident(as_name)?)])) } else { Ok(None) };
        }
        let mut list: Vec<(String, String)> = Vec::new();
        loop {
            let name = self.expect_ident("a name to take after `use`")?;
            let alias = if self.eat_kw("as")? { self.expect_ident(as_name)? } else { name.clone() };
            if list.iter().any(|(_, a)| *a == alias) {
                return Err(QuilloError::new(format!("`{}` is taken twice in this `use`", alias), line));
            }
            list.push((name, alias));
            let comma_line = self.line()?;
            if !self.eat(&Tok::Comma)? {
                return Ok(Some(list));
            }
            // `use price,` at a line's end, then `print(...)`: the list went on into the next
            // statement (FEEDBACK45.md, Q-523). A name on the next line goes on it; a call or an
            // assignment there does not.
            self.eat(&Tok::Newline)?;
            if self.line()? > comma_line {
                let saved = self.save();
                let next_is_name = matches!(self.next()?.tok, Tok::Ident(_))
                    && (matches!(self.peek()?, Tok::Comma | Tok::Newline | Tok::Semi | Tok::Eof) || self.at_kw("as")?);
                self.restore(saved);
                if !next_is_name {
                    return Err(QuilloError::new("this `use` list ends with `,`: drop it, or name what else to take on the next line", line));
                }
            }
        }
    }

    /// `import lib/auth.qwe use login, logout as bye`, and for what runs in the browser
    /// `client /assets/leaflet.js use L`, `preload module "https://..." use Chart` or
    /// `preconnect "https://..." use L`: the first word says how the page gets it, the
    /// file comes before its names so that an editor can offer them, and a long address
    /// may leave its `use` to the next line (FEEDBACK19.md, Q-156; branch feedback19-fixes).
    fn import_stmt(&mut self, word: &str, line: u32) -> PResult<Stmt> {
        self.next()?;
        let module = self.past_module();
        for _ in 0..module.unwrap_or(0) {
            self.lx.bump();
        }
        let module = module.is_some();
        let hint = match word {
            "preload" => Some(LibHint::Preload),
            "preconnect" => Some(LibHint::Preconnect),
            _ => None,
        };
        let path = self.use_path(true, line)?;
        self.eat(&Tok::Newline)?;
        self.next()?;
        let names = self.import_names(line)?;
        if word == "import" && (module || crate::ast::is_script(&path)) {
            return Err(QuilloError::new(format!(
                "`import` is for Quillo and Rust files; what runs in the browser starts with `client`, `preload` or `preconnect`: client \"{}\" use ...", path), line));
        }
        if word != "import" && !crate::ast::is_script(&path) {
            return Err(QuilloError::new(format!(
                "`{}` is for what runs in the browser, a .js or .mjs file or an address; {} is Quillo: import {} use ...", word, path, path), line));
        }
        self.use_stmt(path, names, line, LibOptions { module, hint, old: false })
    }

    /// What every way of taking from a file must agree with, however it was written.
    fn use_stmt(&mut self, path: String, names: Option<Vec<(String, String)>>, line: u32, mut options: LibOptions) -> PResult<Stmt> {
        if let Some(why) = crate::ast::bad_use_path(&path) {
            return Err(QuilloError::new(why, line));
        }
        // A script's names are what it defines on the page (branch js-libraries).
        if crate::ast::is_script(&path) {
            let Some(list) = &names else {
                return Err(QuilloError::new(format!(
                    "a script gives the page names of its own: name what it defines, as in client \"{}\" use Chart", path), line));
            };
            if let Some((n, _)) = list.iter().find(|(n, _)| n.starts_with("__")) {
                return Err(QuilloError::new(format!("`{}`: a name from a script cannot start with `__`", n), line));
            }
            options.module |= crate::ast::is_module_file(&path);
            if let (false, Some((_, alias))) = (options.module, list.iter().find(|(n, _)| n == "*")) {
                return Err(QuilloError::new(format!(
                    "`* as {}` takes a whole ES module: write client module \"{}\" use * as {}", alias, path, alias), line));
            }
            if options.module && list.iter().any(|(n, a)| n == "default" && a == "default") {
                return Err(QuilloError::new(format!(
                    "a module's default export needs a name of yours: client module \"{}\" use default as Name", path), line));
            }
            if options.hint == Some(LibHint::Preconnect) && crate::ast::script_origin(&path).is_none() {
                return Err(QuilloError::new(format!(
                    "`preconnect` is for a script at another address; `{}` is on this site's own connection (`preload` fetches it ahead)", path), line));
            }
            return Ok(Stmt::Use(path, names, line, options));
        }
        if options.module || options.hint.is_some() {
            return Err(QuilloError::new(format!("`with module`, `with preload` and `with preconnect` are for a browser library (a `.js` file or an address), not `{}`", path), line));
        }
        if let Some((_, alias)) = names.iter().flatten().find(|(n, _)| n == "*") {
            return Err(QuilloError::new(format!("`* as {}` takes a whole ES module, not `{}`", alias, path), line));
        }
        Ok(Stmt::Use(path, names, line, options))
    }

    // ---- statements ----------------------------------------------------

    /// What the lexer noted of `${name}` in plain strings (see `Lexer::plain_dollars`).
    pub fn plain_dollars(&self) -> Vec<(String, String)> {
        self.lx.plain_dollars.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().cloned().collect()
    }

    pub fn parse_program(&mut self) -> PResult<Vec<Stmt>> {
        let mut out = Vec::new();
        loop {
            self.skip_seps()?;
            if self.at(&Tok::Eof)? {
                if self.file_style.is_some() && self.components == 0 {
                    return Err(QuilloError::new("a file's `style` styles the components declared in it, and this file has none", self.lx.line));
                }
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
                // What a JavaScript habit gets here (FEEDBACK25.md, Q-324; branch
                // feedback26-fixes).
                if let Some(h) = habit(&other) {
                    return self.err_at(h);
                }
                if let Some(h) = self.word_before().and_then(|w| word_habit(&w)) {
                    return self.err_at(h);
                }
                // `x ? 1` alone, read as `x?` and then `1` (FEEDBACK45.md, Q-520).
                if matches!(other, Tok::Int(_) | Tok::Float(_) | Tok::Str(_) | Tok::Ident(_)) {
                    let text = self.lx.text();
                    let upto = self.lx.pos.min(text.len());
                    let start = text[..upto].iter().rposition(|c| *c == '\n').map_or(0, |i| i + 1);
                    let line: String = text[start..upto].iter().collect();
                    if let Some(q) = line.rfind(" ? ") {
                        if !line[q..].contains(':') && !line.contains("??") {
                            return self.err_at("`?` needs `:` and a value for when it does not hold: `x ? 1 : 0`, or `x && 1`");
                        }
                    }
                }
                // `use lib/m.qwe use price`: names are taken with `import` (Q-520).
                if matches!(&other, Tok::Ident(w) if w == "use") {
                    let text = self.lx.text();
                    let upto = self.lx.pos.min(text.len());
                    let start = text[..upto].iter().rposition(|c| *c == '\n').map_or(0, |i| i + 1);
                    let line: String = text[start..upto].iter().collect();
                    if let Some(path) = line.trim_start().strip_prefix("use ") {
                        let path = path.trim().trim_end_matches("use").trim();
                        return self.err_at(format!("to take names from a file, `import {} use ...`; `use {}` alone takes all its public names", path, path));
                    }
                }
                // `... } elif ...`: the word is what was found.
                if let Some(h) = match &other {
                    Tok::Ident(w) => word_habit(w),
                    _ => None,
                } {
                    return self.err_at(h);
                }
                // `retrun x`, `lett x = 1`, `mtach x {}`: a keyword misspelt (FEEDBACK45.md,
                // Q-520; branch feedback40-43).
                // A word written more than once is a name of the program's (`f 2` with an `fn f`),
                // not a slip (found in review).
                if let Some(w) = self.word_before().filter(|w| !KEYWORDS.contains(&w.as_str()) && self.word_count(w) <= 1) {
                    const STATEMENT: &[&str] = &["let", "fn", "view", "route", "routes", "use", "import", "return", "if", "else", "match", "for", "while", "component", "type", "state", "reply"];
                    let most = if w.chars().count() <= 4 { 1 } else { 2 };
                    if let Some(k) = STATEMENT.iter().copied().filter(|k| *k != w && crate::check::edit_distance(&w, k) <= most).min_by_key(|k| (crate::check::edit_distance(&w, k), *k)) {
                        return self.err_at(format!("`{}` is no keyword: did you mean `{}`?", w, k));
                    }
                }
                match other {
                    Tok::Comma => self.err_at("expected end of statement, found `,`: there is one `let` to a line"),
                    Tok::Colon => self.err_at("expected end of statement, found `:`: a map's key is a name or text, and for one in a variable use `set(m, k, v)`"),
                    _ => self.err_at(format!("expected end of statement, found {}", other.describe())),
                }
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
                        if top && self.styling.is_none() {
                            return self.file_style_stmt(line);
                        }
                        return self.style_stmt(line);
                    }
                    self.restore(saved);
                }
                "let" => {
                    self.next()?;
                    let pat = self.parse_binding()?;
                    // `let x: int = 1`, TypeScript's: a `let` takes its value's type
                    // (FEEDBACK45.md, Q-519; branch feedback40-43).
                    if self.at(&Tok::Colon)? {
                        if let Pattern::Bind(n) = &pat {
                            return self.err_at(format!("a `let` takes no type, but its value's: `let {} = ...`, or `int(...)` to make one", n));
                        }
                    }
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
                        self.callers_ok = true;
                        let def = self.parse_fn_rest(name, line);
                        self.callers_ok = false;
                        let mut def = def?;
                        def.server = true;
                        // `for ["shop"]`: the services it takes calls from (FEEDBACK38.md, Q-458;
                        // branch feedback36-39), noted as the program loads.
                        if let Some(names) = self.callers.take() {
                            let list = Expr::List(names.into_iter().map(|n| Expr::Lit(Lit::Str(n.into()))).collect());
                            let note = Expr::Call(Box::new(Expr::Ident("__callers".into(), line)), vec![Expr::Lit(Lit::Str(def.name.as_str().into())), list], line);
                            self.more.push(Stmt::Expr(note));
                        }
                        return Ok(Stmt::Fn(Arc::new(def)));
                    }
                    self.restore(saved);
                }
                // `accept_calls { keys: ... }`: how calls from other services are taken.
                "accept_calls" if top => {
                    let saved = self.save();
                    self.next()?;
                    if self.at(&Tok::LBrace)? && self.line()? == line {
                        let m = self.parse_expr()?;
                        return Ok(Stmt::Expr(Expr::Call(Box::new(Expr::Ident("accept_calls".into(), line)), vec![m], line)));
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
                    if self.peeked.is_none() {
                        if let Some(msg) = self.group_slip() {
                            return Err(QuilloError::new(msg, line));
                        }
                    }
                    if self.peeked.is_none() && self.group_ahead() {
                        if !top {
                            return Err(QuilloError::new("a `routes` group is written at the top level of a file, as `route` is", line));
                        }
                        let routes = self.parse_group(line, &[], &[], &[], &None)?;
                        if routes.iter().all(|r| r.method == crate::ast::GUARD_METHOD) {
                            return Err(QuilloError::new("this `routes` group has no routes in it", line));
                        }
                        let mut routes = routes.into_iter();
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
                    if top {
                        self.components += 1;
                    }
                    return Ok(Stmt::Fn(Arc::new(self.parse_component(line)?)));
                }
                "type" => {
                    // `type Name = ...`; otherwise `type` is a name.
                    let saved = self.save();
                    self.next()?;
                    if let Tok::Ident(n) = self.peek()? {
                        // `type if = int`: the keyword message, as for `let if` (FEEDBACK45.md,
                        // Q-526; branch feedback40-43).
                        if KEYWORDS.contains(&n.as_str()) && self.lx.text().get(self.lx.pos..).map_or(false, |rest| {
                            let after: String = rest.iter().skip_while(|c| c.is_alphanumeric() || **c == '_').skip_while(|c| **c == ' ' || **c == '\t').take(2).collect();
                            after.starts_with('=') && !after.starts_with("==")
                        }) {
                            return Err(QuilloError::new(format!("`{}` is a keyword, so it cannot be a name here: pick another name", n), line));
                        }
                        let capital = n.chars().next().map_or(false, |c| c.is_ascii_uppercase());
                        let lower_name = n.clone();
                        self.next()?;
                        // `type user = { ... }`: a type's name starts with a capital (Q-520).
                        if !capital && self.at(&Tok::Assign)? {
                            // By characters, not bytes: `type ūser`, `type Éclair` and `type _x` too
                            // (found in review).
                            let mut chars = lower_name.chars();
                            let first = chars.next().unwrap_or('_');
                            let up: String = first.to_uppercase().collect();
                            let msg = if first.is_ascii_lowercase() {
                                format!("a type's name starts with a capital letter: `type {}{} = ...`", up, chars.as_str())
                            } else if first.is_alphabetic() {
                                format!("a type's name starts with a capital letter from A to Z, as `User`: `{}` does not", lower_name)
                            } else {
                                format!("a type's name starts with a capital letter, as `User`: `{}` does not", lower_name)
                            };
                            return Err(QuilloError::new(msg, line));
                        }
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
                // `site shop = "https://shop.example.com"`: a deployed Quillo site, whose server
                // fns are called as `shop.notes()` (FEEDBACK33.md, Q-412; branch
                // feedback27-35-fixes). Two names and `=` are nothing else, so `site` stays a name.
                "site" if top => {
                    let saved = self.save();
                    self.next()?;
                    let next = self.peek()?;
                    let same_line = self.line()? == line;
                    let name = match next {
                        Tok::Ident(n) if same_line && !KEYWORDS.contains(&n.as_str()) => Some(n),
                        _ => None,
                    };
                    if let Some(name) = name {
                        self.next()?;
                        if self.at(&Tok::Assign)? {
                            self.next()?;
                            let url = self.parse_expr()?;
                            let call = Expr::Call(Box::new(Expr::Ident("__site".into(), line)), vec![Expr::Lit(Lit::Str(name.as_str().into())), url], line);
                            return Ok(Stmt::Let(Pattern::Bind(name.as_str().into()), call, line));
                        }
                    }
                    self.restore(saved);
                }
                // `build "/app.css" = tailwind(...)`: made once the port is open, beside the requests,
                // where top-level work held the port shut; a request for it that comes first waits for
                // it (FEEDBACK37.md, Q-454; branch feedback36-39). Otherwise `build` is a name.
                "build" if top => {
                    let saved = self.save();
                    self.next()?;
                    if let (Tok::Str(parts), true) = (self.peek()?, self.line()? == line) {
                        if let Some(path) = lit_string(parts) {
                            self.next()?;
                            if self.at(&Tok::Assign)? {
                                self.next()?;
                                if !path.starts_with('/') || path.contains(['?', '#', ':', '*']) || path.ends_with('/') {
                                    return Err(QuilloError::new(format!("`build` names the path it is served at, like \"/app.css\": got {:?}", path), line));
                                }
                                let made = self.measured(|p| p.parse_expr())?;
                                let f = arrow(Vec::new(), None, made, line, self.file.clone());
                                let body = Expr::Call(Box::new(Expr::Ident("__built".into(), line)), vec![Expr::Lit(Lit::Str(path.as_str().into())), f], line);
                                let segs = path.split('/').filter(|s| !s.is_empty()).map(|s| Seg::Lit(s.to_string())).collect();
                                return Ok(Stmt::Route(Arc::new(RouteDef {
                                    method: "GET".into(),
                                    path: segs,
                                    query: None,
                                    body_pat: None,
                                    guards: Vec::new(),
                                    body,
                                    line,
                                    file: self.file.clone(),
                                    uses_request: false,
                                    effects: None,
                                    limit: None,
                                })));
                            }
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
                    // `use lib/auth.qwe`, `use * from lib/auth`, `use a, b as c from redis.qwer`:
                    // names taken from a `.qwer` file, and paths with or without quotes
                    // (branch imports).
                    let mut names: Option<Vec<(String, String)>> = None;
                    let mut options = LibOptions::default();
                    // Told apart by the next character, so a path is read as written.
                    let next = if self.peeked.is_none() {
                        self.lx.skip_spaces();
                        self.lx.peek_char()
                    } else {
                        None
                    };
                    let path = if next == Some('"') || self.peeked.is_some() {
                        self.use_path(false, line)?
                    } else if next == Some('*') {
                        self.lx.bump();
                        // `use * as THREE with module from ...`: a whole module under
                        // one name (branch library-hints).
                        let whole = self.eat_kw("as")?;
                        if whole {
                            names = Some(vec![("*".to_string(), self.expect_ident("the name to call it after `as`")?)]);
                            if self.eat_kw("with")? {
                                options = self.use_with(line)?;
                            }
                        }
                        if !self.eat_kw("from")? {
                            return Err(QuilloError::new("`use *` takes every name from a file: import lib/auth.qwe use *", line));
                        }
                        options.old = true;
                        self.use_path(whole, line)?
                    } else if !self.names_then_from() {
                        self.use_path(false, line)?
                    } else {
                        // A list of names, then `from`.
                        let saved = self.save();
                        let mut list: Vec<(String, String)> = Vec::new();
                        let mut is_list = false;
                        while let Tok::Ident(name) = self.peek()? {
                            if name == "from" && !list.is_empty() {
                                break;
                            }
                            self.next()?;
                            let alias = if self.eat_kw("as")? { self.expect_ident("the name to call it after `as`")? } else { name.clone() };
                            if list.iter().any(|(_, a)| *a == alias) {
                                return Err(QuilloError::new(format!("`{}` is taken twice in this `use`", alias), line));
                            }
                            list.push((name, alias));
                            if self.eat(&Tok::Comma)? {
                                continue;
                            }
                            // `use Chart with module, preload from ...`, as 0.6 wrote
                            // what is now `preload module` (branch library-hints).
                            if self.eat_kw("with")? {
                                options = self.use_with(line)?;
                            }
                            is_list = self.at_kw("from")?;
                            break;
                        }
                        if is_list {
                            self.next()?;
                            names = Some(list);
                            options.old = true;
                            self.use_path(true, line)?
                        } else {
                            self.restore(saved);
                            self.use_path(false, line)?
                        }
                    };
                    return self.use_stmt(path, names, line, options);
                }
                w if IMPORT_WORDS.contains(&w) => {
                    if self.at_import() {
                        return self.import_stmt(w, line);
                    }
                    // `import { price } from "lib/m.qwe"`, `import price from ...`: JavaScript's
                    // and Python's order (FEEDBACK45.md, Q-520, Q-523; branch feedback40-43).
                    let rest = self.rest_of_line();
                    if w == "import" && (rest.trim_start().starts_with('{') || rest.contains(" from ")) {
                        let file = rest.rsplit(" from ").next().unwrap_or("").trim().trim_matches(|c| c == '"' || c == '\'' || c == ';');
                        let names: String = rest.split(" from ").next().unwrap_or("").trim().trim_matches(|c| c == '{' || c == '}').trim().to_string();
                        return Err(QuilloError::new(format!("the file comes first, then the names: `import {} use {}`", if file.is_empty() { "lib/m.qwe" } else { file }, if names.is_empty() { "price" } else { &names }), line));
                    }
                    // `client /total` is a division, so a path that starts with `/` needs its `use`.
                    if !rest.trim_start().starts_with('/') && import_line(&format!("{} {}", w, rest)) {
                        // `import lib/m.qwe price`: its `use` left out (Q-523).
                        // `preload module a.js`: the word `module` goes with the path.
                        let rest = rest.trim();
                        let (module, rest) = match rest.strip_prefix("module ") {
                            Some(r) => ("module ", r.trim_start()),
                            None => ("", rest),
                        };
                        let mut words = rest.splitn(2, char::is_whitespace);
                        let file = format!("{}{}", module, words.next().unwrap_or(""));
                        let names = words.next().map(str::trim).filter(|n| !n.is_empty()).unwrap_or("...");
                        return Err(QuilloError::new(format!("`{}` takes names from a file, and says which after `use`: {} {} use {}", w, w, file, names), line));
                    }
                }
                "return" => {
                    self.next()?;
                    let e = if can_start_expr(&self.peek()?) { Some(self.parse_expr()?) } else { None };
                    return Ok(Stmt::Return(e, line));
                }
                _ => {}
            }
        }
        let e = self.parse_expr()?;
        self.assign_rest(e, line)
    }

    /// `e`, or the assignment it starts (`n += 1`, `el.value = x`).
    fn assign_rest(&mut self, e: Expr, line: u32) -> PResult<Stmt> {
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
                Expr::List(_) => return Err(QuilloError::new("several names are not assigned at once: `let [c, d] = [b, a + b]`, then assign each", line)),
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
            if self.at(&Tok::DotDot)? {
                return self.err_at("a function takes no `...rest`: take a list");
            }
            if self.at(&Tok::LBrace)? {
                return self.destructured_params(&name);
            }
            let pname = self.expect_ident("a parameter name")?;
            let ty = self.param_type(&pname)?;
            let default = if self.eat(&Tok::Assign)? { Some(self.parse_expr()?) } else { None };
            params.push(Param { name: pname.into(), ty, default });
            if !self.eat(&Tok::Comma)? {
                break;
            }
        }
        self.expect(&Tok::RParen, "`)`")?;
        // `fn f(a: int): int`, TypeScript's and Kotlin's result type (FEEDBACK45.md, Q-519).
        if self.at(&Tok::Colon)? {
            return self.err_at("a function's result type follows `->`: `fn f(a: int) -> int = ...`");
        }
        let ret = if self.eat(&Tok::Arrow)? { Some(self.parse_type()?) } else { None };
        let err = if self.eat(&Tok::Bang)? { Some(self.parse_type()?) } else { None };
        let effects = self.parse_with()?;
        if std::mem::take(&mut self.callers_ok) && self.eat_kw("for")? {
            if !self.eat(&Tok::LBracket)? {
                return self.err_at("after `for`, the services it takes calls from: for [\"shop\", \"orders\"]");
            }
            let mut names = Vec::new();
            while !self.eat(&Tok::RBracket)? {
                match self.next()?.tok {
                    Tok::Str(parts) => match lit_string(parts) {
                        Some(n) if !n.is_empty() => names.push(n),
                        _ => return self.err_at("a caller is named in plain text: for [\"shop\"]"),
                    },
                    other => return self.err_at(format!("a caller is named in plain text, like \"shop\", found {}", other.describe())),
                }
                if !self.eat(&Tok::Comma)? {
                    self.expect(&Tok::RBracket, "`]` after the callers")?;
                    break;
                }
            }
            if names.is_empty() {
                return self.err_at("`for []` takes no caller: leave `for` out, or name the services that may call it");
            }
            self.callers = Some(names);
        }
        let may = std::mem::take(&mut self.view_next);
        // A helper written inside a styled component or view is part of it.
        let scope = if may { None } else { self.styling.as_ref().and_then(|s| s.scope.clone()) };
        let component = !may && self.styling.as_ref().is_some_and(|s| s.component);
        let styling = Styling { owner: name.clone(), may, component, params: params.iter().map(|p| p.name.to_string()).collect(), scope, els: 0, marked: 0 };
        let outer = self.styling.replace(styling);
        let body = if self.eat(&Tok::Assign)? {
            self.measured(Self::parse_arrow_body)
        } else if self.at(&Tok::LBrace)? {
            self.measured(Self::parse_block)
        } else {
            self.styling = outer;
            return self.err_at(format!("expected `=` or `{{` to start the body of `{}`", name));
        };
        let ours = std::mem::replace(&mut self.styling, outer);
        let (mut body, nesting) = body?;
        name_unmarked_styles(&mut body, ours);
        Ok(FnDef { name, params, ret, err, effects, server: false, component: false, body, line, nesting, file: self.file.clone(), hot: Default::default() })
    }

    /// `style <css>` at a file's top level: the CSS of the components the file declares, and no
    /// one else's: their elements carry the file's mark too, and its rules need it, sent once for
    /// the file where a CSS value given to each component's `style` was sent once for each
    /// (FEEDBACK37.md, Q-457; branch feedback36-39). A function of its own holds it, which the
    /// program's styles are taken from as a component's are.
    fn file_style_stmt(&mut self, line: u32) -> PResult<Stmt> {
        if self.components > 0 {
            return Err(QuilloError::new("a file's `style` comes before its components, so that its rules are written where they are read", line));
        }
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
        let id = file_style_id(self.file.as_deref());
        self.file_style = Some(id.clone());
        self.file_styles += 1;
        let name = format!("{}{}_{:04}", FILE_STYLE, id, self.file_styles);
        let mut args = vec![e, Expr::Lit(Lit::Bool(global)), Expr::Lit(Lit::Str(id.as_str().into()))];
        if !global {
            args[1] = Expr::Lit(Lit::Nil);
        }
        let body = Expr::Block(vec![Stmt::Expr(Expr::Call(Box::new(Expr::Ident("__style".into(), line)), args, line))]);
        Ok(Stmt::Fn(Arc::new(FnDef { name, params: Vec::new(), ret: None, err: None, effects: None, server: false, component: false, body, line, nesting: 1, file: self.file.clone(), hot: Default::default() })))
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
                if read.is_none() && params.contains(&n.to_string()) {
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
            if self.at(&Tok::DotDot)? {
                return self.err_at("a function takes no `...rest`: take a list");
            }
            if self.at(&Tok::LBrace)? {
                return self.destructured_params(&name);
            }
            let pname = self.expect_ident("a parameter name")?;
            let ty = if self.eat(&Tok::Colon)? { Some(self.parse_type()?) } else { None };
            let default = if self.eat(&Tok::Assign)? { Some(self.parse_expr()?) } else { None };
            params.push(Param { name: pname.into(), ty, default });
            if !self.eat(&Tok::Comma)? {
                break;
            }
        }
        self.expect(&Tok::RParen, "`)`")?;
        let styling = Styling { owner: name.clone(), may: true, component: true, params: params.iter().map(|p| p.name.to_string()).collect(), scope: None, els: 0, marked: 0 };
        let outer = self.styling.replace(styling);
        let body = self.measured(|p| p.component_body(&name));
        let ours = std::mem::replace(&mut self.styling, outer);
        let (mut body, nesting) = body?;
        name_unmarked_styles(&mut body, ours);
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
            // `state n: int = 0`: a state takes the type of its value (FEEDBACK45.md, Q-519).
            if self.at(&Tok::Colon)? {
                return Err(QuilloError::new(format!("a `state` takes no type, but its value's: `state {} = 0`", n), line));
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
                // In a `type`, this `>` ends the line: it is no comparison, to go on
                // past it (FEEDBACK16.md, Q-136; branch feedback20-fixes). Elsewhere a
                // body may stand on the next line, as it always could after `>`.
                if self.in_typedef {
                    self.lx.last = Some(Tok::RBracket);
                }
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
                // `integer`, `string`, `ayn`: the type meant, when it is plain (FEEDBACK45.md,
                // Q-517; branch feedback40-43).
                let meant = match name.as_str() {
                    "integer" | "long" | "i64" | "i32" | "number" => Some("int"),
                    "string" | "text" | "String" => Some("str"),
                    "boolean" | "Boolean" => Some("bool"),
                    "double" | "f64" | "f32" | "real" => Some("float"),
                    "decimal" => Some("dec"),
                    "array" | "vec" | "Vec" => Some("list"),
                    "dict" | "object" | "record" | "hash" => Some("map"),
                    "function" | "func" | "lambda" => Some("fn"),
                    _ => TYPE_NAMES.iter().copied().filter(|t| crate::check::edit_distance(&name, t) <= 2 && crate::check::edit_distance(&name, t) < name.len()).min_by_key(|t| (crate::check::edit_distance(&name, t), *t)),
                };
                return Err(QuilloError::new(
                    match meant {
                        Some(m) => format!("unknown type `{}`: did you mean `{}`?", name, m),
                        None => format!("unknown type `{}` (known types: {}, null, and the program's own, which start with a capital letter)", name, TYPE_NAMES.join(", ")),
                    },
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
        self.in_typedef = true;
        let r = self.typedef_body(name, line);
        self.in_typedef = false;
        r
    }

    fn typedef_body(&mut self, name: String, line: u32) -> PResult<TypeDef> {
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
            // `{ name: str, name: int }`: the second won (FEEDBACK45.md, Q-508; branch
            // feedback40-43).
            if fields.iter().any(|(f, _)| *f == key) {
                return Err(QuilloError::new(format!("field `{}` is named twice in this type", key), t.line));
            }
            self.expect(&Tok::Colon, &format!("`:` and a type after `{}`", key))?;
            let ty = match self.parse_type() {
                Ok(t) => t,
                // `{ str: name }`: the name and type swapped (FEEDBACK45.md, Q-519).
                Err(e) if TYPE_NAMES.contains(&key.as_str()) && e.msg.starts_with("unknown type") => {
                    let written = e.msg.split('`').nth(1).unwrap_or("x").to_string();
                    return Err(QuilloError::new(format!("`{0}: {1}` has the name and type swapped: write `{1}: {0}`", key, written), e.line));
                }
                Err(e) => return Err(e),
            };
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

    /// `with db, http`, if it is there: the effects a function, a route or a group may use.
    fn parse_with(&mut self) -> PResult<Option<Vec<String>>> {
        if !self.eat_kw("with")? {
            return Ok(None);
        }
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
        Ok(Some(effs))
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
            // `/a/{id}` or `/a/{id:int}`, as OpenAPI, Express 5, ASP.NET and FastAPI write a
            // parameter: read as the path `/a/` and a request pattern, it matched nothing
            // (FEEDBACK45.md, Q-506; branch feedback40-43).
            // Right after the root `/`, `{json}` is a request pattern, as it always was
            // (found in review).
            // Not `id` or `path`: there, `/{id}` is a parameter meant (found in review).
            const REQUEST: &[&str] = &["method", "query", "query_string", "params", "headers", "cookies", "body", "json", "form", "files", "ip"];
            if let Some((name, ty)) = self.braced_param().filter(|(n, _)| !(path.is_empty() && REQUEST.contains(&n.as_str()))) {
                let typed = ty.as_ref().map_or(String::new(), |t| format!("({})", t));
                let written = ty.as_ref().map_or(name.clone(), |t| format!("{}:{}", name, t));
                return Err(QuilloError::new(format!("path parameters are written `:{0}{1}`, as in `/users/:{0}{1}`, not `{{{2}}}`", name, typed, written), self.lx.line));
            }
            // `/@:name`, `/~:name`: a parameter after a prefix of `@` or `~`, as profiles'
            // addresses are written (FEEDBACK53.md, Q-683; branch main).
            let mut prefix = String::new();
            let mut k = 0;
            while let Some(c @ ('@' | '~')) = self.lx.peek_char_at(k) {
                prefix.push(c);
                k += 1;
            }
            if !prefix.is_empty() && self.lx.peek_char_at(k) == Some(':') {
                for _ in 0..k {
                    self.lx.bump();
                }
            } else {
                prefix.clear();
            }
            match self.lx.peek_char() {
                Some(':') => {
                    self.lx.bump();
                    let name = self.read_word();
                    if name.is_empty() {
                        return Err(QuilloError::new("expected a parameter name after `:` in the path", self.lx.line));
                    }
                    // `/:if`, `/:1`: a name the route's body can never read (Q-546).
                    if KEYWORDS.contains(&name.as_str()) || name.starts_with(|c: char| c.is_ascii_digit()) {
                        return Err(QuilloError::new(format!("`:{}` is no name the route can read: a parameter is named as a variable is, `/users/:id`", name), self.lx.line));
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
                    path.push(Seg::Param(name, ty, prefix));
                }
                Some('*') => {
                    self.lx.bump();
                    let name = self.read_word();
                    if KEYWORDS.contains(&name.as_str()) || name.starts_with(|c: char| c.is_ascii_digit()) {
                        return Err(QuilloError::new(format!("`*{}` is no name the route can read: the rest is named as a variable is, `/files/*path`", name), self.lx.line));
                    }
                    let name = if name.is_empty() { "rest".to_string() } else { name };
                    // `/f/*rest/more`: what came after the rest was never compared (FEEDBACK47.md,
                    // Q-540; branch feedback40-43).
                    if self.lx.peek_char() == Some('/') {
                        return Err(QuilloError::new(format!("`*{}` takes the rest of the path, so it comes last: nothing can follow it", name), self.lx.line));
                    }
                    path.push(Seg::Rest(name));
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
            // `/a/:id(int)x`, `/a:b`, `/a+b`: a character the path cannot hold, where the route
            // line went on to say it expected its body (Q-546).
            if let Some(c) = self.lx.peek_char() {
                if !c.is_whitespace() && !matches!(c, '/' | '{' | '=' | '?' | '|' | ')' | ',' | ';') {
                    let hint = if c == ':' { ": a parameter is a piece of its own, `/a/:b`" } else { "" };
                    return Err(QuilloError::new(format!("a path cannot hold `{}` here{}", c, hint), self.lx.line));
                }
            }
        }
        self.lx.last = Some(Tok::RParen);
        Ok(path)
    }

    /// A parameter's `: type`, if it has one, and what other languages' forms of it mean
    /// (FEEDBACK45.md, Q-519; branch feedback40-43): `int: a` swapped, `a int` (Go), `int a` (C,
    /// Java), `a?: int` and `a: ?int`, `int[]` and `list[int]`.
    fn param_type(&mut self, pname: &str) -> PResult<Option<Ty>> {
        let typed = |n: &str| TYPE_NAMES.contains(&n);
        match self.peek()?.clone() {
            Tok::Ident(t) if typed(&t) => {
                return self.err_at(format!("a parameter's type follows its name after `:`: `{}: {}`", pname, t));
            }
            Tok::Ident(n) if typed(pname) => {
                return self.err_at(format!("a parameter's type follows its name after `:`: `{}: {}`", n, pname));
            }
            Tok::Question => {
                return self.err_at(format!("`?` goes after the type, for a value that may be null: `{}: int?`; for one that may be left out, give it a default, `{} = 0`", pname, pname));
            }
            Tok::Colon => {}
            _ => return Ok(None),
        }
        self.next()?;
        if self.at(&Tok::Question)? {
            return self.err_at(format!("`?` goes after the type: `{}: int?`", pname));
        }
        let ty = match self.parse_type() {
            Ok(t) => t,
            // `int: a`: the name is a type's, the type is no type.
            Err(e) if typed(pname) && e.msg.starts_with("unknown type") => {
                let written = e.msg.split('`').nth(1).unwrap_or("x").to_string();
                return Err(QuilloError::new(format!("`{0}: {1}` has the name and type swapped: write `{1}: {0}`", pname, written), e.line));
            }
            Err(e) => return Err(e),
        };
        if self.at(&Tok::LBracket)? {
            let elem = ty.names.first().cloned().unwrap_or_default();
            let elem = if elem == "list" { "int".to_string() } else { elem };
            return self.err_at(format!("a list's item type goes in `<>`: `{}: list<{}>`", pname, elem));
        }
        Ok(Some(ty))
    }

    /// `{id}` or `{id:int}` just after a path's `/`: its name and type. Not a request pattern
    /// (`{ json: ... }`), which has spaces or more than a name.
    fn braced_param(&self) -> Option<(String, Option<String>)> {
        if self.lx.peek_char() != Some('{') {
            return None;
        }
        let word = |from: usize| -> (String, usize) {
            let mut w = String::new();
            let mut n = from;
            while let Some(c) = self.lx.peek_char_at(n).filter(|c| c.is_alphanumeric() || *c == '_') {
                w.push(c);
                n += 1;
            }
            (w, n)
        };
        let (name, mut n) = word(1);
        if name.is_empty() {
            return None;
        }
        let mut ty = None;
        if self.lx.peek_char_at(n) == Some(':') {
            let (t, m) = word(n + 1);
            if t.is_empty() {
                return None;
            }
            ty = Some(t);
            n = m;
        }
        (self.lx.peek_char_at(n) == Some('}')).then_some((name, ty))
    }

    fn parse_route_rest(&mut self, method: String, path: Vec<Seg>, line: u32) -> PResult<RouteDef> {
        // Mistakes on the route line, each said for what it is (FEEDBACK47.md, Q-546; branch
        // feedback40-43).
        self.lx.skip_spaces();
        if self.peeked.is_none() && self.lx.peek_char() == Some('/') {
            return self.err_at("a path has no spaces: write it in one piece, `/a/b`");
        }
        if self.at(&Tok::Bar)? {
            return self.err_at("a route has no middleware of its own: put it in a group, `routes /x | g { route ... }`, or call it in the body, `request | g`");
        }
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
                Ok(_) => matches!(self.peek(), Ok(Tok::LBrace) | Ok(Tok::Assign)) || matches!(self.peek(), Ok(Tok::Ident(k)) if k == "with"),
                Err(_) => false,
            };
            if let (Ok(Pattern::Record(fields)), false) = (&res, is_pat) {
                if self.at(&Tok::Question)? {
                    return self.err_at("the query pattern goes before the request pattern: `route POST /x ? { q: str } { json: j } = ...`");
                }
                // Only when the braces are no body either: `{ "hi" }`, `{ n }` and `{ request }`
                // are blocks (found in review).
                let ends = matches!(self.peek(), Ok(Tok::Newline | Tok::Eof | Tok::RBrace));
                let is_block = ends && {
                    let after = self.save();
                    self.restore(saved.clone());
                    let ok = self.parse_block().is_ok();
                    self.restore(after);
                    ok
                };
                if ends && !is_block {
                    const REQUEST: &[&str] = &["json", "form", "headers", "query", "cookies", "files", "body", "method", "path", "params", "ip", "host"];
                    return if !fields.is_empty() && fields.iter().all(|f| REQUEST.contains(&f.path[0].as_str())) {
                        Err(QuilloError::new("this request pattern has no body after it: `= ...` or `{ ... }`", line))
                    } else {
                        Err(QuilloError::new("a route's `{` starts its body, or its request pattern: to answer this map, write `= { ... }`", line))
                    };
                }
            }
            if is_pat {
                body_pat = res.ok();
            } else {
                self.restore(saved);
            }
        }
        if let Some(name) = body_pat.as_ref().and_then(bare_alternative) {
            return Err(QuilloError::new(format!(
                "`| {}` after a request pattern is another pattern, one that takes any request, so the route always runs: a route has no middleware of its own; put it in a group, `routes /x | {} {{ route ... }}`, or call it in the body",
                name, name
            ), line));
        }

        // `with db`: what the route may do, as for a function (branch feedback40-43).
        let effects = self.parse_with()?;
        // `route POST /x with db { json: j } = j`: the pattern was read as the body (Q-546).
        if effects.is_some() && body_pat.is_none() && self.at(&Tok::LBrace)? {
            let saved = self.save();
            let pat = self.parse_pattern();
            let pattern_then_body = pat.is_ok() && matches!(self.peek(), Ok(Tok::LBrace | Tok::Assign));
            self.restore(saved);
            if pattern_then_body {
                return self.err_at("the request pattern goes before `with`: `route POST /x { json: j } with db = ...`");
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
        Ok(RouteDef { method, path, query, body_pat, guards: Vec::new(), body, line, file: self.file.clone(), uses_request, effects, limit: None }.limited())
    }

    /// Whether what follows `routes` is a group's path: `/` and a path
    /// (`/admin`, `/:team`), or `/` alone before `|`, `{` or `with` (a group at
    /// the root). `routes / 2` is a division (branch route-groups).
    /// `routes GET /a { ... }` and `routes admin { ... }`: what a group is not, said as such
    /// rather than as a misspelt `route` (FEEDBACK47.md, Q-546; branch feedback40-43).
    fn group_slip(&self) -> Option<String> {
        let mut n = 0;
        let mut word = String::new();
        while let Some(c) = self.lx.peek_char_at(n) {
            if !(c.is_alphanumeric() || c == '_') {
                break;
            }
            word.push(c);
            n += 1;
        }
        if word.is_empty() {
            return None;
        }
        while matches!(self.lx.peek_char_at(n), Some(' ' | '\t')) {
            n += 1;
        }
        let next = self.lx.peek_char_at(n);
        if METHODS.contains(&word.as_str()) && next == Some('/') {
            return Some(format!("a group takes no method, each of its routes has one: `routes /... {{ route {} /... = ... }}`", word));
        }
        if matches!(next, Some('{' | '|')) && !KEYWORDS.contains(&word.as_str()) {
            return Some(format!("a group's path starts with `/`: `routes /{} {{ ... }}`", word));
        }
        None
    }

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
                // `routes / with db { ... }`: `with` and a space (branch feedback40-43).
                let with = "with".chars().enumerate().all(|(i, c)| self.lx.peek_char_at(n + i) == Some(c))
                    && matches!(self.lx.peek_char_at(n + 4), Some(' ' | '\t'));
                with || matches!(self.lx.peek_char_at(n), Some('|' | '{'))
            }
        }
    }

    /// `routes /prefix | middleware ... { request pattern } { routes }`, the
    /// prefix and the rest added to those of the groups around it: its routes,
    /// each with the prefix before its path, the middleware called before its
    /// body (`__answer(m(request))`: a `reply` in it, or a response it gives,
    /// answers instead), and the pattern matched with its own (branch
    /// route-groups).
    fn parse_group(&mut self, line: u32, prefix: &[Seg], before: &[(Expr, u32)], guards: &[Pattern], outer: &Option<Vec<String>>) -> PResult<Vec<RouteDef>> {
        let mut path = prefix.to_vec();
        path.extend(self.parse_path("after `routes`")?);
        let mut before = before.to_vec();
        while self.eat(&Tok::Bar)? {
            // Its call is told of at the line it is written on.
            let at = self.line()?;
            before.push((self.parse_or()?, at));
        }
        // `routes /admin | signed_in with db { ... }`: what every route in it, and its
        // middleware, may do; a group inside narrows it further (branch feedback40-43).
        let with_line = self.line()?;
        let effects = match (outer, self.parse_with()?) {
            (None, mine) => mine,
            (Some(o), None) => Some(o.clone()),
            (Some(o), Some(mine)) => {
                if let Some(e) = mine.iter().find(|e| !o.contains(e)) {
                    return Err(QuilloError::new(format!("this group asks for `{}`, which the group around it does not allow (`with {}`)", e, o.join(", ")), with_line));
                }
                Some(mine)
            }
        };
        if self.at(&Tok::Bar)? {
            return self.err_at("a group's middleware goes before `with`: `routes /admin | signed_in with db { ... }`");
        }
        if self.at(&Tok::Question)? {
            return self.err_at("a group takes no query pattern: put `? { ... }` on each route that reads it");
        }
        let mut guards = guards.to_vec();
        if self.at(&Tok::LBrace)? {
            let saved = self.save();
            match self.parse_pattern() {
                Ok(p) if self.at(&Tok::LBrace)? => {
                    // `routes /api { headers: ... } | g { ... }`: `| g` was an alternative that
                    // takes anything, so neither the pattern nor `g` ever refused a request
                    // (FEEDBACK47.md, Q-536; branch feedback40-43).
                    if let Some(name) = bare_alternative(&p) {
                        return Err(QuilloError::new(format!(
                            "`| {}` after a request pattern is another pattern, one that takes any request: a group's middleware goes before the pattern, `routes {} | {} {{ headers: ... }} {{ ... }}`",
                            name, shown_path(&path), name
                        ), line));
                    }
                    guards.push(p)
                }
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
                    // `routes /docs/*path { route GET /edit ... }`: the group's rest took every
                    // path under it, so its first route answered them all (Q-540).
                    if let (Some(Seg::Rest(name)), false) = (path.last(), r.path.is_empty()) {
                        return Err(QuilloError::new(format!(
                            "`*{0}` takes the rest of the path, so it comes last: a route in this group can add nothing after it; give the group no rest, and each route its own (`route GET /edit/*{0}`)",
                            name
                        ), at));
                    }
                    let mut full = path.clone();
                    full.append(&mut r.path);
                    r.path = full;
                    // `routes /t/:team(int) | member_of(team) { route POST /move { json: { team: int } } }`:
                    // the body's `team` took the path's place in the middleware's call, so a
                    // request chose whose team it was checked against (found in review).
                    let mut bound = Vec::new();
                    for p in r.query.iter().chain(r.body_pat.iter()).chain(r.guards.iter()) {
                        crate::check::pattern_names(p, &mut bound);
                    }
                    for (m, _) in &before {
                        let mut read = Vec::new();
                        crate::check::walk_expr(m, &mut |x| {
                            if let Expr::Ident(n, _) = x {
                                read.push(n.to_string());
                            }
                        });
                        if let Some(n) = bound.iter().find(|n| read.contains(n)) {
                            return Err(QuilloError::new(format!(
                                "this route's pattern binds `{0}`, which the group's middleware reads: it would be called with the request's `{0}`, not the path's; name the field otherwise (`{{ json: {{ {0}: other }} }}`)",
                                n
                            ), at));
                        }
                    }
                    let mut gs = guards.clone();
                    gs.append(&mut r.guards);
                    r.guards = gs;
                    if !before.is_empty() {
                        let mut stmts = guard_calls(&before);
                        stmts.push(Stmt::Expr(std::mem::replace(&mut r.body, Expr::Lit(Lit::Nil))));
                        r.body = Expr::Block(stmts);
                    }
                    r.uses_request = r.uses_request || !before.is_empty() || !r.guards.is_empty();
                    if let Some(g) = &effects {
                        match &r.effects {
                            Some(own) => {
                                if let Some(e) = own.iter().find(|e| !g.contains(e)) {
                                    return Err(QuilloError::new(format!("this route asks for `{}`, which its group does not allow (`with {}`)", e, g.join(", ")), at));
                                }
                            }
                            None => r.effects = Some(g.clone()),
                        }
                    }
                    out.push(r.limited());
                }
                Tok::Ident(k) if k == "routes" => {
                    self.next()?;
                    let inner = self.parse_group(at, &path, &before, &guards, &effects)?;
                    // As at the top level (Q-546): a group inside another with nothing in it.
                    if inner.iter().all(|r| r.method == crate::ast::GUARD_METHOD) {
                        return Err(QuilloError::new("this `routes` group has no routes in it", at));
                    }
                    out.extend(inner);
                }
                other => return self.err_at(format!("a `routes` group holds `route` and `routes`, found {}", other.describe())),
            }
            self.end_stmt()?;
        }
        // Its middleware for what no route of it answers (Q-539): after the routes of groups
        // inside it, whose own guards are nearer.
        if !before.is_empty() {
            let mut guard_path = path.clone();
            if !matches!(guard_path.last(), Some(Seg::Rest(_))) {
                guard_path.push(Seg::Rest("__rest".into()));
            }
            let mut body = guard_calls(&before);
            body.push(Stmt::Expr(Expr::Lit(Lit::Nil)));
            out.push(
                RouteDef {
                    method: crate::ast::GUARD_METHOD.to_string(),
                    path: guard_path,
                    query: None,
                    body_pat: None,
                    // The group's request pattern (a host's) says which requests are under it.
                    guards: guards.clone(),
                    body: Expr::Block(body),
                    line,
                    file: self.file.clone(),
                    uses_request: true,
                    effects: effects.clone(),
                    limit: None,
                }
                .limited(),
            );
        }
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
        // An arrow parameter's type error, told from a parse that only shows this is no arrow.
        const ARROW_TYPE: &str = "\u{1}arrow type: ";
        let saved = self.save();
        let head = (|| -> PResult<Option<(Vec<Param>, Option<Ty>)>> {
            let mut params = Vec::new();
            while !self.at(&Tok::RParen)? {
                let Tok::Ident(name) = self.peek()? else { return Ok(None) };
                if KEYWORDS.contains(&name.as_str()) {
                    return Ok(None);
                }
                self.next()?;
                // `(a: string) => a`: the type's error, not a parenthesis's; `name :` in a
                // parenthesis is nothing but an arrow's (FEEDBACK45.md, Q-519).
                let ty = if self.eat(&Tok::Colon)? {
                    match self.parse_type() {
                        Ok(t) => Some(t),
                        Err(e) if e.msg.starts_with("unknown type") && TYPE_NAMES.contains(&name.as_str()) => {
                            let written = e.msg.split('`').nth(1).unwrap_or("x").to_string();
                            return Err(QuilloError::new(format!("{}`{}: {}` has the name and type swapped: write `{}: {}`", ARROW_TYPE, name, written, written, name), e.line));
                        }
                        Err(e) if e.msg.starts_with("unknown type") => return Err(QuilloError::new(format!("{}{}", ARROW_TYPE, e.msg), e.line)),
                        Err(e) => return Err(e),
                    }
                } else {
                    None
                };
                let default = if self.eat(&Tok::Assign)? { Some(self.parse_expr()?) } else { None };
                params.push(Param { name: name.into(), ty, default });
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
            Err(e) if e.msg.starts_with(ARROW_TYPE) => Err(QuilloError::new(e.msg.trim_start_matches(ARROW_TYPE).to_string(), e.line)),
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
        // `() => n += 1`, `() => count = 0`: the assignment is the body, as in `() => { n += 1 }`,
        // and the arrow gives null (FEEDBACK45.md, Q-518; branch feedback40-43).
        let line = self.line()?;
        let e = self.parse_expr()?;
        if matches!(self.peek()?, Tok::Assign | Tok::PlusAssign | Tok::MinusAssign) {
            return Ok(Expr::Block(vec![self.assign_rest(e, line)?]));
        }
        Ok(e)
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
            // `x ? 1` alone (FEEDBACK45.md, Q-520).
            if !self.at(&Tok::Colon)? {
                return self.err_at("`?` needs `:` and a value for when it does not hold: `x ? 1 : 0`, or `x && 1`");
            }
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
        // `view V() = <p>a</p><p>b</p>`: the second element read as `<` (FEEDBACK45.md, Q-521;
        // branch feedback40-43).
        if matches!(lhs, Expr::Html(..)) && matches!(self.peek()?, Tok::Lt) && self.lx.peek_char().map_or(false, |c| c.is_alphabetic()) {
            return self.err_at("two elements side by side: a view gives one, so wrap them in one, like `<div>...</div>`");
        }
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
            if op == BinOp::Mul && self.at(&Tok::Star)? {
                return self.err_at("there is no `**`: `pow(x, 2)`");
            }
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
                            if p.at(&Tok::DotDot)? {
                                return p.err_at("a spread goes in a list or a map, not in a call: pass the list");
                            }
                            args.push(p.parse_expr()?);
                            if !p.eat(&Tok::Comma)? {
                                break;
                            }
                        }
                        if p.at(&Tok::FatArrow)? {
                            return p.err_at("a function's parameters are plain names, not patterns: `(p) => p[0] + p[1]`");
                        }
                        if p.at(&Tok::Colon)? {
                            // With the names written (FEEDBACK45.md, Q-525).
                            let f = match &e { Expr::Ident(n, _) => n.to_string(), _ => "f".to_string() };
                            let arg = match args.last() { Some(Expr::Ident(n, _)) => n.to_string(), _ => "c".to_string() };
                            return p.err_at(format!("there are no named arguments: give the value alone, `{}(...)`, or pass a map with the names, `{}({{ {}: ... }})`", f, f, arg));
                        }
                        let next = p.peek()?.clone();
                        if let Some(h) = habit(&next) {
                            return p.err_at(h);
                        }
                        // `print(typeof x)`: the word before what was found (Q-520).
                        if let (Some(Expr::Ident(w, _)), false) = (args.last(), matches!(next, Tok::RParen)) {
                            if let Some(h) = word_habit(w) {
                                return p.err_at(h);
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
                    if self.at(&Tok::Colon)? {
                        return self.err_at("there are no slices with `:`, and no step: `slice(xs, a, b)`, and `filter` over `enumerate` for every second one");
                    }
                    let i = self.no_bare_arrow_off(|p| p.parse_expr())?;
                    if self.at(&Tok::Colon)? {
                        return self.err_at("there are no slices with `:`: `slice(xs, a, b)`");
                    }
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
                } else if self.peeked.is_none() && self.doctype_len() > 0 {
                    // `<!DOCTYPE html>` before `<html>`, as an HTML file has it: an <html> gets
                    // one by itself, so it is passed over (FEEDBACK36.md, Q-449; branch feedback36-39).
                    for _ in 0..self.doctype_len() {
                        self.lx.bump();
                    }
                    self.lx.skip_all_ws();
                    let html: String = (1..6).map_while(|i| self.lx.peek_char_at(i)).collect();
                    if self.lx.peek_char() != Some('<') || !html.to_ascii_lowercase().starts_with("html") || html.chars().nth(4).is_some_and(|c| c.is_alphanumeric()) {
                        return Err(QuilloError::new("`<!DOCTYPE html>` goes before an <html> element, which gets one by itself", line));
                    }
                    self.lx.bump();
                    self.parse_html(line)
                } else {
                    let fragment = self.peeked.is_none() && self.lx.peek_char() == Some('>');
                    Err(QuilloError::new(if fragment { FRAGMENTS } else { "expected an expression, found `<`" }, line))
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
                // A reply's status may be a reply: it nests, and counts as nesting
                // (FEEDBACK16.md, Q-133; branch feedback20-fixes).
                "reply" => self.nested(|p| p.parse_reply(line)),
                // `?? return null`, `pat => return x`: leaves the function, as
                // the statement does.
                "return" => {
                    let e = if can_start_expr(&self.peek()?) { Some(self.parse_expr()?) } else { None };
                    Ok(Expr::Block(vec![Stmt::Return(e, line)]))
                }
                kw if KEYWORDS.contains(&kw) => Err(QuilloError::new(format!("unexpected keyword `{}`", kw), line)),
                _ if self.peeked.is_none() && self.lx.peek_char() == Some('`') => {
                    // A tagged template: css`...` is css(`...`).
                    let parts = self.lx.lex_template()?;
                    self.lx.last = Some(Tok::RParen);
                    let t = self.template(parts)?;
                    Ok(Expr::Call(Box::new(Expr::Ident(k.clone().into(), line)), vec![t], line))
                }
                _ => {
                    // `x => body`: an arrow function with one parameter.
                    if self.no_bare_arrow == 0 && self.at(&Tok::FatArrow)? {
                        self.next()?;
                        let body = self.measured(Self::parse_arrow_body)?;
                        let params = vec![Param { name: k.clone().into(), ty: None, default: None }];
                        return Ok(arrow(params, None, body, line, self.file.clone()));
                    }
                    Ok(Expr::Ident(k.clone().into(), line))
                }
            },
            other => Err(QuilloError::new(format!("expected an expression, found {}", other.describe()), line)),
        }
    }

    /// After `{` in expression position: a map if it is empty or starts with
    /// The word just before the next token, for what an error can say of it.
    /// How many times `w` is written in the file as a whole word.
    fn word_count(&self, w: &str) -> usize {
        let text = self.lx.text();
        let want: Vec<char> = w.chars().collect();
        let word = |c: char| c.is_alphanumeric() || c == '_';
        (0..text.len().saturating_sub(want.len() - 1))
            .filter(|&i| text[i..i + want.len()] == want[..] && (i == 0 || !word(text[i - 1])) && text.get(i + want.len()).map_or(true, |c| !word(*c)))
            .count()
    }

    fn word_before(&self) -> Option<String> {
        let text = self.lx.text();
        let upto = self.lx.pos.min(text.len());
        let line: Vec<char> = text[..upto].iter().rev().copied().skip_while(|c| !c.is_whitespace()).skip_while(|c| c.is_whitespace()).take_while(|c| c.is_alphanumeric() || *c == '_').collect();
        (!line.is_empty()).then(|| line.into_iter().rev().collect())
    }

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
        let mut first = true;
        loop {
            // One `,` between entries, as a list takes: `{ a: 1,, b: 2 }` is an empty entry
            // (FEEDBACK45.md, Q-529; branch feedback40-43).
            let mut commas = 0;
            while matches!(self.peek()?, Tok::Newline | Tok::Semi | Tok::Comma) {
                if matches!(self.next()?.tok, Tok::Comma) {
                    commas += 1;
                }
            }
            if commas > 1 || (first && commas > 0) {
                return self.err_at("an empty entry in a map: one `,` goes between two entries");
            }
            first = false;
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
                Tok::LBracket => return Err(QuilloError::new("a map's key is a name or text, and for one in a variable use `set(m, k, v)`", t.line)),
                other => return Err(QuilloError::new(format!("expected a map key, found {}", other.describe()), t.line)),
            };
            if self.eat(&Tok::Colon)? {
                let v = self.parse_expr()?;
                entries.push((key, v));
            } else if shorthand_ok {
                entries.push((key.clone(), Expr::Ident(key.into(), t.line)));
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
        // `if x = 1 {`: an assignment meant as a comparison (FEEDBACK45.md, Q-520).
        if self.at(&Tok::Assign)? {
            return self.err_at("`=` gives a value; to compare, `==`: `if x == 1 { ... }`");
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
            let arm_line = self.line()?;
            let pat = self.parse_pattern()?;
            let guard = if self.eat_kw("if")? {
                self.no_bare_arrow += 1;
                let g = self.parse_expr();
                self.no_bare_arrow -= 1;
                Some(g?)
            } else {
                None
            };
            // `1: "a"`, `1 -> "a"`, `case 1 =>`, `n where n > 0 =>`: other languages' arms
            // (FEEDBACK45.md, Q-520; branch feedback40-43).
            match self.peek()? {
                Tok::Colon | Tok::Arrow => return self.err_at("a `match` arm is a pattern, `=>` and its value: `1 => \"one\"`"),
                Tok::Ident(w) if w == "where" || w == "when" => return self.err_at("a guard is written with `if`: `n if n > 0 => ...`"),
                _ => {}
            }
            if let (Pattern::Bind(w), Tok::Int(_) | Tok::Str(_) | Tok::Ident(_)) = (&pat, self.peek()?.clone()) {
                if &**w == "case" {
                    return self.err_at("there is no `case`: a `match` arm is `1 => \"one\"`");
                }
            }
            self.expect(&Tok::FatArrow, "`=>` after the match pattern")?;
            let body = self.parse_arrow_body()?;
            arms.push(Arm { pat, guard, body, line: arm_line });
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
        let line = self.line()?;
        let word = match self.peek()? {
            Tok::Ident(k) => Some(k.clone()),
            _ => None,
        };
        // `let fn = 1` (`fn` is a type's name too), `let true = 1`: a keyword, as `let if`
        // is, not a name nor a pattern that fails as the program runs (FEEDBACK45.md, Q-516,
        // Q-526; branch feedback40-43).
        if let Some(k) = word.filter(|k| matches!(k.as_str(), "fn" | "true" | "false" | "null")) {
            return Err(QuilloError::new(format!("`{}` is a keyword, so it cannot be a name here: pick another name", k), line));
        }
        Ok(match self.parse_pattern()? {
            Pattern::Type(n) if TYPE_NAMES.contains(&n.as_str()) => Pattern::Bind(n.into()),
            p => p,
        })
    }

    /// React's `function Card({ title, body })`: a view's props are its parameters, each by name
    /// (FEEDBACK48.md, Q-577; branch main).
    fn destructured_params<T>(&mut self, name: &str) -> PResult<T> {
        let names = match self.parse_binding() {
            Ok(Pattern::Record(fs)) => fs.iter().filter_map(|f| f.path.first().cloned()).collect::<Vec<_>>().join(", "),
            _ => String::new(),
        };
        let names = if names.is_empty() { "title, body".to_string() } else { names };
        if name.starts_with(|c: char| c.is_uppercase()) {
            self.err_at(format!("a view's parameters are its props, each by name: write `view {}({})`, without the braces", name, names))
        } else {
            self.err_at(format!("a parameter is a name, not a `{{ ... }}` pattern: write `{}({})`, or take the map and read its fields", name, names))
        }
    }

    fn parse_for(&mut self, line: u32) -> PResult<Expr> {
        // JavaScript's `(x, i)`, for the index (FEEDBACK48.md, Q-574; branch main).
        if self.at(&Tok::LParen)? {
            return self.err_at("`for (x, i)`: a pattern takes no parentheses; for each item with its index, write `for [i, x] in enumerate(xs)`");
        }
        let pat = self.parse_binding()?;
        if !self.eat_kw("in")? {
            return self.err_at("expected `in` after the `for` pattern");
        }
        self.no_bare_arrow += 1;
        let iter = self.parse_expr();
        self.no_bare_arrow -= 1;
        let iter = iter?;
        let body = if self.eat(&Tok::FatArrow)? {
            // A body in braces ends at its `}`, as without the arrow, so
            // what follows applies to the list: `for x in xs => { ... } |
            // flatten()` flattens the list, not each round's block
            // (FEEDBACK14.md, Q-095; branch feedback14-fixes).
            if self.at(&Tok::LBrace)? {
                self.parse_primary()?
            } else {
                self.parse_expr()?
            }
        } else if self.at(&Tok::LBrace)? {
            self.parse_block()?
        } else {
            if matches!(self.peek()?, Tok::Ident(k) if k == "if") {
                return self.err_at("a `for` takes no `if`: `filter` the list first, or put the `if` in its body");
            }
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
                _ => Pattern::Bind(k.clone().into()),
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
                        Pattern::Bind(path.last().unwrap().clone().into())
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

    /// How many characters `!DOCTYPE html>` takes after a `<` just read (any case), or 0.
    fn doctype_len(&self) -> usize {
        let ahead: String = (0..15).map_while(|i| self.lx.peek_char_at(i)).collect();
        let lower = ahead.to_ascii_lowercase();
        if lower.starts_with("!doctype html>") {
            "!doctype html>".len()
        } else {
            0
        }
    }

    /// How many characters `<!-- ... -->` takes from its `<`, or `None` when it is not closed.
    fn html_comment_len(&self) -> Option<usize> {
        let mut i = 4;
        loop {
            match (self.lx.peek_char_at(i)?, self.lx.peek_char_at(i + 1), self.lx.peek_char_at(i + 2)) {
                ('-', Some('-'), Some('>')) => return Some(i + 3),
                _ => i += 1,
            }
        }
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
                    let void = VOID_TAGS.contains(&tag.as_str());
                    // `<img></img>`, `<br></br>`, as JSX and XHTML write them: the end tag right
                    // after is passed over, where it was taken for the end of the element around
                    // it (FEEDBACK48.md, Q-576; branch main).
                    if void {
                        let end: Vec<char> = format!("</{}>", tag).chars().collect();
                        if end.iter().enumerate().all(|(i, c)| self.lx.peek_char_at(i).is_some_and(|d| d.eq_ignore_ascii_case(c))) {
                            for _ in 0..end.len() {
                                self.lx.bump();
                            }
                        }
                    }
                    break void;
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
                            // Plain text with HTML's entities, as in JSX: `&amp;` in a link is `&`
                            // (FEEDBACK48.md, Q-562 and Q-563; branch main).
                            Some('"') => Expr::Lit(Lit::Str(decode_entities(&self.lx.lex_attr_string()?).into())),
                            Some('{') => self.parse_braced_expr()?,
                            // HTML's other two ways: 'single quotes' and an unquoted value, taken as
                            // the text they are, with no `{...}` in them (FEEDBACK36.md, Q-449;
                            // branch feedback36-39).
                            Some('\'') => {
                                self.lx.bump();
                                let mut v = String::new();
                                loop {
                                    match self.lx.bump() {
                                        Some('\'') => break,
                                        Some(c) => v.push(c),
                                        None => return self.html_err(format!("attribute `{}`'s value is never closed with `'`", name)),
                                    }
                                }
                                Expr::Lit(Lit::Str(decode_entities(&v).into()))
                            }
                            Some(c) if !c.is_whitespace() && !"\"'=<>`{}".contains(c) => {
                                let mut v = String::new();
                                while let Some(c) = self.lx.peek_char() {
                                    // `/>` ends the tag, as written after a value: `<img src=a.png/>`.
                                    if c.is_whitespace() || "\"'=<>`{}".contains(c) || (c == '/' && self.lx.peek_char_at(1) == Some('>')) {
                                        break;
                                    }
                                    v.push(c);
                                    self.lx.bump();
                                }
                                Expr::Lit(Lit::Str(decode_entities(&v).into()))
                            }
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
                // `{/* a note */}` between attributes, as in JSX (FEEDBACK48.md, Q-575; branch main).
                Some('{') if self.braced_comment_len() > 0 => {
                    for _ in 0..self.braced_comment_len() {
                        self.lx.bump();
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
                            Expr::Ident(name, l) => attrs.push((name.clone().to_string(), AttrVal::Expr(Expr::Ident(name, l)))),
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
        // `onclick={...}` on an element is `@click={...}`: a handler the program gives, not text the
        // browser runs. `onclick="..."` stays HTML's own, and text built from values stays refused
        // below; on a component, `on_change={...}` is a parameter (branch feedback40-43).
        // `.name` and `#name` on a view's tag are its `class` and `id`, as written out, where
        // they were dropped; a capital after the dot is JSX's member tag, which Quillo has not
        // (FEEDBACK48.md, Q-568 and Q-577; branch main).
        if tag.starts_with(|c: char| c.is_uppercase()) {
            let member = classes.iter().find_map(|c| match c {
                Expr::Lit(Lit::Str(n)) if n.starts_with(|c: char| c.is_uppercase()) => Some(n.to_string()),
                _ => None,
            });
            match member {
                Some(m) if tag == "React" && m == "Fragment" => return Err(QuilloError::new(FRAGMENTS, line)),
                Some(m) => {
                    return Err(QuilloError::new(
                        format!("there are no member tags: `<{tag}.{m}>` is <{tag}> with the class `{m}`; import the view by its own name and write <{m}>"),
                        line,
                    ))
                }
                None => {}
            }
            let mut parts = Vec::new();
            for (i, c) in std::mem::take(&mut classes).into_iter().enumerate() {
                if i > 0 {
                    parts.push(TPart::Lit(" ".to_string()));
                }
                match c {
                    Expr::Lit(Lit::Str(n)) => parts.push(TPart::Lit(n.to_string())),
                    Expr::Template(ps) => parts.extend(ps),
                    e => parts.push(TPart::Expr(e)),
                }
            }
            if !parts.is_empty() {
                let v = match parts.as_slice() {
                    [TPart::Lit(n)] => Expr::Lit(Lit::Str(n.clone().into())),
                    _ => Expr::Template(parts),
                };
                attrs.insert(0, ("class".to_string(), AttrVal::Expr(v)));
            }
            if let Some(i) = id.take() {
                attrs.insert(0, ("id".to_string(), AttrVal::Expr(i)));
            }
        }
        let mut renamed: Vec<(String, String)> = Vec::new();
        if tag.starts_with(|c: char| c.is_ascii_lowercase()) {
            for (n, v) in attrs.iter_mut() {
                let lower = n.to_ascii_lowercase();
                let handler = matches!(v, AttrVal::Expr(e) if !matches!(e, Expr::Lit(_) | Expr::Template(_)));
                if handler && lower.len() > 2 && lower.starts_with("on") && lower[2..].bytes().all(|b| b.is_ascii_lowercase() || b == b'-' || b == b'_') {
                    let ev = react_event(&tag, n, &lower[2..]).map_err(|m| QuilloError::new(m, line))?;
                    renamed.push((format!("@{}", ev), n.clone()));
                    *n = format!("@{}", ev);
                }
            }
        }
        // One of the two would be lost: in a list, `key` and a view's own
        // `key` was such a pair.
        if let Some((_, (name, _))) = attrs.iter().enumerate().find(|(i, (n, _))| n != "..." && attrs[..*i].iter().any(|(m, _)| m == n)) {
            let hint = match renamed.iter().find(|(r, _)| r == name) {
                Some((r, from)) => format!(" (`{}={{...}}` is `{}`)", from, r),
                None => String::new(),
            };
            return self.html_err(format!("attribute `{}` is written twice in <{}>{}", name, tag, hint));
        }
        // The browser decodes these attributes before it runs them, so no
        // escaping keeps a value from data from running as code.
        if let Some((name, _)) = attrs.iter().find(|(n, v)| code_from_data(&tag, &attrs, n, v)) {
            let lower = name.to_ascii_lowercase();
            let msg = match lower.strip_prefix("bind:").unwrap_or(&lower).strip_prefix("on") {
                Some(ev) if !ev.is_empty() && ev.bytes().all(|b| b.is_ascii_lowercase() || b == b'-' || b == b'_') => {
                    format!("`{}` in <{}> takes only text written here, as it runs as code: handle the event with `@{}={{...}}`", name, tag, ev)
                }
                Some(_) => format!("`{}` in <{}> takes only text written here, as the browser runs it as code", name, tag),
                None => format!("`{}` in <{}> takes only text written here, unless the element also has a `sandbox` attribute without allow-same-origin", name, tag),
            };
            return Err(QuilloError::new(msg, line));
        }
        let mut children = Vec::new();
        let mut dollars = Vec::new();
        let mut joined = Vec::new();
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
            let r = self.parse_html_children(&tag, line, &mut dollars, &mut joined);
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
        // `<form idempotent>`: no script is there to send a header, so a key goes
        // with the form, made as it is rendered (FEEDBACK24.md, Q-196; branch
        // feedback24-fixes).
        if tag == "form" {
            if let Some(i) = attrs.iter().position(|(n, a)| n == "idempotent" && matches!(a, AttrVal::Bool)) {
                attrs.remove(i);
                children.insert(0, HtmlChild::El(form_key_field(line)));
            }
        }
        // An element of a component or view with a style of its own.
        if let Some(s) = &mut self.styling {
            s.els += 1;
            if let Some(scope) = &s.scope {
                if !tag.starts_with(|c: char| c.is_uppercase()) {
                    attrs.push((format!("data-q-{}", scope), AttrVal::Bool));
                    s.marked += 1;
                }
            }
            // And of a component of a file with a style (FEEDBACK37.md, Q-457).
            if let (true, Some(f)) = (s.component, &self.file_style) {
                if !tag.starts_with(|c: char| c.is_uppercase()) && s.scope.as_deref() != Some(f.as_str()) {
                    attrs.push((format!("data-q-{}", f), AttrVal::Bool));
                }
            }
        }
        let facts = ElFacts::of(&tag, &id, &classes, &attrs, &children, VOID_TAGS.contains(&tag.as_str()));
        Ok(HtmlEl { tag, id, classes, attrs, children, line, dollars, joined, facts })
    }

    /// What is between an element's tags, up to its end tag. A missing end
    /// tag is told at the element that lacks it, with where it should go
    /// as the fix (branch editor-end-tags): before an end tag that closes
    /// an element around it, before the `}` that ends the code around it,
    /// or before the declaration that the file goes on with.
    fn parse_html_children(&mut self, tag: &str, line: u32, dollars: &mut Vec<u32>, joined: &mut Vec<(u32, String, String)>) -> PResult<Vec<HtmlChild>> {
        let mut children = Vec::new();
        let mut text = String::new();
        // In a <pre>, a <textarea> or a <listing>, text keeps its lines and spaces, as HTML has
        // it, and `//` starts no comment (FEEDBACK48.md, Q-572; branch main).
        let pre = self.open.iter().any(|o| KEEPS_SPACES.contains(&o.tag.as_str()));
        let own_pre = KEEPS_SPACES.contains(&tag);
        // The inline element just before the text being read, and the line it ended on (Q-573).
        let mut after_inline: Option<(String, u32)> = None;
        loop {
            // Text, then a line break, then this inline element; or this one's text after one.
            if text.is_empty() && matches!(self.lx.peek_char(), Some('<' | '{')) {
                after_inline = None;
            }
            if !pre {
                if let Some((el, at)) = after_inline.as_ref().filter(|_| !text.is_empty()) {
                    let next_is_text = !matches!(self.lx.peek_char(), Some('<' | '{') | None);
                    if !next_is_text {
                        if let Some(word) = text_after_break(&text) {
                            joined.push((*at, format!("<{}>", el), format!("\"{}\"", word)));
                        }
                        after_inline = None;
                    }
                }
            }
            match (self.lx.peek_char(), self.lx.peek_char_at(1)) {
                (Some('<'), Some('/')) => {
                    push_text_in(&mut children, &mut text, pre, own_pre);
                    let (at, at_line) = (self.lx.pos, self.lx.line);
                    self.lx.bump();
                    self.lx.bump();
                    let close = self.read_html_name();
                    // An end tag for an element that has none, away from it: removed by the fix,
                    // where the fix made it this element's end (Q-576).
                    if close != tag && VOID_TAGS.contains(&close.as_str()) {
                        let col = col_at(self.lx.text(), at);
                        self.lx.skip_inline_ws();
                        let len = if self.lx.peek_char() == Some('>') { self.lx.pos + 1 - at } else { self.lx.pos - at };
                        return Err(QuilloError::new(format!("`<{}>` has no end tag: remove `</{}>`", close, close), at_line)
                            .with_fix(Fix { title: format!("Remove </{}>", close), line: at_line, col, len: len as u32, text: String::new() }));
                    }
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
                    let next: String = (1..).map_while(|i| self.lx.peek_char_at(i).filter(|c| c.is_alphanumeric())).collect();
                    if !pre && inline_tag(&next) {
                        if let Some(word) = text_before_break(&text) {
                            joined.push((self.lx.line, format!("\"{}\"", word), format!("<{}>", next)));
                        }
                    }
                    push_text_in(&mut children, &mut text, pre, own_pre);
                    let l = self.lx.line;
                    self.lx.bump();
                    children.push(HtmlChild::El(self.nested(|p| p.parse_html_el(l))?));
                    after_inline = inline_tag(&next).then(|| (next, self.lx.line));
                    continue;
                }
                // `<!-- a note -->`, as HTML has it: a comment, left out of the page, where it
                // showed as text (FEEDBACK36.md, Q-448; branch feedback36-39).
                (Some('<'), Some('!')) if tag != "script" && tag != "style" && self.lx.peek_char_at(2) == Some('-') && self.lx.peek_char_at(3) == Some('-') => {
                    let Some(n) = self.html_comment_len() else {
                        return self.html_err("`<!--` is never closed with `-->`");
                    };
                    push_text_in(&mut children, &mut text, pre, own_pre);
                    for _ in 0..n {
                        self.lx.bump();
                    }
                }
                // `{/* a note */}`: a comment, as in JSX.
                (Some('{'), _) if self.braced_comment_len() > 0 => {
                    for _ in 0..self.braced_comment_len() {
                        self.lx.bump();
                    }
                }
                // `{...xs}`, JSX's spread of children: a list shows each item already (Q-577).
                (Some('{'), _) if self.lx.peek_char_at(1) == Some('.') && self.lx.peek_char_at(2) == Some('.') && self.lx.peek_char_at(3) == Some('.') => {
                    return self.html_err("`{...xs}` among children: a list shows each of its items where it stands, so write `{xs}`");
                }
                (Some('{'), _) => {
                    if text.ends_with('$') {
                        dollars.push(self.lx.line);
                    }
                    push_text_in(&mut children, &mut text, pre, own_pre);
                    // JavaScript's own braces in a <script> are read as Quillo: said so, with what to
                    // write (FEEDBACK35.md, Q-428; branch feedback27-35-fixes).
                    let e = match self.parse_braced_expr() {
                        Err(e) if tag == "script" => {
                            return Err(QuilloError::new(
                                format!("{} (a `{{` in a <script> starts a Quillo value: write the code as a string, <script>{{raw(CODE)}}</script>, or put it in a file of its own)", e.msg),
                                e.line,
                            ))
                        }
                        r => r?,
                    };
                    // `{"hello"}` in a <script> is a value, written as JSON (`"hello"`), not the
                    // script's own text, which a string written so was taken for
                    // (FEEDBACK53.md, Q-682; branch main).
                    let e = match e {
                        e @ Expr::Lit(Lit::Str(_)) if tag == "script" => Expr::Template(vec![TPart::Expr(e)]),
                        e => e,
                    };
                    children.push(HtmlChild::Expr(and_as_if(e)));
                }
                // A line of markup starting with `//` is a comment, not
                // text for the page (`{"//"}` writes the characters).
                (Some('/'), Some('/')) if !pre && text.contains('\n') && text.rsplit('\n').next().map_or(false, |l| l.trim().is_empty()) => {
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
    /// comment; the comment may hold `}`). JSX's `{// a note` and a `}` on a line after it
    /// is one too, and so are several (FEEDBACK48.md, Q-575; branch main).
    fn braced_comment_len(&self) -> usize {
        let at = |i: usize| self.lx.peek_char_at(i);
        let mut i = 1;
        let mut comments = 0;
        loop {
            while matches!(at(i), Some(' ' | '\t' | '\r' | '\n')) {
                i += 1;
            }
            match (at(i), at(i + 1)) {
                (Some('/'), Some('*')) => {
                    i += 2;
                    loop {
                        match (at(i), at(i + 1)) {
                            (Some('*'), Some('/')) => break,
                            (Some(_), _) => i += 1,
                            (None, _) => return 0,
                        }
                    }
                    i += 2;
                }
                (Some('/'), Some('/')) => {
                    while !matches!(at(i), Some('\n') | None) {
                        i += 1;
                    }
                }
                (Some('}'), _) if comments > 0 => return i + 1,
                _ => return 0,
            }
            comments += 1;
        }
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
        // `@click={n += 1}`: an assignment, which a handler runs, not a value (FEEDBACK45.md,
        // Q-518; branch feedback40-43).
        if let Some(op) = match self.peek()? {
            Tok::Assign => Some("="),
            Tok::PlusAssign => Some("+="),
            Tok::MinusAssign => Some("-="),
            _ => None,
        } {
            let name = match &e {
                Expr::Ident(n, _) => n.to_string(),
                _ => "x".to_string(),
            };
            return self.err_at(format!("an assignment is not a value: a handler is a function, `@click={{() => {{ {} {} ... }}}}`", name, op));
        }
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

/// The syntax errors of `src` after the one on line `after`: each from the next line that
/// starts a top-level statement on, as a parser that stops at its first error cannot (at most
/// nine more; FEEDBACK45.md, Q-522; branch feedback40-43).
pub fn more_errors(src: &str, after: u32) -> Vec<QuilloError> {
    const STARTS: &[&str] = &[
        "let ", "fn ", "view ", "component ", "route ", "routes ", "type ", "use ", "import ", "server fn ", "service ", "mount ",
        "client ", "preload ", "preconnect ", "print(", "database ", "site ", "style ", "test ",
    ];
    let lines: Vec<&str> = src.split('\n').collect();
    // A line inside a `...` template or a string is its text, not a statement (found in
    // review): which lines start inside one, read as the lexer would, `//` comments aside.
    let mut inside = Vec::with_capacity(lines.len());
    let (mut tick, mut quote) = (false, false);
    for line in &lines {
        inside.push(tick || quote);
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\\' if tick || quote => {
                    chars.next();
                }
                '`' if !quote => tick = !tick,
                '"' if !tick => quote = !quote,
                '/' if !tick && !quote && chars.peek() == Some(&'/') => break,
                _ => {}
            }
        }
    }
    let mut out: Vec<QuilloError> = Vec::new();
    let mut last = after;
    // The error's own line may start the next statement (`let a = (1` is said at the `let` of
    // the line after): look from there, and a line further each time nothing new is found.
    let mut from = (after as usize).saturating_sub(1);
    while out.len() < 9 {
        let Some(i) = (from..lines.len()).find(|&i| !inside[i] && STARTS.iter().any(|s| lines[i].starts_with(s))) else { break };
        let rest = lines[i..].join("\n");
        match Parser::with_line(&rest, (i + 1) as u32).parse_program() {
            Ok(_) => break,
            Err(e) if e.line <= last => from = i + 1,
            Err(e) => {
                last = e.line;
                from = i + 1;
                out.push(e);
            }
        }
    }
    out
}

/// The calls of a group's middleware, each answered by `__answer`, that start each of its
/// routes' bodies and its hidden guard route's.
fn guard_calls(before: &[(Expr, u32)]) -> Vec<Stmt> {
    before
        .iter()
        .map(|(m, ml)| {
            // With the request itself, which the route's patterns cannot bind: a
            // JSON field named `request` was what the middleware checked
            // (FEEDBACK47.md, Q-535; branch feedback40-43). Its name goes along,
            // for what it gave (Q-534).
            let called = Expr::Call(Box::new(m.clone()), vec![Expr::Ident("__request".into(), *ml)], *ml);
            let shown = match m {
                Expr::Ident(n, _) => n.to_string(),
                Expr::Call(f, _, _) => match &**f {
                    Expr::Ident(n, _) => format!("{}(...)", n),
                    _ => "the middleware".to_string(),
                },
                _ => "the middleware".to_string(),
            };
            let name = Expr::Lit(Lit::Str(shown.into()));
            Stmt::Expr(Expr::Call(Box::new(Expr::Ident("__answer".into(), *ml)), vec![called, name], *ml))
        })
        .collect()
}

/// The name in `{ ... } | g`, a request pattern's alternative that binds anything: written for
/// middleware, it made the pattern match every request (FEEDBACK47.md, Q-536).
fn bare_alternative(p: &Pattern) -> Option<String> {
    match p {
        Pattern::Alt(ps) => ps.iter().find_map(|p| match p {
            Pattern::Bind(n) => Some(n.to_string()),
            _ => None,
        }),
        _ => None,
    }
}

/// A route path as written: `/admin/:id`.
fn shown_path(path: &[Seg]) -> String {
    let s: String = path
        .iter()
        .map(|seg| match seg {
            Seg::Lit(l) => format!("/{}", l),
            Seg::Param(n, Some(t), p) => format!("/{}:{}({})", p, n, t),
            Seg::Param(n, None, p) => format!("/{}:{}", p, n),
            Seg::Rest(n) => format!("/*{}", n),
        })
        .collect();
    if s.is_empty() { "/".to_string() } else { s }
}

/// What to write instead of an old Wisp-era spelling.
/// What to write for what Python and JavaScript write, where a token gives it away
/// (FEEDBACK29.md, Q-359; branch feedback29-fixes).
fn habit(t: &Tok) -> Option<&'static str> {
    match t {
        Tok::Ident(k) if k == "in" => Some("there is no `in`: `contains(xs, x)`, or `has(m, k)` for a map"),
        Tok::Lt | Tok::Le | Tok::Gt | Tok::Ge => Some("comparisons do not chain: `a < x && x < b`"),
        _ => None,
    }
}

/// The same, for a word that reads as a name until what follows it.
fn word_habit(w: &str) -> Option<&'static str> {
    Some(match w {
        "try" | "catch" | "except" => "there is no `try`: errors are values, so `?` passes one on and `??` replaces it",
        "class" => "there are no classes: `type` for a record, and functions",
        "yield" => "there are no generators: return a list",
        "throw" | "raise" => "there is no `throw`: `return err(...)`",
        "def" | "function" | "func" | "fun" => "a function is `fn name(x) = ...`",
        // Other languages' words (FEEDBACK45.md, Q-520; branch feedback40-43).
        "const" | "var" | "val" => "there is no `const` or `var`: `let x = 1`, which can be given again with `x = 2`",
        "elif" | "elsif" => "after `if`, `else if`",
        "switch" => "there is no `switch`: `match x { 1 => \"one\", _ => \"other\" }`",
        "typeof" => "`type_of(x)` gives a value's type",
        "from" => "names are taken from a file with `import lib/m.qwe use price`",
        _ => return None,
    })
}

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

/// The DOM event a React handler's name (`onDoubleClick`, as `doubleclick`) listens for, where
/// the two differ: React's `onChange` on a field is every keystroke, the DOM's `input`, where
/// `change` waited for the field to lose focus; its `onFocus` and `onBlur` reach a wrapper, as
/// the DOM's `focusin` and `focusout` do (FEEDBACK48.md, Q-560 and Q-561; branch main).
pub(crate) fn react_event(tag: &str, written: &str, ev: &str) -> Result<String, String> {
    Ok(match ev {
        "change" if tag == "input" || tag == "textarea" => "input",
        "doubleclick" => "dblclick",
        "focus" => "focusin",
        "blur" => "focusout",
        "gotpointercapture" | "lostpointercapture" => ev,
        _ if ev.len() > "capture".len() && ev.ends_with("capture") => {
            let base = &ev[..ev.len() - "capture".len()];
            return Err(format!(
                "`{}` listens before the element's own handlers, as React's `…Capture` does, which Quillo has no form for: write `on{}{}` (or `@{}`), which hears it as it bubbles up",
                written,
                &written[2..3].to_ascii_uppercase(),
                &written[3..written.len() - "capture".len()],
                react_event(tag, written, base).unwrap_or_else(|_| base.to_string())
            ));
        }
        _ => ev,
    }
    .to_string())
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
        // Every name HTML has, as JSX decodes them, where ten were known and `&rarr;` showed as
        // written (FEEDBACK48.md, Q-562; branch main). The longest is 31 letters.
        let end = rest[1..].find(|c: char| !c.is_ascii_alphanumeric() && c != '#').map(|e| e + 1).filter(|&e| rest[e..].starts_with(';') && e <= 33);
        let decoded = end.and_then(|e| {
            let name = &rest[1..e];
            let c = match name.strip_prefix('#') {
                Some(n) => n
                    .strip_prefix(['x', 'X'])
                    .map_or_else(|| n.parse().ok(), |h| u32::from_str_radix(h, 16).ok())
                    .and_then(char::from_u32)
                    .map(String::from),
                None => crate::entities::named(name).map(String::from),
            };
            c.map(|c| (c, e))
        });
        match decoded {
            Some((c, e)) => {
                out.push_str(&c);
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

/// Elements shown in the line of text, where a missing space shows; a view too.
pub(crate) fn inline_tag(tag: &str) -> bool {
    tag.starts_with(|c: char| c.is_uppercase())
        || matches!(tag, "a" | "b" | "i" | "em" | "strong" | "span" | "code" | "small" | "mark" | "s" | "u" | "sub" | "sup" | "abbr" | "kbd" | "cite" | "q" | "time" | "var" | "samp" | "dfn")
}

/// The last word of text that ends with a line break, when a space would be wanted after it.
fn text_before_break(text: &str) -> Option<&str> {
    let body = text.trim_end();
    let tail = &text[body.len()..];
    if body.trim().is_empty() || !tail.contains('\n') || body.ends_with(['(', '[', '{', '"', '\'', '\u{201c}', '\u{2018}', '/', '-']) {
        return None;
    }
    body.split_whitespace().last()
}

/// The first word of text that starts after a line break, when a space would be wanted before it.
fn text_after_break(text: &str) -> Option<&str> {
    let body = text.trim_start();
    let head = &text[..text.len() - body.len()];
    if body.trim().is_empty() || !head.contains('\n') || body.starts_with(['.', ',', ';', ':', '!', '?', ')', ']', '}', '"', '\'', '\u{201d}', '\u{2019}', '/', '-']) {
        return None;
    }
    body.split_whitespace().next()
}

/// What `<>`, `<Fragment>` and `<React.Fragment>` are told.
pub const FRAGMENTS: &str = "there are no `<>` fragments: wrap the elements in one, like <div>, or give a list";

/// The elements whose text keeps its layout.
const KEEPS_SPACES: &[&str] = &["pre", "textarea", "listing"];

/// Text inside a <pre> (`pre`) as written, entities decoded: only spaces across lines, as
/// between its tags in code laid out to read, are left out, as JSX leaves them, and the line
/// break right after `<pre>` (its first text, `first`) as HTML does.
fn push_text_in(children: &mut Vec<HtmlChild>, text: &mut String, pre: bool, first: bool) {
    if !pre {
        return push_text(children, text);
    }
    if text.is_empty() {
        return;
    }
    let raw = std::mem::take(text);
    if raw.trim().is_empty() && raw.contains('\n') {
        return;
    }
    let raw = if first && children.is_empty() { raw.strip_prefix("\r\n").or_else(|| raw.strip_prefix('\n')).unwrap_or(&raw) } else { &raw };
    if !raw.is_empty() {
        children.push(HtmlChild::Expr(Expr::Lit(Lit::Str(decode_entities(raw).into()))));
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
        Pattern::Bind(n) => out.push(n.to_string()),
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

/// The hidden field `<form idempotent>` carries: see `idempotency::FORM_FIELD`.
fn form_key_field(line: u32) -> HtmlEl {
    let text = |s: &str| AttrVal::Expr(Expr::Lit(Lit::Str(s.into())));
    let key = Expr::Call(Box::new(Expr::Ident("uuid".into(), line)), Vec::new(), line);
    let attrs = vec![("type".to_string(), text("hidden")), ("name".to_string(), text(crate::idempotency::FORM_FIELD)), ("value".to_string(), AttrVal::Expr(key))];
    let facts = ElFacts::of("input", &None, &[], &attrs, &[], true);
    HtmlEl { tag: "input".into(), id: None, classes: Vec::new(), attrs, children: Vec::new(), line, dollars: Vec::new(), joined: Vec::new(), facts }
}
