//! `db` on a real PostgreSQL server, past the usual path (src/pg.rs, src/db.rs,
//! src/txn.rs): every type there is, placeholders in all the places they can
//! hide, a pool with every connection taken, the connections requests share,
//! servers that go away, conflicts, replicas that fail, transactions over
//! several databases left part way, and TLS's options. Skipped when
//! PostgreSQL's server programs are not installed.

mod support;

use quillo::Interp;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};
use support::Pg;

fn data_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("quillo-pg-{}-{}", name, std::process::id()))
}

/// A throwaway server that can prepare transactions, as `db.atomic` needs.
fn server(name: &str) -> Option<Pg> {
    let Some(pg) = support::postgres(name) else {
        eprintln!("skipping: PostgreSQL's initdb and pg_ctl were not found");
        return None;
    };
    psql(&pg, &pg.url, &["alter system set max_prepared_transactions = 10"]);
    restart(&pg, name);
    Some(pg)
}

fn restart(pg: &Pg, name: &str) {
    let dir = data_dir(name);
    let ok = Command::new(pg.bin.join("pg_ctl")).arg("-D").arg(&dir).arg("-l").arg(dir.join("log")).args(["-w", "restart"]).output().unwrap();
    assert!(ok.status.success(), "{}", String::from_utf8_lossy(&ok.stderr));
}

fn psql(pg: &Pg, url: &str, sql: &[&str]) {
    let mut c = Command::new(pg.bin.join("psql"));
    c.args([url, "-q", "-v", "ON_ERROR_STOP=1"]);
    for s in sql {
        c.args(["-c", s]);
    }
    let out = c.output().unwrap();
    assert!(out.status.success(), "{:?}: {}", sql, String::from_utf8_lossy(&out.stderr));
}

fn psql_value(pg: &Pg, url: &str, sql: &str) -> String {
    let out = Command::new(pg.bin.join("psql")).args([url, "-qtA", "-c", sql]).output().unwrap();
    assert!(out.status.success(), "{}: {}", sql, String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn load(d: &Path, src: &str) -> Interp {
    let mut it = Interp::new(d);
    it.load_source(src, "app.qwe").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    it
}

fn eval(it: &mut Interp, e: &str) -> String {
    it.load_source(&format!("fn __t() {{\n{}\n}}\n", e), "case.qwe").unwrap_or_else(|x| panic!("{}\n{}", x, e));
    match it.call(&quillo::value::lookup(&it.globals, "__t").unwrap(), vec![], 0) {
        Ok(v) => v.repr(),
        Err(quillo::interp::Ctrl::Error(e)) => format!("ERROR: {}", e.msg),
        Err(_) => "returned".into(),
    }
}

fn says(got: String, what: &str) {
    let got = got.replace("\\\"", "\"");
    assert!(got.contains(what), "wanted {:?} in: {}", what, got);
}

/// A program on `url`, in a fresh folder.
fn program(name: &str, url: &str, more: &str) -> Interp {
    let d = support::temp_dir(&format!("dbpg-{}", name));
    load(&d, &format!("db.open(\"{}\")\n{}", url, more))
}

/// The value of column `v` of a query.
fn v(it: &mut Interp, select: &str) -> String {
    eval(it, &format!("db.one(\"select {} as v\").v", select))
}

fn eventually<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(t) = f() {
            return t;
        }
        assert!(Instant::now() < until, "{}", what);
        std::thread::sleep(Duration::from_millis(20));
    }
}

// ---- types ------------------------------------------------------------------------------------

#[test]
fn every_type_comes_back_as_a_value() {
    let Some(pg) = server("types") else { return };
    let mut it = program("types", &pg.url, "");
    assert_eq!(v(&mut it, "1259::oid"), "1259");
    assert_eq!(v(&mut it, "1.5::float4"), "1.5");
    assert_eq!(v(&mut it, "'8f14e45f-ceea-467f-a6ce-6c2f5e4a7a21'::uuid"), "\"8f14e45f-ceea-467f-a6ce-6c2f5e4a7a21\"");
    // Dates at the ends of time.
    assert_eq!(v(&mut it, "'infinity'::date"), "\"infinity\"");
    assert_eq!(v(&mut it, "'-infinity'::date"), "\"-infinity\"");
    assert_eq!(v(&mut it, "'infinity'::timestamp"), "\"infinity\"");
    assert_eq!(v(&mut it, "'-infinity'::timestamptz"), "\"-infinity\"");
    assert_eq!(v(&mut it, "'2024-01-02 03:04:05.5'::timestamp"), "\"2024-01-02T03:04:05.5\"");
    assert_eq!(v(&mut it, "'01:02:03.25'::time"), "\"01:02:03.25\"");
    // Intervals, in the words Postgres uses for them.
    assert_eq!(v(&mut it, "'1 mon 2 days 03:04:05'::interval"), "\"1 mon 2 days 03:04:05\"");
    assert_eq!(v(&mut it, "'14 mons -1 day'::interval"), "\"14 mons -1 day\"");
    assert_eq!(v(&mut it, "'1 day'::interval"), "\"1 day\"");
    assert_eq!(v(&mut it, "'-00:00:01.5'::interval"), "\"-00:00:01.5\"");
    assert_eq!(v(&mut it, "'0'::interval"), "\"00:00:00\"");
    assert_eq!(eval(&mut it, "db.one(\"select decode('dead', 'hex') as v\").v == \"\\\\xdead\""), "true");
    // Numbers past what a float or an int holds.
    assert_eq!(v(&mut it, "'NaN'::numeric"), "NaN");
    assert_eq!(v(&mut it, "'Infinity'::numeric"), "inf");
    assert_eq!(v(&mut it, "'-Infinity'::numeric"), "-inf");
    assert_eq!(v(&mut it, "12345678901234567890::numeric"), "12345678901234567890m");
    assert_eq!(v(&mut it, "1234567890123456789012345678901234567890::numeric"), "\"1234567890123456789012345678901234567890\"");
    // Arrays, of more dimensions, with holes, and of a domain.
    psql(&pg, &pg.url, &["create domain pos as int check (value > 0)"]);
    assert_eq!(v(&mut it, "'{1,NULL,3}'::int[]"), "[1, null, 3]");
    assert_eq!(v(&mut it, "'{{1,2},{3,4}}'::int[]"), "[[1, 2], [3, 4]]");
    assert_eq!(v(&mut it, "'{}'::text[]"), "[]");
    assert_eq!(v(&mut it, "array[5::pos]"), "[5]");
}

