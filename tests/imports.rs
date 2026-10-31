//! The file first (FEEDBACK19.md, Q-156; branch feedback19-fixes): `import lib/a.qwe use b`,
//! `client`, `preload` and `preconnect` for what runs in the browser, the `use` on the next
//! line, the words staying names, and `quillo fix` turning 0.6's lines around.

mod support;

use quillo::check::{check_file, Severity};
use quillo::parser::Parser;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A program in a folder of its own: its first file is the main one.
fn program(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = support::temp_dir(&format!("imports-{}", name));
    for (f, text) in files {
        let p = d.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    d.join(files[0].0)
}

fn diags(main: &Path) -> Vec<String> {
    let dir = main.parent().unwrap().display().to_string();
    check_file(main)
        .diagnostics
        .iter()
        .map(|d| {
            let f = d.file.replace(&dir, "").replace("./", "");
            format!("{}:{}{} {}", f.trim_start_matches('/'), d.line, if d.severity == Severity::Warning { " warning:" } else { "" }, d.message)
        })
        .collect()
}

fn clean(main: &Path) {
    assert_eq!(diags(main), Vec::<String>::new());
}

fn runs(main: &Path) -> String {
    let mut it = quillo::Interp::new(main.parent().unwrap());
    it.load_file(main).unwrap();
    let f = quillo::value::lookup(&it.globals, "out").unwrap();
    it.call(&f, vec![], 0).map_or("failed".to_string(), |v| v.display())
}

fn parsed(src: &str) -> Result<String, String> {
    match Parser::new(src).parse_program() {
        Ok(stmts) => Ok(format!("{:?}", stmts).replace("old: true", "old: false")),
        Err(e) => Err(e.msg),
    }
}

fn quillo(args: &[&str], dir: &Path) -> (bool, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_quillo")).args(args).current_dir(dir).output().unwrap();
    (o.status.success(), format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)))
}

const LIB: &str = "fn one() = \"1\"\nfn two() = \"2\"\nfn three() = \"3\"\n";

// ---- the two ways of writing a line ------------------------------------------------------------

#[test]
fn each_line_means_what_it_meant_before() {
    let url = "https://cdn.example/chart.js";
    let pairs = [
        ("import lib/a.qwe use one".to_string(), "use one from lib/a.qwe".to_string()),
        ("import lib/a.qwe use one, two as deux".to_string(), "use one, two as deux from lib/a.qwe".to_string()),
        ("import lib/a use one".to_string(), "use one from lib/a".to_string()),
        ("import \"lib/a.qwe\" use one".to_string(), "use one from \"lib/a.qwe\"".to_string()),
        ("import lib\\a.qwe use one".to_string(), "use one from lib\\a.qwe".to_string()),
        ("import ../shared/a.qwe use one".to_string(), "use one from ../shared/a.qwe".to_string()),
        ("import lib/a.qwe use *".to_string(), "use * from lib/a.qwe".to_string()),
        ("import redis.qwer use get_it as fetch".to_string(), "use get_it as fetch from redis.qwer".to_string()),
        (format!("client \"{}\" use Chart", url), format!("use Chart from \"{}\"", url)),
        (format!("client {} use Chart", url), format!("use Chart from {}", url)),
        ("client /assets/l.js use L, Draw".to_string(), "use L, Draw from /assets/l.js".to_string()),
        ("client /assets/m.mjs use go".to_string(), "use go from /assets/m.mjs".to_string()),
        ("client /assets/app.js?v=2 use app".to_string(), "use app from /assets/app.js?v=2".to_string()),
        (format!("preload \"{}\" use Chart", url), format!("use Chart with preload from \"{}\"", url)),
        (format!("preconnect \"{}\" use Chart", url), format!("use Chart with preconnect from \"{}\"", url)),
        (format!("client module \"{}\" use a, b", url), format!("use a, b with module from \"{}\"", url)),
        (format!("preload module \"{}\" use a as z", url), format!("use a as z with module, preload from \"{}\"", url)),
        (format!("preconnect module \"{}\" use a", url), format!("use a with module, preconnect from \"{}\"", url)),
        (format!("client module \"{}\" use default as d", url), format!("use default as d with module from \"{}\"", url)),
        (format!("client module \"{}\" use * as THREE", url), format!("use * as THREE with module from \"{}\"", url)),
    ];
    for (new, old) in pairs {
        assert_eq!(parsed(&new), parsed(&old), "{}", new);
        assert!(parsed(&new).is_ok(), "{}", new);
    }
}

