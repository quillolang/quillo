//! Modules (FEEDBACK19.md, Q-154 and Q-155; branch feedback19-fixes): what a file sees,
//! one public name in two files, what runs in the browser, and `quillo fix`.

mod support;

use quillo::check::{check_file, Severity};
use std::path::{Path, PathBuf};
use std::process::Command;

/// A program in a folder of its own: its first file is the main one.
fn program(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = support::temp_dir(&format!("modules-{}", name));
    for (f, text) in files {
        let p = d.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    d.join(files[0].0)
}

/// Diagnostics as `line[ warning:] message`, each with its file's name.
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

/// What the program's `out()` gives, run with no checks first.
fn runs(main: &Path) -> String {
    match load(main) {
        Ok(it) => {
            let f = quillo::value::lookup(&it.globals, "out").unwrap();
            let mut it = it;
            it.call(&f, vec![], 0).map_or("failed".to_string(), |v| v.display())
        }
        Err(e) => panic!("{}", e),
    }
}

fn load(main: &Path) -> Result<quillo::Interp, quillo::error::QuilloError> {
    let mut it = quillo::Interp::new(main.parent().unwrap());
    it.load_file(main)?;
    Ok(it)
}

fn refused(main: &Path) -> String {
    load(main).err().expect("it should not load").msg
}

fn quillo(args: &[&str], dir: &Path) -> (bool, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_quillo")).args(args).current_dir(dir).output().unwrap();
    (o.status.success(), format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)))
}

// ---- what a file sees ---------------------------------------------------------------------------

#[test]
fn a_plain_use_takes_every_public_name() {
    let m = program("plain", &[
        ("main.qwe", "use lib/a.qwe\nfn out() = one() + two()\n"),
        ("lib/a.qwe", "fn one() = \"1\"\nlet two = () => \"2\"\nprivate fn three() = 3\n"),
    ]);
    clean(&m);
    assert_eq!(runs(&m), "12");
}

#[test]
fn names_are_taken_by_name_and_as_another_name() {
    let m = program("named", &[
        ("main.qwe", "import lib/a.qwe use one, two as deux\nfn out() = one() + deux()\n"),
        ("lib/a.qwe", "fn one() = \"1\"\nfn two() = \"2\"\nfn three() = \"3\"\n"),
    ]);
    clean(&m);
    assert_eq!(runs(&m), "12");
}

#[test]
fn a_path_with_no_extension_is_the_qwe_file_there() {
    let m = program("no-extension", &[
        ("main.qwe", "import lib/a use one\nfn out() = one()\n"),
        ("lib/a.qwe", "fn one() = \"1\"\n"),
    ]);
    clean(&m);
    assert_eq!(runs(&m), "1");
}

#[test]
fn a_use_is_not_passed_on() {
    let m = program("not-passed-on", &[
        ("main.qwe", "use lib/a.qwe\nfn out() = deep()\n"),
        ("lib/a.qwe", "use lib/b.qwe\nfn a() = deep()\n"),
        ("lib/b.qwe", "fn deep() = \"d\"\n"),
    ]);
    assert_eq!(diags(&m), ["main.qwe:2 warning: `deep` comes from lib/b.qwe, which this file does not take: add `import lib/b.qwe use deep` (`quillo fix` writes it)"]);
    // A warning for now: it still runs, so a program can move a file at a time.
    assert_eq!(runs(&m), "d");
}

#[test]
fn a_name_not_taken_is_told_once_a_file_however_it_is_used() {
    let m = program("forms", &[
        ("main.qwe", "use lib/a.qwe\nfn out() = shared\n"),
        ("lib/a.qwe", "use lib/b.qwe\nfn a() = 1\n"),
        ("lib/b.qwe", "let shared = 1\nfn call() = 2\nview V() = <b>v</b>\n"),
        ("lib/c.qwe", ""),
    ]);
    let d = diags(&m);
    assert_eq!(d.len(), 1, "{:#?}", d);
    let m = program("forms-2", &[
        ("main.qwe", "use lib/a.qwe\nfn out() = [call(), call(), [1] | call]\nroute GET / = <V/>\nfn reset() { shared = 2 }\n"),
        ("lib/a.qwe", "use lib/b.qwe\nfn a() = 1\n"),
        ("lib/b.qwe", "let shared = 1\nfn call(x = 0) = x\nview V() = <b>v</b>\n"),
    ]);
    let d = diags(&m);
    assert_eq!(d.iter().filter(|l| l.contains("`call` comes from")).count(), 1, "{:#?}", d);
    assert_eq!(d.iter().filter(|l| l.contains("`V` comes from")).count(), 1, "{:#?}", d);
    assert_eq!(d.iter().filter(|l| l.contains("`shared` comes from")).count(), 1, "{:#?}", d);
}

