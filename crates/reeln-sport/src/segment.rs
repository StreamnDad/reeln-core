use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::SportError;
use crate::registry::SportAlias;

/// A game segment (e.g., period, half, quarter, inning).
///
/// Equivalent to the Python `Segment` dataclass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    /// 1-indexed segment number.
    pub number: u32,
    /// Directory name alias, e.g., "period-1".
    pub alias: String,
    /// Video files belonging to this segment.
    pub files: Vec<PathBuf>,
    /// Path to the merged output file, if any.
    pub merged_path: Option<PathBuf>,
}

/// Build the directory name for a segment, e.g., "period-1".
#[must_use]
pub fn segment_dir_name(sport: &SportAlias, number: u32) -> String {
    format!("{}-{number}", sport.segment_name)
}

/// Build the display name for a segment, e.g., "Period 1".
#[must_use]
pub fn segment_display_name(sport: &SportAlias, number: u32) -> String {
    let label = &sport.segment_name;
    let mut chars = label.chars();
    let capitalized = match chars.next() {
        None => String::new(),
        Some(c) => {
            let upper: String = c.to_uppercase().collect();
            format!("{upper}{}", chars.as_str())
        }
    };
    format!("{capitalized} {number}")
}

/// Validate that a segment number is >= 1.
pub fn validate_segment_number(number: u32) -> Result<(), SportError> {
    if number < 1 {
        return Err(SportError::InvalidSegment(
            "segment number must be >= 1".into(),
        ));
    }
    Ok(())
}

/// Validate a segment number against a sport's expected count.
///
/// Returns a list of warning strings. An empty list means no warnings.
/// This is non-blocking: exceeding the expected count is a warning, not
/// an error.
#[must_use]
pub fn validate_segment_for_sport(sport: &SportAlias, number: u32) -> Vec<String> {
    let mut warnings = Vec::new();
    if number > sport.segment_count {
        warnings.push(format!(
            "segment {number} exceeds expected count of {} for {}",
            sport.segment_count, sport.sport
        ));
    }
    warnings
}

/// Create a single `Segment` with a validated alias.
pub fn make_segment(sport: &SportAlias, number: u32) -> Result<Segment, SportError> {
    validate_segment_number(number)?;
    Ok(Segment {
        number,
        alias: segment_dir_name(sport, number),
        files: Vec::new(),
        merged_path: None,
    })
}

