use std::path::{Path, PathBuf};

use crate::error::StateError;
use crate::game::GameState;

/// Generate a game directory name.
///
/// Format: `YYYY-MM-DD_{home}_vs_{away}`.
/// If `game_number > 1`, appends `_g2`, `_g3`, etc.
/// The caller is responsible for passing pre-formatted team names.
pub fn game_dir_name(date: &str, home: &str, away: &str, game_number: u32) -> String {
    let base = format!("{date}_{home}_vs_{away}");
    if game_number > 1 {
        format!("{base}_g{game_number}")
    } else {
        base
    }
}

/// Detect the next game number for a double-header.
///
/// Scans existing directories matching the date/home/away prefix.
/// Returns `1` if none exist, `2` if the base name exists (without `_gN`),
/// or `max(N) + 1` if `_gN` suffixed directories exist.
pub fn detect_next_game_number(base_dir: &Path, date: &str, home: &str, away: &str) -> u32 {
    let prefix = format!("{date}_{home}_vs_{away}");
    let mut game_number = 1u32;

    let entries = match std::fs::read_dir(base_dir) {
        Ok(e) => e,
        Err(_) => return 1,
    };

    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.as_ref() == prefix {
            // First game exists — next is at least 2
            game_number = game_number.max(2);
        } else if let Some(suffix) = name.strip_prefix(&format!("{prefix}_g")) {
            if let Ok(n) = suffix.parse::<u32>() {
                game_number = game_number.max(n + 1);
            }
        }
    }

    game_number
}

/// Create a game directory structure.
///
/// Creates the game directory under `base_dir`. The directory name is derived
/// from the game info. Returns the path to the created directory.
pub fn create_game_directory(
    base_dir: &Path,
    game_info: &crate::game::GameInfo,
) -> Result<PathBuf, StateError> {
    let dir_name = game_dir_name(
        &game_info.date,
        &game_info.home_team,
        &game_info.away_team,
        game_info.game_number,
    );
    let game_dir = base_dir.join(&dir_name);

    if game_dir.exists() {
        return Err(StateError::Directory(format!(
            "game directory already exists: {}",
            game_dir.display()
        )));
    }

    std::fs::create_dir_all(&game_dir)?;

    Ok(game_dir)
}

