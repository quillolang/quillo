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
