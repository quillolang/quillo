(async () => {
  const out = {};
  const tick = (ms = 60) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  const texts = (s) => [...document.querySelectorAll(s)].map((e) => e.textContent);
  try {
    for (let i = 0; i < 200 && !window.quillo; i++) await tick(30);
    await tick(300);
    out.server = { top: texts("#top tr"), sheet: texts("#sheet tr"), options: texts("#pick option"), score: $(".score").textContent };
    $("#top tr:nth-child(2) button").click(); await tick();
    $("#sheet tr:nth-child(1) button").click(); await tick();
    $("#add").click(); await tick();
    $("#sheet tr:nth-child(3) button").click(); await tick();
    $(".score button").click(); await tick();
    out.after = { top: texts("#top tr"), sheet: texts("#sheet tr"), score: $(".score").textContent };
    // No row or option in a wrapper: each is its table's or select's own.
    out.wrapped = document.querySelectorAll("quillo-island > tr, quillo-island > option").length;
    out.rows = $("#sheet tbody").rows.length;
    out.remade = window.__quilloRemade || 0;
    out.errors = window.__quilloErrors || [];
  } catch (e) { out.exception = String(e && e.stack || e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
