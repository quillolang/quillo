//! `run_script(source, vars)`: code a program's users write (a rule on a
//! form's field, who may see a record) run on the server, safely. The
//! script sees only the variables it is given and the builtins that compute
//! (no database, files, network, clock, sessions or replies); it cannot
//! define functions, routes or state; and it stops after a budget of steps.
//! What it gives back is its last expression's value, or an error value
//! saying what went wrong (a parse error, a name it may not use, too many
//! steps), so the program can show it as a message.

use crate::ast::*;
use crate::interp::{Ctrl, Interp};
use crate::value::*;

/// Builtins a script may call: they compute from their arguments and
/// nothing else (and cannot make huge values from small ones).
const SAFE: &[&str] = &[
    "str", "int", "float", "len", "type_of", "upper", "lower", "normalize", "local_path", "trim", "filled", "split", "join", "contains", "starts_with", "ends_with",
    "replace", "slice", "sum", "min", "max", "sort", "reverse", "first", "last", "push", "enumerate", "keys", "values", "get", "has",
    "set", "remove", "merge", "err", "is_err", "unwrap", "escape", "url_encode", "abs", "round", "floor", "ceil", "sqrt", "pow", "unique", "flatten",
    "zip", "take", "drop", "index_of", "map", "filter", "find", "any", "all", "reduce", "sort_by", "count", "group_by",
    // A decimal from text, as `int` and `float` are; and what `{ ...m }` and `[...xs]` are
    // written as (src/parser.rs), which a script's spread called an unknown `__concat`
    // (FEEDBACK72.md, Q-1013; branch feedback72-81).
    "dec", "__merge", "__concat",
];

/// Modules a script may use whole, as `date` without its clock: they compute from their
/// arguments and nothing else, where the guide's names for them were each an unknown name in a
/// formula (FEEDBACK72.md, Q-1013; branch feedback72-81). `stats.histogram`'s bins are held to
/// what a script may make (`would_be_too_big`); `re` keeps its guard, and the script's limits
/// hold inside its searches (src/re.rs, `script_room`). Not `vec`: `vec.zeros`, `vec.full`,
/// `vec.linspace`, `vec.arange` and `vec.matmul` make big values from small ones.
const SAFE_MODULES: &[&str] = &["math", "stats", "re", "bit"];

/// Steps (calls and loop rounds) a script may take.
pub const SCRIPT_STEPS: u64 = 20_000;

/// How long a script may take, in milliseconds, unless `run_script`'s `timeout` says, and the
/// most that may say: a step can read up to 1 MB, so steps alone let a 143-byte script hold its
/// instance for 13 s (FEEDBACK75.md, Q-1055; branch feedback75).
pub const SCRIPT_MS: u64 = 100;
const MOST_SCRIPT_MS: u64 = 60_000;

/// The most a value a script makes may hold, in bytes, roughly: a string's
/// bytes, and eight for each number, item or entry, through nested lists
/// and maps. Each 1 KB made also costs a step.
pub const SCRIPT_BYTES: usize = 1 << 20;

/// What a script is told of a value past `SCRIPT_BYTES`.
pub const TOO_BIG: &str = "the script made a value larger than 1 MB";

/// How deep a script's lists and maps may nest.
const SCRIPT_NESTING: usize = 256;

/// The size of `v`, made by a script, or why it is too big. It counts no
/// further than the limit, so a check costs at most that.
pub fn too_big(v: &Value) -> Result<usize, &'static str> {
    let mut n = 0usize;
    let mut todo = vec![(v, 0usize)];
    while let Some((v, depth)) = todo.pop() {
        if depth > SCRIPT_NESTING {
            return Err("the script nested lists or maps too deeply");
        }
        match v {
            Value::Str(s) | Value::Html(s) | Value::Css(s) => n += s.len(),
            Value::List(l) => {
                n = n.saturating_add(8 * l.len());
                if n <= SCRIPT_BYTES {
                    todo.extend(l.iter().map(|x| (x, depth + 1)));
                }
            }
            Value::Map(m) => {
                for (k, x) in m.iter() {
                    n += 8 + k.len();
                    if n > SCRIPT_BYTES {
                        break;
                    }
                    todo.push((x, depth + 1));
                }
            }
            Value::Err(x, _, _) => todo.push((x, depth + 1)),
            _ => n += 8,
        }
        if n > SCRIPT_BYTES {
            return Err(TOO_BIG);
        }
    }
    Ok(n)
}

