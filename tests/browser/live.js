window.__quilloErrors = [];
const tick = () => new Promise((r) => setTimeout(r, 30));
async function waitFor(f) { for (let i = 0; i < 150; i++) { if (f()) return true; await tick(); } return false; }
const post = (url, body) => fetch(url, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body || {}) });
window.addEventListener("load", async () => {
  const out = {};
  try {
    await waitFor(() => window.quillo || document.querySelector("#board"));
    await tick(); await tick();
    out.before = [...document.querySelectorAll("#board li")].map((l) => l.textContent);
    await post("/api/notes", { text: "from elsewhere" });
    out.noteArrived = await waitFor(() => document.querySelectorAll("#board li").length === 2);
    out.after = [...document.querySelectorAll("#board li")].map((l) => l.textContent);
    await post("/api/hit"); await post("/api/hit");
    out.hitsArrived = await waitFor(() => document.querySelector(".hits").textContent === "2");
    out.hits = document.querySelector(".hits").textContent;
  } catch (e) { out.exception = String(e); }
  out.errors = window.__quilloErrors;
  document.body.setAttribute("data-result", JSON.stringify(out));
});
