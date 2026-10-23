//! Which tables a SQL statement reads and writes, for live updates.
//!
//! This is a light scan, not a SQL parser: it skips strings, quoted
//! identifiers and comments, and looks at the words after `from`, `join`,
//! `into`, `update`, `delete from`, `truncate` and `create/alter/drop table`.
//! Getting a name too many only wakes a page for nothing; the rest of the
//! system does not depend on it being exact.

#[derive(Debug, Default, PartialEq)]
pub struct Tables {
    pub reads: Vec<String>,
    pub writes: Vec<String>,
}

#[derive(Debug, PartialEq)]
enum Tok {
    Word(String),
    Name(String),
    Punct(char),
}

fn tokens(sql: &str) -> Vec<Tok> {
    let c: Vec<char> = sql.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() {
            i += 1;
        } else if ch == '\'' {
            i += 1;
            while i < c.len() {
                if c[i] == '\'' {
                    if c.get(i + 1) == Some(&'\'') {
                        i += 2;
                        continue;
                    }
                    break;
                }
                if c[i] == '\\' {
                    i += 1;
                }
                i += 1;
            }
            i += 1;
        } else if ch == '"' || ch == '`' || ch == '[' {
            let close = if ch == '[' { ']' } else { ch };
            let mut s = String::new();
            i += 1;
            while i < c.len() && c[i] != close {
                s.push(c[i]);
                i += 1;
            }
            i += 1;
            out.push(Tok::Name(s));
        } else if ch == '-' && c.get(i + 1) == Some(&'-') {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
        } else if ch == '/' && c.get(i + 1) == Some(&'*') {
            while i < c.len() && !(c[i] == '*' && c.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            i += 2;
        } else if ch == '$' && !c.get(i + 1).map_or(false, |d| d.is_ascii_digit()) {
            // $tag$ ... $tag$
            let mut j = i + 1;
            while j < c.len() && (c[j].is_alphanumeric() || c[j] == '_') {
                j += 1;
            }
            if c.get(j) == Some(&'$') {
                let tag: String = c[i..=j].iter().collect();
                let rest: String = c[j + 1..].iter().collect();
                i = match rest.find(&tag) {
                    Some(e) => j + 1 + rest[..e].chars().count() + tag.chars().count(),
                    None => c.len(),
                };
            } else {
                i += 1;
            }
        } else if ch.is_alphanumeric() || ch == '_' {
            let mut s = String::new();
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_' || c[i] == '$') {
                s.push(c[i]);
                i += 1;
            }
            out.push(Tok::Word(s.to_ascii_lowercase()));
        } else {
            out.push(Tok::Punct(ch));
            i += 1;
        }
    }
    out
}

fn is_word(t: Option<&Tok>, w: &str) -> bool {
    matches!(t, Some(Tok::Word(x)) if x == w)
}

