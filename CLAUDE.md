# reeln-core

Rust workspace with PyO3 Python bindings (`reeln-native`).

## Build & Test

```bash
# Build (exclude reeln-python locally — needs maturin/venv)
cargo build --workspace --exclude reeln-python

# Test
cargo test --workspace --exclude reeln-python
```

## CI Checks (must pass before committing)

These run in CI with `-D warnings` — failures block merge:

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
```

**Always run `cargo fmt --all` and fix clippy warnings before committing.**

## Crate Layout

- `reeln-media` — native libav* media operations (probe, concat, render, xfade, composite)
- `reeln-overlay` — PNG overlay generation (text, shapes, gradients)
- `reeln-config` — config loading and validation
- `reeln-sport` — sport-specific data models
- `reeln-state` — game state machine, JSON persistence, directory management, **all game state mutations**
- `reeln-plugin` — dynamic plugin loading
- `reeln-ffi` — C FFI bindings
- `reeln-python` — PyO3 Python bindings (builds via maturin in CI)

## State Mutation Ownership (CRITICAL)

**`reeln-state` is the authoritative owner of ALL game state mutations.** No consumer
(dock, CLI, plugin, or future integration) may directly modify `GameState` fields and
call `save_game_state()`. Instead, consumers call dedicated mutation functions that
`reeln-state` provides.

### The Principle

> Anything that manages what "reeln" IS lives in the central libraries (reeln-core).
> Consumers are thin wrappers: UX helpers (dock), CLI adapters (reeln-cli), or capability
> providers (plugins).

### What Belongs in reeln-state

Every operation that changes `game.json` must be a function in `reeln-state`:

| Category | Functions |
|---|---|
| **Event mutations** | `add_event()`, `remove_event()`, `update_event_field()`, `tag_event()`, `bulk_update_event_type()` |
| **Render tracking** | `add_render()`, `clear_renders()` |
| **Segment tracking** | `mark_segment_processed()`, `set_segment_output()` |
| **Highlights** | `mark_highlighted()` |
| **Game lifecycle** | `mark_finished()`, `set_tournament()`, `update_game_info_field()` |
| **Persistence** | `load_game_state()`, `save_game_state()` (existing) |
| **Directory ops** | `create_game_directory()`, `detect_next_game_number()`, etc. (existing) |

Each mutation function takes `&mut GameState` — a pure in-memory transform:

```rust
pub fn mark_finished(state: &mut GameState) {
    state.finished = true;
    state.finished_at = chrono::Utc::now().to_rfc3339();
}
```

The caller is responsible for `load_game_state()` and `save_game_state()` around the
mutation (to control transaction boundaries), but the mutation logic itself lives in
`reeln-state`.

### What Does NOT Belong in reeln-state

- Media operations (concat, render, probe) — `reeln-media`
- Overlay generation — `reeln-overlay`
- Sport definitions — `reeln-sport`
- Config parsing — `reeln-config`
- Plugin loading — `reeln-plugin`
- UI layout, window management, IPC routing — dock
- CLI arg parsing, output formatting — CLI

### PyO3 Bindings (reeln-python)

Every mutation function added to `reeln-state` must also be exposed in `reeln-python`
so the CLI can call it via `reeln_native`. The binding follows the same pattern as
existing state functions in `reeln-python/src/lib.rs`.

### Crate Dependency Rules

- `reeln-state` depends only on `reeln-sport` (for sport types) + serialization crates.
  It must NEVER depend on `reeln-media`, `reeln-overlay`, `reeln-config`, or `reeln-plugin`.
- `reeln-state` is a pure data + mutation layer. It does not perform I/O beyond file
  read/write of `game.json`.
