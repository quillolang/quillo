// Navigates around tests/programs/spa.qwe without reloading.
(async () => {
  const out = {};
  const tick = (ms = 60) => new Promise((r) => setTimeout(r, ms));
  const waitFor = async (f, n = 100) => { for (let i = 0; i < n; i++) { if (f()) return true; await tick(30); } return false; };
  const $ = (s) => document.querySelector(s);
  const h2 = () => ($("main h2") ? $("main h2").textContent : null);
  const click = (sel) => $(sel).dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, button: 0 }));
  try {
    window.__sameDocument = "yes";
    await waitFor(() => window.quillo); await tick(150);
    out.start = [location.pathname, h2()];
    click('nav a[href="/products"]');
    await waitFor(() => document.querySelectorAll(".products li").length === 4);
    out.products = [location.pathname, h2(), $("nav a.active").textContent];
    click('.products a[href="/products/2"]');
    await waitFor(() => h2() === "Notebook");
    out.product = [location.pathname, h2()];
    click("main button"); await tick();
    click("main button"); await tick();
    out.cart = $(".cart").textContent.replace(/\s+/g, " ").trim();
    history.back();
    await waitFor(() => location.pathname === "/products" && h2() === "Products");
    out.back = [location.pathname, h2()];
    history.forward();
    await waitFor(() => h2() === "Notebook");
    out.forward = [location.pathname, h2()];
    click('nav a[href="/about"]');
    await waitFor(() => h2() === "About");
    out.about = [location.pathname, $("nav a.active").textContent];
    out.cartKept = $(".cart b").textContent;
    // An address changed from outside (as Back does), to a missing product.
    history.pushState(null, "", "/products/99");
    window.dispatchEvent(new PopStateEvent("popstate"));
    await waitFor(() => h2() === "Not found");
    out.missing = [location.pathname, h2()];
    out.noReload = window.__sameDocument === "yes";
    out.errors = window.__quilloErrors || [];
  } catch (e) { out.exception = String(e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
