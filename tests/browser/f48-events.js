// Types into a field without leaving it, double-clicks, moves focus into and out of a
// wrapper, and picks an option (FEEDBACK48.md, Q-560 and Q-561).
(async () => {
  const out = {};
  const tick = (ms = 50) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  try {
    for (let i = 0; i < 200 && !(window[Symbol.for("quillo")] || {}).booted; i++) await tick(30);
    const a = $("#a");
    a.focus();
    out.typed = [];
    for (const ch of "ann") {
      a.value += ch;
      a.dispatchEvent(new Event("input", { bubbles: true }));
      await tick(120);
      out.typed.push($("#pa").textContent);
    }
    out.focusedStill = document.activeElement === a;
    const t = $("#t");
    t.focus();
    t.value = "hi";
    t.dispatchEvent(new Event("input", { bubbles: true }));
    await tick(150);
    out.note = $("#pt").textContent;
    $("#me").dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    await tick(150);
    // As the browser sends them when a field in the wrapper gains and loses focus (a
    // headless window has none to give, so `focus()` alone sends no event).
    $("#i").dispatchEvent(new FocusEvent("focus"));
    $("#i").dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    await tick(150);
    $("#i").dispatchEvent(new FocusEvent("blur"));
    $("#i").dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
    await tick(150);
    const s = $("#s");
    s.value = "y";
    s.dispatchEvent(new Event("change", { bubbles: true }));
    await tick(150);
    out.log = $("#log").textContent;
  } catch (e) {
    out.exception = String(e);
  }
  out.errors = (window.__quilloErrors || []).map(String);
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
