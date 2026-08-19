## 1. Scaffolding

- [x] 1.1 Initialize Go module `spotlight`, git repo, `.gitignore`
- [x] 1.2 Create package skeletons: `cmd/spotlight`, `internal/hotkey`, `internal/indexer`, `internal/search`, `internal/launcher`, `internal/ui`
- [x] 1.3 Verify: `go build ./...` succeeds

## 2. Search / ranking engine

- [x] 2.1 Define `search.Entry` (Name, Path, Kind, Icon) and result model
- [x] 2.2 Implement case-insensitive tiered ranking: exact > prefix > substring > fuzzy, with deterministic tie-breaks
- [x] 2.3 Cap results at 20; support empty-query quick entries
- [x] 2.4 Verify: unit tests cover each tier, ties, cap, and empty query; `go test ./internal/search/...`

## 3. Indexer

- [x] 3.1 Implement Linux app discovery: parse `.desktop` files (Name, Exec, Icon) from XDG application dirs, skip hidden entries (`NoDisplay`/`Hidden`)
- [x] 3.2 Implement user-file walker: roots `$HOME`, Desktop, Documents, Downloads; skip hidden dirs and noise dirs (`.git`, `node_modules`, …); depth and entry caps
- [x] 3.3 Background population with `sync.RWMutex`-guarded store and non-fatal per-directory errors
- [x] 3.4 Verify: unit tests with fixture dirs and sample `.desktop` files; `go test ./internal/indexer/...`

## 4. Hotkey

- [x] 4.1 Define `hotkey.Manager` interface (Register/Toggle events, Close)
- [x] 4.2 Implement Linux (X11) manager with custom XGrabKey via `jezek/xgb` (pure Go; `golang.design/x/hotkey`'s Linux path requires cgo+X11 headers, so it is used only on Windows)
- [x] 4.3 Implement Windows manager behind the same interface (`GOOS=windows` build tag)
- [x] 4.4 Verify: `go build ./...` on Linux; `GOOS=windows go build` compiles

## 5. Overlay UI

- [x] 5.1 Fyne window: fixed size, centered, search entry + results list (frameless via `_MOTIF_WM_HINTS` on X11)
- [x] 5.2 Live search on each keystroke wired to the search engine; empty-query quick entries
- [x] 5.3 Keyboard handling: `↑`/`↓` select, `Enter` open, `Esc` close
- [x] 5.4 Show/Hide/Reset APIs for the main wiring; matched-substring highlight in rows
- [x] 5.5 Verify: `go build ./...`; automated smoke test via XTEST (show, frameless, Esc-hide) passed

## 6. Launch actions

- [x] 6.1 Implement `launcher.Open(entry)`: apps via exec of `.desktop` Exec (field codes stripped), files via `xdg-open` (Linux) / shell (Windows)
- [x] 6.2 Verify: unit tests for Exec parsing and command construction; `go test ./internal/launcher/...`

## 7. Main wiring

- [x] 7.1 `cmd/spotlight`: start indexer goroutine, register hotkey, show/hide overlay on toggle, open selected result and hide on Enter
- [x] 7.2 Hotkey events marshaled onto the UI thread (`fyne.Do`); graceful degradation if hotkey registration fails
- [x] 7.3 Verify: `go build`, `go vet`, `go test ./...`; automated smoke test passed; manual hotkey check pending user

## 8. Verification and archive

- [x] 8.1 `openspec validate spotlight-launcher` passes
- [x] 8.2 Cross-compile check: `GOOS=windows go build` of platform packages
- [x] 8.3 Archive the change: `openspec archive spotlight-launcher`; confirm delta merged into `openspec/specs/launcher/spec.md`