/// A type Quillo has no reader for comes back as a string, not a failure.
#[test]
fn a_type_without_a_reader_is_still_a_string() {
    let Some(pg) = server("macaddr") else { return };
    let mut it = program("macaddr", &pg.url, "");
    assert_eq!(eval(&mut it, "type_of(db.one(\"select 'ff:ff:ff:ff:ff:ff'::macaddr as v\").v)"), "\"str\"");
}

#[test]
fn an_address_comes_back_as_its_text() {
    let Some(pg) = server("inet") else { return };
    let mut it = program("inet", &pg.url, "");
    assert_eq!(v(&mut it, "'192.168.0.1'::inet"), "\"192.168.0.1\"");
    assert_eq!(v(&mut it, "'192.168.0.1/24'::inet"), "\"192.168.0.1/24\"");
    assert_eq!(v(&mut it, "'10.0.0.0/8'::cidr"), "\"10.0.0.0/8\"");
    assert_eq!(v(&mut it, "'2001:db8::1'::inet"), "\"2001:db8::1\"");
    assert_eq!(v(&mut it, "'08:00:2b:01:02:03'::macaddr"), "\"08:00:2b:01:02:03\"");
    assert_eq!(v(&mut it, "'08:00:2b:01:02:03:04:05'::macaddr8"), "\"08:00:2b:01:02:03:04:05\"");
    assert_eq!(v(&mut it, "'12.50'::money"), "12.50m");
    assert_eq!(v(&mut it, "'-0.05'::money"), "-0.05m");
    assert_eq!(v(&mut it, "B'10110'::bit(5)"), "\"10110\"");
    assert_eq!(v(&mut it, "B'1'::varbit"), "\"1\"");
    // A type with no reader is bytes, shown as `bytea` is: here two floats.
    assert!(v(&mut it, "point(1, 2)").starts_with("\"\\\\x"), "{}", v(&mut it, "point(1, 2)"));
    // And a type whose bytes are text stays text.
    assert_eq!(v(&mut it, "'a b'::tsvector::text"), "\"'a' 'b'\"");
}

const COLUMNS: &str = "db.exec(\"create table t (i2 smallint, i4 int, i8 bigint, f4 real, f8 double precision, b boolean, j jsonb, ts timestamp)\")\n";

/// What `insert` sends into column `col`, read back; or the error.
fn stored(it: &mut Interp, col: &str, value: &str) -> String {
    eval(
        it,
        &format!(
            "let r = db.one(\"insert into t ({c}) values (?) returning {c} as v\", {v})\nif is_err(r) {{ return r }}\nr.v",
            c = col,
            v = value
        ),
    )
}

#[test]
fn values_are_sent_as_the_column_wants_them() {
    let Some(pg) = server("params") else { return };
    let mut it = program("params", &pg.url, COLUMNS);
    // Numbers that are whole go into ints, whatever they are held in.
    assert_eq!(stored(&mut it, "i4", "2.0"), "2");
    assert_eq!(stored(&mut it, "i4", "3.00m"), "3");
    assert_eq!(stored(&mut it, "i8", "\" 7 \""), "7");
    assert_eq!(stored(&mut it, "i2", "null"), "null");
    says(stored(&mut it, "i4", "2.5"), "cannot send float 2.5 as int4");
    says(stored(&mut it, "i4", "2.5m"), "cannot send");
    says(stored(&mut it, "i4", "\"x\""), "cannot send str \"x\" as int4");
    says(stored(&mut it, "i4", "true"), "cannot send bool true as int4");
    says(stored(&mut it, "i2", "70000"), "cannot send int 70000 as int2");
    says(stored(&mut it, "i4", "3000000000"), "cannot send int 3000000000 as int4");
    // Floats take any number, and numbers in strings.
    assert_eq!(stored(&mut it, "f8", "2"), "2.0");
    assert_eq!(stored(&mut it, "f8", "1.25m"), "1.25");
    assert_eq!(stored(&mut it, "f8", "\" 0.5 \""), "0.5");
    assert_eq!(stored(&mut it, "f4", "1.5"), "1.5");
    says(stored(&mut it, "f8", "\"x\""), "cannot send str \"x\" as float8");
    says(stored(&mut it, "f8", "true"), "cannot send bool true as float8");
    // A boolean is a boolean.
    assert_eq!(stored(&mut it, "b", "true"), "true");
    says(stored(&mut it, "b", "1"), "cannot send int 1 as bool");
    // JSON takes anything; other types get the text, parsed by Postgres.
    assert_eq!(stored(&mut it, "j", "{ a: [1, null] }"), "{ a: [1, null] }");
    assert_eq!(stored(&mut it, "ts", "\"2024-01-02 03:04:05\""), "\"2024-01-02T03:04:05\"");
}

#[test]
fn a_float_too_large_for_a_bigint_is_refused() {
    let Some(pg) = server("toolarge") else { return };
    let mut it = program("toolarge", &pg.url, COLUMNS);
    says(stored(&mut it, "i8", "1e30"), "cannot send float");
}

