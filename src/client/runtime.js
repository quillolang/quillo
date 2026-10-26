// Quillo browser runtime: values, the standard library, a small virtual DOM,
// components with ~state, and calls to `server fn`s. The compiled program is
// appended after this and registers its functions in G.
"use strict";

// Where the program is mounted ("" at the root); the bundle sets it.
let BASE = "";
const withBase = (u) => (BASE && typeof u === "string" && u[0] === "/" && u[1] !== "/" ? BASE + u : u);

// ---- values ------------------------------------------------------------

class QuilloError extends Error {}
class Ret { constructor(v) { this.v = v; } }
class Err { constructor(e) { this.error = e === undefined ? null : e; } }
class VNode {}
class VText extends VNode { constructor(t) { super(); this.t = t; } }
class VEl extends VNode { constructor(tag, props, on, kids, key) { super(); this.tag = tag; this.props = props; this.on = on; this.kids = kids; this.key = key; } }
class VComp extends VNode { constructor(def, props, key) { super(); this.def = def; this.props = props; this.key = key; } }
class VRaw extends VNode { constructor(html) { super(); this.html = html; } }
// Markup of a fixed shape: the values of its holes (see `block`).
class VBlock extends VNode { constructor(def, vals, key) { super(); this.def = def; this.vals = vals; this.key = key; } }

const hasOwn = (o, k) => Object.prototype.hasOwnProperty.call(o, k);
const fail = (msg) => { throw new QuilloError(msg); };
const isNil = (v) => v === null || v === undefined;
const isEvent = (v) => typeof Event !== "undefined" && v instanceof Event;
const isMap = (v) => v !== null && typeof v === "object" && (v.constructor === Object || (!Array.isArray(v) && !(v instanceof Err) && !(v instanceof VNode) && !isEvent(v)));
const isInt = (v) => typeof v === "number" && Number.isInteger(v);
// What Quillo code does not reach on the page's objects: what runs text as
// code or HTML (markup in components does that safely), and the way to the
// Function constructor.
const FOREIGN_DENY = ["eval", "Function", "setTimeout", "setInterval", "execScript", "execCommand", "insertAdjacentHTML",
  "setHTMLUnsafe", "parseHTMLUnsafe", "createContextualFragment", "write", "writeln", "constructor", "__proto__", "prototype"];
function foreignName(name) {
  if (FOREIGN_DENY.includes(name)) fail(`\`${name}\` is not used from Quillo code: it runs text as code or HTML`);
}
// Calls a method of the page's object `o`, unless it would run text as code.
function foreignCall(o, name, args) {
  foreignName(name);
  for (const a of args) if (typeof a === "string" && scriptURL(a)) fail(`\`${name}\` is not given a javascript: URL`);
  if (name === "setAttribute" || name === "setAttributeNS") {
    const k = String(args[name === "setAttribute" ? 0 : 1]).toLowerCase();
    if (k.startsWith("on") || k === "srcdoc") fail(`\`${name}\` does not set ${k}: it runs text as code`);
  }
  const r = o[name](...args);
  return r === undefined ? null : r;
}
// An object from the page's scripts (an element, a library's), not one the
// program made: its inherited properties and methods are its fields too.
const foreign = (v) => v !== null && typeof v === "object" && (v === globalThis || v.constructor !== Object && v.constructor !== undefined
  && !Array.isArray(v) && !(v instanceof Err) && !(v instanceof VNode));
// Functions of the page that run text as code: Quillo code calls none of
// them, however it came by one.
let deniedFns = null;
function denied(f) {
  if (deniedFns === null) {
    deniedFns = new Set();
    for (const n of ["eval", "Function", "setTimeout", "setInterval", "execScript"]) if (typeof globalThis[n] === "function") deniedFns.add(globalThis[n]);
  }
  if (deniedFns.has(f)) fail(`\`${f.name}\` is not called from Quillo code: it runs text as code`);
}

function typeName(v) {
  if (isNil(v)) return "null";
  switch (typeof v) {
    case "boolean": return "bool";
    case "number": return Number.isInteger(v) ? "int" : "float";
    case "string": return "str";
    case "function": return "fn";
  }
  if (Array.isArray(v)) return "list";
  if (v instanceof Err) return "error";
  if (v instanceof VNode) return "html";
  return "map";
}

function typeIs(t, v) {
  switch (t) {
    case "any": return true;
    case "int": return isInt(v);
    // JS numbers do not remember whether they were written 2 or 2.0.
    case "float": return typeof v === "number";
    case "num": return typeof v === "number";
    case "str": return typeof v === "string";
    case "bool": return typeof v === "boolean";
    case "list": return Array.isArray(v);
    case "map": return isMap(v) || isEvent(v);
    case "html": return v instanceof VNode;
    case "fn": return typeof v === "function";
    default: return false;
  }
}

const fmtKey = (k) => /^[A-Za-z_][A-Za-z0-9_]*$/.test(k) ? k : JSON.stringify(k);

function repr(v) {
  if (isNil(v)) return "null";
  if (typeof v === "string") return JSON.stringify(v);
  if (typeof v === "number") return String(v);
  if (typeof v === "boolean") return String(v);
  if (typeof v === "function") return "<fn " + (v.$name || "<fn>") + ">";
  if (Array.isArray(v)) return "[" + v.map(repr).join(", ") + "]";
  if (v instanceof Err) return "err(" + repr(v.error) + ")";
  if (v instanceof VNode) return "<html>";
  if (isEvent(v)) return "<event " + v.type + ">";
  const ks = Object.keys(v);
  return ks.length ? "{ " + ks.map((k) => fmtKey(k) + ": " + repr(v[k])).join(", ") + " }" : "{}";
}

const display = (v) => typeof v === "string" ? v : repr(v);
const short = (v) => { const r = repr(v); return r.length > 40 ? r.slice(0, 40) + "…" : r; };
const truthy = (v) => !(isNil(v) || v === false || v instanceof Err);

function eq(a, b) {
  if (a === b) return true;
  // Different numbers, strings, bools and functions; null and undefined match.
  if (typeof a !== "object" || typeof b !== "object" || a === null || b === null) return isNil(a) && isNil(b);
  if (Array.isArray(a) && Array.isArray(b)) return a.length === b.length && a.every((x, i) => eq(x, b[i]));
  if (a instanceof Err && b instanceof Err) return eq(a.error, b.error);
  if (isMap(a) && isMap(b)) {
    const ka = Object.keys(a);
    return ka.length === Object.keys(b).length && ka.every((k) => hasOwn(b, k) && eq(a[k], b[k]));
  }
  return false;
}

function cmp(a, b) {
  if ((typeof a === "number" && typeof b === "number") || (typeof a === "string" && typeof b === "string")) {
    return a < b ? -1 : a > b ? 1 : 0;
  }
  return null;
}

// ---- operators ---------------------------------------------------------

function add(a, b) {
  if (typeof a === "number" && typeof b === "number") return a + b;
  if (typeof a === "string") return a + display(b);
  if (typeof b === "string") return display(a) + b;
  if (a instanceof VNode && b instanceof VNode) return frag([a, b]);
  if (Array.isArray(a) && Array.isArray(b)) return a.concat(b);
  if (isMap(a) && isMap(b)) return Object.assign({}, a, b);
  fail(`cannot add ${typeName(a)} and ${typeName(b)}`);
}

