//! Which tables a SQL statement reads and writes, for live updates and for read replicas.
//!
//! This is a light look, not a SQL parser: it reads the tokens of the SQL tokenizer every
//! other part of Quillo reads SQL with (src/sql.rs), as the statement's database reads them,
//! so strings, quoted names, dollar quotes and comments are what that database says they are
//! (FEEDBACK55.md, Q-750; branch feedback55), and looks at the words after `from`, `join`,
//! `into`, `update`, `delete from`, `truncate` and `create/alter/drop table`. Getting a name
//! too many only wakes a page for nothing; the rest of the system does not depend on it being
//! exact.

use crate::sql::{Kind, Tok};

#[derive(Debug, Default, PartialEq)]
pub struct Tables {
    pub reads: Vec<String>,
    pub writes: Vec<String>,
}

/// The tables `toks` (the tokens of `sql`) read and write.
pub(crate) fn tables_in(sql: &str, toks: &[Tok]) -> Tables {
    let word = |i: usize| toks.get(i).filter(|t| t.kind == Kind::Word).map(|t| &sql[t.start..t.end]);
    let is = |i: usize, w: &str| word(i).map_or(false, |x| x.eq_ignore_ascii_case(w));
    let mut out = Tables::default();
    for i in 0..toks.len() {
        let Some(w) = word(i) else { continue };
        match w.to_ascii_lowercase().as_str() {
            "from" | "join" => {
                let after_delete = i > 0 && is(i - 1, "delete");
                if let Some((name, next)) = table_at(sql, toks, i + 1) {
                    if after_delete {
                        push(&mut out.writes, name);
                    } else {
                        push(&mut out.reads, name);
                        // `from a x, b y`
                        let mut j = skip_alias(sql, toks, next);
                        while toks.get(j).map(|t| t.kind) == Some(Kind::Comma) {
                            match table_at(sql, toks, j + 1) {
                                Some((n, k)) => {
                                    push(&mut out.reads, n);
                                    j = skip_alias(sql, toks, k);
                                }
                                None => break,
                            }
                        }
                    }
                }
            }
            "into" => {
                if let Some((name, _)) = table_at(sql, toks, i + 1) {
                    push(&mut out.writes, name);
                }
            }
            // Not `on conflict ... do update set`.
            "update" if !is(i + 1, "set") => {
                if let Some((name, _)) = table_at(sql, toks, i + 1) {
                    push(&mut out.writes, name);
                }
            }
            "truncate" => {
                let start = if is(i + 1, "table") { i + 2 } else { i + 1 };
                if let Some((name, _)) = table_at(sql, toks, start) {
                    push(&mut out.writes, name);
                }
            }
            "table" if i > 0 && (is(i - 1, "create") || is(i - 1, "alter") || is(i - 1, "drop")) => {
                if let Some((name, _)) = table_at(sql, toks, i + 1) {
                    push(&mut out.writes, name);
                }
            }
            _ => {}
        }
    }
    out
}

/// A name as written, lowercased: a word, or a quoted name without its quotes (`"x"`,
/// `` `x` ``, `[x]`, `U&"x"`).
fn name_at(sql: &str, toks: &[Tok], i: usize) -> Option<String> {
    let t = toks.get(i)?;
    let text = &sql[t.start..t.end];
    match t.kind {
        Kind::Word => Some(text.to_ascii_lowercase()),
        Kind::Quoted => {
            let inner = text.get(..3).filter(|p| p.eq_ignore_ascii_case("u&\"")).map_or_else(|| text.get(1..).unwrap_or(""), |_| &text[3..]);
            let inner = inner.strip_suffix(['"', '`', ']']).unwrap_or(inner);
            Some(inner.replace("\"\"", "\"").replace("``", "`").to_lowercase())
        }
        _ => None,
    }
}

/// The table named at `i` (`name`, `schema.name`, `"Name"`), lowercased without its schema,
/// and the position after it.
fn table_at(sql: &str, toks: &[Tok], mut i: usize) -> Option<(String, usize)> {
    let is = |i: usize, w: &str| toks.get(i).map_or(false, |t| t.kind == Kind::Word && sql[t.start..t.end].eq_ignore_ascii_case(w));
    for skip in ["only", "lateral", "if", "not", "exists"] {
        while is(i, skip) {
            i += 1;
        }
    }
    let mut name = name_at(sql, toks, i)?;
    i += 1;
    while toks.get(i).map(|t| t.kind) == Some(Kind::Dot) {
        match name_at(sql, toks, i + 1) {
            Some(n) => name = n,
            None => break,
        }
        i += 2;
    }
    const NOT_TABLES: &[&str] = &["select", "values", "set", "where", "default", "unnest", "generate_series", "json_each"];
    if NOT_TABLES.contains(&name.as_str()) {
        return None;
    }
    Some((name, i))
}