#[test]
fn lists_and_maps_go_as_array_text() {
    let Some(pg) = server("arrays") else { return };
    let mut it = program("arrays", &pg.url, "");
    let cast = |it: &mut Interp, ty: &str, value: &str| eval(it, &format!("db.one(\"select ?::{} as v\", {}).v", ty, value));
    assert_eq!(cast(&mut it, "int[]", "[1, null, 3]"), "[1, null, 3]");
    assert_eq!(cast(&mut it, "int[]", "[[1, 2], [3, 4]]"), "[[1, 2], [3, 4]]");
    assert_eq!(cast(&mut it, "text[]", "[\"a\\\"b\", \"c\\\\d\"]"), "[\"a\\\"b\", \"c\\\\d\"]");
    assert_eq!(cast(&mut it, "text[]", "[{ a: 1 }]"), "[\"{\\\"a\\\":1}\"]");
    assert_eq!(cast(&mut it, "numeric", "1.50m"), "1.50m");
    // A type of another schema is named with its schema.
    psql(&pg, &pg.url, &["create schema \"Other\"", "create domain \"Other\".\"Pos\" as int check (value > 0)", "create table cast_me (id int, p \"Other\".\"Pos\")"]);
    assert_eq!(eval(&mut it, "db.one(\"insert into cast_me (id, p) values (1, ?) returning p as v\", 5).v"), "5");
    assert!(eval(&mut it, "db.one(\"insert into cast_me (id, p) values (1, ?) returning p as v\", -5)").contains("violates check constraint"));
}

// ---- placeholders -----------------------------------------------------------------------------

#[test]
fn placeholders_are_found_only_where_they_are_placeholders() {
    let Some(pg) = server("holders") else { return };
    let mut it = program("holders", &pg.url, "");
    let two = |it: &mut Interp, sql: &str| eval(it, &format!("let r = db.one(\"{}\", 1)\nis_err(r) ? r : r.a", sql));
    // In quotes of every kind, comments and dollar quotes, `?` is text.
    assert_eq!(two(&mut it, "select 'it''s ?' as q, ?::int as a"), "1");
    assert_eq!(two(&mut it, "select E'it\\\\'s ?' as q, ?::int as a"), "1");
    assert_eq!(two(&mut it, "select e'a\\\\\\\\' as q, ?::int as a"), "1");
    assert_eq!(two(&mut it, "select 1 as \\\"a?\\\", ?::int as a"), "1");
    assert_eq!(two(&mut it, "select 1 as \\\"a\\\"\\\"?\\\", ?::int as a"), "1");
    assert_eq!(two(&mut it, "select -- what ?\\n ?::int as a"), "1");
    assert_eq!(two(&mut it, "select /* what ? */ ?::int as a"), "1");
    assert_eq!(two(&mut it, "select $$ ? $$ as q, ?::int as a"), "1");
    assert_eq!(two(&mut it, "select $t$ ? $$ ? $t$ as q, ?::int as a"), "1");
    // Where a string never closes, Postgres says so.
    says(two(&mut it, "select ?::int as a, 'open"), "unterminated quoted string");
    says(two(&mut it, "select ?::int as a, $$ open"), "unterminated dollar-quoted string");
    // A comment left open ends with the statement.
    assert_eq!(two(&mut it, "select ?::int as a /* open"), "1");
    // A `$` that opens nothing is left to Postgres.
    says(two(&mut it, "select ?::int as a, $x"), "syntax error");
    // A statement cannot begin with one.
    says(two(&mut it, "? nonsense"), "syntax error");
}

#[test]
fn named_placeholders_take_their_values_from_a_map() {
    let Some(pg) = server("named") else { return };
    let mut it = program("named", &pg.url, "");
    assert_eq!(eval(&mut it, "db.one(\"select :a::int + :a::int + :b::int as v\", { a: 1, b: 2 }).v"), "4");
    assert_eq!(eval(&mut it, "db.one(\"select :a::int as v\", { \":a\": 3 }).v"), "3");
    says(eval(&mut it, "db.one(\"select :a::int + :b::int as v\", { a: 1 })"), "no value for :b");
    // A map where a `?` is asked for is one value.
    assert_eq!(eval(&mut it, "db.one(\"select ?::jsonb as v\", { a: 1 }).v"), "{ a: 1 }");
    // Named ones in quotes are text.
    assert_eq!(eval(&mut it, "db.one(\"select ':a' as t, :b::int as v\", { b: 2 }).t"), "\":a\"");
}

#[test]
fn jsonb_s_question_marks_are_operators() {
    let Some(pg) = server("question") else { return };
    let mut it = program("question", &pg.url, "db.exec(\"create table j (d jsonb)\")\ndb.exec(\"insert into j values ('{\\\"a\\\": 1, \\\"b\\\": 2}')\")\n");
    let n = |it: &mut Interp, cond: &str, args: &str| eval(it, &format!("len(db.query(\"select 1 from j where {}\"{}))", cond, args));
    assert_eq!(n(&mut it, "d ? 'a'", ""), "1");
    assert_eq!(n(&mut it, "d ? 'z'", ""), "0");
    assert_eq!(n(&mut it, "(d) ? 'a'", ""), "1");
    assert_eq!(n(&mut it, "d ?| array['a', 'z']", ""), "1");
    assert_eq!(n(&mut it, "d ?& array['a', 'z']", ""), "0");
    assert_eq!(n(&mut it, "d ? 'a' and d->>'b' = ?", ", \"2\""), "1");
    assert_eq!(n(&mut it, "d->'a' is not null and 1 = ?", ", 1"), "1");
    // With numbered parameters it is never rewritten.
    assert_eq!(n(&mut it, "d ? $1", ", \"a\""), "1");
}

