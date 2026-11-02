// Loads a page in headless Chrome, in real time, and prints what its test
// script stored in <body data-result="...">. Talks to Chrome over the
// DevTools protocol (Node's built-in WebSocket), so pages may keep requests
// open (live updates) without stalling the test.
//
//   node run.js <chrome> <url> <profile-dir> [timeout-ms] [script.js]
//
// With a script, it is run in the page once it has loaded; it should set
// data-result itself.

const { spawn } = require("child_process");
const fs = require("fs");
const path = require("path");
const rtcover = require("./rtcover");

const [chrome, url, profile, timeoutArg, scriptFile] = process.argv.slice(2);
const timeout = Number(timeoutArg || 20000);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function main() {
  fs.rmSync(profile, { recursive: true, force: true });
  const proc = spawn(chrome, [
    "--headless=new", "--disable-gpu", "--no-first-run", "--no-sandbox", "--disable-dev-shm-usage",
    "--remote-debugging-port=0", `--user-data-dir=${profile}`,
    // For leak tests: gc() and exact performance.memory figures.
    "--js-flags=--expose-gc", "--enable-precise-memory-info",
    // More for one test, as QUILLO_CHROME_FLAGS: a fake camera and microphone
    // for the media tests (tests/runtime_cover_media.rs), say.
    ...(process.env.QUILLO_CHROME_FLAGS || "").split(" ").filter(Boolean),
    "about:blank",
  ], { stdio: ["ignore", "ignore", "pipe"] });
  // Every wait below checks the deadline, but a reply Chrome never sends would
  // wait for good: end the run, or one lost reply holds up the whole suite.
  setTimeout(() => { console.error("Chrome stopped answering"); proc.kill(); process.exit(1); }, timeout + 10000).unref();
  // Chrome prints "DevTools listening on ws://127.0.0.1:PORT/..." when ready.
  let log = "";
  proc.stderr.on("data", (d) => { log += d; });
  const deadline = Date.now() + timeout;
  try {
    let port = null;
    while (!port) {
      if (Date.now() > deadline) throw new Error("Chrome did not start:\n" + log.slice(-2000));
      const m = log.match(/DevTools listening on ws:\/\/[^:]+:(\d+)\//);
      if (m) port = m[1];
      if (!port) {
        try { port = fs.readFileSync(path.join(profile, "DevToolsActivePort"), "utf8").split("\n")[0] || null; } catch (_) {}
      }
      await sleep(50);
    }
    // A blank tab first, so the page can be prepared before it loads.
    const target = await (await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: "PUT" })).json();
    const ws = new WebSocket(target.webSocketDebuggerUrl);
    await new Promise((ok, bad) => { ws.onopen = ok; ws.onerror = bad; });
    let id = 0;
    const pending = new Map();
    ws.onmessage = (m) => {
      const msg = JSON.parse(m.data);
      if (msg.id && pending.has(msg.id)) { pending.get(msg.id)(msg); pending.delete(msg.id); }
    };
    const send = (method, params) => new Promise((ok) => { const n = ++id; pending.set(n, ok); ws.send(JSON.stringify({ id: n, method, params })); });
    // Errors the runtime reports (quillo's `report`) are collected from the
    // first script on, for the tests' "errors" checks.
    await send("Page.enable");
    await send("Page.addScriptToEvaluateOnNewDocument", { source: "window.__quilloErrors = [];" });
    // Measuring runtime.js's coverage (rtcover.js): what ran is taken often,
    // since what a page ran before it navigates goes with it.
    const coverDir = process.env.QUILLO_RUNTIME_COVERAGE;
    const covered = new Set();
    const maps = new Map();
    const takeCoverage = async () => {
      if (!coverDir) return;
      const r = await send("Profiler.takePreciseCoverage");
      for (const s of (r.result && r.result.result) || []) {
        if (!maps.has(s.scriptId)) {
          const src = await send("Debugger.getScriptSource", { scriptId: s.scriptId });
          const source = src.result ? src.result.scriptSource : "";
          maps.set(s.scriptId, { source, map: rtcover.mapScript(source) });
        }
        const { source, map } = maps.get(s.scriptId);
        if (map) rtcover.hits(source, s.functions, map, covered);
      }
    };
    if (coverDir) {
      await send("Debugger.enable");
      await send("Debugger.setSkipAllPauses", { skip: true });
      await send("Profiler.enable");
      await send("Profiler.startPreciseCoverage", { callCount: true, detailed: true });
    }
    await send("Page.navigate", { url });
    if (scriptFile) {
      for (;;) {
        if (Date.now() > deadline) throw new Error("the page did not load");
        // A new tab starts as about:blank, which is "complete" too: wait for
        // our page, or the script would run in the blank one and be lost
        // when ours replaces it (which slow machines show).
        const origin = new URL(url).origin;
        const ready = `location.origin === ${JSON.stringify(origin)} && document.readyState === "complete"`;
        const r = await send("Runtime.evaluate", { expression: ready, returnByValue: true });
        if (r.result && r.result.result && r.result.result.value === true) break;
        await takeCoverage();
        await sleep(50);
      }
      await send("Runtime.evaluate", { expression: fs.readFileSync(scriptFile, "utf8") });
    }
    for (;;) {
      if (Date.now() > deadline) {
        // Say where the page was, to tell a slow machine from a stuck page.
        const r = await send("Runtime.evaluate", { expression: "JSON.stringify({ url: location.href, ready: document.readyState, quillo: !!window.quillo, errors: window.__quilloErrors || null, html: document.body ? document.body.innerHTML.slice(0, 400) : null })", returnByValue: true });
        const where = r.result && r.result.result && r.result.result.value;
        throw new Error("the page did not finish its test in time: " + where);
      }
      const r = await send("Runtime.evaluate", { expression: "document.body && document.body.getAttribute('data-result')", returnByValue: true });
      const v = r.result && r.result.result && r.result.result.value;
      await takeCoverage();
      if (v) { process.stdout.write(v); break; }
      await sleep(100);
    }
    if (coverDir) rtcover.record(coverDir, "chrome", covered);
    ws.close();
  } finally {
    proc.kill();
  }
}

main().catch((e) => { console.error(String(e)); process.exit(1); });
