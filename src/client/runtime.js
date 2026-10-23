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
const HTML_SINKS = ["insertAdjacentHTML", "setHTMLUnsafe", "parseHTMLUnsafe", "createContextualFragment", "write", "writeln"];
// An object from the page's scripts (an element, a library's), not one the
// program made: its inherited properties and methods are its fields too.
const foreign = (v) => v !== null && typeof v === "object" && v.constructor !== Object && v.constructor !== undefined
  && !Array.isArray(v) && !(v instanceof Err) && !(v instanceof VNode);

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
  if (v !== null && typeof v === "object" && v.constructor === Object) {
    const x = v[name];
    if (x !== undefined) return x;
  }
  return field(v, name, fb);
}

function field(v, name, fb) {
  // A map made by the program (the usual case) first.
  if (v !== null && typeof v === "object" && v.constructor === Object) {
    if (hasOwn(v, name)) return v[name];
  }
  if (isNil(v)) return null;
  if (isMap(v)) {
    if (hasOwn(v, name)) return v[name];
    if (foreign(v) && name in v) { const x = v[name]; return x === undefined ? null : typeof x === "function" ? x.bind(v) : x; }
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
  return f(...args);
}

// `recv.name(args)`: a function stored in the map, or `name(recv, args)`.
function mcall(recv, name, fb, args) {
  if (isMap(recv) && hasOwn(recv, name)) return call(recv[name], args);
  if (foreign(recv) && typeof recv[name] === "function") {
    // Text becomes markup or code through these, which markup in the
    // component does safely.
    if (HTML_SINKS.includes(name)) fail(`\`${name}\` is not called from Quillo code: it runs text as HTML`);
    const r = recv[name](...args);
    return r === undefined ? null : r;
  }
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
const F = (v, f, i = 2) => { want(typeof v === "function", f, "a function", i, v); return v; };
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
  const r = v.apply(owner, args);
  return r === undefined ? null : r;
}, "js", null);
B.js_new = meta((path, ...args) => {
  const [, v] = jsPath(path, "js_new");
  if (typeof v !== "function") fail(`\`js_new\`: ${path} is not a class`);
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

// ---- server fns --------------------------------------------------------

async function rpcRaw(name, args, live) {
  let r;
  const headers = { "content-type": "application/json" };
  if (live) headers["x-quillo-live"] = "1";
  // A call that fails (no connection, a server error) is an error value,
  // as with http.* on the server, so `??` and `is_err` handle it.
  const failed = (msg) => ({ value: new Err(msg), topics: [], seq: 0, server: "", failed: true });
  try {
    r = await fetch(BASE + "/_quillo/rpc/" + encodeURIComponent(name), {
      method: "POST",
      headers,
      body: JSON.stringify(args.map(toJSON)),
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

function blockTemplate(def, svg) {
  const make = (t) => {
    const el = svg ? document.createElementNS(SVG, t[0]) : document.createElement(t[0]);
    for (const [k, v] of t[1]) setProp(el, k, v, undefined);
    for (const c of t[2]) el.appendChild(typeof c === "string" ? document.createTextNode(c) : make(c));
    return el;
  };
  return svg ? (def.svg || (def.svg = make(def.tpl))) : (def.html || (def.html = make(def.tpl)));
}

function setClass(el, v, svg) {
  if (isNil(v) || v === "") el.removeAttribute("class");
  else if (svg) el.setAttribute("class", v);
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
  el.textContent = "";
  const ks = nodes(x);
  for (const k of ks) el.appendChild(mount(k, svg));
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
      patchKids(n, was, ks, v.svg);
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
    const nv = k === "value" ? (isNil(v) ? "" : display(v)) : truthy(v);
    if (el[k] !== nv) el[k] = nv;
    if (k !== "value") { if (nv) el.setAttribute(k, ""); else el.removeAttribute(k); }
    return;
  }
  if (isNil(v) || v === false) { el.removeAttribute(k); return; }
  if (URL_ATTRS.includes(k) && typeof v === "string" && scriptURL(v)) v = "about:blank#blocked";
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

function boot() {
  // The outermost islands, found before any is hydrated: hydrating one
  // replaces its content, and islands inside it, detached, would no longer
  // look nested.
  const outer = [...document.querySelectorAll("quillo-island")].filter((i) => !(i.parentElement && i.parentElement.closest("quillo-island")));
  for (const island of outer) {
    const def = C[island.dataset.c];
    if (!def) { report(`unknown component <${island.dataset.c}>`); continue; }
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
    inst.update();
    hydrating = null;
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
