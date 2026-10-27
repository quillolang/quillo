// Edits the JSON in tests/programs/layout.qwe and checks the preview.
(async () => {
  const out = {};
  const tick = (ms = 80) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  const $$ = (s) => [...document.querySelectorAll(s)];
  const text = (s) => ($(s) ? $(s).textContent.replace(/\s+/g, " ").trim() : null);
  const edit = async (v) => { const t = $("textarea"); t.value = v; t.dispatchEvent(new Event("input", { bubbles: true })); await tick(); };
  try {
    out.serverTextarea = $("textarea").value.length > 100;
    await tick(400);
    out.stats = $$(".preview .stat .value").map((e) => e.textContent);
    out.trendClass = $(".preview .stat").className;
    const a = $(".preview a");
    out.link = { href: a.getAttribute("href"), cls: a.className, title: a.title, text: a.textContent };
    out.unknown = text(".preview .unknown");
    $(".preview .counter button").click(); await tick();
    out.counter = text(".preview .counter b");
    const layout = JSON.parse($("textarea").value);
    layout.children[1].children.push({ type: "stat", props: { label: "Refunds", value: 7, trend: "down", ignored: true } });
    await edit(JSON.stringify(layout, null, 2));
    out.afterEdit = $$(".preview .stat .label").map((e) => e.textContent.trim());
    await edit("{ broken");
    out.error = !!$(".preview .error");
    await edit(JSON.stringify({ type: "card", props: { title: "Only" }, children: [{ type: "heading", props: { text: "Hi" } }] }));
    out.replaced = text(".preview");
    out.errors = window.__quilloErrors || [];
  } catch (e) { out.exception = String(e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
