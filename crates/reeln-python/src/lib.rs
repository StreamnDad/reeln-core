use std::collections::HashMap;
use std::path::PathBuf;

use pyo3::exceptions::{PyOSError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;

// ── Error conversion ────────────────────────────────────────────────

fn media_err(e: reeln_media::MediaError) -> PyErr {
    PyRuntimeError::new_err(e.to_string())
}

fn sport_err(e: reeln_sport::SportError) -> PyErr {
    PyValueError::new_err(e.to_string())
}

fn state_err(e: reeln_state::StateError) -> PyErr {
    PyOSError::new_err(e.to_string())
}

fn config_err(e: reeln_config::ConfigError) -> PyErr {
    PyOSError::new_err(e.to_string())
}

fn overlay_err(e: reeln_overlay::OverlayError) -> PyErr {
    PyRuntimeError::new_err(e.to_string())
}

fn plugin_err(e: reeln_plugin::LoadError) -> PyErr {
    PyRuntimeError::new_err(e.to_string())
}

// ── Media functions ─────────────────────────────────────────────────

/// Probe a media file, returning a dict with duration, fps, width, height, codec.
#[pyfunction]
fn probe(path: &str) -> PyResult<PyObject> {
    let info = reeln_media::probe::probe(std::path::Path::new(path)).map_err(media_err)?;
    Python::with_gil(|py| {
        let dict = PyDict::new(py);
        dict.set_item("duration_secs", info.duration_secs)?;
        dict.set_item("fps", info.fps)?;
        dict.set_item("width", info.width)?;
        dict.set_item("height", info.height)?;
        dict.set_item("codec", info.codec)?;
        Ok(dict.into())
    })
}

/// Concatenate media segments into a single output file.
///
/// Args:
///     segments: list of input file paths
///     output: output file path
///     copy: if True, stream-copy without re-encoding
///     video_codec: video codec (default "libx264")
///     crf: constant rate factor (default 23)
///     audio_codec: audio codec (default "aac")
///     audio_rate: audio sample rate (default 48000)
#[pyfunction]
#[pyo3(signature = (segments, output, copy=true, video_codec="libx264", crf=23, audio_codec="aac", audio_rate=48000))]
fn concat(
    segments: Vec<String>,
    output: &str,
    copy: bool,
    video_codec: &str,
    crf: u32,
    audio_codec: &str,
    audio_rate: u32,
) -> PyResult<()> {
    let paths: Vec<PathBuf> = segments.iter().map(PathBuf::from).collect();
    let refs: Vec<&std::path::Path> = paths.iter().map(|p| p.as_path()).collect();
    let opts = reeln_media::ConcatOptions {
        copy,
        video_codec: video_codec.to_string(),
        crf,
        audio_codec: audio_codec.to_string(),
        audio_rate,
    };
    reeln_media::concat::concat_native(&refs, std::path::Path::new(output), &opts)
        .map_err(media_err)
}

/// Render a video with optional filters.
///
/// Returns a dict with output path and duration_secs.
#[pyfunction]
#[pyo3(signature = (input, output, video_codec="libx264", crf=23, audio_codec="aac", filters=vec![]))]
fn render(
    input: &str,
    output: &str,
    video_codec: &str,
    crf: u32,
    audio_codec: &str,
    filters: Vec<String>,
) -> PyResult<PyObject> {
    let plan = reeln_media::RenderPlan {
        input: PathBuf::from(input),
        output: PathBuf::from(output),
        video_codec: video_codec.to_string(),
        crf,
        preset: None,
        audio_codec: audio_codec.to_string(),
        audio_bitrate: None,
        filters,
        filter_complex: None,
        audio_filter: None,
    };
    let result = reeln_media::render::render_native(&plan).map_err(media_err)?;
    Python::with_gil(|py| {
        let dict = PyDict::new(py);
        dict.set_item("output", result.output.to_string_lossy().to_string())?;
        dict.set_item("duration_secs", result.duration_secs)?;
        Ok(dict.into())
    })
}

/// Render a video with a full filter_complex string and optional audio filter.
///
/// This executes the filter graph natively via libavfilter's graph.parse(),
/// which accepts the exact same syntax as the ffmpeg CLI -filter_complex flag.
///
/// Returns a dict with output path and duration_secs.
#[pyfunction]
#[pyo3(signature = (input, output, filter_complex=None, audio_filter=None, video_codec="libx264", crf=23, preset=None, audio_codec="aac", audio_bitrate=None))]
#[allow(clippy::too_many_arguments)]
fn render_with_filters(
    input: &str,
    output: &str,
    filter_complex: Option<String>,
    audio_filter: Option<String>,
    video_codec: &str,
    crf: u32,
    preset: Option<String>,
    audio_codec: &str,
    audio_bitrate: Option<u32>,
) -> PyResult<PyObject> {
    let plan = reeln_media::RenderPlan {
        input: PathBuf::from(input),
        output: PathBuf::from(output),
        video_codec: video_codec.to_string(),
        crf,
        preset,
        audio_codec: audio_codec.to_string(),
        audio_bitrate,
        filters: vec![],
        filter_complex,
        audio_filter,
    };
    let result = reeln_media::render::render_native(&plan).map_err(media_err)?;
    Python::with_gil(|py| {
        let dict = PyDict::new(py);
        dict.set_item("output", result.output.to_string_lossy().to_string())?;
        dict.set_item("duration_secs", result.duration_secs)?;
        Ok(dict.into())
    })
}

/// Extract a single frame from a video at the given timestamp and write it as PNG.
///
/// This uses native seek + decode + swscale + PNG write — no ffmpeg subprocess.
#[pyfunction]
fn extract_frame(input: &str, timestamp: f64, output: &str) -> PyResult<()> {
    reeln_media::extract::extract_frame(
        std::path::Path::new(input),
        timestamp,
        std::path::Path::new(output),
    )
    .map_err(media_err)
}

/// Composite a PNG overlay onto a video file.
///
/// Returns a dict with output path and duration_secs.
#[pyfunction]
#[pyo3(signature = (video, overlay_png, output, x=0, y=0, start_time=None, end_time=None, video_codec="libx264", crf=23, audio_codec="aac"))]
#[allow(clippy::too_many_arguments)]
fn composite_overlay(
    video: &str,
    overlay_png: &str,
    output: &str,
    x: u32,
    y: u32,
    start_time: Option<f64>,
    end_time: Option<f64>,
    video_codec: &str,
    crf: u32,
    audio_codec: &str,
) -> PyResult<PyObject> {
    let opts = reeln_media::composite::CompositeOptions {
        x,
        y,
        start_time,
        end_time,
        video_codec: video_codec.to_string(),
        crf,
        audio_codec: audio_codec.to_string(),
    };
    // Try native composite first, fall back to subprocess.
    let result = reeln_media::composite::composite_overlay_native(
        std::path::Path::new(video),
        std::path::Path::new(overlay_png),
        std::path::Path::new(output),
        &opts,
    )
    .or_else(|_| {
        reeln_media::composite::composite_overlay(
            std::path::Path::new(video),
            std::path::Path::new(overlay_png),
            std::path::Path::new(output),
            &opts,
        )
    })
    .map_err(media_err)?;
    Python::with_gil(|py| {
        let dict = PyDict::new(py);
        dict.set_item("output", result.output.to_string_lossy().to_string())?;
        dict.set_item("duration_secs", result.duration_secs)?;
        Ok(dict.into())
    })
}

/// Concatenate video files with xfade/acrossfade transitions.
///
/// Tries native execution first, falls back to subprocess.
///
/// Args:
///     files: list of input file paths (minimum 2)
///     durations: list of durations in seconds for each file
///     output: output file path
///     fade_duration: cross-fade duration in seconds (default 0.5)
///     video_codec: video codec (default "libx264")
///     crf: constant rate factor (default 23)
///     audio_codec: audio codec (default "aac")
///     audio_rate: audio sample rate (default 48000)
#[pyfunction]
#[pyo3(signature = (files, durations, output, fade_duration=0.5, video_codec="libx264", crf=23, audio_codec="aac", audio_rate=48000))]
#[allow(clippy::too_many_arguments)]
fn xfade_concat(
    files: Vec<String>,
    durations: Vec<f64>,
    output: &str,
    fade_duration: f64,
    video_codec: &str,
    crf: u32,
    audio_codec: &str,
    audio_rate: u32,
) -> PyResult<()> {
    let paths: Vec<PathBuf> = files.iter().map(PathBuf::from).collect();
    let refs: Vec<&std::path::Path> = paths.iter().map(|p| p.as_path()).collect();
    let opts = reeln_media::xfade::XfadeOptions {
        fade_duration,
        video_codec: video_codec.to_string(),
        crf,
        audio_codec: audio_codec.to_string(),
        audio_rate,
    };
    reeln_media::xfade::xfade_concat_native(&refs, &durations, std::path::Path::new(output), &opts)
        .or_else(|_| {
            reeln_media::xfade::xfade_concat_subprocess(
                &refs,
                &durations,
                std::path::Path::new(output),
                &opts,
            )
        })
        .map(|_| ())
        .map_err(media_err)
}

// ── Sport functions ─────────────────────────────────────────────────

/// List all supported sport aliases.
#[pyfunction]
fn list_sports() -> Vec<String> {
    let registry = reeln_sport::SportRegistry::new();
    registry
        .list_sports()
        .into_iter()
        .map(|a| a.sport.clone())
        .collect()
}

/// Generate a segment directory name.
#[pyfunction]
fn segment_dir_name(sport: &str, segment_number: u32) -> PyResult<String> {
    let registry = reeln_sport::SportRegistry::new();
    let info = registry.get_sport(sport).map_err(sport_err)?;
    Ok(reeln_sport::segment_dir_name(info, segment_number))
}

/// Generate a segment display name.
#[pyfunction]
fn segment_display_name(sport: &str, segment_number: u32) -> PyResult<String> {
    let registry = reeln_sport::SportRegistry::new();
    let info = registry.get_sport(sport).map_err(sport_err)?;
    Ok(reeln_sport::segment_display_name(info, segment_number))
}

// ── State functions ─────────────────────────────────────────────────

/// Generate a game directory name.
#[pyfunction]
fn game_dir_name(date: &str, home: &str, away: &str, game_number: u32) -> String {
    reeln_state::game_dir_name(date, home, away, game_number)
}

/// Detect the next game number for a double-header.
#[pyfunction]
fn detect_next_game_number(base_dir: &str, date: &str, home: &str, away: &str) -> u32 {
    reeln_state::detect_next_game_number(std::path::Path::new(base_dir), date, home, away)
}

/// Find unfinished games under a base directory.
/// Returns a list of directory paths.
#[pyfunction]
fn find_unfinished_games(base_dir: &str) -> PyResult<Vec<String>> {
    let games =
        reeln_state::find_unfinished_games(std::path::Path::new(base_dir)).map_err(state_err)?;
    Ok(games
        .into_iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect())
}

/// Load game state from a game directory.
/// Returns the JSON string of the game state.
#[pyfunction]
fn load_game_state(game_dir: &str) -> PyResult<String> {
    let state = reeln_state::load_game_state(std::path::Path::new(game_dir)).map_err(state_err)?;
    serde_json::to_string_pretty(&state)
        .map_err(|e| PyRuntimeError::new_err(format!("failed to serialize: {e}")))
}

/// Save game state to a game directory.
/// Accepts the game state as a JSON string.
#[pyfunction]
fn save_game_state(game_dir: &str, json_str: &str) -> PyResult<String> {
    let state: reeln_state::GameState = serde_json::from_str(json_str)
        .map_err(|e| PyValueError::new_err(format!("invalid JSON: {e}")))?;
    let path =
        reeln_state::save_game_state(&state, std::path::Path::new(game_dir)).map_err(state_err)?;
    Ok(path.to_string_lossy().to_string())
}

// ── Config functions ────────────────────────────────────────────────

/// Get the default config directory path.
#[pyfunction]
fn config_dir() -> String {
    reeln_config::config_dir().to_string_lossy().to_string()
}

/// Get the default data directory path.
#[pyfunction]
fn data_dir() -> String {
    reeln_config::data_dir().to_string_lossy().to_string()
}

/// Load configuration, returning it as a JSON string.
///
/// Args:
///     path: optional explicit config file path (uses default if None)
///     profile: optional profile name for overlay
#[pyfunction]
#[pyo3(signature = (path=None, profile=None))]
fn load_config(path: Option<&str>, profile: Option<&str>) -> PyResult<String> {
    let config_path = match path {
        Some(p) => PathBuf::from(p),
        None => reeln_config::resolve_config_path(None, profile),
    };

    let mut config = reeln_config::load_config(&config_path, profile).map_err(config_err)?;
    reeln_config::apply_env_overrides(&mut config);

    serde_json::to_string_pretty(&config)
        .map_err(|e| PyRuntimeError::new_err(format!("failed to serialize: {e}")))
}

/// Validate a configuration JSON string.
/// Returns a list of warning messages (empty means valid).
#[pyfunction]
fn validate_config(json_str: &str) -> PyResult<Vec<String>> {
    let value: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| PyValueError::new_err(format!("invalid JSON: {e}")))?;
    Ok(reeln_config::validate_config(&value))
}

