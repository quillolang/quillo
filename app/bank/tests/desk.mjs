// A new customer opens an account at the desk: the clerk (Maria) on a
// desktop, the client on a phone, each in a browser of their own.
//
//   BASE=http://localhost:8080 node feedback/tools/drive.mjs app/bank/tests/desk.mjs
//
// Needs BANK_DEMO=1 (Maria). Returns the checks, each true or the value found.

export default async ({ browser, page: _unused, sleep }) => {
  const base = process.env.BASE;
  const checks = {};
  const check = (name, ok, got) => { checks[name] = ok ? true : (got ?? false); };
  const until = async (p, code, ms = 6000) => {
    for (let t = 0; t < ms; t += 150) { try { if (await p.ev(code)) return true; } catch {} await sleep(150); }
    return false;
  };
  const text = (p, sel) => p.ev(`return document.querySelector(${JSON.stringify(sel)})?.textContent ?? null`);
  // The part (a section) titled `title`: its badge text.
  const badge = (p, title) => p.ev(`return [...document.querySelectorAll(".part")].find((s) => s.querySelector("h2")?.textContent == ${JSON.stringify(title)})?.querySelector(".badge")?.textContent ?? null`);
  const clickIn = (p, sel, label) => p.ev(`const b = [...document.querySelectorAll(${JSON.stringify(sel)})].find((x) => x.textContent.trim().startsWith(${JSON.stringify(label)})); if (!b) return false; b.click(); return true`);

  // ---- the clerk signs in and starts a session ----
  const clerk = await browser.page("about:blank", { own: true });
  await clerk.go(base + "/staff/login");
  await clerk.ev(`const f = document.querySelector("form.login"); f.username.value = "maria"; f.password.value = "password"; f.submit(); return true`);
  await sleep(800);
  await clerk.ev(`const f = document.querySelector("form.new-desk"); f.kind.value = "open_account"; f.number.value = ""; f.submit(); return true`);
  await sleep(1000);
  const code = await clerk.ev(`return location.pathname.split("/").pop()`);
  check("a 6-digit code", /^\d{6}$/.test(code), code);
  await clerk.booted();
  check("waiting for the phone", (await text(clerk, ".code-box .waiting"))?.includes("Waiting"), await text(clerk, ".code-box"));

  // ---- the client joins on their phone ----
  const phone = await browser.page("about:blank", { own: true, phone: true });
  await phone.go(base + "/desk");
  await phone.booted();
  await phone.ev(`const i = document.querySelector("form.join input"); i.focus(); return true`);
  await phone.type(code);
  await phone.ev(`document.querySelector("form.join button").click(); return true`);
  check("the phone shows the form", await until(phone, `return document.querySelectorAll(".desk-client .part").length == 5`));
  check("the clerk sees the phone joined, live", await until(clerk, `return document.querySelector(".code-box .joined") != null`));

  // ---- the clerk types; the phone follows ----
  const typeInto = async (name, value) => {
    await clerk.ev(`const i = document.querySelector(${JSON.stringify(`input[name="${name}"]`)}); i.focus(); i.select(); return true`);
    await clerk.type(value);
  };
  const setField = (name, value) => clerk.ev(`const i = document.querySelector(${JSON.stringify(`input[name="${name}"]`)}); i.value = ${JSON.stringify(value)}; i.dispatchEvent(new Event("input", { bubbles: true })); return true`);
  // Typed a key at a time, fast: what the clerk's box shows must be what was typed.
  await clerk.ev(`document.querySelector('input[name="first_name"]').focus(); return true`);
  for (const ch of "Grace") await clerk.type(ch);
  await typeInto("last_name", "Hopper");
  await sleep(1200);
  check("the clerk's boxes keep what was typed", (await clerk.ev(`return [document.querySelector('input[name="first_name"]').value, document.querySelector('input[name="last_name"]').value].join(" ")`)) === "Grace Hopper",
    await clerk.ev(`return [document.querySelector('input[name="first_name"]').value, document.querySelector('input[name="last_name"]').value].join(" ")`));
  const phoneValue = (label) => phone.ev(`return [...document.querySelectorAll(".desk-client .part dl > div")].find((d) => d.querySelector("dt").textContent == ${JSON.stringify(label)})?.querySelector("dd").textContent ?? null`);
  check("the phone shows the typing, live", await until(phone, `return [...document.querySelectorAll(".desk-client .part dl > div")].some((d) => d.textContent.includes("Hopper"))`), await phoneValue("Last name"));
  check("a field just changed lights up on the phone", await phone.ev(`return document.querySelector(".desk-client .part dl > div.lit") != null`));
  await setField("birth_date", "1990-12-09");
  await typeInto("email", "grace@example.com");
  await typeInto("phone", "+353 1 555 0199");
  await sleep(600);

  // ---- asking the client to confirm a part ----
  await clickIn(clerk, ".part button", "Ask the client");   // the first part: About you
  check("the phone is asked", await until(phone, `return document.querySelector(".ask") != null`));
  check("what it is asked shows the details", (await text(phone, ".ask"))?.includes("grace@example.com"), await text(phone, ".ask"));
  check("the clerk sees it waiting", await until(clerk, `return [...document.querySelectorAll(".part")].find((s) => s.querySelector("h2").textContent == "About you")?.querySelector(".badge").textContent.includes("Waiting")`));
  await clickIn(phone, ".ask button", "Yes");
  check("approved, on the clerk's screen", await until(clerk, `return [...document.querySelectorAll(".part")].find((s) => s.querySelector("h2").textContent == "About you")?.querySelector(".badge").textContent.includes("Approved")`));

  // Changing it after approval undoes the approval, on both screens.
  await typeInto("email", "grace.hopper@example.com");
  check("changed since approved, on the clerk's screen", await until(clerk, `return [...document.querySelectorAll(".part")].find((s) => s.querySelector("h2").textContent == "About you")?.querySelector(".badge").textContent.includes("Changed")`), await badge(clerk, "About you"));
  check("and on the phone", await until(phone, `return [...document.querySelectorAll(".part")].find((s) => s.querySelector("h2").textContent == "About you")?.querySelector(".badge").textContent.includes("Changed")`), await badge(phone, "About you"));

  // Asked again; the clerk changes it before the client answers: the
  // client's approval of what they saw is refused.
  await clickIn(clerk, ".part button", "Ask again");
  await until(phone, `return document.querySelector(".ask") != null`);
  await typeInto("phone", "+353 1 555 0100");
  await sleep(300);
  await clickIn(phone, ".ask button", "Yes");
  check("an answer to what changed since is refused", await until(phone, `return (document.querySelector(".ask .problem")?.textContent ?? "").includes("changed this since") || document.querySelector(".ask") == null`), await text(phone, ".ask"));

  // Declined with a note: the clerk reads it.
  await clickIn(clerk, ".part button", "Ask again");
  await until(phone, `return document.querySelector(".ask") != null && !document.querySelector(".ask .problem")`);
  await clickIn(phone, ".ask button", "Something");
  await phone.ev(`const t = document.querySelector(".ask textarea"); t.value = "My phone ends in 0199"; t.dispatchEvent(new Event("input", { bubbles: true })); return true`);
  await clickIn(phone, ".ask button", "Tell the clerk");
  check("the clerk reads why", await until(clerk, `return (document.querySelector(".part .note")?.textContent ?? "").includes("0199")`), await text(clerk, ".part .note"));
  await typeInto("phone", "+353 1 555 0199");
  await sleep(300);
  await clickIn(clerk, ".part button", "Ask again");
  await until(phone, `return document.querySelector(".ask") != null`);
  await clickIn(phone, ".ask button", "Yes");
  await until(clerk, `return [...document.querySelectorAll(".part")].find((s) => s.querySelector("h2").textContent == "About you")?.querySelector(".badge").textContent.includes("Approved")`);

  // ---- the other parts ----
  await typeInto("street", "1 Navy Way");
  await typeInto("postcode", "D02");
  await typeInto("city", "Dublin");
  await typeInto("country", "Ireland");
  await setField("account_name", "Everyday");
  await typeInto("deposit", "250,00");
  await sleep(600);
  for (const title of ["Your address", "Your new account", "First deposit, in cash", "Terms and conditions"]) {
    await clerk.ev(`const s = [...document.querySelectorAll(".part")].find((s) => s.querySelector("h2").textContent == ${JSON.stringify(title)}); s.querySelector("footer button").click(); return true`);
    const asked = await until(phone, `return document.querySelector(".ask") != null`);
    await clickIn(phone, ".ask button", title == "Terms and conditions" ? "I agree" : "Yes");
    const ok = await until(clerk, `return [...document.querySelectorAll(".part")].find((s) => s.querySelector("h2").textContent == ${JSON.stringify(title)})?.querySelector(".badge").textContent.includes("Approved")`);
    check(`approved: ${title}`, asked && ok, await badge(clerk, title));
  }

  // ---- the PIN, on the phone only ----
  const pin = async (a, b) => {
    await phone.ev(`const [x, y] = document.querySelectorAll("form.pin input"); x.value = ${JSON.stringify(a)}; x.dispatchEvent(new Event("input", { bubbles: true })); y.value = ${JSON.stringify(b)}; y.dispatchEvent(new Event("input", { bubbles: true })); return true`);
    await phone.ev(`document.querySelector("form.pin button").click(); return true`);
    await sleep(500);
    return text(phone, "form.pin .problem");
  };
  check("two different PINs are refused", (await pin("135790", "135791"))?.includes("not the same"));
  check("an easy PIN is refused", (await pin("111111", "111111"))?.includes("too easy"));
  await pin("135790", "135790");
  check("the clerk sees a PIN was chosen, not the PIN", await until(clerk, `return (document.body.textContent.includes("Chosen on the phone")) && !document.body.textContent.includes("135790")`));
  check("nothing is missing", await until(clerk, `return document.querySelector(".submit .ready") != null`), await text(clerk, ".submit"));

  // ---- submit: customers, accounts, the deposit, the messages ----
  await clickIn(clerk, ".submit button", "Open the account");
  check("done, on the clerk's screen", await until(clerk, `return document.querySelector(".finished") != null`, 10000), await text(clerk, ".desk-clerk"));
  check("done, on the phone, with the customer number", await until(phone, `return /customer number is \\d{8}/.test(document.querySelector(".desk-client")?.textContent ?? "")`), await text(phone, ".desk-client"));
  const number = await phone.ev(`return document.querySelector(".desk-client .big")?.textContent`);

  // ---- the new customer signs in on the phone ----
  await phone.go(base + "/login");
  await phone.ev(`const f = document.querySelector("form.stack"); f.number.value = ${JSON.stringify(number)}; f.pin.value = "135790"; f.submit(); return true`);
  await sleep(1200);
  await phone.booted();
  const home = await text(phone, "main.app");
  check("signed in, with the account and its first deposit", home?.includes("Grace") && home?.includes("€250.00"), home?.slice(0, 300));
  await phone.go(base + "/messages");
  check("a welcome message", (await text(phone, ".messages"))?.includes("Welcome"), await text(phone, "main.app"));

  return { code, number, checks, phoneProblems: phone.problems, phoneMismatches: phone.mismatches, clerkProblems: clerk.problems, clerkMismatches: clerk.mismatches };
};
