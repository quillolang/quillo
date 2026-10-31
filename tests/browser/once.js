// Clicks twice, as a person in a hurry does, and counts what reached the server.
(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const sent = (f) => performance.getEntriesByType("resource").filter((e) => e.name.endsWith("/rpc/" + f)).length;
  const twice = (id) => { document.getElementById(id).click(); document.getElementById(id).click(); };
  const count = async () => { document.getElementById("count").click(); await sleep(200); return document.getElementById("n").textContent; };
  const out = { errors: [] };
  twice("send");
  await sleep(600);
  out.form = [sent("pay"), await count()];
  out.said = document.getElementById("said").textContent;
  twice("click");
  await sleep(600);
  out.click = [sent("pay"), await count()];
  // Done, so the same payment again is a new one.
  document.getElementById("click").click();
  await sleep(600);
  out.again = [sent("pay"), await count()];
  twice("helper");
  await sleep(600);
  out.helper = [sent("pay_by_helper"), await count()];
  // Nothing changes for a function that did not ask.
  twice("plain");
  await sleep(300);
  out.plain = [sent("plain"), await count()];
  out.errors = window.__quilloErrors || [];
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