/// What a builtin or an operator reads of `v`, in bytes, counted as `too_big` counts what a script
/// makes and no further than `most` (FEEDBACK75.md, Q-1055; branch feedback75). Numbers and text
/// cost no list to count with.
pub fn read_size(v: &Value, most: usize) -> usize {
    match v {
        Value::Str(s) | Value::Html(s) | Value::Css(s) => return s.len(),
        Value::List(_) | Value::Map(_) | Value::Err(..) => {}
        _ => return 8,
    }
    let mut n = 0usize;
    let mut todo = vec![v];
    while let Some(v) = todo.pop() {
        match v {
            Value::Str(s) | Value::Html(s) | Value::Css(s) => n = n.saturating_add(s.len()),
            Value::List(l) => {
                n = n.saturating_add(l.len().saturating_mul(8));
                todo.extend(l.iter().filter(|x| matches!(x, Value::Str(_) | Value::Html(_) | Value::Css(_) | Value::List(_) | Value::Map(_) | Value::Err(..))));
            }
            Value::Map(m) => {
                for (k, x) in m.iter() {
                    n = n.saturating_add(8 + k.len());
                    if matches!(x, Value::Str(_) | Value::Html(_) | Value::Css(_) | Value::List(_) | Value::Map(_) | Value::Err(..)) {
                        todo.push(x);
                    }
                }
            }
            Value::Err(x, _, _) => todo.push(x),
            _ => n = n.saturating_add(8),
        }
        if n >= most {
            break;
        }
    }
    n
}

/// Whether builtin `name`'s first argument is paid for otherwise than as read: looked into without
/// reading it through (a list's or a map's length, an entry, an end, a type), or copied into what
/// it gives back, which is paid for as made (`push`, `set`, `merge`, `remove`, `sort`, `reverse`,
/// the cases of a text): `out = push(out, x)` paid twice for `out`, and a loop 0.8.8 ran stopped
/// (FEEDBACK75.md, Q-1055, found in review; branch feedback75). A text's length is counted through
/// it, so `len` of a text reads it.
pub fn looks_into(name: &str, args: &[Value]) -> bool {
    match name {
        "get" | "has" | "first" | "last" | "type_of" | "is_err" | "unwrap" | "err" | "filled" => true,
        "push" | "set" | "merge" | "remove" | "sort" | "reverse" | "upper" | "lower" => true,
        "len" | "count" => !matches!(args.first(), Some(Value::Str(_) | Value::Css(_))) && args.len() == 1,
        _ => false,
    }
}

/// `run_script`'s third argument, `{ timeout }`: milliseconds (`250`) or text (`"250ms"`,
/// `"2s"`), from 1 ms to 60 s; 100 ms when not given (FEEDBACK75.md, Q-1055; branch feedback75).
pub fn timeout_of(opts: Option<&Map>) -> Result<u64, String> {
    let Some(m) = opts else { return Ok(SCRIPT_MS) };
    if let Some(k) = m.keys().find(|k| k.as_ref() != "timeout") {
        return Err(format!("`run_script`: unknown option `{}` (the option is timeout)", k));
    }
    let wrong = |v: &Value| format!("`run_script`: `timeout` is milliseconds from 1 to {}, as 250, \"250ms\" or \"2s\", got {}", MOST_SCRIPT_MS, v.repr());
    let ms = match m.get("timeout") {
        None | Some(Value::Nil) => return Ok(SCRIPT_MS),
        Some(Value::Int(n)) => Some(*n as f64),
        Some(Value::Str(t) | Value::Css(t)) => {
            let t = t.trim();
            let (num, scale) = match t.strip_suffix("ms") {
                Some(n) => (n, 1.0),
                None => (t.strip_suffix('s').unwrap_or("x"), 1000.0),
            };
            num.trim().parse::<f64>().ok().filter(|n| n.is_finite()).map(|n| n * scale)
        }
        _ => None,
    };
    match ms {
        Some(ms) if (1.0..=MOST_SCRIPT_MS as f64).contains(&ms) => Ok(ms.round() as u64),
        _ => Err(wrong(m.get("timeout").unwrap_or(&Value::Nil))),
    }
}