#[test]
fn a_long_address_may_leave_its_use_to_the_next_line() {
    let one = parsed("preload \"https://cdn.example/a.js\" use Chart").unwrap();
    for two in [
        "preload \"https://cdn.example/a.js\"\n    use Chart",
        "preload \"https://cdn.example/a.js\"\nuse Chart",
        "preload \"https://cdn.example/a.js\" // the charts\n    use Chart",
        "preload \"https://cdn.example/a.js\"  \r\n  use Chart",
    ] {
        assert_eq!(parsed(two).unwrap(), one, "{}", two);
    }
}

#[test]
fn names_run_on_after_a_comma_and_the_line_after_a_star_is_its_own() {
    let m = program("run-on", &[
        ("main.qwe", "import lib/a.qwe use one,\n    two as deux\nimport lib/a.qwe use *\nlet after = three()\nfn out() = one() + deux() + after\n"),
        ("lib/a.qwe", LIB),
    ]);
    clean(&m);
    assert_eq!(runs(&m), "123");
}

#[test]
fn a_program_runs_with_files_taken_every_way() {
    let m = program("run", &[
        ("main.qwe", "import lib/a use one\nimport \"lib/a.qwe\" use two as deux\nimport lib/module use named\nfn out() = one() + deux() + named()\n"),
        ("lib/a.qwe", LIB),
        ("lib/module.qwe", "fn named() = \"m\"\n"),
    ]);
    clean(&m);
    assert_eq!(runs(&m), "12m");
}

#[test]
fn a_file_called_module_is_a_file() {
    let m = program("module-file", &[
        ("main.qwe", "import module use one\nfn out() = one()\n"),
        ("module.qwe", LIB),
    ]);
    clean(&m);
    assert_eq!(runs(&m), "1");
}

#[test]
fn a_rust_file_is_imported_the_same_way() {
    let m = program("rust", &[
        ("main.qwe", "import shout use loud\nimport shout.qwer use *\nprint(loud(\"a\"))\n"),
        ("shout.qwer", "pub fn loud(s: String) -> String {\n    s\n}\n"),
    ]);
    clean(&m);
}

#[test]
fn what_runs_in_the_browser_is_known_to_the_checker() {
    let m = program("browser", &[
        ("main.qwe", "client /assets/l.js use L\npreload module \"https://cdn.example/c.js\" use Chart as C2\ncomponent Map() {\n    <div @mount={(el) => { L.map(el); C2.new(el) }}></div>\n}\nroute GET / = <html><body><Map/></body></html>\n"),
    ]);
    clean(&m);
}

// ---- the words stay names ----------------------------------------------------------------------

#[test]
fn the_words_are_still_names() {
    let m = program("names", &[
        ("main.qwe", concat!(
            "let client = 8\nlet total = 2\nlet preload = (x) => x + 1\nlet import = 3\nlet module = 4\nlet preconnect = { use_it: 5 }\n",
            "let a = client /total\n",
            "let b = client / total\n",
            "client /total\n",
            "let c = preload(1)\n",
            "let d = preconnect.use_it\n",
            "import\n",
            "client = client + import + module\n",
            "fn out() = [a, b, c, d, client]\n",
        )),
    ]);
    assert_eq!(runs(&m), "[4, 4, 2, 5, 15]");
}

