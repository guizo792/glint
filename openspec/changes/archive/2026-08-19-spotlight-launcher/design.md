## Context

- Greenfield Go project on WSL2 (WSLg provides X11 `:0` and Wayland), Go 1.26.5, no prior code.
- Goal: a Spotlight-style launcher binary with hotkey, overlay UI, indexing, ranked search, and launch actions.
- The environment's X11 server (WSLg) only sees key events while a WSLg application has focus, so the "global" hotkey is effectively global within the Linux session; a Windows build of the same code provides true system-wide hotkeys.
- OpenSpec (v1.9.0, spec-driven schema) is initialized; `openspec/changes/spotlight-launcher/` holds this change.

## Goals / Non-Goals

**Goals:**
- Single binary (`cmd/spotlight`) with clean package boundaries: `internal/hotkey`, `internal/indexer`, `internal/search`, `internal/launcher`, `internal/ui`.
- Cross-platform seams for hotkey, app discovery, and file opening (build-tag based implementations behind interfaces), Linux (X11) working end-to-end.
- Search ranking as pure, unit-tested Go with no GUI or OS dependencies.
- Verified with `go build`, `go vet`, `go test`, `openspec validate`.

**Non-Goals:**
- Wayland native global shortcuts (XDG portal) in this change.
- Always-on-top and backdrop blur (no portable Fyne API); the overlay is a plain window.
- Usage-frequency result boosting, recents, bookmarks, or non-app/file content types.
- Tray icon, autostart, or daemon mode.
- Windows runtime testing in this change (cross-compile verification only).

## Decisions

1. **UI: Fyne v2** — pure-Go, cross-platform, actively maintained, and proven for launchers (mudler/golauncher precedent, which also pairs Fyne with `golang.design/x/hotkey`). Alternative (raw X11/win32 windows) rejected for portability and time; GTK4 rejected for CGO/system dependency weight.
2. **Hotkey: platform-native, pure Go** — Linux (X11) uses a custom `XGrabKey` implementation over `github.com/jezek/xgb` (no cgo, no system deps); Windows uses `golang.design/x/hotkey` (pure Go via `RegisterHotKey` syscalls on that platform). Both are wrapped by `internal/hotkey.Manager`. Rationale: `golang.design/x/hotkey` on Linux requires cgo + X11 headers (`libx11-dev`), which is unnecessary here since `jezek/xgb` speaks X11 directly; the pure-Go path also keeps `CGO_ENABLED=0` builds viable. Modifier-lock variants (CapsLock/NumLock/ScrollLock) are grabbed in addition to the exact mask, per XGrabKey semantics.
3. **App discovery: XDG desktop entries (Linux)** — parse `.desktop` files (Name, Exec, Icon) from `/usr/share/applications`, `/usr/local/share/applications`, `~/.local/share/applications`. Windows: scan Start Menu (`.lnk`) and app dirs for executable names; `.lnk` resolution is out of scope — v1 indexes the link names and launches via the shell.
4. **File indexing: `filepath.WalkDir`** — roots `$HOME`, `$HOME/Desktop`, `$HOME/Documents`, `$HOME/Downloads`; skip hidden entries and well-known noise dirs (`.git`, `node_modules`, `vendor`, build dirs); depth cap (8) and entry cap (e.g. 50k) to bound memory. Background goroutine populates a `sync.RWMutex`-guarded store.
5. **Ranking: tiered scorer in `internal/search`** — case-insensitive; scores: exact name = 4, prefix = 3, substring = 2, fuzzy (in-order subsequence) = 1; tie-break by shorter name then lexicographic; deterministic order. Pure function `Rank(query string, entries []Entry) []Entry` for unit tests.
6. **Launch actions: `internal/launcher`** — apps via `exec.Command` (`.desktop` Exec parsing with field-code `%` removal); files via `xdg-open` (Linux) and `rundll32 shell32.dll,ShellExecuteA` / `cmd /c start` (Windows).
7. **Overlay UI: Fyne window** — `SetFixedSize(true)`, `CenterOnScreen`, custom `keyEntry` (embedded `widget.Entry` intercepting `↑`/`↓`/`Enter`/`Esc` before Entry's own handler), `widget.List` for results with manual selection highlight (List's `Select` would fire `OnSelected`/open on arrow navigation, so selection state is tracked in the overlay and rows re-render their background). Matched-substring highlighting uses `widget.TextSegment` rich text. Fyne exposes no public frameless-window API, so framelessness is achieved per platform: `_MOTIF_WM_HINTS` via xgb on X11 (window located by title in the X tree) — on Windows/Wayland the window stays decorated (documented limitation).
8. **"Click outside closes" on X11** — a second xgb connection watches `FocusOut` events on the overlay window and hides it; covered for WSLg/X11 desktops. Not available on Windows v1.
9. **Hotkey→UI thread-safety** — hotkey events arrive on a non-GUI goroutine; UI mutations are marshaled with `fyne.Do` onto Fyne's event loop.
10. **Windows cross-compile** — the hotkey package (RegisterHotKey path), indexer, launcher, and UI build for Windows; verified with `GOOS=windows go build ./...` (no runtime test in this change).
11. **OpenSpec workflow** — change artifacts authored first; implementation follows `tasks.md`; `openspec validate` gates the change; `openspec archive` merges the delta into `openspec/specs/launcher/spec.md`.
12. **Linux build prerequisites** — Fyne requires cgo + system dev libraries: `libgl1-mesa-dev libwayland-dev libx11-dev libxrandr-dev libxcursor-dev libxi-dev libxinerama-dev libxxf86vm-dev` (apt). Documented in README.

## Risks / Trade-offs

- **WSLg hotkey scope (accepted):** XGrabKey only fires while a Linux-side app has focus. Documented in spec (`Global hotkey toggle`) and this design; mitigation is a Windows-native build, which the same code supports.
- **No always-on-top/blur (deferred):** Fyne has no portable API; the overlay may sit below some windows on some WMs. Deferred to a later change (per-platform X11 `_NET_WM_STATE_ABOVE` hint or portal), not silently half-implemented.
- **Window decorations:** frameless via `_MOTIF_WM_HINTS` on X11 only; Windows/Wayland show a title bar in v1.
- **Click-outside-close:** implemented via X11 `FocusOut` watching; not on Windows v1.
- **Wayland (accepted for now):** launcher works on X11/WSLg; Wayland requires the GlobalShortcuts portal + layer-shell, isolated behind the hotkey interface.
- **`.desktop` Exec parsing:** field codes (`%f`, `%U`, …) are stripped; quoting edge cases are handled minimally and covered by tests.
- **Index freshness:** v1 indexes once at startup; no file watching. Recents/watching are future changes.
- **Build prerequisites:** Fyne on Linux needs cgo + apt dev libraries (`libgl1-mesa-dev`, `libwayland-dev`, `libx11-dev`, …); documented in README. The search/indexer/hotkey/launcher packages are cgo-free.