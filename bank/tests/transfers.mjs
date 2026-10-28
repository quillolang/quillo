// Money between people, live: Ada and Ben on their phones, Omar (a
// supervisor) at his desk, each in a browser of their own.
//
//   BASE=http://localhost:8080 node feedback/tools/drive.mjs bank/tests/transfers.mjs
//
// Needs BANK_DEMO=1, freshly seeded.

export default async ({ browser, sleep }) => {
  const base = process.env.BASE;
  const checks = {};
  const check = (name, ok, got) => { checks[name] = ok ? true : (got ?? false); };
  const until = async (p, code, ms = 6000) => {
    for (let t = 0; t < ms; t += 150) { try { if (await p.ev(code)) return true; } catch {} await sleep(150); }
    return false;
  };
  const text = (p, sel) => p.ev(`return document.querySelector(${JSON.stringify(sel)})?.textContent ?? null`);
  const signInClient = async (p, number) => {
    await p.go(base + "/login");
    await p.ev(`const f = document.querySelector("form.stack"); f.number.value = ${JSON.stringify(number)}; f.pin.value = "246810"; f.submit(); return true`);
    await sleep(900);
  };
  const balance = (p, name) => p.ev(`return [...document.querySelectorAll(".card")].find((c) => c.querySelector(".kind").textContent.startsWith(${JSON.stringify(name)}))?.querySelector(".balance").textContent ?? null`);
  const setInput = (p, sel, value) => p.ev(`const i = document.querySelector(${JSON.stringify(sel)}); i.value = ${JSON.stringify(value)}; i.dispatchEvent(new Event("input", { bubbles: true })); return true`);

  const ada = await browser.page("about:blank", { own: true, phone: true });
  const ben = await browser.page("about:blank", { own: true, phone: true });
  const omar = await browser.page("about:blank", { own: true });
  await signInClient(ada, "10000001");
  await signInClient(ben, "10000002");
  await omar.go(base + "/staff/login");
  await omar.ev(`const f = document.querySelector("form.login"); f.username.value = "omar"; f.password.value = "password"; f.submit(); return true`);
  await sleep(900);
  await omar.booted();
  await ben.booted();
  const benBefore = await balance(ben, "Everyday");
  const benNumber = await ben.ev(`return document.querySelector(".card .number").textContent`);
  check("Ben starts with €848.00", benBefore === "€848.00", benBefore);

  // ---- the form checks what is typed, in the browser ----
  await ada.go(base + "/transfer");
  await ada.booted();
  const typo = benNumber.replace(/ /g, "").slice(0, -1) + ((+benNumber.slice(-1) + 1) % 10);
  await setInput(ada, "form.transfer input", typo);
  check("a typo in the account number is caught", await until(ada, `return (document.querySelector(".field-problem")?.textContent ?? "").includes("typo")`), await text(ada, "form.transfer"));
  await setInput(ada, "form.transfer input", benNumber);
  check("the payee's name shows", await until(ada, `return (document.querySelector(".payee")?.textContent ?? "").includes("Ben O.")`), await text(ada, "form.transfer"));
  await setInput(ada, 'form.transfer input[inputmode="decimal"]', "99999");
  check("more than the balance is caught", await until(ada, `return (document.querySelector(".field-problem")?.textContent ?? "").includes("More than")`), await text(ada, "form.transfer"));
  check("and Send is off", await ada.ev(`return document.querySelector("form.transfer button").disabled`));

  // ---- a small transfer: Ben's balance moves while he looks ----
  await setInput(ada, 'form.transfer input[inputmode="decimal"]', "25,50");
  await ada.ev(`const m = document.querySelectorAll("form.transfer input")[2]; m.value = "Lunch"; m.dispatchEvent(new Event("input", { bubbles: true })); document.querySelector("form.transfer button").click(); return true`);
  check("sent", await until(ada, `return document.querySelector(".result.done") != null`), await text(ada, "main.app"));
  check("Ben's balance follows, live", await until(ben, `return [...document.querySelectorAll(".card")].some((c) => c.querySelector(".balance").textContent == "€873.50")`), await balance(ben, "Everyday"));
  check("Ben's messages tab shows one unread", await until(ben, `return document.querySelector(".tab .dot") != null`));

  // ---- a large one is held; Omar sees it arrive ----
  const heldBefore = await omar.ev(`return document.querySelectorAll("li.held-row").length`);
  await ada.ev(`[...document.querySelectorAll(".result button")].find((b) => b.textContent.includes("Another")).click(); return true`);
  await until(ada, `return document.querySelector("form.transfer") != null`);
  // From savings, which never paid Ben: a large first payment is checked.
  await ada.ev(`const s = document.querySelector("form.transfer select"); s.value = [...s.options].find((o) => o.textContent.startsWith("Rainy day")).value; s.dispatchEvent(new Event("change", { bubbles: true })); return true`);
  await setInput(ada, "form.transfer input", benNumber);
  await setInput(ada, 'form.transfer input[inputmode="decimal"]', "1500");
  await ada.ev(`document.querySelector("form.transfer button").click(); return true`);
  check("held, with why", await until(ada, `return document.querySelector(".result.held") != null && document.querySelector(".result .reasons") != null`), await text(ada, "main.app"));
  check("Omar sees it arrive, live", await until(omar, `return document.querySelectorAll("li.held-row").length == ${heldBefore + 1}`), await omar.ev(`return document.querySelectorAll("li.held-row").length`));
  await ada.go(base + "/");
  await ada.booted();
  check("Ada's home says it is being checked", (await text(ada, ".rows"))?.includes("Being checked"), await text(ada, ".rows"));

  // Omar approves it: both phones follow.
  await omar.ev(`const h = [...document.querySelectorAll("li.held-row")].find((x) => x.textContent.includes("€1,500.00")); [...h.querySelectorAll("button")].find((b) => b.textContent == "Approve").click(); return true`);
  check("gone from the queue", await until(omar, `return ![...document.querySelectorAll("li.held-row")].some((x) => x.textContent.includes("€1,500.00"))`));
  check("Ada's home says sent, live", await until(ada, `return document.querySelector(".rows li .status")?.textContent == "Sent"`), await text(ada, ".rows"));
  check("Ben's balance follows, live", await until(ben, `return [...document.querySelectorAll(".card")].some((c) => c.querySelector(".balance").textContent == "€2,373.50")`), await balance(ben, "Everyday"));

  // The seeded €12,500 from Chloé: Omar rejects it, with a note.
  await omar.ev(`const h = [...document.querySelectorAll("li.held-row")].find((x) => x.textContent.includes("€12,500.00")); const i = h.querySelector("input"); i.value = "Please call us about this one"; i.dispatchEvent(new Event("input", { bubbles: true })); [...h.querySelectorAll("button")].find((b) => b.textContent == "Reject").click(); return true`);
  check("rejected and gone", await until(omar, `return ![...document.querySelectorAll("li.held-row")].some((x) => x.textContent.includes("€12,500.00"))`));

  // ---- a frozen account ----
  await omar.go(base + "/staff/customers/10000002");
  await omar.booted();
  await omar.ev(`window.confirm = () => true; [...document.querySelectorAll(".accounts button")].find((b) => b.textContent == "Freeze").click(); return true`);
  await until(omar, `return document.querySelector(".q-dialog") != null`, 2000);
  await omar.ev(`const b = [...document.querySelectorAll(".q-dialog button")].find((x) => x.textContent == "Freeze"); b && b.click(); return true`);
  check("frozen, on Omar's screen", await until(omar, `return [...document.querySelectorAll(".accounts button")].some((b) => b.textContent == "Unfreeze")`));
  check("Ben sees it frozen, live", await until(ben, `return (document.querySelector(".card .kind")?.textContent ?? "").includes("frozen")`), await text(ben, ".cards"));
  await ada.go(base + "/transfer");
  await ada.booted();
  await setInput(ada, "form.transfer input", benNumber);
  await setInput(ada, 'form.transfer input[inputmode="decimal"]', "5");
  await sleep(300);
  await ada.ev(`document.querySelector("form.transfer button").click(); return true`);
  check("a transfer to it is refused", await until(ada, `return document.querySelector(".result.denied") != null || (document.querySelector(".problem")?.textContent ?? "") != ""`), await text(ada, "main.app"));

  // ---- the ledger still adds up ----
  const health = await (await fetch(base + "/health")).json();
  check("the ledger adds up", health.ok === true && health.ledger.length === 0, JSON.stringify(health));

  const pages = { ada, ben, omar };
  return {
    checks,
    problems: Object.fromEntries(Object.entries(pages).map(([k, p]) => [k, p.problems])),
    mismatches: Object.fromEntries(Object.entries(pages).map(([k, p]) => [k, p.mismatches])),
  };
};
