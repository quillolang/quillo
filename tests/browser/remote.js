(async () => {
  const out = {};
  const tick = (ms = 60) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  const until = async (f) => { for (let i = 0; i < 100 && !f(); i++) await tick(50); };
  try {
    for (let i = 0; i < 200 && !$("#get"); i++) await tick(30);
    await tick(300);
    const base = $("#base");
    base.value = "http://localhost:" + location.port;
    base.dispatchEvent(new Event("input", { bubbles: true }));
    await tick();
    $("#get").click();
    await until(() => $("#got").textContent !== "null");
    out.got = JSON.parse($("#got").textContent);
    $("#closed").click();
    await until(() => $("#refused").textContent !== "null");
    out.refused = JSON.parse($("#refused").textContent);
    const dt = new DataTransfer();
    dt.items.add(new File(["hello, file"], "note.txt", { type: "text/plain" }));
    $("#file").files = dt.files;
    $("#file").dispatchEvent(new Event("change", { bubbles: true }));
    await until(() => $("#sent").textContent !== "null");
    out.sent = JSON.parse($("#sent").textContent);
    out.token = $("#token").textContent;
    out.errors = window.__quilloErrors || [];
  } catch (e) { out.exception = String(e && e.stack || e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
