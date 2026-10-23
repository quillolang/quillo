(async () => {
  const out = {};
  const tick = (ms = 60) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  const SVG = "http://www.w3.org/2000/svg";
  try {
    for (let i = 0; i < 200 && !$("#more"); i++) await tick(30);
    await tick(300);
    $("#more").click(); await tick();
    const els = [...$("svg.chart").querySelectorAll("*")];
    out.svg = $("svg.chart").namespaceURI === SVG;
    out.allSvg = els.filter((e) => e.closest("foreignObject") === null || e.tagName === "foreignObject").every((e) => e.namespaceURI === SVG);
    out.rects = $("svg.chart").querySelectorAll("rect").length;
    out.circle = !!$("svg.chart circle") && $("svg.chart circle").namespaceURI === SVG;
    out.html = $("foreignObject p").namespaceURI === "http://www.w3.org/1999/xhtml";
    out.cls = $("svg.chart g").getAttribute("class");
    out.drawn = $("svg.chart rect").getBBox().height > 0;
    out.errors = window.__quilloErrors || [];
  } catch (e) { out.exception = String(e && e.stack || e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
