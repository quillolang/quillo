// Counts how the page asks once it is refused.
(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  await sleep(6500);
  const at = window.__asked;
  const gaps = at.slice(1).map((t, i) => t - at[i]);
  const out = {
    errors: (window.__quilloErrors || []).filter((e) => !e.includes("live updates")),
    told_once: (window.__quilloErrors || []).filter((e) => e.includes("live updates")).length,
    asked_at_most_five_times: at.length >= 2 && at.length <= 5,
    waits_as_told: gaps.every((g) => g >= 450),
    all_different: new Set(gaps).size === gaps.length,
  };
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
