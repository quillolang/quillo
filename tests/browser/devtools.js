// Drives devtools.qwe: listens as a tool would, then does what makes each message.
(async () => {
  const out = {};
  const t = (ms) => new Promise((r) => setTimeout(r, ms));
  const heard = (type) => window.__heard.filter((m) => m.type === type);
  const ask = async (q) => { document.dispatchEvent(new CustomEvent("quillo:devtools-ask", { detail: typeof q === "string" ? q : JSON.stringify(q) })); await t(150); };
  const $ = (s) => document.querySelector(s);
  try {
    const up = $("#up");
    up.dispatchEvent(new PointerEvent("pointerover", { bubbles: true })); up.focus();
    for (let i = 0; i < 100 && !window.__quillo_booted; i++) await t(30);
    await t(300);
    up.click(); await t(200);                       // a state set, a component made
    up.click(); await t(200);                       // and gone
    $("#add").click(); await t(600);               // a call, and a live change
    $("#refuse").click(); await t(400);            // a call that gives an error value
    $("#send").click(); await t(600);              // a channel's message out and in
    $("#boom").click(); await t(200);              // an error
    const panel = heard("component").find((m) => m.name === "Panel");
    await ask({ id: 1, op: "list" });
    await ask({ id: 2, op: "where", target: panel && panel.id });
    out.marked = !!document.querySelector('[data-quillo-devtools-id]');
    await ask({ id: 3, op: "where", target: 9999 });
    out.unmarked = !document.querySelector('[data-quillo-devtools-id]');
    await ask({ id: 4, op: "set", target: panel && panel.id, name: "count", value: 41 });
    await ask({ id: 5, op: "set", target: panel && panel.id, name: "nothing", value: 1 });
    await ask({ id: 6, op: "frob" });
    await ask("not json");
    await ask("null");
    await ask({ op: "list" });
    out.shown = $("#up").textContent;
    out.said = $(".said").textContent;
    out.heard = window.__heard.filter((m) => m.type !== "answer");
    out.answers = window.__heard.filter((m) => m.type === "answer");
    out.errors = window.__quilloErrors || [];
  } catch (e) { out.exception = String(e && e.stack || e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
