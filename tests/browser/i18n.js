(async () => {
  const out = {};
  const tick = (ms = 60) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  try {
    for (let i = 0; i < 200 && !$("#add"); i++) await tick(30);
    await tick(300);
    const seen = [$(".count").textContent];
    for (const _ of [2, 3, 4, 5]) { $("#add").click(); await tick(); seen.push($(".count").textContent); }
    out.seen = seen;
    out.button = $("#add").textContent;
    out.locale = $(".locale").textContent;
    out.errors = window.__quilloErrors || [];
  } catch (e) { out.exception = String(e && e.stack || e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
