// Sample data for the live demo. Nothing here is real: it is a made-up PC.
const U = "C:\\Users\\you";

const file = (dir, name) => ({ kind: "file", dir: `${U}\\${dir}`, name });
const folder = (dir, name) => ({ kind: "folder", dir: `${U}\\${dir}`, name });

export const ITEMS = [
  // Apps
  ...[
    ["Notepad", "📝"], ["Calculator", "🧮"], ["Paint", "🎨"], ["Task Manager", "📈"],
    ["File Explorer", "🗂"], ["Snipping Tool", "✂️"], ["Windows Terminal", "⌨️"],
    ["Command Prompt", "▮"], ["Windows PowerShell", "🔷"], ["Camera", "📷"],
    ["Clock", "⏰"], ["Photos", "🖼"], ["Sticky Notes", "🗒"], ["Voice Recorder", "🎙"],
  ].map(([name, icon]) => ({ kind: "app", name, icon, dir: "" })),

  // Settings pages
  ...[
    "Display settings", "Sound settings", "Bluetooth & devices", "Wi-Fi settings",
    "Lock screen settings", "Background settings", "Power & battery", "Apps & features",
    "Default apps", "Night light settings", "Taskbar settings", "Windows Update",
  ].map((name) => ({ kind: "setting", name, dir: "", sub: "Windows settings" })),

  // System commands
  { kind: "command", name: "Lock", sub: "System command", did: "lock the PC" },
  { kind: "command", name: "Sleep", sub: "System command", did: "put the PC to sleep" },
  { kind: "command", name: "Restart", sub: "System command", did: "restart the PC", confirm: true },
  { kind: "command", name: "Shut down", sub: "System command", did: "shut the PC down", confirm: true },
  { kind: "command", name: "Sign out", sub: "System command", did: "sign you out", confirm: true },
  { kind: "command", name: "Empty Recycle Bin", sub: "System command", did: "empty the Recycle Bin", confirm: true },

  // Files and folders: a small project called Helios, plus everyday things.
  folder("Documents", "Helios"),
  folder("Documents\\Helios", "Finance"),
  folder("Documents\\Helios", "Planning"),
  folder("Documents\\Helios", "Notes"),
  file("Documents\\Helios\\Finance", "Helios Budget 2026.xlsx"),
  file("Documents\\Helios\\Finance", "Helios Quarterly Report Q3.pdf"),
  file("Documents\\Helios\\Finance", "Helios Annual Report 2025.pdf"),
  file("Documents\\Helios\\Planning", "Helios Roadmap.md"),
  file("Documents\\Helios\\Planning", "Helios Launch Checklist.docx"),
  file("Documents\\Helios\\Notes", "Helios Kickoff Notes.txt"),
  file("Pictures\\Offsite", "Helios Team Offsite 014.jpg"),
  file("Documents", "Resume 2026.pdf"),
  file("Documents", "Invoice March.pdf"),
  file("Documents", "Lease Agreement.pdf"),
  file("Documents\\Taxes", "Tax Return 2025.pdf"),
  file("Documents\\Taxes", "Receipts 2025.xlsx"),
  file("Downloads", "Glint_0.1.0_x64-setup.exe"),
  file("Downloads", "wallpaper-purple.png"),
  file("Pictures\\Lisbon", "Lisbon Trip 001.jpg"),
  file("Pictures\\Lisbon", "Lisbon Trip 002.jpg"),
  file("Pictures\\Lisbon", "Lisbon Trip 003.jpg"),
  folder("Pictures", "Lisbon"),
  folder("Projects", "atlas"),
  file("Projects\\atlas", "README.md"),
  file("Projects\\atlas\\src", "main.rs"),
  file("Projects\\atlas", "Cargo.toml"),
  folder("Projects", "website"),
  file("Projects\\website", "index.html"),
  file("Music\\Playlists", "Focus Mix.m3u"),
  // Six look-alike scripts, for trying how Glint learns which one you mean.
  ...["Alpha", "Beta", "Gamma", "Delta", "Epsilon", "Zeta"].map((n) =>
    file("Scripts", `Deploy ${n}.cmd`)),
].map((it, i) => ({ id: `i${i}`, ...it }));

export const WINDOWS = [
  { id: "w1", kind: "window", name: "Helios Roadmap.md - Editor", sub: "Switch to window" },
  { id: "w2", kind: "window", name: "Inbox - Mail", sub: "Switch to window" },
  { id: "w3", kind: "window", name: "Lisbon Trip 002.jpg - Photos", sub: "Switch to window" },
];

export const CLIPBOARD = [
  { id: "c1", kind: "clipboard", name: "Meeting at 3 PM, room 4", sub: "Copied just now" },
  { id: "c2", kind: "clipboard", name: "https://example.com/pricing", sub: "Copied 5 minutes ago" },
  { id: "c3", kind: "clipboard", name: "npm run build -- --release", sub: "Copied an hour ago" },
];

export const BANGS = [
  { key: "g", name: "Google", url: "https://www.google.com/search?q={q}" },
  { key: "ddg", name: "DuckDuckGo", url: "https://duckduckgo.com/?q={q}" },
  { key: "yt", name: "YouTube", url: "https://www.youtube.com/results?search_query={q}" },
  { key: "gh", name: "GitHub", url: "https://github.com/search?q={q}" },
];

export const FILE_ICONS = {
  pdf: "📕", xlsx: "📊", docx: "📘", md: "📝", txt: "📄", jpg: "🖼", png: "🖼",
  cmd: "⚙️", exe: "💠", rs: "🦀", toml: "🧾", html: "🌐", m3u: "🎵",
};
export const KIND_ICONS = {
  command: "⚡", setting: "⚙", window: "▣", clipboard: "📋", calc: "=", web: "🌐",
  content: "🔎", folder: "📁",
};
