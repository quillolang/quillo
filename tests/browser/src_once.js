// One click, which the page keeps if its code has not come yet, then what was asked for: read until
// the click's frame has been asked for, and once more after a while, for a late second request.
(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const out = { errors: [] };
  const go = document.getElementById("go");
  const seen = async () => (await fetch("/counts")).json();
  go.click();
  for (let i = 0; i < 400; i++) {
    out.seen = await seen();
    if (go.textContent !== "0" && out.seen.tpl1 >= 1 && out.seen.odd >= 2) break;
    await sleep(50);
  }
  await sleep(1000);
  out.seen = await seen();
  out.tpl = document.getElementById("tpl").getAttribute("src");
  out.clicked = go.textContent;
  out.errors = window.__quilloErrors || [];
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
