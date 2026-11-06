import { beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach } from "vitest";

const handlers: Record<string, (e: { payload: unknown }) => void> = {};
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, cb: (e: { payload: unknown }) => void) => {
    handlers[name] = cb;
    return () => {};
  }),
}));

const api = vi.hoisted(() => ({
  search: vi.fn(),
  runItem: vi.fn(),
  icon: vi.fn(),
  hideLauncher: vi.fn(),
  setHeight: vi.fn(),
  getSettings: vi.fn(),
  openSettings: vi.fn(),
  applyAppearance: vi.fn(),
  trace: vi.fn(),
}));
vi.mock("./api", () => api);

import Launcher from "./Launcher.svelte";
import type { Item } from "./api";

function item(title: string, extra: Partial<Item> = {}): Item {
  return {
    key: `file:${title}`,
    title,
    subtitle: "C:\\Docs",
    kind: "file",
    score: 1,
    confirm: false,
    pinned: false,
    actions: ["open", "reveal", "copy_path", "pin", "hide"],
    ...extra,
  };
}

const three = [item("alpha.txt"), item("beta.txt"), item("gamma.txt")];

async function setup(items: Item[] = three) {
  api.search.mockImplementation(async (_q: string, generation: number) => ({ generation, items }));
  render(Launcher);
  await waitFor(() => expect(api.getSettings).toHaveBeenCalled());
  const input = screen.getByRole("combobox") as HTMLInputElement;
  return input;
}

async function type(input: HTMLInputElement, text: string) {
  await fireEvent.input(input, { target: { value: text } });
  await waitFor(() => expect(api.search).toHaveBeenCalled());
}

const selectedTitle = () =>
  document.querySelector('li[aria-selected="true"] .title')?.textContent;

beforeEach(() => {
  Element.prototype.scrollIntoView = () => {};
  Object.values(api).forEach((f) => f.mockReset());
  api.icon.mockResolvedValue(null);
  api.runItem.mockResolvedValue({ stay_open: false });
  api.setHeight.mockResolvedValue(undefined);
  api.getSettings.mockResolvedValue({
    config: { language: "en", remember_query_secs: 0 },
    trace: false,
    user_css: "",
  });
});
afterEach(cleanup);