#[test]
fn files_may_use_each_other() {
    let m = program("cycle", &[
        ("main.qwe", "use lib/a.qwe\nfn top() = \"t\"\nfn out() = a()\n"),
        ("lib/a.qwe", "import main.qwe use top\nfn a() = top() + \"a\"\n"),
    ]);
    clean(&m);
    assert_eq!(runs(&m), "ta");
}

#[test]
fn types_and_services_are_the_programs() {
    let m = program("types", &[
        ("main.qwe", "use lib/a.qwe\nfn out() = str(make().n)\n"),
        ("lib/a.qwe", "use lib/b.qwe\nfn make() = Point({ n: 1 })\n"),
        ("lib/b.qwe", "type Point = { n: int }\n"),
    ]);
    clean(&m);
    assert_eq!(runs(&m), "1");
    // And can be taken by name.
    let m = program("types-named", &[
        ("main.qwe", "import lib/b.qwe use Point\nfn out() = str(Point({ n: 1 }).n)\n"),
        ("lib/b.qwe", "type Point = { n: int }\n"),
    ]);
    clean(&m);
    assert_eq!(runs(&m), "1");
    let m = program("services", &[
        ("main.qwe", "use lib/a.qwe\nfn out() = pay\n"),
        ("lib/a.qwe", "service pay = \"pay.qwe\"\n"),
        ("pay.qwe", "server fn charge() = 1\n"),
    ]);
    clean(&m);
}

// ---- one name in two files ----------------------------------------------------------------------

#[test]
fn two_files_may_have_one_public_name() {
    let m = program("clash", &[
        ("main.qwe", "import lib/a.qwe use a\nimport lib/b.qwe use b\nfn out() = a() + b()\n"),
        ("lib/a.qwe", "fn helper() = \"a\"\nfn a() = helper()\n"),
        ("lib/b.qwe", "fn helper() = \"b\"\nfn b() = helper()\n"),
    ]);
    clean(&m);
    assert_eq!(runs(&m), "ab");
}

#[test]
fn a_file_takes_either_as_it_says() {
    let m = program("clash-as", &[
        ("main.qwe", "import lib/a.qwe use helper as a_helper\nimport lib/b.qwe use helper\nfn out() = a_helper() + helper()\n"),
        ("lib/a.qwe", "fn helper() = \"a\"\n"),
        ("lib/b.qwe", "fn helper() = \"b\"\n"),
    ]);
    clean(&m);
    assert_eq!(runs(&m), "ab");
}

#[test]
fn a_plain_use_gives_the_name_of_that_file() {
    let m = program("clash-plain", &[
        ("main.qwe", "use lib/a.qwe\nimport lib/b.qwe use b\nfn out() = helper() + b()\n"),
        ("lib/a.qwe", "fn helper() = \"a\"\n"),
        ("lib/b.qwe", "fn helper() = \"b\"\nfn b() = helper()\n"),
    ]);
    clean(&m);
    assert_eq!(runs(&m), "ab");
}

#[test]
fn the_main_files_own_name_comes_first() {
    let m = program("clash-main", &[
        ("main.qwe", "use lib/a.qwe\nfn helper() = \"main\"\nfn out() = helper() + a()\n"),
        ("lib/a.qwe", "fn helper() = \"a\"\nfn a() = helper()\n"),
    ]);
    clean(&m);
    assert_eq!(runs(&m), "maina");
}

