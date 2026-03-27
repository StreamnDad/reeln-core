//! C ABI exports for reeln-core.
//!
//! Used by the OBS plugin and other native consumers.
//! Header generated via cbindgen → include/reeln.h
//!
//! # Safety
//!
//! All functions in this module accept raw C pointers and must be called
//! with valid, null-terminated UTF-8 strings. The caller is responsible
//! for freeing any returned strings via `reeln_free_string`.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::path::Path;
use std::ptr;

// ── String helpers ──────────────────────────────────────────────────

/// Convert a C string to a Rust `&str`, returning `None` for null or invalid UTF-8.
unsafe fn cstr_to_str<'a>(s: *const c_char) -> Option<&'a str> {
    if s.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(s) }.to_str().ok()
}

/// Allocate a C string from a Rust string. Returns null on failure.
fn string_to_cstr(s: &str) -> *mut c_char {
    CString::new(s).map_or(ptr::null_mut(), |cs| cs.into_raw())
}

/// Free a string previously returned by a `reeln_*` function.
///
/// # Safety
///
/// `s` must be a pointer previously returned by a `reeln_*` function,
/// or null (which is a no-op).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn reeln_free_string(s: *mut c_char) {
    if !s.is_null() {
        drop(unsafe { CString::from_raw(s) });
    }
}

// ── Version ─────────────────────────────────────────────────────────

/// Return the library version as a C string. Caller must free with `reeln_free_string`.
#[unsafe(no_mangle)]
pub extern "C" fn reeln_version() -> *mut c_char {
    string_to_cstr(env!("CARGO_PKG_VERSION"))
}

// ── Probe result ────────────────────────────────────────────────────

/// Result of a media probe operation.
#[repr(C)]
pub struct ReelnProbeResult {
    /// Duration in seconds, or -1.0 if unknown.
    pub duration_secs: f64,
    /// Frames per second, or -1.0 if unknown.
    pub fps: f64,
    /// Width in pixels, or 0 if unknown.
    pub width: u32,
    /// Height in pixels, or 0 if unknown.
    pub height: u32,
    /// Codec name (caller must free), or null if unknown.
    pub codec: *mut c_char,
    /// Error message (caller must free), or null on success.
    pub error: *mut c_char,
}

/// Probe a media file for metadata.
///
/// # Safety
///
/// `path` must be a valid, null-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn reeln_probe(path: *const c_char) -> ReelnProbeResult {
    let path_str = match unsafe { cstr_to_str(path) } {
        Some(s) => s,
        None => {
            return ReelnProbeResult {
                duration_secs: -1.0,
                fps: -1.0,
                width: 0,
                height: 0,
                codec: ptr::null_mut(),
                error: string_to_cstr("null or invalid path"),
            };
        }
    };

    match reeln_media::probe::probe(Path::new(path_str)) {
        Ok(info) => ReelnProbeResult {
            duration_secs: info.duration_secs.unwrap_or(-1.0),
            fps: info.fps.unwrap_or(-1.0),
            width: info.width.unwrap_or(0),
            height: info.height.unwrap_or(0),
            codec: info
                .codec
                .as_deref()
                .map_or(ptr::null_mut(), string_to_cstr),
            error: ptr::null_mut(),
        },
        Err(e) => ReelnProbeResult {
            duration_secs: -1.0,
            fps: -1.0,
            width: 0,
            height: 0,
            codec: ptr::null_mut(),
            error: string_to_cstr(&e.to_string()),
        },
    }
}

/// Free a probe result's heap-allocated fields.
///
/// # Safety
///
/// `result` must be a pointer to a valid `ReelnProbeResult` previously
/// returned by `reeln_probe`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn reeln_probe_result_free(result: *mut ReelnProbeResult) {
    if result.is_null() {
        return;
    }
    let r = unsafe { &mut *result };
    if !r.codec.is_null() {
        drop(unsafe { CString::from_raw(r.codec) });
        r.codec = ptr::null_mut();
    }
    if !r.error.is_null() {
        drop(unsafe { CString::from_raw(r.error) });
        r.error = ptr::null_mut();
    }
}

