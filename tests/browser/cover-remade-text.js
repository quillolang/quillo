// Drives tests/browser/cover-remade.qwe: a script changes the words of an
// inner component (the same element) before the page is reached. The guide:
// "An element whose tag or text differs (a script changed the page) is made
// again, for that component only."
(async () => {
  const out = {};
  const tick = (ms = 60) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  for (let i = 0; i < 200 && !window.__quillo_booted; i++) await tick(30);
  try {
    $(".said").textContent = "changed by a script";
    $("#wake").click();
    for (let i = 0; i < 100 && $("#wake").textContent !== "1"; i++) await tick(20);
    await tick(100);
    out.clicks = $("#wake").textContent;
    out.said = $(".said").textContent;
    out.remade = window.__quilloRemade || 0;
    out.errors = window.__quilloErrors || [];
  } catch (e) {
    out.exception = String((e && e.stack) || e);
  }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
