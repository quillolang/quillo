// Loads a page in headless Chrome, in real time, and prints what its test
// script stored in <body data-result="...">. Talks to Chrome over the
// DevTools protocol (Node's built-in WebSocket), so pages may keep requests
// open (live updates) without stalling the test.
//
//   node run.js <chrome> <url> <profile-dir> [timeout-ms]

const { spawn } = require("child_process");
const fs = require("fs");
const path = require("path");

const [chrome, url, profile, timeoutArg] = process.argv.slice(2);
const timeout = Number(timeoutArg || 20000);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function main() {
  fs.rmSync(profile, { recursive: true, force: true });
  const proc = spawn(chrome, [
    "--headless=new", "--disable-gpu", "--no-first-run", "--no-sandbox",
    "--remote-debugging-port=0", `--user-data-dir=${profile}`, "about:blank",
  ], { stdio: "ignore" });
  const deadline = Date.now() + timeout;
  try {
    // Chrome writes its debugging port here once it is ready.
    let port = null;
    while (!port) {
      if (Date.now() > deadline) throw new Error("Chrome did not start");
      try { port = fs.readFileSync(path.join(profile, "DevToolsActivePort"), "utf8").split("\n")[0]; } catch (_) {}
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
    for (;;) {
      if (Date.now() > deadline) throw new Error("the page did not finish its test in time");
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
