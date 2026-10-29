// A month of salary deductions: the clerk makes Acme's batch (Ana's loan
// installment and savings, Ben's and Fred's savings); Acme's payroll staff see
// it, download its CSV and upload their remittance, Ana short, Fred over, and
// an employee number the batch does not have; each line is paid as far as the
// money goes, the rest goes to savings, and the unknown number is said. Then
// Luzon Telecom remits through the API, the same request sent twice with one
// Idempotency-Key, and it is posted once; a wrong key and a reused key with
// another body are refused, and one employer cannot open another's batch.
//
//   BASE=http://127.0.0.1:8080 node feedback/tools/drive.mjs app/coop/tests/payroll.mjs
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
  const line = (p, emp, kind) => p.ev(`return [...document.querySelectorAll("table.data tbody tr")].filter((r) => r.textContent.includes(${JSON.stringify(emp)}) && r.textContent.includes(${JSON.stringify(kind)})).map((r) => r.textContent)`);

  const clara = await login("clara");
  await open(clara, "/office/payroll");
  await clara.ev(`document.querySelector("form.card select").focus(); return true`);
  await clara.booted();
  await sleep(300);
  await click(clara, "form.card button", "Make it");
  check("Acme's batch: four lines, ₱9,330.93", await until(clara, `return (document.querySelector("form.card .done")?.textContent ?? "") == "4 lines, ₱9,330.93 in all."`), await text(clara, "form.card"));
  check("the batches list shows it, live", await until(clara, `return [...document.querySelectorAll("table.data tbody tr")].some((r) => r.textContent.includes("Acme") && r.textContent.includes("₱9,330.93"))`), await text(clara, "main"));
  const acmeBatch = await clara.ev(`return [...document.querySelectorAll("table.data tbody tr")].find((r) => r.textContent.includes("Acme")).querySelector("a").getAttribute("href").split("/").pop()`);

  const hr = await login("acme.hr");
  await open(hr, "/payroll");
  check("Acme's payroll sees its batch", await until(hr, `return [...document.querySelectorAll("table.data tbody tr")].length == 1`), await text(hr, "main"));
  await open(hr, `/payroll/${acmeBatch}`);
  const csv = await hr.ev(`return await (await fetch(${JSON.stringify(`/payroll/${acmeBatch}/csv`)})).text()`);
  check("the CSV has a line for each deduction", csv.split("\n").filter(Boolean).length == 5 && csv.includes("A-1001,Ana Dizon,loan,L-00004,5330.93"), csv);
  // Ana pays ₱6,000 of ₱7,330.93; Ben his ₱1,000; Fred ₱1,500 for ₱1,000; and someone the batch does not have.
  const remittance = "employee_no,amount\nA-1001,\"6,000.00\"\nA-1002,1000\nA-1003,1500\nA-9999,50\n";
  const r = await hr.ev(`const f = new File([${JSON.stringify(remittance)}], "november.csv", { type: "text/csv" }); return JSON.stringify(await window.quillo.server.upload_remittance(${acmeBatch}, f))`);
  check("posted, with the unknown number said", String(JSON.stringify(r)).includes("A-9999"), r);
  check("the batch is posted, live", await until(hr, `return document.querySelector("h1 .status")?.textContent == "posted"`), await text(hr, "h1"));
  const ana_loan = await line(hr, "A-1001", "loan");
  const ana_savings = await line(hr, "A-1001", "savings");
  check("Ana's installment is paid first", (ana_loan[0] ?? "").includes("₱5,330.93₱5,330.93paid"), ana_loan);
  check("her savings get what was left, short", (ana_savings[0] ?? "").includes("₱2,000.00₱669.07short"), ana_savings);
  check("Fred's line is paid", ((await line(hr, "A-1003", "savings"))[0] ?? "").includes("paid"), await line(hr, "A-1003", "savings"));

  const ana = await login("M-0001");
  await open(ana, "/me/loans/L-00004");
  check("Ana's first installment is settled", await until(ana, `return document.querySelector(".loan table.data tbody tr")?.classList.contains("settled")`), await text(ana, ".loan table.data tbody tr"));
  const fred = await login("M-0006");
  await open(fred, "/me/accounts/SV-0006");
  check("Fred's savings show the deduction and the excess", await until(fred, `return document.querySelector("table.data")?.textContent.includes("Remitted beyond the deductions")`), await text(fred, "table.data"));

  // Luzon Telecom, through the API.
  await open(clara, "/office/payroll");
  await clara.ev(`document.querySelector("form.card select").focus(); return true`);
  await clara.booted();
  await sleep(300);
  await clara.ev(`const s = document.querySelector("form.card select"); s.value = [...s.options].find((o) => o.textContent.includes("Luzon")).value; s.dispatchEvent(new Event("change", { bubbles: true })); return true`);
  await click(clara, "form.card button", "Make it");
  check("Luzon's batch: three savings lines", await until(clara, `return (document.querySelector("form.card .done")?.textContent ?? "").startsWith("3 lines")`), await text(clara, "form.card"));
  const mario = await login("mario");
  await open(mario, "/employers");
  const ltel = await mario.ev(`return [...document.querySelectorAll("table.data tbody tr")].findIndex((r) => r.textContent.includes("LTEL"))`);
  // A server fn called as the page calls it (the page's code loads only when a component is reached for).
  const key = await mario.ev(`const r = await fetch("/_quillo/rpc/new_api_key", { method: "POST", headers: { "content-type": "application/json" }, body: "[2]" }); return (await r.json()).ok`);
  check("the manager makes Luzon an API key", typeof key == "string" && key.startsWith("coop_"), key);
  const period = new Date().toISOString().slice(0, 7);
  const body = JSON.stringify({ period, lines: [{ employee_no: "T-2001", amount: "3000" }, { employee_no: "T-2002", amount: "500.00" }, { employee_no: "T-2003", amount: "1,500" }] });
  const idem = "test-" + Date.now();
  const send = (b, k, id) => fetch(base + "/api/remittances", { method: "POST", headers: { "content-type": "application/json", authorization: `Bearer ${k}`, "idempotency-key": id }, body: b });
  const first = await send(body, key, idem);
  const firstBody = await first.json();
  check("the API posts the remittance", first.status == 201 && firstBody.posted == "5000.00" && firstBody.unknown.length == 0, `${first.status} ${JSON.stringify(firstBody)}`);
  const again = await send(body, key, idem);
  check("sent again, the same answer, replayed", again.status == 201 && again.headers.get("idempotent-replayed") == "true" && JSON.stringify(await again.json()) == JSON.stringify(firstBody), `${again.status} ${again.headers.get("idempotent-replayed")}`);
  const other = await send(body.replace("3000", "3001"), key, idem);
  check("the key with another body is refused", other.status == 422, other.status);
  const wrong = await send(body, "coop_nope", "x-" + Date.now());
  check("a wrong key is refused", wrong.status == 401, wrong.status);
  const carla = await login("M-0003");
  await open(carla, "/me/accounts/SV-0003");
  const deductions = await carla.ev(`return [...document.querySelectorAll("table.data tbody tr")].filter((r) => r.textContent.includes("Salary savings")).length`);
  check("Carla's savings got it once", deductions == 1, deductions);
  const peek = await hr.ev(`return (await fetch("/payroll/${Number(acmeBatch) + 1}/csv")).status`);
  check("Acme cannot read Luzon's batch", peek == 404, peek);

  const problems = {};
  for (const [name, p] of Object.entries({ clara, hr, ana, fred, mario, carla })) problems[name] = [...p.problems, ...p.mismatches];
  return { checks, passed: Object.values(checks).filter((v) => v === true).length, of: Object.keys(checks).length, problems, ltel };
};
