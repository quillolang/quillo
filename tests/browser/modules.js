(async () => {
  const out = {};
  const tick = (ms = 60) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  try {
    out.early = window.__quillo_booted === true;
    $("#clicks").click();
    for (let i = 0; i < 200 && $(".made").textContent === "waiting"; i++) await tick(30);
    await tick();
    out.made = $(".made").textContent;
    out.clicks = $("#clicks").textContent;
    // Nothing of the modules on the page, and the page's own `add` left alone.
    out.page = ["Thing", "version", "plus", "hello", "NS", "late", "default"].filter((n) => n in window);
    out.add = window.add();
    out.scripts = [...document.scripts].filter((s) => s.src).map((s) => new URL(s.src).pathname.split("/")[1]);
    out.links = [...document.querySelectorAll("link[rel=modulepreload]")].map((l) => new URL(l.href).pathname);
    $("#nothing").click(); await tick();
    out.nothing = $(".wrong").textContent;
    $("#gone").click(); await tick();
    out.gone = $(".wrong").textContent.replace(/did not load:.*/, "did not load");
    out.errors = (window.__quilloErrors || []).map((e) => String(e).slice(0, 120));
  } catch (e) { out.exception = String(e && e.stack || e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
