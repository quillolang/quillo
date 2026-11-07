// Clicks until the page answers, which says it has been taken up, then reads what was asked for.
(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const out = { errors: [] };
  const go = document.getElementById("go");
  for (let i = 0; i < 200 && go.textContent === "0"; i++) { if (i % 10 === 0) go.click(); await sleep(50); }
  await sleep(800);
  out.seen = await (await fetch("/counts")).json();
  out.tpl = document.getElementById("tpl").getAttribute("src");
  out.clicked = go.textContent;
  out.errors = window.__quilloErrors || [];
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
