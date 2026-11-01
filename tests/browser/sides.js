// Runs the results in the browser, compares them with the server's, and does what stopped pages.
(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const $ = (id) => document.getElementById(id);
  const out = { errors: [] };
  $("run").click();
  await sleep(200);
  const server = JSON.parse($("server").textContent), browser = JSON.parse($("browser").textContent);
  out.differ = Object.keys(server).filter((k) => JSON.stringify(server[k]) !== JSON.stringify(browser[k])).map((k) => `${k}: ${JSON.stringify(server[k])} / ${JSON.stringify(browser[k])}`);
  out.values = browser;
  // A field read does not follow a local of its name, and a branch that went runs nothing in it.
  $("drop").click();
  await sleep(150);
  out.inner_gone = $("inner") === null;
  // What was typed stays, past a wait in the handler.
  const input = $("in");
  input.value = "hello";
  input.dispatchEvent(new Event("input", { bubbles: true }));
  await sleep(150);
  out.typed = $("typed").textContent;
  out.field = input.value;
  // A CR survives the server's HTML.
  out.cr = $("cr").value.length;
  // A long text read by index is not quadratic.
  const started = performance.now();
  await window.__long?.();
  out.errors = window.__quilloErrors || [];
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
