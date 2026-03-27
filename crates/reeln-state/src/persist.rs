use std::path::{Path, PathBuf};

use crate::error::StateError;
use crate::game::GameState;

const GAME_JSON: &str = "game.json";

/// Load game state from `game.json` inside `game_dir`.
pub fn load_game_state(game_dir: &Path) -> Result<GameState, StateError> {
    let path = game_dir.join(GAME_JSON);
    let content = std::fs::read_to_string(&path)?;
    let state: GameState = serde_json::from_str(&content)?;
    Ok(state)
}

/// Save game state to `game.json` inside `game_dir` with atomic write
/// (write to temp file, then rename).
///
/// Returns the path to the written file.
pub fn save_game_state(state: &GameState, game_dir: &Path) -> Result<PathBuf, StateError> {
    let path = game_dir.join(GAME_JSON);

    let tmp = tempfile::NamedTempFile::new_in(game_dir)?;
    serde_json::to_writer_pretty(&tmp, state)?;
    tmp.persist(&path)
        .map_err(|e| StateError::Persist(format!("atomic rename failed: {e}")))?;

    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{GameEvent, GameInfo, GameState, RenderEntry};
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn make_full_state() -> GameState {
        let mut metadata = HashMap::new();
        metadata.insert(
            "speed".to_string(),
            serde_json::Value::String("fast".to_string()),
        );

        let mut livestreams = HashMap::new();
        livestreams.insert("youtube".to_string(), "https://yt.be/abc".to_string());

        GameState {
            game_info: GameInfo {
                date: "2026-02-26".to_string(),
                home_team: "Home Team".to_string(),
                away_team: "Away Team".to_string(),
                sport: "hockey".to_string(),
                game_number: 1,
                venue: "Arena".to_string(),
                game_time: "19:00".to_string(),
                period_length: 20,
                description: "Playoff game".to_string(),
                thumbnail: "https://example.com/thumb.jpg".to_string(),
                level: "AA".to_string(),
                home_slug: "home-team".to_string(),
                away_slug: "away-team".to_string(),
                tournament: "Playoffs".to_string(),
            },
            segments_processed: vec![1, 2, 3],
            highlighted: true,
            finished: false,
            created_at: "2026-02-26T19:00:00Z".to_string(),
            finished_at: String::new(),
            renders: vec![RenderEntry {
                input: "/in/video.mp4".to_string(),
                output: "/out/short.mp4".to_string(),
                segment_number: 1,
                format: "short".to_string(),
                crop_mode: "center".to_string(),
                rendered_at: "2026-02-26T20:00:00Z".to_string(),
                event_id: "abc123".to_string(),
            }],
            events: vec![GameEvent {
                id: "deadbeef".to_string(),
                clip: "clips/goal1.mp4".to_string(),
                segment_number: 2,
                event_type: "goal".to_string(),
                player: "Player One".to_string(),
                created_at: "2026-02-26T19:30:00Z".to_string(),
                metadata,
            }],
            livestreams,
            segment_outputs: vec![
                "segments/period-1.mp4".to_string(),
                "segments/period-2.mp4".to_string(),
            ],
            highlights_output: "highlights.mp4".to_string(),
        }
    }

    #[test]
    fn test_round_trip() {
        let tmp = TempDir::new().unwrap();
        let state = make_full_state();

        let path = save_game_state(&state, tmp.path()).unwrap();
        assert!(path.exists());
        assert_eq!(path.file_name().unwrap(), "game.json");

        let loaded = load_game_state(tmp.path()).unwrap();
        assert_eq!(loaded, state);
    }

    #[test]
    fn test_round_trip_all_fields() {
        let tmp = TempDir::new().unwrap();
        let state = make_full_state();

        save_game_state(&state, tmp.path()).unwrap();
        let loaded = load_game_state(tmp.path()).unwrap();

        // Verify all GameInfo fields
        assert_eq!(loaded.game_info.date, "2026-02-26");
        assert_eq!(loaded.game_info.home_team, "Home Team");
        assert_eq!(loaded.game_info.away_team, "Away Team");
        assert_eq!(loaded.game_info.sport, "hockey");
        assert_eq!(loaded.game_info.game_number, 1);
        assert_eq!(loaded.game_info.venue, "Arena");
        assert_eq!(loaded.game_info.game_time, "19:00");
        assert_eq!(loaded.game_info.period_length, 20);
        assert_eq!(loaded.game_info.description, "Playoff game");
        assert_eq!(loaded.game_info.thumbnail, "https://example.com/thumb.jpg");
        assert_eq!(loaded.game_info.level, "AA");
        assert_eq!(loaded.game_info.home_slug, "home-team");
        assert_eq!(loaded.game_info.away_slug, "away-team");
        assert_eq!(loaded.game_info.tournament, "Playoffs");

        // Verify all GameState fields
        assert_eq!(loaded.segments_processed, vec![1, 2, 3]);
        assert!(loaded.highlighted);
        assert!(!loaded.finished);
        assert_eq!(loaded.created_at, "2026-02-26T19:00:00Z");
        assert_eq!(loaded.finished_at, "");
        assert_eq!(loaded.segment_outputs.len(), 2);
        assert_eq!(loaded.highlights_output, "highlights.mp4");

        // Verify events
        assert_eq!(loaded.events.len(), 1);
        assert_eq!(loaded.events[0].id, "deadbeef");
        assert_eq!(loaded.events[0].clip, "clips/goal1.mp4");
        assert_eq!(loaded.events[0].segment_number, 2);
        assert_eq!(loaded.events[0].event_type, "goal");
        assert_eq!(loaded.events[0].player, "Player One");
        assert_eq!(loaded.events[0].created_at, "2026-02-26T19:30:00Z");
        assert_eq!(
            loaded.events[0].metadata.get("speed").unwrap(),
            &serde_json::Value::String("fast".to_string())
        );

        // Verify renders
        assert_eq!(loaded.renders.len(), 1);
        assert_eq!(loaded.renders[0].input, "/in/video.mp4");
        assert_eq!(loaded.renders[0].output, "/out/short.mp4");
        assert_eq!(loaded.renders[0].segment_number, 1);
        assert_eq!(loaded.renders[0].format, "short");
        assert_eq!(loaded.renders[0].crop_mode, "center");
        assert_eq!(loaded.renders[0].rendered_at, "2026-02-26T20:00:00Z");
        assert_eq!(loaded.renders[0].event_id, "abc123");

        // Verify livestreams
        assert_eq!(
            loaded.livestreams.get("youtube").unwrap(),
            "https://yt.be/abc"
        );
    }

    #[test]
    fn test_load_nonexistent() {
        let tmp = TempDir::new().unwrap();
        let result = load_game_state(tmp.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_load_invalid_json() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("game.json"), "not json").unwrap();
        let result = load_game_state(tmp.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_atomic_write_creates_file() {
        let tmp = TempDir::new().unwrap();
        let state = make_full_state();

        // Verify no game.json before write
        assert!(!tmp.path().join("game.json").exists());

        save_game_state(&state, tmp.path()).unwrap();

        // Verify game.json exists after write
        assert!(tmp.path().join("game.json").exists());
    }

    #[test]
    fn test_save_overwrites_existing() {
        let tmp = TempDir::new().unwrap();
        let mut state = make_full_state();
        save_game_state(&state, tmp.path()).unwrap();

        state.finished = true;
        state.finished_at = "2026-02-26T21:00:00Z".to_string();
        save_game_state(&state, tmp.path()).unwrap();

        let loaded = load_game_state(tmp.path()).unwrap();
        assert!(loaded.finished);
        assert_eq!(loaded.finished_at, "2026-02-26T21:00:00Z");
    }

    #[test]
    fn test_defaults_on_minimal_json() {
        let tmp = TempDir::new().unwrap();
        let minimal = r#"{
            "game_info": {
                "date": "2026-02-26",
                "home_team": "Home",
                "away_team": "Away",
                "sport": "hockey"
            }
        }"#;
        std::fs::write(tmp.path().join("game.json"), minimal).unwrap();
        let loaded = load_game_state(tmp.path()).unwrap();
        assert_eq!(loaded.game_info.game_number, 1);
        assert_eq!(loaded.game_info.venue, "");
        assert_eq!(loaded.game_info.game_time, "");
        assert_eq!(loaded.game_info.period_length, 0);
        assert!(!loaded.finished);
        assert!(!loaded.highlighted);
        assert!(loaded.segments_processed.is_empty());
        assert!(loaded.events.is_empty());
        assert!(loaded.renders.is_empty());
        assert!(loaded.livestreams.is_empty());
        assert!(loaded.segment_outputs.is_empty());
        assert_eq!(loaded.highlights_output, "");
        assert_eq!(loaded.created_at, "");
        assert_eq!(loaded.finished_at, "");
    }
}
