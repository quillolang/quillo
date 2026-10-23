(async () => {
  const out = {};
  const tick = (ms = 60) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  try {
    for (let i = 0; i < 200 && !($(".gauge") && $(".gauge").textContent); i++) await tick(30);
    out.mounted = [$(".gauge").textContent, window.made];
    $("#inc").click(); await tick();
    out.called = [$(".gauge").textContent, $(".doubled").textContent];
    $("#inc").click(); await tick();
    // A render does not mount the element again.
    out.again = [$(".gauge").textContent, window.made];
    $("#toggle").click(); await tick();
    out.removed = [!!$(".gauge"), window.gone || null];
    $("#toggle").click(); await tick();
    out.back = [$(".gauge").textContent, window.made];
    out.errors = window.__quilloErrors || [];
  } catch (e) { out.exception = String(e && e.stack || e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
