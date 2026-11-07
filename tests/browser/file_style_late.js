// Opens what the server did not show, then reads the colours the sheets give it.
(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const out = { errors: [] };
  const go = document.getElementById("go");
  for (let i = 0; i < 200 && !document.getElementById("badge"); i++) { if (i % 10 === 0) go.click(); await sleep(50); }
  const colour = (id) => { const e = document.getElementById(id); return e ? getComputedStyle(e).color : null; };
  for (let i = 0; i < 60 && colour("badge") !== "rgb(1, 2, 3)"; i++) await sleep(50);
  out.badge = colour("badge");
  out.own = colour("own");
  out.mine = colour("mine");
  out.errors = window.__quilloErrors || [];
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
