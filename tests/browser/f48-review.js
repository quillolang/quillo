// Types into a bound field with a spread's handler, ticks a bound box with one, and reads what a
// function given to an attribute and a spread's `CLASS` became (FEEDBACK48.md; review).
(async () => {
  const out = {};
  const tick = (ms = 100) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  try {
    for (let i = 0; i < 200 && !(window[Symbol.for("quillo")] || {}).booted; i++) await tick(30);
    await tick(200);
    out.remade = window[Symbol.for("quillo")].remade || 0;
    out.n = $("#n").textContent;
    out.t = $("#t").outerHTML;
    out.up = $("#up").getAttribute("class");
    const i = $("#i"); i.value = "hey"; i.dispatchEvent(new Event("input", { bubbles: true })); await tick();
    $("#cb").click(); await tick();
    out.after = $("#out").textContent;
  } catch (e) { out.exception = String((e && e.stack) || e); }
  out.errors = (window.__quilloErrors || []).map(String);
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
