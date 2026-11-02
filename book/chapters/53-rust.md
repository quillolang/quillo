# Rust

A `.qwer` or `.rs` file is Rust, and a program `use`s it as it uses a `.qwe` file: its
top-level `pub fn`s are the program's functions. The two extensions are the same to
Quillo: `.rs` is the one editors, `rustfmt` and clippy know, and `.qwer` is the catalog's
and `quillo install`'s. A `.rs` file is the program's only when a `use` or `import` names
it (`use "imaging.rs"`, `import imaging.rs use brightness`), so a crate's own `src/*.rs`
beside the program is left alone. Here is `imaging.qwer`:

```rust
//! effects: fs
//! [dependencies]
//! image = { version = "0.25", default-features = false, features = ["jpeg"] }

/// How bright a picture is, from 0 to 255.
pub fn brightness(path: &str) -> Result<f64, String> {
    let img = image::open(path).map_err(|e| e.to_string())?.to_luma8();
    Ok(img.pixels().map(|p| p.0[0] as f64).sum::<f64>() / img.len() as f64)
}
```

```quillo
use "imaging.qwer"

route GET /photos/:name = { brightness: brightness(`photos/${name}.jpg`)? }
```

- Values cross as Quillo has them: `i64` and the other integers up to 64 bits are `int`
  (a `u64` or `usize` answer past `i64`'s range is a runtime error; `i128` cannot
  cross); `f64` and `f32` are `float`; `bool`; `String`, `&str` and `char` are `str` (a
  `char` argument must be one character); `Vec<T>`, `VecDeque<T>` and `&[T]` are lists;
  `HashMap<String, T>` and `BTreeMap<String, T>` are maps, nested too; `Option<T>` is the
  value or `null`; `serde_json::Value` is anything. A function returning
  `Result<T, E>` gives `T`, or an error value with `E`'s message, so `?` and `??` work
  on it. `()` is `null`. References (`&String`, `&Vec<T>`) are fine as parameters.
  A `u64` past `i64`'s range inside a returned `serde_json::Value` is a runtime error
  that names where it is (`.id`, `[1].n`), not a float that has lost digits.
- An argument that does not fit is refused with the parameter's name: `` `small`:
  argument `n` does not fit in i32: 5000000000 ``. Values cross at most 126 levels deep:
  an argument nested deeper is refused (`` `deep`: argument `v` is nested too deeply: a
  plugin takes values at most 126 levels deep ``), and an answer nested deeper is an
  error that says so (`` `pr_deep` (w.qwer) gave back a value nested too deeply ``).
- What does not cross: structs (even with `#[derive(Deserialize)]`: take a
  `serde_json::Value` and `serde_json::from_value` it inside), tuples, sets, `Box`,
  `Rc`, `Arc`, `PathBuf`, `Cow`. A decimal arrives as its text, so an `f64` parameter
  refuses one. Bytes are a list of numbers each way (a 10 MB file is about 40 MB of
  JSON): pass a file's path, or base64 text, instead.
- The rest of the file is ordinary Rust: private functions, structs, `impl`s, modules,
  `use`. A `pub fn` that is generic (a written lifetime counts: `fn f<'a>`), `async`,
  takes `&mut`, or has a type that cannot cross, as a fixed-size array cannot, is not
  exported, and `quillo check` says why. Another crate's type named `Value` passes the
  check and fails the build.
- The `//! [dependencies]` lines are the file's crates, as in `Cargo.toml`: any crate
  from crates.io or git. They come first, after other `//!` lines if any; the first line
  that is not `//!` ends them. Only that table is read (no `[features]` table, no
  `[patch]`, no `build.rs` of the file's own; a crate's own build scripts do run).
  `serde_json` is always there. C libraries come through `-sys` crates.