#[test]
fn statements_are_kept_and_forgotten_as_they_pile_up() {
    let Some(pg) = server("many") else { return };
    let mut it = program("many", &pg.url, "");
    // Past what a connection keeps prepared, and what the translation keeps.
    let r = eval(&mut it, "for n in range(0, 1100) {\n    let got = db.one(`select ?::int + ${n} as v`, 1).v\n    if got != n + 1 { return err(got) }\n}\n\"ok\"");
    assert_eq!(r, "\"ok\"");
}

// ---- the pool ---------------------------------------------------------------------------------

#[test]
fn a_connection_that_died_is_replaced() {
    let Some(pg) = server("died") else { return };
    let pool = quillo::pg::Pool::open(&format!("{}?pool_size=1", pg.url)).unwrap();
    let mut c = pool.checkout().unwrap();
    let pid = c.query("select pg_backend_pid() as p", &[], None).unwrap()[0].repr();
    let pid: String = pid.chars().filter(|c| c.is_ascii_digit()).collect();
    psql(&pg, &pg.url, &[&format!("select pg_terminate_backend({})", pid)]);
    assert!(c.query("select 1", &[], None).is_err());
    // The client hears of the close a moment after the query fails (FEEDBACK16.md, Q-160).
    let deadline = Instant::now() + Duration::from_secs(5);
    while !matches!(c.query("select 1", &[], None), Err(e) if e.contains("connection closed")) {
        assert!(Instant::now() < deadline, "the client never saw the close");
        std::thread::sleep(Duration::from_millis(10));
    }
    pool.give_back(c);
    // Not kept: the next is a new one, though the pool holds one at most.
    let mut c = pool.checkout().unwrap();
    assert_eq!(c.query("select 1 as x", &[], None).unwrap()[0].repr(), "{ x: 1 }");
    pool.give_back(c);
}

#[test]
#[ignore = "finding: a connection given back right after the server ended it is kept, so the next caller gets a reset (seen on Linux; `is_closed` is set a moment later)"]
fn a_connection_given_back_as_it_dies_is_not_handed_out_again() {
    let Some(pg) = server("died-race") else { return };
    let pool = quillo::pg::Pool::open(&format!("{}?pool_size=1", pg.url)).unwrap();
    let mut c = pool.checkout().unwrap();
    let pid = c.query("select pg_backend_pid() as p", &[], None).unwrap()[0].repr();
    let pid: String = pid.chars().filter(|c| c.is_ascii_digit()).collect();
    psql(&pg, &pg.url, &[&format!("select pg_terminate_backend({})", pid)]);
    assert!(c.query("select 1", &[], None).is_err());
    pool.give_back(c);
    let mut c = pool.checkout().unwrap();
    assert_eq!(c.query("select 1 as x", &[], None).unwrap()[0].repr(), "{ x: 1 }");
    pool.give_back(c);
}

#[test]
fn a_pool_with_every_connection_taken() {
    let Some(pg) = server("taken") else { return };
    let pool = Arc::new(quillo::pg::Pool::open(&format!("{}?pool_size=1", pg.url)).unwrap());
    let held = pool.checkout().unwrap();
    assert!(pool.try_checkout().unwrap().is_none());
    assert!(pool.none_free().contains("all 1 are in use (`?pool_size=N` in the URL allows more)"));
    // Quillo's own work never waits for one.
    let mut aside = pool.checkout_nested().unwrap();
    assert_eq!(aside.query("select 1 as x", &[], None).unwrap()[0].repr(), "{ x: 1 }");
    pool.give_back(aside);
    assert!(pool.try_checkout().unwrap().is_none());
    // One given back goes to whoever waits.
    let p = pool.clone();
    let waiter = std::thread::spawn(move || p.checkout().map(|c| p.give_back(c)));
    std::thread::sleep(Duration::from_millis(100));
    pool.give_back(held);
    assert!(waiter.join().unwrap().is_ok());
}

#[test]
fn transactions_wait_their_turn_outside_the_server_too() {
    let Some(pg) = server("turns") else { return };
    let mut it = program("turns", &format!("{}?pool_size=1", pg.url), "db.exec(\"create table t (n int)\")\n");
    let r = eval(
        &mut it,
        "let one = (n) => () => db.transaction(() => {\n    db.one(\"select pg_sleep(0.1)\")\n    db.exec(\"insert into t values (?)\", n).changes\n})\nparallel(one(1), one(2), one(3))",
    );
    assert_eq!(r, "[1, 1, 1]");
    assert_eq!(eval(&mut it, "len(db.query(\"select * from t\"))"), "3");
}

/// The full wait: thirty seconds.
#[test]
fn no_connection_after_thirty_seconds_is_an_error() {
    let Some(pg) = server("nofree") else { return };
    let url = format!("{}?pool_size=1", pg.url);
    let pool = Arc::new(quillo::pg::Pool::open(&url).unwrap());
    let held = pool.checkout().unwrap();
    let p = pool.clone();
    let waiter = std::thread::spawn(move || p.checkout().map(|_| ()).unwrap_err());
    // A transaction waits as long, and says the same.
    let mut it = program("nofree", &url, "");
    let r = eval(
        &mut it,
        "parallel(() => db.transaction(() => db.one(\"select pg_sleep(31.5) as s\")), () => {\n    sleep(1000)\n    db.transaction(() => 1)\n})",
    );
    says(r, "no PostgreSQL connection free after 30 s");
    says(waiter.join().unwrap(), "no PostgreSQL connection free after 30 s");
    pool.give_back(held);
}

