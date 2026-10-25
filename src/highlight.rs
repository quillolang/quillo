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
    keywords: &["let", "fn", "view", "component", "server", "route", "use", "if", "else", "match", "for", "in", "return", "reply", "as", "while", "break", "continue", "type", "and", "or", "not"],
    consts: &["true", "false", "null", "int", "str", "bool", "float", "map", "list", "any"],
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

fn ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == '$'
}

fn ident_char(c: char, css: bool) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$' || (css && c == '-')
}

pub fn highlight(code: &str, lang: &str) -> String {
    let l = lang_of(lang);
    let s: Vec<char> = code.chars().collect();
    let mut out = String::with_capacity(code.len() * 2);
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
        let c = s[i];
        // Comments.
        if let Some(p) = l.line.iter().find(|p| starts(&s, i, p)) {
            let _ = p;
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
            if starts(&s, i, open) && !(l.tags && open == "/*" && in_tag) {
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
        // Strings.
        if l.quotes.contains(c) {
            let mut j = i + 1;
            while j < s.len() && s[j] != c && (c == '`' || s[j] != '\n') {
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
        if !prev_word && (c.is_ascii_digit() || (c == '.' && i + 1 < s.len() && s[i + 1].is_ascii_digit())) {
            let mut j = i;
            while j < s.len() && (s[j].is_ascii_alphanumeric() || s[j] == '.' || s[j] == '_' || (l.css && s[j] == '%')) {
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
            let probe = if l.nocase { word.to_ascii_lowercase() } else { word.clone() };
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
    out
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
    fn unknown_languages_are_only_escaped() {
        assert_eq!(highlight("<b> & x", "nope"), "&lt;b&gt; &amp; x");
    }
}
