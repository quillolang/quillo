//! Coverage of the lines of src/confirm.rs, src/native.rs and src/check.rs that the fix rounds
//! after the second coverage round added and the full run at 8b84078 left unrun (FEEDBACK83.md,
//! Q-1300 and Q-1305: the reading of a Rust plugin by its tokens as rustc reads them, a dependency
//! line as TOML reads it, a pinned build held to the bytes confirmed, a held start that its check
//! cannot read through, `quillo confirm`'s own paths; coverage of feedback82-85; branch
//! feedback82-85). Each test asserts what a developer sees: the reading's line, the refusal and
//! its line, the exit code; where the reading claims to read as rustc does, rustc is asked too.

mod support;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use support::temp_dir;

// ---- helpers, as tests/cover8285_confirm2.rs has them -------------------------------------------

/// A test's folder, removed with the record of what its machine built when the test ends.
struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
        let _ = std::fs::remove_dir_all(record_dir(&self.0));
    }
}

impl std::ops::Deref for Dir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

fn dir(name: &str) -> Dir {
    Dir(temp_dir(&format!("cov8285c-{}", name)))
}

/// Where a test's program keeps the record of what its machine built: beside its folder.
fn record_dir(dir: &Path) -> PathBuf {
    dir.with_file_name(format!("{}-built", dir.file_name().unwrap().to_string_lossy()))
}

const SECRET: &str = "test secret for the tests, 32 bytes or more";

