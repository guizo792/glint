// The demo's search engine. It follows the real app's rules where they are visible from the
// outside (query modes and filters, percent maths, "what you pick comes first next time"),
// but it works on a tiny made-up data set, so it is only a demonstration.
import { ITEMS, WINDOWS, CLIPBOARD, BANGS, FILE_ICONS, KIND_ICONS } from "./data.js";

const ACCENTS = /[̀-ͯ]/g;
export const norm = (s) => s.normalize("NFD").replace(ACCENTS, "").toLowerCase();

// ---- Parsing what was typed ------------------------------------------------------------

const PREFIXES = [
  ["> ", "commands"], ["? ", "web"], ["= ", "calc"], ["w ", "windows"], ["c ", "clipboard"],
];

export function parse(input) {
  const trimmed = input.replace(/^\s+/, "");
  let mode = "all";
  let rest = trimmed;
  for (const [prefix, m] of PREFIXES) {
    if (trimmed.startsWith(prefix)) { mode = m; rest = trimmed.slice(prefix.length); break; }
  }
  const q = { mode, text: "", only: null, ext: null, within: null };
  const words = [];
  for (const word of rest.split(/\s+/).filter(Boolean)) {
    const lw = word.toLowerCase();
    const value = (p) => (lw.startsWith(p) && word.length > p.length ? word.slice(p.length) : null);
    if (lw === "app:") q.only = "app";
    else if (lw === "file:") q.only = "file";
    else if (lw === "dir:") q.only = "dir";
    else if (value("ext:") !== null) q.ext = value("ext:").replace(/^\./, "").toLowerCase();
    else if (value("in:") !== null) q.within = value("in:").toLowerCase();
    else words.push(word);
  }
  q.text = words.join(" ");
  return q;
}

// ---- Calculator and unit conversion ----------------------------------------------------

export function formatNumber(n) {
  if (Number.isInteger(n)) return String(n);
  let s = n.toFixed(10).replace(/0+$/, "").replace(/\.$/, "");
  if (s === "0" || s === "-0") s = String(Number(n.toPrecision(6)));
  return s;
}

const LINEAR = {
  length: { m: 1, km: 1000, cm: 0.01, mm: 0.001, mi: 1609.344, ft: 0.3048, in: 0.0254, yd: 0.9144 },
  mass: { kg: 1, g: 0.001, lb: 0.45359237, oz: 0.028349523125 },
  volume: { l: 1, ml: 0.001, gal: 3.785411784 },
};
const ALIASES = {
  mile: "mi", miles: "mi", feet: "ft", foot: "ft", inch: "in", inches: "in", yard: "yd",
  yards: "yd", meter: "m", meters: "m", metre: "m", metres: "m", kilometer: "km",
  kilometers: "km", kilometre: "km", kilometres: "km", pound: "lb", pounds: "lb", lbs: "lb",
  kilogram: "kg", kilograms: "kg", gram: "g", grams: "g", ounce: "oz", ounces: "oz",
  liter: "l", liters: "l", litre: "l", litres: "l", gallon: "gal", gallons: "gal",
  celsius: "c", fahrenheit: "f", kelvin: "k", "°c": "c", "°f": "f",
};
const TEMPS = new Set(["c", "f", "k"]);

function unitOf(raw) {
  const u = ALIASES[raw.toLowerCase()] ?? raw.toLowerCase();
  if (TEMPS.has(u)) return { group: "temp", u };
  for (const [group, table] of Object.entries(LINEAR)) if (u in table) return { group, u };
  return null;
}

function toKelvin(v, u) { return u === "c" ? v + 273.15 : u === "f" ? (v - 32) * 5 / 9 + 273.15 : v; }
function fromKelvin(v, u) { return u === "c" ? v - 273.15 : u === "f" ? (v - 273.15) * 9 / 5 + 32 : v; }

