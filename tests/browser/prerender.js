(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const rpcs = () => performance.getEntriesByType("resource").filter((e) => e.name.includes("/_quillo/rpc/")).length;
  await sleep(1200);
  const held = { text: document.getElementById("mounted").textContent, calls: rpcs() };
  Object.defineProperty(document, "prerendering", { value: false, configurable: true });
  document.dispatchEvent(new Event("prerenderingchange"));
  for (let i = 0; i < 60 && document.getElementById("mounted").textContent !== "ran"; i++) await sleep(50);
  const shown = { text: document.getElementById("mounted").textContent, calls: rpcs() };
  document.body.setAttribute("data-result", JSON.stringify({ held, shown, errors: window.__quilloErrors || [] }));
})();
