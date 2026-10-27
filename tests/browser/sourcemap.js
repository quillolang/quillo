// Debugs a page in headless Chrome as an editor's JavaScript debugger would,
// through its source maps: sets a breakpoint on a line of a .qwe file, clicks
// the page's button, and prints (as JSON) where Chrome stopped, mapped back
// to the .qwe file, with what the scripts and their maps were.
//
//   node sourcemap.js <chrome> <url> <profile-dir> <file.qwe> <line>

const { spawn } = require("child_process");
const fs = require("fs");
const path = require("path");

const [chrome, url, profile, qwe, lineArg] = process.argv.slice(2);
const want = Number(lineArg);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// Source map "mappings": for each generated line, its [column, source line] segments (0-based).
function decode(mappings) {
  const digits = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  const lines = [];
  let srcLine = 0;
  for (const line of mappings.split(";")) {
    const segs = [];
    let col = 0;
    for (const seg of line.split(",").filter(Boolean)) {
      const nums = [];
      let n = 0, shift = 0;
      for (const ch of seg) {
        const d = digits.indexOf(ch);
        n += (d & 31) << shift;
        if (d & 32) { shift += 5; continue; }
        nums.push(n & 1 ? -(n >> 1) : n >> 1);
        n = 0; shift = 0;
      }
      col += nums[0];
      srcLine += nums[2];
      segs.push([col, srcLine]);
    }
    lines.push(segs);
  }
  return lines;
}

async function main() {
  fs.rmSync(profile, { recursive: true, force: true });
  const proc = spawn(chrome, ["--headless=new", "--disable-gpu", "--no-first-run", "--no-sandbox", "--remote-debugging-port=0", `--user-data-dir=${profile}`, "about:blank"], { stdio: ["ignore", "ignore", "pipe"] });
  let log = "";
  proc.stderr.on("data", (d) => { log += d; });
  const deadline = Date.now() + 20000;
  try {
    let port = null;
    while (!port) {
      if (Date.now() > deadline) throw new Error("Chrome did not start");
      const m = log.match(/DevTools listening on ws:\/\/[^:]+:(\d+)\//);
      if (m) port = m[1];
      await sleep(50);
    }
    const target = await (await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: "PUT" })).json();
    const ws = new WebSocket(target.webSocketDebuggerUrl);
    await new Promise((ok, bad) => { ws.onopen = ok; ws.onerror = bad; });
    let id = 0;
    const pending = new Map();
    const scripts = [];
    let paused = null;
    ws.onmessage = (m) => {
      const msg = JSON.parse(m.data);
      if (msg.id && pending.has(msg.id)) { pending.get(msg.id)(msg.result || msg); pending.delete(msg.id); }
      if (msg.method === "Debugger.scriptParsed" && msg.params.url) scripts.push(msg.params);
      if (msg.method === "Debugger.paused") paused = msg.params;
    };
    const send = (method, params) => new Promise((ok) => { const n = ++id; pending.set(n, ok); ws.send(JSON.stringify({ id: n, method, params })); });
    const evaluate = async (expression) => (await send("Runtime.evaluate", { expression, returnByValue: true })).result.value;
    await send("Debugger.enable");
    await send("Page.enable");
    await send("Page.navigate", { url });
    // Wake the components (the bundle loads when one is reached for), and wait for them.
    for (;;) {
      if (Date.now() > deadline) throw new Error("the page did not boot");
      const ok = await evaluate(`(() => { const b = document.querySelector("button"); if (!b) return false; b.dispatchEvent(new PointerEvent("pointerover", { bubbles: true })); return !!window.__quillo_booted; })()`);
      if (ok) break;
      await sleep(50);
    }
    // The script whose map names the .qwe file, and where the line is in it.
    const out = { scripts: scripts.map((s) => new URL(s.url).pathname), stops: [] };
    const source = "file://" + qwe;
    // Every place the line is in, as an editor's debugger sets them.
    const places = [];
    for (const s of scripts) {
      if (!s.sourceMapURL || !s.sourceMapURL.startsWith("data:")) continue;
      const map = JSON.parse(Buffer.from(s.sourceMapURL.split(",")[1], "base64").toString());
      if (map.sources[0] !== source) continue;
      out.sourcesContent = typeof map.sourcesContent[0] === "string";
      decode(map.mappings).forEach((segs, line) => {
        for (const [col, src] of segs) if (src + 1 === want) places.push({ url: s.url, line, col, map });
      });
    }
    if (!places.length) throw new Error(`no script maps ${qwe}:${want}; scripts: ${JSON.stringify(out.scripts)}`);
    const place = places[0];
    out.breakpointIn = new URL(place.url).pathname;
    out.places = places.length;
    for (const p of places) await send("Debugger.setBreakpointByUrl", { url: p.url, lineNumber: p.line, columnNumber: p.col });
    await evaluate(`setTimeout(() => document.querySelector("button").click(), 50); 1`);
    for (let i = 0; i < 100 && !paused; i++) await sleep(50);
    if (!paused) throw new Error("did not stop at the breakpoint");
    const top = paused.callFrames[0];
    const segs = decode(place.map.mappings)[top.location.lineNumber] || [];
    const seg = segs.filter(([c]) => c <= (top.location.columnNumber || 0)).pop() || segs[0];
    out.stoppedAt = seg ? seg[1] + 1 : null;
    const local = top.scopeChain.find((s) => s.type === "local" || s.type === "closure");
    if (local) {
      const props = await send("Runtime.getProperties", { objectId: local.object.objectId, ownProperties: true });
      out.names = (props.result || []).map((p) => p.name);
    }
    // Each place on the line stops once; going on from the last finishes the click.
    out.stops = [out.stoppedAt];
    for (;;) {
      paused = null;
      await send("Debugger.resume");
      for (let i = 0; i < 10 && !paused; i++) await sleep(30);
      if (!paused || out.stops.length > 10) break;
      const l = paused.callFrames[0].location;
      const s = (decode(place.map.mappings)[l.lineNumber] || []).filter(([c]) => c <= (l.columnNumber || 0)).pop();
      out.stops.push(s ? s[1] + 1 : null);
    }
    out.text = await evaluate(`document.querySelector("button").textContent`);
    process.stdout.write(JSON.stringify(out));
    ws.close();
  } finally {
    proc.kill();
  }
}

main().catch((e) => { console.error(String(e)); process.exit(1); });
