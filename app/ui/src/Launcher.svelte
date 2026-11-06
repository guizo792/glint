<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onMount, tick } from "svelte";
  import * as api from "./api";
  import type { Action, Item } from "./api";
  import { setLanguage, t } from "./i18n";

  const ROW = 46;
  const INPUT = 54;
  const FOOTER = 30;
  const MAX_VISIBLE = 8;
  const FUZZY_DELAY_MS = 120;

  let query = $state("");
  let items = $state<Item[]>([]);
  let selected = $state(0);
  let icons = $state<Record<string, string>>({});
  let menu = $state<{ item: Item; index: number } | null>(null);
  let confirming = $state<string | null>(null);
  let error = $state<string | null>(null);
  let input: HTMLInputElement;
  let listEl = $state<HTMLUListElement>();
  let generation = 0;
  let fuzzyTimer: ReturnType<typeof setTimeout> | undefined;
  let rememberSecs = 0;
  let tracing = false;
  let hiddenAt = 0;

  const selectedItem = $derived(items[selected]);
  const hasFilter = $derived(/(^|\s)(ext:\S|in:\S|app:|file:|dir:)/.test(query));

  function resize() {
    const rows = menu ? menu.item.actions.length : items.length;
    const showEmpty = !menu && rows === 0 && query.trim() !== "";
    const body = showEmpty ? ROW : Math.min(rows, MAX_VISIBLE) * ROW;
    const footer = rows > 0 || error ? FOOTER : 0;
    const errorRow = error ? 34 : 0;
    api.setHeight(INPUT + body + footer + errorRow);
  }

  async function runSearch(text: string, fuzzy: boolean) {
    const mine = ++generation;
    const started = performance.now();
    const result = await api.search(text, mine, fuzzy);
    if (result.generation !== generation) return;
    const keep = fuzzy ? selectedItem?.key : undefined;
    items = result.items;
    const kept = keep ? items.findIndex((i) => i.key === keep) : -1;
    selected = kept >= 0 ? kept : 0;
    await tick();
    if (tracing && !fuzzy) {
      requestAnimationFrame(() => api.trace("search", performance.now() - started));
    }
    resize();
    loadIcons();
    if (!fuzzy && text.trim().length >= 3 && items.length < 8) {
      clearTimeout(fuzzyTimer);
      fuzzyTimer = setTimeout(() => {
        if (mine === generation) runSearch(text, true);
      }, FUZZY_DELAY_MS);
    }
  }

  function loadIcons() {
    for (const item of items.slice(0, MAX_VISIBLE + 2)) {
      if (icons[item.key] || !["app", "file", "folder"].includes(item.kind)) continue;
      api.icon(item.key).then((url) => {
        if (url) icons[item.key] = url;
      });
    }
  }

  function onInput() {
    clearTimeout(fuzzyTimer);
    error = null;
    confirming = null;
    menu = null;
    runSearch(query, false);
  }

  async function run(item: Item, action: Action) {
    if (action === "open" && item.confirm && confirming !== item.key) {
      confirming = item.key;
      return;
    }
    confirming = null;
    error = null;
    try {
      const outcome = await api.runItem(item.key, action, query);
      menu = null;
      if (outcome.stay_open) {
        await runSearch(query, false);
      } else {
        query = "";
      }
    } catch (e) {
      error = String(e);
      await tick();
      resize();
    }
  }

  function move(delta: number) {
    if (menu) {
      const n = menu.item.actions.length;
      menu.index = (menu.index + delta + n) % n;
      return;
    }
    if (!items.length) return;
    selected = (selected + delta + items.length) % items.length;
    confirming = null;
    tick().then(() => {
      listEl?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
    });
  }

  function jump(delta: number) {
    if (!items.length || menu) return;
    selected = Math.max(0, Math.min(items.length - 1, selected + delta));
    tick().then(() => {
      listEl?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
    });
  }

  function openMenu() {
    if (!selectedItem) return;
    menu = { item: selectedItem, index: 0 };
    tick().then(resize);
  }

  function closeMenu() {
    menu = null;
    tick().then(resize);
  }

  function onKeydown(e: KeyboardEvent) {
    const ctrl = e.ctrlKey;
    if (e.key === "ArrowDown" || (ctrl && (e.key === "j" || e.key === "n"))) {
      e.preventDefault();
      move(1);
    } else if (e.key === "ArrowUp" || (ctrl && (e.key === "k" && menu || e.key === "p"))) {
      e.preventDefault();
      move(-1);
    } else if (ctrl && e.key === "k") {
      e.preventDefault();
      openMenu();
    } else if (e.key === "Tab") {
      e.preventDefault();
      if (menu) closeMenu();
      else openMenu();
    } else if (e.key === "PageDown") {
      e.preventDefault();
      jump(MAX_VISIBLE);
    } else if (e.key === "PageUp") {
      e.preventDefault();
      jump(-MAX_VISIBLE);
    } else if (ctrl && e.key === "Home") {
      e.preventDefault();
      selected = 0;
    } else if (ctrl && e.key === "End") {
      e.preventDefault();
      selected = Math.max(0, items.length - 1);
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (menu) {
        run(menu.item, menu.item.actions[menu.index]);
      } else if (selectedItem) {
        if (ctrl && e.shiftKey && selectedItem.actions.includes("admin")) run(selectedItem, "admin");
        else if (ctrl && selectedItem.actions.includes("reveal")) run(selectedItem, "reveal");
        else run(selectedItem, "open");
      }
    } else if (e.key === "Escape") {
      e.preventDefault();
      if (menu) closeMenu();
      else if (confirming) confirming = null;
      else close();
    } else if (ctrl && e.key === ",") {
      e.preventDefault();
      api.openSettings();
    } else if (e.altKey && /^[1-9]$/.test(e.key)) {
      e.preventDefault();
      const item = items[Number(e.key) - 1];
      if (item) run(item, "open");
    }
  }

  function close() {
    hiddenAt = Date.now();
    api.hideLauncher();
  }

  function highlight(title: string, text: string) {
    const needle = text.trim().split(/\s+/)[0]?.toLowerCase() ?? "";
    const at = needle ? title.toLowerCase().indexOf(needle) : -1;
    if (at < 0) return { before: title, match: "", after: "" };
    return {
      before: title.slice(0, at),
      match: title.slice(at, at + needle.length),
      after: title.slice(at + needle.length),
    };
  }

  function actionLabel(item: Item, action: Action) {
    if (action === "pin" && item.pinned) return t("unpin");
    return t(action);
  }

  const glyphs: Record<string, string> = {
    command: "⚡", setting: "⚙", window: "▣", clipboard: "📋", calc: "=", web: "🌐", extension: "✦",
  };

  async function loadSettings() {
    const settings = await api.getSettings();
    setLanguage(settings.config.language);
    rememberSecs = settings.config.remember_query_secs;
    tracing = settings.trace;
    api.applyAppearance(settings);
  }

  onMount(() => {
    loadSettings();
    resize();
    const unlisten = [
      listen<number>("shown", async (event) => {
        if (tracing) {
          requestAnimationFrame(() =>
            api.trace("show", performance.timeOrigin + performance.now() - event.payload / 1000),
          );
        }
        menu = null;
        confirming = null;
        error = null;
        const fresh = rememberSecs > 0 && Date.now() - hiddenAt < rememberSecs * 1000;
        if (!fresh) query = "";
        input.focus();
        input.select();
        await runSearch(query, false);
      }),
      listen("config-changed", loadSettings),
    ];
    const onBlur = () => (hiddenAt = Date.now());
    window.addEventListener("blur", onBlur);
    return () => {
      window.removeEventListener("blur", onBlur);
      unlisten.forEach((u) => u.then((f) => f()));
    };
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="launcher" onkeydown={onKeydown}>
  <input
    bind:this={input}
    bind:value={query}
    oninput={onInput}
    type="text"
    dir="auto"
    role="combobox"
    aria-expanded={items.length > 0}
    aria-controls="results"
    aria-activedescendant={selectedItem ? `result-${selected}` : undefined}
    aria-label={t("placeholder")}
    placeholder={t("placeholder")}
    spellcheck="false"
    autocomplete="off"
    autocapitalize="off"
    autocorrect="off"
  />

  {#if menu}
    <ul class="list" role="listbox" aria-label={menu.item.title}>
      {#each menu.item.actions as action, i}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <li role="option" aria-selected={i === menu.index} onclick={() => run(menu!.item, action)}>
          <span class="title">{actionLabel(menu.item, action)}</span>
        </li>
      {/each}
    </ul>
  {:else if items.length}
    <ul class="list" id="results" role="listbox" bind:this={listEl}>
      {#each items as item, i (item.key)}
        {@const parts = highlight(item.title, query)}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <li
          id={`result-${i}`}
          role="option"
          aria-selected={i === selected}
          onclick={() => run(item, "open")}
          onmousemove={() => (selected = i)}
        >
          <span class="icon">
            {#if icons[item.key]}
              <img src={icons[item.key]} alt="" width="26" height="26" />
            {:else}
              <span aria-hidden="true">{glyphs[item.kind] ?? "•"}</span>
            {/if}
          </span>
          <span class="text">
            <span class="title" dir="auto">{parts.before}<mark>{parts.match}</mark>{parts.after}</span>
            {#if confirming === item.key}
              <span class="subtitle warn">{t("confirm", { title: item.title })}</span>
            {:else if item.subtitle}
              <span class="subtitle" dir="auto">{item.subtitle}</span>
            {/if}
          </span>
          {#if item.pinned}<span class="badge" aria-label={t("pin")}>📌</span>{/if}
          {#if i < 9}<span class="badge">Alt+{i + 1}</span>{/if}
        </li>
      {/each}
    </ul>
  {:else if query.trim()}
    <div class="empty">{t("noResults")}{#if hasFilter}: {t("noResultsHint")}{/if}</div>
  {/if}

  {#if error}
    <div class="error" role="alert">{error}</div>
  {/if}
  {#if items.length || menu || error}
    <div class="footer">
      <span><kbd>Enter</kbd> {t("footerOpen")}</span>
      <span><kbd>Tab</kbd> {menu ? t("footerBack") : t("footerActions")}</span>
      <span><kbd>Esc</kbd> {t("footerClose")}</span>
    </div>
  {/if}
  <div class="sr" aria-live="polite">{query.trim() ? t("results", { n: items.length }) : ""}</div>
</div>

<style>
  .launcher {
    display: flex;
    flex-direction: column;
    height: 100vh;
    overflow: hidden;
  }
  input {
    flex: none;
    height: 54px;
    width: 100%;
    padding: 0 20px;
    font-size: 20px;
    background: transparent;
    border: 0;
    outline: 0;
  }
  input::placeholder {
    color: var(--text-dim);
  }
  .list {
    flex: 1;
    min-height: 0;
    margin: 0;
    padding: 0 6px;
    list-style: none;
    overflow-y: auto;
    border-top: 1px solid var(--border);
  }
  li {
    display: flex;
    align-items: center;
    gap: 12px;
    height: 46px;
    padding: 0 12px;
    border-radius: 8px;
    cursor: default;
  }
  li[aria-selected="true"] {
    background: var(--selected);
  }
  .icon {
    flex: none;
    width: 26px;
    height: 26px;
    display: grid;
    place-items: center;
    font-size: 17px;
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .title,
  .subtitle {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .subtitle {
    font-size: 12px;
    color: var(--text-dim);
  }
  .warn {
    color: var(--danger);
  }
  mark {
    background: transparent;
    color: var(--accent);
    font-weight: 600;
  }
  .badge {
    flex: none;
    font-size: 11px;
    color: var(--text-dim);
  }
  .empty {
    height: 46px;
    line-height: 46px;
    padding: 0 20px;
    color: var(--text-dim);
    border-top: 1px solid var(--border);
  }
  .error {
    height: 34px;
    line-height: 34px;
    padding: 0 20px;
    font-size: 13px;
    color: var(--danger);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .footer {
    flex: none;
    height: 30px;
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 0 20px;
    font-size: 12px;
    color: var(--text-dim);
    border-top: 1px solid var(--border);
  }
  kbd {
    font: inherit;
    font-weight: 600;
  }
  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
  }
</style>
