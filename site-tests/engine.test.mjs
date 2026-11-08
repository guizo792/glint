import { parse, calc, search, Memory, formatNumber } from "../site/engine.js";

let failed = 0;
const eq = (name, got, want) => {
  const ok = JSON.stringify(got) === JSON.stringify(want);
  if (!ok) failed++;
  console.log(ok ? "PASS" : "FAIL", name, ok ? "" : `\n   got  ${JSON.stringify(got)}\n   want ${JSON.stringify(want)}`);
};

// Parsing mirrors the app's rules.
eq("mode prefix", parse("= 2+2").mode, "calc");
eq("filters anywhere", (({ text, ext, within, only }) => ({ text, ext, within, only }))(parse("report ext:.PDF in:Documents file: 2024")),
  { text: "report 2024", ext: "pdf", within: "documents", only: "file" });
eq("w alone is plain", parse("w").mode, "all");
eq("empty filter value stays text", parse("ext: foo").text, "ext: foo");

// Calculator and percent maths.
eq("12*12", calc("12*12", false), "144");
eq("18% of 240", calc("18% of 240", false), "43.2");
eq("200 + 15%", calc("200 + 15%", false), "230");
eq("200 - 10%", calc("200 - 10%", false), "180");
eq("200 * 10%", calc("200*10%", false), "20");
eq("precedence", calc("2+3*4", false), "14");
eq("power", calc("2^10", false), "1024");
eq("parens", calc("(1+2)*3", false), "9");
eq("nested parens and unary minus", [calc("2*(3+(4-1))", false), calc("-(1+2)", true)], ["12", "-3"]);
eq("modulo", calc("10 % 3", false), "1");
eq("0.1+0.2", calc("0.1+0.2", false), "0.3");
eq("plain number is not math", calc("2024", false), null);
eq("plain number in = mode", calc("2024", true), "2024");
eq("a word is not math", calc("report 2", false), null);
eq("2024/3 forced", calc("2024/3", true), "674.6666666667");
eq("divide by zero", calc("1/0", true), null);
eq("100 f to c", calc("100 f to c", false), "37.7777777778 C");
eq("10 km to mi", calc("10 km to mi", false), "6.2137119224 mi");
eq("5 ft in cm", calc("5 ft in cm", false), "152.4 cm");
eq("mismatched units", calc("5 kg to km", false), null);
eq("format", formatNumber(1 / 3), "0.3333333333");

// Search.
const titles = (input, mem = new Memory()) => search(input, mem).map((r) => r.title);
eq("helios", [titles("helios")[0], titles("helios").includes("Helios Budget 2026.xlsx")], ["Helios", true]);
eq("helios has a content row last", titles("helios").at(-1), "Search file contents for helios");
eq("filters", titles("report ext:pdf in:Finance").slice(0, 2), ["Helios Annual Report 2025.pdf", "Helios Quarterly Report Q3.pdf"]);
eq("filter ext only", titles("ext:pdf").length >= 3, true);
eq("only dirs", search("dir: helios", new Memory()).every((r) => r.kind === "folder" || r.kind === "content"), true);
eq("words can match folders", titles("finance budget")[0], "Helios Budget 2026.xlsx");
eq("1-2 chars match word starts only", titles("he").some((t) => t === "Helios") && !titles("lo").includes("Helios"), true);
eq("calc row first", search("18% of 240", new Memory())[0].title, "43.2");
eq("= mode is calculator only", titles("= 12*12"), ["144"]);
eq("> mode", titles("> lock").slice(0, 2), ["Lock", "Lock screen settings"]);
eq("? mode", titles("? rust"), ["Search Google for rust"]);
eq("bang", titles("yt cats")[0], "Search YouTube for cats");
eq("no match falls back to web", titles("zzzqqq").slice(0, 2), ["Search Google for zzzqqq", "Search file contents for zzzqqq"]);
eq("w mode", titles("w ").length, 3);
eq("c mode", titles("c pricing"), ["https://example.com/pricing"]);
eq("app: flag", search("app: note", new Memory()).filter((r) => r.kind !== "content").every((r) => r.kind === "app"), true);

// Learning: choose the third result for "deploy", then it comes first for every prefix.
const mem = new Memory();
const before = search("deploy", mem);
const pick = before[2];
mem.choose("deploy", pick.id);
eq("learned for the full query", search("deploy", mem)[0].id, pick.id);
eq("learned for a prefix", search("dep", mem)[0].id, pick.id);
eq("not for something else", search("helios", mem)[0].title, "Helios");

// Home: pinned first, then frequent.
const m2 = new Memory();
eq("empty home with nothing used", search("", m2), []);
const budget = search("budget", m2)[0];
m2.choose("budget", budget.id);
m2.pinned.add(search("roadmap", m2)[0].id);
eq("home shows pinned then used", search("", m2).map((r) => r.title), ["Helios Roadmap.md", "Helios Budget 2026.xlsx"]);

console.log(failed ? `\n${failed} FAILED` : "\nall passed");
process.exit(failed ? 1 : 0);
