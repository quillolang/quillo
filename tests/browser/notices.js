(async () => {
  const out = {};
  const tick = (ms = 60) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  try {
    for (let i = 0; i < 200 && !window.__quillo_booted; i++) await tick(30);
    $("#del").click();
    await tick(100);
    const d = $(".q-dialog");
    out.dialog = d && d.getAttribute("role");
    out.title = $(".q-dialog .q-title").textContent;
    out.focus = document.activeElement.className;
    out.waits = $("#n").textContent;
    $(".q-dialog .q-ok").click();
    await tick(250);
    out.after = $("#n").textContent;
    out.gone = !$(".q-dialog");
    out.toast = $(".q-toast .q-message") && $(".q-toast .q-message").textContent;
    await tick(700);
    out.toastGone = !$(".q-toast");
    // Escape: no, and focus goes back where it was.
    $("#del").focus();
    $("#del").click();
    await tick(100);
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await tick(250);
    out.escaped = $("#n").textContent;
    out.focusBack = document.activeElement.id;
    out.errors = window.__quilloErrors || [];
  } catch (e) { out.exception = String(e && e.stack || e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
