//! `db.migrate`: a folder of SQL files, each applied once, in order.

use quillo::Interp;
use std::path::PathBuf;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("quillo-migrate-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("migrations")).unwrap();
    d
}

fn load(d: &PathBuf, src: &str) -> Result<String, String> {
    let mut it = Interp::new(d);
    it.load_source(src, "app.qwe").map(|v| v.repr()).map_err(|e| e.to_string())
}

#[test]
fn migrations_apply_once_in_order_and_a_failing_one_changes_nothing() {
    let d = dir("sqlite");
    let m = d.join("migrations");
    std::fs::write(m.join("001_todos.sql"), "create table todos (id integer primary key, text text not null);").unwrap();
    std::fs::write(m.join("002_done.sql"), "alter table todos add column done integer not null default 0;\ninsert into todos (text) values ('first');").unwrap();
    std::fs::write(m.join("notes.txt"), "not SQL").unwrap();
    let src = "db.open(\"app.db\")\ndb.migrate(\"migrations\")";
    assert_eq!(load(&d, src).unwrap(), "[\"001_todos.sql\", \"002_done.sql\"]");
    // Again: nothing new, and the data is still there.
    assert_eq!(load(&d, src).unwrap(), "[]");
    let rows = "db.open(\"app.db\")\ndb.migrate()\n[db.query(\"select text, done from todos\"), db.query(\"select name from _quillo_migrations order by name\") | map((r) => r.name)]";
    assert_eq!(load(&d, rows).unwrap(), "[[{ text: \"first\", done: 0 }], [\"001_todos.sql\", \"002_done.sql\"]]");
    // A file that fails is rolled back and stops the program loading.
    std::fs::write(m.join("003_bad.sql"), "create table tags (id integer primary key);\ninsert into nowhere values (1);").unwrap();
    let e = load(&d, src).unwrap_err();
    assert!(e.contains("003_bad.sql") && e.contains("nowhere"), "{}", e);
    let after = "db.open(\"app.db\")\n[db.query(\"select name from sqlite_master where name = 'tags'\"), len(db.query(\"select * from _quillo_migrations\"))]";
    assert_eq!(load(&d, after).unwrap(), "[[], 2]");
    let _ = std::fs::remove_dir_all(&d);
}
