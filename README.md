# reeln-core

Shared Rust core library for [reeln](https://github.com/StreamnDad/reeln-cli) — the sports highlight rendering CLI. Provides native media processing, overlay rendering, game state management, and sport-specific logic consumed by the Python CLI, OBS plugin, and future Tauri desktop app.

**License:** AGPL-3.0-only | **Homepage:** https://streamn.dad | **Org:** [StreamnDad](https://github.com/StreamnDad)

## Architecture

```
reeln-core (Rust workspace)
├── reeln-media      Media processing via libav* (probe, render, concat, xfade, composite, extract)
├── reeln-overlay    2D overlay template engine (tiny-skia + cosmic-text → PNG)
├── reeln-sport      Sport registry & segment naming
├── reeln-state      Game state machine, JSON persistence, directory management
├── reeln-config     XDG configuration, env overrides, validation
├── reeln-plugin     Hook system, capabilities, dynamic plugin loading
├── reeln-python     PyO3 bindings → published as `reeln-native` on PyPI
└── reeln-ffi        C ABI (cdylib + staticlib) for OBS plugin integration
```

## Crate Details

| Crate | Tests | Key Dependencies | Purpose |
|---|---|---|---|
| `reeln-media` | 150 | `ffmpeg-next` 8 | Probe, render, concat, xfade, composite, frame extract |
| `reeln-overlay` | 156 | `tiny-skia`, `cosmic-text`, `image` | Template-to-PNG rasterization |
| `reeln-sport` | 38 | `serde` | Sport aliases, segment naming |
| `reeln-state` | 35 | `serde_json`, `chrono`, `uuid`, `glob` | Game state CRUD, directory ops |
| `reeln-config` | 61 | `dirs`, `serde_json` | Config loading, merge, env overrides |
| `reeln-plugin` | 107 | `libloading` | Hook emission, plugin discovery |
| `reeln-ffi` | 37 | All `reeln-*` crates | C ABI exports |
| `reeln-python` | — | `pyo3` 0.24 | Python extension module |
| **Total** | **584** | | |

## Prerequisites

**Rust toolchain:**

```bash
rustup update stable
```

**libav\* development libraries** (the `ffmpeg` command-line binary is not required at runtime):

```bash
# macOS
brew install ffmpeg pkg-config

# Ubuntu/Debian
sudo apt-get install -y libavcodec-dev libavformat-dev libavfilter-dev \
  libavutil-dev libswscale-dev libswresample-dev pkg-config

# Fedora
sudo dnf install ffmpeg-devel pkg-config
```

## Building

```bash
# Build all crates
cargo build --release

# Run all tests (584 tests)
cargo test --all

# Check without building (faster)
cargo check --all

# Lint
cargo clippy --all-targets -- -D warnings

# Format
cargo fmt --all
```

## Python Bindings

The `reeln-python` crate builds a native Python extension module published as [`reeln-native`](https://pypi.org/project/reeln-native/) on PyPI.

```bash
# Development build (installs into active venv)
cd crates/reeln-python
pip install maturin
maturin develop --release

# Build wheel for distribution
maturin build --release
```

**From reeln-cli:**

```bash
# Install with native backend
pip install "reeln[native]"

# Or development install from source
cd /path/to/reeln-cli
make dev-install-native
```

<!-- AUTO-GENERATED:PYTHON-API:START -->
### Python API (`reeln_native`)

| Function | Module | Description |
|---|---|---|
| `probe(path)` | Media | Probe media file → dict with duration, fps, width, height, codec |
| `render(input, output, ...)` | Media | Render video with optional filters |
| `render_with_filters(input, output, filter_complex, ...)` | Media | Render with full filter_complex string |
| `concat(segments, output, ...)` | Media | Concatenate media segments |
| `xfade_concat(files, durations, output, ...)` | Media | Cross-fade concatenation (xfade + acrossfade) |
| `composite_overlay(video, overlay_png, output, ...)` | Media | Composite PNG overlay onto video |
| `extract_frame(input, timestamp, output)` | Media | Extract single frame as PNG |
| `list_sports()` | Sport | List supported sport aliases |
| `segment_dir_name(sport, number)` | Sport | Generate segment directory name |
| `segment_display_name(sport, number)` | Sport | Generate segment display name |
| `game_dir_name(date, home, away, number)` | State | Generate game directory name |
| `detect_next_game_number(base, date, home, away)` | State | Detect next double-header number |
| `find_unfinished_games(base_dir)` | State | Find unfinished game directories |
| `load_game_state(game_dir)` | State | Load game state JSON |
| `save_game_state(game_dir, json)` | State | Save game state JSON |
| `config_dir()` | Config | Default config directory path |
| `data_dir()` | Config | Default data directory path |
| `load_config(path, profile)` | Config | Load configuration as JSON |
| `validate_config(json)` | Config | Validate config, return warnings |
| `load_template(path)` | Overlay | Load overlay template JSON |
| `render_overlay(template, context, output)` | Overlay | Render template to PNG |
| `substitute_variables(text, context)` | Overlay | Substitute `{{variables}}` |
| `evaluate_visibility(condition, context)` | Overlay | Evaluate visibility condition |
| `list_hooks()` | Plugin | List all hook names |
| `load_native_plugin(path)` | Plugin | Load native plugin from .so/.dylib |
| `discover_plugins(dir)` | Plugin | Discover plugins in directory |
<!-- AUTO-GENERATED:PYTHON-API:END -->

## C ABI (`reeln-ffi`)

The `reeln-ffi` crate produces a C-compatible shared library for integration with C/C++ projects (e.g., OBS Studio plugins).

```bash
cargo build -p reeln-ffi --release
# Output: target/release/libreeln_ffi.{so,dylib,dll}
```

## CI/CD

| Workflow | Trigger | What it does |
|---|---|---|
| `ci.yml` | Push/PR to `main` | Format, clippy, test on Linux/macOS/Windows |
| `release.yml` | Tag `v*` | Build wheels for 5 platforms, publish to PyPI (OIDC) |

**Release targets:**

- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin` (Apple Silicon)
- `x86_64-pc-windows-msvc`

## Consumers

| Project | How it uses reeln-core |
|---|---|
| [reeln-cli](https://github.com/StreamnDad/reeln-cli) | `pip install "reeln[native]"` — Python FFI via `reeln_native` |
| reeln plugins | Indirectly via reeln-cli at runtime |
| streamn-scoreboard | Subprocess calls to `reeln` CLI (future: link `reeln-ffi`) |
| reeln-tauri (planned) | Direct Rust dependency |

## Versioning

- Workspace version: **0.2.0** (all crates share this version)
- reeln-cli declares `reeln-native >= 0.1.0, < 1.0` as optional dependency
- Changelog: [CHANGELOG.md](CHANGELOG.md)
- Semantic versioning: MAJOR.MINOR.PATCH
- Releases are tag-triggered (`v*` pattern)
