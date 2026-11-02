// Drives tests/browser/cover-remade.qwe: changes one inner component's
// element before the page is reached, then clicks, and reads back what was
// taken up as it was and what was made again.
(async () => {
  const out = {};
  const tick = (ms = 60) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  const $$ = (s) => [...document.querySelectorAll(s)];
  const until = async (f, ms = 8000) => {
    const end = Date.now() + ms;
    while (Date.now() < end) {
      const v = f();
      if (v) return v;
      await tick(20);
    }
    return null;
  };
  const warned = [];
  const warn = console.warn;
  console.warn = (...a) => { warned.push(a.join(" ")); return warn.apply(console, a); };
  try {
    await until(() => (window[Symbol.for("quillo")] || {}).booted);
    await tick(200);
    out.resumedEarly = (window[Symbol.for("quillo")] || {}).resumed || 0;
    // What should stay the page's own elements.
    const kept = $$(".inner")[0].querySelector("b"), cell = $(".cell"), bold = $("#raw b"), said = $(".said");
    // A script changes the second inner one's <b> to an <em>, same words.
    const b = $$(".inner")[1].querySelector("b");
    const em = document.createElement("em");
    em.textContent = b.textContent;
    b.replaceWith(em);
    $("#wake").click();
    out.clicks = await until(() => $("#wake").textContent === "1" && "1");
    const inners = $$(".inner");
    out.inners = inners.map((s) => s.firstElementChild.localName + ":" + s.firstElementChild.textContent);
    out.keptSame = inners[0].querySelector("b") === kept;
    // The one made again works: its own state, its handler.
    inners[1].querySelector(".bump").click();
    await until(() => $$(".inner")[1].querySelector(".bump").textContent === "2");
    out.bumped = $$(".inner").map((s) => s.querySelector(".bump").textContent);
    // The cell is its own island, in its row, taken up where it stands.
    out.cellSame = $(".cell") === cell;
    out.cellIsland = cell.hasAttribute("data-qi") && cell.dataset.c;
    out.cellParent = cell.parentElement.localName;
    cell.click();
    out.cell = await until(() => $(".cell").textContent === "x 1" && "x 1");
    out.rawSame = $("#raw b") === bold;
    out.rawWrapped = bold.parentElement.localName;
    out.saidSame = $(".said") === said;
    out.remade = (window[Symbol.for("quillo")] || {}).remade || 0;
    out.warned = warned.filter((w) => w.includes("made again"));
    out.errors = window.__quilloErrors || [];
  } catch (e) {
    out.exception = String((e && e.stack) || e);
  }
  console.warn = warn;
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
