//! `db.transaction`'s options: an isolation level, and trying again when
//! another transaction got in the way.
//!
//! ```text
//! db.transaction(() => { ... })                                      // as before
//! db.transaction({ isolation: "serializable", retries: 3 }, () => { ... })
//! ```
//!
//! - `isolation`: `"serializable"`, `"repeatable_read"` or `"read_committed"`.
//!   PostgreSQL begins with `begin isolation level ...`; MySQL runs `set
//!   transaction isolation level ...` before `begin` (it applies to that
//!   transaction only). SQLite has one writer at a time, so its transactions
//!   are serializable whatever is asked; with options, it begins with
//!   `begin immediate`, taking the write lock at once rather than at the
//!   first write (where another process holding it would fail the
//!   transaction part way).
//! - `retries`: how many times to run the whole function again when the
//!   database gave up on the transaction because of another one: PostgreSQL's
//!   serialization failures and deadlocks (SQLSTATE 40001 and 40P01),
//!   MySQL's deadlocks and lock wait timeouts (1213, 1205), and SQLite's
//!   busy database (another process writing). Each try waits a little
//!   longer than the one before, with some randomness, so the transactions
//!   that collided do not collide again. Any other failure ends it at once.
//!
//! A nested transaction is a savepoint inside the outermost one, whose
//! isolation it cannot change: asking is an error. Only the outermost
//! retries (a conflict aborts the whole transaction, not a savepoint).
//!
//! Once a statement has failed that way, the transaction is over: Postgres
//! refuses the rest, and MySQL has already rolled it back (what came next
//! would run outside any transaction). So the next statements of the try
//! fail at once (see `db::Pinned`), and a function that ignores the error
//! still does not commit.

use crate::interp::{fail, Ctrl, Interp};
use crate::value::*;

type R = Result<Value, Ctrl>;

/// A transaction's isolation level.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Isolation {
    Serializable,
    RepeatableRead,
    ReadCommitted,
}

impl Isolation {
    fn sql(self) -> &'static str {
        match self {
            Isolation::Serializable => "serializable",
            Isolation::RepeatableRead => "repeatable read",
            Isolation::ReadCommitted => "read committed",
        }
    }
}

/// What `db.transaction({ ... }, f)` was given.
#[derive(Clone, Debug, Default)]
pub struct Options {
    pub isolation: Option<Isolation>,
    pub retries: u32,
    /// Options were given at all (SQLite then begins `immediate`).
    pub given: bool,
}

/// The most `retries` may be: past that, something else is wrong.
const MAX_RETRIES: i64 = 20;

/// Reads `{ isolation, retries }` (other keys are left to the caller: see
/// `idempotent`, which takes these too).
pub fn options(v: &Value, f: &str, line: u32) -> Result<Options, Ctrl> {
    let Value::Map(m) = v else {
        return fail(line, format!("`{}` expects options as a map, like {{ isolation: \"serializable\", retries: 3 }}", f));
    };
    let mut o = Options { given: true, ..Options::default() };
    match m.get("isolation") {
        None | Some(Value::Nil) => {}
        Some(Value::Str(s)) => {
            o.isolation = Some(match &**s {
                "serializable" => Isolation::Serializable,
                "repeatable_read" => Isolation::RepeatableRead,
                "read_committed" => Isolation::ReadCommitted,
                other => {
                    return fail(
                        line,
                        format!("`{}`'s isolation is \"serializable\", \"repeatable_read\" or \"read_committed\", got {:?}", f, other),
                    )
                }
            })
        }
        Some(other) => return fail(line, format!("`{}`'s isolation is a string, got {}", f, other.type_name())),
    }
    match m.get("retries") {
        None | Some(Value::Nil) => {}
        Some(Value::Int(n)) if (0..=MAX_RETRIES).contains(n) => o.retries = *n as u32,
        Some(other) => return fail(line, format!("`{}`'s retries is a number from 0 to {}, got {}", f, MAX_RETRIES, other.repr())),
    }
    Ok(o)
}

/// The statement that begins the outermost transaction.
pub(crate) fn begin_sql(mysql: bool, isolation: Option<Isolation>) -> String {
    match (isolation, mysql) {
        (None, _) => "begin".into(),
        (Some(i), false) => format!("begin isolation level {}", i.sql()),
        // For the next transaction only.
        (Some(i), true) => format!("set transaction isolation level {}; begin", i.sql()),
    }
}

/// Runs `body` in a transaction on connection `on` (None: the program's
/// database), trying again as the options allow.
pub fn run(it: &mut Interp, on: Option<usize>, opts: &Options, line: u32, body: &mut dyn FnMut(&mut Interp) -> R) -> R {
    let mut tries = 0;
    loop {
        let (r, conflict) = crate::db::attempt(it, on, opts, line, body);
        if !conflict || tries >= opts.retries {
            return r;
        }
        tries += 1;
        backoff(it, tries);
    }
}

/// Waits before try `n` (1, 2, ...): about 10 ms, then twice as long each
/// time up to 320 ms, give or take half, so two transactions that collided
/// are unlikely to start again together.
fn backoff(it: &mut Interp, n: u32) {
    let base = 10u64 << (n - 1).min(5);
    let ms = fastrand::u64(base / 2..=base + base / 2);
    it.unlocked(move || std::thread::sleep(std::time::Duration::from_millis(ms)));
}

/// What a try that conflicted gives when there are no tries left, and its
/// function did not see the error (it went on, or the commit failed).
pub(crate) fn gave_up() -> Value {
    Value::err(Value::str(
        "db: the transaction was rolled back: it conflicted with another (a serialization failure or a deadlock); try again, or give it retries",
    ))
}