#[test]
fn two_uses_giving_one_name_must_say_which() {
    let m = program("ambiguous", &[
        ("main.qwe", "use lib/a.qwe\nuse lib/b.qwe\nfn out() = helper()\n"),
        ("lib/a.qwe", "fn helper() = \"a\"\n"),
        ("lib/b.qwe", "fn helper() = \"b\"\n"),
    ]);
    assert_eq!(diags(&m), ["main.qwe:3 `helper` comes from lib/a.qwe and lib/b.qwe: take the one you mean: import lib/a.qwe use helper"]);
    // Nothing to say while it is not used, and taking one by name settles it.
    let m = program("ambiguous-unused", &[
        ("main.qwe", "use lib/a.qwe\nuse lib/b.qwe\nimport lib/b.qwe use helper\nfn out() = helper()\n"),
        ("lib/a.qwe", "fn helper() = \"a\"\n"),
        ("lib/b.qwe", "fn helper() = \"b\"\n"),
    ]);
    clean(&m);
    assert_eq!(runs(&m), "b");
}

#[test]
fn the_same_file_used_twice_gives_one_name() {
    let m = program("twice", &[
        ("main.qwe", "use lib/a.qwe\nuse lib/a.qwe\nuse lib/b.qwe\nfn out() = helper() + b()\n"),
        ("lib/a.qwe", "fn helper() = \"a\"\n"),
        ("lib/b.qwe", "private fn helper() = \"b\"\nfn b() = helper()\n"),
    ]);
    clean(&m);
    assert_eq!(runs(&m), "ab");
}

#[test]
fn a_name_several_files_have_and_this_one_took_from_none_is_refused() {
    let m = program("from-none", &[
        ("main.qwe", "import lib/a.qwe use one\nimport lib/b.qwe use two\nfn out() = helper()\n"),
        ("lib/a.qwe", "fn helper() = \"a\"\nfn one() = 1\n"),
        ("lib/b.qwe", "fn helper() = \"b\"\nfn two() = 2\n"),
    ]);
    assert_eq!(diags(&m), ["main.qwe:3 `helper` comes from lib/a.qwe and lib/b.qwe: take the one you mean: import lib/a.qwe use helper"]);
}

/// The main file's names stay plain, so the choice is met where a name is found.
#[test]
fn a_name_in_the_main_file_and_another_is_still_chosen_between() {
    let m = program("main-and-file", &[
        ("main.qwe", "use lib/c.qwe\nfn helper() = 1\n"),
        ("lib/c.qwe", "use main.qwe\nuse lib/a.qwe\nfn c() = helper()\n"),
        ("lib/a.qwe", "fn helper() = 2\n"),
    ]);
    assert_eq!(diags(&m), ["lib/c.qwe:3 `helper` comes from main.qwe and lib/a.qwe: take the one you mean: import main.qwe use helper"]);
}

#[test]
fn a_server_fns_name_is_its_address_so_it_stays_one() {
    let m = program("server-fn", &[
        ("main.qwe", "use lib/a.qwe\nuse lib/b.qwe\nfn out() = 1\n"),
        ("lib/a.qwe", "server fn save() = 1\n"),
        ("lib/b.qwe", "server fn save() = 2\n"),
    ]);
    let d = diags(&m);
    assert!(d.iter().any(|l| l.contains("`save` is defined twice, at lib/a.qwe:1 and lib/b.qwe:1")), "{:#?}", d);
    let m = program("type-twice", &[
        ("main.qwe", "use lib/a.qwe\nuse lib/b.qwe\nfn out() = 1\n"),
        ("lib/a.qwe", "type Point = { a: int }\n"),
        ("lib/b.qwe", "type Point = { b: int }\n"),
    ]);
    assert!(diags(&m).iter().any(|l| l.contains("`Point` is defined twice")), "{:#?}", diags(&m));
    // A plain function beside a server fn gives way: the address stays.
    let m = program("server-fn-beside", &[
        ("main.qwe", "use lib/a.qwe\nimport lib/b.qwe use b\nfn out() = str(b())\n"),
        ("lib/a.qwe", "server fn save() = 1\n"),
        ("lib/b.qwe", "fn save() = 2\nfn b() = save()\n"),
    ]);
    clean(&m);
    assert_eq!(runs(&m), "2");
}