// ── Concat ──────────────────────────────────────────────────────────

/// Concatenate media segments.
///
/// Returns null on success, or an error message string (caller must free).
///
/// # Safety
///
/// `segments` must be an array of `segment_count` valid C strings.
/// `output` must be a valid C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn reeln_concat(
    segments: *const *const c_char,
    segment_count: usize,
    output: *const c_char,
    copy: bool,
) -> *mut c_char {
    let output_str = match unsafe { cstr_to_str(output) } {
        Some(s) => s,
        None => return string_to_cstr("null or invalid output path"),
    };

    if segments.is_null() || segment_count == 0 {
        return string_to_cstr("no segments provided");
    }

    let mut paths = Vec::with_capacity(segment_count);
    for i in 0..segment_count {
        let seg_ptr = unsafe { *segments.add(i) };
        match unsafe { cstr_to_str(seg_ptr) } {
            Some(s) => paths.push(std::path::PathBuf::from(s)),
            None => return string_to_cstr(&format!("invalid segment path at index {i}")),
        }
    }
    let refs: Vec<&Path> = paths.iter().map(|p| p.as_path()).collect();

    let opts = reeln_media::ConcatOptions {
        copy,
        video_codec: "libx264".to_string(),
        crf: 23,
        audio_codec: "aac".to_string(),
        audio_rate: 48000,
    };

    match reeln_media::concat::concat_native(&refs, Path::new(output_str), &opts) {
        Ok(()) => ptr::null_mut(),
        Err(e) => string_to_cstr(&e.to_string()),
    }
}

// ── Composite ────────────────────────────────────────────────────────

/// Composite a PNG overlay onto a video.
///
/// Returns null on success, or an error message string (caller must free).
///
/// # Safety
///
/// All string parameters must be valid, null-terminated UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn reeln_composite(
    video: *const c_char,
    overlay_png: *const c_char,
    output: *const c_char,
    x: u32,
    y: u32,
    start_time: f64,
    end_time: f64,
) -> *mut c_char {
    let video_str = match unsafe { cstr_to_str(video) } {
        Some(s) => s,
        None => return string_to_cstr("null or invalid video path"),
    };
    let overlay_str = match unsafe { cstr_to_str(overlay_png) } {
        Some(s) => s,
        None => return string_to_cstr("null or invalid overlay path"),
    };
    let output_str = match unsafe { cstr_to_str(output) } {
        Some(s) => s,
        None => return string_to_cstr("null or invalid output path"),
    };

    let opts = reeln_media::composite::CompositeOptions {
        x,
        y,
        start_time: if start_time < 0.0 {
            None
        } else {
            Some(start_time)
        },
        end_time: if end_time < 0.0 { None } else { Some(end_time) },
        ..Default::default()
    };

    match reeln_media::composite::composite_overlay(
        Path::new(video_str),
        Path::new(overlay_str),
        Path::new(output_str),
        &opts,
    ) {
        Ok(_) => ptr::null_mut(),
        Err(e) => string_to_cstr(&e.to_string()),
    }
}

// ── Game directory name ─────────────────────────────────────────────

/// Generate a game directory name. Caller must free the result.
///
/// # Safety
///
/// All string parameters must be valid, null-terminated UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn reeln_game_dir_name(
    date: *const c_char,
    home: *const c_char,
    away: *const c_char,
    game_number: u32,
) -> *mut c_char {
    let date = unsafe { cstr_to_str(date) }.unwrap_or("");
    let home = unsafe { cstr_to_str(home) }.unwrap_or("");
    let away = unsafe { cstr_to_str(away) }.unwrap_or("");
    string_to_cstr(&reeln_state::game_dir_name(date, home, away, game_number))
}

// ── Segment names ───────────────────────────────────────────────────