/// Waits for `child` up to `secs`, killing it past them: its exit code.
fn ended(child: &mut std::process::Child, secs: u64) -> Option<i32> {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(s) = child.try_wait().unwrap() {
            return s.code();
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The command `quillo` with `args` in `dir`, as every test here starts it.
fn quillo_cmd(dir: &Path, args: &[&str]) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_quillo"));
    c.args(args)
        .current_dir(dir)
        .env_remove("QUILLO_POLICY_PATH")
        .env_remove("CARGO_TARGET_DIR")
        .env("QUILLO_BUILT_RECORD_DIR", record_dir(dir))
        .env("QUILLO_SECRET", SECRET)
        .stdin(Stdio::null());
    c
}

/// `quillo` with `args` in `dir`, ended after 120 s: its exit code and what it wrote, stdout and
/// stderr together.
fn quillo(dir: &Path, args: &[&str]) -> (Option<i32>, String) {
    let log = dir.join(format!("cmd-{}.log", std::process::id()));
    let file = std::fs::File::create(&log).unwrap();
    let mut child = quillo_cmd(dir, args).stdout(file.try_clone().unwrap()).stderr(file).spawn().unwrap();
    let code = ended(&mut child, 120);
    let out = read(&log);
    let _ = std::fs::remove_file(&log);
    (code, out)
}

/// `quillo` with `args` in `dir`: its exit code and its stdout alone.
fn quillo_stdout(dir: &Path, args: &[&str]) -> (Option<i32>, String) {
    let log = dir.join(format!("out-{}.log", std::process::id()));
    let file = std::fs::File::create(&log).unwrap();
    let mut child = quillo_cmd(dir, args).stdout(file).stderr(Stdio::null()).spawn().unwrap();
    let code = ended(&mut child, 120);
    let out = read(&log);
    let _ = std::fs::remove_file(&log);
    (code, out)
}

fn write(dir: &Path, name: &str, text: &str) {
    let p = dir.join(name);
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(p, text).unwrap();
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

fn sha(text: &str) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(text.as_bytes()).iter().map(|x| format!("{:02x}", x)).collect()
}

/// A plugin's text: its version, `//! effects: none`, then `code`.
fn plugin(code: &str) -> String {
    format!("//! version: 1.0.0\n//! effects: none\n{}\n", code)
}

/// What `quillo confirm --effects` says of the Rust file `name` with the text `src`: its exit code
/// and its output.
fn effects_of(d: &Path, name: &str, src: &str) -> (Option<i32>, String) {
    let f = format!("{}.qwer", name);
    write(d, &f, src);
    quillo(d, &["confirm", "--effects", &f])
}

/// The line `--effects` gives a file that says `effects: none` and calls `calls`.
fn reads_as(name: &str, effects: &str, calls: &str) -> String {
    format!("{}.qwer: effects: {}\n  but it says `effects: none`, and calls {}: a reading of its text, not a proof (a crate can do what it does unseen)\n", name, effects, calls)
}

/// Whether rustc (the one on the PATH, which builds plugins) compiles `src` as a 2021 library,
/// as a plugin is built: the reading is said to read where rustc does.
fn rustc_compiles(d: &Path, name: &str, src: &str) -> bool {
    let file = d.join(format!("{}.rs", name));
    std::fs::write(&file, src).unwrap();
    let out = d.join(format!("{}.rmeta", name));
    let mut child = Command::new("rustc")
        .args(["--edition", "2021", "--crate-type", "lib", "--emit", "metadata", "--cap-lints", "allow", "-o"])
        .arg(&out)
        .arg(&file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let ok = ended(&mut child, 300) == Some(0);
    let _ = std::fs::remove_file(&out);
    ok
}

// ---- quillo confirm's own paths ------------------------------------------------------------------

/// `quillo confirm --help` and `-h`, anywhere among its arguments, print its usage on stdout, exit
/// 0, and read and write nothing: no quillo.confirmed is made beside a program that reaches
/// something. The command's own entry (`quillo::confirm::main`, which the binary reaches only
/// past its own reading of `-h`) answers the same: 0, nothing written (src/confirm.rs main,
/// FEEDBACK83.md Q-1303; coverage of feedback82-85; branch feedback82-85).
#[test]
fn confirm_help_prints_its_usage_and_writes_nothing() {
    let d = dir("help");
    write(&d, "main.qwe", "route GET / = read_file(\"notes.txt\")\n");
    let (code, out) = quillo_stdout(&d, &["confirm", "--help"]);
    assert_eq!(code, Some(0), "{}", out);
    assert_eq!(out, format!("{}\n", quillo::confirm::USAGE));
    assert!(out.starts_with("usage:\n  quillo confirm [<file.qwe>] [PATH...]   list what the program's files reach that is not\n"), "{}", out);
    assert!(out.contains("\n  --effects FILE... what a reading of each Rust file finds it does, as an\n") && out.ends_with("where its own line leaves that out\n"), "{}", out);
    for args in [&["confirm", "-h"][..], &["confirm", "--yes", "--help"], &["confirm", "main.qwe", "-h"]] {
        assert_eq!(quillo_stdout(&d, args), (Some(0), out.clone()), "{:?}", args);
    }
    assert!(!d.join("quillo.confirmed").exists());
    // The entry itself, asked with --yes and a file before its -h: nothing confirmed, nothing read.
    for args in [vec!["--help"], vec!["--yes", d.join("main.qwe").to_str().unwrap(), "-h"]] {
        assert_eq!(quillo::confirm::main(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>()), 0, "{:?}", args);
    }
    assert!(!d.join("quillo.confirmed").exists());
}

/// With no quillo.confirmed and no provider's policy, `quillo check` warns of a plugin that builds
/// another file into it, at its line, and says that under a hold it would not load: `mod helper;`
/// and `include_str!` each; the check passes (exit 0) as nothing holds the run
/// (src/confirm.rs for_check, FEEDBACK83.md Q-1305; coverage of feedback82-85; branch
/// feedback82-85).
#[test]
fn check_with_no_hold_warns_of_a_plugin_that_builds_another_file_in() {
    // The second says fs, which its `include_str!` reads as: the one warning is the building in.
    for (name, effects, code, how, line) in [("mod", "none", "mod helper;", "`mod helper;`, a module in another file", 3), ("text", "fs", "\n\nconst T: &str = include_str!(\"/etc/hosts\");", "`include_str!`", 5)] {
        let d = dir(&format!("check-nohold-{}", name));
        write(&d, "hi.qwer", &format!("//! version: 1.0.0\n//! effects: {}\n{}\npub fn hi() -> String {{ \"hi\".into() }}\n", effects, code));
        write(&d, "main.qwe", "import hi.qwer use hi\nroute GET / = hi()\n");
        let (code, out) = quillo(&d, &["check", "main.qwe"]);
        assert_eq!(code, Some(0), "{}: {}", name, out);
        assert!(
            out.contains(&format!("hi.qwer:{}: warning: hi.qwer builds another file into it ({} at line {}), which this reading does not see; under a quillo.confirmed it does not load\n", line, how, line)),
            "{}: {}",
            name,
            out
        );
        assert!(out.contains("main.qwe: ok (1 route, 1 function, 0 components, in 2 files), 1 warning"), "{}: {}", name, out);
    }
}

// ---- where the reading starts, as rustc does ----------------------------------------------------

/// A first line `#!` followed by comments that are not documentation is an inner attribute where
/// the first thing after them is `[` (`#! // a note` then `[allow(unused)]` on the next line,
/// `#!/* a /* b */ c */[...]` nested, `#!/* a */ // b` then a comment over lines) and a line rustc
/// drops where it is not (`#! // a note` then `mod`, a `[` inside the comment): each `mod helper;`
/// after them is refused at its line, as rustc reads it (rustc compiles each with an inline `mod
/// helper { }`, which needs the reading's choice), and each inline module is confirmed
/// (src/confirm.rs rust_code_start, FEEDBACK83.md Q-1305; coverage of feedback82-85; branch
/// feedback82-85).
#[test]
fn a_shebang_line_with_comments_is_read_where_rustc_reads_it() {
    let cases = [
        ("attrnote", "#! // a note\n[allow(unused)]\n", 3),
        ("dropnote", "#! // a note\n", 2),
        ("attrnested", "#!/* a /* b */ c */[allow(unused)]\n", 2),
        ("attrlines", "#!/* a */ // b\n/* c\n */ [allow(unused)] ", 3),
        ("dropbracket", "#! // [no attribute\n", 2),
        ("dropfour", "#! //// four slashes are no documentation\n", 2),
    ];
    for (name, head, line) in cases {
        let d = dir(&format!("shebang-{}", name));
        let tail = "\npub fn hi() -> String { \"hi\".into() }\n";
        assert!(rustc_compiles(&d, name, &format!("{}mod helper {{ }}{}", head, tail)), "{}: rustc reads it otherwise", name);
        write(&d, "main.qwe", "import hi.qwer use hi\nroute GET / = hi()\n");
        write(&d, "hi.qwer", &format!("{}mod helper;{}", head, tail));
        let (code, out) = quillo(&d, &["confirm", "--yes"]);
        assert_eq!(code, Some(1), "{}: {}", name, out);
        let why = format!("quillo confirm: hi.qwer builds another file into it (`mod helper;`, a module in another file at line {}), which its SHA-256 in quillo.confirmed would not cover", line);
        assert!(out.starts_with(&why) && out.ends_with("quillo confirm: nothing written\n"), "{}: {}", name, out);
        assert!(!d.join("quillo.confirmed").exists(), "{}", name);
        write(&d, "hi.qwer", &format!("{}mod helper {{ }}{}", head, tail));
        let (code, out) = quillo(&d, &["confirm", "--yes"]);
        assert_eq!(code, Some(0), "{}: {}", name, out);
        assert!(read(&d.join("quillo.confirmed")).contains("[\"hi.qwer\"]\nsha256 = "), "{}", name);
    }
}

// ---- literals and paths, as rustc reads them -----------------------------------------------------

/// A quote, a name and a quote (`'ab'`) are one character as rustc's lexer reads them, not a
/// lifetime and a quote that opens another: the call after it on its line is read (fs, line 3);
/// a character cut by a `/` (`'(/'`) does not end as rustc reads one, and is refused under a hold
/// at its line; a raw lifetime (`'r#a`, which rustc compiles) leaves the reading as it was
/// (src/confirm.rs rust_lifetime_or_char, rust_quoted, rust_joined; FEEDBACK83.md Q-1305; coverage
/// of feedback82-85; branch feedback82-85).
#[test]
fn characters_and_lifetimes_are_read_as_rustc_reads_them() {
    let d = dir("chars");
    let read = "std::fs::read_to_string(\"p\").unwrap_or_default()";
    let (code, out) = effects_of(&d, "pair", &plugin(&format!("pub fn r() -> String {{ m!('ab' {}) }}", read)));
    assert_eq!((code, out.as_str()), (Some(1), reads_as("pair", "fs", "std::fs::read_to_string (line 3)").as_str()));
    let (code, out) = effects_of(&d, "slash", &plugin("pub const C: char = m!('(/');"));
    assert_eq!(code, Some(1), "{}", out);
    assert_eq!(out, "slash.qwer: effects: none\n  but it builds another file into it (a character that does not end as rustc reads one at line 3), which this reading does not see; under a quillo.confirmed it does not load\n");
    let raw = format!("pub fn r<'r#a>(p: &'r#a str) -> String {{ let _ = p; {} }}", read);
    assert!(rustc_compiles(&d, "rawlife", &raw), "rustc does not take a raw lifetime");
    let (code, out) = effects_of(&d, "rawlife", &plugin(&raw));
    assert_eq!((code, out.as_str()), (Some(1), reads_as("rawlife", "fs", "std::fs::read_to_string (line 3)").as_str()));
    // Under a hold the cut character is refused, and nothing is written.
    write(&d, "main.qwe", "import slash.qwer use hi\nroute GET / = hi()\n");
    write(&d, "slash.qwer", &plugin("pub const C: char = m!('(/');\npub fn hi() -> String { \"hi\".into() }"));
    let (code, out) = quillo(&d, &["confirm", "--yes"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.starts_with("quillo confirm: slash.qwer builds another file into it (a character that does not end as rustc reads one at line 3)"), "{}", out);
    assert!(!d.join("quillo.confirmed").exists());
}

/// Where the code is written as the reading looks through it, each call keeps its line: a `<` that
/// starts no qualified path of names (`<&str>::len`), a string over lines just before a `::`, a
/// number or a lifetime with a line break before its `(` or `!{`, and a path whose `::` ends the
/// file, each read so that the `std::fs` call after (or in) it is said at its own line
/// (src/confirm.rs rust_written, rust_joined; FEEDBACK83.md Q-1305; coverage of feedback82-85;
/// branch feedback82-85).
#[test]
fn each_call_keeps_its_line_however_the_code_around_it_is_written() {
    let d = dir("lines");
    let read = "pub fn r() -> String { std::fs::read_to_string(\"p\").unwrap_or_default() }";
    let cases = [
        ("refqual", format!("pub fn n() -> usize {{ <&str>::len(&\"ab\") }}\n{}", read), "std::fs::read_to_string (line 4)"),
        ("stringpath", format!("pub const S: &str = m!(\"a\nb\"::x);\n{}", read), "std::fs::read_to_string (line 5)"),
        ("numbercall", format!("pub fn f() -> u8 {{ m!(1\n(2)) }}\npub fn g<'a>() {{ m!('a\n!{{}}) }}\n{}", read), "std::fs::read_to_string (line 7)"),
        ("pathend", "pub fn r() -> String { String::new() }\npub fn e() { let _ = std::fs\n::".to_string(), "std::fs (line 4)"),
    ];
    for (name, code, call) in &cases {
        let (got, out) = effects_of(&d, name, &plugin(code));
        assert_eq!((got, out.as_str()), (Some(1), reads_as(name, "fs", call).as_str()), "{}", name);
    }
}

/// A `use` of a name by the module of its own name (`use log::log;`, then `use log::info;`), an
/// associated type with no `=` (`type Item;`), and one name two modules each take for another
/// module (`f` for `std::fs` and for `std::net`) are read: the first two leave the reading as it
/// was, and the second module's net is read beside the first's fs (src/confirm.rs rust_use_names,
/// RustNames; FEEDBACK83.md Q-1305; coverage of feedback82-85; branch feedback82-85).
#[test]
fn names_a_use_gives_are_read_however_they_repeat() {
    let d = dir("names");
    let read = "pub fn r() -> String { std::fs::read_to_string(\"p\").unwrap_or_default() }";
    let (code, out) = effects_of(&d, "own", &plugin(&format!("use log::log;\nuse log::info;\npub trait T {{ type Item; }}\n{}", read)));
    assert_eq!((code, out.as_str()), (Some(1), reads_as("own", "fs", "std::fs::read_to_string (line 6)").as_str()));
    let (code, out) = effects_of(&d, "twice", &plugin("mod a { pub use std::fs as f; }\nmod b { use std::net as f; pub fn c() { let _ = f::TcpStream::connect(\"203.0.113.9:80\"); } }"));
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.starts_with("twice.qwer: effects: fs, net\n  but it says `effects: none`, and calls "), "{}", out);
    let (code, out) = effects_of(&d, "twicecall", &plugin("mod a { pub use std::fs::File as f; }\nmod b { use std::net::TcpStream as f; pub fn c() { let _ = f::connect(\"203.0.113.9:80\"); } }"));
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.starts_with("twicecall.qwer: effects: fs, net\n") && out.contains("std::net::TcpStream (line 4)"), "{}", out);
}

/// What the reading cannot follow is read as every effect but db, said at its line:
/// `concat_idents!`, and paste's `[<...>]` whose 128 bytes looked through end inside a character
/// (`é` across the end: no panic, read as paste); a word that ends in `extern` before a `{`
/// (`struct Noextern { }`) is no foreign block, and the file reads as the fs it calls
/// (src/confirm.rs unfollowed; FEEDBACK83.md Q-1305; coverage of feedback82-85; branch
/// feedback82-85).
#[test]
fn what_cannot_be_followed_is_every_effect_and_a_name_ending_in_extern_is_not() {
    let d = dir("unfollowed");
    let every = "http, fs, env, time, random, net, run";
    let (code, out) = effects_of(&d, "concat", &plugin("pub fn r() -> u8 { concat_idents!(a, b) }"));
    assert_eq!((code, out.as_str()), (Some(1), reads_as("concat", every, "`concat_idents!` (a name a macro builds) (line 3)").as_str()));
    // `[<` at 0 of the code read, then 125 bytes, then `é` over bytes 127 and 128.
    let name = format!("a{}é", "_".repeat(124));
    let (code, out) = effects_of(&d, "paste", &plugin(&format!("paste::paste! {{ pub const [<{}>]: u8 = 1; }}", name)));
    assert_eq!((code, out.as_str()), (Some(1), reads_as("paste", every, "`[<...>]` (a name paste builds) (line 3)").as_str()));
    for n in [123, 125] {
        let (code, out) = effects_of(&d, "pastenear", &plugin(&format!("paste::paste! {{ pub const [<a{}é>]: u8 = 1; }}", "_".repeat(n))));
        assert_eq!((code, out.as_str()), (Some(1), reads_as("pastenear", every, "`[<...>]` (a name paste builds) (line 3)").as_str()), "{}", n);
    }
    let read = "pub fn r() -> String { std::fs::read_to_string(\"p\").unwrap_or_default() }";
    let (code, out) = effects_of(&d, "noextern", &plugin(&format!("pub struct Noextern {{ }}\n{}", read)));
    assert_eq!((code, out.as_str()), (Some(1), reads_as("noextern", "fs", "std::fs::read_to_string (line 4)").as_str()));
}

// ---- a dependency line as TOML reads it ---------------------------------------------------------

/// A dependency's table is read key by key as TOML reads it: a list of features with commas (a
/// trailing one too, `'a'` and `"b"` mixed) is confirmed with its crate; a quoted key (`"path"`),
/// two features with no comma between, a value that is no text, list, word or number (`{}`), and
/// two keys with no comma between are each a line not read, refused under a hold at its line,
/// and `quillo confirm` writes nothing (src/confirm.rs dep_line, FEEDBACK83.md Q-1305; coverage of
/// feedback82-85; branch feedback82-85).
#[test]
fn a_dependency_table_is_read_key_by_key_as_toml_reads_it() {
    let refused = [
        ("quotedkey", "x = { \"path\" = \"../x\" }"),
        ("nocomma", "x = { version = \"1\", features = [\"a\" \"b\"] }"),
        ("table", "x = { version = \"1\", optional = {} }"),
        ("keys", "x = { version = \"1\" features = [\"a\"] }"),
    ];
    for (name, dep) in refused {
        let d = dir(&format!("toml-{}", name));
        write(&d, "hi.qwer", &format!("//! version: 1.0.0\n//! effects: none\n//! [dependencies]\n//! {}\npub fn hi() -> String {{ \"hi\".into() }}\n", dep));
        write(&d, "main.qwe", "import hi.qwer use hi\nroute GET / = hi()\n");
        let (code, out) = quillo(&d, &["confirm", "--yes"]);
        assert_eq!(code, Some(1), "{}: {}", name, out);
        let why = format!("quillo confirm: hi.qwer builds another file into it (the dependency line `{}`, not read as a crate's name and version at line 4), which its SHA-256 in quillo.confirmed would not cover", dep);
        assert!(out.starts_with(&why) && out.ends_with("quillo confirm: nothing written\n"), "{}: {}", name, out);
        assert!(!d.join("quillo.confirmed").exists(), "{}", name);
    }
    for (name, dep) in [("commas", "itoa = { version = \"1\", features = [\"a\", \"b\"] }"), ("trailing", "itoa = { version = '1', features = ['a', \"b\",], default-features = false }")] {
        let d = dir(&format!("toml-{}", name));
        write(&d, "hi.qwer", &format!("//! version: 1.0.0\n//! effects: none\n//! [dependencies]\n//! {}\npub fn hi() -> String {{ \"hi\".into() }}\n", dep));
        write(&d, "main.qwe", "import hi.qwer use hi\nroute GET / = hi()\n");
        let (code, out) = quillo(&d, &["confirm", "--yes"]);
        assert_eq!(code, Some(0), "{}: {}", name, out);
        assert!(read(&d.join("quillo.confirmed")).contains("crates = [\"itoa 1\"]"), "{}: {}", name, read(&d.join("quillo.confirmed")));
    }
}

// ---- a held start that its check cannot read through --------------------------------------------

/// A held `--no-check` start whose main file and the two files it uses come to 32 MiB less a few
/// hundred bytes: the program it mounts, or the service it starts, is past what a check reads
/// together, so its main file is not read, and the start is refused, saying which file and what to
/// do; `quillo check` says the same error (src/check.rs check_file_budgeted, FEEDBACK83.md Q-1300;
/// coverage of feedback82-85; branch feedback82-85).
#[test]
fn a_held_start_whose_mounted_program_is_past_what_a_check_reads_does_not_start() {
    let mib = 1usize << 20;
    for (name, line, sub, file) in [("mount", "mount /m = \"m/main.qwe\"", "m", "main.qwe"), ("service", "service billing = \"svc/billing.qwe\"", "svc", "billing.qwe")] {
        let d = dir(&format!("budget-{}", name));
        write(&d, "main.qwe", &format!("use a.qwe\nuse b.qwe\n{}\nroute GET / = \"main\"\n", line));
        write(&d, "a.qwe", &format!("fn fa() {{\n  return 1\n}}\n{}", "\n".repeat(16 * mib - 64)));
        write(&d, "b.qwe", &format!("fn fb() {{\n  return 2\n}}\n{}", "\n".repeat(16 * mib - 600)));
        let program = if name == "mount" { "route GET / = \"mounted\"\n" } else { "server fn data() {\n  return \"billed\"\n}\n" };
        write(&d, &format!("{}/{}", sub, file), &format!("{}{}", program, "\n".repeat(600)));
        write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
        let (code, out) = quillo(&d, &["run", "main.qwe", "--port", "0", "--no-check"]);
        assert_eq!(code, Some(1), "{}: {}", name, out);
        let unread = format!("./{}/{}: error: cannot read file: the program's files come to more than 32 MiB together", sub, file);
        assert!(
            out.contains(&format!("the program is not started\n  {}\n  a check reads 32 MiB of a program's files together: make them smaller, or take fewer (`quillo check` says the same)\n", unread)),
            "{}: {}",
            name,
            out
        );
        assert!(!out.contains("listening on"), "{}: {}", name, out);
        let (code, out) = quillo(&d, &["check", "main.qwe"]);
        assert_eq!(code, Some(1), "{}: {}", name, out);
        assert!(out.contains(&unread), "{}: {}", name, out);
    }
}

// ---- a held program whose folder goes while it runs ---------------------------------------------

/// A held program started in its own folder (its paths judged from `.`) whose folder is removed
/// while it serves: `.` is then nowhere the system can say, so no path is where a confirmed one
/// is; `notes.txt`, confirmed to be read, and `out/x.txt`, confirmed to be written, are refused
/// (said once each in the log), the notes are not shown again and nothing is written anywhere
/// (src/confirm.rs real_under, FEEDBACK83.md Q-1300; coverage of feedback82-85; branch
/// feedback82-85).
#[test]
fn a_held_program_whose_folder_is_removed_reaches_no_path() {
    let d = dir("gone");
    let logs = dir("gone-log");
    write(&d, "main.qwe", "route GET / = str(read_file(\"notes.txt\"))\nroute writes GET /w = str(write_file(\"out/x.txt\", \"hi\"))\n");
    write(&d, "notes.txt", "my notes\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nfiles = [\"notes.txt\"]\nwrites = [\"out/\"]\n");
    let log = logs.join("run.log");
    let child = quillo_cmd(&d, &["run", "main.qwe", "--port", "0"]).stdout(Stdio::null()).stderr(std::fs::File::create(&log).unwrap()).spawn().unwrap();
    let mut g = support::Guard::new(child);
    let port = support::port_in_log(&mut g.0, &log, Duration::from_secs(60));
    assert_eq!(support::send(port, "GET", "/", &[], "").body, "my notes\n", "{}", read(&log));
    std::fs::remove_dir_all(&*d).unwrap();
    let a = support::send(port, "GET", "/", &[], "");
    assert!(!a.body.contains("my notes"), "{}", a.body);
    let w = support::send(port, "GET", "/w", &[], "");
    assert!(!w.body.contains("true"), "{}", w.body);
    let until = Instant::now() + Duration::from_secs(60);
    while !read(&log).contains("the path out/x.txt was refused") && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
    drop(g);
    let said = read(&log);
    assert!(said.contains("quillo: main.qwe: the path notes.txt was refused: it is not confirmed for this file (`quillo confirm` lists it)\n"), "{}", said);
    assert!(said.contains("quillo: main.qwe: the path out/x.txt was refused: it is not confirmed for this file (`quillo confirm` lists it)\n"), "{}", said);
    assert!(!d.exists() && !logs.join("out").exists(), "written somewhere");
}

// ---- the bytes built are the bytes confirmed -----------------------------------------------------

/// A plugin held both by a quillo.confirmed (by its SHA-256, A) and by a policy that pins its
/// source (A or B) with building off, while another process puts A and B in its place over and
/// over: a start that read A where it was judged but B where it was to be built says that it
/// changed since it was read, with B's SHA-256, and does not load; every start is refused one of
/// the four ways, and B is never taken for the file confirmed. The pinned build reads the file
/// once and builds those bytes (src/native.rs build_private, FEEDBACK83.md Q-1305; coverage of
/// feedback82-85; branch feedback82-85).
#[test]
fn a_plugin_changed_between_its_judging_and_its_build_does_not_load() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let d = dir("swap");
    // A comment of 256 KiB, so the reads are apart in time.
    let pad = format!("// {}\n", "x".repeat(256 << 10));
    let a = format!("//! version: 1.0.0\n//! effects: none\n{}pub fn hi() -> String {{ \"from A\".into() }}\n", pad);
    let b = format!("//! version: 1.0.0\n//! effects: none\n{}pub fn hi() -> String {{ \"from B\".into() }}\n", pad);
    let (ha, hb) = (sha(&a), sha(&b));
    write(&d, "a.src", &a);
    write(&d, "b.src", &b);
    write(&d, "hi.qwer", &a);
    write(&d, "main.qwe", "use \"hi.qwer\"\nprint(hi())\n");
    write(&d, "pin.toml", &format!("[plugins]\nallow = [\"hi@sha256:{}\", \"hi@sha256:{}\"]\nbuild = false\n", ha, hb));
    write(&d, "quillo.confirmed", &format!("[\"main.qwe\"]\n\n[\"hi.qwer\"]\nsha256 = \"{}\"\n", ha));
    // Read A throughout: refused only as building is off.
    let off = "so only a library built here from that source may load, and building is off";
    let (code, out) = quillo(&d, &["run", "main.qwe", "--no-check", "--policy", "pin.toml"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains(off), "{}", out);
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let swapper = {
        let (stop, d) = (stop.clone(), d.to_path_buf());
        std::thread::spawn(move || {
            let tmp = d.join("hi.qwer.next");
            while !stop.load(Ordering::Relaxed) {
                for src in ["a.src", "b.src"] {
                    let _ = std::fs::remove_file(&tmp);
                    if std::fs::hard_link(d.join(src), &tmp).is_ok() {
                        let _ = std::fs::rename(&tmp, d.join("hi.qwer"));
                    }
                }
            }
        })
    };
    let changed = format!("hi.qwer changed since it was read: its SHA-256 is {} now, not the one confirmed; it does not load", hb);
    let other = format!("hi.qwer is not the file confirmed: its SHA-256 is {} now", hb);
    let (mut seen, mut outs) = (false, Vec::new());
    for _ in 0..150 {
        let (code, out) = quillo(&d, &["run", "main.qwe", "--no-check", "--policy", "pin.toml"]);
        assert_eq!(code, Some(1), "{}", out);
        assert!(!out.contains("from A") && !out.contains("from B"), "{}", out);
        assert!(out.contains(&changed) || out.contains(&other) || out.contains(off), "refused otherwise: {}", out);
        if out.contains(&changed) {
            seen = true;
            break;
        }
        outs.push(out);
    }
    stop.store(true, Ordering::Relaxed);
    swapper.join().unwrap();
    assert!(seen, "no start read B where it was built: {:?}", &outs[outs.len().saturating_sub(3)..]);
}

// Fix round 6

/// What `quillo confirm --yes` says of a held program whose plugin `hi.qwer` is `code` under a
/// version and `effects: none` line: its exit code and what it wrote; nothing is written where it
/// is refused.
fn confirm_plugin(d: &Path, code: &str) -> (Option<i32>, String) {
    write(d, "main.qwe", "import hi.qwer use hi\nroute GET / = hi()\n");
    write(d, "hi.qwer", &plugin(code));
    let _ = std::fs::remove_file(d.join("quillo.confirmed"));
    quillo(d, &["confirm", "--yes"])
}

/// The refusal `quillo confirm` writes for hi.qwer, built in by `how` at `line`.
fn built_in_said(how: &str, line: u32, instead: &str) -> String {
    format!("quillo confirm: hi.qwer builds another file into it ({} at line {}), which its SHA-256 in quillo.confirmed would not cover: a change there would build another plugin under the one confirmed, so it is not confirmed and does not load under a quillo.confirmed ({})\nquillo confirm: nothing written\n", how, line, instead)
}

/// Assembly is refused under a hold however it is written, as `include!` is: its `.include "f.s"`
/// and `.incbin "f"` have the assembler read another file's code or bytes into a plugin held to its
/// SHA-256, a file rustc's list of what it read does not name either. `global_asm!` by its path
/// (`core::arch::`, `std::arch::`), a raw name, spaced, a comment or Unicode's space before its
/// `!`, braces; by a name a `use` gives it (`as g`, in a group, in a module, through `core::arch as
/// ar`, a name of a name, `pub(crate)`); under a glob, `extern crate core as c`, `{self as a}`,
/// handed through a macro; with its text made by `concat!`; `asm!` in a function and
/// `naked_asm!` in a naked one; a macro named by what a macro is given (`$m!`). rustc compiles
/// each; `quillo confirm` refuses each at its line and writes nothing. A function named `asm`, a
/// value named `asm` compared (`asm != 1`), and `#[doc = $d]` are no assembly: confirmed, and
/// `asm != 1` reads as `effects: none`, where it read as everything (src/confirm.rs builds_in,
/// unfollowed, FEEDBACK83.md Q-1305, found by the coverage tests; branch feedback82-85).
#[test]
fn assembly_however_it_is_written_is_refused_under_a_hold() {
    let d = dir("fix6-asm");
    let i = "\".incbin \\\"/nonexistent/x\\\"\"";
    let how = "assembly, whose `.include` and `.incbin` read another file in";
    let instead = "write that code in Rust, without assembly";
    let named = |n: &str| format!("`{}!`, {}", n, how);
    let other = |n: &str| format!("`{}!`, an assembly macro by another name: {}", n, how);
    let cases: Vec<(&str, String, String, u32)> = vec![
        ("corepath", format!("core::arch::global_asm!({});", i), named("global_asm"), 3),
        ("stdpath", format!("pub fn hi() -> String {{ \"hi\".into() }}\nstd::arch::global_asm!({});", i), named("global_asm"), 4),
        ("raw", format!("std::arch::r#global_asm!({});", i), named("global_asm"), 3),
        ("spaced", format!("std :: arch :: global_asm ! ({});", i), named("global_asm"), 3),
        ("comment", format!("core::arch::global_asm/* x */!({});", i), named("global_asm"), 3),
        ("unispace", format!("core::arch::global_asm\u{200E}!({});", i), named("global_asm"), 3),
        ("braces", format!("core::arch::global_asm!{{{}}}", i), named("global_asm"), 3),
        ("alias", format!("use core::arch::global_asm as g;\ng!({});", i), other("g"), 4),
        ("group", format!("use core::arch::{{global_asm as g}};\ng!({});", i), other("g"), 4),
        ("modalias", format!("mod a {{ pub use core::arch::global_asm as g; }}\na::g!({});", i), other("g"), 4),
        ("chain", format!("use core::arch as ar;\nuse ar::global_asm as g;\ng!({});", i), other("g"), 5),
        ("aliasalias", format!("use core::arch::global_asm as g;\nuse g as h;\nh!({});", i), other("h"), 5),
        ("crateuse", format!("pub(crate) use core::arch::global_asm as g;\ng!({});", i), other("g"), 4),
        ("glob", format!("use core::arch::*;\nglobal_asm!({});", i), named("global_asm"), 4),
        ("externcrate", format!("extern crate core as c;\nc::arch::global_asm!({});", i), named("global_asm"), 4),
        ("selfarch", format!("use core::arch::{{self as a}};\na::global_asm!({});", i), named("global_asm"), 4),
        ("passthru", format!("macro_rules! m {{ ($($t:tt)*) => {{ $($t)* }} }}\nm!(core::arch::global_asm!({}););", i), named("global_asm"), 4),
        ("concat", "core::arch::global_asm!(concat!(\".inc\", \"bin \\\"/nonexistent/x\\\"\"));".to_string(), named("global_asm"), 3),
        ("inline", format!("pub fn f() {{\n    unsafe {{ core::arch::asm!({}) }}\n}}", i), named("asm"), 4),
        ("naked", "#[unsafe(naked)]\npub extern \"C\" fn f() { core::arch::naked_asm!(\"ret\") }".to_string(), named("naked_asm"), 4),
        ("dollar", format!("macro_rules! m {{ ($m:ident) => {{ core::arch::$m!({}); }} }}\nm!(global_asm);", i), "`$m!`, a macro named by what a macro is given, which can be `include!`".to_string(), 3),
    ];
    for (name, code, why, line) in &cases {
        assert!(rustc_compiles(&d, name, code), "{}: rustc does not compile it", name);
        let instead = if name == &"dollar" { "put that code in hi.qwer" } else { instead };
        assert_eq!(confirm_plugin(&d, code), (Some(1), built_in_said(why, *line, instead)), "{}", name);
        assert!(!d.join("quillo.confirmed").exists(), "{}", name);
    }
    // A module named `assembly` is a module in another file, said as one.
    assert_eq!(confirm_plugin(&d, "mod assembly;"), (Some(1), built_in_said("`mod assembly;`, a module in another file", 3, "put that code in hi.qwer")));
    let fine = [
        ("fnasm", "pub fn asm() -> u8 { 1 }\npub fn hi() -> String { format!(\"{}\", asm()) }"),
        ("neq", "pub fn f(asm: u8) -> bool { asm != 1 }\npub fn hi() -> String { \"hi\".into() }"),
        ("docvalue", "macro_rules! d { ($d:expr) => { #[doc = $d] pub fn hi() -> String { \"hi\".into() } } }\nd!(\"x\");"),
    ];
    for (name, code) in fine {
        assert!(rustc_compiles(&d, name, code), "{}: rustc does not compile it", name);
        let (got, out) = confirm_plugin(&d, code);
        assert_eq!(got, Some(0), "{}: {}", name, out);
        assert!(read(&d.join("quillo.confirmed")).contains("[\"hi.qwer\"]\nsha256 = "), "{}", name);
        let (got, out) = effects_of(&d, name, &plugin(code));
        assert_eq!((got, out), (Some(0), format!("{}.qwer: effects: none\n", name)), "{}", name);
    }
}

/// The repro of the coverage tests: a plugin whose `global_asm!(".include \"<dir>/evil.s\"")`
/// builds another file's machine code in. With no hold, `quillo check` warns at its line that a
/// hold would refuse it (exit 0), and `--effects` says so. With a quillo.confirmed that holds it by
/// its SHA-256 (as one written before this fix would), `quillo check` says the error at line 3, a
/// `--no-check` start and `quillo build` are refused before anything is built, and no
/// `.quillo/native` is made. `confirm::built_in_refused`, for a build a provider's policy holds to
/// the source's SHA-256, says it too; a plugin of Rust alone, nothing (src/confirm.rs builds_in,
/// built_in_refused, FEEDBACK83.md Q-1305, found by the coverage tests; branch feedback82-85).
#[test]
fn a_plugin_that_assembles_another_file_in_does_not_load_where_it_is_held() {
    let d = dir("fix6-asm-repro");
    write(&d, "evil.s", ".globl _quillo_evil\n.p2align 2\n_quillo_evil:\n    mov x0, #42\n    ret\n");
    let src = format!("//! version: 1.0.0\n//! effects: none\nstd::arch::global_asm!(\".include \\\"{}/evil.s\\\"\");\nextern \"C\" {{ fn quillo_evil() -> i64; }}\npub fn hi() -> i64 {{ unsafe {{ quillo_evil() }} }}\n", d.display());
    write(&d, "hi.qwer", &src);
    write(&d, "main.qwe", "import hi.qwer use hi\nroute GET / = str(hi())\n");
    let how = "`global_asm!`, assembly, whose `.include` and `.incbin` read another file in at line 3";
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains(&format!("hi.qwer:3: warning: hi.qwer builds another file into it ({}), which this reading does not see; under a quillo.confirmed it does not load\n", how)), "{}", out);
    let (code, out) = quillo(&d, &["confirm", "--effects", "hi.qwer"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.ends_with(&format!("  but it builds another file into it ({}), which this reading does not see; under a quillo.confirmed it does not load\n", how)), "{}", out);
    write(&d, "quillo.confirmed", &format!("[\"main.qwe\"]\n\n[\"hi.qwer\"]\nsha256 = \"{}\"\n", sha(&src)));
    let refused = format!("hi.qwer builds another file into it ({}), which its SHA-256 in quillo.confirmed would not cover: a change there would build another plugin under the one confirmed, so it is not confirmed and does not load under a quillo.confirmed (write that code in Rust, without assembly)", how);
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains(&format!("hi.qwer:3: error: {}\n", refused)), "{}", out);
    let (code, out) = quillo(&d, &["run", "main.qwe", "--no-check", "--port", "0"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains(&format!("quillo: {}\n", refused)) && !out.contains("42"), "{}", out);
    let (code, out) = quillo(&d, &["build", "main.qwe"]);
    assert_ne!(code, Some(0), "{}", out);
    assert!(out.contains(&refused), "{}", out);
    assert!(!d.join(".quillo").join("native").exists(), "something was built");
    assert_eq!(
        quillo::confirm::built_in_refused("hi.qwer", &src, false),
        Some(format!("hi.qwer builds another file into it ({}), which the SHA-256 its source is held to would not cover, so it does not load (write that code in Rust, without assembly)", how))
    );
    assert_eq!(quillo::confirm::built_in_refused("hi.qwer", &plugin("pub fn hi() -> i64 { 42 }"), false), None);
    assert_eq!(quillo::confirm::built_in_refused("hi.qwer", "", false), None);
}

/// An attribute that has the linker read a library in is refused under a hold, however it is
/// written: `#[link(...)]`, in a `cfg_attr`, raw (`r#link`), spaced, in a module; `#[$m]` given
/// `link(name = ..)` by a `meta`; and `#![debugger_visualizer]`, a file rustc reads in. Where the
/// attribute's name or its `#` is all a macro is given (`#[$a(...)]` with `m!(link)`, `$h [ $a
/// (...) ]` with `m!(# link)`), the reading refuses it only where no linker's list judges the build
/// (`strict`); elsewhere the build is judged by what its linker read, and the reading no longer
/// refuses `$(#[$m:meta])*` and a `#` in a macro's input (fix round 7, which decides it).
/// rustc compiles each (the visualizer with its file there). What reads no file is confirmed: a
/// function and a value named `link`, `#[derive]`, `#[cfg(feature = ..)]`, `#![allow]`,
/// `#[doc = concat!(..)]`, a raw lifetime's `#` (`'r#a`) (src/confirm.rs attribute_built_in, FEEDBACK83.md
/// Q-1305, found by the coverage tests; branch feedback82-85).
#[test]
fn an_attribute_that_links_a_library_in_is_refused_under_a_hold() {
    let d = dir("fix6-link");
    write(&d, "x.py", "print('x')\n");
    let lib = "`#[link(...)]`, a library the linker reads in";
    let given = "an attribute a macro is given (`#[$m]`), which can be `#[link(...)]`: a library the linker reads in";
    let instead = "take that library through a crate instead, and write each attribute out";
    let cases: Vec<(&str, &str, &str, u32, &str)> = vec![
        ("link", "#[link(name = \"evil\", kind = \"static\")]\nextern \"C\" { fn f(); }", lib, 3, instead),
        ("cfgattr", "#[cfg_attr(all(), link(name = \"evil\"))]\nextern \"C\" { fn f(); }", lib, 3, instead),
        ("rawlink", "#[r#link(name = \"evil\")]\nextern \"C\" { fn f(); }", lib, 3, instead),
        ("spaced", "pub fn hi() -> String { \"hi\".into() }\n# [ link ( name = \"evil\" ) ]\nextern \"C\" { fn f(); }", lib, 4, instead),
        ("inmodule", "mod m { #![allow(unused)] #[link(name = \"evil\")] extern \"C\" { fn f(); } }", lib, 3, instead),
        ("meta", "macro_rules! m { ($m:meta) => { #[$m] extern \"C\" { fn f(); } } }\nm!(link(name = \"evil\"));", "`link(name = ...)`, which a macro can make `#[link(...)]` of: a library the linker reads in", 4, instead),
        ("visualizer", "#![debugger_visualizer(gdb_script_file = \"x.py\")]", "`#[debugger_visualizer(...)]`, a file rustc reads in", 3, "leave the visualizer out"),
    ];
    for (name, code, why, line, instead) in &cases {
        assert!(rustc_compiles(&d, name, code), "{}: rustc does not compile it", name);
        assert_eq!(confirm_plugin(&d, code), (Some(1), built_in_said(why, *line, instead)), "{}", name);
        assert!(!d.join("quillo.confirmed").exists(), "{}", name);
    }
    // All a macro is given: refused where no linker's list judges the build, read as nothing else.
    let strict = [
        ("givenname", "macro_rules! m { ($a:ident) => { #[$a(name = \"evil\")] extern \"C\" { fn f(); } } }\nm!(link);", given, 3),
        ("lone", "macro_rules! m { ($h:tt $a:ident) => { $h [ $a (name = \"evil\") ] extern \"C\" { fn f(); } } }\nm!(# link);", "a `[...]` a macro fills (`$h [ $a (...) ]`), which can be `#[link(...)]`: a library the linker reads in", 3),
    ];
    for (name, code, why, line) in strict {
        assert!(rustc_compiles(&d, name, code), "{}: rustc does not compile it", name);
        let src = plugin(code);
        assert_eq!(quillo::confirm::built_in_refused("hi.qwer", &src, true), Some(format!("hi.qwer builds another file into it ({} at line {}), which the SHA-256 its source is held to would not cover, so it does not load ({})", why, line, instead)), "{}", name);
        assert_eq!(quillo::confirm::built_in_refused("hi.qwer", &src, false), None, "{}", name);
    }
    let fine = [
        ("fnlink", "pub fn link(a: u8) -> u8 { a }\npub fn hi() -> String { format!(\"{}\", link(1)) }"),
        ("linkname", "pub fn f(link: u32) -> String { format!(\"{}\", link) }\n#[derive(Debug, Clone)]\npub struct S;\n#[cfg(feature = \"x\")]\npub fn g() {}\npub fn hi() -> String { \"hi\".into() }"),
        ("inner", "#![allow(unused)]\nmacro_rules! d { ($n:ident) => { #[doc = concat!(\"of \", stringify!($n))] pub fn $n() -> String { \"hi\".into() } } }\nd!(hi);"),
        ("rawlife", "pub fn hi<'r#a>() -> String { let _: &'r#a str = \"\"; \"hi\".into() }"),
    ];
    for (name, code) in fine {
        assert!(rustc_compiles(&d, name, code), "{}: rustc does not compile it", name);
        let (got, out) = confirm_plugin(&d, code);
        assert_eq!(got, Some(0), "{}: {}", name, out);
        assert!(read(&d.join("quillo.confirmed")).contains("[\"hi.qwer\"]\nsha256 = "), "{}", name);
    }
}

/// A qualified path whose type is in brackets or is a reference to it is read as the type:
/// `<(std::net::TcpStream)>::connect(a)`, `<((..))>::`, `<( ( .. ) )>::` spaced, `<(T) as
/// std::io::Read>::read`, `<&'static mut (T) as Read>::read`, `<&(T)>::connect` (read wide, as
/// rustc does not take it), a trait with generics (`<T as From<OwnedFd>>::from`), each read as
/// net where they read as `effects: none`; a qualified path over lines keeps the next call's line.
/// And the type an `impl` is for, which the code calls `Self` (`impl X for std::net::TcpStream {
/// .. Self::connect(a) .. }`), reads as net. What names no such type reads as nothing: `<()>::`,
/// `<(u8, u16)>::`, `<(u8)>::`, an `impl` for a tuple, `impl Trait` in an argument (src/confirm.rs
/// rust_written, impl_self_type, FEEDBACK83.md Q-1305, found by the coverage tests; branch
/// feedback82-85).
#[test]
fn a_qualified_path_of_a_type_in_brackets_reads_as_the_type() {
    let d = dir("fix6-qualified");
    let net = "std::net::TcpStream::connect (line 3)";
    let cases: Vec<(&str, &str, &str, bool)> = vec![
        ("paren", "pub fn c(a: &str) { let _ = <(std::net::TcpStream)>::connect(a); }", net, true),
        ("twice", "pub fn c(a: &str) { let _ = <((std::net::TcpStream))>::connect(a); }", net, true),
        ("spaced", "pub fn c(a: &str) { let _ = < ( ( std :: net :: TcpStream ) ) > :: connect(a); }", net, true),
        ("parenas", "pub fn c() { let _ = <(std::net::TcpStream) as std::io::Read>::read; }", "std::net::TcpStream::read (line 3)", true),
        ("refmut", "pub fn c() { let _ = <&'static mut (std::net::TcpStream) as std::io::Read>::read; }", "std::net::TcpStream::read (line 3)", true),
        ("ref", "pub fn c(a: &str) { let _ = <&(std::net::TcpStream)>::connect(a); }", net, false),
        ("generic", "pub fn c() { let _ = <std::net::TcpStream as From<std::os::fd::OwnedFd>>::from; }", "std::net::TcpStream::from (line 3)", true),
        ("selfimpl", "trait X { fn c(a: &str) -> Self; }\nimpl X for std::net::TcpStream { fn c(a: &str) -> Self { Self::connect(a).unwrap() } }", "std::net::TcpStream (line 4)", true),
        ("selfgeneric", "trait X<T> { fn c(a: &str) -> Self; }\nimpl<T: Into<String>> X<T> for &'static std::net::TcpStream where T: Clone { fn c(_: &str) -> Self { todo!() } }", "std::net::TcpStream (line 4)", true),
        ("lines", "pub fn c(a: &str) { let _ = <(\n\nstd::net::TcpStream\n\n) as\nstd::io::Read>::read;\nlet _ = std::net::TcpStream::connect(a); }", "std::net::TcpStream::read (line 5), std::net::TcpStream::connect (line 9)", true),
    ];
    for (name, code, calls, compiles) in &cases {
        assert_eq!(rustc_compiles(&d, name, code), *compiles, "{}: rustc reads it otherwise", name);
        let (got, out) = effects_of(&d, name, &plugin(code));
        assert_eq!((got, out.as_str()), (Some(1), reads_as(name, "net", calls).as_str()), "{}", name);
    }
    for (name, code) in [
        ("unit", "pub fn c() -> () { <()>::default() }"),
        ("tuple", "pub fn c() -> (u8, u16) { <(u8, u16)>::default() }"),
        ("one", "pub fn c() -> u8 { <(u8)>::default() }"),
        ("impltuple", "pub trait X { fn n() -> u8; }\nimpl X for (u8, u16) { fn n() -> u8 { 1 } }"),
        ("implarg", "pub fn c(r: impl std::io::Read) -> usize { std::mem::size_of_val(&r) }"),
    ] {
        assert!(rustc_compiles(&d, name, code), "{}: rustc does not compile it", name);
        assert_eq!(effects_of(&d, name, &plugin(code)), (Some(0), format!("{}.qwer: effects: none\n", name)), "{}", name);
    }
}

/// The crate of the macros of each crate that builds files in is refused under a hold too, as the
/// crate is, by its name, by its `package`, with `_` for `-` and in another case, as cargo finds
/// it: askama_derive, askama_macros, rinja_derive, sailfish-macros, rust-embed-impl (as
/// `rust_embed_impl` and as `package = "rust-embed-impl"`), include_dir_macros, include_dir_impl,
/// include-flate-codegen and its -exports, sqlx-macros, sqlx-macros-core, migrations_macros,
/// refinery-macros; and yarte, ramhorns, rust-i18n, graphql_client, typify, progenitor,
/// git-version and dotenv_codegen, each with its macros' crate. A name one letter longer (`askama_derive_x`, `sqlx-macros-cores`) or shorter
/// (`sqlx-macro`) is another crate, confirmed (src/confirm.rs RUST_FILE_CRATES,
/// dep_built_elsewhere, FEEDBACK83.md Q-1305, found by the coverage tests; branch feedback82-85).
#[test]
fn the_macro_crate_of_a_crate_that_builds_files_in_is_refused_under_a_hold() {
    let refused = [
        ("askama_derive = \"0.12\"", "askama_derive", "askama_derive"),
        ("askama_macros = \"0.14\"", "askama_macros", "askama_macros"),
        ("rinja_derive = \"0.3\"", "rinja_derive", "rinja_derive"),
        ("sailfish-macros = \"0.9\"", "sailfish-macros", "sailfish-macros"),
        ("rust-embed-impl = \"8\"", "rust-embed-impl", "rust-embed-impl"),
        ("rust_embed_impl = \"8\"", "rust_embed_impl", "rust-embed-impl"),
        ("e = { package = \"rust-embed-impl\", version = \"8\" }", "e", "rust-embed-impl"),
        ("e = { package = 'Rust_Embed_Impl', version = '8' }", "e", "rust-embed-impl"),
        ("Rust-Embed = \"8\"", "Rust-Embed", "rust-embed"),
        ("include_dir_macros = \"0.7\"", "include_dir_macros", "include_dir_macros"),
        ("include_dir_impl = \"0.6\"", "include_dir_impl", "include_dir_impl"),
        ("include-flate-codegen = \"0.3\"", "include-flate-codegen", "include-flate-codegen"),
        ("include_flate_codegen_exports = \"0.1\"", "include_flate_codegen_exports", "include-flate-codegen-exports"),
        ("sqlx-macros = \"0.8\"", "sqlx-macros", "sqlx-macros"),
        ("sqlx-macros-core = \"0.8\"", "sqlx-macros-core", "sqlx-macros-core"),
        ("migrations_macros = \"2\"", "migrations_macros", "migrations_macros"),
        ("refinery-macros = \"0.8\"", "refinery-macros", "refinery-macros"),
        ("yarte = \"0.15\"", "yarte", "yarte"),
        ("yarte_derive = \"0.15\"", "yarte_derive", "yarte_derive"),
        ("ramhorns = \"1\"", "ramhorns", "ramhorns"),
        ("ramhorns-derive = \"1\"", "ramhorns-derive", "ramhorns-derive"),
        ("rust-i18n = \"3\"", "rust-i18n", "rust-i18n"),
        ("rust-i18n-macro = \"3\"", "rust-i18n-macro", "rust-i18n-macro"),
        ("graphql_client = \"0.14\"", "graphql_client", "graphql_client"),
        ("graphql_query_derive = \"0.14\"", "graphql_query_derive", "graphql_query_derive"),
        ("typify = \"0.4\"", "typify", "typify"),
        ("typify-macro = \"0.4\"", "typify-macro", "typify-macro"),
        ("progenitor = \"0.11\"", "progenitor", "progenitor"),
        ("progenitor-macro = \"0.11\"", "progenitor-macro", "progenitor-macro"),
        ("git-version = \"0.3\"", "git-version", "git-version"),
        ("git-version-macro = \"0.3\"", "git-version-macro", "git-version-macro"),
        ("dotenv_codegen = \"0.15\"", "dotenv_codegen", "dotenv_codegen"),
        ("dotenv_codegen_implementation = \"0.15\"", "dotenv_codegen_implementation", "dotenv_codegen_implementation"),
    ];
    let d = dir("fix6-crates");
    write(&d, "main.qwe", "import hi.qwer use hi\nroute GET / = hi()\n");
    let src = |dep: &str| format!("//! version: 1.0.0\n//! effects: none\n//! [dependencies]\n//! {}\npub fn hi() -> String {{ \"hi\".into() }}\n", dep);
    for (dep, name, krate) in refused {
        write(&d, "hi.qwer", &src(dep));
        let (code, out) = quillo(&d, &["confirm", "--yes"]);
        assert_eq!(code, Some(1), "{}: {}", dep, out);
        let why = format!("the dependency `{}`, {}, whose macros build files in", name, krate);
        assert_eq!(out, built_in_said(&why, 4, "put that code in hi.qwer"), "{}", dep);
        assert!(!d.join("quillo.confirmed").exists(), "{}", dep);
    }
    for dep in ["askama_derive_x = \"1\"", "sqlx-macros-cores = \"1\"", "sqlx-macro = \"1\""] {
        write(&d, "hi.qwer", &src(dep));
        let (code, out) = quillo(&d, &["confirm", "--yes"]);
        assert_eq!(code, Some(0), "{}: {}", dep, out);
        assert!(read(&d.join("quillo.confirmed")).contains("[\"hi.qwer\"]\nsha256 = "), "{}", dep);
        std::fs::remove_file(d.join("quillo.confirmed")).unwrap();
    }
}

// Fix round 7

/// An assembly macro named where it is not called is refused under a hold, as a macro can call
/// it with its `!` handed apart: `m!(! core::arch::global_asm)` with `$($p)* $b (...)`,
/// `m!(core::arch::global_asm)` with `$($p)*!(...)`, `m!(! a)` where `use core::arch::asm as a;`;
/// and a name a `use` of the file's own path gives one (`use self::g as h; h!(...)`,
/// `crate::`, `super::` from a module), which was read as nothing. rustc compiles each. A field
/// and a value named `asm`, and an `asm` in a string, are no assembly: confirmed
/// (src/confirm.rs builds_in, FEEDBACK83.md Q-1305, found by the coverage tests; branch
/// feedback82-85).
#[test]
fn assembly_whose_bang_is_handed_apart_is_refused_under_a_hold() {
    let d = dir("fix7-asm");
    let i = "\".incbin \\\"/nonexistent/x\\\"\"";
    let how = "assembly, whose `.include` and `.incbin` read another file in";
    let instead = "write that code in Rust, without assembly";
    let named = |n: &str| format!("`{}`, an assembly macro named, which a macro can call: {}", n, how);
    let other = |n: &str| format!("`{}!`, an assembly macro by another name: {}", n, how);
    let cases: Vec<(&str, String, String, u32)> = vec![
        ("apart", format!("macro_rules! m {{ ($b:tt $($p:tt)*) => {{ $($p)* $b ({}); }} }}\nm!(! core::arch::global_asm);", i), named("global_asm"), 4),
        ("after", format!("macro_rules! m {{ ($($p:tt)*) => {{ $($p)*!({}); }} }}\nm!(core::arch::global_asm);", i), named("global_asm"), 4),
        ("bare", format!("use core::arch::global_asm;\nmacro_rules! m {{ ($b:tt $($p:tt)*) => {{ $($p)* $b ({}); }} }}\nm!(! global_asm);", i), named("global_asm"), 5),
        ("aliasapart", format!("use core::arch::asm as a;\nmacro_rules! m {{ ($b:tt $($p:tt)*) => {{ pub fn f() {{ unsafe {{ $($p)* $b ({}) }} }} }} }}\nm!(! a);", i), named("a"), 5),
        ("pathasm", format!("macro_rules! m {{ ($b:tt $($p:tt)*) => {{ pub fn f() {{ unsafe {{ $($p)* $b ({}) }} }} }} }}\nm!(! core::arch::asm);", i), named("asm"), 4),
        ("selfalias", format!("use core::arch::global_asm as g;\nuse self::g as h;\nh!({});", i), other("h"), 5),
        ("cratealias", format!("use core::arch::global_asm as g;\nmod m {{ use crate::g as h; h!({}); }}", i), other("h"), 4),
        ("superalias", format!("use core::arch::global_asm as g;\nmod m {{ use super::g as h; h!({}); }}", i), other("h"), 4),
    ];
    for (name, code, why, line) in &cases {
        assert!(rustc_compiles(&d, name, code), "{}: rustc does not compile it", name);
        assert_eq!(confirm_plugin(&d, code), (Some(1), built_in_said(why, *line, instead)), "{}", name);
        assert!(!d.join("quillo.confirmed").exists(), "{}", name);
    }
    let fine = "pub struct S { pub asm: u8, pub link: u8 }\npub fn g(s: &S) -> u8 { let asm = s.asm; asm + s.link }\npub fn hi() -> &'static str { \"asm!(x) #[link(name = \\\"z\\\")] global_asm\" }";
    assert!(rustc_compiles(&d, "fine", fine));
    let (got, out) = confirm_plugin(&d, fine);
    assert_eq!(got, Some(0), "{}", out);
}

/// A doc comment is a `#` and a `[doc = ".."]` to a macro, which can hand the `#` to another
/// group: `m!(/// x\n[link(name = "evil", kind = "static")])` with `$h $b extern ...`, and with
/// `$h [ $($a)* ]` given `link(name = ..)`, and with a `+verbatim` path; and the same of
/// `[debugger_visualizer(..)]` on an inline module. rustc compiles each; each is refused under a
/// hold at its line (src/confirm.rs builds_in, FEEDBACK83.md Q-1305, found by the coverage tests;
/// branch feedback82-85).
#[test]
fn a_doc_comments_hash_handed_to_a_macro_links_nothing_under_a_hold() {
    let d = dir("fix7-doc");
    write(&d, "x.py", "print('x')\n");
    let lib = "`link(name = ...)`, which a macro can make `#[link(...)]` of: a library the linker reads in";
    // A `[link(` after a doc comment (fix round 8, which names it so).
    let group = "`[link(...)]` where a doc comment or a macro gives it its `#`: a library the linker reads in";
    let instead = "take that library through a crate instead, and write each attribute out";
    let verbatim = format!("{}/libevil.a", d.display());
    let cases: Vec<(&str, String, &str, u32, &str)> = vec![
        ("group", "macro_rules! m { ($h:tt $d:tt $b:tt) => { $h $b extern \"C\" { fn evil_f() -> i32; } } }\nm!(/// x\n[link(name = \"evil\", kind = \"static\")]);".to_string(), group, 5, instead),
        ("inner", "macro_rules! m { ($h:tt $d:tt $($a:tt)*) => { $h [ $($a)* ] extern \"C\" { fn evil_f() -> i32; } } }\nm!(/// x\nlink(name = \"evil\", kind = \"static\"));".to_string(), lib, 5, instead),
        ("verbatim", format!("macro_rules! m {{ ($h:tt $d:tt $b:tt) => {{ $h $b extern \"C\" {{ fn evil_f() -> i32; }} }} }}\nm!(/// x\n[link(name = {:?}, kind = \"static\", modifiers = \"+verbatim\")]);", verbatim), group, 5, instead),
        ("kindfirst", "macro_rules! m { ($h:tt $d:tt $b:tt) => { $h $b extern \"C\" { fn evil_f() -> i32; } } }\nm!(/** x */ [link(kind = \"static\", name = \"evil\")]);".to_string(), group, 4, instead),
        ("visualizer", "macro_rules! m { ($h:tt $d:tt $b:tt) => { $h $b mod v {} } }\nm!(/// x\n[debugger_visualizer(gdb_script_file = \"x.py\")]);".to_string(), "`#[debugger_visualizer(...)]`, a file rustc reads in", 5, "leave the visualizer out"),
    ];
    for (name, code, why, line, instead) in &cases {
        assert!(rustc_compiles(&d, name, code), "{}: rustc does not compile it", name);
        assert_eq!(confirm_plugin(&d, code), (Some(1), built_in_said(why, *line, instead)), "{}", name);
        assert!(!d.join("quillo.confirmed").exists(), "{}", name);
    }
}

/// A held program whose plugin's text the reading reads as nothing, but whose build assembles or
/// links another file in, does not load: the pinned build is judged by its own crate's LLVM IR
/// (`m!(! asm)` under `use core::arch::*;`, which the reading does not see, calls inline
/// assembly) and on macOS by what its linker was given (a framework named through `../` from the
/// SDK's folder, by a `#` and a name a macro is given, linked a library of the test's folder,
/// which the built plugin would load; since fix round 8 refused by the linker's command line,
/// before its list). `quillo confirm --yes` confirms each, as the reading sees
/// nothing; the start says why it does not load, nothing is kept in `.quillo/native`, and the
/// plugin's code never runs (src/native.rs build_private, assembled_in, linked_in, FEEDBACK83.md
/// Q-1305, found by the coverage tests; branch feedback82-85).
#[cfg(unix)]
#[test]
fn a_held_build_is_judged_by_what_its_assembler_and_linker_read() {
    let d = dir("fix7-judged");
    write(&d, "main.qwe", "use \"hi.qwer\"\nprint(hi())\n");
    let asm = plugin("use core::arch::*;\nmacro_rules! m { ($b:tt $($p:tt)*) => { pub fn f() { unsafe { $($p)* $b (\"nop\") } } } }\nm!(! asm);\npub fn hi() -> String { f(); \"ran\".into() }");
    let mut cases = vec![("asm", asm, "and building it assembled code of its own (`".to_string(), "in rustc's IR), whose `.include` and `.incbin` read files that hash does not cover: it does not load, and what was built is not kept (write that code in Rust, without assembly)".to_string())];
    if cfg!(target_os = "macos") {
        // A library of the test's folder, named as a framework from the SDK's folder up.
        write(&d, "fw/e.c", "int evil_f(void) { return 4242; }\n");
        write(&d, "fw/E.framework/keep", "");
        let lib = d.join("fw/E");
        let mut cc = Command::new("cc").args(["-dynamiclib", "-o"]).arg(&lib).arg(d.join("fw/e.c")).arg("-install_name").arg(&lib).stdin(Stdio::null()).spawn().unwrap();
        assert_eq!(ended(&mut cc, 120), Some(0));
        let name = format!("{}{}", "../".repeat(40), lib.display().to_string().trim_start_matches('/'));
        let src = plugin(&format!("macro_rules! m {{ ($h:tt $a:ident $n:literal $k:literal) => {{ $h [ $a (name = $n, kind = $k) ] extern \"C\" {{ fn evil_f() -> i32; }} }} }}\nm!(# link {:?} \"framework\");\npub fn hi() -> String {{ unsafe {{ evil_f() }}.to_string() }}", name));
        // Refused by the linker's command line, before its list (fix round 8, which judges it).
        cases.push(("link", src, "and its linker was given the framework `".to_string(), format!("{}`, named by a path, which that hash does not cover: it does not load, and what was built is not kept (link only the system's libraries and what its crates build)", name)));
    }
    for (name, src, before, after) in &cases {
        write(&d, "hi.qwer", src);
        let _ = std::fs::remove_file(d.join("quillo.confirmed"));
        let (code, out) = quillo(&d, &["confirm", "--yes"]);
        assert_eq!(code, Some(0), "{}: the reading refused it: {}", name, out);
        let (code, out) = quillo(&d, &["run", "main.qwe"]);
        assert_eq!(code, Some(1), "{}: {}", name, out);
        assert!(out.contains(&format!("hi.qwer is held to its SHA-256, {}", before)) && out.contains(after.as_str()), "{}: {}", name, out);
        assert!(!out.contains("ran") && !out.contains("4242"), "{}: {}", name, out);
        let kept = std::fs::read_dir(d.join(".quillo/native")).map(|r| r.count()).unwrap_or(0);
        assert_eq!(kept, 0, "{}: a library was kept", name);
    }
}

/// The linker's list is judged file by file: the build's own objects, the toolchain's rlibs (its
/// lib/rustlib, as `rustc --print sysroot` says) and a file of the system's library folders (root's,
/// that no one else may write) pass; `/bin/sh`, root's but in no library folder, does not (fix round
/// 8, which trusts the library folders and lib/rustlib, not all that is root's nor the whole
/// sysroot); a library of the test's folder, by its name or through `../` from a
/// system folder, is the one named; a list with nothing read in it is refused; a file looked for
/// and not found, the output and the linker's version are not files read (macOS's records; Linux's
/// make rule with `\` lines and `\ ` spaces) (src/native.rs linked_from, FEEDBACK83.md Q-1305, found
/// by the coverage tests; branch feedback82-85).
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn the_linkers_list_is_judged_file_by_file() {
    let d = dir("fix7-list");
    let (target, crate_dir) = (d.join("target"), d.join("crate"));
    write(&d, "target/release/deps/x.o", "");
    write(&d, "crate/lib.rs", "");
    write(&d, "evil lib/libevil.a", "");
    let out = Command::new("rustc").args(["--print", "sysroot"]).output().unwrap();
    let sysroot = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    let rlib = std::fs::read_dir(sysroot.join("lib/rustlib")).unwrap().flatten().map(|e| e.path().join("lib")).find(|p| p.is_dir()).and_then(|p| std::fs::read_dir(p).unwrap().flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|x| x == "rlib"))).unwrap();
    let evil = d.join("evil lib/libevil.a");
    let through = PathBuf::from(format!("/usr/{}{}", "../".repeat(3), evil.display().to_string().trim_start_matches('/')));
    let own = target.join("release/deps/x.o");
    let list = |read: &[&Path]| -> Vec<u8> {
        if cfg!(target_os = "macos") {
            let mut b = b"\x00@(#)PROGRAM:ld PROJECT:ld-1\n\x00".to_vec();
            for (op, p) in [(0x11u8, Path::new("/nonexistent/libgone.a")), (0x40, Path::new("/out/lib.dylib"))].into_iter().chain(read.iter().map(|p| (0x10u8, *p))) {
                b.push(op);
                b.extend_from_slice(p.display().to_string().as_bytes());
                b.push(0);
            }
            b
        } else {
            let names: Vec<String> = read.iter().map(|p| p.display().to_string().replace(' ', "\\ ")).collect();
            let mut t = format!("/out/lib.so:{}\n", names.iter().map(|n| format!(" \\\n  {}", n)).collect::<String>());
            for n in &names {
                t.push_str(&format!("\n{}:\n", n));
            }
            t.into_bytes()
        }
    };
    let judged = |read: &[&Path]| quillo::native::linked_judged(&list(read), &crate_dir, &target);
    let system = system_library();
    assert_eq!(judged(&[&own, &rlib, &system]), Ok(None));
    assert_eq!(judged(&[&own, &rlib, &evil, &system]), Ok(Some(evil.clone())));
    assert_eq!(judged(&[&own, Path::new("/bin/sh")]), Ok(Some(PathBuf::from("/bin/sh"))));
    assert_eq!(judged(&[&own, &through]), Ok(Some(through.clone())));
    assert_eq!(judged(&[Path::new("lib.rs")]), Ok(Some(crate_dir.join("lib.rs"))), "a relative name is from the crate's folder, which is not the build's target");
    assert!(judged(&[]).is_err());
}

