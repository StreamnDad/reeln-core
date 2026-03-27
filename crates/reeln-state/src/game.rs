use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Video file extensions recognized by the system.
pub const VIDEO_EXTENSIONS: &[&str] = &[".mkv", ".mp4", ".mov", ".avi", ".webm", ".ts", ".flv"];

/// Core game information.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GameInfo {
    pub date: String,
    pub home_team: String,
    pub away_team: String,
    pub sport: String,
    #[serde(default = "default_game_number")]
    pub game_number: u32,
    #[serde(default)]
    pub venue: String,
    #[serde(default)]
    pub game_time: String,
    #[serde(default)]
    pub period_length: u32,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub thumbnail: String,
    #[serde(default)]
    pub level: String,
    #[serde(default)]
    pub home_slug: String,
    #[serde(default)]
    pub away_slug: String,
    #[serde(default)]
    pub tournament: String,
}

fn default_game_number() -> u32 {
    1
}

/// An event that occurred during a game (goal, penalty, etc.).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GameEvent {
    pub id: String,
    pub clip: String,
    pub segment_number: u32,
    #[serde(default)]
    pub event_type: String,
    #[serde(default)]
    pub player: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Record of a completed render.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RenderEntry {
    pub input: String,
    pub output: String,
    pub segment_number: u32,
    pub format: String,
    pub crop_mode: String,
    pub rendered_at: String,
    #[serde(default)]
    pub event_id: String,
}

/// Full game state, persisted as game.json.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GameState {
    pub game_info: GameInfo,
    #[serde(default)]
    pub segments_processed: Vec<u32>,
    #[serde(default)]
    pub highlighted: bool,
    #[serde(default)]
    pub finished: bool,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub finished_at: String,
    #[serde(default)]
    pub renders: Vec<RenderEntry>,
    #[serde(default)]
    pub events: Vec<GameEvent>,
    #[serde(default)]
    pub livestreams: HashMap<String, String>,
    #[serde(default)]
    pub segment_outputs: Vec<String>,
    #[serde(default)]
    pub highlights_output: String,
}
