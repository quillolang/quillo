// Drives tests/browser/cover-channels.qwe while the test stops the server and
// starts it again: what is sent while it is down is kept, and sent once the
// page is let in by the new one; the page hears the new server.
(async () => {
  const out = {};
  const tick = (ms = 50) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  const until = async (f, ms = 15000) => {
    const end = Date.now() + ms;
    while (Date.now() < end) {
      const v = await f();
      if (v) return v;
      await tick(50);
    }
    return null;
  };
  const get = async (p) => { try { const r = await fetch(p); return r.ok ? await r.text() : null; } catch (e) { return null; } };
  const items = () => [...document.querySelectorAll(".listener.a li")].map((l) => l.textContent);
  try {
    out.via = await until(() => window.__quilloChannelVia);
    const gen = await until(() => get("/gen"));
    await until(async () => (await get("/peers")) === "1");
    // The test waits for this to reach the server, then stops it.
    $(".listener.a .send").click();
    out.heardBefore = !!(await until(() => items().includes("a says")));
    out.down = !!(await until(async () => (await get("/gen")) === null));
    await tick(300);
    // Sent while the server is down.
    $(".listener.a .send").click();
    out.back = !!(await until(async () => { const g = await get("/gen"); return g !== null && g !== gen; }, 20000));
    out.rejoined = !!(await until(async () => (await get("/peers")) === "1", 20000));
    out.keptArrived = await until(async () => { const l = await get("/log"); return l && JSON.parse(l).includes("a says") && JSON.parse(l); });
    out.heardTwice = !!(await until(() => items().filter((x) => x === "a says").length === 2));
    await get("/bcast/after");
    out.heardNew = !!(await until(() => items().includes("after")));
    out.items = items();
    out.errors = window.__quilloErrors || [];
  } catch (e) {
    out.exception = String((e && e.stack) || e);
  }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
