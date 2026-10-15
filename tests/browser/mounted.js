// examples/index.qwe: the SPA under /spa and the notes app under /notes.
(async () => {
  const out = {};
  const tick = (ms = 60) => new Promise((r) => setTimeout(r, ms));
  const waitFor = async (f, n = 120) => { for (let i = 0; i < n; i++) { if (f()) return true; await tick(30); } return false; };
  const $ = (s) => document.querySelector(s);
  const h2 = () => ($("main h2") ? $("main h2").textContent : null);
  const click = (el) => el.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, button: 0 }));
  try {
    if (location.pathname.startsWith("/spa")) {
      window.__same = 1;
      await waitFor(() => window.quillo); await tick(150);
      click($('nav a[href="/spa/products"]'));
      await waitFor(() => document.querySelectorAll(".products li").length === 4);
      out.list = [location.pathname, h2()];
      click($('.products a[href="/spa/products/2"]'));
      await waitFor(() => h2() === "Notebook");
      out.product = [location.pathname, h2()];
      click($("main button")); await tick();
      out.cart = $(".cart b").textContent;
      history.back();
      await waitFor(() => h2() === "Products");
      out.back = location.pathname;
      out.noReload = window.__same === 1;
    } else {
      await waitFor(() => window.quillo); await tick(150);
      const input = $("form input");
      input.value = "mounted note"; input.dispatchEvent(new Event("input", { bubbles: true }));
      $("form").requestSubmit();
      await waitFor(() => [...document.querySelectorAll("li span")].some((s) => s.textContent === "mounted note"));
      // Another tab adds a note through the mounted server fn: live data.
      await fetch("/notes/_quillo/rpc/add_note", { method: "POST", headers: { "content-type": "application/json" }, body: '["from another tab"]' });
      out.liveArrived = await waitFor(() => [...document.querySelectorAll("li span")].some((s) => s.textContent === "from another tab"));
      out.notes = [...document.querySelectorAll("li span")].map((s) => s.textContent);
    }
    out.errors = window.__quilloErrors || [];
  } catch (e) { out.exception = String(e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