// ---- servers that go away ----------------------------------------------------------------------

#[test]
fn a_server_that_stops_gives_errors_not_a_crash() {
    let Some(pg) = server("stops") else { return };
    let other = pg.database("other");
    let down = support::postgres("stopsdown").unwrap();
    let gone = down.database("gone");
    let src = format!("let far = db.connect(\"{}?pool_size=1\")\nlet near = db.connect(\"{}\")\nfar.query(\"select 1\")\nnear.exec(\"create table t (x int)\")\n", gone, other);
    let d = support::temp_dir("dbpg-stops");
    let mut it = load(&d, &format!("db.open(\"main.db\")\n{}", src));
    drop(down);
    // The connection kept is dead: `begin` fails on it, then there is none to be had.
    eventually("a new connection was not tried", || {
        let r = eval(&mut it, "far.transaction(() => 1)");
        assert!(r.starts_with("err(\"db:"), "{}", r);
        r.contains("Connection refused").then_some(())
    });
    // A part that cannot begin: nothing is kept in the others, and they let go.
    says(eval(&mut it, "db.atomic([near, far], () => near.exec(\"insert into t values (1)\"))"), "Connection refused");
    assert_eq!(eval(&mut it, "len(near.query(\"select * from t\"))"), "0");
    says(eval(&mut it, "db.atomic([far, near], () => 1)"), "Connection refused");
    assert_eq!(eval(&mut it, "db.atomic([near], () => 2)"), "2");
    // Nor can a tenant be listed.
    let mut it = load(&d, &format!("db.open(\"{}?schema=t_{{tenant}}\")\n", gone));
    says(eval(&mut it, "db.tenants()"), "Connection refused");
    assert_eq!(eval(&mut it, "db.tenant_exists(\"acme\")"), "false");
}

// ---- transactions -----------------------------------------------------------------------------

#[test]
fn after_a_conflict_the_transaction_is_over() {
    let Some(pg) = server("conflict") else { return };
    let src = format!(
        "let other = db.connect(\"{}?pool_size=2\")\ndb.exec(\"create table t (n int)\")\ndb.exec(\"insert into t values (1)\")\n",
        pg.url
    );
    let mut it = program("conflict", &pg.url, &src);
    let body = r#"
db.one("select n from t")
other.exec("update t set n = n + 1")
let a = db.exec("update t set n = 100")
let b = db.query("select 1")
let c = db.exec("select 1")
err([a, b, c] | map((e) => str(e.error)))
"#;
    let r = eval(&mut it, &format!("db.transaction({{ isolation: \"repeatable_read\" }}, () => {{{}}})", body));
    says(r.clone(), "could not serialize access");
    assert_eq!(r.matches("the transaction conflicted with another and was rolled back").count(), 2, "{}", r);
    assert_eq!(eval(&mut it, "db.error_kind(err(\"db: could not serialize access due to concurrent update\"))"), "\"conflict\"");
    assert_eq!(eval(&mut it, "db.one(\"select n from t\").n"), "2");
}

#[test]
fn every_isolation_level_is_asked_of_postgres() {
    let Some(pg) = server("isolation") else { return };
    let mut it = program("isolation", &pg.url, "");
    let level = |it: &mut Interp, l: &str| eval(it, &format!("db.transaction({{ isolation: \"{}\" }}, () => db.one(\"show transaction_isolation\").transaction_isolation)", l));
    assert_eq!(level(&mut it, "read_committed"), "\"read committed\"");
    assert_eq!(level(&mut it, "repeatable_read"), "\"repeatable read\"");
    assert_eq!(level(&mut it, "serializable"), "\"serializable\"");
}

#[test]
fn a_commit_that_fails_is_an_error_value() {
    let Some(pg) = server("commit") else { return };
    let src = "db.exec(\"create table p (id int primary key)\")\ndb.exec(\"create table k (p int references p (id) deferrable initially deferred)\")\n";
    let mut it = program("commit", &pg.url, src);
    let r = eval(&mut it, "db.transaction(() => db.exec(\"insert into k values (9)\").changes)");
    says(r.clone(), "violates foreign key constraint");
    assert_eq!(eval(&mut it, "db.error_kind(db.transaction(() => db.exec(\"insert into k values (9)\")))"), "\"foreign_key\"");
    assert_eq!(eval(&mut it, "len(db.query(\"select * from k\"))"), "0");
}

#[test]
fn errors_postgres_gives_have_kinds() {
    let Some(pg) = server("kinds") else { return };
    let src = "db.exec(\"create table p (id int primary key, n int not null, c int check (c > 0))\")\ndb.exec(\"insert into p values (1, 1, 1)\")\n";
    let mut it = program("kinds", &pg.url, src);
    let kind = |it: &mut Interp, sql: &str| eval(it, &format!("db.error_kind(db.exec(\"{}\"))", sql));
    assert_eq!(kind(&mut it, "insert into p values (1, 1, 1)"), "\"unique\"");
    assert_eq!(kind(&mut it, "insert into p values (2, null, 1)"), "\"not_null\"");
    assert_eq!(kind(&mut it, "insert into p values (2, 1, 0)"), "\"check\"");
}

// ---- replicas ---------------------------------------------------------------------------------

