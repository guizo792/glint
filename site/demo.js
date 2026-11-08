// The interactive launcher on the page. It runs entirely in the browser, on made-up data.
import { search, Memory, actionsFor, ACTION_LABELS, parse } from "./engine.js";

const $ = (id) => document.getElementById(id);
const input = $("q");
const list = $("list");
const empty = $("empty");
const foot = $("foot");
const status = $("status");
const win = $("win");
const pop = $("pop");
const gear = $("gear");
const tabLabel = $("tab-label");

const memory = new Memory();
let rows = [];
let sel = 0;
let menu = null; // { row, actions, sel } while the Tab menu is open
let confirming = null; // id of a command waiting for a second Enter
let interacted = false;

// ---- Theme (the same choice the app's Settings has) -------------------------------------

function setTheme(value) {
  const root = document.documentElement;
  if (value === "system") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", value);
  const radio = document.querySelector(`input[name="theme"][value="${value}"]`);
  if (radio) radio.checked = true;
}
document.querySelectorAll('input[name="theme"]').forEach((r) =>
  r.addEventListener("change", () => setTheme(r.value)));
$("theme-toggle").addEventListener("click", () => {
  const dark = matchMedia("(prefers-color-scheme: dark)").matches;
  const now = document.documentElement.getAttribute("data-theme") ?? (dark ? "dark" : "light");
  setTheme(now === "dark" ? "light" : "dark");
});

// ---- Rendering --------------------------------------------------------------------------

function highlight(el, text, ranges) {
  const marks = [...(ranges ?? [])].sort((a, b) => a[0] - b[0]);
  let at = 0;
  for (const [from, to] of marks) {
    if (from < at) continue;
    if (from > at) el.append(text.slice(at, from));
    const m = document.createElement("mark");
    m.textContent = text.slice(from, to);
    el.append(m);
    at = to;
  }
  if (at < text.length) el.append(text.slice(at));
}

function rowEl(row, i, selected, badge) {
  const li = document.createElement("li");
  li.id = `opt-${i}`;
  li.setAttribute("role", "option");
  li.setAttribute("aria-selected", String(selected));
  const icon = document.createElement("span");
  icon.className = "icon";
  icon.setAttribute("aria-hidden", "true");
  icon.textContent = row.icon ?? "";
  const text = document.createElement("span");
  text.className = "text";
  const title = document.createElement("span");
  title.className = "title";
  if (row.ranges) highlight(title, row.title, row.ranges); else title.textContent = row.title;
  text.append(title);
  if (row.sub) {
    const sub = document.createElement("span");
    sub.className = "subtitle";
    sub.textContent = row.sub;
    text.append(sub);
  }
  li.append(icon, text);
  if (row.pinned) {
    const pin = document.createElement("span");
    pin.className = "badge";
    pin.textContent = "📌";
    pin.setAttribute("aria-label", "Pinned");
    li.append(pin);
  }
  if (badge) {
    const b = document.createElement("span");
    b.className = "badge";
    b.textContent = badge;
    li.append(b);
  }
  li.addEventListener("mousemove", () => { if (current().sel !== i) { setSel(i); } });
  li.addEventListener("click", () => { setSel(i); activate(); input.focus(); });
  return li;
}

function current() { return menu ?? { sel, rows }; }

function render() {
  list.replaceChildren();
  confirming = confirming && rows[sel]?.id === confirming ? confirming : null;
  let shown;
  if (menu) {
    shown = menu.actions.map((a) => ({
      title: a === "pin" && menu.row.pinned ? ACTION_LABELS.unpin : ACTION_LABELS[a], ranges: [],
    }));
  } else {
    shown = rows;
  }
  const hasInput = input.value.trim() !== "";
  const any = shown.length > 0;
  const noResults = !any && hasInput;
  list.hidden = !any;
  empty.hidden = !noResults;
  foot.hidden = !(any || noResults);
  tabLabel.textContent = menu ? "Back" : "Actions";
  shown.forEach((row, i) => {
    const sel_ = (menu ? menu.sel : sel) === i;
    list.append(rowEl(row, i, sel_, !menu && i < 9 ? `Alt+${i + 1}` : ""));
  });
  input.setAttribute("aria-expanded", String(any));
  const active = menu ? menu.sel : sel;
  if (any) input.setAttribute("aria-activedescendant", `opt-${active}`);
  else input.removeAttribute("aria-activedescendant");
  list.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
}

function recompute(keepSel = false) {
  rows = search(input.value, memory);
  if (!keepSel) sel = 0;
  sel = Math.min(sel, Math.max(rows.length - 1, 0));
  menu = null;
  render();
}