/// Whether builtin `name` would make a value over the limit from `args`,
/// checked before it runs: `join`, `replace` and `split` can make far more
/// than they are given.
pub fn would_be_too_big(name: &str, args: &[Value]) -> bool {
    let text = |i: usize| match args.get(i) {
        Some(Value::Str(s) | Value::Css(s)) => Some(&**s),
        _ => None,
    };
    let times = |s: &str, part: &str| if part.is_empty() { s.chars().count() + 1 } else { s.matches(part).count() + 1 };
    let made = match name {
        "join" => match args.first() {
            Some(Value::List(l)) => {
                let sep = text(1).map_or(0, str::len);
                too_big(&args[0]).unwrap_or(usize::MAX).saturating_add(sep.saturating_mul(l.len()))
            }
            _ => 0,
        },
        "replace" => match (text(0), text(1), text(2)) {
            (Some(s), Some(from), Some(to)) => s.len().saturating_add(times(s, from).saturating_mul(to.len())),
            _ => 0,
        },
        "split" => match text(0) {
            Some(s) => s.len().saturating_add(8 * times(s, text(1).unwrap_or(" "))),
            _ => 0,
        },
        // Each list's items, 8 bytes each at the least: `[...xs, ...xs, ...]` (FEEDBACK72.md,
        // Q-1013; branch feedback72-81).
        "__concat" => args.iter().map(|v| if let Value::List(l) = v { l.len() } else { 0 }).fold(0usize, usize::saturating_add).saturating_mul(8),
        // Its counts and its edges, two lists of a number a bin, from one int that may ask for a
        // million bins: 16 bytes a number in a list, as `too_big` counts (FEEDBACK72.md, Q-1013;
        // branch feedback72-81).
        // A whole float too, as `stats.histogram` takes it (src/numeric.rs, `int`): 1000000.0 made
        // its 99 MB (found in review).
        "stats.histogram" => match args.get(1) {
            Some(Value::Int(bins)) => usize::try_from(*bins).unwrap_or(0).saturating_mul(32),
            Some(Value::Float(x)) if x.fract() == 0.0 && *x >= 1.0 && *x < 9.0e15 => (*x as usize).saturating_mul(32),
            _ => 0,
        },
        _ => 0,
    };
    made > SCRIPT_BYTES
}

fn bad(what: &str) -> Value {
    Value::err(Value::str(what))
}

/// What a script may not contain, if anything.
fn forbidden_stmt(s: &Stmt) -> Option<&'static str> {
    match s {
        Stmt::Let(_, e, _) | Stmt::Assign(_, _, e, _) | Stmt::Expr(e) | Stmt::Return(Some(e), _) => forbidden(e),
        Stmt::SetField(o, _, _, e, _) => forbidden(o).or_else(|| forbidden(e)),
        Stmt::Return(None, _) => None,
        Stmt::Fn(_) => Some("a script cannot define functions (use `(x) => ...`)"),
        Stmt::Route(_) | Stmt::Mount(..) => Some("a script cannot define routes"),
        Stmt::Service(..) => Some("a script cannot call services"),
        Stmt::Type(td) => td.defaults.iter().find_map(|(_, e)| forbidden(e)),
        Stmt::Use(..) => Some("a script cannot `use` files"),
        Stmt::State(..) => Some("a script cannot keep state"),
    }
}

fn forbidden(e: &Expr) -> Option<&'static str> {
    let all = |es: &[Expr]| es.iter().find_map(forbidden);
    match e {
        Expr::Lit(_) | Expr::Ident(..) => None,
        Expr::Template(parts) => parts.iter().find_map(|p| if let TPart::Expr(e) = p { forbidden(e) } else { None }),
        Expr::List(es) => all(es),
        Expr::Map(kvs) => kvs.iter().find_map(|(_, e)| forbidden(e)),
        Expr::Unary(_, a, _) | Expr::Try(a) | Expr::Field(a, _, _) => forbidden(a),
        Expr::Binary(_, a, b, _) | Expr::Coalesce(a, b) | Expr::Pipe(a, b, _) | Expr::Index(a, b, _) => forbidden(a).or_else(|| forbidden(b)),
        Expr::Call(f, args, _) => forbidden(f).or_else(|| all(args)),
        Expr::If(c, a, b) => forbidden(c).or_else(|| forbidden(a)).or_else(|| b.as_deref().and_then(forbidden)),
        Expr::Match(x, arms, _) => forbidden(x).or_else(|| arms.iter().find_map(|a| a.guard.as_ref().and_then(forbidden).or_else(|| forbidden(&a.body)))),
        Expr::For(_, it, body, _) | Expr::While(it, body, _) => forbidden(it).or_else(|| forbidden(body)),
        Expr::Block(stmts) => stmts.iter().find_map(forbidden_stmt),
        Expr::Lambda(def) => def.params.iter().find_map(|p| p.default.as_ref().and_then(forbidden)).or_else(|| forbidden(&def.body)),
        Expr::Reply(..) => Some("a script cannot reply"),
        Expr::Html(..) => Some("a script gives values, not markup"),
    }
}