#[test]
fn replicas_are_left_out_when_they_fail_or_in_a_transaction() {
    let Some(pg) = server("replicas") else { return };
    let (primary, replica) = (pg.database("rprimary"), pg.database("rreplica"));
    psql(&pg, &primary, &["create table who (name text)", "insert into who values ('primary')", "create table only_here (x int)"]);
    psql(&pg, &replica, &["create table who (name text)", "insert into who values ('replica')"]);
    let d = support::temp_dir("dbpg-replicas");
    let mut it = load(&d, &format!("db.open(\"{}\", {{ replicas: [\"{}\"] }})\n", primary, replica));
    let name = "db.one(\"select name from who\").name";
    assert_eq!(eval(&mut it, name), "\"replica\"");
    // In a transaction, reads stay with it.
    assert_eq!(eval(&mut it, &format!("db.transaction(() => {})", name)), "\"primary\"");
    // A replica that cannot answer: the primary does, and it is left out.
    assert_eq!(eval(&mut it, "len(db.query(\"select * from only_here\"))"), "0");
    assert_eq!(eval(&mut it, name), "\"primary\"");
    // One that cannot be reached is said at once.
    let nobody = format!("postgres://quillo@127.0.0.1:{}/x", support::free_port());
    says(eval(&mut it, &format!("db.open(\"{}\", {{ replicas: [\"{}\"] }})", primary, nobody)), &format!("replica {}", nobody));
}

// ---- tenants ----------------------------------------------------------------------------------

#[test]
fn schemas_a_url_may_not_name() {
    let Some(pg) = server("schemas") else { return };
    let mut it = program("schemas", &pg.url, "");
    says(eval(&mut it, &format!("db.open(\"{}?schema=Bad!\")", pg.url)), "bad schema: \"Bad!\"");
    says(eval(&mut it, &format!("db.open(\"{}?schema=public\")", pg.url)), "the schema \"public\" is Postgres's own");
    says(eval(&mut it, &format!("db.open(\"{}?schema=pg_x\")", pg.url)), "the schema \"pg_x\" is Postgres's own");
    says(eval(&mut it, &format!("db.open(\"{}?schema=information_schema\")", pg.url)), "is Postgres's own");
    says(eval(&mut it, &format!("db.open(\"{}?pool_size=none\")", pg.url)), "bad pool_size: none");
    says(eval(&mut it, &format!("db.open(\"{}?pool_size=0\")", pg.url)), "bad pool_size: 0");
    says(eval(&mut it, &format!("db.open(\"{}?sslmode=maybe\")", pg.url)), "bad sslmode: \"maybe\"");
}

#[test]
fn only_schemas_can_be_listed_as_tenants() {
    let Some(pg) = server("listing") else { return };
    let mut it = program("listing", &format!("{}?schema=t_{{tenant}}&pool_size=2", pg.url), "");
    eval(&mut it, "with_tenant(\"acme\", () => db.exec(\"create table a (x int)\"))");
    assert_eq!(eval(&mut it, "db.tenants()"), "[\"acme\"]");
    assert_eq!(eval(&mut it, "[db.tenant_exists(\"acme\"), db.tenant_exists(\"ghost\")]"), "[true, false]");
    // Asked after it is closed too: the schema is what counts.
    let mut it = program("listing2", &format!("{}?pool_size=2&schema=t_{{tenant}}_x", pg.url), "");
    eval(&mut it, "with_tenant(\"zeta\", () => db.exec(\"create table a (x int)\"))");
    let mut it = program("listing2", &format!("{}?pool_size=2&schema=t_{{tenant}}_x", pg.url), "");
    assert_eq!(eval(&mut it, "db.tenants()"), "[\"zeta\"]");
    assert_eq!(eval(&mut it, "[db.tenant_exists(\"zeta\"), db.tenant_exists(\"acme\")]"), "[true, false]");
    // A database per tenant cannot be listed: there is no schema to go by.
    let mut it = program("listing3", &pg.url.replace("/postgres", "/app_{tenant}"), "");
    says(eval(&mut it, "db.tenants()"), "`db.tenants` lists PostgreSQL schemas (?schema=t_{tenant}), not databases");
    // `schema=` that is not an option is the database's name.
    let mut it = program("listing4", &pg.url.replace("/postgres", "/schema={tenant}"), "");
    says(eval(&mut it, "db.tenants()"), "does not exist");
}

#[test]
fn the_least_recently_used_tenants_are_closed() {
    let Some(pg) = server("evict") else { return };
    let mut it = program("evict", &format!("{}?schema=t_{{tenant}}&pool_size=1", pg.url), "");
    let put = |it: &mut Interp, t: &str| eval(it, &format!("with_tenant(\"{}\", () => {{\n    db.exec(\"create table if not exists a (x text)\")\n    db.exec(\"insert into a values (?)\", \"{}\").changes\n}})", t, t));
    for i in 0..18 {
        assert_eq!(put(&mut it, &format!("t{}", i)), "1");
    }
    // Their schemas stay, and opening one again finds its data.
    assert_eq!(put(&mut it, "t0"), "1");
    assert_eq!(eval(&mut it, "with_tenant(\"t0\", () => len(db.query(\"select * from a\")))"), "2");
    let open = || psql_value(&pg, &pg.url, "select count(*) from pg_stat_activity where datname = 'postgres' and pid <> pg_backend_pid()");
    eventually("the pools of tenants closed kept their connections", || (open().parse::<i64>().unwrap() <= 16).then_some(()));
}

// ---- requests sharing connections -------------------------------------------------------------

const APP: &str = r#"
db.exec("create table if not exists t (x int)")
route GET /sleep = db.one("select pg_sleep(0.5) as s, 'slow' as v").v
route GET /quick = db.one("select 'quick' as v").v
route GET /bad = str(db.one("select 1", 2).error)
route GET /badexec = str(db.exec("select 1", 2).error)
route GET /cast = str(db.one("select ?::numeric(10,2) as v", "1.5").v)
route GET /n/:i = str(db.one("select " + i + "::int as v").v)
route GET /tx = db.transaction(() => db.one("select pg_sleep(0.1) as s, 'done' as v").v)
route GET /two = str(db.exec("insert into t values (1); insert into t values (2)").changes)
route GET /count = str(db.one("select count(*) as n from t").n)
"#;

