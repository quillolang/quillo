// examples/life.qwe: generations computed in the browser match the server's,
// and `every` runs the game only while it is playing.
(async () => {
  const out = {};
  const tick = (ms = 30) => new Promise((r) => setTimeout(r, ms));
  const waitFor = async (f, n = 200) => { for (let i = 0; i < n; i++) { if (f()) return true; await tick(); } return false; };
  const $ = (s) => document.querySelector(s);
  const $$ = (s) => [...document.querySelectorAll(s)];
  const button = (t) => $$("button").find((b) => b.textContent === t);
  const text = (s) => $(s).textContent;
  try {
    await waitFor(() => window.quillo); await tick(100);
    out.start = [text(".generation"), text(".population")];
    button("Step").click();
    await waitFor(() => text(".generation") === "1");
    out.one = text(".population");
    // State changes at once; the page re-renders once after all 29.
    for (let i = 1; i < 30; i++) button("Step").click();
    await waitFor(() => text(".generation") === "30");
    out.thirty = [text(".generation"), text(".population")];
    // Playing: the timer advances the game by itself.
    // The slider: 25 generations a second.
    const slider = $('input[type="range"]');
    slider.value = "25";
    slider.dispatchEvent(new Event("input", { bubbles: true }));
    await waitFor(() => text(".per-second") === "25/s");
    out.speed = text(".per-second");
    button("Play").click();
    out.played = await waitFor(() => +text(".generation") >= 34);
    button("Pause").click(); await tick(50);
    const paused = text(".generation");
    await tick(500);
    out.stopped = text(".generation") === paused;
    // A click flips a cell.
    const cells = $$(".board i");
    const i = cells.findIndex((c) => !c.classList.contains("on"));
    const before = +text(".population");
    cells[i].click();
    await waitFor(() => +text(".population") === before + 1);
    out.flipped = $$(".board i")[i].classList.contains("on");
    // Clear stops the game and empties the board.
    button("Play").click(); await tick(200);
    button("Clear").click(); await tick(100);
    const g = text(".generation");
    await tick(400);
    out.cleared = [text(".population"), text(".generation") === g, !!button("Play")];
    out.cells = cells.length;
  } catch (e) { out.exception = String(e); }
  out.errors = window.__quilloErrors || [];
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
