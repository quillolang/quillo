// A cash withdrawal at the desk for a customer who is signed in on their
// phone: the session is offered on their home screen, live; they approve
// the amount; the money leaves their account while they look.
//
//   BASE=http://localhost:8080 node feedback/tools/drive.mjs app/bank/tests/withdrawal.mjs

export default async ({ browser, sleep }) => {
  const base = process.env.BASE;
  const checks = {};
  const check = (name, ok, got) => { checks[name] = ok ? true : (got ?? false); };
  const until = async (p, code, ms = 6000) => {
    for (let t = 0; t < ms; t += 150) { try { if (await p.ev(code)) return true; } catch {} await sleep(150); }
    return false;
  };
  const text = (p, sel) => p.ev(`return document.querySelector(${JSON.stringify(sel)})?.textContent ?? null`);

  const ben = await browser.page("about:blank", { own: true, phone: true });
  await ben.go(base + "/login");
  await ben.ev(`const f = document.querySelector("form.stack"); f.number.value = "10000002"; f.pin.value = "246810"; f.submit(); return true`);
  await sleep(900);
  await ben.booted();
  const before = await text(ben, ".card .balance");

  const maria = await browser.page("about:blank", { own: true });
  await maria.go(base + "/staff/login");
  await maria.ev(`const f = document.querySelector("form.login"); f.username.value = "maria"; f.password.value = "password"; f.submit(); return true`);
  await sleep(900);
  await maria.ev(`const f = document.querySelector("form.new-desk"); f.kind.value = "withdrawal"; f.number.value = "10000002"; f.submit(); return true`);
  await sleep(1000);
  await maria.booted();
  check("the clerk's page says whose session it is", (await text(maria, ".desk-head"))?.includes("Ben Okafor"), await text(maria, ".desk-head"));

  check("Ben's home offers the session, live", await until(ben, `return (document.querySelector(".invite")?.textContent ?? "").includes("Maria Keane started a session")`), await text(ben, "main.app"));
  await ben.ev(`document.querySelector(".invite button").click(); return true`);
  check("one tap joins it", await until(ben, `return location.pathname == "/desk" && document.querySelector(".desk-client") != null`), await ben.ev(`return location.href`));
  check("the clerk sees the phone joined", await until(maria, `return document.querySelector(".code-box .joined") != null`));

  await maria.ev(`const s = document.querySelector(".part select"); s.value = s.options[1].value; s.dispatchEvent(new Event("change", { bubbles: true })); return true`);
  await maria.ev(`const i = document.querySelector('input[name="amount"]'); i.value = "100"; i.dispatchEvent(new Event("input", { bubbles: true })); return true`);
  await sleep(600);
  await maria.ev(`document.querySelector(".part footer button").click(); return true`);
  check("Ben is asked to approve the amount", await until(ben, `return (document.querySelector(".ask")?.textContent ?? "").includes("100")`), await text(ben, ".desk-client"));
  await ben.ev(`[...document.querySelectorAll(".ask button")].find((b) => b.textContent.startsWith("Yes")).click(); return true`);
  check("approved, on the clerk's screen", await until(maria, `return document.querySelector(".submit .ready") != null`), await text(maria, ".submit"));
  await maria.ev(`[...document.querySelectorAll(".submit button")].find((b) => b.textContent == "Pay out").click(); return true`);
  check("paid out", await until(maria, `return document.querySelector(".finished") != null`), await text(maria, ".desk-clerk"));
  check("Ben's phone says so", await until(ben, `return (document.querySelector(".desk-client")?.textContent ?? "").includes("paid out")`), await text(ben, ".desk-client"));
  await ben.go(base + "/");
  await ben.booted();
  const after = await text(ben, ".card .balance");
  check("Ben's balance is €100 less", before === "€848.00" && after === "€748.00", `${before} -> ${after}`);
  check("and the statement says cash at the desk", (await ben.ev(`return fetch("/accounts/" + document.querySelector(".card").getAttribute("href").split("/").pop()).then((r) => r.text())`))?.includes("Cash at the desk"));
  return { checks, problems: [ben.problems, maria.problems], mismatches: [ben.mismatches, maria.mismatches] };
};
