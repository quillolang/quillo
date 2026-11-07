// Reads what each frame's listener heard, once both have or a while has gone by.
(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const out = { errors: [] };
  const text = (id) => document.getElementById(id).textContent;
  for (let i = 0; i < 100 && !(text("after") === "1" && text("before") === "1"); i++) await sleep(50);
  out.after = text("after");
  out.before = text("before");
  out.errors = window.__quilloErrors || [];
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