#[test]
fn a_prop_cannot_have_a_name_that_is_renamed() {
    let m = program("prop", &[
        ("main.qwe", "import lib/a.qwe use title as heading\nview V(heading: str) = <b>{heading}</b>\nfn out() = 1\n"),
        ("lib/a.qwe", "fn title() = 1\n"),
    ]);
    let want = "`heading` is a name another file has too, and a prop of `V` too: props are passed by name, so rename one";
    assert_eq!(diags(&m), [format!("main.qwe:2 {}", want)]);
    assert_eq!(refused(&m), want);
    let m = program("prop-used-file", &[
        ("main.qwe", "use lib/a.qwe\nuse lib/b.qwe\nfn out() = 1\n"),
        ("lib/a.qwe", "fn title() = 1\nview V(title: str) = <b>{title}</b>\n"),
        ("lib/b.qwe", "fn title() = 2\n"),
    ]);
    assert!(diags(&m).iter().any(|l| l.contains("`title` is a name another file has too")), "{:#?}", diags(&m));
    assert!(refused(&m).contains("`title` is a name another file has too"));
}

// ---- what a `use` cannot take --------------------------------------------------------------------

#[test]
fn what_a_use_cannot_take_stops_the_program() {
    let cases: &[(&str, &str, &str)] = &[
        ("import lib/a.qwe use nope\nfn out() = 1\n", "fn one() = 1\n", "lib/a.qwe has no public name `nope`"),
        ("import lib/a.qwe use hidden\nfn out() = 1\n", "private fn hidden() = 1\n", "`hidden` is private to lib/a.qwe: make it `public` there, or take something public"),
        ("import lib/a.qwe use one\nfn one() = 2\nfn out() = 1\n", "fn one() = 1\n", "`one` is defined in this file, and taken from lib/a.qwe: take it `as` another name"),
    ];
    for (i, (main, lib, want)) in cases.iter().enumerate() {
        let m = program(&format!("cannot-{}", i), &[("main.qwe", main), ("lib/a.qwe", lib)]);
        assert_eq!(refused(&m), *want);
        assert!(diags(&m).iter().any(|l| l.contains(want)), "{:#?}", diags(&m));
    }
    let m = program("taken-twice", &[
        ("main.qwe", "import lib/a.qwe use one\nimport lib/b.qwe use one\nfn out() = 1\n"),
        ("lib/a.qwe", "fn one() = 1\n"),
        ("lib/b.qwe", "fn one() = 2\n"),
    ]);
    assert_eq!(refused(&m), "`one` is taken twice, the second time from lib/b.qwe: take one `as` another name");
}

#[test]
fn a_file_that_is_missing_or_broken_is_reported_where_it_is_used() {
    let m = program("missing", &[("main.qwe", "import lib/gone.qwe use one\nfn out() = 1\n")]);
    assert!(diags(&m).iter().any(|l| l.contains("cannot read")), "{:#?}", diags(&m));
    let m = program("broken", &[("main.qwe", "use lib/a.qwe\nfn out() = 1\n"), ("lib/a.qwe", "fn (\n")]);
    assert!(diags(&m).iter().any(|l| l.starts_with("lib/a.qwe:1")), "{:#?}", diags(&m));
}

#[test]
fn a_confined_check_reads_nothing_outside_its_folder() {
    let d = support::temp_dir("modules-confined");
    std::fs::write(d.join("secret.qwe"), "fn one() = 1\n").unwrap();
    std::fs::create_dir_all(d.join("sub")).unwrap();
    let opts = quillo::check::Options { confined: true, ..Default::default() };
    let a = quillo::check::check_source_opts("import ../secret.qwe use nope\n", "main.qwe", &d.join("sub"), opts);
    let shown: Vec<&str> = a.diagnostics.iter().map(|d| d.message.as_str()).collect();
    assert!(shown.iter().any(|m| m.contains("must stay inside")), "{:#?}", shown);
    assert!(!shown.iter().any(|m| m.contains("no public name")), "{:#?}", shown);
}

// ---- what runs in the browser ---------------------------------------------------------------

#[test]
fn the_first_word_marks_what_runs_in_the_browser() {
    for line in [
        "client \"https://cdn.example/chart.js\" use Chart",
        "client https://cdn.example/chart.js use Chart",
        "client /assets/leaflet.js use L",
        "preload \"https://cdn.example/chart.js\" use Chart",
        "client /assets/three.mjs use * as T",
        "client /assets/day.mjs use default as day",
        "client /assets/both.js use a, b",
        "client  /assets/chart.js  use  Chart",
    ] {
        let m = program("client", &[("main.qwe", &format!("{}\nfn out() = 1\n", line))]);
        clean(&m);
        assert_eq!(runs(&m), "1");
    }
}

