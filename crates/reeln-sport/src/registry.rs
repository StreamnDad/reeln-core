use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::SportError;

/// A sport definition with segment naming conventions.
///
/// Equivalent to the Python `SportAlias` frozen dataclass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SportAlias {
    /// Canonical sport name, e.g. "hockey".
    pub sport: String,
    /// Segment label, e.g. "period", "quarter".
    pub segment_name: String,
    /// Expected number of segments.
    pub segment_count: u32,
    /// Optional segment duration in minutes.
    pub duration_minutes: Option<u32>,
}

/// Registry of built-in and custom sport definitions.
#[derive(Debug, Clone)]
pub struct SportRegistry {
    /// Maps canonical name and aliases to `SportAlias`.
    sports: HashMap<String, SportAlias>,
}

impl Default for SportRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl SportRegistry {
    /// Create a new registry pre-populated with built-in sports.
    #[must_use]
    pub fn new() -> Self {
        let mut reg = Self {
            sports: HashMap::new(),
        };
        reg.register_builtins();
        reg
    }

    fn register_builtins(&mut self) {
        let builtins: Vec<(SportAlias, Vec<&str>)> = vec![
            (
                SportAlias {
                    sport: "hockey".into(),
                    segment_name: "period".into(),
                    segment_count: 3,
                    duration_minutes: Some(20),
                },
                vec![],
            ),
            (
                SportAlias {
                    sport: "basketball".into(),
                    segment_name: "quarter".into(),
                    segment_count: 4,
                    duration_minutes: Some(12),
                },
                vec![],
            ),
            (
                SportAlias {
                    sport: "soccer".into(),
                    segment_name: "half".into(),
                    segment_count: 2,
                    duration_minutes: Some(45),
                },
                vec![],
            ),
            (
                SportAlias {
                    sport: "football".into(),
                    segment_name: "half".into(),
                    segment_count: 2,
                    duration_minutes: Some(30),
                },
                vec!["nfl", "american-football"],
            ),
            (
                SportAlias {
                    sport: "baseball".into(),
                    segment_name: "inning".into(),
                    segment_count: 9,
                    duration_minutes: None,
                },
                vec![],
            ),
            (
                SportAlias {
                    sport: "lacrosse".into(),
                    segment_name: "quarter".into(),
                    segment_count: 4,
                    duration_minutes: Some(12),
                },
                vec![],
            ),
            (
                SportAlias {
                    sport: "generic".into(),
                    segment_name: "segment".into(),
                    segment_count: 1,
                    duration_minutes: None,
                },
                vec![],
            ),
        ];

        for (alias, extra_aliases) in builtins {
            let key = alias.sport.clone();
            for a in &extra_aliases {
                self.sports.insert((*a).to_string(), alias.clone());
            }
            self.sports.insert(key, alias);
        }
    }

    /// Register a custom sport (can override builtins).
    pub fn register_sport(&mut self, alias: SportAlias) {
        self.sports.insert(alias.sport.clone(), alias);
    }

    /// Register a sport with additional lookup aliases.
    pub fn register_sport_with_aliases(&mut self, alias: SportAlias, aliases: &[&str]) {
        for a in aliases {
            self.sports.insert((*a).to_string(), alias.clone());
        }
        self.sports.insert(alias.sport.clone(), alias);
    }

    /// Look up a sport by name or alias. Returns `SportError::UnknownSport` if
    /// not found.
    pub fn get_sport(&self, sport: &str) -> Result<&SportAlias, SportError> {
        self.sports
            .get(&sport.to_lowercase())
            .ok_or_else(|| SportError::UnknownSport(sport.to_string()))
    }

    /// List all registered sports, sorted by canonical name.
    /// Deduplicates aliases so each sport appears once.
    #[must_use]
    pub fn list_sports(&self) -> Vec<&SportAlias> {
        let mut seen = std::collections::HashSet::new();
        let mut result: Vec<&SportAlias> = self
            .sports
            .values()
            .filter(|a| seen.insert(&a.sport))
            .collect();
        result.sort_by(|a, b| a.sport.cmp(&b.sport));
        result
    }
}