/// dotenvy's and dotenv's macro crates are refused under a hold, as dotenv_codegen is: their
/// `dotenv!` reads a `.env` as the plugin is built, from the folder rustc runs in up to `/`. A
/// macro named `dotenv!`, `try_dotenv!` or `x::dotenv!` reads as env, where it read as `effects:
/// none`; a name that only holds the word (`dotenv_path`) does not (src/confirm.rs
/// RUST_FILE_CRATES, RUST_CALLS, FEEDBACK83.md Q-1305, found by the coverage tests; branch
/// feedback82-85).
#[test]
fn the_crates_of_a_dotenv_macro_are_refused_and_it_reads_as_env() {
    let d = dir("fix7-dotenv");
    write(&d, "main.qwe", "import hi.qwer use hi\nroute GET / = hi()\n");
    for (dep, name) in [("dotenvy_macro = \"0.15\"", "dotenvy_macro"), ("dotenv_macros = \"0.10\"", "dotenv_macros"), ("dotenvy-macro = \"0.15\"", "dotenvy-macro")] {
        write(&d, "hi.qwer", &format!("//! version: 1.0.0\n//! effects: none\n//! [dependencies]\n//! {}\npub fn hi() -> String {{ \"hi\".into() }}\n", dep));
        let (code, out) = quillo(&d, &["confirm", "--yes"]);
        let krate = if name == "dotenvy-macro" { "dotenvy_macro" } else { name };
        assert_eq!((code, out.as_str()), (Some(1), built_in_said(&format!("the dependency `{}`, {}, whose macros build files in", name, krate), 4, "put that code in hi.qwer").as_str()), "{}", dep);
        assert!(!d.join("quillo.confirmed").exists(), "{}", dep);
    }
    for (name, code, call) in [
        ("dotenvy", "pub fn hi() -> String { dotenvy_macro::dotenv!(\"TOKEN\").to_string() }", "dotenvy_macro::dotenv!"),
        ("bare", "pub fn hi() -> String { dotenv!(\"TOKEN\").to_string() }", "dotenv!"),
        ("try", "pub fn hi() -> String { try_dotenv!(\"TOKEN\").to_string() }", "try_dotenv!"),
    ] {
        let (got, out) = effects_of(&d, name, &plugin(code));
        assert_eq!((got, out.as_str()), (Some(1), reads_as(name, "env", &format!("{} (line 3)", call)).as_str()), "{}", name);
    }
    let (got, out) = effects_of(&d, "path", &plugin("pub fn dotenv_path() -> u8 { 1 }\npub fn hi() -> u8 { dotenv_path() }"));
    assert_eq!((got, out), (Some(0), "path.qwer: effects: none\n".to_string()));
}

