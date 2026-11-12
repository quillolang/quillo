// Quillo browser runtime: values, the standard library, a small virtual DOM,
// components with state, and calls to `server fn`s. The compiled program is
// appended after this and registers its functions in G.
"use strict";

// Where the program is mounted ("" at the root); the bundle sets it.
let BASE = "";
// Development (`--watch`): a page the browser makes again, rather than
// taking up the server's HTML, says why. Lines ending `// dev` say it, and
// are left out of production bundles (see `runtime_units` in client.rs).
let DEV = false;
// Whether the server shows a builtin's errors (`--watch`), which it hides from visitors else: the
// browser does as it does (see `repr`); the bundle sets it (FEEDBACK55.md, Q-773; branch
// feedback55).
let ERRORS_SHOWN = false;
const withBase = (u) => (BASE && typeof u === "string" && u[0] === "/" && u[1] !== "/" ? BASE + u : u);
// In an app, the one way out the app's shell gives (FEEDBACK39.md, Q-465 and Q-470; branch
// feedback36-39): the page comes from the app itself, through a scheme, and what a scheme cannot
// carry (a body on Android, a file on WebKit, a socket anywhere) goes through the app's bridge.
// In a browser, `fetch` and `WebSocket`.
let WIRE = null;
if (typeof document !== "undefined" && typeof CustomEvent === "function") document.dispatchEvent(new CustomEvent("quillo-wire", { detail: (w) => { WIRE = w; } }));
const wireFetch = (u, init) => (WIRE && WIRE.fetch ? WIRE.fetch(u, init) : fetch(u, init));
const openSocket = (path) => (WIRE && WIRE.socket ? WIRE.socket(path) : new WebSocket(location.origin.replace(/^http/, "ws") + path));

// ---- values ------------------------------------------------------------

class QuilloError extends Error {}
class Ret { constructor(v) { this.v = v; } }
// `own`: shown as it is. The program wrote it (`err(x)`, a record's), the server sent it (having
// hidden what a visitor is not to read), or it is of the page's own JavaScript (`js`, a library's
// or a module's name), which has no server side and holds nothing of the server's. The rest are a
// builtin's, whose text is hidden as the server hides it (see `repr`; FEEDBACK55.md, Q-773; branch
// feedback55).
class Err { constructor(e, own) { this.error = e === undefined ? null : e; this.own = own === true; } }

const hasOwn = (o, k) => Object.prototype.hasOwnProperty.call(o, k);
// Puts a key in a map the program made: "__proto__" is a key like any
// other there (from JSON, say), not the object's prototype. Spreading
// (`{ ...a, ...b }`) keeps it too; Object.assign and `o[k] = v` do not.
const putKey = (o, k, v) => {
  if (k === "__proto__") Object.defineProperty(o, k, { value: v, enumerable: true, writable: true, configurable: true });
  else o[k] = v;
  return o;
};

// A map's keys in the order they were set, as the server keeps them: JavaScript lists an object's
// keys that are array indexes ("0" to "4294967294" written so) first, by number, then the others
// in the order they were set, so `{ b: 1, "1": 2 }` listed "1" first (FEEDBACK55.md, Q-875; branch
// feedback55). A map with such a key keeps its keys in their order beside it: a list (ORD) and how
// many of it are the map's (ORDN), symbols set as plain properties (defineProperty took 70 ns, and
// made a spread of the map 20 times as slow). The rest, nearly all, need none. The page's code
// never has such a map, but a plain copy of it (`forPage`, `lenient`), so the order kept is always
// the map's: nothing changes a map once it is made. A map made from another shares its list, and
// adds a key to its end while no other map has: setting a key in a map of 1,000 made the list
// again, twice the time of the copy itself. A map with no list is in JavaScript's order: its keys
// as Object.keys lists them (read from JSON so, or made with array indexes alone set in rising
// order, ids, whose largest is under IDXTOP, Infinity once another key came after them).
const ORD = Symbol("order"), ORDN = Symbol("count"), IDXTOP = Symbol("top");
function isIdx(k) {
  const c = k.charCodeAt(0);
  if (!(c >= 48 && c <= 57)) return false;
  const n = k.length;
  if (n === 1) return true;
  if (c === 48 || n > 10) return false;
  for (let i = 1; i < n; i++) { const d = k.charCodeAt(i); if (d < 48 || d > 57) return false; }
  return n < 10 || k <= "4294967294";
}
// The order map o keeps, or undefined; a page's Proxy that refuses the read has none.
const ordOf = (o) => { try { return o[ORD]; } catch (_) { return undefined; } };
const topOf = (o) => { try { return o[IDXTOP]; } catch (_) { return undefined; } };
// o, kept in the order of the first n of ks (its keys, each once).
const ordered = (o, ks, n = ks.length) => { o[ORD] = ks; o[ORDN] = n; return o; };
// The keys of map o in the order they were set, to read, not to change.
const mkeys = (o) => { const ord = ordOf(o); if (ord === undefined) return Object.keys(o); const n = o[ORDN]; return n === ord.length ? ord : ord.slice(0, n); };
// Key k set to v in o, a map being made (a copy, or a new one), in its place: a key set again keeps
// its first place.
function mput(o, k, v) {
  const ord = o[ORD];
  if (ord !== undefined) {
    if (!hasOwn(o, k)) {
      const n = o[ORDN];
      if (n === ord.length) { ord.push(k); o[ORDN] = n + 1; } else { const ks = ord.slice(0, n); ks.push(k); ordered(o, ks); }
    }
  } else if (!hasOwn(o, k)) {
    const top = o[IDXTOP];
    if (isIdx(k)) {
      const x = +k;
      if (top === undefined ? isEmpty(o) : x > top) o[IDXTOP] = x;
      else { const ks = Object.keys(o); ks.push(k); ordered(o, ks); }
    } else if (top !== undefined) o[IDXTOP] = Infinity;
  }
  return putKey(o, k, v);
}
const isEmpty = (o) => { for (const _ in o) return false; return true; };
// Key k taken out of o, a map being made (a copy, or a new one).
function mdel(o, k) {
  if (!hasOwn(o, k)) return o;
  delete o[k];
  const ord = o[ORD];
  if (ord !== undefined) {
    const n = o[ORDN];
    if (ord[n - 1] === k) o[ORDN] = n - 1;
    else { const ks = ord.slice(0, n); ks.splice(ks.indexOf(k), 1); ordered(o, ks); }
  }
  return o;
}
// a's keys and b's, in a's order and then b's new ones; b's values where both have one: in
// JavaScript's order when both are and b adds no array index.
function mmerge(a, b) {
  const out = { ...a, ...b };
  if (ordOf(a) === undefined && ordOf(b) === undefined && !newIdx(a, b)) {
    // b's rising ids are not out's, and a key of b's after a's (FEEDBACK55.md, Q-875's second
    // review; branch feedback55).
    const ta = topOf(a);
    if (ta !== undefined || topOf(b) !== undefined) out[IDXTOP] = ta === undefined ? undefined : Infinity;
    return out;
  }
  const ks = mkeys(a).slice();
  for (const k of mkeys(b)) if (!hasOwn(a, k)) ks.push(k);
  return ordered(out, ks);
}
// Whether map b, in JavaScript's order, has an array index that a has not.
function newIdx(a, b) {
  for (const k of Object.keys(b)) { if (!isIdx(k)) return false; if (!hasOwn(a, k)) return true; }
  return false;
}
// Whether map v keeps an order of its own (a list, or rising ids), which the page's code is not
// given (see `forPage`; FEEDBACK55.md, Q-875's second review; branch feedback55).
const keepsOrder = (v) => ordOf(v) !== undefined || topOf(v) !== undefined;
// Map v as the page's code has it: a plain copy, in the order JavaScript lists it.
function plainOf(v) {
  const o = {};
  for (const k of Object.keys(v)) putKey(o, k, v[k]);
  return o;
}
const fail = (msg) => { throw new QuilloError(msg); };
const isNil = (v) => v === null || v === undefined;
const isEvent = (v) => typeof Event !== "undefined" && v instanceof Event;
const isMap = (v) => v !== null && typeof v === "object" && (v.constructor === Object || (!Array.isArray(v) && !(v instanceof Err) && !(v instanceof Html) && !(v instanceof Dec) && !(v instanceof Fl) && !isEvent(v)));
const isInt = (v) => typeof v === "number" && Number.isInteger(v);

// A float whose value is whole (2.0, -0.0, 1e300): a JS number cannot say it is no int, so it is
// held in an Fl, and `2.0 / 0` is inf, an `int` parameter refuses it, and `type_of` says float, as
// on the server. A float with a fraction, NaN and the infinities stay numbers; an int is a whole
// number to 2^53, a BigInt past it (FEEDBACK55.md, Q-775; branch feedback55).
// It reads as its number where the page's code (or the runtime's own) takes one: `Number(x)`, a
// player's volume, a timer's delay.
class Fl { constructor(f) { this.f = f; } valueOf() { return this.f; } toString() { return String(this.f); } }
// A float answer as the page keeps it.
const fl = (x) => (Number.isInteger(x) ? new Fl(x) : x);
const isFloat = (v) => v instanceof Fl || (typeof v === "number" && !Number.isInteger(v));
// A number as a float, as the server's `as_f64` reads it (an int past 2^53 too); null for what is
// no number.
const numOf = (v) => (typeof v === "number" ? v : v instanceof Fl ? v.f : typeof v === "bigint" ? Number(v) : null);
const isNumber = (v) => typeof v === "number" || typeof v === "bigint" || v instanceof Fl;

// ---- decimals (src/decimal.rs, the same answers) -------------------------
// A decimal is a whole number (a BigInt, at most 2^96 - 1 either way) and
// how many of its digits come after the point (0 to 28). Ints past what a
// JS number holds whole (2^53) are BigInts; smaller ones stay numbers, so
// the usual arithmetic never meets either.

class Dec { constructor(m, s) { this.m = m; this.s = s; } }
const DEC_MAX = (1n << 96n) - 1n;
const DEC_SCALE = 28;
const P10 = [1n];
const p10 = (k) => { while (P10.length <= k) P10.push(P10[P10.length - 1] * 10n); return P10[k]; };
const babs = (x) => (x < 0n ? -x : x);
const ROUNDINGS = "half_even, half_up, half_down, up, down, ceiling or floor";
const DEC_FLOAT = ": a dec is exact and a float is not, so convert one with dec(x) or float(x)";
const FLOAT_ONLY = " (this works on floats: convert a dec with float(x))";
const decHint = (a, b) => ((a instanceof Dec && isFloat(b)) || (b instanceof Dec && isFloat(a)) ? DEC_FLOAT : "");
const compareHint = (a, b) => (isNil(a) !== isNil(b) ? NIL_COMPARED : decHint(a, b));
// As on the server (FEEDBACK28.md, Q-347; branch feedback29-fixes).
const NIL_COMPARED = ": what is not found is null, not -1, from `index_of`, `find` and `get` too, so ask `x == null` first";

// Whether to add one to what is kept, given the first digit dropped,
// whether any after it are not zero, and whether what is kept is odd.
function decBump(mode, digit, rest, odd, neg) {
  const any = digit !== 0 || rest;
  switch (mode) {
    case "half_even": return digit > 5 || (digit === 5 && (rest || odd));
    case "half_up": return digit >= 5;
    case "half_down": return digit > 5 || (digit === 5 && rest);
    case "up": return any;
    case "down": return false;
    case "ceiling": return any && !neg;
    case "floor": return any && neg;
  }
  return false;
}

// mag at scale made to fit (`fit` in src/decimal.rs): digits past the
// 28th decimal or past 96 bits dropped and the rest rounded; null when the
// whole part alone does not fit.
function decFit(neg, mag, scale, sticky, mode) {
  let digit = 0, rest = sticky, dropped = sticky;
  for (;;) {
    while (scale > DEC_SCALE || mag > DEC_MAX) {
      if (scale === 0) return null;
      rest = rest || digit !== 0;
      digit = Number(mag % 10n);
      mag /= 10n;
      scale--;
      dropped = true;
    }
    if (dropped && decBump(mode, digit, rest, (mag & 1n) === 1n, neg)) {
      mag += 1n;
      digit = 0; rest = false; dropped = false;
      continue;
    }
    return new Dec(neg ? -mag : mag, scale);
  }
}
const decOk = (d) => (d === null ? fail("decimal overflow") : d);

function decAdd(a, b) {
  if (a.s === b.s) { const m = a.m + b.m; if (babs(m) <= DEC_MAX) return new Dec(m, a.s); }
  const s = Math.max(a.s, b.s);
  const m = a.m * p10(s - a.s) + b.m * p10(s - b.s);
  return decOk(decFit(m < 0n, babs(m), s, false, "half_even"));
}
function decMul(a, b) {
  const m = a.m * b.m, s = a.s + b.s;
  if (s <= DEC_SCALE && babs(m) <= DEC_MAX) return new Dec(m, s);
  return decOk(decFit((a.m < 0n) !== (b.m < 0n), babs(m), s, false, "half_even"));
}
// Long division, a digit at a time, while there is a remainder and room.
function decDiv(a, b) {
  if (b.m === 0n) fail("division by zero");
  const bm = babs(b.m);
  let scale = a.s - b.s, q = babs(a.m) / bm, r = babs(a.m) % bm;
  while (r !== 0n && (scale < 0 || (scale <= DEC_SCALE && q <= DEC_MAX))) {
    r *= 10n;
    q = q * 10n + r / bm;
    r %= bm;
    scale++;
  }
  if (scale < 0) { q *= p10(-scale); scale = 0; }
  return decOk(decFit((a.m < 0n) !== (b.m < 0n), q, scale, r !== 0n, "half_even"));
}
function decRem(a, b) {
  if (b.m === 0n) fail("division by zero");
  const s = Math.max(a.s, b.s);
  const r = (babs(a.m) * p10(s - a.s)) % (babs(b.m) * p10(s - b.s));
  return decOk(decFit(a.m < 0n, r, s, false, "half_even"));
}
// Rounded to `digits` decimals, padded with zeros when it has fewer.
function decRound(d, digits, mode) {
  digits = Math.min(Math.max(digits, -DEC_SCALE), DEC_SCALE);
  if (digits >= d.s) {
    const m = d.m * p10(digits - d.s);
    return babs(m) <= DEC_MAX ? new Dec(m, digits) : d;
  }
  const neg = d.m < 0n, mag = babs(d.m), p = p10(d.s - digits), unit = p / 10n;
  let kept = mag / p;
  const gone = mag % p;
  if (decBump(mode, Number(gone / unit), gone % unit !== 0n, (kept & 1n) === 1n, neg)) kept += 1n;
  return decOk(digits >= 0 ? decFit(neg, kept, digits, false, mode) : decFit(neg, kept * p10(-digits), 0, false, mode));
}
function decCmp(a, b) {
  const s = Math.max(a.s, b.s);
  const x = a.m * p10(s - a.s), y = b.m * p10(s - b.s);
  return x < y ? -1 : x > y ? 1 : 0;
}
function decText(d) {
  let t = babs(d.m).toString();
  if (d.s > 0) {
    if (t.length <= d.s) t = "0".repeat(d.s - t.length + 1) + t;
    t = t.slice(0, t.length - d.s) + "." + t.slice(t.length - d.s);
  }
  return d.m < 0n ? "-" + t : t;
}
// Its text with no zeros at the end of the fraction: as a map key.
function decKey(d) {
  let m = d.m, s = d.s;
  while (s > 0 && m % 10n === 0n) { m /= 10n; s--; }
  return decText(new Dec(m, s));
}
const DEC_RE = /^([+-]?)(\d*)(?:\.(\d*))?(?:[eE](\+?\d{1,6}|-\d{1,5}))?$/;
// Text as a program writes a number, exactly (null if digits past what a
// decimal holds are not zeros), or rounded half to even.
function decParse(s, rounded) {
  if (typeof s !== "string" || s.length === 0 || s.length > 400) return null;
  const r = DEC_RE.exec(s);
  if (!r) return null;
  const whole = r[2], frac = r[3] || "";
  if (whole.length + frac.length === 0) return null;
  const neg = r[1] === "-";
  let mag = BigInt(whole + frac);
  let scale = frac.length - (r[4] ? parseInt(r[4], 10) : 0);
  if (scale < 0) {
    if (mag !== 0n && -scale > 80) return null;
    if (mag !== 0n) mag *= p10(-scale);
    scale = 0;
  }
  // Zero is zero however many zeros are written after the point.
  if (mag === 0n) scale = Math.min(scale, DEC_SCALE);
  if (scale > 400) return null;
  if (rounded) return decFit(neg, mag, scale, false, "half_even");
  while (scale > DEC_SCALE || mag > DEC_MAX) {
    if (scale === 0 || mag % 10n !== 0n) return null;
    mag /= 10n;
    scale--;
  }
  return new Dec(neg ? -mag : mag, scale);
}
// A float through its shortest text, so 0.1 is 0.1.
const decFromFloat = (f) => (Number.isFinite(f) && Math.abs(f) < 7.9228162514264337e28 ? decParse(f.toExponential(), true) : null);
// A value as a decimal, for a `dec` pattern or record field.
function asDec(v) {
  if (v instanceof Dec) return v;
  if (isInt(v) || typeof v === "bigint") return new Dec(BigInt(v), 0);
  if (typeof v === "number") return decFromFloat(v);
  if (v instanceof Fl) return decFromFloat(v.f);
  if (typeof v === "string") return decParse(v.trim(), false);
  return null;
}
function decArith(a, b, op, verb) {
  const x = a instanceof Dec ? a : isInt(a) || typeof a === "bigint" ? new Dec(BigInt(a), 0) : null;
  const y = b instanceof Dec ? b : isInt(b) || typeof b === "bigint" ? new Dec(BigInt(b), 0) : null;
  if (x === null || y === null) fail(`cannot ${verb} ${typeName(a)} and ${typeName(b)}${decHint(a, b)}`);
  switch (op) {
    case "+": return decAdd(x, y);
    case "-": return decAdd(x, new Dec(-y.m, y.s));
    case "*": return decMul(x, y);
    case "/": return decDiv(x, y);
  }
  return decRem(x, y);
}

// An int as the page keeps it: a number while it is whole as one.
const I64_MIN = -(1n << 63n), I64_MAX = (1n << 63n) - 1n;
const intOf = (b) => (b >= -9007199254740991n && b <= 9007199254740991n ? Number(b) : b);
const bigChecked = (b) => (b < I64_MIN || b > I64_MAX ? fail("integer overflow") : intOf(b));
// Arithmetic with an int past 2^53: exact, 64-bit as on the server; with a float, a float.
function bigArith(a, b, op, verb) {
  const ai = isInt(a) || typeof a === "bigint", bi = isInt(b) || typeof b === "bigint";
  if (ai && bi) {
    const x = BigInt(a), y = BigInt(b);
    if ((op === "/" || op === "%") && y === 0n) fail("division by zero");
    // Not exact: a float, whole too past 2^53 (FEEDBACK55.md, Q-775; branch feedback55).
    if (op === "/") return x % y === 0n ? bigChecked(x / y) : fl(Number(x) / Number(y));
    return bigChecked(op === "+" ? x + y : op === "-" ? x - y : op === "*" ? x * y : x % y);
  }
  return flArith(a, b, op, verb);
}
// Arithmetic with a float on either side: in floats, an int as its float, as on the server; the
// answer a float, an Fl when whole (FEEDBACK55.md, Q-775; branch feedback55).
function flArith(a, b, op, verb) {
  const x = numOf(a), y = numOf(b);
  if (x === null || y === null) fail(`cannot ${verb} ${typeName(a)} and ${typeName(b)}`);
  return fl(op === "+" ? x + y : op === "-" ? x - y : op === "*" ? x * y : op === "/" ? x / y : x % y);
}
// What Quillo code does not reach on the page's objects: what runs text as
// code or HTML (markup in components does that safely), and the way to the
// Function constructor. These checks are a guard-rail against mistakes, not
// a wall: the page's own scripts can do anything, and a determined program
// can find a way through them.
const FOREIGN_DENY = ["eval", "Function", "setTimeout", "setInterval", "execScript", "execCommand", "insertAdjacentHTML",
  "setHTMLUnsafe", "parseHTMLUnsafe", "createContextualFragment", "write", "writeln", "constructor", "__proto__", "prototype",
  "__defineGetter__", "__defineSetter__", "__lookupGetter__", "__lookupSetter__"];
function foreignName(name) {
  if (FOREIGN_DENY.includes(name)) fail(`\`${name}\` is not used from Quillo code: it runs text as code or HTML`);
}
// Calls a method of the page's object `o`, unless it would run text as code.
function foreignCall(o, name, args) {
  foreignName(name);
  const f = o[name];
  return fromPage(Reflect.apply(f, o, guardCall(`\`${name}\``, f, args, name)));
}
// An object from the page's scripts (an element, a library's), not one the
// program made: its inherited properties and methods are its fields too.
// By its prototype: a map may hold the key `constructor` (FEEDBACK25.md, Q-303; branch
// feedback26-fixes).
const foreign = (v) => {
  if (v === null || typeof v !== "object") return false;
  if (v === globalThis) return true;
  const p = Object.getPrototypeOf(v);
  return p !== Object.prototype && p !== null && !Array.isArray(v) && !(v instanceof Err) && !(v instanceof Html) && !(v instanceof Dec) && !(v instanceof Fl);
};
// Functions of the page that run text as code or HTML, or that step around
// the checks here (setting any property, calling a method with any `this`,
// reading a property by a name from data, making a function from text, or
// parsing markup into nodes): Quillo code calls none of them, and hands
// none to the page's functions, however it came by one.
let deniedFns = null;
// Where the tag of an element made, and the name of an attribute set, are
// in the arguments of the page's functions that make and set them.
let makers = null;
let setters = null;
function guarded() {
  if (deniedFns !== null) return deniedFns;
  const g = globalThis, out = new Set();
  const add = (o, ...names) => {
    if (o) for (const n of names) {
      // Getters of the page's prototypes throw when read off them.
      try { if (typeof o[n] === "function") out.add(o[n]); } catch (_) {}
    }
  };
  add(g, "eval", "Function", "setTimeout", "setInterval", "execScript");
  // Function's kin, which any async or generator function leads to.
  for (const f of [async function () {}, function* () {}, async function* () {}]) out.add(Object.getPrototypeOf(f).constructor);
  add(Function.prototype, "call", "apply", "bind");
  add(g.Reflect, "apply", "construct", "get", "set", "defineProperty", "setPrototypeOf", "getOwnPropertyDescriptor");
  add(Object, "assign", "defineProperty", "defineProperties", "setPrototypeOf", "getOwnPropertyDescriptor", "getOwnPropertyDescriptors");
  add(g.DOMParser && g.DOMParser.prototype, "parseFromString");
  add(g.Document, "parseHTMLUnsafe");
  add(g.Document && g.Document.prototype, "write", "writeln", "execCommand");
  add(g.Element && g.Element.prototype, "insertAdjacentHTML", "setHTMLUnsafe");
  add(g.ShadowRoot && g.ShadowRoot.prototype, "setHTMLUnsafe");
  add(g.Range && g.Range.prototype, "createContextualFragment");
  const places = (o, pairs) => {
    const m = new Map();
    if (o) for (const [n, i] of pairs) if (typeof o.prototype[n] === "function") m.set(o.prototype[n], i);
    return m;
  };
  makers = places(g.Document, [["createElement", 0], ["createElementNS", 1]]);
  setters = places(g.Element, [["setAttribute", 0], ["setAttributeNS", 1]]);
  return (deniedFns = out);
}
function denied(f) {
  if (guarded().has(f)) fail(`\`${f.name || "this function"}\` is not called from Quillo code: it runs text as code or steps around these checks`);
}
// Functions that came from the page as values (`js("lib.fn")`, or what one
// of its functions gave back), each with what calls it: `pageCall`, which
// checks what it is given.
const pageFns = new WeakMap();
function fromPage(r) {
  if (typeof r === "function") {
    if (guarded().has(r)) fail(`\`${r.name || "this function"}\` is not given to Quillo code: it runs text as code`);
    pageFns.set(r, pageCall);
  }
  return r === undefined ? null : r;
}
function pageCall(f, args) {
  guardArgs(`\`${f.name || "this function"}\``, args, 0);
  return fromPage(f(...forPage(args, 0)));
}
// A whole float is a number to the page's code, in a list or a map too: it went as an Fl, so
// `JSON.stringify({ a: 1.0 })` was {"a":{"f":1}} and `Number.isFinite(2.0)` false, and a chart
// library's numbers objects (found in review of FEEDBACK55.md's Q-775; branch feedback55). Copied
// where one changes.
// A map that keeps its order is given as a plain copy, in the order JavaScript lists it: its
// symbol broke URLSearchParams, Headers and the page's deep equality (FEEDBACK55.md, Q-875's
// review; branch feedback55); past 16 levels too (`noOrd`).
function forPage(v, depth) {
  if (v instanceof Fl) return v.f;
  if (v === null || typeof v !== "object" || foreign(v)) return v;
  if (depth >= 16) return noOrd(v, new Map());
  if (Array.isArray(v)) {
    let out = v;
    v.forEach((x, i) => { const y = forPage(x, depth + 1); if (y !== x) { if (out === v) out = v.slice(); out[i] = y; } });
    return out;
  }
  if (!isMap(v)) return v;
  let out = keepsOrder(v) ? plainOf(v) : v;
  for (const k of Object.keys(v)) {
    const y = forPage(v[k], depth + 1);
    if (y !== v[k]) { if (out === v) out = { ...v }; putKey(out, k, y); }
  }
  return out;
}
// Past where `forPage` and `lenient` go (16 levels), a map that keeps its order still given as a
// plain copy, and what holds one copied; anything else as it is, the page's own objects once.
// A whole float there too is a number (FEEDBACK55.md, Q-875's second review; branch feedback55).
function noOrd(v, seen) {
  if (v instanceof Fl) return v.f;
  if (v === null || typeof v !== "object" || foreign(v)) return v;
  if (seen.has(v)) return seen.get(v);
  seen.set(v, v);
  let out = v;
  if (Array.isArray(v)) v.forEach((x, i) => { const y = noOrd(x, seen); if (y !== x) { if (out === v) out = v.slice(); out[i] = y; } });
  else if (isMap(v)) {
    if (keepsOrder(v)) out = plainOf(v);
    for (const k of Object.keys(v)) { const y = noOrd(v[k], seen); if (y !== v[k]) { if (out === v) out = { ...v }; putKey(out, k, y); } }
  }
  seen.set(v, out);
  return out;
}
// A call of the page's function f (the method `name`) with args: f is not
// one of the above, makes no script element, sets no handler or srcdoc,
// and is given nothing that would run as code.
function guardCall(what, f, args, name) {
  denied(f);
  if (makers.has(f)) {
    const tag = String(args[makers.get(f)]).split(":").pop().trim().toLowerCase();
    if (tag === "script") fail(`${what} does not make a script element: it runs text as code`);
  }
  const at = setters.has(f) ? setters.get(f) : name === "setAttribute" ? 0 : name === "setAttributeNS" ? 1 : -1;
  if (at >= 0) {
    const k = String(args[at]).toLowerCase();
    if (k.startsWith("on") || k === "srcdoc") fail(`${what} does not set ${k}: it runs text as code`);
  }
  guardArgs(what, args, 0);
  return lenient(args, 0);
}
// A function of the program's handed to the page is called the page's way:
// JavaScript passes callbacks more arguments than they use (Leaflet's
// `on("click", f)` passes the event), so it is given only as many as it
// takes, not refused for the rest. In lists and maps too (a library's
// options); those are copied when one changes (FEEDBACK5.md, Q-050; branch
// stays-fixes).
const lenientFns = new WeakMap();
function lenient(v, depth) {
  if (typeof v === "function") {
    // The page's own functions and builtins (`$p` null) take what they are given.
    if (pageFns.has(v) || v.$p === null) return v;
    // The same wrapper each time, so `off("click", f)` finds what `on` was given.
    let w = lenientFns.get(v);
    if (!w) {
      const f = v;
      // What the page passes as `undefined` is null to Quillo, as what its functions give
      // back is: a Promise of nothing calls `.then`'s function so (FEEDBACK28.md, Q-336;
      // branch feedback29-fixes).
      // What it gives back is the page's as an argument is (FEEDBACK55.md, Q-875's review; branch
      // feedback55).
      w = function (...a) {
        const b = a.slice(0, f.length);
        for (let i = 0; i < b.length; i++) if (b[i] === undefined) b[i] = null;
        const r = f.apply(this, b);
        return r instanceof Promise ? r.then((x) => forPage(x, 0)) : forPage(r, 0);
      };
      lenientFns.set(v, w);
    }
    return w;
  }
  // A number to the page, and a map that keeps its order a plain copy (see `forPage`).
  if (v instanceof Fl) return v.f;
  if (v === null || typeof v !== "object" || foreign(v)) return v;
  if (depth >= 16) return noOrd(v, new Map());
  if (Array.isArray(v)) {
    let out = v;
    v.forEach((x, i) => { const y = lenient(x, depth + 1); if (y !== x) { if (out === v) out = v.slice(); out[i] = y; } });
    return out;
  }
  if (!isMap(v)) return v;
  let out = keepsOrder(v) ? plainOf(v) : v;
  for (const k of Object.keys(v)) {
    const y = lenient(v[k], depth + 1);
    if (y !== v[k]) { if (out === v) out = { ...v }; out[k] = y; }
  }
  return out;
}
// What is handed to the page's functions: no javascript: URL (in a list or
// a map too, which turn into it as text), no function above, no map that
// makes its own text, and no node of another document (markup parsed from
// text, whose handlers would run once it is on the page).
function guardArgs(what, args, depth) {
  for (const a of args) {
    if (typeof a === "string") {
      if (scriptURL(a)) fail(`${what} is not given a ${scriptScheme(a)} URL`);
    } else if (typeof a === "function") {
      if (guarded().has(a)) fail(`${what} is not given \`${a.name || "this function"}\`: it runs text as code`);
    } else if (a !== null && typeof a === "object" && depth < 16) {
      if (Array.isArray(a)) guardArgs(what, a, depth + 1);
      else if (foreign(a)) {
        if (typeof document !== "undefined" && typeof a.nodeType === "number" && (a.nodeType === 9 ? a : a.ownerDocument) !== document)
          fail(`${what} is not given a node of another document: markup parsed from text runs its handlers on the page`);
      } else if (isMap(a)) {
        if (typeof a.toString === "function" && hasOwn(a, "toString") || typeof a.valueOf === "function" && hasOwn(a, "valueOf"))
          fail(`${what} is not given a map with its own toString or valueOf`);
        guardArgs(what, Object.values(a), depth + 1);
      }
    }
  }
}

// `x.name = v` in a component: a field of the page's object x (an event's
// that it does not have is its element's: `e.value = ""`), not one that
// runs text as code or HTML, and given nothing that would.
function setField(o, name, v, op) {
  if (!foreign(o)) {
    fail(isMap(o) ? `a map's fields are not set in place: write \`m = set(m, "${name}", …)\``
      : `${typeName(o)} has no field \`${name}\` to set: only the page's objects (an element, an event) do`);
  }
  foreignName(name);
  if (/^on/i.test(name) && typeof v === "function")
    fail(`\`${name}\` is not set from Quillo code, as it would take text to run as code too: write x.addEventListener("${name.slice(2)}", f), or @${name.slice(2)} on an element`);
  if (/^on/i.test(name) || name === "innerHTML" || name === "outerHTML" || name === "srcdoc")
    fail(`\`${name}\` is not set from Quillo code: it runs text as code or HTML`);
  if (isEvent(o) && !(name in o) && o.target) o = o.target;
  if (op) v = (op === "+" ? add : sub)(field(o, name, null), v);
  guardArgs(`setting \`${name}\``, [v], 0);
  o[name] = forPage(v, 0);
}

function typeName(v) {
  if (isNil(v)) return "null";
  switch (typeof v) {
    case "boolean": return "bool";
    case "number": return Number.isInteger(v) ? "int" : "float";
    case "string": return "str";
    case "function": return "fn";
    case "bigint": return "int";
  }
  if (v instanceof Fl) return "float";
  if (v instanceof Dec) return "dec";
  if (Array.isArray(v)) return "list";
  if (v instanceof Err) return "error";
  if (v instanceof Html) return "html";
  // A Promise from the page's code is not a map (FEEDBACK6.md, Q-057; branch feedback6-fixes).
  if (typeof Promise !== "undefined" && v instanceof Promise) return "promise";
  return "map";
}

// "2026-11-20", a real day of years 1 to 9999, as the server's `date` type
// takes: apart from the date module, which a page that only checks a type
// does not carry (FEEDBACK13.md, Q-093; branch open-fixes).
function isDay(s) {
  const m = typeof s === "string" && /^(\d{4})-(\d{2})-(\d{2})$/.exec(s);
  if (!m) return false;
  const y = +m[1], mo = +m[2], d = +m[3];
  const feb = y % 4 === 0 && (y % 100 !== 0 || y % 400 === 0) ? 29 : 28;
  return y >= 1 && mo >= 1 && mo <= 12 && d >= 1 && d <= [31, feb, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][mo - 1];
}

function typeIs(t, v) {
  switch (t) {
    case "any": return true;
    case "int": return isInt(v) || typeof v === "bigint";
    // A whole float is an Fl (FEEDBACK55.md, Q-775; branch feedback55).
    case "float": return isFloat(v);
    case "num": return isNumber(v);
    case "dec": return v instanceof Dec;
    case "str": return typeof v === "string";
    // Text that is a date (FEEDBACK13.md, Q-093; branch open-fixes).
    case "date": return isDay(v);
    // Money every money function takes (FEEDBACK50.md, Q-621; branch feedback50-fixes).
    case "money": return moneyIs(v);
    case "bool": return typeof v === "boolean";
    case "list": return Array.isArray(v);
    case "map": return isMap(v) || isEvent(v);
    case "html": return v instanceof Html;
    case "fn": return typeof v === "function";
    default:
      // Strings a type names ("admin" | "member"), and the program's own types.
      if (t[0] === '"') return v === JSON.parse(t);
      return ownTypes ? ownTypes.is(t, v) : false;
  }
}

// The program's own types, when it has any (set by `W.type`, which brings
// the code for them into the bundle): checks, reasons, and making values.
let ownTypes = null;

// ---- records (src/record.rs) -------------------------------------------
// The program's own types, as the compiler gave them (W.type): a record's
// fields ({r: [[name, type, default]]}) or the type a name stands for ({a:
// type}). A type is {n: names, o: null allowed, e: list item, f: fields, t: text}.
const OWN_TYPES = Object.create(null);
const ownTyOf = (name) => ({ n: [name], o: false, e: null, f: null, t: name });
// Where a value goes wrong, and how ("" when it is just not the type).
const OWN_PLAIN = { path: "", what: "" };
const ownWhyAt = (seg, w) => ({ path: w.path === "" ? seg : w.path[0] === "[" ? seg + w.path : seg + "." + w.path, what: w.what });
const ownWhyHere = (w, t, v) => (w.what ? w : { path: w.path, what: `must be ${t.t}, got ${typeName(v)} ${short(v)}` });
const ownWhyText = (w) => (!w.what ? null : w.path ? `\`${w.path}\` ${w.what}` : w.what);
const ownBecause = (w) => { const t = w && ownWhyText(w); return t ? ` (${t})` : ""; };

// null when v is of type t, else where it is not. At the top of an
// annotation lists and maps are checked as any list and map; records field
// by field, and all that is inside them.
function ownFits(t, v, deep, depth = 0) {
  if ((t.o && isNil(v)) || depth > 64) return null;
  let best = OWN_PLAIN;
  for (const n of t.n) {
    const w = ownFitsName(n, t, v, deep, depth);
    if (!w) return null;
    if (!best.what && w.what) best = w;
  }
  return best;
}
function ownFieldsFit(fs, m, depth) {
  for (const [k, ft] of fs) {
    const x = hasOwn(m, k) ? m[k] : null;
    if (isNil(x)) { if (ft.o) continue; return { path: k, what: "is missing" }; }
    const w = ownFits(ft, x, true, depth + 1);
    if (w) return ownWhyAt(k, ownWhyHere(w, ft, x));
  }
  return null;
}
function ownFitsName(n, t, v, deep, depth) {
  if (n[0] === '"') return v === JSON.parse(n) ? null : OWN_PLAIN;
  const td = OWN_TYPES[n];
  if (td) {
    if (td.a) return ownFits(td.a, v, deep, depth + 1);
    return isMap(v) ? ownFieldsFit(td.r, v, depth) : OWN_PLAIN;
  }
  if (n === "dec") return asDec(v) === null ? OWN_PLAIN : null;
  if (/^[A-Z]/.test(n) || !typeIs(n, v)) return OWN_PLAIN;
  if (deep && n === "list" && t.e) {
    for (let i = 0; i < v.length; i++) {
      const w = ownFits(t.e, v[i], true, depth + 1);
      if (w) return ownWhyAt(`[${i}]`, ownWhyHere(w, t.e, v[i]));
    }
  }
  if (deep && n === "map" && t.f) return ownFieldsFit(t.f, v, depth);
  return null;
}

const ownHasRecord = (t, d = 0) => d < 8 && (t.n.some((n) => OWN_TYPES[n] && (OWN_TYPES[n].r || ownHasRecord(OWN_TYPES[n].a, d + 1))) || (!!t.e && ownHasRecord(t.e, d + 1)));
// Whether a type has a `float` in it, which an int is made into, as the server's `has_float`.
const ownHasFloat = (t, d = 0) => d < 8 && (t.n.some((n) => n === "float" || (OWN_TYPES[n] && OWN_TYPES[n].a && ownHasFloat(OWN_TYPES[n].a, d + 1))) || (!!t.e && ownHasFloat(t.e, d + 1)));

// v made a value of type t: records made from maps (defaults put in, other
// fields left out), lists of them item by item, the rest checked as it is.
function ownMake(t, v, depth = 0) {
  if ((t.o && isNil(v)) || depth > 64) return { ok: true, v };
  let best = OWN_PLAIN;
  for (const n of t.n) {
    const r = ownMakeName(n, t, v, depth);
    if (r.ok) return r;
    if (!best.what && r.w.what) best = r.w;
  }
  return { ok: false, w: best };
}
function ownMakeName(n, t, v, depth) {
  const td = OWN_TYPES[n];
  if (td) {
    if (td.a) return ownMake(td.a, v, depth + 1);
    if (!isMap(v)) return { ok: false, w: OWN_PLAIN };
    const out = {};
    for (const [k, ft, def] of td.r) {
      let x = hasOwn(v, k) ? v[k] : null;
      if (isNil(x)) {
        if (def) x = def();
        else if (ft.o) continue;
        else return { ok: false, w: { path: k, what: "is missing" } };
      }
      const r = ownMake(ft, x, depth + 1);
      if (!r.ok) return { ok: false, w: ownWhyAt(k, ownWhyHere(r.w, ft, x)) };
      // In the order the type has its fields, a quoted "1" too (FEEDBACK55.md, Q-875; branch feedback55).
      mput(out, k, r.v);
    }
    return { ok: true, v: out };
  }
  if (n === "list" && Array.isArray(v) && t.e && (ownHasRecord(t.e) || ownHasFloat(t.e))) {
    const out = [];
    for (let i = 0; i < v.length; i++) {
      const r = ownMake(t.e, v[i], depth + 1);
      if (!r.ok) return { ok: false, w: ownWhyAt(`[${i}]`, ownWhyHere(r.w, t.e, v[i])) };
      out.push(r.v);
    }
    return { ok: true, v: out };
  }
  // A float takes an int, made a float, as on the server (FEEDBACK18.md, Q-103): the int is no
  // float here either now (FEEDBACK55.md, Q-775; branch feedback55).
  if (n === "float" && (isInt(v) || typeof v === "bigint") && !t.n.includes("int")) return { ok: true, v: fl(Number(v)) };
  // A decimal field takes an int, numeric text or a JSON number.
  if (n === "dec") { const d = asDec(v); return d === null ? { ok: false, w: OWN_PLAIN } : { ok: true, v: d }; }
  // Money, held to money.of's rules, with what is wrong (FEEDBACK50.md, Q-621).
  if (n === "money") { const r = moneyFromData(v); return r.why ? { ok: false, w: { path: "", what: r.why } } : { ok: true, v: r.v }; }
  const w = ownFits({ n: [n], o: false, e: t.e, f: t.f, t: t.t }, v, true, depth);
  return w ? { ok: false, w } : { ok: true, v };
}



// A key as the server's `fmt_key` writes it: bare when it is a name of letters and digits of any
// script (FEEDBACK55.md, Q-782; branch feedback55).
const fmtKey = (k) => /^[\p{Alphabetic}_][\p{Alphabetic}\p{Number}_]*$/u.test(k) ? k : JSON.stringify(k);

// A number's text, as the server's: JavaScript writes Infinity (FEEDBACK16.md, Q-120; branch feedback20-fixes).
// Never in exponent form, as on the server (FEEDBACK25.md, Q-309; branch
// feedback26-fixes).
const numText = (v) => {
  if (v === Infinity) return "inf";
  if (v === -Infinity) return "-inf";
  const s = String(v), at = s.indexOf("e");
  if (at < 0) return s;
  const neg = s[0] === "-", exp = Number(s.slice(at + 1));
  const [whole, part = ""] = s.slice(neg ? 1 : 0, at).split(".");
  const digits = whole + part, point = whole.length + exp;
  const text = point <= 0 ? "0." + "0".repeat(-point) + digits
    : point >= digits.length ? digits + "0".repeat(point - digits.length)
    : digits.slice(0, point) + "." + digits.slice(point);
  return (neg ? "-" : "") + text;
};

function repr(v) {
  if (isNil(v)) return "null";
  if (typeof v === "string") return JSON.stringify(v);
  if (typeof v === "number") return numText(v);
  if (typeof v === "boolean") return String(v);
  if (typeof v === "bigint") return String(v);
  if (v instanceof Fl) return numText(v.f);
  if (v instanceof Dec) return decText(v) + "m";
  if (typeof v === "function") return "<fn " + (v.$name || "<fn>") + ">";
  if (Array.isArray(v)) return "[" + v.map(repr).join(", ") + "]";
  // A builtin's error says `internal error` as the server writes it, unless the server shows
  // errors (`--watch`): its text was shown once the browser made the component again, and made
  // it again for that (FEEDBACK55.md, Q-773; branch feedback55).
  if (v instanceof Err) return "err(" + (v.own || ERRORS_SHOWN ? repr(v.error) : "\"internal error\"") + ")";
  // Its markup, as on the server (FEEDBACK55.md, Q-779; branch feedback55).
  if (v instanceof Html) return htmlText(v);
  if (isEvent(v)) return "<event " + v.type + ">";
  // In the order set (FEEDBACK55.md, Q-875; branch feedback55).
  const ks = mkeys(v);
  return ks.length ? "{ " + ks.map((k) => fmtKey(k) + ": " + repr(v[k])).join(", ") + " }" : "{}";
}

const display = (v) => typeof v === "string" ? v : typeof v === "number" ? numText(v) : v instanceof Fl ? numText(v.f) : v instanceof Dec ? decText(v) : repr(v);
// A value as a map key (`set`, `get`, `group_by`): 12.5 and 12.50 are one.
const keyText = (v) => (v instanceof Dec ? decKey(v) : display(v));
const short = (v) => { const r = repr(v); return r.length > 40 ? r.slice(0, 40) + "…" : r; };
const truthy = (v) => !(isNil(v) || v === false || v instanceof Err);

function eq(a, b) {
  if (a === b) return true;
  // Different numbers, strings, bools and functions; null and undefined match.
  if (typeof a !== "object" || typeof b !== "object" || a === null || b === null) {
    // Two numbers (the usual case) test only their types here; an int past 2^53 (a BigInt) or a
    // whole float (an Fl) and a number, by their values, exactly (FEEDBACK55.md, Q-776; branch
    // feedback55).
    if (typeof a === "object" || typeof b === "object") return a instanceof Dec || b instanceof Dec ? decEq(a, b) : a instanceof Fl || b instanceof Fl ? numEq(a, b) : isNil(a) && isNil(b);
    return typeof a === "bigint" || typeof b === "bigint" ? numEq(a, b) : isNil(a) && isNil(b);
  }
  if (a instanceof Dec || b instanceof Dec) return decEq(a, b);
  if (a instanceof Fl || b instanceof Fl) return numEq(a, b);
  if (Array.isArray(a) && Array.isArray(b)) return a.length === b.length && a.every((x, i) => eq(x, b[i]));
  if (a instanceof Err && b instanceof Err) return eq(a.error, b.error);
  // Markup by its text, as on the server (FEEDBACK55.md, Q-779; branch feedback55).
  if (a instanceof Html && b instanceof Html) return htmlText(a) === htmlText(b);
  // The page's objects are themselves, not their fields: any two elements have none
  // (FEEDBACK28.md, Q-334; branch feedback29-fixes).
  if (foreign(a) || foreign(b)) return false;
  if (isMap(a) && isMap(b)) {
    const ka = Object.keys(a);
    return ka.length === Object.keys(b).length && ka.every((k) => hasOwn(b, k) && eq(a[k], b[k]));
  }
  return false;
}

// Two numbers (an int, a BigInt, a float or an Fl) by their values, exactly: JavaScript compares
// a BigInt with a number as numbers, not through a float (FEEDBACK55.md, Q-776; branch
// feedback55).
const numEq = (a, b) => numCmp(a, b) === 0;
function numCmp(a, b) {
  const x = a instanceof Fl ? a.f : a, y = b instanceof Fl ? b.f : b;
  if ((typeof x !== "number" && typeof x !== "bigint") || (typeof y !== "number" && typeof y !== "bigint")) return null;
  // NaN is not ordered, as on the server (FEEDBACK55.md, Q-780; branch feedback55).
  return x < y ? -1 : x > y ? 1 : x == y ? 0 : null;
}

// A decimal and a number, compared as decimals (a float through its text).
function decEq(a, b) {
  const x = a instanceof Dec ? a : isNumber(a) ? asDec(a) : null;
  const y = b instanceof Dec ? b : isNumber(b) ? asDec(b) : null;
  return x !== null && y !== null && decCmp(x, y) === 0;
}

// Text in the order of its code points, as the server's (by its UTF-8) has it: JavaScript's `<`
// goes by UTF-16 unit, which puts U+E000 to U+FFFF after the characters past U+FFFF
// (FEEDBACK55.md, Q-778; branch feedback55).
function strCmp(a, b) {
  const n = a.length < b.length ? a.length : b.length;
  for (let i = 0; i < n; i++) {
    let x = a.charCodeAt(i), y = b.charCodeAt(i);
    if (x === y) continue;
    // A surrogate (half of a character past U+FFFF) comes after U+E000 to U+FFFF.
    if (x >= 0xd800 && y >= 0xd800) { x += x < 0xe000 ? 0x2000 : -0x800; y += y < 0xe000 ? 0x2000 : -0x800; }
    return x < y ? -1 : 1;
  }
  return a.length < b.length ? -1 : a.length > b.length ? 1 : 0;
}

function cmp(a, b) {
  // NaN is not ordered, as on the server (FEEDBACK55.md, Q-780; branch feedback55).
  if (typeof a === "number" && typeof b === "number") return a < b ? -1 : a > b ? 1 : a === b ? 0 : null;
  if (typeof a === "string" && typeof b === "string") return a === b ? 0 : strCmp(a, b);
  // A decimal with a decimal or an int; not with a float.
  if (a instanceof Dec || b instanceof Dec) {
    const x = a instanceof Dec ? a : isInt(a) || typeof a === "bigint" ? new Dec(BigInt(a), 0) : null;
    const y = b instanceof Dec ? b : isInt(b) || typeof b === "bigint" ? new Dec(BigInt(b), 0) : null;
    return x === null || y === null ? null : decCmp(x, y);
  }
  // Ints past 2^53 and whole floats too, exactly (FEEDBACK55.md, Q-776; branch feedback55).
  if (isNumber(a) && isNumber(b)) return numCmp(a, b);
  // Item by item, as on the server (FEEDBACK25.md, Q-320; branch feedback26-fixes).
  if (Array.isArray(a) && Array.isArray(b)) {
    for (let i = 0; i < a.length && i < b.length; i++) {
      const c = cmp(a[i], b[i]);
      if (c !== 0) return c;
    }
    return a.length < b.length ? -1 : a.length > b.length ? 1 : 0;
  }
  return null;
}

// ---- operators ---------------------------------------------------------

// Ints past 2^53 are exact, as a literal is, or a sum of money in cents loses units
// with no error (FEEDBACK25.md, Q-300; branch feedback26-fixes); past 64 bits an overflow, as on
// the server: `3037000500 * 3037000500` was a number (FEEDBACK55.md, Q-776; branch feedback55).
const SAFE = Number.MAX_SAFE_INTEGER;
// What `+`, `-` or `*` of two numbers gave, `r`: two ints' past 2^53 worked out exactly; a whole
// one with a float on a side, an Fl; an int has no -0, which `-3 * 0` gave, and `1.5 / x` was
// -inf (FEEDBACK55.md, Q-775; branch feedback55).
const num2 = (r, a, b, op, verb) => (Number.isInteger(a) && Number.isInteger(b) ? (r > SAFE || r < -SAFE ? bigArith(a, b, op, verb) : r + 0) : fl(r));

// Two ints of 32 bits (the usual case) first, with nothing to work out; then a whole float with a
// number, without the general case (found in review: the float loop was half again as slow).
function add(a, b) {
  if (typeof a === "number") {
    if (typeof b === "number") return (a | 0) === a && (b | 0) === b ? a + b : num2(a + b, a, b, "+", "add");
    if (b instanceof Fl) return fl(a + b.f);
  } else if (a instanceof Fl) {
    if (typeof b === "number") return fl(a.f + b);
    if (b instanceof Fl) return fl(a.f + b.f);
  }
  if (typeof a === "string") return a + display(b);
  if (typeof b === "string") return display(a) + b;
  if (a instanceof Dec || b instanceof Dec) return decArith(a, b, "+", "add");
  if (a instanceof Fl || b instanceof Fl) return flArith(a, b, "+", "add");
  if (typeof a === "bigint" || typeof b === "bigint") return bigArith(a, b, "+", "add");
  if (a instanceof Html && b instanceof Html) return frag([a, b]);
  if (Array.isArray(a) && Array.isArray(b)) return a.concat(b);
  if (isMap(a) && isMap(b)) return moneyOp(a, b, false) ?? mmerge(a, b);
  fail(`cannot add ${typeName(a)} and ${typeName(b)}`);
}

// Numbers: the usual case first; anything else says what is wrong. Only two ints divide by zero
// with an error: a float on a side gives inf or NaN, as on the server (FEEDBACK55.md, Q-775;
// branch feedback55).
function arith(a, b, op, verb) {
  if (a instanceof Dec || b instanceof Dec) return decArith(a, b, op, verb);
  if (a instanceof Fl || b instanceof Fl) return flArith(a, b, op, verb);
  if (typeof a === "bigint" || typeof b === "bigint") return bigArith(a, b, op, verb);
  if (typeof a !== "number" || typeof b !== "number") fail(`cannot ${verb} ${typeName(a)} and ${typeName(b)}`);
  if (isInt(a) && isInt(b) && b === 0) fail("division by zero");
  return fl(op === "/" ? a / b : a % b);
}
function sub(a, b) {
  if (typeof a === "number") {
    if (typeof b === "number") return (a | 0) === a && (b | 0) === b ? a - b : num2(a - b, a, b, "-", "subtract");
    if (b instanceof Fl) return fl(a - b.f);
  } else if (a instanceof Fl) {
    if (typeof b === "number") return fl(a.f - b);
    if (b instanceof Fl) return fl(a.f - b.f);
  }
  return (isMap(a) && isMap(b) && moneyOp(a, b, true)) || arith(a, b, "-", "subtract");
}
function mul(a, b) {
  if (typeof a === "number") {
    if (typeof b === "number") {
      if ((a | 0) === a && (b | 0) === b) { const r = a * b; if (r <= SAFE && r >= -SAFE) return r + 0; }
      return num2(a * b, a, b, "*", "multiply");
    }
    if (b instanceof Fl) return fl(a * b.f);
  } else if (a instanceof Fl) {
    if (typeof b === "number") return fl(a.f * b);
    if (b instanceof Fl) return fl(a.f * b.f);
  }
  return arith(a, b, "*", "multiply");
}
// Two ints divide to an int, or to a float with a fraction; a float on a side, to a float.
const div = (a, b) => (typeof a === "number" && typeof b === "number" && b !== 0 ? (Number.isInteger(a) && Number.isInteger(b) ? a / b : fl(a / b)) : arith(a, b, "/", "divide"));
const mod = (a, b) => (typeof a === "number" && typeof b === "number" && b !== 0 ? (Number.isInteger(a) && Number.isInteger(b) ? (a % b) + 0 : fl(a % b)) : arith(a, b, "%", "take the remainder of"));

function order(a, b) {
  const c = cmp(a, b);
  if (c === null) fail(`cannot compare ${typeName(a)} with ${typeName(b)}${compareHint(a, b)}`);
  return c;
}
// Two numbers (a whole float too) compared where they stand; NaN, and the rest, through `order`
// (found in review).
const fnum = (v) => (typeof v === "number" ? v : v instanceof Fl ? v.f : NaN);
function lt(a, b) {
  if (typeof a === "number" && typeof b === "number") { if (a < b) return true; if (a >= b) return false; }
  else { const x = fnum(a), y = fnum(b); if (x < y) return true; if (x >= y) return false; }
  return order(a, b) < 0;
}
function le(a, b) {
  if (typeof a === "number" && typeof b === "number") { if (a <= b) return true; if (a > b) return false; }
  else { const x = fnum(a), y = fnum(b); if (x <= y) return true; if (x > y) return false; }
  return order(a, b) <= 0;
}
function gt(a, b) {
  if (typeof a === "number" && typeof b === "number") { if (a > b) return true; if (a <= b) return false; }
  else { const x = fnum(a), y = fnum(b); if (x > y) return true; if (x <= y) return false; }
  return order(a, b) > 0;
}
function ge(a, b) {
  if (typeof a === "number" && typeof b === "number") { if (a >= b) return true; if (a < b) return false; }
  else { const x = fnum(a), y = fnum(b); if (x >= y) return true; if (x < y) return false; }
  return order(a, b) >= 0;
}

function intRange(a, b) {
  // Past 2^53 too, each item exact (FEEDBACK55.md, Q-776; branch feedback55).
  if (typeof a === "bigint" || typeof b === "bigint") {
    if (!(isInt(a) || typeof a === "bigint") || !(isInt(b) || typeof b === "bigint")) fail("`..` needs two ints");
    const x = BigInt(a), y = BigInt(b), out = [];
    if (y - x > 10000000n) fail("range is too large");
    for (let i = x; i < y; i++) out.push(intOf(i));
    return out;
  }
  if (!isInt(a) || !isInt(b)) fail("`..` needs two ints");
  if (b - a > 10000000) fail("range is too large");
  const out = [];
  for (let i = a; i < b; i++) out.push(i);
  return out;
}

function neg(v) {
  if (typeof v !== "number") {
    if (v instanceof Dec) return new Dec(-v.m, v.s);
    if (v instanceof Fl) return new Fl(-v.f);
    if (typeof v === "bigint") return bigChecked(-v);
    fail(`cannot negate ${typeName(v)}`);
  }
  return v === 0 ? 0 : -v;
}

function eventField(e, name) {
  const t = e.target || {};
  switch (name) {
    case "value": return hasOwn(e, "$value") ? e.$value : t.value === undefined ? null : t.value;
    case "checked": return hasOwn(e, "$checked") ? e.$checked : !!t.checked;
    case "type": return e.type;
    case "key": return e.key === undefined ? null : e.key;
    case "target": return t;
    case "form": {
      const f = t.tagName === "FORM" ? t : t.form;
      if (!f) return {};
      const out = {};
      // With the button that sent it, as the browser sends the form (FEEDBACK55.md, Q-767; branch
      // feedback55).
      const s = e.submitter;
      // In the form's order, as the server reads it (FEEDBACK55.md, Q-875; branch feedback55).
      for (const [k, v] of (s && s.form === f ? new FormData(f, s) : new FormData(f))) mput(out, k, typeof v === "string" ? v : v.name);
      return out;
    }
  }
  // Then the event's own fields (clientX, deltaY, ...), then the element's
  // (scrollTop, clientHeight, dataset, ...).
  for (const src of [e, t]) {
    const v = src[name];
    if (v !== undefined && typeof v !== "function") return v === null ? null : v;
  }
  return null;
}

// `x.name`: a map field, `e.value` on events, or a function called on x.
// `x.name` for a name Object.prototype does not have: a map's key is read
// directly (a key never holds undefined), anything else as `field` does.
function fieldp(v, name, fb) {
  if (v !== null && typeof v === "object" && v.constructor === Object && v !== globalThis) {
    const x = v[name];
    if (x !== undefined) return x;
  }
  return field(v, name, fb);
}

function field(v, name, fb) {
  // A map made by the program (the usual case) first.
  if (v !== null && typeof v === "object" && v.constructor === Object && v !== globalThis) {
    if (hasOwn(v, name)) return v[name];
  }
  if (isNil(v)) return null;
  if (isMap(v)) {
    if (foreign(v)) {
      if (name in v) {
        foreignName(name);
        const x = v[name];
        if (x === undefined) return null;
        if (typeof x !== "function") return x;
        // It keeps what it is, as `window.Array.from` and `requestPermission` hang off it
        // (FEEDBACK28.md, Q-341; branch feedback29-fixes).
        const call = (...a) => foreignCall(v, name, a);
        call.$page = x;
        return call;
      }
    } else if (hasOwn(v, name)) return v[name];
    if (name === "len") return B.len(v);
    if (name === "keys") return B.keys(v);
    if (name === "values") return B.values(v);
    return null;
  }
  if (v instanceof Err && (name === "error" || name === "message")) return v.error;
  if (isEvent(v)) return eventField(v, name);
  if (typeof v === "function" && v.$page) {
    foreignName(name);
    const x = v.$page[name];
    if (x === undefined) return null;
    if (typeof x !== "function") return x;
    const call = (...a) => foreignCall(v.$page, name, a);
    call.$page = x;
    return call;
  }
  const f = (fb instanceof Later ? fb.read() : fb) || (hasOwn(B, name) ? B[name] : null);
  if (typeof f === "function") return f(v);
  // As on the server (FEEDBACK26.md, Q-199; branch feedback26-fixes).
  if (name === "error" || name === "message") return null;
  fail(`${typeName(v)} has no field \`${name}\`${name === "length" && (Array.isArray(v) || typeof v === "string") ? ": `len(x)` is how many it holds" : ""}`);
}

// What `field` falls back on, read only if it does.
class Later { constructor(read) { this.read = read; } }

function index(o, i) {
  if (Array.isArray(o) && isInt(i)) {
    const n = i < 0 ? o.length + i : i;
    return n >= 0 && n < o.length ? o[n] : null;
  }
  // The page's objects: by name, or by number for lists of theirs (files, elements).
  if (foreign(o) && (typeof i === "string" || isInt(i))) return field(o, String(i), null);
  if (isMap(o) && (typeof i === "string" || isInt(i))) return hasOwn(o, String(i)) ? o[String(i)] : null;
  if (typeof o === "string" && isInt(i)) {
    // A text read by index in a loop is split once, and not at all when no
    // character takes two units (FEEDBACK25.md, Q-313; branch feedback26-fixes).
    if (o !== indexed.text) indexed = { text: o, chars: /[\ud800-\udfff]/.test(o) ? Array.from(o) : null };
    const cs = indexed.chars || o;
    const n = i < 0 ? cs.length + i : i;
    return n >= 0 && n < cs.length ? cs[n] : null;
  }
  if (isNil(o)) return null;
  // An int past 2^53 is past any list's end, and a map's key by its digits; one counted as a
  // BigInt (`W.span`) is the int it is (FEEDBACK55.md, Q-776; branch feedback55).
  if (typeof i === "bigint") return Number.isSafeInteger(Number(i)) ? index(o, Number(i)) : Array.isArray(o) || typeof o === "string" ? null : isMap(o) ? (hasOwn(o, String(i)) ? o[String(i)] : null) : fail(`cannot index ${typeName(o)} with int`);
  fail(`cannot index ${typeName(o)} with ${typeName(i)}`);
}
let indexed = { text: "", chars: null };

function callFn(f, args) {
  if (typeof f !== "function") fail(`cannot call ${typeName(f)}: it is not a function`);
  denied(f);
  // A builtin given more than it takes, as on the server (`most`; FEEDBACK55.md, Q-786; branch
  // feedback55).
  if (args.length > f.$m) fail(tooMany(f, args.length));
  const call = pageFns.get(f);
  if (call !== undefined) return call(f, args);
  return f(...args);
}

// A builtin `f` taken as a value (`let f = split`, `[split][0]`, `reduce(xs, 0, max)`), with how
// many arguments it takes and its call as the docs write it: a call of it with more says so, as
// the server's does. The rest were dropped (FEEDBACK55.md, Q-786; branch feedback55). A call
// written out is held to it as the page is made (client.rs).
function most(f, name, n, shape) {
  if (f.$m === undefined) { f.$m = n; f.$mn = name; f.$ms = shape; }
  return f;
}
// What the server's `check::too_many_args` says: `ask` given a third argument is told of `about`,
// a tool's question (`mcp::ASK_OF_THREE`; FEEDBACK56.md, the user's decision 2026-11-09; branch
// feedback56).
const tooMany = (f, got) => f.$mn === "ask" ? "`ask(message, options)` is the page's dialog; a question with choices for the person at an AI client is `about(question, choices, default)`" : `\`${f.$mn}\` takes ${f.$m} argument${f.$m === 1 ? "" : "s"}, got ${got}: ${f.$ms}${f.$mn === "sort" ? "; to sort by a key, `sort_by(xs, (x) => key)`, and `reverse` of it for the other way" : ""}`;

// `floor(a / b)` (`ceil`, `round`, `math.floor`, `math.ceil`, `math.trunc`: `f`; `g` the function)
// of two ints, in ints, as on the server's `int_div_rounded`: through the float `a / b` it was off
// past 2^53 (FEEDBACK55.md, Q-788; branch feedback55).
function idiv(f, a, b, g) {
  if (!(isInt(a) || typeof a === "bigint") || !(isInt(b) || typeof b === "bigint")) return g(div(a, b));
  if (b == 0) fail("division by zero");
  const down = f === "floor" || f === "math.floor", up = f === "ceil" || f === "math.ceil", half = f === "round";
  if (typeof a === "number" && typeof b === "number") {
    // Exact in floats: a less what is left over is a multiple of b.
    const r = a % b;
    let q = (a - r) / b + 0;
    if (r !== 0) {
      if (down && (r < 0) !== (b < 0)) q -= 1;
      else if (up && (r < 0) === (b < 0)) q += 1;
      else if (half && 2 * Math.abs(r) >= Math.abs(b)) q += (a < 0) === (b < 0) ? 1 : -1;
    }
    return q;
  }
  const x = BigInt(a), y = BigInt(b), r = x % y;
  let q = x / y;
  if (r !== 0n) {
    if (down && (r < 0n) !== (y < 0n)) q -= 1n;
    else if (up && (r < 0n) === (y < 0n)) q += 1n;
    else if (half && 2n * babs(r) >= babs(y)) q += (x < 0n) === (y < 0n) ? 1n : -1n;
  }
  return bigChecked(q);
}

// A builtin that makes a quotient of two ints whole in ints, `f(a / b)` however it is reached
// (`idiv`): which one it is (FEEDBACK55.md, Q-788; branch feedback55).
function divides(f, kind) {
  f.$div = kind;
  return f;
}
// `f(a / b)` where `f` is not written as a builtin's name (`let fl = floor`, `[ceil][0]`, a pipe):
// in ints when it is one of `floor`'s kind, as on the server (found in review).
function divCall(f, a, b) {
  return typeof f === "function" && f.$div ? idiv(f.$div, a, b, f) : callFn(f, [div(a, b)]);
}
// `m.floor(a / b)`, `m` a map holding it (the `math` module in a variable).
function mcallDiv(recv, name, fb, a, b) {
  const f = isMap(recv) && hasOwn(recv, name) ? recv[name] : null;
  return typeof f === "function" && f.$div ? idiv(f.$div, a, b, f) : mcall(recv, name, fb, [div(a, b)]);
}
// `(a / b).floor()`: the function of that name, on the quotient.
function divMethod(a, b, name, fb) {
  return typeof fb === "function" && fb.$div ? idiv(fb.$div, a, b, fb) : mcall(div(a, b), name, fb, []);
}

// `recv.name(args)`: a function stored in the map, or `name(recv, args)`.
function mcall(recv, name, fb, args) {
  if (foreign(recv) && typeof recv[name] === "function") return foreignCall(recv, name, args);
  if (typeof recv === "function" && recv.$page && typeof recv.$page[name] === "function") return foreignCall(recv.$page, name, args);
  if (isMap(recv) && hasOwn(recv, name)) return callFn(recv[name], args);
  if (typeof fb === "function") return fb(recv, ...args);
  fail(`${typeName(recv)} has no method \`${name}\``);
}

function tryv(v) {
  if (v instanceof Err) throw new Ret(v);
  return v;
}

function iter(v) {
  if (Array.isArray(v)) return v;
  // In the order set (FEEDBACK55.md, Q-875; branch feedback55).
  if (isMap(v)) return mkeys(v).map((k) => [k, v[k]]);
  if (typeof v === "string") return Array.from(v);
  if (isNil(v)) return [];
  fail(`cannot loop over ${typeName(v)}`);
}

function tyText(names, opt) {
  return names.join(" | ") + (opt ? (names.length === 1 ? "?" : " | null") : "");
}

function tyOk(v, names, opt) {
  if (opt && isNil(v)) return true;
  for (let i = 0; i < names.length; i++) if (typeIs(names[i], v)) return true;
  return false;
}

// Argument count and type checks, as on the server.
function arity(n, max, name) {
  if (n > max) fail(`\`${name}\` takes ${max} argument${max === 1 ? "" : "s"}, got ${n}`);
}
function missing(p, name) { fail(`missing argument \`${p}\` in call to \`${name}\``); }
// What a record lacks, after a mismatch: " (`email` is missing)".
const lacks = (v, names, opt) => (ownTypes ? ownTypes.lacks(v, names, opt) : "");
function argty(v, names, opt, p, name) {
  if (!tyOk(v, names, opt)) fail(`argument \`${p}\` of \`${name}\` must be ${tyText(names, opt)}, got ${typeName(v)} ${short(v)}${lacks(v, names, opt)}`);
}
function retty(v, names, opt, name, enames, eopt) {
  if (v instanceof Err) {
    if (enames && !tyOk(v.error, enames, eopt)) fail(`\`${name}\` declares errors as \`! ${tyText(enames, eopt)}\`, but returned err(${short(v.error)})`);
    return v;
  }
  if (names && !tyOk(v, names, opt)) fail(`\`${name}\` must return ${tyText(names, opt)}, but returned ${typeName(v)} ${short(v)}${lacks(v, names, opt)}`);
  return v;
}

function meta(f, name, params) {
  f.$name = name;
  f.$p = params;
  return f;
}

// ---- patterns ----------------------------------------------------------
// { k: "w" } wild, { k: "b", n } bind, { k: "l", v } literal, { k: "t", t } type,
// { k: "a", ps } alternatives, { k: "as", p, n }, { k: "ls", ps, rest },
// { k: "r", fs: [{ path, p, d, ty }] } record (d: a default thunk or null;
// ty: the field only tests a type, as in `page: int | null`, and binds its name).

function getPath(v, path) {
  let cur = v;
  for (const k of path) {
    if (!isMap(cur) || !hasOwn(cur, k)) return undefined;
    cur = cur[k];
  }
  return cur;
}

// What the last record pattern made of its value (`User as u` binds that).
let madeLast;

function matchInto(p, v, b) {
  switch (p.k) {
    case "w": return true;
    case "b": b[p.n] = v; return true;
    case "l": return eq(p.v, v);
    case "t": {
      madeLast = undefined;
      // A decimal from an int, numeric text or a JSON number.
      if (p.t === "dec") { const d = asDec(v); if (d === null) return false; madeLast = d; return true; }
      // Money from data (FEEDBACK50.md, Q-621; branch feedback50-fixes).
      if (p.t === "money") { const r = moneyFromData(v); if (r.why) return false; madeLast = r.v; return true; }
      // A record: the value made as `User(v)` makes it.
      const r = ownTypes && ownTypes.make(p.t, v);
      if (r) {
        if (r.ok) madeLast = r.v;
        return r.ok;
      }
      return typeIs(p.t, v);
    }
    case "a": {
      for (const q of p.ps) {
        const t = {};
        if (matchInto(q, v, t)) { Object.assign(b, t); return true; }
      }
      return false;
    }
    case "as": {
      if (!matchInto(p.p, v, b)) return false;
      b[p.n] = p.p.k === "t" && madeLast !== undefined ? madeLast : v;
      return true;
    }
    case "ls": {
      if (!Array.isArray(v)) return false;
      if (v.length < p.ps.length || (p.rest === undefined && v.length !== p.ps.length)) return false;
      for (let i = 0; i < p.ps.length; i++) if (!matchInto(p.ps[i], v[i], b)) return false;
      if (p.rest) b[p.rest] = v.slice(p.ps.length);
      return true;
    }
    case "r": {
      if (!isMap(v)) return false;
      for (const f of p.fs) {
        let x = getPath(v, f.path);
        if (isNil(x)) {
          if (f.d) {
            x = f.d();
            // A default is taken as written, even `int = null`.
            if (f.ty) { b[f.path[f.path.length - 1]] = x; continue; }
            if (f.as) { b[f.as] = x; continue; }
          } else if (x === undefined) return false;
          else x = null;
        }
        const before = Object.keys(b).length;
        if (!matchInto(f.p, x, b)) return false;
        if (f.ty && Object.keys(b).length === before) b[f.path[f.path.length - 1]] = f.p.k === "t" && madeLast !== undefined ? madeLast : x;
      }
      return true;
    }
  }
  return false;
}

function match(p, v) {
  const b = {};
  return matchInto(p, v, b) ? b : null;
}

// ---- the standard library (the parts that make sense in a browser) -----

const want = (ok, f, what, i, v) => { if (!ok) fail(`\`${f}\` expects ${what} as argument ${i}, got ${isNil(v) && v === undefined ? "nothing" : typeName(v)}${v instanceof Dec && what === "a number" ? FLOAT_ONLY : ""}`); };
const S = (v, f, i = 1) => { want(typeof v === "string", f, "a str", i, v); return v; };
const L = (v, f, i = 1) => { if (isNil(v)) return []; want(Array.isArray(v), f, "a list", i, v); return v; };
const I = (v, f, i = 1) => { want(isInt(v), f, "an int", i, v); return v; };
// An int, past 2^53 too (a BigInt), where the server takes any (FEEDBACK55.md, Q-776; branch
// feedback55).
const II = (v, f, i = 1) => { want(isInt(v) || typeof v === "bigint", f, "an int", i, v); return v; };
// An int as a JS number, for a count or a place: one past 2^53 is past any length, and clamped or
// refused as the server clamps or refuses it (found in review).
const IN = (v, f, i = 1) => Number(II(v, f, i));
const M = (v, f, i = 1) => { if (isNil(v)) return {}; want(isMap(v), f, "a map", i, v); return v; };
const F = (v, f, i = 2) => {
  want(typeof v === "function", f, "a function", i, v);
  denied(v);
  // The page's own function is called through one of Quillo's, which checks what it is given.
  if (pageFns.has(v)) fail(`\`${f}\` is given a function of the page: wrap it, as in (x) => f(x)`);
  return v;
};
// A number, as a float: an int past 2^53 and a whole float too, as the server's `as_f64` reads
// them (FEEDBACK55.md, Q-775 and Q-776; branch feedback55).
const N = (v, f, i = 1) => { const x = numOf(v); want(x !== null, f, "a number", i, v); return x; };

// Calls a function given to a builtin; it must not wait on the server.
// Every caller passes one argument (two for `reduce`).
function cb(f, who, a, b) {
  // A builtin given more than it takes: `reduce(xs, 0, max)` (FEEDBACK55.md, Q-786).
  if (arguments.length - 2 > f.$m) fail(tooMany(f, arguments.length - 2));
  const r = arguments.length > 3 ? f(a, b) : f(a);
  if (r instanceof Promise) fail(`\`${who}\` cannot call a function that waits, on the server or the page (\`js\` in it waits): use a \`for\` loop instead`);
  return r;
}

function clampRange(len, start, end) {
  const fix = (i) => Math.min(Math.max(i < 0 ? len + i : i, 0), len);
  const s = fix(start);
  const e = end === undefined ? len : fix(end);
  return [s, Math.max(e, s)];
}

function sortValues(items, name) {
  let bad = null;
  const sorted = items.slice().sort((a, b) => {
    const c = cmp(a[0], b[0]);
    if (c === null) { bad = [typeName(a[0]), typeName(b[0]), decHint(a[0], b[0])]; return 0; }
    return c;
  });
  if (bad) fail(`\`${name}\` cannot compare ${bad[0]} with ${bad[1]}${bad[2]}`);
  return sorted.map((x) => x[1]);
}

function extreme(l, name, want) {
  let best = null;
  for (const x of L(l, name)) {
    if (best === null) { best = x; continue; }
    const c = cmp(x, best);
    if (c === null) fail(`\`${name}\` cannot compare ${typeName(x)} with ${typeName(best)}${decHint(x, best)}`);
    if (c === want) best = x;
  }
  return best;
}

function coerce(t, s) {
  s = s.trim();
  if (t === "int") return /^[+-]?\d+$/.test(s) ? parseInt(s, 10) : null;
  // `nan` and `inf` as the server reads them (FEEDBACK25.md, Q-309; branch feedback26-fixes).
  if (t === "float" || t === "num") {
    const named = /^([+-]?)(inf|infinity|nan)$/i.exec(s);
    if (named) return named[2][0].toLowerCase() === "n" ? NaN : named[1] === "-" ? -Infinity : Infinity;
    // Digits as Rust reads a float, and a float: `0x10`, `0b11` and `0o17` are none, which
    // JavaScript's Number() took (FEEDBACK55.md, Q-775 and Q-782; branch feedback55).
    return /^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/.test(s) ? fl(Number(s)) : null;
  }
  if (t === "dec") return decParse(s, false);
  if (t === "date") return isDay(s) ? s : null;
  return null;
}

// A value as JSON data. `wire`: for the program's own server (a server
// fn's arguments), decimals and large ints tagged ({"$dec": "12.50"}) so
// they arrive as they are; else a decimal is its text, as the server
// writes it, and a large int its digits (`writeJSON` writes them).
// A whole float is written as the server writes it, `2.0` (`writeJSON` writes it), so
// that it is read back as a float on either side (FEEDBACK55.md, Q-775; branch feedback55);
// `wire` "tools": as a number, for the developer's tools.
function toJSON(v, wire) {
  if (isNil(v)) return null;
  if (v instanceof Err) return { error: toJSON(v.error, wire) };
  if (Array.isArray(v)) return v.map((x) => toJSON(x, wire));
  // Its markup, as the server writes it (FEEDBACK55.md, Q-779; branch feedback55).
  if (v instanceof Html) return htmlText(v);
  if (typeof v === "function") return repr(v);
  if (v instanceof Dec) return wire ? { $dec: decText(v) } : decText(v);
  if (typeof v === "bigint") return wire ? { $int: String(v) } : rawNum(String(v));
  if (v instanceof Fl) return wire === "tools" ? v.f : rawNum(flJSON(v.f));
  if (isMap(v)) {
    // In the order set (FEEDBACK55.md, Q-875; branch feedback55).
    const ord = ordOf(v), ks = ord === undefined ? Object.keys(v) : mkeys(v);
    // A map of one key that starts with `$` goes to the server as `{"$": [key, value]}`, which it
    // reads back as that map: a visitor's key made `{"$int": "5"}` an int there (FEEDBACK55.md,
    // Q-875's review; branch feedback55); one that keeps a list too (its second review).
    if (wire && ks.length === 1 && ks[0].charCodeAt(0) === 36) return { $: [ks[0], toJSON(v[ks[0]], wire)] };
    if (ord !== undefined) return new Kept(ks, ks.map((k) => toJSON(v[k], wire)));
    const o = {};
    for (const k of ks) putKey(o, k, toJSON(v[k], wire));
    return o;
  }
  if (typeof v === "number" && !isFinite(v)) return null;
  return v;
}
// A whole float's JSON text, as the server's: `.0` after its digits (`-0.0`), and from 1e21 an
// exponent, as JavaScript writes it (FEEDBACK55.md, Q-775 and Q-781; branch feedback55).
const flJSON = (f) => (Object.is(f, -0) ? "-0.0" : (f = String(f), f.includes("e") ? f : f + ".0"));
// A number's JSON text `t` (a large int's digits, a whole float's), written as it is: JSON.rawJSON
// where the browser has it; else marked with letters the page alone knows, which `unmark` takes
// out of the text. A fixed mark ("\u0000int:") was taken out of a visitor's text too: `?q=%00int:5`
// reached a server fn as the int 5, and `a"\0int:5` made its JSON broken (FEEDBACK55.md, Q-875's
// review; branch feedback55).
let rawOk = null;
const rawNum = (t) => ((rawOk ?? (rawOk = typeof JSON.rawJSON === "function")) ? JSON.rawJSON(t) : marks().num + t);
// The marks of a page, made once at random: a number's text (`num`), a map in order (`kept`); and
// how JSON writes them (`numText`, `keptText`).
let marksMade = null;
function marks() {
  if (marksMade === null) {
    const r = randomTag();
    marksMade = { num: "\u0000n" + r + ":", numText: "\"\\u0000n" + r + ":", numRe: new RegExp("\"\\\\u0000n" + r + ":(-?[\\d.e+]+)\"", "g"),
      kept: "\u0000k" + r + ":", keptText: "\"\\u0000k" + r + ":", keptRe: new RegExp("\"\\\\u0000k" + r + ":(\\d+)\"", "g") };
  }
  return marksMade;
}
// What `rawNum` marked written as it is.
const unmark = (t) => (marksMade !== null && t.includes(marksMade.numText) ? t.replace(marksMade.numRe, "$1") : t);
// Letters no text holds, by chance.
function randomTag() {
  return typeof crypto !== "undefined" && crypto.getRandomValues ? Array.from(crypto.getRandomValues(new Uint32Array(4)), (x) => x.toString(36)).join("") : String(Math.random()).slice(2);
}
// A map that keeps its order, as `toJSON` makes it: its keys and their values. JSON.stringify would
// put its array indexes first, so it writes a mark in its place, which `writeJSON` takes out for
// the map's text, keys in order (FEEDBACK55.md, Q-875; branch feedback55).
class Kept {
  constructor(ks, vs) { this.ks = ks; this.vs = vs; }
  toJSON() {
    if (keptOut === null) return [this.ks, this.vs];
    keptOut.push(this);
    return marks().kept + (keptOut.length - 1);
  }
}
// The maps in order met while `writeJSON` writes.
let keptOut = null;
// JSON text of what `toJSON` made, `indent` spaces a level when given.
function writeJSON(j, indent) {
  const was = keptOut, out = [];
  keptOut = out;
  try {
    const t = JSON.stringify(j, null, indent);
    return unmark(out.length ? keptIn(t, out, indent) : t);
  } finally {
    keptOut = was;
  }
}
// Text `t` with each map in order written where its mark is.
const keptIn = (t, out, indent) => t.replace(marksMade.keptRe, (m, n, at) => keptText(out[n], out, indent, indent ? lineIndent(t, at) : ""));
function lineIndent(t, at) {
  const s = t.lastIndexOf("\n", at) + 1;
  let e = s;
  while (t.charCodeAt(e) === 32) e++;
  return t.slice(s, e);
}
// Map k's text, as JSON.stringify writes an object, its lines `base` in.
function keptText(k, out, indent, base) {
  const n = k.ks.length;
  if (n === 0) return "{}";
  const gap = indent ? " ".repeat(indent) : "", sep = indent ? ",\n" + base + gap : ",", colon = indent ? ": " : ":";
  const parts = new Array(n);
  for (let i = 0; i < n; i++) {
    const key = k.ks[i], x = k.vs[i];
    let t;
    if (x === null) t = "null";
    else if (x === true) t = "true";
    else if (x === false) t = "false";
    else if (typeof x === "number") t = String(x);
    else {
      t = JSON.stringify(x, null, indent);
      if (typeof x === "object" && t.includes(marksMade.keptText)) t = keptIn(t, out, indent);
      if (indent) t = t.replace(/\n/g, "\n" + base + gap);
    }
    parts[i] = (isIdx(key) ? "\"" + key + "\"" : JSON.stringify(key)) + colon + t;
  }
  return indent ? "{\n" + base + gap + parts.join(sep) + "\n" + base + "}" : "{" + parts.join(",") + "}";
}
// JSON text of a value.
const jsonText = (v, indent) => writeJSON(toJSON(v), indent);
// What a visitor may read of `v` as JSON (`json.stringify`, an attribute's): a builtin's error
// says `internal error`, as the server's `hidden` has it, unless the server shows errors
// (FEEDBACK55.md, Q-773; branch feedback55).
// The value itself when it holds no such error, as the server's `holds_hidden_error` finds.
const visible = (v) => (ERRORS_SHOWN || !holdsHidden(v) ? v : hiddenIn(v));
const holdsHidden = (v) => (v instanceof Err ? !v.own || holdsHidden(v.error) : Array.isArray(v) ? v.some(holdsHidden) : isMap(v) && Object.keys(v).some((k) => holdsHidden(v[k])));
function hiddenIn(v) {
  if (v instanceof Err) return v.own ? new Err(hiddenIn(v.error), true) : new Err("internal error");
  if (Array.isArray(v)) return v.map(hiddenIn);
  // In the order set (FEEDBACK55.md, Q-875; branch feedback55).
  if (isMap(v)) { const o = {}, ks = mkeys(v); for (const k of ks) putKey(o, k, hiddenIn(v[k])); return ordOf(v) === undefined ? o : ordered(o, ks); }
  return v;
}

const B = {};

// ---- streams (src/stream.rs) -------------------------------------------
// What a streamed answer sends, given to f as it arrives: an event's data
// (JSON, parsed) and its name, a line of JSON, or a piece of text.
// `at`: where an event stream's last id and retry are kept, for `listen` to connect again from
// (FEEDBACK36.md, Q-444; branch feedback36-39).
async function readStream(res, f, who, signal, at) {
  const type = (res.headers.get("content-type") || "").toLowerCase();
  const kind = type.startsWith("text/event-stream") ? "events" : /ndjson|jsonl|json-seq/.test(type) ? "lines" : "text";
  // Numbers as the server reads them, floats and ints past 2^53 too (found in review).
  // Deeper than 1,000 levels, its text, as on the server (FEEDBACK55.md, Q-875's review; branch
  // feedback55).
  const parse = (t) => { try { return readJSON(t, true); } catch (_) { return t; } };
  const give = async (args) => {
    const n = Array.isArray(f.$p) ? f.$p.length : f.length;
    const r = f(...args.slice(0, Math.max(n, 1)));
    return r instanceof Promise ? await r : r;
  };
  const reader = res.body.getReader();
  const dec = new TextDecoder();
  let buf = "";
  const pieces = (end) => {
    const out = [];
    if (kind === "text") { if (buf) out.push([buf]); buf = ""; return out; }
    if (kind === "lines") {
      const lines = buf.split("\n");
      buf = end ? "" : lines.pop();
      for (const l of lines) if (l.trim()) out.push([parse(l.replace(/\r$/, ""))]);
      return out;
    }
    buf = buf.replace(/\r\n/g, "\n");
    const blocks = buf.split("\n\n");
    buf = end ? "" : blocks.pop();
    for (const b of blocks) {
      const data = [];
      let name = null;
      for (const l of b.split("\n")) {
        const i = l.indexOf(":");
        const field = i < 0 ? l : l.slice(0, i);
        let v = i < 0 ? "" : l.slice(i + 1);
        if (v[0] === " ") v = v.slice(1);
        if (field === "data") data.push(v);
        else if (field === "event") name = v;
        else if (field === "id" && at && !v.includes("\0")) at.id = v;
        else if (field === "retry" && at && /^\d+$/.test(v)) at.retry = +v;
      }
      if (data.length) out.push([parse(data.join("\n")), name]);
    }
    return out;
  };
  for (;;) {
    const { value, done } = await reader.read();
    if (value) buf += dec.decode(value, { stream: !done });
    for (const args of pieces(done)) {
      if (signal && signal.aborted) return;
      if ((await give(args)) === false) { reader.cancel().catch(() => {}); return; }
    }
    if (done) return;
  }
}

// A request's body: text as it is, a file (an input's, a Blob) as its
// bytes, with its own type, and other values as JSON.
function bodyInit(init, body) {
  const has = (k) => Object.keys(init.headers).some((h) => h.toLowerCase() === k);
  if (typeof Blob !== "undefined" && body instanceof Blob) init.body = body;
  else if (typeof body === "string") {
    init.body = body;
    if (!has("content-type")) init.headers["content-type"] = "text/plain; charset=utf-8";
  } else if (body instanceof Html && body.text !== undefined) {
    init.body = body.text;
    if (!has("content-type")) init.headers["content-type"] = "text/html; charset=utf-8";
  } else {
    init.body = jsonText(body);
    if (!has("content-type")) init.headers["content-type"] = "application/json";
  }
}

// `http.get(url, headers, options)` and the rest, in the browser: the
// request goes from the page, as `fetch` makes it (the other site must
// allow this one, by CORS; cookies go only to this site), and gives
// `{ status, ok, headers, body, json }` as on the server, or an error value
// when it could not be made. Options: `redirects: 0` keeps a redirect from
// being followed (it is then an error value, as the browser shows it no
// further); `public_only` and `cache` are the server's (src/fetch.rs);
// `timeout`: seconds the call may take, as on the server. Any other option
// is an error (FEEDBACK5.md, Q-049; branch stays-fixes).
// `at`: the place of the URL among the call's arguments.
const HTTP_OPTIONS = ["redirects", "public_only", "cache", "timeout"];
async function httpCall(fname, method, url, body, headers, opts, at) {
  S(url, fname, at);
  headers = M(headers, fname, at + (fname === "http.get" || fname === "http.delete" ? 1 : 2));
  opts = M(opts, fname, at + (fname === "http.get" || fname === "http.delete" ? 2 : 3));
  // The first one set named, as on the server (FEEDBACK55.md, Q-875; branch feedback55).
  for (const k of mkeys(opts)) if (!HTTP_OPTIONS.includes(k)) fail(`\`${fname}\`: unknown option \`${k}\` (the options are ${HTTP_OPTIONS.join(", ")})`);
  const secs = numOf(opts.timeout);
  if (!isNil(opts.timeout) && !(secs !== null && secs > 0 && secs <= 300)) fail(`\`${fname}\`: \`timeout\` is seconds, more than 0 and up to 300, got ${display(opts.timeout)}`);
  if (/^[a-z][a-z0-9+.-]*:/i.test(url) && !/^https?:/i.test(url)) return new Err(`http: not an http(s) URL: ${url}`);
  const init = { method, headers: {} };
  for (const k of Object.keys(headers)) init.headers[k] = display(headers[k]);
  if (!isNil(body)) bodyInit(init, body);
  if (opts.redirects === 0) init.redirect = "manual";
  if (!isNil(secs) && typeof AbortSignal !== "undefined" && AbortSignal.timeout) init.signal = AbortSignal.timeout(Math.max(1, Math.round(secs * 1000)));
  let res;
  try { res = await wireFetch(withBase(url), init); } catch (e) {
    if (e && e.name === "TimeoutError") return new Err(`http: ${url} took more than ${secs} s`);
    // A browser says only "Failed to fetch" or "Load failed": for another site, what it may be
    // (FEEDBACK33.md, Q-411; branch feedback27-35-fixes).
    let other = null;
    try { const u = new URL(withBase(url), location.href); if (u.origin !== location.origin) other = u; } catch (_) {}
    if (other && e && e.name === "TypeError") return new Err(`http: no answer from ${other.host}: it may not allow this page (CORS), or be plain http, or be unreachable; a server fn can call it from the server (${e.message})`);
    return new Err("http: " + (e && e.message));
  }
  if (res.type === "opaqueredirect") return new Err(`http: ${url} redirects, and redirects: 0 does not follow it`);
  let text;
  try { text = await res.text(); } catch (e) { return new Err("http: " + e.message); }
  const hs = {};
  res.headers.forEach((v, k) => { hs[k] = v; });
  let json = null;
  // JSON deeper than 1,000 levels is none, as on the server (FEEDBACK55.md, Q-875's review; branch
  // feedback55).
  try { json = fromJSON(readJSON(text, true)); } catch (e) { json = null; }
  return { status: res.status, ok: res.ok, headers: hs, body: text, json };
}

B.http = {
  get: meta((url, headers, opts) => httpCall("http.get", "GET", url, null, headers, opts, 1), "http.get", null),
  delete: meta((url, headers, opts) => httpCall("http.delete", "DELETE", url, null, headers, opts, 1), "http.delete", null),
  post: meta((url, body, headers, opts) => httpCall("http.post", "POST", url, body, headers, opts, 1), "http.post", null),
  put: meta((url, body, headers, opts) => httpCall("http.put", "PUT", url, body, headers, opts, 1), "http.put", null),
  patch: meta((url, body, headers, opts) => httpCall("http.patch", "PATCH", url, body, headers, opts, 1), "http.patch", null),
  request: meta((method, url, body, headers, opts) => httpCall("http.request", S(method, "http.request").toUpperCase(), url, body, headers, opts, 2), "http.request", null),
};

// `receive(url, f, { method, body, headers })` in a handler: sends the
// request and calls f with what the answer streams, until it ends (or f
// gives false). An error value if it could not be had.
B.receive = meta(async (url, f, opts) => {
  S(url, "receive");
  F(f, "receive", 2);
  opts = M(opts, "receive", 3);
  const init = { method: opts.method ? S(opts.method, "receive").toUpperCase() : (isNil(opts.body) ? "GET" : "POST"), headers: {} };
  if (isMap(opts.headers)) for (const k of Object.keys(opts.headers)) init.headers[k] = display(opts.headers[k]);
  if (!isNil(opts.body)) bodyInit(init, opts.body);
  let res;
  try { res = await wireFetch(withBase(url), init); } catch (e) { return new Err("receive: " + e.message); }
  if (!res.ok) return new Err(`receive: ${res.status} ${await res.text().catch(() => "")}`.trim());
  try { await readStream(res, f, "receive"); } catch (e) { if (e instanceof QuilloError) throw e; return new Err("receive: " + e.message); }
  return null;
}, "receive", null);

// `listen(url, f)` in a component: reads the stream at url (a GET),
// calling f with what it sends, for as long as the code that asked keeps
// asking (an effect that runs again without asking, or the component
// leaving the page, stops it). Dropped connections are opened again
// (after a second, then longer); a stream that ends is not.
B.listen = meta((url, f) => {
  // In a component's code, or a handler or `@mount` of one, before or after a wait, where it lasts
  // as long as the component, as `every` does: it checked `Owner` alone, which a handler never has
  // (FEEDBACK61.md, Q-901; branch feedback61).
  const own = Owner || Acting;
  if (!own) fail("`listen` works in a component");
  if (own.dead) return null;
  S(url, "listen");
  F(f, "listen", 2);
  const ctl = new AbortController();
  (own.clean || (own.clean = [])).push(() => ctl.abort());
  (async () => {
    let wait = 1000;
    // The last event id and the server's `retry`: connecting again, the stream goes on from
    // there, as `EventSource` does, where it started over (FEEDBACK36.md, Q-444; branch
    // feedback36-39).
    const at = { id: null, retry: null };
    while (!ctl.signal.aborted) {
      try {
        const headers = { accept: "text/event-stream, application/x-ndjson, */*" };
        if (at.id) headers["last-event-id"] = at.id;
        const res = await wireFetch(withBase(url), { signal: ctl.signal, headers });
        if (!res.ok) throw new Error(`${res.status}`);
        wait = 1000;
        await readStream(res, f, "listen", ctl.signal, at);
        return;
      } catch (e) {
        if (ctl.signal.aborted) return;
        if (e instanceof QuilloError) return report(e);
        await new Promise((r) => setTimeout(r, at.retry ?? wait));
        wait = Math.min(wait * 2, 30000);
      }
    }
  })();
  return null;
}, "listen", null);

// `every(ms, f)` in a component: calls f every ms milliseconds for as long
// as the code that asked keeps asking. `if running { every(100, step) }`
// runs again when `running` changes, and stops it when it is false; the
// component leaving the page stops it too.
// `run_after(ms, f)`: once, gone with the component (FEEDBACK28.md, Q-349; branch
// feedback29-fixes).
B.run_after = meta((ms, f) => {
  ms = N(ms, "run_after");
  F(f, "run_after", 2);
  const own = Owner || Acting;
  if (own && own.dead) return null;
  const id = setTimeout(() => {
    try {
      const r = f();
      if (r instanceof Promise) r.catch(report);
    } catch (e) {
      report(e);
    }
  }, Math.max(ms, 0));
  if (own) (own.clean || (own.clean = [])).push(() => clearTimeout(id));
  return null;
}, "run_after", null);
// `page_updated()`: waits until the page shows what was just set.
B.page_updated = meta(() => new Promise((done) => { if (flushing && effectQueue.length) flush(); queueMicrotask(() => done(null)); }), "page_updated", null);
// `js_has("a.b")`: whether the page has it, without calling it (FEEDBACK28.md, Q-341; branch
// feedback29-fixes).
B.js_has = meta((name) => {
  let o = globalThis;
  for (const p of S(name, "js_has").split(".")) {
    if (o === null || o === undefined || !(typeof o === "object" || typeof o === "function") || !(p in o)) return false;
    o = o[p];
  }
  return o !== undefined;
}, "js_has", null);
// `cursor_index(s, n)`: a field's cursor counts UTF-16 units, and `slice` counts characters
// (FEEDBACK28.md, Q-344; branch feedback29-fixes).
B.cursor_index = meta((s, n) => Array.from(S(s, "cursor_index").slice(0, Math.max(IN(n, "cursor_index", 2), 0))).length, "cursor_index", null);
// How long, in ms, `every` waits for a run that does not answer, at the least two gaps.
const EVERY_STALLED = 30000;
B.every = meta((ms, f) => {
  // In a component, or a handler or `@mount` of one, as `run_after` (FEEDBACK28.md, Q-349;
  // branch feedback29-fixes).
  const own = Owner || Acting;
  if (!own) fail("`every` works in a component");
  if (own.dead) return null;
  ms = N(ms, "every");
  F(f, "every", 2);
  // A tick that finds the last run still waiting does nothing, and the next tries again: as it was,
  // a poll slower than its timer had each answer come after a newer call, and showed none. A call
  // that never answers would stop the timer for good: after EVERY_STALLED the run is given up on,
  // and its answer, if it comes, frees nothing (FEEDBACK55.md, Q-800; branch feedback55).
  const stalled = Math.max(EVERY_STALLED, ms * 2);
  let waiting = null, since = 0;
  const id = setInterval(() => {
    if (waiting !== null && performance.now() - since < stalled) return;
    try {
      const r = f();
      if (r instanceof Promise) {
        waiting = r;
        since = performance.now();
        const done = () => { if (waiting === r) waiting = null; };
        r.then(done, (e) => { done(); report(e); });
      }
    } catch (e) {
      report(e);
    }
  }, Math.max(ms, 1));
  (own.clean || (own.clean = [])).push(() => clearInterval(id));
  return null;
}, "every", null);

// `parallel(f, g, ...)`: calls them at once (server calls overlap); their
// results, in order.
B.parallel = meta(async (...fs) => {
  if (fs.length === 1 && Array.isArray(fs[0])) fs = fs[0];
  // Each function is a run of its own, so the last one called is the latest call of a state they
  // both set; under the run that called them, whose calls their plain sets leave alone (see
  // `claimedBy`; FEEDBACK55.md, Q-800; branch feedback55).
  const run = Run;
  let calls;
  try {
    calls = fs.map((f, i) => { Run = { held: null, up: run || undefined, nav: Nav.n }; return F(f, "parallel", i + 1)(); });
  } finally {
    Run = run;
  }
  return Promise.all(calls);
}, "parallel", null);
B.print = meta((...a) => { console.log(a.map(display).join(" ")); return null; }, "print", null);
// HTML made from text (by raw or markdown) is that text, as on the server.
B.error = meta((v) => (v instanceof Err ? v.error : null), "error", null);
B.str = meta((v) => display(v), "str", null);
B.int = meta((v) => {
  if (isInt(v) || typeof v === "bigint") return v;
  const f = v instanceof Fl ? v.f : v;
  // Past an int's range, as on the server (FEEDBACK16.md, Q-111; branch feedback20-fixes): -2^63
  // is one, and past 2^53 the int is exact (FEEDBACK55.md, Q-776; branch feedback55).
  if (typeof f === "number" && isFinite(f) && (f < -(2 ** 63) || f >= 2 ** 63)) return new Err(`${display(v)} is too large for an int`);
  // The whole part (toward zero).
  if (v instanceof Dec) { const w = v.m / p10(v.s); return w < I64_MIN || w > I64_MAX ? new Err(`${decText(v)} is too large for an int`) : intOf(w); }
  if (typeof f === "number" && isFinite(f)) { const t = Math.trunc(f) + 0; return Number.isSafeInteger(t) ? t : BigInt(t); }
  if (typeof v === "boolean") return v ? 1 : 0;
  if (typeof v === "string") {
    // By its digits: past 2^53 a number has lost them (FEEDBACK25.md, Q-301; branch feedback26-fixes).
    const t = v.trim();
    if (!/^[+-]?\d+$/.test(t)) return new Err(`not an int: ${JSON.stringify(v)}`);
    const b = BigInt(t);
    return b < I64_MIN || b > I64_MAX ? new Err(`not an int: ${JSON.stringify(v)}`) : intOf(b);
  }
  return new Err(`cannot convert ${typeName(v)} to int`);
}, "int", null);
B.float = meta((v) => {
  // A float, whole too (FEEDBACK55.md, Q-775; branch feedback55).
  if (typeof v === "number") return fl(v);
  if (v instanceof Fl) return v;
  if (v instanceof Dec) return fl(Number(decText(v)));
  if (typeof v === "bigint") return fl(Number(v));
  if (typeof v === "string") { const n = coerce("float", v); return n === null ? new Err(`not a number: ${JSON.stringify(v)}`) : n; }
  return new Err(`cannot convert ${typeName(v)} to float`);
}, "float", null);
B.len = meta((v) => {
  if (typeof v === "string") return Array.from(v).length;
  if (Array.isArray(v)) return v.length;
  if (isMap(v)) return Object.keys(v).length;
  if (isNil(v)) return 0;
  // Of its markup, as on the server (FEEDBACK55.md, Q-779; branch feedback55).
  if (v instanceof Html) return Array.from(htmlText(v)).length;
  fail(`\`len\` expects a str, list or map, got ${typeName(v)}`);
}, "len", null);
B.type_of = meta((v) => typeName(v), "type_of", null);
// `dec(x)`: a decimal from text (exactly), an int, or a float (through its
// shortest text, so dec(0.1) is 0.1m).
B.dec = meta((v) => {
  if (v instanceof Dec) return v;
  if (Number.isSafeInteger(v) || typeof v === "bigint") return new Dec(BigInt(v), 0);
  if (typeof v === "number" || v instanceof Fl) return decFromFloat(numOf(v)) ?? new Err("cannot convert float to dec: it is too large, or not a number");
  if (typeof v === "string") return decParse(v.trim(), false) ?? new Err(`not a decimal: ${JSON.stringify(v)}`);
  return new Err(`cannot convert ${typeName(v)} to dec`);
}, "dec", null);
B.upper = meta((s) => S(s, "upper").toUpperCase(), "upper", null);
B.lower = meta((s) => S(s, "lower").toLowerCase(), "lower", null);
// An address on this site, or null, as on the server (FEEDBACK31.md, Q-390; branch
// feedback27-35-fixes).
B.local_path = meta((s) => {
  if (typeof s !== "string") return null;
  s = s.replace(/[\t\n\r]/g, "").replace(/\\/g, "/").replace(/^[\x00-\x20]+|[\x00-\x20]+$/g, "");
  if (!s.startsWith("/") || s.startsWith("//") || /[\x00-\x1f\x7f]/.test(s) || /^\/%(2f|5c)/i.test(s)) return null;
  return s;
}, "local_path", null);
// One Unicode form, as on the server (FEEDBACK31.md, Q-394; branch feedback27-35-fixes).
B.normalize = meta((s, form) => {
  S(s, "normalize");
  const f = isNil(form) ? "NFC" : typeof form === "string" ? form.toUpperCase() : fail(`\`normalize\` expects a form ("NFC", "NFD", "NFKC" or "NFKD") as argument 2, got ${typeName(form)}`);
  if (!["NFC", "NFD", "NFKC", "NFKD"].includes(f)) fail(`\`normalize\`: the form is "NFC", "NFD", "NFKC" or "NFKD", got ${JSON.stringify(f)}`);
  return s.normalize(f);
}, "normalize", null);
// The server's set, Unicode's White_Space: JavaScript's trim takes U+FEFF too, and leaves U+0085
// (FEEDBACK55.md, Q-778; branch feedback55).
B.trim = meta((s) => rsTrim(S(s, "trim")), "trim", null);
// What `||` does in JavaScript, for text (FEEDBACK19.md, Q-151; branch feedback19-fixes).
B.filled = (x) => (x === "" ? null : x);
B.split = meta((s, sep = " ") => { S(s, "split"); S(sep, "split", 2); return sep === "" ? Array.from(s) : s.split(sep); }, "split", null);
B.join = meta((l, sep = "") => L(l, "join").map(display).join(S(sep, "join", 2)), "join", null);
B.contains = meta((c, x) => {
  if (typeof c === "string") return c.includes(display(x));
  if (Array.isArray(c)) return c.some((y) => eq(y, x));
  if (isMap(c)) return hasOwn(c, display(x));
  if (isNil(c)) return false;
  fail(`\`contains\` expects a str, list or map, got ${typeName(c)}`);
}, "contains", null);
B.starts_with = meta((s, p) => S(s, "starts_with").startsWith(S(p, "starts_with", 2)), "starts_with", null);
B.ends_with = meta((s, p) => S(s, "ends_with").endsWith(S(p, "ends_with", 2)), "ends_with", null);
// An empty pattern is found before each character and at the end, as Rust's replace finds it, a
// character past U+FFFF whole (FEEDBACK55.md, Q-778; branch feedback55).
B.replace = meta((s, a, b) => {
  S(s, "replace"); S(a, "replace", 2); S(b, "replace", 3);
  if (a !== "") return s.split(a).join(b);
  const cs = Array.from(s);
  return b + cs.join(b) + (cs.length ? b : "");
}, "replace", null);
B.slice = meta((v, start, end) => {
  start = IN(start, "slice", 2);
  if (end !== undefined) end = IN(end, "slice", 3);
  if (Array.isArray(v)) { const [s, e] = clampRange(v.length, start, end); return v.slice(s, e); }
  if (typeof v === "string") { const cs = Array.from(v); const [s, e] = clampRange(cs.length, start, end); return cs.slice(s, e).join(""); }
  fail(`\`slice\` expects a list or str, got ${typeName(v)}`);
}, "slice", null);
B.map = meta((l, f) => {
  F(f, "map");
  const a = L(l, "map"), out = new Array(a.length);
  for (let i = 0; i < a.length; i++) out[i] = cb(f, "map", a[i]);
  return out;
}, "map", null);
B.filter = meta((l, f) => {
  F(f, "filter");
  const a = L(l, "filter"), out = [];
  for (let i = 0; i < a.length; i++) {
    const r = cb(f, "filter", a[i]);
    if (r instanceof Err) return r;
    if (truthy(r)) out.push(a[i]);
  }
  return out;
}, "filter", null);
B.find = meta((l, f) => { F(f, "find"); for (const x of L(l, "find")) { const r = cb(f, "find", x); if (r instanceof Err) return r; if (truthy(r)) return x; } return null; }, "find", null);
B.any = meta((l, f) => { F(f, "any"); for (const x of L(l, "any")) { const r = cb(f, "any", x); if (r instanceof Err) return r; if (truthy(r)) return true; } return false; }, "any", null);
B.all = meta((l, f) => { F(f, "all"); for (const x of L(l, "all")) { const r = cb(f, "all", x); if (r instanceof Err) return r; if (!truthy(r)) return false; } return true; }, "all", null);
B.reduce = meta((l, acc, f) => { F(f, "reduce", 3); for (const x of L(l, "reduce")) acc = cb(f, "reduce", acc, x); return acc; }, "reduce", null);
// Text is no number: `+` joined it, "012.505.00" (FEEDBACK55.md, Q-783; branch feedback55).
B.sum = meta((l) => { let a = 0, i = 0; for (const x of L(l, "sum")) { if (typeof x === "string") fail(`\`sum\` adds numbers; got the str ${repr(x)} at item ${i}: use \`float(...)\` or \`dec(...)\` first`); a = add(a, x); i++; } return a; }, "sum", null);
B.min = meta((l) => extreme(l, "min", -1), "min", null);
B.max = meta((l) => extreme(l, "max", 1), "max", null);
B.sort = meta((l) => sortValues(L(l, "sort").map((v) => [v, v]), "sort"), "sort", null);
B.sort_by = meta((l, f) => { F(f, "sort_by"); return sortValues(L(l, "sort_by").map((v) => [cb(f, "sort_by", v), v]), "sort_by"); }, "sort_by", null);
B.reverse = meta((v) => {
  if (Array.isArray(v)) return v.slice().reverse();
  if (typeof v === "string") return Array.from(v).reverse().join("");
  fail(`\`reverse\` expects a list or str, got ${typeName(v)}`);
}, "reverse", null);
B.first = meta((l) => { l = L(l, "first"); return l.length ? l[0] : null; }, "first", null);
B.last = meta((l) => { l = L(l, "last"); return l.length ? l[l.length - 1] : null; }, "last", null);
B.push = meta((l, x = null) => L(l, "push").concat([x]), "push", null);
B.range = meta((a, b, c) => {
  if (c !== undefined) {
    const [from, to, step] = W.stepped(a, b, c), out = [];
    if ((to - from) / step > 10000000) fail("range is too large");
    for (let i = from; step > 0 ? i < to : i > to; i += step) out.push(intOf(i));
    return out;
  }
  return b === undefined ? intRange(0, II(a, "range")) : intRange(II(a, "range"), II(b, "range", 2));
}, "range", null);
B.enumerate = meta((l) => L(l, "enumerate").map((v, i) => [i, v]), "enumerate", null);
// In the order set, as the rest that read or make a map (FEEDBACK55.md, Q-875; branch feedback55).
B.keys = meta((m) => { m = M(m, "keys"); const ord = ordOf(m); return ord === undefined ? Object.keys(m) : ord.slice(0, m[ORDN]); }, "keys", null);
B.values = meta((m) => { m = M(m, "values"); return mkeys(m).map((k) => m[k]); }, "values", null);
// An error is no key: it is given back, as on the server (FEEDBACK55.md, Q-784; branch feedback55).
B.get = meta((m, k, d = null) => { m = M(m, "get"); if (k instanceof Err) return k; k = keyText(k); return hasOwn(m, k) ? m[k] : d; }, "get", null);
B.has = meta((m, k) => { m = M(m, "has"); return k instanceof Err ? k : hasOwn(m, keyText(k)); }, "has", null);
B.set = meta((m, k, v = null) => {
  if (Array.isArray(m)) {
    II(k, "set", 2);
    const at = typeof k === "bigint" ? -1 : k < 0 ? m.length + k : k;
    if (at < 0 || at >= m.length) fail(`\`set\`: index ${k} is outside a list of ${m.length}`);
    const l = m.slice();
    l[at] = v;
    return l;
  }
  m = M(m, "set");
  if (k instanceof Err) return k;
  const key = keyText(k);
  // A map that keeps its order, or a key JavaScript would put first, in its place (FEEDBACK55.md,
  // Q-875; branch feedback55).
  return ordOf(m) === undefined && topOf(m) === undefined && !isIdx(key) ? putKey({ ...m }, key, v) : mput({ ...m }, key, v);
}, "set", null);
B.remove = meta((c, k) => {
  if (Array.isArray(c)) return c.filter((v) => !eq(v, k));
  const m = { ...M(c, "remove") };
  if (k instanceof Err) return k;
  return mdel(m, keyText(k));
}, "remove", null);
B.merge = meta((a, b) => mmerge(M(a, "merge"), M(b, "merge", 2)), "merge", null);
// Spreads, which the parser makes calls of (FEEDBACK3.md, Q-036; branch
// feedback6-fixes): null spreads nothing; anything else that is not a map
// (a list) is said as the spread it is.
B.__merge = meta((...ms) => {
  let out = null;
  for (const m of ms) {
    if (isNil(m)) continue;
    if (!isMap(m)) fail(`\`{...x}\` spreads a map (or null) into a map, but x is ${typeName(m)}`);
    // A key at a time: `Object.assign` took a `__proto__` key from JSON for the prototype
    // (FEEDBACK51.md, from reading; branch feedback51-fixes). The first map whole, as a spread
    // takes it, its order shared; the rest a key at a time in their order (FEEDBACK55.md, Q-875;
    // branch feedback55).
    if (out === null) out = { ...m };
    else for (const k of mkeys(m)) mput(out, k, m[k]);
  }
  return out ?? {};
}, "__merge", null);
B.__concat = meta((...ls) => {
  const out = [];
  for (const l of ls) {
    if (isNil(l)) continue;
    if (!Array.isArray(l)) fail(`\`[...x]\` spreads a list (or null) into a list, but x is ${typeName(l)}`);
    for (const x of l) out.push(x);
  }
  return out;
}, "__concat", null);
B.err = meta((v = null) => new Err(v, true), "err", null);
B.is_err = meta((v) => v instanceof Err, "is_err", null);
B.unwrap = meta((v) => { if (v instanceof Err) fail(`unwrap of an error: ${display(v.error)}`); return v; }, "unwrap", null);
const textHtml = (s) => Object.assign(rawHtml(s), { text: s });
B.raw = meta((v) => textHtml(display(v)), "raw", null);
// markdown(text, options): the server makes it (/_quillo/markdown), so a page
// shows the same HTML as the server would, highlighting included; awaited,
// like a server fn, and as one, an error value when it cannot be reached.
B.markdown = meta(async (s, o) => {
  S(s, "markdown");
  if (!isNil(o) && !isMap(o)) fail(`\`markdown\` expects options as a map, got ${typeName(o)}`);
  let r, body = null;
  try {
    r = await wireFetch(BASE + "/_quillo/markdown", { method: "POST", headers: { "content-type": "application/json" }, body: jsonText([s, isNil(o) ? null : o]) });
    body = await r.json();
  } catch (e) {
    return new Err(`markdown could not be made: ${e.message}`);
  }
  if (!r.ok || typeof body.ok !== "string") return new Err(`markdown could not be made (${r.status})${body && body.error ? ": " + body.error : ""}`);
  return textHtml(body.ok);
}, "markdown", null);
// ask(message, options) and notify(message, options): a dialog that waits
// for an answer, and a note that goes by itself. They are markup with q-
// classes in a host on the page (#q-notices), laid out and styled by the
// page's CSS: <body data-notices="..."> adds its classes to the host.
function noticeHost() {
  let h = document.getElementById("q-notices");
  if (!h) {
    h = document.createElement("div");
    h.id = "q-notices";
    h.className = "q-notices " + (document.body.dataset.notices || "dialog-center toasts-top-right");
    document.body.appendChild(h);
  }
  return h;
}
function noticeEl(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
}
// `ask`'s own buttons in the page's language: the program's `quillo.ok` and `quillo.cancel`
// when it translates them, else these (FEEDBACK30.md, Q-377; branch feedback27-35-fixes).
const ASK_WORDS = { es: ["Aceptar", "Cancelar"], fr: ["OK", "Annuler"], de: ["OK", "Abbrechen"], it: ["OK", "Annulla"], pt: ["OK", "Cancelar"],
  nl: ["OK", "Annuleren"], sv: ["OK", "Avbryt"], nb: ["OK", "Avbryt"], da: ["OK", "Annuller"], fi: ["OK", "Peruuta"], pl: ["OK", "Anuluj"],
  cs: ["OK", "Zrušit"], ro: ["OK", "Anulează"], hu: ["OK", "Mégse"], tr: ["Tamam", "İptal"], el: ["OK", "Ακύρωση"], ru: ["ОК", "Отмена"],
  uk: ["OK", "Скасувати"], ar: ["موافق", "إلغاء"], he: ["אישור", "ביטול"], hi: ["ठीक है", "रद्द करें"], ja: ["OK", "キャンセル"],
  zh: ["确定", "取消"], "zh-hant": ["確定", "取消"], ko: ["확인", "취소"], th: ["ตกลง", "ยกเลิก"], vi: ["OK", "Hủy"], id: ["OK", "Batal"],
  ms: ["OK", "Batal"], fil: ["OK", "Kanselahin"], tl: ["OK", "Kanselahin"] };
const askWords = () => {
  const d = i18n();
  const loc = String((d && d.locale) || (typeof document !== "undefined" && document.documentElement.lang) || "en").toLowerCase().replace("_", "-");
  const lang = loc.split("-")[0];
  const zhHant = lang === "zh" && /-(hant|tw|hk|mo)\b/.test(loc);
  const w = ASK_WORDS[zhHant ? "zh-hant" : lang] || ["OK", "Cancel"];
  const own = (k, i) => d && hasOwn(d.t, k) && typeof d.t[k] === "string" ? d.t[k] : w[i];
  return [own("quillo.ok", 0), own("quillo.cancel", 1)];
};
B.ask = meta((message, options) => new Promise((resolve) => {
  const o = options == null ? {} : M(options, "ask", 2);
  const host = noticeHost(), before = document.activeElement;
  const back = noticeEl("div", "q-backdrop");
  const box = noticeEl("div", "q-dialog" + (o.tone ? " q-" + display(o.tone) : ""));
  box.setAttribute("role", "alertdialog");
  box.setAttribute("aria-modal", "true");
  const id = "q-" + Math.random().toString(36).slice(2);
  if (o.title != null) { const h = noticeEl("h2", "q-title", display(o.title)); h.id = id + "-t"; box.appendChild(h); box.setAttribute("aria-labelledby", h.id); }
  const p = noticeEl("p", "q-message", display(message)); p.id = id + "-m"; box.appendChild(p); box.setAttribute("aria-describedby", p.id);
  const row = noticeEl("div", "q-actions");
  const words = askWords();
  const no = noticeEl("button", "q-cancel", o.cancel != null ? display(o.cancel) : words[1]);
  const yes = noticeEl("button", "q-ok", o.ok != null ? display(o.ok) : words[0]);
  no.type = yes.type = "button";
  row.append(no, yes);
  box.appendChild(row);
  back.appendChild(box);
  host.appendChild(back);
  requestAnimationFrame(() => back.classList.add("q-open"));
  const done = (answer) => {
    document.removeEventListener("keydown", keys, true);
    back.classList.remove("q-open");
    setTimeout(() => back.remove(), 160);
    if (before && before.focus) before.focus();
    resolve(answer);
  };
  const keys = (e) => {
    if (e.key === "Escape") { e.preventDefault(); done(false); }
    else if (e.key === "Tab") { e.preventDefault(); (document.activeElement === yes ? no : yes).focus(); }
  };
  document.addEventListener("keydown", keys, true);
  yes.onclick = () => done(true);
  no.onclick = () => done(false);
  back.onclick = (e) => { if (e.target === back) done(false); };
  // A dangerous question starts on "Cancel".
  (o.tone === "danger" ? no : yes).focus();
}), "ask", ["message", "options"]);
B.notify = meta((message, options) => {
  const o = options == null ? {} : M(options, "notify", 2);
  const host = noticeHost();
  let list = host.querySelector(".q-toasts");
  if (!list) { list = noticeEl("div", "q-toasts"); list.setAttribute("role", "status"); list.setAttribute("aria-live", "polite"); host.appendChild(list); }
  const t = noticeEl("div", "q-toast" + (o.tone ? " q-" + display(o.tone) : ""));
  if (o.title != null) t.appendChild(noticeEl("b", "q-title", display(o.title)));
  t.appendChild(noticeEl("span", "q-message", display(message)));
  const x = noticeEl("button", "q-close", "×");
  x.type = "button";
  x.setAttribute("aria-label", "Close");
  const close = () => { t.classList.remove("q-open"); setTimeout(() => t.remove(), 160); };
  x.onclick = close;
  t.appendChild(x);
  list.appendChild(t);
  requestAnimationFrame(() => t.classList.add("q-open"));
  setTimeout(close, (o.seconds != null ? +o.seconds : 4) * 1000);
  return null;
}, "notify", ["message", "options"]);

// highlight(code, lang): the same tokens as the server's (highlight.rs).
const HL = (() => {
  const L = (o) => Object.assign({ line: [], block: null, quotes: "", keywords: [], consts: [], types: false, tags: false, css: false, json: false, nocase: false }, o);
  const qwe = L({ line: ["//"], block: ["/*", "*/"], quotes: "\"`", types: true, tags: true,
    keywords: ["let", "fn", "view", "component", "server", "route", "routes", "use", "import", "if", "else", "match", "for", "in", "return", "reply", "as", "while", "break", "continue", "type", "and", "or", "not", "with"],
    consts: ["true", "false", "null", "int", "str", "bool", "float", "dec", "map", "list", "any"] });
  const js = L({ line: ["//"], block: ["/*", "*/"], quotes: "\"'`", types: true,
    keywords: ["break", "case", "catch", "class", "const", "continue", "debugger", "default", "delete", "do", "else", "export", "extends", "finally", "for",
      "function", "if", "import", "in", "instanceof", "let", "new", "return", "super", "switch", "this", "throw", "try", "typeof", "var", "void",
      "while", "with", "yield", "async", "await", "of", "static", "from", "interface", "type", "enum", "implements", "as"],
    consts: ["true", "false", "null", "undefined", "NaN", "Infinity"] });
  const py = L({ line: ["#"], quotes: "\"'", types: true,
    keywords: ["and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif", "else", "except", "finally", "for", "from",
      "global", "if", "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while", "with", "yield"],
    consts: ["True", "False", "None", "self"] });
  const rust = L({ line: ["//"], block: ["/*", "*/"], quotes: "\"", types: true,
    keywords: ["as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "fn", "for", "if", "impl", "in", "let",
      "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "static", "struct", "super", "trait", "type", "unsafe", "use", "where", "while"],
    consts: ["true", "false", "self", "Self", "None"] });
  const sh = L({ line: ["#"], quotes: "\"'",
    keywords: ["if", "then", "else", "elif", "fi", "for", "while", "do", "done", "case", "esac", "function", "in", "return", "export", "local", "exit", "cd", "echo", "sudo"],
    consts: ["true", "false"] });
  const sql = L({ line: ["--"], block: ["/*", "*/"], quotes: "'\"", nocase: true, consts: ["null", "true", "false"],
    keywords: ["select", "from", "where", "insert", "into", "values", "update", "set", "delete", "create", "table", "alter", "add", "column", "index",
      "primary", "key", "references", "not", "and", "or", "join", "left", "right", "inner", "outer", "on", "group", "by", "order", "having",
      "limit", "offset", "as", "distinct", "unique", "default", "integer", "text", "real", "begin", "commit", "drop", "if", "exists", "case",
      "when", "then", "else", "end", "like", "in", "is", "desc", "asc", "union", "all", "with"] });
  const css = L({ block: ["/*", "*/"], quotes: "\"'", keywords: ["!important"], css: true });
  const html = L({ tags: true });
  const json = L({ quotes: "\"", consts: ["true", "false", "null"], json: true });
  const plain = L({});
  const langs = { qwe, quillo: qwe, js, javascript: js, ts: js, typescript: js, jsx: js, tsx: js, mjs: js, py, python: py, rs: rust, rust,
    sh, bash: sh, shell: sh, zsh: sh, console: sh, sql, css, html, xml: html, svg: html, json };
  const esc = (t) => t.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
  // Rust's is_alphabetic and is_alphanumeric: with marks such as U+093F and numbers such as `Ⅰ`,
  // where `\p{L}` left them out, and a word or a quote was read as another on the server
  // (FEEDBACK55.md, Q-834; branch feedback55).
  const alpha = (c) => c !== undefined && /\p{Alphabetic}/u.test(c);
  const alnum = (c) => c !== undefined && /[\p{Alphabetic}\p{N}]/u.test(c);
  const digit = (c) => c !== undefined && c >= "0" && c <= "9";
  const identStart = (c) => alpha(c) || c === "_" || c === "$";
  const identChar = (c, isCss) => alnum(c) || c === "_" || c === "$" || (isCss && c === "-");
  // Where a route's path is on the line from `b` (`route ANY /*rest`, `routes /admin`, `mount /blog`):
  // `/*` there starts no comment, as the lexer reads a path (branch feedback40-43). Found once a line,
  // as highlight.rs route_path: read from the line's start for each `/*`, it was quadratic
  // (FEEDBACK55.md, Q-706; branch feedback55).
  const routePath = (s, b) => {
    const space = (c) => c === " " || c === "\t" || c === "\r";
    const skip = (j) => { while (j < s.length && space(s[j])) j++; return j; };
    const word = (j) => { const f = j; while (j < s.length && s[j] !== "\n" && !space(s[j])) j++; return [f, j]; };
    let [f, t] = word(skip(b)), at = skip(t);
    const first = s.slice(f, t).join("");
    if (first === "route") {
      [f, t] = word(at);
      if (f === t || !s.slice(f, t).every((c) => c >= "A" && c <= "Z")) return null;
      at = skip(t);
    } else if (first !== "routes" && first !== "mount") return null;
    [f, t] = word(at);
    return f < t && s[f] === "/" ? [f, t] : null;
  };
  // Where a Rust raw string at `i` ends, `r"..."`, `r#"..."#`, `br"..."`, as highlight.rs raw_end;
  // -1 where none starts (FEEDBACK55.md, Q-834; branch feedback55).
  const rawEnd = (s, i) => {
    let j = i + (s[i] === "b" ? 1 : 0);
    if (s[j] !== "r") return -1;
    const from = ++j;
    while (j < s.length && s[j] === "#") j++;
    const hashes = j - from;
    if (s[j] !== '"') return -1;
    for (j++; j < s.length; j++) {
      if (s[j] === '"' && s.length - (j + 1) >= hashes && s.slice(j + 1, j + 1 + hashes).every((c) => c === "#")) return j + 1 + hashes;
    }
    return s.length;
  };
  // Whether a `/` at `i` starts a regular expression in JavaScript, as highlight.rs regex_may_start
  // (FEEDBACK55.md, Q-834; branch feedback55).
  const regexMayStart = (s, i) => {
    let k = i; while (k > 0 && " \t\r\n".includes(s[k - 1])) k--;
    if (k === 0 || "(,=:[!&|?{};+-*%>~^".includes(s[k - 1])) return true;
    let b = k; while (b > 0 && k - b < 11 && identChar(s[b - 1], false)) b--;
    return (b === 0 || !identChar(s[b - 1], false))
      && ["return", "typeof", "instanceof", "case", "do", "else", "in", "of", "new", "delete", "void", "throw", "yield", "await"].includes(s.slice(b, k).join(""));
  };
  // Whether the `mcp` from `i` to `j` is a tool's tag, as highlight.rs mcp_tag: after `fn`, `view` or
  // `component` with a name after it, or after `service` with a name or a `/` (FEEDBACK56.md, Q-853;
  // branch feedback56).
  const mcpTag = (s, i, j) => {
    const space = (c) => c === " " || c === "\t";
    let b = i; while (b > 0 && space(s[b - 1])) b--;
    const end = b; while (b > 0 && identChar(s[b - 1], false)) b--;
    const before = s.slice(b, end).join("");
    let k = j; while (k < s.length && space(s[k])) k++;
    // A `/` that starts a comment starts no path (FEEDBACK56.md, Q-853, found in review; branch feedback56).
    const after = s[k], name = alpha(after) || after === "_", path = after === "/" && s[k + 1] !== "/" && s[k + 1] !== "*";
    return end < i && space(s[j]) && ((["fn", "view", "component"].includes(before) && name) || (before === "service" && (name || path)));
  };
  return (code, name) => {
    // A language of its own: `constructor` or `__proto__` found the object's, and threw
    // (FEEDBACK55.md, Q-711; branch feedback55).
    const key = String(name || "").toLowerCase();
    const l = hasOwn(langs, key) ? langs[key] : plain;
    // A language it does not know is text, only escaped (FEEDBACK55.md, Q-834; branch feedback55).
    if (l === plain) return esc(code);
    const s = Array.from(code);
    const n = s.length;
    const starts = (i, p) => { const ps = Array.from(p); for (let k = 0; k < ps.length; k++) if (s[i + k] !== ps[k]) return false; return true; };
    let out = "", i = 0, plainAt = 0, depth = 0, inTag = false;
    const span = (from, to, cls) => { out += esc(s.slice(plainAt, from).join("")) + `<span class="${cls}">` + esc(s.slice(from, to).join("")) + "</span>"; plainAt = to; };
    const block = l.block || (l.tags ? ["<!--", "-->"] : null);
    // The last line a route's path was looked for on: where it ends, and the path, so that each
    // line is read once (Q-706); where a regular expression may start again, after one that did
    // not close on its line (Q-834).
    let seen = [0, 0, 0], noRegex = 0;
    const inPath = (i) => {
      if (i >= seen[0]) {
        let b = i, e = i;
        while (b > seen[0] && s[b - 1] !== "\n") b--;
        while (e < n && s[e] !== "\n") e++;
        const p = routePath(s, b) || [0, 0];
        seen = [e, p[0], p[1]];
      }
      return seen[1] <= i && i < seen[2];
    };
    // A range's `..` ends a number and starts none: `0..n` (Q-834).
    const dots = (j) => s[j] === "." && (s[j + 1] === "." || (j > 0 && s[j - 1] === "."));
    while (i < n) {
      const c = s[i];
      // The shell's `#` starts a comment at a word's start only: `${#a[@]}` (Q-834).
      if (l.line.some((p) => starts(i, p)) && !(l === sh && i > 0 && !" \t\r\n;|&()<>".includes(s[i - 1]))) { let j = i; while (j < n && s[j] !== "\n") j++; span(i, j, "hl-c"); i = j; continue; }
      if (block && starts(i, block[0]) && !(l.tags && block[0] === "/*" && inTag) && !(l === qwe && inPath(i))) {
        let j = i + Array.from(block[0]).length; while (j < n && !starts(j, block[1])) j++;
        j = Math.min(j + Array.from(block[1]).length, n); span(i, j, "hl-c"); i = j; continue;
      }
      if (l.tags && starts(i, "<!--")) { let j = i + 4; while (j < n && !starts(j, "-->")) j++; j = Math.min(j + 3, n); span(i, j, "hl-c"); i = j; continue; }
      if (l.tags && c === "<") {
        let j = i + 1; if (s[j] === "/") j++;
        if (alpha(s[j])) { const from = j; while (j < n && (alnum(s[j]) || "-_.:".includes(s[j]))) j++; span(from, j, "hl-t"); inTag = l === html; i = j; continue; }
      }
      if (inTag) {
        if (c === ">") inTag = false;
        else if (c === '"' || c === "'") { let j = i + 1; while (j < n && s[j] !== c) j++; j = Math.min(j + 1, n); span(i, j, "hl-s"); i = j; continue; }
        else if (alpha(c) || c === "@" || c === ":") { let j = i; while (j < n && (alnum(s[j]) || "-_:@.".includes(s[j]))) j++; span(i, j, "hl-a"); i = j; continue; }
        i++; continue;
      }
      // Python's `"""` and `'''` go on over lines, as Rust's `"` does (Q-834).
      const triple = c === '"' ? '"""' : "'''";
      if (l === py && (c === '"' || c === "'") && starts(i, triple)) {
        let j = i + 3; while (j < n && !starts(j, triple)) { if (s[j] === "\\") j++; j++; }
        j = Math.min(j + 3, n); span(i, j, "hl-s"); i = j; continue;
      }
      // Rust's raw `r"..."` (Q-834).
      if (l === rust && (c === "r" || c === "b") && !(i > 0 && identChar(s[i - 1], false))) {
        const j = rawEnd(s, i);
        if (j >= 0) { span(i, j, "hl-s"); i = j; continue; }
      }
      // Rust's `'"'` is a character and `'a` a lifetime (Q-834).
      if (l === rust && c === "'") {
        let close = -1;
        if (s[i + 1] === "\\" && s[i + 2] !== undefined && s[i + 2] !== "\n") {
          for (let j = i + 3; j < n && j < i + 12 && s[j] !== "\n"; j++) if (s[j] === "'") { close = j; break; }
        } else if (i + 2 < n && s[i + 1] !== "\n" && s[i + 1] !== "'" && s[i + 2] === "'") close = i + 2;
        if (close >= 0) { span(i, close + 1, "hl-s"); i = close + 1; continue; }
      }
      // The shell's `it's`: a `'` in a word is no quote (Q-834).
      if (l.quotes.includes(c) && !(l === sh && c === "'" && i > 0 && identChar(s[i - 1], false))) {
        let j = i + 1;
        while (j < n && s[j] !== c && (c === "`" || (l === rust && c === '"') || s[j] !== "\n")) { if (s[j] === "\\") j++; j++; }
        j = Math.min(j + 1, n);
        let k = j; while (k < n && (s[k] === " " || s[k] === "\t")) k++;
        span(i, j, l.json && s[k] === ":" ? "hl-a" : "hl-s"); i = j; continue;
      }
      // A JavaScript regular expression, to its `/` on the line, with its flags (Q-834).
      if (l === js && c === "/" && i >= noRegex && regexMayStart(s, i)) {
        let j = i + 1, inClass = false;
        while (j < n && s[j] !== "\n" && (inClass || s[j] !== "/")) {
          if (s[j] === "\\" && j + 1 < n && s[j + 1] !== "\n") j++;
          else if (s[j] === "[") inClass = true;
          else if (s[j] === "]") inClass = false;
          j++;
        }
        if (s[j] === "/") { j++; while (j < n && /[A-Za-z]/.test(s[j])) j++; span(i, j, "hl-s"); i = j; continue; }
        noRegex = j;
      }
      if (l.css) {
        if (c === "{") depth++; else if (c === "}") depth = Math.max(depth - 1, 0);
        if ((c === "#" || c === ".") && identChar(s[i + 1], true) && !(c === "." && digit(s[i + 1]))) {
          let j = i + 1; while (j < n && identChar(s[j], true)) j++; span(i, j, depth > 0 && c === "#" ? "hl-n" : "hl-f"); i = j; continue;
        }
        if (c === "@" || (c === "!" && starts(i, "!important"))) { let j = i + 1; while (j < n && identChar(s[j], true)) j++; span(i, j, "hl-k"); i = j; continue; }
      }
      const prevIdent = i > 0 && identChar(s[i - 1], l.css);
      if (!(i > 0 && identChar(s[i - 1], false)) && (digit(c) || (c === "." && digit(s[i + 1]) && !dots(i)))) {
        let j = i; while (j < n && ((/[A-Za-z0-9_]/.test(s[j]) || (s[j] === "." && !dots(j))) || (l.css && s[j] === "%"))) j++; span(i, j, "hl-n"); i = j; continue;
      }
      if (identStart(c) && !prevIdent) {
        let j = i; while (j < n && identChar(s[j], l.css)) j++;
        const word = s.slice(i, j).join(""), probe = l.nocase ? word.toLowerCase() : word;
        let k = j; while (k < n && (s[k] === " " || s[k] === "\t")) k++;
        const next = s[k] === undefined ? " " : s[k];
        const cls = l.css ? (depth > 0 && next === ":" ? "hl-a" : depth === 0 ? "hl-t" : null)
          : l.keywords.includes(probe) || (l === qwe && probe === "mcp" && mcpTag(s, i, j)) ? "hl-k" : l.consts.includes(probe) ? "hl-n" : next === "(" ? "hl-f"
          : l.types && c !== c.toLowerCase() && c === c.toUpperCase() ? "hl-t" : null;
        if (cls) span(i, j, cls);
        i = j; continue;
      }
      i++;
    }
    return out + esc(s.slice(plainAt).join(""));
  };
})();
// With its text, as `markdown`'s and `raw`'s: `str(highlight(...))` was "<html>", where the
// server gives the markup (FEEDBACK55.md, Q-834; branch feedback55).
B.highlight = meta((code, lang) => textHtml(HL(display(code), lang == null ? "" : display(lang))), "highlight", null);

// As the server escapes, a CR `&#13;` and a NUL U+FFFD too (FEEDBACK55.md, Q-772; found in review).
B.escape = meta((v) => escText(display(v)), "escape", null);
// url_encode(v): as the server's, so links built on either side match.
const urlPart = (s) => encodeURIComponent(s).replace(/[!'()*]/g, (c) => "%" + c.charCodeAt(0).toString(16).toUpperCase());
// An error is given back, not its text in the URL (FEEDBACK55.md, Q-784; branch feedback55). In
// the order set (FEEDBACK55.md, Q-875; branch feedback55).
B.url_encode = meta((v) => firstErr(v) || (isMap(v)
  ? mkeys(v).filter((k) => !isNil(v[k])).map((k) => urlPart(k) + "=" + urlPart(display(v[k]))).join("&")
  : urlPart(display(v))), "url_encode", null);
// The first error in `v`, itself or held in its lists and maps at any depth, in their order, as
// the server's `first_error`: `url_encode` gives it back (FEEDBACK55.md, Q-784; branch feedback55).
function firstErr(v) {
  const todo = [v];
  while (todo.length) {
    const x = todo.pop();
    if (x instanceof Err) return x;
    if (Array.isArray(x)) for (let i = x.length - 1; i >= 0; i--) todo.push(x[i]);
    else if (isMap(x) && !foreign(x)) { const ks = mkeys(x); for (let i = ks.length - 1; i >= 0; i--) todo.push(x[ks[i]]); }
  }
  return null;
}
B.now = meta(() => Date.now(), "now", null);
B.random = meta(() => fl(Math.random()), "random", null);
B.random_int = meta((lo, hi) => {
  II(lo, "random_int"); II(hi, "random_int", 2);
  if (lo > hi) fail(`\`random_int\` needs lo <= hi, got ${lo} and ${hi}`);
  // Past 2^53, from 64 random bits (found in review).
  if (typeof lo === "bigint" || typeof hi === "bigint" || hi - lo >= 2 ** 52) {
    const span = BigInt(hi) - BigInt(lo) + 1n, bits = (BigInt(Math.floor(Math.random() * 2 ** 32)) << 32n) | BigInt(Math.floor(Math.random() * 2 ** 32));
    return intOf(BigInt(lo) + bits % span);
  }
  return lo + Math.floor(Math.random() * (hi - lo + 1));
}, "random_int", null);
B.uuid = meta(() => (typeof crypto !== "undefined" && crypto.randomUUID) ? crypto.randomUUID()
  : "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, (c) => { const r = Math.random() * 16 | 0; return (c === "x" ? r : (r & 3 | 8)).toString(16); }), "uuid", null);
B.abs = meta((x) => x instanceof Dec ? new Dec(babs(x.m), x.s) : typeof x === "bigint" ? bigChecked(babs(x)) : x instanceof Fl ? new Fl(Math.abs(x.f)) : Math.abs(N(x, "abs")), "abs", null);
// Halves round away from zero, as on the server (Math.round(-2.5) is -2).
const roundHalf = (x) => Math.sign(x) * Math.round(Math.abs(x));
// What `round`, `floor` and `ceil` give: an int wherever one holds it (past 2^53 a BigInt), else
// a float, as the server's `to_int_or_float` (FEEDBACK55.md, Q-788 and Q-789; branch
// feedback55); an int is itself, exactly (FEEDBACK55.md, Q-776; branch feedback55).
const intOrFloat = (r) => (r >= -(2 ** 63) && r < 2 ** 63 ? (Number.isSafeInteger(r) ? r + 0 : BigInt(r)) : fl(r));
const isWhole = (x) => isInt(x) || typeof x === "bigint";
// A decimal rounded: round(d, digits = 0, mode = "half_even"), keeping
// exactly `digits` decimals.
function roundDec(x, d, mode) {
  if (!isNil(d)) d = IN(d, "round", 2);
  if (isNil(mode)) mode = "half_even";
  else if (typeof mode !== "string") fail(`\`round\` expects a str as argument 3, got ${typeName(mode)}`);
  else if (!["half_even", "half_up", "half_down", "up", "down", "ceiling", "floor"].includes(mode)) fail(`\`round\`: unknown rounding ${JSON.stringify(mode)} (one of ${ROUNDINGS})`);
  return decRound(x, isNil(d) ? 0 : d, mode);
}
B.round = divides(meta((x, d, mode) => {
  if (x instanceof Dec) return roundDec(x, d, mode);
  if (!isNil(mode)) fail("`round` takes a rounding mode for a dec only: convert with dec(x)");
  if (d === undefined) return isWhole(x) ? x : intOrFloat(roundHalf(N(x, "round")));
  // An int to `d` places is itself, to fewer than none a multiple of a power of ten, exactly; a
  // float to fewer than none, as a decimal is, as on the server (FEEDBACK55.md, Q-789; branch
  // feedback55).
  if (isWhole(x)) return roundInt(x, IN(d, "round", 2));
  x = N(x, "round");
  if (IN(d, "round", 2) < 0) return fl(roundLeft(x, IN(d, "round", 2)));
  const k = Math.min(Math.max(IN(d, "round", 2), 0), 15);
  // From the float's shortest decimal, a tie away from zero, as on the server (FEEDBACK50.md,
  // Q-626; branch feedback50-fixes). A float, whole too (FEEDBACK55.md, Q-775).
  const p = 10 ** k, y = x * p;
  if (!(Math.abs(y) >= 1e9 || Math.abs(Math.abs(y - Math.trunc(y)) - 0.5) < 1e-6)) return fl(roundHalf(y) / p + 0);
  const dd = decFromFloat(x);
  if (dd !== null) return fl(Number(decText(decRound(dd, k, "half_up"))));
  return fl(Number.isFinite(y) ? roundHalf(y) / p : x); }, "round", null), "round");
// The server's `round_int` and `round_left`.
function roundInt(n, d) {
  if (d >= 0) return n;
  const p = 10n ** BigInt(Math.min(-d, 20)), m = BigInt(n), q = ((m < 0n ? -m : m) + p / 2n) / p * p;
  return bigChecked(m < 0n ? -q : q);
}
// The server's `round_left`: on the float's shortest digits, a half away from zero, made the float
// nearest to it, at every `d`; past the largest float, an error (FEEDBACK55.md, Q-789; found
// writing the docs: past 28 digits it went through floats).
function roundLeft(x, d) {
  if (!Number.isFinite(x) || x === 0) return x + 0;
  const [mant, exp] = Math.abs(x).toExponential().split("e");
  let ds = mant.replace(".", "");
  const keep = Number(exp) + 1 + d;
  if (keep < 0) return 0;
  if (keep >= ds.length) return x;
  const up = ds.charCodeAt(keep) >= 53;
  ds = ds.slice(0, keep);
  if (up) ds = keep === 0 ? "1" : String(BigInt(ds) + 1n);
  if (ds === "") return 0;
  const r = Number(ds + "e" + -d);
  if (!Number.isFinite(r)) fail("`round`: the rounded value is past the largest float");
  return (x < 0 ? -r : r) + 0;
}
B.floor = divides(meta((x) => x instanceof Dec ? decRound(x, 0, "floor") : isWhole(x) ? x : intOrFloat(Math.floor(N(x, "floor"))), "floor", null), "floor");
B.ceil = divides(meta((x) => x instanceof Dec ? decRound(x, 0, "ceiling") : isWhole(x) ? x : intOrFloat(Math.ceil(N(x, "ceil"))), "ceil", null), "ceil");
B.sqrt = meta((x) => fl(Math.sqrt(N(x, "sqrt"))), "sqrt", null);
B.pow = meta((x, y) => {
  // A decimal to an int's power, by squaring as src/decimal.rs's powi (FEEDBACK50.md, Q-632;
  // branch feedback50-fixes).
  if (x instanceof Dec) {
    if (!isInt(y) && typeof y !== "bigint") fail(`\`pow\` takes a dec to an int's power, got ${typeName(y)} as the power: for a fraction of a power, float(x) and math.pow`);
    const one = new Dec(1n, 0), n = BigInt(y), e = n < 0n ? -n : n;
    const powU = (b) => {
      let base = b, acc = one, k = e;
      while (k > 0n) {
        if (k & 1n) acc = decMul(acc, base);
        k >>= 1n;
        if (k > 0n) base = decMul(base, base);
      }
      return acc;
    };
    if (n >= 0n) return powU(x);
    // Below one, one over it first; above one, a power past what fits is 0 at 28 decimals,
    // as src/decimal.rs's powi (found in review).
    if (decCmp(new Dec(babs(x.m), x.s), one) < 0) return powU(decDiv(one, x));
    let p;
    try { p = powU(x); } catch (err) { if (err instanceof QuilloError && err.message === "decimal overflow") return new Dec(0n, DEC_SCALE); throw err; }
    return decDiv(one, p);
  }
  // As `*` does, exactly (FEEDBACK29.md, Q-351; branch feedback29-fixes); a power past 2^53 too,
  // the int it is for -1, 0 and 1 and the overflow for any other base, as on the server: it was
  // a float, `pow(-1, 9007199254740993)` 1.0 (FEEDBACK55.md, Q-790; branch feedback55).
  if ((isInt(x) || typeof x === "bigint") && (isInt(y) || typeof y === "bigint") && y >= 0) {
    if (x === -1) return (typeof y === "bigint" ? y % 2n : y % 2) ? -1 : 1;
    if (x === 0 || x === 1) return y == 0 ? 1 : x;
    if (y > 64) fail("integer overflow");
    return bigChecked(BigInt(x) ** BigInt(y));
  }
  return fl(N(x, "pow") ** N(y, "pow", 2));
}, "pow", null);
// Each item looked for only among those kept under its key, as the server's `unique_of` keys
// them, which `==` gives equal values alike: it compared each with every one kept, quadratic
// (FEEDBACK55.md, Q-791; branch feedback55). Keys are made from a stack, each part once: a part
// was quoted again at each level, twice as long each time (found in review). Without a decimal in
// the list, a float by its value, an int by the float that is exactly it or else itself. With
// one, each number by the decimal it is equal to (past 2^53 its nearest float), but a float its
// decimal does not give back by itself; where it meets a small decimal it may be equal to, the
// item is kept too under its key with such floats made decimals, by the kinds there, and looked
// for among those of other kinds (found in review: `[k * 1e-40, 1m]` was quadratic).
B.unique = meta((l) => {
  l = L(l, "unique");
  const decs = l.some(holdsDec);
  const out = [], byKey = new Map(), groups = new Map();
  for (const x of l) {
    const k = uniqueKeys(x, decs);
    if (k.never) { out.push(x); continue; }
    const kept = byKey.get(k.main);
    let seen = kept !== undefined && kept.some((y) => eq(x, y));
    if (!seen && k.small > 0) {
      const kinds = groups.get(k.canon);
      if (kinds !== undefined) for (const [p, ys] of kinds) if (p !== k.pattern && ys.some((y) => eq(x, y))) { seen = true; break; }
    }
    if (seen) continue;
    out.push(x);
    if (kept === undefined) byKey.set(k.main, [x]); else kept.push(x);
    if (k.small > 0) {
      let kinds = groups.get(k.canon);
      if (kinds === undefined) groups.set(k.canon, (kinds = new Map()));
      const ys = kinds.get(k.pattern);
      if (ys === undefined) kinds.set(k.pattern, [x]); else ys.push(x);
    }
  }
  return out;
}, "unique", null);
// Whether `v` holds a decimal anywhere.
function holdsDec(v) {
  const todo = [v];
  while (todo.length) {
    const x = todo.pop();
    if (x instanceof Dec) return true;
    if (Array.isArray(x)) todo.push(...x);
    else if (x instanceof Err) todo.push(x.error);
    else if (isMap(x)) for (const k of Object.keys(x)) todo.push(x[k]);
  }
  return false;
}
const uniqueIds = new WeakMap();
let uniqueNext = 0;
const TWO53 = 2 ** 53;
// A decimal's token: by its value, or past 2^53 by its nearest float.
function decTok(d) {
  const a = d.m < 0n ? -d.m : d.m;
  return a >= (1n << 53n) * p10(d.s) ? "N" + Number(decText(d)) : "V" + decKey(d);
}
// `|d| < 1e-10`: a decimal a float its text does not give back may be equal to.
function smallDec(d) {
  let m = d.m < 0n ? -d.m : d.m, s = d.s;
  while (s > 0 && m % 10n === 0n) { m /= 10n; s--; }
  return m === 0n || (s > 10 && m < p10(s - 10));
}
// `unique`'s keys for `v`, as the server's: its own, the one with each float made its decimal, and
// the kinds of its small numbers that may meet, with how many there are.
function uniqueKeys(v, decs) {
  const main = [], canon = [], k = { pattern: "", small: 0, never: false };
  const put = (a, b) => { main.push(a); canon.push(b === undefined ? a : b); };
  const todo = [v];
  while (todo.length) {
    const x = todo.pop();
    if (x === MARK_END) { put("]"); continue; }
    if (x instanceof MapKey) { put("k" + JSON.stringify(x.k)); continue; }
    if (typeof x === "string") { put("s" + JSON.stringify(x)); continue; }
    if (isNil(x)) { put("z"); continue; }
    if (typeof x === "boolean") { put(x ? "b1" : "b0"); continue; }
    if (typeof x === "number" || x instanceof Fl || typeof x === "bigint") {
      const f = x instanceof Fl ? x.f : x;
      if (typeof f === "number" && Number.isNaN(f)) { k.never = true; put("n"); continue; }
      const int = typeof f === "bigint" || (!(x instanceof Fl) && Number.isInteger(f));
      if (!decs) {
        if (!int || typeof f === "number" || BigInt(Number(f)) === f) put("f" + (Number(f) + 0));
        else put("i" + f);
        continue;
      }
      const n = Number(f);
      if (Math.abs(n) >= TWO53 || !Number.isFinite(n)) { put("N" + n); continue; }
      if (int) { put("V" + f); continue; }
      // Finite, below 2^53: a decimal holds it.
      const d = decFromFloat(n);
      if (Math.abs(n) >= 1e-10 || Number(decText(d)) === n) { put(decTok(d)); continue; }
      k.small++; k.pattern += "F";
      put("X" + n, decTok(d));
      continue;
    }
    if (x instanceof Dec) {
      if (smallDec(x)) { k.small++; k.pattern += "D"; }
      put(decTok(x));
      continue;
    }
    if (x instanceof Html) { put("h" + JSON.stringify(htmlText(x))); continue; }
    if (Array.isArray(x)) {
      put("[" + x.length);
      todo.push(MARK_END);
      for (let i = x.length - 1; i >= 0; i--) todo.push(x[i]);
      continue;
    }
    if (x instanceof Err) { put("e"); todo.push(MARK_END, x.error); continue; }
    if (isMap(x) && !foreign(x)) {
      const ks = Object.keys(x).sort();
      put("{" + ks.length);
      todo.push(MARK_END);
      for (let i = ks.length - 1; i >= 0; i--) todo.push(x[ks[i]], new MapKey(ks[i]));
      continue;
    }
    // A function, or the page's object, by itself.
    if (!uniqueIds.has(x)) uniqueIds.set(x, ++uniqueNext);
    put("o" + uniqueIds.get(x));
  }
  return { main: main.join("\u0001"), canon: canon.join("\u0001"), pattern: k.pattern, small: k.small, never: k.never };
}
const MARK_END = Symbol("end");
class MapKey { constructor(k) { this.k = k; } }
B.flatten = meta((l) => [].concat(...L(l, "flatten").map((x) => Array.isArray(x) ? x : [x])), "flatten", null);
B.zip = meta((a, b) => { a = L(a, "zip"); b = L(b, "zip", 2); return a.slice(0, Math.min(a.length, b.length)).map((x, i) => [x, b[i]]); }, "zip", null);
B.take = meta((v, n) => { n = Math.max(IN(n, "take", 2), 0); return typeof v === "string" ? Array.from(v).slice(0, n).join("") : L(v, "take").slice(0, n); }, "take", null);
B.drop = meta((v, n) => { n = Math.max(IN(n, "drop", 2), 0); return typeof v === "string" ? Array.from(v).slice(n).join("") : L(v, "drop").slice(n); }, "drop", null);
B.count = meta((l, f) => { l = L(l, "count"); if (f === undefined) return l.length; F(f, "count"); let n = 0; for (const x of l) { const r = cb(f, "count", x); if (r instanceof Err) return r; if (truthy(r)) n++; } return n; }, "count", null);
// Its groups in the order met (FEEDBACK55.md, Q-875; branch feedback55).
B.group_by = meta((l, f) => { F(f, "group_by"); const out = {}; for (const x of L(l, "group_by")) { const r = cb(f, "group_by", x); if (r instanceof Err) return r; const k = keyText(r); if (!hasOwn(out, k)) mput(out, k, []); out[k].push(x); } return out; }, "group_by", null);
B.index_of = meta((c, x) => {
  if (Array.isArray(c)) { const i = c.findIndex((y) => eq(y, x)); return i < 0 ? null : i; }
  if (typeof c === "string") { const i = c.indexOf(display(x)); return i < 0 ? null : Array.from(c.slice(0, i)).length; }
  fail(`\`index_of\` expects a list or str, got ${typeName(c)}`);
}, "index_of", null);
B.repeat = meta((s, n) => { S(s, "repeat"); n = IN(n, "repeat", 2); if (n < 0 || s.length * n > 1e8) fail("`repeat` count is negative or too large"); return s.repeat(n); }, "repeat", null);
// What the program's ES modules give, by the program's name for it, or the error
// value saying why not: here, where the page's scripts cannot reach them or
// replace them (branch library-hints).
const MODS = new Map();
// `js("Chart.register", ...)`: a function of the page's scripts, found from
// the page's globals by its dotted path; `js_new` for a class.
// A name the page does not have is an error value, which code can check and
// say on the page (a library's <script> left out), not an error that stops it
// (FEEDBACK5.md, Q-054; branch feedback6-fixes).
function jsPath(path, f) {
  S(path, f);
  let owner = globalThis, v = globalThis;
  const parts = path.split(".");
  // A name a module gave the program is that, whatever the page has by the name.
  if (MODS.has(parts[0])) {
    v = MODS.get(parts.shift());
    if (v instanceof Err) return [null, null, v];
    owner = undefined;
  }
  for (const p of parts) {
    foreignName(p);
    if (v === null || v === undefined || !(p in Object(v))) return [null, null, new Err(`${f}: ${path} is not defined on this page${jsNear(v, p)}`, true)];
    owner = v;
    v = v[p];
  }
  return [owner, v, null];
}
// A name written in another case than the script's (`chart` for `Chart`):
// the one it has, to say (branch js-libraries).
function jsNear(v, p) {
  if (v === null || v === undefined) return "";
  const want = p.toLowerCase();
  try {
    for (const k of Object.keys(v)) if (k.toLowerCase() === want) return ` (did you mean ${k}?)`;
  } catch (_) {}
  return "";
}
// A class, which only `new` makes one of: `Chart.new(...)`, `js_new(...)`.
const isClass = (v) => { try { return /^class[\s{]/.test(Function.prototype.toString.call(v)); } catch (_) { return false; } };
B.js = meta((path, ...args) => {
  const [owner, v, missing] = jsPath(path, "js");
  if (missing) return missing;
  if (typeof v !== "function") {
    if (args.length) fail(`\`js\`: ${path} is not a function`);
    return v === undefined ? null : v;
  }
  if (isClass(v)) fail(`${path} is a class: make one with ${path}.new(...) (or js_new("${path}", ...))`);
  return fromPage(Reflect.apply(v, owner, guardCall(`\`js\`: ${path}`, v, args)));
}, "js", null);
// `Chart`, `Chart.defaults`: what a library's name (`client "...js" use Chart`)
// holds on the page, its functions not called (branch js-libraries).
B.js_value = (path) => {
  const [, v, missing] = jsPath(path, "js");
  if (missing) return missing;
  return fromPage(v);
};
B.js_new = meta((path, ...args) => {
  const [, v, missing] = jsPath(path, "js_new");
  if (missing) return missing;
  if (typeof v !== "function") fail(`\`js_new\`: ${path} is not a class`);
  return fromPage(Reflect.construct(v, guardCall(`\`js_new\`: ${path}`, v, args)));
}, "js_new", null);

// Translations: the page's locale and its text (see src/i18n.rs).
// What the page's own scripts (the loader, prefetching, translations, deferred parts) and the
// runtime hand each other, under one key of `window` rather than a name each; `quillo` and a
// page's `__quilloErrors` are the only names (FEEDBACK42.md, Q-486; branch feedback40-43).
const QW = typeof window === "undefined" ? {} : window[Symbol.for("quillo")] || (window[Symbol.for("quillo")] = {});
const i18n = () => QW.i18n || null;
// CLDR's operands of a number as it is written, as on the server (src/i18n.rs's `Operands`): i
// its integer digits (the last nine, `ib` when a digit before them is not 0), v how many digits
// it shows after the point, f those digits and t them without their zeros at the end (the last
// nine of each), `fb` when a digit of f's before its last nine is not 0 (FEEDBACK55.md, Q-823;
// branch feedback55).
const pluralOps = (int, frac) => {
  let k = frac.length;
  while (k > 0 && frac.charCodeAt(k - 1) === 48) k--;
  return { i: Number(int.slice(-9) || 0), ib: int.length > 9 && /[1-9]/.test(int.slice(0, -9)), v: frac.length, f: Number(frac.slice(-9) || 0),
    fb: frac.length > 9 && /[1-9]/.test(frac.slice(0, -9)), t: Number(frac.slice(Math.max(k - 9, 0), k) || 0) };
};
const pluralInt = (n) => ({ i: n % 1e9, ib: n >= 1e9, v: 0, f: 0, fb: false, t: 0 });
// Numeric text: digits with a sign, a point and an exponent (`12`, `-1.50`, `.5`, `5.`, `1e3`),
// the point moved by the exponent; null for any other (`0x1`, `Infinity`).
const pluralText = (s) => {
  const m = /^[+-]?(?:(\d+)(?:\.(\d*))?|\.(\d+))(?:[eE]([+-]?\d+))?$/.exec(s);
  if (!m) return null;
  const int = m[1] ?? "", frac = m[2] ?? m[3] ?? "", e = m[4] === undefined ? 0 : Number(m[4]);
  if (e === 0) return pluralOps(int, frac);
  const d = int + frac, p = int.length + e;
  if (p <= 0) return { ...pluralOps("", d), v: d.length - p };
  if (p >= d.length) return p - d.length >= 9 ? { ...pluralInt(0), ib: /[1-9]/.test(d) } : pluralOps(d + "0".repeat(p - d.length), "");
  return pluralOps(d.slice(0, p), d.slice(p));
};
// The languages with no plural forms, and those whose `one` is n = 1, as CLDR 47 lists them (old
// codes too: `in`, `jw`); src/i18n.rs's `plural_of` has the same (found in review of FEEDBACK55.md's
// Q-823).
const PLURAL_NONE = new Set("bm bo dz hnj id ig ii in ja jbo jv jw kde kea km ko lkt lo ms my nqo osa sah ses sg su th to tpi vi wo yo yue zh".split(" "));
const PLURAL_N1 = new Set(("af an asa az bal bem bez bg brx ce cgg chr ckb dv ee el eo eu fo fur gsw ha haw hu jgo jmc ka kaj kcg kk kkj kl ks ksb ku ky lb lg mas mgo ml mn "
  + "mr nah nb nd ne nn nnh no nr ny nyn om or os pap ps rm rof rwk saq sd sdh seh sn so sq ss ssy st syr ta te teo tig tk tn tr ts ug uz ve vo vun wae xh xog").split(" "));
// The plural form of a number by its operands, as on the server (CLDR 47's rules); any other
// language is read as English (FEEDBACK30.md, Q-366; branch feedback27-35-fixes).
const plural = (locale, o) => {
  const parts = String(locale).split(/[-_]/), lang = parts[0].toLowerCase(), region = (parts[1] || "").toUpperCase();
  const { i, ib, v, f, fb, t } = o, whole = f === 0 && !fb, n = (k) => whole && !ib && i === k, nIn = (a, b) => whole && !ib && i >= a && i <= b;
  const is = (k) => !ib && i === k, i10 = i % 10, i100 = i % 100, f10 = f % 10, f100 = f % 100, within = (x, a, b) => x >= a && x <= b;
  const millions = v === 0 && (ib || i !== 0) && i % 1000000 === 0;
  if (PLURAL_NONE.has(lang)) return "other";
  if (PLURAL_N1.has(lang)) return n(1) ? "one" : "other";
  switch (lang) {
    case "am": case "as": case "bn": case "doi": case "fa": case "gu": case "hi": case "kn": case "pcm": case "zu": return is(0) || n(1) ? "one" : "other";
    case "hy": case "ff": case "kab": return !ib && i <= 1 ? "one" : "other";
    case "ak": case "bho": case "guw": case "ln": case "mg": case "nso": case "pa": case "ti": case "wa": return nIn(0, 1) ? "one" : "other";
    // European Portuguese counts as Italian.
    case "pt": if (region !== "PT") return !ib && i <= 1 ? "one" : millions ? "many" : "other";
    // falls through
    case "it": case "ca": case "scn": case "vec": case "lld": return is(1) && v === 0 ? "one" : millions ? "many" : "other";
    case "fr": return !ib && i <= 1 ? "one" : millions ? "many" : "other";
    case "es": return n(1) ? "one" : millions ? "many" : "other";
    case "da": return n(1) || (!whole && !ib && i <= 1) ? "one" : "other";
    case "is": return (whole && i10 === 1 && i100 !== 11) || (t % 10 === 1 && t % 100 !== 11) ? "one" : "other";
    case "mk": return (v === 0 && i10 === 1 && i100 !== 11) || (f10 === 1 && f100 !== 11) ? "one" : "other";
    case "si": return n(0) || n(1) || (is(0) && f === 1 && !fb) ? "one" : "other";
    case "fil": case "tl": case "ceb": return (v === 0 && !ib && within(i, 1, 3)) || (v === 0 && i10 !== 4 && i10 !== 6 && i10 !== 9) || (v !== 0 && f10 !== 4 && f10 !== 6 && f10 !== 9) ? "one" : "other";
    case "lv": case "prg": return (whole && (i10 === 0 || within(i100, 11, 19))) || (v === 2 && within(f100, 11, 19)) ? "zero"
      : (whole && i10 === 1 && i100 !== 11) || (v === 2 && f10 === 1 && f100 !== 11) || (v !== 2 && f10 === 1) ? "one" : "other";
    case "he": case "iw": return (is(1) && v === 0) || (is(0) && v !== 0) ? "one" : is(2) && v === 0 ? "two" : "other";
    case "ro": case "mo": return is(1) && v === 0 ? "one" : v !== 0 || n(0) || (!n(1) && whole && within(i100, 1, 19)) ? "few" : "other";
    case "bs": case "hr": case "sr": case "sh": return (v === 0 && i10 === 1 && i100 !== 11) || (f10 === 1 && f100 !== 11) ? "one"
      : (v === 0 && within(i10, 2, 4) && !within(i100, 12, 14)) || (within(f10, 2, 4) && !within(f100, 12, 14)) ? "few" : "other";
    case "sl": return v !== 0 ? "few" : i100 === 1 ? "one" : i100 === 2 ? "two" : i100 === 3 || i100 === 4 ? "few" : "other";
    case "dsb": case "hsb": return (v === 0 && i100 === 1) || f100 === 1 ? "one" : (v === 0 && i100 === 2) || f100 === 2 ? "two" : (v === 0 && within(i100, 3, 4)) || within(f100, 3, 4) ? "few" : "other";
    case "cs": case "sk": return v !== 0 ? "many" : is(1) ? "one" : !ib && within(i, 2, 4) ? "few" : "other";
    case "pl": return v !== 0 ? "other" : is(1) ? "one" : within(i10, 2, 4) && !within(i100, 12, 14) ? "few" : "many";
    case "ru": case "uk": return v !== 0 ? "other" : i10 === 1 && i100 !== 11 ? "one" : within(i10, 2, 4) && !within(i100, 12, 14) ? "few" : "many";
    case "be": return !whole ? "other" : i10 === 1 && i100 !== 11 ? "one" : within(i10, 2, 4) && !within(i100, 12, 14) ? "few" : "many";
    case "lt": return !whole ? "many" : i10 === 1 && !within(i100, 11, 19) ? "one" : i10 >= 2 && !within(i100, 11, 19) ? "few" : "other";
    case "mt": return n(1) ? "one" : n(2) ? "two" : n(0) || (whole && within(i100, 3, 10)) ? "few" : whole && within(i100, 11, 19) ? "many" : "other";
    case "ga": return n(1) ? "one" : n(2) ? "two" : nIn(3, 6) ? "few" : nIn(7, 10) ? "many" : "other";
    case "gd": return n(1) || n(11) ? "one" : n(2) || n(12) ? "two" : nIn(3, 10) || nIn(13, 19) ? "few" : "other";
    case "ar": case "ars": return n(0) ? "zero" : n(1) ? "one" : n(2) ? "two" : whole && within(i100, 3, 10) ? "few" : whole && i100 >= 11 ? "many" : "other";
    case "cy": return n(0) ? "zero" : n(1) ? "one" : n(2) ? "two" : n(3) ? "few" : n(6) ? "many" : "other";
    case "iu": case "naq": case "sat": case "se": case "sma": case "smi": case "smj": case "smn": case "sms": return n(1) ? "one" : n(2) ? "two" : "other";
    default: return is(1) && v === 0 ? "one" : "other";
  }
};
// A count's operands, from the text `{count}` shows of it, as on the server: a whole float as
// the number it shows (`1`, as a component writes it), a decimal with its digits, numeric text
// trimmed as Rust trims (not `0x1`, which Number reads) (FEEDBACK30.md,
// Q-367; FEEDBACK55.md, Q-832; branch feedback55).
const pluralCount = (c, shown) => {
  if (typeof c === "number" || c instanceof Fl) {
    const x = numOf(c);
    if (Number.isSafeInteger(x)) return pluralInt(Math.abs(x));
    if (!Number.isFinite(x)) return null;
    // By its shortest decimal, as the server reads it, not its binary value: 1e23 is 1 and 23 zeros
    // (found in review of FEEDBACK55.md's Q-823). numText writes no exponent.
    const s = shown[0] === "-" ? shown.slice(1) : shown, at = s.indexOf(".");
    return at < 0 ? pluralOps(s, "") : pluralOps(s.slice(0, at), s.slice(at + 1));
  }
  if (typeof c === "bigint" || c instanceof Dec) return pluralText(shown);
  if (typeof c === "string") return pluralText(rsTrim(c));
  return null;
};
B.locale = meta(() => { const d = i18n(); return d ? d.locale : null; }, "locale", null);
const translate = (k, vars, esc) => {
  S(k, "t"); vars = M(vars, "t", 2);
  const d = i18n();
  let e = d && hasOwn(d.t, k) ? d.t[k] : k, shown = null;
  if (e && typeof e === "object") {
    // A number's text, made once: for its form and for `{count}`.
    const n = vars.count;
    if (typeof n === "number" || typeof n === "bigint" || n instanceof Fl || n instanceof Dec) shown = display(n);
    const c = pluralCount(n, shown);
    const cat = c === null ? "other" : c.i === 0 && !c.ib && c.v === 0 && hasOwn(e, "zero") ? "zero" : plural(d.locale, c);
    e = hasOwn(e, cat) ? e[cat] : (e.other ?? "");
  }
  return String(e).replace(/\{(\w+)\}/g, (m, name) => hasOwn(vars, name) ? esc(name === "count" && shown !== null ? shown : display(vars[name])) : m);
};
B.t = meta((k, vars) => translate(k, vars, (s) => s), "t", null);
// raw(t(key, vars)): the translation's markup, with the values put in escaped.
B.t_html = (k, vars) => rawHtml(translate(k, vars, B.escape));
B.url = meta(() => currentUrl(), "url", null);
B.css = meta((s) => S(s, "css"), "css", null);
// And `&` as `\26 `, as on the server, for a sheet in <svg> or <math> (FEEDBACK55.md, Q-707; branch feedback55).
B.__css_value = meta((v) => cssValue(display(v)).replace(/&/g, "\\26 "), "__css_value", null);
B.sql = meta((s) => S(s, "sql"), "sql", null);
// A run that the page changed under does not move it again: once the person followed a link, went
// Back or Forward, or another run moved the page, a `navigate` after a wait does nothing. A save
// that ended after the person had left took them on to where it led. A change of the hash alone
// keeps the page, and the run's own moves are its own (FEEDBACK61.md, Q-902; branch feedback61).
B.navigate = meta((to) => {
  to = S(to, "navigate");
  let r = Run;
  if (r !== null) { while (r.up) r = r.up; if (r.nav !== Nav.n) return null; }
  navigate(to);
  if (r !== null) r.nav = Nav.n;
  return null;
}, "navigate", null);
// `waiting(state)` is compiled to W.waiting(signal); given any other way, it
// knows of no load (FEEDBACK13.md, Q-091; branch feedback13-fixes).
B.waiting = meta(() => false, "waiting", null);
// `defer(x)` is `x` in the browser: a state that calls the server is null until it
// answers (src/defer.rs; branch defer).
B.defer = meta((v) => v, "defer", null);
B.json = {
  // An id in a document keeps its digits, where the browser says what they were
  // (FEEDBACK25.md, Q-301; branch feedback26-fixes).
  // Numbers as the server reads them (`jsonNum`); refused as there: one past a float's range
  // (`1e400`), and text with half of a character (FEEDBACK55.md, Q-782; branch feedback55).
  // At most 1,000 levels deep, as on the server (FEEDBACK55.md, Q-875's review; branch feedback55).
  parse: (s) => { S(s, "json.parse"); try { return fromJSON(parseData(s, (k, v) => {
    if ((typeof v === "string" && !wellFormed(v)) || !wellFormed(k)) throw new Error("half of a character (a lone surrogate)");
    if (typeof v === "number" && !Number.isFinite(v)) throw new Error("number out of range");
  }, null, true)); } catch (e) { return new Err("invalid JSON: " + e.message); } },
  // A function has no JSON: said, and where it is, as on the server (FEEDBACK55.md, Q-792;
  // branch feedback55).
  stringify: (v, pretty) => {
    v = visible(v);
    const at = fnIn(v, "", 0);
    if (at !== null) return new Err("json.stringify: " + (at === "" ? "a function cannot be written as JSON" : `\`${at.replace(/^\./, "")}\` is a function, which cannot be written as JSON`));
    return jsonText(v, truthy(pretty) ? 2 : undefined);
  },
};

// Where in `v` the first function is (`.items[2].f`), as the server's `json_into_within` meets it:
// null for none.
function fnIn(v, path, depth) {
  if (typeof v === "function") return path;
  if (depth > 10000 || v === null || typeof v !== "object") return null;
  if (Array.isArray(v)) { for (let i = 0; i < v.length; i++) { const r = fnIn(v[i], path + "[" + i + "]", depth + 1); if (r !== null) return r; } return null; }
  if (v instanceof Err) return fnIn(v.error, path + ".error", depth + 1);
  if (!isMap(v)) return null;
  // The first in the order set (FEEDBACK55.md, Q-875; branch feedback55).
  for (const k of mkeys(v)) {
    const key = fmtKey(k);
    const r = fnIn(v[k], path + (key.startsWith('"') ? "[" + key + "]" : "." + k), depth + 1);
    if (r !== null) return r;
  }
  return null;
}

function fromJSON(j) {
  if (j === undefined) return null;
  return j;
}
// A JSON number as the server reads it: written with a point or an exponent, a float (an Fl when
// whole); digits alone, an int, exact past 2^53, and past 64 bits a float. Without the text it
// was written as (a browser whose JSON.parse does not give it), a whole one is an int
// (FEEDBACK55.md, Q-775 and Q-776; branch feedback55).
function jsonNum(v, c) {
  if (!c || !Number.isInteger(v)) return v;
  const t = c.source;
  // `-0` too is the float -0.0 there.
  if (/[.eE]/.test(t) || Object.is(v, -0)) return new Fl(v);
  if (Number.isSafeInteger(v)) return v;
  const b = BigInt(t);
  return b >= I64_MIN && b <= I64_MAX ? intOf(b) : new Fl(v);
}
const wellFormed = (s) => (s.isWellFormed ? s.isWellFormed() : !/[\ud800-\udfff]/.test(s.replace(/[\ud800-\udbff][\udc00-\udfff]/g, "")));
// JSON from the server, or from another site, with its numbers read as the server reads them;
// `limit`: at most 128 levels deep (`JSON_DEEP`).
const readJSON = (text, limit) => parseData(text, null, null, limit);
// Whether JSON.parse gives its reviver the text each number was written as (Chrome 114, Firefox
// 135, Safari 18.4 on), asked once.
let jsonSource = null;
const hasJsonSource = () => jsonSource ?? (jsonSource = (() => {
  try { let ok = false; JSON.parse("[1.0]", (k, v, c) => { if (typeof v === "number") ok = !!c && c.source === "1.0"; return v; }); return ok; } catch (_) { return false; }
})());
// A string, or a number, in JSON text.
const JSON_TOKEN = /"(?:[^"\\]|\\[^])*"|-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?/g;
// Whether a number's text says more than its value: a whole float (`2.0`, `1e2`, `-0`), an int
// past 2^53.
const tellsType = (t) => { const n = Number(t); return /[.eE]/.test(t) ? Number.isInteger(n) : !Number.isSafeInteger(n) || Object.is(n, -0); };
let numKey = null;
// JSON text parsed, each number as the server reads it (`jsonNum`); `check(key, value)` sees each
// value as it comes (and may throw), `after(value)` makes each one that is no number. In a browser
// whose JSON.parse does not give the text a number was written as (Safari before 18.4, Firefox
// before 135), the numbers whose text says more are put first in objects under a key no text has,
// and made from their text here: there whole floats were ints, and ints past 2^53 lost digits
// (found in review of FEEDBACK55.md's Q-775 and Q-776; branch feedback55).
// A map read from text with a key JavaScript puts first (an array index) is given the order its
// keys were written in, as the server reads it: from the text, read for its keys, which costs a
// fifth of JSON.parse; marking such keys for JSON.parse made it four times as slow. Whether one is
// there, and how deep the text is, the reviver JSON.parse calls on each value already sees
// (`jsonWalk`): reading the text for them first made json.parse of 1 MB a tenth slower
// (FEEDBACK55.md, Q-875 and its reviews; branch feedback55).
// `limit`: refused past 128 levels, as the server refuses JSON a program reads.
function parseData(text, check, after, limit) {
  const walk = jsonWalk(limit);
  let v;
  try {
    v = parseNums(text, check, after, walk);
  } catch (e) {
    // Far past 128: JSON.parse ran out of stack before the reviver saw the first level.
    if (limit && e instanceof RangeError) throw new SyntaxError(JSON_DEEP);
    throw e;
  }
  if (walk.idx && v !== null && typeof v === "object") keyOrder(v, jsonShape(text));
  return v;
}
// How deep JSON the program reads may nest, and what is said past it, as the server's
// `json_too_deep` (FEEDBACK55.md, Q-875's second review; branch feedback55).
const JSON_DEEP = "it nests more than 128 deep";
// What the reviver of JSON.parse sees of each value (`see(holder, key, value)`), which comes after
// the values inside it: how deep the text is, an error past `limit`, and whether a map has an
// array index (`idx`). Each holder is a level, with its deepest so far, and whether it is a list.
function jsonWalk(limit) {
  const holders = [], heights = [], lists = [];
  let top = -1, idx = false;
  return {
    get idx() { return idx; },
    see(holder, k, v) {
      let h = 0;
      if (typeof v === "object" && v !== null) {
        if (top >= 0 && holders[top] === v) { h = heights[top] + 1; holders[top--] = null; } else h = 1;
        if (limit && h > 128) throw new SyntaxError(JSON_DEEP);
      }
      if (top >= 0 && holders[top] === holder) {
        if (h > heights[top]) heights[top] = h;
        if (!idx && !lists[top] && isIdx(k)) idx = true;
      } else {
        const list = Array.isArray(holder);
        holders[++top] = holder; heights[top] = h; lists[top] = list;
        if (!idx && !list && isIdx(k)) idx = true;
      }
    },
  };
}
// The objects and lists of JSON text `t`, which JSON.parse took, that hold a map with an array
// index in another place than JavaScript puts it (null for the rest): each such map's keys in the
// order written, a key written twice in its first place (its value the last one, as JSON.parse
// takes it). Array indexes first, by number, are where JavaScript has them: such a map needs no
// order of its own, and is read without taking its keys out of the text.
function jsonShape(t) {
  let i = 0;
  const ws = () => { for (;;) { const c = t.charCodeAt(i); if (c === 32 || c === 9 || c === 10 || c === 13) i++; else return; } };
  // The closing quote of the string at `i`: one after an even number of backslashes.
  const strEnd = () => {
    let e = i;
    for (;;) {
      e = t.indexOf("\"", e + 1);
      if (e < 0) return t.length;
      let b = e - 1;
      while (t.charCodeAt(b) === 92) b--;
      if ((e - b) % 2 === 1) return e;
    }
  };
  // The key from `s` to its closing quote `e`, as text.
  const keyAt = (s, e) => {
    for (let j = s + 1; j < e; j++) if (t.charCodeAt(j) === 92) return JSON.parse(t.slice(s, e + 1));
    return t.slice(s + 1, e);
  };
  // The array index that key is, or -1.
  const idxAt = (s, e) => {
    let x = 0;
    for (let j = s + 1; j < e; j++) {
      const c = t.charCodeAt(j);
      if (c === 92) return escIdx(s, e);
      if (c < 48 || c > 57) return -1;
      x = x * 10 + c - 48;
    }
    const n = e - s - 1;
    return n >= 1 && n <= 10 && (n === 1 || t.charCodeAt(s + 1) !== 48) && x <= 4294967294 ? x : -1;
  };
  const escIdx = (s, e) => { const k = keyAt(s, e); return isIdx(k) ? +k : -1; };
  // An object read for its keys, from its `{`: one whose keys are in JavaScript's order needs none
  // of its own, and is read without taking them out of the text; one out of it is read again from
  // its start (`ordObj`).
  const obj = () => {
    const start = i;
    i++; ws();
    if (t.charCodeAt(i) === 125) { i++; return null; }
    let kids = null, named = false, last = -1;
    // (Text JSON.parse took: the ends are there, but never read past them.)
    while (i < t.length) {
      ws();
      const s = i, e = strEnd();
      i = e + 1;
      const x = idxAt(s, e);
      if (x >= 0) {
        if (named || x <= last) { i = start; return ordObj(); }
        last = x;
      } else named = true;
      ws(); i++;
      const n = val();
      if (n !== null) (kids || (kids = new Map())).set(keyAt(s, e), n); else if (kids !== null) kids.delete(keyAt(s, e));
      ws();
      if (t.charCodeAt(i++) === 125) break;
    }
    return kids !== null ? { keys: null, kids, items: null } : null;
  };
  const ordObj = () => {
    i++;
    const seen = [];
    let kids = null;
    while (i < t.length) {
      ws();
      const s = i, e = strEnd();
      i = e + 1;
      const k = keyAt(s, e);
      ws(); i++;
      const n = val();
      keyIn(seen, k);
      if (n !== null) (kids || (kids = new Map())).set(k, n); else if (kids !== null) kids.delete(k);
      ws();
      if (t.charCodeAt(i++) === 125) break;
    }
    return { keys: seen, kids, items: null };
  };
  // Key k put in `seen` when not there yet: looked for there in a small map, in a set past it.
  const keyIn = (seen, k) => {
    if (seen.length < 16) { if (!seen.includes(k)) seen.push(k); return; }
    const has = seen.$has || (seen.$has = new Set(seen));
    if (!has.has(k)) { has.add(k); seen.push(k); }
  };
  const val = () => {
    ws();
    const c = t.charCodeAt(i);
    if (c === 123) return obj();
    if (c === 91) {
      i++; ws();
      if (t.charCodeAt(i) === 93) { i++; return null; }
      let items = null, at = 0;
      while (i < t.length) {
        const n = val();
        if (n !== null) (items || (items = [])).push([at, n]);
        at++; ws();
        if (t.charCodeAt(i++) === 93) break;
      }
      return items === null ? null : { keys: null, kids: null, items };
    }
    if (c === 34) { i = strEnd() + 1; return null; }
    while (i < t.length) { const d = t.charCodeAt(i); if (d <= 32 || d === 44 || d === 93 || d === 125) break; i++; }
    return null;
  };
  return val();
}
// The order `jsonShape` found put on what JSON.parse made of the text: an error value (`$quillo_err`)
// holds what it was written with.
function keyOrder(v, n) {
  if (n === null || v === null || typeof v !== "object") return;
  if (n.items !== null) { if (Array.isArray(v)) for (const [i, c] of n.items) keyOrder(v[i], c); return; }
  if (v instanceof Err) { if (n.kids !== null && n.kids.has("$quillo_err")) keyOrder(v.error, n.kids.get("$quillo_err")); return; }
  if (!isMap(v)) return;
  if (n.keys !== null) ordered(v, n.keys);
  if (n.kids !== null) for (const [k, c] of n.kids) if (hasOwn(v, k)) keyOrder(v[k], c);
}
// `parseData`'s numbers; `walk` sees each value (see `jsonWalk`).
function parseNums(text, check, after, walk) {
  if (hasJsonSource()) return JSON.parse(text, function (k, v, c) { if (check) check(k, v); walk.see(this, k, v); return typeof v === "number" ? jsonNum(v, c) : after ? after(v) : v; });
  if (numKey === null) numKey = "\u0000n" + randomTag();
  const key = numKey, written = "\\u0000n" + key.slice(2);
  const marked = text.replace(JSON_TOKEN, (t) => (t[0] === '"' || !tellsType(t) ? t : `{"${written}":"${t}"}`));
  return JSON.parse(marked, function (k, v) {
    if (k === key) return v;
    if (v !== null && typeof v === "object" && typeof v[key] === "string") { const n = Number(v[key]); if (check) check(k, n); walk.see(this, k, n); return jsonNum(n, { source: v[key] }); }
    if (check) check(k, v);
    walk.see(this, k, v);
    return after ? after(v) : v;
  });
}

// `User(fields)`: a User, or an error value saying what is wrong.
B.__record = meta((name, v) => {
  const td = OWN_TYPES[name];
  if (!td) fail(`there is no type \`${name}\``);
  if (td.r && isNil(v)) v = {};
  if (td.r && !isMap(v)) return new Err(`${name}: expects a map of its fields, got ${typeName(v)} ${short(v)}`, true);
  const r = ownMake(ownTyOf(name), v);
  if (r.ok) return r.v;
  return new Err(`${name}: ${ownWhyText(ownWhyHere(r.w, { t: td.a ? td.a.t : name }, v))}`, true);
}, "__record", null);

// ---- re (src/re.rs, the same matches) ---------------------------------------
// Patterns are what JavaScript (with the u flag) and the server's regex
// crate mean the same by; reCheck turns away the rest as translate in
// re.rs does, with the same reasons. Positions count characters.

function reCheck(p) {
  const n = p.length;
  const names = [];
  const hex = (s) => /^[0-9A-Fa-f]+$/.test(s);
  // \u escapes: [code point, length] (a surrogate pair counts as one).
  const unicode = (i) => {
    const bad = "`\\u` takes four hex digits or {hex}, as in \\u00e9 or \\u{1F600}";
    if (p[i + 2] === "{") {
      const end = p.indexOf("}", i + 3);
      const h = end < 0 ? "" : p.slice(i + 3, end);
      if (h.length < 1 || h.length > 6 || !hex(h) || parseInt(h, 16) > 0x10ffff || (parseInt(h, 16) >= 0xd800 && parseInt(h, 16) < 0xe000)) throw bad;
      return [parseInt(h, 16), end + 1 - i];
    }
    const h = p.slice(i + 2, i + 6);
    if (h.length !== 4 || !hex(h)) throw bad;
    const hi = parseInt(h, 16);
    if (hi >= 0xd800 && hi < 0xdc00) {
      const l = p.slice(i + 8, i + 12);
      if (p[i + 6] === "\\" && p[i + 7] === "u" && l.length === 4 && hex(l) && parseInt(l, 16) >= 0xdc00 && parseInt(l, 16) < 0xe000) {
        return [0x10000 + ((hi - 0xd800) << 10) + (parseInt(l, 16) - 0xdc00), 12];
      }
      throw "half of a surrogate pair on its own";
    }
    if (hi >= 0xdc00 && hi < 0xe000) throw "half of a surrogate pair on its own";
    return [hi, 6];
  };
  // An escape: [the character it stands for, or null for a set; length; assertion].
  const escape = (i, inClass) => {
    if (i + 1 >= n) throw "the pattern ends with `\\`";
    const e = p[i + 1];
    if ("dDwWsS".includes(e)) return [null, 2, false];
    if (e === "b" || e === "B") { if (inClass) throw "`\\b` cannot go in [...]"; return [null, 2, true]; }
    const plain = { n: 10, t: 9, r: 13, f: 12, v: 11 };
    if (e in plain) return [plain[e], 2, false];
    if (e === "0" && !/[0-9]/.test(p[i + 2] || "")) return [0, 2, false];
    if (e === "x") {
      const h = p.slice(i + 2, i + 4);
      if (h.length !== 2 || !hex(h)) throw "`\\x` takes two hex digits, as in \\x41";
      return [parseInt(h, 16), 4, false];
    }
    if (e === "u") { const [c, len] = unicode(i); return [c, len, false]; }
    if (/[1-9k]/.test(e)) throw "backreferences (\\1, \\k<name>) are not supported";
    if (e === "p" || e === "P") throw "Unicode classes (\\p{...}) are not supported";
    if ("^$\\.*+?()[]{}|/".includes(e) || (e === "-" && inClass)) return [e.codePointAt(0), 2, false];
    throw `\`\\${String.fromCodePoint(p.codePointAt(i + 1))}\` is not one of the escapes patterns have`;
  };
  const cls = (i) => {
    let j = i + 1;
    if (p[j] === "^") j++;
    if (p[j] === "]") throw "an empty class `[]` matches nothing: write `\\[\\]` for the brackets";
    const item = () => {
      if (p[j] === "\\") { const [c, len] = escape(j, true); j += len; return c; }
      if (p[j] === "[") throw "write `\\[` for a `[` in [...]";
      const c = p[j];
      if ((c === "&" || c === "~" || c === "-") && p[j + 1] === c) throw `write \`${c}\\${c}\` for two \`${c}\` in [...]`;
      const cp = p.codePointAt(j);
      j += cp > 0xffff ? 2 : 1;
      return cp;
    };
    for (;;) {
      if (j >= n) throw "`[` without its `]`";
      if (p[j] === "]") return j + 1 - i;
      const lo = item();
      if (p[j] === "-" && j + 1 < n && p[j + 1] !== "]") {
        j++;
        const hi = item();
        if (lo === null || hi === null) throw "a range in [...] is between two characters, not \\d, \\w or \\s";
        if (lo > hi) throw "a range in [...] goes from the lower character to the higher";
      }
    }
  };
  const quantifier = (i) => {
    if (p[i] !== "{") return 1;
    const lone = "`{` on its own: write `\\{`, or repeat with {n}, {n,} or {n,m}";
    const m = /^\{(\d+)(,(\d*))?\}/.exec(p.slice(i));
    if (!m) throw lone;
    const lo = Number(m[1]), hi = m[2] === undefined ? lo : m[3] === "" ? null : Number(m[3]);
    if (lo > 1000 || (hi !== null && hi > 1000)) throw "repeating more than 1000 times is not supported";
    if (hi !== null && hi < lo) throw "in {n,m}, n is more than m";
    return m[0].length;
  };
  try {
    let i = 0, atom = false, depth = 0;
    while (i < n) {
      const c = p[i];
      if (c === "\\") { const [, len, assertion] = escape(i, false); i += len; atom = !assertion; }
      else if (c === "[") { i += cls(i); atom = true; }
      else if (c === "(") {
        if (p[i + 1] === "?") {
          const a = p[i + 2], b = p[i + 3];
          if (a === ":") i += 3;
          else if (a === "<" && b !== undefined && b !== "=" && b !== "!") {
            const m = /^[A-Za-z0-9_]*/.exec(p.slice(i + 3))[0];
            if (m === "" || /^[0-9]/.test(m) || p[i + 3 + m.length] !== ">") throw "a group's name is letters, digits and `_`, as in (?<name>...)";
            if (names.includes(m)) throw `two groups are named \`${m}\``;
            names.push(m);
            i += 4 + m.length;
          } else if (a === "=" || a === "!" || a === "<") throw "lookahead and lookbehind ((?= (?! (?<= (?<!) are not supported";
          else throw "flags go in the last argument (\"i\", \"m\", \"s\"), not in the pattern";
        } else i++;
        depth++;
        atom = false;
      } else if (c === ")") { if (depth === 0) throw "`)` without its `(`"; depth--; i++; atom = true; }
      else if (c === "|" || c === "^" || c === "$") { i++; atom = false; }
      else if ("*+?{".includes(c)) {
        const len = quantifier(i);
        if (!atom) throw `nothing to repeat before \`${c}\``;
        i += len;
        if (p[i] === "?") i++;
        atom = false;
      } else if (c === "]" || c === "}") throw `\`${c}\` on its own: write \`\\${c}\``;
      else { i += p.codePointAt(i) > 0xffff ? 2 : 1; atom = true; }
    }
    if (depth > 0) throw "`(` without its `)`";
  } catch (why) {
    if (typeof why === "string") return why;
    throw why;
  }
  return null;
}

// `^` and `$` (outside a class, unescaped) as the server's multi-line ones.
function reLines(p) {
  let out = "", cls = false;
  for (let i = 0; i < p.length; i++) {
    const c = p[i];
    if (c === "\\") { out += c + (p[i + 1] ?? ""); i++; }
    else if (cls) { if (c === "]") cls = false; out += c; }
    else if (c === "[") { cls = true; out += c; }
    else if (c === "^") out += "(?<![^\\n\\r])(?!(?<=\\r)\\n)";
    else if (c === "$") out += "(?![^\\n\\r])(?!(?<=\\r)\\n)";
    else out += c;
  }
  return out;
}
const reCache = new Map();
function reOf(p, flags, f, at) {
  S(p, f, 2);
  flags = isNil(flags) ? "" : S(flags, f, at);
  const key = flags + "/" + p;
  let r = reCache.get(key);
  if (!r) {
    if ([...flags].some((c, k) => !"ims".includes(c) || flags.indexOf(c) !== k)) {
      fail(`\`${f}\`: the flags are \`i\` (any case), \`m\` (^ and $ at each line) and \`s\` (. matches line breaks), each once; got "${flags}"`);
    }
    const why = reCheck(p);
    if (why) fail(`\`${f}\`: ${why}`);
    if (reCache.size >= 256) reCache.clear();
    // With `m`, a line ends at \n or \r, and \r\n is one end, as the server's regex has it
    // (`(?mR)`): JavaScript's `m` ended one at U+2028 and U+2029 too, and between \r and \n
    // (FEEDBACK55.md, Q-782; branch feedback55).
    r = flags.includes("m") ? new RegExp(reLines(p), "gu" + flags.replace("m", "")) : new RegExp(p, "gu" + flags);
    reCache.set(key, r);
  }
  return r;
}

// The matches from left to right (as matchAll; after an empty one, a
// character later), with positions in UTF-16 (i, j) and in characters.
function reAll(s, r, most) {
  const out = [];
  let pos = 0, at = 0, chars = 0;
  const count = (to) => { for (; at < to; at++) { const c = s.charCodeAt(at); if (!(c >= 0xdc00 && c < 0xe000 && at > 0 && s.charCodeAt(at - 1) >= 0xd800 && s.charCodeAt(at - 1) < 0xdc00)) chars++; } return chars; };
  while (pos <= s.length && out.length < most) {
    r.lastIndex = pos;
    const m = r.exec(s);
    if (!m) break;
    const i = m.index, j = i + m[0].length;
    out.push({ m, i, j, start: count(i), end: count(j) });
    pos = j > i ? j : j + (s.codePointAt(j) > 0xffff ? 2 : 1);
  }
  return out;
}

const reValue = (x) => ({
  text: x.m[0], start: x.start, end: x.end,
  groups: x.m.slice(1).map((g) => g === undefined ? null : g),
  named: Object.fromEntries(Object.entries(x.m.groups || {}).map(([k, g]) => [k, g === undefined ? null : g])),
});

function reExpand(t, x) {
  const groups = x.m.length - 1;
  const names = Object.keys(x.m.groups || {});
  let out = "";
  for (let k = 0; k < t.length; k++) {
    const c = t[k];
    if (c !== "$") { out += c; continue; }
    const d = t[k + 1];
    if (d === "$") { out += "$"; k++; }
    else if (d !== undefined && d >= "0" && d <= "9") {
      const g = Number(d);
      if (g > groups) fail(`\`re.replace\`: \`$${g}\`, but the pattern has ${groups} group${groups === 1 ? "" : "s"}`);
      out += x.m[g] ?? "";
      k++;
    } else if (d === "<") {
      const end = t.indexOf(">", k);
      if (end < 0) fail("`re.replace`: `$<` without its `>`");
      const name = t.slice(k + 2, end);
      if (!names.includes(name)) fail(`\`re.replace\`: no group is named \`${name}\``);
      out += (x.m.groups && x.m.groups[name]) ?? "";
      k = end;
    } else out += "$";
  }
  return out;
}

B.re = {
  valid: (p, flags) => { if (typeof p !== "string") return false; try { reOf(p, flags, "re.valid", 2); return true; } catch (e) { return false; } },
  test: (s, p, flags) => { S(s, "re.test"); return reAll(s, reOf(p, flags, "re.test", 3), 1).length > 0; },
  find: (s, p, flags) => { S(s, "re.find"); const [x] = reAll(s, reOf(p, flags, "re.find", 3), 1); return x ? reValue(x) : null; },
  find_all: (s, p, flags) => { S(s, "re.find_all"); return reAll(s, reOf(p, flags, "re.find_all", 3), Infinity).map(reValue); },
  replace: (s, p, w, flags) => {
    S(s, "re.replace");
    const r = reOf(p, flags, "re.replace", 4);
    const fn = typeof w === "function";
    want(fn || typeof w === "string", "re.replace", "what to put in (a str or a function)", 3, w);
    if (fn) denied(w);
    let out = "", last = 0;
    for (const x of reAll(s, r, Infinity)) {
      // The function's error is given back (FEEDBACK55.md, Q-784; branch feedback55).
      const v = fn ? cb(w, "re.replace", reValue(x)) : null;
      if (v instanceof Err) return v;
      out += s.slice(last, x.i) + (fn ? display(v) : reExpand(w, x));
      last = x.j;
    }
    return out + s.slice(last);
  },
  split: (s, p, flags) => {
    S(s, "re.split");
    const out = [];
    let last = 0;
    for (const x of reAll(s, reOf(p, flags, "re.split", 3), Infinity)) { out.push(s.slice(last, x.i)); last = x.j; }
    out.push(s.slice(last));
    return out;
  },
};

// ---- money (src/money.rs, the same answers) ------------------------------
// An amount of a currency, as a map { amount: dec, currency: "USD" }, with
// the currency's ISO 4217 minor units; written as CLDR has it for a locale.

const MONEY_CURRENCIES = "AED2 AFN2 ALL2 AMD2 AOA2 ARS2 AUD2 AWG2 AZN2 BAM2 BBD2 BDT2 BHD3 BIF0 BMD2 BND2 BOB2 BOV2 BRL2 BSD2 " +
  "BTN2 BWP2 BYN2 BZD2 CAD2 CDF2 CHE2 CHF2 CHW2 CLF4 CLP0 CNY2 COP2 COU2 CRC2 CUC2 CUP2 CVE2 CZK2 DJF0 " +
  "DKK2 DOP2 DZD2 EGP2 ERN2 ETB2 EUR2 FJD2 FKP2 GBP2 GEL2 GHS2 GIP2 GMD2 GNF0 GTQ2 GYD2 HKD2 HNL2 HTG2 " +
  "HUF2 IDR2 ILS2 INR2 IQD3 IRR2 ISK0 JMD2 JOD3 JPY0 KES2 KGS2 KHR2 KMF0 KPW2 KRW0 KWD3 KYD2 KZT2 LAK2 " +
  "LBP2 LKR2 LRD2 LSL2 LYD3 MAD2 MDL2 MGA2 MKD2 MMK2 MNT2 MOP2 MRU2 MUR2 MVR2 MWK2 MXN2 MXV2 MYR2 MZN2 " +
  "NAD2 NGN2 NIO2 NOK2 NPR2 NZD2 OMR3 PAB2 PEN2 PGK2 PHP2 PKR2 PLN2 PYG0 QAR2 RON2 RSD2 RUB2 RWF0 SAR2 " +
  "SBD2 SCR2 SDG2 SEK2 SGD2 SHP2 SLE2 SOS2 SRD2 SSP2 STN2 SVC2 SYP2 SZL2 THB2 TJS2 TMT2 TND3 TOP2 TRY2 " +
  "TTD2 TWD2 TZS2 UAH2 UGX0 USD2 USN2 UYI0 UYU2 UYW4 UZS2 VED2 VES2 VND0 VUV0 WST2 XAF0 XCD2 XCG2 XOF0 " +
  "XPF0 YER2 ZAR2 ZMW2 ZWG2";
// Withdrawn since 2007: amounts kept in them still read (FEEDBACK50.md, Q-630; branch
// feedback50-fixes).
// The decimals CLDR writes where they are not the ISO 4217 minor units: what money.format
// shows (FEEDBACK50.md, Q-627; branch feedback50-fixes).
const MONEY_DISPLAY = "AFN0 ALL0 IQD0 IRR0 KPW0 LAK0 LBP0 MGA0 MMK0 RSD0 SOS0 SYP0 YER0 MRO0 SLL0 STD0 TMM0 ZMK0 ZWD0";
const MONEY_WITHDRAWN = "ANG2 BGN2 BYR0 CYP2 EEK2 GHC2 HRK2 LTL2 LVL2 MRO2 MTL2 SDD2 SIT2 SKK2 SLL2 STD2 TMM2 VEB2 VEF2 ZMK2 ZWD2 ZWL2 ZWN2 ZWR2";
const MONEY_SYMBOLS = "AUD=A$,BRL=R$,CAD=CA$,CNY=CN\u00a5,EUR=\u20ac,GBP=\u00a3,HKD=HK$,ILS=\u20aa,INR=\u20b9,JPY=JP\u00a5,KRW=\u20a9,MXN=MX$,NZD=NZ$,TWD=NT$,USD=US$,VND=\u20ab,XAF=FCFA,XCD=EC$,XCG=Cg.,XOF=F\u202fCFA,XPF=CFPF";
const MONEY_LOCALES = "en-US\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tJPY=\u00a5,PHP=\u20b1,USD=$\n" +
  "en-GB\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tPHP=\u20b1\n" +
  "en-CA\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tCAD=$,PHP=\u20b1\n" +
  "en-AU\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tAUD=$,BRL=BRL,CAD=CAD,CNY=CNY,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,SCR=Rs,TWD=TWD,USD=USD,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF,XPF=CFP\n" +
  "en-NZ\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tNZD=$,PHP=\u20b1\n" +
  "en-IE\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tPHP=\u20b1\n" +
  "en-IN\t.\t,\t\t\t-\t5\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tPHP=\u20b1,USD=$\n" +
  "en-SG\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tPHP=\u20b1,SGD=$\n" +
  "en-ZA\t,\t\u00a0\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tPHP=\u20b1,ZAR=R\n" +
  "de-DE\t,\t.\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tAUD=AU$,JPY=\u00a5,THB=\u0e3f,USD=$\n" +
  "de-AT\t,\t.\t\t\u00a0\t-\t0\t\u00a4\u00a0#\t-\u00a4\u00a0#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tAUD=AU$,JPY=\u00a5,THB=\u0e3f,USD=$\n" +
  "de-CH\t.\t\u2019\t\t\t-\t0\t\u00a4\u00a0#\t\u00a4-#\t\u00a4\u00a0#\t\u00a4-#\tAUD=AU$,EUR=EUR,JPY=\u00a5,THB=\u0e3f,USD=$\n" +
  "fr-FR\t,\t\u202f\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tARS=$AR,AUD=$AU,BMD=$BM,BND=$BN,BZD=$BZ,CAD=$CA,CLP=$CL,CNY=CNY,COP=$CO,FJD=$FJ,FKP=\u00a3FK,GBP=\u00a3GB,GIP=\u00a3GI,HKD=HKD,JPY=JPY,LBP=\u00a3LB,MXN=$MX,NAD=$NA,NZD=$NZ,SBD=$SB,SGD=$SG,SRD=$SR,TTD=$TT,TWD=TWD,USD=$US,UYU=$UY,WST=$WS,XCD=XCD,XPF=FCFP,CYP=\u00a3CY\n" +
  "fr-BE\t,\t\u202f\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tARS=$AR,AUD=$AU,BMD=$BM,BND=$BN,BZD=$BZ,CAD=$CA,CLP=$CL,CNY=CNY,COP=$CO,FJD=$FJ,FKP=\u00a3FK,GBP=\u00a3GB,GIP=\u00a3GI,HKD=HKD,JPY=JPY,LBP=\u00a3LB,MXN=$MX,NAD=$NA,NZD=$NZ,SBD=$SB,SGD=$SG,SRD=$SR,TTD=$TT,TWD=TWD,USD=$US,UYU=$UY,WST=$WS,XCD=XCD,XPF=FCFP,CYP=\u00a3CY\n" +
  "fr-CA\t,\t\u00a0\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tAUD=$\u00a0AU,CAD=$,HKD=$\u00a0HK,ILS=ILS,INR=INR,JPY=\u00a5,KRW=KRW,MXN=MXN,NZD=$\u00a0NZ,SGD=$\u00a0SG,TWD=TWD,USD=$\u00a0US,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF,XPF=XPF,CYP=\u00a3CY\n" +
  "fr-CH\t.\t\u202f\t,\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tARS=$AR,AUD=$AU,BMD=$BM,BND=$BN,BZD=$BZ,CAD=$CA,CLP=$CL,CNY=CNY,COP=$CO,FJD=$FJ,FKP=\u00a3FK,GBP=\u00a3GB,GIP=\u00a3GI,HKD=HKD,JPY=JPY,LBP=\u00a3LB,MXN=$MX,NAD=$NA,NZD=$NZ,SBD=$SB,SGD=$SG,SRD=$SR,TTD=$TT,TWD=TWD,USD=$US,UYU=$UY,WST=$WS,XCD=XCD,XPF=FCFP,CYP=\u00a3CY\n" +
  "it-IT\t,\t.\t\t\t-\t2\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tBRL=BRL,HKD=HKD,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,PHP=\u20b1,THB=\u0e3f,TWD=TWD,USD=USD,VND=VND\n" +
  "it-CH\t.\t\u2019\t\t\t-\t2\t\u00a4\u00a0#\t\u00a4-#\t\u00a4\u00a0#\t\u00a4-#\tBRL=BRL,HKD=HKD,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,PHP=\u20b1,THB=\u0e3f,TWD=TWD,USD=USD,VND=VND\n" +
  "es-ES\t,\t.\t\t\t-\t2\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,THB=\u0e3f,TWD=TWD,XAF=XAF,XCD=XCD,XOF=XOF\n" +
  "es-MX\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MRU=UM,MXN=$,NZD=NZD,TWD=TWD,USD=USD,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF,MRO=MRU\n" +
  "es-AR\t,\t.\t\t\t-\t0\t\u00a4\u00a0#\t-\u00a4\u00a0#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tARS=$,AUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF\n" +
  "pt-BR\t,\t.\t\t\t-\t0\t\u00a4\u00a0#\t-\u00a4\u00a0#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tAUD=AU$,THB=\u0e3f\n" +
  "pt-PT\t,\t\u00a0\t\t\t-\t2\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tAUD=AU$,THB=\u0e3f\n" +
  "nl-NL\t,\t.\t\t\t-\t0\t\u00a4\u00a0#\t\u00a4\u00a0-#\t\u00a4\u00a0#\t\u00a4\u00a0-#\tAUD=AU$,CAD=C$,FJD=FJ$,SBD=SI$,THB=\u0e3f,XPF=XPF\n" +
  "nl-BE\t,\t.\t\t\t-\t0\t\u00a4\u00a0#\t\u00a4\u00a0-#\t\u00a4\u00a0#\t\u00a4\u00a0-#\tAUD=AU$,CAD=C$,FJD=FJ$,SBD=SI$,THB=\u0e3f,XPF=XPF\n" +
  "sv-SE\t,\t\u00a0\t\t\t\u2212\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tAUD=AUD,BBD=Bds$,BMD=BM$,BRL=BR$,BSD=BS$,BZD=BZ$,CNY=CNY,DKK=Dkr,DOP=RD$,EGP=EG\u00a3,GBP=GBP,HKD=HKD,INR=INR,ISK=Ikr,JMD=JM$,JPY=JPY,KRW=KRW,NOK=Nkr,NZD=NZD,SEK=kr,TWD=TWD,VND=VND,EEK=Ekr\n" +
  "nb-NO\t,\t\u00a0\t\t\t\u2212\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NOK=kr,NZD=NZD,TWD=TWD,USD=USD,VND=VND,XAF=XAF,XCD=XCD,XPF=XPF\n" +
  "da-DK\t,\t.\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tAUD=AU$,DKK=kr.,THB=\u0e3f\n" +
  "fi-FI\t,\t\u00a0\t\t\t\u2212\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,HKD=HKD,ILS=ILS,INR=INR,JPY=\u00a5,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,USD=$,VND=VND,XCD=XCD,XPF=XPF\n" +
  "pl-PL\t,\t\u00a0\t\t\t-\t2\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tAUD=AUD,CAD=CAD,CNY=CNY,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,PLN=z\u0142,TWD=TWD,USD=USD,VND=VND\n" +
  "cs-CZ\t,\t\u00a0\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tAUD=AU$,CZK=K\u010d,ILS=ILS,INR=INR,VND=VND\n" +
  "hu-HU\t,\t\u00a0\t\t\t-\t2\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,EUR=EUR,GBP=GBP,HKD=HKD,HUF=Ft,ILS=ILS,INR=INR,JPY=\u00a5,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,USD=USD,VND=VND,XCD=XCD\n" +
  "ro-RO\t,\t.\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,USD=USD,VND=VND,XCD=XCD\n" +
  "el-GR\t,\t.\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tTHB=\u0e3f,USD=$\n" +
  "tr-TR\t,\t.\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tAUD=AU$,JPY=\u00a5,THB=\u0e3f,TRY=\u20ba,USD=$\n" +
  "ru-RU\t,\t\u00a0\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tJPY=\u00a5,RUB=\u20bd,THB=\u0e3f,TMT=\u0422\u041c\u0422,UAH=\u20b4,USD=$\n" +
  "uk-UA\t,\t\u00a0\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=\u00a5,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,UAH=\u20b4,USD=USD,VND=VND,XCD=XCD\n" +
  "ja-JP\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tCNY=\u5143,JPY=\uffe5,USD=$\n" +
  "zh-CN\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tAUD=AU$,CNY=\u00a5\n" +
  "zh-TW\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tAUD=AU$,JPY=\u00a5,KRW=\uffe6,TWD=$\n" +
  "zh-HK\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tAUD=AU$,JPY=\u00a5\n" +
  "ko-KR\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tAUD=AU$\n" +
  "hi-IN\t.\t,\t\t\t-\t5\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tTHB=\u0e3f,USD=$\n" +
  "id-ID\t,\t.\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tAUD=AU$,IDR=Rp,INR=Rs,THB=\u0e3f\n" +
  "th-TH\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tAUD=AU$,JPY=\u00a5,THB=\u0e3f\n" +
  "vi-VN\t,\t.\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tAUD=AU$,JPY=\u00a5,THB=\u0e3f\n" +
  "ms-MY\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tCAD=CAD,MXN=MXN,MYR=RM,USD=USD\n" +
  "fil-PH\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tJPY=\u00a5,PHP=\u20b1,THB=\u0e3f,USD=$\n" +
  "es-419\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,USD=USD,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF\n" +
  "es-CO\t,\t.\t\t\t-\t0\t\u00a4\u00a0#\t-\u00a4\u00a0#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,COP=$,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF\n" +
  "es-CL\t,\t.\t\t\t-\t0\t\u00a4#\t\u00a4-#\t\u00a4\u00a0#\t\u00a4-#\tAUD=AUD,BRL=BRL,CAD=CAD,CLP=$,CNY=CNY,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF\n" +
  "es-PE\t.\t,\t\t\t-\t0\t\u00a4\u00a0#\t-\u00a4\u00a0#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,PEN=S/,TWD=TWD,USD=USD,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF\n" +
  "es-US\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=\u00a5,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,USD=$,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF\n" +
  "es-UY\t,\t.\t\t\t-\t0\t\u00a4\u00a0#\t-\u00a4\u00a0#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,UYU=$,UYW=UP,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF\n" +
  "es-VE\t,\t.\t\t\t-\t0\t\u00a4#\t\u00a4-#\t\u00a4\u00a0#\t\u00a4-#\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,USD=USD,VES=Bs.S,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF,VEF=Bs.\n" +
  "es-EC\t,\t.\t\t\t-\t0\t\u00a4#\t\u00a4-#\t\u00a4\u00a0#\t\u00a4-#\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,USD=$,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF\n" +
  "es-GT\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,EUR=EUR,GBP=GBP,GTQ=Q,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,USD=USD,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF\n" +
  "es-CR\t,\t\u00a0\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,CRC=\u20a1,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,USD=USD,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF\n" +
  "es-DO\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,DOP=RD$,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF\n" +
  "es-BO\t,\t.\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tAUD=AUD,BOB=Bs,BRL=BRL,CAD=CAD,CNY=CNY,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,USD=USD,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF\n" +
  "es-PY\t,\t.\t\t\t-\t0\t\u00a4\u00a0#\t\u00a4\u00a0-#\t\u00a4\u00a0#\t\u00a4\u00a0-#\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,PYG=Gs.,TWD=TWD,USD=USD,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF\n" +
  "es-PA\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,PAB=B/.,TWD=TWD,USD=USD,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF\n" +
  "es-PR\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,EUR=EUR,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,USD=$,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF\n" +
  "pt-AO\t,\t\u00a0\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tAOA=Kz,AUD=AU$,THB=\u0e3f\n" +
  "pt-MZ\t,\t\u00a0\t\t\t-\t2\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tAUD=AU$,MZN=MTn,THB=\u0e3f\n" +
  "en-NG\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tNGN=\u20a6,PHP=\u20b1\n" +
  "en-KE\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tKES=Ksh,PHP=\u20b1\n" +
  "en-PK\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tPHP=\u20b1,PKR=Rs\n" +
  "en-PH\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tJPY=\u00a5,PHP=\u20b1,USD=$\n" +
  "en-MY\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tMYR=RM,PHP=\u20b1\n" +
  "en-AE\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tJPY=\u00a5,PHP=\u20b1,USD=$\n" +
  "en-HK\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tPHP=\u20b1\n" +
  "en-GH\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tGHS=GH\u20b5,PHP=\u20b1\n" +
  "en-IL\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tPHP=\u20b1\n" +
  "en-001\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tPHP=\u20b1\n" +
  "fr-MA\t,\t.\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tARS=$AR,AUD=$AU,BMD=$BM,BND=$BN,BZD=$BZ,CAD=$CA,CLP=$CL,CNY=CNY,COP=$CO,FJD=$FJ,FKP=\u00a3FK,GBP=\u00a3GB,GIP=\u00a3GI,HKD=HKD,JPY=JPY,LBP=\u00a3LB,MXN=$MX,NAD=$NA,NZD=$NZ,SBD=$SB,SGD=$SG,SRD=$SR,TTD=$TT,TWD=TWD,USD=$US,UYU=$UY,WST=$WS,XCD=XCD,XPF=FCFP,CYP=\u00a3CY\n" +
  "fr-SN\t,\t\u202f\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tARS=$AR,AUD=$AU,BMD=$BM,BND=$BN,BZD=$BZ,CAD=$CA,CLP=$CL,CNY=CNY,COP=$CO,FJD=$FJ,FKP=\u00a3FK,GBP=\u00a3GB,GIP=\u00a3GI,HKD=HKD,JPY=JPY,LBP=\u00a3LB,MXN=$MX,NAD=$NA,NZD=$NZ,SBD=$SB,SGD=$SG,SRD=$SR,TTD=$TT,TWD=TWD,USD=$US,UYU=$UY,WST=$WS,XCD=XCD,XPF=FCFP,CYP=\u00a3CY\n" +
  "fr-CI\t,\t\u202f\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tARS=$AR,AUD=$AU,BMD=$BM,BND=$BN,BZD=$BZ,CAD=$CA,CLP=$CL,CNY=CNY,COP=$CO,FJD=$FJ,FKP=\u00a3FK,GBP=\u00a3GB,GIP=\u00a3GI,HKD=HKD,JPY=JPY,LBP=\u00a3LB,MXN=$MX,NAD=$NA,NZD=$NZ,SBD=$SB,SGD=$SG,SRD=$SR,TTD=$TT,TWD=TWD,USD=$US,UYU=$UY,WST=$WS,XCD=XCD,XPF=FCFP,CYP=\u00a3CY\n" +
  "fr-LU\t,\t.\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tARS=$AR,AUD=$AU,BMD=$BM,BND=$BN,BZD=$BZ,CAD=$CA,CLP=$CL,CNY=CNY,COP=$CO,FJD=$FJ,FKP=\u00a3FK,GBP=\u00a3GB,GIP=\u00a3GI,HKD=HKD,JPY=JPY,LBP=\u00a3LB,MXN=$MX,NAD=$NA,NZD=$NZ,SBD=$SB,SGD=$SG,SRD=$SR,TTD=$TT,TWD=TWD,USD=$US,UYU=$UY,WST=$WS,XCD=XCD,XPF=FCFP,CYP=\u00a3CY\n" +
  "de-LU\t,\t.\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tAUD=AU$,JPY=\u00a5,THB=\u0e3f,USD=$\n" +
  "de-LI\t.\t\u2019\t\t\t-\t0\t\u00a4\u00a0#\t-\u00a4\u00a0#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tAUD=AU$,EUR=EUR,JPY=\u00a5,THB=\u0e3f,USD=$\n" +
  "hr-HR\t,\t.\t\t\t\u2212\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,USD=USD,VND=VND,XCD=XCD,XPF=XPF,HRK=kn\n" +
  "sl-SI\t,\t.\t\t\t\u2212\t2\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tAUD=AUD,BRL=BRL,CAD=CAD,GBP=GBP,JPY=\u00a5,MXN=MXN,NZD=NZD,TWD=TWD,USD=$,XCD=XCD\n" +
  "sk-SK\t,\t\u00a0\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,GBP=GBP,HKD=HKD,ILS=NIS,INR=INR,JPY=JPY,KRW=KRW,NZD=NZD,TWD=TWD,USD=USD,VND=VND\n" +
  "bg-BG\t,\t\u00a0\t\t\t-\t2\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,USD=\u0449.\u0434.,VND=VND,XCD=XCD,BGN=\u043b\u0432.\n" +
  "lt-LT\t,\t\u00a0\t\t\t\u2212\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,USD=USD,VND=VND,XAF=XAF,XCD=XCD,XOF=XOF,XPF=XPF\n" +
  "lv-LV\t,\t\u00a0\t\t\t-\t2\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tAUD=AU$,JPY=\u00a5,THB=\u0e3f,USD=$,LVL=Ls\n" +
  "et-EE\t,\t\u00a0\t\t\t\u2212\t2\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tAUD=AU$,JPY=\u00a5,THB=\u0e3f,USD=$,EEK=kr\n" +
  "is-IS\t,\t.\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tAUD=AUD,BRL=BRL,CAD=CAD,EUR=EUR,GBP=GBP,INR=INR,ISK=kr.,KRW=KRW,MXN=MXN,NZD=NZD,TWD=TWD,USD=USD,VND=VND\n" +
  "mk-MK\t,\t.\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tAUD=AUD,CNY=CNY,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MKD=\u0434\u0435\u043d.,NZD=NZD,TWD=TWD,VND=VND\n" +
  "sr-Latn-RS\t,\t.\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tAUD=AUD,BAM=KM,JPY=\u00a5,KRW=KRW,NZD=NZD,VND=VND\n" +
  "bs-Latn-BA\t,\t.\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tAUD=AUD,BAM=KM,BRL=BRL,CAD=CAD,CNY=CNY,GBP=GBP,HKD=HKD,ILS=ILS,JPY=\u00a5,MXN=MXN,NZD=NZD,RSD=din.,THB=\u0e3f,USD=USD,XCD=XCD,XPF=XPF,HRK=kn\n" +
  "sq-AL\t,\t\u00a0\t\t\t-\t2\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tALL=Lek\u00eb,BRL=BRL,THB=\u0e3f\n" +
  "ka-GE\t,\t\u00a0\t\t\t-\t2\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tAUD=AUD,CNY=CNY,GEL=\u20be,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,NZD=NZD,VND=VND\n" +
  "hy-AM\t,\t\u00a0\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tAMD=\u058f,THB=\u0e3f,USD=$\n" +
  "az-Latn-AZ\t,\t.\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tAZN=\u20bc,THB=\u0e3f\n" +
  "kk-KZ\t,\t\u00a0\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tKZT=\u20b8,LSL=\u041b\u0421\u041b,RUB=\u20bd,THB=\u0e3f,USD=$\n" +
  "uz-Latn-UZ\t,\t\u00a0\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tUZS=so\u02bbm\n" +
  "sw-TZ\t.\t,\t\t\t-\t0\t\u00a4\u00a0#\t-\u00a4\u00a0#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tKES=Ksh,THB=\u0e3f,TZS=TSh\n" +
  "sw-KE\t.\t,\t\t\t-\t0\t\u00a4\u00a0#\t-\u00a4\u00a0#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tKES=Ksh,THB=\u0e3f,TZS=TSh,USD=$\n" +
  "zh-SG\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tAUD=AU$,SGD=$\n" +
  "zh-MO\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tAUD=AU$,JPY=\u00a5,MOP=MOP$\n" +
  "ta-IN\t.\t,\t\t\t-\t5\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tJPY=\u00a5,THB=\u0e3f,USD=$\n" +
  "te-IN\t.\t,\t\t\t-\t5\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tTHB=\u0e3f,USD=$\n" +
  "gu-IN\t.\t,\t\t\t-\t5\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tTHB=\u0e3f\n" +
  "kn-IN\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tTHB=\u0e3f,USD=$\n" +
  "ml-IN\t.\t,\t\t\t-\t4\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tJPY=\u00a5,THB=\u0e3f,USD=$\n" +
  "pa-IN\t.\t,\t\t\t-\t5\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tTHB=\u0e3f\n" +
  "km-KH\t.\t,\t\t\t-\t0\t#\u00a4\t-#\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tKHR=\u17db,LSL=\u17a1\u17bc\u1791\u17b8,THB=\u0e3f,USD=$\n" +
  "lo-LA\t,\t.\t\t\t-\t0\t\u00a4#\t\u00a4-#\t\u00a4\u00a0#\t\u00a4-#\tLAK=\u20ad,THB=\u0e3f\n" +
  "si-LK\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tLKR=\u0dbb\u0dd4.,THB=\u0e3f,XOF=\u0dc3\u0dd2\u0dc6\u0dca\u0d91\n" +
  "am-ET\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tAUD=AU$,ETB=\u1265\u122d,THB=\u0e3f\n" +
  "zu-ZA\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tTHB=\u0e3f,USD=$,ZAR=R\n" +
  "af-ZA\t,\t\u00a0\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tCAD=CAD,MXN=MXN,THB=\u0e3f,USD=USD,ZAR=R\n" +
  "ca-ES\t,\t.\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tAUD=AU$,BRL=BRL,CAD=CAD,CNY=CNY,JPY=\u00a5,MXN=MXN,THB=\u0e3f,USD=USD,XCD=XCD\n" +
  "eu-ES\t,\t.\t\t\t\u2212\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tTHB=\u0e3f\n" +
  "gl-ES\t,\t.\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tMXN=$MX,THB=\u0e3f,USD=$,XCD=XCD\n" +
  "ga-IE\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tJPY=\u00a5,PHP=\u20b1,THB=\u0e3f,USD=$\n" +
  "cy-GB\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tKRW=KRW,THB=\u0e3f\n" +
  "mt-MT\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\t\n" +
  "lb-LU\t,\t.\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tAUD=AU$,JPY=\u00a5,THB=\u0e3f,USD=$\n" +
  "nn-NO\t,\t\u00a0\t\t\t\u2212\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tAUD=AUD,BRL=BRL,CAD=CAD,CNY=CNY,GBP=GBP,HKD=HKD,ILS=ILS,INR=INR,JPY=JPY,KRW=KRW,MXN=MXN,NOK=kr,NZD=NZD,TWD=TWD,USD=USD,VND=VND,XAF=XAF,XCD=XCD,XPF=XPF";
let moneyT = null;
function moneyTables() {
  if (moneyT) return moneyT;
  const pairs = (s) => { const o = Object.create(null); for (const p of s.split(",")) { const i = p.indexOf("="); if (i > 0) o[p.slice(0, i)] = p.slice(i + 1); } return o; };
  const units = Object.create(null);
  for (const c of (MONEY_CURRENCIES + " " + MONEY_WITHDRAWN).split(" ")) units[c.slice(0, 3)] = +c.slice(3);
  const locales = MONEY_LOCALES.split("\n").map((l) => {
    const f = l.split("\t");
    return { name: f[0], decimal: f[1], group: f[2], number: [f[3] || f[1], f[4] || f[2]], minus: f[5], indian: (+f[6] & 1) !== 0, numberIndian: (+f[6] & 4) !== 0, min2: (+f[6] & 2) !== 0, pats: f.slice(7, 11), symbols: pairs(f[11] || "") };
  });
  return (moneyT = { units, locales, symbols: pairs(MONEY_SYMBOLS) });
}
const unitsOf = (code) => { const u = moneyTables().units[code]; return u === undefined ? null : u; };
const displayDigits = (code, units) => {
  const t = moneyTables();
  if (!t.display) { t.display = Object.create(null); for (const c of MONEY_DISPLAY.split(" ")) t.display[c.slice(0, 3)] = +c.slice(3); }
  return t.display[code] ?? units;
};
// A locale's data as src/money.rs's locale_found finds it: by name; Chinese by script; by
// language and region without the script; Latin America's Spanish as es-419 and Portugal's
// regions as pt-PT, as CLDR's parents; else the first of the language; else en-US; extensions
// not read. [data, matched] (FEEDBACK50.md, Q-627; branch feedback50-fixes).
const ES_419 = ["419", "AR", "BO", "BZ", "CL", "CO", "CR", "CU", "DO", "EC", "GT", "HN", "MX", "NI", "PA", "PE", "PR", "PY", "SV", "US", "UY", "VE"];
const PT_PT = ["AO", "CH", "CV", "GQ", "GW", "LU", "MO", "MZ", "PT", "ST", "TL"];
const asciiLower = (s) => s.replace(/[A-Z]/g, (x) => x.toLowerCase());
function localeFound(name) {
  const all = moneyTables().locales;
  name = String(name).replace(/_/g, "-");
  const find = (n) => all.find((l) => asciiLower(l.name) === asciiLower(n));
  const exact = find(name);
  if (exact) return [exact, true];
  const subs = name.split("-"), lang = asciiLower(subs[0]);
  const rest = [];
  for (const x of subs.slice(1)) { if (/^[A-Za-z0-9]$/.test(x)) break; rest.push(x); }
  const script = rest.length > 0 && /^[A-Za-z]{4}$/.test(rest[0]) ? asciiLower(rest[0]) : null;
  const r = rest[script === null ? 0 : 1];
  const region = r !== undefined && /^([A-Za-z]{2}|[0-9]{3})$/.test(r) ? r.replace(/[a-z]/g, (x) => x.toUpperCase()) : null;
  let picked = null;
  if (lang === "zh" && script === "hant") picked = region === "HK" || region === "MO" ? `zh-${region}` : "zh-TW";
  else if (lang === "zh" && script === "hans") picked = region === "SG" ? "zh-SG" : "zh-CN";
  else if (region !== null && find(`${lang}-${region}`)) picked = `${lang}-${region}`;
  else if (lang === "es" && ES_419.includes(region)) picked = "es-419";
  else if (lang === "pt" && PT_PT.includes(region)) picked = "pt-PT";
  if (picked !== null && find(picked)) return [find(picked), true];
  const first = all.find((l) => asciiLower(l.name.split("-")[0]) === lang);
  if (!first) return [all[0], false];
  // A language alone (or with its own script) names its first locale.
  const rowScript = first.name.split("-")[1];
  const sameScript = script === null || (rowScript && rowScript.length === 4 ? asciiLower(rowScript) === script : script === "latn");
  return [first, region === null && sameScript];
}
const localeSaid = new Set();
function localeData(name) {
  const [loc, found] = localeFound(name);
  if (!found && localeSaid.size < 64 && !localeSaid.has(String(name))) {
    localeSaid.add(String(name));
    console.warn(`quillo: warning: no money and number data for locale ${JSON.stringify(String(name))}: written as ${loc.name}`);
  }
  return loc;
}
// A currency sign (Unicode's Sc), as src/money.rs's is_currency_sign.
const CURRENCY_SIGN = /^[$\u00a2-\u00a5\u058f\u060b\u07fe\u07ff\u09f2\u09f3\u09fb\u0af1\u0bf9\u0e3f\u17db\u20a0-\u20c0\ua838\ufdfc\ufe69\uff04\uffe0\uffe1\uffe5\uffe6\u{11fdd}-\u{11fe0}\u{1e2ff}\u{1ecb0}]$/u;
function moneyDigits(loc, [decimal, group], d, indian = loc.indian) {
  const t = decText(new Dec(babs(d.m), d.s));
  const dot = t.indexOf(".");
  const whole = dot < 0 ? t : t.slice(0, dot), frac = dot < 0 ? "" : t.slice(dot + 1);
  const n = whole.length, grouped = !(loc.min2 && n < 5);
  let out = "";
  for (let i = 0; i < n; i++) {
    const left = n - i;
    if (i > 0 && grouped && (indian ? left === 3 || (left > 3 && (left - 3) % 2 === 0) : left % 3 === 0)) out += group;
    out += whole[i];
  }
  return frac ? out + decimal + frac : out;
}
function formatAmount(d, code, places, locName) {
  const loc = localeData(locName);
  d = decRound(d, places, "half_even");
  const sym = loc.symbols[code] ?? moneyTables().symbols[code] ?? code;
  const before = loc.pats[0].indexOf("¤") < loc.pats[0].indexOf("#");
  const side = before ? Array.from(sym).pop() : Array.from(sym)[0];
  const letters = side !== undefined && !CURRENCY_SIGN.test(side);
  const pat = loc.pats[(letters ? 2 : 0) + (d.m < 0n ? 1 : 0)];
  const num = moneyDigits(loc, [loc.decimal, loc.group], d);
  return pat.replace("-", () => loc.minus).replace("¤", () => sym).replace("#", () => num);
}
const moneyShort = (v) => (v === undefined ? "nothing" : `${typeName(v)} ${short(v)}`);
// Trimmed as Rust's trim() trims (its White_Space, not JavaScript's \s), and a code
// upper-cased in ASCII only, as on the server (found in review of FEEDBACK50's fixes).
const rsTrim = (s) => s.replace(/^[\t-\r \u0085\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000]+|[\t-\r \u0085\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000]+$/g, "");
const moneyCode = (c) => rsTrim(c).replace(/[a-z]/g, (x) => x.toUpperCase());
function moneyCurrency(c, f, i) {
  if (typeof c !== "string") fail(`\`${f}\` expects a currency code (a str) as argument ${i}, got ${moneyShort(c)}`);
  const code = moneyCode(c), u = unitsOf(code);
  return u === null ? new Err(`unknown currency ${repr(c)}`) : { code, units: u };
}
// Money held to what money.of holds an amount to, as on the server: the code trimmed, no
// exponent in text, no more decimals than the currency has, minor units that fit an int;
// `exact` false for money.round (FEEDBACK50.md, Q-622 and Q-623; branch feedback50-fixes).
function moneyArg(v, f, i, exact = true) {
  const bad = (why) => fail(`\`${f}\` expects money ({ amount, currency }) as argument ${i}, got ${moneyShort(v)}${why}`);
  if (!isMap(v)) bad("");
  const c = hasOwn(v, "currency") ? v.currency : null;
  if (typeof c !== "string") bad(": no currency");
  const code = moneyCode(c), units = unitsOf(code);
  if (units === null) bad(": unknown currency");
  const a = hasOwn(v, "amount") ? v.amount : null;
  let amount;
  if (a instanceof Dec) amount = a;
  else if (isInt(a) || typeof a === "bigint") amount = new Dec(BigInt(a), 0);
  else if (typeof a === "string") { amount = /[eE]/.test(a) ? null : decParse(rsTrim(a), false); if (amount === null) bad(`: its amount ${repr(a)} is not an amount`); }
  else if (typeof a === "number" || a instanceof Fl) bad(": its amount is a float: make it with money.of(dec(x), currency)");
  else bad(": its amount must be a dec");
  if (!exact) return { amount, code, units };
  const fitted = decRound(amount, units, "down");
  if (decCmp(fitted, amount) !== 0) fail(`\`${f}\`: ${decText(amount)} has more decimals than ${code} has (${units}): round it first with money.round(m, mode)`);
  if (fitted.s !== units || fitted.m < I64_MIN || fitted.m > I64_MAX) fail(`\`${f}\`: ${decText(amount)} ${code} is too large an amount`);
  return { amount: fitted, code, units };
}
const moneyIs = (v) => { try { moneyArg(v, "", 0); return true; } catch (e) { if (e instanceof QuilloError) return false; throw e; } };
// What the money type makes of data, as src/money.rs's from_data: a map of an amount (a
// decimal, an int, numeric text, or a JSON number) and a currency, held to money.of's rules
// with no rounding mode; else why, without the value sent (FEEDBACK50.md, Q-621; branch
// feedback50-fixes).
function moneyFromData(v) {
  if (!isMap(v)) return { why: "must be money, { amount, currency }" };
  if (!hasOwn(v, "currency") || v.currency === undefined) return { why: "has no currency" };
  if (typeof v.currency !== "string") return { why: "has a currency that is not text" };
  const code = moneyCode(v.currency), units = unitsOf(code);
  if (units === null) return { why: "has a currency that is not an ISO 4217 code" };
  if (!hasOwn(v, "amount") || v.amount === undefined) return { why: "has no amount" };
  const a = v.amount;
  const amount = a instanceof Dec ? a : isInt(a) || typeof a === "bigint" ? new Dec(BigInt(a), 0) : typeof a === "number" || a instanceof Fl ? decFromFloat(numOf(a))
    : typeof a === "string" && !/[eE]/.test(a) ? decParse(rsTrim(a), false) : null;
  if (amount === null) return { why: "has an amount that is not a number" };
  const fitted = decRound(amount, units, "down");
  if (decCmp(fitted, amount) !== 0) return { why: `has more decimals than ${code} has (${units})` };
  if (fitted.s !== units || fitted.m < I64_MIN || fitted.m > I64_MAX) return { why: `is too large an amount of ${code}` };
  return { v: moneyOf(fitted, code) };
}
// Money past what money.of takes: an error value, as on the server (FEEDBACK49.md, Q-605;
// the browser's half, branch feedback50-fixes).
const moneyIn = (d, code) => (d.m < I64_MIN || d.m > I64_MAX ? new Err(`${decText(d)} ${code} is too large an amount`) : moneyOf(d, code));
// a + b and a - b when both have money's shape: the amounts, and two currencies an error
// value (FEEDBACK50.md, Q-620; branch feedback50-fixes). null for other maps.
const moneyShaped = (v) => isMap(v) && hasOwn(v, "amount") && hasOwn(v, "currency") && Object.keys(v).length === 2;
function moneyOp(x, y, sub) {
  if (!moneyShaped(x) || !moneyShaped(y)) return null;
  const sym = sub ? "-" : "+", a = moneyArg(x, sym, 1), b = moneyArg(y, sym, 2);
  if (a.code !== b.code) return new Err(`cannot ${sub ? "subtract" : "add"} ${a.code} and ${b.code}: convert one currency to the other first`);
  return moneyIn(decAdd(a.amount, sub ? new Dec(-b.amount.m, b.amount.s) : b.amount), a.code);
}
function moneyMode(m, f, i = f === "money.round" ? 2 : 3) {
  if (isNil(m)) return "half_even";
  if (typeof m !== "string") fail(`\`${f}\` expects a rounding mode (a str) as argument ${i}, got ${moneyShort(m)}`);
  if (!["half_even", "half_up", "half_down", "up", "down", "ceiling", "floor"].includes(m)) fail(`\`${f}\`: unknown rounding ${JSON.stringify(m)} (one of ${ROUNDINGS})`);
  return m;
}
const moneyOf = (amount, code) => ({ amount, currency: code });
// The rest of money.of and money.parse, as src/money.rs's of_amount.
function moneyOfAmount(d, c, mode, f, i) {
  let r;
  if (isNil(mode)) {
    r = decRound(d, c.units, "down");
    if (decCmp(r, d) !== 0) return new Err(`${decText(d)} has more decimals than ${c.code} has (${c.units}); give a rounding mode to round it`);
  } else r = decRound(d, c.units, moneyMode(mode, f, i));
  if (r.s !== c.units || r.m < I64_MIN || r.m > I64_MAX) return new Err(`${decText(r)} ${c.code} is too large an amount`);
  return moneyOf(r, c.code);
}
// Text as `loc` writes an amount of `code`, as src/money.rs's read_amount.
const SPACEY = /^[ \u00a0\u202f]$/, MARKS = /^[ \u00a0\u202f\u200e\u200f\u061c]+|[ \u00a0\u202f\u200e\u200f\u061c]+$/g;
function moneyRead(text, code, loc) {
  let t = rsTrim(text).replace(MARKS, "");
  if (t === "" || Array.from(t).length > 400) return null;
  const sym = (l) => l.symbols[code] ?? moneyTables().symbols[code] ?? code;
  const names = [...new Set([...moneyTables().locales.map(sym), code])]
    .sort((a, b) => Array.from(b).length - Array.from(a).length || (a < b ? -1 : a > b ? 1 : 0));
  const sign = () => {
    const m = [loc.minus, "-", "\u2212"].find((x) => t.startsWith(x));
    if (m === undefined) return false;
    t = t.slice(m.length).replace(MARKS, "");
    return true;
  };
  let neg = sign();
  const n = names.find((x) => t.startsWith(x) || t.endsWith(x));
  if (n !== undefined) t = (t.startsWith(n) ? t.slice(n.length) : t.slice(0, t.length - n.length)).replace(MARKS, "");
  if (!neg) neg = sign();
  const spaced = Array.from(loc.group).every((ch) => SPACEY.test(ch));
  const isGroup = (ch) => (spaced ? SPACEY.test(ch) : loc.group.includes(ch) || (loc.group === "\u2019" && ch === "'"));
  const at = t.indexOf(loc.decimal);
  const whole = at < 0 ? t : t.slice(0, at), frac = at < 0 ? null : t.slice(at + loc.decimal.length);
  if (frac !== null && !/^[0-9]+$/.test(frac)) return null;
  const groups = [];
  let cur = "";
  for (const ch of whole) { if (isGroup(ch)) { groups.push(cur); cur = ""; } else cur += ch; }
  groups.push(cur);
  if (groups.some((g) => !/^[0-9]+$/.test(g))) return null;
  if (groups.length > 1) {
    const last = groups[groups.length - 1], rest = groups.slice(0, -1);
    const western = groups[0].length <= 3 && groups.slice(1).every((g) => g.length === 3);
    if (!(western || (loc.indian && last.length === 3 && rest[0].length <= 2 && rest.slice(1).every((g) => g.length === 2)))) return null;
  }
  return decParse((neg ? "-" : "") + groups.join("") + (frac === null ? "" : "." + frac), false);
}
function moneySame(a, b, verb) {
  if (a.code !== b.code) fail(`cannot ${verb} ${a.code} and ${b.code}: convert one currency to the other first`);
}
// The locale to write in: the one given, else the page's, else en-US.
function moneyLocale(l, f, i) {
  if (isNil(l)) { const d = i18n(); return d ? d.locale : "en-US"; }
  if (typeof l !== "string") fail(`\`${f}\` expects a locale (a str) as argument ${i}, got ${moneyShort(l)}`);
  return l;
}
B.money = {
  of: (amount, currency, mode) => {
    const c = moneyCurrency(currency, "money.of", 2);
    if (c instanceof Err) return c;
    let d;
    if (amount instanceof Dec) d = amount;
    else if (isInt(amount) || typeof amount === "bigint") d = new Dec(BigInt(amount), 0);
    // Text as people write an amount, and money whose cents fit an int, as
    // on the server (FEEDBACK14.md, Q-096; branch feedback14-fixes).
    else if (typeof amount === "string") { d = /[eE]/.test(amount) ? null : decParse(rsTrim(amount), false); if (d === null) return new Err(`not an amount: ${repr(amount)}`); }
    else if (typeof amount === "number" || amount instanceof Fl) fail("`money.of` takes a dec, an int or text, not a float: convert it with dec(x)");
    else fail(`\`money.of\` expects an amount (a dec, int or str) as argument 1, got ${moneyShort(amount)}`);
    return moneyOfAmount(d, c, mode, "money.of", 3);
  },
  // An amount as the locale writes one, as src/money.rs's parse (FEEDBACK50.md, Q-624; branch
  // feedback50-fixes).
  parse: (text, currency, locale, mode) => {
    if (typeof text !== "string") fail(`\`money.parse\` expects text as argument 1, got ${moneyShort(text)}`);
    const c = moneyCurrency(currency, "money.parse", 2);
    if (c instanceof Err) return c;
    const d = moneyRead(text, c.code, localeData(moneyLocale(locale, "money.parse", 3)));
    return d === null ? new Err(`not an amount: ${repr(text)}`) : moneyOfAmount(d, c, mode, "money.parse", 4);
  },
  from_minor: (n, currency) => {
    const c = moneyCurrency(currency, "money.from_minor", 2);
    if (c instanceof Err) return c;
    if (!isInt(n) && typeof n !== "bigint") fail(`\`money.from_minor\` expects an int as argument 1, got ${moneyShort(n)}`);
    return moneyOf(new Dec(BigInt(n), c.units), c.code);
  },
  to_minor: (m) => bigChecked(moneyArg(m, "money.to_minor", 1).amount.m),
  add: (x, y) => { const a = moneyArg(x, "money.add", 1), b = moneyArg(y, "money.add", 2); moneySame(a, b, "add"); return moneyIn(decAdd(a.amount, b.amount), a.code); },
  sub: (x, y) => { const a = moneyArg(x, "money.sub", 1), b = moneyArg(y, "money.sub", 2); moneySame(a, b, "subtract"); return moneyIn(decAdd(a.amount, new Dec(-b.amount.m, b.amount.s)), a.code); },
  mul: (x, factor, mode) => {
    const a = moneyArg(x, "money.mul", 1);
    let f;
    if (factor instanceof Dec) f = factor;
    else if (isInt(factor) || typeof factor === "bigint") f = new Dec(BigInt(factor), 0);
    else if (typeof factor === "number" || factor instanceof Fl) fail("`money.mul` takes a dec or an int, not a float: convert it with dec(x)");
    else fail(`\`money.mul\` expects a dec or an int as argument 2, got ${moneyShort(factor)}`);
    return moneyIn(decRound(decMul(a.amount, f), a.units, moneyMode(mode, "money.mul")), a.code);
  },
  neg: (x) => { const a = moneyArg(x, "money.neg", 1); return moneyIn(new Dec(-a.amount.m, a.amount.s), a.code); },
  compare: (x, y) => { const a = moneyArg(x, "money.compare", 1), b = moneyArg(y, "money.compare", 2); moneySame(a, b, "compare"); return decCmp(a.amount, b.amount); },
  zero: (currency) => { const c = moneyCurrency(currency, "money.zero", 1); return c instanceof Err ? c : moneyOf(new Dec(0n, c.units), c.code); },
  is_zero: (x) => moneyArg(x, "money.is_zero", 1).amount.m === 0n,
  round: (x, mode) => { const a = moneyArg(x, "money.round", 1, false); return moneyIn(decRound(a.amount, a.units, moneyMode(mode, "money.round")), a.code); },
  minor_units: (currency) => { const c = moneyCurrency(currency, "money.minor_units", 1); return c instanceof Err ? c : c.units; },
  // Split by ratios without losing or making a minor unit: each share its
  // part rounded down, the units left one each to the largest remainders, a
  // tie to the first (FEEDBACK50.md, Q-631; branch feedback50-fixes).
  allocate: (x, ratios) => {
    const F = "money.allocate", a = moneyArg(x, F, 1);
    let rs;
    if (isInt(ratios) && ratios >= 1 && ratios <= 100000) rs = new Array(ratios).fill(new Dec(1n, 0));
    else if (Array.isArray(ratios) && ratios.length > 0) {
      rs = ratios.map((v, k) => {
        if ((isInt(v) || typeof v === "bigint") && v >= 0) return new Dec(BigInt(v), 0);
        if (v instanceof Dec && v.m >= 0n) return v;
        fail(`\`${F}\`: ratio ${k} must be an int or a dec, 0 or more, got ${moneyShort(v)}`);
      });
    } else fail(`\`${F}\` expects a list of ratios or a number of parts as argument 2, got ${moneyShort(ratios)}`);
    const s = Math.max(...rs.map((r) => r.s));
    const whole = rs.map((r) => r.m * p10(s - r.s));
    const total = whole.reduce((t, r) => t + r, 0n);
    if (total <= 0n) fail(`\`${F}\`: the ratios must add up to more than 0`);
    const minor = a.amount.m, neg = minor < 0n, units = babs(minor);
    const shares = whole.map((r) => (units * r) / total), rests = whole.map((r) => (units * r) % total);
    const left = Number(units - shares.reduce((t, x) => t + x, 0n));
    const order = shares.map((_, k) => k).sort((p, q) => (rests[q] > rests[p] ? 1 : rests[q] < rests[p] ? -1 : p - q));
    for (const k of order.slice(0, left)) shares[k] += 1n;
    return shares.map((x) => moneyOf(new Dec(neg ? -x : x, a.units), a.code));
  },
  format: (x, locale) => { const a = moneyArg(x, "money.format", 1); const shown = displayDigits(a.code, a.units); return formatAmount(decRound(a.amount, shown, "half_up"), a.code, shown, moneyLocale(locale, "money.format", 2)); },
};
// format_number(x, locale, digits): a number as the locale writes one.
B.format_number = meta((x, locale, digits, mode) => {
  let d;
  if (x instanceof Dec) d = x;
  // A whole number past 2^53 from the page's code is a float, by its shortest decimal: BigInt
  // wrote its binary digits, 6.022e23 as 602,200,000,000,000,027,262,976 (FEEDBACK55.md, Q-827;
  // branch feedback55).
  else if (Number.isSafeInteger(x) || typeof x === "bigint") d = new Dec(BigInt(x), 0);
  else if (typeof x === "number" || x instanceof Fl) { d = decFromFloat(numOf(x)); if (d === null) return display(x); }
  else fail(`\`format_number\` expects a number as argument 1, got ${moneyShort(x)}`);
  const loc = localeData(moneyLocale(locale, "format_number", 2));
  const rounding = isNil(mode) ? "half_up" : moneyMode(mode, "format_number", 4);
  // Below 0, and -0.0, keep the sign rounded to 0, as on the server and in Intl: -0.001 to two
  // digits is "-0.00" (FEEDBACK55.md, not counted; branch feedback55).
  const neg = d.m < 0n || ((typeof x === "number" || x instanceof Fl) && (numOf(x) < 0 || Object.is(numOf(x), -0)));
  // Ties away from zero, as Intl writes them; `{ min, max }` digits too (FEEDBACK30.md, Q-369;
  // branch feedback27-35-fixes).
  if (!isNil(digits)) {
    const clamp = (n) => Math.min(Math.max(n, 0), 28);
    let min, max;
    if (isMap(digits)) {
      // The first one set, as on the server (FEEDBACK55.md, Q-875; branch feedback55).
      const k = mkeys(digits).find((k) => k !== "min" && k !== "max");
      if (k !== undefined) fail(`\`format_number\`'s digits are \`{ min, max }\`, not \`${k}\``);
      const get = (k) => { const v = digits[k]; if (isNil(v)) return null; if (!isInt(v)) fail(`\`format_number\`'s ${k} is an int, got ${moneyShort(v)}`); return clamp(v); };
      min = get("min") ?? 0; max = get("max") ?? Math.max(min, 3);
      if (min > max) fail(`\`format_number\`: min ${min} is more than max ${max}`);
    } else if (isInt(digits)) min = max = clamp(digits);
    else fail(`\`format_number\` expects an int or \`{ min, max }\` as argument 3, got ${moneyShort(digits)}`);
    // A tie away from zero unless `mode` says (FEEDBACK50.md, Q-626; branch feedback50-fixes).
    d = decRound(d, max, rounding);
    while (d.s > min && d.m % 10n === 0n) d = new Dec(d.m / 10n, d.s - 1);
  }
  return (neg ? loc.minus : "") + moneyDigits(loc, loc.number, d, loc.numberIndian);
}, "format_number", null);

// ---- math, stats, vec (src/numeric.rs, the same answers) ------------------
// The browser cannot tell 2 from 2.0, so ints are numbers that are whole.

const decNote = (v) => (v instanceof Dec ? FLOAT_ONLY : "");
// A list of numbers as floats, as the server's `nums` reads them (an int past 2^53 and a whole
// float too), and whether all were ints (FEEDBACK55.md, Q-775 and Q-776; branch feedback55).
const NSi = (l, f, i = 1) => {
  l = L(l, f, i);
  const out = new Array(l.length);
  let ints = true;
  for (let k = 0; k < l.length; k++) {
    const v = l[k];
    if (typeof v === "number") { out[k] = v; if (ints && !Number.isInteger(v)) ints = false; continue; }
    const x = numOf(v);
    if (x === null) fail(`\`${f}\` expects a list of numbers, but item ${k} is ${typeName(v)}${decNote(v)}`);
    out[k] = x;
    if (v instanceof Fl) ints = false;
  }
  return [out, ints];
};
const NS = (l, f, i = 1) => NSi(l, f, i)[0];
// An int when the inputs were ints and it is whole (below 9e15), else a float: the server's `back`.
const back = (x, ints) => (ints && Number.isInteger(x) && Math.abs(x) < 9e15 ? x + 0 : fl(x));
const fls = (xs) => xs.map(fl);
// NaN and infinity have no place where data are put in order or counted, as on the server
// (FEEDBACK49.md, Q-605; FEEDBACK55.md, Q-780; branch feedback55).
const finiteOf = (x, f, what) => { const i = x.findIndex((v) => !Number.isFinite(v)); if (i >= 0) fail(`\`${f}\`: the ${what} hold ${Number.isNaN(x[i]) ? "NaN" : x[i] > 0 ? "inf" : "-inf"} at index ${i}: NaN and infinity cannot be counted or put in order`); return x; };
// An int argument of the numeric functions, as the server's `int` takes it: a whole float below
// 9e15 too, and an int past 2^53 (a BigInt) (FEEDBACK55.md, Q-776; branch feedback55).
const NI = (v, f, i = 1) => { if (v instanceof Fl && Math.abs(v.f) < 9e15) return v.f + 0; want(isInt(v) || typeof v === "bigint", f, "an int", i, v); return v; };
const given = (v) => !isNil(v);
const csum2 = (xs) => {
  let s = 0, c = 0;
  for (const x of xs) { const t = s + x; c += Math.abs(s) >= Math.abs(x) ? (s - t) + x : (x - t) + s; s = t; }
  if (!Number.isFinite(s)) return [s, 0];
  const t = s + c;
  return [t, c - (t - s)];
};
const csum = (xs) => { const [s, c] = csum2(xs); return s + c; };
// Products' errors kept without fma: Dekker's split.
const twoProd = (a, b) => {
  const p = a * b, sp = (x) => { const c = 134217729 * x, h = c - (c - x); return [h, x - h]; };
  const [ah, al] = sp(a), [bh, bl] = sp(b);
  return [p, ((ah * bh - p) + ah * bl + al * bh) + al * bl];
};
const cdot2 = (a, b) => {
  let s = 0, c = 0;
  for (let i = 0; i < a.length; i++) {
    const [p, e] = twoProd(a[i], b[i]);
    const t = s + p, z = t - s;
    c += (s - (t - z)) + (p - z) + (Number.isFinite(e) ? e : 0);
    s = t;
  }
  if (!Number.isFinite(s)) return [s, 0];
  const t = s + c;
  return [t, c - (t - s)];
};
const cdot = (a, b) => { const [s, c] = cdot2(a, b); return s + c; };
const div2 = ([hi, lo], n) => {
  const q = hi / n;
  if (!Number.isFinite(q)) return q;
  const [p, e] = twoProd(q, n);
  return q + (((hi - p) - e) + lo) / n;
};
const fsum = (xs) => {
  const partials = [];
  let special = 0, seen = false;
  for (let x of xs) {
    if (!Number.isFinite(x)) { special += x; seen = true; continue; }
    let i = 0;
    for (let j = 0; j < partials.length; j++) {
      let y = partials[j];
      if (Math.abs(x) < Math.abs(y)) [x, y] = [y, x];
      const hi = x + y, lo = y - (hi - x);
      if (lo !== 0) partials[i++] = lo;
      x = hi;
    }
    partials.length = i;
    // As on the server (FEEDBACK16.md, Q-113; branch feedback20-fixes).
    if (!Number.isFinite(x)) { special += x; seen = true; partials.length = 0; continue; }
    partials.push(x);
  }
  if (seen) return special;
  let n = partials.length;
  if (!n) return 0;
  let hi = partials[--n], lo = 0;
  while (n > 0) { const x = hi, y = partials[--n]; hi = x + y; lo = y - (hi - x); if (lo !== 0) break; }
  if (n > 0 && ((lo < 0 && partials[n - 1] < 0) || (lo > 0 && partials[n - 1] > 0))) { const y = lo * 2, x = hi + y; if (y === x - hi) hi = x; }
  return hi;
};
// The sum may pass the floats where the mean does not (FEEDBACK16.md, Q-114; branch feedback20-fixes).
const meanOf = (x) => { const m = div2(csum2(x), x.length); return Number.isFinite(m) || !x.every(Number.isFinite) ? m : csum(x.map((v) => v / x.length)); };
const ssOf = (x, m) => {
  const d = x.map((v) => v - m), c = csum(d), [hi, lo] = cdot2(d, d), fix = c * c / x.length, t = hi - fix;
  return t <= 0 ? [0, 0] : [t, lo - ((t - hi) + fix)];
};
const sortedOf = (x) => Float64Array.from(x).sort();
const overflowed = (n) => { if (!Number.isSafeInteger(n)) fail("integer overflow"); return n; };

const mathDomain = (f, x) => fail(`\`math.${f}\`: math domain error (${display(x)})`);
// A float answer, an Fl when whole (FEEDBACK55.md, Q-775; branch feedback55).
const mathChecked = (f, x, y, ok) => {
  if (!ok || (Number.isNaN(y) && !Number.isNaN(x))) mathDomain(f, x);
  if (!Number.isFinite(y) && !Number.isNaN(y) && Number.isFinite(x)) fail(`\`math.${f}\`: math range error (${display(x)})`);
  return fl(y);
};
const checked2 = (f, x, y, r, ok) => {
  const nanIn = Number.isNaN(x) || Number.isNaN(y);
  if (!ok || (Number.isNaN(r) && !nanIn)) fail(`\`math.${f}\`: math domain error (${display(x)}, ${display(y)})`);
  if (!Number.isFinite(r) && !Number.isNaN(r) && Number.isFinite(x) && Number.isFinite(y)) fail(`\`math.${f}\`: math range error (${display(x)}, ${display(y)})`);
  return fl(r);
};
const unary = (f, g, dom = () => true) => (x) => { x = N(x, "math." + f); return mathChecked(f, x, g(x), Number.isNaN(x) || dom(x)); };

// erf: a series below 2, else from erfc's continued fraction.
const erfc = (x) => {
  if (Number.isNaN(x)) return x;
  if (x < 2) return 1 - erf(x);
  if (x > 27) return 0;
  // Lentz's method for x + 1/2 / (x + 1 / (x + 3/2 / (x + ...))).
  let f = x, C = x, D = 0;
  for (let n = 1; n < 500; n++) {
    const a = n / 2;
    D = x + a * D; D = D === 0 ? 1e-300 : 1 / D;
    C = x + a / C; if (C === 0) C = 1e-300;
    const d = C * D; f *= d;
    if (Math.abs(d - 1) < 1e-16) break;
  }
  return Math.exp(-x * x) / Math.sqrt(Math.PI) / f;
};
const erf = (x) => {
  if (Number.isNaN(x)) return x;
  if (x < 0) return -erf(-x);
  if (x >= 2) return 1 - erfc(x);
  // 2/√π e^(-x²) Σ 2^n x^(2n+1) / (1·3·…·(2n+1)): terms all positive.
  let term = x, s = x;
  for (let n = 1; n < 200; n++) { term *= 2 * x * x / (2 * n + 1); s += term; if (term < s * 1e-17) break; }
  return 2 / Math.sqrt(Math.PI) * Math.exp(-x * x) * s;
};
const LANCZOS = [0.99999999999980993, 676.5203681218851, -1259.1392167224028, 771.32342877765313, -176.61502916214059, 12.507343278686905, -0.13857109526572012, 9.9843695780195716e-6, 1.5056327351493116e-7];
const lanczos = (x) => { x -= 1; let a = LANCZOS[0]; for (let i = 1; i < 9; i++) a += LANCZOS[i] / (x + i); return [a, x + 7.5, x]; };
const pole = (x) => !(x <= 0 && Number.isInteger(x)) && x !== -Infinity;
const gamma = (x) => {
  if (x === Infinity || Number.isNaN(x)) return x;
  if (Number.isInteger(x) && x > 0 && x <= 23) { let r = 1; for (let k = 2; k < x; k++) r *= k; return r; }
  if (x < 0.5) return Math.PI / (Math.sin(Math.PI * x) * gamma(1 - x));
  const [a, t, z] = lanczos(x), h = Math.pow(t, (z + 0.5) / 2);
  return Math.sqrt(2 * Math.PI) * h * (h * Math.exp(-t)) * a;
};
const lgamma = (x) => {
  if (!Number.isFinite(x)) return Math.abs(x);
  if (x < 0.5) return Math.log(Math.PI / Math.abs(Math.sin(Math.PI * x))) - lgamma(1 - x);
  if (x === 1 || x === 2) return 0;
  const [a, t, z] = lanczos(x);
  return 0.5 * Math.log(2 * Math.PI) + (z + 0.5) * Math.log(t) - t + Math.log(a);
};
const norm2 = (v) => {
  if (v.some((x) => x === Infinity || x === -Infinity)) return Infinity;
  const big = v.reduce((m, x) => Math.max(m, Math.abs(x)), 0);
  if (big === 0 || Number.isNaN(big)) return big;
  return big * Math.sqrt(csum(v.map((x) => (x / big) * (x / big))));
};
// An int is itself; a float its whole part, to 9.2e18 and exact past 2^53, as the server's
// `rounded` (FEEDBACK55.md, Q-776; branch feedback55).
const toInt = (f, g) => (x) => {
  if (isWhole(x)) return x;
  x = N(x, f);
  if (!Number.isFinite(x)) fail(`\`${f}\`: cannot convert ${display(x)} to an int`);
  const y = g(x) + 0;
  // Past an int from 2^63, not 9.2e18, below it (FEEDBACK55.md, Q-793; branch feedback55).
  if (!(y >= -(2 ** 63) && y < 2 ** 63)) fail("integer overflow");
  return Number.isSafeInteger(y) ? y : BigInt(y);
};
const natural = (n, f, i = 1) => { n = NI(n, f, i); if (n < 0) fail(`\`${f}\` is not defined for negative numbers (${n})`); return n; };
const gcd2 = (a, b) => { a = a < 0n ? -a : a; b = b < 0n ? -b : b; while (b) [a, b] = [b, a % b]; return a; };
const intsOf = (args, f) => (args.length === 1 && Array.isArray(args[0]) ? args[0] : args).map((v, i) => BigInt(NI(v, f, i + 1)));
// Ints multiplied (or added) exactly, as BigInts past 2^53, and refused past 64 bits as on the
// server (FEEDBACK55.md, Q-776; branch feedback55).
const imul = (a, b) => { if (typeof a === "number" && typeof b === "number") { const r = a * b; if (Number.isSafeInteger(r)) return r + 0; } return bigChecked(BigInt(a) * BigInt(b)); };
const iadd = (a, b) => { if (typeof a === "number" && typeof b === "number") { const r = a + b; if (Number.isSafeInteger(r)) return r; } return bigChecked(BigInt(a) + BigInt(b)); };
const pairOf = (a, b, f) => { const [x, ix] = NSi(a, f), [y, iy] = NSi(b, f, 2); if (x.length !== y.length) fail(`\`${f}\` needs lists of the same length (${x.length} and ${y.length})`); return [x, y, ix && iy]; };
// Ints multiplied and added exactly; else a float, as if in twice the precision.
const dotOf = (a, b, f) => {
  const [x, y, ints] = pairOf(a, b, f);
  if (ints) { let s = 0; for (let i = 0; i < x.length; i++) s = iadd(s, imul(a[i], b[i])); return s; }
  return fl(cdot(x, y));
};

B.math = {
  pi: Math.PI, e: Math.E, tau: 2 * Math.PI, inf: Infinity, nan: NaN,
  sqrt: unary("sqrt", Math.sqrt, (x) => x >= 0),
  cbrt: unary("cbrt", Math.cbrt),
  exp: unary("exp", Math.exp),
  exp2: unary("exp2", (x) => 2 ** x),
  expm1: unary("expm1", Math.expm1),
  log: (x, b) => {
    x = N(x, "math.log");
    if (!given(b)) return mathChecked("log", x, Math.log(x), Number.isNaN(x) || x > 0);
    b = N(b, "math.log", 2);
    if (b <= 0 || b === 1) fail(`\`math.log\`: math domain error (base ${display(b)})`);
    return mathChecked("log", x, Math.log(x) / Math.log(b), Number.isNaN(x) || x > 0);
  },
  log2: unary("log2", Math.log2, (x) => x > 0),
  log10: unary("log10", Math.log10, (x) => x > 0),
  log1p: unary("log1p", Math.log1p, (x) => x > -1),
  pow: (x, y) => { x = N(x, "math.pow"); y = N(y, "math.pow", 2); return checked2("pow", x, y, Math.pow(x, y), !(x === 0 && y < 0)); },
  sin: unary("sin", Math.sin, Number.isFinite),
  cos: unary("cos", Math.cos, Number.isFinite),
  tan: unary("tan", Math.tan, Number.isFinite),
  asin: unary("asin", Math.asin, (x) => Math.abs(x) <= 1),
  acos: unary("acos", Math.acos, (x) => Math.abs(x) <= 1),
  atan: unary("atan", Math.atan),
  atan2: (y, x) => fl(Math.atan2(N(y, "math.atan2"), N(x, "math.atan2", 2))),
  sinh: unary("sinh", Math.sinh),
  cosh: unary("cosh", Math.cosh),
  tanh: unary("tanh", Math.tanh),
  asinh: unary("asinh", Math.asinh),
  acosh: unary("acosh", Math.acosh, (x) => x >= 1),
  atanh: unary("atanh", Math.atanh, (x) => Math.abs(x) < 1),
  degrees: unary("degrees", (x) => x * (180 / Math.PI)),
  radians: unary("radians", (x) => x * (Math.PI / 180)),
  hypot: (...v) => fl(norm2(v.map((x, i) => N(x, "math.hypot", i + 1)))),
  dist: (p, q) => {
    p = NS(p, "math.dist"); q = NS(q, "math.dist", 2);
    if (p.length !== q.length) fail(`\`math.dist\`: both points must have the same number of dimensions (${p.length} and ${q.length})`);
    return fl(norm2(p.map((x, i) => x - q[i])));
  },
  floor: divides(toInt("math.floor", Math.floor), "math.floor"),
  ceil: divides(toInt("math.ceil", Math.ceil), "math.ceil"),
  trunc: divides(toInt("math.trunc", Math.trunc), "math.trunc"),
  fabs: unary("fabs", Math.abs),
  copysign: (x, y) => { x = N(x, "math.copysign"); y = N(y, "math.copysign", 2); return fl((y < 0 || Object.is(y, -0)) ? -Math.abs(x) : Math.abs(x)); },
  fmod: (x, y) => { x = N(x, "math.fmod"); y = N(y, "math.fmod", 2); return checked2("fmod", x, y, x % y, !(y === 0 || !Number.isFinite(x)) || Number.isNaN(x) || Number.isNaN(y)); },
  remainder: (x, y) => {
    x = N(x, "math.remainder"); y = N(y, "math.remainder", 2);
    const ok = !(y === 0 || !Number.isFinite(x)) || Number.isNaN(x) || Number.isNaN(y);
    if (!Number.isFinite(y)) return checked2("remainder", x, y, x, ok);
    let n = Math.round(x / y);
    if (Math.abs(x / y - Math.trunc(x / y)) === 0.5 && n % 2) n -= 1;
    return checked2("remainder", x, y, x - n * y, ok);
  },
  modf: (x) => { x = N(x, "math.modf"); if (!Number.isFinite(x) && !Number.isNaN(x)) return [fl(x < 0 ? -0 : 0), x]; return [fl(x % 1), fl(Math.trunc(x))]; },
  frexp: (x) => {
    x = N(x, "math.frexp");
    if (x === 0 || !Number.isFinite(x)) return [fl(x), 0];
    let e = Math.max(-1073, Math.floor(Math.log2(Math.abs(x))) + 1), m = x * 2 ** -e;
    while (Math.abs(m) < 0.5) { m *= 2; e -= 1; }
    while (Math.abs(m) >= 1) { m /= 2; e += 1; }
    return [m, e];
  },
  ldexp: (x, e) => { x = N(x, "math.ldexp"); e = Number(NI(e, "math.ldexp", 2)); e = Math.min(Math.max(e, -10000), 10000); let r = x; while (e > 1000) { r *= 2 ** 1000; e -= 1000; } while (e < -1000) { r *= 2 ** -1000; e += 1000; } return mathChecked("ldexp", x, r * 2 ** e, true); },
  // Exactly, in BigInts past 2^53, to what an int holds, as on the server: `math.factorial(19)`
  // and `math.comb(60, 30)` were overflows (FEEDBACK55.md, Q-776; branch feedback55).
  factorial: (n) => { n = BigInt(natural(n, "math.factorial")); let r = 1n; for (let k = 2n; k <= n; k++) { r *= k; if (r > I64_MAX) fail("integer overflow"); } return intOf(r); },
  gcd: (...args) => { const v = intsOf(args, "math.gcd"); if (v.includes(I64_MIN)) fail("integer overflow"); return intOf(v.reduce(gcd2, 0n)); },
  lcm: (...args) => {
    const v = intsOf(args, "math.lcm");
    if (v.includes(0n)) return 0;
    let acc = 1n;
    for (const n of v) { acc = acc / gcd2(acc, n) * babs(n); if (acc > I64_MAX) fail("integer overflow"); }
    return intOf(acc);
  },
  comb: (n, k) => {
    n = BigInt(natural(n, "math.comb")); k = BigInt(natural(k, "math.comb", 2));
    if (k > n) return 0;
    if (n - k < k) k = n - k;
    let r = 1n;
    for (let i = 0n; i < k; i++) { r = r * (n - i) / (i + 1n); if (r > I64_MAX) fail("integer overflow"); }
    return intOf(r);
  },
  perm: (n, k) => {
    n = BigInt(natural(n, "math.perm")); k = given(k) ? BigInt(natural(k, "math.perm", 2)) : n;
    if (k > n) return 0;
    let r = 1n;
    for (let x = n - k + 1n; x <= n; x++) { r *= x; if (r > I64_MAX) fail("integer overflow"); }
    return intOf(r);
  },
  isqrt: (n) => { n = BigInt(natural(n, "math.isqrt")); let s = BigInt(Math.floor(Math.sqrt(Number(n)))); while (s * s > n) s--; while ((s + 1n) * (s + 1n) <= n) s++; return intOf(s); },
  isclose: (a, b, rel, abs) => {
    a = N(a, "math.isclose"); b = N(b, "math.isclose", 2);
    rel = given(rel) ? N(rel, "math.isclose", 3) : 1e-9; abs = given(abs) ? N(abs, "math.isclose", 4) : 0;
    if (rel < 0 || abs < 0) fail("`math.isclose`: tolerances must be non-negative");
    if (a === b) return true;
    if (!Number.isFinite(a) || !Number.isFinite(b)) return false;
    const d = Math.abs(a - b);
    return d <= Math.abs(rel * b) || d <= Math.abs(rel * a) || d <= abs;
  },
  isfinite: (x) => Number.isFinite(N(x, "math.isfinite")),
  isinf: (x) => { x = N(x, "math.isinf"); return x === Infinity || x === -Infinity; },
  isnan: (x) => Number.isNaN(N(x, "math.isnan")),
  // An int while all are, exactly (FEEDBACK55.md, Q-776; branch feedback55); else a float.
  prod: (l, start) => {
    let acc = given(start) ? start : 1;
    for (const x of L(l, "math.prod")) {
      if (isWhole(acc) && isWhole(x)) { acc = imul(acc, x); continue; }
      const a = numOf(acc), b = numOf(x);
      if (a === null || b === null) fail(`\`math.prod\` expects numbers, got ${typeName(x)}${decNote(x)}`);
      acc = fl(a * b);
    }
    return acc;
  },
  fsum: (l) => {
    const x = NS(l, "math.fsum"), s = fsum(x);
    if (Number.isNaN(s) && x.includes(Infinity) && x.includes(-Infinity)) fail("`math.fsum`: inf and -inf have no sum");
    if (!Number.isFinite(s) && x.every(Number.isFinite)) fail("`math.fsum`: the sum is too large for a float");
    return fl(s);
  },
  sumprod: (p, q) => dotOf(p, q, "math.sumprod"),
  erf: unary("erf", erf),
  erfc: unary("erfc", erfc),
  gamma: unary("gamma", gamma, pole),
  lgamma: unary("lgamma", lgamma, pole),
};

// The 64 bits of an int, as on the server: JavaScript's own `&` and `<<` keep
// 32. An int past 2^53 is a BigInt here (FEEDBACK19.md, Q-153; branch
// feedback19-fixes).
// A whole float too, as the server's `int` takes it (FEEDBACK55.md, Q-775; branch feedback55).
const bitsOf = (v, f, i = 1) => BigInt(NI(v, f, i));
const fromBits = (b) => intOf(BigInt.asIntN(64, b));
const shiftOf = (n, f) => { const k = bitsOf(n, f, 2); if (k < 0n || k > 63n) fail(`\`${f}\`: a shift is 0 to 63, got ${k}`); return k; };
B.bit = {
  and: (a, b) => fromBits(bitsOf(a, "bit.and") & bitsOf(b, "bit.and", 2)),
  or: (a, b) => fromBits(bitsOf(a, "bit.or") | bitsOf(b, "bit.or", 2)),
  xor: (a, b) => fromBits(bitsOf(a, "bit.xor") ^ bitsOf(b, "bit.xor", 2)),
  not: (a) => fromBits(~bitsOf(a, "bit.not")),
  shl: (a, n) => fromBits(bitsOf(a, "bit.shl") << shiftOf(n, "bit.shl")),
  shr: (a, n) => fromBits(bitsOf(a, "bit.shr") >> shiftOf(n, "bit.shr")),
};

const dataOfI = (l, f, least) => {
  const d = NSi(l, f);
  if (d[0].length < least) fail(`\`${f}\` requires ${least === 1 ? "at least one data point" : `at least ${least} data points`}`);
  return d;
};
const dataOf = (l, f, least) => dataOfI(l, f, least)[0];
// `dataOfI` where data are put in order: NaN and infinity refused (FEEDBACK55.md, Q-780).
const orderedOf = (l, f, least) => { const d = dataOfI(l, f, least); finiteOf(d[0], f, "data"); return d; };
const kth = (x, k) => sortedOf(x)[k];
const atPercent = (d, p) => { const h = p / 100 * (d.length - 1), lo = Math.floor(h), hi = Math.min(lo + 1, d.length - 1); return d[lo] + (d[hi] - d[lo]) * (h - lo); };
const ranksOf = (x) => {
  const idx = x.map((_, i) => i).sort((a, b) => x[a] - x[b] || a - b), r = new Array(x.length);
  for (let i = 0; i < idx.length;) {
    let j = i;
    while (j + 1 < idx.length && x[idx[j + 1]] === x[idx[i]]) j++;
    for (let k = i; k <= j; k++) r[idx[k]] = (i + j) / 2 + 1;
    i = j + 1;
  }
  return r;
};
const momentsOf = (x, y) => {
  const mx = meanOf(x), my = meanOf(y), dx = x.map((v) => v - mx), dy = y.map((v) => v - my);
  return [mx, my, cdot(dx, dx), cdot(dy, dy), cdot(dx, dy)];
};
const pairedOf = (x, y, f) => { [x, y] = pairOf(x, y, f); if (x.length < 2) fail(`\`${f}\` requires at least two data points`); return [x, y]; };
const varianceOf = (l, mu, f, sample) => { const x = dataOf(l, f, sample ? 2 : 1); return div2(ssOf(x, given(mu) ? N(mu, f, 2) : meanOf(x)), x.length - (sample ? 1 : 0)); };
// A number by its float, 1 and 1.0 one value, as on the server.
const tallyKey = (v) => isNumber(v) ? "n" + (numOf(v) + 0) : typeof v === "string" ? "s" + v : "v" + repr(v);
const tallyOf = (l) => { const at = new Map(), out = []; l.forEach((v, i) => { const k = tallyKey(v); if (at.has(k)) out[at.get(k)][1]++; else { at.set(k, out.length); out.push([i, 1]); } }); return out; };
const normalArgs = (x, mu, sigma, f) => { x = N(x, f); mu = given(mu) ? N(mu, f, 2) : 0; sigma = given(sigma) ? N(sigma, f, 3) : 1; if (!(sigma > 0)) fail(`\`${f}\`: sigma must be positive, got ${display(sigma)}`); return [x, mu, sigma]; };
const poly = (c, r) => c.reduce((acc, k) => acc * r + k, 0);
const invCdf = (p) => {
  const q = p - 0.5;
  if (Math.abs(q) <= 0.425) {
    const r = 0.180625 - q * q;
    return poly([2509.0809287301226727, 33430.575583588128105, 67265.770927008700853, 45921.953931549871457, 13731.693765509461125, 1971.5909503065514427, 133.14166789178437745, 3.387132872796366608], r) * q
      / poly([5226.495278852545561, 28729.085735721942674, 39307.89580009271061, 21213.794301586595867, 5394.1960214247511077, 687.1870074920579083, 42.313330701600911252, 1], r);
  }
  let r = Math.sqrt(-Math.log(q <= 0 ? p : 1 - p)), x;
  if (r <= 5) {
    r -= 1.6;
    x = poly([7.7454501427834140764e-4, 0.0227238449892691845833, 0.24178072517745061177, 1.27045825245236838258, 3.64784832476320460504, 5.7694972214606914055, 4.6303378461565452959, 1.42343711074968357734], r)
      / poly([1.05075007164441684324e-9, 5.475938084995344946e-4, 0.0151986665636164571966, 0.14810397642748007459, 0.68976733498510000455, 1.6763848301838038494, 2.05319162663775882187, 1], r);
  } else {
    r -= 5;
    x = poly([2.01033439929228813265e-7, 2.71155556874348757815e-5, 0.0012426609473880784386, 0.026532189526576123093, 0.29656057182850489123, 1.7848265399172913358, 5.4637849111641143699, 6.6579046435011037772], r)
      / poly([2.04426310338993978564e-15, 1.4215117583164458887e-7, 1.8463183175100546818e-5, 7.868691311456132591e-4, 0.0148753612908506148525, 0.13692988092273580531, 0.59983220655588793769, 1], r);
  }
  return q < 0 ? -x : x;
};
const linspaceOf = (a, b, n, endpoint) => {
  if (n === 0) return [];
  const step = (b - a) / (endpoint ? Math.max(n - 1, 1) : n), out = [];
  for (let i = 0; i < n; i++) out.push(a + i * step);
  if (endpoint && n > 1) out[n - 1] = b;
  return out;
};
const countOf = (n, f) => { n = NI(n, f); if (n < 0 || n > 1e7) fail(`\`${f}\`: ${n} is not a count from 0 to 10000000`); return Number(n); };

// Floats as the server gives them, an Fl when whole; an int where it gives one (FEEDBACK55.md,
// Q-775; branch feedback55).
B.stats = {
  mean: (l) => fl(meanOf(dataOf(l, "stats.mean", 1))),
  fmean: (l, w) => {
    const x = dataOf(l, "stats.fmean", 1);
    if (!given(w)) return fl(meanOf(x));
    w = NS(w, "stats.fmean", 2);
    if (w.length !== x.length) fail("`stats.fmean`: data and weights must be the same length");
    const total = csum(w);
    if (total === 0) fail("`stats.fmean`: the sum of the weights must be non-zero");
    return fl(cdot(x, w) / total);
  },
  geometric_mean: (l) => { const x = dataOf(l, "stats.geometric_mean", 1); if (x.some((v) => v <= 0)) fail("`stats.geometric_mean` requires positive numbers"); return fl(Math.exp(csum(x.map(Math.log)) / x.length)); },
  harmonic_mean: (l) => {
    const x = dataOf(l, "stats.harmonic_mean", 1);
    if (x.some((v) => v < 0)) fail("`stats.harmonic_mean` does not support negative values");
    if (x.some((v) => v === 0)) return new Fl(0);
    const r = x.map((v) => 1 / v), e = x.map((v, i) => { const [p, pe] = twoProd(r[i], v); return ((1 - p) - pe) / v; });
    return fl(x.length / csum(r.concat(e)));
  },
  median: (l) => { const [x, ints] = orderedOf(l, "stats.median", 1), d = sortedOf(x), n = d.length; return n % 2 ? back(d[n >> 1], ints) : fl((d[n / 2 - 1] + d[n / 2]) / 2); },
  median_low: (l) => { const [x, ints] = orderedOf(l, "stats.median_low", 1), d = sortedOf(x), n = d.length; return back(d[n % 2 ? n >> 1 : n / 2 - 1], ints); },
  median_high: (l) => { const [x, ints] = orderedOf(l, "stats.median_high", 1); return back(kth(x, x.length >> 1), ints); },
  mode: (l) => { l = L(l, "stats.mode"); const t = tallyOf(l); if (!t.length) fail("`stats.mode`: no mode for empty data"); const most = Math.max(...t.map((x) => x[1])); return l[t.find((x) => x[1] === most)[0]]; },
  multimode: (l) => { l = L(l, "stats.multimode"); const t = tallyOf(l), most = Math.max(0, ...t.map((x) => x[1])); return t.filter((x) => x[1] === most).map((x) => l[x[0]]); },
  pvariance: (l, mu) => fl(varianceOf(l, mu, "stats.pvariance", false)),
  variance: (l, mu) => fl(varianceOf(l, mu, "stats.variance", true)),
  pstdev: (l, mu) => fl(Math.sqrt(varianceOf(l, mu, "stats.pstdev", false))),
  stdev: (l, mu) => fl(Math.sqrt(varianceOf(l, mu, "stats.stdev", true))),
  quantiles: (l, n, method) => {
    const f = "stats.quantiles", d = sortedOf(orderedOf(l, f, 1)[0]), ld = d.length;
    n = given(n) ? Number(NI(n, f, 2)) : 4;
    if (n < 1 || n > 10000) fail(`\`${f}\`: n must be from 1 to 10000, got ${n}`);
    method = given(method) ? S(method, f, 3) : "exclusive";
    if (ld === 1) return new Array(n - 1).fill(fl(d[0]));
    const out = [];
    if (method === "inclusive") {
      const m = ld - 1;
      for (let i = 1; i < n; i++) { const j = Math.floor(i * m / n), delta = i * m - j * n; out.push(fl((d[j] * (n - delta) + d[j + 1] * delta) / n)); }
    } else if (method === "exclusive") {
      const m = ld + 1;
      for (let i = 1; i < n; i++) { const j = Math.min(Math.max(Math.floor(i * m / n), 1), ld - 1), delta = i * m - j * n; out.push(fl((d[j - 1] * (n - delta) + d[j] * delta) / n)); }
    } else fail(`\`${f}\`: unknown method ${JSON.stringify(method)} (exclusive or inclusive)`);
    return out;
  },
  percentile: (l, p) => {
    const f = "stats.percentile", d = sortedOf(orderedOf(l, f, 1)[0]);
    const check = (p) => { if (!(p >= 0 && p <= 100)) fail(`\`${f}\`: percentiles are from 0 to 100, got ${display(p)}`); return p; };
    if (Array.isArray(p)) return NS(p, f, 2).map(check).map((q) => fl(atPercent(d, q)));
    return fl(atPercent(d, check(N(p, f, 2))));
  },
  covariance: (x, y) => { [x, y] = pairedOf(x, y, "stats.covariance"); return fl(momentsOf(x, y)[4] / (x.length - 1)); },
  correlation: (x, y, method) => {
    const f = "stats.correlation";
    [x, y] = pairedOf(x, y, f);
    method = given(method) ? S(method, f, 3) : "linear";
    if (method === "ranked") { x = ranksOf(x); y = ranksOf(y); } else if (method !== "linear") fail(`\`${f}\`: unknown method ${JSON.stringify(method)} (linear or ranked)`);
    const [, , sxx, syy, sxy] = momentsOf(x, y);
    if (sxx === 0 || syy === 0) fail(`\`${f}\`: at least one of the inputs is constant`);
    return fl(Math.min(1, Math.max(-1, sxy / Math.sqrt(sxx * syy))));
  },
  linear_regression: (x, y, proportional) => {
    const f = "stats.linear_regression";
    [x, y] = pairedOf(x, y, f);
    if (truthy(proportional)) { const sxx = cdot(x, x); if (sxx === 0) fail(`\`${f}\`: x is all zeros`); return { slope: fl(cdot(x, y) / sxx), intercept: new Fl(0) }; }
    const [mx, my, sxx, , sxy] = momentsOf(x, y);
    if (sxx === 0) fail(`\`${f}\`: x is constant`);
    const slope = sxy / sxx;
    return { slope: fl(slope), intercept: fl(my - slope * mx) };
  },
  describe: (l) => {
    const [x, ints] = NSi(l, "stats.describe"), n = x.length;
    finiteOf(x, "stats.describe", "data");
    const total = csum(x), d = sortedOf(x), q = (p) => n ? fl(atPercent(d, p)) : null;
    const mean = n ? total / n : null;
    return { count: n, sum: back(total, ints), mean: n ? fl(mean) : null, std: n > 1 ? fl(Math.sqrt(div2(ssOf(x, mean), n - 1))) : null, min: n ? back(d[0], ints) : null, q1: q(25), median: q(50), q3: q(75), max: n ? back(d[n - 1], ints) : null };
  },
  histogram: (l, bins) => {
    const f = "stats.histogram", x = finiteOf(NS(l, f), f, "data");
    let edges;
    if (Array.isArray(bins)) {
      edges = finiteOf(NS(bins, f, 2), f, "edges");
      if (edges.length < 2 || edges.some((e, i) => i && e < edges[i - 1])) fail(`\`${f}\`: edges must be at least two, increasing`);
    } else {
      bins = given(bins) ? Number(NI(bins, f, 2)) : 10;
      if (bins < 1 || bins > 1e6) fail(`\`${f}\`: bins must be from 1 to 1000000, got ${bins}`);
      let lo = Infinity, hi = -Infinity;
      for (const v of x) { lo = Math.min(lo, v); hi = Math.max(hi, v); }
      if (!x.length) [lo, hi] = [0, 1];
      if (lo === hi) [lo, hi] = [lo - 0.5, hi + 0.5];
      if (!Number.isFinite(hi - lo)) fail(`\`${f}\`: the data spread over more than a float can hold, ${display(lo)} to ${display(hi)}: give the edges`);
      edges = linspaceOf(lo, hi, bins + 1, true);
    }
    const nb = edges.length - 1, lo = edges[0], hi = edges[nb], counts = new Array(nb).fill(0);
    for (const v of x) {
      if (!(v >= lo && v <= hi)) continue;
      let a = 0, b = nb;
      while (a < b) { const m = (a + b) >> 1; if (edges[m + 1] <= v) a = m + 1; else b = m; }
      counts[Math.min(a, nb - 1)]++;
    }
    return { counts, edges: fls(edges) };
  },
  zscore: (l) => { const x = NS(l, "stats.zscore"); if (!x.length) return []; const m = meanOf(x), sd = Math.sqrt(div2(ssOf(x, m), x.length)); return x.map((v) => fl(sd === 0 ? 0 : (v - m) / sd)); },
  normalize: (l) => { const x = NS(l, "stats.normalize"); let lo = Infinity, hi = -Infinity; for (const v of x) { lo = Math.min(lo, v); hi = Math.max(hi, v); } const w = hi - lo; return x.map((v) => fl(w > 0 ? (v - lo) / w : 0)); },
  rank: (l) => ranksOf(finiteOf(NS(l, "stats.rank"), "stats.rank", "data")).map((r) => back(r, true)),
  normal_pdf: (x, mu, sigma) => { [x, mu, sigma] = normalArgs(x, mu, sigma, "stats.normal_pdf"); const z = (x - mu) / sigma; return fl(Math.exp(-0.5 * z * z) / (sigma * Math.sqrt(2 * Math.PI))); },
  normal_cdf: (x, mu, sigma) => { [x, mu, sigma] = normalArgs(x, mu, sigma, "stats.normal_cdf"); return fl(0.5 * (1 + erf((x - mu) / (sigma * Math.SQRT2)))); },
  normal_inv: (p, mu, sigma) => {
    [p, mu, sigma] = normalArgs(p, mu, sigma, "stats.normal_inv");
    if (!(p > 0 && p < 1)) fail(`\`stats.normal_inv\`: p must be between 0 and 1 (exclusive), got ${display(p)}`);
    return fl(mu + invCdf(p) * sigma);
  },
};

// As the operators do: ints stay ints, exactly past 2^53 too (`/` when it divides exactly), else
// floats, an Fl when whole (FEEDBACK55.md, Q-775 and Q-776; branch feedback55).
const vecArith = (op, f) => (a, b) => {
  if (isWhole(a) && isWhole(b)) {
    if ((op === "/" || op === "%") && (b === 0 || b === 0n)) fail(`\`${f}\`: division by zero`);
    if (typeof a === "number" && typeof b === "number") {
      const r = op === "+" ? a + b : op === "-" ? a - b : op === "*" ? a * b : op === "%" ? a % b : a / b;
      if (Number.isSafeInteger(r)) return r + 0;
      if (op === "/") return r;
    }
    return bigArith(a, b, op, f);
  }
  const x = numOf(a), y = numOf(b);
  if (x === null || y === null) fail(`\`${f}\` expects numbers, got ${typeName(a)} and ${typeName(b)}${decNote(b) || decNote(a)}`);
  return fl(op === "+" ? x + y : op === "-" ? x - y : op === "*" ? x * y : op === "%" ? x % y : x / y);
};
const elementwise = (op, f) => (a, b) => {
  const g = vecArith(op, f);
  if (Array.isArray(a) && Array.isArray(b)) { if (a.length !== b.length) fail(`\`${f}\` needs lists of the same length (${a.length} and ${b.length})`); return a.map((x, i) => g(x, b[i])); }
  if (Array.isArray(a)) return a.map((x) => g(x, b));
  if (Array.isArray(b)) return b.map((y) => g(a, y));
  return g(a, b);
};
const accumulate = (op, f) => (l) => {
  const g = vecArith(op, f), out = [];
  for (const x of L(l, f)) {
    if (out.length) out.push(g(out[out.length - 1], x));
    else if (isNumber(x)) out.push(x);
    else fail(`\`${f}\` expects numbers, got ${typeName(x)}${decNote(x)}`);
  }
  return out;
};
const argExtreme = (f, better) => (l) => { const x = NS(l, f); let best = null; x.forEach((v, i) => { if (!Number.isNaN(v) && (best === null || better(v, x[best]))) best = i; }); return best; };
// Numbers in order with NaN after all, as the server's sort keys have them (FEEDBACK55.md, Q-780;
// branch feedback55).
const nanLast = (u, v) => (Number.isNaN(u) ? (Number.isNaN(v) ? 0 : 1) : Number.isNaN(v) ? -1 : u < v ? -1 : u > v ? 1 : 0);
const rollingOf = (l, w, f) => {
  const x = NS(l, f);
  w = Number(NI(w, f, 2));
  if (w < 1) fail(`\`${f}\`: the window must be at least 1, got ${w}`);
  const out = [];
  for (let i = 0; i + w <= x.length; i++) out.push(csum(x.slice(i, i + w)));
  return [out, w];
};
// Rows of numbers, all of one length, as floats.
const matrixOf = (rows, f, i = 1) => {
  rows = L(rows, f, i);
  const cols = rows.length ? (Array.isArray(rows[0]) ? rows[0].length : fail(`\`${f}\` expects a list of rows, but row 0 is ${typeName(rows[0])}`)) : 0;
  const out = rows.map((r, k) => {
    if (!Array.isArray(r)) fail(`\`${f}\` expects a list of rows, but row ${k} is ${typeName(r)}`);
    if (r.length !== cols) fail(`\`${f}\`: row ${k} has ${r.length} numbers, row 0 has ${cols}`);
    return r.map((v) => { const x = numOf(v); if (x === null) fail(`\`${f}\` expects numbers, but row ${k} holds ${typeName(v)}${decNote(v)}`); return x; });
  });
  return [out, cols];
};

B.vec = {
  add: elementwise("+", "vec.add"),
  sub: elementwise("-", "vec.sub"),
  mul: elementwise("*", "vec.mul"),
  div: elementwise("/", "vec.div"),
  mod: elementwise("%", "vec.mod"),
  pow: (l, p) => { const x = NS(l, "vec.pow"); p = N(p, "vec.pow", 2); return x.map((v) => fl(p === 2 ? v * v : v ** p)); },
  dot: (a, b) => dotOf(a, b, "vec.dot"),
  norm: (l, p) => {
    const x = NS(l, "vec.norm");
    p = given(p) ? N(p, "vec.norm", 2) : 2;
    if (p === 2) return fl(norm2(x));
    if (p === 1) return fl(csum(x.map(Math.abs)));
    if (p === Infinity) return fl(x.reduce((m, v) => (Number.isNaN(m) || Number.isNaN(v) ? NaN : Math.max(m, Math.abs(v))), 0));
    return fl(csum(x.map((v) => Math.abs(v) ** p)) ** (1 / p));
  },
  cumsum: accumulate("+", "vec.cumsum"),
  cumprod: accumulate("*", "vec.cumprod"),
  diff: (l) => { l = L(l, "vec.diff"); const g = vecArith("-", "vec.diff"), out = []; for (let i = 1; i < l.length; i++) out.push(g(l[i], l[i - 1])); return out; },
  argmin: argExtreme("vec.argmin", (a, b) => a < b),
  argmax: argExtreme("vec.argmax", (a, b) => a > b),
  argsort: (l, desc) => { const x = NS(l, "vec.argsort"), d = truthy(desc); return x.map((_, i) => i).sort((a, b) => (d ? nanLast(x[b], x[a]) : nanLast(x[a], x[b])) || a - b); },
  // Each kept within lo..hi, as it is, or lo or hi as given.
  clip: (l, lo, hi) => {
    l = L(l, "vec.clip");
    const a = given(lo) ? N(lo, "vec.clip", 2) : null, b = given(hi) ? N(hi, "vec.clip", 3) : null;
    return l.map((v, k) => { const x = numOf(v); if (x === null) fail(`\`vec.clip\` expects a list of numbers, but item ${k} is ${typeName(v)}${decNote(v)}`); return a !== null && x < a ? lo : b !== null && x > b ? hi : v; });
  },
  abs: (l) => { const [x, ints] = NSi(l, "vec.abs"); return x.map((v) => back(Math.abs(v), ints)); },
  sqrt: (l) => NS(l, "vec.sqrt").map((v) => fl(Math.sqrt(v))),
  exp: (l) => NS(l, "vec.exp").map((v) => fl(Math.exp(v))),
  log: (l) => NS(l, "vec.log").map((v) => fl(Math.log(v))),
  round: (l, d) => {
    const x = NS(l, "vec.round");
    if (!given(d)) return x.map((v) => back(roundHalf(v), true));
    const p = 10 ** Math.min(Math.max(Number(NI(d, "vec.round", 2)), 0), 15);
    return x.map((v) => fl(roundHalf(v * p) / p));
  },
  linspace: (a, b, n, endpoint) => { a = N(a, "vec.linspace"); b = N(b, "vec.linspace", 2); return fls(linspaceOf(a, b, countOf(given(n) ? n : 50, "vec.linspace"), given(endpoint) ? truthy(endpoint) : true)); },
  // Ints when start and step are, exactly (FEEDBACK55.md, Q-776); else floats.
  arange: (start, stop, step) => {
    const f = "vec.arange";
    if (!given(stop)) [start, stop] = [0, start];
    if (!given(step)) step = 1;
    const a = N(start, f), b = N(stop, f, 2), st = N(step, f, 3);
    if (st === 0 || !Number.isFinite(st)) fail(`\`${f}\`: step must be a non-zero number`);
    const k = Math.max(0, Math.ceil((b - a) / st)), n = countOf(Number.isFinite(k) ? k : -1, f), out = [];
    const ints = isWhole(start) && isWhole(step);
    for (let i = 0; i < n; i++) out.push(ints ? iadd(start, imul(i, step)) : fl(a + i * st));
    return out;
  },
  zeros: (n) => new Array(countOf(n, "vec.zeros")).fill(0),
  ones: (n) => new Array(countOf(n, "vec.ones")).fill(1),
  full: (n, v) => new Array(countOf(n, "vec.full")).fill(v === undefined ? null : v),
  rolling_mean: (l, w) => { const [s, k] = rollingOf(l, w, "vec.rolling_mean"); return s.map((v) => fl(v / k)); },
  rolling_sum: (l, w) => fls(rollingOf(l, w, "vec.rolling_sum")[0]),
  matmul: (a, b) => {
    const f = "vec.matmul", [A, k] = matrixOf(a, f);
    if (Array.isArray(b) && b.length && !Array.isArray(b[0])) {
      const v = NS(b, f, 2);
      if (v.length !== k) fail(`\`${f}\`: a has ${k} columns, b has ${v.length} numbers`);
      return A.map((r) => fl(cdot(r, v)));
    }
    const [Bm, m] = matrixOf(b, f, 2);
    if (Bm.length !== k) fail(`\`${f}\`: a has ${k} columns, b has ${Bm.length} rows`);
    return A.map((r) => { const out = new Array(m).fill(0); r.forEach((x, p) => { for (let j = 0; j < m; j++) out[j] += x * Bm[p][j]; }); return fls(out); });
  },
  transpose: (l) => {
    const rows = L(l, "vec.transpose");
    if (!rows.length) return [];
    rows.forEach((r, k) => { if (!Array.isArray(r)) fail(`\`vec.transpose\` expects a list of rows, but row ${k} is ${typeName(r)}`); if (r.length !== rows[0].length) fail(`\`vec.transpose\`: row ${k} has ${r.length} items, row 0 has ${rows[0].length}`); });
    return rows[0].map((_, j) => rows.map((r) => r[j]));
  },
  column: (l, key) => {
    const rows = L(l, "vec.column");
    if (typeof key === "string") return rows.map((r) => isMap(r) && hasOwn(r, key) ? r[key] : null);
    if (isInt(key)) return rows.map((r) => Array.isArray(r) && key >= 0 && key < r.length ? r[key] : null);
    fail(`\`vec.column\` expects a str or int key as argument 2, got ${typeName(key)}`);
  },
};

// ---- date (src/dates.rs, the same answers) --------------------------------
// Back after 205d7cd removed it, without business days, Easter and day counts
// (FEEDBACK2.md, Q-027; branch feedback6-fixes).
// A date is "2026-10-27" and a date-time "2026-10-27T14:30:00Z": text, as on
// the server. The work is on day numbers (days from 1970-01-01) with the
// server's integer algorithms; time zones come from Intl.DateTimeFormat.
// Only the clock (date.today, date.now) differs from the server.

const dtMod = (a, b) => a - Math.floor(a / b) * b;
function dtDays(y, m, d) {
  y = m <= 2 ? y - 1 : y;
  const era = Math.floor(y / 400), yoe = y - era * 400;
  const doy = Math.floor((153 * ((m + 9) % 12) + 2) / 5) + d - 1;
  const doe = yoe * 365 + Math.floor(yoe / 4) - Math.floor(yoe / 100) + doy;
  return era * 146097 + doe - 719468;
}
function dtCivil(z) {
  z += 719468;
  const era = Math.floor(z / 146097), doe = z - era * 146097;
  const yoe = Math.floor((doe - Math.floor(doe / 1460) + Math.floor(doe / 36524) - Math.floor(doe / 146096)) / 365);
  const doy = doe - (365 * yoe + Math.floor(yoe / 4) - Math.floor(yoe / 100));
  const mp = Math.floor((5 * doy + 2) / 153);
  const d = doy - Math.floor((153 * mp + 2) / 5) + 1;
  const m = mp < 10 ? mp + 3 : mp - 9;
  return [yoe + era * 400 + (m <= 2 ? 1 : 0), m, d];
}
// 1 is Monday, 7 Sunday (ISO).
const dtWeekday = (z) => dtMod(z + 3, 7) + 1;
const dtLeap = (y) => y % 4 === 0 && (y % 100 !== 0 || y % 400 === 0);
const dtMonthLen = (y, m) => m === 2 ? (dtLeap(y) ? 29 : 28) : (m === 4 || m === 6 || m === 9 || m === 11) ? 30 : 31;
const dtInRange = (y) => y >= 1 && y <= 9999;
const dtPad = (n, w = 2) => String(n).padStart(w, "0");
const dtText = (z) => { const [y, m, d] = dtCivil(z); return `${dtPad(y, 4)}-${dtPad(m)}-${dtPad(d)}`; };
function dtAddMonths(z, n) {
  const [y, m, d] = dtCivil(z);
  if (Math.abs(n) > 120000) return null;
  const total = y * 12 + (m - 1) + n, ny = Math.floor(total / 12), nm = dtMod(total, 12) + 1;
  return dtInRange(ny) ? dtDays(ny, nm, Math.min(d, dtMonthLen(ny, nm))) : null;
}
const dtNum = (s) => /^[0-9]+$/.test(s) ? Number(s) : null;
const dtYmd = (y, m, d) => y !== null && m !== null && d !== null && dtInRange(y) && m >= 1 && m <= 12 && d >= 1 && d <= dtMonthLen(y, m) ? dtDays(y, m, d) : null;
function dtDate(s) {
  if (typeof s !== "string" || s.length !== 10 || s[4] !== "-" || s[7] !== "-") return null;
  return dtYmd(dtNum(s.slice(0, 4)), dtNum(s.slice(5, 7)), dtNum(s.slice(8)));
}
function dtHms(s) {
  const h = dtNum(s.slice(0, 2)), m = dtNum(s.slice(3, 5)), x = dtNum(s.slice(6, 8));
  return h === null || m === null || x === null || s[2] !== ":" || s[5] !== ":" || h >= 24 || m >= 60 || x >= 60 ? null : h * 3600 + m * 60 + x;
}
// "Z", "+01:00", "-04:00" (and "+00:09:21", the old local mean times).
function dtOffset(s) {
  if (s === "Z") return 0;
  const sign = s[0] === "+" ? 1 : s[0] === "-" ? -1 : 0;
  if (!sign || (s.length !== 6 && !(s.length === 9 && s[6] === ":")) || s[3] !== ":") return null;
  const h = dtNum(s.slice(1, 3)), m = dtNum(s.slice(4, 6)), x = s.length === 9 ? dtNum(s.slice(7, 9)) : 0;
  return h === null || m === null || x === null || h >= 24 || m >= 60 || x >= 60 ? null : sign * (h * 3600 + m * 60 + x);
}
const dtAscii = (s) => /^[\x00-\x7f]*$/.test(s);
// { day, sec, frac, off }: the local date and time as written, the fraction
// of a second as written, and the offset in seconds east of UTC.
function dtStamp(s) {
  if (typeof s !== "string" || s.length < 20 || !dtAscii(s) || s[10] !== "T") return null;
  const day = dtDate(s.slice(0, 10)), sec = dtHms(s.slice(11, 19));
  if (day === null || sec === null) return null;
  let i = 19;
  if (s[i] === ".") {
    i++;
    while (i < s.length && s[i] >= "0" && s[i] <= "9") i++;
    if (i === 20 || i > 29) return null;
  }
  const off = dtOffset(s.slice(i));
  return off === null ? null : { day, sec, frac: s.slice(19, i), off };
}
const dtInstant = (t) => t.day * 86400 + t.sec - t.off;
function dtOffsetText(off) {
  if (off === 0) return "Z";
  const a = Math.abs(off), hm = (off < 0 ? "-" : "+") + dtPad(Math.floor(a / 3600)) + ":" + dtPad(Math.floor(a % 3600 / 60));
  return a % 60 ? hm + ":" + dtPad(a % 60) : hm;
}
function dtStampText(instant, frac, off) {
  const local = instant + off, day = Math.floor(local / 86400), sec = dtMod(local, 86400);
  if (!dtInRange(dtCivil(day)[0])) return null;
  return `${dtText(day)}T${dtPad(Math.floor(sec / 3600))}:${dtPad(Math.floor(sec % 3600 / 60))}:${dtPad(sec % 60)}${frac}${dtOffsetText(off)}`;
}

const dtGot = (v) => v === undefined ? "nothing" : typeof v === "string" ? JSON.stringify(v) : typeName(v);
function dtDateArg(v, f, i) {
  const z = dtDate(v);
  if (z !== null) return z;
  if (dtStamp(v)) fail(`\`${f}\` expects a date (YYYY-MM-DD) as argument ${i}, got the date-time ${dtGot(v)} (\`date.date_in\` gives its date)`);
  fail(`\`${f}\` expects a date (YYYY-MM-DD) as argument ${i}, got ${dtGot(v)}`);
}
function dtStampArg(v, f, i) {
  const t = dtStamp(v);
  if (!t) fail(`\`${f}\` expects a date-time (like 2026-10-27T14:30:00Z) as argument ${i}, got ${dtGot(v)}`);
  return t;
}
// [day, stamp or null]
function dtEitherArg(v, f, i) {
  const z = dtDate(v);
  if (z !== null) return [z, null];
  const t = dtStamp(v);
  if (t) return [t.day, t];
  fail(`\`${f}\` expects a date or a date-time as argument ${i}, got ${dtGot(v)}`);
}
const dtInt = (v, f, i) => { if (!isInt(v)) fail(`\`${f}\` expects an int as argument ${i}, got ${dtGot(v)}`); return v; };
const dtStr = (v, f, i) => { if (typeof v !== "string") fail(`\`${f}\` expects a str as argument ${i}, got ${dtGot(v)}`); return v; };
const dtOutOfRange = (f) => fail(`\`${f}\`: the date is out of range (years 1 to 9999)`);
const dtDay = (z, f) => z !== null && dtInRange(dtCivil(z)[0]) ? dtText(z) : dtOutOfRange(f);

// A zone: a fixed offset (seconds), or a formatter that tells its local time.
const dtZones = new Map();
// Names the browser takes that are no zone of the IANA database, which the server's are: old ones
// it dropped and Java's three letters (FEEDBACK55.md, Q-782; branch feedback55); src/dates.rs's
// `not_a_zone` is the other half.
const NOT_ZONES = /^(SystemV\/.*|US\/Pacific-New|Canada\/East-Saskatchewan|ACT|AET|AGT|ART|AST|BET|BST|CAT|CNT|CTT|EAT|ECT|IET|IST|JST|MIT|NET|NST|PLT|PNT|PRT|PST|SST|VST)$/i;
function dtZone(v, f, i) {
  if (isNil(v)) return 0;
  if (typeof v !== "string") fail(`\`${f}\` expects a time zone (like "Europe/Paris") as argument ${i}, got ${dtGot(v)}`);
  if (v.toUpperCase() === "UTC" || v === "Z") return 0;
  // An offset is one as `date.parse` reads it, `+05:30`, `+0530` or `+05`, as on the server; no
  // other name with a sign goes to Intl, which takes some, and U+2212's minus (FEEDBACK55.md,
  // Q-829; branch feedback55).
  if (/^[-+\u2212]/.test(v)) { const o = dtWrittenOffset(v); if (o === null) fail(`\`${f}\`: unknown time zone ${dtGot(v)}`); return o; }
  let fmt = dtZones.get(v);
  if (!fmt) {
    if (NOT_ZONES.test(v)) fail(`\`${f}\`: unknown time zone ${dtGot(v)}`);
    try {
      fmt = new Intl.DateTimeFormat("en-US", { timeZone: v, hourCycle: "h23", era: "short", year: "numeric", month: "numeric", day: "numeric", hour: "numeric", minute: "numeric", second: "numeric" });
    } catch (e) {
      fail(`\`${f}\`: unknown time zone ${dtGot(v)}`);
    }
    dtZones.set(v, fmt);
  }
  return fmt;
}
// The zone's offset at an instant (seconds since 1970, UTC).
function dtOffsetAt(zone, instant) {
  if (typeof zone === "number") return zone;
  const p = {};
  for (const x of zone.formatToParts(new Date(instant * 1000))) p[x.type] = x.value;
  const y = p.era === "BC" || p.era === "B" ? 1 - Number(p.year) : Number(p.year);
  const local = dtDays(y, Number(p.month), Number(p.day)) * 86400 + (Number(p.hour) % 24) * 3600 + Number(p.minute) * 60 + Number(p.second);
  return local - instant;
}
// The instant a local time names: in a gap, that far after it; in an
// overlap, the earlier ("compatible", as on the server).
// `later` takes the second of two, and `skipped` is "later", "earlier" or "error" (null), as on
// the server (FEEDBACK30.md, Q-375; branch feedback27-35-fixes).
function dtLocalInstant(zone, local, later = false, skipped = "later") {
  const before = dtOffsetAt(zone, local - 86400), after = dtOffsetAt(zone, local + 86400);
  const fits = (o) => dtOffsetAt(zone, local - o) === o;
  const a = fits(before), b = fits(after);
  if (a && b) return later ? local - Math.min(before, after) : local - Math.max(before, after);
  if (a) return local - before;
  if (b) return local - after;
  return skipped === "earlier" ? local - after : skipped === "error" ? null : local - before;
}
// A date and a wall-clock time with no offset, as `datetime-local` gives it: [day, second].
const dtLocalDateTime = (s) => {
  if (typeof s !== "string" || !dtAscii(s) || (s.length !== 16 && s.length !== 19) || (s[10] !== "T" && s[10] !== " ")) return null;
  const day = dtDate(s.slice(0, 10)), sec = dtHms(s.length === 16 ? s.slice(11) + ":00" : s.slice(11));
  return day === null || sec === null ? null : [day, sec];
};
const dtAtLocal = (z, local, opts, f, n) => {
  let later = false, skipped = "later";
  if (!isNil(opts)) {
    if (!isMap(opts)) fail(`\`${f}\` expects options as a map as argument ${n}, got ${dtGot(opts)}`);
    // In the order set, as on the server (FEEDBACK55.md, Q-875; branch feedback55).
    for (const k of mkeys(opts)) {
      const v = opts[k];
      if (k === "repeated") { if (v !== "earlier" && v !== "later") fail(`\`${f}\`'s repeated is "earlier" or "later", got ${dtGot(v)}`); later = v === "later"; }
      else if (k === "skipped") { if (v !== "later" && v !== "earlier" && v !== "error") fail(`\`${f}\`'s skipped is "later", "earlier" or "error", got ${dtGot(v)}`); skipped = v; }
      else fail(`\`${f}\`: the options are repeated and skipped, not \`${k}\``);
    }
  }
  const i = dtLocalInstant(z, local, later, skipped);
  if (i === null) { const day = Math.floor(local / 86400), sec = dtMod(local, 86400); return new Err(`${f}: ${dtText(day)} ${dtPad(Math.floor(sec / 3600))}:${dtPad(Math.floor(sec % 3600 / 60))} is skipped in that zone, as its clocks move forward`); }
  return dtStampText(i, "", dtOffsetAt(z, i)) ?? dtOutOfRange(f);
};
// An all-zero fraction is none (FEEDBACK30.md, Q-368; branch feedback27-35-fixes).
const dtKeptFrac = (frac) => /^\.0*$/.test(frac || "") ? "" : frac;
// RFC 9110's HTTP date, as UTC.
const dtReadHttp = (s) => {
  const m = /^[A-Z][a-z]{2}, (\d{2}) (Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec) (\d{4}) (\d{2}:\d{2}:\d{2}) GMT$/.exec(s.split(/\s+/).join(" "));
  if (!m) return null;
  const day = dtYmd(Number(m[3]), "JanFebMarAprMayJunJulAugSepOctNovDec".indexOf(m[2]) / 3 + 1, Number(m[1])), sec = dtHms(m[4]);
  return day === null || sec === null ? null : dtStampText(day * 86400 + sec, "", 0);
};
const dtClock = () => Math.floor(Date.now() / 1000);

// A language's names and date styles (pattern tokens), after CLDR; the
// server has the same table.
const dtEn = {
  tags: ["en", "en-us"],
  months: ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"],
  mon: ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"],
  days: ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"],
  dy: ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"],
  styles: ["M/D/YY", "MMM D, YYYY", "MMMM D, YYYY", "dddd, MMMM D, YYYY"],
};
// A row for a language, and for each region CLDR writes otherwise, as on the server
// (FEEDBACK55.md, Q-828; branch feedback55): `row` with other tags, styles and names.
const dtOf = (row, tags, styles, names) => ({ ...row, ...names, tags, styles });
const dtGb = dtOf(dtEn, ["en-gb", "en-nz"], ["DD/MM/YYYY", "D MMM YYYY", "D MMMM YYYY", "dddd, D MMMM YYYY"],
  { mon: ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sept", "Oct", "Nov", "Dec"] });
const dtDe = {
  tags: ["de"],
  months: ["Januar", "Februar", "März", "April", "Mai", "Juni", "Juli", "August", "September", "Oktober", "November", "Dezember"],
  mon: ["Jan.", "Feb.", "März", "Apr.", "Mai", "Juni", "Juli", "Aug.", "Sept.", "Okt.", "Nov.", "Dez."],
  days: ["Montag", "Dienstag", "Mittwoch", "Donnerstag", "Freitag", "Samstag", "Sonntag"],
  dy: ["Mo.", "Di.", "Mi.", "Do.", "Fr.", "Sa.", "So."],
  styles: ["DD.MM.YY", "DD.MM.YYYY", "D. MMMM YYYY", "dddd, D. MMMM YYYY"],
};
const dtFr = {
  tags: ["fr"],
  months: ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"],
  mon: ["janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc."],
  days: ["lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche"],
  dy: ["lun.", "mar.", "mer.", "jeu.", "ven.", "sam.", "dim."],
  styles: ["DD/MM/YYYY", "D MMM YYYY", "D MMMM YYYY", "dddd D MMMM YYYY"],
};
const dtEs = {
  tags: ["es"],
  months: ["enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre", "octubre", "noviembre", "diciembre"],
  mon: ["ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sept", "oct", "nov", "dic"],
  days: ["lunes", "martes", "miércoles", "jueves", "viernes", "sábado", "domingo"],
  dy: ["lun", "mar", "mié", "jue", "vie", "sáb", "dom"],
  styles: ["D/M/YY", "D MMM YYYY", "D [de] MMMM [de] YYYY", "dddd, D [de] MMMM [de] YYYY"],
};
const dtIt = {
  tags: ["it"],
  months: ["gennaio", "febbraio", "marzo", "aprile", "maggio", "giugno", "luglio", "agosto", "settembre", "ottobre", "novembre", "dicembre"],
  mon: ["gen", "feb", "mar", "apr", "mag", "giu", "lug", "ago", "set", "ott", "nov", "dic"],
  days: ["lunedì", "martedì", "mercoledì", "giovedì", "venerdì", "sabato", "domenica"],
  dy: ["lun", "mar", "mer", "gio", "ven", "sab", "dom"],
  styles: ["DD/MM/YY", "D MMM YYYY", "D MMMM YYYY", "dddd D MMMM YYYY"],
};
const dtNl = {
  tags: ["nl"],
  months: ["januari", "februari", "maart", "april", "mei", "juni", "juli", "augustus", "september", "oktober", "november", "december"],
  mon: ["jan", "feb", "mrt", "apr", "mei", "jun", "jul", "aug", "sep", "okt", "nov", "dec"],
  days: ["maandag", "dinsdag", "woensdag", "donderdag", "vrijdag", "zaterdag", "zondag"],
  dy: ["ma", "di", "wo", "do", "vr", "za", "zo"],
  styles: ["DD-MM-YYYY", "D MMM YYYY", "D MMMM YYYY", "dddd D MMMM YYYY"],
};
const dtPt = {
  tags: ["pt"],
  months: ["janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro", "dezembro"],
  mon: ["jan.", "fev.", "mar.", "abr.", "mai.", "jun.", "jul.", "ago.", "set.", "out.", "nov.", "dez."],
  days: ["segunda-feira", "terça-feira", "quarta-feira", "quinta-feira", "sexta-feira", "sábado", "domingo"],
  dy: ["seg.", "ter.", "qua.", "qui.", "sex.", "sáb.", "dom."],
  styles: ["DD/MM/YYYY", "D [de] MMM [de] YYYY", "D [de] MMMM [de] YYYY", "dddd, D [de] MMMM [de] YYYY"],
};
const dtLocales = [dtEn, dtGb,
  dtOf(dtGb, ["en-ie"], ["DD/MM/YYYY", "D MMM YYYY", "D MMMM YYYY", "dddd D MMMM YYYY"]),
  dtOf(dtGb, ["en-au"], ["D/M/YY", "D MMM YYYY", "D MMMM YYYY", "dddd, D MMMM YYYY"], { mon: ["Jan", "Feb", "Mar", "Apr", "May", "June", "July", "Aug", "Sept", "Oct", "Nov", "Dec"] }),
  dtOf(dtGb, ["en-in"], ["DD/MM/YY", "D MMM YYYY", "D MMMM YYYY", "dddd, D MMMM YYYY"]),
  dtOf(dtGb, ["en-za"], ["YYYY/MM/DD", "DD MMM YYYY", "DD MMMM YYYY", "dddd, DD MMMM YYYY"]),
  dtOf(dtEn, ["en-ca"], ["YYYY-MM-DD", "MMM D, YYYY", "MMMM D, YYYY", "dddd, MMMM D, YYYY"]),
  dtDe,
  dtOf(dtDe, ["de-at"], dtDe.styles, {
    months: ["Jänner", "Februar", "März", "April", "Mai", "Juni", "Juli", "August", "September", "Oktober", "November", "Dezember"],
    mon: ["Jän.", "Feb.", "März", "Apr.", "Mai", "Juni", "Juli", "Aug.", "Sep.", "Okt.", "Nov.", "Dez."],
  }),
  dtFr,
  dtOf(dtFr, ["fr-ca"], ["YYYY-MM-DD", "D MMM YYYY", "D MMMM YYYY", "dddd D MMMM YYYY"], { mon: ["janv.", "févr.", "mars", "avr.", "mai", "juin", "juill.", "août", "sept.", "oct.", "nov.", "déc."] }),
  dtOf(dtFr, ["fr-ch"], ["DD.MM.YY", "D MMM YYYY", "D MMMM YYYY", "dddd, D MMMM YYYY"]),
  dtEs,
  dtOf(dtEs, ["es-mx"], ["DD/MM/YY", "D MMM YYYY", "D [de] MMMM [de] YYYY", "dddd, D [de] MMMM [de] YYYY"], { mon: ["ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sep", "oct", "nov", "dic"] }),
  dtIt,
  dtOf(dtIt, ["it-ch"], ["DD.MM.YY", "D MMM YYYY", "D MMMM YYYY", "dddd, D MMMM YYYY"]),
  dtNl,
  dtOf(dtNl, ["nl-be"], ["D/MM/YYYY", "D MMM YYYY", "D MMMM YYYY", "dddd D MMMM YYYY"]),
  dtPt,
  dtOf(dtPt, ["pt-pt"], ["DD/MM/YY", "DD/MM/YYYY", "D [de] MMMM [de] YYYY", "dddd, D [de] MMMM [de] YYYY"], { dy: ["segunda", "terça", "quarta", "quinta", "sexta", "sábado", "domingo"] }),
];
function dtNames(locale) {
  const tag = locale.trim().toLowerCase().replaceAll("_", "-"), lang = tag.split("-")[0];
  return dtLocales.find((n) => n.tags.includes(tag)) || dtLocales.find((n) => n.tags.includes(lang)) || dtEn;
}
// Longest first, so "MMMM" is not read as "MM" twice.
const dtTokens = ["YYYY", "YY", "MMMM", "MMM", "MM", "M", "DD", "D", "dddd", "ddd", "HH", "H", "hh", "h", "mm", "ss", "A", "Z"];
const dtTimeTokens = ["HH", "H", "hh", "h", "mm", "ss", "A", "Z"];
// A pattern's tokens: a token's text, or { lit } for text as it is ([brackets] and the rest).
function dtTokenize(p) {
  const out = [];
  let i = 0;
  next: while (i < p.length) {
    if (p[i] === "[") {
      let end = p.indexOf("]", i + 1);
      if (end < 0) end = p.length;
      out.push({ lit: p.slice(i + 1, end).join("") });
      i = Math.min(end + 1, p.length);
      continue;
    }
    for (const t of dtTokens) {
      if (p.slice(i, i + t.length).join("") === t) { out.push(t); i += t.length; continue next; }
    }
    out.push({ lit: p[i] });
    i++;
  }
  return out;
}
function dtRender(toks, n, z, t) {
  const [y, m, d] = dtCivil(z), sec = t ? t.sec : 0, off = t ? t.off : 0;
  const h = Math.floor(sec / 3600), mi = Math.floor(sec % 3600 / 60), s = sec % 60, h12 = (h + 11) % 12 + 1;
  const v = { YYYY: dtPad(y, 4), YY: dtPad(y % 100), MMMM: n.months[m - 1], MMM: n.mon[m - 1], MM: dtPad(m), M: String(m), DD: dtPad(d), D: String(d),
    dddd: n.days[dtWeekday(z) - 1], ddd: n.dy[dtWeekday(z) - 1], HH: dtPad(h), H: String(h), hh: dtPad(h12), h: String(h12), mm: dtPad(mi), ss: dtPad(s),
    A: h < 12 ? "AM" : "PM", Z: dtOffsetText(off) };
  return toks.map((x) => typeof x === "string" ? v[x] : x.lit).join("");
}
// Up to `max` digits at p.i (at least `min`).
function dtDigits(s, p, min, max) {
  const start = p.i;
  while (p.i < s.length && p.i - start < max && s[p.i] >= "0" && s[p.i] <= "9") p.i++;
  return p.i - start < min ? null : Number(s.slice(start, p.i).join(""));
}
// One of `words` at p.i, ignoring case, the longest that fits; its index.
function dtWord(s, p, words) {
  let best = null;
  words.forEach((w, k) => {
    const ws = Array.from(w);
    if (p.i + ws.length <= s.length && s.slice(p.i, p.i + ws.length).join("").toLowerCase() === w.toLowerCase() && (!best || ws.length > best[1])) best = [k, ws.length];
  });
  if (!best) return null;
  p.i += best[1];
  return best[0];
}
// Text read by a pattern: a date, or a date-time when the pattern has a time.
function dtRead(s, pattern) {
  let y = null, mo = null, d = null, h = null, mi = 0, se = 0, pm = null, twelve = false, off = 0, timed = false;
  const p = { i: 0 };
  for (const tok of dtTokenize(pattern)) {
    if (typeof tok !== "string") {
      for (const c of Array.from(tok.lit)) { if (s[p.i] !== c) return null; p.i++; }
      continue;
    }
    timed = timed || dtTimeTokens.includes(tok);
    let x;
    switch (tok) {
      case "YYYY": x = y = dtDigits(s, p, 4, 4); break;
      case "YY": x = dtDigits(s, p, 2, 2); y = x === null ? null : 2000 + x; break;
      case "MMMM": x = dtWord(s, p, dtEn.months); mo = x === null ? null : x + 1; break;
      case "MMM": x = dtWord(s, p, dtEn.mon); mo = x === null ? null : x + 1; break;
      case "MM": x = mo = dtDigits(s, p, 2, 2); break;
      case "M": x = mo = dtDigits(s, p, 1, 2); break;
      case "DD": x = d = dtDigits(s, p, 2, 2); break;
      case "D": x = d = dtDigits(s, p, 1, 2); break;
      case "dddd": x = dtWord(s, p, dtEn.days); break;
      case "ddd": x = dtWord(s, p, dtEn.dy); break;
      case "HH": x = h = dtDigits(s, p, 2, 2); break;
      case "H": x = h = dtDigits(s, p, 1, 2); break;
      case "hh": case "h":
        x = h = dtDigits(s, p, tok === "h" ? 1 : 2, 2);
        if (x !== null && (x < 1 || x > 12)) return null;
        twelve = true;
        break;
      case "mm": x = mi = dtDigits(s, p, 2, 2); break;
      case "ss": x = se = dtDigits(s, p, 2, 2); break;
      case "A": x = dtWord(s, p, ["AM", "PM"]); pm = x === 1; break;
      // "Z", "+01:00" or "+0100".
      case "Z": {
        const len = s[p.i] === "Z" || s[p.i] === "z" ? 1 : s[p.i + 3] === ":" ? 6 : 5;
        if (p.i + len > s.length) return null;
        let o = s.slice(p.i, p.i + len).join("");
        if (!dtAscii(o)) return null;
        if (len === 5) o = o.slice(0, 3) + ":" + o.slice(3);
        x = off = dtOffset(o.toUpperCase());
        p.i += len;
        break;
      }
    }
    if (x === null) return null;
  }
  if (p.i !== s.length) return null;
  const day = dtYmd(y, mo, d);
  if (day === null) return null;
  if (!timed) return dtText(day);
  // "12:30 AM" is 00:30, "12:30 PM" 12:30.
  let hour = h === null ? 0 : h;
  if (twelve && pm !== null) hour = hour % 12 + (pm ? 12 : 0);
  if (hour > 23 || mi > 59 || se > 59) return null;
  return dtStampText(day * 86400 + hour * 3600 + mi * 60 + se - off, "", off);
}
// A date, or a date-time with "T", "t" or a space, seconds or not, and an
// offset ("Z", "+01:00", "+0100", "+01"), as this module writes them.
function dtReadIso(s) {
  const z = dtDate(s);
  if (z !== null) return dtText(z);
  if (s.length < 16 || !dtAscii(s) || !"Tt ".includes(s[10])) return null;
  const day = dtDate(s.slice(0, 10));
  let sec = dtHms(s.slice(11, 16) + ":00");
  if (day === null || sec === null) return null;
  let i = 16;
  if (s[16] === ":") {
    const x = s.length >= 19 ? dtNum(s.slice(17, 19)) : null;
    if (x === null || x > 59) return null;
    sec += x;
    i = 19;
  }
  const fs = i;
  if (s[i] === "." || s[i] === ",") {
    i++;
    while (i < s.length && s[i] >= "0" && s[i] <= "9") i++;
    if (i === fs + 1 || i > fs + 10) return null;
  }
  const frac = i > fs ? "." + s.slice(fs + 1, i) : "", o = s.slice(i);
  const off = o.length === 1 ? dtOffset(o.toUpperCase()) : dtWrittenOffset(o);
  return off === null ? null : dtStampText(day * 86400 + sec - off, frac, off);
}
// An offset as people write one: `+01`, `+0100`, `+01:00` (and `+00:09:21`), in seconds.
const dtWrittenOffset = (o) => o.length === 3 ? dtOffset(o + ":00") : o.length === 5 && !o.includes(":") ? dtOffset(o.slice(0, 3) + ":" + o.slice(3)) : dtOffset(o);

B.date = {
  today: (tz) => { const z = dtZone(tz, "date.today", 1), t = dtClock(); return dtText(Math.floor((t + dtOffsetAt(z, t)) / 86400)); },
  now: (tz) => { const z = dtZone(tz, "date.now", 1), t = dtClock(); return dtStampText(t, "", dtOffsetAt(z, t)) || ""; },
  parse: (s, pattern) => {
    dtStr(s, "date.parse", 1);
    if (isNil(pattern)) {
      if (dtLocalDateTime(s.trim()) !== null) return new Err(`${dtGot(s)} has no offset, so it is a wall-clock time and names no instant: read it in its zone with date.at(v, zone)`);
      const r = dtReadIso(s.trim()) ?? dtReadHttp(s.trim()); return r === null ? new Err(`not a date or a date-time: ${dtGot(s)}`) : r;
    }
    if (typeof pattern !== "string") fail(`\`date.parse\` expects a pattern (like "DD/MM/YYYY") as argument 2, got ${dtGot(pattern)}`);
    const r = dtRead(Array.from(s.trim()), Array.from(pattern));
    return r === null ? new Err(`${dtGot(s)} is not a date in the form ${dtGot(pattern)}`) : r;
  },
  // "date" or "date-time" as the second argument takes only that (FEEDBACK13.md,
  // Q-093; branch feedback13-fixes).
  valid_zone: (z) => { if (typeof z !== "string") return false; try { dtZone(z, "date.valid_zone", 1); return true; } catch (e) { return false; } },
  valid: (s, kind) => {
    if (!isNil(kind) && kind !== "date" && kind !== "date-time") fail(`\`date.valid\` takes "date" or "date-time" as argument 2, got ${dtGot(kind)}`);
    return (kind !== "date-time" && dtDate(s) !== null) || (kind !== "date" && dtStamp(s) !== null);
  },
  of: (y, m, d) => {
    dtInt(y, "date.of", 1); dtInt(m, "date.of", 2); dtInt(d, "date.of", 3);
    const z = dtYmd(y, m, d);
    if (z === null) fail(`\`date.of\`: there is no date ${y}-${m}-${d}`);
    return dtText(z);
  },
  parts: (v) => {
    const [z, t] = dtEitherArg(v, "date.parts", 1), [year, month, day] = dtCivil(z);
    const out = { year, month, day, weekday: dtWeekday(z) };
    if (t) Object.assign(out, { hour: Math.floor(t.sec / 3600), minute: Math.floor(t.sec % 3600 / 60), second: t.sec % 60, offset: dtOffsetText(t.off) });
    return out;
  },
  weekday: (v) => dtWeekday(dtEitherArg(v, "date.weekday", 1)[0]),
  is_leap_year: (v) => dtLeap(typeof v === "string" ? dtCivil(dtEitherArg(v, "date.is_leap_year", 1)[0])[0] : dtInt(v, "date.is_leap_year", 1)),
  days_in_month: (a, b) => {
    const f = "date.days_in_month";
    let y, m;
    if (typeof a === "string") [y, m] = dtCivil(dtEitherArg(a, f, 1)[0]);
    else { y = dtInt(a, f, 1); m = dtInt(b, f, 2); }
    if (m < 1 || m > 12) fail(`\`${f}\`: there is no month ${m}`);
    return dtMonthLen(y, m);
  },
  add_days: (v, n) => { const z = dtDateArg(v, "date.add_days", 1); dtInt(n, "date.add_days", 2); return dtDay(Math.abs(n) <= 4000000 ? z + n : null, "date.add_days"); },
  add_months: (v, n) => { const z = dtDateArg(v, "date.add_months", 1); dtInt(n, "date.add_months", 2); return dtDay(dtAddMonths(z, n), "date.add_months"); },
  add_years: (v, n) => { const z = dtDateArg(v, "date.add_years", 1); dtInt(n, "date.add_years", 2); return dtDay(Math.abs(n) <= 10000 ? dtAddMonths(z, n * 12) : null, "date.add_years"); },
  days_between: (a, b) => { const x = dtDateArg(a, "date.days_between", 1); return dtDateArg(b, "date.days_between", 2) - x; },
  months_between: (a, b) => {
    a = dtDateArg(a, "date.months_between", 1); b = dtDateArg(b, "date.months_between", 2);
    const from = (a, b) => {
      if (b < a) return -from(b, a);
      const [y1, m1] = dtCivil(a), [y2, m2] = dtCivil(b), n = (y2 * 12 + m2) - (y1 * 12 + m1), x = dtAddMonths(a, n);
      return x === null || x > b ? n - 1 : n;
    };
    return from(a, b);
  },
  start_of_month: (v) => { const [y, m] = dtCivil(dtDateArg(v, "date.start_of_month", 1)); return dtText(dtDays(y, m, 1)); },
  end_of_month: (v) => { const [y, m] = dtCivil(dtDateArg(v, "date.end_of_month", 1)); return dtText(dtDays(y, m, dtMonthLen(y, m))); },
  to_zone: (v, tz) => {
    const t = dtStampArg(v, "date.to_zone", 1), z = dtZone(tz, "date.to_zone", 2), i = dtInstant(t);
    return dtStampText(i, dtKeptFrac(t.frac), dtOffsetAt(z, i)) ?? dtOutOfRange("date.to_zone");
  },
  compare: (a, b) => {
    const key = (v, n) => { const [z, t] = dtEitherArg(v, "date.compare", n); return t ? [dtInstant(t), Number(((t.frac || "").replace(/^\./, "") + "000000000").slice(0, 9))] : [z * 86400, 0]; };
    const x = key(a, 1), y = key(b, 2);
    return x[0] !== y[0] ? (x[0] < y[0] ? -1 : 1) : x[1] !== y[1] ? (x[1] < y[1] ? -1 : 1) : 0;
  },
  date_in: (v, tz) => {
    const t = dtStampArg(v, "date.date_in", 1), z = dtZone(tz, "date.date_in", 2), i = dtInstant(t);
    return dtDay(Math.floor((i + dtOffsetAt(z, i)) / 86400), "date.date_in");
  },
  at: (v, time, tz, opts) => {
    const f = "date.at", both = dtLocalDateTime(v);
    if (both !== null) return dtAtLocal(dtZone(time, f, 2), both[0] * 86400 + both[1], tz, f, 3);
    const day = dtDateArg(v, f, 1);
    let sec = 0;
    if (!isNil(time)) {
      sec = typeof time === "string" && time.length === 5 ? dtHms(time + ":00") : typeof time === "string" && time.length === 8 ? dtHms(time) : null;
      if (sec === null) fail(`\`${f}\` expects a time (HH:MM or HH:MM:SS) as argument 2, got ${dtGot(time)}`);
    }
    return dtAtLocal(dtZone(tz, f, 3), day * 86400 + sec, opts, f, 4);
  },
  ms: (v) => {
    const [z, t] = dtEitherArg(v, "date.ms", 1);
    if (!t) return z * 86400000;
    const frac = (t.frac || "").replace(/^\./, "");
    return dtInstant(t) * 1000 + Number((frac + "000").slice(0, 3));
  },
  from_ms: (n, tz) => {
    dtInt(n, "date.from_ms", 1);
    const z = dtZone(tz, "date.from_ms", 2), instant = Math.floor(n / 1000);
    return dtStampText(instant, "", dtOffsetAt(z, instant)) ?? dtOutOfRange("date.from_ms");
  },
  format: (v, how) => {
    const f = "date.format", [z, t] = dtEitherArg(v, f, 1);
    let pattern, n = dtEn;
    if (typeof how === "string") pattern = how;
    else if (isMap(how)) {
      const has = (k) => hasOwn(how, k), loc = has("locale") ? how.locale : null;
      if (!isNil(loc)) { if (typeof loc !== "string") fail(`\`${f}\`: locale is a str, like "de-DE", got ${dtGot(loc)}`); n = dtNames(loc); }
      // The first one set, as on the server (FEEDBACK55.md, Q-875; branch feedback55).
      const k = mkeys(how).find((k) => k !== "locale" && k !== "style" && k !== "pattern");
      if (k !== undefined) fail(`\`${f}\`: the options are locale, style and pattern, not \`${k}\``);
      const P = has("pattern") ? how.pattern : undefined, St = has("style") ? how.style : undefined;
      if (typeof P === "string") pattern = P;
      else if (isNil(P) && isNil(St)) pattern = n.styles[1];
      else if (isNil(P) && typeof St === "string") {
        const k = ["short", "medium", "long", "full"].indexOf(St);
        if (k < 0) fail(`\`${f}\`: style is short, medium, long or full, got ${dtGot(St)}`);
        pattern = n.styles[k];
      } else fail(`\`${f}\`: pattern and style are strs, got ${dtGot(P !== undefined ? P : St)}`);
    } else fail(`\`${f}\` expects a pattern (like "DD/MM/YYYY") or options as argument 2, got ${dtGot(how)}`);
    const toks = dtTokenize(Array.from(pattern));
    if (!t) { const x = toks.find((x) => dtTimeTokens.includes(x)); if (x) fail(`\`${f}\`: the pattern has a time (${x}), but ${dtGot(v)} is a date`); }
    return dtRender(toks, n, z, t);
  },
};
// Builtins, as the rest are: named, and called by the page as they are.
for (const k of Object.keys(B.date)) B.date[k] = meta(B.date[k], "date." + k, null);

// ---- channels -------------------------------------------------------------
// `channel.open(name, f)` in a component: `f` hears what the server sends on
// the channel for as long as the code that opened it stands, and `.send(v)`
// gives the channel's handler on the server a message. A page's channels
// share one connection: WebTransport where the browser has it (small
// messages go as datagrams, which may be lost or come out of order; larger
// and reliable ones on a stream), else a WebSocket. It is opened again when
// it drops, and the channels joined again.

const Chan = { subs: new Map(), media: new Map(), joined: new Set(), refused: new Set(), waiting: new Map(), conn: null, running: false, wt: true };

B.channel = {
  open: meta((name, f, opts) => {
    S(name, "channel.open");
    F(f, "channel.open", 2);
    let subs = Chan.subs.get(name);
    if (!subs) {
      subs = new Set();
      Chan.subs.set(name, subs);
      if (Chan.conn) Chan.conn.put(JSON.stringify(["j", name]), true);
    }
    const sub = { f };
    subs.add(sub);
    chanStart();
    if (Owner) {
      onCleanup(() => {
        subs.delete(sub);
        if (subs.size || Chan.subs.get(name) !== subs) return;
        Chan.subs.delete(name);
        Chan.joined.delete(name);
        Chan.waiting.delete(name);
        if (Chan.conn) Chan.conn.put(JSON.stringify(["l", name]), true);
      });
    }
    return { send: meta((v, o) => chanSend(name, v, o), "send", null) };
  }, "channel.open", null),
};

// Sent once the server has let the page in; until then (and while the
// connection is down) kept, up to a point.
function chanSend(name, v, o) {
  const text = jsonText(["m", name, v]);
  if (TOOLS) toolsSay("channel", { dir: "out", channel: name, value: toolsWire(v) });
  const reliable = isMap(o) && truthy(o.reliable);
  if (Chan.conn && Chan.joined.has(name)) {
    Chan.conn.put(text, reliable);
    return null;
  }
  const w = Chan.waiting.get(name) || [];
  if (w.length < 256) w.push([text, reliable]);
  Chan.waiting.set(name, w);
  return null;
}

function chanStart() {
  if (Chan.running) return;
  Chan.running = true;
  (async () => {
    let wait = 500;
    while (Chan.subs.size || Chan.media.size) {
      try {
        const c = await chanConnect();
        Chan.conn = c;
        QW.via = c.via;
        wait = 500;
        for (const name of Chan.subs.keys()) c.put(JSON.stringify(["j", name]), true);
        for (const name of Chan.media.keys()) c.put(JSON.stringify(["v", name]), true);
        await c.closed;
      } catch (e) {}
      Chan.conn = null;
      Chan.joined.clear();
      if (!Chan.subs.size && !Chan.media.size) break;
      await new Promise((r) => setTimeout(r, wait));
      wait = Math.min(wait * 2, 15000);
    }
    Chan.running = false;
  })();
}

async function chanConnect() {
  const r = await wireFetch(BASE + "/_quillo/wt-ticket", { method: "POST" });
  if (!r.ok) throw new Error("no ticket");
  const info = await r.json();
  const q = "?t=" + encodeURIComponent(info.ticket);
  if (Chan.wt && info.port && typeof WebTransport === "function") {
    try {
      return await chanTransport(info, q);
    } catch (e) {
      // Blocked, refused or too slow here: a WebSocket from now on.
      Chan.wt = false;
    }
  }
  return chanSocket(q);
}

async function chanTransport(info, q) {
  const host = location.hostname.includes(":") ? "[" + location.hostname + "]" : location.hostname;
  const opts = info.hash ? { serverCertificateHashes: [{ algorithm: "sha-256", value: new Uint8Array(info.hash.match(/../g).map((x) => parseInt(x, 16))) }] } : {};
  const t = new WebTransport("https://" + host + ":" + info.port + "/_quillo/wt" + q, opts);
  let timer;
  try {
    await Promise.race([t.ready, new Promise((_, no) => { timer = setTimeout(() => no(new Error("timeout")), 1500); })]);
  } catch (e) {
    try { t.close(); } catch (e2) {}
    throw e;
  } finally {
    clearTimeout(timer);
  }
  const bs = await t.createBidirectionalStream();
  const out = bs.writable.getWriter(), dg = t.datagrams.writable.getWriter();
  const enc = new TextEncoder(), dec = new TextDecoder();
  const c = {
    via: "webtransport",
    put(text, reliable) {
      const b = enc.encode(text);
      if (!reliable && b.length <= (t.datagrams.maxDatagramSize || 1000)) {
        dg.write(b).catch(() => {});
        return;
      }
      const f = new Uint8Array(4 + b.length);
      new DataView(f.buffer).setUint32(0, b.length);
      f.set(b, 4);
      out.write(f).catch(() => {});
    },
    closed: t.closed.then(() => {}, () => {}),
    // A frame this page sends (`<quillo-media publish>`): a stream of its
    // own per track, each frame after its length.
    putFrame(b, track) {
      if (!this.tracks) this.tracks = new Map();
      let w = this.tracks.get(track);
      if (!w) {
        w = t.createUnidirectionalStream().then((s) => s.getWriter());
        this.tracks.set(track, w);
      }
      const f = new Uint8Array(4 + b.length);
      new DataView(f.buffer).setUint32(0, b.length);
      f.set(b, 4);
      w.then((wr) => wr.write(f)).catch(() => {});
    },
  };
  (async () => {
    const rd = t.datagrams.readable.getReader();
    for (;;) {
      const { value, done } = await rd.read();
      if (done) return;
      chanGot(dec.decode(value));
    }
  })().catch(() => {});
  (async () => {
    const rd = bs.readable.getReader();
    let buf = new Uint8Array(0);
    for (;;) {
      const { value, done } = await rd.read();
      if (done) return;
      const all = new Uint8Array(buf.length + value.length);
      all.set(buf);
      all.set(value, buf.length);
      buf = all;
      while (buf.length >= 4) {
        const n = new DataView(buf.buffer, buf.byteOffset).getUint32(0);
        if (buf.length < 4 + n) break;
        chanGot(dec.decode(buf.subarray(4, 4 + n)));
        buf = buf.slice(4 + n);
      }
    }
  })().catch(() => {});
  // Frames of the streams watched: a stream per track, opened by the server.
  (async () => {
    const rd = t.incomingUnidirectionalStreams.getReader();
    for (;;) {
      const { value, done } = await rd.read();
      if (done) return;
      chanFrames(value);
    }
  })().catch(() => {});
  // The server waits for this stream, which opens with what is first sent.
  c.put(JSON.stringify(["h"]), true);
  return c;
}

// Each frame after its length.
async function chanFrames(stream) {
  const rd = stream.getReader();
  let buf = new Uint8Array(0);
  for (;;) {
    const { value, done } = await rd.read();
    if (done) return;
    const all = new Uint8Array(buf.length + value.length);
    all.set(buf);
    all.set(value, buf.length);
    buf = all;
    let at = 0;
    while (buf.length - at >= 4) {
      const n = new DataView(buf.buffer, buf.byteOffset + at).getUint32(0);
      if (buf.length - at < 4 + n) break;
      chanFrame(buf.subarray(at + 4, at + 4 + n));
      at += 4 + n;
    }
    buf = buf.slice(at);
  }
}

// `[name length][name][kind][key][timestamp]`, then the frame.
function chanFrame(b) {
  const n = b[0], name = new TextDecoder().decode(b.subarray(1, 1 + n));
  const fs = Chan.media.get(name);
  if (!fs) return;
  const ts = new DataView(b.buffer, b.byteOffset + 3 + n).getUint32(0);
  for (const f of fs) f(b[1 + n], b[2 + n] === 1, ts, b.subarray(7 + n));
}

// Frames of stream `name` to `f(kind, key, ts, data)`, until what it gives
// back is called.
function chanWatch(name, f) {
  let fs = Chan.media.get(name);
  if (!fs) {
    fs = new Set();
    Chan.media.set(name, fs);
    if (Chan.conn) Chan.conn.put(JSON.stringify(["v", name]), true);
  }
  fs.add(f);
  chanStart();
  return () => {
    fs.delete(f);
    if (fs.size || Chan.media.get(name) !== fs) return;
    Chan.media.delete(name);
    if (Chan.conn) Chan.conn.put(JSON.stringify(["u", name]), true);
  };
}

function chanSocket(q) {
  return new Promise((ok, no) => {
    const s = openSocket(BASE + "/_quillo/wt" + q);
    s.binaryType = "arraybuffer";
    let ended;
    const closed = new Promise((r) => (ended = r));
    s.onopen = () => ok({ via: "websocket", put: (text) => s.send(text), putFrame: (b) => s.send(b), closed });
    s.onmessage = (m) => { if (typeof m.data === "string") chanGot(m.data); else chanFrame(new Uint8Array(m.data)); };
    s.onclose = () => { ended(); no(new Error("closed")); };
  });
}

// `["j", name, yes]`: the answer to joining; `["m", name, value]`: a message.
function chanGot(text) {
  let m;
  try { m = readJSON(text); } catch (e) { return; }
  const [k, name, v] = m, subs = Chan.subs.get(name);
  if (TOOLS) toolsSay("channel", k === "j" ? { dir: "in", channel: name, joined: !!v } : { dir: "in", channel: name, value: v });
  if (!subs) return;
  if (k === "j") {
    if (!v) {
      // Refused, or let go (`media.close` of a stream this page sends).
      Chan.joined.delete(name);
      Chan.refused.add(name);
      if (!name.startsWith("media:")) console.warn("quillo: the server did not let this page join channel " + name);
      return;
    }
    Chan.refused.delete(name);
    Chan.joined.add(name);
    const w = Chan.waiting.get(name) || [];
    Chan.waiting.delete(name);
    for (const [t, reliable] of w) Chan.conn && Chan.conn.put(t, reliable);
    return;
  }
  if (k !== "m") return;
  const val = fromJSON(v);
  for (const s of subs) {
    try {
      const r = s.f(val);
      if (r instanceof Promise) r.catch(report);
    } catch (e) {
      report(e);
    }
  }
}

// ---- <quillo-media> ---------------------------------------------------------
// `<quillo-media stream="cam">` plays a stream the server takes as RTP (see
// src/media.rs): H.264 video drawn on a canvas, Opus audio, both decoded
// with WebCodecs. It is muted until clicked unless it has no `muted`
// attribute and the page may already play sound; with `controls`, a
// button turns sound on and off. `data-state` says how it is going:
// connecting, waiting (for a keyframe, or for frames), playing, unsupported.

function mediaDefine() {
  if (typeof customElements === "undefined" || customElements.get("quillo-media")) return;
  customElements.define("quillo-media", class extends HTMLElement {
    connectedCallback() {
      if (!this.player) this.player = this.hasAttribute("publish") ? mediaPublisher(this) : mediaPlayer(this);
    }
    // What `publish` sends, instead of the camera and microphone: a
    // MediaStream the page has (a canvas's, a screen's).
    get source() { return this._source || null; }
    set source(s) {
      this._source = s;
      if (this.player && this.player.use) this.player.use(s);
    }
    disconnectedCallback() {
      if (this.player) this.player.stop();
      this.player = null;
    }
  });
}

// ---- <quillo-media publish> --------------------------------------------------------
// `<quillo-media publish="call:42">` sends this page's camera and microphone
// (or the MediaStream given as its `source`) to stream call:42, which every
// `<quillo-media stream="call:42">` plays: encoded in the browser with
// WebCodecs (H.264 baseline and Opus, what the relay passes on), sent on the
// page's channel connection. The program says who may send it (`media.on`):
// the page joins the channel media:<name> first (FEEDBACK6.md, Q-060; branch
// feedback6-fixes). `data-state`: starting, sending, refused (the camera, or
// the program, said no), ended, unsupported.

function mediaPublisher(el) {
  const name = el.getAttribute("publish") || "";
  const channel = "media:" + name;
  const st = { video: 0, audio: 0, sent: 0, bytes: 0 };
  el.mediaStats = st;
  if (!el.style.display) el.style.display = "inline-block";
  if (typeof VideoEncoder === "undefined") {
    el.dataset.state = "unsupported";
    el.textContent = "This browser cannot send video.";
    return { stop() {} };
  }
  el.dataset.state = "starting";
  const preview = document.createElement("video");
  preview.muted = true;
  preview.autoplay = true;
  preview.playsInline = true;
  preview.style.cssText = "display:block;width:100%;height:auto;background:#000";
  el.replaceChildren(preview);
  const label = new TextEncoder().encode(name);
  const wire = (kind, key, ts, data) => {
    const b = new Uint8Array(7 + label.length + data.byteLength);
    b[0] = label.length;
    b.set(label, 1);
    b[1 + label.length] = kind;
    b[2 + label.length] = key ? 1 : 0;
    new DataView(b.buffer).setUint32(3 + label.length, ts >>> 0);
    b.set(new Uint8Array(data), 7 + label.length);
    return b;
  };
  const send = (kind, key, ts, chunk) => {
    const data = new ArrayBuffer(chunk.byteLength);
    chunk.copyTo(data);
    if (!Chan.conn || !Chan.conn.putFrame || !Chan.joined.has(channel)) return;
    Chan.conn.putFrame(wire(kind, key, ts, data), kind);
    st.sent++;
    st.bytes += data.byteLength;
  };
  // Joined as a channel is, so the server knows this page may send.
  let subs = Chan.subs.get(channel);
  if (!subs) {
    subs = new Set();
    Chan.subs.set(channel, subs);
    Chan.refused.delete(channel);
    if (Chan.conn) Chan.conn.put(JSON.stringify(["j", channel]), true);
  }
  const sub = { f: () => {} };
  subs.add(sub);
  chanStart();
  let stopped = false, own = null, venc = null, aenc = null, timer = null, readers = [];
  const watching = setInterval(() => {
    if (Chan.refused.has(channel)) el.dataset.state = st.sent ? "ended" : "refused";
  }, 500);
  const halt = () => {
    clearInterval(timer);
    timer = null;
    for (const r of readers) r.cancel().catch(() => {});
    readers = [];
    for (const e of [venc, aenc]) if (e && e.state !== "closed") e.close();
    venc = aenc = null;
    if (own) for (const t of own.getTracks()) t.stop();
    own = null;
  };
  const video = (track) => {
    const set = track.getSettings();
    const w = (set.width || 640) & ~1, h = (set.height || 480) & ~1;
    venc = new VideoEncoder({
      output: (chunk) => { st.video++; send(1, chunk.type === "key", Math.round(chunk.timestamp * 0.09), chunk); },
      error: () => { venc = null; },
    });
    venc.configure({ codec: "avc1.42E01F", width: w, height: h, bitrate: 1500000, framerate: 30, latencyMode: "realtime", avc: { format: "annexb" } });
    let n = 0;
    const encode = (frame) => {
      // A keyframe every 2 seconds, so a page that starts watching shows a picture soon.
      if (venc && venc.state === "configured" && venc.encodeQueueSize < 3) venc.encode(frame, { keyFrame: n++ % 60 === 0 });
      frame.close();
    };
    if (typeof MediaStreamTrackProcessor === "function") {
      const rd = new MediaStreamTrackProcessor({ track }).readable.getReader();
      readers.push(rd);
      (async () => {
        for (;;) {
          const { value, done } = await rd.read();
          if (done || stopped) { if (value) value.close(); return; }
          encode(value);
        }
      })().catch(() => {});
    } else {
      // Without track processors: the preview drawn on a canvas, 30 times a second.
      const c = document.createElement("canvas");
      c.width = w;
      c.height = h;
      const g = c.getContext("2d"), t0 = performance.now();
      timer = setInterval(() => {
        if (preview.readyState < 2) return;
        g.drawImage(preview, 0, 0, w, h);
        encode(new VideoFrame(c, { timestamp: Math.round((performance.now() - t0) * 1000) }));
      }, 1000 / 30);
    }
  };
  const audio = (track) => {
    if (typeof AudioEncoder === "undefined" || typeof MediaStreamTrackProcessor !== "function") return;
    const rd = new MediaStreamTrackProcessor({ track }).readable.getReader();
    readers.push(rd);
    (async () => {
      for (;;) {
        const { value, done } = await rd.read();
        if (done || stopped) { if (value) value.close(); return; }
        if (!aenc) {
          aenc = new AudioEncoder({
            output: (chunk) => { st.audio++; send(2, true, Math.round(chunk.timestamp * 0.048), chunk); },
            error: () => { aenc = null; },
          });
          aenc.configure({ codec: "opus", sampleRate: value.sampleRate, numberOfChannels: Math.min(value.numberOfChannels, 2), bitrate: 64000 });
        }
        if (aenc.state === "configured") aenc.encode(value);
        value.close();
      }
    })().catch(() => {});
  };
  const use = (src) => {
    halt();
    if (stopped || !src) return;
    preview.srcObject = src;
    const v = src.getVideoTracks()[0], a = src.getAudioTracks()[0];
    if (v) video(v);
    if (a) audio(a);
    el.dataset.state = "sending";
  };
  // The camera, unless the page gives a source first (in @mount, say).
  setTimeout(async () => {
    if (stopped || el._source) { if (el._source) use(el._source); return; }
    try {
      const want = { video: !el.hasAttribute("no-video") && { width: 1280, height: 720 }, audio: !el.hasAttribute("no-audio") };
      const src = await navigator.mediaDevices.getUserMedia(want);
      if (stopped || el._source) { for (const t of src.getTracks()) t.stop(); return; }
      use(src);
      own = src;
    } catch (e) {
      // No camera here, or not allowed; a source the page gives still goes.
      if (!el._source) el.dataset.state = "refused";
    }
  }, 0);
  return {
    use,
    stop() {
      stopped = true;
      halt();
      clearInterval(watching);
      subs.delete(sub);
      if (subs.size || Chan.subs.get(channel) !== subs) return;
      Chan.subs.delete(channel);
      Chan.joined.delete(channel);
      if (Chan.conn) Chan.conn.put(JSON.stringify(["l", channel]), true);
    },
  };
}

function mediaPlayer(el) {
  const name = el.getAttribute("stream") || "";
  const st = { video: 0, audio: 0, keyframes: 0, codec: "", vd: null, ad: null, actx: null, at: 0, last: 0, base: null, abase: null };
  el.mediaStats = st;
  if (!el.style.display) el.style.display = "inline-block";
  if (!el.style.position) el.style.position = "relative";
  if (typeof VideoDecoder === "undefined") {
    el.dataset.state = "unsupported";
    el.textContent = "This browser cannot play this stream.";
    return { stop() {} };
  }
  el.dataset.state = "connecting";
  const canvas = document.createElement("canvas");
  canvas.style.cssText = "display:block;width:100%;height:auto;background:#000";
  el.replaceChildren(canvas);
  const g = canvas.getContext("2d");
  let muted = el.hasAttribute("muted");
  let button = null;
  if (el.hasAttribute("controls")) {
    button = document.createElement("button");
    button.type = "button";
    button.style.cssText = "position:absolute;right:.5rem;bottom:.5rem;font:inherit;padding:.25rem .6rem;border:0;border-radius:6px;background:rgba(0,0,0,.6);color:#fff;cursor:pointer";
    const label = () => { button.textContent = muted ? "\u{1F507} Sound off" : "\u{1F50A} Sound on"; };
    label();
    button.onclick = (e) => {
      e.stopPropagation();
      muted = !muted;
      label();
      if (!muted && st.actx) st.actx.resume();
    };
    el.appendChild(button);
  }
  el.addEventListener("click", () => { if (st.actx) st.actx.resume(); });
  // Microseconds from the stream's start, across 32-bit wraps.
  const micros = (ts, rate, which) => {
    if (st[which] === null) st[which] = ts;
    return Math.round(((ts - st[which]) >>> 0) * 1e6 / rate);
  };
  const newVideo = () => {
    const vd = new VideoDecoder({
      output: (f) => {
        // Catching up (a page that starts watching gets the frames since
        // the last keyframe at once): only the newest is worth drawing.
        if (st.vd && st.vd.decodeQueueSize === 0) {
          if (canvas.width !== f.displayWidth || canvas.height !== f.displayHeight) {
            canvas.width = f.displayWidth;
            canvas.height = f.displayHeight;
          }
          g.drawImage(f, 0, 0);
        }
        f.close();
        st.video++;
        st.last = Date.now();
        el.dataset.state = "playing";
      },
      error: () => { st.vd = null; },
    });
    return vd;
  };
  const video = (key, ts, data) => {
    if (!st.vd || st.vd.state === "closed") {
      if (!key) { el.dataset.state = "waiting"; return; }
      // The profile and level, from the sequence parameter set.
      let codec = "avc1.42E01F";
      for (let i = 0; i + 7 < data.length; i++) {
        if (data[i] === 0 && data[i + 1] === 0 && data[i + 2] === 1 && (data[i + 3] & 0x1f) === 7) {
          codec = "avc1." + [data[i + 4], data[i + 5], data[i + 6]].map((x) => x.toString(16).padStart(2, "0")).join("");
          break;
        }
      }
      st.codec = codec;
      st.vd = newVideo();
      st.vd.configure({ codec, optimizeForLatency: true });
    }
    // Too far behind (more than the server keeps for a page that starts
    // watching): start again from the next keyframe.
    if (st.vd.decodeQueueSize > 600) {
      st.vd.close();
      st.vd = null;
      return;
    }
    if (key) st.keyframes++;
    st.vd.decode(new EncodedVideoChunk({ type: key ? "key" : "delta", timestamp: micros(ts, 90000, "base"), data }));
  };
  const audio = (ts, data) => {
    if (muted || typeof AudioDecoder === "undefined") return;
    if (!st.ad || st.ad.state === "closed") {
      st.actx = st.actx || new AudioContext({ sampleRate: 48000 });
      st.ad = new AudioDecoder({
        output: (d) => {
          const actx = st.actx;
          const buf = actx.createBuffer(d.numberOfChannels, d.numberOfFrames, d.sampleRate);
          for (let c = 0; c < d.numberOfChannels; c++) d.copyTo(buf.getChannelData(c), { planeIndex: c, format: "f32-planar" });
          d.close();
          st.audio++;
          if (actx.state !== "running") return;
          const src = actx.createBufferSource();
          src.buffer = buf;
          src.connect(actx.destination);
          // A little ahead, so gaps between packets are not heard.
          const now = actx.currentTime;
          if (st.at < now + 0.02 || st.at > now + 0.5) st.at = now + 0.12;
          src.start(st.at);
          st.at += buf.duration;
        },
        error: () => { st.ad = null; },
      });
      st.ad.configure({ codec: "opus", sampleRate: 48000, numberOfChannels: 2 });
    }
    st.ad.decode(new EncodedAudioChunk({ type: "key", timestamp: micros(ts, 48000, "abase"), data }));
  };
  const stop = chanWatch(name, (kind, key, ts, data) => {
    try {
      // `media.close`: the stream has ended (FEEDBACK6.md, Q-061; branch feedback6-fixes).
      if (kind === 0) {
        el.dataset.state = "ended";
        if (st.vd && st.vd.state !== "closed") st.vd.close();
        st.vd = null;
        st.last = 0;
        return;
      }
      if (kind === 1) video(key, ts, data);
      else if (kind === 2) audio(ts, data);
    } catch (e) {
      st.vd = null;
    }
  });
  const idle = setInterval(() => {
    if (st.last && Date.now() - st.last > 3000) el.dataset.state = "waiting";
  }, 1000);
  return {
    stop() {
      stop();
      clearInterval(idle);
      for (const d of [st.vd, st.ad]) if (d && d.state !== "closed") d.close();
      if (st.actx) st.actx.close();
    },
  };
}

// ---- server fns --------------------------------------------------------

// A server fn that uses `idempotent` gets a key with each call (the same for
// the call's tries), and the call is sent again when its answer does not
// come back: the connection failed, the server was busy (409: the first try
// is still running) or a proxy gave up on it (502-504). The server runs it
// once, and gives the first answer to the tries after it.
const RETRIED = [409, 502, 503, 504];

// The key belongs to what is sent, not to the click (FEEDBACK24.md, Q-196; branch
// feedback24-fixes). While a call runs, the same call shares its answer and sends
// nothing, so two clicks are one payment. One that got no answer keeps its key, so
// that sending it again is safe; any answer ends it, and the next is a new call.
const Sent = new Map();

function rpcOnce(name, args) {
  if (args.some((a) => typeof Blob !== "undefined" && a instanceof Blob)) return rpcRaw(name, args, false, idemKey());
  const id = name + "\0" + writeJSON(args.map((a) => toJSON(a, true)));
  let call = Sent.get(id);
  if (call && call.running) return call.running;
  if (!call) {
    if (Sent.size >= 64) Sent.delete(Sent.keys().next().value);
    Sent.set(id, (call = { key: idemKey() }));
  }
  call.running = rpcRaw(name, args, false, call.key).then((r) => {
    call.running = null;
    if (!r.unanswered) Sent.delete(id);
    return r;
  });
  return call.running;
}

function idemKey() {
  if (typeof crypto !== "undefined" && crypto.randomUUID) return crypto.randomUUID();
  const b = new Uint8Array(16);
  crypto.getRandomValues(b);
  return Array.from(b, (x) => x.toString(16).padStart(2, "0")).join("");
}

// `live` is the subscription refreshing, if one is: it says what it has
// (`h`, the hash of `base`, its last value from the server) and the answer
// may be a patch to that (FEEDBACK2.md, Q-028; branch feedback6-fixes).
async function rpcRaw(name, args, live, idem) {
  if (!TOOLS) return rpcSend(name, args, live, idem);
  const t0 = performance.now(), r = await rpcSend(name, args, live, idem);
  toolsSay("call", { name, args: args.map(toolsWire), ms: Math.round(performance.now() - t0), ok: !r.failed && !(r.value instanceof Err), failed: !!r.failed, value: toolsWire(r.value), live: !!live, idempotent: !!idem });
  return r;
}

async function rpcSend(name, args, live, idem) {
  let r;
  // It may change what was kept for a link (see `Ahead`).
  if (!live) aheadDrop();
  const headers = { "content-type": "application/json" };
  const had = live && live.h, base = live && live.base;
  if (live) headers["x-quillo-live"] = "1";
  if (had) headers["x-quillo-had"] = had;
  if (idem) headers["idempotency-key"] = idem;
  // Files (from an <input type=file>) go as parts of a multipart body, the
  // other arguments as JSON beside them.
  // A file anywhere in them goes as a part, in a list or a map too, and a FileList is a list of
  // its files (FEEDBACK30.md, Q-364; branch feedback27-35-fixes).
  let payload;
  const files = [];
  const pack = (v) => {
    if (typeof Blob !== "undefined" && v instanceof Blob) { files.push(v); return { $quillo_file: files.length - 1 }; }
    if (typeof FileList !== "undefined" && v instanceof FileList) return Array.from(v, pack);
    if (Array.isArray(v)) return v.map(pack);
    // In the order set, and a map of one key that starts with `$` as `toJSON` writes it
    // (FEEDBACK55.md, Q-875 and its review; branch feedback55).
    if (isMap(v)) {
      const ord = ordOf(v), ks = ord === undefined ? Object.keys(v) : mkeys(v);
      if (ks.length === 1 && ks[0].charCodeAt(0) === 36) return { $: [ks[0], pack(v[ks[0]])] };
      if (ord !== undefined) return new Kept(ks, ks.map((k) => pack(v[k])));
      const o = {};
      for (const k of ks) putKey(o, k, pack(v[k]));
      return o;
    }
    return toJSON(v, true);
  };
  const packed = args.map(pack);
  if (files.length) {
    payload = new FormData();
    payload.append("args", writeJSON(packed));
    files.forEach((f, i) => payload.append("file" + i, f, f.name || "file"));
    delete headers["content-type"];
    headers["x-quillo-rpc"] = "1";
  } else {
    payload = writeJSON(packed);
  }
  // A call that fails (no connection, a server error) is an error value,
  // as with http.* on the server, so `??` and `is_err` handle it.
  const failed = (msg) => ({ value: new Err(msg), topics: [], seq: 0, server: "", failed: true });
  for (let tries = idem ? 4 : 1; ; ) {
    try {
      r = await wireFetch(BASE + "/_quillo/rpc/" + encodeURIComponent(name), {
        method: "POST",
        headers,
        body: payload,
      });
    } catch (e) {
      r = null;
      if (--tries <= 0) return { ...failed(`server fn \`${name}\` could not be reached: ${e.message}`), unanswered: true };
    }
    if (r && !(idem && RETRIED.includes(r.status) && --tries > 0)) break;
    // 0.25 s, 0.5 s, 1 s, give or take a quarter.
    await new Promise((ok) => setTimeout(ok, 250 * 2 ** (3 - tries) * (0.75 + Math.random() / 2)));
  }
  let body = null, text = "", cut = false;
  try { text = await r.text(); } catch (_) { cut = true; }
  try { body = readJSON(text); if (!body || !hasOwn(body, "patch")) body = untagDeep(body); } catch (_) {}
  // In development the server says why, and the console says it too, with its place, as an
  // error in the page does (FEEDBACK64.md, Q-933; branch feedback64).
  if (DEV && r.status === 500 && body && body.error) report(`server fn \`${name}\` failed: ${body.error}`); // dev
  if (!r.ok) return { ...failed(`server fn \`${name}\` failed (${r.status})${body && body.error ? ": " + body.error : ""}`), unanswered: RETRIED.includes(r.status) };
  // A 2xx that is no answer, a sign-in page a redirect led to, nothing, a body cut short, gave
  // `null` as if the call had worked. The server marks what it sends, since `respond` and `reply`
  // hold what they like; a proxy may drop the mark, so an answer's own shape counts too. It may
  // have run: its key is kept, so that trying again cannot run it twice (FEEDBACK49.md, Q-598;
  // branch feedback61).
  const shaped = body && ["ok", "err", "patch"].some((k) => hasOwn(body, k));
  if (cut || (!shaped && !(r.headers && r.headers.get("x-quillo-answer")))) {
    return { ...failed(`server fn \`${name}\` answered something that is not its answer (${(r.headers && r.headers.get("content-type")) || "no type"}, ${text.length} characters)`), unanswered: true };
  }
  let value = null;
  if (body && hasOwn(body, "patch")) {
    if (!had) return failed(`server fn \`${name}\`: a patch to nothing`);
    value = livePatched(base, body.patch);
  } else value = body && hasOwn(body, "err") ? new Err(body.err, true) : body && hasOwn(body, "ok") ? body.ok : null;
  return { value, topics: (body && body.topics) || [], seq: (body && body.seq) || 0, server: (body && body.server) || "", h: (body && body.h) || "" };
}

// A live value made from the last one and what changed (the format is in
// src/livecache.rs). What did not change is the same object as before.
function livePatched(old, p) {
  if (hasOwn(p, "$=")) return old;
  if (hasOwn(p, "$v")) return untagDeep(p.$v);
  const put = (o, k, v) => Object.defineProperty(o, k, { value: v, enumerable: true, writable: true, configurable: true });
  // In the order set, which the server sends a patch for only when it is the old keys' and then
  // the new ones' (FEEDBACK55.md, Q-875; branch feedback55).
  if (hasOwn(p, "$m")) {
    const o = {}, gone = new Set(p.$x || []), ks = [];
    let idx = false;
    for (const k of mkeys(old)) if (!gone.has(k)) { put(o, k, hasOwn(p.$m, k) ? livePatched(old[k], p.$m[k]) : old[k]); ks.push(k); }
    for (const k of mkeys(p.$m)) if (!hasOwn(old, k)) { put(o, k, livePatched(undefined, p.$m[k])); ks.push(k); idx = idx || isIdx(k); }
    return ordOf(old) === undefined && !idx ? o : ordered(o, ks);
  }
  const out = [];
  for (const op of p.$l) {
    if (typeof op === "number") out.push(old[op]);
    else if (Array.isArray(op)) for (let i = 0; i < op[1]; i++) out.push(old[op[0] + i]);
    else if (hasOwn(op, "$p")) out.push(livePatched(old[op.$p], op.d));
    else out.push(untagDeep(op.$v));
  }
  return out;
}

async function rpc(name, args, idem) {
  return (await (idem ? rpcOnce(name, args) : rpcRaw(name, args, false))).value;
}

// ---- live updates --------------------------------------------------------
// Each `state x = live(load, ...)` is a subscription to the topics `load` read
// (tables and globals). One WebSocket to /_quillo/live carries changes to any
// of them; then the affected loaders run again. When the topics change, it
// is opened again for the new ones. (A WebSocket, because browsers allow a
// host only six HTTP requests at once across all tabs; waiting requests
// would leave none for server calls. Where one cannot open, the same URL
// answers a plain request after the first change.)
//
// Sequence numbers belong to the server that gave them: with several
// servers, a page's calls may each go to a different one. The socket's
// server decides; asked with another server's number, it answers that all
// the topics changed, and the page loads them again once.

const Live = { seq: 0, server: "", subs: new Set(), ctrl: null, running: false, key: "" };

// Also takes data-v="<seq>:<server>" from a server-rendered island, split.
function liveSeen(seq, server = "") {
  seq = +seq;
  if (!(seq > 0)) return;
  if (!Live.server) Live.server = server;
  if (server !== Live.server) return;
  Live.seq = Live.seq > 0 ? Math.min(Live.seq, seq) : seq;
}

function liveTopics() {
  const all = new Set();
  for (const s of Live.subs) {
    if (s.inst.dead) { Live.subs.delete(s); continue; }
    for (const t of s.topics) all.add(t);
  }
  return [...all].sort();
}

// The topics as the server reads them: each one's own `%` and `,` written out, so that a comma in
// one is no separator (FEEDBACK55.md, Q-801; branch feedback55). The 256 the server follows, and
// none longer than it can be told of (its `MAX_PAGE_TOPIC`), which keeps what is sent within what
// it reads.
const liveKey = (topics) => topics.filter((t) => t.length <= 448).slice(0, 256).map((t) => t.replace(/%/g, "%25").replace(/,/g, "%2C")).join(",");

// Starts waiting, or starts again when the set of topics changed.
function liveRestart() {
  queueMicrotask(() => {
    if (!Live.running) {
      Live.running = true;
      liveLoop();
    } else if (liveKey(liveTopics()) !== Live.key && Live.ctrl) {
      Live.ctrl.abort();
    }
  });
}

async function liveLoop() {
  // Tries in a row that came to nothing, and whether the page was told.
  let fails = 0;
  for (;;) {
    const topics = liveTopics();
    // Topics too long for an address go after it, in the socket's first message or a POST's
    // body, where the server refused the address and the page never heard of a change
    // (FEEDBACK55.md, Q-801; branch feedback55).
    Live.key = liveKey(topics);
    if (!topics.length) { Live.running = false; return; }
    const ctrl = (Live.ctrl = new AbortController());
    const t = encodeURIComponent(Live.key), later = t.length > LIVE_IN_URL;
    const url = BASE + "/_quillo/live?since=" + Live.seq + "&s=" + Live.server + (later ? "&tm=1" : "&t=" + t);
    // What a refusal asks for: a server that is full says when to try again.
    let after = 0;
    try {
      if (await liveSocket(url, ctrl.signal, later ? Live.key : null)) fails = 0;
      else if (!ctrl.signal.aborted) {
        const r = await wireFetch(url, later ? { method: "POST", body: Live.key, signal: ctrl.signal, cache: "no-store" } : { signal: ctrl.signal, cache: "no-store" });
        if (r.ok) { liveChanged(readJSON(await r.text())); fails = 0; continue; }
        after = (Number(r.headers.get("retry-after")) || 0) * 1000;
        fails++;
      }
    } catch (e) {
      fails++;
    }
    if (ctrl.signal.aborted) continue;
    // A page that stays behind has to be seen (FEEDBACK24.md, Q-189), once. A
    // refusal is plain at once; a failed try may be a moment's.
    if (fails === (after ? 1 : 3)) report("live updates are not reaching this page: the server is not taking them");
    // All at once after a restart is what a restart must not be: each page waits
    // a second or so, spread out, and longer each time it fails
    // (FEEDBACK24.md, Q-194; branch feedback24-fixes).
    const wait = Math.max(after, Math.min(1000 * 2 ** fails, 30000));
    await new Promise((r) => setTimeout(r, wait * (0.5 + Math.random())));
  }
}

// The longest a live request's topics are in its address (see `liveLoop`).
const LIVE_IN_URL = 6000;

// Resolves when the socket closes: true if it had opened. `first`: the topics, sent once it has.
function liveSocket(url, signal, first) {
  return new Promise((resolve) => {
    if (typeof WebSocket !== "function" && !(WIRE && WIRE.socket)) return resolve(false);
    const ws = openSocket(url);
    let opened = false;
    ws.onopen = () => { opened = true; if (first !== null) ws.send(first); };
    ws.onmessage = (m) => { if (!signal.aborted) liveChanged(readJSON(m.data)); };
    ws.onclose = () => resolve(opened);
    // Carry on at once: a closing socket finishes closing in its own time
    // (the browser waits for the server's answer), and says nothing more.
    signal.addEventListener("abort", () => {
      ws.close();
      resolve(true);
    });
  });
}

// A loader whose answer already has the change (its own write: it marked
// what it shows as read) is not asked again; one asked while its answer is on
// the way waits for it, and asks again only if the answer is older than the
// change (FEEDBACK13.md, Q-090; branch feedback13-fixes).
function liveChanged(j) {
  // A new server, after a restart: every page asks at once otherwise.
  const late = Live.server && j.server && Live.server !== j.server ? Math.random() * 1000 : 0;
  Live.seq = j.seq;
  Live.server = j.server || "";
  const changed = new Set(j.changed || []);
  if (!changed.size) return;
  if (TOOLS) toolsSay("live", { seq: j.seq, topics: [...changed], loads: [...Live.subs].filter((s) => s.topics.some((t) => changed.has(t))).map((s) => ({ id: TOOLS.ids.get(s.inst) || null, name: s.inst.def.$name })) });
  for (const s of [...Live.subs]) {
    if (!s.topics.some((t) => changed.has(t))) continue;
    if (s.seen >= j.seq && s.seenOn === Live.server) continue;
    if (s.loading) { s.wanted = Math.max(s.wanted || 0, j.seq); continue; }
    // `live({ every: "1s" }, ...)`: what comes sooner waits for the gap, and
    // whatever more comes meanwhile is the same refresh (FEEDBACK24.md, Q-190;
    // branch feedback24-fixes).
    const gap = s.every ? s.last + s.every - Date.now() : 0;
    if (late || gap > 0) {
      if (s.held) continue;
      s.held = setTimeout(() => {
        s.held = null;
        if (s.inst.dead) return;
        if (s.loading) s.wanted = Live.seq;
        else liveRefresh(s);
      }, Math.max(late, gap));
    } else liveRefresh(s);
  }
}

async function liveRefresh(sub, newArgs) {
  // Only the latest load counts: an older one may answer last.
  const n = (sub.loads = (sub.loads || 0) + 1);
  sub.loading = true;
  sub.last = Date.now();
  sub.wanted = 0;
  // This load is the one a failed load's next try waited for (see below).
  clearTimeout(sub.retry);
  if (newArgs) waitSig(sub.sig).set(true);
  try {
    const { value, topics, seq, server, failed, h } = await rpcRaw(sub.fname, sub.args, sub);
    if (n !== sub.loads) return;
    sub.loading = false;
    // A load that failed (no answer, or a status that is no success) keeps the last good data and
    // tries again, after a second, then two, doubling up to 30 s and spread out as `liveLoop`
    // waits, until one comes; `waiting()` is true meanwhile, so the page can say it is behind. It
    // gave up: the page showed old data until the next change, which a quiet table may never make,
    // and a change heard while it was on its way was forgotten (FEEDBACK61.md, Q-899; branch
    // feedback61).
    if (failed) {
      // Said at the first failure of a run of them.
      if (sub.fails++ === 0) report(value.error);
      waitSig(sub.sig).set(true);
      sub.retry = setTimeout(() => liveRefresh(sub), Math.min(500 * 2 ** sub.fails, 30000) * (0.5 + Math.random()));
      return;
    }
    sub.fails = 0;
    if (sub.sig.$wait) sub.sig.$wait.set(false);
    liveSeen(seq, server);
    sub.seen = seq;
    sub.seenOn = server;
    if (sub.wanted > seq) queueMicrotask(() => liveRefresh(sub));
    sub.topics = topics;
    sub.h = value instanceof Err ? "" : h;
    sub.base = value;
    sub.sig.set(value);
    liveRestart();
  } catch (e) {
    if (n === sub.loads) { sub.loading = false; if (sub.sig.$wait) sub.sig.$wait.set(false); }
    report(e);
  }
}

// ---- reactivity ----------------------------------------------------------
// A component's code runs once. Its state is signals (`Sig`); what reads
// them, a memo or an effect (`Calc`), hears when they change. Effects run
// again in a batch, in a microtask, in the order they were made; memos only
// when read, and only if something they read changed. Each holds what was
// made while it ran (effects, memos, cleanups), which goes when it runs
// again or is disposed: a component, a row, a branch takes all of its
// effects with it when it leaves the page.

const CLEAN = 0, CHECK = 1, DIRTY = 2;
let Listener = null;
let Owner = null;
// The component (or row) a handler or `@mount` running now belongs to, for what it
// starts to end with it: `run_after` and `every`.
let Acting = null;
let effectQueue = [];
let flushing = false;
let calcIds = 0;

// What owns what is made while it runs: a component, a row, a branch.
class Scope {
  constructor() {
    this.owned = null;
    this.clean = null;
    this.dead = false;
    this.up = Owner;
    // Made to size while small (an array pushed to from empty keeps room
    // for a dozen and more; a row's scope owns one or two).
    if (Owner) Owner.owned = Owner.owned ? (Owner.owned.length < 8 ? Owner.owned.concat([this]) : (Owner.owned.push(this), Owner.owned)) : [this];
  }
}

function disposeKids(s) {
  const owned = s.owned, clean = s.clean;
  s.owned = null;
  s.clean = null;
  if (owned) for (const k of owned) { k.up = null; dispose(k); }
  if (clean) for (const f of clean) { try { f(); } catch (e) { report(e); } }
}

// A scope gone: what it made goes too, and it lets go of what it read.
function dispose(s) {
  if (s.dead) return;
  s.dead = true;
  if (TOOLS && s instanceof Inst) toolsGone(s);
  const up = s.up;
  if (up && up.owned) {
    const i = up.owned.indexOf(s), last = up.owned.pop();
    if (last !== s) up.owned[i] = last;
  }
  s.up = null;
  disposeKids(s);
  if (s.src) unlinkFrom(s, 0);
}

// Runs `f` when the current scope goes (or its computation runs again).
function onCleanup(f) {
  if (Owner) (Owner.clean || (Owner.clean = [])).push(f);
}

// What reads what, both ways, in one array each: `c.src` holds pairs
// [source, where c is in its `obs`], `s.obs` pairs [reader, where s is in
// its `src`]. Either side lets go in constant time. Small arrays are made
// to size (a pushed-to array would keep room for a dozen more).
class Sig {
  // `n`: the times it has been claimed by a run that did not hold it, `r` the one that does (see
  // `W.claim`).
  constructor(v) { this._v = v; this.obs = null; this.keys = null; this.n = 0; this.r = null; }
  get v() { if (Listener) link(Listener, this); return this._v; }
  set(v) {
    const was = this._v;
    if (v === was) return null;
    this._v = v;
    const o = this.obs;
    if (o) for (let i = 0; i < o.length; i += 2) stale(o[i], DIRTY);
    // Those that only asked whether it is one value (see `isNow`).
    if (this.keys) { keyStale(this.keys.get(was)); keyStale(this.keys.get(v)); }
    return null;
  }
}

// What tells those, when a page has them (`isNow` brings it).
let keyStale = null;

class Calc extends Scope {
  constructor(fn, effect) {
    super();
    this.fn = fn;
    this.effect = effect;
    this.state = DIRTY;
    this.src = null;
    this.at = 0;
    this.obs = null;
    this._v = undefined;
    this.id = ++calcIds;
    // An effect on the page: `apply(value)` (as a method, untracked) puts
    // what `fn` gave on `el`; `name` and `old` are its to use.
    this.apply = null;
    this.el = null;
    this.name = null;
    this.old = undefined;
  }
  // A memo's value, brought up to date.
  get v() {
    if (Listener) link(Listener, this);
    fresh(this);
    return this._v;
  }
}

const pair = (a, x, y) => {
  if (!a) return [x, y];
  if (a.length === 2) return [a[0], a[1], x, y];
  if (a.length < 16) return a.concat([x, y]);
  a.push(x, y);
  return a;
};

// `c` read `s`. A computation running again mostly reads what it read
// before, in the same order: then nothing changes but `c.at`.
function link(c, s) {
  if (c.dead) return;
  const src = c.src, at = c.at;
  if (src) {
    if (at < src.length) {
      if (src[at] === s) { c.at = at + 2; return; }
      unlinkFrom(c, at);
    }
    if (at <= 16) for (let i = 0; i < at; i += 2) if (src[i] === s) return;
  }
  const idx = src ? src.length : 0;
  c.src = pair(src, s, s.obs ? s.obs.length : 0);
  s.obs = pair(s.obs, c, idx);
  c.at = idx + 2;
}

// `c` lets go of its sources from `from` on.
function unlinkFrom(c, from) {
  const src = c.src;
  for (let i = src.length - 2; i >= from; i -= 2) {
    const slot = src.pop(), s = src.pop(), obs = s.obs, n = obs.length - 2;
    if (slot < n) {
      const last = obs[n], li = obs[n + 1];
      obs[slot] = last;
      obs[slot + 1] = li;
      last.src[li + 1] = slot;
    }
    obs.pop();
    obs.pop();
    if (n === 0 && s.of) s.of.keys.delete(s.k);
  }
}

function stale(c, level) {
  if (c.state >= level || c.dead) return;
  const was = c.state;
  c.state = level;
  if (was !== CLEAN) return;
  if (c.effect) {
    effectQueue.push(c);
    if (!flushing) { flushing = true; queueMicrotask(flush); }
  } else if (c.obs) {
    const o = c.obs;
    for (let i = 0; i < o.length; i += 2) stale(o[i], CHECK);
  }
}

// Brings `c` up to date: if a memo it read may have changed, that memo is
// brought up to date first, and `c` runs only if one did.
function fresh(c) {
  if (c.state === CHECK) {
    const src = c.src;
    if (src) for (let i = 0; i < src.length; i += 2) {
      const s = src[i];
      if (s instanceof Calc) {
        fresh(s);
        if (c.state === DIRTY) break;
      }
    }
    if (c.state === CHECK) c.state = CLEAN;
  }
  if (c.state === DIRTY) runCalc(c);
}

function runCalc(c) {
  disposeKids(c);
  c.at = 0;
  const l = Listener, o = Owner;
  Listener = c;
  Owner = c;
  let v;
  try {
    v = c.fn();
    if (c.apply !== null) {
      Listener = null;
      c.apply(v);
    }
  } catch (e) {
    Listener = l;
    Owner = o;
    if (c.src && c.at < c.src.length) unlinkFrom(c, c.at);
    c.state = c.effect ? CLEAN : DIRTY;
    // Taking up the page's HTML found it not what the code makes (a text a script changed):
    // for the component to be made again (see `island`, `resume`), not an error to report,
    // after which it stayed empty (branch feedback40-43).
    if (c.effect && !(e instanceof Mismatch)) {
      report(e);
      // A place whose value failed is emptied: the last good value beside a field that no
      // longer says it reads as the page's answer (FEEDBACK30.md, Q-370; branch
      // feedback27-35-fixes).
      if (c.apply !== null) { const l2 = Listener; Listener = null; try { c.apply(null); } catch (_) {} finally { Listener = l2; } }
      return;
    }
    throw e;
  }
  Listener = l;
  Owner = o;
  if (c.src && c.at < c.src.length) unlinkFrom(c, c.at);
  c.state = CLEAN;
  if (!c.effect && v !== c._v) {
    c._v = v;
    const ob = c.obs;
    if (ob) for (let i = 0; i < ob.length; i += 2) if (ob[i].state === CHECK) ob[i].state = DIRTY;
  }
}

function flush() {
  let rounds = 0, jumps = 0;
  // An effect is a run of its own, also when `page_updated` brings this about inside one
  // (FEEDBACK55.md, Q-800; branch feedback55).
  const run = Run;
  Run = null;
  while (effectQueue.length) {
    if (++rounds > 1000) {
      effectQueue = [];
      report("updates go round in a loop: something sets what it reads");
      break;
    }
    let q = effectQueue;
    effectQueue = [];
    // In the order they were made (mostly so already).
    for (let i = 1; i < q.length; i++) if (q[i - 1].id > q[i].id) { q.sort((a, b) => a.id - b.id); break; }
    for (let i = 0; i < q.length; i++) {
      const c = q[i];
      if (!c.dead) fresh(c);
      // What this woke goes first when it was made earlier: a row's `cond &&` before
      // the place inside it, which must not run once its branch has gone
      // (FEEDBACK25.md, Q-311; branch feedback26-fixes).
      if (effectQueue.length && i + 1 < q.length && effectQueue.some((n) => n.id < q[i + 1].id) && ++jumps < 100000) {
        q = q.slice(i + 1).concat(effectQueue).sort((a, b) => a.id - b.id);
        effectQueue = [];
        i = -1;
      }
    }
  }
  Run = run;
  flushing = false;
}

// Runs `f` now, and again (in a batch) whenever what it read changes.
function effect(f) {
  const c = new Calc(f, true);
  runCalc(c);
  return c;
}

// An effect putting what `get` gives on the page with `apply`, run now.
function bound(get, apply, el, name, old) {
  const c = new Calc(get, true);
  c.apply = apply;
  c.el = el;
  c.name = name;
  c.old = old;
  runCalc(c);
  return c;
}

// `f`'s value, kept until what it read changes, then worked out again when
// next read. (Worked out at once the first time, as the code is written.)
function memo(f) {
  const c = new Calc(f, false);
  runCalc(c);
  return c;
}

function untrack(f) {
  const l = Listener;
  Listener = null;
  try { return f(); } finally { Listener = l; }
}

function within(scope, f) {
  const o = Owner, l = Listener;
  Owner = scope;
  Listener = null;
  try { return f(); } finally { Owner = o; Listener = l; }
}

// W.is(state, x), for `x == state` read while tracking: it changes only when
// the state becomes `x` or stops being it, so what reads it hears of just
// those changes (a thousand rows asking whether each is the chosen one: two
// run, not all).
class SelKey {
  constructor(of, k) { this.of = of; this.k = k; this.obs = null; }
}

function isNow(s, x) {
  if (Listener && x !== null && typeof x !== "object") {
    const keys = s.keys || (s.keys = new Map());
    let key = keys.get(x);
    if (!key) keys.set(x, (key = new SelKey(s, x)));
    link(Listener, key);
    return eq(s._v, x);
  }
  return eq(s.v, x);
}
keyStale = (key) => {
  const o = key && key.obs;
  if (o) for (let i = 0; i < o.length; i += 2) stale(o[i], DIRTY);
};

// ---- markup ----------------------------------------------------------------
// Markup is made into nodes when it is put on the page: `make(c, out)`
// adds its nodes to `out`, each time anew (the same markup can be put in
// twice). `c` says where: a parent and, while the server's page is taken
// up (`c.claim`), the page's node to take up next, which taking up moves past.

class Html { constructor(make) { if (make) this.make = make; } }

// An HTML value's text is its markup, as the server writes it: it was "<html>", and empty in JSON
// (FEEDBACK55.md, Q-779; branch feedback55). What `raw`, `markdown` and `highlight` made has its
// own text; markup made here is made into nodes off the page, as it is shown, and written out as
// the server writes HTML, text and attribute values escaped as it escapes them. Without a
// document (in Node), "<html>".
const VOID_EL = new Set(["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track", "wbr"]);
// (`&\x6ct;` is `&lt;`: the bundle takes in what a word in the runtime names, and `lt` is one.)
const ESC = { "&": "&amp;", "<": "&\x6ct;", ">": "&\x67t;", '"': "&quot;", "'": "&#39;", "\r": "&#13;", "\0": "\uFFFD" };
const escText = (t) => t.replace(/[&<>"'\r\0]/g, (c) => ESC[c]);
function htmlText(h) {
  if (h.text !== undefined) return h.text;
  if (typeof document === "undefined" || !h.make) return "<html>";
  const box = document.createDocumentFragment(), out = [], o = Owner, was = hydrating, cl = claiming;
  Owner = null;
  const sc = new Scope();
  Owner = o;
  hydrating = null;
  claiming = 0;
  marking = true;
  try { within(sc, () => h.make({ p: box, n: null, claim: false, svg: false }, out)); } finally { hydrating = was; claiming = cl; marking = false; dispose(sc); }
  return out.map(nodeHtml).join("");
}
// While `htmlText` makes markup: each element made from a template is marked with it (`markTpl`),
// and one set to `true` with the attribute (`setProp`), so that it is written as the server
// writes it: an attribute of `true` bare, one of "" as `=""`, in the order the source has them
// (found in review: `alt=""` was bare, and `<input value="v" type="text">` had its type first).
let marking = false;
function markTpl(tp, el) {
  el.__tp = tp;
  const kids = (tp[0] === "template" && el.content || el).childNodes;
  tp[2].forEach((k, i) => { if (typeof k === "object") markTpl(k, kids[i]); });
}
function nodeHtml(n) {
  if (n.nodeType === 3) return escText(n.data);
  if (n.nodeType === 8) return "<!--" + n.data + "-->";
  if (n.nodeType !== 1) return "";
  const kids = (n.localName === "template" ? n.content : n).childNodes;
  if (n.localName === "quillo-raw") return Array.from(kids, nodeHtml).join("");
  const tp = n.__tp, ord = tp && tp[4], bare = n.__bare;
  let attrs = Array.from(n.attributes);
  if (ord) { const at = (a) => { const i = ord.indexOf(a.name); return i < 0 ? ord.length : i; }; attrs = attrs.sort((a, b) => at(a) - at(b)); }
  let t = "<" + n.localName;
  for (const a of attrs) {
    const plain = a.value === "" && ((bare && bare.has(a.name)) || (tp && tp[1].some((f) => f[0] === a.name && f[1] === true)));
    t += plain ? " " + a.name : " " + a.name + '="' + escText(a.value) + '"';
  }
  t += ">";
  return VOID_EL.has(n.localName) ? t : t + Array.from(kids, nodeHtml).join("") + "</" + n.localName + ">";
}

const SVG = "http://www.w3.org/2000/svg";
const inSvg = (el) => el.namespaceURI === SVG && el.localName !== "foreignObject";
const isText = (x) => typeof x === "string" || typeof x === "number" || x instanceof Dec || typeof x === "bigint" || x instanceof Fl;
// Taking up the server's page: inputs keep what was typed (see setProp).
let claiming = 0;

class Mismatch extends Error {}
// With --watch, what was being taken up last: the page's node, and what
// the browser makes there. A Mismatch is thrown right after, so it is what
// differed (see `resume`).
let wanted = null; // dev
const onPage = (n) => !n ? "nothing" : typeof n === "string" ? n : n.nodeType === 3 ? JSON.stringify(n.data.slice(0, 80)) : n.nodeType === 1 ? `<${n.localName}>` : "a comment"; // dev
// Taking up the server's page in development, an attribute is compared as text is: one that
// differs makes the component again, and is said so, where the browser's took the server's place
// unseen (FEEDBACK55.md, Q-774; branch feedback55). In production the browser's value takes its
// place, as before: an extension that rewrites a style, or a <details> opened before the code came,
// would make the component again on every load (decided in review).
function attrDiffers(el, k, s) {
  if (DEV) wanted = [attrSaid(k, el.getAttribute(k)), attrSaid(k, s)]; // dev
  throw new Mismatch("attribute");
}
const attrSaid = (k, v) => (v === null ? `no ${k}` : `${k}=${JSON.stringify(v.slice(0, 80))}`); // dev

// The page's text node at `c.n`, taken up as text `t`: split where the
// browser joined it with text after it; an empty one is made.
function claimText(c, t) {
  if (t === "") {
    const e = document.createTextNode("");
    c.p.insertBefore(e, c.n);
    return e;
  }
  const n = c.n;
  if (DEV) wanted = [n, JSON.stringify(t.slice(0, 80))]; // dev
  if (!n || n.nodeType !== 3) throw new Mismatch("text");
  if (n.data === t) { c.n = n.nextSibling; return n; }
  if (n.data.startsWith(t)) { c.n = n.splitText(t.length); return n; }
  throw new Mismatch("text");
}

// The nodes for value `v` (text, markup, a list of them), into `out`.
function put(v, c, out) {
  // `true` shows nothing, as `false` and null do, as in React, and as the server writes it; a
  // function too (FEEDBACK48.md, Q-564 and Q-571; branch main).
  if (isNil(v) || typeof v === "boolean" || typeof v === "function") return;
  if (Array.isArray(v)) { for (const x of v) put(x, c, out); return; }
  if (v instanceof Html) { v.make(c, out); return; }
  const t = shown(v);
  out.push(c.claim ? claimText(c, t) : document.createTextNode(t));
}
// A value as the page shows it: a NUL is U+FFFD, as the server writes it and the HTML parser makes
// it, or the server's page read otherwise than what the browser makes (FEEDBACK55.md, Q-772;
// branch feedback55).
const noNul = (t) => (t.indexOf("\0") < 0 ? t : t.replace(/\0/g, "\uFFFD"));
const shown = (v) => noNul(display(v));

// A part's nodes now: a dynamic part keeps its own (see `dyn`), ahead of
// its marker.
function nodesOf(list, out = []) {
  for (const n of list) {
    if (n.__part) nodesOf(n.__part.nodes, out);
    out.push(n);
  }
  return out;
}

// What a record of nodes keeps of `got`, the nodes just made (a row's, a part's): not those of a
// part among them, which the part answers for and can replace, but its marker, which `nodesOf` and
// `firstOf` read them from as they are now. A row kept the nodes it was made with: once a branch
// at its root had swapped in a new element, a move brought the old one back, alive, and the place
// after it was a node no longer on the page (FEEDBACK61.md, Q-889; branch feedback61). `made`:
// `partsMade` before they were made; when no part was made since, none of them is a marker.
function own(got, made) {
  if (partsMade === made) return got;
  const len = got.length, last = len ? got[len - 1].__part : null;
  // Most often the last part made made all the others, and put its marker last (a branch at a
  // row's root, its element in a part of its own): the marker alone. Each part notes its own
  // count and size: a page-wide note of the last part held it, and a part that left the page
  // stayed alive with all around it until another was made (found in the second review).
  if (last && last.made === partsMade && last.size === len - 1) return len === 1 ? got : [got[len - 1]];
  let i = 0;
  while (i < len && !got[i].__part) i++;
  if (i === len) return got;
  const theirs = new Set();
  for (; i < len; i++) if (got[i].__part) for (const n of nodesOf(got[i].__part.nodes)) theirs.add(n);
  return got.filter((n) => !theirs.has(n));
}
// How many parts were made. A part notes its number in that count (`made`), and how many nodes it
// put out before its marker (`size`).
let partsMade = 0;

// A <textarea>'s text is only its first value: once someone types, the box
// stops showing it. When the text between its tags changes, the box shows
// the new text; when it does not, what was typed stays.
function textareaFollows(el) {
  if (el && el.localName === "textarea" && el.value !== el.textContent) el.value = el.textContent;
}

// With no handler hearing it (see `removing`).
function removeNodes(list) {
  removing++;
  try { for (const n of nodesOf(list)) if (n.parentNode) n.parentNode.removeChild(n); } finally { removing--; }
}
// Above 0 while Quillo takes elements off the page, during which no handler runs (see `fire`;
// FEEDBACK61.md, Q-896; branch feedback61).
let removing = 0;

// A part of the page that follows `get`: its nodes, then a marker (an
// empty text node) that later nodes go in front of. A text that becomes
// another text is changed in place.
function dyn(get, c, out) {
  bound(get, dynApply, { nodes: [], end: null, c, out, made: 0, size: 0 }, null, undefined);
}

function dynApply(x) {
  const r = this.el, c = r.c;
  if (r.end === null) {
    const got = [], made = partsMade;
    put(x, c, got);
    r.nodes = own(got, made);
    r.end = document.createTextNode("");
    r.end.__part = r;
    partsMade++;
    r.made = partsMade;
    r.size = got.length;
    if (c.claim) c.p.insertBefore(r.end, c.n);
    for (const n of got) r.out.push(n);
    r.out.push(r.end);
    r.c = { claim: false, svg: c.svg };
    r.out = null;
    return;
  }
  const one = r.nodes.length === 1 ? r.nodes[0] : null;
  if (one && one.nodeType === 3 && !one.__part && isText(x)) {
    const t = shown(x);
    if (one.data !== t) {
      one.data = t;
      textareaFollows(one.parentNode);
    }
    return;
  }
  const got = [], made = partsMade;
  put(x, c, got);
  // Markup a function made again, as `{tick(on)}` is, with the same elements as on the page and
  // nothing in it that follows anything itself: the page's elements take the new one's
  // attributes, text and handlers, and stay, so focus and what was typed stay (FEEDBACK25.md,
  // Q-306; branch feedback27-35-fixes).
  if (!this.owned && !this.clean && sameShape(r.nodes, got)) {
    for (let i = 0; i < got.length; i++) morph(r.nodes[i], got[i]);
    return;
  }
  removeNodes(r.nodes);
  const p = r.end.parentNode;
  if (p) for (const n of got) p.insertBefore(n, r.end);
  r.nodes = own(got, made);
  textareaFollows(p);
  // Options shown or hidden: their select shows its value again (FEEDBACK55.md, Q-758; branch
  // feedback55).
  selFollow(p);
}

// Whether two lists of nodes are the same elements and texts, in the same order, all the way
// down, with nothing in the new ones that runs by itself.
function sameShape(a, b) {
  if (a.length !== b.length || a.length === 0) return false;
  for (let i = 0; i < a.length; i++) {
    const x = a[i], y = b[i];
    if (x.nodeType !== y.nodeType || x.__part || y.__part) return false;
    if (x.nodeType === 1) {
      if (x.tagName !== y.tagName || y.__mounted || x.__mounted || (x.type || "") !== (y.type || "")) return false;
      if (!sameShape(Array.from(x.childNodes), Array.from(y.childNodes)) && (x.childNodes.length || y.childNodes.length)) return false;
    } else if (x.nodeType !== 3) return false;
  }
  return true;
}

// `old` made to say what `now` says: attributes, the values a person changes, text, handlers.
function morph(old, now) {
  if (old.nodeType === 3) { if (old.data !== now.data) old.data = now.data; return; }
  for (const a of Array.from(old.attributes)) if (!now.hasAttribute(a.name)) old.removeAttribute(a.name);
  for (const a of Array.from(now.attributes)) if (old.getAttribute(a.name) !== a.value) old.setAttribute(a.name, a.value);
  for (const k of ["value", "checked", "selected", "muted", "indeterminate"]) if (k in now && old[k] !== now[k]) old[k] = now[k];
  old.__click = now.__click;
  old.__own = now.__own;
  if (now.__want !== undefined) old.__want = now.__want;
  if (now.__w) {
    const cur = old.__w || (old.__w = {}), listening = old.__wl || (old.__wl = {});
    for (const t of Object.keys(cur)) if (!(t in now.__w)) delete cur[t];
    for (const t of Object.keys(now.__w)) {
      cur[t] = now.__w[t];
      if (!listening[t]) { listening[t] = true; old.addEventListener(t, (ev) => fire(old, t, ev)); }
    }
  } else if (old.__w) old.__w = {};
  if (now.__click && !delegating) { delegating = true; document.addEventListener("click", delegated); }
  const oc = old.childNodes, nc = now.childNodes;
  for (let i = 0; i < nc.length; i++) morph(oc[i], nc[i]);
}

// An element's only content, following `get`: its children, without markers.
function dynAll(el, get, claim, svg) {
  bound(get, dynAllApply, el, svg, claim);
}

function dynAllApply(x) {
  const el = this.el, svg = this.name;
  if (this.old) {
    // Taking up the page's children, the first time.
    this.old = false;
    const c = { p: el, n: el.firstChild, claim: true, svg };
    if (isText(x) && !el.firstChild) el.appendChild(document.createTextNode(""));
    c.n = el.firstChild;
    const got = [];
    put(x, c, got);
    if (isText(x) && got.length === 0) el.appendChild(document.createTextNode(""));
    rest(el, c.n);
    return;
  }
  const t = el.firstChild;
  if (t && t === el.lastChild && t.nodeType === 3 && !t.__part && isText(x)) {
    const s = shown(x);
    if (t.data !== s) {
      t.data = s;
      textareaFollows(el);
    }
    return;
  }
  const got = [];
  put(x, { claim: false, svg }, got);
  if (isText(x) && got.length === 0) got.push(document.createTextNode(""));
  // The children it replaces hear nothing as they go (see `removing`; FEEDBACK61.md, Q-896; branch
  // feedback61).
  removing++;
  try { el.replaceChildren(...got); } finally { removing--; }
  textareaFollows(el);
  selFollow(el);
}

// What is left of an element's children after taking it up: empty text
// nodes go; anything else is not what the code makes.
function rest(el, n) {
  while (n) {
    const next = n.nextSibling;
    if (DEV) wanted = [n, "nothing"]; // dev
    if (n.nodeType === 3 && n.data === "") el.removeChild(n);
    else throw new Mismatch("extra");
    n = next;
  }
}

// ---- templates -----------------------------------------------------------
// Markup of a fixed shape is a template, made once and cloned for each
// use: [tag, [[name, value], ...], [kid, ...], [[kind, name, i], ...]], a
// kid being text, a template, or a number: value `i`, shown there. The
// last list is the element's holes: 0 an attribute, 1 its class, 2 a
// handler, 3 `{...map}` spread into it. A hole's value is fixed, or a
// function giving it (then it follows what that reads).

function block(tpl) {
  plan(tpl);
  return { tpl, html: null, svg: null, af: autofocused(tpl) };
}

// Whether a template writes `autofocus` itself, which its copies are focused for (see `focusSoon`).
const autofocused = (tp) => tp[1].some((a) => a[0].toLowerCase() === "autofocus") || tp[2].some((k) => typeof k === "object" && autofocused(k));

// Which of a template's children have holes: `tp.dyn`, whether `tp` or
// what is in it has any; `tp.last`, its last child that does (made copies
// are walked that far, and only into those).
function plan(tp) {
  let last = -1;
  tp[2].forEach((k, i) => { if (typeof k === "number" || (typeof k === "object" && plan(k))) last = i; });
  tp.last = last;
  return (tp.dyn = last >= 0 || tp[3].length > 0);
}

function blockTemplate(def, svg) {
  const make = (t, inSvg) => {
    const here = inSvg || t[0] === "svg";
    const el = here ? document.createElementNS(SVG, t[0]) : document.createElement(t[0]);
    for (const [k, v] of t[1]) codeAttr(k) ? el.setAttribute(k, v === true ? "" : display(v)) : setProp(el, k, v, undefined);
    // A <template>'s children go in its content (see `wire`).
    const box = t[0] === "template" && el.content || el;
    for (const k of t[2]) box.appendChild(typeof k === "string" ? document.createTextNode(k) : typeof k === "number" ? document.createTextNode("") : make(k, here && t[0] !== "foreignObject"));
    return el;
  };
  return svg ? (def.svg || (def.svg = make(def.tpl, true))) : (def.html || (def.html = make(def.tpl, false)));
}

// W.t(def, values): a use of a template.
// In a `for` row's own template, `row` is the row's item and `mask` says
// which values read nothing else: the row updates those itself (`direct`).
class Tmpl extends Html {
  constructor(def, vals, row, mask) {
    super(null);
    this.def = def;
    this.vals = vals;
    this.row = row;
    this.mask = mask;
  }
  make(c, out) {
    const def = this.def, svg = c.svg || def.tpl[0] === "svg";
    const r = rowNow, m = maskNow;
    rowNow = this.row;
    maskNow = this.mask;
    try {
      if (c.claim) {
        const el = c.n;
        claimEl(def.tpl, el, this.vals, svg);
        c.n = el.nextSibling;
        out.push(el);
        return;
      }
      const el = blockTemplate(def, svg).cloneNode(true);
      if (marking) markTpl(def.tpl, el);
      wire(def.tpl, el, this.vals, svg);
      if (def.af) focusSoon(el.hasAttribute("autofocus") ? el : el.querySelector("[autofocus]"));
      out.push(el);
    } finally {
      rowNow = r;
      maskNow = m;
    }
  }
}

const tmpl = (def, vals, row = null, mask = 0) => new Tmpl(def, vals, row, mask);

// The row whose template is being made, and which of its values read only
// it; what makes those (`rowHole`, which pages with `for` rows bring).
let rowNow = null, maskNow = 0, rowHole = null;
const byRow = (i) => rowNow !== null && ((maskNow >> i) & 1) === 1;

function wire(tp, el, vals, svg) {
  const holes = tp[3];
  for (let i = 0; i < holes.length; i++) { const h = holes[i]; hole(el, h[0], h[1], vals[h[2]], svg, h[2]); }
  const kids = tp[2], last = tp.last;
  if (last < 0) return;
  const sv = inner(tp, svg);
  // A <template>'s children are its content, where the browser's parser puts them (FEEDBACK55.md,
  // Q-769; branch feedback55).
  const box = tp[0] === "template" && el.content || el;
  if (kids.length === 1 && typeof kids[0] === "number") showIn(box, vals[kids[0]], false, sv, kids[0]);
  else {
    let n = box.firstChild;
    for (let i = 0; i <= last; i++) {
      const k = kids[i], next = i < last ? n.nextSibling : null;
      if (typeof k === "number") showAt(box, n, vals[k], sv, k);
      else if (typeof k === "object" && k.dyn) wire(k, n, vals, sv);
      n = next;
    }
  }
  // Its options are in now: the value it was given before them (FEEDBACK55.md, Q-758; branch
  // feedback55).
  if (tp[0] === "select" && el.__want !== undefined) selectShow(el, el.__want, true);
}

const inner = (tp, svg) => (svg || tp[0] === "svg") && tp[0] !== "foreignObject";

// The page's element `el` taken up as template `tp`.
function claimEl(tp, el, vals, svg) {
  if (DEV) wanted = [el, `<${tp[0]}>`]; // dev
  if (!el || el.nodeType !== 1 || el.localName.toLowerCase() !== String(tp[0]).toLowerCase()) throw new Mismatch("element");
  claiming++;
  try { for (const h of tp[3]) hole(el, h[0], h[1], vals[h[2]], svg, h[2]); } finally { claiming--; }
  const kids = tp[2], sv = inner(tp, svg);
  // The server writes a textarea's value as its text.
  if (kids.length === 0 && tp[0] === "textarea") return;
  // A <template>'s children: its content (see `wire`).
  const box = tp[0] === "template" && el.content || el;
  if (kids.length === 1 && typeof kids[0] === "number") { showIn(box, vals[kids[0]], true, sv, kids[0]); return; }
  const c = { p: box, n: box.firstChild, claim: true, svg: sv };
  for (const k of kids) {
    if (typeof k === "string") claimText(c, k);
    else if (typeof k === "number") showClaim(c, vals[k], k);
    else { const n = c.n; claimEl(k, n, vals, sv); c.n = n.nextSibling; }
  }
  rest(box, c.n);
}

const live = (name) => name === "value" || name === "checked" || name === "selected";

function hole(el, kind, name, v, svg, i) {
  if (kind === 2) { listen(el, name, v); return; }
  if (kind === 3) { spreadInto(el, v); return; }
  if (typeof v !== "function") {
    if (kind === 0) setProp(el, name, v, undefined);
    else if (!isNil(v) && v !== "") setClass(el, v, svg);
    return;
  }
  if (kind === 0 && live(name)) {
    // Follows the state even after the user changed it (see `fire`).
    (el.__live || (el.__live = {}))[name] = v;
    bound(v, liveApply, el, name, undefined);
    return;
  }
  if (byRow(i)) rowHole(rowNow, v, kind === 0 ? attrApply : classApply, el, kind === 0 ? name : svg, undefined, false);
  else if (kind === 0) bound(v, attrApply, el, name, undefined);
  else bound(v, classApply, el, svg, undefined);
}

function liveApply(x) { setProp(this.el, this.name, x, undefined); }
function attrApply(x) { setProp(this.el, this.name, x, this.old); this.old = x; }
function classApply(x) {
  if (x === this.old) return;
  if (typeof this.old === "string" && typeof x === "string") {
    classTokens(this.el, this.old, x);
    if (x === "" && this.el.classList.length === 0) this.el.removeAttribute("class");
  } else setClass(this.el, x, this.name);
  this.old = x;
}

// A value shown among other nodes, at placeholder `at`.
function showAt(p, at, v, svg, i) {
  if (typeof v !== "function") {
    const got = [];
    put(v, { claim: false, svg }, got);
    for (const n of got) p.insertBefore(n, at);
    p.removeChild(at);
    return;
  }
  const got = [];
  if (byRow(i)) rowHole(rowNow, v, dynApply, { nodes: [], end: null, c: { claim: false, svg }, out: got }, null, undefined, true);
  else dyn(v, { claim: false, svg }, got);
  for (const n of got) p.insertBefore(n, at);
  p.removeChild(at);
}

// The same, taking up the page's nodes at `c`.
function showClaim(c, v, i) {
  if (typeof v !== "function") { put(v, c, []); return; }
  if (byRow(i)) rowHole(rowNow, v, dynApply, { nodes: [], end: null, c, out: [] }, null, undefined, true);
  else dyn(v, c, []);
}

// A value that is an element's only content.
function showIn(el, v, claim, svg, i) {
  if (typeof v === "function") {
    if (byRow(i)) rowHole(rowNow, v, dynAllApply, el, svg, claim, true);
    else dynAll(el, v, claim, svg);
    return;
  }
  if (claim) {
    const c = { p: el, n: el.firstChild, claim: true, svg };
    if (isText(v) && !el.firstChild) el.appendChild(document.createTextNode(""));
    c.n = el.firstChild;
    put(v, c, []);
    rest(el, c.n);
    return;
  }
  const got = [];
  put(v, { claim: false, svg }, got);
  if (isText(v)) { el.firstChild.data = shown(v); return; }
  el.replaceChildren(...got);
}

// `{...map}` into an element: attributes and `@event` handlers, as a
// written-out attribute would be, except that those written out win.
function spreadInto(el, get) {
  let had = {};
  effect(() => {
    const now = typeof get === "function" ? get() : get;
    untrack(() => {
      for (const k of Object.keys(had)) if (!hasOwn(now, k)) { if (k[0] === "@") spreadListen(el, k.slice(1), null); else setProp(el, k, null, had[k]); }
      for (const k of Object.keys(now)) { if (k[0] === "@") spreadListen(el, k.slice(1), now[k]); else setProp(el, k, now[k], had[k]); }
      had = now;
    });
  });
}

// A spread's handler on an event the element listens to already, as `bind:value`'s on
// `input`, runs after that one, which it took the place of, and the binding stopped
// (FEEDBACK48.md, Q-567; found in review). Taken away, the element's own is put back.
function spreadListen(el, t, f) {
  const own = el.__sb || (el.__sb = {});
  if (!(t in own)) own[t] = (t === "click" ? el.__click : el.__w && el.__w[t]) || null;
  const b = own[t];
  listen(el, t, !b || !f ? f || b : (e) => { b(e); return f.length > 0 ? f(e) : f(); });
}

// W.spread(maps, written): the attributes the maps give an element, without
// those it has written out, the key and the class (added to its own).
// Names are compared as the browser does (`ACTION` is `action`), so a key
// cannot stand in for a written attribute.
// A key follows the rules of the same one written out: `onClick: f` is a handler, `@click`, as
// the parser makes it (`onChange` on an <input> is `@input`), where it was dropped;
// `className` is `class` and `htmlFor` is `for`, as React's (FEEDBACK48.md, Q-567; branch main).
function spreadAttrs(maps, written, tag) {
  const o = {};
  const seen = new Set(written.map(attrKey));
  for (const m of maps) {
    if (isNil(m)) continue;
    if (!isMap(m)) fail(`\`{...}\` needs a map, got ${typeName(m)}`);
    // In the order set (FEEDBACK55.md, Q-875; branch feedback55).
    for (let k of mkeys(m)) {
      const v = m[k];
      const lower = k.toLowerCase();
      let n = attrKey(k);
      if (typeof v === "function" && /^on[a-z_-]+$/.test(lower)) n = k = "@" + reactEvent(tag, k, lower.slice(2));
      else if (n === "htmlfor") n = k = "for";
      if (n === "key" || n === "class" || n === "classname" || seen.has(n)) continue;
      if (k[0] !== "@" && !spreadAttrOk(k)) continue;
      seen.add(n);
      o[k] = v;
    }
  }
  return o;
}

// The DOM event React's handler `written` (`onDoubleClick`, as `doubleclick`) listens for, as
// the parser's `react_event` has it; a `…Capture` is refused, as written out.
function reactEvent(tag, written, ev) {
  if (ev === "change" && (tag === "input" || tag === "textarea")) return "input";
  if (ev === "doubleclick") return "dblclick";
  if (ev === "focus") return "focusin";
  if (ev === "blur") return "focusout";
  if (ev.length > 7 && ev.endsWith("capture") && ev !== "gotpointercapture" && ev !== "lostpointercapture") {
    const base = written.slice(2, written.length - 7);
    fail(`\`${written}\` listens before the element's own handlers, as React's \`…Capture\` does, which Quillo has no form for: write \`on${base[0].toUpperCase()}${base.slice(1)}\` (or \`@${reactEvent(tag, "on" + base, base.toLowerCase())}\`), which hears it as it bubbles up`);
  }
  return ev;
}

// `className` in a spread is its `class` (FEEDBACK48.md, Q-567; branch main).
// `class={...}`'s words, as clsx gives them and the server writes them: a map's keys whose
// values are truthy, a list's items (lists in it flattened, null and bools skipped); a map or a
// list was its text, and after an update a scramble of words (FEEDBACK48.md, Q-566; branch main).
function clsWords(x, out) {
  if (isNil(x) || typeof x === "boolean" || typeof x === "function" || x === "") return;
  if (Array.isArray(x)) { for (const y of x) clsWords(y, out); return; }
  // In the order set (FEEDBACK55.md, Q-875; branch feedback55).
  if (isMap(x)) { for (const k of mkeys(x)) if (k !== "" && truthy(x[k])) out.push(k); return; }
  out.push(display(x));
}

// `CLASS` and `Class` too, as the server takes them (found in review).
const spreadClasses = (maps) => maps.filter(isMap).flatMap((m) => mkeys(m).filter((k) => { const l = k.toLowerCase(); return l === "class" || l === "classname"; }).map((k) => m[k]));

// Several values side by side, each fixed or following what it reads.
function frag(items) {
  return new Html((c, out) => {
    for (const x of items) {
      if (typeof x === "function") dyn(x, c, out);
      else put(x, c, out);
    }
  });
}

// A part made again only when `key` changes (an element or component with
// a `key`, outside a loop: a new key starts it afresh).
function keyed(key, make) {
  const k = memo(key);
  return frag([() => { k.v; return untrack(make); }]);
}

// `cond ? <a/> : <b/>` and the like: `which` says which branch shows, and
// a branch is made again only when that changes.
function choose(which, branches) {
  const w = memo(which);
  return frag([() => { const i = w.v; return i < 0 || i >= branches.length ? null : part(untrack(branches[i])); }]);
}

// What a branch or a row gives: a function is a part that follows it.
function part(v) {
  return typeof v === "function" ? frag([v]) : v;
}

// Raw HTML, kept in a <quillo-raw> (how the page shows it after taking it up).
function rawHtml(html) {
  return new Html((c, out) => {
    if (c.claim) {
      const n = c.n;
      if (n && n.nodeType === 1 && n.localName === "quillo-raw") { c.n = n.nextSibling; out.push(n); return; }
      const t = document.createElement("template");
      t.innerHTML = html;
      const wrap = document.createElement("quillo-raw");
      wrap.style.display = "contents";
      c.p.insertBefore(wrap, c.n);
      for (let i = 0; i < t.content.childNodes.length; i++) {
        const m = wrap.nextSibling;
        if (DEV) wanted = [null, "raw(...)'s HTML"]; // dev
        if (!m) throw new Mismatch("raw");
        wrap.appendChild(m);
      }
      c.n = wrap.nextSibling;
      out.push(wrap);
      return;
    }
    const el = document.createElement("quillo-raw");
    el.style.display = "contents";
    el.innerHTML = html;
    out.push(el);
  });
}

function setClass(el, v, svg) {
  if (isNil(v) || v === "") el.removeAttribute("class");
  else if (svg || el.namespaceURI === SVG) el.setAttribute("class", v);
  else el.className = v;
}


// ---- client-side navigation ---------------------------------------------
// What reads url() follows the URL. Once something has,
// clicks on same-site links inside components change the URL without a
// reload (not with a modifier key, target, download or data-reload).

// `page`: the address's path and query, and `n`, how many times they changed, for a run to tell
// that the page changed under it (see `B.navigate`; FEEDBACK61.md, Q-902; branch feedback61).
const Nav = { at: new Sig(""), on: false, page: "", n: 0 };

// What reads url() follows it.
function navChanged() {
  const page = location.pathname + location.search;
  if (page !== Nav.page) { Nav.page = page; Nav.n++; }
  Nav.at.set(location.href);
}

// A link in a component is followed here, so a page fetched ahead would be thrown away, and
// the wait is all data. So what is kept is that page's components' state: taken as a page
// being resumed takes it, it needs no new way to ask for data and shows no `null` first
// (FEEDBACK22.md, Q-180; branch feedback22-fixes).
const Ahead = { to: "", states: null, at: 0, turn: 0 };
// As long as a browser may keep a prefetch of ours.
const AHEAD_KEPT = 30000;

function aheadDrop() {
  Ahead.to = "";
  Ahead.states = null;
  Ahead.turn++;
}

// Where a link in a component goes, if it is one this program follows itself.
function followed(a) {
  if (!a || !a.closest(ISLANDS)) return null;
  if ((a.target && a.target !== "_self") || a.hasAttribute("download") || a.hasAttribute("data-reload")) return null;
  const u = new URL(a.href, location.href);
  if (u.origin !== location.origin) return null;
  // Only links into this program; others load normally.
  if (BASE && u.pathname !== BASE && !u.pathname.startsWith(BASE + "/")) return null;
  return (u.pathname.slice(BASE.length) || "/").startsWith("/_quillo/") ? null : u;
}

function fetchAhead(e) {
  const a = e.target.closest && e.target.closest("a[href]");
  const u = a && !a.closest("[data-no-prefetch]") ? followed(a) : null;
  const c = navigator.connection;
  if (!u || !QW.pf || (c && (c.saveData || /2g/.test(c.effectiveType || "")))) return;
  const to = u.pathname + u.search;
  if (to === Ahead.to || to === location.pathname + location.search) return;
  aheadDrop();
  Ahead.to = to;
  const turn = Ahead.turn;
  fetch(to, { credentials: "same-origin", headers: { "x-quillo-prefetch": "1" } })
    .then((r) => (r.ok ? r.text() : null))
    .then((html) => {
      if (html === null || turn !== Ahead.turn) return;
      Ahead.states = new DOMParser().parseFromString(html, "text/html");
      Ahead.at = Date.now();
    })
    .catch(() => {});
}

// The state for the components a page brings. Not for one already on this page: it lives on
// and takes nothing, so one made beside it would take its place in the line and show
// another's data. That one asks for its data, as it would have.
function aheadFor(u) {
  const doc = Ahead.states, fresh = u.pathname + u.search === Ahead.to && Date.now() - Ahead.at < AHEAD_KEPT;
  aheadDrop();
  if (!doc || !fresh) return null;
  const here = new Set([...document.querySelectorAll(ISLANDS)].map((i) => i.dataset.c)), q = {};
  for (const i of doc.querySelectorAll(ISLANDS)) {
    if (here.has(i.dataset.c) || i.hasAttribute("data-qd")) continue;
    liveSeen(...(i.dataset.v || "").split(":"));
    (q[i.dataset.c] = q[i.dataset.c] || []).push({ s: islandJSON(i.dataset.s, {}), l: islandJSON(i.dataset.l, null) });
  }
  return q;
}

function navStart() {
  if (Nav.on || typeof window === "undefined") return;
  Nav.on = true;
  Nav.page = location.pathname + location.search;
  // Links in components are followed here: not prefetched as pages.
  QW.nav = true;
  window.addEventListener("popstate", navChanged);
  window.addEventListener("hashchange", navChanged);
  let pointed = 0;
  const o = { capture: true, passive: true };
  document.addEventListener("pointerover", (e) => { clearTimeout(pointed); pointed = setTimeout(() => fetchAhead(e), 65); }, o);
  for (const v of ["pointerdown", "touchstart", "focusin"]) document.addEventListener(v, fetchAhead, o);
  document.addEventListener("click", (e) => {
    if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
    const u = followed(e.target.closest && e.target.closest("a[href]"));
    if (!u) return;
    const path = u.pathname.slice(BASE.length) || "/";
    // A spot on this page: the browser scrolls there and sets :target, and
    // its popstate (or hashchange) updates url().
    if (u.hash && u.pathname === location.pathname && u.search === location.search) return;
    e.preventDefault();
    navigate(path + u.search + u.hash);
  });
}

function navigate(to) {
  // A javascript: URL runs as the page when nothing follows url(): `navigate(next)` after a
  // sign-in was XSS (FEEDBACK51.md, Q-645; branch feedback51-fixes).
  if (scriptURL(to)) fail(`\`navigate\` is not given a ${scriptScheme(to)} URL`);
  const full = withBase(to);
  // Nothing on the page follows url(): load the page there.
  if (!Nav.on) {
    location.assign(full);
    return null;
  }
  const u = new URL(full, location.href);
  // A spot on this page: the browser goes there itself, as for a link.
  if (u.hash && u.pathname === location.pathname && u.search === location.search) {
    if (u.hash !== location.hash) location.hash = u.hash;
    else toHash();
    navChanged();
    return null;
  }
  if (full !== location.pathname + location.search + location.hash) history.pushState(null, "", full);
  window.scrollTo(0, 0);
  // For the render this sets off, which runs as a microtask queued before this one's.
  const kept = aheadFor(u);
  if (kept) hydrating = kept;
  navChanged();
  if (kept) queueMicrotask(() => { if (hydrating === kept) hydrating = null; });
  if (u.hash) toHash();
  return null;
}

// Goes to the element the address's #hash names, as the browser does for
// a link (scrolled into view, and :target), once it is on the page: a page
// just navigated to may show it only when its data has come. Stays at the
// top when it never comes, or when the address changes meanwhile.
function toHash() {
  const at = location.href;
  const id = decodeURIComponent(location.hash.slice(1));
  let frames = 0;
  const look = () => {
    if (location.href !== at) return;
    // The same address with a #hash: a fragment navigation, no reload and no
    // new history entry.
    if (document.getElementById(id) || document.getElementsByName(id)[0]) location.replace(at);
    else if (++frames < 300) requestAnimationFrame(look);
  };
  look();
}

function currentUrl() {
  navStart();
  if (Nav.at._v === "" && typeof location !== "undefined") Nav.at._v = location.href;
  Nav.at.v;
  const p = location.pathname;
  return {
    path: BASE && p.startsWith(BASE) ? p.slice(BASE.length) || "/" : p,
    // In the order of the address, as on the server (FEEDBACK55.md, Q-875; branch feedback55).
    query: [...new URLSearchParams(location.search)].reduce((o, [k, v]) => mput(o, k, v), {}),
    hash: location.hash.slice(1),
  };
}


// Spread keys are data: plain attribute names only, and no event handlers
// or srcdoc, which would run the value as code (as on the server).
// An attribute's name as the browser compares it.
const attrKey = (k) => k.toLowerCase().replace(/^bind:/, "");

// Attributes whose value runs as code or shows as a page of the site.
function codeAttr(k) {
  const n = attrKey(k);
  return n.startsWith("on") || n === "srcdoc";
}

// The name that would be written is checked (`bind:onclick` is `onclick`);
// names with `:` (but xlink:href) come only from the program.
function spreadAttrOk(k) {
  return /^[^\s"'<>\/=`{}\u0000-\u001f\u007f]+$/.test(k) && !codeAttr(k) && !runtimeAttr(attrKey(k)) && !k.startsWith("__") && (!k.includes(":") || attrKey(k) === "xlink:href");
}

// The attributes this runtime reads to take an element up as a component (an island's mark,
// component, props, state, live data, whether it starts by itself), and Quillo's own
// `data-q-<style>` and `data-quillo-…`: never from a spread, as on the server, where visitor
// JSON made any element a component of its choosing (FEEDBACK55.md, Q-700; branch feedback55).
// The island's own, as the server writes them (`island_attr` in src/interp.rs), `data-qt` on what
// is swapped in for a waiting one (src/defer.rs).
const ISLAND_ATTRS = new Set(["data-qi", "data-qd", "data-qt", "data-c", "data-p", "data-s", "data-l", "data-v", "data-e"]);
const runtimeAttr = (n) => ISLAND_ATTRS.has(n) || n.startsWith("data-q-") || n.startsWith("data-quillo-");

const sandboxed = (el) => el.hasAttribute("sandbox") && !/allow-same-origin/i.test(el.getAttribute("sandbox"));

const URL_ATTRS = ["href", "src", "action", "formaction", "poster", "xlink:href", "cite", "background", "data", "codebase"];

// An SVG animation's values go to another attribute (`<set
// attributeName="href" to={u}>`), perhaps a URL one: they are checked as
// URLs, each of `values` (split by ;) too, as on the server.
const animValueAttr = (el, k) => (el.localName === "set" || el.localName === "animate") && /^(to|from|by|values)$/i.test(k);

// javascript: and vbscript: URLs, read as browsers read them.
function scriptURL(u) { return scriptScheme(u) !== ""; }
// Which of the two it is, for a refusal to name the one given, as on the server (FEEDBACK55.md;
// branch feedback55).
function scriptScheme(u) {
  const s = String(u).replace(/^[\u0000-\u0020]+/, "").replace(/[\t\n\r]/g, "").slice(0, 11).toLowerCase();
  return s.startsWith("javascript:") ? "javascript:" : s.startsWith("vbscript:") ? "vbscript:" : "";
}

// A style's declarations, by property.
function styleParts(text) {
  const out = new Map();
  let at = 0, depth = 0, quote = "";
  const take = (end) => {
    const d = text.slice(at, end), c = d.indexOf(":");
    if (c > 0) out.set(d.slice(0, c).trim(), d.slice(c + 1).trim());
  };
  for (let i = 0; i < text.length; i++) {
    const ch = text[i];
    if (quote) { if (ch === quote) quote = ""; }
    else if (ch === '"' || ch === "'") quote = ch;
    else if (ch === "(") depth++;
    else if (ch === ")") depth--;
    else if (ch === ";" && depth === 0) { take(i); at = i + 1; }
  }
  take(text.length);
  return out;
}

// `style={...}` and `class={...}` change what they name and nothing else: what a handler
// set on the element stays, as a drag's transform (FEEDBACK28.md, Q-337; branch
// feedback29-fixes).
function styleApply(el, was, now) {
  if (was === now) return;
  const before = styleParts(was), after = styleParts(now);
  for (const name of before.keys()) if (!after.has(name)) el.style.removeProperty(name);
  for (const [name, value] of after) {
    if (before.get(name) === value) continue;
    const m = /^(.*?)\s*!\s*important$/i.exec(value);
    el.style.setProperty(name, m ? m[1] : value, m ? "important" : "");
  }
}

function classTokens(el, was, now) {
  if (was === now) return;
  const before = was.split(/\s+/).filter(Boolean), after = now.split(/\s+/).filter(Boolean);
  for (const c of before) if (!after.includes(c)) el.classList.remove(c);
  for (const c of after) if (!before.includes(c)) el.classList.add(c);
}

// `style={{ color: c }}`: a property for each key, as on the server (FEEDBACK35.md, Q-429;
// branch feedback27-35-fixes). A number is in pixels but 0, on a custom property and on
// React's unitless properties, and `msFlex` is `-ms-flex`, as on the server (FEEDBACK48.md,
// Q-565; branch main).
const UNITLESS = new Set(("animation-iteration-count aspect-ratio border-image-outset border-image-slice border-image-width " +
  "box-flex box-flex-group box-ordinal-group column-count columns fill-opacity flex flex-grow flex-negative flex-order " +
  "flex-positive flex-shrink flood-opacity font-weight grid-area grid-column grid-column-end grid-column-span " +
  "grid-column-start grid-row grid-row-end grid-row-span grid-row-start line-clamp line-height opacity order orphans " +
  "scale stop-opacity stroke-dasharray stroke-dashoffset stroke-miterlimit stroke-opacity stroke-width tab-size widows " +
  "z-index zoom").split(" "));
const styleName = (k) => (k.startsWith("--") ? k : k.replace(/_/g, "-").replace(/[A-Z]/g, (c) => "-" + c.toLowerCase()).replace(/^ms-/, "-ms-"));
const styleNonzero = (v) => (typeof v === "number" ? v !== 0 && Number.isFinite(v) : typeof v === "bigint" ? v !== 0n : v instanceof Fl ? v.f !== 0 : v instanceof Dec && v.m !== 0n);
const stylePx = (n, v) => styleNonzero(v) && !n.startsWith("--") && !UNITLESS.has(n.replace(/^-(webkit|moz|ms|o)-/, "")) ? "px" : "";
// A value put in CSS: `{`, `}`, `;` and `\` as CSS's escapes, as on the server; and only a
// property's name for a key (FEEDBACK51.md, Q-668; branch feedback51-fixes).
const cssValue = (s) => s.replace(/[{};\\]/g, (c) => "\\" + c.charCodeAt(0).toString(16) + " ");
const styleKey = (k) => (k.startsWith("--") ? /^--[\p{L}\p{N}_-]+$/u : /^[A-Za-z0-9_-]+$/).test(k);
// In the order set (FEEDBACK55.md, Q-875; branch feedback55).
const styleText = (m) => mkeys(m).filter((k) => !isNil(m[k]) && m[k] !== false && styleKey(k))
  .map((k) => { const n = styleName(k); return n + ": " + cssValue(display(m[k])) + stylePx(n, m[k]); }).join("; ");

// The text a bool is for an attribute whose values are words, as on the server (see its
// `bool_attr_text`): "true"/"false" for aria-*, data-*, draggable, spellcheck and
// contenteditable, "yes"/"no" for translate; null for HTML's boolean attributes
// (FEEDBACK48.md, Q-564; branch main).
function boolText(k, v) {
  const n = k.toLowerCase();
  if (n === "translate") return v ? "yes" : "no";
  return (n.length > 5 && (n.startsWith("aria-") || n.startsWith("data-"))) || n === "draggable" || n === "spellcheck" || n === "contenteditable" ? String(v) : null;
}

function setProp(el, k, v, old) {
  // A component's own root that is its island (a table's row): the island's attributes are the
  // server's, and the markup's own neither change them nor add one (FEEDBACK55.md, Q-700; branch
  // feedback55).
  if (el.__q && ISLAND_ATTRS.has(attrKey(k))) return;
  // A function is no value of an attribute's, as on the server (found in review).
  if (typeof v === "function") v = null;
  if (k === "style" && isMap(v)) v = styleText(v);
  if (k === "style" && isMap(old)) old = styleText(old);
  // What the attribute is only the default of, as `value` (FEEDBACK28.md, Q-338; branch
  // feedback29-fixes).
  if (k === "value") {
    // What the program wants it to show, kept for when a select's options, or a range's `min`,
    // `max` or `step`, come or change after it (see `selFollow`, and `min` below; FEEDBACK55.md,
    // Q-758 and Q-765; branch feedback55).
    const first = el.__want === undefined;
    el.__want = v === undefined ? null : v;
    // Taking up the server's HTML: what the page shows (typed, ticked) stays.
    if (claiming) return;
    if (el.localName === "select") { selectShow(el, v, first); return; }
    // A number the field's text already reads as stays as typed: `1.0` on the way to `1.05` is
    // not written back as `1`; blank text reads as no number, and as null, which a number field
    // gives for it (FEEDBACK55.md, Q-757; branch feedback55).
    const typed = el.localName === "input" || el.localName === "textarea";
    if (typed && (isNil(v) ? el.value.trim() === "" : (typeof v === "number" || v instanceof Fl) && el.value.trim() !== "" && Number(el.value) === numOf(v))) return;
    const nv = isNil(v) ? "" : display(v);
    if (el.value !== nv) el.value = nv;
    if (el.localName === "option") selFollow(el.parentNode);
    // Made here, its first value is its default too, as the server writes it, so that a form's
    // reset brings it back (FEEDBACK55.md, Q-768; branch feedback55).
    if (first && !isNil(v)) { if (el.localName === "input") el.setAttribute("value", nv); else if (el.localName === "textarea") el.defaultValue = nv; }
    return;
  }
  if (k === "checked" || k === "selected" || k === "muted") {
    if (claiming) return;
    const nv = truthy(v);
    if (el[k] !== nv) el[k] = nv;
    if (nv) el.setAttribute(k, ""); else el.removeAttribute(k);
    if (marking && nv) (el.__bare || (el.__bare = new Set())).add(k);
    return;
  }
  // What only a property says: a box half ticked, a player's volume, place and speed. The
  // server's HTML cannot say them, so they are set when the page is taken up too, unless the
  // person has changed them already; a value out of range is brought into it, and one that is
  // no number is left out, as the browser would throw (branch feedback40-43).
  if (k === "indeterminate") { el.indeterminate = truthy(v); return; }
  if (k === "volume" || k === "currentTime" || k === "playbackRate") {
    let n = Number(v);
    if (isNil(v) || !Number.isFinite(n) || (claiming && el[k] !== (k === "currentTime" ? 0 : 1))) return;
    n = k === "volume" ? Math.min(1, Math.max(0, n)) : k === "playbackRate" ? Math.min(16, Math.max(0.0625, n)) : Math.max(0, n);
    // A place the player has just reported, set back a moment later, would seek while it plays:
    // within a tenth of a second is where it is.
    if (k === "currentTime" ? Math.abs(el.currentTime - n) < 0.1 : el[k] === n) return;
    try { el[k] = n; } catch (_) { /* a player that cannot take it now keeps what it has */ }
    return;
  }
  // What a person opened or closed stays until the value itself changes.
  if (k === "open" && old !== undefined && truthy(old) === truthy(v)) return;
  if ((k === "style" || k === "class") && typeof v === "string" && (typeof old === "string" || isNil(old) || old === false) && old !== undefined) {
    (k === "style" ? styleApply : classTokens)(el, typeof old === "string" ? old : "", v);
    return;
  }
  if (typeof v === "boolean") { const t = boolText(k, v); if (t !== null) v = t; }
  if (marking) { if (v === true) (el.__bare || (el.__bare = new Set())).add(k); else if (el.__bare) el.__bare.delete(k); }
  // Limits set after the value, which the browser brought into the old ones (a range's default
  // `max` of 100): the value again, as the server's page has it (FEEDBACK55.md, Q-765; branch
  // feedback55).
  if (k === "min" || k === "max" || k === "step") {
    if (isNil(v) || v === false) el.removeAttribute(k); else el.setAttribute(k, v === true ? "" : display(v));
    if (el.__want !== undefined && !claiming) setProp(el, "value", el.__want, undefined);
    return;
  }
  // On what the page makes later too, as React and Svelte have it (FEEDBACK55.md, Q-771; branch
  // feedback55).
  if (k === "autofocus" && old === undefined && !claiming && truthy(v)) focusSoon(el);
  if (isNil(v) || v === false) {
    if (claiming && el.hasAttribute(k)) attrDiffers(el, k, null); // dev
    el.removeAttribute(k);
    return;
  }
  // Only text written in the program goes there (set with the template),
  // and a page for a sandboxed frame that is kept away from the site.
  if (codeAttr(k) && !(attrKey(k) === "srcdoc" && sandboxed(el))) return;
  if (k === "href" || k === "src" || k === "action" || k === "formaction" || k === "poster") v = withBase(v);
  let s = v === true ? "" : (Array.isArray(v) || isMap(v)) ? jsonText(visible(v)) : noNul(display(v));
  // The text the attribute will say, whatever the value's type (html too).
  if (URL_ATTRS.includes(k.toLowerCase()) && scriptURL(s) || animValueAttr(el, k) && s.split(";").some(scriptURL)) v = s = "about:blank#blocked";
  if (claiming && el.getAttribute(k) !== s) attrDiffers(el, k, s); // dev
  // What the server's page already says is not written again while taking it up: that starts a
  // frame or a player over, and asks the server twice. One with a listener is started over all
  // the same, whichever came first (`listen` does it if `src` did): the load it waits for may have
  // been and gone, and only an image can say so (FEEDBACK61.md, Q-893; branch feedback61).
  const same = claiming && k === "src" && el.tagName !== "IMG" && el.getAttribute(k) === s;
  if (same && !el.__w) el.__kept = true;
  else if (same || (old !== v && !claiming) || el.getAttribute(k) !== s) el.setAttribute(k, s);
}

// A select shows the option its value names; a list (`<select multiple>`) each option it names;
// a one-line select that names none its first option, as the server's page shows them (it marks
// those `selected`, as one `made` here is marked: a form's reset comes back to them). Browsers
// ignore `value` on a select (FEEDBACK55.md, Q-758 and Q-761; branch feedback55).
function selectShow(el, v, made) {
  if (Array.isArray(v)) {
    const want = new Set(v.map((x) => (isNil(x) ? "" : display(x))));
    for (const o of el.options) { const on = want.has(o.value); if (o.selected !== on) o.selected = on; if (made && on) o.defaultSelected = true; }
  } else {
    const nv = isNil(v) ? "" : display(v);
    if (el.value !== nv) el.value = nv;
    if (made && el.selectedIndex >= 0) el.options[el.selectedIndex].defaultSelected = true;
  }
  if (el.selectedIndex < 0 && !el.multiple && el.size <= 1) for (const o of el.options) if (!o.disabled) { o.selected = true; break; }
}

// Options made, moved or changed after their select's value was set: it shows that value again.
function selFollow(p) {
  const s = p && p.localName === "optgroup" ? p.parentNode : p;
  if (s && s.localName === "select" && s.__want !== undefined && !claiming) selectShow(s, s.__want);
}

function fire(el, type, ev) {
  // No handler runs while Quillo takes elements off the page: Chrome sends a focused field `blur`
  // and `focusout` as it goes (to the parts it leaves too), where Firefox and Safari send none, and
  // an inline edit that saves on Enter and on leaving the field saved twice there (FEEDBACK61.md,
  // Q-896; branch feedback61). What comes later still reaches a removed element's handlers (a
  // card's `dragend`, after the drop that took it to another list), and one that moves keeps them.
  if (removing) return;
  const f = type === "click" ? el.__click : el.__w && el.__w[type];
  if (typeof f !== "function") return;
  if (type === "submit") ev.preventDefault();
  // What the field held when the event came: after a wait the field shows the state
  // again, and `e.value` would read that (FEEDBACK25.md, Q-304; branch feedback26-fixes).
  const t = ev.target;
  if (t && t.value !== undefined) { ev.$value = t.value; ev.$checked = !!t.checked; }
  const was = Acting;
  Acting = el.__own || null;
  // A handler is a run of its own, also when `el.click()` brings it about inside one
  // (FEEDBACK55.md, Q-800; branch feedback55).
  const run = Run;
  Run = null;
  try {
    const r = f.length > 0 ? f(ev) : f();
    if (r instanceof Promise) r.catch(report);
  } catch (e) {
    report(e);
  } finally {
    Acting = was;
    Run = run;
  }
  // An input shows the state's value even if the user typed something the
  // state did not take (once what the handler changed is shown).
  if (el.__live) queueMicrotask(() => { for (const k of Object.keys(el.__live)) setProp(el, k, untrack(el.__live[k]), undefined); });
}

// Calls `f` on `t` events from `el`; a later call replaces `f`. Clicks
// are handled once for the whole page, then passed to each element on
// their way up, innermost first (as the browser would).
function listen(el, t, f) {
  // Events named with a `$` are Quillo's own: `$mount` (not named here, so
  // that bundles without it leave it out).
  if (t.charCodeAt(0) === 36) { if (typeof f === "function" && mountHook) mountHook(el, f); return; }
  // An image from the server's HTML may have finished before this ran (FEEDBACK28.md,
  // Q-345; branch feedback29-fixes).
  if (claiming && el.tagName === "IMG" && el.complete && (t === "load" ? el.naturalWidth > 0 : t === "error" && el.getAttribute("src") && el.naturalWidth === 0)) {
    // Sent to the image itself, so the handler's `e.target` is it; a load of the same address
    // the browser sends after is the same one, and is not run twice (FEEDBACK28.md, Q-418;
    // branch feedback27-35-fixes).
    el.__rep = { t, src: el.currentSrc || el.src };
    queueMicrotask(() => { const ev = new Event(t); ev.__quilloReplay = true; el.dispatchEvent(ev); });
  }
  el.__own = Owner;
  if (t === "click") {
    el.__click = f;
    if (!delegating) { delegating = true; document.addEventListener("click", delegated); }
    return;
  }
  if (claiming && el.__kept) { el.__kept = false; el.setAttribute("src", el.getAttribute("src")); }
  const cur = el.__w || (el.__w = {});
  cur[t] = f;
  const listening = el.__wl || (el.__wl = {});
  if (!listening[t]) {
    listening[t] = true;
    el.addEventListener(t, (ev) => {
      const rep = el.__rep;
      if (rep && rep.t === t && !ev.__quilloReplay) { el.__rep = null; if (rep.src === (el.currentSrc || el.src)) return; }
      fire(el, t, ev);
    });
  }
}

let delegating = false;

// `$mount` when a component uses `@mount` (the compiler names the event
// so, and the bundle carries it only then).
let mountHook = null;

function unhook(g) {
  try { g(); } catch (e) { report(e); }
}

// `@mount={(el) => ...}`: called once the element is on the page, once in
// its life. A function it returns is called when the element leaves the
// page (with the part it is in), or its component does.
function $mount(el, f) {
  if (el.__mounted) return;
  el.__mounted = true;
  if ($mount.q.length === 0) queueMicrotask(() => {
    for (const [el, f, owner] of $mount.q.splice(0)) {
      if (!el.isConnected || (owner && owner.dead)) continue;
      const keep = (g) => { if (typeof g === "function" && owner) (owner.clean || (owner.clean = [])).push(() => unhook(g)); };
      const was = Acting;
      Acting = owner;
      try {
        const r = f.length > 0 ? f(el) : f();
        if (r instanceof Promise) r.then(keep, report); else keep(r);
      } catch (e) {
        report(e);
      } finally {
        Acting = was;
      }
    }
  });
  $mount.q.push([el, f, Owner]);
}
$mount.q = [];
mountHook = $mount;

// `autofocus` on what the page makes after it loaded, which the browser leaves alone: focused once
// it is on the page, as React and Svelte do, the first of those made at once on the page, unless
// someone is typing in another field by then (FEEDBACK55.md, Q-771; branch feedback55).
function focusSoon(el) {
  if (!el) return;
  if (focusSoon.q.length === 0) queueMicrotask(() => {
    let first = null;
    for (const e of focusSoon.q.splice(0)) if (e.isConnected && (!first || first.compareDocumentPosition(e) & Node.DOCUMENT_POSITION_PRECEDING)) first = e;
    const a = document.activeElement;
    if (first && !(a && a !== first && typingIn(a))) first.focus();
  });
  focusSoon.q.push(el);
}
focusSoon.q = [];
const typingIn = (a) => a.isContentEditable || a.localName === "textarea" || (a.localName === "input" && !/^(button|submit|reset|checkbox|radio|file|image|color|range)$/.test(a.type));

function delegated(ev) {
  for (const n of ev.composedPath()) {
    if (n === document) return;
    if (n.__click) {
      fire(n, "click", ev);
      if (ev.cancelBubble) return;
    }
  }
}


// ---- lists -----------------------------------------------------------------
// `for x in xs => <row/>`: a row for each item, kept while its key (or,
// without one, its place) stays, and given its new item when that
// changed. Rows that must move are moved: the fewest that leave the others
// in order, so a swap moves two and a removal none.

function each(list, key, row) {
  return new Html((c, out) => {
    const st = { rows: [], end: null, made: 0, size: 0, get nodes() { const a = []; for (const r of this.rows) for (const n of r.nodes) a.push(n); return a; } };
    const svg = c.svg;
    let first = true;
    // Rows are their own scopes, not their owner's: they come and go
    // without it, and go with it. Their nodes stay the list's: what holds it (a branch, as it
    // switches away from it) goes on to take them off the page, and finds them by its marker (see
    // `own`). Emptied here, a list that had grown since it was made left the new rows behind
    // (FEEDBACK61.md, Q-889; branch feedback61).
    onCleanup(() => { for (const r of st.rows) dispose(r.scope); });
    effect(() => {
      const items = iter(list());
      untrack(() => {
        if (!first) { reconcile(st, items, key, row, svg); return; }
        first = false;
        const at = out.length;
        st.rows = items.map((x, i) => newRow(x, key ? key(x) : i, row, c, out));
        st.end = document.createTextNode("");
        st.end.__part = st;
        partsMade++;
        st.made = partsMade;
        st.size = out.length - at;
        if (c.claim) c.p.insertBefore(st.end, c.n);
        out.push(st.end);
      });
    });
  });
}

// A row's item: a signal that also runs what follows only it (`rowDirect`).
class Item extends Sig {
  constructor(v, scope) {
    super(v);
    this.direct = null;
    this.scope = scope;
  }
  set(v) {
    if (v === this._v) return null;
    super.set(v);
    const ds = this.direct;
    if (!ds) return null;
    const l = Listener, o = Owner;
    Listener = null;
    Owner = this.scope;
    try {
      for (let i = 0; i < ds.length; i++) {
        const d = ds[i];
        let x;
        try { x = d.fn(); } catch (e) { report(e); x = null; }
        if (d.kid && !plainVal(x)) { ds.splice(i--, 1); bound(d.fn, d.apply, d.el, d.name, d.old); continue; }
        d.apply(x);
      }
    } finally {
      Listener = l;
      Owner = o;
    }
    return null;
  }
}
rowHole = rowDirect;

// What follows only a row's item: no effect, but a record the item runs
// when it changes (see `Item`). Shown markup, which could hold effects of
// its own, makes it an effect after all.
class Direct {
  constructor(fn, apply, el, name, old, kid) {
    this.fn = fn;
    this.apply = apply;
    this.el = el;
    this.name = name;
    this.old = old;
    this.kid = kid;
  }
}

const plainVal = (x) => x === null || x === undefined || (typeof x !== "object" && typeof x !== "function");

function rowDirect(row, fn, apply, el, name, old, kid) {
  const l = Listener;
  Listener = null;
  let x;
  try { x = fn(); } catch (e) { report(e); } finally { Listener = l; }
  if (kid && !plainVal(x)) { bound(fn, apply, el, name, old); return; }
  const d = new Direct(fn, apply, el, name, old, kid);
  Listener = null;
  try { d.apply(x); } finally { Listener = l; }
  row.direct = row.direct ? row.direct.concat([d]) : [d];
}

function newRow(x, k, row, c, out) {
  const o = Owner, l = Listener;
  Owner = null;
  Listener = null;
  const sc = new Scope();
  const r = { key: k, item: new Item(x, sc), scope: sc, nodes: null };
  Owner = r.scope;
  const got = [], made = partsMade;
  try { put(part(row(r.item)), c, got); } finally { Owner = o; Listener = l; }
  for (const n of got) out.push(n);
  r.nodes = own(got, made);
  return r;
}

// The first node of a part's nodes (see `nodesOf`), without listing them.
function firstOf(list) {
  for (const n of list) {
    if (n.__part) { const f = firstOf(n.__part.nodes); if (f) return f; }
    return n;
  }
  return null;
}

function dropRow(r) {
  removeNodes(r.nodes);
  dispose(r.scope);
}

// Every row gone: the whole parent emptied at once when they are all it holds.
function clearRows(st, p) {
  const old = st.rows, end = st.end;
  const first = old.length ? firstOf(old[0].nodes) : null;
  // Taken off with no handler hearing it (see `fire`; FEEDBACK61.md, Q-896; branch feedback61).
  removing++;
  try {
    if (first && p.firstChild === first && p.lastChild === end) {
      p.textContent = "";
      p.appendChild(end);
    } else if (first && first.parentNode === p) {
      const range = document.createRange();
      range.setStartBefore(first);
      range.setEndBefore(end);
      range.deleteContents();
    } else {
      // A row that changed its element after it was made named the one it began with:
      // each is taken out by its own record (FEEDBACK28.md, Q-342; branch feedback29-fixes).
      // Records name a row's nodes as they are now (see `own`), so this is for a first node
      // that something else took away (FEEDBACK61.md, Q-889; branch feedback61).
      for (const r of old) removeNodes(r.nodes);
    }
  } finally {
    removing--;
  }
  for (const r of old) dispose(r.scope);
  st.rows = [];
}

// A move that keeps what a node holds, such as a pointer's capture, where the browser can
// (FEEDBACK28.md, Q-343; branch feedback29-fixes).
function moveNode(p, n, before) {
  if (p.moveBefore && n.isConnected && p.isConnected) {
    try { p.moveBefore(n, before); return; } catch (e) { /* a move it cannot make */ }
  }
  p.insertBefore(n, before);
}

function reconcile(st, items, key, row, svg) {
  const end = st.end, p = end.parentNode, n = items.length;
  if (!p) return;
  if (n === 0) { clearRows(st, p); return; }
  let old = st.rows, w = old.length;
  const keys = new Array(n);
  let same = n === w;
  for (let i = 0; i < n; i++) {
    const k = key ? key(items[i]) : i;
    keys[i] = k;
    if (same && old[i].key !== k) same = false;
  }
  // Every row where it was (some items changed): nothing moves.
  if (same) {
    for (let i = 0; i < n; i++) { const r = old[i]; if (r.item._v !== items[i]) r.item.set(items[i]); }
    return;
  }
  // Each new row's old one: at the same place, or by key.
  const from = new Int32Array(n).fill(-1), used = new Uint8Array(w);
  let left = 0;
  for (let i = 0; i < n; i++) {
    if (i < w && old[i].key === keys[i]) { from[i] = i; used[i] = 1; } else left++;
  }
  if (left > 0 && w > 0) {
    const byKey = new Map();
    for (let j = w - 1; j >= 0; j--) if (!used[j]) byKey.set(old[j].key, j);
    for (let i = 0; i < n; i++) {
      if (from[i] >= 0) continue;
      const j = byKey.get(keys[i]);
      if (j !== undefined && !used[j]) { from[i] = j; used[j] = 1; byKey.delete(keys[i]); left--; }
    }
  }
  // Rows that come or go: a select of them shows its value again (see `selFollow`); rows that
  // only move or change keep what a person picked (FEEDBACK55.md, Q-758; branch feedback55).
  const came = left > 0 || w + left > n;
  // None kept: the old rows go at once.
  if (left === n && w > 0) { clearRows(st, p); old = []; w = 0; }
  for (let j = 0; j < w; j++) if (!used[j]) dropRow(old[j]);
  // The kept rows already in order stay; the others move. New rows go in
  // together, as a fragment.
  let moved = false, last = -1;
  for (let i = 0; i < n; i++) { const j = from[i]; if (j < 0) continue; if (j < last) moved = true; else last = j; }
  const stay = moved ? inOrder(from) : null;
  // Without `moveBefore` (Safari), an element moved loses the focus: given back (FEEDBACK55.md,
  // Q-766; branch feedback55).
  const back = moved ? focusBack(p) : null;
  const scrolls = moved && !p.moveBefore && scrolled.length ? scrollBack(p, old, from, stay) : null;
  const rows = new Array(n);
  // `after`, what a row goes in front of: the first node of the rows after it. It is asked of the
  // row (`ahead`) only when a row moves or new ones go in, as a row whose root is a branch answers
  // through its branch (FEEDBACK61.md, Q-889; branch feedback61).
  let after = end, ahead = null, pend = null;
  for (let i = n - 1; i >= 0; i--) {
    const j = w ? from[i] : -1;
    let r;
    if (j < 0) {
      const got = [];
      r = newRow(items[i], keys[i], row, { claim: false, svg }, got);
      if (!pend) pend = document.createDocumentFragment();
      for (let k = got.length - 1; k >= 0; k--) pend.insertBefore(got[k], pend.firstChild);
    } else {
      if (pend) {
        if (ahead) { after = firstOf(ahead.nodes); ahead = null; }
        const f = pend.firstChild;
        p.insertBefore(pend, after);
        if (f) after = f;
        pend = null;
      }
      r = old[j];
      r.key = keys[i];
      if (r.item._v !== items[i]) r.item.set(items[i]);
      if (moved && !stay[i]) {
        if (ahead) { after = firstOf(ahead.nodes); ahead = null; }
        const ns = nodesOf(r.nodes);
        // A row of several nodes moves a node at a time, as one of one node does: through a
        // fragment, they left the page, and lost their scroll and reloaded their frames
        // (FEEDBACK61.md, Q-894; branch feedback61).
        if (ns.length === 1) moveNode(p, ns[0], after);
        else for (const x of ns) moveNode(p, x, after);
      }
      // A row of no nodes has no first: the one after it stays `after`.
      if (r.nodes.length) ahead = r;
    }
    rows[i] = r;
  }
  if (pend) {
    if (ahead) after = firstOf(ahead.nodes);
    p.insertBefore(pend, after);
  }
  st.rows = rows;
  if (back) back();
  if (scrolls) scrolls();
  if (came) selFollow(p);
}

// The focused element in `p`, and what is selected in it: what puts them back after a move, when
// the move took the focus away.
function focusBack(p) {
  const a = document.activeElement;
  if (!a || !p.contains(a)) return null;
  const sel = typeof a.selectionStart === "number" ? [a.selectionStart, a.selectionEnd, a.selectionDirection] : null;
  return () => {
    if (document.activeElement === a || !a.isConnected) return;
    a.focus({ preventScroll: true });
    if (sel) a.setSelectionRange(sel[0], sel[1], sel[2]);
  };
}

// Elements scrolled on the page, where the browser has no `moveBefore` (Safari): there an element
// that moves is scrolled back to its start, and `scrollBack` puts back what a move of rows took
// (FEEDBACK61.md, Q-894; branch feedback61). Heard as they scroll, so a move costs nothing more
// where nothing was scrolled, and held weakly: one that leaves the page goes as it would without
// them (they were held, and up to 63 of them stayed alive with all they held: found in review).
// (The listener is in this declaration's lines, so that it comes with the lists that use it.)
const scrolled = [], scrollKnown = new WeakSet();
if (typeof document !== "undefined" && typeof WeakRef === "function" && !Element.prototype.moveBefore) document.addEventListener("scroll", scrollSeen, { capture: true, passive: true });
let scrollKept = 0;
function scrollSeen(e) {
  const t = e.target;
  if (t.nodeType !== 1 || scrollKnown.has(t)) return;
  scrollKnown.add(t);
  scrolled.push(new WeakRef(t));
  // Those gone or off the page leave the list each time it has doubled.
  if (scrolled.length > 2 * scrollKept + 16) {
    let k = 0;
    for (const w of scrolled) {
      const x = w.deref();
      if (x && x.isConnected) scrolled[k++] = w;
      else if (x) scrollKnown.delete(x);
    }
    scrolled.length = k;
    scrollKept = k;
  }
}

// Where the scrolled elements in the rows of `p` that move (`old[from[i]]` where `stay[i]` is not
// set) are scrolled to, and what scrolls them there again. Not `p`'s own scroll, nor a staying
// row's, which a move leaves alone: reading them laid the page out before and after every move
// of a list in a scrolled box (found in review).
function scrollBack(p, old, from, stay) {
  let moving = null, saved = null;
  for (const w of scrolled) {
    const e = w.deref();
    if (!e || e === p || !p.contains(e)) continue;
    if (!moving) {
      moving = new Set();
      for (let i = 0; i < from.length; i++) if (from[i] >= 0 && !stay[i]) for (const n of nodesOf(old[from[i]].nodes)) moving.add(n);
    }
    let top = e;
    while (top.parentNode !== p) top = top.parentNode;
    if (moving.has(top) && (e.scrollTop || e.scrollLeft)) (saved || (saved = [])).push(e, e.scrollTop, e.scrollLeft);
  }
  return saved && (() => {
    for (let i = 0; i < saved.length; i += 3) {
      saved[i].scrollTop = saved[i + 1];
      saved[i].scrollLeft = saved[i + 2];
    }
  });
}

// The positions of a longest increasing run of `a`'s values (skipping -1).
function inOrder(a) {
  const n = a.length, prev = new Int32Array(n), tails = [];
  for (let i = 0; i < n; i++) {
    const v = a[i];
    if (v < 0) continue;
    let lo = 0, hi = tails.length;
    while (lo < hi) { const mid = (lo + hi) >> 1; if (a[tails[mid]] < v) lo = mid + 1; else hi = mid; }
    prev[i] = lo > 0 ? tails[lo - 1] : -1;
    tails[lo] = i;
  }
  const keep = new Uint8Array(n);
  for (let i = tails.length ? tails[tails.length - 1] : -1; i >= 0; i = prev[i]) keep[i] = 1;
  return keep;
}

// ---- components ------------------------------------------------------------
// `<Card .../>`: a component, a view, or a function. A component's and a
// view's code runs once, with its props as getters, so what it shows
// follows the parent's values without the parent running again. A plain
// function returning markup is called again when its arguments change.

function comp(def, props, kids, spreads) {
  if (typeof def !== "function") fail(`<${def}> is not a view`);
  const ps = def.$p || [];
  for (const k of Object.keys(props)) if (!ps.includes(k)) fail(`<${def.$name}> has no parameter \`${k}\``);
  let p = props;
  if (spreads) p = withSpreads(props, ps, def, spreads);
  if (ps.includes("children") && !hasOwn(p, "children")) p.children = kids;
  return new Html((c, out) => {
    if (def.$comp) { island(def, p, takeHydration(def.$name), c, out); return; }
    if (def.$view) {
      const s = new Scope();
      within(s, () => put(part(def.$view(p)), c, out));
      return;
    }
    dyn(() => def(...ps.map((k) => p[k])), c, out);
  });
}

// Props with `{...map}` spreads: what is not written out comes from the maps.
function withSpreads(props, ps, def, spreads) {
  const o = {};
  for (const k of Object.keys(props)) Object.defineProperty(o, k, Object.getOwnPropertyDescriptor(props, k));
  for (const k of ps) {
    if (k === "children" || k === "key" || hasOwn(o, k)) continue;
    Object.defineProperty(o, k, {
      enumerable: true,
      get() {
        for (const m of spreads()) {
          if (isNil(m)) continue;
          if (!isMap(m)) fail(`\`{...}\` in <${def.$name}> needs a map, got ${typeName(m)}`);
          if (hasOwn(m, k)) return m[k];
        }
        return undefined;
      },
    });
  }
  return o;
}

// A parameter's default when the prop is not given.
function withDefaults(p, defaults) {
  const o = {};
  for (const k of Object.keys(p)) if (!hasOwn(defaults, k)) Object.defineProperty(o, k, Object.getOwnPropertyDescriptor(p, k));
  for (const k of Object.keys(defaults)) {
    const src = Object.getOwnPropertyDescriptor(p, k);
    Object.defineProperty(o, k, {
      enumerable: true,
      get() { const v = src ? (src.get ? src.get.call(p) : src.value) : undefined; return v === undefined ? defaults[k]() : v; },
    });
  }
  return o;
}

class Inst extends Scope {
  constructor(def, props, state, liveInfo) {
    super();
    this.def = def;
    this.props = heldProps(props, this);
    this.s0 = state || {};
    this.liveInfo = liveInfo || null;
    this.el = null;
    // Its id from the start, for the components inside it to name it as theirs.
    if (TOOLS) TOOLS.ids.set(this, TOOLS.next++);
  }

  // `state name = init`: what the server rendered with, or init's value.
  // Asked again (a body made again whole), the same state.
  // `page`: the initializer reads the page (`js(...)`), which it could not
  // do while the server rendered the component: resumed, the state starts
  // as the server had it and is set from the initializer run here, once the
  // component is in place (FEEDBACK18.md, Q-105; branch feedback18-fixes).
  sig(name, init, page) {
    const had = this.sigs || (this.sigs = {});
    if (had[name]) return had[name];
    const s = this.made(name, init, page);
    return (had[name] = TOOLS ? toolsSig(this, name, s) : s);
  }

  made(name, init, page) {
    if (hasOwn(this.s0, name)) {
      // `{"$quillo_prop": "initial"}`: the prop's own value, written once (FEEDBACK30.md, Q-371;
      // branch feedback27-35-fixes). Read as the server's mark only, not from a map the program
      // made of the same shape (FEEDBACK55.md, Q-875's review; branch feedback55).
      let v = this.s0[name];
      if (v instanceof PropOf) v = this.props[v.name];
      const s = new Sig(v);
      // Set by the page before this comes, it keeps what the page set (Q-800).
      if (page) { const n = W.claim(s); Promise.resolve().then(init).then((x) => W.latest(s, n, x), report); }
      return s;
    }
    const v = init();
    if (v instanceof Promise) {
      const s = new Sig(null), n = W.claim(s);
      v.then((x) => W.latest(s, n, x), report);
      return s;
    }
    return new Sig(v);
  }
}

// A component's props that follow its parent's values, each as the component last read it once
// it has left the page: what runs then (what `@mount` returned, a handler still waiting) reads
// what it was made with, not the values its parent has moved on to (FEEDBACK55.md, Q-762; branch
// feedback55).
function heldProps(p, inst) {
  const ks = Object.keys(p);
  if (!ks.some((k) => Object.getOwnPropertyDescriptor(p, k).get)) return p;
  const o = {};
  for (const k of ks) {
    const d = Object.getOwnPropertyDescriptor(p, k), get = d.get;
    let last, seen = false;
    Object.defineProperty(o, k, get ? { enumerable: true, get() { if (!inst.dead || !seen) { last = get.call(p); seen = true; } return last; } } : d);
  }
  return o;
}

// Islands on the page: <quillo-island>, or, inside a table or a select
// (where the browser's parser moves other elements out), the component's
// own root element, marked data-qi.
const ISLANDS = "quillo-island,[data-qi]";
const ownRoot = (el) => el.localName !== "quillo-island";
// The parts src/island.rs lists (a text node's localName is undefined).
const TABLE_PART = /^(t[rdh]|thead|tbody|tfoot|caption|colgroup|option|optgroup)$/;
// A <quillo-island> the page makes, which stands anywhere.
function wrapper(name) {
  const w = document.createElement("quillo-island");
  w.style.display = "contents";
  w.dataset.c = name;
  w.__q = true;
  return w;
}
// A component the page made in wrapper `w`: when its markup is one table
// or select part, that element becomes the island, as the server writes it
// (src/island.rs), so a row is a row of its table (`tbody.rows`, CSS). In
// <svg> (`svg`), any one element, as SVG draws only its own elements, not a
// <quillo-island> (FEEDBACK55.md, Q-763; branch feedback55).
function asOwnRoot(w, inst, svg) {
  const r = w.firstChild;
  if (!r || r !== w.lastChild || !(TABLE_PART.test(r.localName) || svg && r.nodeType === 1)) return w;
  // Its markup's own island attributes left out, as the server leaves them out (FEEDBACK55.md,
  // Q-700; branch feedback55).
  for (const a of ISLAND_ATTRS) r.removeAttribute(a);
  r.setAttribute("data-qi", "");
  r.dataset.c = w.dataset.c;
  r.__q = true;
  inst.el = r;
  return r;
}

// A component made again because the server's HTML was not what it makes:
// counted, so a page (or a test) can tell, and said in development, with
// its name and what differed.
function remade(name) {
  QW.remade = (QW.remade || 0) + 1;
  if (DEV && wanted) console.warn(`quillo: <${name}> was made again, not taken up from the server's HTML: where it wrote ${onPage(wanted[0])}, the browser makes ${wanted[1]}`); // dev
  if (TOOLS) toolsSay("remade", Object.assign({ name }, toolsWanted()));
}

// A component in its <quillo-island>: made, or taking up the page's; its nodes into `out`.
function island(def, props, h, c, out) {
  let inst = new Inst(def, props, h && h.s, h && h.l);
  let el;
  if (c.claim) {
    el = c.n;
    if (DEV) wanted = [el, `<${def.$name}>`]; // dev
    if (!el || el.nodeType !== 1 || !el.matches(ISLANDS) || el.dataset.c !== def.$name) throw new Mismatch("component");
    el.__q = true;
    c.n = el.nextSibling;
  } else el = wrapper(def.$name);
  inst.el = el;
  if (!c.claim) {
    run(inst, el, false, c.svg);
    if (TOOLS) toolsMade(inst);
    out.push(asOwnRoot(el, inst, c.svg));
    return;
  }
  // One inside another that cannot be taken up is made again by itself, and
  // said so, as one at the top is; before, its outer one reported a bare
  // "quillo: text" (FEEDBACK2.md, Q-014; branch feedback6-fixes). A table's
  // row is its own root, which only its outer one can make again.
  let got;
  try {
    got = run(inst, el, true);
  } catch (e) {
    if (!(e instanceof Mismatch) || ownRoot(el)) throw e;
    remade(def.$name);
    dispose(inst);
    // What it took up goes, heard by no handler it was given meanwhile (see `removing`;
    // FEEDBACK61.md, Q-896; branch feedback61).
    removing++;
    try { el.replaceChildren(); } finally { removing--; }
    inst = new Inst(def, props, h && h.s, h && h.l);
    inst.el = el;
    run(inst, el, false);
  }
  if (TOOLS) toolsMade(inst);
  if (got) for (const n of got) out.push(n);
  else out.push(el);
}

// The component's code, once; then its markup put in its island (in <svg>
// when `svg`, as SVG's).
function run(inst, el, claim, svg) {
  return within(inst, () => {
    let view;
    try {
      view = part(inst.def(inst, inst.props));
    } catch (e) {
      if (e instanceof Ret) view = e.v;
      else { report(e); return; }
    }
    if (claim && ownRoot(el)) {
      // The island is the markup's root: taken up from where it stands, in <svg> as SVG's
      // (FEEDBACK55.md, Q-763; branch feedback55).
      // What it took up are its nodes, not the element alone: a branch at its root replaces that
      // element later, and a row of a list held on to the old one, which a move brought back
      // (FEEDBACK61.md, Q-889; branch feedback61).
      const after = el.nextSibling;
      const c = { p: el.parentNode, n: el, claim: true, svg: inSvg(el.parentNode) };
      const got = [];
      put(view, c, got);
      if (DEV) wanted = [c.n, "nothing"]; // dev
      if (c.n !== after) throw new Mismatch("extra");
      return got;
    } else if (claim) {
      const c = { p: el, n: el.firstChild, claim: true, svg: false };
      put(view, c, []);
      rest(el, c.n);
    } else {
      const got = [];
      put(view, { claim: false, svg: !!svg }, got);
      el.append(...got);
    }
  });
}

// Island JSON (and a server fn's answer), with error values
// (`{"$quillo_err": x}`), decimals (`{"$dec": "12.50"}`) and large ints
// (`{"$int": "9007199254740993"}`) restored.
// Untagged once read in order: a map under a tag (`{"$": [key, map]}`) lost its order when the
// reviver untagged it first (FEEDBACK55.md, Q-875's second review; branch feedback55).
function islandJSON(text, fallback) {
  if (!text) return fallback;
  return untagDeep(parseData(text));
}
function untag(v) {
  if (!v || typeof v !== "object" || Array.isArray(v)) return v;
  const ks = Object.keys(v);
  if (ks.length !== 1) return v;
  // A map of one key that starts with `$`, which the server writes `{"$": [key, value]}` so that
  // it is not taken for one of these: a visitor's key made an error value, an int or a decimal of
  // it (FEEDBACK55.md, Q-875's review; branch feedback55).
  if (ks[0] === "$" && Array.isArray(v.$) && v.$.length === 2 && typeof v.$[0] === "string") return putKey({}, v.$[0], v.$[1]);
  // The server's mark for a state that is a prop's own value (see `made`).
  if (ks[0] === "$quillo_prop" && typeof v.$quillo_prop === "string") return new PropOf(v.$quillo_prop);
  if (ks[0] === "$quillo_err") return new Err(v.$quillo_err, true);
  if (ks[0] === "$dec" && typeof v.$dec === "string") return decParse(v.$dec, false) ?? v.$dec;
  if (ks[0] === "$int" && typeof v.$int === "string" && /^-?\d{1,19}$/.test(v.$int)) return intOf(BigInt(v.$int));
  return v;
}
// A state the server wrote as a prop's own value.
class PropOf { constructor(name) { this.name = name; } }
// `untag` throughout JSON already parsed, from the inside out.
function untagDeep(v) {
  if (v === null || typeof v !== "object") return v;
  if (Array.isArray(v)) { for (let i = 0; i < v.length; i++) v[i] = untagDeep(v[i]); return v; }
  for (const k of Object.keys(v)) {
    const x = v[k], y = untagDeep(x);
    if (y !== x) Object.defineProperty(v, k, { value: y, enumerable: true, writable: true, configurable: true });
  }
  return untag(v);
}

let hydrating = null;

function takeHydration(name) {
  const q = hydrating && hydrating[name];
  return q && q.length ? q.shift() : null;
}

// What each file of the program's brings in development: its compiled lines'
// marks, so that an error can say where it happened (see `placed`).
const PLACES = {}; // dev
// An error as the server's log has it: the file and line it happened on, and the
// calls it came through, read from its stack. The console's link to the stack goes
// through the source maps as well, for those who click (FEEDBACK21.md, Q-173; branch
// feedback20-fixes).
const placed = (e) => { // dev
  const calls = []; // dev
  for (const l of String(e.stack).split("\n")) { // dev
    const m = l.match(/\/_quillo\/src\/\w+\/(.+?):(\d+):(\d+)\)?$/); // dev
    const f = m && PLACES[m[1]]; // dev
    if (!f) continue; // dev
    let n = +m[2]; // dev
    while (n > 0 && !f[1][n]) n--; // dev
    if (!n) continue; // dev
    const marks = f[1][n]; // dev
    const mark = (n === +m[2] && marks.filter((x) => x[0] < +m[3]).pop()) || marks[marks.length - 1]; // dev
    calls.push([decodeURIComponent(m[1].split("/").pop().slice(0, -3)), `${f[0]}:${mark[1]}`]); // dev
  } // dev
  if (!calls.length) return e.message; // dev
  const chain = calls.slice(0, -1).map((c, i) => `\n  in ${c[0]}, called at ${calls[i + 1][1]}`); // dev
  return `${calls[0][1]}: ${e.message}${chain.join("")}`; // dev
}; // dev

function report(e) {
  const said = ["quillo:", e instanceof Error ? e.message : e];
  if (DEV && e instanceof Error) said.splice(1, 1, placed(e), e); // dev
  console.error(...said);
  if (typeof window !== "undefined" && window.__quilloErrors) window.__quilloErrors.push(String(e && e.message || e));
  if (TOOLS) toolsSay("error", { message: String(e && e.message || e), place: DEV && e instanceof Error ? toolsPlace(e) : null, id: toolsOwner() });
}

// ---- resuming: the server's HTML, taken up as it is ----------------------
//
// A component's HTML from the server is not made again: its code runs, and
// its markup takes up the page's nodes (text the browser joined is split
// where the code has several texts, empty ones are added), handlers are
// attached, and from then on each part follows what it reads. Components
// are taken up when someone first reaches for them; those that run by
// themselves, or are the page, as soon as the bundle is in.

// The outermost island a node is in.
function outerIsland(node) {
  let i = node && node.nodeType === 1 && node.closest ? node.closest(ISLANDS) : null;
  for (let up = i && i.parentElement && i.parentElement.closest(ISLANDS); up; up = up.parentElement && up.parentElement.closest(ISLANDS)) i = up;
  return i;
}

// ---- what a developer tool hears (FEEDBACK42.md, Part C; branch feedback40-43) ----
//
// Off until a tool asks, by `<html data-quillo-devtools="1">` before the runtime starts
// (an extension's content script at document_start). Then the runtime says what happens
// as `quillo:devtools` events on `document`, each `detail` a JSON string (so it reaches an
// extension's isolated world), values as the wire writes them (`$dec`, `$int`,
// `$quillo_err`); a tool asks with `quillo:devtools-ask` (`{ id, op, ... }`) and is
// answered `{ type: "answer", id, ok }`. No name goes on `window`. With no tool, each hook
// is one test of `TOOLS`, or is not put in place at all (a state's `set`).
let TOOLS = null;
let VERSION = ""; // Quillo's version: src/client.rs sets it after the runtime, as BASE

function toolsStart() {
  if (document.documentElement.getAttribute("data-quillo-devtools") !== "1") return;
  TOOLS = { ids: new WeakMap(), all: new Map(), next: 1, marked: null };
  document.addEventListener("quillo:devtools-ask", toolsAsk);
  toolsSay("hello", { protocol: 1, version: VERSION, dev: DEV, base: BASE });
}

// A map's keys in the order set, as `toolsWire` gives them (FEEDBACK55.md, Q-875; branch feedback55).
function toolsSay(type, o) {
  document.dispatchEvent(new CustomEvent("quillo:devtools", { detail: writeJSON(Object.assign({ type }, o)) }));
}

// A value as the wire writes it; what cannot be written (a page's object) as its text.
function toolsWire(v) {
  if (v instanceof Err) return { $quillo_err: toolsWire(v.error) };
  try { return toJSON(v, "tools"); } catch (_) { return String(v); }
}

// The component the code running now belongs to, by its id.
function toolsOwner() {
  for (let s = Owner; s; s = s.up) if (s instanceof Inst) return TOOLS.ids.get(s) || null;
  return null;
}

// Where an error is in the program, in development (the `.qwe` line); and what a component
// made again was given and wanted (see `remade`).
let toolsPlace = () => null;
let toolsWanted = () => ({});
toolsPlace = (e) => placed(e); // dev
toolsWanted = () => (wanted ? { wrote: onPage(wanted[0]), makes: wanted[1] } : {}); // dev

// A component as a tool sees it: its name, the one it is in, its props and its states.
function toolsOf(inst) {
  let parent = null;
  for (let s = inst.up; s; s = s.up) if (s instanceof Inst) { parent = TOOLS.ids.get(s) || null; break; }
  return untrack(() => {
    const props = {}, state = {};
    for (const k of Object.keys(inst.props || {})) { try { props[k] = toolsWire(inst.props[k]); } catch (e) { props[k] = { $quillo_err: String(e && e.message || e) }; } }
    for (const k of Object.keys(inst.sigs || {})) state[k] = toolsWire(inst.sigs[k]._v);
    return { id: TOOLS.ids.get(inst), name: inst.def.$name, parent, props, state };
  });
}

// Said once it is in place, its states made; one made again in its stead was never said.
function toolsMade(inst) {
  TOOLS.all.set(TOOLS.ids.get(inst), inst);
  toolsSay("component", toolsOf(inst));
}

function toolsGone(inst) {
  const id = TOOLS.ids.get(inst);
  if (TOOLS.all.delete(id)) toolsSay("gone", { id });
}

// A state that says when it is set; only the states of a page a tool looks at.
function toolsSig(inst, name, s) {
  s.set = function (v) {
    const was = this._v;
    Sig.prototype.set.call(this, v);
    if (v !== was && TOOLS.all.get(TOOLS.ids.get(inst)) === inst) toolsSay("state", { id: TOOLS.ids.get(inst), name, value: toolsWire(v) });
    return null;
  };
  return s;
}

// `list`: every component now; `where`: marks one's element for the tool to find (the mark
// moves with the next `where`); `set`: sets a state, under --watch only.
function toolsAsk(e) {
  let q;
  // A map set from the tool keeps the order the tool wrote (FEEDBACK55.md, Q-875; branch feedback55).
  try { q = readJSON(e.detail); } catch (_) { return; }
  if (!q || typeof q !== "object") return;
  const answer = (ok, more) => toolsSay("answer", Object.assign({ id: q.id === undefined ? null : q.id, ok }, more));
  const inst = TOOLS.all.get(q.target);
  if (q.op === "list") {
    for (const i of TOOLS.all.values()) toolsSay("component", toolsOf(i));
    return answer(true);
  }
  if (q.op === "where") {
    if (TOOLS.marked) TOOLS.marked.removeAttribute("data-quillo-devtools-id");
    TOOLS.marked = inst && inst.el;
    if (!TOOLS.marked) return answer(false, { error: "no such component" });
    TOOLS.marked.setAttribute("data-quillo-devtools-id", String(q.target));
    return answer(true);
  }
  if (q.op === "set") {
    if (!DEV) return answer(false, { error: "states are set from a tool under --watch only" });
    const sig = inst && inst.sigs && hasOwn(inst.sigs, q.name) ? inst.sigs[q.name] : null;
    if (!sig) return answer(false, { error: "no such state" });
    sig.set(untagDeep(q.value));
    return answer(true);
  }
  answer(false, { error: "unknown op: " + q.op });
}

// An island taken up: its component (and those inside it) resumed from
// the state the server wrote, on the HTML it wrote.
function resume(el) {
  // Still waiting for its deferred state, which the server sends after the page
  // and swaps in (src/defer.rs; branch defer): taken up once it has come.
  if (el.__q || (el.hasAttribute && el.hasAttribute("data-qd"))) return;
  el.__q = true;
  const def = C[el.dataset.c];
  if (!def) { report(`unknown component <${el.dataset.c}>`); return; }
  const queue = () => {
    const q = {};
    for (const inner of el.querySelectorAll(ISLANDS)) {
      (q[inner.dataset.c] = q[inner.dataset.c] || []).push({ s: islandJSON(inner.dataset.s, {}), l: islandJSON(inner.dataset.l, null) });
    }
    return q;
  };
  for (const inner of el.querySelectorAll(ISLANDS)) liveSeen(...(inner.dataset.v || "").split(":"));
  liveSeen(...(el.dataset.v || "").split(":"));
  const props = islandJSON(el.dataset.p, {});
  // A top island's children come as the HTML the server made of them.
  if (typeof props.children === "string") props.children = rawHtml(props.children);
  const state = islandJSON(el.dataset.s, {}), liveInfo = islandJSON(el.dataset.l, null);
  hydrating = queue();
  let inst = new Inst(def, props, state, liveInfo);
  inst.el = el;
  let ok = true;
  try {
    run(inst, el, true);
    QW.resumed = (QW.resumed || 0) + 1;
  } catch (e) {
    if (e instanceof Mismatch) remade(el.dataset.c);
    else {
      report(e);
      QW.remade = (QW.remade || 0) + 1;
    }
    ok = false;
  }
  if (!ok) {
    dispose(inst);
    hydrating = queue();
    // What it took up goes, heard by no handler it was given meanwhile (see `removing`;
    // FEEDBACK61.md, Q-896; branch feedback61).
    removing++;
    try {
      if (ownRoot(el)) {
        const w = wrapper(el.dataset.c);
        el.replaceWith(w);
        el = w;
      } else el.replaceChildren();
    } finally {
      removing--;
    }
    inst = new Inst(def, props, state, liveInfo);
    inst.el = el;
    const svg = inSvg(el.parentNode || el);
    run(inst, el, false, svg);
    const r = asOwnRoot(el, inst, svg);
    if (r !== el) { el.replaceWith(r); el = r; }
  }
  hydrating = null;
  if (TOOLS) toolsMade(inst);
  // What was typed before it was taken up: the component hears it now.
  for (const f of el.querySelectorAll("input, textarea, select")) {
    if (!asParsed(f)) {
      f.dispatchEvent(new Event("input", { bubbles: true }));
      f.dispatchEvent(new Event("change", { bubbles: true }));
    }
  }
}

// Whether a field still shows what the page gave it, as the browser made it:
// a range's value snapped to its step or into min..max, a select with no
// option chosen on its first one. Only a field that does not was typed in
// (or restored by the browser) before its component was taken up
// (FEEDBACK5.md, Q-048; branch stays-fixes).
function asParsed(f) {
  if (f.type === "checkbox" || f.type === "radio") return f.checked === f.defaultChecked;
  if (f.tagName === "SELECT") {
    const opts = [...f.options];
    if (opts.every((o) => o.selected === o.defaultSelected)) return true;
    return !f.multiple && f.size <= 1 && !opts.some((o) => o.defaultSelected) && f.selectedIndex === 0;
  }
  if (f.value === f.defaultValue) return true;
  if (f.tagName !== "INPUT") return false;
  const p = document.createElement("input");
  for (const a of ["type", "min", "max", "step"]) if (f.hasAttribute(a)) p.setAttribute(a, f.getAttribute(a));
  p.setAttribute("value", f.defaultValue);
  return f.value === p.value;
}

function wake(e) {
  const i = outerIsland(e.target);
  if (i && !i.__q) resume(i);
}

function boot() {
  toolsStart();
  // A deferred island swapped in after the runtime came: taken up if it runs by itself.
  QW.resume = (el) => { if (el && !el.__q) resume(el); };
  // Someone reaching for an island takes it up (before their event is
  // handled: it reaches the component).
  for (const t of ["pointerover", "pointerdown", "touchstart", "focusin", "keydown", "click", "submit", "input"]) document.addEventListener(t, wake, true);
  const outer = [...document.querySelectorAll(ISLANDS)].filter((i) => !(i.parentElement && i.parentElement.closest(ISLANDS)));
  // An island runs by itself when one inside it does: the inner one starts
  // with its outer one (FEEDBACK5.md, Q-046; branch stays-fixes).
  const byItself = (i) => i.hasAttribute("data-e") || i.hasAttribute("data-l");
  for (const island of outer) {
    if (byItself(island) || island.parentElement === document.body || [...island.querySelectorAll(ISLANDS)].some(byItself)) resume(island);
  }
  // The island someone reached for, which brought the bundle, and the one
  // that has the focus.
  for (const n of [QW.at, document.activeElement]) {
    const i = n && n !== document.body ? outerIsland(n) : null;
    if (i) resume(i);
  }
  // What was done before the bundle came (the loader in server.rs kept it):
  // the same elements are on the page, so it is done again on them, each
  // mouse and pointer event as it was made, where and with the buttons and
  // keys it had (branch early-events).
  const held = QW.q || [];
  const redo = QW.redo;
  QW.booted = true;
  for (const r of held.splice(0)) {
    const i = outerIsland(r.el);
    if (i) resume(i);
    if (redo) redo(r);
  }
}

// ---- what compiled code uses -------------------------------------------

const G = {};
const C = {};
const W = {};
W.Err = Err;
W.waits = (v) => v instanceof Promise || (v !== null && typeof v === "object" && typeof v.then === "function" && foreign(v));
// A stop that `??` guards against, and anything else goes on.
W.guarded = (e) => { if (e instanceof QuilloError) return null; throw e; };
// `f(a, b, c)` on the left of `??`, a field or an index read, guarded so (FEEDBACK55.md, Q-782;
// branch feedback55).
W.tryq = (f, a, b, c) => { try { return f(a, b, c); } catch (e) { return W.guarded(e); } };
// What `js(...)` gave, waited for if it is a Promise (see `call` in
// src/client.rs): what it settles to, or its rejection as an error value.
W.settle = async (v) => {
  if (v === null || typeof v !== "object" || typeof v.then !== "function" || foreign(v) && !(v instanceof Promise)) return v;
  try { return fromPage(await v); } catch (e) { return new Err("js: " + (e && e.message || String(e)), true); }
};
// A run: what one entry from the runtime or the page does (an event's handler, an effect, a timer,
// a frame, a state's initializer), with every function it calls, across their waits. `Run` is the
// one going on, null outside any: code that does not wait has no run of its own, and each of its
// sets counts as another's. A function that waits joins the run it was called in, or starts one,
// and gives it back as it waits and as it ends, so `Run` is null again when nothing is left on the
// stack and whatever the page calls next starts afresh, with nothing to mark at each way in; after a
// wait it takes it up again. `w`: the run, what was going on when the function was called, and
// whether it has been given back. A run: `held`, the states it has claimed for a call (weakly: a run
// kept by a state of the page keeps no component that has gone); `up`, the run that called it, for
// `parallel`'s (FEEDBACK55.md, Q-800; branch feedback55).
let Run = null;
// What the code belongs to is taken up after a wait too: `w.a`, the part of the page it belongs to
// (`Owner`, or a handler's `Acting`), for what `every`, `run_after` and `listen` start then to end
// with it; `w.p`, what it was before, put back as the code waits again or ends. After a wait the
// code had none: `every` refused to start, and a `run_after` outlived its component
// (FEEDBACK61.md, Q-901; branch feedback61). It is the call's, not the run's, which a state of the
// page keeps. A run's `nav`: the page changes it has seen (see `B.navigate`; Q-902).
W.run = () => { const o = Run; return { r: (Run = o || { held: null, up: undefined, nav: Nav.n }), o, y: false, a: Owner || Acting, p: undefined }; };
W.off = (w, x) => { if (!w.y) { w.y = true; Run = w.o; w.o = null; if (w.p !== undefined) { Acting = w.p; w.p = undefined; } } return x; };
W.on = (w, v) => { w.y = false; Run = w.r; w.p = Acting; Acting = w.a; return v; };
// Whether `r`, or a run that called it, has claimed `s` for a call.
function claimedBy(s, r) {
  for (; r; r = r.up) if (r.held !== null && r.held.has(s)) return true;
  return false;
}
// The latest set of a state wins (see `Stmt::Assign` in src/client.rs): a set claims its state, and
// a function that waits claims, before the wait, a state it will set with the answer. That set is
// dropped when another run has claimed the state since, and changes no claim either way: `n` moves
// only when the claim changes hands, so a run's own sets, a function it called among them, take
// nothing from it. A call is always a claim of its own, whoever held the state: `v = "loading";
// parallel(f, g)` has `f`'s and `g`'s calls, and the later is the latest.
W.claim = (s) => {
  const r = Run;
  if (r !== null) (r.held || (r.held = new WeakSet())).add(s);
  if (r === null || s.r !== r) { s.r = r; s.n++; }
  return s.n;
};
// At a later wait with the same answer: the claim it has, lost or not.
W.again = (s, n) => (n === undefined ? W.claim(s) : n);
W.latest = (s, n, v) => { if (s.n === n) s.set(v); return null; };
// Any other set, a fold too: always made. It claims the state, so an older answer gives way to it,
// unless its run claimed the state and lost it, for the rest of the run: then it takes nothing, and
// the newest call's answer still lands. As it was, `let r = search(q); rows = r; rows = rows | f()`
// typed quickly voided the last call and ended empty.
W.put = (s, v) => {
  const r = Run;
  if (r === null) { s.r = null; s.n++; }
  else if (s.r !== r && !claimedBy(s, r)) { s.r = r; s.n++; }
  return s.set(v);
};
// A set with an answer: its claim's, or a plain one on a way that did not wait for it.
W.to = (s, v, n) => (n === undefined ? W.put(s, v) : W.latest(s, n, v));
W.Dec = Dec;
W.Fl = Fl;
W.Ret = Ret;
// The program's modules, `[name, export, address]` each, all in before the page
// starts. One that fails is said when its name is used, as for a script that did
// not load, and the rest of the page runs. A script with no exports at all is not
// a module, and says so (branch library-hints).
W.imports = (mods) => Promise.all(mods.map(([name, exp, url]) => import(url).then(
  (m) => { MODS.set(name, exp === "*" ? m : exp in m ? m[exp] : new Err(Object.keys(m).length ? `${name}: ${url} has no export \`${exp}\`` : `${name}: ${url} exports nothing: a plain script, so leave out \`with module\``, true)); },
  (e) => { MODS.set(name, new Err(`${name}: ${url} did not load: ${e && e.message || e}`, true)); },
)));
W.fail = fail;
W.truthy = truthy;
W.eq = eq;
W.is = isNow;
W.add = add;
W.sub = sub;
W.mul = mul;
W.div = div;
W.mod = mod;
W.lt = lt;
W.le = le;
W.gt = gt;
W.ge = ge;
W.range = intRange;
W.neg = neg;
W.field = field;
W.fieldp = fieldp;
W.Later = Later;
W.setf = setField;
W.index = index;
W.call = callFn;
W.most = most;
W.idiv = idiv;
W.divCall = divCall;
W.mcallDiv = mcallDiv;
W.divMethod = divMethod;
W.mcall = mcall;
W.type = (name, shape) => {
  OWN_TYPES[name] = shape;
  ownTypes = ownTypes || {
    is: (t, v) => !!OWN_TYPES[t] && ownFits(ownTyOf(t), v, false) === null,
    lacks: (v, names, opt) => ownBecause(ownFits({ n: names, o: opt, e: null, f: null, t: tyText(names, opt) }, v, false)),
    make: (t, v) => (OWN_TYPES[t] ? ownMake(ownTyOf(t), v) : null),
  };
};
W.tryv = tryv;
// `for` over `enumerate(xs)`, `range(a, b)` and `a..b`, by number: checked
// as those check, without making the list.
W.enumlist = (l) => L(l, "enumerate");
// A loop makes no list, so a list's limit means nothing here (FEEDBACK29.md, Q-356;
// branch feedback29-fixes).
W.span = (a, b, dots) => {
  if (!dots) { if (b === undefined) { b = II(a, "range"); a = 0; } else { a = II(a, "range"); b = II(b, "range", 2); } }
  // Past 2^53 counted as BigInts, which a number past it could not be counted by (FEEDBACK55.md,
  // Q-776; branch feedback55).
  if (typeof a === "bigint" || typeof b === "bigint") {
    if (!(isInt(a) || typeof a === "bigint") || !(isInt(b) || typeof b === "bigint")) fail("`..` needs two ints");
    return [BigInt(a), BigInt(b)];
  }
  if (!isInt(a) || !isInt(b)) fail("`..` needs two ints");
  return [a, b];
};
// `range(a, b, step)` (FEEDBACK29.md, Q-360; branch feedback29-fixes).
W.stepped = (a, b, c) => {
  let r = [II(a, "range"), II(b, "range", 2), II(c, "range", 3)];
  // Past 2^53 all three counted as BigInts (found in review).
  if (r.some((x) => typeof x === "bigint")) r = r.map(BigInt);
  if (r[2] == 0) fail("`range`: a step of 0 never gets anywhere");
  return r;
};
W.iter = iter;
W.ordered = ordered;
W.match = match;
W.arity = arity;
W.missing = missing;
W.argty = argty;
W.retty = retty;
W.meta = meta;
W.block = block;
W.t = tmpl;
W.frag = frag;
W.each = each;
W.choose = choose;
W.keyed = keyed;
W.c = comp;
W.memo = memo;
W.effect = effect;
W.props = withDefaults;
W.spread = spreadAttrs;
W.spreadcls = spreadClasses;
W.Html = Html;
W.display = display;
W.repr = repr;
W.short = short;
W.cls = (xs) => { const out = []; clsWords(xs, out); return out.join(" "); };
W.cls1 = (x) => (typeof x === "string" ? x : W.cls([x]));
W.nofn = (v) => (typeof v === "function" ? null : v);
// What a text field or a select bound to state `s` gives it: a number when the state is one and
// the text is that number written as Quillo writes it (null when empty, as a number input gives),
// so `qty + 1` after typing 12 is 13, not "121"; other text stays text (an option "02", "1e2",
// " 7"), which the field and the select show as they are. A state that held a number takes one
// again once the text is one again (FEEDBACK55.md, Q-760; branch feedback55).
// A number state gives a number, a float state a float: `state amount = 1.0` made "3" text, and
// `amount + 1` "31" (FEEDBACK55.md, Q-775; found in review). Which it is, the number it held when
// first typed into says: an int state that was given 1.5 still gives an int for -3.
W.typed = (s, t) => {
  const v = s.v;
  if (!NUMERIC.has(s) && isNumber(v)) { NUMERIC.add(s); if (isFloat(v)) FLOATS.add(s); }
  if (!NUMERIC.has(s) || typeof t !== "string") return t;
  if (t.trim() === "") return null;
  const n = Number(t);
  return Number.isFinite(n) && display(n) === t ? (FLOATS.has(s) ? fl(n) : n) : t;
};
const NUMERIC = new WeakSet();
const FLOATS = new WeakSet();
W.component = (name, params, render) => {
  render.$name = name;
  render.$p = params;
  render.$comp = true;
  C[name] = render;
  return render;
};
// A function's form for `<Name .../>`: its code once, with props as getters.
W.view = (f, view) => { f.$view = view; return f; };
// Whether a live state waits for the answer to its new arguments (or to its
// first, when the server did not render it): its old value is still shown
// meanwhile (FEEDBACK13.md, Q-091; branch feedback13-fixes).
const waitSig = (sig) => sig.$wait || (sig.$wait = new Sig(false));
W.waiting = (sig) => waitSig(sig).v;
// `state name = live(fname, ...args)`: state that follows the server. When
// its arguments change (`live(load, id)` after `id` changed), it loads again.
W.live = (inst, name, fname, args, every) => {
  const had0 = inst.sigs || (inst.sigs = {});
  if (had0[name]) return had0[name];
  return (had0[name] = within(inst, () => liveSig(inst, name, fname, args, every)));
};
function liveSig(inst, name, fname, args, every) {
  const info = inst.liveInfo && inst.liveInfo[name];
  const had = hasOwn(inst.s0, name);
  const sig = new Sig(had ? inst.s0[name] : null);
  // `fails`: loads that failed in a row; `retry`: the timer of the next try (see `liveRefresh`).
  const sub = { inst, sig, name, fname, every: every || 0, last: 0, args: null, topics: info ? info.t : [], h: had && info && info.h || "", base: had ? inst.s0[name] : undefined, fails: 0, retry: 0 };
  Live.subs.add(sub);
  onCleanup(() => { Live.subs.delete(sub); clearTimeout(sub.retry); liveRestart(); });
  let first = true;
  effect(() => {
    const a = args();
    untrack(() => {
      if (first) {
        first = false;
        sub.args = a;
        if (had) liveRestart(); else liveRefresh(sub, true);
        return;
      }
      if (eq(sub.args, a)) return;
      sub.args = a;
      liveRefresh(sub, true);
    });
  });
  return sig;
}
W.server = (name, idem) => meta((...args) => rpc(name, args, idem), name, null);
