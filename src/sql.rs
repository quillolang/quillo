//! SQL as each database reads it: one tokenizer for SQLite, PostgreSQL and MySQL/MariaDB,
//! which every part of Quillo that looks into SQL text goes through (FEEDBACK55.md, Q-746,
//! Q-749, Q-708; branch feedback55). Several small scanners, each with its own idea of quotes
//! and comments, refused function bodies as `begin` and `end`, read `?||` as an operator, and
//! looked for `set_config` only in a `select`.
//!
//! What it knows, by dialect:
//!
//! - strings: `'...'` with `''` everywhere; with `\` escapes on MySQL (its default sql_mode)
//!   and in PostgreSQL's `E'...'`; `U&'...'`, `B'...'`, `X'...'` and `N'...'`. `"..."` is a
//!   string on MySQL and a name elsewhere; `` `...` `` a name on MySQL and SQLite, `[...]` on
//!   SQLite;
//! - PostgreSQL's dollar quotes, `$$...$$` and `$tag$...$tag$`;
//! - comments: `--` (on MySQL only before a space), `#` on MySQL, `/* */` (nested on
//!   PostgreSQL), and MySQL's `/*! ... */`, whose text MySQL runs;
//! - placeholders: `?`, `?NNN` and `:name` everywhere; `@name` and `$name` on SQLite; `$n` on
//!   PostgreSQL, where a `?` after a value is an operator (jsonb's `?`, `?|`, `?&`, geometry's
//!   `?||`) and so is `@?`, while `'%'||?||'%'` and `like ?||'%'` hold a placeholder;
//! - statements, split at `;` outside parentheses and outside the bodies of `create
//!   function`, `procedure`, `trigger` and `event` (`begin atomic ... end`, MySQL's `begin ...
//!   end` with `end if`, `end loop` and `case` inside, SQLite's trigger bodies) and MariaDB's
//!   `begin not atomic ... end`. A placeholder in such a body is not the statement's.
//!
//! Values are numbered as SQLite numbers them, on every database: `?` takes the one after
//! the largest so far, `?NNN` the NNNth, and a name the next the first time it is seen.
//!
//! A text is read once for each dialect, and kept: statements are mostly written in the
//! program, so few, and each call then costs a look-up (without a lock for those a thread used
//! last).

use crate::value::{Map, Value};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Dialect {
    Sqlite,
    Postgres,
    Mysql,
}

impl Dialect {
    pub const ALL: [Dialect; 3] = [Dialect::Sqlite, Dialect::Postgres, Dialect::Mysql];
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// A keyword or a name as written.
    Word,
    /// A quoted name: `"x"`, `` `x` ``, `[x]`.
    Quoted,
    /// A string, dollar-quoted text included.
    Str,
    Num,
    /// A placeholder.
    Param(Mark),
    /// An operator, or a character with no other part.
    Op,
    Open,
    Close,
    OpenBracket,
    CloseBracket,
    Comma,
    Semi,
    Dot,
    /// `::`
    Cast,
    /// MySQL's `@name` and `@@name`.
    Var,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mark {
    /// `?`
    Question,
    /// `?NNN`
    Numbered(usize),
    /// `:name`, and SQLite's `@name` and `$name`
    Named,
    /// PostgreSQL's `$n`
    Dollar(usize),
}

#[derive(Clone, Copy, Debug)]
pub struct Tok {
    pub kind: Kind,
    pub start: usize,
    pub end: usize,
}

/// A statement's tokens, `from..to`, while the text is read.
#[derive(Clone, Copy, Debug)]
struct Span {
    from: usize,
    to: usize,
    routine: bool,
}

/// A statement as kept: where it is in the text, and its first two words.
#[derive(Clone, Copy, Debug)]
pub struct Stmt {
    pub start: usize,
    pub end: usize,
    /// It makes a function, procedure, trigger or event, whose body has statements of its
    /// own (and placeholders of the body's).
    pub routine: bool,
    head: [(usize, usize); 2],
}

/// The most values a statement can take: past it, a number is not a placeholder's.
const MOST_VALUES: usize = 65_535;

/// What Quillo needs of a text, read once and kept: not its tokens, which took some thirty
/// times the text's size, and 800 MB for a thousand texts with long lists written in them
/// (found in review).
pub struct Scan {
    pub sql: Arc<str>,
    pub dialect: Dialect,
    pub stmts: Vec<Stmt>,
    /// The placeholders, in order: where each is in the text, and the value (from 1) it takes.
    marks: Vec<(usize, usize, usize)>,
    /// How many values the SQL takes (the largest number).
    pub count: usize,
    /// A placeholder names its value (`:id`): one map gives the values by name.
    pub named: bool,
    /// The name of each value (from 1), with its sign, as a range of `sql`.
    names: Vec<Option<(usize, usize)>>,
    /// The SQL as the database takes it, when not as written (`$1` for PostgreSQL, `?` for
    /// MySQL).
    native: Option<Box<str>>,
    /// The value each of the database's own placeholders takes, when not 1, 2, 3...
    order: Option<Box<[usize]>>,
    tx_alone: bool,
    tx_any: bool,
    prepares: bool,
    /// PostgreSQL: whether it changes the session whatever its values, and the placeholders
    /// (by their place in `marks`) whose values say (`set_config`'s third).
    lasting: bool,
    given: Vec<usize>,
    /// PostgreSQL: it may change the session where this reading cannot see: a `do` block, whose
    /// SQL may be made as it runs (`execute`).
    pub unseen: bool,
    /// PostgreSQL and MySQL: the names it calls as functions, as the server reads them (a function of
    /// the program's may change the session too).
    pub calls: Vec<Box<str>>,
    /// PostgreSQL: it makes, replaces or drops functions (or an extension's).
    pub defines: bool,
    /// MySQL: the first statement that commits the transaction around it by itself.
    commits: Option<(usize, usize)>,
    /// PostgreSQL takes the first placeholder's value as an array.
    array_first: bool,
    /// The tables it reads and writes (live updates, replicas), read from these tokens
    /// (FEEDBACK55.md, Q-750; branch feedback55).
    pub tables: crate::sqlinfo::Tables,
    /// It only reads, wherever it runs: see `reads_only`.
    reads_only: bool,
    /// MySQL's `@name` variables, as ranges of `sql` without the `@`.
    vars: Vec<(usize, usize)>,
    /// `insert ... values (?, ...)` and nothing after: where `values` starts, and how many `?`.
    tuple: Option<(usize, usize)>,
    /// SQLite: the columns a `create table` or `alter table` gives a type kept as a float
    /// (`warn_lossy_columns`), as (table, column, type).
    pub lossy: Vec<(Box<str>, Box<str>, Box<str>)>,
    /// MySQL: one `replace` or `insert ... on duplicate key update`, whose rows MySQL counts
    /// twice when it replaces or updates them.
    twice: bool,
    /// MySQL: it has `||`, which MariaDB reads as `or` unless the session says otherwise.
    bars: bool,
    /// PostgreSQL: for each value (from 1), whether a placeholder of it is a column's value as
    /// written (`Reader::assigned_at`).
    assigned: Vec<bool>,
    /// PostgreSQL: it changes a schema, and the names it changes (None: any; `Reader::ddl`).
    ddl: Option<Option<Vec<Box<str>>>>,
    /// MySQL: for each value (from 1), the column it is given to or compared with, in lower
    /// case (`Reader::column_at`).
    columns: Vec<Option<Box<str>>>,
}

/// When what `set_config` sets ends.
enum Ends {
    WithTransaction,
    WithSession,
    /// As the value of the placeholder at this token says.
    AsGiven(usize),
}

// ---- reading ----------------------------------------------------------------------------

fn ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c >= 0x80
}

fn ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'$' || c >= 0x80
}

fn ident_end(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && ident(b[i]) {
        i += 1;
    }
    i
}

fn pg_op(c: u8) -> bool {
    matches!(c, b'+' | b'-' | b'*' | b'/' | b'<' | b'>' | b'=' | b'~' | b'!' | b'@' | b'#' | b'%' | b'^' | b'&' | b'|' | b'`' | b'?')
}

/// Where a quoted text that starts at `i` ends (after its closing quote, or at the end).
fn quote_end(b: &[u8], i: usize, q: u8, backslash: bool) -> usize {
    let mut j = i + 1;
    while j < b.len() {
        let c = b[j];
        if backslash && c == b'\\' {
            j += 2;
            continue;
        }
        if c == q {
            if b.get(j + 1) == Some(&q) {
                j += 2;
                continue;
            }
            return j + 1;
        }
        j += 1;
    }
    b.len()
}

/// Where a `/* */` comment that starts at `i` ends (PostgreSQL's nest), and how many it left
/// open at the end of the text.
fn comment_end(b: &[u8], i: usize, nest: bool) -> (usize, usize) {
    let (mut depth, mut j) = (0usize, i);
    while j < b.len() {
        if b[j] == b'/' && b.get(j + 1) == Some(&b'*') {
            if depth == 0 || nest {
                depth += 1;
            }
            j += 2;
        } else if b[j] == b'*' && b.get(j + 1) == Some(&b'/') {
            depth -= 1;
            j += 2;
            if depth == 0 {
                return (j, 0);
            }
        } else {
            j += 1;
        }
    }
    (b.len(), depth)
}

fn line_end(b: &[u8], i: usize) -> usize {
    b[i..].iter().position(|&c| c == b'\n').map_or(b.len(), |k| i + k)
}

/// `$tag$ ... $tag$` (PostgreSQL) starting at `i`: where it ends, or None for no such quote.
fn dollar_quote_end(s: &str, i: usize) -> Option<usize> {
    let b = s.as_bytes();
    let mut k = i + 1;
    while k < b.len() && ident(b[k]) && b[k] != b'$' {
        k += 1;
    }
    if b.get(k) != Some(&b'$') {
        return None;
    }
    let open = &s[i..=k];
    Some(s[k + 1..].find(open).map_or(b.len(), |e| k + 1 + e + open.len()))
}

fn number_end(b: &[u8], mut i: usize) -> usize {
    while i < b.len() {
        let c = b[i];
        if !(c.is_ascii_alphanumeric() || c == b'_' || c == b'.') {
            break;
        }
        if (c == b'e' || c == b'E') && matches!(b.get(i + 1), Some(b'+' | b'-')) && b.get(i + 2).map_or(false, u8::is_ascii_digit) {
            i += 2;
        }
        i += 1;
    }
    i
}

/// The digits at `i`, as a placeholder's number (None past `MOST_VALUES`, or 0), and where
/// they end.
fn digits(b: &[u8], i: usize) -> (Option<usize>, usize) {
    let mut j = i;
    let mut n: usize = 0;
    while j < b.len() && b[j].is_ascii_digit() {
        n = n.saturating_mul(10).saturating_add((b[j] - b'0') as usize);
        j += 1;
    }
    (Some(n).filter(|n| (1..=MOST_VALUES).contains(n)), j)
}

/// Words after which a `?` is a value, not one of PostgreSQL's operators.
const BEFORE_VALUE: &[&str] = &[
    "select", "where", "and", "or", "not", "limit", "offset", "by", "on", "having", "when", "then", "else", "case",
    "in", "is", "like", "ilike", "similar", "to", "between", "symmetric", "asymmetric", "values", "set", "returning",
    "distinct", "all", "any", "some", "escape", "exists", "return", "first", "next", "using", "from", "for", "zone",
    "overlaps", "placing", "both", "leading", "trailing",
];

/// Whether the last token ends a value: a name, a string, a number, a placeholder, `)`, `]`.
fn value_ends(out: &[Tok]) -> bool {
    matches!(out.last().map(|t| t.kind), Some(Kind::Word | Kind::Quoted | Kind::Str | Kind::Num | Kind::Param(_) | Kind::Close | Kind::CloseBracket))
}

/// Whether the token before a `?` ends a value (then the `?` is PostgreSQL's operator): a
/// name, a column, a string, a number, `)` or `]`. After an operator, a bracket, a comma or
/// a keyword such as `limit`, it is a placeholder.
fn value_before(s: &str, out: &[Tok]) -> bool {
    match out.last() {
        None => false,
        Some(t) => match t.kind {
            Kind::Quoted | Kind::Str | Kind::Num | Kind::Close | Kind::CloseBracket | Kind::Param(_) => true,
            Kind::Word => !BEFORE_VALUE.iter().any(|w| s[t.start..t.end].eq_ignore_ascii_case(w)),
            _ => false,
        },
    }
}

