// Drives tests/browser/cover-nav.qwe at /spa#spot: navigate() on a page that
// reads url(), to the spot it is already at, and to a query alone.
(async () => {
  const out = {};
  const tick = (ms = 60) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  const until = async (f, ms = 5000) => {
    const end = Date.now() + ms;
    while (Date.now() < end) {
      const v = f();
      if (v) return v;
      await tick(20);
    }
    return null;
  };
  const go = async (to) => {
    $("#to").value = to;
    $("#to").dispatchEvent(new Event("input", { bubbles: true }));
    await tick(30);
    $("#go").click();
  };
  try {
    await until(() => (window[Symbol.for("quillo")] || {}).booted);
    // The spot the address names, already gone to: scrolled away from, then
    // navigate("#spot") goes there again, with no new history entry.
    window.scrollTo(0, 0);
    await tick(50);
    const len = history.length;
    out.atTop = window.scrollY === 0;
    await go("#spot");
    out.backAtSpot = !!(await until(() => window.scrollY > 2000));
    out.target = document.querySelector(":target") && document.querySelector(":target").id;
    out.sameHistory = history.length === len;
    // A query alone, its key twice: the last one, without a reload.
    await go("?a=1&a=2");
    out.query = await until(() => $("#note").textContent.includes('"a"') && $("#note").textContent);
    out.errors = window.__quilloErrors || [];
    document.body.setAttribute("data-result", JSON.stringify(out));
  } catch (e) {
    out.exception = String((e && e.stack) || e);
    document.body.setAttribute("data-result", JSON.stringify(out));
  }
})();
