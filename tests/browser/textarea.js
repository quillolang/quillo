// Types into both boxes, then resets the one that follows state.
(async () => {
  const out = {};
  const tick = (ms = 50) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  const type = (el, text) => { el.value = text; el.dispatchEvent(new Event("input", { bubbles: true })); };
  try {
    for (let i = 0; i < 200 && !window.__quillo_booted; i++) await tick(30);
    out.first = [$("#follows").value, $("#fixed").value];
    type($("#follows"), "typed");
    type($("#fixed"), "typed too");
    await tick(100);
    out.typed = [$("#follows").value, $("#fixed").value];
    $("#reset").click();
    await tick(100);
    out.reset = [$("#follows").value, $("#fixed").value];
  } catch (e) {
    out.exception = String(e);
  }
  out.errors = (window.__quilloErrors || []).map(String);
  out.remade = window.__quilloRemade || 0;
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
