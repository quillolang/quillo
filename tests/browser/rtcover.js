// Coverage of src/client/runtime.js (scripts/runtime-coverage.sh).
//
// While QUILLO_RUNTIME_COVERAGE names a folder, quillo ships the runtime as
// written rather than minified, each declaration after a line `//@rt N`
// naming the line of runtime.js it starts on (`runtime_into` in
// src/client.rs). Chrome (run.js) and every Node the tests start
// (rtcover-node.js) record which ranges of their scripts ran; this maps them
// back to lines of runtime.js and writes the lines that ran to that folder.
//
//   node rtcover.js report <folder>[,<folder>...] [--min 99.5] [--lcov file] [--json file] [--diff git-rev]
//
// sums up every record there: the share of runtime.js's lines that ran, the
// lines that did not (also as lcov, and as JSON for the coverage page), and
// with --diff, the lines changed since git-rev that did not. It fails below --min, or when a changed line did not run.
//
// A line counts unless it is blank, a comment, or only brackets and
// punctuation (V8 says a `}` after a `return` did not run). It ran when the
// innermost range V8 reports around its first character ran.

"use strict";
const fs = require("fs");
const path = require("path");

const FILE = path.join(__dirname, "../../src/client/runtime.js");
const TEXT = fs.readFileSync(FILE, "utf8");
const LINES = TEXT.split("\n");

const isDev = (l) => l.trimEnd().endsWith("// dev");
const firstCol = (l) => l.length - l.trimStart().length;

function counted(l) {
  const t = l.trim();
  return t !== "" && !t.startsWith("//") && /[^\s{}()[\];,]/.test(t);
}

/// For each line of runtime.js in a script: [offset of its first character
/// in the script, its line in runtime.js]. Null if the script has none.
function mapScript(src) {
  const out = [];
  // The runtime as written: development bundles, or a test reading the file.
  const at = src.indexOf(TEXT.trimEnd());
  if (at >= 0) {
    let off = at;
    LINES.forEach((l, i) => {
      out.push([off + firstCol(l), i + 1]);
      off += l.length + 1;
    });
    return out;
  }
  if (!src.includes("//@rt ")) return null;
  let off = 0;
  let next = null;
  for (const line of src.split("\n")) {
    const m = /^\/\/@rt (\d+|end)$/.exec(line);
    if (m) {
      next = m[1] === "end" ? null : Number(m[1]);
    } else if (next !== null) {
      // Lines for development are left out of production bundles.
      while (next <= LINES.length && isDev(LINES[next - 1])) next++;
      if (LINES[next - 1] !== line) {
        throw new Error(`runtime.js line ${next} is not the script's: ${JSON.stringify(line.slice(0, 80))}`);
      }
      out.push([off + firstCol(line), next]);
      next++;
    }
    off += line.length + 1;
  }
  return out;
}

/// The lines of runtime.js that ran, from a script's source and the
/// functions V8's precise coverage gave for it, into `into` (a Set).
function hits(src, functions, map, into) {
  const ranges = [];
  for (const f of functions) for (const r of f.ranges) ranges.push(r);
  // Outermost first, so each range inside another paints over it.
  ranges.sort((a, b) => b.endOffset - b.startOffset - (a.endOffset - a.startOffset));
  const ran = new Uint8Array(src.length + 1);
  for (const r of ranges) ran.fill(r.count > 0 ? 1 : 0, r.startOffset, r.endOffset);
  for (const [off, line] of map) if (ran[off]) into.add(line);
  return into;
}

/// Writes the lines of runtime.js that ran (a Set) to `dir`, in a file of
/// its own; nothing if none did.
function record(dir, tag, lines) {
  if (lines.size === 0) return;
  fs.mkdirSync(dir, { recursive: true });
  const name = `${tag}-${process.pid}-${Date.now()}-${Math.random().toString(36).slice(2, 8)}.json`;
  fs.writeFileSync(path.join(dir, name), JSON.stringify([...lines].sort((a, b) => a - b)));
}

/// Lines added to runtime.js since `rev`, from `git diff -U0`.
function changedSince(rev) {
  const { execFileSync } = require("child_process");
  const repo = path.join(__dirname, "../..");
  const diff = execFileSync("git", ["-C", repo, "diff", "-U0", rev, "--", "src/client/runtime.js"], { encoding: "utf8" });
  const out = [];
  for (const m of diff.matchAll(/^@@ -\S+ \+(\d+)(?:,(\d+))? @@/gm)) {
    const [start, n] = [Number(m[1]), m[2] === undefined ? 1 : Number(m[2])];
    for (let i = 0; i < n; i++) out.push(start + i);
  }
  return out;
}

