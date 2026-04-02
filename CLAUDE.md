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
- `reeln-state` — state management
- `reeln-plugin` — dynamic plugin loading
- `reeln-ffi` — C FFI bindings
- `reeln-python` — PyO3 Python bindings (builds via maturin in CI)
