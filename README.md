# Glint

A fast keyboard launcher for Windows 11. Press a hotkey, type a few letters, press Enter. It finds apps, files, folders, settings pages and open windows anywhere on your PC, learns what you pick, and sends nothing over the network.

## Installing

Glint is free and open source. Download the installer from the releases page, or build it yourself (below).

**Windows may warn about the installer.** Releases are not code-signed yet, because signing certificates cost money and this is a free community project. SmartScreen then shows "Windows protected your PC". That warning means the file is new and unrecognised, not that it was found to be harmful. To continue, choose More info, then Run anyway. If you prefer not to, build from source: everything you run is then built from the code you can read.

Each release lists a SHA-256 checksum, so you can confirm the file you downloaded is the one that was published:

```bash
certutil -hashfile Glint_0.1.0_x64-setup.exe SHA256
```

## Using it

Press `Alt+Space` (you can change this in Settings).

| Key | Action |
|---|---|
| `↑ ↓`, `Ctrl+J/K`, `Ctrl+N/P` | Move through results |
| `Enter` | Open |
| `Ctrl+Enter` | Open containing folder |
| `Ctrl+Shift+Enter` | Run as administrator |
| `Tab` or `Ctrl+K` | Actions: copy path, open terminal here, properties, pin, hide |
| `Alt+1` to `Alt+9` | Open that result |
| `Ctrl+,` | Settings |
| `Esc` | Back, then close |

What you can type:

| Type | Result |
|---|---|
| `report` | Apps, files, folders, settings pages and open windows |
| `projects report` | Every word must match the name or a folder on its path |
| `report ext:pdf` | Only PDFs. Also `in:Documents`, `file:`, `dir:`, `app:` |
| `12*12`, `= 2024/3` | Calculator; Enter copies the answer |
| `10 km to mi`, `100 f to c` | Unit conversion |
| `> ` | System commands and settings pages |
| `w chrome` | Switch to an open window |
| `c token` | Clipboard history, if you turned it on |
| `g rust`, `? rust` | Web search |

The last row of most searches is "Search file contents for ...". It opens Windows' own search, which can look inside files in the places Windows has indexed.

Results improve as you use it: what you opened for a query comes first next time.

## How it finds things

Glint indexes every fixed drive by name. Most folders are searched as normal; system and tool folders (`Windows`, `Program Files`, `AppData`, `node_modules`, folders starting with a dot) stay findable but rank below your own files. It skips `.git` and a few other folders nobody searches. New files appear within a couple of seconds.

**Glint searches names, not file contents.** For contents, use the last result row, which hands the search to Windows. Windows only indexes some places by default, mainly your user folders, and finds nothing if its search service is turned off. Folders your account cannot read cannot be found.

## Privacy

Glint makes no network connections of its own. Web results only open your browser when you choose them. Clipboard history is off by default, stays in memory, and skips text that password managers mark as private. Usage history is a local file you can clear in Settings.

Extensions are the exception to all of this: they are programs running with your user rights. See [guide/extensions.md](guide/extensions.md).

## Building

Needs [Rust](https://rustup.rs) with the MSVC toolchain, Visual Studio Build Tools (C++ workload) and Node 22 or later.

```bash
npm ci --prefix app/ui
```

```bash
npm run build --prefix app/ui
```

```bash
cargo build --release -p glint
```

The app is `target/release/glint.exe`. To build the installer, run `npx --prefix ui tauri build` from `app/`.

## Documentation

- [guide/config.md](guide/config.md): settings file reference
- [guide/extensions.md](guide/extensions.md): writing extensions
- [CONTRIBUTING.md](CONTRIBUTING.md): how to contribute

## License

MIT or Apache-2.0, at your option.