function arith(op, verb) {
  return (a, b) => {
    if (typeof a !== "number" || typeof b !== "number") fail(`cannot ${verb} ${typeName(a)} and ${typeName(b)}`);
    if (isInt(a) && isInt(b)) {
      if (b === 0 && (op === "/" || op === "%")) fail("division by zero");
    }
    switch (op) {
      case "-": return a - b;
      case "*": return a * b;
      case "/": return a / b;
      default: return a % b;
    }
  };
}
const sub = arith("-", "subtract");
const mul = arith("*", "multiply");
const div = arith("/", "divide");
const mod = arith("%", "take the remainder of");

function order(a, b) {
  const c = cmp(a, b);
  if (c === null) fail(`cannot compare ${typeName(a)} with ${typeName(b)}`);
  return c;
}
const lt = (a, b) => order(a, b) < 0;
const le = (a, b) => order(a, b) <= 0;
const gt = (a, b) => order(a, b) > 0;
const ge = (a, b) => order(a, b) >= 0;

function range(a, b) {
  if (!isInt(a) || !isInt(b)) fail("`..` needs two ints");
  if (b - a > 10000000) fail("range is too large");
  const out = [];
  for (let i = a; i < b; i++) out.push(i);
  return out;
}

function neg(v) {
  if (typeof v !== "number") fail(`cannot negate ${typeName(v)}`);
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
      for (const [k, v] of new FormData(f)) out[k] = typeof v === "string" ? v : v.name;
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

function call(f, args) {
  if (typeof f !== "function") fail(`cannot call ${typeName(f)}: it is not a function`);
  denied(f);
  return f(...args);
}

// `recv.name(args)`: a function stored in the map, or `name(recv, args)`.
function mcall(recv, name, fb, args) {
  if (foreign(recv) && typeof recv[name] === "function") return foreignCall(recv, name, args);
  if (isMap(recv) && hasOwn(recv, name)) return call(recv[name], args);
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
function argty(v, names, opt, p, name) {
  if (!tyOk(v, names, opt)) fail(`argument \`${p}\` of \`${name}\` must be ${tyText(names, opt)}, got ${typeName(v)} ${short(v)}`);
}
function retty(v, names, opt, name, enames, eopt) {
  if (v instanceof Err) {
    if (enames && !tyOk(v.error, enames, eopt)) fail(`\`${name}\` declares errors as \`! ${tyText(enames, eopt)}\`, but returned err(${short(v.error)})`);
    return v;
  }
  if (names && !tyOk(v, names, opt)) fail(`\`${name}\` must return ${tyText(names, opt)}, but returned ${typeName(v)} ${short(v)}`);
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

function matchInto(p, v, b) {
  switch (p.k) {
    case "w": return true;
    case "b": b[p.n] = v; return true;
    case "l": return eq(p.v, v);
    case "t": return typeIs(p.t, v);
    case "a": {
      for (const q of p.ps) {
        const t = {};
        if (matchInto(q, v, t)) { Object.assign(b, t); return true; }
      }
      return false;
    }
    case "as": {
      if (!matchInto(p.p, v, b)) return false;
      b[p.n] = v;
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
        if (f.ty && Object.keys(b).length === before) b[f.path[f.path.length - 1]] = x;
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

const want = (ok, f, what, i, v) => { if (!ok) fail(`\`${f}\` expects ${what} as argument ${i}, got ${isNil(v) && v === undefined ? "nothing" : typeName(v)}`); };
const S = (v, f, i = 1) => { want(typeof v === "string", f, "a str", i, v); return v; };
const L = (v, f, i = 1) => { if (isNil(v)) return []; want(Array.isArray(v), f, "a list", i, v); return v; };
const I = (v, f, i = 1) => { want(isInt(v), f, "an int", i, v); return v; };
const M = (v, f, i = 1) => { if (isNil(v)) return {}; want(isMap(v), f, "a map", i, v); return v; };
const F = (v, f, i = 2) => { want(typeof v === "function", f, "a function", i, v); denied(v); return v; };
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
    if (c === null) { bad = [typeName(a[0]), typeName(b[0])]; return 0; }
    return c;
  });
  if (bad) fail(`\`${name}\` cannot compare ${bad[0]} with ${bad[1]}`);
  return sorted.map((x) => x[1]);
}

function extreme(l, name, want) {
  let best = null;
  for (const x of L(l, name)) {
    if (best === null) { best = x; continue; }
    const c = cmp(x, best);
    if (c === null) fail(`\`${name}\` cannot compare ${typeName(x)} with ${typeName(best)}`);
    if (c === want) best = x;
  }
  return best;
}

function coerce(t, s) {
  s = s.trim();
  if (t === "int") return /^[+-]?\d+$/.test(s) ? parseInt(s, 10) : null;
  if (t === "float" || t === "num") return s !== "" && !isNaN(Number(s)) ? Number(s) : null;
  return null;
}

function toJSON(v) {
  if (isNil(v)) return null;
  if (v instanceof Err) return { error: toJSON(v.error) };
  if (Array.isArray(v)) return v.map(toJSON);
  if (v instanceof VNode) return "";
  if (typeof v === "function") return repr(v);
  if (isMap(v)) { const o = {}; for (const k of Object.keys(v)) o[k] = toJSON(v[k]); return o; }
  if (typeof v === "number" && !isFinite(v)) return null;
  return v;
}

const B = {};
// `every(ms, f)` while a component renders: calls f every ms milliseconds for
// as long as its renders keep asking (with the latest f and ms). A render that
// does not ask (a paused game: `if running { every(100, step) }`) stops it,
// and so does the component leaving the page.
B.every = meta((ms, f) => {
  const inst = rendering;
  if (!inst) fail("`every` works while a component renders");
  N(ms, "every");
  F(f, "every", 2);
  if (!inst.timers) {
    inst.timers = [];
    inst.timersAsked = 0;
    // After each render: stop the timers it did not ask for.
    inst.afterRender = () => {
      for (const t of inst.timers.splice(inst.timersAsked)) clearInterval(t.id);
      inst.timersAsked = 0;
    };
    onGone(inst, () => {
      inst.timersAsked = 0;
      inst.afterRender();
    });
  }
  const timers = inst.timers;
  const i = inst.timersAsked++;
  let t = timers[i];
  if (!t || t.ms !== ms) {
    if (t) clearInterval(t.id);
    t = timers[i] = { ms, f, id: 0 };
    t.id = setInterval(() => {
      if (inst.kids !== null && !inst.el.isConnected) {
        inst.timersAsked = 0;
        return inst.afterRender();
      }
      try {
        const r = t.f();
        if (r instanceof Promise) r.catch(report);
      } catch (e) {
        report(e);
      }
    }, Math.max(ms, 1));
  }
  t.f = f;
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
  if (isInt(v)) return v;
  if (typeof v === "number" && isFinite(v)) return Math.trunc(v);
  if (typeof v === "boolean") return v ? 1 : 0;
  if (typeof v === "string") { const n = coerce("int", v); return n === null ? new Err(`not an int: ${JSON.stringify(v)}`) : n; }
  return new Err(`cannot convert ${typeName(v)} to int`);
}, "int", null);
B.float = meta((v) => {
  if (typeof v === "number") return v;
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
B.range = meta((a, b) => b === undefined ? range(0, I(a, "range")) : range(I(a, "range"), I(b, "range", 2)), "range", null);
B.enumerate = meta((l) => L(l, "enumerate").map((v, i) => [i, v]), "enumerate", null);
B.keys = meta((m) => Object.keys(M(m, "keys")), "keys", null);
B.values = meta((m) => { m = M(m, "values"); return Object.keys(m).map((k) => m[k]); }, "values", null);
B.get = meta((m, k, d = null) => { m = M(m, "get"); k = display(k); return hasOwn(m, k) ? m[k] : d; }, "get", null);
B.has = meta((m, k) => hasOwn(M(m, "has"), display(k)), "has", null);
B.set = meta((m, k, v = null) => {
  const o = { ...M(m, "set") }, key = display(k);
  // "__proto__" is a key like any other here, not the object's prototype.
  if (key === "__proto__") Object.defineProperty(o, key, { value: v, enumerable: true, writable: true, configurable: true });
  else o[key] = v;
  return o;
}, "set", null);
B.remove = meta((c, k) => {
  if (Array.isArray(c)) return c.filter((v) => !eq(v, k));
  const m = Object.assign({}, M(c, "remove"));
  delete m[display(k)];
  return m;
}, "remove", null);
B.merge = meta((a, b) => Object.assign({}, M(a, "merge"), M(b, "merge", 2)), "merge", null);
B.err = meta((v = null) => new Err(v), "err", null);
B.is_err = meta((v) => v instanceof Err, "is_err", null);
B.unwrap = meta((v) => { if (v instanceof Err) fail(`unwrap of an error: ${display(v.error)}`); return v; }, "unwrap", null);
B.raw = meta((v) => new VRaw(display(v)), "raw", null);
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
    consts: ["true", "false", "null", "int", "str", "bool", "float", "map", "list", "any"] });
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
B.highlight = meta((code, lang) => new VRaw(HL(display(code), lang == null ? "" : display(lang))), "highlight", null);

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
B.abs = meta((x) => Math.abs(N(x, "abs")), "abs", null);
// Halves round away from zero, as on the server (Math.round(-2.5) is -2).
const roundHalf = (x) => Math.sign(x) * Math.round(Math.abs(x));
B.round = meta((x, d) => { N(x, "round"); if (d === undefined) return roundHalf(x); const p = 10 ** Math.min(Math.max(I(d, "round", 2), 0), 15); return roundHalf(x * p) / p; }, "round", null);
B.floor = meta((x) => Math.floor(N(x, "floor")), "floor", null);
B.ceil = meta((x) => Math.ceil(N(x, "ceil")), "ceil", null);
B.sqrt = meta((x) => Math.sqrt(N(x, "sqrt")), "sqrt", null);
B.pow = meta((x, y) => N(x, "pow") ** N(y, "pow", 2), "pow", null);
B.unique = meta((l) => { const out = []; for (const x of L(l, "unique")) if (!out.some((y) => eq(x, y))) out.push(x); return out; }, "unique", null);
B.flatten = meta((l) => [].concat(...L(l, "flatten").map((x) => Array.isArray(x) ? x : [x])), "flatten", null);
B.zip = meta((a, b) => { a = L(a, "zip"); b = L(b, "zip", 2); return a.slice(0, Math.min(a.length, b.length)).map((x, i) => [x, b[i]]); }, "zip", null);
B.take = meta((v, n) => { n = Math.max(I(n, "take", 2), 0); return typeof v === "string" ? Array.from(v).slice(0, n).join("") : L(v, "take").slice(0, n); }, "take", null);
B.drop = meta((v, n) => { n = Math.max(I(n, "drop", 2), 0); return typeof v === "string" ? Array.from(v).slice(n).join("") : L(v, "drop").slice(n); }, "drop", null);
B.count = meta((l, f) => { l = L(l, "count"); if (f === undefined) return l.length; F(f, "count"); return l.filter((x) => truthy(cb(f, "count", x))).length; }, "count", null);
B.group_by = meta((l, f) => { F(f, "group_by"); const out = {}; for (const x of L(l, "group_by")) { const k = display(cb(f, "group_by", x)); if (!hasOwn(out, k)) Object.defineProperty(out, k, { value: [], enumerable: true, writable: true, configurable: true }); out[k].push(x); } return out; }, "group_by", null);
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
  for (const a of args) if (typeof a === "string" && scriptURL(a)) fail(`\`js\`: ${path} is not given a javascript: URL`);
  const r = v.apply(owner, args);
  return r === undefined ? null : r;
}, "js", null);
B.js_new = meta((path, ...args) => {
  const [, v] = jsPath(path, "js_new");
  if (typeof v !== "function") fail(`\`js_new\`: ${path} is not a class`);
  for (const a of args) if (typeof a === "string" && scriptURL(a)) fail(`\`js_new\`: ${path} is not given a javascript: URL`);
  return new v(...args);
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
B.t = meta((k, vars) => {
  S(k, "t"); vars = M(vars, "t", 2);
  const d = i18n();
  let e = d && hasOwn(d.t, k) ? d.t[k] : k;
  if (e && typeof e === "object") {
    const cat = typeof vars.count === "number" ? plural(d.locale, vars.count) : "other";
    e = hasOwn(e, cat) ? e[cat] : (e.other ?? "");
  }
  return String(e).replace(/\{(\w+)\}/g, (m, name) => hasOwn(vars, name) ? display(vars[name]) : m);
}, "t", null);
B.url = meta(() => currentUrl(), "url", null);
B.css = meta((s) => S(s, "css"), "css", null);
B.sql = meta((s) => S(s, "sql"), "sql", null);
B.navigate = meta((to) => navigate(S(to, "navigate")), "navigate", null);
B.json = {
  parse: (s) => { S(s, "json.parse"); try { return fromJSON(JSON.parse(s)); } catch (e) { return new Err("invalid JSON: " + e.message); } },
  stringify: (v, pretty) => JSON.stringify(toJSON(v), null, truthy(pretty) ? 2 : undefined),
};

function fromJSON(j) {
  if (j === undefined) return null;
  return j;
}

// ---- math, stats, vec (src/numeric.rs, the same answers) ------------------
// The browser cannot tell 2 from 2.0, so ints are numbers that are whole.

const NS = (l, f, i = 1) => { l = L(l, f, i); l.forEach((v, k) => { if (typeof v !== "number") fail(`\`${f}\` expects a list of numbers, but item ${k} is ${typeName(v)}`); }); return l; };
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

const domain = (f, x) => fail(`\`math.${f}\`: math domain error (${display(x)})`);
const checked = (f, x, y, ok) => {
  if (!ok || (Number.isNaN(y) && !Number.isNaN(x))) domain(f, x);
  if (!Number.isFinite(y) && !Number.isNaN(y) && Number.isFinite(x)) fail(`\`math.${f}\`: math range error (${display(x)})`);
  return y;
};
const checked2 = (f, x, y, r, ok) => {
  const nanIn = Number.isNaN(x) || Number.isNaN(y);
  if (!ok || (Number.isNaN(r) && !nanIn)) fail(`\`math.${f}\`: math domain error (${display(x)}, ${display(y)})`);
  if (!Number.isFinite(r) && !Number.isNaN(r) && Number.isFinite(x) && Number.isFinite(y)) fail(`\`math.${f}\`: math range error (${display(x)}, ${display(y)})`);
  return r;
};
const unary = (f, g, dom = () => true) => (x) => { N(x, "math." + f); return checked(f, x, g(x), Number.isNaN(x) || dom(x)); };

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
    if (!given(b)) return checked("log", x, Math.log(x), Number.isNaN(x) || x > 0);
    N(b, "math.log", 2);
    if (b <= 0 || b === 1) fail(`\`math.log\`: math domain error (base ${display(b)})`);
    return checked("log", x, Math.log(x) / Math.log(b), Number.isNaN(x) || x > 0);
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
  ldexp: (x, e) => { N(x, "math.ldexp"); I(e, "math.ldexp", 2); e = Math.min(Math.max(e, -10000), 10000); let r = x; while (e > 1000) { r *= 2 ** 1000; e -= 1000; } while (e < -1000) { r *= 2 ** -1000; e += 1000; } return checked("ldexp", x, r * 2 ** e, true); },
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
    for (const x of L(l, "math.prod")) { if (typeof x !== "number") fail(`\`math.prod\` expects numbers, got ${typeName(x)}`); acc = isInt(acc) && isInt(x) ? overflowed(acc * x) : acc * x; }
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
    if (n < 1) fail(`\`${f}\`: n must be at least 1`);
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
  if (typeof a !== "number" || typeof b !== "number") fail(`\`${f}\` expects numbers, got ${typeName(a)} and ${typeName(b)}`);
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
    r.forEach((v) => { if (typeof v !== "number") fail(`\`${f}\` expects numbers, but row ${k} holds ${typeName(v)}`); });
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

// ---- server fns --------------------------------------------------------

async function rpcRaw(name, args, live) {
  let r;
  const headers = { "content-type": "application/json" };
  if (live) headers["x-quillo-live"] = "1";
  // Files (from an <input type=file>) go as parts of a multipart body, the
  // other arguments as JSON beside them.
  let payload;
  if (args.some((a) => typeof Blob !== "undefined" && a instanceof Blob)) {
    payload = new FormData();
    payload.append("args", JSON.stringify(args.map((a, i) => a instanceof Blob ? { $quillo_file: i } : toJSON(a))));
    args.forEach((a, i) => { if (a instanceof Blob) payload.append("file" + i, a, a.name || "file"); });
    delete headers["content-type"];
    headers["x-quillo-rpc"] = "1";
  } else {
    payload = JSON.stringify(args.map(toJSON));
  }
  // A call that fails (no connection, a server error) is an error value,
  // as with http.* on the server, so `??` and `is_err` handle it.
  const failed = (msg) => ({ value: new Err(msg), topics: [], seq: 0, server: "", failed: true });
  try {
    r = await fetch(BASE + "/_quillo/rpc/" + encodeURIComponent(name), {
      method: "POST",
      headers,
      body: payload,
    });
  } catch (e) {
    return failed(`server fn \`${name}\` could not be reached: ${e.message}`);
  }
  let body = null;
  try { body = await r.json(); } catch (_) {}
  if (!r.ok) return failed(`server fn \`${name}\` failed (${r.status})${body && body.error ? ": " + body.error : ""}`);
  const value = body && hasOwn(body, "err") ? new Err(body.err) : body && hasOwn(body, "ok") ? body.ok : null;
  return { value, topics: (body && body.topics) || [], seq: (body && body.seq) || 0, server: (body && body.server) || "" };
}

async function rpc(name, args) {
  return (await rpcRaw(name, args, false)).value;
}

// ---- live updates --------------------------------------------------------
// Each `~x = live(load, ...)` is a subscription to the topics `load` read
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
    if (s.inst.kids !== null && !s.inst.el.isConnected) { Live.subs.delete(s); continue; }
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
    sub.inst.set(sub.name, value);
    liveRestart();
  } catch (e) {
    report(e);
  }
}

// ---- virtual DOM -------------------------------------------------------

function frag(items) { return items; }

function norm(x, out) {
  if (isNil(x) || x === false || x === true) return out;
  if (Array.isArray(x)) { for (const y of x) norm(y, out); return out; }
  if (x instanceof VNode) { out.push(x); return out; }
  out.push(new VText(display(x)));
  return out;
}

// `x` as a list of nodes; a memoized loop's rows (see `rows`) are one.
function nodes(x) { return x !== null && x !== undefined && x.$rows === true ? x : norm(x, []); }

// HTML literal: W.h(tag, props, on, kids, key)
function h(tag, props, on, kids, key) {
  return new VEl(tag, props, on, norm(kids, []), isNil(key) ? null : display(key));
}

// ---- blocks ------------------------------------------------------------
// Markup whose shape never changes (its tags, fixed attributes and text) is
// compiled into a block: a template, made once and cloned for each use, with
// holes for what can change. A render gives only the holes' values, and an
// update compares them with the last ones.
//
// tpl: [tag, [[name, value], ...], [child, ...]], a child being text or a tpl.
// holes: [kind, node, name]: 0 attribute, 1 class, 2 handler, 3 an element's
// only child (its node is an empty text node in the template, which text
// fills), 4 an element's children.
// mount(v, root, svg): compiled for the block, it finds the nodes with holes
// in a copy of the template (`v.els`) and fills them from `v.vals`.
// patch(a, v): compiled too, v takes over a's DOM and updates what changed.
function block(tpl, holes, mount, patch) {
  return { tpl, kinds: holes.map((h) => h[0]), at: holes.map((h) => h[1]), names: holes.map((h) => h[2]), mount, patch, html: null, svg: null };
}

// A use of block `def`: W.b(def, values, key). Numbers and strings are
// keys as they are.
function b(def, vals, key) {
  return new VBlock(def, vals, isNil(key) ? null : typeof key === "number" || typeof key === "string" ? key : display(key));
}

// A block's DOM, made once and cloned: elements from <svg> down are SVG's
// (a block may hold an <svg> inside HTML), and HTML again inside a
// <foreignObject>.
function blockTemplate(def, svg) {
  const make = (t, inSvg) => {
    const here = inSvg || t[0] === "svg";
    const el = here ? document.createElementNS(SVG, t[0]) : document.createElement(t[0]);
    for (const [k, v] of t[1]) setProp(el, k, v, undefined);
    for (const c of t[2]) el.appendChild(typeof c === "string" ? document.createTextNode(c) : make(c, here && t[0] !== "foreignObject"));
    return el;
  };
  return svg ? (def.svg || (def.svg = make(def.tpl, true))) : (def.html || (def.html = make(def.tpl, false)));
}

// Whether what goes inside `el` is SVG.
const inSvg = (el) => el.namespaceURI === SVG && el.localName !== "foreignObject";

function setClass(el, v, svg) {
  if (isNil(v) || v === "") el.removeAttribute("class");
  else if (svg || el.namespaceURI === SVG) el.setAttribute("class", v);
  else el.className = v;
}

const isText = (x) => typeof x === "string" || typeof x === "number";

function mountBlock(v, svg) {
  const def = v.def;
  svg = svg || def.tpl[0] === "svg";
  const root = blockTemplate(def, svg).cloneNode(true);
  v.parts = null;
  v.svg = svg;
  def.mount(v, root, svg);
  return (v.d = root);
}

// Hole `i` of block `v` shows virtual DOM: `el`'s children become `x`.
function showNodes(v, i, el, x, svg) {
  const ks = nodes(x);
  svg = svg || inSvg(el);
  if (adopting) adoptKids(el, ks, svg);
  else {
    el.textContent = "";
    for (const k of ks) el.appendChild(mount(k, svg));
  }
  (v.parts || (v.parts = new Array(v.vals.length)))[i] = ks;
  v.els[v.def.at[i]] = el;
}

function patchBlock(a, v) {
  if (a !== v) v.def.patch(a, v);
}

// Hole `i` of mounted block `v` goes from `old` to `x`. A hole of kind 3
// holds a text node while it shows text; showing virtual DOM, its node is
// the element and `parts` has what it shows.
function updateHole(v, i, x, old) {
  const def = v.def, kind = def.kinds[i], at = def.at[i], name = def.names[i], els = v.els;
  // Handlers are new closures each render: only the one to call changes.
  if (kind === 2) { listen(els[at], name, x); return; }
  // Inputs' value and checked follow the state even when the user changed them.
  if (x === old && !(kind === 0 && (name === "value" || name === "checked" || name === "selected"))) return;
  const n = els[at];
  if (kind === 0) setProp(n, name, x, old);
  else if (kind === 1) setClass(n, x, v.svg);
  else {
    const was = v.parts && v.parts[i];
    if (!was) {
      if (isText(x)) n.data = display(x);
      else showNodes(v, i, n.parentNode, x, v.svg);
    } else if (kind === 3 && isText(x)) {
      for (const k of was) drop(k);
      n.textContent = "";
      const t = document.createTextNode(display(x));
      n.appendChild(t);
      els[at] = t;
      v.parts[i] = null;
    } else {
      const ks = nodes(x);
      patchKids(n, was, ks, v.svg || inSvg(n));
      v.parts[i] = ks;
    }
  }
}

// A row kept from the last render (see `memo`), one of whose holes read
// something that changed: the hole and its DOM are updated in place.
function hole(v, i, x) {
  const old = v.vals[i];
  v.vals[i] = x;
  updateHole(v, i, x, old);
}

// `<Card .../>`: a view call, or a component instance. Spread maps fill
// the parameters not written out; their other keys are ignored.
function comp(def, props, kids, key, spreads) {
  if (typeof def !== "function") fail(`<${def}> is not a view`);
  const ps = def.$p || [];
  for (const k of Object.keys(props)) if (!ps.includes(k)) fail(`<${def.$name}> has no parameter \`${k}\``);
  for (const m of spreads || []) {
    if (isNil(m)) continue;
    if (!isMap(m)) fail(`\`{...}\` in <${def.$name}> needs a map, got ${typeName(m)}`);
    for (const k of Object.keys(m)) if (ps.includes(k) && k !== "children" && k !== "key" && !hasOwn(props, k)) props[k] = m[k];
  }
  if (ps.includes("children")) props.children = norm(kids, []);
  if (def.$comp) return new VComp(def, props, isNil(key) ? null : display(key));
  return def(...ps.map((p) => props[p]));
}

// An element with `{...map}` attributes: written-out ones win, classes add up,
// and `@event` keys become handlers.
function hs(tag, spreads, props, on, kids, key) {
  const p = {};
  const o = Object.assign({}, on || {});
  const classes = [];
  for (const m of spreads) {
    if (isNil(m)) continue;
    if (!isMap(m)) fail(`\`{...}\` in <${tag}> needs a map, got ${typeName(m)}`);
    for (const k of Object.keys(m)) {
      if (k === "key") continue;
      if (k === "class") { classes.push(m[k]); continue; }
      if (k[0] !== "@" && !spreadAttrOk(k)) continue;
      if (k[0] === "@") { if (!hasOwn(o, k.slice(1))) o[k.slice(1)] = m[k]; continue; }
      if (!hasOwn(props, k)) p[k] = m[k];
    }
  }
  Object.assign(p, props);
  if (classes.length) p.class = [props.class, ...classes].filter((x) => !isNil(x) && x !== false && x !== "").map(display).join(" ");
  return h(tag, p, Object.keys(o).length ? o : null, kids, key);
}

// ---- client-side navigation ---------------------------------------------
// Components that call url() re-render when the URL changes. Once one has,
// clicks on same-site links inside components change the URL without a
// reload (not with a modifier key, target, download or data-reload).

let rendering = null;
const Nav = { subs: new Set(), on: false };

function navChanged() {
  for (const i of [...Nav.subs]) {
    if (i.kids !== null && !i.el.isConnected) { Nav.subs.delete(i); continue; }
    i.update();
  }
}

function navStart() {
  if (Nav.on || typeof window === "undefined") return;
  Nav.on = true;
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
  const inst = rendering;
  if (inst && !Nav.subs.has(inst)) {
    Nav.subs.add(inst);
    onGone(inst, () => Nav.subs.delete(inst));
  }
  const p = location.pathname;
  return {
    path: BASE && p.startsWith(BASE) ? p.slice(BASE.length) || "/" : p,
    query: Object.fromEntries(new URLSearchParams(location.search)),
    hash: location.hash.slice(1),
  };
}

const SVG = "http://www.w3.org/2000/svg";
let hydrating = null;

// Spread keys are data: plain attribute names only, and no event handlers
// or srcdoc, which would run the value as code (as on the server).
function spreadAttrOk(k) {
  const l = k.toLowerCase();
  return /^[^\s"'<>\/=`{}\u0000-\u001f\u007f]+$/.test(k) && !l.startsWith("on") && l !== "srcdoc" && !k.startsWith("__");
}

const URL_ATTRS = ["href", "src", "action", "formaction", "poster", "xlink:href", "cite", "background", "data", "codebase"];

// javascript: and vbscript: URLs, read as browsers read them.
function scriptURL(u) {
  const s = String(u).replace(/^[\u0000-\u0020]+/, "").replace(/[\t\n\r]/g, "").slice(0, 11).toLowerCase();
  return s.startsWith("javascript:") || s.startsWith("vbscript:");
}

function setProp(el, k, v, old) {
  if (k === "value" || k === "checked" || k === "selected") {
    // Taking up the server's HTML: what the page shows (typed, ticked) stays.
    if (adopting) return;
    const nv = k === "value" ? (isNil(v) ? "" : display(v)) : truthy(v);
    if (el[k] !== nv) el[k] = nv;
    if (k !== "value") { if (nv) el.setAttribute(k, ""); else el.removeAttribute(k); }
    return;
  }
  if (isNil(v) || v === false) { el.removeAttribute(k); return; }
  if (URL_ATTRS.includes(k.toLowerCase()) && typeof v === "string" && scriptURL(v)) v = "about:blank#blocked";
  if (k === "href" || k === "src" || k === "action" || k === "formaction" || k === "poster") v = withBase(v);
  const s = v === true ? "" : (Array.isArray(v) || isMap(v)) ? JSON.stringify(toJSON(v)) : display(v);
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

// The component whose DOM is being made or patched, and `$mount` when a
// component uses `@mount` (the compiler names the event so, and the bundle
// carries it only then).
let updating = null;
let mountHook = null;

function unhook(g) {
  try { g(); } catch (e) { report(e); }
}

// `@mount={(el) => ...}`: called once the element is on the page, once in
// its life (later renders' closures are not called). A function it returns
// is called when the element leaves the page, or its component does.
function $mount(el, f) {
  if (el.__mounted) return;
  el.__mounted = true;
  if ($mount.q.length === 0) queueMicrotask(() => {
    for (const [el, f, inst] of $mount.q.splice(0)) {
      if (!el.isConnected || (inst && inst.dead)) continue;
      const keep = (g) => { if (typeof g === "function" && inst) (inst.hooks || (inst.hooks = [])).push([el, g]); };
      try {
        const r = f.length > 0 ? f(el) : f();
        if (r instanceof Promise) r.then(keep, report); else keep(r);
      } catch (e) {
        report(e);
      }
    }
  });
  $mount.q.push([el, f, updating || rendering]);
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

function setEvents(el, on) {
  if (el.__click && !(on && hasOwn(on, "click"))) el.__click = null;
  const cur = el.__w;
  if (cur) for (const t of Object.keys(cur)) if (!on || !hasOwn(on, t)) cur[t] = null;
  if (!on) return;
  for (const t of Object.keys(on)) listen(el, t, on[t]);
}

function mount(v, svg) {
  if (v instanceof VBlock) return mountBlock(v, svg);
  if (v instanceof VText) return (v.d = document.createTextNode(v.t));
  if (v instanceof VRaw) {
    const el = document.createElement("quillo-raw");
    el.style.display = "contents";
    el.innerHTML = v.html;
    return (v.d = el);
  }
  if (v instanceof VComp) {
    const h = takeHydration(v.def.$name);
    const inst = new Inst(v.def, v.props, h && h.s, h && h.l);
    const el = document.createElement("quillo-island");
    el.style.display = "contents";
    inst.el = el;
    inst.update();
    v.inst = inst;
    return (v.d = el);
  }
  const isSvg = svg || v.tag === "svg";
  const el = isSvg ? document.createElementNS(SVG, v.tag) : document.createElement(v.tag);
  for (const k of Object.keys(v.props)) setProp(el, k, v.props[k], undefined);
  setEvents(el, v.on);
  for (const k of v.kids) el.appendChild(mount(k, isSvg && v.tag !== "foreignObject"));
  return (v.d = el);
}

function same(a, b) {
  if (a.constructor !== b.constructor) return false;
  if (a instanceof VBlock) return a.def === b.def && a.key === b.key;
  if (a instanceof VEl) return a.tag === b.tag && a.key === b.key;
  if (a instanceof VComp) return a.def === b.def && a.key === b.key;
  return true;
}

function patch(parent, a, b, svg) {
  // The same node: a row the render took over unchanged (see `memo`).
  if (a === b) return;
  if (!same(a, b)) {
    const d = mount(b, svg);
    parent.replaceChild(d, a.d);
    drop(a);
    return;
  }
  const d = (b.d = a.d);
  if (b instanceof VBlock) return patchBlock(a, b);
  if (b instanceof VText) { if (a.t !== b.t) d.data = b.t; return; }
  if (b instanceof VRaw) { if (a.html !== b.html) d.innerHTML = b.html; return; }
  if (b instanceof VComp) {
    b.inst = a.inst;
    b.inst.props = b.props;
    b.inst.update();
    return;
  }
  const isSvg = svg || b.tag === "svg";
  for (const k of Object.keys(a.props)) if (!hasOwn(b.props, k)) setProp(d, k, null, a.props[k]);
  for (const k of Object.keys(b.props)) setProp(d, k, b.props[k], a.props[k]);
  setEvents(d, b.on);
  patchKids(d, a.kids, b.kids, isSvg && b.tag !== "foreignObject");
}

function keyOf(v) { return v instanceof VBlock || v instanceof VEl || v instanceof VComp ? v.key : null; }

// `el`'s children were `old` and become `kids` (all of its children).
function patchKids(el, old, kids, svg) {
  if (kids.length === 0) { if (old.length) clearKids(el, old); return; }
  if (old.length === 0) { for (const k of kids) el.appendChild(mount(k, svg)); return; }
  // The same nodes as before (rows a render kept): nothing to do.
  if (old.length === kids.length) {
    let i = 0;
    while (i < kids.length && old[i] === kids[i]) i++;
    if (i === kids.length) return;
  }
  if (kids.some((k) => keyOf(k) !== null)) return patchKeyed(el, old, kids, svg);
  const n = Math.min(old.length, kids.length);
  for (let i = 0; i < n; i++) patch(el, old[i], kids[i], svg);
  for (let i = n; i < kids.length; i++) el.appendChild(mount(kids[i], svg));
  for (let i = n; i < old.length; i++) unmount(el, old[i]);
}

function clearKids(el, old) {
  for (const o of old) drop(o);
  el.textContent = "";
}

function sameKey(a, b) {
  const k = keyOf(b);
  return k !== null && keyOf(a) === k && same(a, b);
}

// Children with keys: those whose key stays are kept and updated; of them,
// the longest run already in order stays where it is and the others move,
// so a swap moves two and a removal none.
function patchKeyed(el, old, kids, svg) {
  let s = 0, oe = old.length - 1, ne = kids.length - 1;
  while (s <= oe && s <= ne && (old[s] === kids[s] || sameKey(old[s], kids[s]))) { patch(el, old[s], kids[s], svg); s++; }
  while (s <= oe && s <= ne && (old[oe] === kids[ne] || sameKey(old[oe], kids[ne]))) { patch(el, old[oe], kids[ne], svg); oe--; ne--; }
  const end = ne + 1 < kids.length ? kids[ne + 1].d : null;
  if (s > oe) {
    for (let i = s; i <= ne; i++) el.insertBefore(mount(kids[i], svg), end);
    return;
  }
  if (s > ne) {
    for (let j = s; j <= oe; j++) unmount(el, old[j]);
    return;
  }
  // Each new child's old one: at the same place, or at the same place from
  // the end, else (for the few left) by key.
  const m = ne - s + 1, shift = oe - ne;
  const from = new Int32Array(m).fill(-1);
  const used = new Uint8Array(oe + 1);
  let left = 0;
  for (let i = 0; i < m; i++) {
    const k = kids[s + i];
    let j = s + i;
    if (!(j <= oe && !used[j] && (old[j] === k || sameKey(old[j], k)))) {
      j = s + i + shift;
      if (!(j >= s && j <= oe && !used[j] && (old[j] === k || sameKey(old[j], k)))) j = -1;
    }
    if (j >= 0) { used[j] = 1; from[i] = j; } else left++;
  }
  if (left > 0) {
    const byKey = new Map(), unkeyed = [];
    for (let j = s; j <= oe; j++) {
      if (used[j]) continue;
      const k = keyOf(old[j]);
      if (k === null) unkeyed.push(j);
      else if (!byKey.has(k)) byKey.set(k, j);
    }
    let u = 0;
    for (let i = 0; i < m && (byKey.size > 0 || u < unkeyed.length); i++) {
      if (from[i] >= 0) continue;
      const k = kids[s + i], key = keyOf(k);
      let j = key === null ? (u < unkeyed.length ? unkeyed[u++] : -1) : (byKey.has(key) ? byKey.get(key) : -1);
      if (j >= 0 && (used[j] || !same(old[j], k))) j = -1;
      if (j >= 0) { used[j] = 1; from[i] = j; }
    }
  }
  let last = -1, moved = false, kept = 0;
  for (let i = 0; i < m; i++) {
    const j = from[i];
    if (j < 0) continue;
    kept++;
    if (j < last) moved = true;
    else last = j;
  }
  // Nothing kept of a whole list: emptied at once, then filled.
  if (kept === 0 && s === 0 && oe === old.length - 1 && ne === kids.length - 1) {
    clearKids(el, old);
    for (const k of kids) el.appendChild(mount(k, svg));
    return;
  }
  for (let j = s; j <= oe; j++) if (!used[j]) unmount(el, old[j]);
  // Most children where they were (a swap): those stay and the few others
  // move, without working out the longest run in order.
  let stay = null;
  if (moved) {
    let displaced = 0;
    for (let i = 0; i < m && displaced <= 16; i++) if (from[i] >= 0 && from[i] !== s + i) displaced++;
    if (displaced <= 16) {
      stay = new Uint8Array(m);
      for (let i = 0; i < m; i++) stay[i] = from[i] === s + i ? 1 : 0;
    } else stay = inOrder(from);
  }
  let after = end;
  for (let i = m - 1; i >= 0; i--) {
    const k = kids[s + i], j = from[i];
    if (j < 0) el.insertBefore(mount(k, svg), after);
    else {
      patch(el, old[j], k, svg);
      if (moved && !stay[i]) el.insertBefore(k.d, after);
    }
    after = k.d;
  }
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

function unmount(el, v) {
  drop(v);
  if (v.d && v.d.parentNode === el) el.removeChild(v.d);
}

// Part of the page is gone: the components in it stop what they started.
function drop(v) {
  if (v instanceof VComp) { if (v.inst) v.inst.destroy(); }
  else if (v instanceof VBlock) { if (v.parts) for (const ks of v.parts) if (ks) for (const k of ks) drop(k); }
  else if (v.kids) for (const k of v.kids) drop(k);
}

// Runs `f` when the component leaves the page (see Inst.destroy).
function onGone(inst, f) {
  (inst.cleanup || (inst.cleanup = [])).push(f);
}

// ---- components --------------------------------------------------------

// A loop's rows from one render to the next (see `memo`): the list they
// were made for, and the rows, in order.
class Memo {
  constructor() { this.deps = null; this.changed = -1; this.list = null; this.rows = null; }
}

// `for` loop `slot` in component `inst`'s render: its rows from the last
// render, and which of what else they read (`deps`) changed, as bits.
function memo(inst, slot, deps) {
  const ms = inst.memos || (inst.memos = []);
  const m = ms[slot] || (ms[slot] = new Memo());
  let changed = 0;
  if (m.deps === null || m.deps.length !== deps.length) changed = -1;
  else for (let i = 0; i < deps.length; i++) if (m.deps[i] !== deps[i]) changed |= i < 31 ? 1 << i : -1;
  m.changed = changed;
  m.deps = deps;
  return m;
}

// A memoized loop's rows: `make(x)` makes the row for item x, and
// `refresh(row, x, changed)` brings a kept one up to date with what changed
// (without it, any change makes every row again). A row is kept for the
// same item (the same value): looked for at the same place first, then at
// the same place from the end (after rows were added or removed before
// it), then, for what is left, anywhere.
function rows(m, list, make, refresh) {
  const n = list.length, c = m.changed;
  const keep = c === 0 || (refresh !== null && c !== -1);
  const was = keep ? m.list : null, old = m.rows;
  const w = was === null ? 0 : was.length;
  if (was === list) {
    // The very list of the last render: its rows, in order, the same array
    // when all are still on the page (so what shows them is left alone).
    let fresh = null;
    for (let k = 0; k < n; k++) {
      const v = old[k];
      if (!v.d) {
        if (fresh === null) { fresh = old.slice(); fresh.$rows = true; }
        fresh[k] = make(list[k]);
      } else if (c) refresh(v, list[k], c);
    }
    if (fresh !== null) m.rows = fresh;
    return m.rows;
  }
  // An array of nodes, none of them null or a list, marked as such for
  // `nodes` (arrays made from it, by `+`, map or slice, are not).
  const out = new Array(n);
  out.$rows = true;
  if (w === 0) for (let k = 0; k < n; k++) out[k] = make(list[k]);
  else {
    const used = new Uint8Array(w), shift = w - n;
    let left = 0;
    for (let k = 0; k < n; k++) {
      const x = list[k];
      let j = k;
      if (!(j < w && was[j] === x && !used[j])) {
        j = k + shift;
        if (!(j >= 0 && j < w && was[j] === x && !used[j])) j = -1;
      }
      // A row from a render that failed was never shown: made again.
      const v = j >= 0 ? old[j] : undefined;
      if (v !== undefined && v.d) {
        used[j] = 1;
        if (c) refresh(v, x, c);
        out[k] = v;
      } else left++;
    }
    if (left > 0) {
      let byItem = null;
      for (let j = 0; j < w; j++) if (!used[j] && old[j].d && !(byItem || (byItem = new Map())).has(was[j])) byItem.set(was[j], j);
      for (let k = 0; k < n && left > 0; k++) {
        if (out[k] !== undefined) continue;
        const x = list[k], j = byItem === null ? undefined : byItem.get(x);
        if (j !== undefined && !used[j]) {
          used[j] = 1;
          const v = old[j];
          if (c) refresh(v, x, c);
          out[k] = v;
        } else out[k] = make(x);
        left--;
      }
    }
  }
  m.list = list;
  m.rows = out;
  return out;
}

class Inst {
  constructor(def, props, state, live) {
    this.def = def;
    this.props = props;
    this.liveInfo = live || null;
    this.subs = {};
    this.s = state || {};
    this.ready = new Set(Object.keys(this.s));
    this.kids = null;
    this.el = null;
    this.queued = false;
  }

  // `~name = init`: runs init the first time only.
  state(name, init) {
    if (this.ready.has(name)) return;
    this.ready.add(name);
    const v = init();
    if (v instanceof Promise) {
      this.s[name] = null;
      v.then((x) => this.set(name, x), report);
    } else {
      this.s[name] = v;
    }
  }

  set(name, v) {
    this.s[name] = v;
    if (!this.queued) {
      this.queued = true;
      queueMicrotask(() => { this.queued = false; this.update(); });
    }
    return null;
  }

  render() {
    const ps = this.def.$p;
    const args = ps.map((p) => {
      const v = this.props[p];
      return p === "children" && typeof v === "string" ? new VRaw(v) : v;
    });
    const outer = rendering;
    rendering = this;
    try {
      return norm(this.def(this, ...args), []);
    } catch (e) {
      if (e instanceof Ret) return norm(e.v, []);
      throw e;
    } finally {
      rendering = outer;
      if (this.afterRender) this.afterRender();
    }
  }

  // Gone from the page: its timers, live data and url() listening stop
  // (whatever registered with onGone), and so does every component inside.
  destroy() {
    if (this.dead) return;
    this.dead = true;
    if (this.cleanup) for (const f of this.cleanup) f();
    if (this.hooks) { for (const [, g] of this.hooks) unhook(g); this.hooks = null; }
    if (this.kids) for (const k of this.kids) drop(k);
  }

  // Takes up the server's HTML as it is: the render's nodes are matched to
  // the page's (nothing is made again), handlers attached. HTML that is not
  // what the render gives (changed by hand, say) is made again instead.
  adopt() {
    if (this.dead) return;
    let kids;
    try {
      kids = this.render();
    } catch (e) {
      report(e);
      return;
    }
    const outer = updating;
    updating = this;
    adopting++;
    let ok = true;
    try {
      adoptKids(this.el, kids, false);
      this.kids = kids;
      if (typeof window !== "undefined") window.__quilloResumed = (window.__quilloResumed || 0) + 1;
    } catch (e) {
      if (!(e instanceof Mismatch)) report(e);
      ok = false;
    } finally {
      adopting--;
      updating = outer;
    }
    if (!ok) {
      // Counted, so a page (or a test) can tell its HTML was made again.
      if (typeof window !== "undefined") window.__quilloRemade = (window.__quilloRemade || 0) + 1;
      this.kids = null;
      this.update();
    }
  }

  update() {
    if (this.dead) return;
    let kids;
    try {
      kids = this.render();
    } catch (e) {
      report(e);
      return;
    }
    const outer = updating;
    updating = this;
    try {
      if (this.kids === null) {
        this.el.replaceChildren(...kids.map((k) => mount(k, false)));
      } else {
        patchKids(this.el, this.kids, kids, false);
      }
    } finally {
      updating = outer;
    }
    this.kids = kids;
    // Elements with an @mount cleanup that this render took off the page.
    if (this.hooks) this.hooks = this.hooks.filter(([el, g]) => el.isConnected || (unhook(g), false));
  }
}

// Island JSON, with error values (`{"$quillo_err": x}`) restored.
function islandJSON(text, fallback) {
  if (!text) return fallback;
  return JSON.parse(text, (k, v) => (v && typeof v === "object" && !Array.isArray(v) && hasOwn(v, "$quillo_err") && Object.keys(v).length === 1) ? new Err(v.$quillo_err) : v);
}

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
// A component's HTML from the server is not made again: its render is
// matched to the nodes on the page (text the browser merged is split where
// the render has several texts, empty ones are added), handlers attached,
// and from then on only what changes is patched. Components are taken up
// when someone first reaches for them; those that run by themselves, or
// are the page, as soon as the bundle is in.

class Mismatch extends Error {}
let adopting = 0;

// The page's element `el` given the node shape of block template `t`.
function tplAlign(t, el) {
  if (!el || el.nodeType !== 1 || el.localName !== String(t[0]).toLowerCase()) throw new Mismatch("element");
  const kids = t[2];
  // A hole for all its children (adopted with the hole), or none.
  if (kids.length === 0) return;
  // A hole for its only child: a text node, there even when empty.
  if (kids.length === 1 && kids[0] === "") {
    if (!el.firstChild) el.appendChild(document.createTextNode(""));
    return;
  }
  let n = el.firstChild;
  for (const c of kids) {
    if (typeof c === "string") {
      if (!n || n.nodeType !== 3) throw new Mismatch("text");
      if (n.data !== c) {
        if (c.length && n.data.startsWith(c)) n.splitText(c.length);
        else throw new Mismatch("text");
      }
    } else {
      tplAlign(c, n);
    }
    n = n.nextSibling;
  }
  if (n) throw new Mismatch("extra");
}

function adoptBlock(v, n, svg) {
  const def = v.def;
  svg = svg || def.tpl[0] === "svg";
  tplAlign(def.tpl, n);
  v.parts = null;
  v.svg = svg;
  def.mount(v, n, svg);
  return (v.d = n);
}

// `el`'s children are what `ks` render: matched one by one.
function adoptKids(el, ks, svg) {
  let n = el.firstChild;
  for (const k of ks) n = adoptNode(el, n, k, svg);
  while (n) {
    const next = n.nextSibling;
    if (n.nodeType === 3 && n.data === "") el.removeChild(n);
    else throw new Mismatch("extra");
    n = next;
  }
}

// Node `k` takes up the page's node `n` (and those after it it needs);
// what comes after it is returned.
function adoptNode(el, n, k, svg) {
  if (k instanceof VText) {
    const t = String(k.t);
    if (t === "") {
      const e = document.createTextNode("");
      el.insertBefore(e, n);
      k.d = e;
      return n;
    }
    if (!n || n.nodeType !== 3) throw new Mismatch("text");
    if (n.data === t) { k.d = n; return n.nextSibling; }
    if (n.data.startsWith(t)) { const rest = n.splitText(t.length); k.d = n; return rest; }
    throw new Mismatch("text");
  }
  if (!n || n.nodeType !== 1) throw new Mismatch("node");
  if (k instanceof VBlock) { adoptBlock(k, n, svg); return n.nextSibling; }
  if (k instanceof VComp) {
    if (n.localName !== "quillo-island" || n.dataset.c !== k.def.$name) throw new Mismatch("component");
    const h = takeHydration(k.def.$name);
    const inst = new Inst(k.def, k.props, h && h.s, h && h.l);
    inst.el = n;
    n.__q = true;
    inst.adopt();
    k.inst = inst;
    k.d = n;
    return n.nextSibling;
  }
  if (k instanceof VRaw) {
    if (n.localName === "quillo-raw") { k.d = n; return n.nextSibling; }
    // The server writes raw HTML as it is: its nodes go into the wrapper the
    // browser keeps it in.
    const t = document.createElement("template");
    t.innerHTML = k.html;
    const wrap = document.createElement("quillo-raw");
    wrap.style.display = "contents";
    el.insertBefore(wrap, n);
    for (let i = 0; i < t.content.childNodes.length; i++) {
      const m = wrap.nextSibling;
      if (!m) throw new Mismatch("raw");
      wrap.appendChild(m);
    }
    k.d = wrap;
    return wrap.nextSibling;
  }
  const isSvg = svg || k.tag === "svg";
  if (n.localName !== String(k.tag).toLowerCase()) throw new Mismatch("element");
  k.d = n;
  for (const p of Object.keys(k.props)) setProp(n, p, k.props[p], undefined);
  setEvents(n, k.on);
  adoptKids(n, k.kids, isSvg && k.tag !== "foreignObject");
  return n.nextSibling;
}

// The outermost island a node is in.
function outerIsland(node) {
  let i = node && node.nodeType === 1 && node.closest ? node.closest("quillo-island") : null;
  for (let up = i && i.parentElement && i.parentElement.closest("quillo-island"); up; up = up.parentElement && up.parentElement.closest("quillo-island")) i = up;
  return i;
}

// An island taken up: its component (and those inside it) resumed from
// the state the server wrote, on the HTML it wrote.
function resume(island) {
  if (island.__q) return;
  island.__q = true;
  const def = C[island.dataset.c];
  if (!def) { report(`unknown component <${island.dataset.c}>`); return; }
  hydrating = {};
  for (const inner of island.querySelectorAll("quillo-island")) {
    (hydrating[inner.dataset.c] = hydrating[inner.dataset.c] || []).push({
      s: islandJSON(inner.dataset.s, {}),
      l: islandJSON(inner.dataset.l, null),
    });
    liveSeen(...(inner.dataset.v || "").split(":"));
  }
  liveSeen(...(island.dataset.v || "").split(":"));
  const inst = new Inst(def, islandJSON(island.dataset.p, {}), islandJSON(island.dataset.s, {}), islandJSON(island.dataset.l, null));
  inst.el = island;
  inst.adopt();
  hydrating = null;
  // What was typed before it was taken up: the component hears it now.
  for (const el of island.querySelectorAll("input, textarea, select")) {
    const changed = el.type === "checkbox" || el.type === "radio" ? el.checked !== el.defaultChecked
      : el.tagName === "SELECT" ? [...el.options].some((o) => o.selected !== o.defaultSelected)
      : el.value !== el.defaultValue;
    if (changed) {
      el.dispatchEvent(new Event("input", { bubbles: true }));
      el.dispatchEvent(new Event("change", { bubbles: true }));
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
W.Ret = Ret;
W.fail = fail;
W.truthy = truthy;
W.eq = eq;
W.add = add;
W.sub = sub;
W.mul = mul;
W.div = div;
W.mod = mod;
W.lt = lt;
W.le = le;
W.gt = gt;
W.ge = ge;
W.range = range;
W.neg = neg;
W.field = field;
W.fieldp = fieldp;
W.index = index;
W.call = call;
W.mcall = mcall;
W.tryv = tryv;
W.iter = iter;
W.match = match;
W.arity = arity;
W.missing = missing;
W.argty = argty;
W.retty = retty;
W.meta = meta;
W.h = h;
W.block = block;
W.b = b;
W.hole = hole;
W.rows = rows;
W.memo = memo;
W.hs = hs;
W.comp = comp;
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
// `~name = live(fname, ...args)`: state that follows the server. Each
// render passes the arguments again; when they change (`live(load, id)`
// after `id` changed), it loads again.
W.live = (inst, name, fname, args) => {
  const old = inst.subs[name];
  if (old) {
    if (eq(old.args, args)) return;
    old.args = args;
    liveRefresh(old);
    return;
  }
  const info = inst.liveInfo && inst.liveInfo[name];
  const sub = { inst, name, fname, args, topics: info ? info.t : [] };
  if (!Object.keys(inst.subs).length) {
    onGone(inst, () => {
      for (const s of Object.values(inst.subs)) Live.subs.delete(s);
      liveRestart();
    });
  }
  inst.subs[name] = sub;
  Live.subs.add(sub);
  if (!inst.ready.has(name)) {
    inst.ready.add(name);
    inst.s[name] = null;
    liveRefresh(sub);
  } else {
    liveRestart();
  }
};
W.server = (name) => meta((...args) => rpc(name, args), name, null);
