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
        quillo::confirm::built_in_refused("hi.qwer", &src),
        Some(format!("hi.qwer builds another file into it ({}), which the SHA-256 its source is held to would not cover, so it does not load (write that code in Rust, without assembly)", how))
    );
    assert_eq!(quillo::confirm::built_in_refused("hi.qwer", &plugin("pub fn hi() -> i64 { 42 }")), None);
    assert_eq!(quillo::confirm::built_in_refused("hi.qwer", ""), None);
}

/// An attribute that has the linker read a library in is refused under a hold, however it is
/// written: `#[link(...)]`, in a `cfg_attr`, raw (`r#link`), spaced, in a module; one a macro is
/// given (`#[$a(...)]` with `m!(link)`, `#[$m]` with a `meta`); one made of a `#` a macro is given
/// (`$h [ $a (...) ]` with `m!(# link)`); and `#![debugger_visualizer]`, a file rustc reads in.
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
        ("givenname", "macro_rules! m { ($a:ident) => { #[$a(name = \"evil\")] extern \"C\" { fn f(); } } }\nm!(link);", given, 3, instead),
        ("meta", "macro_rules! m { ($m:meta) => { #[$m] extern \"C\" { fn f(); } } }\nm!(link(name = \"evil\"));", given, 3, instead),
        ("lone", "macro_rules! m { ($h:tt $a:ident) => { $h [ $a (name = \"evil\") ] extern \"C\" { fn f(); } } }\nm!(# link);", "a `#` that starts no attribute, handed to a macro, which can make one of it, `#[link(...)]`: a library the linker reads in", 4, instead),
        ("visualizer", "#![debugger_visualizer(gdb_script_file = \"x.py\")]", "`#[debugger_visualizer(...)]`, a file rustc reads in", 3, "leave the visualizer out"),
    ];
    for (name, code, why, line, instead) in &cases {
        assert!(rustc_compiles(&d, name, code), "{}: rustc does not compile it", name);
        assert_eq!(confirm_plugin(&d, code), (Some(1), built_in_said(why, *line, instead)), "{}", name);
        assert!(!d.join("quillo.confirmed").exists(), "{}", name);
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
