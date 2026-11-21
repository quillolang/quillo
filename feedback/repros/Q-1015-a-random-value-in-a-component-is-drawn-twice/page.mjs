// The ticket as the server sent it, then after pointing at the raffle (which wakes it).
export default async ({ page, sleep }) => {
  await page.go(`http://127.0.0.1:${process.env.PORT}/`);
  await sleep(1500);
  const ticket = `return document.querySelector(".raffle b").textContent`;
  const before = await page.ev(ticket);
  await page.ev(`document.querySelector(".raffle p").dispatchEvent(new PointerEvent("pointerover", { bubbles: true }))`);
  await sleep(1200);
  const after = await page.ev(ticket);
  const q = await page.quillo();
  return { before, after, same: before === after, resumed: q.resumed, remade: q.remade };
};
