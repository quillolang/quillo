(async () => {
  const out = {};
  const tick = (ms = 60) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  const $$ = (s) => [...document.querySelectorAll(s)];
  try {
    for (let i = 0; i < 100 && !window.__quilloBooted; i++) {
      if (document.querySelector("quillo-island") && window.performance.getEntriesByName(location.origin + "/_quillo/quillo.js").length) break;
      await tick(30);
    }
    await tick(200);
    const [apples, pears] = $$(".counter");
    apples.querySelector("button").click(); await tick();
    apples.querySelector("button").click(); await tick();
    pears.querySelector("button").click(); await tick();
    out.counts = $$(".counter .n").map((e) => e.textContent);
    out.total = $(".total").textContent;
    out.log = $$(".panel li").map((e) => e.textContent);
    out.header = $(".panel .header").innerHTML;
    $(".panel .header").click(); await tick();
    out.closed = !$(".panel .body");
    $(".panel .header").click(); await tick();
    out.reopened = $(".total") ? $(".total").textContent : null;
    out.errors = window.__quilloErrors || [];
  } catch (e) { out.exception = String(e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