function setSel(i) {
  if (menu) menu.sel = i; else sel = i;
  list.querySelectorAll("li").forEach((li, k) => li.setAttribute("aria-selected", String(k === i)));
  input.setAttribute("aria-activedescendant", `opt-${i}`);
}

function say(text) { status.textContent = text; }

// ---- Running things ---------------------------------------------------------------------

function hintPrefix(typed) {
  const t = typed.trim();
  return t.length > 3 ? t.slice(0, 3) : t;
}

function runRow(row, typed) {
  if (!row) return;
  if (row.run) {
    if (row.run.type === "copy") { say(`✓ Copied “${row.run.text}”. In Glint, Enter copies the answer to the clipboard.`); return; }
    if (row.run.type === "web") { say(`Would open ${row.sub} in your browser.`); return; }
    if (row.run.type === "content") { say("Would hand this search to Windows, which can look inside indexed files."); return; }
  }
  const it = row.item;
  if (!it) return;
  if (it.kind === "command") {
    if (it.confirm && confirming !== row.id) {
      confirming = row.id;
      say(`Press Enter again to confirm: ${it.name}`);
      return;
    }
    confirming = null;
    say(`✓ In Glint this would ${it.did}. Nothing happens here.`);
    return;
  }
  if (it.kind === "setting") { say(`✓ Would open Windows Settings → ${it.name.replace(/ settings$/i, "")}.`); return; }
  if (it.kind === "window") { say(`✓ Would switch to “${it.name}”.`); return; }
  if (it.kind === "clipboard") { say(`✓ Would paste “${it.name}”.`); return; }
  // A file, folder or app: opening it teaches Glint.
  const typedNow = typed.trim();
  memory.choose(typedNow, it.id);
  const hint = hintPrefix(typedNow);
  say(`✓ Opened ${it.name}. Glint remembers this. Type “${hint}” again and it comes first.`);
  input.value = "";
  recompute();
}

function runAction(action, row) {
  const it = row.item;
  menu = null;
  switch (action) {
    case "open": runRow(row, input.value); return;
    case "pin":
      if (memory.pinned.has(it.id)) { memory.pinned.delete(it.id); say(`Unpinned ${it.name}.`); }
      else { memory.pinned.add(it.id); say(`📌 Pinned ${it.name}. Pinned items show first when the bar is empty. Press Esc to see.`); }
      break;
    case "hide": memory.hidden.add(it.id); say(`Hidden ${it.name} from results.`); break;
    case "copy_path": say(`✓ Copied the path: ${it.dir ? `${it.dir}\\${it.name}` : it.name}`); break;
    case "copy_name": say(`✓ Copied the name: ${it.name}`); break;
    case "reveal": say(`✓ Would open ${it.dir || "the folder"} in File Explorer.`); break;
    case "admin": say(`✓ Would run ${it.name} as administrator, after Windows asks you to confirm.`); break;
    case "terminal": say(`✓ Would open a terminal in ${it.kind === "folder" ? `${it.dir}\\${it.name}` : it.dir || "your home folder"}.`); break;
    case "properties": say(`✓ Would show the Windows Properties window for ${it.name}.`); break;
    default: break;
  }
  recompute(true);
}

function activate() {
  if (menu) { runAction(menu.actions[menu.sel], menu.row); return; }
  runRow(rows[sel], input.value);
}

function toggleMenu() {
  if (menu) { menu = null; render(); return; }
  const row = rows[sel];
  if (!row?.item) return;
  menu = { row, actions: actionsFor(row), sel: 0 };
  render();
}

/** What Esc does: back out one level at a time, here finishing by clearing the box. */
function escape() {
  if (!pop.hidden) { closePop(); return; }
  if (menu) { menu = null; render(); return; }
  if (input.value) { input.value = ""; recompute(); say("Cleared. In the app, Esc closes the bar."); }
}

function openPop() { pop.hidden = false; gear.setAttribute("aria-expanded", "true"); }
function closePop() { pop.hidden = true; gear.setAttribute("aria-expanded", "false"); }
gear.addEventListener("mousedown", (e) => e.preventDefault());
gear.addEventListener("click", () => (pop.hidden ? openPop() : closePop()));
document.addEventListener("click", (e) => { if (!pop.hidden && !pop.contains(e.target) && e.target !== gear) closePop(); });

// ---- Keyboard ---------------------------------------------------------------------------

function move(delta) {
  const total = menu ? menu.actions.length : rows.length;
  if (!total) return;
  const next = (current().sel + delta + total) % total;
  setSel(next);
  list.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
}