describe("Launcher", () => {
  it("turns off spellcheck and autocorrect on the search box", async () => {
    const input = await setup();
    expect(input.getAttribute("spellcheck")).toBe("false");
    expect(input.getAttribute("autocomplete")).toBe("off");
  });

  it("shows results for typed text and selects the first", async () => {
    const input = await setup();
    await type(input, "a");
    await screen.findByText("beta.txt", { exact: false });
    expect(screen.getAllByRole("option")).toHaveLength(3);
    expect(selectedTitle()).toBe("alpha.txt");
  });

  it("ignores results from an older keystroke that arrive late", async () => {
    let releaseSlow!: () => void;
    api.search.mockImplementation((q: string, generation: number) => {
      if (q === "a") {
        return new Promise((resolve) => {
          releaseSlow = () => resolve({ generation, items: [item("stale.txt")] });
        });
      }
      return Promise.resolve({ generation, items: [item("fresh.txt")] });
    });
    render(Launcher);
    const input = screen.getByRole("combobox");
    await fireEvent.input(input, { target: { value: "a" } });
    await fireEvent.input(input, { target: { value: "ab" } });
    await screen.findByText("fresh.txt");
    releaseSlow();
    await new Promise((r) => setTimeout(r, 10));
    expect(screen.queryByText("stale.txt")).toBeNull();
    expect(screen.getByText("fresh.txt")).toBeTruthy();
  });

  it("moves the selection with arrows and Ctrl shortcuts, wrapping at the ends", async () => {
    const input = await setup();
    await type(input, "a");
    await screen.findByText("gamma.txt");
    await fireEvent.keyDown(input, { key: "ArrowDown" });
    expect(selectedTitle()).toBe("beta.txt");
    await fireEvent.keyDown(input, { key: "j", ctrlKey: true });
    expect(selectedTitle()).toBe("gamma.txt");
    await fireEvent.keyDown(input, { key: "ArrowDown" });
    expect(selectedTitle()).toBe("alpha.txt");
    await fireEvent.keyDown(input, { key: "ArrowUp" });
    expect(selectedTitle()).toBe("gamma.txt");
  });

  it("opens the selected result on Enter, passing the query for learning", async () => {
    const input = await setup();
    await type(input, "bet");
    await screen.findByText("gamma.txt");
    await fireEvent.keyDown(input, { key: "ArrowDown" });
    await fireEvent.keyDown(input, { key: "Enter" });
    expect(api.runItem).toHaveBeenCalledWith("file:beta.txt", "open", "bet");
  });

  it("uses Ctrl+Enter for the folder and Alt+number for a specific result", async () => {
    const input = await setup();
    await type(input, "a");
    await screen.findByText("gamma.txt");
    await fireEvent.keyDown(input, { key: "Enter", ctrlKey: true });
    expect(api.runItem).toHaveBeenCalledWith("file:alpha.txt", "reveal", "a");
    await waitFor(() => expect(input.value).toBe(""));
    await type(input, "a");
    await screen.findByText("gamma.txt");
    await fireEvent.keyDown(input, { key: "3", altKey: true });
    expect(api.runItem).toHaveBeenLastCalledWith("file:gamma.txt", "open", "a");
  });

  it("opens the action menu with Tab and runs the chosen action", async () => {
    const input = await setup();
    await type(input, "a");
    await screen.findByText("gamma.txt");
    await fireEvent.keyDown(input, { key: "Tab" });
    await screen.findByText("Copy path");
    await fireEvent.keyDown(input, { key: "ArrowDown" });
    await fireEvent.keyDown(input, { key: "ArrowDown" });
    await fireEvent.keyDown(input, { key: "Enter" });
    expect(api.runItem).toHaveBeenCalledWith("file:alpha.txt", "copy_path", "a");
  });

  it("closes the menu on Esc without closing the launcher", async () => {
    const input = await setup();
    await type(input, "a");
    await screen.findByText("gamma.txt");
    await fireEvent.keyDown(input, { key: "Tab" });
    await screen.findByText("Copy path");
    await fireEvent.keyDown(input, { key: "Escape" });
    await screen.findByText("gamma.txt");
    expect(api.hideLauncher).not.toHaveBeenCalled();
    await fireEvent.keyDown(input, { key: "Escape" });
    expect(api.hideLauncher).toHaveBeenCalled();
  });

  it("requires a second Enter for destructive commands, and Esc cancels", async () => {
    const shutdown = item("Shut down", { key: "cmd:Shutdown", kind: "command", confirm: true });
    const input = await setup([shutdown]);
    await type(input, "shut");
    await screen.findByText("Shut", { exact: false });
    await fireEvent.keyDown(input, { key: "Enter" });
    await screen.findByText(/Press Enter again/);
    expect(api.runItem).not.toHaveBeenCalled();
    await fireEvent.keyDown(input, { key: "Escape" });
    expect(screen.queryByText(/Press Enter again/)).toBeNull();
    expect(api.runItem).not.toHaveBeenCalled();
    await fireEvent.keyDown(input, { key: "Enter" });
    await fireEvent.keyDown(input, { key: "Enter" });
    expect(api.runItem).toHaveBeenCalledWith("cmd:Shutdown", "open", "shut");
  });

  it("shows a failed open inline and stays open", async () => {
    api.runItem.mockRejectedValue("The file could not be opened");
    const input = await setup();
    await type(input, "a");
    await screen.findByText("gamma.txt");
    await fireEvent.keyDown(input, { key: "Enter" });
    expect((await screen.findByRole("alert")).textContent).toContain("could not be opened");
    expect(input.value).toBe("a");
  });

  it("clears the query each time it is shown", async () => {
    const input = await setup();
    await type(input, "a");
    await screen.findByText("gamma.txt");
    await handlers.shown({ payload: 0 });
    expect(input.value).toBe("");
  });

  it("keeps the query when reopened within the remember window", async () => {
    api.getSettings.mockResolvedValue({
      config: { language: "en", remember_query_secs: 60 },
      trace: false,
      user_css: "",
    });
    const input = await setup();
    await waitFor(() => expect(api.applyAppearance).toHaveBeenCalled());
    await type(input, "a");
    await screen.findByText("gamma.txt");
    await fireEvent.keyDown(input, { key: "Escape" });
    await handlers.shown({ payload: 0 });
    expect(input.value).toBe("a");
  });

  it("tells screen readers how many results there are", async () => {
    const input = await setup();
    await type(input, "a");
    await screen.findByText("gamma.txt");
    expect(document.querySelector('[aria-live="polite"]')?.textContent).toBe("3 results");
    expect(input.getAttribute("aria-activedescendant")).toBe("result-0");
  });

  it("re-runs the search after pin or hide, keeping the window open", async () => {
    api.runItem.mockResolvedValue({ stay_open: true });
    const input = await setup();
    await type(input, "a");
    await screen.findByText("gamma.txt");
    const before = api.search.mock.calls.length;
    await fireEvent.keyDown(input, { key: "k", ctrlKey: true });
    await screen.findByText("Pin");
    await fireEvent.click(screen.getByText("Pin"));
    await waitFor(() => expect(api.search.mock.calls.length).toBeGreaterThan(before));
    expect(input.value).toBe("a");
  });
});
