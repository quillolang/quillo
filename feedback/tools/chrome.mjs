// Headless Chrome over the DevTools protocol, with nothing to install
// (Node 22+ has WebSocket). For driving Quillo pages the way a person does
// (real mouse, keys and drags), and for catching what fails silently:
// exceptions, console errors, and components the browser had to make again
// because the server's HTML did not match ("mismatches").
//
//   import { launch } from "./chrome.mjs"
//   const browser = await launch({ mismatches: true })
//   const page = await browser.page()
//   await page.go("http://localhost:8080/")
//   await page.ev(`return document.title`)
//   console.log(page.problems, page.mismatches)
//   await browser.close()

import { spawn } from "node:child_process";
import { mkdtempSync, writeFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const CANDIDATES = [
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  "/Applications/Chromium.app/Contents/MacOS/Chromium",
  "/usr/bin/google-chrome",
  "/usr/bin/google-chrome-stable",
  "/usr/bin/chromium",
  "/usr/bin/chromium-browser",
];

export function findChrome() {
  if (process.env.QUILLO_CHROME) return process.env.QUILLO_CHROME;
  return CANDIDATES.find((p) => existsSync(p)) ?? null;
}

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const KEY_CODES = { ArrowLeft: 37, ArrowUp: 38, ArrowRight: 39, ArrowDown: 40, Enter: 13, Escape: 27, Tab: 9, Backspace: 8 };

// Modifiers for key(): add them up.
export const ALT = 1, CTRL = 2, META = 4, SHIFT = 8;

export async function launch({ width = 1440, height = 1000, mismatches = false } = {}) {
  const chromePath = findChrome();
  if (!chromePath) throw new Error("no Chrome found: set QUILLO_CHROME");
  const chrome = spawn(chromePath, ["--headless=new", "--remote-debugging-port=0",
    `--user-data-dir=${mkdtempSync(join(tmpdir(), "quillo-cdp-"))}`, "--no-first-run",
    `--window-size=${width},${height}`, "about:blank"], { stdio: ["ignore", "ignore", "pipe"] });
  const wsUrl = await new Promise((res, rej) => {
    let buf = "";
    chrome.stderr.on("data", (d) => { buf += d; const m = buf.match(/ws:\/\/\S+/); if (m) res(m[0]); });
    setTimeout(() => rej(new Error("Chrome did not start")), 15000);
  });
  const ws = new WebSocket(wsUrl);
  await new Promise((r) => ws.addEventListener("open", r));
  let nextId = 0;
  const pending = new Map();
  const handlers = new Map(); // sessionId -> fn(event)
  ws.addEventListener("message", (m) => {
    const msg = JSON.parse(m.data);
    if (msg.id && pending.has(msg.id)) { pending.get(msg.id)(msg); pending.delete(msg.id); return; }
    handlers.get(msg.sessionId)?.(msg);
  });
  const send = (method, params = {}, sessionId) => new Promise((r) => {
    const id = ++nextId;
    pending.set(id, r);
    ws.send(JSON.stringify({ id, method, params, sessionId }));
  });

  async function page(url = "about:blank") {
    const { result: { targetId } } = await send("Target.createTarget", { url: "about:blank" });
    const { result: { sessionId } } = await send("Target.attachToTarget", { targetId, flatten: true });
    const s = (method, params) => send(method, params, sessionId);
    const problems = [];
    const found = [];
    const loadWaiters = [];
    handlers.set(sessionId, async (msg) => {
      const p = msg.params;
      if (msg.method === "Page.loadEventFired") loadWaiters.splice(0).forEach((w) => w());
      if (msg.method === "Runtime.exceptionThrown") problems.push("exception: " + (p.exceptionDetails.exception?.description || p.exceptionDetails.text));
      if (msg.method === "Runtime.consoleAPICalled" && (p.type === "error" || p.type === "warning")) problems.push(`console.${p.type}: ` + p.args.map((a) => a.value ?? a.description).join(" "));
      if (msg.method === "Log.entryAdded" && p.entry.level === "error" && !/favicon\.ico/.test(p.entry.url || "")) problems.push("log: " + p.entry.text + (p.entry.url ? " " + p.entry.url : ""));
      if (msg.method === "Debugger.paused") {
        // Quillo's runtime throws a Mismatch when the server's HTML is not
        // what the component makes, then makes the component again. Read
        // what it wanted and what it found, where it threw.
        const desc = p.data?.description || "";
        const frame = p.callFrames[0];
        if (desc.startsWith("Mismatch")) {
          const expr = frame.functionName === "claimText"
            ? "JSON.stringify({ kind: 'text', path: location.pathname, want: t, have: n ? n.data : null })"
            : frame.functionName === "claimEl"
              ? "JSON.stringify({ kind: 'element', path: location.pathname, want: String(tp && tp[0]), have: el ? (el.outerHTML || el.data || '').slice(0, 200) : null })"
              : `JSON.stringify({ kind: ${JSON.stringify(desc.split("\n")[0])}, path: location.pathname, at: ${JSON.stringify(frame.functionName)} })`;
          const r = await s("Debugger.evaluateOnCallFrame", { callFrameId: frame.callFrameId, expression: expr, returnByValue: true });
          try { found.push(JSON.parse(r.result.result.value)); } catch { found.push({ kind: desc.split("\n")[0] }); }
        }
        s("Debugger.resume");
      }
    });
    await s("Runtime.enable");
    await s("Page.enable");
    await s("Log.enable");
    if (mismatches) {
      await s("Debugger.enable");
      await s("Debugger.setPauseOnExceptions", { state: "all" });
    }
    const api = {
      problems,
      mismatches: found,
      raw: s,
      async go(to, { wait = 300 } = {}) {
        const loaded = new Promise((r) => loadWaiters.push(r));
        await s("Page.navigate", { url: to });
        await Promise.race([loaded, sleep(10000)]);
        await sleep(wait);
      },
      // Runs `code` as the body of an async function in the page; gives what it returns.
      async ev(code) {
        const out = await s("Runtime.evaluate", { expression: `(async () => { ${code} })()`, awaitPromise: true, returnByValue: true, timeout: 30000 });
        const ex = out.result?.exceptionDetails;
        if (ex) return { error: ex.exception?.description || ex.text };
        return out.result?.result?.value;
      },
      // Waits until Quillo's components are running (or `ms` pass).
      async booted(ms = 5000) {
        for (let t = 0; t < ms; t += 50) {
          if (await api.ev(`return !!window.__quillo_booted`)) return true;
          await sleep(50);
        }
        return false;
      },
      // What Quillo itself counted: components taken up as the server sent
      // them, made again, and errors it caught.
      async quillo() {
        return api.ev(`return { resumed: window.__quilloResumed || 0, remade: window.__quilloRemade || 0, errors: (window.__quilloErrors || []).map(String) }`);
      },
      async shot(path, { full = false, width: w, height: h, dark } = {}) {
        if (w) await s("Emulation.setDeviceMetricsOverride", { width: w, height: h || height, deviceScaleFactor: 1, mobile: w < 600 });
        if (dark != null) await s("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: dark ? "dark" : "light" }] });
        await sleep(200);
        const r = await s("Page.captureScreenshot", { format: "png", captureBeyondViewport: full });
        writeFileSync(path, Buffer.from(r.result.data, "base64"));
      },
      // Real mouse events (the page also gets pointer events from them).
      async mouse(type, x, y, extra = {}) {
        await s("Input.dispatchMouseEvent", { type, x, y, button: "left", buttons: type === "mouseReleased" ? 0 : 1, clickCount: 1, ...extra });
      },
      async click(x, y) {
        await api.mouse("mouseMoved", x, y, { buttons: 0 });
        await api.mouse("mousePressed", x, y);
        await api.mouse("mouseReleased", x, y);
      },
      // Drags with the mouse from (x, y) by (dx, dy), in `steps` moves.
      async drag(x, y, dx, dy, steps = 8) {
        await api.mouse("mouseMoved", x, y, { buttons: 0 });
        await api.mouse("mousePressed", x, y);
        for (let i = 1; i <= steps; i++) { await api.mouse("mouseMoved", x + dx * i / steps, y + dy * i / steps); await sleep(20); }
        await api.mouse("mouseReleased", x + dx, y + dy);
      },
      // The middle of the first element matching `selector`, in the window.
      async center(selector) {
        return api.ev(`const r = document.querySelector(${JSON.stringify(selector)})?.getBoundingClientRect(); return r ? { x: r.x + r.width / 2, y: r.y + r.height / 2 } : null`);
      },
      async key(name, modifiers = 0) {
        const code = KEY_CODES[name] ?? name.toUpperCase().charCodeAt(0);
        await s("Input.dispatchKeyEvent", { type: "keyDown", key: name, code: name, windowsVirtualKeyCode: code, modifiers });
        await s("Input.dispatchKeyEvent", { type: "keyUp", key: name, code: name, windowsVirtualKeyCode: code, modifiers });
      },
      async type(text) { await s("Input.insertText", { text }); },
      // HTML5 drag and drop from one element to another, with a DataTransfer
      // (headless Chrome does not start a native drag from mouse events).
      async dragDrop(fromSelector, toSelector) {
        return api.ev(`
          const dt = new DataTransfer();
          const src = document.querySelector(${JSON.stringify(fromSelector)});
          const dst = document.querySelector(${JSON.stringify(toSelector)});
          if (!src || !dst) return "missing " + (!src ? ${JSON.stringify(fromSelector)} : ${JSON.stringify(toSelector)});
          src.dispatchEvent(new DragEvent("dragstart", { bubbles: true, dataTransfer: dt }));
          await new Promise((r) => setTimeout(r, 50));
          dst.dispatchEvent(new DragEvent("dragover", { bubbles: true, cancelable: true, dataTransfer: dt }));
          await new Promise((r) => setTimeout(r, 50));
          dst.dispatchEvent(new DragEvent("drop", { bubbles: true, cancelable: true, dataTransfer: dt }));
          src.dispatchEvent(new DragEvent("dragend", { bubbles: true, dataTransfer: dt }));
          return "dropped";`);
      },
      // Fills a form's fields by name and submits it (a real submit, so the page navigates).
      async submitForm(selector, fields) {
        const loaded = new Promise((r) => loadWaiters.push(r));
        const r = await api.ev(`
          const f = document.querySelector(${JSON.stringify(selector)});
          if (!f) return "no form " + ${JSON.stringify(selector)};
          for (const [k, v] of Object.entries(${JSON.stringify(fields)})) f.elements[k].value = v;
          HTMLFormElement.prototype.submit.call(f);
          return "submitted";`);
        await Promise.race([loaded, sleep(5000)]);
        await sleep(300);
        return r;
      },
    };
    if (url !== "about:blank") await api.go(url);
    return api;
  }

  return {
    page,
    async close() { chrome.kill(); },
  };
}
