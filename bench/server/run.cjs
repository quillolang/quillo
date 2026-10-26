// Runs the server benchmark: Quillo, Node and Bun serving the same answers
// (quillo.qwe, node.js, bun.js), on one core and on several, each loaded by
// load.rs (keep-alive connections, one request in flight on each).
//
//   node bench/server/run.cjs [seconds=5] [names=quillo,node,bun] [cores=6]
//
// Needs rustc (load.rs is built on first use), Node, Bun (BUN, else bun on
// the PATH); QUILLO names the quillo binary (target/release/quillo).
const { spawn, execFileSync } = require("child_process");
const fs = require("fs");
const os = require("os");
const path = require("path");
const net = require("net");

const here = __dirname;
const root = path.join(here, "..", "..");
const secs = Number(process.argv[2] || 5);
const names = (process.argv[3] || "quillo,node,bun").split(",");
const cores = Number(process.argv[4] || 6);
const quillo = process.env.QUILLO || path.join(root, "target", "release", "quillo");
const bun = process.env.BUN || "bun";
const scenarios = ["text", "json", "fortunes", "compute"];
const conns = 64, threads = 4;

const load = path.join(os.tmpdir(), "quillo-bench-load");
if (!fs.existsSync(load) || fs.statSync(load).mtimeMs < fs.statSync(path.join(here, "load.rs")).mtimeMs) {
  execFileSync("rustc", ["-O", "-o", load, path.join(here, "load.rs")]);
}

const freePort = () => new Promise((ok) => { const s = net.createServer().listen(0, () => { const p = s.address().port; s.close(() => ok(p)); }); });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function start(name, port, procs) {
  const opts = { cwd: here, stdio: "ignore", detached: true };
  if (name === "quillo") return spawn(quillo, ["run", "quillo.qwe", "--port", String(port), "--log", "off", ...(procs > 1 ? ["--instances", String(procs)] : [])], opts);
  if (name === "node") return spawn("node", ["node.js", String(port), String(procs)], opts);
  if (name === "bun") return spawn(bun, ["bun.js", String(port), String(procs)], opts);
  throw new Error(`unknown runtime ${name}`);
}

async function main() {
  const rows = [];
  for (const procs of [1, cores]) {
    for (const name of names) {
      const port = await freePort();
      const server = start(name, port, procs);
      try {
        for (let i = 0; i < 200; i++) { try { await fetch(`http://127.0.0.1:${port}/text`); break; } catch (_) { await sleep(50); } }
        await sleep(300);
        for (const sc of scenarios) {
          const url = `http://127.0.0.1:${port}/${sc}`;
          execFileSync(load, [url, String(conns), String(threads), "1"]);
          const r = JSON.parse(execFileSync(load, [url, String(conns), String(threads), String(secs)]));
          rows.push({ procs, name, sc, ...r });
          process.stderr.write(`${procs} ${name} ${sc}: ${r.rps} req/s\n`);
        }
      } finally {
        try { process.kill(-server.pid, "SIGKILL"); } catch (_) {}
        await sleep(500);
      }
    }
  }
  const fmt = (n) => (n >= 1000 ? `${(n / 1000).toFixed(1)}k` : String(Math.round(n)));
  for (const procs of [1, cores]) {
    console.log(`\n${procs === 1 ? "one process" : `${procs} processes`}: requests a second (p99 latency, ms)`);
    console.log("".padEnd(10) + names.map((n) => n.padStart(18)).join(""));
    for (const sc of scenarios) {
      console.log(sc.padEnd(10) + names.map((n) => {
        const r = rows.find((x) => x.procs === procs && x.name === n && x.sc === sc);
        return `${fmt(r.rps)} (${r.p99.toFixed(1)})${r.errors ? "!" : ""}`.padStart(18);
      }).join(""));
    }
  }
  if (process.env.BENCH_JSON) fs.writeFileSync(process.env.BENCH_JSON, JSON.stringify(rows, null, 1));
}

main().catch((e) => { console.error(String(e.stack || e)); process.exit(1); });