/// The top-level declaration a line is in, to say where a missed line is.
function where(line) {
  for (let i = line - 1; i >= 0; i--) {
    const m = /^(?:async function |function |class |const |let )([\w$]+)|^([BW]\.[\w$]+)/.exec(LINES[i]);
    if (m) return m[1] || m[2];
  }
  return "(top)";
}

/// Runs of consecutive line numbers, as "a-b" or "a".
function runs(lines) {
  const out = [];
  for (const l of lines) {
    const last = out[out.length - 1];
    if (last && l === last[1] + 1) last[1] = l;
    else out.push([l, l]);
  }
  return out.map(([a, b]) => (a === b ? `${a}` : `${a}-${b}`));
}

function report(argv) {
  // Several folders, joined by commas, are summed up together.
  const dirs = argv[0].split(",");
  const opt = (name) => {
    const i = argv.indexOf(name);
    return i > 0 ? argv[i + 1] : undefined;
  };
  const min = Number(opt("--min") || 0);
  const files = dirs.flatMap((d) => (fs.existsSync(d) ? fs.readdirSync(d).filter((f) => f.endsWith(".json")).map((f) => path.join(d, f)) : []));
  if (files.length === 0) {
    console.error(`no coverage records in ${dirs.join(", ")}: was QUILLO_RUNTIME_COVERAGE set for the tests?`);
    return 1;
  }
  const ran = new Set();
  for (const f of files) for (const l of JSON.parse(fs.readFileSync(f, "utf8"))) ran.add(l);
  const total = [];
  const missed = [];
  LINES.forEach((l, i) => {
    if (!counted(l)) return;
    total.push(i + 1);
    if (!ran.has(i + 1)) missed.push(i + 1);
  });
  const pct = (100 * (total.length - missed.length)) / total.length;
  const lcov = opt("--lcov");
  if (lcov) {
    const da = total.map((l) => `DA:${l},${ran.has(l) ? 1 : 0}`).join("\n");
    fs.writeFileSync(lcov, `TN:\nSF:${FILE}\n${da}\nLF:${total.length}\nLH:${total.length - missed.length}\nend_of_record\n`);
  }
  const json = opt("--json");
  if (json) {
    const out = { file: "src/client/runtime.js", lines: total.length, run: total.length - missed.length, percent: Number(pct.toFixed(2)), missed: runs(missed) };
    fs.writeFileSync(json, JSON.stringify(out, null, 1) + "\n");
  }
  const byPlace = new Map();
  for (const r of runs(missed)) {
    const w = where(Number(r.split("-")[0]));
    byPlace.set(w, [...(byPlace.get(w) || []), r]);
  }
  for (const [w, rs] of byPlace) console.log(`  not run: ${w}: ${rs.join(", ")}`);
  console.log(`runtime.js: ${total.length - missed.length} of ${total.length} lines run, ${pct.toFixed(2)}% (${files.length} records)`);
  let status = 0;
  if (pct < min) {
    console.log(`below the ${min.toFixed(2)}% runtime.js must keep`);
    status = 1;
  }
  const rev = opt("--diff");
  if (rev) {
    let since;
    try {
      since = changedSince(rev);
    } catch (e) {
      // No git here (a copy made by `git archive`), or no such revision.
      console.log(`cannot compare with ${rev}: ${String(e.message).split("\n")[0]}`);
      return 1;
    }
    const changed = since.filter((l) => counted(LINES[l - 1]) && !ran.has(l));
    if (changed.length) {
      console.log(`lines changed since ${rev} that no test runs: ${runs(changed).join(", ")}`);
      status = 1;
    } else {
      console.log(`every line changed since ${rev} runs`);
    }
  }
  return status;
}

module.exports = { mapScript, hits, record, counted, FILE };

if (require.main === module) {
  const [cmd, ...rest] = process.argv.slice(2);
  if (cmd === "report") process.exitCode = report(rest);
  else {
    console.error("usage: node rtcover.js report <folder>[,<folder>...] [--min 99.5] [--lcov file] [--json file] [--diff git-rev]");
    process.exitCode = 2;
  }
}
