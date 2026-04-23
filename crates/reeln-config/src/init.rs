//! Guided first-time config creation.
//!
//! Provides functions for both CLI (`reeln init`) and dock (SetupWizard)
//! to create a working config from user choices.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::error::ConfigError;
use crate::model::{AppConfig, EventTypeEntry, IterationConfig, PathConfig, RenderProfile};

/// User choices for creating an initial config.
pub struct InitOptions {
    /// Sport name (e.g., "hockey", "basketball", "generic").
    pub sport: String,
    /// Directory where streaming software saves replay files.
    pub source_dir: PathBuf,
    /// Directory where game folders will be created.
    pub output_dir: PathBuf,
    /// Config file path. None = default XDG path.
    pub config_path: Option<PathBuf>,
    /// Whether to create source_dir and output_dir if they don't exist.
    pub create_dirs: bool,
}

/// Information about an available sport.
#[derive(Debug, Clone)]
pub struct SportInfo {
    /// Canonical sport name (e.g., "hockey").
    pub name: String,
    /// Segment label (e.g., "period", "quarter").
    pub segment_name: String,
    /// Expected number of segments per game.
    pub segment_count: u32,
    /// Optional segment duration in minutes.
    pub duration_minutes: Option<u32>,
    /// Default event types for this sport with team-specific flags.
    pub default_event_types: Vec<EventTypeEntry>,
}

/// List all available sports with their segment info and default event types.
pub fn list_available_sports() -> Vec<SportInfo> {
    let registry = reeln_sport::SportRegistry::default();
    registry
        .list_sports()
        .into_iter()
        .map(|alias| {
            let event_entries = reeln_sport::default_event_type_entries(&alias.sport)
                .into_iter()
                .map(|(name, team_specific)| {
                    if team_specific {
                        EventTypeEntry::Full {
                            name,
                            team_specific,
                        }
                    } else {
                        EventTypeEntry::Simple(name)
                    }
                })
                .collect();

            SportInfo {
                name: alias.sport.clone(),
                segment_name: alias.segment_name.clone(),
                segment_count: alias.segment_count,
                duration_minutes: alias.duration_minutes,
                default_event_types: event_entries,
            }
        })
        .collect()
}

/// Build an `AppConfig` from user choices.
///
/// Sets sport, paths, event types (from sport defaults), and includes
/// the bundled `player-overlay` render profile + `goal` iteration mapping
/// for CLI parity with the Python `default_config()`.
pub fn build_initial_config(options: &InitOptions) -> AppConfig {
    let event_types = reeln_sport::default_event_type_entries(&options.sport)
        .into_iter()
        .map(|(name, team_specific)| {
            if team_specific {
                EventTypeEntry::Full {
                    name,
                    team_specific,
                }
            } else {
                EventTypeEntry::Simple(name)
            }
        })
        .collect();

    // Bundled render profile (matches Python default_config)
    let mut render_profiles = HashMap::new();
    render_profiles.insert(
        "player-overlay".to_string(),
        RenderProfile {
            name: "player-overlay".to_string(),
            subtitle_template: Some("builtin:goal_overlay".to_string()),
            ..Default::default()
        },
    );

    // Bundled iteration mapping (matches Python default_config)
    let mut mappings = HashMap::new();
    mappings.insert("goal".to_string(), vec!["player-overlay".to_string()]);

    AppConfig {
        sport: options.sport.clone(),
        paths: PathConfig {
            source_dir: Some(options.source_dir.clone()),
            output_dir: Some(options.output_dir.clone()),
            ..PathConfig::default()
        },
        event_types,
        render_profiles,
        iterations: IterationConfig { mappings },
        ..AppConfig::default()
    }
}

/// Create an initial config file from user choices.
///
/// Resolves the config path, optionally creates directories, builds the
/// config, and writes it atomically. Returns the path where the config
/// was saved.
pub fn create_initial_config(options: &InitOptions) -> Result<PathBuf, ConfigError> {
    let config_path = options
        .config_path
        .clone()
        .unwrap_or_else(|| crate::default_config_path(None));

    // Check if config already exists
    if config_path.is_file() {
        return Err(ConfigError::AlreadyExists(
            config_path.display().to_string(),
        ));
    }

    // Create directories if requested
    if options.create_dirs {
        std::fs::create_dir_all(&options.source_dir)?;
        std::fs::create_dir_all(&options.output_dir)?;
    }

    let config = build_initial_config(options);
    crate::save_config(&config, &config_path)
}