/// What the reading read as `effects: none` reads as what it does: a qualified path after a
/// keyword (`return <..>::connect`, `if`, `while`, `match`), a type with its own generics
/// (`<std::net::TcpStream<>>::`), an `impl` whose header has a const generic (`X<{ 1 }>`), and a
/// name a `use` of the file's own path gives `include_str!` (`use self::g as h;`). A type a macro
/// writes as an `impl`'s (`impl X for $t`) or a qualified path's (`<$t>::connect`) is read from
/// the macro's call, `i!(std::net::TcpStream)`, at its line. rustc compiles each. A comparison
/// (`a < b`), a turbofish of what a macro is given (`size_of::<$t>()`, `Key::<$h>::new`), an
/// `impl` for a type a macro is given `u8`, and an `impl` for a tuple with a const generic read as
/// nothing (src/confirm.rs rust_written, rust_use_names, unfollowed, FEEDBACK83.md Q-1305, found
/// by the coverage tests; branch feedback82-85).
#[test]
fn a_qualified_path_after_a_keyword_or_with_generics_reads_as_its_type() {
    let d = dir("fix7-read");
    let net = "std::net::TcpStream::connect (line 3)";
    let cases: Vec<(&str, &str, &str, &str)> = vec![
        ("ret", "pub fn c(a: &str) -> bool { return <std::net::TcpStream>::connect(a).is_ok(); }", "net", net),
        ("iff", "pub fn c(a: &str) -> u8 { if <std::net::TcpStream>::connect(a).is_ok() { 1 } else { 0 } }", "net", net),
        ("whiles", "pub fn c(a: &str) { while <std::net::TcpStream>::connect(a).is_err() {} }", "net", net),
        ("mat", "pub fn c(a: &str) -> u8 { match <std::net::TcpStream>::connect(a) { Ok(_) => 1, Err(_) => 0 } }", "net", net),
        ("generics", "pub fn c(a: &str) -> bool { let r = <std::net::TcpStream<>>::connect(a); r.is_ok() }", "net", net),
        ("constimpl", "trait X<const N: usize> { fn c(a: &str) -> Self; }\nimpl X<{ 1 }> for std::net::TcpStream { fn c(a: &str) -> Self { Self::connect(a).unwrap() } }", "net", "std::net::TcpStream (line 4)"),
        ("selfinclude", "use std::include_str as g;\nuse self::g as h;\npub fn c() -> &'static str { h!(\"/etc/hosts\") }", "fs", "std::include_str! (line 5)"),
        ("macimpl", "trait X { fn c(a: &str) -> Self; }\nmacro_rules! i { ($t:ty) => { impl X for $t { fn c(a: &str) -> Self { Self::connect(a).unwrap() } } } }\ni!(std::net::TcpStream);", "net", "std::net::TcpStream (line 5)"),
        ("macimplfs", "trait X { fn c(a: &str) -> Self; }\nmacro_rules! i { ($t:ty) => { impl X for $t { fn c(a: &str) -> Self { Self::open(a).unwrap() } } } }\ni!(std::fs::File);", "fs", "Self::open (line 4), std::fs::File (line 5)"),
        ("macqual", "macro_rules! c { ($t:ty) => { <$t>::connect(\"203.0.113.9:80\") } }\npub fn go() -> bool { c!(std::net::TcpStream).is_ok() }", "net", "std::net::TcpStream (line 4)"),
    ];
    for (name, code, effects, calls) in &cases {
        assert!(rustc_compiles(&d, name, code), "{}: rustc does not compile it", name);
        let (got, out) = effects_of(&d, name, &plugin(code));
        // `h!` is `include_str!` by another name, which builds another file in too.
        let built = if *name == "selfinclude" { "  but it builds another file into it (`h!`, an include macro by another name at line 5), which this reading does not see; under a quillo.confirmed it does not load\n" } else { "" };
        assert_eq!((got, out.as_str()), (Some(1), format!("{}{}", reads_as(name, effects, calls), built).as_str()), "{}", name);
    }
    for (name, code) in [
        ("lt", "pub fn lt(a: u8, b: u8) -> bool { a < b }\npub fn ret(a: u8) -> bool { return a < 1; }"),
        ("turbofish", "macro_rules! s { ($t:ty) => { std::mem::size_of::<$t>() } }\npub fn n() -> usize { s!(u64) }\nmacro_rules! v { ($t:ty) => { Vec::<$t>::new() } }\npub fn w() -> Vec<std::time::Duration> { v!(std::time::Duration) }"),
        ("macimplu8", "pub trait X { fn n() -> u8; }\nmacro_rules! i { ($t:ty) => { impl X for $t { fn n() -> u8 { 1 } } } }\ni!(u8);"),
        ("consttuple", "pub trait X<const N: usize> { fn n() -> usize; }\nimpl X<{ 2 }> for (u8, u16) { fn n() -> usize { 2 } }"),
    ] {
        assert!(rustc_compiles(&d, name, code), "{}: rustc does not compile it", name);
        assert_eq!(effects_of(&d, name, &plugin(code)), (Some(0), format!("{}.qwer: effects: none\n", name)), "{}", name);
    }
}

