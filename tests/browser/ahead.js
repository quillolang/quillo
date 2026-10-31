// Points at links in a component and follows them, to see what state each page starts with.
(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const asked = () => performance.getEntriesByType("resource").filter((e) => /_quillo\/rpc\/load_/.test(e.name)).length;
  const point = (id) => document.getElementById(id).dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
  const text = (id) => document.getElementById(id)?.textContent ?? null;
  const out = { errors: [] };
  // Something must follow url() for the runtime to follow links: wait until it has.
  for (let i = 0; i < 100 && !window.__quillo_nav; i++) await sleep(50);
  point("go");
  await sleep(900);
  out.fetched = performance.getEntriesByType("resource").filter((e) => e.name.endsWith("/items")).length;
  const before = asked();
  document.getElementById("go").click();
  await new Promise((r) => requestAnimationFrame(r));
  out.at_once = text("items");
  await sleep(500);
  out.then = text("items");
  out.asked_for_items = asked() - before;
  // A component on this page and the next is left out, so each shows its own.
  point("both");
  await sleep(900);
  document.getElementById("both").click();
  await sleep(900);
  out.both = [...document.querySelectorAll(".named")].map((p) => p.textContent);
  // A call may change what was kept, so it is dropped.
  point("after");
  await sleep(900);
  document.getElementById("touch").click();
  await sleep(300);
  out.said = text("said");
  document.getElementById("after").click();
  await new Promise((r) => requestAnimationFrame(r));
  out.after_a_call = text("fresh");
  await sleep(500);
  out.after_a_call_then = text("fresh");
  // `data-no-prefetch`, as for any link.
  const seen = performance.getEntriesByType("resource").filter((e) => e.name.endsWith("/off")).length;
  point("off");
  await sleep(500);
  out.not_ahead = performance.getEntriesByType("resource").filter((e) => e.name.endsWith("/off")).length - seen;
  out.errors = window.__quilloErrors || [];
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