/// Generate a segment directory name. Caller must free the result.
/// Returns null if the sport is unknown.
///
/// # Safety
///
/// `sport` must be a valid, null-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn reeln_segment_dir_name(
    sport: *const c_char,
    segment_number: u32,
) -> *mut c_char {
    let sport_str = match unsafe { cstr_to_str(sport) } {
        Some(s) => s,
        None => return ptr::null_mut(),
    };
    let registry = reeln_sport::SportRegistry::new();
    match registry.get_sport(sport_str) {
        Ok(info) => string_to_cstr(&reeln_sport::segment_dir_name(info, segment_number)),
        Err(_) => ptr::null_mut(),
    }
}

/// Generate a segment display name. Caller must free the result.
/// Returns null if the sport is unknown.
///
/// # Safety
///
/// `sport` must be a valid, null-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn reeln_segment_display_name(
    sport: *const c_char,
    segment_number: u32,
) -> *mut c_char {
    let sport_str = match unsafe { cstr_to_str(sport) } {
        Some(s) => s,
        None => return ptr::null_mut(),
    };
    let registry = reeln_sport::SportRegistry::new();
    match registry.get_sport(sport_str) {
        Ok(info) => string_to_cstr(&reeln_sport::segment_display_name(info, segment_number)),
        Err(_) => ptr::null_mut(),
    }
}

// ── Overlay ──────────────────────────────────────────────────────────

/// Render an overlay template (JSON string) to a PNG file.
///
/// `template_json` is the template as a null-terminated JSON string.
/// `context_json` is a JSON object of variable substitutions (e.g. `{"home": "Eagles"}`).
/// `output` is the output PNG file path.
///
/// Returns null on success, or an error message string (caller must free).
///
/// # Safety
///
/// All string parameters must be valid, null-terminated UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn reeln_render_overlay(
    template_json: *const c_char,
    context_json: *const c_char,
    output: *const c_char,
) -> *mut c_char {
    let template_str = match unsafe { cstr_to_str(template_json) } {
        Some(s) => s,
        None => return string_to_cstr("null or invalid template JSON"),
    };
    let context_str = match unsafe { cstr_to_str(context_json) } {
        Some(s) => s,
        None => return string_to_cstr("null or invalid context JSON"),
    };
    let output_str = match unsafe { cstr_to_str(output) } {
        Some(s) => s,
        None => return string_to_cstr("null or invalid output path"),
    };

    let template: reeln_overlay::template::Template = match serde_json::from_str(template_str) {
        Ok(t) => t,
        Err(e) => return string_to_cstr(&format!("invalid template JSON: {e}")),
    };

    let context: std::collections::HashMap<String, String> = match serde_json::from_str(context_str)
    {
        Ok(c) => c,
        Err(e) => return string_to_cstr(&format!("invalid context JSON: {e}")),
    };

    match reeln_overlay::render::render_template_to_png(&template, &context, Path::new(output_str))
    {
        Ok(()) => ptr::null_mut(),
        Err(e) => string_to_cstr(&e.to_string()),
    }
}

/// Load an overlay template from a JSON file. Returns the template as
/// a JSON string (caller must free), or null on error.
///
/// If an error occurs, the error message is written to `error_out` (if not null).
///
/// # Safety
///
/// `path` must be a valid, null-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn reeln_load_template(path: *const c_char) -> *mut c_char {
    let path_str = match unsafe { cstr_to_str(path) } {
        Some(s) => s,
        None => return ptr::null_mut(),
    };

    match reeln_overlay::template::load_template(Path::new(path_str)) {
        Ok(template) => match serde_json::to_string(&template) {
            Ok(json) => string_to_cstr(&json),
            Err(_) => ptr::null_mut(),
        },
        Err(_) => ptr::null_mut(),
    }
}

// ── Config (FFI) ────────────────────────────────────────────────────

