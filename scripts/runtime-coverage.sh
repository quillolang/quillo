#!/bin/sh
# What of src/client/runtime.js the tests run, by line; fails below 99.50%,
# or when a line changed since BASE (default main) runs in no test.
#
#   sh scripts/runtime-coverage.sh [cargo test arguments]
#
# e.g. `sh scripts/runtime-coverage.sh --release`, or one file with
# `--release --test browser`. While the tests run, quillo ships the runtime as
# written (QUILLO_RUNTIME_COVERAGE), headless Chrome (tests/browser/run.js) and
# every Node (tests/browser/rtcover-node.js) record what ran, and
# tests/browser/rtcover.js sums it up. Writes target/runtime-coverage/lcov.info
# and summary.txt. Needs Chrome and Node 22 or later, as the browser tests do.
set -u
cd "$(dirname "$0")/.."
OUT=${RUNTIME_COVERAGE_OUT:-$PWD/target/runtime-coverage}
MIN=${RUNTIME_COVERAGE_MIN:-99.50}
BASE=${BASE:-main}
rm -rf "$OUT"
mkdir -p "$OUT/records"
QUILLO_RUNTIME_COVERAGE="$OUT/records" NODE_OPTIONS="--require $PWD/tests/browser/rtcover-node.js" \
  cargo test --no-fail-fast "$@"
tests=$?
node tests/browser/rtcover.js report "$OUT/records" --min "$MIN" --lcov "$OUT/lcov.info" --diff "$BASE" >"$OUT/summary.txt" 2>&1
cover=$?
cat "$OUT/summary.txt"
[ "$tests" -eq 0 ] || echo "some tests failed (cargo test exited $tests): fix them before reading the number"
[ "$tests" -eq 0 ] && [ "$cover" -eq 0 ]