/// Create all segments for a sport.
///
/// If `count` is `None`, uses the sport's `segment_count`.
pub fn make_segments(sport: &SportAlias, count: Option<u32>) -> Result<Vec<Segment>, SportError> {
    let n = count.unwrap_or(sport.segment_count);
    (1..=n).map(|i| make_segment(sport, i)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hockey() -> SportAlias {
        SportAlias {
            sport: "hockey".into(),
            segment_name: "period".into(),
            segment_count: 3,
            duration_minutes: Some(20),
        }
    }

    fn baseball() -> SportAlias {
        SportAlias {
            sport: "baseball".into(),
            segment_name: "inning".into(),
            segment_count: 9,
            duration_minutes: None,
        }
    }

    fn generic() -> SportAlias {
        SportAlias {
            sport: "generic".into(),
            segment_name: "segment".into(),
            segment_count: 1,
            duration_minutes: None,
        }
    }

    #[test]
    fn test_segment_dir_name() {
        assert_eq!(segment_dir_name(&hockey(), 1), "period-1");
        assert_eq!(segment_dir_name(&hockey(), 3), "period-3");
        assert_eq!(segment_dir_name(&baseball(), 9), "inning-9");
    }

    #[test]
    fn test_segment_display_name() {
        assert_eq!(segment_display_name(&hockey(), 1), "Period 1");
        assert_eq!(segment_display_name(&hockey(), 3), "Period 3");
        assert_eq!(segment_display_name(&baseball(), 5), "Inning 5");
    }

    #[test]
    fn test_segment_display_name_empty_label() {
        let sport = SportAlias {
            sport: "test".into(),
            segment_name: String::new(),
            segment_count: 1,
            duration_minutes: None,
        };
        assert_eq!(segment_display_name(&sport, 1), " 1");
    }

    #[test]
    fn test_validate_segment_number_valid() {
        assert!(validate_segment_number(1).is_ok());
        assert!(validate_segment_number(100).is_ok());
    }

    #[test]
    fn test_validate_segment_number_zero() {
        let err = validate_segment_number(0).unwrap_err();
        assert_eq!(
            err,
            SportError::InvalidSegment("segment number must be >= 1".into())
        );
    }

    #[test]
    fn test_validate_segment_for_sport_within_range() {
        let warnings = validate_segment_for_sport(&hockey(), 2);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_validate_segment_for_sport_at_boundary() {
        let warnings = validate_segment_for_sport(&hockey(), 3);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_validate_segment_for_sport_exceeds() {
        let warnings = validate_segment_for_sport(&hockey(), 4);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("exceeds expected count of 3"));
        assert!(warnings[0].contains("hockey"));
    }

    #[test]
    fn test_make_segment() {
        let seg = make_segment(&hockey(), 1).unwrap();
        assert_eq!(seg.number, 1);
        assert_eq!(seg.alias, "period-1");
        assert!(seg.files.is_empty());
        assert!(seg.merged_path.is_none());
    }

    #[test]
    fn test_make_segment_zero() {
        let err = make_segment(&hockey(), 0).unwrap_err();
        assert_eq!(
            err,
            SportError::InvalidSegment("segment number must be >= 1".into())
        );
    }

    #[test]
    fn test_make_segments_default_count() {
        let segs = make_segments(&hockey(), None).unwrap();
        assert_eq!(segs.len(), 3);
        assert_eq!(segs[0].alias, "period-1");
        assert_eq!(segs[1].alias, "period-2");
        assert_eq!(segs[2].alias, "period-3");
    }

    #[test]
    fn test_make_segments_custom_count() {
        let segs = make_segments(&hockey(), Some(5)).unwrap();
        assert_eq!(segs.len(), 5);
        assert_eq!(segs[4].alias, "period-5");
    }

    #[test]
    fn test_make_segments_generic() {
        let segs = make_segments(&generic(), None).unwrap();
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].alias, "segment-1");
    }

    #[test]
    fn test_make_segments_baseball() {
        let segs = make_segments(&baseball(), None).unwrap();
        assert_eq!(segs.len(), 9);
        assert_eq!(segs[0].alias, "inning-1");
        assert_eq!(segs[8].alias, "inning-9");
    }

    #[test]
    fn test_segment_serde_roundtrip() {
        let seg = Segment {
            number: 2,
            alias: "period-2".into(),
            files: vec![PathBuf::from("/tmp/clip1.mp4")],
            merged_path: Some(PathBuf::from("/tmp/merged.mp4")),
        };
        let json = serde_json::to_string(&seg).unwrap();
        let deserialized: Segment = serde_json::from_str(&json).unwrap();
        assert_eq!(seg, deserialized);
    }

    #[test]
    fn test_segment_serde_empty_fields() {
        let seg = Segment {
            number: 1,
            alias: "period-1".into(),
            files: Vec::new(),
            merged_path: None,
        };
        let json = serde_json::to_string(&seg).unwrap();
        let deserialized: Segment = serde_json::from_str(&json).unwrap();
        assert_eq!(seg, deserialized);
    }

    #[test]
    fn test_segment_clone_and_eq() {
        let seg = make_segment(&hockey(), 1).unwrap();
        let cloned = seg.clone();
        assert_eq!(seg, cloned);
    }

    #[test]
    fn test_segment_debug() {
        let seg = make_segment(&hockey(), 1).unwrap();
        let debug = format!("{seg:?}");
        assert!(debug.contains("period-1"));
    }
}
