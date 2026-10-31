(async () => {
  await fetch("/login", { method: "POST" });
  document.getElementById("me").dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
  await new Promise((r) => setTimeout(r, 1500));
  document.getElementById("out").submit();
})();
