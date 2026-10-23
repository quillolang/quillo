(async () => {
  const out = {};
  const tick = (ms = 60) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  const $$ = (s) => [...document.querySelectorAll(s)];
  const labels = () => $$("li .label").map((e) => e.textContent).join(", ");
  // Rows made since the start (the others were kept, wherever they moved).
  const fresh = () => $$("li").filter((li) => li.__was === undefined).map((li) => li.querySelector(".label").textContent);
  const intact = () => $$("li").every((li) => li.__was === undefined || li.__was === li.querySelector(".label").textContent);
  try {
    for (let i = 0; i < 200 && !$("li"); i++) await tick(30);
    await tick(300);
    for (const li of $$("li")) li.__was = li.querySelector(".label").textContent;
    // Rows put into and taken out of the list: moving one is both.
    let moves = { added: 0, removed: 0 };
    new MutationObserver((rs) => { for (const r of rs) { moves.added += r.addedNodes.length; moves.removed += r.removedNodes.length; } })
      .observe($("ul"), { childList: true });
    const took = async () => { await tick(); const m = moves; moves = { added: 0, removed: 0 }; return [m.added, m.removed]; };
    $("#swap").click(); await tick();
    out.swapped = [labels(), fresh(), intact()];
    out.swapMoves = await took();
    $("#remove").click(); await tick();
    out.removed = [labels(), fresh(), intact()];
    out.removeMoves = await took();
    $("#insert").click(); await tick();
    out.inserted = [labels(), fresh(), intact()];
    $("#reverse").click(); await tick();
    out.reversed = [labels(), fresh(), intact()];
    $$("li")[1].querySelector(".pick").click(); await tick();
    out.picked = $$("li").map((li) => li.className);
    out.clicks = $(".clicks").textContent;
    $("#rich").click(); await tick();
    out.rich = $$("li .label").slice(0, 2).map((e) => e.innerHTML);
    $("#rich").click(); await tick();
    out.plain = $$("li .label").slice(0, 2).map((e) => e.innerHTML);
    $("#prefix").click(); await tick();
    $$("li")[0].click(); await tick();
    out.clicksAfter = $(".clicks").textContent;
    out.errors = window.__quilloErrors || [];
  } catch (e) { out.exception = String(e && e.stack || e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