/// A run of PostgreSQL's operator characters at `start`, as tokens: the `?` in it a
/// placeholder or an operator. Where it ends.
fn pg_ops(s: &str, start: usize, out: &mut Vec<Tok>) -> usize {
    let b = s.as_bytes();
    let mut j = start;
    while j < b.len() && pg_op(b[j]) {
        // A comment ends an operator (PostgreSQL's rule).
        if j > start && ((b[j] == b'-' && b.get(j + 1) == Some(&b'-')) || (b[j] == b'/' && b.get(j + 1) == Some(&b'*'))) {
            break;
        }
        j += 1;
    }
    let mut k = start;
    let mut end = j;
    while k < j {
        let Some(p) = b[k..j].iter().position(|&c| c == b'?') else {
            out.push(Tok { kind: Kind::Op, start: k, end: j });
            break;
        };
        let q = k + p;
        // `@?`: jsonpath's operator.
        if p == 1 && b[k] == b'@' {
            out.push(Tok { kind: Kind::Op, start: k, end: q + 1 });
            k = q + 1;
            continue;
        }
        if p > 0 {
            // `=?`, `||?`: an operator, then a value.
            out.push(Tok { kind: Kind::Op, start: k, end: q });
        } else if value_before(s, out) {
            // `data ? 'k'`, `data ?| array[...]`, `a ?|| b`: one operator.
            out.push(Tok { kind: Kind::Op, start: q, end: j });
            break;
        }
        // A placeholder (`?1` numbered); `?||` is one and `||`.
        let mut kind = Kind::Param(Mark::Question);
        let mut e = q + 1;
        if e == j {
            let (n, d) = digits(b, e);
            if d > e {
                kind = n.map_or(Kind::Op, |n| Kind::Param(Mark::Numbered(n)));
                e = d;
                end = d;
            }
        }
        out.push(Tok { kind, start: q, end: e });
        k = q + 1;
    }
    end
}

/// The tokens, and how many block comments the text leaves open at its end.
pub(crate) fn lex(d: Dialect, s: &str) -> (Vec<Tok>, usize) {
    let b = s.as_bytes();
    let n = b.len();
    let mut out: Vec<Tok> = Vec::with_capacity(n / 4 + 2);
    let mut i = 0;
    // PostgreSQL: inside `[...]`, where `a[lo:hi]` is a slice, not a name.
    let mut brackets = 0usize;
    // MySQL: inside `/*! ... */`, whose text it runs.
    let mut hint = false;
    let mut open = 0;
    let (pg, my) = (d == Dialect::Postgres, d == Dialect::Mysql);
    while i < n {
        let c = b[i];
        let start = i;
        let kind = match c {
            b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c => {
                i += 1;
                continue;
            }
            b'-' if b.get(i + 1) == Some(&b'-') && (!my || b.get(i + 2).map_or(true, |c| c.is_ascii_whitespace() || c.is_ascii_control())) => {
                i = line_end(b, i);
                continue;
            }
            b'#' if my => {
                i = line_end(b, i);
                continue;
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                // MySQL's `/*! ... */` and MariaDB's `/*M! ... */` (found in review).
                let runs = if b.get(i + 2) == Some(&b'!') { Some(3) } else if b.get(i + 2) == Some(&b'M') && b.get(i + 3) == Some(&b'!') { Some(4) } else { None };
                if let (true, Some(skip)) = (my, runs) {
                    i += skip;
                    while i < n && b[i].is_ascii_digit() {
                        i += 1;
                    }
                    hint = true;
                } else {
                    (i, open) = comment_end(b, i, pg);
                }
                continue;
            }
            b'*' if hint && b.get(i + 1) == Some(&b'/') => {
                hint = false;
                i += 2;
                continue;
            }
            b'\'' => {
                i = quote_end(b, i, b'\'', my);
                Kind::Str
            }
            b'"' if my => {
                i = quote_end(b, i, b'"', true);
                Kind::Str
            }
            b'"' => {
                i = quote_end(b, i, b'"', false);
                Kind::Quoted
            }
            b'`' if !pg => {
                i = quote_end(b, i, b'`', false);
                Kind::Quoted
            }
            b'[' if d == Dialect::Sqlite => {
                i = b[i..].iter().position(|&c| c == b']').map_or(n, |k| i + k + 1);
                Kind::Quoted
            }
            b'[' => {
                brackets += 1;
                i += 1;
                Kind::OpenBracket
            }
            b']' => {
                brackets = brackets.saturating_sub(1);
                i += 1;
                Kind::CloseBracket
            }
            b'(' => {
                i += 1;
                Kind::Open
            }
            b')' => {
                i += 1;
                Kind::Close
            }
            b',' => {
                i += 1;
                Kind::Comma
            }
            b';' => {
                i += 1;
                Kind::Semi
            }
            b'.' if !b.get(i + 1).map_or(false, u8::is_ascii_digit) => {
                i += 1;
                Kind::Dot
            }
            b':' if b.get(i + 1) == Some(&b':') => {
                i += 2;
                Kind::Cast
            }
            // A name; SQLite's may be digits (`:1`). In PostgreSQL's brackets, after a value it
            // is a slice's bound (`a[lo:hi]`), else a name (`array[:a, :b]`; found in review).
            b':' if b.get(i + 1).map_or(false, |&c| ident_start(c) || (d == Dialect::Sqlite && ident(c) && c != b'$'))
                && !(pg && brackets > 0 && value_ends(&out)) =>
            {
                i = ident_end(b, i + 1);
                Kind::Param(Mark::Named)
            }
            b'?' if !pg => {
                let (num, e) = digits(b, i + 1);
                i = e;
                match (num, e > start + 1) {
                    (_, false) => Kind::Param(Mark::Question),
                    (Some(n), true) => Kind::Param(Mark::Numbered(n)),
                    (None, true) => Kind::Op,
                }
            }
            // SQLite's `@AAAA` and `#AAAA` (digits too: found in review).
            b'@' | b'#' if d == Dialect::Sqlite && b.get(i + 1).map_or(false, |&c| ident(c) && c != b'$') => {
                i = ident_end(b, i + 1);
                Kind::Param(Mark::Named)
            }
            b'@' if my => {
                i += 1;
                if b.get(i) == Some(&b'@') {
                    i += 1;
                }
                match b.get(i) {
                    Some(&q @ (b'\'' | b'"' | b'`')) => i = quote_end(b, i, q, q != b'`'),
                    _ => {
                        while i < n && (ident(b[i]) || b[i] == b'.') {
                            i += 1;
                        }
                    }
                }
                Kind::Var
            }
            b'$' if d == Dialect::Sqlite && b.get(i + 1).map_or(false, |&c| ident(c) && c != b'$') => {
                i = ident_end(b, i + 1);
                Kind::Param(Mark::Named)
            }
            b'$' if pg && b.get(i + 1).map_or(false, u8::is_ascii_digit) => {
                let (num, e) = digits(b, i + 1);
                i = e;
                num.map_or(Kind::Op, |n| Kind::Param(Mark::Dollar(n)))
            }
            b'$' if pg => match dollar_quote_end(s, i) {
                Some(e) => {
                    i = e;
                    Kind::Str
                }
                None => {
                    i += 1;
                    Kind::Op
                }
            },
            c if c.is_ascii_digit() || c == b'.' => {
                i = number_end(b, i);
                Kind::Num
            }
            c if ident_start(c) || (c == b'$' && my) => {
                let w = ident_end(b, i);
                let one = (w == i + 1).then(|| c | 0x20);
                if pg && one == Some(b'u') && b.get(w) == Some(&b'&') && matches!(b.get(w + 1), Some(b'\'' | b'"')) {
                    // U&'...', U&"..."
                    let q = b[w + 1];
                    i = quote_end(b, w + 1, q, false);
                    if q == b'\'' {
                        Kind::Str
                    } else {
                        Kind::Quoted
                    }
                } else if b.get(w) == Some(&b'\'') && matches!(one, Some(b'b' | b'x' | b'n')) || (pg && one == Some(b'e') && b.get(w) == Some(&b'\'')) {
                    // E'...' (with escapes), B'...', X'...', N'...'
                    i = quote_end(b, w, b'\'', my || one == Some(b'e'));
                    Kind::Str
                } else {
                    i = w;
                    Kind::Word
                }
            }
            c if pg && pg_op(c) => {
                i = pg_ops(s, i, &mut out);
                continue;
            }
            _ => {
                // One character (the rest of a UTF-8 one is a name's: see `ident`).
                i += 1;
                Kind::Op
            }
        };
        out.push(Tok { kind, start, end: i.min(n) });
    }
    (out, open)
}

// ---- statements -------------------------------------------------------------------------

/// Whether the statement at token `i` makes a routine whose body holds statements: `create
/// [or replace] function` or `procedure` (PostgreSQL's `begin atomic`, MySQL's `begin`),
/// a trigger on MySQL and SQLite, an event on MySQL, MariaDB's `begin not atomic`.
fn routine_at(d: Dialect, s: &str, toks: &[Tok], i: usize) -> bool {
    let word = |k: usize| toks.get(k).filter(|t| t.kind == Kind::Word).map(|t| &s[t.start..t.end]);
    let is = |k: usize, w: &str| word(k).map_or(false, |x| x.eq_ignore_ascii_case(w));
    if d == Dialect::Mysql && is(i, "begin") && is(i + 1, "not") && is(i + 2, "atomic") {
        return true;
    }
    if !is(i, "create") {
        return false;
    }
    // Past `or replace`, `temp`, `definer = 'u'@'h'`, `aggregate`, `constraint`, ...
    for k in i + 1..(i + 16).min(toks.len()) {
        if toks[k].kind == Kind::Semi {
            return false;
        }
        let Some(w) = word(k) else { continue };
        let w = w.to_ascii_lowercase();
        match w.as_str() {
            "function" | "procedure" => return true,
            // PostgreSQL's names a function; MySQL's and SQLite's have a body.
            "trigger" => return d != Dialect::Postgres,
            "event" => return d == Dialect::Mysql,
            "table" | "view" | "index" | "sequence" | "schema" | "database" | "type" | "role" | "user" | "extension" | "domain"
            | "materialized" | "policy" | "rule" | "server" | "tablespace" | "publication" | "subscription" | "cast" | "operator"
            | "transform" | "collation" | "conversion" | "language" | "foreign" | "statistics" => return false,
            _ => {}
        }
    }
    false
}