/// Load configuration from the default or specified path.
/// Returns the config as a JSON string (caller must free), or null on error.
///
/// # Safety
///
/// `path` may be null (uses default). If non-null, must be valid UTF-8.
/// `profile` may be null (no profile overlay).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn reeln_load_config(
    path: *const c_char,
    profile: *const c_char,
) -> *mut c_char {
    let path_opt = unsafe { cstr_to_str(path) };
    let profile_opt = unsafe { cstr_to_str(profile) };

    let config_path = match path_opt {
        Some(p) => std::path::PathBuf::from(p),
        None => reeln_config::resolve_config_path(None, profile_opt),
    };

    match reeln_config::load_config(&config_path, profile_opt) {
        Ok(mut config) => {
            reeln_config::apply_env_overrides(&mut config);
            match serde_json::to_string(&config) {
                Ok(json) => string_to_cstr(&json),
                Err(_) => ptr::null_mut(),
            }
        }
        Err(_) => ptr::null_mut(),
    }
}

/// Get the default config directory path. Caller must free.
#[unsafe(no_mangle)]
pub extern "C" fn reeln_config_dir() -> *mut c_char {
    string_to_cstr(&reeln_config::config_dir().to_string_lossy())
}

/// Get the default data directory path. Caller must free.
#[unsafe(no_mangle)]
pub extern "C" fn reeln_data_dir() -> *mut c_char {
    string_to_cstr(&reeln_config::data_dir().to_string_lossy())
}

// ── Plugin (FFI) ────────────────────────────────────────────────────

