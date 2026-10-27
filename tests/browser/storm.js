// tests/programs/storm.qwe: after storms that mount and unmount components (with
// timers, live data and url()), nothing removed is still held on to.
(async () => {
  const out = {};
  const tick = (ms = 30) => new Promise((r) => setTimeout(r, ms));
  const waitFor = async (f, n = 200) => { for (let i = 0; i < n; i++) { if (f()) return true; await tick(); } return false; };
  const $ = (s) => document.querySelector(s);
  const $$ = (s) => [...document.querySelectorAll(s)];
  const button = (t) => $$("button").find((b) => b.textContent === t);
  const text = (s) => $(s).textContent;
  // Timers running from now on.
  const active = new Set();
  const si = window.setInterval, ci = window.clearInterval;
  window.setInterval = (f, ms, ...a) => { const id = si(f, ms, ...a); active.add(id); return id; };
  window.clearInterval = (id) => { active.delete(id); return ci(id); };
  const nodes = () => document.getElementsByTagName("*").length;
  const collect = async () => { for (let i = 0; i < 3; i++) { window.gc(); await tick(50); } };
  const heap = () => performance.memory.usedJSHeapSize;
  // One storm through everything, then back to the start.
  const cycle = async (ms) => {
    button("grid").click();
    await waitFor(() => $$(".panel").length === 9 && $(".visits b") && text(".visits b") !== "…");
    button("Storm").click(); await tick(ms);
    button("Calm").click();
    button("list").click(); await tick(30);
    for (const b of $$(".item button.link").slice(0, 3)) b.click();
    await tick(120);
    button("form").click(); await tick(30);
    button("Reset").click();
    await waitFor(() => text(".count") === "12" && text(".opened") === "0" && $$(".item").length === 12);
    // Timers of removed components stop at their next tick.
    await tick(200);
  };
  try {
    await tick(300);
    const startNodes = nodes();
    out.start = [text(".count"), $$(".item").length];
    await cycle(1500);
    out.stormed = +text(".changes") > 30;
    // Every component element added from here on, held weakly.
    const seen = [];
    const mo = new MutationObserver((records) => {
      for (const r of records) for (const n of r.addedNodes) {
        if (n.nodeType !== 1) continue;
        if (n.tagName === "QUILLO-ISLAND") seen.push(new WeakRef(n));
        for (const x of n.querySelectorAll("quillo-island")) seen.push(new WeakRef(x));
      }
    });
    mo.observe(document.body, { childList: true, subtree: true });
    await collect();
    const before = heap();
    for (let i = 0; i < 4; i++) await cycle(800);
    mo.disconnect();
    await collect();
    const after = heap();
    out.componentsSeen = seen.length > 100;
    out.removedButAlive = seen.filter((r) => { const el = r.deref(); return el && !el.isConnected; }).length;
    out.timersRunning = active.size;
    out.sameNodes = nodes() === startNodes;
    out.heapGrowthUnder1MB = after - before < 1024 * 1024;
    out.heapGrowthKB = Math.round((after - before) / 1024);
  } catch (e) { out.exception = String(e); }
  out.errors = window.__quilloErrors || [];
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