/// Ordinary code is no longer refused under a hold: `$(#[$m:meta])*` that hands a struct its
/// attributes, and a `#` in a macro's input (maud's `html!(div #main { .. })`); both are
/// confirmed. Where no linker's list judges the build (`strict`) both are still refused, as either
/// can make a `#[link]` (src/confirm.rs attribute_built_in, FEEDBACK83.md Q-1305, found by the
/// coverage tests; branch feedback82-85).
#[test]
fn attribute_forwarding_and_a_hash_in_a_macros_input_are_confirmed() {
    let d = dir("fix7-fine");
    let cases = [
        ("fwdmeta", "macro_rules! s { ($(#[$m:meta])* $n:ident) => { $(#[$m])* pub struct $n; } }\ns!(#[derive(Debug)] A);\npub fn hi() -> String { format!(\"{:?}\", A) }", "an attribute a macro is given (`#[$m]`), which can be `#[link(...)]`: a library the linker reads in", 3),
        ("maud", "macro_rules! html { ($($t:tt)*) => { stringify!($($t)*) } }\npub fn hi() -> &'static str { html!(div #main { \"x\" }) }", "a `#` that starts no attribute, handed to a macro, which can make one of it, `#[link(...)]`: a library the linker reads in", 4),
    ];
    for (name, code, why, line) in cases {
        assert!(rustc_compiles(&d, name, code), "{}: rustc does not compile it", name);
        let (got, out) = confirm_plugin(&d, code);
        assert_eq!(got, Some(0), "{}: {}", name, out);
        assert!(read(&d.join("quillo.confirmed")).contains("[\"hi.qwer\"]\nsha256 = "), "{}", name);
        let src = plugin(code);
        assert_eq!(quillo::confirm::built_in_refused("hi.qwer", &src, true), Some(format!("hi.qwer builds another file into it ({} at line {}), which the SHA-256 its source is held to would not cover, so it does not load (take that library through a crate instead, and write each attribute out)", why, line)), "{}", name);
    }
}

