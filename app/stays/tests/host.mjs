// The host's side, in a browser, with a guest in another: a request to book
// shown on the host's dashboard as it is made and answered there (the guest's
// checkout following), one declined, one that ran out before it was answered,
// the calendar (nights picked with shift-click, closed and priced, and the
// guest's page following), and a new place placed on the map.
//
//   BASE=http://localhost:8080 STAYS_DB=<path of stays.db> node feedback/tools/drive.mjs app/stays/tests/host.mjs
//
// Needs STAYS_OFFLINE=1 STAYS_DEMO=1, freshly seeded.

import { execFileSync } from "node:child_process";

const day = (n) => { const d = new Date(); d.setUTCDate(d.getUTCDate() + n); return d.toISOString().slice(0, 10); };
const sql = (q) => execFileSync("sqlite3", [process.env.STAYS_DB, q]).toString().trim();

export default async ({ browser, sleep, SHIFT }) => {
  const base = process.env.BASE;
  const checks = {};
  const check = (name, ok, got) => { checks[name] = ok ? true : (got ?? false); };
  const until = async (p, code, ms = 6000) => {
    for (let t = 0; t < ms; t += 150) { try { if (await p.ev(code)) return true; } catch {} await sleep(150); }
    return false;
  };
  const text = (p, sel) => p.ev(`return document.querySelector(${JSON.stringify(sel)})?.textContent ?? null`);
  const signIn = async (email) => {
    const p = await browser.page("about:blank", { own: true });
    await p.go(base + "/login");
    await p.ev(`const f = document.querySelector("form.stack"); f.email.value = ${JSON.stringify(email)}; f.password.value = "password"; f.submit(); return true`);
    await sleep(800);
    return p;
  };
  const click = (p, sel) => p.ev(`document.querySelector(${JSON.stringify(sel)}).click(); return true`);

  const jordi = await signIn("jordi@stays.test");
  const lee = await signIn("lee@stays.test");
  const ana = await signIn("ana@stays.test");
  const sam = await signIn("sam@stays.test");

  // ---- a request, answered live ----
  await jordi.go(base + "/host");
  await jordi.booted();
  check("Jordi has no requests yet", (await text(jordi, "main"))?.includes("None waiting"), await text(jordi, ".stats"));
  await lee.go(base + `/stays/9?start=${day(12)}&end=${day(15)}&guests=2`);
  await lee.booted();
  check("Gràcia is request to book", (await text(lee, "#reserve"))?.includes("Request to book"), await text(lee, "#reserve"));
  await click(lee, "#reserve");
  await until(lee, `return location.pathname.startsWith("/checkout/")`);
  await lee.booted();
  await lee.ev(`const f = document.querySelector("#pay"); for (const [a, v] of [["cc-number", "4242424242424242"], ["cc-exp", "12/30"], ["cc-csc", "123"]]) { const i = f.querySelector("input[autocomplete=" + a + "]"); i.value = v; i.dispatchEvent(new Event("input", { bubbles: true })); } f.querySelector("button[type=submit]").click(); return true`);
  check("Lee's request is sent", await until(lee, `return document.querySelector(".requested") != null`), await text(lee, "main"));
  check("it shows on Jordi's dashboard, live", await until(jordi, `return document.querySelectorAll("[data-request]").length == 1`), await text(jordi, "main"));
  check("with a day to answer it", /23:5\d|1d|24:00/.test(await text(jordi, "[data-request] .ui-cd") ?? ""), await text(jordi, "[data-request] .ui-cd"));
  await click(jordi, "[data-request] .accept");
  check("accepted: gone from the requests", await until(jordi, `return document.querySelectorAll("[data-request]").length == 0`));
  check("and on Lee's checkout, live: booked", await until(lee, `return document.querySelector(".booked") != null`), await text(lee, "main"));
  check("Jordi's coming up shows Lee", await until(jordi, `return (document.querySelector(".ui-timeline")?.textContent ?? "").includes("Lee Park")`), await text(jordi, "main"));

  // ---- declined, and run out ----
  await ana.go(base + "/host");
  await ana.booted();
  check("Ana has Lee's seeded request", await ana.ev(`return document.querySelectorAll("[data-request]").length == 1`), await text(ana, "main"));
  await lee.go(base + "/trips");
  await lee.booted();
  await click(ana, "[data-request] .decline");
  check("declined: Lee's trips say so, live", await until(lee, `return [...document.querySelectorAll(".trip")].some((t) => t.textContent.includes("Tiled house") && t.textContent.includes("Declined"))`), await text(lee, "main"));
  // Another request, whose day to answer has passed (the sweep runs each minute).
  const r = sql(`insert into bookings (listing_id, guest_id, start_day, end_day, guests, status, respond_by, currency, total, breakdown, created) values (2, 5, '${day(60)}', '${day(63)}', 2, 'requested', ${Date.now() - 60000}, 'EUR', 50000, '{"lines":[],"nights":[]}', ${Date.now() - 86400000}) returning id`);
  await ana.go(base + "/host");
  await ana.booted();
  if (await ana.ev(`return document.querySelector("[data-request] .accept") != null`)) {
    await click(ana, "[data-request] .accept");
    check("accepting a request that ran out is refused", await until(ana, `return (document.querySelector(".ui-alert")?.textContent ?? "").includes("ran out")`), await text(ana, "main"));
  } else {
    check("accepting a request that ran out is refused", true);
  }
  check("and it is not booked", sql(`select status from bookings where id = ${r}`) != "confirmed", sql(`select status from bookings where id = ${r}`));

  // ---- the calendar ----
  const month = day(70).slice(0, 7);
  await ana.go(base + `/host/listings/1?month=${month}`);
  await ana.booted();
  const d1 = `${month}-10`, d2 = `${month}-12`;
  await click(ana, `[data-day="${d1}"]`);
  const c = await ana.center(`[data-day="${d2}"]`);
  await ana.mouse("mousePressed", c.x, c.y, { button: "left", clickCount: 1, modifiers: SHIFT });
  await ana.mouse("mouseReleased", c.x, c.y, { button: "left", clickCount: 1, modifiers: SHIFT });
  check("shift-click picks the nights between", await until(ana, `return document.querySelector("#picked-count").textContent == "3 nights picked"`), await text(ana, "#picked-count"));
  await sam.go(base + `/stays/1?start=${d1}&end=${month}-14`);
  await sam.booted();
  const before = await text(sam, ".quote .ui-price-breakdown-total dd");
  await click(ana, "#close-nights");
  check("closed: said so", await until(ana, `return (document.querySelector("#said")?.textContent ?? "").startsWith("3 nights changed")`), await text(ana, "#said"));
  check("Sam's page, live: not free", await until(sam, `return (document.querySelector("#clash")?.textContent ?? "").startsWith("Not free")`), await text(sam, "#widget"));
  await click(ana, `[data-day="${d1}"]`);
  await click(ana, "#open-nights");
  await until(ana, `return (document.querySelector("#said")?.textContent ?? "").startsWith("1 night")`);
  // Now 11 and 12 are closed: price the 13th, then open the 11th and 12th.
  await click(ana, `[data-day="${month}-13"]`);
  await ana.ev(`const i = document.querySelector('input[name="price"]'); i.value = "abc"; i.dispatchEvent(new Event("input", { bubbles: true })); return true`);
  await click(ana, "#price-nights");
  check("a price that is not one is refused", await until(ana, `return (document.querySelector(".ui-alert")?.textContent ?? "").includes("A price in EUR")`), await text(ana, "main"));
  await ana.ev(`const i = document.querySelector('input[name="price"]'); i.value = "200"; i.dispatchEvent(new Event("input", { bubbles: true })); return true`);
  await click(ana, "#price-nights");
  await until(ana, `return document.querySelector("#picked-count").textContent == "0 nights picked"`);
  await click(ana, `[data-day="${month}-11"]`);
  await click(ana, `[data-day="${d2}"]`);
  await click(ana, "#open-nights");
  check("the nights open again, one at €200", await until(ana, `return document.querySelector('[data-day="${month}-13"]').textContent.includes("€200.00") && !document.querySelector('[data-day="${month}-11"]').className.includes("closed")`), await ana.ev(`return ["11", "12", "13"].map((d) => document.querySelector('[data-day="${month}-' + d + '"]')).map((b) => b.className + " " + b.textContent)`));
  check("Sam's price follows, live", await until(sam, `return document.querySelector(".quote .ui-price-breakdown-total dd") != null && document.querySelector(".quote .ui-price-breakdown-total dd").textContent != ${JSON.stringify(before)}`), [before, await text(sam, ".quote .ui-price-breakdown-total dd"), await text(sam, "#widget")]);
  await click(ana, '.ui-month-calendar [aria-label="Next month"]');
  check("the next month", await until(ana, `return document.querySelector('[data-day="${d1}"]') == null`), await text(ana, ".ui-month-calendar"));

  // ---- a new place ----
  await sam.go(base + "/host/new");
  await sam.booted();
  check("the new place's map draws", await until(sam, `return document.querySelectorAll(".ui-map-view .leaflet-tile").length > 0`));
  const m = await sam.center(".ui-map-view");
  await sam.click(m.x + 40, m.y - 30);
  check("a click on the map places the pin", await until(sam, `return (document.querySelector("#pin")?.textContent ?? "").startsWith("Pin at")`), await text(sam, "#pin"));
  await sam.ev(`const f = document.querySelector("#new-listing"); f.title.value = "Sam's attic in Chiado"; f.base.value = "88.50"; f.weekend.value = "99"; f.cleaning.value = "20"; f.description.value = "Under the roof, over the city."; f.querySelector("button[type=submit]").click(); return true`);
  check("listed: on to its calendar", await until(sam, `return /^\\/host\\/listings\\/\\d+$/.test(location.pathname)`), [await sam.ev(`return location.href`), await text(sam, "#new-listing .ui-alert")]);
  const id = await sam.ev(`return location.pathname.split("/").pop()`);
  check("its price and place are kept", sql(`select base_price || ' ' || weekend_price || ' ' || cleaning_fee || ' ' || (abs(lat - 38.71) < 0.2) from listings where id = ${id}`) == "8850 9900 2000 1", sql(`select * from listings where id = ${id}`));
  await sam.go(base + "/s?where=Lisbon");
  check("and it is found in Lisbon", (await text(sam, "#count"))?.startsWith("6 places"), await text(sam, "#count"));
  await sam.go(base + "/host/new");
  await sam.booted();
  await sam.ev(`const f = document.querySelector("#new-listing"); f.title.value = "Kyoto with cents"; f.city.value = "Kyoto"; f.city.dispatchEvent(new Event("change", { bubbles: true })); f.base.value = "18000.50"; f.querySelector("button[type=submit]").click(); return true`);
  check("yen with cents are refused", await until(sam, `return (document.querySelector("#new-listing .ui-alert")?.textContent ?? "").includes("whole yen")`), await text(sam, "#new-listing .ui-alert"));

  // ---- earnings, in each currency ----
  const kenji = await signIn("kenji@stays.test");
  await kenji.go(base + "/host");
  await kenji.booted();
  check("Kenji's earnings are in yen", (await text(kenji, ".ui-bar-chart, [class*=bar-chart]"))?.includes("JPY") || (await text(kenji, "main"))?.includes("Earned in JPY"), await text(kenji, "main"));
  check("and his stats in yen", (await text(kenji, ".stats"))?.includes("￥") || (await text(kenji, ".stats"))?.includes("¥"), await text(kenji, ".stats"));

  const problems = Object.fromEntries([["jordi", jordi], ["lee", lee], ["ana", ana], ["sam", sam], ["kenji", kenji]].map(([n, p]) => [n, p.problems]));
  return { checks, passed: Object.values(checks).filter((v) => v === true).length, of: Object.keys(checks).length, problems };
};
