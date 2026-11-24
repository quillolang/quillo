//! Coverage of the lines of src/head.rs and src/db.rs that the full run at 8b84078 left unrun, or
//! that the speed round 2 of 3117dea added (coverage of feedback82-85; branch feedback82-85):
//! `page_head`'s reading of a head as the browser's tokenizer reads it (FEEDBACK84.md, Q-1320): the
//! four named references the standard reads without their `;` in an attribute's value, and a tag
//! whose attribute's name is followed by spaces and then its `>` or a `/`; and `conn_of`, which
//! tells a connection's map, looked for its "$db" first (FEEDBACK83.md, Q-1300). Each page's head
//! as parse5 7.3.0 reads it (scripting on and off) was checked to hold what the test says.

mod support;

use quillo::head::{apply, Head, KEYS};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

// ---- page_head, as tests/cover8285_head5.rs reads a head ------------------------------------------

/// A head that says `pairs`, each at depth 0 in no component, in their order.
fn said_head(pairs: &[(&str, &str)]) -> Head {
    let mut h = Head::default();
    for (k, v) in pairs {
        h.say(KEYS.iter().position(|x| x == k).unwrap(), 0, 0, v.to_string());
        h.called();
    }
    h
}

/// `page` with `h`'s tags in its head.
fn with_head(page: &str, h: &Head) -> String {
    let mut b = page.as_bytes().to_vec();
    apply(&mut b, h);
    String::from_utf8(b).unwrap()
}

/// A document whose head is `head`.
fn doc(head: &str) -> String {
    format!("<!doctype html><html><head>{head}</head><body>b</body></html>")
}

/// What `page_head({ title: "T" })` writes.
const TT: &str = "<title>T</title><meta property=\"og:title\" content=\"T\">";

/// What `page_head({ description: "D" })` writes.
const DD: &str = "<meta name=\"description\" content=\"D\"><meta property=\"og:description\" content=\"D\">";

/// What `page_head({ canonical: "https://x.example/a" })` writes.
const CC: &str = "<link rel=\"canonical\" href=\"https://x.example/a\"><meta property=\"og:url\" content=\"https://x.example/a\">";

/// `&amp`, `&lt`, `&gt` and `&quot` (and their capitals) without their `;`, in a meta's name or
/// property, are the character they name where no letter, digit or `=` follows, as the browser
/// reads them: `description&amp` is `description&`, `description&lt` (at the value's end)
/// `description<`, `og:description&QUOT` `og:description"`, none a description, so each is left
/// beside `page_head`'s. Where a letter or `=` follows they are as written (`&ampdescription`,
/// `&amp=description`, `descr&GTiption`), no description either. A reference read as nothing
/// would have cut the first three (src/head.rs char_ref, FEEDBACK84.md Q-1320; coverage of
/// feedback82-85; branch feedback82-85).
#[test]
fn a_named_reference_without_its_semicolon_is_read_as_the_browser_reads_it_in_a_metas_name() {
    let h = said_head(&[("description", "D")]);
    for head in [
        "<meta name=\"description&amp\" content=\"K\">",
        "<meta name=\"description&lt\" content=\"K\">",
        "<meta property=\"og:description&QUOT\" content=\"K\">",
        "<meta name=\"description&GT \" content=\"K\">",
        "<meta name=\"&ampdescription\" content=\"K\">",
        "<meta name=\"&amp=description\" content=\"K\">",
        "<meta name=\"descr&GTiption\" content=\"K\">",
    ] {
        assert_eq!(with_head(&doc(head), &h), doc(&format!("{head}{DD}")), "{head}");
    }
    // The same references with their `;` are what they were (`ASCII_REFS`): no description either.
    let head = "<meta name=\"description&amp;\" content=\"K\">";
    assert_eq!(with_head(&doc(head), &h), doc(&format!("{head}{DD}")));
}

/// In a link's `rel`, such a reference is a token of its own where a space or a tab follows it,
/// by reference or written (`&amp canonical`, `&LT&#9;canonical`, `&quot&#32;canonical`, `canonical
/// &lt`): the link is a canonical, and is cut. Followed by a letter or `=` (`&ampcanonical`,
/// `&AMP=canonical`), or ending the token (`canonical&gt`), it is part of a token that is no
/// `canonical`, and the link is left (src/head.rs char_ref and has_token, FEEDBACK84.md Q-1320;
/// coverage of feedback82-85; branch feedback82-85).
#[test]
fn a_named_reference_without_its_semicolon_splits_a_rel_only_where_a_space_follows() {
    let h = said_head(&[("canonical", "https://x.example/a")]);
    for head in [
        "<link rel=\"&amp canonical\" href=\"/old\">",
        "<link rel=\"&LT&#9;canonical\" href=\"/old\">",
        "<link rel=\"&quot&#32;canonical\" href=\"/old\">",
        "<link rel=\"canonical &lt\" href=\"/old\">",
    ] {
        assert_eq!(with_head(&doc(head), &h), doc(CC), "{head}");
    }
    for head in ["<link rel=\"&ampcanonical\" href=\"/k\">", "<link rel=\"&AMP=canonical\" href=\"/k\">", "<link rel=\"canonical&gt\" href=\"/k\">"] {
        assert_eq!(with_head(&doc(head), &h), doc(&format!("{head}{CC}")), "{head}");
    }
}

