window.__quilloErrors = [];
const tick = () => new Promise((r) => setTimeout(r, 20));
async function waitFor(f) { for (let i = 0; i < 200; i++) { if (f()) return true; await tick(); } return false; }
async function run() {
  const out = {};
  try {
    await waitFor(() => window.quillo);
    await tick();
    const inc = document.querySelector("#counter .inc");
    inc.click(); inc.click();
    await tick();
    out.count = document.querySelector("#counter .value").textContent;
    const nested = document.querySelectorAll("#todos #counter .value");
    out.nestedStart = nested.length ? nested[0].textContent : null;
    const input = document.querySelector(".draft");
    input.value = "second";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await tick();
    out.draftShown = document.querySelector(".count").textContent;
    document.querySelector("#todos form").requestSubmit();
    out.saved = await waitFor(() => document.querySelector(".status").textContent === "saved");
    out.items = [...document.querySelectorAll("#todos li")].map((l) => l.textContent);
    out.inputCleared = document.querySelector(".draft").value;
    out.sameInput = document.querySelector(".draft") === input;
    out.nestedAfter = document.querySelector("#todos #counter .value").textContent;
  } catch (e) { out.exception = String(e); }
  out.errors = window.__quilloErrors;
  document.body.setAttribute("data-result", JSON.stringify(out));
}
window.addEventListener("load", run);
