#!/bin/sh
# Both coverage numbers from one run of the tests: Rust's by cargo llvm-cov,
# and src/client/runtime.js's by the browser and Node (scripts/runtime-coverage.sh
# explains how). Writes target/coverage/: coverage.json (what quillolang.org's
# /coverage/ page shows, through the website's tools/coverage.mjs), the
# llvm-cov report and lcov, runtime.js's lcov and summary, and the test log.
#
#   sh scripts/coverage.sh
#
# macOS, cargo-llvm-cov 0.9: continuous mode with page-aligned sections, so a
# server the tests kill still leaves its profile, and always from a deleted
# target dir (COVERAGE_TARGET, default target/llvm-cov-target), or old
# binaries count too. Needs Chrome, PostgreSQL, MariaDB
# and the network, as the full suite does. Fails if a test fails, if
# runtime.js is below 99.50%, or if a line of runtime.js changed since BASE
# (default main) runs in no test. In a copy made by `git archive`, set
# COVERAGE_COMMIT (and COVERAGE_BRANCH) to the commit it is.
set -u
cd "$(dirname "$0")/.."
ROOT=$PWD
OUT=$ROOT/target/coverage
T=${COVERAGE_TARGET:-$ROOT/target/llvm-cov-target}
rm -rf "$T" "$OUT"
mkdir -p "$OUT/records"
eval "$(CARGO_TARGET_DIR=$T cargo llvm-cov show-env --sh --release 2>/dev/null | grep -E '^export (__CARGO|RUSTC_WRAPPER|CARGO_LLVM)')"
export CARGO_TARGET_DIR=$T
export LLVM_PROFILE_FILE="$T/quillo-app1-%8m%c.profraw"
export RUSTFLAGS='-Clink-arg=-Wl,-sectalign,__DATA,__llvm_prf_cnts,0x4000 -Clink-arg=-Wl,-sectalign,__DATA,__llvm_prf_bits,0x4000 -Clink-arg=-Wl,-sectalign,__DATA,__llvm_prf_data,0x4000 -Clink-arg=-Wl,-sectalign,__DATA,__data,0x4000'
export QUILLO_TEST_WINDOW=1
cargo test --release --all-features --no-run || exit 1
rm -f "$T"/*.profraw
QUILLO_RUNTIME_COVERAGE="$OUT/records" NODE_OPTIONS="--require $ROOT/tests/browser/rtcover-node.js" \
  cargo test --release --all-features --no-fail-fast >"$OUT/tests.log" 2>&1
tests=$?
cargo llvm-cov report --release >"$OUT/rust.txt" 2>/dev/null
cargo llvm-cov report --release --json --summary-only --output-path "$OUT/rust.json" 2>/dev/null
cargo llvm-cov report --release --lcov --output-path "$OUT/rust.lcov" 2>/dev/null
# A `git archive` copy has no history to compare with.
diff=""
[ -e .git ] && diff="--diff ${BASE:-main}"
node tests/browser/rtcover.js report "$OUT/records" --min "${RUNTIME_COVERAGE_MIN:-99.50}" \
  --lcov "$OUT/runtime.lcov" --json "$OUT/runtime.json" $diff >"$OUT/runtime.txt" 2>&1
cover=$?
node scripts/coverage-json.mjs "$OUT" || exit 1
tail -1 "$OUT/rust.txt"
cat "$OUT/runtime.txt"
[ "$tests" -eq 0 ] || echo "some tests failed (see $OUT/tests.log): fix them before reading the numbers"
[ "$tests" -eq 0 ] && [ "$cover" -eq 0 ]