fn split(d: Dialect, s: &str, toks: &[Tok]) -> Vec<Span> {
    let word = |k: usize| toks.get(k).filter(|t| t.kind == Kind::Word).map(|t| &s[t.start..t.end]);
    let is = |k: usize, w: &str| word(k).map_or(false, |x| x.eq_ignore_ascii_case(w));
    let mut out = Vec::new();
    let (mut from, mut depth, mut body) = (0, 0usize, 0usize);
    let mut routine: Option<bool> = None;
    let mut i = 0;
    while i < toks.len() {
        if routine.is_none() {
            routine = Some(routine_at(d, s, toks, i));
        }
        match toks[i].kind {
            Kind::Open => depth += 1,
            Kind::Close => depth = depth.saturating_sub(1),
            Kind::Semi if depth == 0 && body == 0 => {
                if i > from {
                    out.push(Span { from, to: i, routine: routine == Some(true) });
                }
                from = i + 1;
                routine = None;
            }
            // A body's blocks, as psql counts them: `begin` opens one, `case` too inside one,
            // `end` closes one. MySQL's `end if`, `end loop`, `end while`, `end repeat` and
            // MariaDB's `end for` close what was not counted, and `end case` a `case`.
            Kind::Word if depth == 0 && routine == Some(true) => {
                if is(i, "begin") {
                    body += 1;
                } else if is(i, "case") {
                    if body > 0 {
                        body += 1;
                    }
                } else if is(i, "end") {
                    if d == Dialect::Mysql && ["if", "loop", "while", "repeat", "for"].iter().any(|w| is(i + 1, w)) {
                        i += 1;
                    } else {
                        if d == Dialect::Mysql && is(i + 1, "case") {
                            i += 1;
                        }
                        body = body.saturating_sub(1);
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    if from < toks.len() {
        out.push(Span { from, to: toks.len(), routine: routine == Some(true) });
    }
    out
}

// ---- reading --------------------------------------------------------------------------------

/// A text's tokens and statements while it is read; what is kept is drawn from it (`Scan`).
struct Reader<'a> {
    d: Dialect,
    sql: &'a str,
    toks: Vec<Tok>,
    spans: Vec<Span>,
    /// Comments the text leaves open at its end.
    open: usize,
}

/// Function-like words that are no function: SQL's own, before a `(`.
const NOT_CALLS: &[&str] = &[
    "in", "values", "exists", "any", "all", "some", "over", "filter", "using", "on", "as", "and", "or", "not", "into", "from",
    "join", "where", "conflict", "references", "key", "primary", "unique", "check", "default", "constraint", "partition", "window",
    "array", "row", "cast", "case", "when", "then", "else", "select", "with", "lateral", "returning", "by", "is", "like",
    "between", "set", "rollup", "cube", "within", "group", "table", "index", "type", "do", "return", "execute", "perform", "if",
];

impl<'a> Reader<'a> {
    fn new(d: Dialect, sql: &'a str) -> Reader<'a> {
        let (toks, open) = lex(d, sql);
        let spans = split(d, sql, &toks);
        Reader { d, sql, toks, spans, open }
    }

    fn text(&self, i: usize) -> &'a str {
        let t = self.toks[i];
        &self.sql[t.start..t.end]
    }

    /// Token `i`, if it is the word `w` (in any case).
    fn is_word(&self, i: usize, w: &str) -> bool {
        self.toks.get(i).map_or(false, |t| t.kind == Kind::Word && self.sql[t.start..t.end].eq_ignore_ascii_case(w))
    }

    fn kind(&self, i: usize) -> Option<Kind> {
        self.toks.get(i).map(|t| t.kind)
    }

    /// A statement's first two words, as ranges of the text ((0, 0) for none).
    fn head(&self, sp: &Span) -> [(usize, usize); 2] {
        let w = |k: usize| if k < sp.to && self.toks[k].kind == Kind::Word { (self.toks[k].start, self.toks[k].end) } else { (0, 0) };
        [w(sp.from), w(sp.from + 1)]
    }

    fn head_text(&self, sp: &Span) -> (&'a str, &'a str) {
        let [a, b] = self.head(sp);
        (&self.sql[a.0..a.1], &self.sql[b.0..b.1])
    }

    /// The placeholders, in order: their token and the value (from 1) each takes, numbered as
    /// SQLite numbers them. A query that numbers its own (`$1`) keeps only those, its `?`
    /// jsonb's.
    fn marks(&self) -> (Vec<(usize, usize)>, usize, Vec<Option<(usize, usize)>>, bool) {
        let mut found: Vec<usize> = Vec::new();
        for sp in self.spans.iter().filter(|sp| !sp.routine) {
            found.extend((sp.from..sp.to).filter(|&i| matches!(self.toks[i].kind, Kind::Param(_))));
        }
        let numbered = found.iter().any(|&i| matches!(self.toks[i].kind, Kind::Param(Mark::Dollar(_))));
        if numbered {
            found.retain(|&i| matches!(self.toks[i].kind, Kind::Param(Mark::Dollar(_))));
        }
        let mut largest = 0;
        let mut seen: Vec<(&str, usize)> = Vec::new();
        let mut names: Vec<Option<(usize, usize)>> = Vec::new();
        let mut marks = Vec::with_capacity(found.len());
        for &i in &found {
            let t = self.toks[i];
            let slot = match t.kind {
                Kind::Param(Mark::Question) => largest + 1,
                Kind::Param(Mark::Numbered(n) | Mark::Dollar(n)) => n,
                _ => {
                    let name = &self.sql[t.start..t.end];
                    match seen.iter().find(|(x, _)| *x == name) {
                        Some((_, slot)) => *slot,
                        None => {
                            seen.push((name, largest + 1));
                            names.resize(largest + 1, None);
                            names[largest] = Some((t.start, t.end));
                            largest + 1
                        }
                    }
                }
            };
            largest = largest.max(slot);
            marks.push((i, slot));
        }
        names.resize(largest, None);
        (marks, largest, names, numbered)
    }

    fn controls_tx(&self, sp: &Span) -> bool {
        let (a, b) = self.head_text(sp);
        // MariaDB's `begin not atomic ... end` is a block of statements.
        if self.d == Dialect::Mysql && a.eq_ignore_ascii_case("begin") && b.eq_ignore_ascii_case("not") {
            return false;
        }
        ["begin", "commit", "end", "rollback", "savepoint", "release", "abort"].iter().any(|w| a.eq_ignore_ascii_case(w))
            || (a.eq_ignore_ascii_case("start") && b.eq_ignore_ascii_case("transaction"))
    }

    /// A name as PostgreSQL reads it: a word folded to lower case; a quoted name as written,
    /// its `""` one `"`, and `U&"..."`'s escapes decoded (`U&"set\005fconfig"`; found in review).
    fn name(&self, i: usize) -> Option<String> {
        let t = self.text(i);
        match self.toks[i].kind {
            Kind::Word => Some(t.to_ascii_lowercase()),
            Kind::Quoted => {
                let unicode = t.get(..3).map_or(false, |p| p.eq_ignore_ascii_case("u&\""));
                let rest = if unicode { &t[3..] } else { t.strip_prefix('"')? };
                // One never closed ends with the text.
                let inner = rest.strip_suffix('"').unwrap_or(rest).replace("\"\"", "\"");
                Some(if unicode { unicode_escapes(&inner) } else { inner })
            }
            _ => None,
        }
    }

    /// Whether, on PostgreSQL, tokens `from..to` change the session past their transaction:
    /// a statement, or the text of a `do` block (`depth` deep), where statements also start
    /// after `begin`, `then`, `else` and `loop`. A `set_config` whose third value is given notes
    /// its placeholder's token in `given`.
    fn lasts(&self, from: usize, to: usize, given: &mut Vec<usize>, depth: usize) -> bool {
        let is = |k: usize, w: &str| k < to && self.is_word(k, w);
        let any = |k: usize, ws: &[&str]| ws.iter().any(|w| is(k, w));
        for k in from..to {
            let starts = k == from || (depth > 0 && (self.kind(k - 1) == Some(Kind::Semi) || any(k - 1, &["begin", "then", "else", "loop"])));
            let lasting = starts
                && if is(k, "set") {
                    !any(k + 1, &["local", "transaction", "constraints"])
                } else if is(k, "create") {
                    // `create temp`, `create local temporary`, `create or replace temp view`.
                    (k + 1..(k + 6).min(to)).any(|j| any(j, &["temp", "temporary"]))
                } else if is(k, "declare") {
                    (k..to).any(|j| is(j, "with") && is(j + 1, "hold"))
                } else if is(k, "do") && depth < 2 {
                    // What the block runs, read as statements.
                    (k + 1..to).find(|&j| self.toks[j].kind == Kind::Str).map_or(false, |j| self.body_lasts(self.text(j), depth))
                } else {
                    any(k, &["listen", "load"])
                };
            if lasting {
                return true;
            }
            // SQL a `do` block makes from a string written out: read too.
            if depth > 0 && depth < 4 && is(k, "execute") && self.kind(k + 1) == Some(Kind::Str) && self.body_lasts(self.text(k + 1), depth) {
                return true;
            }
            let Some(name) = self.name(k) else { continue };
            let called = self.kind(k + 1) == Some(Kind::Open);
            // A temporary table by its schema's name (`create table pg_temp.x`; found in review).
            if name == "pg_temp" && self.kind(k + 1) == Some(Kind::Dot) {
                return true;
            }
            if name == "into" && any(k + 1, &["temp", "temporary"]) {
                return true;
            }
            if called && matches!(name.as_str(), "pg_advisory_lock" | "pg_advisory_lock_shared" | "pg_try_advisory_lock" | "pg_try_advisory_lock_shared") {
                return true;
            }
            if called && name == "set_config" {
                match self.set_config_ends(k + 1, to) {
                    Ends::WithTransaction => {}
                    Ends::AsGiven(p) if depth == 0 => given.push(p),
                    _ => return true,
                }
            }
        }
        false
    }

    /// Whether a string's text, read as PostgreSQL statements, changes the session.
    fn body_lasts(&self, quoted: &str, depth: usize) -> bool {
        let text = str_body(quoted);
        let body = Reader::new(Dialect::Postgres, &text);
        let mut inner = Vec::new();
        body.spans.iter().any(|sp| body.lasts(sp.from, sp.to, &mut inner, depth + 1)) || !inner.is_empty()
    }

    /// When what `set_config(...)`, its `(` at token `open`, sets ends: with the transaction
    /// when its third argument is the word `true`; as given, for a placeholder; else with the
    /// session (`false`, `'false'`, anything else).
    fn set_config_ends(&self, open: usize, to: usize) -> Ends {
        let (mut depth, mut arg, mut third) = (0usize, 0usize, Vec::new());
        for k in open + 1..to {
            match self.toks[k].kind {
                Kind::Open => depth += 1,
                Kind::Close if depth == 0 => break,
                Kind::Close => depth -= 1,
                Kind::Comma if depth == 0 => {
                    arg += 1;
                    continue;
                }
                _ => {}
            }
            if arg == 2 {
                third.push(k);
            }
        }
        match third.as_slice() {
            [k] if self.is_word(*k, "true") => Ends::WithTransaction,
            [k] if matches!(self.toks[*k].kind, Kind::Param(_)) => Ends::AsGiven(*k),
            _ => Ends::WithSession,
        }
    }

    /// Whether MySQL commits the transaction around a statement by itself before it runs (a
    /// schema's change, `truncate`, `lock tables`, a user's or a grant's...). A temporary
    /// table's `create` and `drop` do not, nor MariaDB's `analyze select ...`.
    fn commits(&self, sp: &Span) -> bool {
        let (a, b) = self.head_text(sp);
        // No such word is longer than `uninstall`.
        if a.len() > 9 {
            return false;
        }
        let (a, b) = (a.to_ascii_lowercase(), b.to_ascii_lowercase());
        let table_soon = || (sp.from + 1..(sp.from + 4).min(sp.to)).any(|k| self.is_word(k, "table") || self.is_word(k, "tables"));
        match a.as_str() {
            "alter" | "rename" | "truncate" | "grant" | "revoke" | "install" | "uninstall" | "flush" | "cache" | "lock" | "unlock" | "stop"
            | "change" => true,
            "optimize" | "repair" | "analyze" => table_soon(),
            "create" | "drop" => !(sp.from + 1..(sp.from + 5).min(sp.to)).any(|k| self.is_word(k, "temporary")),
            "load" => b == "index",
            "check" => b == "table",
            "reset" => b != "persist",
            "start" => b != "transaction",
            "set" => b == "password",
            _ => false,
        }
    }

    /// Whether PostgreSQL takes the value of the placeholder at token `t` as an array: in
    /// `any(?)`, `all(?)`, `unnest(?)`, `cardinality(?)`, or cast to one (`?::int[]`, `cast(?
    /// as text[])`).
    fn takes_an_array(&self, t: usize) -> bool {
        let call = |name: &str| t >= 2 && self.kind(t - 1) == Some(Kind::Open) && self.is_word(t - 2, name);
        if ["any", "all", "some", "unnest", "cardinality", "array_length"].iter().any(|f| call(f)) {
            return true;
        }
        // Past the type's name (`character varying(10)`, `public."T"`): `[`.
        let array_after = |mut k: usize| {
            while let Some(kind) = self.kind(k) {
                match kind {
                    Kind::Word | Kind::Quoted | Kind::Dot => k += 1,
                    Kind::Open => {
                        while self.kind(k).map_or(false, |x| x != Kind::Close) {
                            k += 1;
                        }
                        k += 1;
                    }
                    _ => break,
                }
            }
            self.kind(k) == Some(Kind::OpenBracket)
        };
        (self.kind(t + 1) == Some(Kind::Cast) && array_after(t + 2)) || (call("cast") && self.is_word(t + 1, "as") && array_after(t + 2))
    }

    /// Whether the SQL only reads, wherever it runs, so a read replica can answer it: one
    /// statement, a `select`, `with`, `values`, `table` or `show`, that writes no table, puts
    /// nothing `into` a table or a variable, locks no row (`for update`, `for share`, `lock in
    /// share mode`), takes no sequence's next value (`next value for`) and calls none of the
    /// databases' own functions that write or keep something in the session (`nextval`,
    /// `pg_advisory_lock`, `get_lock`...). A `select nextval('s')` went to the replica, which
    /// refused it, and took the replica out of use for ten seconds (FEEDBACK55.md, Q-750; branch
    /// feedback55). A function of the program's own is looked up where it is known
    /// (`db::reader`).
    fn reads_only(&self, tables: &crate::sqlinfo::Tables) -> bool {
        let [sp] = self.spans.as_slice() else { return false };
        let first = self.head_text(sp).0;
        if !["select", "with", "values", "table", "show"].iter().any(|w| first.eq_ignore_ascii_case(w)) || !tables.writes.is_empty() {
            return false;
        }
        let is = |k: usize, w: &str| self.is_word(k, w);
        !(sp.from..sp.to).any(|k| {
            (is(k, "for") && (is(k + 1, "update") || is(k + 1, "share") || (is(k + 1, "no") && is(k + 2, "key")) || (is(k + 1, "key") && is(k + 2, "share"))))
                || (is(k, "lock") && is(k + 1, "in") && is(k + 2, "share"))
                || is(k, "into")
                || (is(k, "next") && is(k + 1, "value") && is(k + 2, "for"))
                || (self.kind(k + 1) == Some(Kind::Open) && self.name(k).map_or(false, |n| writes_when_called(&n)))
        })
    }

    /// Whether the placeholder at token `t` is a column's value as written: an item of an
    /// `insert`'s `values (...)`, or the whole right side of `set col = ?` (in an `update`, an
    /// upsert's `do update set`), a cast after it or not. Elsewhere (a function's argument, a
    /// comparison) it is a value of an expression (FEEDBACK55.md, Q-733; branch feedback55).
    fn assigned_at(&self, t: usize) -> bool {
        let Some(sp) = self.spans.iter().find(|sp| sp.from <= t && t < sp.to) else { return false };
        // After it, past a cast (`::jsonb`, `::varchar(10)[]`): where the item ends.
        let mut k = t + 1;
        if self.kind(k) == Some(Kind::Cast) {
            k += 1;
            while matches!(self.kind(k), Some(Kind::Word | Kind::Quoted | Kind::Dot)) {
                k += 1;
            }
            if self.kind(k) == Some(Kind::Open) {
                while k < sp.to && self.kind(k) != Some(Kind::Close) {
                    k += 1;
                }
                k += 1;
            }
            while self.kind(k) == Some(Kind::OpenBracket) && self.kind(k + 1) == Some(Kind::CloseBracket) {
                k += 2;
            }
        }
        if !(k >= sp.to || matches!(self.kind(k), Some(Kind::Comma | Kind::Close | Kind::Word))) || t == sp.from {
            return false;
        }
        // How deep in parentheses it is, and the `(` it is in.
        let (mut depth, mut open) = (0usize, None);
        for j in (sp.from..t).rev() {
            match self.toks[j].kind {
                Kind::Close => depth += 1,
                Kind::Open if depth == 0 => {
                    open = Some(j);
                    break;
                }
                Kind::Open => depth -= 1,
                _ => {}
            }
        }
        let before = self.toks[t - 1].kind;
        match open {
            // `set col = ?`, `set a = 1, col = ?`.
            None => {
                t >= sp.from + 3
                    && before == Kind::Op
                    && self.text(t - 1) == "="
                    && matches!(self.kind(t - 2), Some(Kind::Word | Kind::Quoted))
                    && (self.is_word(t - 3, "set") || (self.kind(t - 3) == Some(Kind::Comma) && (sp.from..t).any(|j| self.is_word(j, "set"))))
            }
            // An item of `values (...)`, the first row's or another's.
            Some(o) => {
                let head = self.is_word(sp.from, "insert") || self.is_word(sp.from, "replace") || self.is_word(sp.from, "with");
                let tuple = o > sp.from && (self.is_word(o - 1, "values") || (self.kind(o - 1) == Some(Kind::Comma) && o >= 2 && self.kind(o - 2) == Some(Kind::Close)));
                head && tuple && matches!(before, Kind::Open | Kind::Comma) && self.kind(k) != Some(Kind::Word)
            }
        }
    }

    /// MySQL: the column the placeholder at token `t` is given to or compared with, as written
    /// beside it: `insert into t (a, b) values (?, ?)`, `set a = ?`, `a = ?`, `? <> a`, `a in (?,
    /// ?)`. Without a column's type a value has none of its own (a BIT's digits, a FLOAT's
    /// range: FEEDBACK55.md, Q-741; found in review, branch feedback55).
    fn column_at(&self, t: usize) -> Option<Box<str>> {
        let sp = self.spans.iter().find(|sp| sp.from <= t && t < sp.to)?;
        // A name as MySQL writes it: a word, or in backticks.
        let col = |k: usize| {
            let t = self.text(k);
            let n = if self.kind(k) == Some(Kind::Quoted) { t.trim_start_matches('`').trim_end_matches('`').replace("``", "`") } else { t.to_string() };
            Some(n.to_lowercase().into_boxed_str())
        };
        let named = |k: usize| matches!(self.kind(k), Some(Kind::Word | Kind::Quoted)) && !["and", "or", "not", "where", "set", "on", "when", "then", "else"].iter().any(|w| self.is_word(k, w));
        // An operator written next to the placeholder, of MySQL's one-character tokens: where it
        // starts (before) or ends (after), if it compares.
        let op = |from: usize, to: usize| ["=", "<>", "!=", "<", ">", "<=", ">=", "<=>"].contains(&&self.sql[self.toks[from].start..self.toks[to].end]);
        let mut b = t;
        while b > sp.from && self.kind(b - 1) == Some(Kind::Op) && (b == t || self.toks[b - 1].end == self.toks[b].start) {
            b -= 1;
        }
        // The whole operand: `a = ? + 1` compares no value of `a`'s with the placeholder's.
        if b < t && op(b, t - 1) && b > sp.from && named(b - 1) && self.kind(t + 1) != Some(Kind::Op) {
            return col(b - 1);
        }
        let mut a = t;
        while self.kind(a + 1) == Some(Kind::Op) && (a == t || self.toks[a].end == self.toks[a + 1].start) {
            a += 1;
        }
        if a > t && op(t + 1, a) && a + 1 < sp.to && named(a + 1) && (t == sp.from || self.kind(t - 1) != Some(Kind::Op)) && self.kind(a + 2) != Some(Kind::Op) {
            return col(a + 1);
        }
        // Inside parentheses: `a in (?, ?)`, or an insert's values by its column list.
        let mut depth = 0usize;
        let mut item = 0usize;
        let mut open = None;
        for j in (sp.from..t).rev() {
            match self.toks[j].kind {
                Kind::Close => depth += 1,
                Kind::Open if depth == 0 => {
                    open = Some(j);
                    break;
                }
                Kind::Open => depth -= 1,
                Kind::Comma if depth == 0 => item += 1,
                _ => {}
            }
        }
        let o = open?;
        if !matches!(self.toks[t - 1].kind, Kind::Open | Kind::Comma) || !matches!(self.kind(t + 1), Some(Kind::Comma | Kind::Close)) {
            return None;
        }
        if o >= sp.from + 2 && self.is_word(o - 1, "in") && named(o - 2) {
            return col(o - 2);
        }
        if !(self.is_word(sp.from, "insert") || self.is_word(sp.from, "replace")) {
            return None;
        }
        let tuple = o > sp.from && (self.is_word(o - 1, "values") || self.is_word(o - 1, "value") || (self.kind(o - 1) == Some(Kind::Comma) && self.kind(o - 2) == Some(Kind::Close)));
        if !tuple {
            return None;
        }
        // The column list: the first `(` of the statement, before `values`.
        let list = (sp.from..o).find(|&j| self.kind(j) == Some(Kind::Open))?;
        let names: Vec<usize> = (list + 1..sp.to).take_while(|&j| self.kind(j) != Some(Kind::Close)).filter(|&j| matches!(self.kind(j), Some(Kind::Word | Kind::Quoted))).collect();
        (list < o).then(|| names.get(item).and_then(|&j| col(j))).flatten()
    }

    /// PostgreSQL: whether it changes what a kept statement may read (`alter`, `drop`, `create
    /// or replace`), and the tables, views and functions it names, in lower case (None: it may
    /// change any, as `drop ... cascade` and `alter schema` do). A new table, index or function
    /// changes none: `create table if not exists`, which Quillo's own tables run often, lets
    /// nothing go (FEEDBACK55.md, Q-745; found in review, branch feedback55).
    fn ddl(&self) -> Option<Option<Vec<Box<str>>>> {
        // What is named after the word; any other (`schema`, `owned`, `role`...): all.
        const OBJECTS: &[&str] = &["table", "view", "function", "procedure", "routine", "aggregate", "type", "domain", "sequence", "index", "trigger", "rule", "policy", "statistics"];
        let name = |k: usize| self.name(k).map(|n| n.to_lowercase().into_boxed_str());
        let mut names = Vec::new();
        let mut any = false;
        for sp in &self.spans {
            let (a, b) = self.head_text(sp);
            let mut k = sp.from + 1;
            if a.eq_ignore_ascii_case("create") && b.eq_ignore_ascii_case("or") && self.is_word(k + 1, "replace") {
                k += 2;
            } else if !a.eq_ignore_ascii_case("alter") && !a.eq_ignore_ascii_case("drop") {
                continue;
            }
            any = true;
            while ["materialized", "foreign", "temp", "temporary", "recursive", "trusted", "procedural", "unlogged"].iter().any(|w| self.is_word(k, w)) {
                k += 1;
            }
            let kind = self.toks.get(k).filter(|t| t.kind == Kind::Word).map(|t| self.sql[t.start..t.end].to_ascii_lowercase());
            if !kind.as_deref().map_or(false, |w| OBJECTS.contains(&w)) || (sp.from..sp.to).any(|j| self.is_word(j, "cascade")) {
                return Some(None);
            }
            k += 1;
            while ["if", "not", "exists", "only", "concurrently"].iter().any(|w| self.is_word(k, w)) {
                k += 1;
            }
            // `a`, `s.a`, `a, b` (a drop's list), `f(int)`.
            loop {
                let mut last = None;
                while matches!(self.kind(k), Some(Kind::Word | Kind::Quoted)) {
                    last = name(k);
                    k += 1;
                    if self.kind(k) != Some(Kind::Dot) {
                        break;
                    }
                    k += 1;
                }
                names.extend(last);
                if self.kind(k) == Some(Kind::Open) {
                    let mut depth = 0;
                    while k < sp.to {
                        match self.toks[k].kind {
                            Kind::Open => depth += 1,
                            Kind::Close => depth -= 1,
                            _ => {}
                        }
                        k += 1;
                        if depth == 0 {
                            break;
                        }
                    }
                }
                if self.kind(k) != Some(Kind::Comma) || k >= sp.to {
                    break;
                }
                k += 1;
            }
            // `alter table a rename to b`: b too.
            for j in sp.from..sp.to {
                if self.is_word(j, "rename") {
                    if let Some(to) = (j + 1..sp.to).find(|&t| self.is_word(t, "to")) {
                        names.extend(name(to + 1));
                    }
                }
            }
        }
        any.then_some(Some(names))
    }

    /// MySQL's `@name` variables (not `@@` the server's), as ranges of the text without the `@`
    /// (and the quotes of `@'name'`).
    fn vars(&self) -> Vec<(usize, usize)> {
        self.toks
            .iter()
            .filter(|t| t.kind == Kind::Var && !self.sql[t.start..t.end].starts_with("@@"))
            .map(|t| {
                let quoted = matches!(self.sql.as_bytes().get(t.start + 1), Some(b'\'' | b'"' | b'`'));
                if quoted && t.end >= t.start + 3 {
                    (t.start + 2, t.end - 1)
                } else {
                    (t.start + 1, t.end)
                }
            })
            .collect()
    }

    /// `insert ... values (?, ...)` with nothing after its values, as one statement, its only
    /// placeholders in that tuple: where its `values` starts, and how many `?` it has. Read from
    /// the tokens, where it was read from the text, which a string or a comment could mislead.
    fn tuple(&self) -> Option<(usize, usize)> {
        let [sp] = self.spans.as_slice() else { return None };
        if !self.is_word(sp.from, "insert") || sp.to < sp.from + 3 || self.kind(sp.to - 1) != Some(Kind::Close) {
            return None;
        }
        let at = (sp.from..sp.to).rev().find(|&k| self.is_word(k, "values"))?;
        if self.kind(at + 1) != Some(Kind::Open) {
            return None;
        }
        // `(?, ?, ...)`, its `)` the statement's last token.
        let mut n = 0;
        for (j, k) in (at + 2..sp.to - 1).enumerate() {
            let want = if j % 2 == 0 { Kind::Param(Mark::Question) } else { Kind::Comma };
            if self.toks[k].kind != want {
                return None;
            }
            n += (j % 2 == 0) as usize;
        }
        let before = (sp.from..at).any(|k| matches!(self.toks[k].kind, Kind::Param(_)));
        (n > 0 && (sp.to - at - 3) % 2 == 1 && !before).then(|| (self.toks[at].start, n))
    }

    /// SQLite: the columns a `create table` or an `alter table ... add` gives `money`, `decimal`,
    /// `numeric(p, s)` or `dec`, which it keeps with NUMERIC affinity, as (table, column, type):
    /// read from the tokens, where comments and strings are none (FEEDBACK50.md, Q-628).
    fn lossy(&self) -> Vec<(Box<str>, Box<str>, Box<str>)> {
        let unquote = |t: &str| t.trim_matches(|c| matches!(c, '"' | '`' | '[' | ']')).into();
        // A name, `schema.name` too, at `k`: as written, and where it ends.
        let name_at = |mut k: usize| -> Option<(Box<str>, usize)> {
            let from = k;
            while matches!(self.kind(k), Some(Kind::Word | Kind::Quoted)) {
                k += 1;
                if self.kind(k) != Some(Kind::Dot) {
                    break;
                }
                k += 1;
            }
            (k > from).then(|| (unquote(&self.sql[self.toks[from].start..self.toks[k - 1].end]), k))
        };
        let lossy_type = |k: usize| -> Option<Box<str>> {
            let w = self.text(k).to_ascii_lowercase();
            let open = self.kind(k + 1) == Some(Kind::Open);
            (self.kind(k) == Some(Kind::Word) && (w.starts_with("money") || w == "decimal" || w == "dec" || (w == "numeric" && open))).then(|| w.into())
        };
        let mut out = Vec::new();
        for sp in &self.spans {
            let mut k = sp.from;
            if self.is_word(k, "create") {
                k += 1;
                if self.is_word(k, "temp") || self.is_word(k, "temporary") {
                    k += 1;
                }
                if !self.is_word(k, "table") {
                    continue;
                }
                k += 1;
                if self.is_word(k, "if") && self.is_word(k + 1, "not") && self.is_word(k + 2, "exists") {
                    k += 3;
                }
                let Some((table, mut k)) = name_at(k) else { continue };
                if self.kind(k) != Some(Kind::Open) {
                    continue;
                }
                // Each column, after the table's `(` or a `,` directly inside it.
                let mut depth = 0;
                while k < sp.to {
                    match self.toks[k].kind {
                        Kind::Open => depth += 1,
                        Kind::Close => depth -= 1,
                        _ => {}
                    }
                    if depth == 1 && matches!(self.toks[k].kind, Kind::Open | Kind::Comma) {
                        if let Some((col, at)) = name_at(k + 1).filter(|(_, at)| *at == k + 2) {
                            if let Some(ty) = lossy_type(at) {
                                out.push((table.clone(), col, ty));
                            }
                        }
                    }
                    k += 1;
                }
            } else if self.is_word(k, "alter") && self.is_word(k + 1, "table") {
                let Some((table, k)) = name_at(k + 2) else { continue };
                if !self.is_word(k, "add") {
                    continue;
                }
                let k = if self.is_word(k + 1, "column") { k + 2 } else { k + 1 };
                if let Some((col, at)) = name_at(k).filter(|(_, at)| *at == k + 1) {
                    if let Some(ty) = lossy_type(at) {
                        out.push((table, col, ty));
                    }
                }
            }
        }
        out
    }
}

/// The databases' own functions that write, or keep something in the session a later statement
/// reads, which only the primary can run (see `Reader::reads_only`): PostgreSQL's sequences,
/// locks, settings, notices, large objects, transaction ids and WAL; MySQL's and MariaDB's
/// sequences and locks, and the session's last id and counts.
fn writes_when_called(name: &str) -> bool {
    const NAMES: &[&str] = &[
        "nextval", "setval", "currval", "lastval", "set_config", "pg_notify", "setseed", "txid_current", "txid_current_if_assigned", "pg_switch_wal",
        "pg_reload_conf", "pg_rotate_logfile", "pg_terminate_backend", "pg_cancel_backend", "pg_promote", "pg_import_system_collations", "dblink_exec",
        "get_lock", "release_lock", "release_all_locks", "last_insert_id", "found_rows", "row_count",
    ];
    const STARTS: &[&str] = &["pg_advisory", "pg_try_advisory", "lo_", "pg_stat_reset", "pg_create_", "pg_drop_", "pg_replication_", "pg_logical_", "pg_current_", "pg_backup_", "pg_start_backup", "pg_stop_backup"];
    NAMES.contains(&name) || STARTS.iter().any(|p| name.starts_with(p))
}

/// `U&"..."`'s text decoded: `\XXXX`, `\+XXXXXX` and `\\`.
fn unicode_escapes(t: &str) -> String {
    let mut out = String::with_capacity(t.len());
    let mut rest = t;
    while let Some(at) = rest.find('\\') {
        out.push_str(&rest[..at]);
        rest = &rest[at + 1..];
        let (digits, len) = if rest.starts_with('+') { (rest.get(1..7), 7) } else { (rest.get(..4), 4) };
        match digits.and_then(|h| u32::from_str_radix(h, 16).ok()).and_then(char::from_u32) {
            Some(c) => {
                out.push(c);
                rest = &rest[len..];
            }
            None if rest.starts_with('\\') => {
                out.push('\\');
                rest = &rest[1..];
            }
            None => out.push('\\'),
        }
    }
    out.push_str(rest);
    out
}

// ---- what is kept ---------------------------------------------------------------------------

impl Scan {
    fn new(d: Dialect, sql: &str) -> Scan {
        let r = Reader::new(d, sql);
        let (marks, count, names, numbered) = r.marks();
        let pg = d == Dialect::Postgres;
        let rewrite = |to: &dyn Fn(usize) -> String| {
            let mut out = String::with_capacity(sql.len() + 8);
            let mut at = 0;
            for &(i, slot) in &marks {
                let t = r.toks[i];
                out.push_str(&sql[at..t.start]);
                out.push_str(&to(slot));
                at = t.end;
            }
            out.push_str(&sql[at..]);
            out
        };
        let (mut native, mut order) = (None, None);
        match d {
            Dialect::Postgres if !numbered && !marks.is_empty() => native = Some(rewrite(&|slot| format!("${}", slot))),
            Dialect::Mysql if marks.iter().any(|&(i, _)| !matches!(r.toks[i].kind, Kind::Param(Mark::Question))) => {
                native = Some(rewrite(&|_| "?".to_string()));
                order = Some(marks.iter().map(|&(_, slot)| slot).collect::<Box<[usize]>>());
            }
            _ => {}
        }
        // A comment left open ends with the text, as on SQLite: PostgreSQL refuses it.
        if pg && r.open > 0 {
            let mut text = native.unwrap_or_else(|| sql.to_string());
            text.push_str(&"*/".repeat(r.open));
            native = Some(text);
        }
        // PostgreSQL: what changes the session, and what may where this cannot see.
        let (mut lasting, mut given, mut unseen, mut calls, mut defines) = (false, Vec::new(), false, Vec::<Box<str>>::new(), false);
        if pg {
            let mut at = Vec::new();
            // Making a function runs nothing of its body.
            lasting = r.spans.iter().filter(|sp| !sp.routine).any(|sp| r.lasts(sp.from, sp.to, &mut at, 0));
            given = at.iter().filter_map(|t| marks.iter().position(|(m, _)| m == t)).collect();
            if given.len() < at.len() {
                lasting = true;
            }
        }
        // The functions it calls, PostgreSQL's and MySQL's (whose own may write, which a replica
        // must not be asked: FEEDBACK55.md, Q-750; found in review, branch feedback55).
        if d != Dialect::Sqlite {
            for sp in &r.spans {
                let a = r.head_text(sp).0.to_ascii_lowercase();
                unseen |= pg && a == "do";
                defines |= matches!(a.as_str(), "create" | "drop" | "alter")
                    && (sp.from + 1..(sp.from + 5).min(sp.to)).any(|k| ["function", "procedure", "routine", "extension"].iter().any(|w| r.is_word(k, w)));
                if sp.routine {
                    continue;
                }
                for k in sp.from..sp.to {
                    if r.kind(k + 1) != Some(Kind::Open) {
                        continue;
                    }
                    // MySQL's names in any case are one.
                    let name = if pg { r.name(k) } else { r.name(k).map(|n| n.to_lowercase()) };
                    if let Some(n) = name {
                        if !(r.toks[k].kind == Kind::Word && NOT_CALLS.contains(&n.as_str())) && !calls.iter().any(|c| **c == *n) {
                            calls.push(n.into_boxed_str());
                        }
                    }
                }
            }
        }
        let tx_alone = r.spans.len() == 1 && r.controls_tx(&r.spans[0]);
        let tx_any = r.spans.iter().any(|sp| r.controls_tx(sp));
        let prepares = r.spans.iter().any(|sp| r.head_text(sp).0.eq_ignore_ascii_case("prepare"));
        let commits = if d == Dialect::Mysql {
            r.spans.iter().find(|sp| r.commits(sp)).map(|sp| (r.toks[sp.from].start, r.toks[sp.to - 1].end))
        } else {
            None
        };
        let array_first = pg && marks.first().map_or(false, |&(t, _)| r.takes_an_array(t));
        let tables = crate::sqlinfo::tables_in(sql, &r.toks);
        let reads_only = r.reads_only(&tables);
        let vars = if d == Dialect::Mysql { r.vars() } else { Vec::new() };
        let tuple = r.tuple();
        let bars = d == Dialect::Mysql && r.toks.windows(2).any(|w| w[0].end == w[1].start && &sql[w[0].start..w[1].end] == "||");
        let ddl = if pg { r.ddl() } else { None };
        let mut columns: Vec<Option<Box<str>>> = Vec::new();
        if d == Dialect::Mysql {
            columns.resize(count, None);
            for &(t, slot) in &marks {
                if slot >= 1 && slot <= count && columns[slot - 1].is_none() {
                    columns[slot - 1] = r.column_at(t);
                }
            }
        }
        let mut assigned = Vec::new();
        if pg {
            assigned.resize(count, false);
            for &(t, slot) in &marks {
                if slot >= 1 && slot <= count && r.assigned_at(t) {
                    assigned[slot - 1] = true;
                }
            }
        }
        let twice = d == Dialect::Mysql
            && r.spans.len() == 1
            && (r.is_word(r.spans[0].from, "replace")
                || (r.is_word(r.spans[0].from, "insert") && (r.spans[0].from..r.spans[0].to).any(|k| r.is_word(k, "duplicate") && r.is_word(k + 1, "key") && r.is_word(k + 2, "update"))));
        let lossy = if d == Dialect::Sqlite && r.spans.iter().any(|sp| r.is_word(sp.from, "create") || r.is_word(sp.from, "alter")) { r.lossy() } else { Vec::new() };
        let stmts = r.spans.iter().map(|sp| Stmt { start: r.toks[sp.from].start, end: r.toks[sp.to - 1].end, routine: sp.routine, head: r.head(sp) }).collect();
        let marks = marks.iter().map(|&(i, slot)| (r.toks[i].start, r.toks[i].end, slot)).collect();
        Scan {
            sql: Arc::from(sql),
            dialect: d,
            stmts,
            marks,
            count,
            named: names.iter().any(Option::is_some),
            names,
            native: native.map(String::into_boxed_str),
            order,
            tx_alone,
            tx_any,
            prepares,
            lasting,
            given,
            unseen,
            calls,
            defines,
            commits,
            array_first,
            tables,
            reads_only,
            vars,
            tuple,
            lossy,
            twice,
            bars,
            assigned,
            ddl,
            columns,
        }
    }

    /// About what it holds, in bytes, for the cache's bound.
    fn size(&self) -> usize {
        256 + self.sql.len()
            + self.native.as_ref().map_or(0, |n| n.len())
            + self.marks.len() * 24
            + self.names.len() * 24
            + self.order.as_ref().map_or(0, |o| o.len() * 8)
            + self.stmts.len() * std::mem::size_of::<Stmt>()
            + self.calls.iter().map(|c| c.len() + 16).sum::<usize>()
            + self.tables.reads.iter().chain(&self.tables.writes).map(|t| t.len() + 24).sum::<usize>()
            + self.vars.len() * 16
            + self.assigned.len()
            + self.columns.iter().flatten().map(|c| c.len() + 24).sum::<usize>()
            + self.ddl.iter().flatten().flatten().map(|n| n.len() + 16).sum::<usize>()
            + self.lossy.iter().map(|(a, b, c)| a.len() + b.len() + c.len() + 48).sum::<usize>()
    }

    /// The SQL as the database takes it.
    pub fn native(&self) -> &str {
        self.native.as_deref().unwrap_or(&self.sql)
    }

    /// The placeholders: where each is in the text, and the value (from 1) it takes.
    pub fn marks(&self) -> &[(usize, usize, usize)] {
        &self.marks
    }

    /// The text of a statement, as written.
    pub fn stmt_text(&self, st: &Stmt) -> &str {
        &self.sql[st.start..st.end]
    }

    /// Each statement's first word, and its second ("" for none).
    pub fn heads(&self) -> impl Iterator<Item = (&str, &str)> {
        self.stmts.iter().map(|st| (&self.sql[st.head[0].0..st.head[0].1], &self.sql[st.head[1].0..st.head[1].1]))
    }

    /// The first statement's first word, and its second ("" for none).
    pub fn first_words(&self) -> Option<(&str, &str)> {
        self.heads().next().filter(|(a, _)| !a.is_empty())
    }

    /// A value's name (`:id` for `:id`), from 1.
    fn name_of(&self, slot: usize) -> Option<&str> {
        self.names.get(slot - 1).copied().flatten().map(|(a, b)| &self.sql[a..b])
    }

    /// The names the SQL takes its values by, as written (`:id`, SQLite's `@id`).
    pub fn signed_names(&self) -> impl Iterator<Item = &str> {
        self.names.iter().flatten().map(|&(a, b)| &self.sql[a..b])
    }

    /// The value of a named placeholder in a map of values: by its name, or as written
    /// (`":id"`).
    pub fn by_name<'a>(m: &'a Map, name: &str) -> Option<&'a Value> {
        m.get(&name[1..]).or_else(|| m.get(name))
    }

    /// Whether `params` are the values by name: one map, where the SQL names its values.
    pub fn by_names<'a>(&self, params: &'a [Value]) -> Option<&'a Map> {
        match params {
            [Value::Map(m)] if self.named => Some(m),
            _ => None,
        }
    }

    /// The values in the order the database takes them, from the program's: one map gives
    /// them by name where the SQL names them (`:id`); one map where the SQL has no placeholder
    /// is not a value (FEEDBACK55.md, Q-747, Q-748; branch feedback55); else they are taken by
    /// position, as many as the SQL has.
    pub fn values(&self, params: &[Value]) -> Result<Vec<Value>, String> {
        if let Some(m) = self.by_names(params) {
            let get = |slot: usize| match self.name_of(slot) {
                Some(name) => Scan::by_name(m, name).cloned().ok_or_else(|| format!("no value for {}", name)),
                None => Err(format!("no value for ?{}", slot)),
            };
            return match &self.order {
                Some(o) => o.iter().map(|&s| get(s)).collect(),
                None => (1..=self.count).map(get).collect(),
            };
        }
        if matches!(params, [Value::Map(_)]) && self.count == 0 {
            return Ok(Vec::new());
        }
        if params.len() != self.count {
            return Err(wrong_count(self.count, params.len()));
        }
        Ok(match &self.order {
            None => params.to_vec(),
            Some(o) => o.iter().map(|s| params[s - 1].clone()).collect(),
        })
    }

    /// The value placeholder `m` (its place in `marks`) takes from `params`.
    fn value_of<'a>(&self, m: usize, params: &'a [Value]) -> Option<&'a Value> {
        let slot = self.marks.get(m)?.2;
        match self.by_names(params) {
            Some(map) => Scan::by_name(map, self.name_of(slot)?),
            None => params.get(slot - 1),
        }
    }

    /// Whether the SQL begins or ends a transaction by hand: the one statement it is, or, with
    /// `anywhere`, any of its statements.
    pub fn tx_control(&self, anywhere: bool) -> bool {
        if anywhere {
            self.tx_any
        } else {
            self.tx_alone
        }
    }

    /// A statement that starts with `prepare` (PostgreSQL's statement made by name).
    pub fn prepares(&self) -> bool {
        self.prepares
    }

    /// Whether, on PostgreSQL, the SQL changes the session past its transaction, where the
    /// next request on the connection reads it: `set` (not `set local`), `listen`, `load`, a
    /// temporary table (`create temp`, `select ... into temp`, `pg_temp.x`), a cursor `with
    /// hold`, a session's advisory lock, and `set_config(name, value, is_local)` by any of its
    /// names anywhere in any statement, unless `is_local` is the word `true` or a value given as
    /// `true` (FEEDBACK44.md, Q-581; FEEDBACK55.md, Q-708; branch feedback55). What a `do` block
    /// runs is read too, and the SQL it makes from text written out.
    pub fn changes_session(&self, params: &[Value]) -> bool {
        self.lasting || self.given.iter().any(|&m| !matches!(self.value_of(m, params), Some(Value::Bool(true))))
    }

    /// The first statement MySQL commits the transaction around by itself before it runs:
    /// inside `db.transaction`, what ran before it could no longer be rolled back
    /// (FEEDBACK55.md, Q-728; branch feedback55).
    pub fn commits_by_itself(&self) -> Option<&str> {
        self.commits.map(|(a, b)| &self.sql[a..b])
    }

    /// Whether PostgreSQL takes the first placeholder's value as an array.
    pub fn first_takes_an_array(&self) -> bool {
        self.array_first
    }

    /// Whether it only reads, wherever it runs (a read replica's answer is the primary's): see
    /// `Reader::reads_only` (FEEDBACK55.md, Q-750; branch feedback55).
    pub fn reads_only(&self) -> bool {
        self.reads_only
    }

    /// It holds no statement: empty, blank, or comments alone (FEEDBACK55.md, Q-753).
    pub fn is_empty(&self) -> bool {
        self.stmts.is_empty()
    }

    /// One statement that reads rows and changes none: its `changes` is 0 on every database
    /// (PostgreSQL counted a `select`'s rows; FEEDBACK55.md, Q-753; branch feedback55).
    pub fn reads(&self) -> bool {
        self.stmts.len() == 1 && self.first_words().map_or(false, |(a, _)| reading_word(a))
    }

    /// One `insert`, `update`, `delete` or `replace` (a `with` before it too): when it changed
    /// no row, nothing changed that a page could show (FEEDBACK13.md, Q-090). Read from its
    /// statements, where a `;` in a string made two (FEEDBACK55.md; branch feedback55).
    pub fn changes_rows(&self) -> bool {
        self.stmts.len() == 1 && self.first_words().map_or(false, |(a, _)| ["insert", "update", "delete", "replace", "with"].iter().any(|w| a.eq_ignore_ascii_case(w)))
    }

    /// MySQL: the first of the SQL's `@name` variables that `params`, one map of values, has a
    /// value for. MySQL reads `@name` as a variable of the session, which is null unless set,
    /// where SQLite reads it as a value's name: a map's `name` would be silently left out
    /// (FEEDBACK55.md, Q-752; branch feedback55).
    pub fn var_given<'a>(&'a self, params: &[Value]) -> Option<&'a str> {
        let [Value::Map(m)] = params else { return None };
        self.vars.iter().map(|&(a, b)| &self.sql[a..b]).find(|v| m.get(*v).is_some() || m.get(format!("@{}", v).as_str()).is_some())
    }

    /// MySQL: one `replace` or `insert ... on duplicate key update`, whose rows MySQL counts
    /// twice when it replaces or updates them (`my::Conn::exec` counts them once).
    pub fn counts_twice(&self) -> bool {
        self.twice
    }

    /// The value (from 1) the database's own placeholder `i` (from 0) takes.
    pub fn native_slot(&self, i: usize) -> usize {
        self.order.as_ref().map_or(i + 1, |o| o.get(i).copied().unwrap_or(0))
    }

    /// MySQL: the column value `slot` (from 1) is given to or compared with, in lower case (see
    /// `Reader::column_at`).
    pub fn column(&self, slot: usize) -> Option<&str> {
        self.columns.get(slot.wrapping_sub(1)).and_then(|c| c.as_deref())
    }

    /// PostgreSQL: whether it changes what a kept statement may read, and the names it changes
    /// (None: any; see `Reader::ddl`).
    pub fn ddl(&self) -> Option<Option<&[Box<str>]>> {
        self.ddl.as_ref().map(|n| n.as_deref())
    }

    /// PostgreSQL: whether value `slot` (from 1) is a column's value as written, where a str
    /// for a json column is the JSON text it holds (`Reader::assigned_at`).
    pub fn assigned(&self, slot: usize) -> bool {
        self.assigned.get(slot.wrapping_sub(1)).copied().unwrap_or(false)
    }

    /// MySQL: the SQL has `||`, which joins text on the other databases (see `my::Conn::bars`).
    pub fn has_bars(&self) -> bool {
        self.bars
    }

    /// `insert into t (a, b) values (?, ?)` with nothing after its values, as many `?` as
    /// `width`: the statement up to `values`, and `values `, to which more rows' `(?, ?)` can be
    /// added. None for any other statement, which runs once a row.
    pub fn values_tuple(&self, width: usize) -> Option<String> {
        let (at, _) = self.tuple.filter(|(_, n)| *n == width)?;
        Some(format!("{}values ", &self.sql[..at]))
    }
}

