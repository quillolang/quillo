// Types into the box while the page updates ten times a second, then
// closes it: the box is the same element all along, and holds the text.
(async () => {
  const out = {};
  const tick = (ms = 50) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  try {
    for (let i = 0; i < 200 && !window.__quillo_booted; i++) await tick(30);
    const box = $("#box");
    box.focus();
    for (const ch of "Grace") {
      box.value += ch;
      box.dispatchEvent(new Event("input", { bubbles: true }));
      await tick(120);
    }
    await tick(300);
    out.same = $("#box") === box;
    out.focused = document.activeElement === box;
    out.value = $("#box").value;
    out.typed = $("#typed").textContent;
    out.ticked = Number($("#ticks").textContent) > 5;
    $("#close").click();
    await tick(200);
    out.closed = $("#box") === null;
  } catch (e) {
    out.exception = String(e);
  }
  out.errors = (window.__quilloErrors || []).map(String);
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
