(async () => {
  const out = {};
  const tick = (ms = 60) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  const names = (el) => [...el.attributes].map((a) => a.name).sort();
  try {
    for (let i = 0; i < 200 && !$("#go"); i++) await tick(30);
    await tick(300);
    $("#go").click(); await tick();
    out.form = [$("#f").getAttribute("action"), $("#f").getAttribute("method")];
    out.formAttrs = names($("#f"));
    out.link = [$("#a").getAttribute("href"), $("#a").getAttribute("onclick"), $("#a").getAttribute("title")];
    out.linkAttrs = names($("#a"));
    out.frame = $("#frame").getAttribute("srcdoc");
    out.anim = [$("#set").getAttribute("to"), $("#anim").getAttribute("values")];
    $("#a").dispatchEvent(new MouseEvent("mouseover"));
    out.ran = window.ran || 0;
    out.errors = window.__quilloErrors || [];
  } catch (e) { out.exception = String(e && e.stack || e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
