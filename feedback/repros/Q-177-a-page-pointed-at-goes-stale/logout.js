(async () => {
  await fetch("/login", { method: "POST" });
  const a = document.getElementById("me");
  a.dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
  await new Promise((r) => setTimeout(r, 1500));
  await fetch("/logout", { method: "POST" });
  a.click();
})();
