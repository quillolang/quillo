// Drives tests/browser/rpc_failed.qwe: the value a failed server fn gives, and the console.
(async () => {
  const out = {};
  const said = [];
  const error = console.error;
  console.error = (...a) => { said.push(a.map((x) => (x instanceof Error ? "<the error>" : String(x))).join(" ")); error(...a); };
  try {
    const v = () => document.querySelector("#v").textContent;
    const before = v();
    document.querySelector("#down").click();
    for (let i = 0; i < 100 && v() === before; i++) await new Promise((r) => setTimeout(r, 30));
    await new Promise((r) => setTimeout(r, 100));
    out.value = v();
    out.said = said;
  } catch (e) { out.exception = String(e && e.stack || e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