fn app(name: &str, url: &str) -> (PathBuf, support::Server) {
    let d = support::temp_dir(&format!("dbpg-{}", name));
    let file = d.join("app.qwe");
    std::fs::write(&file, format!("db.open(\"{}\")\n{}", url, APP)).unwrap();
    let s = support::serve(&file, &[]);
    (d, s)
}

fn get(port: u16, path: &str) -> String {
    support::send(port, "GET", path, &[], "").body
}

#[test]
fn requests_share_connections_that_can_get_stuck_and_die() {
    let Some(pg) = server("lanes") else { return };
    let (_d, s) = app("lanes", &format!("{}?pool_size=1", pg.url));
    let port = s.port;
    // Sent while the only connection is stuck behind a slow query: it waits there.
    let slow = std::thread::spawn(move || get(port, "/sleep"));
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(get(port, "/quick"), "quick");
    assert_eq!(slow.join().unwrap(), "slow");
    // What goes wrong is said as it is outside a request.
    assert_eq!(get(port, "/bad"), "db: the query has 0 parameters, got 1");
    assert_eq!(get(port, "/badexec"), "db: the query has 0 parameters, got 1");
    assert_eq!(get(port, "/cast"), "1.50");
    assert_eq!(get(port, "/two"), "null");
    assert_eq!(get(port, "/count"), "2");
    // The database ended it: the next request gets a new one.
    psql(&pg, &pg.url, &["select pg_terminate_backend(pid) from pg_stat_activity where datname = 'postgres' and pid <> pg_backend_pid()"]);
    eventually("no new connection", || (get(port, "/quick") == "quick").then_some(()));
}

#[test]
fn stuck_connections_are_passed_by() {
    let Some(pg) = server("lanes2") else { return };
    let (_d, s) = app("lanes2", &pg.url);
    let port = s.port;
    let slow: Vec<_> = (0..12).map(|_| std::thread::spawn(move || get(port, "/sleep"))).collect();
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(get(port, "/quick"), "quick");
    for t in slow {
        assert_eq!(t.join().unwrap(), "slow");
    }
}

#[test]
fn a_shared_connection_forgets_statements_as_they_pile_up() {
    let Some(pg) = server("lanes3") else { return };
    let (_d, s) = app("lanes3", &format!("{}?pool_size=1", pg.url));
    for n in 0..270 {
        assert_eq!(get(s.port, &format!("/n/{}", n)), n.to_string());
    }
}

#[test]
fn transactions_in_requests_wait_for_a_connection_without_a_thread() {
    let Some(pg) = server("waiting") else { return };
    let (_d, s) = app("waiting", &format!("{}?pool_size=1", pg.url));
    let port = s.port;
    let all: Vec<_> = (0..4).map(|_| std::thread::spawn(move || get(port, "/tx"))).collect();
    for t in all {
        assert_eq!(t.join().unwrap(), "done");
    }
}

// ---- one transaction over several databases, left part way ------------------------------------

#[test]
fn a_part_that_cannot_commit_is_left_to_recovery() {
    let Some(pg) = server("leftover") else { return };
    let (part, sys) = (pg.database("lpart"), pg.database("lsys"));
    psql(&pg, &part, &["create table t (x int)"]);
    psql(
        &pg,
        &sys,
        &[
            "create table _quillo_atomic (gid text primary key, decision text not null, at bigint not null)",
            "create function cut() returns trigger language plpgsql as $$ begin perform pg_terminate_backend(pid) from pg_stat_activity where datname = 'lpart'; return new; end $$",
            "create trigger cut after insert on _quillo_atomic for each row execute function cut()",
        ],
    );
    let d = support::temp_dir("dbpg-leftover");
    let mut it = load(&d, &format!("db.open(\"main.db\")\ndb.system(\"{}\")\nlet part = db.connect(\"{}\")\n", sys, part));
    // The decision to commit stands, though the part has not heard of it.
    assert_eq!(eval(&mut it, "db.atomic([part], () => part.exec(\"insert into t values (1)\").changes)"), "1");
    assert_eq!(psql_value(&pg, &sys, "select decision from _quillo_atomic"), "commit");
    assert_eq!(psql_value(&pg, &part, "select count(*) from pg_prepared_xacts"), "1");
    assert_eq!(psql_value(&pg, &part, "select count(*) from t"), "0");
    // Too young for recovery: it may still be finishing.
    quillo::db::recover(&mut it);
    assert_eq!(psql_value(&pg, &part, "select count(*) from pg_prepared_xacts"), "1");
}