function convert(text) {
  const m = /^\s*(-?\d[\d,]*\.?\d*)\s*([a-z°]+)\s+(?:to|in|as)\s+([a-z°]+)\s*$/i.exec(text);
  if (!m) return null;
  const value = Number(m[1].replace(/,/g, ""));
  const from = unitOf(m[2]);
  const to = unitOf(m[3]);
  if (!from || !to || from.group !== to.group || !Number.isFinite(value)) return null;
  let out;
  if (from.group === "temp") out = fromKelvin(toKelvin(value, from.u), to.u);
  else out = (value * LINEAR[from.group][from.u]) / LINEAR[to.group][to.u];
  const label = to.group === "temp" ? to.u.toUpperCase() : to.u;
  return `${formatNumber(out)} ${label}`;
}

/** Percent works like a phone calculator: 200 + 15% is 230, 18% of 240 is 43.2. */
function evaluate(text) {
  const src = text.replace(/×/g, "*").replace(/÷/g, "/").replace(/,/g, "");
  const tokens = [];
  const re = /\s*(\d+\.?\d*|\.\d+|of\b|[-+*/^()%x])/gy;
  let m;
  let last = 0;
  while ((m = re.exec(src)) !== null) { tokens.push(m[1]); last = re.lastIndex; }
  if (src.slice(last).trim() !== "" || tokens.length === 0) return null;

  let pos = 0;
  const peek = () => tokens[pos];
  const isOperand = (t) => t !== undefined && (/^[\d.]/.test(t) || t === "(" || t === "-");

  function primary() {
    const t = tokens[pos++];
    if (t === undefined) throw new Error("end");
    if (t === "(") {
      const v = additive().v;
      if (tokens[pos++] !== ")") throw new Error("paren");
      return { v };
    }
    if (/^[\d.]/.test(t)) return { v: Number(t) };
    throw new Error("token");
  }
  function postfix() {
    const node = primary();
    // A percent sign after a number is a percentage; before another operand it is modulo.
    if (peek() === "%" && !isOperand(tokens[pos + 1])) { pos++; return { v: node.v / 100, pct: true }; }
    return node;
  }
  function unary() {
    if (peek() === "-") { pos++; const n = unary(); return { v: -n.v, pct: n.pct }; }
    if (peek() === "+") { pos++; return unary(); }
    return postfix();
  }
  function power() {
    const base = unary();
    if (peek() === "^") { pos++; const exp = power(); return { v: Math.pow(base.v, exp.v) }; }
    return base;
  }
  function term() {
    let left = power();
    for (;;) {
      const op = peek();
      if (op === "*" || op === "x" || op === "of") { pos++; left = { v: left.v * power().v }; }
      else if (op === "/") { pos++; left = { v: left.v / power().v }; }
      else if (op === "%" && isOperand(tokens[pos + 1])) { pos++; left = { v: left.v % power().v }; }
      else return left;
    }
  }
  function additive() {
    let left = term();
    for (;;) {
      const op = peek();
      if (op !== "+" && op !== "-") return left;
      pos++;
      const right = term();
      const rv = right.pct ? left.v * right.v : right.v;
      left = { v: op === "+" ? left.v + rv : left.v - rv };
    }
  }
  try {
    const result = additive();
    if (pos !== tokens.length) return null;
    return Number.isFinite(result.v) ? result.v : null;
  } catch {
    return null;
  }
}

const MATHY = /^[\d\s.,+\-*/^%()x×÷]+$|^[\d\s.,+\-*/^%()x×÷]+\bof\b[\d\s.,+\-*/^%()x×÷]+$/i;

/** Returns the answer text, or null. `forced` is true after "= ", where plain numbers also count. */
export function calc(text, forced) {
  const t = text.trim();
  if (!t) return null;
  const converted = convert(t);
  if (converted) return converted;
  if (!forced) {
    if (!MATHY.test(t) || !/\d/.test(t) || !/[-+*/^%()x×÷]|\bof\b/i.test(t.replace(/^-/, ""))) return null;
  }
  const v = evaluate(t);
  return v === null ? null : formatNumber(v);
}

// ---- Ranking -----------------------------------------------------------------------------

