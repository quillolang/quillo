// Three clicks on each button, then what each shows.
export default async ({ page, sleep }) => {
  await page.go(`http://127.0.0.1:${process.env.PORT}/`);
  await sleep(1000);
  const html = await page.ev(`return document.body.innerHTML.replace(/<script[\\s\\S]*?<\\/script>/g, "").slice(0, 300)`);
  for (const id of ["view", "component"]) {
    for (let i = 0; i < 3; i++) { await page.click(...Object.values(await page.center("#" + id))); await sleep(300); }
  }
  await sleep(500);
  const text = (id) => page.ev(`return document.getElementById(${JSON.stringify(id)}).textContent`);
  return { view: await text("view"), component: await text("component"), html };
};
