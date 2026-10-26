//! examples/ledger.qwe under load: many transfers at once between a few
//! accounts, each sent again with its Idempotency-Key (some while the first
//! is still running, as a client that timed out would), on SQLite and on
//! PostgreSQL. Then the books must balance: every entry adds up to zero,
//! the totals have not moved, no customer is below zero, and each transfer
//! that was made is there exactly once.

mod support;

use quillo::Interp;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use support::{send, serve};

const EUR: &[&str] = &["alice", "bob", "carol"];
const USD: &[&str] = &["dan", "erin"];
const THREADS: usize = 8;
const EACH: usize = 25;

/// Answers that said to try again (409, 5xx), by the server's port.
static AGAIN: Mutex<Vec<(u16, String)>> = Mutex::new(Vec::new());

fn example() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/ledger.qwe")
}

/// What became of one transfer: its request, and the final answer.
struct Done {
    key: String,
    body: String,
    status: u16,
    answer: String,
    from: String,
    to: String,
    cents: i64,
}

/// Sends a transfer until it has a final answer, as a careful client
/// would: again, with the same key, after a 409 (the first is still
/// running), a 5xx or a dropped connection.
fn until_final(port: u16, key: &str, body: &str) -> (u16, String) {
    for _ in 0..200 {
        let a = send(port, "POST", "/api/transfers", &[("Idempotency-Key", key)], body);
        if a.status == 201 || a.status == 422 {
            return (a.status, a.body);
        }
        AGAIN.lock().unwrap().push((port, format!("{} {}", a.status, a.body)));
        std::thread::sleep(Duration::from_millis(20 + fastrand::u64(0..30)));
    }
    panic!("no final answer for {} {}", key, body);
}

