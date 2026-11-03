// Reads f48-render.qwe's page once taken up, then after a state change, clicks, a double
// click, typing and focus (FEEDBACK48.md, Q-564 to Q-567, Q-571 and Q-572).
(async () => {
  const out = {};
  const tick = (ms = 80) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  const cls = (s) => [...$(s).classList].join(" ");
  const attrs = (s, names) => Object.fromEntries(names.map((n) => [n, $(s).getAttribute(n)]));
  const WORDS = ["aria-hidden", "aria-expanded", "data-on", "draggable", "spellcheck", "contenteditable", "translate", "hidden", "aria-label"];
  const read = () => ({
    m: cls("#m"),
    l: cls("#l"),
    e: $("#e").getAttribute("class"),
    z: $("#z").getAttribute("class"),
    mixed: cls("#mixed"),
    style: $("#s").getAttribute("style"),
    fontSize: getComputedStyle($("#s")).fontSize,
    a: attrs("#a", WORDS),
    props: { draggable: $("#a").draggable, spellcheck: $("#a").spellcheck, translate: $("#a").translate, on: $("#a").dataset.on },
    af: attrs("#af", ["aria-hidden", "aria-expanded", "data-x", "disabled"]),
    b: attrs("#b", ["class", "classname", "title", "for", "htmlfor", "aria-pressed", "style", "gone", "data-n", "onclick"]),
    n: $("#b").textContent,
    k: $("#k").textContent,
    kk: $("#kk").textContent,
    h: $("#h").textContent,
    pre: $("#pre").textContent,
    pre2: $("#pre2").textContent,
    ta: $("#ta").value,
    sc: cls("#sc"),
    rows: $("#rows").textContent,
    late: $("#late") && { ...attrs("#late", ["aria-hidden", "aria-expanded", "data-x", "disabled", "style"]), cls: cls("#late") },
    latepre: $("#latepre") && $("#latepre").textContent,
  });
  try {
    for (let i = 0; i < 200 && !(window[Symbol.for("quillo")] || {}).booted; i++) await tick(30);
    await tick(200);
    out.before = read();
    $("#go").click();
    await tick();
    out.after = read();
    $("#b").click();
    await tick();
    out.clicked = $("#b").textContent;
    // A handler the spread no longer has is gone.
    $("#hp").click();
    await tick();
    out.hpGone = $("#b").textContent;
    $("#go").click();
    await tick();
    $("#hp").click();
    await tick();
    out.hpBack = $("#b").textContent;
    // The written-out handler wins over the spread's.
    $("#w2").click();
    await tick();
    out.written = $("#b").textContent;
    $("#c").dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    await tick();
    const i = $("#i");
    i.focus();
    i.value = "hey";
    i.dispatchEvent(new Event("input", { bubbles: true }));
    await tick();
    out.typed = $("#typed").textContent;
    // Headless, the page may not have the focus, so the events are sent as focusing sends them.
    $("#wi").dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    await tick();
    $("#wi").dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
    await tick();
    out.log = $("#log").textContent;
    out.back = read();
    $("#cap").click();
    await tick();
    out.cap = !!$("#cb");
  } catch (e) {
    out.exception = String((e && e.stack) || e);
  }
  out.errors = (window.__quilloErrors || []).map(String);
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