/// Skips `as x` or `x` after a table name.
fn skip_alias(sql: &str, toks: &[Tok], mut i: usize) -> usize {
    const KEYWORDS: &[&str] = &[
        "where", "join", "on", "left", "right", "inner", "outer", "full", "cross", "natural", "group", "order", "limit", "offset", "having", "union",
        "except", "intersect", "returning", "set", "using", "window", "for",
    ];
    let word = |i: usize| toks.get(i).filter(|t| t.kind == Kind::Word).map(|t| &sql[t.start..t.end]);
    if word(i).map_or(false, |w| w.eq_ignore_ascii_case("as")) {
        i += 1;
    }
    match toks.get(i).map(|t| t.kind) {
        Some(Kind::Word) if !KEYWORDS.iter().any(|k| word(i).map_or(false, |w| w.eq_ignore_ascii_case(k))) => i + 1,
        Some(Kind::Quoted) => i + 1,
        _ => i,
    }
}

fn push(v: &mut Vec<String>, name: String) {
    if !v.contains(&name) {
        v.push(name);
    }
}

#[cfg(test)]
mod tests {
    use crate::sql::{scan, Dialect};

    fn rw_in(d: Dialect, sql: &str) -> (Vec<String>, Vec<String>) {
        let s = scan(d, sql);
        (s.tables.reads.clone(), s.tables.writes.clone())
    }

    fn rw(sql: &str) -> (Vec<String>, Vec<String>) {
        rw_in(Dialect::Postgres, sql)
    }

    fn v(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn reads_and_writes() {
        assert_eq!(rw("select * from notes where id = ?"), (v(&["notes"]), v(&[])));
        assert_eq!(rw("SELECT n.id FROM public.Notes n JOIN \"Users\" u ON u.id = n.user_id"), (v(&["notes", "users"]), v(&[])));
        assert_eq!(rw("select * from notes as n, tags t, users where n.x = 1"), (v(&["notes", "tags", "users"]), v(&[])));
        assert_eq!(rw("insert into notes (text) values (?) returning id"), (v(&[]), v(&["notes"])));
        assert_eq!(rw("insert into log select * from notes"), (v(&["notes"]), v(&["log"])));
        assert_eq!(rw("update notes set done = 1 - done where id = ?"), (v(&[]), v(&["notes"])));
        assert_eq!(rw("delete from notes where id in (select id from old)"), (v(&["old"]), v(&["notes"])));
        assert_eq!(rw("insert into t (k) values (1) on conflict (k) do update set k = 2"), (v(&[]), v(&["t"])));
        assert_eq!(rw("create table if not exists notes (id int); drop table old"), (v(&[]), v(&["notes", "old"])));
        assert_eq!(rw("truncate table a"), (v(&[]), v(&["a"])));
        assert_eq!(rw("select 'from fake', $$ from other $$ -- from x\n from real /* join y */"), (v(&["real"]), v(&[])));
        assert_eq!(rw("select count(*) from (select 1) as s"), (v(&[]), v(&[])));
    }

    // Each database's own quotes and comments (FEEDBACK55.md, Q-750; branch feedback55).
    #[test]
    fn tables_as_each_database_reads_the_sql() {
        // PostgreSQL nests comments; a `from` in the inner one's tail is still a comment.
        assert_eq!(rw("select 1 /* a /* b */ from fake */ from real"), (v(&["real"]), v(&[])));
        assert_eq!(rw_in(Dialect::Sqlite, "select 1 /* a /* b */ from fake */ from real").0, v(&["fake", "real"]));
        // MySQL's `#` comment, backticks, and `"..."` as a string.
        assert_eq!(rw_in(Dialect::Mysql, "select `x` from `Orders` o # from fake\n join t2 on 1"), (v(&["orders", "t2"]), v(&[])));
        assert_eq!(rw_in(Dialect::Mysql, "select \"from fake\" from t"), (v(&["t"]), v(&[])));
        // SQLite's brackets, and a schema's name before the table's.
        assert_eq!(rw_in(Dialect::Sqlite, "insert into main.[Line Items] select * from \"Ünï\""), (v(&["ünï"]), v(&["line items"])));
        // Nothing, one, and a name with `""` in it.
        assert_eq!(rw(""), (v(&[]), v(&[])));
        assert_eq!(rw("delete from \"a\"\"b\""), (v(&[]), v(&["a\"b"])));
    }
}
