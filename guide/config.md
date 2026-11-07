# Settings file

Settings live in `%APPDATA%\Glint\config.toml`. Open it from Settings, under Advanced. Changes apply the next time you open Glint. If the file has a mistake, Glint keeps your previous settings and shows what is wrong.

Every setting is optional.

| Setting | Default | Meaning |
|---|---|---|
| `hotkey` | `"Alt+Space"` | Such as `"Ctrl+Shift+KeyK"`. A failed registration is reported in Settings. |
| `autostart` | `false` | Start with Windows. First-run setup offers to turn this on. |
| `theme` | `"system"` | `system`, `light` or `dark` |
| `language` | `"en"` | `en` or `fr` |
| `max_results` | `20` | From 1 to 200 |
| `remember_query_secs` | `0` | Keep the query if reopened within this many seconds |
| `extra_roots` | `[]` | Folders to index beyond the fixed drives, such as a USB or network drive. Applies after restart. |
| `extra_excludes` | `[]` | Folder names to skip, added to the built-in list. Applies after restart. |
| `rebuild_hours` | `6` | Hours between full rebuilds. New files appear sooner through file watching. |
| `clipboard_history` | `false` | Keep copied text in memory |
| `currency_rates` | `false` | Download European Central Bank exchange rates about once a day so `100 usd to eur` works. The only network use in Glint. Turning it off stops downloads and deletes the saved rates. |
| `enabled_extensions` | `[]` | Installed extensions allowed to run, by folder name |

## Web search and links

```toml
[[bangs]]
key = "w"
name = "Wikipedia"
url = "https://en.wikipedia.org/w/index.php?search={q}"

[[quicklinks]]
name = "Webmail"
url = "https://mail.example.com"
```

Then `w rust` searches Wikipedia, and typing `webmail` offers the link. The first bang is the default for `? ` searches and for when nothing on the PC matches. Setting `bangs` replaces the built-in list (`g`, `ddg`, `yt`, `gh`). Links must start with `http://` or `https://`.

## Custom theme

Create `%APPDATA%\Glint\theme.css` to override colors:

```css
:root {
  --bg: #101418;
  --selected: #24303c;
  --accent: #6cc4ff;
}
```

Also available: `--bg-raised`, `--text`, `--text-dim`, `--border`, `--danger`.

## Where data lives

| Path | Contents |
|---|---|
| `%APPDATA%\Glint` | `config.toml`, `theme.css`, `extensions` |
| `%LOCALAPPDATA%\Glint\index` | File name index. Safe to delete; it is rebuilt. |
| `%LOCALAPPDATA%\Glint\history.json` | What you opened, pins and hidden results |
| `%LOCALAPPDATA%\Glint\icons` | Icon cache. Safe to delete. |
| `%LOCALAPPDATA%\Glint\rates.json` | Saved exchange rates, only if turned on. Safe to delete. |
| `%LOCALAPPDATA%\Glint\logs` | Logs. They contain no file paths you searched for. |