fn transfers(port: u16) -> Vec<Done> {
    let done = Arc::new(Mutex::new(Vec::new()));
    let threads: Vec<_> = (0..THREADS)
        .map(|t| {
            let done = done.clone();
            std::thread::spawn(move || {
                for i in 0..EACH {
                    let accounts = if fastrand::u8(0..4) == 0 { USD } else { EUR };
                    let from = accounts[fastrand::usize(0..accounts.len())];
                    let to = accounts.iter().copied().filter(|a| *a != from).nth(fastrand::usize(0..accounts.len() - 1)).unwrap();
                    let cents = fastrand::i64(1..=30_000);
                    let currency = if accounts == USD { "USD" } else { "EUR" };
                    let body = format!(r#"{{"from": "{}", "to": "{}", "amount": "{}.{:02}", "currency": "{}"}}"#, from, to, cents / 100, cents % 100, currency);
                    let key = format!("t{}-{}-{}", t, i, fastrand::u64(..));
                    // A third are sent twice at once: the second as a retry
                    // after a timeout would be.
                    let twin = (fastrand::u8(0..3) == 0).then(|| {
                        let (key, body) = (key.clone(), body.clone());
                        std::thread::spawn(move || until_final(port, &key, &body))
                    });
                    let (status, answer) = until_final(port, &key, &body);
                    if let Some(twin) = twin {
                        assert_eq!(twin.join().unwrap(), (status, answer.clone()), "both tries of {} get the same answer", key);
                    }
                    let d = Done { key, body, status, answer, from: from.into(), to: to.into(), cents };
                    done.lock().unwrap().push(d);
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    Arc::try_unwrap(done).ok().unwrap().into_inner().unwrap()
}

/// Account balances in cents, as the API gives them.
fn balances(port: u16) -> HashMap<String, i64> {
    let a = send(port, "GET", "/api/accounts", &[], "");
    assert_eq!(a.status, 200, "{}", a.body);
    let rows: serde_json::Value = serde_json::from_str(&a.body).unwrap();
    rows.as_array()
        .unwrap()
        .iter()
        .map(|r| {
            let amount = r["balance"]["amount"].as_str().unwrap();
            let (whole, cents) = amount.split_once('.').unwrap();
            let sign = if whole.starts_with('-') { -1 } else { 1 };
            let n = whole.trim_start_matches('-').parse::<i64>().unwrap() * 100 + cents.parse::<i64>().unwrap();
            (r["id"].as_str().unwrap().to_string(), sign * n)
        })
        .collect()
}

/// Asks the database itself, through a program of its own.
fn ask(target: &str, query: &str) -> String {
    let mut it = Interp::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("examples"));
    it.load_source(&format!("db.open(\"{}\")\nfn q() = db.query(\"{}\")\n", target, query), "check.qwe").unwrap();
    let f = quillo::value::lookup(&it.globals, "q").unwrap();
    match it.call(&f, vec![], 0) {
        Ok(v) => v.repr(),
        Err(_) => panic!("{} failed", query),
    }
}

fn the_books_balance(target: &str) {
    let s = serve(&example(), &[("LEDGER_DB", target)]);
    let port = s.port;
    let before = balances(port);
    assert_eq!(before["alice"], 100_000);
    assert_eq!(before["opening-eur"], -175_000);

    let done = transfers(port);
    assert_eq!(done.len(), THREADS * EACH);
    let made: Vec<&Done> = done.iter().filter(|d| d.status == 201).collect();
    let refused = done.iter().filter(|d| d.status == 422).count();
    assert!(made.len() > 50 && refused > 0, "{} made, {} refused", made.len(), refused);
    let again: Vec<String> = AGAIN.lock().unwrap().iter().filter(|(p, _)| *p == port).map(|(_, a)| a.clone()).collect();
    let conflicts = again.iter().filter(|a| a.starts_with("409")).count();
    eprintln!("{}: {} transfers made, {} refused; {} answers said to try again, {} of them 409", target, made.len(), refused, again.len(), conflicts);

    // Each transfer made is one entry, its own.
    let entries: HashSet<String> = made
        .iter()
        .map(|d| serde_json::from_str::<serde_json::Value>(&d.answer).unwrap()["entry"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(entries.len(), made.len());
    let count = ask(target, "select count(*) as n from entries where created_by = 'demo'");
    assert_eq!(count, format!("[{{ n: {} }}]", made.len()), "no transfer was applied twice, or lost");
    assert_eq!(ask(target, "select count(*) as n from entries where posted = 0"), "[{ n: 0 }]");

    // Every entry adds up to zero, in each currency.
    assert_eq!(ask(target, "select entry_id from postings group by entry_id, currency having sum(amount_minor) <> 0"), "[]");
    assert_eq!(ask(target, "select entry_id from postings group by entry_id having count(*) < 2"), "[]");

    // The balances are the opening ones moved by exactly the transfers made.
    let after = balances(port);
    let mut expected = before.clone();
    for d in &made {
        *expected.get_mut(&d.from).unwrap() -= d.cents;
        *expected.get_mut(&d.to).unwrap() += d.cents;
    }
    assert_eq!(after, expected);
    for (currency, accounts) in [("EUR", EUR), ("USD", USD)] {
        let bank = format!("opening-{}", currency.to_lowercase());
        let total: i64 = accounts.iter().map(|a| after[*a]).sum::<i64>() + after[&bank];
        assert_eq!(total, 0, "{} adds up to zero", currency);
        let customers: i64 = accounts.iter().map(|a| after[*a]).sum();
        assert_eq!(customers, -before[&bank], "{}: the customers' money is all still there", currency);
    }
    for a in EUR.iter().chain(USD) {
        assert!(after[*a] >= 0, "{} went below zero: {}", a, after[*a]);
    }

    // Sent again later, each gets its first answer, and nothing moves.
    for d in done.iter().step_by(7) {
        let again = send(port, "POST", "/api/transfers", &[("Idempotency-Key", &d.key)], &d.body);
        assert_eq!((again.status, &again.body), (d.status, &d.answer), "{}", d.key);
        assert_eq!(again.header("idempotent-replayed"), Some("true"));
    }
    assert_eq!(balances(port), after);
    // And a key with another transfer is refused.
    let d = &done[0];
    let other = d.body.replace("\"amount\": \"", "\"amount\": \"1");
    assert_eq!(send(port, "POST", "/api/transfers", &[("Idempotency-Key", &d.key)], &other).status, 422);

}

#[test]
fn sqlite() {
    let dir = support::temp_dir("ledger-sqlite");
    let db = dir.join("ledger.db");
    the_books_balance(db.to_str().unwrap());
    unbalanced_entries_are_refused_by_the_database(db.to_str().unwrap());
}

#[test]
fn postgres() {
    let Some(pg) = support::postgres("ledger") else {
        eprintln!("skipping: PostgreSQL's initdb and pg_ctl were not found");
        return;
    };
    let url = pg.database("ledger");
    the_books_balance(&url);
    // How many transactions were rolled back (serialization failures, tried again).
    eprintln!("rolled back: {}", ask(&url, "select xact_rollback from pg_stat_database where datname = 'ledger'"));
    unbalanced_entries_are_refused_by_the_database(&url);
}

/// The triggers: an entry whose postings do not add up is not posted, and
/// a posted one does not change.
fn unbalanced_entries_are_refused_by_the_database(target: &str) {
    let mut it = Interp::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("examples"));
    let src = format!(
        r#"db.open("{target}")
fn unbalanced() = db.transaction(() => {{
    db.exec("insert into entries (id, memo, value_date, created_by, created_at) values ('x', 'bad', '2026-10-27', 'test', '2026-10-27T00:00:00Z')")?
    db.exec("insert into postings (entry_id, line, account_id, currency, amount_minor) values ('x', 1, 'alice', 'EUR', -100)")?
    db.exec("insert into postings (entry_id, line, account_id, currency, amount_minor) values ('x', 2, 'bob', 'EUR', 99)")?
    db.exec("update entries set posted = 1 where id = 'x'")?
}})
fn wrong_currency() = db.exec("insert into postings (entry_id, line, account_id, currency, amount_minor) values ('y', 1, 'dan', 'EUR', 1)")
fn rewrite() = db.exec("update postings set amount_minor = amount_minor + 1 where account_id = 'alice'")
fn posted_at_once() = db.exec("insert into entries (id, memo, value_date, created_by, created_at, posted) values ('z', 'bad', '2026-10-27', 'test', '2026-10-27T00:00:00Z', 1)")
"#
    );
    it.load_source(&src, "check.qwe").unwrap();
    let mut call = |name: &str| {
        let f = quillo::value::lookup(&it.globals, name).unwrap();
        it.call(&f, vec![], 0).ok().unwrap().repr()
    };
    assert!(call("unbalanced").contains("add up to zero"));
    assert!(call("wrong_currency").starts_with("err("));
    assert!(call("rewrite").contains("do not change"));
    assert!(call("posted_at_once").contains("posted after its postings"));
}
