import { invoke } from "@tauri-apps/api/core";

export type Action =
  | "open" | "admin" | "reveal" | "copy_path" | "copy_name"
  | "terminal" | "properties" | "pin" | "hide" | "forget";

export interface Item {
  key: string;
  title: string;
  subtitle: string;
  kind: string;
  score: number;
  confirm: boolean;
  pinned: boolean;
  actions: Action[];
}

export interface Config {
  hotkey: string;
  autostart: boolean;
  theme: string;
  language: string;
  max_results: number;
  remember_query_secs: number;
  extra_roots: string[];
  extra_excludes: string[];
  rebuild_hours: number;
  clipboard_history: boolean;
  enabled_extensions: string[];
  bangs: { key: string; name: string; url: string }[];
  quicklinks: { name: string; url: string }[];
}

export interface Settings {
  config: Config;
  config_path: string;
  config_error: string | null;
  hotkey_error: string | null;
  index_status: string;
  entries: number;
  version: string;
  user_css: string;
}

export const search = (query: string, generation: number, fuzzy: boolean) =>
  invoke<{ generation: number; items: Item[] }>("search", { query, generation, fuzzy });
export const runItem = (key: string, action: Action, query: string) =>
  invoke<{ stay_open: boolean }>("run_item", { key, action, query });
export const icon = (key: string) => invoke<string | null>("icon", { key });
export const hideLauncher = () => invoke<void>("hide_launcher");
export const setHeight = (height: number) => invoke<void>("set_height", { height });
export const getSettings = () => invoke<Settings>("get_settings");
export const saveSettings = (config: Config) => invoke<void>("save_settings", { config });
export const openSettings = () => invoke<void>("open_settings");
export const openConfigFile = () => invoke<void>("open_config_file");
export const rebuildIndex = () => invoke<void>("rebuild_index");
export const clearHistory = () => invoke<void>("clear_history");
export const clearClipboardHistory = () => invoke<void>("clear_clipboard_history");
export const diagnostics = () => invoke<string>("diagnostics");

/** Apply theme and any user stylesheet to the document. */
export function applyAppearance(settings: Settings) {
  const root = document.documentElement;
  if (settings.config.theme === "system") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", settings.config.theme);
  root.lang = settings.config.language;
  let style = document.getElementById("user-theme");
  if (!style) {
    style = document.createElement("style");
    style.id = "user-theme";
    document.head.append(style);
  }
  style.textContent = settings.user_css;
}