/// Check whether a config file already exists at the given or default path.
pub fn config_exists(path: Option<&Path>) -> bool {
    let resolved = path
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::default_config_path(None));
    resolved.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_available_sports_includes_builtin() {
        let sports = list_available_sports();
        let names: Vec<&str> = sports.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"hockey"));
        assert!(names.contains(&"basketball"));
        assert!(names.contains(&"soccer"));
        assert!(names.contains(&"football"));
        assert!(names.contains(&"baseball"));
        assert!(names.contains(&"lacrosse"));
        assert!(names.contains(&"generic"));
    }

    #[test]
    fn list_available_sports_hockey_has_event_types() {
        let sports = list_available_sports();
        let hockey = sports.iter().find(|s| s.name == "hockey").unwrap();
        assert_eq!(hockey.segment_name, "period");
        assert_eq!(hockey.segment_count, 3);
        let names: Vec<&str> = hockey
            .default_event_types
            .iter()
            .map(|e| e.name())
            .collect();
        assert!(names.contains(&"goal"));
        assert!(names.contains(&"save"));
    }

    #[test]
    fn list_available_sports_generic_has_no_events() {
        let sports = list_available_sports();
        let generic = sports.iter().find(|s| s.name == "generic").unwrap();
        assert!(generic.default_event_types.is_empty());
    }

    #[test]
    fn build_initial_config_sets_sport_and_paths() {
        let opts = InitOptions {
            sport: "hockey".to_string(),
            source_dir: PathBuf::from("/tmp/replays"),
            output_dir: PathBuf::from("/tmp/games"),
            config_path: None,
            create_dirs: false,
        };
        let config = build_initial_config(&opts);
        assert_eq!(config.sport, "hockey");
        assert_eq!(config.paths.source_dir, Some(PathBuf::from("/tmp/replays")));
        assert_eq!(config.paths.output_dir, Some(PathBuf::from("/tmp/games")));
    }

    #[test]
    fn build_initial_config_populates_event_types() {
        let opts = InitOptions {
            sport: "hockey".to_string(),
            source_dir: PathBuf::from("/tmp/src"),
            output_dir: PathBuf::from("/tmp/out"),
            config_path: None,
            create_dirs: false,
        };
        let config = build_initial_config(&opts);
        assert!(!config.event_types.is_empty());
        // goal should be team_specific
        let goal = config
            .event_types
            .iter()
            .find(|e| e.name() == "goal")
            .unwrap();
        assert!(goal.team_specific());
    }

    #[test]
    fn build_initial_config_generic_has_no_events() {
        let opts = InitOptions {
            sport: "generic".to_string(),
            source_dir: PathBuf::from("/tmp/src"),
            output_dir: PathBuf::from("/tmp/out"),
            config_path: None,
            create_dirs: false,
        };
        let config = build_initial_config(&opts);
        assert!(config.event_types.is_empty());
    }

    #[test]
    fn build_initial_config_includes_bundled_profile() {
        let opts = InitOptions {
            sport: "hockey".to_string(),
            source_dir: PathBuf::from("/tmp/src"),
            output_dir: PathBuf::from("/tmp/out"),
            config_path: None,
            create_dirs: false,
        };
        let config = build_initial_config(&opts);
        assert!(config.render_profiles.contains_key("player-overlay"));
        let profile = &config.render_profiles["player-overlay"];
        assert_eq!(
            profile.subtitle_template,
            Some("builtin:goal_overlay".to_string())
        );
    }

    #[test]
    fn build_initial_config_includes_goal_iteration() {
        let opts = InitOptions {
            sport: "hockey".to_string(),
            source_dir: PathBuf::from("/tmp/src"),
            output_dir: PathBuf::from("/tmp/out"),
            config_path: None,
            create_dirs: false,
        };
        let config = build_initial_config(&opts);
        assert_eq!(
            config.iterations.mappings.get("goal"),
            Some(&vec!["player-overlay".to_string()])
        );
    }

    #[test]
    fn create_initial_config_writes_loadable_file() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        let source = dir.path().join("replays");
        let output = dir.path().join("games");

        let opts = InitOptions {
            sport: "basketball".to_string(),
            source_dir: source.clone(),
            output_dir: output.clone(),
            config_path: Some(config_path.clone()),
            create_dirs: true,
        };

        let result = create_initial_config(&opts);
        assert!(result.is_ok(), "create failed: {:?}", result.err());
        assert_eq!(result.unwrap(), config_path);

        // Load it back and verify
        let loaded = crate::load_config(&config_path, None).unwrap();
        assert_eq!(loaded.sport, "basketball");
        assert_eq!(loaded.paths.source_dir, Some(source));
        assert_eq!(loaded.paths.output_dir, Some(output));
    }

    #[test]
    fn create_initial_config_creates_directories() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("new_replays");
        let output = dir.path().join("new_games");

        assert!(!source.exists());
        assert!(!output.exists());

        let opts = InitOptions {
            sport: "generic".to_string(),
            source_dir: source.clone(),
            output_dir: output.clone(),
            config_path: Some(dir.path().join("config.json")),
            create_dirs: true,
        };

        create_initial_config(&opts).unwrap();
        assert!(source.is_dir());
        assert!(output.is_dir());
    }

    #[test]
    fn create_initial_config_returns_already_exists() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");

        // Create the file first
        std::fs::write(&config_path, "{}").unwrap();

        let opts = InitOptions {
            sport: "hockey".to_string(),
            source_dir: dir.path().join("src"),
            output_dir: dir.path().join("out"),
            config_path: Some(config_path),
            create_dirs: false,
        };

        let result = create_initial_config(&opts);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("already exists"), "unexpected error: {err}");
    }

    #[test]
    fn config_exists_true_when_file_present() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, "{}").unwrap();
        assert!(config_exists(Some(&path)));
    }

    #[test]
    fn config_exists_false_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");
        assert!(!config_exists(Some(&path)));
    }

    #[test]
    fn roundtrip_create_then_load_matches() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");

        let opts = InitOptions {
            sport: "soccer".to_string(),
            source_dir: PathBuf::from("/videos/replays"),
            output_dir: PathBuf::from("/videos/games"),
            config_path: Some(config_path.clone()),
            create_dirs: false,
        };

        let built = build_initial_config(&opts);
        create_initial_config(&opts).unwrap();

        let loaded = crate::load_config(&config_path, None).unwrap();
        assert_eq!(built.sport, loaded.sport);
        assert_eq!(built.paths.source_dir, loaded.paths.source_dir);
        assert_eq!(built.paths.output_dir, loaded.paths.output_dir);
        assert_eq!(built.event_types.len(), loaded.event_types.len());
    }
}
