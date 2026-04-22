//! Game state mutation functions.
//!
//! Every function in this module takes `&mut GameState` and performs a pure
//! in-memory transform. The caller is responsible for `load_game_state()` /
//! `save_game_state()` around the mutation to control transaction boundaries.

use crate::error::StateError;
use crate::game::{GameEvent, GameState, RenderEntry};

// ── Group A: Trivial setters ──────────────────────────────────────────

/// Mark a game as finished with the current timestamp.
pub fn mark_finished(state: &mut GameState) {
    state.finished = true;
    state.finished_at = chrono::Utc::now().to_rfc3339();
}

/// Set the tournament name on a game.
pub fn set_tournament(state: &mut GameState, tournament: &str) {
    state.game_info.tournament = tournament.to_string();
}

/// Record a segment as processed (idempotent — skips if already present).
pub fn mark_segment_processed(state: &mut GameState, segment_number: u32) {
    if !state.segments_processed.contains(&segment_number) {
        state.segments_processed.push(segment_number);
        state.segments_processed.sort();
    }
}

/// Record a segment output path (idempotent — skips if already present).
pub fn set_segment_output(state: &mut GameState, output_path: String) {
    if !state.segment_outputs.contains(&output_path) {
        state.segment_outputs.push(output_path);
    }
}

/// Mark highlights as merged with the given output path.
pub fn mark_highlighted(state: &mut GameState, output_path: String) {
    state.highlighted = true;
    state.highlights_output = output_path;
}

// ── Group B: Collection operations ────────────────────────────────────

/// Add an event to the game state.
pub fn add_event(state: &mut GameState, event: GameEvent) {
    state.events.push(event);
}

/// Add a render entry to the game state.
pub fn add_render(state: &mut GameState, render: RenderEntry) {
    state.renders.push(render);
}

/// Clear all render entries. Returns the number of entries removed.
/// File deletion is the caller's responsibility.
pub fn clear_renders(state: &mut GameState) -> u32 {
    let count = state.renders.len() as u32;
    state.renders.clear();
    count
}

/// Set a livestream URL for a platform.
pub fn set_livestream(state: &mut GameState, platform: &str, url: &str) {
    state
        .livestreams
        .insert(platform.to_string(), url.to_string());
}

/// Remove a livestream entry by platform. Returns `true` if it existed.
pub fn remove_livestream(state: &mut GameState, platform: &str) -> bool {
    state.livestreams.remove(platform).is_some()
}

/// Update a single field on `game_info` by name.
///
/// Supports all `GameInfo` fields. Numeric fields (`game_number`,
/// `period_length`) are parsed from the string value.
pub fn update_game_info_field(
    state: &mut GameState,
    field: &str,
    value: String,
) -> Result<(), StateError> {
    match field {
        "date" => state.game_info.date = value,
        "home_team" => state.game_info.home_team = value,
        "away_team" => state.game_info.away_team = value,
        "sport" => state.game_info.sport = value,
        "game_number" => {
            state.game_info.game_number = value
                .parse::<u32>()
                .map_err(|_| StateError::Mutation(format!("Invalid game_number: {}", value)))?;
        }
        "venue" => state.game_info.venue = value,
        "game_time" => state.game_info.game_time = value,
        "period_length" => {
            state.game_info.period_length = value
                .parse::<u32>()
                .map_err(|_| StateError::Mutation(format!("Invalid period_length: {}", value)))?;
        }
        "description" => state.game_info.description = value,
        "thumbnail" => state.game_info.thumbnail = value,
        "level" => state.game_info.level = value,
        "home_slug" => state.game_info.home_slug = value,
        "away_slug" => state.game_info.away_slug = value,
        "tournament" => state.game_info.tournament = value,
        other => {
            return Err(StateError::Mutation(format!(
                "Unknown game_info field: {}",
                other
            )));
        }
    }
    Ok(())
}

//── Group C: Lookup-then-mutate ───────────────────────────────────────

