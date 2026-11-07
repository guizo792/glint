// Type `ts` for the current Unix time, or `ts 1700000000` to read one.
// Press Enter to copy. Needs Node 23.6 or later on PATH, which runs TypeScript directly.
import { createInterface } from "node:readline";

interface Item {
  id: string;
  title: string;
  subtitle: string;
}

function query(text: string): { items: Item[] } {
  const number = Number(text);
  const date = text && Number.isFinite(number) ? new Date(number * 1000) : new Date();
  if (Number.isNaN(date.getTime())) return { items: [] };
  const seconds = String(Math.floor(date.getTime() / 1000));
  return {
    items: [
      { id: date.toISOString(), title: date.toISOString(), subtitle: "ISO 8601 time" },
      { id: seconds, title: seconds, subtitle: "Unix seconds" },
    ],
  };
}

function run(id: string) {
  // The id is the text to copy, so this needs no memory between calls.
  return { effects: [{ type: "copy", text: id }] };
}

// Readline ends when Glint closes stdin, which is how extensions must stop.
for await (const line of createInterface({ input: process.stdin })) {
  const request = JSON.parse(line);
  const result = request.method === "query" ? query(request.params.text) : run(request.params.id);
  console.log(JSON.stringify({ jsonrpc: "2.0", id: request.id, result }));
}
