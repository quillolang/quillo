// A leave request from start to end, in three browsers at once: an operator
// (E0001, line L01, crew A) asks for next week off, the supervisor of L01
// (S01) sees it arrive and approves it, and the manager (M01) watches next
// week's manpower drop. The operator's calendar is checked against the
// rotation worked out here.
//
//   BASE=http://127.0.0.1:8080 node feedback/tools/drive.mjs app/shifts/tests/leave.mjs
//
// Needs a freshly seeded database (lib/seed.qwe).

export default async ({ browser, sleep }) => {
  const base = process.env.BASE;
  const checks = {};
  const check = (name, ok, got) => { checks[name] = ok ? true : (got ?? false); };
  const until = async (p, code, ms = 6000) => {
    for (let t = 0; t < ms; t += 150) { try { if (await p.ev(code)) return true; } catch {} await sleep(150); }
    return false;
  };
  const day = (d) => d.toISOString().slice(0, 10);
  const addDays = (s, n) => { const d = new Date(s + "T00:00:00Z"); d.setUTCDate(d.getUTCDate() + n); return day(d); };
  const between = (a, b) => Math.round((Date.parse(b) - Date.parse(a)) / 86400000);
  const shift = (d, offset) => "DDNNRR"[(((between("2026-01-05", d) - offset) % 6) + 6) % 6];
  // The factory's date, not this machine's (PUBLIC_FACTORY_TZ, Europe/Berlin by default).
  const today = process.env.TODAY || new Intl.DateTimeFormat("en-CA", { timeZone: process.env.PUBLIC_FACTORY_TZ || "Europe/Berlin" }).format(new Date());
  const weekday = (new Date(today + "T00:00:00Z").getUTCDay() + 6) % 7 + 1;
  const monday = addDays(today, 8 - weekday);
  const sunday = addDays(monday, 6);

  const signIn = async (who) => {
    const p = await browser.page("about:blank", { own: true });
    await p.go(base + "/login");
    await p.submitForm("form", { emp_no: who, password: "shift123" });
    await p.booted();
    await sleep(500);
    return p;
  };

  const mgr = await signIn("M01");
  const cell = (line, d, s) => mgr.ev(`
    const tr = document.querySelector('tr[data-line="${line}"]');
    const ths = [...document.querySelectorAll("table.grid thead tr:first-child th")].slice(1).map((t) => t.textContent);
    return tr ? [...tr.querySelectorAll("td")].map((td) => td.textContent) : null`);
  const before = await cell("L01");
  check("the manager sees next week, 7 days of two shifts", before && before.length == 14, before);

  const sup = await signIn("S01");
  const waiting = () => sup.ev(`return document.querySelectorAll("li.request").length`);
  const waitingBefore = await waiting();

  const op = await signIn("E0001");
  check("the operator lands on their roster", await op.ev(`return location.pathname`) == "/me", await op.ev(`return location.pathname`));
  const month = today.slice(0, 7);
  const cal = await op.ev(`return [...document.querySelectorAll(".cal .day")].map((d) => [d.dataset.day, d.querySelector(".s").textContent])`);
  const expected = cal.map(([d]) => [d, shift(d, 0)]);
  check("the calendar is this month", cal.length > 27 && cal.every(([d]) => d.startsWith(month)), cal.length);
  check("the calendar follows the rotation", JSON.stringify(cal) == JSON.stringify(expected), { cal, expected });
  check("today is marked", await op.ev(`return document.querySelector(".day.today")?.dataset.day`) == today);
  check("the first day sits under its weekday", await op.ev(`return document.querySelectorAll(".cal i").length`) == (new Date(month + "-01T00:00:00Z").getUTCDay() + 6) % 7);

  await op.ev(`document.querySelector('[aria-label="Next month"]').click(); return true`);
  check("next month shows", await until(op, `return document.querySelector(".cal .day").dataset.day.slice(0, 7) == "${addDays(month + "-28", 7).slice(0, 7)}"`));

  // Ask for next week.
  const set = (sel, v) => op.ev(`const i = document.querySelector('${sel}'); i.value = ${JSON.stringify(v)}; i.dispatchEvent(new Event("input", { bubbles: true })); i.dispatchEvent(new Event("change", { bubbles: true })); return true`);
  await set('input[name="start"]', monday);
  await set('input[name="end"]', sunday);
  const working = [...Array(7).keys()].map((i) => addDays(monday, i)).filter((d) => shift(d, 0) != "R").length;
  check("the form counts the working shifts as days are picked", await until(op, `return document.querySelector(".count").textContent.startsWith("${working} working")`), await op.ev(`return document.querySelector(".count").textContent`));
  await set('textarea[name="note"]', "Family visit <b>abroad</b>");
  await op.ev(`document.querySelector("form.card button.primary").click(); return true`);
  check("the request is sent", await until(op, `return !!document.querySelector("p.ok")`), await op.ev(`return document.querySelector("form.card").innerText`));
  check("it shows as pending among my requests", await until(op, `return [...document.querySelectorAll("table.list tbody tr")].some((r) => r.innerText.includes("pending") && r.innerText.includes("${working}"))`));

  check("the supervisor sees it arrive, without reloading", await until(sup, `return document.querySelectorAll("li.request").length == ${waitingBefore + 1}`, 8000), { before: waitingBefore, now: await waiting() });
  check("the note is shown as text", await sup.ev(`return [...document.querySelectorAll("li.request .note")].some((n) => n.textContent == "Family visit <b>abroad</b>")`));
  check("the manager sees it asked for", await until(mgr, `return [...document.querySelectorAll('tr[data-line="L01"] td sup')].length > 0`, 8000), await cell("L01"));

  // Approve it.
  const id = await sup.ev(`return [...document.querySelectorAll("li.request")].find((li) => li.innerText.includes("E0001"))?.dataset.id`);
  await sup.ev(`
    const li = document.querySelector('li.request[data-id="${id}"]');
    const box = li.querySelector('input[type=checkbox]');
    if (box) { box.click(); const r = li.querySelector('input[aria-label=Reason]'); r.value = "covered by overtime"; r.dispatchEvent(new Event("input", { bubbles: true })); }
    await new Promise((r) => setTimeout(r, 100));
    li.querySelector("button.primary").click(); return true`);
  check("the supervisor's list drops it", await until(sup, `return !document.querySelector('li.request[data-id="${id}"]')`, 8000), await sup.ev(`return document.querySelector('li.request[data-id="${id}"] .problem')?.textContent`));
  check("the operator sees it approved", await until(op, `return [...document.querySelectorAll("table.list tbody tr")].some((r) => r.dataset.id == "${id}" && r.innerText.includes("approved"))`, 8000));
  check("the operator's calendar marks the leave", await until(op, `return document.querySelectorAll(".day.leave.approved").length > 0`), await op.ev(`return document.querySelectorAll(".day.leave").length`));
  const after = await cell("L01");
  check("the manager's grid lost one operator on each of those shifts", await until(mgr, `
    const now = [...document.querySelector('tr[data-line="L01"]').querySelectorAll("td")].map((td) => td.textContent);
    return now.filter((t, i) => t != ${JSON.stringify(before)}[i]).length == ${working}`, 8000), { before, after: await cell("L01") });

  const q = { op: await op.quillo(), sup: await sup.quillo(), mgr: await mgr.quillo() };
  check("nothing made again, no errors caught", Object.values(q).every((x) => x.remade == 0 && x.errors.length == 0), q);
  return { checks, passed: Object.values(checks).filter((v) => v === true).length, of: Object.keys(checks).length,
           problems: { op: op.problems, sup: sup.problems, mgr: mgr.problems } };
};
