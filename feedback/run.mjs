// Runs every known issue's repro against the current quillo and says, for
// each, whether it is still open, fixed, or changed (neither: look at it).
//
//   node feedback/run.mjs                  all of them
//   node feedback/run.mjs Q-012 Q-013      some
//   node feedback/run.mjs --update         and write the results into feedback/INDEX.md
//   node feedback/run.mjs --verbose        print each repro's output too
//
// A repro is feedback/repros/Q-NNN-name.qwe, or a folder of that name with a
// main.qwe (for issues that need several files). Its first comment lines say
// how to run it and how to read what comes out:
//
//   // Q-012 `let` with a type's name does not bind it
//   // mode: run                  check | run | serve | browser | cli | script
//   // args: run --watch          cli: quillo's own arguments, in the repro's folder
//   // get: /path [header=value]  serve: requests to make (several allowed)
//   // page: /path                browser: the page to open
//   // script: steps.sh           script: a shell script in the repro's folder, run with $QUILLO
//                                 (for what a request cannot show: signals, editing a file)
//   // probe: return {...}        browser: JavaScript run in the page once it is up
//   // flags: --no-prefetch        more options for `quillo run` (serve, browser)
//   // open-when: <builtin        still a bug if the output contains this (any of them)
//   // fixed-when: 6              fixed if the output contains this (all of them)
//
// A marker between slashes is a regular expression: /line \d+: error/. A repro
// with no open-when is open until its fixed-when markers all show.
// The output a repro is judged on: `check` and `run`, what quillo printed;
// `serve`, each response's status and body, then the server's log; `browser`,
// the probe's result and what the page reported (see tools/chrome.mjs).