#[test]
fn recovery_finishes_what_was_left_as_it_was_decided() {
    let Some(pg) = server("recover") else { return };
    let part = pg.database("rcpart");
    psql(&pg, &part, &["create table t (x text)"]);
    let prepare = |gid: &str| psql(&pg, &part, &["begin", &format!("insert into t values ('{}')", gid), &format!("prepare transaction '{}'", gid)]);
    let d = support::temp_dir("dbpg-recover");
    let src = format!(
        "db.open(\"main.db\")\nlet part = db.connect(\"{}\")\nlet each = db.connect(\"t/{{tenant}}.db\")\n\
         db.exec(\"create table _quillo_atomic (gid text primary key, decision text not null, at bigint not null)\")\n",
        part
    );
    let mut it = load(&d, &src);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis();
    prepare("quillo_1000_kept_0");
    eval(&mut it, "db.exec(\"insert into _quillo_atomic values ('quillo_1000_kept', 'commit', 1)\")");
    prepare("quillo_1000_lost_0");
    prepare(&format!("quillo_{}_young_0", now));
    prepare("quillo_1000_stuck_0");
    eval(&mut it, "db.exec(\"create trigger stuck before insert on _quillo_atomic when new.gid = 'quillo_1000_stuck' begin select raise(abort, 'no'); end\")");
    // Not of `db.atomic`'s making.
    prepare("quillo_someone_else");
    prepare("quillo_when_who_0");
    prepare("other_1000_x_0");
    quillo::db::recover(&mut it);
    let rows = |sql: &str| psql_value(&pg, &part, sql).lines().map(str::to_string).collect::<Vec<_>>();
    // What was decided is committed; what was not is rolled back, and that recorded.
    assert_eq!(rows("select x from t"), ["quillo_1000_kept_0"]);
    assert_eq!(eval(&mut it, "db.one(\"select decision from _quillo_atomic where gid = 'quillo_1000_lost'\").decision"), "\"abort\"");
    let young = format!("quillo_{}_young_0", now);
    let left = rows("select gid from pg_prepared_xacts order by 1");
    assert_eq!(left, ["other_1000_x_0", "quillo_1000_stuck_0", young.as_str(), "quillo_someone_else", "quillo_when_who_0"]);
    // Decisions a day old go.
    assert_eq!(eval(&mut it, "len(db.query(\"select * from _quillo_atomic where gid = 'quillo_1000_kept'\"))"), "0");
    assert_eq!(eval(&mut it, "len(db.query(\"select * from _quillo_atomic where gid = 'quillo_1000_lost'\"))"), "1");
}

// ---- TLS's options ----------------------------------------------------------------------------

#[test]
fn a_unix_socket_is_on_this_machine() {
    let Some(pg) = support::postgres("socket") else { return };
    let port = pg.url.rsplit(':').next().unwrap().split('/').next().unwrap();
    let dir = data_dir("socket").display().to_string().replace('/', "%2F");
    let mut it = program("socket", &format!("postgres://quillo@{}:{}/postgres", dir, port), "");
    assert_eq!(v(&mut it, "1"), "1");
}

#[test]
fn tls_options_that_cannot_work_are_said_before_connecting() {
    let d = support::temp_dir("dbpg-tlsopts");
    let empty = d.join("empty.pem");
    std::fs::write(&empty, "").unwrap();
    let not_a_cert = d.join("bad.pem");
    std::fs::write(&not_a_cert, "-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n").unwrap();
    let mut it = load(&d, "");
    let open = |it: &mut Interp, q: &str| eval(it, &format!("db.open(\"postgres://u@127.0.0.1:1/x?{}\")", q));
    says(open(&mut it, "sslcert=/c.pem"), "sslcert and sslkey go together");
    says(open(&mut it, "sslkey=/k.pem"), "sslcert and sslkey go together");
    says(open(&mut it, "sslcert=/no/such.pem&sslkey=/no/such.key"), "sslcert /no/such.pem");
    says(open(&mut it, "sslrootcert=system&sslmode=require"), "sslrootcert=system needs sslmode=verify-full");
    says(open(&mut it, "sslmode=verify-ca"), "sslmode=verify-ca needs sslrootcert");
    says(open(&mut it, &format!("sslmode=verify-full&sslrootcert={}", empty.display())), "no PEM certificates in it");
    says(open(&mut it, &format!("sslmode=verify-full&sslrootcert={}", not_a_cert.display())), "sslrootcert");
    says(open(&mut it, "sslmode=verify-full&sslrootcert=/no/such/ca.pem"), "sslrootcert /no/such/ca.pem");
    says(eval(&mut it, "db.open(\"postgres://u@127.0.0.1:port/x\")"), "bad Postgres URL");
}

#[test]
fn the_systems_certificates_are_used_when_asked_for() {
    let Some(pg) = support::postgres("system") else { return };
    let mut it = program("system-certs", &pg.url, "");
    // This server has no TLS, and verify-full will not go without.
    says(eval(&mut it, &format!("db.open(\"{}?sslrootcert=system\")", pg.url)), "TLS");
}

#[test]
fn a_system_without_certificates_says_so() {
    let d = support::temp_dir("dbpg-nocerts");
    let empty = d.join("empty.pem");
    std::fs::write(&empty, "").unwrap();
    let file = d.join("app.qwe");
    std::fs::write(&file, "route GET /open = str(db.open(\"postgres://u@127.0.0.1:1/x?sslrootcert=system\"))\n").unwrap();
    let s = support::serve(&file, &[("SSL_CERT_FILE", empty.to_str().unwrap()), ("SSL_CERT_DIR", d.to_str().unwrap())]);
    says(support::send(s.port, "GET", "/open", &[], "").body, "sslrootcert=system: no CA certificates found on this system");
}

/// A certificate for `localhost`, in the server's folder.
fn certificate(dir: &Path) {
    let key = rcgen::KeyPair::generate().unwrap();
    let cert = rcgen::CertificateParams::new(vec!["localhost".to_string()]).unwrap().self_signed(&key).unwrap();
    std::fs::write(dir.join("server.crt"), cert.pem()).unwrap();
    std::fs::write(dir.join("server.key"), key.serialize_pem()).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir.join("server.key"), std::fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn an_older_tls_is_checked_too() {
    let Some(pg) = support::postgres("tls12") else { return };
    certificate(&data_dir("tls12"));
    psql(&pg, &pg.url, &["alter system set ssl = on", "alter system set ssl_max_protocol_version = 'TLSv1.2'"]);
    restart(&pg, "tls12");
    let mut it = program("tls12", &format!("{}?sslmode=require", pg.url), "");
    assert_eq!(eval(&mut it, "db.one(\"select version from pg_stat_ssl where pid = pg_backend_pid()\").version"), "\"TLSv1.2\"");
}