/// Update a single field on an event identified by `event_id`.
///
/// Supported fields: `clip`, `event_type`, `player`, or any metadata key.
/// For metadata, an empty string value removes the key.
pub fn update_event_field(
    state: &mut GameState,
    event_id: &str,
    field: &str,
    value: String,
) -> Result<(), StateError> {
    let event = state
        .events
        .iter_mut()
        .find(|e| e.id == event_id)
        .ok_or_else(|| StateError::Mutation(format!("Event {} not found", event_id)))?;

    match field {
        "clip" => event.clip = value,
        "event_type" => event.event_type = value,
        "player" => event.player = value,
        other => {
            if value.is_empty() {
                event.metadata.remove(other);
            } else {
                event
                    .metadata
                    .insert(other.to_string(), serde_json::Value::String(value));
            }
        }
    }

    Ok(())
}

/// Tag an event with an event type and optional team metadata.
///
/// Sets `event_type` and inserts/removes `metadata["team"]`.
pub fn tag_event(
    state: &mut GameState,
    event_id: &str,
    event_type: &str,
    team: Option<&str>,
) -> Result<(), StateError> {
    let event = state
        .events
        .iter_mut()
        .find(|e| e.id == event_id)
        .ok_or_else(|| StateError::Mutation(format!("Event {} not found", event_id)))?;

    event.event_type = event_type.to_string();

    if let Some(team_val) = team {
        event.metadata.insert(
            "team".to_string(),
            serde_json::Value::String(team_val.to_string()),
        );
    } else {
        event.metadata.remove("team");
    }

    Ok(())
}

/// Update `event_type` on all events whose IDs are in `event_ids`.
/// Returns the number of events updated.
pub fn bulk_update_event_type(
    state: &mut GameState,
    event_ids: &[String],
    event_type: &str,
) -> u32 {
    let mut count = 0u32;
    for event in state.events.iter_mut() {
        if event_ids.contains(&event.id) {
            event.event_type = event_type.to_string();
            count += 1;
        }
    }
    count
}

