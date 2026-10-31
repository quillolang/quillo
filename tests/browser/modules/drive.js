// Drives tests/browser/modules/main.qwe.
(async () => {
  const out = {};
  const tick = (ms = 60) => new Promise((r) => setTimeout(r, ms));
  const text = (s) => document.querySelector(s).textContent;
  const until = async (f) => { for (let i = 0; i < 100 && !f(); i++) await tick(30); };
  try {
    out.server = [text("#red"), text("#blue")];
    document.querySelector("#red").click();
    await until(() => text("#red") !== "red 1");
    document.querySelector("#blue").click();
    await until(() => text("#blue") !== "blue 10");
    out.after = [text("#red"), text("#blue")];
    out.remade = window.__quilloRemade || 0;
    out.errors = window.__quilloErrors || [];
  } catch (e) { out.exception = String(e && e.stack || e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