- **`//! effects: fs, http`**, a header line, says what the file's functions do, in the
  words of `with` (`db`, `http`, `fs`, `env`, `time`, `random`, `net`; `db.analytics` is
  `db`), or `//! effects: none`. It is what lets a `with` function call them: a function
  declared `with fs` may call `brightness`, one declared `with time` may not. A file
  without the line may do anything: its functions are refused inside any `with`, and
  anywhere under a policy that narrows effects (see [Effects](#effects)). A word that is
  not an effect is said by `quillo check` at its line. Quillo cannot see what Rust does,
  so the line is the author's claim, read as a promise: write what the code really
  reaches, and review it in a plugin from elsewhere.
- **Taking names.** `import redis.qwer use cache_get, cache_set` takes only those
  functions; `as` gives one another name, `import redis.qwer use cache_get as redis_get`,
  and it is not there under its own. A name the file does not export is an error that
  names the nearest it does. `import redis.qwer use *`, or `use redis.qwer`, takes them
  all. Each file takes what it calls, as with a `.qwe` file (see [Files of a
  program](#files-of-a-program)).
- Two `.qwer` files exporting one name: the last `use`d replaces the other, with a
  warning that says how to take one under another name.
- A `pub fn` named like a builtin (`len`, `get`) hides the builtin in the files that
  take it, and `quillo check` warns of it at the `.qwer` file's line, with how to take it
  under another name: `import x.qwer use len as x_len` (FEEDBACK27.md, Q-326).
- `quillo check` reads the functions' types from the Rust, without building anything:
  calls are checked, and editors show them on hover. The language server also runs
  `cargo check` on each save of a `.qwer` file, and shows Rust's errors in the editor.
  They run on the server only (in services and mounted programs too); components call
  them through a `server fn`.
- A Rust function cannot call back into Quillo: no `db`, `publish` or `env()` (it reads
  `std::env` itself). It may start threads, keep a runtime of its own in a `static` and
  `block_on` it, and hold a connection pool in a `static`.

How it runs:

- The Rust files the program uses that are not built yet are built before its first
  statement runs, all in one cargo call over a workspace, so cargo builds them side by
  side (`quillo: building ./a.rs, ./b.qwer together`); a file that fails there is built
  again alone by its `use`, which says why.
- `use` builds the file with `cargo` (`$CARGO`, or the one on the `PATH`), in a crate of
  its own in Quillo's cache: `QUILLO_NATIVE_CACHE`, else `$XDG_CACHE_HOME/quillo/native`,
  else `~/.cache/quillo/native`, else the system's temporary folder. The crates' builds
  are shared there, and nothing old is removed (it grows: clear it now and then). The
  library is kept in `.quillo/native/` next to the file, named by a hash of the file's
  text, the glue's version and the processor: the next start loads it without building.
  A build takes seconds the first time, more with large crates, and needs crates.io
  (or cargo's own offline mode).
- `quillo build app.qwe` builds every Rust file the program and its mounted programs
  use (not those of its services: build each service's file too), and prints where each
  library went; it also takes a `.qwer` or `.rs` file itself. Deploy `.quillo/native/` with the
  program, and the server needs no Rust: it loads what was built for that content, on
  the same processor and system (there is no cross-compiling: build on Linux for
  Linux), by the same Quillo glue. A Quillo whose glue changed builds again, and needs
  cargo then.
- **`.quillo/plugins.lock`**, beside `.quillo/native/`, holds each library's file name
  and SHA-256, a line each, written by every build: `quillo build`, a `use` that built,
  [`--watch`](#options-of-quillo-run). Deploy it with `.quillo/native/`. A library is loaded only when it is the one
  built: one whose hash differs, or that has no line, is built again where cargo may run
  (`quillo: libhi-687dd0b24f29e5cf.dylib is not the library that was built (its SHA-256
  differs from .quillo/plugins.lock's): building it again`), and refused where it may
  not. A folder built before there were locks has none: its libraries load as before, with
  a warning once (`.quillo/plugins.lock is not there, so the libraries in .quillo/native
  are loaded unchecked, as before; quillo build writes it`). A build writes the lock, but
  a library already there is not built again, so remove `.quillo/native/` and run
  `quillo build` to get one. Whoever can write both the library and the lock can still
  run code in the server.
- **[`--no-build`](#options-of-quillo-run)** never runs cargo, so no crate's build script runs on the server: a
  library is loaded only as the lock names it, and a file not built is an error
  (`./hi.qwer is not built, and --no-build says not to build it here: build it with
  quillo build and bring .quillo along`). Under it, a folder with no lock loads nothing.
  [`--split`](#options-of-quillo-run) passes it to its services, and a policy's `[plugins] build = false` is the
  same (see [The policy](#the-policy)).
- **A policy's `[plugins] allow`** names the plugins a program may `use`: `"csv"` any
  version, `"csv@1"` or `"csv@1.4.2"` by the file's `//! version:` line (`1` takes
  `1.4.2`, not `10.0`). A plugin it does not name is refused before anything is built:
  `` the plugin `pdf` 2.0.1 is not in the plugins /etc/quillo/policy.toml allows: csv@1, qr ``.
  It names plugins by their file's name and header; it does not prove their code.
- A call runs off the event loop, without the lock Quillo code holds, on the threads
  that also wait for databases ([`--workers`](#options-of-quillo-run)): a slow function holds up no one, and
  calls from several requests run on several cores at once; many long calls at once
  can keep database waits waiting. Inside a SQLite transaction a call holds the lock,
  and every request waits. Nothing times a call out or cancels it. The values cross as
  JSON, which suits most calls; a list of millions of numbers takes a while to cross.
- A panic is a runtime error in the request that called the function (not an error
  value: `??` does not catch it), and the server goes on. `std::process::exit`, an
  abort, or a stack overflow ends the whole server. Arguments of the wrong type are
  refused before the function runs. A stop (`SIGTERM`) while a request is in a call is
  logged as a stop, `quillo: stopped with GET /slow?ms=14000 still running`, not as a bug
  in Quillo.
- When the program is started with [`--watch`](#options-of-quillo-run) or [`--reload`](#options-of-quillo-run)
  (`quillo run main.qwe --watch`), a Rust file that changes is built again and the
  program reloaded with it. A build that fails prints the compiler's errors, and the
  program goes on as it was. Requests in progress finish with the version they started
  with: libraries stay loaded, the old ones for the life of the process, with their
  `static`s (an old pool stays open).
- Rust can do anything the server can, so Rust files are the developers', like the
  rest of the program: never let a program's users write them (they write sandboxed
  Quillo with `run_script`).

See [rust.qwe](https://github.com/quillolang/examples/blob/main/rust.qwe) and [rust.qwer](https://github.com/quillolang/examples/blob/main/rust.qwer).

## Plugins from the catalog

[quillolang.org/plugins](https://quillolang.org/plugins/) has `.qwer` files ready to use,
each with a version, an example, and a test run before it is published. `quillo install`
puts them beside the program (its [options](#other-commands) are below):

```
quillo install csv qr jwt          # csv.qwer, qr.qwer and jwt.qwer, into this folder
quillo install csv --example       # and csv.qwe, its example program
quillo install --update            # the plugins here that the catalog has newer
quillo install --list pdf          # what the catalog has, matching "pdf"
```

- It reads the catalog (`https://quillolang.org/plugins/index.json`; [`--registry`](#other-commands) or
  `QUILLO_REGISTRY` for another), downloads each file, and writes it only when its
  SHA-256 and the `//! version:` line of its header match the catalog's. A plugin
  that fails leaves the others going, and the exit status is 1.
- A file already there is left alone when it is the same, replaced when the catalog's
  version is newer, and kept when it is newer or the same version changed by hand:
  [`--force`](#other-commands) replaces it. [`--dir DIR`](#other-commands) installs elsewhere. `name@1.2.0` asks for that
  version, and fails when the catalog has another (it keeps only the newest).
- Then `use csv.qwer`, or only what you need, `import csv use csv_parse, csv_write`: the
  first run builds it with cargo, as any `.qwer` file.
- A catalog plugin is called inside a `with` function only once its header has a
  `//! effects:` line, as any Rust file (see [Rust](#rust)). The catalog's files do not
  have it yet: add it to your copy after reading what the plugin does.
- `install` checks a file against the catalog it came from, which a changed server
  could change too: the catalog's entries are not signed yet. A host can still list the
  plugins and versions it takes, in a policy's `[plugins] allow`.

## Other stores

`db` is for SQL databases. For another kind (Redis, MongoDB, a graph database), use its
crate in a `.qwer` file: its functions are the driver, and the program calls them like
any other. A count of visits in Redis:

```rust
//! effects: db, env
//! [dependencies]
//! redis = "0.27"

use std::sync::OnceLock;

/// The client, made on the first call (`REDIS_URL`, or Redis on this machine).
fn client() -> Result<&'static redis::Client, String> {
    static C: OnceLock<redis::Client> = OnceLock::new();
    if let Some(c) = C.get() {
        return Ok(c);
    }
    let url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1/".into());
    let c = redis::Client::open(url).map_err(|e| e.to_string())?;
    Ok(C.get_or_init(|| c))
}

/// The value at `key`, or null.
pub fn cache_get(key: &str) -> Result<Option<String>, String> {
    let mut con = client()?.get_connection().map_err(|e| e.to_string())?;
    redis::cmd("GET").arg(key).query(&mut con).map_err(|e| e.to_string())
}

/// Adds one to the number at `key`, and gives the new number.
pub fn cache_incr(key: &str) -> Result<i64, String> {
    let mut con = client()?.get_connection().map_err(|e| e.to_string())?;
    redis::cmd("INCR").arg(key).query(&mut con).map_err(|e| e.to_string())
}
```

```quillo
use "redis.qwer"

server fn visits(page: str) {
    watch(`visits:${page}`)                  // live pages showing this follow the topic
    int(cache_get(`visits:${page}`)? ?? "0")
}

server fn visit(page: str) {
    let n = cache_incr(`visits:${page}`)?
    publish(`visits:${page}`)                // and refresh when it changes
    n
}

component Visits(page: str) {
    state n = live(visits, page)
    <button @click={() => visit(page)}>{n} visits</button>
}
```

- Values cross as JSON, so a connection cannot be a Quillo value: the Rust keeps it in a
  `static`, and the library stays loaded between calls. (This one connects on each call;
  a pool, such as the crate's `r2d2` feature, keeps connections open.)
- Calls run off the event loop, so a slow store holds up no one, and a store that is
  down gives an error value.
- What `db` does by itself is the program's to do here: live pages follow what a loader
  `watch`es and a write `publish`es, as above, and writes are not part of
  `db.transaction`.
- `//! effects: db, env` makes `cache_get` and `cache_incr` the `db` effect (and `env`,
  for `REDIS_URL`), so a function declared `with db, env` may call them and one declared
  `with http` may not.