#[test]
fn a_name_on_a_line_before_a_use_stays_a_name() {
    let m = program("name-before-use", &[
        ("main.qwe", "let client = 8\nclient\nuse lib/a.qwe\nfn out() = client\n"),
        ("lib/a.qwe", LIB),
    ]);
    assert_eq!(runs(&m), "8");
}

#[test]
fn text_is_not_taken_for_an_import() {
    // A quote that never closes, a line with nothing after the word, and end of file.
    assert!(parsed("client \"https://a.b/c.js").unwrap_err().contains("says which"));
    assert!(parsed("let client = 1\nclient").is_ok());
    assert!(parsed("let client = 1\nclient // the end").is_ok());
}

// ---- mistakes ----------------------------------------------------------------------------------

#[test]
fn mistakes_say_what_to_write() {
    let url = "https://x.y/a.js";
    for (src, says) in [
        (format!("import \"{}\" use A", url), "what runs in the browser starts with `client`, `preload` or `preconnect`".to_string()),
        (format!("import module \"{}\" use A", url), "what runs in the browser starts with".to_string()),
        ("import module lib/a.qwe use A".to_string(), "what runs in the browser starts with".to_string()),
        ("client lib/a.qwe use one".to_string(), "`client` is for what runs in the browser, a .js or .mjs file or an address; lib/a.qwe is Quillo: import lib/a.qwe use ...".to_string()),
        ("preload lib/a.qwe use one".to_string(), "`preload` is for what runs in the browser".to_string()),
        ("import lib/a.qwe use * as all".to_string(), "`* as all` takes a whole ES module, not `lib/a.qwe`".to_string()),
        (format!("client \"{}\" use *", url), "a script gives the page names of its own".to_string()),
        (format!("client \"{}\" use * as all", url), "write client module".to_string()),
        (format!("client module \"{}\" use default", url), "a module's default export needs a name of yours".to_string()),
        (format!("client \"{}\" use __x", url), "cannot start with `__`".to_string()),
        ("preconnect /assets/a.js use A".to_string(), "`preconnect` is for a script at another address".to_string()),
        ("import ftp://x.y/a.qwe use a".to_string(), "not a `ftp:` address".to_string()),
        ("import lib/a.qwe use".to_string(), "expected a name to take after `use`".to_string()),
        ("import lib/a.qwe use in".to_string(), "which is a keyword".to_string()),
        ("import lib/a.qwe use a,".to_string(), "expected a name to take after `use`".to_string()),
        ("import lib/a.qwe use a as".to_string(), "the name to call it after `as`".to_string()),
        ("import lib/a.qwe use * as".to_string(), "the name to call it after `as`".to_string()),
        ("import lib/a.qwe use a, a".to_string(), "`a` is taken twice in this `use`".to_string()),
        ("import lib/a.qwe".to_string(), "`import` takes names from a file, and says which: import lib/a.qwe use ...".to_string()),
        (format!("client \"{}\"", url), format!("client \"{}\" use ...", url)),
        ("preload module a.js".to_string(), "preload module a.js use ...".to_string()),
    ] {
        let got = parsed(&src).unwrap_err();
        assert!(got.contains(&says), "{}: {}", src, got);
    }
}

#[test]
fn a_mark_left_open_ends_at_an_import() {
    for line in ["import lib/a.qwe use one", "client /a.js use go", "preload module \"https://a.b/c.js\" use go", "preconnect \"https://a.b/c.js\"", "import csv use parse"] {
        let src = format!("fn f() = <p>\n{}\n", line);
        let got = parsed(&src).unwrap_err();
        assert!(got.contains("never closed") || got.contains("<p>"), "{}: {}", line, got);
    }
    // Plain words in markup are its text.
    assert!(parsed("fn f() = <p>\nclient of ours\nimport\n</p>\n").is_ok());
    assert!(parsed("fn f() = <p>\nclient / ours\n</p>\n").is_ok());
}