// ── Overlay functions ──────────────────────────────────────────────

/// Load an overlay template from a JSON file.
/// Returns the template as a JSON string.
#[pyfunction]
fn load_template(path: &str) -> PyResult<String> {
    let template =
        reeln_overlay::template::load_template(std::path::Path::new(path)).map_err(overlay_err)?;
    serde_json::to_string_pretty(&template)
        .map_err(|e| PyRuntimeError::new_err(format!("failed to serialize template: {e}")))
}

/// Render an overlay template to a PNG file.
///
/// Args:
///     template_json: the template as a JSON string
///     context: dict of variable substitutions
///     output: output PNG file path
#[pyfunction]
fn render_overlay(
    template_json: &str,
    context: HashMap<String, String>,
    output: &str,
) -> PyResult<()> {
    let template: reeln_overlay::template::Template = serde_json::from_str(template_json)
        .map_err(|e| PyValueError::new_err(format!("invalid template JSON: {e}")))?;
    reeln_overlay::render::render_template_to_png(&template, &context, std::path::Path::new(output))
        .map_err(overlay_err)
}

/// Substitute {{variables}} in a string using a context dict.
#[pyfunction]
fn substitute_variables(text: &str, context: HashMap<String, String>) -> String {
    reeln_overlay::template::substitute_variables(text, &context)
}

