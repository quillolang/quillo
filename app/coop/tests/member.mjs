// A member's coop, from joining to the stores: someone applies on the public
// form; the clerk approves them and reads their first password; they sign in
// and must choose their own; the clerk takes cash for their share capital and
// savings, which their dashboard shows as it happens. Then Carla, a member
// for years, opens a fixed deposit, orders from the store (the office marks it
// ready and collected), buys raffle tickets, books a paid seminar and is let
// in at the door, and asks the help desk, whose answer she sees come in.
//
//   BASE=http://127.0.0.1:8080 node feedback/tools/drive.mjs app/coop/tests/member.mjs
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
  // Types into a field as a person would: focused first, so its component's code loads.
  const fill = (p, sel, value, nth = 0) => p.ev(`const i = document.querySelectorAll(${JSON.stringify(sel)})[${nth}]; i.focus(); i.value = ${JSON.stringify(value)}; i.dispatchEvent(new Event("input", { bubbles: true })); return true`);
  const choose = (p, sel, value, nth = 0) => p.ev(`const s = document.querySelectorAll(${JSON.stringify(sel)})[${nth}]; s.focus(); s.value = ${JSON.stringify(value)}; s.dispatchEvent(new Event("change", { bubbles: true })); return true`);
  const login = async (who, password = "coop1234") => {
    const p = await browser.page("about:blank", { own: true });
    await p.go(base + "/login");
    await p.ev(`const f = document.querySelector("form.card"); f.username.value = ${JSON.stringify(who)}; f.password.value = ${JSON.stringify(password)}; f.submit(); return true`);
    await sleep(900);
    return p;
  };
  const open = async (p, path) => { await p.go(base + path); await p.booted(); };
  const tile = (p, label) => p.ev(`return [...document.querySelectorAll(".tile")].find((t) => t.textContent.includes(${JSON.stringify(label)}))?.querySelector("b")?.textContent ?? null`);
  const pesos = (s) => Number(String(s ?? "").replace(/[^0-9.]/g, ""));

  // Joining.
  const visitor = await browser.page("about:blank", { own: true });
  await visitor.go(base + "/join");
  await visitor.ev(`const f = document.querySelector("form.card"); const v = { first_name: "Juan", last_name: "dela Cruz", birth_date: "1990-06-12", phone: "0917 555 0101",
    address: "3 Luna St., Makati", email: "juan@example.ph", employer_code: "LTEL", employee_no: "T-2099", pledge: "2,000", monthly_savings: "500" };
    for (const [k, x] of Object.entries(v)) f[k].value = x; f.submit(); return true`);
  await sleep(900);
  check("the application is taken", (await text(visitor, "main h1")) == "Thank you", await text(visitor, "main"));
  await visitor.go(base + "/join");
  await visitor.ev(`const f = document.querySelector("form.card"); const v = { first_name: "Kid", last_name: "Young", birth_date: "2015-01-01", phone: "09175550102",
    address: "x", email: "kid@example.ph", pledge: "2000" }; for (const [k, x] of Object.entries(v)) f[k].value = x; f.submit(); return true`);
  await sleep(700);
  check("a child is refused, with the form kept", (await text(visitor, ".problem")) == "Members are 18 or older" && (await visitor.ev(`return document.querySelector("form.card").first_name.value`)) == "Kid", await text(visitor, ".problem"));

  const clara = await login("clara");
  await open(clara, "/office");
  check("the office sees Juan's application", await until(clara, `return (document.querySelector("li.application")?.textContent ?? "").includes("Juan")`) || await until(clara, `return [...document.querySelectorAll("li.application")].some((l) => l.textContent.includes("Juan"))`), await text(clara, "main"));
  await clara.ev(`const li = [...document.querySelectorAll("li.application")].find((l) => l.textContent.includes("Juan")); li.querySelector("button").click(); return true`);
  check("approved, with a first password", await until(clara, `return (document.querySelector(".given")?.textContent ?? "").includes("Juan dela Cruz is member M-0009")`), await text(clara, "main"));
  check("and off the list", await until(clara, `return ![...document.querySelectorAll("li.application")].some((l) => l.textContent.includes("Juan"))`), await text(clara, "main"));
  const password = await text(clara, ".given code");

  const juan = await login("M-0009", password);
  check("a first sign-in asks for a new password", await juan.ev(`return location.pathname`) == "/me/password", await juan.ev(`return location.href`));
  await juan.ev(`const f = document.querySelector("form.card"); f.old.value = ${JSON.stringify(password)}; f["new"].value = "juan-secret-1"; f.submit(); return true`);
  await sleep(900);
  await open(juan, "/me");
  check("Juan's coop, empty", (await tile(juan, "Savings")) == "₱0.00" && (await tile(juan, "Share capital")) == "₱0.00", await text(juan, ".tiles"));

  await open(clara, "/office/members/M-0009");
  await choose(clara, "form.card select", "share", 0);
  await fill(clara, "form.card input[inputmode=decimal]", "2,000");
  await click(clara, "form.card button", "Post");
  check("the clerk takes ₱2,000 of share capital", await until(clara, `return (document.querySelector("form.card .done")?.textContent ?? "").includes("₱2,000.00")`), await text(clara, "form.card"));
  check("Juan sees his share capital, live", await until(juan, `return [...document.querySelectorAll(".tile")].some((t) => t.textContent.includes("Share capital") && t.textContent.includes("₱2,000.00"))`), await text(juan, ".tiles"));
  await choose(clara, "form.card select", "savings", 0);
  await fill(clara, "form.card input[inputmode=decimal]", "5000");
  await click(clara, "form.card button", "Post");
  check("and his savings", await until(juan, `return [...document.querySelectorAll(".tile")].some((t) => t.textContent.includes("Savings") && t.textContent.includes("₱5,000.00"))`), await text(juan, ".tiles"));
  await choose(clara, "form.card select", "out", 1);
  await fill(clara, "form.card input[inputmode=decimal]", "4,800");
  await click(clara, "form.card button", "Post");
  check("a withdrawal below the least to keep is refused", await until(clara, `return (document.querySelector("form.card .problem")?.textContent ?? "").includes("the least to keep is ₱500.00")`), await text(clara, "form.card"));

  // Carla: a fixed deposit.
  const carla = await login("M-0003");
  await open(carla, "/me");
  const s0 = pesos(await tile(carla, "Savings"));
  await fill(carla, ".mini input[inputmode=decimal]", "20,000", 1);
  check("the deposit's interest is offered", await until(carla, `return (document.querySelectorAll(".mini")[1].textContent).includes("₱1,100.00 interest after 12 months")`), await carla.ev(`return document.querySelectorAll(".mini")[1].textContent`));
  await click(carla, ".mini button", "Open");
  check("the deposit opens", await until(carla, `return [...document.querySelectorAll(".tile")].some((t) => t.textContent.includes("Fixed deposit") && t.textContent.includes("₱20,000.00"))`), await text(carla, ".tiles"));
  check("from her savings", await until(carla, `return true`) && pesos(await tile(carla, "Savings")) == s0 - 20000, `${s0} -> ${await tile(carla, "Savings")}`);

  // The store.
  await open(carla, "/store/mart");
  await carla.ev(`const li = [...document.querySelectorAll(".items li")]; const plus = (name) => [...li.find((x) => x.textContent.includes(name)).querySelectorAll("button")].pop();
    plus("Rice").focus(); return true`);
  await carla.booted();
  await sleep(300);
  await carla.ev(`const li = [...document.querySelectorAll(".items li")]; const plus = (name) => [...li.find((x) => x.textContent.includes(name)).querySelectorAll("button")].pop();
    plus("Rice").click(); plus("Rice").click(); plus("Cooking oil").click(); return true`);
  check("the cart adds up at members' prices", await until(carla, `return (document.querySelector(".cart .big")?.textContent ?? "") == "₱2,750.00"`), await text(carla, ".cart"));
  await click(carla, ".cart button", "Order");
  check("the order is placed", await until(carla, `return (document.querySelector(".cart .done")?.textContent ?? "").includes("O-00001")`), await text(carla, ".cart"));
  check("the stock goes down", await until(carla, `return [...document.querySelectorAll(".items li")].find((x) => x.textContent.includes("Rice")).textContent.includes("38 in stock")`), await text(carla, ".items"));
  await open(clara, "/office");
  check("the office sees the order", await until(clara, `return (document.querySelector("main").textContent).includes("O-00001")`), await text(clara, "main"));
  await clara.ev(`const tr = [...document.querySelectorAll("tr")].find((r) => r.textContent.includes("O-00001")); tr.querySelector("button").click(); return true`);
  await until(clara, `return [...document.querySelectorAll("tr")].find((r) => r.textContent.includes("O-00001"))?.textContent.includes("ready")`);
  await clara.ev(`const tr = [...document.querySelectorAll("tr")].find((r) => r.textContent.includes("O-00001")); tr.querySelector("button").click(); return true`);
  check("ready, then collected", await until(clara, `return ![...document.querySelectorAll("tr")].some((r) => r.textContent.includes("O-00001"))`), await text(clara, "main"));

  // A raffle.
  await open(carla, "/raffles");
  await fill(carla, ".raffle input[type=number]", "3");
  await click(carla, ".raffle button", "Buy");
  check("three raffle tickets", await until(carla, `return (document.querySelector(".raffle .done")?.textContent ?? "").includes("1, 2, 3")`), await text(carla, ".raffle"));

  // A paid seminar, and the door.
  await open(carla, "/events");
  await carla.ev(`const e = [...document.querySelectorAll(".event")].find((x) => x.textContent.includes("Financial literacy")); e.querySelector("button").focus(); return true`);
  await carla.booted();
  await sleep(300);
  await carla.ev(`const e = [...document.querySelectorAll(".event")].find((x) => x.textContent.includes("Financial literacy")); e.querySelector("button").click(); return true`);
  check("a place booked, with a code", await until(carla, `return !!document.querySelector(".event .done b.code")`), await text(carla, "main"));
  const code = await text(carla, ".event .done b.code");
  const slug = await clara.ev(`return (await (await fetch("/office/events")).text()).match(/\\/office\\/events\\/(financial-literacy-[0-9-]+)/)[1]`);
  await open(clara, `/office/events/${slug}`);
  await fill(clara, "form.card input", "zzzzzz");
  await click(clara, "form.card button", "Check in");
  check("a wrong code is refused", await until(clara, `return (document.querySelector(".problem")?.textContent ?? "").includes("No ticket ZZZZZZ")`), await text(clara, "form.card"));
  await fill(clara, "form.card input", code.toLowerCase());
  await click(clara, "form.card button", "Check in");
  check("Carla is let in", await until(clara, `return (document.querySelector(".done")?.textContent ?? "").includes("Carla Mendoza")`), await text(clara, "form.card"));
  check("the door counts her", await until(clara, `return (document.querySelectorAll(".card h2")[0]?.textContent ?? "").startsWith("1 of 1 arrived")`), await text(clara, "main"));
  await click(clara, "form.card button", "Check in");
  await fill(clara, "form.card input", code);
  await click(clara, "form.card button", "Check in");
  check("not twice", await until(clara, `return (document.querySelector(".problem")?.textContent ?? "").includes("came in already")`), await text(clara, "form.card"));

  // The help desk.
  await open(carla, "/me/help");
  await fill(carla, "form.card input", "Interest on my deposit");
  await fill(carla, "form.card textarea", "When is the interest on FD paid, at maturity or monthly?");
  await click(carla, "form.card button", "Send");
  check("the ticket opens", await until(carla, `return location.pathname.startsWith("/me/help/")`), await carla.ev(`return location.href`));
  await carla.booted();
  const ticket = await carla.ev(`return location.pathname.split("/").pop()`);
  await open(clara, `/office/help/${ticket}`);
  await fill(clara, "form textarea", "At maturity, with the principal, into your savings.");
  await click(clara, "form button", "Reply");
  check("Carla sees the answer, live", await until(carla, `return (document.querySelector(".thread")?.textContent ?? "").includes("At maturity")`), await text(carla, ".thread"));
  check("the ticket is answered", await until(carla, `return document.querySelector("h1 .status")?.textContent == "answered"`), await text(carla, "h1"));

  const problems = {};
  for (const [name, p] of Object.entries({ visitor, clara, juan, carla })) problems[name] = [...p.problems, ...p.mismatches];
  return { checks, passed: Object.values(checks).filter((v) => v === true).length, of: Object.keys(checks).length, problems };
};
