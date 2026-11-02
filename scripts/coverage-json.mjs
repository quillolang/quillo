// target/coverage/coverage.json from a run of scripts/coverage.sh: the commit
// measured, the tests run, Rust's lines by file (cargo llvm-cov's summary) and
// runtime.js's. quillolang.org's /coverage/ page shows it, and keeps each run
// in its history (the website's tools/coverage.mjs).
//
//   node scripts/coverage-json.mjs target/coverage

import { readFileSync, writeFileSync, existsSync } from "fs";
import { execFileSync } from "child_process";
import { join, relative } from "path";

const out = process.argv[2];
if (!out) {
  console.error("usage: node scripts/coverage-json.mjs <coverage folder>");
  process.exit(2);
}
const root = join(import.meta.dirname, "..");
const git = (...a) => execFileSync("git", ["-C", root, ...a], { encoding: "utf8" }).trim();

// llvm-cov's summary: the total, and each file of src/ (the tests' own files left out).
const rust = JSON.parse(readFileSync(join(out, "rust.json"), "utf8")).data[0];
const lines = (s) => ({ lines: s.lines.count, run: s.lines.covered, percent: Number(s.lines.percent.toFixed(2)) });
const files = rust.files
  .map((f) => ({ file: relative(root, f.filename), ...lines(f.summary) }))
  .filter((f) => f.file.startsWith("src/"))
  .sort((a, b) => a.file.localeCompare(b.file));

// The tests: each binary's "test result" line.
let passed = 0;
let failed = 0;
const log = existsSync(join(out, "tests.log")) ? readFileSync(join(out, "tests.log"), "utf8") : "";
for (const m of log.matchAll(/^test result: \w+\. (\d+) passed; (\d+) failed/gm)) {
  passed += Number(m[1]);
  failed += Number(m[2]);
}

// A copy made by `git archive` has no .git: COVERAGE_COMMIT and COVERAGE_BRANCH name what it is.
const inGit = !process.env.COVERAGE_COMMIT;
const dirty = inGit && git("status", "--porcelain", "--untracked-files=no", "--", "src", "tests").length > 0;
const report = {
  commit: inGit ? git("rev-parse", "--short", "HEAD") : process.env.COVERAGE_COMMIT,
  branch: inGit ? git("rev-parse", "--abbrev-ref", "HEAD") : process.env.COVERAGE_BRANCH || "",
  changed: dirty,
  date: new Date().toISOString().slice(0, 10),
  version: JSON.parse(execFileSync("cargo", ["metadata", "--no-deps", "--format-version", "1"], { cwd: root, encoding: "utf8" })).packages[0].version,
  system: `${{ darwin: "macOS", linux: "Linux", win32: "Windows" }[process.platform] ?? process.platform} ${process.arch}`,
  tests: { passed, failed },
  rust: { ...lines(rust.totals), files },
  runtime: JSON.parse(readFileSync(join(out, "runtime.json"), "utf8")),
};
writeFileSync(join(out, "coverage.json"), JSON.stringify(report, null, 1) + "\n");
console.log(`coverage.json: Rust ${report.rust.percent}%, runtime.js ${report.runtime.percent}%, ${passed} tests passed, ${failed} failed, at ${report.commit}${dirty ? " with changes" : ""}`);
