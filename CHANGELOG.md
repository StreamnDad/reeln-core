# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-04-02

### Removed

- **`SubprocessBackend`** — the backend that shelled out to the `ffmpeg` binary for
  concat and render operations. All media operations now use native libav* bindings
  exclusively via `LibavBackend`.
- **`concat_subprocess()`**, **`render_subprocess()`**, **`xfade_concat_subprocess()`** —
  subprocess fallback functions removed from `reeln-media`.
- **`composite_overlay()` subprocess variant** — the native implementation has been
  renamed from `composite_overlay_native()` to `composite_overlay()`.
- **`ffmpeg_path`** field from `VideoConfig` and the `REELN_VIDEO_FFMPEG_PATH`
  environment variable override. The `ffmpeg` binary is no longer used at runtime.
- **`tempfile`** removed from `reeln-media` production dependencies (kept as dev-dependency
  for test fixtures).

### Changed

- `composite_overlay_native()` renamed to `composite_overlay()`.
- Python bindings (`reeln_native`) no longer fall back to subprocess for
  `composite_overlay` and `xfade_concat`.
- The `ffmpeg` command-line binary is no longer required at runtime. Only libav*
  development libraries are needed for building.

## [0.1.0] - 2026-03-15

### Added

- Initial release with `reeln-media`, `reeln-overlay`, `reeln-sport`, `reeln-state`,
  `reeln-config`, `reeln-plugin`, `reeln-python`, and `reeln-ffi` crates.
