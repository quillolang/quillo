(async () => {
  const a = document.getElementById("count");
  a.dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
  await new Promise((r) => setTimeout(r, 1500));
  await fetch("/bump", { method: "POST" });
  a.click();
})();