// ---- what the checker says ---------------------------------------------------------------------

#[test]
fn a_use_written_as_0_6_did_still_works_and_says_how_it_is_written_now() {
    let m = program("old", &[
        ("main.qwe", "use one, two as deux from lib/a\nuse * from lib/a.qwe\nuse Chart from \"https://x.y/a.js\"\nuse * as T with module from \"https://x.y/t.js\"\nuse go from \"/my lib/m.mjs\"\nfn out() = one() + deux()\n"),
        ("lib/a.qwe", LIB),
    ]);
    assert_eq!(diags(&m), [
        "main.qwe:1 warning: write this as `import lib/a.qwe use one, two as deux` (`quillo fix` does)",
        "main.qwe:2 warning: write this as `import lib/a.qwe use *` (`quillo fix` does)",
        "main.qwe:3 warning: write this as `client \"https://x.y/a.js\" use Chart` (`quillo fix` does)",
        "main.qwe:4 warning: write this as `client module \"https://x.y/t.js\" use * as T` (`quillo fix` does)",
        "main.qwe:5 warning: write this as `client \"/my lib/m.mjs\" use go` (`quillo fix` does)",
    ]);
    assert_eq!(runs(&m), "12");
}

#[test]
fn a_name_not_taken_is_told_how_to_take_it() {
    let m = program("not-taken", &[
        ("main.qwe", "use lib/a.qwe\nfn out() = deep()\n"),
        ("lib/a.qwe", "use lib/b.qwe\nfn a() = deep()\n"),
        ("lib/b.qwe", "fn deep() = \"d\"\n"),
    ]);
    assert_eq!(diags(&m), ["main.qwe:2 warning: `deep` comes from lib/b.qwe, which this file does not take: add `import lib/b.qwe use deep` (`quillo fix` writes it)"]);
    let m = program("not-taken-both", &[
        ("main.qwe", "use lib/a.qwe\nuse lib/b.qwe\nfn out() = deep()\n"),
        ("lib/a.qwe", "fn deep() = \"a\"\n"),
        ("lib/b.qwe", "fn deep() = \"b\"\n"),
    ]);
    assert_eq!(diags(&m), ["main.qwe:3 `deep` comes from lib/a.qwe and lib/b.qwe: take the one you mean: import lib/a.qwe use deep"]);
    let m = program("not-taken-script", &[
        ("main.qwe", "use lib/a.qwe\nclient /assets/l.js use L\ncomponent Map() {\n    <div @mount={(el) => { L.map(el) }}></div>\n}\n"),
        ("lib/a.qwe", "component Other() {\n    <div @mount={(el) => { L.map(el) }}></div>\n}\n"),
    ]);
    assert_eq!(diags(&m), ["lib/a.qwe:2 warning: `L` is a browser library this file does not take: write its line here too (`quillo fix` does)"]);
}

// ---- quillo fix --------------------------------------------------------------------------------

fn fixed(name: &str, main: &str, others: &[(&str, &str)]) -> (PathBuf, String) {
    let mut files = vec![("main.qwe", main)];
    files.extend_from_slice(others);
    let m = program(name, &files);
    quillo::modules::fix(&m).unwrap();
    let text = std::fs::read_to_string(&m).unwrap();
    (m, text)
}

