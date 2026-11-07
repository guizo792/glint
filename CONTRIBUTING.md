# Contributing to Glint

Bug reports and pull requests are welcome. For anything large, open an issue first so we can agree on the approach.

## Layout

| Path | What it is |
|---|---|
| `crates/glint-core` | OS-independent: index, search, ranking, history, config, calculator |
| `crates/glint-win` | Everything that calls Windows directly |
| `crates/glint-ext` | Extension host |
| `app` | Tauri app wiring it together, with the Svelte UI in `app/ui` |
| `spikes/bench` | Search benchmark |
| `extensions` | SDK templates and reference extensions |
| `scripts` | End-to-end test, memory measurement, icon generator |

`glint-core` must not depend on Tauri or the UI.

## Before opening a pull request

```bash
npm run build --prefix app/ui
```

```bash
cargo fmt --all --check
```

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

```bash
cargo test --workspace
```

```bash
npm test --prefix app/ui
```

If you touched search, also run the speed gate. It models a mid-range machine and must stay under 33 ms:

```bash
RAYON_NUM_THREADS=4 cargo run --release -p bench -- --gate 33
```

CI runs a version that does not depend on the machine, because shared runners are slower than a developer machine: across queries of 3 or more characters, the fast path must be at least 3 times faster than the full scan, measured in the same run.

```bash
cargo run --release -p bench -- --min-speedup 3
```

If you touched the window or hotkey, run `scripts/e2e.ps1` on an unlocked desktop. It takes over the keyboard briefly.

## Principles

- Latency and memory are features. The hotkey should show the window in under 50 ms (p95), a search should finish in under 33 ms (p95) on 4 threads, and the whole process tree should stay under 180 MB of private memory.
- Never start programs through `cmd`, so that characters in file names stay characters.
- No network use in the core. Anything that needs the network must be opt-in.
- Change only what your task needs, and add a test that fails without your change.

By contributing you agree your work is licensed under both MIT and Apache-2.0.
