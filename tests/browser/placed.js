// Drives tests/browser/placed.qwe: what the console is told of a failure.
(async () => {
  const out = {};
  const said = [];
  const error = console.error;
  console.error = (...a) => { said.push(a.map((x) => (x instanceof Error ? "<the error>" : String(x))).join(" ")); error(...a); };
  try {
    document.querySelector("#go").click();
    for (let i = 0; i < 100 && !said.length; i++) await new Promise((r) => setTimeout(r, 30));
    out.said = said;
  } catch (e) { out.exception = String(e && e.stack || e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
