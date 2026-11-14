# Installing {#install}

Quillo is one binary, `quillo`. Download it built, or build it from its source with
Cargo, which needs two things on your machine: **Rust** and a **C compiler**. Everything
else it uses, Cargo downloads by itself. This chapter lists what each system needs, how to
check a machine in one command, and what is optional.

## Download

Each [release](https://github.com/quillolang/quillo/releases) has `quillo` built for:

| System | File |
|---|---|
| Linux, x86-64 | `quillo-<version>-x86_64-unknown-linux-gnu.tar.gz` |
| Linux, ARM | `quillo-<version>-aarch64-unknown-linux-gnu.tar.gz` |
| macOS, Apple silicon | `quillo-<version>-aarch64-apple-darwin.tar.gz` |
| macOS, Intel | `quillo-<version>-x86_64-apple-darwin.tar.gz` |
| Windows | `quillo-<version>-x86_64-pc-windows-msvc.zip` |

Unpack it and put `quillo` on your `PATH`. The Linux builds of 0.8.3 are linked against
glibc 2.17, and start on CentOS 7, CloudLinux 7 and 8, AlmaLinux 8 and anything newer (see
[Shared hosting](#shared-hosting)); 0.6.0's needed glibc 2.35 or later. They are built without `quillo app`
(desktop windows): build from source with `--features app` for that. Beside each
archive is a `.sha256` that checks the download:

```sh
shasum -a 256 -c quillo-v0.8.3-aarch64-apple-darwin.tar.gz.sha256   # macOS
sha256sum -c quillo-v0.8.3-x86_64-unknown-linux-gnu.tar.gz.sha256    # Linux
```

The rest of this chapter is for building it yourself.

## What you need

| | Needed for | How to get it |
|---|---|---|
| **Rust 1.94 or later** | building Quillo | [rustup.rs](https://rustup.rs), then `rustup update stable` to stay current |
| **A C compiler** | building: a few dependencies compile C (the bundled SQLite, `ring`, mimalloc) | macOS: Xcode's command line tools. Linux: gcc or clang. Windows: Visual Studio's C++ build tools |
| **git** | getting the source | [git-scm.com](https://git-scm.com), or your package manager |
| **About 1 GB of disk** | Cargo's compiled dependencies, kept in `target/` to make the next build fast. Quillo itself is a binary of 20 to 25 MB; the tests take about 6 GB more | `target/` can be deleted once `quillo` is installed |

## Check your machine

The repository has a script that checks all of this, says what to install for anything
missing, and with `--build` builds Quillo and runs the sample app to prove it works:

```sh
# macOS, Linux, WSL
sh scripts/doctor.sh --build

# Windows, in PowerShell
powershell -ExecutionPolicy Bypass -File scripts\doctor.ps1 -Build
```

It installs nothing: each line it marks *missing* comes with the command that fixes it.
It also points out a `quillo` on your `PATH` older than the one just built: that one runs
when you type `quillo`, and fails on newer code as if the code were wrong
(`cargo install --path .` replaces it).

```
To build and run Quillo
  ok       Rust 1.99.0
  ok       C compiler (Apple clang version 21.0.0)
  ok       17 GB free on disk
  ok       port 8080 is free (quillo run's default)

Optional: the tests, and extras
  ok       Node 24.11.0
  --       Chrome or Chromium: the browser tests are skipped without it
           install it, or set QUILLO_CHROME to its path
```

## macOS

```sh
# The C compiler, then Rust
xcode-select --install
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

git clone https://github.com/quillolang/quillo.git && cd quillo
sh scripts/doctor.sh --build
```

## Linux

The C compiler and git, from your distribution, then Rust from rustup (a distribution's
own Rust is often older than 1.94):

```sh
sudo apt-get install build-essential git curl    # Debian, Ubuntu
sudo dnf install gcc make git                    # Fedora
sudo pacman -S base-devel git                    # Arch

# Then Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
git clone https://github.com/quillolang/quillo.git && cd quillo
sh scripts/doctor.sh --build
```

## Windows

Rust's Windows toolchain links with Microsoft's C++ build tools. `rustup-init.exe` offers
to install them; with winget, all three come in a PowerShell window:

```sh
winget install Microsoft.VisualStudio.2022.BuildTools `
    --override "--passive --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
winget install Rustlang.Rustup
winget install Git.Git
```

Then, in a new PowerShell window (so it sees them):

```sh
git clone https://github.com/quillolang/quillo.git; cd quillo
powershell -ExecutionPolicy Bypass -File scripts\doctor.ps1 -Build
```

Quillo is built and tested on Windows in CI; macOS and Linux are where it is used most.
In [WSL](https://learn.microsoft.com/windows/wsl/install), follow the Linux steps.

## Building

```sh
cargo build --release    # the binary is target/release/quillo
cargo install --path .   # or: build it and put quillo on your PATH
```

Without a clone, Cargo can fetch, build and install it in one step:

```sh
cargo install --git https://github.com/quillolang/quillo quillo
```

Then run the sample app in the clone, `quillo run app/main.qwe --watch`, and open
http://localhost:8080. `--watch` is an option of `quillo run`: it reloads the program
and refreshes its pages when a file changes. [The options](#running-quillo) lists the others.

## What Cargo downloads

The first build fetches Quillo's Rust dependencies from crates.io and builds them; none of
them is installed by hand, and none needs a library from the system:

| | Crates |
|---|---|
| HTTP and waiting | `mio`, `httparse`, `tokio`, `ureq` (requests a program makes) |
| Databases | `rusqlite` with SQLite built in, `tokio-postgres` and `postgres`, `mysql_async` |
| TLS | `rustls` and `webpki-roots`: no OpenSSL |
| Compiled numbers | `cranelift-*`, the code generator for [compiled numbers](#compiled-numbers) |
| Real time | `wtransport`, for WebTransport |
| The rest | `image`, `pulldown-cmark` (Markdown), `argon2` and `sha2` (passwords, hashes), `serde_json`, `flate2`, `libmimalloc-sys` (the allocator) |

Three of them are features, on by default, that a smaller build can leave out:

| Feature | Brings | Without it |
|---|---|---|
| `mysql` | MySQL and MariaDB for `db` | SQLite and PostgreSQL still work |
| `jit` | compiled numbers (Cranelift) | arithmetic runs in the interpreter, with the same answers |
| `realtime` | WebTransport for channels and media | pages use a WebSocket |

```sh
# For example, without MySQL and WebTransport:
cargo build --release --no-default-features --features jit
```

One more, `app`, is off by default: `cargo build --release --features app` adds
`quillo app`, a program in a native window (`wry`, `tao`, `muda`).

## Optional: the tests, and extras

| | Needed for | Found by |
|---|---|---|
| **Node 22 or later** | the component compiler's tests, the browser tests, the [VS Code extension](https://github.com/quillolang/editors); the money tests compare with Node 24's CLDR data, and skip on older ones | `node` on your PATH |
| **Chrome or Chromium** | the browser tests (skipped without it) | the usual places, or `QUILLO_CHROME` |
| **PostgreSQL** | its tests, which start a server of their own (skipped without it) | `initdb` and `pg_ctl` on your PATH or in Homebrew's folders, or `QUILLO_TEST_PG_BIN` |
| **MariaDB** | its tests, the same way | `mariadbd` and `mariadb-install-db`, or `QUILLO_TEST_MYSQL_BIN` |
| **limer** | the tests of several servers behind it | `LIMER`, or `../limer/limer` |
| **The network** | the Rust (`.qwer`) tests, whose first build fetches a crate; they are not skipped | crates.io |
| **WebKitGTK 4.1, GTK 3** (Linux) | `quillo app` | `sudo apt-get install libwebkit2gtk-4.1-dev libgtk-3-dev` |
| **WebView2** (Windows) | `quillo app` | part of Windows 11; [an installer](https://developer.microsoft.com/microsoft-edge/webview2/) for Windows 10 |

`cargo test` runs everything it can and says what it skipped; `QUILLO_TEST_LOG=1` shows
the output of the servers the tests start.

## Which systems

Releases are built for Linux and macOS, on x86-64 and ARM, and for Windows (see
[Download](#download)). On Windows, `quillo dap` (the debugger) is not there yet, and
`quillo run`'s `--with` option (`quillo run main.qwe --with "npx tailwindcss ..."`,
a command run beside the program) needs a `sh` on the `PATH`.

## Running a program

The `quillo` binary is all a program needs to run (see [Hosting](#hosting) for
putting one online): SQLite is built in. A program that
uses PostgreSQL or MySQL needs a server to connect to, like any program. One with
[Rust files](#rust) (`.qwer`) builds them with Cargo the first time it starts, so
it needs Rust there too, unless you build them beforehand with `quillo build app.qwe` and
deploy `.quillo/native/` with it.