#[test]
fn a_library_taken_as_0_6_did_still_works_and_says_how_it_is_written_now() {
    let m = program("client-plain", &[("main.qwe", "use Chart, Other from \"https://cdn.example/chart.js\"\nfn out() = 1\n")]);
    assert_eq!(diags(&m), ["main.qwe:1 warning: write this as `client \"https://cdn.example/chart.js\" use Chart, Other` (`quillo fix` does)"]);
    assert_eq!(runs(&m), "1");
}

#[test]
fn client_is_for_the_browser() {
    let m = program("client-qwe", &[("main.qwe", "client lib/a.qwe use one\n"), ("lib/a.qwe", "fn one() = 1\n")]);
    assert_eq!(refused(&m), "`client` is for what runs in the browser, a .js or .mjs file or an address; lib/a.qwe is Quillo: import lib/a.qwe use ...");
}

#[test]
fn client_is_still_a_name_a_file_can_have() {
    for (line, want) in [
        ("import lib/a.qwe use client", "c"),
        ("import lib/a.qwe use client as other\nlet client = other", "c"),
        ("import lib/a.qwe use client, more", "c"),
    ] {
        let m = program("client-name", &[
            ("main.qwe", &format!("{}\nfn out() = client()\n", line)),
            ("lib/a.qwe", "fn client() = \"c\"\nfn more() = 1\n"),
        ]);
        assert_eq!(runs(&m), want, "{}", line);
    }
    let m = program("client-file", &[("main.qwe", "use client.qwe\nfn out() = c()\n"), ("client.qwe", "fn c() = \"c\"\n")]);
    assert_eq!(runs(&m), "c");
    let m = program("client-alone", &[("main.qwe", "use client\nfn out() = c()\n"), ("client.qwe", "fn c() = \"c\"\n")]);
    assert_eq!(runs(&m), "c");
}

#[test]
fn a_browser_library_is_taken_by_the_file_using_it() {
    let m = program("client-file-sees", &[
        ("main.qwe", "use lib/a.qwe\nclient /assets/chart.js use Chart\n"),
        ("lib/a.qwe", "component C() = <p>{Chart.version}</p>\n"),
    ]);
    assert_eq!(diags(&m), ["lib/a.qwe:1 warning: `Chart` is a browser library this file does not take: write its line here too (`quillo fix` does)"]);
}

// ---- `quillo fix` -------------------------------------------------------------------------------

#[test]
fn fix_writes_each_files_uses() {
    let m = program("fix", &[
        ("main.qwe", "// The app.\nuse lib/a.qwe\n\nfn top() = 1\nprint(deep(), other())\n"),
        ("lib/a.qwe", "// Helpers.\n\nuse lib/b.qwe\nfn a() = top() + z_last()\n"),
        ("lib/b.qwe", "// Deep.\nfn deep() = 1\nfn other() = 2\nfn z_last() = 3\n"),
    ]);
    let d = m.parent().unwrap();
    let (ok, said) = quillo(&["fix", "main.qwe"], d);
    assert!(ok, "{}", said);
    assert!(said.contains("import lib/b.qwe use deep, other"), "{}", said);
    assert_eq!(std::fs::read_to_string(d.join("main.qwe")).unwrap(), "// The app.\nuse lib/a.qwe\nimport lib/b.qwe use deep, other\n\nfn top() = 1\nprint(deep(), other())\n");
    assert_eq!(std::fs::read_to_string(d.join("lib/a.qwe")).unwrap(), "// Helpers.\n\nuse lib/b.qwe\nimport main.qwe use top\nfn a() = top() + z_last()\n");
    clean(&m);
    let (ok, said) = quillo(&["fix", "main.qwe"], d);
    assert!(ok && said.contains("nothing to fix"), "{}", said);
}

