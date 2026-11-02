(async () => {
  const t = (ms) => new Promise((r) => setTimeout(r, ms));
  const out = {};
  try {
    const first = document.querySelector(".tally");
    first.dispatchEvent(new PointerEvent("pointerover", { bubbles: true })); first.focus(); await t(800);
    out.report = typeof report === "function" ? report("x") : "no report";
    document.querySelector(".widget").click(); await t(500);
    const second = document.querySelector(".later .tally");
    second.dispatchEvent(new PointerEvent("pointerover", { bubbles: true })); await t(500);
    second.click(); await t(150); second.click(); await t(300);
    out.second = second.textContent;
    out.globals = Object.getOwnPropertyNames(window).filter((n) => n.startsWith("__quillo") || ["boot", "run", "resume", "toJSON", "W", "G", "B", "PLACES", "DEV"].includes(n));
    out.errors = window.__quilloErrors || [];
  } catch (e) { out.exception = String(e.stack || e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
