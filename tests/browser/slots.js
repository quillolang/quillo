(async () => {
  const out = {};
  const tick = (ms = 60) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  const $$ = (s) => [...document.querySelectorAll(s)];
  const cards = () => $$(".card").map((c) => ({
    title: c.querySelector("h3").textContent,
    footer: c.querySelector("footer").innerHTML,
    open: c.querySelector(".tallies").className,
    tallies: [...c.querySelectorAll(".tallies li")].map((li) => li.querySelector("b").textContent + "=" + li.querySelector("i").textContent + "/" + li.querySelector("s").textContent),
    badges: [...c.querySelectorAll(".badge")].map((b) => b.textContent + (b.classList.contains("hot") ? "!" : "")),
    radii: [...c.querySelectorAll("circle")].map((e) => e.getAttribute("r")),
  }));
  try {
    for (let i = 0; i < 200 && !$(".tally"); i++) await tick(30);
    await tick(300);
    out.layout = [$(".layout h1").textContent, $(".layout nav").innerHTML];
    out.start = cards();
    const summary = () => $$(".summary .tally").map((t) => t.querySelector("b").textContent + "=" + t.querySelector("i").textContent);
    $$(".summary .inc")[1].click(); await tick();
    // State inside rows: the second tally (pear) counts to 2.
    const pear = $$(".card .tally")[1];
    pear.querySelector(".inc").click(); await tick();
    pear.querySelector(".inc").click(); await tick();
    // The parent re-renders: rows are kept, and so is the tally's state.
    $("#toggle").click(); await tick();
    $("#rename").click(); await tick();
    $("#add").click(); await tick();
    $("#bump").click(); await tick();
    out.after = cards();
    out.summary = summary();
    out.samePear = $$(".card .tally")[1] === pear;
    $("#share").click(); await tick();
    out.shared = cards().map((c) => c.tallies);
    // Each card's rows stay its own: a click in one, then the parent again.
    $$(".card")[0].querySelectorAll(".tally .inc")[1].click(); await tick();
    $$(".card")[1].querySelectorAll(".tally .inc")[0].click(); await tick();
    $("#toggle").click(); await tick();
    $("#bump").click(); await tick();
    out.sharedAfter = cards().map((c) => [c.tallies, c.badges]);
    $("#unshare").click(); await tick();
    out.unshared = cards().map((c) => [c.tallies, c.badges]);
    const li = $(".badges li");
    out.spread = [li.getAttribute("title"), li.getAttribute("data-x")];
    out.svg = $("circle").namespaceURI;
    out.errors = window.__quilloErrors || [];
  } catch (e) { out.exception = String(e && e.stack || e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
