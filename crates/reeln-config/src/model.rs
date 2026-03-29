use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

// ── Default value helpers ────────────────────────────────────────────

fn default_ffmpeg_path() -> String {
    "ffmpeg".to_string()
}
fn default_codec() -> String {
    "libx264".to_string()
}
fn default_preset() -> String {
    "medium".to_string()
}
fn default_crf() -> u32 {
    18
}
fn default_audio_codec() -> String {
    "aac".to_string()
}
fn default_audio_bitrate() -> String {
    "128k".to_string()
}
fn default_source_glob() -> String {
    "Replay_*.mkv".to_string()
}
fn default_config_version() -> u32 {
    1
}
fn default_sport() -> String {
    "generic".to_string()
}
fn default_branding_enabled() -> bool {
    true
}
fn default_branding_template() -> String {
    "builtin:branding".to_string()
}
fn default_branding_duration() -> f64 {
    5.0
}
fn default_sequential() -> bool {
    true
}
fn default_enforce_hooks() -> bool {
    true
}

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

// ── VideoConfig ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VideoConfig {
    #[serde(default = "default_ffmpeg_path")]
    pub ffmpeg_path: String,
    #[serde(default = "default_codec")]
    pub codec: String,
    #[serde(default = "default_preset")]
    pub preset: String,
    #[serde(default = "default_crf")]
    pub crf: u32,
    #[serde(default = "default_audio_codec")]
    pub audio_codec: String,
    #[serde(default = "default_audio_bitrate")]
    pub audio_bitrate: String,
}

impl Default for VideoConfig {
    fn default() -> Self {
        Self {
            ffmpeg_path: default_ffmpeg_path(),
            codec: default_codec(),
            preset: default_preset(),
            crf: default_crf(),
            audio_codec: default_audio_codec(),
            audio_bitrate: default_audio_bitrate(),
        }
    }
}

// ── PathConfig ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PathConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_dir: Option<PathBuf>,
    #[serde(default = "default_source_glob")]
    pub source_glob: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_dir: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temp_dir: Option<PathBuf>,
}

impl Default for PathConfig {
    fn default() -> Self {
        Self {
            source_dir: None,
            source_glob: default_source_glob(),
            output_dir: None,
            temp_dir: None,
        }
    }
}

// ── PluginsConfig ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginsConfig {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enabled: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disabled: Vec<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub settings: HashMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub registry_url: String,
    #[serde(default = "default_enforce_hooks")]
    pub enforce_hooks: bool,
}

impl Default for PluginsConfig {
    fn default() -> Self {
        Self {
            enabled: Vec::new(),
            disabled: Vec::new(),
            settings: HashMap::new(),
            registry_url: String::new(),
            enforce_hooks: default_enforce_hooks(),
        }
    }
}

// ── BrandingConfig ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BrandingConfig {
    #[serde(default = "default_branding_enabled")]
    pub enabled: bool,
    #[serde(default = "default_branding_template")]
    pub template: String,
    #[serde(default = "default_branding_duration")]
    pub duration: f64,
}

impl Default for BrandingConfig {
    fn default() -> Self {
        Self {
            enabled: default_branding_enabled(),
            template: default_branding_template(),
            duration: default_branding_duration(),
        }
    }
}

// ── OrchestrationConfig ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OrchestrationConfig {
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub upload_bitrate_kbps: u32,
    #[serde(default = "default_sequential")]
    pub sequential: bool,
}

impl Default for OrchestrationConfig {
    fn default() -> Self {
        Self {
            upload_bitrate_kbps: 0,
            sequential: default_sequential(),
        }
    }
}

// ── SpeedSegment ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpeedSegment {
    pub speed: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<f64>,
}

// ── RenderProfile ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RenderProfile {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor_x: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor_y: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pad_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smart: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed_segments: Option<Vec<SpeedSegment>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lut: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle_template: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codec: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crf: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_codec: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_bitrate: Option<String>,
}

// ── IterationConfig ──────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct IterationConfig {
    #[serde(flatten, default, skip_serializing_if = "HashMap::is_empty")]
    pub mappings: HashMap<String, Vec<String>>,
}

