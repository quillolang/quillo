// Drives tests/browser/decimals.qwe.
(async () => {
  const out = {};
  const tick = (ms = 60) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  const text = (s) => $(s).textContent;
  const until = async (f) => { for (let i = 0; i < 100 && !f(); i++) await tick(30); };
  try {
    out.server = [text("#total"), text("#big"), text("#kind")];
    $("#add").click();
    await until(() => text("#total") !== "25.00");
    out.added = text("#total");
    $("#tax").click();
    await until(() => text("#taxed") !== "0");
    out.taxed = text("#taxed");
    $("#echo").click();
    await until(() => text("#back") !== "");
    out.back = text("#back");
    out.big = text("#big");
    out.errors = window.__quilloErrors || [];
  } catch (e) { out.exception = String(e && e.stack || e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
