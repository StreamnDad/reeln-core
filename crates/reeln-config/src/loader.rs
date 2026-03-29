use std::path::Path;

use crate::error::ConfigError;
use crate::model::AppConfig;

/// The current config version supported by this build.
pub const CURRENT_CONFIG_VERSION: u64 = 1;

/// Return a default `AppConfig` with all defaults applied.
pub fn default_config() -> AppConfig {
    AppConfig::default()
}

/// Load config from a JSON file, optionally merging a profile overlay.
///
/// 1. Reads the base config file.
/// 2. If a profile is given, reads `config.<profile>.json` in the same
///    directory and deep-merges it on top.
/// 3. Deserializes into `AppConfig`.
pub fn load_config(path: &Path, profile: Option<&str>) -> Result<AppConfig, ConfigError> {
    if !path.exists() {
        return Err(ConfigError::NotFound(path.display().to_string()));
    }
    let content = std::fs::read_to_string(path)?;
    let mut base: serde_json::Value = serde_json::from_str(&content)?;

    // Merge profile overlay if it exists
    if let Some(profile_name) = profile {
        let profile_path = path
            .parent()
            .unwrap_or(Path::new("."))
            .join(format!("config.{profile_name}.json"));
        if profile_path.exists() {
            let overlay_content = std::fs::read_to_string(&profile_path)?;
            let overlay: serde_json::Value = serde_json::from_str(&overlay_content)?;
            deep_merge(&mut base, &overlay);
        }
    }

    let config: AppConfig = serde_json::from_value(base)?;
    Ok(config)
}

/// Save config to a JSON file with atomic write.
///
/// Writes to a temporary file in the same directory, then renames to the
/// target path for crash safety.
pub fn save_config(config: &AppConfig, path: &Path) -> Result<std::path::PathBuf, ConfigError> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;

    let json = serde_json::to_string_pretty(config)?;

    // Atomic write: write to temp file then rename
    let tmp_path = path.with_extension("json.tmp");
    std::fs::write(&tmp_path, &json)?;
    std::fs::rename(&tmp_path, path)?;

    Ok(path.to_path_buf())
}

/// Deep-merge two JSON values (overlay wins on conflict).
pub fn deep_merge(base: &mut serde_json::Value, overlay: &serde_json::Value) {
    if let (serde_json::Value::Object(base_map), serde_json::Value::Object(overlay_map)) =
        (base, overlay)
    {
        for (key, value) in overlay_map {
            let entry = base_map
                .entry(key.clone())
                .or_insert(serde_json::Value::Null);
            if value.is_object() && entry.is_object() {
                deep_merge(entry, value);
            } else {
                *entry = value.clone();
            }
        }
    }
}

/// Apply `REELN_*` environment variable overrides to a config.
pub fn apply_env_overrides(config: &mut AppConfig) {
    // Top-level
    if let Ok(val) = std::env::var("REELN_SPORT") {
        config.sport = val;
    }

    // Video
    if let Ok(val) = std::env::var("REELN_VIDEO_CODEC") {
        config.video.codec = val;
    }
    if let Ok(val) = std::env::var("REELN_VIDEO_PRESET") {
        config.video.preset = val;
    }
    if let Ok(val) = std::env::var("REELN_VIDEO_CRF")
        && let Ok(crf) = val.parse()
    {
        config.video.crf = crf;
    }
    if let Ok(val) = std::env::var("REELN_VIDEO_FFMPEG_PATH") {
        config.video.ffmpeg_path = val;
    }
    if let Ok(val) = std::env::var("REELN_VIDEO_AUDIO_CODEC") {
        config.video.audio_codec = val;
    }
    if let Ok(val) = std::env::var("REELN_VIDEO_AUDIO_BITRATE") {
        config.video.audio_bitrate = val;
    }

    // Paths
    if let Ok(val) = std::env::var("REELN_PATHS_SOURCE_DIR") {
        config.paths.source_dir = Some(std::path::PathBuf::from(val));
    }
    if let Ok(val) = std::env::var("REELN_PATHS_OUTPUT_DIR") {
        config.paths.output_dir = Some(std::path::PathBuf::from(val));
    }
    if let Ok(val) = std::env::var("REELN_PATHS_TEMP_DIR") {
        config.paths.temp_dir = Some(std::path::PathBuf::from(val));
    }
    if let Ok(val) = std::env::var("REELN_PATHS_SOURCE_GLOB") {
        config.paths.source_glob = val;
    }
}

