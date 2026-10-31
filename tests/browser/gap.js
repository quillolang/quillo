// Writes for a second and a half and counts the refreshes.
(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  await sleep(300);
  const sent = () => performance.getEntriesByType("resource").filter((e) => e.name.endsWith("/rpc/seen")).length;
  for (let i = 0; i < 10; i++) {
    document.getElementById("add").click();
    await sleep(150);
  }
  await sleep(1500);
  const out = { errors: window.__quilloErrors || [], refreshes_few: sent() >= 1 && sent() <= 3, ended_with_the_last: document.getElementById("slow").textContent };
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
