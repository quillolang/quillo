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

const [chrome, url, profile, timeoutArg, scriptFile] = process.argv.slice(2);
const timeout = Number(timeoutArg || 20000);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function main() {
  fs.rmSync(profile, { recursive: true, force: true });
  const proc = spawn(chrome, [
    "--headless=new", "--disable-gpu", "--no-first-run", "--no-sandbox", "--disable-dev-shm-usage",
    "--remote-debugging-port=0", `--user-data-dir=${profile}`, "about:blank",
  ], { stdio: ["ignore", "ignore", "pipe"] });
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
    const target = await (await fetch(`http://127.0.0.1:${port}/json/new?${encodeURIComponent(url)}`, { method: "PUT" })).json();
    const ws = new WebSocket(target.webSocketDebuggerUrl);
    await new Promise((ok, bad) => { ws.onopen = ok; ws.onerror = bad; });
    let id = 0;
    const pending = new Map();
    ws.onmessage = (m) => {
      const msg = JSON.parse(m.data);
      if (msg.id && pending.has(msg.id)) { pending.get(msg.id)(msg); pending.delete(msg.id); }
    };
    const send = (method, params) => new Promise((ok) => { const n = ++id; pending.set(n, ok); ws.send(JSON.stringify({ id: n, method, params })); });
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
      if (v) { process.stdout.write(v); break; }
      await sleep(100);
    }
    ws.close();
  } finally {
    proc.kill();
  }
}

main().catch((e) => { console.error(String(e)); process.exit(1); });
