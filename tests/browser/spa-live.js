// examples/spa.qwe: live stock, the cart, and another tab buying.
(async () => {
  window.__quilloErrors = [];
  const out = {};
  const tick = (ms = 60) => new Promise((r) => setTimeout(r, ms));
  const waitFor = async (f, n = 150) => { for (let i = 0; i < n; i++) { if (f()) return true; await tick(30); } return false; };
  const $ = (s) => document.querySelector(s);
  const $$ = (s) => [...document.querySelectorAll(s)];
  const click = (el) => el.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, button: 0 }));
  const stockOf = (name) => { const li = $$(".products li").find((l) => l.textContent.includes(name)); return li ? li.querySelector(".stock").textContent : null; };
  const otherTab = (name, args) => fetch("/_quillo/rpc/" + name, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(args) });
  try {
    await waitFor(() => window.quillo); await tick(150);
    click($('nav a[href="/products"]'));
    await waitFor(() => stockOf("Notebook") === "10 in stock");
    out.before = stockOf("Notebook");
    // Another tab buys a notebook: this page updates by itself.
    await otherTab("buy", [2]);
    out.liveArrived = await waitFor(() => stockOf("Notebook") === "9 in stock");
    // The last fountain pen.
    click($('.products a[href="/products/3"]'));
    await waitFor(() => $("main h2") && $("main h2").textContent === "Fountain pen" && $("main .stock"));
    out.penBefore = $("main .stock").textContent;
    click($("main button")); 
    await waitFor(() => $("main .stock").textContent === "Sold out");
    out.penAfter = [$("main .stock").textContent, $("main button").disabled, !!$("main button.secondary")];
    // The cart: remove puts it back in stock, checkout empties the cart.
    out.cartLink = $(".cart").textContent.replace(/\s+/g, " ").trim();
    click($(".cart"));
    await waitFor(() => $$(".cart-items li").length === 1);
    click($(".cart-items button"));
    await waitFor(() => $$(".cart-items li").length === 0);
    const r = await (await otherTab("get_product", [3])).json();
    out.penReturned = r.ok.stock;
    history.back();
    await waitFor(() => $("main .stock") && $("main .stock").textContent === "Only 1 left");
    out.penPage = $("main .stock").textContent;
    click($("main button")); await tick(200);
    click($(".cart"));
    await waitFor(() => $$(".cart-items li").length === 1);
    click($$("main button").find((b) => b.textContent === "Check out"));
    await waitFor(() => $(".thanks"));
    out.checkout = [$(".thanks").textContent, $(".cart b").textContent];
    // Seven more open pages, each waiting for live changes: more than the six
    // HTTP connections a browser allows a host. Server calls still answer at
    // once, and every page hears about a change.
    const frames = [];
    for (let i = 0; i < 7; i++) {
      const f = document.createElement("iframe");
      f.src = "/products";
      document.body.append(f);
      frames.push(f);
    }
    const frameStock = (f) => { const li = f.contentDocument && [...f.contentDocument.querySelectorAll(".products li")].find((l) => l.textContent.includes("Desk mat")); return li ? li.querySelector(".stock").textContent : null; };
    await waitFor(() => frames.every((f) => frameStock(f) === "5 in stock" && f.contentWindow.quillo), 300);
    await tick(500);
    const t0 = performance.now();
    await otherTab("buy", [4]);
    out.callWhileManyWait = performance.now() - t0 < 2000;
    out.allFramesLive = await waitFor(() => frames.every((f) => frameStock(f) === "4 in stock"), 300);
    out.errors = window.__quilloErrors || [];
  } catch (e) { out.exception = String(e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
