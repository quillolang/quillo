// Quillo 0.8.12, slice S1 (branch feedback86-89): databases. Q-1060 (FEEDBACK75.md): on
// PostgreSQL's connection requests share, a view, a rule, a trigger or a domain's check that calls
// `set_config(..., false)` set the session for the next requests, past row-level security, and
// `lastval()` read another request's sequence. Q-603 (FEEDBACK49.md): SQLite's wait for another
// process's write lock held the interpreter, so every other request waited with it.

mod support;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// A folder with `main.qwe`, removed when the value goes.
struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl std::ops::Deref for Dir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

fn dir(name: &str, src: &str) -> Dir {
    let d = support::temp_dir(&format!("f812-{}", name));
    std::fs::write(d.join("main.qwe"), src).unwrap();
    Dir(d)
}

/// `quillo check` of `d`'s main.qwe passes, within a deadline.
fn checked(d: &Path) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("check")
        .arg(d.join("main.qwe"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let end = Instant::now() + Duration::from_secs(60);
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        if Instant::now() > end {
            let _ = child.kill();
            let _ = child.wait();
            panic!("quillo check did not end within 60 s");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut said = String::new();
    let _ = child.stdout.take().unwrap().read_to_string(&mut said);
    let _ = child.stderr.take().unwrap().read_to_string(&mut said);
    assert!(status.success(), "{}", said);
}

// ---- Q-1060: what the catalog says, read from the statement --------------------------------------

/// What the reading still keeps for the shared connection: the names a statement calls, as
/// PostgreSQL takes them (`U&"..."` with its own `UESCAPE`, cut to the 63 bytes it keeps); a
/// routine's header with a `begin` of its own no longer hides the statement after it; and which
/// statements change the catalog: `create`, `alter`, `drop` of anything but a temporary one, `do`;
/// a procedure's `call` no longer (a database whose code may run DDL counts as using a session's
/// state from the start: the third review). `pg_input_is_valid` runs a type's input from text,
/// apart (FEEDBACK75.md, Q-1060, and its reviews; branch feedback86-89).
#[test]
fn q1060_what_a_statement_calls_and_changes_is_read_as_postgresql_takes_it() {
    use quillo::sql::{scan, Dialect};
    let calls = |sql: &str| scan(Dialect::Postgres, sql).calls.iter().map(|n| n.to_string()).collect::<Vec<_>>();
    let long = "a".repeat(70);
    assert_eq!(calls(&format!("select {}()", long)), [&long[..63]]);
    assert_eq!(calls(&format!("select \"{}\"()", "é".repeat(40))), ["é".repeat(31)]);
    assert_eq!(calls(&format!("select {}()", "b".repeat(63))), ["b".repeat(63)]);
    assert_eq!(calls("select U&\"!0066\"(1)"), ["!0066"]);
    assert!(calls("select pg_catalog.lower('X')").contains(&"lower".to_string()));
    assert!(calls("").is_empty());
    assert!(scan(Dialect::Postgres, "create function f() returns begin language sql as $$ select 1 $$; select set_config('role', 'x', false)").changes_session(&[]));
    assert!(!scan(Dialect::Postgres, "create function f() returns int language sql begin atomic select 1; end").changes_session(&[]));
    for sql in ["create view v as select 1", "alter table t add y int", "drop table t", "create trigger t before insert on x for each row execute function f()", "do $$ begin perform 1; end $$", "select 1; create index i on t (x)", "drop table if exists tt"] {
        assert!(scan(Dialect::Postgres, sql).changes_catalog(), "{}", sql);
    }
    for sql in ["select 1", "insert into t values (1)", "update t set x = 1", "select 'create view v'", "create temp table t (x int)", "create temporary view v as select 1", "create local temp table t (x int) on commit drop", "call provision()", ""] {
        assert!(!scan(Dialect::Postgres, sql).changes_catalog(), "{}", sql);
    }
    assert!(scan(Dialect::Postgres, "select pg_input_is_valid('x', 'dmail')").unseen);
    assert!(!scan(Dialect::Mysql, "create view v as select 1").changes_catalog());
}

/// `lastval()` and `currval()` read the session's sequences, however written; a column or a string
/// that names them does not; MySQL and SQLite have none (FEEDBACK75.md, Q-1060; branch
/// feedback86-89).
#[test]
fn q1060_a_statement_that_reads_the_sessions_sequences_is_known() {
    use quillo::sql::{scan, Dialect};
    let reads = |sql: &str| scan(Dialect::Postgres, sql).reads_last_value();
    for sql in [
        "select setseed(0.5)",
        "select lastval()",
        "select currval('s') as n",
        "SELECT LASTVAL()",
        "select pg_catalog.lastval()",
        "select \"currval\"('s')",
        "insert into t (id) values (currval('s'))",
        "select 1; select lastval()",
    ] {
        assert!(reads(sql), "{}", sql);
    }
    for sql in ["select nextval('s')", "select lastval from t", "select 'lastval()'", "select setval('s', 1)", "select \"LastVal\"()", ""] {
        assert!(!reads(sql), "{}", sql);
    }
    assert!(!scan(Dialect::Mysql, "select lastval()").reads_last_value());
    assert!(!scan(Dialect::Sqlite, "select lastval()").reads_last_value());
}

// ---- Q-1060: the shared connection, on PostgreSQL --------------------------------------------------

/// psql as the server's superuser, on database `url`.
fn psql(pg: &support::Pg, url: &str, sql: &str) {
    let out = Command::new(pg.bin.join("psql")).args([url, "-q", "-v", "ON_ERROR_STOP=1", "-c", sql]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

/// The report's database, and a view, a rule, a trigger and a domain whose check each set
/// `app.user_id` for the session; plain tables (one a foreign key's), a sequence. The plain role
/// `app` has every statement it runs written to the server's log.
const SHOP: &str = r#"
create role app login;
create table orders (id serial primary key, owner text, item text);
insert into orders (owner, item) values ('alice', 'alice-secret-ring'), ('bob', 'bob-hat');
alter table orders enable row level security;
create policy own on orders using (owner = coalesce(nullif(current_setting('app.user_id', true), ''), '-'));
grant select on orders to app;
create view setter as select set_config('app.user_id', 'alice', false) as v;
grant select on setter to app;
create table guard (v text);
insert into guard values ('');
create table notes_log (x text);
create rule notes_log_r as on insert to notes_log do also update guard set v = set_config('app.user_id', 'alice', false);
grant insert, select on notes_log to app;
create function audit_trg() returns trigger language plpgsql as $$ begin perform set_config('app.user_id', 'alice', false); return new; end $$;
create table audit (x text);
create trigger audit_t before insert on audit for each row execute function audit_trg();
grant insert, select on audit to app;
create function okv(text) returns bool language plpgsql as $$ begin perform set_config('app.user_id', 'alice', false); return true; end $$;
create domain checked as text check (okv(value));
create table dtab (c checked);
grant insert, select on dtab to app;
create table plain (id int primary key, item text);
insert into plain values (1, 'p');
create table kid (id int primary key, plain_id int references plain (id));
insert into kid values (1, 1);
grant select, insert on plain, kid to app;
create sequence s;
grant usage, select on sequence s to app;
grant create on schema public to app;
create table mine (x text);
alter table mine owner to app;
create table mine3 (x text);
alter table mine3 owner to app;
create table "user" (x text);
create trigger user_t before insert on "user" for each row execute function audit_trg();
grant insert, select on "user" to app;
create view my_orgs as select 1 as org_id from (select set_config('app.user_id', 'alice', false)) s;
create table docs (id int, org_id int, body text);
insert into docs values (1, 1, 'doc');
alter table docs enable row level security;
create policy docs_p on docs using (org_id in (select org_id from my_orgs));
grant select on docs, my_orgs to app;
create table notes2 (body text);
insert into notes2 values ('n');
alter table notes2 enable row level security;
create policy notes2_p on notes2 using (set_config('app.user_id', 'alice', false) is not null);
grant select on notes2 to app;
create table orders2 (id serial primary key, item text);
create table refs2 (ref bigint default currval('orders2_id_seq'), note text);
grant insert, select on orders2, refs2 to app;
grant usage, select on sequence orders2_id_seq to app;
create function next_code() returns text language sql as $$ select 'c' || floor(random() * 1000)::text $$;
create table codes (c text default next_code(), n int);
grant select, insert on codes to app;
create function leaky(plain) returns text language plpgsql as $$ begin perform set_config('app.user_id', 'alice', false); return 'x'; end $$;
create table pdocs (owner text, body text);
insert into pdocs values ('x', 'pd');
alter table pdocs enable row level security;
create policy pdocs_p on pdocs using ((body::checked) is not null);
grant select on pdocs to app;
create table ddef (x text default ('d'::checked)::text, y int);
grant insert, select on ddef to app;
create table tp (id int primary key);
create table tc (id int references tp (id));
create function trg_s() returns trigger language plpgsql as $$ begin perform set_config('app.user_id', 'alice', false); return null; end $$;
create trigger tc_t before truncate on tc for each statement execute function trg_s();
grant truncate, select on tp, tc to app;
create function my_cmp(text, text) returns int language plpgsql immutable as $$ begin perform set_config('app.user_id', 'alice', false); return bttextcmp($1, $2); end $$;
create operator class my_text_ops for type text using btree as operator 1 <, operator 2 <=, operator 3 =, operator 4 >=, operator 5 >, function 1 my_cmp(text, text);
create table p8 (x text);
create index p8_i on p8 (x my_text_ops);
grant insert, select on p8 to app;
alter role app set log_statement = 'all';
"#;

const SHOP_APP: &str = r#"db.open("URL")
route GET /view = db.one("select v from setter").v
route POST /rule = db.exec("insert into notes_log (x) values ('n')").changes
route POST /trigger = db.exec("insert into audit (x) values ('a')").changes
route GET /domain = db.one("select 'x'::checked as v").v
route POST /domain-column = db.exec("insert into dtab (c) values (?)", "y").changes
route GET /orders = db.query("select item from orders")
route GET /who = db.one("select coalesce(current_setting('app.user_id', true), '') as u").u
route GET /plain = db.query("select p.item from plain p join kid k on k.plain_id = p.id")
route GET /audit-read = db.one("select count(*) as n from audit").n
route GET /domain-read = db.one("select count(*) as n from dtab").n
route POST /mine = db.exec("insert into mine (x) values ('m')").changes
route POST /migrate = len(db.migrate("migrations"))
route POST /user = db.exec("insert into public.user (x) values ('u')").changes
route GET /docs = db.query("select body from docs")
route GET /notes2 = db.query("select body from notes2")
route POST /mkview = db.transaction(() => db.exec("create view v2 as select set_config('app.user_id', 'alice', false) as v").changes)
route GET /v2 = db.one("select v from v2").v
route POST /mktrig = db.exec("create trigger mine_t2 before insert on mine for each row execute function audit_trg()").changes
route POST /ddl-tx = db.transaction(() => {
    db.exec("create trigger mine3_t before insert on mine3 for each row execute function audit_trg()")
    sleep(400)
    return 1
})
route POST /mine3 = db.exec("insert into mine3 (x) values ('m')").changes
route POST /order2 = db.one("insert into orders2 (item) values ('a') returning id").id
route POST /ref2 {
    let r = db.exec("insert into refs2 (note) values ('b')")
    return is_err(r) ? "refused" : str(db.one("select max(ref) as r from refs2").r)
}
route GET /codes = db.one("select count(*) as n from codes").n
route POST /code = db.exec("insert into codes (n) values (1)").changes
route GET /a = db.one("select p.leaky as v from plain p").v
route GET /b = db.query("select body from pdocs")
route GET /b2 = db.one("select ('(x)'::dtab).c as v").v
route GET /b2j = db.one("select c as v from json_populate_record(null::dtab, json_build_object('c', 'x'))").v
route POST /b3 = db.exec("insert into ddef (y) values (1)").changes
route POST /c = db.exec("truncate tp cascade").changes
route POST /g = db.exec("insert into p8 (x) values ('c')").changes
route POST /setseed {
    let r = db.one("select setseed(0.5) as s")
    return is_err(r) ? r.error : "ran"
}
route GET /seq = db.one("select nextval('s') as n").n
route GET /last {
    let r = db.one("select lastval() as n")
    return is_err(r) ? r.error : str(r.n)
}
route GET /cur {
    let r = db.one("select currval('s') as n")
    return is_err(r) ? r.error : str(r.n)
}
route GET /last-in = db.transaction(() => {
    let a = db.one("select nextval('s') as n").n
    return [a, db.one("select lastval() as n").n]
})
route GET /last-alone {
    let r = db.transaction(() => db.one("select lastval() as n"))
    return is_err(r) ? r.error : str(r.n)
}
"#;

/// The migration the program runs: a trigger on its own table that sets the session.
const MINE_TRIGGER: &str = "create function mine_trg() returns trigger language plpgsql as $$ begin perform set_config('app.user_id', 'alice', false); return new; end $$;\n\
create trigger mine_t before insert on mine for each row execute function mine_trg();\n";

/// The report's repro and the forms the triage found on 0.8.11: a view, a rule, a trigger, a domain
/// named and a domain's column, each calling `set_config(..., false)`, run on a connection of their
/// own, reset after, and later requests read none of Alice's rows on every connection the requests
/// share (on 0.8.11, half of them did after each). Each answers as before (FEEDBACK75.md, Q-1060;
/// branch feedback86-89).
#[test]
fn q1060_a_view_rule_trigger_or_domain_that_sets_the_session_leaves_nothing_on_the_shared_connection() {
    let Some(pg) = support::postgres("f812-q1060") else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    let url = pg.database("f812_shop");
    psql(&pg, &url, SHOP);
    let app_url = url.replace("postgres://quillo@", "postgres://app@");
    let d = dir("q1060", &SHOP_APP.replace("URL", &app_url));
    checked(&d);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let call = |method: &str, path: &str| {
        let r = support::send(s.port, method, path, &[], "");
        assert_eq!(r.status, 200, "{}: {}", path, r.body);
        r.body
    };
    let nothing_left = |after: &str| {
        // Eight requests, over every connection the requests share.
        let all: Vec<(String, String)> = (0..8).map(|_| (call("GET", "/orders"), call("GET", "/who"))).collect();
        assert!(all.iter().all(|(o, w)| o == "[]" && w.is_empty()), "after {}: {:?}", after, all);
    };
    nothing_left("nothing");
    assert_eq!(call("GET", "/view"), "alice");
    nothing_left("/view");
    assert_eq!(call("POST", "/rule"), "1");
    nothing_left("/rule");
    assert_eq!(call("POST", "/trigger"), "1");
    nothing_left("/trigger");
    assert_eq!(call("GET", "/domain"), "x");
    nothing_left("/domain");
    assert_eq!(call("POST", "/domain-column"), "1");
    nothing_left("/domain-column");
    // Again: what was judged once is kept, and still holds.
    assert_eq!(call("GET", "/view"), "alice");
    nothing_left("/view again");
}

/// A server with the shop: its port, its checker of what is left on the lanes, and a caller.
struct Shop {
    _pg: support::Pg,
    _d: Dir,
    s: support::Server,
}

impl Shop {
    fn start(name: &str, extra: &[(&str, &str)]) -> Option<Shop> {
        let pg = support::postgres(&format!("f812-{}", name))?;
        let url = pg.database(&format!("f812_{}", name.replace('-', "_")));
        psql(&pg, &url, SHOP);
        let app_url = url.replace("postgres://quillo@", "postgres://app@");
        let d = dir(name, &SHOP_APP.replace("URL", &app_url));
        for (f, text) in extra {
            if let Some(parent) = d.join(f).parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(d.join(f), text).unwrap();
        }
        checked(&d);
        let s = support::serve(&d.join("main.qwe"), &[]);
        Some(Shop { _pg: pg, _d: d, s })
    }

    fn call(&self, method: &str, path: &str) -> String {
        let r = support::send(self.s.port, method, path, &[], "");
        assert_eq!(r.status, 200, "{}: {}", path, r.body);
        r.body
    }

    /// Eight requests, over every connection the requests share, each reading no one's setting.
    fn nothing_left(&self, after: &str) {
        let all: Vec<(String, String)> = (0..8).map(|_| (self.call("GET", "/orders"), self.call("GET", "/who"))).collect();
        assert!(all.iter().all(|(o, w)| o == "[]" && w.is_empty()), "after {}: {:?}", after, all);
    }
}

/// The times the server set a connection back (`pg::RESET`, after a statement run apart or a
/// transaction), as its log says.
fn resets(pg: &support::Pg) -> usize {
    pg.log().matches("close all; discard temp").count()
}

/// The times a shared connection's statement went after a reset in its pipeline (`pg::LANE_RESET`).
fn lane_resets(pg: &support::Pg) -> usize {
    let log = pg.log();
    // In one query (a transaction's, a prepare's), or statement by statement under one Sync.
    log.matches("discard sequences; close all").count() + log.matches(": close all").count()
}

/// Until the database's watcher has answered: statements before it go after a reset. Calls `ping`
/// until three in a row add none (no fixed wait).
fn settled(pg: &support::Pg, ping: &dyn Fn()) {
    let mut quiet = 0;
    for _ in 0..2000 {
        let before = lane_resets(pg);
        ping();
        quiet = if lane_resets(pg) == before { quiet + 1 } else { 0 };
        if quiet == 3 {
            return;
        }
    }
    panic!("the database was never known");
}

/// A database whose own code uses no session state: tables, a foreign key, a view of them, a
/// policy, and a SQL function with a stored body as a column's default.
const CLEAN: &str = r#"
create role app login;
create table plain (id int primary key, item text);
insert into plain values (1, 'p');
create table kid (id int primary key, plain_id int references plain (id));
insert into kid values (1, 1);
create view plain_v as select item from plain;
create table owned (owner text, item text);
insert into owned values ('x', 'o');
alter table owned enable row level security;
create policy owned_p on owned using (owner = 'x');
create function shout(t text) returns text language sql immutable begin atomic select upper(t); end;
create table logged (item text default shout('a'));
grant select, insert on plain, kid, plain_v, owned, logged to app;
alter role app set log_statement = 'all';
"#;

/// A function that makes a trigger setting the session, by DDL of its own (its text never names
/// `set_config` whole).
const MK: &str = r#"
create function mk() returns int language plpgsql security definer as $f$ begin
  execute 'create function z() returns trigger language plpgsql as $$ begin perform set_' || 'config(''app.user_id'', ''alice'', false); return new; end $$';
  execute 'create trigger z_t before insert on logged for each row execute function z()';
  return 1;
end $f$;
grant execute on function mk() to app;
"#;

/// On a database whose own code uses no session state everything stays on the shared, pipelined
/// connections, as on 0.8.11: tables, a foreign key, a view, a policy, a trigger, reads and writes,
/// none set back before or after (FEEDBACK75.md, Q-1060, the user's decisions after its second
/// review; branch feedback86-89).
#[test]
fn q1060_a_database_whose_code_uses_no_session_keeps_its_pipelining() {
    let Some(pg) = support::postgres("f812-q1060-clean") else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    let url = pg.database("f812_clean");
    psql(&pg, &url, CLEAN);
    let app = url.replace("postgres://quillo@", "postgres://app@");
    let src = format!(
        "db.open(\"{}\")\nroute GET /plain = db.query(\"select p.item from plain p join kid k on k.plain_id = p.id\")\nroute GET /view = db.query(\"select item from plain_v\")\nroute GET /owned = db.query(\"select item from owned\")\nroute POST /log = db.exec(\"insert into logged (item) values ('a')\").changes\n",
        app
    );
    let d = dir("q1060-clean", &src);
    checked(&d);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let call = |m: &str, p: &str| {
        let r = support::send(s.port, m, p, &[], "");
        assert_eq!(r.status, 200, "{}: {}", p, r.body);
        r.body
    };
    assert_eq!(call("GET", "/plain"), r#"[{"item":"p"}]"#);
    settled(&pg, &|| {
        call("GET", "/plain");
    });
    let (before, lane) = (resets(&pg), lane_resets(&pg));
    for _ in 0..4 {
        assert_eq!(call("GET", "/plain"), r#"[{"item":"p"}]"#);
        assert_eq!(call("GET", "/view"), r#"[{"item":"p"}]"#);
        assert_eq!(call("GET", "/owned"), r#"[{"item":"o"}]"#);
        assert_eq!(call("POST", "/log"), "1");
    }
    assert_eq!(resets(&pg), before, "{}", pg.log());
    assert_eq!(lane_resets(&pg), lane, "{}", pg.log());
}

/// A database whose own code comes to use a session's state, through another pool of the process
/// (an owner's connection making a trigger): from then on its statements go after a reset in
/// their pipeline, and leave nothing. One with a function of its own that may make one (DDL in its
/// text, which never names `set_config` whole) counts as using it from the start (FEEDBACK75.md, Q-1060, the second review; branch feedback86-89).
#[test]
fn q1060_session_code_made_by_another_pool_or_a_function_is_seen() {
    for (name, how) in [("pool", "/admin"), ("fn", "/mk")] {
        let Some(pg) = support::postgres(&format!("f812-q1060-made-{}", name)) else {
            eprintln!("skipping: no PostgreSQL");
            return;
        };
        let url = pg.database("f812_made");
        psql(&pg, &url, CLEAN);
        psql(&pg, &url, "create table orders (id serial primary key, owner text, item text); insert into orders (owner, item) values ('alice', 'ring'); grant select on orders to app");
        if name == "fn" {
            psql(&pg, &url, MK);
        }
        let app = url.replace("postgres://quillo@", "postgres://app@");
        let src = format!(
            "db.open(\"{}\")\nlet admin = db.connect(\"{}\")\nroute POST /log = db.exec(\"insert into logged (item) values ('a')\").changes\nroute GET /who = db.one(\"select coalesce(current_setting('app.user_id', true), '') as u\").u\nroute GET /orders = db.query(\"select item from orders where owner = coalesce(nullif(current_setting('app.user_id', true), ''), '-')\")\nroute POST /mk = db.one(\"select mk() as n\").n\nroute POST /admin = admin.transaction(() => admin.exec(\"create function z() returns trigger language plpgsql as $$ begin perform set_config('app.user_id', 'alice', false); return new; end $$; create trigger z_t before insert on logged for each row execute function z()\").changes)\n",
            app, url
        );
        let d = dir(&format!("q1060-made-{}", name), &src);
        checked(&d);
        let s = support::serve(&d.join("main.qwe"), &[]);
        let call = |m: &str, p: &str| {
            let r = support::send(s.port, m, p, &[], "");
            assert_eq!(r.status, 200, "{}: {}", p, r.body);
            r.body
        };
        assert_eq!(call("POST", "/log"), "1");
        call("POST", how);
        let before = lane_resets(&pg);
        assert_eq!(call("POST", "/log"), "1");
        assert!(lane_resets(&pg) > before, "{}: the insert after it went with no reset", name);
        let all: Vec<(String, String)> = (0..8).map(|_| (call("GET", "/orders"), call("GET", "/who"))).collect();
        assert!(all.iter().all(|(o, w)| o == "[]" && w.is_empty()), "{}: {:?}", name, all);
    }
}

/// Event triggers whose function sets the session: `comment`, `grant` and `select ... into`, which
/// fire them without making anything the reading counts, go after a reset, and leave nothing (the second
/// review's; FEEDBACK75.md, Q-1060; branch feedback86-89).
#[test]
fn q1060_event_triggers_setting_the_session_leave_nothing() {
    let Some(pg) = support::postgres("f812-q1060-ev") else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    let url = pg.database("f812_ev");
    psql(
        &pg,
        &url,
        "create role app login; grant create on schema public to app; create table orders (id serial primary key, owner text, item text); insert into orders (owner, item) values ('alice', 'ring'); grant select on orders to app; \
         create table mine (x int); alter table mine owner to app; \
         create function ev() returns event_trigger language plpgsql as $$ begin perform set_config('app.user_id', 'alice', false); end $$; \
         create event trigger ev_t on ddl_command_end execute function ev()",
    );
    let app = url.replace("postgres://quillo@", "postgres://app@");
    let src = format!(
        "db.open(\"{}\")\nroute GET /who = db.one(\"select coalesce(current_setting('app.user_id', true), '') as u\").u\nroute GET /orders = db.query(\"select item from orders where owner = coalesce(nullif(current_setting('app.user_id', true), ''), '-')\")\nroute POST /comment = db.exec(\"comment on table mine is 'x'\").changes\nroute POST /grant = db.exec(\"grant select on mine to public\").changes\nroute POST /selinto = db.exec(\"select 1 as x into newt\").changes\n",
        app
    );
    let d = dir("q1060-ev", &src);
    checked(&d);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let call = |m: &str, p: &str| {
        let r = support::send(s.port, m, p, &[], "");
        assert_eq!(r.status, 200, "{}: {}", p, r.body);
        r.body
    };
    for p in ["/comment", "/grant", "/selinto"] {
        call("POST", p);
        let all: Vec<(String, String)> = (0..8).map(|_| (call("GET", "/orders"), call("GET", "/who"))).collect();
        assert!(all.iter().all(|(o, w)| o == "[]" && w.is_empty()), "after {}: {:?}", p, all);
    }
}

/// Whether a database's code uses a session's state is asked in one cheap query: on a catalog of
/// 10,000 tables, each with a key, a default, a check and an index, it takes milliseconds (here
/// under 3 s, for a slow machine), with no loop over the relations (FEEDBACK75.md, Q-1060, the
/// second review; branch feedback86-89).
#[test]
fn q1060_the_check_is_cheap_on_ten_thousand_tables() {
    let Some(pg) = support::postgres("f812-q1060-10k") else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    let url = pg.database("f812_10k");
    // 500 at a time: one transaction holds a lock on each table it makes.
    for from in (1..=10000).step_by(500) {
        psql(
            &pg,
            &url,
            &format!("do $$ begin for i in {}..{} loop execute format('create table t%s (id serial primary key, x text default ''a'' check (x <> ''''), y int)', i); execute format('create index t%s_y on t%s (y)', i, i); end loop; end $$", from, from + 499),
        );
    }
    let pool = quillo::pg::Pool::open(&url).unwrap();
    let at = Instant::now();
    assert_eq!(pool.ask_session_use(), Ok(false));
    let ms = at.elapsed().as_secs_f64() * 1000.0;
    eprintln!("the check on 10,000 tables: {} ms", ms);
    assert!(ms < 3000.0, "{} ms", ms);
}

/// On a database with an operator of its own whose function's stored body calls what the safe list
/// does not hold (`pg_backend_pid`; a plain one keeps it clean), every
/// statement outside a transaction goes after a reset in its pipeline, `select 1` too, still
/// on the shared connection; the program's other database, without one, goes with none
/// (FEEDBACK75.md, Q-1060; branch feedback86-89).
#[test]
fn q1060_a_database_with_its_own_operators_sets_back_before_every_statement() {
    let Some(pg) = support::postgres("f812-q1060-ops") else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    let url = pg.database("f812_main");
    let ops = pg.database("f812_ops");
    psql(&pg, &url, "create role app login; alter role app set log_statement = 'all'");
    psql(&pg, &ops, "create function eq3(int, int) returns boolean language sql immutable begin atomic select $1 = $2 and pg_backend_pid() > 0; end; create operator === (leftarg = int4, rightarg = int4, function = eq3)");
    let app = |u: &str| u.replace("postgres://quillo@", "postgres://app@");
    let src = format!(
        "db.open(\"{}\")\nlet ops = db.connect(\"{}\")\nroute GET /main = db.one(\"select 1 as v\").v\nroute GET /ops = ops.one(\"select 1 as v\").v\nroute GET /ops-eq = ops.one(\"select 2 === 2 as v\").v\n",
        app(&url),
        app(&ops)
    );
    let d = dir("q1060-ops", &src);
    checked(&d);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let call = |path: &str| {
        let r = support::send(s.port, "GET", path, &[], "");
        assert_eq!(r.status, 200, "{}: {}", path, r.body);
        r.body
    };
    assert_eq!(call("/main"), "1");
    assert_eq!(call("/ops"), "1");
    settled(&pg, &|| {
        call("/main");
    });
    let (before, lane) = (resets(&pg), lane_resets(&pg));
    for _ in 0..3 {
        assert_eq!(call("/main"), "1");
    }
    assert_eq!(lane_resets(&pg), lane);
    // One in front of the statement, and one in front of its parse where this connection had not
    // prepared it yet.
    assert_eq!(call("/ops"), "1");
    let after = lane_resets(&pg);
    assert!(after > lane, "{}", after);
    assert_eq!(call("/ops-eq"), "true");
    assert!(lane_resets(&pg) > after);
    assert_eq!(resets(&pg), before, "none run apart");
}

/// The review's probes of the first fix, which read names and the catalog: a view the program makes
/// and then reads, a trigger it adds to a table after an insert there, a table named by a reserved
/// word after a dot (`public.user`) with a trigger, a policy that reads a view setting the session,
/// and one that calls `set_config` itself; and the second review's. The database's own code uses a
/// session's state, so each statement goes after a reset in its pipeline, and none reaches a later
/// request; `setseed` is refused outside a transaction, as `lastval` is (FEEDBACK75.md, Q-1060, the
/// reviews of its fix; branch feedback86-89).
#[test]
fn q1060_what_no_reading_of_names_sees_leaves_nothing_on_the_shared_connection() {
    let Some(shop) = Shop::start("q1060-probes", &[]) else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    shop.nothing_left("nothing");
    assert_eq!(shop.call("POST", "/mkview"), "0");
    assert_eq!(shop.call("GET", "/v2"), "alice");
    shop.nothing_left("a view made, then read");
    assert_eq!(shop.call("POST", "/mine"), "1");
    assert_eq!(shop.call("POST", "/mktrig"), "0");
    assert_eq!(shop.call("POST", "/mine"), "1");
    shop.nothing_left("a trigger added after an insert");
    assert_eq!(shop.call("POST", "/user"), "1");
    shop.nothing_left("public.user's trigger");
    assert_eq!(shop.call("GET", "/docs"), r#"[{"body":"doc"}]"#);
    shop.nothing_left("a policy reading a view");
    assert_eq!(shop.call("GET", "/notes2"), r#"[{"body":"n"}]"#);
    shop.nothing_left("a policy calling set_config");
    // The second review's: an own function in attribute notation, a domain reached from a
    // policy's cast, a row type and a default, `truncate ... cascade` reaching a trigger, an
    // operator class of the database's own.
    assert_eq!(shop.call("GET", "/a"), "x");
    shop.nothing_left("an own function in attribute notation");
    assert_eq!(shop.call("GET", "/b"), r#"[{"body":"pd"}]"#);
    shop.nothing_left("a policy's cast to a domain");
    assert_eq!(shop.call("GET", "/b2"), "x");
    shop.nothing_left("a row type's domain");
    assert_eq!(shop.call("GET", "/b2j"), "x");
    shop.nothing_left("a row type from json");
    assert_eq!(shop.call("POST", "/b3"), "1");
    shop.nothing_left("a default casting to a domain");
    assert_eq!(shop.call("POST", "/c"), "0");
    shop.nothing_left("truncate cascade");
    assert_eq!(shop.call("POST", "/g"), "1");
    shop.nothing_left("an operator class of the database's own");
    assert!(shop.call("POST", "/setseed").contains("setseed()"));
}

/// A trigger made inside a transaction while other requests insert into its table: whatever the
/// catalog said meanwhile, the inserts after its commit leave nothing (FEEDBACK75.md, Q-1060, the
/// review of its first fix; branch feedback86-89).
#[test]
fn q1060_a_trigger_made_in_a_transaction_beside_inserts_leaves_nothing() {
    let Some(shop) = Shop::start("q1060-ddltx", &[]) else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    let port = shop.s.port;
    let ddl = std::thread::spawn(move || support::send(port, "POST", "/ddl-tx", &[], "").body);
    let mut done = 0;
    let end = Instant::now() + Duration::from_secs(60);
    while !ddl.is_finished() && Instant::now() < end {
        assert_eq!(shop.call("POST", "/mine3"), "1");
        done += 1;
    }
    assert_eq!(ddl.join().unwrap(), "1");
    for _ in 0..4 {
        assert_eq!(shop.call("POST", "/mine3"), "1");
    }
    assert!(done > 0);
    shop.nothing_left("a trigger made in a transaction");
}

/// A column's default reading `currval` of another table's sequence: a request's insert there never
/// gets the id another request's insert took on the same connection; with no `nextval` of its own,
/// it is refused, on every connection (FEEDBACK75.md, Q-1060, the review of its first fix; branch
/// feedback86-89).
#[test]
fn q1060_a_default_reading_currval_never_gets_another_requests_id() {
    let Some(shop) = Shop::start("q1060-currval", &[]) else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    for _ in 0..4 {
        let id = shop.call("POST", "/order2");
        assert!(id.parse::<i64>().is_ok(), "{}", id);
        let got: Vec<String> = (0..8).map(|_| shop.call("POST", "/ref2")).collect();
        assert!(got.iter().all(|g| g == "refused"), "{:?}", got);
    }
}

/// The session the URL sets (`options=-c statement_timeout=7s`) is the one each statement runs in,
/// on the lanes, after each reset in their pipeline (FEEDBACK75.md, Q-1060; branch
/// feedback86-89). Passes on 0.8.11 too: the form that must keep working.
#[test]
fn q1060_what_the_url_sets_holds_after_each_reset() {
    let Some(pg) = support::postgres("f812-q1060-url") else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    let url = pg.database("f812_url");
    // Code of the database's own that reads the session: each statement goes after a reset.
    psql(&pg, &url, "create function cs() returns text language sql as $$ select current_setting('statement_timeout') $$");
    let src = format!(
        "db.open(\"{}?options=-c%20statement_timeout%3D7s\")\nroute GET /timeout = db.one(\"select current_setting('statement_timeout') as t\").t\nroute GET /set = db.one(\"select set_config('statement_timeout', '9s', true) as t\").t\n",
        url
    );
    let d = dir("q1060-url", &src);
    checked(&d);
    let s = support::serve(&d.join("main.qwe"), &[]);
    for _ in 0..8 {
        assert_eq!(support::send(s.port, "GET", "/timeout", &[], "").body, "7s");
        assert_eq!(support::send(s.port, "GET", "/set", &[], "").body, "9s");
    }
}

/// A trigger a migration of the program's adds to its own table, after inserts there: the inserts
/// after it leave nothing (FEEDBACK75.md, Q-1060; branch feedback86-89).
#[test]
fn q1060_a_trigger_a_migration_adds_leaves_nothing() {
    let Some(shop) = Shop::start("q1060-mig", &[("migrations/001_mine_trigger.sql", MINE_TRIGGER)]) else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    assert_eq!(shop.call("POST", "/mine"), "1");
    assert_eq!(shop.call("POST", "/migrate"), "1");
    assert_eq!(shop.call("POST", "/mine"), "1");
    shop.nothing_left("a migration's trigger");
}

/// `lastval()` and `currval()` are refused outside a transaction, saying to write `returning`; the
/// `nextval` before them runs as before. Inside a transaction they read its own `nextval`, and a
/// transaction that took none reads no other's: the connection's sequences are set back with the
/// rest of its session (FEEDBACK75.md, Q-1060; branch feedback86-89).
#[test]
fn q1060_lastval_is_refused_outside_a_transaction_and_reads_its_own_inside() {
    let Some(pg) = support::postgres("f812-q1060-seq") else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    let url = pg.database("f812_seq");
    psql(&pg, &url, SHOP);
    let app_url = url.replace("postgres://quillo@", "postgres://app@");
    let d = dir("q1060-seq", &SHOP_APP.replace("URL", &app_url));
    checked(&d);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let call = |path: &str| {
        let r = support::send(s.port, "GET", path, &[], "");
        assert_eq!(r.status, 200, "{}: {}", path, r.body);
        r.body
    };
    assert_eq!(call("/seq"), "1");
    for path in ["/last", "/cur"] {
        let said = call(path);
        assert!(said.contains("outside `db.transaction` this connection is shared by every request") && said.contains("returning"), "{}: {}", path, said);
    }
    assert_eq!(call("/seq"), "2");
    assert_eq!(call("/last-in"), "[3,3]");
    // The same connection (the pool's last one given back), its sequences set back.
    let alone = call("/last-alone");
    assert!(alone.contains("lastval is not yet defined"), "{}", alone);
    assert_eq!(call("/last-in"), "[4,4]");
}

// ---- Q-603: SQLite's write lock, held by another process ------------------------------------------

const LOCKED_APP: &str = r#"db.open("main.db")
db.exec("create table if not exists t (x int)")
route POST /w {
    write_file("started", "w")
    let r = db.exec("insert into t (x) values (1)")
    return is_err(r) ? db.error_kind(r) + " " + r.error : str(r.changes)
}
route POST /two {
    write_file("started", "two")
    let r = db.exec("insert into t (x) values (3); insert into t (x) values (4)")
    return is_err(r) ? r.error : "ran"
}
route POST /tx {
    write_file("started", "tx")
    let r = db.transaction(() => db.exec("insert into t (x) values (2)").changes)
    return is_err(r) ? r.error : str(r)
}
route GET /count = db.one("select count(*) as n from t").n
route GET /counts = db.query("select x, count(*) as n from t group by x order by x")
route GET /ping = "pong"
"#;

/// Waits until the route has written `started` (it is about to run its statement), then a moment
/// more, so that its statement meets the lock.
fn started(d: &Path, what: &str) {
    let end = Instant::now() + Duration::from_secs(60);
    while std::fs::read_to_string(d.join("started")).map_or(true, |s| s != what) {
        assert!(Instant::now() < end, "the route never started");
        std::thread::sleep(Duration::from_millis(5));
    }
    std::thread::sleep(Duration::from_millis(200));
}

/// Another process's write lock, taken now (`begin immediate`).
fn lock(d: &Path) -> rusqlite::Connection {
    let other = rusqlite::Connection::open(d.join("main.db")).unwrap();
    other.execute_batch("begin immediate").unwrap();
    other
}

/// While another process holds SQLite's write lock (about 2 s), an unrelated `GET /ping` and a read
/// answer at once, the writer's statement waiting with the interpreter released; it succeeds once
/// the lock is let go. On 0.8.11 `/ping` waited for the writer, until the lock was let go
/// (FEEDBACK49.md, Q-603; branch feedback86-89).
#[test]
fn q603_another_requests_answer_while_a_statement_waits_for_the_write_lock() {
    let d = dir("q603", LOCKED_APP);
    checked(&d);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let port = s.port;
    let other = lock(&d);
    let writer = std::thread::spawn(move || (support::send(port, "POST", "/w", &[], "").body, Instant::now()));
    started(&d, "w");
    // Let go about 2 s later, on a thread of its own.
    let freed = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(2));
        other.execute_batch("rollback").unwrap();
        Instant::now()
    });
    let ping = support::send(port, "GET", "/ping", &[], "");
    let pinged = Instant::now();
    assert_eq!((ping.status, ping.body.as_str()), (200, "pong"));
    assert_eq!(support::send(port, "GET", "/count", &[], "").body, "0");
    let read = Instant::now();
    let freed = freed.join().unwrap();
    let (wrote, written) = writer.join().unwrap();
    assert!(pinged < freed && read < freed, "/ping answered only once the lock was let go");
    assert_eq!(wrote, "1");
    assert!(written >= freed);
    assert_eq!(support::send(port, "GET", "/count", &[], "").body, "1");
}

/// Past the same 5 s in all, the statement fails as before, with SQLite's own error, a conflict;
/// `/ping` answered long before (FEEDBACK49.md, Q-603; branch feedback86-89).
#[test]
fn q603_past_five_seconds_the_error_is_as_before() {
    let d = dir("q603-past", LOCKED_APP);
    checked(&d);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let port = s.port;
    let other = lock(&d);
    let sent = Instant::now();
    let writer = std::thread::spawn(move || (support::send(port, "POST", "/w", &[], "").body, Instant::now()));
    started(&d, "w");
    let ping = support::send(port, "GET", "/ping", &[], "");
    let pinged = Instant::now();
    let (wrote, written) = writer.join().unwrap();
    drop(other);
    assert_eq!(ping.body, "pong");
    assert_eq!(wrote, "conflict db: database is locked");
    assert!(written - sent >= Duration::from_millis(4900), "{:?}", written - sent);
    assert!(pinged + Duration::from_secs(1) < written, "/ping waited for the writer");
    assert_eq!(support::send(port, "GET", "/count", &[], "").body, "0");
}

/// A script of statements: the first waits released, as one statement does; the ones after it run
/// once each, never again (those before had committed) (FEEDBACK49.md, Q-603; branch
/// feedback86-89).
#[test]
fn q603_a_script_waits_released_and_runs_each_statement_once() {
    let d = dir("q603-two", LOCKED_APP);
    checked(&d);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let port = s.port;
    let other = lock(&d);
    let writer = std::thread::spawn(move || support::send(port, "POST", "/two", &[], "").body);
    started(&d, "two");
    let freed = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(1));
        other.execute_batch("rollback").unwrap();
        Instant::now()
    });
    assert_eq!(support::send(port, "GET", "/ping", &[], "").body, "pong");
    let pinged = Instant::now();
    let freed = freed.join().unwrap();
    assert_eq!(writer.join().unwrap(), "ran");
    assert!(pinged < freed);
    assert_eq!(support::send(port, "GET", "/counts", &[], "").body, r#"[{"x":3,"n":1},{"x":4,"n":1}]"#);
}

/// Inside `db.transaction` SQLite waits in place, as before: nothing else may run in its
/// transaction, so `/ping` answers only once the lock is let go and the transaction is done; it
/// then commits (FEEDBACK49.md, Q-603; branch feedback86-89). Passes on 0.8.11 too: the form that
/// must keep working.
#[test]
fn q603_inside_a_transaction_the_wait_is_in_place_as_before() {
    let d = dir("q603-tx", LOCKED_APP);
    checked(&d);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let port = s.port;
    let other = lock(&d);
    let tx = std::thread::spawn(move || support::send(port, "POST", "/tx", &[], "").body);
    started(&d, "tx");
    let freed = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(800));
        other.execute_batch("rollback").unwrap();
        Instant::now()
    });
    assert_eq!(support::send(port, "GET", "/ping", &[], "").body, "pong");
    let pinged = Instant::now();
    let freed = freed.join().unwrap();
    assert_eq!(tx.join().unwrap(), "1");
    assert!(pinged >= freed, "/ping ran inside the transaction's wait");
    assert_eq!(support::send(port, "GET", "/count", &[], "").body, "1");
}

/// A `db.transaction`'s commit in rollback-journal mode, while another process reads: the commit
/// waits for the reader in place, as before, and commits once it is done; it gave up at once there
/// (in 0.02 s) when the hold on other requests was let go before it (FEEDBACK49.md, Q-603, found in
/// review; branch feedback86-89). Passes on 0.8.11 too: the form that must keep working.
#[test]
fn q603_a_transactions_commit_waits_for_a_reader_in_place() {
    let d = dir(
        "q603-journal",
        "db.open(\"main.db\")\ndb.exec(\"pragma journal_mode = delete\")\ndb.exec(\"create table if not exists t (x int)\")\nroute POST /txj {\n    write_file(\"started\", \"txj\")\n    let r = db.transaction(() => db.exec(\"insert into t (x) values (5)\").changes)\n    return is_err(r) ? r.error : str(r)\n}\nroute GET /count = db.one(\"select count(*) as n from t\").n\n",
    );
    checked(&d);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let port = s.port;
    let reader = rusqlite::Connection::open(d.join("main.db")).unwrap();
    reader.execute_batch("begin; select count(*) from t;").unwrap();
    let tx = std::thread::spawn(move || (support::send(port, "POST", "/txj", &[], "").body, Instant::now()));
    started(&d, "txj");
    std::thread::sleep(Duration::from_millis(600));
    let freed = Instant::now();
    reader.execute_batch("commit").unwrap();
    let (said, answered) = tx.join().unwrap();
    assert_eq!(said, "1");
    assert!(answered >= freed);
    assert_eq!(support::send(port, "GET", "/count", &[], "").body, "1");
}

/// On a database whose code uses a session's state, a transaction with an isolation level begins
/// on a session set back and keeps its level: the reset in one query string with the `begin` took
/// the snapshot first, and PostgreSQL refused the level ("SET TRANSACTION ISOLATION LEVEL must be
/// called before any query"); a setting the last transaction left is gone inside the next
/// (FEEDBACK75.md, Q-1060; branch feedback86-89).
#[test]
fn q1060_a_transaction_on_a_session_database_keeps_its_isolation_level() {
    let Some(pg) = support::postgres("f812-q1060-iso") else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    let url = pg.database("f812_iso");
    psql(&pg, &url, "create function who() returns text language sql as $$ select current_setting('app.user_id', true) $$");
    let src = format!(
        "db.open(\"{}\")\nroute GET /ser = db.transaction({{ isolation: \"serializable\" }}, () => db.one(\"select current_setting('transaction_isolation') as t, coalesce(who(), '') as u\"))\nroute GET /rr = db.transaction({{ isolation: \"repeatable_read\" }}, () => db.one(\"select current_setting('transaction_isolation') as t\").t)\nroute POST /leave = db.transaction(() => db.one(\"select set_config('app.user_id', 'alice', false) as u\").u)\n",
        url
    );
    let d = dir("q1060-iso", &src);
    checked(&d);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let call = |m: &str, p: &str| {
        let r = support::send(s.port, m, p, &[], "");
        assert_eq!(r.status, 200, "{}: {}", p, r.body);
        r.body
    };
    for _ in 0..4 {
        assert_eq!(call("POST", "/leave"), "alice");
        assert_eq!(call("GET", "/ser"), r#"{"t":"serializable","u":""}"#);
        assert_eq!(call("GET", "/rr"), "repeatable read");
    }
}

/// A trigger's PL/pgSQL `set` statement (no `set_config` named), a SQL function's, and one an
/// `execute` runs from a `format()` string or a literal: the database uses a session's state, every
/// statement goes after a reset, and nothing reaches the next request: any PL/pgSQL or text-bodied
/// function counts. A SQL function with a stored body that updates keeps a database clean
/// (FEEDBACK75.md, Q-1060, the fifth review's decision; branch feedback86-89).
#[test]
fn q1060_a_functions_own_set_statement_is_seen() {
    let Some(pg) = support::postgres("f812-q1060-setstmt") else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    let clean = pg.database("f812_setclean");
    psql(&pg, &clean, "create table t (x int, y int); create function f(n int) returns int language sql begin atomic update t set y = 1 where x = n; select n; end");
    assert_eq!(quillo::pg::Pool::open(&clean).unwrap().ask_session_use(), Ok(false));
    for (name, sql) in [
        ("plpgsql", "create function z() returns trigger language plpgsql as $$ begin set app.user_id = 'alice'; return new; end $$; create trigger z_t before insert on logged for each row execute function z()"),
        ("sql", "create function z() returns void language sql as $$ select 1; reset role; set app.user_id = 'alice' $$; create function zt() returns trigger language plpgsql as $$ begin perform z(); return new; end $$; create trigger z_t before insert on logged for each row execute function zt()"),
        ("execute", "create function z() returns trigger language plpgsql as $$ begin execute format('set %I = %L', 'app.user_id', 'alice'); return new; end $$; create trigger z_t before insert on logged for each row execute function z()"),
        ("execute_literal", "create function z() returns trigger language plpgsql as $$ begin execute 'set app.user_id = ''alice'''; return new; end $$; create trigger z_t before insert on logged for each row execute function z()"),
    ] {
        let url = pg.database(&format!("f812_set_{}", name));
        psql(&pg, &url, "create table logged (item text); create table orders (owner text, item text); insert into orders values ('alice', 'ring')");
        psql(&pg, &url, sql);
        assert_eq!(quillo::pg::Pool::open(&url).unwrap().ask_session_use(), Ok(true), "{}", name);
        let src = format!(
            "db.open(\"{}\")\nroute POST /log = db.exec(\"insert into logged (item) values ('a')\").changes\nroute GET /who = db.one(\"select coalesce(current_setting('app.user_id', true), '') as u\").u\nroute GET /orders = db.query(\"select item from orders where owner = coalesce(nullif(current_setting('app.user_id', true), ''), '-')\")\n",
            url
        );
        let d = dir(&format!("q1060-set-{}", name), &src);
        checked(&d);
        let s = support::serve(&d.join("main.qwe"), &[]);
        let call = |m: &str, p: &str| {
            let r = support::send(s.port, m, p, &[], "");
            assert_eq!(r.status, 200, "{}: {}", p, r.body);
            r.body
        };
        for _ in 0..4 {
            assert_eq!(call("POST", "/log"), "1");
            let all: Vec<(String, String)> = (0..8).map(|_| (call("GET", "/orders"), call("GET", "/who"))).collect();
            assert!(all.iter().all(|(o, w)| o == "[]" && w.is_empty()), "{}: {:?}", name, all);
        }
    }
}

// ---- Q-1060: the third review's probes, end to end --------------------------------------------------

/// Serves `routes` on `url` and returns a caller.
fn served(name: &str, url: &str, routes: &str) -> (support::Server, Dir) {
    let src = format!(
        "db.open(\"{}\")\nroute GET /who = db.one(\"select coalesce(current_setting('app.user_id', true), '') as u\").u\nroute GET /role = db.one(\"select current_user as r\").r\nroute GET /tz = db.one(\"select current_setting('TimeZone') as t\").t\nroute GET /ping = db.one(\"select 1 as v\").v\n{}",
        url, routes
    );
    let d = dir(name, &src);
    checked(&d);
    (support::serve(&d.join("main.qwe"), &[]), d)
}

fn get(s: &support::Server, m: &str, p: &str) -> support::Answer {
    support::send(s.port, m, p, &[], "")
}

/// One probe: database code that changes the session where no reading of names saw it before.
/// After each poke, every connection the requests share answers as a fresh session: no setting,
/// the role `app`, the server's own time zone (FEEDBACK75.md, Q-1060, the third review's probes;
/// branch feedback86-89).
fn probe(name: &str, setup: &str, poke: &str) {
    let Some(pg) = support::postgres(&format!("r3-{}", name)) else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    let url = pg.database(&format!("f812_r3_{}", name));
    let zone = {
        let out = Command::new(pg.bin.join("psql")).args([url.as_str(), "-qAt", "-c", "show timezone"]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    psql(&pg, &url, "create role app login; create table logged (item text); grant select, insert on logged to app");
    // A language this server was built without: nothing to probe.
    if setup.starts_with("create extension") {
        let out = Command::new(pg.bin.join("psql")).args([url.as_str(), "-q", "-c", setup.split(';').next().unwrap_or("")]).output().unwrap();
        if !out.status.success() {
            eprintln!("skipping {}: {}", name, String::from_utf8_lossy(&out.stderr));
            return;
        }
    }
    psql(&pg, &url, setup);
    let app = url.replace("postgres://quillo@", "postgres://app@");
    let (s, _d) = served(&format!("q1060-r3-{}", name), &app, &format!("route {}\n", poke));
    for _ in 0..4 {
        let r = get(&s, "POST", "/poke");
        assert_eq!(r.status, 200, "{}: {}", name, r.body);
        for _ in 0..8 {
            assert_eq!(get(&s, "GET", "/who").body, "", "{}: a setting was left", name);
            assert_eq!(get(&s, "GET", "/role").body, "app", "{}: a role was left", name);
            assert_eq!(get(&s, "GET", "/tz").body, zone, "{}: a time zone was left", name);
        }
    }
}

const POKE: &str = "POST /poke = db.exec(\"insert into logged (item) values ('a')\").changes";

#[test]
fn q1060_r3_a_triggers_set_role_after_a_comment() {
    probe("setrole", "create role boss; grant boss to app; create function z() returns trigger language plpgsql as $$ begin /* the owner's */ set role boss; return new; end $$; create trigger z_t before insert on logged for each row execute function z()", POKE);
}

#[test]
fn q1060_r3_a_set_after_a_line_comment() {
    probe("comment", "create function z() returns trigger language plpgsql as $$ begin -- remember who\n set app.user_id = 'alice'; return new; end $$; create trigger z_t before insert on logged for each row execute function z()", POKE);
}

#[test]
fn q1060_r3_an_execute_of_a_variable() {
    probe("executeq", "create function z() returns trigger language plpgsql as $$ declare q text; begin q := format('set %I = %L', 'app.user_id', 'alice'); execute q; return new; end $$; create trigger z_t before insert on logged for each row execute function z()", POKE);
}

/// A function in a language no reading of text can judge (PL/Perl here; PL/Python's is the same
/// rule, and this server has no PL/Python).
#[test]
fn q1060_r3_a_function_in_another_language() {
    probe("plperl", "create extension if not exists plperl; create function z() returns trigger language plperl as $$ spi_exec_query(\"set app.user_id = 'alice'\"); return; $$; create trigger z_t before insert on logged for each row execute function z()", POKE);
}

#[test]
fn q1060_r3_an_update_of_pg_settings() {
    probe("pgsettings", "create function z() returns trigger language plpgsql as $$ begin update pg_settings set setting = 'Asia/Tokyo' where name = 'TimeZone'; return new; end $$; create trigger z_t before insert on logged for each row execute function z()", POKE);
}

#[test]
fn q1060_r3_a_triggers_when_clause() {
    probe("tgqual", "create function ok() returns trigger language plpgsql as $$ begin return new; end $$; create trigger z_t before insert on logged for each row when (set_config('app.user_id', 'alice', false) is not null) execute function ok()", POKE);
}

#[test]
fn q1060_r3_a_domains_default() {
    probe("domain", "create domain who_d as text default set_config('app.user_id', 'alice', false); create table dl (x int, v who_d); grant insert on dl to app", "POST /poke = db.exec(\"insert into dl (x) values (1)\").changes");
}

#[test]
fn q1060_r3_a_functions_argument_default() {
    probe("argdefault", "create function f(a text default set_config('app.user_id', 'alice', false)) returns int language plpgsql as $f$ begin return 1; end $f$; create function z() returns trigger language plpgsql as $$ begin perform f(); return new; end $$; create trigger z_t before insert on logged for each row execute function z()", POKE);
}

/// A literal is read as its statement is parsed: on a database whose code uses a session's state,
/// a statement first prepared after another left `TimeZone` changed is parsed on a session set
/// back, and gives the time the server's zone gives (FEEDBACK75.md, Q-1060, the third review;
/// branch feedback86-89).
#[test]
fn q1060_a_statement_is_parsed_on_a_session_set_back() {
    let Some(pg) = support::postgres("f812-q1060-parse") else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    let url = pg.database("f812_parse");
    psql(&pg, &url, "create view tokyo as select set_config('TimeZone', 'Asia/Tokyo', false) as t");
    let want = {
        let out = Command::new(pg.bin.join("psql")).args([url.as_str(), "-qAt", "-c", "select '2024-01-01 00:00'::timestamptz::text"]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    let (s, _d) = served("q1060-parse", &url, "route GET /leak = db.one(\"select t from tokyo\").t\nroute GET /at = db.one(\"select '2024-01-01 00:00'::timestamptz::text as t\").t\n");
    for _ in 0..8 {
        assert_eq!(get(&s, "GET", "/leak").body, "Asia/Tokyo");
    }
    for _ in 0..8 {
        assert_eq!(get(&s, "GET", "/at").body, want);
        assert_eq!(get(&s, "GET", "/leak").body, "Asia/Tokyo");
    }
}

/// On a database whose code uses no session state and runs no DDL, calls of its own functions are
/// not schema changes: it is asked once, and no statement is set back (the third review saw 361
/// resets and a check of about 12 ms after each call; FEEDBACK75.md, Q-1060; branch feedback86-89).
#[test]
fn q1060_calls_of_the_databases_own_functions_ask_nothing_again() {
    let Some(pg) = support::postgres("f812-q1060-calls") else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    let url = pg.database("f812_calls");
    psql(&pg, &url, "create role app login; alter role app set log_statement = 'all'; create function add1(n int) returns int language sql immutable begin atomic select n + 1; end; create procedure noop() language sql begin atomic select 1; end");
    let app = url.replace("postgres://quillo@", "postgres://app@");
    let (s, _d) = served("q1060-calls", &app, "route GET /add = db.one(\"select add1(1) as n\").n\nroute POST /noop = db.exec(\"call noop()\").changes\n");
    assert_eq!(get(&s, "GET", "/add").body, "2");
    settled(&pg, &|| {
        get(&s, "GET", "/ping");
    });
    let asks = |pg: &support::Pg| pg.log().matches("own_fn(oid").count();
    let (asked, lane) = (asks(&pg), lane_resets(&pg));
    for _ in 0..40 {
        assert_eq!(get(&s, "GET", "/add").body, "2");
        assert_eq!(get(&s, "POST", "/noop").status, 200);
        assert_eq!(get(&s, "GET", "/ping").body, "1");
    }
    assert_eq!(asks(&pg), asked, "asked again");
    assert_eq!(lane_resets(&pg), lane, "set back");
}

/// A temporary table and a session advisory lock the database's code leaves on a shared
/// connection (a trigger's; a program's own statement making either is refused) are gone before
/// the next statement there: the trigger makes its table again on every connection, and no
/// connection keeps the lock (FEEDBACK75.md, Q-1060, the third review; branch feedback86-89).
#[test]
fn q1060_temporary_tables_and_advisory_locks_are_let_go() {
    let Some(pg) = support::postgres("f812-q1060-temp") else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    let url = pg.database("f812_temp");
    psql(&pg, &url, "create table logged (item text); create function z() returns trigger language plpgsql as $$ begin create temp table scratch (x int); perform pg_try_advisory_lock(42); return new; end $$; create trigger z_t before insert on logged for each row execute function z()");
    let (s, _d) = served("q1060-temp", &url, &format!("route {}\n", POKE));
    for _ in 0..16 {
        let r = get(&s, "POST", "/poke");
        assert_eq!(r.status, 200, "{}", r.body);
    }
    for _ in 0..16 {
        assert_eq!(get(&s, "GET", "/ping").body, "1");
    }
    let out = Command::new(pg.bin.join("psql")).args([url.as_str(), "-qAt", "-c", "select pg_try_advisory_lock(42)"]).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "t", "a shared connection kept the lock");
}


// ---- Q-1060: the fourth review's cases, and the whitelist -------------------------------------------

/// A view whose function runs `set role boss` written so that only PostgreSQL's own reading sees
/// it: visitor A reads the view, and every shared connection then answers visitor B as `app`
/// (FEEDBACK75.md, Q-1060, the fourth review; branch feedback86-89).
fn bypass(name: &str, setup: &str) {
    let Some(pg) = support::postgres(&format!("r4-{}", name)) else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    let url = pg.database(&format!("f812_r4_{}", name));
    psql(&pg, &url, "create role app login; create role boss; grant boss to app; alter role app set log_statement = 'all'");
    psql(&pg, &url, setup);
    psql(&pg, &url, "grant select on v to app");
    let app = url.replace("postgres://quillo@", "postgres://app@");
    let (s, _d) = served(&format!("q1060-r4-{}", name), &app, "route GET /a = db.query(\"select r from v\")\n");
    // After the watcher's first answer: before it, every statement is set back anyway.
    get(&s, "GET", "/ping");
    rechecked(&pg, 0);
    for _ in 0..4 {
        for _ in 0..8 {
            get(&s, "GET", "/a");
        }
        for _ in 0..8 {
            assert_eq!(get(&s, "GET", "/role").body, "app", "{}: a role was left", name);
        }
    }
}

fn view_of(body: &str, extra: &str) -> String {
    format!("create function f() returns text language plpgsql {} as $body${}$body$; create view v as select f() as r", extra, body)
}

#[test]
fn q1060_r4_a_comment_ended_by_a_carriage_return() {
    bypass("cr", &view_of("begin perform 1; -- c\rset role boss;\nreturn current_user; end", ""));
}

#[test]
fn q1060_r4_an_e_strings_octal_escape() {
    bypass("eoct", &view_of("begin execute E'\\163et role boss'; return current_user; end", ""));
}

#[test]
fn q1060_r4_an_e_strings_hex_escape() {
    bypass("ehex", &view_of("begin execute E'\\x73et role boss'; return current_user; end", ""));
}

#[test]
fn q1060_r4_an_e_strings_unicode_escape() {
    bypass("euni", &view_of("begin execute E'\\u0073et role boss'; return current_user; end", ""));
}

#[test]
fn q1060_r4_a_non_ascii_dollar_tag() {
    bypass("uml", &view_of("begin perform $ä$'$ä$; set role boss; perform $ä$'$ä$; return current_user; end", ""));
}

#[test]
fn q1060_r4_a_vertical_tab_before_a_statement() {
    bypass("vt", &view_of("begin perform 1;\u{0b}set role boss; return current_user; end", ""));
}

#[test]
fn q1060_r4_standard_conforming_strings_off() {
    bypass("scs", &view_of("begin perform 'a\\''; set role boss; perform ''; return current_user; end", "set standard_conforming_strings = off"));
}

#[test]
fn q1060_r4_a_typed_literal_read_as_an_e_string() {
    bypass("etyped", &format!("create domain e as text; {}", view_of("begin perform e 'a\\'; set role boss; return current_user; --';\nend", "")));
}

#[test]
fn q1060_r4_a_quoted_e_typed_literal() {
    bypass("qetyped", &format!("create domain \"E\" as text; {}", view_of("begin perform \"E\"'a\\'; set role boss; return current_user; --';\nend", "")));
}

#[test]
fn q1060_r4_an_aggregate_whose_step_is_set_config() {
    bypass("agg", "create aggregate leak(text, bool) (sfunc = pg_catalog.set_config, stype = text, initcond = 'role'); create view v as select leak('boss', false) as r");
}

#[test]
fn q1060_r4_query_to_xml_running_set_config() {
    bypass("qxml", "create view v as select query_to_xml('select pg_catalog.set_config(''role'', ''boss'', false)', true, false, '')::text as r");
}

/// Every name on the safe list is a built-in function of PostgreSQL 17's `pg_catalog`; a schema of
/// the usual kind with no functions (tables, keys, checks, a domain, a generated column, views with
/// aggregates, a policy, partitions), and one with SQL functions with stored bodies, use no session
/// state; and each unusual construct counts as using it (FEEDBACK75.md, Q-1060, the fifth review's
/// decision: parsed trees only; branch feedback86-89).
#[test]
fn q1060_parsed_trees_both_ways() {
    let Some(pg) = support::postgres("f812-q1060-white") else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    let url = pg.database("f812_white");
    let names = quillo::pg::SAFE_FUNCTIONS.iter().map(|n| format!("'{}'", n)).collect::<Vec<_>>().join(", ");
    let out = Command::new(pg.bin.join("psql"))
        .args([url.as_str(), "-qAt", "-c", &format!("select n from unnest(array[{}]) n where not exists (select 1 from pg_proc where oid < 16384 and pronamespace = 'pg_catalog'::regnamespace and proname = n)", names)])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "", "not built in");
    const USUAL: &str = r#"
create table items (id serial primary key, name text not null, price numeric(10,2) check (price >= 0), upper_name text generated always as (upper(name)) stored, updated_at timestamptz default now(), owner text);
create domain email as text check (value like '%@%');
create table people (id bigint generated always as identity primary key, mail email, joined date default current_date);
create table events (id bigint, at timestamptz not null) partition by range (at);
create table events_2026 partition of events for values from ('2026-01-01') to ('2027-01-01');
create view daily as select date_trunc('day', updated_at) as day, count(*) as n, avg(price) as mean, string_agg(name, ', ' order by name) as names from items group by 1;
create view priced as select id, coalesce(price, 0) * 1.2 as gross from items where price between 1 and 1000 order by id;
create index items_lower on items (lower(name));
alter table items enable row level security;
create policy own_items on items using (owner = current_user);
"#;
    const ATOMIC: &str = r#"
create function add1(n int) returns int language sql immutable begin atomic select n + 1; end;
create function total(cat text) returns numeric language sql stable return (select coalesce(sum(price), 0) from items where name like cat || '%');
create function touched(i int) returns void language sql begin atomic update items set updated_at = now() where id = i; end;
create view next_ids as select id, add1(id) as next from items;
"#;
    psql(&pg, &url, USUAL);
    assert_eq!(quillo::pg::Pool::open(&url).unwrap().ask_session_use(), Ok(false), "the usual schema");
    let atomic = pg.database("f812_white_atomic");
    psql(&pg, &atomic, USUAL);
    psql(&pg, &atomic, ATOMIC);
    assert_eq!(quillo::pg::Pool::open(&atomic).unwrap().ask_session_use(), Ok(false), "stored SQL bodies");
    let unusual = [
        ("plpgsql", "create function n() returns int language plpgsql as $$ begin return 1; end $$"),
        ("textsql", "create function n() returns int language sql as 'select 1'"),
        ("viewsleep", "create view n as select pg_sleep(0) is null as s"),
        ("atomicsleep", "create function n() returns boolean language sql begin atomic select pg_sleep(0) is null; end"),
        ("rulequal", "create table t (a int); create table log (a int); create rule r as on insert to t where pg_catalog.set_config('role', 'x', true) is not null do also insert into log values (new.a)"),
        ("aggregate", "create aggregate leak(text, bool) (sfunc = pg_catalog.set_config, stype = text, initcond = 'role')"),
        ("eventtrigger", "create function n() returns event_trigger language plpgsql as $$ begin null; end $$; create event trigger n_t on ddl_command_end execute function n()"),
        ("unknownfn", "create view n as select xmlcomment('x') as c"),
        ("pgsettings", "create view n as select setting from pg_settings"),
        ("indexexpr", "create index n on items (pg_size_pretty(id::bigint))"),
        ("statsexpr", "create statistics n on (pg_size_pretty(id::bigint)) from items"),
        ("partexpr", "create table pt (id int) partition by range (length(pg_size_pretty(id::bigint)))"),
        ("extmember", "create extension if not exists pg_trgm; create view n as select similarity('a', 'b') as s"),
    ];
    for (name, sql) in unusual {
        let u = pg.database(&format!("f812_white_{}", name));
        psql(&pg, &u, USUAL);
        psql(&pg, &u, sql);
        assert_eq!(quillo::pg::Pool::open(&u).unwrap().ask_session_use(), Ok(true), "{}", name);
    }
}

/// Pooled statements outside the lanes: a request's inside a SQLite transaction, the program's at
/// its start, a function of the database's own run apart, a transaction of its own. Each is set
/// back where someone else's statement ran on its connection last, and none answers another as
/// `boss` (FEEDBACK75.md, Q-1060, the fourth review's owner cases; branch feedback86-89).
#[test]
fn q1060_who_ran_last_on_a_pooled_connection() {
    let Some(pg) = support::postgres("f812-q1060-owner") else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    let url = pg.database("f812_owner");
    psql(&pg, &url, "create role app login; create role boss; grant boss to app; create function f() returns text language plpgsql as $b$begin set role boss; return current_user; end$b$; create view v as select f() as r; grant select on v to app; create function who() returns text language sql as $b$select current_user::text$b$");
    let app = url.replace("postgres://quillo@", "postgres://app@");
    let src = format!(
        r#"db.open("main.db")
db.exec("create table if not exists t (x int)")
let pg = db.connect("{}")
print("start leaves: " + pg.one("select r from v").r)
route GET /b = db.transaction(() => pg.one("select current_user::text as u").u)
route GET /a = db.transaction(() => pg.one("select r from v").r)
route GET /ab = db.transaction(() => {{
    let r = pg.one("select r from v").r
    return r + " then " + pg.one("select current_user::text as u").u
}})
route GET /err = db.transaction(() => {{
    let r = pg.one("select r from v").r
    let e = pg.one("select 1/0 as z")
    return pg.one("select current_user::text as u").u
}})
route GET /iso = pg.one("select who() as u").u
route GET /tx = pg.transaction(() => pg.one("select current_user::text as u").u)
route GET /aplain = pg.one("select r from v").r
route GET /bplain = pg.one("select current_user::text as u").u
"#,
        app
    );
    let d = dir("q1060-owner", &src);
    let s = support::serve(&d.join("main.qwe"), &[]);
    assert_eq!(get(&s, "GET", "/iso").body, "app", "a function run apart after the start");
    for p in ["/b", "/tx", "/bplain"] {
        assert_eq!(get(&s, "GET", p).body, "app", "{} after the start", p);
    }
    // Each right after the poke, on the connection it left (the pool hands back the last one).
    for poke in ["/a", "/ab", "/err", "/aplain"] {
        for p in ["/iso", "/b", "/tx", "/bplain"] {
            for _ in 0..3 {
                get(&s, "GET", poke);
                assert_eq!(get(&s, "GET", p).body, "app", "{} after {}", p, poke);
            }
        }
    }
}

/// A shared connection whose reset fails (a function it calls revoked, or the last statement's
/// `statement_timeout` cutting it short): the statement after it is not run under the session left,
/// and the connection answers no one again (FEEDBACK75.md, Q-1060, the fourth review; branch
/// feedback86-89).
#[test]
fn q1060_a_failed_reset_runs_nothing_after_it() {
    let Some(pg) = support::postgres("f812-q1060-rfail") else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    for (name, f, revoke) in [
        ("revoked", "create function f() returns text language plpgsql as $b$begin set role boss; return current_user; end$b$", true),
        ("timeout", "create function f() returns text language plpgsql as $b$begin for i in 1..300 loop execute format('create temp table if not exists t%s (a int primary key, b text unique)', i); end loop; set role boss; perform set_config('statement_timeout', '1', false); return current_user; end$b$", false),
    ] {
        let url = pg.database(&format!("f812_rfail_{}", name));
        psql(&pg, &url, "do $$ begin if not exists (select 1 from pg_roles where rolname = 'app') then create role app login; create role boss; grant boss to app; end if; end $$");
        psql(&pg, &url, &format!("{}; create view v as select f() as r; grant select on v to app; create table audit (who text, n serial); grant insert, select on audit to app, boss; grant usage on sequence audit_n_seq to app, boss", f));
        let app = url.replace("postgres://quillo@", "postgres://app@");
        let (s, _d) = served(&format!("q1060-rfail-{}", name), &app, "route GET /a = db.query(\"select r from v\")\nroute GET /w = db.exec(\"insert into audit (who) values (current_user)\").changes\n");
        for _ in 0..8 {
            get(&s, "GET", "/a");
            get(&s, "GET", "/w");
        }
        let rows = |pg: &support::Pg| {
            let out = Command::new(pg.bin.join("psql")).args([url.as_str(), "-qAt", "-c", "select count(*) from audit"]).output().unwrap();
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        let before = rows(&pg);
        if revoke {
            psql(&pg, &url, "revoke execute on function pg_catalog.pg_advisory_unlock_all() from public");
        }
        for _ in 0..8 {
            get(&s, "GET", "/a");
        }
        for _ in 0..16 {
            get(&s, "GET", "/w");
        }
        let out = Command::new(pg.bin.join("psql")).args([url.as_str(), "-qAt", "-c", "select count(*) from audit where who = 'boss'"]).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "0", "{}: a write ran as boss", name);
        if revoke {
            // Every reset fails now: no write runs at all.
            assert_eq!(rows(&pg), before, "{}: a write ran after a failed reset", name);
        }
        if revoke {
            psql(&pg, &url, "grant execute on function pg_catalog.pg_advisory_unlock_all() to public");
        }
    }
}

// ---- Q-1060: the fifth review's cases ---------------------------------------------------------------

#[test]
fn q1060_r5_a_rules_condition() {
    let Some(pg) = support::postgres("r5-rq") else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    let url = pg.database("f812_r5_rq");
    psql(&pg, &url, "create role app login; create role boss; grant boss to app; create table t (a int); create table log (a int); create rule r as on insert to t where pg_catalog.set_config('role', 'boss', false) is not null do also insert into log values (new.a); grant insert on t, log to app, boss");
    let app = url.replace("postgres://quillo@", "postgres://app@");
    let (s, _d) = served("q1060-r5-rq", &app, "route POST /a = db.exec(\"insert into t values (1)\").changes\n");
    for _ in 0..4 {
        for _ in 0..8 {
            get(&s, "POST", "/a");
        }
        for _ in 0..8 {
            assert_eq!(get(&s, "GET", "/role").body, "app", "a rule's condition left a role");
        }
    }
}

const SETROLE: &str = "begin set role boss; return current_user; end";

#[test]
fn q1060_r5_declare_as_a_column_name() {
    bypass("declare", &format!("create table dt (\"declare\" int); insert into dt values (1); grant select on dt to app; {}", view_of("begin perform declare from dt; set role boss; return current_user; end", "")));
}

#[test]
fn q1060_r5_a_shift_read_as_a_label() {
    bypass("shift", &view_of("begin perform 1 << length(pg_catalog.set_config('role', 'boss', false)) >> 1; return current_user; end", ""));
}

#[test]
fn q1060_r5_a_case_end_before_loop() {
    bypass("caseloop", &view_of("begin for i in 1 .. case when true then 1 end loop set role boss; end loop; return current_user; end", ""));
}

#[test]
fn q1060_r5_an_extension_member_named_like_a_safe_function() {
    bypass("extname", &format!("create extension if not exists pg_trgm; create function public.lower(int) returns text language plpgsql as $b${}$b$; alter extension pg_trgm add function public.lower(int); {}", SETROLE, view_of("begin return lower(1); end", "")));
}

#[test]
fn q1060_r5_an_extension_operator() {
    bypass("extop", &format!("create extension if not exists pg_trgm; create function public.evil(int, int) returns text language plpgsql as $b${}$b$; create operator public.<~> (function = public.evil, leftarg = int, rightarg = int); alter extension pg_trgm add function public.evil(int, int); alter extension pg_trgm add operator public.<~> (int, int); {}", SETROLE, view_of("begin return 1 <~> 2; end", "")));
}

/// A database that uses a session's state comes to use none while serving (the program replaces the
/// function): a connection whose last statement ran under the reset is set back once more before
/// the first statement that needs none, and so answers as `app` (FEEDBACK75.md, Q-1060, the fifth
/// review; branch feedback86-89).
#[test]
fn q1060_r5_from_session_using_to_plain() {
    let Some(pg) = support::postgres("r5-trans") else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    let url = pg.database("f812_r5_trans");
    psql(&pg, &url, "create role app login; create role boss; grant boss to app; grant create on schema public to app");
    let app = url.replace("postgres://quillo@", "postgres://app@");
    psql(&pg, &app, "create function f() returns text language plpgsql as $b$begin set role boss; return current_user; end$b$; create view v as select f() as r");
    let (s, _d) = served("q1060-r5-trans", &app, "route GET /a = db.query(\"select r from v\")\nroute POST /fix = db.exec(\"create or replace function f() returns text language sql begin atomic select current_user::text; end\").changes\n");
    for _ in 0..8 {
        get(&s, "GET", "/a");
    }
    for _ in 0..8 {
        assert_eq!(get(&s, "GET", "/role").body, "app");
    }
    for _ in 0..8 {
        get(&s, "GET", "/a");
    }
    assert_eq!(get(&s, "POST", "/fix").status, 200);
    settled(&pg, &|| {
        get(&s, "GET", "/ping");
    });
    for _ in 0..16 {
        assert_eq!(get(&s, "GET", "/role").body, "app", "a role left from before the change");
    }
}

/// The check cannot finish (another session holds `pg_aggregate`): requests are answered at once,
/// set back, never waiting for it (FEEDBACK75.md, Q-1060, the fifth review; branch feedback86-89).
#[test]
fn q1060_r5_requests_never_wait_for_the_check() {
    let Some(pg) = support::postgres("r5-hang") else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    let url = pg.database("f812_r5_hang");
    let mut locker = Command::new(pg.bin.join("psql")).args([url.as_str(), "-X", "-q"]).stdin(Stdio::piped()).stdout(Stdio::null()).spawn().unwrap();
    {
        use std::io::Write;
        let stdin = locker.stdin.as_mut().unwrap();
        writeln!(stdin, "begin; lock table pg_catalog.pg_aggregate in access exclusive mode; select pg_sleep(60);").unwrap();
    }
    // The lock is held before the program starts (asked with no aggregate, which would wait).
    let mut held = false;
    for _ in 0..200 {
        let out = Command::new(pg.bin.join("psql")).args([url.as_str(), "-qAt", "-c", "select 1 from pg_locks where relation = 'pg_catalog.pg_aggregate'::regclass and mode = 'AccessExclusiveLock'"]).output().unwrap();
        if String::from_utf8_lossy(&out.stdout).trim() == "1" {
            held = true;
            break;
        }
    }
    assert!(held);
    let (s, _d) = served("q1060-r5-hang", &url, "");
    for _ in 0..3 {
        let at = Instant::now();
        assert_eq!(get(&s, "GET", "/ping").body, "1");
        assert!(at.elapsed() < Duration::from_secs(20), "waited {:?}", at.elapsed());
    }
    let _ = locker.kill();
    let _ = locker.wait();
}

/// Another process makes the database's code use a session's state while the program serves: its
/// watcher sees it within `RECHECK_MS` and the check, and from then on nothing is left for the next
/// request. Within that window such a change is not seen: the documented limit (FEEDBACK75.md,
/// Q-1060, the fifth review; branch feedback86-89).
#[test]
fn q1060_r5_another_process_changes_the_catalog() {
    let Some(pg) = support::postgres("r5-ext") else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    let url = pg.database("f812_r5_ext");
    psql(&pg, &url, "create role app login; create role boss; grant boss to app; alter role app set log_statement = 'all'; create function f() returns text language sql begin atomic select current_user::text; end; create view v as select f() as r; grant select on v to app");
    let app = url.replace("postgres://quillo@", "postgres://app@");
    let (s, _d) = served("q1060-r5-ext", &app, "route GET /a = db.query(\"select r from v\")\n");
    settled(&pg, &|| {
        get(&s, "GET", "/ping");
    });
    let lane = lane_resets(&pg);
    psql(&pg, &url, "create or replace function f() returns text language plpgsql as $b$begin set role boss; return current_user; end$b$");
    // Seen within the interval: statements are set back again.
    let start = Instant::now();
    while lane_resets(&pg) == lane {
        get(&s, "GET", "/ping");
        assert!(start.elapsed() < Duration::from_millis(quillo::pg::RECHECK_MS * 5 + 60_000), "never seen");
    }
    for _ in 0..4 {
        for _ in 0..8 {
            get(&s, "GET", "/a");
        }
        for _ in 0..8 {
            assert_eq!(get(&s, "GET", "/role").body, "app");
        }
    }
}

// ---- Q-1060: the sixth review's cases ---------------------------------------------------------------

/// The watcher's full checks so far, as the server's log has them (the role logs every statement).
fn checks(pg: &support::Pg) -> usize {
    pg.log().matches("own_fn(oid").count()
}

/// Until the watcher has checked again since `before` (its answer is kept as the check returns).
fn rechecked(pg: &support::Pg, before: usize) {
    let start = Instant::now();
    while checks(pg) <= before {
        assert!(start.elapsed() < Duration::from_secs(120), "never checked again");
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(200));
}

/// A database that uses a session's state comes to use none (another process drops its code); the
/// first statement after on a connection the old time used fails, which rolls its reset back with
/// it: the connection still counts as not set back, and the next statement goes after a reset too
/// (FEEDBACK75.md, Q-1060, the sixth review; branch feedback86-89).
#[test]
fn q1060_r6_a_failed_statement_does_not_count_as_set_back() {
    let Some(pg) = support::postgres("r6-fail") else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    for (name, tag, who) in [
        ("role", "begin set role boss; return current_user; end", "select current_user::text as w"),
        ("setting", "begin perform set_config('app.who', v, false); return v; end", "select coalesce(nullif(current_setting('app.who', true), ''), 'app') as w"),
    ] {
        let url = pg.database(&format!("f812_r6_fail_{}", name));
        psql(&pg, &url, "do $$ begin if not exists (select 1 from pg_roles where rolname = 'app') then create role app login; create role boss; grant boss to app; end if; end $$");
        psql(&pg, &url, &format!("alter role app set log_statement = 'all'; create function tag(v text) returns text language plpgsql as $b${}$b$; create view va as select tag('visitor-A') as r; create table z (a int); insert into z values (1); grant select on va, z to app; grant delete on z to app", tag));
        let app = format!("{}?pool_size=1", url.replace("postgres://quillo@", "postgres://app@"));
        let (s, _d) = served(&format!("q1060-r6-fail-{}", name), &app, &format!("route GET /a = db.query(\"select r from va\")\nroute GET /w = db.one(\"{}\").w\nroute GET /fail = db.query(\"select 10 / (select count(*) from z)::int as r\")\n", who));
        get(&s, "GET", "/ping");
        // The watcher's first check, before the change; each statement prepared on the one lane.
        rechecked(&pg, 0);
        assert_eq!(get(&s, "GET", "/fail").status, 200);
        get(&s, "GET", "/w");
        let before = checks(&pg);
        get(&s, "GET", "/a");
        get(&s, "GET", "/a");
        psql(&pg, &url, "drop view va; drop function tag(text); delete from z");
        rechecked(&pg, before);
        assert_eq!(get(&s, "GET", "/fail").status, 500);
        for _ in 0..3 {
            assert_eq!(get(&s, "GET", "/w").body, "app", "{}: left after a failed statement", name);
        }
    }
}

/// `db.atomic` begins on a pooled connection after the owner rule, as `db.transaction` does
/// (FEEDBACK75.md, Q-1060, the sixth review; branch feedback86-89).
#[test]
fn q1060_r6_atomic_begins_on_a_session_set_back() {
    let Some(pg) = support::postgres("r6-atomic") else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    // `db.atomic` prepares its transactions.
    psql(&pg, &pg.url, "alter system set max_prepared_transactions = 10");
    let data = std::env::temp_dir().join(format!("quillo-pg-{}-{}", "r6-atomic", std::process::id()));
    let ok = Command::new(pg.bin.join("pg_ctl")).arg("-D").arg(&data).arg("-l").arg(data.join("log")).args(["-w", "restart"]).output().unwrap();
    assert!(ok.status.success(), "{}", String::from_utf8_lossy(&ok.stderr));
    let url = pg.database("f812_r6_atomic");
    psql(&pg, &url, "create role app login; create role boss; grant boss to app; create function f() returns text language plpgsql as $b$begin set role boss; return current_user; end$b$; create view va as select f() as r; grant select on va to app");
    let app = format!("{}?pool_size=1", url.replace("postgres://quillo@", "postgres://app@"));
    let src = format!(
        "db.open(\"main.db\")\ndb.exec(\"create table if not exists t (x int)\")\nlet pg = db.connect(\"{}\")\nroute GET /a = db.transaction(() => pg.one(\"select r from va\").r)\nroute GET /atomic = db.atomic([pg], () => pg.one(\"select current_user::text as u\").u)\nroute GET /tx = db.transaction(() => pg.one(\"select current_user::text as u\").u)\n",
        app
    );
    let d = dir("q1060-r6-atomic", &src);
    let s = support::serve(&d.join("main.qwe"), &[]);
    for _ in 0..3 {
        get(&s, "GET", "/a");
        assert_eq!(get(&s, "GET", "/atomic").body, "app");
        get(&s, "GET", "/a");
        assert_eq!(get(&s, "GET", "/tx").body, "app");
    }
}

/// `setseed` is refused everywhere, a transaction too: the seed outlives it, and `reset all`
/// leaves it (FEEDBACK75.md, Q-1060, the sixth review; branch feedback86-89).
#[test]
fn q1060_r6_setseed_is_refused_in_a_transaction_too() {
    let Some(pg) = support::postgres("r6-seed") else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    let url = pg.database("f812_r6_seed");
    let (s, _d) = served("q1060-r6-seed", &url, "route GET /seed = db.transaction(() => db.one(\"select setseed(0.5)::text as v\").v)\n");
    let r = get(&s, "GET", "/seed");
    assert_ne!(r.status, 200, "{}", r.body);
}

/// A built-in on the safe list a superuser replaced in place with PL/pgSQL (its oid kept) is no
/// longer safe (FEEDBACK75.md, Q-1060, the sixth review; branch feedback86-89).
#[test]
fn q1060_r6_a_replaced_built_in() {
    bypass("builtin", "create or replace function pg_catalog.round(numeric) returns numeric language plpgsql as $b$begin set role boss; return 1; end$b$; create view v as select pg_catalog.round(1.5::numeric)::text as r");
}

/// A statement that cannot run in a transaction block, sent by `db.query` on a database that uses a
/// session's state, runs apart, as `db.exec`'s (FEEDBACK75.md, Q-1060, the sixth review; branch
/// feedback86-89).
#[test]
fn q1060_r6_vacuum_by_query_on_a_session_database() {
    let Some(pg) = support::postgres("r6-vac") else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    let url = pg.database("f812_r6_vac");
    psql(&pg, &url, "create table z (a int); create function who() returns text language plpgsql as $b$begin return current_setting('app.x', true); end$b$");
    let (s, _d) = served("q1060-r6-vac", &url, "route GET /q = db.query(\"vacuum z\")\nroute GET /e = db.exec(\"vacuum z\").changes\n");
    let r = get(&s, "GET", "/q");
    assert_eq!(r.status, 200, "{}", r.body);
    let r = get(&s, "GET", "/e");
    assert_eq!(r.status, 200, "{}", r.body);
}

/// The watcher's connection stops answering (its server process stopped, as a dropped network
/// looks): after `STALE_MS` with no answer nothing is known, and statements are set back, so code
/// another process makes meanwhile leaves nothing (FEEDBACK75.md, Q-1060, the sixth review; branch
/// feedback86-89).
#[test]
fn q1060_r6_a_watcher_that_stops_answering() {
    let Some(pg) = support::postgres("r6-stuck") else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    let url = pg.database("f812_r6_stuck");
    psql(&pg, &url, "create role app login; create role boss; grant boss to app; create table z (a int)");
    let app = format!("{}?pool_size=1", url.replace("postgres://quillo@", "postgres://app@"));
    let (s, _d) = served("q1060-r6-stuck", &app, "route GET /a = db.query(\"select r from va\")\n");
    get(&s, "GET", "/ping");
    let mut pid = String::new();
    for _ in 0..500 {
        let out = Command::new(pg.bin.join("psql")).args([url.as_str(), "-qAt", "-c", "select pid from pg_stat_activity where datname = current_database() and query like '%concat_ws%' and pid <> pg_backend_pid() limit 1"]).output().unwrap();
        pid = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !pid.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!pid.is_empty(), "no watcher");
    Command::new("kill").args(["-STOP", &pid]).status().unwrap();
    psql(&pg, &url, "create function f() returns text language plpgsql as $b$begin set role boss; return current_user; end$b$; create view va as select f() as r; grant select on va to app");
    let start = Instant::now();
    let mut clean_rounds = 0;
    while clean_rounds < 3 {
        get(&s, "GET", "/a");
        let who: Vec<String> = (0..4).map(|_| get(&s, "GET", "/role").body).collect();
        clean_rounds = if who.iter().all(|w| w == "app") { clean_rounds + 1 } else { 0 };
        assert!(start.elapsed() < Duration::from_millis(quillo::pg::STALE_MS * 6), "still trusted the stuck watcher");
    }
    Command::new("kill").args(["-CONT", &pid]).status().unwrap();
}

/// On a database that uses no session state, a transaction on a pooled connection someone else's
/// statement ran on last begins with its sequences' last values let go: its first `lastval()` is
/// not defined, never another request's id; `currval` after its own `nextval` reads its own; and
/// an isolation level still holds (FEEDBACK75.md, Q-1060, reviewS1f's f3; branch feedback86-89).
#[test]
fn q1060_r6_a_transactions_first_lastval_is_its_own() {
    let Some(pg) = support::postgres("r6-lastval") else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    let url = pg.database("f812_r6_lastval");
    psql(&pg, &url, "create table orders (id serial primary key, who text); alter database f812_r6_lastval set log_statement = 'all'");
    let src = format!(
        "db.open(\"main.db\")\ndb.exec(\"create table if not exists t (x int)\")\nlet pg = db.connect(\"{}?pool_size=1\")\nroute GET /a = db.transaction(() => pg.exec(\"insert into orders (who) values ('A')\").changes)\nroute GET /b = pg.transaction(() => pg.one(\"select lastval() as v\").v)\nroute GET /c = pg.transaction(() => {{\n    pg.exec(\"insert into orders (who) values ('C')\")\n    return pg.one(\"select currval('orders_id_seq') = (select max(id) from orders) as v\").v\n}})\nroute GET /iso = pg.transaction({{ isolation: \"serializable\" }}, () => pg.one(\"select current_setting('transaction_isolation') as v\").v)\n",
        url
    );
    let d = dir("q1060-r6-lastval", &src);
    let s = support::serve(&d.join("main.qwe"), &[]);
    // Known to use no session state first: before that, every connection is set back whole.
    get(&s, "GET", "/a");
    get(&s, "GET", "/a");
    rechecked(&pg, 0);
    for _ in 0..3 {
        assert_eq!(get(&s, "GET", "/a").body, "1");
        let b = get(&s, "GET", "/b");
        assert_ne!(b.status, 200, "a transaction's first lastval() read {}", b.body);
        assert_eq!(get(&s, "GET", "/c").body, "true");
        assert_eq!(get(&s, "GET", "/a").body, "1");
        assert_eq!(get(&s, "GET", "/iso").body, "serializable");
    }
}
