//! `db` on a real MariaDB server, past the usual path (src/my.rs, src/db.rs):
//! the URL's options, every type there is, placeholders in all the places
//! they can hide, and Quillo's own tables there. Skipped when MariaDB's
//! server programs are not installed.

mod support;

use quillo::my::Pool;
use quillo::Interp;
use std::path::Path;
use std::sync::{Arc, Mutex, Weak};
use support::Maria;

/// The server, shared by the tests running at the same time (each with a
/// database of its own): starting many at once fails now and then, for the
/// load. It stops with the last test using it.
fn server() -> Option<Arc<Maria>> {
    static SHARED: Mutex<(Weak<Maria>, u32)> = Mutex::new((Weak::new(), 0));
    let mut g = SHARED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(s) = g.0.upgrade() {
        return Some(s);
    }
    g.1 += 1;
    let Some(s) = support::mariadb(&format!("dbmariadb{}", g.1)) else {
        eprintln!("skipping: no MariaDB server (mariadbd, mariadb-install-db) was found");
        return None;
    };
    let s = Arc::new(s);
    g.0 = Arc::downgrade(&s);
    Some(s)
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

fn program(name: &str, url: &str, more: &str) -> Interp {
    let d = support::temp_dir(&format!("dbmy-{}", name));
    load(&d, &format!("db.open(\"{}\")\n{}", url, more))
}

fn v(it: &mut Interp, select: &str) -> String {
    eval(it, &format!("db.one(\"select {} as v\").v", select))
}

#[test]
fn urls_say_which_databases_are_mysql() {
    assert!(quillo::my::is_url("mysql://h/app") && quillo::my::is_url("mariadb://h/app"));
    assert!(!quillo::my::is_url("postgres://h/app") && !quillo::my::is_url("app.db"));
}

#[test]
fn a_urls_tls_options_are_refused_before_connecting() {
    let open = |q: &str| Pool::open_as(&format!("mysql://root@127.0.0.1:1/x?{}", q), false).err().unwrap_or_default();
    says(open("verify_identity=false&require_ssl=true"), "verify_identity=false is not supported");
    says(open("require_ssl=true&ssl_cert=/c.pem"), "ssl_cert and ssl_key go together");
    says(open("require_ssl=true&ssl_key=/k.pem"), "ssl_cert and ssl_key go together");
    says(open("require_ssl=maybe"), "require_ssl is true or false, not \"maybe\"");
    says(open("verify_ca=maybe"), "verify_ca is true or false");
    says(open("built_in_roots=maybe"), "built_in_roots is true or false");
    assert!(Pool::open_as("mysql://[bad/x", false).is_err());
}

#[test]
fn a_certificate_of_ones_own_is_presented_to_the_server() {
    let d = support::temp_dir("dbmy-client-cert");
    let key = rcgen::KeyPair::generate().unwrap();
    let cert = rcgen::CertificateParams::new(vec!["client".to_string()]).unwrap().self_signed(&key).unwrap();
    std::fs::write(d.join("c.pem"), cert.pem()).unwrap();
    std::fs::write(d.join("k.pem"), key.serialize_pem()).unwrap();
    let Some(s) = server() else { return };
    // This server has no TLS: the options are read, and it says so.
    let url = format!("{}?require_ssl=true&ssl_cert={}&ssl_key={}&built_in_roots=true", s.url("mysql"), d.join("c.pem").display(), d.join("k.pem").display());
    assert!(!Pool::open_as(&url, false).err().unwrap_or_default().is_empty());
}

#[test]
fn every_way_of_naming_a_server_reaches_it() {
    let Some(s) = server() else { return };
    // `mariadb://` is the same protocol; options the driver knows pass through.
    let mut it = program("urls1", &s.url("mysql").replace("mysql://", "mariadb://"), "");
    assert_eq!(v(&mut it, "1"), "1");
    let mut it = program("urls2", &format!("{}?pool_min=1&pool_max=2", s.url("mysql")), "");
    assert_eq!(v(&mut it, "1"), "1");
    // Without a database in the URL, there is nothing to make.
    let mut it = program("urls3", &s.url(""), "");
    assert_eq!(v(&mut it, "1"), "1");
    assert_eq!(eval(&mut it, "db.one(\"select database() as d\").d"), "null");
    // A database not there is made; not made, it is an error.
    assert!(Pool::open_as(&s.url("not_there"), false).is_err());
    assert!(Pool::open_as(&s.url("made_now"), true).is_ok());
    assert!(Pool::open_as(&s.url("made_now"), false).unwrap().databases().unwrap().iter().any(|d| d == "made_now"));
}

#[test]
fn every_type_comes_back_as_a_value() {
    let Some(s) = server() else { return };
    let mut it = program("types", &s.url("types"), "");
    // Unsigned, and past what an int holds.
    assert_eq!(v(&mut it, "cast(5 as unsigned)"), "5");
    assert_eq!(v(&mut it, "cast(18446744073709551615 as unsigned)"), "18446744073709552000");
    // Dates and times, with and without their fractions.
    assert_eq!(v(&mut it, "cast('2024-01-02' as date)"), "\"2024-01-02\"");
    assert_eq!(v(&mut it, "cast('2024-01-02 03:04:05' as datetime)"), "\"2024-01-02 03:04:05\"");
    assert_eq!(v(&mut it, "cast('2024-01-02 03:04:05.5' as datetime(6))"), "\"2024-01-02 03:04:05.500000\"");
    assert_eq!(v(&mut it, "cast('03:04:05' as time)"), "\"03:04:05\"");
    assert_eq!(v(&mut it, "cast('03:04:05.25' as time(6))"), "\"03:04:05.250000\"");
    assert_eq!(v(&mut it, "cast('-03:04:05' as time)"), "\"-03:04:05\"");
    assert_eq!(v(&mut it, "cast('30:00:00' as time)"), "\"30:00:00\"");
    assert_eq!(v(&mut it, "cast('-100:00:00' as time)"), "\"-100:00:00\"");
    // Decimals, whole and not, and past what a decimal holds.
    assert_eq!(v(&mut it, "cast(7 as decimal(10,0))"), "7");
    assert_eq!(v(&mut it, "cast(1.50 as decimal(10,2))"), "1.50m");
    assert_eq!(v(&mut it, "cast(12345678901234567890123456789012345678 as decimal(38,0))"), "\"12345678901234567890123456789012345678\"");
    // A tinyint(1) is a boolean, other tinyints are not.
    eval(&mut it, "db.exec(\"create table b (f tinyint(1), n tinyint(2), r float, j json)\")");
    eval(&mut it, "db.exec(\"insert into b values (1, 1, 1.5, '{}')\")");
    assert_eq!(eval(&mut it, "db.one(\"select f, n, r from b\")"), "{ f: true, n: 1, r: 1.5 }");
    // MariaDB's JSON is text.
    assert_eq!(eval(&mut it, "db.one(\"select j from b\").j"), "\"{}\"");
}

#[test]
fn values_are_sent_as_text_or_refused() {
    let Some(s) = server() else { return };
    let mut it = program("values", &s.url("values"), "db.exec(\"create table t (v text)\")\n");
    let round = |it: &mut Interp, value: &str| {
        eval(it, &format!("db.exec(\"delete from t\")\ndb.exec(\"insert into t values (?)\", {})\ndb.one(\"select v from t\").v", value))
    };
    assert_eq!(round(&mut it, "raw(\"<b>x</b>\")"), "\"<b>x</b>\"");
    assert_eq!(round(&mut it, "[1, null]"), "\"[1,null]\"");
    assert_eq!(round(&mut it, "1.50m"), "\"1.50\"");
    says(eval(&mut it, "db.exec(\"insert into t values (?)\", () => 1)"), "a fn cannot be stored in the database");
    says(eval(&mut it, "db.exec(\"insert into t values (:a)\", { a: () => 1 })"), "a fn cannot be stored in the database");
}

#[test]
fn placeholders_are_found_only_where_they_are_placeholders() {
    let Some(s) = server() else { return };
    let mut it = program("holders", &s.url("holders"), "");
    let named = |it: &mut Interp, sql: &str| eval(it, &format!("let r = db.one(\"{}\", {{ a: 1 }})\nis_err(r) ? r : r.a", sql));
    assert_eq!(named(&mut it, "select ':no' as q, :a as a"), "1");
    assert_eq!(named(&mut it, "select 'it\\\\'s :no' as q, :a as a"), "1");
    assert_eq!(named(&mut it, "select 1 as `:no`, :a as a"), "1");
    assert_eq!(named(&mut it, "select -- :no\\n :a as a"), "1");
    assert_eq!(named(&mut it, "select # :no\\n :a as a"), "1");
    assert_eq!(named(&mut it, "select /* :no */ :a as a"), "1");
    // Comments that run to the end.
    assert_eq!(named(&mut it, "select :a as a -- :no"), "1");
    assert_eq!(named(&mut it, "select :a as a # :no"), "1");
    says(named(&mut it, "select :a as a /* :no"), "");
    says(named(&mut it, "select :b as a"), "no value for :b");
}

#[test]
fn errors_mariadb_gives_have_kinds() {
    let Some(s) = server() else { return };
    let src = "db.exec(\"create table p (id int primary key, n int not null, c int check (c > 0))\")\n\
               db.exec(\"create table k (p int, foreign key (p) references p (id))\")\n\
               db.exec(\"insert into p values (1, 1, 1)\")\n";
    let mut it = program("kinds", &s.url("kinds"), src);
    let kind = |it: &mut Interp, sql: &str| eval(it, &format!("db.error_kind(db.exec(\"{}\"))", sql));
    assert_eq!(kind(&mut it, "insert into p values (1, 1, 1)"), "\"unique\"");
    assert_eq!(kind(&mut it, "insert into p values (2, null, 1)"), "\"not_null\"");
    assert_eq!(kind(&mut it, "insert into k values (9)"), "\"foreign_key\"");
    assert_eq!(kind(&mut it, "insert into nowhere values (1)"), "null");
}

#[test]
fn a_failed_check_on_mariadb_is_a_check() {
    let Some(s) = server() else { return };
    let mut it = program("check", &s.url("check"), "db.exec(\"create table p (c int check (c > 0))\")\n");
    assert_eq!(eval(&mut it, "db.error_kind(db.exec(\"insert into p values (0)\"))"), "\"check\"");
}

#[test]
fn a_row_changed_since_the_snapshot_is_a_conflict() {
    let Some(s) = server() else { return };
    let src = format!("let other = db.connect(\"{}?pool_min=1&pool_max=9\")\ndb.exec(\"create table t (n int primary key)\")\ndb.exec(\"insert into t values (1)\")\n", s.url("snapshot"));
    let mut it = program("snapshot", &s.url("snapshot"), &src);
    let r = eval(
        &mut it,
        "db.transaction({ isolation: \"repeatable_read\" }, () => {\n    db.one(\"select n from t\")\n    other.exec(\"update t set n = 2\")\n    err(db.error_kind(db.exec(\"update t set n = 3\")))\n})",
    );
    assert_eq!(r, "err(\"conflict\")");
}

#[test]
fn sql_name_quotes_as_mariadb_reads_names() {
    let Some(s) = server() else { return };
    let mut it = program("names", &s.url("names"), "");
    assert_eq!(eval(&mut it, "sql_name(\"a\")"), "\"`a`\"");
    let mut it = program("names2", &s.url("names_{tenant}"), "");
    assert_eq!(eval(&mut it, "sql_name(\"a\")"), "\"`a`\"");
    // A function of the tenant: as the first tenant's database is.
    let url = s.url("names_acme");
    let d = support::temp_dir("dbmy-names3");
    let mut it = load(&d, &format!("db.open((t) => \"{}\")\n", url));
    eval(&mut it, "with_tenant(\"acme\", () => db.query(\"select 1\"))");
    assert_eq!(eval(&mut it, "sql_name(\"a\")"), "\"`a`\"");
}

#[test]
fn tenants_of_a_server_that_cannot_be_reached_are_an_error() {
    let d = support::temp_dir("dbmy-noserver");
    let mut it = load(&d, &format!("db.open(\"mysql://root@127.0.0.1:{}/app_{{tenant}}\")\n", support::free_port()));
    assert!(eval(&mut it, "db.tenants()").starts_with("err(\"db:"));
    assert_eq!(eval(&mut it, "db.tenant_exists(\"acme\")"), "false");
}

// ---- Quillo's own tables ----------------------------------------------------------------------

#[test]
fn a_jobs_run_is_claimed_once() {
    let Some(s) = server() else { return };
    let src = "db.exec(\"create table ran (n int)\")\nfn work() = db.exec(\"insert into ran values (1)\")\nschedule(\"1h\", work)\n";
    let mut it = program("jobs", &s.url("jobs"), src);
    let job = |it: &mut Interp, at: u64| {
        let headers = vec![("Host".to_string(), "localhost".to_string()), ("X-Quillo-Job".to_string(), quillo::jobs::token().to_string())];
        let r = it.handle(&quillo::server::HttpRequest { method: "POST".into(), url: format!("/_quillo/job/work?at={}", at), headers, body: Vec::new(), remote: "127.0.0.1".into() });
        String::from_utf8_lossy(&r.body).into_owned()
    };
    assert_eq!(job(&mut it, 100), "ok\n");
    assert_eq!(job(&mut it, 100), "run by another server\n");
    assert_eq!(job(&mut it, 200), "ok\n");
    assert_eq!(eval(&mut it, "len(db.query(\"select * from ran\"))"), "2");
}

#[test]
fn calls_made_later_are_kept_on_mariadb() {
    let Some(s) = server() else { return };
    let d = support::temp_dir("dbmy-later");
    std::fs::write(d.join("mailer.qwe"), "server fn send(to: str) = to\n").unwrap();
    let app = d.join("app.qwe");
    std::fs::write(&app, format!("db.open(\"{}\")\nservice mailer = \"mailer.qwe\"\nroute GET /send/:to = mailer.later.send(to)\nroute GET /kept = str(db.one(\"select count(*) as n from _quillo_later\").n)\n", s.url("later"))).unwrap();
    let nobody = format!("http://127.0.0.1:{}", support::free_port());
    let srv = support::serve(&app, &[("QUILLO_SERVICE_MAILER", &nobody), ("QUILLO_SERVICE_SECRET", "a secret")]);
    assert_eq!(support::send(srv.port, "GET", "/send/ana", &[], "").status, 200);
    assert_eq!(support::send(srv.port, "GET", "/kept", &[], "").body, "1");
}

#[test]
fn a_call_sent_again_to_another_copy_is_answered_as_it_was() {
    let Some(s) = server() else { return };
    let d = support::temp_dir("dbmy-calls");
    let app = d.join("ledger.qwe");
    std::fs::write(&app, format!("db.open(\"{}\")\ndb.exec(\"create table if not exists entries (n int)\")\nserver fn add(n: int) {{\n    db.exec(\"insert into entries values (?)\", n)\n    n * 2\n}}\nroute GET /n = str(db.one(\"select count(*) as n from entries\").n)\n", s.url("calls"))).unwrap();
    let secret = "a secret for these tests";
    let (a, b) = (support::serve(&app, &[("QUILLO_SERVICE_SECRET", secret)]), support::serve(&app, &[("QUILLO_SERVICE_SECRET", secret)]));
    let call = |port: u16| {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        let sig = quillo::service::signature(secret, t, "ledger", "add", "", "call1", b"[21]");
        let signed = format!("t={},sig={}", t, sig);
        support::send(port, "POST", "/_quillo/service/add", &[("x-quillo-service", signed.as_str()), ("x-quillo-to", "ledger"), ("x-quillo-call", "call1")], "[21]")
    };
    assert_eq!(call(a.port).body, "{\"ok\":42}");
    assert_eq!(call(b.port).body, "{\"ok\":42}");
    assert_eq!(support::send(a.port, "GET", "/n", &[], "").body, "1");
}

#[test]
fn recovery_finishes_what_was_left_as_it_was_decided() {
    let Some(s) = server() else { return };
    let src = "db.exec(\"create table t (x text)\")\ndb.exec(\"insert into t values ('first')\")\n\
               db.exec(\"create table _quillo_atomic (gid varchar(64) primary key, decision varchar(8) not null, at bigint not null)\")\n";
    let mut it = program("recover", &s.url("recover"), src);
    let prepare = |gid: &str| {
        let pool = Pool::open_as(&s.url("recover"), false).unwrap();
        let mut c = pool.checkout().unwrap();
        c.batch(&format!("xa start '{}'", gid)).unwrap();
        c.batch(&format!("insert into t values ('{}')", gid)).unwrap();
        c.batch(&format!("xa end '{}'", gid)).unwrap();
        c.batch(&format!("xa prepare '{}'", gid)).unwrap();
        pool.give_back(c);
    };
    prepare("quillo_1000_kept_0");
    eval(&mut it, "db.exec(\"insert into _quillo_atomic values ('quillo_1000_kept', 'commit', 1)\")");
    prepare("quillo_1000_lost_0");
    quillo::db::recover(&mut it);
    assert_eq!(eval(&mut it, "db.query(\"select x from t\") | map((r) => r.x)"), "[\"first\", \"quillo_1000_kept_0\"]");
    assert_eq!(eval(&mut it, "len(db.query(\"xa recover\"))"), "0");
    assert_eq!(eval(&mut it, "db.one(\"select decision from _quillo_atomic where gid = 'quillo_1000_lost'\").decision"), "\"abort\"");
}