/// Find all unfinished games under `base_dir`.
///
/// Scans for `game.json` files where `finished` is `false`.
/// Returns the paths to the game directories.
pub fn find_unfinished_games(base_dir: &Path) -> Result<Vec<PathBuf>, StateError> {
    let mut unfinished = Vec::new();

    if !base_dir.exists() {
        return Ok(unfinished);
    }

    let entries = std::fs::read_dir(base_dir)?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir()
            && let Ok(content) = std::fs::read_to_string(path.join("game.json"))
            && let Ok(state) = serde_json::from_str::<GameState>(&content)
            && !state.finished
        {
            unfinished.push(path);
        }
    }

    unfinished.sort();
    Ok(unfinished)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{GameInfo, GameState};
    use crate::persist::save_game_state;
    use tempfile::TempDir;

    fn make_info(date: &str, home: &str, away: &str, game_number: u32) -> GameInfo {
        GameInfo {
            date: date.to_string(),
            home_team: home.to_string(),
            away_team: away.to_string(),
            sport: "hockey".to_string(),
            game_number,
            venue: String::new(),
            game_time: String::new(),
            period_length: 0,
            description: String::new(),
            thumbnail: String::new(),
            level: String::new(),
            home_slug: String::new(),
            away_slug: String::new(),
            tournament: String::new(),
        }
    }

    fn make_state(info: GameInfo, finished: bool) -> GameState {
        GameState {
            game_info: info,
            segments_processed: vec![],
            highlighted: false,
            finished,
            created_at: String::new(),
            finished_at: String::new(),
            renders: vec![],
            events: vec![],
            livestreams: std::collections::HashMap::new(),
            segment_outputs: vec![],
            highlights_output: String::new(),
        }
    }

    #[test]
    fn test_game_dir_name_basic() {
        let name = game_dir_name("2026-02-26", "roseville", "mahtomedi", 1);
        assert_eq!(name, "2026-02-26_roseville_vs_mahtomedi");
    }

    #[test]
    fn test_game_dir_name_double_header() {
        let name = game_dir_name("2026-02-26", "a", "b", 2);
        assert_eq!(name, "2026-02-26_a_vs_b_g2");

        let name = game_dir_name("2026-02-26", "a", "b", 3);
        assert_eq!(name, "2026-02-26_a_vs_b_g3");
    }

    #[test]
    fn test_game_dir_name_no_slug_transform() {
        // Caller is responsible for formatting — no lowercasing or space replacement
        let name = game_dir_name("2026-02-26", "Home Team", "Away Team", 1);
        assert_eq!(name, "2026-02-26_Home Team_vs_Away Team");
    }

    #[test]
    fn test_detect_next_game_number_empty() {
        let tmp = TempDir::new().unwrap();
        let num = detect_next_game_number(tmp.path(), "2026-02-26", "Home", "Away");
        assert_eq!(num, 1);
    }

    #[test]
    fn test_detect_next_game_number_one_existing() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("2026-02-26_a_vs_b")).unwrap();
        let num = detect_next_game_number(tmp.path(), "2026-02-26", "a", "b");
        assert_eq!(num, 2);
    }

    #[test]
    fn test_detect_next_game_number_double_header() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("2026-02-26_a_vs_b")).unwrap();
        std::fs::create_dir(tmp.path().join("2026-02-26_a_vs_b_g2")).unwrap();
        let num = detect_next_game_number(tmp.path(), "2026-02-26", "a", "b");
        assert_eq!(num, 3);
    }

    #[test]
    fn test_detect_next_game_number_nonexistent_dir() {
        let tmp = TempDir::new().unwrap();
        let num = detect_next_game_number(
            &tmp.path().join("nonexistent"),
            "2026-02-26",
            "Home",
            "Away",
        );
        assert_eq!(num, 1);
    }

    #[test]
    fn test_create_game_directory() {
        let tmp = TempDir::new().unwrap();
        let info = make_info("2026-02-26", "roseville", "mahtomedi", 1);
        let dir = create_game_directory(tmp.path(), &info).unwrap();
        assert!(dir.exists());
        assert_eq!(
            dir.file_name().unwrap(),
            "2026-02-26_roseville_vs_mahtomedi"
        );
    }

    #[test]
    fn test_create_game_directory_already_exists() {
        let tmp = TempDir::new().unwrap();
        let info = make_info("2026-02-26", "a", "b", 1);
        create_game_directory(tmp.path(), &info).unwrap();
        let result = create_game_directory(tmp.path(), &info);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn test_create_game_directory_double_header() {
        let tmp = TempDir::new().unwrap();
        let info = make_info("2026-02-26", "a", "b", 2);
        let dir = create_game_directory(tmp.path(), &info).unwrap();
        assert_eq!(dir.file_name().unwrap(), "2026-02-26_a_vs_b_g2");
    }

    #[test]
    fn test_find_unfinished_games_empty() {
        let tmp = TempDir::new().unwrap();
        let games = find_unfinished_games(tmp.path()).unwrap();
        assert!(games.is_empty());
    }

    #[test]
    fn test_find_unfinished_games_nonexistent_dir() {
        let tmp = TempDir::new().unwrap();
        let games = find_unfinished_games(&tmp.path().join("nonexistent")).unwrap();
        assert!(games.is_empty());
    }

    #[test]
    fn test_find_unfinished_games() {
        let tmp = TempDir::new().unwrap();

        // Create an unfinished game
        let info1 = make_info("2026-02-26", "Home", "Away", 1);
        let dir1 = create_game_directory(tmp.path(), &info1).unwrap();
        let state1 = make_state(info1, false);
        save_game_state(&state1, &dir1).unwrap();

        // Create a finished game
        let info2 = make_info("2026-02-27", "Home", "Away", 1);
        let dir2 = create_game_directory(tmp.path(), &info2).unwrap();
        let state2 = make_state(info2, true);
        save_game_state(&state2, &dir2).unwrap();

        let games = find_unfinished_games(tmp.path()).unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(games[0], dir1);
    }

    #[test]
    fn test_find_unfinished_games_ignores_non_dirs() {
        let tmp = TempDir::new().unwrap();
        // Create a file (not a directory)
        std::fs::write(tmp.path().join("not_a_game.json"), "{}").unwrap();
        let games = find_unfinished_games(tmp.path()).unwrap();
        assert!(games.is_empty());
    }

    #[test]
    fn test_find_unfinished_games_ignores_invalid_json() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("bad_game");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("game.json"), "not valid json").unwrap();
        let games = find_unfinished_games(tmp.path()).unwrap();
        assert!(games.is_empty());
    }
}