// Fix round 8

/// A file of the system's library folders, root's alone: on macOS `/usr/lib/dyld`, elsewhere the
/// first such regular file in `/usr/lib` or a folder of it.
#[cfg(unix)]
fn system_library() -> PathBuf {
    use std::os::unix::fs::MetadataExt;
    let ok = |p: &Path| std::fs::symlink_metadata(p).is_ok_and(|m| m.is_file() && m.uid() == 0 && m.mode() & 0o022 == 0);
    for d in ["/usr/lib", "/usr/lib/x86_64-linux-gnu", "/usr/lib/aarch64-linux-gnu", "/usr/lib64"] {
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            if ok(&e.path()) {
                return e.path().canonicalize().unwrap();
            }
        }
    }
    panic!("no file of the system's library folders here");
}

/// `[link(` is refused where an attribute can be, after a doc comment or what a macro hands in
/// (`$h [link(kind = .., name = ..)]` with `m!(#)`), and not in an array of calls of the plugin's
/// own `link` (`vec![link("/a"), link("/b")]`), which was refused, said as `link(name = ...)`
/// (src/confirm.rs builds_in, FEEDBACK83.md Q-1305, found by the coverage tests; branch
/// feedback82-85).
#[test]
fn a_call_of_a_function_named_link_in_an_array_is_confirmed() {
    let d = dir("fix8-link");
    let fine = "pub fn link(p: &str) -> String { p.to_string() }\npub fn hi() -> String { vec![link(\"/a\"), link(\"/b\")].join(\",\") }\npub fn two() -> [String; 1] { [link(\"/c\")] }";
    assert!(rustc_compiles(&d, "fine", fine));
    let (code, out) = confirm_plugin(&d, fine);
    assert_eq!(code, Some(0), "{}", out);
    assert!(read(&d.join("quillo.confirmed")).contains("[\"hi.qwer\"]\nsha256 = "));
    let given = "macro_rules! m { ($h:tt) => { $h [link(kind = \"static\", name = \"evil\")] extern \"C\" { fn f(); } } }\nm!(#);";
    assert!(rustc_compiles(&d, "given", given));
    assert_eq!(confirm_plugin(&d, given), (Some(1), built_in_said("`[link(...)]` where a doc comment or a macro gives it its `#`: a library the linker reads in", 3, "take that library through a crate instead, and write each attribute out")));
}

/// The linker's command line, as `--print link-args` prints it, is judged as it was given: what
/// rustc gives it of the build's own folder, the toolchain's lib/rustlib and the system's (its
/// objects, `-L`, `-lSystem`, `-o`, `-Wl,-Bstatic`, a temporary list gone after the link, a crate
/// named `plugin`) passes; a search folder elsewhere (`-L /opt/evil`), a path with `..`
/// (`-l:<t>/S/../../../bin/sh`, which lld lists as `/bin/sh`), a library elsewhere in `-Wl,` and
/// past `-Xlinker`, a framework named by a path, a link inside the folder that leads out of it, an
/// `-rpath` and a linker plugin are each refused, named; nothing printed is refused. `env -u`, the
/// variables set and the quoted words are read as rustc writes them (src/native.rs
/// link_args_refused, printed_args, FEEDBACK83.md Q-1305, found by the coverage tests; branch
/// feedback82-85).
#[cfg(unix)]
#[test]
fn the_linkers_command_line_is_judged_as_it_was_given() {
    let d = dir("fix8-args");
    write(&d, "t/release/deps/x.o", "");
    write(&d, "rl/lib/libstd.rlib", "");
    std::fs::create_dir_all(d.join("crate")).unwrap();
    std::os::unix::fs::symlink("/etc", d.join("t/out")).unwrap();
    let (t, rl) = (d.join("t").canonicalize().unwrap(), d.join("rl").canonicalize().unwrap());
    let ok = |p: &Path| p.starts_with(&t) || p.starts_with(&rl);
    let q = |a: &[String]| a.iter().map(|w| format!("{:?}", w)).collect::<Vec<_>>().join(" ");
    let line = |extra: &[String]| format!("env -u IPHONEOS_DEPLOYMENT_TARGET LC_ALL=\"C\" PATH=\"/opt/evil/bin:/usr/bin\" VSLANG=\"1033\" \"cc\" {}", q(&[&[format!("{}/release/deps/x.o", t.display()), "-L".into(), format!("{}/lib", rl.display()), "-lSystem".into(), "-lc".into(), "-arch".into(), "arm64".into(), "-mmacosx-version-min=11.0.0".into(), "-o".into(), format!("{}/release/deps/libquillo_plugin_x_pinned.dylib", t.display()), "-Wl,-dead_strip".into(), "-Wl,-Bstatic".into(), format!("-Wl,--version-script={}/release/deps/rustcX1/list", t.display()), format!("{}/release/deps/quillo_plugin_x_pinned.o", t.display())], extra].concat()));
    let judged = |extra: &[&str]| quillo::native::link_args_refused(&line(&extra.iter().map(|s| s.to_string()).collect::<Vec<_>>()), &d.join("crate"), &ok);
    assert_eq!(judged(&[]), None);
    assert_eq!(quillo::native::printed_args("env -u A -u B X=\"1 2\" \"cc\" \"-L\" \"/a b\" \"-x\\\"y\""), vec!["cc", "-L", "/a b", "-x\"y"]);
    let t_ = t.display().to_string();
    for (extra, said) in [
        (vec!["-L", "/opt/evil"], "`/opt/evil`"),
        (vec!["-L/opt/evil"], "`/opt/evil`"),
        (vec![&format!("-l:{}/S/../../../bin/sh", t_) as &str], "a path with `..`"),
        (vec!["-Wl,-force_load,/opt/evil/libx.a"], "`/opt/evil/libx.a`"),
        (vec!["-Xlinker", "/opt/evil/libx.a"], "`/opt/evil/libx.a`"),
        (vec!["-framework", "../../../x/E"], "the framework `../../../x/E`, named by a path"),
        (vec![&format!("{}/out/passwd", t_) as &str], "/etc/passwd"),
        (vec!["-Xlinker", "-rpath", "-Xlinker", "/opt/x"], "`-rpath`"),
        (vec!["-Wl,-plugin=/opt/x.so"], "`-plugin=/opt/x.so`"),
        (vec!["-B/opt/evil/bin"], "`/opt/evil/bin`"),
    ] {
        let why = judged(&extra).unwrap_or_else(|| panic!("{:?} passed", extra));
        assert!(why.contains(said), "{:?}: {}", extra, why);
    }
    assert_eq!(quillo::native::link_args_refused("", &d.join("crate"), &ok), Some("rustc printed no linker command for it".to_string()));
}

/// lld's and GNU ld's `--dependency-file` is read as they write it: a space as `\ `, a `#` as
/// `\#`, a `$` as `$$`, names over lines ended by ` \`, the output first and a rule of each name
/// after, each name once; and a failed build's log is known for a linker that does not take
/// `--dependency-file` (GNU ld before 2.35, lld's own words), not for another error (src/native.rs
/// dep_file_inputs, linker_lists_not, FEEDBACK83.md Q-1305, found by the coverage tests; branch
/// feedback82-85).
#[test]
fn a_dependency_file_is_read_as_lld_writes_it() {
    let text = "/t/lib.so: \\\n /a/b\\ c.o \\\n /x/\\#y.a \\\n /p/$$q.rlib \\\n /plain.o\n\n/a/b\\ c.o:\n\n/x/\\#y.a:\n\n/p/$$q.rlib:\n\n/plain.o:\n";
    assert_eq!(quillo::native::dep_file_inputs(text), vec!["/a/b c.o", "/x/#y.a", "/p/$q.rlib", "/plain.o"]);
    assert_eq!(quillo::native::dep_file_inputs("/t/lib.so:\r\n"), Vec::<String>::new());
    let gnu = "error: linking with `cc` failed: exit status: 1\n  = note: /usr/bin/ld: unrecognized option '--dependency-file=/t/linked.list'\n          /usr/bin/ld: use the --help option for usage information";
    assert_eq!(quillo::native::linker_lists_not(gnu), Some("  = note: /usr/bin/ld: unrecognized option '--dependency-file=/t/linked.list'"));
    assert!(quillo::native::linker_lists_not("ld.lld: error: unknown argument '--dependency-file=/t/x'").is_some());
    assert_eq!(quillo::native::linker_lists_not("error: linking with `cc` failed\n  = note: ld: library 'evil' not found"), None);
}

/// A Mach-O image of these load commands (`(cmd, name)`), 64 bits, little-endian.
fn macho(cmds: &[(u32, &str)]) -> Vec<u8> {
    let mut lc = Vec::new();
    for (cmd, name) in cmds {
        let fixed = if *cmd == 0x8000_001c { 12 } else { 24 };
        let size = (fixed + name.len() + 1 + 7) & !7;
        lc.extend_from_slice(&cmd.to_le_bytes());
        lc.extend_from_slice(&(size as u32).to_le_bytes());
        lc.extend_from_slice(&(fixed as u32).to_le_bytes());
        lc.resize(lc.len() + fixed - 12, 0);
        lc.extend_from_slice(name.as_bytes());
        lc.resize(lc.len() + size - fixed - name.len(), 0);
    }
    let mut b = vec![0xcf, 0xfa, 0xed, 0xfe];
    for v in [0x0100_000cu32, 0, 6, cmds.len() as u32, lc.len() as u32, 0, 0] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.extend(lc);
    b
}

/// An ELF image, 64 bits, little-endian, that needs `needed` and has a `DT_RUNPATH` if given.
fn elf(needed: &[&str], runpath: Option<&str>) -> Vec<u8> {
    let mut strtab = vec![0u8];
    let mut offs = Vec::new();
    for n in needed.iter().chain(runpath.iter()) {
        offs.push(strtab.len() as u64);
        strtab.extend_from_slice(n.as_bytes());
        strtab.push(0);
    }
    let str_at = 64 + 2 * 56;
    let dyn_at = (str_at + strtab.len() + 7) & !7;
    let mut dynamic = Vec::new();
    let mut entry = |t: u64, v: u64| {
        dynamic.extend_from_slice(&t.to_le_bytes());
        dynamic.extend_from_slice(&v.to_le_bytes());
    };
    for o in &offs[..needed.len()] {
        entry(1, *o);
    }
    if runpath.is_some() {
        entry(29, offs[needed.len()]);
    }
    entry(5, str_at as u64);
    entry(0, 0);
    let total = dyn_at + dynamic.len();
    let mut b = vec![0u8; 64];
    b[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    b[16..18].copy_from_slice(&3u16.to_le_bytes());
    b[18..20].copy_from_slice(&62u16.to_le_bytes());
    b[20..24].copy_from_slice(&1u32.to_le_bytes());
    b[32..40].copy_from_slice(&64u64.to_le_bytes());
    b[52..54].copy_from_slice(&64u16.to_le_bytes());
    b[54..56].copy_from_slice(&56u16.to_le_bytes());
    b[56..58].copy_from_slice(&2u16.to_le_bytes());
    for (kind, off, size) in [(1u32, 0u64, total as u64), (2, dyn_at as u64, dynamic.len() as u64)] {
        let mut h = vec![0u8; 56];
        h[..4].copy_from_slice(&kind.to_le_bytes());
        h[8..16].copy_from_slice(&off.to_le_bytes());
        h[16..24].copy_from_slice(&off.to_le_bytes());
        h[32..40].copy_from_slice(&size.to_le_bytes());
        h[40..48].copy_from_slice(&size.to_le_bytes());
        b.extend(h);
    }
    b.extend(strtab);
    b.resize(dyn_at, 0);
    b.extend(dynamic);
    b
}

/// What a library has the system's loader load is judged: Mach-O (thin and fat) loads only the
/// system's (`/usr/lib/`, `/System/Library/`), by its install name, with no `LC_RPATH`, `@rpath`
/// or `..`; ELF needs only the C runtime's (libc, libgcc_s, the dynamic loader), by name, with no
/// `DT_RUNPATH`; bytes that are no library are refused. And a library this machine linked, that
/// loads one of the test's folder by its install name, is refused with its name (src/native.rs
/// loads_refused, FEEDBACK83.md Q-1305, found by the coverage tests; branch feedback82-85).
#[test]
fn what_a_library_loads_is_the_systems_alone() {
    let sys = (0xc, "/usr/lib/libSystem.B.dylib");
    assert_eq!(quillo::native::loads_refused(&macho(&[sys, (0xc, "/System/Library/Frameworks/Security.framework/Versions/A/Security")])), None);
    for (cmds, said) in [
        (vec![sys, (0xc, "/tmp/x/libE.dylib")], "loads /tmp/x/libE.dylib by its install name, which is not the system's"),
        (vec![sys, (0x8000_0018, "@rpath/libE.dylib")], "loads @rpath/libE.dylib by its install name"),
        (vec![(0xc, "/usr/lib/../../tmp/libE.dylib")], "loads /usr/lib/../../tmp/libE.dylib"),
        (vec![sys, (0x8000_001c, "/tmp/x")], "has an LC_RPATH (/tmp/x)"),
    ] {
        let why = quillo::native::loads_refused(&macho(&cmds)).unwrap_or_else(|| panic!("{:?} passed", cmds));
        assert!(why.starts_with(said), "{:?}: {}", cmds, why);
    }
    // A fat library, its one slice the one that loads another.
    let slice = macho(&[sys, (0xc, "/tmp/x/libE.dylib")]);
    let mut fat = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 1];
    for v in [0x0100_000cu32, 0, 64, slice.len() as u32, 3] {
        fat.extend_from_slice(&v.to_be_bytes());
    }
    fat.resize(64, 0);
    fat.extend(&slice);
    assert!(quillo::native::loads_refused(&fat).is_some_and(|w| w.starts_with("loads /tmp/x/libE.dylib")));
    assert_eq!(quillo::native::loads_refused(&elf(&["libc.so.6", "libgcc_s.so.1", "ld-linux-x86-64.so.2", "libm.so.6"], None)), None);
    assert_eq!(quillo::native::loads_refused(&elf(&["libc.so.6", "libssl.so.3"], None)), Some("needs libssl.so.3, which is not the C runtime's".to_string()));
    assert_eq!(quillo::native::loads_refused(&elf(&["/opt/x/libc.so.6"], None)), Some("needs /opt/x/libc.so.6, which is not the C runtime's".to_string()));
    assert_eq!(quillo::native::loads_refused(&elf(&["libc.so.6"], Some("$ORIGIN/../lib"))), Some("has a DT_RUNPATH ($ORIGIN/../lib), where the loader looks for libraries".to_string()));
    if cfg!(any(target_os = "macos", target_os = "linux")) {
        // Said by what it is (after fcac7a3, which said it could not be read as one).
        assert_eq!(quillo::native::loads_refused(b"not a library"), Some("is not a library this system loads".to_string()));
        assert_eq!(quillo::native::loads_refused(&macho(&[sys])[..40]), Some("is not a library this system loads".to_string()));
    }
}