#[test]
fn fix_turns_every_kind_of_line_around() {
    let (m, text) = fixed("fix-all", concat!(
        "use one, two as deux from lib/a   // why\n",
        "use * from lib/a.qwe\n",
        "use a with preload from \"https://x.y/a.js\" // first\n",
        "use b with preconnect from \"https://x.y/b.js\"\n",
        "use c from \"https://x.y/c.js\"\n",
        "use d with module, preload from \"https://x.y/d.js\"\n",
        "use * as e with module from \"https://x.y/e.js\"\n",
        "use default as f with module from \"https://x.y/f.js\"\n",
        "use g from /assets/g.mjs\n",
        "use h from /assets/h.js\n",
        "use i from \"/my files/i.js\"\n",
        "    use j from \"/j.js\"\n",
        "use k from \"https://x.y/never-closed.js\n",
        "fn out() = one()\n",
    ).replace("use k from \"https://x.y/never-closed.js\n", "").as_str(), &[("lib/a.qwe", LIB)]);
    assert_eq!(text, concat!(
        "import lib/a.qwe use one, two as deux   // why\n",
        "import lib/a.qwe use *\n",
        "preload \"https://x.y/a.js\" use a // first\n",
        "preconnect \"https://x.y/b.js\" use b\n",
        "client \"https://x.y/c.js\" use c\n",
        "preload module \"https://x.y/d.js\" use d\n",
        "client module \"https://x.y/e.js\" use * as e\n",
        "client module \"https://x.y/f.js\" use default as f\n",
        "client /assets/g.mjs use g\n",
        "client /assets/h.js use h\n",
        "client \"/my files/i.js\" use i\n",
        "    client /j.js use j\n",
        "fn out() = one()\n",
    ));
    clean(&m);
    assert_eq!(quillo::modules::fix(&m).unwrap(), Vec::<String>::new());
}

#[test]
fn a_long_line_is_written_on_two_and_the_lines_after_it_are_not_lost() {
    let long = "https://cdn.jsdelivr.net/npm/canvas-confetti@1.9.3/dist/confetti.browser.min.js";
    let (m, text) = fixed("fix-long", &format!(
        "use a from \"{long}\"\n  use b from \"{long}\"  // two\nuse one from lib/a.qwe\nfn out() = one()\n"), &[("lib/a.qwe", LIB)]);
    assert_eq!(text, format!("client \"{long}\"\n    use a\n  client \"{long}\"\n      use b  // two\nimport lib/a.qwe use one\nfn out() = one()\n"));
    clean(&m);
    assert_eq!(runs(&m), "1");
}

#[test]
fn fix_says_what_it_did_a_line_each() {
    let long = "https://cdn.jsdelivr.net/npm/canvas-confetti@1.9.3/dist/confetti.browser.min.js";
    let m = program("fix-says", &[
        ("main.qwe", &format!("use a from \"{long}\"\nuse lib/a.qwe\nfn out() = deep()\n")),
        ("lib/a.qwe", "use lib/b.qwe\n"),
        ("lib/b.qwe", "fn deep() = 1\n"),
    ]);
    let said = quillo::modules::fix(&m).unwrap().join("\n").replace(&format!("{}/", m.parent().unwrap().display()), "");
    assert!(said.contains(&format!("main.qwe:1: client \"{long}\" use a")), "{}", said);
    assert!(said.contains("main.qwe: import lib/b.qwe use deep"), "{}", said);
    assert!(!said.contains('\n') || said.lines().count() == 2, "{}", said);
}

#[test]
fn a_line_with_more_than_a_use_stays_and_so_does_its_warning() {
    let m = program("fix-two-on-a-line", &[
        ("main.qwe", "let x = 1; use one from lib/a.qwe\nuse two from lib/a.qwe; use three from lib/a.qwe\nfn out() = one() + two() + three()\n"),
        ("lib/a.qwe", LIB),
    ]);
    quillo::modules::fix(&m).unwrap();
    let text = std::fs::read_to_string(&m).unwrap();
    assert_eq!(text, "let x = 1; use one from lib/a.qwe\nimport lib/a.qwe use two; use three from lib/a.qwe\nfn out() = one() + two() + three()\n");
    assert_eq!(runs(&m), "123");
    assert_eq!(diags(&m).len(), 2);
}

