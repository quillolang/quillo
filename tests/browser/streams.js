window.__quilloErrors = [];
const tick = () => new Promise((r) => setTimeout(r, 30));
async function waitFor(f) { for (let i = 0; i < 200; i++) { if (f()) return true; await tick(); } return false; }
const text = (sel) => document.querySelector(sel).textContent;
window.addEventListener("load", async () => {
  const out = {};
  try {
    out.progressDone = await waitFor(() => text(".progress") === "3");
    out.names = document.querySelector(".progress").getAttribute("data-names");
    // The reply shows as it comes, a word at a time.
    document.querySelector(".ask").click();
    const seen = new Set();
    out.replyDone = await waitFor(() => { seen.add(text(".reply")); return text(".reply") === "echo hi back "; });
    out.replySteps = seen.size >= 3;
    // A render that stops asking stops the stream.
    out.ticking = await waitFor(() => Number(text(".tick")) >= 2);
    document.querySelector(".off").click();
    await new Promise((r) => setTimeout(r, 400));
    const a = Number(await (await fetch("/ticks-sent")).text());
    await new Promise((r) => setTimeout(r, 500));
    const b = Number(await (await fetch("/ticks-sent")).text());
    out.tickerStopped = a === b && a > 0;
  } catch (e) { out.exception = String(e); }
  out.errors = window.__quilloErrors;
  document.body.setAttribute("data-result", JSON.stringify(out));
});
