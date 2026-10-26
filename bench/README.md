# Benchmarks

Two suites: the browser side (components updating the page) against plain
JavaScript and other frameworks, and the server side (answering requests)
against Node and Bun.

## In the browser: `bench/ui`

The [js-framework-benchmark](https://github.com/krausest/js-framework-benchmark)
app, keyed: a table of rows and buttons for the usual operations, in Quillo
(`quillo.qwe`), plain JavaScript (`vanilla.html`, as vanillajs-keyed writes it)
and, under `frameworks/`, Solid, Svelte 5, Vue 3, Preact and React 19, each
written as that framework's own entry writes it and built for production with
its own compiler. All have the same markup, labels and button ids.
`driver.js` clicks the buttons in headless Chrome and times each operation from
the click until the update is done and laid out (and, in the `…Js` rows, until
the update is done, before layout; small operations are timed 20 at a time).
Medians of 15 runs per page load, then the median of the loads.

```sh
cargo build --release
(cd bench/ui/frameworks && npm install --ignore-scripts && node build.mjs)
node bench/ui/run.js 3                      # or: node bench/ui/run.js 3 vanilla,quillo,solid
```

On an Apple M-series laptop (10 cores), Chrome headless, October 2026:

| | vanilla | **Quillo** | Solid | Svelte | Vue | Preact | React |
|---|---:|---:|---:|---:|---:|---:|---:|
| create 1,000 rows | 10.7 | **11.1** | 10.5 | 10.7 | 12.3 | 13.6 | 12.2 |
| replace 1,000 rows | 11.1 | **11.9** | 11.7 | 12.0 | 13.1 | 15.7 | 15.3 |
| update every 10th row | 2.3 | **2.5** | 2.4 | 2.3 | 3.4 | 3.0 | 2.7 |
| select a row | 0.1 | **0.1** | 0.1 | 0.2 | 1.0 | 0.4 | 0.3 |
| swap two rows | 0.9 | **0.9** | 0.9 | 1.0 | 1.8 | 11.5 | 12.0 |
| remove a row | 0.9 | **0.9** | 0.9 | 0.9 | 1.9 | 1.1 | 1.1 |
| create 10,000 rows | 119 | **131** | 122 | 127 | 139 | 159 | 216 |
| append 1,000 rows | 12.0 | **13.0** | 12.4 | 12.6 | 15.1 | 16.7 | 14.5 |
| clear 1,000 rows | 1.0 | **1.2** | 1.2 | 1.3 | 1.4 | 1.4 | 3.6 |
| **geometric mean** (over the fastest) | 1.00 | **1.07** | 1.04 | 1.15 | 1.78 | 1.94 | 2.08 |
| script only, geometric mean | 1.00 | **2.08** | 1.83 | 2.12 | 9.11 | 6.14 | 6.94 |
| JS heap with 1,000 rows (MB) | 0.78 | **1.86** | 1.71 | 2.05 | 3.34 | 3.17 | 3.71 |
| scripts, compressed (KB) | 0 | **13.1** | 6.5 | 13.1 | 64.1 | 8.5 | 58.2 |

Times in milliseconds. Quillo's script includes its whole runtime (standard
library, resuming the server's HTML, live updates), compressed.

## On the server: `bench/server`

The same four answers from Quillo (`quillo.qwe`), Node's `http` (`node.js`)
and `Bun.serve` (`bun.js`): plain text, JSON, TechEmpower's Fortunes without a
database (the list, one more added per request, sorted, as an escaped HTML
table; computed on every request) and a loop of arithmetic in the language
itself. `load.rs` (built with `rustc -O` on first use) keeps 64 connections
busy from 4 threads on the same machine.

```sh
cargo build --release
node bench/server/run.cjs 8
```

Requests a second (p99 latency in ms), same machine:

| one process | Quillo | Node 24 | Bun 1.4 |
|---|---:|---:|---:|
| text | **243k** (0.4) | 157k (0.7) | 228k (0.5) |
| JSON | **235k** (0.3) | 149k (0.8) | 228k (0.5) |
| fortunes | **139k** (0.5) | 86k (1.4) | 168k (0.8) |
| compute (10,000-step loop) | **1.0k** (72) | 24.7k (4.8) | 23.7k (5.0) |

Each of them keeps about one core busy. Quillo is a tree-walking interpreter:
code that loops in the language is some 25 times slower than in a JavaScript
JIT, while what it does natively (HTTP, JSON, rendering HTML) is as fast as Bun.
With 6 processes (`--instances 6`, Node's cluster, Bun's `reusePort`) the
load generator, on the same 4 performance cores, is what limits text and JSON
(about 190–230k for all three); macOS does not spread connections across
`reusePort` sockets, so Bun stays at one process's figures there.
