<script lang="ts">
  import { onMount } from "svelte";
  import * as api from "./api";
  import type { Config, Installed, Settings } from "./api";
  import { setLanguage, t } from "./i18n";

  let { welcome = false }: { welcome?: boolean } = $props();

  let settings = $state<Settings | null>(null);
  let config = $state<Config | null>(null);
  let roots = $state("");
  let excludes = $state("");
  let message = $state("");
  let error = $state("");
  let extensions = $state<Installed[]>([]);
  let installUrl = $state("");
  let installing = $state(false);

  async function load() {
    const loaded = await api.getSettings();
    setLanguage(loaded.config.language);
    api.applyAppearance(loaded);
    settings = loaded;
    config = loaded.config;
    roots = loaded.config.extra_roots.join("\n");
    excludes = loaded.config.extra_excludes.join("\n");
    if (welcome) config.autostart = true;
    extensions = await api.listExtensions();
  }

  const lines = (text: string) => text.split("\n").map((l) => l.trim()).filter(Boolean);

  async function save() {
    if (!config) return;
    error = "";
    try {
      await api.saveSettings({ ...$state.snapshot(config), extra_roots: lines(roots), extra_excludes: lines(excludes) });
      await load();
      welcome = false;
      message = t("saved");
    } catch (e) {
      error = String(e);
    }
  }

  async function copyDiagnostics() {
    await api.diagnostics();
    message = t("copied");
  }

  function toggleExtension(folder: string, on: boolean) {
    if (!config) return;
    const others = config.enabled_extensions.filter((f) => f !== folder);
    config.enabled_extensions = on ? [...others, folder] : others;
  }

  async function reloadExtensions() {
    await api.reloadExtensions();
    extensions = await api.listExtensions();
  }

  async function install() {
    error = "";
    installing = true;
    try {
      await api.installExtension(installUrl.trim());
      installUrl = "";
      extensions = await api.listExtensions();
    } catch (e) {
      error = String(e);
    }
    installing = false;
  }

  async function remove(folder: string) {
    error = "";
    try {
      await api.removeExtension(folder);
      toggleExtension(folder, false);
      extensions = await api.listExtensions();
    } catch (e) {
      error = String(e);
    }
  }

  /** Turn a key press into a hotkey string such as Ctrl+Shift+K. */
  function captureHotkey(e: KeyboardEvent) {
    if (!config || ["Control", "Shift", "Alt", "Meta", "Tab"].includes(e.key)) return;
    e.preventDefault();
    const parts: string[] = [];
    if (e.ctrlKey) parts.push("Ctrl");
    if (e.altKey) parts.push("Alt");
    if (e.shiftKey) parts.push("Shift");
    if (e.metaKey) parts.push("Super");
    if (parts.length) {
      parts.push(e.code);
      config.hotkey = parts.join("+");
    }
  }

  onMount(load);
</script>

