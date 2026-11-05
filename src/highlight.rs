//! `highlight(code, lang)`: source code as HTML, its parts in spans a style
//! sheet can colour: `hl-k` keywords, `hl-s` strings, `hl-c` comments,
//! `hl-n` numbers and constants, `hl-f` functions called, `hl-t` types and
//! tags, `hl-a` attributes, properties and keys. Everything is escaped, so
//! the result is safe to put in a page. The browser's runtime has the same
//! function (`B.highlight` in runtime.js), token for token, for editors that
//! colour as you type.

struct Lang {
    line: &'static [&'static str],
    block: Option<(&'static str, &'static str)>,
    quotes: &'static str,
    keywords: &'static [&'static str],
    consts: &'static [&'static str],
    types: bool,
    tags: bool,
    css: bool,
    json: bool,
    nocase: bool,
}

const QWE: Lang = Lang {
    line: &["//"],
    block: Some(("/*", "*/")),
    quotes: "\"`",
    keywords: &["let", "fn", "view", "component", "server", "route", "routes", "use", "import", "if", "else", "match", "for", "in", "return", "reply", "as", "while", "break", "continue", "type", "and", "or", "not", "with"],
    consts: &["true", "false", "null", "int", "str", "bool", "float", "dec", "map", "list", "any"],
    types: true,
    tags: true,
    css: false,
    json: false,
    nocase: false,
};

const JS: Lang = Lang {
    line: &["//"],
    block: Some(("/*", "*/")),
    quotes: "\"'`",
    keywords: &[
        "break", "case", "catch", "class", "const", "continue", "debugger", "default", "delete", "do", "else", "export", "extends", "finally", "for",
        "function", "if", "import", "in", "instanceof", "let", "new", "return", "super", "switch", "this", "throw", "try", "typeof", "var", "void",
        "while", "with", "yield", "async", "await", "of", "static", "from", "interface", "type", "enum", "implements", "as",
    ],
    consts: &["true", "false", "null", "undefined", "NaN", "Infinity"],
    types: true,
    tags: false,
    css: false,
    json: false,
    nocase: false,
};

const PY: Lang = Lang {
    line: &["#"],
    block: None,
    quotes: "\"'",
    keywords: &[
        "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif", "else", "except", "finally", "for", "from",
        "global", "if", "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while", "with", "yield",
    ],
    consts: &["True", "False", "None", "self"],
    types: true,
    tags: false,
    css: false,
    json: false,
    nocase: false,
};

const RUST: Lang = Lang {
    line: &["//"],
    block: Some(("/*", "*/")),
    quotes: "\"",
    keywords: &[
        "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "fn", "for", "if", "impl", "in", "let",
        "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "static", "struct", "super", "trait", "type", "unsafe", "use", "where", "while",
    ],
    consts: &["true", "false", "self", "Self", "None"],
    types: true,
    tags: false,
    css: false,
    json: false,
    nocase: false,
};

const SH: Lang = Lang {
    line: &["#"],
    block: None,
    quotes: "\"'",
    keywords: &["if", "then", "else", "elif", "fi", "for", "while", "do", "done", "case", "esac", "function", "in", "return", "export", "local", "exit", "cd", "echo", "sudo"],
    consts: &["true", "false"],
    types: false,
    tags: false,
    css: false,
    json: false,
    nocase: false,
};

const SQL: Lang = Lang {
    line: &["--"],
    block: Some(("/*", "*/")),
    quotes: "'\"",
    keywords: &[
        "select", "from", "where", "insert", "into", "values", "update", "set", "delete", "create", "table", "alter", "add", "column", "index",
        "primary", "key", "references", "not", "and", "or", "join", "left", "right", "inner", "outer", "on", "group", "by", "order", "having",
        "limit", "offset", "as", "distinct", "unique", "default", "integer", "text", "real", "begin", "commit", "drop", "if", "exists", "case",
        "when", "then", "else", "end", "like", "in", "is", "desc", "asc", "union", "all", "with",
    ],
    consts: &["null", "true", "false"],
    types: false,
    tags: false,
    css: false,
    json: false,
    nocase: true,
};

const CSS: Lang = Lang {
    line: &[],
    block: Some(("/*", "*/")),
    quotes: "\"'",
    keywords: &["!important"],
    consts: &[],
    types: false,
    tags: false,
    css: true,
    json: false,
    nocase: false,
};

