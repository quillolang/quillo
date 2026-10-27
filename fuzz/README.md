# Fuzzing

Targets for [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz), which needs the
nightly toolchain:

```sh
cargo install cargo-fuzz --locked
fuzz/seed.sh                                   # starting inputs, in fuzz/corpus/
cargo +nightly fuzz run parse -- -max_total_time=180 -rss_limit_mb=2048
```

| Target | What it feeds |
|---|---|
| `parse` | Any text to the lexer and parser |
| `check` | Programs that parse, to the checker |
| `http` | Request bytes, arriving in pieces, to the request parser and chunked decoding |
| `json` | JSON text to values and back |
| `decimal` | Text to `dec`, which must read back as written |
| `decimal_ops` | `+ - *` and comparison of decimals, against a big-integer reference |

What a target finds is kept in `fuzz/artifacts/<target>/`; run it again with
`cargo +nightly fuzz run <target> fuzz/artifacts/<target>/<file>`. Each bug found gets a
test in the ordinary suite (`tests/`), so it stays fixed without the fuzzer.

The hooks the targets use are compiled only under `cfg(fuzzing)`, which cargo-fuzz sets.
