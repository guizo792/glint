# Spotlight

A macOS-Spotlight-style launcher for Linux (X11) and Windows, written in Go. Press
`Ctrl+Space` anywhere to summon a search bar that finds and opens your apps and files.

Built with [OpenSpec](https://openspec.dev) — see `openspec/specs/launcher/spec.md` for the behavior contract.

## Features

- Global `Ctrl+Space` toggle (X11 on Linux; `RegisterHotKey` on Windows)
- Frameless, centered overlay with live, ranked search
  (exact > prefix > substring > fuzzy, capped at 20 results)
- Background index of installed apps (`.desktop` entries / Start Menu) and
  user files (Home, Desktop, Documents, Downloads; hidden dirs and `node_modules`
  etc. excluded, depth- and count-capped)
- Keyboard-first: `↑`/`↓` navigate, `Enter` opens, `Esc` closes
- Apps launch via their registered command; files via the OS default handler (`xdg-open` / shell)
- Clicking outside the overlay closes it (X11)

## Prerequisites

Go 1.26+. On Linux, Fyne requires cgo and system dev libraries:

```sh
sudo apt-get install -y libgl1-mesa-dev libwayland-dev libx11-dev libxrandr-dev \
  libxcursor-dev libxi-dev libxinerama-dev libxxf86vm-dev libxkbcommon-dev \
  libgbm-dev libdrm-dev libegl1-mesa-dev libxext-dev libdecor-0-dev
```

## Build & run

```sh
go build -tags migrated_fynedo ./cmd/spotlight
./spotlight
```

The `migrated_fynedo` tag silences Fyne's threading-model notice (this app
marshals all UI calls through `fyne.Do` as Fyne 2.8 requires).

For a Windows binary (hotkey via `RegisterHotKey`): `GOOS=windows go build` with
a MinGW-w64 cross toolchain, or build natively on Windows.

## Project layout

- `cmd/spotlight` — entrypoint: indexer, hotkey, overlay wiring
- `internal/search` — pure-Go ranking engine (unit-tested)
- `internal/indexer` — app discovery + file walker (per-OS app sources)
- `internal/hotkey` — `Ctrl+Space` manager (custom XGrabKey on Linux, `golang.design/x/hotkey` on Windows)
- `internal/launcher` — open actions (exec / xdg-open, per-OS)
- `internal/ui` — Fyne overlay window, keyboard handling, result highlighting
- `openspec/` — OpenSpec specs and change history

## Known limitations (v1)

- On Linux, the hotkey fires only while an X11/WSLg app has focus; a Windows build
  provides true system-wide hotkeys.
- No always-on-top or backdrop blur; on Windows/Wayland the window keeps its title bar.
- Index is built once at startup (no file watching); no usage-frequency ranking yet.