const HTML: Lang = Lang {
    line: &[],
    block: None,
    quotes: "",
    keywords: &[],
    consts: &[],
    types: false,
    tags: true,
    css: false,
    json: false,
    nocase: false,
};

const JSON: Lang = Lang {
    line: &[],
    block: None,
    quotes: "\"",
    keywords: &[],
    consts: &["true", "false", "null"],
    types: false,
    tags: false,
    css: false,
    json: true,
    nocase: false,
};

const PLAIN: Lang = Lang {
    line: &[],
    block: None,
    quotes: "",
    keywords: &[],
    consts: &[],
    types: false,
    tags: false,
    css: false,
    json: false,
    nocase: false,
};

fn lang_of(name: &str) -> &'static Lang {
    match name.to_ascii_lowercase().as_str() {
        "qwe" | "quillo" => &QWE,
        "js" | "javascript" | "ts" | "typescript" | "jsx" | "tsx" | "mjs" => &JS,
        "py" | "python" => &PY,
        "rs" | "rust" => &RUST,
        "sh" | "bash" | "shell" | "zsh" | "console" => &SH,
        "sql" => &SQL,
        "css" => &CSS,
        "html" | "xml" | "svg" => &HTML,
        "json" => &JSON,
        _ => &PLAIN,
    }
}