/// The table named at `i` (`name`, `schema.name`, `"Name"`), lowercased
/// without its schema, and the position after it.
fn table_at(t: &[Tok], mut i: usize) -> Option<(String, usize)> {
    for skip in ["only", "lateral", "if", "not", "exists"] {
        while is_word(t.get(i), skip) {
            i += 1;
        }
    }
    let mut name = match t.get(i)? {
        Tok::Word(w) => w.clone(),
        Tok::Name(n) => n.to_lowercase(),
        Tok::Punct(_) => return None,
    };
    i += 1;
    while matches!(t.get(i), Some(Tok::Punct('.'))) {
        match t.get(i + 1) {
            Some(Tok::Word(w)) => name = w.clone(),
            Some(Tok::Name(n)) => name = n.to_lowercase(),
            _ => break,
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
fn skip_alias(t: &[Tok], mut i: usize) -> usize {
    const KEYWORDS: &[&str] = &[
        "where", "join", "on", "left", "right", "inner", "outer", "full", "cross", "natural", "group", "order",
        "limit", "offset", "having", "union", "except", "intersect", "returning", "set", "using", "window", "for",
    ];
    if is_word(t.get(i), "as") {
        i += 1;
    }
    match t.get(i) {
        Some(Tok::Word(w)) if !KEYWORDS.contains(&w.as_str()) => i + 1,
        Some(Tok::Name(_)) => i + 1,
        _ => i,
    }
}

fn push(v: &mut Vec<String>, name: String) {
    if !v.contains(&name) {
        v.push(name);
    }
}

/// `tables`, worked out once per statement (statements are usually
/// written in the program, so few).
pub fn tables_of(sql: &str) -> std::sync::Arc<Tables> {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    static SEEN: Mutex<Option<HashMap<String, Arc<Tables>>>> = Mutex::new(None);
    if let Some(t) = SEEN.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(sql)) {
        return t.clone();
    }
    let t = Arc::new(tables(sql));
    let mut g = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    let m = g.get_or_insert_with(HashMap::new);
    if m.len() >= 1024 {
        m.clear();
    }
    m.insert(sql.to_string(), t.clone());
    t
}

pub fn tables(sql: &str) -> Tables {
    let t = tokens(sql);
    let mut out = Tables::default();
    let mut i = 0;
    while i < t.len() {
        let Tok::Word(w) = &t[i] else {
            i += 1;
            continue;
        };
        match w.as_str() {
            "from" | "join" => {
                let after_delete = i > 0 && is_word(t.get(i - 1), "delete");
                if let Some((name, next)) = table_at(&t, i + 1) {
                    if after_delete {
                        push(&mut out.writes, name);
                    } else {
                        push(&mut out.reads, name.clone());
                        // `from a x, b y`
                        let mut j = skip_alias(&t, next);
                        while matches!(t.get(j), Some(Tok::Punct(','))) {
                            match table_at(&t, j + 1) {
                                Some((n, k)) => {
                                    push(&mut out.reads, n);
                                    j = skip_alias(&t, k);
                                }
                                None => break,
                            }
                        }
                    }
                }
            }
            "into" => {
                if let Some((name, _)) = table_at(&t, i + 1) {
                    push(&mut out.writes, name);
                }
            }
            "update" => {
                // not `on conflict ... do update set`
                if !is_word(t.get(i + 1), "set") {
                    if let Some((name, _)) = table_at(&t, i + 1) {
                        push(&mut out.writes, name);
                    }
                }
            }
            "truncate" => {
                let start = if is_word(t.get(i + 1), "table") { i + 2 } else { i + 1 };
                if let Some((name, _)) = table_at(&t, start) {
                    push(&mut out.writes, name);
                }
            }
            "table" if i > 0 && matches!(&t[i - 1], Tok::Word(p) if p == "create" || p == "alter" || p == "drop") => {
                if let Some((name, _)) = table_at(&t, i + 1) {
                    push(&mut out.writes, name);
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rw(sql: &str) -> (Vec<&'static str>, Vec<&'static str>) {
        let t = tables(sql);
        let leak = |v: Vec<String>| v.into_iter().map(|s| &*Box::leak(s.into_boxed_str())).collect();
        (leak(t.reads), leak(t.writes))
    }

    #[test]
    fn reads_and_writes() {
        assert_eq!(rw("select * from notes where id = ?"), (vec!["notes"], vec![]));
        assert_eq!(rw("SELECT n.id FROM public.Notes n JOIN \"Users\" u ON u.id = n.user_id"), (vec!["notes", "users"], vec![]));
        assert_eq!(rw("select * from notes as n, tags t, users where n.x = 1"), (vec!["notes", "tags", "users"], vec![]));
        assert_eq!(rw("insert into notes (text) values (?) returning id"), (vec![], vec!["notes"]));
        assert_eq!(rw("insert into log select * from notes"), (vec!["notes"], vec!["log"]));
        assert_eq!(rw("update notes set done = 1 - done where id = ?"), (vec![], vec!["notes"]));
        assert_eq!(rw("delete from notes where id in (select id from old)"), (vec!["old"], vec!["notes"]));
        assert_eq!(rw("insert into t (k) values (1) on conflict (k) do update set k = 2"), (vec![], vec!["t"]));
        assert_eq!(rw("create table if not exists notes (id int); drop table old"), (vec![], vec!["notes", "old"]));
        assert_eq!(rw("truncate table a"), (vec![], vec!["a"]));
        assert_eq!(rw("select 'from fake', $$ from other $$ -- from x\n from real /* join y */"), (vec!["real"], vec![]));
        assert_eq!(rw("select count(*) from (select 1) as s"), (vec![], vec![]));
    }
}
