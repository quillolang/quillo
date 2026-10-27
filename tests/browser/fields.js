// Reads the fields fields.qwe sets, after the events that set them.
(async () => {
  const out = {};
  const tick = (ms = 50) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  try {
    for (let i = 0; i < 200 && !window.__quillo_booted; i++) await tick(30);
    out.mounted = $("#q").value;
    $("#clear").click();
    await tick();
    out.cleared = $("#q").value;
    $("#r").value = "typed";
    $("#r").dispatchEvent(new Event("input", { bubbles: true }));
    await tick();
    out.by_event = $("#r").value;
    $("#s").dispatchEvent(new Event("change", { bubbles: true }));
    await tick();
    out.added = $("#s").value;
    $("#bad").click();
    await tick();
    out.bad = $("#q").innerHTML;
  } catch (e) {
    out.exception = String(e);
  }
  out.errors = (window.__quilloErrors || []).map(String);
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