function nameScore(nameN, term, short) {
  const base = nameN.replace(/\.[a-z0-9]+$/, "");
  if (base === term) return [1000, [[0, term.length]]];
  if (nameN.startsWith(term)) return [800, [[0, term.length]]];
  for (let i = 1; i < nameN.length; i++) {
    if (!/[a-z0-9]/.test(nameN[i - 1]) && nameN.startsWith(term, i)) return [600, [[i, i + term.length]]];
  }
  if (short) return null;
  const idx = nameN.indexOf(term);
  if (idx >= 0) return [400, [[idx, idx + term.length]]];
  if (term.length >= 3) {
    const ranges = [];
    let at = 0;
    for (const ch of term) {
      const found = nameN.indexOf(ch, at);
      if (found < 0) return null;
      ranges.push([found, found + 1]);
      at = found + 1;
    }
    return [150, ranges];
  }
  return null;
}

const KIND_BONUS = { app: 60, setting: 20, command: 20, folder: 10, file: 0 };

export class Memory {
  constructor() {
    this.byPrefix = new Map(); // typed prefix -> Map(id -> times chosen)
    this.uses = new Map();
    this.pinned = new Set();
    this.hidden = new Set();
  }
  /** Remember that `id` was chosen for what was typed, for every prefix of it. */
  choose(typed, id) {
    const t = norm(typed.trim());
    for (let k = 1; k <= t.length; k++) {
      const key = t.slice(0, k);
      const m = this.byPrefix.get(key) ?? new Map();
      m.set(id, (m.get(id) ?? 0) + 1);
      this.byPrefix.set(key, m);
    }
    this.uses.set(id, (this.uses.get(id) ?? 0) + 1);
  }
  affinity(typed, id) { return this.byPrefix.get(norm(typed.trim()))?.get(id) ?? 0; }
}

function iconFor(it) {
  if (it.icon) return it.icon;
  if (it.kind === "folder") return KIND_ICONS.folder;
  if (it.kind === "file") return FILE_ICONS[it.name.split(".").pop().toLowerCase()] ?? "📄";
  return KIND_ICONS[it.kind] ?? "•";
}

function itemRow(it, ranges, memory) {
  return {
    id: it.id, kind: it.kind, title: it.name, ranges,
    sub: it.kind === "command" || it.kind === "setting" || it.kind === "window" || it.kind === "clipboard"
      ? it.sub : it.dir,
    icon: iconFor(it), pinned: memory.pinned.has(it.id), item: it,
  };
}

function webRow(bang, text) {
  const title = `Search ${bang.name} for ${text}`;
  const at = title.length - text.length;
  return {
    id: `web:${bang.key}`, kind: "web", title, ranges: [[at, title.length]],
    sub: bang.url.replace("{q}", encodeURIComponent(text)), icon: KIND_ICONS.web,
    run: { type: "web", name: bang.name },
  };
}

function contentRow(text) {
  const title = `Search file contents for ${text}`;
  return {
    id: "content", kind: "content", title,
    ranges: [[title.length - text.length, title.length]],
    sub: "Opens Windows search in its indexed locations", icon: KIND_ICONS.content,
    run: { type: "content" },
  };
}

export const MAX_RESULTS = 5;

/** Everything the launcher would show for what is typed. */
export function search(input, memory) {
  const q = parse(input);
  const typed = input;

  if (q.mode === "calc") {
    const answer = calc(q.text, true);
    return answer ? [calcRow(answer, q.text)] : [];
  }
  if (q.mode === "web") {
    return q.text ? [webRow(BANGS[0], q.text)] : [];
  }
  if (q.mode === "windows") return filterPool(WINDOWS, q, memory, typed, 20);
  if (q.mode === "clipboard") return filterPool(CLIPBOARD, q, memory, typed, 20);
  if (q.mode === "commands") {
    return filterPool(ITEMS.filter((i) => i.kind === "command" || i.kind === "setting"), q, memory, typed, 20);
  }

  // Normal search.
  const hasFilter = q.only || q.ext || q.within;
  if (!q.text && !hasFilter) return homeRows(memory);

  const rows = [];
  const answer = q.text && !hasFilter ? calc(q.text, false) : null;
  if (answer) rows.push(calcRow(answer, q.text));

  const bangMatch = /^(\S+)\s+(.+)$/.exec(q.text);
  const bang = bangMatch && BANGS.find((b) => b.key === bangMatch[1].toLowerCase());
  if (bang) rows.push(webRow(bang, bangMatch[2]));

  const found = filterPool(ITEMS, q, memory, typed, MAX_RESULTS);
  rows.push(...found);
  if (q.text) {
    if (found.length === 0 && !answer && !bang) rows.push(webRow(BANGS[0], q.text));
    if (q.text.length >= 2) rows.push(contentRow(q.text));
  }
  return rows;
}