/// A tag whose last attribute's name is followed by spaces and then its `>` (`async >`) or a `/`
/// (`async />`) ends there, in a start tag and in an end tag (`</title lang >`): the meta and the
/// title are cut whole, and nothing after them is (src/head.rs tag_close, FEEDBACK84.md Q-1320;
/// coverage of feedback82-85; branch feedback82-85).
#[test]
fn a_tag_whose_attributes_name_is_followed_by_spaces_ends_at_its_own_close() {
    let h = said_head(&[("title", "T"), ("description", "D")]);
    for head in [
        "<meta name=description content=OLD async >",
        "<meta name=description content=OLD async \t/>",
        "<meta name=description content=OLD async / >",
        "<title lang >Old</title lang >",
        "<title lang >Old</title><meta name=description content=OLD async >",
    ] {
        assert_eq!(with_head(&doc(&format!("{head}<link rel=icon href=/i.png>")), &h), doc(&format!("<link rel=icon href=/i.png>{TT}{DD}")), "{head}");
    }
}

/// In a template of the head, `<svg x />` (its attribute's name, a space, then `/>`) is a whole
/// svg, as the browser closes it, so the `<title>` after it is the page's own, whose text holds
/// the `</template>` in it, and the template ends at the second: the head's title after it is cut.
/// `<svg x >` and `<svg x / >` (a space between `/` and `>`) leave the svg open: the `<title>` is
/// the svg's, the first `</template>` ends the template, the `<p>` after it ends the head, and the
/// tags go before that `<p>`, the title after it left as the body's (src/head.rs tag_close and walk,
/// FEEDBACK84.md Q-1320; coverage of feedback82-85; branch feedback82-85).
#[test]
fn an_svgs_self_closing_tag_after_an_attributes_name_is_read_as_the_browser_reads_it() {
    let h = said_head(&[("title", "T")]);
    let rest = "<title></template><p>t</p></title></template><title>Old</title>";
    let closed = format!("<template><svg x />{rest}");
    assert_eq!(with_head(&doc(&closed), &h), doc(&format!("<template><svg x /><title></template><p>t</p></title></template>{TT}")));
    for open in ["<svg x >", "<svg x / >"] {
        let head = format!("<template>{open}{rest}");
        assert_eq!(
            with_head(&doc(&head), &h),
            format!("<!doctype html><html><head><template>{open}<title></template>{TT}<p>t</p></title></template><title>Old</title></head><body>b</body></html>"),
            "{open}"
        );
    }
}

// ---- conn_of: a map called as a connection -----------------------------------------------------

/// A test's folder, removed when the test ends.
struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `quillo run main.qwe` in `d`, ended after a minute: its exit code and what it wrote.
fn run(d: &Path) -> (Option<i32>, String) {
    let log = d.join("cmd.log");
    let file = std::fs::File::create(&log).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", "main.qwe"])
        .current_dir(d)
        .env_remove("QUILLO_POLICY_PATH")
        .env_remove("QUILLO_DB")
        .env_remove("CARGO_TARGET_DIR")
        .stdin(Stdio::null())
        .stdout(file.try_clone().unwrap())
        .stderr(file)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let code = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s.code();
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    (code, std::fs::read_to_string(&log).unwrap_or_default())
}

/// A method called on a map is a connection's where the map has a "$db" that is an int and the
/// name is one of `db`'s builtins: it runs on that connection, the one the number says (`a` or
/// `b`), whatever map it is in; one the program has not opened (-1, 2, the last int) is refused.
/// A map with no "$db", or one that is no int (`"1"`, `1.0`), is no connection: `db.one` runs on
/// the program's own database, which has no `t`. A function of the map's own, or a builtin not of
/// `db`'s, is called as it is (src/db.rs conn_of, the speed round 2 of 0.8.11, FEEDBACK83.md
/// Q-1300; coverage of feedback82-85; branch feedback82-85).
#[test]
fn a_map_is_called_as_a_connection_only_where_its_db_is_an_int_and_the_name_is_dbs() {
    let d = Dir(support::temp_dir("cov8285r3-conn"));
    std::fs::create_dir_all(d.0.join("data")).unwrap();
    std::fs::write(
        d.0.join("main.qwe"),
        r#"let a = db.connect("data/a.db")
let b = db.connect("data/b.db")
a.exec("create table t (x int)")
a.exec("insert into t values (7)")
b.exec("create table t (x int)")
b.exec("insert into t values (8)")
print(a["$db"], b["$db"])
print(a.one("select x from t"), b.one("select x from t"))
let on_b = { "$db": b["$db"], one: db.one, query: db.query }
print(on_b.one("select x from t"), on_b.query("select x + 1 as y from t"))
let on_a = { "$db": a["$db"], one: db.one }
print(on_a.one("select x from t"))
print({ "$db": "1", one: db.one }.one("select x from t"))
print({ "$db": 1.0, one: db.one }.one("select x from t"))
print({ one: db.one }.one("select x from t"))
print({ "$db": -1, one: db.one }.one("select x from t"))
print({ "$db": 2, one: db.one }.one("select x from t"))
print({ "$db": 9223372036854775807, one: db.one }.one("select x from t"))
print({ "$db": b["$db"], one: (sql) => "its own: " + sql }.one("select x from t"))
print({ "$db": b["$db"], one: upper }.one("select x from t"))
"#,
    )
    .unwrap();
    let (code, out) = run(&d.0);
    assert_eq!(code, Some(0), "{out}");
    assert_eq!(
        out,
        "0 1\n{ x: 7 } { x: 8 }\n{ x: 8 } [{ y: 9 }]\n{ x: 7 }\n\
         err(\"db: no such table: t\")\nerr(\"db: no such table: t\")\nerr(\"db: no such table: t\")\n\
         err(\"db: this connection is not one of this program's\")\nerr(\"db: this connection is not one of this program's\")\nerr(\"db: this connection is not one of this program's\")\n\
         its own: select x from t\nSELECT X FROM T\n"
    );
}