/// Evaluate a visibility condition against a context dict.
#[pyfunction]
fn evaluate_visibility(condition: &str, context: HashMap<String, String>) -> bool {
    reeln_overlay::template::evaluate_visibility(condition, &context)
}

// ── Plugin functions ──────────────────────────────────────────────

/// List all hook names.
#[pyfunction]
fn list_hooks() -> Vec<String> {
    reeln_plugin::Hook::all()
        .iter()
        .map(|h| h.as_str().to_string())
        .collect()
}

/// Load a native plugin from a shared library path.
/// Returns a JSON string with the plugin info.
#[pyfunction]
fn load_native_plugin(path: &str) -> PyResult<String> {
    let plugin = reeln_plugin::load_plugin(std::path::Path::new(path)).map_err(plugin_err)?;
    let info = plugin.info();
    serde_json::to_string_pretty(&info)
        .map_err(|e| PyRuntimeError::new_err(format!("failed to serialize plugin info: {e}")))
}

/// Discover native plugins in a directory.
/// Returns a JSON array of plugin info objects, plus a list of error messages.
#[pyfunction]
fn discover_plugins(dir: &str) -> PyResult<PyObject> {
    let (plugins, errors) = reeln_plugin::discover_plugins(std::path::Path::new(dir));
    let infos: Vec<reeln_plugin::PluginInfo> = plugins.iter().map(|p| p.info()).collect();
    let error_msgs: Vec<String> = errors
        .iter()
        .map(|(path, e)| format!("{}: {e}", path.display()))
        .collect();

    Python::with_gil(|py| {
        let dict = PyDict::new(py);
        let plugins_json = serde_json::to_string_pretty(&infos)
            .map_err(|e| PyRuntimeError::new_err(format!("serialize error: {e}")))?;
        dict.set_item("plugins", plugins_json)?;
        dict.set_item("errors", error_msgs)?;
        Ok(dict.into())
    })
}