function calcRow(answer, text) {
  return {
    id: "calc", kind: "calc", title: answer, ranges: [],
    sub: `${text.trim()} (press Enter to copy)`, icon: "=", run: { type: "copy", text: answer },
  };
}

function homeRows(memory) {
  const byId = new Map(ITEMS.map((i) => [i.id, i]));
  const pinned = [...memory.pinned].map((id) => byId.get(id)).filter(Boolean);
  const frequent = [...memory.uses.entries()]
    .sort((a, b) => b[1] - a[1]).map(([id]) => byId.get(id))
    .filter((i) => i && !memory.pinned.has(i.id) && !memory.hidden.has(i.id));
  return [...pinned, ...frequent].slice(0, MAX_RESULTS).map((i) => itemRow(i, [], memory));
}

function filterPool(pool, q, memory, typed, limit) {
  const terms = norm(q.text).split(/\s+/).filter(Boolean);
  const short = norm(q.text).replace(/\s+/g, "").length <= 2;
  const scored = [];
  for (const it of pool) {
    if (memory.hidden.has(it.id)) continue;
    if (q.only === "app" && it.kind !== "app") continue;
    if (q.only === "file" && it.kind !== "file") continue;
    if (q.only === "dir" && it.kind !== "folder") continue;
    const nameN = norm(it.name);
    const pathN = norm(`${it.dir ?? ""}\\${it.name}`);
    if (q.ext && !(it.kind === "file" && nameN.endsWith(`.${q.ext}`))) continue;
    if (q.within && !pathN.includes(q.within)) continue;

    let total = 0;
    let ranges = [];
    let ok = true;
    for (const term of terms) {
      const hit = nameScore(nameN, term, short);
      if (hit) { total += hit[0]; ranges = ranges.concat(hit[1]); }
      else if (!short && it.dir && pathN.includes(term)) total += 60; // a word from the folder path
      else { ok = false; break; }
    }
    if (!ok) continue;
    let score = terms.length ? total / terms.length : 100;
    score += (KIND_BONUS[it.kind] ?? 0) - nameN.length * 0.5;
    score += Math.min(memory.uses.get(it.id) ?? 0, 10) * 8;
    if (memory.pinned.has(it.id)) score += 50;
    const aff = terms.length ? memory.affinity(typed, it.id) : 0;
    if (aff > 0) score += 5000 + aff * 300;
    scored.push({ it, score, ranges });
  }
  scored.sort((a, b) => b.score - a.score || a.it.name.localeCompare(b.it.name));
  return scored.slice(0, limit).map(({ it, ranges }) => itemRow(it, ranges, memory));
}

// ---- Actions -----------------------------------------------------------------------------

export const ACTIONS = ["open", "admin", "reveal", "copy_path", "copy_name", "terminal", "properties", "pin", "hide"];
export const ACTION_LABELS = {
  open: "Open", admin: "Run as administrator", reveal: "Open containing folder",
  copy_path: "Copy path", copy_name: "Copy name", terminal: "Open in Terminal",
  properties: "Properties", pin: "Pin", unpin: "Unpin", hide: "Hide from results",
};

export function actionsFor(row) {
  if (!row.item) return ["open"];
  const k = row.item.kind;
  if (k === "file" || k === "folder" || k === "app") return ACTIONS;
  return ["open", "pin"];
}
