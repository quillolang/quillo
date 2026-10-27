// Types texts into the composer and compares its previews with what the
// server makes of each (GET /server).
(async () => {
  const out = { same: [], errors: [] };
  const tick = (ms = 50) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  // raw() HTML is in a <quillo-raw> of its own.
  const html = (s) => { const el = $(s + " quillo-raw") || $(s); return el.innerHTML; };
  // The server's HTML as the page's DOM writes it (<input/> is <input>).
  const dom = (h) => { const t = document.createElement("template"); t.innerHTML = h; return t.innerHTML; };
  try {
    for (let i = 0; i < 200 && !window.__quillo_booted; i++) await tick(30);
    out.first = html("#preview");
    const texts = [
      "# Title\n\nSome *em* and `code`, a [link](javascript:alert(1)).",
      "- [x] done\n- [ ] todo\n\n| a | b |\n|---|---|\n| 1 | 2 |",
      "```rust\nfn main() { let x = 1; }\n```",
      "<script>alert(1)</script> ~~gone~~",
    ];
    for (const t of texts) {
      const ta = $("#draft");
      ta.value = t;
      ta.dispatchEvent(new Event("input", { bubbles: true }));
      const want = await (await fetch("/server?t=" + encodeURIComponent(t))).json();
      let ok = false;
      for (let i = 0; i < 100 && !ok; i++) {
        await tick(30);
        ok = html("#preview") === dom(want.block) && html("#inline") === dom(want.inline);
      }
      out.same.push(ok || [html("#preview"), want.block]);
    }
    try { $("#bad").click(); } catch (e) {}
    await tick(200);
  } catch (e) {
    out.exception = String(e);
  }
  out.errors = (window.__quilloErrors || []).map(String);
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
