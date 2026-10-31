// Drives tests/browser/bits.qwe.
(async () => {
  const out = {};
  const tick = (ms = 60) => new Promise((r) => setTimeout(r, ms));
  const text = (s) => document.querySelector(s).textContent;
  const until = async (f) => { for (let i = 0; i < 100 && !f(); i++) await tick(30); };
  try {
    out.server = [text("#mask"), text("#top"), text("#big"), text("#name")];
    document.querySelector("#flip").click();
    await until(() => text("#mask") !== "48");
    out.flipped = text("#mask");
    out.top = text("#top");
    out.remade = window.__quilloRemade || 0;
    out.errors = window.__quilloErrors || [];
  } catch (e) { out.exception = String(e && e.stack || e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
