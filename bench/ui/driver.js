// Clicks the benchmark app's buttons and times each operation: from the
// click until its update is done (every framework here finishes a click's
// update in microtasks; waiting for the next task would let the browser
// render in between) and laid out. The median of several runs, after
// warming up; and the heap with the page ready and with 1,000 rows. Sets
// <body data-result>.
(async () => {
  const out = {};
  const settle = () => new Promise((r) => setTimeout(r, 0));
  const micro = async () => { for (let i = 0; i < 20; i++) await null; };
  const click = (sel) => document.querySelector(sel).click();
  const rows = () => document.querySelectorAll("tbody tr");
  // The time to laid out; `js[name]`, the time until the update is done.
  const js = {};
  async function time(setup, op, n, name) {
    const ts = [], ss = [];
    for (let k = 0; k < n; k++) {
      await setup();
      await settle();
      // What the setup changed is laid out before the clock starts.
      document.body.getBoundingClientRect();
      const t = performance.now();
      op();
      await micro();
      ss.push(performance.now() - t);
      document.body.getBoundingClientRect();
      ts.push(performance.now() - t);
    }
    ts.sort((a, b) => a - b);
    ss.sort((a, b) => a - b);
    js[name] = ss[n >> 1];
    return ts[n >> 1];
  }
  const heap = async () => { await settle(); if (window.gc) { gc(); gc(); } await settle(); return performance.memory ? performance.memory.usedJSHeapSize / 1048576 : null; };
  try {
    for (let i = 0; i < 500 && !document.querySelector("#run"); i++) await new Promise((r) => setTimeout(r, 10));
    const island = document.querySelector("quillo-island");
    for (let i = 0; island && i < 500 && !island.__q; i++) await new Promise((r) => setTimeout(r, 10));
    // The page's scripts as they came (compressed when they were).
    out.scriptKB = performance.getEntriesByType("resource").filter((e) => e.initiatorType === "script").reduce((s, e) => s + e.encodedBodySize, 0) / 1024;
    out.readyMB = await heap();
    for (let k = 0; k < 5; k++) { click("#run"); await settle(); click("#clear"); await settle(); }
    // As js-framework-benchmark measures it: after warming up, with 1,000 rows.
    click("#run"); await settle();
    out.rowsMB = await heap();
    const fresh = async () => { click("#clear"); await settle(); };
    const filled = async () => { click("#run"); await settle(); };
    const N = 15;
    out.create1k = await time(fresh, () => click("#run"), N, "create1k");
    out.replace1k = await time(filled, () => click("#run"), N, "replace1k");
    out.update10th = await time(filled, () => click("#update"), N, "update10th");
    out.select = await time(filled, () => rows()[5].querySelector("a.lbl").click(), N, "select");
    out.swap = await time(filled, () => click("#swaprows"), N, "swap");
    out.remove = await time(filled, () => rows()[10].querySelector("a.remove").click(), N, "remove");
    out.create10k = await time(fresh, () => click("#runlots"), 5, "create10k");
    out.append1k = await time(filled, () => click("#add"), N, "append1k");
    out.clear1k = await time(filled, () => click("#clear"), N, "clear1k");
    for (const k of Object.keys(js)) out[k + "Js"] = js[k];
    // Small operations, script only, timed 20 at a time (finer than the
    // clock's tenth of a millisecond).
    const batch = async (op, k = 20) => {
      const ts = [];
      for (let run = 0; run < 5; run++) {
        await filled();
        await settle();
        document.body.getBoundingClientRect();
        const t = performance.now();
        for (let i = 0; i < k; i++) { op(i); await micro(); }
        ts.push((performance.now() - t) / k);
      }
      ts.sort((a, b) => a - b);
      return ts[2];
    };
    out.update10thJs = await batch(() => click("#update"));
    out.selectJs = await batch((i) => rows()[i * 37].querySelector("a.lbl").click());
    out.swapJs = await batch(() => click("#swaprows"));
    out.removeJs = await batch((i) => rows()[i * 31].querySelector("a.remove").click());
    click("#run"); await settle();
    out.ok = rows().length === 1000 && rows()[0].children[1].textContent.length > 5;
  } catch (e) { out.exception = String(e.stack || e); }
  out.errors = window.__quilloErrors || [];
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