/// A statement's first word when it reads rows and changes none.
pub fn reading_word(w: &str) -> bool {
    ["select", "values", "table", "show"].iter().any(|x| w.eq_ignore_ascii_case(x))
}

/// Whether Quillo's own statements (`batch`) leave a connection in a transaction: `begin`
/// and `start transaction` begin one (not MariaDB's `begin not atomic`); `commit`, `end`,
/// `abort`, `rollback` (not `rollback to`) and `prepare transaction` end it.
pub fn in_tx_after(scan: &Scan, mut in_tx: bool) -> bool {
    for (a, b) in scan.heads() {
        let is = |w: &str, x: &str| w.eq_ignore_ascii_case(x);
        if (is(a, "begin") && !is(b, "not")) || (is(a, "start") && is(b, "transaction")) {
            in_tx = true;
        } else if (is(a, "commit") && !is(b, "prepared")) || is(a, "end") || is(a, "abort") || (is(a, "rollback") && !is(b, "to") && !is(b, "prepared")) || (is(a, "prepare") && is(b, "transaction")) {
            in_tx = false;
        }
    }
    in_tx
}

/// The placeholders PostgreSQL's error at byte `at` of `sql` (its `$n` form) is about: those
/// inside the innermost parentheses around it, or every one where it is in none. As `$n`'s n.
pub fn values_near(sql: &str, at: usize) -> Vec<usize> {
    let (toks, _) = lex(Dialect::Postgres, sql);
    let (mut from, mut to) = (0, toks.len());
    let mut opened: Vec<usize> = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        match t.kind {
            Kind::Open => opened.push(i),
            Kind::Close => {
                if let Some(o) = opened.pop() {
                    if toks[o].start < at && at < t.end && i - o < to - from {
                        (from, to) = (o, i);
                    }
                }
            }
            _ => {}
        }
    }
    toks[from..to].iter().filter_map(|t| if let Kind::Param(Mark::Dollar(n)) = t.kind { Some(n) } else { None }).collect()
}

