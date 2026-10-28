// The colors each title is shown in, before and after Inner is made in the browser.
(async () => {
  const out = {};
  const tick = (ms = 50) => new Promise((r) => setTimeout(r, ms));
  const color = (s) => getComputedStyle(document.querySelector(s)).color;
  try {
    for (let i = 0; i < 200 && !window.__quillo_booted; i++) await tick(30);
    out.server = ["#outer", "#panel", "#page", "#row1"].map(color);
    document.querySelector("#open").click();
    await tick(150);
    out.browser = [color("#inner"), getComputedStyle(document.querySelector("#shown")).fontWeight];
    out.sheets = [...document.querySelectorAll('link[rel="stylesheet"]')].map((l) => l.getAttribute("href").replace(/[0-9a-f]{16}/, "H"));
  } catch (e) {
    out.exception = String(e);
  }
  out.errors = (window.__quilloErrors || []).map(String);
  out.remade = window.__quilloRemade || 0;
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