/// Return default event types for a sport.
///
/// Returns a curated list of common event types for known sports,
/// or an empty list for unknown or generic sports.
#[must_use]
pub fn default_event_types(sport: &str) -> Vec<String> {
    default_event_type_entries(sport)
        .into_iter()
        .map(|(name, _)| name)
        .collect()
}

/// Return default event types with team-specific flags.
///
/// Each entry is `(name, team_specific)`. Team-specific types have
/// Home/Away variants in the UI.
#[must_use]
pub fn default_event_type_entries(sport: &str) -> Vec<(String, bool)> {
    let entries: &[(&str, bool)] = match sport.to_lowercase().as_str() {
        "hockey" => &[
            ("goal", true),
            ("save", true),
            ("penalty", true),
            ("assist", false),
        ],
        "basketball" => &[
            ("basket", true),
            ("foul", true),
            ("turnover", true),
            ("block", true),
        ],
        "soccer" => &[
            ("goal", true),
            ("foul", true),
            ("corner", false),
            ("offside", false),
            ("save", true),
        ],
        "football" | "nfl" | "american-football" => &[
            ("touchdown", true),
            ("field-goal", true),
            ("interception", true),
            ("sack", true),
        ],
        "baseball" => &[
            ("hit", true),
            ("strikeout", true),
            ("home-run", true),
            ("catch", true),
        ],
        "lacrosse" => &[
            ("goal", true),
            ("save", true),
            ("penalty", true),
            ("ground-ball", false),
        ],
        _ => &[],
    };
    entries
        .iter()
        .map(|(name, team)| ((*name).to_string(), *team))
        .collect()
}

