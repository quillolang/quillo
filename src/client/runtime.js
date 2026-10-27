// Quillo browser runtime: values, the standard library, a small virtual DOM,
// components with state, and calls to `server fn`s. The compiled program is
// appended after this and registers its functions in G.
"use strict";

// Where the program is mounted ("" at the root); the bundle sets it.
let BASE = "";
const withBase = (u) => (BASE && typeof u === "string" && u[0] === "/" && u[1] !== "/" ? BASE + u : u);

// ---- values ------------------------------------------------------------

class QuilloError extends Error {}
class Ret { constructor(v) { this.v = v; } }
class Err { constructor(e) { this.error = e === undefined ? null : e; } }

const hasOwn = (o, k) => Object.prototype.hasOwnProperty.call(o, k);
// Puts a key in a map the program made: "__proto__" is a key like any
// other there (from JSON, say), not the object's prototype. Spreading
// (`{ ...a, ...b }`) keeps it too; Object.assign and `o[k] = v` do not.
const putKey = (o, k, v) => {
  if (k === "__proto__") Object.defineProperty(o, k, { value: v, enumerable: true, writable: true, configurable: true });
  else o[k] = v;
  return o;
};
const fail = (msg) => { throw new QuilloError(msg); };
const isNil = (v) => v === null || v === undefined;
const isEvent = (v) => typeof Event !== "undefined" && v instanceof Event;
const isMap = (v) => v !== null && typeof v === "object" && (v.constructor === Object || (!Array.isArray(v) && !(v instanceof Err) && !(v instanceof Html) && !(v instanceof Dec) && !isEvent(v)));
const isInt = (v) => typeof v === "number" && Number.isInteger(v);

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
const isFloatV = (v) => typeof v === "number" && !Number.isInteger(v);
const decHint = (a, b) => ((a instanceof Dec && isFloatV(b)) || (b instanceof Dec && isFloatV(a)) ? DEC_FLOAT : "");

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
const decFromFloat = (f) => (Number.isFinite(f) ? decParse(f.toExponential(), true) : null);
// A value as a decimal, for a `dec` pattern or record field.
function asDec(v) {
  if (v instanceof Dec) return v;
  if (isInt(v) || typeof v === "bigint") return new Dec(BigInt(v), 0);
  if (typeof v === "number") return decFromFloat(v);
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
// Arithmetic with an int past 2^53: exact, 64-bit as on the server.
function bigArith(a, b, op, verb) {
  const ai = isInt(a) || typeof a === "bigint", bi = isInt(b) || typeof b === "bigint";
  if (ai && bi) {
    const x = BigInt(a), y = BigInt(b);
    if ((op === "/" || op === "%") && y === 0n) fail("division by zero");
    if (op === "/") return x % y === 0n ? bigChecked(x / y) : Number(x) / Number(y);
    return bigChecked(op === "+" ? x + y : op === "-" ? x - y : op === "*" ? x * y : x % y);
  }
  if ((ai || typeof a === "number") && (bi || typeof b === "number")) {
    const x = Number(a), y = Number(b);
    return op === "+" ? x + y : op === "-" ? x - y : op === "*" ? x * y : op === "/" ? x / y : x % y;
  }
  fail(`cannot ${verb} ${typeName(a)} and ${typeName(b)}`);
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
  guardCall(`\`${name}\``, f, args, name);
  return fromPage(Reflect.apply(f, o, args));
}
// An object from the page's scripts (an element, a library's), not one the
// program made: its inherited properties and methods are its fields too.
const foreign = (v) => v !== null && typeof v === "object" && (v === globalThis || v.constructor !== Object && v.constructor !== undefined
  && !Array.isArray(v) && !(v instanceof Err) && !(v instanceof Html) && !(v instanceof Dec));
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
  return fromPage(f(...args));
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
}
// What is handed to the page's functions: no javascript: URL (in a list or
// a map too, which turn into it as text), no function above, no map that
// makes its own text, and no node of another document (markup parsed from
// text, whose handlers would run once it is on the page).
function guardArgs(what, args, depth) {
  for (const a of args) {
    if (typeof a === "string") {
      if (scriptURL(a)) fail(`${what} is not given a javascript: URL`);
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

function typeName(v) {
  if (isNil(v)) return "null";
  switch (typeof v) {
    case "boolean": return "bool";
    case "number": return Number.isInteger(v) ? "int" : "float";
    case "string": return "str";
    case "function": return "fn";
    case "bigint": return "int";
  }
  if (v instanceof Dec) return "dec";
  if (Array.isArray(v)) return "list";
  if (v instanceof Err) return "error";
  if (v instanceof Html) return "html";
  return "map";
}

function typeIs(t, v) {
  switch (t) {
    case "any": return true;
    case "int": return isInt(v) || typeof v === "bigint";
    // JS numbers do not remember whether they were written 2 or 2.0.
    case "float": return typeof v === "number";
    case "num": return typeof v === "number" || typeof v === "bigint";
    case "dec": return v instanceof Dec;
    case "str": return typeof v === "string";
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
      Object.defineProperty(out, k, { value: r.v, enumerable: true, writable: true, configurable: true });
    }
    return { ok: true, v: out };
  }
  if (n === "list" && Array.isArray(v) && t.e && ownHasRecord(t.e)) {
    const out = [];
    for (let i = 0; i < v.length; i++) {
      const r = ownMake(t.e, v[i], depth + 1);
      if (!r.ok) return { ok: false, w: ownWhyAt(`[${i}]`, ownWhyHere(r.w, t.e, v[i])) };
      out.push(r.v);
    }
    return { ok: true, v: out };
  }
  // A decimal field takes an int, numeric text or a JSON number.
  if (n === "dec") { const d = asDec(v); return d === null ? { ok: false, w: OWN_PLAIN } : { ok: true, v: d }; }
  const w = ownFits({ n: [n], o: false, e: t.e, f: t.f, t: t.t }, v, true, depth);
  return w ? { ok: false, w } : { ok: true, v };
}



const fmtKey = (k) => /^[A-Za-z_][A-Za-z0-9_]*$/.test(k) ? k : JSON.stringify(k);

function repr(v) {
  if (isNil(v)) return "null";
  if (typeof v === "string") return JSON.stringify(v);
  if (typeof v === "number") return String(v);
  if (typeof v === "boolean") return String(v);
  if (typeof v === "bigint") return String(v);
  if (v instanceof Dec) return decText(v) + "m";
  if (typeof v === "function") return "<fn " + (v.$name || "<fn>") + ">";
  if (Array.isArray(v)) return "[" + v.map(repr).join(", ") + "]";
  if (v instanceof Err) return "err(" + repr(v.error) + ")";
  if (v instanceof Html) return "<html>";
  if (isEvent(v)) return "<event " + v.type + ">";
  const ks = Object.keys(v);
  return ks.length ? "{ " + ks.map((k) => fmtKey(k) + ": " + repr(v[k])).join(", ") + " }" : "{}";
}

const display = (v) => typeof v === "string" ? v : typeof v === "number" ? String(v) : v instanceof Dec ? decText(v) : repr(v);
// A value as a map key (`set`, `get`, `group_by`): 12.5 and 12.50 are one.
const keyText = (v) => (v instanceof Dec ? decKey(v) : display(v));
const short = (v) => { const r = repr(v); return r.length > 40 ? r.slice(0, 40) + "…" : r; };
const truthy = (v) => !(isNil(v) || v === false || v instanceof Err);

function eq(a, b) {
  if (a === b) return true;
  // Different numbers, strings, bools and functions; null and undefined match.
  if (typeof a !== "object" || typeof b !== "object" || a === null || b === null) {
    // Two numbers (the usual case) test only their types here.
    return (typeof a === "object" || typeof b === "object") && (a instanceof Dec || b instanceof Dec) ? decEq(a, b) : isNil(a) && isNil(b);
  }
  if (a instanceof Dec || b instanceof Dec) return decEq(a, b);
  if (Array.isArray(a) && Array.isArray(b)) return a.length === b.length && a.every((x, i) => eq(x, b[i]));
  if (a instanceof Err && b instanceof Err) return eq(a.error, b.error);
  if (isMap(a) && isMap(b)) {
    const ka = Object.keys(a);
    return ka.length === Object.keys(b).length && ka.every((k) => hasOwn(b, k) && eq(a[k], b[k]));
  }
  return false;
}

// A decimal and a number, compared as decimals (a float through its text).
function decEq(a, b) {
  const x = a instanceof Dec ? a : typeof a === "number" || typeof a === "bigint" ? asDec(a) : null;
  const y = b instanceof Dec ? b : typeof b === "number" || typeof b === "bigint" ? asDec(b) : null;
  return x !== null && y !== null && decCmp(x, y) === 0;
}

function cmp(a, b) {
  if ((typeof a === "number" && typeof b === "number") || (typeof a === "string" && typeof b === "string")) {
    return a < b ? -1 : a > b ? 1 : 0;
  }
  // A decimal with a decimal or an int; not with a float.
  if (a instanceof Dec || b instanceof Dec) {
    const x = a instanceof Dec ? a : isInt(a) || typeof a === "bigint" ? new Dec(BigInt(a), 0) : null;
    const y = b instanceof Dec ? b : isInt(b) || typeof b === "bigint" ? new Dec(BigInt(b), 0) : null;
    return x === null || y === null ? null : decCmp(x, y);
  }
  if ((typeof a === "bigint" || typeof a === "number") && (typeof b === "bigint" || typeof b === "number")) {
    return a < b ? -1 : a > b ? 1 : 0;
  }
  return null;
}

// ---- operators ---------------------------------------------------------

function add(a, b) {
  if (typeof a === "number" && typeof b === "number") return a + b;
  if (typeof a === "string") return a + display(b);
  if (typeof b === "string") return display(a) + b;
  if (a instanceof Dec || b instanceof Dec) return decArith(a, b, "+", "add");
  if (typeof a === "bigint" || typeof b === "bigint") return bigArith(a, b, "+", "add");
  if (a instanceof Html && b instanceof Html) return frag([a, b]);
  if (Array.isArray(a) && Array.isArray(b)) return a.concat(b);
  if (isMap(a) && isMap(b)) return { ...a, ...b };
  fail(`cannot add ${typeName(a)} and ${typeName(b)}`);
}

// Numbers: the usual case first; anything else says what is wrong.
function arith(a, b, op, verb) {
  if (a instanceof Dec || b instanceof Dec) return decArith(a, b, op, verb);
  if (typeof a === "bigint" || typeof b === "bigint") return bigArith(a, b, op, verb);
  if (typeof a !== "number" || typeof b !== "number") fail(`cannot ${verb} ${typeName(a)} and ${typeName(b)}`);
  if (isInt(a) && isInt(b) && b === 0) fail("division by zero");
  return op === "/" ? a / b : a % b;
}
const sub = (a, b) => (typeof a === "number" && typeof b === "number" ? a - b : arith(a, b, "-", "subtract"));
const mul = (a, b) => (typeof a === "number" && typeof b === "number" ? a * b : arith(a, b, "*", "multiply"));
const div = (a, b) => (typeof a === "number" && typeof b === "number" && b !== 0 ? a / b : arith(a, b, "/", "divide"));
const mod = (a, b) => (typeof a === "number" && typeof b === "number" && b !== 0 ? a % b : arith(a, b, "%", "take the remainder of"));

function order(a, b) {
  const c = cmp(a, b);
  if (c === null) fail(`cannot compare ${typeName(a)} with ${typeName(b)}${decHint(a, b)}`);
  return c;
}
const lt = (a, b) => order(a, b) < 0;
const le = (a, b) => order(a, b) <= 0;
const gt = (a, b) => order(a, b) > 0;
const ge = (a, b) => order(a, b) >= 0;

function intRange(a, b) {
  if (!isInt(a) || !isInt(b)) fail("`..` needs two ints");
  if (b - a > 10000000) fail("range is too large");
  const out = [];
  for (let i = a; i < b; i++) out.push(i);
  return out;
}

function neg(v) {
  if (typeof v !== "number") {
    if (v instanceof Dec) return new Dec(-v.m, v.s);
    if (typeof v === "bigint") return bigChecked(-v);
    fail(`cannot negate ${typeName(v)}`);
  }
  return -v;
}

function eventField(e, name) {
  const t = e.target || {};
  switch (name) {
    case "value": return t.value === undefined ? null : t.value;
    case "checked": return !!t.checked;
    case "type": return e.type;
    case "key": return e.key === undefined ? null : e.key;
    case "target": return t;
    case "form": {
      const f = t.tagName === "FORM" ? t : t.form;
      if (!f) return {};
      const out = {};
      for (const [k, v] of new FormData(f)) putKey(out, k, typeof v === "string" ? v : v.name);
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
        return x === undefined ? null : typeof x === "function" ? (...a) => foreignCall(v, name, a) : x;
      }
    } else if (hasOwn(v, name)) return v[name];
    if (name === "len") return B.len(v);
    if (name === "keys") return B.keys(v);
    if (name === "values") return B.values(v);
    return null;
  }
  if (v instanceof Err && name === "error") return v.error;
  if (isEvent(v)) return eventField(v, name);
  const f = fb || (hasOwn(B, name) ? B[name] : null);
  if (typeof f === "function") return f(v);
  fail(`${typeName(v)} has no field \`${name}\``);
}

function index(o, i) {
  if (Array.isArray(o) && isInt(i)) {
    const n = i < 0 ? o.length + i : i;
    return n >= 0 && n < o.length ? o[n] : null;
  }
  // The page's objects: by name, or by number for lists of theirs (files, elements).
  if (foreign(o) && (typeof i === "string" || isInt(i))) return field(o, String(i), null);
  if (isMap(o) && typeof i === "string") return hasOwn(o, i) ? o[i] : null;
  if (typeof o === "string" && isInt(i)) {
    const cs = Array.from(o);
    const n = i < 0 ? cs.length + i : i;
    return n >= 0 && n < cs.length ? cs[n] : null;
  }
  if (isNil(o)) return null;
  fail(`cannot index ${typeName(o)} with ${typeName(i)}`);
}

function callFn(f, args) {
  if (typeof f !== "function") fail(`cannot call ${typeName(f)}: it is not a function`);
  denied(f);
  const call = pageFns.get(f);
  if (call !== undefined) return call(f, args);
  return f(...args);
}

// `recv.name(args)`: a function stored in the map, or `name(recv, args)`.
function mcall(recv, name, fb, args) {
  if (foreign(recv) && typeof recv[name] === "function") return foreignCall(recv, name, args);
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
  if (isMap(v)) return Object.keys(v).map((k) => [k, v[k]]);
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
const M = (v, f, i = 1) => { if (isNil(v)) return {}; want(isMap(v), f, "a map", i, v); return v; };
const F = (v, f, i = 2) => {
  want(typeof v === "function", f, "a function", i, v);
  denied(v);
  // The page's own function is called through one of Quillo's, which checks what it is given.
  if (pageFns.has(v)) fail(`\`${f}\` is given a function of the page: wrap it, as in (x) => f(x)`);
  return v;
};
const N = (v, f, i = 1) => { want(typeof v === "number", f, "a number", i, v); return v; };

// Calls a function given to a builtin; it must not wait on the server.
// Every caller passes one argument (two for `reduce`).
function cb(f, who, a, b) {
  const r = arguments.length > 3 ? f(a, b) : f(a);
  if (r instanceof Promise) fail(`\`${who}\` cannot call a function that waits on the server; use a \`for\` loop instead`);
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
  if (t === "float" || t === "num") return s !== "" && !isNaN(Number(s)) ? Number(s) : null;
  if (t === "dec") return decParse(s, false);
  return null;
}

// A value as JSON data. `wire`: for the program's own server (a server
// fn's arguments), decimals and large ints tagged ({"$dec": "12.50"}) so
// they arrive as they are; else a decimal is its text, as the server
// writes it, and a large int its digits (`jsonText` writes them).
function toJSON(v, wire) {
  if (isNil(v)) return null;
  if (v instanceof Err) return { error: toJSON(v.error, wire) };
  if (Array.isArray(v)) return v.map((x) => toJSON(x, wire));
  if (v instanceof Html) return "";
  if (typeof v === "function") return repr(v);
  if (v instanceof Dec) return wire ? { $dec: decText(v) } : decText(v);
  if (typeof v === "bigint") return wire ? { $int: String(v) } : "\u0000int:" + v;
  if (isMap(v)) { const o = {}; for (const k of Object.keys(v)) putKey(o, k, toJSON(v[k], wire)); return o; }
  if (typeof v === "number" && !isFinite(v)) return null;
  return v;
}
// JSON text of a value, a large int written as its digits.
const jsonText = (v, indent) => JSON.stringify(toJSON(v), null, indent).replace(/"\\u0000int:(-?\d+)"/g, "$1");

const B = {};

// ---- streams (src/stream.rs) -------------------------------------------
// What a streamed answer sends, given to f as it arrives: an event's data
// (JSON, parsed) and its name, a line of JSON, or a piece of text.
async function readStream(res, f, who, signal) {
  const type = (res.headers.get("content-type") || "").toLowerCase();
  const kind = type.startsWith("text/event-stream") ? "events" : /ndjson|jsonl|json-seq/.test(type) ? "lines" : "text";
  const parse = (t) => { try { return JSON.parse(t); } catch (_) { return t; } };
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

// `receive(url, f, { method, body, headers })` in a handler: sends the
// request and calls f with what the answer streams, until it ends (or f
// gives false). An error value if it could not be had.
B.receive = meta(async (url, f, opts) => {
  S(url, "receive");
  F(f, "receive", 2);
  opts = M(opts, "receive", 3);
  const init = { method: opts.method ? S(opts.method, "receive").toUpperCase() : (isNil(opts.body) ? "GET" : "POST"), headers: {} };
  if (isMap(opts.headers)) for (const k of Object.keys(opts.headers)) init.headers[k] = display(opts.headers[k]);
  if (!isNil(opts.body)) {
    if (typeof opts.body === "string") init.body = opts.body;
    else { init.body = jsonText(opts.body); init.headers["content-type"] = init.headers["content-type"] || "application/json"; }
  }
  let res;
  try { res = await fetch(withBase(url), init); } catch (e) { return new Err("receive: " + e.message); }
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
  if (!Owner) fail("`listen` works in a component");
  S(url, "listen");
  F(f, "listen", 2);
  const ctl = new AbortController();
  onCleanup(() => ctl.abort());
  (async () => {
    let wait = 1000;
    while (!ctl.signal.aborted) {
      try {
        const res = await fetch(withBase(url), { signal: ctl.signal, headers: { accept: "text/event-stream, application/x-ndjson, */*" } });
        if (!res.ok) throw new Error(`${res.status}`);
        wait = 1000;
        await readStream(res, f, "listen", ctl.signal);
        return;
      } catch (e) {
        if (ctl.signal.aborted) return;
        if (e instanceof QuilloError) return report(e);
        await new Promise((r) => setTimeout(r, wait));
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
B.every = meta((ms, f) => {
  if (!Owner) fail("`every` works in a component");
  N(ms, "every");
  F(f, "every", 2);
  const id = setInterval(() => {
    try {
      const r = f();
      if (r instanceof Promise) r.catch(report);
    } catch (e) {
      report(e);
    }
  }, Math.max(ms, 1));
  onCleanup(() => clearInterval(id));
  return null;
}, "every", null);

// `parallel(f, g, ...)`: calls them at once (server calls overlap); their
// results, in order.
B.parallel = meta(async (...fs) => {
  if (fs.length === 1 && Array.isArray(fs[0])) fs = fs[0];
  return Promise.all(fs.map((f, i) => F(f, "parallel", i + 1)()));
}, "parallel", null);
B.print = meta((...a) => { console.log(a.map(display).join(" ")); return null; }, "print", null);
B.str = meta((v) => display(v), "str", null);
B.int = meta((v) => {
  if (isInt(v) || typeof v === "bigint") return v;
  // The whole part (toward zero).
  if (v instanceof Dec) { const w = v.m / p10(v.s); return w < I64_MIN || w > I64_MAX ? new Err(`${decText(v)} is too large for an int`) : intOf(w); }
  if (typeof v === "number" && isFinite(v)) return Math.trunc(v);
  if (typeof v === "boolean") return v ? 1 : 0;
  if (typeof v === "string") { const n = coerce("int", v); return n === null ? new Err(`not an int: ${JSON.stringify(v)}`) : n; }
  return new Err(`cannot convert ${typeName(v)} to int`);
}, "int", null);
B.float = meta((v) => {
  if (typeof v === "number") return v;
  if (v instanceof Dec) return Number(decText(v));
  if (typeof v === "bigint") return Number(v);
  if (typeof v === "string") { const n = coerce("float", v); return n === null ? new Err(`not a number: ${JSON.stringify(v)}`) : n; }
  return new Err(`cannot convert ${typeName(v)} to float`);
}, "float", null);
B.len = meta((v) => {
  if (typeof v === "string") return Array.from(v).length;
  if (Array.isArray(v)) return v.length;
  if (isMap(v)) return Object.keys(v).length;
  if (isNil(v)) return 0;
  fail(`\`len\` expects a str, list or map, got ${typeName(v)}`);
}, "len", null);
B.type_of = meta((v) => typeName(v), "type_of", null);
// `dec(x)`: a decimal from text (exactly), an int, or a float (through its
// shortest text, so dec(0.1) is 0.1m).
B.dec = meta((v) => {
  if (v instanceof Dec) return v;
  if (isInt(v) || typeof v === "bigint") return new Dec(BigInt(v), 0);
  if (typeof v === "number") return decFromFloat(v) ?? new Err("cannot convert float to dec: it is too large, or not a number");
  if (typeof v === "string") return decParse(v.trim(), false) ?? new Err(`not a decimal: ${JSON.stringify(v)}`);
  return new Err(`cannot convert ${typeName(v)} to dec`);
}, "dec", null);
B.upper = meta((s) => S(s, "upper").toUpperCase(), "upper", null);
B.lower = meta((s) => S(s, "lower").toLowerCase(), "lower", null);
B.trim = meta((s) => S(s, "trim").trim(), "trim", null);
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
B.replace = meta((s, a, b) => S(s, "replace").split(S(a, "replace", 2)).join(S(b, "replace", 3)), "replace", null);
B.slice = meta((v, start, end) => {
  I(start, "slice", 2);
  if (end !== undefined) I(end, "slice", 3);
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
  for (let i = 0; i < a.length; i++) if (truthy(cb(f, "filter", a[i]))) out.push(a[i]);
  return out;
}, "filter", null);
B.find = meta((l, f) => { F(f, "find"); for (const x of L(l, "find")) if (truthy(cb(f, "find", x))) return x; return null; }, "find", null);
B.any = meta((l, f) => { F(f, "any"); return L(l, "any").some((x) => truthy(cb(f, "any", x))); }, "any", null);
B.all = meta((l, f) => { F(f, "all"); return L(l, "all").every((x) => truthy(cb(f, "all", x))); }, "all", null);
B.reduce = meta((l, acc, f) => { F(f, "reduce", 3); for (const x of L(l, "reduce")) acc = cb(f, "reduce", acc, x); return acc; }, "reduce", null);
B.sum = meta((l) => L(l, "sum").reduce((a, x) => add(a, x), 0), "sum", null);
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
B.range = meta((a, b) => b === undefined ? intRange(0, I(a, "range")) : intRange(I(a, "range"), I(b, "range", 2)), "range", null);
B.enumerate = meta((l) => L(l, "enumerate").map((v, i) => [i, v]), "enumerate", null);
B.keys = meta((m) => Object.keys(M(m, "keys")), "keys", null);
B.values = meta((m) => { m = M(m, "values"); return Object.keys(m).map((k) => m[k]); }, "values", null);
B.get = meta((m, k, d = null) => { m = M(m, "get"); k = keyText(k); return hasOwn(m, k) ? m[k] : d; }, "get", null);
B.has = meta((m, k) => hasOwn(M(m, "has"), keyText(k)), "has", null);
B.set = meta((m, k, v = null) => {
  return putKey({ ...M(m, "set") }, keyText(k), v);
}, "set", null);
B.remove = meta((c, k) => {
  if (Array.isArray(c)) return c.filter((v) => !eq(v, k));
  const m = { ...M(c, "remove") };
  delete m[keyText(k)];
  return m;
}, "remove", null);
B.merge = meta((a, b) => ({ ...M(a, "merge"), ...M(b, "merge", 2) }), "merge", null);
B.err = meta((v = null) => new Err(v), "err", null);
B.is_err = meta((v) => v instanceof Err, "is_err", null);
B.unwrap = meta((v) => { if (v instanceof Err) fail(`unwrap of an error: ${display(v.error)}`); return v; }, "unwrap", null);
B.raw = meta((v) => rawHtml(display(v)), "raw", null);
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
  const no = noticeEl("button", "q-cancel", o.cancel != null ? display(o.cancel) : "Cancel");
  const yes = noticeEl("button", "q-ok", o.ok != null ? display(o.ok) : "OK");
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
    keywords: ["let", "fn", "view", "component", "server", "route", "use", "if", "else", "match", "for", "in", "return", "reply", "as", "while", "break", "continue", "type", "and", "or", "not"],
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
  const alpha = (c) => c !== undefined && /\p{L}/u.test(c);
  const alnum = (c) => c !== undefined && /[\p{L}\p{N}]/u.test(c);
  const digit = (c) => c !== undefined && c >= "0" && c <= "9";
  const identStart = (c) => alpha(c) || c === "_" || c === "$";
  const identChar = (c, isCss) => alnum(c) || c === "_" || c === "$" || (isCss && c === "-");
  return (code, name) => {
    const l = langs[String(name || "").toLowerCase()] || plain;
    const s = Array.from(code);
    const n = s.length;
    const starts = (i, p) => { const ps = Array.from(p); for (let k = 0; k < ps.length; k++) if (s[i + k] !== ps[k]) return false; return true; };
    let out = "", i = 0, plainAt = 0, depth = 0, inTag = false;
    const span = (from, to, cls) => { out += esc(s.slice(plainAt, from).join("")) + `<span class="${cls}">` + esc(s.slice(from, to).join("")) + "</span>"; plainAt = to; };
    const block = l.block || (l.tags ? ["<!--", "-->"] : null);
    while (i < n) {
      const c = s[i];
      if (l.line.some((p) => starts(i, p))) { let j = i; while (j < n && s[j] !== "\n") j++; span(i, j, "hl-c"); i = j; continue; }
      if (block && starts(i, block[0]) && !(l.tags && block[0] === "/*" && inTag)) {
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
      if (l.quotes.includes(c)) {
        let j = i + 1;
        while (j < n && s[j] !== c && (c === "`" || s[j] !== "\n")) { if (s[j] === "\\") j++; j++; }
        j = Math.min(j + 1, n);
        let k = j; while (k < n && (s[k] === " " || s[k] === "\t")) k++;
        span(i, j, l.json && s[k] === ":" ? "hl-a" : "hl-s"); i = j; continue;
      }
      if (l.css) {
        if (c === "{") depth++; else if (c === "}") depth = Math.max(depth - 1, 0);
        if ((c === "#" || c === ".") && identChar(s[i + 1], true) && !(c === "." && digit(s[i + 1]))) {
          let j = i + 1; while (j < n && identChar(s[j], true)) j++; span(i, j, depth > 0 && c === "#" ? "hl-n" : "hl-f"); i = j; continue;
        }
        if (c === "@" || (c === "!" && starts(i, "!important"))) { let j = i + 1; while (j < n && identChar(s[j], true)) j++; span(i, j, "hl-k"); i = j; continue; }
      }
      const prevIdent = i > 0 && identChar(s[i - 1], l.css);
      if (!(i > 0 && identChar(s[i - 1], false)) && (digit(c) || (c === "." && digit(s[i + 1])))) {
        let j = i; while (j < n && (/[A-Za-z0-9._]/.test(s[j]) || (l.css && s[j] === "%"))) j++; span(i, j, "hl-n"); i = j; continue;
      }
      if (identStart(c) && !prevIdent) {
        let j = i; while (j < n && identChar(s[j], l.css)) j++;
        const word = s.slice(i, j).join(""), probe = l.nocase ? word.toLowerCase() : word;
        let k = j; while (k < n && (s[k] === " " || s[k] === "\t")) k++;
        const next = s[k] === undefined ? " " : s[k];
        const cls = l.css ? (depth > 0 && next === ":" ? "hl-a" : depth === 0 ? "hl-t" : null)
          : l.keywords.includes(probe) ? "hl-k" : l.consts.includes(probe) ? "hl-n" : next === "(" ? "hl-f"
          : l.types && c !== c.toLowerCase() && c === c.toUpperCase() ? "hl-t" : null;
        if (cls) span(i, j, cls);
        i = j; continue;
      }
      i++;
    }
    return out + esc(s.slice(plainAt).join(""));
  };
})();
B.highlight = meta((code, lang) => rawHtml(HL(display(code), lang == null ? "" : display(lang))), "highlight", null);

B.escape = meta((v) => display(v).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]), "escape", null);
B.now = meta(() => Date.now(), "now", null);
B.random = meta(() => Math.random(), "random", null);
B.random_int = meta((lo, hi) => {
  I(lo, "random_int"); I(hi, "random_int", 2);
  if (lo > hi) fail(`\`random_int\` needs lo <= hi, got ${lo} and ${hi}`);
  return lo + Math.floor(Math.random() * (hi - lo + 1));
}, "random_int", null);
B.uuid = meta(() => (typeof crypto !== "undefined" && crypto.randomUUID) ? crypto.randomUUID()
  : "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, (c) => { const r = Math.random() * 16 | 0; return (c === "x" ? r : (r & 3 | 8)).toString(16); }), "uuid", null);
B.abs = meta((x) => x instanceof Dec ? new Dec(babs(x.m), x.s) : typeof x === "bigint" ? bigChecked(babs(x)) : Math.abs(N(x, "abs")), "abs", null);
// Halves round away from zero, as on the server (Math.round(-2.5) is -2).
const roundHalf = (x) => Math.sign(x) * Math.round(Math.abs(x));
// A decimal rounded: round(d, digits = 0, mode = "half_even"), keeping
// exactly `digits` decimals.
function roundDec(x, d, mode) {
  if (!isNil(d)) I(d, "round", 2);
  if (isNil(mode)) mode = "half_even";
  else if (typeof mode !== "string") fail(`\`round\` expects a str as argument 3, got ${typeName(mode)}`);
  else if (!["half_even", "half_up", "half_down", "up", "down", "ceiling", "floor"].includes(mode)) fail(`\`round\`: unknown rounding ${JSON.stringify(mode)} (one of ${ROUNDINGS})`);
  return decRound(x, isNil(d) ? 0 : d, mode);
}
B.round = meta((x, d, mode) => {
  if (x instanceof Dec) return roundDec(x, d, mode);
  if (!isNil(mode)) fail("`round` takes a rounding mode for a dec only: convert with dec(x)");
  N(x, "round"); if (d === undefined) return roundHalf(x); const p = 10 ** Math.min(Math.max(I(d, "round", 2), 0), 15); return roundHalf(x * p) / p; }, "round", null);
B.floor = meta((x) => x instanceof Dec ? decRound(x, 0, "floor") : Math.floor(N(x, "floor")), "floor", null);
B.ceil = meta((x) => x instanceof Dec ? decRound(x, 0, "ceiling") : Math.ceil(N(x, "ceil")), "ceil", null);
B.sqrt = meta((x) => Math.sqrt(N(x, "sqrt")), "sqrt", null);
B.pow = meta((x, y) => N(x, "pow") ** N(y, "pow", 2), "pow", null);
B.unique = meta((l) => { const out = []; for (const x of L(l, "unique")) if (!out.some((y) => eq(x, y))) out.push(x); return out; }, "unique", null);
B.flatten = meta((l) => [].concat(...L(l, "flatten").map((x) => Array.isArray(x) ? x : [x])), "flatten", null);
B.zip = meta((a, b) => { a = L(a, "zip"); b = L(b, "zip", 2); return a.slice(0, Math.min(a.length, b.length)).map((x, i) => [x, b[i]]); }, "zip", null);
B.take = meta((v, n) => { n = Math.max(I(n, "take", 2), 0); return typeof v === "string" ? Array.from(v).slice(0, n).join("") : L(v, "take").slice(0, n); }, "take", null);
B.drop = meta((v, n) => { n = Math.max(I(n, "drop", 2), 0); return typeof v === "string" ? Array.from(v).slice(n).join("") : L(v, "drop").slice(n); }, "drop", null);
B.count = meta((l, f) => { l = L(l, "count"); if (f === undefined) return l.length; F(f, "count"); return l.filter((x) => truthy(cb(f, "count", x))).length; }, "count", null);
B.group_by = meta((l, f) => { F(f, "group_by"); const out = {}; for (const x of L(l, "group_by")) { const k = keyText(cb(f, "group_by", x)); if (!hasOwn(out, k)) Object.defineProperty(out, k, { value: [], enumerable: true, writable: true, configurable: true }); out[k].push(x); } return out; }, "group_by", null);
B.index_of = meta((c, x) => {
  if (Array.isArray(c)) { const i = c.findIndex((y) => eq(y, x)); return i < 0 ? null : i; }
  if (typeof c === "string") { const i = c.indexOf(display(x)); return i < 0 ? null : Array.from(c.slice(0, i)).length; }
  fail(`\`index_of\` expects a list or str, got ${typeName(c)}`);
}, "index_of", null);
B.repeat = meta((s, n) => { S(s, "repeat"); I(n, "repeat", 2); if (n < 0 || s.length * n > 1e8) fail("`repeat` count is negative or too large"); return s.repeat(n); }, "repeat", null);
// `js("Chart.register", ...)`: a function of the page's scripts, found from
// the page's globals by its dotted path; `js_new` for a class.
function jsPath(path, f) {
  S(path, f);
  let owner = globalThis, v = globalThis;
  for (const p of path.split(".")) {
    foreignName(p);
    if (v === null || v === undefined || !(p in Object(v))) fail(`\`${f}\`: ${path} is not defined on this page`);
    owner = v;
    v = v[p];
  }
  return [owner, v];
}
B.js = meta((path, ...args) => {
  const [owner, v] = jsPath(path, "js");
  if (typeof v !== "function") {
    if (args.length) fail(`\`js\`: ${path} is not a function`);
    return v === undefined ? null : v;
  }
  guardCall(`\`js\`: ${path}`, v, args);
  return fromPage(Reflect.apply(v, owner, args));
}, "js", null);
B.js_new = meta((path, ...args) => {
  const [, v] = jsPath(path, "js_new");
  if (typeof v !== "function") fail(`\`js_new\`: ${path} is not a class`);
  guardCall(`\`js_new\`: ${path}`, v, args);
  return fromPage(Reflect.construct(v, args));
}, "js_new", null);

// Translations: the page's locale and its text (see src/i18n.rs).
const i18n = () => (typeof window !== "undefined" && window.__quillo_i18n) || null;
// The plural form of n, as on the server (CLDR's rules for whole numbers).
const plural = (locale, n) => {
  if (!Number.isInteger(n)) return "other";
  n = Math.abs(n);
  const m10 = n % 10, m100 = n % 100, lang = String(locale).split(/[-_]/)[0].toLowerCase();
  const few = m10 >= 2 && m10 <= 4 && !(m100 >= 12 && m100 <= 14);
  switch (lang) {
    case "ja": case "zh": case "ko": case "th": case "vi": case "id": case "ms": case "lo": case "my": case "km": case "yue": return "other";
    case "fr": case "pt": case "hi": case "bn": case "fa": case "gu": case "kn": case "zu": case "am": return n <= 1 ? "one" : "other";
    case "ru": case "uk": case "be": return m10 === 1 && m100 !== 11 ? "one" : few ? "few" : "many";
    case "sr": case "hr": case "bs": return m10 === 1 && m100 !== 11 ? "one" : few ? "few" : "other";
    case "pl": return n === 1 ? "one" : few ? "few" : "many";
    case "cs": case "sk": return n === 1 ? "one" : n >= 2 && n <= 4 ? "few" : "other";
    case "ar": return n === 0 ? "zero" : n === 1 ? "one" : n === 2 ? "two" : m100 >= 3 && m100 <= 10 ? "few" : m100 >= 11 ? "many" : "other";
    case "he": return n === 1 ? "one" : n === 2 ? "two" : "other";
    default: return n === 1 ? "one" : "other";
  }
};
B.locale = meta(() => { const d = i18n(); return d ? d.locale : null; }, "locale", null);
const translate = (k, vars, esc) => {
  S(k, "t"); vars = M(vars, "t", 2);
  const d = i18n();
  let e = d && hasOwn(d.t, k) ? d.t[k] : k;
  if (e && typeof e === "object") {
    const cat = typeof vars.count === "number" ? plural(d.locale, vars.count) : "other";
    e = hasOwn(e, cat) ? e[cat] : (e.other ?? "");
  }
  return String(e).replace(/\{(\w+)\}/g, (m, name) => hasOwn(vars, name) ? esc(display(vars[name])) : m);
};
B.t = meta((k, vars) => translate(k, vars, (s) => s), "t", null);
// raw(t(key, vars)): the translation's markup, with the values put in escaped.
B.t_html = (k, vars) => rawHtml(translate(k, vars, B.escape));
B.url = meta(() => currentUrl(), "url", null);
B.css = meta((s) => S(s, "css"), "css", null);
B.sql = meta((s) => S(s, "sql"), "sql", null);
B.navigate = meta((to) => navigate(S(to, "navigate")), "navigate", null);
B.json = {
  parse: (s) => { S(s, "json.parse"); try { return fromJSON(JSON.parse(s)); } catch (e) { return new Err("invalid JSON: " + e.message); } },
  stringify: (v, pretty) => jsonText(v, truthy(pretty) ? 2 : undefined),
};

function fromJSON(j) {
  if (j === undefined) return null;
  return j;
}

// `User(fields)`: a User, or an error value saying what is wrong.
B.__record = meta((name, v) => {
  const td = OWN_TYPES[name];
  if (!td) fail(`there is no type \`${name}\``);
  if (td.r && isNil(v)) v = {};
  if (td.r && !isMap(v)) return new Err(`${name}: expects a map of its fields, got ${typeName(v)} ${short(v)}`);
  const r = ownMake(ownTyOf(name), v);
  if (r.ok) return r.v;
  return new Err(`${name}: ${ownWhyText(ownWhyHere(r.w, { t: td.a ? td.a.t : name }, v))}`);
}, "__record", null);

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
const MONEY_SYMBOLS = "USD=US$,EUR=\u20ac,GBP=\u00a3,JPY=JP\u00a5,CNY=CN\u00a5,INR=\u20b9,CAD=CA$,AUD=AU$,NZD=NZ$,BRL=R$,MXN=MX$,KRW=\u20a9,TWD=NT$,HKD=HK$,VND=\u20ab,ILS=\u20aa";
const MONEY_LOCALES = "en-US\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tUSD=$,JPY=\u00a5,AUD=A$,PHP=\u20b1\n" +
  "en-GB\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tAUD=A$,PHP=\u20b1\n" +
  "en-CA\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tCAD=$,AUD=A$,PHP=\u20b1\n" +
  "en-AU\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tUSD=USD,EUR=EUR,GBP=GBP,JPY=JPY,CNY=CNY,INR=INR,CAD=CAD,AUD=$,NZD=NZD,BRL=BRL,MXN=MXN,KRW=KRW,TWD=TWD,HKD=HKD,VND=VND,ILS=ILS\n" +
  "en-NZ\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tAUD=A$,NZD=$,PHP=\u20b1\n" +
  "en-IE\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tAUD=A$,PHP=\u20b1\n" +
  "en-IN\t.\t,\t\t\t-\t1\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tUSD=$,AUD=A$,PHP=\u20b1\n" +
  "en-SG\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tAUD=A$,SGD=$,PHP=\u20b1\n" +
  "en-ZA\t,\t\u00a0\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tAUD=A$,PHP=\u20b1,ZAR=R\n" +
  "de-DE\t,\t.\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tUSD=$,JPY=\u00a5,THB=\u0e3f\n" +
  "de-AT\t,\t.\t\t\u00a0\t-\t0\t\u00a4\u00a0#\t-\u00a4\u00a0#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tUSD=$,JPY=\u00a5,THB=\u0e3f\n" +
  "de-CH\t.\t\u2019\t\t\t-\t0\t\u00a4\u00a0#\t\u00a4-#\t\u00a4\u00a0#\t\u00a4-#\tUSD=$,EUR=EUR,JPY=\u00a5,THB=\u0e3f\n" +
  "fr-FR\t,\t\u202f\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tUSD=$US,GBP=\u00a3GB,JPY=JPY,CNY=CNY,CAD=$CA,AUD=$AU,NZD=$NZ,MXN=$MX,ARS=$AR,TWD=TWD,HKD=HKD,SGD=$SG\n" +
  "fr-BE\t,\t\u202f\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tUSD=$US,GBP=\u00a3GB,JPY=JPY,CNY=CNY,CAD=$CA,AUD=$AU,NZD=$NZ,MXN=$MX,ARS=$AR,TWD=TWD,HKD=HKD,SGD=$SG\n" +
  "fr-CA\t,\t\u00a0\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tUSD=$\u00a0US,JPY=\u00a5,INR=INR,CAD=$,AUD=$\u00a0AU,NZD=$\u00a0NZ,MXN=MXN,KRW=KRW,TWD=TWD,HKD=$\u00a0HK,SGD=$\u00a0SG,VND=VND,ILS=ILS\n" +
  "fr-CH\t.\t\u202f\t,\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tUSD=$US,GBP=\u00a3GB,JPY=JPY,CNY=CNY,CAD=$CA,AUD=$AU,NZD=$NZ,MXN=$MX,ARS=$AR,TWD=TWD,HKD=HKD,SGD=$SG\n" +
  "it-IT\t,\t.\t\t\t-\t2\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tUSD=USD,JPY=JPY,INR=INR,AUD=A$,BRL=BRL,MXN=MXN,KRW=KRW,TWD=TWD,HKD=HKD,THB=\u0e3f,VND=VND,PHP=\u20b1\n" +
  "it-CH\t.\t\u2019\t\t\t-\t2\t\u00a4\u00a0#\t\u00a4-#\t\u00a4\u00a0#\t\u00a4-#\tUSD=USD,JPY=JPY,INR=INR,AUD=A$,BRL=BRL,MXN=MXN,KRW=KRW,TWD=TWD,HKD=HKD,THB=\u0e3f,VND=VND,PHP=\u20b1\n" +
  "es-ES\t,\t.\t\t\t-\t2\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tGBP=GBP,JPY=JPY,CNY=CNY,INR=INR,CAD=CAD,AUD=AUD,NZD=NZD,BRL=BRL,MXN=MXN,KRW=KRW,TWD=TWD,HKD=HKD,THB=\u0e3f,ILS=ILS\n" +
  "es-MX\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tUSD=USD,EUR=EUR,GBP=GBP,JPY=JPY,CNY=CNY,INR=INR,CAD=CAD,AUD=AUD,NZD=NZD,BRL=BRL,MXN=$,KRW=KRW,TWD=TWD,HKD=HKD,VND=VND,ILS=ILS\n" +
  "es-AR\t,\t.\t\t\t-\t0\t\u00a4\u00a0#\t-\u00a4\u00a0#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tEUR=EUR,GBP=GBP,JPY=JPY,CNY=CNY,INR=INR,CAD=CAD,AUD=AUD,NZD=NZD,BRL=BRL,MXN=MXN,ARS=$,KRW=KRW,TWD=TWD,HKD=HKD,VND=VND,ILS=ILS\n" +
  "pt-BR\t,\t.\t\t\t-\t0\t\u00a4\u00a0#\t-\u00a4\u00a0#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tTHB=\u0e3f\n" +
  "pt-PT\t,\t\u00a0\t\t\t-\t2\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tTHB=\u0e3f\n" +
  "nl-NL\t,\t.\t\t\t-\t0\t\u00a4\u00a0#\t\u00a4\u00a0-#\t\u00a4\u00a0#\t\u00a4\u00a0-#\tCAD=C$,THB=\u0e3f\n" +
  "nl-BE\t,\t.\t\t\t-\t0\t\u00a4\u00a0#\t\u00a4\u00a0-#\t\u00a4\u00a0#\t\u00a4\u00a0-#\tCAD=C$,THB=\u0e3f\n" +
  "sv-SE\t,\t\u00a0\t\t\t\u2212\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tGBP=GBP,JPY=JPY,CNY=CNY,INR=INR,AUD=AUD,NZD=NZD,SEK=kr,NOK=Nkr,DKK=Dkr,BRL=BR$,KRW=KRW,TWD=TWD,HKD=HKD,VND=VND,EGP=EG\u00a3\n" +
  "nb-NO\t,\t\u00a0\t\t\t\u2212\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tUSD=USD,JPY=JPY,CNY=CNY,INR=INR,CAD=CAD,AUD=AUD,NZD=NZD,NOK=kr,BRL=BRL,MXN=MXN,KRW=KRW,TWD=TWD,HKD=HKD,VND=VND,ILS=ILS\n" +
  "da-DK\t,\t.\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tDKK=kr.,THB=\u0e3f\n" +
  "fi-FI\t,\t\u00a0\t\t\t\u2212\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tUSD=$,JPY=\u00a5,CNY=CNY,INR=INR,CAD=CAD,AUD=AUD,NZD=NZD,BRL=BRL,MXN=MXN,KRW=KRW,TWD=TWD,HKD=HKD,VND=VND,ILS=ILS\n" +
  "pl-PL\t,\t\u00a0\t\t\t-\t2\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tUSD=USD,GBP=GBP,JPY=JPY,CNY=CNY,INR=INR,CAD=CAD,AUD=AUD,NZD=NZD,PLN=z\u0142,MXN=MXN,KRW=KRW,TWD=TWD,HKD=HKD,VND=VND,ILS=ILS\n" +
  "cs-CZ\t,\t\u00a0\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tINR=INR,CZK=K\u010d,VND=VND,ILS=ILS\n" +
  "hu-HU\t,\t\u00a0\t\t\t-\t2\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tUSD=USD,EUR=EUR,GBP=GBP,JPY=\u00a5,CNY=CNY,INR=INR,CAD=CAD,AUD=AUD,NZD=NZD,HUF=Ft,BRL=BRL,MXN=MXN,KRW=KRW,TWD=TWD,HKD=HKD,VND=VND,ILS=ILS\n" +
  "ro-RO\t,\t.\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tUSD=USD,EUR=EUR,GBP=GBP,JPY=JPY,CNY=CNY,INR=INR,CAD=CAD,AUD=AUD,NZD=NZD,BRL=BRL,MXN=MXN,KRW=KRW,TWD=TWD,HKD=HKD,VND=VND,ILS=ILS\n" +
  "el-GR\t,\t.\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tUSD=$,AUD=A$,THB=\u0e3f\n" +
  "tr-TR\t,\t.\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tUSD=$,JPY=\u00a5,TRY=\u20ba,THB=\u0e3f\n" +
  "ru-RU\t,\t\u00a0\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tUSD=$,JPY=\u00a5,AUD=A$,RUB=\u20bd,UAH=\u20b4,THB=\u0e3f\n" +
  "uk-UA\t,\t\u00a0\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tUSD=USD,EUR=EUR,GBP=GBP,JPY=\u00a5,CNY=CNY,INR=INR,CAD=CAD,AUD=AUD,NZD=NZD,UAH=\u20b4,BRL=BRL,MXN=MXN,KRW=KRW,TWD=TWD,HKD=HKD,VND=VND,ILS=ILS\n" +
  "ja-JP\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tUSD=$,JPY=\uffe5,CNY=\u5143,AUD=A$\n" +
  "zh-CN\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tCNY=\u00a5\n" +
  "zh-TW\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tJPY=\u00a5,KRW=\uffe6,TWD=$\n" +
  "zh-HK\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tJPY=\u00a5\n" +
  "ko-KR\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\t\n" +
  "hi-IN\t.\t,\t\t\t-\t1\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tUSD=$,AUD=A$,THB=\u0e3f\n" +
  "id-ID\t,\t.\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tINR=Rs,THB=\u0e3f,IDR=Rp\n" +
  "th-TH\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tJPY=\u00a5,THB=\u0e3f\n" +
  "vi-VN\t,\t.\t\t\t-\t0\t#\u00a0\u00a4\t-#\u00a0\u00a4\t#\u00a0\u00a4\t-#\u00a0\u00a4\tJPY=\u00a5,THB=\u0e3f\n" +
  "ms-MY\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tUSD=USD,CAD=CAD,AUD=A$,MXN=MXN,MYR=RM\n" +
  "fil-PH\t.\t,\t\t\t-\t0\t\u00a4#\t-\u00a4#\t\u00a4\u00a0#\t-\u00a4\u00a0#\tUSD=$,JPY=\u00a5,AUD=A$,THB=\u0e3f,PHP=\u20b1";
let moneyT = null;
function moneyTables() {
  if (moneyT) return moneyT;
  const pairs = (s) => { const o = Object.create(null); for (const p of s.split(",")) { const i = p.indexOf("="); if (i > 0) o[p.slice(0, i)] = p.slice(i + 1); } return o; };
  const units = Object.create(null);
  for (const c of MONEY_CURRENCIES.split(" ")) units[c.slice(0, 3)] = +c.slice(3);
  const locales = MONEY_LOCALES.split("\n").map((l) => {
    const f = l.split("\t");
    return { name: f[0], decimal: f[1], group: f[2], number: [f[3] || f[1], f[4] || f[2]], minus: f[5], indian: (+f[6] & 1) !== 0, min2: (+f[6] & 2) !== 0, pats: f.slice(7, 11), symbols: pairs(f[11] || "") };
  });
  return (moneyT = { units, locales, symbols: pairs(MONEY_SYMBOLS) });
}
const unitsOf = (code) => { const u = moneyTables().units[code]; return u === undefined ? null : u; };
function localeData(name) {
  const all = moneyTables().locales;
  name = String(name).replace(/_/g, "-").toLowerCase();
  const lang = name.split("-")[0];
  return all.find((l) => l.name.toLowerCase() === name) || all.find((l) => l.name.split("-")[0].toLowerCase() === lang) || all[0];
}
function moneyDigits(loc, [decimal, group], d) {
  const t = decText(new Dec(babs(d.m), d.s));
  const dot = t.indexOf(".");
  const whole = dot < 0 ? t : t.slice(0, dot), frac = dot < 0 ? "" : t.slice(dot + 1);
  const n = whole.length, grouped = !(loc.min2 && n < 5);
  let out = "";
  for (let i = 0; i < n; i++) {
    const left = n - i;
    if (i > 0 && grouped && (loc.indian ? left === 3 || (left > 3 && (left - 3) % 2 === 0) : left % 3 === 0)) out += group;
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
  const letters = side !== undefined && /\p{L}/u.test(side);
  const pat = loc.pats[(letters ? 2 : 0) + (d.m < 0n ? 1 : 0)];
  const num = moneyDigits(loc, [loc.decimal, loc.group], d);
  return pat.replace("-", loc.minus).replace("¤", sym).replace("#", num);
}
const moneyShort = (v) => (v === undefined ? "nothing" : `${typeName(v)} ${short(v)}`);
function moneyCurrency(c, f, i) {
  if (typeof c !== "string") fail(`\`${f}\` expects a currency code (a str) as argument ${i}, got ${moneyShort(c)}`);
  const code = c.trim().toUpperCase(), u = unitsOf(code);
  return u === null ? new Err(`unknown currency ${JSON.stringify(c)}`) : { code, units: u };
}
function moneyArg(v, f, i) {
  const bad = (why) => fail(`\`${f}\` expects money ({ amount, currency }) as argument ${i}, got ${moneyShort(v)}${why}`);
  if (!isMap(v)) bad("");
  const c = hasOwn(v, "currency") ? v.currency : null;
  if (typeof c !== "string") bad(": no currency");
  const code = c.toUpperCase(), units = unitsOf(code);
  if (units === null) bad(": unknown currency");
  const a = hasOwn(v, "amount") ? v.amount : null;
  const amount = a instanceof Dec || isInt(a) || typeof a === "bigint" || typeof a === "string" ? asDec(a) : null;
  if (amount === null) bad(": its amount must be a dec");
  return { amount, code, units };
}
function moneyMode(m, f) {
  if (isNil(m)) return "half_even";
  if (typeof m !== "string") fail(`\`${f}\` expects a rounding mode (a str) as argument ${f === "money.round" ? 2 : 3}, got ${moneyShort(m)}`);
  if (!["half_even", "half_up", "half_down", "up", "down", "ceiling", "floor"].includes(m)) fail(`\`${f}\`: unknown rounding ${JSON.stringify(m)} (one of ${ROUNDINGS})`);
  return m;
}
const moneyOf = (amount, code) => ({ amount, currency: code });
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
    else if (typeof amount === "string") { d = decParse(amount.trim(), false); if (d === null) return new Err(`not an amount: ${JSON.stringify(amount)}`); }
    else if (typeof amount === "number") fail("`money.of` takes a dec, an int or text, not a float: convert it with dec(x)");
    else fail(`\`money.of\` expects an amount (a dec, int or str) as argument 1, got ${moneyShort(amount)}`);
    if (isNil(mode)) {
      const exact = decRound(d, c.units, "down");
      if (decCmp(exact, d) !== 0) return new Err(`${decText(d)} has more decimals than ${c.code} has (${c.units}); give a rounding mode to round it`);
      return moneyOf(exact, c.code);
    }
    return moneyOf(decRound(d, c.units, moneyMode(mode, "money.of")), c.code);
  },
  from_minor: (n, currency) => {
    const c = moneyCurrency(currency, "money.from_minor", 2);
    if (c instanceof Err) return c;
    if (!isInt(n) && typeof n !== "bigint") fail(`\`money.from_minor\` expects an int as argument 1, got ${moneyShort(n)}`);
    return moneyOf(new Dec(BigInt(n), c.units), c.code);
  },
  to_minor: (m) => {
    const a = moneyArg(m, "money.to_minor", 1), d = decRound(a.amount, a.units, "half_even");
    return d.s === a.units ? bigChecked(d.m) : fail("integer overflow");
  },
  add: (x, y) => { const a = moneyArg(x, "money.add", 1), b = moneyArg(y, "money.add", 2); moneySame(a, b, "add"); return moneyOf(decRound(decAdd(a.amount, b.amount), a.units, "half_even"), a.code); },
  sub: (x, y) => { const a = moneyArg(x, "money.sub", 1), b = moneyArg(y, "money.sub", 2); moneySame(a, b, "subtract"); return moneyOf(decRound(decAdd(a.amount, new Dec(-b.amount.m, b.amount.s)), a.units, "half_even"), a.code); },
  mul: (x, factor, mode) => {
    const a = moneyArg(x, "money.mul", 1);
    let f;
    if (factor instanceof Dec) f = factor;
    else if (isInt(factor) || typeof factor === "bigint") f = new Dec(BigInt(factor), 0);
    else if (typeof factor === "number") fail("`money.mul` takes a dec or an int, not a float: convert it with dec(x)");
    else fail(`\`money.mul\` expects a dec or an int as argument 2, got ${moneyShort(factor)}`);
    return moneyOf(decRound(decMul(a.amount, f), a.units, moneyMode(mode, "money.mul")), a.code);
  },
  neg: (x) => { const a = moneyArg(x, "money.neg", 1); return moneyOf(new Dec(-a.amount.m, a.amount.s), a.code); },
  compare: (x, y) => { const a = moneyArg(x, "money.compare", 1), b = moneyArg(y, "money.compare", 2); moneySame(a, b, "compare"); return decCmp(a.amount, b.amount); },
  zero: (currency) => { const c = moneyCurrency(currency, "money.zero", 1); return c instanceof Err ? c : moneyOf(new Dec(0n, c.units), c.code); },
  is_zero: (x) => moneyArg(x, "money.is_zero", 1).amount.m === 0n,
  round: (x, mode) => { const a = moneyArg(x, "money.round", 1); return moneyOf(decRound(a.amount, a.units, moneyMode(mode, "money.round")), a.code); },
  minor_units: (currency) => { const c = moneyCurrency(currency, "money.minor_units", 1); return c instanceof Err ? c : c.units; },
  // Split by ratios without losing or making a minor unit: each share its
  // part rounded down, the units left one each to the first shares with a
  // ratio (Fowler's allocation).
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
    const minor = decRound(a.amount, a.units, "half_even").m, neg = minor < 0n, units = babs(minor);
    const shares = whole.map((r) => (units * r) / total);
    let left = units - shares.reduce((t, x) => t + x, 0n);
    for (let i = 0; i < shares.length && left > 0n; i++) if (whole[i] > 0n) { shares[i] += 1n; left -= 1n; }
    return shares.map((x) => moneyOf(new Dec(neg ? -x : x, a.units), a.code));
  },
  format: (x, locale) => { const a = moneyArg(x, "money.format", 1); return formatAmount(a.amount, a.code, a.units, moneyLocale(locale, "money.format", 2)); },
};
// format_number(x, locale, digits): a number as the locale writes one.
B.format_number = meta((x, locale, digits) => {
  let d;
  if (x instanceof Dec) d = x;
  else if (isInt(x) || typeof x === "bigint") d = new Dec(BigInt(x), 0);
  else if (typeof x === "number") { d = decFromFloat(x); if (d === null) return display(x); }
  else fail(`\`format_number\` expects a number as argument 1, got ${moneyShort(x)}`);
  const loc = localeData(moneyLocale(locale, "format_number", 2));
  if (!isNil(digits)) d = decRound(d, Math.min(Math.max(I(digits, "format_number", 3), 0), 28), "half_even");
  return (d.m < 0n ? loc.minus : "") + moneyDigits(loc, loc.number, d);
}, "format_number", null);

// ---- math, stats, vec (src/numeric.rs, the same answers) ------------------
// The browser cannot tell 2 from 2.0, so ints are numbers that are whole.

const decNote = (v) => (v instanceof Dec ? FLOAT_ONLY : "");
const NS = (l, f, i = 1) => { l = L(l, f, i); l.forEach((v, k) => { if (typeof v !== "number") fail(`\`${f}\` expects a list of numbers, but item ${k} is ${typeName(v)}${decNote(v)}`); }); return l; };
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
const meanOf = (x) => div2(csum2(x), x.length);
const ssOf = (x, m) => {
  const d = x.map((v) => v - m), c = csum(d), [hi, lo] = cdot2(d, d), fix = c * c / x.length, t = hi - fix;
  return t <= 0 ? [0, 0] : [t, lo - ((t - hi) + fix)];
};
const sortedOf = (x) => Float64Array.from(x).sort();
const overflowed = (n) => { if (!Number.isSafeInteger(n)) fail("integer overflow"); return n; };

const mathDomain = (f, x) => fail(`\`math.${f}\`: math domain error (${display(x)})`);
const mathChecked = (f, x, y, ok) => {
  if (!ok || (Number.isNaN(y) && !Number.isNaN(x))) mathDomain(f, x);
  if (!Number.isFinite(y) && !Number.isNaN(y) && Number.isFinite(x)) fail(`\`math.${f}\`: math range error (${display(x)})`);
  return y;
};
const checked2 = (f, x, y, r, ok) => {
  const nanIn = Number.isNaN(x) || Number.isNaN(y);
  if (!ok || (Number.isNaN(r) && !nanIn)) fail(`\`math.${f}\`: math domain error (${display(x)}, ${display(y)})`);
  if (!Number.isFinite(r) && !Number.isNaN(r) && Number.isFinite(x) && Number.isFinite(y)) fail(`\`math.${f}\`: math range error (${display(x)}, ${display(y)})`);
  return r;
};
const unary = (f, g, dom = () => true) => (x) => { N(x, "math." + f); return mathChecked(f, x, g(x), Number.isNaN(x) || dom(x)); };

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
const toInt = (f, g) => (x) => { N(x, f); if (!Number.isFinite(x)) fail(`\`${f}\`: cannot convert ${display(x)} to an int`); return overflowed(g(x) + 0); };
const natural = (n, f, i = 1) => { I(n, f, i); if (n < 0) fail(`\`${f}\` is not defined for negative numbers (${n})`); return n; };
const gcd2 = (a, b) => { a = Math.abs(a); b = Math.abs(b); while (b) [a, b] = [b, a % b]; return a; };
const intsOf = (args, f) => (args.length === 1 && Array.isArray(args[0]) ? args[0] : args).map((v, i) => I(v, f, i + 1));
const pairOf = (a, b, f) => { a = NS(a, f); b = NS(b, f, 2); if (a.length !== b.length) fail(`\`${f}\` needs lists of the same length (${a.length} and ${b.length})`); return [a, b]; };
const dotOf = (a, b, f) => {
  [a, b] = pairOf(a, b, f);
  if (a.every(isInt) && b.every(isInt)) { let s = 0; for (let i = 0; i < a.length; i++) s = overflowed(s + overflowed(a[i] * b[i])); return s; }
  return cdot(a, b);
};

B.math = {
  pi: Math.PI, e: Math.E, tau: 2 * Math.PI, inf: Infinity, nan: NaN,
  sqrt: unary("sqrt", Math.sqrt, (x) => x >= 0),
  cbrt: unary("cbrt", Math.cbrt),
  exp: unary("exp", Math.exp),
  exp2: unary("exp2", (x) => 2 ** x),
  expm1: unary("expm1", Math.expm1),
  log: (x, b) => {
    N(x, "math.log");
    if (!given(b)) return mathChecked("log", x, Math.log(x), Number.isNaN(x) || x > 0);
    N(b, "math.log", 2);
    if (b <= 0 || b === 1) fail(`\`math.log\`: math domain error (base ${display(b)})`);
    return mathChecked("log", x, Math.log(x) / Math.log(b), Number.isNaN(x) || x > 0);
  },
  log2: unary("log2", Math.log2, (x) => x > 0),
  log10: unary("log10", Math.log10, (x) => x > 0),
  log1p: unary("log1p", Math.log1p, (x) => x > -1),
  pow: (x, y) => { N(x, "math.pow"); N(y, "math.pow", 2); return checked2("pow", x, y, Math.pow(x, y), !(x === 0 && y < 0)); },
  sin: unary("sin", Math.sin, Number.isFinite),
  cos: unary("cos", Math.cos, Number.isFinite),
  tan: unary("tan", Math.tan, Number.isFinite),
  asin: unary("asin", Math.asin, (x) => Math.abs(x) <= 1),
  acos: unary("acos", Math.acos, (x) => Math.abs(x) <= 1),
  atan: unary("atan", Math.atan),
  atan2: (y, x) => Math.atan2(N(y, "math.atan2"), N(x, "math.atan2", 2)),
  sinh: unary("sinh", Math.sinh),
  cosh: unary("cosh", Math.cosh),
  tanh: unary("tanh", Math.tanh),
  asinh: unary("asinh", Math.asinh),
  acosh: unary("acosh", Math.acosh, (x) => x >= 1),
  atanh: unary("atanh", Math.atanh, (x) => Math.abs(x) < 1),
  degrees: unary("degrees", (x) => x * (180 / Math.PI)),
  radians: unary("radians", (x) => x * (Math.PI / 180)),
  hypot: (...v) => norm2(v.map((x, i) => N(x, "math.hypot", i + 1))),
  dist: (p, q) => {
    p = NS(p, "math.dist"); q = NS(q, "math.dist", 2);
    if (p.length !== q.length) fail(`\`math.dist\`: both points must have the same number of dimensions (${p.length} and ${q.length})`);
    return norm2(p.map((x, i) => x - q[i]));
  },
  floor: toInt("math.floor", Math.floor),
  ceil: toInt("math.ceil", Math.ceil),
  trunc: toInt("math.trunc", Math.trunc),
  fabs: unary("fabs", Math.abs),
  copysign: (x, y) => { N(x, "math.copysign"); N(y, "math.copysign", 2); return (y < 0 || Object.is(y, -0)) ? -Math.abs(x) : Math.abs(x); },
  fmod: (x, y) => { N(x, "math.fmod"); N(y, "math.fmod", 2); return checked2("fmod", x, y, x % y, !(y === 0 || !Number.isFinite(x)) || Number.isNaN(x) || Number.isNaN(y)); },
  remainder: (x, y) => {
    N(x, "math.remainder"); N(y, "math.remainder", 2);
    const ok = !(y === 0 || !Number.isFinite(x)) || Number.isNaN(x) || Number.isNaN(y);
    if (!Number.isFinite(y)) return checked2("remainder", x, y, x, ok);
    let n = Math.round(x / y);
    if (Math.abs(x / y - Math.trunc(x / y)) === 0.5 && n % 2) n -= 1;
    return checked2("remainder", x, y, x - n * y, ok);
  },
  modf: (x) => { N(x, "math.modf"); if (!Number.isFinite(x) && !Number.isNaN(x)) return [x < 0 ? -0 : 0, x]; return [x % 1, Math.trunc(x)]; },
  frexp: (x) => {
    N(x, "math.frexp");
    if (x === 0 || !Number.isFinite(x)) return [x, 0];
    let e = Math.max(-1073, Math.floor(Math.log2(Math.abs(x))) + 1), m = x * 2 ** -e;
    while (Math.abs(m) < 0.5) { m *= 2; e -= 1; }
    while (Math.abs(m) >= 1) { m /= 2; e += 1; }
    return [m, e];
  },
  ldexp: (x, e) => { N(x, "math.ldexp"); I(e, "math.ldexp", 2); e = Math.min(Math.max(e, -10000), 10000); let r = x; while (e > 1000) { r *= 2 ** 1000; e -= 1000; } while (e < -1000) { r *= 2 ** -1000; e += 1000; } return mathChecked("ldexp", x, r * 2 ** e, true); },
  factorial: (n) => { natural(n, "math.factorial"); let r = 1; for (let k = 2; k <= n; k++) r = overflowed(r * k); return r; },
  gcd: (...args) => intsOf(args, "math.gcd").reduce(gcd2, 0),
  lcm: (...args) => { let acc = 1; for (const n of intsOf(args, "math.lcm")) { if (n === 0) return 0; acc = overflowed(acc / gcd2(acc, n) * Math.abs(n)); } return acc; },
  comb: (n, k) => {
    natural(n, "math.comb"); natural(k, "math.comb", 2);
    if (k > n) return 0;
    k = Math.min(k, n - k);
    let r = 1;
    for (let i = 0; i < k; i++) { const g = gcd2(r, i + 1); r = overflowed((r / g) * ((n - i) / ((i + 1) / g))); }
    return r;
  },
  perm: (n, k) => { natural(n, "math.perm"); k = given(k) ? natural(k, "math.perm", 2) : n; if (k > n) return 0; let r = 1; for (let x = n - k + 1; x <= n; x++) r = overflowed(r * x); return r; },
  isqrt: (n) => { natural(n, "math.isqrt"); let s = Math.floor(Math.sqrt(n)); while (s * s > n) s--; while ((s + 1) * (s + 1) <= n) s++; return s; },
  isclose: (a, b, rel, abs) => {
    N(a, "math.isclose"); N(b, "math.isclose", 2);
    rel = given(rel) ? N(rel, "math.isclose", 3) : 1e-9; abs = given(abs) ? N(abs, "math.isclose", 4) : 0;
    if (rel < 0 || abs < 0) fail("`math.isclose`: tolerances must be non-negative");
    if (a === b) return true;
    if (!Number.isFinite(a) || !Number.isFinite(b)) return false;
    const d = Math.abs(a - b);
    return d <= Math.abs(rel * b) || d <= Math.abs(rel * a) || d <= abs;
  },
  isfinite: (x) => Number.isFinite(N(x, "math.isfinite")),
  isinf: (x) => { N(x, "math.isinf"); return x === Infinity || x === -Infinity; },
  isnan: (x) => Number.isNaN(N(x, "math.isnan")),
  prod: (l, start) => {
    let acc = given(start) ? N(start, "math.prod", 2) : 1;
    for (const x of L(l, "math.prod")) { if (typeof x !== "number") fail(`\`math.prod\` expects numbers, got ${typeName(x)}${decNote(x)}`); acc = isInt(acc) && isInt(x) ? overflowed(acc * x) : acc * x; }
    return acc;
  },
  fsum: (l) => fsum(NS(l, "math.fsum")),
  sumprod: (p, q) => dotOf(p, q, "math.sumprod"),
  erf: unary("erf", erf),
  erfc: unary("erfc", erfc),
  gamma: unary("gamma", gamma, pole),
  lgamma: unary("lgamma", lgamma, pole),
};

const dataOf = (l, f, least) => {
  l = NS(l, f);
  if (l.length < least) fail(`\`${f}\` requires ${least === 1 ? "at least one data point" : `at least ${least} data points`}`);
  return l;
};
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
const tallyKey = (v) => typeof v === "number" ? "n" + (v + 0) : typeof v === "string" ? "s" + v : "v" + repr(v);
const tallyOf = (l) => { const at = new Map(), out = []; l.forEach((v, i) => { const k = tallyKey(v); if (at.has(k)) out[at.get(k)][1]++; else { at.set(k, out.length); out.push([i, 1]); } }); return out; };
const normalArgs = (x, mu, sigma, f) => { N(x, f); mu = given(mu) ? N(mu, f, 2) : 0; sigma = given(sigma) ? N(sigma, f, 3) : 1; if (!(sigma > 0)) fail(`\`${f}\`: sigma must be positive, got ${display(sigma)}`); return [x, mu, sigma]; };
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
const countOf = (n, f) => { I(n, f); if (n < 0 || n > 1e7) fail(`\`${f}\`: ${n} is not a count from 0 to 10000000`); return n; };

B.stats = {
  mean: (l) => meanOf(dataOf(l, "stats.mean", 1)),
  fmean: (l, w) => {
    const x = dataOf(l, "stats.fmean", 1);
    if (!given(w)) return meanOf(x);
    w = NS(w, "stats.fmean", 2);
    if (w.length !== x.length) fail("`stats.fmean`: data and weights must be the same length");
    const total = csum(w);
    if (total === 0) fail("`stats.fmean`: the sum of the weights must be non-zero");
    return cdot(x, w) / total;
  },
  geometric_mean: (l) => { const x = dataOf(l, "stats.geometric_mean", 1); if (x.some((v) => v <= 0)) fail("`stats.geometric_mean` requires positive numbers"); return Math.exp(csum(x.map(Math.log)) / x.length); },
  harmonic_mean: (l) => {
    const x = dataOf(l, "stats.harmonic_mean", 1);
    if (x.some((v) => v < 0)) fail("`stats.harmonic_mean` does not support negative values");
    if (x.some((v) => v === 0)) return 0;
    const r = x.map((v) => 1 / v), e = x.map((v, i) => { const [p, pe] = twoProd(r[i], v); return ((1 - p) - pe) / v; });
    return x.length / csum(r.concat(e));
  },
  median: (l) => { const d = sortedOf(dataOf(l, "stats.median", 1)), n = d.length; return n % 2 ? d[n >> 1] : (d[n / 2 - 1] + d[n / 2]) / 2; },
  median_low: (l) => { const d = sortedOf(dataOf(l, "stats.median_low", 1)), n = d.length; return d[n % 2 ? n >> 1 : n / 2 - 1]; },
  median_high: (l) => { const x = dataOf(l, "stats.median_high", 1); return kth(x, x.length >> 1); },
  mode: (l) => { l = L(l, "stats.mode"); const t = tallyOf(l); if (!t.length) fail("`stats.mode`: no mode for empty data"); const most = Math.max(...t.map((x) => x[1])); return l[t.find((x) => x[1] === most)[0]]; },
  multimode: (l) => { l = L(l, "stats.multimode"); const t = tallyOf(l), most = Math.max(0, ...t.map((x) => x[1])); return t.filter((x) => x[1] === most).map((x) => l[x[0]]); },
  pvariance: (l, mu) => varianceOf(l, mu, "stats.pvariance", false),
  variance: (l, mu) => varianceOf(l, mu, "stats.variance", true),
  pstdev: (l, mu) => Math.sqrt(varianceOf(l, mu, "stats.pstdev", false)),
  stdev: (l, mu) => Math.sqrt(varianceOf(l, mu, "stats.stdev", true)),
  quantiles: (l, n, method) => {
    const f = "stats.quantiles", d = sortedOf(dataOf(l, f, 1)), ld = d.length;
    n = given(n) ? I(n, f, 2) : 4;
    if (n < 1 || n > 10000) fail(`\`${f}\`: n must be from 1 to 10000, got ${n}`);
    method = given(method) ? S(method, f, 3) : "exclusive";
    if (ld === 1) return new Array(n - 1).fill(d[0]);
    const out = [];
    if (method === "inclusive") {
      const m = ld - 1;
      for (let i = 1; i < n; i++) { const j = Math.floor(i * m / n), delta = i * m - j * n; out.push((d[j] * (n - delta) + d[j + 1] * delta) / n); }
    } else if (method === "exclusive") {
      const m = ld + 1;
      for (let i = 1; i < n; i++) { const j = Math.min(Math.max(Math.floor(i * m / n), 1), ld - 1), delta = i * m - j * n; out.push((d[j - 1] * (n - delta) + d[j] * delta) / n); }
    } else fail(`\`${f}\`: unknown method ${JSON.stringify(method)} (exclusive or inclusive)`);
    return out;
  },
  percentile: (l, p) => {
    const f = "stats.percentile", d = sortedOf(dataOf(l, f, 1));
    const check = (p) => { if (!(p >= 0 && p <= 100)) fail(`\`${f}\`: percentiles are from 0 to 100, got ${display(p)}`); return p; };
    if (Array.isArray(p)) return NS(p, f, 2).map((q) => atPercent(d, check(q)));
    return atPercent(d, check(N(p, f, 2)));
  },
  covariance: (x, y) => { [x, y] = pairedOf(x, y, "stats.covariance"); return momentsOf(x, y)[4] / (x.length - 1); },
  correlation: (x, y, method) => {
    const f = "stats.correlation";
    [x, y] = pairedOf(x, y, f);
    method = given(method) ? S(method, f, 3) : "linear";
    if (method === "ranked") { x = ranksOf(x); y = ranksOf(y); } else if (method !== "linear") fail(`\`${f}\`: unknown method ${JSON.stringify(method)} (linear or ranked)`);
    const [, , sxx, syy, sxy] = momentsOf(x, y);
    if (sxx === 0 || syy === 0) fail(`\`${f}\`: at least one of the inputs is constant`);
    return Math.min(1, Math.max(-1, sxy / Math.sqrt(sxx * syy)));
  },
  linear_regression: (x, y, proportional) => {
    const f = "stats.linear_regression";
    [x, y] = pairedOf(x, y, f);
    if (truthy(proportional)) { const sxx = cdot(x, x); if (sxx === 0) fail(`\`${f}\`: x is all zeros`); return { slope: cdot(x, y) / sxx, intercept: 0 }; }
    const [mx, my, sxx, , sxy] = momentsOf(x, y);
    if (sxx === 0) fail(`\`${f}\`: x is constant`);
    const slope = sxy / sxx;
    return { slope, intercept: my - slope * mx };
  },
  describe: (l) => {
    const x = NS(l, "stats.describe"), n = x.length, total = csum(x), d = sortedOf(x), q = (p) => n ? atPercent(d, p) : null;
    const mean = n ? total / n : null;
    return { count: n, sum: total, mean, std: n > 1 ? Math.sqrt(div2(ssOf(x, mean), n - 1)) : null, min: n ? d[0] : null, q1: q(25), median: q(50), q3: q(75), max: n ? d[n - 1] : null };
  },
  histogram: (l, bins) => {
    const f = "stats.histogram", x = NS(l, f);
    let edges;
    if (Array.isArray(bins)) {
      edges = NS(bins, f, 2);
      if (edges.length < 2 || edges.some((e, i) => i && e < edges[i - 1])) fail(`\`${f}\`: edges must be at least two, increasing`);
    } else {
      bins = given(bins) ? I(bins, f, 2) : 10;
      if (bins < 1 || bins > 1e6) fail(`\`${f}\`: bins must be from 1 to 1000000, got ${bins}`);
      let lo = Infinity, hi = -Infinity;
      for (const v of x) { lo = Math.min(lo, v); hi = Math.max(hi, v); }
      if (!x.length) [lo, hi] = [0, 1];
      if (lo === hi) [lo, hi] = [lo - 0.5, hi + 0.5];
      edges = linspaceOf(lo, hi, bins + 1, true);
    }
    const nb = edges.length - 1, lo = edges[0], hi = edges[nb], counts = new Array(nb).fill(0);
    for (const v of x) {
      if (!(v >= lo && v <= hi)) continue;
      let a = 0, b = nb;
      while (a < b) { const m = (a + b) >> 1; if (edges[m + 1] <= v) a = m + 1; else b = m; }
      counts[Math.min(a, nb - 1)]++;
    }
    return { counts, edges };
  },
  zscore: (l) => { const x = NS(l, "stats.zscore"); if (!x.length) return []; const m = meanOf(x), sd = Math.sqrt(div2(ssOf(x, m), x.length)); return x.map((v) => sd === 0 ? 0 : (v - m) / sd); },
  normalize: (l) => { const x = NS(l, "stats.normalize"); let lo = Infinity, hi = -Infinity; for (const v of x) { lo = Math.min(lo, v); hi = Math.max(hi, v); } const w = hi - lo; return x.map((v) => w > 0 ? (v - lo) / w : 0); },
  rank: (l) => ranksOf(NS(l, "stats.rank")),
  normal_pdf: (x, mu, sigma) => { [x, mu, sigma] = normalArgs(x, mu, sigma, "stats.normal_pdf"); const z = (x - mu) / sigma; return Math.exp(-0.5 * z * z) / (sigma * Math.sqrt(2 * Math.PI)); },
  normal_cdf: (x, mu, sigma) => { [x, mu, sigma] = normalArgs(x, mu, sigma, "stats.normal_cdf"); return 0.5 * (1 + erf((x - mu) / (sigma * Math.SQRT2))); },
  normal_inv: (p, mu, sigma) => {
    [p, mu, sigma] = normalArgs(p, mu, sigma, "stats.normal_inv");
    if (!(p > 0 && p < 1)) fail(`\`stats.normal_inv\`: p must be between 0 and 1 (exclusive), got ${display(p)}`);
    return mu + invCdf(p) * sigma;
  },
};

const vecArith = (op, f) => (a, b) => {
  if (typeof a !== "number" || typeof b !== "number") fail(`\`${f}\` expects numbers, got ${typeName(a)} and ${typeName(b)}${decNote(b) || decNote(a)}`);
  if ((op === "/" || op === "%") && b === 0 && isInt(a) && isInt(b)) fail(`\`${f}\`: division by zero`);
  const r = op === "+" ? a + b : op === "-" ? a - b : op === "*" ? a * b : op === "%" ? a % b : a / b;
  return isInt(a) && isInt(b) && Number.isInteger(r) ? overflowed(r) : r;
};
const elementwise = (op, f) => (a, b) => {
  const g = vecArith(op, f);
  if (Array.isArray(a) && Array.isArray(b)) { if (a.length !== b.length) fail(`\`${f}\` needs lists of the same length (${a.length} and ${b.length})`); return a.map((x, i) => g(x, b[i])); }
  if (Array.isArray(a)) return a.map((x) => g(x, b));
  if (Array.isArray(b)) return b.map((y) => g(a, y));
  return g(a, b);
};
const accumulate = (op, f) => (l) => { const g = vecArith(op, f), out = []; for (const x of L(l, f)) out.push(out.length ? g(out[out.length - 1], x) : (typeof x === "number" ? x : g(x, 0))); return out; };
const argExtreme = (f, better) => (l) => { const x = NS(l, f); let best = null; x.forEach((v, i) => { if (!Number.isNaN(v) && (best === null || better(v, x[best]))) best = i; }); return best; };
const rollingOf = (l, w, f) => {
  const x = NS(l, f);
  I(w, f, 2);
  if (w < 1) fail(`\`${f}\`: the window must be at least 1, got ${w}`);
  const out = [];
  for (let i = 0; i + w <= x.length; i++) out.push(csum(x.slice(i, i + w)));
  return out;
};
const matrixOf = (rows, f, i = 1) => {
  rows = L(rows, f, i);
  const cols = rows.length ? (Array.isArray(rows[0]) ? rows[0].length : fail(`\`${f}\` expects a list of rows, but row 0 is ${typeName(rows[0])}`)) : 0;
  rows.forEach((r, k) => {
    if (!Array.isArray(r)) fail(`\`${f}\` expects a list of rows, but row ${k} is ${typeName(r)}`);
    if (r.length !== cols) fail(`\`${f}\`: row ${k} has ${r.length} numbers, row 0 has ${cols}`);
    r.forEach((v) => { if (typeof v !== "number") fail(`\`${f}\` expects numbers, but row ${k} holds ${typeName(v)}${decNote(v)}`); });
  });
  return [rows, cols];
};

B.vec = {
  add: elementwise("+", "vec.add"),
  sub: elementwise("-", "vec.sub"),
  mul: elementwise("*", "vec.mul"),
  div: elementwise("/", "vec.div"),
  mod: elementwise("%", "vec.mod"),
  pow: (l, p) => { const x = NS(l, "vec.pow"); N(p, "vec.pow", 2); return x.map((v) => v ** p); },
  dot: (a, b) => dotOf(a, b, "vec.dot"),
  norm: (l, p) => {
    const x = NS(l, "vec.norm");
    p = given(p) ? N(p, "vec.norm", 2) : 2;
    if (p === 2) return norm2(x);
    if (p === 1) return csum(x.map(Math.abs));
    if (p === Infinity) return x.reduce((m, v) => Math.max(m, Math.abs(v)), 0);
    return csum(x.map((v) => Math.abs(v) ** p)) ** (1 / p);
  },
  cumsum: accumulate("+", "vec.cumsum"),
  cumprod: accumulate("*", "vec.cumprod"),
  diff: (l) => { l = L(l, "vec.diff"); const g = vecArith("-", "vec.diff"), out = []; for (let i = 1; i < l.length; i++) out.push(g(l[i], l[i - 1])); return out; },
  argmin: argExtreme("vec.argmin", (a, b) => a < b),
  argmax: argExtreme("vec.argmax", (a, b) => a > b),
  argsort: (l, desc) => { const x = NS(l, "vec.argsort"), d = truthy(desc); return x.map((_, i) => i).sort((a, b) => (d ? x[b] - x[a] : x[a] - x[b]) || a - b); },
  clip: (l, lo, hi) => {
    const x = NS(l, "vec.clip");
    if (given(lo)) N(lo, "vec.clip", 2);
    if (given(hi)) N(hi, "vec.clip", 3);
    return x.map((v) => given(lo) && v < lo ? lo : given(hi) && v > hi ? hi : v);
  },
  abs: (l) => NS(l, "vec.abs").map(Math.abs),
  sqrt: (l) => NS(l, "vec.sqrt").map(Math.sqrt),
  exp: (l) => NS(l, "vec.exp").map(Math.exp),
  log: (l) => NS(l, "vec.log").map(Math.log),
  round: (l, d) => {
    const x = NS(l, "vec.round");
    if (!given(d)) return x.map(roundHalf);
    const p = 10 ** Math.min(Math.max(I(d, "vec.round", 2), 0), 15);
    return x.map((v) => roundHalf(v * p) / p);
  },
  linspace: (a, b, n, endpoint) => { N(a, "vec.linspace"); N(b, "vec.linspace", 2); return linspaceOf(a, b, countOf(given(n) ? n : 50, "vec.linspace"), given(endpoint) ? truthy(endpoint) : true); },
  arange: (start, stop, step) => {
    const f = "vec.arange";
    if (!given(stop)) [start, stop] = [0, start];
    N(start, f); N(stop, f, 2);
    step = given(step) ? N(step, f, 3) : 1;
    if (step === 0 || !Number.isFinite(step)) fail(`\`${f}\`: step must be a non-zero number`);
    const n = countOf(Math.max(0, Math.ceil((stop - start) / step)), f), out = [];
    for (let i = 0; i < n; i++) out.push(start + i * step);
    return out;
  },
  zeros: (n) => new Array(countOf(n, "vec.zeros")).fill(0),
  ones: (n) => new Array(countOf(n, "vec.ones")).fill(1),
  full: (n, v) => new Array(countOf(n, "vec.full")).fill(v === undefined ? null : v),
  rolling_mean: (l, w) => rollingOf(l, w, "vec.rolling_mean").map((s) => s / w),
  rolling_sum: (l, w) => rollingOf(l, w, "vec.rolling_sum"),
  matmul: (a, b) => {
    const f = "vec.matmul", [A, k] = matrixOf(a, f);
    if (Array.isArray(b) && b.length && !Array.isArray(b[0])) {
      const v = NS(b, f, 2);
      if (v.length !== k) fail(`\`${f}\`: a has ${k} columns, b has ${v.length} numbers`);
      return A.map((r) => cdot(r, v));
    }
    const [Bm, m] = matrixOf(b, f, 2);
    if (Bm.length !== k) fail(`\`${f}\`: a has ${k} columns, b has ${Bm.length} rows`);
    return A.map((r) => { const out = new Array(m).fill(0); r.forEach((x, p) => { for (let j = 0; j < m; j++) out[j] += x * Bm[p][j]; }); return out; });
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
function dtZone(v, f, i) {
  if (isNil(v)) return 0;
  if (typeof v !== "string") fail(`\`${f}\` expects a time zone (like "Europe/Paris") as argument ${i}, got ${dtGot(v)}`);
  if (v.toUpperCase() === "UTC" || v === "Z") return 0;
  if (v[0] === "+" || v[0] === "-") { const o = dtOffset(v); if (o !== null) return o; }
  let fmt = dtZones.get(v);
  if (!fmt) {
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
function dtLocalInstant(zone, local) {
  const before = dtOffsetAt(zone, local - 86400), after = dtOffsetAt(zone, local + 86400);
  const fits = (o) => dtOffsetAt(zone, local - o) === o;
  const a = fits(before), b = fits(after);
  if (a && b) return local - Math.max(before, after);
  if (b) return local - after;
  return local - before;
}
const dtClock = () => Math.floor(Date.now() / 1000);

// { weekend: [6, 7], holidays: ["2026-12-25"] }
function dtCalendar(v, f, i) {
  const cal = { weekend: [false, false, false, false, false, false, true, true], holidays: new Set() };
  if (isNil(v)) return cal;
  if (!isMap(v)) fail(`\`${f}\` expects a calendar ({ weekend, holidays }) as argument ${i}, got ${dtGot(v)}`);
  for (const k of Object.keys(v)) {
    const x = v[k];
    if (k !== "weekend" && k !== "holidays") fail(`\`${f}\`: a calendar has \`weekend\` and \`holidays\`, not \`${k}\``);
    if (isNil(x)) continue;
    if (!Array.isArray(x)) fail(`\`${f}\`: a calendar's ${k} is a list, got ${dtGot(x)}`);
    if (k === "weekend") {
      cal.weekend = [false, false, false, false, false, false, false, false];
      for (const d of x) {
        if (!isInt(d) || d < 1 || d > 7) fail(`\`${f}\`: a calendar's weekend is days 1 (Monday) to 7 (Sunday), got ${dtGot(d)}`);
        cal.weekend[d] = true;
      }
      if (cal.weekend.slice(1).every((w) => w)) fail(`\`${f}\`: a calendar's weekend cannot be the whole week`);
    } else {
      for (const h of x) {
        const z = dtDate(h);
        if (z === null) fail(`\`${f}\`: a calendar's holidays are dates (YYYY-MM-DD), got ${dtGot(h)}`);
        cal.holidays.add(z);
      }
    }
  }
  return cal;
}
const dtOpen = (cal, z) => !cal.weekend[dtWeekday(z)] && !cal.holidays.has(z);
const dtStep = (z, by) => z === null || !dtInRange(dtCivil(z + by)[0]) ? null : z + by;
function dtRoll(cal, z, by) {
  while (z !== null && !dtOpen(cal, z)) z = dtStep(z, by);
  return z;
}
// The business day conventions (ISDA 2006, section 4.12); undefined for an unknown one.
function dtAdjust(cal, z, how) {
  const month = (x) => dtCivil(x)[1];
  const modified = (by) => { const x = dtRoll(cal, z, by); return x !== null && month(x) === month(z) ? x : dtRoll(cal, z, -by); };
  switch (how) {
    case "unadjusted": return z;
    case "following": return dtRoll(cal, z, 1);
    case "modified_following": return modified(1);
    case "preceding": return dtRoll(cal, z, -1);
    case "modified_preceding": return modified(-1);
  }
  return undefined;
}

// Day counts: [days as the convention counts them, the year fraction].
function dtBasis(v, f) {
  const n = dtStr(v, f, 3).trim().toUpperCase().replaceAll("ACTUAL", "ACT");
  switch (n) {
    case "ACT/360": return "act360";
    case "ACT/365": case "ACT/365F": case "ACT/365 FIXED": return "act365";
    case "ACT/ACT": case "ACT/ACT ISDA": return "actact";
    case "30/360": case "30/360 ISDA": case "360/360": case "BOND BASIS": return "bond";
    case "30U/360": case "30/360 US": return "us";
    case "30E/360": case "EUROBOND BASIS": return "euro";
  }
  fail(`\`${f}\`: unknown day count convention ${dtGot(v)} (ACT/360, ACT/365, ACT/ACT, 30/360, 30U/360 or 30E/360)`);
}
function dtCount(c, a, b) {
  const [y1, m1, d1] = dtCivil(a), [y2, m2, d2] = dtCivil(b), act = b - a;
  const thirty = (e1, e2) => { const n = 360 * (y2 - y1) + 30 * (m2 - m1) + (e2 - e1); return [n, n / 360]; };
  switch (c) {
    case "act360": return [act, act / 360];
    case "act365": return [act, act / 365];
    case "actact": {
      const base = (y) => dtLeap(y) ? 366 : 365;
      if (y1 === y2) return [act, act / base(y1)];
      return [act, (dtDays(y1 + 1, 1, 1) - a) / base(y1) + (y2 - y1 - 1) + (b - dtDays(y2, 1, 1)) / base(y2)];
    }
    case "bond": { const e1 = d1 === 31 ? 30 : d1; return thirty(e1, d2 === 31 && e1 === 30 ? 30 : d2); }
    case "us": {
      const febEnd = (y, m, d) => m === 2 && d === dtMonthLen(y, 2);
      const f1 = febEnd(y1, m1, d1), f2 = febEnd(y2, m2, d2);
      let e2 = f1 && f2 ? 30 : d2;
      const e1 = f1 || d1 === 31 ? 30 : d1;
      if (e2 === 31 && e1 === 30) e2 = 30;
      return thirty(e1, e2);
    }
    case "euro": return thirty(Math.min(d1, 30), Math.min(d2, 30));
  }
}
function dtCounted(a, b, c, f) {
  a = dtDateArg(a, f, 1); b = dtDateArg(b, f, 2); c = dtBasis(c, f);
  if (b < a) { const [n, x] = dtCount(c, b, a); return [-n, -x]; }
  return dtCount(c, a, b);
}

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
const dtLocales = [dtEn, {
  tags: ["en-gb", "en-ie", "en-au", "en-nz", "en-in", "en-za"],
  months: dtEn.months,
  mon: ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sept", "Oct", "Nov", "Dec"],
  days: dtEn.days, dy: dtEn.dy,
  styles: ["DD/MM/YYYY", "D MMM YYYY", "D MMMM YYYY", "dddd, D MMMM YYYY"],
}, {
  tags: ["de"],
  months: ["Januar", "Februar", "März", "April", "Mai", "Juni", "Juli", "August", "September", "Oktober", "November", "Dezember"],
  mon: ["Jan.", "Feb.", "März", "Apr.", "Mai", "Juni", "Juli", "Aug.", "Sept.", "Okt.", "Nov.", "Dez."],
  days: ["Montag", "Dienstag", "Mittwoch", "Donnerstag", "Freitag", "Samstag", "Sonntag"],
  dy: ["Mo.", "Di.", "Mi.", "Do.", "Fr.", "Sa.", "So."],
  styles: ["DD.MM.YY", "DD.MM.YYYY", "D. MMMM YYYY", "dddd, D. MMMM YYYY"],
}, {
  tags: ["fr"],
  months: ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"],
  mon: ["janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc."],
  days: ["lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche"],
  dy: ["lun.", "mar.", "mer.", "jeu.", "ven.", "sam.", "dim."],
  styles: ["DD/MM/YYYY", "D MMM YYYY", "D MMMM YYYY", "dddd D MMMM YYYY"],
}, {
  tags: ["es"],
  months: ["enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre", "octubre", "noviembre", "diciembre"],
  mon: ["ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sept", "oct", "nov", "dic"],
  days: ["lunes", "martes", "miércoles", "jueves", "viernes", "sábado", "domingo"],
  dy: ["lun", "mar", "mié", "jue", "vie", "sáb", "dom"],
  styles: ["D/M/YY", "D MMM YYYY", "D [de] MMMM [de] YYYY", "dddd, D [de] MMMM [de] YYYY"],
}, {
  tags: ["it"],
  months: ["gennaio", "febbraio", "marzo", "aprile", "maggio", "giugno", "luglio", "agosto", "settembre", "ottobre", "novembre", "dicembre"],
  mon: ["gen", "feb", "mar", "apr", "mag", "giu", "lug", "ago", "set", "ott", "nov", "dic"],
  days: ["lunedì", "martedì", "mercoledì", "giovedì", "venerdì", "sabato", "domenica"],
  dy: ["lun", "mar", "mer", "gio", "ven", "sab", "dom"],
  styles: ["DD/MM/YY", "D MMM YYYY", "D MMMM YYYY", "dddd D MMMM YYYY"],
}, {
  tags: ["nl"],
  months: ["januari", "februari", "maart", "april", "mei", "juni", "juli", "augustus", "september", "oktober", "november", "december"],
  mon: ["jan", "feb", "mrt", "apr", "mei", "jun", "jul", "aug", "sep", "okt", "nov", "dec"],
  days: ["maandag", "dinsdag", "woensdag", "donderdag", "vrijdag", "zaterdag", "zondag"],
  dy: ["ma", "di", "wo", "do", "vr", "za", "zo"],
  styles: ["DD-MM-YYYY", "D MMM YYYY", "D MMMM YYYY", "dddd D MMMM YYYY"],
}, {
  tags: ["pt"],
  months: ["janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro", "dezembro"],
  mon: ["jan.", "fev.", "mar.", "abr.", "mai.", "jun.", "jul.", "ago.", "set.", "out.", "nov.", "dez."],
  days: ["segunda-feira", "terça-feira", "quarta-feira", "quinta-feira", "sexta-feira", "sábado", "domingo"],
  dy: ["seg.", "ter.", "qua.", "qui.", "sex.", "sáb.", "dom."],
  styles: ["DD/MM/YYYY", "D [de] MMM [de] YYYY", "D [de] MMMM [de] YYYY", "dddd, D [de] MMMM [de] YYYY"],
}];
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
  const off = o.length === 1 ? dtOffset(o.toUpperCase()) : o.length === 3 ? dtOffset(o + ":00")
    : o.length === 5 && !o.includes(":") ? dtOffset(o.slice(0, 3) + ":" + o.slice(3)) : dtOffset(o);
  return off === null ? null : dtStampText(day * 86400 + sec - off, frac, off);
}

B.date = {
  today: (tz) => { const z = dtZone(tz, "date.today", 1), t = dtClock(); return dtText(Math.floor((t + dtOffsetAt(z, t)) / 86400)); },
  now: (tz) => { const z = dtZone(tz, "date.now", 1), t = dtClock(); return dtStampText(t, "", dtOffsetAt(z, t)) || ""; },
  parse: (s, pattern) => {
    dtStr(s, "date.parse", 1);
    if (isNil(pattern)) { const r = dtReadIso(s.trim()); return r === null ? new Err(`not a date or a date-time: ${dtGot(s)}`) : r; }
    if (typeof pattern !== "string") fail(`\`date.parse\` expects a pattern (like "DD/MM/YYYY") as argument 2, got ${dtGot(pattern)}`);
    const r = dtRead(Array.from(s.trim()), Array.from(pattern));
    return r === null ? new Err(`${dtGot(s)} is not a date in the form ${dtGot(pattern)}`) : r;
  },
  valid: (s) => dtDate(s) !== null || dtStamp(s) !== null,
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
    return dtStampText(i, t.frac, dtOffsetAt(z, i)) ?? dtOutOfRange("date.to_zone");
  },
  date_in: (v, tz) => {
    const t = dtStampArg(v, "date.date_in", 1), z = dtZone(tz, "date.date_in", 2), i = dtInstant(t);
    return dtDay(Math.floor((i + dtOffsetAt(z, i)) / 86400), "date.date_in");
  },
  at: (v, time, tz) => {
    const f = "date.at", day = dtDateArg(v, f, 1);
    let sec = 0;
    if (!isNil(time)) {
      sec = typeof time === "string" && time.length === 5 ? dtHms(time + ":00") : typeof time === "string" && time.length === 8 ? dtHms(time) : null;
      if (sec === null) fail(`\`${f}\` expects a time (HH:MM or HH:MM:SS) as argument 2, got ${dtGot(time)}`);
    }
    const z = dtZone(tz, f, 3), i = dtLocalInstant(z, day * 86400 + sec);
    return dtStampText(i, "", dtOffsetAt(z, i)) ?? dtOutOfRange(f);
  },
  is_business_day: (v, cal) => { const z = dtDateArg(v, "date.is_business_day", 1); return dtOpen(dtCalendar(cal, "date.is_business_day", 2), z); },
  add_business_days: (v, n, cal) => {
    const f = "date.add_business_days";
    let z = dtDateArg(v, f, 1);
    dtInt(n, f, 2);
    cal = dtCalendar(cal, f, 3);
    if (Math.abs(n) > 4000000) dtOutOfRange(f);
    const by = n < 0 ? -1 : 1;
    for (let k = 0; k < Math.abs(n); k++) z = dtRoll(cal, dtStep(z, by), by);
    return dtDay(dtRoll(cal, z, 1), f);
  },
  business_days_between: (a, b, cal) => {
    const f = "date.business_days_between";
    a = dtDateArg(a, f, 1); b = dtDateArg(b, f, 2); cal = dtCalendar(cal, f, 3);
    let n = 0;
    for (let z = Math.min(a, b); z < Math.max(a, b); z++) if (dtOpen(cal, z)) n++;
    return b < a ? -n : n;
  },
  adjust: (v, how, cal) => {
    const f = "date.adjust", z = dtDateArg(v, f, 1);
    if (!isNil(how) && typeof how !== "string") fail(`\`${f}\` expects a convention as argument 2, got ${dtGot(how)}`);
    const r = dtAdjust(dtCalendar(cal, f, 3), z, isNil(how) ? "following" : how);
    if (r === undefined) fail(`\`${f}\`: unknown convention ${dtGot(how)} (following, modified_following, preceding, modified_preceding or unadjusted)`);
    return dtDay(r, f);
  },
  easter: (y) => {
    dtInt(y, "date.easter", 1);
    if (y < 1583 || y > 9999) fail(`\`date.easter\` is for Gregorian years (1583 to 9999), got ${y}`);
    const a = y % 19, b = Math.floor(y / 100), c = y % 100, d = Math.floor(b / 4), e = b % 4;
    const g = Math.floor((b - Math.floor((b + 8) / 25) + 1) / 3), h = (19 * a + b - d - g + 15) % 30;
    const l = (32 + 2 * e + 2 * Math.floor(c / 4) - h - c % 4) % 7, m = Math.floor((a + 11 * h + 22 * l) / 451), n = h + l - 7 * m + 114;
    return dtText(dtDays(y, Math.floor(n / 31), n % 31 + 1));
  },
  day_count: (a, b, c) => dtCounted(a, b, c, "date.day_count")[0],
  year_fraction: (a, b, c) => dtCounted(a, b, c, "date.year_fraction")[1],
  format: (v, how) => {
    const f = "date.format", [z, t] = dtEitherArg(v, f, 1);
    let pattern, n = dtEn;
    if (typeof how === "string") pattern = how;
    else if (isMap(how)) {
      const has = (k) => hasOwn(how, k), loc = has("locale") ? how.locale : null;
      if (!isNil(loc)) { if (typeof loc !== "string") fail(`\`${f}\`: locale is a str, like "de-DE", got ${dtGot(loc)}`); n = dtNames(loc); }
      const k = Object.keys(how).find((k) => k !== "locale" && k !== "style" && k !== "pattern");
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

// ---- channels -------------------------------------------------------------
// `channel.open(name, f)` in a component: `f` hears what the server sends on
// the channel for as long as the code that opened it stands, and `.send(v)`
// gives the channel's handler on the server a message. A page's channels
// share one connection: WebTransport where the browser has it (small
// messages go as datagrams, which may be lost or come out of order; larger
// and reliable ones on a stream), else a WebSocket. It is opened again when
// it drops, and the channels joined again.

const Chan = { subs: new Map(), media: new Map(), joined: new Set(), waiting: new Map(), conn: null, running: false, wt: true };

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
        if (typeof window !== "undefined") window.__quilloChannelVia = c.via;
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
  const r = await fetch(BASE + "/_quillo/wt-ticket", { method: "POST" });
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
    const s = new WebSocket(location.origin.replace(/^http/, "ws") + BASE + "/_quillo/wt" + q);
    s.binaryType = "arraybuffer";
    let ended;
    const closed = new Promise((r) => (ended = r));
    s.onopen = () => ok({ via: "websocket", put: (text) => s.send(text), closed });
    s.onmessage = (m) => { if (typeof m.data === "string") chanGot(m.data); else chanFrame(new Uint8Array(m.data)); };
    s.onclose = () => { ended(); no(new Error("closed")); };
  });
}

// `["j", name, yes]`: the answer to joining; `["m", name, value]`: a message.
function chanGot(text) {
  let m;
  try { m = JSON.parse(text); } catch (e) { return; }
  const [k, name, v] = m, subs = Chan.subs.get(name);
  if (!subs) return;
  if (k === "j") {
    if (!v) { console.warn("quillo: the server did not let this page join channel " + name); return; }
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
      if (!this.player) this.player = mediaPlayer(this);
    }
    disconnectedCallback() {
      if (this.player) this.player.stop();
      this.player = null;
    }
  });
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

function idemKey() {
  if (typeof crypto !== "undefined" && crypto.randomUUID) return crypto.randomUUID();
  const b = new Uint8Array(16);
  crypto.getRandomValues(b);
  return Array.from(b, (x) => x.toString(16).padStart(2, "0")).join("");
}

async function rpcRaw(name, args, live, idem) {
  let r;
  const headers = { "content-type": "application/json" };
  if (live) headers["x-quillo-live"] = "1";
  if (idem) headers["idempotency-key"] = idemKey();
  // Files (from an <input type=file>) go as parts of a multipart body, the
  // other arguments as JSON beside them.
  let payload;
  if (args.some((a) => typeof Blob !== "undefined" && a instanceof Blob)) {
    payload = new FormData();
    payload.append("args", JSON.stringify(args.map((a, i) => a instanceof Blob ? { $quillo_file: i } : toJSON(a, true))));
    args.forEach((a, i) => { if (a instanceof Blob) payload.append("file" + i, a, a.name || "file"); });
    delete headers["content-type"];
    headers["x-quillo-rpc"] = "1";
  } else {
    payload = JSON.stringify(args.map((a) => toJSON(a, true)));
  }
  // A call that fails (no connection, a server error) is an error value,
  // as with http.* on the server, so `??` and `is_err` handle it.
  const failed = (msg) => ({ value: new Err(msg), topics: [], seq: 0, server: "", failed: true });
  for (let tries = idem ? 4 : 1; ; ) {
    try {
      r = await fetch(BASE + "/_quillo/rpc/" + encodeURIComponent(name), {
        method: "POST",
        headers,
        body: payload,
      });
    } catch (e) {
      r = null;
      if (--tries <= 0) return failed(`server fn \`${name}\` could not be reached: ${e.message}`);
    }
    if (r && !(idem && RETRIED.includes(r.status) && --tries > 0)) break;
    // 0.25 s, 0.5 s, 1 s, give or take a quarter.
    await new Promise((ok) => setTimeout(ok, 250 * 2 ** (3 - tries) * (0.75 + Math.random() / 2)));
  }
  let body = null;
  try { body = untagDeep(await r.json()); } catch (_) {}
  if (!r.ok) return failed(`server fn \`${name}\` failed (${r.status})${body && body.error ? ": " + body.error : ""}`);
  const value = body && hasOwn(body, "err") ? new Err(body.err) : body && hasOwn(body, "ok") ? body.ok : null;
  return { value, topics: (body && body.topics) || [], seq: (body && body.seq) || 0, server: (body && body.server) || "" };
}

async function rpc(name, args, idem) {
  return (await rpcRaw(name, args, false, idem)).value;
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

// Starts waiting, or starts again when the set of topics changed.
function liveRestart() {
  queueMicrotask(() => {
    if (!Live.running) {
      Live.running = true;
      liveLoop();
    } else if (liveTopics().join(",") !== Live.key && Live.ctrl) {
      Live.ctrl.abort();
    }
  });
}

async function liveLoop() {
  for (;;) {
    const topics = liveTopics();
    Live.key = topics.join(",");
    if (!topics.length) { Live.running = false; return; }
    const ctrl = (Live.ctrl = new AbortController());
    const url = BASE + "/_quillo/live?since=" + Live.seq + "&s=" + Live.server + "&t=" + encodeURIComponent(Live.key);
    try {
      if (!(await liveSocket(url, ctrl.signal)) && !ctrl.signal.aborted)
        liveChanged(await (await fetch(url, { signal: ctrl.signal, cache: "no-store" })).json());
    } catch (e) {
      if (!ctrl.signal.aborted) await new Promise((r) => setTimeout(r, 1000));
    }
  }
}

// Resolves when the socket closes: true if it had opened.
function liveSocket(url, signal) {
  return new Promise((resolve) => {
    if (typeof WebSocket !== "function") return resolve(false);
    const ws = new WebSocket(location.origin.replace(/^http/, "ws") + url);
    let opened = false;
    ws.onopen = () => { opened = true; };
    ws.onmessage = (m) => { if (!signal.aborted) liveChanged(JSON.parse(m.data)); };
    ws.onclose = () => resolve(opened);
    // Carry on at once: a closing socket finishes closing in its own time
    // (the browser waits for the server's answer), and says nothing more.
    signal.addEventListener("abort", () => {
      ws.close();
      resolve(true);
    });
  });
}

function liveChanged(j) {
  Live.seq = j.seq;
  Live.server = j.server || "";
  const changed = new Set(j.changed || []);
  if (changed.size) for (const s of [...Live.subs]) if (s.topics.some((t) => changed.has(t))) liveRefresh(s);
}

async function liveRefresh(sub) {
  // Only the latest load counts: an older one may answer last.
  const n = (sub.loads = (sub.loads || 0) + 1);
  try {
    const { value, topics, seq, server, failed } = await rpcRaw(sub.fname, sub.args, true);
    if (n !== sub.loads) return;
    // Keep showing the last good data; the next change tries again.
    if (failed) { report(value.error); return; }
    liveSeen(seq, server);
    sub.topics = topics;
    sub.sig.set(value);
    liveRestart();
  } catch (e) {
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
  constructor(v) { this._v = v; this.obs = null; this.keys = null; }
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
    if (c.effect) { report(e); return; }
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
  let rounds = 0;
  while (effectQueue.length) {
    if (++rounds > 1000) {
      effectQueue = [];
      report("updates go round in a loop: something sets what it reads");
      break;
    }
    const q = effectQueue;
    effectQueue = [];
    // In the order they were made (mostly so already).
    for (let i = 1; i < q.length; i++) if (q[i - 1].id > q[i].id) { q.sort((a, b) => a.id - b.id); break; }
    for (const c of q) if (!c.dead) fresh(c);
  }
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

const SVG = "http://www.w3.org/2000/svg";
const inSvg = (el) => el.namespaceURI === SVG && el.localName !== "foreignObject";
const isText = (x) => typeof x === "string" || typeof x === "number" || x instanceof Dec || typeof x === "bigint";
// Taking up the server's page: inputs keep what was typed (see setProp).
let claiming = 0;

class Mismatch extends Error {}

// The page's text node at `c.n`, taken up as text `t`: split where the
// browser joined it with text after it; an empty one is made.
function claimText(c, t) {
  if (t === "") {
    const e = document.createTextNode("");
    c.p.insertBefore(e, c.n);
    return e;
  }
  const n = c.n;
  if (!n || n.nodeType !== 3) throw new Mismatch("text");
  if (n.data === t) { c.n = n.nextSibling; return n; }
  if (n.data.startsWith(t)) { c.n = n.splitText(t.length); return n; }
  throw new Mismatch("text");
}

// The nodes for value `v` (text, markup, a list of them), into `out`.
function put(v, c, out) {
  if (isNil(v) || v === false || v === true) return;
  if (Array.isArray(v)) { for (const x of v) put(x, c, out); return; }
  if (v instanceof Html) { v.make(c, out); return; }
  const t = display(v);
  out.push(c.claim ? claimText(c, t) : document.createTextNode(t));
}

// A part's nodes now: a dynamic part keeps its own (see `dyn`), ahead of
// its marker.
function nodesOf(list, out = []) {
  for (const n of list) {
    if (n.__part) nodesOf(n.__part.nodes, out);
    out.push(n);
  }
  return out;
}

function removeNodes(list) {
  for (const n of nodesOf(list)) if (n.parentNode) n.parentNode.removeChild(n);
}

// A part of the page that follows `get`: its nodes, then a marker (an
// empty text node) that later nodes go in front of. A text that becomes
// another text is changed in place.
function dyn(get, c, out) {
  bound(get, dynApply, { nodes: [], end: null, c, out }, null, undefined);
}

function dynApply(x) {
  const r = this.el, c = r.c;
  if (r.end === null) {
    const got = [];
    put(x, c, got);
    r.nodes = got;
    r.end = document.createTextNode("");
    r.end.__part = r;
    if (c.claim) c.p.insertBefore(r.end, c.n);
    for (const n of got) r.out.push(n);
    r.out.push(r.end);
    r.c = { claim: false, svg: c.svg };
    r.out = null;
    return;
  }
  const one = r.nodes.length === 1 ? r.nodes[0] : null;
  if (one && one.nodeType === 3 && !one.__part && isText(x)) {
    const t = display(x);
    if (one.data !== t) one.data = t;
    return;
  }
  removeNodes(r.nodes);
  const got = [];
  put(x, c, got);
  const p = r.end.parentNode;
  if (p) for (const n of got) p.insertBefore(n, r.end);
  r.nodes = got;
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
    const s = display(x);
    if (t.data !== s) t.data = s;
    return;
  }
  const got = [];
  put(x, { claim: false, svg }, got);
  if (isText(x) && got.length === 0) got.push(document.createTextNode(""));
  el.replaceChildren(...got);
}

// What is left of an element's children after taking it up: empty text
// nodes go; anything else is not what the code makes.
function rest(el, n) {
  while (n) {
    const next = n.nextSibling;
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
  return { tpl, html: null, svg: null };
}

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
    for (const k of t[2]) el.appendChild(typeof k === "string" ? document.createTextNode(k) : typeof k === "number" ? document.createTextNode("") : make(k, here && t[0] !== "foreignObject"));
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
      wire(def.tpl, el, this.vals, svg);
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
  if (kids.length === 1 && typeof kids[0] === "number") { showIn(el, vals[kids[0]], false, sv, kids[0]); return; }
  let n = el.firstChild;
  for (let i = 0; i <= last; i++) {
    const k = kids[i], next = i < last ? n.nextSibling : null;
    if (typeof k === "number") showAt(el, n, vals[k], sv, k);
    else if (typeof k === "object" && k.dyn) wire(k, n, vals, sv);
    n = next;
  }
}

const inner = (tp, svg) => (svg || tp[0] === "svg") && tp[0] !== "foreignObject";

// The page's element `el` taken up as template `tp`.
function claimEl(tp, el, vals, svg) {
  if (!el || el.nodeType !== 1 || el.localName.toLowerCase() !== String(tp[0]).toLowerCase()) throw new Mismatch("element");
  claiming++;
  try { for (const h of tp[3]) hole(el, h[0], h[1], vals[h[2]], svg, h[2]); } finally { claiming--; }
  const kids = tp[2], sv = inner(tp, svg);
  // The server writes a textarea's value as its text.
  if (kids.length === 0 && tp[0] === "textarea") return;
  if (kids.length === 1 && typeof kids[0] === "number") { showIn(el, vals[kids[0]], true, sv, kids[0]); return; }
  const c = { p: el, n: el.firstChild, claim: true, svg: sv };
  for (const k of kids) {
    if (typeof k === "string") claimText(c, k);
    else if (typeof k === "number") showClaim(c, vals[k], k);
    else { const n = c.n; claimEl(k, n, vals, sv); c.n = n.nextSibling; }
  }
  rest(el, c.n);
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
function classApply(x) { if (x !== this.old) setClass(this.el, x, this.name); this.old = x; }

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
  if (isText(v)) { el.firstChild.data = display(v); return; }
  el.replaceChildren(...got);
}

// `{...map}` into an element: attributes and `@event` handlers, as a
// written-out attribute would be, except that those written out win.
function spreadInto(el, get) {
  let had = {};
  effect(() => {
    const now = typeof get === "function" ? get() : get;
    untrack(() => {
      for (const k of Object.keys(had)) if (!hasOwn(now, k)) { if (k[0] === "@") listen(el, k.slice(1), null); else setProp(el, k, null, had[k]); }
      for (const k of Object.keys(now)) { if (k[0] === "@") listen(el, k.slice(1), now[k]); else setProp(el, k, now[k], had[k]); }
      had = now;
    });
  });
}

// W.spread(maps, written): the attributes the maps give an element, without
// those it has written out, the key and the class (added to its own).
// Names are compared as the browser does (`ACTION` is `action`), so a key
// cannot stand in for a written attribute.
function spreadAttrs(maps, written) {
  const o = {};
  const seen = new Set(written.map(attrKey));
  for (const m of maps) {
    if (isNil(m)) continue;
    if (!isMap(m)) fail(`\`{...}\` needs a map, got ${typeName(m)}`);
    for (const k of Object.keys(m)) {
      const n = attrKey(k);
      if (n === "key" || n === "class" || seen.has(n)) continue;
      if (k[0] !== "@" && !spreadAttrOk(k)) continue;
      seen.add(n);
      o[k] = m[k];
    }
  }
  return o;
}

const spreadClasses = (maps) => maps.filter((m) => isMap(m) && hasOwn(m, "class")).map((m) => m.class);

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

const Nav = { at: new Sig(""), on: false };

// What reads url() follows it.
function navChanged() {
  Nav.at.set(location.href);
}

function navStart() {
  if (Nav.on || typeof window === "undefined") return;
  Nav.on = true;
  // Links in components are followed here: not prefetched as pages.
  window.__quillo_nav = true;
  window.addEventListener("popstate", navChanged);
  document.addEventListener("click", (e) => {
    if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
    const a = e.target.closest && e.target.closest("a[href]");
    if (!a || !a.closest("quillo-island")) return;
    if ((a.target && a.target !== "_self") || a.hasAttribute("download") || a.hasAttribute("data-reload")) return;
    const u = new URL(a.href, location.href);
    if (u.origin !== location.origin) return;
    // Only links into this program; others load normally.
    if (BASE && u.pathname !== BASE && !u.pathname.startsWith(BASE + "/")) return;
    const path = u.pathname.slice(BASE.length) || "/";
    if (path.startsWith("/_quillo/")) return;
    e.preventDefault();
    navigate(path + u.search + u.hash);
  });
}

function navigate(to) {
  navStart();
  const full = withBase(to);
  if (full !== location.pathname + location.search + location.hash) history.pushState(null, "", full);
  window.scrollTo(0, 0);
  navChanged();
  return null;
}

function currentUrl() {
  navStart();
  if (Nav.at._v === "" && typeof location !== "undefined") Nav.at._v = location.href;
  Nav.at.v;
  const p = location.pathname;
  return {
    path: BASE && p.startsWith(BASE) ? p.slice(BASE.length) || "/" : p,
    query: Object.fromEntries(new URLSearchParams(location.search)),
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
  return /^[^\s"'<>\/=`{}\u0000-\u001f\u007f]+$/.test(k) && !codeAttr(k) && !k.startsWith("__") && (!k.includes(":") || attrKey(k) === "xlink:href");
}

const sandboxed = (el) => el.hasAttribute("sandbox") && !/allow-same-origin/i.test(el.getAttribute("sandbox"));

const URL_ATTRS = ["href", "src", "action", "formaction", "poster", "xlink:href", "cite", "background", "data", "codebase"];

// An SVG animation's values go to another attribute (`<set
// attributeName="href" to={u}>`), perhaps a URL one: they are checked as
// URLs, each of `values` (split by ;) too, as on the server.
const animValueAttr = (el, k) => (el.localName === "set" || el.localName === "animate") && /^(to|from|by|values)$/i.test(k);

// javascript: and vbscript: URLs, read as browsers read them.
function scriptURL(u) {
  const s = String(u).replace(/^[\u0000-\u0020]+/, "").replace(/[\t\n\r]/g, "").slice(0, 11).toLowerCase();
  return s.startsWith("javascript:") || s.startsWith("vbscript:");
}

function setProp(el, k, v, old) {
  if (k === "value" || k === "checked" || k === "selected") {
    // Taking up the server's HTML: what the page shows (typed, ticked) stays.
    if (claiming) return;
    const nv = k === "value" ? (isNil(v) ? "" : display(v)) : truthy(v);
    if (el[k] !== nv) el[k] = nv;
    if (k !== "value") { if (nv) el.setAttribute(k, ""); else el.removeAttribute(k); }
    return;
  }
  if (isNil(v) || v === false) { el.removeAttribute(k); return; }
  // Only text written in the program goes there (set with the template),
  // and a page for a sandboxed frame that is kept away from the site.
  if (codeAttr(k) && !(attrKey(k) === "srcdoc" && sandboxed(el))) return;
  if (k === "href" || k === "src" || k === "action" || k === "formaction" || k === "poster") v = withBase(v);
  let s = v === true ? "" : (Array.isArray(v) || isMap(v)) ? jsonText(v) : display(v);
  // The text the attribute will say, whatever the value's type (html too).
  if (URL_ATTRS.includes(k.toLowerCase()) && scriptURL(s) || animValueAttr(el, k) && s.split(";").some(scriptURL)) v = s = "about:blank#blocked";
  if (old !== v || el.getAttribute(k) !== s) el.setAttribute(k, s);
}

function fire(el, type, ev) {
  const f = type === "click" ? el.__click : el.__w && el.__w[type];
  if (typeof f !== "function") return;
  if (type === "submit") ev.preventDefault();
  try {
    const r = f.length > 0 ? f(ev) : f();
    if (r instanceof Promise) r.catch(report);
  } catch (e) {
    report(e);
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
  if (t === "click") {
    el.__click = f;
    if (!delegating) { delegating = true; document.addEventListener("click", delegated); }
    return;
  }
  const cur = el.__w || (el.__w = {});
  cur[t] = f;
  const listening = el.__wl || (el.__wl = {});
  if (!listening[t]) {
    listening[t] = true;
    el.addEventListener(t, (ev) => fire(el, t, ev));
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
      try {
        const r = f.length > 0 ? f(el) : f();
        if (r instanceof Promise) r.then(keep, report); else keep(r);
      } catch (e) {
        report(e);
      }
    }
  });
  $mount.q.push([el, f, Owner]);
}
$mount.q = [];
mountHook = $mount;

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
    const st = { rows: [], end: null, get nodes() { const a = []; for (const r of this.rows) for (const n of r.nodes) a.push(n); return a; } };
    const svg = c.svg;
    let first = true;
    // Rows are their own scopes, not their owner's: they come and go
    // without it, and go with it.
    onCleanup(() => { for (const r of st.rows) dispose(r.scope); st.rows = []; });
    effect(() => {
      const items = iter(list());
      untrack(() => {
        if (!first) { reconcile(st, items, key, row, svg); return; }
        first = false;
        st.rows = items.map((x, i) => newRow(x, key ? key(x) : i, row, c, out));
        st.end = document.createTextNode("");
        st.end.__part = st;
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
        try { x = d.fn(); } catch (e) { report(e); continue; }
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
  const got = [];
  try { put(part(row(r.item)), c, got); } finally { Owner = o; Listener = l; }
  r.nodes = got.slice();
  for (const n of r.nodes) out.push(n);
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
  if (first && p.firstChild === first && p.lastChild === end) {
    p.textContent = "";
    p.appendChild(end);
  } else if (first && first.parentNode === p) {
    const range = document.createRange();
    range.setStartBefore(first);
    range.setEndBefore(end);
    range.deleteContents();
  }
  for (const r of old) dispose(r.scope);
  st.rows = [];
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
  // None kept: the old rows go at once.
  if (left === n && w > 0) { clearRows(st, p); old = []; w = 0; }
  for (let j = 0; j < w; j++) if (!used[j]) dropRow(old[j]);
  // The kept rows already in order stay; the others move. New rows go in
  // together, as a fragment.
  let moved = false, last = -1;
  for (let i = 0; i < n; i++) { const j = from[i]; if (j < 0) continue; if (j < last) moved = true; else last = j; }
  const stay = moved ? inOrder(from) : null;
  const rows = new Array(n);
  let after = end, pend = null;
  for (let i = n - 1; i >= 0; i--) {
    const j = w ? from[i] : -1;
    let r;
    if (j < 0) {
      const got = [];
      r = newRow(items[i], keys[i], row, { claim: false, svg }, got);
      if (!pend) pend = document.createDocumentFragment();
      for (let k = got.length - 1; k >= 0; k--) pend.insertBefore(got[k], pend.firstChild);
    } else {
      if (pend) { const f = pend.firstChild; p.insertBefore(pend, after); if (f) after = f; pend = null; }
      r = old[j];
      r.key = keys[i];
      if (r.item._v !== items[i]) r.item.set(items[i]);
      if (moved && !stay[i]) {
        const ns = nodesOf(r.nodes);
        if (ns.length === 1) p.insertBefore(ns[0], after);
        else { const f = document.createDocumentFragment(); for (const x of ns) f.appendChild(x); p.insertBefore(f, after); }
      }
      const f = firstOf(r.nodes);
      if (f) after = f;
    }
    rows[i] = r;
  }
  if (pend) p.insertBefore(pend, after);
  st.rows = rows;
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
    if (def.$comp) { out.push(island(def, p, takeHydration(def.$name), c)); return; }
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
    this.props = props;
    this.s0 = state || {};
    this.liveInfo = liveInfo || null;
    this.el = null;
  }

  // `state name = init`: what the server rendered with, or init's value.
  // Asked again (a body made again whole), the same state.
  sig(name, init) {
    const had = this.sigs || (this.sigs = {});
    if (had[name]) return had[name];
    if (hasOwn(this.s0, name)) return (had[name] = new Sig(this.s0[name]));
    const v = init();
    if (v instanceof Promise) {
      const s = new Sig(null);
      v.then((x) => s.set(x), report);
      return (had[name] = s);
    }
    return (had[name] = new Sig(v));
  }
}

// A component in its <quillo-island>: made, or taking up the page's.
function island(def, props, h, c) {
  const inst = new Inst(def, props, h && h.s, h && h.l);
  let el;
  if (c.claim) {
    el = c.n;
    if (!el || el.nodeType !== 1 || el.localName !== "quillo-island" || el.dataset.c !== def.$name) throw new Mismatch("component");
    el.__q = true;
    c.n = el.nextSibling;
  } else {
    el = document.createElement("quillo-island");
    el.style.display = "contents";
  }
  inst.el = el;
  run(inst, el, c.claim);
  return el;
}

// The component's code, once; then its markup put in its island.
function run(inst, el, claim) {
  within(inst, () => {
    let view;
    try {
      view = part(inst.def(inst, inst.props));
    } catch (e) {
      if (e instanceof Ret) view = e.v;
      else { report(e); return; }
    }
    if (claim) {
      const c = { p: el, n: el.firstChild, claim: true, svg: false };
      put(view, c, []);
      rest(el, c.n);
    } else {
      const got = [];
      put(view, { claim: false, svg: false }, got);
      el.append(...got);
    }
  });
}

// Island JSON (and a server fn's answer), with error values
// (`{"$quillo_err": x}`), decimals (`{"$dec": "12.50"}`) and large ints
// (`{"$int": "9007199254740993"}`) restored.
function islandJSON(text, fallback) {
  if (!text) return fallback;
  return JSON.parse(text, (k, v) => untag(v));
}
function untag(v) {
  if (!v || typeof v !== "object" || Array.isArray(v)) return v;
  const ks = Object.keys(v);
  if (ks.length !== 1) return v;
  if (ks[0] === "$quillo_err") return new Err(v.$quillo_err);
  if (ks[0] === "$dec" && typeof v.$dec === "string") return decParse(v.$dec, false) ?? v.$dec;
  if (ks[0] === "$int" && typeof v.$int === "string" && /^-?\d{1,19}$/.test(v.$int)) return intOf(BigInt(v.$int));
  return v;
}
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

function report(e) {
  console.error("quillo:", e instanceof Error ? e.message : e);
  if (typeof window !== "undefined" && window.__quilloErrors) window.__quilloErrors.push(String(e && e.message || e));
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
  let i = node && node.nodeType === 1 && node.closest ? node.closest("quillo-island") : null;
  for (let up = i && i.parentElement && i.parentElement.closest("quillo-island"); up; up = up.parentElement && up.parentElement.closest("quillo-island")) i = up;
  return i;
}

// An island taken up: its component (and those inside it) resumed from
// the state the server wrote, on the HTML it wrote.
function resume(el) {
  if (el.__q) return;
  el.__q = true;
  const def = C[el.dataset.c];
  if (!def) { report(`unknown component <${el.dataset.c}>`); return; }
  const queue = () => {
    const q = {};
    for (const inner of el.querySelectorAll("quillo-island")) {
      (q[inner.dataset.c] = q[inner.dataset.c] || []).push({ s: islandJSON(inner.dataset.s, {}), l: islandJSON(inner.dataset.l, null) });
    }
    return q;
  };
  for (const inner of el.querySelectorAll("quillo-island")) liveSeen(...(inner.dataset.v || "").split(":"));
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
    if (typeof window !== "undefined") window.__quilloResumed = (window.__quilloResumed || 0) + 1;
  } catch (e) {
    if (!(e instanceof Mismatch)) report(e);
    ok = false;
  }
  if (!ok) {
    // Counted, so a page (or a test) can tell its HTML was made again.
    if (typeof window !== "undefined") window.__quilloRemade = (window.__quilloRemade || 0) + 1;
    dispose(inst);
    hydrating = queue();
    el.replaceChildren();
    inst = new Inst(def, props, state, liveInfo);
    inst.el = el;
    run(inst, el, false);
  }
  hydrating = null;
  // What was typed before it was taken up: the component hears it now.
  for (const f of el.querySelectorAll("input, textarea, select")) {
    const changed = f.type === "checkbox" || f.type === "radio" ? f.checked !== f.defaultChecked
      : f.tagName === "SELECT" ? [...f.options].some((o) => o.selected !== o.defaultSelected)
      : f.value !== f.defaultValue;
    if (changed) {
      f.dispatchEvent(new Event("input", { bubbles: true }));
      f.dispatchEvent(new Event("change", { bubbles: true }));
    }
  }
}

function wake(e) {
  const i = outerIsland(e.target);
  if (i && !i.__q) resume(i);
}

function boot() {
  // Someone reaching for an island takes it up (before their event is
  // handled: it reaches the component).
  for (const t of ["pointerover", "pointerdown", "touchstart", "focusin", "keydown", "click", "submit", "input"]) document.addEventListener(t, wake, true);
  const outer = [...document.querySelectorAll("quillo-island")].filter((i) => !(i.parentElement && i.parentElement.closest("quillo-island")));
  for (const island of outer) {
    if (island.hasAttribute("data-e") || island.hasAttribute("data-l") || island.parentElement === document.body) resume(island);
  }
  // The island someone reached for, which brought the bundle, and the one
  // that has the focus.
  for (const n of [window.__quillo_at, document.activeElement]) {
    const i = n && n !== document.body ? outerIsland(n) : null;
    if (i) resume(i);
  }
  // What was done before the bundle came (the loader in server.rs kept it):
  // the same elements are on the page, so it is done again on them.
  const held = window.__quillo_q || [];
  window.__quillo_booted = true;
  for (const e of held.splice(0)) {
    const i = outerIsland(e.target);
    if (i) resume(i);
    if (!e.target.isConnected) continue;
    if (e.type === "submit") {
      const form = e.target.closest("form") || e.target;
      if (form.requestSubmit) form.requestSubmit(); else form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    } else {
      e.target.click();
    }
  }
}

// ---- what compiled code uses -------------------------------------------

const G = {};
const C = {};
const W = {};
W.Err = Err;
W.Dec = Dec;
W.Ret = Ret;
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
W.index = index;
W.call = callFn;
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
W.span = (a, b, dots) => {
  if (!dots) { if (b === undefined) { b = I(a, "range"); a = 0; } else { a = I(a, "range"); b = I(b, "range", 2); } }
  if (!isInt(a) || !isInt(b)) fail("`..` needs two ints");
  if (b - a > 10000000) fail("range is too large");
  return [a, b];
};
W.iter = iter;
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
W.cls = (xs) => xs.filter((x) => !isNil(x) && x !== false).map(display).join(" ");
W.cls1 = (x) => (isNil(x) || x === false ? "" : display(x));
W.component = (name, params, render) => {
  render.$name = name;
  render.$p = params;
  render.$comp = true;
  C[name] = render;
  return render;
};
// A function's form for `<Name .../>`: its code once, with props as getters.
W.view = (f, view) => { f.$view = view; return f; };
// `state name = live(fname, ...args)`: state that follows the server. When
// its arguments change (`live(load, id)` after `id` changed), it loads again.
W.live = (inst, name, fname, args) => {
  const had0 = inst.sigs || (inst.sigs = {});
  if (had0[name]) return had0[name];
  return (had0[name] = within(inst, () => liveSig(inst, name, fname, args)));
};
function liveSig(inst, name, fname, args) {
  const info = inst.liveInfo && inst.liveInfo[name];
  const had = hasOwn(inst.s0, name);
  const sig = new Sig(had ? inst.s0[name] : null);
  const sub = { inst, sig, name, fname, args: null, topics: info ? info.t : [] };
  Live.subs.add(sub);
  onCleanup(() => { Live.subs.delete(sub); liveRestart(); });
  let first = true;
  effect(() => {
    const a = args();
    untrack(() => {
      if (first) {
        first = false;
        sub.args = a;
        if (had) liveRestart(); else liveRefresh(sub);
        return;
      }
      if (eq(sub.args, a)) return;
      sub.args = a;
      liveRefresh(sub);
    });
  });
  return sig;
}
W.server = (name, idem) => meta((...args) => rpc(name, args, idem), name, null);
