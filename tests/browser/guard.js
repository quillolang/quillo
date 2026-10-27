(async () => {
  const out = {};
  const tick = (ms = 60) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  try {
    for (let i = 0; i < 200 && !$("#ok"); i++) await tick(30);
    await tick(300);
    window.__quilloErrors = window.__quilloErrors || [];
    // Each button's error, cut to what says why.
    for (const id of ["reflect", "parse", "adopt", "script", "svgscript", "setter", "wrapped"]) {
      const before = window.__quilloErrors.length;
      $("#" + id).click(); await tick(200);
      const e = window.__quilloErrors.slice(before);
      out[id] = e.length === 1 && /is not called from Quillo code|another document|script element|does not set onclick|javascript: URL/.test(e[0]) ? "refused" : e;
    }
    $("#ok").click(); await tick();
    document.body.click(); await tick();
    out.target = $("#target").innerHTML;
    out.pwned = window.pwned || null;
  } catch (e) { out.exception = String(e && e.stack || e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