fn escape_into(out: &mut String, s: &[char]) {
    for &c in s {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
}

fn starts(s: &[char], i: usize, p: &str) -> bool {
    let mut j = i;
    for c in p.chars() {
        if j >= s.len() || s[j] != c {
            return false;
        }
        j += 1;
    }
    true
}

/// Where the route's path is on the line that starts at `b` (`route ANY /*rest`, `routes /admin`,
/// `mount /blog`), which the lexer reads as a path: `/*` there starts no comment (branch
/// feedback40-43). Found once a line: a line of many `/*` was read again from its start for
/// each, and highlighting was quadratic (FEEDBACK55.md, Q-706; branch feedback55).
fn route_path(s: &[char], b: usize) -> Option<(usize, usize)> {
    let space = |c: char| matches!(c, ' ' | '\t' | '\r');
    let skip = |mut j: usize| {
        while j < s.len() && space(s[j]) {
            j += 1;
        }
        j
    };
    let word = |mut j: usize| {
        let from = j;
        while j < s.len() && s[j] != '\n' && !space(s[j]) {
            j += 1;
        }
        (from, j)
    };
    let (f, t) = word(skip(b));
    let mut at = skip(t);
    match s[f..t].iter().collect::<String>().as_str() {
        "route" => {
            let (f, t) = word(at);
            if f == t || !s[f..t].iter().all(|c| c.is_ascii_uppercase()) {
                return None;
            }
            at = skip(t);
        }
        "routes" | "mount" => {}
        _ => return None,
    }
    let (f, t) = word(at);
    (f < t && s[f] == '/').then_some((f, t))
}

/// Whether `i` is in a route's path. `seen` is the last line looked at: where it ends, and where
/// its path is, so that each line is read once (Q-706).
#[inline(never)]
fn in_route_path(s: &[char], i: usize, seen: &mut (usize, usize, usize)) -> bool {
    if i >= seen.0 {
        let (mut b, mut e) = (i, i);
        while b > seen.0 && s[b - 1] != '\n' {
            b -= 1;
        }
        while e < s.len() && s[e] != '\n' {
            e += 1;
        }
        let (f, t) = route_path(s, b).unwrap_or((0, 0));
        *seen = (e, f, t);
    }
    seen.1 <= i && i < seen.2
}

/// Where a Python string that starts with `"""` or `'''` at `i` ends, over lines: None when `i`
/// starts none (FEEDBACK55.md, Q-834; branch feedback55). This and the scans below are out of
/// `highlight`'s loop, which stays as fast as it was.
#[inline(never)]
fn triple_end(s: &[char], i: usize) -> Option<usize> {
    let triple = if s[i] == '"' { "\"\"\"" } else { "'''" };
    if !starts(s, i, triple) {
        return None;
    }
    let mut j = i + 3;
    while j < s.len() && !starts(s, j, triple) {
        if s[j] == '\\' {
            j += 1;
        }
        j += 1;
    }
    Some((j + 3).min(s.len()))
}

/// Where a Rust character that starts at `i` ends: on the character (`'"'`), or on an escape
/// (`'\n'`, `'\u{1F600}'`); None for a lifetime, `'a` (Q-834).
#[inline(never)]
fn char_end(s: &[char], i: usize) -> Option<usize> {
    if s.get(i + 1) == Some(&'\\') && s.get(i + 2).is_some_and(|&e| e != '\n') {
        let mut j = i + 3;
        while j < s.len() && j < i + 12 && s[j] != '\n' {
            if s[j] == '\'' {
                return Some(j + 1);
            }
            j += 1;
        }
        return None;
    }
    (i + 2 < s.len() && !matches!(s[i + 1], '\n' | '\'') && s[i + 2] == '\'').then_some(i + 3)
}

/// Where a Rust raw string that starts at `i` ends: `r"..."`, `r#"..."#` (to a `"` and as many
/// `#`), `br"..."`, with no escapes, over lines; None where none starts (a word, `r#type`). Read
/// as a `"` string, `r"C:\temp\"` went on to the end of the text (FEEDBACK55.md, Q-834; branch
/// feedback55).
#[inline(never)]
fn raw_end(s: &[char], i: usize) -> Option<usize> {
    let mut j = i + usize::from(s[i] == 'b');
    if s.get(j) != Some(&'r') {
        return None;
    }
    j += 1;
    let from = j;
    while j < s.len() && s[j] == '#' {
        j += 1;
    }
    let hashes = j - from;
    if s.get(j) != Some(&'"') {
        return None;
    }
    j += 1;
    while j < s.len() {
        if s[j] == '"' && s.len() - (j + 1) >= hashes && s[j + 1..j + 1 + hashes].iter().all(|&c| c == '#') {
            return Some(j + 1 + hashes);
        }
        j += 1;
    }
    Some(s.len())
}

/// Where a JavaScript regular expression that starts at `i` ends: at its `/` on the line (one in
/// `[...]` or escaped is in it), with its flags; else Err, with where its line ends (Q-834).
#[inline(never)]
fn regex_end(s: &[char], i: usize) -> Result<usize, usize> {
    let (mut j, mut class) = (i + 1, false);
    while j < s.len() && s[j] != '\n' && (class || s[j] != '/') {
        match s[j] {
            '\\' if j + 1 < s.len() && s[j + 1] != '\n' => j += 1,
            '[' => class = true,
            ']' => class = false,
            _ => {}
        }
        j += 1;
    }
    if j == s.len() || s[j] != '/' {
        return Err(j);
    }
    j += 1;
    while j < s.len() && s[j].is_ascii_alphabetic() {
        j += 1;
    }
    Ok(j)
}

/// Whether a `/` at `i` starts a JavaScript regular expression: at the start, or after an
/// operator, a bracket that opens or a keyword such as `return`, where no value has ended
/// (FEEDBACK55.md, Q-834; branch feedback55).
#[inline(never)]
fn regex_may_start(s: &[char], i: usize) -> bool {
    let mut k = i;
    while k > 0 && matches!(s[k - 1], ' ' | '\t' | '\r' | '\n') {
        k -= 1;
    }
    if k == 0 || "(,=:[!&|?{};+-*%>~^".contains(s[k - 1]) {
        return true;
    }
    let mut b = k;
    while b > 0 && k - b < 11 && ident_char(s[b - 1], false) {
        b -= 1;
    }
    let word: String = s[b..k].iter().collect();
    (b == 0 || !ident_char(s[b - 1], false))
        && ["return", "typeof", "instanceof", "case", "do", "else", "in", "of", "new", "delete", "void", "throw", "yield", "await"].contains(&word.as_str())
}

fn ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == '$'
}

fn ident_char(c: char, css: bool) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$' || (css && c == '-')
}

pub fn highlight(code: &str, lang: &str) -> String {
    colour(code, lang, false).unwrap_or_default()
}

/// `highlight` for a request: None once the request has computed past `--max-compute`, which
/// it asks every few thousand tokens, so that no text holds its instance past the limit
/// (FEEDBACK55.md, Q-706; branch feedback55).
pub fn highlight_within(code: &str, lang: &str) -> Option<String> {
    colour(code, lang, true)
}