/// Validate a config JSON value and return a list of warning strings.
///
/// Does not error — collects warnings for the caller to handle.
pub fn validate_config(data: &serde_json::Value) -> Vec<String> {
    let mut warnings = Vec::new();

    // Check config_version
    if let Some(ver) = data.get("config_version") {
        if let Some(v) = ver.as_u64() {
            if v > CURRENT_CONFIG_VERSION {
                warnings.push(format!(
                    "config_version {v} is newer than supported version {CURRENT_CONFIG_VERSION}"
                ));
            }
        } else {
            warnings.push("config_version must be an integer".to_string());
        }
    }

    // Check that known sections are objects if present
    let sections = [
        "video",
        "paths",
        "render_profiles",
        "iterations",
        "branding",
        "orchestration",
        "plugins",
    ];
    for section in &sections {
        if let Some(val) = data.get(*section)
            && !val.is_object()
        {
            warnings.push(format!("'{section}' must be an object"));
        }
    }

    // Validate event_types if present
    if let Some(val) = data.get("event_types") {
        if let Some(arr) = val.as_array() {
            for (i, item) in arr.iter().enumerate() {
                let valid = item.is_string()
                    || (item.is_object() && item.get("name").is_some_and(|n| n.is_string()));
                if !valid {
                    warnings.push(format!(
                        "event_types[{i}] must be a string or {{\"name\": ..., \"team_specific\": ...}}"
                    ));
                }
            }
        } else {
            warnings.push("'event_types' must be an array".to_string());
        }
    }

    // Cross-validate: iterations referencing types not in event_types
    if let Some(event_types_val) = data.get("event_types")
        && let Some(event_types_arr) = event_types_val.as_array()
        && !event_types_arr.is_empty()
    {
        let event_types: Vec<&str> = event_types_arr
            .iter()
            .filter_map(|v| {
                v.as_str()
                    .or_else(|| v.get("name").and_then(|n| n.as_str()))
            })
            .collect();
        if let Some(iterations) = data.get("iterations")
            && let Some(iter_map) = iterations.as_object()
        {
            for key in iter_map.keys() {
                if key != "default" && !event_types.contains(&key.as_str()) {
                    warnings.push(format!(
                        "iterations references type '{key}' not listed in event_types"
                    ));
                }
            }
        }
    }

    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    // ── deep_merge tests ─────────────────────────────────────────────

    #[test]
    fn test_deep_merge_basic() {
        let mut base = serde_json::json!({"a": 1, "b": 2});
        let overlay = serde_json::json!({"b": 3, "c": 4});
        deep_merge(&mut base, &overlay);
        assert_eq!(base["a"], 1);
        assert_eq!(base["b"], 3);
        assert_eq!(base["c"], 4);
    }

    #[test]
    fn test_deep_merge_nested() {
        let mut base = serde_json::json!({"video": {"codec": "libx264", "crf": 18}});
        let overlay = serde_json::json!({"video": {"crf": 22}});
        deep_merge(&mut base, &overlay);
        assert_eq!(base["video"]["codec"], "libx264");
        assert_eq!(base["video"]["crf"], 22);
    }

    #[test]
    fn test_deep_merge_overlay_non_object() {
        let mut base = serde_json::json!({"a": {"nested": 1}});
        let overlay = serde_json::json!({"a": "replaced"});
        deep_merge(&mut base, &overlay);
        assert_eq!(base["a"], "replaced");
    }

    #[test]
    fn test_deep_merge_base_non_object_overlay_object() {
        let mut base = serde_json::json!({"a": "string"});
        let overlay = serde_json::json!({"a": {"nested": 1}});
        deep_merge(&mut base, &overlay);
        assert_eq!(base["a"]["nested"], 1);
    }

    #[test]
    fn test_deep_merge_non_objects() {
        let mut base = serde_json::json!(42);
        let overlay = serde_json::json!(99);
        deep_merge(&mut base, &overlay);
        assert_eq!(base, 42);
    }

    #[test]
    fn test_deep_merge_new_nested_key() {
        let mut base = serde_json::json!({"video": {"codec": "libx264"}});
        let overlay = serde_json::json!({"video": {"preset": "slow"}});
        deep_merge(&mut base, &overlay);
        assert_eq!(base["video"]["codec"], "libx264");
        assert_eq!(base["video"]["preset"], "slow");
    }

    // ── default_config tests ─────────────────────────────────────────

    #[test]
    fn test_default_config() {
        let c = default_config();
        assert_eq!(c.config_version, 1);
        assert_eq!(c.sport, "generic");
        assert_eq!(c.video.codec, "libx264");
        assert_eq!(c.video.crf, 18);
        assert_eq!(c.paths.source_glob, "Replay_*.mkv");
        assert!(c.branding.enabled);
        assert!(c.orchestration.sequential);
    }

    // ── load_config / save_config tests ──────────────────────────────

    #[test]
    fn test_load_config_not_found() {
        let result = load_config(Path::new("/nonexistent/config.json"), None);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ConfigError::NotFound(_)));
    }

    #[test]
    fn test_load_save_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");

        let mut config = default_config();
        config.sport = "hockey".to_string();
        config.video.crf = 22;

        save_config(&config, &path).unwrap();
        let loaded = load_config(&path, None).unwrap();

        assert_eq!(loaded.sport, "hockey");
        assert_eq!(loaded.video.crf, 22);
        assert_eq!(loaded.video.codec, "libx264");
    }

    #[test]
    fn test_save_config_creates_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deep").join("nested").join("config.json");

        let config = default_config();
        let result = save_config(&config, &path);
        assert!(result.is_ok());
        assert!(path.exists());
    }

    #[test]
    fn test_save_config_returns_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let config = default_config();
        let returned = save_config(&config, &path).unwrap();
        assert_eq!(returned, path);
    }

    #[test]
    fn test_load_config_with_profile_overlay() {
        let dir = tempfile::tempdir().unwrap();
        let base_path = dir.path().join("config.json");
        let profile_path = dir.path().join("config.dev.json");

        let base = serde_json::json!({
            "sport": "hockey",
            "video": {"crf": 18, "codec": "libx264"}
        });
        std::fs::write(&base_path, serde_json::to_string(&base).unwrap()).unwrap();

        let overlay = serde_json::json!({
            "video": {"crf": 28}
        });
        std::fs::write(&profile_path, serde_json::to_string(&overlay).unwrap()).unwrap();

        let config = load_config(&base_path, Some("dev")).unwrap();
        assert_eq!(config.sport, "hockey");
        assert_eq!(config.video.crf, 28);
        assert_eq!(config.video.codec, "libx264");
    }

    #[test]
    fn test_load_config_profile_not_found_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        let base_path = dir.path().join("config.json");
        std::fs::write(&base_path, "{}").unwrap();

        let config = load_config(&base_path, Some("nonexistent")).unwrap();
        assert_eq!(config.config_version, 1);
    }

    #[test]
    fn test_load_config_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, "not json").unwrap();

        let result = load_config(&path, None);
        assert!(result.is_err());
    }

    // ── apply_env_overrides tests ────────────────────────────────────

    #[test]
    #[serial]
    fn test_apply_env_overrides_sport() {
        let mut config = default_config();
        unsafe { std::env::set_var("REELN_SPORT", "hockey") };
        apply_env_overrides(&mut config);
        assert_eq!(config.sport, "hockey");
        unsafe { std::env::remove_var("REELN_SPORT") };
    }

    #[test]
    #[serial]
    fn test_apply_env_overrides_video_codec() {
        let mut config = default_config();
        unsafe { std::env::set_var("REELN_VIDEO_CODEC", "libx265") };
        apply_env_overrides(&mut config);
        assert_eq!(config.video.codec, "libx265");
        unsafe { std::env::remove_var("REELN_VIDEO_CODEC") };
    }

    #[test]
    #[serial]
    fn test_apply_env_overrides_video_crf() {
        let mut config = default_config();
        unsafe { std::env::set_var("REELN_VIDEO_CRF", "22") };
        apply_env_overrides(&mut config);
        assert_eq!(config.video.crf, 22);
        unsafe { std::env::remove_var("REELN_VIDEO_CRF") };
    }

    #[test]
    #[serial]
    fn test_apply_env_overrides_video_crf_invalid() {
        let mut config = default_config();
        unsafe { std::env::set_var("REELN_VIDEO_CRF", "not_a_number") };
        apply_env_overrides(&mut config);
        assert_eq!(config.video.crf, 18);
        unsafe { std::env::remove_var("REELN_VIDEO_CRF") };
    }

    #[test]
    #[serial]
    fn test_apply_env_overrides_video_preset() {
        let mut config = default_config();
        unsafe { std::env::set_var("REELN_VIDEO_PRESET", "slow") };
        apply_env_overrides(&mut config);
        assert_eq!(config.video.preset, "slow");
        unsafe { std::env::remove_var("REELN_VIDEO_PRESET") };
    }

    #[test]
    #[serial]
    fn test_apply_env_overrides_video_ffmpeg_path() {
        let mut config = default_config();
        unsafe { std::env::set_var("REELN_VIDEO_FFMPEG_PATH", "/usr/local/bin/ffmpeg") };
        apply_env_overrides(&mut config);
        assert_eq!(config.video.ffmpeg_path, "/usr/local/bin/ffmpeg");
        unsafe { std::env::remove_var("REELN_VIDEO_FFMPEG_PATH") };
    }

    #[test]
    #[serial]
    fn test_apply_env_overrides_video_audio_codec() {
        let mut config = default_config();
        unsafe { std::env::set_var("REELN_VIDEO_AUDIO_CODEC", "opus") };
        apply_env_overrides(&mut config);
        assert_eq!(config.video.audio_codec, "opus");
        unsafe { std::env::remove_var("REELN_VIDEO_AUDIO_CODEC") };
    }

    #[test]
    #[serial]
    fn test_apply_env_overrides_video_audio_bitrate() {
        let mut config = default_config();
        unsafe { std::env::set_var("REELN_VIDEO_AUDIO_BITRATE", "256k") };
        apply_env_overrides(&mut config);
        assert_eq!(config.video.audio_bitrate, "256k");
        unsafe { std::env::remove_var("REELN_VIDEO_AUDIO_BITRATE") };
    }

    #[test]
    #[serial]
    fn test_apply_env_overrides_paths_source_dir() {
        let mut config = default_config();
        unsafe { std::env::set_var("REELN_PATHS_SOURCE_DIR", "/tmp/replays") };
        apply_env_overrides(&mut config);
        assert_eq!(
            config.paths.source_dir,
            Some(std::path::PathBuf::from("/tmp/replays"))
        );
        unsafe { std::env::remove_var("REELN_PATHS_SOURCE_DIR") };
    }

    #[test]
    #[serial]
    fn test_apply_env_overrides_paths_output_dir() {
        let mut config = default_config();
        unsafe { std::env::set_var("REELN_PATHS_OUTPUT_DIR", "/tmp/output") };
        apply_env_overrides(&mut config);
        assert_eq!(
            config.paths.output_dir,
            Some(std::path::PathBuf::from("/tmp/output"))
        );
        unsafe { std::env::remove_var("REELN_PATHS_OUTPUT_DIR") };
    }

    #[test]
    #[serial]
    fn test_apply_env_overrides_paths_temp_dir() {
        let mut config = default_config();
        unsafe { std::env::set_var("REELN_PATHS_TEMP_DIR", "/tmp/work") };
        apply_env_overrides(&mut config);
        assert_eq!(
            config.paths.temp_dir,
            Some(std::path::PathBuf::from("/tmp/work"))
        );
        unsafe { std::env::remove_var("REELN_PATHS_TEMP_DIR") };
    }

    #[test]
    #[serial]
    fn test_apply_env_overrides_paths_source_glob() {
        let mut config = default_config();
        unsafe { std::env::set_var("REELN_PATHS_SOURCE_GLOB", "*.mp4") };
        apply_env_overrides(&mut config);
        assert_eq!(config.paths.source_glob, "*.mp4");
        unsafe { std::env::remove_var("REELN_PATHS_SOURCE_GLOB") };
    }

    // ── validate_config tests ────────────────────────────────────────

    #[test]
    fn test_validate_config_valid() {
        let data = serde_json::json!({
            "config_version": 1,
            "video": {},
            "paths": {}
        });
        let warnings = validate_config(&data);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_validate_config_future_version() {
        let data = serde_json::json!({"config_version": 99});
        let warnings = validate_config(&data);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("newer than supported"));
    }

    #[test]
    fn test_validate_config_version_not_int() {
        let data = serde_json::json!({"config_version": "one"});
        let warnings = validate_config(&data);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("must be an integer"));
    }

    #[test]
    fn test_validate_config_section_not_object() {
        let data = serde_json::json!({"video": "invalid"});
        let warnings = validate_config(&data);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("'video' must be an object"));
    }

    #[test]
    fn test_validate_config_multiple_bad_sections() {
        let data = serde_json::json!({
            "video": 123,
            "paths": [],
            "branding": "bad"
        });
        let warnings = validate_config(&data);
        assert_eq!(warnings.len(), 3);
    }

    #[test]
    fn test_validate_config_empty() {
        let data = serde_json::json!({});
        let warnings = validate_config(&data);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_validate_config_current_version() {
        let data = serde_json::json!({"config_version": 1});
        let warnings = validate_config(&data);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_validate_config_all_sections_valid() {
        let data = serde_json::json!({
            "video": {},
            "paths": {},
            "render_profiles": {},
            "iterations": {},
            "branding": {},
            "orchestration": {},
            "plugins": {}
        });
        let warnings = validate_config(&data);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_validate_config_event_types_valid() {
        let data = serde_json::json!({
            "event_types": ["goal", "save", "penalty"]
        });
        let warnings = validate_config(&data);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_validate_config_event_types_not_array() {
        let data = serde_json::json!({"event_types": "goal"});
        let warnings = validate_config(&data);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("must be an array"));
    }

    #[test]
    fn test_validate_config_event_types_non_string_element() {
        let data = serde_json::json!({"event_types": ["goal", 42, "save"]});
        let warnings = validate_config(&data);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("event_types[1]"));
    }

    #[test]
    fn test_validate_config_event_types_full_objects_valid() {
        let data = serde_json::json!({
            "event_types": [
                {"name": "goal", "team_specific": true},
                {"name": "timeout"},
                "clip"
            ]
        });
        let warnings = validate_config(&data);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_validate_config_event_types_object_missing_name() {
        let data = serde_json::json!({"event_types": [{"team_specific": true}]});
        let warnings = validate_config(&data);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("event_types[0]"));
    }

    #[test]
    fn test_validate_config_iterations_cross_validate_with_objects() {
        let data = serde_json::json!({
            "event_types": [{"name": "goal", "team_specific": true}, "save"],
            "iterations": {"goal": ["p1"], "penalty": ["p2"]}
        });
        let warnings = validate_config(&data);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("iterations references type 'penalty'"));
    }

    #[test]
    fn test_validate_config_event_types_empty_array() {
        let data = serde_json::json!({"event_types": []});
        let warnings = validate_config(&data);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_validate_config_iterations_references_unlisted_type() {
        let data = serde_json::json!({
            "event_types": ["goal", "save"],
            "iterations": {"goal": ["profile1"], "penalty": ["profile2"]}
        });
        let warnings = validate_config(&data);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("iterations references type 'penalty'"));
    }

    #[test]
    fn test_validate_config_iterations_default_not_warned() {
        let data = serde_json::json!({
            "event_types": ["goal"],
            "iterations": {"goal": ["p1"], "default": ["p2"]}
        });
        let warnings = validate_config(&data);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_validate_config_iterations_no_cross_validate_when_event_types_empty() {
        let data = serde_json::json!({
            "event_types": [],
            "iterations": {"penalty": ["profile1"]}
        });
        let warnings = validate_config(&data);
        assert!(warnings.is_empty());
    }
}