/// List all hook names as a comma-separated string. Caller must free.
#[unsafe(no_mangle)]
pub extern "C" fn reeln_list_hooks() -> *mut c_char {
    let hooks: Vec<&str> = reeln_plugin::Hook::all()
        .iter()
        .map(|h| h.as_str())
        .collect();
    string_to_cstr(&hooks.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn test_version() {
        let v = reeln_version();
        assert!(!v.is_null());
        let s = unsafe { CStr::from_ptr(v) }.to_str().unwrap();
        assert!(s.contains('.'));
        unsafe { reeln_free_string(v) };
    }

    #[test]
    fn test_free_null_string() {
        // Should not panic.
        unsafe { reeln_free_string(ptr::null_mut()) };
    }

    #[test]
    fn test_probe_null_path() {
        let result = unsafe { reeln_probe(ptr::null()) };
        assert!(!result.error.is_null());
        let err = unsafe { CStr::from_ptr(result.error) }.to_str().unwrap();
        assert!(err.contains("null"));
        unsafe { reeln_free_string(result.error) };
    }

    #[test]
    fn test_probe_nonexistent() {
        let path = CString::new("/tmp/nonexistent_ffi_test.mp4").unwrap();
        let result = unsafe { reeln_probe(path.as_ptr()) };
        assert!(!result.error.is_null());
        unsafe {
            reeln_free_string(result.codec);
            reeln_free_string(result.error);
        }
    }

    #[test]
    fn test_probe_result_free_null() {
        // Should not panic.
        unsafe { reeln_probe_result_free(ptr::null_mut()) };
    }

    #[test]
    fn test_probe_result_free_valid() {
        let mut result = ReelnProbeResult {
            duration_secs: -1.0,
            fps: -1.0,
            width: 0,
            height: 0,
            codec: string_to_cstr("h264"),
            error: string_to_cstr("some error"),
        };
        unsafe { reeln_probe_result_free(&mut result) };
        assert!(result.codec.is_null());
        assert!(result.error.is_null());
    }

    #[test]
    fn test_concat_null_output() {
        let err = unsafe { reeln_concat(ptr::null(), 0, ptr::null(), true) };
        assert!(!err.is_null());
        unsafe { reeln_free_string(err) };
    }

    #[test]
    fn test_concat_no_segments() {
        let output = CString::new("/tmp/out.mp4").unwrap();
        let err = unsafe { reeln_concat(ptr::null(), 0, output.as_ptr(), true) };
        assert!(!err.is_null());
        let msg = unsafe { CStr::from_ptr(err) }.to_str().unwrap();
        assert!(msg.contains("no segments"));
        unsafe { reeln_free_string(err) };
    }

    #[test]
    fn test_concat_invalid_segment() {
        let output = CString::new("/tmp/out.mp4").unwrap();
        let segs: Vec<*const c_char> = vec![ptr::null()];
        let err = unsafe { reeln_concat(segs.as_ptr(), segs.len(), output.as_ptr(), true) };
        assert!(!err.is_null());
        let msg = unsafe { CStr::from_ptr(err) }.to_str().unwrap();
        assert!(msg.contains("invalid segment"));
        unsafe { reeln_free_string(err) };
    }

    #[test]
    fn test_concat_nonexistent_segment() {
        let output = CString::new("/tmp/ffi_concat_out.mp4").unwrap();
        let seg = CString::new("/tmp/ffi_nonexistent.mp4").unwrap();
        let segs: Vec<*const c_char> = vec![seg.as_ptr()];
        let err = unsafe { reeln_concat(segs.as_ptr(), segs.len(), output.as_ptr(), true) };
        assert!(!err.is_null());
        unsafe { reeln_free_string(err) };
    }

    #[test]
    fn test_game_dir_name() {
        let date = CString::new("2026-02-26").unwrap();
        let home = CString::new("roseville").unwrap();
        let away = CString::new("mahtomedi").unwrap();
        let result = unsafe { reeln_game_dir_name(date.as_ptr(), home.as_ptr(), away.as_ptr(), 1) };
        assert!(!result.is_null());
        let s = unsafe { CStr::from_ptr(result) }.to_str().unwrap();
        assert_eq!(s, "2026-02-26_roseville_vs_mahtomedi");
        unsafe { reeln_free_string(result) };
    }

    #[test]
    fn test_game_dir_name_double_header() {
        let date = CString::new("2026-02-26").unwrap();
        let home = CString::new("a").unwrap();
        let away = CString::new("b").unwrap();
        let result = unsafe { reeln_game_dir_name(date.as_ptr(), home.as_ptr(), away.as_ptr(), 2) };
        let s = unsafe { CStr::from_ptr(result) }.to_str().unwrap();
        assert_eq!(s, "2026-02-26_a_vs_b_g2");
        unsafe { reeln_free_string(result) };
    }

    #[test]
    fn test_game_dir_name_null_params() {
        let result = unsafe { reeln_game_dir_name(ptr::null(), ptr::null(), ptr::null(), 1) };
        assert!(!result.is_null());
        // Should produce a valid (though empty) string.
        unsafe { reeln_free_string(result) };
    }

    #[test]
    fn test_segment_dir_name_hockey() {
        let sport = CString::new("hockey").unwrap();
        let result = unsafe { reeln_segment_dir_name(sport.as_ptr(), 1) };
        assert!(!result.is_null());
        let s = unsafe { CStr::from_ptr(result) }.to_str().unwrap();
        assert_eq!(s, "period-1");
        unsafe { reeln_free_string(result) };
    }

    #[test]
    fn test_segment_dir_name_unknown_sport() {
        let sport = CString::new("quidditch").unwrap();
        let result = unsafe { reeln_segment_dir_name(sport.as_ptr(), 1) };
        assert!(result.is_null());
    }

    #[test]
    fn test_segment_dir_name_null() {
        let result = unsafe { reeln_segment_dir_name(ptr::null(), 1) };
        assert!(result.is_null());
    }

    #[test]
    fn test_segment_display_name_hockey() {
        let sport = CString::new("hockey").unwrap();
        let result = unsafe { reeln_segment_display_name(sport.as_ptr(), 2) };
        assert!(!result.is_null());
        let s = unsafe { CStr::from_ptr(result) }.to_str().unwrap();
        assert_eq!(s, "Period 2");
        unsafe { reeln_free_string(result) };
    }

    #[test]
    fn test_segment_display_name_unknown() {
        let sport = CString::new("quidditch").unwrap();
        let result = unsafe { reeln_segment_display_name(sport.as_ptr(), 1) };
        assert!(result.is_null());
    }

    #[test]
    fn test_segment_display_name_null() {
        let result = unsafe { reeln_segment_display_name(ptr::null(), 1) };
        assert!(result.is_null());
    }

    #[test]
    fn test_string_to_cstr_and_back() {
        let s = string_to_cstr("hello");
        assert!(!s.is_null());
        let back = unsafe { CStr::from_ptr(s) }.to_str().unwrap();
        assert_eq!(back, "hello");
        unsafe { reeln_free_string(s) };
    }

    #[test]
    fn test_cstr_to_str_null() {
        let result = unsafe { cstr_to_str(ptr::null()) };
        assert!(result.is_none());
    }

    #[test]
    fn test_cstr_to_str_valid() {
        let cs = CString::new("test").unwrap();
        let result = unsafe { cstr_to_str(cs.as_ptr()) };
        assert_eq!(result, Some("test"));
    }

    // ── Overlay FFI tests ────────────────────────────────────────────

    #[test]
    fn test_render_overlay_null_template() {
        let ctx = CString::new("{}").unwrap();
        let out = CString::new("/tmp/out.png").unwrap();
        let err = unsafe { reeln_render_overlay(ptr::null(), ctx.as_ptr(), out.as_ptr()) };
        assert!(!err.is_null());
        let msg = unsafe { CStr::from_ptr(err) }.to_str().unwrap();
        assert!(msg.contains("null"));
        unsafe { reeln_free_string(err) };
    }

    #[test]
    fn test_render_overlay_null_context() {
        let tmpl = CString::new("{}").unwrap();
        let out = CString::new("/tmp/out.png").unwrap();
        let err = unsafe { reeln_render_overlay(tmpl.as_ptr(), ptr::null(), out.as_ptr()) };
        assert!(!err.is_null());
        let msg = unsafe { CStr::from_ptr(err) }.to_str().unwrap();
        assert!(msg.contains("null"));
        unsafe { reeln_free_string(err) };
    }

    #[test]
    fn test_render_overlay_null_output() {
        let tmpl = CString::new("{}").unwrap();
        let ctx = CString::new("{}").unwrap();
        let err = unsafe { reeln_render_overlay(tmpl.as_ptr(), ctx.as_ptr(), ptr::null()) };
        assert!(!err.is_null());
        let msg = unsafe { CStr::from_ptr(err) }.to_str().unwrap();
        assert!(msg.contains("null"));
        unsafe { reeln_free_string(err) };
    }

    #[test]
    fn test_render_overlay_invalid_template_json() {
        let tmpl = CString::new("not json at all").unwrap();
        let ctx = CString::new("{}").unwrap();
        let out = CString::new("/tmp/out.png").unwrap();
        let err = unsafe { reeln_render_overlay(tmpl.as_ptr(), ctx.as_ptr(), out.as_ptr()) };
        assert!(!err.is_null());
        let msg = unsafe { CStr::from_ptr(err) }.to_str().unwrap();
        assert!(msg.contains("invalid template JSON"));
        unsafe { reeln_free_string(err) };
    }

    #[test]
    fn test_render_overlay_invalid_context_json() {
        let tmpl = CString::new(
            r#"{"name":"t","version":1,"canvas":{"width":100,"height":100},"layers":[]}"#,
        )
        .unwrap();
        let ctx = CString::new("not json").unwrap();
        let out = CString::new("/tmp/out.png").unwrap();
        let err = unsafe { reeln_render_overlay(tmpl.as_ptr(), ctx.as_ptr(), out.as_ptr()) };
        assert!(!err.is_null());
        let msg = unsafe { CStr::from_ptr(err) }.to_str().unwrap();
        assert!(msg.contains("invalid context JSON"));
        unsafe { reeln_free_string(err) };
    }

    #[test]
    fn test_load_template_null() {
        let result = unsafe { reeln_load_template(ptr::null()) };
        assert!(result.is_null());
    }

    #[test]
    fn test_load_template_nonexistent() {
        let path = CString::new("/tmp/nonexistent_template.json").unwrap();
        let result = unsafe { reeln_load_template(path.as_ptr()) };
        assert!(result.is_null());
    }

    // ── Config FFI tests ─────────────────────────────────────────────

    #[test]
    fn test_config_dir() {
        let result = reeln_config_dir();
        assert!(!result.is_null());
        let s = unsafe { CStr::from_ptr(result) }.to_str().unwrap();
        assert!(!s.is_empty());
        unsafe { reeln_free_string(result) };
    }

    #[test]
    fn test_data_dir() {
        let result = reeln_data_dir();
        assert!(!result.is_null());
        let s = unsafe { CStr::from_ptr(result) }.to_str().unwrap();
        assert!(!s.is_empty());
        unsafe { reeln_free_string(result) };
    }

    #[test]
    fn test_load_config_null_path() {
        // With null path, uses default. May return null if no config exists.
        let result = unsafe { reeln_load_config(ptr::null(), ptr::null()) };
        // Either valid JSON or null is acceptable.
        if !result.is_null() {
            unsafe { reeln_free_string(result) };
        }
    }

    // ── Plugin FFI tests ─────────────────────────────────────────────

    #[test]
    fn test_list_hooks() {
        let result = reeln_list_hooks();
        assert!(!result.is_null());
        let s = unsafe { CStr::from_ptr(result) }.to_str().unwrap();
        assert!(s.contains("on_game_init"));
        assert!(s.contains("pre_render"));
        assert!(s.contains("on_error"));
        // Should have 14 hooks.
        assert_eq!(s.split(',').count(), 14);
        unsafe { reeln_free_string(result) };
    }

    #[test]
    fn test_composite_null_video() {
        let overlay = CString::new("/tmp/overlay.png").unwrap();
        let output = CString::new("/tmp/out.mp4").unwrap();
        let err = unsafe {
            reeln_composite(
                ptr::null(),
                overlay.as_ptr(),
                output.as_ptr(),
                0,
                0,
                -1.0,
                -1.0,
            )
        };
        assert!(!err.is_null());
        let msg = unsafe { CStr::from_ptr(err) }.to_str().unwrap();
        assert!(msg.contains("null"));
        unsafe { reeln_free_string(err) };
    }

    #[test]
    fn test_composite_null_overlay() {
        let video = CString::new("/tmp/video.mp4").unwrap();
        let output = CString::new("/tmp/out.mp4").unwrap();
        let err = unsafe {
            reeln_composite(
                video.as_ptr(),
                ptr::null(),
                output.as_ptr(),
                0,
                0,
                -1.0,
                -1.0,
            )
        };
        assert!(!err.is_null());
        let msg = unsafe { CStr::from_ptr(err) }.to_str().unwrap();
        assert!(msg.contains("null"));
        unsafe { reeln_free_string(err) };
    }

    #[test]
    fn test_composite_null_output() {
        let video = CString::new("/tmp/video.mp4").unwrap();
        let overlay = CString::new("/tmp/overlay.png").unwrap();
        let err = unsafe {
            reeln_composite(
                video.as_ptr(),
                overlay.as_ptr(),
                ptr::null(),
                0,
                0,
                -1.0,
                -1.0,
            )
        };
        assert!(!err.is_null());
        let msg = unsafe { CStr::from_ptr(err) }.to_str().unwrap();
        assert!(msg.contains("null"));
        unsafe { reeln_free_string(err) };
    }

    #[test]
    fn test_composite_nonexistent_video() {
        let video = CString::new("/tmp/nonexistent_composite_video.mp4").unwrap();
        let overlay = CString::new("/tmp/overlay.png").unwrap();
        let output = CString::new("/tmp/out.mp4").unwrap();
        let err = unsafe {
            reeln_composite(
                video.as_ptr(),
                overlay.as_ptr(),
                output.as_ptr(),
                0,
                0,
                -1.0,
                -1.0,
            )
        };
        assert!(!err.is_null());
        let msg = unsafe { CStr::from_ptr(err) }.to_str().unwrap();
        assert!(msg.contains("does not exist"));
        unsafe { reeln_free_string(err) };
    }
}
