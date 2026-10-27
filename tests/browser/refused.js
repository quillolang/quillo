// Points at the link (which prefetches it), waits for the prefetch, clicks.
(async () => {
  const a = document.getElementById("go");
  a.dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
  await new Promise((r) => setTimeout(r, 800));
  a.click();
})();