import { spawn, execFileSync } from "node:child_process";
import { readdirSync, readFileSync, writeFileSync, statSync, existsSync, rmSync } from "node:fs";
import { join, dirname, basename } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = dirname(here);
const quillo = process.env.QUILLO || join(root, "target/release/quillo");
if (!existsSync(quillo)) {
  console.error(`no quillo at ${quillo}: run cargo build --release, or set QUILLO`);
  process.exit(2);
}
const version = execFileSync(quillo, ["--version"]).toString().trim();
const commit = version.match(/\(([0-9a-f]{7,})/)?.[1] ?? "?";

const args = process.argv.slice(2);
const update = args.includes("--update");
const verbose = args.includes("--verbose");
const only = args.filter((a) => /^Q-\d+$/.test(a));

const reprosDir = join(here, "repros");
const repros = readdirSync(reprosDir)
  .filter((n) => /^Q-\d+/.test(n) && (n.endsWith(".qwe") || statSync(join(reprosDir, n)).isDirectory()))
  .map((n) => {
    const path = join(reprosDir, n);
    const file = statSync(path).isDirectory() ? join(path, "main.qwe") : path;
    return { id: n.match(/^Q-\d+/)[0], name: n, file, dir: dirname(file), header: header(file) };
  })
  .filter((r) => only.length === 0 || only.includes(r.id))
  .sort((a, b) => a.id.localeCompare(b.id));

function header(file) {
  const h = { title: "", mode: "run", args: [], get: [], page: "/", probe: null, script: null, flags: [], open: [], fixed: [] };
  for (const line of readFileSync(file, "utf8").split("\n")) {
    if (!line.startsWith("//")) break;
    const text = line.slice(2).trim();
    const m = text.match(/^([a-z-]+):\s?(.*)$/);
    if (!m) { if (!h.title) h.title = text.replace(/^Q-\d+\s*/, ""); continue; }
    const [, key, value] = m;
    if (key === "mode") h.mode = value.trim();
    else if (key === "args") h.args = value.trim().split(/\s+/).filter(Boolean);
    else if (key === "get") h.get.push(value.trim());
    else if (key === "page") h.page = value.trim();
    else if (key === "probe") h.probe = value;
    else if (key === "script") h.script = value.trim();
    else if (key === "flags") h.flags = value.trim().split(/\s+/);
    else if (key === "open-when") h.open.push(value);
    else if (key === "fixed-when") h.fixed.push(value);
  }
  return h;
}

const matches = (out, marker) => {
  const re = marker.match(/^\/(.+)\/$/);
  return re ? new RegExp(re[1]).test(out) : out.includes(marker);
};

// Runs quillo with `args` in `cwd`; gives everything it printed.
function exec(args, cwd, ms = 20000) {
  return new Promise((res) => {
    const p = spawn(quillo, args, { cwd });
    let out = "";
    p.stdout.on("data", (d) => (out += d));
    p.stderr.on("data", (d) => (out += d));
    const t = setTimeout(() => { p.kill(); out += "\n[killed after " + ms + " ms]"; }, ms);
    p.on("close", () => { clearTimeout(t); res(out); });
  });
}

let nextPort = 18700 + Math.floor(Math.random() * 200);

// Serves the repro until `f` is done with it; gives f's output and the log.
async function serving(r, f) {
  const port = nextPort++;
  const p = spawn(quillo, ["run", basename(r.file), "--port", String(port), ...r.header.flags], { cwd: r.dir });
  let log = "";
  p.stdout.on("data", (d) => (log += d));
  p.stderr.on("data", (d) => (log += d));
  // On its own port: with --split, its services say they listen first.
  const up = new RegExp(`listening on \\S+:${port}\\b`);
  for (let i = 0; i < 100 && !up.test(log) && p.exitCode === null; i++) await new Promise((x) => setTimeout(x, 50));
  if (!up.test(log)) { p.kill(); return log; }
  try {
    const out = await f(`http://127.0.0.1:${port}`);
    await new Promise((x) => setTimeout(x, 150));
    return out + "\n--- server log ---\n" + log;
  } finally {
    p.kill();
  }
}

let browser = null;
async function getBrowser() {
  if (!browser) browser = await (await import("./tools/chrome.mjs")).launch({ mismatches: true });
  return browser;
}

async function output(r) {
  const h = r.header;
  if (h.mode === "check") return exec(["check", basename(r.file)], r.dir);
  if (h.mode === "run") return exec(["run", basename(r.file), ...h.flags], r.dir);
  // The command line itself: no program named, a folder, flags in its place.
  if (h.mode === "cli") return exec(h.args, r.dir, 5000);
  if (h.mode === "serve") {
    return serving(r, async (base) => {
      let out = "";
      for (const g of h.get.length ? h.get : ["/"]) {
        const [path, ...hs] = g.split(/\s+/);
        const headers = Object.fromEntries(hs.map((x) => [x.slice(0, x.indexOf("=")), x.slice(x.indexOf("=") + 1)]));
        const res = await fetch(base + path, { headers, redirect: "manual" });
        out += `GET ${path} -> ${res.status}\n${await res.text()}\n`;
      }
      return out;
    });
  }
  if (h.mode === "browser") {
    return serving(r, async (base) => {
      const page = await (await getBrowser()).page();
      await page.go(base + h.page);
      await page.booted();
      await new Promise((x) => setTimeout(x, 800));
      const probe = h.probe ? await page.ev(h.probe) : null;
      await new Promise((x) => setTimeout(x, 300));
      const q = await page.quillo();
      return JSON.stringify({ probe, ...q, mismatches: page.mismatches, problems: page.problems }, null, 1);
    });
  }
  // Steps a request cannot take (a signal, a file edited while it runs): a script.
  if (h.mode === "script") {
    return new Promise((res) => {
      const p = spawn("sh", [h.script], { cwd: r.dir, env: { ...process.env, QUILLO: quillo } });
      let out = "";
      p.stdout.on("data", (d) => (out += d));
      p.stderr.on("data", (d) => (out += d));
      const t = setTimeout(() => { p.kill(); out += "\n[killed after 60000 ms]"; }, 60000);
      p.on("close", () => { clearTimeout(t); res(out); });
    });
  }
  return `unknown mode ${h.mode}`;
}

console.log(`${version}\n`);
const results = [];
for (const r of repros) {
  const before = new Set(readdirSync(r.dir));
  const out = await output(r);
  // A database a repro made (db.open("x.db")) goes with it.
  for (const f of readdirSync(r.dir)) if (!before.has(f) && /\.db(-wal|-shm)?$/.test(f)) rmSync(join(r.dir, f));
  const fixedAll = r.header.fixed.length > 0 && r.header.fixed.every((m) => matches(out, m));
  const open = r.header.open.length > 0 ? r.header.open.some((m) => matches(out, m)) : !fixedAll;
  const fixed = !open && fixedAll;
  const status = open ? "open" : fixed ? "fixed" : "changed";
  results.push({ id: r.id, status });
  console.log(`${status.padEnd(8)} ${r.id}  ${r.header.title}`);
  if (verbose || status === "changed") console.log(out.trim().split("\n").map((l) => "         | " + l).join("\n"));
}
if (browser) await browser.close();

const count = (s) => results.filter((x) => x.status === s).length;
console.log(`\n${count("open")} open, ${count("fixed")} fixed, ${count("changed")} changed, on ${commit}`);

// The Status column of INDEX.md, for each issue run: "open (896e758)".
if (update) {
  const indexPath = join(here, "INDEX.md");
  const lines = readFileSync(indexPath, "utf8").split("\n");
  const head = lines.find((l) => l.startsWith("| ID"));
  const col = head.split("|").map((c) => c.trim()).indexOf("Status");
  const byId = new Map(results.map((x) => [x.id, x.status]));
  const updated = lines.map((l) => {
    const id = l.match(/^\| (Q-\d+) /)?.[1];
    if (!id || !byId.has(id)) return l;
    // A `\\|` in a cell's text is a pipe, not a cell's end.
    const cells = l.split(/(?<!\\)\|/);
    cells[col] = ` ${byId.get(id)} (${commit}) `;
    return cells.join("|");
  });
  writeFileSync(indexPath, updated.join("\n"));
  console.log(`updated ${indexPath}`);
}