/// What a string token holds: `$tag$...$tag$`, `'...'`, `E'...'`, without its quotes.
fn str_body(t: &str) -> String {
    if t.starts_with('$') {
        let open = t[1..].find('$').map_or(t.len(), |k| k + 2);
        let close = if t.len() >= 2 * open && t.ends_with(&t[..open]) { t.len() - open } else { t.len() };
        return t[open.min(close)..close].to_string();
    }
    let t = t.trim_start_matches(|c: char| c != '\'');
    t.strip_prefix('\'').map(|r| r.strip_suffix('\'').unwrap_or(r)).unwrap_or(t).replace("''", "'")
}

pub fn wrong_count(want: usize, got: usize) -> String {
    format!("the query has {} parameter{}, got {}", want, if want == 1 { "" } else { "s" }, got)
}

// ---- kept ------------------------------------------------------------------------------

/// A hash of the text alone, short work for SQL (FxHash's step); the texts are the program's.
#[derive(Default)]
struct Fx(u64);

impl std::hash::Hasher for Fx {
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for c in &mut chunks {
            self.add(u64::from_le_bytes(c.try_into().unwrap()));
        }
        let rest = chunks.remainder();
        if !rest.is_empty() {
            let mut w = [0u8; 8];
            w[..rest.len()].copy_from_slice(rest);
            self.add(u64::from_le_bytes(w));
        }
    }

    fn write_u8(&mut self, i: u8) {
        self.add(i as u64);
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

impl Fx {
    fn add(&mut self, w: u64) {
        self.0 = (self.0.rotate_left(5) ^ w).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
}

type Kept = std::collections::HashMap<Arc<str>, Arc<Scan>, std::hash::BuildHasherDefault<Fx>>;

/// What is kept for each dialect, in bytes (`Scan::size`); past it, it starts again. Texts
/// made with values written in them (`in (1, 2, ...)`), each kept, took 800 MB (found in
/// review).
const KEPT_BYTES: usize = 4 << 20;
/// A text whose reading is larger than this is read each time, not kept.
const LARGEST_KEPT: usize = KEPT_BYTES / 16;
/// The texts each thread used last, looked at first: a loop's statement is found without a
/// lock. Only small ones, so that each thread holds little.
const RECENT: usize = 8;
const LARGEST_RECENT: usize = 16 << 10;

static SEEN: std::sync::Mutex<Option<[(Kept, usize); 3]>> = std::sync::Mutex::new(None);

thread_local! {
    static RECENTLY: std::cell::RefCell<[Vec<Arc<Scan>>; 3]> = Default::default();
}

/// `sql` as `d` reads it: read once, then kept.
pub fn scan(d: Dialect, sql: &str) -> Arc<Scan> {
    if sql.len() > LARGEST_KEPT {
        return Arc::new(Scan::new(d, sql));
    }
    if let Some(s) = RECENTLY.with(|r| r.borrow()[d as usize].iter().find(|s| &*s.sql == sql).cloned()) {
        return s;
    }
    let kept = SEEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref().and_then(|m| m[d as usize].0.get(sql).cloned());
    let s = match kept {
        Some(s) => s,
        // Read without the lock, which other threads' look-ups wait for.
        None => {
            let s = Arc::new(Scan::new(d, sql));
            let size = s.size();
            if size <= LARGEST_KEPT {
                let mut g = SEEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let (m, bytes) = &mut g.get_or_insert_with(Default::default)[d as usize];
                if *bytes + size > KEPT_BYTES {
                    m.clear();
                    *bytes = 0;
                }
                if m.insert(s.sql.clone(), s.clone()).is_none() {
                    *bytes += size;
                }
            }
            s
        }
    };
    if s.size() <= LARGEST_RECENT {
        RECENTLY.with(|r| {
            let mut r = r.borrow_mut();
            let v = &mut r[d as usize];
            v.truncate(RECENT - 1);
            v.insert(0, s.clone());
        });
    }
    s
}

// ---- for `quillo check`, where the database is not known -------------------------------

/// SQL that begins or ends a transaction, one statement alone, as every database reads it
/// (`quillo check` does not know which the program uses).
pub fn tx_control_everywhere(sql: &str) -> bool {
    Dialect::ALL.iter().all(|d| scan(*d, sql).tx_control(false))
}

/// For `quillo check`'s warning of a list given as a statement's values (FEEDBACK50.md,
/// Q-629): how many values SQL written out takes, the fewest any database reads in it; and
/// none where PostgreSQL takes the first as an array (`= any(?)`, `?::int[]`), where one
/// list is right (FEEDBACK55.md, Q-755; branch feedback55).
pub fn values_for_a_list(sql: &str) -> usize {
    if scan(Dialect::Postgres, sql).first_takes_an_array() {
        return 0;
    }
    Dialect::ALL.iter().map(|d| scan(*d, sql).count).min().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(d: Dialect, sql: &str) -> Vec<(Kind, String)> {
        lex(d, sql).0.iter().map(|t| (t.kind, sql[t.start..t.end].to_string())).collect()
    }

    fn native(d: Dialect, sql: &str) -> String {
        Scan::new(d, sql).native().to_string()
    }

    fn stmts(d: Dialect, sql: &str) -> Vec<String> {
        let s = Scan::new(d, sql);
        s.stmts.iter().map(|st| s.stmt_text(st).to_string()).collect()
    }

    #[test]
    fn strings_names_and_comments_by_dialect() {
        use Dialect::*;
        let strs = |d, sql: &str| kinds(d, sql).into_iter().filter(|(k, _)| *k == Kind::Str).map(|(_, t)| t).collect::<Vec<_>>();
        // `\'` ends nothing on MySQL; elsewhere the string ends at it.
        assert_eq!(strs(Mysql, r"select 'a\'b', 'c'"), vec![r"'a\'b'", "'c'"]);
        assert_eq!(strs(Postgres, r"select 'a\', 'c'"), vec![r"'a\'", "'c'"]);
        assert_eq!(strs(Postgres, r"select E'a\'b', e'\\', U&'d\0061t', x'1F'"), vec![r"E'a\'b'", r"e'\\'", r"U&'d\0061t'", "x'1F'"]);
        assert_eq!(strs(Postgres, "select $$a;b$$, $f$ $$ ; $f$, 'it''s'"), vec!["$$a;b$$", "$f$ $$ ; $f$", "'it''s'"]);
        assert_eq!(strs(Mysql, r#"select "a\"b", "c""d""#), vec![r#""a\"b""#, r#""c""d""#]);
        assert!(strs(Postgres, r#"select "a""b""#).is_empty());
        assert_eq!(kinds(Sqlite, "select [a ? b], `c`")[1], (Kind::Quoted, "[a ? b]".into()));
        // Comments: nested on PostgreSQL; MySQL's `--` needs a space, and `#`.
        assert_eq!(kinds(Postgres, "a /* x /* y */ ? */ b").len(), 2);
        assert_eq!(kinds(Sqlite, "a /* x /* y */ b */ c").len(), 5);
        assert_eq!(kinds(Mysql, "a --x\nb").len(), 5);
        assert_eq!(kinds(Mysql, "a -- x\nb # c\n").len(), 2);
        // MySQL runs what `/*! */` holds.
        assert_eq!(kinds(Mysql, "select /*!50000 1, */ 2").len(), 4);
        // Unicode in names and strings, never split.
        assert_eq!(kinds(Postgres, "select ünï, 'é?'"), vec![(Kind::Word, "select".into()), (Kind::Word, "ünï".into()), (Kind::Comma, ",".into()), (Kind::Str, "'é?'".into())]);
        // Unterminated: to the end.
        assert_eq!(strs(Postgres, "select $$ never"), vec!["$$ never"]);
        assert_eq!(kinds(Postgres, "select /* never").len(), 1);
        // A name never closed, its last letter not ASCII (found in review).
        assert!(!Scan::new(Postgres, "select \"set_config(ü").changes_session(&[]));
        assert!(!Scan::new(Postgres, "select U&\"aü").changes_session(&[]));
        // A comment left open is closed for PostgreSQL, as many deep as it is.
        assert_eq!(native(Postgres, "select ? /* open /* deeper"), "select $1 /* open /* deeper*/*/");
        assert_eq!(native(Postgres, "select 1 /* open"), "select 1 /* open*/");
        assert_eq!(native(Sqlite, "select 1 /* open"), "select 1 /* open");
    }

    #[test]
    fn question_marks_on_postgresql() {
        use Dialect::Postgres as P;
        // FEEDBACK55.md, Q-749.
        assert_eq!(native(P, "select 'ab' like ?||'%'"), "select 'ab' like $1||'%'");
        assert_eq!(native(P, "select '%'||?||'%'"), "select '%'||$1||'%'");
        assert_eq!(native(P, "select data @? '$.a' from t where x = ?"), "select data @? '$.a' from t where x = $1");
        assert_eq!(native(P, "select ?1::int, ?2, ?1"), "select $1::int, $2, $1");
        assert_eq!(native(P, "select /* /* ? */ ? */ ?"), "select /* /* ? */ ? */ $1");
        assert_eq!(native(P, "select a ?|| b, x =?, -?"), "select a ?|| b, x =$1, -$2");
        assert_eq!(native(P, "select data ?| array['a'], data ?& array[?]"), "select data ?| array['a'], data ?& array[$1]");
        assert_eq!(native(P, "select arr[1:2], arr[lo:hi], :id"), "select arr[1:2], arr[lo:hi], $1");
        // A query that numbers its own is as written.
        assert_eq!(native(P, "select $1, data ? 'k'"), "select $1, data ? 'k'");
        // A function's body is its own.
        assert_eq!(native(P, "create function f(int) returns int language sql return $1 + ?"), "create function f(int) returns int language sql return $1 + ?");
        assert_eq!(Scan::new(P, "create function f(int) returns int language sql return $1").count, 0);
    }

    // Found in review of slice 5 (FEEDBACK55.md; branch feedback55).
    #[test]
    fn names_in_brackets_and_sqlites_variables() {
        use Dialect::*;
        // A name in PostgreSQL's brackets is a placeholder unless it is a slice's bound.
        assert_eq!(native(Postgres, "select array[:a, :b], x = any(array[:a]), y[:a]"), "select array[$1, $2], x = any(array[$1]), y[$1]");
        assert_eq!(native(Postgres, "select a[lo:hi], a[1:2], a[i:j], a[f(x):n], a[\"k\":n]"), "select a[lo:hi], a[1:2], a[i:j], a[f(x):n], a[\"k\":n]");
        // SQLite's `:1`, `@1`, `#a` and `$1` are names; not on the others.
        let s = Scan::new(Sqlite, "insert into t values (:1, @2, #a, $3, :1)");
        assert_eq!((s.count, s.named), (4, true));
        assert_eq!(Scan::new(Postgres, "select a[1:2]").count, 0);
        assert_eq!(Scan::new(Mysql, "select 1 # :a\n").count, 0);
        assert_eq!(Scan::new(Sqlite, "select '#a', \"@b\" -- :c").count, 0);
    }

    #[test]
    fn mariadbs_for_loops_and_comments_it_runs() {
        use Dialect::Mysql;
        assert_eq!(stmts(Mysql, "create procedure p() begin for i in 1..3 do insert into t values (i); end for; end; select 1").len(), 2);
        assert_eq!(kinds(Mysql, "select /*M!100100 1, */ 2").len(), 4);
        assert_eq!(kinds(Mysql, "select /*M 1, */ 2").len(), 2);
        assert_eq!(Scan::new(Mysql, "select /*M! :a, */ 2").count, 1);
        // MariaDB's `analyze select` runs a query; `analyze table` commits.
        assert!(Scan::new(Mysql, "analyze select 1").commits_by_itself().is_none());
        assert!(Scan::new(Mysql, "analyze format=json select 1").commits_by_itself().is_none());
        assert!(Scan::new(Mysql, "ANALYZE NO_WRITE_TO_BINLOG TABLE t").commits_by_itself().is_some());
        assert!(Scan::new(Mysql, "optimize local table t").commits_by_itself().is_some());
    }

    #[test]
    fn values_near_where_postgresql_refused() {
        let sql = "select greatest($1, $2) as g, (select count(*) from t where name = $3) as c";
        assert_eq!(values_near(sql, sql.find("= $3").unwrap()), vec![3]);
        assert_eq!(values_near(sql, sql.find("$1").unwrap()), vec![1, 2]);
        let list = "select * from t where name in ($1, $2, $3)";
        assert_eq!(values_near(list, list.find(" in").unwrap() + 1), vec![1, 2, 3]);
        assert_eq!(values_near("", 0), Vec::<usize>::new());
    }

    #[test]
    fn what_is_kept_is_bounded() {
        // A text with a long list written in it is kept as its own size, not its tokens'.
        let list: Vec<String> = (0..12_000).map(|i| (i % 10).to_string()).collect();
        let sql = format!("select count(*) as c from (select 1) where 1 in ({})", list.join(","));
        let s = Scan::new(Dialect::Sqlite, &sql);
        assert!(s.size() < sql.len() * 2, "{} for {}", s.size(), sql.len());
        // Many such texts: what is kept stays under its bound.
        for k in 0..400 {
            scan(Dialect::Sqlite, &format!("{},{})", &sql[..sql.len() - 1], k));
        }
        let g = SEEN.lock().unwrap();
        let (m, bytes) = &g.as_ref().unwrap()[Dialect::Sqlite as usize];
        assert!(*bytes <= KEPT_BYTES && m.values().map(|s| s.size()).sum::<usize>() == *bytes);
    }

    #[test]
    fn values_are_numbered_as_sqlite_numbers_them() {
        for d in Dialect::ALL {
            let s = Scan::new(d, "select ?, ?3, ?, :a, :a, :b");
            assert_eq!((s.count, s.named), (6, true), "{:?}", d);
        }
        let s = Scan::new(Dialect::Mysql, "select :a, ?2, :a");
        assert_eq!((s.native(), s.order.as_deref().map(<[usize]>::to_vec)), ("select ?, ?, ?", Some(vec![1, 2, 1])));
        assert_eq!(Scan::new(Dialect::Mysql, "select ?, ?").native, None);
        assert_eq!(Scan::new(Dialect::Sqlite, "select @a, $b, :c, $1").count, 4);
        // `?0` and a number past the most are not placeholders.
        assert_eq!(Scan::new(Dialect::Sqlite, "select ?0, ?70000").count, 0);
        assert_eq!(Scan::new(Dialect::Postgres, "select $0").count, 0);
    }

    #[test]
    fn statements_and_routine_bodies() {
        use Dialect::*;
        assert_eq!(stmts(Postgres, "create function f(a int, b int) returns int language plpgsql as $$ begin return a + b; end $$; select 1").len(), 2);
        assert_eq!(
            stmts(Postgres, "create or replace function f() returns int language sql begin atomic select case when true then 1 else 2 end; select 2; end; select 3"),
            vec!["create or replace function f() returns int language sql begin atomic select case when true then 1 else 2 end; select 2; end", "select 3"]
        );
        assert_eq!(stmts(Postgres, "do $$ begin perform 1; end $$; select 1;").len(), 2);
        assert_eq!(
            stmts(Mysql, "create trigger t before insert on x for each row begin if new.a then set new.b = 1; end if; case new.c when 1 then set new.d = 2; end case; end; select 1").len(),
            2
        );
        assert_eq!(stmts(Mysql, "create definer = 'u'@'%' procedure p() begin declare x int; l: loop leave l; end loop l; begin select 1; end; end; select 2").len(), 2);
        assert_eq!(stmts(Mysql, "begin not atomic select 1; select 2; end; select 3").len(), 2);
        assert_eq!(stmts(Sqlite, "create temp trigger t after insert on x begin update y set n = case when 1 then 2 end; delete from z; end; select 1").len(), 2);
        // A trigger with no body, and statements in parentheses.
        assert_eq!(stmts(Mysql, "create trigger t before insert on x for each row set new.a = 1; select 1").len(), 2);
        assert_eq!(stmts(Postgres, "select (1); ; ;select 2").len(), 2);
        assert_eq!(stmts(Postgres, "").len(), 0);
        // `create table` with a column named `function` is not a routine.
        assert_eq!(stmts(Postgres, "create table t (function int, x int); select 1").len(), 2);
    }

    #[test]
    fn what_changes_a_session() {
        let lasts = |sql: &str, params: &[Value]| Scan::new(Dialect::Postgres, sql).changes_session(params);
        for sql in [
            "set app.user_id = 'a'",
            "select set_config('a', 'b', false)",
            "select set_config('a', 'b', 'false')",
            "values (set_config('a', 'b', false))",
            "update t set x = x where set_config('a', 'b', false) is not null",
            "select pg_catalog.set_config('a', 'b', false)",
            "select \"set_config\"('a', 'b', false)",
            "select 1; select set_config('a', coalesce(null, 'b'), false)",
            "select pg_advisory_lock_shared(1)",
            "create local temporary table x (a int)",
            "select 1 into temp x",
            "declare c cursor with hold for select 1",
            "do $$ begin perform set_config('a', 'b', false); end $$",
            "do $$ begin set role x; end $$",
            "listen x",
            // Found in review: names PostgreSQL reads as `set_config`, a temporary table by its
            // schema, SQL a block makes from text.
            "select U&\"set\\005fconfig\"('a', 'b', false)",
            "select u&\"set_config\"('a', 'b', false)",
            "create table pg_temp.x (a int)",
            "select * into pg_temp.x from t",
            "do $$ begin execute 'select set_config(''a'', ''b'', false)'; end $$",
            "do $$ begin execute 'set role x'; end $$",
        ] {
            assert!(lasts(sql, &[]), "{}", sql);
        }
        for sql in [
            "set local app.user_id = 'a'",
            "select set_config('a', 'b', true)",
            "select set_config('a', 'b', TRUE), 'set_config(x, y, false)'",
            "select pg_advisory_xact_lock(1)",
            "select my_set_config('a', 'b', false)",
            "do $$ begin perform 1; end $$",
            "select 1 -- set_config('a', 'b', false)",
            "select \"SET_CONFIG\"('a', 'b', false)",
            "create function f() returns int language sql begin atomic select set_config('a', 'b', false); select 1; end",
            "select pg_temp_x(1)",
        ] {
            assert!(!lasts(sql, &[]), "{}", sql);
        }
        // Its third value given: lasting unless `true`.
        let sql = "select set_config('a', ?, ?)";
        assert!(lasts(sql, &[Value::str("42"), Value::Bool(false)]));
        assert!(lasts(sql, &[Value::str("42"), Value::str("true")]));
        assert!(!lasts(sql, &[Value::str("42"), Value::Bool(true)]));
        let mut m = Map::new();
        m.insert("l".into(), Value::Bool(true));
        assert!(!lasts("select set_config('a', 'b', :l)", &[Value::map(m.clone())]));
        m.insert("l".into(), Value::Nil);
        assert!(lasts("select set_config('a', 'b', :l)", &[Value::map(m)]));
    }

    #[test]
    fn what_may_change_a_session_unseen() {
        let s = Scan::new(Dialect::Postgres, "select my_fn(?), count(*), \"Odd\"(1), lower(x) from t where y in (1) and exists (select 1)");
        assert_eq!(s.calls.iter().map(|c| &**c).collect::<Vec<_>>(), vec!["my_fn", "count", "Odd", "lower"]);
        assert!(!s.unseen && !s.defines);
        assert!(Scan::new(Dialect::Postgres, "do $$ begin perform 1; end $$").unseen);
        assert!(Scan::new(Dialect::Postgres, "create or replace function f() returns int language sql return 1").defines);
        assert!(Scan::new(Dialect::Postgres, "drop function if exists f").defines);
        assert!(Scan::new(Dialect::Postgres, "create function f() returns int language sql begin atomic select g(); end").calls.is_empty());
        // Quillo's own transactions on each.
        let after = |d, sql: &str, was| in_tx_after(&Scan::new(d, sql), was);
        assert!(after(Dialect::Mysql, "set transaction isolation level serializable; begin", false));
        assert!(!after(Dialect::Mysql, "begin not atomic select 1; end", false));
        assert!(!after(Dialect::Mysql, "commit", true));
    }

    #[test]
    fn what_mysql_commits_by_itself() {
        let commits = |sql: &str| Scan::new(Dialect::Mysql, sql).commits_by_itself().map(str::to_string);
        assert_eq!(commits("insert into t values (1); truncate table t"), Some("truncate table t".into()));
        for sql in ["create table t (a int)", "drop table t", "alter table t add b int", "create index i on t (a)", "rename table a to b", "lock tables t write", "create user u", "grant all on *.* to u", "analyze table t"] {
            assert!(commits(sql).is_some(), "{}", sql);
        }
        for sql in ["create temporary table t (a int)", "drop temporary table if exists t", "insert into t select 1", "select 'truncate'", "start transaction", "set @a = 1", "load data infile 'x' into table t"] {
            assert_eq!(commits(sql), None, "{}", sql);
        }
    }

    #[test]
    fn arrays_on_postgresql() {
        for sql in ["select * from t where x = any(?)", "select * from t where x <> ALL (?)", "select ?::int[]", "select ?::character varying(10)[]", "select cast(? as text[])", "select unnest(?)"] {
            assert_eq!(values_for_a_list(sql), 0, "{}", sql);
        }
        for (sql, n) in [("insert into t values (?)", 1), ("select ?::int", 1), ("select ?, ?", 2), ("select '?'", 0), ("select x from t where data ? 'k' and y = ?", 1)] {
            assert_eq!(values_for_a_list(sql), n, "{}", sql);
        }
    }

    // FEEDBACK55.md, Q-750: what a read replica may answer, read from the tokens.
    #[test]
    fn what_only_reads() {
        for d in Dialect::ALL {
            for sql in ["select * from t", "  SELECT 1", "with x as (select 1) select * from x", "values (1)", "show tables", "select 'for update', \"into\" from t", "select * from t -- for update\n", "select substring(a from 1 for 2) from t", "select now(), lower(x) from t"] {
                assert!(Scan::new(d, sql).reads_only(), "{:?} {}", d, sql);
            }
            for sql in [
                "insert into t values (1)",
                "select * from t for update",
                "select * from t for no key update",
                "select * from t FOR SHARE",
                "select * from t for key share",
                "select * from t lock in share mode",
                "select * into t2 from t",
                "select nextval('s')",
                "select setval('s', 1)",
                "select currval('s'), lastval()",
                "select pg_advisory_lock(1)",
                "select pg_try_advisory_xact_lock(1)",
                "select lo_create(0)",
                "select get_lock('a', 1)",
                "select next value for s",
                "with d as (delete from t returning *) select * from d",
                "select 1; delete from t",
                "",
                "update t set a = 1",
                "explain analyze delete from t",
            ] {
                assert!(!Scan::new(d, sql).reads_only(), "{:?} {}", d, sql);
            }
        }
        // As PostgreSQL names a function: in any case, its schema before it, quoted.
        for sql in ["select NEXTVAL('s')", "select pg_catalog.nextval('s')", "select \"nextval\"('s')"] {
            assert!(!Scan::new(Dialect::Postgres, sql).reads_only(), "{}", sql);
        }
        // MySQL's `@x` and `into @x`.
        assert!(!Scan::new(Dialect::Mysql, "select 1 into @x").reads_only());
    }

    // FEEDBACK55.md, Q-752, Q-753, Q-739 and the readers moved onto the tokens.
    #[test]
    fn what_the_drivers_read_of_a_statement() {
        let my = |sql: &str| Scan::new(Dialect::Mysql, sql);
        let with = |pairs: &[(&str, i64)]| vec![Value::map(pairs.iter().map(|(k, v)| ((*k).into(), Value::Int(*v))).collect())];
        // MySQL's variables, given a value by a map.
        assert_eq!(my("select @x as v").var_given(&with(&[("x", 1)])), Some("x"));
        assert_eq!(my("select @`x y` + @'z' as v").var_given(&with(&[("z", 1)])), Some("z"));
        assert_eq!(my("select @x as v").var_given(&with(&[("@x", 1)])), Some("x"));
        assert_eq!(my("select @x as v").var_given(&with(&[("y", 1)])), None);
        assert_eq!(my("select @@sql_mode as v").var_given(&with(&[("sql_mode", 1)])), None);
        assert_eq!(my("select '@x' as v").var_given(&with(&[("x", 1)])), None);
        assert_eq!(my("select @x as v").var_given(&[Value::Int(1)]), None);
        assert_eq!(Scan::new(Dialect::Sqlite, "select @x as v").var_given(&with(&[("x", 1)])), None);
        // No statement; a statement that reads; one that changes rows.
        for sql in ["", "  ", "-- x", ";", "/* */ ; ;"] {
            assert!(Scan::new(Dialect::Postgres, sql).is_empty(), "{:?}", sql);
        }
        assert!(!Scan::new(Dialect::Postgres, "select 1").is_empty());
        assert!(Scan::new(Dialect::Postgres, "select 1").reads() && Scan::new(Dialect::Postgres, " Values (1)").reads());
        assert!(!Scan::new(Dialect::Postgres, "insert into t select 1").reads() && !Scan::new(Dialect::Postgres, "select 1; select 2").reads());
        for (sql, rows) in [("insert into t values (1)", true), ("with x as (select 1) update t set a = 1", true), ("replace into t values (1)", true), ("delete from t", true), ("create table t (a int)", false), ("insert into t values (';'); delete from t", false), ("insert into t values (';')", true)] {
            assert_eq!(Scan::new(Dialect::Sqlite, sql).changes_rows(), rows, "{}", sql);
        }
        // MySQL counts these rows twice; `||` it reads as `or`.
        for (sql, twice) in [("replace into t values (1)", true), ("insert into t values (1) on duplicate key update a = 1", true), ("insert into t values (1)", false), ("insert into t select 'on duplicate key update'", false), ("insert into t values (1) on duplicate key update a = 1; select 1", false)] {
            assert_eq!(my(sql).counts_twice(), twice, "{}", sql);
        }
        for (sql, bars) in [("select 'a' || ?", true), ("select a||b", true), ("select '||'", false), ("select a | | b", false), ("select 1 -- ||", false), ("select a or b", false)] {
            assert_eq!(my(sql).has_bars(), bars, "{}", sql);
        }
        assert!(!Scan::new(Dialect::Postgres, "select 'a' || ?").has_bars());
    }

    // Found in review: which values are a column's (Q-733), which column a MySQL value meets
    // (Q-741), and what a schema's statement changes (Q-745).
    #[test]
    fn what_the_drivers_read_of_each_value_and_each_schema_change() {
        let pg = |sql: &str| Scan::new(Dialect::Postgres, sql);
        let given = |sql: &str| (1..=pg(sql).count).map(|i| pg(sql).assigned(i)).collect::<Vec<_>>();
        assert_eq!(given("insert into t (a, b) values (?, ?::jsonb), (?, f(?))"), [true, true, true, false]);
        assert_eq!(given("update t set a = ?, b = ?::jsonb[], c = jsonb_set(c, '{a}', ?) where d = ?"), [true, true, false, false]);
        assert_eq!(given("insert into t values (?) on conflict (k) do update set j = ?"), [true, true]);
        assert_eq!(given("select jsonb_set('{}', '{a}', ?), ?::json"), [false, false]);
        assert_eq!(given("insert into t select ?"), [false]);
        assert_eq!(given("update t set a = ? || 'x'"), [false]);
        assert_eq!(given("insert into t (j) values ($1)"), [true]);
        let my = |sql: &str| {
            let s = Scan::new(Dialect::Mysql, sql);
            (1..=s.count).map(|i| s.column(i).map(str::to_string)).collect::<Vec<_>>()
        };
        let c = |xs: &[Option<&str>]| xs.iter().map(|x| x.map(str::to_string)).collect::<Vec<_>>();
        assert_eq!(my("insert into t (a, `B`) values (?, ?), (?, ?)"), c(&[Some("a"), Some("b"), Some("a"), Some("b")]));
        assert_eq!(my("update t set b = ? where t.k <> ? and ? = c and d in (?, ?) and e <=> ?"), c(&[Some("b"), Some("k"), Some("c"), Some("d"), Some("d"), Some("e")]));
        assert_eq!(my("select f(?) from t where a = ? + 1 and b like ?"), c(&[None, None, None]));
        assert_eq!(my("insert into t values (?)"), c(&[None]));
        assert_eq!(my("select ? where x = :n"), c(&[None, Some("x")]));
        let ddl = |sql: &str| pg(sql).ddl().map(|n| n.map(|n| n.iter().map(|x| x.to_string()).collect::<Vec<_>>()));
        assert_eq!(ddl("select 1"), None);
        assert_eq!(ddl("create table if not exists t (a int)"), None);
        assert_eq!(ddl("drop table if exists a, public.\"B\""), Some(Some(vec!["a".to_string(), "b".to_string()])));
        assert_eq!(ddl("alter table only t add column c int"), Some(Some(vec!["t".to_string()])));
        assert_eq!(ddl("alter table t rename to u"), Some(Some(vec!["t".to_string(), "u".to_string()])));
        assert_eq!(ddl("create or replace view v as select * from t"), Some(Some(vec!["v".to_string()])));
        assert_eq!(ddl("drop function if exists f(int, text), g()"), Some(Some(vec!["f".to_string(), "g".to_string()])));
        assert_eq!(ddl("drop materialized view m"), Some(Some(vec!["m".to_string()])));
        for all in ["drop table t cascade", "alter schema s rename to r", "drop owned by x", "alter role r set search_path = a"] {
            assert_eq!(ddl(all), Some(None), "{}", all);
        }
    }

    // SQLite's columns kept as floats (FEEDBACK50.md, Q-628), read from the tokens.
    #[test]
    fn sqlites_lossy_columns_from_the_tokens() {
        let lossy = |sql: &str| Scan::new(Dialect::Sqlite, sql).lossy.iter().map(|(t, c, ty)| format!("{}.{} {}", t, c, ty)).collect::<Vec<_>>();
        assert_eq!(lossy("create table t (id integer primary key, price money, p2 DECIMAL(12, 2), p3 decimal_text, n numeric, d dec(5,1), \"q\" numeric ( 10, 2), dx decimal, c money_cents, check (price > 0))"), ["t.price money", "t.p2 decimal", "t.d dec", "t.q numeric", "t.dx decimal", "t.c money_cents"]);
        assert_eq!(lossy("create temp table if not exists main.u (\"unit price\" money) -- , note decimal"), ["main.u.unit price money"]);
        assert_eq!(lossy("alter table t add column fee decimal; alter table t add tax money; alter table t rename to v"), ["t.fee decimal", "t.tax money"]);
        assert!(lossy("create table v (a text default ', x money', /* , y money */ b int)").is_empty());
        assert!(lossy("create index i on t (price)").is_empty() && lossy("").is_empty() && lossy("create table").is_empty());
    }
}
