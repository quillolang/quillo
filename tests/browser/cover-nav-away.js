// Drives tests/browser/cover-nav.qwe at /spa: navigate() to another site
// (localhost is another origin than 127.0.0.1) from a page that reads url().
// /landed reports where it was loaded; if the page stays, this says why.
(async () => {
  for (let i = 0; i < 200 && !(window[Symbol.for("quillo")] || {}).booted; i++) await new Promise((r) => setTimeout(r, 30));
  const to = document.getElementById("to");
  to.value = `http://localhost:${location.port}/landed/away?from=spa`;
  to.dispatchEvent(new Event("input", { bubbles: true }));
  await new Promise((r) => setTimeout(r, 50));
  document.getElementById("go").click();
  await new Promise((r) => setTimeout(r, 3000));
  document.body.setAttribute("data-result", JSON.stringify({ stayed: location.href, errors: window.__quilloErrors || [] }));
})();
