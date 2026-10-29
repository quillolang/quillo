(async () => {
  const out = {};
  const tick = (ms = 80) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  const type = async (el, v) => { el.focus(); await tick(300); el.value = v; el.dispatchEvent(new Event("input", { bubbles: true })); await tick(); };
  try {
    for (let i = 0; i < 200 && !$("#amount"); i++) await tick(30);
    await type($("#amount"), "20000");
    await type($("#name"), "Ana");
    $("#ok").click(); await tick();
    out.text = $("#out").textContent;
    $("#go").click(); await tick();
    out.cleared = $("#amount").value;
    out.errors = window.__quilloErrors || [];
  } catch (e) { out.exception = String(e && e.stack || e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
