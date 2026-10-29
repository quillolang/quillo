(async () => {
  const out = {};
  const tick = (ms = 60) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  try {
    for (let i = 0; i < 200 && !($(".gauge") && $(".gauge").textContent); i++) await tick(30);
    out.loaded = [...document.scripts].filter((s) => s.src).map((s) => new URL(s.src).pathname.split("/")[1]);
    out.mounted = [$(".gauge").textContent, window.made, $(".version").textContent];
    $("#inc").click(); await tick();
    out.called = [$(".gauge").textContent, $(".doubled").textContent];
    $("#near").click(); await tick();
    out.near = $(".wrong").textContent;
    $("#cls").click(); await tick();
    out.cls = $(".wrong").textContent;
    out.errors = (window.__quilloErrors || []).map((e) => String(e).slice(0, 120));
  } catch (e) { out.exception = String(e && e.stack || e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
