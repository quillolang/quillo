// Clicks through the tabs of examples/index.qwe.
(async () => {
  const out = {};
  const tick = (ms = 60) => new Promise((r) => setTimeout(r, ms));
  const waitFor = async (f, n = 120) => { for (let i = 0; i < n; i++) { if (f()) return true; await tick(30); } return false; };
  const $ = (s) => document.querySelector(s);
  const state = () => [location.pathname + location.search, $(".tabs a.active").textContent, $("iframe").getAttribute("src")];
  const click = (el) => el.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, button: 0 }));
  try {
    window.__same = 1;
    await waitFor(() => window.quillo); await tick(150);
    out.start = state();
    click($('.tabs a[href="/?ex=spa"]'));
    await waitFor(() => $("iframe").getAttribute("src") === "/spa");
    out.shop = state();
    click($('.tabs a[href="/?ex=layout"]'));
    await waitFor(() => $("iframe").getAttribute("src") === "/layout");
    out.layout = state();
    out.about = $(".about span").textContent;
    history.back();
    await waitFor(() => $("iframe").getAttribute("src") === "/spa");
    out.back = state();
    out.external = $(".about a").getAttribute("target");
    out.noReload = window.__same === 1;
    out.errors = window.__quilloErrors || [];
  } catch (e) { out.exception = String(e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