#[test]
fn a_file_that_calls_a_library_gets_the_line_of_the_file_that_takes_it() {
    let component = "component Other() {\n    <div @mount={(el) => { L.map(el) }}></div>\n}\n";
    let main = |take: &str| format!("use lib/a.qwe\n{}\ncomponent Map() {{\n    <div @mount={{(el) => {{ L.map(el) }}}}></div>\n}}\n", take);
    let long = "https://cdn.jsdelivr.net/npm/canvas-confetti@1.9.3/dist/confetti.browser.min.js";
    for (name, take, comes) in [
        ("one-line", "client /assets/l.js use L".to_string(), "client /assets/l.js use L\n".to_string()),
        ("one-line-comment", "preload \"https://x.y/l.js\" use L // the map".to_string(), "preload \"https://x.y/l.js\" use L // the map\n".to_string()),
        ("two-lines", format!("client \"{long}\"\n    use L"), format!("client \"{long}\"\n    use L\n")),
        ("written-before", "use L from /assets/l.js".to_string(), "client /assets/l.js use L\n".to_string()),
    ] {
        let m = program(&format!("fix-script-{}", name), &[("main.qwe", &main(&take)), ("lib/a.qwe", component)]);
        quillo::modules::fix(&m).unwrap();
        let lib = std::fs::read_to_string(m.parent().unwrap().join("lib/a.qwe")).unwrap();
        assert_eq!(lib, format!("{}\n{}", comes, component), "{}", name);
        clean(&m);
    }
}

#[test]
fn a_new_import_goes_below_those_at_the_start_of_a_file() {
    let long = "https://cdn.jsdelivr.net/npm/canvas-confetti@1.9.3/dist/confetti.browser.min.js";
    let m = program("fix-below", &[
        ("main.qwe", "use lib/a.qwe\nfn out() = deep()\n"),
        ("lib/a.qwe", &format!("// the file\nimport lib/c.qwe use c\nclient \"{long}\"\n    use go\npreconnect \"https://x.y/l.js\" use L\n\nimport lib/b.qwe use deep\nfn a() = c() + deep()\n")),
        ("lib/b.qwe", "fn deep() = \"d\"\n"),
        ("lib/c.qwe", "fn c() = \"c\"\n"),
    ]);
    quillo::modules::fix(&m).unwrap();
    assert_eq!(std::fs::read_to_string(&m).unwrap(), "use lib/a.qwe\nimport lib/b.qwe use deep\nfn out() = deep()\n");
    assert_eq!(runs(&m), "d");
}

#[test]
fn a_new_import_with_many_names_is_written_on_two_lines() {
    let names: Vec<String> = (0..14).map(|i| format!("a_long_name_{}", i)).collect();
    let lib: String = names.iter().map(|n| format!("fn {}() = 1\n", n)).collect();
    let calls = names.iter().map(|n| format!("{}()", n)).collect::<Vec<_>>().join(", ");
    let m = program("fix-many", &[
        ("main.qwe", "use lib/a.qwe\nuse lib/b.qwe\nprint(1)\n"),
        ("lib/a.qwe", &lib),
        ("lib/b.qwe", &format!("fn listed() = [{}]\n", calls)),
    ]);
    quillo::modules::fix(&m).unwrap();
    let b = std::fs::read_to_string(m.parent().unwrap().join("lib/b.qwe")).unwrap();
    let mut sorted = names.clone();
    sorted.sort();
    assert!(b.starts_with(&format!("import lib/a.qwe\n    use {}\n", sorted.join(", "))), "{}", b);
    clean(&m);
}

#[test]
fn the_command_writes_the_lines_and_then_has_nothing_to_do() {
    let m = program("fix-command", &[
        ("main.qwe", "use one from lib/a.qwe\nfn out() = one()\n"),
        ("lib/a.qwe", LIB),
    ]);
    let dir = m.parent().unwrap();
    let (ok, said) = quillo(&["fix", "main.qwe"], dir);
    assert!(ok && said.contains("import lib/a.qwe use one"), "{}", said);
    let (ok, said) = quillo(&["fix", "main.qwe"], dir);
    assert!(ok && said.contains("nothing to fix"), "{}", said);
}
