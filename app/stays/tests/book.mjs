// A guest's way through, in a browser, with a second guest watching the same
// place in another: search and its map, the price worked out in the browser
// against the one charged, nights held and booked showing live on the other
// guest's calendar, paying (a declined card first), cancelling with the
// refund asked first, and an activity's places going down live.
//
//   BASE=http://localhost:8080 node feedback/tools/drive.mjs app/stays/tests/book.mjs
//
// Needs STAYS_OFFLINE=1 STAYS_DEMO=1, freshly seeded.

const day = (n) => { const d = new Date(); d.setUTCDate(d.getUTCDate() + n); return d.toISOString().slice(0, 10); };
// A day as the kit's DateRangePicker numbers its cells: days since 1970.
const cell = (iso) => Date.parse(iso + "T00:00:00Z") / 86400000;

export default async ({ browser, page, sleep }) => {
  const base = process.env.BASE;
  const checks = {};
  const check = (name, ok, got) => { checks[name] = ok ? true : (got ?? false); };
  const until = async (p, code, ms = 6000) => {
    for (let t = 0; t < ms; t += 150) { try { if (await p.ev(code)) return true; } catch {} await sleep(150); }
    return false;
  };
  const text = (p, sel) => p.ev(`return document.querySelector(${JSON.stringify(sel)})?.textContent ?? null`);
  const signIn = async (p, email) => {
    await p.go(base + "/login");
    await p.ev(`const f = document.querySelector("form.stack"); f.email.value = ${JSON.stringify(email)}; f.password.value = "password"; f.submit(); return true`);
    await sleep(800);
  };

  // ---- searching, signed out ----
  await page.go(base + "/s?where=Lisbon");
  check("search: five places in Lisbon", (await text(page, "#count"))?.startsWith("5 places in Lisbon"), await text(page, "#count"));
  check("search: the map draws its pins without a touch", await until(page, `return document.querySelectorAll(".ui-map-view-label").length == 5`), await page.ev(`return document.querySelectorAll(".ui-map-view-label").length`));
  await page.ev(`[...document.querySelectorAll(".ui-chips label")].find((l) => l.textContent.trim() == "Room").click(); return true`);
  check("search: the Room chip leaves one place, and one pin", await until(page, `return document.querySelectorAll(".result").length == 1 && document.querySelectorAll(".ui-map-view-label").length == 1`), await text(page, "#count"));
  await page.ev(`[...document.querySelectorAll(".ui-chips label")].find((l) => l.textContent.trim() == "Room").click(); return true`);
  await page.ev(`[...document.querySelectorAll(".ui-segmented label")].find((l) => l.textContent.trim() == "Cheapest").click(); return true`);
  const prices = await until(page, `const p = [...document.querySelectorAll(".result .price b")].map((b) => parseFloat(b.textContent.replace(/[^0-9.]/g, ""))); return p.length == 5 && p.every((x, i) => i == 0 || p[i - 1] <= x)`);
  check("search: Cheapest sorts by the night's price", prices, await page.ev(`return [...document.querySelectorAll(".result .price b")].map((b) => b.textContent)`));
  await page.ev(`document.querySelectorAll(".ui-map-view-label")[0].dispatchEvent(new MouseEvent("click", { bubbles: true })); return true`);
  check("search: a pin picked marks its card", await until(page, `return document.querySelectorAll(".result.picked").length == 1`));
  await page.go(base + `/s?where=Lisbon&start=${day(14)}&end=${day(17)}&guests=2`);
  check("search: with Sam's dates, his Alfama flat is not offered", (await text(page, "#count"))?.startsWith("4 places"), await text(page, "#count"));
  check("search: and the totals for the dates show", await page.ev(`return document.querySelectorAll(".result .price .muted").length == 4`));
  await page.go(base + "/s?where=Kyoto");
  // ja-JP writes yen with the full-width ￥, as Intl.NumberFormat does.
  check("search: Kyoto in yen", /^[¥￥]\d/.test(await text(page, ".result .price b") ?? ""), await text(page, ".result .price b"));
  await page.go(base + "/s?where=Atlantis");
  check("search: a place we do not know says so", (await text(page, ".ui-alert"))?.includes("Atlantis"), await text(page, ".search-list"));

  // ---- two guests on the same place ----
  const sam = await browser.page("about:blank", { own: true });
  await signIn(sam, "sam@stays.test");
  const lee = await browser.page("about:blank", { own: true });
  await signIn(lee, "lee@stays.test");
  const a = day(8), b = day(11);
  await lee.go(base + "/stays/3");
  await lee.booted();
  await sam.go(base + `/stays/3?start=${a}&end=${b}&guests=2`);
  await sam.booted();
  const widgetTotal = await text(sam, ".quote .ui-price-breakdown-total dd");
  check("the widget prices three nights in the browser", /€\d/.test(widgetTotal ?? ""), await text(sam, "#widget"));
  check("the weather for the dates comes after the page", await until(sam, `return document.querySelectorAll("#weather .day").length == 4`), await text(sam, "#weather"));
  check("places around come after the page, with pins", await until(sam, `return document.querySelectorAll("#around li").length > 0 && document.querySelectorAll("#around .ui-map-view-dot").length > 0`), await text(sam, "#around"));
  await sam.ev(`document.querySelector("#reserve").click(); return true`);
  check("Reserve holds the nights and goes to checkout", await until(sam, `return location.pathname.startsWith("/checkout/")`), await sam.ev(`return location.href`));
  await sam.booted();
  check("the total charged is the widget's, to the cent", await until(sam, `return document.querySelector(".checkout-total .ui-price-breakdown-total dd")?.textContent == ${JSON.stringify(widgetTotal)}`), [await text(sam, ".checkout-total .ui-price-breakdown-total dd"), widgetTotal]);
  check("a countdown shows the 10 minutes", /(10:00|9:5\d)/.test(await text(sam, ".ui-cd") ?? ""), await text(sam, ".ui-cd"));
  await lee.ev(`document.querySelector("#widget .ui-date-range-picker-trigger").click(); return true`);
  check("Lee's dates picker closes Sam's held nights, live", await until(lee, `return document.querySelector('#widget [data-day="${cell(a)}"]')?.getAttribute("aria-disabled") == "true"`), await lee.ev(`return document.querySelector('#widget [data-day="${cell(a)}"]')?.outerHTML.slice(0, 200) ?? "no cell"`));
  check("with each night's price under it", /€\d/.test(await lee.ev(`return document.querySelector('#widget [data-day="${cell(day(12))}"]')?.textContent ?? ""`) ?? ""), await lee.ev(`return document.querySelector('#widget [data-day="${cell(day(12))}"]')?.textContent`));
  await lee.go(base + `/stays/3?start=${day(7)}&end=${day(10)}`);
  await lee.booted();
  check("Lee given dates across them is told they are not free", await until(lee, `return (document.querySelector("#clash")?.textContent ?? "").startsWith("Not free")`), await text(lee, "#widget"));
  check("and is offered no price for them", await lee.ev(`return document.querySelector(".quote") == null`));

  const pay = async (number) => {
    await sam.ev(`const f = document.querySelector("#pay"); const set = (a, v) => { const i = f.querySelector("input[autocomplete=" + a + "]"); i.value = v; i.dispatchEvent(new Event("input", { bubbles: true })); }; set("cc-number", ${JSON.stringify(number)}); set("cc-exp", "12/30"); set("cc-csc", "123"); return true`);
    await sam.ev(`document.querySelector("#pay button[type=submit]").click(); return true`);
  };
  await pay("4000 0000 0000 0002");
  check("a declined card is said so", await until(sam, `return (document.querySelector("#pay .ui-alert")?.textContent ?? "").includes("declined")`), await text(sam, "#pay"));
  await pay("4242 4242 4242 4242");
  check("a good card books it", await until(sam, `return document.querySelector(".booked") != null`), await text(sam, "main"));
  check("Steps moves to Booked", await sam.ev(`return document.querySelectorAll(".ui-steps li")[2]?.getAttribute("aria-current") == "step" || document.querySelectorAll(".ui-steps .ui-steps-done, .ui-steps [data-phase=done]").length >= 1`), await sam.ev(`return document.querySelector(".ui-steps")?.outerHTML.slice(0, 400)`));

  // ---- Kyoto, in yen: the browser's price and the server's ----
  await sam.go(base + `/stays/15?start=${day(20)}&end=${day(23)}&guests=2`);
  await sam.booted();
  const yen = await text(sam, ".quote .ui-price-breakdown-total dd");
  await sam.ev(`document.querySelector("#reserve").click(); return true`);
  await until(sam, `return location.pathname.startsWith("/checkout/")`);
  await sam.booted();
  check("yen: the total charged is the widget's", await until(sam, `return document.querySelector(".checkout-total .ui-price-breakdown-total dd")?.textContent == ${JSON.stringify(yen)}`), [await text(sam, ".checkout-total .ui-price-breakdown-total dd"), yen]);
  check("yen: no cents", !/[.,]\d\d$/.test(yen ?? ""), yen);
  await sam.ev(`[...document.querySelectorAll("button")].find((x) => x.textContent == "Let the nights go").click(); return true`);
  check("letting the nights go goes back to the place", await until(sam, `return location.pathname == "/stays/15"`), await sam.ev(`return location.href`));

  // ---- cancelling ----
  await sam.go(base + "/trips");
  await sam.booted();
  const when = await sam.ev(`return [...document.querySelectorAll(".trip")].find((t) => t.textContent.includes("Bairro Alto") && t.querySelector(".cancel"))?.dataset.trip`);
  const row = `document.querySelector('.trip[data-trip="${when}"]')`;
  check("the trip is coming up", await sam.ev(`return ${row} != null`), await text(sam, "main"));
  await sam.ev(`${row}.querySelector(".cancel").click(); return true`);
  check("cancelling asks first, with the refund", await until(sam, `return [...document.querySelectorAll("dialog, [class^=q-], [class*=' q-']")].some((d) => /You get back €/.test(d.textContent))`), await sam.ev(`return document.body.innerText.slice(-400)`));
  await sam.ev(`[...document.querySelectorAll("button")].find((x) => x.textContent == "Cancel the stay").click(); return true`);
  check("cancelled: it moves to Cancelled", await until(sam, `return ${row}?.textContent.includes("Cancelled")`), await text(sam, "main"));
  await lee.go(base + "/stays/3");
  await lee.booted();
  await lee.ev(`document.querySelector("#widget .ui-date-range-picker-trigger").click(); return true`);
  check("and the nights are free in the place's dates picker", await until(lee, `const c = document.querySelector('#widget [data-day="${cell(a)}"]'); return c != null && c.getAttribute("aria-disabled") != "true"`));

  // ---- an activity's places, live ----
  // The seed fills the pastel de nata class's times tomorrow.
  await lee.go(base + `/stays/1?start=${day(1)}&end=${day(3)}`);
  await lee.booted();
  await sam.go(base + `/stays/1?start=${day(1)}&end=${day(3)}`);
  await sam.booted();
  const slot = await sam.ev(`return [...document.querySelectorAll("#activities [data-slot]")].find((l) => !l.querySelector("input").disabled)?.dataset.slot`);
  const leftText = (p) => p.ev(`return document.querySelector('#activities [data-slot="${slot}"] .ui-time-slots-meta')?.textContent ?? ""`);
  const before = parseInt(await leftText(lee));
  await sam.ev(`document.querySelector('#activities [data-slot="${slot}"] input').click(); return true`);
  await until(sam, `return document.querySelector("#book-activity") != null`);
  await sam.ev(`document.querySelector("#book-activity").click(); return true`);
  check("an activity booked says so", await until(sam, `return document.querySelector("#activity-done") != null`), await text(sam, "#activities"));
  check("Lee sees two places fewer, live", await until(lee, `return parseInt(document.querySelector('#activities [data-slot="${slot}"] .ui-time-slots-meta')?.textContent) == ${before - 2}`), [before, await leftText(lee)]);
  check("a full time cannot be picked", await sam.ev(`return [...document.querySelectorAll("#activities [data-slot]")].some((s) => s.querySelector("input").disabled && s.textContent.includes("Full"))`));

  const problems = { page: page.problems, sam: sam.problems, lee: lee.problems };
  return { checks, passed: Object.values(checks).filter((v) => v === true).length, of: Object.keys(checks).length, problems, quillo: { sam: await sam.quillo(), lee: await lee.quillo() } };
};