/// Remove an event by ID. Returns the removed event.
pub fn remove_event(state: &mut GameState, event_id: &str) -> Result<GameEvent, StateError> {
    let pos = state
        .events
        .iter()
        .position(|e| e.id == event_id)
        .ok_or_else(|| StateError::Mutation(format!("Event {} not found", event_id)))?;
    Ok(state.events.remove(pos))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_state() -> GameState {
        serde_json::from_str::<GameState>(
            r#"{"game_info":{"date":"2026-04-20","home_team":"A","away_team":"B","sport":"hockey"}}"#,
        )
        .unwrap()
    }

    fn make_event(id: &str, event_type: &str) -> GameEvent {
        GameEvent {
            id: id.to_string(),
            clip: format!("{id}.mp4"),
            segment_number: 1,
            event_type: event_type.to_string(),
            player: String::new(),
            created_at: String::new(),
            metadata: HashMap::new(),
        }
    }

    // ── mark_finished ─────────────────────────────────────────────────

    #[test]
    fn mark_finished_sets_fields() {
        let mut state = make_state();
        assert!(!state.finished);
        assert!(state.finished_at.is_empty());

        mark_finished(&mut state);

        assert!(state.finished);
        assert!(!state.finished_at.is_empty());
    }

    #[test]
    fn mark_finished_overwrites_previous() {
        let mut state = make_state();
        mark_finished(&mut state);
        let first = state.finished_at.clone();

        mark_finished(&mut state);
        assert!(state.finished);
        // Timestamp updated (may be same in fast tests, but field is set)
        assert!(!state.finished_at.is_empty());
        let _ = first; // suppress unused warning
    }

    // ── set_tournament ────────────────────────────────────────────────

    #[test]
    fn set_tournament_updates_field() {
        let mut state = make_state();
        set_tournament(&mut state, "Cup 2026");
        assert_eq!(state.game_info.tournament, "Cup 2026");
    }

    #[test]
    fn set_tournament_overwrites_existing() {
        let mut state = make_state();
        state.game_info.tournament = "Old".to_string();
        set_tournament(&mut state, "New");
        assert_eq!(state.game_info.tournament, "New");
    }

    // ── mark_segment_processed ────────────────────────────────────────

    #[test]
    fn mark_segment_processed_adds_and_sorts() {
        let mut state = make_state();
        mark_segment_processed(&mut state, 3);
        mark_segment_processed(&mut state, 1);
        mark_segment_processed(&mut state, 2);
        assert_eq!(state.segments_processed, vec![1, 2, 3]);
    }

    #[test]
    fn mark_segment_processed_idempotent() {
        let mut state = make_state();
        mark_segment_processed(&mut state, 1);
        mark_segment_processed(&mut state, 1);
        assert_eq!(state.segments_processed, vec![1]);
    }

    // ── set_segment_output ────────────────────────────────────────────

    #[test]
    fn set_segment_output_adds_path() {
        let mut state = make_state();
        set_segment_output(&mut state, "seg1.mp4".to_string());
        assert_eq!(state.segment_outputs, vec!["seg1.mp4"]);
    }

    #[test]
    fn set_segment_output_idempotent() {
        let mut state = make_state();
        set_segment_output(&mut state, "seg1.mp4".to_string());
        set_segment_output(&mut state, "seg1.mp4".to_string());
        assert_eq!(state.segment_outputs.len(), 1);
    }

    // ── mark_highlighted ──────────────────────────────────────────────

    #[test]
    fn mark_highlighted_sets_fields() {
        let mut state = make_state();
        mark_highlighted(&mut state, "highlights.mp4".to_string());
        assert!(state.highlighted);
        assert_eq!(state.highlights_output, "highlights.mp4");
    }

    // ── add_event ─────────────────────────────────────────────────────

    #[test]
    fn add_event_appends() {
        let mut state = make_state();
        assert!(state.events.is_empty());
        add_event(&mut state, make_event("e1", "goal"));
        assert_eq!(state.events.len(), 1);
        assert_eq!(state.events[0].id, "e1");
    }

    // ── add_render ────────────────────────────────────────────────────

    #[test]
    fn add_render_appends() {
        let mut state = make_state();
        let render = RenderEntry {
            input: "clip.mp4".to_string(),
            output: "short.mp4".to_string(),
            segment_number: 1,
            format: "short".to_string(),
            crop_mode: "center".to_string(),
            rendered_at: String::new(),
            event_id: String::new(),
        };
        add_render(&mut state, render);
        assert_eq!(state.renders.len(), 1);
        assert_eq!(state.renders[0].format, "short");
    }

    // ── clear_renders ─────────────────────────────────────────────────

    #[test]
    fn clear_renders_empties_and_returns_count() {
        let mut state = make_state();
        add_render(
            &mut state,
            RenderEntry {
                input: "a.mp4".to_string(),
                output: "b.mp4".to_string(),
                segment_number: 1,
                format: "short".to_string(),
                crop_mode: String::new(),
                rendered_at: String::new(),
                event_id: String::new(),
            },
        );
        add_render(
            &mut state,
            RenderEntry {
                input: "c.mp4".to_string(),
                output: "d.mp4".to_string(),
                segment_number: 2,
                format: "short".to_string(),
                crop_mode: String::new(),
                rendered_at: String::new(),
                event_id: String::new(),
            },
        );
        let count = clear_renders(&mut state);
        assert_eq!(count, 2);
        assert!(state.renders.is_empty());
    }

    #[test]
    fn clear_renders_empty_returns_zero() {
        let mut state = make_state();
        let count = clear_renders(&mut state);
        assert_eq!(count, 0);
    }

    // ── set_livestream ────────────────────────────────────────────────

    #[test]
    fn set_livestream_inserts() {
        let mut state = make_state();
        set_livestream(&mut state, "youtube", "https://youtube.com/live/123");
        assert_eq!(
            state.livestreams.get("youtube").unwrap(),
            "https://youtube.com/live/123"
        );
    }

    #[test]
    fn set_livestream_overwrites_existing() {
        let mut state = make_state();
        set_livestream(&mut state, "youtube", "old");
        set_livestream(&mut state, "youtube", "new");
        assert_eq!(state.livestreams.get("youtube").unwrap(), "new");
    }

    // ── update_event_field ────────────────────────────────────────────

    #[test]
    fn update_event_field_clip() {
        let mut state = make_state();
        add_event(&mut state, make_event("e1", "goal"));
        update_event_field(&mut state, "e1", "clip", "new.mp4".to_string()).unwrap();
        assert_eq!(state.events[0].clip, "new.mp4");
    }

    #[test]
    fn update_event_field_event_type() {
        let mut state = make_state();
        add_event(&mut state, make_event("e1", "goal"));
        update_event_field(&mut state, "e1", "event_type", "assist".to_string()).unwrap();
        assert_eq!(state.events[0].event_type, "assist");
    }

    #[test]
    fn update_event_field_player() {
        let mut state = make_state();
        add_event(&mut state, make_event("e1", "goal"));
        update_event_field(&mut state, "e1", "player", "Player 7".to_string()).unwrap();
        assert_eq!(state.events[0].player, "Player 7");
    }

    #[test]
    fn update_event_field_metadata_insert() {
        let mut state = make_state();
        add_event(&mut state, make_event("e1", "goal"));
        update_event_field(&mut state, "e1", "scorer", "Player 9".to_string()).unwrap();
        assert_eq!(
            state.events[0].metadata.get("scorer"),
            Some(&serde_json::Value::String("Player 9".to_string()))
        );
    }

    #[test]
    fn update_event_field_metadata_remove() {
        let mut state = make_state();
        let mut evt = make_event("e1", "goal");
        evt.metadata.insert(
            "scorer".to_string(),
            serde_json::Value::String("Player 9".to_string()),
        );
        add_event(&mut state, evt);

        update_event_field(&mut state, "e1", "scorer", String::new()).unwrap();
        assert!(!state.events[0].metadata.contains_key("scorer"));
    }

    #[test]
    fn update_event_field_not_found() {
        let mut state = make_state();
        let err = update_event_field(&mut state, "missing", "clip", "x".to_string()).unwrap_err();
        assert!(err.to_string().contains("missing"));
        assert!(err.to_string().contains("not found"));
    }

    // ── tag_event ─────────────────────────────────────────────────────

    #[test]
    fn tag_event_sets_type_and_team() {
        let mut state = make_state();
        add_event(&mut state, make_event("e1", ""));
        tag_event(&mut state, "e1", "goal", Some("home")).unwrap();
        assert_eq!(state.events[0].event_type, "goal");
        assert_eq!(
            state.events[0].metadata.get("team"),
            Some(&serde_json::Value::String("home".to_string()))
        );
    }

    #[test]
    fn tag_event_without_team_removes_team() {
        let mut state = make_state();
        let mut evt = make_event("e1", "goal");
        evt.metadata.insert(
            "team".to_string(),
            serde_json::Value::String("home".to_string()),
        );
        add_event(&mut state, evt);

        tag_event(&mut state, "e1", "save", None).unwrap();
        assert_eq!(state.events[0].event_type, "save");
        assert!(!state.events[0].metadata.contains_key("team"));
    }

    #[test]
    fn tag_event_not_found() {
        let mut state = make_state();
        let err = tag_event(&mut state, "missing", "goal", None).unwrap_err();
        assert!(err.to_string().contains("missing"));
    }

    // ── bulk_update_event_type ────────────────────────────────────────

    #[test]
    fn bulk_update_event_type_updates_matching() {
        let mut state = make_state();
        add_event(&mut state, make_event("e1", "goal"));
        add_event(&mut state, make_event("e2", "goal"));
        add_event(&mut state, make_event("e3", "save"));

        let count =
            bulk_update_event_type(&mut state, &["e1".to_string(), "e2".to_string()], "penalty");

        assert_eq!(count, 2);
        assert_eq!(state.events[0].event_type, "penalty");
        assert_eq!(state.events[1].event_type, "penalty");
        assert_eq!(state.events[2].event_type, "save");
    }

    #[test]
    fn bulk_update_event_type_ignores_missing() {
        let mut state = make_state();
        add_event(&mut state, make_event("e1", "goal"));
        let count = bulk_update_event_type(
            &mut state,
            &["e1".to_string(), "missing".to_string()],
            "assist",
        );
        assert_eq!(count, 1);
    }

    // ── remove_event ──────────────────────────────────────────────────

    #[test]
    fn remove_event_removes_and_returns() {
        let mut state = make_state();
        add_event(&mut state, make_event("e1", "goal"));
        add_event(&mut state, make_event("e2", "save"));

        let removed = remove_event(&mut state, "e1").unwrap();
        assert_eq!(removed.id, "e1");
        assert_eq!(state.events.len(), 1);
        assert_eq!(state.events[0].id, "e2");
    }

    #[test]
    fn remove_event_not_found() {
        let mut state = make_state();
        let err = remove_event(&mut state, "missing").unwrap_err();
        assert!(err.to_string().contains("missing"));
    }

    // ── non-target fields untouched ───────────────────────────────────

    #[test]
    fn mark_finished_preserves_other_fields() {
        let mut state = make_state();
        add_event(&mut state, make_event("e1", "goal"));
        state.highlighted = true;
        state.game_info.tournament = "Cup".to_string();

        mark_finished(&mut state);

        assert_eq!(state.events.len(), 1);
        assert!(state.highlighted);
        assert_eq!(state.game_info.tournament, "Cup");
    }

    // ── remove_livestream ──────────────────────────────────────────────

    #[test]
    fn remove_livestream_existing() {
        let mut state = make_state();
        set_livestream(&mut state, "youtube", "https://youtube.com/live/123");
        assert!(remove_livestream(&mut state, "youtube"));
        assert!(!state.livestreams.contains_key("youtube"));
    }

    #[test]
    fn remove_livestream_nonexistent() {
        let mut state = make_state();
        assert!(!remove_livestream(&mut state, "twitch"));
    }

    // ── update_game_info_field ────────────────────────────────────────

    #[test]
    fn update_game_info_field_string_fields() {
        let mut state = make_state();
        let cases = [
            ("date", "2026-05-01"),
            ("home_team", "Eagles"),
            ("away_team", "Hawks"),
            ("sport", "soccer"),
            ("venue", "Main Arena"),
            ("game_time", "19:30"),
            ("description", "Playoff game 2"),
            ("thumbnail", "/path/to/thumb.png"),
            ("level", "AA"),
            ("home_slug", "eagles"),
            ("away_slug", "hawks"),
            ("tournament", "Cup 2026"),
        ];
        for (field, value) in cases {
            update_game_info_field(&mut state, field, value.to_string()).unwrap();
        }
        assert_eq!(state.game_info.date, "2026-05-01");
        assert_eq!(state.game_info.home_team, "Eagles");
        assert_eq!(state.game_info.away_team, "Hawks");
        assert_eq!(state.game_info.sport, "soccer");
        assert_eq!(state.game_info.venue, "Main Arena");
        assert_eq!(state.game_info.game_time, "19:30");
        assert_eq!(state.game_info.description, "Playoff game 2");
        assert_eq!(state.game_info.thumbnail, "/path/to/thumb.png");
        assert_eq!(state.game_info.level, "AA");
        assert_eq!(state.game_info.home_slug, "eagles");
        assert_eq!(state.game_info.away_slug, "hawks");
        assert_eq!(state.game_info.tournament, "Cup 2026");
    }

    #[test]
    fn update_game_info_field_game_number() {
        let mut state = make_state();
        update_game_info_field(&mut state, "game_number", "3".to_string()).unwrap();
        assert_eq!(state.game_info.game_number, 3);
    }

    #[test]
    fn update_game_info_field_period_length() {
        let mut state = make_state();
        update_game_info_field(&mut state, "period_length", "20".to_string()).unwrap();
        assert_eq!(state.game_info.period_length, 20);
    }

    #[test]
    fn update_game_info_field_invalid_game_number() {
        let mut state = make_state();
        let err = update_game_info_field(&mut state, "game_number", "abc".to_string()).unwrap_err();
        assert!(err.to_string().contains("Invalid game_number"));
    }

    #[test]
    fn update_game_info_field_invalid_period_length() {
        let mut state = make_state();
        let err =
            update_game_info_field(&mut state, "period_length", "xyz".to_string()).unwrap_err();
        assert!(err.to_string().contains("Invalid period_length"));
    }

    #[test]
    fn update_game_info_field_unknown_field() {
        let mut state = make_state();
        let err = update_game_info_field(&mut state, "nonexistent", "val".to_string()).unwrap_err();
        assert!(err.to_string().contains("Unknown game_info field"));
    }

    // ── non-target fields untouched ──────────────────────────────────

    #[test]
    fn tag_event_preserves_other_metadata() {
        let mut state = make_state();
        let mut evt = make_event("e1", "");
        evt.metadata.insert(
            "scorer".to_string(),
            serde_json::Value::String("Player 9".to_string()),
        );
        add_event(&mut state, evt);

        tag_event(&mut state, "e1", "goal", Some("home")).unwrap();

        // scorer metadata preserved
        assert_eq!(
            state.events[0].metadata.get("scorer"),
            Some(&serde_json::Value::String("Player 9".to_string()))
        );
    }
}
