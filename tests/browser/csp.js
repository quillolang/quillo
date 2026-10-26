// Clicks the counter twice, points at a link (prefetching it), then reports
// what ran and what the policy blocked.
(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const b = document.getElementById("count");
  for (let i = 0; i < 50 && !window.quillo; i++) {
    b.dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
    await sleep(100);
  }
  b.click();
  await sleep(100);
  b.click();
  await sleep(200);
  document.getElementById("next").dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
  await sleep(800);
  document.body.setAttribute("data-result", JSON.stringify({
    count: b.textContent,
    good: window.good === 1,
    bad: window.bad === 1,
    blocked: window.__csp,
    errors: window.__quilloErrors || [],
  }));
})();