/// The repro of the review of a86f326: a held plugin that links, by a `#` and a name a macro is
/// given, a framework named through `../` from the SDK's folder to a library of the program's
/// folder, whose install name is another file there (`rt/libE.dylib`), which the system's loader
/// would load and whose code (31337) a change there would replace under the same SHA-256. The
/// reading confirms it; the start is refused by the linker's command line (the framework named by
/// a path), nothing is kept, and 31337 never prints (src/native.rs link_args_refused,
/// resolved_strict, FEEDBACK83.md Q-1305, found by the coverage tests; branch feedback82-85).
#[cfg(target_os = "macos")]
#[test]
fn a_held_plugin_that_links_a_framework_by_a_path_does_not_load() {
    let d = dir("fix8-framework");
    write(&d, "main.qwe", "use \"hi.qwer\"\nprint(hi())\n");
    write(&d, "w/s/e.c", "int evil_f(void) { return 31337; }\n");
    write(&d, "w/s/E.framework/keep", "");
    std::fs::create_dir_all(d.join("rt")).unwrap();
    let (lib, rt) = (d.join("w/s/E"), d.join("rt/libE.dylib"));
    for out in [&lib, &rt] {
        let mut cc = Command::new("cc").args(["-dynamiclib", "-o"]).arg(out).arg(d.join("w/s/e.c")).arg("-install_name").arg(&rt).stdin(Stdio::null()).spawn().unwrap();
        assert_eq!(ended(&mut cc, 120), Some(0));
    }
    let name = format!("{}{}", "../".repeat(40), lib.display().to_string().trim_start_matches('/'));
    write(&d, "hi.qwer", &plugin(&format!("macro_rules! m {{ ($h:tt $a:ident $n:literal $k:literal) => {{ $h [ $a (name = $n, kind = $k) ] extern \"C\" {{ fn evil_f() -> i32; }} }} }}\nm!(# link {:?} \"framework\");\npub fn hi() -> String {{ unsafe {{ evil_f() }}.to_string() }}", name)));
    let (code, out) = quillo(&d, &["confirm", "--yes"]);
    assert_eq!(code, Some(0), "the reading refused it: {}", out);
    let (code, out) = quillo(&d, &["run", "main.qwe"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains(&format!("hi.qwer is held to its SHA-256, and its linker was given the framework `{}`, named by a path, which that hash does not cover: it does not load, and what was built is not kept", name)), "{}", out);
    assert!(!out.contains("31337"), "{}", out);
    assert_eq!(std::fs::read_dir(d.join(".quillo/native")).map(|r| r.count()).unwrap_or(0), 0);
}

/// A library pinned by its own SHA-256 (a provider's `QUILLO_POLICY_PATH` layer) in
/// `.quillo/native`, which loads a library of the program's folder by its install name, does not
/// load: its hash covers it, not what the system's loader loads for it (src/native.rs use_file,
/// loads_refused, FEEDBACK83.md Q-1305, found by the coverage tests; branch feedback82-85).
#[cfg(target_os = "macos")]
#[test]
fn a_library_pinned_by_its_hash_that_loads_another_by_its_install_name_does_not_load() {
    let d = dir("fix8-libpin");
    write(&d, "main.qwe", "use \"hi.qwer\"\nprint(hi())\n");
    write(&d, "hi.qwer", &plugin("pub fn hi() -> String { \"hi\".into() }"));
    write(&d, "rt/e.c", "int evil_f(void) { return 31337; }\n");
    write(&d, "rt/x.c", "extern int evil_f(void);\nint quillo_native_abi(void) { return evil_f(); }\n");
    let rt = d.join("rt/libE.dylib");
    let mut cc = Command::new("cc").args(["-dynamiclib", "-o"]).arg(&rt).arg(d.join("rt/e.c")).arg("-install_name").arg(&rt).stdin(Stdio::null()).spawn().unwrap();
    assert_eq!(ended(&mut cc, 120), Some(0));
    let lib = quillo::native::library_path(&d.join("hi.qwer")).unwrap();
    std::fs::create_dir_all(lib.parent().unwrap()).unwrap();
    let mut cc = Command::new("cc").args(["-dynamiclib", "-o"]).arg(&lib).arg(d.join("rt/x.c")).arg(&rt).stdin(Stdio::null()).spawn().unwrap();
    assert_eq!(ended(&mut cc, 120), Some(0));
    let bytes = std::fs::read(&lib).unwrap();
    let lib_hash: String = { use sha2::Digest; sha2::Sha256::digest(&bytes).iter().map(|x| format!("{:02x}", x)).collect() };
    write(&d, "pin.toml", &format!("[plugins]\nallow = [\"hi@sha256:{}\"]\n", lib_hash));
    let mut c = quillo_cmd(&d, &["run", "main.qwe"]);
    let log = d.join("pin.log");
    let file = std::fs::File::create(&log).unwrap();
    let mut child = c.env("QUILLO_POLICY_PATH", d.join("pin.toml")).stdout(file.try_clone().unwrap()).stderr(file).spawn().unwrap();
    let code = ended(&mut child, 120);
    let out = read(&log);
    assert_eq!(code, Some(1), "{}", out);
    // Its hash covers the library itself: what is said is what it loads (after fcac7a3, which said
    // that hash did not cover it).
    assert!(out.contains(&format!("{} loads {} by its install name, which is not the system's (/usr/lib, /System/Library): it does not load", lib.file_name().unwrap().to_string_lossy(), rt.display())), "{}", out);
    assert!(!out.contains("31337"), "{}", out);
}

// Fix round 10

/// A plugin that takes a crate that reaches the system or loads code itself (libc, nix, rustix,
/// libloading, dlopen2, windows-sys, any `-sys` crate), by its name or as a `package`, reads as
/// every effect but db, at its dependency's line: `libc::dlopen(p)`, `libc::system(c)` and
/// `libloading::Library::new(p)` read as `effects: none`. A crate whose name only starts so
/// (`libcx`) or holds `sys` (`sysinfo`, read as fs) is not one (src/confirm.rs scan_rust,
/// RUST_SYSTEM_CRATES, FEEDBACK83.md Q-1305, found by the coverage tests; branch feedback82-85).
#[test]
fn a_crate_that_reaches_the_system_itself_reads_as_every_effect() {
    let d = dir("fix10-system");
    let every = "http, fs, env, time, random, net, run";
    let head = |dep: &str| format!("//! version: 1.0.0\n//! effects: none\n//! [dependencies]\n//! {}\n", dep);
    for (name, dep, krate, code) in [
        ("dl", "libc = \"0.2\"", "libc", "pub fn load(p: &str) -> i64 { let c = std::ffi::CString::new(p).unwrap(); let h = unsafe { libc::dlopen(c.as_ptr(), libc::RTLD_NOW) }; if h.is_null() { 0 } else { 1 } }"),
        ("sys", "libc = \"0.2\"", "libc", "pub fn run(p: &str) -> i64 { let c = std::ffi::CString::new(p).unwrap(); unsafe { libc::system(c.as_ptr()) as i64 } }"),
        ("loading", "libloading = \"0.8\"", "libloading", "pub fn load(p: &str) -> i64 { match unsafe { libloading::Library::new(p) } { Ok(l) => { std::mem::forget(l); 1 } Err(_) => 0 } }"),
        ("nix", "nix = { version = \"0.29\", features = [\"process\"] }", "nix", "pub fn n() -> u8 { 1 }"),
        ("rustix", "rustix = \"0.38\"", "rustix", "pub fn n() -> u8 { 1 }"),
        ("dlopen2", "dlopen2 = \"0.7\"", "dlopen2", "pub fn n() -> u8 { 1 }"),
        ("winsys", "windows-sys = \"0.59\"", "windows-sys", "pub fn n() -> u8 { 1 }"),
        ("zstdsys", "zstd-sys = \"2\"", "zstd-sys", "pub fn n() -> u8 { 1 }"),
        ("renamed", "c = { package = \"libc\", version = \"0.2\" }", "libc", "pub fn n() -> u8 { 1 }"),
    ] {
        let (code_, out) = effects_of(&d, name, &format!("{}{}\n", head(dep), code));
        assert_eq!((code_, out.as_str()), (Some(1), reads_as(name, every, &format!("the crate {}, which reaches the system or loads code itself (line 4)", krate)).as_str()), "{}", name);
    }
    for (name, dep, said) in [("libcx", "libcx = \"1\"", "libcx.qwer: effects: none\n"), ("sysinfo", "sysinfo = \"0.30\"", "sysinfo.qwer: effects: fs\n")] {
        let (_, out) = effects_of(&d, name, &format!("{}pub fn n() -> u8 {{ 1 }}\n", head(dep)));
        assert!(out.starts_with(said), "{}: {}", name, out);
    }
}

/// An ELF image (64 bits, little-endian) whose dynamic table needs `needed` and names itself
/// `soname`; `shift` moves the table's `p_vaddr` from where its `p_offset` is, and `far` puts
/// `DT_STRTAB` past every loaded segment.
fn elf_image(needed: &[&str], soname: Option<&str>, shift: u64, far: bool) -> Vec<u8> {
    let mut strtab = vec![0u8];
    let mut offs = Vec::new();
    for n in needed.iter().chain(soname.iter()) {
        offs.push(strtab.len() as u64);
        strtab.extend_from_slice(n.as_bytes());
        strtab.push(0);
    }
    let str_at = 64 + 2 * 56;
    let dyn_at = (str_at + strtab.len() + 7) & !7;
    let mut dynamic = Vec::new();
    let mut entry = |t: u64, v: u64| {
        dynamic.extend_from_slice(&t.to_le_bytes());
        dynamic.extend_from_slice(&v.to_le_bytes());
    };
    for o in &offs[..needed.len()] {
        entry(1, *o);
    }
    if soname.is_some() {
        entry(14, offs[needed.len()]);
    }
    entry(5, if far { 1 << 40 } else { str_at as u64 });
    entry(0, 0);
    let total = dyn_at + dynamic.len();
    let mut b = vec![0u8; 64];
    b[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    b[16..18].copy_from_slice(&3u16.to_le_bytes());
    b[32..40].copy_from_slice(&64u64.to_le_bytes());
    b[54..56].copy_from_slice(&56u16.to_le_bytes());
    b[56..58].copy_from_slice(&2u16.to_le_bytes());
    for (kind, off, va, size) in [(1u32, 0u64, 0u64, total as u64), (2, dyn_at as u64, dyn_at as u64 + shift, dynamic.len() as u64)] {
        let mut h = vec![0u8; 56];
        h[..4].copy_from_slice(&kind.to_le_bytes());
        h[8..16].copy_from_slice(&off.to_le_bytes());
        h[16..24].copy_from_slice(&va.to_le_bytes());
        h[32..40].copy_from_slice(&size.to_le_bytes());
        b.extend(h);
    }
    b.extend(strtab);
    b.resize(dyn_at, 0);
    b.extend(dynamic);
    b
}

/// What an ELF library needs is the C runtime's by its exact soname: `libc.so.6`, `ld-linux-
/// x86-64.so.2`, `libgcc_s.so.1` pass; `libc.so$ORIGIN` (which glibc expands into a path from the
/// working folder), `libc.so.7`, `libcx.so.6`, `ld-linux-x86-64.so.2$x` do not. Its dynamic table
/// is read where the loader reads it, at its `p_vaddr`: one whose `p_offset` is elsewhere, or whose
/// strings are past every loaded segment, is refused. A library that names itself a system
/// library (`DT_SONAME` `libc.so.6`, a path) is refused, one of its own name passes. And Mach-O: a
/// library whose own install name is a system library's (`LC_ID_DYLIB` `/usr/lib/libqprobe.dylib`)
/// is refused, its load commands are read inside `sizeofcmds`, and a fat one with no slice for this
/// machine is refused (src/native.rs elf_loads, macho_loads, runtime_soname, FEEDBACK83.md
/// Q-1305, found by the coverage tests; branch feedback82-85).
#[test]
fn a_library_is_read_where_its_loader_reads_it() {
    let refused = |b: &[u8]| quillo::native::loads_refused(b);
    assert_eq!(refused(&elf_image(&["libc.so.6", "ld-linux-x86-64.so.2", "libgcc_s.so.1", "libpthread.so.0"], Some("libquillo_hi_pinned.so"), 0, false)), None);
    for n in ["libc.so$ORIGIN", "libc.so.7", "libcx.so.6", "ld-linux-x86-64.so.2$x", "ld-linux-.so.2", "../libc.so.6"] {
        assert_eq!(refused(&elf_image(&[n], None, 0, false)), Some(format!("needs {}, which is not the C runtime's", n)), "{}", n);
    }
    for n in ["libc.so.6", "/opt/x/libq.so", "lib$ORIGIN.so"] {
        assert!(refused(&elf_image(&["libc.so.6"], Some(n), 0, false)).is_some_and(|w| w.starts_with(&format!("names itself {} (its DT_SONAME), a system library's name", n))), "{}", n);
    }
    assert_eq!(refused(&elf_image(&["libc.so.6"], None, 8, false)), Some("has a dynamic table (PT_DYNAMIC) whose place in the file is not where its loader reads it".to_string()));
    assert_eq!(refused(&elf_image(&["libc.so.6"], None, 0, true)), Some("is not a library this system loads".to_string()));
    let sys = (0xc, "/usr/lib/libSystem.B.dylib");
    assert_eq!(refused(&macho(&[(0xd, "libquillo_hi_pinned.dylib"), sys])), None);
    assert!(refused(&macho(&[(0xd, "/usr/lib/libqprobe.dylib"), sys])).is_some_and(|w| w.starts_with("names itself /usr/lib/libqprobe.dylib (its LC_ID_DYLIB), a system library's install name")));
    // `sizeofcmds` one short of its commands: read as dyld reads them, past it is no command.
    let mut short = macho(&[sys]);
    let size = u32::from_le_bytes(short[20..24].try_into().unwrap()) - 1;
    short[20..24].copy_from_slice(&size.to_le_bytes());
    assert_eq!(refused(&short), Some("is not a library this system loads".to_string()));
    // A fat library with one slice, for another processor than this machine's.
    let slice = macho(&[sys]);
    let other: u32 = if cfg!(target_arch = "aarch64") { 0x0100_0007 } else { 0x0100_000c };
    let mut fat = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 1];
    for v in [other, 0, 64, slice.len() as u32, 3] {
        fat.extend_from_slice(&v.to_be_bytes());
    }
    fat.resize(64, 0);
    fat.extend(&slice);
    if cfg!(any(target_arch = "aarch64", target_arch = "x86_64")) {
        assert_eq!(refused(&fat), Some("has no slice for this machine's processor".to_string()));
    }
}

/// A failed build's log is taken to say its linker lists nothing only by the linker's own line
/// (`/usr/bin/ld: unrecognized option '--dependency-file=..'`, `ld.lld: error: unknown argument ..`,
/// `mold: fatal: unknown command line option: ..`); a line of a plugin's `compile_error!` or a build
/// script's warning with the same words is not (src/native.rs linker_lists_not, FEEDBACK83.md
/// Q-1305, found by the coverage tests; branch feedback82-85).
#[test]
fn only_the_linkers_own_line_says_it_lists_nothing() {
    for line in [
        "  = note: /usr/bin/ld: unrecognized option '--dependency-file=/t/linked.list'",
        "ld.lld: error: unknown argument '--dependency-file=/t/x'",
        "          /usr/bin/x86_64-linux-gnu-ld: unrecognized option '--dependency-file=/t/x'",
        "mold: fatal: unknown command line option: --dependency-file=/t/x",
    ] {
        assert!(quillo::native::linker_lists_not(line).is_some(), "{}", line);
    }
    for line in [
        "error: unknown option --dependency-file",
        "error: ld: unrecognized option '--dependency-file=/t/x'",
        "warning: x@0.1.0: unknown option --dependency-file",
        "  = note: cc: unknown option --dependency-file",
    ] {
        assert_eq!(quillo::native::linker_lists_not(line), None, "{}", line);
    }
}

// Fix round 11

/// A foreign block a macro writes is one: `extern $abi { fn dlopen(..); }` with `foreign!("C")`,
/// `$k $abi { .. }` with `foreign!(extern "C")`, and `extern` handed in before a macro's group
/// (`foreign!(extern "C" { .. })`) each read as every effect but db, at the `extern`'s line, where
/// they read as `effects: none`. rustc compiles each. What is no foreign block still reads as
/// nothing: a function of the file's own made for C (`pub extern "C" fn`, `extern $abi fn` in a
/// macro), a pointer to one (`extern "C" fn() -> u8`), `extern crate core`, a word that holds
/// `extern` (src/confirm.rs unfollowed, FEEDBACK83.md Q-1305, found by the coverage tests; branch
/// feedback82-85).
#[test]
fn a_foreign_block_a_macro_writes_reads_as_every_effect() {
    let d = dir("fix11-foreign");
    let every = "http, fs, env, time, random, net, run";
    let said = |line: u32| format!("a foreign block (`extern \"C\" {{ ... }}`), whose functions are not Rust's to read (line {})", line);
    for (name, code, line) in [
        ("literal", "macro_rules! foreign { ($abi:literal) => { extern $abi { fn dlopen(p: *const u8, f: i32) -> *mut u8; } }; }\nforeign!(\"C\");\npub fn n() -> u8 { 1 }", 3),
        ("handed", "macro_rules! foreign { ($k:tt $abi:literal) => { $k $abi { fn dlopen(p: *const u8, f: i32) -> *mut u8; } }; }\nforeign!(extern \"C\");\npub fn n() -> u8 { 1 }", 4),
        ("group", "macro_rules! foreign { ($($t:tt)*) => { $($t)* }; }\npub fn n() -> u8 { 1 }\nforeign!(extern \"C\" { fn dlopen(p: *const u8, f: i32) -> *mut u8; });", 5),
    ] {
        assert!(rustc_compiles(&d, name, code), "{}: rustc does not compile it", name);
        let (got, out) = effects_of(&d, name, &plugin(code));
        assert_eq!((got, out.as_str()), (Some(1), reads_as(name, every, &said(line)).as_str()), "{}", name);
    }
    for (name, code) in [
        ("ownfn", "pub extern \"C\" fn n() -> u8 { 1 }"),
        ("macrofn", "macro_rules! c { ($abi:literal) => { pub extern $abi fn n() -> u8 { 1 } }; }\nc!(\"C\");"),
        ("pointer", "pub extern \"C\" fn g() -> u8 { 1 }\npub fn n() -> u8 { let f: extern \"C\" fn() -> u8 = g; f() }"),
        ("crate", "extern crate core;\npub fn n() -> u8 { 1 }"),
        ("word", "pub fn externally() -> u8 { 1 }\npub fn n() -> u8 { externally() }"),
    ] {
        assert!(rustc_compiles(&d, name, code), "{}: rustc does not compile it", name);
        assert_eq!(effects_of(&d, name, &plugin(code)), (Some(0), format!("{}.qwer: effects: none\n", name)), "{}", name);
    }
}

/// A crate that maps memory to run, runs programs, embeds an interpreter with the system at its
/// call, or calls into the system's frameworks reads as every effect but db, at its dependency's
/// line: memmap2, region, duct, mlua, core-foundation, objc2, pyo3, jni, each read as `effects:
/// none` (src/confirm.rs RUST_SYSTEM_CRATES, FEEDBACK83.md Q-1305, found by the coverage tests;
/// branch feedback82-85).
#[test]
fn a_crate_that_runs_code_or_calls_the_systems_frameworks_reads_as_every_effect() {
    let d = dir("fix11-crates");
    let every = "http, fs, env, time, random, net, run";
    for (dep, krate) in [
        ("memmap2 = \"0.9\"", "memmap2"),
        ("region = \"3\"", "region"),
        ("duct = \"0.13\"", "duct"),
        ("mlua = { version = \"0.10\", features = [\"lua54\", \"vendored\"] }", "mlua"),
        ("core-foundation = \"0.10\"", "core-foundation"),
        ("objc2 = \"0.6\"", "objc2"),
        ("pyo3 = \"0.23\"", "pyo3"),
        ("jni = \"0.21\"", "jni"),
    ] {
        let src = format!("//! version: 1.0.0\n//! effects: none\n//! [dependencies]\n//! {}\npub fn n() -> u8 {{ 1 }}\n", dep);
        let (got, out) = effects_of(&d, "c", &src);
        assert_eq!((got, out.as_str()), (Some(1), reads_as("c", every, &format!("the crate {}, which reaches the system or loads code itself (line 4)", krate)).as_str()), "{}", dep);
    }
}

// Fix round 12

/// A plugin that starts with a byte order mark is read from where rustc starts, past it: its
/// header's dependency from a folder (`evil = { path = .. }`) is refused under a hold at its line,
/// where the mark made the header no header and `quillo confirm` confirmed it, the build then
/// building that dependency; and its `//! effects: none` is read and said against its calls
/// (src/native.rs rust_text, header_lines, parse; FEEDBACK83.md Q-1305, found by the coverage tests;
/// branch feedback82-85).
#[test]
fn a_plugin_that_starts_with_a_byte_order_mark_is_read_past_it() {
    let d = dir("fix12-bom");
    let path = "\u{FEFF}//! version: 1.0.0\n//! effects: none\n//! [dependencies]\n//! evil = { path = \"/nonexistent/evil\" }\npub fn hi() -> String { \"hi\".into() }";
    write(&d, "main.qwe", "import hi.qwer use hi\nroute GET / = hi()\n");
    write(&d, "hi.qwer", &format!("{}\n", path));
    let _ = std::fs::remove_file(d.join("quillo.confirmed"));
    let (code, out) = quillo(&d, &["confirm", "--yes"]);
    assert_eq!((code, out.as_str()), (Some(1), built_in_said("the dependency `evil` from a folder (`path`)", 4, "put that code in hi.qwer").as_str()));
    assert!(!d.join("quillo.confirmed").exists());
    let src = "\u{FEFF}//! version: 1.0.0\n//! effects: none\npub fn r() -> String { std::fs::read_to_string(\"p\").unwrap_or_default() }\n";
    let (code, out) = effects_of(&d, "bomfx", src);
    assert_eq!((code, out.as_str()), (Some(1), reads_as("bomfx", "fs", "std::fs::read_to_string (line 3)").as_str()));
}

/// The Cargo.lock a pinned build's cargo wrote is judged package by package: the plugin's own crate
/// with no source, and crates.io's by its git index or its sparse one, pass; a package from a
/// folder (no source), from git, or from another registry is refused, named; a lock with no
/// package is refused (src/native.rs lock_refused, FEEDBACK83.md Q-1305, found by the coverage
/// tests; branch feedback82-85).
#[test]
fn a_builds_cargo_lock_takes_only_crates_io() {
    let lock = |pkgs: &[(&str, Option<&str>)]| -> String {
        let mut t = "# This file is automatically @generated by Cargo.\nversion = 4\n".to_string();
        for (name, source) in pkgs {
            t.push_str(&format!("\n[[package]]\nname = \"{}\"\nversion = \"1.0.0\"\n", name));
            if let Some(s) = source {
                t.push_str(&format!("source = \"{}\"\nchecksum = \"00\"\n", s));
            }
            t.push_str("dependencies = [\n \"x\",\n]\n");
        }
        t
    };
    let own = ("quillo_hi_pinned", None);
    let io = ("serde_json", Some("registry+https://github.com/rust-lang/crates.io-index"));
    let sparse = ("itoa", Some("sparse+https://index.crates.io/"));
    assert_eq!(quillo::native::lock_refused(&lock(&[own, io, sparse]), "quillo_hi_pinned"), None);
    for (pkg, said) in [
        (("evil", None), "the package evil from a folder (it has no source in Cargo.lock)"),
        (("evil", Some("git+https://example.com/evil#0123abc")), "the package evil from git+https://example.com/evil#0123abc"),
        (("evil", Some("registry+https://other.example/index")), "the package evil from registry+https://other.example/index"),
    ] {
        assert_eq!(quillo::native::lock_refused(&lock(&[own, io, pkg]), "quillo_hi_pinned"), Some(said.to_string()), "{:?}", pkg);
    }
    assert_eq!(quillo::native::lock_refused("version = 4\n", "quillo_hi_pinned"), Some("a Cargo.lock with no package in it".to_string()));
}

/// An ELF image (64 bits, little-endian) whose dynamic table holds `entries`, its strings
/// `strtab` at file offset 176 (`DT_STRTAB` 176), its PT_DYNAMIC saying it is `declared` bytes.
fn elf_dynamic(entries: &[(u64, u64)], strtab: &[u8], declared: u64) -> Vec<u8> {
    let str_at = 64 + 2 * 56;
    let dyn_at = (str_at + strtab.len() + 7) & !7;
    let mut dynamic = Vec::new();
    for (t, v) in entries {
        dynamic.extend_from_slice(&t.to_le_bytes());
        dynamic.extend_from_slice(&v.to_le_bytes());
    }
    let total = dyn_at + dynamic.len();
    let mut b = vec![0u8; 64];
    b[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    b[16..18].copy_from_slice(&3u16.to_le_bytes());
    b[32..40].copy_from_slice(&64u64.to_le_bytes());
    b[54..56].copy_from_slice(&56u16.to_le_bytes());
    b[56..58].copy_from_slice(&2u16.to_le_bytes());
    for (kind, off, size) in [(1u32, 0u64, total as u64), (2, dyn_at as u64, declared)] {
        let mut h = vec![0u8; 56];
        h[..4].copy_from_slice(&kind.to_le_bytes());
        h[8..16].copy_from_slice(&off.to_le_bytes());
        h[16..24].copy_from_slice(&off.to_le_bytes());
        h[32..40].copy_from_slice(&size.to_le_bytes());
        h[40..48].copy_from_slice(&size.to_le_bytes());
        b.extend(h);
    }
    b.extend(strtab);
    b.resize(dyn_at, 0);
    b.extend(dynamic);
    b
}

/// An ELF library's dynamic table is read as glibc and musl read it, to its DT_NULL, past what its
/// PT_DYNAMIC says it holds: a table said to be 16 bytes (its DT_STRTAB) and then a DT_RUNPATH and a
/// DT_NEEDED of a library not the C runtime's is refused; one with no DT_NULL in its segment is no
/// library this system loads; one whose DT_NULL ends it as said passes (src/native.rs elf_loads,
/// FEEDBACK83.md Q-1305, found by the coverage tests; branch feedback82-85).
#[test]
fn an_elf_librarys_dynamic_table_is_read_to_its_end() {
    let strs = b"\0/srv/app/evil\0libevil.so\0libc.so.6\0";
    let past = elf_dynamic(&[(5, 176), (29, 1), (1, 15), (0, 0)], strs, 16);
    assert_eq!(quillo::native::loads_refused(&past), Some("has a DT_RUNPATH (/srv/app/evil), where the loader looks for libraries".to_string()));
    let endless = elf_dynamic(&[(5, 176), (1, 26)], strs, 32);
    assert_eq!(quillo::native::loads_refused(&endless), Some("is not a library this system loads".to_string()));
    let fine = elf_dynamic(&[(5, 176), (1, 26), (0, 0)], strs, 48);
    assert_eq!(quillo::native::loads_refused(&fine), None);
}

/// On macOS a library's install names are judged by their folders and whether they are there: a
/// weak load of a library in dyld's shared cache (`/usr/lib/libSystem.B.dylib`, no file on disk)
/// passes; a weak load of a file that is not there is refused, as one put there later would be
/// loaded; a load of one whose folder anyone may write in (`/System/Library/AssetsV2/downloadDir`,
/// root's but 0777, where it is so here) is refused (src/native.rs macho_loads, system_folders,
/// in_shared_cache, FEEDBACK83.md Q-1305, found by the coverage tests; branch feedback82-85).
#[cfg(target_os = "macos")]
#[test]
fn a_weak_or_writable_install_name_is_refused() {
    use std::os::unix::fs::MetadataExt;
    let sys = (0xc, "/usr/lib/libSystem.B.dylib");
    assert_eq!(quillo::native::loads_refused(&macho(&[sys, (0x8000_0018, "/usr/lib/libSystem.B.dylib")])), None);
    assert_eq!(quillo::native::loads_refused(&macho(&[sys, (0x8000_0018, "/usr/lib/libquillo-not-there.dylib")])), Some("weak-loads /usr/lib/libquillo-not-there.dylib, which is not there: a file put there later would be loaded".to_string()));
    let open = Path::new("/System/Library/AssetsV2/downloadDir");
    if std::fs::metadata(open).is_ok_and(|m| m.mode() & 0o022 != 0 && m.mode() & 0o1000 == 0) {
        assert_eq!(quillo::native::loads_refused(&macho(&[sys, (0xc, "/System/Library/AssetsV2/downloadDir/x.dylib")])), Some("loads /System/Library/AssetsV2/downloadDir/x.dylib by its install name, whose folder others than the system may write in".to_string()));
    } else {
        eprintln!("skipped: {} is not a folder others may write in here", open.display());
    }
}