/// `--max-memory`'s stop is the request's, not the script's error: it ends the request through
/// the script, as no `??` takes it (FEEDBACK71.md, Q-977, found in review: `run_script(...) ??
/// "caught"` caught it; branch security).
pub fn run_script(it: &mut Interp, src: &str, vars: Option<&Map>) -> Result<Value, Ctrl> {
    run_script_for(it, src, vars, SCRIPT_MS)
}

/// `run_script`, stopped after `ms` milliseconds (FEEDBACK75.md, Q-1055; branch feedback75).
pub fn run_script_for(it: &mut Interp, src: &str, vars: Option<&Map>, ms: u64) -> Result<Value, Ctrl> {
    // Its world: the safe builtins, then its variables (`run_parsed`). Quillo's own, not what the
    // program's globals have by those names: a program's `fn upper` was the script's `upper`, run
    // with the program's effects (FEEDBACK51.md, Q-648; branch feedback51-fixes). Made first: its
    // address is the run's number, which each function written in the source carries (`run_of`,
    // `parsing_run`; Q-1274, found reviewing the trusted CSS of FEEDBACK79.md's Q-1250; branch
    // feedback72-81).
    let root = ROOT.with(Scope::root_copy);
    // Its clock starts as it is read: a source near `--max-body` takes its time to parse. Not
    // before its world is there: the first script on a thread paid for making the world, and every
    // builtin Quillo has (`BUILTINS`), out of its own 100 ms (FEEDBACK75.md, Q-1055, found by the
    // coverage tests of feedback72-81; branch feedback72-81).
    let until = std::time::Instant::now() + std::time::Duration::from_millis(ms);
    // Names are kept for good, and this source may be a visitor's: its own are its own
    // (src/sym.rs), and messages say them as it wrote them.
    let (parsed, names) = {
        let _parsing = Parsing::of(run_of(&root));
        crate::sym::Sym::in_script(|| crate::parser::Parser::new(src).parse_program())
    };
    let out = match parsed {
        Ok(stmts) => run_parsed(it, &stmts, &names, vars, (until, ms), root)?,
        Err(e) => bad(&format!("line {}: {}", e.line, e.msg)),
    };
    // Each name a message quotes, cut at 100 characters: an unknown name of 1 MB was said whole,
    // into the log with each request (FEEDBACK75.md, Q-1076, found in review; branch feedback75).
    // Quillo's own messages, not an error value the script made itself.
    Ok(match &out {
        Value::Err(e, own, _) if matches!(e.value(), Value::Str(_) | Value::Css(_)) && (!names.is_empty() || (!own && e.display().len() > 100)) => {
            let msg = e.display();
            let msg = if names.is_empty() { msg } else { said_as_written(&msg, &names) };
            if !own && msg.len() > 100 {
                bad(&quoted_cut(&msg))
            } else if names.is_empty() {
                out
            } else {
                bad(&msg)
            }
        }
        _ => out,
    })
}

/// `msg` with each part in backticks cut at 100 characters, as `interp::said_name` cuts a name.
fn quoted_cut(msg: &str) -> String {
    let mut out = String::with_capacity(msg.len().min(4096));
    let mut quoted = false;
    for (i, part) in msg.split('`').enumerate() {
        if i > 0 {
            out.push('`');
        }
        out.push_str(if quoted { crate::interp::said_name(part) } else { part.into() }.as_ref());
        quoted = !quoted;
    }
    out
}

/// `msg` with a script's own names as it wrote them: a message quotes a name in backticks,
/// `` `$2` ``, and that is put back as `` `total` ``. Other text is the script's own.
fn said_as_written(msg: &str, names: &std::collections::HashMap<String, Sym>) -> String {
    let back: std::collections::HashMap<&str, &str> = names.iter().map(|(k, v)| (v.as_str(), k.as_str())).collect();
    let mut out = String::with_capacity(msg.len());
    let mut rest = msg;
    while let Some(at) = rest.find("`$") {
        out.push_str(&rest[..at]);
        let digits = rest[at + 2..].bytes().take_while(u8::is_ascii_digit).count();
        let token = &rest[at + 1..at + 2 + digits];
        match back.get(token) {
            Some(name) if rest[at + 2 + digits..].starts_with('`') => {
                out.push('`');
                out.push_str(name);
            }
            _ => out.push_str(&rest[at..at + 2 + digits]),
        }
        rest = &rest[at + 2 + digits..];
    }
    out.push_str(rest);
    out
}

