// Loads Quillo Arena (app/arena) and measures its game loop and what players get.
//
//   node feedback/tools/arena-sim.mjs <base url> [players] [bots,bots,...] [seconds]
//   node feedback/tools/arena-sim.mjs http://127.0.0.1:8087 10 0,20,50,100,200,400 6
//
// Opens `players` real players (headless Chrome tabs, each signed in, in the north
// room, moving its pointer), then for each count of bots in the room: fills the room to
// that many, starts the arena's measures again, waits `seconds`, and reports the loop's
// ticks a second, its step time, the gaps between ticks, the snapshot's size and how
// often it is past a datagram, and the updates each player got a second. Needs the
// arena's /game/bots and /game/stats routes (from this machine) and Chrome
// (`QUILLO_CHROME`, as tools/chrome.mjs).
import { launch } from "./chrome.mjs";

const [base, playersArg = "10", botsArg = "0,20,50,100,200", secsArg = "6"] = process.argv.slice(2);
if (!base) { console.error("usage: arena-sim.mjs <base url> [players] [bots,...] [seconds]"); process.exit(2); }
const players = Number(playersArg), stages = botsArg.split(",").map(Number), secs = Number(secsArg);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const get = async (path, method = "GET") => (await fetch(base + path, { method })).json().catch(() => ({}));

const browser = await launch();
const tabs = [];
for (let i = 0; i < players; i++) {
    const page = await browser.page();
    await page.go(base + "/");
    await page.ev(`return fetch("/name", { method: "POST", headers: { "content-type": "application/x-www-form-urlencoded" }, body: "name=player${i}" }).then(() => 1)`);
    await page.go(`${base}/room/north`);
    tabs.push(page);
}
await sleep(2000);

// Each player moves its pointer around a circle of its own, ten times a second.
let moving = true;
const move = (async () => {
    const t0 = Date.now();
    while (moving) {
        const a = (Date.now() - t0) / 700;
        await Promise.all(tabs.map((p, i) => p.ev(`return (() => { const cv = document.querySelector("canvas"); if (!cv) return 0; const r = cv.getBoundingClientRect();
            cv.dispatchEvent(new PointerEvent("pointermove", { bubbles: true, clientX: r.left + r.width * (0.5 + 0.35 * Math.cos(${a} + ${i})), clientY: r.top + r.height * (0.5 + 0.35 * Math.sin(${a} + ${i})) })); return 1; })()`).catch(() => 0)));
        await sleep(100);
    }
})();

const updates = async () => Promise.all(tabs.map((p) => p.ev(`return Number((document.querySelector(".side div")?.textContent.match(/(\\d+) updates/) ?? [0, 0])[1])`).catch(() => 0)));
const pct = (xs, q) => { const s = [...xs].sort((a, b) => a - b); return s.length ? s[Math.min(s.length - 1, Math.floor(s.length * q))] : 0; };

const rows = [];
let bots = 0;
for (const target of stages) {
    if (target > bots) { await get(`/game/bots/north/${target - bots}`, "POST"); bots = target; }
    await sleep(1000);
    await get("/game/stats/reset", "POST");
    const u0 = await updates();
    await sleep(secs * 1000);
    const u1 = await updates();
    const s = await get("/game/stats");
    const gaps = (s.gaps ?? []).slice(2);
    const per = u1.map((v, i) => (v - u0[i]) / secs);
    rows.push({
        bots: target, players, "ticks/s": (s.ticks / secs).toFixed(1), "step ms": s.step_ms, "worst ms": s.worst_ms,
        "gap p50": pct(gaps, 0.5), "gap p99": pct(gaps, 0.99), "gap max": Math.max(0, ...gaps), late: s.late,
        "snapshot B": s.bytes, "on a stream": s.ticks ? `${Math.round(100 * s.reliable / s.ticks)}%` : "-",
        "updates/s min": Math.min(...per).toFixed(1), "updates/s avg": (per.reduce((a, b) => a + b, 0) / per.length).toFixed(1),
    });
    console.error(`stage ${target} bots done`);
}
moving = false;
await move;
await browser.close();
console.log("| " + Object.keys(rows[0]).join(" | ") + " |\n|" + Object.keys(rows[0]).map(() => "---").join("|") + "|");
for (const r of rows) console.log("| " + Object.values(r).join(" | ") + " |");
