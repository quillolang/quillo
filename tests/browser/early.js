(async () => {
  const out = {};
  const tick = (ms = 60) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  const fire = (sel, type, init = {}) => {
    const C = type.startsWith("pointer") || ["click", "auxclick", "contextmenu"].includes(type) ? PointerEvent : MouseEvent;
    $(sel).dispatchEvent(new C(type, { bubbles: true, cancelable: true, composed: true, view: window, ...init }));
  };
  try {
    for (let i = 0; i < 200 && !$("#plain"); i++) await tick(30);
    await tick(300);
    out.before = !!window.__quillo_booted;
    // All in one go, before the bundle can come: a touch press on the grip
    // (its finger gone by then), a shift-click and a double click, a right
    // and a middle click, and a checkbox.
    fire("#grip", "pointerdown", { pointerType: "touch", pointerId: 9, isPrimary: true });
    fire("#grip", "mousedown");
    fire("#grip", "pointerup", { pointerType: "touch", pointerId: 9, isPrimary: true });
    fire("#plain", "click", { clientX: 31, shiftKey: true, detail: 1 });
    fire("#plain", "click", { clientX: 31, detail: 2 });
    fire("#plain", "dblclick", { clientX: 31, detail: 2 });
    fire("#menu", "contextmenu", { button: 2 });
    fire("#menu", "auxclick", { button: 1 });
    $("#box").click();
    out.kept = (window.__quillo_q || []).length;
    out.boxWaits = $("#box").checked;
    for (let i = 0; i < 200 && !window.__quillo_booted; i++) await tick(30);
    await tick(200);
    out.heard = $("#got").textContent;
    out.box = $("#box").checked;
    out.errors = window.__quilloErrors || [];
  } catch (e) { out.exception = String(e && e.stack || e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
