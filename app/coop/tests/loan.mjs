// A loan from application to release, each step on its own screen, live:
// Ben's salary loan L-00002 waits for his co-maker Ana; Ana vouches from her
// dashboard; the clerk sees it in her queue and forwards it; three directors
// approve it from the board's agenda while Ben watches his loan's page; the
// clerk releases it and Ben's savings get the money. Then Dan applies for an
// emergency loan, its schedule worked out in the browser as he types, and the
// server keeps the same one.
//
//   BASE=http://127.0.0.1:8080 node feedback/tools/drive.mjs app/coop/tests/loan.mjs
//
// Needs COOP_DEMO=1, freshly seeded.

export default async ({ browser, sleep }) => {
  const base = process.env.BASE;
  const checks = {};
  const check = (name, ok, got) => { checks[name] = ok ? true : (got ?? false); };
  const until = async (p, code, ms = 6000) => {
    for (let t = 0; t < ms; t += 150) { try { if (await p.ev(code)) return true; } catch {} await sleep(150); }
    return false;
  };
  const text = (p, sel) => p.ev(`return document.querySelector(${JSON.stringify(sel)})?.textContent ?? null`);
  const click = (p, sel, label) => p.ev(`const b = [...document.querySelectorAll(${JSON.stringify(sel)})].find((x) => x.textContent.trim().startsWith(${JSON.stringify(label)})); if (!b) return false; b.click(); return true`);
  const login = async (who) => {
    const p = await browser.page("about:blank", { own: true });
    await p.go(base + "/login");
    await p.ev(`const f = document.querySelector("form.card"); f.username.value = ${JSON.stringify(who)}; f.password.value = "coop1234"; f.submit(); return true`);
    await sleep(900);
    return p;
  };
  const open = async (p, path) => { await p.go(base + path); await p.booted(); };
  const status = (p) => text(p, ".loan h1 .status");
  const savings = (p) => p.ev(`return [...document.querySelectorAll(".tile")].find((t) => t.textContent.includes("Savings"))?.querySelector("b")?.textContent ?? null`);

  const ben = await login("M-0002");
  await open(ben, "/me");
  const before = await savings(ben);
  await open(ben, "/me/loans/L-00002");
  check("Ben's loan waits for its co-maker", (await status(ben)) == "submitted", await status(ben));

  const ana = await login("M-0001");
  await open(ana, "/me");
  check("Ana is asked to co-make it", await until(ana, `return (document.querySelector(".ask")?.textContent ?? "").includes("L-00002")`), await text(ana, "main"));
  await click(ana, ".ask button", "I vouch");
  check("Ana's request goes once answered", await until(ana, `return document.querySelector(".ask") == null`));

  const clara = await login("clara");
  await open(clara, "/office");
  check("the office sees the co-maker agreed", await until(clara, `return (document.querySelector("main")?.textContent ?? "").includes("1 of 1 co-makers agreed")`), await text(clara, "main"));
  await open(clara, "/office/loans/L-00002");
  await click(clara, ".actions-card button", "Forward");
  check("forwarded to the board", await until(clara, `return document.querySelector(".loan h1 .status")?.textContent == "board"`), await status(clara));
  check("Ben sees it before the board, live", await until(ben, `return document.querySelector(".loan h1 .status")?.textContent == "board"`), await status(ben));

  const votes = [];
  for (const d of ["dir.cruz", "dir.lim", "dir.garcia"]) {
    const p = await login(d);
    await open(p, "/board");
    const found = await until(p, `return [...document.querySelectorAll("article.agenda")].some((a) => a.textContent.includes("L-00002"))`);
    await p.ev(`const a = [...document.querySelectorAll("article.agenda")].find((x) => x.textContent.includes("L-00002")); [...a.querySelectorAll("button")].find((b) => b.textContent == "Approve").click(); return true`);
    await sleep(600);
    votes.push(found);
  }
  check("each director found it on the agenda", votes.every(Boolean), votes);
  check("three approvals approve it, on Ben's screen", await until(ben, `return document.querySelector(".loan h1 .status")?.textContent == "approved"`), await status(ben));
  check("Ben sees the three votes", await until(ben, `return document.querySelectorAll(".votes li.approve").length == 3`), await text(ben, ".votes"));

  check("the clerk is offered the release, live", await until(clara, `return [...document.querySelectorAll(".actions-card button")].some((b) => b.textContent.startsWith("Release"))`), await text(clara, ".actions-card"));
  await click(clara, ".actions-card button", "Release");
  check("released", await until(ben, `return document.querySelector(".loan h1 .status")?.textContent == "released"`), await status(ben));
  check("Ben's schedule shows six installments", await until(ben, `return document.querySelectorAll(".loan table.data tbody tr").length == 6`), await ben.ev(`return document.querySelectorAll(".loan table.data tbody tr").length`));
  await open(ben, "/me");
  const after = await savings(ben);
  // ₱30,000 less ₱450 fee and ₱150 insurance.
  const toNum = (s) => Number(String(s).replace(/[^0-9.]/g, ""));
  check("Ben's savings got ₱29,400", toNum(after) - toNum(before) == 29400, `${before} -> ${after}`);

  // Dan applies for an emergency loan; the browser's schedule is the server's.
  const dan = await login("M-0004");
  await open(dan, "/me/loans/new");
  // Focused first, as a person would: the form's code loads when it is reached for.
  await dan.ev(`const s = document.querySelector(".apply select"); s.focus(); return true`);
  await dan.booted();
  await sleep(300);
  await dan.ev(`const s = document.querySelector(".apply select"); s.value = "EMR"; s.dispatchEvent(new Event("change", { bubbles: true })); return true`);
  await sleep(200);
  await dan.ev(`const set = (i, v) => { i.focus(); i.value = v; i.dispatchEvent(new Event("input", { bubbles: true })); };
    const ins = document.querySelectorAll(".apply input");
    set(ins[0], "10,000"); set(ins[1], "6"); set(ins[2], "Roof repair after the rains"); return true`);
  await sleep(300);
  const preview = await text(dan, ".apply .big");
  // Flat: ₱500 of interest over 6 months; the last installment takes the centavos left over.
  check("the browser works out the installment", (preview ?? "").includes("₱1,749.99"), preview);
  await click(dan, ".apply button", "Check it");
  check("the server would take it", await until(dan, `return (document.querySelector(".apply .done")?.textContent ?? "").includes("would take")`), await text(dan, ".apply"));
  await click(dan, ".apply button", "Apply");
  check("Dan's application is L-00005", await until(dan, `return location.pathname == "/me/loans/L-00005"`), await dan.ev(`return location.href`));
  await dan.booted();
  check("its installment is the one the browser showed", await until(dan, `return (document.querySelector(".facts")?.textContent ?? "").includes("₱1,749.99 a month")`), await text(dan, ".facts"));

  const problems = {};
  for (const [name, p] of Object.entries({ ben, ana, clara, dan })) problems[name] = [...p.problems, ...p.mismatches];
  return { checks, passed: Object.values(checks).filter((v) => v === true).length, of: Object.keys(checks).length, problems };
};
