## Why

There is no lightweight, keyboard-driven launcher for this Go desktop environment. macOS Spotlight proves the pattern: a global hotkey summons a search bar that opens apps and files instantly. This change builds that experience natively in Go with a spec-driven foundation, so later features (recents, fuzzy history, more content types) build on agreed behavior rather than drift.

## What Changes

- A frameless, centered search overlay toggled by a global `Ctrl+Space` hotkey.
- A background index of installed applications and user files (Desktop, Documents, Downloads, Home).
- Live, ranked search results as the user types (exact > prefix > substring > fuzzy), capped for performance.
- Full keyboard interaction: `↑`/`↓` to navigate, `Enter` to open, `Esc` to close.
- Opening results: apps launched via their registered command, files via the OS default handler.
- Platform-neutral interfaces for hotkey registration, app discovery, and file opening, with a working Linux (X11) implementation and a verified Windows cross-compile.
- OpenSpec workflow artifacts (`openspec/`) as the project's planning layer.

## Capabilities

### New Capabilities
- `launcher`: the Spotlight-style launcher capability — global hotkey toggle, overlay window, indexing, ranked live search, result navigation, and result actions.

### Modified Capabilities
- None.

## Impact

- New Go project `~/projects/go/spotlight` (module `spotlight`), single binary at `cmd/spotlight`.
- Packages: `internal/hotkey`, `internal/indexer`, `internal/search`, `internal/launcher`, `internal/ui`.
- Dependencies: `fyne.io/fyne/v2` (UI), `golang.design/x/hotkey` (global hotkey).
- Known platform limitation: on Linux/WSLg the hotkey fires only while a WSLg/X11 app has focus; a native Windows build (supported by the same codebase) provides true system-wide hotkeys.