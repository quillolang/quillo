(async () => {
  const out = {};
  const tick = (ms = 60) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  const loaded = () => performance.getEntriesByType("resource").some((r) => r.name.includes("/_quillo/b/"));
  try {
    if (location.pathname === "/clock") {
      for (let i = 0; i < 100 && !(window.__quillo_booted && +$("#ticks").textContent > 2); i++) await tick(30);
      out.clockRuns = +$("#ticks").textContent > 2;
    } else {
      await tick(400);
      out.idleLoads = loaded();
      // Typed before the bundle came, then a click: both are kept.
      $("#name").value = "Ada";
      $("#name").dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
      $("#go").click();
      for (let i = 0; i < 100 && !window.__quillo_booted; i++) await tick(30);
      await tick(100);
      out.loadedOnFocus = loaded();
      out.echo = $("#echo").textContent;
      out.clicked = $("#go").textContent;
      out.value = $("#name").value;
    }
    out.errors = window.__quilloErrors || [];
  } catch (e) { out.exception = String(e && e.stack || e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