input.addEventListener("keydown", (e) => {
  stopAutoplay();
  const ctrl = e.ctrlKey;
  const k = e.key;
  if (k === "ArrowDown" || (ctrl && (k === "j" || k === "n"))) { e.preventDefault(); move(1); }
  else if (k === "ArrowUp" || (ctrl && (k === "p" || (k === "k" && menu)))) { e.preventDefault(); move(-1); }
  else if (ctrl && k === "k") { e.preventDefault(); toggleMenu(); }
  else if (k === "Tab") { e.preventDefault(); toggleMenu(); }
  else if (k === "Enter") {
    e.preventDefault();
    const row = rows[sel];
    if (!menu && ctrl && e.shiftKey && row?.item) runAction("admin", row);
    else if (!menu && ctrl && row?.item) runAction("reveal", row);
    else activate();
  }
  else if (k === "Escape") { e.preventDefault(); escape(); }
  else if (e.altKey && /^[1-9]$/.test(k) && !menu) {
    e.preventDefault();
    const row = rows[Number(k) - 1];
    if (row) { sel = Number(k) - 1; runRow(row, input.value); }
  }
  else if (ctrl && k === ",") { e.preventDefault(); pop.hidden ? openPop() : closePop(); }
});

input.addEventListener("input", () => { stopAutoplay(); recompute(); });
input.addEventListener("focus", stopAutoplay);
input.addEventListener("pointerdown", stopAutoplay);

foot.addEventListener("mousedown", (e) => e.preventDefault()); // keep focus in the box
foot.addEventListener("click", (e) => {
  const act = e.target.closest("button")?.dataset.act;
  if (act === "enter") activate();
  else if (act === "tab") toggleMenu();
  else if (act === "esc") escape();
  else if (act === "settings") (pop.hidden ? openPop() : closePop());
  input.focus();
});

// ---- Example chips ----------------------------------------------------------------------

const EXAMPLES = [
  { label: "helios", q: "helios", tip: "Files, folders and a content-search row. Try the arrow keys, then Tab for actions." },
  { label: "report ext:pdf in:Finance", q: "report ext:pdf in:Finance", tip: "Filters narrow the list: ext: for the type, in: for a folder on the path." },
  { label: "18% of 240", q: "18% of 240", tip: "Percent works like a phone calculator. Press Enter to copy the answer." },
  { label: "10 km to mi", q: "10 km to mi", tip: "Units and temperatures convert too. Try 100 f to c." },
  { label: "> lock", q: "> lock", tip: "Type > for system commands and Windows settings pages. Try > restart." },
  { label: "w notes", q: "w ", tip: "w lists your open windows so you can jump to one." },
  { label: "? rust", q: "? rust", tip: "? searches the web. g, ddg, yt and gh are shortcuts: try yt cats." },
  { label: "deploy", q: "deploy", tip: "Six similar scripts. Move to the third, press Enter, then type dep: it is now first." },
];
for (const ex of EXAMPLES) {
  const b = document.createElement("button");
  b.type = "button";
  b.className = "chip";
  b.textContent = ex.label;
  b.addEventListener("click", () => {
    stopAutoplay();
    input.value = ex.q;
    input.focus({ preventScroll: true });
    recompute();
    say(ex.tip);
  });
  $("chips").append(b);
}

// ---- A short autoplay until the visitor takes over --------------------------------------

const SCRIPT = ["helios", "= 18% of 240", "> lock", "report ext:pdf in:Finance", "10 km to mi"];
let timers = [];
function stopAutoplay() {
  if (interacted) return;
  interacted = true;
  timers.forEach(clearTimeout);
  timers = [];
  win.dataset.state = "live";
}
function later(fn, ms) { timers.push(setTimeout(fn, ms)); }

function startAutoplay() {
  if (matchMedia("(prefers-reduced-motion: reduce)").matches) {
    input.value = "helios";
    recompute();
    return;
  }
  let t = 400;
  for (const text of SCRIPT) {
    later(() => { input.value = ""; recompute(); }, t);
    t += 300;
    for (let i = 1; i <= text.length; i++) {
      later(() => { input.value = text.slice(0, i); recompute(); }, t);
      t += 90;
    }
    t += 1900;
  }
  later(() => { input.value = ""; recompute(); win.dataset.state = "live"; }, t);
}

let started = false;
const watch = new IntersectionObserver((entries) => {
  if (!started && entries.some((e) => e.isIntersecting)) { started = true; startAutoplay(); watch.disconnect(); }
}, { threshold: 0.4 });
watch.observe(win);

// Keep `parse` referenced for debugging in the console.
window.glintDemo = { parse, search, memory };