/// A run's number, never 0, which is the program's: the address of its world, `root` (`FnDef::run`).
/// No other run has it while a function of this one is about, as each holds the world through its
/// scope (Q-1274, found reviewing the trusted CSS of FEEDBACK79.md's Q-1250; branch feedback72-81).
pub fn run_of(root: &Env) -> u64 {
    std::sync::Arc::as_ptr(root) as u64
}

thread_local! {
    /// The run whose source is being read, 0 for the program's (`parsing_run`).
    static PARSING: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// The run whose source the parser is reading, which each function written in it carries
/// (`FnDef::run`): 0 but while a script's own source is read, not the program's that a helper
/// loads while a script runs. For the thread: reading a source never waits, so no other request
/// runs on it meanwhile (Q-1274, found reviewing the trusted CSS of FEEDBACK79.md's Q-1250;
/// branch feedback72-81).
pub fn parsing_run() -> u64 {
    PARSING.with(std::cell::Cell::get)
}

/// `PARSING` set for a script's source, and put back however its reading ends.
struct Parsing(u64);

impl Parsing {
    fn of(run: u64) -> Parsing {
        Parsing(PARSING.with(|p| p.replace(run)))
    }
}

impl Drop for Parsing {
    fn drop(&mut self) {
        PARSING.with(|p| p.set(self.0));
    }
}

fn run_parsed(it: &mut Interp, stmts: &[Stmt], names: &std::collections::HashMap<String, Sym>, vars: Option<&Map>, clock: (std::time::Instant, u64), root: Env) -> Result<Value, Ctrl> {
    let env = Scope::child(&root);
    // A variable the script never names is none of its business, and makes no name. Trusted CSS
    // in one is text in the script's world, where css`...` makes text too (`Interp::eval_call`):
    // the script's source may be a visitor's (FEEDBACK79.md, Q-1250; branch feedback72-81).
    if let Some(m) = vars {
        for (k, v) in m.iter() {
            if let Some(n) = names.get(k.as_str()).copied().or_else(|| Sym::known(k.as_str())) {
                define(&env, n, Value::plain_deep(v).unwrap_or_else(|| v.clone()));
            }
        }
    }
    // A budget of steps, and no effects for what the script reaches by
    // itself; helpers the program passes in keep what the program may do
    // (`Interp::script_mask`).
    let saved = std::mem::replace(&mut it.fuel, SCRIPT_STEPS);
    let saved_allowed = std::mem::replace(&mut it.allowed, 0);
    let saved_restrictor = it.restrictor.take();
    // A script a helper of another's runs ends by the outer one's time too (FEEDBACK75.md,
    // Q-1055; branch feedback75).
    let clock = match &it.script {
        Some((_, _, until, ms)) if *until < clock.0 => (*until, *ms),
        _ => clock,
    };
    let saved_script = it.script.replace((root.clone(), saved_allowed, clock.0, clock.1));
    // Its number, which each function it makes carries: one runs only while this run does, as
    // the script's, called back by a helper too; kept past it, it is not run (`FnDef::run`;
    // Q-1274, found reviewing the trusted CSS of FEEDBACK79.md's Q-1250; branch feedback72-81).
    let outer_runs = it.script_runs.len();
    it.script_runs.push(run_of(&root));
    let outer_helper = std::mem::replace(&mut it.in_helper, false);
    let mut stopped = None;
    // Room on the stack for code nested as deeply as the parser allows.
    let out = crate::stack::for_nesting(crate::parser::MAX_NESTING, || {
        if let Some(why) = stmts.iter().find_map(forbidden_stmt) {
            return bad(why);
        }
        let mut last = Value::Nil;
        for s in stmts.iter() {
            match it.exec_stmt(s, &env) {
                Ok(v) => last = v,
                Err(Ctrl::Return(v)) => return v,
                Err(Ctrl::Error(e)) if e.memory_stop => {
                    stopped = Some(e);
                    return Value::Nil;
                }
                Err(Ctrl::Error(e)) => {
                    let msg = left_out(&e.msg).unwrap_or_else(|| e.msg.clone());
                    return bad(&if e.line > 0 { format!("line {}: {}", e.line, msg) } else { msg });
                }
                Err(Ctrl::Reply(_)) => return bad("a script cannot reply"),
            }
        }
        last
    });
    // Its world let go of first: a lambda it gave a name there (`upper = (x) => x`) held its
    // scope, and so the world, for ever (FEEDBACK72.md, Q-1013, found in review; branch
    // feedback72-81). Then its own variables, a lambda it kept in one too, which held them for
    // ever (FEEDBACK65.md, Q-941; branch feedback65).
    Scope::forget(&root);
    Scope::done(env);
    it.fuel = saved;
    it.allowed = saved_allowed;
    it.restrictor = saved_restrictor;
    it.script = saved_script;
    it.script_runs.truncate(outer_runs);
    it.in_helper = outer_helper;
    if let Some(e) = stopped {
        return Err(Ctrl::Error(e));
    }
    // A function it made would run after it, with no budget of steps: `(n) => ...` doubling a
    // text made 16 MB, and a `while true` ran for good (FEEDBACK51.md, Q-670; branch
    // feedback51-fixes).
    if holds_function(&out) {
        return Ok(bad("a script gives back no function (it would run without the script's limits): give back what it computes"));
    }
    // Nothing trusted leaves it: a helper the program gave it may have handed it trusted CSS
    // (FEEDBACK79.md, Q-1250; branch feedback72-81).
    Ok(Value::plain_deep(&out).unwrap_or(out))
}

/// `format_number` in a script: the program's, but its locale is the script's to give, en-US
/// where it gives none. The visitor's, as the program's call takes, would make one script's
/// answer differ from one visitor to the next, and a script reads only what it is given
/// (FEEDBACK72.md, Q-1013; branch feedback72-81).
fn format_number(it: &mut Interp, mut args: Vec<Value>, line: u32) -> Result<Value, Ctrl> {
    match args.get(1) {
        None if args.len() == 1 => args.push(Value::str("en-US")),
        Some(Value::Nil) => args[1] = Value::str("en-US"),
        _ => {}
    }
    crate::money::format_number_fn(it, args, line)
}

/// "unknown name `range`" said of one of Quillo's own builtins that a script does not have: what
/// it is, and why it is not there. It read as a typo (FEEDBACK72.md, Q-1013; branch
/// feedback72-81). A script's own names are `$1`, ... here (src/sym.rs), never a builtin's.
fn left_out(msg: &str) -> Option<String> {
    let name = msg.strip_prefix("unknown name `")?.strip_suffix('`')?;
    if !BUILTINS.with(|b| lookup(b, name).is_some()) || name.starts_with("__") {
        return None;
    }
    let why = match name {
        "range" | "repeat" | "join_on" | "vec" => "it can make a huge value from a small one",
        "raw" | "markdown" | "markdown_page" | "highlight" => "a script gives values, not markup",
        _ => "a script has only the builtins the guide lists for `run_script`, which reach nothing outside it: no database, files, network, clock, randomness, sessions or replies",
    };
    Some(format!("`{}` is not available in a script: {}", name, why))
}

thread_local! {
    /// A script's world, made once on each thread: the safe builtins, `date` without its clock
    /// (a script gives the same answer each time) and the script's `format_number`. Each script
    /// runs in a copy (`Scope::root_copy`), as it may assign to a name here. Made again for each
    /// script, the eight names Q-1013 added made 50,000 short scripts 6% slower (FEEDBACK72.md,
    /// Q-1013; branch feedback72-81).
    static ROOT: Env = {
        let root = Scope::root();
        BUILTINS.with(|builtins| {
            for name in SAFE.iter().chain(SAFE_MODULES) {
                if let Some(v) = lookup(builtins, name) {
                    define(&root, name, v);
                }
            }
        });
        let mut date = Map::new();
        crate::dates::install_pure(&mut date);
        define(&root, "date", Value::map(date));
        define(&root, "format_number", Value::Builtin("format_number", format_number));
        root
    };

    /// The builtins as Quillo installs them, for scripts' safe names.
    static BUILTINS: Env = {
        let g = Scope::root();
        crate::builtins::install(&g);
        g
    };
}

/// Whether a function is in `v`, however deep.
fn holds_function(v: &Value) -> bool {
    let mut todo = vec![v];
    while let Some(v) = todo.pop() {
        match v {
            Value::Func(_) => return true,
            Value::List(l) => todo.extend(l.iter()),
            Value::Map(m) => todo.extend(m.values()),
            Value::Err(e, ..) => todo.push(e),
            _ => {}
        }
    }
    false
}