/// Whether a request, `timed`, has computed past `--max-compute`: it asks once (Q-706).
fn over(timed: bool) -> bool {
    timed && crate::httpd::OVER_TIME.load(std::sync::atomic::Ordering::Relaxed) && crate::httpd::over_time()
}

fn colour(code: &str, lang: &str, timed: bool) -> Option<String> {
    let l = lang_of(lang);
    let s: Vec<char> = code.chars().collect();
    let mut out = String::with_capacity(code.len() * 2);
    // A language it does not know is text, only escaped, as the guide says: its numbers and
    // calls were coloured (FEEDBACK55.md, Q-834; branch feedback55).
    if std::ptr::eq(l, &PLAIN) {
        escape_into(&mut out, &s);
        return (!over(timed)).then_some(out);
    }
    let (qwe, js, rust, py, sh) = (std::ptr::eq(l, &QWE), std::ptr::eq(l, &JS), std::ptr::eq(l, &RUST), std::ptr::eq(l, &PY), std::ptr::eq(l, &SH));
    // The last line a route's path was looked for on: where it ends, and the path (Q-706).
    let mut seen = (0usize, 0usize, 0usize);
    // JavaScript: where a regular expression may start again, after one that did not close on
    // its line, so that each line is read for one once (FEEDBACK55.md, Q-834; branch feedback55).
    let mut no_regex = 0usize;
    let mut steps = 0u32;
    let mut i = 0;
    let mut plain = 0; // where the text not yet written starts
    let mut depth = 0i32; // CSS: inside braces
    let mut in_tag = false; // HTML: between a tag's name and its `>`
    let span = |out: &mut String, plain: &mut usize, from: usize, to: usize, class: &str| {
        escape_into(out, &s[*plain..from]);
        out.push_str("<span class=\"");
        out.push_str(class);
        out.push_str("\">");
        escape_into(out, &s[from..to]);
        out.push_str("</span>");
        *plain = to;
    };
    while i < s.len() {
        // `--max-compute`, every 4096 steps and at the first too, as Markdown's many small blocks
        // are a call each (FEEDBACK55.md, Q-706; branch feedback55).
        if steps % 4096 == 0 && over(timed) {
            return None;
        }
        steps = steps.wrapping_add(1);
        let c = s[i];
        // Comments. The shell's `#` starts one at a word's start only: `${#a[@]}` and `$#` are
        // code (FEEDBACK55.md, Q-834; branch feedback55).
        if l.line.iter().any(|p| starts(&s, i, p)) && !(sh && i > 0 && !matches!(s[i - 1], ' ' | '\t' | '\r' | '\n' | ';' | '|' | '&' | '(' | ')' | '<' | '>')) {
            let mut j = i;
            while j < s.len() && s[j] != '\n' {
                j += 1;
            }
            span(&mut out, &mut plain, i, j, "hl-c");
            i = j;
            continue;
        }
        let block = l.block.or(if l.tags { Some(("<!--", "-->")) } else { None });
        if let Some((open, close)) = block {
            if starts(&s, i, open) && !(l.tags && open == "/*" && in_tag) && !(qwe && in_route_path(&s, i, &mut seen)) {
                let mut j = i + open.chars().count();
                while j < s.len() && !starts(&s, j, close) {
                    j += 1;
                }
                j = (j + close.chars().count()).min(s.len());
                span(&mut out, &mut plain, i, j, "hl-c");
                i = j;
                continue;
            }
        }
        if l.tags && starts(&s, i, "<!--") {
            let mut j = i + 4;
            while j < s.len() && !starts(&s, j, "-->") {
                j += 1;
            }
            j = (j + 3).min(s.len());
            span(&mut out, &mut plain, i, j, "hl-c");
            i = j;
            continue;
        }
        // Tags: `<name`, `</name`.
        if l.tags && c == '<' {
            let mut j = i + 1;
            if j < s.len() && s[j] == '/' {
                j += 1;
            }
            if j < s.len() && s[j].is_alphabetic() {
                let from = j;
                while j < s.len() && (s[j].is_alphanumeric() || matches!(s[j], '-' | '_' | '.' | ':')) {
                    j += 1;
                }
                span(&mut out, &mut plain, from, j, "hl-t");
                in_tag = std::ptr::eq(l, &HTML);
                i = j;
                continue;
            }
        }
        if in_tag {
            if c == '>' {
                in_tag = false;
            } else if c == '"' || c == '\'' {
                let mut j = i + 1;
                while j < s.len() && s[j] != c {
                    j += 1;
                }
                j = (j + 1).min(s.len());
                span(&mut out, &mut plain, i, j, "hl-s");
                i = j;
                continue;
            } else if c.is_alphabetic() || c == '@' || c == ':' {
                let mut j = i;
                while j < s.len() && (s[j].is_alphanumeric() || matches!(s[j], '-' | '_' | ':' | '@' | '.')) {
                    j += 1;
                }
                span(&mut out, &mut plain, i, j, "hl-a");
                i = j;
                continue;
            }
            i += 1;
            continue;
        }
        // Strings. Python's `"""` and `'''` go on over lines, as Rust's `"` does: a docstring's
        // lines were coloured as code. Rust's `'"'` is a character and `'a` a lifetime, and
        // `r"..."` a raw string (FEEDBACK55.md, Q-834; branch feedback55).
        let quoted = if (c == '"' || c == '\'') && py {
            triple_end(&s, i)
        } else if c == '\'' && rust {
            char_end(&s, i)
        } else if (c == 'r' || c == 'b') && rust && !(i > 0 && ident_char(s[i - 1], false)) {
            raw_end(&s, i)
        } else {
            None
        };
        if let Some(j) = quoted {
            span(&mut out, &mut plain, i, j, "hl-s");
            i = j;
            continue;
        }
        // The shell's `it's`: a `'` in a word is no quote (Q-834).
        if c.is_ascii() && l.quotes.as_bytes().contains(&(c as u8)) && !(sh && c == '\'' && i > 0 && ident_char(s[i - 1], false)) {
            let mut j = i + 1;
            while j < s.len() && s[j] != c && (c == '`' || (rust && c == '"') || s[j] != '\n') {
                if s[j] == '\\' {
                    j += 1;
                }
                j += 1;
            }
            j = (j + 1).min(s.len());
            let mut k = j;
            while k < s.len() && (s[k] == ' ' || s[k] == '\t') {
                k += 1;
            }
            let key = l.json && k < s.len() && s[k] == ':';
            span(&mut out, &mut plain, i, j, if key { "hl-a" } else { "hl-s" });
            i = j;
            continue;
        }
        // A JavaScript regular expression: `/"/` started a string (FEEDBACK55.md, Q-834; branch
        // feedback55).
        if c == '/' && js && i >= no_regex && regex_may_start(&s, i) {
            match regex_end(&s, i) {
                Ok(j) => {
                    span(&mut out, &mut plain, i, j, "hl-s");
                    i = j;
                    continue;
                }
                Err(line_end) => no_regex = line_end,
            }
        }
        if l.css {
            match c {
                '{' => depth += 1,
                '}' => depth = (depth - 1).max(0),
                _ => {}
            }
            // A colour, `#c0ffee`, in a rule; a selector's id or class outside.
            if (c == '#' || c == '.') && i + 1 < s.len() && ident_char(s[i + 1], true) && !(c == '.' && s[i + 1].is_ascii_digit()) {
                let mut j = i + 1;
                while j < s.len() && ident_char(s[j], true) {
                    j += 1;
                }
                span(&mut out, &mut plain, i, j, if depth > 0 && c == '#' { "hl-n" } else { "hl-f" });
                i = j;
                continue;
            }
            if c == '@' || (c == '!' && starts(&s, i, "!important")) {
                let mut j = i + 1;
                while j < s.len() && ident_char(s[j], true) {
                    j += 1;
                }
                span(&mut out, &mut plain, i, j, "hl-k");
                i = j;
                continue;
            }
        }
        // Numbers (with a unit, in CSS: `12px`, `50%`).
        let prev_ident = i > 0 && ident_char(s[i - 1], l.css);
        // `-0.5em` is a number in CSS, though `-` is part of its names.
        let prev_word = i > 0 && ident_char(s[i - 1], false);
        // A range's `..` ends a number and starts none: `0..n` was one number (FEEDBACK55.md,
        // Q-834; branch feedback55).
        let dots = |j: usize| s[j] == '.' && (s.get(j + 1) == Some(&'.') || (j > 0 && s[j - 1] == '.'));
        if !prev_word && (c.is_ascii_digit() || (c == '.' && i + 1 < s.len() && s[i + 1].is_ascii_digit() && !dots(i))) {
            let mut j = i;
            while j < s.len() && (s[j].is_ascii_alphanumeric() || (s[j] == '.' && !dots(j)) || s[j] == '_' || (l.css && s[j] == '%')) {
                j += 1;
            }
            span(&mut out, &mut plain, i, j, "hl-n");
            i = j;
            continue;
        }
        // Words.
        if ident_start(c) && !prev_ident {
            let mut j = i;
            while j < s.len() && ident_char(s[j], l.css) {
                j += 1;
            }
            let word: String = s[i..j].iter().collect();
            // The word itself, not a copy of it: this, and a quote looked for among bytes
            // (above), keep highlighting as fast as before the checks Q-834 added
            // (FEEDBACK55.md; branch feedback55).
            let lower;
            let probe = if l.nocase {
                lower = word.to_ascii_lowercase();
                &lower
            } else {
                &word
            };
            let mut k = j;
            while k < s.len() && (s[k] == ' ' || s[k] == '\t') {
                k += 1;
            }
            let next = s.get(k).copied().unwrap_or(' ');
            let class = if l.css {
                if depth > 0 && next == ':' {
                    Some("hl-a")
                } else if depth == 0 {
                    Some("hl-t")
                } else {
                    None
                }
            } else if l.keywords.contains(&probe.as_str()) {
                Some("hl-k")
            } else if l.consts.contains(&probe.as_str()) {
                Some("hl-n")
            } else if next == '(' {
                Some("hl-f")
            } else if l.types && c.is_uppercase() {
                Some("hl-t")
            } else {
                None
            };
            match class {
                Some(cl) => span(&mut out, &mut plain, i, j, cl),
                None => {}
            }
            i = j;
            continue;
        }
        i += 1;
    }
    escape_into(&mut out, &s[plain..]);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::highlight;

    #[test]
    fn quillo_is_coloured_and_escaped() {
        let h = highlight("let x = f(\"<a>\") // hi", "qwe");
        assert_eq!(h, "<span class=\"hl-k\">let</span> x = <span class=\"hl-f\">f</span>(<span class=\"hl-s\">&quot;&lt;a&gt;&quot;</span>) <span class=\"hl-c\">// hi</span>");
    }

    #[test]
    fn css_properties_values_and_selectors() {
        let h = highlight(".a { color: #fff; margin: -0.5em }", "css");
        assert_eq!(h, "<span class=\"hl-f\">.a</span> { <span class=\"hl-a\">color</span>: <span class=\"hl-n\">#fff</span>; <span class=\"hl-a\">margin</span>: -<span class=\"hl-n\">0.5em</span> }");
    }

    #[test]
    fn html_tags_and_attributes() {
        let h = highlight("<a href=\"/x\">y</a>", "html");
        assert_eq!(h, "&lt;<span class=\"hl-t\">a</span> <span class=\"hl-a\">href</span>=<span class=\"hl-s\">&quot;/x&quot;</span>&gt;y&lt;/<span class=\"hl-t\">a</span>&gt;");
    }

    #[test]
    fn a_route_path_starts_no_comment() {
        // `/*rest` is a path, as the lexer reads it; the rest of the line is code.
        let h = highlight("route ANY /*rest = respond(404, rest) // last", "qwe");
        assert!(h.starts_with("<span class=\"hl-k\">route</span> <span class=\"hl-t\">ANY</span> /*rest = <span class=\"hl-f\">respond</span>"), "{}", h);
        assert!(h.ends_with("<span class=\"hl-c\">// last</span>"), "{}", h);
        let h = highlight("routes /*x with db { }\nmount /*m = \"a.qwe\"", "qwe");
        assert!(!h.contains("hl-c"), "{}", h);
        assert!(h.contains("<span class=\"hl-k\">with</span>"), "{}", h);
        // Elsewhere `/*` is a comment still, a lower-case word before it too.
        assert!(highlight("let x = 1 /* one */", "qwe").contains("<span class=\"hl-c\">/* one */</span>"));
        assert!(highlight("route get /*x*/", "qwe").contains("hl-c"));
        assert!(highlight("route GET /a /* note */", "qwe").contains("<span class=\"hl-c\">/* note */</span>"));
    }

    #[test]
    fn unknown_languages_are_only_escaped() {
        assert_eq!(highlight("<b> & x", "nope"), "&lt;b&gt; &amp; x");
    }
}