{#if config && settings}
  <main>
    {#if welcome}
      <h1>{t("welcome")}</h1>
      <p>{t("welcomeText")}</p>
    {:else}
      <h1>{t("settings")}</h1>
    {/if}

    {#if settings.config_error}<p class="error" role="alert">{settings.config_error}</p>{/if}
    {#if settings.hotkey_error}<p class="error" role="alert">{settings.hotkey_error}</p>{/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}

    <section>
      <h2>{t("general")}</h2>
      <label>
        {t("hotkey")}
        <input value={config.hotkey} readonly onkeydown={captureHotkey} />
        <small>{t("hotkeyHint")}</small>
      </label>
      <label class="row"><input type="checkbox" bind:checked={config.autostart} /> {t("autostart")}</label>
    </section>

    <section>
      <h2>{t("appearance")}</h2>
      <label>
        {t("theme")}
        <select bind:value={config.theme}>
          <option value="system">{t("themeSystem")}</option>
          <option value="light">{t("themeLight")}</option>
          <option value="dark">{t("themeDark")}</option>
        </select>
      </label>
      <label>
        {t("language")}
        <select bind:value={config.language}>
          <option value="en">English</option>
          <option value="fr">Français</option>
        </select>
      </label>
    </section>

    {#if !welcome}
      <section>
        <h2>{t("behavior")}</h2>
        <label>{t("maxResults")} <input type="number" min="1" max="200" bind:value={config.max_results} /></label>
        <label>{t("remember")} <input type="number" min="0" bind:value={config.remember_query_secs} /></label>
      </section>

      <section>
        <h2>{t("index")}</h2>
        <p>{t("entries", { n: settings.entries.toLocaleString() })}. {settings.index_status}</p>
        <button onclick={() => api.rebuildIndex()}>{t("rebuild")}</button>
        <label>{t("rebuildHours")} <input type="number" min="1" bind:value={config.rebuild_hours} /></label>
        <label>{t("extraRoots")} <textarea rows="2" bind:value={roots}></textarea></label>
        <label>{t("extraExcludes")} <textarea rows="2" bind:value={excludes}></textarea></label>
        <small>{t("restartNote")}</small>
      </section>

      <section>
        <h2>{t("privacy")}</h2>
        <label class="row"><input type="checkbox" bind:checked={config.clipboard_history} /> {t("clipboard")}</label>
        <small>{t("clipboardHint")}</small>
        <div class="buttons">
          <button onclick={() => api.clearClipboardHistory()}>{t("clearClipboard")}</button>
          <button onclick={() => api.clearHistory()}>{t("clearHistory")}</button>
        </div>
      </section>

      <section>
        <h2>{t("extensions")}</h2>
        <p class="warn">{t("extensionsWarning")}</p>
        {#each extensions as ext (ext.folder)}
          <div class="extension">
            <label class="row">
              <input
                type="checkbox"
                checked={config.enabled_extensions.includes(ext.folder)}
                onchange={(e) => toggleExtension(ext.folder, e.currentTarget.checked)}
              />
              {ext.manifest.name} {ext.manifest.version}
            </label>
            <small>
              {t("triggers")}: {ext.manifest.trigger}.
              {t("permissions")}: {ext.manifest.permissions.join(", ") || "-"}
            </small>
            {#if ext.error}<small class="error">{ext.error}</small>{/if}
            <button onclick={() => remove(ext.folder)}>{t("remove")}</button>
          </div>
        {:else}
          <p>{t("extensionsNone")}</p>
        {/each}
        <div class="buttons">
          <input aria-label={t("installHint")} placeholder={t("installHint")} bind:value={installUrl} />
          <button disabled={installing || !installUrl.trim()} onclick={install}>{t("install")}</button>
          <button onclick={reloadExtensions}>{t("reload")}</button>
        </div>
      </section>

      <section>
        <h2>{t("advanced")}</h2>
        <div class="buttons">
          <button onclick={() => api.openConfigFile()}>{t("editConfig")}</button>
          <button onclick={copyDiagnostics}>{t("copyDiagnostics")}</button>
        </div>
        <small>Glint {settings.version}</small>
      </section>
    {/if}

    <div class="actions">
      <button class="primary" onclick={save}>{welcome ? t("done") : t("save")}</button>
      <span role="status">{message}</span>
    </div>
  </main>
{/if}

<style>
  main {
    max-width: 600px;
    margin: 0 auto;
    padding: 20px 24px 40px;
  }
  h1 {
    font-size: 22px;
    margin: 4px 0 12px;
  }
  h2 {
    font-size: 14px;
    margin: 0 0 10px;
    color: var(--text-dim);
  }
  section {
    padding: 14px 0;
    border-top: 1px solid var(--border);
  }
  label {
    display: block;
    margin-bottom: 10px;
  }
  label.row {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  input:not([type="checkbox"]),
  select,
  textarea {
    display: block;
    width: 100%;
    margin-top: 4px;
    padding: 6px 8px;
    background: var(--bg-raised);
    border: 1px solid var(--border);
    border-radius: 6px;
  }
  small {
    display: block;
    color: var(--text-dim);
    font-size: 12px;
  }
  button {
    padding: 6px 12px;
    background: var(--bg-raised);
    border: 1px solid var(--border);
    border-radius: 6px;
    cursor: pointer;
  }
  .primary {
    background: var(--accent);
    color: #1b1c22;
    border-color: transparent;
    font-weight: 600;
  }
  .buttons,
  .actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    align-items: center;
    margin-bottom: 6px;
  }
  .error {
    color: var(--danger);
  }
  .warn {
    margin-top: 0;
    font-size: 13px;
  }
  .extension {
    padding: 8px 0;
  }
  .extension label {
    margin-bottom: 2px;
  }
  .extension button {
    margin-top: 6px;
  }
</style>
