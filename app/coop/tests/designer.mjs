// The loan designer and the board: the manager designs an agricultural loan,
// its terms checked and its sample schedule worked out in the browser as he
// types (a largest amount below the smallest is said at once); he saves it and
// puts it to the board; a director's rejection without a reason is refused;
// three directors approve it and it is offered, the manager's list showing it
// live, and a member can apply under it. The seeded providential loan is
// rejected by enough directors that it goes back to the manager as a draft.
//
//   BASE=http://127.0.0.1:8080 node feedback/tools/drive.mjs app/coop/tests/designer.mjs
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
  // The designer's field with this label: typed into as a person would.
  const field = (p, label, value) => p.ev(`const l = [...document.querySelectorAll(".designer label")].find((x) => x.textContent.trim().startsWith(${JSON.stringify(label)}));
    const i = l.querySelector("input, select"); i.focus(); i.value = ${JSON.stringify(value)};
    i.dispatchEvent(new Event(i.tagName == "SELECT" ? "change" : "input", { bubbles: true })); return true`);

  const mario = await login("mario");
  await open(mario, "/products");
  const before = await mario.ev(`return document.querySelectorAll("table.data tbody tr").length`);
  await open(mario, "/products/new");
  await field(mario, "Code", "agri-9");
  await mario.booted();
  await sleep(300);
  for (const [label, value] of [["Code", "agri-9"], ["Name", "Agricultural loan"], ["Description", "Seeds and fertiliser, paid back after the harvest."],
    ["Smallest amount", "3,000"], ["Largest amount", "50,000"], ["Shortest term", "3"], ["Longest term", "9"], ["Interest a year", "9"],
    ["Method", "flat"], ["Service fee", "1"], ["Insurance", "0"], ["Most: times", "2"], ["Months of membership", "3"], ["Co-makers", "0"], ["Board approvals", "3"]]) {
    await field(mario, label, value);
  }
  await sleep(300);
  check("the terms hold together", (await text(mario, ".designer .done")) == "The terms hold together.", await text(mario, ".designer section"));
  await mario.ev(`const l = [...document.querySelectorAll(".designer section")[1].querySelectorAll("input")]; const set = (i, v) => { i.focus(); i.value = v; i.dispatchEvent(new Event("input", { bubbles: true })); };
    set(l[0], "30000"); set(l[1], "6"); return true`);
  await sleep(300);
  // Flat 9% on ₱30,000 for 6 months: ₱1,350 interest; ₱5,225 a month.
  check("the sample schedule, worked out in the browser", (await text(mario, ".designer section:nth-child(2) .big") ?? "").startsWith("₱5,225.00"), await text(mario, ".designer section:nth-child(2)"));
  await field(mario, "Largest amount", "1,000");
  await sleep(200);
  check("a largest amount below the smallest is said at once", (await text(mario, ".designer .problem")) == "The largest amount is below the smallest", await text(mario, ".designer .problem"));
  await field(mario, "Largest amount", "50,000");
  await sleep(200);
  await click(mario, ".designer button", "Save the draft");
  check("saved, at its own address", await until(mario, `return location.pathname == "/products/AGRI-9"`), await mario.ev(`return location.href`));
  await mario.booted();
  await mario.ev(`[...document.querySelectorAll(".designer button")].find((b) => b.textContent.startsWith("Put it")).focus(); return true`);
  await mario.booted();
  await sleep(300);
  await click(mario, ".designer button", "Put it to the board");
  check("put to the board", await until(mario, `return document.querySelector(".designer h1 .status")?.textContent == "proposed"`), await text(mario, ".designer h1"));

  await open(mario, "/products");
  check("the list has it, proposed", await until(mario, `return [...document.querySelectorAll("table.data tbody tr")].some((r) => r.textContent.includes("AGRI-9") && r.textContent.includes("proposed"))`), await text(mario, "table.data"));

  const vote = async (who, code, v, comment = "") => {
    const p = await login(who);
    await open(p, "/board/products");
    await until(p, `return [...document.querySelectorAll("article.agenda")].some((a) => a.textContent.includes(${JSON.stringify(code)}))`);
    await p.ev(`const a = [...document.querySelectorAll("article.agenda")].find((x) => x.textContent.includes(${JSON.stringify(code)}));
      const i = a.querySelector("input"); i.focus(); i.value = ${JSON.stringify(comment)}; i.dispatchEvent(new Event("input", { bubbles: true })); return true`);
    await p.booted();
    await sleep(200);
    await p.ev(`const a = [...document.querySelectorAll("article.agenda")].find((x) => x.textContent.includes(${JSON.stringify(code)}));
      [...a.querySelectorAll("button")].find((b) => b.textContent == ${JSON.stringify(v == "approve" ? "Approve" : "Reject")}).click(); return true`);
    await sleep(700);
    return p;
  };
  const tan = await vote("dir.tan", "AGRI-9", "reject", "");
  check("a rejection needs a reason", await until(tan, `return [...document.querySelectorAll("article.agenda")].find((x) => x.textContent.includes("AGRI-9"))?.querySelector(".problem")?.textContent == "Say why, for the manager"`), await text(tan, "article.agenda"));
  await vote("dir.tan", "AGRI-9", "approve");
  await vote("dir.ramos", "AGRI-9", "approve", "Good for our farmer members");
  check("two approvals: still before the board", await until(mario, `return [...document.querySelectorAll("table.data tbody tr")].some((r) => r.textContent.includes("AGRI-9") && r.textContent.includes("2 for"))`), await text(mario, "table.data"));
  await vote("dir.cruz", "AGRI-9", "approve");
  check("three: offered, on the manager's list, live", await until(mario, `return [...document.querySelectorAll("table.data tbody tr")].some((r) => r.textContent.includes("AGRI-9") && r.textContent.includes("active"))`), await text(mario, "table.data"));

  const dan = await login("M-0004");
  await open(dan, "/me/loans/new");
  check("a member can apply under it", await dan.ev(`return [...document.querySelectorAll(".apply select option")].some((o) => o.value == "AGRI-9")`), await text(dan, ".apply select"));

  // Three of five reject the providential loan: it can no longer get its three approvals.
  await vote("dir.lim", "PROV", "reject", "Too long a term for our liquidity");
  await vote("dir.garcia", "PROV", "reject", "Wait for the audit");
  check("two rejections: still before the board", await until(mario, `return [...document.querySelectorAll("table.data tbody tr")].some((r) => r.textContent.includes("PROV") && r.textContent.includes("proposed"))`), await text(mario, "table.data"));
  await vote("dir.tan", "PROV", "reject", "Agree with the others");
  check("three: back to the manager as a draft", await until(mario, `return [...document.querySelectorAll("table.data tbody tr")].some((r) => r.textContent.includes("PROV") && r.textContent.includes("draft"))`), await text(mario, "table.data"));
  check("the list grew by one", (await mario.ev(`return document.querySelectorAll("table.data tbody tr").length`)) == before + 1, before);

  return { checks, passed: Object.values(checks).filter((v) => v === true).length, of: Object.keys(checks).length, problems: { mario: [...mario.problems, ...mario.mismatches], dan: [...dan.problems, ...dan.mismatches] } };
};