#[test]
fn fix_writes_above_the_code_in_a_file_with_no_use() {
    let m = program("fix-no-use", &[
        ("main.qwe", "use lib/a.qwe\nuse lib/b.qwe\nprint(a())\n"),
        ("lib/a.qwe", "// Helpers.\n\nfn a() = b()\n"),
        ("lib/b.qwe", "fn b() = 1\n"),
    ]);
    quillo::modules::fix(&m).unwrap();
    assert_eq!(std::fs::read_to_string(m.parent().unwrap().join("lib/a.qwe")).unwrap(), "// Helpers.\n\nimport lib/b.qwe use b\n\nfn a() = b()\n");
    let m = program("fix-only-comments", &[
        ("main.qwe", "use lib/a.qwe\nuse lib/b.qwe\nprint(1)\n"),
        ("lib/a.qwe", "// Nothing.\n"),
        ("lib/b.qwe", "fn b() = 1\n"),
    ]);
    assert_eq!(quillo::modules::fix(&m).unwrap(), Vec::<String>::new());
}

#[test]
fn fix_turns_libraries_around_and_repeats_them_where_they_are_used() {
    let m = program("fix-client", &[
        ("main.qwe", "use lib/a.qwe\nuse Chart with preload from \"https://cdn.example/chart.js\"\n"),
        ("lib/a.qwe", "component C() = <p>{Chart.version}</p>\n"),
    ]);
    let d = m.parent().unwrap();
    let said = quillo::modules::fix(&m).unwrap();
    assert_eq!(said.len(), 2, "{:#?}", said);
    let line = "preload \"https://cdn.example/chart.js\" use Chart";
    assert!(std::fs::read_to_string(d.join("main.qwe")).unwrap().contains(line));
    assert!(std::fs::read_to_string(d.join("lib/a.qwe")).unwrap().starts_with(line));
    clean(&m);
}

#[test]
fn fix_leaves_a_program_with_errors_alone() {
    let m = program("fix-errors", &[("main.qwe", "print(nope)\n")]);
    let (ok, said) = quillo(&["fix", "main.qwe"], m.parent().unwrap());
    assert!(!ok && said.contains("unknown name `nope`"), "{}", said);
    assert_eq!(std::fs::read_to_string(&m).unwrap(), "print(nope)\n");
}

#[cfg(unix)]
#[test]
fn fix_says_when_it_cannot_write() {
    use std::os::unix::fs::PermissionsExt;
    let m = program("fix-read-only", &[
        ("main.qwe", "use lib/a.qwe\nuse lib/b.qwe\nprint(a())\n"),
        ("lib/a.qwe", "fn a() = b()\n"),
        ("lib/b.qwe", "fn b() = 1\n"),
    ]);
    let f = m.parent().unwrap().join("lib/a.qwe");
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o444)).unwrap();
    assert!(quillo::modules::fix(&m).unwrap_err().contains("cannot write"));
}

/// Every file now has `use` lines of its own, and they count from the program's folder.
#[test]
fn a_use_in_a_file_in_a_folder_counts_from_the_programs_folder_for_every_command() {
    let m = program("nested", &[
        ("main.qwe", "service billing = \"svc.qwe\"\nuse lib/a.qwe\nmount /sub = \"sub/main.qwe\"\nfn out() = a()\n"),
        ("lib/a.qwe", "import lib/b use b\nfn a() = b()\n"),
        ("lib/b.qwe", "fn b() = \"b\"\n"),
        ("svc.qwe", "use lib/s1.qwe\n"),
        ("lib/s1.qwe", "use lib/s2.qwe\nimport lib/s3 use s3\nserver fn invoice() = s2() + s3()\n"),
        ("lib/s2.qwe", "fn s2() = \"s2\"\n"),
        ("lib/s3.qwe", "fn s3() = \"s3\"\n"),
        ("sub/main.qwe", "use lib/c.qwe\nroute GET / = c()\n"),
        ("sub/lib/c.qwe", "use lib/d.qwe\nfn c() = d()\n"),
        ("sub/lib/d.qwe", "fn d() = \"d\"\n"),
    ]);
    clean(&m);
    let (ok, said) = quillo(&["build", "main.qwe"], m.parent().unwrap());
    assert!(ok, "{}", said);
    assert_eq!(quillo::describe::of_source(&m).is_ok(), true);
    assert_eq!(quillo::service::offered(&m.parent().unwrap().join("svc.qwe")).unwrap().len(), 1);
}
