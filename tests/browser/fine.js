// Drives tests/browser/fine.qwe: what each change does to the page.
(async () => {
  const out = {};
  const tick = (ms = 60) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  // The page's changes while `f` runs, by kind.
  const changes = async (root, f) => {
    const seen = [];
    const mo = new MutationObserver((rs) => { for (const r of rs) seen.push(r.type); });
    mo.observe(root, { subtree: true, childList: true, characterData: true, attributes: true });
    await f();
    await tick();
    seen.push(...mo.takeRecords().map((r) => r.type));
    mo.disconnect();
    return seen;
  };
  try {
    for (let i = 0; i < 100 && !$("quillo-island").__q; i++) await tick(20);
    await tick();
    out.start = { ...window.tally.runs };
    const lis = [...document.querySelectorAll("li")];
    out.edit = await changes($("ul"), () => $("#edit").click());
    out.rows = [...document.querySelectorAll("li")].map((li) => li.textContent);
    out.sameRows = [...document.querySelectorAll("li")].every((li, i) => li === lis[i]);
    const child = $(".child");
    out.childOnOther = await changes(child, () => $("#other").click());
    out.other = $(".other").textContent;
    out.same = await changes($("quillo-island"), () => $("#same").click());
    out.label = await changes(child, () => $("#label").click());
    out.child = $(".child").textContent;
    out.runs = { ...window.tally.runs };
    out.remade = window.__quilloRemade || 0;
    out.errors = window.__quilloErrors || [];
  } catch (e) { out.exception = String(e.stack || e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
