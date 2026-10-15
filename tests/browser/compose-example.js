// Clicks through examples/compose.qwe.
(async () => {
  const out = {};
  const tick = (ms = 60) => new Promise((r) => setTimeout(r, ms));
  const $$ = (s) => [...document.querySelectorAll(s)];
  try {
    await tick(400);
    const plus = (i) => $$(".counter")[i].querySelectorAll("button")[1].click();
    const minus = (i) => $$(".counter")[i].querySelectorAll("button")[0].click();
    plus(0); await tick(); plus(0); await tick(); minus(1); await tick(); plus(3); await tick();
    out.counts = $$(".counter .n").map((e) => e.textContent);
    out.sum = document.querySelector(".sum b").textContent;
    out.log = $$(".panel li").map((e) => e.textContent);
    out.header = document.querySelector(".panel .header").textContent.trim();
    document.querySelector(".panel .header").click(); await tick();
    out.collapsed = !document.querySelector(".panel .body");
    out.errors = window.__quilloErrors || [];
  } catch (e) { out.exception = String(e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
