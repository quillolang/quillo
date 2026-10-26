// Runs the benchmark app in headless Chrome in Quillo, plain JavaScript and
// other frameworks in turn, and prints each operation's time (the median of
// the runs' medians), as js-framework-benchmark's keyed tests do.
//
//   node bench/ui/run.js [runs=3] [names=vanilla,quillo,solid,svelte,vue,preact,react]
//
// The other frameworks are built first: (cd bench/ui/frameworks && npm
// install --ignore-scripts && node build.mjs). Needs Chrome (CHROME, else
// the usual place) and Node 22 or later; QUILLO names the quillo binary.
const { spawn, execFileSync } = require("child_process");
const fs = require("fs");
const os = require("os");
const path = require("path");
const net = require("net");

const here = __dirname;
const root = path.join(here, "..", "..");
const runs = Number(process.argv[2] || 3);
const names = (process.argv[3] || "vanilla,quillo,solid,svelte,vue,preact,react").split(",");
const quillo = process.env.QUILLO || path.join(root, "target", "release", "quillo");
const chrome = process.env.CHROME || (process.platform === "darwin" ? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" : "google-chrome");
const pages = { vanilla: "/vanilla", quillo: "/" };
const pageOf = (n) => pages[n] || `/fw/${n}.html`;

const freePort = () => new Promise((ok) => { const s = net.createServer().listen(0, () => { const p = s.address().port; s.close(() => ok(p)); }); });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const ops = ["create1k", "replace1k", "update10th", "select", "swap", "remove", "create10k", "append1k", "clear1k"];
const rowsShown = [
  ...ops.map((o) => [o, "ms"]),
  ...ops.map((o) => [o + "Js", "ms"]),
  ["readyMB", "MB"], ["rowsMB", "MB"], ["scriptKB", "KB"],
];

async function main() {
  for (const n of names) if (!pages[n] && !fs.existsSync(path.join(here, "frameworks", "dist", `${n}.js`))) throw new Error(`${n} is not built: see frameworks/build.mjs`);
  const port = await freePort();
  const server = spawn(quillo, ["run", path.join(here, "quillo.qwe"), "--port", String(port)], { stdio: "ignore" });
  try {
    for (let i = 0; i < 100; i++) { try { await fetch(`http://127.0.0.1:${port}/`); break; } catch (_) { await sleep(50); } }
    const results = Object.fromEntries(names.map((n) => [n, []]));
    for (let r = 0; r < runs; r++) {
      for (const n of names) {
        const profile = fs.mkdtempSync(path.join(os.tmpdir(), "quillo-bench-"));
        const out = execFileSync("node", [path.join(root, "tests", "browser", "run.js"), chrome, `http://127.0.0.1:${port}${pageOf(n)}`, profile, "180000", path.join(here, "driver.js")]);
        fs.rmSync(profile, { recursive: true, force: true });
        const res = JSON.parse(out);
        if (res.exception || !res.ok || res.errors.length) throw new Error(`${n}: ${out}`);
        results[n].push(res);
      }
    }
    const median = (xs) => { const s = [...xs].sort((a, b) => a - b); return s[s.length >> 1]; };
    const table = {};
    for (const n of names) table[n] = Object.fromEntries(rowsShown.map(([k]) => [k, median(results[n].map((x) => x[k]))]));
    const fmt = (v) => (v >= 100 ? v.toFixed(0) : v >= 10 ? v.toFixed(1) : v >= 1 ? v.toFixed(2) : v.toFixed(3));
    const w = 10;
    console.log("".padEnd(14) + names.map((n) => n.padStart(w)).join(""));
    for (const [k, unit] of rowsShown) {
      if (k === "create1kJs") console.log("script only, before layout:");
      if (k === "readyMB") console.log("memory and size:");
      console.log(`${k} (${unit})`.padEnd(14) + names.map((n) => fmt(table[n][k]).padStart(w)).join(""));
    }
    // js-framework-benchmark's summary: the geometric mean of each time over the fastest's.
    // (Times are floored at the clock's resolution over a batch of 20.)
    const t = (n, k) => Math.max(table[n][k], 0.005);
    const geo = (keys) => Object.fromEntries(names.map((n) => [n, Math.exp(keys.reduce((s, k) => s + Math.log(t(n, k) / Math.min(...names.map((m) => t(m, k)))), 0) / keys.length)]));
    const g = geo(ops), gj = geo(ops.map((o) => o + "Js"));
    console.log("geometric mean of the times over the fastest:");
    console.log("total".padEnd(14) + names.map((n) => g[n].toFixed(2).padStart(w)).join(""));
    console.log("script only".padEnd(14) + names.map((n) => gj[n].toFixed(2).padStart(w)).join(""));
    if (process.env.BENCH_JSON) fs.writeFileSync(process.env.BENCH_JSON, JSON.stringify({ table, total: g, script: gj }, null, 1));
  } finally {
    server.kill();
  }
}

main().catch((e) => { console.error(String(e.stack || e)); process.exit(1); });