impl IterationConfig {
    /// Returns profile names for a given event type, falling back to "default".
    pub fn profiles_for_event(&self, event_type: &str) -> Vec<String> {
        if let Some(profiles) = self.mappings.get(event_type) {
            return profiles.clone();
        }
        if let Some(profiles) = self.mappings.get("default") {
            return profiles.clone();
        }
        Vec::new()
    }
}

// ── EventTypeEntry ───────────────────────────────────────────────────

/// A configured event type, supporting both simple strings and full entries.
///
/// Backward compatible: `"goal"` deserializes as `Simple("goal")`,
/// `{"name": "goal", "team_specific": true}` as `Full { .. }`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum EventTypeEntry {
    Full {
        name: String,
        #[serde(default)]
        team_specific: bool,
    },
    Simple(String),
}

impl EventTypeEntry {
    /// The event type name.
    pub fn name(&self) -> &str {
        match self {
            Self::Simple(s) => s,
            Self::Full { name, .. } => name,
        }
    }

    /// Whether this event type has Home/Away variants.
    pub fn team_specific(&self) -> bool {
        match self {
            Self::Simple(_) => false,
            Self::Full { team_specific, .. } => *team_specific,
        }
    }
}

// ── AppConfig ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppConfig {
    #[serde(default = "default_config_version")]
    pub config_version: u32,
    #[serde(default = "default_sport")]
    pub sport: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_types: Vec<EventTypeEntry>,
    #[serde(default)]
    pub video: VideoConfig,
    #[serde(default)]
    pub paths: PathConfig,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub render_profiles: HashMap<String, RenderProfile>,
    #[serde(default)]
    pub iterations: IterationConfig,
    #[serde(default)]
    pub branding: BrandingConfig,
    #[serde(default)]
    pub orchestration: OrchestrationConfig,
    #[serde(default)]
    pub plugins: PluginsConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            config_version: default_config_version(),
            sport: default_sport(),
            event_types: Vec::new(),
            video: VideoConfig::default(),
            paths: PathConfig::default(),
            render_profiles: HashMap::new(),
            iterations: IterationConfig::default(),
            branding: BrandingConfig::default(),
            orchestration: OrchestrationConfig::default(),
            plugins: PluginsConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_video_config_defaults() {
        let v = VideoConfig::default();
        assert_eq!(v.ffmpeg_path, "ffmpeg");
        assert_eq!(v.codec, "libx264");
        assert_eq!(v.preset, "medium");
        assert_eq!(v.crf, 18);
        assert_eq!(v.audio_codec, "aac");
        assert_eq!(v.audio_bitrate, "128k");
    }

    #[test]
    fn test_path_config_defaults() {
        let p = PathConfig::default();
        assert_eq!(p.source_dir, None);
        assert_eq!(p.source_glob, "Replay_*.mkv");
        assert_eq!(p.output_dir, None);
        assert_eq!(p.temp_dir, None);
    }

    #[test]
    fn test_plugins_config_defaults() {
        let p = PluginsConfig::default();
        assert!(p.enabled.is_empty());
        assert!(p.disabled.is_empty());
        assert!(p.settings.is_empty());
        assert_eq!(p.registry_url, "");
        assert!(p.enforce_hooks);
    }

    #[test]
    fn test_branding_config_defaults() {
        let b = BrandingConfig::default();
        assert!(b.enabled);
        assert_eq!(b.template, "builtin:branding");
        assert_eq!(b.duration, 5.0);
    }

    #[test]
    fn test_orchestration_config_defaults() {
        let o = OrchestrationConfig::default();
        assert_eq!(o.upload_bitrate_kbps, 0);
        assert!(o.sequential);
    }

    #[test]
    fn test_app_config_defaults() {
        let c = AppConfig::default();
        assert_eq!(c.config_version, 1);
        assert_eq!(c.sport, "generic");
        assert!(c.render_profiles.is_empty());
    }

    #[test]
    fn test_iteration_config_profiles_for_event_exact() {
        let mut mappings = HashMap::new();
        mappings.insert("goal".to_string(), vec!["vertical".to_string()]);
        mappings.insert("default".to_string(), vec!["standard".to_string()]);
        let ic = IterationConfig { mappings };
        assert_eq!(ic.profiles_for_event("goal"), vec!["vertical".to_string()]);
    }

    #[test]
    fn test_iteration_config_profiles_for_event_fallback() {
        let mut mappings = HashMap::new();
        mappings.insert("default".to_string(), vec!["standard".to_string()]);
        let ic = IterationConfig { mappings };
        assert_eq!(
            ic.profiles_for_event("unknown"),
            vec!["standard".to_string()]
        );
    }

    #[test]
    fn test_iteration_config_profiles_for_event_empty() {
        let ic = IterationConfig::default();
        assert!(ic.profiles_for_event("anything").is_empty());
    }

    #[test]
    fn test_speed_segment_serialize() {
        let seg = SpeedSegment {
            speed: 2.0,
            until: Some(10.0),
        };
        let json = serde_json::to_value(&seg).unwrap();
        assert_eq!(json["speed"], 2.0);
        assert_eq!(json["until"], 10.0);
    }

    #[test]
    fn test_speed_segment_without_until() {
        let seg = SpeedSegment {
            speed: 1.5,
            until: None,
        };
        let json = serde_json::to_value(&seg).unwrap();
        assert_eq!(json["speed"], 1.5);
        assert!(json.get("until").is_none());
    }

    #[test]
    fn test_render_profile_minimal_serialize() {
        let rp = RenderProfile {
            name: "test".to_string(),
            width: None,
            height: None,
            crop_mode: None,
            anchor_x: None,
            anchor_y: None,
            pad_color: None,
            scale: None,
            smart: None,
            speed: None,
            speed_segments: None,
            lut: None,
            subtitle_template: None,
            codec: None,
            preset: None,
            crf: None,
            audio_codec: None,
            audio_bitrate: None,
        };
        let json = serde_json::to_value(&rp).unwrap();
        assert_eq!(json["name"], "test");
        // Optional fields should be absent
        assert!(json.get("width").is_none());
        assert!(json.get("codec").is_none());
    }

    #[test]
    fn test_render_profile_full_roundtrip() {
        let rp = RenderProfile {
            name: "full".to_string(),
            width: Some(1920),
            height: Some(1080),
            crop_mode: Some("center".to_string()),
            anchor_x: Some(0.5),
            anchor_y: Some(0.5),
            pad_color: Some("#000000".to_string()),
            scale: Some(1.0),
            smart: Some(true),
            speed: Some(1.5),
            speed_segments: Some(vec![SpeedSegment {
                speed: 2.0,
                until: Some(5.0),
            }]),
            lut: Some("cinematic.cube".to_string()),
            subtitle_template: Some("{event}".to_string()),
            codec: Some("libx265".to_string()),
            preset: Some("slow".to_string()),
            crf: Some(20),
            audio_codec: Some("opus".to_string()),
            audio_bitrate: Some("192k".to_string()),
        };
        let json_str = serde_json::to_string(&rp).unwrap();
        let deserialized: RenderProfile = serde_json::from_str(&json_str).unwrap();
        assert_eq!(rp, deserialized);
    }

    #[test]
    fn test_app_config_roundtrip() {
        let config = AppConfig::default();
        let json_str = serde_json::to_string_pretty(&config).unwrap();
        let deserialized: AppConfig = serde_json::from_str(&json_str).unwrap();
        assert_eq!(config, deserialized);
    }

    #[test]
    fn test_app_config_from_empty_json() {
        let config: AppConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(config.config_version, 1);
        assert_eq!(config.sport, "generic");
        assert_eq!(config.video.codec, "libx264");
    }

    #[test]
    fn test_app_config_partial_json() {
        let json = r#"{"sport": "hockey", "video": {"crf": 22}}"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.sport, "hockey");
        assert_eq!(config.video.crf, 22);
        // Other fields still get defaults
        assert_eq!(config.video.codec, "libx264");
        assert_eq!(config.video.preset, "medium");
    }

    #[test]
    fn test_plugins_config_with_settings() {
        let json = r#"{
            "enabled": ["plugin-a"],
            "settings": {"plugin-a": {"key": "value"}},
            "enforce_hooks": false
        }"#;
        let p: PluginsConfig = serde_json::from_str(json).unwrap();
        assert_eq!(p.enabled, vec!["plugin-a"]);
        assert!(!p.enforce_hooks);
        assert!(p.settings.contains_key("plugin-a"));
    }

    #[test]
    fn test_path_config_with_paths() {
        let json = r#"{
            "source_dir": "/tmp/replays",
            "output_dir": "/tmp/output",
            "temp_dir": "/tmp/work"
        }"#;
        let p: PathConfig = serde_json::from_str(json).unwrap();
        assert_eq!(p.source_dir, Some(PathBuf::from("/tmp/replays")));
        assert_eq!(p.output_dir, Some(PathBuf::from("/tmp/output")));
        assert_eq!(p.temp_dir, Some(PathBuf::from("/tmp/work")));
    }

    #[test]
    fn test_skip_serializing_if() {
        let config = AppConfig::default();
        let val = serde_json::to_value(&config).unwrap();
        // render_profiles is empty -> should be absent
        assert!(val.get("render_profiles").is_none());
        // orchestration.upload_bitrate_kbps is 0 -> should be absent
        assert!(val["orchestration"].get("upload_bitrate_kbps").is_none());
    }

    #[test]
    fn test_is_zero_u32_helper() {
        assert!(is_zero_u32(&0));
        assert!(!is_zero_u32(&1));
    }

    #[test]
    fn test_event_type_entry_simple() {
        let entry = EventTypeEntry::Simple("goal".to_string());
        assert_eq!(entry.name(), "goal");
        assert!(!entry.team_specific());
    }

    #[test]
    fn test_event_type_entry_full() {
        let entry = EventTypeEntry::Full {
            name: "goal".to_string(),
            team_specific: true,
        };
        assert_eq!(entry.name(), "goal");
        assert!(entry.team_specific());
    }

    #[test]
    fn test_event_type_entry_full_default_team_specific() {
        let entry: EventTypeEntry =
            serde_json::from_str(r#"{"name": "save"}"#).unwrap();
        assert_eq!(entry.name(), "save");
        assert!(!entry.team_specific());
    }

    #[test]
    fn test_event_type_entry_simple_from_string_json() {
        let entry: EventTypeEntry = serde_json::from_str(r#""goal""#).unwrap();
        assert_eq!(entry.name(), "goal");
        assert!(!entry.team_specific());
    }

    #[test]
    fn test_event_type_entry_full_roundtrip() {
        let entry = EventTypeEntry::Full {
            name: "goal".to_string(),
            team_specific: true,
        };
        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: EventTypeEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(entry, deserialized);
    }

    #[test]
    fn test_app_config_event_types_roundtrip_full() {
        let config = AppConfig {
            event_types: vec![
                EventTypeEntry::Full {
                    name: "goal".to_string(),
                    team_specific: true,
                },
                EventTypeEntry::Full {
                    name: "timeout".to_string(),
                    team_specific: false,
                },
            ],
            ..AppConfig::default()
        };
        let json_str = serde_json::to_string_pretty(&config).unwrap();
        let deserialized: AppConfig = serde_json::from_str(&json_str).unwrap();
        assert_eq!(config.event_types, deserialized.event_types);
    }

    #[test]
    fn test_app_config_event_types_skip_serializing_when_empty() {
        let config = AppConfig::default();
        let val = serde_json::to_value(&config).unwrap();
        assert!(val.get("event_types").is_none());
    }

    #[test]
    fn test_app_config_event_types_from_simple_strings() {
        let json = r#"{"event_types": ["goal", "assist"]}"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.event_types.len(), 2);
        assert_eq!(config.event_types[0].name(), "goal");
        assert!(!config.event_types[0].team_specific());
    }

    #[test]
    fn test_app_config_event_types_from_full_objects() {
        let json = r#"{"event_types": [{"name": "goal", "team_specific": true}, {"name": "timeout"}]}"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.event_types.len(), 2);
        assert_eq!(config.event_types[0].name(), "goal");
        assert!(config.event_types[0].team_specific());
        assert_eq!(config.event_types[1].name(), "timeout");
        assert!(!config.event_types[1].team_specific());
    }

    #[test]
    fn test_app_config_event_types_mixed_format() {
        let json = r#"{"event_types": ["clip", {"name": "goal", "team_specific": true}]}"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.event_types.len(), 2);
        assert_eq!(config.event_types[0].name(), "clip");
        assert!(!config.event_types[0].team_specific());
        assert_eq!(config.event_types[1].name(), "goal");
        assert!(config.event_types[1].team_specific());
    }

    #[test]
    fn test_app_config_event_types_missing_defaults_empty() {
        let json = r#"{"sport": "hockey"}"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert!(config.event_types.is_empty());
    }
}
