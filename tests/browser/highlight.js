(async () => {
  const out = {};
  const tick = (ms = 60) => new Promise((r) => setTimeout(r, ms));
  try {
    for (let i = 0; i < 200 && !document.querySelector("#client .sample"); i++) await tick(30);
    const html = (sel) => [...document.querySelectorAll(sel + " .sample")].map((p) => (p.querySelector("quillo-raw") || p).innerHTML);
    const server = html("#server"), client = html("#client");
    out.samples = server.length;
    out.same = server.length === client.length && server.every((s, i) => s === client[i]);
    out.differ = server.map((s, i) => (s === client[i] ? null : { server: s, client: client[i] })).filter(Boolean);
    out.coloured = server.filter((s) => s.includes("<span")).length;
    out.errors = window.__quilloErrors || [];
  } catch (e) { out.exception = String(e && e.stack || e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
