// Drives tests/browser/cover-values.qwe: before the components are reached,
// a person picks in one select and a script adds an element inside another
// component; then each is woken by a click and what it heard is read back.
(async () => {
  const out = {};
  const tick = (ms = 60) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  const until = async (f, ms = 8000) => {
    const end = Date.now() + ms;
    while (Date.now() < end) {
      const v = f();
      if (v) return v;
      await tick(20);
    }
    return null;
  };
  const warned = [];
  const warn = console.warn;
  console.warn = (...a) => { warned.push(a.join(" ")); return warn.apply(console, a); };
  try {
    await until(() => window.__quillo_booted);
    await tick(200);
    // Picked by a person before the code came: the component hears it once taken up.
    $("#typed").selectedIndex = 1;
    $("#wake").click();
    out.changes = await until(() => $("#changes").textContent.includes("wake") && $("#changes").textContent);
    out.values = ["#plain", "#chosen", "#typed"].map((s) => $(s).value);
    // An element a script put inside a component: it is made again, without it.
    const i = document.createElement("i");
    i.textContent = "added";
    $("#extra").appendChild(i);
    // The click that woke it went to the element the script changed; the
    // button made again works.
    $("#bump").click();
    await until(() => !$("#extra i"));
    out.woken = $("#bump").textContent;
    $("#bump").click();
    out.bumped = await until(() => $("#bump").textContent === "1" && "1");
    out.extra = $("#extra").innerHTML;
    // Rows that show nothing, as rows are added, dropped and all taken away.
    const list = () => [...$("#list").querySelectorAll("li")].map((x) => x.textContent).join(",");
    out.rows = [list()];
    $("#add").click();
    await until(() => list() === "2,4");
    out.rows.push(list());
    $("#drop").click();
    await until(() => $("#list").querySelectorAll("li").length === 2 && list() === "2,4");
    out.rows.push(list());
    $("#add").click();
    await until(() => list() === "2,4,4");
    out.rows.push(list());
    $("#clear").click();
    await until(() => list() === "");
    out.rows.push(list());
    $("#add").click();
    await until(() => list() === "1");
    out.rows.push(list());
    out.warned = warned.filter((w) => w.includes("made again"));
    out.errors = window.__quilloErrors || [];
  } catch (e) {
    out.exception = String((e && e.stack) || e);
  }
  console.warn = warn;
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