/// Deserialize a `SportAlias` from a JSON-like map.
///
/// Expected keys: `sport`, `segment_name`, `segment_count`,
/// `duration_minutes` (optional).
pub fn sport_from_dict(data: &serde_json::Value) -> Result<SportAlias, SportError> {
    serde_json::from_value::<SportAlias>(data.clone())
        .map_err(|e| SportError::InvalidSegment(format!("failed to deserialize sport: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_hockey() {
        let reg = SportRegistry::new();
        let s = reg.get_sport("hockey").unwrap();
        assert_eq!(s.sport, "hockey");
        assert_eq!(s.segment_name, "period");
        assert_eq!(s.segment_count, 3);
        assert_eq!(s.duration_minutes, Some(20));
    }

    #[test]
    fn test_builtin_basketball() {
        let reg = SportRegistry::new();
        let s = reg.get_sport("basketball").unwrap();
        assert_eq!(s.sport, "basketball");
        assert_eq!(s.segment_name, "quarter");
        assert_eq!(s.segment_count, 4);
        assert_eq!(s.duration_minutes, Some(12));
    }

    #[test]
    fn test_builtin_soccer() {
        let reg = SportRegistry::new();
        let s = reg.get_sport("soccer").unwrap();
        assert_eq!(s.sport, "soccer");
        assert_eq!(s.segment_name, "half");
        assert_eq!(s.segment_count, 2);
        assert_eq!(s.duration_minutes, Some(45));
    }

    #[test]
    fn test_builtin_football() {
        let reg = SportRegistry::new();
        let s = reg.get_sport("football").unwrap();
        assert_eq!(s.sport, "football");
        assert_eq!(s.segment_name, "half");
        assert_eq!(s.segment_count, 2);
        assert_eq!(s.duration_minutes, Some(30));
    }

    #[test]
    fn test_football_aliases() {
        let reg = SportRegistry::new();
        let nfl = reg.get_sport("nfl").unwrap();
        assert_eq!(nfl.sport, "football");
        let af = reg.get_sport("american-football").unwrap();
        assert_eq!(af.sport, "football");
    }

    #[test]
    fn test_builtin_baseball() {
        let reg = SportRegistry::new();
        let s = reg.get_sport("baseball").unwrap();
        assert_eq!(s.sport, "baseball");
        assert_eq!(s.segment_name, "inning");
        assert_eq!(s.segment_count, 9);
        assert_eq!(s.duration_minutes, None);
    }

    #[test]
    fn test_builtin_lacrosse() {
        let reg = SportRegistry::new();
        let s = reg.get_sport("lacrosse").unwrap();
        assert_eq!(s.sport, "lacrosse");
        assert_eq!(s.segment_name, "quarter");
        assert_eq!(s.segment_count, 4);
        assert_eq!(s.duration_minutes, Some(12));
    }

    #[test]
    fn test_builtin_generic() {
        let reg = SportRegistry::new();
        let s = reg.get_sport("generic").unwrap();
        assert_eq!(s.sport, "generic");
        assert_eq!(s.segment_name, "segment");
        assert_eq!(s.segment_count, 1);
        assert_eq!(s.duration_minutes, None);
    }

    #[test]
    fn test_unknown_sport() {
        let reg = SportRegistry::new();
        let err = reg.get_sport("curling").unwrap_err();
        assert_eq!(err, SportError::UnknownSport("curling".to_string()));
        assert_eq!(err.to_string(), "unknown sport: curling");
    }

    #[test]
    fn test_case_insensitive_lookup() {
        let reg = SportRegistry::new();
        let s = reg.get_sport("HOCKEY").unwrap();
        assert_eq!(s.sport, "hockey");
    }

    #[test]
    fn test_register_custom_sport() {
        let mut reg = SportRegistry::new();
        let custom = SportAlias {
            sport: "cricket".into(),
            segment_name: "innings".into(),
            segment_count: 2,
            duration_minutes: None,
        };
        reg.register_sport(custom);
        let s = reg.get_sport("cricket").unwrap();
        assert_eq!(s.sport, "cricket");
        assert_eq!(s.segment_count, 2);
    }

    #[test]
    fn test_register_sport_overrides_builtin() {
        let mut reg = SportRegistry::new();
        let custom = SportAlias {
            sport: "hockey".into(),
            segment_name: "period".into(),
            segment_count: 4,
            duration_minutes: Some(15),
        };
        reg.register_sport(custom);
        let s = reg.get_sport("hockey").unwrap();
        assert_eq!(s.segment_count, 4);
        assert_eq!(s.duration_minutes, Some(15));
    }

    #[test]
    fn test_register_sport_with_aliases() {
        let mut reg = SportRegistry::new();
        let custom = SportAlias {
            sport: "rugby".into(),
            segment_name: "half".into(),
            segment_count: 2,
            duration_minutes: Some(40),
        };
        reg.register_sport_with_aliases(custom, &["rugby-union", "xv"]);
        assert_eq!(reg.get_sport("rugby").unwrap().sport, "rugby");
        assert_eq!(reg.get_sport("rugby-union").unwrap().sport, "rugby");
        assert_eq!(reg.get_sport("xv").unwrap().sport, "rugby");
    }

    #[test]
    fn test_list_sports_sorted_and_deduplicated() {
        let reg = SportRegistry::new();
        let list = reg.list_sports();
        let names: Vec<&str> = list.iter().map(|a| a.sport.as_str()).collect();
        // Should be sorted and each sport appears exactly once.
        assert!(names.contains(&"hockey"));
        assert!(names.contains(&"basketball"));
        assert!(names.contains(&"soccer"));
        assert!(names.contains(&"football"));
        assert!(names.contains(&"baseball"));
        assert!(names.contains(&"lacrosse"));
        assert!(names.contains(&"generic"));
        // Verify sorted
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
        // Verify no duplicates
        let unique: std::collections::HashSet<&&str> = names.iter().collect();
        assert_eq!(unique.len(), names.len());
    }

    #[test]
    fn test_sport_from_dict_valid() {
        let data = serde_json::json!({
            "sport": "tennis",
            "segment_name": "set",
            "segment_count": 5,
            "duration_minutes": null
        });
        let alias = sport_from_dict(&data).unwrap();
        assert_eq!(alias.sport, "tennis");
        assert_eq!(alias.segment_name, "set");
        assert_eq!(alias.segment_count, 5);
        assert_eq!(alias.duration_minutes, None);
    }

    #[test]
    fn test_sport_from_dict_with_duration() {
        let data = serde_json::json!({
            "sport": "hockey",
            "segment_name": "period",
            "segment_count": 3,
            "duration_minutes": 20
        });
        let alias = sport_from_dict(&data).unwrap();
        assert_eq!(alias.duration_minutes, Some(20));
    }

    #[test]
    fn test_sport_from_dict_missing_field() {
        let data = serde_json::json!({
            "sport": "hockey"
        });
        let err = sport_from_dict(&data).unwrap_err();
        match err {
            SportError::InvalidSegment(msg) => {
                assert!(msg.contains("failed to deserialize sport"));
            }
            _ => panic!("expected InvalidSegment error"),
        }
    }

    #[test]
    fn test_default_trait() {
        let reg = SportRegistry::default();
        // Default should also have builtins
        assert!(reg.get_sport("hockey").is_ok());
    }

    #[test]
    fn test_sport_alias_serde_roundtrip() {
        let alias = SportAlias {
            sport: "hockey".into(),
            segment_name: "period".into(),
            segment_count: 3,
            duration_minutes: Some(20),
        };
        let json = serde_json::to_string(&alias).unwrap();
        let deserialized: SportAlias = serde_json::from_str(&json).unwrap();
        assert_eq!(alias, deserialized);
    }

    #[test]
    fn test_default_event_types_hockey() {
        let types = default_event_types("hockey");
        assert_eq!(types, vec!["goal", "save", "penalty", "assist"]);
    }

    #[test]
    fn test_default_event_types_basketball() {
        let types = default_event_types("basketball");
        assert_eq!(types, vec!["basket", "foul", "turnover", "block"]);
    }

    #[test]
    fn test_default_event_types_soccer() {
        let types = default_event_types("soccer");
        assert_eq!(types, vec!["goal", "foul", "corner", "offside", "save"]);
    }

    #[test]
    fn test_default_event_types_football() {
        let types = default_event_types("football");
        assert_eq!(
            types,
            vec!["touchdown", "field-goal", "interception", "sack"]
        );
    }

    #[test]
    fn test_default_event_types_football_aliases() {
        assert_eq!(default_event_types("nfl"), default_event_types("football"));
        assert_eq!(
            default_event_types("american-football"),
            default_event_types("football")
        );
    }

    #[test]
    fn test_default_event_types_baseball() {
        let types = default_event_types("baseball");
        assert_eq!(types, vec!["hit", "strikeout", "home-run", "catch"]);
    }

    #[test]
    fn test_default_event_types_lacrosse() {
        let types = default_event_types("lacrosse");
        assert_eq!(types, vec!["goal", "save", "penalty", "ground-ball"]);
    }

    #[test]
    fn test_default_event_types_generic_empty() {
        assert!(default_event_types("generic").is_empty());
    }

    #[test]
    fn test_default_event_types_unknown_sport_empty() {
        assert!(default_event_types("curling").is_empty());
    }

    #[test]
    fn test_default_event_types_case_insensitive() {
        assert_eq!(default_event_types("HOCKEY"), default_event_types("hockey"));
    }

    #[test]
    fn test_sport_alias_clone_and_eq() {
        let a = SportAlias {
            sport: "hockey".into(),
            segment_name: "period".into(),
            segment_count: 3,
            duration_minutes: Some(20),
        };
        let b = a.clone();
        assert_eq!(a, b);
    }
}
