// Drives tests/browser/cover-channels.qwe at /plain, standing in for what is
// between the page and the server: first a proxy that refuses the channel
// ticket (502, as one does while the program restarts), then UDP blocked
// (the ticket's WebTransport port answers nothing).
(async () => {
  const out = {};
  const tick = (ms = 50) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  const until = async (f, ms = 8000) => {
    const end = Date.now() + ms;
    while (Date.now() < end) {
      const v = await f();
      if (v) return v;
      await tick(30);
    }
    return null;
  };
  const get = async (p) => (await fetch(p)).text();
  const items = () => [...document.querySelectorAll(".listener.a li")].map((l) => l.textContent);
  const asked = [];
  let mode = "refuse";
  const fetched = window.fetch;
  window.fetch = function (u, init) {
    if (String(u).endsWith("/_quillo/wt-ticket")) {
      asked.push(Date.now());
      if (mode === "refuse") return Promise.resolve(new Response("bad gateway", { status: 502 }));
      if (mode === "blocked") {
        return fetched.apply(this, arguments).then(async (r) => {
          const info = await r.json();
          // A UDP port no one listens on: WebTransport never gets ready.
          if (info.port) info.port = 9;
          return new Response(JSON.stringify(info), { status: 200, headers: { "content-type": "application/json" } });
        });
      }
    }
    return fetched.apply(this, arguments);
  };
  try {
    await until(() => (window[Symbol.for("quillo")] || {}).booted);
    // Refused: asked again after a wait.
    $("#toggle-a").click();
    await until(() => asked.length >= 2, 5000);
    out.askedAgain = asked.length >= 2;
    out.waited = asked.length >= 2 && asked[1] - asked[0] >= 400;
    // Nothing on the page listens any more: no more asking.
    $("#toggle-a").click();
    await until(() => !$(".listener.a"));
    const n = asked.length;
    await tick(2500);
    out.stoppedAsking = asked.length - n <= 1;
    out.noVia = (window[Symbol.for("quillo")] || {}).via === undefined;
    // A listener again, and UDP blocked: a WebSocket instead.
    mode = "blocked";
    $("#toggle-a").click();
    out.via = await until(() => (window[Symbol.for("quillo")] || {}).via, 10000);
    out.joined = !!(await until(async () => (await get("/peers")) === "1"));
    await get("/bcast/hello");
    out.heard = !!(await until(() => items().includes("hello")));
    window.fetch = fetched;
    out.errors = window.__quilloErrors || [];
  } catch (e) {
    out.exception = String((e && e.stack) || e);
  }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
