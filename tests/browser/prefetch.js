// Points at the link, waits for the page to be prefetched, then follows it.
(async () => {
  const a = document.getElementById("slow");
  a.dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
  await new Promise((r) => setTimeout(r, 1500));
  a.click();
})();
