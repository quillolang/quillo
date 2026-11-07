// Answers one call at a time with what the case says, then lets the real ones through.
(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const out = { errors: [], seen: {} };
  const said = () => document.getElementById("said").textContent;
  const real = window.fetch;
  let answer = null;
  window.fetch = function (u) {
    if (answer && String(u).includes("/_quillo/rpc/")) {
      const a = answer;
      return Promise.resolve(new Response(a.body === "" ? null : a.body, { status: 200, headers: a.type ? { "content-type": a.type } : {} }));
    }
    return real.apply(this, arguments);
  };
  // Pressed until it answers, which says the page has been taken up.
  let last = "-";
  const press = async (id) => {
    const start = last;
    for (let i = 0; i < 200 && said() === start; i++) { if (i % 20 === 0) document.getElementById(id).click(); await sleep(25); }
    last = said();
    return last.replace(/^\d+ /, "");
  };
  out.first = await press("ask");
  const cases = {
    html: ["<html>sign in</html>", "text/html; charset=utf-8"],
    empty: ["", null],
    cut: ['{"ok":"rea', "application/json"],
    json_null: ["null", "application/json"],
    list: ["[1]", "application/json"],
    number: ["55", "application/json"],
    other: ['{"a":1}', "application/json"],
    ok_null: ['{"ok":null}', "application/json"],
    ok_text: ['{"ok":"x"}', "application/json"],
  };
  for (const [name, [body, type]] of Object.entries(cases)) {
    answer = { body, type };
    out.seen[name] = await press("ask");
  }
  answer = null;
  out.real = await press("ask");
  out.nothing = await press("nothing");
  out.refused = await press("refused");
  out.errors = window.__quilloErrors || [];
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