// ── Module definition ───────────────────────────────────────────────

/// Python bindings for reeln-core.
///
/// Exposes the `reeln_native` module to Python via PyO3.
#[pymodule]
fn reeln_native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Suppress ffmpeg's verbose warnings (e.g. UDTA parsing spam).
    ffmpeg_next::log::set_level(ffmpeg_next::log::Level::Error);

    m.add("__version__", env!("CARGO_PKG_VERSION"))?;

    // Media
    m.add_function(wrap_pyfunction!(probe, m)?)?;
    m.add_function(wrap_pyfunction!(concat, m)?)?;
    m.add_function(wrap_pyfunction!(render, m)?)?;
    m.add_function(wrap_pyfunction!(render_with_filters, m)?)?;
    m.add_function(wrap_pyfunction!(composite_overlay, m)?)?;
    m.add_function(wrap_pyfunction!(extract_frame, m)?)?;
    m.add_function(wrap_pyfunction!(xfade_concat, m)?)?;

    // Sport
    m.add_function(wrap_pyfunction!(list_sports, m)?)?;
    m.add_function(wrap_pyfunction!(segment_dir_name, m)?)?;
    m.add_function(wrap_pyfunction!(segment_display_name, m)?)?;

    // State
    m.add_function(wrap_pyfunction!(game_dir_name, m)?)?;
    m.add_function(wrap_pyfunction!(detect_next_game_number, m)?)?;
    m.add_function(wrap_pyfunction!(find_unfinished_games, m)?)?;
    m.add_function(wrap_pyfunction!(load_game_state, m)?)?;
    m.add_function(wrap_pyfunction!(save_game_state, m)?)?;

    // Config
    m.add_function(wrap_pyfunction!(config_dir, m)?)?;
    m.add_function(wrap_pyfunction!(data_dir, m)?)?;
    m.add_function(wrap_pyfunction!(load_config, m)?)?;
    m.add_function(wrap_pyfunction!(validate_config, m)?)?;

    // Overlay
    m.add_function(wrap_pyfunction!(load_template, m)?)?;
    m.add_function(wrap_pyfunction!(render_overlay, m)?)?;
    m.add_function(wrap_pyfunction!(substitute_variables, m)?)?;
    m.add_function(wrap_pyfunction!(evaluate_visibility, m)?)?;

    // Plugin
    m.add_function(wrap_pyfunction!(list_hooks, m)?)?;
    m.add_function(wrap_pyfunction!(load_native_plugin, m)?)?;
    m.add_function(wrap_pyfunction!(discover_plugins, m)?)?;

    Ok(())
}
