// Answers a call with what each case says, then lets the real ones through. One click a press,
// which the page keeps if its code has not come yet, and the next only when that one has answered.
(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const out = { errors: [], seen: {}, keys: [], marks: {} };
  const said = () => document.getElementById("said").textContent;
  const real = window.fetch;
  let answer = null;
  window.fetch = function (u, init) {
    if (String(u).includes("/rpc/pay")) {
      const h = (init && init.headers) || {};
      out.keys.push(h instanceof Headers ? h.get("idempotency-key") : h["idempotency-key"]);
    }
    if (answer && String(u).includes("/_quillo/rpc/")) return Promise.resolve(answer());
    return real.apply(this, arguments);
  };
  let last = "-";
  const press = async (id) => {
    const start = last;
    document.getElementById(id).click();
    for (let i = 0; i < 800 && said() === start; i++) await sleep(25);
    last = said();
    return last.replace(/^\d+ /, "");
  };
  const reply = (body, type, more) => () =>
    new Response(body === "" ? null : body, { status: 200, headers: { ...(type ? { "content-type": type } : {}), ...more } });
  const json = "application/json";
  const cut = () => new Response(new ReadableStream({
    start(c) { c.enqueue(new TextEncoder().encode('{"ok":')); c.error(new TypeError("cut short")); },
  }), { status: 200, headers: { "content-type": json, "x-quillo-answer": "1" } });
  out.first = await press("question");
  const cases = {
    html: reply("<html>sign in</html>", "text/html; charset=utf-8"),
    empty: reply("", null),
    json_null: reply("null", json),
    list: reply("[1]", json),
    number: reply("55", json),
    other: reply('{"a":1}', json),
    cut,
    ok_null: reply('{"ok":null}', json),
    ok_text: reply('{"ok":"x"}', json),
    marked: reply('{"a":1}', json, { "x-quillo-answer": "1" }),
  };
  for (const [name, make] of Object.entries(cases)) {
    answer = make;
    out.seen[name] = await press("question");
  }
  answer = null;
  out.real = await press("question");
  for (const id of ["nothing", "refused", "made", "early", "record", "blank", "moved"]) out[id] = await press(id);
  // Cut short, then the same key again; then, answered, a new one.
  answer = cut;
  await press("pay");
  answer = null;
  out.paid = await press("pay");
  await press("pay");
  // The mark, on every way out.
  const mark = async (name, headers) => (await real("/_quillo/rpc/" + name, {
    method: "POST", headers: { "content-type": json, ...headers }, body: name === "pay" ? '["me"]' : "[]",
  })).headers.get("x-quillo-answer");
  out.marks.ok = await mark("question");
  out.marks.err = await mark("refused");
  out.marks.respond = await mark("made");
  out.marks.reply = await mark("early");
  out.marks.empty = await mark("blank");
  out.marks.none = await mark("no_such_fn");
  out.marks.live = await mark("question", { "x-quillo-live": "1" });
  await mark("pay", { "idempotency-key": "same-key" });
  out.marks.replayed = await mark("pay", { "idempotency-key": "same-key" });
  out.marks.page = (await real("/")).headers.get("x-quillo-answer");
  out.errors = window.__quilloErrors || [];
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
